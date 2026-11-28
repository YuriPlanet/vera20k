"""Regression checks for the version and certificate accepted at APK promotion."""

import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SOURCE = Path(__file__).resolve().parents[1] / ".github/release/android/apk.py"
SPEC = importlib.util.spec_from_file_location("release_apk", SOURCE)
apk = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(apk)


class AndroidReleaseTests(unittest.TestCase):
    def test_version_code_is_the_same_at_promotion_and_increases_across_midnight(self):
        self.assertEqual(apk.version_code("20261128-000000"), 217_987_200)
        self.assertEqual(apk.version_code("20261127-235959"), 217_987_199)
        self.assertEqual(apk.version_code("20860718-132000"), 2_100_000_000)
        for stamp in ("20200101-000000", "20860718-132001", "20261128-00000"):
            with self.subTest(stamp=stamp), self.assertRaises(ValueError):
                apk.version_code(stamp)

    def test_the_trusted_certificate_must_be_the_only_signer(self):
        trusted, other = "a1" * 32, "b2" * 32
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            pin = directory / "certificate.sha256"
            pin.write_text(trusted + "\n", encoding="ascii")
            with patch.object(apk, "CERTIFICATE", pin), patch.object(apk.subprocess, "check_output") as verify:
                verify.return_value = f"Number of signers: 1\nSigner #1 certificate SHA-256 digest: {trusted}\n"
                apk.check_signer(directory, Path("release.apk"))
                for certificates in ([other], [trusted, other]):
                    verify.return_value = f"Number of signers: {len(certificates)}\n" + "".join(
                        f"Signer #{i} certificate SHA-256 digest: {value}\n"
                        for i, value in enumerate(certificates, 1))
                    with self.subTest(certificates=certificates), self.assertRaisesRegex(ValueError, "one signer"):
                        apk.check_signer(directory, Path("release.apk"))
