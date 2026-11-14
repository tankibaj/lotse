//! Length-prefixed frame codec.
//!
//! ```text
//! u32 BE  len        bytes after this field (type byte + payload), 1..=MAX_FRAME_LEN
//! u8      type       0x01 control (JSON `Message`), 0x02 PTY bytes
//! ...     payload    control: UTF-8 JSON
//!                    PTY:     u16 BE id_len | session id (UTF-8) | raw bytes
//! ```
//!
//! PTY frames flow both ways: daemon → client is terminal output, client → daemon is keyboard input.

use crate::ids::SessionId;
use crate::message::Message;
use std::io::{self, Read, Write};

pub const FRAME_CONTROL: u8 = 0x01;
pub const FRAME_PTY: u8 = 0x02;
/// Largest accepted `len` field (16 MiB).
pub const MAX_FRAME_LEN: usize = 16 * 1024 * 1024;
/// Limit for the first (`hello`) frame of a connection, before the token is verified (64 KiB).
pub const MAX_HELLO_LEN: usize = 64 * 1024;

// Frames are decoded and consumed immediately; boxing `Message` would only make the API clumsy.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    Control(Message),
    Pty {
        session_id: SessionId,
        data: Vec<u8>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("frame length {0} out of range (1..={MAX_FRAME_LEN})")]
    BadLength(usize),
    #[error("unknown frame type {0:#04x}")]
    UnknownType(u8),
    #[error("malformed PTY frame: {0}")]
    BadPty(&'static str),
    #[error("invalid control JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("stream ended inside a frame")]
    Truncated,
    #[error("io: {0}")]
    Io(#[from] io::Error),
}

/// Encode one frame to bytes (length prefix included).
pub fn encode(frame: &Frame) -> Result<Vec<u8>, FrameError> {
    let (kind, payload) = match frame {
        Frame::Control(msg) => (FRAME_CONTROL, serde_json::to_vec(msg)?),
        Frame::Pty { session_id, data } => {
            let id = session_id.as_str().as_bytes();
            let id_len = u16::try_from(id.len()).map_err(|_| FrameError::BadPty("id too long"))?;
            let mut p = Vec::with_capacity(2 + id.len() + data.len());
            p.extend_from_slice(&id_len.to_be_bytes());
            p.extend_from_slice(id);
            p.extend_from_slice(data);
            (FRAME_PTY, p)
        }
    };
    let len = 1 + payload.len();
    if len > MAX_FRAME_LEN {
        return Err(FrameError::BadLength(len));
    }
    let mut out = Vec::with_capacity(4 + len);
    out.extend_from_slice(&(len as u32).to_be_bytes());
    out.push(kind);
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Decode one frame from the front of `buf`.
///
/// `Ok(None)`: not enough bytes yet (read more). `Ok(Some((frame, used)))`: drop `used` bytes.
pub fn decode(buf: &[u8]) -> Result<Option<(Frame, usize)>, FrameError> {
    decode_max(buf, MAX_FRAME_LEN)
}

/// Like [`decode`] with a smaller `limit` on the length field. The daemon uses [`MAX_HELLO_LEN`]
/// until the token is checked, so an unauthenticated peer cannot make it buffer 16 MiB.
pub fn decode_max(buf: &[u8], limit: usize) -> Result<Option<(Frame, usize)>, FrameError> {
    let Some(head) = buf.get(..4) else {
        return Ok(None);
    };
    let len = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
    if len == 0 || len > limit.min(MAX_FRAME_LEN) {
        return Err(FrameError::BadLength(len));
    }
    let Some(body) = buf.get(4..4 + len) else {
        return Ok(None);
    };
    Ok(Some((decode_body(body[0], &body[1..])?, 4 + len)))
}

fn decode_body(kind: u8, payload: &[u8]) -> Result<Frame, FrameError> {
    match kind {
        FRAME_CONTROL => Ok(Frame::Control(serde_json::from_slice(payload)?)),
        FRAME_PTY => {
            let id_len = payload
                .get(..2)
                .map(|b| u16::from_be_bytes([b[0], b[1]]) as usize)
                .ok_or(FrameError::BadPty("missing id length"))?;
            let id = payload
                .get(2..2 + id_len)
                .ok_or(FrameError::BadPty("id past end of frame"))?;
            let id = std::str::from_utf8(id).map_err(|_| FrameError::BadPty("id not UTF-8"))?;
            Ok(Frame::Pty {
                session_id: SessionId::new(id),
                data: payload[2 + id_len..].to_vec(),
            })
        }
        other => Err(FrameError::UnknownType(other)),
    }
}

/// Blocking write of one frame.
pub fn write_frame<W: Write>(w: &mut W, frame: &Frame) -> Result<(), FrameError> {
    w.write_all(&encode(frame)?)?;
    Ok(())
}

/// Blocking read of one frame. `Ok(None)` on clean EOF before the first byte of a frame.
pub fn read_frame<R: Read>(r: &mut R) -> Result<Option<Frame>, FrameError> {
    read_frame_max(r, MAX_FRAME_LEN)
}

/// Like [`read_frame`] with a smaller `limit` on the length field (checked before allocating).
pub fn read_frame_max<R: Read>(r: &mut R, limit: usize) -> Result<Option<Frame>, FrameError> {
    let mut head = [0u8; 4];
    loop {
        match r.read(&mut head[..1]) {
            Ok(0) => return Ok(None),
            Ok(_) => break,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    r.read_exact(&mut head[1..]).map_err(eof_is_truncated)?;
    let len = u32::from_be_bytes(head) as usize;
    if len == 0 || len > limit.min(MAX_FRAME_LEN) {
        return Err(FrameError::BadLength(len));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).map_err(eof_is_truncated)?;
    decode_body(body[0], &body[1..]).map(Some)
}

fn eof_is_truncated(e: io::Error) -> FrameError {
    if e.kind() == io::ErrorKind::UnexpectedEof {
        FrameError::Truncated
    } else {
        FrameError::Io(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{Request, RequestEnvelope};

    fn ping() -> Frame {
        Frame::Control(Message::Request(RequestEnvelope {
            req_id: 7,
            request: Request::Ping,
        }))
    }

    fn pty(data: &[u8]) -> Frame {
        Frame::Pty {
            session_id: SessionId::new("a1"),
            data: data.to_vec(),
        }
    }

    #[test]
    fn golden_pty_bytes() {
        // len = 1 (type) + 2 (id_len) + 2 ("a1") + 3 ("abc") = 8
        let bytes = encode(&pty(b"abc")).unwrap();
        assert_eq!(
            bytes,
            [0, 0, 0, 8, 0x02, 0, 2, b'a', b'1', b'a', b'b', b'c']
        );
    }

    #[test]
    fn golden_control_bytes() {
        let bytes = encode(&ping()).unwrap();
        let json = br#"{"type":"request","body":{"req_id":7,"request":{"method":"ping"}}}"#;
        assert_eq!(&bytes[..4], &((json.len() + 1) as u32).to_be_bytes());
        assert_eq!(bytes[4], 0x01);
        assert_eq!(&bytes[5..], json);
    }

    #[test]
    fn roundtrip_control_and_pty_including_binary() {
        for f in [ping(), pty(b""), pty(&[0, 255, 0x1b, b'[', b'A'])] {
            let bytes = encode(&f).unwrap();
            let (back, used) = decode(&bytes).unwrap().unwrap();
            assert_eq!(back, f);
            assert_eq!(used, bytes.len());
        }
    }

    #[test]
    fn partial_input_asks_for_more_and_back_to_back_frames_split() {
        let mut bytes = encode(&ping()).unwrap();
        let second = encode(&pty(b"x")).unwrap();
        bytes.extend_from_slice(&second);
        for cut in 0..bytes.len() - second.len() {
            assert!(
                decode(&bytes[..cut]).unwrap().is_none(),
                "cut {cut} should be incomplete"
            );
        }
        let (f1, used) = decode(&bytes).unwrap().unwrap();
        assert_eq!(f1, ping());
        let (f2, used2) = decode(&bytes[used..]).unwrap().unwrap();
        assert_eq!(f2, pty(b"x"));
        assert_eq!(used + used2, bytes.len());
    }

    #[test]
    fn rejects_bad_length_type_and_pty() {
        assert!(matches!(
            decode(&[0, 0, 0, 0]),
            Err(FrameError::BadLength(0))
        ));
        let huge = (MAX_FRAME_LEN as u32 + 1).to_be_bytes();
        assert!(matches!(decode(&huge), Err(FrameError::BadLength(_))));
        assert!(matches!(
            decode(&[0, 0, 0, 1, 0x7f]),
            Err(FrameError::UnknownType(0x7f))
        ));
        // PTY frame whose id_len points past the end.
        assert!(matches!(
            decode(&[0, 0, 0, 4, 0x02, 0, 9, b'a']),
            Err(FrameError::BadPty(_))
        ));
        // Control frame with invalid JSON.
        assert!(matches!(
            decode(&[0, 0, 0, 3, 0x01, b'{', b'x']),
            Err(FrameError::Json(_))
        ));
    }

    #[test]
    fn oversized_pty_payload_is_refused_on_encode() {
        let f = pty(&vec![0u8; MAX_FRAME_LEN]);
        assert!(matches!(encode(&f), Err(FrameError::BadLength(_))));
    }

    #[test]
    fn a_frame_of_exactly_the_maximum_is_accepted_everywhere() {
        // len = 1 (type) + 2 (id_len) + 2 ("a1") + data
        let f = pty(&vec![7u8; MAX_FRAME_LEN - 5]);
        let bytes = encode(&f).unwrap();
        assert_eq!(&bytes[..4], &(MAX_FRAME_LEN as u32).to_be_bytes());
        assert_eq!(decode(&bytes).unwrap().unwrap().0, f);
        assert_eq!(read_frame(&mut io::Cursor::new(bytes)).unwrap(), Some(f));
    }

    #[test]
    fn blocking_read_refuses_huge_length_before_allocating() {
        for len in [u32::MAX, MAX_FRAME_LEN as u32 + 1, 0] {
            let mut r = io::Cursor::new(len.to_be_bytes().to_vec());
            assert!(matches!(
                read_frame(&mut r),
                Err(FrameError::BadLength(n)) if n == len as usize
            ));
        }
    }

    #[test]
    fn blocking_read_reports_truncation_inside_header_and_body() {
        let mut r = io::Cursor::new(vec![0, 0, 0]);
        assert!(matches!(read_frame(&mut r), Err(FrameError::Truncated)));
        let mut r = io::Cursor::new(vec![0, 0, 0, 5, 0x01, b'{']);
        assert!(matches!(read_frame(&mut r), Err(FrameError::Truncated)));
    }

    #[test]
    fn smaller_limit_applies_to_both_decoders() {
        let bytes = encode(&pty(&[0u8; 100])).unwrap();
        assert!(matches!(
            decode_max(&bytes, 50),
            Err(FrameError::BadLength(_))
        ));
        assert!(decode_max(&bytes, 200).unwrap().is_some());
        assert!(matches!(
            read_frame_max(&mut io::Cursor::new(bytes.clone()), 50),
            Err(FrameError::BadLength(_))
        ));
        // The limit can never exceed the protocol maximum.
        let huge = (MAX_FRAME_LEN as u32 + 1).to_be_bytes();
        assert!(decode_max(&huge, usize::MAX).is_err());
        let hello = encode(&ping()).unwrap();
        assert!(hello.len() < MAX_HELLO_LEN);
        assert!(decode_max(&hello, MAX_HELLO_LEN).unwrap().is_some());
    }

    #[test]
    fn blocking_read_retries_interrupted_reads() {
        struct Flaky(Option<io::Cursor<Vec<u8>>>, bool);
        impl Read for Flaky {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                if !self.1 {
                    self.1 = true;
                    return Err(io::ErrorKind::Interrupted.into());
                }
                self.0.as_mut().unwrap().read(buf)
            }
        }
        let wire = encode(&ping()).unwrap();
        let mut r = Flaky(Some(io::Cursor::new(wire)), false);
        assert_eq!(read_frame(&mut r).unwrap(), Some(ping()));
    }

    #[test]
    fn blocking_read_write() {
        let mut wire = Vec::new();
        write_frame(&mut wire, &ping()).unwrap();
        write_frame(&mut wire, &pty(b"hi")).unwrap();
        let mut r = io::Cursor::new(wire.clone());
        assert_eq!(read_frame(&mut r).unwrap(), Some(ping()));
        assert_eq!(read_frame(&mut r).unwrap(), Some(pty(b"hi")));
        assert_eq!(read_frame(&mut r).unwrap(), None);
        let mut cut = io::Cursor::new(wire[..wire.len() - 1].to_vec());
        read_frame(&mut cut).unwrap();
        assert!(matches!(read_frame(&mut cut), Err(FrameError::Truncated)));
    }
}
