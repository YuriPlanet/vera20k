"""Unit tests for fail-closed shell presentation-profile derivation."""

from __future__ import annotations

import hashlib
import io
import json
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

from tools.shell_certification.cli import build_parser, main as cli_main
from tools.shell_certification.core import OutputExistsError, ValidationError
from tools.shell_certification.presentation_profile import (
    PROFILE_SCHEMA_VERSION,
    derive_channel_codebooks,
    derive_presentation_profile,
    write_presentation_profile,
)


FIVE_BIT = (
    0,
    8,
    16,
    25,
    33,
    41,
    49,
    58,
    66,
    74,
    82,
    90,
    99,
    107,
    115,
    123,
    132,
    140,
    148,
    156,
    164,
    173,
    181,
    189,
    197,
    206,
    214,
    222,
    230,
    238,
    247,
    255,
)
SIX_BIT = (
    0,
    4,
    8,
    12,
    16,
    20,
    24,
    28,
    32,
    36,
    40,
    45,
    49,
    53,
    57,
    61,
    65,
    69,
    73,
    77,
    81,
    85,
    89,
    93,
    97,
    101,
    105,
    109,
    113,
    117,
    121,
    125,
    129,
    133,
    138,
    142,
    146,
    150,
    154,
    158,
    162,
    166,
    170,
    174,
    178,
    182,
    186,
    190,
    194,
    198,
    202,
    206,
    210,
    214,
    219,
    223,
    227,
    231,
    235,
    239,
    243,
    247,
    251,
    255,
)
WIDTH = 8
HEIGHT = 8
ENVIRONMENT_SHA256 = "a" * 64


def _frame(
    *,
    blue: tuple[int, ...] = FIVE_BIT,
    green: tuple[int, ...] = SIX_BIT,
    red: tuple[int, ...] | None = None,
    alpha: tuple[int, ...] = (255,),
    offset: int = 0,
) -> bytes:
    red_values = blue if red is None else red
    pixels = bytearray()
    for index in range(WIDTH * HEIGHT):
        shifted = index + offset
        pixels.extend(
            (
                blue[shifted % len(blue)],
                green[shifted % len(green)],
                red_values[shifted % len(red_values)],
                alpha[shifted % len(alpha)],
            )
        )
    return bytes(pixels)


class _Fixture:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.guard_path = root / "guard.json"
        self.runs_root = root / "runs"
        self.runs_root.mkdir()
        self.sources: list[dict[str, str]] = []
        for index in range(3):
            run_id = f"run-{index}"
            surface_blob = "capture/frame.bgra"
            frame = _frame(offset=index)
            frame_path = self.runs_root / run_id / surface_blob
            frame_path.parent.mkdir(parents=True)
            frame_path.write_bytes(frame)
            digest = hashlib.sha256(frame).hexdigest()
            self.sources.append(
                {
                    "run_id": run_id,
                    "frame_blob": surface_blob,
                    "frame_pixel_sha256": digest,
                    "surface_blob": surface_blob,
                    "surface_pixel_sha256": digest,
                    "environment_identity_sha256": ENVIRONMENT_SHA256,
                }
            )
        self.rewrite_guard()

    def rewrite_guard(self) -> None:
        document = {
            "sources": self.sources,
            "environment_identity": {
                "presentation_configuration": {
                    "surface_format": "B8G8R8A8_UNORM"
                }
            },
        }
        raw = json.dumps(
            document, ensure_ascii=False, sort_keys=True, separators=(",", ":")
        ).encode("utf-8")
        self.guard_path.write_bytes(raw)
        self.guard_sha256 = hashlib.sha256(raw).hexdigest()

    def validated_guard(self) -> SimpleNamespace:
        return SimpleNamespace(
            path=self.guard_path.absolute(),
            sha256=self.guard_sha256,
            presentation_width=WIDTH,
            presentation_height=HEIGHT,
        )

    def validation_patch(self) -> mock._patch:
        return mock.patch(
            "tools.shell_certification.presentation_profile.validate_guard",
            return_value=self.validated_guard(),
        )

    def replace_source_frame(self, index: int, frame: bytes) -> None:
        source = self.sources[index]
        frame_path = (
            self.runs_root / source["run_id"] / source["surface_blob"]
        )
        frame_path.write_bytes(frame)
        digest = hashlib.sha256(frame).hexdigest()
        source["frame_pixel_sha256"] = digest
        source["surface_pixel_sha256"] = digest

    def replace_all_frames(self, frame: bytes) -> None:
        for index in range(3):
            self.replace_source_frame(index, frame)
        self.rewrite_guard()


