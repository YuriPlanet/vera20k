"""Original ore-field harvesting: Mission_Harvest states 0/1 and their helpers.

Run python -m tools.spatial_oracle.harvest_field --check (or --write).

On the refinery_dock fixture (a War Miner: the real Unit vtable over a
constructed Drive on the 32x32 map, a 4x3 DockUnload refinery whose NW cell
is (6,9)) each row lays ore/gem overlays on cells and runs original bytes:
- CellClass::Reduce_Tiberium 0x480A80, with the real TiberiumClass
  AddToGrowthQueue/ClearSpreadBitmaps/AddToSpreadQueue over supplied queues;
- FootClass::Scan_For_Tiberium 0x4DD0A0 and Is_Cell_Harvestable 0x4DCE80;
- FootClass::Search_For_Tiberium_And_Move 0x4DCFE0;
- UnitClass::Harvest_Ore_Tick 0x73D450;
- UnitClass::Mission_Harvest 0x73E5E0 in states 0 and 1.

Observed at entry and answered (see `substitutions` in the meta file):
MapClass::Can_Reach_Zone 0x56D100, CellClass::RecalcAttributes 0x47D2B0,
RadarClass::MarkTerrainDirty 0x6551C0, TacticalClass::DirtyScreenRect.
Unit Assign_Destination 0x741970 runs live (the Drive MoveTo).
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.refinery_dock import (
    ACTOR, ASSIGN, BLD, EXTRA, HOUSE, RULES, TIB_ITEMS, TYPE, cell, cell_xy,
    make_dock_fixture, observe_dock, state as dock_state,
)
from tools.spatial_oracle.unit_source_scatter import SCENARIO

REDUCE, SCAN, SEARCH, ORE_TICK, HARVEST = 0x480A80, 0x4DD0A0, 0x4DCFE0, 0x73D450, 0x73E5E0
IS_HARVESTABLE, REACH_ZONE, RECALC, RADAR_DIRTY = 0x4DCE80, 0x56D100, 0x47D2B0, 0x6551C0
ADD_GROWTH, ADD_SPREAD, CLEAR_BITMAPS = 0x7235A0, 0x722AF0, 0x722AB0
DIRTY_RECTS, DIRTY_SCREEN = (0x47FDE0, 0x47FB90), 0x6D2790
# A region of its own for the overlay/Tiberium tables and their queues.
FIELD = 0x21000000
OVERLAY_ITEMS, OVERLAYS, TIBS, QUEUES = FIELD, FIELD + 0x1000, FIELD + 0x20000, FIELD + 0x30000
# Overlay ordinals: Riparius TIB01..TIB12 + TIB13..TIB20, Cruentus GEM01..GEM12.
IMAGES = {0: (102, 12, 8), 1: (27, 12, 0)}
OVERLAY_COUNT = 128


def place_tiberium(u, read32, case):
    """The overlay/Tiberium tables and the row's ore cells."""
    u.mem_map(FIELD, 0x40000)
    # MapClass MapSize height (+0xF8): Cell_in_bounds_check 0x568300 and the
    # queue capacity 0x42B1F0 read it; the 16-wide diamond holds (1..31, 1..31).
    u.mem_write(0x87F7E8 + 0xF8, dwords(16))
    # Ground speed table [LandType][9 SpeedTypes] (0x89EA40): Tiberium (5)
    # passable like Clear, as the Rust scene's [Tiberium] Wheel=100%.
    u.mem_write(0x89EA40 + 5 * 9 * 4, struct.pack('<9f', *[1.0] * 9))
    u.mem_write(0xA83D84, dwords(OVERLAY_ITEMS))
    for index in range(OVERLAY_COUNT):
        u.mem_write(OVERLAY_ITEMS + index * 4, dwords(OVERLAYS + index * 0x300))
        u.mem_write(OVERLAYS + index * 0x300 + 0x294, dwords(index))
        tiberium = any(base <= index < base + main + extra for base, main, extra in IMAGES.values())
        u.mem_write(OVERLAYS + index * 0x300 + 0x2A9, bytes([tiberium]))
    u.mem_write(0xB0F4F8, dwords(4))
    spread = case.get('spread_percentage', 0.1)
    for index in range(4):
        tib = TIBS + index * 0x200
        u.mem_write(TIB_ITEMS + index * 4, dwords(tib))
        u.mem_write(tib + 0xB8, dwords((25, 50, 25, 25)[index]))
        base, main, extra = IMAGES.get(index, (OVERLAY_COUNT - 1, 0, 0))
        u.mem_write(tib + 0x98, dwords(index))
        u.mem_write(tib + 0xA0, struct.pack('<d', spread))
        u.mem_write(tib + 0xE0, dwords(OVERLAYS + base * 0x300))
        u.mem_write(tib + 0xE8, dwords(main, extra))
        # Spread queue: entries (+0xFC, count +0xF0), heap (+0xF4), bitmap (+0xF8).
        q = QUEUES + index * 0x1000
        u.mem_write(tib + 0xF0, dwords(0, q, q + 0x400, q + 0x100))
        u.mem_write(q, dwords(0, 64, q + 0x200, 0, 0xFFFFFFFF))
        # Growth queue: count +0x10C, heap +0x110, bitmap +0x114, entries +0x118.
        g = q + 0x800
        u.mem_write(tib + 0x10C, dwords(0, g, g + 0x400, g + 0x100))
        u.mem_write(g, dwords(0, 64, g + 0x200, 0, 0xFFFFFFFF))
    for x, y in case.get('spread_queued', []):
        u.mem_write(QUEUES + 0x400 + bitmap_index(x, y), b'\x01')
    u.mem_write(SCENARIO, dwords(0x80 if case.get('spreads', True) else 0))
    for x, y, kind, variant, data in case.get('ore', []):
        base = IMAGES[kind][0]
        u.mem_write(cell(x, y) + 0x44, dwords(base + variant))
        u.mem_write(cell(x, y) + 0x11E, bytes([data]))
        u.mem_write(cell(x, y) + 0xEC, dwords(5))


