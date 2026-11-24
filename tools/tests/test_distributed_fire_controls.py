"""Incomplete numeric/RNG controls must not be accepted or published."""
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32

from tools import native_oracle as native
from tools.spatial_oracle import distributed_fire, fly_landing_phase, rearm_timer


class DistributedFireControlTests(unittest.TestCase):
    def test_air_tracker_startup_rejects_an_early_stop(self):
        machine = Uc(UC_ARCH_X86, UC_MODE_32)
        machine.mem_map(0x400000, 0x500000)
        machine.mem_map(native.STACK_BASE, native.STACK_SIZE)
        machine.mem_write(0x412870, b'\xf4')
        with self.assertRaisesRegex(native.OracleError, 'Incomplete execution'):
            fly_landing_phase.initialize_air_tracker(
                machine, native.STACK_BASE+native.STACK_SIZE-0x1000,
                buffers=native.SCRATCH, extent=(64,64))

    def test_guard_tail_rejects_an_early_stop(self):
        machine = Uc(UC_ARCH_X86, UC_MODE_32)
        machine.mem_map(0x400000, 0x800000)
        machine.mem_map(native.SCRATCH, native.SCRATCH_SIZE)
        machine.mem_map(native.STACK_BASE, native.STACK_SIZE)
        machine.mem_map(native.RET_MAGIC, 0x1000)
        machine.mem_write(rearm_timer.GUARD_BEGIN, b'\xf4')
        case = dict(name='early_stop', distributed=True, candidate_count=1,
                    frame=173, rearm_start=173, rearm_duration=0, base_cadence=27)
        with patch.object(rearm_timer, 'machine', return_value=machine):
            with self.assertRaisesRegex(native.OracleError, 'Incomplete execution'):
                rearm_timer.guard_distributed_rng(case, bytes(0x3F4))

    def test_failed_query_cannot_publish_partial_selection_rows(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'candidate.json'
            with patch.object(distributed_fire, 'cases', return_value=[]), \
                 patch.object(distributed_fire, 'air_query_controls',
                              side_effect=native.OracleError('incomplete populated query')):
                with self.assertRaisesRegex(native.OracleError, 'incomplete populated query'):
                    distributed_fire.main(['--write', '--output', str(output)])
            self.assertEqual(list(Path(directory).iterdir()), [])


if __name__ == '__main__':
    unittest.main()
