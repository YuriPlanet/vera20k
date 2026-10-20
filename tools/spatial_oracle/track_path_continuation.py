"""Original Drive/Ship Process path request continuations after Foot::Find_Path.

Run python -m tools.spatial_oracle.track_path_continuation --check (or --write).
The real Unit Cell setter (0x741970), outer Process and Process_Movement run.
Find_Path (0x4D3920) is supplied in the found/failed rows: a found route is
written to Foot+5E0 and AL=1 returned, a refusal returns AL=0 with no writes
(the destination survives, as after the nonhuman relocation 0x500200). The
native rows execute the original Find_Path wrapper with only the AStar core
(0x4CBBA0) supplied: NULL, or a PathType whose moves it writes to the
wrapper's buffer. Rows stop where Process_Movement returns to the outer
Process (Drive 0x4B0A7E / Ship 0x6A0147) or at head selection in the same
call (Drive 0x4B32A1 / Ship 0x6A28F1).

The post_warp rows replace the setter with a Chrono Warp's
Force_Track(-1, destination) (0x6CCAA2 through the locomotor's vt+0x70) on
a Unit already standing on its landing: the original Force_Track with the
Unit's own coordinate, after the warp's NULL setter left Foot+5E0's live
head -1 and no NavCom. Find_Path then asks for the Unit's own cell, and the
AStar core is supplied NULL: AStar_pathfind_search returns 0 without
searching when the start cell and height are the goal's (0x429BF3..0x429C0A).
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.track_destination import make_destination_fixture, ACTOR, LOCO
from tools.spatial_oracle.unit_entry import EXTRA, HOUSE, ENEMY
from tools.spatial_oracle.unit_scatter_state import SP, TYPE
from tools.spatial_oracle.unit_source_scatter import CELLS, MAP

PROCESS = {'drive': 0x4B0500, 'ship': 0x69FC10}
FORCE_TRACK = {'drive': 0x4B0C40, 'ship': 0x6A0310}
RETURNED = {'drive': 0x4B0A7E, 'ship': 0x6A0147}
HEAD_SELECTION = {'drive': 0x4B32A1, 'ship': 0x6A28F1}
FIND_PATH, ASTAR = 0x4D3920, 0x4CBBA0
RULES = EXTRA + 0x10000
ZONE_RECORDS, ZONE_TABLE, PATH_TYPE = EXTRA + 0x2C000, EXTRA + 0x2D000, EXTRA + 0x2E000
COUNTRY = EXTRA + 0x2F800
PROCESS_FRAME = 101


def cell(x, y):
    return CELLS + (y * 32 + x) * 0x200


def install_zone_inputs(u, read32, far_zone=()):
    """Supplied native level-zero connectivity, shared by composed Drive scenes."""
    stride = read32(MAP + 0xF8) + 1 + read32(MAP + 0xF4)
    count = stride * 33
    records = bytearray(count * 4)
    for x, y in far_zone:
        struct.pack_into('<H', records, (stride * y + x) * 4 + 2, 1)
    u.mem_write(ZONE_RECORDS, bytes(records))
    u.mem_write(ZONE_TABLE, struct.pack('<HH', 1, 2))
    u.mem_write(MAP + 0x68, dwords(ZONE_RECORDS, count))
    u.mem_write(MAP + 0x18, dwords(ZONE_TABLE))


def query(case):
    family = case['family']
    u, call, read32 = make_destination_fixture(dict(family=family, head=[0, 0, 0],
                                                    mission=case.get('mission', 5)))
    # unit_entry's owner/house prestate for the live Unit73F0A0 answers.
    u.mem_write(0xA8E9A0, b'\x01')
    for address, index in ((HOUSE, 0), (ENEMY, 1)):
        u.mem_write(address + 0x30, dwords(index))
    u.mem_write(HOUSE + 0x1EC, bytes([case.get('human', True)]))
    u.mem_write(RULES + 0x1718, dwords(case.get('close_enough', 128)))
    u.mem_write(RULES + 0x1760, struct.pack('<d', 0.01))
    # MovementZone 0 lookup (Map+18) over per-cell records (Map+68/+6C), index
    # (Map+F8 + 1 + Map+F4) * y + x as in GetZoneID 0x56D230.
    install_zone_inputs(u, read32, case.get('far_zone', []))
    if case.get('post_warp'):
        # HouseClass::GetSpeedBonus 0x50C050 reads the country type's +0x12C.
        u.mem_write(HOUSE + 0x34, dwords(COUNTRY))
        u.mem_write(COUNTRY + 0x12C, struct.pack('<f', 1.0))
        u.mem_write(ACTOR + 0x5E0, dwords(-1))
        location = struct.unpack('<iii', u.mem_read(ACTOR + 0x9C, 12))
        force = FORCE_TRACK[family]
        assert read32(read32(LOCO + 4) + 0x70) == force
        u.mem_write(SP, dwords(RET_MAGIC, LOCO + 4, -1, *location))
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, force, RET_MAGIC, count=100000, required_addresses=[force])
    else:
        destination = case.get('destination', [11, 10])
        call(0x741970, ACTOR, [cell(*destination), 1])
    if case.get('empty_path'):
        u.mem_write(ACTOR + 0x5E0, dwords(-1))
    u.mem_write(ACTOR + 0x64C, dwords(case.get('retries', 10)))

    events = []

    def ret(cleanup, value):
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value)
        u.reg_write(UC_X86_REG_EIP, read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    observed = {0x741970: 'unit_destination', 0x4D55C0: 'failed_receiver',
                0x4B28A8: 'drive_continuation', 0x6A1EF8: 'ship_continuation',
                0x481670: 'scatter_objects', 0x578AD0: 'gate_open'}
    if case.get('post_warp'):
        # Scenario draws, Random 0x65C780 and RandomRanged 0x65C7E0.
        observed.update({0x65C780: 'random', 0x65C7E0: 'random_ranged'})

    def observe(_u, address, _size, _data):
        if address == FIND_PATH:
            sp = u.reg_read(UC_X86_REG_ESP)
            packed = read32(sp + 4)
            events.append(['find_path', packed & 0xFFFF, packed >> 16,
                           read32(sp + 8), read32(sp + 12)])
            result = case['find_path']
            if result == 'native':
                return
            if result == 'found':
                route = case['route']
                u.mem_write(ACTOR + 0x5E0, dwords(*route, *([-1] * (24 - len(route)))))
            ret(12, int(result == 'found'))
        elif address == ASTAR:
            route = case.get('route')
            events.append('astar_found' if route else 'astar_null')
            if not route:
                ret(24, 0)
                return
            # As AStar_reconstruct_path 0x42AA90 returns it: the moves then -1
            # in the wrapper buffer (second argument, 0x4D3E33), PathType +4
            # cost (at least 1) and +8 length = moves + 1 (node count).
            sp = u.reg_read(UC_X86_REG_ESP)
            u.mem_write(read32(sp + 8), dwords(*route, -1))
            u.mem_write(PATH_TYPE, dwords(0, len(route), len(route) + 1, read32(sp + 8), 0, 0, 0, 0))
            ret(24, PATH_TYPE)
        elif address in observed:
            events.append(observed[address])

    u.hook_add(UC_HOOK_CODE, observe)
    u.mem_write(0xA8ED84, dwords(PROCESS_FRAME))
    entry = read32(read32(LOCO + 4) + 0x40)
    assert entry == PROCESS[family]
    u.mem_write(SP, dwords(RET_MAGIC, LOCO + 4))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ECX, 0)
    stop = run_checked(u, entry, (RETURNED[family], HEAD_SELECTION[family]),
                       count=2000000, required_addresses=[entry])
    signed = lambda address, count: list(struct.unpack('<' + 'i' * count,
                                                       u.mem_read(address, count * 4)))
    nav = read32(ACTOR + 0x5A4)
    state = dict(destination=signed(LOCO + 0x34, 3), head=signed(LOCO + 0x40, 3),
                 selector=signed(LOCO + 0x58, 1)[0],
                 nav=list(struct.unpack('<hh', u.mem_read(nav + 0x24, 4))) if nav else None,
                 path=signed(ACTOR + 0x5E0, 4), retries=read32(ACTOR + 0x64C),
                 movement_timer=[read32(ACTOR + 0x640), read32(ACTOR + 0x648)],
                 blocked_timer=[read32(ACTOR + 0x668), read32(ACTOR + 0x670)],
                 target=read32(ACTOR + 0x2B4),
                 mission=signed(ACTOR + 0xAC, 1)[0], queued=signed(ACTOR + 0xB4, 1)[0])
    if stop == RETURNED[family]:
        sp = u.reg_read(UC_X86_REG_ESP)
        state.update(returned=True, result=u.reg_read(UC_X86_REG_EAX) & 0xFF,
                     out=u.mem_read(sp + 0x24, 1)[0])
    else:
        state.update(returned=False)
    return dict(input=case, events=events, state=state)


def generate():
    rows = []
    for family in PROCESS:
        base = dict(family=family)
        rows += [dict(base, find_path='found', route=route, retries=3)
                 for route in ([2], [0, 2], [8])]
        rows += [dict(base, find_path='failed', retries=n) for n in (10, 1, 0)]
        rows += [dict(base, find_path='failed', mission=2, close_enough=n) for n in (256, 257)]
        rows += [dict(base, find_path='failed', mission=11, close_enough=257),
                 dict(base, find_path='failed', mission=7, close_enough=257),
                 dict(base, find_path='failed', mission=7, close_enough=257, empty_path=True),
                 dict(base, find_path='failed', far_zone=[[11, 10]])]
        rows += [dict(base, find_path='native', destination=destination, human=human)
                 for destination in ([11, 10], [13, 10]) for human in (True, False)]
        rows += [dict(base, find_path='native', destination=[13, 10], route=[2, 2, 2])]
        rows += [dict(base, find_path='native', post_warp=True, mission=mission)
                 for mission in (5, 2)]
    return [query(row) for row in rows]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original Unit Cell setter then Drive/Ship outer Process through the no-queue Find_Path continuation: 6 supplied found routes, 18 supplied refusals (retry ladder, CloseEnough stop for Move/AreaGuard with Guard and Enter contrasts, the Enter-without-contact setter keeping the path word, zone recheck), 8 original-wrapper AStar-NULL rows (near/far, human/nonhuman), 2 original-wrapper found rows, and 4 post-warp rows (original Force_Track(-1, own coordinate), Guard and Move, own-cell Find_Path with the AStar-NULL core). No head selection, Process_Track, NavQueue, code-3/code-6 cell answers or Rust parity claim.',
        entry_points={'unit_destination': 0x741970, 'find_path': FIND_PATH, 'astar_core': ASTAR,
                      'drive_force_track': FORCE_TRACK['drive'],
                      'ship_force_track': FORCE_TRACK['ship'],
                      'failed_receiver': 0x4D55C0, 'zone_precheck': 0x4D3810, **PROCESS,
                      'drive_returned': RETURNED['drive'], 'ship_returned': RETURNED['ship'],
                      'drive_head_selection': HEAD_SELECTION['drive'],
                      'ship_head_selection': HEAD_SELECTION['ship']},
        assumptions=[
            'Fixture from track_destination (real Unit/Drive/Ship vtables, 32x32 original Cell table, Rules at EXTRA+0x10000 with BlockagePathDelay 22) plus unit_entry owner prestate (0xA8E9A0=1, House indices). Actor Cell 10,10 centre, no paid head, no NavQueue, no TarCom, no radio contact, body facing 0 (octant 0).',
            'Rules PathDelay double 0.01 (9 frames), CloseEnough 128 unless the row sets it. MovementZone 0 zone lookup [1,2] over zeroed records; far_zone assigns record index 1 (zone 2) to the listed cells. Map width/height and bounds are the inherited fixture values.',
            'Setter at frame 100; Process at frame 101 (timer +640 expired). Foot+64C supplied after the setter (10 unless the row sets it). Mission Guard(5) unless set: Move 2, Enter 7, AreaGuard 11. House+1EC human byte per row; GameMode is the fixture default (0).',
            'The fixture path prestate 2,3,4,5 survives the setter only for Enter without a radio contact (0x741C54..0x741C78); empty_path supplies Foot+5E0=-1 after it to reach the continuation.',
            'post_warp rows: no setter; Foot+5E0 live word -1, NavCom null, Foot+64C 10, then the original Drive 0x4B0C40 / Ship 0x6A0310 Force_Track(-1, Actor+9C) before the Process. House+34 names a supplied country type whose +0x12C speed bonus is the float 1.0 (read by HouseClass::GetSpeedBonus 0x50C050).',
        ],
        substitutions=[
            'found/failed rows: Find_Path 0x4D3920 returns AL=1 after writing the route (then -1 words) to Foot+5E0, or AL=0 with no writes; original stdcall 12 cleanup.',
            'post_warp rows: the AStar core returns EAX=0 as AStar_pathfind_search does for a goal in its start cell (0x429BF3..0x429C0A -> 0x42A451, read). Their events also record calls of Random 0x65C780 and RandomRanged 0x65C7E0.',
            'native rows: only the AStar core 0x4CBBA0 is supplied (stdcall 24 cleanup): EAX=0, or a PathType in the shape of AStar_reconstruct_path 0x42AA90 (moves then -1 in the wrapper buffer, cost = moves, length = moves + 1); the Find_Path wrapper (copy, Mark, timers, Unit vt+540 = 0x41C140 no-op), Unit receivers, setters and continuation execute.',
            'Only OS Interlocked imports inherited from the fixture.',
        ]))
