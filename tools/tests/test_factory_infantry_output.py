"""Guards on historical/current publication evidence, without native replay."""
import copy
import json
import unittest
from unittest import mock

from tools.spatial_oracle._factory_infantry_output import fixture, runtime as rt, saved
from tools import native_oracle as native
from tools.spatial_oracle import factory_infantry_output as public


class ReplayImageAdmissionTests(unittest.TestCase):
    def test_default_helper_admission_rejects_a_changed_inspection_owner(self):
        metadata = copy.deepcopy(rt.metadata())
        owner = next(iter(metadata['inspection_owner_files'].values()))
        owner['sha256'] = '0' * 64
        with mock.patch.object(rt, 'metadata', return_value=metadata):
            with self.assertRaisesRegex(ValueError, 'Unsupported inspection-owner version'):
                rt.verify_helpers()

    def test_direct_consumer_and_gate_replays_reject_another_supported_file(self):
        for control in ('unit_ready_consumer', 'infantry_unlimbo_gate'):
            with self.subTest(control=control), \
                    mock.patch.object(native, 'image_sha256', return_value=native.STEAM_NATIVE_SHA256), \
                    mock.patch('sys.argv', ['factory', '--replay', control, '--output', 'unused-steam-replay.json']):
                with self.assertRaisesRegex(ValueError, 'recorded executable identity'):
                    public.main()


class CandidateSourceAdmissionTests(unittest.TestCase):
    """Candidate execution never changes the registered source authority."""

    def candidate_metadata(self):
        meta = copy.deepcopy(rt.metadata())
        self.assertNotIn('unregistered-initialized-candidate', meta['helper_profiles'])
        return meta

    def assert_registered_maps_preserved(self, meta, callers, profiles):
        self.assertEqual(meta['caller_files'], callers)
        self.assertEqual(meta['helper_profiles'], profiles)

    def test_reject_unselected_caller_before_admission(self):
        meta = self.candidate_metadata()
        callers, profiles = copy.deepcopy(meta['caller_files']), copy.deepcopy(meta['helper_profiles'])
        meta['initialized_candidate_caller_files']['phase.py'] = meta['caller_files']['phase.py']
        with mock.patch.object(rt, 'metadata', return_value=meta):
            with self.assertRaisesRegex(ValueError, 'Unsupported candidate caller'):
                with rt.candidate_helper_profile():
                    self.fail('Unselected caller was admitted')
        self.assert_registered_maps_preserved(meta, callers, profiles)

    def test_reject_changed_candidate_caller_bytes(self):
        meta = self.candidate_metadata()
        callers, profiles = copy.deepcopy(meta['caller_files']), copy.deepcopy(meta['helper_profiles'])
        meta['initialized_candidate_caller_files']['initialized.py']['sha256'] = '0' * 64
        with mock.patch.object(rt, 'metadata', return_value=meta):
            with self.assertRaisesRegex(ValueError, 'Explicit initialized candidate caller bytes changed'):
                with rt.candidate_helper_profile():
                    self.fail('Changed caller bytes were admitted')
        self.assert_registered_maps_preserved(meta, callers, profiles)

    def test_reject_changed_candidate_helper_bytes(self):
        meta = self.candidate_metadata()
        callers, profiles = copy.deepcopy(meta['caller_files']), copy.deepcopy(meta['helper_profiles'])
        name = next(iter(meta['initialized_candidate_helper_files']))
        meta['initialized_candidate_helper_files'][name] = '0' * 64
        with mock.patch.object(rt, 'metadata', return_value=meta):
            with self.assertRaisesRegex(ValueError, 'Explicit initialized candidate helper bytes changed'):
                with rt.candidate_helper_profile():
                    self.fail('Changed helper bytes were admitted')
        self.assert_registered_maps_preserved(meta, callers, profiles)

    def test_restore_registered_maps_after_native_comparison_failure(self):
        meta = self.candidate_metadata()
        callers, profiles = copy.deepcopy(meta['caller_files']), copy.deepcopy(meta['helper_profiles'])
        # This exercises context restoration after a comparison fails, not
        # registration/replay of today's shared helpers against native controls.
        candidate = {'profile_sha256': rt.canonical_sha(meta['initialized_candidate_helper_files'])}
        with mock.patch.object(rt, 'metadata', return_value=meta), \
                mock.patch.object(rt, 'verify_helpers', return_value=candidate):
            with self.assertRaisesRegex(RuntimeError, 'native comparison failed'):
                with rt.candidate_helper_profile() as profile:
                    self.assertEqual(profile['profile_sha256'],
                                     rt.canonical_sha(meta['initialized_candidate_helper_files']))
                    self.assertEqual(meta['caller_files']['initialized.py'],
                                     meta['initialized_candidate_caller_files']['initialized.py'])
                    raise RuntimeError('native comparison failed')
        self.assert_registered_maps_preserved(meta, callers, profiles)

    def test_restore_registered_maps_after_shared_census_failure(self):
        meta = self.candidate_metadata()
        callers, profiles = copy.deepcopy(meta['caller_files']), copy.deepcopy(meta['helper_profiles'])
        with mock.patch.object(rt, 'metadata', return_value=meta), \
                mock.patch.object(rt, 'verify_helpers', side_effect=ValueError('candidate census refusal')):
            with self.assertRaisesRegex(ValueError, 'candidate census refusal'):
                with rt.candidate_helper_profile():
                    self.fail('Refused source census was admitted')
        self.assert_registered_maps_preserved(meta, callers, profiles)


class PublicationPhaseEvidenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.meta = rt.metadata()['publication_phase']
        cls.replays = {order: rt.read_pinned(relative)
                       for order, relative in cls.meta['compatibility_receipts'].items()}
        # Both groups are saved executions. 'current' is the compatibility
        # execution at capture time, not an attestation of today's helper bytes.
        cls.receipts = {
            'historical': {order: rt.read_pinned(row['receipt'])
                           for order, row in cls.meta['controls'].items()},
            'current': {order: replay['full_original_publication_control']
                        for order, replay in cls.replays.items()},
        }

    def setUp(self):
        # Verify the retained source claim against its sealed registration,
        # then isolate saved-witness validation from current-source admission.
        # Production verify_helpers remains strict; no profile is registered.
        claim = self.receipts['current']['before_strip']['shared_helpers']
        metadata = rt.metadata()
        profile = metadata['helper_profiles'][claim['profile']]
        inspections = metadata['inspection_owner_files']
        expected = dict(profile=claim['profile'], imported_owner_files=len(profile['files']),
                        profile_sha256=rt.canonical_sha(profile['files']), status=profile['status'],
                        inspection_owner_files=len(inspections),
                        inspection_owner_sha256=rt.canonical_sha({p: row['sha256'] for p, row in inspections.items()}))
        for receipt in self.receipts['current'].values():
            self.assertEqual(receipt['shared_helpers'], expected)
        patcher = mock.patch.object(rt, 'verify_helpers', return_value=expected)
        patcher.start()
        self.addCleanup(patcher.stop)

    def control_receipts(self, *branches):
        # Copy only branches a rejection control mutates. The complete warmed
        # native rounds stay shared and are never written.
        for origin, controls in self.receipts.items():
            for order, original in controls.items():
                receipt = dict(original)
                for name in branches:
                    receipt[name] = copy.deepcopy(original[name])
                yield origin, order, receipt

    def test_both_complete_original_receipts_pass(self):
        self.assertEqual(set(self.replays), set(self.meta['controls']))
        for origin, order, receipt in self.control_receipts():
            with self.subTest(origin=origin, order=order):
                result = saved.compare_publication_phase(
                    order, receipt, historical=origin == 'historical')
                self.assertTrue(result['complete_original_observations_equal'])
                self.assertEqual(result['complete_warmed_prefix_rounds'], 216)
                self.assertEqual(result['complete_normalized_original_sha256'],
                                 self.meta['controls'][order]['complete_normalized_original_sha256'])
                if origin == 'current':
                    replay = self.replays[order]
                    self.assertEqual(replay['status'], 'PASS')
                    self.assertEqual(replay['control'], 'publication_' + order)
                    self.assertEqual(result, replay['comparison'])

    def test_fixture_preserves_historical_serialization_and_current_selection(self):
        raw = (rt.REPO_ROOT / self.meta['rust_fixture']['path']).read_bytes()
        for origin, receipts in self.receipts.items():
            with self.subTest(origin=origin):
                selected = fixture.publication_phase_local_fixture(
                    receipts, self.meta['local_sources'], self.meta['parent_manifest_sha256'])
                self.assertEqual(json.loads(raw), selected)
                if origin == 'historical':
                    self.assertEqual(raw, json.dumps(selected, indent=2).encode())
                before, after = (selected['cases'][name] for name in ('before_strip', 'after_strip'))
                self.assertEqual(before['executed_event_types'], [30, 11])
                self.assertEqual(after['executed_event_types'], [11, 30])
                self.assertTrue(before['placed']['archive_is_set'])
                self.assertFalse(after['placed']['archive_is_set'])

    def test_reject_receipt_relabelled_to_the_other_source_origin(self):
        for origin, order, receipt in self.control_receipts():
            with self.subTest(origin=origin, order=order):
                with self.assertRaisesRegex(ValueError, 'Publication source driver differs'):
                    saved.compare_publication_phase(
                        order, receipt, historical=origin != 'historical')

    def test_reject_changed_source_claims_before_complete_comparison(self):
        claims = (
            (None, 'driver_sha256', 'Publication source driver differs'),
            ('caller_adaptation', 'original_owner_sha256', 'Publication initialized caller identity differs'),
            ('caller_adaptation', 'derived_caller_ast_sha256', 'Publication caller adaptation differs'),
            ('shared_helpers', 'profile_sha256', 'Publication shared-helper identity differs'),
            ('full_original_initialized_first_place', 'driver_sha256',
             'Publication embedded initialized source identity differs'),
            ('full_original_initialized_first_place', 'shared_helpers',
             'Publication embedded initialized source identity differs'),
        )
        for origin, order, original in self.control_receipts():
            for branch, name, error in claims:
                with self.subTest(origin=origin, order=order, branch=branch, field=name):
                    receipt = dict(original)
                    parent = receipt
                    if branch is not None:
                        parent = receipt[branch] = dict(original[branch])
                    value = parent[name]
                    parent[name] = (dict(value, profile_sha256='0' * 64)
                                    if isinstance(value, dict) else '0' * 64)
                    with self.assertRaisesRegex(ValueError, error):
                        saved.compare_publication_phase(
                            order, receipt, historical=origin == 'historical')

    def test_reject_native_code_change(self):
        for origin, order, receipt in self.control_receipts('control_state'):
            with self.subTest(origin=origin, order=order):
                receipt['control_state']['native_code_unchanged'] = False
                with self.assertRaisesRegex(ValueError, 'Original publication code changed'):
                    saved.validate_publication_phase_receipt(
                        receipt, self.meta['controls'][order], historical=origin == 'historical')

    def test_reject_truncated_complete_rng_buffer(self):
        for origin, order, receipt in self.control_receipts('complete_rng_buffers'):
            with self.subTest(origin=origin, order=order):
                row = next(iter(receipt['complete_rng_buffers'].values()))
                row['bytes'] = row['bytes'][:-2]
                with self.assertRaisesRegex(ValueError, 'Incomplete publication RNG buffer'):
                    saved.validate_publication_phase_receipt(
                        receipt, self.meta['controls'][order], historical=origin == 'historical')

    def test_reject_unwitnessed_strip_return(self):
        for origin, order, receipt in self.control_receipts('original_calls'):
            with self.subTest(origin=origin, order=order):
                row = next(row for row in receipt['original_calls'] if row['kind'] == 'factory_take_changed')
                del row['result']
                with self.assertRaisesRegex(ValueError, 'takeChanged return/write witness missing'):
                    saved.validate_publication_phase_receipt(
                        receipt, self.meta['controls'][order], historical=origin == 'historical')

    def test_reject_reversed_actual_event_order(self):
        for origin, order, receipt in self.control_receipts('original_calls'):
            with self.subTest(origin=origin, order=order):
                rows = [row for row in receipt['original_calls'] if row['kind'] == 'event_execute']
                rows[0]['event_bytes'], rows[1]['event_bytes'] = rows[1]['event_bytes'], rows[0]['event_bytes']
                with self.assertRaisesRegex(ValueError, 'publication event order differs'):
                    saved.validate_publication_phase_receipt(
                        receipt, self.meta['controls'][order], historical=origin == 'historical')

    def test_reject_missing_class_unlimbo_return(self):
        for origin, order, receipt in self.control_receipts('original_calls'):
            with self.subTest(origin=origin, order=order):
                row = next(row for row in receipt['original_calls'] if row['kind'] == 'infantry_unlimbo')
                del row['result']
                with self.assertRaisesRegex(ValueError, 'Infantry Unlimbo return missing'):
                    saved.validate_publication_phase_receipt(
                        receipt, self.meta['controls'][order], historical=origin == 'historical')

    def test_complete_comparison_preserves_unselected_observation(self):
        for origin, order, receipt in self.control_receipts('original_calls'):
            with self.subTest(origin=origin, order=order):
                receipt['original_calls'][0]['edx'] ^= 1
                with self.assertRaisesRegex(ValueError, 'Complete original publication observations differ'):
                    saved.compare_publication_phase(
                        order, receipt, historical=origin == 'historical')


if __name__ == '__main__':
    unittest.main()
