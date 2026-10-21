"""Original attached GAPOWR_A FPS/detail admission and ordered hysteresis.

Imports and --help are inert. See building_anim_detail.md for physical inputs,
original call boundaries, supplied process priors and the admission-only scope.
"""
from collections import deque
import hashlib
from pathlib import Path
import struct

from tools.native_oracle import configured_gamemd, finish_vectors, provenance


def sha(raw):
    """Digest a runtime/asset observation, never executable or source identity."""
    return hashlib.sha256(raw).hexdigest()


def performance_state(m):
    u = m.u
    return dict(options_detail=m.read32(0xA8EB78),
        logic_count=m.read32(0xABCD40), current_fps=m.read32(0xABCD44),
        total=m.read32(0xABCD48), buckets=m.read32(0xABCD4C),
        reduced=bool(u.mem_read(0xABCD50, 1)[0]),
        timer_words=list(struct.unpack('<3I', u.mem_read(0xABCD88, 12))),
        timer_initialized=bool(u.mem_read(0xABCD94, 1)[0]),
        minimum=m.read32(0x829FF4), buffer=m.read32(0x829FF8))

def physical_image_names(u, pointer, assets, names):
    if not pointer:
        return []
    return [name for name in names if assets.get(name) is not None
            and bytes(u.mem_read(pointer, len(assets[name]))) == assets[name]]

def converter_identity(m, pointer):
    u = m.u
    identities = []
    if pointer == m.read32(0x87F6C0):
        identities.append('global_ANIM_PAL')
    if pointer == m.read32(0x87F6C4):
        identities.append('global_PALETTE_PAL')
    count, items = m.read32(0xB054E0), m.read32(0xB054D4)
    schemes = []
    for index in range(count):
        scheme = m.read32(items + index * 4)
        converter = m.read32(scheme + 0x30C)
        schemes.append(dict(index=index, pointer=f'{scheme:08X}', converter=f'{converter:08X}'))
        if pointer == converter:
            identities.append(f'constructed_LightGold_local_index_{index}')
    result = dict(pointer=f'{pointer:08X}', matches=identities, schemes=schemes,
                  global_anim=f'{m.read32(0x87F6C0):08X}',
                  global_palette=f'{m.read32(0x87F6C4):08X}')
    if pointer:
        shades, table = m.read32(pointer + 0x16C), m.read32(pointer + 0x170)
        assert 0 < shades < 256 and table
        result.update(shade_count=shades,
            lookup_table_sha256=sha(u.mem_read(table, shades * 512)),
            half_mask=m.read32(pointer + 0x180), quarter_mask=m.read32(pointer + 0x184))
    return result

def palette_receipt(m, anim):
    u = m.u
    typ = m.read32(anim + 0xC8)
    return dict(anim_drawer_d4=converter_identity(m, m.read32(anim + 0xD4)),
                anim_light_fc=struct.unpack('<i', u.mem_read(anim + 0xFC, 4))[0],
                type_should_use_cell_drawer_35c=bool(u.mem_read(typ + 0x35C, 1)[0]),
                type_use_normal_light_35d=bool(u.mem_read(typ + 0x35D, 1)[0]),
                boundary='Observed actual slot451890 palette/light stores and draw selector pointers; +CC is recorded only as attached-object state, not treated as palette ownership.')

def slot_state(m, building, anim, slot):
    from tools.spatial_oracle.anim_bouncer_launch import constructor_state
    u = m.u
    state = constructor_state(u, anim)
    state.update(pointer=f'{anim:08X}', type_name=m.string(m.read32(anim + 0xC8) + 0x24),
        attached_object_raw_cc=f'{m.read32(anim + 0xCC):08X}',
        attached_object_is_building=m.read32(anim + 0xCC) == building,
        palette=palette_receipt(m, anim),
        is_building_anim=bool(u.mem_read(anim + 0x118, 1)[0]),
        forced_translucency=bool(u.mem_read(anim + 0x119, 1)[0]),
        temporal_paused=bool(u.mem_read(anim + 0x11A, 1)[0]),
        temporal_resume_pending=bool(u.mem_read(anim + 0x11B, 1)[0]),
        temporal_saved_frame=m.read32(anim + 0x11C),
        stored_xyz=list(struct.unpack('<3i', u.mem_read(anim + 0x9C, 12))),
        y_sort=struct.unpack('<i', u.mem_read(anim + 0x104, 4))[0])
    scenario = m.read32(0xA8B230)
    return dict(identity_cursor=m.read32(scenario + 0x214),
        scenario_rng_sha256=sha(u.mem_read(scenario + 0x218, 0x3F4)),
        building=dict(pointer=f'{building:08X}',
        warped=bool(u.mem_read(building + 0x270, 1)[0]),
        online=bool(u.mem_read(building + 0x660, 1)[0]),
        temporal_head=f'{m.read32(building + 0x278):08X}',
        slot=slot, slot_pointer=f'{m.read32(building + 0x55C + slot * 4):08X}',
        location=list(struct.unpack('<3i', u.mem_read(building + 0x9C, 12)))), anim=state)

