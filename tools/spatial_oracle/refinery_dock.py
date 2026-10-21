"""Original refinery dock: radio, Enter, deposit and Harvest continuation.

Run python -m tools.spatial_oracle.refinery_dock --check (or --write).

A War Miner (the real Unit vtable over a constructed Drive locomotor) and a
stock refinery (the real Building vtable over a supplied BuildingTypeClass
with DockUnload=/Refinery=, Foundation 4x3) sit on the 32x32 source-scatter
map. The refinery's NW cell is (6,9), so its pad is (9,10) and the miner's
default cell (10,10) is the art QueueingCell=4,1 east of it.

The107 legacy rows retain their supplied idle/readiness/bay/nearby replies.
Additional controls and retained histories execute the original selection,
radio, idle, readiness, Enter/Harvest/Unload and dispatcher, with stock input
receipts. Arrival and scene construction remain supplied. See refinery_dock.md
and the metadata for per-corpus seams; animation/redraw sinks remain observed.
"""
import hashlib
import os
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX,
                               UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_ESI,
                               UC_X86_REG_EDI, UC_X86_REG_EBP, UC_X86_REG_EBX,
                               UC_X86_REG_FPCW)
from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.track_destination import make_destination_fixture, ACTOR, LOCO
from tools.spatial_oracle.unit_entry import EXTRA, HOUSE
from tools.spatial_oracle.unit_scatter_state import TYPE, SP
from tools.spatial_oracle.unit_source_scatter import CELLS, SCENARIO

BLD, BTYPE, OTHER = EXTRA + 0x18000, EXTRA + 0x1A000, EXTRA + 0x1C000
MINER_ITEMS, BLD_ITEMS, OTHER_ITEMS = EXTRA + 0x2F000, EXTRA + 0x2F100, EXTRA + 0x2F200
TIB_ITEMS, TIBERIUMS = EXTRA + 0x2F300, EXTRA + 0x2F400
HTYPE, DOCK_ITEMS, AI_ITEMS = EXTRA + 0x2E000, EXTRA + 0x2E400, EXTRA + 0x2E800
RULES = EXTRA + 0x10000
NW = (6, 9)
PAD = (9, 10)
TRANSMIT = 0x65A970
TRANSMIT_RETURNS = (0x65A990, 0x65A9E5, 0x65AA66, 0x65AA72)
SCATTER, IDLE, READY = 0x743A50, 0x738970, 0x744270
PLAY_ANIM, DESTROY_ANIM, SMOKE = 0x451750, 0x451E40, 0x459900
DO_TURN, ASSIGN, QUEUE, COMMENCE, RANDOM = 0x4B0EF0, 0x741970, 0x5B35E0, 0x5B3570, 0x65C7E0
FIND_DOCKING_BAY, FNPC, GIVE_TIBERIUM = 0x4DF040, 0x56DC20, 0x4F9610
MISSION = {'guard': 5, 'enter': 7, 'harvest': 10, 'return': 12, 'unload': 16, 'selling': 19}
RATES = {5: 0.030, 7: 0.016, 10: 0.016, 16: 0.016}


def cell(x, y):
    return CELLS + (y * 32 + x) * 0x200


def cell_xy(pointer):
    if pointer == 0:
        return None
    offset = pointer - CELLS
    assert 0 <= offset < 32 * 32 * 0x200 and offset % 0x200 == 0, hex(pointer)
    return [offset // 0x200 % 32, offset // 0x200 // 32]


def name_of(pointer):
    return {ACTOR: 'miner', BLD: 'refinery', OTHER: 'other', 0: None}.get(pointer, hex(pointer))


def nav_target(pointer):
    return name_of(pointer) if pointer in (BLD, OTHER, ACTOR) else cell_xy(pointer)


def place_building(u, building, nw, *, constructor_abstract_flags=None):
    # Supplied admitted-state helper, not a constructor or Mark port. Legacy
    # partial objects keep flags1. A whole-ctor caller passes its actual
    # retained+14 instead: Object5F3B37 OR2 then Techno6F322F OR1. Overwriting
    # that datum would suppress original72592D..38 expiry dispatch.
    # Primary and RTTI (What_Am_I) vtables, as the constructor writes them (0x43B725).
    u.mem_write(building, dwords(0x7E3EBC, 0x7E3EA0))
    u.mem_write(building + 0x14, dwords(1 if constructor_abstract_flags is None
                                     else constructor_abstract_flags))
    u.mem_write(building + 0x9C, dwords(nw[0] * 256 + 128, nw[1] * 256 + 128, 0))
    u.mem_write(building + 0xB4, dwords(-1))
    u.mem_write(building + 0x21C, dwords(HOUSE))
    u.mem_write(building + 0x520, dwords(BTYPE))
    u.mem_write(building + 0x660, b'\x01')


def make_dock_fixture(case):
    u, call, read32 = make_destination_fixture(dict(family='drive', head=[0, 0, 0], prior=[0, 0, 0],
                                                    seed=case.get('seed', 1)))
    # Miner: a stopped War Miner at its case cell, on the case mission.
    x, y = case.get('miner_cell', [10, 10])
    u.mem_write(ACTOR + 0x9C, dwords(x * 256 + 128, y * 256 + 128, 0))
    u.mem_write(ACTOR + 0x6C, dwords(1000))
    u.mem_write(ACTOR + 0xAC, dwords(MISSION[case.get('mission', 'enter')]))
    queued = case.get('queued')
    u.mem_write(ACTOR + 0xB4, dwords(MISSION[queued] if queued else -1))
    u.mem_write(ACTOR + 0x418, bytes([case.get('miner_tether', False)]))
    u.mem_write(ACTOR + 0x6AF, bytes([case.get('turret_latch', False)]))
    u.mem_write(TYPE + 0xE0E, bytes([case.get('harvester', True)]))  # Harvester=
    if case.get('weeder'):
        u.mem_write(TYPE + 0xE0F, b'\x01')  # Weeder=
    nav = case.get('nav')
    u.mem_write(ACTOR + 0x5A4, dwords({'refinery': BLD, 'other': OTHER}.get(nav, 0)
                                    if isinstance(nav, str) else cell(*nav) if nav else 0))
    if case.get('moving'):
        u.mem_write(LOCO + 0x34, dwords((nav[0] if nav else x) * 256 + 128,
                                        (nav[1] if nav else y) * 256 + 128, 0))
        u.mem_write(LOCO + 0x40, dwords(x * 256 + 128, y * 256 + 128, 0))
    # PrimaryFacing: the original constructor, SetROT(5) and a settled raw value.
    call(0x4C91C0, ACTOR + 0x388, [])
    call(0x4C9680, ACTOR + 0x388, [5])
    raw = case.get('facing', 0xC000)
    u.mem_write(ACTOR + 0x388, dwords(raw, raw, -1, 0, 0))
    # Refinery: Building vtable, 4x3 foundation (index 12), DockUnload/Refinery.
    place_building(u, BLD, NW)
    u.mem_write(BLD + 0x6C, dwords(case.get('refinery_health', 900)))
    u.mem_write(BLD + 0xAC, dwords(MISSION[case.get('refinery_mission', 'guard')]))
    u.mem_write(BLD + 0xE0, dwords(0x7E180C, BLD_ITEMS, 1))
    u.mem_write(BLD + 0x418, bytes([case.get('refinery_tether', False)]))
    u.mem_write(BLD + 0x660, bytes([case.get('online', True)]))
    u.mem_write(BLD + 0x57C, dwords(case.get('production_anim', 0)))
    u.mem_write(BLD + 0x584, dwords(case.get('special_anim', 0)))
    u.mem_write(BTYPE + 0xA0, dwords(900))
    u.mem_write(BTYPE + 0xEF0, dwords(12))
    u.mem_write(BTYPE + 0x1618, dwords(4, 1))  # art QueueingCell=4,1 (ReadMinMax)
    u.mem_write(BTYPE + 0x16B3, bytes([case.get('dock_unload', True)]))
    u.mem_write(BTYPE + 0x16BB, bytes([case.get('refinery_flag', True)]))  # Refinery=
    if case.get('weeder_dock'):
        u.mem_write(BTYPE + 0x16BC, b'\x01')  # Weeder=
    u.mem_write(BTYPE + 0x1780, dwords(1))
    # A second refinery elsewhere, holding whatever slot the row gives it.
    place_building(u, OTHER, (20, 20))
    u.mem_write(OTHER + 0x6C, dwords(900))
    u.mem_write(OTHER + 0xAC, dwords(MISSION['guard']))
    u.mem_write(OTHER + 0xE0, dwords(0x7E180C, OTHER_ITEMS, 1))
    u.mem_write(OTHER_ITEMS, dwords({'miner': ACTOR}.get(case.get('other_contact'), 0)))
    # Contacts: linked both ways unless the row says otherwise.
    linked = case.get('linked', True)
    named = {'other': OTHER, 'miner': ACTOR, 'refinery': BLD, None: 0}
    u.mem_write(MINER_ITEMS, dwords(BLD if linked else named[case.get('miner_contact')]))
    u.mem_write(BLD_ITEMS, dwords(ACTOR if linked else named[case.get('refinery_contact')]))
    # Every 4x3 foundation cell, the pad included, lists the refinery as its
    # first object, as MapClass::Place_Down (0x5683C0) leaves it: Occupy_Down
    # (0x47E8A0) for each offset of the type's Occupy_List (BuildingType+0xDFC,
    # the 12-cell 4x3 list 0x45B1C0 builds; art AddOccupy/RemoveOccupy feed
    # only Cell+0x100). Unload's west-cell lookup and Per_Cell_Process's
    # north-cell lookup (0x47C520) find it there.
    u.mem_write(0xA8E9A0, b'\x01')
    if case.get('west_building', True):
        for fy in range(NW[1], NW[1] + 3):
            for fx in range(NW[0], NW[0] + 4):
                u.mem_write(cell(fx, fy) + 0xE4, dwords(BLD))
    # Harvester storage (Unit+0x33C float[4]) and the Tiberium Value table.
    u.mem_write(ACTOR + 0x33C, struct.pack('<4f', *case.get('storage', [0, 0, 0, 0])))
    u.mem_write(0xB0F4EC, dwords(TIB_ITEMS))
    for index, value in enumerate((25, 50, 25, 25)):
        u.mem_write(TIB_ITEMS + index * 4, dwords(TIBERIUMS + index * 0x100))
        u.mem_write(TIBERIUMS + index * 0x100 + 0xB8, dwords(value))
    # House economy: Balance, Score, IncomeMult, purifiers, human/AI, mode.
    u.mem_write(HOUSE + 0x30C, dwords(case.get('balance', 0)))
    u.mem_write(HOUSE + 0x54E8, dwords(0))
    u.mem_write(HOUSE + 0x34, dwords(HTYPE))
    u.mem_write(HTYPE + 0x148, struct.pack('<f', case.get('income_mult', 1.0)))
    u.mem_write(HOUSE + 0x538C, dwords(case.get('purifiers', 0)))
    u.mem_write(HOUSE + 0x1EC, bytes([case.get('human', True)]))
    u.mem_write(HOUSE + 0x184, dwords(case.get('difficulty', 0)))
    u.mem_write(0xA8B238, dwords(case.get('game_mode', 1)))
    u.mem_write(RULES + 0x1320 + 4, dwords(AI_ITEMS))
    u.mem_write(AI_ITEMS, dwords(4, 2, 0))
    # Rules: HarvesterTooFarDistance/ChronoHarvTooFarDistance, PurifierBonus,
    # HarvesterDumpRate and ConditionYellow at their retail/constructor values.
    u.mem_write(RULES + 0xD78, dwords(5, 50))
    u.mem_write(RULES + 0xF3C, struct.pack('<f', 0.25))
    u.mem_write(RULES + 0x1528, struct.pack('<d', 0.016))
    u.mem_write(RULES + 0x1700, struct.pack('<d', 0.5))
    # The type's Dock= list names this refinery type; the House owns one.
    u.mem_write(TYPE + 0x3EC, dwords(DOCK_ITEMS))
    u.mem_write(TYPE + 0x3F8, dwords(1))
    u.mem_write(DOCK_ITEMS, dwords(BTYPE))
    u.mem_write(BTYPE + 0xDF8, dwords(0))
    # House+0x5500 per-BuildingType owned counter (vector: items +4, size +8).
    u.mem_write(HOUSE + 0x5500, dwords(0, AI_ITEMS + 0x100, 4))
    u.mem_write(AI_ITEMS + 0x100, dwords(1, 0, 0, 0))
    # NavQueue (+0x588 vector: items +0x58C, capacity +0x590, count +0x598).
    if case.get('nav_queue'):
        items = EXTRA + 0x2C000
        u.mem_write(items, dwords(*[cell(*c) for c in case['nav_queue']]))
        u.mem_write(ACTOR + 0x58C, dwords(items, len(case['nav_queue'])))
        u.mem_write(ACTOR + 0x598, dwords(len(case['nav_queue'])))
    # Harvest/Unload state: Status, the +0x6D1 latch and the +0xF8 StageClass.
    u.mem_write(ACTOR + 0xBC, dwords(case.get('status', 0)))
    u.mem_write(ACTOR + 0x6D1, bytes([case.get('unloading', False)]))
    stage = case.get('stage', [0, 0, -1, 0, 0])
    u.mem_write(ACTOR + 0xF8, dwords(stage[0]))
    u.mem_write(ACTOR + 0x100, dwords(stage[2], 0, stage[3], stage[4], 1))
    # Retail [Guard]/[Enter]/[Harvest]/[Unload] Rate (MissionControl +0x10).
    for mission, rate in RATES.items():
        u.mem_write(0xA8E3A8 + mission * 32 + 0x10, struct.pack('<d', rate))
    u.mem_write(0xA8ED84, dwords(case.get('frame', 200)))
    return u, call, read32


def observe_dock(u, read32, case):
    events, pending, draws = [], [], {}
    ready = list(case.get('ready', []))
    bays = list(case.get('bays', []))
    passable = list(case.get('passable', []))

    def ret(cleanup, value=0):
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value)
        u.reg_write(UC_X86_REG_EIP, read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    def observe(_u, address, _size, _data):
        from unicorn.x86_const import UC_X86_REG_ECX
        sp = u.reg_read(UC_X86_REG_ESP)
        this = u.reg_read(UC_X86_REG_ECX)
        if address in draws:
            events[draws.pop(address)].append(u.reg_read(UC_X86_REG_EAX))
        if address == TRANSMIT:
            msg, target = read32(sp + 4), read32(sp + 12)
            if target == 0:
                target = read32(read32(this + 0xE4))
            pending.append(len(events))
            events.append(['send', name_of(this), msg, name_of(target), None])
        elif address in TRANSMIT_RETURNS and pending:
            events[pending.pop()][4] = u.reg_read(UC_X86_REG_EAX)
        elif address == ASSIGN:
            events.append(['assign_destination', name_of(this), nav_target(read32(sp + 4)), read32(sp + 8)])
        elif address == DO_TURN:
            events.append(['do_turn', read32(sp + 8) & 0xFFFF])
        elif address == QUEUE:
            events.append(['queue', name_of(this), read32(sp + 4), read32(sp + 8) & 0xFF])
        elif address == COMMENCE:
            events.append(['commence', name_of(this)])
        elif address == RANDOM:
            draws[read32(sp)] = len(events)
            events.append(['random', read32(sp + 4), read32(sp + 8)])
        elif address == SCATTER:
            events.append(['scatter', name_of(this), read32(sp + 8), read32(sp + 12)])
            if not case.get('native_scatter', False):
                ret(12)
        elif address == IDLE:
            events.append(['enter_idle_mode', read32(sp + 4), read32(sp + 8)])
            if not case.get('native_idle', False):
                ret(8)
        elif address == READY:
            if case.get('native_ready', False):
                events.append(['ready_original'])
                return
            assert ready, ('unsupplied Ready_To_Commence', case)
            answer = ready.pop(0)
            events.append(['ready', answer])
            ret(0, answer)
        elif address == FIND_DOCKING_BAY:
            if case.get('native_bays', False):
                events.append(['find_docking_bay_original', read32(sp + 8), read32(sp + 12),
                               read32(0xA8E7AC)])
                return
            assert bays, ('unsupplied Find_Docking_Bay', case)
            bay = {'refinery': BLD, 'other': OTHER, None: 0}[bays.pop(0)]
            events.append(['find_docking_bay', read32(sp + 8), read32(sp + 12), read32(0xA8E7AC),
                           name_of(bay)])
            ret(12, bay)
        elif address == FNPC:
            out, query = read32(sp + 4), read32(sp + 8)
            seed = list(struct.unpack('<hh', u.mem_read(query, 4)))
            if case.get('native_passable', False):
                events.append(['nearby_passable_original', seed, read32(sp + 12)])
                return
            assert passable, ('unsupplied Find_Nearby_Passable_Cell', case)
            answer = passable.pop(0)
            events.append(['nearby_passable_cell', seed, read32(sp + 12), answer])
            u.mem_write(out, struct.pack('<hh', *(answer or (0, 0))))
            ret(0x3C, out)
        elif address == GIVE_TIBERIUM:
            events.append(['give_tiberium', struct.unpack('<f', u.mem_read(sp + 4, 4))[0],
                           read32(sp + 8)])
        elif address in (PLAY_ANIM, DESTROY_ANIM, SMOKE):
            events.append([{PLAY_ANIM: 'play_anim', DESTROY_ANIM: 'destroy_anim',
                            SMOKE: 'smoke'}[address], read32(sp + 4)])
            ret({PLAY_ANIM: 16, DESTROY_ANIM: 4, SMOKE: 0}[address])

    u.hook_add(UC_HOOK_CODE, observe)
    return events, (ready, bays, passable)


def state(u, read32):
    signed = lambda address: struct.unpack('<i', u.mem_read(address, 4))[0]
    return dict(
        miner_mission=signed(ACTOR + 0xAC), miner_queued=signed(ACTOR + 0xB4),
        miner_status=signed(ACTOR + 0xBC),
        miner_nav=nav_target(read32(ACTOR + 0x5A4)),
        miner_contact=name_of(read32(MINER_ITEMS)),
        refinery_contact=name_of(read32(BLD_ITEMS)),
        other_contact=name_of(read32(OTHER_ITEMS)),
        miner_tether=u.mem_read(ACTOR + 0x418, 1)[0],
        refinery_tether=u.mem_read(BLD + 0x418, 1)[0],
        facing=dict(desired=read32(ACTOR + 0x388) & 0xFFFF, start=read32(ACTOR + 0x38C) & 0xFFFF,
                    timer_start=signed(ACTOR + 0x390), duration=signed(ACTOR + 0x398)),
        refinery_queued=signed(BLD + 0xB4),
        refinery_bstate=[signed(BLD + 0x534), signed(BLD + 0x538)],
        unloading=u.mem_read(ACTOR + 0x6D1, 1)[0],
        stage=[signed(ACTOR + 0xF8), signed(ACTOR + 0x100), signed(ACTOR + 0x108),
               signed(ACTOR + 0x10C)],
        storage=list(struct.unpack('<4f', u.mem_read(ACTOR + 0x33C, 16))),
        balance=signed(HOUSE + 0x30C), score=signed(HOUSE + 0x54E8),
        dispatch_timer=[signed(ACTOR + 0xC8), signed(ACTOR + 0xD0)],
    )


def can_dock(case):
    """Building 0x0E from the miner, as Mission_Enter sends it."""
    u, call, read32 = make_dock_fixture(case)
    events, unused = observe_dock(u, read32, case)
    call(TRANSMIT, ACTOR, [0x0E, 0x00A8EC30, BLD])
    reply = u.reg_read(UC_X86_REG_EAX)
    assert not any(unused), (case, unused)
    return dict(input=case, reply=reply, events=events, state=state(u, read32))


def mission(case, entry):
    """One original mission handler dispatch on the miner (ecx=miner)."""
    u, call, read32 = make_dock_fixture(case)
    events, unused = observe_dock(u, read32, case)
    call(entry, ACTOR, [])
    delay = struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0]
    assert not any(unused), (case, unused)
    return dict(input=case, delay=delay, events=events, state=state(u, read32))


