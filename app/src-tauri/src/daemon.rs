//! The bridge between the webview and `lotsed`.
//!
//! One Unix-socket connection to the daemon, owned by [`Bridge`] (no TCP, ever):
//!
//! - **Requests**: the webview sends the proto `Request` JSON through `daemon_request`; the answer is
//!   the proto `Response` JSON (`{kind, data}`), matched by `req_id`.
//! - **Events**: every pushed `Event` is emitted to the webview as `lotse://event` (and handed to
//!   [`crate::notify::on_event`]). Link changes go out as `lotse://connection`
//!   (`"disconnected"` / `"reconnected"`).
//! - **PTY**: daemon → webview terminal bytes travel through a `tauri::ipc::Channel` the webview passes
//!   to `daemon_connect`, each message `u16 BE id_len | session id | bytes` (the proto PTY payload);
//!   webview → daemon keystrokes use `daemon_pty_write` (raw body, session id in a header).
//! - **Lifecycle**: the daemon is started when the socket is not there (`lotsed --detach`) and again
//!   after it died; sessions live in the daemon, so quitting the app never kills them.
//!
//! Everything that touches Tauri sits behind [`Sink`], so the bridge is tested against the real
//! daemon without a window (`tests/bridge.rs`).
//!
//! Tauri API assumptions (Unverified, no web access): `Channel::send(InvokeResponseBody::Raw)` and raw
//! `invoke` bodies with custom headers, see <https://v2.tauri.app/develop/calling-rust/#channels> and
//! <https://v2.tauri.app/develop/calling-rust/#accessing-the-request-object>.

use crate::notify;
use lotse_proto::{
    Event, Frame, Hello, HelloOk, Message, NotificationSettings, PROTO_VERSION, Request,
    RequestEnvelope, Response, SessionId, encode, read_frame, write_frame,
};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;
use tauri::ipc::{Channel, InvokeBody, InvokeResponseBody, Request as IpcRequest};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tokio::sync::oneshot;

/// Environment override of the data directory (dev, tests); the daemon honours it too.
pub const DATA_DIR_ENV: &str = "LOTSE_DATA_DIR";
/// Environment override of the daemon binary.
pub const DAEMON_BIN_ENV: &str = "LOTSE_DAEMON_BIN";
const EVENT_NAME: &str = "lotse://event";
const CONNECTION_NAME: &str = "lotse://connection";
/// Header carrying the session id of a raw `daemon_pty_write` body.
const SESSION_HEADER: &str = "x-lotse-session";
/// Longest wait for an answer (`create_session` may wait for the login-shell environment).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// Where the bridge delivers what the daemon pushes. [`TauriSink`] in the app, a recorder in tests.
pub trait Sink: Send + Sync + 'static {
    fn event(&self, event: &Event);
    /// `"disconnected"` or `"reconnected"`.
    fn connection(&self, state: &'static str);
    /// Terminal output of an attached session.
    fn pty(&self, session_id: &SessionId, data: &[u8]);
    /// The notification toggles (pause bell included) after a (re)connection.
    fn notification_settings(&self, settings: NotificationSettings);
}

/// The app's sink: webview events, the PTY channel, and the notifier.
pub struct TauriSink {
    app: AppHandle<Wry>,
    pty: Mutex<Option<Channel<InvokeResponseBody>>>,
}

impl TauriSink {
    pub fn new(app: AppHandle<Wry>) -> Self {
        Self {
            app,
            pty: Mutex::new(None),
        }
    }

    fn set_channel(&self, channel: Channel<InvokeResponseBody>) {
        *lock(&self.pty) = Some(channel);
    }
}

impl Sink for TauriSink {
    fn event(&self, event: &Event) {
        notify::on_event(&self.app, event);
        if let Err(e) = self.app.emit(EVENT_NAME, event) {
            tracing::warn!("emitting {}: {e}", event.name());
        }
    }

    fn connection(&self, state: &'static str) {
        let _ = self.app.emit(CONNECTION_NAME, state);
    }

    fn pty(&self, session_id: &SessionId, data: &[u8]) {
        if let Some(channel) = lock(&self.pty).as_ref()
            && let Err(e) = channel.send(InvokeResponseBody::Raw(pty_message(session_id, data)))
        {
            tracing::warn!("PTY channel: {e}");
        }
    }

    fn notification_settings(&self, settings: NotificationSettings) {
        self.app
            .state::<notify::NotifyState>()
            .set_settings(settings);
    }
}

