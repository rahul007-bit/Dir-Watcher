"""
Generate Pixel Art Spritesheet for Neko (Japanese Calico Cat companion).
Format: 320x160 RGBA PNG (10 columns x 5 rows of 32x32 frames).
Row 0: Idle (10 frames) - loaf/sitting, ear twitches, eye blinks, tail swishes
Row 1: Alert (10 frames) - startled ears, crouch, playful pounce hop with '!'
Row 2: Walk (10 frames) - 4-legged feline trot/prowl, bouncy tail, jingle bell
Row 3: Toss / Drop (10 frames) - playful two-paw bat/swat into folder, paw lick
Row 4: Sleep (10 frames) - curled-up cozy cat loaf, wrapped tail, rhythmic breathing
"""

import math
from PIL import Image, ImageDraw

# Spritesheet dimensions
SHEET_W = 320
SHEET_H = 160
FRAME_SIZE = 32

sheet = Image.new('RGBA', (SHEET_W, SHEET_H), (0, 0, 0, 0))

# --- Calico Color Palette ---
OUTLINE = (55, 45, 52, 255)         # Soft dark charcoal/brown outline
BODY_WHITE = (252, 250, 246, 255)   # Warm creamy white coat
SHADOW_WHITE = (220, 218, 224, 255) # Soft shaded fur
PATCH_GINGER = (235, 125, 45, 255)  # Vibrant ginger calico patch
PATCH_GINGER_D = (195, 95, 30, 255) # Ginger shade
PATCH_BLACK = (62, 55, 68, 255)     # Dark espresso / black calico patch
PATCH_BLACK_L = (85, 78, 92, 255)   # Soft black highlight
EAR_PINK = (255, 170, 185, 255)     # Cute pastel inner ear
EAR_PINK_D = (230, 140, 160, 255)
EYE_DARK = (32, 40, 48, 255)        # Deep anime pupil
EYE_TEAL = (38, 145, 135, 255)      # Emerald/teal iris
EYE_SHINE = (255, 255, 255, 255)    # Glossy eye reflection
NOSE_PINK = (245, 140, 160, 255)    # Little button nose
BLUSH = (255, 175, 185, 180)        # Soft cheek blush
COLLAR_RED = (220, 45, 45, 255)     # Red ribbon collar
BELL_GOLD = (255, 210, 45, 255)     # Golden jingle bell
BELL_SHINE = (255, 245, 150, 255)
ALERT_GOLD = (255, 200, 40, 255)


def put_px(draw, x, y, col):
    draw.point((int(x), int(y)), fill=col)


def fill_rect(draw, x0, y0, x1, y1, col):
    draw.rectangle([int(x0), int(y0), int(x1), int(y1)], fill=col)


