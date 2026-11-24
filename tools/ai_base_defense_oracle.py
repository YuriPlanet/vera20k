"""Native references for the computer's base defense choice.

Run python -m tools.ai_base_defense_oracle --check (or explicit --write).
Rust consumers: src/sim/ai_base_defense_tests.rs and
src/sim/ai_base_site_tests.rs.

Sections, each executed in a fresh emulator per case (the fixture machinery
is tools.ai_base_building_oracle's):
- influence: the coverage spread 0x506D50 (Sqrt_Approx 0x4CAC40, ftol
  0x7C5F00) of one building into a grid.
- key: the defense site key 0x505FD0 (atan2 wrapper 0x4CAE30).
- threat: AI_UpdateEnemyThreatRatios 0x508150 with its three
  RandomRanged(-r, r) draws.
- choose: whole AI_ChooseNextProduction 0x506EF0 runs without a perimeter
  vector: coverage, weakest quadrant, threat ratios, the three candidate
  lists (0x507B80, 0x507D70, 0x507F60 with the prerequisite test 0x505360),
  the category, the weighted RandomRanged(1, total) pick and the node writes.
  The site search is supplied; its call (type, key, argument) and the grid
  the key reads at that moment are recorded.
- site: FindBaseBuildingSite 0x5060B0 with the defense key and a coverage
  grid on synthetic maps; the transcript records every cell lookup,
  occupancy test and placement test with the answer the model gave.
"""
from pathlib import Path
import random
import struct

from unicorn.x86_const import UC_X86_REG_ECX

from tools.ai_base_building_oracle import (BUILDING_TYPES, GROUP_VTABLE, HOUSE,
                                           NODE_VTABLE, RULES, SCENARIO, TYPES,
                                           TYPE_SIZE, FAKE, Emu, Map, base_blob, i16s,
                                           perimeter_of, site_row, u32)
from tools.native_oracle import OracleError, finish_vectors, provenance

HOUSES = 0xA8022C
BUILDING_VTABLE = 0x7E3EBC
FIND_INDEX_OF_NAME = 0x5117D0
SITE_SEARCH = 0x5060B0
DEFENSE_KEY = 0x505FD0

# Fixture regions past tools.ai_base_building_oracle's.
ENEMY = FAKE + 0x400000
GRID = FAKE + 0x500000
OBJECTS = FAKE + 0x600000
OBJECT_SIZE = 0x800
HOUSE_TYPE = FAKE + 0x700000
NODES = FAKE + 0x701000
NODE_CAPACITY = 64
AREA = FAKE + 0x710000
HOUSE_ITEMS = AREA
FUDGE_ITEMS = AREA + 0x100
RECT = AREA + 0x200
PREREQUISITES = AREA + 0x1000
TYPE_LISTS = AREA + 0x2000
TYPE_ITEMS = AREA + 0x3000
OBJECT_ITEMS = AREA + 0x4000
COUNTRY = 3


def i32(value):
    return struct.unpack('<i', u32(value))[0]


def f32_bits(emu, address):
    return emu.read32(address)


def write_grid(emu, address, values):
    for slot, value in enumerate(values):
        emu.write32(address + 4 * slot, value)


def read_grid(emu, address, count):
    return [emu.read_i32(address + 4 * slot) for slot in range(count)]


# ---------------------------------------------------------------- influence

def influence_row(generator, *, cell, value, rect, before=None):
    emu = Emu()
    building = OBJECTS
    emu.write32(building + 0x9C, cell[0] * 256 + 128)
    emu.write32(building + 0xA0, cell[1] * 256 + 128)
    x, y, w, h = rect
    count = max(w, 0) * max(h, 0)
    if before is None:
        before = [generator.randrange(0, 400) for _ in range(count)]
    write_grid(emu, GRID, before)
    for slot, part in enumerate(rect):
        emu.write32(RECT + 4 * slot, part)
    # Some cell of the clamped window lies within five cells: ftol runs.
    reached = any((cx - cell[0]) ** 2 + (cy - cell[1]) ** 2 <= 35
                  for cx in range(max(cell[0] - 6, x), min(x + w, cell[0] + 6))
                  for cy in range(max(cell[1] - 6, y), min(y + h, cell[1] + 6)))
    emu.invoke(0x506D50, ecx=HOUSE, args=[building, value & 0xFFFFFFFF, GRID, RECT],
               required=[0x7C5F00] if reached else ())
    return dict(cell=list(cell), value=value, rect=list(rect), before=before,
                after=read_grid(emu, GRID, count))


