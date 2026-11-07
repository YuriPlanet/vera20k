"""Render a whole map with VERA's tactical renderer, for its preview on vera20k.org.

The picture is the game's own hidden map-picture run (``vera20k --map-picture``,
``app::map_picture``) over the whole world rectangle the camera can show: the
map's own terrain, lighting, overlays and structures, with the shroud off and no
starting units. The run shrinks it by a whole factor to at most ``--width``
pixels wide, each pixel the mean of the block it covers; it is written as an RGB
PNG.

Each multiplayer start then gets the skirmish preview's marker
(``DrawStartPositions`` 0x00640710: STARTBUT.SHP with its number in yellow
GAME.FNT), which the run hands over with the picture. It is drawn
``round(max(width / 240, height / 150))`` times its size, so it shows at its own
size in the 240 x 150 box vera20k.org/maps shows the picture in. Unlike the
game's preview, which marks the ``[Header]`` start points, a marker sits on its
start waypoint's cell centre at the terrain's height, and a map whose header
lists no starts still gets them. Where the game would draw two markers over each
other, they are pushed apart along the line between them until their anchors
lie a marker's width and two pixels apart, so every number stays readable.
Markers at the picture's edges are then moved inward until their whole sprite fits.

    python3 -m tools.map_render --map /abs/map.yrm --output /abs/preview.png

Run it from a checkout whose config.toml points at the retail game, with a
``fast`` or release vera20k built there (or ``--executable``/``--build-label``).
The run logs to the checkout's ``logs/map-picture.log``.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import math
import sys
import tempfile
from pathlib import Path

from tools import map_observation as observation
from tools.child_process import run_child
from tools.shell_capture_diff import read_png_rgb, write_png_rgb

# The box vera20k.org/maps shows each picture in (.map-preview in the website's
# styles.css), where the start markers show at their own size.
LIST_BOX = (240, 150)
# The pixels, at the marker's own size, between two start markers pushed apart.
MARKER_GAP = 2
# Taller than any map's picture, so only --width limits the run's shrink.
TALLEST = 1 << 16
# Longer than the run gives itself (map_picture::LIFETIME, 150 s).
TIMEOUT_SECONDS = 180


def spread(spots: list[tuple[int, int]], distance: float) -> list[tuple[int, int]]:
    """The spots with each two closer than `distance` pushed apart along the line between
    them, about their midpoint, until `distance` separates them; others stay where they are."""
    points = [[float(x), float(y)] for x, y in spots]
    for _ in range(100):
        moved = False
        for index, first in enumerate(points):
            for second in points[index + 1:]:
                dx, dy = second[0] - first[0], second[1] - first[1]
                gap = math.hypot(dx, dy)
                if gap >= distance - 0.01:
                    continue
                # Spots on top of each other part sideways.
                ux, uy = (dx / gap, dy / gap) if gap else (1.0, 0.0)
                push = (distance - gap) / 2
                first[0], first[1] = first[0] - ux * push, first[1] - uy * push
                second[0], second[1] = second[0] + ux * push, second[1] + uy * push
                moved = True
        if not moved:
            break
    return [(round(x), round(y)) for x, y in points]


def mark_starts(rgb: bytes, width: int, height: int, markers: list[dict],
                area: tuple[float, float, float, float], factor: int) -> bytes:
    """Packed RGB with each start marker on its anchor, shrunk by `factor` from world `area`,
    overlapping markers spread apart and edge markers moved inside the picture."""
    scale = max(1, round(max(width / LIST_BOX[0], height / LIST_BOX[1])))
    canvas = bytearray(rgb)
    anchors = [(math.floor((marker['anchor'][0] - area[0]) / factor),
                math.floor((marker['anchor'][1] - area[1]) / factor)) for marker in markers]
    widest = max((marker['picture']['size'][0] for marker in markers), default=0)
    for marker, (anchor_x, anchor_y) in zip(markers, spread(anchors, (widest + MARKER_GAP) * scale)):
        picture = marker['picture']
        (columns, rows), (origin_x, origin_y) = picture['size'], picture['origin']
        rgba = base64.b64decode(picture['rgba'])
        left = max(0, min(anchor_x - origin_x * scale, width - columns * scale))
        top = max(0, min(anchor_y - origin_y * scale, height - rows * scale))
        for y in range(max(0, -top), min(rows * scale, height - top)):
            for x in range(max(0, -left), min(columns * scale, width - left)):
                pixel = ((y // scale) * columns + x // scale) * 4
                if rgba[pixel + 3]:
                    at = ((top + y) * width + left + x) * 3
                    canvas[at:at + 3] = rgba[pixel:pixel + 3]
    return bytes(canvas)


def render(map_path: Path, output: Path, max_width: int, checkout: Path, binary: dict) -> tuple[int, int]:
    """Write the map's picture to `output`; its width and height."""
    executable = observation.vera20k_executable(**binary)
    sha256 = hashlib.sha256(map_path.read_bytes()).hexdigest()
    with tempfile.TemporaryDirectory(prefix=f'{output.stem}.run-', dir=output.parent) as folder:
        work = Path(folder)
        picture, markers = work / 'picture.png', work / 'markers.json'
        child = run_child([str(executable), '--map-picture', str(map_path), '--sha256', sha256,
                           '--output', str(picture), '--area', 'view',
                           '--max-size', f'{max_width}x{TALLEST}', '--markers', str(markers)],
                          cwd=checkout, temporary_directory=work,
                          timeout_seconds=TIMEOUT_SECONDS, label='map picture run')
        if child.errors or child.exit_status != 0:
            reasons = '; '.join([*child.errors, f'exit status {child.exit_status}'])
            raise RuntimeError(f'{reasons} (see {checkout / "logs" / "map-picture.log"})')
        width, height, pixels = read_png_rgb(picture.read_bytes())
        evidence = json.loads(markers.read_text())
    rgb = mark_starts(bytes(channel for pixel in pixels for channel in pixel), width, height,
                      evidence['start_markers'], tuple(evidence['area']), evidence['factor'])
    partial = output.with_name(output.name + '.partial')
    write_png_rgb(partial, width, height, rgb)
    partial.replace(output)
    return width, height


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--map', type=Path, required=True, help='Absolute path to the map file')
    parser.add_argument('--output', type=Path, required=True, help='Absolute path of the PNG to write')
    parser.add_argument('--width', type=int, default=1200, help='Widest picture to write (default 1200)')
    parser.add_argument('--cwd', type=Path, default=Path.cwd(), help='Checkout with config.toml (default: here)')
    binary = parser.add_mutually_exclusive_group()
    binary.add_argument('--executable', type=Path)
    binary.add_argument('--build-label')
    parser.add_argument('--build-profile', choices=observation.CAPTURE_PROFILES)
    args = parser.parse_args(argv)
    map_path, output = args.map.resolve(), args.output.resolve()
    if not map_path.is_file() or not output.parent.is_dir() or args.width < 1:
        parser.error('--map must be a file, --output must be in an existing folder and --width at least 1')
    try:
        width, height = render(map_path, output, args.width, args.cwd.resolve(),
                               {'executable': args.executable and args.executable.resolve(),
                                'build_label': args.build_label, 'build_profile': args.build_profile})
    except (RuntimeError, observation.ValidationError) as error:
        print(f'FAILED: {error}', file=sys.stderr)
        return 1
    print(f'{output}: {width}x{height}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
