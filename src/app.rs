use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use auto_launch::{AutoLaunch, AutoLaunchBuilder};
#[cfg(target_os = "linux")]
use auto_launch::LinuxLaunchMode;
use eframe::egui;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

use crate::config::{self, Config, Stability, WatchEntry};
use crate::watcher::Watcher;

const ID_SHOW: &str = "watch-folder.show";
const ID_PAUSE: &str = "watch-folder.pause";
const ID_RELOAD: &str = "watch-folder.reload";
const ID_OPEN_CONFIG: &str = "watch-folder.open-config";
const ID_OPEN_LOGS: &str = "watch-folder.open-logs";
const ID_QUIT: &str = "watch-folder.quit";

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Loopback port used for single-instance detection and takeover.
const INSTANCE_PORT: u16 = 49717;
/// Passed to the binary by the autostart entry so it starts hidden (tray only).
const AUTOSTART_ARG: &str = "--autostart";

/// Can winit's X11 backend run here? It needs an X server (XWayland) and the
/// `libxkbcommon-x11` shared library that it dlopens for its keymap. The latter
/// is not installed on every Wayland-only desktop, so probe for it explicitly
/// rather than crashing when winit tries to load it.
#[cfg(target_os = "linux")]
fn x11_backend_available() -> bool {
    let has_display = std::env::var_os("DISPLAY")
        .map(|d| !d.is_empty())
        .unwrap_or(false);
    if !has_display {
        return false;
    }
    let name = std::ffi::CString::new("libxkbcommon-x11.so.0").unwrap();
    unsafe { !libc::dlopen(name.as_ptr(), libc::RTLD_LAZY).is_null() }
}

fn has_arg(name: &str) -> bool {
    std::env::args().any(|a| a == name)
}

fn home_dir() -> std::path::PathBuf {
    home::home_dir().expect("could not determine home directory")
}

/// Where an installed copy lives.
fn install_path() -> std::path::PathBuf {
    #[cfg(windows)]
    // Use real path separators: the autostart entry embeds this path verbatim,
    // and the shell's Run-key launcher rejects a path with forward slashes.
    let path = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home_dir().join("AppData").join("Local"))
        .join("Programs")
        .join("watch-folder")
        .join("watch-folder.exe");
    #[cfg(target_os = "macos")]
    let path = home_dir().join("Applications/watch-folder");
    #[cfg(all(unix, not(target_os = "macos")))]
    let path = home_dir().join(".local/bin/watch-folder");
    #[cfg(not(any(windows, target_os = "macos", unix)))]
    let path = home_dir().join("watch-folder");
    path
}

fn installed_marker() -> std::path::PathBuf {
    home_dir().join(".config/watch-dir/installed")
}

fn read_installed_version() -> Option<String> {
    std::fs::read_to_string(installed_marker())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Copy this executable to the install location (retrying in case an old copy
/// was just terminated and the file is briefly locked on Windows).
fn install_self() -> std::io::Result<std::path::PathBuf> {
    let target = install_path();
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let current = std::env::current_exe()?;
    if current == target {
        let _ = std::fs::write(installed_marker(), CURRENT_VERSION);
        return Ok(target);
    }

    let mut last_err = None;
    for _ in 0..8 {
        match std::fs::copy(&current, &target) {
            Ok(_) => {
                last_err = None;
                break;
            }
            Err(err) => {
                last_err = Some(err);
                thread::sleep(Duration::from_millis(300));
            }
        }
    }
    if let Some(err) = last_err {
        return Err(err);
    }

    let _ = std::fs::write(installed_marker(), CURRENT_VERSION);
    Ok(target)
}

fn auto_launch_for(path: &Path) -> Option<AutoLaunch> {
    let path = path.to_string_lossy().to_string();
    let mut builder = AutoLaunchBuilder::new();
    builder
        .set_app_name("watch-folder")
        .set_app_path(&path)
        .set_args(&[AUTOSTART_ARG]);
    #[cfg(target_os = "linux")]
    builder.set_linux_launch_mode(LinuxLaunchMode::XdgAutostart);
    builder
        .build()
        .map_err(|e| log::warn!("autostart unavailable: {e}"))
        .ok()
}

/// Start the tray application with its egui settings window.
///
/// Returns an error if the tray icon or the windowing system is unavailable,
/// so the caller can fall back to headless mode.
pub fn run() -> Result<(), String> {
    let autostart_launch = has_arg(AUTOSTART_ARG);
    let force_install = has_arg("--install");
    let force_test_run = has_arg("--test-run");

    let own = CURRENT_VERSION;
    let running = running_version();

    // If an equal/newer instance is already running, surface it and stop.
    if let Some(running_version) = &running {
        if !version_gt(own, running_version) {
            if !autostart_launch {
                log::info!("v{running_version} already running; showing it");
                send_instance_command("SHOW");
            }
            return Ok(());
        }
        log::info!("running instance v{running_version} is older than v{own}");
    }

    // Only one instance may run. A newer binary replaces an older running one.
    let listener = match acquire_instance() {
        InstanceOutcome::Primary(listener) => listener,
        InstanceOutcome::AlreadyRunning => return Ok(()),
    };

    let config = config::load_or_create();
    install_desktop_entry(config.icon_style);
    let paused = Arc::new(AtomicBool::new(false));
    let visible = Arc::new(AtomicBool::new(!autostart_launch));

    let (pet_tx, pet_rx) = channel::<crate::pet::PetEvent>();
    let watcher = Watcher::start_with(config.clone(), paused.clone(), Some(pet_tx.clone()));

    let (reload_tx, reload_rx) = channel::<()>();

    let is_installed_self = std::env::current_exe()
        .map(|p| p == install_path())
        .unwrap_or(false);
    let installed = read_installed_version();
    let current_version = running.clone().or(installed);

    // Ask to install / test-run when this binary is newer than what's around.
    let need_prompt = !autostart_launch
        && !force_install
        && !force_test_run
        && !is_installed_self
        && match &current_version {
            Some(version) => version_gt(own, version),
            None => true,
        };

    // 128 px is enough for the title-bar / taskbar icon at up to 400 % scaling,
    // and keeps the `_NET_WM_ICON` X11 request comfortably under the 256 KiB
    // protocol limit (a 256 px icon needs Big-Requests, which some servers or
    // compositors silently drop, leaving the taskbar icon blank).
    let (rgba_window, ww, wh) = icon_rgba(config.icon_style, 128);
    let viewport_icon = egui::IconData {
        rgba: rgba_window,
        width: ww,
        height: wh,
    };

    // 128 px for the tray: high enough for 200 % DPI (32-px slot × 4× factor)
    // while still keeping the buffer small enough for the tray-icon crate.
    let (rgba_tray, tw, th) = icon_rgba(config.icon_style, 128);

    let root_viewport = egui::ViewportBuilder::default()
        .with_title("watch-folder")
        .with_app_id("watch-folder")
        .with_inner_size([640.0, 700.0])
        .with_min_inner_size([480.0, 420.0])
        .with_visible(!autostart_launch)
        .with_icon(viewport_icon);
    // The root window's GL config decides whether an alpha channel exists at
    // all (see the DESKTOP_PET_GUIDE note). The desktop pet is drawn in a
    // transparent child viewport, so without this the whole ego GL config has
    // no alpha bits and the pet falls back to an opaque black rectangle. The
    // settings UI paints its own opaque panels, so this is invisible there.
    #[cfg(not(windows))]
    let root_viewport = root_viewport.with_transparent(true);

    // `mut` is only needed by the Linux X11 block below.
    #[allow(unused_mut)]
    let mut native_options = eframe::NativeOptions {
        viewport: root_viewport,
        ..Default::default()
    };

    // GNOME on Wayland does not let a client position, raise, or click-through
    // its own windows (winit's `set_outer_position`/`set_window_level` are
    // no-ops there). The desktop pet relies on all three, so when an X server
    // (XWayland) is available we run the whole UI through X11 instead, where
    // those calls are honoured. A pure-Wayland session, or one missing the
    // library winit's X11 backend needs, falls back to the Wayland backend.
    #[cfg(target_os = "linux")]
    {
        if x11_backend_available() {
            native_options.event_loop_builder = Some(Box::new(|builder| {
                use winit::platform::x11::EventLoopBuilderExtX11 as _;
                builder.with_x11();
            }));
            log::info!("using X11 (XWayland) backend for window placement");
        } else {
            log::warn!(
                "X11/XWayland backend unavailable (install libxkbcommon-x11-0); \
                 staying on Wayland, where the desktop pet cannot be positioned \
                 or kept on top"
            );
        }
    }

    eframe::run_native(
        "watch-folder",
        native_options,
        Box::new(move |cc| {
            configure_style(&cc.egui_ctx);
            let hwnd = window_handle_isize(cc);
            // Style the settings window by X id right away, before eframe's
            // forced first show maps it. This keeps a hidden autostart launch
            // out of the dock/taskbar and titles from flashing on screen.
            #[cfg(not(windows))]
            if hwnd != 0 {
                let hidden = !visible.load(Ordering::SeqCst);
                crate::pet::taskbar::x11_style_window(hwnd as u32, hidden, !hidden);
            }
            let tray = create_tray(rgba_tray, tw, th)?;

            // Listen for other launches (show window / takeover requests).
            spawn_instance_listener(listener, cc.egui_ctx.clone(), visible.clone(), hwnd);

            // Consume tray menu events on a dedicated thread and act on them
            // immediately. This does not depend on egui's repaint loop, so it
            // works even while the window is hidden.
            {
                let ctx = cc.egui_ctx.clone();
                let paused = paused.clone();
                let visible = visible.clone();
                let reload_tx = reload_tx.clone();
                thread::spawn(move || {
                    let events = MenuEvent::receiver();
                    while let Ok(event) = events.recv() {
                        handle_menu_event(
                            &event.id.0,
                            &ctx,
                            &paused,
                            &visible,
                            &reload_tx,
                            hwnd,
                        );
                    }
                });
            }

            Ok(Box::new(App::new(
                config,
                watcher,
                tray,
                paused,
                visible,
                reload_rx,
                hwnd,
                pet_rx,
                pet_tx,
                need_prompt,
                force_install,
                current_version,
            )))
        }),
    )
    .map_err(|e| e.to_string())
}

fn handle_menu_event(
    id: &str,
    ctx: &egui::Context,
    paused: &Arc<AtomicBool>,
    visible: &Arc<AtomicBool>,
    reload_tx: &Sender<()>,
    hwnd: isize,
) {
    log::info!("tray menu click: {id}");
    match id {
        ID_SHOW => {
            let show = !visible.load(Ordering::SeqCst);
            apply_visibility(show, ctx, hwnd, visible);
        }
        ID_PAUSE => {
            let now_paused = !paused.load(Ordering::SeqCst);
            paused.store(now_paused, Ordering::SeqCst);
            log::info!("watcher {}", if now_paused { "paused" } else { "resumed" });
        }
        ID_RELOAD => {
            let _ = reload_tx.send(());
        }
        ID_OPEN_CONFIG => open_path(&config::config_path()),
        ID_OPEN_LOGS => open_path(&config::log_path()),
        ID_QUIT => {
            log::info!("quit requested from tray");
            // Ask the native pet thread to stop before we tear the process down.
            crate::pet::request_shutdown();
            // Hard-exit so quitting works even if the window is hidden and no
            // further frame is drawn. Submitted file moves are atomic renames,
            // so there is no partial state to flush.
            std::process::exit(0);
        }
        _ => {}
    }
    ctx.request_repaint();
}

/// Open a file with its default application; if there is no association
/// (common for `.yaml` on Windows), reveal the containing folder instead.
fn open_path(path: &Path) {
    log::info!("opening {path:?}");
    if opener::open(path).is_err() {
        log::warn!("no handler for {path:?}; opening its folder");
        if let Some(parent) = path.parent() {
            let _ = opener::open(parent);
        }
    }
}

fn window_handle_isize(cc: &eframe::CreationContext<'_>) -> isize {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = cc.window_handle() {
            if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                return win32.hwnd.get();
            }
        }
    }
    // On X11/XWayland this is the raw X window id, which lets us style the
    // window (decorations, skip-taskbar) before the WM maps it.
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = cc.window_handle() {
            match handle.as_raw() {
                RawWindowHandle::Xlib(h) => return h.window as isize,
                RawWindowHandle::Xcb(h) => return h.window.get() as isize,
                _ => {}
            }
        }
    }
    let _ = cc;
    0
}

