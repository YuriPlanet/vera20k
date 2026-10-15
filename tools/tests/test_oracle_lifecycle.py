"""Oracle imports and help must never execute native code or publish references."""
import os
from pathlib import Path
import subprocess
import sys
import unittest


ROOT = Path(__file__).resolve().parents[2]
PRODUCERS = tuple('tools.projectile_oracle.' + name for name in (
    'fireat_launch', 'directed_launch', 'launch_scatter', 'building_pitch', 'voxel_launch',
    'arc_second_probe', 'fireat_runtime', 'fire_once', 'load_timers', 'ordinary_collision',
    'shared_collision', 'homing_impact', 'arc_domain', 'projectile_trailer',
)) + ('tools.input_oracle.gattling_loop',
     'tools.anim_oracle.boundary', 'tools.palette_oracle.oracle',
     'tools.ramp_height_oracle', 'tools.render_depth_oracle',
     'tools.ai_base_building_oracle', 'tools.ai_base_defense_oracle',
     'tools.ai_strategy_oracle', 'tools.ai_team_oracle',
     'tools.control_group_text_oracle',
     'tools.spatial_oracle.rearm_timer', 'tools.spatial_oracle.distributed_fire',
     'tools.spatial_oracle.fly_landing_phase')
HELPERS = ('tools.native_slope', 'tools.projectile_oracle.collision_fixture',
           'tools.palette_oracle.check')


def absent_retail_environment():
    environment = dict(os.environ)
    environment['VERA20K_GAMEMD_EXE'] = str(ROOT / 'deliberately-absent-retail.exe')
    environment.pop('RA2_DIR', None)
    environment['PYTHONDONTWRITEBYTECODE'] = '1'
    return environment


# A fresh interpreter prevents earlier imports from hiding module-level work.
# Inspect calls as well as raising: swallowing the guard error must not pass.
GUARDED_LIFECYCLE = r'''
from contextlib import ExitStack
import importlib
import runpy
import sys
from pathlib import Path
from unittest.mock import patch
from tools import native_oracle
operation, module = sys.argv[1:]
with ExitStack() as stack:
    guards = [stack.enter_context(patch.object(owner, name,
              side_effect=RuntimeError('forbidden lifecycle operation: ' + name)))
              for owner, name in ((native_oracle, 'image_bytes'),
                                  (native_oracle, 'run_checked'),
                                  (Path, 'write_text'), (Path, 'write_bytes'))]
    if module == 'tools.control_group_text_oracle':
        original_import = __import__
        def reject_optional_runtime(name, *args, **kwargs):
            if (name == 'tools.ai_base_building_oracle' or name == 'tools.native_oracle'
                    or name == 'unicorn' or name.startswith('unicorn.')):
                raise RuntimeError('forbidden optional runtime import: ' + name)
            return original_import(name, *args, **kwargs)
        stack.enter_context(patch('builtins.__import__', side_effect=reject_optional_runtime))
        guards.extend(stack.enter_context(patch.object(Path, name,
                      side_effect=RuntimeError('forbidden lifecycle operation: ' + name)))
                      for name in ('read_bytes', 'read_text'))
    if operation == 'import':
        importlib.import_module(module)
    else:
        sys.argv = [module, '--help']
        try:
            runpy.run_module(module, run_name='__main__')
        except SystemExit as error:
            if error.code not in (None, 0):
                raise
    for guard in guards:
        guard.assert_not_called()
'''


class OracleLifecycleTests(unittest.TestCase):
    def check_lifecycle(self, operation, module, optimized):
        command = [sys.executable] + (['-O'] if optimized else [])
        result = subprocess.run(
            command + ['-c', GUARDED_LIFECYCLE, operation, module],
            cwd=ROOT, env=absent_retail_environment(), capture_output=True,
            text=True, timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        if operation == 'help':
            options = (('--font', '--check', '--write')
                       if module == 'tools.control_group_text_oracle'
                       else ('--check', '--write', '--output'))
            for option in options:
                self.assertIn(option, result.stdout)

    def test_imports_are_inert_without_retail_in_normal_and_optimized_python(self):
        for optimized in (False, True):
            for module in PRODUCERS + HELPERS:
                with self.subTest(module=module, optimized=optimized):
                    self.check_lifecycle('import', module, optimized)

    def test_help_is_inert_without_retail_in_normal_and_optimized_python(self):
        for optimized in (False, True):
            for module in PRODUCERS:
                with self.subTest(module=module, optimized=optimized):
                    self.check_lifecycle('help', module, optimized)


if __name__ == '__main__':
    unittest.main()
