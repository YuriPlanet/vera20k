"""MapClass's area reveals, 0x5678E0 and RevealShroud 0x5673A0: the cells
their walk visits.

Run python -B -m tools.spatial_oracle.reveal_area --check / --write.
Rust consumer: src/sim/vision/reveal_area_tests.rs.

Each row calls one of them (thiscall on the Map 0x87F7E8, RET 0x20) with
(coord*, radius, house, 0, 0, 0, by_height, final) and records, in call
order, each MapCell 0x653830 call (the cell, the house it is handed,
`final`) and each CellClass::Unshroud 0x4876F0. 0x5673A0's closing fog
border call 0x567DA0 is a silent stub. The two static offset tables,
0xABD490 (cells) and 0xABCF60 (their line-of-sight neighbours), come from
their initializers 0x561910 and 0x5638D0. The map is a fixture `Size=`
diamond with a CellClass (+0x24 coordinates, +0x11B level) at every
in-bounds cell near the centre. HouseClass::IsAlliedWith 0x4F9A50 is a
recorded stub; AdjustForZ 0x6D20E0, the square root 0x4CAC40 and ftol
0x7C5F00 run natively.
"""
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_ECX

from tools.ai_base_building_oracle import FAKE, RULES, Emu
from tools.native_oracle import finish_vectors, provenance

MAP = 0x87F7E8
REVEAL_AREA = 0x5678E0
REVEAL_SHROUD = 0x5673A0
CELL_OFFSETS_INIT = 0x561910
LOS_OFFSETS_INIT = 0x5638D0
PLAYER_PTR = 0xA83D4C
GAME_MODE = 0xA8B238
SESSION_PTR = 0xA8B23C
LEVEL_LEPTONS_PTR = 0xABDE88
Z_MULTIPLIER = 0xB0CD48
STANDARD_Z_MULTIPLIER_BITS = 0x3FC25E5374344960

CELL_POINTERS = 0x70000000
CELLS = 0x71000000
CELL_STRIDE = 0x200
HOUSES = 0x72000000
HOUSE_STRIDE = 0x8000
PLAYER = HOUSES
OTHER = HOUSES + HOUSE_STRIDE
HOUSE_TYPES = HOUSES + 2 * HOUSE_STRIDE
COORD = FAKE + 0x7F1000

PLAYER_INDEX = 3
SIZE = (50, 50)
WINDOW = 24
LEVEL_LEPTONS = 104


def in_diamond(size, x, y):
    width, height = size
    return width < x + y and x - y < width and y - x < width and x + y <= width + 2 * height


