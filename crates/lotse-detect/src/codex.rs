//! Codex hook adapter: maps Codex hook events to Lotse status. Source: [`DetectSource::Hook`].
//!
//! # Where the events come from
//!
//! `lotse-hook codex --injected` forwards each hook payload as `Request::HookEvent`. Codex runs it
//! because `lotsed` points every pane's `CODEX_HOME` at a mirror of the user's home whose
//! `hooks.json` lists it for each event below and whose `config.toml` trusts it
//! (`lotse_hook::codex_home`, `integrations/codex/README.md`); nothing in `~/.codex` changes.
//!
//! The event names are the ones of the Codex 0.162.1 binary, and of Orca's `CODEX_EVENTS`
//! (`src/main/codex/codex-hook-definition.ts`). The payload fields the adapter reads are those of the
//! binary's hook input struct: `hook_event_name`, `agent_id` + `agent_type` (a sub-agent's thread),
//! `source` (`SessionStart`), `tool_name`, `tool_use_id`, `tool_input`. Everything else is ignored, and
//! an unknown event name or a missing field never changes the status.
//!
//! # Status table
//!
//! Codex's hooks are Claude Code's with a few differences, so this reuses [`crate::claude::ClaudeAdapter`]
//! (lead and sub-agents tracked separately, session status derived: lead `Perm`, else a sub-agent
//! `Perm`, else lead `Run`, else `Run` while a sub-agent runs, else the lead's state). Rows that
//! differ from the table in `claude.rs`:
//!
//! | Event | Context | Effect |
//! |---|---|---|
//! | `SessionStart` | `source` = `startup` / `resume` / `clear` / `fork` | lead `Run` (Codex fires it as a turn begins, as Orca reads it), sub-agents cleared |
//! | `SessionStart` | `source` = `compact` | no change |
//! | `SessionStart`, `UserPromptSubmit`, `Stop`, `Interrupt` | sub-agent (`agent_id` set) | that sub-agent `Run`; the lead is untouched (its own end is `SubagentStop`) |
//! | `PreToolUse` | lead, `tool_name` = `request_user_input` | lead `Perm`: the model asked the user a question (Orca: `isAskUserQuestionTool`) |
//! | `Interrupt` | | lead `Done` (Esc cancels a turn and Codex fires no `Stop`) |
//! | `PreToolUse`, `PostToolUse` | lead / sub-agent | as in `claude.rs` (`perm.cleared_by_*`, `tool.sub`) |
//! | `PermissionRequest`, `UserPromptSubmit`, `Stop`, `SubagentStart`, `SubagentStop` | | as in `claude.rs` |
//! | `PreCompact`, `PostCompact`, anything else | | ignored |
//!
//! Codex has no `StopFailure` and no `PostToolUseFailure`, so this adapter never reports `Fail`.
//!
//! TODO(codex-failed-turn): a turn that fails (API or network error) fires no hook at all, so the
//! status stays `Run` until the next prompt or `Interrupt`, and the screen manifest cannot correct
//! it (hook beats screen). Orca reads the rollout file for it (`codexTranscriptTurnEnd`); revisit
//! when Codex adds a failure hook, or when Lotse tails `transcript_path`.
//!
//! **Unverified:** payloads were not recorded from a live Codex (tests never run one); the fixtures
//! in `integrations/codex/fixtures/` are shaped from the binary's field names. With Codex's
//! approval auto-review on, `PermissionRequest` also fires for requests the reviewer answers, so
//! `Perm` can flash for a moment (Orca reads the rollout to tell them apart).

use crate::adapter::Adapter;
use crate::claude::ClaudeAdapter;
use lotse_proto::{AgentKind, SessionId};

/// Called by `Engine::with_default_adapters`: the hook adapter for `AgentKind::Codex`.
pub fn default_adapter(_session: &SessionId, agent: AgentKind) -> Option<Box<dyn Adapter>> {
    (agent == AgentKind::Codex)
        .then(|| Box::new(ClaudeAdapter::for_agent(AgentKind::Codex)) as Box<dyn Adapter>)
}

/// The tool Codex gives the model to ask the user something. Orca matches it as
/// `askuserquestion` / `requestuserinput` after dropping every non-alphanumeric character.
pub(crate) fn is_user_input_tool(tool_name: &str) -> bool {
    let normalized: String = tool_name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    normalized == "requestuserinput" || normalized == "askuserquestion"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{HookObservation, Observation};

    #[test]
    fn only_codex_gets_the_codex_adapter_and_it_ignores_other_agents_hooks() {
        let id = SessionId::new("s");
        assert!(default_adapter(&id, AgentKind::ClaudeCode).is_none());
        assert!(default_adapter(&id, AgentKind::Terminal).is_none());
        let mut adapter = default_adapter(&id, AgentKind::Codex).unwrap();
        assert_eq!(
            (adapter.name(), adapter.agent()),
            ("codex-hooks", AgentKind::Codex)
        );
        let claude_hook = Observation::Hook(HookObservation {
            agent: AgentKind::ClaudeCode,
            payload: serde_json::json!({"hook_event_name": "UserPromptSubmit"}),
            at: 1,
        });
        assert!(adapter.observe(&claude_hook).is_empty());
    }

    #[test]
    fn the_question_tool_is_found_by_either_spelling() {
        for name in [
            "request_user_input",
            "RequestUserInput",
            "AskUserQuestion",
            "ask-user-question",
        ] {
            assert!(is_user_input_tool(name), "{name}");
        }
        for name in ["Bash", "apply_patch", "request_permissions", ""] {
            assert!(!is_user_input_tool(name), "{name}");
        }
    }
}