/// Where the daemon lives and how to start it.
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// `None`: `$LOTSE_DATA_DIR`, else `~/Library/Application Support/Lotse`.
    pub data_dir: Option<PathBuf>,
    /// `None`: `$LOTSE_DAEMON_BIN`, else `lotsed` next to the app executable.
    pub daemon_bin: Option<PathBuf>,
    /// Extra `lotsed` arguments (tests: `--no-login-env`).
    pub daemon_args: Vec<String>,
}

struct Link {
    writer: Mutex<UnixStream>,
    generation: u64,
}

#[derive(Default)]
pub struct Bridge {
    config: Config,
    sink: OnceLock<Arc<dyn Sink>>,
    link: Mutex<Option<Arc<Link>>>,
    hello: Mutex<Option<HelloOk>>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Response>>>,
    next_id: AtomicU64,
    generation: AtomicU64,
    reconnecting: AtomicBool,
    closed: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---- paths and the daemon process ---------------------------------------------------------------

/// `$LOTSE_DATA_DIR`, else `~/Library/Application Support/Lotse`.
pub fn data_dir() -> Option<PathBuf> {
    data_dir_with(std::env::var_os(DATA_DIR_ENV))
}

fn data_dir_with(override_dir: Option<std::ffi::OsString>) -> Option<PathBuf> {
    match override_dir.filter(|v| !v.is_empty()) {
        Some(d) => Some(PathBuf::from(d)),
        None => dirs::data_dir().map(|d| d.join("Lotse")),
    }
}

/// A binary that ships next to the app executable (`Contents/MacOS` in the bundle, `target/<profile>`
/// in development), unless `env_var` names another one.
pub fn sibling_binary(name: &str, env_var: &str) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(env_var).filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(p));
    }
    let p = std::env::current_exe().ok()?.with_file_name(name);
    p.is_file().then_some(p)
}

impl Config {
    fn dir(&self) -> Result<PathBuf, String> {
        self.data_dir
            .clone()
            .or_else(data_dir)
            .ok_or_else(|| "cannot find the Lotse data directory".into())
    }

    /// Start `lotsed --detach` (it waits until the socket accepts connections).
    fn start_daemon(&self, dir: &Path) -> Result<(), String> {
        let bin = self
            .daemon_bin
            .clone()
            .or_else(|| sibling_binary("lotsed", DAEMON_BIN_ENV))
            .ok_or_else(|| {
                format!("lotsed not found next to the app; build it (`cargo build -p lotsed`) or set {DAEMON_BIN_ENV}")
            })?;
        let out = Command::new(&bin)
            .arg("--detach")
            .arg("--data-dir")
            .arg(dir)
            .args(&self.daemon_args)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("starting {}: {e}", bin.display()))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "lotsed did not start: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }

    /// Connect (starting the daemon when needed) and complete the `hello` handshake.
    fn establish(&self) -> Result<(UnixStream, HelloOk), String> {
        let dir = self.dir()?;
        let socket = dir.join("lotsed.sock");
        let mut stream = match UnixStream::connect(&socket) {
            Ok(s) => s,
            Err(_) => {
                self.start_daemon(&dir)?;
                UnixStream::connect(&socket)
                    .map_err(|e| format!("connecting to {}: {e}", socket.display()))?
            }
        };
        // the token is read from its file (0600) and only ever sent over the socket
        let token = std::fs::read_to_string(dir.join("token"))
            .map_err(|e| format!("reading the daemon token: {e}"))?
            .trim()
            .to_owned();
        stream
            .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
            .and_then(|()| stream.set_write_timeout(Some(WRITE_TIMEOUT)))
            .map_err(|e| e.to_string())?;
        write_frame(
            &mut stream,
            &Frame::Control(Message::Hello(Hello {
                proto_version: PROTO_VERSION,
                token,
                client: "app".into(),
            })),
        )
        .map_err(|e| format!("hello: {e}"))?;
        let hello = match read_frame(&mut stream).map_err(|e| format!("hello: {e}"))? {
            Some(Frame::Control(Message::HelloOk(ok))) => ok,
            Some(Frame::Control(Message::Response(r))) => {
                return Err(match r.response {
                    Response::Error(e) => format!("daemon refused the connection: {}", e.message),
                    _ => "unexpected answer to hello".into(),
                });
            }
            _ => return Err("the daemon closed the connection during the handshake".into()),
        };
        stream.set_read_timeout(None).map_err(|e| e.to_string())?;
        Ok((stream, hello))
    }
}

// ---- the bridge ---------------------------------------------------------------------------------

