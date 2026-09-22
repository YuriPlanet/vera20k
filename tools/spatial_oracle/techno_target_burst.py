"""Original Unit target setter and its Event/EnterIdle call boundaries.

Every body and vtable is original. Supplied receiver state has no linked beam,
spawn manager or animation, and targets are ordinary cells. Event and idle
cases enter at already-admitted virtual-call boundaries, not full order/idle
admission. The corpus covers retained burst/passive state, not the whole setter.
"""
import hashlib
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import (
    UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDI,
    UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import (
    RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors,
    load_image, provenance, run_checked,
)
from tools.spatial_oracle.map_queries import dwords

OWNER, CELL_A, CELL_B = SCRATCH, SCRATCH + 0x1000, SCRATCH + 0x2000
TARGETS = (0, CELL_A, CELL_B)
UNIT_VTABLE, CELL_VTABLE = 0x7F5C70, 0x7E4EEC
SP = STACK_BASE + STACK_SIZE - 0x1000
ENTRIES = {
    'setter': (0x6FCDB0, RET_MAGIC),
    'event': (0x4C7462, 0x4C746D),
    'idle_guard': (0x738AEA, 0x738AFB),
    'idle_harvest': (0x738C6F, 0x738C7B),
    'sticky': (0x4D572A, 0x4D5736),
}
RANGES = ((0x6FCDB0, 0x6FCF95), (0x4C7462, 0x4C746D),
          (0x738AEA, 0x738AFB), (0x738C6F, 0x738C7B),
          (0x746CC0, 0x746CC5), (0x4D572A, 0x4D5736))


class Fixture:
    def __init__(self):
        self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(self.u)
        self.u.mem_map(SCRATCH, 0x4000)
        self.u.mem_map(STACK_BASE, STACK_SIZE)
        self.u.mem_map(RET_MAGIC, 0x1000)
        self.original = [bytes(self.u.mem_read(a, b-a)) for a, b in RANGES]
        assert self.i32(UNIT_VTABLE + 0x3C8) == ENTRIES['setter'][0]

    def i32(self, address):
        return struct.unpack('<i', self.u.mem_read(address, 4))[0]

    def execute(self, entry, previous, requested, index, passive):
        u = self.u
        u.mem_write(SCRATCH, bytes(0x4000))
        u.mem_write(OWNER, dwords(UNIT_VTABLE))
        for cell in (CELL_A, CELL_B):
            u.mem_write(cell, dwords(CELL_VTABLE))
        u.mem_write(OWNER + 0x2B4, dwords(TARGETS[previous]))
        u.mem_write(OWNER + 0x3B8, dwords(index))
        u.mem_write(OWNER + 0x50C, bytes([passive]))
        u.reg_write(UC_X86_REG_ECX, OWNER)
        u.reg_write(UC_X86_REG_ESI, OWNER)
        u.reg_write(UC_X86_REG_EDI, OWNER)
        u.reg_write(UC_X86_REG_EBX, OWNER if entry == 'sticky' else TARGETS[requested])
        u.reg_write(UC_X86_REG_ESP, SP)
        u.mem_write(SP, dwords(RET_MAGIC, TARGETS[requested]))
        begin, end = ENTRIES[entry]
        run_checked(u, begin, end, count=500, required_addresses=(0x6FCDC4,))
        assert u.reg_read(UC_X86_REG_ESP) == SP + (8 if entry == 'setter' else 0)
        assert self.original == [bytes(u.mem_read(a, b-a)) for a, b in RANGES]
        return dict(entry=entry, previous=previous, requested=requested,
                    index=index, passive=passive,
                    target=TARGETS.index(self.i32(OWNER + 0x2B4)),
                    next_index=self.i32(OWNER + 0x3B8),
                    next_passive=int.from_bytes(u.mem_read(OWNER + 0x50C, 1), 'little'))


def generate():
    f = Fixture()
    return [f.execute(entry, previous, requested, index, passive)
            for entry in ENTRIES
            for previous in (0, 1)
            for requested in ((0, 1, 2) if entry in ('setter', 'event') else (0,))
            for index in (0, 1, 257)
            for passive in (0, 1)]


def metadata():
    f = Fixture()
    out = provenance(
        scope='108 original Unit target/burst/passive-state setter and caller-boundary cases',
        entry_points={key + suffix: value for key, points in ENTRIES.items()
                      for suffix, value in zip(('_begin', '_end_exclusive'), points)},
        assumptions=[
            'Original Unit and Cell vtables; no linked beam, spawn manager or animation',
            'Cells supplied as ordinary targets; no dead/cloaked/self/bunker redirection cases',
            'Event, EnterIdle and Sticky admission already completed; only their virtual setter call executes',
            'No Infantry override, whole order admission, whole mission or full combat parity claim',
        ], substitutions=[])
    out['original_code'] = [dict(begin=f'{a:08X}', end_exclusive=f'{b:08X}',
                                  sha256=hashlib.sha256(code).hexdigest())
                            for (a, b), code in zip(RANGES, f.original)]
    return out


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