/// Show or hide the window.
///
/// Hiding via egui's viewport command stops redraws, which means the matching
/// "show" command is never processed — so on Windows we drive the OS window
/// directly, which works even while it is hidden.
fn apply_visibility(show: bool, ctx: &egui::Context, hwnd: isize, visible: &Arc<AtomicBool>) {
    visible.store(show, Ordering::SeqCst);
    os_set_visible(hwnd, show);
    #[cfg(not(windows))]
    {
        if show {
            // Restore the titlebar that was removed while parked (see below).
            crate::pet::taskbar::x11_set_decorations("watch-folder", true);
            // Restore the size and place the window back where it was.
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
                SETTINGS_MIN_SIZE[0],
                SETTINGS_MIN_SIZE[1],
            )));
            if let Some(size) = *SAVED_WINDOW_SIZE.lock().unwrap() {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            }
            if let Some(pos) = *SAVED_WINDOW_POS.lock().unwrap() {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        } else {
            // Collapse to a single transparent pixel instead of unmapping it.
            // Unmapping stops eframe's redraw loop on Linux (freezing the
            // desktop pet), and GNOME's window manager refuses to park a window
            // off-screen, so the old "move to -32000" trick left a transparent
            // rectangle on screen. A 1x1 transparent window is invisible yet
            // keeps the event loop alive so the pet keeps animating.
            if let Some(r) = ctx.input(|i| i.viewport().outer_rect) {
                *SAVED_WINDOW_POS.lock().unwrap() = Some(r.min);
            }
            if let Some(r) = ctx.input(|i| i.viewport().inner_rect) {
                *SAVED_WINDOW_SIZE.lock().unwrap() = Some(r.size());
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(1.0, 1.0)));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1.0, 1.0)));
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(0.0, 0.0)));
            // Drop the titlebar/borders: mutter otherwise still draws a
            // titlebar-sized frame around the 1x1 client, which stays visible.
            crate::pet::taskbar::x11_set_decorations("watch-folder", false);
        }
        // Keep the collapsed window out of the dock and Activities overview.
        crate::pet::taskbar::x11_set_skip_taskbar("watch-folder", !show);
    }
    #[cfg(windows)]
    {
        if show {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }
    ctx.request_repaint();
}

/// The settings window's normal minimum size, restored when it is shown again.
#[cfg(not(windows))]
const SETTINGS_MIN_SIZE: [f32; 2] = [480.0, 420.0];

/// Last on-screen position of the settings window, saved before parking it.
#[cfg(not(windows))]
static SAVED_WINDOW_POS: std::sync::Mutex<Option<egui::Pos2>> = std::sync::Mutex::new(None);

/// Last inner size of the settings window, saved before shrinking it.
#[cfg(not(windows))]
static SAVED_WINDOW_SIZE: std::sync::Mutex<Option<egui::Vec2>> = std::sync::Mutex::new(None);

