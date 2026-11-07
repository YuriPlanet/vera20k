"""Turn Red Alert 3's end-game art into VERA's two end-game pictures.

VERA paints a finished match **twice**, and the two paints want different
pictures (see `src/rules/end_game.rs` for the key names they answer to):

* **Banner** — over the living battlefield when the result lands, where the
  retail `GRFXTXT.SHP` (a very flat 436x112 strip) goes. This must be the RA3
  end-game **card** whole and unclipped: a 354x512 portrait. The battle is still
  visible behind it, so nothing may be cropped off it.
* **Score** — the **score page's** own full-screen background, 800x600 like the
  side's `MP*SCRNL.SHP`. This is artwork, a wide illustration, so it is cropped
  to fill rather than letterboxed (`--anchor` picks the surviving band).

The source names below are Red Alert 3's own. Cards came out of
`RA3SDK_UI_ScreensPack/TacticalEndGameDialog/art/Textures` as three 1024x512
atlases of two 354x512 cards each; they are now extracted side by side as
`tex<atlas>_<half>`. Only the Allied and Soviet pair exists — VERA's third side
is Yuri and the Imperial cards (`tex1_R`, `tex3_L`) have nobody to show them to.

Output, all 8.3 and uppercase the way the engine looks files up:

| File | Place | Side | Outcome |
| --- | --- | --- | --- |
| `ENDAWIN.SHP` + `.PAL` | banner | Allied | victory |
| `ENDALOS.SHP` + `.PAL` | banner | Allied | defeat |
| `ENDSWIN.SHP` + `.PAL` | banner | Soviet | victory |
| `ENDSLOS.SHP` + `.PAL` | banner | Soviet | defeat |
| `SCAWIN.SHP` + `.PAL` | score | Allied | victory |
| `SCALOS.SHP` + `.PAL` | score | Allied | defeat |
| `SCSWIN.SHP` + `.PAL` | score | Soviet | victory |
| `SCSLOS.SHP` + `.PAL` | score | Soviet | defeat |

The score page has one artwork per side, not one per outcome, so each side's
artwork is written twice — once for the win, once for the loss. Drop in a second
file and change the `SCORES` table when there is a distinct losing picture.

Formats, matching what `src/assets/shp_file.rs` and `src/assets/pal_file.rs`
read:

* SHP(TS): `u16 0, u16 width, u16 height, u16 frames`, then 24-byte frame
  headers, then the pixel data. One frame, `format = 0` (raw, not RLE), offset
  32. Every pixel is a palette index; the blitter skips index 0, so the
  quantizer never emits it and the background stays opaque.
* PAL: 768 bytes, 256 entries of R, G, B in the VGA 6-bit range. The engine
  stores each component as `raw << 2`, so palette colours are snapped to
  multiples of 4 and the quantizer measures distances against those.

Pillow does the decoding and the resampling; everything past that — the
quantizer and the two file formats — is this script's own, because those are the
formats VERA reads and nothing else produces them.

Usage:

    python -m tools.end_game_cards --sources <dir> --out <dir>
    python -m tools.end_game_cards ... --only soviet-win --anchor 0.4 --dim 0.86

The script writes nothing into the checkout unless told to: point `--out` at
wherever the game will read the shapes from.
"""
from __future__ import annotations

import argparse
import struct
import sys
from pathlib import Path

from PIL import Image

#: What a score-page background must be: the dialog is 800x600 and draws the
#: side's art at its origin, so a background that is not this size letterboxes.
SCORE_W = 800
SCORE_H = 600

