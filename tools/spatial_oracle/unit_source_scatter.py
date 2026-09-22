"""Original source-aware Unit Scatter through observed destination dispatch.

Runs native gates, ScenarioRandom, heading, Foot coordinate, map/height and
projection. QueueMission and SetDestination are observers. Candidate entry can
use either supplied numeric answers or the actual Unit73F0A0 body on empty lists.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import SCRATCH, finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.unit_scatter_state import make_fixture, ACTOR, TYPE, LOCO, SOURCE, SP

VT, SCENARIO = SCRATCH + 0x8000, SCRATCH + 0x9000
CELLS = SCRATCH + 0x10000
ENTRY, QUEUE, SET = [SCRATCH + 0xF000 + i * 0x100 for i in range(3)]
MAP, TABLE = 0x87F7E8, 0xC00000
NEIGHBORS = [(10, 9), (11, 9), (11, 10), (11, 11), (10, 11), (9, 11), (9, 10), (9, 9)]


def query(case):
    u, call, read32 = make_fixture(case)
    u.mem_map(CELLS, 0x90000)
    u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
    table = bytearray(0x100000)
    overrides = {(x, y): (level, flags) for x, y, level, flags in case.get('cells', [])}
    for y in range(32):
        for x in range(32):
            ptr = CELLS + (y * 32 + x) * 0x200
            struct.pack_into('<I', table, (y * 512 + x) * 4, ptr)
            u.mem_write(ptr, dwords(0x7E4EEC))
            u.mem_write(ptr + 0x24, packed(x, y))
            level, flags = overrides.get((x, y), (0, 0))
            u.mem_write(ptr + 0x11B, bytes([level & 255, 0]))
            u.mem_write(ptr + 0x140, dwords(flags))
            u.mem_write(ptr + 0x44, dwords(-1))
            u.mem_write(ptr + 0x54, dwords(-1, -1))
    for x, y, ground, deck in case.get('raw', []):
        u.mem_write(CELLS + (y * 32 + x) * 0x200 + 0x124, dwords(ground, deck))
    for x, y in case.get('blocked_terrain', []):
        u.mem_write(CELLS + (y * 32 + x) * 0x200 + 0xEC, dwords(1))
    u.mem_write(0x89EA40, struct.pack('<18f', *([1.0] * 9 + [0.0] * 9)))
    u.mem_write(TABLE, bytes(table))
    u.mem_write(MAP + 0x13C, dwords(TABLE, 0x40000))
    bounds = case.get('bounds', [16, -16, -16, 64, 64])
    u.mem_write(MAP + 0xF4, dwords(bounds[0]))
    u.mem_write(MAP + 0xFC, dwords(*bounds[1:]))
    u.mem_write(VT, bytes(u.mem_read(0x7F5C70, 0x600)))
    assert read32(VT + 0x1AC) == 0x73F0A0
    assert read32(VT + 0x480) == 0x741970
    for offset, pointer in [(0x1AC, ENTRY), (0x1E8, QUEUE), (0x480, SET)]:
        if offset != 0x1AC or not case.get('live_entry'):
            u.mem_write(VT + offset, dwords(pointer))
    u.mem_write(ACTOR, dwords(VT))
    u.mem_write(ACTOR + 0x9C, dwords(*case.get('actor', [2688, 2688, 0])))
    u.mem_write(ACTOR + 0x684, b'\xff')  # no retained tunnel/tube index
    u.mem_write(ACTOR + 0x8C, bytes([case.get('on_bridge', False)]))
    u.mem_write(ACTOR + 0x6AF, bytes([case.get('turret_latch', False)]))
    u.mem_write(ACTOR + 0x6D1, bytes([case.get('unload_active', False)]))
    u.mem_write(ACTOR + 0x2B4, dwords(TYPE if case.get('target') else 0))
    u.mem_write(TYPE + 0x67C, dwords(1))  # supplied speed row 1 (all nine rows agree)
    u.mem_write(TYPE + 0xDFC, dwords(-1))  # no type-specific land restriction
    u.mem_write(LOCO + 0x40, dwords(*case.get('head', [0, 0, 0])))
    control = 0xA8E3A8 + case.get('mission', 5) * 32
    u.mem_write(control + 9, bytes([case.get('mission_scatter', True)]))
    u.mem_write(control + 7, bytes([case.get('paralyzed', False)]))
    u.mem_write(0xA8B230, dwords(SCENARIO))
    u.mem_write(SOURCE, dwords(*case.get('source', [1000, 2688, 0])))
    for address in (0xB1CFE8, 0x8A0790, 0x89C848, 0x8B3DA8):
        u.mem_write(address, dwords(0, 0, 0))
    u.mem_write(0xB1CFB8, packed(0, 0))
    call(0x49F2F0, 0, [])
    call(0x65C6D0, SCENARIO + 0x218, [case.get('seed', 1)])
    events, checks = [], []
    destination, start_direction, pending, returning = None, None, None, None

    def ret(cleanup, value=0):
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value)
        u.reg_write(UC_X86_REG_EIP, read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    def observe(_u, address, _size, _data):
        nonlocal destination, start_direction, pending, returning
        sp = u.reg_read(UC_X86_REG_ESP)
        if address == 0x743E08:
            from unicorn.x86_const import UC_X86_REG_EDI
            start_direction = u.reg_read(UC_X86_REG_EDI) & 7
        elif address == 0x65C7E0:
            events.append(['random', read32(sp + 4), read32(sp + 8)])
        elif address == returning and pending is not None:
            checks.append([*pending, u.reg_read(UC_X86_REG_EAX)])
            pending = None
        elif address == ENTRY or (case.get('live_entry') and address == 0x73F0A0):
            args = [read32(sp + 4 + i * 4) for i in range(5)]
            coord = list(struct.unpack('<hh', u.mem_read(args[0] + 0x24, 4)))
            assert args[3:] == [0, 1]
            height = struct.unpack('<i', dwords(args[2]))[0]
            events.append('entry')
            if case.get('live_entry'):
                pending, returning = [coord, args[1], height], read32(sp)
                return
            answer = case.get('answers', [0] * 8)[args[1]]
            checks.append([coord, args[1], height, answer])
            ret(20, answer)
        elif address == QUEUE:
            assert [read32(sp + 4), read32(sp + 8)] == [2, 0]
            events.append('queue_move')
            ret(8)
        elif address == SET:
            assert read32(sp + 8) == 1
            destination = list(struct.unpack('<hh', u.mem_read(read32(sp + 4) + 0x24, 4)))
            events.append('destination')
            ret(8)

    u.hook_add(UC_HOOK_CODE, observe)
    call(0x743A50, ACTOR, [SOURCE, case.get('force', True), case.get('second', False)])
    assert pending is None
    assert u.reg_read(UC_X86_REG_ESP) == SP + 16
    assert read32(LOCO + 0x14) == 1
    return dict(input=case, destination=destination, checks=checks, events=events,
                start_direction=start_direction,
                random_indices=[read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)])


def generate():
    cases = [dict(seed=seed, source=source) for seed in (1, 31, 42)
             for source in ([1000, 2688, 0], [4000, 2688, 0], [2688, 1000, 0],
                            [2688, 4000, 0], [1000, 1000, 0], [4000, 4000, 0])]
    cases += [dict(answers=[code] * 8) for code in range(1, 8)]
    cases += [dict(answers=[0 if i == direction else 7 for i in range(8)]) for direction in range(8)]
    cases += [dict(paralyzed=True), dict(unload_active=True), dict(nav=True),
              dict(nav=True, second=True), dict(nav=True, second=True, turret_latch=True),
              dict(force=False, mission_scatter=False), dict(force=True, mission_scatter=False),
              dict(head=[3968, 3968, 0]), dict(cells=[[x, y, 0, 0x100] for x, y in NEIGHBORS])]
    cases += [dict(force=False, target=True, seed=seed) for seed in (1, 2, 3, 4, 5, 6, 8, 1000)]
    live = [dict(), dict(raw=[[x, y, 0x20, 0] for x, y in NEIGHBORS]),
            dict(raw=[[x, y, 0x1C, 0] for x, y in NEIGHBORS]),
            dict(blocked_terrain=NEIGHBORS), dict(raw=[[11, 10, 0x20, 0]]),
            dict(cells=[[x, y, 0, 0x100] for x, y in NEIGHBORS])]
    cases += [dict(case, live_entry=True) for case in live]
    return [query(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Source-aware Unit Scatter743A50 through observed QueueMission/SetDestination: original admission, RNG, heading, Foot navigation seed, numeric candidate entry and preferred/fallback selection. Not full class setter or movement parity.',
        entry_points={'scatter': 0x743A50, 'unit_entry': 0x73F0A0, 'foot_coord': 0x4DBDF0,
                      'random': 0x65C7E0, 'projection': 0x6D6410, 'height': 0x5F5F00},
        assumptions=['Original Unit/Drive fixture from unit_scatter_state, only OS Interlocked imports substituted there.',
                     'Widened synthetic bounds, 32x32 allocated original Cell vtables, empty object lists/overlays, supplied raw masks/levels/bridge flags; all nine land0 speed rows1.0 and land1 rows0.0. Type native SpeedType row1, unrestricted land(-1), Foot tube index(-1); no weapon/radio consumers admitted by empty lists.',
                     'Original ScenarioRandom seed and direction-table startup execute. Nonnull source only; optional old NavCom/Target are pointer-identity sentinels compared but never dereferenced. No NULL-source FNPC coverage.'],
        substitutions=['Candidate+1AC returns supplied per-direction answers unless live_entry=true, which executes original Unit73F0A0.',
                       'QueueMission and SetDestination are argument-checking observers without state effects.']))