#[cfg(windows)]
fn os_set_visible(hwnd: isize, show: bool) {
    if hwnd == 0 {
        return;
    }
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongW, GetWindowRect, SetForegroundWindow, SetWindowLongW, SetWindowPos,
        ShowWindow, GWL_EXSTYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_SHOWWINDOW,
        SW_HIDE, SW_RESTORE, SW_SHOW, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };

    static SAVED_X: AtomicI32 = AtomicI32::new(100);
    static SAVED_Y: AtomicI32 = AtomicI32::new(100);
    static SAVED_W: AtomicI32 = AtomicI32::new(640);
    static SAVED_H: AtomicI32 = AtomicI32::new(700);
    static HAS_SAVED: AtomicBool = AtomicBool::new(false);

    let hwnd = hwnd as HWND;
    unsafe {
        if show {
            // Restore normal ex-style: drop toolwindow and re-assert appwindow so the
            // settings window appears in the taskbar / Alt-Tab while it is open.
            ShowWindow(hwnd, SW_HIDE as i32);
            let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
            let ex_style = (ex_style & !(WS_EX_TOOLWINDOW as i32)) | (WS_EX_APPWINDOW as i32);
            SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style);

            // Restore saved position
            let x = SAVED_X.load(Ordering::SeqCst);
            let y = SAVED_Y.load(Ordering::SeqCst);
            let w = SAVED_W.load(Ordering::SeqCst);
            let h = SAVED_H.load(Ordering::SeqCst);
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                x,
                y,
                w,
                h,
                SWP_FRAMECHANGED | SWP_SHOWWINDOW,
            );
            ShowWindow(hwnd, SW_RESTORE as i32);
            ShowWindow(hwnd, SW_SHOW as i32);
            SetForegroundWindow(hwnd);
        } else {
            // Save current position before parking offscreen
            let mut rect = std::mem::MaybeUninit::<RECT>::uninit();
            if GetWindowRect(hwnd, rect.as_mut_ptr()) != 0 {
                let r = rect.assume_init();
                if r.left > -10000 {
                    SAVED_X.store(r.left, Ordering::SeqCst);
                    SAVED_Y.store(r.top, Ordering::SeqCst);
                    SAVED_W.store((r.right - r.left).max(400), Ordering::SeqCst);
                    SAVED_H.store((r.bottom - r.top).max(300), Ordering::SeqCst);
                    HAS_SAVED.store(true, Ordering::SeqCst);
                }
            }

            // Hide window first so the Windows Taskbar immediately drops the taskbar button
            ShowWindow(hwnd, SW_HIDE as i32);

            // Set as toolwindow so it does not appear on taskbar or alt-tab when shown.
            // WS_EX_APPWINDOW forces a window onto the taskbar/Alt-Tab/Task View and
            // overrides WS_EX_TOOLWINDOW, so it must be cleared too — otherwise the
            // parked window keeps showing up in the "all virtual desktops" view.
            let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
            let ex_style = (ex_style | (WS_EX_TOOLWINDOW as i32)) & !(WS_EX_APPWINDOW as i32);
            SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style);

            // Park offscreen with SWP_SHOWWINDOW. The window is now shown offscreen as a toolwindow:
            // the Windows Taskbar NEVER shows a button for it, but Windows OS continues message
            // pumping for eframe and the desktop companion!
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                -32000,
                -32000,
                100,
                100,
                SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }
}

#[cfg(not(windows))]
fn os_set_visible(_hwnd: isize, _show: bool) {}

/// TEMP diagnostic: log frames/sec and average per-frame cost.
fn record_frame(app: &App, total_ms: f64, pet_update_ms: f64, pet_render_ms: f64) {
    use std::sync::{Mutex, OnceLock};
    use std::time::Instant;
    struct Acc {
        start: Instant,
        frames: u64,
        total_ms: f64,
        pet_update_ms: f64,
        pet_render_ms: f64,
    }
    static STATE: OnceLock<Mutex<Acc>> = OnceLock::new();
    let state = STATE.get_or_init(|| {
        Mutex::new(Acc {
            start: Instant::now(),
            frames: 0,
            total_ms: 0.0,
            pet_update_ms: 0.0,
            pet_render_ms: 0.0,
        })
    });
    let mut acc = state.lock().unwrap();
    acc.frames += 1;
    acc.total_ms += total_ms;
    acc.pet_update_ms += pet_update_ms;
    acc.pet_render_ms += pet_render_ms;
    let elapsed = acc.start.elapsed().as_secs_f64();
    if elapsed >= 2.0 {
        let frames = acc.frames as f64;
        log::debug!(
            "ui fps {:.1} frame {:.2}ms pet_update {:.2}ms pet_render {:.2}ms pet_enabled={} state={:?} papers={} carried={}",
            frames / elapsed,
            acc.total_ms / frames,
            acc.pet_update_ms / frames,
            acc.pet_render_ms / frames,
            app.pet_enabled(),
            app.pet_state(),
            app.pet_paper_count(),
            app.pet_carried_count(),
        );
        acc.start = Instant::now();
        acc.frames = 0;
        acc.total_ms = 0.0;
        acc.pet_update_ms = 0.0;
        acc.pet_render_ms = 0.0;
    }
}

/// Brighter, larger text and roomier spacing than the egui defaults.
fn configure_style(ctx: &egui::Context) {
    use egui::{Color32, FontId, TextStyle, Visuals};

    let mut visuals = Visuals::dark();
    visuals.override_text_color = Some(Color32::from_rgb(233, 236, 244));
    visuals.panel_fill = Color32::from_rgb(23, 25, 32);
    visuals.window_fill = Color32::from_rgb(23, 25, 32);
    visuals.faint_bg_color = Color32::from_rgb(31, 34, 43);
    visuals.extreme_bg_color = Color32::from_rgb(16, 18, 24);
    visuals.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(196, 202, 214);
    visuals.widgets.inactive.fg_stroke.color = Color32::from_rgb(220, 224, 232);
    visuals.selection.bg_fill = Color32::from_rgb(59, 130, 246);
    visuals.hyperlink_color = Color32::from_rgb(120, 170, 255);
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Heading, FontId::proportional(22.0)),
        (TextStyle::Body, FontId::proportional(15.5)),
        (TextStyle::Monospace, FontId::monospace(14.0)),
        (TextStyle::Button, FontId::proportional(15.0)),
        (TextStyle::Small, FontId::proportional(12.5)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    ctx.set_style(style);
}

enum InstanceOutcome {
    Primary(TcpListener),
    AlreadyRunning,
}

fn instance_addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], INSTANCE_PORT))
}

fn bind_instance(tries: u32) -> Option<TcpListener> {
    for _ in 0..tries {
        if let Ok(listener) = TcpListener::bind(instance_addr()) {
            return Some(listener);
        }
        thread::sleep(Duration::from_millis(100));
    }
    None
}

/// Stop any other running watch-folder process.
///
/// Older builds (v0.2.0) predate the instance listener, so they can't be asked
/// to quit over IPC — they have to be terminated by name. This runs only once
/// we have established ourselves as the primary instance.
#[cfg(windows)]
fn kill_other_instances() {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let me = std::process::id();

    let output = match std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq watch-folder*", "/FO", "CSV", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(output) => output,
        Err(err) => {
            log::warn!("could not enumerate processes: {err}");
            return;
        }
    };

    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        // CSV: "watch-folder.exe","1234","Console","1","12,345 K"
        let mut fields = line.split(',');
        let _name = fields.next();
        let pid = fields.next().map(|p| p.trim_matches('"'));
        let Some(pid) = pid.and_then(|p| p.parse::<u32>().ok()) else {
            continue;
        };
        if pid == me {
            continue;
        }
        log::info!("stopping old watch-folder process (pid {pid})");
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
}

#[cfg(unix)]
fn kill_other_instances() {
    let me = std::process::id().to_string();
    let output = match std::process::Command::new("pgrep")
        .args(["-x", "watch-folder"])
        .output()
    {
        Ok(output) => output,
        Err(_) => return,
    };
    let text = String::from_utf8_lossy(&output.stdout);
    for pid in text.split_whitespace() {
        if pid == me {
            continue;
        }
        log::info!("stopping old watch-folder process (pid {pid})");
        let _ = std::process::Command::new("kill").args(["-9", pid]).output();
    }
}

#[cfg(not(any(windows, unix)))]
fn kill_other_instances() {}

/// Try to become the primary instance. If another instance is already running,
/// either take it over (when this binary is newer) or ask it to show its window.
fn acquire_instance() -> InstanceOutcome {
    if let Some(listener) = bind_instance(20) {
        kill_other_instances();
        return InstanceOutcome::Primary(listener);
    }

    let running = running_version();
    let newer = running
        .as_deref()
        .map(|v| version_gt(CURRENT_VERSION, v))
        .unwrap_or(false);

    if newer {
        log::info!(
            "replacing running instance v{} with v{CURRENT_VERSION}",
            running.as_deref().unwrap_or("?")
        );
        send_instance_command("REPLACE");
        if let Some(listener) = bind_instance(50) {
            kill_other_instances();
            return InstanceOutcome::Primary(listener);
        }

        // The running instance didn't release the port (e.g. its listener is
        // stuck). Stop it by name, then try once more.
        log::warn!("old instance did not exit on request; stopping it by name");
        kill_other_instances();
        if let Some(listener) = bind_instance(50) {
            return InstanceOutcome::Primary(listener);
        }
        log::warn!("could not take over the instance port; exiting");
        return InstanceOutcome::AlreadyRunning;
    }

    log::info!(
        "another instance is already running (v{}); asking it to show",
        running.as_deref().unwrap_or("?")
    );
    send_instance_command("SHOW");
    InstanceOutcome::AlreadyRunning
}

