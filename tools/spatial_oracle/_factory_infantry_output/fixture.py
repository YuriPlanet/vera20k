"""Mechanically select native milestones; no gameplay math or decisions."""
from pathlib import Path
import ast
import hashlib
import struct
from .runtime import require


def unit_ready_consumer(controls, source_receipts, *, lean=False, projection=None, pcm=None):
    """Select literal native audio/radar readbacks for production regressions.

    No clock conversion, eligibility, decoder, completion or timer decision is
    calculated here. Full raw receipts and their normalized hashes remain the
    comparison authority; this is the single compact consumer selector.
    """
    from . import runtime as rt
    rng_states = {}
    device_payloads = {}

    def state(s):
        streams = {}
        for name, encoded in s['rng'].items():
            raw = bytes.fromhex(encoded)
            require(len(raw) == 1012, 'Consumer RNG buffer is incomplete')
            digest = hashlib.sha256(raw).hexdigest()
            row = rng_states.setdefault(digest, dict(bytes=encoded, length=len(raw), streams=[]))
            if name not in row['streams']:
                row['streams'].append(name)
            streams[name] = digest
        selected = [{k: x[k] for k in ('name', 'volume_f32_bits', 'yuri', 'russian', 'allied',
                     'priority', 'type', 'state')} for x in s['selected']]
        stream = s.get('stream_fields')
        result = dict(selected=selected, current_is_set=bool(s['current']),
            pending_standard_is_set=s['pending_standard'] is not None,
            pending_standard=s['pending_standard'], sequence=s['sequence'],
            suspend_depth=s['suspend_depth'], pause_depth=s['pause_depth'],
            pause_flag=s['pause_flag'], gap_words=s['wait_words'],
            queue_counts={k: len(v) for k, v in s['queues'].items()},
            interrupt_count=len(s['interrupts']), critical_count=len(s['critical']),
            registry_count=s['registry']['count'], voice_side=s['voice_side'],
            radar=s['radar'], rng=streams)
        if stream is not None:
            raw = bytes.fromhex(stream['native_bytes'])
            result['stream'] = dict(flags=stream['flags'], end_words=stream['end_time'],
                start_words=list(struct.unpack_from('<2I', raw, 0x28)),
                native_bytes=stream['native_bytes'])
        if 'audio_clock' in s:
            result['clock'] = s['audio_clock']
        if 'audio_service' in s:
            result['service'] = s['audio_service']
        if 'selected_device_buffer' in s:
            payload = s['selected_device_buffer']
            raw = bytes.fromhex(payload['bytes_hex'])
            digest = hashlib.sha256(raw).hexdigest()
            require(len(raw) == payload['length'], 'Incomplete device storage observation')
            device_payloads.setdefault(digest, dict(length=len(raw), bytes=payload['bytes_hex']))
            result['device_payload_sha256'] = digest
        if 'native_channel' in s:
            result['native_channel'] = s['native_channel']
        if 'device_thread' in s:
            result['device_thread'] = s['device_thread']
        return result

    def step(s):
        require(s['success'] is True and s['before']['rng'] == s['after']['rng'] and
                not s['draws'] and not s['advances'], 'Consumer native step/RNG differs')
        result = dict(label=s['label'], entry=s['entry'],
            before=state(s['before']), after=state(s['after']),
            original_call_order=s['events'])
        for key in ('return', 'return_value', 'stop', 'end', 'clock_prior', 'clock_calls',
                    'file_io', 'device_io', 'os_status_prior', 'first_entry',
                    'original_thread_parameter', 'reached_sleep_arg',
                    'native_decoded_remaining_signed', 'os_playing_after',
                    'native_member_writes', 'platform_calls'):
            if key in s:
                result[key] = s[key]
        return result

    if projection is None:
        selected = {}
        for name, data in controls.items():
            require(data['success'] is True and data['original_code_unchanged'] is True and
                    not data['requests'] and not data['advances'], 'Consumer control failed: ' + name)
            row = dict(full_original_receipt_sha256=rt.canonical_sha(data),
                steps=[step(s) for s in data.get('steps', [])],
                final=state(data['final']), bounds=data['bounds'])
            prior = data['borrowed_prior']
            while 'borrowed_prior' in prior:
                prior = prior['borrowed_prior']
            row['registered_prior'] = dict(physical=prior['setup']['physical'],
                clock_steps=[step(s) for s in prior['setup']['steps'] if s['label'] in
                             ('original_audio_clock_initializer', 'original_positive_audio_clock')],
                notification_suffix=[step(s) for s in prior['suffix']])
            if name == 'buffer':
                row['returned_native_format'] = dict(data['returned_native_format'])
                row['returned_native_format']['backend_44_format_kind'] = row['returned_native_format'].pop('source_compression')
                row['format_label_provenance'] = 'Original408754 admits physical WAV tag0x11; backend+44 kind4 is not the WAV compression tag; backend+50 compressed flag1.'
                row['os_cursor_time_inputs'] = data['os_cursor_time_inputs']
            if name == 'radar':
                row.update(native_type6_bytes=data['native_type6_bytes'],
                    native_type6_fields=data['native_type6_fields'],
                    native_scalar_bytes=data['native_scalar_bytes'],
                    entry_birth_bytes=data['entry_birth_bytes'],
                    phase_frame=data['phase_frame'], expiry_frame=data['expiry_frame'],
                    frames=[dict(frame=f['frame'], tick=step(f['tick']), cleanup=step(f['cleanup']),
                                 native_member_writes=f['native_member_writes']) for f in data['frames']],
                    cleanup=[step(s) for s in data['cleanup']])
            selected[name] = row
        result = dict(schema=1, kind='Original registered UnitReady bounded consumer readbacks',
            source_receipts=source_receipts, controls=selected,
            complete_rng_states=rng_states, complete_device_payloads=device_payloads,
            comparison_limit='OS frequency/counter/status/cursors and single-threaded Sleep/locks are supplied inputs. MIX traversal, real device concurrency/audible output and radar screen geometry are excluded. No whole-object completion claim.')
    else:
        require(projection['schema'] == 1 and set(projection['controls']) == {'cadence', 'device', 'buffer', 'radar'},
                'Retained consumer projection differs')
        result = projection
        selected = result['controls']
    if pcm is not None:
        observed = pcm['observation']
        require(observed['success'] and observed['original_code_unchanged'], 'Original PCM witness failed')
        from .pcm import decoded_ranges
        raw, source = decoded_ranges(observed['observation'])
        identity = observed['pcm']
        require(len(raw) == identity['bytes'] and hashlib.sha256(raw).hexdigest() == identity['sha256'],
                'Original literal PCM count/hash differs')
        selected['buffer']['decoded_pcm'] = dict(
            **{k:v for k,v in identity.items() if k != 'path'}, bytes_hex=raw.hex(),
            source_receipt=pcm['source_receipt'], source_boundary=observed['source_boundary'],
            complete_original_observation_sha256=rt.canonical_sha(observed['observation']),
            original_call_order=observed['observation']['events'],
            source_reads=[{k:v for k,v in row.items() if not k.endswith('_hex')}
                for row in observed['observation']['sample_reads']],
            block_returns=[{k:row[k] for k in ('index','phase','entry_pc','caller','return_pc',
                'return_status','offered_input_bytes','returned_output_bytes','output_sha256',
                'channels','nibble_calls')} for row in observed['observation']['blocks']],
            callback_returns=[{k:row[k] for k in ('index','phase','entry_pc','caller','return_pc',
                'return_status','offered_source_bytes','consumed_source_bytes','returned_output_bytes',
                'output_sha256','first_block','last_block_exclusive')}
                for row in observed['observation']['callbacks']], bounds=observed['bounds'])
    if not lean:
        return result
    # A narrow serialization of this same selected data for include_str tests;
    # no second native selector, timing calculation or expectation source.
    def narrow_step(s):
        out = {k: s[k] for k in ('label', 'entry', 'return', 'return_value', 'clock_prior') if k in s}
        after = s['after']
        out['after'] = {k: after[k] for k in ('selected', 'current_is_set', 'pending_standard_is_set',
            'pause_depth', 'gap_words', 'service', 'clock') if k in after}
        if 'stream' in after:
            out['after']['stream'] = {k: v for k, v in after['stream'].items() if k != 'native_bytes'}
        out['original_call_order'] = [x['pc'] for x in s['original_call_order']]
        out['complete_selected_step_sha256'] = rt.canonical_sha(s)
        return out
    result = {k: result[k] for k in ('schema', 'kind', 'source_receipts', 'comparison_limit')}
    result['controls'] = {}
    for name, row in selected.items():
        out = {k: row[k] for k in ('full_original_receipt_sha256', 'bounds')}
        out['registered_prior'] = dict(physical=row['registered_prior']['physical'],
            clock_steps=[narrow_step(s) for s in row['registered_prior']['clock_steps']],
            notification_suffix=[narrow_step(s) for s in row['registered_prior']['notification_suffix']])
        out['steps'] = [narrow_step(s) for s in row['steps']]
        if name == 'buffer':
            out.update(returned_native_format={k: v for k, v in row['returned_native_format'].items() if not k.endswith('_hex')},
                format_label_provenance=row['format_label_provenance'], os_cursor_time_inputs=row['os_cursor_time_inputs'])
            if 'decoded_pcm' in row:
                out['decoded_pcm'] = row['decoded_pcm']
        if name == 'radar':
            out.update({k: row[k] for k in ('native_type6_bytes', 'native_type6_fields', 'native_scalar_bytes',
                'entry_birth_bytes', 'phase_frame', 'expiry_frame')})
            out['frames'] = [dict(frame=f['frame'],
                tick_after_radar=f['tick']['after']['radar'],
                cleanup_after_radar=f['cleanup']['after']['radar'],
                complete_tick_sha256=rt.canonical_sha(f['tick']),
                complete_cleanup_sha256=rt.canonical_sha(f['cleanup'])) for f in row['frames']]
        result['controls'][name] = out
    return result


