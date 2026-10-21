from contextlib import redirect_stderr, redirect_stdout
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    'integration_slot', Path(__file__).resolve().parents[1] / 'integration_slot.py')
slot = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(slot)


def git(root, *arguments):
    subprocess.run(['git', '-c', 'user.name=t', '-c', 'user.email=t@t', '-c', 'commit.gpgsign=false',
                    '-c', 'core.hooksPath=/dev/null', *arguments],
                   cwd=root, check=True, capture_output=True)


class IntegrationSlotTest(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.main = Path(directory.name).resolve() / 'repo'
        self.main.mkdir()
        git(self.main, 'init', '-q')
        (self.main / 'a.txt').write_text('a\n')
        git(self.main, 'add', '-A')
        git(self.main, 'commit', '-q', '-m', 'c')
        self.claims = self.main / '.git' / 'integration-slots'

    def claim(self, owner):
        out, err = io.StringIO(), io.StringIO()
        with redirect_stdout(out), redirect_stderr(err):
            code = slot.claim(self.main, owner, 0)
        return code, out.getvalue().strip(), err.getvalue()

    def release(self, owner):
        with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
            return slot.release(self.main, owner)

    def slot(self, index):
        return str(self.main.parent / 'repo-wt' / f'integration-{index}')

    def test_owners_get_distinct_slots_and_a_released_slot_keeps_its_path(self):
        self.assertEqual(self.claim('pr1')[:2], (0, self.slot(1)))
        self.assertEqual(self.claim('pr1')[:2], (0, self.slot(1)))  # Same owner, same slot.
        self.assertEqual(self.claim('pr2')[:2], (0, self.slot(2)))
        code, _, err = self.claim('pr3')
        self.assertEqual(code, 1)
        self.assertIn('held by pr1', err)
        self.assertEqual(self.release('pr1'), 0)
        self.assertEqual(self.claim('pr3')[:2], (0, self.slot(1)))
        self.assertEqual(self.release('pr1'), 1)  # No longer the holder.

    def test_release_refuses_uncommitted_work(self):
        path = Path(self.claim('pr1')[1])
        (path / 'a.txt').write_text('changed\n')
        self.assertEqual(self.release('pr1'), 1)
        self.assertTrue((self.claims / '1.json').exists())
        git(path, 'checkout', '--', 'a.txt')
        self.assertEqual(self.release('pr1'), 0)

    def test_stale_claim_is_taken_over_only_on_a_clean_slot(self):
        first = Path(self.claim('pr1')[1])
        self.claim('pr2')
        for index in (1, 2):
            (self.claims / f'{index}.json').write_text(json.dumps({'owner': f'pr{index}', 'claimed_unix': 0}))
        (first / 'stray.txt').write_text('x\n')
        code, path, err = self.claim('pr3')
        self.assertEqual((code, path), (0, self.slot(2)))
        self.assertIn('slot 1 has a stale claim by pr1', err)
        self.assertIn('took over a stale claim by pr2', err)
        self.assertTrue((first / 'stray.txt').exists())  # Never discarded.
        self.assertEqual(json.loads((self.claims / '1.json').read_text())['owner'], 'pr1')

    def test_unregistered_directory_is_never_used(self):
        Path(self.slot(1)).mkdir(parents=True)
        code, path, err = self.claim('pr1')
        self.assertEqual((code, path), (0, self.slot(2)))
        self.assertIn('not a worktree of this repository', err)
        self.assertFalse((self.claims / '1.json').exists())


if __name__ == '__main__':
    unittest.main()
