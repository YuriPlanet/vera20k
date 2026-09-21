"""Original paid Walk numeric step through its placement-branch decision.

Executes 75BFA9..75C117/75C1FB, including the actual Foot speed setter, Walk
facing callback, coordinate/cell getters, direction and sine/cosine routines.
The movement-speed getter's integer result is supplied. Earlier completion and
movement admission, and subsequent coordinate/occupation transactions, are not
executed by this corpus.
"""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EBP, UC_X86_REG_ECX,
    UC_X86_REG_ESP, UC_X86_REG_FPCW,
)
from tools.native_oracle import (
    NATIVE_FPCW, RET_MAGIC, SCRATCH, SCRATCH_SIZE, STACK_BASE, STACK_SIZE,
    finish_vectors, load_image, provenance, run_checked,
)

ENTRY, BOUNDARY, SAME_CELL = 0x75BFA9, 0x75C117, 0x75C1FB
LOCO, FOOT, VTABLE, SPEED = [SCRATCH + i * 0x1000 for i in range(1, 5)]
FRAME = 100


def dwords(*values):
    return struct.pack('<' + 'I' * len(values), *(v & 0xFFFFFFFF for v in values))


def execute(case):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, SCRATCH_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)
    u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    u.mem_write(0xA8ED84, dwords(FRAME))
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.mem_write(sp, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ECX, LOCO)
    u.reg_write(UC_X86_REG_ESP, sp)
    run_checked(u, 0x75AA90, RET_MAGIC, count=200)
    assert u.reg_read(UC_X86_REG_EAX) == LOCO
    assert u.reg_read(UC_X86_REG_ESP) == sp + 4

    # Only the speed receiver is replaced, in a separate fixture vtable.
    u.mem_write(FOOT, dwords(VTABLE))
    for slot, address in ((0x544, 0x4D3710), (0x48, 0x5F65A0), (0x1B8, 0x41BEA0)):
        assert bytes(u.mem_read(0x7EB058 + slot, 4)) == dwords(address)
        u.mem_write(VTABLE + slot, dwords(address))
    assert bytes(u.mem_read(0x7EB058 + 0x538, 4)) == dwords(0x521D80)
    u.mem_write(VTABLE + 0x538, dwords(SPEED))
    u.mem_write(SPEED, b'\xB8' + dwords(case['speed']) + b'\xC3')
    u.mem_write(LOCO + 0xC, dwords(FOOT))
    u.mem_write(LOCO + 0x28, dwords(*case['head']))
    u.mem_write(FOOT + 0x9C, dwords(*case['current']))
    u.mem_write(FOOT + 0x388, dwords(case['initial_facing'], case['initial_previous'],
                                  case['initial_start'], 0, case['initial_duration'],
                                  case['rot'] << 8))
    u.mem_write(FOOT + 0x578, struct.pack('<d', 0.5))
    u.mem_write(FOOT + 0x6B7, b'\x01')
    u.reg_write(UC_X86_REG_EBP, LOCO)
    u.reg_write(UC_X86_REG_EBX, LOCO + 0x28)
    u.reg_write(UC_X86_REG_ESP, sp)
    calls = []
    observed = {0x4D3710: 'set_speed_fraction', SPEED: 'supplied_movement_speed',
                0x5F65A0: 'get_coords', 0x75AE00: 'walk_do_turn',
                0x4C9300: 'snap_facing', 0x41BEA0: 'get_cell_coords',
                0x4CACB0: 'sin_table', 0x4CAD00: 'cos_table'}
    u.hook_add(UC_HOOK_CODE, lambda _u, address, _size, _data:
               calls.append(observed[address]) if address in observed else None)
    end = run_checked(u, ENTRY, (BOUNDARY, SAME_CELL), count=10000,
                      required_addresses=(0x75C035, 0x75C09C, 0x75C0B6, 0x75C0F3))
    assert u.reg_read(UC_X86_REG_ESP) == sp
    assert bytes(u.mem_read(FOOT + 0x9C, 12)) == dwords(*case['current'])
    return dict(input=case,
                proposed=list(struct.unpack('<iii', u.mem_read(sp + 0x30, 12))),
                crosses_cell=end == BOUNDARY,
                facing=struct.unpack('<I', u.mem_read(FOOT + 0x388, 4))[0] & 0xFFFF,
                previous=struct.unpack('<I', u.mem_read(FOOT + 0x38C, 4))[0] & 0xFFFF,
                turn_duration=struct.unpack('<I', u.mem_read(FOOT + 0x398, 4))[0],
                speed_fraction=struct.unpack('<d', u.mem_read(FOOT + 0x578, 8))[0],
                blocked=u.mem_read(FOOT + 0x6B7, 1)[0], calls=calls)


def generate():
    base = dict(current=[2496, 2624, 104], initial_facing=0x8123,
                initial_previous=0x8123, initial_start=0xFFFFFFFF,
                initial_duration=0, rot=5)
    rows = []
    for dx, dy in ((0, -256), (256, -256), (256, 0), (256, 256),
                   (0, 256), (-256, 256), (-256, 0), (-256, -256),
                   (191, -67), (-73, 229)):
        for speed in (1, 6, 17):
            rows.append(execute(dict(base, name=f'heading_{dx}_{dy}_speed_{speed}',
                                     head=[base['current'][0] + dx, base['current'][1] + dy, 104],
                                     speed=speed)))
    for current, head in (([2559, 2559, 104], [2752, 2752, 104]),
                          ([2560, 2560, 104], [2368, 2368, 104]),
                          ([261, 261, 0], [280, 280, 0])):
        rows.append(execute(dict(base, name=f'boundary_{current[0]}',
                                 current=current, head=head, speed=17)))
    rows.append(execute(dict(base, name='zero_speed', head=[2752, 2624, 104], speed=0)))
    rows.append(execute(dict(base, name='snap_equality_keeps_destination',
                             head=[2496, 2368, 104], speed=6,
                             initial_facing=0x3FFF, initial_previous=0,
                             initial_start=FRAME, initial_duration=1, rot=32)))
    return rows


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'walk_process': 0x75AEC0, 'paid_step': ENTRY,
                      'boundary': BOUNDARY, 'same_cell': SAME_CELL,
                      'foot_speed_setter': 0x4D3710, 'infantry_movement_speed': 0x521D80,
                      'walk_turn': 0x75AE00, 'facing_snap': 0x4C9300,
                      'sin_table': 0x4CACB0, 'cos_table': 0x4CAD00},
        assumptions=['Supplied post-admission paid-head frame, owner XYZ and heading; original Walk constructor executes.',
                     'Flat/same-Z cases; endpoints precede physical coordinate, height and occupation transactions.',
                     'Original facing setter executes, including a live-angle equality contrast.'],
        substitutions=['External Infantry movement-speed receiver returns the supplied integer; its original body and Foot getter are excluded.'],
        scope='35 paid-step vectors covering ten directions at three integer speeds, three boundary/near-head cases, zero speed and active-turn equality. No complete Process, admission, completion or Rust parity claim.',
    ))
