"""Executed draw lists of the shell bevel `0x006208F0` for trackbar frames.

`OwnerDraw_Trackbar_0061D950` frames a trackbar with two calls to
`0x006208F0` (`0x0061E204`, `0x0061E269`): the rail box, then a value box past
it whose inset is 1 + (value plaque on). This fixture runs the original bevel
with a recording surface (Draw_Line `+0x30`, Put_Pixel `+0x24`) in RGB565 and
records every line and pixel it issues, in order, for the boxes of a
plaque-less trackbar (campaign `0x94` difficulty, 272x22, whose second box is
one pixel wide negative) and a plaque trackbar (128x21, reserve 50).

Boxes are control-relative; colours are RGB565 values of the runtime shell
colours `0x00C5BEA7` / `0x00807A68` (`0x0060FA81..0x0060FA9B`).
"""

from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP

from tools.native_oracle import (
    RET_MAGIC, SCRATCH, SCRATCH_SIZE, STACK_BASE, STACK_SIZE,
    finish_vectors, load_image, provenance, run_checked,
)

SURFACE = SCRATCH + 0x100
VTABLE = SCRATCH + 0x200
DRAW_LINE = SCRATCH + 0x800
PUT_PIXEL = SCRATCH + 0x810
BOX = SCRATCH + 0x900

# (label, control width, control height, plaque reserve, plaque on)
TRACKBARS = (
    ("campaign-difficulty", 272, 22, 0, False),
    ("plaque-128", 128, 21, 50, True),
)


def boxes(width, height, reserve, plaque):
    """The two boxes `0x0061E1B9..0x0061E262` pass, control-relative."""
    inset = 2 if plaque else 1
    return [
        (0, 0, width - reserve, height),
        (width - reserve + inset, 0, reserve - inset, height),
    ]


def _i32(uc, address):
    return struct.unpack("<i", uc.mem_read(address, 4))[0]


def run_bevel(box, border=2):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, SCRATCH_SIZE)
    uc.mem_map(RET_MAGIC, 0x1000)
    # RGB565 pixel format: shift/loss pairs for R, G and B.
    for address, value in ((0x8A0DD0, 11), (0x8A0DD4, 3), (0x8A0DE0, 5),
                           (0x8A0DE4, 2), (0x8A0DD8, 0), (0x8A0DDC, 3)):
        uc.mem_write(address, struct.pack("<I", value))
    # Runtime shell colours written by 0x0060F9A0 (0x0060FA81..0x0060FA9B).
    uc.mem_write(0xAC1B98, struct.pack("<I", 0x00C5BEA7))
    uc.mem_write(0xAC1B94, struct.pack("<I", 0x00807A68))
    uc.mem_write(0xAC4624, struct.pack("<I", 0xFF))
    uc.mem_write(SURFACE, struct.pack("<I", VTABLE))
    uc.mem_write(VTABLE + 0x30, struct.pack("<I", DRAW_LINE))
    uc.mem_write(VTABLE + 0x24, struct.pack("<I", PUT_PIXEL))
    uc.mem_write(BOX, struct.pack("<4i", *box))
    sp = STACK_BASE + STACK_SIZE - 0x1000
    for value in (0xFFFFFFFF, border):
        sp -= 4
        uc.mem_write(sp, struct.pack("<I", value & 0xFFFFFFFF))
    sp -= 4
    uc.mem_write(sp, struct.pack("<I", RET_MAGIC))
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_ECX, SURFACE)
    uc.reg_write(UC_X86_REG_EDX, BOX)
    ops = []

    def hook(u, address, _size, _data):
        if address not in (DRAW_LINE, PUT_PIXEL):
            return
        esp = u.reg_read(UC_X86_REG_ESP)
        ret = struct.unpack("<I", u.mem_read(esp, 4))[0]
        args = [struct.unpack("<I", u.mem_read(esp + 4 * i, 4))[0] for i in (1, 2, 3)]
        if address == DRAW_LINE:
            p1, p2, color = args
            ops.append(["line", _i32(u, p1), _i32(u, p1 + 4), _i32(u, p2), _i32(u, p2 + 4), color])
            count = 3
        else:
            point, color = args[:2]
            ops.append(["pixel", _i32(u, point), _i32(u, point + 4), color])
            count = 2
        u.reg_write(UC_X86_REG_EIP, ret)
        u.reg_write(UC_X86_REG_ESP, esp + 4 * (1 + count))

    uc.hook_add(UC_HOOK_CODE, hook)
    run_checked(uc, 0x006208F0, RET_MAGIC, count=100000)
    return ops


def generate():
    cases = []
    for label, width, height, reserve, plaque in TRACKBARS:
        cases.append({
            "trackbar": label, "width": width, "height": height,
            "reserve": reserve, "plaque": plaque,
            "boxes": [{"box": list(box), "ops": run_bevel(box)}
                      for box in boxes(width, height, reserve, plaque)],
        })
    return {"source": "unicorn/gamemd.exe", "cases": cases}


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
                   provenance=lambda: provenance(
        scope="Original shell bevel 0x006208F0 (border 2) for the two trackbar frame boxes of 0x0061D950, plaque-less 272x22 and plaque 128x21",
        assumptions=[
            "Box inset 1 + (plaque on) from 0x0061E22A..0x0061E235; boxes are control-relative",
            "Runtime colours 0x00C5BEA7 / 0x00807A68 as written by 0x0060FA81..0x0060FA9B",
        ],
        substitutions=[
            "Surface Draw_Line (+0x30) and Put_Pixel (+0x24) are recorded and return",
            "Pixel-format globals set to RGB565",
        ],
        entry_points={"bevel": 0x6208F0},
    ))