def execute(assets, metadata):
    from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE, UC_HOOK_MEM_INVALID
    from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX,
        UC_X86_REG_EDX, UC_X86_REG_ESI, UC_X86_REG_EIP, UC_X86_REG_ESP)
    from tools import native_oracle as native
    from tools.procedural_drawing_oracle import shroud
    from tools.projectile_oracle.bridge_render_shape import prepare_surface, PIXELS, Z, A, SURFACE
    from tools.spatial_oracle.anim_bouncer_launch import constructor_state
    from tools.spatial_oracle.building_body_rules import INI, SP, dwords
    from tools.spatial_oracle.trigger_action_values import initialize_discovery_geometry

    from tools.spatial_oracle import building_anim_detail_setup as helper
    scene = helper.prepare_power_plant_scene(shroud, assets, metadata)
    m, u = scene['m'], scene['u']
    building, typ, clip = (scene[key] for key in ('building', 'typ', 'clip'))
    body = m.read32(typ + 0xA4)
    body_names = physical_image_names(u, body, assets, ('GAPOWR.SHP', 'GTPOWR.SHP', 'GGPOWR.SHP'))
    assert body_names, 'Original selected Building body must match a supplied physical resource'
    assert not any(struct.unpack('<21I', u.mem_read(building + 0x55C, 84)))
    code_before = bytes(u.mem_read(0x401000, 0x3E0000))
    vtables = {p: bytes(u.mem_read(p, size)) for p, size in (
        (0x7E3EBC, 0x500), (0x7E3354, 0x210), (0x7E3608, 0xA0),
        (0x7E2070, 0x100), (0x7F4348, 0x100), (0x7E9F24, 0x30))}
    assert m.read32(0x7E3354 + 0x104) == 0x422C70
    assert m.read32(0x7E3354 + 0xAC) == 0x41BE00
    assert m.read32(0x7E3354 + 0x48) == 0x422BE0
    live_vectors = {p: bytes(u.mem_read(p, 24)) for p in
                    (0xA8E360, 0xB0F720, 0xB0F670, 0xB0F618, 0xA8EC50)}
    # Only geometry constants are initialized; do not repeat the helper which
    # empties Abstract/Object/Techno registries after live Building/House creation.
    geometry = initialize_discovery_geometry(m.invoke, m.read32, u)
    assert m.read32(0x89E7C0) == m.read32(0xABDE88) == 104
    assert all(raw == bytes(u.mem_read(p, 24)) for p, raw in live_vectors.items())
    # The body-only Tactical had only0x200 bytes. Use a correctly sized data
    # receiver, retain that supplied viewport/camera state, and execute the
    # original constructor's matrix stores rather than supplying conversions.
    old_tactical = scene['tactical']
    tactical = m.alloc(0x1200)
    u.mem_write(tactical, bytes(u.mem_read(old_tactical, 0x200)))
    u.reg_write(UC_X86_REG_ESI, tactical)
    u.reg_write(UC_X86_REG_EBX, 0)
    u.reg_write(UC_X86_REG_ESP, SP)
    native.run_checked(u, 0x6D1DC5, 0x6D1E24, count=1000)
    assert m.read32(0x887324) == tactical
    scene['tactical'] = tactical
    u.mem_write(tactical + 0xB0, dwords(-384, 380))
    matrix = dict(entry='006D1DC5', stop_before='006D1E24',
                  pixel_to_coord_matrix_hex=bytes(u.mem_read(tactical + 0xDE4, 48)).hex(),
                  boundary='Original Tactical constructor matrix tail only; existing supplied viewport retained, no full window construction.')
    vector = 0xA8E9A8
    vector_before = list(struct.unpack('<6I', u.mem_read(vector, 24)))
    assert vector_before[4] == 0, ('Unexpected existing Anim instances', vector_before)
    if vector_before[0] != 0x7E9F24:
        assert vector_before[1] == vector_before[2] == 0
        u.mem_write(SP, dwords(native.RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP, SP)
        native.run_checked(u, 0x4E6D60, 0x4E6D96, count=100,
                           required_addresses=(0x4E6D7D, 0x4E6D91))
        assert u.reg_read(UC_X86_REG_ESP) == SP - 4
        assert m.read32(SP - 4) == 0x4E6DA0
        assert list(struct.unpack('<6I', u.mem_read(vector, 24))) == [0x7E9F24, 0, 0, 1, 0, 10]
        registry_action = 'Original CRT4E6D60..4E6D96; stop before atexit only'
    else:
        registry_action = 'Retain already initialized empty Anim vector'
    registry = dict(before=vector_before, after=list(struct.unpack('<6I', u.mem_read(vector, 24))),
                    action=registry_action, live_object_vectors_preserved=True)
    # Object5F3B80 searches these five listener vectors even when no listener
    # names this Anim. Complete only pristine missing CRT headers before
    # their original queries (including5F3BBF); never reset an
    # established registry (in particular Reader's supplied B0F670 storage).
    # Prefix/typed-vptr stores are the original ones also documented by
    # building_slot_replacement.VECTORS; all stop before atexit registration.
    retirement_initializers = (
        (0xB0F698, 0x725850, 0x725886, 0x72586D, 0x7E91EC, 0x725890),
        (0xA8E360, 0x4E7AE0, 0x4E7B16, 0x4E7AFD, 0x7E4F64, 0x4E7B20),
        (0xB0F720, 0x7252D0, 0x725306, 0x7252ED, 0x7E91EC, 0x725310),
        (0xB0F670, 0x725350, 0x725386, 0x72536D, 0x7E91EC, 0x725390),
        (0xB0F618, 0x7253D0, 0x725406, 0x7253ED, 0x7E91EC, 0x725410),
    )
    guarded_vectors = tuple(dict.fromkeys(
        [row[0] for row in retirement_initializers]
        + list(live_vectors) + [vector, 0xA83C68, 0xA80228, 0xA83C98]))
    retirement_before = {p: bytes(u.mem_read(p, 24)) for p in guarded_vectors}
    retirement_setup = []
    initialized_retirement_headers = set()
    for address, begin, stop, store_pc, vptr, destructor in retirement_initializers:
        before = retirement_before[address]
        executed = m.read32(address) == 0
        if executed:
            assert before == bytes(24), ('Partial missing destructor registry', hex(address), before.hex())
            u.reg_write(UC_X86_REG_ESP, SP)
            native.run_checked(u, begin, stop, count=32,
                required_addresses=(store_pc,),
                context=dict(component='building_anim_detail',
                             phase='Object destructor registry startup',
                             registry=f'{address:08X}'))
            assert u.reg_read(UC_X86_REG_ESP) == SP - 4
            assert m.read32(SP - 4) == destructor
            assert list(struct.unpack('<6I', u.mem_read(address, 24))) == [vptr, 0, 0, 1, 0, 10]
            initialized_retirement_headers.add(address)
            u.reg_write(UC_X86_REG_ESP, SP)
        retirement_setup.append(dict(address=f'{address:08X}',
            before_words=list(struct.unpack('<6I', before)),
            after_words=list(struct.unpack('<6I', u.mem_read(address, 24))),
            action='original_CRT_prefix' if executed else 'preserve_established_header',
            entry=f'{begin:08X}', stop_before_atexit=f'{stop:08X}',
            required_vptr_store=f'{store_pc:08X}', original_vptr=f'{vptr:08X}',
            executed=executed))
    assert all(before == bytes(u.mem_read(p, 24))
               for p, before in retirement_before.items()
               if p not in initialized_retirement_headers)
    assert code_before == bytes(u.mem_read(0x401000, 0x3E0000))
    assert all(raw == bytes(u.mem_read(p, len(raw))) for p, raw in vtables.items())
    retirement_registry = dict(headers=retirement_setup,
        established_headers_preserved=True,
        guarded_headers=[f'{p:08X}' for p in guarded_vectors],
        original_text_and_vtables_unchanged=True,
        boundary='Only original CRT prefixes for wholly pristine missing Object-destructor listener headers, before any Anim slot constructor; no live/header reset, supplied vptr, FindIndex result, destructor or callback replacement. Established inherited headers remain byte-identical.')
    # Supplied admitted match/draw timing, not a Game startup or frame traversal.
    u.mem_write(0xA8E9A0, b'\1')
    u.mem_write(0xA8E7AC, dwords(0))
    u.mem_write(0xA8ED84, dwords(1000))
    u.mem_write(0xA8EB60, dwords(4))
    performance_setup = None  # Installed after the real slot constructor, below.
    slot = 3
    record = typ + 0xF4C + slot * 0x44
    slot_input = dict(slot=slot, name=m.string(record),
        damaged_name=m.string(record + 0x10), garrisoned_name=m.string(record + 0x20),
        pixel_offset=list(struct.unpack('<ii', u.mem_read(record + 0x30, 8))),
        z_adjust=struct.unpack('<i', u.mem_read(record + 0x38, 4))[0],
        y_sort=struct.unpack('<i', u.mem_read(record + 0x3C, 4))[0],
        powered=bool(u.mem_read(record + 0x40, 1)[0]),
        building_location=list(struct.unpack('<iii', u.mem_read(building + 0x9C, 12))),
        building_damaged=bool(u.mem_read(building + 0x6E6, 1)[0]),
        building_stuff_enabled=bool(u.mem_read(building + 0x6EA, 1)[0]),
        native_body_frame=m.invoke(0x43EF90, building))
    assert slot_input['name'] == 'GAPOWR_A'
    selected = dict(metadata['lexical_art']['GAPOWR_A'])
    phase, reached, setup_calls, setup_sinks, recent = 'reader', set(), [], [], deque(maxlen=48)
    setup_rng = []
    failure = None

    def setup_observer(_u, pc, size, _data):
        recent.append(f'{pc:08X}')
        if pc in (0x428B80, 0x427530, 0x427D00, 0x427B50, 0x451890, 0x6D2360,
                  0x459EF0, 0x421EA0, 0x424CE0, 0x423AC0, 0x422C70, 0x5F4B10,
                  0x41BE00, 0x422BE0, 0x6D2140, 0x422CA0, 0x4AED70,
                  0x71B0EA, 0x4521C0, 0x71ABC0, 0x452210, 0x70C5B0, 0x451B25):
            reached.add(pc)
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc in (0x65C780, 0x65C7E0):
            setup_rng.append(dict(phase=phase, entry=f'{pc:08X}',
                caller=f'{m.read32(sp):08X}', stream=f'{u.reg_read(UC_X86_REG_ECX):08X}'))
        if pc == 0x421EA0:
            args = list(struct.unpack('<7I', u.mem_read(sp + 4, 28)))
            setup_calls.append(dict(phase=phase, entry='00421EA0', caller=f'{m.read32(sp):08X}',
                type_name=m.string(args[0] + 0x24),
                received_coord=list(struct.unpack('<iii', u.mem_read(args[1], 12))),
                delay=args[2], loops=args[3], flags=args[4], z_adjust=args[5], reverse=args[6]))
        if pc in (0x45190F, 0x451920):
            pointer = u.reg_read(UC_X86_REG_EAX)
            setup_calls.append(dict(phase=phase, return_pc=f'{pc:08X}',
                result_coord=list(struct.unpack('<iii', u.mem_read(pointer, 12)))))
        # Same two explicitly bounded presentation sinks as the independently
        # successful direct Anim probe. Neither supplies a coordinate or draw.
        if pc in (0x5F5850, 0x4A9720):
            setup_sinks.append(dict(phase=phase, entry=f'{pc:08X}',
                caller=f'{m.read32(sp):08X}', receiver=f'{u.reg_read(UC_X86_REG_ECX):08X}',
                argument=m.read32(sp + 4)))
            m.ret(1, 4)

    def invalid(_u, access, address, size, value, _data):
        nonlocal failure
        failure = dict(phase=phase, access=access, address=f'{address:08X}', size=size,
            value=value, eip=f'{u.reg_read(UC_X86_REG_EIP):08X}', recent=list(recent))
        return False

    hooks = [u.hook_add(UC_HOOK_CODE, setup_observer), u.hook_add(UC_HOOK_MEM_INVALID, invalid)]
    try:
        m.make_ini({'GAPOWR_A': selected})
        anim_type = m.invoke(0x428B80, m.cstring('GAPOWR_A'))
        mark = len(m.asset_loaded)
        admitted = m.invoke(0x427D00, anim_type, (INI,))
        reader = m.result('GAPOWR_A', anim_type, admitted)
        reader.update(physical_art_keys=selected, asset_loads=m.asset_loaded[mark:],
            flat=bool(u.mem_read(anim_type + 0x369, 1)[0]),
            y_draw_offset=struct.unpack('<i', u.mem_read(anim_type + 0x344, 4))[0],
            z_adjust=struct.unpack('<i', u.mem_read(anim_type + 0x348, 4))[0])
        idle_image = m.read32(anim_type + 0xA4)
        idle_names = physical_image_names(u, idle_image, assets, ('GAPOWR_A.SHP', 'GTPOWR_A.SHP', 'GGPOWR_A.SHP'))
        assert idle_names, 'Original selected Anim image must match a supplied physical resource'
        reader.update(bound_physical_names=idle_names,
            native_detail_level=m.read32(anim_type + 0x2D4),
            should_use_cell_drawer=bool(u.mem_read(anim_type + 0x35C, 1)[0]),
            use_normal_light=bool(u.mem_read(anim_type + 0x35D, 1)[0]))
        assert reader['art_body_read'] and not reader['shadow'] and not reader['flat']
        assert reader['native_detail_level'] == 2, 'Physical GAPOWR_A detail must remain unmodified'
        phase = 'original_slot_creation'
        scenario = m.read32(0xA8B230)
        identity_before = m.read32(scenario + 0x214)
        random_before = sha(u.mem_read(scenario + 0x218, 0x3F4))
        # Pass the actual record name retained by the physical Building ART
        # reader. No slot, Anim coordinate, frame or draw flag is written here.
        m.invoke(0x451890, building, (record, slot, 0, 0, 0))
        anim = m.read32(building + 0x55C + slot * 4)
        assert anim and m.read32(anim) == 0x7E3354 and m.read32(anim + 0xC8) == anim_type
        assert [index for index, pointer in enumerate(struct.unpack('<21I', u.mem_read(building + 0x55C, 84))) if pointer] == [slot]
        constructed = constructor_state(u, anim)
        constructed.update(stored_xyz=list(struct.unpack('<iii', u.mem_read(anim + 0x9C, 12))),
            attached_object_raw_cc=f'{m.read32(anim + 0xCC):08X}', is_building_anim=bool(u.mem_read(anim + 0x118, 1)[0]),
            y_sort=struct.unpack('<i', u.mem_read(anim + 0x104, 4))[0])
        construction_effects = dict(identity_cursor_before=identity_before,
            identity_cursor_after=m.read32(scenario + 0x214),
            scenario_rng_before_sha256=random_before,
            scenario_rng_after_sha256=sha(u.mem_read(scenario + 0x218, 0x3F4)))
        assert constructed['runtime']['current_frame'] == 0
        assert constructed['draw_flags'] == 0x1600 and constructed['is_building_anim']
        constructed['palette'] = palette_receipt(m, anim)
        # One real slot at its constructor-selected frame is retained for all
        # process-performance and Options controls. No AI/frame/timer/flag writes.
        rows = []
        performance = PerformanceDriver(m)
        performance_setup = performance.receipt
        try:
            performance.advance(0, 0)
            for fps, details, label in (
                (0, (0, 1, 2), 'zero'),
                (60, (0, 1, 2), 'sixty'),
                (14, (2,), 'below_minimum'),
                (19, (2,), 'below_recovery'),
                (20, (2,), 'at_recovery'),
                (19, (2,), 'above_minimum_after_recovery'),
                (15, (2,), 'at_minimum_after_recovery'),
                (14, (2,), 'below_minimum_again'),
                (60, (2,), 'final_admitted'),
            ):
                performance.advance(performance.now + 960, fps)
                assert performance_state(m)['current_fps'] == fps
                for detail in details:
                    phase = f'{label}_fps{fps}_options{detail}'
                    # Supplied process Options prior, not an ART/type mutation.
                    u.mem_write(0xA8EB78, dwords(detail))
                    before = slot_state(m, building, anim, slot)
                    row = draw_admission(scene, anim, idle_image, phase)
                    row.update(state_before=before,
                        state_after=slot_state(m, building, anim, slot))
                    rows.append(row)
                    assert row['state_before'] == row['state_after'], phase
            # Same Anim/type/frame/light, deliberately offscreen camera. This
            # tests only an earlier component projection rejection; no gate is
            # overwritten and whole DrawIfVisible still executes.
            old_camera = bytes(u.mem_read(tactical + 0xB0, 8))
            try:
                u.mem_write(tactical + 0xB0, dwords(100000, 100000))
                phase = 'offscreen_camera_fps60_options2'
                before = slot_state(m, building, anim, slot)
                row = draw_admission(scene, anim, idle_image, phase)
                row.update(state_before=before,
                    state_after=slot_state(m, building, anim, slot),
                    supplied_camera=[100000, 100000])
                rows.append(row)
                assert row['state_before'] == row['state_after'], phase
            finally:
                u.mem_write(tactical + 0xB0, old_camera)
            # Strong earlier-projection control: FPS60 left the native latch
            # clear. Publish14 through the same original counter/Throttle owner
            # without querying, then compare offscreen and restored camera at
            # that exact retained FPS. Record the native latch/call count; do
            # not fill visibility or hysteresis answers with a host predicate.
            performance.advance(performance.now + 960, 14)
            assert performance_state(m)['current_fps'] == 14
            for offscreen in (True, False):
                phase = ('offscreen' if offscreen else 'restored') + '_camera_fps14_options2'
                try:
                    u.mem_write(tactical + 0xB0,
                        dwords(100000, 100000) if offscreen else old_camera)
                    before = slot_state(m, building, anim, slot)
                    row = draw_admission(scene, anim, idle_image, phase)
                    row.update(state_before=before,
                        state_after=slot_state(m, building, anim, slot),
                        supplied_camera=[100000, 100000] if offscreen else [-384, 380])
                    rows.append(row)
                    assert row['state_before'] == row['state_after'], phase
                finally:
                    u.mem_write(tactical + 0xB0, old_camera)
            # One genuine ordinary AI visit after every admission observation
            # proves that this draw-only sequence has not retired the Anim.
            phase = 'post_admission_ordinary_ai'
            ai_before = slot_state(m, building, anim, slot)
            u.mem_write(0xA8ED84, dwords(1001))
            m.invoke(0x423AC0, anim)
            ai_after = slot_state(m, building, anim, slot)
            assert ai_after['anim']['alive'] and not ai_after['anim']['runtime']['inactive']
        finally:
            performance.close()
        assert code_before == bytes(u.mem_read(0x401000, 0x3E0000))
        assert all(raw == bytes(u.mem_read(p, len(raw))) for p, raw in vtables.items())
        return dict(schema=1, physical_inputs=metadata,
            normal_camera=list(struct.unpack('<2i', old_camera)),
            type_layers=scene['layer_reads'], slot_input=slot_input,
            anim_type_reader=reader, anim_after_slot_creation=constructed,
            original_slot_calls=setup_calls, presentation_sinks=setup_sinks,
            setup_rng_calls=setup_rng, slot_construction_effects=construction_effects,
            performance_setup=performance_setup, rows=rows,
            post_admission_ai=dict(before=ai_before, after=ai_after),
            geometry_startup=geometry, tactical_matrix_startup=matrix,
            anim_registry_startup=registry,
            object_retirement_registry_startup=retirement_registry,
            building_dependencies=scene['building_dependencies'],
            bound_building_body_names=body_names,
            color_scheme_setup=scene['color_scheme'],
            reached=[f'{pc:08X}' for pc in sorted(reached)],
            checked_text_and_vtables_unchanged=True,
            checked_ranges=dict(text_start='00401000', text_bytes=0x3E0000,
                vtables=[dict(address=f'{p:08X}', bytes=len(raw)) for p, raw in vtables.items()]),
            boundary='One physical GAPOWR_A created by original451890 in the existing admitted flat-map Building/House fixture. Original Logic counter/Throttle produces every tested FPS bucket; only OS milliseconds and process Options0/1/2 are supplied. Whole422C70 retains native viewport, DrawIt and55AF60 ordering. No forced hidden/image/ART/FPS result/frame fields, no full Logic/Game/Options/Display traversal. One separate supplied offscreen camera tests the earlier projection boundary. Pixel output is not golden here: actual Surface/selector/leaf calls and lifecycle observations establish admission. Base palette format priors are inherited unchanged; pixel output and Convert-mask parity are excluded.')
    except Exception as error:
        error.native_admission_failure = dict(phase=phase, fault=failure,
            exception=repr(error), recent=list(recent),
            original_slot_calls=setup_calls, presentation_sinks=setup_sinks[-20:],
            performance_setup=performance_setup,
            performance_failure=getattr(error, 'native_performance_failure', None),
            draw_failure=getattr(error, 'native_draw_failure', None),
            completed_rows=locals().get('rows', []),
            reached=[f'{pc:08X}' for pc in sorted(reached)])
        raise
    finally:
        for hook in hooks:
            u.hook_del(hook)



class PerformanceDriver:
    """Original counter/Throttle owner; no extra hysteresis query before DrawIt."""
    def __init__(self, m):
        from unicorn import UC_HOOK_CODE
        from unicorn.x86_const import UC_X86_REG_ESP
        from tools.spatial_oracle.building_body_rules import dwords
        self.m, self.u = m, m.u
        self.now, self.reads, self.history, self.registrations = 0, [], [], []
        self.recent = deque(maxlen=48)
        self.old_import = bytes(m.u.mem_read(0x7E1530, 4))
        self.time_import = m.alloc(16)
        self.code = bytes(m.u.mem_read(0x401000, 0x3E0000))
        self.receipt = dict(before=performance_state(m), history=self.history,
            exit_registrations=self.registrations,
            boundary='Empty counter/timer/latch inputs installed once; minimum15/buffer5 use original setters. Each next960ms bucket runs the requested number of real55AFB0..55AFD9 counter prefixes then whole55E33B tail. timeGetTime and caller-checked CRT atexit are platform transports. No extra55AF60 query runs here: only whole Anim DrawIt consumes its retained latch.')
        def observe(_u, pc, _size, _data):
            self.recent.append(f'{pc:08X}')
            if pc == self.time_import:
                self.reads.append(self.now)
                m.ret(self.now)
            elif pc == 0x7C978A:
                sp = m.u.reg_read(UC_X86_REG_ESP)
                caller = m.read32(sp)
                assert caller == 0x55E36F, ('Unexpected atexit caller', hex(caller))
                self.registrations.append(dict(entry='007C978A', caller=f'{caller:08X}',
                    destructor=f'{m.read32(sp+4):08X}', supplied_platform_result=0))
                m.ret(0)
        self.hook = m.u.hook_add(UC_HOOK_CODE, observe)
        try:
            m.u.mem_write(0x7E1530, dwords(self.time_import))
            m.u.mem_write(0xA8EB78, dwords(2))
            m.u.mem_write(0xABCD40, dwords(0, 0, 0, 0))
            m.u.mem_write(0xABCD50, b'\0')
            m.u.mem_write(0xABCD88, dwords(0, 0, 0))
            m.u.mem_write(0xABCD94, b'\0')
            m.invoke(0x55AF40, 15)
            m.invoke(0x55AF50, 5)
            self.receipt['supplied_priors'] = performance_state(m)
        except Exception:
            self.close()
            raise

    def advance(self, milliseconds, visits):
        from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EBP, UC_X86_REG_EBX
        from tools import native_oracle as native
        from tools.spatial_oracle.building_body_rules import SP, dwords
        m, u = self.m, self.u
        self.now = milliseconds
        row = dict(milliseconds=milliseconds, logic_visits=visits,
            before=performance_state(m))
        mark = len(self.reads)
        try:
            for _ in range(visits):
                u.reg_write(UC_X86_REG_ESP, SP)
                native.run_checked(u, 0x55AFB0, 0x55AFD9, count=30)
            u.mem_write(SP, bytes(40) + dwords(native.RET_MAGIC))
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_EBP, 0)
            u.reg_write(UC_X86_REG_EBX, 0xFFFFFFFF)
            native.run_checked(u, 0x55E33B, native.RET_MAGIC, count=3000)
            assert u.reg_read(UC_X86_REG_ESP) == SP + 44
            row.update(clock_reads=self.reads[mark:], after=performance_state(m))
            self.history.append(row)
            assert self.code == bytes(u.mem_read(0x401000, 0x3E0000))
        except Exception as error:
            error.native_performance_failure = dict(input=row, recent=list(self.recent),
                current=performance_state(m), clock_reads=self.reads[mark:])
            raise

    def close(self):
        self.u.mem_write(0x7E1530, self.old_import)
        self.u.hook_del(self.hook)
        self.receipt['after'] = performance_state(self.m)
        self.receipt['checked_text_unchanged'] = self.code == bytes(self.u.mem_read(0x401000, 0x3E0000))
        assert self.receipt['checked_text_unchanged']


