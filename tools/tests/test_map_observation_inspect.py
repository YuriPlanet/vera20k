"""Checked-observation projections, not game/native behavior fixtures."""
from copy import deepcopy
import json
import unittest

from tools.map_observation_inspect import (
    EXPECTATIONS_SCHEMA, MAX_ASSERTION_SAMPLES, inspect_observations, validate_expectations,
)
from tools.tactical_certification.core import ValidationError


def actor(identity=1, owner='Computer1', type_id='E1', health=50, cell=None):
    # Only fields this projection consumes; full receipt validation belongs to
    # test_map_observation.py and occurs before inspect_observations is called.
    return {'stable_id': identity, 'owner': owner, 'type_id': type_id, 'category': 'Infantry',
            'health': health, 'cell': cell or [10, 20], 'active': True, 'in_limbo': False,
            'dying': False, 'mission': {'effective': 5}, 'target': None, 'nav': None}


def house(credits=1000, ready=True, *, weapon=True):
    result = {'owner': 'Computer1', 'economy': {'credits': credits, 'spent_credits': 0, 'score': 0}}
    if weapon:
        result['super_weapons'] = [{'type': 'IronCurtainSpecial', 'interned_id': 30,
                                   'granted': True, 'ready': ready, 'on_hold': False,
                                   'charge_start': 0 if ready else 3, 'charge_duration': 100,
                                   'remaining': 0 if ready else 100, 'fade_countdown': 0,
                                   'fade_coords': [0, 0, 0]}]
    return result


def observations(actor_frames, house_frames=None):
    seen = set()
    frames = []
    for step, actors in enumerate(actor_frames):
        present = {row['stable_id'] for row in actors}
        seen.update(present)
        frames.append({'completed_steps': step, 'simulation_tick': step, 'binary_frame': step,
                       'total_simulation_ms': 22 * step, 'actors': deepcopy(actors),
                       'houses': deepcopy(house_frames[step] if house_frames else []),
                       'missing_actor_ids': sorted(seen - present), 'terrain': []})
    return {'policy': 'map-ordinary-command-observation-v4', 'owners': ['Computer1'],
            'commands': [], 'frames': frames, 'rule_types': []}


def expectation(*, subject=None, field='health', op='eq', value=100, **window):
    return {'name': 'sample assertion', 'subject': subject or {'actor': {'stable_id': 1}},
            'field': field, 'op': op, 'value': value, **(window or {'at_step': 0})}


def document(*assertions):
    return {'schema_version': EXPECTATIONS_SCHEMA, 'assertions': list(assertions)}


def result(observed, assertion):
    return inspect_observations(observed, expectations=document(assertion))['assertions']['results'][0]


