"""Tests for the additive 0xE2 entry-sequence evidence contract."""

from __future__ import annotations

import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools.shell_certification.cli import build_parser
from tools.shell_certification.core import ValidationError
from tools.shell_certification.entry_sequence import (
    CHECKPOINT,
    FRAME_BYTE_LENGTH,
    FRAME_COUNT,
    MANIFEST_FILENAME,
    PAYLOAD_BYTE_LENGTH,
    PAYLOAD_FILENAME,
    SCHEMA_VERSION,
    build_entry_sequence_command,
    capture_entry_sequence,
    validate_entry_sequence_bundle,
)


def _manifest() -> dict[str, object]:
    return {
        "schema_version": SCHEMA_VERSION,
        "checkpoint": CHECKPOINT,
        "surface": {
            "width": 800,
            "height": 600,
            "format": "Bgra8UnormSrgb",
            "pixel_layout": "BGRA8",
            "row_order": "top-left",
            "bytes_per_pixel": 4,
            "row_stride": 3200,
        },
        "cursor": {
            "x": 400,
            "y": 300,
            "policy": "software-composited",
        },
        "shell": {
            "screen": "main-menu",
            "dialog_resource_id": 0x00E2,
            "movie_owner": "main-menu-0xe2",
            "movie_base": "ra2ts-l",
            "title_hidden_during_frames": True,
        },
        "presenter_domain": "final-swapchain-after-rgb565",
        "generation": 17,
        "completion_observed": True,
        "payload": {
            "path": PAYLOAD_FILENAME,
            "byte_length": PAYLOAD_BYTE_LENGTH,
        },
        "frames": [
            {
                "tick": tick,
                "byte_offset": tick * FRAME_BYTE_LENGTH,
                "byte_length": FRAME_BYTE_LENGTH,
            }
            for tick in range(FRAME_COUNT)
        ],
    }


def _write_bundle(directory: Path, manifest: dict[str, object] | None = None) -> None:
    directory.mkdir()
    (directory / MANIFEST_FILENAME).write_text(
        json.dumps(manifest or _manifest(), sort_keys=True),
        encoding="utf-8",
    )
    with (directory / PAYLOAD_FILENAME).open("wb") as stream:
        stream.truncate(PAYLOAD_BYTE_LENGTH)


class EntrySequenceValidationTests(unittest.TestCase):
    def test_valid_bundle_reports_every_frame_hash(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            bundle = Path(temp) / "bundle"
            _write_bundle(bundle)
            result = validate_entry_sequence_bundle(bundle)
        self.assertEqual(result["schema_version"], SCHEMA_VERSION)
        self.assertEqual(result["checkpoint"], CHECKPOINT)
        self.assertEqual(result["generation"], 17)
        self.assertEqual(len(result["frame_sha256"]), FRAME_COUNT)
        self.assertEqual(len(set(result["frame_sha256"])), 1)

    def test_duplicate_json_key_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            bundle = Path(temp) / "bundle"
            _write_bundle(bundle)
            raw = (bundle / MANIFEST_FILENAME).read_text(encoding="utf-8")
            duplicate = raw[:-1] + ',"checkpoint":"main-menu-0xe2-entry-sequence"}'
            (bundle / MANIFEST_FILENAME).write_text(duplicate, encoding="utf-8")
            with self.assertRaisesRegex(ValidationError, "duplicate JSON object key"):
                validate_entry_sequence_bundle(bundle)

    def test_wrong_tick_order_and_unexpected_file_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            bad_order = root / "bad-order"
            manifest = _manifest()
            frames = manifest["frames"]
            assert isinstance(frames, list)
            frames[4]["tick"] = 5
            _write_bundle(bad_order, manifest)
            with self.assertRaisesRegex(ValidationError, r"frames\[4\]\.tick"):
                validate_entry_sequence_bundle(bad_order)

            unexpected = root / "unexpected"
            _write_bundle(unexpected)
            (unexpected / "extra.txt").write_text("no", encoding="utf-8")
            with self.assertRaisesRegex(ValidationError, "file inventory"):
                validate_entry_sequence_bundle(unexpected)

    def test_payload_length_is_exact(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            bundle = Path(temp) / "bundle"
            _write_bundle(bundle)
            with (bundle / PAYLOAD_FILENAME).open("r+b") as stream:
                stream.truncate(PAYLOAD_BYTE_LENGTH - 1)
            with self.assertRaisesRegex(ValidationError, "byte length"):
                validate_entry_sequence_bundle(bundle)


class EntrySequenceRunnerTests(unittest.TestCase):
    def test_command_is_exact_and_cli_keeps_steady_commands(self) -> None:
        executable = Path(r"C:\vera\vera20k.exe")
        run_dir = Path(r"C:\evidence\entry")
        self.assertEqual(
            build_entry_sequence_command(executable, run_dir),
            [
                str(executable),
                "--shell-capture",
                CHECKPOINT,
                "--width",
                "800",
                "--height",
                "600",
                "--cursor-x",
                "400",
                "--cursor-y",
                "300",
                "--output",
                str(run_dir),
            ],
        )
        parser = build_parser()
        self.assertEqual(
            parser.parse_args(
                [
                    "validate-entry-sequence",
                    "--capture",
                    str(run_dir),
                ]
            ).command_name,
            "validate-entry-sequence",
        )
        self.assertEqual(
            parser.parse_args(
                [
                    "compare",
                    "--capture",
                    "capture",
                    "--guard",
                    "guard",
                    "--output",
                    "output",
                ]
            ).command_name,
            "compare",
        )

    def test_timeout_kills_only_the_exact_popen_child(self) -> None:
        class FakeChild:
            pid = 4242
            returncode: int | None = None

            def __init__(self) -> None:
                self.wait_calls = 0
                self.kill_calls = 0

            def wait(self, timeout: float) -> None:
                self.wait_calls += 1
                if self.wait_calls == 1:
                    raise subprocess.TimeoutExpired(["vera20k"], timeout)
                self.returncode = -9

            def kill(self) -> None:
                self.kill_calls += 1

        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            executable = root / "vera20k.exe"
            executable.write_bytes(b"exe")
            working = root / "working"
            working.mkdir()
            (working / "config.toml").write_text("[paths]\n", encoding="utf-8")
            run_dir = root / "run"
            child = FakeChild()
            with mock.patch(
                "tools.shell_certification.entry_sequence.subprocess.Popen",
                return_value=child,
            ):
                with self.assertRaisesRegex(ValidationError, "exceeded"):
                    capture_entry_sequence(
                        executable,
                        run_dir,
                        working_directory=working,
                        timeout_seconds=0.1,
                    )
            self.assertEqual(child.kill_calls, 1)
            self.assertEqual(child.pid, 4242)


if __name__ == "__main__":
    unittest.main()
