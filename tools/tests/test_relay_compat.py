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
    @mock.patch("tools.relay_compat.OLDEST_ADMITTED", "0.2.7")
    def test_every_release_from_the_floor_and_the_newest_dev_build(self):
        rows = [
            release("v0.2.6", "2026-11-04T22:06:40Z"),
            release("v0.2.9", "2026-11-09T17:02:54Z"),
            release("v0.2.7", "2026-11-07T20:15:53Z"),
            release("v0.2.8", "2026-11-09T01:42:38Z"),
            release("v0.3.0", "2026-11-10T09:00:00Z", draft=True),
            release("v0.2.8-20261108-232628", "2026-11-09T01:14:31Z", prerelease=True),
            release("v0.2.9-20261109-144127", "2026-11-09T16:45:25Z", prerelease=True),
            release("v0.3.0-rc1", "2026-11-10T10:00:00Z", prerelease=True),
        ]
        self.assertEqual(supported_clients(rows), ["v0.2.7", "v0.2.8", "v0.2.9", "v0.2.9-20261109-144127"])

    def test_a_list_that_stops_above_the_floor_fails_and_a_floor_above_every_build_plays_none(self):
        rows = [release("v0.2.8", "2026-11-09T01:42:38Z"),
                release("v0.2.9-20261109-144127", "2026-11-09T16:45:25Z", prerelease=True)]
        with mock.patch("tools.relay_compat.OLDEST_ADMITTED", "0.2.7"), self.assertRaises(SystemExit):
            supported_clients(rows)
        with mock.patch("tools.relay_compat.OLDEST_ADMITTED", "0.3.0"), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(supported_clients(rows), [])


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

    def test_a_release_the_checkouts_example_does_not_build_against_plays_its_own(self):
        checkout, own = "// the checkout's example\n", "// the release's own example\n"
        with tempfile.TemporaryDirectory() as scratch:
            tree = Path(scratch)
            (tree / EXAMPLE).parent.mkdir(parents=True)
            built = []

            def build(tree, package, target, output):
                built.append((tree / EXAMPLE).read_text())
                if built[-1] == checkout:
                    raise BuildFailed("no such variant")
                return output

            with mock.patch("tools.relay_compat.build", build), contextlib.redirect_stdout(io.StringIO()):
                (tree / EXAMPLE).write_text(checkout)
                self.assertEqual(released_client(tree, own.encode(), tree / "client"), tree / "client")
                (tree / EXAMPLE).write_text(checkout)
                with self.assertRaises(BuildFailed):
                    released_client(tree, None, tree / "client")
            self.assertEqual(built, [checkout, own, checkout])


if __name__ == "__main__":
    unittest.main()
