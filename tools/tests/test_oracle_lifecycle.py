"""Oracle imports and help must never execute native code or publish references."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
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
     'tools.spatial_oracle.fly_landing_phase',
     'tools.spatial_oracle.magnetron_aircraft_lift',
     'tools.spatial_oracle.foot_path_window',
     'tools.spatial_oracle.captured_building_sale',
     'tools.procedural_drawing_oracle.airstrike_beam')
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
operation, module, *arguments = sys.argv[1:]
with ExitStack() as stack:
    guards = [stack.enter_context(patch.object(owner, name,
              side_effect=RuntimeError('forbidden lifecycle operation: ' + name)))
              for owner, name in ((native_oracle, 'image_bytes'),
                                  (native_oracle, 'run_checked'),
                                  (Path, 'write_text'), (Path, 'write_bytes'))]
    if operation == 'retired-cache' or module == 'tools.spatial_oracle.trigger_action_values':
        import pickle
        guards.extend(stack.enter_context(patch.object(owner, name,
                      side_effect=RuntimeError('forbidden cache operation: ' + name)))
                      for owner, name in ((native_oracle, 'configured_gamemd'),
                                          (native_oracle, 'image_sha256'),
                                          (Path, 'read_bytes'),
                                          (pickle, 'load'), (pickle, 'loads')))
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
        sys.argv = [module, *(arguments or ['--help'])]
        try:
            runpy.run_module(module, run_name='__main__')
        except SystemExit as error:
            allowed = (2,) if operation == 'retired-cache' else (None, 0)
            if error.code not in allowed:
                raise
        else:
            if operation == 'retired-cache':
                raise AssertionError('retired cache setting must be rejected')
    for guard in guards:
        guard.assert_not_called()
'''


class OracleLifecycleTests(unittest.TestCase):
    def check_lifecycle(self, operation, module, optimized, *, arguments=(), environment=None):
        command = [sys.executable] + (['-O'] if optimized else [])
        child_environment = absent_retail_environment()
        child_environment.update(environment or {})
        result = subprocess.run(
            command + ['-c', GUARDED_LIFECYCLE, operation, module, *arguments],
            cwd=ROOT, env=child_environment, capture_output=True,
            text=True, timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        if operation == 'help':
            options = (('--font', '--check', '--write')
                       if module == 'tools.control_group_text_oracle'
                       else ('--check', '--write', '--output'))
            for option in options:
                self.assertIn(option, result.stdout)
        return result

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

    def test_explosion_rejects_retired_cache_before_inputs_deserialization_or_publication(self):
        module = 'tools.spatial_oracle.trigger_action_values'
        with tempfile.TemporaryDirectory() as directory:
            checkpoint = Path(directory) / 'untrusted.pickle'
            original = b'untrusted checkpoint must not be read'
            checkpoint.write_bytes(original)
            for optimized in (False, True):
                for option in ('--check', '--write', '--wr', '--wri'):
                    with self.subTest(optimized=optimized, option=option):
                        result = self.check_lifecycle(
                            'retired-cache', module, optimized,
                            arguments=('--do-explosion', option),
                            environment={'VERA20K_NATIVE_STATE_CACHE': directory},
                        )
                        self.assertIn('VERA20K_NATIVE_STATE_CACHE is no longer supported',
                                      result.stderr)
                        self.assertEqual(checkpoint.read_bytes(), original)

    def test_explosion_help_without_cache_never_reads_inputs_or_writes(self):
        for optimized in (False, True):
            with self.subTest(optimized=optimized):
                self.check_lifecycle(
                    'help', 'tools.spatial_oracle.trigger_action_values', optimized,
                    arguments=('--do-explosion', '--help'),
                    environment={'VERA20K_NATIVE_STATE_CACHE': ''},
                )


if __name__ == '__main__':
    unittest.main()
