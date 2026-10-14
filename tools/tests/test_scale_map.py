"""Check that the scale map decodes to the full Size diamond with in-map start waypoints."""

import unittest

from tools.scale_map import build_map
from tools.tests.test_render_depth_fixture import decode_iso_cells, parse_ini


class ScaleMapTests(unittest.TestCase):
    def test_small_map_covers_the_size_diamond(self):
        size = 16
        ini = parse_ini(build_map(size))
        self.assertEqual(ini['Map']['Size'], f'0,0,{size},{size}')
        x, y, width, height = map(int, ini['Map']['LocalSize'].split(','))
        self.assertTrue(width > 0 and height > 0)
        cells = decode_iso_cells(ini['IsoMapPack5'])
        # Size=0,0,N,N has 2N-1 diagonal rows of N cells each.
        self.assertEqual(len(cells), (2 * size - 1) * size)
        self.assertTrue(all(value == (0, 0, 0) for value in cells.values()))
        waypoints = {int(value) for value in ini['Waypoints'].values()}
        self.assertEqual(len(waypoints), 8)
        for waypoint in waypoints:
            self.assertIn((waypoint % 1000, waypoint // 1000), cells)

    def test_size_is_bounded_by_the_playfield_and_the_cell_array(self):
        for size in (15, 257):
            with self.assertRaises(ValueError):
                build_map(size)


if __name__ == '__main__':
    unittest.main()
