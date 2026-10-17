"""Make the MSIX package's tile and logo images from Kabuk's app icon (DECISIONS D84).

Usage: python tools/msix-assets.py <Assets folder>

The square images are the app icon itself at each size and scale; the small ones the
taskbar and Start show use the icon drawn for that size. The wide tile holds the icon in
its middle on a see-through ground, as the square tiles do (BackgroundColor transparent).
"""
import os
import sys

from PIL import Image

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ICONS = os.path.join(ROOT, "design", "logo", "png", "app-icon-dark")
# The sizes drawn for small places (design/logo/source/generate.py).
DRAWN = (16, 20, 24, 32, 40, 48, 64, 128, 256, 512, 1024)
# Each image at 100 % and 200 % scale; the sizes Windows asks for.
SCALES = (100, 200)
SQUARES = {"Square44x44Logo": 44, "Square150x150Logo": 150, "StoreLogo": 50}
WIDE = ("Wide310x150Logo", 310, 150)
# The icon's height in the wide tile, against the tile's height.
WIDE_ICON = 0.66
# The taskbar's and Start's own sizes, unplated (drawn as they are, without a tile).
TARGETS = (16, 24, 32, 48, 256)


def icon(size):
    """The app icon at `size` px: the one drawn for it, or the nearest bigger shrunk."""
    best = min(s for s in DRAWN if s >= size)
    im = Image.open(os.path.join(ICONS, f"kabuk-app-icon-dark-{best}.png")).convert("RGBA")
    return im if best == size else im.resize((size, size), Image.LANCZOS)


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__.strip())
    out = sys.argv[1]
    os.makedirs(out, exist_ok=True)
    for name, size in SQUARES.items():
        for scale in SCALES:
            icon(size * scale // 100).save(os.path.join(out, f"{name}.scale-{scale}.png"))
    for target in TARGETS:
        for suffix in ("", "_altform-unplated"):
            icon(target).save(os.path.join(out, f"Square44x44Logo.targetsize-{target}{suffix}.png"))
    name, w, h = WIDE
    for scale in SCALES:
        tw, th = w * scale // 100, h * scale // 100
        tile = Image.new("RGBA", (tw, th), (0, 0, 0, 0))
        side = round(th * WIDE_ICON)
        tile.alpha_composite(icon(side), ((tw - side) // 2, (th - side) // 2))
        tile.save(os.path.join(out, f"{name}.scale-{scale}.png"))


if __name__ == "__main__":
    main()
