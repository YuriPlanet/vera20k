"""Native/production comparison boundaries."""

import copy
import json
from pathlib import Path, PurePosixPath, PureWindowsPath
import tempfile
import unittest
from unittest.mock import patch

from tools.procedural_drawing_oracle import rally_production as production
from tools.procedural_drawing_oracle import rally


class StockTypeInputTests(unittest.TestCase):
    def test_physical_input_paths_have_the_same_spelling_on_every_host(self):
        files = {'rulesmd.ini': b'[GAPILE]\r\nFactory=InfantryType\r\n',
                 'artmd.ini': b'[GAPILE]\r\nFoundation=3x2\r\n'}
        for flavor in (PurePosixPath, PureWindowsPath):
            with self.subTest(flavor=flavor.__name__):
                class FixturePath(flavor):
                    def read_bytes(self):
                        return files[self.name]

                with patch.object(rally, 'Path', FixturePath):
                    inputs = rally.stock_type_physical()
                for kind, name in (('rules', 'rulesmd.ini'), ('art', 'artmd.ini')):
                    self.assertEqual(inputs[kind]['path'], f'ini/{name}')
                    self.assertEqual(inputs[kind]['bytes'], len(files[name]))
                    self.assertEqual(inputs[kind]['sha256'], production.digest(files[name]))
                self.assertEqual(inputs['rules']['keys'], {
                    'Image': None, 'Factory': 'InfantryType',
                    'UnitRepair': None, 'Cloning': None})
                self.assertEqual(inputs['art']['keys'], {'Foundation': '3x2'})


class RetainedEvidenceTests(unittest.TestCase):
    # Sample homes are joined at run time, because tools/tests/test_no_home_paths.py
    # rejects a home directory spelled out in a tracked file.
    def test_retained_text_spells_the_home_directory_as_tilde(self):
        home = PurePosixPath('/Users', 'someone')
        self.assertEqual(production.without_home(f'{{"cwd":"{home}","path":"{home}/x"}}'.encode(), home),
                         b'{"cwd":"~","path":"~/x"}')
        for kept in (f'{home}2/x', f'{home}.else/x', f'{PurePosixPath("/home", home.name)}/x'):
            self.assertEqual(production.without_home(kept.encode(), home), kept.encode())
        accented = PurePosixPath('/home', '\u00e9mile')
        self.assertEqual(production.without_home(f'{accented}/x'.encode(), accented), b'~/x')
        self.assertEqual(production.without_home(json.dumps(f'{accented}/x').encode(), accented),
                         b'"~/x"')
        self.assertEqual(production.without_home(b'/x/y', PurePosixPath('/')), b'/x/y')

    def test_windows_spellings_of_the_home_directory_are_retained_as_tilde(self):
        home = PureWindowsPath('C:/', 'Users', 'Some One')
        native, escaped = f'{home}\\a', json.dumps(f'{home}\\b')
        text = ' '.join((native, escaped, f'{home.as_posix().lower()}/c')).encode()
        self.assertEqual(production.without_home(text, home), b'~\\a "~\\\\b" ~/c')

    def test_retained_identities_name_the_retained_bytes(self):
        home = Path.home()
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory)
            (run / 'child-output').mkdir()
            for name, text in (('config.toml', f'ra2_dir = "{home / "ra2"}"\n'), ('stderr.log', f'{home / "x"}\n'),
                               ('stdout.log', ''), ('profile.json', '{}'), ('contract.json', '{}')):
                (run / name).write_text(text)
            frame = str(home / 'pixels').encode()
            (run / 'child-output/frame.bgra').write_bytes(frame)

            def write(name, document):
                (run / name).write_text(json.dumps(document, separators=(',', ':')))

            def identity(name):
                raw = (run / name).read_bytes()
                return {'byte_length': len(raw), 'path': str(run / name), 'sha256': production.digest(raw)}

            write('child-output/capture.json', {'inputs': {'config': identity('config.toml')},
                                                'frame': identity('child-output/frame.bgra')})
            write('run.json', {'inputs': {'config': identity('config.toml')},
                               'logs': {'stderr.log': identity('stderr.log')},
                               'capture': {'manifest': identity('child-output/capture.json')}})
            kept = production.retained_run(run, {})
        capture, report = json.loads(kept['child-output/capture.json']), json.loads(kept['run.json'])
        for pin, name in ((capture['inputs']['config'], 'config.toml'), (report['inputs']['config'], 'config.toml'),
                          (report['logs']['stderr.log'], 'stderr.log'),
                          (report['capture']['manifest'], 'child-output/capture.json')):
            self.assertEqual((pin['byte_length'], pin['sha256']), (len(kept[name]), production.digest(kept[name])))
        self.assertEqual(kept['child-output/frame.bgra'], frame)
        if len(home.parts) > 1:
            self.assertEqual([name for name, raw in kept.items()
                              if str(home).encode() in raw and not name.endswith('.bgra')], [])


class ActionCaseIdentityTests(unittest.TestCase):
    def test_native_identity_binds_covered_rows_independently_of_other_mechanisms(self):
        cases = production.action_cases()
        expected = production.action_case_set_digest(cases)
        cases = copy.deepcopy(cases)
        cases['unit-move-active-v1']['pixels'][0][2] ^= 1
        self.assertNotEqual(production.action_case_set_digest(cases), expected)


if __name__ == '__main__':
    unittest.main()
