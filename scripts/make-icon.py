#!/usr/bin/env python3
"""Build every PuriCat icon from one square, full-bleed artwork file.

    python3 scripts/make-icon.py branding/source/kawaii-b.png [--zoom 1.0]
    pnpm tauri icon branding/app-icon.png      # then regenerate the platform icon set

Writes:
  branding/app-icon.png      1024px macOS-style icon (rounded "squircle" body on the 824px grid)
  src/assets/brand-mark.png  256px tightly cropped copy used inside the app

Requires Pillow (`pip install pillow`).
"""
import argparse
import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
BODY = 824  # Apple's icon grid: 824px body centred on a 1024px canvas


def squircle_mask(size: int, exponent: float = 5.0) -> Image.Image:
    """Superellipse |x|^n + |y|^n = 1, a close match for the macOS icon shape."""
    mask = Image.new("L", (size, size), 0)
    points = []
    for i in range(1440):
        t = 2 * math.pi * i / 1440
        c, s = math.cos(t), math.sin(t)
        x = math.copysign(abs(c) ** (2 / exponent), c)
        y = math.copysign(abs(s) ** (2 / exponent), s)
        points.append((size / 2 + x * size / 2, size / 2 + y * size / 2))
    ImageDraw.Draw(mask).polygon(points, fill=255)
    return mask


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("source", type=Path, help="square artwork that fills the whole canvas")
    parser.add_argument("--zoom", type=float, default=1.0, help=">1 crops into the centre of the artwork")
    args = parser.parse_args()

    art = Image.open(args.source).convert("RGB")
    side = min(art.size)
    crop = round(side / args.zoom)
    left, top = (art.width - crop) // 2, (art.height - crop) // 2
    art = art.crop((left, top, left + crop, top + crop))

    # Render the body at 4x and scale down for a clean anti-aliased edge.
    body = art.resize((BODY * 4, BODY * 4), Image.LANCZOS)
    body.putalpha(squircle_mask(BODY * 4))
    body = body.resize((BODY, BODY), Image.LANCZOS)

    canvas = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
    shadow = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
    tint = Image.new("RGBA", (BODY, BODY), (110, 45, 0, 255))
    tint.putalpha(body.getchannel("A").point(lambda a: a * 70 // 255))
    shadow.paste(tint, (100, 112), tint)
    canvas = Image.alpha_composite(canvas, shadow.filter(ImageFilter.GaussianBlur(14)))
    canvas.paste(body, (100, 100), body)

    (ROOT / "branding").mkdir(exist_ok=True)
    canvas.save(ROOT / "branding" / "app-icon.png")
    (ROOT / "src" / "assets").mkdir(parents=True, exist_ok=True)
    body.resize((256, 256), Image.LANCZOS).save(ROOT / "src" / "assets" / "brand-mark.png", optimize=True)
    print("wrote branding/app-icon.png and src/assets/brand-mark.png")


if __name__ == "__main__":
    main()