# =========================================================================
# ROW 0: IDLE (10 frames)
# Front-facing sitting cat with twitching ears, blinking eyes, and swishing tail.
# =========================================================================
def draw_idle_frame(col, sub_state):
    """
    sub_state: dict controlling ear angles, eye state, tail angle, breath_y
    """
    draw = ImageDraw.Draw(sheet)
    cx = col * 32 + 16
    base_y = 30 # baseline at y=30

    breath = sub_state.get('breath', 0)
    eye_mode = sub_state.get('eye', 'open')
    left_ear_flick = sub_state.get('left_ear_flick', 0)
    right_ear_flick = sub_state.get('right_ear_flick', 0)
    tail_angle = sub_state.get('tail_angle', 0) # -2 to +2
    smile = sub_state.get('smile', False)

    # --- 1. Tail (Behind body) ---
    # Curves from left side (cx - 7) upward to (cx - 13 + tail_angle*2)
    tail_base_x = cx - 7
    tail_base_y = base_y - 3
    for t in range(9):
        progress = t / 8.0
        tx = tail_base_x - math.sin(progress * 2.2) * (5.0 + tail_angle * 1.5) - progress * 2.0
        ty = tail_base_y - progress * 10.0 + math.cos(progress * 3.0) * (tail_angle * 0.8)
        color = PATCH_GINGER if t > 5 else BODY_WHITE
        # Draw 2px thick tail with outline
        fill_rect(draw, tx - 1, ty - 1, tx + 2, ty + 1, OUTLINE)
        fill_rect(draw, tx, ty, tx + 1, ty, color)

    # --- 2. Body (Sitting loaf) ---
    body_y0 = base_y - 12 + breath
    body_y1 = base_y
    # Body outline (width 18)
    draw.rounded_rectangle([cx - 9, body_y0, cx + 9, body_y1], radius=4, fill=OUTLINE)
    # Body fill
    draw.rounded_rectangle([cx - 8, body_y0 + 1, cx + 8, body_y1 - 1], radius=3, fill=BODY_WHITE)
    # Shading at bottom
    fill_rect(draw, cx - 7, body_y1 - 2, cx + 7, body_y1 - 1, SHADOW_WHITE)

    # Calico patch on right hip (black patch)
    fill_rect(draw, cx + 3, body_y0 + 3, cx + 7, body_y1 - 2, PATCH_BLACK)
    put_px(draw, cx + 8, body_y0 + 4, PATCH_BLACK)

    # Front paws (cute little white paws at bottom center)
    # Left paw
    fill_rect(draw, cx - 5, base_y - 3, cx - 2, base_y, OUTLINE)
    fill_rect(draw, cx - 4, base_y - 2, cx - 3, base_y - 1, BODY_WHITE)
    # Right paw
    fill_rect(draw, cx + 2, base_y - 3, cx + 5, base_y, OUTLINE)
    fill_rect(draw, cx + 3, base_y - 2, cx + 4, base_y - 1, BODY_WHITE)

    # --- 3. Collar and Bell ---
    collar_y = body_y0 + 1
    fill_rect(draw, cx - 5, collar_y, cx + 5, collar_y + 1, COLLAR_RED)
    # Shiny gold bell in center
    fill_rect(draw, cx - 1, collar_y + 1, cx + 1, collar_y + 3, BELL_GOLD)
    put_px(draw, cx, collar_y + 1, BELL_SHINE)
    put_px(draw, cx, collar_y + 2, OUTLINE)

    # --- 4. Head ---
    head_y0 = body_y0 - 10
    head_y1 = body_y0 + 2
    # Head outline (width 18, height 12)
    draw.rounded_rectangle([cx - 9, head_y0, cx + 9, head_y1], radius=5, fill=OUTLINE)
    # Head fill
    draw.rounded_rectangle([cx - 8, head_y0 + 1, cx + 8, head_y1 - 1], radius=4, fill=BODY_WHITE)

    # Ginger calico patch over right forehead & ear
    fill_rect(draw, cx + 2, head_y0 + 1, cx + 7, head_y0 + 5, PATCH_GINGER)
    put_px(draw, cx + 8, head_y0 + 3, PATCH_GINGER)
    put_px(draw, cx + 1, head_y0 + 2, PATCH_GINGER)

    # Small black patch on left side of head
    fill_rect(draw, cx - 8, head_y0 + 2, cx - 6, head_y0 + 5, PATCH_BLACK)

    # --- 5. Ears ---
    # Left Ear (with flick offset)
    ley = head_y0 - 4 + left_ear_flick
    draw.polygon([(cx - 8, head_y0 + 1), (cx - 5, ley), (cx - 2, head_y0 + 1)], fill=OUTLINE)
    draw.polygon([(cx - 7, head_y0 + 1), (cx - 5, ley + 1), (cx - 3, head_y0 + 1)], fill=BODY_WHITE)
    draw.polygon([(cx - 6, head_y0 + 1), (cx - 5, ley + 2), (cx - 4, head_y0 + 1)], fill=EAR_PINK)

    # Right Ear (Ginger with flick offset)
    rey = head_y0 - 4 + right_ear_flick
    draw.polygon([(cx + 2, head_y0 + 1), (cx + 5, rey), (cx + 8, head_y0 + 1)], fill=OUTLINE)
    draw.polygon([(cx + 3, head_y0 + 1), (cx + 5, rey + 1), (cx + 7, head_y0 + 1)], fill=PATCH_GINGER)
    draw.polygon([(cx + 4, head_y0 + 1), (cx + 5, rey + 2), (cx + 6, head_y0 + 1)], fill=EAR_PINK_D)

    # --- 6. Face (Eyes, Nose, Whiskers, Blush) ---
    ey = head_y0 + 5
    if eye_mode == 'open':
        # Left eye (large cute anime cat eye with teal iris & sparkle shine)
        fill_rect(draw, cx - 6, ey, cx - 3, ey + 3, OUTLINE)
        fill_rect(draw, cx - 5, ey + 1, cx - 4, ey + 3, EYE_TEAL)
        put_px(draw, cx - 5, ey + 1, EYE_SHINE) # Sparkle highlight

        # Right eye
        fill_rect(draw, cx + 3, ey, cx + 6, ey + 3, OUTLINE)
        fill_rect(draw, cx + 4, ey + 1, cx + 5, ey + 3, EYE_TEAL)
        put_px(draw, cx + 4, ey + 1, EYE_SHINE) # Sparkle highlight
    elif eye_mode == 'blink':
        # Gentle closed curved line (- -)
        draw.line([(cx - 6, ey + 2), (cx - 3, ey + 2)], fill=OUTLINE)
        draw.line([(cx + 3, ey + 2), (cx + 6, ey + 2)], fill=OUTLINE)
    elif eye_mode == 'happy':
        # Happy curved eyes (^ ^)
        draw.line([(cx - 6, ey + 2), (cx - 4, ey + 1)], fill=OUTLINE)
        draw.line([(cx - 4, ey + 1), (cx - 3, ey + 2)], fill=OUTLINE)
        draw.line([(cx + 3, ey + 2), (cx + 5, ey + 1)], fill=OUTLINE)
        draw.line([(cx + 5, ey + 1), (cx + 6, ey + 2)], fill=OUTLINE)

    # Pink button nose
    put_px(draw, cx, ey + 2, NOSE_PINK)

    # Mouth (:3 / w)
    if smile:
        put_px(draw, cx - 1, ey + 3, OUTLINE)
        put_px(draw, cx + 1, ey + 3, OUTLINE)
        put_px(draw, cx, ey + 4, OUTLINE)

    # Soft pink cheek blush
    fill_rect(draw, cx - 8, ey + 3, cx - 6, ey + 4, BLUSH)
    fill_rect(draw, cx + 6, ey + 3, cx + 8, ey + 4, BLUSH)

    # Whiskers (subtle fine lines)
    draw.line([(cx - 9, ey + 2), (cx - 12, ey + 1)], fill=OUTLINE)
    draw.line([(cx - 9, ey + 4), (cx - 12, ey + 5)], fill=OUTLINE)
    draw.line([(cx + 9, ey + 2), (cx + 12, ey + 1)], fill=OUTLINE)
    draw.line([(cx + 9, ey + 4), (cx + 12, ey + 5)], fill=OUTLINE)


