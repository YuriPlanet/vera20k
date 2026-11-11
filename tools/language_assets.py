"""Build a translation's own art for VERA20k from a language pack.

    python -m tools.language_assets cameos RETAIL_DIR TRANSLATED_DIR PALETTE OUTPUT
        [--band NAME=ROW ...] [--source NAME=OTHER ...]
    python -m tools.language_assets letters RETAIL_FNT TRANSLATED_FNT OUTPUT

`cameos` writes `vera20k.<tag>.cameos` (format: `src/i18n/cameo_labels.rs`).
`RETAIL_DIR` and `TRANSLATED_DIR` hold the 60x48 cameo SHPs, written out by
`asset extract <name>`, and `PALETTE` is the retail `cameo.pal`. A label is only
the rows from the top of the cameo's name band down, XORed with the retail
rows, so the file holds no game art and a re-quantised picture above the band
stays the retail one. The top of the band is found from the colours: a row
belongs to it when `STRONG_PIXELS` of its pixels moved at least `STRONG_DISTANCE`
in a colour channel, walking up from the bottom. `--band gtnkicon.shp=36` sets it
by hand for a cameo whose picture moved as far as its name, and
`--source minduico.shp=mindicon.shp` takes a cameo's band from another
translated file when its own is damaged.

`letters` writes `src/i18n/vera20k.vi.letters`, the Vietnamese letters retail
GAME.FNT lacks as the translator drew them: per letter its code point as a
little-endian u16, then 16 rows of big-endian u16 whose top bit is the left
column (`src/render/text_engine/fnt_fill.rs`). `RETAIL_FNT` and `TRANSLATED_FNT`
come from `asset extract GAME.FNT` with each install's files. Ink on a
translated font's 17th row moves up one row, onto GAME.FNT's last: only the
dot under `ỵ` reaches it.
"""

import argparse
import pathlib
import struct
import zlib

CAMEO_WIDTH = 60
CAMEO_HEIGHT = 48
MAGIC = b"VCL1"
STRONG_DISTANCE = 48
STRONG_PIXELS = 6
# Names sit in the lower half of a cameo: higher rows are never its band.
FIRST_BAND_ROW = 20
# Marks on the top text line can rise past the rows that clear the threshold.
ROWS_ABOVE_BAND = 2

FONT_HEADER = 4 + 6 * 4
FONT_TABLE = 65536 * 2
FONT_ROWS = 16
PLACEHOLDER_SHARE = 64
VIETNAMESE_LETTERS = (
    "ÀÁÂÃÈÉÊÌÍÒÓÔÕÙÚÝàáâãèéêìíòóôõùúýĂăĐđĨĩŨũƠơƯư"
    + "".join(chr(code) for code in range(0x1EA0, 0x1EFA))
)


def cameo_pixels(data):
    """The palette indices of an uncompressed one-frame 60x48 cameo SHP, or None."""
    zero, width, height, count = struct.unpack_from("<HHHH", data, 0)
    if (zero, width, height, count) != (0, CAMEO_WIDTH, CAMEO_HEIGHT, 1):
        return None
    x, y, frame_width, frame_height, compression = struct.unpack_from("<HHHHB", data, 8)
    offset = struct.unpack_from("<I", data, 8 + 20)[0]
    if (x, y, frame_width, frame_height) != (0, 0, CAMEO_WIDTH, CAMEO_HEIGHT) or compression > 1:
        return None
    pixels = data[offset : offset + CAMEO_WIDTH * CAMEO_HEIGHT]
    return pixels if len(pixels) == CAMEO_WIDTH * CAMEO_HEIGHT else None


def read_palette(data):
    return [tuple(min(255, value * 4) for value in data[i * 3 : i * 3 + 3]) for i in range(256)]


def band_top(retail, translated, palette):
    """The first row of the name band; `CAMEO_HEIGHT` when there is none."""
    strong = []
    for y in range(CAMEO_HEIGHT):
        moved = 0
        for x in range(CAMEO_WIDTH):
            a = palette[retail[y * CAMEO_WIDTH + x]]
            b = palette[translated[y * CAMEO_WIDTH + x]]
            if max(abs(a[i] - b[i]) for i in range(3)) >= STRONG_DISTANCE:
                moved += 1
        strong.append(moved)
    top = CAMEO_HEIGHT
    quiet = 0
    for y in range(CAMEO_HEIGHT - 1, FIRST_BAND_ROW - 1, -1):
        if strong[y] >= STRONG_PIXELS:
            top = y
            quiet = 0
        else:
            quiet += 1
            if quiet >= (2 if top != CAMEO_HEIGHT else 4):
                break
    return top