#: role, name, source file stem, output stems, vertical crop anchor, note
CARDS = (
    ("banner", "allied-win", "tex2_L", ("ENDAWIN",), 0.0, "the Allied poster, clean and unburnt"),
    ("banner", "allied-lose", "tex3_R", ("ENDALOS",), 0.0, "the same poster, bandaged and shot up"),
    ("banner", "soviet-win", "tex2_R", ("ENDSWIN",), 0.0, "the soldier raising the flag"),
    ("banner", "soviet-lose", "tex1_L", ("ENDSLOS",), 0.0, "the soldier kneeling in the ruins"),
    ("score", "allied", "SCORE_ALLIED", ("SCAWIN", "SCALOS"), 0.5, "Allied artwork"),
    ("score", "soviet", "SCORE_SOVIET", ("SCSWIN", "SCSLOS"), 0.5, "Soviet artwork"),
)

# The Imperial cards (`tex1_R`, `tex3_L`) are deliberately absent: RA2's third
# side is Yuri and there is no picture for it, so nothing would ever draw them.


def open_source(directory: Path, stem: str) -> Image.Image:
    """One source picture, flattened to RGB so the quantizer has one shape.

    Accepts whatever Pillow reads; the card extracts are PNG and the artwork is
    JPEG, and both carry colour the output palette has to be chosen from.
    """
    for suffix in (".png", ".PNG", ".jpg", ".JPG", ".jpeg", ".JPEG", ".tga", ".TGA"):
        path = directory / f"{stem}{suffix}"
        if path.is_file():
            with Image.open(path) as image:
                image.load()
                if image.mode != "RGB":
                    image = image.convert("RGB")
                return image
    raise SystemExit(f"missing source picture: {directory / (stem + '.*')}")


def fit_banner(image: Image.Image, max_height: int | None) -> Image.Image:
    """Shrink the whole card to fit, keeping every part of it.

    Nothing is ever cropped here and nothing is ever enlarged: the engine's own
    fit already refuses to magnify a card larger than its pixel size, so a
    smaller source would only lose detail on the way in.
    """
    if max_height is None or image.height <= max_height:
        return image
    width = max(1, round(image.width * max_height / image.height))
    return image.resize((width, max_height), Image.LANCZOS)


def fit_score(image: Image.Image, anchor: float) -> Image.Image:
    """Crop the artwork to fill exactly 800x600, `anchor` picking the band.

    A landscape illustration will give up its sides; a taller one gives up the
    band that `anchor` does not keep (0 the top, 1 the bottom).
    """
    return _ops_fit(image, (SCORE_W, SCORE_H), anchor)


def _ops_fit(image: Image.Image, size: tuple[int, int], anchor: float) -> Image.Image:
    """`ImageOps.fit` without the import, so the centring is spelled locally."""
    target_w, target_h = size
    scale = max(target_w / image.width, target_h / image.height)
    scaled_w = max(target_w, round(image.width * scale))
    scaled_h = max(target_h, round(image.height * scale))
    scaled = image.resize((scaled_w, scaled_h), Image.LANCZOS)
    left = round((scaled_w - target_w) * 0.5)
    top = round((scaled_h - target_h) * min(1.0, max(0.0, anchor)))
    return scaled.crop((left, top, left + target_w, top + target_h))


def rows_of(image: Image.Image) -> list[list[tuple[int, int, int]]]:
    """The picture as rows of RGB, the shape the rest of the script works on."""
    width, height = image.size
    flat = list(image.get_flattened_data() if hasattr(image, "get_flattened_data") else image.getdata())
    return [flat[y * width : (y + 1) * width] for y in range(height)]


def dim(rows: list[list[tuple[int, int, int]]], factor: float) -> None:
    """Scale every channel in place. Below 1 so the table reads over the art."""
    if factor >= 1.0:
        return
    for line in rows:
        for index, (r, g, b) in enumerate(line):
            line[index] = (int(r * factor), int(g * factor), int(b * factor))


def _key(colour: tuple[int, int, int]) -> int:
    """The 5-5-5 bucket a colour falls in, as median cut's histogram key."""
    return ((colour[0] >> 3) << 10) | ((colour[1] >> 3) << 5) | (colour[2] >> 3)


def _unkey(key: int) -> tuple[int, int, int]:
    return ((key >> 10) & 31) << 3, ((key >> 5) & 31) << 3, (key & 31) << 3