idle_specs = [
    {'breath': 0, 'eye': 'open', 'left_ear_flick': 0, 'right_ear_flick': 0, 'tail_angle': -1, 'smile': False},
    {'breath': -1, 'eye': 'open', 'left_ear_flick': 0, 'right_ear_flick': 0, 'tail_angle': 0, 'smile': False},
    {'breath': -1, 'eye': 'open', 'left_ear_flick': 1, 'right_ear_flick': 0, 'tail_angle': 1, 'smile': False}, # ear flick
    {'breath': 0, 'eye': 'blink', 'left_ear_flick': 0, 'right_ear_flick': 0, 'tail_angle': 1, 'smile': False}, # blink
    {'breath': 0, 'eye': 'blink', 'left_ear_flick': 0, 'right_ear_flick': 0, 'tail_angle': 0, 'smile': True},  # blink
    {'breath': -1, 'eye': 'open', 'left_ear_flick': 0, 'right_ear_flick': 1, 'tail_angle': -1, 'smile': True}, # right ear perk
    {'breath': -1, 'eye': 'happy', 'left_ear_flick': 0, 'right_ear_flick': 0, 'tail_angle': -2, 'smile': True},# happy tail swish
    {'breath': 0, 'eye': 'open', 'left_ear_flick': 0, 'right_ear_flick': 0, 'tail_angle': -1, 'smile': True},
    {'breath': 0, 'eye': 'open', 'left_ear_flick': 0, 'right_ear_flick': 0, 'tail_angle': 0, 'smile': False},
    {'breath': 0, 'eye': 'open', 'left_ear_flick': 0, 'right_ear_flick': 0, 'tail_angle': 0, 'smile': False},
]

for c, spec in enumerate(idle_specs):
    draw_idle_frame(c, spec)


