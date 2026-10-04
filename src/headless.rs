#[cfg(unix)]
use std::path::Path;

use crate::{config, watcher};

/// Headless mode: no tray or settings window. On Unix the process daemonizes;
/// elsewhere it runs in the foreground.
pub fn run() {
    let config = config::load_or_create();

    start_daemon();

    if let Err(err) = watcher::run_blocking(config) {
        log::error!("Error: {err}");
    }
}

#[cfg(unix)]
fn start_daemon() {
    use daemonize_me::Daemon;
    use std::fs::File;
    use std::process::exit;

    // Bail out if an instance is already running.
    let pid_file = Path::new("watch-dir.pid");
    if pid_file.exists() {
        eprintln!("The daemon is already running");
        exit(-1);
    }

    let stdout = File::create("/tmp/info.log").unwrap();
    let stderr = File::create("/tmp/err.log").unwrap();
    let daemon = Daemon::new()
        .pid_file("watch-dir.pid", Some(false))
        .umask(0o000)
        .work_dir(".")
        .stdout(stdout)
        .stderr(stderr)
        .start();

    match daemon {
        Ok(_) => println!("Daemonized with success"),
        Err(e) => {
            eprintln!("Error, {e}");
            exit(-1);
        }
    }
}

#[cfg(not(unix))]
fn start_daemon() {
    // No daemonizing on Windows/macOS; run in the foreground.
}