def median_cut(histogram: dict[int, int], colours: int) -> list[tuple[int, int, int]]:
    """Pick `colours` representatives for the histogram, weighted by count."""
    boxes: list[list[tuple[int, int]]] = [sorted(histogram.items())]
    while len(boxes) < colours:
        # Split the box whose colours span the widest single channel, so the
        # palette spends its entries where the picture actually varies.
        best = -1
        best_span = 0
        best_channel = 0
        for index, box in enumerate(boxes):
            if len(box) < 2:
                continue
            for channel in range(3):
                values = [_unkey(entry[0])[channel] for entry in box]
                span = max(values) - min(values)
                if span > best_span:
                    best, best_span, best_channel = index, span, channel
        if best < 0:
            break
        box = boxes.pop(best)
        box.sort(key=lambda entry: _unkey(entry[0])[best_channel])
        total = sum(count for _, count in box)
        running = 0
        cut = len(box) - 1
        for position, (_, count) in enumerate(box):
            running += count
            if running * 2 >= total:
                cut = position
                break
        cut = min(max(cut, 0), len(box) - 2)
        boxes.append(box[: cut + 1])
        boxes.append(box[cut + 1 :])
    palette: list[tuple[int, int, int]] = []
    for box in boxes:
        weight = sum(count for _, count in box) or 1
        r = sum(_unkey(key)[0] * count for key, count in box) // weight
        g = sum(_unkey(key)[1] * count for key, count in box) // weight
        b = sum(_unkey(key)[2] * count for key, count in box) // weight
        # Snap to what a 6-bit palette can actually store.
        palette.append((r & 0xFC, g & 0xFC, b & 0xFC))
    return palette


def nearest_index(
    colour: tuple[int, int, int], palette: list[tuple[int, int, int]]
) -> int:
    """Index of the closest palette entry, weighted for how the eye sees RGB.

    Index 0 is skipped by the blitter, so the search starts at 1.
    """
    best = 1
    best_distance = 1 << 30
    for index in range(1, len(palette)):
        r, g, b = palette[index]
        dr = colour[0] - r
        dg = colour[1] - g
        db = colour[2] - b
        distance = 2 * dr * dr + 4 * dg * dg + 3 * db * db
        if distance < best_distance:
            best_distance = distance
            best = index
    return best


def quantize(
    rows: list[list[tuple[int, int, int]]], colours: int
) -> tuple[list[tuple[int, int, int]], list[int]]:
    """Reduce the picture to `colours` entries plus index 0, and map it."""
    histogram: dict[int, int] = {}
    for line in rows:
        for colour in line:
            key = _key(colour)
            histogram[key] = histogram.get(key, 0) + 1
    palette: list[tuple[int, int, int]] = [(0, 0, 0)]
    palette.extend(median_cut(histogram, colours))
    lookup: dict[int, int] = {key: nearest_index(_unkey(key), palette) for key in histogram}
    indices: list[int] = []
    for line in rows:
        for colour in line:
            indices.append(lookup[_key(colour)])
    return palette, indices


def palette_bytes(palette: list[tuple[int, int, int]]) -> bytes:
    """A .pal file: 256 entries of 6-bit R, G, B, padded with the last colour."""
    if len(palette) > 256:
        raise ValueError(f"{len(palette)} entries do not fit a 256-entry palette")
    entries = list(palette)
    while len(entries) < 256:
        entries.append(entries[-1] if entries else (0, 0, 0))
    out = bytearray()
    for r, g, b in entries:
        out.extend((r >> 2, g >> 2, b >> 2))
    return bytes(out)