def reveal_area_row(*, function='area', center=(75, 50), z=0, radius=10, house='player',
                    by_height=False, final=0, reveal_by_height=True, ally_reveal=True,
                    allied=False, size=SIZE, levels=None):
    """`function` is 'area' (0x5678E0) or 'shroud' (RevealShroud 0x5673A0,
    whose cells take CellClass::Unshroud 0x4876F0 unless `final` is set).
    `center` is the coordinate's cell, `z` its height in leptons; `levels`
    gives a cell's +0x11B level. `house` is 'player' (the current player,
    0xA83D4C) or 'other', whose IsAlliedWith answers `allied`. The outline
    and unreveal arguments are 0, GameMode is 1 and no house holds a +0x54E4
    bit."""
    levels = levels or {}
    emu = Emu()
    uc = emu.uc
    uc.mem_map(CELL_POINTERS, 0x100000)
    uc.mem_map(CELLS, 0x200000)
    uc.mem_map(HOUSES, 0x20000)
    emu.invoke(CELL_OFFSETS_INIT)
    emu.invoke(LOS_OFFSETS_INIT)
    emu.write32(LEVEL_LEPTONS_PTR, LEVEL_LEPTONS)
    uc.mem_write(Z_MULTIPLIER, struct.pack('<Q', STANDARD_Z_MULTIPLIER_BITS))
    emu.write32(MAP + 0xF4, size[0])
    emu.write32(MAP + 0xF8, size[1])
    emu.write32(MAP + 0x13C, CELL_POINTERS)
    uc.mem_write(RULES + 0x17E7, bytes([int(ally_reveal)]))
    uc.mem_write(RULES + 0x17EE, bytes([int(reveal_by_height)]))
    emu.write32(PLAYER_PTR, PLAYER)
    emu.write32(PLAYER + 0x34, HOUSE_TYPES)
    emu.write32(HOUSE_TYPES + 0xB8, PLAYER_INDEX)
    emu.write32(OTHER + 0x34, HOUSE_TYPES + 0x100)
    emu.write32(HOUSE_TYPES + 0x100 + 0xB8, PLAYER_INDEX + 1)
    emu.write32(OTHER + 0x54E4, 0)
    emu.write32(GAME_MODE, 1)
    emu.write32(SESSION_PTR, 0)
    cells = {}
    for y in range(center[1] - WINDOW, center[1] + WINDOW + 1):
        for x in range(center[0] - WINDOW, center[0] + WINDOW + 1):
            if not (0 <= x < 512 and 0 <= y < 512 and in_diamond(size, x, y)):
                continue
            this = CELLS + CELL_STRIDE * len(cells)
            uc.mem_write(this + 0x24, struct.pack('<hh', x, y))
            uc.mem_write(this + 0x11B, struct.pack('<b', levels.get((x, y), 0)))
            emu.write32(CELL_POINTERS + 4 * (y * 512 + x), this)
            cells[(x, y)] = this
    names = {PLAYER: 'player', OTHER: 'other'}

    def leaf(e):
        x, y = struct.unpack('<hh', e.uc.mem_read(e.arg(0), 4))
        e.events.append(['leaf', [x, y], names.get(e.arg(1), hex(e.arg(1))), e.arg(2) & 0xFF])
        return 1

    def unreveal_leaf(e):
        x, y = struct.unpack('<hh', e.uc.mem_read(e.arg(0), 4))
        e.events.append(['unreveal', f'{x},{y}', names.get(e.arg(1), hex(e.arg(1)))])
        return 0

    def is_allied(e):
        e.events.append(['allied', names.get(e.uc.reg_read(UC_X86_REG_ECX)), names.get(e.arg(0))])
        return int(allied)

    def unshroud(e):
        x, y = struct.unpack('<hh', e.uc.mem_read(e.uc.reg_read(UC_X86_REG_ECX) + 0x24, 4))
        e.events.append(['unshroud', [x, y]])

    emu.hook(0x653830, leaf, 0xC)
    emu.hook(0x4A9DD0, unreveal_leaf, 8)
    emu.hook(0x4F9A50, is_allied, 4)
    emu.hook(0x4876F0, unshroud, 0)
    emu.hook(0x567DA0, lambda _e: None, 0x10)
    uc.mem_write(COORD, struct.pack('<iii', center[0] * 256 + 128, center[1] * 256 + 128, z))
    house_ptr = {'player': PLAYER, 'other': OTHER}[house]
    entry = {'area': REVEAL_AREA, 'shroud': REVEAL_SHROUD}[function]
    emu.invoke(entry, ecx=MAP, args=[COORD, radius, house_ptr, 0, 0, 0, int(by_height), final])
    leaves = [event for event in emu.events if event[0] == 'leaf']
    handed = {(event[2], event[3]) for event in leaves}
    if len(handed) > 1:
        raise ValueError(f'one walk hands its leaf several houses or finals: {handed}')
    walked = [tuple(event[1]) for event in emu.events if event[0] in ('leaf', 'unshroud')]
    if missing := [cell for cell in walked if cell not in cells]:
        raise ValueError(f'walked cells outside the fixture window: {missing}')
    return dict(function=function, center=list(center), z=z, radius=radius, house=house,
                by_height=by_height, final=final, reveal_by_height=reveal_by_height,
                ally_reveal=ally_reveal, allied=allied, size=list(size), levels=cell_list(levels),
                leaf=cell_text(tuple(event[1]) for event in leaves),
                leaf_house=leaves[0][2] if leaves else None,
                leaf_final=leaves[0][3] if leaves else None,
                unshroud=cell_text(tuple(event[1]) for event in emu.events
                                   if event[0] == 'unshroud'),
                calls=[event for event in emu.events if event[0] not in ('leaf', 'unshroud')])


def cell_text(cells):
    """Cells as `x,y` pairs separated by spaces, in order."""
    return ' '.join(f'{x},{y}' for x, y in cells)


def cell_list(values):
    """A per-cell map as `x,y=value` items separated by spaces, sorted."""
    return ' '.join(f'{x},{y}={value}' for (x, y), value in sorted(values.items()))


