"""Original AircraftClass::Find_Attack_Cell (0x00418E20) and Mission_Move (0x004166E0).

Find_Attack_Cell rows run the original search on the fly-landing fixture's flat 128x128 map
(tools/spatial_oracle/fly_landing_phase.py), every cell carrying the CellClass vtable
0x007E4EEC. Its callees execute: FootClass::IsLandZoneClear 0x004DDC60 (vt+0x550),
AircraftClass::Is_Cell_Free_For_Landing 0x00419B00, MapClass::Is_Cell_In_Playfield 0x00578460
and Get_CellClass 0x005657A0, the sine and cosine tables 0x004CACB0/0x004CAD00, ftol
0x007C5F00 and Scenario RandomRanged 0x0065C7E0. A row supplies the map state: cells whose raw
occupation (+0x124, the unit bit 0x20) fails the passability test, cells holding the fixture's
blocker object (+0xE4), aircraft whose NavCom (+0x5A4) reserves a cell, the LocalSize
playfield, and the search's parameter (a cell or the blocker object). The blocker is a live
Unit in the Foot vector 0x008B3DC4; the owner and the reserving aircraft are in both it and the
Aircraft vector 0x00A8E394.

Mission_Move rows preset Mission+0xBC and run the original 0x004166E0 on the same fixture with
the Aircraft vtable 0x007E22A4 cloned: slots +0x480 (Assign_Destination 0x0041AA80) and +0x484
(Enter_Idle_Mode 0x004176F0) are stubs that record their arguments, and the owner's ILocomotion
(+0x674) is a scratch interface whose Is_Moving (+0x10) answers the row's value and whose
Move_To (+0x44) records its coordinate. Find_Attack_Cell, the NavCom's coordinate virtuals
(+0x48, +0x4C), the owner's cell (vt+0x1B8 0x0041BEA0), Is_Cell_Free_For_Landing, the
MissionControl entry 0x005B3A00, ftol and RandomRanged run natively.

Every row records the Scenario RandomRanged results by call site, the raw words their rejection
sampling consumed, and the next raw Random 0x0065C780 after the call, so a port that draws on
another path cannot hide behind the same result.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
)

from tools.native_oracle import SCRATCH, finish_vectors, provenance
from tools.spatial_oracle.aircraft_fire_location import (
    BLOCKER, BLOCKER_TYPE, CELLS, SCENARIO, SIDE, TYPE, cell, dwords,
)
from tools.spatial_oracle.fly_landing_phase import HOUSE, OWNER, RULES, fixture

FIND_ATTACK_CELL, MISSION_MOVE = 0x418E20, 0x4166E0
CELL_VT, AIRCRAFT_VT, UNIT_VT = 0x7E4EEC, 0x7E22A4, 0x7F5C70
MISSION_CONTROL, MOVE = 0xA8E3A8, 2
# Both aircraft registries: Is_Cell_Free_For_Landing walks 0x008B3DC4 (count 0x008B3DD0),
# IsLandZoneClear the vector whose items are at 0x00A8E394 (count 0x00A8E3A0).
FREE_VECTOR, FREE_COUNT, ZONE_VECTOR, ZONE_COUNT = 0x8B3DC4, 0x8B3DD0, 0xA8E394, 0xA8E3A0
# Rules+0x1478 has no INI reader: the RulesClass constructor's 0x2000 (0x00667235) is the
# value every scenario searches with, 32 rings.
RING_RADIUS = 0x2000
# Scratch above the fixture's cells (SCRATCH + 0x200000 .. + 0xA00000).
VTABLE = SCRATCH + 0xA00000
LOCO = SCRATCH + 0xA01000
LOCO_VT = LOCO + 0x100
BLOCKER_LOCO = SCRATCH + 0xA01800
BLOCKER_LOCO_VT = BLOCKER_LOCO + 0x100
STUBS = SCRATCH + 0xA02000
RESERVERS = SCRATCH + 0xA04000
ITEMS = SCRATCH + 0xA20000
STUB_POPS = {'assign_destination': 8, 'enter_idle_mode': 8, 'is_moving': 4, 'move_to': 16,
             'blocker_destination': 8}
STUB_AT = {name: STUBS + 0x10 * n for n, name in enumerate(STUB_POPS)}
STUB_NAME = {address: name for name, address in STUB_AT.items()}
DRAW_SITES = {0x418F54: 'ring', 0x41673E: 'epilogue'}
# RandomRanged's rejection loop holds each raw Random word in ESI here (0x0065C87C MOV EAX,ESI).
RAW_WORD = 0x65C87C


def word(u, address):
    return struct.unpack('<I', u.mem_read(address, 4))[0]


def i32(value):
    return struct.unpack('<i', struct.pack('<I', value & 0xFFFFFFFF))[0]


def name(pointer, param=None):
    """A returned or passed pointer as data: null, the parameter, the blocker or a cell."""
    if pointer == 0:
        return None
    if pointer == BLOCKER:
        return 'blocker'
    if CELLS <= pointer < CELLS + SIDE * SIDE * 0x200 and (pointer - CELLS) % 0x200 == 0:
        index = (pointer - CELLS) // 0x200
        return ['cell', index % SIDE, index // SIDE]
    raise AssertionError(f'unexpected pointer 0x{pointer:08X}')


class Run:
    def __init__(self, row):
        self.row = row
        self.f, _ = fixture(dict(cell=row.get('owner_cell', [60, 64]), z=row.get('z', 0)))
        f, u = self.f, self.f.u
        # Every cell a real flat Clear CellClass without overlay, Track land cost 1.0.
        for y in range(SIDE):
            for x in range(SIDE):
                u.mem_write(cell(x, y), dwords(CELL_VT))
                u.mem_write(cell(x, y) + 0x44, dwords(0xFFFFFFFF))
        u.mem_write(0x89EA40, struct.pack('<90f', *[1.0] * 90))
        u.mem_write(RULES + 0x1478, dwords(RING_RADIUS))
        u.mem_write(0x87F7E8 + 0xFC, dwords(*row.get('local', [0, 0, 64, 64])))
        u.mem_write(TYPE + 0xD54, bytes([row.get('spawned', False)]))
        for x, y in row.get('occupied', []):
            u.mem_write(cell(x, y) + 0x124, b'\x20')
        bx, by = row.get('blocker_at', [1, 1])
        u.mem_write(BLOCKER + 0x9C, dwords(bx * 256 + 128, by * 256 + 128, 0))
        u.mem_write(BLOCKER + 0x90, b'\1')
        u.mem_write(BLOCKER + 0x21C, dwords(HOUSE))
        u.mem_write(BLOCKER + 0x2D0, dwords(int(row.get('blocker_spawn_manager', False))))
        u.mem_write(BLOCKER_TYPE + 0xD54, bytes([row.get('blocker_spawned', False)]))
        for x, y in row.get('blocker_cells', []):
            u.mem_write(cell(x, y) + 0xE4, dwords(BLOCKER))
        # The blocker is a Foot without a tube (+0x684 = -1): its vt+0x4C (0x004DBDF0) reads
        # its locomotor's Destination (+0x18), which answers the row's coordinate.
        u.mem_write(BLOCKER + 0x684, b'\xFF')
        u.mem_write(BLOCKER + 0x674, dwords(BLOCKER_LOCO))
        u.mem_write(BLOCKER_LOCO, dwords(BLOCKER_LOCO_VT))
        u.mem_write(BLOCKER_LOCO_VT + 0x18, dwords(STUB_AT['blocker_destination']))
        u.mem_write(STUBS, b'\xCC' * 0x100)
        aircraft = [OWNER]
        for n, (x, y) in enumerate(row.get('reserved', [])):
            actor = RESERVERS + n * 0x1000
            u.mem_write(actor, dwords(UNIT_VT))
            u.mem_write(actor + 0x74, b'\1')
            u.mem_write(actor + 0x81, b'\0')
            u.mem_write(actor + 0x90, b'\1')
            u.mem_write(actor + 0x9C, dwords(128, 128, 0))
            u.mem_write(actor + 0x5A4, dwords(cell(x, y)))
            aircraft.append(actor)
        # The blocker is a live Unit: a Foot (0x008B3DC4) but not an Aircraft (0x00A8E394).
        for vector, count, items, members in ((FREE_VECTOR, FREE_COUNT, ITEMS, aircraft + [BLOCKER]),
                                              (ZONE_VECTOR, ZONE_COUNT, ITEMS + 0x1000, aircraft)):
            u.mem_write(items, dwords(*members))
            u.mem_write(vector, dwords(items))
            u.mem_write(count, dwords(len(members)))
        u.mem_write(OWNER + 0x5D4, dwords(0))
        u.mem_write(OWNER + 0x520, dwords(0xFFFFFFFF))
        f.call(0x65C6D0, SCENARIO + 0x218, [row.get('seed', 31)])
        self.draws, self.raw, self.calls = [], [], []
        u.hook_add(UC_HOOK_CODE, self.on_draw, begin=min(DRAW_SITES), end=max(DRAW_SITES))
        u.hook_add(UC_HOOK_CODE, self.on_raw, begin=RAW_WORD, end=RAW_WORD)
        u.hook_add(UC_HOOK_CODE, self.on_stub, begin=STUBS, end=STUBS + 0xFF)

    def on_draw(self, u, address, _size, _data):
        if address in DRAW_SITES:
            self.draws.append([DRAW_SITES[address], i32(u.reg_read(UC_X86_REG_EAX))])

    def on_raw(self, u, _address, _size, _data):
        self.raw.append(u.reg_read(UC_X86_REG_ESI))

    def on_stub(self, u, address, _size, _data):
        stub = STUB_NAME[address]
        sp = u.reg_read(UC_X86_REG_ESP)
        args = [word(u, sp + 4 * (n + 1)) for n in range(STUB_POPS[stub] // 4)]
        value = 0
        if stub == 'assign_destination':
            assert u.reg_read(UC_X86_REG_ECX) == OWNER
            self.calls.append(['assign_destination', name(args[0]), i32(args[1])])
        elif stub == 'enter_idle_mode':
            assert u.reg_read(UC_X86_REG_ECX) == OWNER
            self.calls.append(['enter_idle_mode', i32(args[0]), i32(args[1])])
        elif stub == 'blocker_destination':
            assert args[0] == BLOCKER_LOCO
            u.mem_write(args[1], dwords(*self.row.get('blocker_destination', [0, 0, 0])))
            value = args[1]
        elif stub == 'is_moving':
            assert args[0] == LOCO
            value = int(self.row['moving'])
            self.calls.append(['is_moving'])
        else:
            assert args[0] == LOCO
            self.calls.append(['move_to', [i32(a) for a in args[1:]]])
        u.reg_write(UC_X86_REG_EAX, value)
        u.reg_write(UC_X86_REG_EIP, word(u, sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + STUB_POPS[stub])

    def param(self):
        target = self.row.get('param')
        if target is None:
            return 0
        if target == 'blocker':
            return BLOCKER
        return cell(*target)

    def finish(self, **result):
        f = self.f
        next_random = f.call(0x65C780, SCENARIO + 0x218, [])
        return dict(input=self.row, draws=self.draws, raw=self.raw, next_random=next_random,
                    **result)


def find_attack_cell(row):
    run = Run(row)
    pointer = run.f.call(FIND_ATTACK_CELL, OWNER, [run.param()])
    return run.finish(result=name(pointer))


def mission_move(row):
    run = Run(row)
    f, u = run.f, run.f.u
    vtable = bytearray(u.mem_read(AIRCRAFT_VT, 0x600))
    assert word(u, AIRCRAFT_VT + 0x480) == 0x41AA80 and word(u, AIRCRAFT_VT + 0x484) == 0x4176F0
    struct.pack_into('<I', vtable, 0x480, STUB_AT['assign_destination'])
    struct.pack_into('<I', vtable, 0x484, STUB_AT['enter_idle_mode'])
    u.mem_write(VTABLE, bytes(vtable))
    u.mem_write(OWNER, dwords(VTABLE))
    u.mem_write(LOCO, dwords(LOCO_VT))
    u.mem_write(LOCO_VT + 0x10, dwords(STUB_AT['is_moving']))
    u.mem_write(LOCO_VT + 0x44, dwords(STUB_AT['move_to']))
    u.mem_write(OWNER + 0x674, dwords(LOCO))
    u.mem_write(OWNER + 0xAC, dwords(MOVE))
    rate = struct.unpack('<f', struct.pack('<f', row.get('rate', 0.016)))[0]
    u.mem_write(MISSION_CONTROL + MOVE * 0x20 + 0x10, struct.pack('<d', rate))
    u.mem_write(OWNER + 0x5A4, dwords(run.param()))
    u.mem_write(OWNER + 0xBC, dwords(row['state']))
    delay = i32(f.call(MISSION_MOVE, OWNER, []))
    return run.finish(delay=delay, state=i32(word(u, OWNER + 0xBC)), calls=run.calls)


def find_rows():
    # The owner sits at (60, 64), four cells west of the default parameter (64, 64).
    ring1 = [[64 + dx, 64 + dy] for dx in (-1, 0, 1) for dy in (-1, 0, 1)]
    ring2 = [[64 + dx, 64 + dy] for dx in range(-2, 3) for dy in range(-2, 3)]
    rows = [
        dict(name='null'),
        dict(name='clear_cell', param=[64, 64]),
        dict(name='occupied_cell', param=[64, 64], occupied=[[64, 64]]),
        dict(name='reserved_cell', param=[64, 64], reserved=[[64, 64]]),
        dict(name='blocked_ring1', param=[64, 64], occupied=ring1),
        dict(name='blocked_ring2', param=[64, 64], occupied=ring2),
        dict(name='reserved_ring1', param=[64, 64], occupied=[[64, 64]],
             reserved=[[63, 63], [64, 63], [65, 63], [63, 64], [65, 64]]),
        dict(name='blocker_object', param='blocker', blocker_at=[64, 64],
             blocker_cells=[[64, 64]]),
        dict(name='blocker_object_ring1', param='blocker', blocker_at=[64, 64],
             blocker_cells=ring1),
        dict(name='blocker_object_moving', param='blocker', blocker_at=[64, 64],
             blocker_cells=[[64, 64]], blocker_destination=[70 * 256 + 128, 64 * 256 + 128, 0]),
        dict(name='blocker_object_moving_clear', param='blocker', blocker_at=[64, 64],
             blocker_destination=[70 * 256 + 128, 64 * 256 + 128, 0]),
        dict(name='blocker_object_moving_reserved', param='blocker', blocker_at=[64, 64],
             blocker_destination=[70 * 256 + 128, 64 * 256 + 128, 0], reserved=[[70, 64]]),
        dict(name='owner_cell', param=[60, 64], occupied=[[60, 64]]),
        # Playfield sum x+y > 64 on this fixture: ring 1 around (33, 33) crosses the edge.
        dict(name='playfield_edge', param=[33, 33],
             occupied=[[33 + dx, 33 + dy] for dx in (-1, 0, 1) for dy in (-1, 0, 1)]),
        dict(name='outside_playfield', param=[20, 20]),
        dict(name='spawned_launcher_cell', param=[64, 64], spawned=True, blocker_at=[64, 64],
             blocker_cells=[[64, 64]], blocker_spawn_manager=True),
        dict(name='spawned_other_cell', param=[64, 64], spawned=True, blocker_at=[64, 64],
             blocker_cells=[[64, 64]]),
        dict(name='spawned_blocked_ring1', param=[64, 64], spawned=True, occupied=ring1),
    ]
    rows += [dict(name=f'blocked_ring1_seed_{seed}', param=[64, 64], occupied=ring1, seed=seed)
             for seed in (1, 7, 12345, 0x5EED)]
    return rows


def move_rows():
    rows = []
    for state in (0, 1, 2, 3, 4, 5):
        for nav in (None, [64, 64], 'blocker'):
            for moving in (False, True):
                rows.append(dict(name=f's{state}_{nav}_{moving}', state=state, param=nav,
                                 moving=moving, blocker_at=[66, 64]))
    rows += [
        dict(name='s0_blocked_nav', state=0, param=[64, 64], moving=True,
             occupied=[[64, 64]]),
        dict(name='s2_nav_is_own_cell', state=2, param=[60, 64], moving=True),
        dict(name='s2_nav_reserved', state=2, param=[64, 64], moving=True,
             reserved=[[64, 64]]),
        dict(name='s4_nav_is_own_cell', state=4, param=[60, 64], moving=True),
        dict(name='s4_nav_reserved', state=4, param=[64, 64], moving=True,
             reserved=[[64, 64]]),
        dict(name='s0_rate', state=0, param=[64, 64], moving=True, rate=0.03, seed=99),
        dict(name='s0_spawned_blocked_nav', state=0, param=[64, 64], moving=True,
             spawned=True, occupied=[[64, 64]]),
        dict(name='s2_spawned_nav', state=2, param=[64, 64], moving=True, spawned=True),
        dict(name='s2_far_nav', state=2, param=[90, 64], moving=True),
        dict(name='s1_blocker_moving', state=1, param='blocker', moving=True, blocker_at=[66, 64],
             blocker_destination=[70 * 256 + 128, 64 * 256 + 128, 0]),
        dict(name='s2_blocker_moving_reserved', state=2, param='blocker', moving=True,
             blocker_at=[66, 64], blocker_destination=[70 * 256 + 128, 64 * 256 + 128, 0],
             reserved=[[70, 64]]),
    ]
    return rows


def generate():
    return dict(find_attack_cell=[find_attack_cell(row) for row in find_rows()],
                mission_move=[mission_move(row) for row in move_rows()])


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='AircraftClass::Find_Attack_Cell on supplied flat-map states (clear, occupied and '
              'reserved parameters, blocked first and second rings, the blocker object, the '
              "owner's own cell, a playfield edge, five seeds) and Mission_Move states 0..5 with "
              'a null, cell and object NavCom, moving or not, plus occupied and reserved NavCom '
              'cells and a second Rate. Not the Carryall arm (0x00416D50), the planning-path '
              'branch of state 2, Assign_Destination, Enter_Idle_Mode or the locomotors.',
        entry_points={'find_attack_cell': FIND_ATTACK_CELL, 'mission_move': MISSION_MOVE,
                      'land_zone_clear': 0x4DDC60, 'cell_free_for_landing': 0x419B00,
                      'cell_in_playfield': 0x578460, 'mission_control': 0x5B3A00},
        assumptions=['The fly-landing fixture: an ordinary marked Aircraft over flat Clear cells '
                     'with real Cell/Map vtables, its Fly locomotor replaced for Mission_Move; '
                     'MapSize width 64; Rules+0x1478 = 0x2000, the RulesClass constructor value '
                     '(no INI reader writes it); MissionControl[Move].Rate the float-widened row '
                     'rate; no team (+0x5D4) and no planning path (+0x520 = -1).'],
        substitutions=['Aircraft vt+0x480 and vt+0x484 record their arguments and return; the '
                       "owner's ILocomotion is a scratch interface answering Is_Moving from the "
                       'row and recording Move_To.'],
    ))
