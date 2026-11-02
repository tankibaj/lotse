//! Opaque string ids. The daemon mints them; clients treat them as opaque.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(id: impl Into<String>) -> Self {
                Self(id.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_owned())
            }
        }
    };
}

id_type!(
    /// A workspace groups projects. Alpha UI shows one default workspace.
    WorkspaceId
);
id_type!(
    /// A project is a folder (usually a git repo) with colour and icon.
    ProjectId
);
id_type!(
    /// A session is one PTY running a shell or an agent CLI.
    SessionId
);
id_type!(
    /// A sub-agent (Claude Code Task) inside a session.
    SubAgentId
);
