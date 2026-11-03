//! Beta (wp07): masks secrets in transcript text, for the UI and for exports.
//!
//! Regular expressions over the text, one per kind of secret; every match becomes [`MASK`] and is
//! counted. Best effort by nature: a secret in a shape no rule knows passes, so the rules lean
//! towards masking, but a rule that would eat ordinary prose (`sk-learn`, `Bearer authentication`,
//! `TOKEN_COUNT=5`, `ssh://git@host`) keeps the match unchanged.
//!
//! - private key blocks, whole, cut off before their END line (a clipped turn loses it), or cut off
//!   after their BEGIN line (the base64 lines before an END line are masked with it, the last one
//!   even when short: `Ag==`);
//! - vendor tokens: `sk-` / `sk-ant-`, `ghp_` `gho_` `ghu_` `ghs_` `ghr_` `github_pat_`, `glpat-`,
//!   `AKIA` / `ASIA`, `xox?-`, JWTs;
//! - `Bearer` / `Basic` credentials, URL userinfo (`scheme://user:password@`);
//! - assignments: `*_KEY`, `*_TOKEN`, `*_SECRET`, `*_PASSWORD`, `password=`, `api_key=`, command line
//!   flags (`--password x`), JSON keys (`"password": "x"`) and colon forms (YAML `password: x`,
//!   headers `X-Api-Key: x`, `Authorization: x`). A value that is a reference (`$VAR`, `${VAR}`,
//!   `<placeholder>`) is not a secret; neither is a type word after a colon (`token: string`).
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
        // own): the base64 lines before an END line go with it. A key's last body line is often
        // short (`Ag==`, `AQAB`, `=`): one such line may sit right before END, the others need 8+
        // characters so ordinary text is not masked
        rule(
            r"(?m)(?:^[ \t]*[A-Za-z0-9+/=]{8,}[ \t]*\r?\n)*(?:^[ \t]*[A-Za-z0-9+/=]{1,7}[ \t]*\r?\n)?-----END (?:[A-Z0-9]+ )*PRIVATE KEY(?: BLOCK)?-----",
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
        // `name: value` (YAML, headers, dict literals): the secret names of the `=` form, hyphens
        // allowed for headers (`X-Api-Key`), plus `Authorization`. Quoted values always count; a
        // bare value only when it is the whole value (end of line, `,` `;` `}` `]` or a comment), so
        // prose after the colon and type annotations (`token: string`) stay
        rule(
            &format!(r#"(?i)\b({COLON_NAME}["']?[ \t]*:[ \t]*)("(?:[^"\\\n]|\\.)*"|'[^'\n]*')"#),
            &[":"],
            |caps| is_secret_value(&caps[2]).then(|| format!("{}{MASK}", &caps[1])),
        ),
        rule(
            &format!(
                r#"(?im)\b({COLON_NAME}["']?[ \t]*:[ \t]*)([^\s"',;}}\]\[]+)([ \t]*(?:[,;}}\]#]|\r?$))"#
            ),
            &[":"],
            |caps| {
                is_bare_secret_value(&caps[2]).then(|| format!("{}{MASK}{}", &caps[1], &caps[3]))
            },
        ),
    ]
});

/// Names a colon form masks: the `=` form's secret names, with `-` for headers.
const COLON_NAME: &str = r"(?:[a-z0-9_-]*(?:password|passwd|secret|token)|[a-z0-9_-]*(?:api|access|secret|private|auth|encryption|signing)[-_]?key|(?:proxy-)?authorization)";

