//! Character definitions and embedded spritesheet assets.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CharacterKind {
    #[default]
    Slime,
    Neko,
    Oneko,
    Dog,
}

impl CharacterKind {
    pub fn display_name(&self) -> &'static str {
        match self {
            CharacterKind::Slime => "White Mochi Slime",
            CharacterKind::Neko => "Pixel Neko (Calico)",
            CharacterKind::Oneko => "Classic Oneko (1989)",
            CharacterKind::Dog => "Shiba Inu",
        }
    }
}

/// Description of an animation strip within a spritesheet.
#[derive(Debug, Clone, Copy)]
pub struct AnimationDef {
    /// Row index in the spritesheet (0-indexed).
    pub row: usize,
    /// Starting frame column index.
    pub start_col: usize,
    /// Number of frames in this animation.
    pub frame_count: usize,
    /// Duration per frame in milliseconds.
    pub frame_ms: u64,
    /// Whether the animation loops indefinitely.
    pub loops: bool,
}

pub struct CharacterSpec {
    pub kind: CharacterKind,
    pub frame_width: u32,
    pub frame_height: u32,
    /// Sprite sheet raw PNG bytes.
    pub sheet_bytes: &'static [u8],
    /// File carry offset relative to sprite center (in unscaled pixels).
    pub carry_offset: (f32, f32),
    pub anim_idle: AnimationDef,
    pub anim_walk: AnimationDef,
    pub anim_sleep: AnimationDef,
    pub anim_alert: AnimationDef,
    pub anim_drop: AnimationDef,
}

// Embedded assets
pub const SLIME_SHEET: &[u8] = include_bytes!("../../assets/pet/slime_white_mochi.png");
pub const NEKO_SHEET: &[u8] = include_bytes!("../../assets/pet/neko.png");
pub const ONEKO_SHEET: &[u8] = include_bytes!("../../assets/pet/oneko.png");
pub const FILE_ICON: &[u8] = include_bytes!("../../assets/pet/paper_realistic.png");
pub const ZZZ_SHEET: &[u8] = include_bytes!("../../assets/pet/zzz_particles.png");
pub const FOLDER_SLOT: &[u8] = include_bytes!("../../assets/pet/folder_slot.png");

impl CharacterSpec {
    pub fn for_kind(kind: CharacterKind) -> Self {
        match kind {
            CharacterKind::Slime => Self::slime(),
            CharacterKind::Neko => Self::neko(),
            CharacterKind::Oneko => Self::oneko(),
            CharacterKind::Dog => Self::dog_placeholder(),
        }
    }

    /// White Mochi Slime layout (32x32 frames, 10 frames per row):
    /// Row 0: Idle (10 frames) - gentle wobble and blinking
    /// Row 1: Alert (10 frames) - eyes pop open, surprise hop with '!'
    /// Row 2: Walk / Hop (10 frames) - bouncy squash & stretch
    /// Row 3: Drop / Toss (10 frames) - happy bow & file toss
    /// Row 4: Sleep (10 frames) - plump cozy mochi loaf breathing gently
    fn slime() -> Self {
        Self {
            kind: CharacterKind::Slime,
            frame_width: 32,
            frame_height: 32,
            sheet_bytes: SLIME_SHEET,
            carry_offset: (0.0, -18.0),
            anim_idle: AnimationDef {
                row: 0,
                start_col: 0,
                frame_count: 10,
                frame_ms: 140,
                loops: true,
            },
            anim_alert: AnimationDef {
                row: 1,
                start_col: 0,
                frame_count: 10,
                frame_ms: 70,
                loops: false,
            },
            anim_walk: AnimationDef {
                row: 2,
                start_col: 0,
                frame_count: 10,
                frame_ms: 80,
                loops: true,
            },
            anim_drop: AnimationDef {
                row: 3,
                start_col: 0,
                frame_count: 10,
                frame_ms: 75,
                loops: false,
            },
            anim_sleep: AnimationDef {
                row: 4,
                start_col: 0,
                frame_count: 10,
                frame_ms: 220,
                loops: true,
            },
        }
    }

    /// Pixel Neko layout (Japanese Calico cat, 32x32 frames, 10 frames per row):
    /// Row 0: Idle (10 frames) - sitting loaf, ear twitches, blinking, tail swishes
    /// Row 1: Alert (10 frames) - startled ears, crouch, playful pounce hop with '!'
    /// Row 2: Walk (10 frames) - 4-legged feline trot/prowl, bouncy tail, jingle bell
    /// Row 3: Drop / Toss (10 frames) - playful two-paw swat into folder, paw lick
    /// Row 4: Sleep (10 frames) - curled-up cozy cat loaf, wrapped tail, rhythmic breathing
    pub fn neko() -> Self {
        Self {
            kind: CharacterKind::Neko,
            frame_width: 32,
            frame_height: 32,
            sheet_bytes: NEKO_SHEET,
            carry_offset: (0.0, -18.0),
            anim_idle: AnimationDef {
                row: 0,
                start_col: 0,
                frame_count: 10,
                frame_ms: 140,
                loops: true,
            },
            anim_alert: AnimationDef {
                row: 1,
                start_col: 0,
                frame_count: 10,
                frame_ms: 70,
                loops: false,
            },
            anim_walk: AnimationDef {
                row: 2,
                start_col: 0,
                frame_count: 10,
                frame_ms: 90,
                loops: true,
            },
            anim_drop: AnimationDef {
                row: 3,
                start_col: 0,
                frame_count: 10,
                frame_ms: 85,
                loops: false,
            },
            anim_sleep: AnimationDef {
                row: 4,
                start_col: 0,
                frame_count: 10,
                frame_ms: 220,
                loops: true,
            },
        }
    }

    /// Classic Oneko layout (1989 X11 cat by Masayuki Koba, 32x32 frames, 10 frames per row):
    /// Row 0: Idle (10 frames) - sit, paw lick, ear scratch, yawn
    /// Row 1: Alert (10 frames) - surprised (!) alert, hop, ready stance
    /// Row 2: Walk (10 frames) - classic retro 2-step run cycle (8 FPS)
    /// Row 3: Drop / Toss / Scratch (10 frames) - upward clawing & scratch, paw wash
    /// Row 4: Sleep (10 frames) - curled up sleeping cat with alternating Zzz
    pub fn oneko() -> Self {
        Self {
            kind: CharacterKind::Oneko,
            frame_width: 32,
            frame_height: 32,
            sheet_bytes: ONEKO_SHEET,
            carry_offset: (0.0, -18.0),
            anim_idle: AnimationDef {
                row: 0,
                start_col: 0,
                frame_count: 10,
                frame_ms: 180,
                loops: true,
            },
            anim_alert: AnimationDef {
                row: 1,
                start_col: 0,
                frame_count: 10,
                frame_ms: 90,
                loops: false,
            },
            anim_walk: AnimationDef {
                row: 2,
                start_col: 0,
                frame_count: 10,
                frame_ms: 125, // 8 FPS
                loops: true,
            },
            anim_drop: AnimationDef {
                row: 3,
                start_col: 0,
                frame_count: 10,
                frame_ms: 90,
                loops: false,
            },
            anim_sleep: AnimationDef {
                row: 4,
                start_col: 0,
                frame_count: 10,
                frame_ms: 250,
                loops: true,
            },
        }
    }

    /// Dog placeholder fallback
    fn dog_placeholder() -> Self {
        let mut spec = Self::slime();
        spec.kind = CharacterKind::Dog;
        spec
    }
}
