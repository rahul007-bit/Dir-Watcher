//! Standalone preview and interactive test harness for Neko & Classic Oneko.
//! Does not modify main.rs or app.rs.
//! Run with: cargo run --example neko_preview

#[path = "../src/pet/mod.rs"]
mod pet;

use std::sync::mpsc::{channel, Sender};
use std::time::Instant;
use eframe::egui::{self, CentralPanel, Color32, RichText, ViewportBuilder};
use pet::character::CharacterKind;
use pet::taskbar;
use pet::{DialogueContext, PetController, PetEvent, PetState};

fn main() -> eframe::Result<()> {
    let (tx, rx) = channel::<PetEvent>();

    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("Neko & Oneko Preview - Companion Test Harness")
            .with_inner_size([520.0, 480.0])
            .with_transparent(true),
        ..Default::default()
    };

    eframe::run_native(
        "Neko Preview",
        options,
        Box::new(|_cc| Ok(Box::new(NekoPreviewApp::new(tx, rx)))),
    )
}

struct NekoPreviewApp {
    pet: PetController,
    tx: Sender<PetEvent>,
    simulated_counter: usize,
    forced_state: Option<PetState>,

    // Mouse Chaser Mode (classic oneko behavior)
    mouse_chaser_mode: bool,
    chase_alert_timer: f32,
    last_chase_tick: Instant,
    last_cursor_dist: f32,
}

impl NekoPreviewApp {
    fn new(tx: Sender<PetEvent>, rx: std::sync::mpsc::Receiver<PetEvent>) -> Self {
        let pet = PetController::new_with_kind(Some(rx), CharacterKind::Neko);
        Self {
            pet,
            tx,
            simulated_counter: 1,
            forced_state: None,
            mouse_chaser_mode: false,
            chase_alert_timer: 0.0,
            last_chase_tick: Instant::now(),
            last_cursor_dist: 0.0,
        }
    }
}