def influence():
    generator = random.Random(0x506D50)
    rows = []
    rect = (10, 12, 20, 16)
    for value in (-7, 0, 1, 3, 25, 100, 999, 123456, 0x7FFFFFFF):
        rows.append(influence_row(generator, cell=(18, 20), value=value, rect=rect,
                                  before=[0] * (20 * 16)))
    for cell in ((10, 12), (29, 27), (8, 20), (35, 20), (18, 5), (18, 34), (4, 6),
                 (40, 40), (15, 30), (30, 11)):
        rows.append(influence_row(generator, cell=cell, value=25, rect=rect))
    for small in ((20, 20, 1, 1), (20, 20, 3, 2), (18, 19, 0, 5), (18, 19, 5, 0),
                  (18, 19, 5, -2)):
        rows.append(influence_row(generator, cell=(20, 20), value=10, rect=small))
    for _case in range(12):
        rect = (generator.randrange(0, 40), generator.randrange(0, 40),
                generator.randrange(1, 30), generator.randrange(1, 30))
        cell = (rect[0] + generator.randrange(-8, rect[2] + 8),
                rect[1] + generator.randrange(-8, rect[3] + 8))
        rows.append(influence_row(generator, cell=cell, value=generator.randrange(1, 60),
                                  rect=rect))
    return rows


# ---------------------------------------------------------------- key

def key_row(*, cell, index, argument, center, rect, grid):
    emu = Emu()
    emu.uc.mem_write(HOUSE + 0x5750, i16s(*center))
    for slot, part in enumerate(rect):
        emu.write32(HOUSE + 0x5754 + 4 * slot, part)
    write_grid(emu, GRID, grid)
    emu.write32(HOUSE + 0x16060, GRID)
    emu.uc.mem_write(SCENARIO, i16s(*cell))
    value = emu.invoke(DEFENSE_KEY, ecx=HOUSE, edx=SCENARIO,
                       args=[index & 0xFFFFFFFF, argument & 0xFFFFFFFF],
                       required=[0x4CAE30] if argument != -1 else ())
    return dict(cell=list(cell), index=index, argument=argument, center=list(center),
                rect=list(rect), grid=grid, key=i32(value))


def key():
    generator = random.Random(0x505FD0)
    rows = []
    center = (20, 20)
    rect = (14, 15, 13, 11)
    grid = [generator.randrange(0, 60) for _ in range(13 * 11)]
    cells = [(x, y) for x in range(14, 27) for y in range(15, 26)]
    for argument in (-1, 0, 2, 4, 6, 1, 7, 8, -2, 13, 255):
        for cell in generator.sample(cells, 12) + [center, (14, 15), (26, 25), (20, 15),
                                                   (26, 20), (20, 25), (14, 20)]:
            rows.append(key_row(cell=cell, index=generator.choice((0, 3, 57, 999)),
                                argument=argument, center=center, rect=rect, grid=grid))
    # Directions around the whole circle, one cell per step.
    ring = [(20 + dx, 20 + dy) for dx in range(-6, 7) for dy in range(-5, 6)]
    for argument in (0, 2, 4, 6):
        for cell in ring:
            rows.append(key_row(cell=cell, index=11, argument=argument, center=center,
                                rect=rect, grid=grid))
    big = [0, 1, 1000, 16000, 0x00FFFFFF, -3]
    for value in big:
        rows.append(key_row(cell=(15, 16), index=7, argument=2, center=center,
                            rect=(15, 16, 1, 1), grid=[value]))
    return rows


# ---------------------------------------------------------------- threat

def install_enemy(emu, enemy_index, values):
    emu.write32(HOUSES, HOUSE_ITEMS)
    for slot in range(8):
        emu.write32(HOUSE_ITEMS + 4 * slot, ENEMY if slot == enemy_index else 0)
    infantry, vehicles, air = values
    emu.write32(ENEMY + 0x160A8, infantry)
    emu.write32(ENEMY + 0x160AC, vehicles)
    emu.write32(ENEMY + 0x160B0, air)


