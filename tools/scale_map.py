#!/usr/bin/env python3
"""Write a flat square TEMPERATE map for sim-bench scale runs, using only stdlib.

Usage:
    python -m tools.scale_map target/scale/flat500.map
    python -m tools.scale_map target/scale/flat200.map --size 100
    python -m tools.scale_map target/scale/match500.map --starts 30

`--size N` writes `Size=0,0,N,N`. Its diamond spans 2N cells on each of the
rx and ry axes, so the default N=250 is a 500x500-cell map. gamemd's cell
array is 512 wide, which bounds N at 256. Every cell is clear level ground.

Without `--starts`, the eight start waypoints sit on a ring and the map has no
overlays or terrain objects: sim-bench's march scenario adds its own houses.

`--starts N` writes the map for sim-bench's match scenario instead: N start
waypoints on a grid across the diamond, an ore field beside each start, and
clumps of trees between the starts. A fixed seed places the ore and trees, so
the same arguments always write the same map.
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


def match_layout(size: int, starts: list[tuple[int, int]], pitch: float):
    """Ore cells {(rx, ry): (overlay, density)} and tree cells {(rx, ry): name}."""
    rng = random.Random(0x5CA1E)
    ore = {}
    # Each field lies a third of the pitch south of its start on screen (+v),
    # clear of the base sim-bench builds around the start.
    offset, radius = pitch / 3, max(2.0, pitch / 10)
    for rx, ry in starts:
        centre = (rx + offset / math.sqrt(2), ry + offset / math.sqrt(2))
        reach = math.ceil(radius)
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


def build_map(size: int, starts: int | None = None) -> str:
    if not 16 <= size <= 256:
        raise ValueError('size must be 16..256: a playfield inside the margin, and the '
                         'diamond inside the 512-wide cell array')
    if starts is not None and starts < 2:
        raise ValueError('a match needs at least two starts')
    cells = bytearray()
    for y in range(1, 2 * size):
        for x in range(1, 2 * size):
            if in_diamond(size, x, y):
                cells += struct.pack('<hHiBBB', x, y, CLEAR_TILE, 0, 0, 0)
    overlay = bytearray(b'\xff' * CELL_ARRAY_WIDTH * CELL_ARRAY_WIDTH)
    overlay_data = bytearray(CELL_ARRAY_WIDTH * CELL_ARRAY_WIDTH)
    terrain = ''
    if starts is None:
        start_cells = ring_starts(size)
        name = f'Scale Flat {2 * size}x{2 * size}'
    else:
        start_cells, pitch = grid_starts(size, starts)
        if pitch < MIN_PITCH:
            raise ValueError(f'{starts} starts on size {size} are {pitch:.1f} cells apart; '
                             f'a match base needs {MIN_PITCH}')
        ore, trees = match_layout(size, start_cells, pitch)
        for (x, y), (kind, density) in ore.items():
            overlay[y * CELL_ARRAY_WIDTH + x] = kind
            overlay_data[y * CELL_ARRAY_WIDTH + x] = density
        terrain = '\n[Terrain]\n' + ''.join(
            f'{y * 1000 + x}={tree}\n' for (x, y), tree in sorted(trees.items(), key=lambda item: (item[0][1], item[0][0]))
        )
        name = f'Scale Match {2 * size}x{2 * size} {starts} starts'
    waypoints = '\n'.join(f'{index}={ry * 1000 + rx}' for index, (rx, ry) in enumerate(start_cells))
    margin = 4
    return f'''[Basic]
Name={name}
NewINIFormat=4
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
'''


def main() -> None:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument('output', type=Path, help='Map path to write')
    parser.add_argument('--size', type=int, default=250, help='Size= width and height (default 250)')
    parser.add_argument('--starts', type=int, help='Grid starts with ore and trees, for the match scenario')
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(build_map(args.size, args.starts), encoding='ascii')


if __name__ == '__main__':
    main()
