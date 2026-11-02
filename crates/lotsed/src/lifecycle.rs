//! Process lifecycle: single instance (pidfile + flock), the listener, detached start, shutdown.

use crate::config::Config;
use crate::db::Db;
use crate::secure;
use crate::server::handle_conn;
use crate::state::State;
use anyhow::{Context, Result, bail};
use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream as StdUnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UnixListener;
use tokio::signal::unix::{SignalKind, signal};

/// How long `--detach` waits for the daemon to accept connections.
const DETACH_WAIT: Duration = Duration::from_secs(5);

fn read_pid(file: &mut File) -> Option<u32> {
    let mut s = String::new();
    file.read_to_string(&mut s).ok()?;
    s.trim().parse().ok()
}

/// Pid of a running daemon for this data dir, if any (its pidfile is flock-ed).
pub fn running_pid(cfg: &Config) -> Option<u32> {
    let mut file = File::open(cfg.pid_path()).ok()?;
    match Flock::lock(file.try_clone().ok()?, FlockArg::LockExclusiveNonblock) {
        Ok(_free) => None,
        Err((_, Errno::EWOULDBLOCK)) => read_pid(&mut file),
        Err(_) => None,
    }
}

/// Take the single-instance lock and write our pid. The lock lives as long as the returned value.
fn acquire_pidfile(cfg: &Config) -> Result<Flock<File>> {
    let file = secure::open_private(&cfg.pid_path())?;
    let mut lock = match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
        Ok(l) => l,
        Err((mut f, Errno::EWOULDBLOCK)) => {
            let pid = read_pid(&mut f).map_or_else(String::new, |p| format!(" (pid {p})"));
            bail!("lotsed is already running{pid}");
        }
        Err((_, e)) => bail!("locking {}: {e}", cfg.pid_path().display()),
    };
    lock.set_len(0)?;
    writeln!(lock, "{}", std::process::id())?;
    lock.flush()?;
    Ok(lock)
}

fn bind_socket(cfg: &Config) -> Result<UnixListener> {
    let path = cfg.socket_path();
    // we hold the pidfile lock, so a leftover socket file is stale
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("removing stale {}", path.display())),
    }
    let listener =
        UnixListener::bind(&path).with_context(|| format!("binding {}", path.display()))?;
    secure::set_mode(&path, 0o600)?;
    Ok(listener)
}

/// Run the daemon in the foreground until `shutdown`, SIGTERM or SIGINT.
pub fn run(cfg: Config) -> Result<()> {
    cfg.check_socket_path().map_err(anyhow::Error::msg)?;
    secure::ensure_private_dir(&cfg.data_dir)?;
    let _pid_lock = acquire_pidfile(&cfg)?;
    let token: Arc<str> = secure::load_or_create_token(&cfg.token_path())?.into();
    let db = Db::open(&cfg.db_path())?;
    let state = State::new(cfg.clone(), db)?;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("lotsed-rt")
        .build()?;
    let result = rt.block_on(async {
        let listener = bind_socket(&cfg)?;
        tracing::info!(
            "lotsed {} (proto {}) listening on {} (pid {})",
            env!("CARGO_PKG_VERSION"),
            lotse_proto::PROTO_VERSION,
            cfg.socket_path().display(),
            std::process::id()
        );
        serve(&state, listener, token).await
    });
    let _ = std::fs::remove_file(cfg.socket_path());
    let _ = std::fs::remove_file(cfg.pid_path());
    rt.shutdown_timeout(Duration::from_secs(1));
    tracing::info!("lotsed stopped");
    result
}

async fn serve(state: &Arc<State>, listener: UnixListener, token: Arc<str>) -> Result<()> {
    let mut term = signal(SignalKind::terminate())?;
    let mut int = signal(SignalKind::interrupt())?;
    // a controlling terminal going away must not take the daemon down
    let _hup = signal(SignalKind::hangup())?;

    // capture the login environment while the first client connects
    let warm = state.clone();
    tokio::spawn(async move {
        warm.launch_env().await;
    });

    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    tokio::spawn(handle_conn(state.clone(), stream, token.clone()));
                }
                Err(e) => {
                    tracing::warn!("accept: {e}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            },
            () = state.shutdown_requested() => break,
            _ = term.recv() => break,
            _ = int.recv() => break,
        }
    }
    tracing::info!("shutting down");
    drop(listener);
    // let the `ok` of a shutdown request reach its client
    tokio::time::sleep(Duration::from_millis(100)).await;
    let st = state.clone();
    tokio::task::spawn_blocking(move || st.kill_all_sessions()).await?;
    state.hub.kick_all();
    Ok(())
}

/// Start the daemon as a detached background process (own session, log file) and wait until it
/// accepts connections. Returns the pid, `already_running` when one is up.
pub fn detach(cfg: &Config) -> Result<(u32, bool)> {
    cfg.check_socket_path().map_err(anyhow::Error::msg)?;
    secure::ensure_private_dir(&cfg.data_dir)?;
    if let Some(pid) = running_pid(cfg) {
        return Ok((pid, true));
    }
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(cfg.log_path())?;
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.arg("--data-dir").arg(&cfg.data_dir);
    if !cfg.login_env {
        cmd.arg("--no-login-env");
    }
    cmd.stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    new_session(&mut cmd);
    let mut child = cmd.spawn().context("starting lotsed")?;
    let started = Instant::now();
    while started.elapsed() < DETACH_WAIT {
        if let Some(status) = child.try_wait()? {
            bail!(
                "lotsed exited right away ({status}); see {}",
                cfg.log_path().display()
            );
        }
        if StdUnixStream::connect(cfg.socket_path()).is_ok() {
            return Ok((child.id(), false));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    bail!(
        "lotsed did not come up within {DETACH_WAIT:?}; see {}",
        cfg.log_path().display()
    )
}

/// Put the child in its own session so closing the launching terminal cannot reach it.
#[allow(unsafe_code)]
fn new_session(cmd: &mut Command) {
    // SAFETY: the closure runs between fork and exec and only calls `setsid`, which is async-signal-safe.
    unsafe {
        cmd.pre_exec(|| {
            nix::unistd::setsid()
                .map(|_| ())
                .map_err(std::io::Error::from)
        });
    }
}
