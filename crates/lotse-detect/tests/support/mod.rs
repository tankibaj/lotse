//! Test support: a minimal terminal screen model, so PTY byte transcripts can be replayed into the
//! `Screen` observations the daemon's VT model (`alacritty_terminal`) produces in production.
//!
//! It models only what agent TUIs use to paint a screen: printable text, CR/LF/BS/TAB, cursor
//! movement and positioning, erase in line/display, insert/delete lines, save/restore cursor, and the
//! alternate screen (treated as a clear). Everything else (colours, modes, queries, OSC, DCS) is
//! parsed and ignored. Wide characters count as one column; the transcripts avoid them.

#![allow(dead_code)]

#[derive(Clone, Copy, PartialEq)]
enum Parse {
    Ground,
    Esc,
    /// `ESC (`, `ESC )`, `ESC #`: one more byte to swallow.
    Skip1,
    Csi,
    /// OSC / DCS / APC / PM / SOS: until BEL or `ESC \`.
    Str,
    StrEsc,
}

pub struct Screen {
    cols: usize,
    rows: usize,
    grid: Vec<Vec<char>>,
    r: usize,
    c: usize,
    saved: (usize, usize),
    parse: Parse,
    csi: String,
    /// The rows held back while a synchronized update is open.
    frozen: Option<Vec<String>>,
}

