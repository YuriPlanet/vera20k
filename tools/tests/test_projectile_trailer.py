"""Checked Scalable execution must not publish partial native observations."""
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32

from tools import native_oracle
from tools.projectile_oracle import projectile_trailer as trailer


class ScalableExecutionTests(unittest.TestCase):
    def test_scalable_initializer_rejects_early_unicorn_stop(self):
        machine = Uc(UC_ARCH_X86, UC_MODE_32)
        machine.mem_map(0x466000, 0x1000)
        machine.mem_write(0x4662E0, b'\xf4')
        with self.assertRaisesRegex(native_oracle.OracleError, 'Incomplete execution'):
            trailer.initialize_scalable_vector(SimpleNamespace(u=machine))

    def test_failed_scalable_history_cannot_publish_even_with_write_requested(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'candidate.json'
            with patch.object(trailer, 'run_case', return_value=({}, {})), \
                 patch.object(trailer, 'source_files', return_value=[]), \
                 patch.object(trailer, 'seeded_struct', return_value=b''), \
                 patch.object(trailer, 'retail_projectiles', return_value=[]), \
                 patch.object(trailer, 'placement_controls', return_value={}), \
                 patch.object(trailer, 'scalable_controls',
                              side_effect=native_oracle.OracleError('incomplete scalable history')):
                with self.assertRaisesRegex(native_oracle.OracleError, 'incomplete scalable history'):
                    trailer.main(['--write', '--output', str(output)])
            self.assertEqual(list(Path(directory).iterdir()), [])


if __name__ == '__main__':
    unittest.main()