def draw_admission(scene, anim, idle_image, name):
    from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
    from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_ECX, UC_X86_REG_EAX
    from tools.procedural_drawing_oracle import shroud
    from tools.projectile_oracle.bridge_render_shape import prepare_surface, PIXELS, Z, A, SURFACE
    from tools.spatial_oracle.building_body_rules import dwords
    m, u, clip = scene['m'], scene['u'], scene['clip']
    size, length = [160, 160], 51200
    prepare_surface(u, dict(depth_baseline=32768, old_z=65535,
        a_value=127, background=0xBDF7), size=(160, 160))
    u.mem_write(SURFACE, dwords(scene['vtable']))
    before = performance_state(m)
    calls, fps_calls, pending_fps, pending_calls = [], [], [], []
    ordered_gates, received, rng = [], [], []
    selected_vtables, pending_selectors, entries = {}, [], set()
    stores = dict(color=0, z=0, a=0)
    recent = deque(maxlen=40)
    stages = {
        0x422C70:'Anim.DrawIfVisible.delay_check', 0x422C7C:'Anim.delay_return',
        0x5F4B10:'Object.DrawIfVisible', 0x6D2140:'Tactical.projection',
        0x5F4B7F:'Object.projection_or_display_return',
        0x422CA0:'Anim.DrawIt', 0x422FCC:'Anim.fps_query',
        0x422FD1:'Anim.fps_compare', 0x422FDF:'Anim.low_fps_type_detail',
        0x422FEC:'Anim.hidden_19d', 0x423000:'Anim.options_detail',
        0x42300E:'Anim.options_branch', 0x423014:'Anim.special_hidden_199',
        0x42301E:'Anim.should_fog_remove_374', 0x423030:'Anim.image_virtual',
        0x423033:'Anim.image_null_check', 0x4238A3:'Anim.return',
        0x4AED70:'Surface.DrawShape',
    }
    def observe(_u, pc, _size, _data):
        recent.append(f'{pc:08X}')
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc in stages:
            ordered_gates.append(dict(pc=f'{pc:08X}', stage=stages[pc]))
        if pending_fps and (pc, sp) == pending_fps[-1][:2]:
            _, _, row = pending_fps.pop()
            row.update(returned=True, threshold=u.reg_read(UC_X86_REG_EAX), after=performance_state(m))
        if pc == 0x55AF60:
            row = dict(entry='0055AF60', caller=f'{m.read32(sp):08X}', before=performance_state(m))
            fps_calls.append(row)
            pending_fps.append((m.read32(sp), sp+4, row))
        if pending_calls and (pc, sp) == pending_calls[-1][:2]:
            _, _, row = pending_calls.pop()
            row['returned'] = True
        if pending_selectors and (pc, sp) == pending_selectors[-1][:2]:
            _, _, row = pending_selectors.pop()
            pixel = u.reg_read(UC_X86_REG_EAX)
            row['pixel_object'] = f'{pixel:08X}'
            if pixel:
                vt = m.read32(pixel)
                selected_vtables.setdefault(vt, bytes(u.mem_read(vt, 12)))
                entries.update(m.read32(vt+i*4) for i in range(3))
                row['vtable'] = f'{vt:08X}'
        if pc in (0x65C780, 0x65C7E0):
            rng.append(f'{pc:08X}')
        if pc == 0x422CA0:
            received.append(dict(caller=f'{m.read32(sp):08X}',
                point=shroud.rally.ints(u, m.read32(sp+4), 2),
                clip=shroud.rally.ints(u, m.read32(sp+8), 4)))
        if pc == 0x4AED70:
            args = shroud.rally.ints(u, sp+4, 14)
            row = dict(caller=f'{m.read32(sp):08X}', frame=args[1],
                source_is_physical_idle=args[0] == idle_image,
                point=shroud.rally.ints(u, args[2], 2),
                clip=shroud.rally.ints(u, args[3], 4), flags=args[4],
                z_adjust=args[6], gradient=args[7], intensity=args[8], color=args[9],
                selectors=[], pixel_leaves=[])
            calls.append(row)
            pending_calls.append((m.read32(sp), sp+60, row))
        if pc in (0x490B90, 0x490E50):
            assert pending_calls
            pointer = u.reg_read(UC_X86_REG_ECX)
            assert m.read32(sp+4) & 6 == 0, 'Admission-only physical control must remain opaque'
            row = dict(entry=f'{pc:08X}', flags=m.read32(sp+4),
                converter_identity=converter_identity(m, pointer))
            pending_calls[-1][2]['selectors'].append(row)
            pending_selectors.append((m.read32(sp), sp+8, row))
        if pc in entries and pending_calls:
            leaves = pending_calls[-1][2]['pixel_leaves']
            if f'{pc:08X}' not in leaves:
                leaves.append(f'{pc:08X}')
    def written(_u, _access, address, count, value, _data):
        for key, begin in (('color', PIXELS), ('z', Z), ('a', A)):
            if begin <= address < begin+length:
                stores[key] += 1
                break
    hooks = [u.hook_add(UC_HOOK_CODE, observe), u.hook_add(UC_HOOK_MEM_WRITE, written)]
    try:
        u.mem_write(clip, dwords(0, 0, 160, 160))
        returned = m.invoke(0x422C70, anim, (clip, 1, 0),
            instruction_limit=12000000, timeout_us=120000000)
    except Exception as error:
        error.native_draw_failure = dict(name=name, before=before,
            current=performance_state(m), gates=ordered_gates, fps_calls=fps_calls,
            calls=calls, recent=list(recent))
        raise
    finally:
        for hook in hooks:
            u.hook_del(hook)
    assert not pending_fps and not pending_calls and not pending_selectors
    assert all(row.get('returned') for row in fps_calls)
    assert len(calls) <= 1
    assert all(row.get('returned') and row['source_is_physical_idle'] for row in calls)
    assert not rng
    assert all(raw == bytes(u.mem_read(p, len(raw))) for p, raw in selected_vtables.items())
    return dict(name=name, entry='00422C70', returned_al=returned & 255,
        observed_surface_call_count=len(calls), calls=calls,
        performance_before=before, performance_after=performance_state(m),
        original_fps_gate_calls=fps_calls, ordered_gate_entries=ordered_gates,
        received_drawit=received, rng_entries=rng,
        display_priors=dict(hidden_19d=u.mem_read(anim+0x19D,1)[0],
            special_hidden_199=u.mem_read(anim+0x199,1)[0],
            delay_remaining=m.read32(anim+0x184),
            type_image=f'{m.read32(m.read32(anim+0xC8)+0xA4):08X}'),
        native_store_counts=stores,
        checked_pixel_vtables_unchanged=True, size=size,
        underlay=dict(background_rgb565=0xBDF7, z=65535, a=127))