impl Screen {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            grid: vec![vec![' '; cols]; rows],
            r: 0,
            c: 0,
            saved: (0, 0),
            parse: Parse::Ground,
            csi: String::new(),
            frozen: None,
        }
    }

    /// Visible rows, top to bottom, trailing blanks trimmed (rows and the list), as `Observation::Screen`.
    /// Inside a synchronized update (`CSI ? 2026 h` ... `l`, which Codex wraps every frame in) the
    /// screen as it was when the update began, like a real terminal that holds back half-painted frames.
    pub fn rows(&self) -> Vec<String> {
        if let Some(frozen) = &self.frozen {
            return frozen.clone();
        }
        self.render()
    }

    fn render(&self) -> Vec<String> {
        let mut rows: Vec<String> = self
            .grid
            .iter()
            .map(|row| row.iter().collect::<String>().trim_end().to_owned())
            .collect();
        while rows.last().is_some_and(String::is_empty) {
            rows.pop();
        }
        rows
    }

    pub fn feed(&mut self, text: &str) {
        for ch in text.chars() {
            self.step(ch);
        }
    }

    fn step(&mut self, ch: char) {
        match self.parse {
            Parse::Ground => match ch {
                '\x1b' => self.parse = Parse::Esc,
                '\r' => self.c = 0,
                '\n' | '\x0b' | '\x0c' => self.line_feed(),
                '\x08' => self.c = self.c.saturating_sub(1),
                '\t' => self.c = ((self.c / 8 + 1) * 8).min(self.cols - 1),
                c if c < ' ' || c == '\x7f' => {}
                c => self.put(c),
            },
            Parse::Esc => {
                self.parse = Parse::Ground;
                match ch {
                    '[' => {
                        self.csi.clear();
                        self.parse = Parse::Csi;
                    }
                    ']' | 'P' | '_' | '^' | 'X' => self.parse = Parse::Str,
                    '(' | ')' | '#' | '*' | '+' => self.parse = Parse::Skip1,
                    '7' => self.saved = (self.r, self.c),
                    '8' => (self.r, self.c) = self.saved,
                    'M' => {
                        if self.r == 0 {
                            self.insert_lines(1);
                        } else {
                            self.r -= 1;
                        }
                    }
                    'c' => self.clear_all(),
                    _ => {}
                }
            }
            Parse::Skip1 => self.parse = Parse::Ground,
            Parse::Csi => {
                if ('\u{40}'..='\u{7e}').contains(&ch) {
                    self.parse = Parse::Ground;
                    let params = std::mem::take(&mut self.csi);
                    self.csi_dispatch(&params, ch);
                } else {
                    self.csi.push(ch);
                }
            }
            Parse::Str => match ch {
                '\x07' => self.parse = Parse::Ground,
                '\x1b' => self.parse = Parse::StrEsc,
                _ => {}
            },
            Parse::StrEsc => {
                self.parse = if ch == '\\' {
                    Parse::Ground
                } else {
                    Parse::Str
                };
            }
        }
    }

    fn put(&mut self, ch: char) {
        if self.c >= self.cols {
            self.c = 0;
            self.line_feed();
        }
        self.grid[self.r][self.c] = ch;
        self.c += 1;
    }

    fn line_feed(&mut self) {
        if self.r + 1 == self.rows {
            self.grid.remove(0);
            self.grid.push(vec![' '; self.cols]);
        } else {
            self.r += 1;
        }
    }

    fn clear_all(&mut self) {
        self.grid = vec![vec![' '; self.cols]; self.rows];
    }

    fn insert_lines(&mut self, n: usize) {
        for _ in 0..n.min(self.rows) {
            self.grid.insert(self.r, vec![' '; self.cols]);
            self.grid.truncate(self.rows);
        }
    }

    fn delete_lines(&mut self, n: usize) {
        for _ in 0..n.min(self.rows - self.r) {
            self.grid.remove(self.r);
            self.grid.push(vec![' '; self.cols]);
        }
    }

    fn csi_dispatch(&mut self, params: &str, fin: char) {
        // Private (`?`, `>`, `=`, `<`) and intermediate-byte sequences are modes, queries and
        // keyboard settings: nothing to draw, except the alternate screen, which starts blank.
        if params.starts_with(['?', '>', '=', '<']) {
            match (fin, params) {
                ('h', "?1049" | "?47" | "?1047") => {
                    self.clear_all();
                    (self.r, self.c) = (0, 0);
                }
                ('h', "?2026") => self.frozen = Some(self.render()),
                ('l', "?2026") => self.frozen = None,
                _ => {}
            }
            return;
        }
        if params.bytes().any(|b| (0x20..=0x2f).contains(&b)) {
            return;
        }
        let nums: Vec<usize> = params
            .split(';')
            .map(|p| p.parse::<usize>().unwrap_or(0))
            .collect();
        let n =
            |i: usize, default: usize| nums.get(i).copied().filter(|v| *v != 0).unwrap_or(default);
        match fin {
            'A' => self.r = self.r.saturating_sub(n(0, 1)),
            'B' | 'e' => self.r = (self.r + n(0, 1)).min(self.rows - 1),
            'C' | 'a' => self.c = (self.c + n(0, 1)).min(self.cols - 1),
            'D' => self.c = self.c.saturating_sub(n(0, 1)),
            'E' => (self.r, self.c) = ((self.r + n(0, 1)).min(self.rows - 1), 0),
            'F' => (self.r, self.c) = (self.r.saturating_sub(n(0, 1)), 0),
            'G' | '`' => self.c = (n(0, 1) - 1).min(self.cols - 1),
            'd' => self.r = (n(0, 1) - 1).min(self.rows - 1),
            'H' | 'f' => {
                self.r = (n(0, 1) - 1).min(self.rows - 1);
                self.c = (n(1, 1) - 1).min(self.cols - 1);
            }
            'J' => match nums[0] {
                0 => {
                    self.erase_line(self.c, self.cols);
                    for row in self.r + 1..self.rows {
                        self.grid[row] = vec![' '; self.cols];
                    }
                }
                1 => {
                    for row in 0..self.r {
                        self.grid[row] = vec![' '; self.cols];
                    }
                    self.erase_line(0, self.c + 1);
                }
                _ => self.clear_all(),
            },
            'K' => match nums[0] {
                0 => self.erase_line(self.c, self.cols),
                1 => self.erase_line(0, self.c + 1),
                _ => self.erase_line(0, self.cols),
            },
            'X' => self.erase_line(self.c, self.c + n(0, 1)),
            'L' => self.insert_lines(n(0, 1)),
            'M' => self.delete_lines(n(0, 1)),
            _ => {}
        }
    }

    fn erase_line(&mut self, from: usize, to: usize) {
        for cell in &mut self.grid[self.r][from.min(self.cols)..to.min(self.cols)] {
            *cell = ' ';
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paints_positions_erases_and_scrolls() {
        let mut s = Screen::new(10, 3);
        s.feed("\x1b[2;3Hab\x1b[1;1H\x1b[31mhi\x1b[0m\x1b]0;title\x07\x1b[3;1Hz");
        assert_eq!(s.rows(), ["hi", "  ab", "z"]);
        s.feed("\x1b[2;1H\x1b[K\x1b[3;1H\x1b[J");
        assert_eq!(s.rows(), ["hi"]);
        s.feed("\x1b[3;1H\r\nnew");
        assert_eq!(s.rows(), ["", "", "new"]);
        s.feed("\x1b[?1049h\x1b[2;2Hx");
        assert_eq!(s.rows(), ["", " x"], "alternate screen starts blank");
    }
}
