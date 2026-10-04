//! Standalone preview and testing harness for the desktop pet.
//! Does not modify main.rs or app.rs.
//! Run with: cargo run --example pet_preview

#[path = "../src/pet/mod.rs"]
mod pet;

use std::sync::mpsc::{channel, Sender};
use eframe::egui::{self, CentralPanel, Color32, RichText, ViewportBuilder};
use pet::{PetController, PetEvent, PetState};

fn main() -> eframe::Result<()> {
    let (tx, rx) = channel::<PetEvent>();

    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("Desktop Pet - Standalone Test Harness")
            .with_inner_size([460.0, 320.0])
            .with_transparent(true),
        ..Default::default()
    };

    eframe::run_native(
        "Pet Preview",
        options,
        Box::new(|_cc| Ok(Box::new(PreviewApp::new(tx, rx)))),
    )
}

struct PreviewApp {
    pet: PetController,
    tx: Sender<PetEvent>,
    simulated_counter: usize,
}

impl PreviewApp {
    fn new(tx: Sender<PetEvent>, rx: std::sync::mpsc::Receiver<PetEvent>) -> Self {
        Self {
            pet: PetController::new(Some(rx)),
            tx,
            simulated_counter: 1,
        }
    }
}

impl eframe::App for PreviewApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Update the pet logic and get the dynamic repaint delay.
        // During Sleep, this requests long intervals (~300-500ms), giving 0% CPU!
        // During Walk, it dynamically switches to smooth 60 FPS (~16ms).
        let delay = self.pet.update();
        ctx.request_repaint_after(delay);

        // Render the desktop pet viewport
        self.pet.render(ctx);

        // Control Panel UI for testing
        CentralPanel::default().show(ctx, |ui| {
            ui.heading("Desktop Pet Test Bench");
            ui.label("This test runner lets you test animations without modifying main.rs or app.rs.");
            ui.add_space(8.0);

            ui.group(|ui| {
                ui.label(RichText::new(format!("Current State: {:?}", self.pet.state)).strong());
                ui.label(format!("Position X: {:.1}, Y: {:.1}", self.pet.current_x, self.pet.current_y));
                ui.label(format!("Speed: {:.0} px/s, Scale: {:.1}x", self.pet.speed, self.pet.scale));
                ui.label(format!("Repaint delay requested: {:?}", delay));
            });

            ui.add_space(10.0);

            ui.horizontal(|ui| {
                if ui.button(RichText::new("Drop 1 File").color(Color32::from_rgb(50, 200, 100))).clicked() {
                    let filename = format!("download_{}.pdf", self.simulated_counter);
                    self.simulated_counter += 1;
                    let _ = self.tx.send(PetEvent::NewFile(filename));
                }

                if ui.button(RichText::new("Drop 3 Files (Full Stack)").color(Color32::from_rgb(80, 180, 240))).clicked() {
                    for _ in 0..3 {
                        let filename = format!("paper_{}.png", self.simulated_counter);
                        self.simulated_counter += 1;
                        let _ = self.tx.send(PetEvent::NewFile(filename));
                    }
                }

                if ui.button("Drop 5 Files (Multi-Trip)").clicked() {
                    for _ in 0..5 {
                        let filename = format!("batch_{}.txt", self.simulated_counter);
                        self.simulated_counter += 1;
                        let _ = self.tx.send(PetEvent::NewFile(filename));
                    }
                }

                if ui.button(RichText::new("Drop at Folder (Test Stuck)").color(Color32::from_rgb(255, 140, 40))).clicked() {
                    let folder_x = self.pet.folder_x;
                    // Drop one paper directly blocking the folder slot
                    let name_blocked = format!("blocking_paper_{}.pdf", self.simulated_counter);
                    self.simulated_counter += 1;
                    self.pet.spawn_paper_at(name_blocked, folder_x - 10.0);

                    // And one paper further out so Mochi collects it and walks towards the folder
                    let name_pickup = format!("doc_{}.txt", self.simulated_counter);
                    self.simulated_counter += 1;
                    self.pet.spawn_paper_at(name_pickup, folder_x - 120.0);
                }

                let can_help = !self.pet.ground_papers.is_empty() || !self.pet.carried_stack.is_empty();
                if can_help {
                    let text = if self.pet.state == PetState::Stuck {
                        RichText::new("🖐 Help Mochi (Clear Stuck)").color(Color32::from_rgb(255, 70, 70)).strong()
                    } else if self.pet.is_overwhelmed {
                        RichText::new("🖐 Help Slime Clean! (Overwhelmed)").color(Color32::from_rgb(255, 90, 90)).strong()
                    } else {
                        RichText::new("🖐 Help Clean").color(Color32::from_rgb(255, 180, 50))
                    };
                    if ui.button(text).clicked() {
                        self.pet.help_clean();
                    }
                }
            });

            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.label("Speed:");
                ui.add(egui::Slider::new(&mut self.pet.speed, 15.0..=100.0).text("px/s"));
            });

            ui.horizontal(|ui| {
                ui.label("Scale:");
                ui.add(egui::Slider::new(&mut self.pet.scale, 1.0..=3.0).text("x"));
            });

            ui.add_space(10.0);
            if self.pet.state == PetState::Stuck {
                ui.label(RichText::new("⚠ Mochi is STUCK! Path is blocked. Click the glowing paper on your desktop to clear it!").color(Color32::from_rgb(255, 90, 90)).strong());
            } else if self.pet.state == PetState::Sleeping {
                ui.label(RichText::new("Status: Slime is sleeping peacefully (0% CPU)").italics());
            } else {
                ui.label(RichText::new("Status: Slime is actively delivering!").color(Color32::from_rgb(255, 180, 50)));
            }
        });
    }
}
