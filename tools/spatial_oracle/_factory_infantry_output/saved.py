"""Saved native receipt/byte consistency checks; explicit errors survive -O."""
import hashlib
import json
import struct
from . import runtime as rt


def visit(value):
    yield value
    if isinstance(value, dict):
        for child in value.values():
            yield from visit(child)
    elif isinstance(value, list):
        for child in value:
            yield from visit(child)


def publication_phase_prefix(receipt):
    """Select the complete pre-input observations, without replacing decisions."""
    original = receipt['full_original_initialized_first_place']
    joined = original['full_original_primary']['joined']
    queue = joined['admitted_queue']['queue']
    rounds = next(step['rounds'] for step in queue['steps'] if 'rounds' in step)
    before = [row for row in rounds if row['frame'] < 268]
    rt.require(len(before) == 216 and before[0]['frame'] == 52 and before[-1]['frame'] == 267,
               'Publication control lost complete native rounds52..267')
    rt.require(rounds[-1]['frame'] == 268, 'Publication stop did not follow actual PLACE268')
    return dict(registered_startup=original['original_registered_walk_startup'],
        opening=joined['opening'],
        native_inputs={name: queue[name] for name in
            ('selected_inputs', 'physical_clear_map', 'barracks_constructor', 'map_diamond_prior', 'runtime_mode')},
        complete_rounds=before)


def validate_publication_phase_receipt(receipt, identity):
    """Check literal original return/write witnesses before full comparison."""
    name = receipt['order']
    rt.require(receipt['status'] == 'PASS' and receipt['failure'] is None,
               'Original publication control failed: ' + name)
    rt.require(receipt['control_state']['native_code_unchanged'], 'Original publication code changed')
    rt.require(receipt['caller_adaptation']['changes'] == dict(stop=1, product_count=1, terminal_assertions=1),
               'Publication caller/stop amendments differ')
    for key, row in receipt['complete_rng_buffers'].items():
        raw = bytes.fromhex(row['bytes'])
        rt.require(len(raw) == 0x3F4 and hashlib.sha256(raw).hexdigest() == row['sha256'],
                   'Incomplete publication RNG buffer: ' + key)
    for state in visit(receipt):
        if isinstance(state, dict) and set(state) == {'main', 'scenario', 'mapgen'}:
            if all(isinstance(x, str) and x.startswith(stream + ':') for stream, x in state.items()):
                rt.require(all(key in receipt['complete_rng_buffers'] and
                               receipt['complete_rng_buffers'][key]['stream'] == stream
                               for stream, key in state.items()), 'Publication RNG stream reference differs')
    original = receipt['full_original_initialized_first_place']
    rt.require(original['status'] == 'PASS' and original['failure'] is None,
               'Original publication PLACE boundary failed')
    strip = next(row for row in receipt['boundaries'] if row['label'] == 'actual_strip268_publication')
    factory = strip['before']['factory']
    rt.require(factory['stage'] == 54 and factory['changed'] == 1 and factory['balance'] == 0 and
               factory['suspended'] == 1, 'Original completed/changed publication prior differs')
    take = [row for row in receipt['original_calls'] if row['kind'] == 'factory_take_changed']
    rt.require(len(take) == 1 and take[0]['pc'] == '0x4c9c60' and take[0]['caller'] == '0x6a8dd8' and
               take[0].get('result') == 1 and take[0]['after']['factory']['changed'] == 0,
               'Actual Strip takeChanged return/write witness missing')
    click = next(row for row in receipt['boundaries'] if row['label'] == 'authored_clear_cell_actual_building_click')
    rt.require(click['what_action']['result'] == 1 and click['what_action']['entry'] == '0x447540' and
               click['click']['entry'] == '0x4436f0', 'Original admitted Building input differs')
    rt.require(click['requests'] == click['raw_advances'] == [] and click['before']['rng'] == click['after']['rng'],
               'Original class input acquired an RNG effect')
    rally = [row for row in receipt['original_calls'] if row['kind'] == 'set_rally']
    rt.require(len(rally) == 1 and rally[0]['caller'] == '0x4437ad', 'Original clicked-action caller differs')
    executors = [row for row in receipt['original_calls'] if row['kind'] == 'event_execute']
    rt.require(len(executors) == 2 and all(row['pc'] == '0x4c6cb0' and row['caller'] == '0x64c916' and
               'result' in row for row in executors), 'Original event return witness missing')
    types = [bytes.fromhex(row['event_bytes'])[0] for row in executors]
    rt.require(types == identity['executed_event_types'], 'Original publication event order differs')
    unlimbo = [row for row in receipt['original_calls'] if row['kind'] == 'infantry_unlimbo']
    rt.require(len(unlimbo) == 1 and unlimbo[0]['pc'] == '0x51dff0' and unlimbo[0]['caller'] == '0x444c9b' and
               unlimbo[0].get('result') == 1 and unlimbo[0]['requested_xyz'] == [3904, 4032, 0],
               'Original publication Infantry Unlimbo return missing')
    prefix = publication_phase_prefix(receipt)
    rt.require(rt.canonical_sha(rt.normalize(prefix)) == rt.metadata()['publication_phase']['complete_parent_prefix_sha256'],
               'Complete publication pre-input state differs from original P10')


