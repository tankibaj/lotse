//! Minimal JSON model for editing Claude's `settings.json` without disturbing the rest of it.
//!
//! `serde_json::Value` (without its `preserve_order` feature, which would change the lockfile) sorts
//! object keys and re-parses numbers and escapes. This model keeps key order, number text and string
//! escapes exactly as written, so untouched settings survive an install/uninstall round trip.
//! Output is pretty-printed like `JSON.stringify(value, null, 2)`, the format Claude Code writes, so a
//! file in that format is reproduced byte for byte. Comments, trailing commas and other JSON5
//! extensions are rejected: the installer refuses to touch a file it cannot parse strictly.

use std::fmt;

/// Deepest nesting accepted (settings files are shallow; this bounds recursion).
const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// Number as written.
    Num(String),
    /// String content between the quotes, escapes as written. Use [`Json::string`] / [`decode`].
    Str(String),
    Arr(Vec<Json>),
    /// Members in file order; keys are raw (escaped) text like `Str`.
    Obj(Vec<(String, Json)>),
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseError {
    pub offset: usize,
    pub message: &'static str,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for ParseError {}

impl Json {
    /// A string value from plain text.
    pub fn string(text: &str) -> Json {
        Json::Str(encode(text))
    }

    /// Decoded text of a string value.
    pub fn as_string(&self) -> Option<String> {
        match self {
            Json::Str(raw) => Some(decode(raw)),
            _ => None,
        }
    }

    /// Member `key` of an object (first match).
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(members) => members
                .iter()
                .find(|(k, _)| decode(k) == key)
                .map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Json> {
        match self {
            Json::Obj(members) => members
                .iter_mut()
                .find(|(k, _)| decode(k) == key)
                .map(|(_, v)| v),
            _ => None,
        }
    }

    /// Build an object from `(plain key, value)` pairs.
    pub fn object<const N: usize>(members: [(&str, Json); N]) -> Json {
        Json::Obj(members.into_iter().map(|(k, v)| (encode(k), v)).collect())
    }
}

pub fn parse(text: &str) -> Result<Json, ParseError> {
    let mut p = Parser {
        text,
        bytes: text.as_bytes(),
        at: 0,
    };
    let value = p.value(0)?;
    p.skip_ws();
    if p.at != p.bytes.len() {
        return Err(p.err("trailing content after the JSON value"));
    }
    Ok(value)
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn err(&self, message: &'static str) -> ParseError {
        ParseError {
            offset: self.at,
            message,
        }
    }

    fn skip_ws(&mut self) {
        while matches!(self.bytes.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, ParseError> {
        if depth > MAX_DEPTH {
            return Err(self.err("nesting too deep"));
        }
        self.skip_ws();
        match self.bytes.get(self.at) {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.err("unexpected character")),
            None => Err(self.err("unexpected end of input")),
        }
    }

    fn literal(&mut self, word: &'static str, value: Json) -> Result<Json, ParseError> {
        if self.text[self.at..].starts_with(word) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(self.err("invalid literal"))
        }
    }

    fn number(&mut self) -> Result<Json, ParseError> {
        let start = self.at;
        let digits = |p: &mut Self| {
            let from = p.at;
            while matches!(p.bytes.get(p.at), Some(b'0'..=b'9')) {
                p.at += 1;
            }
            p.at - from
        };
        if self.bytes.get(self.at) == Some(&b'-') {
            self.at += 1;
        }
        match self.bytes.get(self.at) {
            Some(b'0') => self.at += 1,
            Some(b'1'..=b'9') => {
                digits(self);
            }
            _ => return Err(self.err("invalid number")),
        }
        if self.bytes.get(self.at) == Some(&b'.') {
            self.at += 1;
            if digits(self) == 0 {
                return Err(self.err("invalid number"));
            }
        }
        if matches!(self.bytes.get(self.at), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.bytes.get(self.at), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if digits(self) == 0 {
                return Err(self.err("invalid number"));
            }
        }
        Ok(Json::Num(self.text[start..self.at].to_owned()))
    }

    /// Raw string content; `self.at` is on the opening quote.
    fn string(&mut self) -> Result<String, ParseError> {
        self.at += 1;
        let start = self.at;
        loop {
            match self.bytes.get(self.at) {
                None => return Err(self.err("unterminated string")),
                Some(b'"') => break,
                Some(b'\\') => {
                    match self.bytes.get(self.at + 1) {
                        Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => {
                            self.at += 2;
                        }
                        Some(b'u') => {
                            let hex = self.bytes.get(self.at + 2..self.at + 6);
                            if !hex.is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit)) {
                                return Err(self.err("invalid \\u escape"));
                            }
                            self.at += 6;
                        }
                        _ => return Err(self.err("invalid escape")),
                    };
                }
                Some(c) if *c < 0x20 => return Err(self.err("control character in string")),
                Some(_) => self.at += 1,
            }
        }
        let raw = self.text[start..self.at].to_owned();
        self.at += 1;
        Ok(raw)
    }

    fn array(&mut self, depth: usize) -> Result<Json, ParseError> {
        self.at += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.bytes.get(self.at) == Some(&b']') {
            self.at += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.skip_ws();
            match self.bytes.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err(self.err("expected ',' or ']'")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, ParseError> {
        self.at += 1;
        let mut members = Vec::new();
        self.skip_ws();
        if self.bytes.get(self.at) == Some(&b'}') {
            self.at += 1;
            return Ok(Json::Obj(members));
        }
        loop {
            self.skip_ws();
            if self.bytes.get(self.at) != Some(&b'"') {
                return Err(self.err("expected a string key"));
            }
            let key = self.string()?;
            self.skip_ws();
            if self.bytes.get(self.at) != Some(&b':') {
                return Err(self.err("expected ':'"));
            }
            self.at += 1;
            members.push((key, self.value(depth + 1)?));
            self.skip_ws();
            match self.bytes.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Obj(members));
                }
                _ => return Err(self.err("expected ',' or '}'")),
            }
        }
    }
}

