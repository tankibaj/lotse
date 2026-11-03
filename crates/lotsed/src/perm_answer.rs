//! Answer a permission prompt the way a user would: read the option list off the visible screen and
//! type the key of the matching option (WP: permission-card buttons).
//!
//! Key mapping, from Orca (`src/shared/native-chat/native-chat-agent-support.ts`,
//! `native-chat-approval-envelope.ts`): Claude Code and Codex take the option's digit (Orca sends `1`
//! to allow); OpenCode takes Enter (its prompt starts on "Allow once"); every agent takes Esc to
//! deny. Orca has no "always" answer; the digit of the "don't ask again" option is this module's own.
//! A prompt without one (Claude Code's "can't be checked" Bash prompts) gets "Yes, and switch to auto
//! mode" for Always instead (Naim's call): from then on the session stops asking.
//! Unverified: OpenCode's "Allow always" is a second confirm step whose keys no recording shows, so
//! it is not answered.

use lotse_proto::{AgentKind, PermAlways, PermDecision};

const ESC: &str = "\x1b";
const ENTER: &str = "\r";
/// Selection markers: Claude Code `❯`, Codex `›`, plain `>`.
const MARKERS: [char; 3] = ['❯', '›', '>'];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerError {
    /// The screen shows no permission prompt this module can read.
    NoPrompt,
    /// The prompt has no option for the decision (e.g. no "don't ask again").
    NoChoice,
}

/// The keystrokes that give `decision` to the prompt on `rows` (the visible screen, top to bottom).
pub fn answer_keys(
    agent: AgentKind,
    rows: &[String],
    decision: PermDecision,
) -> Result<String, AnswerError> {
    match agent {
        AgentKind::ClaudeCode | AgentKind::Codex => numbered(rows, decision),
        AgentKind::OpenCode => opencode(rows, decision),
        AgentKind::Terminal => Err(AnswerError::NoPrompt),
    }
}

/// What Always would pick on the prompt on screen (the card's label); `None` when no prompt can be
/// read there yet.
pub fn always_option(agent: AgentKind, rows: &[String]) -> Option<PermAlways> {
    match answer_keys(agent, rows, PermDecision::Always) {
        Err(AnswerError::NoPrompt) => None,
        Err(AnswerError::NoChoice) => Some(PermAlways::Unavailable),
        Ok(_) => numbered_choices(rows)
            .and_then(|choices| always_choice(&choices).map(|(_, kind)| kind))
            .or(Some(PermAlways::Remember)),
    }
}

struct Choice {
    n: u32,
    text: String,
}

fn numbered(rows: &[String], decision: PermDecision) -> Result<String, AnswerError> {
    let choices = numbered_choices(rows).ok_or(AnswerError::NoPrompt)?;
    let pick = match decision {
        PermDecision::Allow => choices.iter().find(|c| is_plain_yes(&c.text)),
        PermDecision::Always => always_choice(&choices).map(|(c, _)| c),
        PermDecision::Deny => choices.iter().find(|c| is_no(&c.text)),
    };
    match (pick, decision) {
        (Some(c), _) => Ok(c.n.to_string()),
        (None, PermDecision::Deny) => Ok(ESC.to_owned()),
        (None, _) => Err(AnswerError::NoChoice),
    }
}

/// A "don't ask again" option, else the switch to auto mode.
fn always_choice(choices: &[Choice]) -> Option<(&Choice, PermAlways)> {
    let find = |f: fn(&str) -> bool| choices.iter().find(|c| f(&c.text));
    find(is_always)
        .map(|c| (c, PermAlways::Remember))
        .or_else(|| find(is_auto_mode_switch).map(|c| (c, PermAlways::AutoMode)))
}

