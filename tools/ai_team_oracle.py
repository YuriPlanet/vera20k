"""Native references for the computer's team creation and unit choices.

Run python -m tools.ai_team_oracle --check (or explicit --write).
Rust consumer: src/sim/ai_team_creation_tests.rs.

Sections, each executed in a fresh emulator per case:
- choosers: the Unit, Infantry and Aircraft choosers 0x4FEA60, 0x4FEEE0 and
  0x4FF210 (their team tally, the free-object pass, the candidate pass, the
  RandomRanged draws and the FillEarliestTeamProbability compare); the Unit
  chooser with its harvester branch closed.
- harvester: the Unit chooser's harvester branch 0x4FEA7D..0x4FEBD7.
- selector: the AI trigger team selector 0x6F0AB0 (the RatioAITriggerTeam
  draw, the team counts and the defense-team eviction, the weight ftol, the
  5000 tier, the weighted draw and the cancel pass).
- eligibility: the AI trigger test 0x41E720's defense gate and its power and
  money conditions, every other gate admitting.
- charge: the Iron Curtain readiness 0x41F0D0 on one Super.
- team_block: HouseClass::Update's team block 0x4F8A00..0x4F8B08.

Control flow the Rust tests do not compare here (the other eligibility
gates, which counter a condition reads, the Super lookup) rests on
instruction reading; see src/sim/ai_team_creation.rs.
"""
from pathlib import Path
import struct

from unicorn.x86_const import (UC_X86_REG_EBP, UC_X86_REG_ECX, UC_X86_REG_EDI,
                               UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI,
                               UC_X86_REG_ESP, UC_X86_REG_FPCW)

from tools.ai_base_building_oracle import Emu, FAKE_SIZE
from tools.native_oracle import (NATIVE_FPCW, OracleError, finish_vectors, provenance,
                                 run_checked)

FAKE = 0x50000000
HOUSE = FAKE
RULES = FAKE + 0x20000
SCENARIO = FAKE + 0x22000
ENEMY = FAKE + 0x100000
HOUSE_TYPES = FAKE + 0x110000
LISTS = FAKE + 0x120000
TYPES = FAKE + 0x130000
TYPE_SIZE = 0x1000
TEAMS = FAKE + 0x180000
TEAM_TYPES = FAKE + 0x190000
TRIGGERS = FAKE + 0x1A0000
OBJECTS = FAKE + 0x1B0000
OBJECT_SIZE = 0x800
SUPERS = FAKE + 0x1F0000
SW_TYPES = FAKE + 0x1F8000
TASK_FORCE = FAKE + 0x200000
VTABLES = FAKE + 0x210000
TYPE_VTABLE = VTABLES
IHOUSE_VTABLE = VTABLES + 0x400
TEAM_VTABLE = VTABLES + 0x800
BLOCK_STACK = FAKE + 0x220000

STUBS = 0x60000000
STUB_RTTI = STUBS + 0x100
STUB_COST = STUBS + 0x110
STUB_INDEX = STUBS + 0x120
STUB_MONEY = STUBS + 0x130
STUB_OUTPUT = STUBS + 0x140
STUB_DRAIN = STUBS + 0x150
STUB_TEAM_DELETE = STUBS + 0x160

GAME_MODE = 0xA8B238
FRAME = 0xA8ED84
TEAM_ARRAY = (0x8B40EC, 0x8B40F8)
TRIGGER_ARRAY = (0xA8B204, 0xA8B210)
HOUSE_ARRAY = (0xA8022C, 0xA80238)
UNIT_TYPES = (0xA83CE4, 0xA83CF0)
INFANTRY_TYPES = (0xA8E34C, 0xA8E358)
AIRCRAFT_TYPES = (0xA8B21C, 0xA8B228)
UNITS = (0x8B410C, 0x8B4118)
INFANTRY = (0xA83DEC, 0xA83DF8)
AIRCRAFT = (0xA8E394, 0xA8E3A0)

# class: (chooser, choice field, RTTI of the type, type array, object array,
# the object's type field)
CHOOSERS = {
    'unit': (0x4FEA60, 0x5650, 0x28, UNIT_TYPES, UNITS, 0x6C4),
    'infantry': (0x4FEEE0, 0x5654, 0x10, INFANTRY_TYPES, INFANTRY, 0x6C0),
    'aircraft': (0x4FF210, 0x5658, 0x03, AIRCRAFT_TYPES, AIRCRAFT, 0x6C4),
}


def f64(value):
    return struct.pack('<d', value)