class InspectionTests(unittest.TestCase):
    def test_changes_follow_stable_identity_through_capture_missing_and_reappearance(self):
        observed = observations([[actor()], [actor(owner='Computer2', health=75)], [],
                                 [actor(owner='Computer2', health=75)]])
        before = deepcopy(observed)
        report = inspect_observations(observed, owners=['Computer1'], fields=['owner', 'health'])
        rows = report['timeline']['rows']
        self.assertEqual([row['event'] for row in rows], ['added', 'changed', 'missing', 'reappeared'])
        self.assertEqual(rows[1]['changes']['owner']['after']['value'], 'Computer2')
        self.assertEqual(rows[1]['subject']['stable_id'], 1)
        self.assertIn('cause_not_observed', rows[2]['observation'])
        self.assertEqual(observed, before)
        json.dumps(report, allow_nan=False)

    def test_born_actor_is_added_when_first_observed(self):
        report = inspect_observations(observations([[], [actor()], [actor()]]), types=['E1'])
        self.assertEqual(len(report['timeline']['rows']), 1)
        self.assertEqual(report['timeline']['rows'][0]['completed_steps'], 1)

    def test_report_filters_use_first_observed_identity_and_do_not_reselect(self):
        observed = observations([[actor(owner='Computer2')], [actor(owner='Computer1')]])
        report = inspect_observations(observed, owners=['Computer1'])
        self.assertEqual(report['timeline']['selected_actor_count'], 0)
        self.assertEqual(report['timeline']['rows'], [])

    def test_requested_nested_fields_distinguish_null_from_absence(self):
        first, second = actor(), actor()
        first['foot'] = {'pending_entry_500': None}
        second['foot'] = {}
        report = inspect_observations(observations([[first], [second]]), fields=['foot.pending_entry_500'])
        delta = report['timeline']['rows'][1]['changes']['foot.pending_entry_500']
        self.assertEqual(delta['before'], {'observed': True, 'value': None})
        self.assertEqual(delta['after'], {'observed': False})

    def test_house_and_super_weapon_changes_and_queued_command_have_no_inferred_outcome(self):
        observed = observations([[], [], []], [[house()], [house(credits=1250, ready=False)],
                                              [house(credits=1250, ready=False)]])
        observed['frames'][2]['houses'][0]['super_weapons'][0]['remaining'] = 99
        observed['commands'] = [{'ordinal': 0, 'issue_after_step': 0, 'issued_simulation_tick': 0,
                                 'envelope_execute_tick': 0, 'owner': 'Computer1',
                                 'payload': {'LaunchSuperWeapon': {'super_type': 30, 'target_rx': 10, 'target_ry': 20}}}]
        rows = inspect_observations(observed)['timeline']['rows']
        self.assertEqual([row['kind'] for row in rows], ['house', 'super_weapon', 'command', 'house', 'super_weapon'])
        self.assertEqual(rows[2]['event'], 'queued')
        self.assertEqual(rows[2]['outcome'], 'not_observed')
        self.assertFalse(any(row['completed_steps'] == 2 for row in rows))
        self.assertEqual(rows[3]['changes']['economy.credits']['after']['value'], 1250)
        self.assertEqual(rows[4]['changes']['ready']['after']['value'], False)

    def test_type_and_id_filters_suppress_unrelated_house_rows(self):
        observed = observations([[actor(), actor(2, type_id='E2')]], [[house()]])
        report = inspect_observations(observed, types=['E2'], stable_ids=[2])
        self.assertEqual([row['subject']['stable_id'] for row in report['timeline']['rows']], [2])

    def test_truncated_timeline_still_evaluates_all_assertion_samples_and_ignores_filters(self):
        observed = observations([[actor(health=health)] for health in (50, 60, 70, 80, 100)])
        report = inspect_observations(observed, max_rows=1, expectations=document(
            expectation(eventually={'from_step': 0, 'through_step': 4})))
        self.assertEqual(report['timeline']['total_rows'], 5)
        self.assertEqual(report['timeline']['omitted_rows'], 4)
        self.assertTrue(report['timeline']['truncated'])
        checked = report['assertions']['results'][0]
        self.assertEqual(checked['status'], 'PASS')
        self.assertEqual(checked['observed_count'], 5)
        self.assertEqual(checked['first_match']['completed_steps'], 4)
        report = inspect_observations(observed, types=['NONEXISTENT'], max_rows=0,
                                      expectations=document(expectation(at_step=4)))
        self.assertEqual(report['timeline']['rows'], [])
        self.assertEqual(report['assertions']['status'], 'PASS')

    def test_quickload_uses_completed_steps_in_timeline_and_assertions(self):
        observed = observations([[actor()], [actor()], [actor(health=100)]])
        observed['frames'][2].update(simulation_tick=1, binary_frame=1)
        observed['load_segments'] = {'transitions': [{'after_step': 1, 'gesture_ordinal': 0,
                                                     'before': {'simulation_tick': 1, 'binary_frame': 1, 'total_simulation_ms': 22},
                                                     'after': {'simulation_tick': 0, 'binary_frame': 0, 'total_simulation_ms': 0}}]}
        report = inspect_observations(observed, expectations=document(expectation(
            subject={'actor': {'stable_id': 1, 'bind_step': 2}}, at_step=2)))
        self.assertEqual([row['completed_steps'] for row in report['timeline']['rows']], [0, 1, 2])
        self.assertEqual(report['timeline']['rows'][1]['event'], 'clock_restored')
        match = report['assertions']['results'][0]['first_match']
        self.assertEqual((match['completed_steps'], match['simulation_tick']), (2, 1))
        self.assertEqual(match['observation_epoch'], 1)

    def test_quickload_reused_id_starts_new_timeline_identity_and_rebinds_filters(self):
        observed = observations([[actor(type_id='E1')], [], [actor(type_id='E2', health=100)]])
        observed['load_segments'] = {'transitions': [{'after_step': 1, 'gesture_ordinal': 0,
            'before': {'simulation_tick': 1}, 'after': {'simulation_tick': 0}}]}
        report = inspect_observations(observed)
        rows = [row for row in report['timeline']['rows'] if row['kind'] == 'actor']
        self.assertEqual([row['event'] for row in rows], ['added', 'missing', 'added'])
        self.assertEqual([row['observation_epoch'] for row in rows], [0, 0, 1])
        self.assertEqual(report['timeline']['selected_actor_count'], 2)
        filtered = inspect_observations(observed, types=['E1'])
        self.assertFalse(any(row['kind'] == 'actor' and row['completed_steps'] == 2
                             for row in filtered['timeline']['rows']))
        filtered = inspect_observations(observed, types=['E2'])
        self.assertEqual([row['completed_steps'] for row in filtered['timeline']['rows']
                          if row['kind'] == 'actor'], [2])