impl Bridge {
    pub fn with_config(config: Config) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }

    /// Where pushed events, terminal output and link changes go. Set once, before connecting.
    pub fn set_sink(&self, sink: Arc<dyn Sink>) {
        let _ = self.sink.set(sink);
    }

    fn sink(&self) -> Option<&Arc<dyn Sink>> {
        self.sink.get()
    }

    pub fn is_connected(&self) -> bool {
        lock(&self.link).is_some()
    }

    /// Hang up for good (the app is quitting): no reconnect, no `disconnected` event. The daemon and
    /// its sessions keep running.
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        if let Some(link) = lock(&self.link).take() {
            let _ = lock(&link.writer).shutdown(std::net::Shutdown::Both);
        }
    }

    /// Connect (starting the daemon if needed). A second call returns the same hello.
    /// Blocking: the command runs it on a worker thread.
    pub fn connect_blocking(self: &Arc<Self>) -> Result<HelloOk, String> {
        if !self.is_connected() {
            let (stream, hello) = self.config.establish()?;
            self.install(stream, hello)?;
        }
        lock(&self.hello)
            .clone()
            .ok_or_else(|| "not connected".into())
    }

    /// Install a freshly handshaken connection and start reading from it. Returns the generation of the
    /// link now in place (the one just installed, or the one that won the race).
    fn install(self: &Arc<Self>, stream: UnixStream, hello: HelloOk) -> Result<u64, String> {
        let reader = stream.try_clone().map_err(|e| e.to_string())?;
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        {
            // A connect command and the reconnect thread can both get here: the first one wins, the
            // other's stream is dropped (two links would deliver every event twice).
            let mut link = lock(&self.link);
            // Under the lock: a drop of this link is handled by `link_lost` only after this, so it
            // sees `reconnecting == false` and starts a new reconnect thread.
            self.reconnecting.store(false, Ordering::Release);
            if let Some(current) = link.as_ref() {
                return Ok(current.generation);
            }
            *link = Some(Arc::new(Link {
                writer: Mutex::new(stream),
                generation,
            }));
            *lock(&self.hello) = Some(hello);
        }
        let bridge = self.clone();
        std::thread::Builder::new()
            .name("lotse-daemon-reader".into())
            .spawn(move || bridge.read_loop(reader, generation))
            .map_err(|e| e.to_string())?;
        Ok(generation)
    }

    fn write(&self, bytes: &[u8]) -> Result<(), String> {
        let link = lock(&self.link)
            .clone()
            .ok_or("the daemon is not connected")?;
        let mut w = lock(&link.writer);
        w.write_all(bytes).and_then(|()| w.flush()).map_err(|e| {
            // a timed-out write may have left half a frame on the wire: drop the link, the
            // reader sees the end and the reconnect starts clean
            let _ = w.shutdown(std::net::Shutdown::Both);
            format!("writing to the daemon: {e}")
        })
    }

    /// Keystrokes for a session.
    pub fn write_pty(&self, session_id: &SessionId, data: &[u8]) -> Result<(), String> {
        let frame = encode(&Frame::Pty {
            session_id: session_id.clone(),
            data: data.to_vec(),
        })
        .map_err(|e| e.to_string())?;
        self.write(&frame)
    }

    /// One request, one response. Registers the answer slot before writing (the answer can only
    /// arrive after the write, but the reader thread must find the slot by then).
    pub async fn request(&self, request: Request) -> Result<Response, String> {
        let req_id = self.next_id.fetch_add(1, Ordering::AcqRel) + 1;
        let (tx, rx) = oneshot::channel();
        lock(&self.pending).insert(req_id, tx);
        let bytes = encode(&Frame::Control(Message::Request(RequestEnvelope {
            req_id,
            request,
        })))
        .map_err(|e| e.to_string())?;
        if let Err(e) = self.write(&bytes) {
            lock(&self.pending).remove(&req_id);
            return Err(e);
        }
        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err("the daemon connection was lost".into()),
            Err(_) => {
                lock(&self.pending).remove(&req_id);
                Err("the daemon did not answer in time".into())
            }
        }
    }

    fn read_loop(self: Arc<Self>, mut reader: UnixStream, generation: u64) {
        while let Ok(Some(frame)) = read_frame(&mut reader) {
            self.dispatch(frame);
        }
        self.link_lost(generation);
    }

    fn dispatch(&self, frame: Frame) {
        match frame {
            Frame::Control(Message::Response(env)) => {
                if let Some(tx) = lock(&self.pending).remove(&env.req_id) {
                    let _ = tx.send(env.response);
                }
            }
            Frame::Control(Message::Event(event)) => {
                if let Some(sink) = self.sink() {
                    sink.event(&event);
                }
            }
            Frame::Control(_) => {}
            Frame::Pty { session_id, data } => {
                if let Some(sink) = self.sink() {
                    sink.pty(&session_id, &data);
                }
            }
        }
    }

    fn link_lost(self: &Arc<Self>, generation: u64) {
        {
            let mut link = lock(&self.link);
            if link.as_ref().is_none_or(|l| l.generation != generation) {
                return; // an older connection ended; a newer one is already installed
            }
            *link = None;
        }
        lock(&self.pending).clear(); // waiting requests fail: their senders are dropped
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        if let Some(sink) = self.sink() {
            sink.connection("disconnected");
        }
        if self.reconnecting.swap(true, Ordering::AcqRel) {
            return;
        }
        let bridge = self.clone();
        if let Err(e) = std::thread::Builder::new()
            .name("lotse-daemon-reconnect".into())
            .spawn(move || bridge.reconnect())
        {
            tracing::error!("cannot start the reconnect thread: {e}");
            self.reconnecting.store(false, Ordering::Release);
        }
    }

    /// Retry with a growing pause; each attempt restarts the daemon if its socket is gone.
    fn reconnect(self: Arc<Self>) {
        let mut pause = Duration::from_millis(300);
        loop {
            std::thread::sleep(pause);
            if self.closed.load(Ordering::Acquire) {
                self.reconnecting.store(false, Ordering::Release);
                return;
            }
            pause = (pause * 2).min(Duration::from_secs(5));
            match self.config.establish() {
                Ok((stream, hello)) => {
                    let generation = match self.install(stream, hello) {
                        Ok(g) => g,
                        Err(e) => {
                            tracing::warn!("reconnect: {e}");
                            continue;
                        }
                    };
                    self.sync_notification_settings_blocking();
                    // The link may have dropped during the sync (a new reconnect thread then owns
                    // the announcement). Announce under the link lock so a "disconnected" cannot
                    // overtake it: `link_lost` clears the link under the same lock first.
                    let link = lock(&self.link);
                    if link.as_ref().is_some_and(|l| l.generation == generation)
                        && let Some(sink) = self.sink()
                    {
                        sink.connection("reconnected");
                    }
                    return;
                }
                Err(e) => tracing::debug!("reconnect: {e}"),
            }
        }
    }

    /// Hand the notification toggles to the notifier, from a plain (non-async) thread.
    fn sync_notification_settings_blocking(&self) {
        if let Ok(Response::Snapshot(s)) =
            tauri::async_runtime::block_on(self.request(Request::GetSnapshot))
            && let Some(sink) = self.sink()
        {
            sink.notification_settings(s.settings.notifications);
        }
    }

    /// Same, from async code (the connect command).
    pub async fn sync_notification_settings(&self) {
        if let Ok(Response::Snapshot(s)) = self.request(Request::GetSnapshot).await
            && let Some(sink) = self.sink()
        {
            sink.notification_settings(s.settings.notifications);
        }
    }
}

