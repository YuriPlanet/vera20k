"""Failure probes use synthetic memory, never retail bytes or native algorithms."""
import os
from pathlib import Path
import subprocess
import sys
import unittest


ROOT = Path(__file__).resolve().parents[2]


def fault_probe(kind):
    from unittest.mock import patch
    from tools import native_oracle
    from tools.projectile_oracle import load_timers as fixture

    original_uc = fixture.Uc

    def machine_factory(*args):
        machine = original_uc(*args)
        original_hook_add = machine.hook_add

        def hook_add(hook_type, callback, *args, **kwargs):
            machine.fixture_hook = callback
            return original_hook_add(hook_type, callback, *args, **kwargs)

        machine.hook_add = hook_add
        return machine

    def synthetic_image(machine):
        machine.mem_map(native_oracle.IMAGE_BASE, native_oracle.IMAGE_SIZE)

    def boundary(machine, begin, end, **_kwargs):
        machine.reg_write(fixture.UC_X86_REG_EAX, 0)
        if begin == 0x46AFB0:
            print('SAVE_INVOKED', flush=True)
            if kind == 'save':
                machine.reg_write(fixture.UC_X86_REG_EAX, 1)
                return end
            # Supply a valid-length stream write so the read-underflow probe
            # can reach the global reader. No Save algorithm is reproduced.
            fixture.w32(machine, fixture.SP+8, fixture.BULLET)
            fixture.w32(machine, fixture.SP+12, 356)
            machine.fixture_hook(machine, fixture.HOOK, 1, None)
        elif begin == 0x67F9C0:
            fixture.w32(machine, fixture.SP+8, fixture.BULLET)
            fixture.w32(machine, fixture.SP+12, 16)  # Only 12 bytes supplied.
            machine.fixture_hook(machine, fixture.HOOK+16, 1, None)
        return end

    with patch.object(fixture, 'Uc', machine_factory), \
         patch.object(fixture, 'load_image', synthetic_image), \
         patch.object(fixture, 'run_checked', boundary):
        fixture.run(10, 100, 1, 'unit', 20)


class LoadTimerFailures(unittest.TestCase):
    def test_save_and_stream_failures_survive_optimized_python(self):
        for kind, message in [('save', 'Bullet Save failed: HRESULT=1'),
                              ('read', 'IStream read exceeds supplied fixture bytes')]:
            with self.subTest(kind=kind):
                env = dict(os.environ, VERA20K_GAMEMD_EXE='deliberately-missing.exe',
                           PYTHONDONTWRITEBYTECODE='1')
                result = subprocess.run(
                    [sys.executable, '-O', '-c',
                     'from tools.tests.test_load_timers import fault_probe; '
                     f'fault_probe({kind!r})'],
                    cwd=ROOT, env=env, capture_output=True, text=True,
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('SAVE_INVOKED', result.stdout, result.stderr)
                self.assertIn(message, result.stderr)


if __name__ == '__main__':
    unittest.main()
