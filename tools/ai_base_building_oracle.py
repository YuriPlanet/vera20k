"""Native references for a computer house's base building.

Run python -m tools.ai_base_building_oracle --check (or explicit --write).
Rust consumers: src/sim/ai_base_building_tests.rs and
src/sim/ai_base_site_tests.rs.

Sections, each executed in a fresh emulator per case:
- economy: HouseClass::AI_EconomyStateMachine 0x509700 (mode step, the
  RandomRanged(0,1) draw at 0x509863 and its stream).
- chooser: HouseClass::AI_Choose_Building 0x4FE3E0 with the native node
  vector (vtable 0x7E38B0), the native advanced-plant admission 0x505360 and
  the native walls 0x50C340.
- walls: HouseClass::AI_BuildWalls 0x50C340 alone, over every foundation of
  the native Width/Height tables and the wall type, search and insert cases.
- placement_delay: BuildingClass::Factory_AI's retry wait 0x4501CB..0x4501E7.
- key / sort / direction: the ordinary key 0x505F80, the retail qsort
  0x7C8B48 with comparator 0x5108F0, and the away-from-base direction block
  0x5065E6..0x50664C (atan2 wrapper 0x4CAE30, ftol 0x7C5F00).
- reserved_near: HouseClass 0x50B760 around one reserved cell.
- site: HouseClass::FindBaseBuildingSite 0x5060B0 with the ordinary key on
  synthetic maps; the transcript records every cell lookup, occupancy test
  and placement test with the answer the model gave.

Substitutions are listed in the provenance sidecar. Nothing here executes
Find_Node's cell/building queries, Get_Node_Building 0x42E820,
ChooseNextProduction, the perimeter scan, CellRect::CheckOccupancy or
CanPlaceAt; their answers are supplied and recorded.
"""
from pathlib import Path
import random
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_ECX,
                               UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI,
                               UC_X86_REG_ESP, UC_X86_REG_FPCW)

from tools.native_oracle import (NATIVE_FPCW, OracleError, RET_MAGIC, STACK_BASE,
                                 STACK_SIZE, finish_vectors, load_image, provenance,
                                 run_checked)

HEAP, HEAP_SIZE = 0x40000000, 0x00400000
FAKE, FAKE_SIZE = 0x50000000, 0x00800000
STUBS = 0x60000000

HOUSE = FAKE
HOUSE_TYPE = FAKE + 0x2F000
NODE_BUILDINGS = FAKE + 0x80000
RULES = FAKE + 0x20000
SCENARIO = FAKE + 0x22000
MONEY_VTABLE = FAKE + 0x23000
TYPE_VTABLE = FAKE + 0x23400
LISTS = FAKE + 0x24000
TYPES = FAKE + 0x30000
TYPE_SIZE = 0x2000
CELLS = FAKE + 0x100000
CELL_SIZE = 0x150
DUMMY_CELL = FAKE + 0x300000
MAP_SIZE = 64

GAME_MODE = 0xA8B238
RULES_PTR = 0x8871E0
SCENARIO_PTR = 0xA8B230
FRAME = 0xA8ED84
BUILDING_TYPES = 0xA83C6C
MAP = 0x87F7E8

STUB_MONEY = STUBS + 0x10
STUB_CAN_PLACE = STUBS + 0x20

NODE_VTABLE = 0x7E38B0
GROUP_VTABLE = 0x7ED90C


def u32(value):
    return struct.pack('<I', value & 0xFFFFFFFF)


def i16s(*values):
    return struct.pack('<' + 'h' * len(values), *values)


class Emu:
    """One fresh original image with fixture memory and call stubs."""

    def __init__(self):
        uc = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(uc)
        uc.mem_map(STACK_BASE, STACK_SIZE)
        uc.mem_map(RET_MAGIC, 0x1000)
        uc.mem_map(HEAP, HEAP_SIZE)
        uc.mem_map(FAKE, FAKE_SIZE)
        uc.mem_map(STUBS, 0x1000)
        # An unhooked stub traps instead of running on.
        uc.mem_write(STUBS, b'\xcc' * 0x1000)
        self.uc = uc
        self.heap = HEAP
        self.events = []
        self.write32(RULES_PTR, RULES)
        self.write32(SCENARIO_PTR, SCENARIO)
        self.hook(0x7C8E17, self.operator_new, 0)
        self.hook(0x7C8B3D, lambda _e: 0, 0)
        self.hook(0x7C978A, lambda _e: 0, 0)
        # The CRT static initializer of the adjacent-cell table 0x89F688
        # (listed at 0x812B90) that the neighbour steps read.
        self.invoke(0x49F2F0)

    def read32(self, address):
        return struct.unpack('<I', self.uc.mem_read(address, 4))[0]

    def read_i32(self, address):
        return struct.unpack('<i', self.uc.mem_read(address, 4))[0]

    def write32(self, address, value):
        self.uc.mem_write(address, u32(value))

    def arg(self, index):
        """Stack argument `index` at a stub's entry."""
        return self.read32(self.uc.reg_read(UC_X86_REG_ESP) + 4 + 4 * index)

    def hook(self, address, answer, pops):
        """Replace the call at `address`: `answer(emu)` gives EAX (or None)
        and the stub returns, popping `pops` argument bytes."""
        def stub(uc, _address, _size, _data):
            sp = uc.reg_read(UC_X86_REG_ESP)
            back = self.read32(sp)
            eax = answer(self)
            if eax is not None:
                uc.reg_write(UC_X86_REG_EAX, eax & 0xFFFFFFFF)
            uc.reg_write(UC_X86_REG_ESP, sp + 4 + pops)
            uc.reg_write(UC_X86_REG_EIP, back)
        self.uc.hook_add(UC_HOOK_CODE, stub, begin=address, end=address)

    def mark(self, address, event):
        self.uc.hook_add(UC_HOOK_CODE, lambda *_: self.events.append(event),
                         begin=address, end=address)

    def operator_new(self, _emu):
        size = self.arg(0)
        address = self.heap
        self.heap += (size + 15) & ~15
        if self.heap > HEAP + HEAP_SIZE:
            raise OracleError('fixture heap exhausted')
        return address

    def invoke(self, entry, ecx=None, edx=None, args=(), required=()):
        uc = self.uc
        sp = STACK_BASE + STACK_SIZE - 0x1000
        for value in reversed(args):
            sp -= 4
            uc.mem_write(sp, u32(value))
        sp -= 4
        uc.mem_write(sp, u32(RET_MAGIC))
        uc.reg_write(UC_X86_REG_ESP, sp)
        uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        if ecx is not None:
            uc.reg_write(UC_X86_REG_ECX, ecx)
        if edx is not None:
            uc.reg_write(UC_X86_REG_EDX, edx)
        run_checked(uc, entry, RET_MAGIC, required_addresses=required)
        if uc.reg_read(UC_X86_REG_ESP) != sp + 4 + 4 * len(args) and entry not in CALLER_CLEANS:
            raise OracleError(f'0x{entry:08X} returned with an unexpected stack')
        return uc.reg_read(UC_X86_REG_EAX)

    def draws(self, answers):
        """RandomRanged 0x65C7E0 (thiscall, RET 8): record the stream and
        range, answer from `answers` in order."""
        pending = list(answers)

        def draw(emu):
            stream = emu.uc.reg_read(UC_X86_REG_ECX) - emu.read32(SCENARIO_PTR)
            low, high = emu.arg(0), emu.arg(1)
            if not pending:
                raise OracleError('more draws than answers')
            value = pending.pop(0)
            emu.events.append(['draw', stream, low, high, value])
            return value
        self.hook(0x65C7E0, draw, 8)


