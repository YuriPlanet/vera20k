"""The update test's stand-in config.toml lasts only for its run."""
import os
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


if __name__ == "__main__":
    unittest.main()
