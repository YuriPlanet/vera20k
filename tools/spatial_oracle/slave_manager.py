"""Original SlaveManagerClass over a deployed Yuri slave refinery.

AI_Update's per-slave machine (0x6AF6C0), the manager's own machine
(0x6AFD60) for a Building owner in states 0, 4, 5 and 6, DeploySlaves (0x6B04C0),
the slave's InfantryClass::Mission_Harvest (0x522E70) and its deposit
(0x522D50) run over the original Infantry and Building vtables on the
harvest_field map fixture (its ore and gem tables, House, Rules and Scenario
RNG).

The manager and its slave nodes are supplied (no constructor). The slave's
class setter (0x51AA40), Limbo (0x51DF10) and PlayAnim (0x51D6F0) are
observed and answered, and the slave type's CreateObject (0x523B10) hands out
a prepared spare; their Rust owners carry their own evidence. Unlimbo
(0x51DFF0, its PlaceInfantryInCell and occupy mark) and Scatter (0x51D0D0)
run natively; FootClass::Unlimbo (0x4D7170) is answered after writing its
placement, without the sight reveal and layer submission.
State 5's relocation check (no ore within SlaveMinerShortScan) is not
exercised: every state-5 row keeps ore within that range.

A Slave Miner owner (`owner='unit'`: the fixture's Unit with DeploysInto=YAREFN,
ResourceGatherer/ResourceDestination and the manager at +0x2D8) runs the
manager machine's unit states, ShouldRecallSlaves 0x6B1020, the hunt start
0x6B0CC0, the reset 0x6B0C80, HandleReturnedSlaves 0x6B0DB0 and the
Guard/AreaGuard kick (UnitClass::Mission_Guard 0x740810 / Mission_AreaGuard
0x744100, stopped where the ordinary mission continues). Its Assign_Destination
0x741970 runs live; UnitClass::Deploy 0x7393C0 and MapClass::GetZoneID 0x56D230
are answered, and FindDeployCell's Find_Nearby_Passable_Cell arguments are
recorded at its call (0x6B0417) while the dock observer answers the search.

Usage: python -m tools.spatial_oracle.slave_manager [--check|--write]
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle import harvest_field as field
from tools.spatial_oracle.refinery_dock import ACTOR, HOUSE, RULES, TYPE, cell, cell_xy, place_building
from tools.spatial_oracle.unit_scatter_state import SP
from tools.spatial_oracle.unit_source_scatter import SCENARIO

REGION = 0x22000000
MANAGER, ITEMS, YAREFN, YTYPE, STYPE = (REGION + n for n in (0, 0x100, 0x1000, 0x3000, 0x5000))
NODE_BASE, SLAVE_BASE = REGION + 0x7000, REGION + 0x10000
SLAVE_SIZE = 0x2000  # the object, then its Walk locomotor at +0x1000
SLAVE_SLOTS = 8  # nodes 0..6; the last slot is the regen spare
SPARE = SLAVE_SLOTS - 1
AI_UPDATE, MANAGER_AI, DEPLOY_SLAVES = 0x6AF6C0, 0x6AFD60, 0x6B04C0
SLAVE_HARVEST, DEPOSIT = 0x522E70, 0x522D50
SETTER, LIMBO, UNLIMBO, SCATTER, PLAY_ANIM = 0x51AA40, 0x51DF10, 0x51DFF0, 0x51D0D0, 0x51D6F0
CREATE_OBJECT, WALK_CTOR, FOOT_UNLIMBO = 0x523B10, 0x75AA90, 0x4D7170
# A Slave Miner owner (the fixture's Unit, chain 6): its manager helpers, the
# Guard/AreaGuard kick, UnitClass::Deploy (answered), GetZoneID (answered) and
# FindDeployCell's Find_Nearby_Passable_Cell call (arguments recorded at the
# call; the dock observer answers from `passable`).
SHOULD_RECALL, BEGIN_HUNT, RESET_MANAGER, HANDLE_RETURNED = 0x6B1020, 0x6B0CC0, 0x6B0C80, 0x6B0DB0
UNIT_GUARD, UNIT_AREA_GUARD = 0x740810, 0x744100
GUARD_NO_KICK, AREA_GUARD_NO_KICK = 0x740854, 0x74416C
UNIT_DEPLOY, GET_ZONE, FNPC_CALL = 0x7393C0, 0x56D230, 0x6B0417
MISSION = {'guard': 5, 'move': 2, 'harvest': 10, 'construction': 18, 'selling': 19, 'area_guard': 11,
           'hunt': 8, 'none': -1}
# The deployed refinery: 2x2 (Foundation index 3, art [YAREFN] Foundation=2x2),
# its drop cell NW + (FoundationWidth - 1, FoundationHeight / 2) = (13, 13).
YAREFN_NW = (12, 12)
SPOT_OFFSETS = [(128, 128, 0), (64, 64, 0), (192, 64, 0), (64, 192, 0), (192, 192, 0)]


def slave_address(index):
    return SLAVE_BASE + index * SLAVE_SIZE


def node_address(index):
    return NODE_BASE + index * 0x20


def slave_index(pointer):
    return None if pointer == 0 else (pointer - SLAVE_BASE) // SLAVE_SIZE


def owner_of(case):
    return ACTOR if case.get('owner') == 'unit' else YAREFN


def make_fixture(case):
    u, call, read32, events, (unused, reach) = field.fixture(
        dict(case, miner_cell=case.get('miner_cell', [2, 2]), many_scanners=True))
    u.mem_map(REGION, 0x10000 + SLAVE_SIZE * SLAVE_SLOTS)
    frame = read32(0xA8ED84)
    # PlaceInfantryInCell's spot offsets (0x89E9F0) and its no-spot answer
    # (0x89E778), and the cell-(0,0) centre DeploySlaves also refuses
    # (0xB0B618, compared at 0x6B0624; initialiser 0x6AF0E0), filled by
    # static initialisers the fixture does not run.
    u.mem_write(0x89E778, dwords(0, 0, 0))
    u.mem_write(0x89E9F0, dwords(*(v for spot in SPOT_OFFSETS for v in spot)))
    u.mem_write(0xB0B618, dwords(0x80, 0x80, 0))
    # [General] SlaveMinerShortScan/SlaveScan/LongScan/ScanCorrection (ReadRange
    # leptons) and SlaveMinerKickFrameDelay: retail 8, 14, 48, 3 cells and 150;
    # ApproachTargetResetMultiplier (ReadInt of retail "1.5").
    u.mem_write(RULES + 0x1780, dwords(*[cells * 256 for cells in case.get('slave_ranges', [8, 14, 48, 3])],
                                       case.get('kick_delay', 150)))
    u.mem_write(RULES + 0xDF8, dwords(case.get('approach_reset', 1)))
    # The deployed refinery: Building vtables over a supplied type.
    owner = owner_of(case)
    place_building(u, YAREFN, YAREFN_NW)
    u.mem_write(YAREFN + 0x520, dwords(YTYPE))
    u.mem_write(YAREFN + 0x6C, dwords(2000))
    u.mem_write(YAREFN + 0xAC, dwords(MISSION[case.get('owner_mission', 'guard') if owner == YAREFN else 'guard']))
    # BState (+0x534): 0 is BSTATE_CONSTRUCTION, the build-up.
    u.mem_write(YAREFN + 0x534, dwords(case.get('bstate', 0)))
    # Place_Down lists it in all four foundation cells (0x5683C0).
    for fy in range(YAREFN_NW[1], YAREFN_NW[1] + 2):
        for fx in range(YAREFN_NW[0], YAREFN_NW[0] + 2):
            u.mem_write(cell(fx, fy) + 0xE4, dwords(YAREFN))
    u.mem_write(YTYPE, dwords(0x7E4570))
    u.mem_write(YTYPE + 0xEF0, dwords(3))
    u.mem_write(YTYPE + 0xA0, dwords(2000))
    # SLAV: InfantryType vtable, Strength 125, Storage 4, HarvestRate 150,
    # MovementZone Infantry.
    u.mem_write(STYPE, dwords(0x7EB610))
    u.mem_write(STYPE + 0xA0, dwords(125))
    u.mem_write(STYPE + 0x800, dwords(case.get('slave_storage', 4)))
    u.mem_write(STYPE + 0xEB8, dwords(case.get('harvest_rate', 150)))
    u.mem_write(STYPE + 0x5B4, dwords(7))
    # The manager: owner, slave type, count, RegenRate, ReloadRate, node
    # vector, AI timer, state and frame.
    nodes = case.get('nodes', [])
    if owner == ACTOR:
        # SMIN: DeploysInto=YAREFN (+0x404), ResourceGatherer=/ResourceDestination=
        # (+0x5EC/+0x5ED), the row's mission, its MissionClass start frame
        # (+0xC0), NavCom and deploy-pending byte (+0x68C).
        u.mem_write(TYPE + 0x404, dwords(YTYPE))
        u.mem_write(TYPE + 0x5EC, bytes([1, 1]))
        u.mem_write(ACTOR + 0xAC, dwords(MISSION[case.get('owner_mission', 'guard')]))
        u.mem_write(ACTOR + 0xB4, dwords(-1))
        u.mem_write(ACTOR + 0xC0, dwords(frame + case.get('mission_start', 0)))
        u.mem_write(ACTOR + 0x68C, bytes([case.get('deploy_pending', False)]))
        nav = case.get('owner_nav')
        u.mem_write(ACTOR + 0x5A4, dwords(cell(*nav) if nav else 0))
        # [AreaGuard] Rate (MissionControl entry 11, as the fixture's other rates).
        u.mem_write(0xA8E3A8 + 11 * 32 + 0x10, struct.pack('<d', case.get('area_guard_rate', 0.016)))
    u.mem_write(MANAGER + 0x24, dwords(owner, STYPE, len(nodes), case.get('regen', 500), case.get('reload', 25)))
    u.mem_write(MANAGER + 0x38, dwords(0x7F322C, ITEMS, 16) + bytes([1, 0, 0, 0]) + dwords(len(nodes), 10))
    manager_frame = case.get('manager_frame', 0)
    if 'manager_frame_ago' in case:
        manager_frame = frame - case['manager_frame_ago']
    u.mem_write(MANAGER + 0x50, dwords(frame, 0, 10, case.get('manager_state', 5), manager_frame))
    u.mem_write(owner + 0x2D8, dwords(MANAGER))
    for index, node in enumerate(nodes):
        address = node_address(index)
        u.mem_write(ITEMS + index * 4, dwords(address))
        slave = slave_address(index) if node.get('slave', True) else 0
        start, duration = node.get('timer', [0, 0])
        u.mem_write(address, dwords(slave, node.get('state', 0), frame + start, 0, duration))
        if slave:
            make_slave(u, call, slave, node, owner)
    make_slave(u, call, slave_address(SPARE), dict(limbo=True), owner)
    return u, call, read32, events


def make_slave(u, call, slave, node, owner):
    loco = slave + 0x1000
    u.mem_write(slave, dwords(0x7EB058, 0x7EB03C, 0x7EB034, 0x7EB02C))
    u.mem_write(slave + 0x14, dwords(5))
    u.mem_write(slave + 0x21C, dwords(HOUSE))
    u.mem_write(slave + 0x6C0, dwords(STYPE))
    u.mem_write(slave + 0x6C4, dwords(node.get('doing', -1)))
    u.mem_write(slave + 0x6C, dwords(node.get('health', 125), node.get('health', 125)))
    u.mem_write(slave + 0x90, bytes([1]))
    u.mem_write(slave + 0x94, dwords(-1))  # in no display layer
    # In limbo (+0x81) until DeploySlaves; on the map (+0x74) otherwise.
    u.mem_write(slave + 0x74, bytes([not node.get('limbo', False)]))
    u.mem_write(slave + 0x81, bytes([node.get('limbo', False)]))
    x, y = node.get('cell', [15, 15])
    u.mem_write(slave + 0x9C, dwords(x * 256 + 128, y * 256 + 128, 0))
    u.mem_write(slave + 0xAC, dwords(MISSION[node.get('mission', 'guard')]))
    u.mem_write(slave + 0xB4, dwords(MISSION[node.get('queued', 'none')]))
    u.mem_write(slave + 0x2DC, dwords(owner))
    u.mem_write(slave + 0x684, b'\xff')
    u.mem_write(slave + 0x33C, struct.pack('<4f', *node.get('storage', [0, 0, 0, 0])))
    nav = node.get('nav')
    u.mem_write(slave + 0x5A4, dwords(cell(*nav) if nav else 0))
    call(WALK_CTOR, loco, [])
    u.mem_write(loco + 0xC, dwords(slave))
    u.mem_write(slave + 0x674, dwords(loco + 4))


def observe(u, read32, events, case):
    unlimbo = list(case.get('unlimbo', []))
    deploys = list(case.get('deploys', []))

    def ret(cleanup, value=0):
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value)
        u.reg_write(UC_X86_REG_EIP, read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    def coord(pointer):
        return list(struct.unpack('<3i', u.mem_read(pointer, 12)))

    def hook(_u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        this = u.reg_read(UC_X86_REG_ECX)
        if address == SETTER:
            target = read32(sp + 4)
            events.append(['set_destination', slave_index(this), cell_xy(target) if target else None,
                           read32(sp + 8) & 0xFF])
            u.mem_write(this + 0x5A4, dwords(target))
            ret(8)
        elif address == LIMBO:
            events.append(['limbo', slave_index(this)])
            u.mem_write(this + 0x81, b'\x01')
            ret(0, 1)
        elif address == UNLIMBO:
            events.append(['unlimbo', slave_index(this), coord(read32(sp + 4)), read32(sp + 8)])
            if unlimbo:
                ret(8, unlimbo.pop(0))
        elif address == FOOT_UNLIMBO:
            # FootClass::Unlimbo's placement, without its sight reveal and
            # layer submission: the coordinate, out of limbo, on the map.
            placed = coord(read32(sp + 4))
            events.append(['foot_unlimbo', slave_index(this), placed, read32(sp + 8)])
            u.mem_write(this + 0x9C, dwords(*placed))
            u.mem_write(this + 0x81, b'\x00')
            u.mem_write(this + 0x74, b'\x01')
            ret(8, 1)
        elif address == SCATTER:
            events.append(['scatter', slave_index(this), coord(read32(sp + 4)),
                           read32(sp + 8) & 0xFF, read32(sp + 12) & 0xFF])
            if case.get('supply_scatter'):
                ret(12)
        elif address == PLAY_ANIM:
            events.append(['play_anim', slave_index(this), read32(sp + 4)])
            u.mem_write(this + 0x6C4, dwords(read32(sp + 4)))
            ret(12, 1)
        elif address == CREATE_OBJECT:
            events.append(['create_slave', read32(sp + 4) == HOUSE])
            ret(4, slave_address(SPARE))
        elif address == UNIT_DEPLOY:
            assert deploys, ('unsupplied UnitClass::Deploy', case['name'])
            answer = deploys.pop(0)
            events.append(['unit_deploy', this == ACTOR, answer])
            ret(0, answer)
        elif address == GET_ZONE:
            events.append(['zone', list(struct.unpack('<hh', u.mem_read(read32(sp + 4), 4))),
                           read32(sp + 8), read32(sp + 12) & 0xFF])
            ret(12, case.get('zone', 1))
        elif address == FNPC_CALL:
            # FindDeployCell's pushed Find_Nearby_Passable_Cell arguments:
            # out, seed, SpeedType, zone, MovementZone, bridge-aware, W, H,
            # reject-overlay, height gate, obstacle gate, allow bridge,
            # reference cell, quadrant skip, occupancy.
            args = [read32(sp + 4 * i) for i in range(15)]
            pair = lambda pointer: list(struct.unpack('<hh', u.mem_read(pointer, 4)))
            events.append(['deploy_cell_query', pair(args[1]), *args[2:12], pair(args[12]), args[13], args[14]])

    u.hook_add(UC_HOOK_CODE, hook)


def run(case, entry, this, args=(), stops=()):
    u, call, read32, events = make_fixture(case)
    observe(u, read32, events, case)
    u.mem_write(SP, dwords(RET_MAGIC, *args))
    u.reg_write(UC_X86_REG_ECX, this)
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, entry, (RET_MAGIC, *stops), count=5_000_000, required_addresses=[entry])
    return u, read32, events


def state(u, read32, case):
    signed = lambda address: struct.unpack('<i', u.mem_read(address, 4))[0]
    nodes = []
    for index in range(len(case.get('nodes', []))):
        address = node_address(index)
        nodes.append(dict(slave=slave_index(read32(address)), state=signed(address + 4),
                          timer=[signed(address + 8), signed(address + 16)]))
    slaves = {}
    present = [index for index, node in enumerate(case.get('nodes', [])) if node.get('slave', True)]
    owner = owner_of(case)
    for index in sorted(set(present + [SPARE])):
        slave = slave_address(index)
        slaves[str(index)] = dict(
            mission=signed(slave + 0xAC), queued=signed(slave + 0xB4), nav=cell_xy(read32(slave + 0x5A4)),
            archive=cell_xy(read32(slave + 0x218)), doing=signed(slave + 0x6C4),
            storage=list(struct.unpack('<4f', u.mem_read(slave + 0x33C, 16))),
            health=[signed(slave + 0x6C), signed(slave + 0x70)], owner=read32(slave + 0x2DC) == owner,
            limbo=u.mem_read(slave + 0x81, 1)[0], coord=list(struct.unpack('<3i', u.mem_read(slave + 0x9C, 12))))
    base = field.field_state(u, read32, case)
    result = dict(nodes=nodes, slaves=slaves, manager_state=signed(MANAGER + 0x5C),
                  manager_frame=signed(MANAGER + 0x60), frame=read32(0xA8ED84),
                  cells=base['cells'], balance=base['balance'], random_indices=base['random_indices'])
    if owner == ACTOR:
        result['owner'] = dict(mission=signed(ACTOR + 0xAC), queued=signed(ACTOR + 0xB4),
                               nav=cell_xy(read32(ACTOR + 0x5A4)), deploy_pending=u.mem_read(ACTOR + 0x68C, 1)[0])
        result['ai_timer'] = [signed(MANAGER + 0x50), signed(MANAGER + 0x58)]
    return result


def ai_update(case):
    u, read32, events = run(case, AI_UPDATE, MANAGER)
    return dict(input=case, events=events, state=state(u, read32, case))


def manager(case):
    u, read32, events = run(case, MANAGER_AI, MANAGER)
    return dict(input=case, events=events, state=state(u, read32, case))


def deploy(case):
    u, read32, events = run(case, DEPLOY_SLAVES, MANAGER)
    return dict(input=case, events=events, state=state(u, read32, case))


def slave_harvest(case):
    u, read32, events = run(case, SLAVE_HARVEST, slave_address(0))
    delay = struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0]
    return dict(input=case, delay=delay, events=events, state=state(u, read32, case))


def deposit(case):
    u, read32, events = run(case, DEPOSIT, slave_address(0), (YAREFN,))
    return dict(input=case, events=events, state=state(u, read32, case))


def unit_helper(case):
    """One manager helper on a Slave Miner's manager: ShouldRecallSlaves
    (its AL answer), the hunt start 0x6B0CC0, the reset 0x6B0C80 or
    HandleReturnedSlaves 0x6B0DB0."""
    entry = {'should_recall': SHOULD_RECALL, 'begin_hunt': BEGIN_HUNT, 'reset': RESET_MANAGER,
             'handle_returned': HANDLE_RETURNED}[case['helper']]
    u, read32, events = run(case, entry, MANAGER)
    result = dict(input=case, events=events, state=state(u, read32, case))
    if case['helper'] == 'should_recall':
        result['answer'] = u.reg_read(UC_X86_REG_EAX) & 0xFF
    return result


def unit_mission(case):
    """UnitClass::Mission_Guard 0x740810 or Mission_AreaGuard 0x744100 on the
    Slave Miner up to its kick: the delay when it kicks, else the address
    where the ordinary mission continues."""
    entry, stop = {'guard': (UNIT_GUARD, GUARD_NO_KICK),
                   'area_guard': (UNIT_AREA_GUARD, AREA_GUARD_NO_KICK)}[case['owner_mission']]
    u, read32, events = run(case, entry, ACTOR, stops=(stop,))
    kicked = u.reg_read(UC_X86_REG_EIP) == RET_MAGIC
    delay = struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0] if kicked else None
    return dict(input=case, kicked=kicked, delay=delay, events=events, state=state(u, read32, case))


DROP = [13, 13]
ORE = [17, 15, 0, 0, 5]


def ai_update_cases():
    s = lambda **node: [dict(node)]
    return [
        # 1 Scan: SlaveMinerSlaveScan (14 cells) from the slave's cell.
        dict(name='n1_scan_hit', ore=[ORE], nodes=s(state=1, cell=[15, 15])),
        dict(name='n1_scan_miss', ore=[], nodes=s(state=1, cell=[15, 15])),
        dict(name='n1_on_ore', ore=[[15, 15, 0, 0, 5]], nodes=s(state=1, cell=[15, 15])),
        dict(name='n1_beyond_slave_scan', ore=[[29, 15, 0, 0, 5]], nodes=s(state=1, cell=[15, 15])),
        # 2 Move: on Tiberium land -> Harvest; else wait for the NavCom.
        dict(name='n2_arrived', ore=[ORE], nodes=s(state=2, cell=[17, 15], mission='move')),
        dict(name='n2_arrived_harvesting', ore=[ORE], nodes=s(state=2, cell=[17, 15], mission='harvest')),
        dict(name='n2_moving', ore=[ORE], nodes=s(state=2, cell=[16, 15], mission='move', nav=[17, 15])),
        dict(name='n2_stopped', ore=[ORE], nodes=s(state=2, cell=[16, 15], mission='guard')),
        # 3 Harvest: full -> archive the cell and return; else keep cutting.
        dict(name='n3_full', ore=[ORE], nodes=s(state=3, cell=[17, 15], mission='harvest', storage=[4, 0, 0, 0])),
        dict(name='n3_full_gems', ore=[[17, 15, 1, 0, 5]],
             nodes=s(state=3, cell=[17, 15], mission='harvest', storage=[1, 3, 0, 0])),
        dict(name='n3_cutting', ore=[ORE], nodes=s(state=3, cell=[17, 15], mission='harvest', storage=[1, 0, 0, 0])),
        dict(name='n3_dry', ore=[], nodes=s(state=3, cell=[17, 15], mission='harvest', storage=[1, 0, 0, 0])),
        dict(name='n3_mission_lost', ore=[ORE], nodes=s(state=3, cell=[17, 15], mission='guard', storage=[1, 0, 0, 0])),
        # 4 Bring it back: deposit and enter at the drop cell; re-path when
        # the NavCom has drifted more than ApproachTargetResetMultiplier cells.
        dict(name='n4_at_drop', ore=[], nodes=s(state=4, cell=DROP, mission='guard', storage=[3, 0, 0, 0])),
        dict(name='n4_at_drop_gems', ore=[], nodes=s(state=4, cell=DROP, mission='guard', storage=[1, 2, 0, 0])),
        dict(name='n4_at_drop_empty', ore=[], nodes=s(state=4, cell=DROP, mission='guard')),
        dict(name='n4_at_drop_still_moving', ore=[],
             nodes=s(state=4, cell=DROP, mission='move', nav=DROP, storage=[3, 0, 0, 0])),
        dict(name='n4_en_route', ore=[], nodes=s(state=4, cell=[16, 16], mission='move', nav=DROP)),
        dict(name='n4_en_route_near', ore=[], nodes=s(state=4, cell=[16, 16], mission='move', nav=[14, 13])),
        dict(name='n4_en_route_far', ore=[], nodes=s(state=4, cell=[16, 16], mission='move', nav=[20, 20])),
        dict(name='n4_stopped_elsewhere', ore=[], nodes=s(state=4, cell=[16, 16], mission='guard')),
        # 5 Reload inside the refinery (ReloadRate), then 0.
        dict(name='n5_reloading', ore=[], nodes=s(state=5, cell=DROP, timer=[-10, 25], health=60, limbo=True)),
        dict(name='n5_reloaded', ore=[], nodes=s(state=5, cell=DROP, timer=[-25, 25], health=60, limbo=True)),
        # 6 Dead: regenerate after RegenRate.
        dict(name='n6_waiting', ore=[], nodes=s(state=6, slave=False, timer=[-100, 500])),
        dict(name='n6_regen', ore=[], nodes=s(state=6, slave=False, timer=[-500, 500])),
        dict(name='n_lost_slave', ore=[], nodes=s(state=3, slave=False)),
        dict(name='n0_idle', ore=[], nodes=s(state=0, cell=DROP, limbo=True)),
        # Several nodes in vector order.
        dict(name='nodes_in_order', ore=[ORE, [15, 17, 0, 0, 5]], nodes=[
            dict(state=1, cell=[15, 15]), dict(state=4, cell=DROP, mission='guard', storage=[4, 0, 0, 0]),
            dict(state=2, cell=[16, 15], mission='guard'), dict(state=1, cell=[14, 16])]),
    ]


def manager_cases():
    ready = [dict(state=0, cell=DROP, limbo=True)] * 3
    return [
        dict(name='m0_guard', manager_state=0, owner_mission='guard'),
        dict(name='m0_construction', manager_state=0, owner_mission='construction'),
        dict(name='m0_selling', manager_state=0, owner_mission='selling'),
        # 4, a refinery just deployed from a Slave Miner: waits for its BState.
        dict(name='m4_building_up', manager_state=4, bstate=0),
        dict(name='m4_built', manager_state=4, bstate=1),
        # 5 with ore inside SlaveMinerShortScan of the refinery's centre: deploy.
        dict(name='m5_deploys', manager_state=5, ore=[[16, 13, 0, 0, 5]], nodes=ready),
        dict(name='m5_deploys_mixed', manager_state=5, ore=[[16, 13, 0, 0, 5]], nodes=[
            dict(state=0, cell=DROP, limbo=True), dict(state=3, cell=[16, 13], mission='harvest'),
            dict(state=5, cell=DROP, limbo=True, timer=[-5, 25]), dict(state=0, cell=DROP, limbo=True)]),
        dict(name='m6_building', manager_state=6, ore=[]),
    ]


SMIN = [15, 15]
FIELD = [20, 15, 0, 0, 5]
# The fixture map is 32x32: a 12-cell SlaveMinerLongScan keeps every ring of a
# Slave Miner's scan from (15, 15) on it.
UNIT_RANGES = [8, 14, 12, 3]


def unit_manager_cases():
    """The manager machine for a Slave Miner owner (states 1..6)."""
    unit = dict(owner='unit', miner_cell=SMIN, harvester=False, slave_ranges=UNIT_RANGES)
    out = [dict(state=3, cell=[16, 16], mission='harvest'), dict(state=0, cell=SMIN, limbo=True),
           dict(state=6, slave=False, timer=[-10, 500])]
    return [
        dict(unit, name='mu0_unit', manager_state=0, ore=[FIELD]),
        dict(unit, name='mu1_nav', manager_state=1, ore=[FIELD], owner_nav=[25, 25]),
        dict(unit, name='mu1_no_field', manager_state=1, ore=[]),
        dict(unit, name='mu1_field', manager_state=1, ore=[FIELD], passable=[[20, 16]]),
        dict(unit, name='mu1_no_deploy_cell', manager_state=1, ore=[FIELD], passable=[None]),
        dict(unit, name='mu1_on_ore', manager_state=1, ore=[[15, 15, 0, 0, 5]], passable=[[15, 16]]),
        dict(unit, name='mu2_driving', manager_state=2, owner_nav=[21, 13]),
        dict(unit, name='mu2_deploys', manager_state=2, deploys=[1]),
        dict(unit, name='mu2_deploy_refused', manager_state=2, deploys=[0]),
        dict(unit, name='mu2_building_owner', manager_state=2, owner='building'),
        dict(unit, name='mu3_deploys', manager_state=3, deploys=[1]),
        dict(unit, name='mu3_deploy_refused', manager_state=3, deploys=[0]),
        dict(unit, name='mu4_pending', manager_state=4, deploy_pending=True),
        dict(unit, name='mu4_guard', manager_state=4),
        dict(unit, name='mu4_moving', manager_state=4, owner_mission='move'),
        dict(unit, name='mu5_unit', manager_state=5, nodes=out),
        dict(unit, name='mu6_unit', manager_state=6, nodes=out),
    ]


def unit_helper_cases():
    unit = dict(owner='unit', miner_cell=SMIN, harvester=False, slave_ranges=UNIT_RANGES)
    out = [dict(state=3, cell=[16, 16], mission='harvest'), dict(state=0, cell=SMIN, limbo=True),
           dict(state=6, slave=False, timer=[-10, 500])]
    recall = dict(unit, helper='should_recall', manager_state=0)
    return [
        # ShouldRecallSlaves 0x6B1020: a computer house always; a human's on
        # Tiberium land, or past KickFrameDelay (150) with ore within
        # SlaveMinerShortScan (8).
        dict(recall, name='sr_busy', manager_state=2, human=False),
        dict(recall, name='sr_computer', human=False, ore=[]),
        dict(recall, name='sr_human_on_ore', ore=[[15, 15, 0, 0, 5]]),
        dict(recall, name='sr_human_kick_pending', manager_frame_ago=150, ore=[[18, 15, 0, 0, 5]]),
        dict(recall, name='sr_human_kicked_near', manager_frame_ago=151, ore=[[18, 15, 0, 0, 5]]),
        dict(recall, name='sr_human_kicked_far', manager_frame_ago=151, ore=[[25, 15, 0, 0, 5]]),
        # 0x6B0CC0: an idle manager starts the hunt; its live slaves reset.
        dict(unit, name='hunt_idle', helper='begin_hunt', manager_state=0, nodes=out),
        dict(unit, name='hunt_busy', helper='begin_hunt', manager_state=2, nodes=out),
        # 0x6B0C80: any state back to 0 at now; live slaves reset.
        dict(unit, name='reset_travelling', helper='reset', manager_state=2, nodes=out),
        # HandleReturnedSlaves 0x6B0DB0 (Mission_Harvest's Enslaves prologue).
        dict(unit, name='hr_nav_field', helper='handle_returned', manager_state=0, owner_nav=[20, 15],
             owner_mission='harvest', ore=[FIELD], passable=[[20, 16]], nodes=out),
        dict(unit, name='hr_nav_no_cell', helper='handle_returned', manager_state=0, owner_nav=[20, 15],
             owner_mission='harvest', ore=[FIELD], passable=[None], nodes=out),
        dict(unit, name='hr_no_nav', helper='handle_returned', manager_state=0, owner_mission='harvest',
             nodes=out),
    ]


def unit_mission_cases():
    unit = dict(owner='unit', miner_cell=SMIN, harvester=False, manager_state=0, slave_ranges=UNIT_RANGES)
    return [
        # MissionClass start frame (+0xC0) + KickFrameDelay (150) must lie
        # before now, then ShouldRecallSlaves.
        dict(unit, name='guard_kick_computer', owner_mission='guard', human=False, mission_start=-151),
        dict(unit, name='guard_kick_too_soon', owner_mission='guard', human=False, mission_start=-150),
        dict(unit, name='guard_kick_human_near', owner_mission='guard', mission_start=-151,
             manager_frame_ago=151, ore=[[18, 15, 0, 0, 5]]),
        dict(unit, name='guard_no_kick_human_far', owner_mission='guard', mission_start=-151,
             manager_frame_ago=151, ore=[[25, 15, 0, 0, 5]]),
        dict(unit, name='guard_no_kick_hunting', owner_mission='guard', human=False, mission_start=-151,
             manager_state=2),
        dict(unit, name='area_guard_kick_computer', owner_mission='area_guard', human=False,
             mission_start=-151),
        dict(unit, name='area_guard_too_soon', owner_mission='area_guard', human=False, mission_start=-10),
    ]


def deploy_cases():
    ready = dict(state=0, cell=DROP, limbo=True)
    return [
        dict(name='d_one', ore=[], nodes=[ready]),
        dict(name='d_three', ore=[], nodes=[ready] * 3),
        dict(name='d_five', ore=[], nodes=[ready] * 5),
        dict(name='d_unlimbo_refused', ore=[], nodes=[ready] * 2, unlimbo=[0, 1]),
        dict(name='d_skips_busy', ore=[], nodes=[ready, dict(state=1, cell=[15, 15]), ready]),
    ]


def slave_harvest_cases():
    on = dict(state=3, cell=[15, 15], mission='harvest')
    return [
        dict(name='h_cut_ore', ore=[[15, 15, 0, 0, 5]], nodes=[on]),
        dict(name='h_cut_ore_in_anim', ore=[[15, 15, 0, 0, 5]], nodes=[dict(on, doing=0x26)]),
        dict(name='h_cut_gem', ore=[[15, 15, 1, 0, 5]], nodes=[dict(on, storage=[1, 0, 0, 0])]),
        dict(name='h_last_level', ore=[[15, 15, 0, 0, 1]], nodes=[on]),
        dict(name='h_empty_level', ore=[[15, 15, 0, 0, 0]], nodes=[on]),
        dict(name='h_fills', ore=[[15, 15, 0, 0, 5]], nodes=[dict(on, storage=[3, 0, 0, 0])]),
        dict(name='h_full', ore=[[15, 15, 0, 0, 5]], nodes=[dict(on, storage=[4, 0, 0, 0])]),
        dict(name='h_off_ore', ore=[], nodes=[on]),
        dict(name='h_no_storage', ore=[[15, 15, 0, 0, 5]], nodes=[on], slave_storage=0),
    ]


def deposit_cases():
    home = dict(state=4, cell=DROP)
    return [
        dict(name='dep_ore', ore=[], nodes=[dict(home, storage=[3, 0, 0, 0])]),
        dict(name='dep_gems', ore=[], nodes=[dict(home, storage=[0, 2, 0, 0])]),
        dict(name='dep_mixed', ore=[], nodes=[dict(home, storage=[2, 1, 0, 0])]),
        dict(name='dep_purifier', ore=[], nodes=[dict(home, storage=[4, 0, 0, 0])], purifiers=1),
        # An AI house outside the campaign adds AIVirtualPurifiers (4,2,0) by
        # difficulty; IncomeMult scales the credits.
        dict(name='dep_ai_hard', ore=[], nodes=[dict(home, storage=[4, 0, 0, 0])], human=False, difficulty=0),
        dict(name='dep_ai_easy', ore=[], nodes=[dict(home, storage=[4, 0, 0, 0])], human=False, difficulty=2),
        dict(name='dep_ai_campaign', ore=[], nodes=[dict(home, storage=[4, 0, 0, 0])], human=False,
             difficulty=0, game_mode=0),
        dict(name='dep_income_mult', ore=[], nodes=[dict(home, storage=[4, 0, 0, 0])], income_mult=0.9),
        dict(name='dep_empty', ore=[], nodes=[dict(home)]),
    ]


def generate():
    return {'source': 'unicorn/gamemd.exe',
            'ai_update': [ai_update(case) for case in ai_update_cases()],
            'manager': [manager(case) for case in manager_cases()],
            'deploy': [deploy(case) for case in deploy_cases()],
            'slave_harvest': [slave_harvest(case) for case in slave_harvest_cases()],
            'deposit': [deposit(case) for case in deposit_cases()],
            'unit_manager': [manager(case) for case in unit_manager_cases()],
            'unit_helper': [unit_helper(case) for case in unit_helper_cases()],
            'unit_mission': [unit_mission(case) for case in unit_mission_cases()]}


def main(argv=None):
    finish_vectors(
        generate, Path(__file__).with_suffix('.json'),
        provenance=lambda: provenance(
            scope='SlaveManagerClass AI_Update 0x6AF6C0, manager state machine 0x6AFD60 (Building owner '
                  'states 0/4/5/6, Unit owner states 0..6), DeploySlaves 0x6B04C0, '
                  'InfantryClass::Mission_Harvest 0x522E70, the slave deposit 0x522D50, ShouldRecallSlaves '
                  '0x6B1020, the hunt start 0x6B0CC0, the reset 0x6B0C80, HandleReturnedSlaves 0x6B0DB0 and '
                  'the Slave Miner Guard/AreaGuard kick (0x740810/0x744100)',
            entry_points={'ai_update': AI_UPDATE, 'manager_ai': MANAGER_AI, 'deploy_slaves': DEPLOY_SLAVES,
                          'slave_mission_harvest': SLAVE_HARVEST, 'slave_deposit': DEPOSIT,
                          'infantry_unlimbo': UNLIMBO, 'should_recall_slaves': SHOULD_RECALL,
                          'begin_hunt': BEGIN_HUNT, 'reset_manager': RESET_MANAGER,
                          'handle_returned_slaves': HANDLE_RETURNED, 'unit_mission_guard': UNIT_GUARD,
                          'unit_mission_area_guard': UNIT_AREA_GUARD},
            assumptions=['harvest_field fixture (refinery_dock map, House, Rules, Scenario RNG, ore/gem '
                         'tables); a 2x2 Building (vtable 0x7E3EBC, Foundation index 3) at NW (12,12) as the '
                         'owner, first object of its four foundation cells; the manager and its nodes '
                         'supplied at REGION without the constructor.',
                         'PlaceInfantryInCell spot offsets (0x89E9F0) seeded as the static initialiser '
                         'leaves them: centre, then (64,64), (192,64), (64,192), (192,192).',
                         'Slaves: original Infantry vtables (0x7EB058/03C/034/02C) over supplied fields and a '
                         'constructed Walk (0x75AA90); SLAV type Strength 125, Storage 4, HarvestRate 150, '
                         'MovementZone Infantry.',
                         'Rules SlaveMiner ranges 8/14/48/3 cells (leptons), KickFrameDelay 150, '
                         'ApproachTargetResetMultiplier 1 (ReadInt of retail 1.5); Slave Miner rows use a '
                         '12-cell LongScan so every scan ring stays on the 32x32 map.',
                         'Slave Miner owner: the fixture Unit (Drive, MovementZone Crusher, Harvester=no) with '
                         'DeploysInto=YAREFN (+0x404), ResourceGatherer/ResourceDestination (+0x5EC/+0x5ED), the '
                         'manager at +0x2D8, MissionClass start frame +0xC0 and deploy-pending +0x68C supplied; '
                         'MissionControl Rate 0.03 (Guard) and 0.016 (AreaGuard).'],
            substitutions=['Infantry setter 0x51AA40 (writes the NavCom), Limbo 0x51DF10 (sets +0x81) and '
                           'PlayAnim 0x51D6F0 observed and answered; InfantryType CreateObject 0x523B10 '
                           'returns a prepared spare; InfantryClass::Unlimbo 0x51DFF0 native (its '
                           'PlaceInfantryInCell and occupy mark) unless a row supplies answers, with '
                           'FootClass::Unlimbo 0x4D7170 answered after writing the coordinate and the '
                           'limbo/on-map bytes (no sight reveal or layer submission); Scatter 0x51D0D0 '
                           'native; harvest_field and refinery_dock observers. Slave Miner rows: '
                           'UnitClass::Deploy 0x7393C0 answered from `deploys`, MapClass::GetZoneID 0x56D230 '
                           'answered (zone 1), Find_Nearby_Passable_Cell 0x56DC20 answered from `passable` by '
                           'the dock observer with its arguments recorded at 0x6B0417.']),
        argv=argv)


if __name__ == '__main__':
    main()
