"""
Pack Classic Oneko (1989 X11 / glreno/oneko) frames into the standard
DirWatcher companion spritesheet: 320x160 RGBA PNG (10 cols x 5 rows of 32x32 frames).

Row 0: Idle (10 frames) - Sit, paw wash, ear itch, yawn, sit
Row 1: Alert (10 frames) - Sit, Surprised (!) alert, yawn, sit
Row 2: Walk (10 frames) - 4-legged trot (frames 5 & 6 shifted to y=30 baseline)
Row 3: Drop / Toss / Claw (10 frames) - Up scratch / wall clawing, paw wash, sit
Row 4: Sleep (10 frames) - Curled up sleeping cat with alternating Zzz
"""

import os
from PIL import Image

SRC_DIR = r"C:\Users\HP\.gemini\antigravity-cli\brain\7e2c2b27-13dd-4819-b34c-e8f91beec635\scratch\oneko\src\main\resources\images"
OUT_PATH = "assets/pet/oneko.png"

# Load all 32 GIFs as RGBA
images = {}
for i in range(1, 33):
    p = os.path.join(SRC_DIR, f"{i}.GIF")
    im = Image.open(p).convert("RGBA")
    images[i] = im

sheet = Image.new("RGBA", (320, 160), (0, 0, 0, 0))


def paste_frame(frame_img, col, row, ox=0, oy=0):
    x = col * 32 + ox
    y = row * 32 + oy
    sheet.paste(frame_img, (x, y), frame_img)


# Row 0: Idle (10 frames)
# 25: sit, 31: lick paw, 27/28: scratch ear, 26: yawn
idle_frames = [
    (25, 0, 1),
    (31, 0, 1),
    (25, 0, 1),
    (31, 0, 1),
    (27, 0, 1),
    (28, 0, 1),
    (27, 0, 1),
    (28, 0, 1),
    (26, 0, 1),
    (25, 0, 1),
]
for col, (idx, ox, oy) in enumerate(idle_frames):
    paste_frame(images[idx], col, 0, ox, oy)

# Row 1: Alert (10 frames)
# 25: sit, 32: surprised (!), 26: yawn, 25: sit
alert_frames = [
    (25, 0, 1),
    (32, 0, 1),
    (32, 0, 0),
    (32, 0, -2), # slight hop
    (32, 0, -1),
    (32, 0, 1),
    (26, 0, 1),
    (26, 0, 1),
    (25, 0, 1),
    (25, 0, 1),
]
for col, (idx, ox, oy) in enumerate(alert_frames):
    paste_frame(images[idx], col, 1, ox, oy)

# Row 2: Walk (10 frames)
# 5, 6: running right. Paws are at y=27, shift by oy=+3 so bottom touches y=30
walk_frames = [
    (5, 0, 3),
    (6, 0, 4),
    (5, 0, 3),
    (6, 0, 4),
    (5, 0, 3),
    (6, 0, 4),
    (5, 0, 3),
    (6, 0, 4),
    (5, 0, 3),
    (6, 0, 4),
]
for col, (idx, ox, oy) in enumerate(walk_frames):
    paste_frame(images[idx], col, 2, ox, oy)

# Row 3: Drop / Toss / Scratch (10 frames)
# 17, 18: up scratch; 19, 20: right scratch; 31: lick paw; 25: sit
drop_frames = [
    (17, 0, 0),
    (18, 0, 0),
    (17, 0, 0),
    (18, 0, 0),
    (19, 0, 3),
    (20, 0, 3),
    (31, 0, 1),
    (31, 0, 1),
    (25, 0, 1),
    (25, 0, 1),
]
for col, (idx, ox, oy) in enumerate(drop_frames):
    paste_frame(images[idx], col, 3, ox, oy)

# Row 4: Sleep (10 frames)
# 29, 30: sleeping curled cat with Zzz
sleep_frames = [
    (29, 0, 0),
    (29, 0, 0),
    (30, 0, 0),
    (30, 0, 0),
    (29, 0, 0),
    (29, 0, 0),
    (30, 0, 0),
    (30, 0, 0),
    (29, 0, 0),
    (30, 0, 0),
]
for col, (idx, ox, oy) in enumerate(sleep_frames):
    paste_frame(images[idx], col, 4, ox, oy)

sheet.save(OUT_PATH)
print(f"Successfully generated {OUT_PATH} (320x160 RGBA)")
