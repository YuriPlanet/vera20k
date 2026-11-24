"""Native references for a computer team's recruitment, centre and scripts.

Run python -m tools.team_recruit_oracle --check (or explicit --write).
The bounded Script64 seek controls use --nearest-occupiable --check/--write;
Script16 uses --patrol --check/--write; Script37 uses --delete-members for
bounded original loop controls. Each has its own corpus; the
ordinary reference is unchanged. Under-strength retreat and directional Foot
nearby controls use --retreat; consumed by retreat_tests.rs and find_nearby_cell.rs.
Rust consumers: src/sim/team_script_vm/recruit_oracle_tests.rs,
src/sim/building_entry_tests.rs (the seeks) and src/sim/docking/bunker_link.rs
(the tank bunker admission).

Sections, each executed in a fresh emulator per case:
- recalc: TeamClass::Recalc 0x6EA3E0 with the TaskForce total 0x6E8160 (the
  full and under-strength bytes, a Reinforce= TeamType's third, the empty
  team's destruction).
- center: TeamClass::Calc_Center 0x6EAEE0 off action 10 (the member tests,
  the GuardSlower= double count, the mean, the closest member by 0x5F6500
  and the head fallback).
- recruit: TeamClass::Recruit 0x6EAA90 over the Infantry, Unit and Aircraft
  arrays (the group filter and penalty, the 0x5F6560 key, the Unit search's
  house and type test).
- distance: ObjectClass::Distance 0x5F6360 (Sqrt_Approx 0x4CAC40, ftol
  0x7C5F00, a building's foundation deduction over every foundation).
- guard: TeamClass::AI's action 5 timer 0x6E97CE..0x6E97EC.
- regroup: action 54 0x6EFA10 up to its FNPC call 0x6EFC3A (the enemy's
  base centre and the atan2 facing 0x4CAE30, or the RandomRanged(0, 255)
  facing; the sin/cos step 0x4CACB0/0x4CAD00 and the cell quotients).
- own_building: FindOwnBuilding 0x6EEEA0.
- target_building: Find_Best_Target_Building 0x6EEBD0, actions 46/47's
  pick over BuildingClass::Array (the type, enemy, IsAlliedWith 0x4F9A50 and
  MultiplayPassive tests, the GetCoords cell each threat read takes, the
  Sqrt_Approx distance, the two bests and OnlyTargetHouseEnemy=).
- gather: action 53 0x6EF700 with `first` up to its FNPC call 0x6EF98A, or
  to its return when it finishes at once (the enemy's and own base centres,
  the leader's location without a base, the same atan2/sin/cos step as
  action 54 from the enemy's centre).
- quarry: Quarry_To_Threat 0x645BB0 and its jump table 0x645BF8.
- attack_cadence: Coordinate_Attack's signed Frame % 8 == 4 test
  0x6EB59A..0x6EB5AE.
- bio_reactor: FootClass::EnterBioReactor 0x4DFB70, action 62's Foot seek
  (the house's absorber list walked from its end, the CAN_LOAD answers, the
  Sqrt_Approx distance and its ties, the latch +0x68F, the radio-contact and
  pending-entry test, the Enter and destination calls).
- battle_bunker: FootClass::EnterBattleBunker 0x4DFCB0, action 63's Foot
  seek (the house's occupiable list walked from its end, the distance, the
  CanBeOccupiedBy 0x457CE0 answers asked only of a nearer building, with the
  Foot or NULL, the latch +0x690, the NavCom-and-Capture test, the
  destination and Capture calls).
- tank_bunker: FootClass::EnterTankBunker 0x4DFF40, action 61's Foot seek
  (the admission 0x70FB50 run as original, each of its tests refusing
  alone; the house's tank bunker list walked from its end, the distance and
  its ties, the first radio contact 0x65AD30 and occupant +0x2E4 read only
  of a nearer bunker; the destination, HELLO and Enter calls).
- load: action 14 0x6ED200 (the TaskForce's aircraft test, the transport
  pick, the radio-contact wait, the join-up, one Enter order per update,
  the completion and the TransportsReturnOnUnload= archive).
- wait_loaded: action 43's inline test 0x6E9C22..0x6E9C5C.
- unload: action 8 0x6EF110 (the join-up, the Unload orders with their
  building and mission tests, then the sort into transports and others by
  the argument and TransportsReturnOnUnload=).
- deploy: action 9 0x6ED4D0 (the join-up; a DeploysInto= unit's Unload
  test, Mark, the placement question at its GetMapCoords cell 0x41BEA0 for
  no house, Flush_For_Placement or the orders; the IsSimpleDeployer= and
  Deployer= Unload orders; the completion).

Control flow the rows do not vary (Calc_Center's action-10 branch, the
TeamType waypoint origin, the recruited unit's passengers, action 54's leader
loop) rests on instruction reading; see src/sim/team_script_vm/.
"""
from pathlib import Path
import random
import struct
import sys

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX,
                               UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP,
                               UC_X86_REG_FPCW)

from tools.ai_base_building_oracle import FAKE, FRAME, RULES, STUBS, Emu, u32
from tools.native_oracle import (NATIVE_FPCW, RET_MAGIC, STACK_BASE, STACK_SIZE,
                                 finish_vectors, provenance, run_checked)

TEAM = FAKE + 0x100000
TEAM_VTABLE = FAKE + 0x100800
TEAM_TYPE = FAKE + 0x101000
TASK_FORCE = FAKE + 0x102000
SCRIPT = FAKE + 0x103000
HOUSES = FAKE + 0x104000
HOUSE_SIZE = 0x6000
TYPES = FAKE + 0x120000
TYPE_SIZE = 0x2000
TYPE_VTABLE = FAKE + 0x160000
OBJECTS = FAKE + 0x200000
OBJECT_SIZE = 0x800
OBJECT_VTABLE = FAKE + 0x300000
BUILDING_VTABLE = FAKE + 0x300800
BUILDING_TYPES = FAKE + 0x310000
ARRAYS = FAKE + 0x340000
CELL = FAKE + 0x350000

STUB_RTTI = STUBS + 0x100
STUB_TECHNO_TYPE = STUBS + 0x110
STUB_WEIGHT = STUBS + 0x120
STUB_CAN_ENTER = STUBS + 0x130
STUB_SET_TARGET = STUBS + 0x140
STUB_DELETE = STUBS + 0x150
STUB_BUILDING_COORDS = STUBS + 0x160
STUB_TRANSMIT = STUBS + 0x170
STUB_QUEUE_MISSION = STUBS + 0x180
STUB_ASSIGN_DESTINATION = STUBS + 0x190

GET_COORDS = 0x5F65A0
SCENARIO_INIT = 0xA8E7AC
HOUSE_ARRAY = 0xA8022C
INFANTRY = (0xA83DEC, 0xA83DF8)
UNITS = (0x8B410C, 0x8B4118)
AIRCRAFT = (0xA8E394, 0xA8E3A0)

RTTI_UNIT, RTTI_AIRCRAFT, RTTI_BUILDING, RTTI_INFANTRY = 1, 2, 6, 15
TYPE_RTTI = {'infantry': 0x10, 'unit': 0x28, 'aircraft': 0x03}
CLASS_ARRAY = {'infantry': INFANTRY, 'unit': UNITS, 'aircraft': AIRCRAFT}


def i32(value):
    return struct.unpack('<i', u32(value))[0]


class TeamEmu(Emu):
    """A fresh image with one team, its TeamType and TaskForce, and objects
    whose GetCoords (vt+0x48) is the original 0x5F65A0."""

    def __init__(self):
        super().__init__()
        self.rtti = {}
        self.techno_type = {}
        self.weight = {}
        self.can_enter = {}
        for slot, stub in ((0x2C, STUB_RTTI), (0x84, STUB_TECHNO_TYPE), (0x4E0, STUB_WEIGHT),
                           (0x1AC, STUB_CAN_ENTER), (0x3C8, STUB_SET_TARGET)):
            self.write32(OBJECT_VTABLE + slot, stub)
            self.write32(BUILDING_VTABLE + slot, stub)
        self.write32(OBJECT_VTABLE + 0x48, GET_COORDS)
        self.write32(BUILDING_VTABLE + 0x48, STUB_BUILDING_COORDS)
        self.write32(TYPE_VTABLE + 0x2C, STUB_RTTI)
        self.write32(TEAM_VTABLE + 0x20, STUB_DELETE)
        self.write32(TEAM, TEAM_VTABLE)
        self.write32(TEAM + 0x24, TEAM_TYPE)
        self.write32(TEAM + 0x28, SCRIPT)
        self.write32(TEAM_TYPE + 0xE4, TASK_FORCE)
        self.hook(STUB_RTTI, lambda e: e.rtti[e.uc.reg_read(UC_X86_REG_ECX)], 0)
        self.hook(STUB_TECHNO_TYPE, lambda e: e.techno_type[e.uc.reg_read(UC_X86_REG_ECX)], 0)
        self.hook(STUB_WEIGHT, lambda e: e.weight[e.uc.reg_read(UC_X86_REG_ECX)], 0)
        self.hook(STUB_CAN_ENTER, self.can_enter_stub, 20)
        self.hook(STUB_SET_TARGET, lambda e: e.events.append(
            ['set_target', self.index_of(e.uc.reg_read(UC_X86_REG_ECX)), e.arg(0)]), 4)
        self.hook(STUB_DELETE, lambda e: e.events.append(['delete', e.arg(0)]), 4)

    def can_enter_stub(self, e):
        member = e.uc.reg_read(UC_X86_REG_ECX)
        e.events.append(['can_enter', self.index_of(member), e.arg(0) == CELL,
                         i32(e.arg(1)), i32(e.arg(2)), e.arg(3), e.arg(4)])
        return e.can_enter[member]

    @staticmethod
    def index_of(address):
        return (address - OBJECTS) // OBJECT_SIZE

    def write8(self, address, value):
        self.uc.mem_write(address, bytes([value & 0xFF]))

    def read8(self, address):
        return self.uc.mem_read(address, 1)[0]

    def object(self, index, *, xyz=(0, 0, 0), vtable=OBJECT_VTABLE, rtti=RTTI_INFANTRY):
        address = OBJECTS + index * OBJECT_SIZE
        self.write32(address, vtable)
        for offset, value in zip((0x9C, 0xA0, 0xA4), xyz):
            self.write32(address + offset, value)
        self.rtti[address] = rtti
        return address

    def techno_type_at(self, index, *, rtti=None, passengers=0, naval=False):
        address = TYPES + index * TYPE_SIZE
        self.write32(address, TYPE_VTABLE)
        self.write32(address + 0x5E0, passengers)
        self.write8(address + 0xCCE, int(naval))
        if rtti is not None:
            self.rtti[address] = rtti
        return address

    def start(self, entry, end, *, ecx, args=()):
        """Run `entry` (thiscall) from a fresh stack to `end` (one address or
        several); answer the address reached."""
        uc = self.uc
        sp = STACK_BASE + STACK_SIZE - 0x1000
        for value in reversed(args):
            sp -= 4
            uc.mem_write(sp, u32(value))
        sp -= 4
        uc.mem_write(sp, u32(RET_MAGIC))
        uc.reg_write(UC_X86_REG_ESP, sp)
        uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        uc.reg_write(UC_X86_REG_ECX, ecx)
        return run_checked(uc, entry, end)


# ---------------------------------------------------------- retreat / Foot nearby

FOOT_NEARBY = 0x4CBC40
RETREAT = 0x6EA0D0
CELL_LOOKUP = 0x5657A0
CELL_VTABLE = 0x7E4EEC
RETREAT_MAP_TABLE = 0xC00000
STUB_RETREAT_WEAPON = STUBS + 0xF00


class RetreatEmu(TeamEmu):
    """Original retreat and Foot search; supplied class entry/weapon answers.

    Distinct real Cells are essential: Calc_Center keeps a Cell pointer while
    later threat/nearby lookups run. A single mutable fake Cell corrupts it.
    """

    def __init__(self, *, accept=-1, blocked=()):
        self.cells = {}
        self.visits = []
        self.accept = accept
        self.blocked = set(map(tuple, blocked))
        self.building_inputs = {}
        super().__init__()
        # 56BCD0 performs its own inline fixed-stride lookup through this
        # original Map field. Keep it original, including the House grid read.
        self.write32(0x87F7E8 + 0x13C, RETREAT_MAP_TABLE)
        self.write32(0x87F7E8 + 0x140, 0x40000)
        self.hook(CELL_LOOKUP, lambda e: e.cell(struct.unpack(
            '<hh', e.uc.mem_read(e.arg(0), 4))), 4)
        self.hook(GET_CELL_AT, lambda e: e.cell(tuple(
            trunc_div256(v) for v in struct.unpack('<iii', e.uc.mem_read(e.arg(0), 12))[:2])), 4)
        self.draws([])

    def cell(self, point):
        if point not in self.cells:
            address = ARRAYS + 0x1000 + len(self.cells) * 0x200
            self.cells[point] = address
            self.write32(address, CELL_VTABLE)
            self.uc.mem_write(address + 0x24, struct.pack('<hh', *point))
            if 0 <= point[0] < 512 and 0 <= point[1] < 512:
                self.write32(RETREAT_MAP_TABLE + (point[0] + point[1] * 512) * 4, address)
        return self.cells[point]

    def cell_coord(self, address):
        return list(struct.unpack('<hh', self.uc.mem_read(address + 0x24, 4)))

    def can_enter_stub(self, e):
        point = e.cell_coord(e.arg(0))
        row = dict(cell=point, direction=i32(e.arg(1)), height=i32(e.arg(2)),
                   previous=e.arg(3), mode=e.arg(4), member=e.index_of(
                       e.uc.reg_read(UC_X86_REG_ECX)))
        self.visits.append(row)
        if self.accept is None:
            return int(tuple(point) in self.blocked)
        return int(len(self.visits) - 1 != self.accept)

    def install_retreat(self, buildings):
        self.accept = None
        def current(e):
            out = e.arg(0)
            e.write32(out, 58)
            e.write32(out + 4, 0)
            return out
        self.hook(CURRENT_ACTION, current, 4)
        self.hook(0x6EBAD0, lambda e: e.events.append(
            ['coordinate_move', e.cell_coord(e.read32(TEAM + 0x40))]), 0)
        self.write8(TEAM + 0x7F, 1)
        self.write32(TEAM + 0x2C, HOUSES)
        self.write32(TEAM + 0x48, 2)
        # Native-declared inputs, independent of VERA coordinates/subcells.
        for i in range(2):
            actor = self.object(i, xyz=(5248 + i * 256, 5248, 0))
            self.write8(actor + 0x90, 1)
            self.write32(actor + 0x6C, 100)
            self.write8(actor + 0x689, 1)
            self.write8(actor + 0x3D5, 1)
            techno_type = self.techno_type_at(i)
            self.techno_type[actor] = techno_type
            self.write32(techno_type + 0x5FC, 5)
            self.weight[actor] = 0
            if i == 0:
                self.write32(TEAM + 0x54, actor)
            else:
                self.write32(previous + 0x5D8, actor)
            previous = actor
        self.write32(HOUSES + 0x6C, ARRAYS)
        self.write32(HOUSES + 0x78, len(buildings))
        self.write32(BUILDING_VTABLE + 0x3F8, STUB_RETREAT_WEAPON)
        self.write32(ARRAYS + 0x100, 0x1234)
        self.hook(STUB_RETREAT_WEAPON, lambda e: ARRAYS + (0x100 if
            e.building_inputs[e.uc.reg_read(UC_X86_REG_ECX)].get('armed') else 0x104), 4)
        def coords(e):
            out = e.arg(0)
            row = e.building_inputs[e.uc.reg_read(UC_X86_REG_ECX)]
            e.uc.mem_write(out, struct.pack('<iii', *row['xyz']))
            return out
        self.hook(STUB_BUILDING_COORDS, coords, 4)
        threats = {}
        for i, row in enumerate(buildings):
            if row is None:
                continue
            actor = self.object(i + 16, xyz=row['xyz'], vtable=BUILDING_VTABLE,
                                rtti=RTTI_BUILDING)
            self.building_inputs[actor] = row
            techno_type = BUILDING_TYPES + i * TYPE_SIZE
            self.write32(actor + 0x520, techno_type)
            self.write8(actor + 0x81, row.get('limbo', False))
            self.write8(techno_type + 0x16A9, row.get('repair', False))
            self.write32(ARRAYS + i * 4, actor)
            point = tuple(trunc_div256(v) for v in row['xyz'][:2])
            self.cell(point)
            index = trunc_div4(point[0]) + 130 * trunc_div4(point[1])
            threat = row.get('threat', 0)
            if index in threats and threats[index] != threat:
                raise ValueError('Conflicting inputs for one retained House threat slot')
            threats[index] = threat
            self.write32(HOUSES + 0x59F0 + index * 4, threat)