def install_fudge(emu, fudge):
    emu.write32(RULES + 0x9A8 + 4, FUDGE_ITEMS)
    emu.write32(RULES + 0x9A8 + 0x10, len(fudge))
    for slot, value in enumerate(fudge):
        emu.write32(FUDGE_ITEMS + 4 * slot, value)


def draw_answers(generator, policy):
    """RandomRanged answers: `policy` picks each from its sorted range."""
    def answer(low, high):
        low, high = i32(low), i32(high)
        lo, hi = min(low, high), max(low, high)
        if policy == 'low':
            return lo
        if policy == 'high':
            return hi
        if policy == 'zero':
            return max(lo, min(hi, 0))
        return generator.randint(lo, hi)
    return answer


def install_draws(emu, answer):
    from unicorn.x86_const import UC_X86_REG_ECX as ECX

    def draw(e):
        stream = e.uc.reg_read(ECX) - e.read32(0xA8B230)
        low, high = e.arg(0), e.arg(1)
        value = answer(low, high)
        e.events.append(['draw', stream, i32(low), i32(high), value])
        return value
    emu.hook(0x65C7E0, draw, 8)


def threat_row(generator, *, enemy, values=(0, 0, 0), fudge=(5, 25, 80), difficulty=1,
               policy='random'):
    emu = Emu()
    emu.write32(HOUSE + 0x5600, enemy)
    emu.write32(HOUSE + 0x184, difficulty)
    install_enemy(emu, enemy, values)
    install_fudge(emu, fudge)
    install_draws(emu, draw_answers(generator, policy))
    # The constructor's ratios; every path overwrites them.
    for offset in (0x1609C, 0x160A0, 0x160A4):
        emu.write32(HOUSE + offset, 0xDEADBEEF)
    emu.invoke(0x508150, ecx=HOUSE)
    return dict(enemy=enemy, values=list(values), fudge=list(fudge), difficulty=difficulty,
                draws=[event[2:] for event in emu.events if event[0] == 'draw'],
                vehicles=f32_bits(emu, HOUSE + 0x1609C), air=f32_bits(emu, HOUSE + 0x160A0),
                infantry=f32_bits(emu, HOUSE + 0x160A4))


def threat():
    generator = random.Random(0x508150)
    rows = [threat_row(generator, enemy=-1)]
    for difficulty in (0, 1, 2):
        for policy in ('low', 'high', 'zero', 'random'):
            rows.append(threat_row(generator, enemy=1, values=(1200, 5400, 800),
                                   difficulty=difficulty, policy=policy))
    for values in ((0, 0, 0), (-3000, -3000, -3000), (-3000, 0, 0), (100000, 0, 0),
                   (0, 250000, 3), (7, 11, 13), (40000, 40000, 40000),
                   (-2999, -2999, -2999), (-9000, 1000, 1000), (0x7FFFFFFF - 3000, 0, 0)):
        rows.append(threat_row(generator, enemy=2, values=values))
    for fudge in ((0, 0, 0), (100, 100, 100), (1000, 1000, 1000), (-50, 3, 7), (1, 1, 1)):
        rows.append(threat_row(generator, enemy=0, values=(900, 1800, 2700), fudge=fudge,
                               difficulty=generator.randrange(3)))
    for _case in range(16):
        rows.append(threat_row(generator, enemy=generator.randrange(8),
                               values=tuple(generator.randrange(-4000, 30000) for _ in range(3)),
                               fudge=tuple(generator.randrange(0, 120) for _ in range(3)),
                               difficulty=generator.randrange(3)))
    return rows


# ---------------------------------------------------------------- choose