fn opencode(rows: &[String], decision: PermDecision) -> Result<String, AnswerError> {
    let has = |needle: &str| rows.iter().any(|r| r.to_lowercase().contains(needle));
    if !(has("permission required") && has("allow once")) {
        return Err(AnswerError::NoPrompt);
    }
    match decision {
        PermDecision::Allow => Ok(ENTER.to_owned()),
        PermDecision::Deny => Ok(ESC.to_owned()),
        // TODO(opencode-always): needs a recorded "Allow always" confirm step; revisit with one.
        PermDecision::Always => Err(AnswerError::NoChoice),
    }
}

/// The lowest numbered option list on the screen: it starts at "1.", counts up, has a selection
/// marker on one option, and its first option starts with "Yes". A composer draft such as
/// "❯ 1. fix the test" or a question picker is not a permission prompt.
fn numbered_choices(rows: &[String]) -> Option<Vec<Choice>> {
    let start = rows
        .iter()
        .rposition(|r| parse_option(r).is_some_and(|o| o.n == 1))?;
    let mut choices: Vec<Choice> = Vec::new();
    let mut text_col = 0;
    let mut marked = false;
    for row in &rows[start..] {
        let row = row.trim_end();
        match parse_option(row) {
            Some(o) if o.n as usize == choices.len() + 1 => {
                marked |= o.marked;
                text_col = o.col;
                choices.push(Choice {
                    n: o.n,
                    text: o.text.to_owned(),
                });
            }
            // a wrapped option continues indented under its text
            None if !row.trim().is_empty() && indent(row) >= text_col => {
                if let Some(last) = choices.last_mut() {
                    last.text.push(' ');
                    last.text.push_str(row.trim());
                }
            }
            _ => break,
        }
    }
    let first_is_yes = choices
        .first()
        .is_some_and(|c| strip_hint(&c.text).to_lowercase().starts_with("yes"));
    (choices.len() >= 2 && marked && first_is_yes && choices.iter().all(|c| c.n <= 9))
        .then_some(choices)
}

struct Opt<'a> {
    n: u32,
    text: &'a str,
    marked: bool,
    /// Column (chars) where the option text starts.
    col: usize,
}

fn indent(row: &str) -> usize {
    row.chars().take_while(|c| c.is_whitespace()).count()
}

/// `❯ 1. Yes`, `  2. Yes, and ...`.
fn parse_option(row: &str) -> Option<Opt<'_>> {
    let row = row.trim_end();
    let t = row.trim_start();
    let (marked, rest) = match t.chars().next() {
        Some(c) if MARKERS.contains(&c) => (true, t[c.len_utf8()..].trim_start()),
        _ => (false, t),
    };
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits > 2 {
        return None;
    }
    let text = rest[digits..].strip_prefix(". ")?.trim_start();
    if text.is_empty() {
        return None;
    }
    Some(Opt {
        n: rest[..digits].parse().ok()?,
        text,
        marked,
        col: row.chars().count() - text.chars().count(),
    })
}

/// Drop a trailing key hint: Codex "(y)" / "(p)" / "(esc)", Claude "(shift+tab)".
fn strip_hint(text: &str) -> &str {
    let t = text.trim_end();
    match (t.strip_suffix(')'), t.rfind(" (")) {
        (Some(_), Some(i)) if !t[i + 2..t.len() - 1].contains(' ') => t[..i].trim_end(),
        _ => t,
    }
}

/// "Yes", Codex's "Yes, proceed" / "Yes, continue" or a folder-trust dialog's "Yes, I trust this
/// folder": no scope, mode switch or other qualifier.
fn is_plain_yes(text: &str) -> bool {
    let t = strip_hint(text).to_lowercase();
    matches!(t.as_str(), "yes" | "yes, proceed" | "yes, continue") || t.starts_with("yes, i trust")
}

/// A "Yes, ..." that stops the question coming back. A mode switch ("switch to auto mode", "accept
/// edits") changes how the agent runs, not what it may do: never picked.
fn is_always(text: &str) -> bool {
    let t = strip_hint(text).to_lowercase().replace('\u{2019}', "'");
    t.starts_with("yes")
        && !t.contains("switch to")
        && !t.contains("auto mode")
        && [
            "don't ask again",
            "do not ask again",
            "always",
            "this project",
            "this session",
        ]
        .iter()
        .any(|k| t.contains(k))
}

