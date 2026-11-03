"""Whole-map render: tiles placed by their cameras, cropped to the view area and shrunk."""

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


def fake_capture(requests):
    """map_observation.capture for a world whose pixel (x, y) is RGB (x, y, 7) from the area's corner."""
    def capture(*, profile_path, output, **_):
        profile = json.loads(profile_path.read_text())
        x, y = profile['camera_top_left']
        requests.append((x, y))
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
        return {'status': 'VALID', 'errors': [], 'capture': {
            'camera': {'top_left': list(camera)}, 'tactical_extent': list(EXTENT),
            'view_area': list(AREA), 'frame': {'path': str(path)}}}
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
            self.assertEqual(requests, [(-far, -far), (4, -far), (far, -far), (-far, far), (4, far), (far, far)])
            width, height, pixels = read_png_rgb(output.read_bytes())
            self.assertEqual(sorted(path.name for path in output.parent.iterdir()), ['map.png'])
        # Each pixel is its 2x2 block's mean, rounded: world (2x..2x+1, 2y..2y+1).
        self.assertEqual((width, height), (23, 10))
        self.assertEqual(pixels, [(2 * x + 1, 2 * y + 1, 7) for y in range(10) for x in range(23)])


if __name__ == '__main__':
    unittest.main()