def ridge(x_from, x_to, y_from, y_to, level):
    return {(x, y): level for x in range(x_from, x_to + 1) for y in range(y_from, y_to + 1)}


def rows():
    out = []
    # The Psychic Reveal's two calls (final 0, then 1) and every radius.
    for final in (0, 1):
        for radius in (*range(12), 15):
            out.append(reveal_area_row(radius=radius, final=final))
    # Height lifts the centre toward the map's north by AdjustForZ / 30 cells.
    for z in (52, 104, 208, 312, 416, 624, 832, 1040, 1500, 2000):
        out.append(reveal_area_row(z=z))
    # The Size= diamond bounds each cell and, before the walk, the lifted
    # centre: each side, and a centre a lift takes off the map.
    for center in ((30, 25), (28, 24), (85, 63), (87, 63), (70, 23), (73, 25), (23, 70),
                   (25, 73), (20, 20), (26, 26)):
        out.append(reveal_area_row(center=center))
    out.append(reveal_area_row(center=(27, 27), z=416))
    out.append(reveal_area_row(center=(27, 27), z=416, radius=3))
    # The house gate: another house reveals the player's map only when it is
    # allied with the player (IsAlliedWith asked of it) and AllyReveal= is set;
    # the leaf is then handed the player.
    for allied in (False, True):
        for ally_reveal in (False, True):
            out.append(reveal_area_row(house='other', allied=allied, ally_reveal=ally_reveal,
                                       radius=2))
    # RevealByHeight= with the by-height argument: a cell whose line-of-sight
    # neighbour stands more than three levels above the viewer is skipped.
    hills = ridge(78, 79, 40, 60, 4)
    for z in (0, 104, 416):
        for by_height, rule in ((True, True), (True, False), (False, True)):
            out.append(reveal_area_row(z=z, levels=hills, by_height=by_height,
                                       reveal_by_height=rule))
    out.append(reveal_area_row(z=1500, levels=ridge(70, 80, 44, 46, 9), by_height=True))
    # RevealShroud 0x5673A0 with FireAt's arguments (radius 3, by height,
    # 0x006FF6FC..0x006FF71B) shares the walk.
    for z in (0, 416, 1500):
        out.append(reveal_area_row(function='shroud', radius=3, by_height=True, z=z))
    for center in ((30, 25), (28, 24), (85, 63), (87, 63), (20, 20)):
        out.append(reveal_area_row(function='shroud', radius=3, by_height=True, center=center))
    out.append(reveal_area_row(function='shroud', radius=3, by_height=True, center=(27, 27),
                               z=416))
    out.append(reveal_area_row(function='shroud', radius=3, by_height=True,
                               levels=ridge(77, 77, 48, 52, 4)))
    return out


if __name__ == '__main__':
    finish_vectors(lambda: {'rows': rows()}, Path(__file__).with_suffix('.json'),
                   provenance=lambda: provenance(
        scope='MapClass area reveals 0x5678E0 and RevealShroud 0x5673A0: the cells their walk '
              'hands its leaves, and the house',
        assumptions=[
            'The offset tables 0xABD490 and 0xABCF60 come from their static initializers '
            '0x561910 and 0x5638D0',
            'Map Size= is the fixture diamond (MapClass+0xF4/+0xF8); every in-bounds cell '
            'within 24 of the centre is a fixture CellClass (+0x24, +0x11B), others are absent',
            'Rules+0x17E7 AllyReveal= and +0x17EE RevealByHeight= are written; 0xABDE88 is 104 '
            'and 0xB0CD48 the standard Z multiplier; GameMode 1, no session object, no +0x54E4 '
            'bit; the outline and unreveal arguments are 0',
            'MapCell 0x653830, Unshroud 0x4876F0, the fog border 0x567DA0, the unreveal leaf '
            '0x4A9DD0 and IsAlliedWith 0x4F9A50 are recorded stubs; their effects are not '
            'compared',
        ],
        substitutions=['0x653830', '0x4876F0', '0x567DA0', '0x4A9DD0', '0x4F9A50'],
        entry_points={'reveal_area': REVEAL_AREA, 'reveal_shroud': REVEAL_SHROUD,
                      'cell_offsets': CELL_OFFSETS_INIT, 'los_offsets': LOS_OFFSETS_INIT}))
