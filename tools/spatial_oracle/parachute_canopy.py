"""Original PARACH attachment/AnimAI/whole-draw component observations.

The two fixed owner heights are supplied admitted inputs, not a descent. This
producer does not execute the whole Paradrop placement prefix or Display order.
Import and --help do not execute native code. See parachute_canopy.md.
"""
from copy import deepcopy
from pathlib import Path

from tools.native_oracle import configured_gamemd, finish_vectors, provenance
from tools.spatial_oracle import parachute_canopy_setup as setup


SIZE = [160, 160]
PIXELS = SIZE[0] * SIZE[1]
PLANES = {'color_words_rle', 'z_words_rle', 'a_words_rle'}


def selected(record, keys):
    """An explicit allowlist: never serialize heap addresses or resource bytes."""
    return {key: deepcopy(record[key]) for key in keys}


def palette(record):
    return selected(record, ('matches', 'shade_count', 'half_mask', 'quarter_mask'))


def object_state(record):
    """Keep actual getters and lifecycle fields, not the emulated allocation."""
    return selected(record, ('vtable', 'stored_xyz', 'actual_get_coords', 'alive',
        'limbo', 'marked', 'retained_display_layer', 'actual_get_layer'))


def attached_state(record):
    owner, anim = record['owner'], record['anim']
    assert owner['canopy'] == anim['pointer']
    assert anim['owner_object'] == owner['pointer']
    output = dict(owner=object_state(owner), anim=object_state(anim))
    output['owner'].update(selected(owner, ('health', 'is_parachuted', 'in_air')))
    output['owner']['has_this_canopy'] = True
    output['anim'].update(selected(anim, ('light_intensity_fc',
        'runtime_z_adjust_100', 'type_z_adjust_348', 'type_y_draw_offset_344')))
    output['anim'].update(owner_is_this_infantry=True,
        draw_flags=anim['constructor_state']['draw_flags'],
        runtime=deepcopy(anim['constructor_state']['runtime']),
        drawer_d4=palette(anim['drawer_d4']))
    return output


def ai_visit(record):
    keys = ('runtime', 'alive', 'limbo', 'marked', 'owner_stored_xyz',
            'anim_stored_xyz', 'identity_cursor')
    assert record['before']['owner_object'] == record['after']['owner_object']
    return dict(visit=record['visit'], clock=record['clock'],
        target_frame=record['target_frame'],
        before=selected(record['before'], keys),
        after=selected(record['after'], keys), owner_unchanged=True,
        scenario_rng_unchanged=(record['before']['scenario_rng_sha256']
                                == record['after']['scenario_rng_sha256']))


def surface_call(record, before_state):
    output = selected(record, ('caller', 'frame', 'point', 'clip', 'flags',
        'stack_argument_5', 'z_adjust', 'gradient', 'intensity', 'color',
        'has_zshape', 'zshape_frame', 'zshape_offset', 'source_frame',
        'before_planes', 'after_planes', 'leaves', 'returned'))
    output['source_is_physical_parach'] = record['source_is_physical_idle']
    assert output['source_is_physical_parach'] and output['returned']
    output['selectors'] = []
    for selector in record['selectors']:
        assert selector['converter'] == before_state['anim']['drawer_d4']['pointer']
        item = selected(selector, ('entry', 'caller', 'flags', 'shade_count',
            'half_mask', 'quarter_mask', 'vtable', 'first_three_entries'))
        item.update(uses_attached_convert=True,
                    converter_identity=palette(selector['converter_identity']))
        output['selectors'].append(item)
    assert output['selectors'] and output['leaves']
    output['writes'] = {
        key: selected(value, ('store_count', 'distinct_pixels', 'bounds_inclusive',
                              'pixel_runs_y_x_begin_x_end'))
        for key, value in record['writes'].items()
    }
    return output