def per_cell(case):
    """Unit Per_Cell_Process(2)'s Enter arm, 0x73A31F..0x73A5EA, on the miner.

    The function prologue's locals are supplied: [esp+0x14] = Contact(0)
    (0x739F1D) and [esp+0x1C] = Get_Cell() (0x739ED3).
    """
    from unicorn.x86_const import UC_X86_REG_EBP
    from tools.native_oracle import run_checked
    u, call, read32 = make_dock_fixture(case)
    events, unused = observe_dock(u, read32, case)
    x, y = case.get('miner_cell', [10, 10])
    frame = SP - 0x100
    u.mem_write(frame + 0x14, dwords(read32(MINER_ITEMS)))
    u.mem_write(frame + 0x1C, struct.pack('<hh', x, y))
    u.reg_write(UC_X86_REG_EBP, ACTOR)
    u.reg_write(UC_X86_REG_ESP, frame)
    run_checked(u, 0x73A31F, 0x73A5EA, count=200000)
    assert not any(unused), (case, unused)
    return dict(input=case, events=events, state=state(u, read32))


def per_cell_release(case):
    """Unit Per_Cell_Process(2)'s Ready/Commence and its Refinery=/Weeder=
    contact release, 0x73ACB3..0x73ADCA, on the miner (no prologue local)."""
    from unicorn.x86_const import UC_X86_REG_EBP
    from tools.native_oracle import run_checked
    u, call, read32 = make_dock_fixture(case)
    events, unused = observe_dock(u, read32, case)
    u.reg_write(UC_X86_REG_EBP, ACTOR)
    u.reg_write(UC_X86_REG_ESP, SP - 0x100)
    run_checked(u, 0x73ACB3, 0x73ADCA, count=200000)
    assert not any(unused), (case, unused)
    return dict(input=case, events=events, state=state(u, read32))


def stage_tick(case):
    """TechnoClass::AI's StageClass step (0x6FABC4..0x6FAC31) over frames."""
    from unicorn.x86_const import UC_X86_REG_EBP, UC_X86_REG_ESI
    from tools.native_oracle import run_checked
    u, call, read32 = make_dock_fixture(case)
    values = []
    for frame in range(case['from_frame'], case['to_frame'] + 1):
        u.mem_write(0xA8ED84, dwords(frame))
        u.reg_write(UC_X86_REG_ESI, ACTOR)
        u.reg_write(UC_X86_REG_EBP, 0)
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, 0x6FABC4, 0x6FAC31, count=200)
        values.append([frame, struct.unpack('<i', u.mem_read(ACTOR + 0xF8, 4))[0],
                       u.mem_read(ACTOR + 0xFC, 1)[0]])
    return dict(input=case, values=values)


def radio(case):
    """One original transmit between the miner and the refinery."""
    u, call, read32 = make_dock_fixture(case)
    events, unused = observe_dock(u, read32, case)
    sender, target = {'miner': (ACTOR, BLD), 'refinery': (BLD, ACTOR)}[case['from']]
    call(TRANSMIT, sender, [case['msg'], 0x00A8EC30, target])
    reply = u.reg_read(UC_X86_REG_EAX)
    assert not any(unused), (case, unused)
    return dict(input=case, reply=reply, events=events, state=state(u, read32))


def radio_cases():
    cases = []
    # HELLO: fresh link, already linked, busy receiver, full sender (evicts).
    cases.append(dict(name='hello_fresh', msg=2, **{'from': 'miner'}, linked=False))
    cases.append(dict(name='hello_linked', msg=2, **{'from': 'miner'}))
    cases.append(dict(name='hello_busy', msg=2, **{'from': 'miner'}, linked=False,
                      refinery_contact='other'))
    cases.append(dict(name='hello_sender_full', msg=2, **{'from': 'miner'}, linked=False,
                      miner_contact='other', other_contact='miner'))
    cases.append(dict(name='hello_dead_receiver', msg=2, **{'from': 'miner'}, linked=False,
                      refinery_health=0))
    # OVER_OUT with and without both tethers, from either end.
    for sender in ('miner', 'refinery'):
        cases.append(dict(name=f'over_out_{sender}', msg=3, **{'from': sender}))
        cases.append(dict(name=f'over_out_{sender}_tethered', msg=3, **{'from': sender},
                          miner_tether=True, refinery_tether=True))
        cases.append(dict(name=f'over_out_{sender}_half_tether', msg=3, **{'from': sender},
                          miner_tether=True))
    # OVER_OUT on a miner returning to Return queues Guard.
    cases.append(dict(name='over_out_miner_on_return', msg=3, **{'from': 'refinery'}, mission='return'))
    # Not a contact: the receiver answers 0.
    cases.append(dict(name='over_out_unlinked', msg=3, **{'from': 'miner'}, linked=False))
    # 0x15 straight to the refinery (the Per_Cell_Process sender).
    cases.append(dict(name='dock_now', msg=0x15, **{'from': 'miner'}))
    cases.append(dict(name='dock_now_selling', msg=0x15, **{'from': 'miner'},
                      refinery_mission='selling'))
    # Tether ping-pong started by either end.
    cases.append(dict(name='tether_from_refinery', msg=0x18, **{'from': 'refinery'}))
    cases.append(dict(name='tether_from_miner', msg=0x18, **{'from': 'miner'}))
    cases.append(dict(name='untether', msg=0x19, **{'from': 'refinery'},
                      miner_tether=True, refinery_tether=True))
    return cases


def handshake_cases():
    cases = []
    # Off the pad, stopped: 0x13 answers 1, MOVE_HERE assigns the pad.
    cases.append(dict(name='off_pad_stopped'))
    # Driving to the pad: 0x13 answers 10, the pad NavCom forces MOVE_HERE.
    cases.append(dict(name='off_pad_driving_to_pad', nav=list(PAD), moving=True))
    # Driving elsewhere with a destination: force, and a re-assign to the pad.
    cases.append(dict(name='off_pad_driving_elsewhere', nav=[12, 12], moving=True))
    # A destination equal to the GetDockCoord cell NW+(2,1) does not force.
    cases.append(dict(name='nav_on_dock_coord', nav=[8, 10], moving=True))
    # On the pad facing west: tether ping-pong, then 0x16 turns the hull.
    cases.append(dict(name='on_pad_facing_west', miner_cell=list(PAD)))
    # On the pad, already East, stopped, tethered: 0x16 sends 0x15 (Unload).
    cases.append(dict(name='on_pad_facing_east_tethered', miner_cell=list(PAD), facing=0x4000,
                      miner_tether=True, refinery_tether=True))
    # East within the Unload window but not exactly 0x4000: still turns.
    cases.append(dict(name='on_pad_facing_near_east', miner_cell=list(PAD), facing=0x3F80,
                      miner_tether=True, refinery_tether=True))
    # Turret latch skips the facing test: 0x15 while still facing west.
    cases.append(dict(name='on_pad_turret_latch', miner_cell=list(PAD), turret_latch=True,
                      miner_tether=True, refinery_tether=True))
    # On the pad but still moving (track not ended): tethers, turns, no 0x15.
    cases.append(dict(name='on_pad_moving', miner_cell=list(PAD), nav=list(PAD), moving=True))
    cases.append(dict(name='on_pad_moving_east', miner_cell=list(PAD), nav=list(PAD), moving=True,
                      facing=0x4000))
    # Mission no longer Enter: tethered and stopped, but no 0x15.
    cases.append(dict(name='on_pad_not_enter', miner_cell=list(PAD), facing=0x4000,
                      mission='harvest', miner_tether=True, refinery_tether=True))
    # A refinery being sold refuses 0x15.
    cases.append(dict(name='on_pad_selling', miner_cell=list(PAD), facing=0x4000,
                      miner_tether=True, refinery_tether=True, refinery_mission='selling'))
    # Offline refinery answers 10 before anything else.
    cases.append(dict(name='offline', online=False))
    # Not linked, slot free: the refinery HELLOs the miner back.
    cases.append(dict(name='not_linked_free', linked=False))
    # Not linked, slot held by another object: no HELLO, 0x13, MOVE_HERE.
    cases.append(dict(name='not_linked_busy', linked=False, refinery_contact='other'))
    # Guard with nothing queued: MOVE_HERE queues Move before assigning.
    cases.append(dict(name='off_pad_guard', mission='guard'))
    # Queued Enter and ready: MOVE_HERE commences it.
    cases.append(dict(name='off_pad_queued_enter', mission='harvest', queued='enter', ready=[1]))
    cases.append(dict(name='off_pad_queued_enter_not_ready', mission='harvest', queued='enter',
                      ready=[0]))
    # No DockUnload=: the pad branch is skipped entirely.
    cases.append(dict(name='no_dock_unload', dock_unload=False))
    return cases


