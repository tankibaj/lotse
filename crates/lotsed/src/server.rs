//! Unix-socket server: hello/token handshake, request dispatch, PTY frames, one task pair per client.

use crate::config::HELLO_TIMEOUT;
use crate::hub::ClientTx;
use crate::secure::ct_eq;
use crate::state::State;
use lotse_proto::{
    ErrorCode, Frame, FrameError, HelloOk, MAX_FRAME_LEN, MAX_HELLO_LEN, Message, PROTO_VERSION,
    ProtoError, Request, Response, ResponseEnvelope, decode_max,
};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::net::unix::OwnedWriteHalf;

/// Incremental frame reader over any async byte stream.
struct FrameReader<R> {
    r: R,
    buf: Vec<u8>,
    start: usize,
}

impl<R: AsyncRead + Unpin> FrameReader<R> {
    fn new(r: R) -> Self {
        Self {
            r,
            buf: Vec::new(),
            start: 0,
        }
    }

    /// Next frame; `Ok(None)` on a clean close between frames. Cancel safe.
    async fn next(&mut self, limit: usize) -> Result<Option<Frame>, FrameError> {
        loop {
            if let Some((frame, used)) = decode_max(&self.buf[self.start..], limit)? {
                self.start += used;
                if self.start == self.buf.len() {
                    self.buf.clear();
                    self.start = 0;
                } else if self.start > 1 << 20 {
                    self.buf.drain(..self.start);
                    self.start = 0;
                }
                return Ok(Some(frame));
            }
            let mut chunk = [0u8; 32 * 1024];
            let n = self.r.read(&mut chunk).await?;
            if n == 0 {
                return if self.start == self.buf.len() {
                    Ok(None)
                } else {
                    Err(FrameError::Truncated)
                };
            }
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }
}

fn response_frame(req_id: u64, response: Response) -> Frame {
    Frame::Control(Message::Response(ResponseEnvelope { req_id, response }))
}

/// Requests that can take seconds run in their own task so the connection keeps reading.
fn is_slow(req: &Request) -> bool {
    matches!(
        req,
        Request::DetectLaunchCommands
            | Request::CreateSession(_)
            | Request::GetGitInfo { .. }
            | Request::GetDiff { .. }
            | Request::GetTranscript { .. }
            | Request::GetReview { .. }
            | Request::ListFiles { .. }
            | Request::ReadFile { .. }
            | Request::WriteFile { .. }
            | Request::GetUsage { .. }
            | Request::ExportTranscript { .. }
            | Request::GetSessionInfo { .. }
            | Request::StopTask { .. }
            | Request::ListConnectors { .. }
            | Request::CheckConnectors { .. }
    )
}

pub async fn handle_conn(state: Arc<State>, stream: UnixStream, token: Arc<str>) {
    // The socket is 0600 in a 0700 directory; check the peer's uid anyway.
    match stream.peer_cred() {
        Ok(cred) if cred.uid() == nix::unistd::getuid().as_raw() => {}
        _ => return,
    }
    let (rd, mut wr) = stream.into_split();
    let mut reader = FrameReader::new(rd);

    let hello = match tokio::time::timeout(HELLO_TIMEOUT, reader.next(MAX_HELLO_LEN)).await {
        Ok(Ok(Some(Frame::Control(Message::Hello(h))))) => h,
        _ => return,
    };
    let rejection = if !ct_eq(hello.token.as_bytes(), token.as_bytes()) {
        Some((ErrorCode::Unauthorized, "bad token"))
    } else if hello.proto_version != PROTO_VERSION {
        Some((ErrorCode::VersionMismatch, "protocol version mismatch"))
    } else {
        None
    };
    if let Some((code, message)) = rejection {
        let frame = response_frame(
            0,
            Response::Error(ProtoError {
                code,
                message: message.into(),
            }),
        );
        write_frame(&mut wr, &frame).await;
        let _ = wr.shutdown().await;
        return;
    }
    let ok = Frame::Control(Message::HelloOk(HelloOk {
        proto_version: PROTO_VERSION,
        daemon_version: env!("CARGO_PKG_VERSION").into(),
    }));
    if !write_frame(&mut wr, &ok).await {
        return;
    }

    let name: String = hello
        .client
        .chars()
        .filter(|c| c.is_ascii_graphic())
        .take(32)
        .collect();
    let (client, mut rx) = state.hub.register(name);
    tracing::debug!("client {} ({}) connected", client.id, client.name);

    let writer_client = client.clone();
    let writer = tokio::spawn(async move {
        while let Some(bytes) = rx.recv().await {
            let n = bytes.len();
            let ok = wr.write_all(&bytes).await.is_ok();
            writer_client.written(n);
            if !ok {
                break;
            }
        }
        let _ = wr.shutdown().await;
    });

    loop {
        let frame = tokio::select! {
            f = reader.next(MAX_FRAME_LEN) => f,
            () = client.kicked() => {
                tracing::warn!("client {} is too slow, disconnecting", client.id);
                break;
            }
        };
        match frame {
            Ok(Some(Frame::Control(Message::Request(env)))) => {
                if is_slow(&env.request) {
                    let (state, client) = (state.clone(), client.clone());
                    tokio::spawn(async move {
                        dispatch(&state, &client, env.req_id, env.request).await;
                    });
                } else {
                    dispatch(&state, &client, env.req_id, env.request).await;
                }
            }
            Ok(Some(Frame::Control(_))) => {
                client.send_frame(&response_frame(
                    0,
                    Response::Error(ProtoError {
                        code: ErrorCode::InvalidRequest,
                        message: "expected a request".into(),
                    }),
                ));
            }
            Ok(Some(Frame::Pty { session_id, data })) => state.pty_input(&session_id, &data),
            Ok(None) => break,
            Err(e) => {
                tracing::debug!("client {}: {e}", client.id);
                break;
            }
        }
    }
    state.client_gone(client.id);
    tracing::debug!("client {} disconnected", client.id);
    // let what is already queued (the error answer of a bad frame, a last response) reach the socket
    let flush_until = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
    while client.pending() > 0 && tokio::time::Instant::now() < flush_until {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    // the writer holds a sender of its own queue, so it never ends by itself: stop it, closing the socket
    writer.abort();
}

async fn dispatch(state: &Arc<State>, client: &Arc<ClientTx>, req_id: u64, request: Request) {
    if let Some(resp) = state.handle(client, req_id, request).await {
        client.send_frame(&response_frame(req_id, resp));
    }
}

async fn write_frame(wr: &mut OwnedWriteHalf, frame: &Frame) -> bool {
    match lotse_proto::encode(frame) {
        Ok(b) => wr.write_all(&b).await.is_ok(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lotse_proto::{RequestEnvelope, encode};

    fn ping(id: u64) -> Vec<u8> {
        encode(&Frame::Control(Message::Request(RequestEnvelope {
            req_id: id,
            request: Request::Ping,
        })))
        .unwrap()
    }

    #[test]
    fn git_info_runs_off_the_connection_loop() {
        let req = Request::GetGitInfo {
            project_id: lotse_proto::ProjectId::new("p"),
            session_id: None,
        };
        assert!(is_slow(&req));
        assert!(!is_slow(&Request::Ping));
    }

    #[test]
    fn beta_git_file_usage_transcript_and_connector_requests_run_off_the_connection_loop() {
        use lotse_proto::{DiffBase, ExportFormat, SessionId};
        let s = || SessionId::new("a1");
        let slow = [
            Request::GetDiff {
                session_id: s(),
                base: DiffBase::MergeBase,
            },
            Request::GetReview { session_id: s() },
            Request::ListFiles {
                session_id: s(),
                changed_only: false,
            },
            Request::ReadFile {
                session_id: s(),
                path: "a".into(),
                rev: None,
            },
            Request::WriteFile {
                session_id: s(),
                path: "a".into(),
                text: String::new(),
                expected_sha: None,
            },
            Request::GetUsage { session_id: s() },
            Request::ExportTranscript {
                session_id: s(),
                format: ExportFormat::Text,
            },
            Request::GetSessionInfo { session_id: s() },
            Request::ListConnectors { session_id: s() },
            Request::CheckConnectors { session_id: s() },
        ];
        for req in slow {
            assert!(
                is_slow(&req),
                "{} must not block the connection",
                req.method()
            );
        }
        // wp08 may re-run `ps` to check the pid before it signals it
        assert!(is_slow(&Request::StopTask {
            session_id: s(),
            pid: 1
        }));
    }

    #[tokio::test]
    async fn reader_splits_coalesced_and_fragmented_frames() {
        let mut bytes = ping(1);
        bytes.extend(ping(2));
        bytes.extend(ping(3));
        let (mut a, b) = tokio::io::duplex(64);
        let writer = tokio::spawn(async move {
            for chunk in bytes.chunks(7) {
                a.write_all(chunk).await.unwrap();
            }
        });
        let mut r = FrameReader::new(b);
        for want in 1..=3u64 {
            match r.next(MAX_FRAME_LEN).await.unwrap() {
                Some(Frame::Control(Message::Request(e))) => assert_eq!(e.req_id, want),
                other => panic!("unexpected {other:?}"),
            }
        }
        writer.await.unwrap();
        assert!(r.next(MAX_FRAME_LEN).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn reader_rejects_oversized_and_truncated_frames() {
        let (mut a, b) = tokio::io::duplex(64);
        a.write_all(&(10_000u32).to_be_bytes()).await.unwrap();
        let mut r = FrameReader::new(b);
        assert!(matches!(
            r.next(100).await,
            Err(FrameError::BadLength(10_000))
        ));

        let (mut a, b) = tokio::io::duplex(1024);
        let p = ping(1);
        a.write_all(&p[..p.len() - 1]).await.unwrap();
        drop(a);
        let mut r = FrameReader::new(b);
        assert!(matches!(
            r.next(MAX_FRAME_LEN).await,
            Err(FrameError::Truncated)
        ));
    }
}
