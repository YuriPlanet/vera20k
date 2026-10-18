"""The replay command a native run records."""
from pathlib import Path
import os
import shlex
import subprocess
from types import ModuleType, SimpleNamespace
import unittest

from tools import oracle_sources as sources


def main_module(name=None, file=None):
    module = ModuleType('__main__')
    module.__spec__ = SimpleNamespace(name=name) if name else None
    if file:
        module.__file__ = file
    return module


class ReplayCommandTests(unittest.TestCase):
    def test_module_run_records_its_arguments_without_mode_or_output(self):
        module = main_module('tools.spatial_oracle.building_guard_attack')
        for argv in (['--gate', '--check', '--output', 'out dir/x.json'],
                     ['--output=/tmp/x.json', '--gate', '--write']):
            command = sources.replay_command(argv, module)
            self.assertEqual(command, 'python -m tools.spatial_oracle.building_guard_attack --gate')
        self.assertEqual(sources.replay_command(['--label', 'a b'], module),
                         "python -m tools.spatial_oracle.building_guard_attack --label 'a b'")

    def test_checkout_paths_are_relative_to_the_root_a_replay_runs_from(self):
        module = main_module('tools.spatial_oracle.building_guard_attack')
        relative = str(Path('tools', 'fixture.json'))
        outside = str(Path(sources.REPO.anchor) / 'elsewhere.json')
        nested = f'{Path("/mnt")}{sources.REPO / relative}'
        argv = ['--input', str(sources.REPO / relative), f'--map={sources.REPO / relative}', outside, nested]
        self.assertEqual(sources.replay_command(argv, module),
                         shlex.join(['python', '-m', 'tools.spatial_oracle.building_guard_attack',
                                     '--input', relative, f'--map={relative}', outside, nested]))

    @unittest.skipUnless(os.name == 'posix', 'recorded commands use POSIX shell quoting')
    def test_external_home_paths_replay_as_exact_literal_arguments(self):
        home = Path.home()
        path = str(home / "replay maps" / "map 'quoted' $literal.json")
        other = str(home / 'second map.json')
        argv = ['--input', path, f'--map={path}', f'--maps={path},{other}',
                '--empty', '', '--literal', '$literal']
        module = main_module('tools.synthetic')
        command = sources.replay_command(argv, module)
        self.assertNotIn(str(home), command)
        arguments = command.removeprefix('python -m tools.synthetic ')
        actual = subprocess.run(['sh', '-c', 'printf "%s\\0" ' + arguments],
                                check=True, capture_output=True).stdout
        self.assertEqual(actual.decode().split('\0')[:-1], argv)

    def test_script_runs_become_module_runs_and_other_mains_are_not_recorded(self):
        script = str(sources.REPO / 'tools' / 'render_depth_oracle.py')
        self.assertEqual(sources.replay_command(['--write'], main_module(file=script)),
                         'python -m tools.render_depth_oracle')
        self.assertEqual(sources.replay_command([], main_module('tools.anim_oracle.__main__')),
                         'python -m tools.anim_oracle')
        self.assertIsNone(sources.replay_command([], main_module('unittest')))
        self.assertIsNone(sources.replay_command([], main_module(file='/elsewhere/tool.py')))

    def test_the_replay_command_and_a_legacy_source_pin_are_not_evidence(self):
        metadata = {'native_sha256': 'n', 'payload_sha256': 'p', sources.REPLAY_FIELD: 'python -m tools.x',
                    sources.LEGACY_SOURCE_FIELD: {'tools/x.py': '0' * 64}}
        self.assertEqual(sources.evidence(metadata), {'native_sha256': 'n', 'payload_sha256': 'p'})


if __name__ == '__main__':
    unittest.main()