def bitmap_index(x, y, width=16):
    """FUN_0042B1C0 over the fixture's MapClass width (bounds[0])."""
    return (x - width + y - 1) * width + ((x - y + width - 1) >> 1)


def observe_field(u, read32, case, events, reach_args):
    unreachable = {tuple(c) for c in case.get('unreachable', [])}
    bare = case.get('bare_land', 0)

    def ret(cleanup, value=0):
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value)
        u.reg_write(UC_X86_REG_EIP, read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    def xy(pointer):
        return list(struct.unpack('<hh', u.mem_read(pointer, 4)))

    def observe(_u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        this = u.reg_read(UC_X86_REG_ECX)
        if address == REACH_ZONE:
            # Target only; the source cell, MovementZone and the three flag
            # arguments are the same on every probe of a row (reach_args).
            target = xy(read32(sp + 8))
            args = [xy(read32(sp + 4)), read32(sp + 12), read32(sp + 16) & 0xFF,
                    read32(sp + 20) & 0xFF, read32(sp + 24) & 0xFF]
            if reach_args and reach_args[0] != args:
                raise AssertionError(('Can_Reach_Zone arguments changed within a row', args))
            reach_args[:1] = [args]
            events.append(['reach', target])
            ret(0x18, tuple(target) not in unreachable)
        elif address == RECALC:
            events.append(['recalc', cell_xy(this)])
            u.mem_write(this + 0xEC, dwords(bare))
            ret(4)
        elif address == RADAR_DIRTY:
            events.append(['radar_dirty', xy(read32(sp + 4))])
            ret(4)
        elif address in DIRTY_RECTS:
            # The cell's screen rectangles for the tactical redraw: empty.
            out = read32(sp + 4)
            u.mem_write(out, dwords(0, 0, 0, 0))
            ret(4, out)
        elif address == DIRTY_SCREEN:
            ret(0x14)
        elif address == ADD_SPREAD:
            events.append(['spread_queue', (this - TIBS) // 0x200, xy(read32(sp + 4))])
        elif address == ADD_GROWTH:
            events.append(['growth_queue', (this - TIBS) // 0x200, xy(read32(sp + 4))])

    u.hook_add(UC_HOOK_CODE, observe)


def field_state(u, read32, case):
    signed = lambda address: struct.unpack('<i', u.mem_read(address, 4))[0]
    cells = {}
    for x, y, *_ in case.get('ore', []):
        cells[f'{x},{y}'] = [signed(cell(x, y) + 0x44), u.mem_read(cell(x, y) + 0x11E, 1)[0],
                             signed(cell(x, y) + 0xEC)]
    heaps = []
    for index in range(2):
        q = QUEUES + index * 0x1000
        count = read32(TIBS + index * 0x200 + 0xF0)
        entries = [[list(struct.unpack('<hh', u.mem_read(q + 0x100 + i * 8, 4))),
                    struct.unpack('<f', u.mem_read(q + 0x104 + i * 8, 4))[0]] for i in range(count)]
        heaps.append(entries)
    base = dock_state(u, read32)
    return dict(base, cells=cells, spread=heaps, harvesting=u.mem_read(ACTOR + 0x6D2, 1)[0],
                archive=cell_xy(read32(ACTOR + 0x218)), flag_3d0=u.mem_read(ACTOR + 0x3D0, 1)[0],
                house_no_ore=u.mem_read(HOUSE + 0x242, 1)[0],
                random_indices=[read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)])


def fixture(case):
    # At the ore field the miner holds no radio contact (the dock fixture
    # links it to the refinery unless told otherwise).
    u, call, read32 = make_dock_fixture(dict(case, linked=case.get('linked', False)))
    events, unused = observe_dock(u, read32, case)
    place_tiberium(u, read32, case)
    reach_args = []
    observe_field(u, read32, case, events, reach_args)
    # [General] TiberiumShortScan/TiberiumLongScan (leptons) and HarvesterLoadRate.
    u.mem_write(RULES + 0x1778, dwords(case.get('short_scan', 6 * 256), case.get('long_scan', 48 * 256)))
    u.mem_write(RULES + 0x1520, dwords(case.get('load_rate', 2)))
    u.mem_write(TYPE + 0x800, dwords(case.get('capacity', 40)))
    u.mem_write(TYPE + 0xE0F, bytes([case.get('weeder', False)]))
    u.mem_write(ACTOR + 0x6D2, bytes([case.get('harvesting', False)]))
    archive = case.get('archive')
    u.mem_write(ACTOR + 0x218, dwords(cell(*archive) if archive else 0))
    return u, call, read32, events, (unused, reach_args)


def reduce(case):
    """CellClass::Reduce_Tiberium(amount) on the row's cell."""
    u, call, read32, events, (unused, reach) = fixture(case)
    call(REDUCE, cell(*case['cell']), [case['amount']])
    removed = u.reg_read(UC_X86_REG_EAX)
    return dict(input=case, removed=removed, events=events, reach_args=reach,
                state=field_state(u, read32, case))


def scan(case):
    """FootClass::Scan_For_Tiberium(range, flag) from the miner's cell."""
    u, call, read32, events, (unused, reach) = fixture(case)
    out = EXTRA + 0x2BF00
    call(SCAN, ACTOR, [out, case['range'], case.get('flag', 0)])
    found = list(struct.unpack('<hh', u.mem_read(out, 4)))
    return dict(input=case, found=found, events=events, reach_args=reach,
                state=field_state(u, read32, case))


def search(case):
    """FootClass::Search_For_Tiberium_And_Move(range, flag)."""
    u, call, read32, events, (unused, reach) = fixture(case)
    call(SEARCH, ACTOR, [case['range'], case.get('flag', 0)])
    ok = u.reg_read(UC_X86_REG_EAX) & 0xFF
    return dict(input=case, ok=ok, events=events, reach_args=reach,
                state=field_state(u, read32, case))


def ore_tick(case):
    """UnitClass::Harvest_Ore_Tick on the miner."""
    u, call, read32, events, (unused, reach) = fixture(case)
    call(ORE_TICK, ACTOR, [])
    ok = u.reg_read(UC_X86_REG_EAX) & 0xFF
    return dict(input=case, ok=ok, events=events, reach_args=reach,
                state=field_state(u, read32, case))


def harvest(case):
    """One UnitClass::Mission_Harvest dispatch in state 0 or 1."""
    from tools.native_oracle import RET_MAGIC, run_checked
    from tools.spatial_oracle.unit_scatter_state import SP
    u, call, read32, events, (unused, reach) = fixture(case)
    u.mem_write(SP, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ECX, ACTOR)
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, HARVEST, RET_MAGIC, count=5_000_000, required_addresses=[HARVEST])
    delay = struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0]
    assert not any(unused), (case, unused)
    return dict(input=case, delay=delay, events=events, reach_args=reach,
                state=field_state(u, read32, case))


ORE = [10, 10, 0, 3, 5]


def ring(data, kind=0, center=(10, 10)):
    """The eight neighbours of `center`, N..NW, each with ore of `data`."""
    x, y = center
    steps = [(0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1)]
    return [[x + dx, y + dy, kind, 0, d] for (dx, dy), d in zip(steps, data)]


def reduce_cases():
    empty = [10, 10, 0, 3, 0]
    return [
        dict(name='reduce_partial', cell=[10, 10], amount=1, ore=[ORE]),
        dict(name='reduce_last_level', cell=[10, 10], amount=1, ore=[[10, 10, 0, 3, 1]]),
        dict(name='reduce_empty_level', cell=[10, 10], amount=1, ore=[empty]),
        dict(name='reduce_two_of_five', cell=[10, 10], amount=2, ore=[ORE]),
        dict(name='reduce_all_of_five', cell=[10, 10], amount=6, ore=[ORE]),
        dict(name='reduce_zero_amount', cell=[10, 10], amount=0, ore=[ORE]),
        dict(name='reduce_no_tiberium', cell=[10, 10], amount=1, ore=[]),
        dict(name='reduce_full_density', cell=[10, 10], amount=1, ore=[[10, 10, 0, 11, 11]]),
        dict(name='reduce_gem', cell=[10, 10], amount=1, ore=[[10, 10, 1, 2, 7]]),
        dict(name='reduce_empty_neighbours_0_to_7', cell=[10, 10], amount=1,
             ore=[empty] + ring([0, 1, 2, 3, 4, 5, 6, 7])),
        dict(name='reduce_empty_neighbours_dense', cell=[10, 10], amount=1,
             ore=[empty] + ring([11, 10, 9, 8, 7, 6, 5, 4])),
        dict(name='reduce_empty_gem_neighbours', cell=[10, 10], amount=1,
             ore=[empty] + ring([0, 1, 2, 3, 4, 5, 6, 7], kind=1)),
        dict(name='reduce_empty_neighbours_queued', cell=[10, 10], amount=1,
             ore=[empty] + ring([6] * 8), spread_queued=[[10, 9], [11, 10]]),
        dict(name='reduce_empty_no_spread_flag', cell=[10, 10], amount=1,
             ore=[empty] + ring([6] * 8), spreads=False),
        dict(name='reduce_empty_zero_spread_percentage', cell=[10, 10], amount=1,
             ore=[empty] + ring([6] * 8), spread_percentage=0.0),
        dict(name='reduce_empty_seed_31', cell=[10, 10], amount=1,
             ore=[empty] + ring([6] * 8), seed=31),
    ]


def scan_cases():
    # The miner stands at (15,15), clear of both refineries (the second one's
    # 4x3 foundation starts at (20,20)).
    at = dict(miner_cell=[15, 15], range=6)
    return [
        dict(at, name='scan_own_cell', ore=[[15, 15, 0, 0, 3], [16, 15, 0, 0, 9]]),
        dict(at, name='scan_nothing', ore=[]),
        dict(at, name='scan_ring1_single', ore=[[16, 16, 0, 0, 2]]),
        dict(at, name='scan_ring1_richest', ore=[[14, 14, 0, 0, 2], [16, 15, 0, 0, 7], [15, 16, 0, 0, 4]]),
        dict(at, name='scan_ring1_tie_top_row_first', ore=[[14, 15, 0, 0, 5], [15, 14, 0, 0, 5]]),
        dict(at, name='scan_ring1_tie_corner', ore=[[16, 16, 0, 0, 5], [16, 14, 0, 0, 5], [14, 16, 0, 0, 5]]),
        dict(at, name='scan_ring1_beats_richer_ring2', ore=[[17, 15, 0, 0, 11], [15, 16, 0, 0, 0]]),
        dict(at, name='scan_ring2_only', ore=[[17, 13, 0, 0, 3], [13, 17, 0, 0, 4]]),
        dict(at, name='scan_gem_beats_ore', ore=[[16, 15, 0, 0, 5], [14, 15, 1, 0, 3]]),
        dict(at, name='scan_last_ring', ore=[[20, 15, 0, 0, 3]]),
        dict(at, name='scan_beyond_range', ore=[[21, 15, 0, 0, 3]]),
        dict(at, name='scan_range_two', range=2, ore=[[17, 15, 0, 0, 3], [16, 14, 0, 0, 1]]),
        dict(at, name='scan_range_one', range=1, ore=[[16, 15, 0, 0, 3]]),
        dict(at, name='scan_unreachable_skipped', ore=[[16, 15, 0, 0, 9], [14, 15, 0, 0, 2]],
             unreachable=[[16, 15]]),
        # (9,11) is a foundation cell of the refinery at NW (6,9): Unit
        # Can_Enter_Cell refuses it, unless the refinery is the radio contact.
        dict(at, name='scan_building_cell_skipped', miner_cell=[10, 12],
             ore=[[9, 11, 0, 0, 9], [11, 12, 0, 0, 2]]),
        dict(at, name='scan_contact_building_cell_admitted', miner_cell=[10, 12], linked=True,
             ore=[[9, 11, 0, 0, 9], [11, 12, 0, 0, 2]]),
        dict(at, name='scan_flag_set', flag=1, ore=[[16, 16, 0, 0, 2]]),
    ]


def search_cases():
    at = dict(miner_cell=[15, 15], range=6)
    return [
        dict(at, name='search_on_ore', ore=[[15, 15, 0, 0, 3]]),
        dict(at, name='search_moves_to_ore', ore=[[16, 15, 0, 0, 3]]),
        dict(at, name='search_nothing', ore=[]),
        dict(at, name='search_driving', ore=[[16, 15, 0, 0, 3]], nav=[18, 18], moving=True),
    ]


def ore_tick_cases():
    on = dict(miner_cell=[15, 15])
    stage = [9, 0, 150, 0, 2]
    return [
        dict(on, name='tick_takes_one', ore=[[15, 15, 0, 0, 5]], stage=stage),
        dict(on, name='tick_gem', ore=[[15, 15, 1, 0, 5]], stage=stage),
        dict(on, name='tick_last_level', ore=[[15, 15, 0, 0, 1]], stage=stage),
        dict(on, name='tick_empty_level', ore=[[15, 15, 0, 0, 0]] + ring([4] * 8, center=(15, 15)),
             stage=stage),
        dict(on, name='tick_not_tiberium', ore=[], stage=stage),
        dict(on, name='tick_driving', ore=[[15, 15, 0, 0, 5]], stage=stage, nav=[18, 18], moving=True),
        dict(on, name='tick_full', ore=[[15, 15, 0, 0, 5]], stage=stage, storage=[40, 0, 0, 0]),
        dict(on, name='tick_fills', ore=[[15, 15, 0, 0, 5]], stage=stage, storage=[39, 0, 0, 0]),
        dict(on, name='tick_mixed_storage', ore=[[15, 15, 1, 0, 5]], stage=stage, storage=[30, 5, 0, 0]),
        dict(on, name='tick_not_harvester', ore=[[15, 15, 0, 0, 5]], stage=stage, harvester=False),
        dict(on, name='tick_load_rate_three', ore=[[15, 15, 0, 0, 5]], stage=stage, load_rate=3),
    ]


def harvest_cases():
    # TiberiumLongScan 12 cells keeps a scan miss inside the step budget.
    s0 = dict(miner_cell=[15, 15], mission='harvest', status=0, long_scan=12 * 256)
    s1 = dict(s0, status=1, harvesting=True)
    ready = [9, 0, 150, 0, 2]
    return [
        dict(s0, name='s0_full', ore=[[15, 15, 0, 0, 3]], storage=[40, 0, 0, 0]),
        dict(s0, name='s0_on_ore', ore=[[15, 15, 0, 0, 3]], stage=[4, 0, 120, 15, 1]),
        dict(s0, name='s0_ore_nearby', ore=[[17, 16, 0, 0, 3]]),
        dict(s0, name='s0_no_ore', ore=[]),
        dict(s0, name='s0_no_ore_partly_full', ore=[], storage=[12, 0, 0, 0]),
        dict(s0, name='s0_archive', ore=[[17, 16, 0, 0, 3]], archive=[20, 12]),
        dict(s0, name='s0_archive_on_ore', ore=[[15, 15, 0, 0, 3]], archive=[20, 12]),
        dict(s0, name='s0_driving', ore=[[17, 16, 0, 0, 3]], nav=[18, 18], moving=True),
        dict(s0, name='s0_gem_nearby', ore=[[13, 15, 1, 0, 1], [17, 15, 0, 0, 3]]),
        dict(s1, name='s1_stage_unarmed', ore=[[15, 15, 0, 0, 5]], stage=[0, 0, -1, 0, 0]),
        dict(s1, name='s1_counting', ore=[[15, 15, 0, 0, 5]], stage=[5, 0, 150, 2, 2]),
        dict(s1, name='s1_harvest', ore=[[15, 15, 0, 0, 5]], stage=ready),
        dict(s1, name='s1_driving', ore=[[17, 16, 0, 0, 5]], stage=ready, nav=[17, 16], moving=True),
        dict(s1, name='s1_fills', ore=[[15, 15, 0, 0, 5]], stage=ready, storage=[39, 0, 0, 0]),
        dict(s1, name='s1_full_on_ore', ore=[[15, 15, 0, 0, 5]], stage=ready, storage=[40, 0, 0, 0]),
        dict(s1, name='s1_full_ore_nearby', ore=[[15, 15, 0, 0, 0], [18, 14, 0, 0, 2]], stage=ready,
             storage=[40, 0, 0, 0]),
        dict(s1, name='s1_full_nothing_near', ore=[[23, 15, 0, 0, 2]], stage=ready,
             storage=[40, 0, 0, 0], archive=[20, 12]),
        dict(s1, name='s1_depleted_hop', ore=[[15, 15, 0, 0, 0], [16, 14, 0, 0, 2]], stage=ready,
             storage=[20, 0, 0, 0]),
        dict(s1, name='s1_depleted_nothing', ore=[[15, 15, 0, 0, 0]], stage=ready,
             storage=[20, 0, 0, 0], archive=[20, 12]),
        dict(s1, name='s1_off_ore_hop', ore=[[16, 16, 1, 0, 4]], stage=ready),
        dict(s1, name='s1_gems_full', ore=[[15, 15, 1, 0, 5]], stage=ready, storage=[30, 10, 0, 0]),
    ]


def generate():
    return {'source': 'unicorn/gamemd.exe',
            'reduce': [reduce(case) for case in reduce_cases()],
            'scan': [scan(case) for case in scan_cases()],
            'search': [search(case) for case in search_cases()],
            'ore_tick': [ore_tick(case) for case in ore_tick_cases()],
            'harvest': [harvest(case) for case in harvest_cases()]}


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='69 original executions at the ore field: 16 CellClass::Reduce_Tiberium calls (with the '
              'TiberiumClass growth/spread queue inserts and their Scenario draws), 17 '
              'FootClass::Scan_For_Tiberium scans (with Is_Cell_Harvestable), 4 '
              'Search_For_Tiberium_And_Move calls, 11 UnitClass::Harvest_Ore_Tick calls and 21 '
              'UnitClass::Mission_Harvest dispatches in states 0 and 1.',
        entry_points={'reduce_tiberium': REDUCE, 'scan_for_tiberium': SCAN,
                      'search_for_tiberium_and_move': SEARCH, 'harvest_ore_tick': ORE_TICK,
                      'mission_harvest': HARVEST, 'is_cell_harvestable': IS_HARVESTABLE,
                      'add_to_spread_queue': ADD_SPREAD, 'add_to_growth_queue': ADD_GROWTH},
        assumptions=['refinery_dock fixture: original Unit vtable over a constructed Drive, 32x32 map, '
                     'House, Rules, Scenario RNG seeded through the original seeder; a 4x3 DockUnload '
                     'refinery at NW (6,9) listed in its foundation cells but the pad, a second at (20,20) '
                     'listed in none.',
                     'MapClass MapSize 16x16 (+0xF4/+0xF8): Cell_in_bounds_check and the queue capacity '
                     'read it. Ground speed table rows LandType 0 and 5 (Tiberium) passable for every '
                     'SpeedType, as the Rust scene.',
                     'Overlay ordinals: Riparius TIB01..TIB12 at 102.. (+8 extra), Cruentus GEM01..GEM12 '
                     'at 27..; each row cell is (x, y, tiberium, variant, OverlayData) with LandType 5. '
                     'TiberiumClass Values 25/50/25/25, SpreadPercentage 0.1 unless the row says, spread '
                     'and growth queues empty with 64 slots, Scenario flag 0x80 (spreads) unless the row '
                     'says.',
                     '[General] TiberiumShortScan 6 cells, TiberiumLongScan 48 cells (12 in the dispatch '
                     'rows), HarvesterLoadRate 2 unless the row says; Storage 40; retail [Harvest] Rate.'],
        substitutions=['MapClass::Can_Reach_Zone 0x56D100 is observed and answers reachable unless the row '
                       'lists the target cell.',
                       'CellClass::RecalcAttributes 0x47D2B0 is observed and sets the cleared cell\'s '
                       'LandType to the row\'s bare land (0).',
                       'RadarClass::MarkTerrainDirty 0x6551C0, the cell screen rectangles 0x47FDE0/'
                       '0x47FB90 and TacticalClass::DirtyScreenRect 0x6D2790 are observed and return '
                       'without effect.']))