class TeamEmu(Emu):
    """An Emu with the fixture objects of this oracle."""

    def __init__(self, *, image_owner=None):
        if image_owner is None:
            super().__init__()
        else:
            # Compose an existing ActionFixture image: constructor/read/dispatch
            # and the selector must consume the very same House byte. The
            # caller retains its CRT, allocation, Scenario and stack transports.
            self.uc = image_owner.u
            self.uc.mem_map(FAKE, FAKE_SIZE)
            self.uc.mem_map(STUBS, 0x1000)
            self.uc.mem_write(STUBS, b'\xcc' * 0x1000)
            self.events = []
            self.write32(0x8871E0, RULES)
        self.rtti = {}
        self.cost = {}
        self.index = {}
        self.money = {}
        self.power = {}
        self.list_top = LISTS
        for slot, stub in ((0x2C, STUB_RTTI), (0x84, STUB_COST), (0x40, STUB_INDEX)):
            self.write32(TYPE_VTABLE + slot, stub)
        for slot, stub in ((0x18, STUB_MONEY), (0x20, STUB_OUTPUT), (0x24, STUB_DRAIN)):
            self.write32(IHOUSE_VTABLE + slot, stub)
        self.write32(TEAM_VTABLE + 0x20, STUB_TEAM_DELETE)
        self.hook(STUB_RTTI, lambda e: e.rtti[e.uc.reg_read(UC_X86_REG_ECX)], 0)
        self.hook(STUB_INDEX, lambda e: e.index[e.uc.reg_read(UC_X86_REG_ECX)], 0)
        self.hook(STUB_COST, self.cost_stub, 4)
        self.hook(STUB_MONEY, lambda e: e.money[e.arg(0) - 0x24], 4)
        self.hook(STUB_OUTPUT, lambda e: e.power[e.arg(0) - 0x24][0], 4)
        self.hook(STUB_DRAIN, lambda e: e.power[e.arg(0) - 0x24][1], 4)
        for house in (HOUSE, ENEMY):
            self.write32(house + 0x24, IHOUSE_VTABLE)

    def cost_stub(self, e):
        if e.arg(0) != HOUSE:
            raise OracleError('Cost_Of for another house')
        return e.cost[e.uc.reg_read(UC_X86_REG_ECX)]

    def list(self, values):
        """A fixture array of dwords; returns its address."""
        address = self.list_top
        for slot, value in enumerate(values):
            self.write32(address + 4 * slot, value)
        self.list_top += max(4 * len(values), 4) + 0x10
        return address

    def array(self, where, values):
        items, count = where
        self.write32(items, self.list(values))
        self.write32(count, len(values))

    def vector(self, address, values):
        """A DynamicVectorClass at `address` (+4 items, +0x10 count)."""
        self.write32(address + 4, self.list(values))
        self.write32(address + 8, len(values))
        self.write32(address + 0x10, len(values))

    def techno_type(self, slot, rtti, index, cost=0):
        ty = TYPES + slot * TYPE_SIZE
        self.write32(ty, TYPE_VTABLE)
        self.write32(ty + 0xDF8, index)
        self.rtti[ty] = rtti
        self.index[ty] = index
        self.cost[ty] = cost
        return ty

    def read64(self, address):
        return struct.unpack('<Q', bytes(self.uc.mem_read(address, 8)))[0]

    def rules_vector(self, offset, values):
        """A Rules per-difficulty vector whose items sit at `offset`."""
        self.write32(RULES + offset, self.list(values))


# ---------------------------------------------------------------- choosers

