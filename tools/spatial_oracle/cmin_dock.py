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

Three additive instance histories retain the complete raw Drive/Foot/Teleport,
mapped-cell and RNG state around fresh/reused/repeated Move, END refusal,
pointer transfer and final Release. See cmin_dock.md for their receipt format
and the same fixture seams. The legacy 50-row payload has a canonical guard.
"""
from pathlib import Path
import hashlib
import json
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
                               UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI,
                               UC_X86_REG_ESP, UC_X86_REG_FPCW)
from tools.native_oracle import (NativeCallTrace, RET_MAGIC, changed_byte_spans,
                                finish_vectors, provenance, run_checked)
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle import refinery_dock as dock
from tools.spatial_oracle.refinery_dock import (ACTOR, LOCO, BLD, OTHER, MINER_ITEMS, BLD_ITEMS,
                                                RULES, PAD, cell, cell_xy)
from tools.spatial_oracle.unit_entry import EXTRA
from tools.spatial_oracle.unit_scatter_state import TYPE, SP
from tools.spatial_oracle.unit_source_scatter import CELLS, SCENARIO

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
DRIVE_SIZE, TELE_SIZE, FOOT_SIZE = 0x6C, 0x4C, 0x700
INSTANCE_GUARD = bytes.fromhex('d3c7915b')
LEGACY_PAYLOAD_SHA256 = '0c09972becd0dbae19513665f9629a251bf443efe5cff0117aa9724541f84879'
INSTANCE_BOUNDARIES = {
    0x4AF540: 'drive_constructor_entry', 0x4AF5D9: 'drive_constructor_ret',
    0x7425F8: 'unit_drive_clsid_branch', 0x7426CC: 'unit_link_return',
    0x742772: 'unit_begin_return', 0x74277E: 'unit_before_install',
    0x742780: 'unit_after_install', 0x4AF8E0: 'drive_begin_entry',
    0x4AF918: 'drive_begin_ret', 0x4AF970: 'drive_end_gate_entry',
    0x4AF9A1: 'drive_end_gate_true_ret', 0x4AF9A7: 'drive_end_gate_false_ret',
    0x4DAE5F: 'foot_ai_end_entry', 0x4DAEAD: 'foot_ai_after_end_gate',
    0x4DAEB9: 'foot_ai_before_active_release', 0x4DAEBC: 'foot_ai_before_clear',
    0x4DAEBF: 'foot_ai_after_clear', 0x4DAEC3: 'foot_ai_end_call',
    0x4AF930: 'drive_end_entry', 0x4AF94B: 'drive_end_before_output',
    0x4AF94D: 'drive_end_after_output_before_stash_clear',
    0x4AF956: 'drive_end_true_ret', 0x742554: 'unit_pad_end_gate_call',
    0x742557: 'unit_pad_after_end_gate', 0x742587: 'unit_pad_end_call',
    0x7425A0: 'unit_pad_stop_fallback',
    DRIVE_MOVE_TO: 'drive_move_to_entry', DRIVE_STOP: 'drive_stop_entry',
    0x4B4BE0: 'drive_disable_end_entry', 0x4B4BF0: 'drive_enable_end_entry',
}
INSTANCE_RNGS = {'main': 0x886B88, 'scenario': SCENARIO + 0x218, 'mapgen': 0xABE890}
# Complete mapped region, including the 0x10000 tail after the 32x32 live cells.
INSTANCE_CELL_BYTES = 0x90000
INSTANCE_PADDING = ((0x12, 2), (0x28, 4), (0x66, 2))


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


def observe(u, read32, case, *, instance_boundary=None):
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
            if instance_boundary is not None:
                # Only new instance controls poison the selected allocation.
                # Constructor padding is retained, not converted to defaults.
                u.mem_write(drive, b'\xA5' * DRIVE_SIZE)
                instance_boundary('allocated_before_constructor', drive)
            pending.append((this, drive))
            u.mem_write(sp - 4, dwords(STUB_CTOR))
            u.reg_write(UC_X86_REG_ESP, sp - 4)
            u.reg_write(UC_X86_REG_ECX, drive)
            u.reg_write(UC_X86_REG_EIP, DRIVE_CTOR)
        elif address == STUB_CTOR:
            drive = pending[-1][1]
            if instance_boundary is not None:
                instance_boundary('allocator_original_constructor_return', drive)
            u.mem_write(sp - 8, dwords(STUB_ADDREF, drive + 4))
            u.reg_write(UC_X86_REG_ESP, sp - 8)
            u.reg_write(UC_X86_REG_EIP, DRIVE_ADDREF)
        elif address == STUB_ADDREF:
            smart, drive = pending.pop()
            u.mem_write(smart, dwords(drive + 4))
            if instance_boundary is not None:
                instance_boundary('allocator_original_addref_return', drive)
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
            if instance_boundary is not None:
                instance_boundary('operator_delete_before_no_free_seam', read32(sp + 4))
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


def piggyback_end_step(u, call, read32, *, instance_boundary=None):
    """FootClass::AI's tail 0x4DAE5F..0x4DAEC6 (Is_Ok_To_End, then END), then its
    Release of the IPiggyback reference (0x4DAEFA)."""
    frame = SP - 0x100
    u.mem_write(frame + 0x14, dwords(0))
    u.reg_write(UC_X86_REG_ESI, ACTOR)
    u.reg_write(UC_X86_REG_EBX, 0)
    u.reg_write(UC_X86_REG_ESP, frame)
    run_checked(u, TAIL_BEGIN, TAIL_END, count=100000)
    piggy = u.reg_read(UC_X86_REG_EDI)
    if instance_boundary is not None:
        instance_boundary('foot_ai_end_before_final_release', None)
    if piggy:
        call(read32(read32(piggy) + 8), 0, [piggy])
        if instance_boundary is not None:
            instance_boundary('foot_ai_end_final_release_return', None)


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


def instance_state(u, read32, cell_baseline, padding, lifetime):
    """Read-only complete instance receipt, including the temporary null owner.

    The mapped-cell baseline plus changed spans reconstructs every mapped byte;
    occupation/list words are also packed independently in fixed y/x order.
    Opaque words and constructor padding stay raw evidence, not Rust defaults.
    """
    raw = lambda address, size: bytes(u.mem_read(address, size))
    signed = lambda address: struct.unpack('<i', raw(address, 4))[0]
    coords = lambda address: list(struct.unpack('<3i', raw(address, 12)))
    byte = lambda address: raw(address, 1)[0]
    drives = {}
    for base in (LOCO, *DRIVES):
        value = raw(base, DRIVE_SIZE)
        guard = raw(base + DRIVE_SIZE, len(INSTANCE_GUARD))
        assert guard == INSTANCE_GUARD, ('Drive allocation guard', hex(base), guard.hex())
        pad = [value[offset:offset + size].hex() for offset, size in INSTANCE_PADDING]
        assert pad == padding[base], ('Drive untouched padding', hex(base), pad, padding[base])
        drives[loco_name(base)] = dict(
            address=hex(base), lifetime=lifetime[base], raw_hex=value.hex(), guard_hex=guard.hex(),
            padding_hex=pad, vtables=[hex(read32(base + offset)) for offset in (0, 4, 0x18)],
            linked_foot=hex(read32(base + 0xC)), power=list(value[0x10:0x12]),
            # Constructor4AF54D/4AF550 zero full dwords: current +1C,
            # previous +20. Process4B0523/4B052A/4B0533 reads/writes the
            # same complete-object fields. Publish previous/current order.
            refs=signed(base + 0x14), slopes=[signed(base + 0x20), signed(base + 0x1C)],
            timer_words=list(struct.unpack('<3i', value[0x24:0x30])),
            interpolation_total=signed(base + 0x30), destination=coords(base + 0x34),
            head=coords(base + 0x40), residual=signed(base + 0x4C),
            target_speed_bits=f'{struct.unpack("<Q", value[0x50:0x58])[0]:016x}',
            selector=signed(base + 0x58), cursor=signed(base + 0x5C),
            flags_60_67=list(value[0x60:0x68]), stash=hex(read32(base + 0x68)),
        )
    active = read32(ACTOR + 0x674)
    foot = raw(ACTOR, FOOT_SIZE)
    assert raw(ACTOR + FOOT_SIZE, len(INSTANCE_GUARD)) == INSTANCE_GUARD, 'Foot receipt guard'
    assert raw(TELE + TELE_SIZE, len(INSTANCE_GUARD)) == INSTANCE_GUARD, 'Teleport receipt guard'
    queue_items, queue_capacity, queue_count = (read32(ACTOR + offset) for offset in
                                                (0x58C, 0x590, 0x598))
    assert 0 <= queue_count <= queue_capacity <= 32, ('NavQueue bound', queue_capacity, queue_count)
    assert queue_items or not queue_capacity, ('NavQueue null items', queue_capacity)
    if queue_capacity:
        # The existing fixture supplies this one mapped queue allocation.
        assert queue_items == EXTRA + 0x2C000, ('unexpected NavQueue allocation', hex(queue_items))
    cells = raw(CELLS, INSTANCE_CELL_BYTES)
    changed = changed_byte_spans(cell_baseline, cells)
    occupation_lists = b''.join(cells[index * 0x200 + offset:index * 0x200 + offset + 4]
                               for index in range(32 * 32) for offset in (0x124, 0x128, 0xE4, 0xE8))
    return dict(
        drives=drives,
        foot=dict(raw_hex=foot.hex(), active_pointer=hex(active), active=loco_name(active),
                  physical=coords(ACTOR + 0x9C), on_bridge=byte(ACTOR + 0x8C),
                  applied_speed_bits=f'{struct.unpack("<Q", foot[0x578:0x580])[0]:016x}',
                  reference_cell=list(struct.unpack('<hh', foot[0x558:0x55C])),
                  path=list(struct.unpack('<24i', foot[0x5E0:0x640])),
                  movement_timer=list(struct.unpack('<3i', foot[0x640:0x64C])),
                  blocked_timer=list(struct.unpack('<3i', foot[0x668:0x674])),
                  retries=signed(ACTOR + 0x64C), nav=hex(read32(ACTOR + 0x5A4)),
                  aux=hex(read32(ACTOR + 0x5A0)), path_blocked=byte(ACTOR + 0x6B7),
                  skip_move_to=byte(ACTOR + 0x6AC), force_reassign=byte(ACTOR + 0x1F8),
                  swap_6ad=byte(ACTOR + 0x6AD), mission=signed(ACTOR + 0xAC),
                  queued=signed(ACTOR + 0xB4), status=signed(ACTOR + 0xBC),
                  dispatch_timer=list(struct.unpack('<3i', foot[0xC8:0xD4])),
                  facing_raw_hex=foot[0x388:0x39C].hex(),
                  nav_queue=dict(header_hex=foot[0x588:0x5A0].hex(), items=hex(queue_items),
                                 capacity=queue_capacity, count=queue_count,
                                 allocation_hex=raw(queue_items, queue_capacity * 4).hex()
                                 if queue_capacity else '')),
        teleport=dict(address=hex(TELE), raw_hex=raw(TELE, TELE_SIZE).hex(),
                      refs=signed(TELE + 0x14), destination=coords(TELE + 0x1C),
                      resolved=coords(TELE + 0x28), moving=byte(TELE + 0x34),
                      timer_words=list(struct.unpack('<3i', raw(TELE + 0x3C, 12))),
                      stash=hex(read32(TELE + 0x48))),
        mapped_cells=dict(sha256=hashlib.sha256(cells).hexdigest(), changed_spans=changed,
                          occupation_and_lists_hex=occupation_lists.hex(),
                          object_next={name_of(pointer): hex(read32(pointer + 0x30))
                                       for pointer in (ACTOR, BLD, OTHER, PAD_UNIT)}),
        contacts=dict(miner=hex(read32(MINER_ITEMS)), refinery=hex(read32(BLD_ITEMS)),
                      miner_tether=byte(ACTOR + 0x418), refinery_tether=byte(BLD + 0x418),
                      refinery_queued=signed(BLD + 0xB4)),
        rng={name: dict(address=hex(pointer), raw_hex=raw(pointer, 1012).hex())
             for name, pointer in INSTANCE_RNGS.items()},
    )


def instance_cases():
    common = dict(miner_coord=[2688, 2688, 123], frame=200, seed=31,
                  foot_speed_bits='3fec000000000000')
    return [dict(common, name='far_fresh_reuse_end_repeat', history='far', linked=False),
            dict(common, name='pad_stopped_end', history='pad_stopped', loco='drive_piggy',
                 dest=list(PAD)),
            dict(common, name='pad_moving_refused_stop_end', history='pad_moving',
                 loco='drive_piggy', dest=list(PAD), moving=True, nav=[12, 12])]


def instance_control(case):
    """Three additive histories through the existing fixture, observer and call.

    All new helpers below measure state; only the existing call and FootAI-tail
    owners execute original instructions. Return values are sampled at actual
    return PCs and matching ESP, including the runner's non-executed stop PCs.
    """
    u, call, read32 = make_cmin_fixture(case)
    for base, size in ((ACTOR, FOOT_SIZE), (TELE, TELE_SIZE),
                       *((base, DRIVE_SIZE) for base in (LOCO, *DRIVES))):
        u.mem_write(base + size, INSTANCE_GUARD)
    u.mem_write(ACTOR + 0x578, struct.pack('<Q', int(case['foot_speed_bits'], 16)))
    for pointer in INSTANCE_RNGS.values():
        call(0x65C6D0, pointer, [31])
        assert u.reg_read(UC_X86_REG_EIP) == RET_MAGIC and u.reg_read(UC_X86_REG_ESP) == SP + 8
    cell_baseline = bytes(u.mem_read(CELLS, INSTANCE_CELL_BYTES))
    padding = {base: [bytes(u.mem_read(base + offset, size)).hex()
                      for offset, size in INSTANCE_PADDING] for base in (LOCO, *DRIVES)}
    lifetime = {base: 'fixture_constructed' if base == LOCO else 'unallocated'
                for base in (LOCO, *DRIVES)}
    text_before = hashlib.sha256(bytes(u.mem_read(0x401000, 0x3E0000))).hexdigest()
    vtable_spans = {'unit': (0x7F5C70, 0x600), 'building': (read32(BLD), 0x600),
                    'drive_locomotion': (0x7E7EB0, 0xC0), 'drive_piggyback': (0x7E7E8C, 0x20),
                    'drive_object': (0x7E7F7C, 0x20), 'teleport_locomotion': (0x7F5000, 0xC0)}
    vtables_before = {name: hashlib.sha256(bytes(u.mem_read(pointer, size))).hexdigest()
                      for name, (pointer, size) in vtable_spans.items()}
    boundaries, native_calls, steps = [], [], []
    call_trace = NativeCallTrace(u, read32, native_calls)
    pending = call_trace.pending
    returned = call_trace.returned
    current_drive = LOCO if case.get('loco') == 'drive_piggy' else None

    def snapshot(label, drive=None):
        nonlocal current_drive
        pc, sp = u.reg_read(UC_X86_REG_EIP), u.reg_read(UC_X86_REG_ESP)
        returned(pc, sp)
        if label == 'allocated_before_constructor':
            assert drive in DRIVES and lifetime[drive] == 'unallocated'
            padding[drive] = ['a5' * size for _offset, size in INSTANCE_PADDING]
            lifetime[drive] = 'allocated_before_constructor'
        elif label == 'allocator_original_constructor_return':
            lifetime[drive] = 'constructed'
        elif label == 'operator_delete_before_no_free_seam':
            assert drive in (LOCO, *DRIVES), hex(drive)
            lifetime[drive] = 'delete_entry_no_free_seam'
        if drive is not None:
            current_drive = drive
        fpcw = u.reg_read(UC_X86_REG_FPCW)
        assert fpcw == 0x0E7F, ('ambient FPCW changed', label, hex(fpcw))
        boundaries.append(dict(label=label, drive=loco_name(current_drive or 0),
                               pc=hex(pc), esp=hex(sp), eax=u.reg_read(UC_X86_REG_EAX),
                               frame=read32(0xA8ED84), fpcw=fpcw,
                               state=instance_state(u, read32, cell_baseline, padding, lifetime)))
        return len(boundaries) - 1

    events, unused = observe(u, read32, case, instance_boundary=snapshot)
    # (name, argument count, callee cleanup). Receiver adjustment remains native.
    call_specs = {dock.ASSIGN: ('unit_set_destination', 2, 8),
                  0x4D94B0: ('foot_set_destination', 2, 8),
                  DRIVE_CTOR: ('drive_constructor', 0, 0),
                  0x55A6C0: ('locomotor_base_constructor', 0, 0),
                  LINK: ('link_to_object', 2, 8), DRIVE_BEGIN: ('drive_begin', 2, 8),
                  DRIVE_END: ('drive_end', 2, 8), 0x4AF970: ('drive_end_gate', 1, 4),
                  0x4AFB80: ('drive_is_moving', 1, 4),
                  DRIVE_MOVE_TO: ('drive_move_to', 4, 16), DRIVE_STOP: ('drive_stop', 1, 4),
                  0x4B4BE0: ('drive_disable_end', 1, 4), 0x4B4BF0: ('drive_enable_end', 1, 4),
                  0x65C780: ('rng_raw', 0, 0), 0x65C7E0: ('rng_ranged', 2, 8)}
    for vtable, interface in ((0x7E7EB0, 'drive_ilocomotion'),
                              (0x7E7E8C, 'drive_ipiggyback'), (0x7F5000, 'teleport_ilocomotion')):
        for slot, name, count in ((0, 'query_interface', 3), (4, 'addref', 1), (8, 'release', 1)):
            entry = read32(vtable + slot)
            call_specs.setdefault(entry, (interface + '_' + name, count, count * 4))

    def trace(_u, pc, _size, _data):
        nonlocal current_drive
        sp = u.reg_read(UC_X86_REG_ESP)
        returned(pc, sp)
        if pc in (DRIVE_BEGIN, DRIVE_END, 0x4AF970):
            current_drive = read32(sp + 4) - 0x18
        elif pc in (DRIVE_MOVE_TO, DRIVE_STOP, 0x4AFB80, 0x4B4BE0, 0x4B4BF0):
            current_drive = read32(sp + 4) - 4
        elif pc == DRIVE_CTOR:
            current_drive = u.reg_read(UC_X86_REG_ECX)
        if pc in call_specs:
            call_trace.entered(pc, sp, call_specs[pc])
        if pc in INSTANCE_BOUNDARIES:
            snapshot(INSTANCE_BOUNDARIES[pc])

    u.hook_add(UC_HOOK_CODE, trace)

    def start_step(label, supplied=None):
        return dict(label=label, supplied=supplied or [], before=snapshot(label + '_before'),
                    event_start=len(events), call_start=len(native_calls))

    def finish_step(step, cleanup):
        assert u.reg_read(UC_X86_REG_EIP) == RET_MAGIC, (step['label'], 'non-return stop')
        assert u.reg_read(UC_X86_REG_ESP) == SP + 4 + cleanup, (step['label'], 'stack cleanup')
        step.update(after=snapshot(step['label'] + '_after'), event_end=len(events),
                    call_end=len(native_calls), terminal_esp=hex(u.reg_read(UC_X86_REG_ESP)),
                    raw_eax=u.reg_read(UC_X86_REG_EAX), cleanup=cleanup)
        steps.append(step)

    def retained_input(base, head=None):
        writes = [(0x1C, dwords(7, 9)), (0x24, dwords(197)), (0x2C, dwords(3, 3)),
                  (0x4C, dwords(37)), (0x50, struct.pack('<Q', 0x3FE4000000000000)),
                  (0x58, dwords(10, 3)), (0x60, b'\x01'), (0x62, b'\x01\x01')]
        if head is not None:
            writes.append((0x40, dwords(*head)))
        for offset, value in writes:
            u.mem_write(base + offset, value)
        return [dict(address=hex(base + offset), raw_hex=value.hex()) for offset, value in writes]

    def refused(step, base):
        before, after = (boundaries[step[key]]['state'] for key in ('before', 'after'))
        assert before['drives'][loco_name(base)]['raw_hex'] == after['drives'][loco_name(base)]['raw_hex']
        assert before['foot']['raw_hex'] == after['foot']['raw_hex']
        assert before['teleport']['raw_hex'] == after['teleport']['raw_hex']
        assert before['rng'] == after['rng'] and before['mapped_cells'] == after['mapped_cells']
        calls = native_calls[step['call_start']:step['call_end']]
        gates = [row for row in calls if row['name'] == 'drive_end_gate']
        assert len(gates) == 1 and gates[0]['return_eax'] & 0xFF == 0
        assert not any(row['name'] == 'drive_end' for row in calls)
        assert not any(row[0] == 'delete' for row in events[step['event_start']:step['event_end']])

    def ended(step, base):
        before = boundaries[step['before']]['state']
        live = [row['state'] for row in boundaries[step['before'] + 1:step['after']]
                if row['label'] == 'drive_end_true_ret']
        assert len(live) == 1, ('END did not reach its native successful ret', step['label'])
        old_drive = bytes.fromhex(before['drives'][loco_name(base)]['raw_hex'])
        live_drive = bytes.fromhex(live[0]['drives'][loco_name(base)]['raw_hex'])
        assert old_drive[0x1C:0x68] == live_drive[0x1C:0x68], 'END changed retained Drive fields'
        assert live[0]['drives'][loco_name(base)]['stash'] == '0x0'
        assert live[0]['foot']['active_pointer'] == hex(TELE + 4)
        assert before['foot']['physical'] == live[0]['foot']['physical']
        assert before['foot']['applied_speed_bits'] == live[0]['foot']['applied_speed_bits']
        assert any(row[0] == 'delete' and row[1] == loco_name(base)
                   for row in events[step['event_start']:step['event_end']])

    if case['history'] == 'far':
        step = start_step('fresh_move')
        call(dock.ASSIGN, ACTOR, [cell(14, 12), 1])
        finish_step(step, 8)
        base = DRIVES[0]
        assert read32(ACTOR + 0x674) == base + 4 and read32(base + 0x68) == TELE + 4
        assert sum(row[0] == 'cocreate' for row in events) == 1
        supplied = retained_input(base, [3013, 2979, -347])
        step = start_step('reuse_move', supplied)
        call(dock.ASSIGN, ACTOR, [cell(15, 13), 1])
        finish_step(step, 8)
        assert read32(ACTOR + 0x674) == base + 4 and read32(base + 0x68) == TELE + 4
        assert sum(row[0] == 'cocreate' for row in events) == 1
        step = start_step('moving_end_refusal')
        piggyback_end_step(u, call, read32, instance_boundary=snapshot)
        finish_step(step, 4)
        refused(step, base)
        step = start_step('original_stop')
        call(DRIVE_STOP, 0, [base + 4])
        finish_step(step, 4)
        physical = list(struct.unpack('<3i', u.mem_read(ACTOR + 0x9C, 12)))
        settled_head = dwords(physical[0], physical[1], physical[2] + 104)
        u.mem_write(base + 0x40, settled_head)
        step = start_step('disable_end', [dict(address=hex(base + 0x40), raw_hex=settled_head.hex())])
        call(0x4B4BE0, 0, [base + 4])
        finish_step(step, 4)
        step = start_step('permission_end_refusal')
        piggyback_end_step(u, call, read32, instance_boundary=snapshot)
        finish_step(step, 4)
        refused(step, base)
        step = start_step('enable_end')
        call(0x4B4BF0, 0, [base + 4])
        finish_step(step, 4)
        u.mem_write(ACTOR + 0x6AD, b'\x01')
        step = start_step('swap_end_refusal', [dict(address=hex(ACTOR + 0x6AD), raw_hex='01')])
        piggyback_end_step(u, call, read32, instance_boundary=snapshot)
        finish_step(step, 4)
        refused(step, base)
        u.mem_write(ACTOR + 0x6AD, b'\x00')
        step = start_step('successful_end', [dict(address=hex(ACTOR + 0x6AD), raw_hex='00')])
        piggyback_end_step(u, call, read32, instance_boundary=snapshot)
        finish_step(step, 4)
        ended(step, base)
        assert read32(ACTOR + 0x674) == TELE + 4
        u.mem_write(0xA8ED84, dwords(201))
        step = start_step('repeat_fresh_move', [dict(address='0xa8ed84', raw_hex=dwords(201).hex())])
        call(dock.ASSIGN, ACTOR, [cell(14, 12), 1])
        finish_step(step, 8)
        assert read32(ACTOR + 0x674) == DRIVES[1] + 4 and read32(DRIVES[1] + 0x68) == TELE + 4
        assert sum(row[0] == 'cocreate' for row in events) == 2
    else:
        base = LOCO
        supplied = retained_input(base)
        step = start_step('pad_setter', supplied)
        call(dock.ASSIGN, ACTOR, [cell(*PAD), 1])
        finish_step(step, 8)
        if case['history'] == 'pad_stopped':
            ended(step, base)
            assert read32(ACTOR + 0x674) == TELE + 4
        else:
            assert case['history'] == 'pad_moving'
            calls = native_calls[step['call_start']:step['call_end']]
            gates = [row for row in calls if row['name'] == 'drive_end_gate']
            assert len(gates) == 1 and gates[0]['return_eax'] & 0xFF == 0
            assert not any(row['name'] == 'drive_end' for row in calls)
            assert any(row['name'] == 'drive_stop' for row in calls)
            assert read32(ACTOR + 0x674) == base + 4
            step = start_step('post_stop_end')
            piggyback_end_step(u, call, read32, instance_boundary=snapshot)
            finish_step(step, 4)
            ended(step, base)
            assert read32(ACTOR + 0x674) == TELE + 4
        assert not any(row[0] == 'cocreate' for row in events), 'pad caller allocated a replacement Drive'
    assert not any(unused), (case, unused)
    assert not pending, ('unmatched original returns', pending, case)
    assert all('return_eax' in row for row in native_calls), 'missing executed return receipt'
    labels = {row['label'] for row in boundaries}
    required = {'drive_end_gate_entry', 'drive_end_gate_true_ret', 'drive_end_entry',
                'drive_end_before_output', 'drive_end_after_output_before_stash_clear',
                'drive_end_true_ret', 'operator_delete_before_no_free_seam'}
    if case['history'] == 'far':
        required.update({'allocated_before_constructor', 'drive_constructor_entry',
                         'drive_constructor_ret', 'allocator_original_constructor_return',
                         'allocator_original_addref_return', 'unit_link_return',
                         'drive_begin_entry', 'drive_begin_ret', 'unit_begin_return',
                         'unit_before_install', 'unit_after_install', 'unit_drive_clsid_branch',
                         'drive_move_to_entry', 'drive_end_gate_false_ret',
                         'foot_ai_before_active_release', 'foot_ai_before_clear',
                         'foot_ai_after_clear', 'foot_ai_end_before_final_release',
                         'foot_ai_end_final_release_return'})
    else:
        required.update({'unit_pad_end_gate_call', 'unit_pad_after_end_gate'})
        required.add('unit_pad_end_call' if case['history'] == 'pad_stopped'
                     else 'unit_pad_stop_fallback')
    assert required <= labels, ('missing original boundaries', sorted(required - labels), case)
    text_after = hashlib.sha256(bytes(u.mem_read(0x401000, 0x3E0000))).hexdigest()
    vtables_after = {name: hashlib.sha256(bytes(u.mem_read(pointer, size))).hexdigest()
                     for name, (pointer, size) in vtable_spans.items()}
    assert text_before == text_after and vtables_before == vtables_after
    return dict(input=case, steps=steps, boundaries=boundaries, events=events,
                native_calls=native_calls,
                rng_calls=[row for row in native_calls if row['name'] in ('rng_raw', 'rng_ranged')],
                cell_baseline=dict(address=hex(CELLS), bytes=INSTANCE_CELL_BYTES,
                                   raw_hex=cell_baseline.hex(),
                                   sha256=hashlib.sha256(cell_baseline).hexdigest(),
                                   occupation_and_lists_order=['ground_124', 'deck_128',
                                                               'ground_head_e4', 'deck_head_e8']),
                guards=dict(text_before_sha256=text_before, text_after_sha256=text_after,
                            vtables_before_sha256=vtables_before, vtables_after_sha256=vtables_after))


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
    legacy = {'source': 'unicorn/gamemd.exe',
              'assign_destination': [assign(case) for case in assign_cases()],
              'teleport_move_to': [move_to(case) for case in move_to_cases()],
              'teleport_process': [process(case) for case in process_cases()],
              'mission_harvest': [mission(case, HARVEST) for case in harvest_cases()],
              'mission_enter': [mission(case, ENTER) for case in enter_cases()],
              'mission_unload': [mission(case, UNLOAD) for case in unload_cases()],
              'per_cell': [per_cell(case) for case in per_cell_cases()]}
    canonical = json.dumps(legacy, sort_keys=True, separators=(',', ':'), allow_nan=False).encode()
    assert hashlib.sha256(canonical).hexdigest() == LEGACY_PAYLOAD_SHA256, 'legacy 50 CMIN rows changed'
    return dict(legacy, instance_controls=[instance_control(case) for case in instance_cases()])


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='50 unchanged original rows of the Chrono Miner (CMIN) refinery chain: 17 Unit setter '
              '(vt+0x480) calls through its Teleporter arm; 4 Teleport Move_To guard refusals; 6 Teleport '
              'Move_To arming runs followed by 12 Teleport Process frames (warp, chrono delay, delay-expiry '
              'tick), each with the FootClass::AI piggyback END step; 15 UnitClass::Mission_Harvest '
              '(states 0/2/3), 5 FootClass::Mission_Enter and 1 UnitClass::Mission_Unload dispatches with '
              'their return value; 2 Per_Cell_Process(2) Enter-arm snippets. Every nested receiver, COM '
              'call, occupy-bit write, transmit reply and Scenario draw is recorded in order. Three '
              'additive complete Drive instance histories measure ordinary fresh/reused/repeated Move, '
              'moving/permission/swap END refusals and successful END/final Release, plus the stopped '
              'and moving refinery-pad setter callers. They retain full raw Drive (108 bytes), Foot '
              '(0x700), Teleport (0x4C), all mapped-cell bytes and all three full RNG objects at the '
              'declared boundaries; actual call returns match return PC and ESP.',
        entry_points={'assign_destination': dock.ASSIGN, 'foot_assign_destination': 0x4D94B0,
                      'teleport_constructor': TELE_CTOR, 'drive_constructor': DRIVE_CTOR,
                      'link_to_object': LINK, 'bridge_height': BRIDGE_HEIGHT,
                      'teleport_move_to': TELE_MOVE_TO, 'teleport_destination': 0x718B70,
                      'teleport_process': TELE_PROCESS, 'teleport_stop_moving': TELE_STOP,
                      'teleport_do_turn': TELE_DO_TURN, 'teleport_delay_tick': TELE_DELAY_TICK,
                      'teleport_is_ok_to_end': 0x719F30, 'drive_begin_piggyback': DRIVE_BEGIN,
                      'drive_end_piggyback': DRIVE_END, 'drive_is_ok_to_end': 0x4AF970,
                      'drive_move_to': DRIVE_MOVE_TO, 'drive_stop_moving': DRIVE_STOP,
                      'drive_is_moving': 0x4AFB80, 'drive_disable_end': 0x4B4BE0,
                      'drive_enable_end': 0x4B4BF0, 'random_seed': 0x65C6D0,
                      'random_raw': 0x65C780, 'can_enter_cell': CAN_ENTER,
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
            'locals as refinery_dock does.',
            'Only the three instance histories initialize main 0x886B88, Scenario+0x218 and mapgen '
            '0xABE890 with original 0x65C6D0(seed=31), preserving all 1012 bytes per RNG. They supply '
            'physical Z 123 and independent Foot+0x578 binary64 speed bits 0x3FEC000000000000. '
            'Distinctive retained Drive inputs are declared raw writes, not claims of a paid Process '
            'history: previous/current slope dwords +0x20/+0x1C=9/7 (constructor4AF54D/4AF550; '
            'Process4B0523/4B052A/4B0533), timer start 197 and duration/total 3/3, residual 37, target bits '
            '0x3FE4000000000000, selector/cursor 10/3 and bytes +0x60/+0x62/+0x63=1; the far history '
            'also supplies head [3013,2979,-347], then a settled physical XY/different-Z head and '
            'Foot+0x6AD refusal input. Constructor padding +0x12..0x14/+0x28..0x2C/+0x66..0x68 is '
            'guarded raw evidence; only selected new allocations are A5-poisoned. Fixture LOCO is '
            'constructed at inherited frame 100; Teleport and fresh Drives see 200, then repeated '
            'Move sees 201. Original text/vtables, allocation guards, stack cleanup and FPCW 0x0E7F '
            'are checked. Full mapped-cell baseline plus exact changed spans reconstructs all '
            '0x90000 bytes; fixed y/x occupation/list words and touched object next pointers are '
            'retained independently. See tools/spatial_oracle/cmin_dock.md for receipt fields and limits.'],
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
            'Ready_To_Commence answers and its Scatter, Enter_Idle_Mode and refinery animation observers.',
            'Instance histories use the same existing call/fixture/observer owners. The FootAI tail '
            'ends at 0x4DAEC6 and the helper runs its original IPiggyback Release corresponding to '
            '0x4DAEFA..0x4DAEFD; intervening FootAI callbacks and the remaining AI continuation are '
            'outside coverage. Deleted Drive bytes remain readable fixture memory, not a live '
            'instance or evidence of CRT address reuse. These histories do not certify complete '
            'paid-track Process, native save/load, generic nested-BEGIN policy, Ship permission '
            'semantics or whole-game allocation. Unit setter raw EAX is not a return contract.']))
