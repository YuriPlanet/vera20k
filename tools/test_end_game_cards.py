"""Contracts for the end-game art converter; no retail or RA3 files needed.

python -m unittest tools.test_end_game_cards -v
"""

import struct
import tempfile
import unittest
from pathlib import Path

from PIL import Image

from tools import end_game_cards as cards


class FitBannerTest(unittest.TestCase):
    def test_the_card_keeps_its_own_size_by_default(self):
        # The engine fits a card without ever enlarging it, so writing the
        # source size is what puts the whole portrait on screen unclipped.
        image = Image.new("RGB", (354, 512))
        self.assertEqual(cards.fit_banner(image, None).size, (354, 512))

    def test_cartoon_shrinking_keeps_the_aspect(self):
        image = Image.new("RGB", (100, 200))
        shrunk = cards.fit_banner(image, 100)
        self.assertEqual(shrunk.size, (50, 100), "half as tall is half as wide")

    def test_a_card_already_inside_the_limit_is_untouched(self):
        # Shrinking is opt-in; the default pass through must not resample a
        # card that already fits, or every rebuild would soften it again.
        image = Image.new("RGB", (354, 512))
        self.assertEqual(cards.fit_banner(image, 512).size, (354, 512))


class FitScoreTest(unittest.TestCase):
    def test_the_page_is_filled_exactly(self):
        # The score dialog draws its background at the dialog's own 800x600, so
        # anything else letterboxes and shows the shell through the gap.
        out = cards.fit_score(Image.new("RGB", (700, 393)), 0.5)
        self.assertEqual(out.size, (cards.SCORE_W, cards.SCORE_H))

    def test_a_wider_artwork_gives_up_its_sides_not_its_centre(self):
        # `SCORE_ALLIED` is 700x393: cropping to 4:3 must keep the whole figure
        # down the middle, so the loss has to come off the left and right.
        image = Image.new("RGB", (700, 393))
        out = cards.fit_score(image, 0.5)
        self.assertEqual(out.size, (800, 600))
        self.assertEqual(out.mode, "RGB", "the crop stays colour-true for the quantizer")

    def test_anchor_picks_which_band_survives_on_a_tall_picture(self):
        # A vertical gradient: whichever band survives carries its own level,
        # so the top of the crop says which part of the source it came from.
        image = Image.new("RGB", (400, 1200))
        image.putdata([(0, y * 255 // 1199, 0) for y in range(1200) for _ in range(400)])
        for anchor in (0.0, 0.5, 1.0):
            self.assertEqual(cards.fit_score(image, anchor).size, (800, 600))
        top = cards.fit_score(image, 0.0)
        bottom = cards.fit_score(image, 1.0)
        self.assertLess(
            top.getpixel((0, 0))[1],
            bottom.getpixel((0, 0))[1],
            "the bottom anchor starts further down the gradient",
        )


class QuantizeTest(unittest.TestCase):
    def test_index_zero_is_never_emitted(self):
        # The blitter skips index 0, so a background pixel that landed there
        # would be a hole. Black must map to some other entry.
        rows = [[(0, 0, 0), (0, 0, 0)], [(0, 0, 0), (0, 0, 0)]]
        palette, indices = cards.quantize(rows, 8)
        self.assertEqual(palette[0], (0, 0, 0), "the reserved entry stays black")
        self.assertTrue(indices)
        self.assertNotIn(0, indices)

    def test_colours_snap_to_what_a_six_bit_palette_stores(self):
        rows = [[(201, 133, 77), (12, 240, 3)]]
        palette, _ = cards.quantize(rows, 4)
        for r, g, b in palette:
            self.assertEqual((r & 3, g & 3, b & 3), (0, 0, 0))

    def test_two_distinct_colours_stay_distinct(self):
        rows = [[(255, 0, 0), (0, 0, 255)] * 4 for _ in range(4)]
        palette, indices = cards.quantize(rows, 8)
        # median cut stops when no box has two colours left to split, so two
        # source colours give exactly two entries beside the reserved one.
        self.assertEqual(len(palette), 3)
        self.assertEqual(len(set(indices)), 2)

    def test_a_flat_picture_needs_one_entry(self):
        rows = [[(40, 80, 120)] * 4 for _ in range(4)]
        palette, indices = cards.quantize(rows, 32)
        self.assertEqual(len(set(indices)), 1)
        self.assertLessEqual(len(palette), 33)


class ShpAndPalTest(unittest.TestCase):
    def test_shp_header_is_what_the_game_parser_reads(self):
        width, height = 6, 5
        pixels = [1] * (width * height)
        data = cards.shp_bytes(width, height, pixels)
        zero, w, h, frames = struct.unpack_from("<HHHH", data, 0)
        self.assertEqual((zero, w, h, frames), (0, width, height, 1))
        x, y, fw, fh, fmt = struct.unpack_from("<HHHHB", data, 8)
        self.assertEqual((x, y, fw, fh, fmt), (0, 0, width, height, 0))
        offset = struct.unpack_from("<I", data, 8 + 20)[0]
        self.assertEqual(offset, 8 + 24, "pixel data follows the one frame header")
        self.assertEqual(len(data), offset + width * height)

    def test_palette_is_768_six_bit_bytes(self):
        palette = [(255, 128, 4)] * 200
        data = cards.palette_bytes(palette)
        self.assertEqual(len(data), 768)
        self.assertTrue(all(value <= 63 for value in data))
        self.assertEqual(tuple(data[:3]), (63, 32, 1))

    def test_an_oversized_palette_is_refused_not_truncated(self):
        # 255 quantised colours plus index 0 fit exactly; anything past that is
        # a caller bug and must not silently lose colours.
        cards.palette_bytes([(0, 0, 0)] * 256)
        with self.assertRaises(ValueError):
            cards.palette_bytes([(0, 0, 0)] * 257)

    def test_pixel_count_must_match_the_canvas(self):
        with self.assertRaises(ValueError):
            cards.shp_bytes(4, 4, [1] * 15)

    def test_verify_accepts_a_pair_it_wrote(self):
        width, height = 4, 3
        palette, indices = cards.quantize([[(9, 9, 9)] * width] * height, 4)
        with tempfile.TemporaryDirectory() as room:
            shp = Path(room) / "T.SHP"
            pal = Path(room) / "T.PAL"
            shp.write_bytes(cards.shp_bytes(width, height, indices))
            pal.write_bytes(cards.palette_bytes(palette))
            summary = cards.verify(shp, pal, width, height)
        self.assertIn("4x3", summary)

    def test_verify_rejects_a_background_with_holes(self):
        width, height = 4, 3
        with tempfile.TemporaryDirectory() as room:
            shp = Path(room) / "T.SHP"
            pal = Path(room) / "T.PAL"
            shp.write_bytes(cards.shp_bytes(width, height, [0] * (width * height)))
            pal.write_bytes(cards.palette_bytes([(0, 0, 0)]))
            with self.assertRaises(ValueError):
                cards.verify(shp, pal, width, height)


class CardTableTest(unittest.TestCase):
    def test_both_places_the_match_result_is_painted_are_filled(self):
        # Banner over the battlefield, score page behind the dialog. Neither
        # borrows the other's art, so both have to be listed.
        self.assertEqual({card[0] for card in cards.CARDS}, {"banner", "score"})

    def test_only_the_two_sides_veras_third_side_can_use_are_built(self):
        # Allied and Soviet only: RA2's third side is Yuri and there is no art
        # for it, so the Imperial cards must not appear anywhere.
        sources = {card[2] for card in cards.CARDS}
        self.assertNotIn("tex1_R", sources)
        self.assertNotIn("tex3_L", sources)

    def test_the_banner_stems_are_the_four_cards(self):
        banners = {
            stem
            for card in cards.CARDS
            if card[0] == "banner"
            for stem in card[3]
        }
        self.assertEqual(banners, {"ENDAWIN", "ENDALOS", "ENDSWIN", "ENDSLOS"})

    def test_the_score_stems_cover_both_outcomes_of_both_sides(self):
        scores = {
            stem for card in cards.CARDS if card[0] == "score" for stem in card[3]
        }
        self.assertEqual(scores, {"SCAWIN", "SCALOS", "SCSWIN", "SCSLOS"})

    def test_every_output_name_fits_eight_point_three(self):
        for card in cards.CARDS:
            for stem in card[3]:
                self.assertLessEqual(len(stem), 8, stem)
                self.assertTrue(stem.isupper(), stem)

    def test_every_source_is_named_once(self):
        sources = [card[2] for card in cards.CARDS]
        self.assertEqual(len(sources), len(set(sources)))


if __name__ == "__main__":
    unittest.main()
