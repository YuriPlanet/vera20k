"""Unit tests for fail-closed capture and guard comparison."""

from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from tools.shell_certification.core import (
    CONTENT_RECT,
    DRIFT,
    EXPECTED_PRESENTATION_REGION_RECTS,
    INVALID,
    MATCH,
    EXPECTED_REGION_RECTS,
    LOGICAL_HEIGHT,
    LOGICAL_WIDTH,
    SCALE_DENOMINATOR,
    SCALE_NUMERATOR,
    OutputExistsError,
    Rect,
    ValidationError,
    build_comparison_report,
    compare_to_file,
    crop_tight_bgra,
    cursor_overlaps_rect,
    point_scale_presentation_crop_bgra,
    sha256_bytes,
    validate_capture_bundle,
    validate_guard,
    write_json_exclusive,
)


def _frame() -> bytes:
    """Return a nonuniform, deterministic 800x600 BGRA frame."""

    row = bytearray()
    for x in range(LOGICAL_WIDTH):
        row.extend((x & 0xFF, (x >> 3) & 0xFF, (255 - x) & 0xFF, 255))
    output = bytearray()
    for y in range(LOGICAL_HEIGHT):
        varied = bytearray(row)
        for x in range(0, len(varied), 4):
            varied[x + 1] ^= y & 0xFF
        output.extend(varied)
    return bytes(output)


def _capture_manifest() -> dict[str, object]:
    return {
        "schema_version": "vera20k.shell-capture.v2",
        "checkpoint": "main-menu-0xe2-steady",
        "surface": {
            "width": LOGICAL_WIDTH,
            "height": LOGICAL_HEIGHT,
            "format": "Bgra8Unorm",
            "pixel_layout": "BGRA8",
            "row_order": "top-left",
            "bytes_per_pixel": 4,
            "row_stride": LOGICAL_WIDTH * 4,
        },
        "cursor": {"x": 400, "y": 300, "policy": "software-composited"},
        "shell": {
            "screen": "main-menu",
            "dialog_resource_id": 226,
            "movie_owner": "main-menu-0xe2",
            "movie_base": "ra2ts-l",
            "main_menu_shell_failed": False,
            "single_player_active": False,
            "skirmish_active": False,
            "modal_open": False,
            "quit_active": False,
            "first_paint_slide_active": False,
            "title_terminal_persistent": True,
        },
        "frame": {
            "path": "frame.bgra",
            "byte_length": LOGICAL_WIDTH * LOGICAL_HEIGHT * 4,
        },
    }


def _guard_for_frame(frame: bytes) -> dict[str, object]:
    regions = []
    control_ids = {
        "title": 1684,
        "single_player": 1667,
        "options": 1372,
        "exit_game": 1006,
    }
    for name, coordinates in EXPECTED_REGION_RECTS.items():
        left, top, right, bottom = coordinates
        presentation_coordinates = EXPECTED_PRESENTATION_REGION_RECTS[name]
        digest = sha256_bytes(
            point_scale_presentation_crop_bgra(
                frame,
                LOGICAL_WIDTH,
                LOGICAL_HEIGHT,
                Rect(*CONTENT_RECT),
                Rect(*presentation_coordinates),
                SCALE_NUMERATOR,
                SCALE_DENOMINATOR,
            )
        )
        presentation_left, presentation_top, presentation_right, presentation_bottom = (
            presentation_coordinates
        )
        regions.append(
            {
                "name": name,
                "control_id": control_ids[name],
                "logical_rect": {
                    "left": left,
                    "top": top,
                    "right": right,
                    "bottom": bottom,
                },
                "presentation_rect": {
                    "left": presentation_left,
                    "top": presentation_top,
                    "right": presentation_right,
                    "bottom": presentation_bottom,
                },
                "raw_bgra_sha256": digest,
                "surface_raw_bgra_sha256": digest,
            }
        )
    return {
        "schema_version": "vera20k.shell-presentation-guard.v4",
        "guard_id": "main-menu-0xe2",
        "state": "main_menu_0xe2",
        "mapping": {
            "kind": "ddrawcompat.integer-800x600-to-1920x1080.v1",
            "logical_width": 800,
            "logical_height": 600,
            "presentation_width": 1920,
            "presentation_height": 1080,
            "content_rect": {
                "left": 240,
                "top": 0,
                "right": 1680,
                "bottom": 1080,
            },
            "scale_numerator": 9,
            "scale_denominator": 5,
        },
        "neutral_cursor": {"input_client_point": {"x": 400, "y": 300}},
        "regions": regions,
        "signature_identity": {"dialog_resource_id": 226},
        "environment_identity": {
            "presentation_configuration": {
                "surface_format": "B8G8R8A8_UNORM"
            }
        },
    }