/// The webview's PTY message: `u16 BE id_len | session id | bytes` (same layout as the proto frame).
pub fn pty_message(session_id: &SessionId, data: &[u8]) -> Vec<u8> {
    let id = session_id.as_str().as_bytes();
    let mut out = Vec::with_capacity(2 + id.len() + data.len());
    out.extend_from_slice(&u16::try_from(id.len()).unwrap_or(u16::MAX).to_be_bytes());
    out.extend_from_slice(id);
    out.extend_from_slice(data);
    out
}

// ---- commands -----------------------------------------------------------------------------------

/// Connect (starting the daemon if needed). `pty` receives every terminal output message.
/// Idempotent: a second call (webview reload) swaps the PTY channel and answers the same hello.
#[tauri::command]
pub async fn daemon_connect(
    bridge: tauri::State<'_, Arc<Bridge>>,
    sink: tauri::State<'_, Arc<TauriSink>>,
    pty: Channel<InvokeResponseBody>,
) -> Result<Value, String> {
    sink.set_channel(pty);
    let b = bridge.inner().clone();
    let hello = tauri::async_runtime::spawn_blocking(move || b.connect_blocking())
        .await
        .map_err(|e| e.to_string())??;
    bridge.sync_notification_settings().await;
    serde_json::to_value(hello).map_err(|e| e.to_string())
}