def generate(original, private, source_receipts):
    require(original['fault'] is None and private['fault'] is None and
            private['primary_comparison'].get('native_observations_equal', private['primary_comparison'].get('whole_receipt_equal')) is True,
            'selected original accepted receipt is not successful')
    queue = original['admitted_queue']['queue']
    require(queue['original_code_unchanged'] is True and private['native_code_unchanged'] is True,
            'original code identity changed')
    first = next(row for row in queue['steps'] if row['label'] == 'native_human_first_e1_begin')
    second = next(row for row in queue['steps'] if row['label'] == 'native_human_second_e1_queue')
    loop = next(row for row in queue['steps'] if row['label'] == 'ordered_native_sidebar_foot_factory_event_control')
    rounds = loop['rounds']
    rng = {}

    def retained_state(name, encoded):
        if isinstance(encoded, int):
            state = private['complete_rng_states'][str(encoded)]
            require(state['stream'] == name, 'wrong private stream reference')
            encoded = state['bytes']
        raw = bytes.fromhex(encoded)
        require(len(raw) == 0x3F4, 'incomplete original RNG buffer')
        digest = hashlib.sha256(raw).hexdigest()
        record = rng.setdefault(digest, dict(bytes=encoded, length=len(raw),
                                            prefix_words=list(struct.unpack('<3I', raw[:12])), streams=[]))
        if name not in record['streams']:
            record['streams'].append(name)
        return digest

    def project(value):
        if isinstance(value, dict):
            if set(value) == {'main', 'scenario', 'mapgen'} and all(isinstance(x, (str, int)) for x in value.values()):
                return {name: retained_state(name, encoded) for name, encoded in value.items()}
            return {key: project(child) for key, child in value.items()}
        if isinstance(value, list):
            return [project(child) for child in value]
        return value

    complete = []
    old = None
    for row in rounds:
        current = row['factory']['stage'] if row['factory'] else None
        if current == 54 and old != 54:
            complete.append(row['frame'])
        old = current
    place = sorted({event['frame'] for event in queue['events'] if event['kind'] == 'house_place'})
    scatter = sorted({event['frame'] for event in private['events'] if event['kind'] == 'InfantryScatter' and
                      event['before']['nav'] == 0})
    release = []
    contact_before = False
    for row in rounds:
        contact_after = any(row['barracks']['contacts'])
        if contact_before and not contact_after:
            release.append(row['frame'])
        contact_before = contact_after
    milestones = sorted({rounds[0]['frame'], *complete, *place, *scatter, *release})
    selected = []
    for frame in milestones:
        row = next(row for row in rounds if row['frame'] == frame)
        state = {key: value for key, value in row.items() if key not in ('events', 'requests')}
        relevant_calls = [event for event in row['events'] if event['kind'] in
            ('house_place', 'building_exit', 'infantry_ctor', 'infantry_unlimbo', 'archive_assignment',
             'infantry_destination', 'queue_mission', 'radio_transmit', 'start_next', 'factory_abandon',
             'counter_increment', 'counter_decrement')]
        private_calls = [event for event in private['events'] if event['frame'] == frame and event['kind'] in
            ('InfantryScatter', 'FootIdle', 'InfantryIdle', 'PlanningCreate', 'PlanningSetter', 'NativeBoundary')]
        selected.append(dict(frame=frame, state=project(state), original_calls=project(relevant_calls),
                             private_observations=project(private_calls), requests=project(row['requests'])))
    return dict(schema_version=1, kind='Native-derived ordinary no-rally producer/queue/movement milestones',
        source_receipts=source_receipts,
        binary_and_source_scope='Original active-retail executable; P5 Rust baseline committed 2909; no latest integrated source assertion',
        rng_representation='SHA256 keys reference complete unmodified 0x3F4 native buffers; prefix words are raw readback, not inferred seed/index semantics',
        rng_states=rng,
        producer_opening=dict(original_constructor_admission_steps=project(original['admitted_queue']['joined']['admission']['steps']),
                              yard_admitted=project(original['opening']['yard_admitted']),
                              completion_frame=original['opening']['completion_frame'],
                              ready_frame=original['opening']['frames'][-1]['frame'],
                              ready=project(original['opening']['final']),
                              supplied_opening_fields=original['opening']['supplied_opening_fields']),
        original_queue_admission=dict(first=project(first), second=project(second)),
        environment=dict(runtime=queue['runtime'], game_mode=queue['runtime_mode'],
                         map_diamond=queue['map_diamond_prior'], physical_clear_map=project(queue['physical_clear_map']),
                         recorded_pre_begin_state=project(first['before']),
                         post_begin_separate_cursor_field=first['after']['cursor'],
                         selected_native_input_reference=dict(path=source_receipts['producer_queue']['path'], json_path='admitted_queue.queue.selected_inputs')),
        measured_frames=dict(queue_first_visit=rounds[0]['frame'], completion=complete, place=place,
                             first_live_scatter=scatter, reciprocal_radio_free=release, final=queue['final']['frame']),
        milestones=selected, final=project(queue['final']),
        requests=project(queue['requests']), raw_advances=project(queue['advances']),
        exact_schedule=[
            'Setup uses existing whole selected input owners and runtime fixture with seed2; original Main seeder65C6D0(seed2), Scenario seeder65C6D0(seed2), and MapGen seeder65C6D0(seed31) are existing caller inputs, not a promise that re-seeding at frame51 reproduces its already advanced buffers.',
            'Selected whole GAPILE constructor, original type registration, native Unlimbo/House admission, original yard HELLO/childC/BREAK and bounded producer construction/attached-Anim visits run before original yard expiry/removal and House power consumer.',
            'Each queue frame visits Strip6A8B30, then already delivered live Infantry51BAB0 in delivery order, then held Factory4C9B20, then local6474C7..6474BF OutList/DoList dispatch. Frame advances externally after that sequence.',
            'New PLACE output is discovered after local dispatch and enters delivered visits next frame. The native caller does not visit the held limbo GI before delivery.',
            'During construction only selected producer construction/header/mission/animation pieces and its attached live Anims run in native Logic order. Later producer whole BuildingAI/HouseAI and unrelated prior Unit/Building/AnimAI are excluded.',
        ],
        supplied_closure=[
            dict(owner='existing source-scatter/destination/refinery runtime', prior='32x32 Cell pointer grid and raw level0 cells; real cell coordinates are supplied, legacy Drive/refinery blobs exist but repair_target_prior clears every ground-list head before new target/map admission; their AI is not visited'),
            dict(owner='building_death_anims.joined_fixture', prior='Flat 33x33 class/height plane: interior1..31 class0/level0, border class7/level0; map diamond16x16 and coordinate bounds0..32; original connectivity and Pathfinder run, physical map/theater startup excluded'),
            dict(owner='engineer_repair_admission.repair_target_prior plus death fixture', prior='GAPOWR at10,10: whole original ctor followed by supplied admitted location/flags/mission/House prior and original Mark0/3. Whole fixture health argument750 and sample750 remain; second copied human American enemy House owns it; no paid repair. Its active Anims remain but receive no loop AI'),
            dict(owner='building_death_anims.joined_fixture', prior='MTNK at11,8: whole original Unit ctor/Unlimbo, native target GAPOWR10,10 and settled native facing; no later UnitAI/shots in this factory control. These prior constructor RNG effects precede the retained buffers'),
            dict(owner='building_construction.JoinedFixture', prior='Supplied GameOptionsA8EB60=3, presentation640x480/zoom0.1953125, opaque allocator/import/Win32/DirectSound and empty SEH transport. Original data/input fields and original simulation owners remain; complete options/UI/device startup excluded'),
            dict(owner='native selected House corridors with inherited country prior', prior='Human American House, difficulty0, player/currentHouse identity and alliance prior. Selected original membership/interface/counter/registry/expiry sequences execute, not whole House constructor/whole match startup. Before Begin this House owns only admitted GAPILE, power0/drain10, credits10000; all values are retained native state above'),
            dict(owner='original cash/queue ingress', prior='Native AddCash4F9950(10000); original Sidebar/type cameo setup; original HouseBeginProduction4FA350(16,1,0,0) twice. No product stage, balance, exit cell, movement, arrival, occupation, mission timer or opening result is supplied in this accepted ordinary loop'),
            dict(owner='original selected CRT consumers', prior='Actual Object and Infantry height tables, five Infantry subcell offsets, type/Factory/registry prerequisites execute through existing owners. P2 no-rally does not warm the optional Foundation exit table because the clear preferred exit suffices; blocked variants separately execute its original CRT slot'),
        ],
        limits=[
            'Native-derived compact fixture plus retained full receipt regeneration/comparison. No gameplay value is generated by Rust or hand arithmetic.',
            'Whole canonical advance_tick visits more objects/phases than this original restricted caller. The current snapshot is not evidence for visiting the retained prior MTNK/GAPOWR/Anims or removing them; resolve that schedule/prior-actor boundary explicitly before claiming joined canonical parity.',
            'Pointer identities and raw snapshots are retained. Rust tests should compare authoritative lifecycle/relationships through existing owners, not create duplicate pointer/state ownership or treat raw retained freed arena bytes as live objects.',
            'No full startup, map load, human GUI, networking/full ring-capacity, checksum/render/audio timeline or whole-object completeness is claimed. Required no-rally GI movement/radio consumers are measured through automatic native visits, with no forced arrival or supplied radio release.',
        ])



