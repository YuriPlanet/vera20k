"""Focused checks for staged Windows signing and strict certificate validation."""

import copy
import importlib.util
import json
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest
from unittest.mock import patch


SOURCE = Path(__file__).resolve().parents[1] / ".github/release/windows/signing.py"
SPEC = importlib.util.spec_from_file_location("windows_signing", SOURCE)
signing = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(signing)
FINGERPRINT = "AB" * 32


def pe_fixture():
    """A non-executable PE header with payload bytes for content-change checks."""
    data = bytearray(513)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 60, 64)
    data[64:68] = b"PE\0\0"
    struct.pack_into("<H", data, 84, 224)
    struct.pack_into("<H", data, 88, 0x10B)
    struct.pack_into("<I", data, 180, 16)
    data[500:513] = b"test-payload!"
    return bytes(data)


def signed_fixture(data):
    """Model only Authenticode's permitted header/table changes, never executed."""
    data = bytearray(data)
    data.extend(b"\0" * (-len(data) % 8))
    struct.pack_into("<I", data, 152, 1234)  # optional-header checksum
    struct.pack_into("<II", data, 216, len(data), 16)  # security directory
    data.extend(b"certificate-data")
    return bytes(data)


def waiting_result(directory, data):
    candidate = directory / f"uninst-7.1.0-{signing.digest(data)[:10]}.e32"
    candidate.write_bytes(data)
    message = ("Signed uninstaller mode is enabled. Using an external code-signing tool, "
               "please attach your digital signature to the following executable file:\r\n\r\n"
               f"{candidate}\r\n\r\nand compile again.")
    error = "\n".join(json.dumps({"severity": "error", "message": text})
                      for text in (message, "Compile aborted."))
    return candidate, subprocess.CompletedProcess([], 2, b"", error.encode())


