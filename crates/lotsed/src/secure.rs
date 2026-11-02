//! Private files and directories: the data dir is 0700, the socket, token, pidfile and database 0600.

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

/// Create `dir` (0700) if missing; if it exists it must belong to us, and is tightened to 0700.
pub fn ensure_private_dir(dir: &Path) -> io::Result<()> {
    DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    let meta = fs::metadata(dir)?;
    if !meta.is_dir() {
        return Err(io::Error::other(format!(
            "{} is not a directory",
            dir.display()
        )));
    }
    if meta.uid() != nix::unistd::getuid().as_raw() {
        return Err(io::Error::other(format!(
            "{} is not owned by the current user",
            dir.display()
        )));
    }
    if meta.permissions().mode() & 0o777 != 0o700 {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

/// Create (or open, without truncating) a file with mode 0600.
pub fn open_private(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?;
    if file.metadata()?.permissions().mode() & 0o777 != 0o600 {
        set_mode(path, 0o600)?;
    }
    Ok(file)
}

/// Content of the token file, created with 256 random bits (two UUIDv4s from the OS RNG) when
/// missing or unusable. The value is never logged.
pub fn load_or_create_token(path: &Path) -> io::Result<String> {
    if let Ok(mut f) = File::open(path) {
        let mut s = String::new();
        f.read_to_string(&mut s)?;
        let s = s.trim().to_owned();
        if s.len() >= 32 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
            if fs::metadata(path)?.permissions().mode() & 0o777 != 0o600 {
                set_mode(path, 0o600)?;
            }
            return Ok(s);
        }
    }
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let tmp = path.with_extension("tmp");
    let _ = fs::remove_file(&tmp);
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(token.as_bytes())?;
    f.sync_all()?;
    fs::rename(&tmp, path)?;
    Ok(token)
}

/// Compare without leaking the matching prefix length through timing.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= usize::from(a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0));
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(p: &Path) -> u32 {
        fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn private_dir_is_created_and_tightened() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path().join("a/b");
        ensure_private_dir(&d).unwrap();
        assert_eq!(mode(&d), 0o700);
        fs::set_permissions(&d, fs::Permissions::from_mode(0o755)).unwrap();
        ensure_private_dir(&d).unwrap();
        assert_eq!(mode(&d), 0o700);
    }

    #[test]
    fn token_is_stable_private_and_long() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("token");
        let t1 = load_or_create_token(&p).unwrap();
        assert_eq!(t1.len(), 64);
        assert_eq!(mode(&p), 0o600);
        assert_eq!(load_or_create_token(&p).unwrap(), t1);
        fs::write(&p, "short").unwrap();
        assert_ne!(load_or_create_token(&p).unwrap(), "short");
    }

    #[test]
    fn ct_eq_matches_only_equal() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"abcd"));
        assert!(!ct_eq(b"", b"a"));
        assert!(ct_eq(b"", b""));
    }
}
