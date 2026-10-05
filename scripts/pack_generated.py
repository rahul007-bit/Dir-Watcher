"""Slice AI-generated animation strips/grids into the DirWatcher pet spritesheet.

Takes per-action horizontal strips (or one full grid) of pixel-art frames on a
flat solid background, removes the background, slices each frame, downscales to
32x32 with the feet on a common baseline, and packs them into the standard
320x192 sheet (10 cols x 6 rows).

Rows (top to bottom): Idle, Alert, Walk, Drop, Sleep, Groom.

Usage
-----
Per-action strips (recommended):
    python scripts/pack_generated.py --pet hamster \
        --strips assets/pet/src/gen/hamster

Per-action folders of individual frames (folder names may alias ideal->idle,
run->walk):
    python scripts/pack_generated.py --pet hamster \
        --frames assets/pet/src/gen/hamster

One full grid image (rows = actions in order, cols = frames):
    python scripts/pack_generated.py --pet dog --grid assets/pet/src/gen/dog/sheet.png
"""

import argparse
import os
import re
from collections import Counter

from PIL import Image

FRAME = 32
BASELINE = 30
SHEET_W = 320
SHEET_H = 192
ACTIONS = ["idle", "alert", "walk", "drop", "sleep", "groom"]
# Common folder-name aliases for the per-action frames mode.
ALIASES = {
    "ideal": "idle",
    "idle": "idle",
    "run": "walk",
    "walk": "walk",
    "walking": "walk",
    "jump": "alert",
    "sleeping": "sleep",
    "grooming": "groom",
}


