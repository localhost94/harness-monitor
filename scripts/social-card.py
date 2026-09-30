#!/usr/bin/env python3
"""Builds docs/social-preview.png, the card GitHub shows when the repo is linked.

The two widgets are pasted as real screenshots rather than drawn, so the card
cannot describe a UI the app no longer has - the previous card still showed the
orange/green accent palette from before the black-and-white redesign, and
nothing in the repo would have caught it.

Needs two dark-theme previews first, both gitignored:

    ./scripts/preview.sh 440 82 preview-darkpill.png collapsed theme=dark
    ./scripts/preview.sh 118 300 preview-darkvert.png shape=vertical collapsed theme=dark
    python3 scripts/social-card.py

The background is the same vertical gradient the first card used, so the link
preview does not change colour between releases.
"""

import os

from PIL import Image, ImageDraw, ImageFont

WIDTH, HEIGHT = 1280, 640
TOP, BOTTOM = (36, 41, 76), (21, 24, 44)
ACCENT = (240, 138, 30)
WHITE, MUTED, FAINT = (238, 240, 245), (178, 185, 199), (150, 158, 175)

SANS = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"
MONO = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "docs", "social-preview.png")


def gradient_card() -> Image.Image:
    card = Image.new("RGB", (WIDTH, HEIGHT))
    pixels = card.load()
    for y in range(HEIGHT):
        t = y / (HEIGHT - 1)
        row = tuple(round(TOP[i] + (BOTTOM[i] - TOP[i]) * t) for i in range(3))
        for x in range(WIDTH):
            pixels[x, y] = row
    return card


def shot(name: str, scale: float) -> Image.Image:
    """A preview, trimmed and scaled for the card.

    The page background is transparent by design - Tauri has to see the desktop
    through it - and Chrome paints that white, so every screenshot carries a
    hairline white edge. Trim it. Keying the white out by colour instead would
    take the widget's own light chips with it, and at card scale the corner
    radius lost to a 3px trim is not visible.
    """
    image = Image.open(os.path.join(ROOT, name)).convert("RGB")
    image = image.crop((3, 3, image.width - 3, image.height - 3))
    if scale != 1:
        image = image.resize(
            (round(image.width * scale), round(image.height * scale)), Image.LANCZOS
        )
    return image


def main() -> None:
    card = gradient_card()
    draw = ImageDraw.Draw(card)

    draw.rectangle((0, 0, 7, HEIGHT), fill=ACCENT)
    draw.text((72, 92), "harness-monitor", font=ImageFont.truetype(SANS, 62), fill=WHITE)
    for i, line in enumerate(
        (
            "Know the moment your AI coding agent stops working",
            "and starts waiting for you.",
        )
    ):
        draw.text((76, 178 + i * 44), line, font=ImageFont.truetype(SANS, 33), fill=MUTED)

    pill = shot("preview-darkpill.png", 1.18)
    card.paste(pill, (72, 320))
    strip = shot("preview-darkvert.png", 1.5)
    card.paste(strip, (WIDTH - 78 - strip.width, 112))

    draw.text(
        (76, 566),
        "MIT  ·  Rust + Tauri  ·  Windows + WSL2  ·  fully local",
        font=ImageFont.truetype(MONO, 21),
        fill=FAINT,
    )

    card.save(OUT, optimize=True)
    print(f"{OUT}  {os.path.getsize(OUT)} bytes")


if __name__ == "__main__":
    main()
