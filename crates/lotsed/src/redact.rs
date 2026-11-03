//! Beta (wp07): masks secrets in transcript text, for the UI and for exports.
//!
//! Regular expressions over the text, one per kind of secret; every match becomes [`MASK`] and is
//! counted. Best effort by nature: a secret in a shape no rule knows passes, so the rules lean
//! towards masking, but a rule that would eat ordinary prose (`sk-learn`, `Bearer authentication`,
//! `TOKEN_COUNT=5`, `ssh://git@host`) keeps the match unchanged.
//!
//! - private key blocks, whole, cut off before their END line (a clipped turn loses it), or cut off
//!   after their BEGIN line (the base64 lines before an END line are masked with it);
//! - vendor tokens: `sk-` / `sk-ant-`, `ghp_` `gho_` `ghu_` `ghs_` `ghr_` `github_pat_`, `glpat-`,
//!   `AKIA` / `ASIA`, `xox?-`, JWTs;
//! - `Bearer` / `Basic` credentials, URL userinfo (`scheme://user:password@`);
//! - assignments: `*_KEY`, `*_TOKEN`, `*_SECRET`, `*_PASSWORD`, `password=`, `api_key=`, command line
//!   flags (`--password x`), and JSON keys (`"password": "x"`). A value that is a reference
//!   (`$VAR`, `${VAR}`, `<placeholder>`) is not a secret.
//!
//! Masking is idempotent: [`MASK`] matches no rule, so a second pass changes and counts nothing.

use regex::{Captures, Regex};
use std::borrow::Cow;
use std::sync::LazyLock;

/// What a secret is replaced with.
pub const MASK: &str = "[REDACTED]";

/// `text` with its secrets masked, and how many were.
pub fn redact(text: &str) -> (Cow<'_, str>, u32) {
    let mut count = 0;
    let mut out = Cow::Borrowed(text);
    for rule in RULES.iter() {
        // a cheap substring test first: most text holds none of a rule's markers
        if !rule.markers.iter().any(|m| out.contains(m)) {
            continue;
        }
        let replaced = rule
            .re
            .replace_all(&out, |caps: &Captures| match (rule.mask)(caps) {
                Some(masked) => {
                    count += 1;
                    masked
                }
                None => caps[0].to_owned(),
            });
        if let Cow::Owned(s) = replaced {
            out = Cow::Owned(s);
        }
    }
    (out, count)
}

struct Rule {
    re: Regex,
    /// The rule can only match text that holds one of these.
    markers: &'static [&'static str],
    /// The replacement of a match; `None` leaves it as it is.
    mask: fn(&Captures) -> Option<String>,
}

fn rule(
    pattern: &str,
    markers: &'static [&'static str],
    mask: fn(&Captures) -> Option<String>,
) -> Rule {
    Rule {
        re: Regex::new(pattern).expect("redaction pattern"),
        markers,
        mask,
    }
}

fn whole(_: &Captures) -> Option<String> {
    Some(MASK.to_owned())
}

/// `prefix` (group 1) kept, the rest masked.
fn after_prefix(caps: &Captures) -> Option<String> {
    Some(format!("{}{MASK}", &caps[1]))
}

fn has_digit(s: &str) -> bool {
    s.bytes().any(|b| b.is_ascii_digit())
}

static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        rule(
            r"(?s)-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY(?: BLOCK)?-----.*?(?:-----END (?:[A-Z0-9]+ )*PRIVATE KEY(?: BLOCK)?-----|\z)",
            &["-----BEGIN "],
            whole,
        ),
        // what is left of a key whose BEGIN line was cut away (a chunk, the window, a turn of its
        // own): the base64 lines before an END line go with it
        rule(
            r"(?m)(?:^[ \t]*[A-Za-z0-9+/=]{8,}[ \t]*\r?\n)*-----END (?:[A-Z0-9]+ )*PRIVATE KEY(?: BLOCK)?-----",
            &["-----END "],
            whole,
        ),
        // `scheme://user:password@host`, or a token as the user name
        rule(
            r"\b([A-Za-z][A-Za-z0-9+.-]*://)([^\s/@?#]+)@",
            &["://"],
            |caps| {
                let userinfo = &caps[2];
                (userinfo.contains(':') || userinfo.len() >= 20)
                    .then(|| format!("{}{MASK}@", &caps[1]))
            },
        ),
        rule(
            r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]*",
            &["eyJ"],
            whole,
        ),
        // `sk-ant-…` and `sk-proj-…`: real keys are long and mixed, `sk-learn-pipeline` is a word
        rule(r"\bsk-[A-Za-z0-9_-]{20,}", &["sk-"], |caps| {
            has_digit(&caps[0]).then(|| MASK.to_owned())
        }),
        rule(
            r"\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,})",
            &["ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_"],
            whole,
        ),
        rule(r"\bglpat-[A-Za-z0-9_-]{20,}", &["glpat-"], whole),
        rule(r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b", &["AKIA", "ASIA"], whole),
        rule(r"\bxox[abprs]-[A-Za-z0-9-]{10,}", &["xox"], whole),
        rule(
            r"(?i)\b(bearer\s+)([A-Za-z0-9._~+/=-]{16,})",
            &["earer", "EARER"],
            |caps| {
                let token = &caps[2];
                (has_digit(token) || token.len() >= 32).then(|| format!("{}{MASK}", &caps[1]))
            },
        ),
        rule(
            r"(?i)\b(authorization[ \t]*[:=][ \t]*(?:basic|token)[ \t]+)([A-Za-z0-9._~+/=-]{8,})",
            &["uthorization", "UTHORIZATION"],
            after_prefix,
        ),
        // `NAME=value`: the value is masked, the name stays
        rule(
            r#"\b((?:(?i:[a-z0-9_]*(?:password|passwd|secret|token))|(?i:[a-z0-9_]*(?:api|access|secret|private|auth|encryption|signing)_?key)|[A-Z][A-Z0-9_]*_KEY|KEY)[ \t]*=[ \t]*)("[^"\n]*"|'[^'\n]*'|[^\s"']+)"#,
            &["="],
            |caps| is_secret_value(&caps[2]).then(|| format!("{}{MASK}", &caps[1])),
        ),
        rule(
            r"(?i)(--(?:[a-z-]*-)?(?:token|password|secret|api-key)(?:=|[ \t]+))(\S+)",
            &["--"],
            |caps| is_secret_value(&caps[2]).then(|| format!("{}{MASK}", &caps[1])),
        ),
        rule(
            r#"(?i)("(?:password|passwd|secret|token|api_?key|access_token|refresh_token|client_secret|private_key|auth_token|authorization)"[ \t]*:[ \t]*)"(?:[^"\\]|\\.)*""#,
            &["\":", "\" :"],
            |caps| {
                (!caps[0].ends_with(&format!("\"{MASK}\"")))
                    .then(|| format!("{}\"{MASK}\"", &caps[1]))
            },
        ),
    ]
});

/// Is `value` (an assignment's right-hand side, quotes included) something to mask? Not a reference
/// to another variable, a placeholder, an empty string, a flag, a boolean or what is already masked.
fn is_secret_value(value: &str) -> bool {
    let bare = value.trim_matches(|c| c == '"' || c == '\'');
    !(bare.is_empty()
        || bare.starts_with(['$', '<', '-', '['])
        || matches!(
            bare.to_ascii_lowercase().as_str(),
            "true" | "false" | "null" | "none" | "nil"
        ))
}
