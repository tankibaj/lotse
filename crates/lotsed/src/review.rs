//! Beta (wp04): branch review: `get_review`, and `get_diff` against the merge-base.
//! `get_diff` against `HEAD` is Show Changes and stays in `state`.

use crate::state::{State, unavailable};
use lotse_proto::{DiffBase, ErrorCode, ProtoError, Request, Response, SessionId};
use std::path::PathBuf;
use std::sync::Arc;

pub async fn handle(state: &Arc<State>, req: Request) -> Response {
    match req {
        Request::GetReview { session_id } => {
            on_git(state, &session_id, |dir| {
                lotse_git::review::review(&dir).map(Response::Review)
            })
            .await
        }
        Request::GetDiff {
            session_id,
            base: DiffBase::MergeBase,
        } => {
            on_git(state, &session_id, |dir| {
                lotse_git::review::diff_from_merge_base(&dir).map(Response::Diff)
            })
            .await
        }
        other => unavailable(other.method()),
    }
}

/// Runs `work` (it runs `git`) off the async runtime in the session's folder.
async fn on_git(
    state: &Arc<State>,
    id: &SessionId,
    work: impl FnOnce(PathBuf) -> Result<Response, lotse_git::GitError> + Send + 'static,
) -> Response {
    let Some(session) = state.session(id) else {
        return error(ErrorCode::NotFound, "session not found");
    };
    let dir = session.cwd();
    match tokio::task::spawn_blocking(move || work(dir)).await {
        Ok(Ok(response)) => response,
        Ok(Err(e)) => error(ErrorCode::Unavailable, &e.to_string()),
        Err(e) => {
            tracing::error!("{e}");
            error(ErrorCode::Internal, "internal error")
        }
    }
}

fn error(code: ErrorCode, message: &str) -> Response {
    Response::Error(ProtoError {
        code,
        message: message.into(),
    })
}