def public_case(record):
    """Project observations before finish_vectors can retain a raw archive.

    All planes and selected scalar measurements survive. Allocator pointers,
    source-resource snippets, lexical INI sections, private paths and identities
    are intentionally not publication inputs.
    """
    assert record['stage'] == 'draw-controls' and record['attachment'] == 'suffix'
    assert len(record['draws']) == 4
    assert record['checked_original_text_and_selected_vtables_unchanged']
    reader_keys = ('name', 'art_body_read', 'start', 'loop_start', 'loop_end',
        'end', 'loop_count', 'rate', 'shadow', 'bouncer', 'normalized',
        'raw_shp_frame_count', 'image', 'random_rate', 'type_z_adjust_348',
        'type_y_draw_offset_344')
    output = dict(supplied_owner_height=record['supplied_owner_height'],
        anim_reader=selected(record['anim_reader'], reader_keys),
        after_attachment=attached_state(record['after_attachment']),
        after_draw=attached_state(record['after_draw']),
        ai_visits=record['ai_visits'],
        ai_history=[ai_visit(visit) for visit in record['ai_history']],
        observed_cell_light_10a=record['observed_cell_light_10a'],
        owner_virtuals=deepcopy(record['owner_virtuals']),
        performance=selected(record['performance'], ('before', 'supplied_priors',
            'history', 'after', 'reached', 'checked_text_unchanged')),
        presentation_sinks=[selected(sink, ('phase', 'entry', 'caller'))
                            for sink in record['presentation_sinks']],
        checked_original_text_and_selected_vtables_unchanged=True,
        draws=[])
    for row in record['draws']:
        assert row['size'] == SIZE and len(row['calls']) == 1
        output_row = selected(row, ('name', 'entry', 'returned_al', 'size',
            'clock', 'ai_visits', 'requested_frame', 'supplied_owner_height',
            'camera', 'surface_input', 'a_unchanged', 'z_unchanged', 'rng_entries',
            'performance_before', 'performance_after', 'original_fps_gate_calls',
            'checked_selector_vtables_unchanged', 'before_planes', 'after_planes'))
        output_row.update(state_before=attached_state(row['state_before']),
            state_after=attached_state(row['state_after']),
            received_drawit=deepcopy(row['received_drawit']),
            calls=[surface_call(row['calls'][0], row['state_before'])])
        output['draws'].append(output_row)
    return output


def generate():
    assets, metadata = setup.physical_inputs(configured_gamemd().parent)
    return dict(schema=1, inputs=dict(size=SIZE, clip=[0, 0, 160, 160],
        camera=[-384, 380], owner_xy=[2688, 5248], owner_heights=[104, 232],
        constructor_clock=1000, observed_frames=[0, 20], max_ai_visits=512,
        admitted_owner=True,
        background=0xBDF7, a_value=127, depth_baseline=32768,
        z_priors=dict(empty='65535 everywhere',
            left_half_zero='0 where x<80, otherwise65535; every row')),
        cases=[public_case(setup.execute(assets, metadata, 'draw-controls',
                                         'suffix', height))
               for height in (104, 232)])


def plane_text(runs):
    """Lossless count:word RLE; every plane must cover the whole viewport."""
    total = 0
    for run in runs:
        if not isinstance(run, list) or len(run) != 2:
            raise ValueError('Expected two-integer plane runs')
        count, word = run
        if type(count) is not int or type(word) is not int:
            raise ValueError('Plane runs require integer count and word')
        if not 0 < count <= PIXELS - total or not 0 <= word <= 65535:
            raise ValueError('Plane run exceeds the complete uint16 viewport')
        total += count
    if total != PIXELS:
        raise ValueError('Plane must contain exactly160x160 words')
    text = ' '.join(f'{count}:{word}' for count, word in runs)
    if [[int(value) for value in pair.split(':')] for pair in text.split()] != runs:
        raise ValueError('Plane encoding changed native words')
    return text


def compact_projection(data):
    """Remove duplicate planes only after comparing whole draw and Surface.

    The sanitized unprojected observations remain in a deterministic raw archive.
    No scalar native result or plane word is changed by the text encoding.
    """
    if data['schema'] != 1 or len(data['cases']) != 2:
        raise ValueError('Expected two fixed-height cases')
    output = dict(data, plane_encoding='decimal count:word RLE, row-major uint16')
    cases = []
    for case in data['cases']:
        if len(case['draws']) != 4:
            raise ValueError('Every height must keep both frames and both Z inputs')
        rows = []
        for row in case['draws']:
            if row['size'] != SIZE or len(row['calls']) != 1:
                raise ValueError('Expected a single-call complete160x160 draw')
            call = dict(row['calls'][0])
            for boundary in ('before_planes', 'after_planes'):
                if row[boundary] != call[boundary] or set(call[boundary]) != PLANES:
                    raise ValueError('Whole-draw and Surface color/Z/A planes differ')
                call[boundary] = {key: plane_text(runs)
                                  for key, runs in call[boundary].items()}
            rows.append(dict({key: value for key, value in row.items()
                              if key not in ('before_planes', 'after_planes')},
                             calls=[call]))
        cases.append(dict(case, draws=rows))
    output['cases'] = cases
    return output


