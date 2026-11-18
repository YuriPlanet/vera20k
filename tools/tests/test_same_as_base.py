import unittest

from tools.same_as_base import (PROFILE, capture_profile, differing_pixels, first_difference,
                                 noise_allowance, pixel_runs)


class SameAsBaseTests(unittest.TestCase):
    def test_capture_profile_drops_commands_at_or_after_the_step(self):
        profile = capture_profile(PROFILE, 650, 150)
        self.assertEqual(profile['ticks'], 650)
        self.assertEqual([c['issue_after_step'] for c in profile['commands']], [0, 70])
        self.assertEqual(profile['presentation'], {'zoom': 150, 'sidebar': 100})
        wheel = capture_profile(PROFILE, 650, 100, 1.5)
        self.assertEqual(wheel['presentation'], {'zoom': 100, 'sidebar': 100, 'wheel_zoom': 1.5})

    def test_pixel_runs_add_wheel_view_distances_at_zoom_100(self):
        self.assertEqual(pixel_runs('100,150', '1.5,1'), [
            ('zoom100', 100, None), ('zoom150', 150, None),
            ('zoom100-wheel1.5', 100, 1.5), ('zoom100-wheel1', 100, 1)])
        self.assertEqual(pixel_runs('100', ''), [('zoom100', 100, None)])
        with self.assertRaises(ValueError):
            pixel_runs('100', '2')

    def test_first_difference_names_the_pixel(self):
        before = bytes(4 * 6)
        after = bytearray(before)
        after[4 * 4 + 2] = 9  # pixel 4 of a 3-wide frame: x 1, y 1
        self.assertEqual(first_difference(before, bytes(after), 3),
                         (1, 1, '00000000', '00000900'))
        self.assertIsNone(first_difference(before, before, 3))



    def test_head_passes_within_the_base_runs_own_noise(self):
        base = bytes(4 * 10)
        head = bytearray(base)
        head[0] = 1
        head[8] = 1
        self.assertEqual(differing_pixels(base, bytes(head)), 2)
        # Two base runs that differ in one pixel let head differ in up to 10.
        self.assertTrue(2 <= noise_allowance(1))
        self.assertFalse(11 <= noise_allowance(1))

if __name__ == '__main__':
    unittest.main()
