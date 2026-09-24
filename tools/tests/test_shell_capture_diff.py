"""RGB565-unit comparison of shell captures against native screenshots."""

import hashlib
import json
import struct
import tempfile
import unittest
import zlib
from pathlib import Path

from tools import shell_capture_diff as diff


def png_bytes(width, height, depth, color, rows, palette=None):
    def chunk(kind, body):
        return (
            struct.pack(">I", len(body))
            + kind
            + body
            + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)
        )

    body = chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, depth, color, 0, 0, 0))
    if palette is not None:
        body += chunk(b"PLTE", bytes(v for rgb in palette for v in rgb))
    raw = b"".join(bytes([kind]) + bytes(line) for kind, line in rows)
    return diff.PNG_SIGNATURE + body + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


def write_bundle(directory, width, height, rgb_pixels):
    raw = bytearray()
    for r, g, b in rgb_pixels:
        raw.extend((b, g, r, 255))
    (directory / "frame.bgra").write_bytes(bytes(raw))
    manifest = {
        "checkpoint": "test",
        "capture_frame": 7,
        "frame": {"path": "frame.bgra", "sha256": hashlib.sha256(raw).hexdigest()},
        "surface": {
            "width": width,
            "height": height,
            "row_stride": width * 4,
            "pixel_layout": "BGRA8",
            "row_order": "top-left",
        },
    }
    (directory / "capture.json").write_text(json.dumps(manifest), encoding="utf-8")


class PngDecodeTests(unittest.TestCase):
    def test_every_filter_reconstructs_rgb(self):
        # Row 1 (target 1..6 under row 0 = 10..60) encoded by hand with each
        # PNG filter type (RFC 2083 section 6).
        row0 = [10, 20, 30, 40, 50, 60]
        for kind, encoded in (
            (0, [1, 2, 3, 4, 5, 6]),
            (1, [1, 2, 3, 3, 3, 3]),
            (2, [247, 238, 229, 220, 211, 202]),
            (3, [252, 248, 244, 240, 235, 231]),
            (4, [247, 238, 229, 220, 241, 232]),
        ):
            data = png_bytes(2, 2, 8, 2, [(0, row0), (kind, encoded)])
            width, height, pixels = diff.read_png_rgb(data)
            self.assertEqual((width, height), (2, 2))
            self.assertEqual(
                pixels, [(10, 20, 30), (40, 50, 60), (1, 2, 3), (4, 5, 6)], f"filter {kind}"
            )

    def test_sub_byte_palette_indices_unpack_most_significant_first(self):
        palette = [(0, 0, 0), (255, 0, 0), (0, 255, 0), (0, 0, 255)]
        # Depth 2: indices 1, 2, 3, 0 in one byte.
        data = png_bytes(4, 1, 2, 3, [(0, [0b01101100])], palette)
        _, _, pixels = diff.read_png_rgb(data)
        self.assertEqual(pixels, [palette[1], palette[2], palette[3], palette[0]])


class CompareTests(unittest.TestCase):
    def test_units_ignore_expansion_bits_and_masks_exclude_pixels(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            # Same units with different low bits, one 1-unit and one 3-unit
            # red difference, and a masked difference.
            rust = [(247, 255, 8), (8, 0, 0), (24, 0, 0), (200, 200, 200)]
            native = [(240, 252, 15), (16, 0, 0), (0, 0, 0), (0, 0, 0)]
            write_bundle(temp, 2, 2, rust)
            rows = [(0, [v for p in native[:2] for v in p]), (0, [v for p in native[2:] for v in p])]
            (temp / "native.png").write_bytes(png_bytes(2, 2, 8, 2, rows))
            report_path = temp / "report.json"
            status = diff.main(
                [
                    "--capture",
                    str(temp),
                    "--native",
                    str(temp / "native.png"),
                    "--mask",
                    "cursor=1,1,1,1",
                    "--output",
                    str(report_path),
                    "--max-differing",
                    "1",
                ]
            )
            report = json.loads(report_path.read_text(encoding="utf-8"))
            self.assertEqual(status, 1)
            self.assertEqual(report["compared_pixels"], 3)
            self.assertEqual(report["differing_pixels"], 2)
            self.assertEqual(report["max_unit_difference"], 3)
            self.assertEqual(report["unit_difference_histogram"], {"1": 1, "3": 1})
            self.assertEqual(report["differing_bounds"], [0, 0, 1, 1])
            self.assertEqual(report["masks"], [{"label": "cursor", "rect": [1, 1, 1, 1]}])

    def test_manifest_without_frame_hash_is_checked_by_length(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            write_bundle(temp, 1, 1, [(0, 0, 0)])
            manifest = json.loads((temp / "capture.json").read_text(encoding="utf-8"))
            del manifest["frame"]["sha256"]
            manifest["frame"]["byte_length"] = 4
            (temp / "capture.json").write_text(json.dumps(manifest), encoding="utf-8")
            (temp / "native.png").write_bytes(png_bytes(1, 1, 8, 2, [(0, [0, 0, 0])]))
            report_path = temp / "report.json"
            status = diff.main(
                [
                    "--capture",
                    str(temp),
                    "--native",
                    str(temp / "native.png"),
                    "--output",
                    str(report_path),
                ]
            )
            report = json.loads(report_path.read_text(encoding="utf-8"))
            self.assertEqual(status, 0)
            self.assertEqual(
                report["capture"]["frame_sha256"],
                hashlib.sha256(b"\x00\x00\x00\xff").hexdigest(),
            )
            manifest["frame"]["byte_length"] = 8
            (temp / "capture.json").write_text(json.dumps(manifest), encoding="utf-8")
            status = diff.main(["--capture", str(temp), "--native", str(temp / "native.png")])
            self.assertEqual(status, 2)

    def test_tampered_frame_is_invalid_input(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            write_bundle(temp, 1, 1, [(0, 0, 0)])
            (temp / "frame.bgra").write_bytes(b"\x01\x00\x00\xff")
            (temp / "native.png").write_bytes(png_bytes(1, 1, 8, 2, [(0, [0, 0, 0])]))
            status = diff.main(["--capture", str(temp), "--native", str(temp / "native.png")])
            self.assertEqual(status, 2)


if __name__ == "__main__":
    unittest.main()