/// "Yes, and switch to auto mode": Always's fallback when no option stops just this question.
/// The accept-edits switch and plan approval's "use auto mode" are not it.
fn is_auto_mode_switch(text: &str) -> bool {
    let t = strip_hint(text).to_lowercase();
    t.starts_with("yes") && t.contains("switch to auto mode")
}

/// "No" or "No, and tell Claude what to do differently"; not "None" or "Not now".
fn is_no(text: &str) -> bool {
    let t = text.to_lowercase();
    t.strip_prefix("no")
        .is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use AgentKind::{ClaudeCode, Codex, OpenCode, Terminal};
    use PermDecision::{Allow, Always, Deny};

    fn screen(s: &str) -> Vec<String> {
        s.lines().map(str::to_owned).collect()
    }

    fn keys(agent: AgentKind, s: &str, d: PermDecision) -> Result<String, AnswerError> {
        answer_keys(agent, &screen(s), d)
    }

    // Claude Code 2.1.x Bash prompt with four options (the screenshot of the bug report).
    const CLAUDE_4: &str = "\
● Bash(npx playwright test e2e/patients)

 Bash command

   npx playwright test e2e/patients
   Run the patient e2e suite

 Do you want to proceed?
 ❯ 1. Yes
   2. Yes, allow reading from patients/ from this project
   3. Yes, and switch to auto mode (shift+tab)
   4. No

 Esc to cancel · Tab to amend";

    const CLAUDE_3: &str = "\
 Bash command

   npm test

 Do you want to proceed?
 ❯ 1. Yes
   2. Yes, and don't ask again for npm test commands in /repo
   3. No, and tell Claude what to do differently (esc)";

    const CLAUDE_YES_NO: &str = "\
 Do you want to create hello.txt?
 ❯ 1. Yes
   2. No

 Esc to cancel · Tab to amend";

    // Claude Code 2.1.310, as captured in Orca's tests.
    const CLAUDE_EDIT: &str = "\
 Do you want to create hello.txt?
 ❯ 1. Yes
   2. Yes, and switch to accept edits (shift+tab)
   3. No
 Esc to cancel · Tab to amend";

    const CLAUDE_PLAN: &str = "\
 Claude has written up a plan and is ready to execute. Would you like to proceed?
 ❯ 1. Yes, and use auto mode
   2. Yes, manually approve edits
   3. Tell Claude what to change";

    // Codex 0.158.0, from Orca's recorded approval transcript.
    const CODEX: &str = "\
• Running touch approved.txt

  Would you like to run the following command?

  Reason: May I create approved.txt in the current workspace as requested?

  $ touch approved.txt

› 1. Yes, proceed (y)
  2. Yes, and don't ask again for commands that start with `touch approved.txt` (p)
  3. No, and tell Codex what to do differently (esc)

  Press enter to confirm or esc to cancel";

    const OPENCODE: &str = "\
  △ Permission required
    ← Access external directory ~/projects

  Patterns
  - /Users/dev/projects/*

   Allow once   Allow always   Reject
  ⇆ select  enter confirm";

    // The folder-trust dialogs both agents show before a session starts (reconstructed from the
    // dialogs' wording; no recording): the "yes" is worded "Yes, I trust this folder" / "Yes, continue".
    const CLAUDE_TRUST: &str = "\
 Accessing workspace:

 /Users/dev/projects/lotse

 Quick safety check: Is this a project you created or one you trust?

 ❯ 1. Yes, I trust this folder
   2. No, exit

 Enter to confirm · Esc to cancel";

    const CLAUDE_TRUST_OLD: &str = "\
 Do you trust the files in this folder?

 /Users/dev/projects/lotse

 ❯ 1. Yes, proceed
   2. No, exit

 Enter to confirm · Esc to cancel";

    const CODEX_TRUST: &str = "\
> You are in /Users/dev/projects/lotse

  Do you trust the contents of this directory? Working with untrusted contents comes with
  higher risk of prompt injection.

› 1. Yes, continue
  2. No, quit

  Press enter to continue";

    #[test]
    fn a_folder_trust_dialog_is_answered_with_its_yes_and_no() {
        for (agent, s) in [
            (ClaudeCode, CLAUDE_TRUST),
            (ClaudeCode, CLAUDE_TRUST_OLD),
            (Codex, CODEX_TRUST),
        ] {
            assert_eq!(keys(agent, s, Allow), Ok("1".into()), "{s}");
            assert_eq!(keys(agent, s, Deny), Ok("2".into()), "{s}");
            // nothing stops the question coming back: the card hides Always
            assert_eq!(keys(agent, s, Always), Err(AnswerError::NoChoice), "{s}");
            assert_eq!(
                always_option(agent, &screen(s)),
                Some(PermAlways::Unavailable),
                "{s}"
            );
        }
    }

    #[test]
    fn claude_four_options_allow_always_deny() {
        assert_eq!(keys(ClaudeCode, CLAUDE_4, Allow), Ok("1".into()));
        assert_eq!(keys(ClaudeCode, CLAUDE_4, Always), Ok("2".into()));
        assert_eq!(keys(ClaudeCode, CLAUDE_4, Deny), Ok("4".into()));
    }

    #[test]
    fn claude_always_switches_to_auto_mode_only_without_a_dont_ask_again() {
        // the "A variable in this command can't be checked" prompt of the bug report
        let only_auto = "\
 Do you want to proceed?
 ❯ 1. Yes
   2. Yes, and switch to auto mode · auto mode handles these prompts for you
   3. No";
        assert_eq!(keys(ClaudeCode, only_auto, Always), Ok("2".into()));
        assert_eq!(keys(ClaudeCode, only_auto, Allow), Ok("1".into()));
        assert_eq!(keys(ClaudeCode, only_auto, Deny), Ok("3".into()));
        // with both, the narrower "this project" wins
        assert_eq!(keys(ClaudeCode, CLAUDE_4, Always), Ok("2".into()));
    }

    #[test]
    fn always_option_names_what_always_would_pick() {
        let label = |agent, s: &str| always_option(agent, &screen(s));
        let only_auto =
            " Do you want to proceed?\n ❯ 1. Yes\n   2. Yes, and switch to auto mode\n   3. No";
        assert_eq!(label(ClaudeCode, CLAUDE_4), Some(PermAlways::Remember));
        assert_eq!(label(Codex, CODEX), Some(PermAlways::Remember));
        assert_eq!(label(ClaudeCode, only_auto), Some(PermAlways::AutoMode));
        assert_eq!(
            label(ClaudeCode, CLAUDE_YES_NO),
            Some(PermAlways::Unavailable)
        );
        assert_eq!(
            label(ClaudeCode, CLAUDE_EDIT),
            Some(PermAlways::Unavailable)
        );
        assert_eq!(label(OpenCode, OPENCODE), Some(PermAlways::Unavailable));
        assert_eq!(label(ClaudeCode, "$ ls"), None);
    }

    #[test]
    fn claude_three_options_with_dont_ask_again() {
        assert_eq!(keys(ClaudeCode, CLAUDE_3, Allow), Ok("1".into()));
        assert_eq!(keys(ClaudeCode, CLAUDE_3, Always), Ok("2".into()));
        assert_eq!(keys(ClaudeCode, CLAUDE_3, Deny), Ok("3".into()));
    }

    #[test]
    fn claude_yes_no_only_makes_always_a_no_op() {
        assert_eq!(keys(ClaudeCode, CLAUDE_YES_NO, Allow), Ok("1".into()));
        assert_eq!(keys(ClaudeCode, CLAUDE_YES_NO, Deny), Ok("2".into()));
        assert_eq!(
            keys(ClaudeCode, CLAUDE_YES_NO, Always),
            Err(AnswerError::NoChoice)
        );
        // the accept-edits switch is a mode change, not an always
        assert_eq!(
            keys(ClaudeCode, CLAUDE_EDIT, Always),
            Err(AnswerError::NoChoice)
        );
        assert_eq!(keys(ClaudeCode, CLAUDE_EDIT, Deny), Ok("3".into()));
    }

    #[test]
    fn a_wrapped_option_is_read_whole() {
        let wrapped = "\
 Do you want to proceed?
 ❯ 1. Yes
   2. Yes, allow reading from /very/long/path/to/some/place
      from this project
   3. No";
        assert_eq!(keys(ClaudeCode, wrapped, Always), Ok("2".into()));
        assert_eq!(keys(ClaudeCode, wrapped, Deny), Ok("3".into()));
    }

    #[test]
    fn plan_approval_cannot_be_allowed_and_denies_with_escape() {
        assert_eq!(
            keys(ClaudeCode, CLAUDE_PLAN, Allow),
            Err(AnswerError::NoChoice)
        );
        assert_eq!(
            keys(ClaudeCode, CLAUDE_PLAN, Always),
            Err(AnswerError::NoChoice)
        );
        assert_eq!(keys(ClaudeCode, CLAUDE_PLAN, Deny), Ok(ESC.into()));
    }

    #[test]
    fn codex_digits_with_key_hints_stripped() {
        assert_eq!(keys(Codex, CODEX, Allow), Ok("1".into()));
        assert_eq!(keys(Codex, CODEX, Always), Ok("2".into()));
        assert_eq!(keys(Codex, CODEX, Deny), Ok("3".into()));
    }

    #[test]
    fn opencode_enter_allows_escape_denies_and_always_is_not_answered() {
        assert_eq!(keys(OpenCode, OPENCODE, Allow), Ok("\r".into()));
        assert_eq!(keys(OpenCode, OPENCODE, Deny), Ok(ESC.into()));
        assert_eq!(keys(OpenCode, OPENCODE, Always), Err(AnswerError::NoChoice));
    }

    #[test]
    fn screens_without_a_prompt_send_nothing() {
        let idle = "\
● Done. All 12 tests pass.

────────────────────────────────
❯ 
────────────────────────────────
  ⏵⏵ bypass permissions on (shift+tab to cycle)";
        let draft = "\
────────────────────────────────
❯ 1. fix the flaky test
────────────────────────────────";
        let picker = "\
 Which database?
 ❯ 1. Postgres
   2. SQLite
 Enter to select · ↑/↓ to navigate · Esc to cancel";
        let prose = "\
Steps:
1. Yes, do this first
2. No, do that later";
        let opencode_idle = "  Ask anything...\n  ctrl+p commands";
        for d in [Allow, Always, Deny] {
            for s in [idle, draft, picker, prose, ""] {
                assert_eq!(keys(ClaudeCode, s, d), Err(AnswerError::NoPrompt), "{s}");
                assert_eq!(keys(Codex, s, d), Err(AnswerError::NoPrompt), "{s}");
            }
            assert_eq!(keys(OpenCode, opencode_idle, d), Err(AnswerError::NoPrompt));
            assert_eq!(keys(Terminal, CLAUDE_4, d), Err(AnswerError::NoPrompt));
        }
    }

    #[test]
    fn an_answered_prompt_higher_up_is_ignored_for_the_one_below() {
        let two = format!("{CLAUDE_3}\n\n● Ran npm test\n\n{CLAUDE_YES_NO}");
        assert_eq!(
            keys(ClaudeCode, &two, Always),
            Err(AnswerError::NoChoice),
            "the lowest prompt is the live one"
        );
    }

    #[test]
    fn no_is_matched_as_a_word() {
        assert!(is_no("No"));
        assert!(is_no("No, and tell Codex what to do differently (esc)"));
        assert!(!is_no("None of the above"));
        assert!(!is_no("Not now"));
    }
}
