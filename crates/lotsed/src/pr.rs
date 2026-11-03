//! Beta (wp04): PR status of a session's branch through `gh` / `glab` (opt-in, cached).
//!
//! Only runs when `Settings::external.pr_status` is on (the callers decide). No shell: argv only,
//! the branch after `--` so it can never read as an option. One call at a time, each bounded by
//! [`TIMEOUT`]; the answer, a miss included, is kept for [`TTL`].

use crate::launch::LaunchEnv;
use lotse_proto::{PrChecks, PrInfo, PrProvider, PrState};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex, mpsc};
use std::time::{Duration, Instant};

/// How long one lookup (host check and `pr view` together) may take.
const TIMEOUT: Duration = Duration::from_secs(5);
/// How long an answer, or the lack of one, is reused.
const TTL: Duration = Duration::from_secs(60);
/// Answers kept; older ones are dropped first.
const MAX_ENTRIES: usize = 256;
const MAX_OUTPUT: u64 = 1 << 20;

type Key = (PathBuf, String);
type Cache = HashMap<Key, (Instant, Option<PrInfo>)>;

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(Mutex::default);
/// Held while `gh` / `glab` runs: one call in flight, the others wait and then find the answer.
static IN_FLIGHT: Mutex<()> = Mutex::new(());

/// The pull or merge request of `branch` in `dir`'s repository; `None` when there is none, no
/// `origin`, the CLI is missing or logged out, or it did not answer in time. Blocks up to
/// [`TIMEOUT`]: call it off the async runtime.
pub fn status(dir: &Path, branch: &str, env: &LaunchEnv) -> Option<PrInfo> {
    if branch.is_empty() {
        return None;
    }
    let key = (dir.to_path_buf(), branch.to_owned());
    if let Some(hit) = cached(&key) {
        return hit;
    }
    let _flight = IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(hit) = cached(&key) {
        return hit;
    }
    let found = lookup(dir, branch, env);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if cache.len() >= MAX_ENTRIES {
        let oldest = cache
            .iter()
            .min_by_key(|(_, (at, _))| *at)
            .map(|(k, _)| k.clone());
        if let Some(k) = oldest {
            cache.remove(&k);
        }
    }
    cache.insert(key, (Instant::now(), found.clone()));
    found
}

fn cached(key: &Key) -> Option<Option<PrInfo>> {
    let cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache
        .get(key)
        .filter(|(at, _)| at.elapsed() < TTL)
        .map(|(_, pr)| pr.clone())
}

fn lookup(dir: &Path, branch: &str, env: &LaunchEnv) -> Option<PrInfo> {
    let deadline = Instant::now() + TIMEOUT;
    let host = lotse_git::review::origin_host(dir)?;
    let mut vars = env.vars().clone();
    // never wait for a prompt or check for updates
    for (k, v) in [
        ("GH_PROMPT_DISABLED", "1"),
        ("GH_NO_UPDATE_NOTIFIER", "1"),
        ("NO_PROMPT", "1"),
        ("GLAB_CHECK_UPDATE", "false"),
        ("NO_COLOR", "1"),
    ] {
        vars.insert(k.to_owned(), v.to_owned());
    }
    let run = |program: &str, args: &[&str]| {
        let path = env.resolve(&[program.to_owned()], dir)?;
        capture(&path, args, &vars, dir, deadline)
    };
    let github = host == "github.com"
        || run("gh", &["auth", "status", "--hostname", host.as_str()]).is_some();
    if github {
        let json = run(
            "gh",
            &[
                "pr",
                "view",
                "--json",
                "number,url,state,isDraft,statusCheckRollup",
                "--",
                branch,
            ],
        )?;
        parse_gh(&json)
    } else {
        let json = run("glab", &["mr", "view", "-F", "json", "--", branch])?;
        parse_glab(&json)
    }
}

/// Stdout of a successful run, `None` on a spawn failure, a non-zero exit or when `deadline`
/// passes (the whole process group is killed then).
fn capture(
    program: &Path,
    args: &[&str],
    env: &HashMap<String, String>,
    cwd: &Path,
    deadline: Instant,
) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = (&mut stdout).take(MAX_OUTPUT).read_to_end(&mut out);
        let _ = tx.send(out);
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                kill_group(child.id());
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let out = rx
        .recv_timeout(
            deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(200),
        )
        .ok()?;
    status
        .success()
        .then(|| String::from_utf8_lossy(&out).into_owned())
}

fn kill_group(pid: u32) {
    use nix::sys::signal::{Signal, killpg};
    let _ = killpg(nix::unistd::Pid::from_raw(pid as i32), Signal::SIGKILL);
}

/// `gh pr view --json number,url,state,isDraft,statusCheckRollup`.
fn parse_gh(json: &str) -> Option<PrInfo> {
    let v: Value = serde_json::from_str(json).ok()?;
    let state = match v["state"].as_str()? {
        "MERGED" => PrState::Merged,
        "CLOSED" => PrState::Closed,
        _ if v["isDraft"].as_bool() == Some(true) => PrState::Draft,
        _ => PrState::Open,
    };
    Some(PrInfo {
        number: v["number"].as_u64()?,
        url: v["url"].as_str()?.to_owned(),
        state,
        checks: v["statusCheckRollup"].as_array().and_then(|r| gh_checks(r)),
        provider: PrProvider::Github,
    })
}

