"""Tests for the local game-file preparation tool."""

from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools import game_files  # noqa: E402


class KeepFileTest(unittest.TestCase):
    """Which files a slim copy keeps."""

    def test_archives_and_data_files_are_kept(self) -> None:
        for name in ('ra2md.mix', 'ra2.mix', 'main.csf', 'sidebar.pal', 'e1.shp', 'theme.wav'):
            self.assertTrue(game_files.keep_file(Path(name)), name)

    def test_loose_inis_are_dropped_because_they_override_the_archives(self) -> None:
        self.assertFalse(game_files.keep_file(Path('rulesmd.ini')))
        self.assertFalse(game_files.keep_file(Path('RULESMD.INI')))

    def test_executables_and_movies_are_dropped(self) -> None:
        for name in ('gamemd.exe', 'game.dll', 'ea_wwlogo.bik', 'setup.exe'):
            self.assertFalse(game_files.keep_file(Path(name)), name)


class SlimTest(unittest.TestCase):
    """Copying a licensed folder into a slim one."""

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.source = self.base / 'source'
        self.target = self.base / 'target'
        self.source.mkdir()
        (self.source / 'ra2md.mix').write_bytes(b'archive')
        (self.source / 'main.csf').write_bytes(b'strings')
        (self.source / 'rulesmd.ini').write_bytes(b'[General]\n')
        (self.source / 'gamemd.exe').write_bytes(b'executable')

    def test_only_the_files_the_engine_reads_are_copied(self) -> None:
        lines = game_files.slim(self.source, self.target)
        copied = {path.name for path in self.target.iterdir()}
        self.assertEqual(copied, {'ra2md.mix', 'main.csf'})
        self.assertIn('copied 2 files', lines[0])
        self.assertIn('skipped 2 files', lines[1])

    def test_a_missing_source_is_refused(self) -> None:
        with self.assertRaises(SystemExit):
            game_files.slim(self.base / 'absent', self.target)


class ManifestTest(unittest.TestCase):
    """The JSON inventory of a folder."""

    def test_every_kept_file_carries_its_size_and_digest(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary)
            (folder / 'ra2md.mix').write_bytes(b'archive')
            (folder / 'gamemd.exe').write_bytes(b'executable')
            report = game_files.manifest(folder)
        names = [entry['name'] for entry in report['files']]
        self.assertEqual(names, ['ra2md.mix'])
        entry = report['files'][0]
        self.assertEqual(entry['bytes'], len(b'archive'))
        self.assertEqual(len(entry['sha256']), 64)
        self.assertEqual(report['steam_appid'], game_files.STEAM_APPID)


class WriteConfigTest(unittest.TestCase):
    """Pointing the checkout at a game folder."""

    def test_the_ra2_dir_line_is_replaced_and_the_rest_is_kept(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / 'config.toml.example').write_text(
                '[paths]\nra2_dir = "C:/Westwood/RA2"\nsettings_dir = "."\n', encoding='utf-8')
            folder = root / 'game'
            folder.mkdir()
            message = game_files.write_config(folder, root)
            written = (root / 'config.toml').read_text(encoding='utf-8')
        self.assertIn('ra2_dir', message)
        self.assertIn(str(folder.resolve().as_posix()), written)
        self.assertIn('settings_dir = "."', written)


class VerifyTest(unittest.TestCase):
    """Reporting on a folder that is not there needs no game executable."""

    def test_a_missing_folder_is_reported_as_a_problem(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            lines, ready = game_files.verify(Path(temporary) / 'absent')
        self.assertFalse(ready)
        self.assertIn('is not a folder', lines[0])

    def test_a_passing_install_check_is_not_described_as_a_successful_launch(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            with mock.patch.object(game_files, 'run_game_data_check', return_value=([], [], 0)):
                lines, ready = game_files.verify(Path(temporary))
        self.assertTrue(ready)
        self.assertEqual(lines[-1], 'required game-data checks passed')
        self.assertNotIn('can start', '\n'.join(lines))


class PrepareTest(unittest.TestCase):
    """Staging a licensed copy so a bad source cannot replace a good folder."""

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.source = self.base / 'source'
        self.target = self.base / 'target'
        self.root = self.base / 'checkout'
        self.source.mkdir()
        self.root.mkdir()
        (self.source / 'ra2md.mix').write_bytes(b'archive')
        (self.root / 'config.toml.example').write_text(
            '[paths]\nra2_dir = "C:/Westwood/RA2"\nsettings_dir = "."\n', encoding='utf-8')

    def test_an_incomplete_source_leaves_the_working_folder_and_the_config_alone(self) -> None:
        self.target.mkdir()
        (self.target / 'ra2md.mix').write_bytes(b'the copy in use')
        with mock.patch.object(game_files, 'run_game_data_check',
                               return_value=(['ra2md.csf: Asset not found'], [], 1)):
            lines, ready = game_files.prepare(self.source, self.target, self.root)
        self.assertFalse(ready)
        self.assertEqual((self.target / 'ra2md.mix').read_bytes(), b'the copy in use')
        self.assertFalse((self.root / 'config.toml').exists())
        self.assertFalse(self.target.with_name(self.target.name + '.staging').exists())
        self.assertIn('nothing was changed', '\n'.join(lines))

    def test_a_complete_source_replaces_the_folder_and_writes_the_config(self) -> None:
        self.target.mkdir()
        (self.target / 'stale.mix').write_bytes(b'from an older copy')
        with mock.patch.object(game_files, 'run_game_data_check', return_value=([], [], 0)):
            lines, ready = game_files.prepare(self.source, self.target, self.root)
        self.assertTrue(ready)
        self.assertEqual({path.name for path in self.target.iterdir()}, {'ra2md.mix'})
        written = (self.root / 'config.toml').read_text(encoding='utf-8')
        self.assertIn(str(self.target.resolve().as_posix()), written)
        self.assertIn('required game-data checks passed', '\n'.join(lines))


class HumanSizeTest(unittest.TestCase):
    """Byte counts read as short strings."""

    def test_sizes_are_scaled(self) -> None:
        self.assertEqual(game_files.human_size(512), '512 B')
        self.assertEqual(game_files.human_size(2048), '2.0 KB')
        self.assertEqual(game_files.human_size(5 * 1024 * 1024), '5.0 MB')


if __name__ == '__main__':
    unittest.main()
