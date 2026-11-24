"""Original entity GetLayer through real vtables and locomotor constructors.

Supplies one flat/ramped cell and object lifecycle bytes; no calls are replaced.
This checks queries, not the Unlimbo/Process resubmission cadence.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle.crate_speed_effect import fixture, CELL, RULES
from tools.spatial_oracle.map_queries import dwords, EMPTY_TABLE, TABLE, GLOBAL_TABLE

LOCO = SCRATCH + 0x1B000
CTORS = {'walk': 0x75AA90, 'drive': 0x4AF540, 'fly': 0x4CC9A0, 'jumpjet': 0x54AC40}


def execute(case):
    u, sp, actors, _ = fixture(dict(actors=[dict(kind='unit')]))
    owner = actors[0]

    def call(address, receiver=0, args=()):
        u.reg_write(UC_X86_REG_ECX, receiver)
        u.reg_write(UC_X86_REG_ESP, sp)
        u.mem_write(sp, dwords(RET_MAGIC, *args))
        run_checked(u, address, RET_MAGIC, count=10000)
        assert u.reg_read(UC_X86_REG_ESP) == sp + 4 + 4 * len(args)
        return u.reg_read(UC_X86_REG_EAX)

    u.mem_write(TABLE, EMPTY_TABLE)
    # Both lookup bodies require Map+140 length as well as Map+13C pointer.
    u.mem_write(GLOBAL_TABLE, dwords(TABLE, 0x40000))
    u.mem_write(TABLE + (10 * 512 + 10) * 4, dwords(CELL))
    u.mem_write(CELL + 0x11B, bytes([case.get('level', 0), case.get('slope', 0)]))
    u.mem_write(CELL + 0x140, dwords(0x100 if case.get('bridge', False) else 0))
    for address, value in [(0xAC13C8, 104), (0xAC13BC, 416), (0xABC5DC, 416)]:
        u.mem_write(address, dwords(value))
    u.mem_write(RULES + 0x420, dwords(case.get('global_cruise_height', 400)))
    u.mem_write(owner + 0x74, bytes([int(case.get('marked', True))]))
    u.mem_write(owner + 0x8C, bytes([int(case.get('on_bridge', False)),
                                  int(case.get('falling', False))]))
    u.mem_write(owner + 0x9C, dwords(2688, 2688, case['z']))
    kind = case['kind']
    if kind == 'building':
        u.mem_write(owner, dwords(0x7E3EBC))
    else:
        call(CTORS[kind], LOCO)
        u.mem_write(LOCO + 0xC, dwords(owner))
        u.mem_write(owner + 0x674, dwords(LOCO + 4))
        if kind == 'jumpjet':
            u.mem_write(LOCO + 0x2C, dwords(case.get('linked_height', 500)))
    vtable = struct.unpack('<I', u.mem_read(owner, 4))[0]
    entry = struct.unpack('<I', u.mem_read(vtable + 0x78, 4))[0]
    ground_reads = []
    bridge_reads = []
    def observe(_u, address, _size, _data):
        if address == 0x47B3A0:
            cell = u.reg_read(UC_X86_REG_ECX)
            assert cell == CELL, f'ground query unexpectedly selected {cell:#x}'
            ground_reads.append(cell)
        elif address == 0x54B91B:
            cell = u.reg_read(UC_X86_REG_EAX)
            assert cell == CELL, f'Jumpjet bridge query unexpectedly selected {cell:#x}'
            bridge_reads.append(cell)
    u.hook_add(UC_HOOK_CODE, observe)
    layer = call(entry, owner)
    if kind in ('fly', 'jumpjet'):
        assert ground_reads
    if kind == 'jumpjet' and not case.get('on_bridge', False):
        assert bridge_reads
    return dict(input=case, layer=layer)


def generate():
    cases = []
    for kind in ['walk', 'drive', 'fly', 'jumpjet', 'building']:
        for z in [-1, 0, 1, 207, 208, 399, 400, 416, 499, 500, 600]:
            cases.append(dict(name=f'{kind}_height_{z}', kind=kind, z=z))
    for z in [208, 415, 416, 417, 624, 915, 916]:
        for marked, on_bridge, falling in [(True, False, False), (False, False, False),
                                           (True, True, False), (True, False, True)]:
            cases.append(dict(name=f'bridge_{z}_{marked}_{on_bridge}_{falling}',
                              kind='jumpjet', z=z, bridge=True, marked=marked,
                              on_bridge=on_bridge, falling=falling))
    cases += [dict(name='slope_fly_on_surface', kind='fly', z=260, level=2, slope=1),
              dict(name='slope_fly_one_above', kind='fly', z=261, level=2, slope=1),
              dict(name='unmarked_high_building', kind='building', z=600, marked=False),
              dict(name='global_threshold_building', kind='building', z=600, global_cruise_height=700),
              dict(name='global_does_not_replace_linked_height', kind='jumpjet', z=600, global_cruise_height=700)]
    return [execute(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'foot_layer': 0x4DB7E0, 'fly_layer': 0x4CFCF0,
                      'jumpjet_layer': 0x54B8D0, 'object_layer': 0x5F4260,
                      'get_height': 0x5F5F40, 'high_flying': 0x5F6B90},
        assumptions=['Real Unit and Building vtables; original Walk/Drive/Fly/Jumpjet constructors. Object location, lifecycle bytes and one map cell are supplied.',
                     'Map+13C table and Map+140 length are initialized. Read-only observers require ground and Jumpjet bridge queries to select the real cell; earlier fixtures without the length used Dummy and did not establish ramp/bridge behavior.',
                     'Jumpjet linked height and global CruiseHeight are supplied; this does not execute the rules reader or Link_To_Object.',
                     'No allocator or callbacks are replaced. Layer queries do not establish resubmission timing or full process parity.'],
        substitutions=[],
        scope='88 entity layer queries across height thresholds, marking, bridge/falling, terrain slope and distinct linked/global cruise heights. Excludes non-entity layers, lifecycle submission and Rocket/Fly process behavior.',
    ))