fn connect_instance() -> Option<TcpStream> {
    TcpStream::connect_timeout(&instance_addr(), Duration::from_millis(500)).ok()
}

fn send_instance_command(command: &str) {
    if let Some(mut stream) = connect_instance() {
        let _ = writeln!(stream, "{command}");
    }
}

fn running_version() -> Option<String> {
    let mut stream = connect_instance()?;
    stream.set_read_timeout(Some(Duration::from_millis(500))).ok()?;
    writeln!(stream, "HELLO {CURRENT_VERSION}").ok()?;
    // Read up to the newline (or EOF): the reply can arrive split across
    // several TCP segments, and a single `read` used to lose the tail of the
    // version number — which silently broke update detection.
    let mut text = String::new();
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                if byte[0] == b'\n' {
                    break;
                }
                text.push(byte[0] as char);
                if text.len() > 64 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    text.trim()
        .strip_prefix("VERSION ")
        .map(|v| v.trim().to_string())
}

fn spawn_instance_listener(
    listener: TcpListener,
    ctx: egui::Context,
    visible: Arc<AtomicBool>,
    hwnd: isize,
) {
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            // Read the whole command line: a single `read` can return a partial
            // line, and leaving bytes unread makes closing the socket send an
            // RST that can discard the reply. See `running_version`.
            let mut line = String::new();
            let mut byte = [0u8; 1];
            loop {
                match stream.read(&mut byte) {
                    Ok(0) => break,
                    Ok(_) => {
                        if byte[0] == b'\n' {
                            break;
                        }
                        line.push(byte[0] as char);
                        if line.len() > 128 {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let command = line.trim();
            // Reply with a single write so the client receives it in one piece.
            if command.starts_with("HELLO") {
                let _ = stream.write_all(format!("VERSION {CURRENT_VERSION}\n").as_bytes());
            } else if command == "SHOW" {
                apply_visibility(true, &ctx, hwnd, &visible);
            } else if command == "REPLACE" {
                log::info!("a newer instance is taking over; exiting");
                crate::pet::request_shutdown();
                std::process::exit(0);
            }
        }
    });
}

fn version_gt(a: &str, b: &str) -> bool {
    let a = parse_version(a);
    let b = parse_version(b);
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

fn parse_version(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches('v')
        .split('.')
        .map(|part| {
            part.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap_or(0)
        })
        .collect()
}

struct TrayHandles {
    tray: TrayIcon,
    status_item: MenuItem,
    pause_item: MenuItem,
}

fn create_tray(
    rgba: Vec<u8>,
    width: u32,
    height: u32,
) -> Result<TrayHandles, Box<dyn std::error::Error + Send + Sync>> {
    let menu = Menu::new();

    let status_item =
        MenuItem::with_id(MenuId::new("watch-folder.status"), "Status: Watching", false, None);
    let show = MenuItem::with_id(MenuId::new(ID_SHOW), "Show / hide settings", true, None);
    let pause_item = MenuItem::with_id(MenuId::new(ID_PAUSE), "Pause watching", true, None);
    let reload = MenuItem::with_id(MenuId::new(ID_RELOAD), "Reload config", true, None);
    let config_item = MenuItem::with_id(MenuId::new(ID_OPEN_CONFIG), "Open config file", true, None);
    let logs = MenuItem::with_id(MenuId::new(ID_OPEN_LOGS), "Open logs", true, None);
    let quit = MenuItem::with_id(MenuId::new(ID_QUIT), "Quit", true, None);

    menu.append(&status_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&show)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&pause_item)?;
    menu.append(&reload)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&config_item)?;
    menu.append(&logs)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&quit)?;

    let icon = tray_icon::Icon::from_rgba(rgba, width, height)?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("watch-folder - watching")
        .with_icon(icon)
        .build()?;

    Ok(TrayHandles {
        tray,
        status_item,
        pause_item,
    })
}

#[derive(PartialEq)]
enum Stage {
    Prompt,
    Running,
}

struct App {
    config: Config,
    draft: Draft,
    watcher: Watcher,
    reload_rx: Receiver<()>,
    tray: TrayHandles,
    paused: Arc<AtomicBool>,
    visible: Arc<AtomicBool>,
    hwnd: isize,
    status: String,
    last_tray_status: String,
    auto: Option<AutoLaunch>,
    autostart: bool,
    #[cfg(windows)]
    native_pet: Option<crate::pet::native::NativePet>,
    #[cfg(windows)]
    pet_preview: Option<crate::pet::animation::PetTextures>,
    #[cfg(windows)]
    pet_spec: crate::pet::character::CharacterSpec,
    #[cfg(not(windows))]
    pet: crate::pet::PetController,
    pet_tx: Sender<crate::pet::PetEvent>,
    pet_kind: crate::pet::character::CharacterKind,
    stage: Stage,
    own_version: &'static str,
    current_version: Option<String>,
    do_install: bool,
    startup_frame: u32,
    /// Last time we (re)asserted skip-taskbar for the hidden settings window.
    #[cfg(not(windows))]
    settings_skip_at: std::time::Instant,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    fn new(
        config: Config,
        watcher: Watcher,
        tray: TrayHandles,
        paused: Arc<AtomicBool>,
        visible: Arc<AtomicBool>,
        reload_rx: Receiver<()>,
        hwnd: isize,
        pet_rx: Receiver<crate::pet::PetEvent>,
        pet_tx: Sender<crate::pet::PetEvent>,
        need_prompt: bool,
        force_install: bool,
        current_version: Option<String>,
    ) -> App {
        let draft = Draft::from_config(&config);
        let auto = build_auto_launch();
        let autostart = auto
            .as_ref()
            .and_then(|a| a.is_enabled().ok())
            .unwrap_or(false);
        // If autostart is on, re-enable so the entry points at this binary —
        // otherwise an upgrade would keep launching the old copy at login.
        if autostart {
            if let Some(auto) = &auto {
                let _ = auto.enable();
            }
        }

        let kind = config.pet_character;

        #[cfg(windows)]
        let native_pet = crate::pet::native::NativePet::spawn(
            config.pet_enabled,
            28.0,
            config.pet_position_offset,
            kind,
            pet_rx,
        );
        #[cfg(windows)]
        let pet_spec = crate::pet::character::CharacterSpec::for_kind(kind);
        // TODO(linux/macos): replace this egui-viewport fallback with a native
        // per-pixel-alpha overlay (see the TODOs in `src/pet/mod.rs`) so
        // transparency and idle CPU match the Windows `pet::native` backend.
        #[cfg(not(windows))]
        let mut pet = crate::pet::PetController::new_with_kind(Some(pet_rx), kind);
        #[cfg(not(windows))]
        {
            pet.enabled = config.pet_enabled;
            pet.position_offset = config.pet_position_offset;
            // On the X11/XWayland backend the pet viewport is a real X window:
            // shape its input region so clicks on the sprite reach it (that is
            // what makes dragging work on GNOME). On pure Wayland the overlay
            // stays fully click-through instead.
            pet.x11_shaped_input = x11_backend_available();
            pet.refresh_taskbar_coords();
        }

        App {
            config,
            draft,
            watcher,
            reload_rx,
            tray,
            paused,
            visible,
            hwnd,
            status: "Watching".to_string(),
            last_tray_status: String::new(),
            auto,
            autostart,
            #[cfg(windows)]
            native_pet: Some(native_pet),
            #[cfg(windows)]
            pet_preview: None,
            #[cfg(windows)]
            pet_spec,
            #[cfg(not(windows))]
            pet,
            pet_tx,
            pet_kind: kind,
            stage: if need_prompt {
                Stage::Prompt
            } else {
                Stage::Running
            },
            own_version: CURRENT_VERSION,
            current_version,
            do_install: force_install,
            startup_frame: 0,
            #[cfg(not(windows))]
            settings_skip_at: std::time::Instant::now(),
        }
    }

