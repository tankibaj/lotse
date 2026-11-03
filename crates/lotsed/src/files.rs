//! Beta (wp05): files of a session folder, confined to it: `list_files`, `read_file`, `write_file`.
//!
//! The root is the session's folder. A client path must be relative with no `..`, no NUL and no
//! `.git` part, and once symlinks are resolved it must still be inside the (resolved) root:
//! a link out of the folder is refused for read and write alike. A revision given to `read_file`
//! must not look like a git option. Files are plain regular files of at most 2 MiB; a binary one
//! (a NUL byte, or not UTF-8) is read-only. A write goes to a temp file in the same folder and is
//! renamed over the target, so a reader never sees half a file.
//!
//! Limit: the checks run before the file is opened, not atomically with it. Someone who can
//! already change the folder concurrently (the user's own agent) could swap a folder for a link
//! in between; the daemon runs with that user's rights anyway.

use crate::state::{State, unavailable};
use lotse_git::GitError;
use lotse_proto::{ErrorCode, File, ProtoError, Request, Response, SessionId};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Largest file read or written as text.
const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;

pub async fn handle(state: &Arc<State>, req: Request) -> Response {
    match req {
        Request::ListFiles {
            session_id,
            changed_only,
        } => in_session(state, &session_id, move |root| list(root, changed_only)).await,
        Request::ReadFile {
            session_id,
            path,
            rev,
        } => {
            in_session(state, &session_id, move |root| {
                read(root, &path, rev.as_deref())
            })
            .await
        }
        Request::WriteFile {
            session_id,
            path,
            text,
            expected_sha,
        } => {
            in_session(state, &session_id, move |root| {
                write(root, &path, &text, expected_sha.as_deref())
            })
            .await
        }
        other => unavailable(other.method()),
    }
}

/// Runs `job` (it touches the disk and runs `git`) off the async runtime on the session's folder.
async fn in_session(
    state: &Arc<State>,
    id: &SessionId,
    job: impl FnOnce(&Path) -> Result<Response, Failure> + Send + 'static,
) -> Response {
    let Some(session) = state.session(id) else {
        return not_found("session").into();
    };
    let root = session.cwd();
    match tokio::task::spawn_blocking(move || job(&root)).await {
        Ok(Ok(response)) => response,
        Ok(Err(failure)) => failure.into(),
        Err(e) => {
            tracing::error!("files: {e}");
            Failure::new(ErrorCode::Internal, "internal error").into()
        }
    }
}

/// An error answer in the making.
#[derive(Debug)]
struct Failure {
    code: ErrorCode,
    message: String,
}

