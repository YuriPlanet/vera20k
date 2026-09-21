"""Original speed-crate recipient loop; selection/removal and UI suffix excluded.

Runs482F4A..483098 with original Cell height, object coordinate/RTTI, approximate
sqrt and ftol callees. Runtime vector order, selected multiplier and crate cell
are supplied. No hooks replace instructions, calls or returned values.
"""
from pathlib import Path
import math
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import (
    load_image, run_checked, STACK_BASE, STACK_SIZE, SCRATCH, RET_MAGIC, finish_vectors, provenance,
)
from tools.spatial_oracle.map_queries import dwords, packed

CELL, RULES, VECTOR, ACTORS, HOUSES = [SCRATCH + i for i in (0, 0x2000, 0x5000, 0x6000, 0x16000)]
VTABLES = {'infantry': 0x7EB058, 'unit': 0x7F5C70, 'aircraft': 0x7E22A4}
TYPE_VTABLES = {'infantry': 0x7EB610, 'unit': 0x7F6218, 'aircraft': 0x7E2868}


def fixture(case):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x20000)
    u.mem_map(RET_MAGIC, 0x1000)
    u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
    sp = STACK_BASE + STACK_SIZE - 0x1000
    # Entry follows the diagnostic printf;482F4F still removes its three args.
    u.reg_write(UC_X86_REG_ESP, sp - 12)
    u.reg_write(UC_X86_REG_ESI, CELL)
    u.mem_write(CELL + 0x24, packed(10, 10))
    u.mem_write(CELL + 0x11B, bytes([case.get('level', 0), case.get('slope', 0)]))
    u.mem_write(0x89E7C0, dwords(104))
    u.mem_write(0x8871E0, dwords(RULES))
    u.mem_write(RULES + 0x172C, dwords(case.get('radius', 768)))
    u.mem_write(sp + 0x20, struct.pack('<d', case.get('multiplier', 1.2)))
    u.mem_write(sp + 0x1B, b'\0')
    pointers = []
    indexes = {}
    for index, actor in enumerate(case['actors']):
        if actor is None:
            pointers.append(0)
            continue
        address = ACTORS + index * 0x1000
        pointers.append(address)
        indexes[address] = index
        house = HOUSES + actor.get('house', 0) * 0x400
        kind = actor.get('kind', 'infantry')
        u.mem_write(address, dwords(VTABLES[kind]))
        u.mem_write(address + 0x14, dwords(actor.get('flags', 4)))
        u.mem_write(address + 0x21C, dwords(house))
        u.mem_write(house + 0x1ED, bytes([actor.get('player_controlled', 0)]))
        u.mem_write(house + 0x34, dwords(house + 0x200))
        for offset in (0x128, 0x12C, 0x130):
            u.mem_write(house + 0x200 + offset, struct.pack('<f', 1.0))
        object_type = address + 0x800
        u.mem_write(address + (0x6C0 if kind == 'infantry' else 0x6C4), dwords(object_type))
        u.mem_write(object_type, dwords(TYPE_VTABLES[kind]))
        u.mem_write(object_type + 0x678, dwords(actor.get('raw_speed', 10)))
        u.mem_write(address + 0x6CC, dwords(-1))
        u.mem_write(address + 0x578, struct.pack('<d', 1.0))
        u.mem_write(address + 0x580, struct.pack('<d', actor.get('factor', 1.0)))
        delta = actor.get('delta', [0, 0, 0])
        u.mem_write(address + 0x9C, dwords(2688 + delta[0], 2688 + delta[1], delta[2]))
    u.mem_write(VECTOR, dwords(*pointers))
    u.mem_write(0x8A0394, dwords(VECTOR))
    u.mem_write(0x8A03A0, dwords(len(pointers)))
    return u, sp, pointers, indexes


