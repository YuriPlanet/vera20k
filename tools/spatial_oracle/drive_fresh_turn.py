"""Original Drive fresh-heading gate, virtual Do_Turn and Facing setter/sampler.

Interior entry: path finding and mission dispatch are deliberately excluded.
An aligned result stops before CanEnter and does not assert track admission.
"""
from itertools import product
from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import (
    RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors, load_image,
    provenance, run_checked,
)
from tools.spatial_oracle.map_queries import dwords

LOCO, OWNER, RESULT = SCRATCH + 0x1000, SCRATCH + 0x2000, SCRATCH + 0x4000
SP = STACK_BASE + STACK_SIZE - 0x1000
VTABLE, FRAME = 0x7E7EB0, 0xA8ED84
GATE, ALIGNED, TURN, SET, CURRENT = 0x4B3408, 0x4B345B, 0x4B0EF0, 0x4C9220, 0x4C93D0
RANGES = ((GATE, ALIGNED), (TURN, 0x4B0F12), (SET, 0x4C92FF),
          (CURRENT, 0x4C9480))


def read32(u, address):
    return struct.unpack('<I', u.mem_read(address, 4))[0]


def sample(u):
    u.mem_write(SP, dwords(RET_MAGIC, RESULT))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ECX, OWNER + 0x388)
    run_checked(u, CURRENT, RET_MAGIC, count=300, required_addresses=(CURRENT,))
    assert u.reg_read(UC_X86_REG_EAX) == RESULT
    return struct.unpack('<H', u.mem_read(RESULT, 2))[0]


def execute(row):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x10000)
    u.mem_map(RET_MAGIC, 0x1000)
    originals = [bytes(u.mem_read(a, b - a)) for a, b in RANGES]
    vtable = bytes(u.mem_read(VTABLE, 0x100))
    assert read32(u, VTABLE + 0x4C) == TURN
    u.mem_write(LOCO + 4, dwords(VTABLE))
    u.mem_write(LOCO + 0xC, dwords(OWNER))
    facing = OWNER + 0x388
    u.mem_write(facing, dwords(row['initial']))
    u.mem_write(facing + 4, dwords(row['initial']))
    u.mem_write(facing + 8, dwords(-1, 0, 0))
    u.mem_write(facing + 0x14, struct.pack('<h', row['rate']))
    visits = []

    def observe(_uc, address, _size, _data):
        if address in (GATE, TURN, SET, CURRENT):
            visits.append(f'{address:08X}')

    u.hook_add(UC_HOOK_CODE, observe)
    calls = []
    for frame in (2, 3):
        u.mem_write(FRAME, dwords(frame))
        before = sample(u)
        visits.clear()
        # Four saved registers precede 0x4C local bytes. The original RET0xC
        # consumes the return address at SP+0x5C and three caller arguments.
        saved = (0x11111111, 0x22222222, 0x33333333, 0x44444444)
        u.mem_write(SP, dwords(*saved))
        u.mem_write(SP + 0x14, dwords(LOCO + 4))
        u.mem_write(SP + 0x5C, dwords(RET_MAGIC, 0, 0, 0))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_EBP, LOCO)
        u.reg_write(UC_X86_REG_EBX, row['direction'])
        run_checked(u, GATE, (ALIGNED, RET_MAGIC), count=500,
                    required_addresses=(GATE, CURRENT))
        end = u.reg_read(UC_X86_REG_EIP)
        gate_visits = list(visits)
        returned = end == RET_MAGIC
        if returned:
            assert u.reg_read(UC_X86_REG_EAX) & 0xFF == 1
            assert u.reg_read(UC_X86_REG_ESP) == SP + 0x6C
            assert tuple(u.reg_read(reg) for reg in
                         (UC_X86_REG_EDI, UC_X86_REG_ESI,
                          UC_X86_REG_EBP, UC_X86_REG_EBX)) == saved
            assert f'{TURN:08X}' in gate_visits and f'{SET:08X}' in gate_visits
        else:
            assert f'{TURN:08X}' not in gate_visits
        calls.append(dict(frame=frame, sampled_before=before, sampled_after=sample(u),
                          destination=read32(u, facing) & 0xFFFF,
                          origin=read32(u, facing + 4) & 0xFFFF,
                          timer_start=read32(u, facing + 8),
                          timer_duration=read32(u, facing + 0x10),
                          boundary='turn_then_return' if returned else 'aligned_before_admission',
                          returned_al=1 if returned else None, visits=gate_visits))
    assert bytes(u.mem_read(VTABLE, 0x100)) == vtable
    assert all(bytes(u.mem_read(a, b - a)) == original
               for (a, b), original in zip(RANGES, originals))
    return dict(input=row, calls=calls, original_code_and_vtable_unchanged=True)


def generate():
    # Exact octants, one-bit mismatches invisible to an 8-bit facing cache,
    # wrap-around and signed-half-circle differences, instant/finite rates.
    return [execute(dict(initial=initial, direction=direction, rate=rate))
            for initial, direction, rate in product(
                (0, 1, 0x1FFF, 0x2000, 0x4000, 0x4001, 0xC000, 0xFFFF),
                (0, 2, 6), (0, 256, 1280, 0x7F00, -1))]


def metadata():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    result = provenance(
        scope='Drive fresh-heading exact16 gate, original virtual Do_Turn, Facing setter and same/next-frame sampled publication',
        assumptions=[
            'Interior entry4B3408 supplies resolved first path direction EBX, class EBP, native local interface pointer and original saved-register frame. Earlier mission/pathfinding/active-turn gates are excluded',
            'PrimaryFacing+388 begins stationary with equal origin/destination, paused zero-duration timer and supplied signed native rate. Negative rate is a raw-state boundary, not a rules-produced rate claim',
            'Two calls retain native Facing state at frames2/3. Re-entering the interior gate while rotating is diagnostic, not proof the full outer Process reaches it then',
            'Aligned stops before4B345B, so CanEnter, terrain, track installation and movement are excluded. Mismatch executes original RET0xC and records AL1',
            '120 rows cross eight initial16-bit angles, three path directions and five rates (240 gate calls). Current4C93D0 executes before/after each call; this establishes facing publication separately from track admission',
        ],
        substitutions=['None. Original Drive ILocomotion vtable+4C, Do_Turn4B0EF0, Facing::Set4C9220 and Current4C93D0 execute unchanged. Hooks only observe addresses'],
        entry_points=dict(fresh_heading_gate=GATE, aligned_before_admission=ALIGNED,
                          drive_do_turn=TURN, facing_set=SET, facing_current=CURRENT),
    )
    result['original_code_range_sha256'] = {
        f'{a:08X}..{b:08X}': hashlib.sha256(bytes(u.mem_read(a, b - a))).hexdigest()
        for a, b in RANGES}
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
