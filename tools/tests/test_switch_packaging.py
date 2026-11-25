"""Regression tests for Switch program-only packaging."""

from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

BUILD = Path(__file__).resolve().parents[2] / "platform" / "switch" / "build.py"


class SwitchPackagingTests(unittest.TestCase):
    def test_client_rejects_legacy_asset_bundling_option(self):
        with tempfile.TemporaryDirectory() as directory:
            result = subprocess.run(
                [sys.executable, str(BUILD), "--client", "--nsp-assets", "game"],
                cwd=directory,
                capture_output=True,
                text=True,
                check=False,
            )

        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("unrecognized arguments: --nsp-assets", result.stderr)