def execute(case):
    u, sp, pointers, indexes = fixture(case)
    visits, distances, kinds = [], [], []

    def observe(_u, address, _size, _data):
        eax = u.reg_read(UC_X86_REG_EAX)
        if address == 0x482F6E:
            visits.append(indexes.get(eax))
        elif address == 0x48302E:
            distances.append(struct.unpack('<i', struct.pack('<I', eax))[0])
        elif address == 0x483057:
            kinds.append(eax)

    u.hook_add(UC_HOOK_CODE, observe)
    run_checked(u, 0x482F4A, (0x483098, 0x4830AF), count=20000,
                required_addresses=[0x482F4A])
    assert u.reg_read(UC_X86_REG_ESP) == sp
    announce = bool(u.mem_read(sp + 0x1B, 1)[0])
    speeds = []
    for pointer in pointers:
        if pointer == 0:
            speeds.append(None)
            continue
        # Consume the newly written factor with the original full Foot getter.
        # Neutral house, rookie/no FASTER, full fraction and no Unit flag supplied.
        initial = bytes(u.mem_read(pointer, 0x800))
        u.reg_write(UC_X86_REG_ECX, pointer)
        u.reg_write(UC_X86_REG_ESP, sp)
        u.mem_write(sp, dwords(RET_MAGIC))
        run_checked(u, 0x4DB1A0, RET_MAGIC, count=3000,
                    required_addresses=[0x50C050, 0x70EFE0, 0x70D0D0, 0x7C5F00])
        assert u.reg_read(UC_X86_REG_ESP) == sp + 4
        assert bytes(u.mem_read(pointer, 0x800)) == initial
        speeds.append(struct.unpack('<i', struct.pack('<I', u.reg_read(UC_X86_REG_EAX)))[0])
    return dict(input=case, visits=visits, distances=distances, object_kinds=kinds,
                factors=[None if p == 0 else f'{struct.unpack("<Q", u.mem_read(p + 0x580, 8))[0]:016x}'
                         for p in pointers],
                foot_speeds=speeds, announce=announce)


def generate():
    cases = [dict(name='empty', actors=[])]
    for name, delta in (
        ('center', [0, 0, 0]), ('inside', [767, 0, 0]),
        ('at_radius', [768, 0, 0]), ('outside', [769, 0, 0]),
        ('diagonal_inside', [542, 542, 0]), ('diagonal_outside', [544, 544, 0]),
        ('vertical_inside', [0, 0, 767]), ('vertical_at_radius', [0, 0, 768]),
    ):
        cases.append(dict(name=name, actors=[dict(delta=delta)]))
    for kind in VTABLES:
        cases.append(dict(name=kind, actors=[dict(kind=kind)]))
    for factor in (0.0, 1.2, math.nextafter(1.0, 0.0), math.nextafter(1.0, 2.0)):
        cases.append(dict(name=f'existing_{factor!r}', actors=[dict(factor=factor)]))
    cases.extend([
        dict(name='non_foot_mask', actors=[dict(flags=1)]),
        dict(name='distinct_owners_and_null', actors=[dict(house=0), None, dict(house=1)]),
        dict(name='controlled_announcement', actors=[dict(player_controlled=1)]),
        dict(name='controlled_outside', actors=[dict(player_controlled=1, delta=[768, 0, 0])]),
        dict(name='elevated_center', level=2, actors=[dict(delta=[0, 0, 208])]),
        dict(name='slope_center', level=2, slope=1, actors=[dict(delta=[0, 0, 260])]),
        dict(name='zero_radius', radius=0, actors=[{}]),
    ])
    return [execute(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'pickup': 0x481A00, 'speed_loop': 0x482F4A,
                      'loop_end': 0x483098, 'empty_end': 0x4830AF,
                      'cell_height': 0x47B3A0, 'sqrt': 0x4CAC40, 'ftol': 0x7C5F00,
                      'foot_speed': 0x4DB1A0},
        assumptions=['Supplied post-selection frame, vector order, object class flags, coordinates, house control byte and selected powerup multiplier.',
                     'Original Infantry/Unit/Aircraft vtables; unused object state zero. PC53/chop ambient state.',
                     'Following original Foot speed calls use the same actor factor, raw speed10 unless overridden, neutral house, rookie/no FASTER, full speed fraction and Unit+6CC=-1. Infantry prone override is outside coverage.',
                     'Retail Powerups.Speed multiplier1.2 and CrateRules.CrateRadius3.0 cells (768 leptons), with explicit contrast cases.'],
        substitutions=[],
        scope='23 original speed-effect loops plus subsequent full Foot speed queries: strict 3-D radius and native distance rounding, existing-factor rejection, class selection, vector order/nulls, owner independence and announcement flag. Excludes pickup admission, weighted selection, crate removal, UI suffix, lifecycle producers and Rust parity.',
    ))
