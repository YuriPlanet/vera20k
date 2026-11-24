"""Original FireAt burst increment/GetROF boundary/signed remainder.

GetROF is a supplied callback returning 20 and observing the already incremented
object dword. This does not certify rearm math, shot effects or whole FireAt.
"""
import hashlib
import struct
from pathlib import Path
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ESI, UC_X86_REG_EBX, UC_X86_REG_EBP, UC_X86_REG_ESP
from tools.native_oracle import load_image, run_checked, SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords

OWNER, WEAPON, VTABLE, CALLBACK = [SCRATCH + n * 0x1000 for n in range(4)]
START, END = 0x6FF274, 0x6FF2D1


class Fixture:
    def __init__(self):
        self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(self.u)
        self.u.mem_map(SCRATCH, 0x5000)
        self.u.mem_map(STACK_BASE, STACK_SIZE)
        self.original = bytes(self.u.mem_read(START, END - START))
        self.seen = []
        self.u.hook_add(UC_HOOK_CODE, self.observe)

    def i32(self, address):
        return struct.unpack('<i', self.u.mem_read(address, 4))[0]

    def observe(self, u, pc, size, data):
        if pc == CALLBACK:
            self.seen.append(self.i32(OWNER + 0x3B8))

    def execute(self, index, burst):
        u = self.u
        u.mem_write(SCRATCH, bytes(0x5000))
        u.mem_write(OWNER, dwords(VTABLE))
        u.mem_write(OWNER + 0x3B8, dwords(index))
        u.mem_write(WEAPON + 0x9C, dwords(burst))
        u.mem_write(VTABLE + 0x318, dwords(CALLBACK))
        u.mem_write(CALLBACK, b'\xB8' + dwords(20) + b'\xC2\x04\x00')
        sp = STACK_BASE + STACK_SIZE - 0x1000
        u.reg_write(UC_X86_REG_ESP, sp)
        u.reg_write(UC_X86_REG_EBP, sp + 0x100)
        u.reg_write(UC_X86_REG_ESI, OWNER)
        u.reg_write(UC_X86_REG_EBX, WEAPON)
        u.mem_write(sp + 0x10C, dwords(0))
        self.seen = []
        run_checked(u, START, END, count=100)
        assert self.original == bytes(u.mem_read(START, END - START))
        assert u.reg_read(UC_X86_REG_ESP) == sp
        return dict(index=index, burst=burst, get_rof_indices=self.seen,
                    next_index=self.i32(OWNER + 0x3B8))


def generate():
    f = Fixture()
    return [f.execute(index, burst)
            for index in (-2147483648, -2, -1, 0, 1, 4, 255, 256, 2147483647)
            for burst in (-2, 1, 2, 5, 256, 257)]


def metadata():
    f = Fixture()
    out = provenance(scope='54 original signed object burst increment/remainder cases',
        entry_points={'start': START, 'end_exclusive': END},
        assumptions=['Entry at normal FireAt rearm tail after shot effects; raw retained index and nonzero Burst supplied',
                     'No initialization, target assignment, spawn manager, FLH or complete firing parity claim'],
        substitutions=['Scratch GetROF returns20 and records the incremented index; original code bytes unchanged'])
    out['original_slice'] = dict(start=f'{START:08X}', end_exclusive=f'{END:08X}',
        hex=f.original.hex(), sha256=hashlib.sha256(f.original).hexdigest())
    return out


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