/// [`is_secret_value`] for an unquoted value after a colon, which prose and type annotations also
/// produce: YAML tags and block scalars, template syntax, and the words that say "no value here".
fn is_bare_secret_value(value: &str) -> bool {
    is_secret_value(value)
        && !value.starts_with(['{', '(', '!', '|', '>', '&', '*', '%'])
        && !matches!(
            value.to_ascii_lowercase().as_str(),
            "string"
                | "str"
                | "number"
                | "int"
                | "integer"
                | "float"
                | "bool"
                | "boolean"
                | "any"
                | "object"
                | "array"
                | "yes"
                | "no"
                | "on"
                | "off"
                | "required"
                | "optional"
                | "bearer"
                | "basic"
                | "token"
                | "password"
                | "secret"
                | "expired"
                | "invalid"
                | "missing"
                | "valid"
                | "set"
                | "unset"
                | "empty"
                | "undefined"
                | "redacted"
                | "hidden"
                | "masked"
                | "ok"
                | "error"
                | "n/a"
        )
}

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

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = "KEYBODY00aB3xaB3xaB3xaB3x\nKEYBODY01aB3xaB3xaB3xaB3x";
    const END: &str = "-----END RSA PRIVATE KEY-----";

    #[test]
    fn colon_forms_are_masked_like_the_equals_form() {
        let cases = [
            ("password: hunter2", "password: [REDACTED]", 1),
            ("api_key: sk_live_abc", "api_key: [REDACTED]", 1),
            ("X-Api-Key: abc123", "X-Api-Key: [REDACTED]", 1),
            ("x-auth-token: abc123", "x-auth-token: [REDACTED]", 1),
            (
                "Authorization: abc123def456",
                "Authorization: [REDACTED]",
                1,
            ),
            (
                "Proxy-Authorization: abc123def456",
                "Proxy-Authorization: [REDACTED]",
                1,
            ),
            ("client_secret: \"s3 cr3t\"", "client_secret: [REDACTED]", 1),
            ("accessToken: 'abc123'", "accessToken: [REDACTED]", 1),
            (
                "  db_password: hunter2 # dev",
                "  db_password: [REDACTED] # dev",
                1,
            ),
            (
                "{password: hunter2, user: bob}",
                "{password: [REDACTED], user: bob}",
                1,
            ),
            ("'api_key': 'abcd'", "'api_key': [REDACTED]", 1),
            (
                "services:\r\n  db:\r\n    password: hunter2\r\n    user: bob\r\n",
                "services:\r\n  db:\r\n    password: [REDACTED]\r\n    user: bob\r\n",
                1,
            ),
            (
                "A_KEY=1\npassword: hunter2",
                "A_KEY=[REDACTED]\npassword: [REDACTED]",
                2,
            ),
        ];
        for (input, want, count) in cases {
            let (out, n) = redact(input);
            assert_eq!((out.as_ref(), n), (want, count), "input: {input}");
            assert_eq!(redact(&out).1, 0, "not idempotent: {out}");
        }
        // the existing forms still count once
        let (out, n) = redact("Authorization: Basic dXNlcjpwYXNzd29yZA==");
        assert_eq!((out.as_ref(), n), ("Authorization: Basic [REDACTED]", 1));
        let (out, n) = redact(r#"{"password": "hunter2", "api_key":"abcd"}"#);
        assert_eq!(n, 2, "{out}");
    }

    #[test]
    fn colon_forms_that_are_not_secrets_are_left_alone() {
        for input in [
            "password: string",
            "token: string;",
            "password?: string",
            "api_key: Optional[str]",
            "token: str",
            "password: required",
            "Token: expired",
            "api_key: ${API_KEY}",
            "API_KEY: $OTHER_VAR",
            "X-Api-Key: ${KEY}",
            "password: <your password>",
            "password: !vault |",
            "private_key: |",
            "secret: ${{ secrets.FOO }}",
            "password: \"\"",
            "password:",
            "secret:\n  name: db\n",
            "Token: the count of tokens",
            "Note: set a password: in the settings page",
            "Authorization: Bearer authentication is required",
            "Authorization: required",
            "max_tokens: 4096",
            "token_count: 5",
            "password_file: /run/secrets/db",
            "sort_key: name",
            r#"{"token_count": 5, "name": "x"}"#,
            "def f(password: str = x): pass",
            "ssh://git@host:22/repo",
            "see https://example.com/token: ok soon",
        ] {
            assert_eq!(redact(input).1, 0, "masked: {input} -> {}", redact(input).0);
            assert_eq!(redact(input).0, input);
        }
    }

    #[test]
    fn an_orphan_key_body_ending_in_a_short_line_is_masked_whole() {
        // a key's last base64 line is often short: padding (`Ag==`), an exponent (`AQAB`), `=`
        for short in ["Ag==", "AQAB", "="] {
            let text = format!("before\n{BODY}\n{short}\n{END}\nafter");
            let (out, n) = redact(&text);
            assert_eq!(out, "before\n[REDACTED]\nafter", "{short}");
            assert_eq!(n, 1, "{short}");
        }
    }

    #[test]
    fn only_one_short_line_goes_with_the_end_line() {
        let text = format!("intro\nab\ncd\n{END}");
        let (out, n) = redact(&text);
        assert_eq!(out, "intro\nab\n[REDACTED]");
        assert_eq!(n, 1);
    }

    #[test]
    fn ordinary_text_before_an_end_line_is_not_masked() {
        // prose has spaces; a short word inside a sentence is not a body line
        let text = format!("The build finished\nand it ended with ok\n{END}");
        assert_eq!(
            redact(&text).0,
            format!("The build finished\nand it ended with ok\n[REDACTED]")
        );
        // a short last word of a paragraph before an unrelated line (no key end) is left alone
        let text = "Some ordinary paragraph that ends in a word\nok\nthe next, unrelated line\n";
        let (out, n) = redact(text);
        assert_eq!((out.as_ref(), n), (text, 0));
    }
}
