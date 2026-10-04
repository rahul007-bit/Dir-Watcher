//! Spritesheet texture management, frame UV calculations, and rendering.
#![allow(dead_code)]

use std::time::Instant;
use eframe::egui::{self, Pos2, Rect, TextureHandle, TextureOptions};
use super::character::{AnimationDef, CharacterSpec, FILE_ICON, ZZZ_SHEET};

pub struct AnimationPlayer {
    current_frame: usize,
    last_tick: Instant,
    accumulated_ms: u64,
    finished: bool,
}

impl AnimationPlayer {
    pub fn new() -> Self {
        Self {
            current_frame: 0,
            last_tick: Instant::now(),
            accumulated_ms: 0,
            finished: false,
        }
    }

    pub fn reset(&mut self) {
        self.current_frame = 0;
        self.last_tick = Instant::now();
        self.accumulated_ms = 0;
        self.finished = false;
    }

    /// Advance the animation by elapsed real time.
    /// Returns the recommended sleep duration until the next frame.
    pub fn update(&mut self, anim: &AnimationDef) -> std::time::Duration {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_tick);
        self.last_tick = now;

        self.accumulated_ms += elapsed.as_millis() as u64;

        if anim.frame_ms > 0 {
            while self.accumulated_ms >= anim.frame_ms {
                self.accumulated_ms -= anim.frame_ms;
                if self.current_frame + 1 < anim.frame_count {
                    self.current_frame += 1;
                } else if anim.loops {
                    self.current_frame = 0;
                } else {
                    self.finished = true;
                    break;
                }
            }
        }

        let remaining = anim.frame_ms.saturating_sub(self.accumulated_ms);
        std::time::Duration::from_millis(remaining.max(16))
    }

    pub fn current_frame(&self) -> usize {
        self.current_frame
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }
}

pub struct PetTextures {
    pub character: TextureHandle,
    pub file_icon: TextureHandle,
    pub folder_slot: TextureHandle,
    pub zzz: TextureHandle,
    pub char_sheet_w: f32,
    pub char_sheet_h: f32,
}

impl PetTextures {
    pub fn load(ctx: &egui::Context, spec: &CharacterSpec) -> Self {
        let char_img = load_image_from_bytes(spec.sheet_bytes).expect("Failed to load character sheet");
        let (cw, ch) = (char_img.width() as f32, char_img.height() as f32);
        let char_tex = ctx.load_texture("pet_character", char_img, TextureOptions::NEAREST);

        let file_img = load_image_from_bytes(FILE_ICON).expect("Failed to load file icon");
        let file_tex = ctx.load_texture("pet_file_icon", file_img, TextureOptions::NEAREST);

        let folder_img = load_image_from_bytes(super::character::FOLDER_SLOT).expect("Failed to load folder slot");
        let folder_tex = ctx.load_texture("pet_folder_slot", folder_img, TextureOptions::NEAREST);

        let zzz_img = load_image_from_bytes(ZZZ_SHEET).expect("Failed to load zzz sheet");
        let zzz_tex = ctx.load_texture("pet_zzz", zzz_img, TextureOptions::NEAREST);

        Self {
            character: char_tex,
            file_icon: file_tex,
            folder_slot: folder_tex,
            zzz: zzz_tex,
            char_sheet_w: cw,
            char_sheet_h: ch,
        }
    }

    /// Calculate UV coordinates for a frame in the character sheet.
    pub fn char_uv(&self, spec: &CharacterSpec, anim: &AnimationDef, frame_idx: usize, facing_left: bool) -> Rect {
        let col = anim.start_col + (frame_idx % anim.frame_count);
        let row = anim.row;

        let u0 = (col as f32 * spec.frame_width as f32) / self.char_sheet_w;
        let u1 = ((col + 1) as f32 * spec.frame_width as f32) / self.char_sheet_w;
        let v0 = (row as f32 * spec.frame_height as f32) / self.char_sheet_h;
        let v1 = ((row + 1) as f32 * spec.frame_height as f32) / self.char_sheet_h;

        if facing_left {
            // Flip horizontally
            Rect::from_min_max(Pos2::new(u1, v0), Pos2::new(u0, v1))
        } else {
            Rect::from_min_max(Pos2::new(u0, v0), Pos2::new(u1, v1))
        }
    }
}

fn load_image_from_bytes(bytes: &[u8]) -> Result<egui::ColorImage, String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let pixels = rgba.into_raw();
    Ok(egui::ColorImage::from_rgba_unmultiplied(size, &pixels))
}
