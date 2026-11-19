"""tools/update_test.py's run, with stand-ins for publishing, serving and installing."""
import contextlib
import io
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

from tools import update_test


def publish(work, key, version, release):
    objects = work / "publish/objects"
    objects.mkdir(parents=True)
    (objects / "latest.json").write_text("{}\n")
    return objects


def install(work, release, program, game_folder, quarantine):
    # The Mac copy is made in the run's folder before it is signed.
    (work / "VERA20K.app").mkdir()
    raise subprocess.CalledProcessError(1, ["codesign"])


class UpdateTestRunTests(unittest.TestCase):
    def test_a_failed_run_removes_its_folder_but_not_the_game_folder(self):
        with tempfile.TemporaryDirectory() as base:
            base = Path(base)
            temp, game = base / "temp", base / "game"
            temp.mkdir()
            game.mkdir()
            (game / "ra2.mix").write_bytes(b"")
            argv = ["run", "--key", str(base / "signing-key.pem"), "--release", str(base / "release.dmg"),
                    "--version", "0.2.4", "--program", str(base / "vera20k"), "--game", str(game),
                    "--scenario", "update"]
            with (mock.patch.object(tempfile, "tempdir", str(temp)),
                  mock.patch.multiple(update_test, publish=publish, install=install, serve=mock.DEFAULT,
                                      remove_installed=mock.DEFAULT),
                  contextlib.redirect_stdout(io.StringIO()),
                  self.assertRaises(subprocess.CalledProcessError)):
                update_test.main(argv)
            self.assertEqual(list(temp.iterdir()), [])
            self.assertTrue((game / "ra2.mix").exists())

    def test_the_older_build_is_installed_from_its_own_release_and_updated_to_the_new(self):
        used = {}

        def publish_new(work, key, version, release):
            used["published"] = release
            return publish(work, key, version, release)

        def install_old(work, release, program, game_folder, quarantine):
            used["installed"] = release
            raise subprocess.CalledProcessError(1, ["setup"])

        with tempfile.TemporaryDirectory() as base:
            base = Path(base).resolve()
            new, old = base / "vera20k-0.2.5-macos-arm64.dmg", base / "vera20k-0.2.4-macos-arm64.dmg"
            argv = ["run", "--key", str(base / "signing-key.pem"), "--release", str(new), "--installed", str(old),
                    "--version", "0.2.5", "--program", str(base / "vera20k"), "--game", str(base),
                    "--scenario", "update"]
            with (mock.patch.multiple(update_test, publish=publish_new, install=install_old, serve=mock.DEFAULT,
                                      remove_installed=mock.DEFAULT),
                  contextlib.redirect_stdout(io.StringIO()),
                  self.assertRaises(subprocess.CalledProcessError)):
                update_test.main(argv)
        self.assertEqual(used, {"published": new, "installed": old})

    @unittest.skipUnless(sys.platform == "darwin", "only a macOS run lists the processes naming the app")
    def test_a_failed_run_lists_the_restart_helper_still_waiting_for_the_game(self):
        # Like the helper an update starts, it waits for the end of its
        # standard input before it would open the app.
        helper = subprocess.Popen([sys.executable, "-c", "import sys; sys.stdin.read()", "vera20k-restart",
                                   "", "/usr/bin/open", str(update_test.MAC_APP)], stdin=subprocess.PIPE)
        try:
            with tempfile.TemporaryDirectory() as work, contextlib.redirect_stdout(io.StringIO()) as output:
                update_test.report(Path(work), None)
        finally:
            helper.stdin.close()
            helper.wait(timeout=30)
        listing = output.getvalue().split("--- processes naming the app:\n", 1)[1].split("\n---")[0]
        self.assertIn(str(helper.pid), [row.split()[0] for row in listing.splitlines()[1:] if row])


if __name__ == "__main__":
    unittest.main()