def initialized_projection(control, data):
    from tools.spatial_oracle._factory_infantry_output import runtime as rt
    require(data['status'] == 'PASS' and data['selected_control'] == control, 'Unsuccessful original control')
    primary = data['full_original_primary']
    private = data['full_original_gi_observer']
    require(private['fault'] is None and private['native_code_unchanged'] is True, 'Original private observer failed')
    joined = primary['joined']
    queue = joined['admitted_queue']['queue']
    require(queue['fault'] is None and queue['original_code_unchanged'] is True, 'Original queue failed')
    loop = next((x for x in queue['steps'] if x['label'] == 'ordered_native_sidebar_foot_factory_event_control'))
    rounds = loop['rounds']
    frames = {x['frame']: x for x in rounds}
    rng = {}

    def project(value):
        if isinstance(value, dict):
            if set(value) == {'main', 'scenario', 'mapgen'} and all((isinstance(x, (str, int)) for x in value.values())):
                out = {}
                for name, encoded in value.items():
                    if isinstance(encoded, int):
                        state = private['complete_rng_states'][str(encoded)]
                        require(state['stream'] == name, 'Wrong original RNG stream reference')
                        encoded = state['bytes']
                    if len(encoded) == 64 and encoded in data['gate_complete_rng_states']:
                        state = data['gate_complete_rng_states'][encoded]
                        require(state['stream'] == name, 'Wrong original gate stream reference')
                        encoded = state['bytes']
                    raw = bytes.fromhex(encoded)
                    require(len(raw) == 1012, 'Incomplete original RNG buffer')
                    digest = hashlib.sha256(raw).hexdigest()
                    record = rng.setdefault(digest, dict(bytes=encoded, length=len(raw), streams=[]))
                    if name not in record['streams']:
                        record['streams'].append(name)
                    out[name] = digest
                return out
            return {k: project(v) for k, v in value.items()}
        if isinstance(value, list):
            return [project(x) for x in value]
        return value
    complete = []
    old_stage = None
    release = []
    had_contact = False
    null_destinations = []
    guard_commits = []
    prior_products = {}
    for row in rounds:
        stage = row['factory']['stage'] if row['factory'] else None
        if stage == 54 and old_stage != 54:
            complete.append(row['frame'])
        old_stage = stage
        contact = any(row['barracks']['contacts'])
        if had_contact and (not contact):
            release.append(row['frame'])
        had_contact = contact
        for product in row['delivered_infantry']:
            prior = prior_products.get(product['pointer'])
            if prior and prior['nav'] and (not product['nav']):
                null_destinations.append(row['frame'])
            if prior and prior['mission'] != 5 and (product['mission'] == 5):
                guard_commits.append(row['frame'])
            prior_products[product['pointer']] = product
    place = sorted({x['frame'] for x in queue['events'] if x['kind'] == 'house_place'})
    scatter = sorted({x['frame'] for x in private['events'] if x['kind'] == 'InfantryScatter' and x['before']['nav'] == 0})
    handoff = sorted({x['frame'] for x in private['events'] if x['kind'] == 'InfantryIdle' and x.get('caller') == '0x00520F92' and x['before']['archive'] and (x.get('after', {}).get('archive') == 0)})
    first_live = {}
    for x in private['events']:
        if x['kind'] == 'NativeBoundary' and x['pc'] == '0x0051BCA4':
            first_live.setdefault(x['this'], x['frame'])
    milestones = sorted({rounds[0]['frame'], *complete, *place, *scatter, *first_live.values(), *release, *(x - 1 for x in release if x - 1 in frames), *null_destinations, *guard_commits, *handoff, queue['final']['frame']})
    selected = []
    for frame in milestones:
        row = frames[frame]
        selected.append(dict(frame=frame, state=project({k: v for k, v in row.items() if k not in ('events', 'requests')}), original_calls=project(row['events']), requests=project(row['requests']), private_observations=project([x for x in private['events'] if x['frame'] == frame and x['kind'] in ('InfantryScatter', 'FootIdle', 'InfantryIdle', 'InfantryCtor', 'FootCtor', 'PlanningCreate', 'PlanningSetter', 'NativeBoundary')]), walk_completion_gates=project([x for x in data['walk_completion_gate_observations'] if x['frame'] == frame])))
    result = dict(schema=1, control=control, complete_normalized_primary_sha256=rt.canonical_sha(rt.normalize(primary)), complete_normalized_private_sha256=rt.canonical_sha(rt.normalize(private)), executed_registered_startup=project(data['original_registered_walk_startup']), original_startup_calls=project(data['startup_calls']), original_startup_writes=data['startup_writes'], selected_loop_global_reads=data['selected_loop_global_reads'], selected_loop_global_writes=data['selected_loop_global_writes'], original_global_consumer_instructions=data['original_global_consumer_instructions'], producer_opening=project(joined['opening']), producer_admission=project(joined['admitted_queue']['joined']), original_queue_admission=project(queue['steps'][:-1]), native_environment=dict(runtime=queue['runtime'], runtime_mode=queue['runtime_mode'], map_diamond_prior=queue['map_diamond_prior'], physical_clear_map=project(queue['physical_clear_map']), selected_inputs=queue['selected_inputs']), original_rally_setup=project(primary['setup']), actual_house_place_notifications=data['actual_house_place_notification_coordinates'], live_virtual_identities=private['live_virtual_identities'], measured_frames=dict(completion=complete, place=place, first_live_scatter=scatter, radio_release=release, first_live_infantry=sorted(first_live.values()), archive_handoff=handoff, navigation_null=null_destinations, guard_commit=guard_commits, final=queue['final']['frame']), milestones=selected, final_public=project(queue['final']), final_private=project(private['primary_boundary']), full_request_order=project(queue['requests']), full_raw_advance_order=project(queue['advances']), original_executed_spans=private['original_executed_spans'], rng_states=rng, bounds=data['bounds'])
    for digest, row in data['gate_complete_rng_states'].items():
        raw = bytes.fromhex(row['bytes'])
        require(len(raw) == 1012 and hashlib.sha256(raw).hexdigest() == digest, 'Gate RNG bytes differ')
        record = rng.setdefault(digest, dict(bytes=row['bytes'], length=len(raw), streams=[]))
        if row['stream'] not in record['streams']:
            record['streams'].append(row['stream'])
    return result

