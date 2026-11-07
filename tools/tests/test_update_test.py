"""The update test: its stand-in config.toml lasts only for its run, its key
survives Git Bash, and a program that hangs fails it."""
import os
import sys
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from tools import update_test


@unittest.skipIf(update_test.platform() == "windows", "the Windows installer writes its own config.toml")
class FirstLaunchConfigTest(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        home = Path(scratch.name)
        environment = mock.patch.dict(os.environ, {"HOME": str(home), "XDG_DATA_HOME": str(home / "share")})
        environment.start()
        self.addCleanup(environment.stop)
        self.config = update_test.data_folder() / "config.toml"
        self.game = home / "game"

    def test_a_run_removes_the_config_it_wrote_so_the_next_run_names_its_own_game(self):
        with update_test.first_launch_config(self.game):
            self.assertIn(f'ra2_dir = "{self.game.as_posix()}"', self.config.read_text())
        self.assertFalse(self.config.exists())

    def test_a_players_config_stays_as_it_was(self):
        self.config.parent.mkdir(parents=True)
        self.config.write_text('ra2_dir = "/Games/RA2"\n')
        with update_test.first_launch_config(self.game):
            pass
        self.assertEqual(self.config.read_text(), 'ra2_dir = "/Games/RA2"\n')


class UpdateKeyTest(unittest.TestCase):
    def test_a_key_git_bash_would_rewrite_as_a_path_is_drawn_again(self):
        keys = [("pem", "/NtLEIKtreh/Pov9g+VmZOivzbKxYAmanXTFtZQI2iI="),
                ("pem", "NtLEIKtreh/Pov9g+VmZOivzbKxYAmanXTFtZQI2iI/=")]
        with mock.patch.object(update_test, "signing_key", side_effect=keys):
            self.assertEqual(update_test.update_key(Path("unused")), keys[1][1])

    def test_the_rekeyed_releases_new_key_is_drawn_again_too(self):
        keys = [("pem", "/NtLEIKtreh/Pov9g+VmZOivzbKxYAmanXTFtZQI2iI="),
                ("pem", "NtLEIKtreh/Pov9g+VmZOivzbKxYAmanXTFtZQI2iI/=")]
        with tempfile.TemporaryDirectory() as temp, \
                mock.patch.object(update_test, "signing_key", side_effect=keys) as drawn, \
                mock.patch.object(update_test, "publish") as publish:
            update_test.publish_rekeyed(Path(temp), "1.2.3", None)
        self.assertEqual(drawn.call_count, 2)
        publish.assert_called_once()


class RunTest(unittest.TestCase):
    def test_a_program_that_hangs_fails_the_run_instead_of_holding_it(self):
        with self.assertRaisesRegex(SystemExit, r"still running after 1 s"):
            update_test.run(sys.executable, "-c", "import time; time.sleep(60)", timeout=1)


if __name__ == "__main__":
    unittest.main()
