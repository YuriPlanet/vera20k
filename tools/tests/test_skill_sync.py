"""Check mirroring behavior and preservation of local-only content."""

from pathlib import Path
import tempfile
import unittest

from tools.skill_sync import synchronize


class SkillSyncTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.source = self.root / "source"
        self.destination = self.root / "destination"
        self.put(self.source / "example/SKILL.md", b"canonical\n")

    @staticmethod
    def put(path, content):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)

    def test_check_does_not_write_then_copy_is_idempotent(self):
        self.assertEqual(synchronize(self.source, self.destination, write=False),
                         (0, ["Missing: example/SKILL.md"]))
        self.assertFalse(self.destination.exists())
        self.assertEqual(synchronize(self.source, self.destination, write=True), (1, []))
        self.assertEqual(synchronize(self.source, self.destination, write=True), (0, []))
        self.put(self.source / "example/SKILL.md", b"revised\n")
        self.assertEqual(synchronize(self.source, self.destination, write=False),
                         (0, ["Different: example/SKILL.md"]))
        self.assertEqual((self.destination / "example/SKILL.md").read_bytes(), b"canonical\n")
        self.assertEqual(synchronize(self.source, self.destination, write=True), (1, []))

    def test_local_skill_and_host_metadata_are_preserved(self):
        self.put(self.source / "ghidra-up/SKILL.md", b"source local")
        self.put(self.source / "example/agents/openai.yaml", b"codex metadata")
        self.put(self.source / "example/__pycache__/x.pyc", b"cache")
        self.put(self.destination / "ghidra-up/SKILL.md", b"destination local")
        self.assertEqual(synchronize(self.source, self.destination, write=True), (1, []))
        self.assertEqual((self.destination / "ghidra-up/SKILL.md").read_bytes(), b"destination local")
        self.assertFalse((self.destination / "example/agents").exists())
        self.assertFalse((self.destination / "example/__pycache__").exists())

    def test_destination_only_content_blocks_writes_and_is_not_deleted(self):
        self.put(self.destination / "old/notes.md", b"unique")
        count, issues = synchronize(self.source, self.destination, write=True)
        self.assertEqual(count, 0)
        self.assertEqual(issues, ["Destination-only file (review manually): old/notes.md"])
        self.assertEqual((self.destination / "old/notes.md").read_bytes(), b"unique")
        self.assertFalse((self.destination / "example/SKILL.md").exists())

    def test_missing_source_is_an_error(self):
        with self.assertRaises(ValueError):
            synchronize(self.root / "absent", self.destination, write=True)

    def test_redirected_destination_is_rejected(self):
        outside = self.root / "outside"
        outside.mkdir()
        try:
            self.destination.symlink_to(outside, target_is_directory=True)
        except OSError:
            self.skipTest("Creating symlinks requires OS permission")
        with self.assertRaises(ValueError):
            synchronize(self.source, self.destination, write=True)
        self.assertEqual(list(outside.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
