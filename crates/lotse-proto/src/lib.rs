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
/// 4: Beta. `GitInfo` behind / +− / `pr` (`PrInfo`); `DiffFile` +−, `get_diff` `base`; requests
///    `get_review`, `list_files`, `read_file`, `write_file`, `get_usage`, `export_transcript`,
///    `get_session_info`, `stop_task`, `list_connectors`, `check_connectors` and their responses;
///    transcript `Approval` role, `approval`, `sub_agent`, tool `status` / `result`,
///    `Transcript::source`; `PermRequest::id`, `resolve_permission` `via` / `prompt_id`;
///    `Event::Notification` `banner` / `sound_name` / `prompt_id`; `EventPrefs::sound_name`;
///    `Settings::external`.
/// 5: Mod and child sessions. `SubAgent` parent / activity / tokens / cost_usd / model / background;
///    `Session` context_pct / source_layer / parent_session / todo; `UsageToday::rate_limits`;
///    `NewSession` parent_session / requested_by_agent; `DetectSource::Mod`; requests `mod_attach`,
///    `resolve_session_start`; events `mod_command`, `session_start_requested`, `session_start_withdrawn`;
///    `Settings::agents_start_sessions_without_asking`. All additive.
pub const PROTO_VERSION: u32 = 5;

#[cfg(test)]
mod fixture_tests;