CALLER_CLEANS = {0x7C8B48}


# ---------------------------------------------------------------- economy

CUTOFF = 2000
BARRACKS = ('BARRA', 'BARRB')
WEAPONS = ('WEAPA', 'WEAPB')


def economy_row(*, game_mode=1, human=False, control=False, mode, kind, money,
                barracks=True, weapons=True, output=100, drain=50, answer=0):
    emu = Emu()
    emu.write32(GAME_MODE, game_mode)
    emu.uc.mem_write(HOUSE + 0x1EC, bytes([int(human), int(control)]))
    emu.write32(HOUSE + 0x1E4, mode)
    emu.write32(HOUSE + 0x24, MONEY_VTABLE)
    emu.write32(MONEY_VTABLE + 0x18, STUB_MONEY)
    emu.write32(HOUSE + 0x53A4, output)
    emu.write32(HOUSE + 0x53A8, drain)
    emu.write32(RULES + 0x1300, CUTOFF)
    owned = set()
    for base, first, names, has in ((0x904, 20, BARRACKS, barracks),
                                     (0x93C, 30, WEAPONS, weapons)):
        items = LISTS + first * 4
        emu.write32(RULES + base, items)
        emu.write32(RULES + base + 0xC, len(names))
        for slot, _name in enumerate(names):
            index = first + slot
            ty = TYPES + index * TYPE_SIZE
            emu.write32(items + 4 * slot, ty)
            emu.write32(ty + 0xDF8, index)
            # The house owns the list's second type, so the scan passes one
            # unowned type first.
            if has and slot == 1:
                owned.add(index)

    def money_stub(e):
        if e.arg(0) != HOUSE + 0x24:
            raise OracleError('money interface called on the wrong object')
        return money

    def count_stub(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != HOUSE + 0x5550:
            raise OracleError('GetItemCount on the wrong counter')
        return 1 if e.arg(0) in owned else 0

    emu.hook(STUB_MONEY, money_stub, 4)
    emu.hook(0x49FAE0, count_stub, 4)
    emu.draws([answer])
    emu.invoke(0x509700, ecx=HOUSE, args=[kind])
    return dict(game_mode=game_mode, human=human, control=control, mode=mode, kind=kind,
                money=money, cutoff=CUTOFF, barracks=barracks, weapons=weapons,
                output=output, drain=drain, answer=answer,
                mode_after=emu.read_i32(HOUSE + 0x1E4), draws=emu.events)


def economy():
    rows = []
    for game_mode, human, control in ((1, True, False), (0, False, False),
                                      (0, False, True), (0, True, False)):
        for mode in (0, 1, 2):
            rows.append(economy_row(game_mode=game_mode, human=human, control=control,
                                    mode=mode, kind=6, money=CUTOFF - 1))
    for mode in (-1, 0, 1, 2, 3, 4):
        for kind in (6, 1):
            for money in (CUTOFF - 1, CUTOFF, CUTOFF + 1):
                if mode == 2 and money < CUTOFF:
                    continue
                rows.append(economy_row(mode=mode, kind=kind, money=money))
    for kind in (6, 1):
        for barracks in (False, True):
            for weapons in (False, True):
                for output, drain in ((100, 50), (100, 100), (50, 100)):
                    for answer in (0, 1):
                        rows.append(economy_row(mode=2, kind=kind, money=CUTOFF - 1,
                                                barracks=barracks, weapons=weapons,
                                                output=output, drain=drain, answer=answer))
    return rows


# ---------------------------------------------------------------- chooser

# The chooser's BuildingTypes, by array index.
CHOOSER_TYPES = [
    # name, naval, drain (+0xEE4)
    ('GPOWER', False, 0),
    ('NPOWER', False, 0),
    ('NAPOWER', False, 0),
    ('TPOWER', False, 0),
    ('YARD', False, 50),
    ('DRAINER', False, 50),
    ('WALLTOWER', False, 0),
    ('DOCK', True, 0),
    ('PLAIN', False, 0),
]
TYPE_INDEX = {name: index for index, (name, _, _) in enumerate(CHOOSER_TYPES)}
# The walls' types, after the economy's (13 and 14 in the Rust fixture): a
# wall of every side and a 2x2 ProtectWithWall= building.
CHOOSER_WALL = 13
CHOOSER_GUARDED = 14
NODE_ITEMS = HEAP + 0x300000
NODE_CAPACITY = 32
THRESHOLDS = (30, 50, 70)


def write_nodes(emu, nodes, capacity=NODE_CAPACITY):
    """Nodes are (type, x, y) or (type, x, y, filled)."""
    base = HOUSE + 0x5704
    emu.write32(base, NODE_VTABLE)
    emu.write32(base + 4, NODE_ITEMS)
    emu.write32(base + 8, capacity)
    emu.uc.mem_write(base + 0xC, bytes([1, 1]))
    emu.write32(base + 0x10, len(nodes))
    emu.write32(base + 0x14, 10)
    for slot, (ty, x, y, *filled) in enumerate(nodes):
        emu.uc.mem_write(NODE_ITEMS + 16 * slot,
                         struct.pack('<ihhII', ty, x, y, int(bool(filled and filled[0])), 0))


def node_buildings_hook(emu, objects):
    """Get_Node_Building 0x42E820 (BaseClass thiscall, RET 4) answers the
    building object placed for a node index, or none."""
    def stub(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != HOUSE + 0x5700:
            raise OracleError('Get_Node_Building on the wrong base')
        which = e.read_i32(e.uc.reg_read(UC_X86_REG_ESP) + 4)
        e.events.append(['building', which])
        return objects.get(which, 0)
    emu.hook(0x42E820, stub, 4)


def place_node_buildings(emu, nodes, node_buildings):
    """A building object of each listed node's type at its Location
    (+0x9C, leptons); returns them by node index."""
    objects = {}
    for slot, (node, (x, y)) in enumerate(sorted(node_buildings.items())):
        building = NODE_BUILDINGS + slot * 0x800
        emu.write32(building + 0x520, TYPES + nodes[node][0] * TYPE_SIZE)
        emu.write32(building + 0x9C, x)
        emu.write32(building + 0xA0, y)
        emu.write32(building + 0xA4, 0)
        objects[node] = building
    return objects


def at(x, y, dx=128, dy=128):
    """The Location of a building whose top-left cell is (x, y)."""
    return (x * 256 + dx, y * 256 + dy)


def read_nodes(emu):
    count = emu.read_i32(HOUSE + 0x5714)
    items = emu.read32(HOUSE + 0x5708)
    nodes = []
    for slot in range(count):
        ty, x, y, filled, retry = struct.unpack('<ihhBxxxi', emu.uc.mem_read(items + 16 * slot, 16))
        nodes.append([ty, x, y, filled, retry])
    return nodes


def chooser_row(label, nodes, *, choice=-1, yards=1, naval_allowed=True, difficulty=1,
                output=100, drain=0, side=0, blackout=0, drained_source=False,
                advanced_prereqs=(), buildings=0, draw=0, node_buildings=None):
    emu = Emu()
    emu.write32(GAME_MODE, 1)
    emu.write32(FRAME, 1000)
    emu.write32(HOUSE + 0x564C, choice)
    emu.write32(HOUSE + 0x60, yards)
    emu.uc.mem_write(HOUSE + 0x1F0, bytes([int(naval_allowed)]))
    emu.write32(HOUSE + 0x184, difficulty)
    emu.write32(HOUSE + 0x53A4, output)
    emu.write32(HOUSE + 0x53A8, drain)
    # Power blackout CDTimer: started 10 frames ago with `blackout + 10` left.
    emu.write32(HOUSE + 0x2A4, 990 if blackout else 0xFFFFFFFF)
    emu.write32(HOUSE + 0x2AC, blackout + 10 if blackout else 0)
    emu.uc.mem_write(HOUSE + 0x577B, bytes([int(drained_source)]))
    emu.write32(HOUSE + 0x1E8, side)
    # The house's BuildingClass vector (+0x6C items, +0x78 count): objects
    # whose type is the advanced plant's first prerequisite.
    objects = LISTS + 0x800
    emu.write32(HOUSE + 0x6C, objects)
    emu.write32(HOUSE + 0x78, buildings)
    type_items = LISTS
    emu.write32(BUILDING_TYPES, type_items)
    for index, (_name, naval, power_drain) in enumerate(CHOOSER_TYPES):
        ty = TYPES + index * TYPE_SIZE
        emu.write32(type_items + 4 * index, ty)
        emu.write32(ty + 0xDF8, index)
        emu.uc.mem_write(ty + 0xCCE, bytes([int(naval)]))
        emu.write32(ty + 0xEE4, power_drain)
        emu.write32(ty + 0x16FC, 0xFFFFFFFF)
        emu.uc.mem_write(ty + 0xE88, b'\0')
    advanced = TYPES + TYPE_INDEX['NAPOWER'] * TYPE_SIZE
    prereqs = LISTS + 0x400
    emu.write32(advanced + 0x63C, prereqs)
    emu.write32(advanced + 0x648, len(advanced_prereqs))
    for slot, prereq in enumerate(advanced_prereqs):
        emu.write32(prereqs + 4 * slot, prereq)
    # Every generic prerequisite group (TypeList vtable 0x7ED90C) holds the
    # yard type, which the house's buildings are of.
    for group in (0x8C8, 0x938, 0x900, 0xA34, 0x91C, 0x8E4):
        items = LISTS + 0x600 + group
        emu.write32(RULES + group, GROUP_VTABLE)
        emu.write32(RULES + group + 4, items)
        emu.write32(RULES + group + 0x10, 1)
        emu.write32(items, TYPES + TYPE_INDEX['YARD'] * TYPE_SIZE)
    for slot in range(buildings):
        building = FAKE + 0x28000 + slot * 0x800
        emu.write32(objects + 4 * slot, building)
        emu.write32(building + 0x520, TYPES + TYPE_INDEX['YARD'] * TYPE_SIZE)
    emu.write32(RULES + 0x87C, TYPES + TYPE_INDEX['WALLTOWER'] * TYPE_SIZE)
    thresholds = LISTS + 0x300
    emu.write32(RULES + 0xDD8, thresholds)
    for slot, value in enumerate(THRESHOLDS):
        emu.write32(thresholds + 4 * slot, value)
    build_const = LISTS + 0x380
    emu.write32(RULES + 0x8B0, build_const)
    emu.write32(RULES + 0x8BC, 1)
    emu.write32(build_const, TYPES + TYPE_INDEX['YARD'] * TYPE_SIZE)
    for offset, name in ((0x89C, 'GPOWER'), (0x8A0, 'NPOWER'), (0x8A4, 'NAPOWER'),
                         (0x8A8, 'TPOWER')):
        emu.write32(RULES + offset, TYPES + TYPE_INDEX[name] * TYPE_SIZE)
    emu.write32(HOUSE + 0x5748, 0)
    for index, side_value, protect in ((CHOOSER_WALL, -1, False), (CHOOSER_GUARDED, -1, True)):
        ty = TYPES + index * TYPE_SIZE
        emu.write32(type_items + 4 * index, ty)
        emu.write32(ty + 0xDF8, index)
        emu.write32(ty + 0x6D0, side_value)
        emu.uc.mem_write(ty + 0x1765, bytes([int(protect)]))
        emu.write32(ty + 0xEF0, 3)
        emu.write32(ty + 0x16FC, 0xFFFFFFFF)
    walls = LISTS + 0x1400
    emu.write32(RULES + 0xA54, walls)
    emu.write32(RULES + 0xA60, 1)
    emu.write32(walls, TYPES + CHOOSER_WALL * TYPE_SIZE)
    emu.write32(HOUSE + 0x34, HOUSE_TYPE)
    emu.write32(HOUSE_TYPE + 0xBC, 0)
    write_nodes(emu, nodes)
    node_buildings = node_buildings or {}
    node_buildings_hook(emu, place_node_buildings(emu, nodes, node_buildings))

    def find_node(e):
        wanted = e.read_i32(e.uc.reg_read(UC_X86_REG_ESP) + 4)
        e.events.append(['find', wanted])
        if e.uc.reg_read(UC_X86_REG_ECX) != HOUSE + 0x5700:
            raise OracleError('Find_Node on the wrong base')
        items = e.read32(HOUSE + 0x5708)
        for slot in range(e.read_i32(HOUSE + 0x5714)):
            ty, _x, _y, filled = struct.unpack('<ihhB', e.uc.mem_read(items + 16 * slot, 9))
            if not filled and (wanted == -1 or ty == wanted):
                return items + 16 * slot
        return 0

    def record(name, pops_args):
        def stub(e):
            if e.uc.reg_read(UC_X86_REG_ECX) != HOUSE:
                raise OracleError(f'{name} called on the wrong house')
            e.events.append([name] + [e.read_i32(e.uc.reg_read(UC_X86_REG_ESP) + 4 + 4 * i)
                                      for i in range(pops_args)])
            return 0
        return stub

    emu.hook(0x42EB20, find_node, 4)
    emu.mark(0x50C340, ['walls'])
    emu.hook(0x506EF0, record('choose_next_production', 1), 8)
    emu.hook(0x5082C0, record('perimeter', 0), 0)
    emu.draws([draw])
    result = emu.invoke(0x4FE3E0, ecx=HOUSE)
    return dict(label=label, nodes=[list(node) for node in nodes], choice=choice,
                yards=yards, naval_allowed=naval_allowed, difficulty=difficulty,
                threshold=THRESHOLDS[difficulty], output=output, drain=drain, side=side,
                blackout=blackout, drained_source=drained_source,
                advanced_prereqs=list(advanced_prereqs), buildings=buildings, draw=draw,
                node_buildings=[[node, x, y] for node, (x, y) in sorted(node_buildings.items())],
                result=result, choice_after=emu.read_i32(HOUSE + 0x564C),
                nodes_after=read_nodes(emu), events=emu.events)


def chooser():
    t = TYPE_INDEX
    rows = []
    for draw in (0, THRESHOLDS[1] - 1, THRESHOLDS[1], 99):
        rows.append(chooser_row('defense sentinel', [(-1, 0, 0), (t['PLAIN'], 0, 0)], draw=draw))
        rows.append(chooser_row('wall tower without cell',
                                [(t['WALLTOWER'], 0, 0), (-1, 0, 0), (t['PLAIN'], 0, 0)],
                                draw=draw))
    rows.append(chooser_row('wall tower, last node', [(t['WALLTOWER'], 0, 0)]))
    rows.append(chooser_row('wall tower with cell', [(t['WALLTOWER'], 12, 0), (t['PLAIN'], 0, 0)]))
    rows.append(chooser_row('wall tower with a y only', [(t['WALLTOWER'], 0, 7)]))
    rows.append(chooser_row('end of plan', [(-2, 0, 0), (t['PLAIN'], 0, 0)]))
    rows.append(chooser_row('perimeter node', [(-3, 0, 0), (t['PLAIN'], 0, 0)]))
    rows.append(chooser_row('plain', [(t['PLAIN'], 0, 0)]))
    rows.append(chooser_row('choice already made', [(t['PLAIN'], 0, 0)], choice=t['DOCK']))
    rows.append(chooser_row('no yard', [(t['PLAIN'], 0, 0)], yards=0))
    rows.append(chooser_row('no open node', []))
    rows.append(chooser_row('naval refused', [(t['DOCK'], 0, 0), (t['PLAIN'], 0, 0)],
                            naval_allowed=False))
    rows.append(chooser_row('naval refused, nothing after', [(t['DOCK'], 0, 0)],
                            naval_allowed=False))
    rows.append(chooser_row('naval refused, then a sentinel', [(t['DOCK'], 0, 0), (-1, 0, 0)],
                            naval_allowed=False, draw=99))
    rows.append(chooser_row('naval allowed', [(t['DOCK'], 0, 0)]))
    drainer = [(t['DRAINER'], 3, 4), (t['PLAIN'], 0, 0)]
    for side in (0, 1, 2, 3):
        rows.append(chooser_row(f'short of power, side {side}', drainer, output=100, drain=60,
                                side=side))
    rows.append(chooser_row('advanced plant without prerequisite', drainer, output=100,
                            drain=60, side=1))
    for prereqs, buildings in (((t['YARD'],), 0), ((t['YARD'],), 2), ((-1,), 2), ((-6,), 2)):
        rows.append(chooser_row('advanced plant with prerequisite', drainer, output=100,
                                drain=60, side=1, advanced_prereqs=prereqs,
                                buildings=buildings))
    rows.append(chooser_row('power exactly enough', drainer, output=100, drain=50))
    rows.append(chooser_row('build const type', [(t['YARD'], 0, 0)], output=100, drain=60))
    rows.append(chooser_row('blackout', drainer, output=100, drain=60, blackout=5))
    rows.append(chooser_row('drained source', drainer, output=100, drain=60,
                            drained_source=True))
    guarded = (CHOOSER_GUARDED, 10, 12, 1)
    for draw in (0, THRESHOLDS[1] - 1, THRESHOLDS[1], 99):
        rows.append(chooser_row('walls around a guarded building',
                                [guarded, (-1, 0, 0), (t['PLAIN'], 0, 0)],
                                node_buildings={0: at(10, 12)}, draw=draw))
    for difficulty in (0, 2):
        for draw in (THRESHOLDS[difficulty] - 1, THRESHOLDS[difficulty]):
            rows.append(chooser_row('walls at another difficulty',
                                    [guarded, (-1, 0, 0)], node_buildings={0: at(10, 12)},
                                    difficulty=difficulty, draw=draw))
    # The wall node lies away from every building, so Find_Node's replan
    # test (0x50CAD0, not modelled by the stub) would not take it either.
    rows.append(chooser_row('walls, the guarded building already walled',
                            [guarded, (CHOOSER_WALL, 20, 5, 1), (-1, 0, 0)],
                            node_buildings={0: at(10, 12)}))
    rows.append(chooser_row('walls, no building on the node', [guarded, (-1, 0, 0)]))
    rows.append(chooser_row('walls for a wall tower node',
                            [guarded, (t['WALLTOWER'], 0, 0), (-1, 0, 0), (t['PLAIN'], 0, 0)],
                            node_buildings={0: at(10, 12)}))
    return rows


# ---------------------------------------------------------------- walls

# The wall rows' BuildingTypes, by array index: name, AIBasePlanningSide
# (+0x6D0), ProtectWithWall (+0x1765), foundation (+0xEF0), Bib (+0x1570).
WALL_TYPES = ([('GAWALL', 0, False, 0, False), ('NAWALL', 1, False, 0, False),
               ('YAWALL', 2, False, 0, False), ('ANYWALL', -1, False, 0, False),
               ('PLAIN', -1, False, 3, False)]
              + [(f'GUARD{foundation}', -1, True, foundation, foundation % 2 == 1)
                 for foundation in range(22)])
WALL_INDEX = {name: index for index, (name, *_rest) in enumerate(WALL_TYPES)}
WALL_ORDER = ('GAWALL', 'NAWALL', 'YAWALL')
FOUNDATION_WIDTHS = 0x8192B8
FOUNDATION_HEIGHTS = 0x819310


def wall_types():
    """The rows' types with the native Width/Height tables' size of their
    foundation."""
    emu = Emu()
    return [[name, side, protect, foundation, bib,
             emu.read_i32(FOUNDATION_WIDTHS + 4 * foundation),
             emu.read_i32(FOUNDATION_HEIGHTS + 4 * foundation)]
            for name, side, protect, foundation, bib in WALL_TYPES]


def walls_row(label, nodes, index, *, side=0, walls=WALL_ORDER, node_buildings=None,
              capacity=NODE_CAPACITY):
    emu = Emu()
    type_items = LISTS
    emu.write32(BUILDING_TYPES, type_items)
    for type_index, (_name, plan_side, protect, foundation, bib) in enumerate(WALL_TYPES):
        ty = TYPES + type_index * TYPE_SIZE
        emu.write32(type_items + 4 * type_index, ty)
        emu.write32(ty + 0xDF8, type_index)
        emu.write32(ty + 0x6D0, plan_side)
        emu.uc.mem_write(ty + 0x1765, bytes([int(protect)]))
        emu.write32(ty + 0xEF0, foundation)
        emu.uc.mem_write(ty + 0x1570, bytes([int(bib)]))
    items = LISTS + 0x1400
    emu.write32(RULES + 0xA54, items)
    emu.write32(RULES + 0xA60, len(walls))
    for slot, name in enumerate(walls):
        emu.write32(items + 4 * slot, TYPES + WALL_INDEX[name] * TYPE_SIZE)
    emu.write32(HOUSE + 0x34, HOUSE_TYPE)
    emu.write32(HOUSE_TYPE + 0xBC, side)
    nodes = [(WALL_INDEX[ty] if isinstance(ty, str) else ty, x, y) for ty, x, y in nodes]
    write_nodes(emu, nodes, capacity)
    node_buildings = node_buildings or {}
    node_buildings_hook(emu, place_node_buildings(emu, nodes, node_buildings))
    result = emu.invoke(0x50C340, ecx=HOUSE, args=[index]) & 0xFF
    return dict(label=label, nodes=[list(node) for node in nodes], index=index, side=side,
                walls=[WALL_INDEX[name] for name in walls],
                node_buildings=[[node, x, y] for node, (x, y) in sorted(node_buildings.items())],
                capacity=capacity, result=result, nodes_after=read_nodes(emu),
                events=emu.events)


def walls():
    rows = [walls_row('no node before', [(-1, 0, 0)], 0),
            walls_row('index 0 with a guarded node', [('GUARD3', 20, 20), (-1, 0, 0)], 0,
                      node_buildings={0: at(20, 20)})]
    for foundation in range(22):
        rows.append(walls_row(f'foundation {foundation}',
                              [(f'GUARD{foundation}', 20, 20), (-1, 0, 0)], 1,
                              node_buildings={0: at(20, 20)}))
    fillers = [('PLAIN', 2 + 3 * slot, 40) for slot in range(10)]
    rows.append(walls_row('the vector grows',
                          fillers + [('GUARD20', 20, 20), (-1, 0, 0), ('PLAIN', 1, 1)], 11,
                          node_buildings={10: at(20, 20)}))
    rows.append(walls_row('a full vector grows',
                          fillers + [('GUARD3', 20, 20), (-1, 0, 0)], 11,
                          node_buildings={10: at(20, 20)}, capacity=12))
    rows.append(walls_row('not protected', [('PLAIN', 20, 20), (-1, 0, 0)], 1,
                          node_buildings={0: at(20, 20)}))
    rows.append(walls_row('no building on the node', [('GUARD3', 20, 20), (-1, 0, 0)], 1))
    already = [('GUARD3', 20, 20), ('GAWALL', 19, 19), (-1, 0, 0)]
    for side in (0, 1):
        rows.append(walls_row(f'next node a wall, side {side}', already, 2, side=side,
                              node_buildings={0: at(20, 20)}))
    rows.append(walls_row('the nearest guarded node',
                          [('GUARD3', 10, 10), ('PLAIN', 14, 14), ('GUARD5', 20, 20),
                           (-1, 0, 0)], 3,
                          node_buildings={0: at(10, 10), 1: at(14, 14), 2: at(20, 20)}))
    rows.append(walls_row('the nearest already walled, an earlier one',
                          [('GUARD3', 10, 10), ('PLAIN', 14, 14), ('GUARD5', 20, 20),
                           ('GAWALL', 19, 19), (-1, 0, 0)], 4,
                          node_buildings={0: at(10, 10), 1: at(14, 14), 2: at(20, 20)}))
    rows.append(walls_row('nodes after the sentinel stay',
                          [('GUARD3', 10, 10), (-1, 0, 0), ('PLAIN', 30, 30), (-2, 0, 0)], 1,
                          node_buildings={0: at(10, 10)}))
    for side in (0, 1, 2, 3, -1):
        rows.append(walls_row(f'side {side}', [('GUARD3', 20, 20), (-1, 0, 0)], 1, side=side,
                              node_buildings={0: at(20, 20)}))
    no_wall = [('GUARD3', 20, 20), ('PLAIN', 22, 22), (-1, 0, 0)]
    rows.append(walls_row('no wall for the side: the next node is -1',
                          [('GUARD3', 20, 20), (-1, 0, 0)], 1, side=3,
                          node_buildings={0: at(20, 20)}))
    rows.append(walls_row('no wall for the side: -1 walls', no_wall, 2, side=3,
                          node_buildings={0: at(20, 20)}))
    rows.append(walls_row('no walls listed', no_wall, 2, walls=(),
                          node_buildings={0: at(20, 20)}))
    for order in (('ANYWALL', 'GAWALL'), ('GAWALL', 'ANYWALL'), ('YAWALL', 'NAWALL'),
                  ('NAWALL', 'NAWALL', 'GAWALL')):
        for side in (0, 1):
            rows.append(walls_row(f'wall list {order}, side {side}',
                                  [('GUARD3', 20, 20), (-1, 0, 0)], 1, side=side,
                                  walls=order, node_buildings={0: at(20, 20)}))
    for cell in ((0, 0), (0, 7), (7, 0), (1, 1)):
        rows.append(walls_row(f'building at {cell}', [('GUARD4', *cell), (-1, 0, 0)], 1,
                              node_buildings={0: at(*cell)}))
    for offset in ((0, 0), (255, 255), (1, 254)):
        rows.append(walls_row(f'Location offset {offset}', [('GUARD3', 20, 20), (-1, 0, 0)], 1,
                              node_buildings={0: at(20, 20, *offset)}))
    generator = random.Random(0x50C340)
    names = [name for name, *_rest in WALL_TYPES]
    for case in range(30):
        count = generator.randrange(2, 12)
        nodes = []
        for _slot in range(count):
            roll = generator.random()
            if roll < 0.15:
                nodes.append((generator.choice((-1, -2)), 0, 0))
            else:
                nodes.append((generator.choice(names), generator.randrange(2, 60),
                              generator.randrange(2, 60)))
        index = generator.randrange(0, count)
        node_buildings = {slot: at(x, y, generator.randrange(256), generator.randrange(256))
                          for slot, (ty, x, y) in enumerate(nodes)
                          if isinstance(ty, str) and slot < index and generator.random() < 0.8}
        walls_list = tuple(generator.sample(('GAWALL', 'NAWALL', 'YAWALL', 'ANYWALL'),
                                            generator.randrange(0, 4)))
        rows.append(walls_row(f'random {case}', nodes, index,
                              side=generator.choice((0, 1, 2, 3)), walls=walls_list,
                              node_buildings=node_buildings))
    return rows


# ---------------------------------------------------------------- blocks

PLACEMENT_DELAYS = (0.05, 0.0, 1.0, 0.1, 1 / 30, 0.05000000000000001, 10 / 900,
                    0.0011111111111111111, 2.5, -0.05, 1e-9, 100.0, 0.07)


def placement_delay():
    rows = []
    for minutes in PLACEMENT_DELAYS:
        emu = Emu()
        emu.uc.mem_write(RULES + 0x5F8, struct.pack('<d', minutes))
        emu.uc.reg_write(UC_X86_REG_ESP, STACK_BASE + STACK_SIZE - 0x1000)
        emu.uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        run_checked(emu.uc, 0x4501CB, 0x4501E7, required_addresses=[0x7C5F00])
        rows.append(dict(minutes_bits=struct.unpack('<Q', struct.pack('<d', minutes))[0],
                         minutes=minutes, frames=struct.unpack('<i', u32(emu.uc.reg_read(UC_X86_REG_EAX)))[0]))
    return rows


def direction():
    rows = []
    for x in range(-3, 4):
        for y in range(-3, 4):
            if (x, y) == (0, 0):
                continue
            emu = Emu()
            frame = STACK_BASE + STACK_SIZE - 0x800
            emu.uc.reg_write(UC_X86_REG_ESP, frame - 0x200)
            emu.uc.reg_write(UC_X86_REG_EBP, frame)
            emu.uc.reg_write(UC_X86_REG_EAX, x & 0xFFFF)
            emu.uc.reg_write(UC_X86_REG_ECX, y & 0xFFFF)
            emu.uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
            run_checked(emu.uc, 0x5065E6, 0x50664C, required_addresses=[0x4CAE30, 0x7C5F00])
            sp = emu.uc.reg_read(UC_X86_REG_ESP)
            facing = struct.unpack('<H', emu.uc.mem_read(sp + 0xD4, 2))[0]
            rows.append(dict(sum=[x, y], facing=facing, direction=emu.uc.reg_read(UC_X86_REG_ESI)))
    return rows


def key():
    rows = []
    cells = [(20, 20), (21, 20), (19, 23), (26, 14), (20, 20), (0, 0), (-5, 7),
             (32767, -32768), (-32768, 32767), (100, -100)]
    centers = [(20, 20), (0, 0), (-32768, -32768), (32767, 32767)]
    for center in centers:
        for index, cell in enumerate(cells):
            for position in sorted({index, 999, -1, 0x7FFFFFFF - 1000 * 65535}):
                emu = Emu()
                emu.uc.mem_write(HOUSE + 0x5750, i16s(*center))
                emu.uc.mem_write(SCENARIO, i16s(*cell))
                value = emu.invoke(0x505F80, ecx=HOUSE, edx=SCENARIO, args=[position & 0xFFFFFFFF, 0xFFFFFFFF])
                rows.append(dict(center=list(center), cell=list(cell), index=position,
                                 key=struct.unpack('<i', u32(value))[0]))
    return rows


def sort():
    rows = []
    generator = random.Random(0x5108F0)
    shapes = [list(range(n)) for n in (0, 1, 2, 3, 8, 9, 17)]
    for n in (2, 5, 8, 9, 13, 24, 40, 64):
        shapes.append([generator.randrange(4) * 1000 + generator.randrange(3) for _ in range(n)])
        shapes.append([generator.randrange(-50, 50) for _ in range(n)])
        shapes.append([n - i for i in range(n)])
        shapes.append([5] * n)
    for keys in shapes:
        emu = Emu()
        records = HEAP + 0x200000
        for slot, value in enumerate(keys):
            emu.uc.mem_write(records + 8 * slot, struct.pack('<ihh', value, slot, -slot))
        emu.invoke(0x7C8B48, args=[records, len(keys), 8, 0x5108F0])
        order = []
        for slot in range(len(keys)):
            value, tag, check = struct.unpack('<ihh', emu.uc.mem_read(records + 8 * slot, 8))
            if check != -tag or keys[tag] != value:
                raise OracleError('qsort lost a record')
            order.append(tag)
        rows.append(dict(keys=keys, order=order))
    return rows


# ---------------------------------------------------------------- map fixtures

HOUSE_INDEX = 2


class Map:
    """A synthetic 64x64 map: the house's reservation bits, levels, cells the
    occupancy model reports as blocked and sites CanPlaceAt refuses."""

    def __init__(self, reserved=(), levels=None, blocked=(), unplaceable=(), foreign=()):
        self.reserved = set(map(tuple, reserved))
        self.foreign = set(map(tuple, foreign))
        self.levels = dict(levels or {})
        self.blocked = set(map(tuple, blocked))
        self.unplaceable = set(map(tuple, unplaceable))

    def on_map(self, x, y):
        return 0 <= x < MAP_SIZE and 0 <= y < MAP_SIZE

    def write(self, emu):
        for (x, y) in self.reserved | self.foreign | set(self.levels):
            if not self.on_map(x, y):
                raise OracleError('fixture cell off the synthetic map')
            cell = CELLS + (y * MAP_SIZE + x) * CELL_SIZE
            bits = (1 << HOUSE_INDEX if (x, y) in self.reserved else 0) | \
                   (1 << (HOUSE_INDEX + 1) if (x, y) in self.foreign else 0)
            emu.write32(cell + 0xDC, bits)
            emu.uc.mem_write(cell + 0x11B, struct.pack('<b', self.levels.get((x, y), 0)))

    def clear(self, x, y, w, h):
        if x < 0 or y < 0 or x + w > MAP_SIZE or y + h > MAP_SIZE:
            return False
        return not any((cx, cy) in self.blocked
                       for cx in range(x, x + w) for cy in range(y, y + h))

    def placeable(self, x, y, w, h):
        return (x, y) not in self.unplaceable and 1 <= x and 1 <= y \
            and x + w < MAP_SIZE and y + h < MAP_SIZE


def install_map(emu, world):
    world.write(emu)

    def cell(e):
        x, y = struct.unpack('<hh', e.uc.mem_read(e.arg(0), 4))
        if e.uc.reg_read(UC_X86_REG_ECX) != MAP:
            raise OracleError('cell lookup on the wrong map')
        if world.on_map(x, y):
            address = CELLS + (y * MAP_SIZE + x) * CELL_SIZE
        else:
            address = DUMMY_CELL
        reserved = bool(e.read32(address + 0xDC) & (1 << HOUSE_INDEX))
        level = struct.unpack('<b', e.uc.mem_read(address + 0x11B, 1))[0]
        e.events.append(['cell', x, y, reserved, level])
        return address

    emu.hook(0x5657A0, cell, 4)


def write_type(emu, width_height, protect=False, extra=False):
    foundation = FOUNDATIONS[width_height]
    ty = TYPES
    emu.write32(ty, TYPE_VTABLE)
    emu.write32(TYPE_VTABLE + 0xA8, STUB_CAN_PLACE)
    emu.write32(ty + 0xEF0, foundation)
    emu.uc.mem_write(ty + 0xCCE, b'\0')
    emu.uc.mem_write(ty + 0x16C7, b'\0')
    emu.uc.mem_write(ty + 0x1578, bytes([int(extra)]))
    emu.uc.mem_write(ty + 0x1765, bytes([int(protect)]))
    return ty


# BuildingTypeClass::Width/Height tables 0x8192B8/0x819310 by Foundation.
FOUNDATIONS = {(1, 1): 0, (2, 1): 1, (1, 2): 2, (2, 2): 3, (2, 3): 4, (3, 2): 5,
               (3, 3): 6, (3, 5): 7, (4, 2): 8, (1, 3): 10, (3, 1): 11, (4, 3): 12,
               (1, 4): 13, (1, 5): 14, (2, 6): 15, (2, 5): 16, (5, 3): 17, (4, 4): 18,
               (3, 4): 19, (6, 4): 20}


def reserved_near():
    rows = []
    for (w, h), spacing in (((1, 1), 0), ((2, 2), 1), ((3, 3), 2), ((4, 3), 1), ((2, 6), 3),
                            ((6, 4), 0), ((1, 4), 2)):
        site = (30, 30)
        answers = []
        span_x = range(-spacing - 3, w + 2 * spacing + 3)
        span_y = range(-spacing - 3, h + 2 * spacing + 3)
        for dy in span_y:
            for dx in span_x:
                emu = Emu()
                emu.write32(GAME_MODE, 1)
                emu.write32(RULES + 0x1460, spacing)
                emu.write32(HOUSE + 0x30, HOUSE_INDEX)
                ty = write_type(emu, (w, h))
                install_map(emu, Map(reserved=[(site[0] + dx, site[1] + dy)]))
                emu.uc.mem_write(SCENARIO, i16s(*site))
                answers.append(emu.invoke(0x50B760, ecx=HOUSE, args=[ty, SCENARIO]) & 0xFF)
        rows.append(dict(width=w, height=h, spacing=spacing, site=list(site),
                         first=[span_x.start, span_y.start], columns=len(span_x),
                         answers=''.join(str(int(bool(a))) for a in answers)))
    for game_mode, foreign in ((1, True), (0, False), (0, True)):
        emu = Emu()
        emu.write32(GAME_MODE, game_mode)
        emu.write32(RULES + 0x1460, 1)
        emu.write32(HOUSE + 0x30, HOUSE_INDEX)
        ty = write_type(emu, (2, 2))
        world = Map(foreign=[(31, 31)]) if foreign else Map()
        install_map(emu, world)
        emu.uc.mem_write(SCENARIO, i16s(30, 30))
        answer = emu.invoke(0x50B760, ecx=HOUSE, args=[ty, SCENARIO]) & 0xFF
        rows.append(dict(width=2, height=2, spacing=1, site=[30, 30], game_mode=game_mode,
                         foreign=foreign, answer=bool(answer), lookups=len(emu.events)))
    return rows


# ---------------------------------------------------------------- site search

def base_blob(generator, center, size):
    """A connected block of reserved cells around `center`."""
    cells = {center}
    while len(cells) < size:
        x, y = generator.choice(sorted(cells))
        dx, dy = generator.choice(((1, 0), (-1, 0), (0, 1), (0, -1)))
        cell = (x + dx, y + dy)
        if 4 <= cell[0] < MAP_SIZE - 4 and 4 <= cell[1] < MAP_SIZE - 4:
            cells.add(cell)
    return cells


def perimeter_of(cells):
    ring = set()
    for (x, y) in cells:
        for dx in (-1, 0, 1):
            for dy in (-1, 0, 1):
                cell = (x + dx, y + dy)
                if cell not in cells:
                    ring.add(cell)
    return sorted(ring)


def site_row(label, *, center, perimeter, world, size=(2, 2), spacing=1, protect=False,
             extra=False, game_mode=1, alternate=(0, 0), base=(0, 0), key=0x505F80,
             argument=-1, rect=None, grid=None):
    """One search with the ordinary key, or with another key function and its
    argument; the defense key 0x505FD0 reads `grid` over `rect` through
    House+0x16060."""
    emu = Emu()
    emu.write32(GAME_MODE, game_mode)
    emu.write32(RULES + 0x1460, spacing)
    emu.write32(HOUSE + 0x30, HOUSE_INDEX)
    emu.uc.mem_write(HOUSE + 0x5750, i16s(*center))
    emu.uc.mem_write(HOUSE + 0x5494, i16s(*alternate))
    emu.uc.mem_write(HOUSE + 0x5490, i16s(*base))
    cells = LISTS + 0x1000
    emu.write32(HOUSE + 0x5724, cells)
    emu.write32(HOUSE + 0x5730, len(perimeter))
    for slot, cell in enumerate(perimeter):
        emu.uc.mem_write(cells + 4 * slot, i16s(*cell))
    ty = write_type(emu, size, protect, extra)
    install_map(emu, world)
    width, height = size

    def occupancy(e):
        rect = [e.read_i32(e.arg(0) + 4 * i) for i in range(4)]
        house = e.read_i32(e.uc.reg_read(UC_X86_REG_ESP) + 8)
        answer = world.clear(*rect)
        e.events.append(['clear', *rect, house, answer])
        return int(answer)

    def can_place(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != ty:
            raise OracleError('CanPlaceAt on the wrong type')
        if e.arg(1) != HOUSE:
            raise OracleError('CanPlaceAt for another house')
        x, y = struct.unpack('<hh', e.uc.mem_read(e.arg(0), 4))
        answer = world.placeable(x, y, width, height)
        e.events.append(['place', x, y, answer])
        return int(answer)

    emu.hook(0x586780, occupancy, 8)
    emu.hook(STUB_CAN_PLACE, can_place, 8)
    emu.mark(0x506540, ['pass'])
    if grid is not None:
        for slot, part in enumerate(rect):
            emu.write32(HOUSE + 0x5754 + 4 * slot, part)
        cells_grid = FAKE + 0x500000
        for slot, value in enumerate(grid):
            emu.write32(cells_grid + 4 * slot, value)
        emu.write32(HOUSE + 0x16060, cells_grid)
    out = SCENARIO + 0x100
    emu.write32(out, 0xDEADBEEF)
    emu.invoke(0x5060B0, ecx=HOUSE, args=[out, ty, key, argument & 0xFFFFFFFF])
    answer = list(struct.unpack('<hh', emu.uc.mem_read(out, 4)))
    # Events as one-line strings: `cell x y reserved level`,
    # `clear x y w h house answer`, `place x y answer`.
    lines, starts = [], []
    for event in emu.events:
        if event == ['pass']:
            starts.append(len(lines))
            continue
        lines.append(' '.join(str(int(v)) if isinstance(v, bool) else str(v) for v in event))
    if len(starts) == 2:
        # The body's second pass repeats the first exactly: nothing it reads
        # changed. Only the first is kept.
        first, second = starts
        if lines[second:] != lines[first:second]:
            raise OracleError(f'{label}: the second pass differs from the first')
        lines = lines[:second]
    passes = len(starts)
    row = dict(label=label, center=list(center), alternate=list(alternate), base=list(base),
               perimeter=[list(cell) for cell in perimeter], width=width, height=height,
               spacing=spacing, protect=protect, extra=extra, game_mode=game_mode,
               house=HOUSE_INDEX, answer=answer, passes=passes, events=lines)
    if grid is not None:
        row.update(argument=argument, rect=list(rect), grid=list(grid))
    return row


def site():
    rows = []
    rows.append(site_row('no plan centre, alternate', center=(0, 0), perimeter=[(5, 5)],
                         world=Map(), alternate=(17, 9), base=(3, 4)))
    rows.append(site_row('no plan centre, primary', center=(0, 0), perimeter=[(5, 5)],
                         world=Map(), base=(3, 4)))
    rows.append(site_row('no perimeter', center=(20, 20), perimeter=[], world=Map()))
    square = {(x, y) for x in range(19, 22) for y in range(19, 22)}
    ring = perimeter_of(square)
    rows.append(site_row('open ground', center=(20, 20), perimeter=ring,
                         world=Map(reserved=square)))
    rows.append(site_row('ring without reservations', center=(20, 20), perimeter=ring,
                         world=Map(foreign=square)))
    rows.append(site_row('campaign', center=(20, 20), perimeter=ring, world=Map(reserved=square),
                         game_mode=0))
    for size in ((1, 1), (3, 3), (4, 3), (2, 6), (6, 4)):
        for spacing in (0, 1, 3):
            rows.append(site_row(f'sizes {size} spacing {spacing}', center=(20, 20),
                                 perimeter=ring, world=Map(reserved=square), size=size,
                                 spacing=spacing))
    rows.append(site_row('protect with wall', center=(20, 20), perimeter=ring,
                         world=Map(reserved=square), protect=True))
    rows.append(site_row('extra space', center=(20, 20), perimeter=ring,
                         world=Map(reserved=square), extra=True, size=(3, 3)))
    blocked_all = {(x, y) for x in range(MAP_SIZE) for y in range(MAP_SIZE)}
    rows.append(site_row('everything blocked', center=(20, 20), perimeter=ring,
                         world=Map(reserved=square, blocked=blocked_all)))
    rows.append(site_row('nothing placeable', center=(20, 20), perimeter=ring,
                         world=Map(reserved=square,
                                   unplaceable={(x, y) for x in range(MAP_SIZE)
                                                for y in range(MAP_SIZE)})))
    hills = {(x, y): 4 for x in range(MAP_SIZE) for y in range(MAP_SIZE)}
    hills.update({cell: 0 for cell in square})
    rows.append(site_row('hills', center=(20, 20), perimeter=ring,
                         world=Map(reserved=square, levels=hills)))
    rows.append(site_row('near hills', center=(20, 20), perimeter=ring,
                         world=Map(reserved=square, levels={(x, y): 2 for x in range(10, 20)
                                                            for y in range(10, 30)})))
    rows.append(site_row('negative levels', center=(20, 20), perimeter=ring,
                         world=Map(reserved=square, levels={**{c: -2 for c in square},
                                                            (16, 16): 1, (23, 23): -5})))
    generator = random.Random(0x5060B0)
    for case in range(40):
        # The later half blocks most of the map, so searches run deep.
        dense = case >= 24
        center = (generator.randrange(14, 50), generator.randrange(14, 50))
        blob = base_blob(generator, center, generator.randrange(4, 40))
        ring = perimeter_of(blob)
        generator.shuffle(ring)
        blocked = {(generator.randrange(MAP_SIZE), generator.randrange(MAP_SIZE))
                   for _ in range(generator.randrange(1500, 3000) if dense
                                  else generator.randrange(0, 400))}
        blocked -= blob
        unplaceable = {(generator.randrange(MAP_SIZE), generator.randrange(MAP_SIZE))
                       for _ in range(generator.randrange(0, 200))}
        levels = {(generator.randrange(MAP_SIZE), generator.randrange(MAP_SIZE)):
                  generator.randrange(-4, 5) for _ in range(generator.randrange(0, 300))}
        size = generator.choice(sorted(FOUNDATIONS))
        rows.append(site_row(f'random {case}', center=center, perimeter=ring,
                             world=Map(reserved=blob, blocked=blocked, unplaceable=unplaceable,
                                       levels=levels),
                             size=size, spacing=generator.randrange(0, 4),
                             protect=generator.random() < 0.3, extra=generator.random() < 0.3))
    return rows


def generate():
    return {'source': 'unicorn/gamemd.exe', 'economy': economy(), 'chooser': chooser(),
            'wall_types': wall_types(), 'walls': walls(),
            'placement_delay': placement_delay(), 'direction': direction(), 'key': key(),
            'sort': sort(), 'reserved_near': reserved_near(), 'site': site()}


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope=('computer base building: EconomyStateMachine mode steps and draws, '
               'AI_Choose_Building node handling and draws with the walls, AI_BuildWalls, '
               'Factory_AI retry wait, the '
               'ordinary site key, qsort order, away direction, reserved-near bounds and '
               'whole FindBaseBuildingSite searches on synthetic 64x64 maps'),
        assumptions=['fresh emulator per case; fixture House/Rules/Type layouts from live disassembly',
                     'x87 control word 0x0E7F (53-bit chop), the process word ftol callers assume',
                     'skirmish unless a row says game_mode 0'],
        substitutions=['money interface vt+0x18, GetItemCount 0x49FAE0 and RandomRanged 0x65C7E0 answer from the row',
                       'Find_Node 0x42EB20 answers the first unfilled node',
                       'Get_Node_Building 0x42E820 answers the building object a row places for a node index',
                       'ChooseNextProduction 0x506EF0 fails; the perimeter scan 0x5082C0 does nothing',
                       'operator new/delete and atexit are fixture stubs',
                       'MapClass::operator[] 0x5657A0 answers synthetic cells; CheckOccupancy 0x586780 and CanPlaceAt vt+0xA8 answer a blocked-cell/unplaceable-site model'],
        entry_points={'EconomyStateMachine': 0x509700, 'AI_Choose_Building': 0x4FE3E0,
                      'AI_BuildWalls': 0x50C340,
                      'Factory_AI_retry_wait': 0x4501CB, 'ordinary_key': 0x505F80,
                      'qsort': 0x7C8B48, 'site_comparator': 0x5108F0,
                      'away_direction': 0x5065E6, 'reserved_near': 0x50B760,
                      'FindBaseBuildingSite': 0x5060B0}))
