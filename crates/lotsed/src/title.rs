//! Session title from the first prompt: the first line the user submits, or `first_prompt` of a new
//! session. Editable afterwards (`rename_session`), which switches the automatic title off.

/// Title shown until a prompt is known (research: never repeat the agent name as the title).
pub const PLACEHOLDER_TITLE: &str = "New session";
const MAX_TITLE_CHARS: usize = 80;
const MAX_LINE_BYTES: usize = 4096;

/// One-line title: whitespace collapsed, control characters dropped, at most 80 characters.
pub fn derive_title(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let mut out = String::new();
    let mut last_space = false;
    for c in line.chars() {
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
            }
            last_space = true;
        } else if !c.is_control() {
            out.push(c);
            last_space = false;
        }
    }
    let out = out.trim().to_owned();
    if out.is_empty() {
        return None;
    }
    if out.chars().count() > MAX_TITLE_CHARS {
        let cut: String = out.chars().take(MAX_TITLE_CHARS - 1).collect();
        return Some(format!("{}…", cut.trim_end()));
    }
    Some(out)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EscState {
    None,
    /// after ESC
    Start,
    /// ESC [ … final byte 0x40..=0x7E
    Csi,
    /// ESC ] … BEL or ESC \
    Osc,
    OscEsc,
    /// ESC O x, ESC ( x: one more byte
    One,
}

/// Follows what the user types (raw keystrokes of the input frames) and reports the first
/// submitted line. Understands backspace, Ctrl-C / Ctrl-U, escape sequences (arrows, bracketed paste).
pub struct LineTracker {
    buf: Vec<u8>,
    esc: EscState,
}

impl Default for LineTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl LineTracker {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            esc: EscState::None,
        }
    }

    /// Feed raw input; returns the title for the first non-empty line submitted in this chunk.
    pub fn feed(&mut self, data: &[u8]) -> Option<String> {
        for &b in data {
            match self.esc {
                EscState::Start => {
                    self.esc = match b {
                        b'[' => EscState::Csi,
                        b']' => EscState::Osc,
                        b'O' | b'(' | b')' => EscState::One,
                        _ => EscState::None,
                    };
                    continue;
                }
                EscState::Csi => {
                    if (0x40..=0x7e).contains(&b) {
                        self.esc = EscState::None;
                    }
                    continue;
                }
                EscState::Osc => {
                    self.esc = match b {
                        0x07 => EscState::None,
                        0x1b => EscState::OscEsc,
                        _ => EscState::Osc,
                    };
                    continue;
                }
                EscState::OscEsc => {
                    self.esc = if b == b'\\' {
                        EscState::None
                    } else {
                        EscState::Osc
                    };
                    continue;
                }
                EscState::One => {
                    self.esc = EscState::None;
                    continue;
                }
                EscState::None => {}
            }
            match b {
                0x1b => self.esc = EscState::Start,
                b'\r' | b'\n' => {
                    let line = String::from_utf8_lossy(&std::mem::take(&mut self.buf)).into_owned();
                    if let Some(t) = derive_title(&line) {
                        return Some(t);
                    }
                }
                0x7f | 0x08 => {
                    while matches!(self.buf.last(), Some(c) if c & 0xC0 == 0x80) {
                        self.buf.pop();
                    }
                    self.buf.pop();
                }
                0x03 | 0x15 => self.buf.clear(),
                b'\t' => self.buf.push(b' '),
                c if c < 0x20 => {}
                c => {
                    if self.buf.len() < MAX_LINE_BYTES {
                        self.buf.push(c);
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_collapses_and_truncates() {
        assert_eq!(
            derive_title("  fix   the\tbug \n second"),
            Some("fix the bug".into())
        );
        assert_eq!(derive_title("\n\n  \n"), None);
        assert_eq!(derive_title("a\u{7}b"), Some("ab".into()));
        let long = "x".repeat(200);
        let t = derive_title(&long).unwrap();
        assert_eq!(t.chars().count(), 80);
        assert!(t.ends_with('…'));
    }

    #[test]
    fn tracker_reports_first_submitted_line() {
        let mut t = LineTracker::new();
        assert_eq!(t.feed(b"fix the"), None);
        assert_eq!(t.feed(b" bug\r"), Some("fix the bug".into()));
    }

    #[test]
    fn tracker_handles_editing_keys_and_escapes() {
        let mut t = LineTracker::new();
        // "helo" + left arrow + "l" would need cursor tracking; we only track the tail: arrows are skipped
        assert_eq!(t.feed(b"helx\x7flo\x1b[D\x1b[A\r"), Some("hello".into()));
        let mut t = LineTracker::new();
        assert_eq!(t.feed(b"oops\x03real thing\n"), Some("real thing".into()));
        let mut t = LineTracker::new();
        assert_eq!(
            t.feed(b"\x1b[200~pasted text\x1b[201~\r"),
            Some("pasted text".into())
        );
    }

    #[test]
    fn tracker_skips_empty_lines_and_handles_utf8_backspace() {
        let mut t = LineTracker::new();
        assert_eq!(t.feed(b"\r\r  \r"), None);
        assert_eq!(t.feed("naïve\u{7f}x\r".as_bytes()), Some("naïvx".into()));
    }
}
