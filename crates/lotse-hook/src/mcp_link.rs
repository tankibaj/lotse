//! The MCP server's connection to `lotsed`: one Unix-socket connection kept open, `Hello` with
//! `client` = `mcp:<session id>` (the daemon's allowlist for agents, see `lotsed::child_policy`), the
//! token read from its file at connect time (never from the environment or an argument).

use lotse_proto::{
    Frame, FrameError, Hello, Message, PROTO_VERSION, Request, RequestEnvelope, Response,
    ResponseEnvelope, read_frame, write_frame,
};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// What the tools need from the daemon; the real one is [`Link`].
pub trait Daemon {
    /// Send `request`, wait at most `timeout` for its answer.
    fn call(&mut self, request: Request, timeout: Duration) -> Result<Response, String>;
}

pub struct Link {
    socket: PathBuf,
    token_file: PathBuf,
    client: String,
    conn: Option<UnixStream>,
    next_id: u64,
}

/// How long connecting and the hello may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Why a request got no answer.
enum Failed {
    /// The daemon closed the connection (a reused one may simply have gone stale).
    Closed,
    Other(String),
}

impl Link {
    pub fn new(socket: PathBuf, token_file: PathBuf, session_id: &str) -> Link {
        Link {
            socket,
            token_file,
            client: format!("mcp:{session_id}"),
            conn: None,
            next_id: 0,
        }
    }

    fn connect(&self) -> Result<UnixStream, String> {
        let token = std::fs::read_to_string(&self.token_file).map_err(|e| {
            format!(
                "cannot read the token file {}: {e}",
                self.token_file.display()
            )
        })?;
        let mut stream = UnixStream::connect(&self.socket)
            .map_err(|e| format!("cannot connect to {}: {e}", self.socket.display()))?;
        let _ = stream.set_write_timeout(Some(CONNECT_TIMEOUT));
        let hello = Frame::Control(Message::Hello(Hello {
            proto_version: PROTO_VERSION,
            token: token.trim().to_owned(),
            client: self.client.clone(),
        }));
        write_frame(&mut stream, &hello).map_err(|e| format!("cannot greet the daemon: {e}"))?;
        let deadline = Instant::now() + CONNECT_TIMEOUT;
        loop {
            match read_before(&mut stream, deadline) {
                Ok(Frame::Control(Message::HelloOk(_))) => return Ok(stream),
                Ok(Frame::Control(Message::Response(ResponseEnvelope {
                    response: Response::Error(e),
                    ..
                }))) => return Err(format!("the daemon refused the connection: {}", e.message)),
                Ok(_) => {}
                Err(Failed::Closed) => return Err("the daemon closed the connection".into()),
                Err(Failed::Other(e)) => return Err(e),
            }
        }
    }

    fn exchange(
        stream: &mut UnixStream,
        req_id: u64,
        request: &Request,
        timeout: Duration,
    ) -> Result<Response, Failed> {
        let frame = Frame::Control(Message::Request(RequestEnvelope {
            req_id,
            request: request.clone(),
        }));
        let _ = stream.set_write_timeout(Some(CONNECT_TIMEOUT));
        write_frame(stream, &frame).map_err(|_| Failed::Closed)?;
        let deadline = Instant::now() + timeout;
        loop {
            // Pushed events and PTY frames are not for us.
            if let Frame::Control(Message::Response(r)) = read_before(stream, deadline)?
                && r.req_id == req_id
            {
                return Ok(r.response);
            }
        }
    }
}

/// The next frame, waiting until `deadline` at most.
fn read_before(stream: &mut UnixStream, deadline: Instant) -> Result<Frame, Failed> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(Failed::Other("no answer from Lotse in time".into()));
    }
    // An error here means the peer is gone; the read below reports that.
    let _ = stream.set_read_timeout(Some(left));
    match read_frame(stream) {
        Ok(Some(frame)) => Ok(frame),
        Ok(None) => Err(Failed::Closed),
        Err(FrameError::Io(e))
            if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            Err(Failed::Other("no answer from Lotse in time".into()))
        }
        Err(e) => Err(Failed::Other(format!("cannot read from Lotse: {e}"))),
    }
}