def _write_json(path: Path, value: object) -> str:
    raw = json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")
    path.write_bytes(raw)
    return hashlib.sha256(raw).hexdigest()


def _write_fixture(root: Path, frame: bytes) -> tuple[Path, Path, str]:
    capture = root / "capture"
    capture.mkdir()
    _write_json(capture / "capture.json", _capture_manifest())
    (capture / "frame.bgra").write_bytes(frame)
    guard = root / "guard.json"
    guard_digest = _write_json(guard, _guard_for_frame(frame))
    return capture, guard, guard_digest


class CropTests(unittest.TestCase):
    def test_crop_is_tight_and_half_open(self) -> None:
        pixels = [
            bytes((index, index + 1, index + 2, 255)) for index in range(0, 24, 3)
        ]
        frame = b"".join(pixels)
        actual = crop_tight_bgra(frame, 4, 2, (1, 0, 3, 2))
        self.assertEqual(actual, b"".join((pixels[1], pixels[2], pixels[5], pixels[6])))

    def test_crop_rejects_invalid_bounds_and_length(self) -> None:
        frame = bytes(4 * 4 * 4)
        invalid_rectangles = (
            (-1, 0, 1, 1),
            (0, -1, 1, 1),
            (0, 0, 5, 1),
            (0, 0, 1, 5),
            (1, 0, 1, 1),
            (0, 1, 1, 1),
        )
        for rectangle in invalid_rectangles:
            with self.subTest(rectangle=rectangle):
                with self.assertRaises(ValidationError):
                    crop_tight_bgra(frame, 4, 4, rectangle)
        with self.assertRaisesRegex(ValidationError, "frame byte length"):
            crop_tight_bgra(frame[:-1], 4, 4, (0, 0, 1, 1))

    def test_cursor_overlap_uses_half_open_edges(self) -> None:
        rectangle = Rect(10, 20, 30, 40)
        self.assertTrue(cursor_overlaps_rect(10, 20, rectangle))
        self.assertTrue(cursor_overlaps_rect(29, 39, rectangle))
        self.assertFalse(cursor_overlaps_rect(30, 39, rectangle))
        self.assertFalse(cursor_overlaps_rect(29, 40, rectangle))

    def test_point_scale_fixture_repeats_each_source_pixel_at_scale_two(self) -> None:
        pixels = (
            bytes((1, 2, 3, 4)),
            bytes((5, 6, 7, 8)),
            bytes((9, 10, 11, 12)),
            bytes((13, 14, 15, 16)),
        )
        frame = b"".join(pixels)
        actual = point_scale_presentation_crop_bgra(
            frame,
            2,
            2,
            Rect(0, 0, 4, 4),
            Rect(0, 0, 4, 4),
            2,
            1,
        )
        expected = b"".join(
            (
                pixels[0],
                pixels[0],
                pixels[1],
                pixels[1],
                pixels[0],
                pixels[0],
                pixels[1],
                pixels[1],
                pixels[2],
                pixels[2],
                pixels[3],
                pixels[3],
                pixels[2],
                pixels[2],
                pixels[3],
                pixels[3],
            )
        )
        self.assertEqual(actual, expected)

    def test_presentation_crop_uses_full_surface_absolute_point_mapping(self) -> None:
        frame = _frame()
        options_crop = point_scale_presentation_crop_bgra(
            frame,
            LOGICAL_WIDTH,
            LOGICAL_HEIGHT,
            Rect(*CONTENT_RECT),
            Rect(*EXPECTED_PRESENTATION_REGION_RECTS["options"]),
            SCALE_NUMERATOR,
            SCALE_DENOMINATOR,
        )
        first_source_x = ((2 * (1399 - 240) + 1) * 5) // 18
        first_source_y = ((2 * 660 + 1) * 5) // 18
        self.assertEqual(first_source_x, 644)
        # The physical crop includes the adjacent row above logical_rect.top.
        self.assertEqual(first_source_y, 366)
        self.assertLess(first_source_y, EXPECTED_REGION_RECTS["options"][1])
        source_offset = (
            (first_source_y * LOGICAL_WIDTH + first_source_x) * 4
        )
        self.assertEqual(options_crop[:4], frame[source_offset : source_offset + 4])