class AssertionTests(unittest.TestCase):
    def test_actor_assertions_cannot_follow_reused_ids_across_quickload(self):
        observed = observations([[actor(type_id='E1')], [], [actor(type_id='E2', health=100)]])
        observed['load_segments'] = {'transitions': [{'after_step': 1, 'gesture_ordinal': 0,
            'before': {'simulation_tick': 1}, 'after': {'simulation_tick': 0}}]}
        for selector in ({'type_id': 'E1'}, {'stable_id': 1}):
            with self.subTest(selector=selector):
                checked = result(observed, expectation(subject={'actor': selector}, at_step=2))
                self.assertEqual(checked['status'], 'UNOBSERVED')
                self.assertEqual(checked['first_unobserved']['reason'], 'actor_identity_crosses_load')
        for selector in ({'type_id': 'E2', 'bind_step': 2}, {'stable_id': 1, 'bind_step': 2}):
            checked = result(observed, expectation(subject={'actor': selector}, at_step=2))
            self.assertEqual(checked['status'], 'PASS')
            self.assertEqual(checked['binding']['observation_epoch'], 1)
            checked = result(observed, expectation(subject={'actor': selector}, at_step=0))
            self.assertEqual(checked['status'], 'UNOBSERVED')
        # The quickload is issued after its boundary frame, which is still old.
        checked = result(observed, expectation(field='present', value=False, at_step=1))
        self.assertEqual(checked['status'], 'PASS')
        observed['frames'][2]['actors'][0]['type_id'] = 'E1'
        checked = result(observed, expectation(subject={'actor': {'type_id': 'E1'}}, at_step=2))
        self.assertEqual(checked['status'], 'UNOBSERVED', 'same type does not prove identity')

    def test_named_actor_binding_survives_capture_and_does_not_pick_replacement(self):
        observed = observations([[actor()], [actor(owner='Computer2', health=80), actor(2, health=100)],
                                 [actor(2, health=100)]])
        subject = {'actor': {'owner': 'Computer1', 'type_id': 'E1'}}
        checked = result(observed, expectation(subject=subject, at_step=1))
        self.assertEqual(checked['status'], 'FAIL')
        self.assertEqual(checked['binding']['actor'], 1)
        self.assertEqual(checked['first_failure']['actual'], 80)
        checked = result(observed, expectation(subject=subject, at_step=2))
        self.assertEqual(checked['status'], 'UNOBSERVED')
        self.assertEqual(checked['first_unobserved']['reason'], 'actor_missing')

    def test_ambiguous_selector_never_passes_even_if_every_actor_matches(self):
        observed = observations([[actor(health=100), actor(2, health=100)]])
        checked = result(observed, expectation(subject={'actor': {'type_id': 'E1'}}))
        self.assertEqual(checked['status'], 'UNOBSERVED')
        self.assertEqual(checked['binding']['candidate_stable_ids'], [1, 2])
        self.assertEqual(checked['first_unobserved']['reason'], 'ambiguous_actor')

    def test_cell_disambiguates_and_new_actor_requires_explicit_binding_step(self):
        observed = observations([[], [actor(health=100), actor(2, cell=[30, 40])]])
        selector = {'type_id': 'E1', 'cell': [10, 20]}
        checked = result(observed, expectation(subject={'actor': selector}, at_step=1))
        self.assertEqual(checked['status'], 'UNOBSERVED')
        selector['bind_step'] = 1
        checked = result(observed, expectation(subject={'actor': selector}, at_step=1))
        self.assertEqual(checked['status'], 'PASS')
        self.assertEqual(checked['binding']['actor'], 1)

    def test_missing_binding_frame_and_missing_evaluation_frame_are_not_false(self):
        observed = observations([[actor()]])
        checked = result(observed, expectation(subject={'actor': {'type_id': 'E1', 'bind_step': 2}}))
        self.assertEqual(checked['first_unobserved']['reason'], 'binding_frame_not_observed')
        checked = result(observed, expectation(at_step=2))
        self.assertEqual(checked['status'], 'UNOBSERVED')
        self.assertEqual(checked['false_count'], 0)
        self.assertEqual(checked['first_unobserved']['completed_steps'], 2)

    def test_disappearance_requires_retained_missing_id_not_empty_selection(self):
        observed = observations([[actor()], []])
        checked = result(observed, expectation(field='present', value=False, at_step=1))
        self.assertEqual(checked['status'], 'PASS')
        checked = result(observed, expectation(subject={'actor': {'stable_id': 2}},
                                               field='present', value=False, at_step=1))
        self.assertEqual(checked['status'], 'UNOBSERVED')
        checked = result(observed, expectation(subject={'actor': {'type_id': 'MTNK'}},
                                               field='present', value=False, at_step=1))
        self.assertEqual(checked['status'], 'UNOBSERVED')

    def test_eventually_passes_on_witness_even_if_actor_disappears_later(self):
        observed = observations([[actor(health=100)], []])
        checked = result(observed, expectation(eventually={'from_step': 0, 'through_step': 1}))
        self.assertEqual(checked['status'], 'PASS')
        self.assertEqual(checked['true_count'], 1)
        self.assertEqual(checked['unobserved_count'], 1)
        self.assertFalse(checked['coverage_complete'])

    def test_eventually_without_witness_needs_complete_coverage_for_fail(self):
        checked = result(observations([[actor()], [actor()]]), expectation(
            eventually={'from_step': 0, 'through_step': 1}))
        self.assertEqual(checked['status'], 'FAIL')
        checked = result(observations([[actor()], []]), expectation(
            eventually={'from_step': 0, 'through_step': 1}))
        self.assertEqual(checked['status'], 'UNOBSERVED')
        self.assertEqual(checked['first_failure']['actual'], 50)

    def test_always_fails_on_counterexample_despite_later_missing_coverage(self):
        checked = result(observations([[actor()], []]), expectation(always={'from_step': 0, 'through_step': 1}))
        self.assertEqual(checked['status'], 'FAIL')
        self.assertEqual(checked['unobserved_count'], 1)
        checked = result(observations([[actor(health=100)], []]), expectation(always={'from_step': 0, 'through_step': 1}))
        self.assertEqual(checked['status'], 'UNOBSERVED')
        checked = result(observations([[actor(health=100)]]), expectation(always={'from_step': 0, 'through_step': 0}))
        self.assertEqual(checked['status'], 'PASS')

    def test_false_unobserved_and_not_requested_have_distinct_report_statuses(self):
        observed = observations([[actor()]])
        self.assertEqual(inspect_observations(observed)['assertions']['status'], 'NOT_REQUESTED')
        report = inspect_observations(observed, expectations=document(expectation(field='unknown', value=None)))
        self.assertEqual(report['assertions']['status'], 'FAIL')
        self.assertEqual(report['assertions']['unobserved_count'], 1)
        self.assertEqual(report['assertions']['fail_count'], 0)
        self.assertNotIn('integrity', report)

    def test_null_is_observed_but_missing_parent_is_not_null(self):
        observed = observations([[actor()]])
        checked = result(observed, expectation(field='target', value=None))
        self.assertEqual(checked['status'], 'PASS')
        checked = result(observed, expectation(field='target.Entity', value=None))
        self.assertEqual(checked['status'], 'UNOBSERVED')
        checked = result(observed, expectation(field='unknown', op='ne', value=False))
        self.assertEqual(checked['status'], 'UNOBSERVED')

    def test_literal_types_do_not_confuse_false_zero_or_true_one(self):
        observed = observations([[actor(health=1)]])
        self.assertEqual(result(observed, expectation(field='active', value=1))['status'], 'FAIL')
        self.assertEqual(result(observed, expectation(value=True))['status'], 'FAIL')
        self.assertEqual(result(observed, expectation(value=1.0))['status'], 'PASS')
        self.assertEqual(result(observed, expectation(field='cell', value=[10, 20]))['status'], 'PASS')

    def test_numeric_comparisons_and_wrong_actual_type(self):
        observed = observations([[actor()]])
        for op, value in [('lt', 60), ('le', 50), ('gt', 40), ('ge', 50), ('ne', 100)]:
            with self.subTest(op=op):
                self.assertEqual(result(observed, expectation(op=op, value=value))['status'], 'PASS')
        checked = result(observed, expectation(field='active', op='ge', value=1))
        self.assertEqual(checked['status'], 'UNOBSERVED')
        self.assertEqual(checked['first_unobserved']['actual'], True)

    def test_house_and_named_super_weapon_assertions(self):
        observed = observations([[], []], [[house()], [house(credits=1250, ready=False)]])
        checked = result(observed, expectation(subject={'house': 'Computer1'}, field='economy.credits',
                                               op='ge', value=1200, at_step=1))
        self.assertEqual(checked['status'], 'PASS')
        checked = result(observed, expectation(subject={'super_weapon': {'owner': 'Computer1', 'type': 'IronCurtainSpecial'}},
                                               field='ready', value=False, at_step=1))
        self.assertEqual(checked['status'], 'PASS')
        self.assertEqual(checked['first_match']['actual'], False)
        checked = result(observed, expectation(subject={'house': 'Computer2'}, field='economy.credits', value=0))
        self.assertEqual(checked['first_unobserved']['reason'], 'house_not_observed')

    def test_unretained_super_weapons_do_not_pass_negative_assertions(self):
        observed = observations([[]], [[house(weapon=False)]])
        checked = result(observed, expectation(subject={'super_weapon': {'owner': 'Computer1', 'type': 'IronCurtainSpecial'}},
                                               field='ready', value=False))
        self.assertEqual(checked['status'], 'UNOBSERVED')
        self.assertEqual(checked['first_unobserved']['reason'], 'super_weapons_not_observed')


