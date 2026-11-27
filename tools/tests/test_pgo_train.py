import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools import pgo_train


class TrainingTests(unittest.TestCase):
    def _run_headless_training(self, root, checked_hashes):
        def process(argv, **kwargs):
            if '--scenario' in argv:
                mode = argv[argv.index('--scenario') + 1]
                (root / f'{mode}.profraw').write_bytes(b'profile')
                stdout = f'ticks={pgo_train.MATCH_TICKS} total_s=1\nfinal_hash=0123456789abcdef\n'
            else:
                self.assertEqual(argv[1], '--replay-verify')
                (root / 'verify.profraw').write_bytes(b'profile')
                stdout = json.dumps({'ticks': pgo_train.REPLAY_TICKS, 'checked_hashes': checked_hashes,
                                     'final_tick': pgo_train.REPLAY_TICKS})
            return subprocess.CompletedProcess(argv, 0, stdout)

        with patch('tools.pgo_train.subprocess.run', side_effect=process) as run:
            reports = pgo_train.train(Path('game'), Path('bench'), root, root, root)
            self.assertEqual(run.call_count, 3)
        return reports

    def test_each_workload_must_write_its_own_nonempty_profile(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            old = root / 'sim.profraw'
            old.write_bytes(b'previous run')
            empty = root / 'game.profraw'
            empty.touch()
            with self.assertRaisesRegex(RuntimeError, 'shipping game'):
                pgo_train.require_profile(root, {old}, 'shipping game')
            empty.write_bytes(b'new counters')
            pgo_train.require_profile(root, {old}, 'shipping game')

    def test_partial_shipping_replay_fails_even_if_it_exited_successfully(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(RuntimeError, 'before verifying'):
                self._run_headless_training(Path(directory), 0)

    def test_release_training_finishes_with_the_headless_battle_and_replay(self):
        with tempfile.TemporaryDirectory() as directory:
            reports = self._run_headless_training(Path(directory), pgo_train.REPLAY_TICKS)
        self.assertEqual(reports, [
            'AI battle: 1800 frames, final hash 0123456789abcdef.',
            'Shipping game replay: 240 verified frame hashes.',
        ])
