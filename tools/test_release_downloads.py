"""Exercise the real Bash publisher against local GitHub/S3 CLI fixtures."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from tools.release_fixture import Publisher, signing_key, verify_signature

# The updater-version file in the release's commit.
UPDATER = 3


class ReleaseDownloadsTests(unittest.TestCase):
    def setUp(self):
        if not shutil.which("jq"):
            self.skipTest("publisher requires jq")
        self.temp = tempfile.TemporaryDirectory(prefix="release-downloads-fixture-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        # The real script, beside a public key for this fixture's signing key.
        try:
            self.signing_key, self.public_key = signing_key(self.root)
        except (OSError, RuntimeError, subprocess.CalledProcessError):
            self.skipTest("publisher requires OpenSSL with Ed25519")
        self.publisher = Publisher(self.root, self.public_key)
        self.names = ["vera20k-0.2.2-windows-x64-setup.exe", "vera20k-0.2.2-macos-arm64.dmg", "vera20k-0.2.2-linux-x64.AppImage"]
        assets = []
        for index, name in enumerate(self.names):
            data = (name + "\n").encode()
            (self.root / "assets" / name).write_bytes(data)
            assets.append({"id": index + 1, "name": name, "size": len(data), "digest": "sha256:" + hashlib.sha256(data).hexdigest(), "state": "uploaded"})
        self.config = {"tag": "v0.2.2", "commit": "a" * 40, "updater_version": f"{UPDATER}\n",
                       "release": {"id": 10, "tag_name": "v0.2.2", "draft": False, "prerelease": False, "assets": assets}}

    def run_publisher(self, tag="v0.2.2", signing_key=None, roll_back=False):
        return self.publisher.run(tag, self.config, self.signing_key if signing_key is None else signing_key,
                                  *(["--roll-back"] if roll_back else []))

    def calls(self):
        path = self.root / "calls.jsonl"
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def puts(self):
        return [args[args.index("--key") + 1] for tool, args in self.calls() if tool == "aws" and "put-object" in args]

    def prior_pointer(self, version="0.2.1", platforms=("windows", "macos", "linux")):
        data = {"version": version, "tag": "v" + version, "commit": "c" * 40,
                "files": {platform: {} for platform in platforms}}
        (self.root / "objects/latest.json").write_text(json.dumps(data))
        return (self.root / "objects/latest.json").read_bytes()

    def test_publication_and_identical_retry_verify_all_bytes(self):
        result = self.run_publisher()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.puts()), 6)
        manifest = self.root / "objects/releases/v0.2.2/manifest.json"
        self.assertEqual(manifest.read_bytes(), (self.root / "objects/latest.json").read_bytes())
        signature = (self.root / "objects/releases/v0.2.2/manifest.json.sig").read_text()
        self.assertTrue(verify_signature(self.public_key, manifest.read_bytes(), signature, self.root))
        self.assertFalse(verify_signature(self.public_key, manifest.read_bytes() + b" ", signature, self.root))
        data = json.loads(manifest.read_text())
        self.assertEqual(set(data["files"]), {"windows", "macos", "linux"})
        # What an older game needs to offer a release it cannot install.
        self.assertEqual((data["updater"], data["update_key"]), (UPDATER, self.public_key))
        for entry in data["files"].values():
            contents = (self.root / "objects" / entry["key"]).read_bytes()
            self.assertEqual(len(contents), entry["size"])
            self.assertEqual(hashlib.sha256(contents).hexdigest(), entry["sha256"])
        self.assertEqual(sum(args[:2] == ["attestation", "verify"] for tool, args in self.calls() if tool == "gh"), 3)
        for tool, args in self.calls():
            if tool != "aws" or "put-object" not in args:
                continue
            key = args[args.index("--key") + 1]
            if key == "latest.json":
                self.assertEqual(args[args.index("--cache-control") + 1], "no-cache")
            elif key.endswith(("/manifest.json", "/manifest.json.sig")):
                self.assertEqual(args[args.index("--content-disposition") + 1], "inline")
            else:
                self.assertEqual(args[args.index("--content-disposition") + 1],
                                 'attachment; filename="' + key.rsplit("/", 1)[1] + '"')
        again = self.run_publisher()
        self.assertEqual(again.returncode, 0, again.stderr)
        self.assertEqual(len(self.puts()), 6, "an identical retry must not overwrite any object")

    def test_a_release_with_an_intel_mac_image_publishes_it_for_intel_macs(self):
        name = "vera20k-0.2.2-macos-x64.dmg"
        data = (name + "\n").encode()
        (self.root / "assets" / name).write_bytes(data)
        self.config["release"]["assets"].append({"id": 4, "name": name, "size": len(data),
                                                 "digest": "sha256:" + hashlib.sha256(data).hexdigest(),
                                                 "state": "uploaded"})
        self.prior_pointer()
        result = self.run_publisher()
        self.assertEqual(result.returncode, 0, result.stderr)
        files = json.loads((self.root / "objects/latest.json").read_text())["files"]
        self.assertEqual(set(files), {"windows", "macos", "macos-x64", "linux"})
        self.assertEqual(files["macos-x64"]["name"], name)
        self.assertEqual((self.root / "objects" / files["macos-x64"]["key"]).read_bytes(), data)

    def test_signed_manifest_lists_the_release_pages_changes(self):
        # As .github/release/notes.sh writes it, saved from GitHub's editor.
        body = ("### Changes since v0.2.1\n\n- Play online through Internet → Custom Match.\n"
                "- **Faster** [LAN](https://example.com/lan) games with `-fast`.\n- [and 3 more](https://example.com)\n\n"
                "[All changes](https://example.com/compare)\n\nYou need your own copy of Yuri's Revenge.\n\n"
                "- **Windows:** run the `-setup.exe`.\n")
        for text, notes in [(body.replace("\n", "\r\n"), ["Play online through Internet → Custom Match.",
                                                          "Faster LAN games with -fast."]),
                            ("Free software under the GNU GPL.\n\n- **Windows:** run it.", [])]:
            with self.subTest(notes=notes):
                self.config["release"]["body"] = text
                shutil.rmtree(self.root / "objects")
                (self.root / "objects").mkdir()
                result = self.run_publisher()
                self.assertEqual(result.returncode, 0, result.stderr)
                manifest = (self.root / "objects/releases/v0.2.2/manifest.json").read_bytes()
                self.assertEqual(json.loads(manifest)["notes"], notes)
                signature = (self.root / "objects/releases/v0.2.2/manifest.json.sig").read_text()
                self.assertTrue(verify_signature(self.public_key, manifest, signature, self.root))

    def test_a_retry_keeps_the_fields_first_published(self):
        manifest = self.root / "objects/releases/v0.2.2/manifest.json"
        self.config["release"]["body"] = "### Changes\n\n- First.\n"
        self.assertEqual(self.run_publisher().returncode, 0)
        self.config["release"]["body"] = "### Changes\n\n- Edited after publication.\n"
        self.config["updater_version"] = f"{UPDATER + 1}\n"
        result = self.run_publisher()
        self.assertEqual(result.returncode, 0, result.stderr)
        data = json.loads(manifest.read_bytes())
        self.assertEqual((data["notes"], data["updater"]), (["First."], UPDATER))
        # As R2 holds the releases mirrored before manifests were signed.
        stored = subprocess.run(["jq", "-S", "del(.notes, .updater, .update_key)", str(manifest)],
                                capture_output=True, check=True).stdout
        for path in (manifest, self.root / "objects/latest.json"):
            path.write_bytes(stored)
        (self.root / "objects/releases/v0.2.2/manifest.json.sig").unlink()
        result = self.run_publisher()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(manifest.read_bytes(), stored)
        signature = (self.root / "objects/releases/v0.2.2/manifest.json.sig").read_text()
        self.assertTrue(verify_signature(self.public_key, stored, signature, self.root))

    def test_a_release_from_before_updater_versions_names_none(self):
        del self.config["updater_version"]
        result = self.run_publisher()
        self.assertEqual(result.returncode, 0, result.stderr)
        data = json.loads((self.root / "objects/releases/v0.2.2/manifest.json").read_text())
        self.assertNotIn("updater", data)
        self.assertEqual(data["update_key"], self.public_key)

    def test_signing_key_must_match_the_trusted_public_key(self):
        old_pointer = self.prior_pointer()
        self.assertNotEqual(self.run_publisher(signing_key="").returncode, 0)
        self.assertEqual(self.calls(), [], "a missing key stops before any remote call")
        other = Path(self.root / "other")
        other.mkdir()
        result = self.run_publisher(signing_key=signing_key(other)[0])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not match update-key.pub", result.stderr)
        self.assertEqual(self.puts(), [])
        self.assertEqual((self.root / "objects/latest.json").read_bytes(), old_pointer)

    def test_release_shape_and_provenance_fail_before_storage_writes(self):
        for failure in ("draft", "extra", "digest", "attestation"):
            with self.subTest(failure=failure):
                original = json.loads(json.dumps(self.config))
                if failure == "draft":
                    self.config["release"]["draft"] = True
                elif failure == "extra":
                    self.config["release"]["assets"].append(dict(self.config["release"]["assets"][0], name="unexpected.zip"))
                elif failure == "digest":
                    self.config["tamper_name"] = self.names[-1]
                else:
                    self.config["attestation_fail"] = True
                result = self.run_publisher()
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.puts(), [])
                self.config = original

    def test_existing_different_immutable_object_is_never_overwritten(self):
        old_pointer = self.prior_pointer()
        path = self.root / "objects/releases/v0.2.2" / self.names[0]
        path.parent.mkdir(parents=True)
        path.write_bytes(b"different existing release")
        result = self.run_publisher()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(path.read_bytes(), b"different existing release")
        self.assertEqual((self.root / "objects/latest.json").read_bytes(), old_pointer)
        self.assertEqual(self.puts(), [])

    def test_a_locked_object_from_a_racing_publisher_is_verified(self):
        self.config["raced_put"] = "releases/v0.2.2/" + self.names[0]
        result = self.run_publisher()
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = self.root / "objects/releases/v0.2.2/manifest.json"
        self.assertEqual(manifest.read_bytes(), (self.root / "objects/latest.json").read_bytes())

    def test_partial_or_corrupt_upload_never_publishes_a_manifest_pointer(self):
        for failure in ("fail_put", "corrupt_put"):
            with self.subTest(failure=failure):
                old_pointer = self.prior_pointer()
                self.config[failure] = "releases/v0.2.2/" + self.names[1]
                result = self.run_publisher()
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual((self.root / "objects/latest.json").read_bytes(), old_pointer)
                self.assertNotIn("latest.json", self.puts())
                self.assertFalse((self.root / "objects/releases/v0.2.2/manifest.json").exists())
                self.config.pop(failure)

    def test_nonlatest_or_older_release_keeps_existing_pointer(self):
        for latest, previous in (("v0.2.3", "0.2.1"), ("v0.2.2", "0.2.3")):
            with self.subTest(latest=latest, previous=previous):
                self.config["latest_tag"] = latest
                old_pointer = self.prior_pointer(previous)
                result = self.run_publisher()
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual((self.root / "objects/latest.json").read_bytes(), old_pointer)
                self.assertNotIn("latest.json", self.puts())

    def test_roll_back_moves_the_pointer_back_only_to_githubs_latest_release(self):
        # Back from a release with an Intel Mac image to one from before them.
        newer = self.prior_pointer("0.2.3", ("windows", "macos", "macos-x64", "linux"))
        self.config["latest_tag"] = "v0.2.3"
        result = self.run_publisher(roll_back=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("gh release edit v0.2.2 --latest", result.stderr)
        self.assertEqual((self.root / "objects/latest.json").read_bytes(), newer)
        self.config["latest_tag"] = "v0.2.2"
        result = self.run_publisher(roll_back=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = self.root / "objects/releases/v0.2.2/manifest.json"
        self.assertEqual((self.root / "objects/latest.json").read_bytes(), manifest.read_bytes())

    def test_failed_pointer_update_preserves_previous_pointer(self):
        old_pointer = self.prior_pointer()
        self.config["fail_put"] = "latest.json"
        result = self.run_publisher()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((self.root / "objects/latest.json").read_bytes(), old_pointer)
        self.assertTrue((self.root / "objects/releases/v0.2.2/manifest.json").exists())

    def test_concurrent_pointer_is_not_overwritten(self):
        self.prior_pointer()
        competitor = {"version": "0.2.3", "tag": "v0.2.3"}
        self.config["competing_latest"] = competitor
        result = self.run_publisher()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(json.loads((self.root / "objects/latest.json").read_text()), competitor)

    def test_untrusted_arguments_and_existing_local_lock_stop_before_remote_calls(self):
        for tag in ("v0.2.2;echo nope", "v0.2.2-dev", "v00.2.2", "../latest"):
            with self.subTest(tag=tag):
                self.assertNotEqual(self.run_publisher(tag).returncode, 0)
                self.assertEqual(self.calls(), [])
        self.assertNotEqual(self.publisher.run("v0.2.2", self.config, self.signing_key, "--rollback").returncode, 0)
        self.assertEqual(self.calls(), [])
        (self.root / "publish.lock").mkdir()
        self.assertNotEqual(self.run_publisher().returncode, 0)
        self.assertEqual(self.calls(), [])
        self.assertTrue((self.root / "publish.lock").is_dir())


if __name__ == "__main__":
    unittest.main()
