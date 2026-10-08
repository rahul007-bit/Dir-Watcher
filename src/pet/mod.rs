//! Desktop companion (Pet) controller and state machine.
#![allow(dead_code)]

pub mod animation;
pub mod character;
pub mod taskbar;

#[cfg(windows)]
pub mod native;

// TODO(linux): provide a native per-pixel-alpha overlay so the pet is genuinely
// transparent and cheap, like the Windows `native` backend. Options:
//   * X11: an override-redirect window with a 32-bit ARGB visual (compositor),
//     no decorations, `_NET_WM_WINDOW_TYPE_DOCK`.
//   * Wayland: a `wlr-layer-shell` surface in the overlay layer.
// Until then Linux uses the egui immediate-viewport fallback below.
//
// TODO(macos): provide a native overlay too: an `NSWindow` with
// `isOpaque = NO`, `backgroundColor = clear`, `ignoresMouseEvents = YES`,
// level = `.statusBar`/`.floating`, presented off the main thread.
// Until then macOS uses the egui immediate-viewport fallback below.

/// Ask the native pet thread to stop, if one is running.
#[cfg(windows)]
pub use native::request_shutdown;

/// No native pet on non-Windows platforms.
#[cfg(not(windows))]
pub fn request_shutdown() {}

use std::collections::VecDeque;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui::{self, epaint::Vertex, Color32, Pos2, Rect, Vec2};

