"""Scroll-rate fit over timed screenshots."""

import json
import tempfile
import unittest
from pathlib import Path

from tools import shell_scroll_rate
from tools.shell_capture_diff import write_png_rgb


def striped(height, width, lines, offset):
    """Bright horizontal text-like bars of distinct widths, moved up by `offset`."""
    pixels = []
    for y in range(height):
        source = y + offset
        bar = next((w for top, w in lines if top <= source < top + 3), 0)
        pixels.extend((255, 255, 128) if x < bar else (0, 0, 0) for x in range(width))
    return pixels


class ScrollRateTests(unittest.TestCase):
    def test_uniform_scroll_is_fitted_from_exact_pairs(self):
        lines = [(y, 10 + (y * 7) % 40) for y in range(20, 400, 16)]
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            rows = []
            for index, offset in enumerate((0, 12, 24)):
                path = temp / f"shot{index}.png"
                write_png_rgb(path, 64, 200, striped(200, 64, lines, offset))
                rows.append(f"{path}\t{index * 0.2}")
            shots = temp / "shots.tsv"
            shots.write_text("\n".join(rows) + "\n", encoding="utf-8")
            report_path = temp / "report.json"
            status = shell_scroll_rate.main(
                [
                    "--shots",
                    str(shots),
                    "--columns",
                    "0:64",
                    "--rows",
                    "0:200",
                    "--output",
                    str(report_path),
                ]
            )
            report = json.loads(report_path.read_text(encoding="utf-8"))
        self.assertEqual(status, 0)
        self.assertEqual([p["shift_px"] for p in report["pairs"]], [12, 12])
        self.assertEqual(report["exact_pairs"], 2)
        self.assertAlmostEqual(report["px_per_second"], 60.0)


if __name__ == "__main__":
    unittest.main()
