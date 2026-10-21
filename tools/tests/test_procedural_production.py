"""Native/production comparison boundaries, using the retained real captures."""

import copy
import gzip
import json
from pathlib import Path, PurePosixPath, PureWindowsPath
import tempfile
import unittest
from unittest.mock import patch

from tools.procedural_drawing_oracle import rally_production as production


def archived_captures(archive, names):
    captures, hashes = [], []
    for name in names:
        raw = gzip.decompress((archive / name / 'child-output/capture.json.gz').read_bytes())
        pixels = gzip.decompress((archive / name / 'child-output/frame.bgra.gz').read_bytes())
        captures.append((json.loads(raw), pixels))
        hashes.append(production.digest(raw))
    return captures, hashes


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


class UnitMoveProductionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        archive = Path(__file__).resolve().parents[1] / 'procedural_drawing_oracle' / 'unit-move-production-validation'
        cls.cases = production.action_cases()
        cls.captures, cls.hashes = archived_captures(archive, cls.cases)
        cls.receipt = production.document(archive / 'receipt.json')
        cls.final_captures, cls.final_hashes = [], []
        if cls.receipt.get('final_candidate'):
            candidate = archive / cls.receipt['final_candidate_run']
            cls.final_captures, cls.final_hashes = archived_captures(candidate, cls.cases)

    def test_retained_native_stores_and_real_input_controls(self):
        self.assertEqual(production.compare_family('unit-move',
                                                  self.captures + self.final_captures,
                                                  self.hashes + self.final_hashes),
                         self.receipt['comparison'])

    def test_original_pixel_mismatch_cannot_pass_on_receipt_hashes_alone(self):
        cases = copy.deepcopy(self.cases)
        cases['unit-move-active-v1']['pixels'][0][2] ^= 1
        with patch.object(production, 'action_cases', return_value=cases):
            with self.assertRaisesRegex(ValueError, 'original opaque store differs'):
                production.compare_action(self.captures, self.hashes)

    def test_captured_source_must_match_original_input(self):
        captures = copy.deepcopy(self.captures)
        index = list(self.cases).index('unit-move-active-v1')
        production.action_actor(captures[index][0]['observations']['frames'][-1])['physical_leptons'][0] += 256
        with self.assertRaisesRegex(ValueError, 'native source differs'):
            production.compare_action(captures, self.hashes)

    def test_native_identity_binds_covered_rows_independently_of_other_mechanisms(self):
        expected = production.action_case_set_digest(self.cases)
        cases = copy.deepcopy(self.cases)
        cases['unit-move-active-v1']['pixels'][0][2] ^= 1
        self.assertNotEqual(production.action_case_set_digest(cases), expected)

    def test_unrelated_observed_consumer_change_is_not_a_line_difference(self):
        captures = copy.deepcopy(self.captures)
        index = list(self.cases).index('unit-move-reselect-v1')
        production.action_actor(captures[index][0]['observations']['frames'][-1], 1375)['health'] -= 1
        with self.assertRaisesRegex(ValueError, 'unexplained state delta'):
            production.compare_action(captures, self.hashes)

    def test_final_candidate_rechecks_all_pixels_and_observed_boundaries(self):
        captures = self.captures + copy.deepcopy(self.captures)
        hashes = self.hashes * 2
        result = production.compare_family('unit-move', captures, hashes)
        self.assertEqual(len(result['final_candidate']), 12)
        candidate, pixels = captures[12]
        changed = bytearray(pixels)
        changed[0] ^= 1
        captures[12] = candidate, bytes(changed)
        with self.assertRaisesRegex(ValueError, 'candidate frame differs'):
            production.compare_family('unit-move', captures, hashes)

    def test_final_candidate_cannot_hide_a_state_change_behind_equal_pixels(self):
        candidates = copy.deepcopy(self.captures)
        production.action_actor(candidates[0][0]['observations']['frames'][-1], 1375)['health'] -= 1
        with self.assertRaisesRegex(ValueError, 'candidate observations differs'):
            production.compare_family('unit-move', self.captures + candidates, self.hashes * 2)


class UnitAttackProductionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        archive = Path(__file__).resolve().parents[1] / 'procedural_drawing_oracle' / 'unit-attack-production-validation'
        cls.cases = production.attack_cases()
        cls.names = tuple(cls.cases) + (production.ATTACK_OBSERVER_OFF_RUN,)
        cls.captures, cls.hashes = archived_captures(archive, cls.names)
        cls.receipt = production.document(archive / 'receipt.json')
        cls.final_captures, cls.final_hashes = [], []
        if cls.receipt.get('final_candidate'):
            cls.final_captures, cls.final_hashes = archived_captures(
                archive / cls.receipt['final_candidate_run'], cls.names)

    def test_retained_native_stores_live_getters_and_observer_control(self):
        self.assertEqual(production.compare_family(
            'unit-attack', self.captures + self.final_captures,
            self.hashes + self.final_hashes), self.receipt['comparison'])

    def test_native_pixel_mismatch_cannot_pass_on_capture_identity(self):
        cases = copy.deepcopy(self.cases)
        cases['moving-attack-inputs-v1']['pixels'][0][2] ^= 1
        with patch.object(production, 'attack_cases', return_value=cases):
            with self.assertRaisesRegex(ValueError, 'original opaque store differs'):
                production.compare_attack(self.captures, self.hashes)

    def test_native_heading_must_bind_to_actual_live_target(self):
        cases = copy.deepcopy(self.cases)
        cases['moving-attack-inputs-v1']['input']['target_facing'] ^= 16384
        with patch.object(production, 'attack_cases', return_value=cases):
            with self.assertRaisesRegex(ValueError, 'native target_facing differs'):
                production.compare_attack(self.captures, self.hashes)

    def test_prepared_getter_result_must_match_observed_speed(self):
        cases = copy.deepcopy(self.cases)
        cases['moving-attack-inputs-v1']['captured_getter_checks']['target_current_speed'] = 0
        with patch.object(production, 'attack_cases', return_value=cases):
            with self.assertRaisesRegex(ValueError, 'original live getters differ'):
                production.compare_attack(self.captures, self.hashes)

    def test_matched_control_cannot_change_unrelated_gameplay(self):
        captures = copy.deepcopy(self.captures)
        index = self.names.index('moving-enemy-band-control-inputs-v1')
        production.action_actor(captures[index][0]['observations']['frames'][100])['health'] -= 1
        with self.assertRaisesRegex(ValueError, 'unexplained state delta at step100'):
            production.compare_attack(captures, self.hashes)

    def test_observer_control_checks_each_boundary_not_only_final_hash(self):
        captures = copy.deepcopy(self.captures)
        production.action_actor(captures[-1][0]['observations']['frames'][100])['health'] -= 1
        with self.assertRaisesRegex(ValueError, 'input observer changed boundary100'):
            production.compare_attack(captures, self.hashes)

    def test_observer_control_rejects_one_changed_pixel(self):
        captures = list(self.captures)
        capture, pixels = captures[-1]
        changed = bytearray(pixels)
        changed[0] ^= 1
        captures[-1] = capture, bytes(changed)
        with self.assertRaisesRegex(ValueError, 'input observer changed frame pixels'):
            production.compare_attack(captures, self.hashes)


if __name__ == '__main__':
    unittest.main()
