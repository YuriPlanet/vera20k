"""Original Infantry GetFireError speed refusal after common legality passes.

Executes the comparison at 51C9B8 and its error-7 return. The allowed boundary
precedes the separate NavCom/action gate. This is not full fire legality.
"""
from pathlib import Path
import math
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import (
    NATIVE_FPCW, RET_MAGIC, SCRATCH, SCRATCH_SIZE, STACK_BASE, STACK_SIZE,
    finish_vectors, load_image, provenance, run_checked,
)

ENTRY, ALLOWED = 0x51C9B8, 0x51C9CF
OWNER = SCRATCH + 0x1000


def witness(name, fraction, fixed_bits=None):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, SCRATCH_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)
    assert bytes(u.mem_read(0x7EB058 + 0x3C0, 4)) == struct.pack('<I', 0x51C8B0)
    assert bytes(u.mem_read(0x7E3860, 8)) == struct.pack('<d', 0.1)
    u.mem_write(OWNER + 0x578, struct.pack('<d', fraction))
    # Native frame after sub esp,14 / push ebx,esi,edi; error 7 unwinds it.
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.mem_write(sp + 0x20, struct.pack('<I', RET_MAGIC))
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_EBX, OWNER)
    u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    end = run_checked(u, ENTRY, (ALLOWED, RET_MAGIC), count=32,
                      required_addresses=(0x51C9BE, 0x51C9C9))
    refused = end == RET_MAGIC
    if refused:
        assert u.reg_read(UC_X86_REG_EAX) == 7
        assert u.reg_read(UC_X86_REG_ESP) == sp + 0x30
    return dict(name=name, fraction=fraction, fixed_bits=fixed_bits,
                refused=refused, fire_error=7 if refused else None)


def generate():
    rows = [witness(f'fixed_{bits}', bits / 65536, bits)
            for bits in (0, 1, 6552, 6553, 6554, 6555, 32768, 65536)]
    rows.extend(witness(name, fraction) for name, fraction in (
        ('below_native_threshold', math.nextafter(0.1, 0.0)),
        ('native_threshold', 0.1),
        ('above_native_threshold', math.nextafter(0.1, 1.0)),
    ))
    return rows


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'infantry_get_fire_error': 0x51C8B0, 'speed_gate': ENTRY,
                      'allowed_boundary': ALLOWED, 'refused_return': 0x51CAFA},
        assumptions=['Supplied Foot+578 fraction and post-common-legality CPU frame.',
                     'Eight exactly representable SimFixed inputs and three binary64 threshold neighbors.'],
        substitutions=['No original instructions patched and no supplied call results within the region.'],
        scope='11 speed-refusal witnesses; earlier common legality and subsequent NavCom/action, weapon and locomotor gates are excluded.',
    ))