impl Failure {
    fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<Failure> for Response {
    fn from(f: Failure) -> Self {
        Response::Error(ProtoError {
            code: f.code,
            message: f.message,
        })
    }
}

fn invalid(message: &str) -> Failure {
    Failure::new(ErrorCode::InvalidRequest, message)
}

fn not_found(what: &str) -> Failure {
    Failure::new(ErrorCode::NotFound, format!("{what} not found"))
}

fn io_failure(e: &std::io::Error) -> Failure {
    match e.kind() {
        ErrorKind::NotFound | ErrorKind::NotADirectory => not_found("file"),
        _ => Failure::new(
            ErrorCode::Unavailable,
            format!("cannot access the file: {e}"),
        ),
    }
}

fn git_failure(e: &GitError) -> Failure {
    match e {
        GitError::NotADirectory(_) => not_found("folder"),
        _ => Failure::new(ErrorCode::Unavailable, e.to_string()),
    }
}

fn list(root: &Path, changed_only: bool) -> Result<Response, Failure> {
    lotse_git::list_files(root, changed_only)
        .map(Response::FileTree)
        .map_err(|e| git_failure(&e))
}

fn is_dot_git(part: &std::ffi::OsStr) -> bool {
    // macOS volumes are case-insensitive: `.GIT` is `.git`
    part.to_str()
        .is_some_and(|s| s.eq_ignore_ascii_case(".git"))
}

/// `path` as a clean relative path: normal parts only (`.` is dropped), none of them `.git`
/// (hooks and config in there run commands).
fn clean(path: &str) -> Result<PathBuf, Failure> {
    if path.contains('\0') {
        return Err(invalid("the path has a NUL byte"));
    }
    let mut clean = PathBuf::new();
    for part in Path::new(path).components() {
        match part {
            Component::Normal(name) if is_dot_git(name) => {
                return Err(invalid("paths inside .git are not available"));
            }
            Component::Normal(name) => clean.push(name),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(invalid(
                    "the path must be relative and stay inside the folder",
                ));
            }
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(invalid("the path is empty"));
    }
    Ok(clean)
}

/// The real path of `rel` under `root`. Refused when a link leads out of the folder (or into
/// `.git`); `not_found` when it does not exist.
fn resolve(root: &Path, rel: &Path) -> Result<PathBuf, Failure> {
    let root = root.canonicalize().map_err(|_| not_found("folder"))?;
    let target = root.join(rel).canonicalize().map_err(|e| io_failure(&e))?;
    let inside = target
        .strip_prefix(&root)
        .map_err(|_| invalid("the path leaves the session folder"))?;
    if inside
        .components()
        .any(|c| matches!(c, Component::Normal(n) if is_dot_git(n)))
    {
        return Err(invalid("paths inside .git are not available"));
    }
    Ok(target)
}

/// What was read of a file.
struct Loaded {
    /// At most `MAX_FILE_BYTES`.
    bytes: Vec<u8>,
    size: u64,
    /// More than `MAX_FILE_BYTES`: `bytes` is the start.
    cut: bool,
}

/// Opens `path` without blocking (a FIFO is no file) and without following a link, and reads it.
fn load(path: &Path) -> Result<Loaded, Failure> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NONBLOCK | nix::libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| io_failure(&e))?;
    let meta = file.metadata().map_err(|e| io_failure(&e))?;
    if !meta.is_file() {
        return Err(invalid("not a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io_failure(&e))?;
    let cut = bytes.len() > MAX_FILE_BYTES;
    bytes.truncate(MAX_FILE_BYTES);
    Ok(Loaded {
        size: meta.len().max(bytes.len() as u64),
        bytes,
        cut,
    })
}

fn sha_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// The answer for what was read. Text only when it is UTF-8 without a NUL byte and within the
/// size limit; anything else is `binary` (read-only, no text). A file over the limit has no
/// `sha` (it is never read whole).
fn file_of(path: String, loaded: Loaded) -> File {
    let Loaded { bytes, size, cut } = loaded;
    if cut {
        return File {
            path,
            text: String::new(),
            sha: String::new(),
            binary: true,
            size,
        };
    }
    let sha = sha_hex(&bytes);
    match String::from_utf8(bytes) {
        Ok(text) if !text.contains('\0') => File {
            path,
            text,
            sha,
            binary: false,
            size,
        },
        _ => File {
            path,
            text: String::new(),
            sha,
            binary: true,
            size,
        },
    }
}

fn read(root: &Path, path: &str, rev: Option<&str>) -> Result<Response, Failure> {
    let rel = clean(path)?;
    let shown = rel.to_string_lossy().into_owned();
    let loaded = match rev {
        None => load(&resolve(root, &rel)?)?,
        Some(rev) => {
            if !lotse_git::valid_rev(rev) {
                return Err(invalid("not a git revision"));
            }
            if !crate::git::is_git_repo(root) {
                return Err(invalid("the folder is not a git repository"));
            }
            match lotse_git::read_blob(root, rev, &shown, MAX_FILE_BYTES) {
                Ok(blob) => Loaded {
                    bytes: blob.bytes,
                    size: blob.size,
                    cut: blob.cut,
                },
                // git failed: no such revision, or no such file at it
                Err(GitError::Command(_)) => return Err(not_found("file at that revision")),
                Err(e) => return Err(git_failure(&e)),
            }
        }
    };
    Ok(Response::File(file_of(shown, loaded)))
}

fn write(
    root: &Path,
    path: &str,
    text: &str,
    expected_sha: Option<&str>,
) -> Result<Response, Failure> {
    let rel = clean(path)?;
    if text.len() > MAX_FILE_BYTES {
        return Err(invalid("the text is over 2 MiB"));
    }
    if text.contains('\0') {
        return Err(invalid("the text has a NUL byte"));
    }
    let shown = rel.to_string_lossy().into_owned();
    let target = resolve(root, &rel)?;
    let current = file_of(shown.clone(), load(&target)?);
    if current.binary {
        return Err(invalid("a binary or over-2-MiB file is read-only"));
    }
    if let Some(want) = expected_sha
        && !want.eq_ignore_ascii_case(&current.sha)
    {
        return Err(Failure::new(
            ErrorCode::Conflict,
            "the file changed since it was read",
        ));
    }
    replace(&target, text.as_bytes())?;
    Ok(Response::File(File {
        path: shown,
        sha: sha_hex(text.as_bytes()),
        text: text.to_owned(),
        binary: false,
        size: text.len() as u64,
    }))
}

/// Puts `bytes` in `target` (an existing regular file, already resolved): written to a new temp
/// file next to it with the same permissions, then renamed over it.
fn replace(target: &Path, bytes: &[u8]) -> Result<(), Failure> {
    static TEMPS: AtomicU64 = AtomicU64::new(0);
    let (Some(dir), Ok(meta)) = (target.parent(), fs::metadata(target)) else {
        return Err(not_found("file"));
    };
    let temp = dir.join(format!(
        ".lotse-{}-{}.tmp",
        std::process::id(),
        TEMPS.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.set_permissions(meta.permissions())?;
        file.sync_all()?;
        fs::rename(&temp, target)
    })();
    written.map_err(|e| {
        let _ = fs::remove_file(&temp);
        io_failure(&e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(f: Failure) -> ErrorCode {
        f.code
    }

    #[test]
    fn clean_keeps_normal_parts_and_drops_dots() {
        assert_eq!(clean("a/b.txt").unwrap(), Path::new("a/b.txt"));
        assert_eq!(clean("./a//b/./c/").unwrap(), Path::new("a/b/c"));
        assert_eq!(clean("a.git/x").unwrap(), Path::new("a.git/x"));
        assert_eq!(
            clean(".github/ci.yml").unwrap(),
            Path::new(".github/ci.yml")
        );
    }

    #[test]
    fn clean_refuses_escapes_nul_empty_and_dot_git_in_any_case() {
        for bad in [
            "..",
            "../a",
            "a/..",
            "a/../b",
            "/a",
            "/",
            "",
            ".",
            "./",
            "a\0b",
            ".git",
            ".git/config",
            ".GIT/config",
            "a/.Git/x",
            "a/.git",
        ] {
            let e = clean(bad).expect_err(bad);
            assert_eq!(code(e), ErrorCode::InvalidRequest, "{bad:?}");
        }
    }

    #[test]
    fn sha_is_lowercase_hex_sha256() {
        assert_eq!(
            sha_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha_hex(b"hello\n"),
            "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
        );
    }

    fn loaded(bytes: &[u8]) -> Loaded {
        Loaded {
            bytes: bytes.to_vec(),
            size: bytes.len() as u64,
            cut: false,
        }
    }

    #[test]
    fn text_is_utf8_without_nul_and_anything_else_is_binary() {
        let f = file_of("a".into(), loaded("héllo\n".as_bytes()));
        assert!(!f.binary && f.text == "héllo\n");
        for bytes in [&b"a\0b"[..], &[0xff, 0xfe], &[b'c', 0xe9]] {
            let f = file_of("a".into(), loaded(bytes));
            assert!(f.binary && f.text.is_empty(), "{bytes:?}");
            assert_eq!(f.sha, sha_hex(bytes));
            assert_eq!(f.size, bytes.len() as u64);
        }
    }

    #[test]
    fn a_cut_file_has_no_text_and_no_sha() {
        let f = file_of(
            "a".into(),
            Loaded {
                bytes: vec![b'a'; 4],
                size: 99,
                cut: true,
            },
        );
        assert!(f.binary && f.text.is_empty() && f.sha.is_empty());
        assert_eq!(f.size, 99);
    }

    #[test]
    fn resolve_refuses_a_link_out_and_accepts_one_that_stays() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        let outside = tmp.path().join("outside");
        fs::create_dir_all(root.join("in")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("s"), "s").unwrap();
        fs::write(root.join("in/f"), "f").unwrap();
        symlink(outside.join("s"), root.join("out")).unwrap();
        symlink(&outside, root.join("outdir")).unwrap();
        symlink("in", root.join("alias")).unwrap();
        symlink("missing", root.join("dangling")).unwrap();

        let err = |p: &str| code(resolve(&root, Path::new(p)).expect_err(p));
        assert_eq!(err("out"), ErrorCode::InvalidRequest);
        assert_eq!(err("outdir/s"), ErrorCode::InvalidRequest);
        assert_eq!(err("nope"), ErrorCode::NotFound);
        assert_eq!(err("dangling"), ErrorCode::NotFound);
        assert_eq!(err("in/f/x"), ErrorCode::NotFound);
        let ok = resolve(&root, Path::new("alias/f")).unwrap();
        assert_eq!(ok, root.canonicalize().unwrap().join("in/f"));
    }

    #[test]
    fn a_failed_replace_leaves_no_temp_file() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("t.txt");
        fs::write(&target, "old").unwrap();
        replace(&target, b"new").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
        let missing = tmp.path().join("gone.txt");
        assert!(replace(&missing, b"x").is_err());
        let names: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from("t.txt")]);
    }
}
