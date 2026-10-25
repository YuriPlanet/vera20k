"""Guards on saved publication evidence, without native replay."""
import copy
import json
import unittest

from tools.spatial_oracle._factory_infantry_output import fixture, runtime as rt, saved


class PublicationPhaseEvidenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.meta = rt.metadata()['publication_phase']
        cls.receipts = {order: rt.read_pinned(row['receipt']) for order, row in cls.meta['controls'].items()}

    def control_receipts(self, *branches):
        # Copy only branches a rejection control mutates. The complete warmed
        # native rounds stay shared and are never written.
        for order, original in self.receipts.items():
            receipt = dict(original)
            for name in branches:
                receipt[name] = copy.deepcopy(original[name])
            yield order, receipt

    def test_both_complete_original_receipts_pass(self):
        for order, receipt in self.control_receipts():
            with self.subTest(order=order):
                result = saved.compare_publication_phase(order, receipt)
                self.assertTrue(result['complete_original_observations_equal'])
                self.assertEqual(result['complete_warmed_prefix_rounds'], 216)
                self.assertEqual(result['complete_normalized_original_sha256'],
                                 self.meta['controls'][order]['complete_normalized_original_sha256'])

    def test_fixture_preserves_serialization_and_selection(self):
        raw = (rt.REPO_ROOT / self.meta['rust_fixture']['path']).read_bytes()
        selected = fixture.publication_phase_local_fixture(
            self.receipts, self.meta['local_sources'], self.meta['parent_manifest_sha256'])
        self.assertEqual(raw, json.dumps(selected, indent=2).encode())
        before, after = (selected['cases'][name] for name in ('before_strip', 'after_strip'))
        self.assertEqual(before['executed_event_types'], [30, 11])
        self.assertEqual(after['executed_event_types'], [11, 30])
        self.assertTrue(before['placed']['archive_is_set'])
        self.assertFalse(after['placed']['archive_is_set'])

    def test_reject_native_code_change(self):
        for order, receipt in self.control_receipts('control_state'):
            with self.subTest(order=order):
                receipt['control_state']['native_code_unchanged'] = False
                with self.assertRaisesRegex(ValueError, 'Original publication code changed'):
                    saved.validate_publication_phase_receipt(receipt, self.meta['controls'][order])

    def test_reject_truncated_complete_rng_buffer(self):
        for order, receipt in self.control_receipts('complete_rng_buffers'):
            with self.subTest(order=order):
                row = next(iter(receipt['complete_rng_buffers'].values()))
                row['bytes'] = row['bytes'][:-2]
                with self.assertRaisesRegex(ValueError, 'Incomplete publication RNG buffer'):
                    saved.validate_publication_phase_receipt(receipt, self.meta['controls'][order])

    def test_reject_unwitnessed_strip_return(self):
        for order, receipt in self.control_receipts('original_calls'):
            with self.subTest(order=order):
                row = next(row for row in receipt['original_calls'] if row['kind'] == 'factory_take_changed')
                del row['result']
                with self.assertRaisesRegex(ValueError, 'takeChanged return/write witness missing'):
                    saved.validate_publication_phase_receipt(receipt, self.meta['controls'][order])

    def test_reject_reversed_actual_event_order(self):
        for order, receipt in self.control_receipts('original_calls'):
            with self.subTest(order=order):
                rows = [row for row in receipt['original_calls'] if row['kind'] == 'event_execute']
                rows[0]['event_bytes'], rows[1]['event_bytes'] = rows[1]['event_bytes'], rows[0]['event_bytes']
                with self.assertRaisesRegex(ValueError, 'publication event order differs'):
                    saved.validate_publication_phase_receipt(receipt, self.meta['controls'][order])

    def test_reject_missing_class_unlimbo_return(self):
        for order, receipt in self.control_receipts('original_calls'):
            with self.subTest(order=order):
                row = next(row for row in receipt['original_calls'] if row['kind'] == 'infantry_unlimbo')
                del row['result']
                with self.assertRaisesRegex(ValueError, 'Infantry Unlimbo return missing'):
                    saved.validate_publication_phase_receipt(receipt, self.meta['controls'][order])

    def test_complete_comparison_preserves_unselected_observation(self):
        for order, receipt in self.control_receipts('original_calls'):
            with self.subTest(order=order):
                receipt['original_calls'][0]['edx'] ^= 1
                with self.assertRaisesRegex(ValueError, 'Complete original publication observations differ'):
                    saved.compare_publication_phase(order, receipt)


if __name__ == '__main__':
    unittest.main()
