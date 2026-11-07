"""tools/unifont_cjk.py's trim of Unifont's 16x16 glyphs to GAME.FNT's 15x15 box."""
import unittest

from tools import unifont_cjk


def frame(side: int, size: int = 16) -> list[list[bool]]:
    """A `side`-pixel square outline in the top-left of a `size`-pixel glyph."""
    return [[x < side and y < side and (x in (0, side - 1) or y in (0, side - 1))
             for x in range(size)] for y in range(size)]


class TrimTest(unittest.TestCase):
    def test_a_blank_edge_goes_before_a_repeated_inner_line(self):
        self.assertEqual(unifont_cjk.trim(frame(15)), frame(15, 15))

    def test_inked_edges_stay(self):
        self.assertEqual(unifont_cjk.trim(frame(16)), frame(15, 15))


if __name__ == '__main__':
    unittest.main()