impl Daemon for Link {
    fn call(&mut self, request: Request, timeout: Duration) -> Result<Response, String> {
        let mut retried = false;
        loop {
            let reused = self.conn.is_some();
            let mut stream = match self.conn.take() {
                Some(s) => s,
                None => self.connect()?,
            };
            self.next_id += 1;
            match Link::exchange(&mut stream, self.next_id, &request, timeout) {
                Ok(response) => {
                    self.conn = Some(stream);
                    return Ok(response);
                }
                // a connection the daemon dropped since the last call: connect again, once
                Err(Failed::Closed) if reused && !retried => retried = true,
                Err(Failed::Closed) => return Err("the daemon closed the connection".into()),
                Err(Failed::Other(e)) => return Err(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lotse_proto::{
        ErrorCode, Frame, HelloOk, Message, PROTO_VERSION, ProtoError, ResponseEnvelope,
        read_frame, write_frame,
    };
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;

    /// Fake daemon: every connection is one `hello`, then requests answered by `answer`.
    struct Fake {
        dir: tempfile::TempDir,
        hellos: mpsc::Receiver<(String, String)>,
        requests: mpsc::Receiver<Request>,
    }

    fn fake(
        answer: impl Fn(&Request) -> Option<Response> + Send + 'static,
        drop_after: Option<usize>,
    ) -> Fake {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("token"), "s3cret\n").unwrap();
        let listener = UnixListener::bind(dir.path().join("lotsed.sock")).unwrap();
        let (hello_tx, hellos) = mpsc::channel();
        let (req_tx, requests) = mpsc::channel();
        std::thread::spawn(move || {
            for (n, conn) in listener.incoming().enumerate() {
                let Ok(mut conn) = conn else { return };
                let Ok(Some(Frame::Control(Message::Hello(h)))) = read_frame(&mut conn) else {
                    continue;
                };
                let _ = hello_tx.send((h.token, h.client));
                let ok = Message::HelloOk(HelloOk {
                    proto_version: PROTO_VERSION,
                    daemon_version: "t".into(),
                });
                write_frame(&mut conn, &Frame::Control(ok)).unwrap();
                let mut served = 0;
                while let Ok(Some(Frame::Control(Message::Request(env)))) = read_frame(&mut conn) {
                    let _ = req_tx.send(env.request.clone());
                    // a pushed event first: the link must skip it
                    let ev = Message::Event(lotse_proto::Event::SessionClosed {
                        session_id: lotse_proto::SessionId::new("x"),
                    });
                    write_frame(&mut conn, &Frame::Control(ev)).unwrap();
                    if let Some(response) = answer(&env.request) {
                        let m = Message::Response(ResponseEnvelope {
                            req_id: env.req_id,
                            response,
                        });
                        write_frame(&mut conn, &Frame::Control(m)).unwrap();
                    }
                    served += 1;
                    if drop_after == Some(served) && n == 0 {
                        break;
                    }
                }
            }
        });
        Fake {
            dir,
            hellos,
            requests,
        }
    }

    fn link(f: &Fake) -> Link {
        Link::new(
            f.dir.path().join("lotsed.sock"),
            f.dir.path().join("token"),
            "s-1",
        )
    }

    const T: Duration = Duration::from_secs(2);

    #[test]
    fn the_hello_names_the_session_and_carries_the_token_from_the_file() {
        let f = fake(|_| Some(Response::Pong), None);
        let mut l = link(&f);
        assert_eq!(l.call(Request::Ping, T), Ok(Response::Pong));
        assert_eq!(
            f.hellos.recv().unwrap(),
            ("s3cret".to_owned(), "mcp:s-1".to_owned())
        );
        // the same connection serves the next call
        assert_eq!(l.call(Request::Ping, T), Ok(Response::Pong));
        assert!(f.hellos.try_recv().is_err(), "no second hello");
        assert_eq!(f.requests.try_iter().count(), 2);
    }

    #[test]
    fn a_connection_the_daemon_dropped_is_replaced_for_the_next_call() {
        let f = fake(|_| Some(Response::Pong), Some(1));
        let mut l = link(&f);
        assert_eq!(l.call(Request::Ping, T), Ok(Response::Pong));
        // the daemon closed the first connection after one answer
        assert_eq!(l.call(Request::Ping, T), Ok(Response::Pong));
        assert_eq!(f.hellos.try_iter().count(), 2);
    }

    #[test]
    fn errors_come_back_as_responses_and_a_silent_daemon_times_out() {
        let f = fake(
            |r| match r {
                Request::Shutdown => None,
                _ => Some(Response::Error(ProtoError {
                    code: ErrorCode::Unauthorized,
                    message: "no".into(),
                })),
            },
            None,
        );
        let mut l = link(&f);
        let Ok(Response::Error(e)) = l.call(Request::GetSnapshot, T) else {
            panic!("the daemon's refusal is an answer, not a failure");
        };
        assert_eq!(e.code, ErrorCode::Unauthorized);
        let started = std::time::Instant::now();
        let err = l
            .call(Request::Shutdown, Duration::from_millis(200))
            .unwrap_err();
        assert!(err.contains("no answer"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn no_daemon_and_no_token_are_errors_not_hangs() {
        let dir = tempfile::tempdir().unwrap();
        let mut l = Link::new(
            dir.path().join("lotsed.sock"),
            dir.path().join("token"),
            "s-1",
        );
        assert!(l.call(Request::Ping, T).unwrap_err().contains("token"));
        std::fs::write(dir.path().join("token"), "t").unwrap();
        assert!(l.call(Request::Ping, T).unwrap_err().contains("connect"));
    }
}
