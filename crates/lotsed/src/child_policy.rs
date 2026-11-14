//! What a `lotse-hook mcp` connection (an agent's own tool process, `Hello.client` = `mcp` or
//! `mcp:<session id>`) may ask of the daemon. Pure functions over a snapshot of the sessions, so the
//! allowlist and the limits are table-tested without a daemon.
//!
//! * Allowlist: `ping`, `list_sessions`, `create_session`, `get_transcript`, `send_input`,
//!   `close_session`. Every other request (including any method a later proto adds) is refused.
//! * `list_sessions` shows the caller's project plus its children and parent.
//! * `get_transcript`, `send_input` and `close_session` reach the caller's own children and parent
//!   only (`close_session`: children only); `send_input` also needs the target idle or done, because
//!   typing at a permission prompt could answer it.
//! * `create_session` never takes a command, never starts a plain terminal (its first prompt would be
//!   typed into a shell), makes the caller the parent, and respects depth and fan-out limits.
//!
//! The same uid can still spoof the client name: this is defence in depth plus an audit trail
//! (every decision is logged by the caller), a per-session key is planned for v2.

use lotse_proto::{
    AgentKind, ErrorCode, NewSession, ProjectId, ProtoError, Request, SessionId, Status,
};

/// A session started by a session started by a session: depth 2 is the deepest.
pub const MAX_DEPTH: usize = 2;
/// Live children per session, pending approvals included.
pub const MAX_LIVE_CHILDREN: usize = 5;
/// Open approval cards per requesting session.
pub const MAX_PENDING_PER_SESSION: usize = 3;
pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_PROMPT_CHARS: usize = 50_000;
pub const MAX_MESSAGE_CHARS: usize = 8_000;
/// Turns `get_transcript` returns to an agent at most.
pub const MAX_TRANSCRIPT_TURNS: u32 = 20;

/// The methods an mcp client may call.
pub const ALLOWED_METHODS: &[&str] = &[
    "ping",
    "list_sessions",
    "create_session",
    "get_transcript",
    "send_input",
    "close_session",
];

pub fn method_allowed(method: &str) -> bool {
    ALLOWED_METHODS.contains(&method)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientKind {
    /// `mcp:<session>`; bare `mcp` has no session and may only ping.
    Mcp(Option<SessionId>),
    Other,
}

pub fn classify(client_name: &str) -> ClientKind {
    match client_name.strip_prefix("mcp") {
        Some("") => ClientKind::Mcp(None),
        Some(rest) => match rest.strip_prefix(':') {
            Some("") => ClientKind::Mcp(None),
            Some(id) => ClientKind::Mcp(Some(SessionId::new(id))),
            None => ClientKind::Other,
        },
        None => ClientKind::Other,
    }
}

/// What the policy needs to know of one session.
#[derive(Debug, Clone)]
pub struct Fact {
    pub id: SessionId,
    pub project_id: ProjectId,
    pub parent: Option<SessionId>,
    pub status: Status,
    pub alive: bool,
}

/// Where `target` stands relative to `caller`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
    Child,
    Parent,
    Other,
}

pub fn relation(caller: &SessionId, target: &SessionId, facts: &[Fact]) -> Relation {
    let parent_of = |id: &SessionId| {
        facts
            .iter()
            .find(|f| &f.id == id)
            .map(|f| f.parent.as_ref())
    };
    if parent_of(target) == Some(Some(caller)) {
        Relation::Child
    } else if parent_of(caller) == Some(Some(target)) && caller != target {
        Relation::Parent
    } else {
        Relation::Other
    }
}

/// Ancestors of `id` that are still open (a root session has depth 0). Bounded, so a cycle ends.
pub fn depth(id: &SessionId, facts: &[Fact]) -> usize {
    let mut depth = 0;
    let mut at = id;
    while depth < 2 * MAX_DEPTH + 4
        && let Some(parent) = facts
            .iter()
            .find(|f| &f.id == at)
            .and_then(|f| f.parent.as_ref())
            .filter(|p| facts.iter().any(|f| &f.id == *p))
    {
        depth += 1;
        at = parent;
    }
    depth
}

/// Sessions `list_sessions` shows the caller: its project, its children and its parent.
pub fn visible(caller: &SessionId, facts: &[Fact]) -> Vec<SessionId> {
    let project = facts
        .iter()
        .find(|f| &f.id == caller)
        .map(|f| &f.project_id);
    facts
        .iter()
        .filter(|f| {
            &f.id == caller
                || project == Some(&f.project_id)
                || relation(caller, &f.id, facts) != Relation::Other
        })
        .map(|f| f.id.clone())
        .collect()
}

