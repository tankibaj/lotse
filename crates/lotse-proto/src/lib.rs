//! Lotse wire protocol. Frozen after WP0; changes need an orchestrator-approved `PROTO_VERSION` bump.
//!
//! The TypeScript mirror lives in `app/src/lib/client/types.ts` and `DaemonClient`.
//! JSON fixtures in `fixtures/*.json` are the shared contract: both sides must round-trip them.

pub mod frame;
pub mod ids;
pub mod message;
pub mod status;
pub mod types;

pub use frame::{
    Frame, FrameError, MAX_FRAME_LEN, MAX_HELLO_LEN, decode, decode_max, encode, read_frame,
    read_frame_max, write_frame,
};
pub use ids::{ProjectId, SessionId, SubAgentId, WorkspaceId};
pub use message::{
    Event, Hello, HelloOk, Message, Request, RequestEnvelope, Response, ResponseEnvelope,
};
pub use status::{Status, rollup};
pub use types::*;

/// Wire version. Sent in `hello` and `hello_ok`; a mismatch is `ErrorCode::VersionMismatch`.
/// 2: `PermRequest::always`.
/// 3: workspaces (`Workspace`, workspace requests and events, `Snapshot::workspaces`), `get_diff`,
///    `get_transcript`, today's usage (`Snapshot::usage`, `usage_changed`).
pub const PROTO_VERSION: u32 = 3;

#[cfg(test)]
mod fixture_tests;