/// Escape plain text for use between JSON quotes.
pub fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Inverse of [`encode`] for any valid JSON string content (a lone surrogate becomes U+FFFD).
pub fn decode(raw: &str) -> String {
    if !raw.contains('\\') {
        return raw.to_owned();
    }
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let unit = hex4(&mut chars);
                let ch = if (0xD800..0xDC00).contains(&unit) {
                    // High surrogate: needs a following `\uXXXX` low surrogate.
                    let mut ahead = chars.clone();
                    if ahead.next() == Some('\\') && ahead.next() == Some('u') {
                        let low = hex4(&mut ahead);
                        if (0xDC00..0xE000).contains(&low) {
                            chars = ahead;
                            char::from_u32(0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    char::from_u32(unit)
                };
                out.push(ch.unwrap_or('\u{fffd}'));
            }
            // `"`, `\`, `/` and anything unexpected stand for themselves.
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

fn hex4(chars: &mut std::str::Chars<'_>) -> u32 {
    (0..4).fold(0, |acc, _| {
        acc * 16 + chars.next().and_then(|c| c.to_digit(16)).unwrap_or(0)
    })
}

/// Pretty-print like `JSON.stringify(value, null, 2)` with `indent` per level and `eol` line ends.
pub fn to_string(value: &Json, indent: &str, eol: &str) -> String {
    let mut out = String::new();
    write_value(&mut out, value, indent, eol, 0);
    out
}

fn write_value(out: &mut String, value: &Json, indent: &str, eol: &str, level: usize) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Num(n) => out.push_str(n),
        Json::Str(raw) => {
            out.push('"');
            out.push_str(raw);
            out.push('"');
        }
        Json::Arr(items) if items.is_empty() => out.push_str("[]"),
        Json::Obj(members) if members.is_empty() => out.push_str("{}"),
        Json::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, indent, eol, level + 1);
                write_value(out, item, indent, eol, level + 1);
            }
            newline(out, indent, eol, level);
            out.push(']');
        }
        Json::Obj(members) => {
            out.push('{');
            for (i, (key, item)) in members.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, indent, eol, level + 1);
                out.push('"');
                out.push_str(key);
                out.push_str("\": ");
                write_value(out, item, indent, eol, level + 1);
            }
            newline(out, indent, eol, level);
            out.push('}');
        }
    }
}

fn newline(out: &mut String, indent: &str, eol: &str, level: usize) {
    out.push_str(eol);
    for _ in 0..level {
        out.push_str(indent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_claude_style_json_byte_for_byte() {
        let text = "{\n  \"b\": 1.50e+2,\n  \"a\": [\n    \"x\\u0061\\n\",\n    null,\n    true\n  ],\n  \"e\": {},\n  \"l\": [],\n  \"ü\": \"ß/\\/\"\n}";
        let v = parse(text).unwrap();
        assert_eq!(to_string(&v, "  ", "\n"), text);
    }

    #[test]
    fn keeps_member_order_and_duplicates() {
        let v = parse(r#"{"z":1,"a":2,"z":3}"#).unwrap();
        assert_eq!(
            to_string(&v, "\t", "\r\n"),
            "{\r\n\t\"z\": 1,\r\n\t\"a\": 2,\r\n\t\"z\": 3\r\n}"
        );
    }

    #[test]
    fn lookup_uses_decoded_keys_and_strings() {
        let v = parse(r#"{"\u0068ooks": {"k": "a\"b"}}"#).unwrap();
        let k = v.get("hooks").unwrap().get("k").unwrap();
        assert_eq!(k.as_string().as_deref(), Some("a\"b"));
        assert!(v.get("missing").is_none());
    }

    #[test]
    fn rejects_non_strict_json() {
        for bad in [
            "",
            "{",
            "{\"a\":1,}",
            "[1,]",
            "{'a':1}",
            "{\"a\":1} x",
            "// c\n{}",
            "{\"a\":01}",
            "{\"a\":1.}",
            "{\"a\":+1}",
            "[\"\\q\"]",
            "[\"\\u12\"]",
            "[\"tab\there\"]",
            "{\"a\" 1}",
            "[nul]",
        ] {
            assert!(parse(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn nesting_is_bounded() {
        let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
        assert!(parse(&deep).is_err());
        let ok = format!("{}{}", "[".repeat(10), "]".repeat(10));
        assert!(parse(&ok).is_ok());
    }

    #[test]
    fn encode_decode_round_trip() {
        for s in [
            "plain",
            "q\"uote",
            "back\\slash",
            "nl\n\ttab\r",
            "ctl\u{1}x",
            "ü😀",
        ] {
            assert_eq!(decode(&encode(s)), s);
        }
        assert_eq!(decode(r"\ud83d\ude00"), "😀");
        assert_eq!(decode(r"\ud83d"), "\u{fffd}");
        assert_eq!(decode(r"\/"), "/");
    }
}