/// Worst result of the rollup: a check run has `status` + `conclusion`, a commit status `state`.
fn gh_checks(rollup: &[Value]) -> Option<PrChecks> {
    let mut checks = None;
    for item in rollup {
        let word = |k: &str| item[k].as_str().unwrap_or("");
        let this = match (word("conclusion"), word("state"), word("status")) {
            ("FAILURE" | "TIMED_OUT" | "CANCELLED" | "STARTUP_FAILURE" | "ACTION_REQUIRED", ..)
            | (_, "FAILURE" | "ERROR", _) => PrChecks::Fail,
            (_, "PENDING" | "EXPECTED", _) => PrChecks::Pending,
            ("", "", status) if status != "COMPLETED" => PrChecks::Pending,
            _ => PrChecks::Pass,
        };
        checks = match (checks, this) {
            (Some(PrChecks::Fail), _) | (_, PrChecks::Fail) => Some(PrChecks::Fail),
            (Some(PrChecks::Pending), _) | (_, PrChecks::Pending) => Some(PrChecks::Pending),
            _ => Some(PrChecks::Pass),
        };
    }
    checks
}

/// `glab mr view -F json`: the merge request as the GitLab API returns it.
fn parse_glab(json: &str) -> Option<PrInfo> {
    let v: Value = serde_json::from_str(json).ok()?;
    let draft = v["draft"].as_bool().or(v["work_in_progress"].as_bool()) == Some(true);
    let state = match v["state"].as_str()? {
        "merged" => PrState::Merged,
        "closed" | "locked" => PrState::Closed,
        _ if draft => PrState::Draft,
        _ => PrState::Open,
    };
    let pipeline = v["head_pipeline"].as_object().or(v["pipeline"].as_object());
    let checks = match pipeline.and_then(|p| p.get("status")?.as_str()) {
        Some("success") => Some(PrChecks::Pass),
        Some("failed" | "canceled") => Some(PrChecks::Fail),
        Some(
            "created" | "pending" | "running" | "preparing" | "waiting_for_resource" | "scheduled",
        ) => Some(PrChecks::Pending),
        _ => None,
    };
    Some(PrInfo {
        number: v["iid"].as_u64()?,
        url: v["web_url"].as_str()?.to_owned(),
        state,
        checks,
        provider: PrProvider::Gitlab,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn gh_states_and_drafts() {
        let pr = |state: &str, draft: bool| {
            parse_gh(
                &json!({"number": 1, "url": "u", "state": state, "isDraft": draft,
                    "statusCheckRollup": []})
                .to_string(),
            )
            .unwrap()
        };
        assert_eq!(pr("OPEN", false).state, PrState::Open);
        assert_eq!(pr("OPEN", true).state, PrState::Draft);
        assert_eq!(pr("MERGED", false).state, PrState::Merged);
        assert_eq!(pr("CLOSED", true).state, PrState::Closed);
        assert_eq!(pr("OPEN", false).checks, None);
        assert_eq!(parse_gh("not json"), None);
        assert_eq!(parse_gh(r#"{"state":"OPEN"}"#), None);
    }

    #[test]
    fn gh_checks_take_the_worst_result() {
        let run = |status: &str, conclusion: &str| json!({"__typename": "CheckRun", "status": status, "conclusion": conclusion});
        let ctx = |state: &str| json!({"__typename": "StatusContext", "state": state});
        let checks = |items: Vec<Value>| gh_checks(&items);
        assert_eq!(checks(vec![]), None);
        assert_eq!(
            checks(vec![
                run("COMPLETED", "SUCCESS"),
                run("COMPLETED", "SKIPPED"),
                ctx("SUCCESS")
            ]),
            Some(PrChecks::Pass)
        );
        assert_eq!(
            checks(vec![run("COMPLETED", "SUCCESS"), run("IN_PROGRESS", "")]),
            Some(PrChecks::Pending)
        );
        assert_eq!(checks(vec![ctx("PENDING")]), Some(PrChecks::Pending));
        assert_eq!(
            checks(vec![run("IN_PROGRESS", ""), run("COMPLETED", "FAILURE")]),
            Some(PrChecks::Fail)
        );
        assert_eq!(checks(vec![ctx("ERROR")]), Some(PrChecks::Fail));
    }

    #[test]
    fn glab_states_and_pipeline() {
        let mr = |state: &str, draft: bool, pipeline: &str| {
            parse_glab(
                &json!({"iid": 3, "web_url": "u", "state": state, "draft": draft,
                    "head_pipeline": {"status": pipeline}})
                .to_string(),
            )
            .unwrap()
        };
        assert_eq!(mr("opened", false, "success").checks, Some(PrChecks::Pass));
        assert_eq!(mr("opened", true, "running").state, PrState::Draft);
        assert_eq!(
            mr("opened", true, "running").checks,
            Some(PrChecks::Pending)
        );
        assert_eq!(mr("merged", false, "failed").state, PrState::Merged);
        assert_eq!(mr("closed", false, "canceled").checks, Some(PrChecks::Fail));
        assert_eq!(mr("opened", false, "manual").checks, None);
        assert_eq!(
            parse_glab(r#"{"iid":1,"web_url":"u","state":"opened"}"#)
                .unwrap()
                .checks,
            None
        );
    }
}
