//! Beta (wp04): branch review: `get_review`, and `get_diff` against the merge-base.
//! Stub: answers `unavailable` until the work package lands. The dispatcher in `state` already
//! routes here, so the work package fills this file only.

use crate::state::{State, unavailable};
use lotse_proto::{Request, Response};
use std::sync::Arc;

pub async fn handle(_state: &Arc<State>, req: Request) -> Response {
    unavailable(req.method())
}