def local_fixture(no_rally, rally, source_receipts):
    """One mechanical selector for the structural Rust integration fixture.

    Source native frames identify observations only. The selected public/private
    values are not an assertion of whole canonical tick or RNG/time parity.
    """

    def route(data):
        control = data['control']
        ms = {x['frame']: x for x in data['milestones']}
        begin = next((x for x in data['original_queue_admission'] if x['label'] == 'native_human_first_e1_begin'))
        producer = begin['before']['barracks']['pointer']

        def local(state, latch=None):
            result = {k: state[k] for k in ('location', 'health', 'mission', 'queued')}
            result.update(nav_is_set=bool(state['nav']), archive_is_set=bool(state['archive']), tether=bool(state['tether']), contact_count=sum((bool(x) for x in state['contacts'])), doing=state.get('doing', state.get('infantry_doing')))
            if latch is not None:
                result['idle_entry_latch'] = latch
            return result

        def radio(frame, product, arrival=False):
            events = [x for x in ms[frame]['original_calls'] if x['kind'] == 'radio_transmit' and {x['this'], x['receiver']} == {producer, product}]
            if arrival:
                at = next((i for i, x in enumerate(events) if x['this'] == product and x['message'] == 8))
                events = events[at:]
            pointers = {producer: 'producer', product: 'product'}
            return [[pointers[x['this']], pointers[x['receiver']], x['message'], x['result']] for x in events]
        products = []
        for place in data['measured_frames']['place']:
            row = ms[place]
            unlimbo = next((x for x in row['original_calls'] if x['kind'] == 'infantry_unlimbo' and x['caller'] == '0x00444C9B'))
            pointer = unlimbo['this']
            public = next((x for x in row['state']['delivered_infantry'] if x['pointer'] == pointer))
            idle = next((x for x in row['private_observations'] if x['kind'] == 'InfantryIdle' and x['this'] == pointer and (x['caller'] == '0x006F6E30')))
            live = next((x for x in data['milestones'] if x['frame'] > place and any((e['kind'] == 'NativeBoundary' and e['pc'] == '0x0051BCA4' and (e['this'] == pointer) for e in x['private_observations']))))
            boundary = next((x for x in live['private_observations'] if x['kind'] == 'NativeBoundary' and x['pc'] == '0x0051BCA4' and (x['this'] == pointer)))
            live_state = next((x for x in live['state']['delivered_infantry'] if x['pointer'] == pointer))
            if control == 'no_rally':
                scatter = next((x for x in live['private_observations'] if x['kind'] == 'InfantryScatter' and x['this'] == pointer))
                latch = scatter['after']['idle_latch_6b3']
                walk = scatter['after']['walk']
            else:
                latch = boundary['state']['idle_latch_6b3']
                walk = boundary['state']['walk']
            first_live = local(live_state, latch)
            first_live.update(paid_head_is_set=any(walk['paid_head']), walk_is_moving=bool(walk['moving']))
            release = next((x for x in data['milestones'] if any((e['kind'] == 'radio_transmit' and e['this'] == pointer and (e['message'] == 8) for e in x['original_calls']))))
            released = next((x for x in release['state']['delivered_infantry'] if x['pointer'] == pointer))
            terminal = next((x for x in data['final_private'] if x['pointer'] == pointer))
            terminal_local = {k: terminal[k] for k in ('health', 'mission', 'queued')}
            terminal_local.update(nav_is_set=bool(terminal['nav']), archive_is_set=bool(terminal['archive']), tether=bool(terminal['tether']), contact_count=sum((bool(x) for x in terminal['contacts'])), paid_head_is_set=any(terminal['walk']['paid_head']), walk_is_moving=bool(terminal['walk']['moving']), walk_destination_is_set=any(terminal['walk']['destination']))
            frames = dict(place=place, scatter=live['frame'], arrival=release['frame']) if control == 'no_rally' else dict(place=place, first_live=live['frame'], contact_release=release['frame'])
            product = dict(native_frames=frames, unlimbo_facing=unlimbo['args'][1], placed=local(public, idle['after']['idle_latch_6b3']), first_live=first_live, delivery_radio=radio(place, pointer), arrival_radio=radio(release['frame'], pointer, True), released={k: local(released)[k] for k in ('location', 'mission', 'queued', 'nav_is_set', 'archive_is_set', 'tether', 'contact_count')}, terminal=terminal_local, terminal_cell=[x // 256 for x in terminal['location'][:2]])
            if control == 'no_rally':
                product['released_location_provenance'] = product['released'].pop('location')
            if control == 'rally':
                handoff = next(((m, x) for m in data['milestones'] for x in m['private_observations'] if x['kind'] == 'InfantryIdle' and x['this'] == pointer and (x['caller'] == '0x00520F92') and x['before']['archive'] and (x['after']['archive'] == 0)))
                product['native_frames']['archive_handoff'] = handoff[0]['frame']
                product['archive_handoff'] = local(handoff[1]['after'], handoff[1]['after']['idle_latch_6b3'])
                product['rally_walk_destination'] = handoff[1]['after']['walk']['destination']
            products.append(product)
        return (dict(source_receipts=source_receipts[control], products=products), begin)
    ordinary, begin = route(no_rally)
    with_rally, _ = route(rally)
    ready = begin['before']
    producer = ready['barracks']
    request = next((x for x in rally['original_rally_setup'] if x['label'] == 'original_selected_barracks_set_rally_click'))
    placed = rally['milestones'][next((i for i, x in enumerate(rally['milestones']) if x['frame'] == rally['measured_frames']['place'][0]))]
    exit_pointer = placed['state']['delivered_infantry'][0]['nav']
    selected = next((x for x in request['after']['perimeter'] if x['pointer'] == exit_pointer))
    from tools.spatial_oracle._factory_infantry_output import exit as exit_caller
    click = [x for x in ast.walk(ast.parse(Path(exit_caller.__file__).read_text(encoding='utf-8'))) if isinstance(x, ast.Call) and isinstance(x.func, ast.Attribute) and (x.func.attr == 'pack') and (len(x.args) == 3) and isinstance(x.args[0], ast.Constant) and (x.args[0].value == '<2h')]
    require(len(click) == 1, 'Authored rally input expression changed')
    requested = [ast.literal_eval(x) for x in click[0].args[1:]]
    rally_destination = with_rally['products'][0]['rally_walk_destination']
    require([x // 256 for x in rally_destination[:2]] == requested, 'Original rally destination differs from clear authored input')
    with_rally.update(request=dict(cell=requested, entry=request['pc'], input_owner='tools/spatial_oracle/_factory_infantry_output/exit.py'), exit_cell=[selected['x'], selected['y']], limits=['Both terminal Guard/Nav/Archive/private Walk/contact states are measured from original registered-startup execution.', 'Native local frames identify source rows; absolute canonical frame/RNG parity is excluded.'])
    return dict(schema_version=2, kind='Initialized native local human barracks Infantry output', source_receipts=source_receipts, producer_prior=dict(location=producer['location'], health=producer['health'], cash=ready['house']['credits'], power=ready['admitted_authorities']['house']['power'], drain=ready['admitted_authorities']['house']['drain']), products=ordinary['products'], final_wallet={k: no_rally['final_public']['house'][k] for k in ('credits', 'spent')}, rally=with_rally, limits=['Local snapshots/transmit order are mechanically selected from original initialized controls, not computed by VERA.', 'Selected Foot EMPTY and whole Walk CRT execute; inherited FPCW/options/House/map/prior-actor inputs and omitted full startup remain explicit.', 'Canonical advance_tick visits more phases/actors than the restricted native caller; absolute frames, no-rally Scatter destination and all-stream RNG parity are excluded.', 'HouseTechLevel10 and the already-ready producer are structural Rust test priors, not a whole House/startup/construction equivalence proof.', 'EVA registry is empty in the original joined fixture; stock UnitReady voice queue/cleanup is not established by this local fixture.'])


def infantry_unlimbo_gate(receipt, source_hash):
    """Select the sealed original Gate frontend readbacks; no gameplay math.

    Source: Gate manifest68c0abc2…, extract.py SHA1f6887db…. The supplied
    Foot4D7170 success seam and oversized native-only alias remain explicit.
    """
    rows = []
    for row in receipt['rows']:
        by_name = {}
        for event in row['events']:
            by_name.setdefault(event['name'], []).append(event)
        rows.append(dict(
            name=row['input']['name'], input=row['input'],
            requested_xyz=row['requested_xyz'],
            representable_frontend_input=row['input']['name'] != 'negative_linear_alias_outside_priority',
            map_fields=row['map_fields'], cell_before=row['cell_before'],
            gate_prior=row['gate_prior'], infantry_prior=row['infantry_prior'],
            incoming_membership=by_name.get('incoming_membership', []),
            retained_membership_diagnostic=row['retained_membership_diagnostic'],
            place=by_name.get('place', []),
            gate_lookups=by_name.get('ground_building_lookup', []),
            gate=by_name.get('is_open_gate', []),
            foot_handoff=by_name.get('foot_handoff', []),
            result=row['result'], raw_after=row['cell_after'],
            water_sentinel=row['water_sentinel'],
            random_requests=[event for event in row['events'] if event['name'].startswith('random_')],
            rng_before=row['rng_before'], rng_after=row['rng_after'],
            rng_summary={name: dict(
                before_sha256=hashlib.sha256(bytes.fromhex(row['rng_before'][name])).hexdigest(),
                after_sha256=hashlib.sha256(bytes.fromhex(row['rng_after'][name])).hexdigest(),
                before_words=list(struct.unpack('<3i', bytes.fromhex(row['rng_before'][name])[:12])),
                after_words=list(struct.unpack('<3i', bytes.fromhex(row['rng_after'][name])[:12])),
            ) for name in row['rng_before']},
        ))
    return dict(schema_version=1,
                source_receipt_sha256=source_hash, gate_reader=receipt['gate_reader'],
                bounds=receipt['bounds'] + [
                    'Negative linear alias -123264 exceeds the current Rust ground_pose XY storage range, whose documented lower limit is -32768. It is native-only range evidence.',
                    'The ordinary input -1/-128 and positive527 alias are representable by the existing Rust position authority; this selector does not claim Rust validation.',
                ], rows=rows)



def publication_phase_local_fixture(receipts, source_receipts, parent_manifest_sha256):
    """Literal observations only; no runtime decisions or arithmetic goldens."""
    cases = {}
    for (name, row) in receipts.items():
        initialized = row['full_original_initialized_first_place']
        placed = initialized['full_original_gi_observer']['primary_boundary'][0]
        strip = next((s for s in row['boundaries'] if s['label'] == 'actual_strip268_publication'))
        click = next((s for s in row['boundaries'] if s['label'] == 'authored_clear_cell_actual_building_click'))
        cases[name] = dict(frame=strip['before']['frame'], rally_request=click['cell'], incoming_factory=strip['before']['factory'], incoming_producer=strip['before']['producer'], what_action=click['what_action']['result'], original_clicked_action_result=click['click']['result'], executed_event_types=[bytes.fromhex(c['event_bytes'])[0] for c in row['original_calls'] if c['kind'] == 'event_execute'], placed=dict(pointer=placed['pointer'], id=placed['id'], position=placed['location'], mission=placed['mission'], queued=placed['queued'], archive=placed['archive'], archive_is_set=bool(placed['archive']), nav=placed['nav'], nav_is_set=bool(placed['nav']), producer_archive=placed['producer_archive'], producer_archive_is_set=bool(placed['producer_archive']), tether=bool(placed['tether']), contacts=placed['contacts'], walk_moving=bool(placed['walk']['moving']), walk_destination=placed['walk']['destination'], walk_paid_head=placed['walk']['paid_head']), after=row['control_state']['final'], click_rng_requests=click['requests'], click_raw_advances=click['raw_advances'])
    return dict(schema_version=1, source_receipts=source_receipts, inherited_parent_manifest_sha256=parent_manifest_sha256, cases=cases)
