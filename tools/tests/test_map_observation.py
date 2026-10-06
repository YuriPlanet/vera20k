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
        self.actor_frames = {}
        self.house_frames = {}
        self.rule_types = [{'type_id': name, 'interned_id': identity, 'category': 'Structure'}
                           for name, identity in [('GACNST', 40), ('GAPOWR', 41), ('GAPILE', 42)]]
        self.terrain_frames = {}
        self.input_frames = {}
        self.gesture_receipts = []
        self.tactical_extent = None
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
        width, height = self.profile['width'], self.profile['height']
        ticks = self.profile['ticks']
        fingerprint = lambda tick: {'simulation_tick': tick, 'binary_frame': tick,
                                   'total_simulation_ms': tick * 22,
                                   'deterministic_state_hash': 7 + tick}
        receipt = lambda tick: {'tick_before': tick, 'tick_after': tick + 1,
                               'binary_frame_before': tick, 'binary_frame_after': tick + 1}
        identity = lambda path: {'path': str(path), 'byte_length': path.stat().st_size,
                                 'sha256': sha256_bytes(path.read_bytes())}
        manifest = {
            'schema_version': observation.CHILD_SCHEMA, 'status': 'COMPLETE',
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
                       'surface_extent': [width, height], 'internal_extent': [width, height],
                       'unit_atlas': deepcopy(self.unit_atlas),
                       'presentation_clock': self.clock(ticks),
                       'camera': {'requested_cell': self.profile.get('camera_cell'),
                                  'top_left': [0.0, 0.0], 'zoom': 1.0},
                       'neutral_input': {'static_default_cursor': True, 'camera_input_idle': True}},
            'frame': {'file_name': 'frame.bgra', 'width': width, 'height': height,
                      'row_stride': width * 4, 'byte_length': len(frame), 'sha256': sha256_bytes(frame),
                      'pixel_layout': 'BGRA8', 'surface_format': 'Bgra8UnormSrgb'},
            'native_comparator': 'NONE', 'parity_certification': 'NONE',
        }
        if 'cursor_position' in self.profile:
            manifest['render']['cursor_position'] = [float(value) for value in self.profile['cursor_position']]
        seen = set()
        frames = []
        for step in range(ticks + 1):
            actors = deepcopy(self.actor_frames.get(step, []))
            current_ids = {actor['stable_id'] for actor in actors}
            seen.update(current_ids)
            terrain = self.terrain_frames.get(step, [self.unallocated_cell(cell)
                                                     for cell in self.profile.get('terrain_cells', [])])
            frames.append({'completed_steps': step, 'simulation_tick': step, 'binary_frame': step,
                           'total_simulation_ms': step * 22, 'actors': actors,
                           'houses': deepcopy(self.house_frames.get(step, [
                               {'owner': owner, 'economy': None}
                               for owner in self.profile.get('observe_owners', [])])),
                           'missing_actor_ids': sorted(seen - current_ids), 'terrain': deepcopy(terrain)})
            if 'gestures' in self.profile:
                frames[-1]['input'] = deepcopy(self.input_frames.get(step, self.input_observation()))
        manifest['observations'] = {
            'policy': observation.OBSERVATION_POLICY, 'owners': self.profile.get('observe_owners', []),
            'rule_types': deepcopy(self.rule_types),
            'commands': [{'ordinal': index, 'issue_after_step': request['issue_after_step'],
                          'issued_simulation_tick': request['issue_after_step'],
                          'envelope_execute_tick': request['issue_after_step'],
                          'owner': request['owner'], 'payload': deepcopy(request['payload'])}
                         for index, request in enumerate(self.profile.get('commands', []))],
            'frames': frames,
        }
        if 'observe_types' in self.profile:
            manifest['observations']['type_filter'] = deepcopy(self.profile['observe_types'])
        if 'gestures' in self.profile:
            manifest['observations']['gesture_input'] = {
                'policy': observation.GESTURE_POLICY, 'equal_step_order': 'commands_then_gestures',
                'tactical_extent': self.tactical_extent or [width, height],
                'receipts': deepcopy(self.gesture_receipts),
            }
        self.change(manifest)
        (directory / 'capture.json').write_text(json.dumps(manifest))
        return self.result

    def run_capture(self):
        with patch.object(observation, 'run_child', side_effect=self.fake_child):
            return observation.capture(profile_path=self.profile_path, contract_path=self.contract,
                                       output=self.output, working_directory=self.root,
                                       executable=self.executable)

    @staticmethod
    def actor(identity=1, owner='Computer1', category='Infantry'):
        return {
            'stable_id': identity, 'owner': owner, 'type_id': 'E1' if category == 'Infantry' else 'MTNK',
            'category': category, 'cell': [87, 53], 'physical_leptons': [22400, 13696, 416],
            'on_bridge': True, 'health': 125, 'active': True, 'in_limbo': False, 'dying': False,
            'mission': {'current': 5, 'queued': -1, 'suspended': -1, 'effective': 5,
                        'handler_state': 0, 'start_frame': 0, 'ai_counter': 0,
                        'dispatch_timer': {'start_frame': 0, 'delay': 15}},
            'target': {'Entity': 8}, 'archive': {'Cell': [87, 53]},
            'nav': {'Object': {'id': 9}},
            'foot': None if category == 'Structure' else {
                     'retarget_after_stop_688': False, 'firing_sequence_latch_68d': 0,
                     'infantry_doing': 0 if category == 'Infantry' else None,
                     'navigation_leptons': [22400, 13696, 416], 'navigation_unavailable': None,
                     'walk_head_leptons': None, 'walk_destination_leptons': None,
                     'walk_is_moving': None},
            'building': MapObservationTests.building(identity) if category == 'Structure' else None,
            'miner': None,
            'radio': {'contacts': [None], 'dock_entered_with': None},
        }

    @staticmethod
    def building(owner=1):
        return {'body_state': 1, 'queued_body_state': -1, 'construction_control': [0, 25, 2],
                'stage': {'value': 0, 'changed': 0, 'rate': 0, 'increment': 1,
                          'timer': {'start_frame': 50, 'duration': 0}},
                'ready_latch': 1, 'actually_placed': True, 'last_operational': False,
                'animation_slots': [{'slot': 3, 'anim_id': 100,
                    'animation': {'stable_id': 100, 'native_id': 10, 'type_id': 'GAPOWR_A',
                        'interned_type_id': 80, 'physical_leptons': [22400, 13696, 416],
                        'in_logic_vector': True, 'owner_entity': None, 'building_slot': [owner, 3],
                        'runtime': {'current_frame': 0, 'frame_step': 1, 'delay_remaining': 0,
                            'rate_reload': 4, 'frame_timer': {'start_frame': 50, 'duration': 4},
                            'loop_remaining': 255, 'first_ai_guard': False,
                            'constructor_reverse': False, 'inactive': False, 'paused': False}}}]}

    @staticmethod
    def unit():
        return {'deployed_6e0': 0, 'deploying_6e1': 1, 'undeploying_6e2': 0,
                'landing_for_deploy_134': False, 'stage_f8': 2, 'body_counter_538': 7,
                'deploy_anim_130': {'stable_id': 100, 'live': {
                    'type_id': 'SCHPDEPL', 'frame': 4, 'owner_entity': 1}}}

    @staticmethod
    def unallocated_cell(coordinate):
        return {'cell': list(coordinate), 'allocated': False,
                **{key: None for key in ('final_tile_index', 'final_sub_tile', 'presentation_tile',
                                        'level', 'slope', 'raw_bridge_flags', 'bridge_state',
                                        'has_deck', 'deck_level', 'walkable', 'transition')}}

    def scripted_profile(self):
        self.profile.update(
            schema_version=observation.PROFILE_V2, observe_owners=['Computer1'],
            camera_cell=[87, 53], terrain_cells=[[87, 53]],
            commands=[{'issue_after_step': 0, 'owner': 'Computer1', 'payload': {'Stop': {'entity_id': 1}}},
                      {'issue_after_step': 0, 'owner': 'Computer1',
                       'payload': {'Guard': {'entity_id': 1, 'target_id': None}}},
                      {'issue_after_step': 2, 'owner': 'Computer1',
                       'payload': {'ForceAttackCell': {'attacker_id': 1, 'target_rx': 87, 'target_ry': 53}}}])
        self.profile_path.write_text(json.dumps(self.profile))
        self.actor_frames = {step: [self.actor()] for step in range(4)}

    def test_v2_records_exact_issue_order_without_input_delay_and_every_actor_frame(self):
        self.scripted_profile()
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        transcript = report['capture']['observations']
        self.assertEqual([row['ordinal'] for row in transcript['commands']], [0, 1, 2])
        self.assertEqual([row['envelope_execute_tick'] for row in transcript['commands']], [0, 0, 2])
        self.assertEqual([row['completed_steps'] for row in transcript['frames']], [0, 1, 2, 3])
        self.assertEqual(transcript['frames'][0]['actors'][0]['nav'], {'Object': {'id': 9}})
        self.assertEqual(report['capture']['camera']['requested_cell'], [87, 53])
        self.assertEqual(observation.validate_run(self.output)['status'], 'VALID')

    def cursor_profile(self):
        self.profile.update(schema_version=observation.PROFILE_V2,
                            width=4, height=4, cursor_position=[2, 1])
        self.frame = bytes(range(64))
        self.profile_path.write_text(json.dumps(self.profile))

    @staticmethod
    def input_observation(selected=(), remaining=0, pending=False, active=None):
        return {'selected_ids': list(selected), 'selection_pending': pending,
                'target_line_remaining': remaining,
                'target_line_active': remaining > 0 if active is None else active}

    def gesture_profile(self):
        # These are synthetic wrapper receipts, not gameplay/parity goldens.
        self.profile.update(schema_version=observation.PROFILE_V2, width=8, height=8,
                            cursor_position=[6, 6],
                            gestures=[{'issue_after_step': 0,
                                       'gesture': {'kind': 'click', 'position': [1, 1]}},
                                      {'issue_after_step': 0,
                                       'gesture': {'kind': 'drag', 'from': [1, 1], 'to': [4, 4]}},
                                      {'issue_after_step': 2,
                                       'gesture': {'kind': 'click', 'position': [2, 2]}}])
        self.profile_path.write_text(json.dumps(self.profile))
        self.frame = bytes(range(256))
        self.tactical_extent = [6, 6]
        self.gesture_receipts = [
            {'ordinal': index, 'issue_after_step': row['issue_after_step'],
             'issued_simulation_tick': row['issue_after_step'],
             'issued_binary_frame': row['issue_after_step'], 'gesture': deepcopy(row['gesture']),
             'before': self.input_observation(),
             'after': self.input_observation([7, 3], 25, True),
             'left_press_captured': True, 'band_box_before_release': row['gesture']['kind'] == 'drag',
             'neutral_input_restored': True,
             'queued_commands': [{'owner': 'VERA-OBSERVER', 'execute_tick': row['issue_after_step'],
                                  'payload': {'Select': {'entity_ids': [7, 3], 'additive': False}}}]}
            for index, row in enumerate(self.profile['gestures'])]
        self.input_frames = {1: self.input_observation([7, 3], 24),
                             2: self.input_observation([7, 3], 23),
                             3: self.input_observation([7, 3], 24)}

    def test_gestures_preserve_input_receipts_and_frame_selection_order_separately_from_commands(self):
        self.gesture_profile()
        self.profile['commands'] = [{'issue_after_step': 0, 'owner': 'VERA-OBSERVER',
                                    'payload': {'Stop': {'entity_id': 7}}}]
        self.profile_path.write_text(json.dumps(self.profile))
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        transcript = report['capture']['observations']
        self.assertEqual(transcript['gesture_input']['receipts'], self.gesture_receipts)
        self.assertEqual(transcript['gesture_input']['equal_step_order'], 'commands_then_gestures')
        self.assertEqual(transcript['frames'][1]['input']['selected_ids'], [7, 3])
        self.assertEqual(transcript['frames'][1]['input']['target_line_remaining'], 24)
        self.assertEqual(transcript['commands'][0]['payload'], {'Stop': {'entity_id': 7}})
        self.assertEqual(observation.validate_run(self.output)['status'], 'VALID')

    def test_gesture_presence_is_optional_and_empty_list_still_records_input_state(self):
        report = self.run_capture()
        self.assertNotIn('gesture_input', report['capture']['observations'])
        self.assertNotIn('input', report['capture']['observations']['frames'][0])
        self.gesture_profile()
        self.profile['gestures'] = []
        self.gesture_receipts = []
        self.profile_path.write_text(json.dumps(self.profile))
        self.output = self.root / 'empty-gesture-input'
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertEqual(report['capture']['observations']['gesture_input']['receipts'], [])
        self.assertIn('input', report['capture']['observations']['frames'][0])

    def test_gesture_profile_rejects_null_v1_unsealed_cursor_bad_shapes_order_and_bounds(self):
        self.gesture_profile()
        modern = deepcopy(self.profile)
        valid = deepcopy(modern['gestures'][0])
        cases = [dict(modern, gestures=None), dict(modern, schema_version=observation.PROFILE_V1),
                 {key: value for key, value in modern.items() if key != 'cursor_position'},
                 dict(modern, gestures=[valid] * 1025)]
        for key, value in (('issue_after_step', True), ('issue_after_step', 3),
                           ('issue_after_step', -1), ('issue_after_step', 0.0), ('extra', True)):
            cases.append(dict(modern, gestures=[dict(valid, **{key: value})]))
        cases.append(dict(modern, gestures=[dict(valid, issue_after_step=1), valid]))
        for gesture in (None, {}, {'kind': 'move', 'position': [1, 1]},
                        {'kind': 'click', 'position': [1, 1], 'button': 'right'},
                        {'kind': 'drag', 'from': [1, 1]},
                        {'kind': 'drag', 'from': [1, 1], 'to': [1, 1]}):
            cases.append(dict(modern, gestures=[dict(valid, gesture=gesture)]))
        for point in (None, [], [1], [1, 1, 1], [0, 1], [1, 0], [7, 1], [1, 7],
                      [8, 1], [-1, 1], [True, 1], [1.0, 1], ['1', 1], [1 << 32, 1]):
            cases.append(dict(modern, gestures=[dict(valid, gesture={'kind': 'click', 'position': point})]))
        for index, candidate in enumerate(cases):
            with self.subTest(case=index), patch.object(observation, 'run_child') as child:
                self.profile_path.write_text(json.dumps(candidate))
                with self.assertRaises(ValidationError):
                    self.run_capture()
                child.assert_not_called()
                self.assertFalse(self.output.exists())

    def test_gesture_receipts_reject_retime_reorder_missed_capture_and_wrong_tactical_extent(self):
        self.gesture_profile()
        changes = [lambda data: data.pop('receipts'),
                   lambda data: data.update(equal_step_order='gestures_then_commands'),
                   lambda data: data.update(policy='unknown'),
                   lambda data: data.update(extra=True),
                   lambda data: data.update(tactical_extent=[9, 6]),
                   lambda data: data.update(tactical_extent=[5, 5]),
                   lambda data: data.update(tactical_extent=[True, 6]),
                   lambda data: data['receipts'].reverse(),
                   lambda data: data['receipts'].pop(),
                   lambda data: data['receipts'][0].update(gesture={'kind': 'click', 'position': [2, 2]})]
        for key, value in (('ordinal', True), ('issued_simulation_tick', 1),
                           ('issued_binary_frame', 1), ('issue_after_step', 1),
                           ('left_press_captured', False), ('neutral_input_restored', False),
                           ('band_box_before_release', True), ('left_press_captured', 1), ('extra', True)):
            changes.append(lambda data, key=key, value=value: data['receipts'][0].update({key: value}))
        for index, change in enumerate(changes):
            with self.subTest(case=index):
                self.output = self.root / f'bad-gesture-{index}'
                self.change = lambda manifest, f=change: f(manifest['observations']['gesture_input'])
                report = self.run_capture()
                self.assertEqual(report['status'], 'INVALID', report)
                self.assertIn('observations.gesture_input', report['errors'][0])

    def test_input_state_and_observed_queue_are_bounded_typed_and_do_not_invent_admission(self):
        self.gesture_profile()
        self.gesture_receipts[1]['queued_commands'] = []  # Refused/empty input still has a receipt.
        self.gesture_receipts[2]['after']['target_line_active'] = False  # Option gate may be off.
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        input_changes = [lambda data: data.update(selected_ids=[7, 7]),
                         lambda data: data.update(selected_ids=[0]),
                         lambda data: data.update(selected_ids=[True]),
                         lambda data: data.update(selection_pending=1),
                         lambda data: data.update(target_line_remaining=True),
                         lambda data: data.update(target_line_remaining=1 << 31),
                         lambda data: data.update(target_line_active=True, target_line_remaining=0),
                         lambda data: data.update(extra=True)]
        for index, change in enumerate(input_changes):
            for location in ('frame', 'gesture'):
                with self.subTest(case=index, location=location):
                    self.output = self.root / f'bad-input-{location}-{index}'
                    self.change = lambda manifest, f=change, where=location: f(
                        manifest['observations']['frames'][1]['input'] if where == 'frame' else
                        manifest['observations']['gesture_input']['receipts'][0]['after'])
                    self.assertEqual(self.run_capture()['status'], 'INVALID')
        for index, change in enumerate((lambda data: data.update(execute_tick=1),
                                         lambda data: data.update(owner=''),
                                         lambda data: data.update(payload={}),
                                         lambda data: data.update(payload={'Select': None}),
                                         lambda data: data.update(extra=True))):
            self.output = self.root / f'bad-observed-command-{index}'
            self.change = lambda manifest, f=change: f(
                manifest['observations']['gesture_input']['receipts'][0]['queued_commands'][0])
            self.assertEqual(self.run_capture()['status'], 'INVALID')
        self.output = self.root / 'input-budget'
        self.change = lambda manifest: None
        with patch.object(observation, 'MAX_OBSERVATION_SAMPLES', 1):
            report = self.run_capture()
            self.assertEqual(report['status'], 'INVALID', report)
            self.assertIn('sample budget', report['errors'][0])

    def test_gesture_extension_requires_matching_presence_and_current_child_policy(self):
        self.gesture_profile()
        changes = [lambda manifest: manifest['observations'].pop('gesture_input'),
                   lambda manifest: manifest['observations']['frames'][0].pop('input')]
        for index, change in enumerate(changes):
            self.output = self.root / f'missing-gesture-extension-{index}'
            self.change = change
            self.assertEqual(self.run_capture()['status'], 'INVALID')
        with self.assertRaises(ValidationError):
            observation._observations({}, self.profile, {}, walk_state=False)
        self.profile.pop('gestures')
        self.profile_path.write_text(json.dumps(self.profile))
        self.output = self.root / 'unrequested-gesture-extension'
        self.change = lambda manifest: manifest['observations'].update(gesture_input={})
        self.assertEqual(self.run_capture()['status'], 'INVALID')

    def test_cursor_position_preserves_profile_presence_and_binds_actual_render_position(self):
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertNotIn('cursor_position', report['capture'])
        self.cursor_profile()
        self.output = self.root / 'positioned-cursor'
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertEqual(report['capture']['cursor_position'], [2.0, 1.0])
        sealed = json.loads((self.output / 'profile.json').read_text())
        self.assertEqual(sealed, self.profile)
        self.assertEqual(observation.validate_run(self.output)['status'], 'VALID')

    def test_cursor_position_requires_v2_integer_pair_inside_the_outermost_pixel(self):
        modern = dict(self.profile, schema_version=observation.PROFILE_V2, width=800, height=600)
        for position in ([1, 1], [798, 598], [720, 556]):
            observation._profile_extensions(dict(modern, cursor_position=position))
        with self.assertRaises(ValidationError):
            observation._profile_extensions(dict(modern, schema_version=observation.PROFILE_V1,
                                                 cursor_position=[720, 556]))
        for position in (None, [], [1], [1, 1, 1], [True, 1], [1, False], [1.0, 1],
                         ['1', 1], [-1, 1], [0, 1], [1, 0], [799, 1], [1, 599],
                         [800, 1], [1, 600], [1 << 32, 1]):
            with self.subTest(position=position), self.assertRaises(ValidationError):
                observation._profile_extensions(dict(modern, cursor_position=position))

    def test_cursor_position_receipt_rejects_absence_changes_or_unrequested_position(self):
        self.cursor_profile()
        for index, position in enumerate((None, [1.0, 1.0], [2.25, 1.0], [True, 1.0],
                                           [2, 1], [], [2.0, 1.0, 0.0])):
            with self.subTest(position=position):
                self.output = self.root / f'bad-cursor-{index}'
                self.change = lambda manifest, value=position: manifest['render'].update(cursor_position=value)
                report = self.run_capture()
                self.assertEqual(report['status'], 'INVALID', report)
                self.assertIn('cursor_position', report['errors'][0])
        self.output = self.root / 'missing-cursor'
        self.change = lambda manifest: manifest['render'].pop('cursor_position')
        self.assertEqual(self.run_capture()['status'], 'INVALID')
        self.profile.pop('cursor_position')
        self.profile_path.write_text(json.dumps(self.profile))
        self.output = self.root / 'unrequested-cursor'
        self.change = lambda manifest: manifest['render'].update(cursor_position=[2.0, 1.0])
        self.assertEqual(self.run_capture()['status'], 'INVALID')

    def test_frame_wall_mean_is_optional_metadata_and_not_deterministic_comparison(self):
        self.change = lambda manifest: manifest['render'].update(frame_wall_mean_ms=0.0)
        before = self.output
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertEqual(report['capture']['frame_wall_mean_ms'], 0.0)
        self.output = self.root / 'different-wall-cadence'
        self.change = lambda manifest: manifest['render'].update(frame_wall_mean_ms=16.25)
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertEqual(report['capture']['frame_wall_mean_ms'], 16.25)
        self.assertEqual(observation.validate_run(self.output)['status'], 'VALID')
        self.assertEqual(observation.compare_runs(before, self.output)['status'], 'MATCH')

    def test_frame_wall_mean_rejects_nonfinite_negative_or_nonnumeric_metadata(self):
        for index, value in enumerate((None, True, -1, float('nan'), float('inf'), '16', [], {})):
            with self.subTest(value=value):
                self.output = self.root / f'bad-cadence-{index}'
                self.change = lambda manifest, value=value: manifest['render'].update(frame_wall_mean_ms=value)
                self.assertEqual(self.run_capture()['status'], 'INVALID')

    def test_pending_entry_projection_round_trips_null_and_stable_target(self):
        self.scripted_profile()
        for step, actors in self.actor_frames.items():
            actors[0]['foot']['pending_entry_500'] = 9 if step < 2 else None
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        frames = report['capture']['observations']['frames']
        self.assertEqual([row['actors'][0]['foot']['pending_entry_500'] for row in frames],
                         [9, 9, None, None])
        self.assertEqual(observation.validate_run(self.output)['status'], 'VALID')

    def test_pending_entry_projection_rejects_nonidentity_values(self):
        for value in (True, 0, -1, '9', 1 << 64):
            with self.subTest(value=value), self.assertRaises(ValidationError):
                actor = self.actor()
                actor['foot']['pending_entry_500'] = value
                observation._actor(actor, 'actor')

    @staticmethod
    def action_line_inputs():
        # Schema fixture only; native coordinate values come from the original
        # executable, never this portable fake-child receipt.
        return {'body_facing': 65535, 'turret_facing': None, 'locomotor': 'Drive',
                'is_moving': True, 'applied_speed_fraction_fixed_bits': 32768,
                'crate_speed_multiplier_f64_bits': 4607182418800017408,
                'house_speed_bonus_f32_bits': 1065353216, 'current_speed': 14,
                'veterancy': 512, 'current_weapon': '105mm', 'turret_offset': -80,
                'rocking_angles_fixed_bits': [0, 0]}

    def test_action_line_inputs_are_opt_in_and_round_trip_nullable_owner_state(self):
        self.scripted_profile()
        self.profile['observe_action_line_inputs'] = True
        self.profile_path.write_text(json.dumps(self.profile))
        for step, actors in self.actor_frames.items():
            actors[0]['action_line_inputs'] = self.action_line_inputs()
            if step == 2:
                actors[0]['action_line_inputs'].update(
                    locomotor=None, is_moving=None, current_weapon=None,
                    rocking_angles_fixed_bits=None)
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        for frame in report['capture']['observations']['frames']:
            expected = self.actor_frames[frame['completed_steps']][0]['action_line_inputs']
            self.assertEqual(frame['actors'][0]['action_line_inputs'], expected)
        self.assertEqual(observation.validate_run(self.output)['status'], 'VALID')
        self.assertEqual(json.loads((self.output / 'profile.json').read_text()), self.profile)

    def test_action_line_inputs_profile_rejects_v1_presence_and_nonboolean_values(self):
        self.scripted_profile()
        for enabled in (False, True):
            observation._profile_extensions(dict(self.profile, observe_action_line_inputs=enabled))
            with self.assertRaises(ValidationError):
                observation._profile_extensions(dict(self.profile,
                    schema_version=observation.PROFILE_V1, observe_action_line_inputs=enabled))
        for value in (None, 0, 1, 'true', [], {}):
            with self.subTest(value=value), self.assertRaises(ValidationError):
                observation._profile_extensions(dict(self.profile, observe_action_line_inputs=value))

    def test_action_line_inputs_require_exact_presence_and_keep_structure_null(self):
        actor = self.actor()
        with self.assertRaises(ValidationError):
            observation._actor(actor, 'actor', action_line_inputs=True)
        actor['action_line_inputs'] = self.action_line_inputs()
        with self.assertRaises(ValidationError):
            observation._actor(actor, 'actor')
        observation._actor(actor, 'actor', action_line_inputs=True)
        actor['action_line_inputs'] = None
        with self.assertRaises(ValidationError):
            observation._actor(actor, 'actor', action_line_inputs=True)
        structure = self.actor(category='Structure')
        structure['action_line_inputs'] = None
        observation._actor(structure, 'actor', action_line_inputs=True)
        structure['action_line_inputs'] = self.action_line_inputs()
        with self.assertRaises(ValidationError):
            observation._actor(structure, 'actor', action_line_inputs=True)

    def test_action_line_inputs_reject_malformed_getter_receipts(self):
        cases = [('body_facing', -1), ('turret_facing', 65536), ('locomotor', 'drive'),
                 ('is_moving', 1), ('applied_speed_fraction_fixed_bits', 65537),
                 ('crate_speed_multiplier_f64_bits', 1 << 64),
                 ('house_speed_bonus_f32_bits', True), ('current_speed', 1 << 31),
                 ('veterancy', 65536), ('current_weapon', ''), ('turret_offset', 1.0),
                 ('rocking_angles_fixed_bits', [0]), ('rocking_angles_fixed_bits', [False, 0])]
        for key, value in cases:
            with self.subTest(key=key, value=value), self.assertRaises(ValidationError):
                inputs = dict(self.action_line_inputs(), **{key: value})
                observation._action_line_inputs(inputs, 'Unit', 'actor.action_line_inputs')
        inputs = self.action_line_inputs()
        inputs.pop('current_speed')
        with self.assertRaises(ValidationError):
            observation._action_line_inputs(inputs, 'Unit', 'actor.action_line_inputs')

    def test_type_filter_binds_discovery_and_retains_identity_after_type_and_owner_change(self):
        self.scripted_profile()
        self.profile['observe_types'] = ['E1']
        self.profile_path.write_text(json.dumps(self.profile))
        self.rule_types.append({'type_id': 'E1', 'interned_id': 43, 'category': 'Infantry'})
        self.actor_frames[0] = []
        self.actor_frames[2][0].update(type_id='MTNK', owner='OtherHouse')
        self.actor_frames[3] = []
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        observed = report['capture']['observations']
        self.assertEqual(observed['type_filter'], ['E1'])
        self.assertEqual(observed['frames'][3]['missing_actor_ids'], [1])
        self.assertEqual(observation.validate_run(self.output)['status'], 'VALID')

    def test_type_filter_rejects_unrequested_discovery_and_mismatched_manifest(self):
        self.scripted_profile()
        self.profile['observe_types'] = ['E1']
        self.profile_path.write_text(json.dumps(self.profile))
        self.rule_types.append({'type_id': 'E1', 'interned_id': 43, 'category': 'Infantry'})
        self.actor_frames[0][0]['type_id'] = 'MTNK'
        report = self.run_capture()
        self.assertEqual(report['status'], 'INVALID')
        self.assertTrue(any('type filter' in error for error in report['errors']))
        self.actor_frames[0][0]['type_id'] = 'E1'
        self.output = self.root / 'mismatched-filter'
        self.change = lambda manifest: manifest['observations'].update(type_filter=['MTNK'])
        report = self.run_capture()
        self.assertEqual(report['status'], 'INVALID')
        self.assertTrue(any('type_filter' in error for error in report['errors']))

    def test_type_filter_requires_literal_registry_names_and_a_nonempty_unique_list(self):
        self.scripted_profile()
        for value in ([], [''], ['E1', 'E1'], None, [1], ['E1'] * 257):
            with self.subTest(value=value), self.assertRaises(ValidationError):
                observation._profile_extensions({**self.profile, 'observe_types': value})
        self.profile['observe_types'] = ['e1']
        self.profile_path.write_text(json.dumps(self.profile))
        self.rule_types.append({'type_id': 'E1', 'interned_id': 43, 'category': 'Infantry'})
        report = self.run_capture()
        self.assertEqual(report['status'], 'INVALID')
        self.assertTrue(any('absent from rule_types' in error for error in report['errors']))

    def production_profile(self):
        self.scripted_profile()
        self.profile['commands'] = [
            {'issue_after_step': 0, 'owner': 'Computer1', 'payload': {'QueueProduction': {'type_id': 41}}},
            {'issue_after_step': 2, 'owner': 'Computer1',
             'payload': {'PlaceReadyBuilding': {'type_id': 41, 'rx': 87, 'ry': 53}}}]
        self.profile_path.write_text(json.dumps(self.profile))
        self.actor_frames = {step: [self.actor(category='Structure')] for step in range(4)}
        for actors in self.actor_frames.values():
            actors[0]['type_id'] = 'GAPOWR'

    def test_production_commands_use_recorded_handles_and_keep_building_animation_state(self):
        self.production_profile()
        self.actor_frames[3][0]['building']['last_operational'] = True
        self.actor_frames[3][0]['building']['animation_slots'][0]['anim_id'] = 101
        self.actor_frames[3][0]['building']['animation_slots'][0]['animation']['stable_id'] = 101
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertEqual(report['schema_version'], 'vera20k.map-observation-run.v7')
        observed = report['capture']['observations']
        self.assertEqual(observed['rule_types'], self.rule_types)
        self.assertEqual([row['payload'] for row in observed['commands']],
                         [row['payload'] for row in self.profile['commands']])
        self.assertEqual(observed['frames'][3]['actors'][0]['building'],
                         self.actor_frames[3][0]['building'])
        self.assertEqual(observation.validate_run(self.output)['status'], 'VALID')

    def test_engineer_and_paid_repair_orders_keep_the_existing_command_payload(self):
        self.production_profile()
        self.profile['commands'] = [
            {'issue_after_step': 0, 'owner': 'Computer1',
             'payload': {'ToggleRepair': {'entity_id': 1}}},
            {'issue_after_step': 2, 'owner': 'Computer1',
             'payload': {'CaptureBuilding': {'engineer_id': 7, 'target_building_id': 1}}}]
        self.profile_path.write_text(json.dumps(self.profile))
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertEqual([row['payload'] for row in report['capture']['observations']['commands']],
                         [row['payload'] for row in self.profile['commands']])
        self.assertEqual(observation.validate_run(self.output)['status'], 'VALID')

    def test_depot_repair_and_sale_orders_keep_payloads_and_fail_changed_receipts(self):
        self.production_profile()
        self.profile['commands'] = [
            {'issue_after_step': 0, 'owner': 'Computer1',
             'payload': {'RepairAtDepot': {'entity_id': 7, 'depot_id': 1}}},
            {'issue_after_step': 2, 'owner': 'Computer1',
             'payload': {'SellBuilding': {'entity_id': 9}}}]
        self.profile_path.write_text(json.dumps(self.profile))
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertEqual([row['payload'] for row in report['capture']['observations']['commands']],
                         [row['payload'] for row in self.profile['commands']])
        self.assertEqual(observation.validate_run(self.output)['status'], 'VALID')
        for index, variant, key in ((0, 'RepairAtDepot', 'depot_id'),
                                    (1, 'SellBuilding', 'entity_id')):
            with self.subTest(variant=variant):
                self.output = self.root / f'changed-{variant}'
                self.change = lambda m, i=index, v=variant, k=key: \
                    m['observations']['commands'][i]['payload'][v].update({k: 99})
                self.assertEqual(self.run_capture()['status'], 'INVALID')

    def test_rally_order_keeps_existing_producer_ids_and_coordinate_payload(self):
        self.production_profile()
        self.profile['commands'] = [
            {'issue_after_step': 0, 'owner': 'Computer1',
             'payload': {'SetRally': {'producer_ids': [1], 'rx': 87, 'ry': 53}}}]
        self.profile_path.write_text(json.dumps(self.profile))
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertEqual([row['payload'] for row in report['capture']['observations']['commands']],
                         [row['payload'] for row in self.profile['commands']])
        self.assertEqual(observation.validate_run(self.output)['status'], 'VALID')


    def test_building_and_rule_handle_receipts_reject_wrong_types_or_unknown_fields(self):
        self.production_profile()
        changes = [lambda m: m['observations'].pop('rule_types'),
                   lambda m: m['observations']['rule_types'][0].update(interned_id=True),
                   lambda m: m['observations']['rule_types'][0].update(category='Unknown'),
                   lambda m: m['observations']['rule_types'][0].update(type_id=''),
                   lambda m: m['observations']['rule_types'][0].update(extra=0)]
        building_changes = [lambda b: b.update(body_state=True),
                            lambda b: b.update(ready_latch=256),
                            lambda b: b.update(actually_placed=1),
                            lambda b: b['stage']['timer'].update(start_frame=1.5),
                            lambda b: b['stage'].update(changed=False),
                            lambda b: b.update(construction_control=[0, 25]),
                            lambda b: b['animation_slots'][0].update(slot=21),
                            lambda b: b['animation_slots'].append(deepcopy(b['animation_slots'][0])),
                            lambda b: b['animation_slots'][0]['animation'].update(stable_id=101),
                            lambda b: b['animation_slots'][0]['animation']['runtime'].update(first_ai_guard=0),
                            lambda b: b['animation_slots'][0]['animation']['runtime'].update(rate_reload=65536),
                            lambda b: b['animation_slots'][0]['animation']['runtime'].update(loop_remaining=-1)]
        changes += [lambda m, change=change: change(m['observations']['frames'][1]['actors'][0]['building'])
                    for change in building_changes]
        for index, change in enumerate(changes):
            with self.subTest(index=index):
                self.output = self.root / f'building-state-invalid-{index}'
                self.change = change
                self.assertEqual(self.run_capture()['status'], 'INVALID')

    def test_building_voxel_gun_extension_is_optional_complete_and_finite(self):
        building = self.building()
        observation._building(building, 'building')
        gun = {'facing': 16384, 'elevation': 14336, 'hva_counter': 0,
               'recoil': [0.0, 8.0], 'recoil_active': True}
        building['voxel_gun'] = gun
        observation._building(building, 'building')
        changes = [lambda g: g.pop('recoil'), lambda g: g.update(facing=65536),
                   lambda g: g.update(elevation=True), lambda g: g.update(recoil=[0.0]),
                   lambda g: g.update(recoil=[0.0, float('nan')]),
                   lambda g: g.update(recoil=[True, 0.0]), lambda g: g.update(recoil_active=1)]
        for change in changes:
            invalid = deepcopy(building)
            change(invalid['voxel_gun'])
            with self.assertRaises(observation.ValidationError):
                observation._building(invalid, 'building')

    def test_missing_live_animation_is_explicit_and_slots_count_towards_sample_budget(self):
        self.production_profile()
        self.actor_frames[1][0]['building']['animation_slots'][0]['animation'] = None
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertIsNone(report['capture']['observations']['frames'][1]['actors'][0]
                          ['building']['animation_slots'][0]['animation'])
        self.output = self.root / 'bounded-building-slots'
        # Four actors + four terrain + four Houses + four occupied slots.
        with patch.object(observation, 'MAX_OBSERVATION_SAMPLES', 15):
            self.assertEqual(self.run_capture()['status'], 'INVALID')

    def test_comparison_includes_rule_handles_and_animation_lifetime_and_runtime(self):
        self.production_profile()
        before = self.valid_capture('building-before')
        self.change = lambda m: m['observations']['frames'][1]['actors'][0]['building'] \
            ['animation_slots'][0]['animation']['runtime'].update(current_frame=1)
        after = self.valid_capture('building-after')
        report = observation.compare_runs(before, after)
        self.assertEqual(report['status'], 'MISMATCH', report['errors'])
        self.assertEqual([row['field'] for row in report['differences']],
                         ['observations.frames[1].actors[0].building.animation_slots[0].animation.runtime.current_frame'])
        self.change = lambda m: m['observations']['rule_types'][0].update(interned_id=70)
        after = self.valid_capture('rule-handle-after')
        report = observation.compare_runs(before, after)
        self.assertEqual(report['status'], 'MISMATCH', report['errors'])
        self.assertEqual([row['field'] for row in report['differences']],
                         ['observations.rule_types[0].interned_id'])

    def test_building_and_terrain_resource_receipts_validate_and_compare_together(self):
        self.production_profile()
        cell = self.unallocated_cell([87, 53])
        cell.update(overlay={'id': None, 'density': 3},
                    terrain_object={'name': 'TIBTRE02', 'frame': 10, 'active': True})
        self.terrain_frames[1] = [cell]
        before = self.valid_capture('building-terrain-before')
        checked = observation.validate_run(before)
        self.assertEqual(checked['status'], 'VALID', checked['errors'])
        frame = checked['capture']['observations']['frames'][1]
        self.assertEqual(frame['actors'][0]['building'], self.actor_frames[1][0]['building'])
        self.assertEqual(frame['terrain'], [cell])

        def change(manifest):
            frame = manifest['observations']['frames'][1]
            frame['actors'][0]['building']['animation_slots'][0]['animation'] \
                ['runtime']['frame_timer']['duration'] = 5
            frame['terrain'][0]['overlay']['density'] = 4

        self.change = change
        after = self.valid_capture('building-terrain-after')
        report = observation.compare_runs(before, after)
        self.assertEqual(report['status'], 'MISMATCH', report['errors'])
        self.assertEqual([row['field'] for row in report['differences']], [
            'observations.frames[1].actors[0].building.animation_slots[0].animation.runtime.frame_timer.duration',
            'observations.frames[1].terrain[0].overlay.density'])

    def test_terrain_visibility_is_retained_and_compared(self):
        self.production_profile()
        cell = self.unallocated_cell([87, 53])
        cell['local_visibility'] = dict(owner='Observer', revealed=False,
                                        visible=False, gap_covered=False)
        self.terrain_frames[1] = [cell]
        before = self.valid_capture('visibility-before')
        def change(manifest):
            manifest['observations']['frames'][1]['terrain'][0]['local_visibility']['revealed'] = True
        self.change = change
        after = self.valid_capture('visibility-after')
        report = observation.compare_runs(before, after)
        self.assertEqual(report['status'], 'MISMATCH', report['errors'])
        self.assertEqual([row['field'] for row in report['differences']],
                         ['observations.frames[1].terrain[0].local_visibility.revealed'])

    def test_terrain_visibility_accepts_legacy_or_missing_viewer_and_rejects_bad_states(self):
        cell = self.unallocated_cell([87, 53])
        observation._terrain(cell, [87, 53], 'terrain')
        cell['local_visibility'] = None
        observation._terrain(cell, [87, 53], 'terrain')
        visibility = dict(owner='Observer', revealed=False, visible=False, gap_covered=False)
        for key, value in [('owner', ''), ('owner', 7), ('revealed', 0),
                           ('visible', None), ('gap_covered', 'false')]:
            with self.subTest(key=key, value=value):
                cell['local_visibility'] = dict(visibility, **{key: value})
                with self.assertRaises(ValidationError):
                    observation._terrain(cell, [87, 53], 'terrain')

    def refinery_profile(self):
        self.scripted_profile()
        self.actor_frames = {step: [self.actor(category='Unit')] for step in range(4)}
        for step, actors in self.actor_frames.items():
            actors[0].update(type_id='HARV', miner={
                'cargo_bales': 40 if step < 2 else step - 2,
                'capacity_bales': 40, 'unload_active': step == 1,
                'harvesting': step == 3}, radio={
                'contacts': [None, 99, None] if step < 2 else [None, None, None],
                'dock_entered_with': 99 if step == 1 else None})
            self.house_frames[step] = [{'owner': 'Computer1', 'economy': {
                'credits': 7000 if step < 2 else 8000,
                'spent_credits': 3000,
                'harvested_credits': 0 if step < 2 else 200}}]

    def test_refinery_cargo_radio_and_house_receipts_remain_exact_observations(self):
        self.refinery_profile()
        before = self.valid_capture('refinery-before')
        checked = observation.validate_run(before)
        self.assertEqual(checked['status'], 'VALID', checked['errors'])
        frames = checked['capture']['observations']['frames']
        self.assertEqual(frames[1]['actors'][0]['radio'], {
            'contacts': [None, 99, None], 'dock_entered_with': 99})
        self.assertEqual(frames[2]['houses'], self.house_frames[2])
        self.assertEqual([row['actors'][0]['miner']['cargo_bales'] for row in frames], [40, 40, 0, 1])
        self.change = lambda m: m['observations']['frames'][2]['houses'][0]['economy'].update(credits=7999)
        after = self.valid_capture('refinery-after')
        report = observation.compare_runs(before, after)
        self.assertEqual(report['status'], 'MISMATCH', report['errors'])
        self.assertEqual([row['field'] for row in report['differences']],
                         ['observations.frames[2].houses[0].economy.credits'])
        self.change = lambda m: m['observations']['frames'][1]['actors'][0]['radio'].update(
            contacts=[99, None, None])
        after = self.valid_capture('refinery-contact-after')
        report = observation.compare_runs(before, after)
        self.assertEqual(report['status'], 'MISMATCH', report['errors'])
        self.assertEqual([row['field'] for row in report['differences']], [
            'observations.frames[1].actors[0].radio.contacts[0]',
            'observations.frames[1].actors[0].radio.contacts[1]'])

    def test_refinery_receipts_reject_missing_fields_types_and_wrong_house_order(self):
        self.refinery_profile()
        actor_changes = [lambda a: a.pop('miner'), lambda a: a.pop('radio'),
                         lambda a: a['miner'].update(cargo_bales=True),
                         lambda a: a['miner'].update(capacity_bales=65536),
                         lambda a: a['miner'].update(unload_active=1),
                         lambda a: a['miner'].update(harvesting=0),
                         lambda a: a['miner'].update(extra=0),
                         lambda a: a['radio'].update(contacts=[]),
                         lambda a: a['radio'].update(contacts=[True]),
                         lambda a: a['radio'].update(contacts=[0]),
                         lambda a: a['radio'].update(dock_entered_with=1.5),
                         lambda a: a['radio'].update(extra=0)]
        changes = [lambda m, change=change: change(m['observations']['frames'][1]['actors'][0])
                   for change in actor_changes]
        changes += [lambda m: m['observations']['frames'][1].pop('houses'),
                    lambda m: m['observations']['frames'][1].update(houses=[]),
                    lambda m: m['observations']['frames'][1]['houses'][0].update(owner='OtherHouse'),
                    lambda m: m['observations']['frames'][1]['houses'][0]['economy'].update(credits=True),
                    lambda m: m['observations']['frames'][1]['houses'][0]['economy'].update(spent_credits=1 << 31),
                    lambda m: m['observations']['frames'][1]['houses'][0]['economy'].update(harvested_credits=0.5),
                    lambda m: m['observations']['frames'][1]['houses'][0]['economy'].update(extra=0)]
        for index, change in enumerate(changes):
            with self.subTest(index=index):
                self.output = self.root / f'refinery-state-invalid-{index}'
                self.change = change
                self.assertEqual(self.run_capture()['status'], 'INVALID')

    def test_house_observations_count_towards_budget_and_absence_stays_explicit(self):
        self.scripted_profile()
        self.profile['terrain_cells'] = []
        self.profile_path.write_text(json.dumps(self.profile))
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertEqual(report['capture']['observations']['frames'][0]['houses'],
                         [{'owner': 'Computer1', 'economy': None}])
        self.output = self.root / 'bounded-house-rows'
        # Four actors and four explicitly unavailable Houses still cost eight samples.
        with patch.object(observation, 'MAX_OBSERVATION_SAMPLES', 7):
            self.assertEqual(self.run_capture()['status'], 'INVALID')

    def test_super_weapon_rows_are_opt_in_ordered_typed_and_count_towards_budget(self):
        self.scripted_profile()
        self.profile['observe_super_weapons'] = True
        self.profile['commands'].append({'issue_after_step': 2, 'owner': 'Computer1', 'payload': {
            'LaunchSuperWeapon': {'sw_type_id': 2500, 'target_rx': 87, 'target_ry': 53}}})
        self.profile_path.write_text(json.dumps(self.profile))
        nuke = {'type': 'NukeSpecial', 'interned_id': 2500, 'granted': True, 'ready': True,
                'on_hold': False, 'charge_start': 0, 'charge_duration': 9000, 'remaining': 0,
                'fade_countdown': -1, 'fade_coords': [0, 0, 0]}
        self.house_frames = {step: [{'owner': 'Computer1', 'economy': None,
                                     'super_weapons': [dict(nuke, ready=step < 3)]}]
                             for step in range(4)}
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertEqual(report['capture']['observations']['frames'][3]['houses'][0]['super_weapons'],
                         [dict(nuke, ready=False)])
        rows = lambda m: m['observations']['frames'][1]['houses'][0]
        changes = [lambda m: rows(m).pop('super_weapons'),
                   lambda m: rows(m)['super_weapons'][0].pop('remaining'),
                   lambda m: rows(m)['super_weapons'][0].update(extra=0),
                   lambda m: rows(m)['super_weapons'][0].update(ready=1),
                   lambda m: rows(m)['super_weapons'][0].update(type=''),
                   lambda m: rows(m)['super_weapons'][0].update(charge_start=1 << 31),
                   lambda m: rows(m)['super_weapons'][0].update(fade_coords=[0, 0]),
                   lambda m: rows(m)['super_weapons'].append(dict(nuke))]
        for index, change in enumerate(changes):
            with self.subTest(index=index):
                self.output = self.root / f'super-weapon-invalid-{index}'
                self.change = change
                self.assertEqual(self.run_capture()['status'], 'INVALID')
        self.change = lambda manifest: None
        # Four frames of one actor, terrain cell, House and Super cost sixteen
        # samples; without the Super rows they would fit in twelve.
        for budget, status in ((15, 'INVALID'), (16, 'VALID')):
            self.output = self.root / f'super-weapon-budget-{budget}'
            with patch.object(observation, 'MAX_OBSERVATION_SAMPLES', budget):
                self.assertEqual(self.run_capture()['status'], status)
        self.profile.pop('observe_super_weapons')
        self.profile_path.write_text(json.dumps(self.profile))
        self.output = self.root / 'unrequested-super-weapon-rows'
        self.assertEqual(self.run_capture()['status'], 'INVALID')

    def test_paid_walk_receipts_retain_head_destination_moving_and_cleanup(self):
        self.scripted_profile()
        self.actor_frames[1][0]['foot'].update(
            walk_head_leptons=[22400, 13696, 416],
            walk_destination_leptons=[22528, 13824, 416], walk_is_moving=True)
        # A stopped Walk may retain its paid head until that step completes.
        self.actor_frames[2][0]['foot'].update(
            walk_head_leptons=[22400, 13696, 416], walk_is_moving=False)
        self.actor_frames[3][0]['foot'].update(walk_is_moving=False)
        before = self.valid_capture('walk-before')
        checked = observation.validate_run(before)
        self.assertEqual(checked['status'], 'VALID', checked['errors'])
        frames = checked['capture']['observations']['frames']
        for step in range(4):
            self.assertEqual(frames[step]['actors'][0]['foot'], self.actor_frames[step][0]['foot'])
        self.assertIsNone(frames[0]['actors'][0]['foot']['walk_is_moving'])
        self.assertIs(frames[3]['actors'][0]['foot']['walk_is_moving'], False)

        def change(manifest):
            foot = manifest['observations']['frames'][1]['actors'][0]['foot']
            foot['walk_head_leptons'][0] += 1
            foot['walk_destination_leptons'][2] += 1
            foot['walk_is_moving'] = False

        self.change = change
        after = self.valid_capture('walk-after')
        compared = observation.compare_runs(before, after)
        self.assertEqual(compared['status'], 'MISMATCH', compared['errors'])
        self.assertCountEqual([row['field'] for row in compared['differences']], [
            'observations.frames[1].actors[0].foot.walk_head_leptons[0]',
            'observations.frames[1].actors[0].foot.walk_destination_leptons[2]',
            'observations.frames[1].actors[0].foot.walk_is_moving'])

    def test_walk_receipts_reject_missing_fields_bad_types_and_unavailable_coordinates(self):
        self.scripted_profile()
        self.actor_frames[1][0]['foot'].update(walk_is_moving=True)
        changes = []
        for key in ('walk_head_leptons', 'walk_destination_leptons', 'walk_is_moving'):
            changes.append(lambda foot, k=key: foot.pop(k))
        for key in ('walk_head_leptons', 'walk_destination_leptons'):
            for value in ([], [1, 2], [1, 2, 3, 4], [True, 2, 3], [1, 2.0, 3],
                          [1, 2, 1 << 31], [-(1 << 31) - 1, 2, 3], '1,2,3', {}):
                changes.append(lambda foot, k=key, v=value: foot.update({k: v}))
            changes.append(lambda foot, k=key: foot.update({k: [1, 2, 3], 'walk_is_moving': None}))
        for value in (0, 1, 'true', [], {}):
            changes.append(lambda foot, v=value: foot.update(walk_is_moving=v))
        for index, change in enumerate(changes):
            with self.subTest(case=index):
                self.output = self.root / f'bad-walk-{index}'
                self.change = lambda m, f=change: f(m['observations']['frames'][1]['actors'][0]['foot'])
                report = self.run_capture()
                self.assertEqual(report['status'], 'INVALID', report)
                self.assertIn('.foot', report['errors'][0])

    def test_track_receipts_preserve_paid_head_through_stop_and_compare_owner_fields(self):
        self.scripted_profile()
        self.actor_frames = {step: [self.actor(category='Unit')] for step in range(4)}
        self.actor_frames[0][0]['foot']['track'] = None
        self.actor_frames[1][0]['foot']['track'] = {
            'family': 'Drive', 'destination_leptons': [22528, 13824, 416],
            'head_leptons': [22400, 13696, 416], 'selector': 28, 'cursor': 2, 'valid': True}
        self.actor_frames[2][0]['foot']['track'] = {
            **self.actor_frames[1][0]['foot']['track'], 'destination_leptons': None}
        self.actor_frames[3][0]['foot']['track'] = {
            'family': 'Drive', 'destination_leptons': None, 'head_leptons': None,
            'selector': -1, 'cursor': 0, 'valid': False}
        before = self.valid_capture('track-before')
        checked = observation.validate_run(before)
        self.assertEqual(checked['status'], 'VALID', checked['errors'])
        for step, frame in enumerate(checked['capture']['observations']['frames']):
            self.assertEqual(frame['actors'][0]['foot']['track'],
                             self.actor_frames[step][0]['foot']['track'])

        def change(manifest):
            track = manifest['observations']['frames'][1]['actors'][0]['foot']['track']
            track['head_leptons'][0] += 1
            track['destination_leptons'][2] += 1
            track['cursor'] += 1
            track['valid'] = False

        self.change = change
        after = self.valid_capture('track-after')
        compared = observation.compare_runs(before, after)
        self.assertEqual(compared['status'], 'MISMATCH', compared['errors'])
        self.assertCountEqual([row['field'] for row in compared['differences']], [
            'observations.frames[1].actors[0].foot.track.head_leptons[0]',
            'observations.frames[1].actors[0].foot.track.destination_leptons[2]',
            'observations.frames[1].actors[0].foot.track.cursor',
            'observations.frames[1].actors[0].foot.track.valid'])

    def test_track_receipts_reject_partial_fields_bad_types_and_unknown_owners(self):
        self.scripted_profile()
        self.actor_frames[1] = [self.actor(category='Unit')]
        self.actor_frames[1][0]['foot']['track'] = {
            'family': 'Ship', 'destination_leptons': None, 'head_leptons': [1, 2, 3],
            'selector': 0, 'cursor': 1, 'valid': True}
        changes = [lambda track: track.pop('cursor'),
                   lambda track: track.update(family='Walk'),
                   lambda track: track.update(selector=0.5),
                   lambda track: track.update(cursor=True),
                   lambda track: track.update(head_leptons=[1, 2]),
                   lambda track: track.update(destination_leptons=[1, 2, 1 << 31]),
                   lambda track: track.update(valid=1),
                   lambda track: track.update(extra=0)]
        for index, change in enumerate(changes):
            with self.subTest(case=index):
                self.output = self.root / f'bad-track-{index}'
                self.change = lambda m, f=change: f(
                    m['observations']['frames'][1]['actors'][0]['foot']['track'])
                report = self.run_capture()
                self.assertEqual(report['status'], 'INVALID', report)
                self.assertIn('.foot.track', report['errors'][0])

    def test_profile_version_order_field_types_and_budgets_are_checked_before_spawn(self):
        profile = deepcopy(self.profile)
        cases = [dict(profile, commands=[]), dict(profile, observe_owners=[]),
                 dict(profile, camera_cell=[87, 53]), dict(profile, terrain_cells=[]),
                 dict(profile, schema_version='unknown'), dict(profile, extra=True)]
        modern = dict(profile, schema_version=observation.PROFILE_V2)
        cases.extend(dict(modern, **extension) for extension in (
            {'commands': None}, {'observe_owners': None}, {'camera_cell': None}, {'terrain_cells': None},
            {'observe_owners': ['Computer1', 'Computer1']}, {'observe_owners': ['']},
            {'observe_owners': [1]}, {'observe_owners': [str(index) for index in range(31)]},
            {'terrain_cells': [[87, 53], [87, 53]]}, {'camera_cell': [True, 53]},
            {'camera_cell': [-1, 53]}, {'camera_cell': [65536, 53]},
            {'terrain_cells': [[index, 0] for index in range(257)]}))
        valid_command = {'issue_after_step': 0, 'owner': 'Computer1', 'payload': {'Stop': {'entity_id': 1}}}
        for key, value in (('issue_after_step', True), ('issue_after_step', 3),
                           ('issue_after_step', -1), ('issue_after_step', 0.0), ('owner', ''),
                           ('owner', 1), ('extra', True), ('payload', {}),
                           ('payload', {'UnknownOrder': {'entity_ids': [1], 'additive': False}})):
            cases.append(dict(modern, commands=[dict(valid_command, **{key: value})]))
        cases.append(dict(modern, commands=[dict(valid_command, issue_after_step=2), valid_command]))
        cases.append(dict(modern, commands=[valid_command] * 1025))
        for index, candidate in enumerate(cases):
            with self.subTest(case=index), patch.object(observation, 'run_child') as child:
                self.profile_path.write_text(json.dumps(candidate))
                with self.assertRaises(ValidationError):
                    self.run_capture()
                child.assert_not_called()
                self.assertFalse(self.output.exists())

    def test_command_receipts_cannot_reorder_retime_omit_or_change_payload(self):
        self.scripted_profile()
        changes = [lambda rows: rows.pop(), lambda rows: rows.reverse(),
                   lambda rows: rows[0].update(extra=True),
                   lambda rows: rows[0]['payload']['Stop'].update(entity_id=2),
                   lambda rows: rows[2].update(issue_after_step=1),
                   lambda rows: rows[2].update(issued_simulation_tick=3),
                   lambda rows: rows[2].update(envelope_execute_tick=4),
                   lambda rows: rows[0].update(owner='Computer2')]
        for key in ('ordinal', 'issue_after_step', 'issued_simulation_tick', 'envelope_execute_tick'):
            changes.append(lambda rows, key=key: rows[0].update({key: True}))
        for index, change in enumerate(changes):
            with self.subTest(case=index):
                self.output = self.root / f'command-receipt-{index}'
                self.change = lambda manifest, change=change: change(manifest['observations']['commands'])
                report = self.run_capture()
                self.assertEqual(report['status'], 'INVALID', report)
                self.assertTrue(any('observations.commands' in error for error in report['errors']))

    def test_terrain_resource_extension_accepts_native_frame_and_rejects_invalid_values(self):
        cell = self.unallocated_cell([74, 32])
        cell['overlay'] = {'id': None, 'density': 3}
        cell['terrain_object'] = {'name': 'TIBTRE02', 'frame': 10, 'active': True}
        observation._terrain(cell, [74, 32], 'terrain')
        for key, value in (('frame', True), ('frame', 1 << 31), ('active', 1), ('name', '')):
            bad = deepcopy(cell)
            bad['terrain_object'][key] = value
            with self.assertRaises(observation.ValidationError):
                observation._terrain(bad, [74, 32], 'terrain')
        for key, value in (('id', 256), ('density', -1), ('density', True)):
            bad = deepcopy(cell)
            bad['overlay'][key] = value
            with self.assertRaises(observation.ValidationError):
                observation._terrain(bad, [74, 32], 'terrain')
        for key in ('overlay', 'terrain_object'):
            bad = deepcopy(cell)
            bad.pop(key)
            with self.assertRaises(observation.ValidationError):
                observation._terrain(bad, [74, 32], 'terrain')
        cell['overlay'] = None
        cell['terrain_object'] = {'name': 'TREE01', 'frame': None, 'active': None}
        observation._terrain(cell, [74, 32], 'terrain')
        cell['terrain_object'] = None
        observation._terrain(cell, [74, 32], 'terrain')

    def test_unit_extension_retains_animation_lifetime_and_compares_observations(self):
        self.scripted_profile()
        self.actor_frames = {step: [dict(self.actor(category='Unit'), unit=self.unit())]
                             for step in range(4)}
        self.actor_frames[0][0]['unit']['deploy_anim_130'] = None
        self.actor_frames[2][0]['unit']['deploy_anim_130']['live'] = None
        before = self.valid_capture('unit-before')
        checked = observation.validate_run(before)
        self.assertEqual(checked['status'], 'VALID', checked['errors'])
        for index, frame in enumerate(checked['capture']['observations']['frames']):
            self.assertEqual(frame['actors'][0]['unit'], self.actor_frames[index][0]['unit'])
        self.assertEqual(checked['capture']['observations']['policy'], observation.OBSERVATION_POLICY)
        self.change = lambda m: m['observations']['frames'][1]['actors'][0]['unit'] \
            ['deploy_anim_130']['live'].update(frame=5)
        after = self.valid_capture('unit-after')
        report = observation.compare_runs(before, after)
        self.assertEqual(report['status'], 'MISMATCH', report['errors'])
        self.assertEqual([row['field'] for row in report['differences']],
                         ['observations.frames[1].actors[0].unit.deploy_anim_130.live.frame'])

    def test_transport_commands_and_signed_selection_remain_observations(self):
        self.scripted_profile()
        self.profile['commands'] = [
            {'issue_after_step': 0, 'owner': 'Computer1',
             'payload': {'EnterTransport': {'passenger_id': 2, 'transport_id': 1}}},
            {'issue_after_step': 0, 'owner': 'Computer1',
             'payload': {'Select': {'entity_ids': [1, 2], 'additive': False}}},
            {'issue_after_step': 2, 'owner': 'Computer1',
             'payload': {'UnloadPassengers': {'transport_id': 1}}}]
        self.profile_path.write_text(json.dumps(self.profile))
        unit = dict(self.unit(), current_weapon_138=17, current_turret_124=-1)
        self.actor_frames = {step: [dict(self.actor(category='Unit'), unit=dict(unit))]
                             for step in range(4)}
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertEqual(report['capture']['observations']['frames'][0]['actors'][0]['unit'], unit)
        unit['current_turret_124'] = True
        with self.assertRaises(observation.ValidationError):
            observation._unit(unit, 'unit')
        del unit['current_turret_124']
        with self.assertRaises(observation.ValidationError):
            observation._unit(unit, 'unit')

    def test_unit_extension_validates_storage_types_without_asserting_gameplay(self):
        actor = dict(self.actor(category='Unit'), unit=self.unit())
        # These are typed observations, including stale pointers and raw flag bytes.
        actor['unit'].update(deployed_6e0=255, deploying_6e1=255, undeploying_6e2=255,
                             landing_for_deploy_134=True, stage_f8=-(1 << 31),
                             body_counter_538=(1 << 32) - 1)
        anim = actor['unit']['deploy_anim_130']
        anim['stable_id'] = (1 << 64) - 1
        anim['live'].update(frame=-(1 << 31), owner_entity=(1 << 64) - 1)
        observation._actor(actor, 'actor')
        actor['unit']['stage_f8'] = (1 << 31) - 1
        anim['live'].update(frame=(1 << 31) - 1, owner_entity=None)
        observation._actor(actor, 'actor')
        anim['live'] = None
        observation._actor(actor, 'actor')
        actor['unit']['deploy_anim_130'] = None
        observation._actor(actor, 'actor')
        for category in ('Infantry', 'Aircraft', 'Structure'):
            with self.subTest(category=category):
                actor = dict(self.actor(category=category), unit=None)
                observation._actor(actor, 'actor')
                actor['unit'] = self.unit()
                with self.assertRaises(ValidationError):
                    observation._actor(actor, 'actor')

    def test_unit_extension_rejects_malformed_fields_and_animation_identity(self):
        actor = dict(self.actor(category='Unit'), unit=self.unit())
        integer_fields = [
            (('unit', key), 0, 255)
            for key in ('deployed_6e0', 'deploying_6e1', 'undeploying_6e2')]
        integer_fields += [
            (('unit', 'stage_f8'), -(1 << 31), (1 << 31) - 1),
            (('unit', 'body_counter_538'), 0, (1 << 32) - 1),
            (('unit', 'deploy_anim_130', 'stable_id'), 1, (1 << 64) - 1),
            (('unit', 'deploy_anim_130', 'live', 'frame'), -(1 << 31), (1 << 31) - 1),
            (('unit', 'deploy_anim_130', 'live', 'owner_entity'), 1, (1 << 64) - 1)]
        for path, minimum, maximum in integer_fields:
            for value in (True, False, 1.0, '1', minimum - 1, maximum + 1):
                with self.subTest(path=path, value=value):
                    bad = deepcopy(actor)
                    field = bad
                    for key in path[:-1]:
                        field = field[key]
                    field[path[-1]] = value
                    with self.assertRaises(ValidationError):
                        observation._actor(bad, 'actor')
        changes = [lambda a: a.update(unit=None),
                   lambda a: a['unit'].update(landing_for_deploy_134=0),
                   lambda a: a['unit'].update(landing_for_deploy_134=None),
                   lambda a: a['unit'].update(deploy_anim_130=[]),
                   lambda a: a['unit']['deploy_anim_130'].update(live=[]),
                   lambda a: a['unit']['deploy_anim_130']['live'].update(type_id=''),
                   lambda a: a['unit']['deploy_anim_130']['live'].update(type_id=1)]
        for path in (('unit',), ('unit', 'deploy_anim_130'),
                     ('unit', 'deploy_anim_130', 'live')):
            original = actor
            for key in path:
                original = original[key]
            for key in (*original, 'unknown'):
                def change(candidate, path=path, key=key):
                    field = candidate
                    for part in path:
                        field = field[part]
                    if key == 'unknown':
                        field[key] = None
                    else:
                        field.pop(key)
                changes.append(change)
        for index, change in enumerate(changes):
            with self.subTest(change=index):
                bad = deepcopy(actor)
                change(bad)
                with self.assertRaises(ValidationError):
                    observation._actor(bad, 'actor')

    def test_older_unit_receipts_remain_unchanged_without_optional_extension(self):
        self.scripted_profile()
        self.actor_frames = {step: [self.actor(category='Unit')] for step in range(4)}
        for historical in (False, True):
            with self.subTest(historical=historical):
                run = self.valid_capture(f'unit-without-extension-{historical}')
                if historical:
                    self.make_historical_trajectory(run)
                paths = (run / 'run.json', run / 'child-output/capture.json')
                originals = {path: path.read_bytes() for path in paths}
                checked = observation.validate_run(run)
                self.assertEqual(checked['status'], 'VALID', checked['errors'])
                for frame in checked['capture']['observations']['frames']:
                    self.assertNotIn('unit', frame['actors'][0])
                for path, original in originals.items():
                    self.assertEqual(path.read_bytes(), original)

    def test_actor_history_retains_capture_and_disappearance_without_rebinding(self):
        self.scripted_profile()
        self.actor_frames[1][0]['owner'] = 'Computer2'
        self.actor_frames[2] = []
        self.actor_frames[3] = [self.actor(identity=2)]
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        rows = report['capture']['observations']['frames']
        self.assertEqual(rows[1]['actors'][0]['owner'], 'Computer2')
        self.assertEqual(rows[2]['missing_actor_ids'], [1])
        self.assertEqual(rows[3]['missing_actor_ids'], [1])
        self.assertEqual(rows[3]['actors'][0]['stable_id'], 2)

    def test_actor_and_frame_transcripts_fail_closed_on_missing_order_or_bad_state(self):
        self.scripted_profile()
        changes = [lambda rows: rows.pop(), lambda rows: rows.reverse(),
                   lambda rows: rows[1].update(simulation_tick=2),
                   lambda rows: rows[1].update(total_simulation_ms=0),
                   lambda rows: rows[1].update(actors=[], missing_actor_ids=[]),
                   lambda rows: rows[1].update(missing_actor_ids=[1]),
                   lambda rows: rows[0]['actors'][0].update(owner='Computer2'),
                   lambda rows: rows[1]['actors'].append(deepcopy(rows[1]['actors'][0])),
                   lambda rows: rows[1]['actors'][0]['mission'].update(queued=True),
                   lambda rows: rows[1]['actors'][0]['foot'].update(firing_sequence_latch_68d=256),
                   lambda rows: rows[1]['actors'][0]['foot'].update(retarget_after_stop_688=1),
                   lambda rows: rows[1]['actors'][0]['foot'].update(infantry_doing=None),
                   lambda rows: rows[1]['actors'][0].update(nav={'Unknown': {'id': 9}}),
                   lambda rows: rows[1]['actors'][0].update(archive={'Cell': [True, 53]}),
                   lambda rows: rows[1]['actors'][0]['foot'].update(navigation_leptons=None),
                   lambda rows: rows[1]['actors'][0].update(physical_leptons=[1, 2]),
                   lambda rows: rows[1]['actors'][0].update(extra='unknown')]
        for index, change in enumerate(changes):
            with self.subTest(case=index):
                self.output = self.root / f'actor-receipt-{index}'
                self.change = lambda manifest, change=change: change(manifest['observations']['frames'])
                self.assertEqual(self.run_capture()['status'], 'INVALID')

    def test_allocated_terrain_and_explicit_unavailable_navigation_are_retained(self):
        self.scripted_profile()
        allocated = {'cell': [87, 53], 'allocated': True, 'final_tile_index': 700,
                     'final_sub_tile': 2, 'presentation_tile': [700, 2], 'level': 4, 'slope': 0,
                     'raw_bridge_flags': 256, 'bridge_state': 0, 'has_deck': True, 'deck_level': 8,
                     'walkable': True, 'transition': False}
        self.terrain_frames[1] = [allocated]
        self.actor_frames[1][0]['foot'].update(navigation_leptons=None,
                                              navigation_unavailable='active locomotor unavailable')
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertEqual(report['capture']['observations']['frames'][1]['terrain'], [allocated])
        changes = [lambda m: m['observations']['frames'][1]['terrain'][0].update(level=True),
                   lambda m: m['observations']['frames'][1]['terrain'][0].update(cell=[88, 53]),
                   lambda m: m['observations']['frames'][1]['terrain'][0].update(allocated=False),
                   lambda m: m['observations']['frames'][0]['terrain'][0].update(level=0),
                   lambda m: m['render']['camera'].update(requested_cell=[88, 53]),
                   lambda m: m['render']['camera'].update(zoom=0),
                   lambda m: m['render']['camera'].update(zoom=1 << 2048),
                   lambda m: m['render']['camera'].update(top_left=[True, 0]),
                   lambda m: m['render']['camera'].update(extra=True)]
        for index, change in enumerate(changes):
            self.output = self.root / f'terrain-camera-{index}'
            self.change = change
            self.assertEqual(self.run_capture()['status'], 'INVALID')

    def test_sample_budget_is_checked_and_comparison_includes_actor_trajectory(self):
        self.scripted_profile()
        with patch.object(observation, 'MAX_OBSERVATION_SAMPLES', 7):
            report = self.run_capture()
            self.assertEqual(report['status'], 'INVALID')
            self.assertTrue(any('sample budget' in error for error in report['errors']))
        before = self.valid_capture('trajectory-before')
        self.change = lambda m: m['observations']['frames'][1]['actors'][0]['mission'].update(handler_state=1)
        after = self.valid_capture('trajectory-after')
        report = observation.compare_runs(before, after)
        self.assertEqual(report['status'], 'MISMATCH', report['errors'])
        self.assertEqual([row['field'] for row in report['differences']],
                         ['observations.frames[1].actors[0].mission.handler_state'])
        self.assertNotIn('observations', report['before']['capture'])
        self.assertEqual(report['before']['observation_transcript']['frame_count'], 4)

    def test_complete_receipt_and_logs_are_bound_to_inputs(self):
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertEqual(report['capture']['exact_step_count'], 3)
        self.assertEqual(report['capture']['unit_atlas'], self.unit_atlas)
        self.assertEqual(report['capture']['presentation_clock']['draws'], [
            {'completed_steps': 1, 'radar_ms': 22, 'tooltip_ms': 22, 'message_ms': 22},
            {'completed_steps': 2, 'radar_ms': 44, 'tooltip_ms': 44, 'message_ms': 44},
            {'completed_steps': 3, 'radar_ms': 66, 'tooltip_ms': 66, 'message_ms': 66},
        ])
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

    def test_atlas_statistics_remain_required(self):
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
        report = self.run_capture()
        self.assertEqual(report['status'], 'VALID')
        self.assertEqual(report['capture']['presentation_clock']['draws'], [
            {'completed_steps': 0, 'radar_ms': 0, 'tooltip_ms': 0, 'message_ms': 0}])

    def test_clock_bounds_include_one_and_maximum_step_budgets(self):
        for ticks in (1, 100_000):
            with self.subTest(ticks=ticks):
                clock = self.clock(ticks)
                self.assertEqual(observation._presentation_clock(clock, ticks), clock)
        self.assertEqual(self.clock(100_000)['draws'][-1]['radar_ms'], 2_200_000)
        for ticks in (-1, 100_001):
            with self.subTest(ticks=ticks), self.assertRaises(ValidationError):
                observation._presentation_clock(self.clock(1), ticks)

    def test_clock_schema_and_every_consumed_time_are_strict(self):
        changes = [lambda c: c.update(extra='unrecognized'),
                   lambda c: c.update(policy='wall-clock'),
                   lambda c: c.update(draws=c['draws'][1:]),
                   lambda c: c.update(draws=c['draws'] + [c['draws'][-1]]),
                   lambda c: c.update(draws=list(reversed(c['draws']))),
                   lambda c: c['draws'].__setitem__(1, deepcopy(c['draws'][0])),
                   lambda c: c['draws'][0].update(completed_steps=0),
                   lambda c: c['draws'][1].update(extra=True)]
        for key in ('policy', 'origin_ms', 'interval_ms', 'draws'):
            changes.append(lambda c, k=key: c.pop(k))
        for key in ('completed_steps', 'radar_ms', 'tooltip_ms', 'message_ms'):
            changes.append(lambda c, k=key: c['draws'][1].pop(k))
            for value in (None, True, False, 44.0, '44', -1, 0, 66, 1 << 64):
                changes.append(lambda c, k=key, v=value: c['draws'][1].update({k: v}))
        for key, values in (('policy', (None, 1, True)),
                            ('origin_ms', (None, True, False, 0.0, -1, 22)),
                            ('interval_ms', (None, True, 22.0, 0, 16, -1)),
                            ('draws', (None, {}, 'draws', [], [None, None, None],
                                       [{}, {}, {}], [1, 2, 3]))):
            for value in values:
                changes.append(lambda c, k=key, v=value: c.update({k: v}))
        for index, change in enumerate(changes):
            with self.subTest(case=index):
                self.output = self.root / f'clock-{index}'
                self.change = lambda m, f=change: f(m['render']['presentation_clock'])
                report = self.run_capture()
                self.assertEqual(report['status'], 'INVALID', report)
                self.assertTrue(any('presentation_clock' in error for error in report['errors']))
        for index, value in enumerate((None, [], 'clock', 1)):
            self.output = self.root / f'clock-object-{index}'
            self.change = lambda m, v=value: m['render'].update(presentation_clock=v)
            self.assertEqual(self.run_capture()['status'], 'INVALID')
        self.output = self.root / 'clock-missing'
        self.change = lambda m: m['render'].pop('presentation_clock')
        self.assertEqual(self.run_capture()['status'], 'INVALID')

    def test_neutral_input_evidence_is_required_and_strict(self):
        changes = [lambda r: r.pop('neutral_input'),
                   lambda r: r.update(neutral_input=None),
                   lambda r: r['neutral_input'].update(extra=True)]
        for key in ('static_default_cursor', 'camera_input_idle'):
            changes.append(lambda r, k=key: r['neutral_input'].pop(k))
            for value in (False, 1, 1.0, None, 'true'):
                changes.append(lambda r, k=key, v=value: r['neutral_input'].update({k: v}))
        for index, change in enumerate(changes):
            with self.subTest(case=index):
                self.output = self.root / f'neutral-{index}'
                self.change = lambda m, f=change: f(m['render'])
                self.assertEqual(self.run_capture()['status'], 'INVALID')

    def test_live_capture_never_accepts_legacy_clock(self):
        self.change = lambda m: (m.update(schema_version=observation.LEGACY_CHILD_SCHEMA),
                                 m['render'].pop('presentation_clock'),
                                 m['render'].pop('neutral_input'))
        report = self.run_capture()
        self.assertEqual(report['status'], 'INVALID')
        self.assertIn('schema_version', report['errors'][0])

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

    @staticmethod
    def clock(ticks):
        return {'policy': 'map-exact-step-presentation-v1', 'origin_ms': 0, 'interval_ms': 22,
                'draws': [{'completed_steps': step, 'radar_ms': step * 22,
                           'tooltip_ms': step * 22, 'message_ms': step * 22}
                          for step in (range(1, ticks + 1) if ticks else [0])]}

    def make_legacy_clock(self, run):
        manifest = run / 'child-output/capture.json'
        def convert_child(document):
            document['schema_version'] = observation.LEGACY_CHILD_SCHEMA
            document['render'].pop('presentation_clock')
            document['render'].pop('neutral_input')
            document['render'].pop('camera')
            document.pop('observations')
        self.edit_json(manifest, convert_child)
        def convert_wrapper(document):
            document['schema_version'] = observation.LEGACY_CLOCK_RUN_SCHEMA
            document['capture'].pop('presentation_clock')
            document['capture'].pop('neutral_input')
            document['capture'].pop('camera')
            document['capture'].pop('observations')
            document['capture']['manifest'].update(byte_length=manifest.stat().st_size,
                                                    sha256=sha256_bytes(manifest.read_bytes()))
        self.edit_json(run / 'run.json', convert_wrapper)

    def make_legacy(self, run):
        self.make_legacy_clock(run)
        self.edit_json(run / 'run.json',
                       lambda report: report.update(schema_version=observation.LEGACY_RUN_SCHEMA))
        (run / 'config.toml').unlink()
        (run / 'contract.json').unlink()

    def test_historical_v3_is_explicitly_readable_without_clock_override(self):
        run = self.valid_capture('historical-v3')
        manifest = run / 'child-output/capture.json'
        def convert_child(document):
            document['schema_version'] = observation.PRIOR_CHILD_SCHEMA
            document.pop('observations')
            document['render'].pop('camera')
        self.edit_json(manifest, convert_child)
        def convert_wrapper(document):
            document['schema_version'] = observation.PRIOR_RUN_SCHEMA
            document['capture'].pop('observations')
            document['capture'].pop('camera')
            document['capture']['manifest'].update(byte_length=manifest.stat().st_size,
                                                    sha256=sha256_bytes(manifest.read_bytes()))
        self.edit_json(run / 'run.json', convert_wrapper)
        original = (run / 'run.json').read_bytes()
        report = observation.validate_run(run)
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertEqual(report['presentation_clock']['policy'], observation.CLOCK_POLICY)
        self.assertNotIn('observations', report['capture'])
        self.assertEqual((run / 'run.json').read_bytes(), original)
        current = self.valid_capture('current-v7')
        report = observation.compare_runs(run, current)
        self.assertEqual(report['status'], 'INVALID')
        self.assertIn('observation policies differ', report['errors'][0])
        self.edit_json(manifest, lambda value: value.update(observations={'policy': observation.OBSERVATION_POLICY}))
        self.assertEqual(observation.validate_run(run)['status'], 'INVALID')

    @staticmethod
    def remove_walk_observations(document):
        for frame in document['observations']['frames']:
            for actor in frame['actors']:
                if actor['foot'] is not None:
                    for key in ('walk_head_leptons', 'walk_destination_leptons', 'walk_is_moving'):
                        actor['foot'].pop(key)

    def make_historical_trajectory(self, run):
        manifest = run / 'child-output/capture.json'

        def observations(document):
            self.remove_walk_observations(document)
            trajectory = document['observations']
            trajectory['policy'] = observation.TRAJECTORY_OBSERVATION_POLICY
            trajectory.pop('rule_types')
            for frame in trajectory['frames']:
                frame.pop('houses')
                for actor in frame['actors']:
                    actor.pop('building')
                    actor.pop('miner')
                    actor.pop('radio')

        def convert_child(document):
            document['schema_version'] = observation.TRAJECTORY_CHILD_SCHEMA
            observations(document)

        self.edit_json(manifest, convert_child)

        def convert_wrapper(document):
            document['schema_version'] = observation.TRAJECTORY_RUN_SCHEMA
            observations(document['capture'])
            document['capture']['manifest'].update(byte_length=manifest.stat().st_size,
                                                   sha256=sha256_bytes(manifest.read_bytes()))

        self.edit_json(run / 'run.json', convert_wrapper)

    def test_historical_v4_trajectory_validates_unchanged_and_compares_only_same_policy(self):
        self.scripted_profile()
        before = self.valid_capture('historical-v4-before')
        after = self.valid_capture('historical-v4-after')
        self.make_historical_trajectory(before)
        self.make_historical_trajectory(after)
        originals = {path: path.read_bytes() for path in
                     (before / 'run.json', before / 'child-output/capture.json')}
        report = observation.validate_run(before)
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertNotIn('rule_types', report['capture']['observations'])
        self.assertNotIn('building', report['capture']['observations']['frames'][0]['actors'][0])
        report = observation.compare_runs(before, after)
        self.assertEqual(report['status'], 'MATCH', report['errors'])
        current = self.valid_capture('current-building-state')
        report = observation.compare_runs(before, current)
        self.assertEqual(report['status'], 'INVALID')
        self.assertIn('observation policies differ', report['errors'][0])
        for path, raw in originals.items():
            self.assertEqual(path.read_bytes(), raw)

    def test_historical_v4_retains_optional_terrain_resource_fields_unchanged(self):
        self.scripted_profile()
        cell = self.unallocated_cell([87, 53])
        cell.update(overlay={'id': 102, 'density': 3},
                    terrain_object={'name': 'TREE01', 'frame': None, 'active': None})
        self.terrain_frames[1] = [cell]
        before = self.valid_capture('historical-v4-terrain-before')
        after = self.valid_capture('historical-v4-terrain-after')
        for run in (before, after):
            self.make_historical_trajectory(run)
        paths = [run / name for run in (before, after)
                 for name in ('run.json', 'child-output/capture.json')]
        originals = {path: path.read_bytes() for path in paths}
        report = observation.validate_run(before)
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertEqual(report['capture']['observations']['frames'][1]['terrain'], [cell])
        report = observation.compare_runs(before, after)
        self.assertEqual(report['status'], 'MATCH', report['errors'])
        for path, raw in originals.items():
            self.assertEqual(path.read_bytes(), raw)

    def test_historical_v4_rejects_v5_fields_and_production_commands(self):
        self.scripted_profile()
        for index, change in enumerate((
                lambda m: m['observations'].update(rule_types=self.rule_types),
                lambda m: m['observations']['frames'][0]['actors'][0].update(building=None))):
            run = self.valid_capture(f'historical-v4-smuggle-{index}')
            self.make_historical_trajectory(run)
            self.edit_json(run / 'child-output/capture.json', change)
            self.assertEqual(observation.validate_run(run)['status'], 'INVALID')
        self.production_profile()
        run = self.valid_capture('historical-v4-production')
        self.make_historical_trajectory(run)
        report = observation.validate_run(run)
        self.assertEqual(report['status'], 'INVALID')
        self.assertIn('outside ordinary order coverage', report['errors'][0])

    def make_historical_building(self, run):
        manifest = run / 'child-output/capture.json'

        def observations(document):
            self.remove_walk_observations(document)
            transcript = document['observations']
            transcript['policy'] = observation.BUILDING_OBSERVATION_POLICY
            for frame in transcript['frames']:
                frame.pop('houses')
                for actor in frame['actors']:
                    actor.pop('miner')
                    actor.pop('radio')

        def convert_child(document):
            document['schema_version'] = observation.BUILDING_CHILD_SCHEMA
            observations(document)

        self.edit_json(manifest, convert_child)

        def convert_wrapper(document):
            document['schema_version'] = observation.BUILDING_RUN_SCHEMA
            observations(document['capture'])
            document['capture']['manifest'].update(byte_length=manifest.stat().st_size,
                                                   sha256=sha256_bytes(manifest.read_bytes()))

        self.edit_json(run / 'run.json', convert_wrapper)

    def test_historical_v5_validates_without_upgrade_and_compares_only_same_policy(self):
        self.refinery_profile()
        before = self.valid_capture('historical-v5-before')
        after = self.valid_capture('historical-v5-after')
        self.make_historical_building(before)
        self.make_historical_building(after)
        paths = [run / name for run in (before, after)
                 for name in ('run.json', 'child-output/capture.json')]
        originals = {path: path.read_bytes() for path in paths}
        report = observation.validate_run(before)
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertNotIn('houses', report['capture']['observations']['frames'][0])
        self.assertNotIn('miner', report['capture']['observations']['frames'][0]['actors'][0])
        report = observation.compare_runs(before, after)
        self.assertEqual(report['status'], 'MATCH', report['errors'])
        current = self.valid_capture('current-refinery-state')
        report = observation.compare_runs(before, current)
        self.assertEqual(report['status'], 'INVALID')
        self.assertIn('observation policies differ', report['errors'][0])
        for path, raw in originals.items():
            self.assertEqual(path.read_bytes(), raw)

    def test_unit_extension_coexists_with_refinery_state_and_survives_historical_v5_read(self):
        self.refinery_profile()
        unit = self.unit()
        unit.update(deployed_6e0=0, deploying_6e1=0, undeploying_6e2=0, deploy_anim_130=None)
        for actors in self.actor_frames.values():
            actors[0]['unit'] = deepcopy(unit)
        before = self.valid_capture('unit-refinery-before')
        after = self.valid_capture('unit-refinery-after')
        checked = observation.validate_run(before)
        self.assertEqual(checked['status'], 'VALID', checked['errors'])
        frames = checked['capture']['observations']['frames']
        self.assertEqual(frames[1]['actors'][0]['unit'], unit)
        self.assertEqual(frames[1]['actors'][0]['miner']['cargo_bales'], 40)
        self.assertEqual(frames[1]['actors'][0]['radio']['contacts'], [None, 99, None])
        self.assertEqual(frames[2]['houses'], self.house_frames[2])
        for run in (before, after):
            self.make_historical_building(run)
        originals = {path: path.read_bytes() for path in
                     (before / 'run.json', before / 'child-output/capture.json')}
        checked = observation.validate_run(before)
        self.assertEqual(checked['status'], 'VALID', checked['errors'])
        frames = checked['capture']['observations']['frames']
        self.assertEqual(frames[1]['actors'][0]['unit'], unit)
        self.assertNotIn('miner', frames[1]['actors'][0])
        self.assertNotIn('houses', frames[1])
        compared = observation.compare_runs(before, after)
        self.assertEqual(compared['status'], 'MATCH', compared['errors'])
        for path, raw in originals.items():
            self.assertEqual(path.read_bytes(), raw)
        self.edit_json(before / 'child-output/capture.json', lambda m:
                       m['observations']['frames'][1]['actors'][0]['unit'].update(deployed_6e0=True))
        checked = observation.validate_run(before)
        self.assertEqual(checked['status'], 'INVALID')
        self.assertIn('deployed_6e0', checked['errors'][0])

    def test_historical_v5_rejects_v6_docking_and_economy_fields(self):
        self.refinery_profile()
        for index, change in enumerate((
                lambda m: m['observations']['frames'][0].update(houses=[]),
                lambda m: m['observations']['frames'][0]['actors'][0].update(miner=None),
                lambda m: m['observations']['frames'][0]['actors'][0].update(radio={}))):
            run = self.valid_capture(f'historical-v5-smuggle-{index}')
            self.make_historical_building(run)
            self.edit_json(run / 'child-output/capture.json', change)
            self.assertEqual(observation.validate_run(run)['status'], 'INVALID')

    def make_historical_docking(self, run):
        manifest = run / 'child-output/capture.json'

        def observations(document):
            self.remove_walk_observations(document)
            document['observations']['policy'] = observation.DOCKING_OBSERVATION_POLICY

        def convert_child(document):
            document['schema_version'] = observation.DOCKING_CHILD_SCHEMA
            observations(document)

        self.edit_json(manifest, convert_child)

        def convert_wrapper(document):
            document['schema_version'] = observation.DOCKING_RUN_SCHEMA
            observations(document['capture'])
            document['capture']['manifest'].update(byte_length=manifest.stat().st_size,
                                                   sha256=sha256_bytes(manifest.read_bytes()))

        self.edit_json(run / 'run.json', convert_wrapper)

    def test_historical_v6_validates_without_upgrade_and_compares_only_same_policy(self):
        self.scripted_profile()
        before = self.valid_capture('historical-v6-before')
        after = self.valid_capture('historical-v6-after')
        for run in (before, after):
            self.make_historical_docking(run)
        originals = {path: path.read_bytes() for run in (before, after)
                     for path in (run / 'run.json', run / 'child-output/capture.json')}
        checked = observation.validate_run(before)
        self.assertEqual(checked['status'], 'VALID', checked['errors'])
        foot = checked['capture']['observations']['frames'][0]['actors'][0]['foot']
        self.assertNotIn('walk_is_moving', foot)
        self.assertIn('radio', checked['capture']['observations']['frames'][0]['actors'][0])
        compared = observation.compare_runs(before, after)
        self.assertEqual(compared['status'], 'MATCH', compared['errors'])
        current = self.valid_capture('current-walk-state')
        compared = observation.compare_runs(before, current)
        self.assertEqual(compared['status'], 'INVALID')
        self.assertIn('observation policies differ', compared['errors'][0])
        for path, raw in originals.items():
            self.assertEqual(path.read_bytes(), raw)

    def test_historical_v6_rejects_v7_walk_fields_child_and_policy(self):
        self.scripted_profile()
        changes = [lambda m: m.update(schema_version=observation.CHILD_SCHEMA),
                   lambda m: m['observations'].update(policy=observation.OBSERVATION_POLICY)]
        for key in ('walk_head_leptons', 'walk_destination_leptons', 'walk_is_moving'):
            changes.append(lambda m, k=key:
                           m['observations']['frames'][0]['actors'][0]['foot'].update({k: None}))
        for index, change in enumerate(changes):
            run = self.valid_capture(f'historical-v6-smuggle-{index}')
            self.make_historical_docking(run)
            self.edit_json(run / 'child-output/capture.json', change)
            self.assertEqual(observation.validate_run(run)['status'], 'INVALID')

    def test_wrapper_v7_requires_child_v7_and_walk_observation_policy(self):
        for index, change in enumerate((
                lambda m: m.update(schema_version=observation.TRAJECTORY_CHILD_SCHEMA),
                lambda m: m.update(schema_version=observation.BUILDING_CHILD_SCHEMA),
                lambda m: m.update(schema_version=observation.DOCKING_CHILD_SCHEMA),
                lambda m: m['observations'].update(policy=observation.TRAJECTORY_OBSERVATION_POLICY),
                lambda m: m['observations'].update(policy=observation.BUILDING_OBSERVATION_POLICY),
                lambda m: m['observations'].update(policy=observation.DOCKING_OBSERVATION_POLICY))):
            self.output = self.root / f'v7-generation-mismatch-{index}'
            self.change = change
            self.assertEqual(self.run_capture()['status'], 'INVALID')

    def test_map_receipt_read_and_write_limits_are_explicit(self):
        # Small overrides exercise both bounded paths without allocating 128 MiB.
        with patch.object(observation, 'MAX_RECEIPT_BYTES', 1):
            with self.assertRaisesRegex(ValidationError, 'exceeds'):
                self.run_capture()
        run = self.valid_capture('bounded-receipt')
        with patch.object(observation, 'MAX_RECEIPT_BYTES', 1):
            report = observation.validate_run(run)
            self.assertEqual(report['status'], 'INVALID')
            self.assertIn('too large', report['errors'][0])

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

    def test_clock_semantics_survive_consistent_child_and_wrapper_rehash(self):
        run = self.valid_capture('clock-rehash')
        manifest = run / 'child-output/capture.json'
        self.edit_json(manifest, lambda m: m['render']['presentation_clock']['draws'][1].update(
            message_ms=43))
        def rehash(report):
            report['capture']['manifest'].update(byte_length=manifest.stat().st_size,
                                                 sha256=sha256_bytes(manifest.read_bytes()))
            report['capture']['presentation_clock']['draws'][1]['message_ms'] = 43
        self.edit_json(run / 'run.json', rehash)
        report = observation.validate_run(run)
        self.assertEqual(report['status'], 'INVALID')
        self.assertIn('draws[1].message_ms', report['errors'][0])

    def test_legacy_clock_permission_is_separate_and_projection_stays_historical(self):
        run = self.valid_capture('wall-clock')
        self.make_legacy_clock(run)
        original = (run / 'run.json').read_bytes()
        for options in ({}, {'allow_legacy_inputs': True}):
            report = observation.validate_run(run, **options)
            self.assertEqual(report['status'], 'INVALID')
            self.assertIn('--allow-legacy-clock', report['errors'][0])
        report = observation.validate_run(run, allow_legacy_clock=True)
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertEqual(report['presentation_clock'], {'policy': 'legacy-wall-clock'})
        self.assertNotIn('presentation_clock', report['capture'])
        self.assertNotIn('neutral_input', report['capture'])
        self.assertEqual((run / 'run.json').read_bytes(), original)

    def test_legacy_clock_cannot_smuggle_diagnostic_guarantees(self):
        for key, value in (('presentation_clock', self.clock(3)),
                           ('neutral_input', {'static_default_cursor': True, 'camera_input_idle': True})):
            run = self.valid_capture(f'legacy-smuggle-{key}')
            self.make_legacy_clock(run)
            self.edit_json(run / 'child-output/capture.json',
                           lambda m, k=key, v=value: m['render'].update({k: v}))
            report = observation.validate_run(run, allow_legacy_clock=True)
            self.assertEqual(report['status'], 'INVALID')
            self.assertIn('legacy child v2', report['errors'][0])

    def test_wrapper_and_child_clock_versions_must_correspond(self):
        for index, (wrapper, child) in enumerate((
                (observation.LEGACY_CLOCK_RUN_SCHEMA, observation.CHILD_SCHEMA),
                (observation.RUN_SCHEMA, observation.LEGACY_CHILD_SCHEMA),
                (observation.LEGACY_RUN_SCHEMA, observation.CHILD_SCHEMA))):
            run = self.valid_capture(f'wrong-generation-{index}')
            if wrapper == observation.LEGACY_RUN_SCHEMA:
                self.make_legacy(run)
            self.edit_json(run / 'run.json', lambda r, v=wrapper: r.update(schema_version=v))
            self.edit_json(run / 'child-output/capture.json',
                           lambda m, v=child: m.update(schema_version=v))
            report = observation.validate_run(run, allow_legacy_inputs=True, allow_legacy_clock=True)
            self.assertEqual(report['status'], 'INVALID')
            self.assertIn('schema_version', report['errors'][0])

    def test_mixed_clock_policies_are_invalid_even_with_equal_frame_bytes(self):
        before = self.valid_capture('clock-before')
        after = self.valid_capture('clock-after')
        self.make_legacy_clock(before)
        self.assertEqual((before / 'child-output/frame.bgra').read_bytes(),
                         (after / 'child-output/frame.bgra').read_bytes())
        report = observation.compare_runs(before, after, allow_legacy_clock=True)
        self.assertEqual(report['status'], 'INVALID')
        self.assertIn('presentation_clock', report['errors'][0])
        self.make_legacy_clock(after)
        report = observation.compare_runs(before, after, allow_legacy_clock=True)
        self.assertEqual(report['status'], 'MATCH', report['errors'])

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
        rejected = observation.validate_run(run, allow_legacy_inputs=True)
        self.assertEqual(rejected['status'], 'INVALID')
        self.assertIn('--allow-legacy-clock', rejected['errors'][0])
        rejected = observation.validate_run(run, allow_legacy_clock=True)
        self.assertEqual(rejected['status'], 'INVALID')
        self.assertIn('--allow-legacy-inputs', rejected['errors'][0])
        report = observation.validate_run(run, allow_legacy_inputs=True, allow_legacy_clock=True)
        self.assertEqual(report['status'], 'VALID', report['errors'])
        self.assertEqual(report['input_provenance']['config'], 'EXTERNALLY_REVALIDATED_UNSEALED')
        self.assertEqual(report['input_provenance']['contract'], 'EXTERNALLY_REVALIDATED_UNSEALED')
        self.assertEqual(report['input_provenance']['profile'], 'SEALED_COPY')
        self.config.write_text('changed after original capture')
        self.assertEqual(observation.validate_run(run, allow_legacy_inputs=True, allow_legacy_clock=True)['status'], 'INVALID')
        self.config.unlink()
        self.assertEqual(observation.validate_run(run, allow_legacy_inputs=True, allow_legacy_clock=True)['status'], 'INVALID')

    def test_legacy_contract_cannot_be_replaced_or_silently_resealed(self):
        original_contract = self.root / 'legacy-contract.json'
        original_contract.write_bytes(self.contract.read_bytes())
        self.contract = original_contract
        run = self.valid_capture('legacy-contract')
        self.make_legacy(run)
        self.contract.write_bytes(self.contract.read_bytes() + b'\n')
        report = observation.validate_run(run, allow_legacy_inputs=True, allow_legacy_clock=True)
        self.assertEqual(report['status'], 'INVALID')
        self.assertIn('contract', report['errors'][0])
        self.contract.unlink()
        self.assertEqual(observation.validate_run(run, allow_legacy_inputs=True, allow_legacy_clock=True)['status'], 'INVALID')

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

        def load_and_change(directory, allow_legacy, allow_clock):
            result = load(directory, allow_legacy, allow_clock)
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
        self.make_legacy_clock(after)
        self.assertEqual(observation.compare_runs(before, after)['status'], 'INVALID')
        report = observation.compare_runs(before, after, allow_legacy_inputs=True, allow_legacy_clock=True)
        self.assertEqual(report['status'], 'MATCH', report['errors'])
        self.edit_json(before / 'child-output/capture.json',
                       lambda m: m.update(schema_version='vera20k.map-observation.v1'))
        report = observation.compare_runs(before, after, allow_legacy_inputs=True, allow_legacy_clock=True)
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

    def test_cli_legacy_clock_flag_is_offline_and_explicit(self):
        run = self.valid_capture('cli-legacy-clock')
        self.make_legacy_clock(run)
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(observation.main(['validate', '--run', str(run), '--output',
                                               str(self.root / 'clock-denied.json')]), 2)
            self.assertEqual(observation.main(['validate', '--run', str(run),
                                               '--allow-legacy-clock', '--output',
                                               str(self.root / 'clock-allowed.json')]), 0)
            with self.assertRaises(SystemExit) as error:
                observation.main(['--allow-legacy-clock', '--profile', str(self.profile_path),
                                  '--contract', str(self.contract), '--output', str(self.root / 'unused')])
            self.assertEqual(error.exception.code, 2)
        report = json.loads((self.root / 'clock-allowed.json').read_text())
        self.assertEqual(report['presentation_clock']['policy'], 'legacy-wall-clock')
        self.assertEqual(report['parity_certification'], 'NONE')


if __name__ == '__main__':
    unittest.main()
