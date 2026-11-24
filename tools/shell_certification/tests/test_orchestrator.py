"""Unit tests for no-overwrite and child-PID-only orchestration."""

from __future__ import annotations

import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools.shell_certification.core import (
    INVALID,
    OutputExistsError,
    ValidationError,
)
from tools.shell_certification.orchestrator import (
    _inventory_child_output,
    capture_and_compare,
)


def _write_config(directory: Path, content: bytes = b"[video]\n") -> Path:
    path = directory / "config.toml"
    path.write_bytes(content)
    return path


class OrchestratorTests(unittest.TestCase):
    def test_child_output_inventory_rejects_every_unexpected_or_non_file_entry(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            run_dir = Path(temporary)
            (run_dir / "capture.json").write_text("{}", encoding="utf-8")
            (run_dir / "frame.bgra").mkdir()
            (run_dir / "unexpected.txt").write_text("diagnostic", encoding="utf-8")

            inventory, errors = _inventory_child_output(run_dir)

            self.assertEqual(
                [entry["name"] for entry in inventory],
                ["capture.json", "frame.bgra", "unexpected.txt"],
            )
            self.assertTrue(
                any("frame.bgra" in error and "regular" in error for error in errors)
            )
            self.assertTrue(
                any("unexpected.txt" in error for error in errors)
            )

    @mock.patch("tools.shell_certification.orchestrator.validate_guard")
    @mock.patch("tools.child_process.subprocess.Popen")
    def test_relative_child_working_directory_rejects_before_launch(
        self, popen: mock.Mock, validate_guard: mock.Mock
    ) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            executable = root / "vera20k.exe"
            executable.write_bytes(b"fixture executable")
            with self.assertRaisesRegex(ValidationError, "must be absolute"):
                capture_and_compare(
                    executable,
                    root / "guard.json",
                    root / "new-run",
                    working_directory=Path("relative-resource-base"),
                    timeout_seconds=1,
                )
            popen.assert_not_called()
            validate_guard.assert_called_once()

    @mock.patch("tools.shell_certification.orchestrator.validate_guard")
    @mock.patch("tools.child_process.subprocess.Popen")
    def test_missing_config_rejects_before_launch(
        self, popen: mock.Mock, validate_guard: mock.Mock
    ) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            executable = root / "vera20k.exe"
            executable.write_bytes(b"fixture executable")
            with self.assertRaisesRegex(ValidationError, "config.toml"):
                capture_and_compare(
                    executable,
                    root / "guard.json",
                    root / "new-run",
                    working_directory=root,
                    timeout_seconds=1,
                )
            popen.assert_not_called()
            validate_guard.assert_called_once()

    @mock.patch("tools.shell_certification.orchestrator.validate_guard")
    @mock.patch("tools.child_process.subprocess.Popen")
    def test_existing_run_directory_rejects_before_launch(
        self, popen: mock.Mock, validate_guard: mock.Mock
    ) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            executable = root / "vera20k.exe"
            executable.write_bytes(b"fixture executable")
            run_dir = root / "already-exists"
            run_dir.mkdir()
            with self.assertRaises(OutputExistsError):
                capture_and_compare(
                    executable,
                    root / "guard.json",
                    run_dir,
                    working_directory=root,
                    timeout_seconds=1,
                )
            popen.assert_not_called()
            validate_guard.assert_called_once()

    @mock.patch("tools.shell_certification.orchestrator.build_comparison_report")
    @mock.patch("tools.shell_certification.orchestrator.validate_capture_bundle")
    @mock.patch("tools.shell_certification.orchestrator.validate_guard")
    @mock.patch("tools.child_process.subprocess.Popen")
    def test_timeout_kills_only_the_exact_child_object_and_retains_diagnostics(
        self,
        popen: mock.Mock,
        validate_guard: mock.Mock,
        validate_capture_bundle: mock.Mock,
        build_comparison_report: mock.Mock,
    ) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            executable = root / "vera20k.exe"
            executable.write_bytes(b"fixture executable")
            config = _write_config(root)
            run_dir = root / "new-run"

            child = mock.Mock()
            child.pid = 4321
            child.returncode = -9
            child.poll.return_value = None
            child.wait.side_effect = (
                subprocess.TimeoutExpired(cmd=["vera20k.exe"], timeout=1),
                -9,
            )
            popen.return_value = child
            validate_capture_bundle.side_effect = ValidationError(
                "capture manifest does not exist"
            )
            build_comparison_report.return_value = {
                "checkpoint": "main-menu-0xe2-steady",
                "status": INVALID,
                "errors": ["timed out"],
            }

            run_report, comparison = capture_and_compare(
                executable,
                root / "guard.json",
                run_dir,
                working_directory=root,
                timeout_seconds=1,
            )

            child.kill.assert_called_once_with()
            self.assertFalse(
                any(call[0] == "taskkill" for call in child.method_calls)
            )
            self.assertTrue(run_report["child"]["timed_out"])
            self.assertEqual(run_report["child"]["pid"], 4321)
            self.assertEqual(
                run_report["child"]["cleanup_scope"], "exact-child-pid-only"
            )
            self.assertEqual(
                popen.call_args.kwargs["cwd"], root
            )
            self.assertFalse(popen.call_args.kwargs["shell"])
            self.assertEqual(
                run_report["working_directories"]["child"], str(root)
            )
            self.assertEqual(run_report["config"]["path"], str(config))
            self.assertTrue(run_report["config"]["unchanged"])
            self.assertEqual(comparison["status"], INVALID)
            self.assertTrue((run_dir / "stdout.txt").is_file())
            self.assertTrue((run_dir / "stderr.txt").is_file())
            self.assertTrue((run_dir / "run.json").is_file())
            self.assertTrue((run_dir / "comparison.json").is_file())

    @mock.patch("tools.shell_certification.orchestrator.build_comparison_report")
    @mock.patch("tools.shell_certification.orchestrator.validate_capture_bundle")
    @mock.patch("tools.shell_certification.orchestrator.validate_guard")
    @mock.patch("tools.child_process.subprocess.Popen")
    def test_config_change_during_child_forces_invalid(
        self,
        popen: mock.Mock,
        validate_guard: mock.Mock,
        validate_capture_bundle: mock.Mock,
        build_comparison_report: mock.Mock,
    ) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            executable = root / "vera20k.exe"
            executable.write_bytes(b"fixture executable")
            config = _write_config(root, b"[video]\nmode='before'\n")
            run_dir = root / "new-run"

            child = mock.Mock()
            child.pid = 9876
            child.returncode = 0

            def wait_and_mutate_config(*, timeout: float) -> int:
                config.write_bytes(b"[video]\nmode='after'\n")
                return 0

            child.wait.side_effect = wait_and_mutate_config
            popen.return_value = child
            validate_capture_bundle.side_effect = ValidationError(
                "capture manifest does not exist"
            )
            build_comparison_report.return_value = {
                "checkpoint": "main-menu-0xe2-steady",
                "status": INVALID,
                "errors": ["config changed"],
            }

            run_report, _ = capture_and_compare(
                executable,
                root / "guard.json",
                run_dir,
                working_directory=root,
                timeout_seconds=1,
            )

            self.assertEqual(run_report["status"], INVALID)
            self.assertFalse(run_report["config"]["unchanged"])
            self.assertNotEqual(
                run_report["config"]["sha256"],
                run_report["config"]["post_run_sha256"],
            )
            self.assertTrue(
                any(
                    "config.toml changed" in error
                    for error in run_report["errors"]
                )
            )


if __name__ == "__main__":
    unittest.main()
