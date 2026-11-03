"""Original TechnoClass::SpawnRadEruption 6FD800: eight kind2 RadBeam births.

Supplied Infantry receiver coordinates, sparse fixed-stride cells with level and
slope, the shared dummy, RadColor and seeded Main/Scenario/Mapgen RNG objects.
The whole wrapper, Infantry GetCoords 5F65A0, Map lookup 5657A0, Cell centre
480A30/47B3A0, RNG, RadBeam allocation 659110, constructor and setters execute.
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW

from tools.native_oracle import (NATIVE_FPCW, RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE,
    finish_vectors, load_image, provenance, run_checked)
from tools.procedural_drawing_oracle.electric_bolt import RngTrace
from tools.procedural_drawing_oracle.rad_beam import beam_state
from tools.spatial_oracle.map_queries import DUMMY, EMPTY_TABLE, GLOBAL_TABLE, TABLE, dwords, packed

ENTRY, NEW, REGISTRY = 0x6FD800, 0x7C8E17, 0xB04A60
INFANTRY_VTABLE, CELL_VTABLE, RULES_POINTER = 0x7EB058, 0x7E4EEC, 0x8871E0
ACTOR, RULES, SCENARIO, ITEMS = SCRATCH, SCRATCH + 0x1000, SCRATCH + 0x3000, SCRATCH + 0x4000
CELLS, HEAP = SCRATCH + 0x8000, SCRATCH + 0x10000
SP = STACK_BASE + STACK_SIZE - 0x1000
RAD_COLOR = (0, 255, 0)  # physical RULESMD [Radiation] RadColor


def execute(case):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x20000)
    assert int.from_bytes(u.mem_read(INFANTRY_VTABLE + 0x48, 4), 'little') == 0x5F65A0
    u.mem_write(TABLE, EMPTY_TABLE)
    u.mem_write(GLOBAL_TABLE, dwords(TABLE, 0x40000))
    u.mem_write(0x89E7C0, dwords(104))  # initialized Cell LevelHeight
    for n, row in enumerate(case['cells']):
        x, y = row['xy']
        ptr = CELLS + n * 0x200
        u.mem_write(TABLE + (y * 512 + x) * 4, dwords(ptr))
        u.mem_write(ptr, dwords(CELL_VTABLE))
        u.mem_write(ptr + 0x24, packed(x, y))
        u.mem_write(ptr + 0x11B, bytes((row['level'] & 255, row['slope'])))
    u.mem_write(DUMMY, dwords(CELL_VTABLE))
    u.mem_write(DUMMY + 0x11B, bytes(2))  # a fresh CellClass: level 0, slope 0
    u.mem_write(ACTOR, dwords(INFANTRY_VTABLE))
    u.mem_write(ACTOR + 0x9C, dwords(*case['source']))
    u.mem_write(RULES_POINTER, dwords(RULES))
    u.mem_write(RULES + 0x1830, bytes(RAD_COLOR))
    u.mem_write(REGISTRY, dwords(0x7EB6D4, ITEMS, 1024, 1, 0, 10))
    rng = RngTrace(u, SCENARIO, case['seed'])
    before = rng.states()
    heap = [HEAP]

    def new(uc, pc, _size, _data):
        if pc != NEW:
            return
        sp = uc.reg_read(UC_X86_REG_ESP)
        ret, size = struct.unpack('<II', uc.mem_read(sp, 8))
        assert size == 0xC8, size
        uc.reg_write(UC_X86_REG_EAX, heap[0])
        heap[0] += size
        uc.reg_write(UC_X86_REG_ESP, sp + 4)
        uc.reg_write(UC_X86_REG_EIP, ret)

    hook = u.hook_add(UC_HOOK_CODE, new, begin=NEW, end=NEW)
    u.mem_write(SP, dwords(RET_MAGIC, case['spread']))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ECX, ACTOR)
    u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    try:
        run_checked(u, ENTRY, RET_MAGIC, count=200_000,
                    required_addresses=(0x5F65A0, 0x5657A0, 0x480A30, 0x47B3A0, 0x659110))
    finally:
        u.hook_del(hook)
    assert u.reg_read(UC_X86_REG_ESP) == SP + 8
    after = rng.states()
    assert before['scenario'] == after['scenario'] and before['mapgen'] == after['mapgen']
    assert {r['stream'] for r in rng.requests} == {'main'} and len(rng.requests) == 40
    count = int.from_bytes(u.mem_read(REGISTRY + 0x10, 4), 'little')
    assert count == 8, count
    births = []
    for index in range(count):
        state = beam_state(u, int.from_bytes(u.mem_read(ITEMS + index * 4, 4), 'little'))
        births.append({key: state[key] for key in ('kind', 'rgb', 'source', 'target', 'z_adjust',
            'duration', 'amplitude_f64_le_hex', 'travel_scale_f64_le_hex')})
    return dict(input=case, main_rng_before=before['main'], main_rng_after=after['main'],
                births=births)


def generate():
    # Interior: firer cell (16,17), spread 10, every target a real cell; four
    # carry level and slope. Edge: firer cell (3,5); x=-7 or y=-5 targets miss
    # (negative or unallocated aliased slot) and read the stamped dummy.
    interior = [dict(xy=[x, y], level=level, slope=slope) for (x, y), level, slope in (
        ((6, 7), 1, 0), ((6, 17), 2, 1), ((6, 27), 0, 0), ((16, 7), 3, 5),
        ((16, 27), 4, 0), ((26, 7), 0, 9), ((26, 17), 1, 13), ((26, 27), 2, 0))]
    edge = [dict(xy=xy, level=1, slope=0) for xy in ([3, 15], [13, 5], [13, 15])]
    cases = [dict(name='interior_levels_and_slopes', seed=31, spread=10,
                  source=[4224 + 61, 4352 + 190, 312], cells=interior),
             dict(name='map_edge_dummy_targets', seed=47, spread=10,
                  source=[3 * 256 + 128, 5 * 256 + 40, 0], cells=edge)]
    return dict(rows=[execute(case) for case in cases])


def metadata():
    return provenance(scope=__doc__, entry_points=dict(spawn_rad_eruption=ENTRY),
        assumptions=[
            'Two controls: interior spread10 with real level/slope target cells, and a map-edge firer whose x=-7/y=-5 targets miss and read the stamped dummy (fresh level0/slope0).',
            'Receiver is a bare Infantry vtable 7EB058 object whose only read field is GetCoords5F65A0 +0x9C. Rules pointer supplies RULESMD RadColor 0,255,0 at +0x1830; spread is the caller-side ftol argument. LevelHeight 89E7C0 is the initialized 104.',
            'RngTrace seeds Main/Scenario/Mapgen with native Random__Seed(seed, seed+1, seed+2); only Main may draw, 40 ranged requests. Registry B04A60 has spare capacity, so no growth runs.',
        ],
        substitutions=['operator new 7C8E17 returns a bump allocation of the requested 0xC8 bytes.'])


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