def _public_observations(value):
    if isinstance(value, dict):
        return {key:_public_observations(item) for key,item in value.items()
                if key != 'header8_hex'}
    if isinstance(value, list):
        return [_public_observations(item) for item in value]
    return value


def _lifecycle(state):
    """Selected measured lifecycle fields; no computed visibility predicate."""
    anim = state['anim']
    return dict(identity_cursor=state['identity_cursor'],
        scenario_rng_sha256=state['scenario_rng_sha256'], building=state['building'],
        anim={key:anim[key] for key in (
            'pointer','type_name','native_id','alive','limbo','marked','draw_flags',
            'z_adjust','runtime','stored_xyz','is_building_anim',
            'forced_translucency','temporal_paused','temporal_resume_pending')})


def generate():
    from tools.spatial_oracle.building_anim_detail_setup import physical_inputs
    assets, physical = physical_inputs(configured_gamemd().parent)
    observed = execute(assets, physical)
    assert len(observed['rows']) == 16
    rows = []
    for row in observed['rows']:
        compact = dict(row)
        for key in ('state_before','state_after'):
            compact[key] = _lifecycle(row[key])
        for call in compact['calls']:
            # Preserve actual calls/selection, omit unrelated color tables.
            call['selectors'] = [{key:value for key,value in selector.items()
                                  if key != 'converter_identity'}
                                 for selector in call['selectors']]
        rows.append(compact)
    return _public_observations(dict(schema=1,
        input=dict(type='GAPOWR', animation='GAPOWR_A', slot=3,
            normal_camera=observed['normal_camera'],
            original_type_detail=observed['anim_type_reader']['native_detail_level'],
            reset_performance_once=True, retain_latch_between_rows=True,
            initial_minimum=15, initial_buffer=5,
            scope='Whole original DrawIfVisible admission; no pixel comparison'),
        physical_inputs=observed['physical_inputs'], type_layers=observed['type_layers'],
        slot_input=observed['slot_input'],
        anim_type_reader=observed['anim_type_reader'],
        slot_construction_effects=observed['slot_construction_effects'],
        original_slot_calls=observed['original_slot_calls'],
        presentation_sinks=observed['presentation_sinks'],
        performance_setup=observed['performance_setup'], rows=rows,
        post_admission_ai={key:_lifecycle(value)
            for key,value in observed['post_admission_ai'].items()},
        startup=dict(geometry=observed['geometry_startup'],
            tactical_matrix=observed['tactical_matrix_startup'],
            anim_registry=observed['anim_registry_startup'],
            object_retirement_registry=observed['object_retirement_registry_startup'],
            building_dependencies=observed['building_dependencies']),
        checked_text_and_vtables_unchanged=observed['checked_text_and_vtables_unchanged'],
        checked_ranges=observed['checked_ranges'], boundary=observed['boundary']))


