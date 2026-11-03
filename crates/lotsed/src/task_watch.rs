//! Background sub-agents report back as a user turn made of `<task-notification>` blocks. Claude
//! Code may not run the `UserPromptSubmit` hook for that turn (the live walk of v2.1.311 showed none
//! of the report-backs as a hook), but the turn is always in the transcript. [`TaskWatch`] tails the
//! transcript for them while a session has running sub-agents; the session feeds each one to the
//! hook adapter as a `TaskNotification` event (`lotse_detect::claude::task_notification_event`).
//!
//! Only appended bytes are read, and only lines that hold the tag are parsed. The first scan of a
//! file starts [`FIRST_LOOKBACK`] before its end: a report cannot precede the launch the daemon saw.

use crate::transcript::open_nofollow;
use lotse_detect::claude::{TaskNotification, parse_task_notifications};
use serde_json::Value;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How often a transcript is looked at while sub-agents run.
pub const SCAN_EVERY: Duration = Duration::from_secs(2);
/// Where the first scan of a file starts, back from its end.
const FIRST_LOOKBACK: u64 = 1024 * 1024;
/// Most bytes one scan reads (a long gap is caught up over several scans).
const MAX_READ: u64 = 8 * 1024 * 1024;
const MARKER: &[u8] = b"<task-notification>";

#[derive(Debug, Default)]
pub struct TaskWatch {
    file: Option<PathBuf>,
    /// Up to and including the last complete line read.
    offset: u64,
    last: Option<Instant>,
}

impl TaskWatch {
    /// Forget the file: the next scan starts from the end again (no sub-agent runs).
    pub fn idle(&mut self) {
        *self = Self::default();
    }

    /// `true` when a scan is due (at most one per [`SCAN_EVERY`]); marks it as taken.
    pub fn due(&mut self, now: Instant) -> bool {
        if self
            .last
            .is_some_and(|t| now.saturating_duration_since(t) < SCAN_EVERY)
        {
            return false;
        }
        self.last = Some(now);
        true
    }

    /// The notifications appended to `path` since the last scan of it.
    pub fn scan(&mut self, path: &Path) -> Vec<TaskNotification> {
        let Ok(file) = open_nofollow(path) else {
            return Vec::new();
        };
        let Ok(len) = file.metadata().map(|m| m.len()) else {
            return Vec::new();
        };
        if self.file.as_deref() != Some(path) {
            self.file = Some(path.to_owned());
            self.offset = len.saturating_sub(FIRST_LOOKBACK);
        } else if len < self.offset {
            self.offset = 0; // rewritten
        }
        let mut notes = Vec::new();
        let mut reader = BufReader::new(file);
        if reader.seek(SeekFrom::Start(self.offset)).is_err() {
            return notes;
        }
        let end = self.offset.saturating_add(MAX_READ);
        let mut line = Vec::new();
        while self.offset < end {
            line.clear();
            let Ok(n) = reader.read_until(b'\n', &mut line) else {
                break;
            };
            if n == 0 || line.last() != Some(&b'\n') {
                break; // end of file, or a line still being written
            }
            self.offset += n as u64;
            if line.windows(MARKER.len()).any(|w| w == MARKER) {
                collect(&line, &mut notes);
            }
        }
        notes
    }
}

/// The notifications in the strings of one transcript line.
fn collect(line: &[u8], out: &mut Vec<TaskNotification>) {
    let Ok(v) = serde_json::from_slice::<Value>(line) else {
        return;
    };
    fn walk(v: &Value, depth: usize, out: &mut Vec<TaskNotification>) {
        if depth > 8 {
            return;
        }
        match v {
            Value::String(s) if s.contains("<task-notification>") => {
                out.extend(parse_task_notifications(s));
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, depth + 1, out)),
            Value::Object(o) => o.values().for_each(|x| walk(x, depth + 1, out)),
            _ => {}
        }
    }
    walk(&v, 0, out);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn user_turn(text: &str) -> String {
        format!(
            "{}\n",
            serde_json::json!({"type": "user", "message": {"role": "user", "content": text}})
        )
    }

    fn note(tool_use_id: &str, status: &str) -> String {
        format!(
            "<task-notification>\n<task-id>t-{tool_use_id}</task-id>\n<tool-use-id>{tool_use_id}</tool-use-id>\n<status>{status}</status>\n<summary>Agent \"x\" finished</summary>\n<result>done</result>\n</task-notification>"
        )
    }

    fn append(path: &Path, text: &str) {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        f.write_all(text.as_bytes()).unwrap();
    }

    #[test]
    fn notifications_appended_after_the_first_scan_are_read_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(dir.path()).unwrap().join("s.jsonl");
        append(&path, &user_turn("hello"));
        append(&path, &user_turn(&note("old", "completed")));
        let mut w = TaskWatch::default();
        // the first scan starts a megabyte back: a short file is read whole
        let first = w.scan(&path);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].tool_use_id.as_deref(), Some("old"));
        assert!(w.scan(&path).is_empty(), "nothing new");

        append(&path, &user_turn(&note("a", "completed")));
        append(&path, "{\"type\":\"assistant\",\"message\":{}}\n");
        append(
            &path,
            &user_turn(&format!("{}\n{}", note("b", "failed"), note("c", "killed"))),
        );
        let got = w.scan(&path);
        let ids: Vec<_> = got
            .iter()
            .map(|n| n.tool_use_id.as_deref().unwrap())
            .collect();
        assert_eq!(ids, ["a", "b", "c"]);
        assert_eq!(got[1].status, "failed");
        assert!(w.scan(&path).is_empty());
    }

    #[test]
    fn a_half_written_line_waits_and_a_content_array_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(dir.path()).unwrap().join("s.jsonl");
        append(&path, "{}\n");
        let mut w = TaskWatch::default();
        w.scan(&path);
        let line = serde_json::json!({"type": "user", "message": {"content": [{"type": "text", "text": note("z", "completed")}]}}).to_string();
        let (head, tail) = line.split_at(line.len() / 2);
        append(&path, head);
        assert!(w.scan(&path).is_empty(), "the line is not complete");
        append(&path, &format!("{tail}\n"));
        assert_eq!(w.scan(&path).len(), 1);
    }

    #[test]
    fn the_first_scan_of_a_big_file_skips_its_old_part_and_a_link_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(dir.path()).unwrap();
        let path = base.join("big.jsonl");
        append(&path, &user_turn(&note("ancient", "completed")));
        append(&path, &user_turn(&"x".repeat(2 * 1024 * 1024)));
        append(&path, &user_turn(&note("recent", "completed")));
        let mut w = TaskWatch::default();
        let got = w.scan(&path);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].tool_use_id.as_deref(), Some("recent"));

        let link = base.join("link.jsonl");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(TaskWatch::default().scan(&link).is_empty());
        assert!(
            TaskWatch::default()
                .scan(&base.join("missing.jsonl"))
                .is_empty()
        );
    }

    #[test]
    fn scans_are_throttled() {
        let mut w = TaskWatch::default();
        let t = Instant::now();
        assert!(w.due(t));
        assert!(!w.due(t + Duration::from_millis(500)));
        assert!(w.due(t + SCAN_EVERY));
    }
}
