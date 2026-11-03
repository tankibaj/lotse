//! Beta (wp05): files of a session folder, confined to it: `list_files`, `read_file`, `write_file`.
//! Stub: answers `unavailable` until the work package lands. The dispatcher in `state` already
//! routes here, so the work package fills this file only.

use crate::state::{State, unavailable};
use lotse_proto::{Request, Response};
use std::sync::Arc;

pub async fn handle(_state: &Arc<State>, req: Request) -> Response {
    unavailable(req.method())
}
