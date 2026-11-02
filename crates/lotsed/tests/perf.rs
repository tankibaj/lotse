//! Manual resource profile of the daemon (`#[ignore]`d): CPU% and RSS per load phase.
//! Run: `cargo test -p lotsed --release --test perf -- --ignored --nocapture`

mod common;

use common::*;
use std::process::Command;
use std::time::{Duration, Instant};

const SAMPLE: Duration = Duration::from_millis(500);
const PHASE: Duration = Duration::from_secs(10);

/// `(cpu %, rss MB)` of the daemon right now.
fn ps(pid: u32) -> (f64, f64) {
    let out = Command::new("ps")
        .args(["-o", "%cpu=,rss=", "-p", &pid.to_string()])
        .output()
        .expect("run ps");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let mut it = text.split_whitespace();
    let cpu = it.next().and_then(|s| s.parse().ok()).expect("ps %cpu");
    let rss_kb: f64 = it.next().and_then(|s| s.parse().ok()).expect("ps rss");
    (cpu, rss_kb / 1024.0)
}

/// Sample every 500 ms while the client drains, until `done` (or at least `PHASE` when `None`).
/// Prints the PERF line and returns `(cpu_avg, rss_max_mb, elapsed)`.
fn phase(
    name: &str,
    pid: u32,
    c: &mut Client,
    done: Option<&dyn Fn() -> bool>,
) -> (f64, f64, Duration) {
    let start = Instant::now();
    let (mut sum, mut max, mut rss_max, mut n) = (0.0, 0.0_f64, 0.0_f64, 0);
    loop {
        c.drain(SAMPLE);
        c.clear_output();
        let (cpu, rss) = ps(pid);
        sum += cpu;
        max = max.max(cpu);
        rss_max = rss_max.max(rss);
        n += 1;
        let finished = match done {
            Some(f) => f() || start.elapsed() > Duration::from_secs(180),
            None => start.elapsed() >= PHASE,
        };
        if finished {
            break;
        }
    }
    let avg = sum / f64::from(n);
    println!("PERF {name} cpu_avg={avg:.1} cpu_max={max:.1} rss_max_mb={rss_max:.1}");
    (avg, rss_max, start.elapsed())
}

#[test]
#[ignore = "manual perf profile"]
fn perf_profile() {
    let d = Daemon::start();
    let pid = d.pid();
    let mut c = d.client();

    phase("idle-empty", pid, &mut c, None);

    let project = c.add_project(&d.project_dir("proj"));
    let sessions: Vec<_> = (0..20)
        .map(|_| c.create_session(&project, Some("/bin/sh")))
        .collect();
    for s in &sessions {
        c.attach(&s.id, 80, 24);
    }
    let (_, rss_shells, _) = phase("idle-20-shells", pid, &mut c, None);

    for s in &sessions[..10] {
        c.send_pty(&s.id, b"while :; do date; sleep 0.05; done\n");
    }
    phase("steady-output", pid, &mut c, None);

    // stop the date loops, then flood one session
    for s in &sessions[..10] {
        c.send_pty(&s.id, b"\x03");
    }
    c.drain(Duration::from_secs(1));
    let marker = d.project_dir("proj").join("flood-done");
    c.send_pty(
        &sessions[0].id,
        format!(
            "head -c 50000000 /dev/zero | tr '\\0' x; echo; touch '{}'\n",
            marker.display()
        )
        .as_bytes(),
    );
    let (_, _, took) = phase("flood-one", pid, &mut c, Some(&|| marker.exists()));
    assert!(marker.exists(), "the flood never finished");
    println!("PERF flood-one wall_s={:.1}", took.as_secs_f64());

    for s in &sessions {
        c.send_pty(&s.id, b"\x03");
    }
    c.drain(Duration::from_secs(2));
    let (cpu, rss, _) = phase("idle-after", pid, &mut c, None);
    println!(
        "PERF idle-after rss_growth_vs_idle_20_shells_mb={:.1} cpu_avg={cpu:.1}",
        rss - rss_shells
    );
}
