"""Regression checks for fail-closed Release notarization."""
import importlib.util
import base64
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch


SOURCE = Path(__file__).resolve().parents[1] / ".github/release/macos/notarize.py"
SPEC = importlib.util.spec_from_file_location("macos_notarize", SOURCE)
notarize = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(notarize)
SUBMISSION = "11111111-2222-4333-8444-555555555555"


class MacNotarizationTests(unittest.TestCase):
    def test_missing_ticket_stops_verification_before_mount_or_execution(self):
        with tempfile.TemporaryDirectory() as temporary:
            image = Path(temporary) / "app.dmg"
            image.write_bytes(b"installer fixture")
            with patch.object(notarize, "command", side_effect=["", RuntimeError("missing ticket")]) as run:
                with self.assertRaisesRegex(RuntimeError, "missing ticket"):
                    notarize.verify_distribution(image, smoke_test=True)
            self.assertEqual(run.call_count, 2)
            self.assertEqual(run.call_args.args[0][:3], ["xcrun", "stapler", "validate"])
            self.assertEqual(image.read_bytes(), b"installer fixture")

    def test_promotion_verification_is_read_only_and_detaches_on_app_rejection(self):
        for reject_app in (False, True):
            with self.subTest(reject_app=reject_app), tempfile.TemporaryDirectory() as temporary:
                image = Path(temporary) / "app.dmg"
                image.write_bytes(b"installer fixture")
                calls = []

                def run(args):
                    calls.append(args)
                    if args[:2] == ["hdiutil", "attach"]:
                        program = Path(args[-2]) / "VERA20K.app/Contents/MacOS/VERA20K"
                        program.parent.mkdir(parents=True)
                        program.touch()
                    if args[:4] == ["spctl", "--assess", "--type", "execute"] and reject_app:
                        raise RuntimeError("app rejected")
                    self.assertIn(args[0], {"codesign", "xcrun", "spctl", "hdiutil"},
                                  "The app must not execute before validation or during promotion")
                    return ""

                with patch.dict(os.environ, {}, clear=True), \
                     patch.object(notarize.ctypes, "CDLL") as library, \
                     patch.object(notarize, "command", side_effect=run):
                    if reject_app:
                        with self.assertRaisesRegex(RuntimeError, "app rejected"):
                            notarize.verify_distribution(image, smoke_test=True)
                    else:
                        notarize.verify_distribution(image)
                    library.assert_not_called()
                self.assertEqual(calls[-1][:2], ["hdiutil", "detach"])
                self.assertEqual(image.read_bytes(), b"installer fixture")

    def test_credentials_are_required_and_cleaned_up_after_failure(self):
        with tempfile.TemporaryDirectory() as temporary, patch.dict(os.environ, {}, clear=True):
            root = Path(temporary)
            with self.assertRaisesRegex(RuntimeError, "missing signing credentials"):
                with notarize.signing_keychain(root):
                    self.fail("Missing credentials must not yield a signing keychain")
            self.assertEqual(list(root.iterdir()), [])
        security, core = Mock(), Mock()
        security.SecKeychainOpen.side_effect = lambda path, out: setattr(out._obj, "value", 123) or 0
        security.SecKeychainUnlock.return_value = 0
        fixture = {"MACOS_SIGNING_KEYCHAIN": base64.b64encode(b"encrypted fixture").decode(),
                   "MACOS_SIGNING_KEYCHAIN_PASSWORD": "fixture-password"}
        with tempfile.TemporaryDirectory() as temporary, patch.dict(os.environ, fixture, clear=True):
            root = Path(temporary)
            with patch.object(notarize.ctypes, "CDLL", side_effect=[security, core]), \
                 patch.object(notarize, "command", side_effect=["", '"login.keychain-db"', "", ""]) as command, \
                 patch.object(notarize.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)) as run:
                with self.assertRaisesRegex(RuntimeError, "signing failed"):
                    with notarize.signing_keychain(root) as keychain:
                        self.assertEqual(keychain.stat().st_mode & 0o777, 0o600)
                        self.assertTrue(all(name not in os.environ for name in fixture))
                        raise RuntimeError("signing failed")
                self.assertEqual(command.call_args.args[0],
                                 ["security", "list-keychains", "-d", "user", "-s", "login.keychain-db"])
                security.SecKeychainLock.assert_called_once()
                core.CFRelease.assert_called_once()
                self.assertNotIn(fixture["MACOS_SIGNING_KEYCHAIN_PASSWORD"], run.call_args.args[0])
                self.assertEqual(run.call_args.kwargs["input"], b"fixture-password\n")
                self.assertTrue(run.call_args.kwargs["start_new_session"])
                self.assertEqual(list(root.iterdir()), [])

    def test_acceptance_preserves_receipt_and_waits_on_same_submission(self):
        replies = [subprocess.CompletedProcess([], 0, json.dumps({"id": SUBMISSION}), ""),
                   subprocess.CompletedProcess([], 0, json.dumps({"id": SUBMISSION, "status": "Accepted"}), ""),
                   subprocess.CompletedProcess([], 0, "", "")]
        with tempfile.TemporaryDirectory() as temporary, patch.object(notarize.subprocess, "run", side_effect=replies) as run:
            root = Path(temporary)
            result = notarize.submit_and_wait(root / "app.dmg", root / "signing.keychain-db", root, "30m")
            self.assertEqual(result, SUBMISSION)
            self.assertEqual(json.loads((root / "submission.json").read_text())["id"], SUBMISSION)
            self.assertEqual(json.loads((root / "status.json").read_text())["status"], "Accepted")
            self.assertEqual(run.call_args_list[1].args[0][2:4], ["wait", SUBMISSION])

    def test_timeout_and_rejection_preserve_id_without_resubmitting(self):
        for code, stdout, stderr in [
            (124, "", json.dumps({"id": SUBMISSION, "message": "Timeout reached"})),
            (0, json.dumps({"id": SUBMISSION, "status": "Invalid"}), ""),
        ]:
            with self.subTest(code=code, stdout=stdout), tempfile.TemporaryDirectory() as temporary:
                replies = [subprocess.CompletedProcess([], 0, json.dumps({"id": SUBMISSION}), ""),
                           subprocess.CompletedProcess([], code, stdout, stderr)]
                if stdout:
                    replies.append(subprocess.CompletedProcess([], 0, "", ""))
                with patch.object(notarize.subprocess, "run", side_effect=replies) as run:
                    root = Path(temporary)
                    with self.assertRaisesRegex(RuntimeError, "was not accepted"):
                        notarize.submit_and_wait(root / "app.dmg", root / "signing.keychain-db", root, "30m")
                    self.assertEqual(json.loads((root / "status.json").read_text())["id"], SUBMISSION)
                    self.assertEqual(sum(call.args[0][2] == "submit" for call in run.call_args_list), 1)


if __name__ == "__main__":
    unittest.main()