/// `Ok` when the daemon may go on with `req` for `caller`; `pending` is the caller's open approval cards.
pub fn check(
    req: &Request,
    caller: &SessionId,
    facts: &[Fact],
    pending: usize,
) -> Result<(), ProtoError> {
    if !method_allowed(req.method()) {
        return Err(deny(
            ErrorCode::Unauthorized,
            format!("{} is not available to agents", req.method()),
        ));
    }
    let fact_of = |id: &SessionId| facts.iter().find(|f| &f.id == id);
    match req {
        Request::CreateSession(ns) => check_start(ns, caller, facts, pending),
        Request::GetTranscript { session_id, .. } => match relation(caller, session_id, facts) {
            Relation::Other => Err(not_relative()),
            _ => Ok(()),
        },
        Request::CloseSession { session_id } => match relation(caller, session_id, facts) {
            Relation::Child => Ok(()),
            _ => Err(deny(
                ErrorCode::Unauthorized,
                "an agent may close its own child sessions only",
            )),
        },
        Request::SendInput { session_id, .. } => {
            if relation(caller, session_id, facts) == Relation::Other {
                return Err(not_relative());
            }
            match fact_of(session_id).map(|f| f.status) {
                Some(Status::Idle | Status::Done) => Ok(()),
                _ => Err(deny(
                    ErrorCode::Conflict,
                    "the session is not idle: wait until it is done (typing at a prompt could answer it)",
                )),
            }
        }
        // ping, list_sessions
        _ => Ok(()),
    }
}

fn check_start(
    ns: &NewSession,
    caller: &SessionId,
    facts: &[Fact],
    pending: usize,
) -> Result<(), ProtoError> {
    let invalid = |m: &str| Err(deny(ErrorCode::InvalidRequest, m));
    if ns.command.is_some() {
        return invalid("agents cannot choose the command of a session");
    }
    if ns.agent == AgentKind::Terminal {
        return invalid("agents start coding agents only (claude, codex, opencode)");
    }
    if ns.parent_session.as_ref().is_some_and(|p| p != caller) {
        return Err(deny(
            ErrorCode::Unauthorized,
            "a session can only be started as the caller's own child",
        ));
    }
    match ns.first_prompt.as_deref().map(sanitize_prompt) {
        Some(Ok(_)) => {}
        Some(Err(e)) => return Err(e),
        None => return invalid("a prompt is required"),
    }
    if ns
        .title
        .as_deref()
        .is_some_and(|t| t.chars().count() > MAX_TITLE_CHARS)
    {
        return invalid("the title must be at most 200 characters");
    }
    if !facts.iter().any(|f| &f.id == caller) {
        return Err(deny(ErrorCode::NotFound, "the requesting session is gone"));
    }
    if depth(caller, facts) >= MAX_DEPTH {
        return Err(deny(
            ErrorCode::Conflict,
            "child sessions can go two levels deep at most",
        ));
    }
    let live = facts
        .iter()
        .filter(|f| f.alive && f.parent.as_ref() == Some(caller))
        .count();
    if live + pending >= MAX_LIVE_CHILDREN || pending >= MAX_PENDING_PER_SESSION {
        return Err(deny(
            ErrorCode::Conflict,
            "too many child sessions (5 at most) or open start requests (3 at most)",
        ));
    }
    Ok(())
}

fn not_relative() -> ProtoError {
    deny(
        ErrorCode::Unauthorized,
        "only the session's own child or parent",
    )
}

/// Whether a start needs the approval card: always, unless the user turned the toggle on and the
/// child stays in the requesting session's project.
pub fn needs_card(ns: &NewSession, caller_project: &ProjectId, auto_approve: bool) -> bool {
    !(auto_approve && &ns.project_id == caller_project)
}

/// `text` as typed into an agent's input: control characters other than newline and tab are dropped
/// (an ESC would end the bracketed paste and let the rest run as keystrokes, or press Shift+Tab).
pub fn sanitize_message(text: &str) -> Result<String, ProtoError> {
    clean(text, MAX_MESSAGE_CHARS, "the message")
}