use animation::{AnimationPlayer, PetTextures};
use character::{CharacterKind, CharacterSpec};
use taskbar::{get_taskbar_info, TaskbarInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PetState {
    Sleeping,
    Grooming,
    Alert,
    Collecting,
    WalkingToTray,
    Arranging,
    WalkingHome,
    Stuck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogueContext {
    Stuck,
    Helped,
}

pub enum PetEvent {
    NewFile(String),
}

#[derive(Debug, Clone)]
pub struct GroundPaper {
    pub filename: String,
    pub base_x: f32,
    pub x: f32,
    pub current_y: f32,
    pub target_y: f32,
    pub flutter_t: f32,
    pub flutter_speed: f32,
    pub sway_amp: f32,
    pub landed: bool,
}

pub struct PetController {
    pub enabled: bool,
    pub spec: CharacterSpec,
    pub state: PetState,
    pub textures: Option<PetTextures>,
    pub player: AnimationPlayer,

    // Coordinates (screen space, in logical points)
    pub current_x: f32,
    pub current_y: f32,
    pub home_x: f32,
    pub target_tray_x: f32,
    pub folder_x: f32,
    pub facing_left: bool,
    pub position_offset: f32,
    pub is_dragging_desk: bool,
    pub drag_start_cursor_x: f32,
    pub drag_start_offset: f32,

    // Movement speed & scale
    pub speed: f32,
    pub scale: f32,

    // Taskbar info
    pub taskbar_info: Option<TaskbarInfo>,
    pub last_taskbar_check: Instant,
    pub last_update: Instant,

    // Event & Paper queuing
    pub rx: Option<Receiver<PetEvent>>,
    pub unspawned_queue: VecDeque<String>,
    pub ground_papers: Vec<GroundPaper>,
    pub carried_stack: Vec<String>, // Up to 3 files max
    pub random_seed: u32,

    // Animation & status timers
    pub alert_timer: f32,
    pub arrange_timer: f32,
    /// Countdown for alternating between Sleeping and Grooming while at home.
    pub idle_timer: f32,
    pub folder_alpha: f32, // Folder stationed at home base
    pub entrance_t: f32,   // 0.0 to 1.0 (smooth rise from taskbar at startup)
    pub is_overwhelmed: bool,
    pub sweat_timer: f32,
    pub thank_timer: f32,  // Heart / Thank you badge duration
    pub window_initialized: bool,
    /// Whether we have asked the window manager to keep the pet out of the
    /// taskbar / Activities overview (winit doesn't do this on Linux).
    pub pet_window_styled: bool,
    /// Frames left in the post-creation burst that re-applies the pet window's
    /// taskbar state (the first attempt can precede WM registration).
    pet_style_burst: u32,
    /// Whether the pet viewport is an X11 window whose input region we can
    /// shape (the app is running on the X11/XWayland backend). Set once by the
    /// app at startup; see `apply_x11_input_regions`.
    pub x11_shaped_input: bool,
    /// Clickable rects last applied via `x11_set_input_regions` (physical px).
    last_input_regions: Vec<(i32, i32, i32, i32)>,
    last_shape_apply: Instant,
    /// Pointer state observed by the pet viewport itself (its own window gets
    /// clicks on the sprite via the shaped input region). Positions are in the
    /// same screen-logical space as `current_x` / `folder_x`.
    viewport_button_down: bool,
    viewport_button_at: Option<Instant>,
    viewport_cursor: Option<(f32, f32)>,
    viewport_cursor_at: Option<Instant>,
    pub sparkle_bursts: Vec<(f32, f32, f32)>, // (x, y, remaining_secs)

    // Coordinate scaling and positioning
    pub ppp: f32,
    /// Current monitor size in logical points, learned from egui. Non-Windows
    /// platforms have no Windows-style taskbar to query, so the pet anchors to
    /// the bottom edge of this monitor instead of a hard-coded rectangle.
    pub screen_size: Option<(f32, f32)>,
    pub was_lbutton_down: bool,

    // Context-aware speech dialogue
    pub speech_text: Option<String>,
    pub speech_timer: f32,
}

impl PetController {
    pub fn new(rx: Option<Receiver<PetEvent>>) -> Self {
        let spec = CharacterSpec::for_kind(CharacterKind::Slime);
        let taskbar = get_taskbar_info();
        let (tray_x, ty) = if let Some(ref tb) = taskbar {
            // Taskbar top minus 30px puts Mochi's feet directly on top of the taskbar line
            (tb.tray_target.0, (tb.bounds.top as f32) - 30.0)
        } else {
            (520.0, 700.0)
        };

        // Folder is fixed near the system tray
        let folder_x = tray_x - 38.0;
        // Slime sleeps right next to its folder desk!
        let home_x = folder_x - 24.0;

        Self {
            enabled: true,
            spec,
            state: PetState::Sleeping,
            textures: None,
            player: AnimationPlayer::new(),
            current_x: home_x,
            current_y: ty,
            home_x,
            target_tray_x: tray_x,
            folder_x,
            facing_left: false,
            speed: 28.0, // Calmer, gentle stroll (24-30 px/s)
            scale: 1.0, // 1.00x native pixel art
            taskbar_info: taskbar,
            last_taskbar_check: Instant::now(),
            last_update: Instant::now(),
            rx,
            unspawned_queue: VecDeque::new(),
            ground_papers: Vec::new(),
            carried_stack: Vec::new(),
            random_seed: 54321,
            alert_timer: 0.0,
            arrange_timer: 0.0,
            idle_timer: 22.0,
            folder_alpha: 1.0,
            entrance_t: 1.0, // Instantly ready, no weird offset
            is_overwhelmed: false,
            sweat_timer: 0.0,
            thank_timer: 0.0,
            window_initialized: false,
            pet_window_styled: false,
            pet_style_burst: 0,
            x11_shaped_input: false,
            last_input_regions: Vec::new(),
            last_shape_apply: Instant::now(),
            viewport_button_down: false,
            viewport_button_at: None,
            viewport_cursor: None,
            viewport_cursor_at: None,
            sparkle_bursts: Vec::new(),
            ppp: 1.0,
            screen_size: None,
            was_lbutton_down: false,
            speech_text: None,
            speech_timer: 0.0,
            position_offset: 0.0,
            is_dragging_desk: false,
            drag_start_cursor_x: 0.0,
            drag_start_offset: 0.0,
        }
    }

    pub fn new_with_kind(rx: Option<Receiver<PetEvent>>, kind: CharacterKind) -> Self {
        let mut pet = Self::new(rx);
        pet.set_character(kind);
        pet
    }

    pub fn set_character(&mut self, kind: CharacterKind) {
        self.spec = CharacterSpec::for_kind(kind);
        self.textures = None; // Invalidate so textures reload with character sheet
    }

    fn pseudo_random(&mut self, min: f32, max: f32) -> f32 {
        self.random_seed = self.random_seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let normalized = (self.random_seed as f32) / (u32::MAX as f32);
        min + normalized * (max - min)
    }

    pub fn say_funny(&mut self, text: &str, duration: f32) {
        self.speech_text = Some(text.to_string());
        self.speech_timer = duration;
    }

    pub fn trigger_dialogue(&mut self, context: DialogueContext) {
        // If a message was triggered very recently (> 1.8s left), do not abruptly swap text
        if self.speech_timer > 1.8 {
            return;
        }

        let is_cat = matches!(self.spec.kind, CharacterKind::Neko | CharacterKind::Oneko);

        let quotes: &[&str] = match (context, is_cat) {
            (DialogueContext::Stuck, true) => &[
                "Nyaa! I'm stuck, help meow!",
                "Who blocked my path nya?!",
                "Neko paws can't jump over this file!",
                "bro i am stuck see naah nya!",
                "Meoww! Path blocked, send help!",
                "Human! Clear the folder slot for meow!",
            ],
            (DialogueContext::Stuck, false) => &[
                "bro i am stuck see naah",
                "yoo bro where are you",
                "bro someone left a file in my way",
                "arre bro look here, i'm stuck",
                "bro help me out, can't reach folder",
                "clear this for me naah bro",
                "Hey! Who dumped a file in my doorway?!",
                "I'm a slime, not a forklift!",
                "I have tiny mochi legs, I can't climb this!",
                "Boss! Clear the doorway with Help button!",
                "Traffic jam at the folder! Send help!",
            ],
            (DialogueContext::Helped, true) => &[
                "Nyaa~ thank you!",
                "Purrrr... thanks bro!",
                "Good human! *happy tail swish*",
                "Filed and purr-fect!",
                "Arigato! Teamwork nya!",
                "Purr... desk is sparkling clean!",
            ],
            (DialogueContext::Helped, false) => &[
                "thaks man!",
                "yoo thanks bro!",
                "saved me there, thanks man",
                "appreciate it bro!",
                "thanks bro, you're a lifesaver",
                "You're the best, boss!",
                "Teamwork makes the dream work!",
                "Phew, crisis averted! Thank you!",
                "Path is clear! Full speed ahead!",
                "10/10 teamwork! Sorted!",
                "Another clean directory victory!",
                "Filed and styled! Thanks bro!",
                "Clean desk, happy life! You rock!",
            ],
        };

        let idx = (self.pseudo_random(0.0, quotes.len() as f32) as usize).min(quotes.len() - 1);
        self.say_funny(quotes[idx], 3.5);
    }

    /// Distance in logical points between the sprite's feet and the very bottom
    /// of the screen when anchored to a desktop edge.
    const BOTTOM_MARGIN: f32 = 2.0;

    /// Refresh the pet's anchor coordinates.
    ///
    /// On non-Windows platforms there is no Windows-style taskbar rectangle to
    /// query, so the pet rests on the bottom edge of the monitor reported by
    /// egui (`screen_size`, in logical points). Windows uses the native pet
    /// backend and never reaches this path.
    pub fn refresh_taskbar_coords(&mut self) {
        if let Some((screen_w, screen_h)) = self.screen_size {
            let baseline_top = (screen_h - Self::BOTTOM_MARGIN).max(0.0);
            let tray_x = (screen_w - 100.0).max(120.0);

            self.target_tray_x = tray_x;
            self.folder_x = tray_x - 38.0 + self.position_offset;
            self.home_x = self.folder_x - 24.0;
            // The sprite baseline in its 32px frame sits at y = 30, so this puts
            // its feet right on the bottom edge of the screen.
            self.current_y = baseline_top - 30.0;
            if matches!(self.state, PetState::Sleeping | PetState::Grooming) {
                self.current_x = self.home_x;
            }
            return;
        }

        if let Some(tb) = get_taskbar_info() {
            self.taskbar_info = Some(tb);
            let ppp = self.ppp.max(0.5);
            let tb_top = (tb.bounds.top as f32) / ppp;
            let tray_x = tb.tray_target.0 / ppp;

            self.target_tray_x = tray_x;
            self.folder_x = tray_x - 38.0 + self.position_offset;
            self.home_x = self.folder_x - 24.0;
            // The slime body baseline in 32px frame is at y = 30.
            // Setting current_y = tb_top - 30.0 aligns the slime's feet exactly on the top edge of the taskbar!
            self.current_y = tb_top - 30.0;
            if matches!(self.state, PetState::Sleeping | PetState::Grooming) {
                self.current_x = self.home_x;
            }
        }
    }

    /// Refresh taskbar periodically if it moved or display changed.
    pub fn refresh_taskbar(&mut self) {
        if self.last_taskbar_check.elapsed() > Duration::from_secs(3) {
            self.last_taskbar_check = Instant::now();
            self.refresh_taskbar_coords();
            // Re-assert this in case the window was only created after our
            // first attempt, or the compositor restarted.
            taskbar::x11_set_skip_taskbar("DirWatcherPet", true);
        }
    }

    /// Spawn a fluttering falling paper at a specific x coordinate.
    pub fn spawn_paper_at(&mut self, name: String, drop_x: f32) {
        let start_y = self.current_y - 85.0; // High in the air
        let target_y = self.current_y + 14.0; // Exactly on taskbar baseline (matches 16px paper height)
        let flutter_t = self.pseudo_random(0.0, std::f32::consts::TAU);
        let flutter_speed = self.pseudo_random(4.5, 6.5);
        let sway_amp = self.pseudo_random(10.0, 16.0);

        self.ground_papers.push(GroundPaper {
            filename: name,
            base_x: drop_x,
            x: drop_x,
            current_y: start_y,
            target_y,
            flutter_t,
            flutter_speed,
            sway_amp,
            landed: false,
        });

        // If sleeping (or grooming) and papers appear, wake up!
        if matches!(self.state, PetState::Sleeping | PetState::Grooming) {
            self.state = PetState::Alert;
            self.alert_timer = 0.5;
            self.player.reset();
        }
    }

    /// Poll incoming events and spawn fluttering falling papers across the path.
    pub fn poll_events(&mut self) {
        if let Some(ref rx) = self.rx {
            while let Ok(event) = rx.try_recv() {
                match event {
                    PetEvent::NewFile(name) => {
                        log::info!("Pet received new file: {name}");
                        self.unspawned_queue.push_back(name);
                    }
                }
            }
        }

        // Spawn falling papers anywhere along the taskbar path:
        // from the left (~280px away from folder) up to slightly right of folder
        while let Some(name) = self.unspawned_queue.pop_front() {
            let min_drop_x = (self.folder_x - 280.0).max(20.0);
            let max_drop_x = self.folder_x + 30.0;
            let drop_x = self.pseudo_random(min_drop_x, max_drop_x);
            self.spawn_paper_at(name, drop_x);
        }
    }

    /// User helper action: cleans papers on ground and delivers them!
    pub fn help_clean(&mut self) {
        if !self.ground_papers.is_empty() || !self.carried_stack.is_empty() {
            self.ground_papers.clear();
            self.carried_stack.clear();
            self.current_x = self.folder_x - 24.0;
            self.facing_left = false;
            self.state = PetState::Arranging;
            self.arrange_timer = 0.9;
            self.is_overwhelmed = false;
            self.thank_timer = 1.5;
            self.trigger_dialogue(DialogueContext::Helped);
        }
    }

    /// Update logic and animation state machine.
    pub fn update(&mut self) -> Duration {
        if !self.enabled {
            if let Some(ref rx) = self.rx {
                while let Ok(_) = rx.try_recv() {}
            }
            return Duration::from_millis(500);
        }

        self.poll_events();
        self.refresh_taskbar();

        let now = Instant::now();
        let dt = now.duration_since(self.last_update).as_secs_f32().min(0.1);
        self.last_update = now;

        // Decrement speech bubble timer and clear text when done
        if self.speech_timer > 0.0 {
            self.speech_timer = (self.speech_timer - dt).max(0.0);
            if self.speech_timer == 0.0 {
                self.speech_text = None;
            }
        }

        // Pointer input into the pet overlay itself, when it gets clicks via
        // the shaped input region (X11/XWayland backend): exact positions and
        // reliable button state, observed in `render()` during the last frame.
        let viewport_fresh = |at: Option<Instant>| -> bool {
            at.map(|t| t.elapsed() < std::time::Duration::from_millis(400))
                .unwrap_or(false)
        };
        let mut lbutton_down = self.viewport_button_down && viewport_fresh(self.viewport_button_at);
        // The pet's own window reports exact positions while it holds the
        // pointer (a click on the shaped input region starts an implicit grab);
        // outside that we fall back to the global pointer below. The button
        // state also comes from XInput2 raw events (see `taskbar`).
        let cursor_screen = self
            .viewport_cursor
            .filter(|_| viewport_fresh(self.viewport_cursor_at));

        let lbutton_poll = taskbar::is_lbutton_pressed();
        if !lbutton_down {
            lbutton_down = lbutton_poll;
        }
        let lbutton_clicked = lbutton_down && !self.was_lbutton_down;
        self.was_lbutton_down = lbutton_down;

        // --- Drag & drop Mochi and folder desk together when Mochi is idle ---
        if matches!(self.state, PetState::Sleeping | PetState::Grooming) {
            if let Some((phys_x, phys_y)) = cursor_screen.or_else(taskbar::get_global_cursor_pos) {
                let ppp = self.ppp.max(0.5);
                let cursor_x = phys_x / ppp;
                let cursor_y = phys_y / ppp;

                if lbutton_down {
                    if !self.is_dragging_desk {
                        let desk_min_x = self.home_x - 14.0;
                        let desk_max_x = self.folder_x + 30.0;
                        let desk_min_y = self.current_y - 12.0;
                        let desk_max_y = self.current_y + 36.0;

                        let hit = cursor_x >= desk_min_x
                            && cursor_x <= desk_max_x
                            && cursor_y >= desk_min_y
                            && cursor_y <= desk_max_y;
                        if log::log_enabled!(log::Level::Debug) && lbutton_clicked {
                            log::debug!(
                                "drag probe: cursor=({cursor_x:.0},{cursor_y:.0}) ppp={ppp} \
                                 home_x={:.0} folder_x={:.0} current_y={:.0} hit={hit} lclicked={lbutton_clicked}",
                                self.home_x, self.folder_x, self.current_y
                            );
                        }

                        if hit {
                            self.is_dragging_desk = true;
                            self.drag_start_cursor_x = cursor_x;
                            self.drag_start_offset = self.position_offset;
                            log::debug!("drag started");
                        }
                    } else {
                        let delta = cursor_x - self.drag_start_cursor_x;
                        // Allow moving the desk anywhere along the bottom edge
                        // (the old hard clamp cut off the left half).
                        let (lo, hi) = match self.screen_size {
                            Some((w, _)) => (140.0 - w, w - 140.0),
                            None => (-1200.0, 300.0),
                        };
                        let new_offset = (self.drag_start_offset + delta).clamp(lo, hi);
                        if log::log_enabled!(log::Level::Debug)
                            && (new_offset - self.position_offset).abs() > 1.0
                        {
                            log::debug!(
                                "dragging: cursor_x={cursor_x:.0} delta={delta:.0} offset {:.0} -> {:.0}",
                                self.position_offset,
                                new_offset
                            );
                        }
                        self.position_offset = new_offset;
                        self.refresh_taskbar_coords();
                    }
                } else if self.is_dragging_desk {
                    self.is_dragging_desk = false;
                    log::debug!("drag ended");
                }
            }
        } else if self.is_dragging_desk {
            self.is_dragging_desk = false;
        }

        // Direct desktop click detection on papers!
        // When Mochi is stuck, the user can click directly on the blocking paper to clear it!
        if lbutton_clicked && !self.is_dragging_desk {
            if let Some((phys_x, phys_y)) = taskbar::get_global_cursor_pos() {
                let ppp = self.ppp.max(0.5);
                let cursor_x = phys_x / ppp;
                let cursor_y = phys_y / ppp;

                let mut clicked_idx = None;
                for (i, p) in self.ground_papers.iter().enumerate() {
                    let center_x = p.x;
                    let center_y = p.current_y + 8.0;
                    let dx = cursor_x - center_x;
                    let dy = cursor_y - center_y;
                    if (dx * dx + dy * dy) <= (24.0 * 24.0) {
                        clicked_idx = Some(i);
                        break;
                    }
                }

                if let Some(idx) = clicked_idx {
                    let paper = self.ground_papers.remove(idx);
                    self.sparkle_bursts.push((paper.x, paper.current_y, 0.7));

                    let still_blocked = self
                        .ground_papers
                        .iter()
                        .any(|p| p.landed && (p.x - self.folder_x).abs() <= 28.0);

                    if self.state == PetState::Stuck && !still_blocked {
                        self.state = if !self.carried_stack.is_empty() {
                            PetState::WalkingToTray
                        } else if !self.ground_papers.is_empty() {
                            PetState::Collecting
                        } else {
                            PetState::WalkingHome
                        };
                        self.thank_timer = 2.0;
                        self.player.reset();
                        self.trigger_dialogue(DialogueContext::Helped);
                    }
                }
            }
        }

        // Smooth entrance rise from below taskbar once window is initialized
        if self.window_initialized && self.entrance_t < 1.0 {
            self.entrance_t = (self.entrance_t + dt * 1.5).min(1.0);
        }

        if self.thank_timer > 0.0 {
            self.thank_timer = (self.thank_timer - dt).max(0.0);
        }

        self.sparkle_bursts.retain_mut(|(_, _, t)| {
            *t -= dt;
            *t > 0.0
        });

        // Check if overwhelmed by too many papers (4+ papers)
        let total_work = self.ground_papers.len() + self.carried_stack.len();
        self.is_overwhelmed = total_work >= 4;
        if self.is_overwhelmed {
            self.sweat_timer += dt;
        }

        // Folder is stationed permanently at the home desk
        self.folder_alpha = 1.0;

        // 1. Realistic Fluttering Paper Physics
        for paper in &mut self.ground_papers {
            if !paper.landed {
                paper.flutter_t += dt * paper.flutter_speed;
                // Side-to-side sway
                let sway = paper.flutter_t.sin() * paper.sway_amp;
                paper.x = paper.base_x + sway;

                // Air resistance: falls slower at the apex of sway, glides faster in transition
                let air_lift = (paper.flutter_t.sin()).abs();
                let fall_rate = 38.0 + (1.0 - air_lift) * 32.0; // ~40-70 px/s gentle drift
                paper.current_y += fall_rate * dt;

                if paper.current_y >= paper.target_y {
                    paper.current_y = paper.target_y;
                    paper.landed = true;
                }
            }
        }

        let stop_in_front_of_folder_x = self.folder_x - 24.0;

        // 2. State machine
        match self.state {
            PetState::Sleeping => {
                let delay = self.player.update(&self.spec.anim_sleep);
                self.idle_timer -= dt;
                if !self.is_dragging_desk && self.idle_timer <= 0.0 {
                    // Wake up just enough to groom for a moment.
                    self.state = PetState::Grooming;
                    self.idle_timer = self.pseudo_random(4.0, 8.0);
                    self.player.reset();
                }
                if self.is_dragging_desk {
                    Duration::from_millis(16)
                } else {
                    delay.max(Duration::from_millis(250))
                }
            }
            PetState::Grooming => {
                let anim = *self.spec.groom();
                let delay = self.player.update(&anim);
                self.idle_timer -= dt;
                if !self.is_dragging_desk && self.idle_timer <= 0.0 {
                    self.state = PetState::Sleeping;
                    self.idle_timer = self.pseudo_random(16.0, 36.0);
                    self.player.reset();
                }
                if self.is_dragging_desk {
                    Duration::from_millis(16)
                } else {
                    delay.max(Duration::from_millis(200))
                }
            }
            PetState::Alert => {
                let delay = self.player.update(&self.spec.anim_alert);
                self.alert_timer -= dt;
                if self.alert_timer <= 0.0 {
                    self.state = PetState::Collecting;
                    self.player.reset();
                }
                delay.min(Duration::from_millis(16))
            }
            PetState::Collecting => {
                let delay = self.player.update(&self.spec.anim_walk);

                // If carrying 3 files (max), or no papers on ground, proceed to arrange them!
                if self.carried_stack.len() >= 3 || self.ground_papers.is_empty() {
                    if !self.carried_stack.is_empty() {
                        self.state = PetState::WalkingToTray;
                    } else {
                        self.state = PetState::WalkingHome;
                    }
                    self.player.reset();
                    return delay.min(Duration::from_millis(16));
                }

                // Find nearest paper
                let mut nearest_idx = 0;
                let mut min_dist = f32::MAX;
                for (i, p) in self.ground_papers.iter().enumerate() {
                    let d = (p.x - self.current_x).abs();
                    if d < min_dist {
                        min_dist = d;
                        nearest_idx = i;
                    }
                }

                let target_paper_x = self.ground_papers[nearest_idx].x;
                self.facing_left = target_paper_x < self.current_x;

                let step = self.speed * dt;
                if (self.current_x - target_paper_x).abs() <= step + 4.0 {
                    // Scoop paper onto head quietly
                    self.current_x = target_paper_x;
                    let picked = self.ground_papers.remove(nearest_idx);
                    self.carried_stack.push(picked.filename);

                    if self.carried_stack.len() >= 3 || self.ground_papers.is_empty() {
                        self.state = PetState::WalkingToTray;
                        self.player.reset();
                    }
                } else if self.current_x < target_paper_x {
                    self.current_x += step;
                } else {
                    self.current_x -= step;
                }

                delay.min(Duration::from_millis(16))
            }
            PetState::WalkingToTray => {
                let delay = self.player.update(&self.spec.anim_walk);
                let target_x = stop_in_front_of_folder_x;
                let step = self.speed * dt;

                // Check if a landed paper is blocking the folder entrance!
                let is_blocking = self.ground_papers.iter().find(|p| p.landed && (p.x - self.folder_x).abs() <= 28.0);
                if let Some(blocker) = is_blocking {
                    if (self.current_x - blocker.x).abs() <= 32.0 || (self.current_x - target_x).abs() <= 32.0 {
                        let was_not_stuck = self.state != PetState::Stuck;
                        self.state = PetState::Stuck;
                        self.facing_left = blocker.x < self.current_x;
                        self.player.reset();
                        // Trigger surprise cry for help ONCE when first stuck!
                        if was_not_stuck {
                            self.trigger_dialogue(DialogueContext::Stuck);
                        }
                        return delay.min(Duration::from_millis(16));
                    }
                }

                if (self.current_x - target_x).abs() <= step + 3.0 {
                    self.current_x = target_x;
                    self.facing_left = false;
                    self.state = PetState::Arranging;
                    self.player.reset();
                    self.arrange_timer = 1.1; // 1.1s organizing sequence
                } else if self.current_x < target_x {
                    self.current_x += step;
                    self.facing_left = false;
                } else {
                    self.current_x -= step;
                    self.facing_left = true;
                }

                delay.min(Duration::from_millis(16))
            }
            PetState::Arranging => {
                // Happy bow & organizing gesture
                let delay = self.player.update(&self.spec.anim_drop);
                self.arrange_timer -= dt;

                if self.arrange_timer <= 0.0 {
                    self.carried_stack.clear();

                    // If more papers remain, go collect next batch!
                    if !self.ground_papers.is_empty() || !self.unspawned_queue.is_empty() {
                        self.state = PetState::Collecting;
                    } else {
                        self.state = PetState::WalkingHome;
                    }
                    self.player.reset();
                }

                delay.min(Duration::from_millis(16))
            }
            PetState::WalkingHome => {
                let delay = self.player.update(&self.spec.anim_walk);
                let target_x = self.home_x;
                let step = self.speed * dt;

                if !self.ground_papers.is_empty() {
                    self.state = PetState::Collecting;
                    self.player.reset();
                } else if (self.current_x - target_x).abs() <= step + 3.0 {
                    self.current_x = target_x;
                    self.facing_left = false;
                    self.state = PetState::Sleeping;
                    self.idle_timer = self.pseudo_random(16.0, 36.0);
                    self.player.reset();
                } else if self.current_x > target_x {
                    self.current_x -= step;
                    self.facing_left = true;
                } else {
                    self.current_x += step;
                    self.facing_left = false;
                }

                delay.min(Duration::from_millis(16))
            }
            PetState::Stuck => {
                // Alert/distressed animation with sweat drops
                let delay = self.player.update(&self.spec.anim_alert);
                self.sweat_timer += dt;

                // Check if blocking paper was cleared
                let still_blocked = self.ground_papers.iter().any(|p| p.landed && (p.x - self.folder_x).abs() <= 28.0);
                if !still_blocked {
                    self.state = if !self.carried_stack.is_empty() {
                        PetState::WalkingToTray
                    } else if !self.ground_papers.is_empty() {
                        PetState::Collecting
                    } else {
                        PetState::WalkingHome
                    };
                    self.thank_timer = 1.8;
                    self.player.reset();
                    self.trigger_dialogue(DialogueContext::Helped);
                }

                delay.min(Duration::from_millis(16))
            }
        }
    }

/// Draw a crisp, shimmering 4-pointed golden vector star with a glowing white center.
/// Does not depend on system fonts or Unicode glyphs (avoids missing-glyph square boxes).
fn draw_sparkle_star(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    if radius <= 0.5 || color.a() == 0 {
        return;
    }
    let cx = center.x;
    let cy = center.y;
    let r = radius;
    let inner = r * 0.28;

    // Vertical diamond
    let poly_v = vec![
        Pos2::new(cx, cy - r),
        Pos2::new(cx + inner, cy),
        Pos2::new(cx, cy + r),
        Pos2::new(cx - inner, cy),
    ];
    painter.add(egui::Shape::convex_polygon(poly_v, color, egui::Stroke::NONE));

    // Horizontal diamond
    let poly_h = vec![
        Pos2::new(cx - r, cy),
        Pos2::new(cx, cy - inner),
        Pos2::new(cx + r, cy),
        Pos2::new(cx, cy + inner),
    ];
    painter.add(egui::Shape::convex_polygon(poly_h, color, egui::Stroke::NONE));

    // Glowing white center core
    let core_alpha = color.a();
    painter.circle_filled(
        center,
        (r * 0.25).max(1.0),
        Color32::from_rgba_unmultiplied(255, 255, 255, core_alpha),
    );
}

    /// Render the pet onto the egui context.
    pub fn render(&mut self, ctx: &egui::Context) {
        if !self.enabled {
            // Make sure the hidden overlay does not keep intercepting clicks.
            if self.x11_shaped_input && !self.last_input_regions.is_empty() {
                self.last_input_regions.clear();
                taskbar::x11_set_input_regions("DirWatcherPet", &[]);
            }
            return;
        }

        let ppp = ctx.pixels_per_point().max(0.5);
        // Learn the monitor size (logical points) so the pet can anchor to the
        // real bottom edge instead of a hard-coded rectangle.
        if let Some(size) = ctx.input(|i| i.viewport().monitor_size) {
            if size.x > 1.0 && size.y > 1.0 {
                self.screen_size = Some((size.x, size.y));
            }
        }
        if (self.ppp - ppp).abs() > 0.01
            || !self.window_initialized
            || self.screen_size.is_none()
        {
            self.ppp = ppp;
            self.refresh_taskbar_coords();
            self.window_initialized = true;
        }

        if self.textures.is_none() {
            self.textures = Some(PetTextures::load(ctx, &self.spec));
        }
        // Owned clone so the closure below can still borrow `self` mutably to
        // handle dragging (TextureHandles are cheap Arc clones).
        let textures = self.textures.as_ref().unwrap().clone();

        let anim_def = match self.state {
            PetState::Sleeping => &self.spec.anim_sleep,
            PetState::Grooming => self.spec.groom(),
            PetState::Alert => &self.spec.anim_alert,
            PetState::Collecting => &self.spec.anim_walk,
            PetState::WalkingToTray => &self.spec.anim_walk,
            PetState::Arranging => &self.spec.anim_drop,
            PetState::WalkingHome => &self.spec.anim_walk,
            PetState::Stuck => &self.spec.anim_alert,
        };

        let frame = self.player.current_frame();
        let uv = textures.char_uv(&self.spec, anim_def, frame, self.facing_left);

        let char_w = self.spec.frame_width as f32 * self.scale;
        let char_h = self.spec.frame_height as f32 * self.scale;

        // Fixed, generous overlay covering the whole paper-drop/walk zone.
        // It must stay constant: moving/resizing the window every frame forces
        // the GL surface to be recreated each frame, which pegs a CPU core.
        let strip_x = (self.folder_x - 400.0).max(0.0);
        let strip_w = ((self.folder_x + 120.0) - strip_x).max(440.0);
        let strip_h = 145.0;
        let mut strip_y = self.current_y - 85.0;
        // Keep the overlay fully on-screen (its natural height would otherwise
        // hang ~28px below the bottom edge once anchored there).
        if let Some((_, screen_h)) = self.screen_size {
            strip_y = strip_y.min((screen_h - strip_h).max(0.0)).max(0.0);
        }

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("desktop_pet_viewport"),
            egui::ViewportBuilder::default()
                .with_title("DirWatcherPet")
                .with_transparent(true)
                .with_decorations(false)
                .with_always_on_top()
                .with_taskbar(false)
                // On the X11/XWayland backend the pet window must RECEIVE
                // clicks on the sprite (its input region is shaped further
                // below), so it cannot be registered as mouse-passthrough. On
                // a pure Wayland session we keep full click-through: dragging
                // is impossible there anyway (no window positioning, no grabs)
                // and a misplaced overlay must not block desktop clicks.
                .with_mouse_passthrough(!self.x11_shaped_input)
                // A click on the sprite should not steal keyboard focus from
                // the user's work.
                .with_active(false)
                .with_inner_size([strip_w, strip_h])
                .with_position([strip_x, strip_y]),
            |sub_ctx, _class| {
                // Observe pointer state in the pet's own window. Clicks land on
                // it via the shaped input region, so egui sees accurate local
                // positions exact to the sprite — far more trustworthy than the
                // QueryPointer/raw-valuator guesses used before. Drag motion
                // while grabbed is also delivered here continuously.
                let press = sub_ctx.input(|i| {
                    i.pointer
                        .primary_clicked()
                        .then_some(i.pointer.press_origin())
                });
                if let Some(origin) = press {
                    log::debug!("viewport press origin {origin:?}");
                    if let Some(origin) = origin {
                        self.viewport_cursor = Some(screen_point(origin, strip_x, strip_y, ppp));
                        self.viewport_cursor_at = Some(Instant::now());
                        self.viewport_button_down = true;
                        self.viewport_button_at = Some(Instant::now());
                    }
                }
                if self.viewport_button_down {
                    let released = sub_ctx.input(|i| i.pointer.any_released());
                    if released {
                        self.viewport_button_down = false;
                        self.viewport_button_at = None;
                    }
                    sub_ctx.input(|i| i.pointer.interact_pos()).inspect(|pos| {
                        log::debug!("viewport pointer at {pos:?}");
                        self.viewport_cursor =
                            Some(screen_point(*pos, strip_x, strip_y, ppp));
                        self.viewport_cursor_at = Some(Instant::now());
                    });
                }
                egui::Area::new(egui::Id::new("pet_viewport_area"))
                    .fixed_pos(Pos2::ZERO)
                    .show(sub_ctx, |ui| {
                        let painter = ui.painter();

                        // --- 1. Draw Destination Folder Organizer (resting directly on taskbar top line) ---
                        let folder_local_x = self.folder_x - strip_x;
                        let folder_local_y = (self.current_y + 14.0) - strip_y;

                        if self.folder_alpha > 0.01 {
                            let folder_rect = Rect::from_min_size(
                                Pos2::new(folder_local_x, folder_local_y),
                                Vec2::new(16.0, 16.0),
                            );
                            let alpha = (self.folder_alpha * 255.0) as u8;
                            painter.image(
                                textures.folder_slot.id(),
                                folder_rect,
                                Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                                Color32::from_white_alpha(alpha),
                            );
                        }

                        // --- 2. Draw Falling / Landed Papers naturally (clean pixel art, no yellow boxes) ---
                        for paper in &self.ground_papers {
                            let px = paper.x - strip_x;
                            let py = paper.current_y - strip_y;

                            if !paper.landed {
                                let tilt = (paper.flutter_t.cos()) * 0.35;
                                let cos_r = tilt.cos();
                                let sin_r = tilt.sin();

                                let rot_pt = |ox: f32, oy: f32| -> Pos2 {
                                    Pos2::new(px + ox * cos_r - oy * sin_r, py + ox * sin_r + oy * cos_r)
                                };

                                let p0 = rot_pt(-8.0, -8.0);
                                let p1 = rot_pt(8.0, -8.0);
                                let p2 = rot_pt(8.0, 8.0);
                                let p3 = rot_pt(-8.0, 8.0);

                                let mut mesh = egui::Mesh::with_texture(textures.file_icon.id());
                                mesh.add_triangle(0, 1, 2);
                                mesh.add_triangle(0, 2, 3);
                                mesh.vertices.push(Vertex { pos: p0, uv: Pos2::new(0.0, 0.0), color: Color32::WHITE });
                                mesh.vertices.push(Vertex { pos: p1, uv: Pos2::new(1.0, 0.0), color: Color32::WHITE });
                                mesh.vertices.push(Vertex { pos: p2, uv: Pos2::new(1.0, 1.0), color: Color32::WHITE });
                                mesh.vertices.push(Vertex { pos: p3, uv: Pos2::new(0.0, 1.0), color: Color32::WHITE });

                                painter.add(egui::Shape::mesh(mesh));
                            } else {
                                // Landed flat on taskbar baseline
                                let paper_rect = Rect::from_min_size(
                                    Pos2::new(px - 8.0, py),
                                    Vec2::new(16.0, 16.0),
                                );
                                painter.image(
                                    textures.file_icon.id(),
                                    paper_rect,
                                    Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                                    Color32::WHITE,
                                );
                            }
                        }

                        // --- 3. Sparkle Bursts from Cleaned Papers (vector golden stars, no missing font glyphs) ---
                        for (sx, sy, timer) in &self.sparkle_bursts {
                            let local_sx = sx - strip_x;
                            let local_sy = sy - strip_y;
                            let progress = 1.0 - (*timer / 0.7).clamp(0.0, 1.0);
                            let sp_y = local_sy - (progress * 22.0);
                            let sp_alpha = ((1.0 - progress) * 255.0) as u8;
                            let gold_color = Color32::from_rgba_unmultiplied(255, 215, 0, sp_alpha);
                            let size = (1.0 - (progress - 0.5).abs() * 2.0).max(0.4) * 6.5;

                            // Main golden star
                            PetController::draw_sparkle_star(painter, Pos2::new(local_sx, sp_y), size, gold_color);
                            // Left fluttering star
                            let left_x = local_sx - 9.0 - progress * 4.0;
                            let left_y = sp_y + 3.0 - progress * 6.0;
                            PetController::draw_sparkle_star(painter, Pos2::new(left_x, left_y), size * 0.65, gold_color);
                            // Right fluttering star
                            let right_x = local_sx + 9.0 + progress * 4.0;
                            let right_y = sp_y + 2.0 - progress * 8.0;
                            PetController::draw_sparkle_star(painter, Pos2::new(right_x, right_y), size * 0.75, gold_color);
                        }

                        // --- 4. Local pet coordinates (resting perfectly on top of taskbar line) ---
                        let pet_local_x = self.current_x - strip_x;
                        let pet_local_y = self.current_y - strip_y;

                        let pet_rect = Rect::from_min_size(
                            Pos2::new(pet_local_x, pet_local_y),
                            Vec2::new(char_w, char_h),
                        );

                        // Draw White Mochi Slime
                        painter.image(
                            textures.character.id(),
                            pet_rect,
                            uv,
                            Color32::WHITE,
                        );

                        // --- 5. Status indicators (Vector sweat teardrop when stuck or overwhelmed) ---
                        if self.state == PetState::Stuck || (self.is_overwhelmed && self.state != PetState::Sleeping) {
                            let sweat_bob = (self.sweat_timer * 7.0).sin() * 2.0;
                            let drop_x = pet_rect.left() - 4.0;
                            let drop_y = pet_rect.top() + 8.0 + sweat_bob;
                            let drop_color = Color32::from_rgb(110, 195, 255);
                            painter.circle_filled(Pos2::new(drop_x, drop_y), 2.5, drop_color);
                            painter.add(egui::Shape::convex_polygon(
                                vec![Pos2::new(drop_x, drop_y - 4.5), Pos2::new(drop_x - 2.2, drop_y), Pos2::new(drop_x + 2.2, drop_y)],
                                drop_color,
                                egui::Stroke::NONE,
                            ));
                            painter.circle_filled(Pos2::new(drop_x - 0.7, drop_y - 0.7), 0.8, Color32::WHITE);
                        }

                        // --- 6. Funny, Random, Context-Aware Comic Speech Bubble ---
                        if self.speech_timer > 0.0 {
                            if let Some(ref msg) = self.speech_text {
                                let bubble_font = egui::FontId::proportional(11.0);
                                let text_galley = painter.layout_no_wrap(msg.clone(), bubble_font, Color32::WHITE);
                                let text_size = text_galley.size();
                                let pad = Vec2::new(10.0, 5.0);
                                let bubble_w = text_size.x + pad.x * 2.0;
                                let bubble_h = text_size.y + pad.y * 2.0;

                                let bubble_center_x = (pet_rect.center().x).clamp(bubble_w / 2.0 + 4.0, strip_w - bubble_w / 2.0 - 4.0);
                                let stack_offset = if !self.carried_stack.is_empty() {
                                    8.0 + (self.carried_stack.len() as f32 * 3.0)
                                } else {
                                    0.0
                                };
                                let bubble_bottom_y = pet_rect.top() - 6.0 - stack_offset;
                                let bubble_rect = Rect::from_min_size(
                                    Pos2::new(bubble_center_x - bubble_w / 2.0, bubble_bottom_y - bubble_h),
                                    Vec2::new(bubble_w, bubble_h),
                                );

                                let alpha_factor = if self.speech_timer < 0.4 {
                                    self.speech_timer / 0.4
                                } else {
                                    1.0
                                }.clamp(0.0, 1.0);

                                let bg_alpha = (alpha_factor * 235.0) as u8;
                                let border_alpha = (alpha_factor * 255.0) as u8;
                                let text_alpha = (alpha_factor * 255.0) as u8;

                                painter.rect_filled(bubble_rect, 6.0, Color32::from_rgba_premultiplied(22, 26, 36, bg_alpha));

                                let border_color = if self.state == PetState::Stuck {
                                    Color32::from_rgba_unmultiplied(255, 110, 100, border_alpha)
                                } else {
                                    Color32::from_rgba_unmultiplied(120, 180, 255, border_alpha)
                                };
                                painter.rect_stroke(bubble_rect, 6.0, egui::Stroke::new(1.0_f32, border_color), egui::StrokeKind::Inside);

                                // Speech tail pointing toward Mochi's head
                                let tail_x = pet_rect.center().x.clamp(bubble_rect.left() + 8.0, bubble_rect.right() - 8.0);
                                let mut tail_mesh = egui::Mesh::default();
                                tail_mesh.add_triangle(0, 1, 2);
                                tail_mesh.vertices.push(Vertex { pos: Pos2::new(tail_x - 4.0, bubble_bottom_y), uv: Pos2::ZERO, color: border_color });
                                tail_mesh.vertices.push(Vertex { pos: Pos2::new(tail_x + 4.0, bubble_bottom_y), uv: Pos2::ZERO, color: border_color });
                                tail_mesh.vertices.push(Vertex { pos: Pos2::new(tail_x, bubble_bottom_y + 4.0), uv: Pos2::ZERO, color: border_color });
                                painter.add(egui::Shape::mesh(tail_mesh));

                                let text_pos = Pos2::new(bubble_rect.left() + pad.x, bubble_rect.top() + pad.y);
                                painter.galley(text_pos, text_galley, Color32::from_white_alpha(text_alpha));
                            }
                        }

                        // --- 7. Sleeping Zzz particles ---
                        if self.state == PetState::Sleeping {
                            let z_frame = (frame / 2) % 4;
                            let z_u0 = (z_frame as f32 * 32.0) / 128.0;
                            let z_u1 = ((z_frame + 1) as f32 * 32.0) / 128.0;
                            let zzz_uv = Rect::from_min_max(Pos2::new(z_u0, 0.0), Pos2::new(z_u1, 1.0));

                            let zzz_rect = Rect::from_min_size(
                                Pos2::new(pet_rect.center().x + 2.0, pet_rect.top() - 2.0),
                                Vec2::new(16.0, 16.0),
                            );
                            painter.image(
                                textures.zzz.id(),
                                zzz_rect,
                                zzz_uv,
                                Color32::WHITE,
                            );
                        }

                        // --- 8. Carried Stack: Resting directly ON the mochi's head surface! ---
                        let stack_count = self.carried_stack.len();
                        if stack_count > 0 && self.state != PetState::Arranging {
                            let bounce_oy = match frame % 10 {
                                1 | 5 => -1.0,
                                2 | 4 => -3.0,
                                3 => -4.0,
                                _ => 0.0,
                            };

                            let head_surface_y = pet_rect.top() + 16.0 + bounce_oy;

                            for i in 0..stack_count.min(3) {
                                let layer = i as f32;
                                let tilt_x = if i == 1 { 1.5 } else if i == 2 { -1.5 } else { 0.0 };
                                let stack_y = head_surface_y - 12.0 - (layer * 3.0);

                                let file_rect = Rect::from_min_size(
                                    Pos2::new(pet_rect.center().x - 8.0 + tilt_x, stack_y),
                                    Vec2::new(16.0, 16.0),
                                );

                                painter.image(
                                    textures.file_icon.id(),
                                    file_rect,
                                    Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                                    Color32::WHITE,
                                );
                            }
                        }

                        // --- 9. Arranging Animation: Papers slot neatly into the folder pocket! ---
                        if self.state == PetState::Arranging && self.arrange_timer > 0.0 {
                            let progress = 1.0 - (self.arrange_timer / 1.1).clamp(0.0, 1.0);

                            // The top paper glides from head smoothly into the folder slot
                            let start_slot_x = pet_rect.center().x - 8.0;
                            let start_slot_y = pet_rect.top() + 4.0;
                            let end_slot_x = folder_local_x;
                            let end_slot_y = folder_local_y - 2.0;

                            let slide_progress = (progress * 1.6).min(1.0);
                            let cur_x = start_slot_x + (end_slot_x - start_slot_x) * slide_progress;
                            let cur_y = start_slot_y + (end_slot_y - start_slot_y) * slide_progress;

                            let file_rect = Rect::from_min_size(
                                Pos2::new(cur_x, cur_y),
                                Vec2::new(16.0, 16.0),
                            );

                            painter.image(
                                textures.file_icon.id(),
                                file_rect,
                                Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                                Color32::WHITE,
                            );

                            // When slotted in (progress > 0.55), golden vector sparkle star pops up!
                            if progress > 0.55 {
                                let sparkle_alpha = ((1.0 - (progress - 0.55) / 0.45) * 255.0) as u8;
                                let gold_color = Color32::from_rgba_unmultiplied(255, 215, 0, sparkle_alpha);
                                PetController::draw_sparkle_star(
                                    painter,
                                    Pos2::new(folder_local_x + 8.0, folder_local_y - 10.0),
                                    6.5,
                                    gold_color,
                                );

                                let check_color = Color32::from_rgba_unmultiplied(80, 225, 120, sparkle_alpha);
                                let ck_cx = folder_local_x + 8.0;
                                let ck_cy = folder_local_y - 20.0;
                                // Vector checkmark lines (never renders as a square tofu box!)
                                painter.line_segment(
                                    [Pos2::new(ck_cx - 20.0, ck_cy), Pos2::new(ck_cx - 17.0, ck_cy + 3.0)],
                                    egui::Stroke::new(1.8_f32, check_color),
                                );
                                painter.line_segment(
                                    [Pos2::new(ck_cx - 17.0, ck_cy + 3.0), Pos2::new(ck_cx - 11.5, ck_cy - 4.5)],
                                    egui::Stroke::new(1.8_f32, check_color),
                                );

                                painter.text(
                                    Pos2::new(ck_cx + 4.0, ck_cy),
                                    egui::Align2::CENTER_CENTER,
                                    "Sorted!",
                                    egui::FontId::proportional(11.0),
                                    check_color,
                                );
                            }
                        }

                        // --- 10. Alert exclamation mark ---
                        if self.state == PetState::Alert {
                            painter.text(
                                Pos2::new(pet_rect.center().x, pet_rect.top() + 6.0),
                                egui::Align2::CENTER_CENTER,
                                "!",
                                egui::FontId::proportional(16.0),
                                Color32::from_rgb(255, 205, 50),
                            );
                        }
                    },
                );
            },
        );

        // The pet overlay must never appear in the taskbar or Activities
        // overview. winit ignores `with_taskbar(false)` on Linux, so set the
        // EWMH state ourselves once the window exists (re-applied periodically
        // in `refresh_taskbar`).
        if !self.pet_window_styled {
            taskbar::x11_set_skip_taskbar("DirWatcherPet", true);
            self.pet_window_styled = true;
        }
        // The window may not be registered with the WM on the first attempt, so
        // re-assert for a short burst after creation (then `refresh_taskbar`
        // keeps it applied periodically). Without this the pet can flash in the
        // dock for a few seconds at login.
        if self.pet_style_burst < 120 {
            self.pet_style_burst += 1;
            taskbar::x11_set_skip_taskbar("DirWatcherPet", true);
        }

        // Shape the pet window's clickable area: the sprite, the desk and any
        // landed papers intercept clicks (needed to drag the pet and clear
        // papers on GNOME/XWayland); the rest of the overlay stays
        // click-through.
        if self.x11_shaped_input {
            self.apply_x11_input_regions(strip_x, strip_y, ppp);
        }
    }

    /// Compute the clickable input rects of the overlay, in window-local
    /// physical pixels (strip-local logical coordinates scaled by `ppp`).
    fn x11_input_regions(&self, strip_x: f32, strip_y: f32, ppp: f32) -> Vec<(i32, i32, i32, i32)> {
        const PAD: f32 = 4.0; // comfortable click margin
        let mut rects = Vec::new();

        // The pet sprite (covers the Zzz particles and the carried stack).
        let char_w = self.spec.frame_width as f32 * self.scale;
        let char_h = self.spec.frame_height as f32 * self.scale;
        rects.push(physical_rect(
            self.current_x - strip_x - PAD,
            self.current_y - strip_y - PAD,
            char_w + 2.0 * PAD,
            char_h + 2.0 * PAD,
            ppp,
        ));

        // The folder desk.
        rects.push(physical_rect(
            self.folder_x - strip_x - PAD,
            (self.current_y + 14.0) - strip_y - PAD,
            16.0 + 2.0 * PAD,
            16.0 + 2.0 * PAD,
            ppp,
        ));

        // Landed papers (clickable to clear a blocked path).
        for paper in &self.ground_papers {
            if paper.landed {
                rects.push(physical_rect(
                    paper.x - 8.0 - strip_x - PAD,
                    paper.current_y - strip_y - PAD,
                    16.0 + 2.0 * PAD,
                    16.0 + 2.0 * PAD,
                    ppp,
                ));
            }
        }

        rects
    }

    /// Push the current input rects to the X server when they changed (and
    /// re-assert them periodically in case the compositor dropped the shape).
    fn apply_x11_input_regions(&mut self, strip_x: f32, strip_y: f32, ppp: f32) {
        let rects = self.x11_input_regions(strip_x, strip_y, ppp);
        let changed = rects != self.last_input_regions;
        let stale = self.last_shape_apply.elapsed() > Duration::from_secs(3);
        if !changed && !stale {
            return;
        }
        if changed {
            self.last_input_regions = rects.clone();
        }
        self.last_shape_apply = Instant::now();
        taskbar::x11_set_input_regions("DirWatcherPet", &rects);
    }
}

/// Logical strip-space rect → window-local physical pixel rect.
fn physical_rect(x: f32, y: f32, w: f32, h: f32, ppp: f32) -> (i32, i32, i32, i32) {
    (
        (x * ppp).floor() as i32,
        (y * ppp).floor() as i32,
        (w * ppp).ceil() as i32,
        (h * ppp).ceil() as i32,
    )
}

/// A pointer position in the pet viewport (logical strip-local points, where
/// the strip origin is `(strip_x, strip_y)`) → screen-logical coordinates, the
/// same space the pet controller uses for `current_x` / `folder_x` etc.
fn screen_point(pos: egui::Pos2, strip_x: f32, strip_y: f32, ppp: f32) -> (f32, f32) {
    ((strip_x + pos.x) * ppp, (strip_y + pos.y) * ppp)
}

