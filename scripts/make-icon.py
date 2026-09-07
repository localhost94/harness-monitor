#!/usr/bin/env python3
"""Generates the HarnessMonitor app icon.

The mark is the app in one glyph: an outer gauge ring (the plan-quota window,
drawn as a partial arc) around a pulsing dot (a session that wants your
attention). Two shapes only, so it still reads at 16px in a taskbar.

Run:  python3 scripts/make-icon.py && cargo tauri icon icon-source.png
"""
from PIL import Image, ImageDraw

SS = 4                      # supersample factor
SIZE = 1024
W = SIZE * SS

BG_TOP = (28, 32, 40)
BG_BOTTOM = (13, 15, 19)
RING_TRACK = (55, 62, 74)
RING_USED = (56, 189, 248)  # sky - "running"
PULSE = (251, 191, 36)      # amber - "needs you"


def rounded_mask(size: int, radius: int) -> Image.Image:
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, size - 1, size - 1], radius, fill=255)
    return mask


def vertical_gradient(size: int, top, bottom) -> Image.Image:
    grad = Image.new("RGB", (1, size))
    px = grad.load()
    for y in range(size):
        t = y / (size - 1)
        px[0, y] = tuple(round(a + (b - a) * t) for a, b in zip(top, bottom))
    return grad.resize((size, size))


def main() -> None:
    base = vertical_gradient(W, BG_TOP, BG_BOTTOM).convert("RGBA")
    draw = ImageDraw.Draw(base)

    cx = cy = W / 2
    ring_r = W * 0.30
    ring_w = int(W * 0.075)
    box = [cx - ring_r, cy - ring_r, cx + ring_r, cy + ring_r]

    # Full track, then the "used" arc on top: a gauge at roughly two thirds.
    draw.arc(box, start=0, end=360, fill=RING_TRACK, width=ring_w)
    draw.arc(box, start=-215, end=25, fill=RING_USED, width=ring_w)

    # Pulse halo, then the solid dot.
    halo = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    hd = ImageDraw.Draw(halo)
    halo_r = W * 0.19
    hd.ellipse([cx - halo_r, cy - halo_r, cx + halo_r, cy + halo_r], fill=PULSE + (45,))
    base = Image.alpha_composite(base, halo)

    draw = ImageDraw.Draw(base)
    dot_r = W * 0.115
    draw.ellipse([cx - dot_r, cy - dot_r, cx + dot_r, cy + dot_r], fill=PULSE + (255,))

    base.putalpha(rounded_mask(W, int(W * 0.22)))
    base.resize((SIZE, SIZE), Image.LANCZOS).save("icon-source.png")
    print("wrote icon-source.png")


if __name__ == "__main__":
    main()
