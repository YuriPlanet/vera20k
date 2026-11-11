"""Which released clients the relay compatibility check plays against, and how it cleans up."""
import contextlib
import io
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

from tools.relay_compat import (EXAMPLE, ROOT, BuildFailed, released_client, restore_example, supported_clients,
                                use_checkout_example)


def release(tag, created, prerelease=False, draft=False):
    return {"tagName": tag, "createdAt": created, "isPrerelease": prerelease, "isDraft": draft}


class SupportedClientsTest(unittest.TestCase):
    def test_latest_stable_and_newest_dev_build(self):
        rows = [
            release("v0.2.2", "2026-10-27T09:17:12Z"),
            release("v0.2.3", "2026-10-29T11:13:36Z"),
            release("v0.2.4", "2026-10-30T09:00:00Z", draft=True),
            release("v0.2.3-20261028-162610", "2026-10-28T16:52:56Z", prerelease=True),
            release("v0.2.3-20261029-101544", "2026-10-29T10:34:35Z", prerelease=True),
            release("v0.2.4-rc1", "2026-10-30T10:00:00Z", prerelease=True),
        ]
        self.assertEqual(supported_clients(rows), ["v0.2.3", "v0.2.3-20261029-101544"])


class ReleasedTreeTest(unittest.TestCase):
    def test_a_release_that_tracks_the_example_gets_its_own_back_and_its_tree_removes(self):
        with tempfile.TemporaryDirectory() as scratch:
            repo, tree = Path(scratch) / "repo", Path(scratch) / "tree"
            repo.mkdir()

            def git(*args):
                subprocess.run(["git", "-C", str(repo), "-c", "user.name=t", "-c", "user.email=t@t",
                                "-c", "commit.gpgsign=false", *args], check=True, capture_output=True)

            git("init", "-q")
            (repo / EXAMPLE).parent.mkdir(parents=True)
            (repo / EXAMPLE).write_text("// the release's own example\n")
            git("add", ".")
            git("commit", "-qm", "release")
            git("worktree", "add", "-q", "--detach", str(tree), "HEAD")
            original = use_checkout_example(tree)
            self.assertEqual((tree / EXAMPLE).read_bytes(), (ROOT / EXAMPLE).read_bytes())
            restore_example(tree, original)
            git("worktree", "remove", str(tree))
            self.assertFalse(tree.exists())

    def test_only_a_break_plays_the_releases_own_example_when_the_checkouts_does_not_build(self):
        checkout, own = "// the checkout's example\n", "// the release's own example\n"
        with tempfile.TemporaryDirectory() as scratch:
            tree = Path(scratch)
            (tree / EXAMPLE).parent.mkdir(parents=True)
            (tree / EXAMPLE).write_text(checkout)
            built = []

            def build(tree, package, target, output):
                built.append((tree / EXAMPLE).read_text())
                if built[-1] == checkout:
                    raise BuildFailed("no such variant")
                return output

            with mock.patch("tools.relay_compat.build", build), contextlib.redirect_stdout(io.StringIO()):
                with self.assertRaises(BuildFailed):
                    released_client(tree, own.encode(), tree / "client", allow_break=False)
                self.assertEqual(released_client(tree, own.encode(), tree / "client", allow_break=True),
                                 tree / "client")
            self.assertEqual(built, [checkout, checkout, own])


if __name__ == "__main__":
    unittest.main()
