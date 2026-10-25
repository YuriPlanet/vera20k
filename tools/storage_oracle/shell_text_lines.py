"""Original 434CD0 line submission with supplied font and observed glyph calls.

Shell wrapper 620F60 computes RECT width/height at 620FC5..620FDA and passes
them directly to 434CD0 at 62102E. The body submits a line before consulting
height at its newline (434EC2..434ED7) or wrap (435112..435127) tail. These
fixtures execute that complete body, including its original glyph lookup and
horizontal-position setter, but replace surface lock/unlock and glyph raster.
They establish glyph submission, not clipped raster pixels or complete layout.

The opt-in --text-label corpus reuses this same font and glyph boundary for
original434500 versus PathA434CD0 count/color controls. The default66 corpus
and its inputs remain unchanged.
"""

from pathlib import Path
import struct
import sys

from tools import native_oracle as native

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP

from tools.native_oracle import (
    RET_MAGIC, SCRATCH, SCRATCH_SIZE, STACK_BASE, STACK_SIZE,
    finish_vectors, load_image, provenance, run_checked,
)


def native_case(mode, height, line_count, *, text=None, width=None, reveal=None):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, SCRATCH_SIZE)
    uc.mem_map(RET_MAGIC, 0x1000)
    font, font_data, indices = SCRATCH, SCRATCH + 0x100, SCRATCH + 0x200
    glyph, text_address = SCRATCH + 0x400, SCRATCH + 0x500

    def put(address, value):
        uc.mem_write(address, struct.pack("<I", value))

    put(font + 4, font_data)
    put(font + 0x1C, 17)
    put(font + 0x24, 1)  # Ordinary non-sentinel text color.
    put(font_data + 0x14, 8)  # Glyph record size.
    put(font_data + 0x18, indices)
    put(font_data + 0x1C, glyph)
    uc.mem_write(indices + ord("A") * 2, struct.pack("<H", 1))
    uc.mem_write(indices + ord(" ") * 2, struct.pack("<H", 2))
    uc.mem_write(glyph, bytes([6]))  # Glyph width; font spacing remains zero.
    uc.mem_write(glyph + 8, bytes([3]))  # Ordinary separating space.
    if text is None:
        text = ("\n" if mode == "newline" else " ").join(["A"] * line_count)
    uc.mem_write(text_address, (text + "\0").encode("utf-16-le"))
    if width is None:
        width = 100 if mode == "newline" else 6
    stack = STACK_BASE + STACK_SIZE - 0x1000
    args = [font, SCRATCH + 0x800, text_address, 11, 13, width, height, 0, 0, 0]
    uc.mem_write(stack, struct.pack("<11I", RET_MAGIC, *args))
    uc.reg_write(UC_X86_REG_ESP, stack)
    submissions = []
    glyph_colors = []
    if reveal is not None:
        # Explicit RGB565 pixel-format bindings, also used by the existing
        # palette/House color fixtures. The original bodies own all blending
        # and repacking; no expected color is calculated by this fixture.
        for address, value in ((0x8A0DD0, 11), (0x8A0DD4, 3),
                               (0x8A0DD8, 0), (0x8A0DDC, 3),
                               (0x8A0DE0, 5), (0x8A0DE4, 2)):
            put(address, value)
        uc.mem_write(font + 0x24, struct.pack("<H", reveal["base_rgb565"]))
        uc.mem_write(0xA80220, bytes(reveal["highlight_rgb"]))
        if reveal["path"] == "generic":
            uc.reg_write(UC_X86_REG_ECX, font)
            uc.mem_write(stack, struct.pack("<6I", RET_MAGIC, text_address,
                                           11, 13, 0, reveal["native_count"]))
        else:
            uc.mem_write(stack + 36, struct.pack("<2I", reveal["path_a_count"], 8))

    def substitute(_uc, address, _size, _data):
        if address not in (0x4348F0, 0x434990, 0x434120):
            return
        sp = uc.reg_read(UC_X86_REG_ESP)
        ret = struct.unpack("<I", uc.mem_read(sp, 4))[0]
        if address == 0x434120:
            char, x, y, _color = struct.unpack("<4I", uc.mem_read(sp + 4, 16))
            if char & 0xFFFF != ord("A"):
                raise RuntimeError(f"Unexpected submitted character: {char:#x}")
            submissions.append({"x": x, "y": y})
            if reveal is not None:
                # Generic434500 changes font+24 and passes the -1 sentinel;
                # PathA434CD0 passes the explicit glyph color. Observe each
                # original choice before returning at the shared raster seam.
                color = (struct.unpack("<H", uc.mem_read(font + 0x24, 2))[0]
                         if _color == 0xFFFFFFFF else _color & 0xFFFF)
                glyph_colors.append(color)
            uc.reg_write(UC_X86_REG_EAX, x + 6)
            argument_bytes = 16
        else:
            # Surface setup/teardown have no layout return value in this body.
            uc.reg_write(UC_X86_REG_EAX, 0)
            argument_bytes = 4
        uc.reg_write(UC_X86_REG_ESP, sp + 4 + argument_bytes)
        uc.reg_write(UC_X86_REG_EIP, ret)

    hook = uc.hook_add(UC_HOOK_CODE, substitute)
    required = [0x4346C0, 0x434110]
    if mode == "hard_cut":
        if len(text) * 6 > width:
            # No-space overflow with more than one measured glyph takes the
            # original scan-pointer-minus-one-code-unit path. A one-glyph-wide
            # first cut may submit no glyph, so do not require its raster call.
            required.extend([0x434F6F, 0x434F75, 0x434F78, 0x435112])
        required.append(0x4352B5)
    elif mode == "word_suffix":
        # Original saved-space rewind and skip/restart must actually execute.
        required.extend([0x434F60, 0x434F64, 0x435103, 0x43512D,
                         0x435112, 0x4350DC])
    elif line_count > 1:
        required.append(0x434EC2 if mode == "newline" else 0x435112)
        required.append(0x434EA1 if mode == "newline" else 0x4350DC)
    else:
        required.append(0x4352B5)
    begin = 0x434CD0
    if reveal is not None:
        begin = 0x434500 if reveal["path"] == "generic" else 0x434CD0
        required = [begin, 0x434120]
        if 0 < reveal["native_count"] < len(text.encode("utf-16-le")) // 2 + 8:
            required.append(0x6612C0)
    try:
        run_checked(uc, begin, RET_MAGIC, count=5000,
                    required_addresses=required)
    finally:
        uc.hook_del(hook)
    if reveal is not None:
        assert struct.unpack("<H", uc.mem_read(font + 0x24, 2))[0] == reveal["base_rgb565"]
        return glyph_colors
    return {"mode": mode, "cell_height": 17, "height": height,
            "line_count": line_count, "width": width, "text": text,
            "submissions": submissions, "result": uc.reg_read(UC_X86_REG_EAX) & 0xFF}


