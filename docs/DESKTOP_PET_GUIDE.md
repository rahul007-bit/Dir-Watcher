# Desktop Companion (Pet) Generation & Architecture Guide

This guide documents the design system, sprite generation pipeline, and architectural lessons learned while developing **Mochi**, the desktop companion for `watch-folder`.

Use this guide to generate new pet characters (e.g. Cats, Shiba Inus, Bunnies, or Slime variants) and integrate them seamlessly into the desktop companion engine.

---

## 1. Spritesheet Specification

Every pet companion is driven by a single unified spritesheet formatted as follows:

- **Total Dimensions**: `320 x 192 px` (RGBA 32-bit PNG)
- **Grid Layout**: `10 columns x 6 rows`
- **Frame Size**: `32 x 32 px` per frame
- **Origin / Anchor**: Bottom-aligned at `y = 30` within each 32px frame, ensuring feet or base resting squarely on the taskbar top edge.

```
       Col 0   Col 1   Col 2   Col 3   Col 4   Col 5   Col 6   Col 7   Col 8   Col 9
Row 0: [------------------------- Idle (up to 10 frames) ---------------------]
Row 1: [------------------------- Alert (up to 10 frames) --------------------]
Row 2: [------------------------- Walk / Hop (up to 10 frames) ---------------]
Row 3: [------------------------- Drop / Toss (up to 10 frames) --------------]
Row 4: [------------------------- Sleep (up to 10 frames) --------------------]
Row 5: [------------------------- Groom (up to 10 frames) --------------------]
```

> Rows may use fewer than 10 frames; `AnimationDef::frame_count` controls playback
> (e.g. the AI-generated hamster uses 4–6 frames per row). Unused cells stay empty.

### Animation Row Breakdown

| Row | State | Frames | Timing (FPS) | Purpose & Movement Details |
|---|---|---|---|---|
| **0** | **Idle** | ≤10 | 6 FPS | Gentle resting wobble, soft breathing rhythm, occasional eye blink. |
| **1** | **Alert** | ≤10 | 8 FPS | Surprise hop when a new file lands: eyes pop wide, hops upward, lands upright. |
| **2** | **Walk / Hop** | ≤10 | 10 FPS | Forward locomotion: squash on takeoff, airborne stretch, and squash on landing. |
| **3** | **Drop / Toss**| ≤10 | 8 FPS | Delivery bow: arcs carried files neatly into the folder slot. |
| **4** | **Sleep** | ≤10 | 4 FPS | Peaceful loaf: eyes gently shut, breathing expansion. Never a flat puddle. |
| **5** | **Groom** | ≤10 | 7 FPS | Idle variant: washes its face / polishes itself. Alternates with Sleep while at home. |

### Bringing AI-generated art into the engine

Pets can be authored by an image model (e.g. Gemini "nano banana") instead of
drawn by hand. Ask for a **single grid**: 6 rows × N columns, one row per action
in the order above, on a **flat solid magenta `#FF00FF`** background, matching a
reference sprite's style. Then pack it:

```
python scripts/pack_generated.py --pet hamster \
    --grid assets/pet/src/gen/hamster/sheet.png --grid-rows 6 --grid-cols 4
```

The packer removes the background (including the magenta fringe), slices each
frame, applies a shared scale per row, aligns feet to `y = 30`, and writes
`assets/pet/<pet>.png`. Per-action strips named `idle/alert/walk/drop/sleep/groom`
under `--strips DIR` work too.

---

## 2. Generating New Pets with Python & Pillow

The easiest way to generate pixel-art spritesheets programmatically is using Python with `Pillow`.

