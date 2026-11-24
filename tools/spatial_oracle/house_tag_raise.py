"""Original HouseClass::Update Tag raise 0x004F8F87..0x004F8FBE.

Each case runs the block once for one house, entered with ESI = house as
HouseClass::Update holds it there. The block reads the frame [0xA8ED84], the
House Tag count +0x48 once and the list +0x3C for each slot, and passes the
Cell at [0xA8EF98] (zeroed by its static initializer) to
TagClass::ProcessTriggerEvent 0x006E53A0. That call is recorded with its five
arguments and returned without running.

Rust consumer: src/sim/trigger_runtime_tests.rs.
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_ESI

from tools.native_oracle import (load_image, run_checked, SCRATCH, STACK_BASE, STACK_SIZE,
                                 RET_MAGIC, finish_vectors, provenance)
from tools.spatial_oracle.map_queries import dwords

REGION = 0x10000
SP = STACK_BASE + STACK_SIZE - 0x1000
RAISE_BEGIN, RAISE_END = 0x4F8F87, 0x4F8FBE
PROCESS_TRIGGER_EVENT = 0x6E53A0
FRAME, RAISE_CELL = 0xA8ED84, 0xA8EF98
HOUSE = SCRATCH
TAGS = SCRATCH + 0x1000
TAG_BASE = 0x7000000  # recorded Tag pointer n is TAG_BASE + n


class Fixture:
    def __init__(self):
        self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(self.u)
        self.u.mem_map(SCRATCH, REGION)
        self.u.mem_map(STACK_BASE, STACK_SIZE)
        self.u.mem_map(RET_MAGIC, 0x1000)
        self.u.hook_add(UC_HOOK_CODE, self.observe)

    def word(self, address):
        return struct.unpack("<I", self.u.mem_read(address, 4))[0]

    def observe(self, u, pc, _size, _data):
        if pc != PROCESS_TRIGGER_EVENT:
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        event, obj, cell, force, source = struct.unpack("<iIIiI", u.mem_read(sp + 4, 20))
        self.raises.append(dict(tag=u.reg_read(UC_X86_REG_ECX) - TAG_BASE, event=event,
                                object=obj, cell=[cell & 0xFFFF, cell >> 16],
                                force=force, source=source))
        u.reg_write(UC_X86_REG_EIP, self.word(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + 20)

    def execute(self, case):
        u = self.u
        self.raises = []
        u.mem_write(SCRATCH, bytes(REGION))
        u.mem_write(FRAME, struct.pack("<i", case["frame"]))
        u.mem_write(RAISE_CELL, dwords(0))
        count = case["tags"]
        u.mem_write(HOUSE + 0x3C, dwords(TAGS))
        u.mem_write(HOUSE + 0x48, struct.pack("<i", count))
        for slot in range(max(count, 0)):
            u.mem_write(TAGS + 4 * slot, dwords(TAG_BASE + slot))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ESI, HOUSE)
        run_checked(u, RAISE_BEGIN, RAISE_END, count=10_000)
        return dict(input=case, raises=self.raises)


def inputs():
    frames = [0, 1, 7, 8, 9, 15, 16, 17, 24, -1, -7, -8, -9, -16, 0x7FFFFFF8, -0x80000000]
    cases = [dict(frame=frame, tags=3) for frame in frames]
    cases += [dict(frame=8, tags=0), dict(frame=8, tags=1), dict(frame=8, tags=-1)]
    return cases


def generate():
    fixture = Fixture()
    return [fixture.execute(case) for case in inputs()]


def metadata():
    return provenance(
        scope="Original HouseClass::Update Tag raise 0x004F8F87..0x004F8FBE",
        assumptions=[
            "Frame [0xA8ED84] signed; House +0x3C Tag list items, +0x48 count; raise Cell [0xA8EF98] zero",
            "The list is not changed between raises: ProcessTriggerEvent is not run",
        ],
        substitutions=[
            "Recorded at entry and returned (ret 0x14) without running: TagClass::ProcessTriggerEvent 0x006E53A0",
        ],
        entry_points={"raise": RAISE_BEGIN, "join": RAISE_END})


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=metadata)