def metadata():
    return provenance(
        scope='Physical GAPOWR_A attached-Anim whole DrawIfVisible admission, original sampled Logic FPS and retained minimum-rate hysteresis, three Options detail priors and an offscreen control.',
        assumptions=[
            'One actual451890 slot at constructor-selected frame0 is retained across16 draws. Physical GAPOWR and GAPOWR_A readers and real Country/House/Building/Anim constructors run. Stock DetailLevel2, noShadow, image, frame, coordinates and light are not changed for the performance controls.',
            'Supplied component priors are the admitted alive/placed/Guard Building, default Country, human/current-player/LightGold House, sparse flat/revealed Cells, ordinary camera(-384,380), clock1000 and game speed4. Complete Scenario/Options UI/Unlimbo/GrandOpening/Display traversal is excluded.',
            'Empty performance counters/timer/latch are installed once. Original55AF40/55AF50 set minimum15/buffer5. Each published FPS bucket runs original55AFB0..55AFD9 counter prefixes and55E33B Throttle tail at960ms steps. No FPS or latch writes follow startup and no extra55AF60 query precedes DrawIt. Options0/1/2 are explicitly supplied process priors, not ART changes.',
            'Whole422C70 calls original Object5F4B10 and DrawIt422CA0. Actual55AF60 return/latch, ordered gate PCs, Surface calls and before/after lifecycle/identity/RNG are retained. Offscreen/restored cameras are declared earlier-projection controls, including a fresh low-FPS bucket before either draw queries its retained latch. One original423AC0 AI visit follows the draw sequence.',
            'The existing base TMP/palette setup and its supplied RGB565 format priors are retained unchanged. Full original opaque/noShadow Surface/selector/raster calls may execute, but color words, blend formulas and Convert-mask parity are not assertions of this corpus. No dependency on the separate translucency-mask helper changes is needed.',
            'Only pristine missing listener-vector headers execute their original CRT prefixes before Anim construction. Established live headers remain unchanged. Original text and listed class/observed pixel vtable ranges are checked unchanged; this is not an assertion about all global data.',
        ],
        substitutions=[
            'Cached lexical INI and immutable named-file transports supply physical strings/bytes. Original INI parsing and native archive search are outside the boundary. Existing heap/delete/TLS/GUID/Interlocked/platform transports and spare registry storage are reused.',
            'During performance steps only timeGetTime supplies declared milliseconds; first-timer CRT atexit7C978A receives platform success only from caller55E36F. Original6C8C40 shift, timer/counter/Throttle and55AF60 policy execute. Process-exit callback and full Logic/Main_Tick are excluded.',
            'Mark5F5850 and Display registration4A9720 return1 as recorded presentation sinks. No native FPS query, hidden/detail/image gate, coordinate/frame, lifecycle, Surface or pixel return is substituted.',
        ],
        entry_points=dict(building_type=0x45DD90, building_reader=0x45FE50,
            country=0x5113F0, house=0x4F54A0, building=0x43B740,
            anim_type_reader=0x427D00, slot=0x451890, anim=0x421EA0,
            ai=0x423AC0, draw_if_visible=0x422C70, object_draw=0x5F4B10,
            tactical_projection=0x6D2140, draw_it=0x422CA0,
            logic_counter=0x55AFB0, throttle_tail=0x55E33B,
            minimum_query=0x55AF60, shape=0x4AED70))


