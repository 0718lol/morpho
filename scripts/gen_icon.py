#!/usr/bin/env python3
"""Generate Morpho's 1024x1024 app icon: gradient tile + butterfly mark."""
import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

S = 1024
icons = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"
icons.mkdir(parents=True, exist_ok=True)

img = Image.new("RGBA", (S, S), (0, 0, 0, 0))

# ---- gradient rounded-square background ----
top = (99, 102, 241)     # indigo-500
bottom = (168, 85, 247)  # purple-500
grad = Image.new("RGBA", (S, S))
gd = ImageDraw.Draw(grad)
for y in range(S):
    t = y / S
    c = tuple(int(top[i] + (bottom[i] - top[i]) * t) for i in range(3)) + (255,)
    gd.line([(0, y), (S, y)], fill=c)

mask = Image.new("L", (S, S), 0)
md = ImageDraw.Draw(mask)
md.rounded_rectangle([0, 0, S, S], radius=224, fill=255)
img.paste(grad, (0, 0), mask)

# ---- butterfly: 4 wings each rotated about its own center, gap at the body ----
cx, cy = S / 2, S * 0.54
white = (255, 255, 255, 255)

# soft drop shadow for depth
shadow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
sd = ImageDraw.Draw(shadow)


def draw_wing(layer_center, size, angle):
    """One wing: ellipse drawn on its own tile, rotated about its center."""
    w, h = size
    tile = Image.new("RGBA", (w + 80, h + 80), (0, 0, 0, 0))
    d = ImageDraw.Draw(tile)
    d.ellipse([40, 40, 40 + w, 40 + h], fill=white)
    tile = tile.rotate(angle, expand=True, resample=Image.BICUBIC)
    px = int(layer_center[0] - tile.width / 2)
    py = int(layer_center[1] - tile.height / 2)
    return tile, (px, py)


wings = [
    # center, (w,h), angle   -- upper wings sweep up/out, lower wings down/out
    ((cx - 175, cy - 145), (390, 310), -30),
    ((cx + 175, cy - 145), (390, 310), 30),
    ((cx - 140, cy + 175), (270, 240), 24),
    ((cx + 140, cy + 175), (270, 240), -24),
]

for center, size, angle in wings:
    tile, pos = draw_wing(center, size, angle)
    sd.ellipse([pos[0] + 20, pos[1] + 30, pos[0] + tile.width - 10, pos[1] + tile.height],
               fill=(30, 20, 80, 90))
shadow = shadow.filter(ImageFilter.GaussianBlur(24))
img.alpha_composite(shadow)

for center, size, angle in wings:
    tile, pos = draw_wing(center, size, angle)
    img.alpha_composite(tile, pos)

# ---- body, head, antennae ----
d = ImageDraw.Draw(img)
d.rounded_rectangle([cx - 20, cy - 210, cx + 20, cy + 330], radius=20, fill=white)
d.ellipse([cx - 34, cy - 268, cx + 34, cy - 200], fill=white)
for sign in (-1, 1):
    pts = []
    for i in range(25):
        t = i / 24
        x = cx + sign * (10 + t * 120 + 10 * math.sin(t * 5))
        y = cy - 250 - t * 150
        pts.append((x, y))
    d.line(pts, fill=white, width=12, joint="curve")

out = icons / "app-icon.png"
img.save(out)
print(f"wrote {out}")
