"""Render a whole map with VERA's tactical renderer, for its preview on vera20k.org.

Each tile is one hidden capture (tools/map_observation.py) of the map's first
frame, placed with the profile's ``camera_top_left``: the map's own terrain,
lighting, overlays and structures, with the shroud off and no starting units
(Bases off, no extra units). The tiles fill the world rectangle the camera can
show (each capture's ``view_area``); the picture is then shrunk by a whole factor
to at most ``--width`` pixels wide, each pixel the mean of the block it covers,
and written as an RGB PNG.

    python3 -m tools.map_render --map /abs/map.yrm --output /abs/preview.png

Run it from a checkout whose config.toml points at the retail game, with a
``fast`` or release vera20k built there (or ``--executable``/``--build-label``).
A tile is the capture's largest frame, 4096 px square, less the sidebar; the
largest maps need six. The tiles' runs go in a folder beside the output, removed
once the picture is written and kept when a capture fails.
"""

from __future__ import annotations

import argparse
import json
import math
import shutil
import sys
import tempfile
import tomllib
from dataclasses import dataclass
from pathlib import Path

from tools import map_observation as observation
from tools.shell_capture_diff import write_png_rgb

ROOT = Path(__file__).resolve().parents[1]
# The launch every tile starts from: a Battle with one human and one AI.
EXAMPLE = ROOT / 'tools' / 'map_observation.example.json'
CONTRACT = ROOT / 'src' / 'app' / 'diagnostics' / 'tactical_capture' / 'contract.v2.json'
# The capture's largest frame (MapCaptureProfile::validate).
FRAME = (4096, 4096)
# A camera request beyond every map's edge, which the clamp moves to that edge.
FAR = 1 << 30
# vera20k's input delay without a config.toml value (default_input_delay_ticks,
# src/util/config.rs); a capture must ask for the configured one.
DEFAULT_INPUT_DELAY = 15


@dataclass(frozen=True)
class Tile:
    """One capture: the world top-left of its battlefield and its BGRA frame."""
    camera: tuple[float, float]
    extent: tuple[int, int]
    frame_width: int
    frame: bytes


def tile_profile(map_path: Path, top_left: tuple[int, int], input_delay: int) -> dict:
    profile = json.loads(EXAMPLE.read_text())
    launch = profile['launch']
    launch['selected_map_file'] = str(map_path)
    launch['options'].update(bases=False, unit_count=0, shroud=False, fog_of_war=False)
    width, height = FRAME
    # The cursor rests on the sidebar, outside every tile, as in the example.
    profile.update(ticks=0, width=width, height=height, input_delay_ticks=input_delay,
                   timeout_seconds=300, cursor_position=[width - 80, height - 44],
                   camera_top_left=list(top_left))
    return profile


def axis_requests(start: float, extent: float, visible: int) -> list[int]:
    """Camera top-left requests along one axis whose views cover [start, start + extent)."""
    if extent <= visible:
        return [-FAR]  # The clamp centres an axis the view covers.
    requests = [-FAR]
    edge = start + visible
    while edge + visible < start + extent:
        requests.append(math.floor(edge))
        edge += visible
    return requests + [FAR]


def stitch(tiles: list[Tile], area: tuple[float, float, float, float]) -> tuple[bytes, int, int]:
    """Packed RGB of world rectangle `area` from the tiles' battlefields; later tiles win."""
    left, top = area[0], area[1]
    width, height = round(area[2]), round(area[3])
    canvas = bytearray(width * height * 3)
    for tile in tiles:
        dx, dy = round(tile.camera[0] - left), round(tile.camera[1] - top)
        first, last = max(0, -dx), min(tile.extent[0], width - dx)
        if first >= last:
            continue
        stride = tile.frame_width * 4
        span = (last - first) * 3
        for y in range(max(0, -dy), min(tile.extent[1], height - dy)):
            row = tile.frame[y * stride + first * 4:y * stride + last * 4]
            at = ((y + dy) * width + first + dx) * 3
            canvas[at:at + span:3] = row[2::4]
            canvas[at + 1:at + span:3] = row[1::4]
            canvas[at + 2:at + span:3] = row[0::4]
    return bytes(canvas), width, height