ENTER, HARVEST, UNLOAD = 0x4D9290, 0x73E5E0, 0x73D630


def enter_cases():
    return [
        dict(name='enter_off_pad'),
        dict(name='enter_on_pad_west', miner_cell=list(PAD)),
        dict(name='enter_on_pad_east_tethered', miner_cell=list(PAD), facing=0x4000,
             miner_tether=True, refinery_tether=True),
        dict(name='enter_offline', online=False),
        dict(name='enter_offline_tethered', online=False, miner_tether=True),
        dict(name='enter_no_target', linked=False),
        dict(name='enter_no_target_queued_guard', linked=False, queued='guard'),
        dict(name='enter_nav_queue', nav_queue=[[12, 12], [13, 13]], dock_unload=False),
        dict(name='enter_frame_seed', frame=4321, seed=7),
    ]


def harvest_cases():
    busy = dict(linked=False, refinery_contact='other')
    return [
        dict(name='return_driving', mission='harvest', status=2, nav=[12, 12], moving=True),
        dict(name='return_hello', mission='harvest', status=2, linked=False, bays=['refinery']),
        dict(name='return_hello_linked', mission='harvest', status=2, bays=['refinery']),
        # A refinery whose slot is taken fails the narrow pass's
        # Has_Free_Or_Own pre-filter (0x004DEF09); the wide pass finds it.
        dict(name='return_busy_close', mission='harvest', status=2, bays=[None, 'refinery'],
             **busy),
        dict(name='return_busy_beyond_0x300', mission='harvest', status=2, miner_cell=[11, 10],
             bays=[None, 'refinery'], passable=[[10, 10]], **busy),
        dict(name='return_too_far', mission='harvest', status=2, miner_cell=[16, 16],
             bays=['refinery', 'refinery'], passable=[[10, 10]], linked=False),
        dict(name='return_too_far_no_cell', mission='harvest', status=2, miner_cell=[16, 16],
             bays=['refinery', 'refinery'], passable=[None], linked=False),
        dict(name='return_no_bay', mission='harvest', status=2, bays=[None, None], linked=False),
        dict(name='handoff', mission='harvest', status=3),
    ]


def unload_cases():
    base = dict(mission='unload', miner_cell=list(PAD), facing=0x4000)
    dumping = dict(base, status=3, unloading=True, stage=[15, 0, 199, 1, 1])
    return [
        dict(base, name='unload_facing_west', facing=0xC000),
        dict(base, name='unload_facing_west_latch', facing=0xC000, turret_latch=True),
        dict(base, name='unload_window_low_edge', facing=0x3F80, storage=[40, 0, 0, 0]),
        dict(base, name='unload_window_outside', facing=0x3F7F),
        dict(base, name='unload_first_pass', storage=[40, 0, 0, 0]),
        dict(dumping, name='unload_below_gate', stage=[14, 0, 199, 1, 1], storage=[40, 0, 0, 0]),
        dict(dumping, name='unload_gate_ore', storage=[40, 0, 0, 0]),
        dict(dumping, name='unload_gate_ore_purifier', storage=[40, 0, 0, 0], purifiers=1),
        dict(dumping, name='unload_gate_partial_purifier', storage=[39, 0, 0, 0], purifiers=1),
        dict(dumping, name='unload_gate_mixed', storage=[5, 10, 0, 0], purifiers=2),
        dict(dumping, name='unload_gate_riparius_vinifera', storage=[5, 0, 5, 0]),
        dict(dumping, name='unload_gate_vinifera', storage=[0, 0, 5, 0]),
        dict(dumping, name='unload_gate_vinifera_aboreus', storage=[0, 0, 5, 5]),
        dict(dumping, name='unload_gate_aboreus', storage=[0, 0, 0, 5]),
        dict(dumping, name='unload_gate_gems', storage=[0, 10, 0, 0]),
        dict(dumping, name='unload_gate_ai_virtual', storage=[40, 0, 0, 0], human=False),
        dict(dumping, name='unload_gate_ai_campaign', storage=[40, 0, 0, 0], human=False,
             game_mode=0),
        dict(dumping, name='unload_gate_ai_easy', storage=[40, 0, 0, 0], human=False, difficulty=2),
        dict(dumping, name='unload_gate_income_mult', storage=[40, 0, 0, 0], income_mult=0.9),
        dict(dumping, name='unload_gate_income_mult_bonus', storage=[39, 0, 0, 0],
             income_mult=0.9, purifiers=1),
        dict(dumping, name='unload_gate_empty'),
        dict(dumping, name='unload_gate_empty_special_anim', special_anim=0x4321),
        dict(dumping, name='unload_gate_ore_special_anim', storage=[40, 0, 0, 0], special_anim=0x4321),
        dict(dumping, name='unload_missing_building', west_building=False, storage=[40, 0, 0, 0],
             ready=[0]),
        dict(dumping, name='unload_missing_building_below_gate', west_building=False,
             stage=[3, 0, 199, 1, 1], storage=[40, 0, 0, 0], ready=[0]),
        dict(dumping, name='unload_new_order', storage=[40, 0, 0, 0], nav=[12, 12], queued='guard'),
        dict(dumping, name='unload_new_order_harvest', storage=[40, 0, 0, 0], nav=[12, 12],
             queued='harvest'),
        dict(dumping, name='unload_new_order_below_gate', stage=[3, 0, 199, 1, 1],
             storage=[40, 0, 0, 0], nav=[12, 12], queued='guard'),
        dict(base, name='unload_state4_ready', status=4, unloading=True, ready=[1]),
        dict(base, name='unload_state4_not_ready', status=4, unloading=True, ready=[0]),
        dict(base, name='unload_state4_production_anim', status=4, unloading=True,
             production_anim=0x1234),
        dict(base, name='unload_state4_new_order', status=4, unloading=True, nav=[12, 12],
             queued='guard', moving=True, ready=[1]),
        dict(base, name='unload_contact_lost', status=3, unloading=True, linked=False, ready=[1]),
        dict(base, name='unload_contact_lost_not_ready', status=3, unloading=True, linked=False,
             ready=[0]),
    ]


def stage_cases():
    return [
        dict(name='rate_one', stage=[0, 0, 100, 1, 1], from_frame=100, to_frame=118),
        dict(name='rate_zero', stage=[5, 0, 100, 1, 0], from_frame=100, to_frame=104),
        dict(name='stopped_timer', stage=[3, 0, -1, 0, 1], from_frame=100, to_frame=103),
    ]


def per_cell_cases():
    pad = dict(miner_cell=list(PAD), miner_tether=True, refinery_tether=True)
    return [
        dict(pad, name='per_cell_pad_tethered'),
        dict(pad, name='per_cell_pad_untethered', miner_tether=False, refinery_tether=False),
        dict(pad, name='per_cell_pad_not_enter', mission='harvest'),
        dict(pad, name='per_cell_pad_selling', refinery_mission='selling'),
        dict(pad, name='per_cell_queue_cell', miner_cell=[10, 10]),
        dict(pad, name='per_cell_no_contact', linked=False),
    ]


def per_cell_release_cases():
    # A War Miner at the queueing cell holding the refinery as its contact
    # (both ways), as Mission_Harvest state 2's HELLO leaves it.
    harvest = dict(mission='harvest', status=2, ready=[0])
    return [
        dict(harvest, name='release_harvest_contact'),
        dict(harvest, name='release_harvest_contact_tethered', miner_tether=True,
             refinery_tether=True),
        dict(harvest, name='release_guard_contact', mission='guard'),
        dict(harvest, name='release_enter_queued_ready', queued='enter', ready=[1]),
        dict(harvest, name='release_enter_queued_not_ready', queued='enter'),
        dict(harvest, name='release_enter_current', mission='enter'),
        dict(harvest, name='release_unload', mission='unload', status=0),
        dict(harvest, name='release_unload_latch', unloading=True, ready=[]),
        dict(harvest, name='release_no_contact', linked=False),
        dict(harvest, name='release_not_refinery', refinery_flag=False),
        dict(harvest, name='release_not_harvester', harvester=False),
        dict(harvest, name='release_weeder_dock', harvester=False, weeder=True,
             weeder_dock=True),
        dict(harvest, name='release_weeder_not_weeder_dock', harvester=False, weeder=True),
    ]


# The continuation corpus deliberately keeps every legacy row above unchanged.
# It uses the same map/Drive/radio and ore owners with original idle, readiness,
# selection and mission-dispatch bodies, rather than their legacy supplied replies.
FRAME, DISPATCH, UNIT_VTABLE = 0xA8ED84, 0x5B3060, 0x7F5C70
CONTACT_BREAK, POINTER_EXPIRED = 0x65ACB0, 0x7446E0
CONTROLS = 0xA8E3A8
HOUSE_BUILDINGS = EXTRA + 0x2B000
DOCK_FIELDS = {0xAC: 'mission', 0xB4: 'queued', 0xBC: 'status',
               0xC8: 'dispatch_start', 0xD0: 'dispatch_duration',
               0xF8: 'stage', 0xFC: 'stage_changed', 0x100: 'stage_start',
               0x108: 'stage_duration', 0x10C: 'stage_rate', 0x110: 'stage_step',
               0x218: 'archive', 0x2B4: 'target', 0x598: 'nav_queue_count',
               0x500: 'pending_entry',
               0x5A0: 'aux_destination', 0x5A4: 'destination',
               0x6D1: 'unloading', 0x6D2: 'harvesting'}
DOCK_CALLS = {DISPATCH: ('dispatch', 0), ENTER: ('enter', 0),
              HARVEST: ('harvest', 0), UNLOAD: ('unload', 0),
              IDLE: ('unit_idle', 2), 0x4D82B0: ('foot_idle', 2),
              READY: ('ready', 0), COMMENCE: ('commence', 0), QUEUE: ('queue', 2),
              0x5B3A00: ('current_mission_control', 0),
              FIND_DOCKING_BAY: ('find_docking_bay', 3),
              0x4DEE80: ('find_near_building', 3), 0x65ADF0: ('has_free_or_own', 1),
              TRANSMIT: ('transmit', 3), CONTACT_BREAK: ('break_contact', 1),
              0x43C2D0: ('building_receive', 3), 0x737430: ('unit_receive', 3),
              0x5F5270: ('object_receive', 3),
              RANDOM: ('scenario_range', 2), 0x65C780: ('scenario_raw', 0),
              ASSIGN: ('destination', 2), GIVE_TIBERIUM: ('give_tiberium', 2),
              POINTER_EXPIRED: ('unit_pointer_expired', 2),
              0x65AAC0: ('radio_pointer_expired', 2),
              0x70D7E0: ('pending_entry_try', 0),
              0x70D8F0: ('pending_entry_nearby', 0), 0x703590: ('nearby_location', 2),
              0x56D230: ('map_zone', 3), FNPC: ('nearby_passable', 15),
              0x4D85D0: ('foot_per_cell', 1), 0x55A910: ('locomotor_power_off', 1),
              0x55A8F0: ('locomotor_power_on', 1)}


