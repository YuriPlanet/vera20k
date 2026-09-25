"""Original refinery dock: radio handshake, Mission_Enter and Mission_Unload.

Run python -m tools.spatial_oracle.refinery_dock --check (or --write).

A War Miner (the real Unit vtable over a constructed Drive locomotor) and a
stock refinery (the real Building vtable over a supplied BuildingTypeClass
with DockUnload=/Refinery=, Foundation 4x3) sit on the 32x32 source-scatter
map. The refinery's NW cell is (6,9), so its pad is (9,10) and the miner's
default cell (10,10) is the art QueueingCell=4,1 east of it.

Everything runs the original bytes: the radio core (0x65A970/0x65A820), the
Building/Unit/Foot/Techno receivers, Unit Assign_Destination with the Drive
MoveTo, FacingClass and Drive Do_Turn, Queue_Mission/Commence and the
Scenario RandomRanged. Observed at entry and returned without effect: Unit
Scatter (0x743A50), Enter_Idle_Mode (0x738970) and the refinery animation
producers (PlayNthAnim 0x451750, DestroyNthAnim 0x451E40, smoke 0x459900).
Unit Ready_To_Commence (0x744270) answers the row's supplied value.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance
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


def place_building(u, building, nw):
    # Primary and RTTI (What_Am_I) vtables, as the constructor writes them (0x43B725).
    u.mem_write(building, dwords(0x7E3EBC, 0x7E3EA0))
    u.mem_write(building + 0x14, dwords(1))
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
    u.mem_write(ACTOR + 0x5A4, dwords(cell(*nav) if nav else 0))
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
            events.append(['assign_destination', name_of(this), cell_xy(read32(sp + 4)), read32(sp + 8)])
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
            ret(12)
        elif address == IDLE:
            events.append(['enter_idle_mode', read32(sp + 4), read32(sp + 8)])
            ret(8)
        elif address == READY:
            assert ready, ('unsupplied Ready_To_Commence', case)
            answer = ready.pop(0)
            events.append(['ready', answer])
            ret(0, answer)
        elif address == FIND_DOCKING_BAY:
            assert bays, ('unsupplied Find_Docking_Bay', case)
            bay = {'refinery': BLD, 'other': OTHER, None: 0}[bays.pop(0)]
            events.append(['find_docking_bay', read32(sp + 8), read32(sp + 12), read32(0xA8E7AC),
                           name_of(bay)])
            ret(12, bay)
        elif address == FNPC:
            out, query = read32(sp + 4), read32(sp + 8)
            seed = list(struct.unpack('<hh', u.mem_read(query, 4)))
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
        miner_nav=cell_xy(read32(ACTOR + 0x5A4)),
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


def generate():
    return {'source': 'unicorn/gamemd.exe',
            'can_dock': [can_dock(case) for case in handshake_cases()],
            'radio': [radio(case) for case in radio_cases()],
            'mission_enter': [mission(case, ENTER) for case in enter_cases()],
            'mission_harvest': [mission(case, HARVEST) for case in harvest_cases()],
            'mission_unload': [mission(case, UNLOAD) for case in unload_cases()],
            'per_cell': [per_cell(case) for case in per_cell_cases()],
            'per_cell_release': [per_cell_release(case) for case in per_cell_release_cases()],
            'stage_tick': [stage_tick(case) for case in stage_cases()]}


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='107 original executions of the War Miner refinery dock: 19 Building 0x0E (DockUnload) '
              'handshakes and 18 single transmits through the radio core with every nested '
              'receiver; 9 FootClass::Mission_Enter, 9 UnitClass::Mission_Harvest (states 2/3) '
              'and 30 UnitClass::Mission_Unload (harvester branch) dispatches with their return '
              'value and Scenario draws; 6 Unit Per_Cell_Process(2) Enter-arm snippets and 13 of '
              'its Ready/Commence and Refinery=/Weeder= contact release; 3 TechnoClass::AI '
              'StageClass tick runs.',
        entry_points={'transmit': TRANSMIT, 'building_receive': 0x43C2D0, 'unit_receive': 0x737430,
                      'foot_receive': 0x4D8FB0, 'techno_receive': 0x6F4AB0, 'radio_receive': 0x65A820,
                      'mission_enter': ENTER, 'mission_harvest': HARVEST, 'mission_unload': UNLOAD,
                      'per_cell_enter_arm': 0x73A31F, 'per_cell_release': 0x73ACB3,
                      'stage_tick': 0x6FABC4,
                      'assign_destination': ASSIGN, 'drive_do_turn': DO_TURN,
                      'give_tiberium': GIVE_TIBERIUM, 'random_ranged': RANDOM},
        assumptions=['track_destination fixture: original Unit vtable over a constructed Drive, 32x32 map, House, Rules; supplied Unit/Radio/Foot constructor prestates; Scenario RNG seeded through the original seeder.',
                     'Refinery: original Building vtable 0x7E3EBC over supplied BuildingClass fields (+14, +6C, +9C, +AC, +B4, +E0 contacts, +21C, +418, +520, +57C, +584, +660) and BuildingTypeClass (+A0, +EF0=12 4x3, +1618 QueueingCell 4,1, +16B3, +16BB, +1780). It is the first object of every foundation cell (Place_Down 0x5683C0 over the 12-cell 4x3 Occupy_List). A second refinery supplies the third radio object.',
                     'Retail MissionControl Rate values for Guard/Enter/Harvest/Unload; Rules HarvesterTooFarDistance 5/50, PurifierBonus .25f, HarvesterDumpRate .016, ConditionYellow .5, AIVirtualPurifiers 4,2,0; Tiberium Values 25/50/25/25; House storage, economy and owned-type counter supplied.',
                     'Per_Cell_Process runs from 0x73A31F with its prologue locals supplied ([esp+14] Contact(0), [esp+1C] Get_Cell); its release snippet runs 0x73ACB3..0x73ADCA, which reads no local.',
                     'Harvester= (+0xE0E), Weeder= (+0xE0F), Refinery= (+0x16BB) and Weeder= (+0x16BC) as each row gives them.'],
        substitutions=['Unit Scatter, Enter_Idle_Mode and the refinery animation producers (PlayNthAnim, DestroyNthAnim, refinery smoke) are observed and return without effect.',
                       'Unit Ready_To_Commence answers the row\'s supplied value; Find_Docking_Bay answers the supplied bay per call; Find_Nearby_Passable_Cell answers the supplied cell.']))
