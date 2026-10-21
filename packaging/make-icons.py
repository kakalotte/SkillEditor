#!/usr/bin/env python3
"""Generate the SkillForge application icons.

Reproducible: the icon is drawn from scratch, so the PNG/ICNS/ICO files in
`src-tauri/icons/` can always be regenerated instead of being opaque binaries.

Usage:
    python3 packaging/make-icons.py

Requires Pillow (`python3 -c "import PIL"`). `iconutil` (macOS) is used for the
`.icns`; when it is unavailable the script still emits the PNG/ICO files.
"""

from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw

# Accent palette, matching the app's light theme.
ACCENT = (47, 111, 235, 255)
ACCENT_DARK = (32, 82, 186, 255)
ACCENT_DARKER = (24, 62, 143, 255)
WHITE = (255, 255, 255, 255)

ROOT = Path(__file__).resolve().parent.parent
ICON_DIR = ROOT / "src-tauri" / "icons"

# Supersampling factor so the rounded corners and bars look smooth.
SS = 8
BASE = 1024


def render(size: int) -> Image.Image:
    """Draw the master icon at a large size, then downscale."""
    master = BASE
    img = Image.new("RGBA", (master, master), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    # Rounded-square background with a subtle top-to-bottom gradient.
    radius = int(master * 0.22)
    for y in range(master):
        t = y / (master - 1)
        if t < 0.5:
            k = t / 0.5
            color = tuple(
                round(ACCENT[i] + (ACCENT_DARK[i] - ACCENT[i]) * k) for i in range(3)
            )
        else:
            k = (t - 0.5) / 0.5
            color = tuple(
                round(ACCENT_DARK[i] + (ACCENT_DARKER[i] - ACCENT_DARK[i]) * k)
                for i in range(3)
            )
        draw.line([(0, y), (master, y)], fill=color + (255,))

    mask = Image.new("L", (master, master), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [0, 0, master - 1, master - 1], radius=radius, fill=255
    )
    img.putalpha(mask)

    # Three white bars of decreasing width: a "skill list" / document glyph.
    bar_h = int(master * 0.085)
    bar_r = bar_h // 2
    x0 = int(master * 0.26)
    widths = [0.48, 0.36, 0.26]
    ys = [0.30, 0.46, 0.62]
    for w, y in zip(widths, ys):
        x1 = x0 + int(master * w)
        y0 = int(master * y)
        draw.rounded_rectangle([x0, y0, x1, y0 + bar_h], radius=bar_r, fill=WHITE)

    if size == master:
        return img
    return img.resize((size, size), Image.LANCZOS)


def main() -> int:
    ICON_DIR.mkdir(parents=True, exist_ok=True)

    pngs = {
        "32x32.png": 32,
        "128x128.png": 128,
        "128x128@2x.png": 256,
        "icon.png": 512,
        "Square150x150Logo.png": 150,
        "Square44x44Logo.png": 44,
        "StoreLogo.png": 50,
    }
    for name, size in pngs.items():
        render(size).save(ICON_DIR / name)
        print(f"wrote {ICON_DIR / name}")

    # Multi-resolution .ico (Windows).
    sizes = [16, 24, 32, 48, 64, 128, 256]
    render(256).save(
        ICON_DIR / "icon.ico", format="ICO", sizes=[(s, s) for s in sizes]
    )
    print(f"wrote {ICON_DIR / 'icon.ico'}")

    # .icns (macOS) via iconutil.
    if shutil.which("iconutil"):
        with tempfile.TemporaryDirectory() as tmp:
            iconset = Path(tmp) / "icon.iconset"
            iconset.mkdir()
            for size in [16, 32, 128, 256, 512]:
                render(size).save(iconset / f"icon_{size}x{size}.png")
                render(size * 2).save(iconset / f"icon_{size}x{size}@2x.png")
            subprocess.run(
                ["iconutil", "-c", "icns", str(iconset), "-o", str(ICON_DIR / "icon.icns")],
                check=True,
            )
        print(f"wrote {ICON_DIR / 'icon.icns'}")
    else:
        print("iconutil not found; skipped icon.icns (macOS only)", file=sys.stderr)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
