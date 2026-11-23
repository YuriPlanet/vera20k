"""Check that the scale map decodes to the full Size diamond with in-map start waypoints."""

import math
import unittest

from tools.scale_map import CELL_ARRAY_WIDTH, ORE_OVERLAYS, TREES, build_map, in_diamond
from tools.tests.test_render_depth_fixture import decode_iso_cells, decode_overlay_section, parse_ini


def start_cells(ini):
    return [(value % 1000, value // 1000) for value in map(int, ini['Waypoints'].values())]


class ScaleMapTests(unittest.TestCase):
    def test_small_map_covers_the_size_diamond(self):
        size = 16
        ini = parse_ini(build_map(size))
        self.assertEqual(ini['Map']['Size'], f'0,0,{size},{size}')
        self.assertNotIn('MaxPlayers', ini['Basic'])
        x, y, width, height = map(int, ini['Map']['LocalSize'].split(','))
        self.assertTrue(width > 0 and height > 0)
        cells = decode_iso_cells(ini['IsoMapPack5'])
        # Size=0,0,N,N has 2N-1 diagonal rows of N cells each.
        self.assertEqual(len(cells), (2 * size - 1) * size)
        self.assertTrue(all(value == (0, 0, 0) for value in cells.values()))
        waypoints = set(start_cells(ini))
        self.assertEqual(len(waypoints), 8)
        for waypoint in waypoints:
            self.assertIn(waypoint, cells)
        self.assertNotIn('Terrain', ini)
        self.assertEqual(set(decode_overlay_section(ini['OverlayPack'])), {0xFF})

    def test_size_is_bounded_by_the_playfield_and_the_cell_array(self):
        for size in (15, 501):
            with self.assertRaises(ValueError):
                build_map(size)
        with self.assertRaises(ValueError):
            build_map(500, cell_array_width=512)

    def test_extended_grid_keeps_high_coordinate_overlays_and_literal_map_size(self):
        ini = parse_ini(build_map(500, 30))
        self.assertEqual(ini['Map']['Size'], '0,0,500,500')
        cells = decode_iso_cells(ini['IsoMapPack5'])
        self.assertEqual(len(cells), 499500)
        self.assertIn((749, 750), cells)
        homes = start_cells(ini)
        self.assertEqual(len(set(homes)), 30)
        self.assertEqual(ini['Basic']['MaxPlayers'], '30')
        self.assertTrue(all(home in cells for home in homes))
        overlay = decode_overlay_section(ini['OverlayPack'])
        self.assertEqual(len(overlay), 1024 * 1024)
        self.assertTrue(any(kind != 255 and index // 1024 > 512 for index, kind in enumerate(overlay)))

    def test_match_map_gives_each_start_its_own_ore_and_keeps_trees_off_the_bases(self):
        size, starts = 128, 9
        text = build_map(size, starts)
        self.assertEqual(text, build_map(size, starts), 'a fixed seed writes the same map')
        ini = parse_ini(text)
        cells = decode_iso_cells(ini['IsoMapPack5'])
        homes = start_cells(ini)
        self.assertEqual(len(set(homes)), starts)
        self.assertTrue(all(home in cells for home in homes))
        overlay = decode_overlay_section(ini['OverlayPack'])
        density = decode_overlay_section(ini['OverlayDataPack'])
        ore = {
            (index % CELL_ARRAY_WIDTH, index // CELL_ARRAY_WIDTH): (kind, density[index])
            for index, kind in enumerate(overlay)
            if kind != 0xFF
        }
        self.assertTrue(all(kind in ORE_OVERLAYS and level <= 11 for kind, level in ore.values()))
        self.assertTrue(all(cell in cells for cell in ore))
        pitch = min(math.dist(a, b) for a in homes for b in homes if a != b)
        for home in homes:
            nearest = min(math.dist(home, cell) for cell in ore)
            # Its field is clear of the base, and nearer than any other start.
            self.assertGreater(nearest, 0.2 * pitch)
            self.assertLess(nearest, 0.5 * pitch)
        trees = {(int(key) % 1000, int(key) // 1000): name for key, name in ini['Terrain'].items()}
        self.assertTrue(trees)
        self.assertTrue(all(name in TREES and in_diamond(size, *cell) for cell, name in trees.items()))
        self.assertFalse(set(trees) & set(ore))
        self.assertGreater(min(math.dist(home, cell) for home in homes for cell in trees), 0.4 * pitch)

    def test_match_map_needs_two_starts_far_enough_apart_for_their_bases(self):
        for size, starts in ((128, 1), (250, 64)):
            with self.assertRaises(ValueError):
                build_map(size, starts)


if __name__ == '__main__':
    unittest.main()
