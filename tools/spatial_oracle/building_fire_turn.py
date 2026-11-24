"""Original voxel-building Mission_Attack facing retry decision.

44B068 reads raw Type ROT, distinct from FacingClass's capped turn rate.
Stops before Snap/GetFireError and does not establish full fire legality.
"""
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EIP,
    UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import (
    RET_MAGIC, SCRATCH, SCRATCH_SIZE, STACK_BASE, STACK_SIZE,
    finish_vectors, load_image, provenance, run_checked,
)
from tools.spatial_oracle.facing_class import dwords

OWNER, TYPE = SCRATCH, SCRATCH + 0x1000
SP = STACK_BASE + STACK_SIZE - 0x1000


def execute(rot, delta):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, SCRATCH_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)
    u.mem_write(0xA8ED84, dwords(100))
    u.mem_write(TYPE + 0x71C, dwords(rot))
    u.mem_write(OWNER + 0x388, dwords(delta, delta, 100, 0, 0, 0))
    u.mem_write(SP, dwords(RET_MAGIC, rot))
    u.reg_write(UC_X86_REG_ECX, OWNER + 0x388)
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, 0x4C9680, RET_MAGIC, count=1000)
    u.mem_write(SP + 0x14, dwords(0))  # DirectionToTarget result supplied.
    u.reg_write(UC_X86_REG_EAX, TYPE)
    u.reg_write(UC_X86_REG_EBX, 0)
    u.reg_write(UC_X86_REG_ESI, OWNER)
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, 0x44B068, (0x44B0A8, 0x44B0D5), count=1000,
                required_addresses=[0x4C93D0] if rot else [])
    assert u.reg_read(UC_X86_REG_ESP) == SP
    return dict(rot=rot, delta=delta, retry=u.reg_read(UC_X86_REG_EIP) == 0x44B0A8)


def generate():
    return [execute(rot, delta)
            for rot in (-257, -256, -255, -129, -128, -1, 0, 1, 5, 127, 128, 255, 256, 2147483647)
            for delta in (0, 1, 255, 256, 257, 0x7F00, 0x7F01, 0x8000, 0xFF00, 0xFFFF)]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'building_retry_decision': 0x44B068, 'set_rot': 0x4C9680,
                      'current': 0x4C93D0, 'accepted': 0x44B0A8, 'refused': 0x44B0D5},
        assumptions=['Voxel-building retry branch already selected; desired heading supplied as zero and current heading as the input delta.',
                     'Stationary PrimaryFacing and Type ROT supplied; original SetROT initializes the rate; original Current executes for nonzero ROT. Frame100 and caller EBX=0 supplied.'],
        substitutions=[],
        scope='140 original retry decisions across signed/raw-byte ROT and signed direction deltas. Excludes preceding GetFireError, target selection, subsequent Snap/GetFireError and complete Mission_Attack.',
    ))
