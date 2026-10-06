#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod config;
mod headless;
mod pet;
mod watcher;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("watch-folder {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    init_file_logging();

    let wants_headless = args.iter().any(|a| a == "--headless" || a == "--no-tray");

    // A machine with no display (headless server, SSH session) can't show a
    // tray, so go straight to the background daemon path there.
    if wants_headless || !has_display() {
        headless::run();
        return;
    }

    if let Err(err) = app::run() {
        log::error!("Tray UI unavailable ({err}); falling back to headless mode");
        headless::run();
    }
}

fn print_help() {
    println!(
        "watch-folder {}\n\
         Sorts new files in watched folders into subfolders by extension.\n\n\
         USAGE:\n    watch-folder [--headless]\n\n\
         FLAGS:\n\
         \x20   --headless, --no-tray   Run without the tray/settings window (daemonizes on Unix)\n\
         \x20   -h, --help              Print this help\n\
         \x20   -V, --version           Print the version",
        env!("CARGO_PKG_VERSION")
    );
}

fn init_file_logging() {
    let path = config::log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Default to warnings from everything, but keep our own crate at info. The
    // D-Bus/zbus stack logs every message at info; leaving it enabled floods the
    // log file (and CPU) on Linux and makes the UI sluggish.
    let mut builder = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,watch_folder=info"),
    );
    match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        Ok(file) => {
            let _ = builder.target(env_logger::Target::Pipe(Box::new(file))).try_init();
        }
        Err(_) => {
            let _ = builder.try_init();
        }
    }
}

#[cfg(target_os = "macos")]
fn has_display() -> bool {
    true
}

#[cfg(all(unix, not(target_os = "macos")))]
fn has_display() -> bool {
    let graphical = std::env::var_os("DISPLAY").is_some()
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var_os("WAYLAND_SOCKET").is_some();
    // The tray uses StatusNotifierItem over the session bus.
    let session_bus = std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some();
    graphical && session_bus
}

#[cfg(not(unix))]
fn has_display() -> bool {
    true
}
