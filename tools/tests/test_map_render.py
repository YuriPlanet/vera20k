"""Whole-map render: tiles placed by their cameras, cropped to the view area, shrunk and marked."""

import base64
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools import map_render
from tools.shell_capture_diff import read_png_rgb

# A 40x30 frame whose top-left 20x14 is the battlefield, and the world
# rectangle the camera can show: three tiles across, two down.
FRAME, EXTENT, AREA = (40, 30), (20, 14), (-16.0, 4.0, 46.0, 20.0)
# A two-pixel marker, red then clear, anchored on its clear pixel.
MARKER = {'label': '1', 'anchor': [-6, 8], 'picture': {
    'size': [2, 1], 'origin': [1, 0], 'rgba': base64.b64encode(bytes((200, 0, 0, 255, 0, 0, 0, 0))).decode()}}


def fake_capture(requests):
    """map_observation.capture for a world whose pixel (x, y) is RGB (x, y, 7) from the area's corner."""
    def capture(*, profile_path, output, **_):
        profile = json.loads(profile_path.read_text())
        x, y = profile['camera_top_left']
        requests.append((x, y, profile.get('observe_start_markers', False)))
        # The camera clamp: a top-left anywhere in the area, the view inside it.
        camera = (min(max(x, AREA[0]), AREA[0] + AREA[2] - EXTENT[0]),
                  min(max(y, AREA[1]), AREA[1] + AREA[3] - EXTENT[1]))
        frame = bytearray(FRAME[0] * FRAME[1] * 4)
        for row in range(FRAME[1]):
            for column in range(FRAME[0]):
                world = (int(camera[0] - AREA[0]) + column, int(camera[1] - AREA[1]) + row)
                at = (row * FRAME[0] + column) * 4
                frame[at:at + 4] = bytes((7, world[1], world[0], 255))
        path = output / 'child-output' / 'frame.bgra'
        path.parent.mkdir(parents=True)
        path.write_bytes(frame)
        evidence = {'camera': {'top_left': list(camera)}, 'tactical_extent': list(EXTENT),
                    'view_area': list(AREA), 'frame': {'path': str(path)}}
        if profile.get('observe_start_markers'):
            evidence['start_markers'] = [MARKER]
        return {'status': 'VALID', 'errors': [], 'capture': evidence}
    return capture


class MapRenderTests(unittest.TestCase):
    def test_tiles_cover_the_view_area_once_each_and_shrink_to_block_means(self):
        requests = []
        with tempfile.TemporaryDirectory() as temp, \
                patch.object(map_render, 'FRAME', FRAME), \
                patch.object(map_render.observation, 'capture', fake_capture(requests)):
            root = Path(temp).resolve()
            (root / 'config.toml').write_text('[gameplay]\ninput_delay_ticks = 2\n')
            output = root / 'out' / 'map.png'
            output.parent.mkdir()
            self.assertEqual(map_render.render(root / 'map.yrm', output, 23, root, {}), (23, 10, 6))
            far = map_render.FAR
            self.assertEqual(requests, [(-far, -far, True), (4, -far, False), (far, -far, False),
                                        (-far, far, False), (4, far, False), (far, far, False)])
            width, height, pixels = read_png_rgb(output.read_bytes())
            self.assertEqual(sorted(path.name for path in output.parent.iterdir()), ['map.png'])
        # Each pixel is its 2x2 block's mean, rounded: world (2x..2x+1, 2y..2y+1).
        # The marker's anchor, world (-6, 8), lands on (5, 2); its red pixel left of it.
        expected = [(2 * x + 1, 2 * y + 1, 7) for y in range(10) for x in range(23)]
        expected[2 * 23 + 4] = (200, 0, 0)
        self.assertEqual((width, height), (23, 10))
        self.assertEqual(pixels, expected)

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