Reference script: [`scripts/generate_white_slime.py`](file:///C:/Users/HP/Projects/Dir-Watcher/scripts/generate_white_slime.py).

### Template Code for a New Pet (`scripts/generate_custom_pet.py`):

```python
from PIL import Image, ImageDraw

SHEET_WIDTH = 320
SHEET_HEIGHT = 160
FRAME_SIZE = 32

sheet = Image.new('RGBA', (SHEET_WIDTH, SHEET_HEIGHT), (0, 0, 0, 0))
draw = ImageDraw.Draw(sheet)

# 1. Define High-Contrast Palette
OUTLINE   = (40, 45, 60, 255)      # Dark crisp boundary (visible on both dark/light desktops)
BASE_BODY = (245, 160, 80, 255)    # Main color (e.g. orange tabby cat)
SHADING   = (210, 120, 50, 255)    # Underside shadow
HIGHLIGHT = (255, 210, 150, 255)   # Top specular highlight
EYE_DARK  = (25, 25, 30, 255)      # Expressive eyes
ACCENT    = (255, 255, 255, 255)   # Paws / chest fluff

def draw_frame(col, row, ox=0, oy=0, w=20, h=14, eyes='open', sleeping=False):
    # Anchor: bottom baseline is row * 32 + 30
    x0 = col * FRAME_SIZE + (16 - w // 2) + ox
    y0 = row * FRAME_SIZE + (30 - h) + oy
    x1 = x0 + w
    y1 = y0 + h

    # Outer outline
    draw.rounded_rectangle([x0, y0, x1, y1], radius=w//3, fill=OUTLINE)
    # Inner body
    draw.rounded_rectangle([x0+1, y0+1, x1-1, y1-1], radius=(w//3)-1, fill=BASE_BODY)
    # Shadow & Highlights
    draw.rounded_rectangle([x0+2, y1-3, x1-2, y1-1], radius=2, fill=SHADING)
    draw.ellipse([x0+3, y0+2, x0+7, y0+5], fill=HIGHLIGHT)

    # Eyes & Expressions
    ey = y0 + h // 2
    if eyes == 'open':
        draw.rectangle([x0+6, ey-1, x0+7, ey+1], fill=EYE_DARK)
        draw.rectangle([x1-7, ey-1, x1-6, ey+1], fill=EYE_DARK)
    elif eyes == 'sleep':
        draw.line([(x0+5, ey), (x0+8, ey)], fill=EYE_DARK)
        draw.line([(x1-8, ey), (x1-5, ey)], fill=EYE_DARK)

# Generate all 5 rows (Row 0: Idle, Row 1: Alert, Row 2: Walk, Row 3: Toss, Row 4: Sleep)
# ...
sheet.save('assets/pets/orange_cat/spritesheet.png')
```

### Designing via Aseprite / Pixelorama
If drawing by hand:
1. Create a canvas of **320 x 160** pixels.
2. Turn on Grid: **32 x 32** pixels.
3. Ensure the character's lowest touching pixel is at **`y = 30`** of each 32px block (2px padding from bottom for shadow clearance).
4. Save directly as PNG with 32-bit RGBA transparency.

---

## 3. Registering the New Pet in Rust

In [`src/pet/character.rs`](file:///C:/Users/HP/Projects/Dir-Watcher/src/pet/character.rs):

1. Add a new variant to `CharacterKind`:
   ```rust
   pub enum CharacterKind {
       WhiteSlime,
       OrangeCat,
   }
   ```
2. Define the character spec:
   ```rust
   impl CharacterSpec {
       pub fn orange_cat() -> Self {
           Self {
               id: "orange_cat",
               display_name: "Mikan (Cat)",
               spritesheet_bytes: include_bytes!("../../assets/pets/orange_cat/spritesheet.png"),
               frame_width: 32,
               frame_height: 32,
               anim_idle:  AnimDef { row: 0, frame_count: 10, fps: 6.0, loop_anim: true },
               anim_alert: AnimDef { row: 1, frame_count: 10, fps: 8.0, loop_anim: false },
               anim_walk:  AnimDef { row: 2, frame_count: 10, fps: 10.0, loop_anim: true },
               anim_drop:  AnimDef { row: 3, frame_count: 10, fps: 8.0, loop_anim: false },
               anim_sleep: AnimDef { row: 4, frame_count: 10, fps: 4.0, loop_anim: true },
           }
       }
   }
   ```

---

## 4. Key Learnings & Pitfalls to Avoid

### 1. Win32 Desktop Transparency & OpenGL Black Box
- **Problem**: When creating transparent overlay windows with OpenGL/egui on Windows, the window can flash or display an opaque black rectangle before blending, or lose transparency when resizing.
- **Solution**:
  - Enable transparency on the root `eframe::NativeOptions` viewport (`viewport.with_transparent(true)`). In `glow_integration`, the root window determines the GL framebuffer format. If transparency is omitted on the root, the pixel format allocates 0 alpha bits (`cAlphaBits = 0`), logging `ERROR eframe::native::glow_integration: Cannot create transparent window: the GL config does not support it` and falling back to a solid black window.
  - Apply `WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW` directly to the OS HWND via Windows API (`src/pet/taskbar.rs`).
  - Track HWND handles with `AtomicIsize` so recreation of viewports receives layered colorkey attributes reliably.
  - Do NOT call `sub_ctx.set_visuals(...)` inside child viewports: `egui::Context` is shared across viewports, so setting default visuals in a child viewport clobbers the main application's custom theme and text styling with dim grayish defaults.

### 2. Keeping Desktop Pets Active When Closing the Settings Window
- **Problem**: In Windows, calling `ShowWindow(hwnd, SW_HIDE)` on the root settings window causes the OS to stop sending `WM_PAINT` and invalidate messages. Because `eframe` executes `App::update()` inside `WM_PAINT` / `RedrawRequested`, hiding the root window halts the entire event loop, freezing the desktop pet in place!
- **Solution**:
  - Instead of `SW_HIDE`, park the root window off-screen at `(-32000, -32000)` and set `WS_EX_TOOLWINDOW` to remove it from the taskbar and Alt-Tab.
  - Because `IsWindowVisible(hwnd)` remains `TRUE`, the Win32 message loop continues pumping repaint timers (`ctx.request_repaint_after()`), allowing the desktop pet to walk, sleep, and sort files while the settings window is closed.
  - When the user opens settings from the tray, restore normal styles and saved coordinates with `SetWindowPos`.

### 3. High-DPI Scaling (Physical vs. Logical Pixels)
- **Problem**: On Windows with 125%, 150%, or 200% display scaling, Win32 APIs (`GetWindowRect`, `GetCursorPos`) return **physical pixels**, while egui uses **logical points**. If passed directly, the pet spawns floating 100px above the taskbar or far to the right.
- **Solution**:
  - Always read egui's scaling factor: `let ppp = ctx.pixels_per_point().max(0.5);`.
  - Convert Win32 physical coordinates to logical points:
    ```rust
    let logical_x = physical_x as f32 / ppp;
    let logical_y = physical_y as f32 / ppp;
    ```
  - Base the character baseline on: `current_y = (taskbar_top / ppp) - 30.0;`.

### 4. Unicode Font Glyphs vs. Vector Primitives (The "Tofu Box" Bug)
- **Problem**: Rendering symbols like `✦` (sparkle), `✓` (checkmark), or emoji sweat `💧` with egui's default font on Windows produces empty square boxes (`□`).
- **Solution**:
  - **Never use unicode symbols or non-ASCII emoji in painter text**.
  - Always draw decorative elements as vector shapes:
    - **Sparkles**: Draw a 4-point diamond star using `egui::Shape::convex_polygon` and `painter.circle_filled` (`PetController::draw_sparkle_star`).
    - **Checkmarks**: Draw two connected strokes with `painter.line_segment(...)`.
    - **Sweat Drops**: Draw a teardrop polygon with `Shape::convex_polygon` and a small circle.

### 5. Direct Desktop Click Interaction Through Mouse-Passthrough Windows
- **Problem**: The pet window must allow mouse clicks to pass through to normal desktop apps (`WS_EX_TRANSPARENT`), but when the pet is stuck by blocking files, the user should be able to click directly on the paper to clear it.
- **Solution**:
  - Use Win32 global mouse polling:
    - Cursor position: `GetCursorPos(...)`.
    - Left-click state: `(GetAsyncKeyState(VK_LBUTTON) as u16 & 0x8000) != 0`.
  - Detect edge trigger (`is_down && !was_down`).
  - Perform geometric distance hit-testing against each landed paper:
    `dx*dx + dy*dy <= radius*radius`.
  - If clicked, pop the file, play a sparkle burst, and update path obstruction state.

### 6. Dialogue Frequency & Multi-File Obstruction
- **Problem**: When several files are dropped simultaneously, clearing one obstacle caused Mochi to cycle dialogue lines every click or trigger the "Thank you!" message even though 4 files were still blocking the way.
- **Solution**:
  - Add a cooldown guard on dialogue: `if self.speech_timer > 1.8 { return; }`.
  - Check whether any remaining papers are still blocking the doorway before unfreezing:
    ```rust
    let still_blocked = self.ground_papers.iter().any(|p| (p.x - self.folder_x).abs() < 24.0);
    if !still_blocked {
        self.state = PetState::Collecting;
        self.trigger_dialogue(DialogueContext::Helped);
    }
    ```

### 7. CPU Throttling & Power Efficiency
- **Problem**: Background pets running constant 60 FPS loops drain laptop batteries.
- **Solution**:
  - **Sleeping**: When in `PetState::Sleeping`, `PetController::update()` returns `Duration::from_millis(300)` (0% CPU usage).
  - **Disabled**: Returns `Duration::from_millis(500)` and drains incoming events.
  - **Active**: Only during active walking, paper fluttering, or arranging does it request 60 FPS repaints (`Duration::from_millis(16)`).

### 8. Linux / GNOME: Run the pet through XWayland

On GNOME's native Wayland session a client cannot position its own window,
force always-on-top, set click-through, or skip the taskbar/dock — everything
the pet needs. The app therefore forces winit's **X11 (XWayland) backend**
whenever an X display is available (`DISPLAY` set) and the required runtime
library can be loaded.

- **Required runtime library**: winit's X11 backend dlopens
  `libxkbcommon-x11.so.0`. Without it the app silently falls back to native
  Wayland, where the pet cannot be positioned or kept on top (file sorting and
  the tray still work). Install it once:

  ```
  sudo apt install libxkbcommon-x11-0
  ```

  The probe is `x11_backend_available()` in `src/app.rs`; the log line
  `using X11 (XWayland) backend for window placement` confirms it took effect.

- **Hiding the settings window**: mutter decorates XWayland windows with a
  *separate* frame window. Collapsing the client to `1x1` is not enough — the
  titlebar-sized frame stays on screen. While hidden, set
  `_MOTIF_WM_HINTS` decorations to `0` (and `_NET_WM_STATE_SKIP_TASKBAR` /
  `SKIP_PAGER`), and restore the decorations when the window is shown.
- **No dock flash at login**: eframe force-shows the root window once after the
  first frame, and `_NET_CLIENT_LIST`-based lookups only see a window *after*
  the WM has mapped it (already in the dock). Style the window **by its raw X
  window id before it is mapped** (`x11_style_window`, id obtained from the
  `raw_window_handle` of the `CreationContext`), then re-assert during a short
  startup burst. Lookups by title remain only as a periodic fallback.
- **Dragging**: with the X11 backend the pet viewport is a real X window whose
  input region is shaped (`x11_set_input_regions`) to the sprite/desk/papers, so
  a click starts an implicit pointer grab and drag motion keeps flowing to the
  pet even over native Wayland surfaces.

