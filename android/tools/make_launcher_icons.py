#!/usr/bin/env python3
"""Render the Android launcher icons from the project's two source PNGs.

The desktop picks its window icon at run time (`src/app/window_icon.rs`): the
retail red star read out of the player's `GAME.EXE`, or VERA's artwork. An
Android launcher icon has to be a packaged resource, so both are rendered here
once and checked in:

  .github/release/icon.png   the red star the release packages ship
  src/app/vera_icon.png      the artwork the VERA Icon box selects

Run from the repository root with Pillow available:

  python android/tools/make_launcher_icons.py

`docs/android.md` records why the icons are generated rather than shipped as a
single drawable.
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
RES = ROOT / "android/app/src/main/res"

# Android's launcher icon densities, in dp.
DENSITIES = {
    "mdpi": 1.0,
    "hdpi": 1.5,
    "xhdpi": 2.0,
    "xxhdpi": 3.0,
    "xxxhdpi": 4.0,
}

# A launcher icon is 48 dp; an adaptive icon's foreground is 108 dp with the
# artwork kept inside the 72 dp safe zone so any mask crop keeps it visible.
LEGACY_DP = 48
ADAPTIVE_DP = 108
SAFE_ZONE_DP = 72

ICONS = {
    "retail": ROOT / ".github/release/icon.png",
    "vera": ROOT / "src/app/vera_icon.png",
}


def dominant_colour(image: Image.Image) -> tuple[int, int, int]:
    """The opaque pixels' median colour, the adaptive icon's background."""
    raw = image.tobytes()
    opaque = [
        (raw[i], raw[i + 1], raw[i + 2])
        for i in range(0, len(raw), 4)
        if raw[i + 3] > 200  # ignore the soft transparent rim
    ]
    if not opaque:
        return (0, 0, 0)
    channels = list(zip(*opaque))
    return tuple(sorted(channel)[len(channel) // 2] for channel in channels)  # type: ignore[return-value]


def render(source: Image.Image, size: int) -> Image.Image:
    return source.resize((size, size), Image.LANCZOS)


def main() -> None:
    RES.mkdir(parents=True, exist_ok=True)
    background_lines = []
    for name, path in ICONS.items():
        source = Image.open(path).convert("RGBA")
        for density, factor in DENSITIES.items():
            folder = RES / f"mipmap-{density}"
            folder.mkdir(parents=True, exist_ok=True)

            legacy = render(source, round(LEGACY_DP * factor))
            legacy.save(folder / f"ic_launcher_{name}.png")

            canvas_px = round(ADAPTIVE_DP * factor)
            artwork = render(source, round(SAFE_ZONE_DP * factor))
            canvas = Image.new("RGBA", (canvas_px, canvas_px), (0, 0, 0, 0))
            offset = (canvas_px - artwork.width) // 2
            canvas.paste(artwork, (offset, offset), artwork)
            canvas.save(folder / f"ic_launcher_{name}_foreground.png")

        red, green, blue = dominant_colour(source)
        background_lines.append(
            f'    <color name="ic_launcher_{name}_background">'
            f"#{red:02X}{green:02X}{blue:02X}</color>"
        )

        anydpi = RES / "mipmap-anydpi-v26"
        anydpi.mkdir(parents=True, exist_ok=True)
        (anydpi / f"ic_launcher_{name}.xml").write_text(
            '<?xml version="1.0" encoding="utf-8"?>\n'
            '<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">\n'
            f'    <background android:drawable="@color/ic_launcher_{name}_background" />\n'
            f'    <foreground android:drawable="@mipmap/ic_launcher_{name}_foreground" />\n'
            "</adaptive-icon>\n",
            encoding="utf-8",
        )

    (RES / "values/ic_launcher_backgrounds.xml").write_text(
        '<?xml version="1.0" encoding="utf-8"?>\n'
        '<resources>\n' + "\n".join(background_lines) + "\n</resources>\n",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
