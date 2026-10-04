//! Desktop companion (Pet) controller and state machine.

pub mod animation;
pub mod character;
pub mod taskbar;

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
    pub folder_alpha: f32, // Folder stationed at home base
    pub entrance_t: f32,   // 0.0 to 1.0 (smooth rise from taskbar at startup)
    pub is_overwhelmed: bool,
    pub sweat_timer: f32,
    pub thank_timer: f32,  // Heart / Thank you badge duration
    pub window_initialized: bool,
    pub sparkle_bursts: Vec<(f32, f32, f32)>, // (x, y, remaining_secs)

    // Coordinate scaling and positioning
    pub ppp: f32,
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
            folder_alpha: 1.0,
            entrance_t: 1.0, // Instantly ready, no weird offset
            is_overwhelmed: false,
            sweat_timer: 0.0,
            thank_timer: 0.0,
            window_initialized: false,
            sparkle_bursts: Vec::new(),
            ppp: 1.0,
            was_lbutton_down: false,
            speech_text: None,
            speech_timer: 0.0,
            position_offset: 0.0,
            is_dragging_desk: false,
            drag_start_cursor_x: 0.0,
            drag_start_offset: 0.0,
        }
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

        let quotes: &[&str] = match context {
            DialogueContext::Stuck => &[
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
            DialogueContext::Helped => &[
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

    /// Refresh taskbar coordinates using logical DPI scale factor.
    pub fn refresh_taskbar_coords(&mut self) {
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
            if self.state == PetState::Sleeping {
                self.current_x = self.home_x;
            }
        }
    }

    /// Refresh taskbar periodically if it moved or display changed.
    pub fn refresh_taskbar(&mut self) {
        if self.last_taskbar_check.elapsed() > Duration::from_secs(3) {
            self.last_taskbar_check = Instant::now();
            self.refresh_taskbar_coords();
        }
    }

    /// Spawn a fluttering falling paper at a specific x coordinate.
    pub fn spawn_paper_at(&mut self, name: String, drop_x: f32) {
        let start_y = self.current_y - 85.0; // High in the air
        let target_y = self.current_y + 14.0; // Exactly on taskbar baseline (matches 16px paper height)
        let flutter_t = self.pseudo_random(0.0, 6.28);
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

        // If sleeping and papers appear, wake up!
        if self.state == PetState::Sleeping {
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

        let lbutton_down = taskbar::is_lbutton_pressed();
        let lbutton_clicked = lbutton_down && !self.was_lbutton_down;
        self.was_lbutton_down = lbutton_down;

        // --- Drag & drop Mochi and folder desk together when Mochi is sleeping ---
        if self.state == PetState::Sleeping {
            if let Some((phys_x, phys_y)) = taskbar::get_global_cursor_pos() {
                let ppp = self.ppp.max(0.5);
                let cursor_x = phys_x / ppp;
                let cursor_y = phys_y / ppp;

                if lbutton_down {
                    if !self.is_dragging_desk {
                        // Check if cursor clicked within Mochi or folder desk area
                        let desk_min_x = self.home_x - 14.0;
                        let desk_max_x = self.folder_x + 30.0;
                        let desk_min_y = self.current_y - 12.0;
                        let desk_max_y = self.current_y + 36.0;

                        if cursor_x >= desk_min_x && cursor_x <= desk_max_x && cursor_y >= desk_min_y && cursor_y <= desk_max_y {
                            self.is_dragging_desk = true;
                            self.drag_start_cursor_x = cursor_x;
                            self.drag_start_offset = self.position_offset;
                        }
                    } else {
                        let delta = cursor_x - self.drag_start_cursor_x;
                        self.position_offset = (self.drag_start_offset + delta).clamp(-1200.0, 300.0);
                        self.refresh_taskbar_coords();
                    }
                } else if self.is_dragging_desk {
                    self.is_dragging_desk = false;
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

                    // Check if there are still any papers blocking the doorway
                    let still_blocked = self.ground_papers.iter().any(|p| p.landed && (p.x - self.folder_x).abs() <= 28.0);

                    // Only unblock Mochi and say thank you once ALL blocking papers are cleared!
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
                if self.is_dragging_desk {
                    Duration::from_millis(16)
                } else {
                    delay.max(Duration::from_millis(250))
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
            return;
        }

        let ppp = ctx.pixels_per_point().max(0.5);
        if (self.ppp - ppp).abs() > 0.01 || !self.window_initialized {
            self.ppp = ppp;
            self.refresh_taskbar_coords();
            self.window_initialized = true;
        }

        if self.textures.is_none() {
            self.textures = Some(PetTextures::load(ctx, &self.spec));
        }
        let textures = self.textures.as_ref().unwrap();

        let anim_def = match self.state {
            PetState::Sleeping => &self.spec.anim_sleep,
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

        // Viewport bounds: transparent strip covering full paper drop and walk zone
        let strip_x = (self.folder_x - 320.0).min(self.current_x - 40.0).max(0.0);
        let strip_w = ((self.folder_x + 80.0) - strip_x).max(420.0);
        let strip_y = self.current_y - 85.0;
        let strip_h = 145.0;

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("desktop_pet_viewport"),
            egui::ViewportBuilder::default()
                .with_title("DirWatcherPet")
                .with_transparent(true)
                .with_decorations(false)
                .with_always_on_top()
                .with_taskbar(false)
                .with_mouse_passthrough(true) // Full desktop click-through
                .with_inner_size([strip_w, strip_h])
                .with_position([strip_x, strip_y]),
            |sub_ctx, _class| {
                // Apply desktop transparency styles to the HWND
                taskbar::apply_pet_window_transparency("DirWatcherPet");

                sub_ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(true));
                sub_ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
                sub_ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));

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
    }
}

