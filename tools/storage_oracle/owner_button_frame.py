"""Original type-three owner-button frame selection, with supplied paint state.

Execute 612F2A..612F5F from the retail gamemd.exe, after type-three admission
and the 72B050 art lookup. The snippet selects a frame and stores it at ESP+14.
This does not emulate mouse capture, art lookup, painting or disabled tinting.
"""

from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_ESP

from tools.native_oracle import (
    SCRATCH, SCRATCH_SIZE, STACK_BASE, STACK_SIZE, finish_vectors, load_image,
    provenance, run_checked,
)


def generate():
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, SCRATCH_SIZE)
    stack = STACK_BASE + STACK_SIZE - 0x1000
    registers = uc.context_save()
    cases = []
    for pressed in (False, True):
        for highlighted in (False, True):
            uc.context_restore(registers)
            uc.mem_write(stack, bytes(0x100))
            uc.mem_write(SCRATCH, bytes(0x100))
            uc.reg_write(UC_X86_REG_ESP, stack)
            uc.reg_write(UC_X86_REG_EBP, SCRATCH)
            uc.reg_write(UC_X86_REG_EAX, SCRATCH + 0x1000)
            uc.mem_write(0xB0FACC, struct.pack("<I", SCRATCH + 0x2000))
            uc.mem_write(stack + 0x2C, bytes([int(pressed)]))
            uc.mem_write(SCRATCH + 0xC5, bytes([int(highlighted)]))
            run_checked(uc, 0x612F2A, 0x612F5F, count=30,
                        required_addresses=[0x612F38, 0x612F5B])
            frame = struct.unpack("<I", uc.mem_read(stack + 0x14, 4))[0]
            cases.append({"pressed": pressed, "highlighted": highlighted,
                          "frame": frame})
    return {"source": "unicorn/gamemd.exe", "cases": cases}


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
                   provenance=lambda: provenance(
        scope="Four supplied type-three button pressed/highlight combinations; original frame-selection instructions only",
        assumptions=[
            "Type-three admission at 612F20 and the preceding 72B050 art lookup are bypassed",
            "ESP+2C contains the supplied pressed bit (bit zero); EBP+C5 contains the supplied highlight byte",
            "ESP+14 is the original selected-frame output; execution stops before the disabled-style handling at 612F5F",
            "No claim about event admission, mouse capture, other button types, art availability, tinting or GUI painting",
        ],
        substitutions=[
            "Writable stack and control record supplied; EAX and B0FACC contain opaque scratch pointers copied by this snippet but never dereferenced",
        ],
        entry_points={"type_three_frame_selection": 0x612F2A,
                      "selection_end_exclusive": 0x612F5F},
    ))