def background_color(im):
    w, h = im.size
    px = im.load()
    corners = [px[0, 0], px[w - 1, 0], px[0, h - 1], px[w - 1, h - 1]]
    # most common corner (each quantised to reduce noise)
    q = Counter((c[0] // 8, c[1] // 8, c[2] // 8) for c in corners)
    key, _ = q.most_common(1)[0]
    for c in corners:
        if (c[0] // 8, c[1] // 8, c[2] // 8) == key:
            return c
    return corners[0]


def erode_alpha(im, n=1):
    """Strip the outermost ring(s) of opaque pixels (removes chroma-key fringe)."""
    im = im.convert("RGBA")
    for _ in range(n):
        px = im.load()
        w, h = im.size
        clear = []
        for y in range(h):
            for x in range(w):
                if px[x, y][3] == 0:
                    continue
                for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    nx, ny = x + dx, y + dy
                    if 0 <= nx < w and 0 <= ny < h and px[nx, ny][3] == 0:
                        clear.append((x, y))
                        break
        for x, y in clear:
            px[x, y] = (0, 0, 0, 0)
    return im


def key_background(im, bg, tol=60, erode=1):
    """Make the flat background (and its magenta/purple fringe) transparent."""
    im = im.convert("RGBA")
    px = im.load()
    w, h = im.size
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if a < 20:
                px[x, y] = (0, 0, 0, 0)
                continue
            magenta_family = (r - g) > 30 and (b - g) > 25 and r > 130 and b > 120
            near_bg = abs(r - bg[0]) + abs(g - bg[1]) + abs(b - bg[2]) <= tol
            if magenta_family or near_bg:
                px[x, y] = (0, 0, 0, 0)
    return erode_alpha(im, erode)


def make_mask(im, bg, tol=60):
    w, h = im.size
    px = im.load()
    mask = [[False] * h for _ in range(w)]
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if a < 20:
                continue
            if abs(r - bg[0]) + abs(g - bg[1]) + abs(b - bg[2]) > tol:
                mask[x][y] = True
    return mask


def axis_runs(profile, thresh=1, min_gap=2):
    """Collapse a 1-D occupancy profile into runs of content, ignoring 1px gaps."""
    runs = []
    start = None
    gap = 0
    for i, v in enumerate(profile):
        if v > thresh:
            if start is None:
                start = i
            gap = 0
        elif start is not None:
            gap += 1
            if gap >= min_gap:
                runs.append((start, i - gap))
                start = None
    if start is not None:
        runs.append((start, len(profile) - 1))
    return runs


def slice_frames(im, expected_cols=None):
    """Slice an image into individual frame images using background gaps."""
    im = im.convert("RGBA")
    bg = background_color(im)
    im = key_background(im, bg)
    mask = make_mask(im, bg)
    w, h = im.size

    col_profile = [sum(1 for y in range(h) if mask[x][y]) for x in range(w)]
    row_profile = [sum(1 for x in range(w) if mask[x][y]) for y in range(h)]

    col_runs = axis_runs(col_profile, min_gap=2)
    row_runs = axis_runs(row_profile, min_gap=2)

    frames = []
    for (rx0, rx1) in row_runs:
        for (cx0, cx1) in col_runs:
            # crop cell, then tighten to its own content bbox
            cell = im.crop((cx0, rx0, cx1 + 1, rx1 + 1))
            sub = make_mask(cell, bg)
            cw, ch = cell.size
            xs = [x for x in range(cw) if any(sub[x][y] for y in range(ch))]
            ys = [y for y in range(ch) if any(sub[x][y] for x in range(cw))]
            if not xs or not ys:
                continue
            tight = cell.crop((xs[0], ys[0], xs[-1] + 1, ys[-1] + 1))
            frames.append(tight)
    return frames


def fit_frame(im, frame=FRAME, baseline=BASELINE, margin=1):
    """Fit a cropped frame into a 32x32 cell, feet on the baseline, centered."""
    im = im.crop(im.getbbox())
    w, h = im.size
    limit = frame - margin * 2
    factor = max(1, -(-max(w, h) // limit))
    nw, nh = max(1, w // factor), max(1, h // factor)
    scaled = im.resize((nw, nh), Image.NEAREST)
    out = Image.new("RGBA", (frame, frame), (0, 0, 0, 0))
    ox = (frame - nw) // 2
    oy = baseline - nh
    out.paste(scaled, (ox, oy), scaled)
    return out


def row_factor(frames, frame=FRAME, margin=1):
    limit = frame - margin * 2
    maxext = max((max(f.size) for f in frames), default=1)
    return max(1, -(-maxext // limit))


def fit_frame_scaled(im, factor, frame=FRAME, baseline=BASELINE):
    """Scale with a shared per-row factor so frames don't jitter in size."""
    im = im.crop(im.getbbox())
    w, h = im.size
    nw, nh = max(1, round(w / factor)), max(1, round(h / factor))
    scaled = im.resize((nw, nh), Image.NEAREST)
    out = Image.new("RGBA", (frame, frame), (0, 0, 0, 0))
    out.paste(scaled, ((frame - nw) // 2, baseline - nh), scaled)
    return out


def normalize_rows(rows):
    """Apply a shared scale within each action so the pet size is stable."""
    out = {}
    for action, frames in rows.items():
        frames = [f for f in frames if f.getbbox()]
        if not frames:
            continue
        factor = row_factor(frames)
        out[action] = [fit_frame_scaled(f, factor) for f in frames]
    return out


def build_sheet(rows):
    """rows: dict action -> list of frame images (already 32x32)."""
    sheet = Image.new("RGBA", (SHEET_W, SHEET_H), (0, 0, 0, 0))
    for row, action in enumerate(ACTIONS):
        frames = rows.get(action)
        if not frames:
            continue
        for col, fr in enumerate(frames[:10]):
            sheet.paste(fr, (col * FRAME, row * FRAME), fr)
    return sheet


def pack_strips(pet, strips_dir):
    rows = {}
    for action in ACTIONS:
        path = None
        for ext in (".png", ".webp", ".jpg", ".jpeg", ".gif"):
            cand = os.path.join(strips_dir, action + ext)
            if os.path.exists(cand):
                path = cand
                break
        if not path:
            continue
        im = Image.open(path)
        frames = slice_frames(im)
        rows[action] = frames
        print(f"  {action}: {len(frames)} frames  <- {os.path.basename(path)}")
    return rows


def even_bands(total, n):
    return [(round(i * total / n), round((i + 1) * total / n)) for i in range(n)]


def split_band_at_min(band, k, profile):
    """Split one content band into k sub-bands, cutting at low-density rows."""
    y0, y1 = band
    cuts = [y0]
    for j in range(1, k):
        approx = y0 + (y1 - y0) * j // k
        window = max(3, (y1 - y0) // 10)
        lo, hi = max(y0 + 1, approx - window), min(y1 - 1, approx + window)
        cut = min(range(lo, hi), key=lambda yy: profile[yy]) if hi > lo else approx
        cuts.append(cut)
    cuts.append(y1)
    return [(cuts[i], cuts[i + 1]) for i in range(len(cuts) - 1)]


def detect_bands(profile, target):
    """Find `target` row bands; splits bands where rows touch and divided evenly fails."""
    runs = axis_runs(profile, min_gap=2)
    if len(runs) == target:
        return runs
    heights = sorted(b[1] - b[0] for b in runs)
    med = heights[len(heights) // 2] or 1
    out = []
    for b in runs:
        k = max(1, int(round((b[1] - b[0]) / med)))
        out.extend(split_band_at_min(b, k, profile))
    if len(out) != target:
        return even_bands(len(profile), target)
    return out


def pack_grid(grid_path, grid_rows=6, grid_cols=None):
    """Slice a full grid image: rows = actions (in order), cols = frames."""
    im = Image.open(grid_path).convert("RGBA")
    bg = background_color(im)
    im = key_background(im, bg)
    mask = make_mask(im, bg)
    w, h = im.size

    row_profile = [sum(1 for x in range(w) if mask[x][y]) for y in range(h)]
    row_runs = detect_bands(row_profile, grid_rows) if grid_rows else axis_runs(row_profile, min_gap=2)

    rows = {}
    for ri, (ry0, ry1) in enumerate(row_runs):
        if ri >= len(ACTIONS):
            break
        action = ACTIONS[ri]
        col_profile = [sum(1 for y in range(ry0, ry1) if mask[x][y]) for x in range(w)]
        col_runs = axis_runs(col_profile, min_gap=2)
        if grid_cols and len(col_runs) != grid_cols:
            col_runs = even_bands(w, grid_cols)
        frames = []
        for (cx0, cx1) in col_runs:
            cell = im.crop((cx0, ry0, cx1, ry1))
            sub = make_mask(cell, bg)
            cw, ch = cell.size
            xs = [x for x in range(cw) if any(sub[x][y] for y in range(ch))]
            ys = [y for y in range(ch) if any(sub[x][y] for x in range(cw))]
            if not xs or not ys:
                continue
            tight = cell.crop((xs[0], ys[0], xs[-1] + 1, ys[-1] + 1))
            frames.append(tight)
        rows[action] = frames
        print(f"  {action}: {len(frames)} frames (grid row {ri + 1})")
    return rows


def natural_key(name):
    m = re.search(r"\((\d+)\)", name)
    return int(m.group(1)) if m else 0


def pack_frame_folders(root):
    """Each subfolder is one action holding individual frame images.

    Folder names may use aliases (`ideal` -> idle, `run` -> walk). Frames are
    ordered so `image.png` is frame 0 and `image (k).png` is frame k.
    """
    exts = (".png", ".webp", ".jpg", ".jpeg", ".gif")
    rows = {}
    for entry in sorted(os.listdir(root)):
        d = os.path.join(root, entry)
        if not os.path.isdir(d):
            continue
        action = ALIASES.get(entry.lower(), entry.lower())
        if action not in ACTIONS:
            continue
        files = [f for f in os.listdir(d) if f.lower().endswith(exts)]
        files.sort(key=natural_key)
        frames = []
        for f in files:
            im = Image.open(os.path.join(d, f)).convert("RGBA")
            bg = background_color(im)
            im = key_background(im, bg)
            if im.getbbox() is None:
                continue
            frames.append(im.crop(im.getbbox()))
        rows[action] = frames
        print(f"  {action}: {len(frames)} frames  <- {entry}/")
    return rows


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--pet", required=True, help="hamster | calico | dog | ...")
    ap.add_argument("--strips", help="directory of <action>.png strips")
    ap.add_argument("--frames", help="directory of per-action subfolders of frame images")
    ap.add_argument("--grid", help="single full-grid image")
    ap.add_argument("--grid-rows", type=int, default=6, help="rows in --grid (default 6)")
    ap.add_argument("--grid-cols", type=int, default=None, help="cols in --grid (auto if omitted)")
    ap.add_argument("--out", help="output path (default assets/pet/<pet>.png)")
    args = ap.parse_args()

    print(f"Packing '{args.pet}'...")
    if args.frames:
        rows = pack_frame_folders(args.frames)
    elif args.strips:
        rows = pack_strips(args.pet, args.strips)
    elif args.grid:
        rows = pack_grid(args.grid, args.grid_rows, args.grid_cols)
    else:
        raise SystemExit("Provide --frames DIR, --strips DIR or --grid FILE")

    if not rows:
        raise SystemExit("No frames found. Check file names (idle/alert/walk/drop/sleep/groom).")

    rows = normalize_rows(rows)
    sheet = build_sheet(rows)
    out = args.out or os.path.join("assets", "pet", f"{args.pet}.png")
    sheet.save(out)
    print(f"Saved {out} ({sheet.width}x{sheet.height})")


if __name__ == "__main__":
    main()
