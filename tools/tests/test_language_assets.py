"""tools/language_assets.py: where a cameo's name band starts, and the letter rows."""

import unittest

from tools import language_assets as tool

WIDTH = tool.CAMEO_WIDTH
HEIGHT = tool.CAMEO_HEIGHT
# Index 0 black, index 1 white: the distance any changed pixel moves.
PALETTE = [(0, 0, 0), (255, 255, 255)] + [(0, 0, 0)] * 254


def picture(*changed_rows):
    pixels = bytearray(WIDTH * HEIGHT)
    for row in changed_rows:
        pixels[row * WIDTH : (row + 1) * WIDTH] = bytes([1]) * WIDTH
    return bytes(pixels)


class BandTop(unittest.TestCase):
    def test_a_quiet_row_between_two_text_lines_stays_in_the_band(self):
        translated = picture(*range(30, 37), *range(38, 47))
        self.assertEqual(tool.band_top(picture(), translated, PALETTE), 30)

    def test_rows_above_the_lower_half_are_never_band(self):
        translated = picture(*range(0, HEIGHT))
        self.assertEqual(tool.band_top(picture(), translated, PALETTE), tool.FIRST_BAND_ROW)


class LabelRecord(unittest.TestCase):
    def test_a_record_holds_the_band_xor_retail(self):
        retail = bytes([7]) * (WIDTH * HEIGHT)
        translated = bytes([7]) * (47 * WIDTH) + bytes([5]) * WIDTH
        record = tool.label_record("GTNK.shp", 47, retail, translated)
        name_end = 1 + len("gtnk.shp")
        self.assertEqual(record[1:name_end], b"gtnk.shp")
        self.assertEqual(record[name_end], 47)
        self.assertEqual(record[name_end + 5 :], bytes([7 ^ 5]) * WIDTH)

    def test_a_pixel_transparent_in_retail_stays_transparent(self):
        retail = bytes([7]) * (WIDTH * HEIGHT - 1) + bytes([0])
        translated = bytes([5]) * (WIDTH * HEIGHT)
        record = tool.label_record("x.shp", 47, retail, translated)
        self.assertEqual(record[-2:], bytes([7 ^ 5, 0]))


class FitRows(unittest.TestCase):
    def test_the_seventeenth_row_moves_onto_the_last(self):
        rows = [0] * 15 + [0x8000, 0x4000]
        self.assertEqual(tool.fit_rows(rows), [0] * 15 + [0xC000])


if __name__ == "__main__":
    unittest.main()