def source_paths():
    """Guard the selected native setup/transport owners, not Rust consumers."""
    root = Path(__file__).resolve().parents[2]
    names = (
        'tools/spatial_oracle/building_anim_detail.py',
        'tools/spatial_oracle/building_anim_detail_setup.py',
        'tools/native_oracle.py',
        'tools/oracle_sources.py',
        'tools/sidebar_oracle/stock.py',
        'tools/projectile_oracle/bridge_render_inputs.py',
        'tools/projectile_oracle/bridge_render_inputs_palette.py',
        'tools/projectile_oracle/bridge_render_shape.py',
        'tools/projectile_oracle/flat_art.py',
        'tools/rules_oracle/bridge_anim_inputs.py',
        'tools/rules_oracle/bridge_anim_lists.py',
        'tools/rules_oracle/bridge_child_sound.py',
        'tools/rules_oracle/weapon_laser.py',
        'tools/rules_oracle/theater_general_reader.py',
        'tools/procedural_drawing_oracle/shroud.py',
        'tools/procedural_drawing_oracle/rally.py',
        'tools/procedural_drawing_oracle/house_color.py',
        'tools/spatial_oracle/anim_bouncer_launch.py',
        'tools/spatial_oracle/building_prism.py',
        'tools/spatial_oracle/tmp_clear_render.py',
        'tools/spatial_oracle/bridge_constructor.py',
        'tools/spatial_oracle/building_body_rules.py',
        'tools/spatial_oracle/trigger_action_values.py',
        'tools/spatial_oracle/bridge_shadow_render.py',
        'tools/spatial_oracle/bridge_recalc_presentation.py',
        'tools/spatial_oracle/bridge_damage_admission.py',
        'tools/spatial_oracle/crate_speed_effect.py',
        'tools/spatial_oracle/engineer_repair_admission.py',
        'tools/spatial_oracle/infantry_deploy_action.py',
        'tools/spatial_oracle/map_queries.py',
        'tools/spatial_oracle/terrain_recalc.py',
        'tools/spatial_oracle/reveal_area.py',
        'tools/bridge_click_oracle.py',
        'tools/rmg_oracle/gen_rng_vectors.py',
        'tools/palette_oracle/oracle.py',
    )
    return {name: root / name for name in names}


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'),
                   provenance=metadata, source_paths=source_paths())