class PresentationProfileTests(unittest.TestCase):
    def test_channel_codebooks_validate_length_and_sort_values(self) -> None:
        frame = bytes((8, 12, 16, 255, 0, 4, 8, 255))
        self.assertEqual(
            derive_channel_codebooks(frame, 2, 1),
            ((0, 8), (4, 12), (8, 16), (255,)),
        )
        with self.assertRaisesRegex(ValidationError, "byte length"):
            derive_channel_codebooks(frame[:-1], 2, 1)

    def test_exact_three_sources_derive_opaque_32_64_32_profile(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = _Fixture(Path(temporary))
            with fixture.validation_patch() as validate_guard:
                profile = derive_presentation_profile(
                    fixture.guard_path,
                    fixture.runs_root,
                    expected_guard_sha256=fixture.guard_sha256,
                )

            validate_guard.assert_called_once_with(
                fixture.guard_path,
                expected_sha256=fixture.guard_sha256,
            )
            self.assertEqual(profile["schema_version"], PROFILE_SCHEMA_VERSION)
            self.assertEqual(
                profile["evidence_status"], "DERIVED_FROM_SEALED_NATIVE_SOURCES"
            )
            self.assertEqual(profile["parity_certification"], "NONE")
            self.assertEqual(profile["codebooks"]["five_bit"], list(FIVE_BIT))
            self.assertEqual(profile["codebooks"]["six_bit"], list(SIX_BIT))
            self.assertEqual(
                profile["channel_cardinalities"],
                {"blue": 32, "green": 64, "red": 32, "alpha": 1},
            )
            self.assertEqual(profile["alpha_values"], [255])
            self.assertEqual(
                profile["environment_identity_sha256"], ENVIRONMENT_SHA256
            )
            self.assertEqual(len(profile["sources"]), 3)
            for expected, actual in zip(fixture.sources, profile["sources"]):
                self.assertEqual(actual["run_id"], expected["run_id"])
                self.assertEqual(
                    actual["frame_pixel_sha256"],
                    expected["frame_pixel_sha256"],
                )
                self.assertEqual(
                    actual["surface_pixel_sha256"],
                    expected["surface_pixel_sha256"],
                )

    def test_profile_is_deterministic_apart_from_generated_timestamp(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = _Fixture(Path(temporary))
            timestamps = (
                "2026-07-25T20:00:00.000000Z",
                "2026-07-25T20:00:01.000000Z",
            )
            with fixture.validation_patch():
                with mock.patch(
                    "tools.shell_certification.presentation_profile.utc_now",
                    side_effect=timestamps,
                ):
                    first = derive_presentation_profile(
                        fixture.guard_path,
                        fixture.runs_root,
                        expected_guard_sha256=fixture.guard_sha256,
                    )
                    second = derive_presentation_profile(
                        fixture.guard_path,
                        fixture.runs_root,
                        expected_guard_sha256=fixture.guard_sha256,
                    )
            self.assertEqual(first.pop("generated_at_utc"), timestamps[0])
            self.assertEqual(second.pop("generated_at_utc"), timestamps[1])
            self.assertEqual(first, second)

    def test_write_is_canonical_and_refuses_existing_output(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = _Fixture(Path(temporary))
            output = fixture.root / "profile.json"
            with fixture.validation_patch():
                profile = write_presentation_profile(
                    fixture.guard_path,
                    fixture.runs_root,
                    output,
                    expected_guard_sha256=fixture.guard_sha256,
                )
            expected = (
                json.dumps(
                    profile,
                    ensure_ascii=False,
                    indent=2,
                    sort_keys=True,
                )
                + "\n"
            )
            self.assertEqual(output.read_text(encoding="utf-8"), expected)

            before = output.read_bytes()
            with self.assertRaisesRegex(OutputExistsError, "overwrite"):
                write_presentation_profile(
                    fixture.guard_path,
                    fixture.runs_root,
                    output,
                    expected_guard_sha256=fixture.guard_sha256,
                )
            self.assertEqual(output.read_bytes(), before)

    def test_requires_exactly_three_guard_sources(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = _Fixture(Path(temporary))
            fixture.sources.pop()
            fixture.rewrite_guard()
            with fixture.validation_patch():
                with self.assertRaisesRegex(ValidationError, "2 entries, expected 3"):
                    derive_presentation_profile(
                        fixture.guard_path,
                        fixture.runs_root,
                        expected_guard_sha256=fixture.guard_sha256,
                    )

    def test_rejects_wrong_frame_length(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = _Fixture(Path(temporary))
            source = fixture.sources[0]
            frame_path = (
                fixture.runs_root / source["run_id"] / source["surface_blob"]
            )
            frame_path.write_bytes(frame_path.read_bytes()[:-1])
            with fixture.validation_patch():
                with self.assertRaisesRegex(ValidationError, "byte length"):
                    derive_presentation_profile(
                        fixture.guard_path,
                        fixture.runs_root,
                        expected_guard_sha256=fixture.guard_sha256,
                    )

    def test_rejects_non_opaque_alpha(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = _Fixture(Path(temporary))
            fixture.replace_all_frames(_frame(alpha=(255, 254)))
            with fixture.validation_patch():
                with self.assertRaisesRegex(ValidationError, "alpha values"):
                    derive_presentation_profile(
                        fixture.guard_path,
                        fixture.runs_root,
                        expected_guard_sha256=fixture.guard_sha256,
                    )

    def test_rejects_mismatched_source_codebooks(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = _Fixture(Path(temporary))
            changed_five_bit = (0, 7, *FIVE_BIT[2:])
            fixture.replace_source_frame(
                2, _frame(blue=changed_five_bit, offset=2)
            )
            fixture.rewrite_guard()
            with fixture.validation_patch():
                with self.assertRaisesRegex(ValidationError, "codebooks disagree"):
                    derive_presentation_profile(
                        fixture.guard_path,
                        fixture.runs_root,
                        expected_guard_sha256=fixture.guard_sha256,
                    )

    def test_rejects_wrong_channel_cardinality(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = _Fixture(Path(temporary))
            fixture.replace_source_frame(0, _frame(blue=FIVE_BIT[:-1]))
            fixture.rewrite_guard()
            with fixture.validation_patch():
                with self.assertRaisesRegex(ValidationError, "cardinalities"):
                    derive_presentation_profile(
                        fixture.guard_path,
                        fixture.runs_root,
                        expected_guard_sha256=fixture.guard_sha256,
                    )

    def test_rejects_blue_red_disagreement(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = _Fixture(Path(temporary))
            changed_red = (0, 7, *FIVE_BIT[2:])
            fixture.replace_all_frames(_frame(red=changed_red))
            with fixture.validation_patch():
                with self.assertRaisesRegex(
                    ValidationError, "blue and red five-bit codebooks disagree"
                ):
                    derive_presentation_profile(
                        fixture.guard_path,
                        fixture.runs_root,
                        expected_guard_sha256=fixture.guard_sha256,
                    )

    def test_rejects_path_escape(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = _Fixture(Path(temporary))
            fixture.sources[0]["surface_blob"] = "../outside.bgra"
            fixture.sources[0]["frame_blob"] = "../outside.bgra"
            fixture.rewrite_guard()
            with fixture.validation_patch():
                with self.assertRaisesRegex(ValidationError, "traversal-free"):
                    derive_presentation_profile(
                        fixture.guard_path,
                        fixture.runs_root,
                        expected_guard_sha256=fixture.guard_sha256,
                    )

    def test_rejects_linked_source(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = _Fixture(Path(temporary))
            source = fixture.sources[0]
            target = (
                fixture.runs_root / source["run_id"] / source["surface_blob"]
            )
            link = target.parent / "linked-frame.bgra"
            try:
                link.symlink_to(target)
            except OSError as exc:
                self.skipTest(f"symbolic links are unavailable: {exc}")
            source["surface_blob"] = "capture/linked-frame.bgra"
            source["frame_blob"] = "capture/linked-frame.bgra"
            fixture.rewrite_guard()
            with fixture.validation_patch():
                with self.assertRaisesRegex(ValidationError, "link"):
                    derive_presentation_profile(
                        fixture.guard_path,
                        fixture.runs_root,
                        expected_guard_sha256=fixture.guard_sha256,
                    )

    def test_rejects_non_file_source(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = _Fixture(Path(temporary))
            source = fixture.sources[0]
            directory = (
                fixture.runs_root
                / source["run_id"]
                / "capture"
                / "frame-directory"
            )
            directory.mkdir()
            source["surface_blob"] = "capture/frame-directory"
            source["frame_blob"] = "capture/frame-directory"
            fixture.rewrite_guard()
            with fixture.validation_patch():
                with self.assertRaisesRegex(ValidationError, "regular file"):
                    derive_presentation_profile(
                        fixture.guard_path,
                        fixture.runs_root,
                        expected_guard_sha256=fixture.guard_sha256,
                    )

    def test_rejects_source_hash_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = _Fixture(Path(temporary))
            source = fixture.sources[0]
            frame_path = (
                fixture.runs_root / source["run_id"] / source["surface_blob"]
            )
            changed = bytearray(frame_path.read_bytes())
            changed[0] ^= 1
            frame_path.write_bytes(changed)
            with fixture.validation_patch():
                with self.assertRaisesRegex(ValidationError, "SHA-256 mismatch"):
                    derive_presentation_profile(
                        fixture.guard_path,
                        fixture.runs_root,
                        expected_guard_sha256=fixture.guard_sha256,
                    )

    def test_stable_read_failure_is_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = _Fixture(Path(temporary))
            from tools.shell_certification import presentation_profile

            real_read = presentation_profile._read_regular_bytes

            def fail_source_read(path: Path, label: str, **kwargs: object) -> bytes:
                if "source frame" in label:
                    raise ValidationError("source changed while being validated")
                return real_read(path, label, **kwargs)

            with fixture.validation_patch():
                with mock.patch(
                    "tools.shell_certification.presentation_profile._read_regular_bytes",
                    side_effect=fail_source_read,
                ):
                    with self.assertRaisesRegex(ValidationError, "changed"):
                        derive_presentation_profile(
                            fixture.guard_path,
                            fixture.runs_root,
                            expected_guard_sha256=fixture.guard_sha256,
                        )

    def test_rejects_disagreeing_environment_identity(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = _Fixture(Path(temporary))
            fixture.sources[2]["environment_identity_sha256"] = "b" * 64
            fixture.rewrite_guard()
            with fixture.validation_patch():
                with self.assertRaisesRegex(
                    ValidationError, "environment identity digests disagree"
                ):
                    derive_presentation_profile(
                        fixture.guard_path,
                        fixture.runs_root,
                        expected_guard_sha256=fixture.guard_sha256,
                    )

    def test_cli_parses_profile_derivation_command(self) -> None:
        arguments = build_parser().parse_args(
            [
                "derive-presentation-profile",
                "--guard",
                "guard.json",
                "--oracle-runs",
                "runs",
                "--output",
                "profile.json",
            ]
        )
        self.assertEqual(arguments.command_name, "derive-presentation-profile")
        self.assertEqual(arguments.guard, Path("guard.json"))
        self.assertEqual(arguments.oracle_runs, Path("runs"))
        self.assertEqual(arguments.output, Path("profile.json"))

    def test_cli_derives_profile_and_reports_written_identity(self) -> None:
        profile = {
            "schema_version": PROFILE_SCHEMA_VERSION,
            "guard": {"sha256": "a" * 64},
        }
        stdout = io.StringIO()
        with mock.patch(
            "tools.shell_certification.cli.write_presentation_profile",
            return_value=profile,
        ) as write_profile:
            with redirect_stdout(stdout):
                exit_code = cli_main(
                    [
                        "derive-presentation-profile",
                        "--guard",
                        "guard.json",
                        "--oracle-runs",
                        "runs",
                        "--output",
                        "profile.json",
                    ]
                )
        self.assertEqual(exit_code, 0)
        write_profile.assert_called_once_with(
            Path("guard.json"), Path("runs"), Path("profile.json")
        )
        report = json.loads(stdout.getvalue())
        self.assertEqual(report["schema_version"], PROFILE_SCHEMA_VERSION)
        self.assertEqual(report["guard_sha256"], "a" * 64)

    def test_write_rejects_link_output_target(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = _Fixture(Path(temporary))
            target = fixture.root / "target.json"
            target.write_text("sentinel", encoding="utf-8")
            output = fixture.root / "profile-link.json"
            try:
                output.symlink_to(target)
            except OSError as exc:
                self.skipTest(f"symbolic links are unavailable: {exc}")
            with self.assertRaisesRegex(OutputExistsError, "overwrite"):
                write_presentation_profile(
                    fixture.guard_path,
                    fixture.runs_root,
                    output,
                    expected_guard_sha256=fixture.guard_sha256,
                )
            self.assertEqual(target.read_text(encoding="utf-8"), "sentinel")


if __name__ == "__main__":
    unittest.main()