def compare_publication_phase(order, actual):
    from tools import native_oracle as native
    identity = rt.metadata()['publication_phase']['controls'][order]
    validate_publication_phase_receipt(actual, identity)
    prior = rt.read_pinned(identity['receipt'])
    validate_publication_phase_receipt(prior, identity)
    expected, observed = rt.normalize(prior), rt.normalize(actual)
    historical_digest = rt.canonical_sha(expected)
    rt.require(historical_digest == identity['complete_normalized_original_sha256'],
               'Complete historical publication observation hash differs')
    difference = native.first_difference(expected, observed)
    rt.require(difference is None, 'Complete original publication observations differ: ' + str(difference))
    return dict(complete_original_observations_equal=True,
                complete_normalized_original_sha256=historical_digest,
                complete_native_comparable_sha256=rt.canonical_sha(observed),
                complete_warmed_prefix_rounds=216, first_frame=52, pre_input_last_frame=267)


def publication_phase_check():
    """Saved original interleavings; no emulation or whole-MainTick inference."""
    from tools import native_oracle as native
    from .fixture import publication_phase_local_fixture
    meta = rt.metadata()['publication_phase']
    image = native.image_bytes()
    source_manifest = rt.read_pinned(meta['source_manifest'])
    rt.require(source_manifest['parent_manifest_sha256'] == meta['parent_manifest_sha256'],
               'Publication source parent identity differs')
    closure = rt.read_pinned(meta['input_closure'])
    rt.require(closure['physical_inputs'] == rt.metadata()['physical_inputs'] and
               closure['native_runtime'] == rt.metadata()['native_runtime'] and
               closure['original_startup_parent_manifest_sha256'] == meta['parent_manifest_sha256'],
               'Publication physical/runtime/parent input closure differs')
    spans = rt.read_pinned(meta['original_instructions'])['spans']
    for row in spans:
        _, raw = native.file_span(image, row['address'], row['bytes'])
        rt.require(raw.hex() == row['original_bytes'], 'Original publication instruction span differs')
    receipts = {}
    comparisons = {}
    for order, identity in meta['controls'].items():
        receipt = rt.read_pinned(identity['receipt'])
        comparisons[order] = compare_publication_phase(order, receipt)
        receipts[order] = receipt
    selected =publication_phase_local_fixture(receipts, meta['local_sources'], meta['parent_manifest_sha256'])
    local = rt.REPO_ROOT / meta['rust_fixture']['path']
    raw = local.read_bytes()
    rt.require(len(raw) == meta['rust_fixture']['bytes'] and
               hashlib.sha256(raw).hexdigest() == meta['rust_fixture']['sha256'], 'Publication Rust fixture bytes changed')
    rt.require(json.loads(raw) == selected and raw == json.dumps(selected, indent=2).encode(),
               'Publication fixture differs from its single original native selector')
    before, after = (selected['cases'][order]['placed'] for order in ('before_strip', 'after_strip'))
    rt.require(before['archive'] == before['producer_archive'] != 0 and after['archive'] == 0 and
               after['producer_archive'] == before['producer_archive'], 'Original publication Archive causality differs')
    rt.require(before['nav_is_set'] and not after['nav_is_set'] and before['walk_moving'] and not after['walk_moving'] and
               before['mission'] == 2 and after['mission'] == 5, 'Original publication downstream route differs')
    return dict(schema=1, status='PASS', controls=comparisons,
        complete_rng_buffers_verified=True, original_static_spans=len(spans),
        mechanical_local_fixture_equal=True, rust_fixture_sha256=meta['rust_fixture']['sha256'],
        window_input_edge='Instruction-established only', whole_main_tick_parity_claimed=False,
        whole_original_emulation_run=False, whole_object_completion_claimed=False, limits=meta['bounds'])