    /// Copy this binary into the install location and point autostart at it.
    fn run_install(&mut self) {
        match install_self() {
            Ok(target) => {
                log::info!("installed to {target:?}");
                self.auto = auto_launch_for(&target);
                if let Some(auto) = &self.auto {
                    let _ = auto.enable();
                    self.autostart = auto.is_enabled().unwrap_or(true);
                }
                self.status = format!("Installed to {}", target.display());
            }
            Err(err) => self.status = format!("Install failed: {err}"),
        }
    }

    fn ui_prompt(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                ui.heading("watch-folder");
                ui.add_space(10.0);
                match &self.current_version {
                    Some(current) => {
                        ui.label(format!("You have v{current}. This is v{}.", self.own_version));
                        ui.label("Install this version (and start it on login)?");
                    }
                    None => {
                        ui.label(format!("Install watch-folder v{}?", self.own_version));
                        ui.label("Installs to your user folder and starts it on login.");
                    }
                }
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    if ui.button("Install").clicked() {
                        self.do_install = true;
                        self.stage = Stage::Running;
                    }
                    if ui
                        .button("Test run")
                        .on_hover_text("Run without installing")
                        .clicked()
                    {
                        self.do_install = false;
                        self.stage = Stage::Running;
                    }
                    if ui.button("Cancel").clicked() {
                        std::process::exit(0);
                    }
                });
            });
        });
    }

    fn status_label(&self) -> &'static str {
        if !self.watcher.is_running() {
            "Stopped"
        } else if self.paused.load(Ordering::SeqCst) {
            "Paused"
        } else {
            "Watching"
        }
    }

    /// Keep the tray tooltip and the "Status:" menu item in sync.
    fn sync_tray(&mut self) {
        let label = self.status_label();
        let status_text = format!("Status: {label}");
        if status_text != self.last_tray_status {
            self.tray.status_item.set_text(&status_text);
            self.tray.pause_item.set_text(if self.paused.load(Ordering::SeqCst) {
                "Resume watching"
            } else {
                "Pause watching"
            });
            let _ = self.tray.tray.set_tooltip(Some(format!("watch-folder - {label}")));
            self.last_tray_status = status_text;
        }
    }

    fn toggle_pause(&mut self) {
        let now_paused = !self.paused.load(Ordering::SeqCst);
        self.paused.store(now_paused, Ordering::SeqCst);
        self.status = if now_paused {
            "Paused".to_string()
        } else {
            "Watching".to_string()
        };
    }

    fn set_visible(&mut self, visible: bool, ctx: &egui::Context) {
        apply_visibility(visible, ctx, self.hwnd, &self.visible);
    }

    // -- Desktop pet backend abstraction (native layered window on Windows, egui elsewhere) --

    #[cfg(windows)]
    fn pet_shared(&self) -> Option<&crate::pet::native::SharedPet> {
        self.native_pet.as_ref().map(|p| p.shared().as_ref())
    }

    fn pet_enabled(&self) -> bool {
        #[cfg(windows)]
        {
            self.pet_shared().map(|s| s.enabled()).unwrap_or(false)
        }
        #[cfg(not(windows))]
        {
            self.pet.enabled
        }
    }

    fn pet_set_enabled(&mut self, value: bool) {
        #[cfg(windows)]
        if let Some(shared) = self.pet_shared() {
            shared.set_enabled(value);
        }
        #[cfg(not(windows))]
        {
            self.pet.enabled = value;
        }
    }

    fn pet_state(&self) -> crate::pet::PetState {
        #[cfg(windows)]
        {
            self.pet_shared()
                .map(|s| s.state())
                .unwrap_or(crate::pet::PetState::Sleeping)
        }
        #[cfg(not(windows))]
        {
            self.pet.state
        }
    }

    fn pet_frame(&self) -> usize {
        #[cfg(windows)]
        {
            self.pet_shared().map(|s| s.frame()).unwrap_or(0)
        }
        #[cfg(not(windows))]
        {
            self.pet.player.current_frame()
        }
    }

    fn pet_facing_left(&self) -> bool {
        #[cfg(windows)]
        {
            self.pet_shared().map(|s| s.facing_left()).unwrap_or(false)
        }
        #[cfg(not(windows))]
        {
            self.pet.facing_left
        }
    }

    #[cfg(windows)]
    fn pet_is_dragging(&self) -> bool {
        self.pet_shared().map(|s| s.dragging()).unwrap_or(false)
    }

    fn pet_paper_count(&self) -> usize {
        #[cfg(windows)]
        {
            self.pet_shared().map(|s| s.paper_count()).unwrap_or(0)
        }
        #[cfg(not(windows))]
        {
            self.pet.ground_papers.len()
        }
    }

    fn pet_carried_count(&self) -> usize {
        #[cfg(windows)]
        {
            self.pet_shared().map(|s| s.carried_count()).unwrap_or(0)
        }
        #[cfg(not(windows))]
        {
            self.pet.carried_stack.len()
        }
    }

    fn pet_speed(&self) -> f32 {
        #[cfg(windows)]
        {
            self.pet_shared().map(|s| s.speed()).unwrap_or(28.0)
        }
        #[cfg(not(windows))]
        {
            self.pet.speed
        }
    }

    fn pet_set_character(&mut self, kind: crate::pet::character::CharacterKind) {
        self.pet_kind = kind;
        #[cfg(windows)]
        {
            if let Some(pet) = &self.native_pet {
                pet.set_character(kind);
            }
            self.pet_spec = crate::pet::character::CharacterSpec::for_kind(kind);
            self.pet_preview = None; // reload preview textures with the new sheet
        }
        #[cfg(not(windows))]
        {
            self.pet.set_character(kind);
        }
    }

    fn pet_set_speed(&mut self, value: f32) {
        #[cfg(windows)]
        if let Some(shared) = self.pet_shared() {
            shared.set_speed(value);
        }
        #[cfg(not(windows))]
        {
            self.pet.speed = value;
        }
    }

    #[cfg(windows)]
    fn pet_position_offset(&self) -> f32 {
        self.pet_shared()
            .map(|s| s.position_offset())
            .unwrap_or(0.0)
    }

    fn pet_set_position_offset(&mut self, value: f32) {
        #[cfg(windows)]
        if let Some(shared) = self.pet_shared() {
            shared.set_position_offset(value);
        }
        #[cfg(not(windows))]
        {
            self.pet.position_offset = value;
            self.pet.refresh_taskbar_coords();
        }
    }

    fn pet_help_clean(&mut self) {
        #[cfg(windows)]
        if let Some(pet) = &self.native_pet {
            pet.help_clean();
        }
        #[cfg(not(windows))]
        {
            self.pet.help_clean();
        }
    }

    fn pet_say_hi(&mut self, text: &str) {
        #[cfg(windows)]
        if let Some(pet) = &self.native_pet {
            pet.say_hi(text.to_string());
        }
        #[cfg(not(windows))]
        {
            self.pet.say_funny(text, 3.0);
        }
    }

    fn pet_realign(&mut self) {
        #[cfg(windows)]
        if let Some(pet) = &self.native_pet {
            pet.realign();
        }
        #[cfg(not(windows))]
        {
            self.pet.refresh_taskbar_coords();
        }
    }

    fn pet_anim_def(&self) -> crate::pet::character::AnimationDef {
        let spec = self.pet_spec();
        match self.pet_state() {
            crate::pet::PetState::Sleeping => spec.anim_sleep,
            crate::pet::PetState::Grooming => *spec.groom(),
            crate::pet::PetState::Alert => spec.anim_alert,
            crate::pet::PetState::Collecting
            | crate::pet::PetState::WalkingToTray
            | crate::pet::PetState::WalkingHome => spec.anim_walk,
            crate::pet::PetState::Arranging => spec.anim_drop,
            crate::pet::PetState::Stuck => spec.anim_alert,
        }
    }

    fn pet_spec(&self) -> &crate::pet::character::CharacterSpec {
        #[cfg(windows)]
        {
            &self.pet_spec
        }
        #[cfg(not(windows))]
        {
            &self.pet.spec
        }
    }

    #[cfg(windows)]
    fn ensure_pet_preview(&mut self, ctx: &egui::Context) {
        if self.pet_preview.is_none() {
            self.pet_preview = Some(crate::pet::animation::PetTextures::load(ctx, &self.pet_spec));
        }
    }
    #[cfg(not(windows))]
    fn ensure_pet_preview(&mut self, _ctx: &egui::Context) {}

    fn pet_textures(&self) -> Option<&crate::pet::animation::PetTextures> {
        #[cfg(windows)]
        {
            self.pet_preview.as_ref()
        }
        #[cfg(not(windows))]
        {
            self.pet.textures.as_ref()
        }
    }

    fn reload_from_disk(&mut self) {
        let config = config::load_or_create();
        self.watcher.reload(config.clone());
        self.draft = Draft::from_config(&config);
        self.pet_set_enabled(config.pet_enabled);
        self.pet_set_position_offset(config.pet_position_offset);
        self.pet_set_character(config.pet_character);
        self.apply_icon_style_tray_only(config.icon_style);
        self.config = config;
        self.status = "Reloaded config from disk".to_string();
    }

    fn save_and_apply(&mut self) {
        let config = self.draft.to_config();
        match config.save() {
            Ok(()) => {
                self.watcher.reload(config.clone());
                self.pet_set_enabled(config.pet_enabled);
                self.pet_set_position_offset(config.pet_position_offset);
                self.pet_set_character(config.pet_character);
                self.config = config;
                self.status = "Saved and restarted watcher".to_string();
            }
            Err(e) => self.status = format!("Save failed: {e}"),
        }
    }

    fn apply_autostart(&mut self) {
        if let Some(auto) = &self.auto {
            let result = if self.autostart {
                auto.enable()
            } else {
                auto.disable()
            };
            match result {
                Ok(()) => {
                    self.status = if self.autostart {
                        "Autostart enabled".to_string()
                    } else {
                        "Autostart disabled".to_string()
                    }
                }
                Err(e) => {
                    self.status = format!("Autostart error: {e}");
                    self.autostart = !self.autostart;
                }
            }
        }
    }

    fn is_dirty(&self) -> bool {
        serde_yaml::to_string(&self.draft.to_config()).unwrap_or_default()
            != serde_yaml::to_string(&self.config).unwrap_or_default()
    }

    fn ui_status(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("watch-folder");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("v{}", env!("CARGO_PKG_VERSION")));
            });
        });
        ui.add_space(4.0);
        let (label, color) = match self.status_label() {
            "Stopped" => ("Stopped", egui::Color32::from_rgb(220, 60, 60)),
            "Paused" => ("Paused", egui::Color32::from_rgb(220, 160, 0)),
            _ => ("Watching", egui::Color32::from_rgb(40, 160, 80)),
        };
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 4.5, color);
            ui.colored_label(color, label);
            ui.add_space(8.0);
            let btn = if self.paused.load(Ordering::SeqCst) {
                "Resume"
            } else {
                "Pause"
            };
            if ui.button(btn).clicked() {
                self.toggle_pause();
            }
            if ui.button("Reload config").clicked() {
                self.reload_from_disk();
            }
        });
        ui.separator();
    }

    fn ui_watch_dirs(&mut self, ui: &mut egui::Ui) {
        ui.heading("Watched folders");
        ui.label("New files dropped directly in these folders are sorted into category subfolders.");
        ui.add_space(4.0);

        let mut remove = None;
        let mut sort_one: Option<String> = None;
        for (i, w) in self.draft.watch.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut w.path)
                        .desired_width(300.0)
                        .hint_text("~/Downloads  |  C:\\Users\\me\\Downloads"),
                );
                if ui
                    .add_enabled(!w.path.trim().is_empty(), egui::Button::new("Sort now"))
                    .on_hover_text("Sort the files already in this folder")
                    .clicked()
                {
                    sort_one = Some(w.path.clone());
                }
                if ui.button("Remove").clicked() {
                    remove = Some(i);
                }
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut w.custom, "Use custom categories for this folder");
            });
            if w.custom {
                ui.indent(("watch-cats", i), |ui| category_editor(ui, &mut w.categories));
            }
            ui.add_space(6.0);
        }
        if let Some(i) = remove {
            self.draft.watch.remove(i);
        }

        ui.horizontal(|ui| {
            if ui.button("+ Add folder").clicked() {
                self.draft.watch.push(DraftWatch::empty());
            }
            if ui.button("Sort all folders now").clicked() {
                let config = self.draft.to_config();
                let n = crate::watcher::sort_all(&config);
                self.status = format!("Queued {n} file(s) from all folders");
            }
        });

        if let Some(path) = sort_one {
            let config = self.draft.to_config();
            let n = crate::watcher::sort_folder(&config, &path);
            self.status = format!("Queued {n} file(s) from {path}");
        }
        ui.separator();
    }

    fn ui_categories(&mut self, ui: &mut egui::Ui) {
        ui.heading("Default categories");
        ui.label("Extensions are comma-separated; matching is case-insensitive.");
        ui.add_space(4.0);
        category_editor(ui, &mut self.draft.categories);
        ui.separator();
    }

    fn ui_ignore(&mut self, ui: &mut egui::Ui) {
        ui.heading("Ignored extensions");
        ui.label("Files with these extensions (partial/temp downloads) are never moved.");
        ui.add_space(2.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.draft.ignore)
                .desired_width(f32::INFINITY)
                .hint_text("crdownload, part, tmp"),
        );
        ui.separator();
    }

    fn ui_stability(&mut self, ui: &mut egui::Ui) {
        ui.heading("Stability");
        ui.label("A file is moved only after its size holds steady for the given ticks.");
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.label("Poll interval (ms)");
            ui.add(egui::DragValue::new(&mut self.draft.interval_ms).range(50..=60_000));
            ui.add_space(16.0);
            ui.label("Stable ticks");
            ui.add(egui::DragValue::new(&mut self.draft.required_ticks).range(1..=60));
        });
        ui.separator();
    }

    fn ui_startup(&mut self, ui: &mut egui::Ui) {
        ui.heading("Startup");
        if self.auto.is_some() {
            if ui
                .checkbox(&mut self.autostart, "Start automatically on login")
                .changed()
            {
                self.apply_autostart();
            }
        } else {
            ui.label("Autostart is not available on this platform.");
        }
    }

    fn apply_icon_style_tray_only(&mut self, style: crate::config::IconStyle) {
        let (rgba_tray, tw, th) = icon_rgba(style, 128);
        if let Ok(tray_icon) = tray_icon::Icon::from_rgba(rgba_tray, tw, th) {
            let _ = self.tray.tray.set_icon(Some(tray_icon));
        }
    }

    fn apply_icon_style(&mut self, style: crate::config::IconStyle, ctx: &egui::Context) {
        self.apply_icon_style_tray_only(style);
        install_desktop_entry(style);
        let (rgba_window, ww, wh) = icon_rgba(style, 128);
        let icon_data = egui::IconData {
            rgba: rgba_window,
            width: ww,
            height: wh,
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(std::sync::Arc::new(icon_data))));
    }

    fn ui_icon_style(&mut self, ui: &mut egui::Ui) {
        ui.heading("App & Tray Icon");
        ui.label("Choose the retro companion style for the system tray and window:");
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.label("Icon Style:");
            let mut selected = self.draft.icon_style;
            egui::ComboBox::from_id_salt("icon_style_picker")
                .selected_text(selected.display_name())
                .show_ui(ui, |ui| {
                    for style in crate::config::IconStyle::ALL {
                        ui.selectable_value(&mut selected, style, style.display_name());
                    }
                });
            if selected != self.draft.icon_style {
                self.draft.icon_style = selected;
                self.apply_icon_style(selected, ui.ctx());
                ui.ctx().request_repaint();
            }
        });
    }

    fn ui_pet(&mut self, ui: &mut egui::Ui) {
        ui.heading("Desktop Companion (Mochi Slime)");
        ui.label("A friendly desktop companion that strolls along your taskbar, picks up new files, and files them into your directory.");
        ui.add_space(4.0);

        if ui
            .checkbox(&mut self.draft.pet_enabled, "Enable desktop pet companion")
            .changed()
        {
            let enabled = self.draft.pet_enabled;
            self.pet_set_enabled(enabled);
        }

        if self.pet_enabled() {
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Companion:");
                let mut selected = self.pet_kind;
                egui::ComboBox::from_id_salt("pet_character_picker")
                    .selected_text(selected.display_name())
                    .show_ui(ui, |ui| {
                        for kind in crate::pet::character::CharacterKind::ALL {
                            ui.selectable_value(&mut selected, kind, kind.display_name());
                        }
                    });
                if selected != self.pet_kind {
                    self.pet_set_character(selected);
                    self.draft.pet_character = selected;
                    ui.ctx().request_repaint();
                }
            });
            ui.add_space(4.0);

            let (state_str, state_color) = match self.pet_state() {
                crate::pet::PetState::Sleeping => ("Sleeping near folder zZz", egui::Color32::from_rgb(150, 180, 220)),
                crate::pet::PetState::Grooming => ("Grooming its fur...", egui::Color32::from_rgb(200, 170, 230)),
                crate::pet::PetState::Alert => ("Alert! Noticed new file!", egui::Color32::from_rgb(255, 205, 50)),
                crate::pet::PetState::Collecting => ("Collecting fluttering papers...", egui::Color32::from_rgb(100, 210, 140)),
                crate::pet::PetState::WalkingToTray => ("Carrying papers to folder...", egui::Color32::from_rgb(100, 210, 140)),
                crate::pet::PetState::Arranging => ("Sorting files into folder!", egui::Color32::from_rgb(80, 225, 120)),
                crate::pet::PetState::WalkingHome => ("Returning home to desk...", egui::Color32::from_rgb(150, 180, 220)),
                crate::pet::PetState::Stuck => ("Stuck! Blocked by files near folder entrance!", egui::Color32::from_rgb(255, 140, 50)),
            };

            self.ensure_pet_preview(ui.ctx());
            let anim = self.pet_anim_def();
            let frame = self.pet_frame();
            let facing = self.pet_facing_left();
            let preview = self.pet_textures().map(|textures| {
                (
                    textures.character.id(),
                    textures.char_uv(self.pet_spec(), &anim, frame, facing),
                )
            });
            let speed_text = self.pet_speed();

            ui.horizontal(|ui| {
                if let Some((tex_id, uv)) = preview {
                    let img = egui::Image::from_texture(egui::load::SizedTexture::new(
                        tex_id,
                        egui::vec2(36.0, 36.0),
                    ))
                    .uv(uv);
                    ui.add(img);
                }
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label("Status:");
                        ui.colored_label(state_color, state_str);
                    });
                    ui.label(format!("Speed: {:.0} px/s", speed_text));
                });
            });

            ui.add_space(4.0);
            let mut speed = speed_text;
            ui.horizontal(|ui| {
                ui.label("Stroll speed:");
                if ui
                    .add(egui::Slider::new(&mut speed, 15.0..=60.0).suffix(" px/s"))
                    .changed()
                {
                    self.pet_set_speed(speed);
                }
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Taskbar position:");
                if ui
                    .add(
                        egui::Slider::new(&mut self.draft.pet_position_offset, -1000.0..=300.0)
                            .suffix(" px")
                            .text("Left <-> Right"),
                    )
                    .on_hover_text("Shift Mochi and the folder desk together along the taskbar")
                    .changed()
                {
                    let offset = self.draft.pet_position_offset;
                    self.pet_set_position_offset(offset);
                    ui.ctx().request_repaint();
                }
            });
            ui.label(
                egui::RichText::new("Tip: You can also drag & drop Mochi and the folder directly on the taskbar when Mochi is sleeping!")
                    .italics()
                    .size(12.0)
                    .color(egui::Color32::from_rgb(160, 180, 210)),
            );

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .button("Drop test paper")
                    .on_hover_text("Spawn a falling paper on the desktop to test Mochi")
                    .clicked()
                {
                    let _ = self.pet_tx.send(crate::pet::PetEvent::NewFile("sample_notes.pdf".to_string()));
                    ui.ctx().request_repaint();
                }
                if ui
                    .button("Help Mochi clean")
                    .on_hover_text("Clear all papers and celebrate with Mochi")
                    .clicked()
                {
                    self.pet_help_clean();
                    ui.ctx().request_repaint();
                }
                if ui
                    .button("Say hi")
                    .on_hover_text("Make Mochi say a friendly line")
                    .clicked()
                {
                    self.pet_say_hi("yoo bro! ready to sort files!");
                    ui.ctx().request_repaint();
                }
                if ui
                    .button("Realign to taskbar")
                    .on_hover_text("Refresh screen and taskbar bounds")
                    .clicked()
                {
                    self.pet_realign();
                    ui.ctx().request_repaint();
                }
            });
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let frame_start = std::time::Instant::now();
        #[allow(unused_mut)]
        let mut pet_update_ms = 0.0_f64;
        #[allow(unused_mut)]
        let mut pet_render_ms = 0.0_f64;
        if self.stage == Stage::Prompt {
            self.ui_prompt(ctx);
            return;
        }

        // eframe always shows the root window once, after the first painted
        // frame, even when it was created with `with_visible(false)`. On a
        // hidden autostart launch that leaves a blank, transparent window on
        // screen. That forced show also makes winit re-apply its cached window
        // styles, so we must park the window again on the following frame.
        self.startup_frame = self.startup_frame.saturating_add(1);
        if !self.visible.load(Ordering::SeqCst) && self.startup_frame <= 2 {
            // Park immediately (avoids a one-frame transparent flash) and again
            // on the next frame to undo the style reset from eframe's forced show.
            self.set_visible(false, ctx);
            if self.startup_frame == 1 {
                ctx.request_repaint();
            }
        }

        // The very first hide runs before the window manager has mapped the
        // settings window, so its skip-taskbar request is lost and the dock
        // keeps showing the app. Re-assert it periodically while hidden (the
        // pet window is re-styled the same way in `refresh_taskbar`).
        #[cfg(not(windows))]
        if !self.visible.load(Ordering::SeqCst)
            && (self.startup_frame <= 120
                || self.settings_skip_at.elapsed() >= Duration::from_secs(2))
        {
            self.settings_skip_at = std::time::Instant::now();
            crate::pet::taskbar::x11_set_skip_taskbar("watch-folder", true);
            crate::pet::taskbar::x11_set_decorations("watch-folder", false);
        }

        // Re-assert the window icon explicitly. Some backends (notably X11)
        // do not apply the icon passed to the initial viewport builder, which
        // leaves the taskbar/dock entry showing an empty icon.
        if self.startup_frame == 1 {
            self.apply_icon_style(self.config.icon_style, ctx);
        }

        if self.do_install {
            self.do_install = false;
            self.run_install();
        }

        // "Reload config" is the one action that needs our own state; the rest
        // are handled directly in the tray event handler.
        if self.reload_rx.try_recv().is_ok() {
            self.reload_from_disk();
        }

        // Closing the window hides it to the tray instead of quitting.
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.set_visible(false, ctx);
        }

        self.sync_tray();

        // Update and render desktop companion.
        //
        // On Windows the pet lives in its own native layered window on a
        // dedicated thread (see `pet::native`), so eframe must not draw it. We
        // only repaint the settings UI while it is visible to animate the small
        // companion preview, and mirror any desktop drag position into the draft.
        #[cfg(windows)]
        {
            if self.pet_is_dragging() {
                self.draft.pet_position_offset = self.pet_position_offset();
            }
            if self.visible.load(Ordering::SeqCst) && self.pet_enabled() {
                // Gentle repaint for the small companion preview; the real pet
                // animates natively. Keep this slow to avoid idle CPU churn.
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }

        #[cfg(not(windows))]
        {
            if self.pet.enabled {
                let pet_start = std::time::Instant::now();
                let delay = self.pet.update();
                pet_update_ms = pet_start.elapsed().as_secs_f64() * 1000.0;
                // Target a 60fps period for the smooth sub-pixel pet motion.
                // Do NOT add egui's predicted frame time here: on Linux that
                // doubles the requested period and halves the frame rate to
                // ~30fps, which reads as choppy.
                let budget = Duration::from_secs_f32(1.0 / 60.0);
                ctx.request_repaint_after(delay.max(budget));
                let render_start = std::time::Instant::now();
                self.pet.render(ctx);
                pet_render_ms = render_start.elapsed().as_secs_f64() * 1000.0;

                // Sync dragged position from desktop to draft settings
                if self.pet.is_dragging_desk {
                    self.draft.pet_position_offset = self.pet.position_offset;
                }
            }
        }

        let is_visible = self.visible.load(Ordering::SeqCst);
        if is_visible {
            egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(&self.status);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Revert").clicked() {
                            self.draft = Draft::from_config(&self.config);
                            self.pet_set_enabled(self.config.pet_enabled);
                            self.pet_set_position_offset(self.config.pet_position_offset);
                            self.pet_set_character(self.config.pet_character);
                            self.status = "Reverted unsaved changes".to_string();
                        }
                        let dirty = self.is_dirty();
                        if ui
                            .add_enabled(dirty, egui::Button::new("Save & apply"))
                            .clicked()
                        {
                            self.save_and_apply();
                        }
                    });
                });
            });

            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.ui_status(ui);
                    ui.add_space(4.0);
                    ui.group(|ui| self.ui_pet(ui));
                    ui.add_space(6.0);
                    ui.group(|ui| self.ui_watch_dirs(ui));
                    ui.add_space(6.0);
                    ui.group(|ui| self.ui_categories(ui));
                    ui.add_space(6.0);
                    ui.group(|ui| self.ui_ignore(ui));
                    ui.add_space(6.0);
                    ui.group(|ui| self.ui_stability(ui));
                    ui.add_space(6.0);
                    ui.group(|ui| self.ui_startup(ui));
                    ui.add_space(6.0);
                    ui.group(|ui| self.ui_icon_style(ui));
                });
            });
        }

        record_frame(
            self,
            frame_start.elapsed().as_secs_f64() * 1000.0,
            pet_update_ms,
            pet_render_ms,
        );
    }
}

