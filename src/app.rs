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

/// Start the tray application with its egui settings window.
///
/// Returns an error if the tray icon or the windowing system is unavailable,
/// so the caller can fall back to headless mode.
pub fn run() -> Result<(), String> {
    // Only one instance may run. A newer binary replaces an older running one;
    // an equal/older binary just asks the running one to show its window.
    let listener = match acquire_instance() {
        InstanceOutcome::Primary(listener) => listener,
        InstanceOutcome::AlreadyRunning => return Ok(()),
    };

    let config = config::load_or_create();
    let paused = Arc::new(AtomicBool::new(false));
    let visible = Arc::new(AtomicBool::new(true));

    let (pet_tx, pet_rx) = channel::<crate::pet::PetEvent>();
    let watcher = Watcher::start_with(config.clone(), paused.clone(), Some(pet_tx.clone()));

    let (reload_tx, reload_rx) = channel::<()>();

    let (rgba, width, height) = icon_rgba(64);
    let viewport_icon = egui::IconData {
        rgba: rgba.clone(),
        width,
        height,
    };

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("watch-folder")
            .with_inner_size([640.0, 700.0])
            .with_min_inner_size([480.0, 420.0])
            .with_icon(viewport_icon)
            .with_transparent(true),
        ..Default::default()
    };

    eframe::run_native(
        "watch-folder",
        native_options,
        Box::new(move |cc| {
            configure_style(&cc.egui_ctx);
            let hwnd = window_handle_isize(cc);
            let tray = create_tray(rgba, width, height)?;

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
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(show));
        if show {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }
    #[cfg(windows)]
    {
        if show {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }
    ctx.request_repaint();
}

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
        SW_HIDE, SW_RESTORE, SW_SHOW, WS_EX_TOOLWINDOW,
    };

    static SAVED_X: AtomicI32 = AtomicI32::new(100);
    static SAVED_Y: AtomicI32 = AtomicI32::new(100);
    static SAVED_W: AtomicI32 = AtomicI32::new(640);
    static SAVED_H: AtomicI32 = AtomicI32::new(700);
    static HAS_SAVED: AtomicBool = AtomicBool::new(false);

    let hwnd = hwnd as HWND;
    unsafe {
        if show {
            // Restore normal ex-style (not toolwindow, so it appears in taskbar when open)
            ShowWindow(hwnd, SW_HIDE as i32);
            let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
            SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style & !(WS_EX_TOOLWINDOW as i32));

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

            // Set as toolwindow so it does not appear on taskbar or alt-tab when shown
            let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
            SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style | (WS_EX_TOOLWINDOW as i32));

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
    let mut buf = [0u8; 128];
    let n = stream.read(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf[..n]);
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
            let mut buf = [0u8; 128];
            let n = stream.read(&mut buf).unwrap_or(0);
            let text = String::from_utf8_lossy(&buf[..n]);
            match text.trim() {
                command if command.starts_with("HELLO") => {
                    let _ = writeln!(stream, "VERSION {CURRENT_VERSION}");
                }
                "SHOW" => {
                    apply_visibility(true, &ctx, hwnd, &visible);
                }
                "REPLACE" => {
                    log::info!("a newer instance is taking over; exiting");
                    std::process::exit(0);
                }
                _ => {}
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
    pet: crate::pet::PetController,
    pet_tx: Sender<crate::pet::PetEvent>,
}

impl App {
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

        let mut pet = crate::pet::PetController::new(Some(pet_rx));
        pet.enabled = config.pet_enabled;
        pet.position_offset = config.pet_position_offset;
        pet.refresh_taskbar_coords();

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
            pet,
            pet_tx,
        }
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

    fn reload_from_disk(&mut self) {
        let config = config::load_or_create();
        self.watcher.reload(config.clone());
        self.draft = Draft::from_config(&config);
        self.pet.enabled = config.pet_enabled;
        self.pet.position_offset = config.pet_position_offset;
        self.pet.refresh_taskbar_coords();
        self.config = config;
        self.status = "Reloaded config from disk".to_string();
    }

    fn save_and_apply(&mut self) {
        let config = self.draft.to_config();
        match config.save() {
            Ok(()) => {
                self.watcher.reload(config.clone());
                self.pet.enabled = config.pet_enabled;
                self.pet.position_offset = config.pet_position_offset;
                self.pet.refresh_taskbar_coords();
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

    fn ui_pet(&mut self, ui: &mut egui::Ui) {
        ui.heading("Desktop Companion (Mochi Slime)");
        ui.label("A friendly desktop companion that strolls along your taskbar, picks up new files, and files them into your directory.");
        ui.add_space(4.0);

        if ui
            .checkbox(&mut self.draft.pet_enabled, "Enable desktop pet companion")
            .changed()
        {
            self.pet.enabled = self.draft.pet_enabled;
        }

        if self.pet.enabled {
            ui.add_space(4.0);
            let (state_str, state_color) = match self.pet.state {
                crate::pet::PetState::Sleeping => ("Sleeping near folder zZz", egui::Color32::from_rgb(150, 180, 220)),
                crate::pet::PetState::Alert => ("Alert! Noticed new file!", egui::Color32::from_rgb(255, 205, 50)),
                crate::pet::PetState::Collecting => ("Collecting fluttering papers...", egui::Color32::from_rgb(100, 210, 140)),
                crate::pet::PetState::WalkingToTray => ("Carrying papers to folder...", egui::Color32::from_rgb(100, 210, 140)),
                crate::pet::PetState::Arranging => ("Sorting files into folder!", egui::Color32::from_rgb(80, 225, 120)),
                crate::pet::PetState::WalkingHome => ("Returning home to desk...", egui::Color32::from_rgb(150, 180, 220)),
                crate::pet::PetState::Stuck => ("Stuck! Blocked by files near folder entrance!", egui::Color32::from_rgb(255, 140, 50)),
            };

            ui.horizontal(|ui| {
                if let Some(ref textures) = self.pet.textures {
                    let frame = self.pet.player.current_frame();
                    let anim_def = match self.pet.state {
                        crate::pet::PetState::Sleeping => &self.pet.spec.anim_sleep,
                        crate::pet::PetState::Alert => &self.pet.spec.anim_alert,
                        crate::pet::PetState::Collecting | crate::pet::PetState::WalkingToTray | crate::pet::PetState::WalkingHome => &self.pet.spec.anim_walk,
                        crate::pet::PetState::Arranging => &self.pet.spec.anim_drop,
                        crate::pet::PetState::Stuck => &self.pet.spec.anim_alert,
                    };
                    let uv = textures.char_uv(&self.pet.spec, anim_def, frame, self.pet.facing_left);
                    let img = egui::Image::from_texture(egui::load::SizedTexture::new(
                        textures.character.id(),
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
                    ui.label(format!("Speed: {:.0} px/s", self.pet.speed));
                });
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Stroll speed:");
                ui.add(egui::Slider::new(&mut self.pet.speed, 15.0..=60.0).suffix(" px/s"));
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
                    self.pet.position_offset = self.draft.pet_position_offset;
                    self.pet.refresh_taskbar_coords();
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
                    self.pet.help_clean();
                    ui.ctx().request_repaint();
                }
                if ui
                    .button("Say hi")
                    .on_hover_text("Make Mochi say a friendly line")
                    .clicked()
                {
                    self.pet.say_funny("yoo bro! ready to sort files!", 3.0);
                    ui.ctx().request_repaint();
                }
                if ui
                    .button("Realign to taskbar")
                    .on_hover_text("Refresh screen and taskbar bounds")
                    .clicked()
                {
                    self.pet.refresh_taskbar_coords();
                    ui.ctx().request_repaint();
                }
            });
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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

        // Update and render desktop companion
        if self.pet.enabled {
            let delay = self.pet.update();
            ctx.request_repaint_after(delay);
            self.pet.render(ctx);

            // Sync dragged position from desktop to draft settings
            if self.pet.is_dragging_desk {
                self.draft.pet_position_offset = self.pet.position_offset;
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
                            self.pet.enabled = self.config.pet_enabled;
                            self.pet.position_offset = self.config.pet_position_offset;
                            self.pet.refresh_taskbar_coords();
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
                });
            });
        }
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
    let exe = std::env::current_exe().ok()?;
    let path = exe.to_string_lossy().to_string();

    let mut builder = AutoLaunchBuilder::new();
    builder.set_app_name("watch-folder").set_app_path(&path);
    #[cfg(target_os = "linux")]
    builder.set_linux_launch_mode(LinuxLaunchMode::XdgAutostart);

    builder
        .build()
        .map_err(|e| log::warn!("autostart unavailable: {e}"))
        .ok()
}

/// Draw a simple folder icon so no binary asset is needed.
fn icon_rgba(size: u32) -> (Vec<u8>, u32, u32) {
    let s = size as f32;
    let mut px = vec![0u8; (size * size * 4) as usize];

    let (bx0, by0, bx1, by1) = (s * 0.12, s * 0.28, s * 0.88, s * 0.82);
    let (tx0, ty0, tx1, ty1) = (s * 0.12, s * 0.18, s * 0.48, s * 0.34);

    for y in 0..size {
        for x in 0..size {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let body = in_rounded(fx, fy, bx0, by0, bx1, by1, s * 0.07);
            let tab = in_rounded(fx, fy, tx0, ty0, tx1, ty1, s * 0.05);
            if body || tab {
                let idx = ((y * size + x) * 4) as usize;
                px[idx] = 59;
                px[idx + 1] = 130;
                px[idx + 2] = 246;
                px[idx + 3] = 255;
            }
        }
    }
    (px, size, size)
}

fn in_rounded(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> bool {
    let cx = x.clamp(x0 + r, x1 - r);
    let cy = y.clamp(y0 + r, y1 - r);
    let dx = x - cx;
    let dy = y - cy;
    dx * dx + dy * dy <= r * r
}