class GestureAssertionTests(unittest.TestCase):
    @staticmethod
    def observed(*receipts):
        observed = observations([[], [], []])
        observed['gesture_input'] = {'receipts': list(receipts)}
        return observed

    @staticmethod
    def receipt(ordinal=0, step=1):
        # Projection fixture only. The capture validator checks the full Thumb
        # schema; this consumer reads the actual receipt without recomputing it.
        return {'ordinal': ordinal, 'issue_after_step': step,
                'command_bar': {'command': 'Thumb', 'thumb': {
                    'before': {'closed': True, 'has_lost': True, 'exit_ready': False},
                    'release': {'dispatch': {'key_after': 0x80f1, 'action_result': 1}},
                    'after': {'closed': False, 'has_lost': True, 'exit_ready': False}}}}

    def test_ordinal_selects_actual_receipt_with_key_and_consumption_kept_distinct(self):
        observed = self.observed(self.receipt(7), self.receipt(0))
        before = deepcopy(observed)
        for field, value in (('command_bar.thumb.release.dispatch.key_after', 0x80f1),
                             ('command_bar.thumb.release.dispatch.action_result', 1),
                             ('command_bar.thumb.before.has_lost', True),
                             ('command_bar.thumb.after.closed', False)):
            with self.subTest(field=field):
                checked = result(observed, expectation(subject={'gesture': 7}, field=field,
                                                       value=value, at_step=1))
                self.assertEqual(checked['status'], 'PASS')
                self.assertEqual(checked['binding'], {'gesture': 7, 'issue_after_step': 1})
        checked = result(observed, expectation(subject={'gesture': 7},
            field='command_bar.thumb.release.dispatch.action_result', value=0x80f1, at_step=1))
        self.assertEqual(checked['status'], 'FAIL')
        self.assertEqual(checked['first_failure']['actual'], 1)
        self.assertEqual(observed, before)

    def test_missing_duplicate_and_unsampled_gestures_are_unobserved(self):
        cases = [(self.observed(), 0, 1, 'gesture_not_observed'),
                 (self.observed(self.receipt(7)), 0, 1, 'gesture_not_observed'),
                 (self.observed(self.receipt(), self.receipt()), 0, 1, 'ambiguous_gesture'),
                 (self.observed(self.receipt(step=3)), 0, 3, 'frame_not_observed')]
        for observed, ordinal, step, reason in cases:
            with self.subTest(reason=reason, ordinal=ordinal):
                checked = result(observed, expectation(subject={'gesture': ordinal},
                    field='command_bar.thumb.after.closed', value=False, at_step=step))
                self.assertEqual(checked['status'], 'UNOBSERVED')
                self.assertEqual(checked['first_unobserved']['reason'], reason)
                self.assertEqual(checked['observed_count'], 0)
        # Historical captures need no gesture_input section at all.
        checked = result(observations([[]]), expectation(subject={'gesture': 0}, field='ordinal', value=0))
        self.assertEqual(checked['status'], 'UNOBSERVED')

    def test_gesture_state_never_persists_before_or_after_its_issue_step(self):
        observed = self.observed(self.receipt())
        for step in (0, 2):
            checked = result(observed, expectation(subject={'gesture': 0},
                field='command_bar.thumb.after.closed', value=False, at_step=step))
            self.assertEqual(checked['status'], 'UNOBSERVED')
            self.assertEqual(checked['first_unobserved']['reason'], 'gesture_not_observed_at_step')
        checked = result(observed, expectation(subject={'gesture': 0},
            field='command_bar.thumb.after.closed', value=False, always={'from_step': 0, 'through_step': 2}))
        self.assertEqual(checked['status'], 'UNOBSERVED')
        self.assertEqual((checked['observed_count'], checked['unobserved_count']), (1, 2))
        checked = result(observed, expectation(subject={'gesture': 0},
            field='command_bar.thumb.after.closed', value=False, eventually={'from_step': 1, 'through_step': 2}))
        self.assertEqual(checked['status'], 'PASS')
        self.assertFalse(checked['coverage_complete'])

    def test_null_dispatch_is_observed_but_missing_dispatch_fields_are_not(self):
        receipt = self.receipt()
        receipt['command_bar']['thumb']['release']['dispatch'] = None
        observed = self.observed(receipt)
        checked = result(observed, expectation(subject={'gesture': 0},
            field='command_bar.thumb.release.dispatch', value=None, at_step=1))
        self.assertEqual(checked['status'], 'PASS')
        checked = result(observed, expectation(subject={'gesture': 0},
            field='command_bar.thumb.release.dispatch.key_after', value=0, at_step=1))
        self.assertEqual(checked['status'], 'UNOBSERVED')
        self.assertEqual(checked['first_unobserved']['reason'], 'field_not_observed')
        receipt['command_bar']['thumb']['release'].pop('dispatch')
        checked = result(observed, expectation(subject={'gesture': 0},
            field='command_bar.thumb.release.dispatch', value=None, at_step=1))
        self.assertEqual(checked['status'], 'UNOBSERVED')


