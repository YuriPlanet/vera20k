#!/usr/bin/env python3
"""Write a flat square TEMPERATE map for sim-bench scale runs, using only stdlib.

Usage:
    python -m tools.scale_map target/scale/flat500.map
    python -m tools.scale_map target/scale/flat200.map --size 100
    python -m tools.scale_map target/scale/match500.map --starts 30

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


def _build_clear_diamond_data(
    size: int,
    cell_array_width: int | None = None,
) -> tuple[int, bytearray, bytearray, bytearray]:
    if cell_array_width is None:
        cell_array_width = 512 if size <= 256 else 1024
    # [Terrain] and [Waypoints] keys are y*1000+x, so x must stay below 1000.
    if cell_array_width not in (512, 1024) or not 16 <= size <= min(cell_array_width // 2, 500):
        raise ValueError('size must fit inside half the selected 512 or 1024 cell array, '
                         'and at most 500 for y*1000+x cell keys')
    cells = bytearray()
    for y in range(1, 2 * size):
        for x in range(1, 2 * size):
            if in_diamond(size, x, y):
                cells += struct.pack('<hHiBBB', x, y, CLEAR_TILE, 0, 0, 0)
    overlay = bytearray(b'\xff' * cell_array_width * cell_array_width)
    overlay_data = bytearray(cell_array_width * cell_array_width)
    return cell_array_width, cells, overlay, overlay_data


def _render_clear_map(
    *,
    basic_section: str,
    size: int,
    middle_sections: str,
    overlay: bytearray,
    overlay_data: bytearray,
    cells: bytearray,
) -> str:
    margin = 4
    return (
        f'{basic_section}\n'
        f'[Map]\n'
        f'Theater=TEMPERATE\n'
        f'Size=0,0,{size},{size}\n'
        f'LocalSize={margin // 2},{margin},{size - margin},{size - 2 * margin}\n\n'
        f'[Lighting]\n'
        f'Ambient=1.0\n'
        f'Red=1.0\n'
        f'Green=1.0\n'
        f'Blue=1.0\n'
        f'Ground=0.0\n'
        f'Level=0.032\n\n'
        f'{middle_sections}'
        f'[OverlayPack]\n'
        f'{pack_lcw(bytes(overlay))}\n\n'
        f'[OverlayDataPack]\n'
        f'{pack_lcw(bytes(overlay_data))}\n\n'
        f'[IsoMapPack5]\n'
        f'{pack_lzo(bytes(cells))}\n'
    )


def build_vmce_minimal_map(size: int = 32) -> str:
    """Build a single-player minimal VMCE map with one House and one straight-walking MTNK."""
    if size < 24:
        raise ValueError('VMCE minimal map requires size >= 24 for centered walkable LocalSize bounds')
    _, cells, overlay, overlay_data = _build_clear_diamond_data(size)
    start_x, start_y = size, size
    goal_x, goal_y = size + 4, size + 4
    basic_section = (
        '[Basic]\n'
        'Name=VMCE Minimal Stage 01\n'
        'NewINIFormat=4\n'
        'Player=HouseAlpha\n'
        'HomeCell=0\n'
        'AltHomeCell=0\n'
        'MultiplayerOnly=no\n'
        'Official=no\n'
    )
    middle_sections = (
        '[Houses]\n'
        '0=HouseAlpha\n\n'
        '[HouseAlpha]\n'
        'Country=Americans\n'
        'Side=GDI\n'
        'PlayerControl=yes\n'
        'IQ=0\n'
        'TechLevel=10\n'
        'Credits=5000\n'
        'Edge=North\n'
        'Allies=HouseAlpha\n'
        'Color=Gold\n\n'
        '[Waypoints]\n'
        f'0={start_y * 1000 + start_x}\n'
        f'1={goal_y * 1000 + goal_x}\n\n'
        '[Units]\n'
        f'0=HouseAlpha,MTNK,256,{start_x},{start_y},128,Guard,None,0,-1,0,-1,1,1\n\n'
    )
    return _render_clear_map(
        basic_section=basic_section,
        size=size,
        middle_sections=middle_sections,
        overlay=overlay,
        overlay_data=overlay_data,
        cells=cells,
    )


def build_map(
    size: int,
    starts: int | None = None,
    *,
    cell_array_width: int | None = None,
    ore_fields: int = 1,
    ore_radius: float | None = None,
) -> str:
    cell_array_width, cells, overlay, overlay_data = _build_clear_diamond_data(size, cell_array_width)
    if starts is not None and starts < 2:
        raise ValueError('a match needs at least two starts')
    terrain = ''
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
    if cell_array_width > 512:
        name = f'Large Size {size}x{size} - {len(start_cells)} Starts'
    waypoints = '\n'.join(f'{index}={ry * 1000 + rx}' for index, (rx, ry) in enumerate(start_cells))
    # Declare additional starts explicitly. The game applies each mode's
    # player limit when selecting the map.
    max_players = f'MaxPlayers={len(start_cells)}\n' if len(start_cells) > 8 else ''
    basic_section = (
        '[Basic]\n'
        f'Name={name}\n'
        f'{max_players}NewINIFormat=4\n'
        'MultiplayerOnly=1\n'
        'Official=no\n'
    )
    middle_sections = f'[Waypoints]\n{waypoints}\n{terrain}\n'
    return _render_clear_map(
        basic_section=basic_section,
        size=size,
        middle_sections=middle_sections,
        overlay=overlay,
        overlay_data=overlay_data,
        cells=cells,
    )


def main() -> None:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument('output', type=Path, help='Map path to write')
    parser.add_argument('--size', type=int, default=250, help='Size= width and height (default 250)')
    parser.add_argument('--starts', type=int, help='Grid starts with ore and trees, for the match scenario')
    parser.add_argument('--cell-array-width', type=int, choices=(512, 1024), default=None,
                        help='overlay encoding width (default: select from map Size)')
    parser.add_argument('--vmce-minimal', action='store_true',
                        help='Write a single-player minimal VMCE map with one House and one straight-walking MTNK')
    parser.add_argument('--ore-fields', type=int, default=1,
                        help='ore fields ringing each start (default 1, as the committed maps)')
    parser.add_argument('--ore-radius', type=float, default=None,
                        help='ore field radius in cells (default: a tenth of the start pitch)')
    args = parser.parse_args()
    if args.vmce_minimal and (args.starts is not None or args.cell_array_width is not None):
        parser.error('--vmce-minimal cannot be combined with --starts or --cell-array-width')
    content = (
        build_vmce_minimal_map(args.size)
        if args.vmce_minimal
        else build_map(
            args.size,
            args.starts,
            cell_array_width=args.cell_array_width,
            ore_fields=args.ore_fields,
            ore_radius=args.ore_radius,
        )
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    # Explicit LF: the map is committed and regenerated byte-identically, so
    # the writer must not follow the host platform's newline translation.
    args.output.write_text(content, encoding='ascii', newline='\n')


if __name__ == '__main__':
    main()
