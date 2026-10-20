"""Machine-local checkout preparation: links, copies, config.toml and the readiness report."""
from contextlib import redirect_stdout
import importlib.util
import io
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest
from unittest.mock import patch

from tools import prepare_checkout as checkout

SYMLINKS = 'symlink privilege varies on Windows'


def git(root, *arguments):
    subprocess.run(['git', '-c', 'user.name=t', '-c', 'user.email=t@t', '-c', 'commit.gpgsign=false',
                    '-c', 'core.hooksPath=/dev/null', *arguments],
                   cwd=root, check=True, capture_output=True)


class CheckoutTest(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.base = Path(directory.name).resolve()
        self.game = self.base / 'game'
        self.game.mkdir()
        (self.game / 'RA2MD.MIX').write_bytes(b'')
        (self.game / 'gamemd.exe').write_bytes(b'MZ')
        self.main = self.base / 'main'
        self.main.mkdir()
        (self.main / 'config.toml.example').write_text(
            (checkout.REPO / 'config.toml.example').read_text(encoding='utf-8'), encoding='utf-8')
        # Report readiness from the checkout alone, not from this shell's game folder.
        environment = patch.dict(os.environ)
        environment.start()
        self.addCleanup(environment.stop)
        for name in ('RA2_DIR', 'VERA20K_GAMEMD_EXE'):
            os.environ.pop(name, None)

    def add_ini(self, root):
        (root / 'ini').mkdir()
        (root / 'ini' / 'rulesmd.ini').write_text('[General]\n')

    def report(self, root, main):
        lines, ready = checkout.report(root, main)
        return '\n'.join(lines), ready


class LinkedWorktreeTests(CheckoutTest):
    def setUp(self):
        super().setUp()
        git(self.main, 'init', '-q')
        git(self.main, 'add', '-A')
        git(self.main, 'commit', '-q', '-m', 'c')
        self.add_ini(self.main)
        checkout.write_config(self.main, self.game)
        # One level deeper than the main checkout, so relative paths differ.
        self.worktree = self.base / 'trees' / 'topic'
        git(self.main, 'worktree', 'add', '-q', '-b', 'topic', str(self.worktree))

    @unittest.skipIf(os.name == 'nt', SYMLINKS)
    def test_links_ini_and_copies_config_once(self):
        self.assertEqual(checkout.main_checkout(self.worktree), self.main)
        self.assertEqual(checkout.prepare(self.worktree, self.main, None),
                         [f'copied config.toml from {self.main}', f'linked ini to {self.main / "ini"}'])
        self.assertEqual((self.worktree / 'ini').resolve(), self.main / 'ini')
        self.assertEqual((self.worktree / 'config.toml').read_bytes(), (self.main / 'config.toml').read_bytes())
        self.assertEqual(checkout.prepare(self.worktree, self.main, None), [])
        text, ready = self.report(self.worktree, self.main)
        self.assertFalse(ready)
        self.assertIn(f'fix: set RA2_DIR to "{self.game}" in the shell that runs them', text)
        os.environ['RA2_DIR'] = str(self.game)
        text, ready = self.report(self.worktree, self.main)
        self.assertTrue(ready, text)
        self.assertIn(f'(worktree of {self.main})', text.splitlines()[0])

    def test_a_relative_ra2_dir_keeps_its_folder(self):
        (self.main / 'config.toml').write_text('[paths]\nra2_dir = "../game"\n')
        self.assertIn(f'copied config.toml from {self.main}, with its relative ra2_dir made absolute',
                      checkout.prepare(self.worktree, self.main, None))
        config = tomllib.loads((self.worktree / 'config.toml').read_text(encoding='utf-8'))
        self.assertEqual(config['paths']['ra2_dir'], self.game.as_posix())

    def test_existing_files_and_folders_are_never_replaced(self):
        (self.worktree / 'config.toml').write_text('[paths]\nra2_dir = "elsewhere"\n')
        (self.worktree / 'ini').mkdir()
        self.assertEqual(checkout.prepare(self.worktree, self.main, None), [])
        self.assertEqual((self.worktree / 'config.toml').read_text(), '[paths]\nra2_dir = "elsewhere"\n')
        self.assertEqual(list((self.worktree / 'ini').iterdir()), [])
        text, ready = self.report(self.worktree, self.main)
        self.assertFalse(ready)
        self.assertIn(f'{self.worktree / "elsewhere"} is not a folder', text)
        self.assertIn(f'fix: remove {self.worktree / "ini"}, then run python -m tools.prepare_checkout', text)

    @unittest.skipIf(os.name == 'nt', SYMLINKS)
    def test_a_broken_config_link_is_not_written_through(self):
        os.symlink(self.base / 'gone.toml', self.worktree / 'config.toml')
        checkout.prepare(self.worktree, self.main, None)
        self.assertFalse((self.base / 'gone.toml').exists())
        text, ready = self.report(self.worktree, self.main)
        self.assertFalse(ready)
        self.assertIn('problem   config.toml: cannot read config.toml', text)

    def test_without_symlink_rights_the_folder_is_copied(self):
        with patch.object(checkout.os, 'symlink', side_effect=OSError('no symlink privilege')):
            self.assertIn(f'copied ini/ from {self.main}', checkout.prepare(self.worktree, self.main, None))
        self.assertFalse((self.worktree / 'ini').is_symlink())
        self.assertEqual((self.worktree / 'ini' / 'rulesmd.ini').read_text(), '[General]\n')

    def test_check_changes_nothing(self):
        with redirect_stdout(io.StringIO()) as output:
            self.assertEqual(checkout.main(['--check'], root=self.worktree), 1)
        self.assertFalse(os.path.lexists(self.worktree / 'ini'))
        self.assertFalse(os.path.lexists(self.worktree / 'config.toml'))
        self.assertIn('fix: python -m tools.prepare_checkout\n', output.getvalue())


class MainCheckoutTests(CheckoutTest):
    def test_ra2_dir_writes_config_from_the_example(self):
        with redirect_stdout(io.StringIO()):
            checkout.main(['--ra2-dir', str(self.game)], root=self.main)
        config = tomllib.loads((self.main / 'config.toml').read_text(encoding='utf-8'))
        self.assertEqual(config['paths']['ra2_dir'], self.game.as_posix())
        example = tomllib.loads((self.main / 'config.toml.example').read_text(encoding='utf-8'))
        self.assertEqual({key: value for key, value in config.items() if key != 'paths'},
                         {key: value for key, value in example.items() if key != 'paths'})
        self.assertEqual(checkout.prepare(self.main, self.main, self.game),
                         ['kept the existing config.toml; edit its ra2_dir to change the game folder'])

    def test_only_the_game_folder_is_accepted(self):
        with self.assertRaisesRegex(SystemExit, "must be the Yuri's Revenge game folder"):
            checkout.prepare(self.main, self.main, self.base)
        self.assertFalse((self.main / 'config.toml').exists())

    def test_only_retail_tests_need_the_executable(self):
        # The game reads no gamemd.exe bytes; RA2_DIR's table checks do.
        (self.game / 'gamemd.exe').unlink()
        self.add_ini(self.main)
        checkout.write_config(self.main, self.game)
        os.environ['RA2_DIR'] = str(self.game)
        text, ready = self.report(self.main, self.main)
        self.assertIn('ok        config.toml: ra2_dir', text)
        self.assertIn('problem   RA2_DIR: ' + str(self.game) + ' has no gamemd.exe; retail tests read it', text)
        self.assertFalse(ready)

    def test_missing_ini_names_the_extractor(self):
        checkout.write_config(self.main, self.game)
        text, ready = self.report(self.main, self.main)
        self.assertFalse(ready)
        self.assertIn('fix: python -m tools.cargo_run -- run --bin extract-ini  (needs a valid ra2_dir)', text)

    def test_relative_ra2_dir_starts_at_the_config_folder(self):
        # GameConfig::parse_from's rule, which src/util/config.rs tests on the Rust side.
        (self.main / 'config.toml').write_text('[paths]\nra2_dir = "../game"\n')
        self.add_ini(self.main)
        os.environ['RA2_DIR'] = str(self.game)
        text, ready = self.report(self.main, self.main)
        self.assertTrue(ready, text)

    @unittest.skipIf(os.name == 'nt', SYMLINKS)
    def test_stray_link_and_bad_ra2_dir_are_reported(self):
        checkout.write_config(self.main, self.game)
        self.add_ini(self.main)
        os.symlink(self.main / 'ini', self.main / 'ini' / 'ini')
        os.environ['RA2_DIR'] = str(self.game)
        text, ready = self.report(self.main, self.main)
        self.assertTrue(ready, text)
        self.assertIn('is a stray link inside the INI folder', text)
        for value, problem in ((str(self.base / 'missing'), 'is not a folder'), ('', 'set but empty')):
            os.environ['RA2_DIR'] = value
            text, ready = self.report(self.main, self.main)
            self.assertFalse(ready)
            self.assertIn(problem, text)
            self.assertIn('fix: point RA2_DIR at the game folder', text)

    @unittest.skipUnless(importlib.util.find_spec('unicorn'), 'native oracles need Unicorn')
    def test_native_oracles_accept_any_executable(self):
        os.environ['RA2_DIR'] = str(self.game)
        text, _ = self.report(self.main, self.main)
        self.assertIn('ok        native oracles: retail gamemd.exe at', text)

    def test_check_and_ra2_dir_conflict(self):
        with self.assertRaises(SystemExit), redirect_stdout(io.StringIO()), \
                patch('sys.stderr', io.StringIO()):
            checkout.main(['--check', '--ra2-dir', str(self.game)], root=self.main)


if __name__ == '__main__':
    unittest.main()