def chooser_row(kind, *, difficulty=1, fill=(50, 50, 50), money=1000, types, teams=(),
                objects=(), answers):
    """`types`: (can_build, cost) per class index; `teams`: dicts with
    created, reinforce, b77, b78, b79, ours and needed (class indexes, or
    ('other', index) for a type of another class); `objects`: (class index,
    recruitable)."""
    entry, field, rtti, type_array, object_array, type_field = CHOOSERS[kind]
    emu = TeamEmu()
    emu.write32(GAME_MODE, 1)
    emu.write32(HOUSE + 0x184, difficulty)
    for chosen in (0x5650, 0x5654, 0x5658):
        emu.write32(HOUSE + chosen, 0xFFFFFFFF)
    emu.rules_vector(0x13F4, fill)
    emu.money[HOUSE] = money
    if kind == 'unit':
        # The harvester branch closed: no HarvesterUnit= type, and no
        # AISlaveMinerNumber= room (0x4FEB7F's gatherer test).
        emu.write32(RULES + 0xB40, emu.list([]))
        emu.write32(RULES + 0xB4C, 0)
        emu.rules_vector(0x1340, [0, 0, 0])
        emu.hook(0x5117D0, lambda _e: 0, 0)
        emu.hook(0x5051E0, lambda _e: (_ for _ in ()).throw(
            OracleError('FirstBuildableFromArray asked with the branch closed')), 4)
    class_types = [emu.techno_type(index, rtti, index, cost)
                   for index, (_can, cost) in enumerate(types)]
    other_types = [emu.techno_type(20 + index, 0x7, index) for index in range(4)]
    emu.array(type_array, class_types)
    can_build = {ty: can for ty, (can, _cost) in zip(class_types, types)}

    def can_build_stub(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != HOUSE or e.arg(1) or e.arg(2):
            raise OracleError('CanBuild asked other than (house, type, 0, 0)')
        e.events.append(['can_build', class_types.index(e.arg(0))])
        return can_build[e.arg(0)]
    emu.hook(0x4F7870, can_build_stub, 12)

    team_objects = []
    needs = {}
    for slot, team in enumerate(teams):
        address = TEAMS + slot * 0x100
        team_type = TEAM_TYPES + slot * 0x100
        emu.write32(address, TEAM_VTABLE)
        emu.write32(address + 0x24, team_type)
        emu.write32(address + 0x2C, HOUSE if team.get('ours', True) else ENEMY)
        emu.write32(address + 0x50, team['created'])
        emu.uc.mem_write(address + 0x77, bytes([int(team.get('b77', False)),
                                                 int(team.get('b78', False)),
                                                 int(team.get('b79', False))]))
        emu.uc.mem_write(team_type + 0xAB, bytes([int(team.get('reinforce', False))]))
        needs[address] = [other_types[need[1]] if isinstance(need, tuple) else class_types[need]
                          for need in team['needed']]
        team_objects.append(address)
    emu.array(TEAM_ARRAY, team_objects)

    def needed_types(e):
        team = e.uc.reg_read(UC_X86_REG_ECX)
        vector = e.arg(0)
        e.events.append(['needed', team_objects.index(team)])
        e.write32(vector + 4, e.list(needs[team]))
        e.write32(vector + 8, len(needs[team]))
        e.uc.mem_write(vector + 0xD, b'\x00')
        e.write32(vector + 0x10, len(needs[team]))
    emu.hook(0x6EF4D0, needed_types, 4)

    free = {}
    object_addresses = []
    for slot, (index, recruitable) in enumerate(objects):
        address = OBJECTS + slot * OBJECT_SIZE
        emu.write32(address + type_field, class_types[index])
        free[address] = recruitable
        object_addresses.append(address)
    emu.array(object_array, object_addresses)

    def recruitable_stub(e):
        if e.arg(0) != HOUSE:
            raise OracleError('IsRecruitable for another house')
        address = e.uc.reg_read(UC_X86_REG_ECX)
        e.events.append(['recruitable', object_addresses.index(address)])
        return int(free[address])
    emu.hook(0x4DA230, recruitable_stub, 4)
    emu.draws(answers)
    emu.invoke(entry, ecx=HOUSE)
    return dict(kind=kind, difficulty=difficulty, fill=list(fill), money=money,
                types=[list(t) for t in types],
                teams=[dict(team, needed=[list(n) if isinstance(n, tuple) else n
                                          for n in team['needed']]) for team in teams],
                objects=[list(o) for o in objects], answers=list(answers),
                choice=emu.read_i32(HOUSE + field), events=emu.events)


def choosers():
    rows = []
    three = [(1, 100), (1, 100), (1, 100)]
    for kind in CHOOSERS:
        # No team wants anything: the first draw still happens.
        rows.append(chooser_row(kind, types=three, answers=[0]))
        rows.append(chooser_row(kind, types=three, answers=[0x7FFFFFFE]))
        teams = [dict(created=40, needed=[0, 1, 1]), dict(created=30, needed=[2, 1])]
        # The earliest team's type (fill% above the roll), or the most needed.
        for answers in ([0], [1073741823], [1073741824], [0x7FFFFFFE, 0]):
            for fill in ((50, 50, 50), (0, 0, 0), (100, 100, 100)):
                rows.append(chooser_row(kind, types=three, teams=teams, fill=fill,
                                        answers=answers + [0]))
        # Ties in need keep index order; a later, lower need still joins.
        tie = [dict(created=10, needed=[0, 0, 2, 2, 1])]
        for pick in (0, 1, 2):
            rows.append(chooser_row(kind, types=three, teams=tie, fill=(0, 0, 0),
                                    answers=[5, pick]))
        # Admission: CanBuild 0 (and -1, which passes), the money.
        for types in ([(0, 100), (1, 100), (-1, 100)], [(1, 1001), (1, 1000), (1, 999)],
                      [(0, 1), (0, 1), (0, 1)]):
            rows.append(chooser_row(kind, types=types, teams=teams, fill=(0, 0, 0),
                                    answers=[7, 0]))
            rows.append(chooser_row(kind, types=types, teams=teams, fill=(100, 100, 100),
                                    answers=[7, 0]))
        # Team filters: another house's, Reinforce= with +0x79, +0x77, +0x78.
        filtered = [dict(created=5, ours=False, needed=[0]),
                    dict(created=6, reinforce=True, b78=True, needed=[1]),
                    dict(created=7, reinforce=True, b79=True, b78=True, needed=[2]),
                    dict(created=8, b77=True, needed=[2]),
                    dict(created=9, b78=True, needed=[0]),
                    dict(created=11, needed=[('other', 1), 2])]
        for fill in ((0, 0, 0), (100, 100, 100)):
            rows.append(chooser_row(kind, types=three, teams=filtered, fill=fill,
                                    answers=[3, 0]))
        # Free objects cover needs: of a needed type, recruitable or not.
        objects = [(1, True), (1, False), (0, True), (1, True), (1, True)]
        for fill in ((0, 0, 0), (100, 100, 100)):
            rows.append(chooser_row(kind, types=three, teams=teams, objects=objects,
                                    fill=fill, answers=[3, 0]))
        # An object of a type whose need is met is not asked IsRecruitable
        # (0x004FED3F).
        objects = [(0, True), (0, True), (2, True), (2, False), (1, False)]
        rows.append(chooser_row(kind, types=three, teams=teams, objects=objects,
                                answers=[3, 0]))
        # The difficulty picks the fill percent.
        for difficulty in (0, 1, 2):
            rows.append(chooser_row(kind, types=three, teams=teams, difficulty=difficulty,
                                    fill=(100, 50, 0), answers=[1073741823, 1]))
    return rows


# ---------------------------------------------------------------- harvester

def harvester_row(*, harvester=True, owned=True, refinery=True, undeploys=True,
                  gatherers=0, destinations=1, per_refinery=2, slave_miners=3, iq=5,
                  iq_harvester=3, no_ore=False, human=False, control=False, game_mode=1,
                  harvester_tech=2, tech=10):
    emu = TeamEmu()
    emu.write32(GAME_MODE, game_mode)
    difficulty = 1
    emu.write32(HOUSE + 0x184, difficulty)
    for chosen in (0x5650, 0x5654, 0x5658):
        emu.write32(HOUSE + chosen, 0xFFFFFFFF)
    emu.rules_vector(0x13F4, [0, 0, 0])
    emu.money[HOUSE] = 0
    emu.array(UNIT_TYPES, [])
    emu.array(UNITS, [])
    emu.array(TEAM_ARRAY, [])
    country = 4
    emu.hook(0x5117D0, lambda _e: country, 0)
    types = []
    if harvester:
        ty = emu.techno_type(0, 0x28, 7)
        emu.write32(ty + 0x6CC, (1 << country) if owned else (1 << (country + 1)))
        emu.write32(ty + 0x634, harvester_tech)
        types.append(ty)
    emu.write32(RULES + 0xB40, emu.list(types))
    emu.write32(RULES + 0xB4C, len(types))
    undeploy = emu.techno_type(1, 0x28, 9)
    refinery_type = TYPES + 2 * TYPE_SIZE
    emu.write32(refinery_type + 0x408, undeploy if undeploys else 0)

    def first_buildable(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != HOUSE or e.arg(0) != RULES + 0x8E4:
            raise OracleError('FirstBuildableFromArray asked other than BuildRefinery=')
        e.events.append(['refinery'])
        return refinery_type if refinery else 0
    emu.hook(0x5051E0, first_buildable, 4)
    emu.rules_vector(0x135C, [0, per_refinery, 0])
    emu.rules_vector(0x1340, [0, slave_miners, 0])
    emu.write32(RULES + 0x1458, iq_harvester)
    emu.write32(HOUSE + 0x24C, iq)
    emu.uc.mem_write(HOUSE + 0x242, bytes([int(no_ore)]))
    emu.uc.mem_write(HOUSE + 0x1EC, bytes([int(human), int(control)]))
    emu.write32(HOUSE + 0x158, gatherers)
    emu.write32(HOUSE + 0x15C, destinations)
    emu.write32(HOUSE + 0x1D4, tech)
    emu.draws([0])
    emu.invoke(0x4FEA60, ecx=HOUSE)
    return dict(harvester=harvester, owned=owned, refinery=refinery, undeploys=undeploys,
                gatherers=gatherers, destinations=destinations, per_refinery=per_refinery,
                slave_miners=slave_miners, iq=iq, iq_harvester=iq_harvester, no_ore=no_ore,
                human=human, control=control, game_mode=game_mode,
                harvester_tech=harvester_tech, tech=tech,
                choice=emu.read_i32(HOUSE + 0x5650), events=emu.events)


def harvester():
    rows = [harvester_row()]
    for key, values in (('owned', [False]), ('refinery', [False]), ('undeploys', [False]),
                        ('gatherers', [1, 2, 3, 4, -1]), ('destinations', [0, 2, -1]),
                        ('per_refinery', [0, 0x40000000]), ('slave_miners', [0, 1, -5]),
                        ('iq', [2, 3]), ('no_ore', [True]), ('human', [True]),
                        ('control', [True]), ('harvester_tech', [10, 11, -1]),
                        ('tech', [-1, 1])):
        for value in values:
            rows.append(harvester_row(**{key: value}))
    for harvester_present in (False,):
        for refinery in (False, True):
            for undeploys in (False, True):
                for gatherers in (2, 3):
                    rows.append(harvester_row(harvester=harvester_present, refinery=refinery,
                                              undeploys=undeploys, gatherers=gatherers))
    rows.append(harvester_row(game_mode=0, control=True))
    rows.append(harvester_row(game_mode=0, human=True))
    rows.append(harvester_row(game_mode=0))
    rows.append(harvester_row(owned=False, gatherers=2, slave_miners=3))
    # HarvestersPerRefinery * destinations wraps (IMUL).
    for destinations in (2, 4):
        rows.append(harvester_row(per_refinery=0x40000000, destinations=destinations))
    return rows


# ---------------------------------------------------------------- selector

def prepare_selector(emu, *, ratio=100, active=True, difficulty=1, enemy=True,
                     cap=(8, 8, 8), max_defense=(2, 2, 2), team_types, teams=(),
                     triggers, eligible=None):
    """`team_types`: base-defense flags; `teams`: dicts with type, created,
    ours, formed; `triggers`: (first type, second type or None, weight);
    `eligible`: answers per trigger, or None to execute original eligibility.

    Shared fixture storage only; no draw or selector body runs here. Existing
    scalar controls and connected TAction controls use this same preparation.
    """
    emu.write32(HOUSE + 0x184, difficulty)
    emu.write32(HOUSE + 0x565C, ratio)
    emu.uc.mem_write(HOUSE + 0x1F2, bytes([int(active)]))
    emu.array(HOUSE_ARRAY, [HOUSE, ENEMY])
    emu.write32(HOUSE + 0x5600, 1 if enemy else 0xFFFFFFFF)
    emu.rules_vector(0x13CC, cap)
    emu.rules_vector(0x13B0, max_defense)
    type_objects = []
    for slot, base_defense in enumerate(team_types):
        address = TEAM_TYPES + slot * 0x100
        emu.uc.mem_write(address + 0xF6, bytes([int(base_defense)]))
        type_objects.append(address)
    team_objects = []
    for slot, team in enumerate(teams):
        address = TEAMS + slot * 0x100
        emu.write32(address, TEAM_VTABLE)
        emu.write32(address + 0x24, type_objects[team['type']])
        emu.write32(address + 0x2C, HOUSE if team.get('ours', True) else ENEMY)
        emu.write32(address + 0x50, team['created'])
        emu.uc.mem_write(address + 0x7F, bytes([int(team.get('formed', False))]))
        team_objects.append(address)
    emu.array(TEAM_ARRAY, team_objects)
    trigger_objects = []
    for slot, (first, second, weight) in enumerate(triggers):
        address = TRIGGERS + slot * 0x200
        emu.write32(address + 0xDC, type_objects[first] if first is not None else 0)
        emu.write32(address + 0xE0, type_objects[second] if second is not None else 0)
        emu.uc.mem_write(address + 0xB8, f64(weight))
        trigger_objects.append(address)
    emu.array(TRIGGER_ARRAY, trigger_objects)
    answers_by_trigger = dict(zip(trigger_objects, eligible or ()))

    def eligibility(e):
        trigger = e.uc.reg_read(UC_X86_REG_ECX)
        house, enemy_house, flag = e.arg(0), e.arg(1), e.arg(2) & 0xFF
        if house != HOUSE or enemy_house != (ENEMY if enemy else 0):
            raise OracleError('eligibility asked for another house')
        e.events.append(['eligible', trigger_objects.index(trigger), flag])
        return int(answers_by_trigger[trigger])
    if eligible is not None:
        emu.hook(0x41E720, eligibility, 12)

    def delete_team(e):
        team = e.uc.reg_read(UC_X86_REG_ECX)
        e.events.append(['destroy', team_objects.index(team)])
        # The destructor leaves TeamClass::Array (0x6E8DE0).
        items, count = e.read32(TEAM_ARRAY[0]), e.read32(TEAM_ARRAY[1])
        live = [e.read32(items + 4 * slot) for slot in range(count)]
        live.remove(team)
        for slot, value in enumerate(live):
            e.write32(items + 4 * slot, value)
        e.write32(TEAM_ARRAY[1], len(live))
    emu.hook(STUB_TEAM_DELETE, delete_team, 4)
    return type_objects


def selector_row(*, ratio=100, active=True, difficulty=1, enemy=True, cap=(8, 8, 8),
                 max_defense=(2, 2, 2), team_types, teams=(), triggers, eligible,
                 answers, name=''):
    emu = TeamEmu()
    type_objects = prepare_selector(emu, ratio=ratio, active=active,
        difficulty=difficulty, enemy=enemy, cap=cap, max_defense=max_defense,
        team_types=team_types, teams=teams, triggers=triggers, eligible=eligible)
    emu.draws(answers)
    out = FAKE + 0x230000
    emu.invoke(0x6F0AB0, ecx=out, edx=HOUSE, args=[0])
    items, count = emu.read32(out + 4), emu.read_i32(out + 0x10)
    picked = [type_objects.index(emu.read32(items + 4 * slot)) for slot in range(count)]
    autocreate = [emu.uc.mem_read(address + 0xA9, 1)[0] for address in type_objects]
    return dict(name=name, ratio=ratio, active=active, difficulty=difficulty, enemy=enemy,
                cap=list(cap), max_defense=list(max_defense), team_types=list(team_types),
                teams=[dict(team) for team in teams],
                triggers=[[first, second, f'{struct.unpack("<Q", f64(weight))[0]:016x}']
                          for first, second, weight in triggers],
                eligible=list(eligible), answers=list(answers), picked=picked,
                autocreate=autocreate, events=emu.events)


def selector():
    rows = []
    types = [False, False, True, False]
    triggers = [(0, None, 60.0), (1, 3, 40.0), (2, None, 25.5)]
    # The ratio draw, the activity latch.
    for ratio, roll in ((100, 100), (50, 50), (50, 51), (0, 1)):
        rows.append(selector_row(name='ratio', ratio=ratio, team_types=types,
                                 triggers=triggers, eligible=[True] * 3,
                                 answers=[roll, 1]))
    rows.append(selector_row(name='inactive', active=False, team_types=types,
                             triggers=triggers, eligible=[True] * 3, answers=[1]))
    # The weighted draw over running sums (unsigned).
    for roll in (1, 60, 61, 100, 101, 125):
        rows.append(selector_row(name='weights', team_types=types, triggers=triggers,
                                 eligible=[True] * 3, answers=[1, roll]))
    rows.append(selector_row(name='ineligible', team_types=types, triggers=triggers,
                             eligible=[False, True, False], answers=[1, 7]))
    rows.append(selector_row(name='none', team_types=types, triggers=triggers,
                             eligible=[False] * 3, answers=[1]))
    # ftol of the weights, and the 5000 tier.
    for weights in ((4999.9, 5000.0, 5000.7), (0.4, 0.6, 1.5), (5000.0, 3.0, 5000.0),
                    (-2.5, 4.0, 1.0), (3.0, -3.0, 0.0)):
        for roll in (1, 2, 3, 4, 5000, 5001, 10000):
            triggers_w = [(0, None, weights[0]), (1, 3, weights[1]), (2, None, weights[2])]
            rows.append(selector_row(name='ftol', team_types=types, triggers=triggers_w,
                                     eligible=[True] * 3, answers=[1, roll]))
    # A negative total draws among reversed bounds.
    rows.append(selector_row(name='negative', team_types=types,
                             triggers=[(0, None, -5.0), (1, None, 2.0)],
                             eligible=[True, True], answers=[1, -4]))
    # The team counts: the cap, the defense teams, the eviction.
    teams = [dict(type=0, created=10), dict(type=2, created=30), dict(type=2, created=20),
             dict(type=1, created=5, ours=False), dict(type=2, created=1, ours=False),
             dict(type=2, created=20)]
    for cap in (1, 3, 4, 5):
        for max_defense in (0, 2, 3):
            rows.append(selector_row(name='counts', cap=(cap, cap, cap),
                                     max_defense=(max_defense,) * 3, team_types=types,
                                     teams=teams, triggers=triggers, eligible=[True] * 3,
                                     answers=[1, 50]))
    # The cancel pass: a forming (or +0x7B) team of a picked type.
    for team in (dict(type=0, created=3), dict(type=0, created=3, formed=True),
                 dict(type=3, created=3), dict(type=0, created=3, ours=False)):
        for roll in (1, 70):
            rows.append(selector_row(name='cancel', team_types=types, teams=[team],
                                     triggers=triggers, eligible=[True] * 3,
                                     answers=[1, roll]))
    # No enemy, and the difficulty's vectors.
    rows.append(selector_row(name='no-enemy', enemy=False, team_types=types, triggers=triggers,
                             eligible=[True] * 3, answers=[1, 90]))
    for difficulty in (0, 1, 2):
        rows.append(selector_row(name='difficulty', difficulty=difficulty, cap=(1, 2, 3),
                                 max_defense=(0, 1, 2), team_types=types, teams=teams[:3],
                                 triggers=triggers, eligible=[True] * 3, answers=[1, 50]))
    return rows


# ---------------------------------------------------------------- eligibility

def eligibility_row(*, name, condition=-1, amount=0, comparator=0, enemy=True, flag=False,
                    base_defense=False, second=None, use_min_rule=False, defense_teams=0,
                    min_defense=(9, 2, 9), output=0, drain=0, money=0):
    """A multiplayer trigger every other gate admits: owner <all>, no side,
    TechLevel 0, a first TeamType without zone relation, an empty TaskForce
    and no Max=, and an optional second TeamType (`second`: its
    IsBaseDefense=)."""
    emu = TeamEmu()
    emu.write32(GAME_MODE, 1)
    emu.uc.mem_write(RULES + 0x17F3, bytes([int(use_min_rule)]))
    emu.rules_vector(0x1394, min_defense)
    emu.write32(HOUSE + 0x184, 1)
    emu.write32(HOUSE + 0x1D4, 10)
    emu.write32(HOUSE + 0x566C, defense_teams)
    emu.power[ENEMY] = (output, drain)
    emu.money[ENEMY] = money
    emu.array(TEAM_ARRAY, [])
    team_types = []
    for slot, defense in enumerate((base_defense, second)):
        if defense is None:
            team_types.append(0)
            continue
        address = TEAM_TYPES + slot * 0x100
        emu.uc.mem_write(address + 0xF6, bytes([int(defense)]))
        emu.write32(address + 0xE4, TASK_FORCE)
        emu.write32(address + 0xB8, 0xFFFFFFFF)
        team_types.append(address)
    emu.write32(TASK_FORCE + 0x9C, 0)
    trigger = TRIGGERS
    emu.write32(trigger + 0xDC, team_types[0])
    emu.write32(trigger + 0xE0, team_types[1])
    emu.write32(trigger + 0x98, condition)
    emu.write32(trigger + 0xA0, 2)
    emu.uc.mem_write(trigger + 0xA4, b'\x01')
    emu.uc.mem_write(trigger + 0xD0, b'\x01')
    emu.uc.mem_write(trigger + 0xD2, b'\x01\x01\x01')
    emu.write32(trigger + 0xE4, amount)
    emu.write32(trigger + 0xE8, comparator)
    result = emu.invoke(0x41E720, ecx=trigger,
                        args=[HOUSE, ENEMY if enemy else 0, int(flag)]) & 0xFF
    return dict(name=name, condition=condition, amount=amount, comparator=comparator,
                enemy=enemy, flag=flag, base_defense=base_defense, second=second,
                use_min_rule=use_min_rule, defense_teams=defense_teams,
                min_defense=list(min_defense), output=output, drain=drain, money=money,
                eligible=bool(result))


def eligibility():
    rows = []
    # The defense gate, at the house's MinimumAIDefensiveTeams= of 2.
    for enemy in (False, True):
        for use_min_rule in (False, True):
            for defense_teams in (0, 1, 2, 3):
                for base_defense, second in ((False, None), (True, None), (False, True),
                                             (False, False), (True, False)):
                    for flag in (False, True):
                        rows.append(eligibility_row(
                            name='defense', enemy=enemy, use_min_rule=use_min_rule,
                            defense_teams=defense_teams, base_defense=base_defense,
                            second=second, flag=flag))
    # The comparison, through the enemy's money (condition 4).
    for comparator in (0, 1, 2, 3, 4, 5, 6, -1):
        for money in (99, 100, 101, -0x80000000):
            rows.append(eligibility_row(name='compare', condition=4, amount=100,
                                        comparator=comparator, money=money))
    # The power conditions (the SUB wraps before FILD).
    for condition in (2, 3):
        for output, drain in ((200, 101), (200, 100), (200, 99), (100, 100), (99, 100),
                              (0, 1), (-0x80000000, 1), (0x7FFFFFFF, -1), (0, 0)):
            rows.append(eligibility_row(name='power', condition=condition, output=output,
                                        drain=drain))
    return rows


# ---------------------------------------------------------------- charge

def charge_row(start, duration, recharge, percent, frame=1000):
    """The house's one Super, an Iron Curtain, granted."""
    emu = TeamEmu()
    emu.write32(FRAME, frame)
    emu.uc.mem_write(RULES + 0xD70, struct.pack('<f', percent))
    address, sw_type = SUPERS, SW_TYPES
    emu.write32(address + 0x28, sw_type)
    emu.write32(sw_type + 0xB4, 1)
    emu.write32(sw_type + 0xB0, recharge)
    emu.uc.mem_write(address + 0x6D, b'\x01')
    emu.write32(address + 0x30, start)
    emu.write32(address + 0x38, duration)
    emu.write32(address + 0x24, 0xFFFFFFFF)
    emu.write32(HOUSE + 0x258, emu.list([address]))
    emu.write32(HOUSE + 0x264, 1)
    result = emu.invoke(0x41F0D0, args=[HOUSE, 0]) & 0xFF
    return dict(start=start, duration=duration, recharge=recharge,
                percent=struct.unpack('<I', struct.pack('<f', percent))[0], frame=frame,
                ready=bool(result))


def charge():
    rows = []
    for percent in (0.2, 0.25, 0.1, 0.0, 1.0, 0.3333333, -0.5):
        for start, duration in ((1000, 500), (900, 500), (700, 500), (600, 500), (-1, 100),
                                (-1, 101), (-1, 99), (-1, 0), (400, 500), (1001, 5)):
            for recharge in (500, 1, 3):
                rows.append(charge_row(start, duration, recharge, percent))
    # A zero recharge divides by zero: 0/0 and x/0 for x of either sign.
    for start, duration in ((-1, 0), (-1, 5), (-1, -5), (990, 5)):
        rows.append(charge_row(start, duration, 0, 0.2))
    return rows


# ---------------------------------------------------------------- team block

BLOCK_START, BLOCK_END = 0x4F8A00, 0x4F8B08


def team_block_row(*, start=0, duration=10, frame=10, game_mode=1, human=False, control=False,
                   passive=False, difficulty=1, delays=(300, 600, 900)):
    """The selector answers an empty vector."""
    emu = TeamEmu()
    emu.write32(GAME_MODE, game_mode)
    emu.write32(FRAME, frame)
    emu.write32(HOUSE + 0x5798, start)
    emu.write32(HOUSE + 0x57A0, duration)
    emu.uc.mem_write(HOUSE + 0x1EC, bytes([int(human), int(control)]))
    emu.write32(HOUSE + 0x34, HOUSE_TYPES)
    emu.uc.mem_write(HOUSE_TYPES + 0x1A6, bytes([int(passive)]))
    emu.write32(HOUSE + 0x184, difficulty)
    emu.rules_vector(0x115C, delays)

    def selector_stub(e):
        vector = e.uc.reg_read(UC_X86_REG_ECX)
        if e.uc.reg_read(UC_X86_REG_EDX) != HOUSE:
            raise OracleError('selector for another house')
        e.events.append(['select'])
        e.write32(vector, 0x7EA9C4)
        e.uc.mem_write(vector + 4, b'\x00' * 0x14)
        e.uc.mem_write(vector + 0xC, b'\x01')
        e.write32(vector + 0x14, 10)
        return vector
    emu.hook(0x6F0AB0, selector_stub, 4)
    emu.hook(0x6F09C0, lambda _e: (_ for _ in ()).throw(
        OracleError('Create_Team without a picked TeamType')), 4)
    uc = emu.uc
    uc.mem_write(BLOCK_STACK, b'\x00' * 0x1000)
    uc.reg_write(UC_X86_REG_ESP, BLOCK_STACK + 0x800)
    uc.reg_write(UC_X86_REG_ESI, HOUSE)
    uc.reg_write(UC_X86_REG_EBP, 0)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    run_checked(uc, BLOCK_START, BLOCK_END)
    return dict(start=start, duration=duration, frame=frame, game_mode=game_mode, human=human,
                control=control, passive=passive, difficulty=difficulty, delays=list(delays),
                timer=[emu.read_i32(HOUSE + 0x5798), emu.read_i32(HOUSE + 0x57A0)],
                selected=emu.events.count(['select']))


def team_block():
    rows = []
    for start, duration, frame in ((0, 10, 10), (0, 10, 9), (0, 10, 11), (-1, 0, 5),
                                   (-1, 3, 5), (-1, -3, 5), (5, 0, 5), (100, 10, 50),
                                   (0x7FFFFFF0, 0x20, -0x7FFFFFF0)):
        rows.append(team_block_row(start=start, duration=duration, frame=frame))
    for game_mode, human, control in ((1, True, False), (1, False, True), (0, False, True),
                                      (0, True, False), (0, False, False)):
        rows.append(team_block_row(game_mode=game_mode, human=human, control=control))
    rows.append(team_block_row(passive=True))
    for difficulty in (0, 1, 2):
        rows.append(team_block_row(difficulty=difficulty))
    return rows


# ---------------------------------------------------------------- feedback

REGISTER_SUCCESS, REGISTER_FAILURE = 0x41FD60, 0x41FE20
RETAIL_DELTAS = (20.0, -50.0)


def f64_bits(value):
    return struct.unpack('<Q', f64(value))[0]


def feedback_rules(emu, deltas, coefficient):
    emu.uc.mem_write(RULES + 0xC0, f64(deltas[0]))
    emu.uc.mem_write(RULES + 0xC8, f64(deltas[1]))
    emu.uc.mem_write(RULES + 0xD0, f64(coefficient))


def feedback_trigger(emu, address, weight, minimum, maximum, successes, attempts):
    emu.uc.mem_write(address + 0xB8, f64(weight))
    emu.uc.mem_write(address + 0xC0, f64(minimum))
    emu.uc.mem_write(address + 0xC8, f64(maximum))
    emu.write32(address + 0x104, successes)
    emu.write32(address + 0x108, attempts)


def feedback_state(emu, address):
    return dict(weight=emu.read64(address + 0xB8), successes=emu.read_i32(address + 0x104),
                attempts=emu.read_i32(address + 0x108))


def feedback_row(*, succeeded, weight, minimum, maximum, successes, attempts,
                 deltas=RETAIL_DELTAS, coefficient=1.0):
    """RegisterSuccess or RegisterFailure on one AI trigger."""
    emu = TeamEmu()
    feedback_rules(emu, deltas, coefficient)
    feedback_trigger(emu, TRIGGERS, weight, minimum, maximum, successes, attempts)
    emu.invoke(REGISTER_SUCCESS if succeeded else REGISTER_FAILURE, ecx=TRIGGERS)
    return dict(succeeded=succeeded, weight=f64_bits(weight), minimum=f64_bits(minimum),
                maximum=f64_bits(maximum), successes=successes, attempts=attempts,
                success_delta=f64_bits(deltas[0]), failure_delta=f64_bits(deltas[1]),
                coefficient=f64_bits(coefficient), result=feedback_state(emu, TRIGGERS))


def feedback():
    rows = []
    records = ((0, 0), (0, 1), (1, 1), (1, 2), (1, 3), (2, 3), (3, 7), (5, 5), (0, 4),
               (7, 3), (1, -1), (0, -5), (333, 1000), (1, 7))
    bounds = ((40.0, 10.0, 70.0), (10.0, 10.0, 70.0), (70.0, 10.0, 70.0),
              (69.9, 10.0, 70.0), (5000.0, 10.0, 5000.0), (0.0, 0.0, 0.0),
              (37.5, 40.0, 40.0), (55.0, 70.0, 10.0))
    for succeeded in (True, False):
        for successes, attempts in records:
            for weight, minimum, maximum in bounds:
                rows.append(feedback_row(succeeded=succeeded, weight=weight, minimum=minimum,
                                         maximum=maximum, successes=successes,
                                         attempts=attempts))
    for deltas, coefficient in (((5.0, -20.0), 1.0), ((0.1, -0.3), 0.5), ((20.0, -50.0), 2.0),
                                ((1.0 / 3.0, -2.0 / 3.0), 0.3333333),
                                ((20.0, -50.0), -1.0)):
        for succeeded in (True, False):
            for successes, attempts in ((1, 3), (2, 3), (0, 4), (4, 4)):
                rows.append(feedback_row(succeeded=succeeded, weight=41.1, minimum=10.0,
                                         maximum=70.0, successes=successes,
                                         attempts=attempts, deltas=deltas,
                                         coefficient=coefficient))
    return rows


# ---------------------------------------------------------------- destructor loop

DESTRUCTOR_LOOP, DESTRUCTOR_LOOP_END = 0x6E8E02, 0x6E8E40


def destructor_row(*, succeeded, triggers):
    """~TeamClass's AI trigger loop for a team of TeamType 0; `triggers` are
    (first TeamType slot, weight, successes, attempts), bounds 10..70."""
    emu = TeamEmu()
    feedback_rules(emu, RETAIL_DELTAS, 1.0)
    team = TEAMS
    emu.write32(team + 0x24, TEAM_TYPES)
    emu.uc.mem_write(team + 0x84, bytes([int(succeeded)]))
    addresses = []
    for slot, (team_type, weight, successes, attempts) in enumerate(triggers):
        address = TRIGGERS + slot * 0x200
        emu.write32(address + 0xDC, TEAM_TYPES + team_type * 0x100)
        feedback_trigger(emu, address, weight, 10.0, 70.0, successes, attempts)
        addresses.append(address)
    emu.array(TRIGGER_ARRAY, addresses)
    uc = emu.uc
    uc.mem_write(BLOCK_STACK, b'\x00' * 0x1000)
    uc.reg_write(UC_X86_REG_ESP, BLOCK_STACK + 0x800)
    uc.reg_write(UC_X86_REG_ESI, team)
    uc.reg_write(UC_X86_REG_EDI, 0)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    run_checked(uc, DESTRUCTOR_LOOP, DESTRUCTOR_LOOP_END)
    return dict(succeeded=succeeded,
                triggers=[dict(team_type=team_type, weight=f64_bits(weight),
                               successes=successes, attempts=attempts)
                          for team_type, weight, successes, attempts in triggers],
                results=[feedback_state(emu, address) for address in addresses])


def destructor():
    rows = []
    mixes = ([(0, 40.0, 1, 3)], [(1, 40.0, 1, 3)],
             [(0, 40.0, 0, 0), (1, 50.0, 2, 2), (0, 65.0, 5, 6), (0, 10.0, 0, 9)],
             [(2, 30.0, 1, 1), (0, 69.0, 3, 3), (0, 69.0, 3, 3)], [])
    for succeeded in (True, False):
        for triggers in mixes:
            rows.append(destructor_row(succeeded=succeeded, triggers=triggers))
    return rows


# ---------------------------------------------------------------- dissolve

DISSOLVE_CHECK = 0x6E929B
DISSOLVE_ENDS = {0x6E9329: 'members', 0x6E9325: 'keep', 0x6E92D0: 'dissolve'}


def dissolve_row(*, members=False, at_strength=False, game_mode=1, frame, created, delay=5000):
    """TeamClass::AI's empty-team test, after the recruit loop."""
    emu = TeamEmu()
    emu.write32(GAME_MODE, game_mode)
    emu.write32(FRAME, frame)
    emu.write32(RULES + 0x1190, delay)
    team = TEAMS
    emu.write32(team + 0x54, OBJECTS if members else 0)
    emu.uc.mem_write(team + 0x78, bytes([int(at_strength)]))
    emu.write32(team + 0x50, created)
    uc = emu.uc
    uc.mem_write(BLOCK_STACK, b'\x00' * 0x1000)
    uc.reg_write(UC_X86_REG_ESP, BLOCK_STACK + 0x800)
    uc.reg_write(UC_X86_REG_ESI, team)
    run_checked(uc, DISSOLVE_CHECK, tuple(DISSOLVE_ENDS))
    return dict(members=members, at_strength=at_strength, game_mode=game_mode, frame=frame,
                created=created, delay=delay,
                outcome=DISSOLVE_ENDS[uc.reg_read(UC_X86_REG_EIP)])


def dissolve():
    rows = []
    for frame, created, delay in ((5001, 0, 5000), (5000, 0, 5000), (4999, 0, 5000),
                                  (7000, 2000, 5000), (7001, 2000, 5000), (0, 0, 0), (1, 0, 0),
                                  (0, 1, 0), (10, 0, -1), (-5, 0, -10),
                                  (-0x7FFFFFF0, 0x7FFFFFF0, 5000),
                                  (0x7FFFFFF0, -0x7FFFFFF0, 5000),
                                  (0x7FFFFFFF, -0x80000000, 5000)):
        for game_mode in (1, 0):
            rows.append(dissolve_row(game_mode=game_mode, frame=frame, created=created,
                                     delay=delay))
    for game_mode in (1, 0):
        rows.append(dissolve_row(at_strength=True, game_mode=game_mode, frame=10, created=0))
        rows.append(dissolve_row(members=True, game_mode=game_mode, frame=9000, created=0))
        rows.append(dissolve_row(members=True, at_strength=True, game_mode=game_mode,
                                 frame=10, created=0))
    return rows


def generate():
    return dict(choosers=choosers(), harvester=harvester(), selector=selector(),
                eligibility=eligibility(), charge=charge(), team_block=team_block(),
                feedback=feedback(), destructor=destructor(), dissolve=dissolve())


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope=('The computer\'s unit choosers over synthetic tallies, admissions and draws; the '
               'Unit chooser\'s harvester branch; the AI trigger team selector over team '
               'counts, weights, eligibility answers and draws; the AI trigger eligibility '
               'test without zone relation or TaskForce entries; the Iron Curtain readiness; '
               'the house update\'s team block; the AI trigger weight feedback, the team '
               'destructor\'s trigger loop and the empty-team dissolve test.'),
        assumptions=['x87 control word 0x0E7F (PC53, chop), the harness default.',
                     'Fixture objects carry only the fields the entries read.'],
        substitutions=['Get_Needed_Types 0x6EF4D0, IsRecruitable 0x4DA230, CanBuild 0x4F7870, '
                       'Cost_Of vt+0x84, Available_Money and Power_Output/Drain (IHouse '
                       '+0x18/+0x20/+0x24), FindIndexOfName 0x5117D0, FirstBuildableFromArray '
                       '0x5051E0 and SideClass::Find_Index 0x6A46D0 answer from the case.',
                       'The selector\'s eligibility test 0x41E720 answers from the case; the '
                       'team destructor (vt+0x20) only leaves TeamClass::Array.',
                       'The team block\'s selector 0x6F0AB0 and Create_Team 0x6F09C0 are '
                       'stubs that record their calls.',
                       'RandomRanged 0x65C7E0 answers from the case and records its range.'],
        entry_points={'choose_unit': 0x4FEA60, 'choose_infantry': 0x4FEEE0,
                      'choose_aircraft': 0x4FF210, 'selector': 0x6F0AB0,
                      'eligibility': 0x41E720, 'iron_curtain_ready': 0x41F0D0,
                      'team_block': BLOCK_START, 'register_success': REGISTER_SUCCESS,
                      'register_failure': REGISTER_FAILURE,
                      'destructor_trigger_loop': DESTRUCTOR_LOOP,
                      'dissolve_check': DISSOLVE_CHECK},
    ))