fn category_editor(ui: &mut egui::Ui, pairs: &mut Vec<(String, String)>) {
    let mut remove = None;
    for (i, (name, exts)) in pairs.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(name)
                    .desired_width(120.0)
                    .hint_text("category"),
            );
            ui.label("=>");
            ui.add(
                egui::TextEdit::singleline(exts)
                    .desired_width(340.0)
                    .hint_text("pdf, doc, txt"),
            );
            if ui.small_button("Remove").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        pairs.remove(i);
    }
    if ui.small_button("+ Add category").clicked() {
        pairs.push((String::new(), String::new()));
    }
}

#[derive(Clone)]
struct DraftWatch {
    path: String,
    custom: bool,
    categories: Vec<(String, String)>,
}

impl DraftWatch {
    fn empty() -> Self {
        DraftWatch {
            path: String::new(),
            custom: false,
            categories: vec![(String::new(), String::new())],
        }
    }
}

struct Draft {
    watch: Vec<DraftWatch>,
    categories: Vec<(String, String)>,
    ignore: String,
    interval_ms: u64,
    required_ticks: u32,
    pet_enabled: bool,
    pet_position_offset: f32,
    pet_character: crate::pet::character::CharacterKind,
    icon_style: crate::config::IconStyle,
}

impl Draft {
    fn from_config(config: &Config) -> Self {
        let default_categories = map_to_pairs(&config.file_types);
        Draft {
            watch: config
                .watch
                .iter()
                .map(|w| DraftWatch {
                    path: w.path.clone(),
                    custom: w.file_types.is_some(),
                    categories: w
                        .file_types
                        .as_ref()
                        .map(map_to_pairs)
                        .unwrap_or_else(|| default_categories.clone()),
                })
                .collect(),
            categories: default_categories,
            ignore: config.ignore_extensions.join(", "),
            interval_ms: config.stability.interval_ms,
            required_ticks: config.stability.required_stable_ticks,
            pet_enabled: config.pet_enabled,
            pet_position_offset: config.pet_position_offset,
            pet_character: config.pet_character,
            icon_style: config.icon_style,
        }
    }