def shrink(rgb: bytes, width: int, height: int, factor: int) -> tuple[bytes, int, int]:
    """Packed RGB at 1/factor size, each pixel the mean of its factor x factor block."""
    if factor == 1:
        return rgb, width, height
    out_w, out_h = width // factor, height // factor
    out = bytearray(out_w * out_h * 3)
    stride, step, area = width * 3, factor * 3, factor * factor
    for y in range(out_h):
        rows = [rgb[(y * factor + row) * stride:(y * factor + row) * stride + out_w * step]
                for row in range(factor)]
        at = y * out_w * 3
        for x in range(out_w):
            for channel in range(3):
                begin = x * step + channel
                total = sum(sum(row[begin:begin + step:3]) for row in rows)
                out[at + x * 3 + channel] = (total + area // 2) // area
    return bytes(out), out_w, out_h


def input_delay(checkout: Path) -> int:
    config = tomllib.loads((checkout / 'config.toml').read_text())
    return int(config.get('gameplay', {}).get('input_delay_ticks', DEFAULT_INPUT_DELAY))


def capture_tile(work: Path, index: int, map_path: Path, top_left: tuple[int, int], delay: int,
                 checkout: Path, binary: dict) -> tuple[Tile, tuple[float, float, float, float]]:
    profile_path = work / f'tile-{index}.json'
    profile_path.write_text(json.dumps(tile_profile(map_path, top_left, delay)))
    report = observation.capture(profile_path=profile_path, contract_path=CONTRACT,
                                 output=work / f'tile-{index}', working_directory=checkout, **binary)
    if report['status'] != 'VALID':
        raise RuntimeError(f'tile {index} capture failed: {report["errors"]}')
    evidence = report['capture']
    frame = Path(evidence['frame']['path']).read_bytes()
    tile = Tile(tuple(evidence['camera']['top_left']), tuple(evidence['tactical_extent']), FRAME[0], frame)
    return tile, tuple(evidence['view_area'])


def render(map_path: Path, output: Path, max_width: int, checkout: Path, binary: dict) -> tuple[int, int, int]:
    """Write the map's picture to `output`; its width, height and tile count."""
    work = Path(tempfile.mkdtemp(prefix=f'{output.stem}.tiles-', dir=output.parent)).resolve()
    delay = input_delay(checkout)
    try:
        first, area = capture_tile(work, 0, map_path, (-FAR, -FAR), delay, checkout, binary)
        tiles = [first]
        requests = [(x, y) for y in axis_requests(area[1], area[3], first.extent[1])
                    for x in axis_requests(area[0], area[2], first.extent[0])]
        # The first request, beyond the top-left corner, is the tile already taken.
        for index, top_left in enumerate(requests[1:], start=1):
            tiles.append(capture_tile(work, index, map_path, top_left, delay, checkout, binary)[0])
    except (RuntimeError, observation.ValidationError) as error:
        raise RuntimeError(f'{error} (tile runs kept in {work})') from None
    rgb, width, height = stitch(tiles, area)
    small, width, height = shrink(rgb, width, height, max(1, math.ceil(width / max_width)))
    partial = output.with_name(output.name + '.partial')
    write_png_rgb(partial, width, height, small)
    partial.replace(output)
    shutil.rmtree(work)
    return width, height, len(tiles)


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
        width, height, count = render(map_path, output, args.width, args.cwd.resolve(),
                                      {'executable': args.executable and args.executable.resolve(),
                                       'build_label': args.build_label, 'build_profile': args.build_profile})
    except RuntimeError as error:
        print(f'FAILED: {error}', file=sys.stderr)
        return 1
    print(f'{output}: {width}x{height} from {count} tile{"s" if count > 1 else ""}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
