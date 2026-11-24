"""Original saved-list scrollbar arithmetic, sampled ordinary ranges."""
from pathlib import Path
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EAX, UC_X86_REG_FPCW, UC_X86_REG_FPTAG
from tools.native_oracle import load_image, run_checked, STACK_BASE, STACK_SIZE, finish_vectors, provenance


def generate():
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    stack = STACK_BASE + STACK_SIZE - 0x1000
    cases = []
    for height in (255, 304, 343):
        for scroll_range in range(1, 151):
            uc.reg_write(UC_X86_REG_ESP, stack)
            uc.reg_write(UC_X86_REG_EAX, scroll_range)
            uc.reg_write(UC_X86_REG_FPCW, 0x0E7F)
            uc.reg_write(UC_X86_REG_FPTAG, 0xFFFF)
            uc.mem_write(stack + 0x18, struct.pack("<i", height - 46))
            run_checked(uc, 0x0061C818, 0x0061C86D, count=150)
            thumb = struct.unpack("<i", uc.mem_read(stack + 0x1C, 4))[0]
            cases.append({"height": height, "range": scroll_range, "thumb": thumb})
    return {"source": "unicorn/gamemd.exe", "cases": cases}


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope="450 saved-list scrollbar thumb-height samples",
        assumptions=[
            "Heights 255, 304 and 343 are supplied geometry vectors, not a claim of observed runtime dimensions",
            "Ranges 1 through 150; input track height equals client height minus 2 border pixels and 44 arrow pixels",
            "x87 input control word 0x0E7F; original ftol helper runs; output minimum is applied by original instructions",
            "This does not certify unsampled x87 rounding edges or pointer/timer behavior",
        ],
        substitutions=[],
        entry_points={"thumb_arithmetic": 0x0061C818, "end": 0x0061C86D},
    ))