/// One proto `Request` (JSON, `{method, params}`) → the proto `Response` (JSON, `{kind, data}`).
#[tauri::command]
pub async fn daemon_request(
    bridge: tauri::State<'_, Arc<Bridge>>,
    request: Value,
) -> Result<Value, String> {
    let request: Request =
        serde_json::from_value(request).map_err(|e| format!("invalid request: {e}"))?;
    let response = bridge.request(request).await?;
    serde_json::to_value(response).map_err(|e| e.to_string())
}

/// Keystrokes: raw body = the bytes, header `x-lotse-session` = the session id.
/// Not `async`: Tauri runs sync commands on the main thread in call order, which keeps keystrokes ordered.
#[tauri::command]
pub fn daemon_pty_write(
    bridge: tauri::State<'_, Arc<Bridge>>,
    request: IpcRequest<'_>,
) -> Result<(), String> {
    let InvokeBody::Raw(data) = request.body() else {
        return Err("daemon_pty_write needs a raw body".into());
    };
    let session_id = request
        .headers()
        .get(SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty())
        .ok_or("missing session header")?;
    bridge.write_pty(&SessionId::new(session_id), data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pty_message_layout_matches_the_proto_payload() {
        let id = SessionId::new("s-abc");
        let msg = pty_message(&id, b"hi\x1b[0m");
        assert_eq!(&msg[..2], &5u16.to_be_bytes());
        assert_eq!(&msg[2..7], b"s-abc");
        assert_eq!(&msg[7..], b"hi\x1b[0m");
        // the same bytes the daemon puts after the frame type byte
        let wire = encode(&Frame::Pty {
            session_id: id,
            data: b"hi\x1b[0m".to_vec(),
        })
        .unwrap();
        assert_eq!(&wire[5..], &msg[..]);
    }

    #[test]
    fn data_dir_honours_the_environment_override() {
        assert_eq!(
            data_dir_with(Some("/tmp/lotse-x".into())),
            Some(PathBuf::from("/tmp/lotse-x"))
        );
        // empty means unset: the platform default
        assert_eq!(data_dir_with(Some("".into())), data_dir_with(None));
        assert!(data_dir_with(None).is_none_or(|p| p.ends_with("Lotse")));
    }

    #[test]
    fn events_serialize_like_the_typescript_mirror() {
        let v = serde_json::to_value(Event::SessionClosed {
            session_id: SessionId::new("s-1"),
        })
        .unwrap();
        assert_eq!(v["event"], "session_closed");
        assert_eq!(v["data"]["session_id"], "s-1");
    }

    /// Proto 3: what the webview sends through `daemon_request` parses, and what the daemon pushes
    /// reaches `lotse://event` in the shape `types.ts` declares.
    #[test]
    fn proto_3_requests_parse_and_events_serialize_like_the_typescript_mirror() {
        let parse = |v: Value| serde_json::from_value::<Request>(v).unwrap();
        assert!(matches!(
            parse(serde_json::json!({"method": "create_workspace", "params": {"name": "Home"}})),
            Request::CreateWorkspace { .. }
        ));
        assert!(matches!(
            parse(
                serde_json::json!({"method": "delete_workspace", "params": {"workspace_id": "w2", "move_projects": true}})
            ),
            Request::DeleteWorkspace {
                move_projects: true,
                ..
            }
        ));
        assert!(matches!(
            parse(
                serde_json::json!({"method": "move_project", "params": {"project_id": "p1", "workspace_id": "w2"}})
            ),
            Request::MoveProject { .. }
        ));
        assert!(matches!(
            parse(serde_json::json!({"method": "get_diff", "params": {"session_id": "s-1"}})),
            Request::GetDiff { .. }
        ));
        assert!(matches!(
            parse(
                serde_json::json!({"method": "get_transcript", "params": {"session_id": "s-1", "limit": null}})
            ),
            Request::GetTranscript { limit: None, .. }
        ));

        let v = serde_json::to_value(Event::UsageChanged(lotse_proto::UsageToday {
            input_tokens: 1,
            output_tokens: 2,
            cache_read_tokens: 3,
            cache_write_tokens: 4,
            cost_micros: Some(5),
        }))
        .unwrap();
        assert_eq!(v["event"], "usage_changed");
        assert_eq!(v["data"]["cost_micros"], 5);
        let v = serde_json::to_value(Event::WorkspaceRemoved {
            workspace_id: lotse_proto::WorkspaceId::new("w2"),
        })
        .unwrap();
        assert_eq!(
            (v["event"].as_str(), v["data"]["workspace_id"].as_str()),
            (Some("workspace_removed"), Some("w2"))
        );
        assert_eq!(PROTO_VERSION, 4);
    }
}
