//! Paths and switches of one daemon instance.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// A client has this long to send `hello` before the connection is dropped.
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
/// `sockaddr_un.sun_path` is 104 bytes on macOS (including the NUL).
const MAX_SOCKET_PATH: usize = 100;

#[derive(Debug, Clone)]
pub struct Config {
    /// `~/Library/Application Support/Lotse` unless overridden (`--data-dir`, `LOTSE_DATA_DIR`).
    pub data_dir: PathBuf,
    /// Ask `$SHELL -l -i` for the user's real environment (PATH from `.zshrc`, nvm, brew, …).
    /// Tests switch it off so they never source the real dotfiles.
    pub login_env: bool,
}

impl Config {
    pub fn new(data_dir: PathBuf, login_env: bool) -> Self {
        Self {
            data_dir,
            login_env,
        }
    }

    /// `…/Application Support/Lotse` on macOS.
    pub fn default_data_dir() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join("Lotse"))
    }

    pub fn socket_path(&self) -> PathBuf {
        self.data_dir.join("lotsed.sock")
    }

    pub fn token_path(&self) -> PathBuf {
        self.data_dir.join("token")
    }

    pub fn pid_path(&self) -> PathBuf {
        self.data_dir.join("lotsed.pid")
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("lotse.db")
    }

    pub fn log_path(&self) -> PathBuf {
        self.data_dir.join("lotsed.log")
    }

    /// Fails early with a clear message instead of a truncated-path `bind` error.
    pub fn check_socket_path(&self) -> Result<(), String> {
        let len = self.socket_path().as_os_str().len();
        if len > MAX_SOCKET_PATH {
            return Err(format!(
                "socket path is {len} bytes, the limit is {MAX_SOCKET_PATH}: use a shorter --data-dir"
            ));
        }
        Ok(())
    }
}

impl AsRef<Path> for Config {
    fn as_ref(&self) -> &Path {
        &self.data_dir
    }
}
