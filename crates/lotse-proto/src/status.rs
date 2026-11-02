//! Status enum and the roll-up used for collapsed projects.

use serde::{Deserialize, Serialize};

/// Per session and per sub-agent status. Names match `spec/mock.html` (`G` glyph map).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// In progress (spinner).
    Run,
    /// Needs permission.
    Perm,
    /// Failed.
    Fail,
    /// Done.
    Done,
    /// Idle (also the state of a plain shell).
    Idle,
    /// Detection has no signal yet.
    Unknown,
}

impl Status {
    pub const ALL: [Status; 6] = [
        Status::Run,
        Status::Perm,
        Status::Fail,
        Status::Done,
        Status::Idle,
        Status::Unknown,
    ];

    /// Roll-up rank: perm > fail > run > done > idle > unknown.
    pub fn rank(self) -> u8 {
        match self {
            Status::Perm => 5,
            Status::Fail => 4,
            Status::Run => 3,
            Status::Done => 2,
            Status::Idle => 1,
            Status::Unknown => 0,
        }
    }

    /// "Need you" filter and badge: perm or fail.
    pub fn needs_you(self) -> bool {
        matches!(self, Status::Perm | Status::Fail)
    }
}

/// Roll-up for a collapsed project (or a session plus its sub-agents).
///
/// Order perm > fail > run > done > idle. An empty input is `Idle` (a project with no
/// sessions); an input of only `Unknown` stays `Unknown`.
pub fn rollup(statuses: impl IntoIterator<Item = Status>) -> Status {
    statuses
        .into_iter()
        .reduce(|a, b| if b.rank() > a.rank() { b } else { a })
        .unwrap_or(Status::Idle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_is_perm_fail_run_done_idle() {
        use Status::*;
        assert_eq!(rollup([Idle, Done, Run, Fail, Perm]), Perm);
        assert_eq!(rollup([Idle, Done, Run, Fail]), Fail);
        assert_eq!(rollup([Idle, Done, Run]), Run);
        assert_eq!(rollup([Idle, Done]), Done);
        assert_eq!(rollup([Idle, Idle]), Idle);
    }

    #[test]
    fn empty_is_idle_and_unknown_is_lowest() {
        assert_eq!(rollup([]), Status::Idle);
        assert_eq!(rollup([Status::Unknown]), Status::Unknown);
        assert_eq!(rollup([Status::Unknown, Status::Idle]), Status::Idle);
    }

    #[test]
    fn needs_you_is_perm_or_fail() {
        let need: Vec<_> = Status::ALL.into_iter().filter(|s| s.needs_you()).collect();
        assert_eq!(need, [Status::Perm, Status::Fail]);
    }

    #[test]
    fn serde_names_match_the_mock() {
        let names: Vec<_> = Status::ALL
            .iter()
            .map(|s| serde_json::to_string(s).unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "\"run\"",
                "\"perm\"",
                "\"fail\"",
                "\"done\"",
                "\"idle\"",
                "\"unknown\""
            ]
        );
    }
}
