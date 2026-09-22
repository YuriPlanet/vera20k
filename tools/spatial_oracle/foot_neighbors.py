"""Original Foot neighbor-history slices, with original virtual/map callees.

Supplied registers enter the retained-counter portions of larger lifecycle
functions. These are counter/history comparisons, not full lifecycle admission.
"""
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_EBX, UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, run_checked, finish_vectors, provenance
from tools.spatial_oracle.aircraft_fire_location import Fixture, OWNER, TYPE, CELLS, SIDE, dwords
from tools.spatial_oracle.map_queries import packed

DUMMY = 0xABDC50


def execute(case):
    x, y = case['cell']
    f = Fixture(dict(aircraft=[x * 256 + 128, y * 256 + 128, case.get('z', 0)]))
    u = f.u
    f.call(0x49F2F0, 0, [])
    seed = case.get('seed', 0)
    cells = bytearray(u.mem_read(CELLS, SIDE * SIDE * 0x200))
    cells[0x122::0x200] = bytes([seed]) * (SIDE * SIDE)
    u.mem_write(CELLS, bytes(cells))
    u.mem_write(DUMMY + 0x122, bytes([seed]))
    u.mem_write(OWNER + 0x55C, packed(*case.get('old', [0, 0])))
    u.mem_write(OWNER + 0x74, bytes([case.get('marked', True)]))
    u.mem_write(0xAC13C8, dwords(104))
    u.mem_write(0xAC13BC, dwords(416))
    u.mem_write(0x8871E0, dwords(SCRATCH + 0x40000))
    u.mem_write(OWNER + 0x21C, dwords(SCRATCH + 0x42000))
    if 'rocket_phase' in case:
        loco = SCRATCH + 0x44000
        f.call(0x661EC0, loco, [])
        u.mem_write(loco + 0x40, dwords(case['rocket_phase']))
        u.mem_write(OWNER + 0x674, dwords(loco + 4))
        u.mem_write(SCRATCH + 0x40000 + case.get('rocket_rule_offset', 0x4E0), dwords(TYPE))
    u.reg_write(UC_X86_REG_ESP, f.sp)
    u.reg_write(UC_X86_REG_ESI, OWNER)
    u.reg_write(UC_X86_REG_EDI, OWNER)
    u.reg_write(UC_X86_REG_EBX, 0)
    entry, stop = {
        'unlimbo': (0x4D7235, 0x4D72E0),
        'per_cell': (0x4D8627, 0x4D8759),
        'limbo': (0x4DB284, 0x4DB2EB),
        'owner_change': (0x4DBF37, 0x4DBF59),
    }[case['operation']]
    run_checked(u, entry, stop, count=100000)
    after = bytes(u.mem_read(CELLS, SIDE * SIDE * 0x200))[0x122::0x200]
    return dict(input=case,
                neighbor_cell=list(struct.unpack('<hh', u.mem_read(OWNER + 0x55C, 4))),
                changed=[[index % SIDE, index // SIDE, value]
                         for index, value in enumerate(after) if value != seed],
                dummy=u.mem_read(DUMMY + 0x122, 1)[0])


def cases():
    result = []
    for operation in ('unlimbo', 'per_cell', 'limbo', 'owner_change'):
        for old, cell in (([0, 0], [64, 64]), ([64, 64], [64, 64]),
                          ([63, 64], [64, 64]), ([60, 60], [64, 64]),
                          ([0, 0], [0, 0]), ([0, 1], [127, 127])):
            for seed in (0, 255):
                result.append(dict(operation=operation, old=old, cell=cell, seed=seed))
    for operation in ('unlimbo', 'owner_change'):
        for z in (207, 208, 900):
            for marked in (False, True):
                result.append(dict(operation=operation, old=[60, 60], cell=[64, 64],
                                   z=z, marked=marked))
        for phase in range(7):
            for offset in (0x4E0, 0x514):
                result.append(dict(operation=operation, old=[60, 60], cell=[64, 64],
                                   z=0, marked=False, rocket_phase=phase, rocket_rule_offset=offset))
    return result


if __name__ == '__main__':
    finish_vectors(lambda: [execute(case) for case in cases()], Path(__file__).with_suffix('.json'),
                   provenance=lambda: provenance(
                       entry_points={'unlimbo_counter_slice': 0x4D7235,
                                     'per_cell_counter_slice': 0x4D8627,
                                     'limbo_counter_slice': 0x4DB284,
                                     'owner_change_history_slice': 0x4DBF37},
                       assumptions=['Supplied live Aircraft owner and original vtable, initialized 128x128 allocated map in native fixed512 table; remaining lookups use original shared Dummy.',
                                    'Enter counter slices with supplied owner/source, seed bytes, stack and registers. Limbo is already admitted; PerCell is already reason2 after its sensor prefix.',
                                    'Original virtual coordinate/high-flight and map callees execute. Native eight-neighbor initializer executes. Rocket cases use the original constructor and supplied phases0..6, under both Rules-designated type slots. No caller admission, sensor deposit, zones or full lifecycle is certified.'],
                       substitutions=[],
                       scope='88 counter/history slices: zero sentinel, overlap, unchanged and disjoint cells, byte wrapping, map edge/shared Dummy, high-flight threshold/Mark gate and V3RocketType/DMislType locomotor override. Not full Unlimbo/PerCell/Limbo/ChangeOwner parity.'))
