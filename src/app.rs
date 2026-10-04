use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

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

#[derive(Debug, Clone, Copy)]
enum MenuAction {
    ToggleWindow,
    TogglePause,
    Reload,
    OpenConfig,
    OpenLogs,
    Quit,
}

fn action_for(id: &str) -> Option<MenuAction> {
    match id {
        ID_SHOW => Some(MenuAction::ToggleWindow),
        ID_PAUSE => Some(MenuAction::TogglePause),
        ID_RELOAD => Some(MenuAction::Reload),
        ID_OPEN_CONFIG => Some(MenuAction::OpenConfig),
        ID_OPEN_LOGS => Some(MenuAction::OpenLogs),
        ID_QUIT => Some(MenuAction::Quit),
        _ => None,
    }
}

/// Start the tray application with its egui settings window.
///
/// Returns an error if the tray icon or the windowing system is unavailable,
/// so the caller can fall back to headless mode.
pub fn run() -> Result<(), String> {
    let config = config::load_or_create();
    let watcher = Watcher::start(config.clone());

    let (tx, rx) = channel::<MenuAction>();
    let ctx_holder: Arc<Mutex<Option<egui::Context>>> = Arc::new(Mutex::new(None));

    // Forward tray menu clicks to the UI thread and wake it up.
    {
        let tx = tx;
        let ctx_holder = ctx_holder.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Some(action) = action_for(&event.id.0) {
                let _ = tx.send(action);
            }
            if let Some(ctx) = ctx_holder.lock().unwrap().as_ref() {
                ctx.request_repaint();
            }
        }));
    }

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
            .with_icon(viewport_icon),
        ..Default::default()
    };

    eframe::run_native(
        "watch-folder",
        native_options,
        Box::new(move |cc| {
            *ctx_holder.lock().unwrap() = Some(cc.egui_ctx.clone());
            let tray = create_tray(rgba, width, height)?;
            Ok(Box::new(App::new(config, watcher, rx, tray)))
        }),
    )
    .map_err(|e| e.to_string())
}

fn create_tray(
    rgba: Vec<u8>,
    width: u32,
    height: u32,
) -> Result<TrayIcon, Box<dyn std::error::Error + Send + Sync>> {
    let menu = Menu::new();
    menu.append(&MenuItem::with_id(
        MenuId::new(ID_SHOW),
        "Show / hide settings",
        true,
        None,
    ))?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::with_id(
        MenuId::new(ID_PAUSE),
        "Pause / resume",
        true,
        None,
    ))?;
    menu.append(&MenuItem::with_id(
        MenuId::new(ID_RELOAD),
        "Reload config",
        true,
        None,
    ))?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::with_id(
        MenuId::new(ID_OPEN_CONFIG),
        "Open config file",
        true,
        None,
    ))?;
    menu.append(&MenuItem::with_id(
        MenuId::new(ID_OPEN_LOGS),
        "Open logs",
        true,
        None,
    ))?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::with_id(
        MenuId::new(ID_QUIT),
        "Quit",
        true,
        None,
    ))?;

    let icon = tray_icon::Icon::from_rgba(rgba, width, height)?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("watch-folder")
        .with_icon(icon)
        .build()?;
    Ok(tray)
}

struct App {
    config: Config,
    draft: Draft,
    watcher: Watcher,
    rx: Receiver<MenuAction>,
    _tray: TrayIcon,
    visible: bool,
    quitting: bool,
    status: String,
    auto: Option<AutoLaunch>,
    autostart: bool,
}

impl App {
    fn new(config: Config, watcher: Watcher, rx: Receiver<MenuAction>, tray: TrayIcon) -> App {
        let draft = Draft::from_config(&config);
        let auto = build_auto_launch();
        let autostart = auto
            .as_ref()
            .and_then(|a| a.is_enabled().ok())
            .unwrap_or(false);

        App {
            config,
            draft,
            watcher,
            rx,
            _tray: tray,
            visible: true,
            quitting: false,
            status: "Watching".to_string(),
            auto,
            autostart,
        }
    }

    fn handle_action(&mut self, action: MenuAction, ctx: &egui::Context) {
        match action {
            MenuAction::ToggleWindow => self.set_visible(!self.visible, ctx),
            MenuAction::TogglePause => self.toggle_pause(),
            MenuAction::Reload => self.reload_from_disk(),
            MenuAction::OpenConfig => {
                if let Err(e) = opener::open(config::config_path()) {
                    self.status = format!("Could not open config: {e}");
                }
            }
            MenuAction::OpenLogs => {
                if let Err(e) = opener::open(config::log_path()) {
                    self.status = format!("Could not open logs: {e}");
                }
            }
            MenuAction::Quit => {
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn set_visible(&mut self, visible: bool, ctx: &egui::Context) {
        self.visible = visible;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(visible));
        if visible {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    fn toggle_pause(&mut self) {
        if self.watcher.is_paused() {
            self.watcher.resume();
            self.status = "Watching".to_string();
        } else {
            self.watcher.pause();
            self.status = "Paused".to_string();
        }
    }

    fn reload_from_disk(&mut self) {
        let config = config::load_or_create();
        self.watcher.reload(config.clone());
        self.draft = Draft::from_config(&config);
        self.config = config;
        self.status = "Reloaded config from disk".to_string();
    }

    fn save_and_apply(&mut self) {
        let config = self.draft.to_config();
        match config.save() {
            Ok(()) => {
                self.watcher.reload(config.clone());
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
        ui.heading("watch-folder");
        ui.add_space(2.0);
        let (label, color) = if !self.watcher.is_running() {
            ("Stopped", egui::Color32::RED)
        } else if self.watcher.is_paused() {
            ("Paused", egui::Color32::from_rgb(220, 160, 0))
        } else {
            ("Watching", egui::Color32::from_rgb(40, 160, 80))
        };
        ui.horizontal(|ui| {
            ui.colored_label(color, format!("● {label}"));
            if ui
                .button(if self.watcher.is_paused() {
                    "Resume"
                } else {
                    "Pause"
                })
                .clicked()
            {
                self.toggle_pause();
            }
        });
        ui.separator();
    }

    fn ui_watch_dirs(&mut self, ui: &mut egui::Ui) {
        ui.heading("Watched folders");
        ui.label("New files dropped directly in these folders are sorted into category subfolders.");
        ui.add_space(4.0);

        let mut remove = None;
        for (i, w) in self.draft.watch.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut w.path)
                        .desired_width(340.0)
                        .hint_text("~/Downloads  |  C:\\Users\\me\\Downloads"),
                );
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
            ui.add_space(4.0);
        }
        if let Some(i) = remove {
            self.draft.watch.remove(i);
        }
        if ui.button("+ Add folder").clicked() {
            self.draft.watch.push(DraftWatch::empty());
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
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(action) = self.rx.try_recv() {
            self.handle_action(action, ctx);
        }

        // Closing the window hides it to the tray instead of quitting.
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.set_visible(false, ctx);
        }

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Revert").clicked() {
                        self.draft = Draft::from_config(&self.config);
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
                self.ui_watch_dirs(ui);
                self.ui_categories(ui);
                self.ui_ignore(ui);
                self.ui_stability(ui);
                self.ui_startup(ui);
            });
        });
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
            ui.label("→");
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