class ValidationTests(unittest.TestCase):
    def test_valid_capture_recomputes_manifest_and_frame_digests(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            frame = _frame()
            capture, _, _ = _write_fixture(root, frame)
            validated = validate_capture_bundle(capture)
            self.assertEqual(validated.frame_sha256, sha256_bytes(frame))
            self.assertEqual(validated.width, 800)
            self.assertEqual(validated.height, 600)
            self.assertEqual(validated.surface_format, "Bgra8Unorm")

    def test_legacy_v1_capture_remains_readable(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            frame = _frame()
            capture = root / "capture"
            capture.mkdir()
            manifest = _capture_manifest()
            manifest["schema_version"] = "vera20k.shell-capture.v1"
            del manifest["shell"]["title_terminal_persistent"]
            _write_json(capture / "capture.json", manifest)
            (capture / "frame.bgra").write_bytes(frame)
            self.assertEqual(validate_capture_bundle(capture).width, 800)

    def test_current_capture_operation_rejects_legacy_v1(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            frame = _frame()
            capture = root / "capture"
            capture.mkdir()
            manifest = _capture_manifest()
            manifest["schema_version"] = "vera20k.shell-capture.v1"
            del manifest["shell"]["title_terminal_persistent"]
            _write_json(capture / "capture.json", manifest)
            (capture / "frame.bgra").write_bytes(frame)
            with self.assertRaisesRegex(ValidationError, "must be current"):
                validate_capture_bundle(
                    capture,
                    required_schema_version="vera20k.shell-capture.v2",
                )

    def test_v2_capture_requires_terminal_retained_title(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            frame = _frame()
            capture = root / "capture"
            capture.mkdir()
            manifest = _capture_manifest()
            manifest["shell"]["title_terminal_persistent"] = False
            _write_json(capture / "capture.json", manifest)
            (capture / "frame.bgra").write_bytes(frame)
            with self.assertRaisesRegex(ValidationError, "must be true"):
                validate_capture_bundle(capture)

    def test_capture_rejects_unknown_schema_fields(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            frame = _frame()
            capture, _, _ = _write_fixture(root, frame)
            manifest = _capture_manifest()
            manifest["untrusted_extension"] = True
            _write_json(capture / "capture.json", manifest)
            with self.assertRaisesRegex(ValidationError, "unexpected"):
                validate_capture_bundle(capture)

    def test_capture_rejects_duplicate_json_keys(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            capture = Path(temporary) / "capture"
            capture.mkdir()
            valid = json.dumps(_capture_manifest(), separators=(",", ":"))
            duplicate = valid[:-1] + ',"checkpoint":"main-menu-0xe2-steady"}'
            (capture / "capture.json").write_text(duplicate, encoding="utf-8")
            (capture / "frame.bgra").write_bytes(_frame())
            with self.assertRaisesRegex(ValidationError, "duplicate JSON object key"):
                validate_capture_bundle(capture)

    def test_overflowing_numbers_are_rejected_on_read_and_never_written(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            capture = Path(temporary) / "capture"
            capture.mkdir()
            valid = json.dumps(_capture_manifest(), separators=(",", ":"))
            (capture / "capture.json").write_text(
                valid[:-1] + ',"value":1e999}', encoding="utf-8"
            )
            (capture / "frame.bgra").write_bytes(_frame())
            with self.assertRaisesRegex(ValidationError, "non-finite"):
                validate_capture_bundle(capture)
            output = Path(temporary) / "report.json"
            with self.assertRaises(ValueError):
                write_json_exclusive(output, {"value": float("inf")})
            self.assertFalse(output.exists())

    def test_capture_rejects_wrong_raw_length(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            capture = root / "capture"
            capture.mkdir()
            _write_json(capture / "capture.json", _capture_manifest())
            (capture / "frame.bgra").write_bytes(bytes(31))
            with self.assertRaisesRegex(ValidationError, "byte length"):
                validate_capture_bundle(capture)

    def test_guard_requires_both_sealed_digest_and_valid_state(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            frame = _frame()
            _, guard_path, guard_digest = _write_fixture(root, frame)
            validated = validate_guard(guard_path, expected_sha256=guard_digest)
            self.assertEqual(len(validated.regions), 4)

            with self.assertRaisesRegex(ValidationError, "SHA-256 mismatch"):
                validate_guard(guard_path, expected_sha256="0" * 64)

            invalid_guard = _guard_for_frame(frame)
            invalid_guard["state"] = "single_player_0x100"
            invalid_digest = _write_json(guard_path, invalid_guard)
            with self.assertRaisesRegex(ValidationError, "guard.state"):
                validate_guard(guard_path, expected_sha256=invalid_digest)

    def test_guard_rejects_changed_presentation_mapping(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            frame = _frame()
            guard = _guard_for_frame(frame)
            guard["regions"][2]["presentation_rect"]["top"] = 661
            path = root / "guard.json"
            digest = _write_json(path, guard)
            with self.assertRaisesRegex(ValidationError, "presentation_rect"):
                validate_guard(path, expected_sha256=digest)

    def test_guard_rejects_cursor_overlap(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            frame = _frame()
            guard = _guard_for_frame(frame)
            guard["neutral_cursor"]["input_client_point"] = {"x": 650, "y": 210}
            path = root / "guard.json"
            digest = _write_json(path, guard)
            with self.assertRaisesRegex(ValidationError, "neutral cursor x"):
                validate_guard(path, expected_sha256=digest)


class ComparisonTests(unittest.TestCase):
    def test_all_regions_match_raw_executable_output(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            frame = _frame()
            capture, guard, guard_digest = _write_fixture(root, frame)
            report = build_comparison_report(
                capture, guard, expected_guard_sha256=guard_digest
            )
            self.assertEqual(report["status"], MATCH)
            self.assertEqual(len(report["regions"]), 4)
            self.assertTrue(
                all(region["status"] == MATCH for region in report["regions"])
            )
            for region in report["regions"]:
                self.assertIn("logical_rect", region)
                self.assertIn("presentation_rect", region)
                self.assertIn(
                    "actual_presentation_raw_bgra_sha256", region
                )
                self.assertNotIn("actual_sha256", region)
            options_expected = next(
                region["expected_surface_raw_bgra_sha256"]
                for region in report["regions"]
                if region["name"] == "options"
            )
            direct_logical_digest = sha256_bytes(
                crop_tight_bgra(
                    frame,
                    LOGICAL_WIDTH,
                    LOGICAL_HEIGHT,
                    EXPECTED_REGION_RECTS["options"],
                )
            )
            self.assertNotEqual(direct_logical_digest, options_expected)

    def test_one_changed_region_is_drift(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            frame = _frame()
            capture, guard, guard_digest = _write_fixture(root, frame)
            changed = bytearray(frame)
            left, top, _, _ = EXPECTED_REGION_RECTS["options"]
            changed[(top * LOGICAL_WIDTH + left) * 4] ^= 0xFF
            (capture / "frame.bgra").write_bytes(changed)
            report = build_comparison_report(
                capture, guard, expected_guard_sha256=guard_digest
            )
            self.assertEqual(report["status"], DRIFT)
            statuses = {region["name"]: region["status"] for region in report["regions"]}
            self.assertEqual(statuses["options"], DRIFT)
            self.assertEqual(statuses["title"], MATCH)

    def test_invalid_bundle_retains_four_region_diagnostics(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            frame = _frame()
            capture, guard, guard_digest = _write_fixture(root, frame)
            (capture / "frame.bgra").write_bytes(b"truncated")
            report = build_comparison_report(
                capture, guard, expected_guard_sha256=guard_digest
            )
            self.assertEqual(report["status"], INVALID)
            self.assertEqual(len(report["regions"]), 4)
            self.assertTrue(
                all(region["status"] == INVALID for region in report["regions"])
            )
            self.assertTrue(report["errors"])

    def test_compare_output_is_exclusive(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            frame = _frame()
            capture, guard, guard_digest = _write_fixture(root, frame)
            output = root / "comparison.json"
            report = compare_to_file(
                capture,
                guard,
                output,
                expected_guard_sha256=guard_digest,
            )
            self.assertEqual(report["status"], MATCH)
            original = output.read_bytes()
            with self.assertRaises(OutputExistsError):
                compare_to_file(
                    capture,
                    guard,
                    output,
                    expected_guard_sha256=guard_digest,
                )
            self.assertEqual(output.read_bytes(), original)

    def test_additional_process_error_forces_invalid_but_keeps_hashes(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            frame = _frame()
            capture, guard, guard_digest = _write_fixture(root, frame)
            report = build_comparison_report(
                capture,
                guard,
                expected_guard_sha256=guard_digest,
                additional_errors=("capture child exited with status 7",),
            )
            self.assertEqual(report["status"], INVALID)
            self.assertEqual(len(report["regions"]), 4)
            self.assertTrue(
                all(
                    region["actual_presentation_raw_bgra_sha256"]
                    for region in report["regions"]
                )
            )


if __name__ == "__main__":
    unittest.main()
