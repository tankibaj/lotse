//! `lotsed` command line.
//!
//! ```text
//! lotsed [--data-dir DIR] [--no-login-env]            run in the foreground
//! lotsed --detach [--data-dir DIR] [--no-login-env]   start in the background (own session, log file)
//! ```
//! `LOTSE_DATA_DIR` is the environment form of `--data-dir`.

use lotsed::config::Config;
use lotsed::lifecycle;
use std::path::PathBuf;
use std::process::ExitCode;
use tracing_subscriber::EnvFilter;

const USAGE: &str = "usage: lotsed [--detach] [--data-dir DIR] [--no-login-env] [--version]";

fn main() -> ExitCode {
    let mut data_dir = std::env::var_os("LOTSE_DATA_DIR").map(PathBuf::from);
    let mut login_env = true;
    let mut detach = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => match args.next() {
                Some(d) => data_dir = Some(PathBuf::from(d)),
                None => return usage("--data-dir needs a value"),
            },
            "--no-login-env" => login_env = false,
            "--detach" => detach = true,
            "--version" | "-V" => {
                println!(
                    "lotsed {} (proto {})",
                    env!("CARGO_PKG_VERSION"),
                    lotse_proto::PROTO_VERSION
                );
                return ExitCode::SUCCESS;
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other => return usage(&format!("unknown argument {other:?}")),
        }
    }
    let Some(data_dir) = data_dir.or_else(Config::default_data_dir) else {
        eprintln!("lotsed: cannot determine the data directory, pass --data-dir");
        return ExitCode::FAILURE;
    };
    let cfg = Config::new(data_dir, login_env);

    if detach {
        return match lifecycle::detach(&cfg) {
            Ok((pid, true)) => {
                println!("lotsed already running (pid {pid})");
                ExitCode::SUCCESS
            }
            Ok((pid, false)) => {
                println!("lotsed started (pid {pid})");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("lotsed: {e:#}");
                ExitCode::FAILURE
            }
        };
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();
    match lifecycle::run(cfg) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

fn usage(msg: &str) -> ExitCode {
    eprintln!("lotsed: {msg}\n{USAGE}");
    ExitCode::from(2)
}