ALL = 1 << COUNTRY
# name, (air, armor, infantry), IsBaseDefense, owner bits, TechLevel,
# prerequisites (type indices or generic group codes)
DEFENSE_TYPES = [
    ('YARD', (0, 0, 0), False, ALL, 1, ()),
    ('POWR', (0, 0, 0), False, ALL, 1, ()),
    ('TECH', (0, 0, 0), False, ALL, 1, ()),
    ('WALLT', (0, 0, 0), False, ALL, 1, ()),
    ('PILL', (0, 10, 25), True, ALL, 1, (1,)),
    ('SAM', (25, 0, 0), True, ALL, 1, (1,)),
    ('TESL', (0, 25, 25), True, ALL, 3, (2,)),
    ('GGUN', (25, 10, 25), True, ALL, 1, ()),
    ('FORN', (25, 25, 25), True, 1 << 1, 1, ()),
    ('HITK', (0, 25, 0), True, ALL, 11, ()),
    ('BUNK', (0, 25, 10), True, ALL, 1, (-1,)),
    ('NEGA', (-5, 30, 0), True, ALL, 1, ()),
    ('WALLD', (40, 40, 40), False, ALL, 1, ()),
    ('TECHD', (0, 0, 0), True, ALL, 1, ()),
]
TYPE_INDEX = {name: index for index, (name, *_rest) in enumerate(DEFENSE_TYPES)}
SIDE_LISTS = {
    0: ['PILL', 'SAM', 'TESL', 'GGUN', 'FORN', 'HITK', 'BUNK', 'NEGA'],
    1: ['BUNK', 'PILL', 'SAM', 'PILL'],
    2: ['GGUN'],
}
# Generic prerequisite groups (0x505360's -1..-6): POWER holds POWR.
GROUPS = {0x8C8: ['POWR'], 0x938: ['YARD'], 0x900: ['YARD'], 0xA34: ['TECH'],
          0x91C: ['TECH'], 0x8E4: ['YARD']}


def type_address(name):
    return TYPES + TYPE_INDEX[name] * TYPE_SIZE


def install_types(emu, wall_tower):
    items = TYPE_ITEMS
    emu.write32(BUILDING_TYPES, items)
    prereqs = PREREQUISITES
    for index, (name, (air, armor, infantry), defense, owner, tech, needs) in \
            enumerate(DEFENSE_TYPES):
        ty = TYPES + index * TYPE_SIZE
        emu.write32(items + 4 * index, ty)
        emu.write32(ty + 0xDF8, index)
        emu.write32(ty + 0x1524, air)
        emu.write32(ty + 0x1528, armor)
        emu.write32(ty + 0x152C, infantry)
        emu.uc.mem_write(ty + 0x1706, bytes([int(defense)]))
        emu.write32(ty + 0x6CC, owner)
        emu.write32(ty + 0x634, tech)
        emu.write32(ty + 0x63C, prereqs + 0x20 * index)
        emu.write32(ty + 0x648, len(needs))
        for slot, need in enumerate(needs):
            emu.write32(prereqs + 0x20 * index + 4 * slot, need)
    emu.write32(RULES + 0x87C, type_address(wall_tower) if wall_tower else 0)
    lists = TYPE_LISTS
    for offset, names in [(0x954, SIDE_LISTS[0]), (0x970, SIDE_LISTS[1]),
                          (0x98C, SIDE_LISTS[2])] + list(GROUPS.items()):
        emu.write32(RULES + offset, GROUP_VTABLE)
        emu.write32(RULES + offset + 4, lists)
        emu.write32(RULES + offset + 0x10, len(names))
        for slot, name in enumerate(names):
            emu.write32(lists + 4 * slot, type_address(name))
        lists += 0x40