def generate():
    cases = [
        native_case(mode, height, lines)
        for mode in ("newline", "wrap")
        for height in (0, 1, 16, 17, 18, 34)
        for lines in range(1, 5)
    ]
    cases.extend(native_case("word_suffix", height, 0, text="AA AAAA", width=width)
                 for width in (18, 24, 30) for height in (0, 34))
    cases.extend(native_case("hard_cut", 0, 0, text=text, width=width)
                 for text in ("AA", "AAA", "AAAA", "AAAAAA")
                 for width in (6, 12, 18))
    return {"source": "unicorn/gamemd.exe", "cases": cases}


def text_label_controls():
    controls = []
    for base, highlight in ((0xF800, [255, 255, 255]), (0xFFFF, [0, 0, 0])):
        for count in (0, 1, 2, 8, 9, 12, 13, 20):
            row = dict(base_rgb565=base, highlight_rgb=highlight,
                       native_count=count, path_a_count=0 if count == 0 else count + 1,
                       text="A" * 12)
            for path in ("generic", "path_a"):
                row[path + "_rgb565"] = native_case(
                    "newline", 100, 1, text=row["text"], width=100,
                    reveal=dict(row, path=path))
            assert row["generic_rgb565"] == row["path_a_rgb565"]
            controls.append(row)
    return dict(schema_version=1, kind="bounded-original-text-label-path-mapping",
                controls=controls)


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    if "--text-label" in argv:
        argv.remove("--text-label")
        finish_vectors(text_label_controls, Path(__file__).with_name("shell_text_label.json"),
                       provenance=lambda: {}, argv=argv,
                       source_paths={"producer": Path(__file__),
                                     "native_runner": Path(native.__file__)})
        return
    finish_vectors(generate, Path(__file__).with_suffix(".json"), argv=argv,
                   provenance=lambda: provenance(
        scope="66 original 434CD0 fixtures: 48 line-admission, six saved-space word-suffix rewinds and 12 no-space hard cuts",
        assumptions=[
            "Supplied font cell height17, A glyph width6, space width3, spacing0, color1; original 4346C0 glyph lookup and 434110 x-position setter execute",
            "One through four A lines, either explicit LF with width100 or space-delimited words wrapping at width6; origin11,13",
            "Six additional AA AAAA word-suffix cases use widths18/24/30 and heights0/34; line_count0 means input line count is unspecified, and submissions are the native result",
            "Twelve no-space cases use AA/AAA/AAAA/AAAAAA, widths6/12/18 and height0; hard-cut instructions are required only when supplied text exceeds width",
            "Height0 is unbounded; ordinary small control heights1/16/17/18/34; alignment and reveal inputs zero",
            "Complete 434CD0 body executes from entry to RET0x28, with fresh emulator and explicit required lookup/setter/raster and newline/wrap tail visits",
            "Captured glyph-call positions establish line admission only, not per-pixel scissor, raster appearance, actual font data or general word wrapping",
        ],
        substitutions=[
            "4348F0 surface lock/setup and 434990 unlock/cleanup return without a real surface; layout fields are supplied separately",
            "434120 glyph raster is observed as character/x/y arguments and returns x+6; no raster pixels are produced",
        ],
        entry_points={"draw_text": 0x434CD0, "shell_caller": 0x62102E,
                      "newline_height": 0x434EC2, "wrap_height": 0x435112},
    ))


if __name__ == "__main__":
    main()