impl eframe::App for NekoPreviewApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_chase_tick).as_secs_f32().min(0.1);
        self.last_chase_tick = now;

        // --- Mouse Cursor Chaser Mode (Classic Oneko) ---
        if self.mouse_chaser_mode && self.forced_state.is_none() {
            if let Some((phys_x, phys_y)) = taskbar::get_global_cursor_pos() {
                let ppp = self.pet.ppp.max(0.5);
                let target_x = phys_x / ppp;
                // Target slightly offset above the mouse tip so Neko doesn't obscure the cursor
                let target_y = (phys_y / ppp) - 16.0;

                let dx = target_x - self.pet.current_x;
                let dy = target_y - self.pet.current_y;
                let dist = (dx * dx + dy * dy).sqrt();
                self.last_cursor_dist = dist;

                let catch_dist = 28.0;
                let trigger_dist = 48.0;

                if dist > trigger_dist {
                    if self.pet.state == PetState::Sleeping {
                        // Startled! Play alert jump with '!'
                        self.pet.state = PetState::Alert;
                        self.chase_alert_timer = 0.35;
                        self.pet.player.reset();
                    } else if self.chase_alert_timer > 0.0 {
                        self.chase_alert_timer -= dt;
                        if self.chase_alert_timer <= 0.0 {
                            self.pet.state = PetState::WalkingToTray;
                            self.pet.player.reset();
                        }
                    } else {
                        // Running towards the mouse cursor
                        self.pet.state = PetState::WalkingToTray;
                        let run_speed = (self.pet.speed * 2.2).max(65.0);
                        let step = (run_speed * dt).min(dist);
                        self.pet.current_x += (dx / dist) * step;
                        self.pet.current_y += (dy / dist) * step;
                        self.pet.facing_left = dx < 0.0;
                    }
                } else if dist <= catch_dist {
                    // Reached the cursor! Cat rests next to mouse
                    if self.pet.state != PetState::Sleeping {
                        self.pet.state = PetState::Sleeping;
                        self.pet.player.reset();
                    }
                }
            }
        }

        // If a state is manually forced for visual inspection, pin the pet state
        if let Some(forced) = self.forced_state {
            self.pet.state = forced;
        }

        // Dynamic repaint delay
        let delay = if self.mouse_chaser_mode && self.pet.state != PetState::Sleeping {
            std::time::Duration::from_millis(16) // Smooth 60 FPS while chasing mouse
        } else {
            self.pet.update()
        };
        ctx.request_repaint_after(delay);

        // Render the desktop pet viewport
        self.pet.render(ctx);

        // Control Panel UI
        CentralPanel::default().show(ctx, |ui| {
            ui.heading(RichText::new("Neko & Classic Oneko Test Bench").strong());
            ui.label("Independent test harness for Calico Neko & 1989 Classic Oneko.");
            ui.add_space(6.0);

            // Character Switcher
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label("Companion Character:");
                    let is_neko = self.pet.spec.kind == CharacterKind::Neko;
                    if ui.selectable_label(is_neko, "Neko (Modern Calico)").clicked() {
                        self.pet.set_character(CharacterKind::Neko);
                    }
                    let is_oneko = self.pet.spec.kind == CharacterKind::Oneko;
                    if ui.selectable_label(is_oneko, "Classic Oneko (1989)").clicked() {
                        self.pet.set_character(CharacterKind::Oneko);
                    }
                    let is_slime = self.pet.spec.kind == CharacterKind::Slime;
                    if ui.selectable_label(is_slime, "White Mochi (Slime)").clicked() {
                        self.pet.set_character(CharacterKind::Slime);
                    }
                    let is_hamster = self.pet.spec.kind == CharacterKind::Hamster;
                    if ui.selectable_label(is_hamster, "Chubby Hamster").clicked() {
                        self.pet.set_character(CharacterKind::Hamster);
                    }
                });
            });

            ui.add_space(6.0);

            // Mode Selector: File Deliverer vs Mouse Cursor Chaser
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Behavior Mode:").strong());
                    if ui.selectable_label(!self.mouse_chaser_mode, "Taskbar File Deliverer").clicked() {
                        self.mouse_chaser_mode = false;
                        self.pet.refresh_taskbar_coords();
                        self.pet.state = PetState::Sleeping;
                    }
                    if ui.selectable_label(self.mouse_chaser_mode, "Mouse Cursor Chaser (Classic Oneko)").clicked() {
                        self.mouse_chaser_mode = true;
                        self.forced_state = None;
                    }
                });
                if self.mouse_chaser_mode {
                    ui.label(
                        RichText::new("Classic Oneko Mode ACTIVE: Move your mouse anywhere on your screen and watch the cat wake up and chase it!")
                            .color(Color32::from_rgb(100, 200, 255))
                    );
                    ui.label(format!("Cursor Distance: {:.1} px", self.last_cursor_dist));
                }
            });

            ui.add_space(6.0);

            // Status Panel
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Current State: {:?}", self.pet.state)).strong());
                    if self.forced_state.is_some() {
                        ui.label(RichText::new("[FORCED PREVIEW]").color(Color32::from_rgb(255, 180, 50)));
                        if ui.button("Resume Normal AI").clicked() {
                            self.forced_state = None;
                            self.pet.state = PetState::Sleeping;
                        }
                    }
                });
                ui.label(format!(
                    "Position X: {:.1}, Y: {:.1} | Frame: {} | Facing Left: {}",
                    self.pet.current_x, self.pet.current_y, self.pet.player.current_frame(), self.pet.facing_left
                ));
                ui.label(format!("Speed: {:.0} px/s, Scale: {:.1}x", self.pet.speed, self.pet.scale));
            });

            ui.add_space(8.0);

            // Animation Inspector
            ui.label(RichText::new("Inspect Individual Animations:").strong());
            ui.horizontal_wrapped(|ui| {
                if ui.button("Idle (Loaf/Wash)").clicked() {
                    self.mouse_chaser_mode = false;
                    self.forced_state = Some(PetState::Sleeping);
                    self.pet.player.reset();
                }
                if ui.button("Alert (Pounce / !) ").clicked() {
                    self.mouse_chaser_mode = false;
                    self.forced_state = Some(PetState::Alert);
                    self.pet.player.reset();
                }
                if ui.button("Walk (4-Leg Trot)").clicked() {
                    self.mouse_chaser_mode = false;
                    self.forced_state = Some(PetState::WalkingToTray);
                    self.pet.player.reset();
                }
                if ui.button("Drop / Claw Swat").clicked() {
                    self.mouse_chaser_mode = false;
                    self.forced_state = Some(PetState::Arranging);
                    self.pet.player.reset();
                }
                if ui.button("Groom").clicked() {
                    self.mouse_chaser_mode = false;
                    self.forced_state = Some(PetState::Grooming);
                    self.pet.player.reset();
                }
                if ui.button("Sleep (Curled Loaf)").clicked() {
                    self.mouse_chaser_mode = false;
                    self.forced_state = Some(PetState::Sleeping);
                    self.pet.player.reset();
                }
            });

            ui.add_space(8.0);

            // Real Paper Simulation Controls
            ui.label(RichText::new("File Delivery Simulation:").strong());
            ui.horizontal_wrapped(|ui| {
                if ui.button(RichText::new("Drop 1 File").color(Color32::from_rgb(60, 200, 110))).clicked() {
                    self.forced_state = None;
                    self.mouse_chaser_mode = false;
                    let filename = format!("invoice_{}.pdf", self.simulated_counter);
                    self.simulated_counter += 1;
                    let _ = self.tx.send(PetEvent::NewFile(filename));
                }

                if ui.button(RichText::new("Drop 3 Files").color(Color32::from_rgb(80, 180, 240))).clicked() {
                    self.forced_state = None;
                    self.mouse_chaser_mode = false;
                    for _ in 0..3 {
                        let filename = format!("photo_{}.png", self.simulated_counter);
                        self.simulated_counter += 1;
                        let _ = self.tx.send(PetEvent::NewFile(filename));
                    }
                }

                if ui.button(RichText::new("Block Folder (Test Stuck)").color(Color32::from_rgb(255, 140, 50))).clicked() {
                    self.forced_state = None;
                    self.mouse_chaser_mode = false;
                    let folder_x = self.pet.folder_x;
                    let name_blocked = format!("huge_archive_{}.zip", self.simulated_counter);
                    self.simulated_counter += 1;
                    self.pet.spawn_paper_at(name_blocked, folder_x - 10.0);

                    let name_pickup = format!("doc_{}.txt", self.simulated_counter);
                    self.simulated_counter += 1;
                    self.pet.spawn_paper_at(name_pickup, folder_x - 140.0);
                }

                let can_help = !self.pet.ground_papers.is_empty() || !self.pet.carried_stack.is_empty();
                if can_help {
                    let text = if self.pet.state == PetState::Stuck {
                        RichText::new("[Help] Clear Stuck File").color(Color32::from_rgb(255, 70, 70)).strong()
                    } else {
                        RichText::new("[Help] Help Clean").color(Color32::from_rgb(255, 180, 50))
                    };
                    if ui.button(text).clicked() {
                        self.pet.help_clean();
                    }
                }

                if ui.button("Test Cat Dialogue").clicked() {
                    self.pet.trigger_dialogue(DialogueContext::Helped);
                }
            });

            ui.add_space(8.0);

            // Sliders
            ui.horizontal(|ui| {
                ui.label("Speed:");
                ui.add(egui::Slider::new(&mut self.pet.speed, 15.0..=120.0).text("px/s"));
            });

            ui.horizontal(|ui| {
                ui.label("Scale:");
                ui.add(egui::Slider::new(&mut self.pet.scale, 1.0..=3.0).text("x"));
            });

            if !self.mouse_chaser_mode {
                ui.horizontal(|ui| {
                    ui.label("Desk Position Offset:");
                    let prev = self.pet.position_offset;
                    if ui.add(egui::Slider::new(&mut self.pet.position_offset, -400.0..=100.0).text("px")).changed() {
                        let delta = self.pet.position_offset - prev;
                        self.pet.folder_x += delta;
                        self.pet.home_x += delta;
                        if self.pet.state == PetState::Sleeping {
                            self.pet.current_x += delta;
                        }
                    }
                });
            }

            ui.add_space(6.0);
            if self.pet.state == PetState::Stuck {
                ui.label(RichText::new("[!] Companion is STUCK! Path is blocked. Click the glowing paper or press Help!").color(Color32::from_rgb(255, 90, 90)).strong());
            } else if self.pet.state == PetState::Sleeping {
                ui.label(RichText::new("Status: Sleeping peacefully curled in a warm loaf (0% CPU)").italics());
            } else if self.mouse_chaser_mode {
                ui.label(RichText::new("Status: Chasing mouse cursor across the desktop!").color(Color32::from_rgb(100, 200, 255)));
            } else {
                ui.label(RichText::new("Status: On the prowl delivering documents!").color(Color32::from_rgb(255, 180, 50)));
            }
        });
    }
}
