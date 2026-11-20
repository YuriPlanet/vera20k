"""Tests for the local game-file preparation tool."""

from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock
import zipfile

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools import game_files  # noqa: E402


class KeepFileTest(unittest.TestCase):
    """Which files a slim copy keeps."""

    def test_archives_and_data_files_are_kept(self) -> None:
        for name in ('ra2md.mix', 'ra2.mix', 'main.csf', 'sidebar.pal', 'e1.shp', 'theme.wav'):
            self.assertTrue(game_files.keep_file(Path(name)), name)

    def test_loose_inis_are_kept_because_the_folder_wins_over_the_archives(self) -> None:
        self.assertTrue(game_files.keep_file(Path('rulesmd.ini')))
        self.assertTrue(game_files.keep_file(Path('RULESMD.INI')))

    def test_loose_scenarios_and_maps_are_kept(self) -> None:
        for name in ('arena.yro', 'Pack.PKT', 'hills.mmx', 'river.mpr'):
            self.assertTrue(game_files.keep_file(Path(name)), name)

    def test_executables_and_installers_are_dropped(self) -> None:
        for name in ('gamemd.exe', 'game.dll', 'setup.exe', 'readme.txt'):
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
        self.assertEqual(copied, {'ra2md.mix', 'main.csf', 'rulesmd.ini'})
        self.assertIn('copied 3 files', lines[0])
        self.assertIn('skipped 1 files', lines[1])

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


class FindGameFolderTest(unittest.TestCase):
    """The folder that holds the archives, however the source is nested."""

    def test_a_steam_style_layout_is_found(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            game = base / 'steamapps' / 'common' / 'Command & Conquer Red Alert 2 and YR'
            game.mkdir(parents=True)
            (game / 'ra2md.mix').write_bytes(b'archive')
            self.assertEqual(game_files.find_game_folder(base), game)

    def test_a_folder_without_the_archives_is_returned_unchanged(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            self.assertEqual(game_files.find_game_folder(base), base)


class UnpackTest(unittest.TestCase):
    """A source that arrives as a download."""

    def test_a_zip_holding_the_game_is_unpacked_and_found(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            archive = base / 'yr.zip'
            with zipfile.ZipFile(archive, 'w') as bundle:
                bundle.writestr('Yuri/ra2md.mix', b'archive')
                bundle.writestr('Yuri/gamemd.exe', b'executable')
            workdir = base / 'unpacked'
            found = game_files.unpack(archive, workdir)
            self.assertEqual(found, workdir / 'Yuri')
            self.assertEqual((found / 'ra2md.mix').read_bytes(), b'archive')

    def test_a_zip_that_would_write_outside_the_folder_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            archive = base / 'evil.zip'
            with zipfile.ZipFile(archive, 'w') as bundle:
                bundle.writestr('../escaped.mix', b'archive')
            with self.assertRaises(SystemExit):
                game_files.unpack(archive, base / 'unpacked')
            self.assertFalse((base / 'escaped.mix').exists())

    def test_a_prepared_archive_replaces_the_folder_and_clears_the_scratch(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            archive = base / 'yr.zip'
            with zipfile.ZipFile(archive, 'w') as bundle:
                bundle.writestr('Yuri/ra2md.mix', b'archive')
            target = base / 'target'
            root = base / 'checkout'
            root.mkdir()
            (root / 'config.toml.example').write_text(
                '[paths]\nra2_dir = "C:/Westwood/RA2"\n', encoding='utf-8')
            with mock.patch.object(game_files, 'run_game_data_check', return_value=([], [], 0)):
                lines, ready = game_files.prepare(archive, target, root)
            self.assertTrue(ready)
            self.assertEqual({path.name for path in target.iterdir()}, {'ra2md.mix'})
            self.assertFalse(target.with_name(target.name + '.unpacked').exists())
            self.assertIn('unpacked yr.zip', '\n'.join(lines))


class SmokeTest(unittest.TestCase):
    """The headless match that says the content plays."""

    def test_a_match_that_runs_is_reported_as_played(self) -> None:
        finished = game_files.subprocess.CompletedProcess([], 0, stdout='ticks=300\n', stderr='')
        with mock.patch.object(game_files.subprocess, 'run', return_value=finished) as run:
            lines, played = game_files.smoke(Path('/tmp/yr'))
        self.assertTrue(played)
        self.assertEqual(lines[-1], 'the match ran on /tmp/yr')
        self.assertEqual(run.call_args.kwargs['env']['RA2_DIR'], str(Path('/tmp/yr').resolve()))

    def test_a_match_that_fails_is_reported_as_not_played(self) -> None:
        finished = game_files.subprocess.CompletedProcess([], 1, stdout='', stderr='no archives\n')
        with mock.patch.object(game_files.subprocess, 'run', return_value=finished):
            lines, played = game_files.smoke(Path('/tmp/yr'))
        self.assertFalse(played)
        self.assertIn('the match did not run on /tmp/yr', lines[-1])
        self.assertIn('no archives', lines)


class HumanSizeTest(unittest.TestCase):
    """Byte counts read as short strings."""

    def test_sizes_are_scaled(self) -> None:
        self.assertEqual(game_files.human_size(512), '512 B')
        self.assertEqual(game_files.human_size(2048), '2.0 KB')
        self.assertEqual(game_files.human_size(5 * 1024 * 1024), '5.0 MB')


if __name__ == '__main__':
    unittest.main()