class WindowsSigningTests(unittest.TestCase):
    def test_only_the_exact_signature_needed_failure_is_accepted(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            candidate, result = waiting_result(directory, pe_fixture())
            self.assertEqual(signing.generated_uninstaller(result, directory), candidate)
            # Inno's GetExceptMessage adds this period to the raw message constant.
            result.stderr = result.stderr.replace(b"and compile again.", b"and compile again")
            with self.assertRaisesRegex(RuntimeError, "other than the expected") as caught:
                signing.generated_uninstaller(result, directory)
            self.assertIn('"returncode": 2', str(caught.exception))
            self.assertIn(r"\n\n", str(caught.exception))
            self.assertIn(candidate.name, str(caught.exception))
            # A compiler error must not be mistaken for a successful prepare just
            # because it left an uninstaller file behind.
            result.stderr = json.dumps({"severity": "error", "message": "Out of memory"}).encode()
            with self.assertRaisesRegex(RuntimeError, "other than the expected"):
                signing.generated_uninstaller(result, directory)

    def test_signed_payload_reuses_cache_name_and_rejects_changed_program(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            package = root / "package"
            package.mkdir()
            for filename in signing.PACKAGE_FILES:
                (package / filename).write_bytes(pe_fixture() if filename.endswith(".exe") else b"text")
            staging = root / "staging"

            def prepare_compile(iscc, package, output, version, build, name, cache):
                return waiting_result(cache, pe_fixture())[1]

            with patch.object(signing, "compile_setup", side_effect=prepare_compile):
                signing.prepare(package, staging, "0.2.5", "20261102-120000", "vera20k-check", "ISCC")
            self.assertEqual(signing.metadata(staging), {"version": "0.2.5", "name": "vera20k-check"})
            manifest = json.loads((staging / "manifest.json").read_text())
            original_bytes = {path.name: path.read_bytes() for path in staging.iterdir()}
            signed = root / "signed"
            shutil.copytree(staging, signed)
            for filename in ("vera20k.exe", "uninstaller.exe"):
                (signed / filename).write_bytes(signed_fixture((signed / filename).read_bytes()))

            def package_compile(iscc, package, output, version, build, name, cache):
                self.assertEqual([p.name for p in cache.iterdir()], [manifest["uninstaller_name"]])
                self.assertEqual((cache / manifest["uninstaller_name"]).read_bytes(),
                                 (signed / "uninstaller.exe").read_bytes())
                self.assertEqual((package / "vera20k.exe").read_bytes(),
                                 (signed / "vera20k.exe").read_bytes())
                output.mkdir()
                (output / f"{name}-setup.exe").write_bytes(b"setup")
                return subprocess.CompletedProcess([], 0, b"", b"")

            with patch.object(signing, "verify") as verify, \
                 patch.object(signing, "compile_setup", side_effect=package_compile):
                image = signing.package_signed(signed, staging, root / "output", "ISCC", FINGERPRINT)
                self.assertEqual(image.name, "vera20k-check-setup.exe")
                verify.assert_called_once()
            changed = bytearray((signed / "vera20k.exe").read_bytes())
            changed[500] ^= 1
            (signed / "vera20k.exe").write_bytes(changed)
            with patch.object(signing, "compile_setup") as compiler, \
                 patch.object(signing, "verify") as verify:
                with self.assertRaisesRegex(RuntimeError, "changed the package contents: vera20k.exe"):
                    signing.package_signed(signed, staging, root / "rejected", "ISCC", FINGERPRINT)
                # Updating the returned manifest must not bless an altered PE.
                manifest["files"]["vera20k.exe"]["content_sha256"] = signing.pe_content_digest(
                    changed, manifest["files"]["vera20k.exe"]["size"])
                (signed / "manifest.json").write_text(json.dumps(manifest))
                with self.assertRaisesRegex(RuntimeError, "changed the signing payload manifest"):
                    signing.package_signed(signed, staging, root / "rejected", "ISCC", FINGERPRINT)
                compiler.assert_not_called()
                verify.assert_not_called()
            self.assertEqual({path.name: path.read_bytes() for path in staging.iterdir()}, original_bytes)

    def test_windows_trust_and_exact_signer_are_both_required(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            original = root / "unsigned" / "vera20k-check-setup.exe"
            original.parent.mkdir()
            original.write_bytes(pe_fixture())
            image = root / "signed" / original.name
            image.parent.mkdir()
            image.write_bytes(signed_fixture(original.read_bytes()))
            report = {"path": str(image.resolve()), "status": "Valid",
                      "type": "Authenticode", "fingerprint": FINGERPRINT}
            with patch.object(signing, "signature_info", return_value=[report]):
                signing.verify([image], FINGERPRINT)
                self.assertEqual(signing.verify_installer(original, image.parent, FINGERPRINT), image.resolve())
                report["status"] = "NotTrusted"
                with self.assertRaisesRegex(RuntimeError, "untrusted, invalid"):
                    signing.verify([image], FINGERPRINT)
                report["status"] = "Valid"
                report["fingerprint"] = "CD" * 32
                with self.assertRaisesRegex(RuntimeError, "another certificate"):
                    signing.verify([image], FINGERPRINT)
            with patch.object(signing, "signature_info") as verifier:
                changed = bytearray(image.read_bytes())
                changed[500] ^= 1
                image.write_bytes(changed)
                with self.assertRaisesRegex(RuntimeError, "changed the installer contents"):
                    signing.verify_installer(original, image.parent, FINGERPRINT)
                image.write_bytes(signed_fixture(original.read_bytes()))
                (image.parent / "unexpected.exe").write_bytes(image.read_bytes())
                with self.assertRaisesRegex(RuntimeError, "Unexpected files"):
                    signing.verify_installer(original, image.parent, FINGERPRINT)
                verifier.assert_not_called()

    def test_reuse_requires_successful_main_build_and_ancestor_artifact(self):
        source, current = "a" * 40, "b" * 40
        run = {"id": 123, "status": "completed", "event": "workflow_dispatch", "head_branch": "main",
               "head_sha": source, "path": ".github/workflows/windows-signing-check.yml",
               "repository": {"id": 45, "full_name": "YuriPlanet/vera20k"},
               "head_repository": {"id": 45, "full_name": "YuriPlanet/vera20k"}}
        jobs = {"total_count": 1, "jobs": [{"run_id": 123, "name": "build",
            "status": "completed", "conclusion": "success", "steps": [
                {"name": "Build Windows game and notices", "status": "completed", "conclusion": "success"},
                {"name": "Prepare the game and uninstaller for signing", "status": "completed",
                 "conclusion": "success"}]}]}
        artifacts = {"total_count": 1, "artifacts": [{"id": 678, "expired": False,
            "name": "windows-signing-payload-unsigned", "workflow_run": {"id": 123,
            "head_branch": "main", "head_sha": source, "repository_id": 45, "head_repository_id": 45}}]}
        with patch.object(signing, "github_json", side_effect=[run, {"status": "ahead"}, jobs, artifacts]):
            self.assertEqual(signing.reuse_info("123", current), {"artifact_id": "678", "source_sha": source})
        wrong_branch = dict(run, head_branch="feature/elsewhere")
        failed_build = copy.deepcopy(jobs)
        failed_build["jobs"][0]["conclusion"] = "failure"
        skipped_build = copy.deepcopy(jobs)
        skipped_build["jobs"][0]["steps"][0]["conclusion"] = "skipped"
        changed_artifact = copy.deepcopy(artifacts)
        changed_artifact["artifacts"][0]["workflow_run"]["head_sha"] = current
        for replies, reason in [([wrong_branch], "main workflow"),
                                ([run, {"status": "diverged"}], "ancestor"),
                                ([run, {"status": "ahead"}, failed_build], "successful build"),
                                ([run, {"status": "ahead"}, skipped_build], "compilation and preparation"),
                                ([run, {"status": "ahead"}, jobs, changed_artifact], "artifact provenance")]:
            with self.subTest(reason=reason), patch.object(signing, "github_json", side_effect=replies):
                with self.assertRaisesRegex(RuntimeError, reason):
                    signing.reuse_info("123", current)
        failed = subprocess.CompletedProcess([], 1, b"private CLI diagnostic", b"private CLI diagnostic")
        with patch.object(signing.subprocess, "run", return_value=failed):
            with self.assertRaises(RuntimeError) as caught:
                signing.github_json("repos/YuriPlanet/vera20k/actions/runs/123")
            self.assertNotIn("private CLI diagnostic", str(caught.exception))


if __name__ == "__main__":
    unittest.main()
