"""Original Chrono Miner (CMIN) refinery return: setter arm, warp, Harvest and Enter.

Run python -m tools.spatial_oracle.cmin_dock --check (or --write).

The refinery_dock fixture (War Miner, refinery NW (6,9), pad (9,10), 32x32
map) turned into a CMIN: TechnoType+0xCD4 Teleporter=1, MovementZone Crusher
(+0x5B4, retail [CMIN]), a TeleportLocomotionClass from its original
constructor 0x718000 linked by the original Link_To_Object and installed as
the Foot's ILocomotion (+0x674). A `drive_piggy` row runs the original Drive
IPiggyback Begin_Piggyback so the fixture Drive carries the Teleport in its
stash. The miner is on the map (ObjectClass+0x74), BridgeHeight comes from its
original initializer, and Rules carry the retail chrono values.

The rows run the original Unit setter 0x741970 with its Teleporter arm, the
Foot setter, COM QueryInterface/AddRef/Release and IPiggyback, Teleport
Move_To/destination/Stop_Moving/Process/Do_Turn and its delay tick, Drive
Move_To/Stop_Moving, Unit Can_Enter_Cell, the occupy-bit writers, Mark with
the map place/remove, Mission_Harvest, Mission_Enter, the radio core with every
receiver, and the FootClass::AI piggyback END step (0x4DAE5F..0x4DAEC6).

Events are native calls in order. 'reserve'/'unreserve' are the Unit
vt+0xF0/+0xF4 writers of cell+0x124 bit 0x20; 'distance' is the ftol result
each threshold compares. Recorded and returned without running: the
CoCreateInstance wrapper 0x41C250 (it runs the original Drive constructor and
AddRef, then returns S_OK with that ILocomotion), operator new/delete, the
AnimClass constructor, VocClass::PlayAt, crate pickup, Unit Per_Cell_Process,
Search_For_Tiberium and the MapClass zone lookup.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
                               UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI,
                               UC_X86_REG_ESP)
from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle import refinery_dock as dock
from tools.spatial_oracle.refinery_dock import (ACTOR, LOCO, BLD, OTHER, MINER_ITEMS, BLD_ITEMS,
                                                RULES, PAD, cell, cell_xy)
from tools.spatial_oracle.unit_entry import EXTRA
from tools.spatial_oracle.unit_scatter_state import TYPE, SP

TELE = EXTRA + 0x20000
DRIVES = [EXTRA + 0x20400 + index * 0x100 for index in range(4)]
ANIMS = [EXTRA + 0x21000 + index * 0x200 for index in range(4)]
WARPOUT, PAD_UNIT = EXTRA + 0x22000, EXTRA + 0x23000
STUB_CTOR, STUB_ADDREF = RET_MAGIC + 0x800, RET_MAGIC + 0x810
CHRONO_IN, CHRONO_OUT, ZONE = 0x41, 0x42, 7

TELE_CTOR, DRIVE_CTOR, LINK, BRIDGE_HEIGHT = 0x718000, 0x4AF540, 0x55A710, 0x7352F0
TELE_MOVE_TO, TELE_STOP, TELE_PROCESS, TELE_DO_TURN = 0x718100, 0x718230, 0x7192F0, 0x7192C0
TELE_DELAY_TICK = 0x719BF0
DRIVE_MOVE_TO, DRIVE_STOP, DRIVE_ADDREF = 0x4AFD40, 0x4AFE00, 0x4B4DA0
DRIVE_BEGIN, DRIVE_END, TELE_BEGIN, TELE_END = 0x4AF8E0, 0x4AF930, 0x719E90, 0x719EE0
COCREATE, OP_NEW, OP_DELETE, COM_ERROR = 0x41C250, 0x7C8E17, 0x7C8B3D, 0x7DC720
ANIM_CTOR, PLAY_AT, PER_CELL, CRATE = 0x421EA0, 0x7509E0, 0x739EC0, 0x481A00
ZONE_TYPE, CAN_ENTER, TELE_FNPC_CALL, SEARCH_ORE = 0x56D230, 0x73F0A0, 0x719185, 0x4DCFE0
ASSIGN_MISSION, RAW_CLEAR, RESERVE, UNRESERVE = 0x5B2FD0, 0x4DF0D0, 0x7441B0, 0x744210
MARK, MAP_PLACE, MAP_REMOVE = 0x4D3780, 0x5683C0, 0x5687F0
TAIL_BEGIN, TAIL_END = 0x4DAE5F, 0x4DAEC6
DISTANCE_PROBES = {0x73EE3A: 'harvest_narrow', 0x73ECD0: 'harvest_wide', 0x7194AC: 'teleport'}
DRIVE_CLSID = 0x7E9A30
ENTER, HARVEST, UNLOAD = dock.ENTER, dock.HARVEST, dock.UNLOAD
PER_CELL_ARM, PER_CELL_ARM_END = 0x73A31F, 0x73A5EA


def make_cmin_fixture(case):
    u, call, read32 = dock.make_dock_fixture(case)
    # BridgeHeight (the occupy-bit writers compare against it): the original
    # initializer from the established level height 104.
    u.mem_write(0xB1D0B8, dwords(104))
    call(BRIDGE_HEIGHT, 0, [])
    assert read32(0xB1D0AC) == 416
    u.mem_write(ACTOR + 0x74, b'\x01')  # IsOnMap: Mark removes and places it
    u.mem_write(TYPE + 0xCD4, bytes([case.get('teleporter', True)]))
    u.mem_write(TYPE + 0xE0E, bytes([case.get('harvester', True)]))
    u.mem_write(TYPE + 0x5B4, dwords(1))  # MovementZone=Crusher (retail [CMIN])
    u.mem_write(TYPE + 0x67C, dwords(2))  # SpeedType: a fixture value distinct from it
    u.mem_write(TYPE + 0x574, dwords(CHRONO_IN, CHRONO_OUT))
    # Teleport: original constructor and Link_To_Object; the Foot holds its reference.
    call(TELE_CTOR, TELE, [])
    call(LINK, 0, [TELE + 4, ACTOR])
    u.mem_write(TELE + 0x14, dwords(1))
    u.mem_write(ACTOR + 0x674, dwords(TELE + 4))
    # Rules: ChronoDelay, ChronoDistanceFactor, ChronoTrigger, ChronoMinimumDelay,
    # ChronoRangeMinimum, ChronoHarvTooFarDistance (10 cells, not the retail 50,
    # so both sides of the threshold fit the map) and WarpOut.
    u.mem_write(RULES + 0xBEC, dwords(60))
    u.mem_write(RULES + 0xBF4, dwords(48))
    u.mem_write(RULES + 0xBF8, b'\x01')
    u.mem_write(RULES + 0xBFC, dwords(16, case.get('range_minimum', 0)))
    u.mem_write(RULES + 0xD7C, dwords(10))
    u.mem_write(RULES + 0x33C, dwords(WARPOUT))
    u.mem_write(RULES + 0x177C, dwords(48 * 256))  # TiberiumLongScan=48 (leptons)
    if 'miner_coord' in case:
        u.mem_write(ACTOR + 0x9C, dwords(*case['miner_coord']))
    if case.get('pad_unit'):
        # A second Unit listed first in the pad cell (0x47EBA0 finds it), ahead
        # of the refinery: Occupy_Down (0x47E8A0) prepends a non-building.
        u.mem_write(PAD_UNIT, dwords(0x7F5C70))
        u.mem_write(PAD_UNIT + 0x14, dwords(5))
        u.mem_write(PAD_UNIT + 0x30, dwords(read32(cell(*PAD) + 0xE4)))
        u.mem_write(cell(*PAD) + 0xE4, dwords(PAD_UNIT))
    for x, y in case.get('reserved', []):
        u.mem_write(cell(x, y) + 0x124, dwords(0x20))
    u.mem_write(ACTOR + 0x270, bytes([case.get('warp_out', False), case.get('warp_in', False)]))
    u.mem_write(ACTOR + 0x27C, bytes([case.get('latch_27c', False)]))
    u.mem_write(ACTOR + 0x2B0, dwords(OTHER if case.get('lifted') else 0))
    u.mem_write(ACTOR + 0x6AD, bytes([case.get('swap_6ad', False)]))
    u.mem_write(ACTOR + 0x1F8, bytes([case.get('force_reassign', False)]))
    u.mem_write(ACTOR + 0x6D8, dwords(case.get('deploy_6d8', -1)))
    if case.get('locked'):
        u.mem_write(ACTOR + 0x6A0, dwords(case.get('frame', 200), 0, 30))
    if case.get('loco') == 'drive_piggy':
        # The fixture Drive at LOCO piggybacks over the Teleport: original
        # Link_To_Object and Drive IPiggyback Begin_Piggyback. The stash then
        # holds the Teleport's only reference and the Foot holds the Drive's.
        call(LINK, 0, [LOCO + 4, ACTOR])
        call(DRIVE_BEGIN, 0, [LOCO + 0x18, TELE + 4])
        assert read32(LOCO + 0x68) == TELE + 4
        u.mem_write(TELE + 0x14, dwords(1))
        u.mem_write(ACTOR + 0x674, dwords(LOCO + 4))
    return u, call, read32


def loco_name(pointer):
    for base, name in [(TELE, 'teleport'), (LOCO, 'drive')] + [
            (drive, f'drive{index + 1}') for index, drive in enumerate(DRIVES)]:
        if base <= pointer < base + 0x100:
            return name
    assert pointer == 0, hex(pointer)
    return None


def name_of(pointer):
    return 'pad_unit' if pointer == PAD_UNIT else dock.name_of(pointer)


def observe(u, read32, case):
    """refinery_dock's observers plus the CMIN hooks; returns (events, unused)."""
    events, unused = dock.observe_dock(u, read32, case)
    ore = list(case.get('ore', []))
    # harvest_field runs the original Search_For_Tiberium_And_Move instead.
    native_search = case.get('native_search', False)
    drives, pending, returns = list(DRIVES), [], {}

    def ret(cleanup, value=0):
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value)
        u.reg_write(UC_X86_REG_EIP, read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    def coords(address):
        return list(struct.unpack('<3i', u.mem_read(address, 12)))

    def cell_arg(address):
        return list(struct.unpack('<hh', u.mem_read(address, 4)))

    def hook(_u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        this = u.reg_read(UC_X86_REG_ECX)
        if address in returns:
            events[returns.pop(address)].append(u.reg_read(UC_X86_REG_EAX))
        if address in DISTANCE_PROBES:
            events.append(['distance', DISTANCE_PROBES[address], u.reg_read(UC_X86_REG_EAX)])
        if address == COCREATE:
            # _com_ptr_t::CreateInstance(clsid, outer, context): run the original
            # Drive constructor, then its AddRef, then store the ILocomotion.
            assert bytes(u.mem_read(read32(sp + 4), 16)) == bytes(u.mem_read(DRIVE_CLSID, 16))
            assert read32(sp + 8) == 0
            drive = drives.pop(0)
            events.append(['cocreate', loco_name(drive), read32(sp + 12)])
            pending.append((this, drive))
            u.mem_write(sp - 4, dwords(STUB_CTOR))
            u.reg_write(UC_X86_REG_ESP, sp - 4)
            u.reg_write(UC_X86_REG_ECX, drive)
            u.reg_write(UC_X86_REG_EIP, DRIVE_CTOR)
        elif address == STUB_CTOR:
            drive = pending[-1][1]
            u.mem_write(sp - 8, dwords(STUB_ADDREF, drive + 4))
            u.reg_write(UC_X86_REG_ESP, sp - 8)
            u.reg_write(UC_X86_REG_EIP, DRIVE_ADDREF)
        elif address == STUB_ADDREF:
            smart, drive = pending.pop()
            u.mem_write(smart, dwords(drive + 4))
            ret(12, 0)
        elif address in (TELE_MOVE_TO, DRIVE_MOVE_TO):
            events.append(['move_to', loco_name(read32(sp + 4)), coords(sp + 8)])
        elif address in (TELE_STOP, DRIVE_STOP):
            events.append(['stop_moving', loco_name(read32(sp + 4))])
        elif address == TELE_DO_TURN:
            events.append(['do_turn', read32(sp + 8) & 0xFFFF])
        elif address in (DRIVE_BEGIN, TELE_BEGIN):
            events.append(['begin_piggyback', loco_name(read32(sp + 4)), loco_name(read32(sp + 8))])
        elif address in (DRIVE_END, TELE_END):
            events.append(['end_piggyback', loco_name(read32(sp + 4))])
        elif address == TELE_DELAY_TICK:
            events.append(['teleport_delay_tick'])
        elif address == ASSIGN_MISSION:
            events.append(['assign_mission', name_of(this),
                           struct.unpack('<i', dwords(read32(sp + 4)))[0]])
        elif address == RAW_CLEAR:
            events.append(['clear_nav', name_of(this)])
        elif address in (RESERVE, UNRESERVE):
            x, y, _ = coords(read32(sp + 4))
            events.append(['reserve' if address == RESERVE else 'unreserve', [x >> 8, y >> 8]])
        elif address == MARK:
            events.append(['mark', name_of(this), read32(sp + 4)])
        elif address in (MAP_PLACE, MAP_REMOVE):
            events.append(['map_place' if address == MAP_PLACE else 'map_remove',
                           cell_arg(read32(sp + 4)), name_of(read32(sp + 8))])
        elif address == CAN_ENTER:
            returns[read32(sp)] = len(events)
            events.append(['can_enter_cell', cell_xy(read32(sp + 4)),
                           [struct.unpack('<i', dwords(read32(sp + 8 + i * 4)))[0] for i in range(4)]])
        elif address == ZONE_TYPE:
            events.append(['movement_zone_type', cell_arg(read32(sp + 4)), read32(sp + 8),
                           read32(sp + 12) & 0xFF, ZONE])
            ret(12, ZONE)
        elif address == TELE_FNPC_CALL:
            # All fifteen Find_Nearby_Passable_Cell arguments at 0x718B70's call.
            args = [read32(sp + i * 4) for i in range(15)]
            args[5] &= 0xFF  # a bool pushed as the whole [esp+10] dword
            events.append(['teleport_fnpc_args', cell_arg(args[1]), args[2:12]])
        elif address == SEARCH_ORE and not native_search:
            assert ore, ('unsupplied Search_For_Tiberium', case)
            answer = ore.pop(0)
            events.append(['search_for_tiberium', read32(sp + 4), read32(sp + 8) & 0xFF,
                           cell_xy(read32(ACTOR + 0x5A4)), answer])
            ret(8, answer)
        elif address == OP_NEW:
            assert read32(sp + 4) == 0x1C8, hex(read32(sp + 4))
            ret(0, ANIMS[sum(1 for event in events if event[0] == 'anim')])
        elif address == ANIM_CTOR:
            kind = 'warpout' if read32(sp + 4) == WARPOUT else hex(read32(sp + 4))
            events.append(['anim', kind, coords(read32(sp + 8)),
                           [read32(sp + 12 + i * 4) for i in range(5)]])
            ret(0x1C, this)
        elif address == PLAY_AT:
            events.append(['sound', this, coords(u.reg_read(UC_X86_REG_EDX)), read32(sp + 4)])
            ret(4)
        elif address == PER_CELL:
            events.append(['per_cell_process', name_of(this), read32(sp + 4)])
            ret(4, 0)
        elif address == CRATE:
            events.append(['crate', cell_xy(this), name_of(read32(sp + 4))])
            ret(4, 0)
        elif address == OP_DELETE:
            events.append(['delete', loco_name(read32(sp + 4))])
            ret(0)
        elif address == COM_ERROR:
            raise AssertionError(('_com_issue_error', hex(read32(sp + 4)), case))

    u.hook_add(UC_HOOK_CODE, hook)
    return events, (*unused, ore)


def state(u, read32):
    signed = lambda address: struct.unpack('<i', u.mem_read(address, 4))[0]
    byte = lambda address: u.mem_read(address, 1)[0]
    coords = lambda address: list(struct.unpack('<3i', u.mem_read(address, 12)))
    active = read32(ACTOR + 0x674)
    # The object by identity and its class by ILocomotion vtable (a destroyed
    # object would show its base vtable instead).
    locomotor = dict(active=loco_name(active),
                     kind={0x7F5000: 'teleport', 0x7E7EB0: 'drive'}.get(read32(active), hex(read32(active))))
    if locomotor['active'] != 'teleport':
        base = active - 4
        locomotor.update(stash=loco_name(read32(base + 0x68)), destination=coords(base + 0x34),
                         head=coords(base + 0x40), refs=signed(base + 0x14))
    cells = [(x, y) for y in range(32) for x in range(32)]
    return dict(
        mission=signed(ACTOR + 0xAC), queued=signed(ACTOR + 0xB4), status=signed(ACTOR + 0xBC),
        nav=cell_xy(read32(ACTOR + 0x5A4)), aux=cell_xy(read32(ACTOR + 0x5A0)),
        skip_move_to=byte(ACTOR + 0x6AC), force_reassign=byte(ACTOR + 0x1F8),
        locomotor=locomotor,
        teleport=dict(destination=coords(TELE + 0x1C), resolved=coords(TELE + 0x28),
                      moving=byte(TELE + 0x34), timer=[signed(TELE + 0x3C), signed(TELE + 0x44)],
                      stash=loco_name(read32(TELE + 0x48)), refs=signed(TELE + 0x14)),
        location=coords(ACTOR + 0x9C), on_bridge=byte(ACTOR + 0x8C),
        warp=[byte(ACTOR + 0x270), byte(ACTOR + 0x271), signed(ACTOR + 0x280)],
        reserved=[[x, y] for x, y in cells if read32(cell(x, y) + 0x124) & 0x20],
        reserved_deck=[[x, y] for x, y in cells if read32(cell(x, y) + 0x128) & 0x20],
        miner_contact=name_of(read32(MINER_ITEMS)), refinery_contact=name_of(read32(BLD_ITEMS)),
        miner_tether=byte(ACTOR + 0x418), refinery_tether=byte(BLD + 0x418),
        facing=dict(desired=read32(ACTOR + 0x388) & 0xFFFF, start=read32(ACTOR + 0x38C) & 0xFFFF,
                    timer_start=signed(ACTOR + 0x390), duration=signed(ACTOR + 0x398)),
        refinery_queued=signed(BLD + 0xB4),
        dispatch_timer=[signed(ACTOR + 0xC8), signed(ACTOR + 0xD0)],
    )


def assign(case):
    """Unit setter 0x741970 as vt+0x480(dest, flag) on the CMIN (ecx=miner)."""
    u, call, read32 = make_cmin_fixture(case)
    events, unused = observe(u, read32, case)
    dest = case.get('dest')
    call(dock.ASSIGN, ACTOR, [cell(*dest) if dest else 0, case.get('flag', 1)])
    assert not any(unused), (case, unused)
    return dict(input=case, events=events, state=state(u, read32))


def move_to(case):
    """Teleport ILocomotion Move_To 0x718100 called directly with a cell centre."""
    u, call, read32 = make_cmin_fixture(case)
    events, unused = observe(u, read32, case)
    x, y = case['dest']
    call(TELE_MOVE_TO, 0, [TELE + 4, x * 256 + 128, y * 256 + 128, 0])
    assert not any(unused), (case, unused)
    return dict(input=case, events=events, state=state(u, read32))


def piggyback_end_step(u, call, read32):
    """FootClass::AI's tail 0x4DAE5F..0x4DAEC6 (Is_Ok_To_End, then END), then its
    Release of the IPiggyback reference (0x4DAEFA)."""
    frame = SP - 0x100
    u.mem_write(frame + 0x14, dwords(0))
    u.reg_write(UC_X86_REG_ESI, ACTOR)
    u.reg_write(UC_X86_REG_EBX, 0)
    u.reg_write(UC_X86_REG_ESP, frame)
    run_checked(u, TAIL_BEGIN, TAIL_END, count=100000)
    piggy = u.reg_read(UC_X86_REG_EDI)
    if piggy:
        call(read32(read32(piggy) + 8), 0, [piggy])


def process(case):
    """Teleport Move_To (the arming, reported separately), then per frame the
    active Teleport's Process 0x7192F0 and the FootClass::AI END step."""
    u, call, read32 = make_cmin_fixture(case)
    events, unused = observe(u, read32, case)
    x, y = case['dest']
    call(TELE_MOVE_TO, 0, [TELE + 4, x * 256 + 128, y * 256 + 128, 0])
    arm, ticks = events[:], []
    for frame in case.get('frames', [200]):
        u.mem_write(0xA8ED84, dwords(frame))
        assert read32(ACTOR + 0x674) == TELE + 4, 'Teleport is not the active locomotor'
        start = len(events)
        call(TELE_PROCESS, 0, [TELE + 4])
        result = u.reg_read(UC_X86_REG_EAX) & 0xFF
        middle = len(events)
        piggyback_end_step(u, call, read32)
        ticks.append(dict(frame=frame, result=result, events=events[start:middle],
                          end_step=events[middle:], state=state(u, read32)))
    assert not any(unused), (case, unused)
    return dict(input=case, arm=arm, ticks=ticks)


def mission(case, entry):
    """One original mission handler dispatch on the CMIN (ecx=miner)."""
    u, call, read32 = make_cmin_fixture(case)
    events, unused = observe(u, read32, case)
    call(entry, ACTOR, [])
    delay = struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0]
    assert not any(unused), (case, unused)
    return dict(input=case, delay=delay, events=events, state=state(u, read32))


def per_cell(case):
    """Unit Per_Cell_Process(2)'s Enter arm 0x73A31F..0x73A5EA with the Teleport
    active; prologue locals supplied as in refinery_dock.per_cell."""
    u, call, read32 = make_cmin_fixture(case)
    events, unused = observe(u, read32, case)
    x, y = case.get('miner_cell', [10, 10])
    frame = SP - 0x100
    u.mem_write(frame + 0x14, dwords(read32(MINER_ITEMS)))
    u.mem_write(frame + 0x1C, struct.pack('<hh', x, y))
    u.reg_write(UC_X86_REG_EBP, ACTOR)
    u.reg_write(UC_X86_REG_ESP, frame)
    run_checked(u, PER_CELL_ARM, PER_CELL_ARM_END, count=200000)
    assert not any(unused), (case, unused)
    return dict(input=case, events=events, state=state(u, read32))


def assign_cases():
    pad = dict(dest=list(PAD))
    far = dict(dest=[14, 12], linked=False)
    return [
        dict(pad, name='pad_teleport_active'),
        dict(pad, name='pad_unit_present', pad_unit=True),
        dict(far, name='no_contact'),
        dict(pad, name='contact_not_dockunload', dock_unload=False),
        dict(pad, name='contact_other_cell', dest=[14, 12]),
        dict(name='null_dest_in_contact', dest=None, nav=[12, 12]),
        dict(name='null_dest_no_nav', dest=None),
        dict(pad, name='drive_piggy_stopped', loco='drive_piggy'),
        dict(pad, name='drive_piggy_moving', loco='drive_piggy', nav=[12, 12], moving=True),
        dict(far, name='drive_piggy_no_contact', loco='drive_piggy'),
        dict(pad, name='force_same_nav', nav=list(PAD), force_reassign=True),
        dict(pad, name='same_nav_no_force', nav=list(PAD)),
        dict(far, name='latch_27c', latch_27c=True),
        dict(far, name='lifted_2b0', lifted=True),
        dict(far, name='swap_6ad', swap_6ad=True),
        dict(pad, name='pad_cannot_enter', reserved=[list(PAD)], passable=[[8, 11]]),
        dict(pad, name='pad_cannot_enter_no_cell', reserved=[list(PAD)], passable=[None]),
    ]


def move_to_cases():
    pad = dict(dest=list(PAD), nav=list(PAD))
    return [
        dict(pad, name='move_to_guard_deploying', deploy_6d8=0),
        dict(pad, name='move_to_guard_locked', locked=True),
        dict(pad, name='move_to_guard_warp_out', warp_out=True),
        dict(pad, name='move_to_guard_warp_in', warp_in=True),
    ]


def process_cases():
    pad = dict(dest=list(PAD), nav=list(PAD))
    far = dict(dest=[25, 26], nav=[25, 26], linked=False)
    return [
        dict(pad, name='warp_harvester'),
        dict(far, name='warp_harvester_no_contact'),
        dict(far, name='warp_non_harvester', harvester=False, frames=[200, 315, 316]),
        dict(pad, name='warp_non_harvester_min_delay', harvester=False, frames=[200, 215, 216]),
        dict(far, name='warp_range_min', harvester=False, range_minimum=100000, frames=[200, 215, 216]),
        dict(pad, name='already_there', miner_cell=list(PAD)),
    ]


def harvest_cases():
    base = dict(mission='harvest', status=2)
    busy = dict(base, linked=False, refinery_contact='other')
    driving = dict(base, loco='drive_piggy', nav=[12, 12], moving=True, linked=False)
    return [
        dict(base, name='hello_close', linked=False, bays=['refinery']),
        dict(base, name='hello_linked', bays=['refinery']),
        # The refinery's GetCoords is (2048,2688,0) and the threshold is
        # ChronoHarvTooFarDistance=10 cells = 2560 leptons. Sqrt_Approx maps an
        # axial 2561 to 2560 (still close) and 2562 to 2561 (too far).
        dict(base, name='hello_edge', linked=False, bays=['refinery'], miner_coord=[4608, 2688, 0]),
        dict(base, name='hello_edge_sqrt_approx', linked=False, bays=['refinery'],
             miner_coord=[4609, 2688, 0]),
        dict(base, name='too_far_edge', linked=False, bays=['refinery', 'refinery'],
             miner_coord=[4610, 2688, 0], passable=[[10, 10]]),
        dict(busy, name='busy_close', bays=[None, 'refinery'], passable=[[10, 10]]),
        dict(base, name='too_far', miner_cell=[16, 16], linked=False, bays=['refinery', 'refinery'],
             passable=[[10, 10]]),
        dict(base, name='too_far_no_cell', miner_cell=[16, 16], linked=False,
             bays=['refinery', 'refinery'], passable=[None]),
        dict(base, name='no_bay', linked=False, bays=[None, None]),
        dict(driving, name='driving_bay_free', bays=['refinery', 'refinery']),
        dict(driving, name='driving_bay_free_far', miner_cell=[16, 16],
             bays=['refinery', 'refinery', 'refinery'], passable=[[10, 10]]),
        dict(driving, name='driving_no_bay', bays=[None]),
        dict(base, name='handoff', status=3),
        # State 0's Teleport-CLSID clause (0x73E82C) before Search_For_Tiberium.
        dict(base, name='state0_teleport_navcom', status=0, linked=False, nav=[12, 12], ore=[0]),
        dict(base, name='state0_drive_navcom', status=0, linked=False, nav=[12, 12], ore=[0],
             loco='drive_piggy', moving=True),
    ]


def enter_cases():
    return [
        dict(name='enter_off_pad'),
        dict(name='enter_after_warp', miner_cell=list(PAD)),
        dict(name='enter_on_pad_east_tethered', miner_cell=list(PAD), facing=0x4000,
             miner_tether=True, refinery_tether=True),
        dict(name='enter_nav_queue_teleport', nav_queue=[[12, 12], [13, 13]], dock_unload=False),
        dict(name='enter_nav_queue_drive_stopped', nav_queue=[[12, 12], [13, 13]], dock_unload=False,
             loco='drive_piggy'),
    ]


def unload_cases():
    # Mission_Unload's harvester branch turns the hull through the active
    # locomotor's Do_Turn: refinery_dock's unload_facing_west with the Teleport.
    return [dict(name='unload_teleport_facing_west', mission='unload', miner_cell=list(PAD),
                 facing=0xC000)]


def per_cell_cases():
    pad = dict(miner_cell=list(PAD))
    return [
        dict(pad, name='per_cell_pad_teleport_arrival_untethered'),
        dict(pad, name='per_cell_pad_teleport_tethered', miner_tether=True, refinery_tether=True),
    ]


def generate():
    return {'source': 'unicorn/gamemd.exe',
            'assign_destination': [assign(case) for case in assign_cases()],
            'teleport_move_to': [move_to(case) for case in move_to_cases()],
            'teleport_process': [process(case) for case in process_cases()],
            'mission_harvest': [mission(case, HARVEST) for case in harvest_cases()],
            'mission_enter': [mission(case, ENTER) for case in enter_cases()],
            'mission_unload': [mission(case, UNLOAD) for case in unload_cases()],
            'per_cell': [per_cell(case) for case in per_cell_cases()]}


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='50 original executions of the Chrono Miner (CMIN) refinery chain: 17 Unit setter '
              '(vt+0x480) calls through its Teleporter arm; 4 Teleport Move_To guard refusals; 6 Teleport '
              'Move_To arming runs followed by 12 Teleport Process frames (warp, chrono delay, delay-expiry '
              'tick), each with the FootClass::AI piggyback END step; 15 UnitClass::Mission_Harvest '
              '(states 0/2/3), 5 FootClass::Mission_Enter and 1 UnitClass::Mission_Unload dispatches with '
              'their return value; 2 Per_Cell_Process(2) Enter-arm snippets. Every nested receiver, COM '
              'call, occupy-bit write, transmit reply and Scenario draw is recorded in order.',
        entry_points={'assign_destination': dock.ASSIGN, 'foot_assign_destination': 0x4D94B0,
                      'teleport_constructor': TELE_CTOR, 'drive_constructor': DRIVE_CTOR,
                      'link_to_object': LINK, 'bridge_height': BRIDGE_HEIGHT,
                      'teleport_move_to': TELE_MOVE_TO, 'teleport_destination': 0x718B70,
                      'teleport_process': TELE_PROCESS, 'teleport_stop_moving': TELE_STOP,
                      'teleport_do_turn': TELE_DO_TURN, 'teleport_delay_tick': TELE_DELAY_TICK,
                      'teleport_is_ok_to_end': 0x719F30, 'drive_begin_piggyback': DRIVE_BEGIN,
                      'drive_end_piggyback': DRIVE_END, 'drive_is_ok_to_end': 0x4AF970,
                      'drive_move_to': DRIVE_MOVE_TO, 'can_enter_cell': CAN_ENTER,
                      'set_occupy_bit': RESERVE, 'clear_occupy_bit': UNRESERVE, 'mark': MARK,
                      'mission_harvest': HARVEST, 'mission_enter': ENTER, 'mission_unload': UNLOAD,
                      'per_cell_enter_arm': PER_CELL_ARM, 'foot_ai_piggyback_end': TAIL_BEGIN,
                      'transmit': dock.TRANSMIT, 'random_ranged': dock.RANDOM,
                      'cocreate_wrapper': COCREATE},
        assumptions=[
            'refinery_dock fixture (track_destination Unit and Drive, 32x32 map, House, Rules, refinery '
            'NW (6,9) with DockUnload and QueueingCell 4,1, a second refinery) made a CMIN: TechnoType '
            '+0xCD4 Teleporter 1, +0xE0E Harvester (0 in the non-harvester rows), +0x5B4 MovementZone 1 '
            '(Crusher, retail [CMIN]), +0x67C SpeedType 2 (fixture value distinct from it), +0x574/+0x578 '
            'ChronoIn/OutSound fixture indices 0x41/0x42.',
            'Teleport from its original constructor at fixture memory, linked by the original '
            'Link_To_Object and installed at Foot+0x674 with one reference. drive_piggy rows run the '
            'original Drive Link_To_Object and IPiggyback Begin_Piggyback over the fixture Drive, so its '
            'stash holds the Teleport; both reference counts are then written as one.',
            'Rules: ChronoDelay 60, ChronoDistanceFactor 48, ChronoTrigger yes, ChronoMinimumDelay 16, '
            'ChronoRangeMinimum 0 (100000 in warp_range_min), TiberiumLongScan 48 cells; '
            'ChronoHarvTooFarDistance 10 instead of the retail 50 so both sides of the threshold fit the '
            'map; WarpOut is a fixture anim type pointer. BridgeHeight 416 from its original initializer '
            '0x7352F0 over the established level height 104. The miner is on the map (ObjectClass+0x74).',
            'Process rows arm the Teleport with a direct Move_To to the cell centre (NavCom supplied), then '
            'per listed frame run Process and the FootClass::AI tail 0x4DAE5F..0x4DAEC6 plus its '
            'IPiggyback Release, not the rest of FootClass::AI. Per_Cell arm rows supply the prologue '
            'locals as refinery_dock does.'],
        substitutions=[
            'CoCreateInstance wrapper 0x41C250 is recorded; it runs the original Drive constructor and '
            'ILocomotion AddRef on fixture memory, stores that ILocomotion and returns S_OK.',
            'operator new answers the AnimClass allocation with fixture memory and operator delete is '
            'recorded without freeing. The AnimClass constructor 0x421EA0, VocClass::PlayAt 0x7509E0, '
            'crate pickup 0x481A00 and Unit Per_Cell_Process 0x739EC0 (inside Process) are recorded and '
            'return without effect, so WarpOut AnimClass RNG draws are not covered.',
            'MapClass zone lookup 0x56D230 answers zone 7 (no map zone tables); Search_For_Tiberium '
            '0x4DCFE0 answers the row value (the original also answers 0 while NavCom is set); '
            'refinery_dock keeps its supplied Find_Docking_Bay, Find_Nearby_Passable_Cell and '
            'Ready_To_Commence answers and its Scatter, Enter_Idle_Mode and refinery animation observers.']))
