"""Build `src/render/fonts/Unifont-CJK-15.bin`: pixel Chinese, Japanese and
Korean characters for the Original font option, where retail GAME.FNT has none.

GAME.FNT draws its hanzi with 1-pixel strokes in a 15x15 box on a 16-pixel
advance, and points 3,897 code points (1,937 of GB2312's 6,763 simplified
hanzi among them) at one empty box. GNU Unifont draws the same kind of
1-pixel Song strokes, in 16x16. This tool takes Unifont's glyph for each CJK
character GAME.FNT lacks and trims it to 15x15: it drops the row, then the
column, whose pixels change the drawing least when merged into a neighbouring
one, so a blank edge line goes first, then a repeated line, and an inked edge
last.

Usage, from the checkout root:

    python -m tools.unifont_cjk UNIFONT_HEX GAME_FNT [OUTPUT]

`UNIFONT_HEX` is `unifont-18.0.01.hex.gz` (or the unpacked `.hex`) from
https://unifoundry.com/pub/unifont/unifont-18.0.01/font-builds/, and
`GAME_FNT` the retail file, written out by `asset extract GAME.FNT`.
`OUTPUT` defaults to `src/render/fonts/Unifont-CJK-15.bin`.

GAME.FNT lacks a character when its lookup table has no glyph for it, the
glyph is zero pixels wide, or 64 or more code points share the glyph (the
placeholder rule of `src/assets/fnt_file.rs`). Unifont's hex file also boxes
unassigned code points; those are left out by Python's Unicode database.

Output: one 32-byte record per character, in code point order: the code point
as a little-endian u16, then 15 rows of 15 pixels, each row a big-endian u16
whose top bit is the left column. `src/render/text_engine/fnt_fill.rs` reads
it.
"""

from __future__ import annotations

import argparse
import collections
import gzip
import struct
import sys
import unicodedata
from pathlib import Path

DEFAULT_OUTPUT = Path("src/render/fonts/Unifont-CJK-15.bin")

# CJK symbols and punctuation, kana, Bopomofo, compatibility jamo, Kanbun,
# strokes, enclosed and compatibility letters; the unified ideographs; Hangul
# syllables; compatibility ideographs; vertical forms; fullwidth forms. All
# draw at GAME.FNT's full-cell CJK size (`i18n::is_cjk_sized`).
BLOCKS = (
    (0x3000, 0x33FF),
    (0x4E00, 0x9FFF),
    (0xAC00, 0xD7A3),
    (0xF900, 0xFAFF),
    (0xFE30, 0xFE4F),
    (0xFF00, 0xFFEF),
)

FNT_HEADER = 4 + 6 * 4
FNT_TABLE = 65536 * 2
PLACEHOLDER_SHARE = 64


def game_fnt_lacks(data: bytes) -> set[int]:
    """The code points `data`, a GAME.FNT, draws no glyph of its own for."""
    stride = struct.unpack_from("<I", data, 4 + 5 * 4)[0]
    glyphs = data[FNT_HEADER + FNT_TABLE :]
    slots = struct.unpack_from("<65536H", data, FNT_HEADER)

    def width(slot: int) -> int:
        offset = (slot - 1) * stride
        return glyphs[offset] if 0 < slot and offset < len(glyphs) else 0

    uses = collections.Counter(slot for slot in slots if width(slot))
    return {
        cp
        for cp, slot in enumerate(slots)
        if not width(slot) or uses[slot] >= PLACEHOLDER_SHARE
    }


def read_hex(lines) -> dict[int, list[list[bool]]]:
    """Unifont's 16x16 glyphs: code point to 16 rows of 16 pixels."""
    glyphs = {}
    for line in lines:
        cp, bits = line.strip().split(":")
        if len(bits) != 64:
            continue
        rows = [int(bits[i : i + 4], 16) for i in range(0, 64, 4)]
        glyphs[int(cp, 16)] = [[row >> (15 - x) & 1 == 1 for x in range(16)] for row in rows]
    return glyphs


def drop_line(rows: list[list[bool]]) -> list[list[bool]]:
    """`rows` less one: the row whose pixels change the drawing least when
    merged into a neighbouring row. Its lit pixels move into that neighbour;
    an inked first or last row costs half a pixel more, so edges stay. Of
    rows that cost the same, a first or last row goes before an inner one:
    a blank edge changes nothing, where closing up inner rows moves strokes."""
    best = None
    for r, row in enumerate(rows):
        edge = r in (0, len(rows) - 1)
        for n in (r - 1, r + 1):
            if not 0 <= n < len(rows):
                continue
            merged = [a or b for a, b in zip(row, rows[n])]
            cost = sum(a != b for a, b in zip(merged, rows[n]))
            if edge and any(row):
                cost += 0.5
            key = (cost, not edge)
            if best is None or key < best[0]:
                best = (key, r, n, merged)
    _, r, n, merged = best
    out = list(rows)
    out[n] = merged
    del out[r]
    return out


def transpose(rows: list[list[bool]]) -> list[list[bool]]:
    return [list(column) for column in zip(*rows)]


def trim(glyph: list[list[bool]]) -> list[list[bool]]:
    """A 16x16 glyph as 15x15: one row dropped, then one column."""
    return transpose(drop_line(transpose(drop_line(glyph))))


def record(cp: int, glyph: list[list[bool]]) -> bytes:
    rows = (sum(1 << (15 - x) for x, lit in enumerate(row) if lit) for row in glyph)
    return struct.pack("<H", cp) + b"".join(struct.pack(">H", row) for row in rows)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("unifont_hex", type=Path)
    parser.add_argument("game_fnt", type=Path)
    parser.add_argument("output", type=Path, nargs="?", default=DEFAULT_OUTPUT)
    args = parser.parse_args(argv)

    opener = gzip.open if args.unifont_hex.suffix == ".gz" else open
    with opener(args.unifont_hex, "rt", encoding="ascii") as lines:
        unifont = read_hex(lines)
    lacks = game_fnt_lacks(args.game_fnt.read_bytes())
    chosen = [
        cp
        for low, high in BLOCKS
        for cp in range(low, high + 1)
        if cp in lacks and cp in unifont and unicodedata.category(chr(cp)) != "Cn"
    ]
    args.output.write_bytes(b"".join(record(cp, trim(unifont[cp])) for cp in chosen))
    print(
        f"wrote {len(chosen)} glyphs to {args.output} "
        f"(Unicode {unicodedata.unidata_version} assigned code points)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