def check():
    from tools import native_oracle as native
    meta = rt.metadata()
    rt.require(sorted(rt.HOST_KEYS) == sorted(meta['comparison_host_exclusions']['recursive_keys']),
        'Declared host comparison exclusions differ')
    image = native.image_bytes()
    for row in meta['original_static_bytes']:
        _, raw = native.file_span(image, row['address'], row['bytes'])
        rt.require(hashlib.sha256(raw).hexdigest() == row['sha256'],
            'Original instructions differ: ' + row['source'])
    table = rt.read_pinned('native-infantry-vtable.json')
    _, raw = native.file_span(image, table['request']['address'], table['request']['bytes'])
    rt.require(raw.hex() == table['result']['bytes'], 'Original GI virtual table differs')
    table_bytes = bytes.fromhex(table['result']['bytes'])
    controls = []
    for control in ('no_rally', 'rally'):
        primary = rt.read_accepted(control)
        projected = rt.read_pinned('accepted/' + control + '.private.json.gz')
        identity = meta['controls'][control]
        rt.require(rt.canonical_sha(rt.normalize(primary)) == identity['native_primary_sha256'],
            'Complete retained primary comparison changed: ' + control)
        rt.require(rt.canonical_sha(rt.normalize(projected)) == identity['native_private_projection_sha256'],
            'Retained private projection changed: ' + control)
        joined = primary['joined'] if control == 'rally' else primary
        queue = joined['admitted_queue']['queue']
        rt.require(primary['fault'] is None and joined['fault'] is None and queue['fault'] is None,
            'Accepted original primary has a fault')
        rt.require(queue['original_code_unchanged'] is True and projected['native_code_unchanged'] is True,
            'Accepted native code guard differs')
        rt.require(queue['final'] == projected['final_native_queue'], 'Private and Factory final boundaries differ')
        rt.require(queue['final']['frame'] == identity['final_frame'], 'Accepted boundary frame differs')
        for state in visit(queue):
            if isinstance(state, dict) and set(state) == {'main', 'scenario', 'mapgen'}:
                rt.require(all(isinstance(x, str) and len(bytes.fromhex(x)) == 0x3F4 for x in state.values()),
                    'Missing complete raw native RNG stream')
        streams = projected['all_three_complete_rng_states']
        for state in streams.values():
            raw = bytes.fromhex(state['bytes'])
            rt.require(len(raw) == 0x3F4 and hashlib.sha256(raw).hexdigest() == state['sha256'],
                'Original private RNG buffer differs')
        for state in visit(projected):
            if isinstance(state, dict) and set(state) == {'main', 'scenario', 'mapgen'}:
                if all(isinstance(index, int) for index in state.values()):
                    rt.require(all(streams[str(index)]['stream'] == name for name, index in state.items()),
                        'Private RNG reference differs')
                else:
                    rt.require(all(isinstance(x,str) and len(bytes.fromhex(x)) == 0x3F4 for x in state.values()),
                        'Private final boundary raw RNG differs')
        for obj in projected['live_virtual_identities']:
            rt.require(obj['original_bytes_equal'] is True, 'Runtime GI vtable does not match original')
            for offset, target in obj['slots'].items():
                rt.require(struct.unpack_from('<I', table_bytes, int(offset,16))[0] == target,
                    'Real GI virtual identity differs: ' + offset)
        controls.append(dict(control=control, final_frame=queue['final']['frame'],
            delivered=len(queue['final']['delivered_infantry']),
            complete_primary_native_sha256=identity['native_primary_sha256'],
            complete_private_native_sha256=identity['native_private_full_sha256'],
            private_projection_native_sha256=identity['native_private_projection_sha256']))
    fixture = rt.read_pinned('fixtures/ordinary-input-closure.json.gz')
    prior = rt.read_accepted('no_rally')
    queue = prior['admitted_queue']['queue']
    loop = next(x for x in queue['steps'] if x['label'] == 'ordered_native_sidebar_foot_factory_event_control')
    def expand(value):
        if isinstance(value, dict):
            if set(value) == {'main', 'scenario', 'mapgen'}:
                return {name: fixture['rng_states'][digest]['bytes'] for name,digest in value.items()}
            return {k: expand(v) for k,v in value.items()}
        if isinstance(value, list):
            return [expand(v) for v in value]
        return value
    for digest, row in fixture['rng_states'].items():
        raw = bytes.fromhex(row['bytes'])
        rt.require(len(raw) == row['length'] == 0x3F4 and hashlib.sha256(raw).hexdigest() == digest,
            'Compact native RNG state differs')
    for row in fixture['milestones']:
        source = next(x for x in loop['rounds'] if x['frame'] == row['frame'])
        state = {k:v for k,v in source.items() if k not in ('events', 'requests')}
        rt.require(state == expand(row['state']), 'Compact milestone state differs from original full receipt')
    rt.require(queue['final'] == expand(fixture['final']), 'Compact native final differs')
    rt.require(prior['opening']['supplied_opening_fields'] == fixture['producer_opening']['supplied_opening_fields'] == [],
        'Ordinary producer opening acquired a supplied field')
    initialized = {}
    for control, identity in meta['initialized_controls'].items():
        data = rt.read_pinned(identity['projection'])
        rt.require(data['control'] == control, 'Initialized native control identity differs')
        rt.require(data['complete_normalized_primary_sha256'] == identity['complete_primary_sha256'] and
            data['complete_normalized_private_sha256'] == identity['complete_private_sha256'],
            'Complete initialized comparison pin differs')
        startup = data['executed_registered_startup'][0]
        _, registered = native.file_span(image, startup['table_begin'], startup['table_end']-startup['table_begin'])
        rt.require(registered.hex() == startup['table_bytes'] and list(struct.unpack('<14I', registered)) == startup['entries'],
            'Registered original Walk initializer table differs')
        rt.require(startup['before']['rng'] == startup['after']['rng'] and not startup['requests'] and not startup['raw_advances'],
            'Registered original startup RNG state differs')
        foot = startup['foot_empty_registered_slot']
        _, registered = native.file_span(image, foot['table_begin'], foot['table_end']-foot['table_begin'])
        rt.require(registered.hex() == foot['bytes'] and struct.unpack('<I', registered)[0] == 0x4D3100,
            'Registered original Foot EMPTY slot differs')
        for digest, row in data['rng_states'].items():
            raw = bytes.fromhex(row['bytes'])
            rt.require(len(raw) == row['length'] == 0x3F4 and hashlib.sha256(raw).hexdigest() == digest,
                'Initialized complete RNG buffer differs')
        for row in visit(data):
            if isinstance(row, dict) and set(row) == {'main','scenario','mapgen'}:
                rt.require(all(digest in data['rng_states'] and name in data['rng_states'][digest]['streams']
                    for name,digest in row.items()), 'Initialized RNG stream reference differs')
        rt.require(len(data['final_private']) == 2, 'Both terminal native products missing')
        for obj in data['live_virtual_identities']:
            rt.require(obj['original_bytes_equal'] is True, 'Initialized real GI vtable differs')
            for offset, target in obj['slots'].items():
                rt.require(struct.unpack_from('<I', table_bytes, int(offset,16))[0] == target,
                    'Initialized native virtual slot differs')
        for obj in data['final_private']:
            rt.require(obj['nav'] == obj['archive'] == 0 and obj['mission'] == 5 and not obj['tether']
                and not any(obj['contacts']) and not obj['walk']['moving'] and not obj['walk']['motion']
                and not any(obj['walk']['paid_head']) and not any(obj['walk']['destination']),
                'Initialized terminal native cleanup differs')
        initialized[control] = data
    from .fixture import local_fixture
    expected_local = local_fixture(initialized['no_rally'], initialized['rally'], meta['initialized_local_sources'])
    local_path = rt.REPO_ROOT / 'src/sim/world/fixtures/factory_infantry_local_native.json'
    local = json.loads(local_path.read_bytes())
    rt.require(local == expected_local, 'Production local fixture differs from its single native selector')
    for row in controls:
        row['scope'] = 'Historical missing-Walk-startup supplied-prior control'
    return dict(schema_version=2, status='PASS', scope='Initialized native terminal controls and preserved historical controls',
        original_static_ranges=len(meta['original_static_bytes']),
        retained_files=len(meta['retained_files']),
        historical_controls=controls,
        initialized_controls=[dict(control=c,final_frame=d['measured_frames']['final'],
            complete_primary_native_sha256=d['complete_normalized_primary_sha256'],
            complete_private_native_sha256=d['complete_normalized_private_sha256']) for c,d in initialized.items()],
        compact_complete_rng_buffers=len(fixture['rng_states']),
        production_local_fixture_matches_single_selector=True,
        physical_inputs_checked=rt._ASSETS is not None, whole_original_emulation_run=False,
        whole_object_completion_claimed=False,limits=meta['initialized_limits'])