class DockInputReader:
    """Docking-only original reads inside the shared HARV layer reader.

    The shared reader owns constructors, physical lexical loading, CRC indexes,
    layer order and the one complete MissionControl read. This extension owns
    only additional type/economy fields consumed by the dock continuation.
    """
    selected = {'General': {'HarvesterDumpRate', 'HarvesterTooFarDistance',
                            'ChronoHarvTooFarDistance', 'PurifierBonus'},
                'HARV': {'Strength'},
                **{name: {'Value'} for name in ('Riparius', 'Cruentus', 'Vinifera', 'Aboreus')},
                **{name: {'Strength', 'DockUnload', 'Refinery', 'UnitRepair', 'NumberOfDocks'}
                   for name in ('NAREFN', 'GAREFN', 'NADEPT')},
                **{name: {'Rate', 'AARate', 'NoThreat', 'Zombie', 'Recruitable',
                          'Paralyzed', 'Retaliate', 'Scatter'}
                   for name in ('Guard', 'Move', 'Enter', 'Harvest', 'Unload')}}
    missions = {'guard': 5, 'move': 2, 'enter': 7, 'harvest': 10, 'unload': 16}
    bfields = {'strength': (0xA0, 'Strength', 4), 'dock_unload': (0x16B3, 'DockUnload', 1),
               'refinery': (0x16BB, 'Refinery', 1), 'unit_repair': (0x16A9, 'UnitRepair', 1),
               'docks': (0x1780, 'NumberOfDocks', 4)}

    def __init__(self, m, typ, rules):
        self.m, self.u, self.typ, self.rules = m, m.u, typ, rules
        constructor_writes = []

        def dock_constructor_write(u, access, address, size, value, data):
            pc = u.reg_read(UC_X86_REG_EIP)
            if pc in (0x45E28A, 0x45E292, 0x45E298, 0x45E2BA,
                      0x45E4D2, 0x45E4D6, 0x45E4D9):
                constructor_writes.append(dict(pc=hex(pc), address=hex(address), size=size, value=value))

        hook = self.u.hook_add(UC_HOOK_MEM_WRITE, dock_constructor_write)
        try:
            self.btypes = {name: m.invoke(0x4653C0, m.cstring(name))
                           for name in ('NAREFN', 'GAREFN', 'NADEPT')}
        finally:
            self.u.hook_del(hook)
        assert all(self.btypes.values()), self.btypes
        self.dock_array_constructor = {}
        for name, ptr in self.btypes.items():
            items = m.read32(ptr + 0x1788)
            self.dock_array_constructor[name] = dict(
                array_bytes=bytes(self.u.mem_read(ptr + 0x1780, 0x12)).hex(),
                offset_bytes=bytes(self.u.mem_read(items, m.read32(ptr + 0x1780) * 12)).hex(),
                writes=[row for row in constructor_writes if
                        ptr + 0x1780 <= int(row['address'], 16) < ptr + 0x1792 or
                        items <= int(row['address'], 16) < items + 12],
                original_constructor='0x45d5e0')
        self.tibs = {name: m.alloc(0x200) for name in ('Riparius', 'Cruentus', 'Vinifera', 'Aboreus')}
        for name, ptr in self.tibs.items():
            self.u.mem_write(ptr + 0x24, name.encode('ascii') + b'\0')
            # 721713 stores the constructor's zero EBX in Value. The enclosing
            # AbstractType name/registry and queue construction are excluded.
            self.u.reg_write(UC_X86_REG_ESI, ptr)
            self.u.reg_write(UC_X86_REG_EBX, 0)
            run_checked(self.u, 0x721713, 0x721719, required_addresses=(0x721713,))
        self.constructor = self.snap()
        self.calls, self.writes, self.layers = [], [], []
        self.current = None
        self.watched = {typ + 0xA0: 'strength', rules + 0x1528: 'dump_rate',
                        rules + 0xD78: 'harvester_too_far', rules + 0xD7C: 'chrono_too_far',
                        rules + 0xF3C: 'purifier_bonus'}
        for name, ptr in self.btypes.items():
            for field, (offset, key, size) in self.bfields.items():
                self.watched[ptr + offset] = name + '.' + field
            for offset, field in ((0xEF0, 'foundation'), (0x1618, 'queueing_x'), (0x161C, 'queueing_y')):
                self.watched[ptr + offset] = name + '.' + field
        for name, ptr in self.tibs.items():
            self.watched[ptr + 0xB8] = name + '.value'
        self.hooks = [self.u.hook_add(UC_HOOK_CODE, self.observe),
                      self.u.hook_add(UC_HOOK_MEM_WRITE, self.written)]

    def snap(self):
        u, m, typ, rules = self.u, self.m, self.typ, self.rules
        return dict(strength=struct.unpack('<i', u.mem_read(typ + 0xA0, 4))[0],
                    dump_rate_bits=bytes(u.mem_read(rules + 0x1528, 8)).hex(),
                    too_far=list(struct.unpack('<2i', u.mem_read(rules + 0xD78, 8))),
                    purifier_bonus_bits=bytes(u.mem_read(rules + 0xF3C, 4)).hex(),
                    full_health_threshold_bits=bytes(u.mem_read(rules + 0x16F8, 8)).hex(),
                    buildings={name: {field: u.mem_read(ptr + off, 1)[0] if size == 1 else
                                      struct.unpack('<i', u.mem_read(ptr + off, 4))[0]
                                      for field, (off, key, size) in self.bfields.items()} |
                               dict(foundation=m.read32(ptr + 0xEF0),
                                    queueing_cell=list(struct.unpack('<2i', u.mem_read(ptr + 0x1618, 8))))
                               for name, ptr in self.btypes.items()},
                    tiberium_values={name: struct.unpack('<i', u.mem_read(ptr + 0xB8, 4))[0]
                                     for name, ptr in self.tibs.items()},
                    mission_controls={name: bytes(u.mem_read(CONTROLS + number * 32, 32)).hex()
                                      for name, number in self.missions.items()})

    def observe(self, u, pc, size, data):
        if pc in (0x5276D0, 0x5283D0, 0x5295F0, 0x474DA0, 0x529880, 0x528A10, 0x529CA0):
            sp, m = u.reg_read(UC_X86_REG_ESP), self.m
            count = {0x5283D0: 4, 0x529880: 4, 0x528A10: 5, 0x529CA0: 4}.get(pc, 3)
            self.calls.append(dict(layer=self.current, pc=hex(pc), caller=hex(m.read32(sp)),
                                   args=[m.read32(sp + n * 4) for n in
                                         range(1, count + 1)]))

    def written(self, u, access, address, size, value, data):
        if address in self.watched or CONTROLS <= address < CONTROLS + 32 * 32:
            self.writes.append(dict(layer=self.current, pc=hex(u.reg_read(UC_X86_REG_EIP)),
                                    field=self.watched.get(address, 'mission_control'),
                                    size=size, value=value))

    def begin_layer(self, filename, raw, sections, lines):
        self.current = filename
        self.row = dict(file=filename, sha256=hashlib.sha256(raw).hexdigest(), bytes=len(raw),
                        selected={s: {k: v for k, v in values.items() if k in self.selected[s]}
                                  for s, values in sections.items() if s in self.selected},
                        lines=[line for line in lines if line['section'] in self.selected
                               and line['key'] in self.selected[line['section']]], before=self.snap())
        self.start, self.wstart = len(self.calls), len(self.writes)

    def read_layer(self):
        from tools.spatial_oracle.building_body_rules import INI, SP as READER_SP
        u, m, typ, rules = self.u, self.m, self.typ, self.rules
        for section, pointer in (('HARV', typ), *self.btypes.items()):
            if not m.invoke(0x526810, INI, [m.cstring(section)]):
                continue
            for register, value in ((UC_X86_REG_ESP, READER_SP), (UC_X86_REG_EBX, pointer),
                                    (UC_X86_REG_ESI, INI), (UC_X86_REG_EBP, pointer + 0x24)):
                u.reg_write(register, value)
            run_checked(u, 0x5F94D3, 0x5F94F3, required_addresses=(0x5276D0,))
            if section != 'HARV':
                for begin, end in ((0x460906, 0x46092F), (0x4609D6, 0x4609F6),
                                   (0x460A52, 0x460A72), (0x46492E, 0x46494B)):
                    for register, value in ((UC_X86_REG_ESP, READER_SP), (UC_X86_REG_EBP, pointer),
                                            (UC_X86_REG_ESI, INI), (UC_X86_REG_EBX, pointer + 0x24),
                                            (UC_X86_REG_EAX, u.mem_read(pointer + 0x16BA, 1)[0])):
                        u.reg_write(register, value)
                    # Refinery's block also stores the preceding reader AL;
                    # preserve that independent field's prior value explicitly.
                    run_checked(u, begin, end,
                                required_addresses=(0x5276D0 if begin == 0x46492E else 0x5295F0,))
        if m.invoke(0x526810, INI, [m.cstring('General')]):
            for begin, end in ((0x66FC56, 0x66FC7B), (0x66FFD7, 0x670010),
                               (0x670CC0, 0x670CE7)):
                for register, value in ((UC_X86_REG_ESP, READER_SP), (UC_X86_REG_ESI, rules),
                                        (UC_X86_REG_EDI, INI)):
                    u.reg_write(register, value)
                run_checked(u, begin, end, required_addresses=(begin,))
        for name, pointer in self.tibs.items():
            if not m.invoke(0x526810, INI, [m.cstring(name)]):
                continue
            for register, value in ((UC_X86_REG_ESI, pointer), (UC_X86_REG_EDI, pointer + 0x24),
                                    (UC_X86_REG_EBX, INI), (UC_X86_REG_ECX, INI),
                                    (UC_X86_REG_EAX, m.read32(pointer + 0xB8)),
                                    (UC_X86_REG_ESP, READER_SP)):
                u.reg_write(register, value)
            run_checked(u, 0x721AFA, 0x721B12, required_addresses=(0x5276D0, 0x721B0C))

    def end_layer(self):
        self.row.update(after=self.snap(), calls=self.calls[self.start:], writes=self.writes[self.wstart:])
        self.layers.append(self.row)

    def read_art(self, path):
        from tools.projectile_oracle.bridge_render_inputs import lexical
        from tools.spatial_oracle.building_body_rules import SP as READER_SP
        u, m = self.u, self.m
        raw = path.read_bytes()
        sections, lines = lexical(raw, set(self.btypes))
        m.make_ini(sections)
        self.current = 'ARTMD.INI'
        start, wstart, before = len(self.calls), len(self.writes), self.snap()
        lookups = {}
        for name, ptr in self.btypes.items():
            lookups[name] = hex(m.invoke(0x526810, 0x887180, [m.cstring(name)]))
            # Effective Image==typeID is supplied for these named stock types.
            # The original dual Foundation read/store executes unchanged, as
            # in the existing engineer_bridge_cursor_caller foundation corpus.
            u.mem_write(ptr + 0x1F8, name.encode('ascii') + b'\0')
            for register, value in ((UC_X86_REG_ESP, READER_SP), (UC_X86_REG_EBP, ptr),
                                    (UC_X86_REG_EDI, ptr + 0x1F8), (UC_X86_REG_EBX, ptr + 0x24)):
                u.reg_write(register, value)
            run_checked(u, 0x461225, 0x46125D, required_addresses=(0x474DA0, 0x528A10))
            u.reg_write(UC_X86_REG_ESP, READER_SP)
            run_checked(u, 0x4614EE, 0x46152C, required_addresses=(0x529880, 0x461520))
        self.art = dict(file='ARTMD.INI', sha256=hashlib.sha256(raw).hexdigest(), bytes=len(raw),
                        selected={name: {key: value for key, value in values.items()
                                         if key in ('Foundation', 'QueueingCell')}
                                  for name, values in sections.items()},
                        section_lookup_eax=lookups, supplied_effective_images=list(self.btypes),
                        before=before, after=self.snap(), calls=self.calls[start:],
                        writes=self.writes[wstart:])
        # Keep this additional dependency's receipts separate from the previous
        # scalar/Foundation/QueueingCell corpus. The retained scene deliberately
        # does not transfer this stock ART pad into its historical sparse type.
        offsets = self.read_dock_offsets('NADEPT')
        offsets.update(key='DockingOffset0', raw=sections['NADEPT']['DockingOffset0'],
                       source_lines=[line for line in lines if line['section'] == 'NADEPT'
                                     and line['key'] == 'DockingOffset0'])
        self.art['docking_offsets'] = {'NADEPT': offsets}

    def read_dock_offsets(self, name):
        """Original ART sprintf/ReadCoord/slot stores over the constructed pad.

        The full native BuildingType constructor allocates one slot and writes
        zero to each component. This stock type retains count1, so the native
        resize gate preserves that allocation/default before the offset read.
        Heap storage is the existing Landing allocation seam.
        """
        from tools.spatial_oracle.building_body_rules import SP as READER_SP
        u, m, ptr = self.u, self.m, self.btypes[name]
        constructor = self.dock_array_constructor[name]
        previous = struct.unpack('<i', bytes.fromhex(constructor['array_bytes'])[:4])[0]
        count = m.read32(ptr + 0x1780)
        assert previous == count == m.read32(ptr + 0x178C) == 1
        text = bytes(u.mem_read(0x401000, 0x3E0000))
        reader_calls, writes = len(self.calls), []

        def written(uc, access, address, size, value, data):
            items = m.read32(ptr + 0x1788)
            if ptr + 0x1784 <= address < ptr + 0x1792 or (items and items <= address < items + count * 12):
                writes.append(dict(pc=hex(u.reg_read(UC_X86_REG_EIP)),
                                   address=hex(address), size=size, value=value))

        hook = u.hook_add(UC_HOOK_MEM_WRITE, written)
        try:
            for register, value in ((UC_X86_REG_ESP, READER_SP), (UC_X86_REG_EBP, ptr),
                                    (UC_X86_REG_EDI, previous), (UC_X86_REG_EAX, count)):
                u.reg_write(register, value)
            run_checked(u, 0x46494B, 0x46499D,
                        required_addresses=(0x464951,))
            items = m.read32(ptr + 0x1788)
            initialized = bytes(u.mem_read(items, count * 12))
            assert initialized == bytes(count * 12)
            assert initialized.hex() == constructor['offset_bytes']
            assert u.reg_read(UC_X86_REG_ESP) == READER_SP
            u.reg_write(UC_X86_REG_ESP, READER_SP)
            run_checked(u, 0x46499D, 0x464A47,
                        required_addresses=(0x464A02, 0x529CA0, 0x464A26, 0x464A2F, 0x464A36))
            assert u.reg_read(UC_X86_REG_ESP) == READER_SP
        finally:
            u.hook_del(hook)
        result = bytes(u.mem_read(items, count * 12))
        assert bytes(u.mem_read(0x401000, 0x3E0000)) == text
        return dict(constructor=constructor, supplied_previous_count=previous,
                    native_number_of_docks=count, items_pointer=hex(items),
                    capacity=m.read32(ptr + 0x178C), initialized_offset_bytes=initialized.hex(),
                    offset_bytes=result.hex(), offsets=[list(struct.unpack('<3i', result[n:n + 12]))
                                                        for n in range(0, len(result), 12)],
                    resize_gate_begin='0x46494b', resize_gate_stop='0x46499d', resize_taken=False,
                    reader_begin='0x46499d', reader_stop='0x464a47', reader_entry='0x529ca0',
                    key_format_address='0x8194b4', effective_image=name,
                    calls=self.calls[reader_calls:], writes=writes,
                    original_text_unchanged=True, native_text_sha256=hashlib.sha256(text).hexdigest())

    def close(self):
        for hook in self.hooks:
            self.u.hook_del(hook)


def native_inputs():
    from tools.spatial_oracle.harvest_attack_return import reader_receipts
    root = Path(os.environ.get('VERA20K_REFINERY_DOCK_INPUTS',
                               'target/asset/refinery-dock/extract'))
    harv = reader_receipts(refinery_dock=True, root=root)
    docking = harv.pop('refinery_dock')
    return dict(harv=harv, **docking)


