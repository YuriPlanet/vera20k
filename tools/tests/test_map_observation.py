"""Portable child receipts exercise the wrapper, without retail files or a GPU."""
from copy import deepcopy
import contextlib
import io
import json
from pathlib import Path
import tempfile
import shutil
import unittest
from unittest.mock import patch

from tools import map_observation as observation
from tools.child_process import ChildResult
from tools.tactical_certification.core import OutputExistsError, ValidationError, sha256_bytes
from tools.tactical_certification.profile import repository_contract_path


class MapObservationTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.profile = json.loads((observation.ROOT / 'tools/map_observation.example.json').read_text())
        self.profile.update(width=2, height=2, ticks=3, timeout_seconds=1)
        # Rust owns admission and real extent limits. These small synthetic frames
        # intentionally exercise only wrapper receipt/byte validation.
        self.profile_path = self.root / 'profile.json'
        self.profile_path.write_text(json.dumps(self.profile))
        self.config = self.root / 'config.toml'
        self.config.write_text('[paths]\nra2_dir="fixture"\n')
        self.executable = self.root / 'game'
        self.executable.write_bytes(b'fake executable identity')
        self.contract = repository_contract_path()
        self.output = self.root / 'observation'
        self.unit_atlas = {
            'resident_sprite_count': 7, 'last_build_rasterized_sprite_count': 23,
            'pages': [
                {'extent': [2, 3, 1], 'format': 'R8Uint', 'dimension': 'D2',
                 'mip_level_count': 1, 'sample_count': 1, 'texel_payload_bytes': 6},
                {'extent': [5, 2, 1], 'format': 'R8Uint', 'dimension': 'D2',
                 'mip_level_count': 1, 'sample_count': 1, 'texel_payload_bytes': 10},
            ],
            'total_texel_payload_bytes': 16,
        }
        self.frame = bytes(range(16))
        self.change = lambda manifest: None
        self.result = ChildResult(42, 0, False, b'child output\n', b'', ())
        environment = patch.dict('os.environ', {}, clear=True)
        environment.start()
        self.addCleanup(environment.stop)

    def fake_child(self, command, **kwargs):
        self.assertEqual(command[:3], [str(self.executable), '--tactical-capture', 'map-observe-v1'])
        self.assertEqual(kwargs['cwd'], self.root)
        self.assertEqual(kwargs['timeout_seconds'], 1)
        directory = Path(command[-1])
        directory.mkdir()
        frame = self.frame
        (directory / 'frame.bgra').write_bytes(frame)
        ticks = self.profile['ticks']
        fingerprint = lambda tick: {'simulation_tick': tick, 'binary_frame': tick,
                                   'total_simulation_ms': tick * 22,
                                   'deterministic_state_hash': 7 + tick}
        receipt = lambda tick: {'tick_before': tick, 'tick_after': tick + 1,
                               'binary_frame_before': tick, 'binary_frame_after': tick + 1}
        identity = lambda path: {'path': str(path), 'byte_length': path.stat().st_size,
                                 'sha256': sha256_bytes(path.read_bytes())}
        manifest = {
            'schema_version': 'vera20k.map-observation.v2', 'status': 'COMPLETE',
            'profile': {'sha256': sha256_bytes(self.profile_path.read_bytes()),
                        'request': deepcopy(self.profile)},
            'contract': {'sha256': sha256_bytes(self.contract.read_bytes())},
            'inputs': {'config': identity(self.config), 'executable': identity(self.executable)},
            'initial': fingerprint(0), 'final': fingerprint(ticks), 'exact_step_count': ticks,
            'first_exact_step': receipt(0) if ticks else None,
            'last_exact_step': receipt(ticks - 1) if ticks else None,
            'startup': {'seed': self.profile['seed'], 'seed_source': 'Controlled',
                        'seed_authority_certifying': True, 'correlation': 1,
                        'classification': 'AcceptedExplicitFixedBattle'},
            'map_source': {'kind': 'mix', 'logical_name': 'Fight.MAP', 'source_archive': 'maps.mix',
                           'entry_id': -10, 'payload_len': 90, 'source_sha256': 'a' * 64},
            'lifecycle': {'window_hidden': True, 'window_focused': False,
                          'focus_violations': 0, 'input_violations': 0},
            'render': {'ready': True, 'sidebar_view_present': True,
                       'surface_extent': [2, 2], 'internal_extent': [2, 2],
                       'unit_atlas': deepcopy(self.unit_atlas)},
            'frame': {'file_name': 'frame.bgra', 'width': 2, 'height': 2, 'row_stride': 8,
                      'byte_length': 16, 'sha256': sha256_bytes(frame),
                      'pixel_layout': 'BGRA8', 'surface_format': 'Bgra8UnormSrgb'},
            'native_comparator': 'NONE', 'parity_certification': 'NONE',
        }
        self.change(manifest)
        (directory / 'capture.json').write_text(json.dumps(manifest))
        return self.result

    def run_capture(self):
        with patch.object(observation, 'run_child', side_effect=self.fake_child):
            return observation.capture(profile_path=self.profile_path, contract_path=self.contract,
                                       output=self.output, working_directory=self.root,
                                       executable=self.executable)

    def test_complete_receipt_and_logs_are_bound_to_inputs(self):
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertEqual(report['capture']['exact_step_count'], 3)
        self.assertEqual(report['capture']['unit_atlas'], self.unit_atlas)
        self.assertEqual((self.output / 'stdout.log').read_bytes(), b'child output\n')
        self.assertEqual((self.output / 'profile.json').read_bytes(), self.profile_path.read_bytes())
        self.assertEqual(json.loads((self.output / 'run.json').read_text()), report)
        self.assertEqual(report['parity_certification'], 'NONE')
        self.assertEqual(report['schema_version'], observation.RUN_SCHEMA)
        self.assertEqual((self.output / 'config.toml').read_bytes(), self.config.read_bytes())
        self.assertEqual((self.output / 'contract.json').read_bytes(), self.contract.read_bytes())

    def test_empty_atlas_receipt_is_valid(self):
        self.unit_atlas.update(resident_sprite_count=0, last_build_rasterized_sprite_count=0,
                               pages=[], total_texel_payload_bytes=0)
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertEqual(report['capture']['unit_atlas'], self.unit_atlas)

    def test_atlas_statistics_are_required_by_v2(self):
        changes = [lambda m: m['render'].pop('unit_atlas'),
                   lambda m: m.update(schema_version='vera20k.map-observation.v1')]
        for value in (None, [], 1, 'statistics'):
            changes.append(lambda m, v=value: m['render'].update(unit_atlas=v))
        for key in self.unit_atlas:
            changes.append(lambda m, k=key: m['render']['unit_atlas'].pop(k))
        for index, change in enumerate(changes):
            with self.subTest(case=index):
                self.output = self.root / f'atlas-missing-{index}'
                self.change = change
                report = self.run_capture()
                self.assertEqual(report['status'], 'INVALID')
                self.assertIsNone(report['capture'])

    def test_atlas_counts_require_nonnegative_integers(self):
        for key in ('resident_sprite_count', 'last_build_rasterized_sprite_count',
                    'total_texel_payload_bytes'):
            for index, value in enumerate((-1, True, False, 1.0, '1', None)):
                with self.subTest(key=key, value=value):
                    self.output = self.root / f'atlas-count-{key}-{index}'
                    self.change = lambda m, k=key, v=value: m['render']['unit_atlas'].update({k: v})
                    self.assertEqual(self.run_capture()['status'], 'INVALID')

    def test_atlas_page_descriptor_and_payload_arithmetic_are_checked(self):
        cases = [('format', 'Rgba8Uint'), ('dimension', 'D3'),
                 ('mip_level_count', 2), ('mip_level_count', True),
                 ('sample_count', 4), ('sample_count', 1.0),
                 ('texel_payload_bytes', 7), ('texel_payload_bytes', 6.0),
                 ('texel_payload_bytes', True)]
        for extent in ([], [2, 3], [2, 3, 1, 1], '2,3,1', None,
                       [0, 3, 1], [2, -1, 1], [True, 3, 1], [2, False, 1],
                       [2.0, 3, 1], [2, '3', 1], [2, 3, 2], [2, 3, True], [2, 3, 1.0]):
            cases.append(('extent', extent))
        for index, (key, value) in enumerate(cases):
            with self.subTest(key=key, value=value):
                self.output = self.root / f'atlas-page-{index}'
                self.change = lambda m, k=key, v=value: m['render']['unit_atlas']['pages'][0].update({k: v})
                self.assertEqual(self.run_capture()['status'], 'INVALID')
        for index, key in enumerate(self.unit_atlas['pages'][0]):
            with self.subTest(missing=key):
                self.output = self.root / f'atlas-page-missing-{index}'
                self.change = lambda m, k=key: m['render']['unit_atlas']['pages'][0].pop(k)
                self.assertEqual(self.run_capture()['status'], 'INVALID')

    def test_atlas_pages_and_total_payload_are_checked(self):
        changes = [lambda m: m['render']['unit_atlas'].update(total_texel_payload_bytes=15)]
        for value in (None, {}, 'pages', [None], [[]], [1]):
            changes.append(lambda m, v=value: m['render']['unit_atlas'].update(pages=v))
        for index, change in enumerate(changes):
            with self.subTest(case=index):
                self.output = self.root / f'atlas-total-{index}'
                self.change = change
                self.assertEqual(self.run_capture()['status'], 'INVALID')

    def test_zero_steps_requires_unchanged_initial_state(self):
        self.profile['ticks'] = 0
        self.profile_path.write_text(json.dumps(self.profile))
        self.assertEqual(self.run_capture()['status'], 'VALID')

    def test_loose_map_receipt_is_supported(self):
        self.change = lambda m: m['map_source'].update(kind='loose', path='/retail/Fight.MAP')
        self.assertEqual(self.run_capture()['status'], 'VALID')

    def test_existing_output_is_never_reused(self):
        self.output.mkdir()
        with patch.object(observation, 'run_child') as child:
            with self.assertRaises(OutputExistsError):
                self.run_capture()
            child.assert_not_called()

    def test_changed_contract_cannot_remove_guards(self):
        path = self.root / 'contract.json'
        document = json.loads(self.contract.read_text())
        document['environment_denylist'] = []
        path.write_text(json.dumps(document))
        self.contract = path
        with self.assertRaisesRegex(ValidationError, 'denylist'):
            self.run_capture()
        self.assertFalse(self.output.exists())

    def test_denied_environment_rejected_without_mutation(self):
        with patch.dict('os.environ', {'RA2_DIR': '/unexpected'}):
            with self.assertRaisesRegex(ValidationError, 'denied'):
                self.run_capture()
        self.assertFalse(self.output.exists())

    def test_contract_timeout_maximum_is_enforced_before_spawn(self):
        self.profile['timeout_seconds'] = 100000
        self.profile_path.write_text(json.dumps(self.profile))
        with self.assertRaisesRegex(ValidationError, 'maximum'):
            self.run_capture()

    def test_release_resolution_has_no_guessed_target_fallback(self):
        with patch.object(observation, 'resolve_binary', return_value=(None, None)) as resolver:
            with self.assertRaisesRegex(ValidationError, 'verified release'):
                observation.capture(profile_path=self.profile_path, contract_path=self.contract,
                                    output=self.output, working_directory=self.root)
            resolver.assert_called_once_with(observation.ROOT, 'vera20k', 'release')

    def test_failed_child_keeps_diagnostics_and_cannot_pass_valid_receipt(self):
        for result in (ChildResult(42, 2, False, b'out', b'failed', ()),
                       ChildResult(42, -9, True, b'out', b'timed out', ('timeout',)),
                       ChildResult(None, None, False, b'', b'', ('spawn failed',))):
            with self.subTest(result=result):
                self.output = self.root / f'run-{result.pid}-{result.exit_status}'
                self.result = result
                report = self.run_capture()
                self.assertEqual(report['status'], 'INVALID')
                self.assertTrue(report['errors'])
                self.assertEqual((self.output / 'stderr.log').read_bytes(), result.stderr)

    def test_receipt_tampering_fails_closed(self):
        cases = [('profile', 'sha256', '0' * 64), ('contract', 'sha256', '0' * 64),
                 ('final', 'simulation_tick', 2), ('final', 'binary_frame', 4),
                 ('initial', 'simulation_tick', 1), ('last_exact_step', 'tick_before', 1),
                 ('first_exact_step', 'binary_frame_after', 2),
                 ('frame', 'sha256', '0' * 64), ('frame', 'byte_length', 15),
                 ('map_source', 'kind', 'generated'), ('map_source', 'source_sha256', 'bad'),
                 ('lifecycle', 'input_violations', 1), ('render', 'ready', False),
                 ('render', 'internal_extent', [2.0, 2]),
                 ('startup', 'seed_source', 'Random')]
        for index, (section, key, value) in enumerate(cases):
            with self.subTest(section=section, key=key):
                self.output = self.root / f'tamper-{index}'
                self.change = lambda m, s=section, k=key, v=value: m[s].update({k: v})
                self.assertEqual(self.run_capture()['status'], 'INVALID')

    def test_input_changed_during_child_cannot_pass(self):
        self.change = lambda m: self.config.write_text('changed')
        report = self.run_capture()
        self.assertEqual(report['status'], 'INVALID')
        self.assertTrue(any('changed' in error for error in report['errors']))

    def test_frame_corruption_cannot_pass(self):
        self.change = lambda m: (self.output / 'child-output/frame.bgra').write_bytes(b'bad')
        self.assertEqual(self.run_capture()['status'], 'INVALID')

    def test_failed_manifest_and_missing_outputs_are_invalid(self):
        self.change = lambda m: (m.update(status='FAILED', failure={'stage': 'loading', 'message': 'load failed'}),
                                 (self.output / 'child-output/frame.bgra').unlink())
        report = self.run_capture()
        self.assertEqual(report['status'], 'INVALID')
        self.assertTrue(any('load failed' in error and 'loading' in error for error in report['errors']))
        self.output = self.root / 'missing'
        with patch.object(observation, 'run_child', return_value=self.result):
            report = observation.capture(profile_path=self.profile_path, contract_path=self.contract,
                                         output=self.output, working_directory=self.root,
                                         executable=self.executable)
        self.assertEqual(report['status'], 'INVALID')

    def valid_capture(self, name):
        self.output = self.root / name
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        return self.output

    @staticmethod
    def edit_json(path, change):
        document = json.loads(path.read_text())
        change(document)
        path.write_text(json.dumps(document))

    def make_legacy(self, run):
        self.edit_json(run / 'run.json',
                       lambda report: report.update(schema_version=observation.LEGACY_RUN_SCHEMA))
        (run / 'config.toml').unlink()
        (run / 'contract.json').unlink()

    def test_sealed_run_revalidates_without_original_profile_config_or_contract(self):
        original_contract = self.root / 'original-contract.json'
        original_contract.write_bytes(self.contract.read_bytes())
        self.contract = original_contract
        run = self.valid_capture('sealed')
        for path in (self.profile_path, self.config, self.contract):
            path.unlink()
        report = observation.validate_run(run)
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertEqual(report['input_provenance'], {
            'profile': 'SEALED_COPY', 'config': 'SEALED_COPY', 'contract': 'SEALED_COPY',
            'executable': 'EXTERNALLY_REVALIDATED'})
        self.assertEqual(report['capture']['unit_atlas'], self.unit_atlas)

    def test_capture_detects_each_retained_copy_changed_by_child(self):
        for name, filename in observation.COPIES.items():
            with self.subTest(input=name):
                self.output = self.root / f'copy-changed-{name}'
                self.change = lambda m, f=filename: (self.output / f).write_bytes(b'changed copy')
                report = self.run_capture()
                self.assertEqual(report['status'], 'INVALID')
                self.assertTrue(any(f'{name} copy' in error for error in report['errors']))

    def test_offline_validator_reads_every_retained_artifact(self):
        artifacts = ('profile.json', 'config.toml', 'contract.json', 'stdout.log', 'stderr.log',
                     'child-output/capture.json', 'child-output/frame.bgra')
        for index, artifact in enumerate(artifacts):
            with self.subTest(artifact=artifact):
                run = self.valid_capture(f'tamper-artifact-{index}')
                path = run / artifact
                path.write_bytes(path.read_bytes() + b'changed')
                report = observation.validate_run(run)
                self.assertEqual(report['status'], 'INVALID')
                self.assertTrue(report['errors'])
        run = self.valid_capture('missing-frame')
        (run / 'child-output/frame.bgra').unlink()
        self.assertEqual(observation.validate_run(run)['status'], 'INVALID')

    def test_offline_validator_rechecks_child_semantics_after_manifest_rehash(self):
        run = self.valid_capture('semantic-corruption')
        manifest = run / 'child-output/capture.json'
        self.edit_json(manifest, lambda value: value['lifecycle'].update(input_violations=1))
        self.edit_json(run / 'run.json', lambda value: value['capture']['manifest'].update(
            byte_length=manifest.stat().st_size, sha256=sha256_bytes(manifest.read_bytes())))
        report = observation.validate_run(run)
        self.assertEqual(report['status'], 'INVALID')
        self.assertIn('input_violations', report['errors'][0])

    def test_offline_validator_rejects_receipt_inconsistency_and_bad_types(self):
        changes = [lambda r: r.update(status='INVALID'),
                   lambda r: r.update(errors=['capture failed']),
                   lambda r: r.update(schema_version='unknown'),
                   lambda r: r.update(native_comparator='NATIVE'),
                   lambda r: r['child'].update(exit_status=False),
                   lambda r: r['child'].update(timed_out=True),
                   lambda r: r['child'].update(pid=None),
                   lambda r: r['capture']['final'].update(deterministic_state_hash=10.0),
                   lambda r: r['capture']['unit_atlas']['pages'][0].update(sample_count=True),
                   lambda r: r['inputs']['config'].update(path='/wrong/config.toml'),
                   lambda r: r['inputs']['executable'].update(sha256='x' * 64),
                   lambda r: r['inputs']['profile'].pop('byte_length'),
                   lambda r: r.update(command=['some other command']),
                   lambda r: r.update(capture=None)]
        for index, change in enumerate(changes):
            with self.subTest(case=index):
                run = self.valid_capture(f'bad-receipt-{index}')
                self.edit_json(run / 'run.json', change)
                report = observation.validate_run(run)
                self.assertEqual(report['status'], 'INVALID')
                self.assertTrue(report['errors'])
        run = self.valid_capture('duplicate-json')
        (run / 'run.json').write_text('{"status":"VALID","status":"VALID"}')
        self.assertEqual(observation.validate_run(run)['status'], 'INVALID')

    def test_live_and_offline_nested_profile_types_are_strict(self):
        # bool is equal to integer 1 in Python, but not in the recorded launch DTO.
        self.change = lambda m: m['profile']['request']['launch']['options'].update(bases=1)
        report = self.run_capture()
        self.assertEqual(report['status'], 'INVALID')
        self.assertTrue(any('bases' in error for error in report['errors']))

    def test_legacy_requires_explicit_original_input_revalidation(self):
        run = self.valid_capture('legacy')
        self.make_legacy(run)
        rejected = observation.validate_run(run)
        self.assertEqual(rejected['status'], 'INVALID')
        self.assertIn('--allow-legacy-inputs', rejected['errors'][0])
        report = observation.validate_run(run, allow_legacy_inputs=True)
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertEqual(report['input_provenance']['config'], 'EXTERNALLY_REVALIDATED_UNSEALED')
        self.assertEqual(report['input_provenance']['contract'], 'EXTERNALLY_REVALIDATED_UNSEALED')
        self.assertEqual(report['input_provenance']['profile'], 'SEALED_COPY')
        self.config.write_text('changed after original capture')
        self.assertEqual(observation.validate_run(run, allow_legacy_inputs=True)['status'], 'INVALID')
        self.config.unlink()
        self.assertEqual(observation.validate_run(run, allow_legacy_inputs=True)['status'], 'INVALID')

    def test_legacy_contract_cannot_be_replaced_or_silently_resealed(self):
        original_contract = self.root / 'legacy-contract.json'
        original_contract.write_bytes(self.contract.read_bytes())
        self.contract = original_contract
        run = self.valid_capture('legacy-contract')
        self.make_legacy(run)
        self.contract.write_bytes(self.contract.read_bytes() + b'\n')
        report = observation.validate_run(run, allow_legacy_inputs=True)
        self.assertEqual(report['status'], 'INVALID')
        self.assertIn('contract', report['errors'][0])
        self.contract.unlink()
        self.assertEqual(observation.validate_run(run, allow_legacy_inputs=True)['status'], 'INVALID')

    def test_offline_contract_semantics_survive_consistent_rehashing(self):
        run = self.valid_capture('contract-guards')
        contract = run / 'contract.json'
        self.edit_json(contract, lambda value: value.update(environment_denylist=[]))
        digest = sha256_bytes(contract.read_bytes())
        self.edit_json(run / 'run.json', lambda value: value['inputs']['contract'].update(
            sha256=digest, byte_length=contract.stat().st_size))
        self.edit_json(run / 'child-output/capture.json',
                       lambda value: value['contract'].update(sha256=digest))
        report = observation.validate_run(run)
        self.assertEqual(report['status'], 'INVALID')
        self.assertIn('denylist', report['errors'][0])

    def test_original_executable_must_still_exist_and_match(self):
        run = self.valid_capture('binary-evidence')
        self.executable.write_bytes(b'different executable')
        self.assertEqual(observation.validate_run(run)['status'], 'INVALID')
        self.executable.unlink()
        self.assertEqual(observation.validate_run(run)['status'], 'INVALID')

    def test_comparison_matches_verified_different_binaries(self):
        before = self.valid_capture('before')
        self.executable = self.root / 'new-game'
        self.executable.write_bytes(b'new executable identity')
        after = self.valid_capture('after')
        report = observation.compare_runs(before, after)
        self.assertEqual(report['status'], 'MATCH', report['errors'])
        self.assertEqual(report['differences'], [])
        self.assertNotEqual(report['before']['inputs']['executable']['sha256'],
                            report['after']['inputs']['executable']['sha256'])
        self.assertEqual(report['native_comparator'], 'NONE')
        self.assertEqual(report['parity_certification'], 'NONE')

    def test_comparison_reports_meaningful_valid_differences(self):
        before = self.valid_capture('difference-before')
        changes = [('initial.deterministic_state_hash',
                    lambda m: m['initial'].update(deterministic_state_hash=8)),
                   ('final.deterministic_state_hash',
                    lambda m: m['final'].update(deterministic_state_hash=12)),
                   ('map_source.source_sha256',
                    lambda m: m['map_source'].update(source_sha256='b' * 64)),
                   ('unit_atlas.resident_sprite_count',
                    lambda m: m['render']['unit_atlas'].update(resident_sprite_count=8)),
                   ('frame.surface_format',
                    lambda m: m['frame'].update(surface_format='Bgra8Unorm'))]
        for index, (field, change) in enumerate(changes):
            with self.subTest(field=field):
                self.change = change
                after = self.valid_capture(f'difference-after-{index}')
                report = observation.compare_runs(before, after)
                self.assertEqual(report['status'], 'MISMATCH', report['errors'])
                self.assertEqual([d['field'] for d in report['differences']], [field])
        self.change = lambda m: None
        self.frame = bytes(reversed(self.frame))
        after = self.valid_capture('different-frame')
        report = observation.compare_runs(before, after)
        self.assertEqual(report['status'], 'MISMATCH', report['errors'])
        self.assertEqual([d['field'] for d in report['differences']], ['frame.bytes'])

    def test_comparison_requires_same_input_bytes(self):
        before = self.valid_capture('input-before')
        self.config.write_text('different production settings')
        after = self.valid_capture('config-after')
        report = observation.compare_runs(before, after)
        self.assertEqual(report['status'], 'INVALID')
        self.assertIn('config input bytes differ', report['errors'][0])
        self.config.write_text('[paths]\nra2_dir="fixture"\n')
        self.profile['ticks'] = 4
        self.profile_path.write_text(json.dumps(self.profile))
        after = self.valid_capture('profile-after')
        report = observation.compare_runs(before, after)
        self.assertEqual(report['status'], 'INVALID')
        self.assertIn('profile input bytes differ', report['errors'][0])

    def test_comparison_checks_frame_bytes_instead_of_copied_wrapper_hash(self):
        before = self.valid_capture('raw-before')
        after = self.valid_capture('raw-after')
        (after / 'child-output/frame.bgra').write_bytes(b'changed raw data!')
        report = observation.compare_runs(before, after)
        self.assertEqual(report['status'], 'INVALID')
        self.assertTrue(report['errors'])

    def test_comparison_rechecks_first_run_after_reading_second(self):
        before = self.valid_capture('race-before')
        after = self.valid_capture('race-after')
        load = observation._load_run

        def load_and_change(directory, allow_legacy):
            result = load(directory, allow_legacy)
            if directory == after:
                (before / 'config.toml').write_bytes(b'changed during comparison')
            return result

        with patch.object(observation, '_load_run', side_effect=load_and_change):
            report = observation.compare_runs(before, after)
        self.assertEqual(report['status'], 'INVALID')
        self.assertTrue(any('changed' in error for error in report['errors']))

    def test_same_alias_and_relocated_runs_are_rejected(self):
        run = self.valid_capture('location')
        report = observation.compare_runs(run, run / '.')
        self.assertEqual(report['status'], 'INVALID')
        self.assertIn('distinct', report['errors'][0])
        alias = self.root / 'alias'
        try:
            alias.symlink_to(run, target_is_directory=True)
        except OSError:
            pass  # Windows can require a privilege for symlink creation.
        else:
            self.assertEqual(observation.compare_runs(run, alias)['status'], 'INVALID')
        copied = self.root / 'copied'
        shutil.copytree(run, copied)
        report = observation.validate_run(copied)
        self.assertEqual(report['status'], 'INVALID')
        self.assertIn('run.command', report['errors'][0])

    def test_legacy_comparison_is_explicit_and_child_v1_remains_unsupported(self):
        before = self.valid_capture('legacy-before')
        after = self.valid_capture('legacy-after')
        self.make_legacy(before)
        self.assertEqual(observation.compare_runs(before, after)['status'], 'INVALID')
        report = observation.compare_runs(before, after, allow_legacy_inputs=True)
        self.assertEqual(report['status'], 'MATCH', report['errors'])
        self.edit_json(before / 'child-output/capture.json',
                       lambda m: m.update(schema_version='vera20k.map-observation.v1'))
        report = observation.compare_runs(before, after, allow_legacy_inputs=True)
        self.assertEqual(report['status'], 'INVALID')
        self.assertIn('schema_version', report['errors'][0])

    def test_labeled_capture_uses_shared_resolver_and_never_falls_back(self):
        with patch.object(observation, 'resolve_labeled_binary',
                          return_value=(self.executable, 'release')) as resolver, \
             patch.object(observation, 'resolve_binary') as latest, \
             patch.object(observation, 'run_child', side_effect=self.fake_child):
            report = observation.capture(profile_path=self.profile_path, contract_path=self.contract,
                                         output=self.output, working_directory=self.root,
                                         build_label='before-build')
        self.assertEqual(report['status'], 'VALID', report['errors'])
        resolver.assert_called_once_with(observation.ROOT, 'before-build', 'vera20k', 'release')
        latest.assert_not_called()
        with patch.object(observation, 'resolve_labeled_binary', return_value=(None, None)), \
             patch.object(observation, 'resolve_binary') as latest:
            with self.assertRaisesRegex(ValidationError, 'build label'):
                observation.capture(profile_path=self.profile_path, contract_path=self.contract,
                                    output=self.root / 'missing-label', working_directory=self.root,
                                    build_label='missing')
            latest.assert_not_called()
        with self.assertRaisesRegex(ValidationError, 'mutually exclusive'):
            observation.capture(profile_path=self.profile_path, contract_path=self.contract,
                                output=self.root / 'conflicting-selector', working_directory=self.root,
                                executable=self.executable, build_label='before-build')

    def test_cli_checks_have_distinct_verdicts_and_exclusive_outputs(self):
        before = self.valid_capture('cli-before')
        after = self.valid_capture('cli-after')
        output = self.root / 'comparison.json'
        arguments = ['compare', '--before', str(before), '--after', str(after),
                     '--output', str(output)]
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(observation.main(arguments), 0)
            self.assertEqual(json.loads(output.read_text())['status'], 'MATCH')
            previous_bytes = output.read_bytes()
            self.assertEqual(observation.main(arguments), 2)
            self.assertEqual(output.read_bytes(), previous_bytes)
            self.frame = b'changed frame!!!'
            changed = self.valid_capture('cli-changed')
            self.assertEqual(observation.main(['compare', '--before', str(before),
                                               '--after', str(changed), '--output',
                                               str(self.root / 'mismatch.json')]), 1)
            self.assertEqual(observation.main(['validate', '--run', str(before), '--output',
                                               str(self.root / 'validation.json')]), 0)
            (after / 'child-output/frame.bgra').unlink()
            self.assertEqual(observation.main(['validate', '--run', str(after), '--output',
                                               str(self.root / 'invalid.json')]), 2)
            self.assertEqual(observation.main(['validate', '--run', str(before), '--output',
                                               str(before / 'forbidden-report.json')]), 2)
            self.assertFalse((before / 'forbidden-report.json').exists())


if __name__ == '__main__':
    unittest.main()