def trunc_div256(value):
    return (abs(value) // 256) * (-1 if value < 0 else 1)


def trunc_div4(value):
    return (abs(value) // 4) * (-1 if value < 0 else 1)


def retreat_live_nearby(kind, slope):
    """Whole Foot search through the original class admission and height prelude.

    Reuses the existing map/locomotor fixture; no entry or gameplay return
    is supplied. Empty cells carry a declared slope (0 flat / 1 ramp).
    """
    from tools.native_oracle import SCRATCH
    from tools.spatial_oracle.map_queries import dwords, packed
    from tools.spatial_oracle.unit_source_scatter import make_source_fixture, CELLS
    from tools.spatial_oracle.unit_scatter_state import ACTOR, TYPE, LOCO, SP
    u, call, read = make_source_fixture(dict(live_entry=True))
    infantry = kind == 'infantry'
    u.mem_write(ACTOR, dwords(0x7EB058 if infantry else 0x7F5C70))
    u.mem_write(TYPE, dwords(0x7EB610 if infantry else 0x7F6218))
    u.mem_write(ACTOR + (0x6C0 if infantry else 0x6C4), dwords(TYPE))
    if infantry:
        u.mem_write(ACTOR + 0x6C4, dwords(-1))
        call(0x75AA90, LOCO, [])
        u.mem_write(LOCO + 0xC, dwords(ACTOR))
        u.mem_write(ACTOR + 0x674, dwords(LOCO + 4))
    u.mem_write(ACTOR + 0x14, dwords(5))
    u.mem_write(ACTOR + 0x3D5, b'\1')
    for y in range(32):
        for x in range(32):
            cell = CELLS + (y * 32 + x) * 0x200
            u.mem_write(cell + 0x116, b'\xff\xff')
            u.mem_write(cell + 0x11C, bytes([slope]))
    out, source, target = SCRATCH + 0xE000, SCRATCH + 0xE010, SCRATCH + 0xE020
    u.mem_write(source, packed(20, 20))
    u.mem_write(target, packed(10, 10))
    visits = []
    entry = 0x51BF90 if infantry else 0x73F0A0
    def observe(_u, pc, _size, _data):
        if pc in (0x65C780, 0x65C7E0):
            raise AssertionError('Unexpected RNG in original nearby/class admission')
        if pc == entry:
            sp = u.reg_read(UC_X86_REG_ESP)
            cell = read(sp + 4)
            visits.append(dict(cell=list(struct.unpack('<hh', u.mem_read(cell + 0x24, 4))),
                               direction=i32(read(sp + 8))))
    hook = u.hook_add(UC_HOOK_CODE, observe)
    try:
        call(FOOT_NEARBY, ACTOR, [out, source, target, 2, 4])
    finally:
        u.hook_del(hook)
    assert u.reg_read(UC_X86_REG_ESP) == SP + 24
    return dict(kind=kind, slope=slope, source=[20, 20], target=[10, 10],
                result=list(struct.unpack('<hh', u.mem_read(out, 4))), visits=visits)


def retreat():
    nearby = []
    pairs = [((0, 0), point) for point in
             [(10, 0), (10, 10), (0, 10), (-10, 10), (-10, 0),
              (-10, -10), (0, -10), (10, -10)]]
    pairs += [((4, 4), (4, 4)), ((-32768, 32767), (32767, -32768))]
    controls = [(source, target, 2, 4, accept) for source, target in pairs
                for accept in (-1, 0, 3, 9)]
    # Exclusive/empty range and accepted null cell: success at (0,0) must
    # stop the Foot search even though the caller rejects that sentinel.
    controls += [((0, 0), (10, 0), 3, 3, -1),
                 ((0, 0), (10, 0), 0, 2, -1),
                 ((-10, -1), (2, -1), 2, 4, 0)]
    for source, target, minimum, maximum, accept in controls:
        e = RetreatEmu(accept=accept)
        actor = e.object(0)
        out, first, last = ARRAYS, ARRAYS + 16, ARRAYS + 32
        e.uc.mem_write(first, struct.pack('<hh', *source))
        e.uc.mem_write(last, struct.pack('<hh', *target))
        e.invoke(FOOT_NEARBY, ecx=actor, args=(out, first, last, minimum, maximum))
        nearby.append(dict(source=source, target=target, minimum=minimum,
                           maximum=maximum, accept=accept,
                           result=list(struct.unpack('<hh', e.uc.mem_read(out, 4))),
                           visits=e.visits))
    def building(x, y, **kwargs):
        return dict(xyz=[x * 256, y * 256, 0], **kwargs)
    cases = [
        ('no-buildings', []),
        ('unarmed', [building(10, 10)]),
        ('armed-limbo-null', [None, building(18, 18, armed=True),
                             building(16, 16, limbo=True), building(5, 5)]),
        ('threat', [building(15, 15, threat=5), building(5, 5)]),
        ('repair', [building(15, 15), building(10, 10, repair=True)]),
        ('tie-first', [building(15, 10), building(15, 31)]),
        ('tie-reversed', [building(15, 31), building(15, 10)]),
        ('height', [dict(xyz=[15 * 256, 15 * 256, 10000]), building(10, 10)]),
        ('wrapped-score', [building(15, 15, threat=2147483647), building(5, 5)]),
    ]
    selections = []
    blocked = [(x, y) for x in range(12, 19) for y in range(12, 19)]
    cases += [('blocked-first', [building(15, 15), building(5, 5)]),
              ('all-blocked', [building(15, 15)])]
    for name, buildings in cases:
        refusals = blocked if name in ('blocked-first', 'all-blocked') else []
        e = RetreatEmu(blocked=refusals)
        e.install_retreat(buildings)
        e.invoke(RETREAT, ecx=TEAM)
        selections.append(dict(name=name, buildings=buildings, blocked=refusals,
                               focus=e.cell_coord(e.read32(TEAM + 0x40)),
                               formed=e.read8(TEAM + 0x7F),
                               visits=e.visits, events=e.events))
    return dict(nearby=nearby, selections=selections,
                live_nearby=[retreat_live_nearby(kind, slope)
                             for kind in ('infantry', 'unit') for slope in (0, 1)])


# ---------------------------------------------------------------- recalc

RECALC = 0x6EA3E0
TEAM_BYTES = (0x76, 0x78, 0x79, 0x7A, 0x7B, 0x7D, 0x7E)


def recalc_row(*, entries, total, reinforce, guard_slower, has_been, under):
    emu = TeamEmu()
    emu.write32(TASK_FORCE + 0x9C, len(entries))
    for slot, amount in enumerate(entries):
        emu.write32(TASK_FORCE + 0xA4 + slot * 8, amount)
    emu.write8(TEAM_TYPE + 0xAB, int(reinforce))
    emu.write8(TEAM_TYPE + 0xA7, int(guard_slower))
    emu.write32(TEAM + 0x48, total)
    emu.write32(TEAM + 0x34, CELL)
    for offset, value in ((0x76, 0), (0x78, has_been), (0x79, 0), (0x7A, under), (0x7B, 0),
                          (0x7D, 1), (0x7E, 1), (0x82, 0)):
        emu.write8(TEAM + offset, int(value))
    result = emu.invoke(RECALC, ecx=TEAM) & 0xFF
    return dict(entries=entries, total=total, reinforce=reinforce, guard_slower=guard_slower,
                has_been=has_been, under=under, result=result,
                bytes={f'{offset:#x}': emu.read8(TEAM + offset) for offset in TEAM_BYTES},
                zone_cleared=emu.read32(TEAM + 0x34) == 0,
                deleted=any(event[0] == 'delete' for event in emu.events))


def recalc():
    rows = []
    for entries in ([], [0], [1], [2], [3], [2, 1], [1, 1, 1], [4, 2], [7], [9], [5, 4, 3],
                    [-1, 3], [-9]):
        desired = sum(entries)
        totals = sorted({0, 1, 2, 3, desired - 1, desired, desired + 1} & set(range(0, 13)))
        for total in totals:
            for reinforce in (False, True):
                for has_been in (False, True):
                    for under in (False, True):
                        rows.append(recalc_row(entries=entries, total=total,
                                               reinforce=reinforce,
                                               guard_slower=(total + len(entries)) % 2 == 1,
                                               has_been=has_been, under=under))
    return rows


# ---------------------------------------------------------------- center

CALC_CENTER = 0x6EAEE0
CURRENT_ACTION = 0x691500
GET_CELL_AT = 0x565730


def center_row(*, members, focus=None, guard_slower=False, action=0):
    """`members`: dicts of location, flags, the weight and cell answers."""
    emu = TeamEmu()
    emu.write8(TEAM_TYPE + 0xA7, int(guard_slower))

    def current(e):
        out = e.arg(0)
        e.write32(out, action)
        e.write32(out + 4, 0)
        return out
    emu.hook(CURRENT_ACTION, current, 4)

    def cell_at(e):
        coord = e.arg(0)
        e.events.append(['cell_at', [i32(e.read32(coord + offset)) for offset in (0, 4, 8)]])
        return CELL
    emu.hook(GET_CELL_AT, cell_at, 4)
    previous = None
    for index, member in enumerate(members):
        address = emu.object(index, xyz=member['xyz'],
                             rtti=RTTI_AIRCRAFT if member['aircraft'] else RTTI_INFANTRY)
        emu.write8(address + 0x90, int(member['active']))
        emu.write32(address + 0x6C, member['health'])
        emu.write8(address + 0x81, int(member['limbo']))
        emu.write8(address + 0x689, int(member['initiated']))
        emu.write8(address + 0x3D5, int(member['in_playfield']))
        emu.techno_type[address] = emu.techno_type_at(index, passengers=member['passengers'],
                                                      naval=member['naval'])
        emu.weight[address] = int(member['weight'])
        emu.can_enter[address] = member['can_enter']
        if previous is None:
            emu.write32(TEAM + 0x54, address)
        else:
            emu.write32(previous + 0x5D8, address)
        previous = address
    if focus is not None:
        emu.write32(TEAM + 0x40, emu.object(len(members), xyz=focus))
    outputs = FAKE + 0x360000
    emu.write32(outputs, 0x1234)
    emu.write32(outputs + 4, 0x1234)
    emu.invoke(CALC_CENTER, ecx=TEAM, args=(outputs, outputs + 4))
    center, closest = emu.read32(outputs), emu.read32(outputs + 4)

    def reference(address):
        if address == 0:
            return None
        if address == CELL:
            return 'cell'
        return TeamEmu.index_of(address)
    return dict(members=members, focus=focus, guard_slower=guard_slower,
                center=reference(center), closest=reference(closest), events=emu.events)



def center_member(rng, *, far=False):
    span = 0x7FFF00 if far else 0x6000
    return dict(xyz=[rng.randrange(0x100, span), rng.randrange(0x100, span),
                     rng.randrange(0, 0x400)],
                active=rng.random() > 0.1, health=rng.choice((0, 1, 100, 100, 100)),
                limbo=rng.random() < 0.1, initiated=rng.random() > 0.3,
                aircraft=rng.random() < 0.15, in_playfield=rng.random() > 0.1,
                passengers=rng.choice((0, 0, 0, 5)), naval=rng.random() < 0.3,
                weight=rng.random() < 0.5, can_enter=rng.choice((0, 0, 0, 1, 2)))


def center():
    rng = random.Random(0x6EAEE0)
    rows = []
    for case in range(120):
        far = case % 4 == 0
        members = [center_member(rng, far=far) for _ in range(rng.randrange(1, 7))]
        focus = None if case % 3 == 0 else [rng.randrange(0, 0x7FFF00 if far else 0x6000),
                                            rng.randrange(0, 0x7FFF00 if far else 0x6000),
                                            0]
        rows.append(center_row(members=members, focus=focus, guard_slower=case % 2 == 1))
    # Equal distances keep the first; a zero distance is replaced by the next.
    same = dict(active=True, health=100, limbo=False, initiated=True, aircraft=False,
                in_playfield=True, passengers=0, naval=False, weight=False, can_enter=0)
    rows.append(center_row(members=[dict(same, xyz=[1000, 1000, 0]),
                                    dict(same, xyz=[3000, 1000, 0]),
                                    dict(same, xyz=[1000, 3000, 0])],
                           focus=[2000, 2000, 0]))
    rows.append(center_row(members=[dict(same, xyz=[2000, 2000, 0]),
                                    dict(same, xyz=[2100, 2000, 0])],
                           focus=[2000, 2000, 0]))
    rows.append(center_row(members=[dict(same, xyz=[1000, 1000, 0], passengers=5, naval=True),
                                    dict(same, xyz=[1200, 1000, 0], passengers=5, naval=True)],
                           focus=[2000, 2000, 0]))
    rows.append(center_row(members=[dict(same, xyz=[1000, 1000, 0], can_enter=1),
                                    dict(same, xyz=[1500, 1000, 0], can_enter=1)]))
    return rows


# ---------------------------------------------------------------- recruit

RECRUIT = 0x6EAA90
CAN_ADD = 0x6EA610
ADD_MEMBER = 0x6EA500
GET_GROUP = 0x6F1870
TEAM_WAYPOINT = 0x6F18A0
FIRST_PASSENGER = 0x473450


def recruit_row(*, kind, candidates, zone=None, group=-1, recruiter=False, amount=2, count=0,
                slot=0):
    """`candidates`: dicts of location, group, house/type match and the
    Can_Add answer, in array order."""
    emu = TeamEmu()
    house, other_house = HOUSES, HOUSES + HOUSE_SIZE
    emu.write32(TEAM + 0x2C, house)
    entry_type = emu.techno_type_at(0, rtti=TYPE_RTTI[kind])
    other_type = emu.techno_type_at(1, rtti=TYPE_RTTI[kind])
    emu.write32(TASK_FORCE + 0x9C, slot + 1)
    emu.write32(TASK_FORCE + 0xA4 + slot * 8, amount)
    emu.write32(TASK_FORCE + 0xA8 + slot * 8, entry_type)
    emu.write32(TEAM + 0x88 + slot * 4, count)
    emu.write8(TEAM_TYPE + 0xA8, int(recruiter))
    emu.hook(GET_GROUP, lambda _e: group, 0)

    def waypoint(e):
        out = e.arg(0)
        e.write32(out, 0)
        return out
    emu.hook(TEAM_WAYPOINT, waypoint, 4)
    answers = {}

    def can_add(e):
        candidate = e.arg(0)
        e.events.append(['can_add', TeamEmu.index_of(candidate), e.arg(2)])
        return int(answers[candidate])
    emu.hook(CAN_ADD, can_add, 12)
    emu.hook(ADD_MEMBER, lambda e: (e.events.append(['add', TeamEmu.index_of(e.arg(0)),
                                                     e.arg(1)]), 1)[1], 8)
    emu.hook(FIRST_PASSENGER, lambda _e: 0, 0)
    array = ARRAYS
    for index, candidate in enumerate(candidates):
        address = emu.object(index, xyz=candidate['xyz'])
        emu.write32(address + 0x214, candidate['group'])
        emu.write32(address + 0x21C, house if candidate['own_house'] else other_house)
        emu.write32(address + 0x6C4, entry_type if candidate['entry_type'] else other_type)
        answers[address] = candidate['can_add']
        emu.write32(array + index * 4, address)
    pointer, length = CLASS_ARRAY[kind]
    emu.write32(pointer, array)
    emu.write32(length, len(candidates))
    if zone is not None:
        emu.write32(TEAM + 0x34, emu.object(len(candidates), xyz=zone))
    result = emu.invoke(RECRUIT, ecx=TEAM, args=(slot,))
    return dict(kind=kind, candidates=candidates, zone=zone, group=group, recruiter=recruiter,
                amount=amount, count=count, slot=slot, result=result, events=emu.events)


def recruit_candidate(rng, *, far=False, groups=(-1,)):
    span = 0x7FFF00 if far else 0x6000
    return dict(xyz=[rng.randrange(0, span), rng.randrange(0, span), 0],
                group=rng.choice(groups), own_house=rng.random() > 0.2,
                entry_type=rng.random() > 0.2, can_add=rng.random() > 0.3)


def recruit():
    rng = random.Random(0x6EAA90)
    rows = []
    for case in range(150):
        kind = ('infantry', 'unit', 'aircraft')[case % 3]
        far = case % 5 == 0
        groups = (-1,) if case % 4 else (-1, 3, 7)
        group = rng.choice((-1, -1, 3, -2)) if case % 4 == 0 else -1
        candidates = [recruit_candidate(rng, far=far, groups=groups)
                      for _ in range(rng.randrange(0, 8))]
        zone = None if case % 6 == 0 else [rng.randrange(0, 0x7FFF00 if far else 0x6000),
                                           rng.randrange(0, 0x7FFF00 if far else 0x6000), 0]
        rows.append(recruit_row(kind=kind, candidates=candidates, zone=zone, group=group,
                                recruiter=case % 7 == 3))
    # Equal keys keep the first; a full entry recruits nobody.
    same = dict(group=-1, own_house=True, entry_type=True, can_add=True)
    rows.append(recruit_row(kind='infantry', zone=[0x1000, 0x1000, 0],
                            candidates=[dict(same, xyz=[0x1100, 0x1000, 0]),
                                        dict(same, xyz=[0x1000, 0x1100, 0])]))
    rows.append(recruit_row(kind='unit', zone=[0x1000, 0x1000, 0], amount=2, count=2,
                            candidates=[dict(same, xyz=[0x1100, 0x1000, 0])]))
    rows.append(recruit_row(kind='unit', zone=[0x1000, 0x1000, 0], amount=-1, count=-3,
                            candidates=[dict(same, xyz=[0x1100, 0x1000, 0])]))
    return rows


# ---------------------------------------------------------------- distance

DISTANCE = 0x5F6360
FOUNDATIONS = 22


def distance_row(*, member, target, building=None):
    """`building`: the target's foundation index, else a Foot target."""
    emu = TeamEmu()
    self_address = emu.object(0, xyz=member)
    if building is None:
        target_address = emu.object(1, xyz=target, rtti=RTTI_UNIT)
    else:
        target_address = emu.object(1, vtable=BUILDING_VTABLE, rtti=RTTI_BUILDING)
        building_type = BUILDING_TYPES
        emu.write32(target_address + 0x520, building_type)
        emu.write32(building_type + 0xEF0, building)

        def coords(e):
            out = e.arg(0)
            for offset, value in zip((0, 4, 8), target):
                e.write32(out + offset, value)
            return out
        emu.hook(STUB_BUILDING_COORDS, coords, 4)
    result = i32(emu.invoke(DISTANCE, ecx=self_address, args=(target_address,)))
    return dict(member=member, target=target, building=building, result=result)


def distance():
    rng = random.Random(0x5F6360)
    rows = []
    for case in range(80):
        span = (0x400, 0x4000, 0x40000, 0x7FFF00)[case % 4]
        member = [rng.randrange(0, span), rng.randrange(0, span), rng.randrange(0, 0x800)]
        target = [rng.randrange(0, span), rng.randrange(0, span), rng.randrange(0, 0x800)]
        rows.append(distance_row(member=member, target=target))
    for foundation in range(FOUNDATIONS):
        for offset in (0, 64, 200, 700):
            rows.append(distance_row(member=[0x2000, 0x2000, 0],
                                     target=[0x2000 + offset, 0x2000, 0], building=foundation))
    return rows


# ---------------------------------------------------------------- guard

GUARD_CASE = 0x6E97CE
GUARD_END = 0x6E97EC


def guard_row(argument, frame=1234):
    emu = TeamEmu()
    emu.write32(FRAME, frame)
    uc = emu.uc
    sp = STACK_BASE + STACK_SIZE - 0x1000
    emu.write32(sp + 0x24, 0x5A5A5A5A)
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_ESI, TEAM)
    uc.reg_write(UC_X86_REG_ECX, argument & 0xFFFFFFFF)
    uc.reg_write(UC_X86_REG_EBX, 1)
    run_checked(uc, GUARD_CASE, GUARD_END)
    return dict(argument=argument, frame=frame, start=i32(emu.read32(TEAM + 0x58)),
                duration=i32(emu.read32(TEAM + 0x60)))


def guard():
    return [guard_row(argument) for argument in
            (0, 1, 2, 4, 20, 60, -1, -3, 0x08888888, 0x08888889, 0x7FFFFFFF, -0x80000000)]


# ---------------------------------------------------------------- regroup

REGROUP_AT_BASE = 0x6EFA10
REGROUP_FNPC = 0x6EFC3A
BASE_CENTER = 0x50DF30


def regroup_row(*, own, enemy=None, safe_distance, draw=None):
    emu = TeamEmu()
    house, enemy_house = HOUSES, HOUSES + HOUSE_SIZE
    leader = emu.object(0, xyz=[0x3000, 0x3000, 0])
    emu.write32(TEAM + 0x54, leader)
    emu.write8(leader + 0x90, 1)
    emu.write32(leader + 0x6C, 100)
    emu.write8(leader + 0x689, 1)
    emu.write32(leader + 0x21C, house)
    emu.techno_type[leader] = emu.techno_type_at(0)
    emu.write32(RULES + 0xD74, safe_distance)
    emu.write32(HOUSE_ARRAY, ARRAYS)
    emu.write32(ARRAYS + 4, enemy_house)
    emu.write32(house + 0x5600, 1 if enemy is not None else -1)
    centers = {house: own, enemy_house: enemy}

    def base_center(e):
        out = e.arg(0)
        for offset, value in zip((0, 4, 8), centers[e.uc.reg_read(UC_X86_REG_ECX)] + [0]):
            e.write32(out + offset, value)
        return out
    emu.hook(BASE_CENTER, base_center, 4)
    emu.draws([] if draw is None else [draw])
    emu.start(REGROUP_AT_BASE, REGROUP_FNPC, ecx=TEAM, args=(0, 1))
    # At the call, the seed cell is FNPC's second argument.
    seed_pointer = emu.read32(emu.uc.reg_read(UC_X86_REG_ESP) + 4)
    seed = struct.unpack('<hh', emu.uc.mem_read(seed_pointer, 4))
    return dict(own=own, enemy=enemy, safe_distance=safe_distance, draw=draw, seed=list(seed),
                draws=[event[1:] for event in emu.events if event[0] == 'draw'])


def regroup():
    rng = random.Random(0x6EFA10)
    rows = []
    for draw in (0, 1, 31, 32, 63, 64, 96, 127, 128, 160, 191, 192, 224, 255):
        for safe_distance in (10, 50):
            rows.append(regroup_row(own=[0x4080, 0x4080], safe_distance=safe_distance,
                                    draw=draw))
    for _ in range(60):
        own = [rng.randrange(0x80, 0x30000), rng.randrange(0x80, 0x30000)]
        enemy = [rng.randrange(0x80, 0x30000), rng.randrange(0x80, 0x30000)]
        rows.append(regroup_row(own=own, enemy=enemy,
                                safe_distance=rng.choice((0, 1, 10, 30, 50, 100))))
    for enemy in ([0x4080, 0x4080], [0x5080, 0x4080], [0x4080, 0x5080], [0x3080, 0x4080],
                  [0x4080, 0x3080], [0x5080, 0x5080], [0x3080, 0x3080]):
        rows.append(regroup_row(own=[0x4080, 0x4080], enemy=enemy, safe_distance=50))
    rows.append(regroup_row(own=[0, 0], safe_distance=50, draw=128))
    rows.append(regroup_row(own=[0x80, 0x80], enemy=[0x30080, 0x80], safe_distance=-5))
    return rows


# ---------------------------------------------------------------- gather

GATHER_AT_ENEMY_BASE = 0x6EF700
GATHER_FNPC = 0x6EF98A


def gather_row(*, own, enemy, safe_distance, leader=(0x3000, 0x3000, 0), has_enemy=True):
    """`own`/`enemy`: a base centre's XY, [0, 0] for none."""
    emu = TeamEmu()
    house, enemy_house = HOUSES, HOUSES + HOUSE_SIZE
    leader_address = emu.object(0, xyz=list(leader))
    emu.write32(TEAM + 0x54, leader_address)
    emu.write8(leader_address + 0x90, 1)
    emu.write32(leader_address + 0x6C, 100)
    emu.write8(leader_address + 0x689, 1)
    emu.write32(leader_address + 0x21C, house)
    emu.techno_type[leader_address] = emu.techno_type_at(0)
    emu.write32(RULES + 0xD74, safe_distance)
    emu.write32(HOUSE_ARRAY, ARRAYS)
    emu.write32(ARRAYS + 4, enemy_house)
    emu.write32(house + 0x5600, 1 if has_enemy else -1)
    centers = {house: own, enemy_house: enemy}

    def base_center(e):
        out = e.arg(0)
        for offset, value in zip((0, 4, 8), centers[e.uc.reg_read(UC_X86_REG_ECX)] + [0]):
            e.write32(out + offset, value)
        return out
    emu.hook(BASE_CENTER, base_center, 4)
    stop = emu.start(GATHER_AT_ENEMY_BASE, (GATHER_FNPC, RET_MAGIC), ecx=TEAM, args=(0, 1))
    seed = None
    if stop == GATHER_FNPC:
        seed_pointer = emu.read32(emu.uc.reg_read(UC_X86_REG_ESP) + 4)
        seed = list(struct.unpack('<hh', emu.uc.mem_read(seed_pointer, 4)))
    return dict(own=own, enemy=enemy, leader=list(leader), has_enemy=has_enemy,
                safe_distance=safe_distance, seed=seed, finished=bool(emu.read8(TEAM + 0x80)))


def gather():
    rng = random.Random(0x6EF700)
    rows = []
    for _ in range(60):
        own = [rng.randrange(0x80, 0x30000), rng.randrange(0x80, 0x30000)]
        enemy = [rng.randrange(0x80, 0x30000), rng.randrange(0x80, 0x30000)]
        rows.append(gather_row(own=own, enemy=enemy,
                               safe_distance=rng.choice((0, 1, 10, 30, 50, 100))))
    for own in ([0x4080, 0x4080], [0x5080, 0x4080], [0x4080, 0x5080], [0x3080, 0x4080],
                [0x4080, 0x3080], [0x5080, 0x5080], [0x3080, 0x3080]):
        rows.append(gather_row(own=own, enemy=[0x4080, 0x4080], safe_distance=50))
    for leader in ((0x2345, 0x6789, 0x100), (0x4080, 0x4080, 0)):
        rows.append(gather_row(own=[0, 0], enemy=[0x4080, 0x4080], leader=leader,
                               safe_distance=30))
    rows.append(gather_row(own=[0x80, 0x80], enemy=[0x30080, 0x80], safe_distance=-5))
    rows.append(gather_row(own=[0x4080, 0x4080], enemy=[0, 0], safe_distance=50))
    rows.append(gather_row(own=[0x4080, 0x4080], enemy=[0x8080, 0x8080], safe_distance=50,
                           has_enemy=False))
    return rows


# ---------------------------------------------------------------- quarry

QUARRY_TO_THREAT = 0x645BB0


def quarry():
    return [dict(quarry=argument,
                 mask=TeamEmu().invoke(QUARRY_TO_THREAT, ecx=argument & 0xFFFFFFFF))
            for argument in list(range(-2, 14)) + [0x7FFFFFFF, -0x80000000]]


# ---------------------------------------------------------------- attack_cadence

CADENCE = 0x6EB59A
CADENCE_ASKS, CADENCE_SKIPS = 0x6EB5B0, 0x6EB5D9


def attack_cadence():
    rows = []
    for frame in (list(range(0, 20)) + list(range(-12, 0))
                  + [0x7FFFFFFC, 0x7FFFFFFF, -0x80000000, -0x7FFFFFFC]):
        emu = TeamEmu()
        emu.write32(FRAME, frame)
        emu.uc.reg_write(UC_X86_REG_ESP, STACK_BASE + STACK_SIZE - 0x1000)
        stop = run_checked(emu.uc, CADENCE, (CADENCE_ASKS, CADENCE_SKIPS))
        rows.append(dict(frame=frame, asks=stop == CADENCE_ASKS))
    return rows


# ---------------------------------------------------------------- patrol

PATROL = 0x6ECCE0
PATROL_READER = 0x67399E
PATROL_READER_END = 0x6739C5
PATROL_FIELD = 0x1150
PATROL_KEY = 0x83D344
PATROL_CONVERT = 0x6ECD40
PATROL_CONVERT_END = 0x6ECD57
PATROL_IDIV = 0x6ECD5F
STUB_PATROL_THREAT = STUBS + 0x300


def patrol_rule_rows():
    """Reuse the existing native INI/CRT fixture; no second ReadDouble port.
    Original constructor/read/multiply/ftol instructions supply every output.
    Malformed authored floats share the reader's documented non-retail policy
    and are deliberately outside these matching controls."""
    from tools.spatial_oracle.path_delay_rules import DelayFixture, INI, RULES as R, SP, AI
    f = DelayFixture()
    f.u.reg_write(UC_X86_REG_ESI, R)
    f.u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(f.u, 0x666E09, 0x666E24)
    default = f'{struct.unpack("<Q", f.u.mem_read(R + PATROL_FIELD, 8))[0]:016x}'
    rows = []
    for initial in (default, '3fb999999999999a'):
        for raw in (None, '0.016', '0.01', '1%', '-1%', '0', '-0', '0.25junk', '1e-3'):
            f.ini(PATROL_KEY, raw)
            f.write(INI + 4, AI)
            f.u.mem_write(R + PATROL_FIELD, struct.pack('<Q', int(initial, 16)))
            f.u.reg_write(UC_X86_REG_ESI, R)
            f.u.reg_write(UC_X86_REG_EDI, INI)
            f.u.reg_write(UC_X86_REG_ESP, SP)
            run_checked(f.u, PATROL_READER, PATROL_READER_END)
            output = f'{struct.unpack("<Q", f.u.mem_read(R + PATROL_FIELD, 8))[0]:016x}'
            f.write(0x8871E0, R)
            run_checked(f.u, PATROL_CONVERT, PATROL_CONVERT_END)
            ticks = i32(f.u.reg_read(UC_X86_REG_EAX))
            rows.append(dict(raw=raw, initial_bits=initial, output_bits=output, ticks=ticks))
    return default, rows


def patrol_row(*, minutes_bits, frame, members, first=False, focus='cell',
               waypoint=11, threat=False, placement_scope=False):
    """Original Script16 control/leader selection and arithmetic. Waypoint and
    scan callbacks supply retained identities; assignment and coordinators are
    bounded shared-owner interfaces, not native downstream execution."""
    emu = TeamEmu()
    action = SCRIPT + 0x200
    old, found = OBJECTS + 8 * OBJECT_SIZE, OBJECTS + 9 * OBJECT_SIZE
    refs = {0: None, CELL: 'cell', old: 'old', found: 'threat'}
    pointers = {value: address for address, value in refs.items()}
    emu.write32(action, 16)
    emu.write32(action + 4, waypoint)
    emu.write32(FRAME, frame)
    emu.write8(SCENARIO_INIT, int(placement_scope))
    emu.uc.mem_write(RULES + PATROL_FIELD, struct.pack('<Q', int(minutes_bits, 16)))
    emu.write32(TEAM + 0x3C, pointers[focus])
    emu.write32(TEAM + 0x40, pointers[focus])
    for address in (old, found):
        emu.write8(address + 0x14, 2)
    previous = None
    for index, member in enumerate(members):
        address = emu.object(index, xyz=member['xyz'],
                             rtti=RTTI_AIRCRAFT if member.get('aircraft') else RTTI_INFANTRY)
        emu.write8(address + 0x90, int(member['active']))
        emu.write32(address + 0x6C, member['health'])
        emu.write8(address + 0x81, int(member['limbo']))
        emu.write8(address + 0x689, int(member['initiated']))
        type_address = emu.techno_type_at(index)
        emu.techno_type[address] = type_address
        emu.write32(type_address + 0x5FC, member['rating'])
        if previous is None:
            emu.write32(TEAM + 0x54, address)
        else:
            emu.write32(previous + 0x5D8, address)
        previous = address
    emu.write32(OBJECT_VTABLE + 0x3C4, STUB_PATROL_THREAT)

    def current(e):
        out = e.arg(0)
        e.write32(out, 16)
        e.write32(out + 4, waypoint)
        return out

    def assign(e):
        target = e.arg(0)
        e.events.append(['assign', refs[target]])
        prior = e.read32(TEAM + 0x3C)
        if e.read32(TEAM + 0x40) in (0, prior):
            e.write32(TEAM + 0x40, target)
        e.write32(TEAM + 0x3C, target)

    def scan(e):
        coord = e.arg(1)
        e.events.append(['scan', e.index_of(e.uc.reg_read(UC_X86_REG_ECX)), e.arg(0),
                         [i32(e.read32(coord + offset)) for offset in (0, 4, 8)], e.arg(2)])
        return found if threat else 0

    emu.hook(0x68BCE0, lambda e: e.events.append(['waypoint', i32(e.arg(0))]) or CELL, 4)
    emu.hook(0x691500, current, 4)
    emu.hook(0x6E9050, assign, 4)
    emu.hook(STUB_PATROL_THREAT, scan, 12)
    emu.hook(0x6EB490, lambda e: e.events.append(['attack', refs[e.read32(TEAM + 0x40)]]), 0)
    emu.hook(0x6EBAD0, lambda e: e.events.append(['move', refs[e.read32(TEAM + 0x40)]]), 0)
    periods = []
    emu.uc.hook_add(UC_HOOK_CODE,
        lambda uc, *_: periods.append(i32(uc.reg_read(UC_X86_REG_ECX))),
        begin=PATROL_IDIV, end=PATROL_IDIV)
    emu.invoke(PATROL, ecx=TEAM, args=(action, int(first)))
    return dict(minutes_bits=minutes_bits, frame=frame, members=members, first=first,
                initial_focus=focus, waypoint=waypoint, threat=threat,
                placement_scope=placement_scope, period=periods[0],
                mission_target=refs[emu.read32(TEAM + 0x3C)],
                focus=refs[emu.read32(TEAM + 0x40)], events=emu.events)


def patrol():
    default, rules = patrol_rule_rows()
    member = dict(xyz=[0xA80, 0xB80, 0x100], active=True, health=100,
                  limbo=False, initiated=True, aircraft=False, rating=5)
    promoted = dict(member, xyz=[0xC80, 0xD80, 0x200], rating=10)
    decimal = next(row['output_bits'] for row in rules
                   if row['initial_bits'] == default and row['raw'] == '0.016')
    percent = next(row['output_bits'] for row in rules
                   if row['initial_bits'] == default and row['raw'] == '1%')
    controls = []
    for args in [
        dict(frame=1, first=True, focus=None),
        dict(frame=1, focus=None),
        dict(frame=1, focus=None, waypoint=702),
        dict(frame=14, threat=True),
        dict(frame=14, focus='old'),
        dict(frame=14),
        dict(frame=-14, threat=True),
        dict(frame=9, minutes_bits=percent, threat=True),
        dict(frame=14, members=[member, dict(promoted, initiated=False)]),
        dict(frame=14, members=[member, dict(promoted, aircraft=True, initiated=False)]),
        dict(frame=14, members=[dict(member, active=False), dict(promoted, health=0)]),
        dict(frame=14, members=[dict(member, limbo=True), promoted], placement_scope=True),
        dict(frame=14, members=[member, dict(promoted, rating=5)]),
        dict(frame=14, members=[]),
    ]:
        controls.append(patrol_row(**(dict(minutes_bits=decimal, members=[member, promoted]) | args)))
    return dict(default_bits=default, rules=rules, controls=controls)


# --------------------------------------------------------- force-facing

FORCE_FACING = 0x6EDCA0
STUB_SCRIPT_MOVING = STUBS + 0x310
TURN_ENTRIES = {'drive': 0x4B0EF0, 'ship': 0x6A05C0, 'walk': 0x75AE00,
                'fly': 0x4CFC10, 'hover': 0x516370, 'jumpjet': 0x54B6E0,
                'teleport': 0x7192C0, 'rocket': 0x55AC30}


def facing_projection(raw):
    """Serialized field projection, not facing arithmetic. Padding/high words
    ignored by the original DirStruct reader are outside the Rust authority."""
    dest, previous, start, _padding, duration, rate = struct.unpack('<IIIIII', raw)
    return dict(current=dest & 0xFFFF, prev=previous & 0xFFFF,
                start_frame=None if start == 0xFFFFFFFF else start,
                duration_frames=duration & 0xFFFF, rot_per_frame=rate & 0xFFFF)


def force_facing_row(*, argument, members, frame=100, first=False):
    emu = TeamEmu()
    action = SCRIPT + 0x200
    emu.write32(action, 42)
    emu.write32(action + 4, argument)
    emu.write32(FRAME, frame)
    addresses = []
    motions = {}
    previous = None
    for index, member in enumerate(members):
        address = emu.object(index)
        addresses.append(address)
        emu.write8(address + 0x90, int(member.get('active', True)))
        emu.write8(address + 0x81, int(member.get('limbo', False)))
        emu.write8(address + 0x689, int(member.get('initiated', True)))
        emu.write32(address + 0x6C, member.get('health', 100))
        loco, vtable = FAKE + 0x360000 + index * 0x100, FAKE + 0x361000 + index * 0x100
        emu.write32(address + 0x674, loco)
        emu.write32(loco, vtable)
        emu.write32(loco + 8, address)
        emu.write32(vtable + 0x10, STUB_SCRIPT_MOVING)
        emu.write32(vtable + 0x4C, TURN_ENTRIES[member['family']])
        motions[loco] = (index, member['moving'])
        for offset, key in ((0x388, 'body'), (0x3A0, 'secondary')):
            state = member[key]
            raw = struct.pack('<IIIIII', state['current'], state['prev'],
                0xFFFFFFFF if state['start_frame'] is None else state['start_frame'],
                0, state['duration_frames'], state['rot_per_frame'])
            emu.uc.mem_write(address + offset, raw)
        if previous is None:
            emu.write32(TEAM + 0x54, address)
        else:
            emu.write32(previous + 0x5D8, address)
        previous = address

    def moving(e):
        index, answer = motions[e.arg(0)]
        e.events.append(['moving', index, answer])
        return int(answer)
    emu.hook(STUB_SCRIPT_MOVING, moving, 4)
    for address in set(TURN_ENTRIES.values()):
        emu.uc.hook_add(UC_HOOK_CODE,
            lambda uc, pc, *_: emu.events.append(['turn',
                motions[emu.arg(0)][0], i32(emu.arg(1)) & 0xFFFF, f'{pc:08X}']),
            begin=address, end=address)
    emu.invoke(FORCE_FACING, ecx=TEAM, args=(action, int(first)))
    output = [dict(body=facing_projection(bytes(emu.uc.mem_read(address + 0x388, 24))),
                   secondary=facing_projection(bytes(emu.uc.mem_read(address + 0x3A0, 24))))
              for address in addresses]
    samples = []
    for elapsed in (0, 1, 6, 12, 25):
        emu.write32(FRAME, frame + elapsed)
        values = []
        for address in addresses:
            pair = []
            for offset in (0x388, 0x3A0):
                out = CELL + 0x100
                emu.invoke(0x4C93D0, ecx=address + offset, args=(out,))
                pair.append(struct.unpack('<H', emu.uc.mem_read(out, 2))[0])
            values.append(pair)
        samples.append(dict(frame=(frame + elapsed) & 0xFFFFFFFF, values=values))
    return dict(argument=argument, members=members, frame=frame, first=first,
                finished=bool(emu.read8(TEAM + 0x80)), output=output,
                samples=samples, events=emu.events)


def force_facing():
    at_rest = dict(current=0, prev=0, start_frame=None, duration_frames=0, rot_per_frame=1280)
    second = dict(at_rest, current=0x2000, prev=0x2000)
    member = dict(family='drive', moving=False, body=at_rest, secondary=second,
                  active=True, health=100, limbo=False, initiated=True)
    rows = [force_facing_row(argument=2, members=[dict(member, family=family)])
            for family in TURN_ENTRIES]
    rows.extend([
        force_facing_row(argument=2, members=[]),
        force_facing_row(argument=2, members=[dict(member, moving=True)]),
        force_facing_row(argument=2, members=[dict(member, family='walk', moving=True)]),
        force_facing_row(argument=2, members=[dict(member, family='ship', moving=True)]),
        force_facing_row(argument=2, members=[dict(member, moving=True), dict(member, family='walk')]),
        force_facing_row(argument=2, members=[dict(member, active=False, health=0, limbo=True, initiated=False)]),
        force_facing_row(argument=2, members=[dict(member, body=dict(at_rest, current=0x4000, prev=0x4000))]),
        force_facing_row(argument=2, members=[dict(member, body=dict(at_rest, current=0x8000, prev=0,
                         start_frame=90, duration_frames=25))]),
        force_facing_row(argument=10, members=[member], first=True),
        force_facing_row(argument=-1, members=[dict(member, family='walk')]),
        force_facing_row(argument=0x7FFFFFFF, members=[dict(member, family='teleport')]),
    ])
    return rows


# ---------------------------------------------------------------- own_building

FIND_OWN_BUILDING = 0x6EEEA0
THREAT_AT = 0x56BCD0


def own_building_row(*, leader, buildings, mode):
    """`buildings`: (location, of the searched type) in the house's order."""
    emu = TeamEmu()
    house = HOUSES
    searched, other = BUILDING_TYPES, BUILDING_TYPES + 0x2000
    leader_address = emu.object(0, xyz=leader)
    emu.write32(leader_address + 0x21C, house)
    array = ARRAYS
    for index, (location, of_type) in enumerate(buildings):
        address = emu.object(index + 1, xyz=location, vtable=OBJECT_VTABLE,
                             rtti=RTTI_BUILDING)
        emu.write32(address + 0x520, searched if of_type else other)
        emu.write32(array + index * 4, address)
    emu.write32(house + 0x6C, array)
    emu.write32(house + 0x78, len(buildings))
    emu.hook(THREAT_AT, lambda _e: 0, 8)
    result = emu.invoke(FIND_OWN_BUILDING, ecx=searched, args=(leader_address, mode, 0))
    return dict(leader=leader, buildings=buildings, mode=mode,
                result=None if result == 0 else TeamEmu.index_of(result) - 1)


def own_building():
    rng = random.Random(0x6EEEA0)
    rows = []
    for case in range(60):
        span = 0x7FFF00 if case % 5 == 0 else 0x8000
        leader = [rng.randrange(0, span), rng.randrange(0, span), rng.randrange(0, 0x400)]
        buildings = [([rng.randrange(0, span), rng.randrange(0, span), rng.randrange(0, 0x400)],
                      rng.random() > 0.3) for _ in range(rng.randrange(0, 7))]
        rows.append(own_building_row(leader=leader, buildings=buildings,
                                     mode=(0, 1, 2, 3, 2, 3, 4, 0x10000)[case % 8]))
    tied = [([0x2100, 0x2000, 0], True), ([0x2000, 0x2100, 0], True)]
    for mode in (2, 3):
        rows.append(own_building_row(leader=[0x2000, 0x2000, 0], buildings=tied, mode=mode))
    return rows


# ---------------------------------------------------------------- target_building

FIND_BEST_TARGET_BUILDING = 0x6EEBD0
BUILDING_ARRAY = (0xA8EB44, 0xA8EB50)
HOUSE_TYPES = BUILDING_TYPES + 0x8000


def target_building_row(*, unit, buildings, mode, only_enemy, enemy=1, allies=(),
                        passive=(3,)):
    """`buildings`: dicts of Location, GetCoords answer, type match, owner
    (house 0..3) and threat answer, in BuildingClass::Array order. House 0
    owns the unit; `enemy` is the house the original receives as the enemy
    (None for NULL); `allies` are the houses house 0's Allies bits name;
    `passive` the houses whose type is MultiplayPassive."""
    emu = TeamEmu()
    houses = [HOUSES + index * HOUSE_SIZE for index in range(4)]
    for index, house in enumerate(houses):
        house_type = HOUSE_TYPES + index * 0x400
        emu.write32(house + 0x30, index)
        emu.write32(house + 0x34, house_type)
        emu.write8(house_type + 0x1A6, int(index in passive))
    emu.write32(houses[0] + 0x5788, sum(1 << ally for ally in allies))
    unit_address = emu.object(0, xyz=unit)
    emu.write32(unit_address + 0x21C, houses[0])
    searched, other = BUILDING_TYPES, BUILDING_TYPES + 0x2000
    coords, threats = {}, {}
    for index, building in enumerate(buildings):
        address = emu.object(index + 1, xyz=building['location'], vtable=BUILDING_VTABLE,
                             rtti=RTTI_BUILDING)
        emu.write32(address + 0x520, searched if building['of_type'] else other)
        emu.write32(address + 0x21C, houses[building['owner']])
        emu.write32(ARRAYS + index * 4, address)
        coords[address] = building['coords']
        threats[address] = building['threat']
    emu.write32(BUILDING_ARRAY[0], ARRAYS)
    emu.write32(BUILDING_ARRAY[1], len(buildings))

    def building_coords(e):
        out = e.arg(0)
        for offset, value in zip((0, 4, 8), coords[e.uc.reg_read(UC_X86_REG_ECX)]):
            e.write32(out + offset, value)
        return out

    def threat_at(e):
        # The building being scored is in ESI throughout the walk.
        building = e.uc.reg_read(UC_X86_REG_ESI)
        cell = e.uc.mem_read(e.arg(0), 4)
        e.events.append(['threat', TeamEmu.index_of(building) - 1,
                         struct.unpack('<h', cell[:2])[0], struct.unpack('<h', cell[2:])[0],
                         houses.index(e.arg(1))])
        return threats[building]
    emu.hook(STUB_BUILDING_COORDS, building_coords, 4)
    emu.hook(THREAT_AT, threat_at, 8)
    result = emu.invoke(FIND_BEST_TARGET_BUILDING, ecx=searched,
                        edx=0 if enemy is None else houses[enemy],
                        args=(unit_address, mode, int(only_enemy)))
    return dict(unit=unit, buildings=buildings, mode=mode, only_enemy=only_enemy, enemy=enemy,
                allies=list(allies), passive=list(passive),
                result=None if result == 0 else TeamEmu.index_of(result) - 1,
                events=emu.events)


def target_building():
    rng = random.Random(0x6EEBD0)
    rows = []
    for case in range(160):
        span = (0x400, 0x8000, 0x40000, 0x7FFF00)[case % 4]

        def xyz():
            return [rng.randrange(-span, span), rng.randrange(-span, span),
                    rng.randrange(0, 0x800)]
        buildings = [dict(location=xyz(), coords=xyz(), of_type=rng.random() > 0.25,
                          owner=rng.randrange(4),
                          threat=rng.choice((0, 0, rng.randrange(1, 0x400),
                                             rng.randrange(0, 0x7FFFFFFF), -rng.randrange(1, 9))))
                     for _ in range(rng.randrange(0, 8))]
        rows.append(target_building_row(
            unit=xyz(), buildings=buildings, mode=(0, 1, 2, 3, 2, 0, 4, 0xFFFF)[case % 8],
            only_enemy=case % 3 == 1, enemy=None if case % 9 == 4 else 1,
            allies=[ally for ally in (1, 2, 3) if rng.random() > 0.6],
            passive=(3,) if case % 5 else (2, 3)))
    near = dict(location=[0x2100, 0x2000, 0], coords=[0x2180, 0x2080, 0], of_type=True,
                owner=2, threat=7)
    far = dict(location=[0x3000, 0x2000, 0], coords=[0x3080, 0x2080, 0], of_type=True,
               owner=1, threat=3)
    for mode in (0, 1, 2, 3):
        for only_enemy in (False, True):
            # The enemy's building is not the best overall: OnlyTargetHouseEnemy=
            # takes it, otherwise the other house's.
            rows.append(target_building_row(unit=[0x2000, 0x2000, 0], buildings=[near, far],
                                            mode=mode, only_enemy=only_enemy))
    # Ties keep the earlier building; an allied, passive owner's building is
    # a candidate, an allied, active one is not; an own building is allied.
    tied = [dict(near, owner=3), dict(near, location=[0x2000, 0x2100, 0], owner=1)]
    for mode in (0, 2):
        rows.append(target_building_row(unit=[0x2000, 0x2000, 0], buildings=tied, mode=mode,
                                        only_enemy=False, allies=(3,)))
    rows.append(target_building_row(unit=[0x2000, 0x2000, 0], mode=2, only_enemy=False,
                                    allies=(2,), buildings=[near, dict(near, owner=0),
                                                            dict(far, owner=3)]))
    # No enemy: OnlyTargetHouseEnemy= finds none.
    rows.append(target_building_row(unit=[0x2000, 0x2000, 0], buildings=[near, far], mode=2,
                                    only_enemy=True, enemy=None))
    # Coordinates whose /256 rounds toward zero, and a threat of -1 that no
    # building beats.
    rows.append(target_building_row(unit=[0, 0, 0], mode=1, only_enemy=False, buildings=[
        dict(near, coords=[-0x80, -0x101, 0], threat=-1),
        dict(near, coords=[-0x100, 0x7F, 0], threat=-1)]))
    return rows


# ---------------------------------------------------------------- bio_reactor

ENTER_BIO_REACTOR = 0x4DFB70
BIO_REACTOR_PICKED = 0x4DFC52
CAN_LOAD = 0xF


def bio_reactor_row(*, foot, absorbers, contact=None, pending=None, latch=False):
    """`absorbers`: dicts of location and CAN_LOAD answer, in House+0xB0
    order; `contact`/`pending`: an absorber's index, else none."""
    emu = TeamEmu()
    house = HOUSES
    foot_address = emu.object(0, xyz=foot)
    emu.write32(foot_address + 0x21C, house)
    emu.write8(foot_address + 0x68F, int(latch))
    for slot, stub in ((0x278, STUB_TRANSMIT), (0x1E8, STUB_QUEUE_MISSION),
                       (0x480, STUB_ASSIGN_DESTINATION)):
        emu.write32(OBJECT_VTABLE + slot, stub)
    addresses = []
    answers, coords = {}, {}
    for index, absorber in enumerate(absorbers):
        address = emu.object(index + 1, vtable=BUILDING_VTABLE, rtti=RTTI_BUILDING)
        addresses.append(address)
        answers[address] = absorber['answer']
        coords[address] = absorber['xyz']
        emu.write32(ARRAYS + index * 4, address)
    emu.write32(house + 0xB4, ARRAYS)
    emu.write32(house + 0xC0, len(absorbers))
    contacts = ARRAYS + 0x1000
    emu.write32(contacts, 0 if contact is None else addresses[contact])
    emu.write32(foot_address + 0xE4, contacts)
    emu.write32(foot_address + 0x500, 0 if pending is None else addresses[pending])

    def index_of(address):
        return None if address == 0 else TeamEmu.index_of(address) - 1

    def transmit(e):
        message, to = e.arg(0), e.arg(1)
        e.events.append(['transmit', message, index_of(to)])
        return answers[to]

    def building_coords(e):
        out = e.arg(0)
        for offset, value in zip((0, 4, 8), coords[e.uc.reg_read(UC_X86_REG_ECX)]):
            e.write32(out + offset, value)
        return out
    emu.hook(STUB_TRANSMIT, transmit, 8)
    emu.hook(STUB_BUILDING_COORDS, building_coords, 4)
    emu.hook(STUB_QUEUE_MISSION, lambda e: e.events.append(
        ['queue_mission', e.arg(0), e.arg(1)]) or 1, 8)
    emu.hook(STUB_ASSIGN_DESTINATION, lambda e: e.events.append(
        ['assign_destination', index_of(e.arg(0)), e.arg(1)]), 8)
    emu.uc.hook_add(UC_HOOK_CODE, lambda uc, *_: emu.events.append(
        ['picked', index_of(uc.reg_read(UC_X86_REG_EDI))]),
        begin=BIO_REACTOR_PICKED, end=BIO_REACTOR_PICKED)
    result = emu.invoke(ENTER_BIO_REACTOR, ecx=foot_address) & 0xFF
    return dict(foot=foot, absorbers=absorbers, contact=contact, pending=pending, latch=latch,
                result=result, latch_after=emu.read8(foot_address + 0x68F),
                events=emu.events)


def bio_reactor():
    rng = random.Random(0x4DFB70)
    rows = []
    for case in range(120):
        span = (0x400, 0x4000, 0x40000, 0x7FFF00)[case % 4]
        foot = [rng.randrange(0, span), rng.randrange(0, span), rng.randrange(0, 0x800)]
        absorbers = [dict(xyz=[rng.randrange(0, span), rng.randrange(0, span),
                               rng.randrange(0, 0x800)],
                          answer=rng.choice((1, 1, 1, 10, 0)))
                     for _ in range(rng.randrange(0, 6))]
        contact = pending = None
        if absorbers and case % 3 == 1:
            contact = rng.randrange(len(absorbers))
        if absorbers and case % 5 == 2:
            pending = rng.randrange(len(absorbers))
        rows.append(bio_reactor_row(foot=foot, absorbers=absorbers, contact=contact,
                                    pending=pending, latch=case % 2 == 1))
    taker = dict(answer=1)
    # Equal distances: the walk from the end meets the later entry first and
    # keeps it; a nearer one later in the walk replaces it.
    rows.append(bio_reactor_row(foot=[0x2000, 0x2000, 0],
                                absorbers=[dict(taker, xyz=[0x2100, 0x2000, 0]),
                                           dict(taker, xyz=[0x2000, 0x2100, 0])]))
    rows.append(bio_reactor_row(foot=[0x2000, 0x2000, 0],
                                absorbers=[dict(taker, xyz=[0x2080, 0x2000, 0]),
                                           dict(taker, xyz=[0x2000, 0x2100, 0])]))
    # Distances that differ below a lepton tie after ftol.
    rows.append(bio_reactor_row(foot=[0, 0, 0],
                                absorbers=[dict(taker, xyz=[300, 400, 0]),
                                           dict(taker, xyz=[0, 500, 1])]))
    # Only a refusing absorber is nearest; none takes it; none at all.
    rows.append(bio_reactor_row(foot=[0x2000, 0x2000, 0],
                                absorbers=[dict(taker, xyz=[0x2800, 0x2000, 0]),
                                           dict(answer=10, xyz=[0x2010, 0x2000, 0])]))
    rows.append(bio_reactor_row(foot=[0x2000, 0x2000, 0], latch=True,
                                absorbers=[dict(answer=10, xyz=[0x2800, 0x2000, 0]),
                                           dict(answer=0, xyz=[0x2010, 0x2000, 0])]))
    rows.append(bio_reactor_row(foot=[0x2000, 0x2000, 0], latch=True, absorbers=[]))
    # The pick is already the radio contact or the pending entry; another
    # absorber is.
    two = [dict(taker, xyz=[0x2400, 0x2000, 0]), dict(taker, xyz=[0x3000, 0x2000, 0])]
    rows.append(bio_reactor_row(foot=[0x2000, 0x2000, 0], absorbers=two, contact=0))
    rows.append(bio_reactor_row(foot=[0x2000, 0x2000, 0], absorbers=two, pending=0))
    rows.append(bio_reactor_row(foot=[0x2000, 0x2000, 0], absorbers=two, contact=1,
                                pending=1))
    return rows


# ---------------------------------------------------------------- battle_bunker

ENTER_BATTLE_BUNKER = 0x4DFCB0
ENTER_NEAREST_OCCUPIABLE = 0x4DFE00
CAN_BE_OCCUPIED_BY = 0x457CE0


def battle_bunker_row(*, foot, bunkers, infantry=True, nav=None, mission=0, latch=False,
                      whole_map=False):
    """`bunkers`: dicts of location and CanBeOccupiedBy answer, in House+0xE0
    order; `nav`: a bunker's index as the Foot's NavCom (+0x5A4), else none;
    `mission`: the Foot's current mission (+0xAC). `whole_map` selects the
    same fixture boundaries for actual4DFE00 over BuildingClass::Array,
    rather than4DFCB0 over House+E0. No seek body is replaced."""
    emu = TeamEmu()
    house = HOUSES
    foot_address = emu.object(0, xyz=foot, rtti=RTTI_INFANTRY if infantry else RTTI_UNIT)
    emu.write32(foot_address + 0x21C, house)
    latch_offset = 0x691 if whole_map else 0x690
    emu.write8(foot_address + latch_offset, int(latch))
    emu.write32(foot_address + 0xAC, mission)
    for slot, stub in ((0x1E8, STUB_QUEUE_MISSION), (0x480, STUB_ASSIGN_DESTINATION)):
        emu.write32(OBJECT_VTABLE + slot, stub)
    addresses = []
    answers, coords = {}, {}
    for index, bunker in enumerate(bunkers):
        address = emu.object(index + 1, vtable=BUILDING_VTABLE, rtti=RTTI_BUILDING)
        addresses.append(address)
        answers[address] = bunker['answer']
        coords[address] = bunker['xyz']
        emu.write32(ARRAYS + index * 4, address)
    emu.write32(house + 0xE4, ARRAYS)
    emu.write32(house + 0xF0, len(bunkers))
    if whole_map:
        emu.write32(BUILDING_ARRAY[0], ARRAYS)
        emu.write32(BUILDING_ARRAY[1], len(bunkers))
    emu.write32(foot_address + 0x5A4, 0 if nav is None else addresses[nav])

    def index_of(address):
        return None if address == 0 else TeamEmu.index_of(address) - 1

    def can_be_occupied_by(e):
        # NULL is refused by the original's first test (0x457CEB).
        building, occupant = e.uc.reg_read(UC_X86_REG_ECX), e.arg(0)
        e.events.append(['can_be_occupied_by', index_of(building),
                         {0: None, foot_address: 'foot'}[occupant]])
        return int(occupant != 0 and answers[building])

    def building_coords(e):
        out = e.arg(0)
        for offset, value in zip((0, 4, 8), coords[e.uc.reg_read(UC_X86_REG_ECX)]):
            e.write32(out + offset, value)
        return out
    emu.hook(CAN_BE_OCCUPIED_BY, can_be_occupied_by, 4)
    emu.hook(STUB_BUILDING_COORDS, building_coords, 4)
    emu.hook(STUB_QUEUE_MISSION, lambda e: e.events.append(
        ['queue_mission', e.arg(0), e.arg(1)]) or 1, 8)
    emu.hook(STUB_ASSIGN_DESTINATION, lambda e: e.events.append(
        ['assign_destination', index_of(e.arg(0)), e.arg(1)]), 8)
    entry = ENTER_NEAREST_OCCUPIABLE if whole_map else ENTER_BATTLE_BUNKER
    result = emu.invoke(entry, ecx=foot_address) & 0xFF
    return dict(foot=foot, bunkers=bunkers, infantry=infantry, nav=nav, mission=mission,
                latch=latch, result=result, latch_after=emu.read8(foot_address + latch_offset),
                events=emu.events)


def battle_bunker():
    rng = random.Random(0x4DFCB0)
    rows = []
    for case in range(120):
        span = (0x400, 0x4000, 0x40000, 0x7FFF00)[case % 4]
        foot = [rng.randrange(0, span), rng.randrange(0, span), rng.randrange(0, 0x800)]
        bunkers = [dict(xyz=[rng.randrange(0, span), rng.randrange(0, span),
                             rng.randrange(0, 0x800)],
                        answer=rng.choice((True, True, False)))
                   for _ in range(rng.randrange(0, 6))]
        nav = rng.randrange(len(bunkers)) if bunkers and case % 3 == 1 else None
        rows.append(battle_bunker_row(foot=foot, bunkers=bunkers, infantry=case % 11 != 5,
                                      nav=nav, mission=(0, 8, 5)[case % 3] if case % 7 else 8,
                                      latch=case % 2 == 1))
    taker = dict(answer=True)
    # Equal distances: the walk from the end meets the later entry first and
    # keeps it; a refusing building nearer than the best is asked and passed.
    rows.append(battle_bunker_row(foot=[0x2000, 0x2000, 0],
                                  bunkers=[dict(taker, xyz=[0x2100, 0x2000, 0]),
                                           dict(taker, xyz=[0x2000, 0x2100, 0])]))
    rows.append(battle_bunker_row(foot=[0x2000, 0x2000, 0],
                                  bunkers=[dict(taker, xyz=[0x2080, 0x2000, 0]),
                                           dict(answer=False, xyz=[0x2000, 0x2040, 0]),
                                           dict(taker, xyz=[0x2000, 0x2100, 0])]))
    # Distances that differ below a lepton tie after ftol.
    rows.append(battle_bunker_row(foot=[0, 0, 0],
                                  bunkers=[dict(taker, xyz=[300, 400, 0]),
                                           dict(taker, xyz=[0, 500, 1])]))
    # A vehicle: each nearer building is asked with NULL and refuses.
    rows.append(battle_bunker_row(foot=[0x2000, 0x2000, 0], infantry=False, latch=True,
                                  bunkers=[dict(answer=False, xyz=[0x2100, 0x2000, 0])]))
    # None at all; the pick already NavCom on Capture; the pick NavCom on
    # another mission; Capture toward another building.
    rows.append(battle_bunker_row(foot=[0x2000, 0x2000, 0], latch=True, bunkers=[]))
    two = [dict(taker, xyz=[0x2400, 0x2000, 0]), dict(taker, xyz=[0x3000, 0x2000, 0])]
    rows.append(battle_bunker_row(foot=[0x2000, 0x2000, 0], bunkers=two, nav=0, mission=8))
    rows.append(battle_bunker_row(foot=[0x2000, 0x2000, 0], bunkers=two, nav=0, mission=5))
    rows.append(battle_bunker_row(foot=[0x2000, 0x2000, 0], bunkers=two, nav=1, mission=8))
    return rows


def nearest_occupiable():
    """Small4DFE00 controls: global registry order, all admission calls,
    ftol ties, NULL for a non-Infantry, latch clearing and reissue."""
    yes = dict(answer=True)
    two = [dict(yes, xyz=[0x2400, 0x2000, 0]),
           dict(yes, xyz=[0x3000, 0x2000, 0])]
    cases = [
        dict(foot=[0x2000, 0x2000, 0], latch=True, bunkers=[]),
        dict(foot=[0x2000, 0x2000, 0], bunkers=two),
        dict(foot=[0x2000, 0x2000, 0], bunkers=[
            dict(yes, xyz=[0x2100, 0x2000, 0]),
            dict(yes, xyz=[0x2000, 0x2100, 0])]),
        dict(foot=[0, 0, 0], bunkers=[dict(yes, xyz=[300, 400, 0]),
                                     dict(yes, xyz=[0, 500, 1])]),
        dict(foot=[0x2000, 0x2000, 0], bunkers=[
            dict(yes, xyz=[0x2080, 0x2000, 0]),
            dict(answer=False, xyz=[0x2000, 0x2040, 0]),
            dict(yes, xyz=[0x2000, 0x2100, 0])]),
        dict(foot=[0x2000, 0x2000, 0], infantry=False, latch=True, bunkers=two),
        dict(foot=[0x2000, 0x2000, 0], bunkers=two, nav=0, mission=8),
        dict(foot=[0x2000, 0x2000, 0], bunkers=two, nav=0, mission=5),
        dict(foot=[0x2000, 0x2000, 0], bunkers=two, nav=1, mission=8),
    ]
    return [battle_bunker_row(**case, whole_map=True) for case in cases]


# ---------------------------------------------------------------- tank_bunker

ENTER_TANK_BUNKER = 0x4DFF40
TANK_BUNKER_ADMITTED = 0x4DFF54
RADIO_CONTACT = 0x65AD30
STUB_CURRENT_WEAPON = STUBS + 0x1D0
WEAPON = FAKE + 0x352400
ADMITTED = dict(bunkerable=True, turret=True, weapon=True, speed_type=1, parasite=False)


def tank_bunker_row(*, foot, bunkers, admission=ADMITTED):
    """`bunkers`: dicts of location, a first radio contact (`contact`) and an
    occupant (+0x2E4, `occupied`), in House+0xCC order; `admission`: the
    Foot's 0x70FB50 inputs (its type's Bunkerable= +0xD2E and Turret= +0xCA1,
    a type on its current weapon (vt+0x3F4), its SpeedType +0x67C, a parasite
    +0x694)."""
    emu = TeamEmu()
    house = HOUSES
    held = OBJECTS + 0x40 * OBJECT_SIZE
    foot_address = emu.object(0, xyz=foot, rtti=RTTI_UNIT)
    emu.write32(foot_address + 0x14, 7)  # AbstractFlags: Techno, Object, Foot
    emu.write32(foot_address + 0x21C, house)
    emu.write32(foot_address + 0x694, held if admission['parasite'] else 0)
    foot_type = emu.techno_type_at(0)
    emu.techno_type[foot_address] = foot_type
    emu.write8(foot_type + 0xD2E, int(admission['bunkerable']))
    emu.write8(foot_type + 0xCA1, int(admission['turret']))
    emu.write32(foot_type + 0x67C, admission['speed_type'])
    emu.write32(WEAPON, TYPES + TYPE_SIZE if admission['weapon'] else 0)
    for slot, stub in ((0x278, STUB_TRANSMIT), (0x1E8, STUB_QUEUE_MISSION),
                       (0x480, STUB_ASSIGN_DESTINATION), (0x3F4, STUB_CURRENT_WEAPON)):
        emu.write32(OBJECT_VTABLE + slot, stub)
    coords = {}
    for index, bunker in enumerate(bunkers):
        address = emu.object(index + 1, vtable=BUILDING_VTABLE, rtti=RTTI_BUILDING)
        coords[address] = bunker['xyz']
        contacts = ARRAYS + 0x1000 + index * 0x40
        emu.write32(contacts, held if bunker['contact'] else 0)
        emu.write32(address + 0xE4, contacts)
        emu.write32(address + 0x2E4, held if bunker['occupied'] else 0)
        emu.write32(ARRAYS + index * 4, address)
    emu.write32(house + 0xCC, ARRAYS)
    emu.write32(house + 0xD8, len(bunkers))

    def index_of(address):
        return None if address == 0 else TeamEmu.index_of(address) - 1

    def building_coords(e):
        building = e.uc.reg_read(UC_X86_REG_ECX)
        e.events.append(['coords', index_of(building)])
        out = e.arg(0)
        for offset, value in zip((0, 4, 8), coords[building]):
            e.write32(out + offset, value)
        return out
    emu.hook(STUB_BUILDING_COORDS, building_coords, 4)
    emu.hook(STUB_CURRENT_WEAPON, lambda e: WEAPON, 0)
    emu.hook(STUB_TRANSMIT, lambda e: e.events.append(
        ['transmit', e.arg(0), index_of(e.arg(1))]) or 1, 8)
    emu.hook(STUB_QUEUE_MISSION, lambda e: e.events.append(
        ['queue_mission', e.arg(0), e.arg(1)]) or 1, 8)
    emu.hook(STUB_ASSIGN_DESTINATION, lambda e: e.events.append(
        ['assign_destination', index_of(e.arg(0)), e.arg(1)]), 8)
    emu.uc.hook_add(UC_HOOK_CODE, lambda uc, *_: emu.events.append(
        ['contact', index_of(uc.reg_read(UC_X86_REG_ECX)), emu.arg(0)]),
        begin=RADIO_CONTACT, end=RADIO_CONTACT)
    admitted = []
    emu.uc.hook_add(UC_HOOK_CODE, lambda uc, *_: admitted.append(
        uc.reg_read(UC_X86_REG_EAX) & 0xFF),
        begin=TANK_BUNKER_ADMITTED, end=TANK_BUNKER_ADMITTED)
    result = emu.invoke(ENTER_TANK_BUNKER, ecx=foot_address) & 0xFF
    return dict(foot=foot, bunkers=bunkers, admission=admission, admitted=admitted[0],
                result=result, events=emu.events)


def tank_bunker():
    rng = random.Random(0x4DFF40)
    rows = []
    for case in range(120):
        span = (0x400, 0x4000, 0x40000, 0x7FFF00)[case % 4]
        foot = [rng.randrange(0, span), rng.randrange(0, span), rng.randrange(0, 0x800)]
        bunkers = [dict(xyz=[rng.randrange(0, span), rng.randrange(0, span),
                             rng.randrange(0, 0x800)],
                        contact=rng.random() < 0.3, occupied=rng.random() < 0.3)
                   for _ in range(rng.randrange(0, 6))]
        rows.append(tank_bunker_row(foot=foot, bunkers=bunkers))
    free = dict(contact=False, occupied=False)
    # Equal distances: the walk from the end meets the later bunker first and
    # keeps it; held bunkers nearer than the best are passed.
    rows.append(tank_bunker_row(foot=[0x2000, 0x2000, 0],
                                bunkers=[dict(free, xyz=[0x2100, 0x2000, 0]),
                                         dict(free, xyz=[0x2000, 0x2100, 0])]))
    rows.append(tank_bunker_row(foot=[0x2000, 0x2000, 0],
                                bunkers=[dict(free, xyz=[0x2080, 0x2000, 0]),
                                         dict(free, contact=True, xyz=[0x2000, 0x2040, 0]),
                                         dict(free, occupied=True, xyz=[0x2040, 0x2000, 0]),
                                         dict(free, xyz=[0x2000, 0x2100, 0])]))
    # Distances that differ below a lepton tie after ftol.
    rows.append(tank_bunker_row(foot=[0, 0, 0],
                                bunkers=[dict(free, xyz=[300, 400, 0]),
                                         dict(free, xyz=[0, 500, 1])]))
    # None at all; every bunker held.
    rows.append(tank_bunker_row(foot=[0x2000, 0x2000, 0], bunkers=[]))
    rows.append(tank_bunker_row(foot=[0x2000, 0x2000, 0],
                                bunkers=[dict(free, contact=True, xyz=[0x2400, 0x2000, 0]),
                                         dict(free, occupied=True, xyz=[0x3000, 0x2000, 0])]))
    # Each admission test refuses alone, a free bunker in reach.
    two = [dict(free, xyz=[0x2400, 0x2000, 0]), dict(free, xyz=[0x3000, 0x2000, 0])]
    for refusal in (dict(bunkerable=False), dict(turret=False), dict(weapon=False),
                    dict(speed_type=3), dict(parasite=True)):
        rows.append(tank_bunker_row(foot=[0x2000, 0x2000, 0], bunkers=two,
                                    admission=dict(ADMITTED, **refusal)))
    return rows


# ---------------------------------------------------------------- transports

LOAD_ONTO_TRANSPORT = 0x6ED200
WAIT_LOADED = 0x6E9C22
WAIT_LOADED_ENDS = {0x6E95AB: False, 0x6E953E: True, 0x6E9C55: True}
UNLOAD = 0x6EF110
REMOVE_MEMBER = 0x6EA870
LOOK_UP_BUILDING = 0x47C520
STUB_MISSION = STUBS + 0x1A0
STUB_READY = STUBS + 0x1B0
STUB_COMMENCE = STUBS + 0x1C0
ACTION = FAKE + 0x352000
PASSENGER = FAKE + 0x352100
CONTACTS = FAKE + 0x352200
NAV = FAKE + 0x352300
ZONE_INDEX = 0x40
CLASS_RTTI = {'infantry': RTTI_INFANTRY, 'unit': RTTI_UNIT, 'aircraft': RTTI_AIRCRAFT}


def transport_team(*, members, entries, zone, stray, returns, action):
    """A team for the transport actions. `members`, in list order: dicts of
    class, type `Passengers=`, passenger count (+0x114, with a first
    passenger +0x118 when not 0), the live bytes (+0x90, +0x6C, +0x81),
    +0x689, a NavCom (+0x5A4), the current mission (vt+0x184), Ready_To_
    Commence's answer, radio contact, an archive target (+0x218), whether a
    building stands in its cell, and its location. `entries`: the TaskForce's
    (class, `Passengers=`) types. The zone (+0x34) is an object at `zone`;
    Stray= is Rules+0x171C, ScriptClass::Current answers `action`."""
    emu = TeamEmu()
    for slot, stub in ((0x184, STUB_MISSION), (0x1E8, STUB_QUEUE_MISSION),
                       (0x480, STUB_ASSIGN_DESTINATION), (0x200, STUB_READY),
                       (0x1EC, STUB_COMMENCE)):
        emu.write32(OBJECT_VTABLE + slot, stub)
    emu.write32(RULES + 0x171C, stray)
    emu.write32(RULES + 0x1720, 0x7FFFFFFF)
    emu.write8(TEAM_TYPE + 0xF4, int(returns))
    emu.write32(TASK_FORCE + 0x9C, len(entries))
    for slot, (kind, passengers) in enumerate(entries):
        emu.write32(TASK_FORCE + 0xA4 + slot * 8, 1)
        emu.write32(TASK_FORCE + 0xA8 + slot * 8,
                    emu.techno_type_at(0x20 + slot, rtti=TYPE_RTTI[kind], passengers=passengers))
    zone_address = emu.object(ZONE_INDEX, xyz=zone)
    emu.write32(TEAM + 0x34, zone_address)
    emu.write32(TEAM + 0x48, len(members))
    addresses, missions, ready, buildings = [], {}, {}, {}
    previous = None
    for index, member in enumerate(members):
        address = emu.object(index, xyz=member['xyz'], rtti=CLASS_RTTI[member['class']])
        addresses.append(address)
        emu.write8(address + 0x90, int(member['active']))
        emu.write32(address + 0x6C, member['health'])
        emu.write8(address + 0x81, int(member['limbo']))
        emu.write8(address + 0x689, int(member['initiated']))
        emu.write32(address + 0x114, member['count'])
        emu.write32(address + 0x118, PASSENGER if member['count'] else 0)
        emu.write32(address + 0x5A4, NAV if member['nav'] else 0)
        if member['radio']:
            emu.write32(address + 0xE4, CONTACTS)
            emu.write32(address + 0xE8, 2)
            emu.write32(CONTACTS + 4, NAV)
        emu.write32(address + 0x218, CELL if member['archive'] else 0)
        emu.techno_type[address] = emu.techno_type_at(index, passengers=member['passengers'])
        missions[address] = member['mission']
        ready[address] = int(member['ready'])
        buildings[tuple(member['xyz'])] = member['on_building']
        if previous is None:
            emu.write32(TEAM + 0x54, address)
        else:
            emu.write32(previous + 0x5D8, address)
        previous = address

    def ref(address):
        if address == 0:
            return None
        if address == CELL:
            return 'cell'
        if address == zone_address:
            return 'zone'
        return TeamEmu.index_of(address)

    def member(e):
        return ref(e.uc.reg_read(UC_X86_REG_ECX))

    def current(e):
        out = e.arg(0)
        e.write32(out, action)
        e.write32(out + 4, 0)
        return out
    last_cell = []

    def cell_at(e):
        coord = e.arg(0)
        xyz = [i32(e.read32(coord + offset)) for offset in (0, 4, 8)]
        e.events.append(['cell_at', xyz])
        last_cell[:] = [tuple(xyz)]
        return CELL

    def building(e):
        found = buildings[last_cell[0]]
        e.events.append(['building', found])
        return 0x1234 if found else 0
    emu.hook(CURRENT_ACTION, current, 4)
    emu.hook(GET_CELL_AT, cell_at, 4)
    emu.hook(LOOK_UP_BUILDING, building, 0)
    emu.hook(STUB_MISSION, lambda e: missions[e.uc.reg_read(UC_X86_REG_ECX)], 0)
    emu.hook(STUB_QUEUE_MISSION, lambda e: e.events.append(
        ['queue_mission', member(e), e.arg(0), e.arg(1)]), 8)
    emu.hook(STUB_ASSIGN_DESTINATION, lambda e: e.events.append(
        ['assign_destination', member(e), ref(e.arg(0)), e.arg(1)]), 8)

    def ready_stub(e):
        e.events.append(['ready', member(e)])
        return ready[e.uc.reg_read(UC_X86_REG_ECX)]
    emu.hook(STUB_READY, ready_stub, 0)
    emu.hook(STUB_COMMENCE, lambda e: e.events.append(['commence', member(e)]) or 1, 0)
    emu.hook(REMOVE_MEMBER, lambda e: e.events.append(
        ['remove_member', ref(e.arg(0)), i32(e.arg(1)), e.arg(2)]) or 1, 12)
    return emu, addresses


def transport_outputs(emu, addresses):
    return dict(finished=emu.read8(TEAM + 0x80),
                initiated=[emu.read8(address + 0x689) for address in addresses],
                archive=[{0: None, CELL: 'cell'}[emu.read32(address + 0x218)]
                         for address in addresses],
                events=emu.events)


def transport_member(rng, *, kind=None):
    kind = kind or rng.choice(('infantry', 'infantry', 'infantry', 'unit', 'unit', 'aircraft'))
    if kind == 'infantry':
        passengers = rng.choice((0, 0, 0, 0, 2))
    elif kind == 'aircraft':
        passengers = rng.choice((0, 2, 2))
    else:
        passengers = rng.choice((0, 1, 2, 2, 5, -1))
    count = rng.randrange(0, passengers + 2) if passengers > 0 else rng.choice((0, 0, 0, 1))
    return {'class': kind, 'passengers': passengers, 'count': count,
            'active': rng.random() > 0.08, 'health': rng.choice((0, 1, 100, 100, 100, 100)),
            'limbo': rng.random() < 0.08, 'initiated': rng.random() > 0.3,
            'nav': rng.random() < 0.3, 'mission': rng.choice((5, 5, 7, 2, 0x10, 1)),
            'ready': rng.random() < 0.6, 'radio': rng.random() < 0.15,
            'archive': rng.random() < 0.4, 'on_building': rng.random() < 0.2,
            'xyz': [rng.randrange(0x80, 0x1F80), rng.randrange(0x80, 0x1F80), 0]}


def transport_case(rng):
    members = [transport_member(rng) for _ in range(rng.randrange(0, 6))]
    # Distinct cells, so the building answer is per member.
    for member, cell in zip(members, rng.sample(range(1, 31 * 31), len(members))):
        member['xyz'][0] = (cell % 31) * 256 + rng.randrange(0, 256)
        member['xyz'][1] = (cell // 31) * 256 + rng.randrange(0, 256)
    entries = [(kind, passengers) for kind, passengers in
               rng.sample([('unit', 5), ('infantry', 0), ('aircraft', 0), ('aircraft', 2),
                           ('unit', 0)], rng.randrange(0, 4))]
    return dict(members=members, entries=entries,
                zone=[rng.randrange(0x80, 0x1F80), rng.randrange(0x80, 0x1F80), 0],
                stray=rng.choice((0x100, 0x200, 0x400, 0x800)), returns=rng.random() < 0.5)


def load_row(*, members, entries, zone, stray, returns):
    emu, addresses = transport_team(members=members, entries=entries, zone=zone, stray=stray,
                                    returns=returns, action=14)
    emu.write32(ACTION, 14)
    emu.invoke(LOAD_ONTO_TRANSPORT, ecx=TEAM, args=(ACTION, 0))
    return dict(members=members, entries=entries, zone=zone, stray=stray, returns=returns,
                **transport_outputs(emu, addresses))


def load():
    rng = random.Random(0x6ED200)
    rows = []
    for case in range(80):
        fields = transport_case(rng)
        if case % 2 and fields['members']:
            # A vehicle with free seats, so passengers are sent aboard.
            member = rng.choice(fields['members'])
            member.update({'class': 'unit', 'passengers': 2, 'count': 0, 'radio': False})
        rows.append(load_row(**fields))

    def plain(kind, passengers=0, count=0, **fields):
        return dict({'class': kind, 'passengers': passengers, 'count': count, 'active': True,
                     'health': 100, 'limbo': False, 'initiated': True, 'nav': False,
                     'mission': 5, 'ready': True, 'radio': False, 'archive': False,
                     'on_building': False, 'xyz': [0x800, 0x800, 0]}, **fields)

    def at(index, member):
        member['xyz'] = [0x480 + 0x200 * index, 0x900, 0]
        return member
    base = dict(entries=[('unit', 2), ('infantry', 0)], zone=[0x800, 0x900, 0], stray=0x200)
    # A full transport before the free one; the first passenger is ordered,
    # the next waits for the update after.
    rows.append(load_row(members=[at(0, plain('infantry')), at(1, plain('unit', 2, 2)),
                                  at(2, plain('unit', 2, 1)), at(3, plain('infantry'))],
                         returns=False, **base))
    # The first passenger is already on Enter, the second is ordered.
    rows.append(load_row(members=[at(0, plain('infantry', mission=7)),
                                  at(1, plain('unit', 2)), at(2, plain('infantry'))],
                         returns=False, **base))
    # Every passenger on Enter: the action waits.
    rows.append(load_row(members=[at(0, plain('infantry', mission=7)),
                                  at(1, plain('unit', 2)), at(2, plain('infantry', mission=7))],
                         returns=False, **base))
    # The transport in radio contact; alone (finished, archived or not).
    rows.append(load_row(members=[at(0, plain('unit', 2, radio=True)), at(1, plain('infantry'))],
                         returns=False, **base))
    for returns in (False, True):
        rows.append(load_row(members=[at(0, plain('unit', 2))], returns=returns, **base))
    # A far member without a NavCom is sent to the zone; one with a NavCom is
    # not; both stay unjoined, so the action finishes.
    rows.append(load_row(members=[at(0, plain('unit', 2)),
                                  dict(plain('infantry', initiated=False), xyz=[0x1E00, 0x1E00, 0]),
                                  dict(plain('infantry', initiated=False, nav=True),
                                       xyz=[0x1E00, 0x1C00, 0])],
                         returns=True, **base))
    # A near unjoined member joins and is ordered at once.
    rows.append(load_row(members=[at(0, plain('unit', 2)),
                                  dict(plain('infantry', initiated=False), xyz=[0x880, 0x900, 0])],
                         returns=False, **base))
    # No vehicle with a free seat; Passengers= 0 and negative; an aircraft
    # entry with Passengers= wants an aircraft.
    rows.append(load_row(members=[at(0, plain('unit', 0)), at(1, plain('unit', -1)),
                                  at(2, plain('infantry', 2))], returns=False, **base))
    rows.append(load_row(members=[at(0, plain('unit', 2)), at(1, plain('infantry'))],
                         returns=False, entries=[('aircraft', 2)], zone=base['zone'],
                         stray=base['stray']))
    # Dead, inactive and limbo members are not ordered.
    rows.append(load_row(members=[at(0, plain('unit', 2)), at(1, plain('infantry', health=0)),
                                  at(2, plain('infantry', active=False)),
                                  at(3, plain('infantry', limbo=True))],
                         returns=True, **base))
    return rows


def wait_loaded_row(*, members):
    emu, _ = transport_team(members=members, entries=[], zone=[0, 0, 0], stray=0, returns=False,
                            action=43)
    emu.uc.reg_write(UC_X86_REG_ESI, TEAM)
    reached = emu.start(WAIT_LOADED, tuple(WAIT_LOADED_ENDS), ecx=TEAM)
    return dict(members=members, finished=WAIT_LOADED_ENDS[reached])


def wait_loaded():
    rng = random.Random(0x6E9C22)
    rows = []
    for _ in range(40):
        rows.append(wait_loaded_row(members=[transport_member(rng)
                                             for _ in range(rng.randrange(0, 5))]))
    return rows


def unload_row(*, members, entries, zone, stray, returns, argument):
    emu, addresses = transport_team(members=members, entries=entries, zone=zone, stray=stray,
                                    returns=returns, action=8)
    emu.write32(ACTION, 8)
    emu.write32(ACTION + 4, argument)
    emu.invoke(UNLOAD, ecx=TEAM, args=(ACTION, 0))
    return dict(members=members, entries=entries, zone=zone, stray=stray, returns=returns,
                argument=argument, **transport_outputs(emu, addresses))


def unload():
    rng = random.Random(0x6EF110)
    rows = []
    for case in range(100):
        fields = transport_case(rng)
        if case % 2:
            # Nobody carries a passenger, so the members are sorted.
            for member in fields['members']:
                member['count'] = 0
        rows.append(unload_row(argument=rng.choice((0, 1, 2, 3, 4, -1)), **fields))
    return rows



# ---------------------------------------------------------------- deploy

SCRIPT_DEPLOY = 0x6ED4D0
GET_MAP_COORDS = 0x41BEA0
FLUSH_FOR_PLACEMENT = 0x45EE70
STUB_MARK = STUBS + 0x1E0
STUB_CAN_PLACE_AT = STUBS + 0x1F0
DEPLOY_TYPE_VTABLE = FAKE + 0x352500


def i16_pair(raw):
    """A CellStruct's X and Y, signed."""
    return [struct.unpack('<h', struct.pack('<H', half))[0] for half in (raw & 0xFFFF, raw >> 16)]


def deploy_row(*, members, zone, stray):
    """Action 9 over a team of `members` (transport_team's fields, with no
    passengers) of house HOUSES (+0x21C). A unit's type `DeploysInto=` a
    building (+0x404) when `deploys`, whose CanPlaceAt (vt+0xA8) answers
    `can_place`, and is `IsSimpleDeployer=` (+0xE13) when `simple`; an
    infantryman's type is `Deployer=` (+0xEC8) when `deployer`."""
    emu, addresses = transport_team(members=members, entries=[], zone=zone, stray=stray,
                                    returns=False, action=9)
    emu.write32(DEPLOY_TYPE_VTABLE + 0xA8, STUB_CAN_PLACE_AT)
    for slot, stub in ((0x124, STUB_MARK), (0x1B8, GET_MAP_COORDS)):
        emu.write32(OBJECT_VTABLE + slot, stub)
    buildings = {}
    for index, (address, member) in enumerate(zip(addresses, members)):
        emu.write32(address + 0x21C, HOUSES)
        kind = emu.techno_type[address]
        if member['class'] == 'unit':
            emu.write32(address + 0x6C4, kind)
            emu.write8(kind + 0xE13, int(member['simple']))
            if member['deploys']:
                building = BUILDING_TYPES + index * 0x100
                emu.write32(building, DEPLOY_TYPE_VTABLE)
                emu.write32(kind + 0x404, building)
                buildings[building] = (index, member['can_place'])
        elif member['class'] == 'infantry':
            emu.write32(address + 0x6C0, kind)
            emu.write8(kind + 0xEC8, int(member['deployer']))

    def can_place(e):
        index, answer = buildings[e.uc.reg_read(UC_X86_REG_ECX)]
        e.events.append(['can_place', index, i16_pair(e.read32(e.arg(0))), e.arg(1)])
        return int(answer)

    def flush(e):
        index, _ = buildings[e.uc.reg_read(UC_X86_REG_ECX)]
        e.events.append(['flush', index, i16_pair(e.read32(e.arg(0))),
                         'house' if e.arg(1) == HOUSES else e.arg(1)])
        return 0
    emu.hook(STUB_CAN_PLACE_AT, can_place, 8)
    emu.hook(FLUSH_FOR_PLACEMENT, flush, 8)
    emu.hook(STUB_MARK, lambda e: e.events.append(
        ['mark', TeamEmu.index_of(e.uc.reg_read(UC_X86_REG_ECX)), e.arg(0)]), 4)
    emu.write32(ACTION, 9)
    emu.invoke(SCRIPT_DEPLOY, ecx=TEAM, args=(ACTION, 0))
    return dict(members=members, zone=zone, stray=stray, **transport_outputs(emu, addresses))


def deploy_member(rng, slot, *, kind=None, **fields):
    """A member on column 4 + 5 * slot, so a building's 2x1 foundation from
    its cell meets no other member's."""
    kind = kind or rng.choice(('unit', 'unit', 'unit', 'infantry', 'infantry', 'aircraft'))
    member = {'class': kind, 'passengers': 0, 'count': 0,
              'active': rng.random() > 0.08, 'health': rng.choice((0, 1, 100, 100, 100, 100)),
              'limbo': rng.random() < 0.08, 'initiated': rng.random() > 0.3,
              'nav': rng.random() < 0.3, 'mission': rng.choice((5, 5, 0x10, 2, 1, 0xF)),
              'ready': False, 'radio': False, 'archive': False, 'on_building': False,
              'deploys': kind == 'unit' and rng.random() < 0.5,
              'can_place': rng.random() < 0.6,
              'simple': kind == 'unit' and rng.random() < 0.4,
              'deployer': kind == 'infantry' and rng.random() < 0.5,
              'xyz': [(4 + 5 * slot) * 256 + rng.randrange(0, 256),
                      rng.randrange(4, 27) * 256 + rng.randrange(0, 256), 0]}
    member.update(fields)
    return member


def deploy():
    rng = random.Random(0x6ED4D0)

    def zone():
        return [rng.randrange(27, 30) * 256 + rng.randrange(0, 256),
                rng.randrange(4, 27) * 256 + rng.randrange(0, 256), 0]
    rows = []
    for _ in range(100):
        members = [deploy_member(rng, slot) for slot in range(rng.randrange(0, 6))]
        rows.append(deploy_row(members=members, zone=zone(),
                               stray=rng.choice((0x200, 0x800, 0x1000, 0x2000))))
    ready = dict(active=True, health=100, limbo=False, initiated=True, nav=True, mission=5)
    mcv = dict(ready, kind='unit', deploys=True, simple=False)
    cases = [
        # No members: finished at once.
        [],
        # A deploying unit on Unload holds the action; one placed or refused.
        [deploy_member(rng, 0, **dict(mcv, mission=0x10))],
        [deploy_member(rng, 0, **dict(mcv, can_place=True))],
        [deploy_member(rng, 0, **dict(mcv, can_place=False))],
        # Simple deployers and Deployer= infantry alone: finished.
        [deploy_member(rng, 0, **dict(ready, kind='unit', deploys=False, simple=True)),
         deploy_member(rng, 1, **dict(ready, kind='infantry', deployer=True)),
         deploy_member(rng, 2, **dict(ready, kind='infantry', deployer=True, mission=0x10)),
         deploy_member(rng, 3, **dict(ready, kind='aircraft'))],
        # A deploying unit that has not joined does not hold the action.
        [deploy_member(rng, 0, **dict(mcv, initiated=False, nav=False)),
         deploy_member(rng, 1, **dict(ready, kind='unit', deploys=False, simple=True))],
        # A unit both DeploysInto= and IsSimpleDeployer= takes the building arm.
        [deploy_member(rng, 0, **dict(mcv, simple=True, can_place=True))],
    ]
    for members in cases:
        rows.append(deploy_row(members=members, zone=zone(), stray=0x200))
    return rows


def delete_members_rows():
    """Original Script37 inline list walk; lifecycle virtuals are receipts.

    Every Limbo callback unlinks the current head and clears its next link;
    this independently exercises the original pre-callback next cache. The
    complete class Limbo/UnInit bodies and deferred drain are Rust integration
    dependencies, NOT emulated by these declared sinks.
    """
    rows = []
    for order in ([], [2], [2, 0, 1]):
        emu = TeamEmu()
        pointers = [emu.object(index, rtti=RTTI_UNIT) for index in order]
        for current, following in zip(pointers, pointers[1:] + [0]):
            emu.write32(current + 0x5D8, following)
        emu.write32(TEAM + 0x54, pointers[0] if pointers else 0)
        emu.write8(TEAM + 0x80, 0)
        def limbo(e):
            current = e.uc.reg_read(UC_X86_REG_ECX)
            e.events.append(["limbo", e.index_of(current)])
            e.write32(TEAM + 0x54, e.read32(current + 0x5D8))
            e.write32(current + 0x5D8, 0)
            return 1
        def uninit(e):
            e.events.append(["uninit", e.index_of(e.uc.reg_read(UC_X86_REG_ECX))])
        emu.write32(OBJECT_VTABLE + 0xD4, STUBS + 0x800)
        emu.write32(OBJECT_VTABLE + 0xF8, STUBS + 0x810)
        emu.hook(STUBS + 0x800, limbo, 0)
        emu.hook(STUBS + 0x810, uninit, 0)
        emu.events = []
        emu.uc.reg_write(UC_X86_REG_ESI, TEAM)
        emu.start(0x6E954D, 0x6E957B, ecx=TEAM)
        rows.append(dict(order=order, events=emu.events,
                         head=emu.read32(TEAM + 0x54), advance=emu.read8(TEAM + 0x80)))
    return rows


def delete_members_metadata():
    return provenance(
        scope="Original TeamAI Script37 inline6E954D..6E957B: cache Foot+5D8, Limbo thenUnInit in member order, set advance+80",
        assumptions=["Supplied synthetic linked members/head, initially advance0; native Script37 control flow runs unchanged",
                     "TeamEmu existing49F2F0 adjacent-cell initializer remains shared transport; no new VM model"],
        substitutions=["vtD4 Limbo records and unlinks current head/clears next; vtF8 UnInit records only; no class lifecycle, damage, RNG, audio/device, raw scalar destructor or full TeamAI timing claim"],
        entry_points={"script37": 0x6E954D, "stop_before_epilogue": 0x6E957B})


def change_script_rows():
    """Script17 replacement/reset and the next original TeamAI script visit.

    The old scalar destructor is an explicit expiry sink; the new Script
    constructor and Reset/Current are original instructions. In particular,
    replacement does NOT complete the Team's step. A later visit therefore
    sees Current(-1), rather than executing the replacement's first action.
    """
    rows = []
    for ordinal, old_script, advance in ((0, True, False), (2, True, False),
                                        (112, True, False), (1, False, False),
                                        (1, True, True)):
        emu = TeamEmu()
        script_types = FAKE + 0x340000
        script_instances = FAKE + 0x341000
        old_vtable = FAKE + 0x342000
        target_type = FAKE + 0x343000
        emu.write32(0x8B41CC, script_types)
        emu.write32(script_types + ordinal * 4, target_type)
        emu.write32(0x8872B4, script_instances)
        emu.write32(0x8872B8, 8)
        emu.write32(0x8872C0, 0)
        emu.write32(SCRIPT, old_vtable)
        emu.write32(old_vtable + 0x20, STUBS + 0x820)
        emu.write32(TEAM + 0x28, SCRIPT if old_script else 0)
        emu.write32(TEAM + 0x3C, CELL)
        emu.write32(TEAM + 0x40, CELL)
        emu.write8(TEAM + 0x80, int(advance))
        def destroy_old(e):
            e.events.append(['destroy_old', e.arg(0)])
            # Actual TeamPointerExpired6EAE60 clears +28 only. Its native
            # body has no +80 write; this transport does not invent one.
            e.write32(TEAM + 0x28, 0)
        emu.hook(STUBS + 0x820, destroy_old, 4)
        emu.mark(0x6913C0, 'construct_new')
        emu.mark(0x691590, 'reset_new')
        emu.events = []
        sp = STACK_BASE + STACK_SIZE - 0x1000 - 4
        emu.write32(sp + 0x24, ordinal)
        emu.uc.reg_write(UC_X86_REG_ESI, TEAM)
        emu.start(0x6E972C, 0x6E9767, ecx=TEAM)
        replacement = emu.read32(TEAM + 0x28)
        row = dict(ordinal=ordinal, old_script=old_script,
                   advance_before=advance,
                   advance_after=bool(emu.read8(TEAM + 0x80)),
                   cursor=emu.read_i32(replacement + 0x2C),
                   target_type_matches=emu.read32(replacement + 0x24) == target_type,
                   target_retained=emu.read32(TEAM + 0x3C) == CELL,
                   focus_retained=emu.read32(TEAM + 0x40) == CELL,
                   instances=emu.read32(0x8872C0), events=emu.events.copy())
        if not advance:
            emu.uc.reg_write(UC_X86_REG_ESI, TEAM)
            emu.start(0x6E9364, 0x6E95AB, ecx=0)
            row['next_action'] = emu.read_i32(sp + 0x20)
            row['next_cursor'] = emu.read_i32(replacement + 0x2C)
            row['next_advance'] = bool(emu.read8(TEAM + 0x80))
        rows.append(row)
    return rows


def change_script_metadata():
    return provenance(
        scope='Original Script17 inline6E972C..6E9767, Script constructor6913C0, Reset691590 and next TeamAI script visit9364..95AB',
        assumptions=['Supplied ScriptType array ordinal, retained mission/focus and Team StepCompleted prior; valid allocation and ScriptType pointer only.',
                     'Script instance registry has preallocated capacity8. No whole TeamAI formation/recruitment or mission scene claim.'],
        substitutions=['Old Script scalar destructor records delete flag and models only TeamPointerExpired6EAE60 clearing+28; no destructor/expiry-scene parity claim.',
                       'Shared heap allocation/free boundaries come from TeamEmu. Abstract/Script constructors, Reset, Current and retained registry append execute unchanged.',
                       'No RNG, timer arithmetic or Team+80 write exists in this action; no RNG/timer replacement is added.'],
        entry_points={'script17': 0x6E972C, 'script_constructor': 0x6913C0,
                      'script_reset': 0x691590, 'script_current': 0x691500,
                      'team_pointer_expired': 0x6EAE60,
                      'team_script_visit': 0x6E9364})


def attack_waypoint_building_rows():
    """Original Script59 selector/ammunition gates, with shared-owner seams."""
    rows=[]
    cases=[dict(name='building_unlimited',blocker='building',first=True,type_ammo=-1,ammo=0,count=3),
        dict(name='other_blocker',blocker='other',first=True,type_ammo=-1,ammo=0,count=3),
        dict(name='no_blocker',blocker=None,first=True,type_ammo=-1,ammo=0,count=3),
        dict(name='spent_building',blocker='building',first=True,type_ammo=1,ammo=0,count=3),
        dict(name='retained_building',blocker=None,first=False,type_ammo=1,ammo=1,count=3),
        dict(name='empty_team',blocker='building',first=True,type_ammo=-1,ammo=0,count=0)]
    for spec in cases:
        emu=TeamEmu();action=SCRIPT+0x200;target=OBJECTS+8*OBJECT_SIZE
        emu.object(8,rtti=RTTI_BUILDING if spec['blocker']!='other' else RTTI_INFANTRY)
        emu.write32(CELL,OBJECT_VTABLE);emu.rtti[CELL]=11
        emu.write32(CELL+0x140,0)
        emu.write32(action,59);emu.write32(action+4,11)
        emu.write32(TEAM+0x3C,target if not spec['first'] else 0)
        emu.write32(TEAM+0x40,emu.read32(TEAM+0x3C))
        previous=None
        for index in range(spec['count']):
            actor=emu.object(index);type_pointer=emu.techno_type_at(index)
            emu.techno_type[actor]=type_pointer
            emu.write32(type_pointer+0x684,spec['type_ammo']);emu.write32(actor+0x2FC,spec['ammo'])
            if previous is None:emu.write32(TEAM+0x54,actor)
            else:emu.write32(previous+0x5D8,actor)
            previous=actor
        def assign(e):
            value=e.arg(0);e.events.append(['assign',bool(value)])
            prior=e.read32(TEAM+0x3C)
            if e.read32(TEAM+0x40) in (0,prior):e.write32(TEAM+0x40,value)
            e.write32(TEAM+0x3C,value)
        def blocker(e):
            e.events.append(['blocker',bool(e.arg(1)&255)])
            return target if spec['blocker'] else 0
        emu.hook(0x68BCE0,lambda e:e.events.append(['waypoint',i32(e.arg(0))]) or CELL,4)
        emu.hook(0x47C5A0,blocker,8)
        emu.hook(0x6E9050,assign,4)
        emu.hook(0x6EB490,lambda e:e.events.append(['coordinate_attack']),0)
        emu.events=[]
        emu.invoke(0x6ECA70,ecx=TEAM,args=(action,int(spec['first'])))
        rows.append(dict(input=spec,target_present=bool(emu.read32(TEAM+0x3C)),
            advance=bool(emu.read8(TEAM+0x80)),coordinate_attack=any(event[0]=='coordinate_attack' for event in emu.events),
            events=emu.events))
    return rows


def attack_waypoint_building_metadata():
    return provenance(scope='Original6ECA70 Script59 first waypoint/blocker RTTI gate, member Type Ammo/current Ammo gate and retained target routing',
        assumptions=['Declared synthetic Team/member/current target, exact signed Type Ammo and current Ammo priors; member count0/3 and first/repeat supplied. Original whole6ECA70 executes unchanged.',
            'This is a handler control fixture, not full waypoint/map/CoordinateAttack execution, type/rules loading, TeamAI scheduling or an ALL01 scene.'],
        substitutions=['Existing TeamEmu WhatAmI/GetTechnoType transports supply declared RTTI/type identity. Scenario68BCE0 supplies a Cell; Cell47C5A0 records bridge-list argument and supplies declared building/other/null blocker.',
            'AssignMissionTarget6E9050 records and maintains the retained shared target view; CoordinateAttack6EB490 records only. Their production owners and original corpora are separate. No arithmetic, RNG or timer call is replaced in59 itself.'],
        entry_points={'script59':0x6ECA70,'waypoint_cell':0x68BCE0,'blocker':0x47C5A0,
            'assign_mission_target':0x6E9050,'coordinate_attack':0x6EB490})


def generate():
    return dict(recalc=recalc(), center=center(), recruit=recruit(), distance=distance(),
                guard=guard(), regroup=regroup(), own_building=own_building(),
                target_building=target_building(), gather=gather(), quarry=quarry(), attack_cadence=attack_cadence(),
                bio_reactor=bio_reactor(), battle_bunker=battle_bunker(),
                tank_bunker=tank_bunker(), load=load(), wait_loaded=wait_loaded(),
                unload=unload(), deploy=deploy())


if __name__ == '__main__' and '--retreat' in sys.argv:
    finish_vectors(retreat, Path(__file__).with_name('team_retreat.json'),
        provenance=lambda: provenance(
            scope=('Original Foot4CBC40 directional cell visits and Team6EA0D0 '
                   'retreat selection through original CalcCenter6EAEE0, House '
                   'threat56BCD0, Sqrt_Approx/ftol and nearby Foot search. '
                   'Four additional whole Foot/Infantry/Unit admission controls '
                   'distinguish flat ground refusal from ramp admission.'),
            assumptions=['x87 control word0E7F (PC53,chop), harness default.',
                         'Synthetic ordered House buildings and retained coarse threat '
                         'grid; two joined live members at [5248,5248,0]/[5504,5248,0].'],
            substitutions=['Map5657A0/565730 return distinct declared real Cells with '
                           'original Cell GetCoords486840;56BCD0 inline lookup runs original.',
                           'Class CanEnter records direction,height,previous,mode and '
                           'answers the supplied ordinal or blocked-cell set.',
                           'Selector/directional controls: Building GetCoords/GetWeapon supply declared coordinates '
                           'and armed status; Script.Current supplies action58. '
                           'Original Script.Reset executes.',
                           'CoordinateMove6EBAD0 records final focus; member movement '
                           'is covered separately through the Rust production team pass.',
                           'live_nearby controls reuse unit_source_scatter with original '
                           'Cell/Infantry/Unit vtables and Walk/Drive construction. '
                           'Declared level0,slope0/1,clear speed rows,no overlay or tube; '
                           'no class entry/gameplay return substituted in these rows.'],
            entry_points={'retreat':RETREAT, 'nearby':FOOT_NEARBY,
                          'center':CALC_CENTER, 'threat':0x56BCD0,
                          'sqrt_approx':0x4CAC40, 'ftol':0x7C5F00}),
        argv=[arg for arg in sys.argv[1:] if arg != '--retreat'])
    raise SystemExit

if __name__ == '__main__' and '--attack-waypoint-building' in sys.argv:
    finish_vectors(attack_waypoint_building_rows, Path(__file__).with_name('team_attack_waypoint_building.json'),
                   provenance=attack_waypoint_building_metadata,
                   argv=[arg for arg in sys.argv[1:] if arg != '--attack-waypoint-building'])
    raise SystemExit

if __name__ == '__main__' and '--delete-members' in sys.argv:
    finish_vectors(delete_members_rows, Path(__file__).with_name('team_delete_members.json'),
                   provenance=delete_members_metadata,
                   argv=[arg for arg in sys.argv[1:] if arg != '--delete-members'])
    raise SystemExit

if __name__ == '__main__' and '--change-script' in sys.argv:
    finish_vectors(change_script_rows, Path(__file__).with_name('team_change_script.json'),
                   provenance=change_script_metadata,
                   argv=[arg for arg in sys.argv[1:] if arg != '--change-script'])
    raise SystemExit

if __name__ == '__main__' and '--force-facing' in sys.argv:
    finish_vectors(force_facing, Path(__file__).with_name('team_force_facing.json'),
        provenance=lambda: provenance(
            scope='Script42/6EDCA0 complete caller with original class DoTurn bodies and FacingClass Set/SetCurrent/Current, eight active locomotor families.',
            assumptions=['Member order and retained FacingClass fields are supplied raw controls.',
                         'Rocket interface7F0B1C+4C inherits55AC30, established from original constructor661F1F and table bytes.',
                         'High/padding words unused by native DirStruct/timer consumers are omitted from serialized projection.',
                         'Null locomotor COM faults are outside the native controls; Rust retains the pending step as a safety policy.'],
            substitutions=['ILocomotion IsMoving+10 supplies fixture booleans, without executing its movement engine. Its existing Rust/native-query corpora remain independent.',
                           'No substitutions for class DoTurn or FacingClass arithmetic/timer writes; these original instructions execute.'],
            entry_points={'entry': FORCE_FACING, 'facing_set': 0x4C9220,
                       'facing_snap': 0x4C9300, 'facing_current': 0x4C93D0} | TURN_ENTRIES),
        argv=[arg for arg in sys.argv[1:] if arg != '--force-facing'])
    raise SystemExit

if __name__ == '__main__' and '--patrol' in sys.argv:
    finish_vectors(patrol, Path(__file__).with_name('team_patrol.json'),
        provenance=lambda: provenance(
            scope='Script16/6ECCE0 bounded control routing, native leader selection, AI PatrolScan constructor/ReadDouble/conversion and global signed modulo.',
            assumptions=['Fixture members retain native order, LeadershipRating and live/limbo/initiated flags.',
                         'The supplied AI reader index and original CRT initializer come from the existing path-delay reader fixture.',
                         'PC53/chop. Invalid zero divisors are not executed as gameplay controls.',
                         'Malformed authored floats retain the existing non-retail reader residual.'],
            substitutions=['Waypoint lookup and GreatestThreat return supplied identities; scanner args are recorded.',
                           'AssignMissionTarget projects only +3C/+40 identity retention; downstream member cleanup is covered by Rust integration, not this native slice.',
                           'CoordinateAttack/Move record which interface is selected; their movement/combat, timers and RNG are not executed.'],
            entry_points={'entry': PATROL, 'constructor': 0x666E09,
                       'reader': PATROL_READER, 'convert': PATROL_CONVERT,
                       'idiv': PATROL_IDIV, 'read_double': 0x5283D0}),
        argv=[arg for arg in sys.argv[1:] if arg != '--patrol'])
    raise SystemExit

if __name__ == '__main__' and '--nearest-occupiable' in sys.argv:
    finish_vectors(nearest_occupiable,
        Path(__file__).with_name('team_nearest_occupiable.json'),
        provenance=lambda: provenance(
            scope=('Foot4DFE00 Script64 seek over the global Building registry; '
                   'all admission calls precede actual Sqrt_Approx4CAC40/ftol7C5F00, '
                   'strict ties, +691 latch and Capture reissue. Nine bounded controls.'),
            assumptions=['x87 control word0E7F (PC53,chop), the existing harness default.',
                         'The retained Building registry order is supplied.'],
            substitutions=['CanBeOccupiedBy457CE0 answers supplied scalar admissions '
                           '(and refuses the NULL non-Infantry argument).',
                           'Building GetCoords supplies fixture coordinates; Foot '
                           'GetCoords uses original5F65A0. Destination/QueueMission '
                           'record arguments. Their downstream path is not executed.'],
            entry_points={'entry': ENTER_NEAREST_OCCUPIABLE,
                       'admission': CAN_BE_OCCUPIED_BY,
                       'sqrt_approx': 0x4CAC40, 'ftol': 0x7C5F00}),
        argv=[arg for arg in sys.argv[1:] if arg != '--nearest-occupiable'])
    raise SystemExit

if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope=('TeamClass::Recalc over TaskForce totals, member counts and strength bytes; '
               'Calc_Center off action 10 over synthetic members and move targets; Recruit '
               'over one class array with supplied Can_Add answers; ObjectClass::Distance to '
               'a Foot or a building of every foundation; the action 5 guard timer; action '
               '54\'s point up to its FNPC call; FindOwnBuilding over one house\'s buildings; '
               'Find_Best_Target_Building over BuildingClass::Array with four houses; '
               'action 53 up to its FNPC call or its early return; Quarry_To_Threat; '
               'Coordinate_Attack\'s frame test; EnterBioReactor over one house\'s '
               'absorber list with supplied CAN_LOAD answers; EnterBattleBunker over one '
               'house\'s occupiable list with supplied CanBeOccupiedBy answers; '
               'EnterTankBunker over one house\'s tank bunker list; actions 14, 9 '
               'and 8 and action 43\'s inline test over up to five synthetic infantry, '
               'vehicle and aircraft members.'),
        assumptions=['x87 control word 0x0E7F (PC53, chop), the harness default.',
                     'Fixture objects carry only the fields the entries read.',
                     'ScenarioInit (0xA8E7AC), the empty coordinate 0xB0E968 and the empty '
                     'cell 0xB0E948 read zero, as at load (BSS).'],
        substitutions=['WhatAmI (vt+0x2C), GetTechnoType (vt+0x84), the GuardSlower weight '
                       '(vt+0x4E0), Can_Enter_Cell (vt+0x1AC) and SetTarget (vt+0x3C8) answer '
                       'from the case; objects\' GetCoords is the original 0x5F65A0, a '
                       'building\'s answers the case\'s coordinate.',
                       'ScriptClass::Current 0x691500 answers action 0; '
                       'MapClass::GetCellAt 0x565730 records its coordinate and answers one '
                       'cell; the team destructor (vt+0x20) records its call.',
                       'Can_Add 0x6EA610 answers from the case and Add_Member 0x6EA500 '
                       'records its call; Get_Group 0x6F1870 answers the case\'s group; the '
                       'TeamType waypoint 0x6F18A0 answers the empty cell; the first '
                       'passenger 0x473450 answers none.',
                       'Base_Center 0x50DF30 answers the case\'s centres ([0, 0] for none, '
                       'z 0); RandomRanged 0x65C7E0 answers from the case and records its '
                       'range.',
                       'FindOwnBuilding\'s threat map read 0x56BCD0 answers 0.',
                       'Find_Best_Target_Building: a building\'s GetCoords (vt+0x48) answers '
                       'the case\'s coordinate; the threat map read 0x56BCD0 answers the '
                       'scored building\'s case value and records its cell and house; '
                       'IsAlliedWith 0x4F9A50 runs on the houses\' ArrayIndex (+0x30) and '
                       'Allies bits (+0x5788).',
                       'EnterBioReactor\'s Transmit_Message (vt+0x278) answers the case\'s '
                       'CAN_LOAD reply and records it; Queue_Mission (vt+0x1E8) and '
                       'Assign_Destination (vt+0x480) record their calls; the pick is read '
                       'from EDI at 0x4DFC52.',
                       'EnterBattleBunker\'s BuildingClass::CanBeOccupiedBy 0x457CE0 '
                       'answers the case\'s admission for the Foot, refuses NULL as its '
                       'first test (0x457CEB) does, and records its building and occupant; '
                       'Queue_Mission and Assign_Destination record their calls.',
                       'EnterTankBunker: the current weapon (vt+0x3F4) answers a weapon '
                       'with or without a type; Transmit_Message, Queue_Mission and '
                       'Assign_Destination record their calls; RadioClass::Contact 0x65AD30 '
                       'runs as original and records its building; the admission\'s answer '
                       'is read from AL at 0x4DFF54.',
                       'The transport actions: Get_Mission (vt+0x184) and Ready_To_Commence '
                       '(vt+0x200) answer from the case; Queue_Mission, Assign_Destination, '
                       'Assign_Target, Commence (vt+0x1EC) and TeamClass::Remove_Member '
                       '0x6EA870 record their calls; MapClass::GetCellAt 0x565730 records its '
                       'coordinate and answers one cell, for which Look_up_building_in_cell '
                       '0x47C520 answers the member\'s case (its game-active gate 0xA8E9A0 is '
                       'set in play); In_Radio_Contact 0x65AE30 and Set_ArchiveTarget '
                       '0x70C610 run as original; action 43 runs from 0x6E9C22 to its '
                       'branch targets.',
                       'Action 9: Mark (vt+0x124) and Flush_For_Placement 0x45EE70 record '
                       'their calls; the DeploysInto= building\'s CanPlaceAt (vt+0xA8) '
                       'answers the case\'s placement and records its cell and house; '
                       'GetMapCoords (vt+0x1B8) is the original 0x41BEA0.'],
        entry_points={'recalc': RECALC, 'calc_center': CALC_CENTER, 'recruit': RECRUIT,
                      'distance': DISTANCE, 'guard_case': GUARD_CASE,
                      'regroup_at_base': REGROUP_AT_BASE, 'find_own_building':
                      FIND_OWN_BUILDING, 'find_best_target_building':
                      FIND_BEST_TARGET_BUILDING, 'gather_at_enemy_base': GATHER_AT_ENEMY_BASE,
                      'quarry_to_threat': QUARRY_TO_THREAT, 'attack_cadence': CADENCE,
                      'enter_bio_reactor': ENTER_BIO_REACTOR,
                      'enter_battle_bunker': ENTER_BATTLE_BUNKER,
                      'enter_tank_bunker': ENTER_TANK_BUNKER,
                      'load_onto_transport': LOAD_ONTO_TRANSPORT, 'wait_loaded': WAIT_LOADED,
                      'unload': UNLOAD, 'script_deploy': SCRIPT_DEPLOY},
    ))