def install_house(emu, *, side, tech, difficulty, center, rect, buildings, nodes):
    emu.write32(HOUSE + 0x34, HOUSE_TYPE)
    emu.write32(HOUSE_TYPE + 0xBC, side)
    emu.write32(HOUSE + 0x184, difficulty)
    emu.write32(HOUSE + 0x1D4, tech)
    emu.uc.mem_write(HOUSE + 0x5750, i16s(*center))
    for slot, part in enumerate(rect):
        emu.write32(HOUSE + 0x5754 + 4 * slot, part)
    emu.write32(HOUSE + 0x5748, 0)
    emu.write32(HOUSE + 0x16060, 0xDEADBEEF)

    def find_index_of_name(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != HOUSE_TYPE + 0x98:
            raise OracleError('FindIndexOfName of another name')
        return COUNTRY
    emu.hook(FIND_INDEX_OF_NAME, find_index_of_name, 0)
    objects = OBJECT_ITEMS
    emu.write32(HOUSE + 0x6C, objects)
    emu.write32(HOUSE + 0x78, len(buildings))
    for slot, (name, (x, y)) in enumerate(buildings):
        building = OBJECTS + slot * OBJECT_SIZE
        emu.write32(objects + 4 * slot, building)
        emu.write32(building, BUILDING_VTABLE)
        emu.write32(building + 0x9C, x * 256 + 128)
        emu.write32(building + 0xA0, y * 256 + 128)
        emu.write32(building + 0x520, type_address(name))
    base = HOUSE + 0x5704
    emu.write32(base, NODE_VTABLE)
    emu.write32(base + 4, NODES)
    emu.write32(base + 8, NODE_CAPACITY)
    emu.uc.mem_write(base + 0xC, bytes([1, 1]))
    emu.write32(base + 0x10, len(nodes))
    emu.write32(base + 0x14, 10)
    # Past the count the capacity holds stale nodes the WallTower successor
    # write may reach.
    for slot in range(NODE_CAPACITY):
        ty, x, y = nodes[slot] if slot < len(nodes) else (0x55555555, 0x5555, 0x5555)
        emu.uc.mem_write(NODES + 16 * slot, struct.pack('<ihhII', i32(ty), x, y, 0, 0))


def read_nodes(emu):
    count = emu.read_i32(HOUSE + 0x5714)
    items = emu.read32(HOUSE + 0x5708)
    nodes = []
    for slot in range(count):
        ty, x, y = struct.unpack('<ihh', emu.uc.mem_read(items + 16 * slot, 8))
        nodes.append([ty, x, y])
    return nodes


def choose_row(label, generator, *, side=0, tech=10, difficulty=1, center=(30, 30),
               rect=(22, 23, 17, 15), buildings=(), enemy=-1, values=(0, 0, 0),
               fudge=(5, 25, 80), nodes=((-1, 0, 0),), index=0, wall_tower='WALLT',
               site=(33, 27), policy='random'):
    emu = Emu()
    install_types(emu, wall_tower)
    install_house(emu, side=side, tech=tech, difficulty=difficulty, center=center, rect=rect,
                  buildings=buildings, nodes=nodes)
    emu.write32(HOUSE + 0x5600, enemy)
    install_enemy(emu, enemy, values)
    install_fudge(emu, fudge)
    install_draws(emu, draw_answers(generator, policy))

    def site_search(e):
        out, ty, key_function, argument = (e.arg(slot) for slot in range(4))
        if e.uc.reg_read(UC_X86_REG_ECX) != HOUSE:
            raise OracleError('site search for another house')
        width, height = e.read_i32(HOUSE + 0x575C), e.read_i32(HOUSE + 0x5760)
        grid = read_grid(e, e.read32(HOUSE + 0x16060), width * height)
        e.events.append(['site', e.read_i32(ty + 0xDF8), key_function, i32(argument), grid])
        e.uc.mem_write(out, i16s(*site))
        return out
    emu.hook(SITE_SEARCH, site_search, 16)
    result = emu.invoke(0x506EF0, ecx=HOUSE, args=[index, 0])
    events = []
    for event in emu.events:
        if event[0] == 'draw':
            if event[1] != 0x218:
                raise OracleError('a draw on another stream')
            events.append(['draw'] + event[2:])
        else:
            events.append(event)
    return dict(label=label, side=side, tech=tech, difficulty=difficulty, center=list(center),
                rect=list(rect), buildings=[[name, list(cell)] for name, cell in buildings],
                enemy=enemy, values=list(values), fudge=list(fudge),
                nodes=[list(node) for node in nodes], index=index, wall_tower=wall_tower,
                site=list(site), result=result & 0xFF, events=events,
                nodes_after=read_nodes(emu), grid_after=emu.read32(HOUSE + 0x16060))


def choose():
    generator = random.Random(0x506EF0)
    rows = []
    base = [('YARD', (30, 30)), ('POWR', (26, 27)), ('TECH', (34, 33))]
    rows.append(choose_row('no defenses, no enemy', generator, buildings=base))
    rows.append(choose_row('no buildings', generator))
    rows.append(choose_row('no power: no candidates need it', generator,
                           buildings=[('YARD', (30, 30))]))
    for side in (0, 1, 2, 3):
        rows.append(choose_row(f'side {side}', generator, side=side, buildings=base))
    rows.append(choose_row('low tech', generator, tech=2, buildings=base))
    rows.append(choose_row('owned pill north', generator,
                           buildings=base + [('PILL', (30, 24))]))
    rows.append(choose_row('owned sams east and west', generator,
                           buildings=base + [('SAM', (36, 30)), ('SAM', (24, 31))]))
    rows.append(choose_row('owned coverage everywhere', generator,
                           buildings=base + [('GGUN', (30, 24)), ('GGUN', (36, 30)),
                                             ('GGUN', (30, 36)), ('TESL', (24, 30)),
                                             ('NEGA', (29, 25))]))
    rows.append(choose_row('a defence type as the only own building', generator,
                           buildings=[('PILL', (31, 30)), ('TECHD', (28, 28))]))
    rows.append(choose_row('a wall-like own building with values', generator,
                           buildings=base + [('WALLD', (27, 33))]))
    rows.append(choose_row('wall tower is not an own type', generator,
                           buildings=[('YARD', (30, 30)), ('WALLT', (28, 27))]))
    for policy in ('low', 'high', 'random'):
        rows.append(choose_row(f'enemy air, {policy} draws', generator, buildings=base,
                               enemy=1, values=(500, 800, 9000), policy=policy))
        rows.append(choose_row(f'enemy armour, {policy} draws', generator, buildings=base,
                               enemy=1, values=(500, 9000, 800), policy=policy))
        rows.append(choose_row(f'enemy infantry, {policy} draws', generator, buildings=base,
                               enemy=1, values=(9000, 800, 500), policy=policy))
    rows.append(choose_row('side 2 with an air threat and a gun', generator, side=2,
                           buildings=base, enemy=0, values=(0, 0, 20000)))
    rows.append(choose_row('side 1 armour threat, no armour candidate beyond bunker', generator,
                           side=1, buildings=[('YARD', (30, 30)), ('TECH', (34, 33))],
                           enemy=0, values=(0, 20000, 0)))
    rows.append(choose_row('site search fails', generator, buildings=base, site=(0, 0)))
    rows.append(choose_row('site with only a y', generator, buildings=base, site=(0, 9)))
    rows.append(choose_row('later node', generator, buildings=base,
                           nodes=((0, 30, 30), (1, 26, 27), (-1, 0, 0), (7, 0, 0)), index=2))
    t = TYPE_INDEX
    rows.append(choose_row('wall tower node, defense after it', generator, buildings=base,
                           nodes=((t['WALLT'], 0, 0), (-1, 0, 0), (7, 0, 0)), index=0))
    rows.append(choose_row('wall tower node, a type after it', generator, buildings=base,
                           nodes=((t['WALLT'], 0, 0), (t['SAM'], 0, 0)), index=0))
    rows.append(choose_row('wall tower node, last', generator, buildings=base,
                           nodes=((0, 30, 30), (t['WALLT'], 0, 0)), index=1))
    rows.append(choose_row('wall tower node, site fails', generator, buildings=base,
                           nodes=((t['WALLT'], 0, 0), (-1, 0, 0)), index=0, site=(0, 0)))
    rows.append(choose_row('no wall tower in the rules', generator, buildings=base,
                           wall_tower=None))
    for case in range(24):
        buildings = [('YARD', (30, 30))]
        for _ in range(generator.randrange(0, 12)):
            name = generator.choice(['POWR', 'TECH', 'PILL', 'SAM', 'TESL', 'GGUN', 'BUNK',
                                     'NEGA', 'WALLD', 'TECHD', 'HITK'])
            buildings.append((name, (generator.randrange(20, 41), generator.randrange(21, 40))))
        enemy = generator.choice((-1, 0, 3))
        rows.append(choose_row(f'random {case}', generator, side=generator.randrange(4),
                               tech=generator.randrange(1, 11),
                               difficulty=generator.randrange(3), buildings=buildings,
                               enemy=enemy,
                               values=tuple(generator.randrange(0, 20000) for _ in range(3)),
                               fudge=tuple(generator.randrange(0, 100) for _ in range(3)),
                               site=generator.choice([(33, 27), (0, 0), (25, 36)])))
    return rows


# ---------------------------------------------------------------- site

def defense_site():
    """FindBaseBuildingSite with the defense key: the coverage grid and the
    quadrant argument decide the order the perimeter is tried in."""
    generator = random.Random(0x505FD0 ^ 0x5060B0)
    rows = []
    square = {(x, y) for x in range(18, 23) for y in range(18, 23)}
    ring = perimeter_of(square)
    rect = (17, 17, 7, 7)
    for argument in (0, 2, 4, 6, -1, 5):
        grid = [generator.randrange(0, 40) for _ in range(rect[2] * rect[3])]
        rows.append(site_row(f'square, argument {argument}', center=(20, 20), perimeter=ring,
                             world=Map(reserved=square), key=DEFENSE_KEY, argument=argument,
                             rect=rect, grid=grid))
    flat = [0] * (rect[2] * rect[3])
    for argument in (0, 2, 4, 6):
        rows.append(site_row(f'square, flat grid, argument {argument}', center=(20, 20),
                             perimeter=ring, world=Map(reserved=square), key=DEFENSE_KEY,
                             argument=argument, rect=rect, grid=flat))
    for case in range(24):
        dense = case >= 12
        center = (generator.randrange(14, 50), generator.randrange(14, 50))
        blob = base_blob(generator, center, generator.randrange(4, 40))
        ring = perimeter_of(blob)
        generator.shuffle(ring)
        xs = [x for x, _ in blob]
        ys = [y for _, y in blob]
        # The reservation bounds grown by one: the perimeter lies inside.
        rect = (min(xs) - 1, min(ys) - 1, max(xs) - min(xs) + 3, max(ys) - min(ys) + 3)
        grid = [generator.randrange(0, 120) for _ in range(rect[2] * rect[3])]
        blocked = {(generator.randrange(64), generator.randrange(64))
                   for _ in range(generator.randrange(1500, 3000) if dense
                                  else generator.randrange(0, 400))}
        blocked -= blob
        rows.append(site_row(f'random {case}', center=center, perimeter=ring,
                             world=Map(reserved=blob, blocked=blocked),
                             size=generator.choice(((1, 1), (2, 2), (1, 2))),
                             spacing=generator.randrange(0, 3), key=DEFENSE_KEY,
                             argument=generator.choice((0, 2, 4, 6)), rect=rect, grid=grid))
    return rows


def generate():
    return {'source': 'unicorn/gamemd.exe', 'influence': influence(), 'key': key(),
            'threat': threat(), 'choose': choose(), 'site': defense_site()}


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope=('computer base defense choice: coverage spread, defense site key, enemy '
               'threat ratios and draws, whole AI_ChooseNextProduction runs without a '
               'perimeter vector, and whole FindBaseBuildingSite searches with the defense '
               'key on synthetic 64x64 maps'),
        assumptions=['fresh emulator per case; fixture House/Rules/Type layouts from live disassembly',
                     'x87 control word 0x0E7F (53-bit chop), the process word ftol callers assume',
                     'BuildingClass objects use the original vtable 0x7E3EBC'],
        substitutions=['RandomRanged 0x65C7E0 answers from a seeded generator within its range',
                       'HouseTypeClass::FindIndexOfName 0x5117D0 answers country 3',
                       'FindBaseBuildingSite 0x5060B0 answers the row site in the choose section',
                       'operator new/delete and atexit are fixture stubs',
                       'MapClass::operator[] 0x5657A0 answers synthetic cells; CheckOccupancy 0x586780 and CanPlaceAt vt+0xA8 answer a blocked-cell/unplaceable-site model'],
        entry_points={'coverage_spread': 0x506D50, 'defense_key': DEFENSE_KEY,
                      'AI_UpdateEnemyThreatRatios': 0x508150,
                      'AI_ChooseNextProduction': 0x506EF0,
                      'FindBaseBuildingSite': SITE_SEARCH}))