class DockContinuation:
    """One retained original VM for dispatch/radio/deposit/cleanup histories."""
    def __init__(self, case, inputs):
        from tools.spatial_oracle.harvest_field import fixture
        self.case = case
        base = dict(case, linked=case.get('linked', True),
                    native_idle=True, native_ready=True, native_bays=True)
        self.u, self.call, self.read32, self.callbacks, self.unused = fixture(base)
        u, r = self.u, self.read32
        if case.get('hover'):
            self.call(0x513C20, LOCO, [])
            u.mem_write(LOCO + 0xC, dwords(ACTOR))
            u.mem_write(LOCO + 0x14, dwords(1))
        stock, harv = inputs['after'], inputs['harv']['after']
        building_name = 'NADEPT' if case.get('unit_repair') else 'NAREFN'
        building_type = stock['buildings'][building_name]
        for field, (off, key, size) in DockInputReader.bfields.items():
            value = building_type[field]
            u.mem_write(BTYPE + off, bytes([value]) if size == 1 else dwords(value))
        u.mem_write(BTYPE + 0xEF0, dwords(building_type['foundation']))
        u.mem_write(BTYPE + 0x1618, dwords(*building_type['queueing_cell']))
        u.mem_write(BLD + 0x6C, dwords(case.get('refinery_health', building_type['strength'])))
        from tools.spatial_oracle.harvest_field import TIBS
        for index, name in enumerate(('Riparius', 'Cruentus', 'Vinifera', 'Aboreus')):
            u.mem_write(TIBS + index * 0x200 + 0xB8, dwords(stock['tiberium_values'][name]))
        for field, offset, size in (('harvester', 0xE0E, 1), ('weeder', 0xE0F, 1),
                                    ('storage', 0x800, 4), ('movement_zone', 0x5B4, 4)):
            value = int(case.get('capacity' if field == 'storage' else field, harv[field]))
            u.mem_write(TYPE + offset, bytes([value]) if size == 1 else dwords(value))
        u.mem_write(TYPE + 0xA0, dwords(case.get('strength', stock['strength'])))
        u.mem_write(ACTOR + 0x6C, dwords(case.get('health', stock['strength'])))
        u.mem_write(ACTOR + 0x90, b'\x01')
        u.mem_write(ACTOR + 0xB0, dwords(-1))
        if 'current' in case:
            u.mem_write(ACTOR + 0xAC, dwords(case['current']))
        if 'pending_entry' in case:
            u.mem_write(ACTOR + 0x500, dwords({'refinery': BLD, 'other': OTHER}[case['pending_entry']]))
        for offset in (0x458, 0x470):
            u.mem_write(ACTOR + offset, dwords(0x7E91EC))
        self.call(0x4DF1A0, ACTOR, [])
        u.reg_write(UC_X86_REG_ESI, ACTOR)
        run_checked(u, 0x4D31F1, 0x4D31FB, required_addresses=(0x4D31F1,))
        u.mem_map(0, 0x1000)
        u.mem_write(0, dwords(-1))
        for name, offset in (('load_rate', 0x1520), ('short_scan', 0x1778), ('long_scan', 0x177C)):
            u.mem_write(RULES + offset, dwords(harv[name]))
        u.mem_write(RULES + 0x1528, bytes.fromhex(stock['dump_rate_bits']))
        u.mem_write(RULES + 0xD78, dwords(*stock['too_far']))
        u.mem_write(RULES + 0xF3C, bytes.fromhex(stock['purifier_bonus_bits']))
        u.mem_write(RULES + 0x16F8, bytes.fromhex(stock['full_health_threshold_bits']))
        # Avoid the separate broad plain-Unit AI guard policy in these controls.
        u.mem_write(RULES + 0x1440, dwords(1))
        numbers = {'guard': 5, 'move': 2, 'enter': 7, 'harvest': 10, 'unload': 16}
        for name, number in numbers.items():
            u.mem_write(CONTROLS + number * 32, bytes.fromhex(stock['mission_controls'][name]))
        for name, rate in case.get('rates', {}).items():
            u.mem_write(CONTROLS + numbers[name] * 32 + 0x10, struct.pack('<d', rate))
        for building in (BLD, OTHER):
            u.mem_write(building + 0x90, b'\x01')
            # Original constructor's -1 and Begin_Mode(Idle) producer establish
            # active mode1. Stop before the separate animation setup producer.
            u.mem_write(building + 0x534, dwords(-1))
            u.mem_write(building + 0x538, dwords(-1))
            u.mem_write(SP, dwords(RET_MAGIC, case.get('building_mode', 1)))
            u.reg_write(UC_X86_REG_ECX, building)
            u.reg_write(UC_X86_REG_ESP, SP)
            run_checked(u, 0x447780, 0x4477C7, required_addresses=(0x4477C1,))
        if 'refinery_current' in case:
            u.mem_write(BLD + 0xAC, dwords(case['refinery_current']))
        if 'refinery_queued' in case:
            u.mem_write(BLD + 0xB4, dwords(case['refinery_queued']))
        if 'pending_entry' in case:
            pointer = {'refinery': BLD, 'other': OTHER}[case['pending_entry']]
            u.mem_write(pointer + 0x90, bytes([case.get('pending_alive', True)]))
            u.mem_write(pointer + 0x14, dwords(int(case.get('pending_techno', True))))
            if 'pending_health' in case:
                u.mem_write(pointer + 0x6C, dwords(case['pending_health']))
        if case.get('native_zone_scene'):
            # A supplied uniform sector0/zone0 scene, not a zone producer.
            # Original GetZone and NearbyLocation consume these arrays.
            u.mem_write(0x87F7E8 + 0x68, dwords(EXTRA + 0x2B400, 2048))
            u.mem_write(EXTRA + 0x2B400, bytes(2048 * 4))
            u.mem_write(EXTRA + 0x2D800, b'\0\0')
            for offset in range(0x18, 0x2C, 4):
                u.mem_write(0x87F7E8 + offset, dwords(EXTRA + 0x2D800))
        if case.get('unit_repair'):
            u.mem_write(BTYPE + 0x16A9, b'\x01')
            u.mem_write(BTYPE + 0x16B3, b'\x00')
            u.mem_write(BTYPE + 0x16BB, b'\x00')
        if case.get('busy'):
            u.mem_write(MINER_ITEMS, dwords(0))
            u.mem_write(BLD_ITEMS, dwords(OTHER))
            u.mem_write(OTHER_ITEMS, dwords(BLD))
        owned = case.get('owned', ['refinery'])
        u.mem_write(HOUSE + 0x6C, dwords(HOUSE_BUILDINGS, 2))
        u.mem_write(HOUSE + 0x78, dwords(len(owned)))
        u.mem_write(HOUSE_BUILDINGS, dwords(*[{'refinery': BLD, 'other': OTHER}[name] for name in owned]))
        u.mem_write(0xA8E7AC, dwords(0))
        if case.get('install_nav'):
            nav = r(ACTOR + 0x5A4)
            assert nav
            u.mem_write(ACTOR + 0x5A4, dwords(0))
            self.call(ASSIGN, ACTOR, [nav, 1])
        self.phase, self.instruction = 'fixture', 0
        self.events, self.writes, self.pending, self.steps = [], [], {}, []
        self.original_text = hashlib.sha256(bytes(u.mem_read(0x401000, 0x3E0000))).hexdigest()
        self.original_vtables = [bytes(u.mem_read(address, size))
                                 for address, size in ((UNIT_VTABLE, 0x600), (0x7E3EBC, 0x600))]
        assert r(UNIT_VTABLE + 0x224) == HARVEST
        assert r(UNIT_VTABLE + 0x210) == 0x7447A0
        assert r(UNIT_VTABLE + 0x484) == IDLE
        u.hook_add(UC_HOOK_CODE, self.observe)
        u.hook_add(UC_HOOK_MEM_WRITE, self.written)
        self.callbacks.clear()
        self.before = self.state()

    def state(self):
        from tools.spatial_oracle.shrapnel_repair.shrapnel_repair import rng_state
        u, r = self.u, self.read32
        result = state(u, r)
        result.update(frame=r(FRAME), cell=list(struct.unpack('<3i', u.mem_read(ACTOR + 0x9C, 12))),
                      target=name_of(r(ACTOR + 0x2B4)), archive=nav_target(r(ACTOR + 0x218)),
                      pending_entry=name_of(r(ACTOR + 0x500)),
                      nav_queue_count=r(ACTOR + 0x598), harvesting=u.mem_read(ACTOR + 0x6D2, 1)[0],
                      stage_changed=u.mem_read(ACTOR + 0xFC, 1)[0], stage_step=r(ACTOR + 0x110),
                      dispatch_words=list(struct.unpack('<3i', u.mem_read(ACTOR + 0xC8, 12))),
                      health=struct.unpack('<i', u.mem_read(ACTOR + 0x6C, 4))[0],
                      strength=struct.unpack('<i', u.mem_read(TYPE + 0xA0, 4))[0],
                      scenario_rng=rng_state(u, SCENARIO + 0x218), bypass_counter=r(0xA8E7AC),
                      building_repairing=u.mem_read(BLD + 0x6DD, 1)[0],
                      locomotor_powered=u.mem_read(LOCO + 0x10, 1)[0],
                      locomotor_kind='hover' if self.case.get('hover') else 'drive',
                      loco_destination=None if self.case.get('hover') else
                      list(struct.unpack('<3i', u.mem_read(LOCO + 0x34, 12))),
                      loco_head=None if self.case.get('hover') else
                      list(struct.unpack('<3i', u.mem_read(LOCO + 0x40, 12))))
        return result

    def observe(self, u, pc, size, data):
        self.instruction += 1
        for event in self.pending.pop(pc, []):
            event['returned_eax'] = u.reg_read(UC_X86_REG_EAX)
        if pc == 0x73A4B7:
            sp = u.reg_read(UC_X86_REG_ESP)
            self.events.append(dict(kind='original_locomotor_class_id', pc=hex(pc),
                                    actual=bytes(u.mem_read(sp + 0x30, 16)).hex(),
                                    compared=bytes(u.mem_read(0x7E9A40, 16)).hex()))
        if pc == 0x65C87E:
            self.events.append(dict(kind='scenario_inline_raw', pc=hex(pc),
                                    value=u.reg_read(UC_X86_REG_EAX)))
        # Existing fixture sinks may already have returned from this entry.
        # Their original arguments/result are in callback_events; do not read
        # the caller's post-return stack as if it were a second native call.
        if u.reg_read(UC_X86_REG_EIP) != pc:
            return
        if pc not in DOCK_CALLS:
            return
        kind, count = DOCK_CALLS[pc]
        sp, r = u.reg_read(UC_X86_REG_ESP), self.read32
        event = dict(pc=hex(pc), kind=kind, caller=hex(r(sp)), phase=self.phase,
                     frame=r(FRAME), this=name_of(u.reg_read(UC_X86_REG_ECX)),
                     args=[r(sp + n * 4) for n in range(1, count + 1)])
        self.events.append(event)
        self.pending.setdefault(r(sp), []).append(event)

    def written(self, u, access, address, size, value, data):
        field = DOCK_FIELDS.get(address - ACTOR)
        if ACTOR + 0x33C <= address < ACTOR + 0x34C:
            field = 'cargo'
        if address in (MINER_ITEMS, BLD_ITEMS, OTHER_ITEMS):
            field = {MINER_ITEMS: 'miner_contact', BLD_ITEMS: 'refinery_contact', OTHER_ITEMS: 'other_contact'}[address]
        if address in (HOUSE + 0x30C, HOUSE + 0x54E8):
            field = 'balance' if address == HOUSE + 0x30C else 'score'
        if address == 0xA8E7AC:
            field = 'bypass_counter'
        if field:
            self.writes.append(dict(pc=hex(u.reg_read(UC_X86_REG_EIP)), phase=self.phase,
                                    frame=self.read32(FRAME), field=field, size=size, value=value))

    def invoke(self, entry, this=ACTOR, args=()):
        self.call(entry, this, list(args))
        for event in self.pending.pop(RET_MAGIC, []):
            event['returned_eax'] = self.u.reg_read(UC_X86_REG_EAX)
        return self.u.reg_read(UC_X86_REG_EAX)

    def try_pending_entry(self):
        """Original70D7E0 plus independent input/read/return-frame receipts.

        Native send wrappers reuse return PCs when Building replies nest sends.
        Keep this added control's exact replies paired with the caller stack
        frame; the historical generic event logs remain unchanged.
        """
        u, r = self.u, self.read32
        fields = {ACTOR + 0x500: ('pending_entry', 4),
                  ACTOR + 0xAC: ('current_mission', 4), ACTOR + 0xB4: ('queued_mission', 4),
                  ACTOR + 0xB8: ('forced_mission', 1), ACTOR + 0x6D1: ('unloading', 1),
                  ACTOR + 0x6E1: ('unit_ready_latch_6e1', 1),
                  ACTOR + 0x6E2: ('unit_ready_latch_6e2', 1),
                  ACTOR + 0x674: ('locomotor_interface', 4),
                  MINER_ITEMS: ('unit_contact0', 4), BLD_ITEMS: ('depot_contact0', 4),
                  BLD + 0x90: ('depot_alive', 1), BLD + 0x14: ('depot_abstract_flags', 4),
                  BLD + 0x520: ('depot_type', 4), BTYPE + 0x16A9: ('unit_repair', 1),
                  BTYPE + 0x16BD: ('weapons_factory', 1),
                  LOCO + 0x34: ('drive_destination_x', 4), LOCO + 0x40: ('drive_head_x', 4)}
        xy = [r(ACTOR + 0x9C) // 256, r(ACTOR + 0xA0) // 256]
        fields[cell(*xy) + 0xE4] = ('unit_cell_first_object', 4)
        inputs = [dict(field=field, address=hex(address), size=size,
                       bytes=bytes(u.mem_read(address, size)).hex())
                  for address, (field, size) in fields.items()]
        reads, calls, frames, reached = [], [], {}, set()
        entries = {TRANSMIT: ('transmit', 3, 12), QUEUE: ('queue', 2, 8),
                   READY: ('unit_ready', 0, 0), COMMENCE: ('commence', 0, 0),
                   ASSIGN: ('unit_destination', 2, 8)}

        def code(uc, pc, size, data):
            if u.reg_read(UC_X86_REG_EIP) != pc:
                return
            sp = u.reg_read(UC_X86_REG_ESP)
            for call in frames.pop((pc, sp), []):
                call['returned_eax'] = u.reg_read(UC_X86_REG_EAX)
                call['returned_al'] = call['returned_eax'] & 255
                if call['kind'] in ('queue', 'unit_ready', 'commence'):
                    call['after_mission'] = struct.unpack('<i', u.mem_read(ACTOR + 0xAC, 4))[0]
                    call['after_queued'] = struct.unpack('<i', u.mem_read(ACTOR + 0xB4, 4))[0]
            if pc in (0x70D7E0, 0x70D83C, 0x70D849, 0x70D84F, 0x70D864, 0x70D872, 0x70D889):
                reached.add(pc)
            if pc not in entries:
                return
            kind, argc, cleanup = entries[pc]
            call = dict(kind=kind, pc=hex(pc), caller=hex(r(sp)), stack_frame=hex(sp),
                        this=name_of(u.reg_read(UC_X86_REG_ECX)),
                        args=[r(sp + n * 4) for n in range(1, argc + 1)],
                        current_mission=struct.unpack('<i', u.mem_read(ACTOR + 0xAC, 4))[0],
                        queued_mission=struct.unpack('<i', u.mem_read(ACTOR + 0xB4, 4))[0],
                        contact0=name_of(r(MINER_ITEMS)))
            calls.append(call)
            frames.setdefault((r(sp), sp + 4 + cleanup), []).append(call)

        def read(uc, access, address, size, value, data):
            if address in fields:
                reads.append(dict(pc=hex(u.reg_read(UC_X86_REG_EIP)), field=fields[address][0],
                                  address=hex(address), size=size,
                                  bytes=bytes(u.mem_read(address, size)).hex()))

        hooks = [u.hook_add(UC_HOOK_CODE, code), u.hook_add(UC_HOOK_MEM_READ, read)]
        try:
            answer = self.invoke(0x70D7E0)
        finally:
            for hook in hooks:
                u.hook_del(hook)
        assert not frames, frames
        return dict(native_raw_eax=answer, native_al=answer & 255,
                    native_return_is_non_semantic_for_rust_void_owner=True,
                    prepared_inputs=inputs, reads=reads, frame_matched_calls=calls,
                    reached_call_sites=[hex(pc) for pc in sorted(reached)])

    def step(self, operation):
        u, r = self.u, self.read32
        self.phase = operation['op']
        before, start, wstart, cstart = self.state(), len(self.events), len(self.writes), len(self.callbacks)
        if 'frame' in operation:
            u.mem_write(FRAME, dwords(operation['frame']))
        answer = None
        if self.phase == 'tick':
            ready = self.invoke(READY) & 255
            promoted = self.invoke(COMMENCE) & 255 if ready else None
            dispatched = self.invoke(DISPATCH)
            # Original TechnoAI Stage follows MissionClass dispatch.
            u.reg_write(UC_X86_REG_ESI, ACTOR)
            u.reg_write(UC_X86_REG_EBP, 0)
            u.reg_write(UC_X86_REG_ESP, SP)
            u.mem_write(SP + 0x2C, dwords(0))
            run_checked(u, 0x6FABB8, 0x6FAC31, required_addresses=(0x6FABB8,))
            # Original UnitAI effective-mission/latch prefix before Fire.
            u.reg_write(UC_X86_REG_ESI, ACTOR)
            u.reg_write(UC_X86_REG_ESP, SP)
            run_checked(u, 0x7365BB, 0x7365DF, required_addresses=(0x7365BB,))
            answer = dict(ready_al=ready, commence_al=promoted, dispatch_eax=dispatched)
        elif self.phase == 'enter':
            answer = self.invoke(ENTER)
        elif self.phase == 'pending_entry_try':
            answer = self.try_pending_entry()
        elif self.phase == 'unload':
            answer = self.invoke(UNLOAD)
        elif self.phase == 'has_free_or_own':
            raw = self.invoke(0x65ADF0, BLD, [ACTOR])
            answer = dict(native_eax=raw, native_al=raw & 255, admitted=bool(raw & 255))
        elif self.phase == 'find_bay':
            u.mem_write(0xA8E7AC, dwords(operation.get('bypass', 0)))
            answer = name_of(self.invoke(FIND_DOCKING_BAY,
                                        args=[TYPE + 0x3E8, 0, operation.get('wide', 0)]))
        elif self.phase == 'per_cell_enter':
            xy = [r(ACTOR + 0x9C) // 256, r(ACTOR + 0xA0) // 256]
            frame = SP - 0x100
            u.mem_write(frame + 0x14, dwords(r(MINER_ITEMS)))
            u.mem_write(frame + 0x1C, struct.pack('<hh', *xy))
            u.reg_write(UC_X86_REG_EBP, ACTOR)
            u.reg_write(UC_X86_REG_ESP, frame)
            end = run_checked(u, 0x73A31F, (0x73A540, 0x73A5EA),
                              required_addresses=(0x73A31F,))
            answer = dict(original_stop=hex(end), early_return_before_unit_tail=end == 0x73A540)
        elif self.phase == 'repair_release_power':
            # Service completion is the declared external repair boundary.
            # This original release prefix powers the linked Foot on before
            # the separately owned repair mechanism selects an exit.
            u.mem_write(ACTOR + 0x6C, dwords(r(TYPE + 0xA0)))
            u.reg_write(UC_X86_REG_EBP, BLD)
            u.reg_write(UC_X86_REG_EDI, 0)
            u.reg_write(UC_X86_REG_ESP, SP - 0x100)
            run_checked(u, 0x44C35B, 0x44C397, required_addresses=(0x44C394,))
            for event in self.pending.pop(0x44C397, []):
                event['returned_eax'] = u.reg_read(UC_X86_REG_EAX)
            answer = dict(original_stop='0x44c397', supplied_service_completion=True)
        elif self.phase == 'radio':
            sender, target = (BLD, ACTOR) if operation.get('from') == 'refinery' else (ACTOR, BLD)
            answer = self.invoke(TRANSMIT, sender, [operation['message'], 0xA8EC30, target])
            answer = dict(reply=answer, payload=name_of(r(0xA8EC30)))
        elif self.phase == 'break_other_contact':
            answer = self.invoke(CONTACT_BREAK, OTHER, [3])
        elif self.phase == 'arrival':
            # Explicit scene/motion boundary: place at the requested native
            # destination and end the Drive. Original destination setter and
            # PerCell arms execute; pathfinding/track traversal are excluded.
            nav = r(ACTOR + 0x5A4)
            assert nav and nav not in (BLD, OTHER, ACTOR), nav
            xy = cell_xy(nav)
            u.mem_write(ACTOR + 0x9C, dwords(xy[0] * 256 + 128, xy[1] * 256 + 128, 0))
            u.mem_write(LOCO + 0x34, dwords(0, 0, 0))
            u.mem_write(LOCO + 0x40, dwords(0, 0, 0))
            self.invoke(ASSIGN, args=[0, 1])
            frame = SP - 0x100
            u.mem_write(frame + 0x14, dwords(r(MINER_ITEMS)))
            u.mem_write(frame + 0x1C, struct.pack('<hh', *xy))
            u.reg_write(UC_X86_REG_EBP, ACTOR)
            u.reg_write(UC_X86_REG_ESP, frame)
            run_checked(u, 0x73A31F, 0x73A5EA, required_addresses=(0x73A31F,))
            u.reg_write(UC_X86_REG_EBP, ACTOR)
            u.reg_write(UC_X86_REG_ESP, SP - 0x100)
            run_checked(u, 0x73ACB3, 0x73ADCA, required_addresses=(0x73ACB3,))
            answer = dict(supplied_arrival_cell=xy, original_per_cell_snippets=True)
        elif self.phase == 'pointer_expiry':
            u.mem_write(BLD + 0x6C, dwords(0))
            u.mem_write(BLD + 0x90, b'\x00')
            answer = self.invoke(POINTER_EXPIRED, args=[BLD, 1])
        elif self.phase == 'building_now_dead_contacts':
            # The original NowDead contact loop executes in ascending order.
            # Supply its already captured contact-vector locals and the dead
            # building prestate; damage/destructor/world traversal are excluded.
            u.mem_write(BLD + 0x6C, dwords(0))
            u.mem_write(BLD + 0x90, b'\x00')
            frame = SP - 0x200
            u.mem_write(frame + 0x18, dwords(r(BLD + 0xE4)))
            u.mem_write(frame + 0x24, dwords(r(BLD + 0xE8)))
            u.reg_write(UC_X86_REG_ESI, BLD)
            u.reg_write(UC_X86_REG_ESP, frame)
            run_checked(u, 0x442511, 0x442608, timeout_us=30_000_000,
                        required_addresses=(0x4425A4,))
            answer = dict(original_loop_end='0x442608')
        else:
            raise AssertionError(operation)
        self.steps.append(dict(input=operation, before=before, after=self.state(), returned=answer,
                               events=self.events[start:], writes=self.writes[wstart:],
                               callback_events=self.callbacks[cstart:]))

    def run_until(self, predicate, limit=200):
        for _ in range(limit):
            if predicate(self.state()):
                return
            self.step(dict(op='tick', frame=self.read32(FRAME) + 1))
        raise AssertionError(('native continuation limit', self.case['name'], self.state()))

    def finish(self):
        assert not self.pending, self.pending
        assert not any(self.unused[0]), self.unused
        assert hashlib.sha256(bytes(self.u.mem_read(0x401000, 0x3E0000))).hexdigest() == self.original_text
        assert [bytes(self.u.mem_read(address, size)) for address, size in
                ((UNIT_VTABLE, 0x600), (0x7E3EBC, 0x600))] == self.original_vtables
        return dict(input=self.case, before=self.before, after=self.state(), steps=self.steps,
                    original_code_and_vtables_unchanged=True, native_text_sha256=self.original_text,
                    instruction_count=self.instruction)


def supplied_rate_conversion_receipts(rates):
    """Independent original Enter rate-load/ftol prefix, before any RNG draw.

    These inputs are supplied binary64 table fields, not INI ReadDouble's
    binary32-widened fields. Keep their frame projection an executed receipt;
    neither host float multiplication nor Enter's expected return supplies it.
    """
    from tools.spatial_oracle.shrapnel_repair.shrapnel_repair import rng_state
    result = {}
    for name, minutes in rates.items():
        u, call, r = make_dock_fixture(dict(harvester=False, linked=False))
        number = DockInputReader.missions[name]
        control = CONTROLS + number * 32
        input_bits = struct.pack('<d', minutes)
        u.mem_write(ACTOR + 0xAC, dwords(number))
        u.mem_write(control + 0x10, input_bits)
        before_rng = rng_state(u, SCENARIO + 0x218)
        before_text = bytes(u.mem_read(0x401000, 0x3E0000))
        before_vtables = [bytes(u.mem_read(address, 0x600))
                          for address in (UNIT_VTABLE, 0x7E3EBC)]
        multiplier = bytes(u.mem_read(0x7E27F8, 8))
        fpcw = u.reg_read(UC_X86_REG_FPCW)
        u.mem_write(SP, dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESI, ACTOR)
        u.reg_write(UC_X86_REG_ESP, SP)
        # 4D946C loads ECX; 5B3A00 selects current AC's control; FLD/FMUL
        # and original ftol7C5F00 execute. Stop before saving the base result
        # and before the separate ScenarioRandomRanged call at4D9492.
        run_checked(u, 0x4D946C, 0x4D9481,
                    required_addresses=(0x5B3A00, 0x4D9473, 0x4D9476, 0x7C5F00))
        assert bytes(u.mem_read(0x401000, 0x3E0000)) == before_text
        assert [bytes(u.mem_read(address, 0x600))
                for address in (UNIT_VTABLE, 0x7E3EBC)] == before_vtables
        assert bytes(u.mem_read(0x7E27F8, 8)) == multiplier
        assert bytes(u.mem_read(control + 0x10, 8)) == input_bits
        assert rng_state(u, SCENARIO + 0x218) == before_rng
        result[name] = dict(mission_id=number, rate_bits=input_bits.hex(),
                            frames=struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0],
                            ftol_eax=u.reg_read(UC_X86_REG_EAX), ftol_edx=u.reg_read(UC_X86_REG_EDX),
                            original_begin='0x4d946c', original_stop='0x4d9481',
                            ftol_entry='0x7c5f00', rate_field_offset='0x10',
                            multiplier_address='0x7e27f8', multiplier_bits=multiplier.hex(),
                            fpcw_before=fpcw, fpcw_after=u.reg_read(UC_X86_REG_FPCW),
                            scenario_rng_unchanged=True, original_text_and_vtables_unchanged=True,
                            native_text_sha256=hashlib.sha256(before_text).hexdigest())
    return result


def supplied_unit_repair_dock_inputs(case, inputs):
    """Independently query the prepared sparse type without running a mission.

    Historical UnitRepair controls have count1 but a NULL offset-items pointer.
    Their mapped page0 contains [-1,0,0]. Record that effective scene input;
    it is distinct from the original-read retail NADEPT ART offset [128,0,0].
    """
    from tools.spatial_oracle.shrapnel_repair.shrapnel_repair import rng_state
    scene = DockContinuation(case, inputs)
    u, r = scene.u, scene.read32
    count, items = r(BTYPE + 0x1780), r(BTYPE + 0x1788)
    assert count == 1 and items == 0
    raw = bytes(u.mem_read(items, count * 12))
    before_rng = rng_state(u, SCENARIO + 0x218)
    prepared = scene.state()
    reads = []

    def read(uc, access, address, size, value, data):
        pc = u.reg_read(UC_X86_REG_EIP)
        if pc in (0x447D3D, 0x447DB5, 0x447DC9, 0x447D8A, 0x447D8D):
            reads.append(dict(pc=hex(pc), address=hex(address), size=size,
                              bytes=bytes(u.mem_read(address, size)).hex()))

    hook = u.hook_add(UC_HOOK_MEM_READ, read)
    try:
        output = EXTRA + 0x2A000
        center_entry = r(r(BLD) + 0x48)
        assert center_entry == 0x447AC0
        scene.invoke(center_entry, BLD, [output])
        center = list(struct.unpack('<3i', u.mem_read(output, 12)))
        scene.invoke(0x447B20, BLD, [output, ACTOR])
        dock = list(struct.unpack('<3i', u.mem_read(output, 12)))
    finally:
        u.hook_del(hook)
    assert scene.state() == prepared
    assert rng_state(u, SCENARIO + 0x218) == before_rng
    assert bytes(u.mem_read(items, count * 12)) == raw
    identity = scene.finish()
    return dict(building_type='NADEPT', foundation_id=r(BTYPE + 0xEF0),
                number_of_docks=count, number_of_docks_field='BuildingType+0x1780',
                items_pointer=hex(items), items_pointer_field='BuildingType+0x1788',
                offset_bytes=raw.hex(), offsets=[list(struct.unpack('<3i', raw))],
                building_location_leptons=list(struct.unpack('<3i', u.mem_read(BLD + 0x9C, 12))),
                center_leptons=center, dock_leptons=dock,
                dock_cell=[value // 256 for value in dock[:2]],
                center_entry=hex(center_entry), dock_entry='0x447b20', reads=reads,
                prepared_state_and_rng_unchanged=True,
                original_code_and_vtables_unchanged=identity['original_code_and_vtables_unchanged'],
                native_text_sha256=identity['native_text_sha256'],
                instruction_count=identity['instruction_count'])


def continuation_controls(inputs):
    distinctive = {'enter': 0.1, 'guard': 0.03, 'harvest': 0.02, 'move': 0.04}
    rate_receipts = supplied_rate_conversion_receipts(distinctive)
    rows = []
    for name, changes in (
            ('no_target_plain', {'harvester': False}),
            ('no_target_plain_seed31', {'harvester': False, 'seed': 31}),
            ('no_target_harv_clear', {}),
            ('no_target_harv_ore', {'ore': [[10, 10, 0, 0, 5]]}),
            ('no_target_harv_ai', {'human': False}),
            ('no_target_nav_cell', {'nav': [12, 12]}),
            ('no_target_nav_building', {'nav': 'refinery'}),
            ('offline_clear', {'linked': True, 'online': False, 'storage': [40, 0, 0, 0]}),
            ('offline_nav_cell', {'linked': True, 'online': False, 'nav': list(PAD)}),
            ('offline_tethered', {'linked': True, 'online': False, 'miner_tether': True}),
            ('no_target_queued_guard', {'queued': 'guard'})):
        scene = DockContinuation(dict(dict(name=name, mission='enter', linked=False, rates=distinctive), **changes), inputs)
        scene.step(dict(op='enter'))
        rows.append(scene.finish())
    for name, changes in (
            ('pending_alive_nearby', {'native_passable': True, 'native_zone_scene': True}),
            ('pending_alive_negative_health', {'pending_health': -1, 'native_passable': True,
                                                'native_zone_scene': True}),
            ('pending_no_cell', {'native_zone_scene': True, 'passable': [None]}),
            ('pending_dead', {'pending_alive': False}),
            ('pending_not_techno', {'pending_techno': False})):
        scene = DockContinuation(dict(dict(name=name, mission='enter', linked=False,
                                            harvester=False, pending_entry='other', rates=distinctive),
                                      **changes), inputs)
        scene.step(dict(op='enter'))
        rows.append(scene.finish())
    for name, changes in (
            ('repair_full_health', {}), ('repair_damaged', {'health': 500}),
            ('repair_offline', {'health': 500, 'online': False}),
            ('repair_unlinked', {'health': 500, 'linked': False}),
            ('repair_far_moving', {'health': 500, 'nav': [12, 12], 'moving': True}),
            ('repair_near_moving', {'health': 500, 'miner_cell': [7, 10], 'nav': [12, 12], 'moving': True})):
        scene = DockContinuation(dict(dict(name=name, unit_repair=True, harvester=False), **changes), inputs)
        scene.step(dict(op='radio', message=14))
        rows.append(scene.finish())
    for health, strength in ((1000, 1000), (999, 1000), (0, 1000), (-1, 1000),
                              (1000, 0), (0, 0), (-1000, -1000)):
        scene = DockContinuation(dict(name=f'health_status_{health}_{strength}', health=health,
                                      strength=strength, harvester=False), inputs)
        scene.step(dict(op='radio', message=34, **{'from': 'refinery'}))
        rows.append(scene.finish())
    for kind in ('refinery', 'repair'):
        scene = DockContinuation(dict(name=f'dock_now_{kind}', unit_repair=kind == 'repair',
                                      harvester=kind != 'repair'), inputs)
        scene.step(dict(op='radio', message=21))
        rows.append(scene.finish())
    for name, changes, operation in (
            ('choose_active', {}, dict(op='find_bay')),
            ('choose_construction', {'building_mode': 0}, dict(op='find_bay')),
            ('choose_current_construction', {'refinery_current': 18}, dict(op='find_bay')),
            ('choose_current_selling', {'refinery_current': 19}, dict(op='find_bay')),
            ('choose_queued_construction', {'refinery_current': -1, 'refinery_queued': 18}, dict(op='find_bay')),
            ('choose_queued_selling', {'refinery_current': -1, 'refinery_queued': 19}, dict(op='find_bay')),
            ('choose_busy_narrow', {'busy': True}, dict(op='find_bay')),
            ('choose_busy_wide_no_bypass', {'busy': True}, dict(op='find_bay', wide=1)),
            ('choose_busy_wide_bypass', {'busy': True}, dict(op='find_bay', wide=1, bypass=1))):
        scene = DockContinuation(dict(dict(name=name, linked=False), **changes), inputs)
        scene.step(operation)
        rows.append(scene.finish())
    scene = DockContinuation(dict(name='unload_offline_at_dump_gate', online=False,
                                  mission='unload', miner_cell=list(PAD), facing=0x4000,
                                  status=3, unloading=True, storage=[40, 0, 0, 0],
                                  stage=[15, 0, 199, 1, 1]), inputs)
    scene.step(dict(op='unload'))
    rows.append(scene.finish())
    for name, changes in (
            ('repair_per_cell_enter_object', {'nav': 'refinery', 'install_nav': True}),
            ('repair_per_cell_power_release', {'nav': 'refinery', 'install_nav': True}),
            ('repair_per_cell_no_nav_drive', {}),
            ('repair_per_cell_no_nav_hover', {'hover': True}),
            ('repair_per_cell_patrol_object', {'nav': 'refinery', 'current': 25}),
            ('repair_per_cell_return_skipped', {'nav': 'refinery', 'current': 12}),
            ('repair_per_cell_wrong_destination', {'nav': [12, 12]})):
        scene = DockContinuation(dict(dict(name=name, unit_repair=True, harvester=False,
                                            health=500, miner_cell=[7, 10], linked=True), **changes), inputs)
        scene.step(dict(op='per_cell_enter'))
        if name == 'repair_per_cell_power_release':
            scene.step(dict(op='repair_release_power'))
        rows.append(scene.finish())
    for name, changes in (
            ('pending_entry_admit_guard', {}),
            ('pending_entry_admit_moving', {'current': 2, 'nav': [12, 12], 'moving': True}),
            ('pending_entry_admit_sleep', {'current': 0}),
            ('pending_entry_admit_linked_guard', {'linked': True}),
            ('pending_entry_admit_linked_moving', {'linked': True, 'current': 2,
                                                   'nav': [12, 12], 'moving': True}),
            ('pending_entry_foundation_guard', {'miner_cell': [7, 10]}),
            ('pending_entry_foundation_moving', {'miner_cell': [7, 10], 'current': 2,
                                                 'nav': [12, 12], 'moving': True}),
            ('pending_entry_busy_guard', {'busy': True}),
            ('pending_entry_busy_moving', {'busy': True, 'current': 2,
                                          'nav': [12, 12], 'moving': True})):
        case = dict(dict(name=name, mission='enter', current=5, unit_repair=True,
                         harvester=False, health=500, pending_entry='refinery', linked=False), **changes)
        scene = DockContinuation(case, inputs)
        scene.step(dict(op='pending_entry_try'))
        rows.append(scene.finish())
    for row in rows:
        if row['input'].get('unit_repair'):
            row['supplied_dock_inputs'] = supplied_unit_repair_dock_inputs(row['input'], inputs)
        if 'rates' in row['input']:
            assert row['input']['rates'] == distinctive
            row['supplied_rate_frames'] = {name: receipt['frames']
                                           for name, receipt in rate_receipts.items()}
            row['supplied_rate_conversion_receipts'] = rate_receipts
    return rows


def deposit_history(inputs, busy=False):
    scene = DockContinuation(dict(name='stock_ore_cycle_busy' if busy else 'stock_ore_cycle',
                                  mission='harvest', status=0, linked=False, busy=busy,
                                  storage=[40, 0, 0, 0], archive=[15, 15],
                                  ore=[[15, 15, 0, 0, 5]]), inputs)
    scene.step(dict(op='tick', frame=200))
    if busy:
        # Native full-bay wait through both scanner passes, then another
        # object's original BREAK delivers the admission-state change.
        for frame in range(201, 241):
            scene.step(dict(op='tick', frame=frame))
        scene.step(dict(op='break_other_contact'))
    scene.run_until(lambda s: s['miner_nav'] == list(PAD))
    scene.step(dict(op='arrival'))
    scene.run_until(lambda s: s['miner_mission'] == 10 and s['miner_contact'] is None
                    and sum(s['storage']) == 0)
    scene.run_until(lambda s: s['miner_nav'] is not None)
    scene.step(dict(op='arrival'))
    scene.run_until(lambda s: sum(s['storage']) > 0)
    return scene.finish()


def destroyed_history(inputs):
    scene = DockContinuation(dict(name='stock_refinery_destroyed_while_unloading',
                                  mission='harvest', status=0, linked=False,
                                  storage=[40, 0, 0, 0], native_scatter=True, native_passable=True,
                                  pending_entry='other',
                                  archive=[15, 15], ore=[[15, 15, 0, 0, 5]]), inputs)
    scene.step(dict(op='tick', frame=200))
    scene.run_until(lambda s: s['miner_nav'] == list(PAD))
    scene.step(dict(op='arrival'))
    scene.run_until(lambda s: s['unloading'] == 1)
    scene.step(dict(op='building_now_dead_contacts'))
    scene.step(dict(op='pointer_expiry'))
    # Capture the due post-exit decision, with the original stale owned list
    # still supplied. Whole House roster/owned-count cleanup is outside the VM.
    scene.step(dict(op='tick', frame=scene.read32(FRAME) + 1))
    return scene.finish()


def contact_vector_receipt(u, read32, building):
    """Read the actual Radio vector independently of BuildingType capacity."""
    items = read32(building + 0xE4)
    count = struct.unpack('<i', u.mem_read(building + 0xE8, 4))[0]
    assert count >= 0, count
    slots = [read32(items + index * 4) for index in range(count)]
    return dict(vector_address=hex(building + 0xE0),
                vector_bytes=bytes(u.mem_read(building + 0xE0, 16)).hex(),
                items_pointer=hex(items), count=count,
                slot_bytes=bytes(u.mem_read(items, count * 4)).hex() if count else '',
                slots=[name_of(pointer) for pointer in slots])


def actual_slot_controls(inputs):
    """Supplied actual slots test the original admission owner, not stock play.

    The type capacity deliberately differs from the vector size in several
    controls. Every query uses unchanged original65ADF0, CAN_LOAD and the
    narrow scanner; no initialization or resizing occurs during these steps.
    """
    rows = []
    pointers = {'miner': ACTOR, 'other': OTHER, None: 0}
    for name, docks, slots in (
            ('actual_slots_two_type_one', 1, ['other', None]),
            ('actual_slots_one_type_three', 3, ['other']),
            ('actual_slots_own_first', 1, ['miner']),
            ('actual_slots_own_later', 1, ['other', 'miner']),
            ('actual_slots_last_hole', 1, ['other', 'other', None]),
            ('actual_slots_full_type_three', 3, ['other', 'other']),
            ('actual_slots_empty_type_three', 3, []),
            ('actual_slots_first_hole', 1, [None, 'other']),
            ('actual_slots_middle_hole', 1, ['other', None, 'other'])):
        case = dict(name=name, busy=True, linked=False,
                    type_number_of_docks=docks, actual_slots=slots)
        scene = DockContinuation(case, inputs)
        u, r = scene.u, scene.read32
        u.mem_write(BTYPE + 0x1780, dwords(docks))
        u.mem_write(BLD + 0xE8, dwords(len(slots)))
        u.mem_write(BLD_ITEMS, dwords(*[pointers[name] for name in slots]) if slots else dwords(0))
        scene.before = scene.state()
        prepared = contact_vector_receipt(u, r, BLD)
        reads = []
        fields = {BTYPE + 0x1780: 'type_number_of_docks',
                  BLD + 0xE4: 'contact_items', BLD + 0xE8: 'actual_count',
                  **{BLD_ITEMS + index * 4: f'contact_slot_{index}'
                     for index in range(len(slots))}}

        def read(uc, access, address, size, value, data):
            if address in fields:
                reads.append(dict(phase=scene.phase, pc=hex(u.reg_read(UC_X86_REG_EIP)),
                                  field=fields[address], address=hex(address), size=size,
                                  bytes=bytes(u.mem_read(address, size)).hex()))

        hook = u.hook_add(UC_HOOK_MEM_READ, read)
        try:
            for operation in (dict(op='has_free_or_own'), dict(op='radio', message=15),
                              dict(op='find_bay', wide=0, bypass=0)):
                before_slots = contact_vector_receipt(u, r, BLD)
                scene.step(operation)
                after_slots = contact_vector_receipt(u, r, BLD)
                scene.steps[-1]['contact_vector_before'] = before_slots
                scene.steps[-1]['contact_vector_after'] = after_slots
                assert before_slots == after_slots
        finally:
            u.hook_del(hook)
        row = scene.finish()
        assert row['before']['scenario_rng'] == row['after']['scenario_rng']
        row.update(supplied_contact_inputs=dict(type_number_of_docks=docks,
                    type_count_address=hex(BTYPE + 0x1780),
                    type_count_bytes=bytes(u.mem_read(BTYPE + 0x1780, 4)).hex(),
                    actual_vector=prepared), contact_reads=reads,
                   contact_vector_after=contact_vector_receipt(u, r, BLD),
                   actual_slots_unchanged=True, full_scenario_rng_unchanged=True,
                   supplied_state_divergence_not_natural_stock_play=True)
        rows.append(row)
    return rows


def building_contact_constructor_controls():
    """Original Radio ctor plus Building's signed capacity clamp/count call.

    Reuse the existing native-reader allocator. Only the Building contact
    prefix executes: the surrounding Building construction/world registration
    is excluded. EAX supplies the prepared type and ESI the Radio-constructed
    Building object, as at43BCBD. The native count setter and vector resize run.
    """
    from tools.rules_oracle.bridge_landing_inputs import Landing
    from tools.spatial_oracle.shrapnel_repair.shrapnel_repair import rng_state
    rows = []
    for count in (-4, 0, 1, 3):
        machine = Landing()
        u, r = machine.u, machine.read32
        building, typ = machine.alloc(0x700), machine.alloc(0x1900)
        slot_start = machine.cursor
        u.mem_write(0xA8B230, dwords(SCENARIO))
        machine.invoke(0x65C6D0, SCENARIO + 0x218, [1])
        u.mem_write(typ + 0x1780, dwords(count))
        original_text = hashlib.sha256(bytes(u.mem_read(0x401000, 0x3E0000))).hexdigest()
        table_ranges = ((UNIT_VTABLE, 0x600), (0x7E3EBC, 0x600),
                        (0x7F0508, 0x300), (0x7E180C, 12))
        original_tables = [bytes(u.mem_read(address, size)) for address, size in table_ranges]
        resize = r(0x7E180C + 8)
        before_rng = rng_state(u, SCENARIO + 0x218)
        before = dict(contacts=contact_vector_receipt(u, r, building), scenario_rng=before_rng)
        calls, reads, writes = [], [], []
        phase = 'radio_constructor'

        def code(uc, pc, size, data):
            if u.reg_read(UC_X86_REG_EIP) == pc and pc in (0x65A750, 0x5B2DA0, 0x65AE60, resize):
                sp = u.reg_read(UC_X86_REG_ESP)
                calls.append(dict(phase=phase, pc=hex(pc), caller=hex(r(sp)),
                                  this=hex(u.reg_read(UC_X86_REG_ECX)),
                                  args=[r(sp + 4)] if pc == 0x65AE60 else
                                  [r(sp + 4), r(sp + 8)] if pc == resize else []))

        def read(uc, access, address, size, value, data):
            if address in (typ + 0x1780, building + 0xE0, building + 0xE4, building + 0xE8):
                reads.append(dict(phase=phase, pc=hex(u.reg_read(UC_X86_REG_EIP)),
                                  address=hex(address), size=size,
                                  bytes=bytes(u.mem_read(address, size)).hex()))

        def write(uc, access, address, size, value, data):
            if building + 0xE0 <= address < building + 0xF0 or slot_start <= address < machine.cursor:
                writes.append(dict(phase=phase, pc=hex(u.reg_read(UC_X86_REG_EIP)),
                                   address=hex(address), size=size, value=value))

        hooks = [u.hook_add(UC_HOOK_CODE, code), u.hook_add(UC_HOOK_MEM_READ, read),
                 u.hook_add(UC_HOOK_MEM_WRITE, write)]
        try:
            machine.invoke(0x65A750, building)
            radio_constructed = dict(contacts=contact_vector_receipt(u, r, building),
                                     scenario_rng=rng_state(u, SCENARIO + 0x218))
            phase = 'building_contact_prefix'
            u.reg_write(UC_X86_REG_EAX, typ)
            u.reg_write(UC_X86_REG_ESI, building)
            u.reg_write(UC_X86_REG_ESP, SP)
            run_checked(u, 0x43BCBD, 0x43BCD5,
                        required_addresses=(0x43BCBD, 0x65AE60, resize) if count > 1 else
                        (0x43BCBD, 0x65AE60))
        finally:
            for hook in hooks:
                u.hook_del(hook)
        assert u.reg_read(UC_X86_REG_ESP) == SP
        after = dict(contacts=contact_vector_receipt(u, r, building),
                     scenario_rng=rng_state(u, SCENARIO + 0x218))
        assert before_rng == radio_constructed['scenario_rng'] == after['scenario_rng']
        assert original_text == hashlib.sha256(bytes(u.mem_read(0x401000, 0x3E0000))).hexdigest()
        assert original_tables == [bytes(u.mem_read(address, size)) for address, size in table_ranges]
        rows.append(dict(input=dict(name=f'building_constructor_docks_{count}',
                                    type_number_of_docks=count, seed=1),
                         supplied_type_input=dict(address=hex(typ + 0x1780),
                                                  bytes=bytes(u.mem_read(typ + 0x1780, 4)).hex()),
                         before=before, after_radio_constructor=radio_constructed, after=after,
                         native_calls=calls, native_reads=reads, native_writes=writes,
                         original_vector_resize_entry=hex(resize),
                         original_stop='0x43bcd5', full_scenario_rng_unchanged=True,
                         original_code_and_vtables_unchanged=True, native_text_sha256=original_text))
    return rows


def generate():
    result = {'source': 'unicorn/gamemd.exe',
            'can_dock': [can_dock(case) for case in handshake_cases()],
            'radio': [radio(case) for case in radio_cases()],
            'mission_enter': [mission(case, ENTER) for case in enter_cases()],
            'mission_harvest': [mission(case, HARVEST) for case in harvest_cases()],
            'mission_unload': [mission(case, UNLOAD) for case in unload_cases()],
            'per_cell': [per_cell(case) for case in per_cell_cases()],
            'per_cell_release': [per_cell_release(case) for case in per_cell_release_cases()],
            'stage_tick': [stage_tick(case) for case in stage_cases()]}
    inputs = native_inputs()
    result.update(retail_inputs=inputs, continuation_controls=continuation_controls(inputs),
                  deposit_histories=[deposit_history(inputs), deposit_history(inputs, busy=True),
                                     destroyed_history(inputs)],
                  actual_slot_controls=actual_slot_controls(inputs),
                  building_contact_constructor_controls=building_contact_constructor_controls())
    return result


def metadata():
    return provenance(
        scope='Preserves107 supplied-scene legacy refinery executions. Adds57 original controls '
              'for current-mission Enter cadence, pending-target nearby continuation/admission, Building '
              'CAN_LOAD/DOCKING, signed Object health, UnitRepair DOCK_NOW, Hover/Drive per-cell '
              'docking and PowerOn release prefix; three retained stock HARV histories execute '
              'full cargo selection, admission, Enter, unload/payment, exit and Harvest resume, '
              'busy-slot retry and a bounded destroyed-building contact/pointer-expiry exit. '
              'Stock constructor/selected key/store/MissionControl/ART receipts, before/after '
              'full Scenario RNG, original calls and state writers are saved. Distinctive '
              'binary64 table inputs have independent original rate/ftol-prefix frame receipts '
              'before RNG, attached only to controls supplying those rates. UnitRepair '
              'controls independently query the prepared sparse dock inputs; a separate '
              'NADEPT retail ART DockingOffset0 receipt executes its original constructor '
              'and selected offset loop without transferring that stock pad into old scenes. '
              'Nine independent supplied actual-slot controls execute HasFreeOrOwn/CAN_LOAD/'
              'narrow scanner, distinct from type capacity; four original Radio constructor/'
              'Building contact-count prefixes pin signed capacity clamp and null-slot allocation.',
        entry_points={'transmit': TRANSMIT, 'building_receive': 0x43C2D0, 'unit_receive': 0x737430,
                      'foot_receive': 0x4D8FB0, 'techno_receive': 0x6F4AB0, 'radio_receive': 0x65A820,
                      'mission_enter': ENTER, 'mission_harvest': HARVEST, 'mission_unload': UNLOAD,
                      'per_cell_enter_arm': 0x73A31F, 'per_cell_release': 0x73ACB3,
                      'stage_tick': 0x6FABC4, 'mission_dispatch': DISPATCH,
                      'ready_to_commence': READY, 'unit_idle': IDLE, 'commence': COMMENCE,
                      'current_mission_control': 0x5B3A00, 'pending_nearby': 0x70D8F0,
                      'pending_entry_admission': 0x70D7E0,
                      'has_free_or_own': 0x65ADF0, 'radio_constructor': 0x65A750,
                      'contact_count_setter': 0x65AE60, 'contact_vector_resize': 0x40B9A0,
                      'building_contact_constructor_begin': 0x43BCBD,
                      'building_contact_constructor_stop': 0x43BCD5,
                      'supplied_rate_conversion_begin': 0x4D946C,
                      'supplied_rate_conversion_stop': 0x4D9481,
                      'original_ftol': 0x7C5F00,
                      'nearby_location': 0x703590, 'map_zone': 0x56D230,
                      'nearby_passable_cell': FNPC, 'find_docking_bay': FIND_DOCKING_BAY,
                      'object_receive': 0x5F5270, 'building_begin_mode_prefix': 0x447780,
                      'hover_constructor': 0x513C20, 'hover_get_class_id': 0x517070,
                      'locomotor_power_off': 0x55A910, 'locomotor_power_on': 0x55A8F0,
                      'repair_release_power_prefix': 0x44C35B,
                      'building_now_dead_contact_loop': 0x442511,
                      'unit_pointer_expired': POINTER_EXPIRED,
                      'strength_read': 0x5F94D3, 'unit_repair_read': 0x460906,
                      'dock_unload_read': 0x4609D6, 'refinery_read': 0x460A52,
                      'number_of_docks_read': 0x46492E, 'foundation_read': 0x461225,
                      'docking_offset_read_loop': 0x46499D, 'docking_offset_reader': 0x529CA0,
                      'building_center': 0x447AC0, 'building_dock_coord': 0x447B20,
                      'queueing_cell_read': 0x4614EE, 'tiberium_value_read': 0x721AFA,
                      'dump_rate_read': 0x670CC0, 'purifier_bonus_read': 0x66FC56,
                      'too_far_read': 0x66FFD7,
                      'assign_destination': ASSIGN, 'drive_do_turn': DO_TURN,
                      'give_tiberium': GIVE_TIBERIUM, 'random_ranged': RANDOM},
        assumptions=[
            'Reuses the existing harvest_field/refinery_dock/track_destination map, Drive, radio, ore and native-CRC reader owners. Unit/House/Scenario/map membership and constructor prestates are supplied; original Drive/Facing/Rules/UnitType/BuildingType constructors and Scenario seeder execute. Native text and Unit/Building vtables are hash-checked unchanged in every new retained VM.',
            'Original selected reads use physical RULESMD.INI, optional LANGRULE.INI, MPBattleMD.ini and XMP03T4.MAP lexical strings in existing native CRC caches, in that order. ARTMD is a separate nonlayered physical input. Selected Image==typeID is supplied; original dual Foundation, QueueingCell and NADEPT DockingOffset0 blocks execute. Original BuildingType constructor writes one zeroed allocated dock slot; count1 retains that slot at the original resize gate. Full INI/MIX IO, full surrounding type read/asset passes and general Image precedence are excluded.',
            'New stock HARV Strength1000, NAREFN/GAREFN1000, NADEPT1200 and additional flags, capacities, General fields, tiberium Values and MissionControl bytes transfer from independent original reader output. Legacy Strength900/1000 and controls overriding rates/health/type flags are controlled inputs, not stock claims.',
            'The selected stock Dock vector resolves two native names; the retained scene supplies one eligible NAREFN pointer/ordinal0 and House counter1, with ordered supplied House building slots. NADEPT controls supply its known repair target. Every refinery foundation cell uses the existing supplied first-object placement; the second radio Building is not foundation-placed.',
            'Original Begin_Mode prefix installs active BState1. Pending nearby controls supply uniform sector0/zone0 arrays; live controls run original GetZone/Nearby/FNPC. Invalid/no-cell controls distinguish native ObjectAlive and Techno flag from signed health.',
            'Nine pending-admission controls execute original Foot70D7E0, HELLO/CAN_LOAD, Queue5B35E0, actual UnitReady744270/Commence5B3570 and class destination setter741970. Parked Guard/Sleep and moving Move callers, already linked targets, foundation denial and busy HELLO refusal use prepared UnitRepair scenes. The native raw helper EAX/AL is recorded as non-semantic for the Rust void owner. No readiness reply is supplied. New input/read receipts and return-PC/stack-frame-matched calls disambiguate nested sends; older generic event return annotations are retained and are not the new rows\' authoritative radio replies.',
            'Nine separate actual-slot controls deliberately supply type NumberOfDocks independently of the actual Radio vector size/items. Original65ADF0, narrow CAN_LOAD43C366 and narrow scanner4DEF0C query those actual slots; own contacts and first/middle/last sparse holes are retained. Query steps preserve all actual slots and full RNG. This is supplied-state divergence, not a natural stock-play history. Rust consumers must seed actual Contacts from input.actual_slots rather than rebuild them from type capacity.',
            'Four contact constructor controls use the existing Landing heap and execute full original Radio65A750, then Building43BCBD..43BCD5 with a supplied BuildingType pointer in EAX and object in ESI. Original65AE60 and virtual vector resize40B9A0 execute; signed type capacities -4/0/1/3, actual count/null slots, field reads/writes and full RNG are recorded. The surrounding full Building constructor, world registration and destruction are excluded.',
            'Distinctive controls write supplied binary64 Rate fields directly. A separate VM executes original current-control lookup and Enter FLD/FMUL/ftol prefix4D946C..4D9481 using the inherited supplied x87 control word, stopping before RNG. Its signed frame result seeds only test-fixture MissionControl projection; it is not derived from Enter expected returns or Rust arithmetic. Raw field/multiplier bytes, control word and unchanged native text/vtables/RNG are recorded.',
            'Every UnitRepair control independently records its prepared count1, NULL offset-items pointer and mapped page0 offset[-1,0,0]. Original GetCoords447AC0 yields[2048,2688,0]; GetDockCoord447B20 reads that offset and yields[2047,2688,0], cell[7,10]. This is supplied sparse input, distinct from the original-read retail NADEPT ART offset[128,0,0]. Rust fixtures project the supplied offset through the existing ART pad owner; production retains its stock reader input.',
            'PerCell snippets supply their documented prologue locals. Stock histories supply physical arrivals at native Cell NavCom and terminate Drive vectors before executing original destination/per-cell cleanup; full path/track traversal, complete object AI, whole Scenario scheduling and external interleavings are excluded.',
            'NowDead contact loop supplies captured vector locals and already dead Building prestate, then original Unit pointer expiry. It does not execute whole-world DetachAll, House roster/type counts, foundation removal or destructor; retained tethers/building slot after this prefix are not final cleanup claims. Depot PowerOn prefix supplies full health as the external repair-completion boundary.'
        ],
        substitutions=[
            'The independent contact constructor controls reuse the existing reader allocator: operator_new returns bump storage, operator_delete is a no-op and CRT TLS storage is supplied. Native Radio initialization, count clamp, resize/copy and null-slot stores execute; no original text or vtable is patched.',
            'Legacy107 rows keep observed no-effect Unit Scatter/idle/animations and supplied Ready/Bay/FNPC answers exactly. New rows execute original idle, readiness and bay selection; only the destruction history executes reached original Scatter bodies. Animation producers and tactical redraw/occupancy-cache sinks remain observed no-effect callbacks.',
            'Inherited CanReachZone56D100 answers the scene unreachable list (default all admitted), preserving all arguments; zone/path producers are excluded. Pending no-cell control supplies only FNPC returnNULL. Native GetZone, Nearby wrapper, pending helper, destination setter and Enter cadence still execute.',
            'The arbitrary stock map scene, two physical arrivals and depot full-health completion are declared external inputs. No Rust result or hand-calculated payout/timer/RNG supplies native goldens.'
        ])


def source_paths():
    from tools.spatial_oracle.harvest_attack_return import source_paths as shared_sources
    paths = shared_sources()
    paths['shared_harv_reader'] = paths.pop('generator')
    return paths | {'generator': Path(__file__)}


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'),
                   provenance=metadata, source_paths=source_paths())
