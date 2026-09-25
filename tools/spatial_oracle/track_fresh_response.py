"""Original Drive/Ship Process_Movement fresh arm: the response bodies.

Run python -m tools.spatial_oracle.track_fresh_response --check (or --write).
The real Unit Cell setter (0x741970) orders the actor from Cell 10,10 to its
destination; the route words are then supplied at Foot+5E0 and the body
facing rests on the first word's octant. The real outer Process runs, then
Process_Movement(&out, 1, 0) with every recursion, stopping where it returns
to the outer Process (Drive 0x4B0A7E / Ship 0x6A0147), before Process_Track.

Supplied: the Unit Can_Enter_Cell answers (0x73F0A0, one per call, in call
order) and Find_Path (0x4D3920: a found route written to Foot+5E0 with AL=1,
or AL=0 with no writes). Observed and returned at entry: Cell Scatter_Objects
(0x481670) and Foot::Override_Mission (0x4D8F40). Everything else runs the
original bytes: the prologue gates, the gate question 0x578AD0 and
0x452540, Mark, the land speed row and slope products, Unit+534, the crate
question, Find_Blocking_Object, the zone precheck 0x4D3810, the Unit setter
and its locomotor Stop, the turn table and the finalize tail with
Apply_Track_Occupation_Mode.
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
RETURNED = {'drive': 0x4B0A7E, 'ship': 0x6A0147}
CAN_ENTER, FIND_PATH, SCATTER, OVERRIDE = 0x73F0A0, 0x4D3920, 0x481670, 0x4D8F40
ASTAR = 0x4CBBA0
RULES = EXTRA + 0x10000
ZONE_RECORDS, ZONE_TABLE = EXTRA + 0x2C000, EXTRA + 0x2D000
CELL_VTABLE = 0x7E4EEC
PROCESS_FRAME = 101
SLOPES = (0.875, 1.25, 0.625, 1.375)


def cell(x, y):
    return CELLS + (y * 32 + x) * 0x200


def query(case):
    family = case['family']
    u, call, read32 = make_destination_fixture(dict(family=family, head=[0, 0, 0],
                                                    mission=case.get('mission', 2),
                                                    cells=case.get('cells', [])))
    # track_destination rewrites its entry Cell (11,10) +140 after the
    # source fixture's per-cell flags; restore the row's.
    for x, y, _level, flags in case.get('cells', []):
        u.mem_write(cell(x, y) + 0x140, dwords(flags))
    # The original startup fills the lepton direction table (0x89F6D8).
    call(0x49F3A0, 0, [])
    # Health (Object+6C) over Strength (Type+A0) for GetHealthRatio 0x5F5C60.
    u.mem_write(TYPE + 0xA0, dwords(case.get('strength', 300)))
    u.mem_write(ACTOR + 0x6C, dwords(case.get('health', 300)))
    u.mem_write(ACTOR + 0x90, b'\x01')  # IsAlive, read after the crate question
    # unit_entry's owner/house prestate for the live Unit readers.
    u.mem_write(0xA8E9A0, b'\x01')
    for address, index in ((HOUSE, 0), (ENEMY, 1)):
        u.mem_write(address + 0x30, dwords(index))
    u.mem_write(HOUSE + 0x1EC, bytes([case.get('human', True)]))
    u.mem_write(RULES + 0x1700, struct.pack('<d', 0.5))
    u.mem_write(RULES + 0x1718, dwords(case.get('close_enough', 576)))
    u.mem_write(RULES + 0x1760, struct.pack('<d', 0.01))
    # TrackedUphill/Downhill, WheeledUphill/Downhill (Rules+0x768..+0x780).
    u.mem_write(RULES + 0x768, struct.pack('<4d', *SLOPES))
    u.mem_write(TYPE + 0x67C, dwords(case.get('speed_type', 1)))
    # The ground-Z evaluator 0x47B3A0 lazily caches this level height.
    u.mem_write(0x89E7C0, dwords(104))
    # MovementZone 0 zone lookup (Map+18) over per-cell records (Map+68/+6C).
    stride = read32(MAP + 0xF8) + 1 + read32(MAP + 0xF4)
    count = stride * 33
    u.mem_write(ZONE_RECORDS, bytes(count * 4))
    u.mem_write(ZONE_TABLE, struct.pack('<HH', 1, 2))
    u.mem_write(MAP + 0x68, dwords(ZONE_RECORDS, count))
    u.mem_write(MAP + 0x18, dwords(ZONE_TABLE))
    for x, y in case.get('far_zone', []):
        u.mem_write(ZONE_RECORDS + (stride * y + x) * 4 + 2, struct.pack('<H', 1))
    # Land speed rows (Clear, Road) and the supplied overlays.
    u.mem_write(0x89EA40, struct.pack('<18f', *([case.get('clear_speed', 1.0)] * 9
                                                + [case.get('road_speed', 0.75)] * 9)))
    overlays = {}
    for x, y, crushable, wall in case.get('overlays', []):
        index = 5 + len(overlays)
        overlay_type = EXTRA + 0x28000 + len(overlays) * 0x400
        u.mem_write(overlay_type + 0x22D, bytes([crushable]))
        u.mem_write(overlay_type + 0x2A8, bytes([wall]))
        overlays[index] = overlay_type
        u.mem_write(cell(x, y) + 0x44, dwords(index))
    if overlays:
        table = EXTRA + 0x2F800
        u.mem_write(table, dwords(*[overlays.get(i, 0) for i in range(max(overlays) + 1)]))
        u.mem_write(0xA83D84, dwords(table))
    for x, y in case.get('tunnel', []):
        u.mem_write(cell(x, y) + 0xEC, dwords(10))
    u.mem_write(TYPE + 0x5B4, dwords(case.get('movement_zone', 0)))
    u.mem_write(TYPE + 0xD28, bytes([case.get('crusher', False)]))
    destination = case.get('destination', [13, 10])
    call(0x741970, ACTOR, [cell(*destination), 1])
    route = case['route']
    u.mem_write(ACTOR + 0x5E0, dwords(*route, *([-1] * (24 - len(route)))))
    # The body rests on the first word's octant unless the row turns it.
    facing = ACTOR + 0x388
    rest = case.get('facing', (route[0] & 7) << 13 if route and route[0] >= 0 else 0)
    u.mem_write(facing, dwords(rest))
    u.mem_write(facing + 4, dwords(rest))
    u.mem_write(facing + 8, dwords(-1, 0, 0))
    u.mem_write(ACTOR + 0x640, dwords(*case.get('movement_timer', [100, 0, 0])))
    u.mem_write(ACTOR + 0x668, dwords(*case.get('blocked_timer', [100, 0, 22])))
    u.mem_write(ACTOR + 0x6B7, bytes([case.get('latched', False)]))
    u.mem_write(ACTOR + 0x64C, dwords(case.get('retries', 10)))
    # FootClass ParalysisTimer (+6A0 start, +6A8 duration), read by 0x4DE770.
    if 'paralysis' in case:
        start, duration = case['paralysis']
        u.mem_write(ACTOR + 0x6A0, dwords(start, 0, duration))
    # A retained selector with the valid byte clear: no active-track dispatch.
    if 'selector' in case:
        u.mem_write(LOCO + 0x58, dwords(case['selector']))
    if 'z' in case:
        u.mem_write(ACTOR + 0xA4, dwords(case['z']))

    events = []
    answers = list(case.get('codes', []))
    paths = list(case.get('find_path', []))
    cores = []

    def ret(cleanup, value):
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value)
        u.reg_write(UC_X86_REG_EIP, read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    def coord_of(pointer):
        return list(struct.unpack('<hh', u.mem_read(pointer + 0x24, 4)))

    def observe(_u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address == CAN_ENTER:
            args = [read32(sp + 4 + i * 4) for i in range(5)]
            assert answers, ('unsupplied Can_Enter_Cell', case)
            code = answers.pop(0)
            events.append(['can_enter', coord_of(args[0]), args[1],
                           struct.unpack('<i', dwords(args[2]))[0], args[3], args[4], code,
                           hex(read32(sp))])
            ret(20, code)
        elif address == FIND_PATH:
            packed = read32(sp + 4)
            assert paths, ('unsupplied Find_Path', case)
            result = paths.pop(0)
            events.append(['find_path', packed & 0xFFFF, packed >> 16,
                           read32(sp + 8), read32(sp + 12), result if isinstance(result, str) else 'found'])
            if result == 'core_null':
                # The original wrapper runs; only its AStar core answers NULL.
                cores.append(result)
                return
            if isinstance(result, list):
                u.mem_write(ACTOR + 0x5E0, dwords(*result, *([-1] * (24 - len(result)))))
            ret(12, int(isinstance(result, list)))
        elif address == SCATTER:
            args = [read32(sp + 4 + i * 4) for i in range(4)]
            events.append(['scatter', coord_of(u.reg_read(UC_X86_REG_ECX)), args[1], args[2],
                           args[3] & 0xFF])
            ret(16, 0)
        elif address == OVERRIDE:
            mission, target, dest = (read32(sp + 4 + i * 4) for i in range(3))
            named = coord_of(target) if read32(target) == CELL_VTABLE else target
            events.append(['override', mission, named, dest])
            ret(12, 0)
        elif address == ASTAR:
            assert cores, ('AStar without a core_null answer', case)
            cores.pop()
            events.append('astar_null')
            ret(24, 0)
        elif address == 0x4D55C0:
            events.append('failed_receiver')
        elif address == 0x741970:
            events.append(['unit_destination', read32(sp + 4)])
        elif address == 0x578AD0:
            events.append('gate')

    u.hook_add(UC_HOOK_CODE, observe)
    u.mem_write(0xA8ED84, dwords(case.get('frame', PROCESS_FRAME)))
    entry = read32(read32(LOCO + 4) + 0x40)
    assert entry == PROCESS[family]
    u.mem_write(SP, dwords(RET_MAGIC, LOCO + 4))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ECX, 0)
    stop = run_checked(u, entry, (RETURNED[family], RET_MAGIC), count=4000000,
                       required_addresses=[entry])
    assert not answers and not paths and not cores, (case, answers, paths, cores)
    signed = lambda address, n: list(struct.unpack('<' + 'i' * n, u.mem_read(address, n * 4)))
    nav = read32(ACTOR + 0x5A4)
    state = dict(
        destination=signed(LOCO + 0x34, 3), head=signed(LOCO + 0x40, 3),
        valid=u.mem_read(LOCO + 0x63, 1)[0], selector=signed(LOCO + 0x58, 1)[0],
        straight=u.mem_read(LOCO + 0x64, 1)[0],
        target_speed=struct.unpack('<d', u.mem_read(LOCO + 0x50, 8))[0],
        applied_speed=struct.unpack('<d', u.mem_read(ACTOR + 0x578, 8))[0],
        nav=coord_of(nav) if nav else None, path=signed(ACTOR + 0x5E0, 6),
        reference=list(struct.unpack('<hh', u.mem_read(ACTOR + 0x558, 4))),
        movement_timer=[read32(ACTOR + 0x640), read32(ACTOR + 0x648)],
        blocked_timer=[read32(ACTOR + 0x668), read32(ACTOR + 0x670)],
        latched=u.mem_read(ACTOR + 0x6B7, 1)[0], retries=read32(ACTOR + 0x64C),
        facing=struct.unpack('<H', u.mem_read(ACTOR + 0x388, 2))[0],
        mission=signed(ACTOR + 0xAC, 1)[0], foot_68b=u.mem_read(ACTOR + 0x68B, 1)[0],
        returned=stop == RETURNED[family])
    if state['returned']:
        state['out'] = u.mem_read(u.reg_read(UC_X86_REG_ESP) + 0x24, 1)[0]
    return dict(input=case, events=events, state=state)


def generate():
    rows = []
    east = [2, 2, 2]
    turn = [2, 1, 2]
    for family in PROCESS:
        base = dict(family=family)
        # The facing gate turns the body and admits nothing.
        rows.append(dict(base, route=east, facing=0))
        # Code 0: a straight track and a turning pair.
        rows.append(dict(base, route=east, codes=[0]))
        rows.append(dict(base, route=turn, codes=[0, 0]))
        rows.append(dict(base, route=turn, codes=[0, 3]))
        # Second-candidate retries recurse through the no-queue arm.
        for second in (1, 6, 7):
            rows.append(dict(base, route=turn, codes=[0, second, 0, 0], find_path=[east]))
        rows.append(dict(base, route=turn, codes=[0, 2, 0]))
        rows.append(dict(base, route=turn, codes=[0, 4, 0, 0], find_path=[east]))
        # Second-candidate code 6 without a retry: the CloseEnough stop and the
        # scatter, reached from a first-candidate redraw retry.
        rows.append(dict(base, route=east, codes=[1, 0, 0, 6], find_path=[turn],
                         destination=[11, 10], close_enough=1024))
        rows.append(dict(base, route=east, codes=[1, 0, 0, 6], find_path=[turn]))
        # The last path word: extension, its failure, and a near destination.
        rows.append(dict(base, route=[2], codes=[0], destination=[14, 10], find_path=[[2, 2]]))
        rows.append(dict(base, route=[2], codes=[0], destination=[14, 10], find_path=['failed']))
        rows.append(dict(base, route=[2], codes=[0], destination=[11, 10]))
        # A crushable overlay under the candidate or the next cell goes straight.
        rows.append(dict(base, route=turn, codes=[0], overlays=[[11, 10, True, False]]))
        rows.append(dict(base, route=turn, codes=[0], overlays=[[12, 9, True, False]]))
        # Code 2: the latch, timers, urgency and both Find_Path answers.
        for latched, blocked, movement, found in (
                (False, [100, 0, 22], [100, 0, 0], [east]),
                (True, [50, 0, 22], [100, 0, 0], [east]),
                (True, [90, 0, 22], [100, 0, 0], ['failed']),
                (False, [100, 0, 22], [100, 0, 9], []),
        ):
            rows.append(dict(base, route=east, codes=[2], latched=latched,
                             blocked_timer=blocked, movement_timer=movement, find_path=found))
        rows.append(dict(base, route=east, codes=[2], find_path=['failed'],
                         far_zone=[[13, 10]]))
        # Code 3: the gate tail.
        rows.append(dict(base, route=east, codes=[3]))
        # The first-candidate retries reach the no-queue arm and the fresh arm again.
        for code in (1, 4, 5, 6, 7):
            rows.append(dict(base, route=east, codes=[code, 0, code], find_path=[east]))
        # Without a retry: code 6's CloseEnough stop, and its scatter off a Tunnel.
        rows.append(dict(base, route=east, codes=[6, 0, 6], find_path=[east],
                         destination=[11, 10], close_enough=1024))
        rows.append(dict(base, route=east, codes=[6, 0, 6], find_path=[east],
                         destination=[11, 10], close_enough=1024, tunnel=[[10, 10]]))
        # Code 5 against a wall overlay: Override(Attack, cell).
        rows.append(dict(base, route=east, codes=[5, 0, 5], find_path=[east],
                         overlays=[[11, 10, False, True]]))
        # The accepted target speed: slopes by SpeedType (Track 1, Wheel 2),
        # the Road row two levels off, the zero row, the clamp and the damage
        # factor at and below ConditionYellow.
        up, down = dict(cells=[[11, 10, 1, 0]]), dict(cells=[[10, 10, 1, 0]], z=104)
        for speed_type in (1, 2):
            rows.append(dict(base, route=east, codes=[0], speed_type=speed_type, **up))
            rows.append(dict(base, route=east, codes=[0], speed_type=speed_type, **down))
        rows.append(dict(base, route=east, codes=[0], cells=[[11, 10, 2, 0]]))
        rows.append(dict(base, route=east, codes=[0], clear_speed=0.0))
        rows.append(dict(base, route=east, codes=[0], clear_speed=0.0, **down))
        rows.append(dict(base, route=east, codes=[0], clear_speed=1.25, **down))
        rows.append(dict(base, route=east, codes=[0], health=100))
        rows.append(dict(base, route=east, codes=[0], health=150))
        rows.append(dict(base, route=east, codes=[0], clear_speed=0.0, health=100, **down))
        # Find_Path's own core failure: the wrapper arms PathDelay and calls the
        # Unit receiver, whose locomotor Stop nulls +34 before the +2CC recheck.
        # The wrapper's goal admission (0x4D3A92) asks Can_Enter_Cell first.
        rows.append(dict(base, route=east, codes=[2, 0], find_path=['core_null']))
        rows.append(dict(base, route=east, codes=[2, 0], latched=True,
                         blocked_timer=[50, 0, 22], find_path=['core_null']))
        rows.append(dict(base, route=[2], codes=[0, 0], destination=[14, 10],
                         find_path=['core_null']))
        # +58 is written before the second query (0x4B401D): a second-stage
        # retry's publish sees the new selector, not the retained one.
        rows.append(dict(base, route=turn, codes=[0, 2, 0], selector=0x41))
        # A running ParalysisTimer returns before the path word (0x4B2761), and
        # an expired one does not.
        rows.append(dict(base, route=east, paralysis=[100, 30]))
        rows.append(dict(base, route=east, codes=[0], paralysis=[50, 30]))
        # A bridge-flagged candidate against OnBridge 0 sets Foot+68B (0x4B3391).
        rows.append(dict(base, route=east, codes=[0], cells=[[11, 10, 0, 0x100]]))
    return [query(row) for row in rows]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original Unit Cell setter then Drive/Ship outer Process through Process_Movement(&out,1,0) and every recursion it makes, stopping where it returns to the outer Process: the facing gate, code-0 straight/turning/extension/straight-forcing acceptance with the finalize tail, second-candidate codes, the first-candidate code-2 latch/timer/urgency ladder, the code-3 tail, the retry recursions of codes 1/4/5/6/7 into the no-queue arm and the fresh arm again, the code-6 CloseEnough stop and scatter, and the wall Override.',
        entry_points={'unit_destination': 0x741970, 'can_enter': CAN_ENTER, 'find_path': FIND_PATH,
                      'scatter_objects': SCATTER, 'override_mission': OVERRIDE, **PROCESS,
                      'drive_returned': RETURNED['drive'], 'ship_returned': RETURNED['ship']},
        assumptions=[
            'Rules TrackedUphill 0.875, TrackedDownhill 1.25, WheeledUphill 0.625, WheeledDownhill 1.375; Type SpeedType (+67C) 1 (Track) unless the row sets 2 (Wheel). Supplied Cell levels are flat (no slope type) over the 104-lepton level height 0x89E7C0; the Foot z follows its Cell level.',
            'Fixture from track_destination (real Unit/Drive/Ship vtables, 32x32 original Cell table, Rules at EXTRA+0x10000 with BlockagePathDelay 22) plus unit_entry owner prestate. Actor Cell 10,10 centre, MovementZone 0 zone table, no NavQueue, TarCom or radio contact.',
            'Rules ConditionYellow 0.5, CloseEnough 576 unless the row sets it, PathDelay 0.01 (9 frames). Land rows: Clear 1.0 and Road 0.75 for every SpeedType unless set. Supplied overlays are indices 5.. with only +22D Crushable and +2A8 Wall.',
            'Setter at frame 100; Process at frame 101. The route words are written after the setter and the body FacingClass rests on the first word octant (timer -1) unless the row sets a facing. Foot+640/+668/+6B7/+64C are supplied after the setter.',
            'Can_Enter_Cell answers are supplied per call in call order; the recorded arguments are the caller\'s. Find_Path answers are supplied per call: a list is written to Foot+5E0 with AL=1, failed returns AL=0 with no writes (the destination survives), core_null runs the original wrapper with only the AStar core 0x4CBBA0 answering NULL (stdcall 24).',
        ],
        substitutions=[
            'Unit Can_Enter_Cell 0x73F0A0 (stdcall 20) and Foot Find_Path 0x4D3920 (stdcall 12) are supplied at entry, except core_null Find_Path calls, whose AStar core 0x4CBBA0 (stdcall 24) alone returns NULL.',
            'Cell Scatter_Objects 0x481670 (thiscall, 16) and Foot::Override_Mission 0x4D8F40 (thiscall, 12) are recorded and returned at entry; their bodies do not run.',
            'Only OS Interlocked imports inherited from the fixture.',
        ]))
