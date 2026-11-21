#!/usr/bin/env python3
"""Write a flat square TEMPERATE map for sim-bench scale runs, using only stdlib.

Usage:
    python -m tools.scale_map target/scale/flat500.map
    python -m tools.scale_map target/scale/flat200.map --size 100
    python -m tools.scale_map target/scale/match500.map --starts 30
    python -m tools.scale_map target/scale/battle30.map --starts 30 --battle

`--size N` writes `Size=0,0,N,N`. Its diamond spans 2N cells on each of the
rx and ry axes, so the default N=250 is a 500x500-cell map. gamemd's cell
array is 512 wide, which bounds N at 256. Every cell is clear level ground.
Larger maps automatically select the extended 1024-cell array (maximum N=500,
whose cells the y*1000+x [Terrain] and [Waypoints] keys can still name).
For example --size 500 writes a literal Size=0,0,500,500 map.

Without `--starts`, the eight start waypoints sit on a ring and the map has no
overlays or terrain objects: sim-bench's march scenario adds its own houses.

`--starts N` writes the map for sim-bench's match scenario instead: N start
waypoints on a grid across the diamond, an ore field beside each start, and
clumps of trees between the starts. Above eight starts it also
writes `[Basic] MaxPlayers=N`, so the lobby seats a player at each. A fixed seed places the ore and trees, so
the same arguments always write the same map.

`--battle` (with `--starts`) also writes the scale battle: a few Rules keys,
more waypoints, and the teams, scripts, triggers and Tags that give every
start's house an army, send its squads to patrol the fronts it shares with its
neighbours and replace them as they fall (see `tools.scale_battle`).
`--battle N` fields N units a house instead of that module's UNITS_PER_HOUSE.

`--ore-fields N` rings each start with N fields instead of one, spread evenly
round it, and `--ore-radius R` sets a field's radius in cells; both leave a
field where the committed maps have it when left alone. A crowded map wants
more: 169 starts on `--size 500` hold 94 ore cells each with one field and 458
with `--ore-fields 3 --ore-radius 7`.
"""

import argparse
import math
import random
import struct
from pathlib import Path

from tools.render_depth_fixture import pack_lcw, pack_lzo
from tools.scale_battle import UNITS_PER_HOUSE, battle_sections

CLEAR_TILE = 0
# TIB01..TIB20, the ore overlays: their positions in the retail [OverlayTypes] list.
ORE_OVERLAYS = range(102, 122)
# Ore growth stages run 0..11 in [OverlayDataPack].
ORE_DENSITIES = range(3, 12)
# TEMPERATE trees, all 1x1 in the retail artmd.ini.
TREES = [f'TREE{n:02}' for n in range(1, 21)]
CELL_ARRAY_WIDTH = 512
# Match starts at least this many cells apart. sim-bench builds a base about
# 11 cells across around each start; a field a third of this out, a tenth of it
# wide, keeps its near edge 12 cells from the start, and the trees stay
# 0.42 of it away.
MIN_PITCH = 52


def in_diamond(size: int, x: int, y: int) -> bool:
    # The Size=0,0,N,N diamond (see render_depth_fixture's N=48 map).
    return size < x + y <= 3 * size and abs(x - y) < size