    fn to_config(&self) -> Config {
        Config {
            watch: self
                .watch
                .iter()
                .filter(|w| !w.path.trim().is_empty())
                .map(|w| WatchEntry {
                    path: w.path.trim().to_string(),
                    file_types: if w.custom {
                        Some(pairs_to_map(&w.categories))
                    } else {
                        None
                    },
                })
                .collect(),
            file_types: pairs_to_map(&self.categories),
            ignore_extensions: split_list(&self.ignore),
            stability: Stability {
                interval_ms: self.interval_ms,
                required_stable_ticks: self.required_ticks,
            },
            pet_enabled: self.pet_enabled,
            pet_position_offset: self.pet_position_offset,
            pet_character: self.pet_character,
            icon_style: self.icon_style,
        }
    }
}

fn map_to_pairs(map: &BTreeMap<String, Vec<String>>) -> Vec<(String, String)> {
    map.iter()
        .map(|(k, v)| (k.clone(), v.join(", ")))
        .collect()
}

fn pairs_to_map(pairs: &[(String, String)]) -> BTreeMap<String, Vec<String>> {
    pairs
        .iter()
        .filter(|(k, _)| !k.trim().is_empty())
        .map(|(k, v)| (k.trim().to_string(), split_list(v)))
        .collect()
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

fn build_auto_launch() -> Option<AutoLaunch> {
    let installed = install_path();
    let target = if installed.exists() {
        installed
    } else {
        std::env::current_exe().unwrap_or(installed)
    };
    auto_launch_for(&target)
}

/// Write the icon and a matching desktop entry for the current user.
///
/// GNOME's dock/taskbar matches a window to a `.desktop` file by app id /
/// WM_CLASS and shows *that* file's icon; the window's own icon is ignored. So
/// without this the entry falls back to a generic gear icon. This is
/// idempotent and cheap, and is re-run whenever the icon style changes.
#[cfg(target_os = "linux")]
fn install_desktop_entry(style: crate::config::IconStyle) {
    let Some(home) = home::home_dir() else {
        return;
    };
    let icon_dir = home.join(".local/share/watch-folder");
    let app_dir = home.join(".local/share/applications");
    if std::fs::create_dir_all(&icon_dir).is_err() || std::fs::create_dir_all(&app_dir).is_err() {
        return;
    }

    let icon_bytes: &[u8] = match style {
        crate::config::IconStyle::Calico => include_bytes!("../assets/icon_calico.png"),
        crate::config::IconStyle::Monochrome => include_bytes!("../assets/icon_mono.png"),
    };
    let icon_path = icon_dir.join("icon.png");
    if std::fs::write(&icon_path, icon_bytes).is_err() {
        return;
    }

    let exec = std::env::current_exe().unwrap_or_default();
    let desktop = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.0\n\
         Name=watch-folder\n\
         Comment=Sort new files into folders\n\
         Exec=\"{}\"\n\
         Icon={}\n\
         Terminal=false\n\
         Categories=Utility;\n\
         StartupWMClass=watch-folder\n",
        exec.display(),
        icon_path.display()
    );
    let _ = std::fs::write(app_dir.join("watch-folder.desktop"), desktop);
}

#[cfg(not(target_os = "linux"))]
fn install_desktop_entry(_style: crate::config::IconStyle) {}

/// Load the chosen bundled retro icon PNG and return it as raw RGBA at the requested size.
fn icon_rgba(style: crate::config::IconStyle, size: u32) -> (Vec<u8>, u32, u32) {
    let bytes: &[u8] = match style {
        crate::config::IconStyle::Calico => include_bytes!("../assets/icon_calico.png"),
        crate::config::IconStyle::Monochrome => include_bytes!("../assets/icon_mono.png"),
    };
    let img = image::load_from_memory(bytes)
        .expect("bundled icon is invalid")
        .resize_exact(size, size, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let (w, h) = img.dimensions();
    (img.into_raw(), w, h)
}
