"""The level height CaptureUnit multiplies a captured building's Height= by.

CaptureManagerClass::CaptureUnit places a building's ring at GetCoords plus
`BuildingType+0xEF4 (Height=) * [0x0089E178]` (0x00471EC3..0x00471ED8). That
dword is set at startup by this file's static initializer 0x00471610, which
reads doubles the neighbouring initializers compute (atan2, tan and the
original CRT helpers). This runs the original initializers in address order
and records the result, with its neighbour [0x0089E16C] as a cross-check.

Rust consumer: src/sim/capture_manager_tests.rs.
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_FPCW

from tools.native_oracle import (load_image, run_checked, STACK_BASE, STACK_SIZE, RET_MAGIC,
                                 NATIVE_FPCW, finish_vectors, provenance)
from tools.spatial_oracle.map_queries import dwords

SP = STACK_BASE + STACK_SIZE - 0x1000
# 0x00471540 [89E120], 0x00471570 [89E170], 0x00471590 [89E158],
# 0x004715B0 [89E150], 0x004715D0 [89E128], 0x004715F0 [89E0E8],
# 0x00471610 [89E178], 0x004716B0 [89E16C].
INITIALIZERS = (0x471540, 0x471570, 0x471590, 0x4715B0, 0x4715D0, 0x4715F0, 0x471610, 0x4716B0)


def generate():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)
    u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    for address in (0x89E178, 0x89E16C):
        u.mem_write(address, dwords(0x5A5A5A5A))
    for initializer in INITIALIZERS:
        u.mem_write(SP, dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, initializer, RET_MAGIC, count=200000)
    read = lambda address: struct.unpack("<i", u.mem_read(address, 4))[0]
    return dict(level_height=read(0x89E178), bridge_height=read(0x89E16C))


def metadata():
    return provenance(
        scope="Original static initializers 0x00471540..0x004716B0 of the CaptureManager file; [0x0089E178] and [0x0089E16C]",
        assumptions=["Initializers run in address order, as the CRT table lists this file's; each reads only rdata or earlier results"],
        substitutions=[],
        entry_points={"level_height": 0x471610, "bridge_height": 0x4716B0})


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
