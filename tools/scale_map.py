#!/usr/bin/env python3
"""Write a flat square TEMPERATE map for sim-bench scale runs, using only stdlib.

Usage:
    python -m tools.scale_map target/scale/flat500.map
    python -m tools.scale_map target/scale/flat200.map --size 100

`--size N` writes `Size=0,0,N,N`. Its diamond spans 2N cells on each of the
rx and ry axes, so the default N=250 is a 500x500-cell map. gamemd's cell
array is 512 wide, which bounds N at 256. Every cell is clear level ground;
the eight start waypoints sit on a ring. The map has no houses or objects:
sim-bench adds its own.
"""

import argparse
import math
import struct
from pathlib import Path

from tools.render_depth_fixture import pack_lcw, pack_lzo

CLEAR_TILE = 0


def build_map(size: int) -> str:
    if not 16 <= size <= 256:
        raise ValueError('size must be 16..256: a playfield inside the margin, and the '
                         'diamond inside the 512-wide cell array')
    cells = bytearray()
    for y in range(1, 2 * size):
        for x in range(1, 2 * size):
            # The Size=0,0,N,N diamond (see render_depth_fixture's N=48 map).
            if size < x + y <= 3 * size and abs(x - y) < size:
                cells += struct.pack('<hHiBBB', x, y, CLEAR_TILE, 0, 0, 0)
    overlay = bytes(b'\xff' * 262144)
    overlay_data = bytes(262144)
    waypoints = []
    for index in range(8):
        angle = 2 * math.pi * index / 8
        # Screen-aligned ring: u = rx - ry, v = rx + ry around the centre.
        u = round(0.7 * size * math.cos(angle))
        v = 2 * size + round(0.7 * size * math.sin(angle))
        rx, ry = (v + u) // 2, (v - u) // 2
        waypoints.append(f'{index}={ry * 1000 + rx}')
    margin = 4
    return f'''[Basic]
Name=Scale Flat {2 * size}x{2 * size}
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
{chr(10).join(waypoints)}

[OverlayPack]
{pack_lcw(overlay)}

[OverlayDataPack]
{pack_lcw(overlay_data)}

[IsoMapPack5]
{pack_lzo(bytes(cells))}
'''


def main() -> None:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument('output', type=Path, help='Map path to write')
    parser.add_argument('--size', type=int, default=250, help='Size= width and height (default 250)')
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(build_map(args.size), encoding='ascii')


if __name__ == '__main__':
    main()