def shp_bytes(width: int, height: int, indices: list[int]) -> bytes:
    """A one-frame SHP(TS) with raw (uncompressed) pixel data."""
    if len(indices) != width * height:
        raise ValueError(f"expected {width * height} pixels, got {len(indices)}")
    header = struct.pack("<HHHH", 0, width, height, 1)
    data_offset = len(header) + 24
    # x, y, width, height, format (0 = raw), 3 pad, radar colour, 1 unused,
    # a reserved word, then the absolute offset of the pixel data.
    frame = struct.pack("<HHHH", 0, 0, width, height)
    frame += bytes((0, 0, 0, 0))
    frame += bytes((0, 0, 0))
    frame += bytes((0,))
    frame += struct.pack("<I", 0)
    frame += struct.pack("<I", data_offset)
    return header + frame + bytes(indices)


def verify(shp_path: Path, pal_path: Path, width: int, height: int) -> str:
    """Read the pair back the way the game's parser does, and describe it."""
    shp = shp_path.read_bytes()
    pal = pal_path.read_bytes()
    if len(pal) != 768:
        raise ValueError(f"{pal_path.name}: {len(pal)} bytes, expected 768")
    zero, w, h, frames = struct.unpack_from("<HHHH", shp, 0)
    if zero != 0 or (w, h, frames) != (width, height, 1):
        raise ValueError(f"{shp_path.name}: header reads {zero},{w},{h},{frames}")
    offset = struct.unpack_from("<I", shp, 8 + 20)[0]
    if offset != 32:
        raise ValueError(f"{shp_path.name}: pixel data at {offset}, expected 32")
    pixels = shp[offset:]
    if len(pixels) != width * height:
        raise ValueError(f"{shp_path.name}: {len(pixels)} pixel bytes, expected {width * height}")
    unused = sum(1 for value in pixels if value == 0)
    if unused:
        raise ValueError(f"{shp_path.name}: {unused} pixels use the transparent index")
    return (
        f"{shp_path.name} {w}x{h}, 1 frame, {len(shp)} bytes; "
        f"{pal_path.name} 768 bytes, {len(set(pixels))} indices used"
    )


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--sources",
        type=Path,
        required=True,
        help="directory holding tex2_L.png, tex3_R.png, ... and SCORE_*.png",
    )
    parser.add_argument("--out", type=Path, required=True, help="where the SHP/PAL pairs go")
    parser.add_argument(
        "--only",
        action="append",
        default=None,
        metavar="NAME",
        help=f"convert one entry only ({', '.join(card[1] for card in CARDS)})",
    )
    parser.add_argument(
        "--anchor",
        type=float,
        default=None,
        help="override the score artwork's vertical crop anchor, 0 = top, 1 = bottom",
    )
    parser.add_argument(
        "--banner-height",
        type=int,
        default=None,
        metavar="PX",
        help="shrink the cards to this height in pixels; the default writes them "
        "at their own size, which the engine then fits without enlarging",
    )
    parser.add_argument(
        "--dim",
        type=float,
        default=1.0,
        help="scale every channel by this before quantising (default 1.0)",
    )
    parser.add_argument(
        "--colours", type=int, default=255, help="palette entries besides index 0"
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    wanted = {name.lower() for name in args.only} if args.only else None
    if wanted:
        unknown = wanted - {card[1] for card in CARDS}
        if unknown:
            raise SystemExit(f"unknown card(s): {', '.join(sorted(unknown))}")
    args.out.mkdir(parents=True, exist_ok=True)
    for role, name, source, stems, anchor, _ in CARDS:
        if wanted and name not in wanted:
            continue
        image = open_source(args.sources, source)
        if role == "banner":
            image = fit_banner(image, args.banner_height)
        else:
            image = fit_score(image, args.anchor if args.anchor is not None else anchor)
        rows = rows_of(image)
        dim(rows, args.dim)
        palette, indices = quantize(rows, args.colours)
        width, height = image.size
        for stem in stems:
            shp_path = args.out / f"{stem}.SHP"
            pal_path = args.out / f"{stem}.PAL"
            shp_path.write_bytes(shp_bytes(width, height, indices))
            pal_path.write_bytes(palette_bytes(palette))
            print(verify(shp_path, pal_path, width, height))
    return 0


if __name__ == "__main__":
    sys.exit(main())