def label_record(name, first_row, retail, translated):
    """A pixel transparent in retail stays so, which keeps the rounded corners
    a re-quantised cameo paints over."""
    start = first_row * CAMEO_WIDTH
    band = retail[start:]
    return (
        bytes([len(name)])
        + name.lower().encode("ascii")
        + bytes([first_row])
        + struct.pack("<I", zlib.crc32(band))
        + bytes(a ^ b if a else 0 for a, b in zip(band, translated[start:]))
    )


def build_cameos(retail_dir, translated_dir, palette, bands, sources):
    """The label file's bytes, and the names it covers."""
    retail_files = {path.name.lower(): path for path in pathlib.Path(retail_dir).iterdir()}
    translated_files = {path.name.lower(): path for path in pathlib.Path(translated_dir).iterdir()}
    file = bytearray(MAGIC)
    covered = []
    for name in sorted(translated_files):
        if name not in retail_files or not name.endswith(".shp"):
            continue
        retail = cameo_pixels(retail_files[name].read_bytes())
        translated = cameo_pixels(translated_files[sources.get(name, name)].read_bytes())
        if retail is None or translated is None or retail == translated:
            continue
        if name in bands:
            top = bands[name]
        else:
            top = band_top(retail, translated, palette)
            if top == CAMEO_HEIGHT:
                continue
            top = max(top - ROWS_ABOVE_BAND, FIRST_BAND_ROW)
        file += label_record(name, top, retail, translated)
        covered.append((name, top))
    return bytes(file), covered


class Font:
    """A `.fnt` file's glyphs (`src/assets/fnt_file.rs`)."""

    def __init__(self, data):
        self.data = data
        self.bytes_per_row, self.rows, _, _, self.stride = struct.unpack_from("<5I", data, 8)
        self.table = struct.unpack_from("<65536H", data, FONT_HEADER)
        self.share = {}
        for slot in self.table:
            self.share[slot] = self.share.get(slot, 0) + 1

    def rows_of(self, code):
        """Each row of `code`'s glyph as a u16 with its top bit the left column,
        or None when the font has no drawing of it."""
        slot = self.table[code]
        if slot == 0 or self.share[slot] >= PLACEHOLDER_SHARE:
            return None
        at = FONT_HEADER + FONT_TABLE + (slot - 1) * self.stride
        if self.data[at] == 0:
            return None
        bits = self.data[at + 1 : at + self.stride]
        rows = [
            (int.from_bytes(bits[r * self.bytes_per_row : (r + 1) * self.bytes_per_row], "big")
             << 16 >> (8 * self.bytes_per_row)) & 0xFFFF
            for r in range(self.rows)
        ]
        return rows if any(rows) else None


def fit_rows(rows):
    """`rows` on GAME.FNT's 16 rows: ink below them moves up onto the last."""
    fitted = rows[:FONT_ROWS]
    for row in rows[FONT_ROWS:]:
        fitted[-1] |= row
    return fitted


def build_letters(retail_font, translated_font):
    file = bytearray()
    for letter in VIETNAMESE_LETTERS:
        code = ord(letter)
        if retail_font.rows_of(code) is not None:
            continue
        rows = translated_font.rows_of(code)
        if rows is None:
            raise SystemExit(f"the translated font has no {letter}")
        file += struct.pack("<H", code) + b"".join(
            struct.pack(">H", row) for row in fit_rows(rows)
        )
    return bytes(file)


def named_pairs(pairs, value=str):
    return {key.lower(): value(item) for key, item in (pair.split("=", 1) for pair in pairs)}


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    commands = parser.add_subparsers(dest="command", required=True)
    cameos = commands.add_parser("cameos")
    cameos.add_argument("retail_dir")
    cameos.add_argument("translated_dir")
    cameos.add_argument("palette")
    cameos.add_argument("output")
    cameos.add_argument("--band", action="append", default=[])
    cameos.add_argument("--source", action="append", default=[])
    letters = commands.add_parser("letters")
    letters.add_argument("retail_fnt")
    letters.add_argument("translated_fnt")
    letters.add_argument("output")
    args = parser.parse_args()
    if args.command == "cameos":
        file, covered = build_cameos(
            args.retail_dir,
            args.translated_dir,
            read_palette(pathlib.Path(args.palette).read_bytes()),
            named_pairs(args.band, int),
            named_pairs(args.source),
        )
        pathlib.Path(args.output).write_bytes(file)
        print(f"{len(covered)} cameos, {len(file)} bytes")
    else:
        file = build_letters(
            Font(pathlib.Path(args.retail_fnt).read_bytes()),
            Font(pathlib.Path(args.translated_fnt).read_bytes()),
        )
        pathlib.Path(args.output).write_bytes(file)
        print(f"{len(file) // 34} letters, {len(file)} bytes")


if __name__ == "__main__":
    main()