def metadata():
    return provenance(
        scope='Eight physical PARACH whole DrawIfVisible/raster observations: '
              'two admitted fixed E1 heights, constructorframe0 and original-AI '
              'frame20, each on empty and split destination Z.',
        assumptions=[
            'Two fresh shared TMP/palette/map/House VMs execute real physical '
            'palette/Convert startup, Country5113F0, House4F54A0, InfantryType5236A0 '
            'and layered E1 reader5240A0. Full selected E1 weapon/projectile/warhead '
            'readers and postpass complete the real Infantry517A50 constructor; '
            'COM creation reaches the original Walk6C4790 factory. The inherited '
            'GAPOWR setup is a House/map dependency and is never drawn here.',
            'Existing immutable MIX/INI caches transport unchanged selected retail '
            'resources from the configured executable directory. The original '
            'archive/physical INI parser is excluded. Full physical PARACH reader '
            '427D00/image load427B50 and General Parachute66E561..66E59F execute. '
            'Resource payloads and lexical sections are excluded from publication.',
            'Admitted ownerXYZ2688/5248 with fresh fixed heights104 or232, alive, '
            'marked/nonlimbo/inAir, sparse revealed flat Cells and original '
            'normalized LocalSize are explicit component priors. Only original '
            'ObjectParadrop suffix5F5A9D..5F5B3E executes, including Anim421EA0, '
            'SetOwner424B50, owner coordinate/remap/cell virtuals and palette/light '
            'stores. Whole5F5940 admission, Unlimbo, placement, aircraft cargo, '
            'owner descent/body actions and landed playout are excluded.',
            'Each fixed owner remains unchanged. Original AnimAI423AC0 advances '
            'from constructorframe0 to actualframe20 through global clock visits '
            'only, bounded512 visits. No frame/timer/rate/loop/shape value is '
            'rewritten. This is not a whole Logic/MainTick schedule.',
            'Original RGB565 masks4BAA73..4BAB06, discovery geometry and Tactical '
            'matrix startup run. Camera(-384,380), clock1000, speed4 and detail2 '
            'are declared inputs. Original Logic-counter/Throttle timing setup '
            'produces FPS60 from60 counter visits with OS times0/960ms; minimum '
            'FPS queries remain original. Actual host performance is excluded.',
            'Every complete422C70/5F4B10/422CA0 draw starts from160x160 RGB565BDF7, '
            'A127 and either Z65535 everywhere or Z0 atx<80/Z65535 elsewhere on '
            'every row. The split is independent of source crop or observed '
            'pixels. Selected4AED70/Convert/decoder/raster bodies execute, with '
            'full color/Z/A before/after planes and store coverage retained. '
            'No output pixel or native decision is computed by a host formula.',
            'Original text and selected class/raster vtables remain unchanged. '
            'Only semantic fields and exact planes are published; allocator '
            'pointers, raw resource snippets and file identities are omitted. '
            'No terrain/body drawing, retained Display traversal, full-scene '
            'occlusion or complete game parity is claimed.',
        ],
        substitutions=[
            'Shared PaletteReader heap/delete/TLS/platform and immutable file '
            'transports; supplied empty registry capacity and sparse map/House '
            'priors. Pristine missing vector headers use original CRT prefixes '
            'before unrelated exit registration, without replacing live vectors.',
            'OS Interlocked/GUID/COM transports, successful OleRun/atexit and '
            'setup clock boundaries. The original Walk constructor, native '
            'readers and returned gameplay/draw decisions remain live.',
            'Object Mark5F5850 and Display registration4A9720 return1 as recorded '
            'presentation sinks. Native retained membership and order are '
            'therefore outside this corpus. No type/frame/palette/light/getter, '
            'selector/decoder or pixel answer is supplied.',
        ],
        entry_points=dict(infantry_type=0x5236A0, infantry_reader=0x5240A0,
            infantry=0x517A50, walk_factory=0x6C4790,
            weapon_reader=0x772080, projectile_reader=0x46BEE0,
            warhead_reader=0x75D3A0, weapon_postpass=0x7729F0,
            general_parachute_begin=0x66E561, general_parachute_end=0x66E59F,
            anim_reader=0x427D00, image_load=0x427B50,
            admitted_paradrop_suffix=0x5F5A9D, anim=0x421EA0,
            set_owner=0x424B50, coords=0x422BE0, layer=0x424CB0,
            ai=0x423AC0, draw_if_visible=0x422C70, object_projection=0x5F4B10,
            draw=0x422CA0, shape=0x4AED70,
            plain_selector=0x490B90, rle_selector=0x490E50,
            format_masks_begin=0x4BAA73, format_masks_end=0x4BAB06))


def source_paths():
    """Protect the native producers/transports for the duration of each run."""
    from tools.spatial_oracle.building_anim_translucency import source_paths as shared
    paths = shared()
    root = Path(__file__).resolve().parents[2]
    for name in ('tools/spatial_oracle/parachute_canopy.py',
                 'tools/spatial_oracle/parachute_canopy_setup.py'):
        paths[name] = root / name
    return paths


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'),
                   provenance=metadata, source_paths=source_paths(),
                   projection=compact_projection)
