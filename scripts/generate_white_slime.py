from PIL import Image, ImageDraw

# Create 320x160 RGBA spritesheet (10 columns x 5 rows of 32x32 frames)
# Row 0: Idle (10 frames) - cute blinking & soft wobble
# Row 1: Alert (10 frames) - eyes pop open, surprise hop with '!'
# Row 2: Walk / Hop (10 frames) - squash & stretch bouncing forward
# Row 3: Drop / Toss (10 frames) - happy bow & file toss
# Row 4: Sleep (10 frames) - plump round mochi loaf breathing gently (NOT a dead puddle!)

sheet = Image.new('RGBA', (320, 160), (0, 0, 0, 0))

# Palette: Clean White Mochi
OUTLINE = (70, 80, 95, 255)       # Crisp border so visible on light & dark desktop
SHADOW = (215, 225, 235, 255)     # Soft shading
BODY = (250, 252, 255, 255)       # Pure bright white body
HIGHLIGHT = (255, 255, 255, 255)  # Gloss shine
EYE = (35, 40, 50, 255)           # Cute dark eye
BLUSH = (255, 175, 190, 230)      # Soft pastel pink cheeks

def draw_slime(col, row, ox=0, oy=0, w=20, h=14, eyes='open', eye_y=0, blush=True):
    """Draw a cute rounded slime blob centered in 32x32 frame."""
    x0 = col * 32 + (16 - w // 2) + ox
    y0 = row * 32 + (30 - h) + oy
    x1 = x0 + w
    y1 = y0 + h

    draw = ImageDraw.Draw(sheet)

    # 1. Base shadow / outline shape (rounded rectangle / oval)
    draw.rounded_rectangle([x0, y0, x1, y1], radius=w//3, fill=OUTLINE)
    # 2. Body fill (inset by 1 pixel)
    draw.rounded_rectangle([x0+1, y0+1, x1-1, y1-1], radius=(w//3)-1, fill=BODY)
    # 3. Bottom shadow shading
    draw.rounded_rectangle([x0+2, y1-3, x1-2, y1-1], radius=2, fill=SHADOW)
    # 4. Top-left gloss highlight
    draw.ellipse([x0+3, y0+2, x0+7, y0+5], fill=HIGHLIGHT)

    # 5. Cute pink blush cheeks
    if blush:
        draw.rectangle([x0+3, y0+h//2+1, x0+5, y0+h//2+2], fill=BLUSH)
        draw.rectangle([x1-5, y0+h//2+1, x1-3, y0+h//2+2], fill=BLUSH)

    # 6. Eyes
    ey = y0 + h // 2 + eye_y
    if eyes == 'open':
        # Big cute shiny eyes
        # Left eye
        draw.rectangle([x0+7, ey-1, x0+8, ey+1], fill=EYE)
        draw.point([(x0+7, ey-1)], fill=HIGHLIGHT)
        # Right eye
        draw.rectangle([x1-8, ey-1, x1-7, ey+1], fill=EYE)
        draw.point([(x1-8, ey-1)], fill=HIGHLIGHT)
    elif eyes == 'sleep':
        # Cute sleeping curved / horizontal happy eyes (- -)
        draw.line([(x0+6, ey), (x0+9, ey)], fill=EYE)
        draw.line([(x1-9, ey), (x1-6, ey)], fill=EYE)
    elif eyes == 'happy':
        # Carefree curved happy eyes (^ ^)
        draw.line([(x0+6, ey), (x0+7, ey-1)], fill=EYE)
        draw.line([(x0+7, ey-1), (x0+8, ey)], fill=EYE)
        draw.line([(x1-8, ey), (x1-7, ey-1)], fill=EYE)
        draw.line([(x1-7, ey-1), (x1-6, ey)], fill=EYE)
    elif eyes == 'surprised':
        # Wide alert dot eyes
        draw.rectangle([x0+6, ey-2, x0+8, ey+1], fill=EYE)
        draw.point([(x0+7, ey-1)], fill=HIGHLIGHT)
        draw.rectangle([x1-8, ey-2, x1-6, ey+1], fill=EYE)
        draw.point([(x1-7, ey-1)], fill=HIGHLIGHT)

# --- Row 0: Idle (Soft wobble & cute blink) ---
for c in range(10):
    if c in (0, 1, 8, 9):
        draw_slime(c, 0, w=20, h=14, eyes='open')
    elif c in (2, 7):
        draw_slime(c, 0, w=21, h=13, eyes='open')
    elif c in (3, 4):
        # Blink
        draw_slime(c, 0, w=21, h=13, eyes='sleep')
    else:
        draw_slime(c, 0, w=20, h=15, eyes='open')

# --- Row 1: Alert (Wake up, eyes wide, jump with '!') ---
for c in range(10):
    if c < 2:
        draw_slime(c, 1, w=20, h=14, eyes='surprised')
    elif c < 5:
        # Jump up
        draw_slime(c, 1, oy=-3, w=18, h=16, eyes='surprised')
        # Little exclamation above
        d = ImageDraw.Draw(sheet)
        d.line([(c*32+16, 1*32+3), (c*32+16, 1*32+7)], fill=(255, 200, 50, 255))
        d.point([(c*32+16, 1*32+9)], fill=(255, 200, 50, 255))
    elif c < 8:
        # Land squash
        draw_slime(c, 1, w=23, h=12, eyes='happy')
    else:
        draw_slime(c, 1, w=20, h=14, eyes='open')

# --- Row 2: Walk / Hop Cycle (Squash & Stretch Bounce) ---
hop_offsets = [
    (0, 22, 12, 'open'),   # 0: Squash prep
    (-1, 19, 15, 'open'),  # 1: Spring up
    (-3, 17, 17, 'open'),  # 2: Peak jump
    (-4, 16, 18, 'happy'), # 3: Floating apex
    (-3, 17, 17, 'happy'), # 4: Coming down
    (-1, 19, 15, 'open'),  # 5: Landing
    (0, 23, 11, 'happy'),  # 6: Land squash
    (0, 22, 12, 'open'),   # 7: Rebound
    (0, 21, 13, 'open'),   # 8: Recovery
    (0, 20, 14, 'open'),   # 9: Reset
]
for c, (oy, w, h, eye) in enumerate(hop_offsets):
    draw_slime(c, 2, oy=oy, w=w, h=h, eyes=eye)

# --- Row 3: Drop / Toss (Happy bow & toss) ---
for c in range(10):
    if c < 3:
        # Squash back
        draw_slime(c, 3, w=22, h=12, eyes='happy')
    elif c < 6:
        # Toss up
        draw_slime(c, 3, oy=-2, w=18, h=16, eyes='happy')
    else:
        # Happy settle
        draw_slime(c, 3, w=20, h=14, eyes='happy')

# --- Row 4: SLEEP (Cute Plump Mochi Loaf Breathing Gently!) ---
# Gentle 10-frame breathing cycle: plump loaf expanding & relaxing
sleep_breaths = [
    (0, 22, 13), # Resting loaf
    (0, 22, 13),
    (-1, 23, 14), # Inhaling gently (soft rise)
    (-1, 23, 14),
    (-2, 23, 15), # Peak breath (cozy plumpness)
    (-2, 23, 15),
    (-1, 23, 14), # Exhaling
    (-1, 23, 14),
    (0, 22, 13), # Back to resting
    (0, 22, 13),
]
for c, (oy, w, h) in enumerate(sleep_breaths):
    draw_slime(c, 4, oy=oy, w=w, h=h, eyes='sleep', eye_y=1, blush=True)

sheet.save('assets/pet/slime_white_mochi.png')
print("Successfully generated assets/pet/slime_white_mochi.png (320x160 RGBA)")

# Updated cute soft-blue Zzz strip
zzz_sheet = Image.new('RGBA', (128, 32), (0, 0, 0, 0))
z_draw = ImageDraw.Draw(zzz_sheet)

def draw_pill_z(ox, oy, sz, color):
    if sz == 1:
        z_draw.line([(ox, oy), (ox+4, oy)], fill=color)
        z_draw.line([(ox+3, oy+1), (ox+1, oy+3)], fill=color)
        z_draw.line([(ox, oy+4), (ox+4, oy+4)], fill=color)
    else:
        z_draw.line([(ox, oy), (ox+6, oy)], fill=color)
        z_draw.line([(ox+5, oy+1), (ox+4, oy+2)], fill=color)
        z_draw.line([(ox+3, oy+3), (ox+2, oy+4)], fill=color)
        z_draw.line([(ox+1, oy+5), (ox, oy+6)], fill=color)
        z_draw.line([(ox, oy+6), (ox+6, oy+6)], fill=color)

c_soft = (175, 215, 255, 220)
c_bright = (210, 235, 255, 255)

draw_pill_z(14, 20, 1, c_soft)
draw_pill_z(32 + 12, 14, 1, c_bright)
draw_pill_z(32 + 18, 22, 1, c_soft)
draw_pill_z(64 + 10, 8, 2, c_bright)
draw_pill_z(64 + 18, 16, 1, c_soft)
draw_pill_z(96 + 8, 4, 2, (175, 215, 255, 140))
draw_pill_z(96 + 16, 12, 1, c_bright)
draw_pill_z(96 + 14, 22, 1, c_soft)

zzz_sheet.save('assets/pet/zzz_particles.png')
print("Updated assets/pet/zzz_particles.png")

# 16x16 crisp document icon for 1.0x scale
doc16 = Image.new('RGBA', (16, 16), (0, 0, 0, 0))
d16 = ImageDraw.Draw(doc16)
d16.rectangle([2, 1, 13, 14], fill=(45, 55, 72, 255))
d16.rectangle([3, 2, 12, 13], fill=(255, 255, 255, 255))
d16.polygon([(9, 2), (12, 5), (9, 5)], fill=(210, 225, 240, 255))
d16.line([(9, 2), (9, 5), (12, 5)], fill=(45, 55, 72, 255))
d16.rectangle([4, 4, 8, 5], fill=(59, 130, 246, 255))
d16.line([(4, 7), (11, 7)], fill=(160, 174, 192, 255))
d16.line([(4, 9), (11, 9)], fill=(160, 174, 192, 255))
d16.line([(4, 11), (9, 11)], fill=(160, 174, 192, 255))
doc16.save('assets/pet/file_icon_16.png')
print("Created assets/pet/file_icon_16.png")

# 16x16 cute organizer folder icon
folder = Image.new('RGBA', (16, 16), (0, 0, 0, 0))
df = ImageDraw.Draw(folder)
# Outline & back flap
df.rectangle([2, 3, 7, 5], fill=(45, 55, 70, 255))
df.rectangle([2, 5, 13, 13], fill=(45, 55, 70, 255))
df.rectangle([3, 4, 6, 5], fill=(245, 185, 65, 255))
df.rectangle([3, 6, 12, 12], fill=(230, 160, 45, 255))
# White paper peek inside
df.rectangle([5, 4, 11, 7], fill=(255, 255, 255, 255))
# Front pocket
df.rectangle([2, 7, 13, 13], fill=(45, 55, 70, 255))
df.rectangle([3, 8, 12, 12], fill=(250, 200, 80, 255))
df.line([(4, 10), (11, 10)], fill=(225, 170, 50, 255))

folder.save('assets/pet/folder_slot.png')
print("Created assets/pet/folder_slot.png")

# Authentic loose-leaf paper sheet (feels like real paper!)
paper = Image.new('RGBA', (16, 16), (0, 0, 0, 0))
dp = ImageDraw.Draw(paper)
# Soft drop shadow
dp.rectangle([3, 2, 14, 15], fill=(0, 0, 0, 40))
# Paper border: subtle light slate-grey
dp.rectangle([2, 1, 13, 14], fill=(130, 142, 158, 255))
# Paper body: crisp bright paper white
dp.rectangle([3, 2, 12, 13], fill=(255, 255, 255, 255))
# Folded dog-ear corner top-right
dp.point([(12, 1), (13, 1), (13, 2)], fill=(0, 0, 0, 0))
dp.polygon([(10, 1), (13, 4), (10, 4)], fill=(225, 232, 240, 255))
dp.line([(10, 1), (10, 4), (13, 4)], fill=(130, 142, 158, 255))
# Authentic soft pencil written lines
dp.line([(4, 4), (9, 4)], fill=(115, 125, 140, 255))
dp.line([(4, 6), (11, 6)], fill=(115, 125, 140, 255))
dp.line([(4, 8), (10, 8)], fill=(115, 125, 140, 255))
dp.line([(4, 10), (11, 10)], fill=(115, 125, 140, 255))
dp.line([(4, 12), (7, 12)], fill=(135, 145, 160, 255))

paper.save('assets/pet/paper_realistic.png')
print("Created assets/pet/paper_realistic.png")
