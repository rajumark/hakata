#!/usr/bin/env python3
"""Regenerate all platform icon assets from the source artwork.

The source is a glyph on a transparent background; every output composites
it onto an opaque white square with 10 % padding on each side (the glyph
is scaled to 80 % of the canvas). Run from the repo root:

    python3 scripts/gen-icons.py
"""

import os
import shutil
import subprocess
import sys
import tempfile

from PIL import Image

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SOURCE = os.path.join(ROOT, "resources", "icon", "source.png")
ICON_DIR = os.path.join(ROOT, "resources", "icon")
HICOLOR = os.path.join(ICON_DIR, "hicolor")
ICON_FILL = 0.80  # glyph occupies 80 % → 10 % padding each side
MAC_SIZES = [
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024),
]
LINUX_SIZES = [16, 32, 48, 64, 128, 256, 512]
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]


def white_background(source: str, size: int) -> Image.Image:
    icon = Image.open(source).convert("RGBA")
    # Crop to the opaque content so transparent edges don't waste space.
    bbox = icon.getbbox()
    if bbox:
        icon = icon.crop(bbox)
    # Scale the glyph to ICON_FILL of the canvas, centered on white.
    max_icon = int(size * ICON_FILL)
    icon.thumbnail((max_icon, max_icon), Image.LANCZOS)
    canvas = Image.new("RGBA", (size, size), (255, 255, 255, 255))
    x = (size - icon.width) // 2
    y = (size - icon.height) // 2
    canvas.alpha_composite(icon, (x, y))
    return canvas.convert("RGB")


def main() -> None:
    if not os.path.exists(SOURCE):
        sys.exit(f"missing source icon: {SOURCE}")

    master = white_background(SOURCE, 512)
    master.save(os.path.join(ICON_DIR, "hakata-512.png"))

    iconset = tempfile.mkdtemp(suffix=".iconset")
    try:
        for name, size in MAC_SIZES:
            white_background(SOURCE, size).save(os.path.join(iconset, name))
        icns = os.path.join(ICON_DIR, "AppIcon.icns")
        subprocess.run(
            ["iconutil", "-c", "icns", iconset, "-o", icns],
            check=True,
        )
    finally:
        shutil.rmtree(iconset)

    for size in LINUX_SIZES:
        directory = os.path.join(HICOLOR, f"{size}x{size}", "apps")
        os.makedirs(directory, exist_ok=True)
        white_background(SOURCE, size).save(
            os.path.join(directory, "hakata.png")
        )

    ico = os.path.join(ICON_DIR, "hakata.ico")
    white_background(SOURCE, 256).save(
        ico, format="ICO", sizes=[(size, size) for size in ICO_SIZES]
    )

    print(f"wrote {os.path.join(ICON_DIR, 'hakata-512.png')}")
    print(f"wrote {os.path.join(ICON_DIR, 'AppIcon.icns')}")
    print(f"wrote hicolor icons under {HICOLOR}")
    print(f"wrote {ico}")


if __name__ == "__main__":
    main()
