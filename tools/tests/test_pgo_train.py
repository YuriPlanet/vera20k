import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools import pgo_train


class TrainingTests(unittest.TestCase):
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
            root = Path(directory)
            def process(argv, **kwargs):
                name = argv[1]
                if '--scenario' in argv:
                    mode = argv[argv.index('--scenario') + 1]
                    (root / f'{mode}.profraw').write_bytes(b'profile')
                    stdout = f'ticks={pgo_train.MATCH_TICKS} total_s=1\nfinal_hash=0123456789abcdef\n'
                else:
                    self.assertEqual(name, '--replay-verify')
                    (root / 'verify.profraw').write_bytes(b'profile')
                    stdout = json.dumps({'ticks': 240, 'checked_hashes': 0, 'final_tick': 240})
                return subprocess.CompletedProcess(argv, 0, stdout)
            with patch('tools.pgo_train.subprocess.run', side_effect=process):
                with self.assertRaisesRegex(RuntimeError, 'before verifying'):
                    pgo_train.train(Path('game'), Path('bench'), root, root, root)
