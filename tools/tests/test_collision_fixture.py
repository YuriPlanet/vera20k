"""Synthetic execution faults must never become accepted native observations."""
import importlib
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from tools import native_oracle, native_slope
from tools.projectile_oracle import collision_fixture as fixture

ROOT = Path(__file__).resolve().parents[2]
ENTRIES = (0x4677D3, 0x467666, 0x47C3D0, 0x467494, 0x467CA9,
           0x468BB0, 0x4CC100, 0x466DB1, 0x467BF0, 0x467C3C, 0x7CEAAF)


def halted_image(machine):
    machine.mem_map(native_oracle.IMAGE_BASE, native_oracle.IMAGE_SIZE)
    machine.mem_write(0x822D80, struct.pack('<H', 0x037F))
    # HLT returns from Unicorn without a UcError, but has not reached the
    # requested region boundary. This is deliberately not a retail algorithm.
    for entry in ENTRIES:
        machine.mem_write(entry, b'\xf4')


class CollisionCompletionTests(unittest.TestCase):
    def test_every_observation_rejects_early_successful_unicorn_stop(self):
        cases = {
            'ordinary admission': lambda: fixture.admission({}),
            'reflection': lambda: fixture.reflection([0] * 12, 0, [20, 0, -6], .75),
            'nearest': lambda: fixture.nearest([]),
            'geometry': lambda: fixture.geometry({'candidate': [640, 640, 0]}, []),
            'ordinary handoff': lambda: fixture.final_handoff(dict(
                candidate=[640, 128, 0], velocity=[4, 0, 0], airburst=False,
                inaccurate=False, target_present=True, near_target=False, mode=0)),
            'shared probe': lambda: fixture.shared_probe({}),
            'line walk': lambda: fixture.line_walk(dict(source_coord=[128,128,5], target_coord=[640,640,-6], cliffs=True)),
            'homing admission': lambda: fixture.homing_admission(1, 3, [4, 0, 0], False, False),
            'homing clamp': lambda: fixture.homing_handoff(1, 0, False, False, True),
            'homing source mode': lambda: fixture.homing_source_mode(False, False, 0),
            'slope startup': native_slope.slope_matrices,
        }
        with patch.object(fixture, 'load_image', halted_image), \
             patch.object(native_slope, 'load_image', halted_image):
            for name, operation in cases.items():
                with self.subTest(name=name), self.assertRaisesRegex(
                        native_oracle.OracleError, 'Incomplete execution'):
                    operation()

    def test_homing_handoff_checks_second_region_after_completed_clamp(self):
        def completed_clamp(machine):
            halted_image(machine)
            # Skip only the first synthetic region to its declared boundary.
            start, end = 0x467BF0, 0x467C0C
            machine.mem_write(start, b'\xe9' + struct.pack('<i', end - start - 5))
        with patch.object(fixture, 'load_image', completed_clamp):
            with self.assertRaisesRegex(native_oracle.OracleError, 'from 0x00467CA9'):
                fixture.homing_handoff(1, 0, False, False, True)

    def test_layouts_preserve_distinct_cached_control_word_contracts(self):
        with patch.object(fixture, 'load_image', halted_image):
            ordinary, _ = fixture.prepare_ordinary({})
            homing, _ = fixture.prepare_homing(1, 207, False, False)
            shared, _, _, _ = fixture.prepare_shared({})
        for machine in (ordinary, homing, shared):
            self.assertEqual(machine.reg_read(fixture.UC_X86_REG_FPCW), 0x0E7F)
        for machine in (ordinary, homing):
            self.assertEqual(bytes(machine.mem_read(0x822D80, 2)), b'\x7f\x03')
        self.assertEqual(bytes(shared.mem_read(0x822D80, 2)), b'\x7f\x0e')

    def test_failed_native_run_never_writes_reference_even_with_write_requested(self):
        for name in ('ordinary_collision', 'shared_collision', 'homing_impact'):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                output = Path(directory) / 'reference.json'
                module = importlib.import_module('tools.projectile_oracle.' + name)
                with patch.object(fixture, 'load_image', halted_image):
                    with self.assertRaises(native_oracle.OracleError):
                        module.main(['--write', '--output', str(output)])
                self.assertEqual(list(Path(directory).iterdir()), [])
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'line.json'
            module = importlib.import_module('tools.projectile_oracle.shared_collision')
            with patch.object(fixture, 'load_image', halted_image):
                with self.assertRaises(native_oracle.OracleError):
                    module.main(['--line-walk', '--write', '--output', str(output)])
            self.assertEqual(list(Path(directory).iterdir()), [])

    def test_line_walk_help_needs_no_retail_image(self):
        result = subprocess.run(
            [sys.executable, '-m', 'tools.projectile_oracle.shared_collision',
             '--line-walk', '--help'], cwd=ROOT,
            env=dict(os.environ, VERA20K_GAMEMD_EXE='absent-retail.exe'),
            capture_output=True, text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('--check', result.stdout)

    def test_failure_gates_survive_optimized_python(self):
        result = subprocess.run(
            [sys.executable, '-O', '-m', 'unittest',
             __name__ + '.CollisionCompletionTests.test_every_observation_rejects_early_successful_unicorn_stop',
             __name__ + '.CollisionCompletionTests.test_failed_native_run_never_writes_reference_even_with_write_requested'],
            cwd=ROOT, env=dict(os.environ, VERA20K_GAMEMD_EXE='absent-retail.exe'),
            capture_output=True, text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == '__main__':
    unittest.main()