# =========================================================================
# ROW 1: ALERT (10 frames)
# Startled! Crouch, then big energetic pounce hop with golden '!' mark!
# =========================================================================
def draw_alert_frame(col, jump_y, crouch_w, crouch_h, show_alert_mark, eye_mode):
    draw = ImageDraw.Draw(sheet)
    cx = col * 32 + 16
    row_y0 = 1 * 32
    base_y = row_y0 + 30 + jump_y

    # Exclamation mark above head if jumping
    if show_alert_mark:
        ax = cx
        ay = row_y0 + 3
        draw.line([(ax, ay), (ax, ay + 4)], fill=ALERT_GOLD)
        draw.line([(ax - 1, ay), (ax + 1, ay)], fill=ALERT_GOLD)
        put_px(draw, ax, ay + 6, ALERT_GOLD)

    # Body
    bw = 18 + crouch_w
    bh = 12 + crouch_h
    body_y0 = base_y - bh
    body_y1 = base_y

    # Tail (raised upright in excitement!)
    tail_base_x = cx - bw // 2 + 2
    for t in range(10):
        prog = t / 9.0
        tx = tail_base_x - math.sin(prog * 1.5) * 4.0
        ty = body_y0 + 4 - prog * 11.0
        fill_rect(draw, tx - 1, ty - 1, tx + 1, ty + 1, OUTLINE)
        put_px(draw, tx, ty, PATCH_GINGER if t > 6 else BODY_WHITE)

    # Body shape
    draw.rounded_rectangle([cx - bw // 2, body_y0, cx + bw // 2, body_y1], radius=4, fill=OUTLINE)
    draw.rounded_rectangle([cx - bw // 2 + 1, body_y0 + 1, cx + bw // 2 - 1, body_y1 - 1], radius=3, fill=BODY_WHITE)
    # Calico patch
    fill_rect(draw, cx + 2, body_y0 + 2, cx + bw // 2 - 2, body_y1 - 2, PATCH_BLACK)

    # Head
    hw = 18
    hh = 11
    head_y0 = body_y0 - hh + 2
    head_y1 = head_y0 + hh
    draw.rounded_rectangle([cx - hw // 2, head_y0, cx + hw // 2, head_y1], radius=5, fill=OUTLINE)
    draw.rounded_rectangle([cx - hw // 2 + 1, head_y0 + 1, cx + hw // 2 - 1, head_y1 - 1], radius=4, fill=BODY_WHITE)

    # Calico patch ginger
    fill_rect(draw, cx + 2, head_y0 + 1, cx + 7, head_y0 + 5, PATCH_GINGER)

    # Ears (alert = pointed high!)
    # Left ear
    draw.polygon([(cx - 8, head_y0 + 1), (cx - 5, head_y0 - 5), (cx - 2, head_y0 + 1)], fill=OUTLINE)
    draw.polygon([(cx - 7, head_y0 + 1), (cx - 5, head_y0 - 4), (cx - 3, head_y0 + 1)], fill=BODY_WHITE)
    draw.polygon([(cx - 6, head_y0 + 1), (cx - 5, head_y0 - 3), (cx - 4, head_y0 + 1)], fill=EAR_PINK)

    # Right ear
    draw.polygon([(cx + 2, head_y0 + 1), (cx + 5, head_y0 - 5), (cx + 8, head_y0 + 1)], fill=OUTLINE)
    draw.polygon([(cx + 3, head_y0 + 1), (cx + 5, head_y0 - 4), (cx + 7, head_y0 + 1)], fill=PATCH_GINGER)
    draw.polygon([(cx + 4, head_y0 + 1), (cx + 5, head_y0 - 3), (cx + 6, head_y0 + 1)], fill=EAR_PINK_D)

    # Collar & Bell
    collar_y = head_y1 - 2
    fill_rect(draw, cx - 4, collar_y, cx + 4, collar_y + 1, COLLAR_RED)
    put_px(draw, cx, collar_y + 2, BELL_GOLD)

    # Eyes
    ey = head_y0 + 4
    if eye_mode == 'wide':
        # Extra large surprised round eyes
        fill_rect(draw, cx - 6, ey - 1, cx - 3, ey + 3, OUTLINE)
        fill_rect(draw, cx - 5, ey, cx - 4, ey + 2, EYE_TEAL)
        put_px(draw, cx - 5, ey, EYE_SHINE)

        fill_rect(draw, cx + 3, ey - 1, cx + 6, ey + 3, OUTLINE)
        fill_rect(draw, cx + 4, ey, cx + 5, ey + 2, EYE_TEAL)
        put_px(draw, cx + 4, ey, EYE_SHINE)
    elif eye_mode == 'happy':
        draw.line([(cx - 6, ey + 2), (cx - 4, ey + 1)], fill=OUTLINE)
        draw.line([(cx - 4, ey + 1), (cx - 3, ey + 2)], fill=OUTLINE)
        draw.line([(cx + 3, ey + 2), (cx + 5, ey + 1)], fill=OUTLINE)
        draw.line([(cx + 5, ey + 1), (cx + 6, ey + 2)], fill=OUTLINE)
    else:
        fill_rect(draw, cx - 5, ey, cx - 4, ey + 2, EYE_DARK)
        fill_rect(draw, cx + 4, ey, cx + 5, ey + 2, EYE_DARK)

    put_px(draw, cx, ey + 2, NOSE_PINK)


alert_frames = [
    # 0, 1: Crouch prep
    (0, 2, -2, False, 'wide'),
    (0, 3, -3, False, 'wide'),
    # 2, 3, 4: High pounce leap into air with '!'
    (-4, -2, 2, True, 'wide'),
    (-6, -2, 3, True, 'wide'),
    (-5, -1, 2, True, 'wide'),
    # 5: Landing squash
    (0, 4, -3, False, 'happy'),
    # 6: Rebound hop
    (-2, 0, 0, False, 'happy'),
    # 7, 8, 9: Proud alert stance
    (0, 0, 0, False, 'happy'),
    (0, 0, 0, False, 'wide'),
    (0, 0, 0, False, 'wide'),
]

for c, (jy, cw, ch, mark, eye) in enumerate(alert_frames):
    draw_alert_frame(c, jy, cw, ch, mark, eye)


# =========================================================================
# ROW 2: WALK CYCLE (10 frames)
# Feline 4-legged prowl/trot cycle!
# Seen in 3/4 side view (walking rightward).
# =========================================================================
def draw_walk_frame(col, leg_phase, bob_y, tail_sway):
    """
    leg_phase: 0..9 step phase
    bob_y: body vertical offset (-1, 0, 1)
    tail_sway: tail tip x offset
    """
    draw = ImageDraw.Draw(sheet)
    cx = col * 32 + 16
    row_y0 = 2 * 32
    base_y = row_y0 + 30

    # 1. Back Tail
    tail_rx = cx - 10
    tail_ry = base_y - 12 + bob_y
    for t in range(9):
        p = t / 8.0
        tx = tail_rx - math.sin(p * 2.0) * (4.0 + tail_sway) - p * 2.0
        ty = tail_ry - p * 9.0 + math.cos(p * 2.5) * 2.0
        fill_rect(draw, tx - 1, ty - 1, tx + 1, ty + 1, OUTLINE)
        put_px(draw, tx, ty, PATCH_BLACK if t > 5 else BODY_WHITE)

    # 2. Far Legs (Back-left and Front-left)
    # Sinusoidal leg extension
    phase_rad = (leg_phase / 10.0) * 2.0 * math.pi
    bl_off = math.sin(phase_rad) * 4.0
    fl_off = math.sin(phase_rad + math.pi) * 4.0

    # Far back leg
    draw.line([(cx - 7, base_y - 6 + bob_y), (cx - 7 + bl_off, base_y)], fill=SHADOW_WHITE)
    put_px(draw, cx - 7 + bl_off, base_y, OUTLINE)

    # Far front leg
    draw.line([(cx + 5, base_y - 6 + bob_y), (cx + 5 + fl_off, base_y)], fill=SHADOW_WHITE)
    put_px(draw, cx + 5 + fl_off, base_y, OUTLINE)

    # 3. Main Cat Body (Horizontal oval walking body)
    body_y0 = base_y - 13 + bob_y
    body_y1 = base_y - 4 + bob_y
    draw.rounded_rectangle([cx - 10, body_y0, cx + 8, body_y1], radius=4, fill=OUTLINE)
    draw.rounded_rectangle([cx - 9, body_y0 + 1, cx + 7, body_y1 - 1], radius=3, fill=BODY_WHITE)

    # Ginger calico saddle patch across middle of back
    fill_rect(draw, cx - 4, body_y0 + 1, cx + 2, body_y0 + 4, PATCH_GINGER)
    # Dark patch at hip
    fill_rect(draw, cx - 8, body_y0 + 2, cx - 5, body_y0 + 5, PATCH_BLACK)

    # 4. Near Legs (Back-right and Front-right)
    br_off = math.sin(phase_rad + math.pi) * 4.0
    fr_off = math.sin(phase_rad) * 4.0

    # Near back leg
    draw.line([(cx - 6, base_y - 6 + bob_y), (cx - 6 + br_off, base_y)], fill=OUTLINE)
    draw.line([(cx - 5, base_y - 6 + bob_y), (cx - 5 + br_off, base_y - 1)], fill=BODY_WHITE)
    fill_rect(draw, cx - 6 + br_off, base_y - 1, cx - 4 + br_off, base_y, BODY_WHITE)
    put_px(draw, cx - 6 + br_off, base_y, OUTLINE)

    # Near front leg
    draw.line([(cx + 6, base_y - 6 + bob_y), (cx + 6 + fr_off, base_y)], fill=OUTLINE)
    draw.line([(cx + 7, base_y - 6 + bob_y), (cx + 7 + fr_off, base_y - 1)], fill=BODY_WHITE)
    fill_rect(draw, cx + 5 + fr_off, base_y - 1, cx + 7 + fr_off, base_y, BODY_WHITE)
    put_px(draw, cx + 5 + fr_off, base_y, OUTLINE)

    # 5. Head (Positioned forward at right side cx + 6)
    hx = cx + 6
    hy = body_y0 - 5
    draw.rounded_rectangle([hx - 6, hy, hx + 8, hy + 11], radius=4, fill=OUTLINE)
    draw.rounded_rectangle([hx - 5, hy + 1, hx + 7, hy + 10], radius=3, fill=BODY_WHITE)

    # Ginger patch over front of head
    fill_rect(draw, hx + 1, hy + 1, hx + 6, hy + 5, PATCH_GINGER)

    # Ears
    # Left Ear (back)
    draw.polygon([(hx - 3, hy + 1), (hx - 1, hy - 4), (hx + 2, hy + 1)], fill=OUTLINE)
    draw.polygon([(hx - 2, hy + 1), (hx - 1, hy - 3), (hx + 1, hy + 1)], fill=EAR_PINK)

    # Right Ear (front, ginger)
    draw.polygon([(hx + 2, hy + 1), (hx + 5, hy - 5), (hx + 8, hy + 1)], fill=OUTLINE)
    draw.polygon([(hx + 3, hy + 1), (hx + 5, hy - 4), (hx + 7, hy + 1)], fill=PATCH_GINGER)
    draw.polygon([(hx + 4, hy + 1), (hx + 5, hy - 3), (hx + 6, hy + 1)], fill=EAR_PINK_D)

    # Collar & Bell
    fill_rect(draw, hx - 4, hy + 9, hx + 2, hy + 10, COLLAR_RED)
    put_px(draw, hx - 1, hy + 11, BELL_GOLD)

    # Eye & Nose (Profile / 3/4 view)
    ey = hy + 4
    fill_rect(draw, hx + 3, ey, hx + 5, ey + 2, OUTLINE)
    fill_rect(draw, hx + 4, ey + 1, hx + 5, ey + 2, EYE_TEAL)
    put_px(draw, hx + 4, ey + 1, EYE_SHINE)

    put_px(draw, hx + 7, ey + 2, NOSE_PINK)
    # Whiskers
    draw.line([(hx + 8, ey + 2), (hx + 11, ey + 1)], fill=OUTLINE)
    draw.line([(hx + 8, ey + 3), (hx + 11, ey + 4)], fill=OUTLINE)


walk_specs = [
    # (leg_phase, bob_y, tail_sway)
    (0,  0, -1),
    (1, -1,  0),
    (2,  0,  1),
    (3,  1,  2),
    (4,  0,  1),
    (5,  0, -1),
    (6, -1,  0),
    (7,  0,  1),
    (8,  1,  2),
    (9,  0,  1),
]

for c, (lp, by, ts) in enumerate(walk_specs):
    draw_walk_frame(c, lp, by, ts)


# =========================================================================
# ROW 3: TOSS / DROP (10 frames)
# Playful cat bat! Reaches both paws up, bats paper neatly down, then licks paw.
# =========================================================================
def draw_toss_frame(col, sub_state):
    draw = ImageDraw.Draw(sheet)
    cx = col * 32 + 16
    row_y0 = 3 * 32
    base_y = row_y0 + 30

    pose = sub_state.get('pose', 'stand') # 'stand', 'swipe', 'land', 'lick'
    paw_y = sub_state.get('paw_y', 0)
    sparkle = sub_state.get('sparkle', False)

    # Hindquarters sitting/planted
    body_y0 = base_y - 14
    body_y1 = base_y
    draw.rounded_rectangle([cx - 8, body_y0, cx + 8, body_y1], radius=4, fill=OUTLINE)
    draw.rounded_rectangle([cx - 7, body_y0 + 1, cx + 7, body_y1 - 1], radius=3, fill=BODY_WHITE)
    # Calico patch
    fill_rect(draw, cx + 2, body_y0 + 2, cx + 6, body_y1 - 2, PATCH_BLACK)

    # Tail happy high curl
    tail_x = cx - 8
    for t in range(8):
        p = t / 7.0
        tx = tail_x - math.sin(p * 2.0) * 4.0 - p * 3.0
        ty = base_y - 4 - p * 11.0
        fill_rect(draw, tx - 1, ty - 1, tx + 1, ty + 1, OUTLINE)
        put_px(draw, tx, ty, PATCH_GINGER if t > 4 else BODY_WHITE)

    # Head
    hy = body_y0 - 9
    draw.rounded_rectangle([cx - 7, hy, cx + 7, hy + 10], radius=4, fill=OUTLINE)
    draw.rounded_rectangle([cx - 6, hy + 1, cx + 6, hy + 9], radius=3, fill=BODY_WHITE)
    fill_rect(draw, cx + 1, hy + 1, cx + 5, hy + 4, PATCH_GINGER)

    # Ears
    draw.polygon([(cx - 6, hy + 1), (cx - 4, hy - 4), (cx - 1, hy + 1)], fill=OUTLINE)
    draw.polygon([(cx - 5, hy + 1), (cx - 4, hy - 3), (cx - 2, hy + 1)], fill=EAR_PINK)

    draw.polygon([(cx + 1, hy + 1), (cx + 4, hy - 4), (cx + 6, hy + 1)], fill=OUTLINE)
    draw.polygon([(cx + 2, hy + 1), (cx + 4, hy - 3), (cx + 5, hy + 1)], fill=PATCH_GINGER)

    # Collar & Bell
    fill_rect(draw, cx - 4, hy + 9, cx + 4, hy + 10, COLLAR_RED)
    put_px(draw, cx, hy + 11, BELL_GOLD)

    # Paws action based on pose
    if pose == 'stand':
        # Reaching up with both paws!
        draw.rounded_rectangle([cx - 9, hy + 2 + paw_y, cx - 5, hy + 8 + paw_y], radius=2, fill=OUTLINE)
        draw.rounded_rectangle([cx - 8, hy + 3 + paw_y, cx - 6, hy + 7 + paw_y], radius=1, fill=BODY_WHITE)

        draw.rounded_rectangle([cx + 5, hy + 2 + paw_y, cx + 9, hy + 8 + paw_y], radius=2, fill=OUTLINE)
        draw.rounded_rectangle([cx + 6, hy + 3 + paw_y, cx + 8, hy + 7 + paw_y], radius=1, fill=BODY_WHITE)
    elif pose == 'swipe':
        # Swiping down hard!
        draw.rounded_rectangle([cx + 2, hy + 8 + paw_y, cx + 8, hy + 14 + paw_y], radius=2, fill=OUTLINE)
        draw.rounded_rectangle([cx + 3, hy + 9 + paw_y, cx + 7, hy + 13 + paw_y], radius=1, fill=BODY_WHITE)
    elif pose == 'lick':
        # Paw held up to mouth, licking cute tongue
        draw.rounded_rectangle([cx - 2, hy + 5, cx + 2, hy + 10], radius=2, fill=OUTLINE)
        draw.rounded_rectangle([cx - 1, hy + 6, cx + 1, hy + 9], radius=1, fill=BODY_WHITE)
        # Little pink tongue
        put_px(draw, cx, hy + 5, EAR_PINK)

    # Face
    ey = hy + 4
    if pose in ('swipe', 'lick'):
        # Happy curved closed eyes (^ ^)
        draw.line([(cx - 5, ey + 1), (cx - 3, ey)], fill=OUTLINE)
        draw.line([(cx - 3, ey), (cx - 2, ey + 1)], fill=OUTLINE)
        draw.line([(cx + 2, ey + 1), (cx + 3, ey)], fill=OUTLINE)
        draw.line([(cx + 3, ey), (cx + 5, ey + 1)], fill=OUTLINE)
    else:
        # Focused playful eyes
        fill_rect(draw, cx - 5, ey, cx - 3, ey + 2, OUTLINE)
        put_px(draw, cx - 4, ey + 1, EYE_TEAL)
        fill_rect(draw, cx + 3, ey, cx + 5, ey + 2, OUTLINE)
        put_px(draw, cx + 4, ey + 1, EYE_TEAL)

    put_px(draw, cx, ey + 2, NOSE_PINK)
    fill_rect(draw, cx - 6, ey + 3, cx - 5, ey + 4, BLUSH)
    fill_rect(draw, cx + 5, ey + 3, cx + 6, ey + 4, BLUSH)

    # Sparkle stars when dropping paper into folder
    if sparkle:
        sx = cx + 10
        sy = base_y - 6
        draw.line([(sx, sy - 2), (sx, sy + 2)], fill=BELL_GOLD)
        draw.line([(sx - 2, sy), (sx + 2, sy)], fill=BELL_GOLD)
        put_px(draw, sx, sy, BELL_SHINE)


toss_specs = [
    # 0, 1: Ready crouch & reach up
    {'pose': 'stand', 'paw_y': 2, 'sparkle': False},
    {'pose': 'stand', 'paw_y': -1, 'sparkle': False},
    # 2, 3: High paw raise
    {'pose': 'stand', 'paw_y': -4, 'sparkle': False},
    {'pose': 'stand', 'paw_y': -6, 'sparkle': False},
    # 4, 5: Downward SWAT into slot!
    {'pose': 'swipe', 'paw_y': 0, 'sparkle': True},
    {'pose': 'swipe', 'paw_y': 3, 'sparkle': True},
    # 6, 7: Happy landing & purr
    {'pose': 'lick', 'paw_y': 0, 'sparkle': False},
    {'pose': 'lick', 'paw_y': 0, 'sparkle': False},
    # 8, 9: Satisfaction
    {'pose': 'lick', 'paw_y': 0, 'sparkle': False},
    {'pose': 'stand', 'paw_y': 4, 'sparkle': False},
]

for c, spec in enumerate(toss_specs):
    draw_toss_frame(c, spec)


# =========================================================================
# ROW 4: SLEEP (10 frames)
# Curled-up cozy cat circle / loaf ("Neko Nabe" style)!
# Breathing gently: expands & relaxes rhythmically.
# =========================================================================
def draw_sleep_frame(col, breath_y, breath_w):
    draw = ImageDraw.Draw(sheet)
    cx = col * 32 + 16
    row_y0 = 4 * 32
    base_y = row_y0 + 30

    # Cozy round loaf dimensions
    w = 22 + breath_w
    h = 13 + breath_y
    loaf_y0 = base_y - h
    loaf_y1 = base_y

    # Main curled-up cat body (round loaf)
    draw.rounded_rectangle([cx - w // 2, loaf_y0, cx + w // 2, loaf_y1], radius=6, fill=OUTLINE)
    draw.rounded_rectangle([cx - w // 2 + 1, loaf_y0 + 1, cx + w // 2 - 1, loaf_y1 - 1], radius=5, fill=BODY_WHITE)

    # Shading underneath
    draw.rounded_rectangle([cx - w // 2 + 2, loaf_y1 - 3, cx + w // 2 - 2, loaf_y1 - 1], radius=2, fill=SHADOW_WHITE)

    # Calico markings:
    # Big ginger patch on middle/top back
    fill_rect(draw, cx - 2, loaf_y0 + 1, cx + 5, loaf_y0 + 4, PATCH_GINGER)
    put_px(draw, cx + 6, loaf_y0 + 2, PATCH_GINGER)
    # Dark black patch on rear
    fill_rect(draw, cx - w // 2 + 2, loaf_y0 + 3, cx - w // 2 + 6, loaf_y1 - 3, PATCH_BLACK)

    # Sleeping head (tucked cozily on the right side)
    hx = cx + w // 2 - 6
    hy = loaf_y0 + 2
    # Folded ears
    # Left folded ear
    draw.polygon([(hx - 4, hy + 2), (hx - 2, hy - 2), (hx, hy + 2)], fill=OUTLINE)
    draw.polygon([(hx - 3, hy + 2), (hx - 2, hy - 1), (hx - 1, hy + 2)], fill=EAR_PINK)

    # Right folded ear (ginger)
    draw.polygon([(hx + 1, hy + 2), (hx + 3, hy - 2), (hx + 5, hy + 2)], fill=OUTLINE)
    draw.polygon([(hx + 2, hy + 2), (hx + 3, hy - 1), (hx + 4, hy + 2)], fill=PATCH_GINGER)

    # Sweet sleeping closed eyes (- -)
    ey = hy + 4
    draw.line([(hx - 3, ey), (hx - 1, ey)], fill=OUTLINE)
    draw.line([(hx + 1, ey), (hx + 3, ey)], fill=OUTLINE)

    # Tiny button nose
    put_px(draw, hx, ey + 1, NOSE_PINK)

    # Soft pink cheek glow
    fill_rect(draw, hx - 4, ey + 2, hx - 2, ey + 3, BLUSH)
    fill_rect(draw, hx + 2, ey + 2, hx + 4, ey + 3, BLUSH)

    # Wrapped tail (swerves around the front to curl snug over the nose!)
    tail_y = loaf_y1 - 3
    tail_x0 = cx - w // 2 + 1
    tail_x1 = hx - 2
    fill_rect(draw, tail_x0, tail_y, tail_x1, tail_y + 2, OUTLINE)
    fill_rect(draw, tail_x0 + 1, tail_y, tail_x1 - 1, tail_y + 1, PATCH_GINGER)
    # White tail tip
    fill_rect(draw, tail_x1 - 2, tail_y, tail_x1 - 1, tail_y + 1, BODY_WHITE)

    # Red collar & bell visible tucked next to cheek
    put_px(draw, hx - 4, hy + 6, COLLAR_RED)
    put_px(draw, hx - 3, hy + 6, BELL_GOLD)


sleep_breaths = [
    (0, 0),  # 0: Rest
    (0, 0),  # 1: Rest
    (1, 1),  # 2: Gentle inhale
    (1, 1),  # 3: Inhaling
    (2, 2),  # 4: Full cozy lung expansion
    (2, 2),  # 5: Peak breath
    (1, 1),  # 6: Gentle exhale
    (1, 1),  # 7: Exhaling
    (0, 0),  # 8: Settle back
    (0, 0),  # 9: Settle
]

for c, (by, bw) in enumerate(sleep_breaths):
    draw_sleep_frame(c, by, bw)

output_path = 'assets/pet/neko.png'
sheet.save(output_path)
print(f"Successfully generated {output_path} ({SHEET_W}x{SHEET_H} RGBA)")
