"""Tests for the sealed 0x694 title differential."""

from __future__ import annotations

import os
import unittest
from pathlib import Path

from tools.shell_certification.cli import build_parser
from tools.shell_certification.core import Rect, ValidationError
from tools.shell_certification.title_differential import (
    BASE_RGB,
    HIGHLIGHT_RGB,
    build_title_differential_report,
    collapse_presentation_crop_to_logical,
    exhaustive_mask_shifts,
    path_a_encoded_rgb,
    presentation_bgra_for_encoded_rgb,
    rightmost_unit_mask,
)


def _configured_path(variable: str) -> Path:
    value = os.environ.get(variable)
    return Path(value) if value else Path(f"__unconfigured_{variable.lower()}__")


SEALED_GUARD = _configured_path("VERA20K_SHELL_GUARD")
ORACLE_RUNS = _configured_path("VERA20K_ORACLE_RUNS")
CURRENT_CAPTURE = _configured_path("VERA20K_SHELL_CAPTURE")


class _SyntheticGuard:
    content_rect = Rect(0, 0, 4, 4)
    scale_numerator = 2
    scale_denominator = 1


class TitleDifferentialTests(unittest.TestCase):
    def test_path_a_terminal_vector_is_encoded_not_presenter_expanded(self) -> None:
        self.assertIsNone(
            path_a_encoded_rgb(9, 9, 8, BASE_RGB, HIGHLIGHT_RGB)
        )
        self.assertEqual(
            path_a_encoded_rgb(9, 17, 8, BASE_RGB, HIGHLIGHT_RGB),
            (248, 252, 30),
        )
        self.assertEqual(
            path_a_encoded_rgb(1, 17, 8, BASE_RGB, HIGHLIGHT_RGB),
            (248, 252, 0),
        )

    def test_path_a_negative_delta_truncates_toward_zero(self) -> None:
        self.assertEqual(
            path_a_encoded_rgb(1, 2, 8, (255, 255, 255), (0, 0, 0)),
            (8, 8, 8),
        )

    def test_rgb565_codebooks_expand_packed_indices(self) -> None:
        five = tuple(index * 8 for index in range(31)) + (255,)
        six = tuple(index * 4 for index in range(63)) + (255,)
        self.assertEqual(
            presentation_bgra_for_encoded_rgb((255, 255, 30), five, six),
            bytes((24, 255, 255, 255)),
        )

    def test_collapse_requires_uniform_point_replicas(self) -> None:
        logical = [
            bytes((1, 2, 3, 255)),
            bytes((4, 5, 6, 255)),
            bytes((7, 8, 9, 255)),
            bytes((10, 11, 12, 255)),
        ]
        physical = b"".join(
            logical[(y // 2) * 2 + x // 2]
            for y in range(4)
            for x in range(4)
        )
        collapsed = collapse_presentation_crop_to_logical(
            physical,
            _SyntheticGuard(),  # type: ignore[arg-type]
            Rect(0, 0, 2, 2),
            Rect(0, 0, 4, 4),
        )
        self.assertEqual(collapsed, b"".join(logical))

        changed = bytearray(physical)
        changed[4:8] = bytes((99, 99, 99, 255))
        with self.assertRaisesRegex(ValidationError, "replicas disagree"):
            collapse_presentation_crop_to_logical(
                changed,
                _SyntheticGuard(),  # type: ignore[arg-type]
                Rect(0, 0, 2, 2),
                Rect(0, 0, 4, 4),
            )

    def test_exhaustive_shift_and_terminal_run_are_content_agnostic(self) -> None:
        source = {(1, 1), (2, 1), (4, 1), (4, 2)}
        target = {(2, 1), (3, 1), (5, 1), (5, 2)}
        results = exhaustive_mask_shifts(source, target, 8, 4)
        self.assertEqual(results[0], (0, 1, 0))
        self.assertEqual(
            [(dx, dy) for mismatch, dx, dy in results if mismatch == 0],
            [(1, 0)],
        )
        self.assertEqual(rightmost_unit_mask(source), {(4, 1), (4, 2)})

    def test_cli_parses_title_differential_command(self) -> None:
        arguments = build_parser().parse_args(
            [
                "title-differential",
                "--capture",
                "capture",
                "--guard",
                "guard.json",
                "--oracle-runs",
                "runs",
                "--output",
                "report.json",
            ]
        )
        self.assertEqual(arguments.command_name, "title-differential")

    @unittest.skipUnless(
        SEALED_GUARD.is_file()
        and ORACLE_RUNS.is_dir()
        and CURRENT_CAPTURE.is_dir()
        and os.name == "nt",
        "sealed local title evidence is unavailable",
    )
    def test_current_sealed_evidence_is_red_but_predictive_transform_is_exact(
        self,
    ) -> None:
        report = build_title_differential_report(
            CURRENT_CAPTURE, SEALED_GUARD, ORACLE_RUNS
        )
        self.assertEqual(report["status"], "DRIFT")
        self.assertEqual(report["masks"]["current_pixels"], 243)
        self.assertEqual(report["masks"]["native_pixels"], 243)
        self.assertEqual(report["masks"]["terminal_unit_pixels"], 29)
        self.assertEqual(
            report["geometry"]["unique_exact_mask_shift"], {"dx": 1, "dy": 0}
        )
        comparisons = report["comparisons"]
        self.assertEqual(
            comparisons["current_rust"]["logical"]["mismatch_pixels"], 240
        )
        self.assertEqual(
            comparisons["current_rust"]["presentation"]["mismatch_pixels"],
            819,
        )
        self.assertEqual(
            comparisons["shift_only"]["logical"]["mismatch_pixels"], 29
        )
        self.assertEqual(
            comparisons["shift_only"]["presentation"]["mismatch_pixels"], 95
        )
        self.assertEqual(
            comparisons["shift_plus_derived_tint"]["logical"][
                "mismatch_pixels"
            ],
            0,
        )
        self.assertEqual(
            comparisons["shift_plus_derived_tint"]["presentation"][
                "mismatch_pixels"
            ],
            0,
        )
        self.assertEqual(
            comparisons["shift_plus_derived_tint"]["presentation"]["sha256"],
            "f8a87d35f9225a3d9c8e1d313ac42684eec788e49470884eaa1564ae3e613f6b",
        )


if __name__ == "__main__":
    unittest.main()