def ring_starts(size: int) -> list[tuple[int, int]]:
    starts = []
    for index in range(8):
        angle = 2 * math.pi * index / 8
        # Screen-aligned ring: u = rx - ry, v = rx + ry around the centre.
        u = round(0.7 * size * math.cos(angle))
        v = 2 * size + round(0.7 * size * math.sin(angle))
        starts.append(((v + u) // 2, (v - u) // 2))
    return starts


def grid_starts(size: int, count: int) -> tuple[list[tuple[int, int]], float]:
    """Starts at the centres of a near-square grid over the diamond, and the
    grid's smaller pitch in cells.

    In screen-aligned (u, v) = (rx - ry, rx + ry) space the diamond is the
    rectangle |u| < size, size < v <= 3 size, so the grid is laid out there.
    """
    columns = math.ceil(math.sqrt(count))
    rows = math.ceil(count / columns)
    u_pitch, v_pitch = 2 * size / columns, 2 * size / rows
    starts = []
    for index in range(count):
        row, column = divmod(index, columns)
        u = round(-size + (column + 0.5) * u_pitch)
        v = round(size + (row + 0.5) * v_pitch)
        # rx and ry are whole cells only when u and v share their parity.
        v += (u + v) % 2
        starts.append(((v + u) // 2, (v - u) // 2))
    # One step in u or in v moves 1/sqrt(2) of a cell along each axis.
    return starts, min(u_pitch, v_pitch) / math.sqrt(2)


def match_layout(
    size: int,
    starts: list[tuple[int, int]],
    pitch: float,
    *,
    ore_fields: int = 1,
    ore_radius: float | None = None,
):
    """Ore cells {(rx, ry): (overlay, density)} and tree cells {(rx, ry): name}.

    `ore_fields` fields ring each start, the first a third of the pitch south
    of it on screen (+v) as the committed maps have it; extra fields sit at
    equal angles round the same distance. `ore_radius` overrides the field's
    own radius, which is a tenth of the pitch by default.
    """
    rng = random.Random(0x5CA1E)
    ore = {}
    # Each field lies a third of the pitch from its start, clear of the base
    # sim-bench builds around the start.
    offset, radius = pitch / 3, ore_radius if ore_radius is not None else max(2.0, pitch / 10)
    for rx, ry in starts:
        centres = []
        if ore_fields == 1:
            # The committed maps' single field: due south, offset/√2 per axis.
            centres.append((rx + offset / math.sqrt(2), ry + offset / math.sqrt(2)))
        else:
            for field in range(ore_fields):
                angle = math.pi / 2 + 2 * math.pi * field / ore_fields
                dv, du = offset * math.sin(angle), offset * math.cos(angle)
                centres.append((rx + (dv + du) / 2, ry + (dv - du) / 2))
        reach = math.ceil(radius)
        for centre in centres:
            for y in range(int(centre[1]) - reach, int(centre[1]) + reach + 2):
                for x in range(int(centre[0]) - reach, int(centre[0]) + reach + 2):
                    if in_diamond(size, x, y) and math.dist((x, y), centre) <= radius:
                        ore[(x, y)] = (rng.choice(ORE_OVERLAYS), rng.choice(ORE_DENSITIES))
    trees = {}
    # Clumps on a coarse lattice, kept off the bases and armies around each
    # start and off the ore and the cells beside it.
    clear = 0.42 * pitch
    step = max(4, round(pitch / 7))
    for cy in range(step // 2, 2 * size, step):
        for cx in range(step // 2, 2 * size, step):
            if not in_diamond(size, cx, cy) or rng.random() >= 0.35:
                continue
            spread = rng.choice((1, 1.5, 2))
            for y in range(cy - 2, cy + 3):
                for x in range(cx - 2, cx + 3):
                    if (
                        in_diamond(size, x, y)
                        and math.dist((x, y), (cx, cy)) <= spread
                        and all(math.dist((x, y), start) > clear for start in starts)
                        and not any((x + dx, y + dy) in ore for dx in (-1, 0, 1) for dy in (-1, 0, 1))
                    ):
                        trees[(x, y)] = rng.choice(TREES)
    return ore, trees


def build_map(
    size: int,
    starts: int | None = None,
    *,
    cell_array_width: int | None = None,
    ore_fields: int = 1,
    ore_radius: float | None = None,
    battle: int | None = None,
) -> str:
    if cell_array_width is None:
        cell_array_width = 512 if size <= 256 else 1024
    # [Terrain] and [Waypoints] keys are y*1000+x, so x must stay below 1000.
    if cell_array_width not in (512, 1024) or not 16 <= size <= min(cell_array_width // 2, 500):
        raise ValueError('size must fit inside half the selected 512 or 1024 cell array, '
                         'and at most 500 for y*1000+x cell keys')
    if starts is not None and starts < 2:
        raise ValueError('a match needs at least two starts')
    if battle is not None and starts is None:
        raise ValueError('a battle needs the match map: give starts')
    cells = bytearray()
    for y in range(1, 2 * size):
        for x in range(1, 2 * size):
            if in_diamond(size, x, y):
                cells += struct.pack('<hHiBBB', x, y, CLEAR_TILE, 0, 0, 0)
    overlay = bytearray(b'\xff' * cell_array_width * cell_array_width)
    overlay_data = bytearray(cell_array_width * cell_array_width)
    terrain = ''
    battle_waypoints, battle_text = {}, ''
    if starts is None:
        start_cells = ring_starts(size)
        name = f'Scale Flat {2 * size}x{2 * size}'
    else:
        start_cells, pitch = grid_starts(size, starts)
        if pitch < MIN_PITCH:
            raise ValueError(f'{starts} starts on size {size} are {pitch:.1f} cells apart; '
                             f'a match base needs {MIN_PITCH}')
        ore, trees = match_layout(size, start_cells, pitch, ore_fields=ore_fields, ore_radius=ore_radius)
        for (x, y), (kind, density) in ore.items():
            overlay[y * cell_array_width + x] = kind
            overlay_data[y * cell_array_width + x] = density
        terrain = '\n[Terrain]\n' + ''.join(
            f'{y * 1000 + x}={tree}\n' for (x, y), tree in sorted(trees.items(), key=lambda item: (item[0][1], item[0][0]))
        )
        name = f'Scale Match {2 * size}x{2 * size} {starts} starts'
        if battle is not None:
            battle_waypoints, battle_text = battle_sections(
                size, start_cells, set(ore) | set(trees), battle)
            battle_text = '\n' + battle_text
            name = f'Scale Battle {2 * size}x{2 * size} {starts} starts'
    if cell_array_width > 512:
        name = f'Large Size {size}x{size} - {len(start_cells)} Starts'
    waypoints = '\n'.join(
        f'{index}={ry * 1000 + rx}'
        for index, (rx, ry) in (dict(enumerate(start_cells)) | battle_waypoints).items())
    # VERA's lobby seats more than the eight retail starts only on a map that
    # declares them (`skirmish_player_capacity`: MaxPlayers 9 to 30).
    max_players = f'MaxPlayers={len(start_cells)}\n' if len(start_cells) > 8 else ''
    margin = 4
    return f'''[Basic]
Name={name}
{max_players}NewINIFormat=4
MultiplayerOnly=1
Official=no

[Map]
Theater=TEMPERATE
Size=0,0,{size},{size}
LocalSize={margin // 2},{margin},{size - margin},{size - 2 * margin}

[Lighting]
Ambient=1.0
Red=1.0
Green=1.0
Blue=1.0
Ground=0.0
Level=0.032

[Waypoints]
{waypoints}
{terrain}
[OverlayPack]
{pack_lcw(bytes(overlay))}

[OverlayDataPack]
{pack_lcw(bytes(overlay_data))}

[IsoMapPack5]
{pack_lzo(bytes(cells))}
{battle_text}'''


def main() -> None:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument('output', type=Path, help='Map path to write')
    parser.add_argument('--size', type=int, default=250, help='Size= width and height (default 250)')
    parser.add_argument('--starts', type=int, help='Grid starts with ore and trees, for the match scenario')
    parser.add_argument('--battle', type=int, nargs='?', const=UNITS_PER_HOUSE, metavar='UNITS',
                        help=f'with --starts: the scale battle, UNITS a house (default {UNITS_PER_HOUSE})')
    parser.add_argument('--cell-array-width', type=int, choices=(512, 1024), default=None,
                        help='overlay encoding width (default: select from map Size)')
    parser.add_argument('--ore-fields', type=int, default=1,
                        help='ore fields ringing each start (default 1, as the committed maps)')
    parser.add_argument('--ore-radius', type=float, default=None,
                        help='ore field radius in cells (default: a tenth of the start pitch)')
    args = parser.parse_args()
    if args.battle is not None and args.starts is None:
        parser.error('--battle needs --starts')
    args.output.parent.mkdir(parents=True, exist_ok=True)
    # Explicit LF: the map is committed and regenerated byte-identically, so
    # the writer must not follow the host platform's newline translation.
    args.output.write_text(
        build_map(
            args.size,
            args.starts,
            cell_array_width=args.cell_array_width,
            ore_fields=args.ore_fields,
            ore_radius=args.ore_radius,
            battle=args.battle,
        ),
        encoding='ascii',
        newline='\n',
    )


if __name__ == '__main__':
    main()
