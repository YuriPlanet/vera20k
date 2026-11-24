"""Integrity/claim boundaries for diagnostic skirmish captures."""

import copy
import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from tools.shell_certification.core import ValidationError
from tools.shell_certification.skirmish import CHECKPOINT, FRAME_BYTES, SCHEMA, validate_bundle


def fixture():
    return {
        "schema_version": SCHEMA, "checkpoint": CHECKPOINT, "parity_certification": "NONE",
        "presenter_domain": "final-swapchain-after-rgb565",
        "surface": {"width": 800, "height": 600, "format": "Bgra8UnormSrgb",
                    "pixel_layout": "BGRA8", "row_order": "top-left", "row_stride": 3200},
        "cursor": {"x": 400, "y": 300, "policy": "software-composited"},
        "route": [{"dialog": 0xE2, "frame": 20, "action": "SinglePlayer"},
                  {"dialog": 0x100, "frame": 40, "action": "Skirmish"}],
        "dialog_resource_id": 0x102, "capture_frame": 60,
        "reveals_completed": True, "ordinary_skirmish_frame": True,
        "input_enrollment": "UNENROLLED; asset/profile bytes require enrollment before native comparison",
        "selection": {
            "map_file": "test.map", "map_label": "Test", "map_capacity": 2, "mode_id": 1,
            "preview": {"width": 2, "height": 2}, "parsed_map_sha256": "a" * 64,
            "parsed_map_hash_encoding": "Rust Debug representation; diagnostic identity only",
            "player": {"name": "Player", "country": "American", "country_random": True,
                       "color": 0, "color_claimed": False, "start": "Auto", "team": -1},
            "opponents": [], "options": {
                "credits": 10000, "speed": 6, "units": 0, "short_game": True,
                "super_weapons": True, "build_off_ally": True, "crates": False,
                "mcv_redeploy": True,
            },
        },
        "frame": {"path": "frame.bgra", "byte_length": FRAME_BYTES,
                  "sha256": hashlib.sha256(bytes(FRAME_BYTES)).hexdigest()},
    }


class SkirmishBundleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.bundle = Path(self.temp.name)
        self.frame = self.bundle / "frame.bgra"
        self.frame.write_bytes(bytes(FRAME_BYTES))

    def write(self, manifest):
        (self.bundle / "capture.json").write_text(json.dumps(manifest), encoding="utf-8")

    def test_valid_diagnostic_bundle_never_claims_parity(self):
        self.write(fixture())
        result = validate_bundle(self.bundle)
        self.assertTrue(result["capture_valid"])
        self.assertEqual(result["parity_certification"], "NONE")

    def test_rejects_false_claims_and_malformed_route_selection(self):
        changes = [
            ("parity_certification", "PASS"), ("reveals_completed", False),
            ("ordinary_skirmish_frame", False), ("capture_frame", 40),
            ("checkpoint", "main-menu-0xe2-steady"), ("selection", None),
            ("route", []), ("capture_frame", True),
        ]
        for key, value in changes:
            with self.subTest(key=key, value=value):
                manifest = fixture()
                manifest[key] = value
                self.write(manifest)
                with self.assertRaises(ValidationError):
                    validate_bundle(self.bundle)
        manifest = fixture()
        manifest["selection"]["player"] = {}
        self.write(manifest)
        with self.assertRaises(ValidationError):
            validate_bundle(self.bundle)

    def test_rejects_frame_corruption_truncation_and_extra_files(self):
        self.write(fixture())
        with self.frame.open("r+b") as stream:
            stream.write(b"corruption")
        with self.assertRaises(ValidationError):
            validate_bundle(self.bundle)

        self.frame.write_bytes(b"short")
        with self.assertRaises(ValidationError):
            validate_bundle(self.bundle)
        self.frame.write_bytes(bytes(FRAME_BYTES))
        (self.bundle / "extra").write_bytes(b"")
        with self.assertRaises(ValidationError):
            validate_bundle(self.bundle)

    def test_rejects_float_cursor_and_false_asset_identity(self):
        for field in ("x", "y"):
            manifest = fixture()
            manifest["cursor"][field] = float(manifest["cursor"][field])
            self.write(manifest)
            with self.assertRaises(ValidationError):
                validate_bundle(self.bundle)
        manifest = fixture()
        manifest["selection"]["parsed_map_hash_encoding"] = "native asset bytes"
        self.write(manifest)
        with self.assertRaises(ValidationError):
            validate_bundle(self.bundle)

    def test_rejects_duplicate_json_and_payload_path_escape(self):
        self.write(fixture())
        path = self.bundle / "capture.json"
        raw = path.read_text(encoding="utf-8")
        path.write_text(raw[:-1] + ',"capture_frame": 60}', encoding="utf-8")
        with self.assertRaises(ValidationError):
            validate_bundle(self.bundle)
        manifest = copy.deepcopy(fixture())
        manifest["frame"]["path"] = "../frame.bgra"
        self.write(manifest)
        with self.assertRaises(ValidationError):
            validate_bundle(self.bundle)


if __name__ == "__main__":
    unittest.main()