class ExpectationSchemaTests(unittest.TestCase):
    def test_gesture_subject_is_one_unsigned_ordinal_without_selector_options(self):
        valid = document(expectation(subject={'gesture': 0}, field='ordinal', value=0))
        self.assertEqual(validate_expectations(valid)[0]['subject'], {'gesture': 0})
        for subject in ({'gesture': -1}, {'gesture': True}, {'gesture': 0.0}, {'gesture': '0'},
                        {'gesture': None}, {'gesture': {'ordinal': 0}}, {'gesture': 1 << 64},
                        {'gesture': 0, 'house': 'Computer1'}, {'gesture': 0, 'bind_step': 0}):
            with self.subTest(subject=subject), self.assertRaises(ValidationError):
                validate_expectations(document(expectation(subject=subject)))

    def test_rejects_typos_ambiguous_windows_and_executable_expressions(self):
        original = document(expectation())
        mutations = [lambda d: d.update(unknown=True),
                     lambda d: d['assertions'][0].update(eventually={'from_step': 0, 'through_step': 2}),
                     lambda d: d['assertions'][0].update(field='health > 0'),
                     lambda d: d['assertions'][0].update(field="__import__('os')"),
                     lambda d: d['assertions'][0].update(subject={'actor': {'type': 'E1'}}),
                     lambda d: d['assertions'][0].update(subject={'actor': {'stable_id': 1, 'type_id': 'E1'}}),
                     lambda d: d['assertions'][0].update(op='ge', value=True),
                     lambda d: d['assertions'][0].update(value=float('nan')),
                     lambda d: d['assertions'][0].update(at_step=True),
                     lambda d: d['assertions'][0].update(value=1 << 65)]
        for ordinal, mutate in enumerate(mutations):
            with self.subTest(ordinal=ordinal), self.assertRaises(ValidationError):
                candidate = deepcopy(original)
                mutate(candidate)
                validate_expectations(candidate)

    def test_windows_names_and_literal_sizes_are_bounded(self):
        invalid = [document(expectation(), expectation()),
                   document(expectation(eventually={'from_step': 2, 'through_step': 1})),
                   document(expectation(value=[0] * 257)),
                   document(expectation(value='x' * 1025)),
                   document()]
        for candidate in invalid:
            with self.subTest(candidate=str(candidate)[:80]), self.assertRaises(ValidationError):
                validate_expectations(candidate)
        assertions = [dict(expectation(always={'from_step': 0, 'through_step': 100_000}), name=f'query {index}')
                      for index in range(MAX_ASSERTION_SAMPLES // 100_001 + 1)]
        with self.assertRaisesRegex(ValidationError, 'requested samples'):
            validate_expectations(document(*assertions))

    def test_report_arguments_are_bounded(self):
        observed = observations([[]])
        for args in ({'max_rows': -1}, {'max_rows': True}, {'max_rows': 10_001},
                     {'fields': []}, {'fields': ['health', 'health']}, {'fields': ['__bad__']},
                     {'stable_ids': [0]}):
            with self.subTest(args=args), self.assertRaises(ValidationError):
                inspect_observations(observed, **args)


if __name__ == '__main__':
    unittest.main()