/// The first prompt of a child session, the same way: what the approval card shows is what is typed.
pub fn sanitize_prompt(text: &str) -> Result<String, ProtoError> {
    clean(text, MAX_PROMPT_CHARS, "the prompt")
}

fn clean(text: &str, max: usize, what: &str) -> Result<String, ProtoError> {
    let clean: String = text
        .replace("\r\n", "\n")
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect();
    if clean.trim().is_empty() || clean.chars().count() > max {
        return Err(deny(
            ErrorCode::InvalidRequest,
            format!("{what} must be 1-{max} characters of text"),
        ));
    }
    Ok(clean)
}

fn deny(code: ErrorCode, message: impl Into<String>) -> ProtoError {
    ProtoError {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lotse_proto::WorktreeSpec;

    fn sid(s: &str) -> SessionId {
        SessionId::new(s)
    }

    fn fact(id: &str, project: &str, parent: Option<&str>, status: Status) -> Fact {
        Fact {
            id: sid(id),
            project_id: ProjectId::new(project),
            parent: parent.map(sid),
            status,
            alive: true,
        }
    }

    fn ns(project: &str) -> NewSession {
        NewSession {
            project_id: ProjectId::new(project),
            agent: AgentKind::ClaudeCode,
            command: None,
            title: Some("child".into()),
            worktree: None,
            first_prompt: Some("do the thing".into()),
            parent_session: None,
            requested_by_agent: false,
        }
    }

    fn start(ns: NewSession) -> Request {
        Request::CreateSession(ns)
    }

    fn code(r: Result<(), ProtoError>) -> Option<ErrorCode> {
        r.err().map(|e| e.code)
    }

    #[test]
    fn the_allowlist_is_exactly_these_methods_and_a_new_proto_method_is_denied_until_listed() {
        let allowed: Vec<&str> = Request::METHODS
            .iter()
            .copied()
            .filter(|m| method_allowed(m))
            .collect();
        assert_eq!(
            allowed,
            [
                "ping",
                "list_sessions",
                "create_session",
                "close_session",
                "send_input",
                "get_transcript",
            ]
        );
        assert_eq!(
            ALLOWED_METHODS.len(),
            allowed.len(),
            "no unknown method listed"
        );
        assert!(!method_allowed("some_future_method"));
        assert!(!method_allowed(""));
    }

    #[test]
    fn every_method_outside_the_allowlist_is_unauthorized() {
        let s = sid("a");
        let facts = [fact("a", "p", None, Status::Run)];
        let pid = ProjectId::new("p");
        let denied = [
            Request::GetSnapshot,
            Request::ListProjects,
            Request::RemoveProject {
                project_id: pid.clone(),
            },
            Request::RenameSession {
                session_id: s.clone(),
                title: "x".into(),
            },
            Request::RetrySession {
                session_id: s.clone(),
            },
            Request::MarkRead {
                session_id: s.clone(),
            },
            Request::ResolvePermission {
                session_id: s.clone(),
                decision: lotse_proto::PermDecision::Allow,
                via: None,
                prompt_id: None,
            },
            Request::Attach {
                session_id: s.clone(),
                cols: 80,
                rows: 24,
            },
            Request::Resize {
                session_id: s.clone(),
                cols: 80,
                rows: 24,
            },
            Request::GetSettings,
            Request::Shutdown,
            Request::ReadFile {
                session_id: s.clone(),
                path: "a".into(),
                rev: None,
            },
            Request::WriteFile {
                session_id: s.clone(),
                path: "a".into(),
                text: String::new(),
                expected_sha: None,
            },
            Request::GetSessionInfo {
                session_id: s.clone(),
            },
            Request::StopTask {
                session_id: s.clone(),
                pid: 1,
            },
            Request::ModAttach {
                session_id: s.clone(),
            },
            // an agent must never approve its own start
            Request::ResolveSessionStart {
                id: "r-1".into(),
                approve: true,
            },
            Request::HookEvent {
                agent: AgentKind::ClaudeCode,
                session_id: Some(s.clone()),
                payload: serde_json::json!({}),
            },
        ];
        for req in denied {
            let got = check(&req, &s, &facts, 0).unwrap_err();
            assert_eq!(got.code, ErrorCode::Unauthorized, "{}", req.method());
        }
    }

    #[test]
    fn start_rules() {
        let facts = [
            fact("root", "p", None, Status::Run),
            fact("kid", "p", Some("root"), Status::Run),
            fact("grandkid", "p", Some("kid"), Status::Run),
        ];
        let root = sid("root");
        let mut cases: Vec<(&str, NewSession, SessionId, Option<ErrorCode>)> = Vec::new();
        cases.push(("plain start", ns("p"), root.clone(), None));
        cases.push((
            "worktree is fine",
            NewSession {
                worktree: Some(WorktreeSpec {
                    branch: "lotse/x".into(),
                }),
                ..ns("p")
            },
            root.clone(),
            None,
        ));
        cases.push((
            "a command is arbitrary code",
            NewSession {
                command: Some("rm -rf ~".into()),
                ..ns("p")
            },
            root.clone(),
            Some(ErrorCode::InvalidRequest),
        ));
        cases.push((
            "an empty command is still a command field",
            NewSession {
                command: Some(String::new()),
                ..ns("p")
            },
            root.clone(),
            Some(ErrorCode::InvalidRequest),
        ));
        cases.push((
            "a terminal would run the prompt in a shell",
            NewSession {
                agent: AgentKind::Terminal,
                ..ns("p")
            },
            root.clone(),
            Some(ErrorCode::InvalidRequest),
        ));
        cases.push((
            "someone else's child",
            NewSession {
                parent_session: Some(sid("kid")),
                ..ns("p")
            },
            root.clone(),
            Some(ErrorCode::Unauthorized),
        ));
        cases.push((
            "naming itself as parent is fine",
            NewSession {
                parent_session: Some(root.clone()),
                ..ns("p")
            },
            root.clone(),
            None,
        ));
        cases.push((
            "no prompt",
            NewSession {
                first_prompt: None,
                ..ns("p")
            },
            root.clone(),
            Some(ErrorCode::InvalidRequest),
        ));
        cases.push((
            "blank prompt",
            NewSession {
                first_prompt: Some("  \n".into()),
                ..ns("p")
            },
            root.clone(),
            Some(ErrorCode::InvalidRequest),
        ));
        cases.push((
            "prompt too long",
            NewSession {
                first_prompt: Some("x".repeat(MAX_PROMPT_CHARS + 1)),
                ..ns("p")
            },
            root.clone(),
            Some(ErrorCode::InvalidRequest),
        ));
        cases.push((
            "title too long",
            NewSession {
                title: Some("t".repeat(MAX_TITLE_CHARS + 1)),
                ..ns("p")
            },
            root.clone(),
            Some(ErrorCode::InvalidRequest),
        ));
        cases.push(("depth 2 may exist", ns("p"), sid("kid"), None));
        cases.push((
            "depth 3 may not",
            ns("p"),
            sid("grandkid"),
            Some(ErrorCode::Conflict),
        ));
        cases.push((
            "the caller is gone",
            ns("p"),
            sid("nobody"),
            Some(ErrorCode::NotFound),
        ));
        for (name, ns, caller, want) in cases {
            assert_eq!(code(check(&start(ns), &caller, &facts, 0)), want, "{name}");
        }
    }

    #[test]
    fn at_most_five_live_children_and_three_open_cards_per_session() {
        let mut facts = vec![fact("root", "p", None, Status::Run)];
        let root = sid("root");
        for i in 0..4 {
            facts.push(fact(&format!("k{i}"), "p", Some("root"), Status::Run));
        }
        assert_eq!(code(check(&start(ns("p")), &root, &facts, 0)), None);
        // the fifth is the last one
        facts.push(fact("k4", "p", Some("root"), Status::Run));
        assert_eq!(
            code(check(&start(ns("p")), &root, &facts, 0)),
            Some(ErrorCode::Conflict)
        );
        // a dead child does not count, a pending card does
        facts[1].alive = false;
        assert_eq!(code(check(&start(ns("p")), &root, &facts, 0)), None);
        assert_eq!(
            code(check(&start(ns("p")), &root, &facts, 1)),
            Some(ErrorCode::Conflict)
        );
        // other sessions' children are not this one's
        let mut other = vec![fact("root", "p", None, Status::Run)];
        other.push(fact("x", "p", None, Status::Run));
        for i in 0..5 {
            other.push(fact(&format!("y{i}"), "p", Some("x"), Status::Run));
        }
        assert_eq!(code(check(&start(ns("p")), &root, &other, 0)), None);
        // open cards alone stop at three
        assert_eq!(
            code(check(
                &start(ns("p")),
                &root,
                &[fact("root", "p", None, Status::Run)],
                3
            )),
            Some(ErrorCode::Conflict)
        );
    }

    #[test]
    fn depth_counts_ancestors_and_survives_a_cycle_and_a_missing_parent() {
        let facts = [
            fact("a", "p", None, Status::Run),
            fact("b", "p", Some("a"), Status::Run),
            fact("c", "p", Some("b"), Status::Run),
            fact("orphan", "p", Some("closed"), Status::Run),
            fact("x", "p", Some("y"), Status::Run),
            fact("y", "p", Some("x"), Status::Run),
        ];
        assert_eq!(depth(&sid("a"), &facts), 0);
        assert_eq!(depth(&sid("b"), &facts), 1);
        assert_eq!(depth(&sid("c"), &facts), 2);
        // its parent was closed: it counts as a top-level session
        assert_eq!(depth(&sid("orphan"), &facts), 0);
        assert!(depth(&sid("x"), &facts) <= 8, "a cycle must terminate");
    }

    #[test]
    fn relations() {
        let facts = [
            fact("a", "p", None, Status::Run),
            fact("b", "p", Some("a"), Status::Run),
            fact("c", "p", Some("b"), Status::Run),
            fact("z", "p", None, Status::Run),
        ];
        let rel = |x: &str, y: &str| relation(&sid(x), &sid(y), &facts);
        assert_eq!(rel("a", "b"), Relation::Child);
        assert_eq!(rel("b", "a"), Relation::Parent);
        assert_eq!(
            rel("a", "c"),
            Relation::Other,
            "a grandchild is not a child"
        );
        assert_eq!(rel("c", "a"), Relation::Other);
        assert_eq!(rel("a", "z"), Relation::Other);
        assert_eq!(rel("a", "a"), Relation::Other);
        assert_eq!(rel("a", "missing"), Relation::Other);
    }

    #[test]
    fn list_shows_the_project_the_children_and_the_parent_only() {
        let facts = [
            fact("me", "p1", Some("boss"), Status::Run),
            fact("boss", "p2", None, Status::Run),
            fact("kid", "p3", Some("me"), Status::Run),
            fact("mate", "p1", None, Status::Run),
            fact("stranger", "p9", None, Status::Run),
            fact("strangers-kid", "p9", Some("stranger"), Status::Run),
        ];
        let mut seen: Vec<String> = visible(&sid("me"), &facts)
            .iter()
            .map(ToString::to_string)
            .collect();
        seen.sort();
        assert_eq!(seen, ["boss", "kid", "mate", "me"]);
    }

    #[test]
    fn send_input_needs_a_relative_that_is_idle_or_done() {
        let mk = |status| {
            [
                fact("a", "p", None, Status::Run),
                fact("kid", "p", Some("a"), status),
                fact("stranger", "p", None, Status::Idle),
            ]
        };
        let send = |to: &str| Request::SendInput {
            session_id: sid(to),
            text: "hi".into(),
        };
        for (status, want) in [
            (Status::Idle, None),
            (Status::Done, None),
            (Status::Run, Some(ErrorCode::Conflict)),
            (Status::Perm, Some(ErrorCode::Conflict)),
            (Status::Fail, Some(ErrorCode::Conflict)),
            (Status::Unknown, Some(ErrorCode::Conflict)),
        ] {
            assert_eq!(
                code(check(&send("kid"), &sid("a"), &mk(status), 0)),
                want,
                "{status:?}"
            );
        }
        // the parent, while it is idle
        let facts = [
            fact("a", "p", None, Status::Idle),
            fact("kid", "p", Some("a"), Status::Run),
        ];
        assert_eq!(code(check(&send("a"), &sid("kid"), &facts, 0)), None);
        // same project, no relation: no
        assert_eq!(
            code(check(&send("stranger"), &sid("a"), &mk(Status::Idle), 0)),
            Some(ErrorCode::Unauthorized)
        );
        assert_eq!(
            code(check(&send("ghost"), &sid("a"), &mk(Status::Idle), 0)),
            Some(ErrorCode::Unauthorized)
        );
        // no typing a session's own terminal
        assert_eq!(
            code(check(&send("a"), &sid("a"), &mk(Status::Idle), 0)),
            Some(ErrorCode::Unauthorized)
        );
    }

    #[test]
    fn transcript_reaches_relatives_only_and_close_only_children() {
        let facts = [
            fact("a", "p", None, Status::Run),
            fact("kid", "p", Some("a"), Status::Run),
            fact("stranger", "p", None, Status::Run),
        ];
        let tr = |to: &str| Request::GetTranscript {
            session_id: sid(to),
            limit: Some(5),
        };
        let close = |to: &str| Request::CloseSession {
            session_id: sid(to),
        };
        assert_eq!(code(check(&tr("kid"), &sid("a"), &facts, 0)), None);
        assert_eq!(code(check(&tr("a"), &sid("kid"), &facts, 0)), None);
        assert_eq!(
            code(check(&tr("stranger"), &sid("a"), &facts, 0)),
            Some(ErrorCode::Unauthorized)
        );
        assert_eq!(code(check(&close("kid"), &sid("a"), &facts, 0)), None);
        // not the parent, not a stranger, not itself
        for target in ["a", "stranger"] {
            assert_eq!(
                code(check(&close(target), &sid("kid"), &facts, 0)),
                Some(ErrorCode::Unauthorized),
                "{target}"
            );
        }
        assert_eq!(
            code(check(&close("a"), &sid("a"), &facts, 0)),
            Some(ErrorCode::Unauthorized)
        );
    }

    #[test]
    fn list_and_ping_are_always_fine() {
        let facts = [fact("a", "p", None, Status::Run)];
        for req in [
            Request::Ping,
            Request::ListSessions { project_id: None },
            Request::ListSessions {
                project_id: Some(ProjectId::new("elsewhere")),
            },
        ] {
            assert_eq!(code(check(&req, &sid("a"), &facts, 0)), None);
        }
    }

    #[test]
    fn the_card_is_skipped_only_with_the_toggle_and_inside_the_parents_project() {
        let own = ProjectId::new("p");
        assert!(needs_card(&ns("p"), &own, false));
        assert!(!needs_card(&ns("p"), &own, true));
        assert!(needs_card(&ns("other"), &own, true));
        assert!(needs_card(&ns("other"), &own, false));
    }

    #[test]
    fn client_names() {
        assert_eq!(classify("mcp:s-1"), ClientKind::Mcp(Some(sid("s-1"))));
        assert_eq!(classify("mcp"), ClientKind::Mcp(None));
        assert_eq!(classify("mcp:"), ClientKind::Mcp(None));
        for other in ["app", "hook", "cli", "mcpx", "MCP", "xmcp:s-1", ""] {
            assert_eq!(classify(other), ClientKind::Other, "{other}");
        }
    }

    #[test]
    fn prompts_keep_their_lines_and_lose_control_characters_and_a_blank_one_is_refused() {
        assert_eq!(
            sanitize_prompt("fix it\r\nthen\ttest \u{1b}[Z\u{7}").unwrap(),
            "fix it\nthen\ttest [Z"
        );
        for blank in ["", "  \n ", "\u{1b}\u{0}"] {
            assert_eq!(
                sanitize_prompt(blank).unwrap_err().code,
                ErrorCode::InvalidRequest,
                "{blank:?}"
            );
        }
        assert!(sanitize_prompt(&"x".repeat(MAX_PROMPT_CHARS)).is_ok());
        assert!(sanitize_prompt(&"x".repeat(MAX_PROMPT_CHARS + 1)).is_err());
        // a start whose prompt is nothing but control characters is refused like a blank one
        let facts = [fact("root", "p", None, Status::Run)];
        let nothing = NewSession {
            first_prompt: Some("\u{1b}\u{7}".into()),
            ..ns("p")
        };
        assert_eq!(
            code(check(&start(nothing), &sid("root"), &facts, 0)),
            Some(ErrorCode::InvalidRequest)
        );
    }

    #[test]
    fn messages_lose_control_characters_and_keep_text() {
        assert_eq!(
            sanitize_message("a\nb\tc \u{1b}[201~rm -rf ~\r\u{7}").unwrap(),
            "a\nb\tc [201~rm -rf ~"
        );
        assert_eq!(sanitize_message("héllo ✓").unwrap(), "héllo ✓");
        assert_eq!(
            sanitize_message("   ").unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            sanitize_message("\u{1b}\u{0}").unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            sanitize_message(&"x".repeat(MAX_MESSAGE_CHARS + 1))
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert!(sanitize_message(&"x".repeat(MAX_MESSAGE_CHARS)).is_ok());
    }
}
