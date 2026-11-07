"""Whole-map render: the game's picture of the map, marked at each start."""

import base64
import hashlib
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools import map_render
from tools.child_process import ChildResult
from tools.shell_capture_diff import read_png_rgb, write_png_rgb

# A two-pixel marker, red then clear, anchored on its clear pixel.
MARKER = {'label': '1', 'anchor': [-6, 8], 'picture': {
    'size': [2, 1], 'origin': [1, 0], 'rgba': base64.b64encode(bytes((200, 0, 0, 255, 0, 0, 0, 0))).decode()}}


def fake_run(commands):
    """run_child for a map-picture run: a grey 4x2 picture of world (-16, 4, 8, 4) at factor 2."""
    def run(command, **_):
        commands.append(command)
        options = dict(zip(command[3::2], command[4::2]))
        write_png_rgb(options['--output'], 4, 2, bytes([50]) * 24)
        Path(options['--markers']).write_text(json.dumps(
            {'area': [-16, 4, 8, 4], 'factor': 2, 'start_markers': [dict(MARKER, anchor=[-10, 6])]}))
        return ChildResult(pid=1, exit_status=0, timed_out=False, stdout=b'', stderr=b'', errors=())
    return run


class MapRenderTests(unittest.TestCase):
    def test_the_runs_picture_of_the_view_area_gets_its_start_markers(self):
        commands = []
        with tempfile.TemporaryDirectory() as temp, \
                patch.object(map_render.observation, 'vera20k_executable', lambda **_: Path('/vera20k')), \
                patch.object(map_render, 'run_child', fake_run(commands)):
            root = Path(temp).resolve()
            map_path, output = root / 'map.yrm', root / 'out' / 'map.png'
            map_path.write_bytes(b'[Basic]\n')
            output.parent.mkdir()
            self.assertEqual(map_render.render(map_path, output, 23, root, {}), (4, 2))
            width, height, pixels = read_png_rgb(output.read_bytes())
            self.assertEqual(sorted(path.name for path in output.parent.iterdir()), ['map.png'])
        command, = commands
        options = dict(zip(command[3::2], command[4::2]))
        self.assertEqual(command[:3], ['/vera20k', '--map-picture', str(map_path)])
        self.assertEqual((options['--sha256'], options['--area'], options['--max-size']),
                         (hashlib.sha256(b'[Basic]\n').hexdigest(), 'view', '23x65536'))
        # The marker's anchor, world (-10, 6), lands on (3, 1); its red pixel left of it.
        self.assertEqual((width, height), (4, 2))
        self.assertEqual(pixels, [(50, 50, 50)] * 6 + [(200, 0, 0), (50, 50, 50)])

    def test_markers_grow_with_the_picture_and_stay_inside_its_edge(self):
        # 480 px wide is twice the list box: the marker is drawn at twice its size.
        marked = map_render.mark_starts(bytes(480 * 2 * 3), 480, 2, [dict(MARKER, anchor=[1, 0])],
                                        (0.0, 0.0, 480.0, 2.0), 1)
        red = [(index // 3 % 480, index // 3 // 480) for index in range(0, len(marked), 3) if marked[index]]
        self.assertEqual(red, [(0, 0), (1, 0), (0, 1), (1, 1)])

    def test_marker_inset_uses_the_scaled_sprite_extent_and_preserves_interior_anchors(self):
        picture = {'size': [2, 3], 'origin': [1, 2],
                   'rgba': base64.b64encode(bytes((200, 0, 0, 255)) * 6).decode()}
        markers = [{'anchor': [479, 299], 'picture': picture},
                   {'anchor': [100, 100], 'picture': picture}]
        marked = map_render.mark_starts(bytes(480 * 300 * 3), 480, 300, markers,
                                        (0.0, 0.0, 480.0, 300.0), 1)
        red = {(index // 3 % 480, index // 3 // 480)
               for index in range(0, len(marked), 3) if marked[index]}
        self.assertEqual(red, {(x, y) for x in range(476, 480) for y in range(294, 300)}
                         | {(x, y) for x in range(98, 102) for y in range(96, 102)})

    def test_overlapping_markers_part_about_their_midpoint_and_others_stay(self):
        self.assertEqual(map_render.spread([(10, 10), (12, 10), (100, 100)], 20), [(1, 10), (21, 10), (100, 100)])
        self.assertEqual(map_render.spread([(5, 5), (5, 5)], 20), [(-5, 5), (15, 5)])


if __name__ == '__main__':
    unittest.main()
