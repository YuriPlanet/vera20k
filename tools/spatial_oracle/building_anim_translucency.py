"""Original populated Building-slot notice, release and RGB565 translucency.

Imports and --help perform no native execution. See building_anim_translucency.md
for physical inputs, supplied lifecycle boundaries and full-plane comparisons.
"""
from collections import deque
import hashlib
import json
from pathlib import Path
import struct

from tools.native_oracle import configured_gamemd, finish_vectors, provenance


def sha(raw):
    """Digest an observed runtime buffer, never an executable or source file."""
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


def prepare_performance(m):
    """Original Logic counter and Throttle tail; only OS time is supplied.

    Same instruction boundary/stack as building_prism.laser_fps. The timer,
    counters and thresholds are explicit component priors, not measured
    whole-game performance. FPS60 is produced by 60 original counter visits
    and the actual due throttle bucket, not a supplied DrawIt/threshold reply.
    """
    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EBP, UC_X86_REG_EBX
    from tools import native_oracle as native
    from tools.spatial_oracle.building_body_rules import SP, dwords
    u = m.u
    before = performance_state(m)
    code = bytes(u.mem_read(0x401000, 0x3E0000))
    old_import = bytes(u.mem_read(0x7E1530, 4))
    time_import = m.alloc(16)
    current_ms, reads, reached = 0, [], set()
    exit_registrations, history = [], []
    recent = deque(maxlen=48)
    def observe(_u, pc, _size, _data):
        recent.append(f'{pc:08X}')
        if pc in (0x55AFB0, 0x55E33B, 0x6C8C40, 0x55AF40, 0x55AF50, 0x55AF60):
            reached.add(pc)
        if pc == time_import:
            reads.append(current_ms)
            m.ret(current_ms)
        elif pc == 0x7C978A:
            sp = u.reg_read(UC_X86_REG_ESP)
            caller = m.read32(sp)
            assert caller == 0x55E36F, ('Unexpected process-exit registration', hex(caller))
            exit_registrations.append(dict(entry='007C978A', caller=f'{caller:08X}',
                destructor=f'{m.read32(sp + 4):08X}', supplied_platform_result=0,
                boundary='Successful CRT atexit registration only; process-exit callback is not executed in this bounded fixture.'))
            m.ret(0)
    hook = u.hook_add(UC_HOOK_CODE, observe)
    try:
        u.mem_write(0x7E1530, dwords(time_import))
        u.mem_write(0xA8EB78, dwords(2))
        u.mem_write(0xABCD40, dwords(0, 0, 0, 0))
        u.mem_write(0xABCD50, b'\0')
        u.mem_write(0xABCD88, dwords(0, 0, 0))
        u.mem_write(0xABCD94, b'\0')
        # Explicit normal-detail policy, through original setters. Full
        # Rules/Options startup is outside this performance composition.
        m.invoke(0x55AF40, 15)
        m.invoke(0x55AF50, 5)
        supplied_priors = performance_state(m)
        history = []
        for current_ms, visits in ((0, 0), (960, 60)):
            step_before = performance_state(m)
            mark = len(reads)
            for _ in range(visits):
                u.reg_write(UC_X86_REG_ESP, SP)
                native.run_checked(u, 0x55AFB0, 0x55AFD9, count=30)
            # Interior Throttle frame: POP EDI/ESI/EBP/EBX, ADD ESP18, RET.
            u.mem_write(SP, bytes(40) + dwords(native.RET_MAGIC))
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_EBP, 0)
            u.reg_write(UC_X86_REG_EBX, 0xFFFFFFFF)
            native.run_checked(u, 0x55E33B, native.RET_MAGIC, count=3000)
            assert u.reg_read(UC_X86_REG_ESP) == SP + 44
            threshold = m.invoke(0x55AF60, 0)
            history.append(dict(milliseconds=current_ms, logic_visits=visits,
                clock_reads=reads[mark:], before=step_before,
                returned_minimum=threshold, after=performance_state(m)))
        after = performance_state(m)
        assert after['current_fps'] == 60 and after['options_detail'] == 2
        assert not after['reduced'] and after['minimum'] == 15 and after['buffer'] == 5
        assert {0x55AFB0, 0x55E33B, 0x6C8C40, 0x55AF40, 0x55AF50, 0x55AF60} <= reached
        assert code == bytes(u.mem_read(0x401000, 0x3E0000))
        return dict(before=before, supplied_priors=supplied_priors, history=history,
            after=after, reached=[f'{pc:08X}' for pc in sorted(reached)],
            checked_text_unchanged=True, exit_registrations=exit_registrations,
            reused_owner='tools/spatial_oracle/building_prism.py::laser_fps',
            boundary='Supplied empty performance counters/timer, normal minimum15/buffer5 and Options detail2; original policy setters, 60 Logic counter-prefix visits, Throttle tail and GetMinFrameRate execute. Only timeGetTime import answers0/960 milliseconds and the first-timer CRT atexit registration7C978A from55E36F returns success; native6C8C40 SHR4 and the full Throttle timing logic remain live. No full Logic/Main_Tick/Options startup, real wall-clock speed, patched type detail or substituted admission result.')
    except Exception as error:
        error.native_performance_failure = dict(phase='original_performance_setup',
            exception=repr(error), before=before, current=performance_state(m),
            supplied_priors=locals().get('supplied_priors'), history=history,
            clock_reads=reads, exit_registrations=exit_registrations,
            reached=[f'{pc:08X}' for pc in sorted(reached)], recent=list(recent),
            current_ms=current_ms,
            checked_text_unchanged=code == bytes(u.mem_read(0x401000, 0x3E0000)))
        raise
    finally:
        u.mem_write(0x7E1530, old_import)
        u.hook_del(hook)


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


def pixel_runs(points):
    result = []
    for y in sorted({y for x, y in points}):
        xs = sorted(x for x, row in points if row == y)
        start = last = xs[0]
        for x in xs[1:]:
            if x != last + 1:
                result.append([y, start, last + 1])
                start = x
            last = x
        result.append([y, start, last + 1])
    return result


def summarize_stores(stores):
    # Complete coordinate coverage plus a digest of ordered raw stores. Values
    # and final color/depth planes remain in each phase's after-image receipt.
    result = {}
    for plane in ('color', 'z', 'a'):
        selected = [s for s in stores if s[0] == plane]
        points = set()
        for _, offset, count, _value in selected:
            for word in range(offset // 2, (offset + count + 1) // 2):
                points.add((word % 160, word // 160))
        result[plane] = dict(store_count=len(selected), distinct_pixels=len(points),
            bounds_inclusive=[min(x for x, y in points), min(y for x, y in points),
                              max(x for x, y in points), max(y for x, y in points)] if points else None,
            pixel_runs_y_x_begin_x_end=pixel_runs(points),
            ordered_store_sha256=sha(json.dumps(selected, separators=(',', ':')).encode()))
    return result


def execute_case(assets, metadata, case):
    from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE, UC_HOOK_MEM_INVALID
    from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX,
        UC_X86_REG_EDX, UC_X86_REG_ESI, UC_X86_REG_EIP, UC_X86_REG_ESP)
    from tools import native_oracle as native
    from tools.procedural_drawing_oracle import shroud
    from tools.projectile_oracle.bridge_render_shape import prepare_surface, PIXELS, Z, A, SURFACE
    from tools.spatial_oracle.anim_bouncer_launch import constructor_state
    from tools.spatial_oracle.building_body_rules import INI, SP, dwords
    from tools.spatial_oracle.trigger_action_values import initialize_discovery_geometry

    from tools.spatial_oracle import building_anim_translucency_setup as helper
    scene = helper.prepare_power_plant_scene(shroud, assets, metadata)
    m, u = scene['m'], scene['u']
    assert m.format_mask_startup['half'] == 0x7BEF
    assert m.format_mask_startup['quarter'] == 0x39E7
    assert m.format_mask_startup['original_text_unchanged']
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
                context=dict(component='building_slot_translucency',
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
    performance_setup = prepare_performance(m)
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
        history, rows, transactions = [], [], []
        frame = 1000

        def snapshot():
            return slot_state(m, building, anim, slot)

        def ai_visits(count):
            nonlocal frame
            for _ in range(count):
                frame += 1
                u.mem_write(0xA8ED84, dwords(frame))
                before = snapshot()
                m.invoke(0x423AC0, anim)
                after = snapshot()
                history.append(dict(phase=phase, frame=frame, before=before, after=after))
                assert after['anim']['alive'] and not after['anim']['runtime']['inactive'], after

        def draw(label):
            before = snapshot()
            row = draw_slot(scene, anim, idle_image, label, case)
            row.update(clock=frame, state_before=before, state_after=snapshot())
            rows.append(row)

        def erase():
            nonlocal temporal
            before = snapshot()
            assert m.read32(building + 0x278) == 0
            temporal = m.alloc(0x50)
            u.mem_write(temporal, bytes(0x50))
            u.mem_write(temporal + 0x28, dwords(building))
            u.mem_write(building + 0x278, dwords(temporal))
            linked = snapshot()
            # Existing factory_loss declared lone-head boundary. The original
            # notice writes +270 before calling the populated Building owner.
            # No native decision, type virtual, pause or House store is stubbed.
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_ESI, temporal)
            u.reg_write(UC_X86_REG_ECX, building)
            native.run_checked(u, 0x71B0EA, 0x71B14E, count=2000000,
                required_addresses=(0x4521C0,))
            assert u.reg_read(UC_X86_REG_ESP) == SP
            after = snapshot()
            transactions.append(dict(op='supplied_lone_head_then_original_notice',
                entry='0071B0EA', stop_before='0071B14E', before=before,
                after_linked_input=linked, after=after))
            assert after['building']['warped'] and not after['building']['online']
            assert after['anim']['forced_translucency'] and after['anim']['temporal_paused']

        def release():
            before = snapshot()
            # Full LetGo on the declared lone-head receiver. This clears +270
            # and the head pointer through original stores, then calls452210.
            m.invoke(0x71ABC0, temporal)
            after = snapshot()
            transactions.append(dict(op='original_let_go', entry='0071ABC0',
                                     before=before, after=after))
            assert not after['building']['warped'] and after['building']['online']
            assert not after['anim']['forced_translucency'] and not after['anim']['temporal_paused']
            assert m.read32(temporal + 0x28) == m.read32(building + 0x278) == 0

        temporal = None
        # Every retained stage is reached through original AI. No animation
        # value, timer, rate, forced flag or renderer selector is written.
        # The baseline/notice/release triplet shares the same genuine frame.
        for target_frame in range(8):
            phase = f'existing_ai_to_stage{target_frame}'
            start_clock = frame
            while m.read32(anim + 0xAC) != target_frame:
                assert frame - start_clock < 256, ('Unreached physical loop frame', target_frame, snapshot())
                ai_visits(1)
            draw(f'existing_stage{target_frame}_baseline')
            phase = f'existing_stage{target_frame}_erase'
            erase()
            paused_frame = m.read32(anim + 0xAC)
            ai_visits(4)
            assert m.read32(anim + 0xAC) == paused_frame
            draw(f'existing_stage{target_frame}_erased_after_ai')
            phase = f'existing_stage{target_frame}_release'
            release()
            draw(f'existing_stage{target_frame}_released_before_ai')
            start_clock = frame
            while m.read32(anim + 0xAC) == paused_frame:
                assert frame - start_clock < 256
                ai_visits(1)
            transactions.append(dict(op='existing_resumed_ai', sampled_frame=target_frame,
                                     after=snapshot()))

        phase = 'new_slot_during_erase'
        erase()
        old_anim, before_replace = anim, snapshot()
        # The physical slot is reallocated through its original owner while
        # erasure is active. Original451890 performs new-before-old-destruction
        # replacement and copies its retained frame itself.
        m.invoke(0x451890, building, (record, slot, 0, 0, 0))
        anim = m.read32(building + 0x55C + slot * 4)
        assert anim and anim != old_anim
        assert m.read32(anim) == 0x7E3354 and m.read32(anim + 0xC8) == anim_type
        after_replace = snapshot()
        transactions.append(dict(op='original_allocate_during_erase', entry='00451890',
            old_pointer=f'{old_anim:08X}', before=before_replace, after=after_replace))
        assert after_replace['building']['warped']
        assert after_replace['anim']['temporal_paused']
        assert not after_replace['anim']['forced_translucency']
        ai_visits(4)
        draw('new_during_erase_after_ai')
        phase = 'new_slot_release'
        release()
        draw('new_released_before_ai')
        new_frame = m.read32(anim + 0xAC)
        start_clock = frame
        while m.read32(anim + 0xAC) == new_frame:
            assert frame - start_clock < 256
            ai_visits(1)
        transactions.append(dict(op='new_resumed_ai', after=snapshot()))
        required = {0x451890, 0x6D2360, 0x459EF0, 0x421EA0, 0x424CE0,
                    0x423AC0, 0x422C70, 0x5F4B10, 0x41BE00, 0x422BE0,
                    0x6D2140, 0x422CA0, 0x4AED70, 0x71B0EA, 0x4521C0,
                    0x71ABC0, 0x452210, 0x70C5B0, 0x451B25}
        assert required <= reached, sorted(f'{x:08X}' for x in required - reached)
        assert code_before == bytes(u.mem_read(0x401000, 0x3E0000))
        assert all(raw == bytes(u.mem_read(p, len(raw))) for p, raw in vtables.items())
        return dict(schema=1, input=case,
            physical_inputs=metadata, type_layers=scene['layer_reads'], slot_input=slot_input,
            anim_type_reader=reader, anim_after_slot_creation=constructed,
            original_slot_calls=setup_calls, presentation_sinks=setup_sinks,
            setup_rng_calls=setup_rng, slot_construction_effects=construction_effects,
            performance_setup=performance_setup,
            geometry_startup=geometry, tactical_matrix_startup=matrix, anim_registry_startup=registry,
            object_retirement_registry_startup=retirement_registry,
            building_dependencies=scene['building_dependencies'], bound_building_body_names=body_names,
            color_scheme_setup=scene['color_scheme'], inherited_terrain=scene['terrain'],
            inherited_resources=scene['sources'], cell_ground_light=[struct.unpack('<h', u.mem_read(cell + 0x10A, 2))[0] for cell in scene['cells']],
            frame_history=history, transactions=transactions, rows=rows,
            format_mask_startup=m.format_mask_startup, reached=[f'{pc:08X}' for pc in sorted(reached)],
            checked_text_and_vtables_unchanged=True,
            checked_ranges=dict(text_start='00401000', text_bytes=0x3E0000,
                vtables=[dict(address=f'{p:08X}', bytes=len(raw)) for p, raw in vtables.items()]),
            boundary='Physical GAPOWR/GAPOWR_A readers, actual Country/House/Building constructors with reader-derived Enslaves=NULL and no slave manager, supplied admitted Building placement, original451890 slot creation and whole Anim DrawIfVisible. Sparse flat/revealed Cells and uniform160x160 RGB565/A/Z input; no terrain or Building-body raster is included. Original cropped71B0EA..71B14E notice starts from a declared already-linked lone Temporal head and calls populated4521C0. Whole71ABC0 LetGo calls452210 and clears parent warp/head state. Existing and newly-reallocated-during-erase slots use original lifecycle/AI; no slot coordinates/frames/forced flags or pixels supplied. Physical nonFlat GAPOWR_A has Shadow=false, so its one whole draw retains full color/Z/A planes. Eight actual AI loop frames each receive a bounded notice/LetGo cycle. Original Logic counter/Throttle tail produces the declared FPS60 prestate with Options detail2; actual55AF60 and every draw admission/latch remain observed. Palette pointers are observed against original LightGold and global converters; no palette identity is inferred from shade count. No full CLEG firing/admission, Temporal ctor/AI, full map/GrandOpening, Display traversal, production screenshot or full-match claim.')
    except Exception as error:
        error.native_slot_failure = dict(input=case, phase=phase, fault=failure, recent=list(recent),
            original_slot_calls=setup_calls, presentation_sinks=setup_sinks[-20:],
            performance_setup=performance_setup, last_draw_failure=getattr(error, 'native_draw_failure', None),
            transactions=locals().get('transactions', []), completed_draws=locals().get('rows', []),
            object_retirement_registry_startup=retirement_registry,
            reached=[f'{pc:08X}' for pc in sorted(reached)])
        raise
    finally:
        for hook in hooks:
            u.hook_del(hook)


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


def word_runs(raw):
    rows = []
    for (value,) in struct.iter_unpack('<H', raw):
        if rows and rows[-1][1] == value:
            rows[-1][0] += 1
        else:
            rows.append([1, value])
    assert b''.join(struct.pack('<H', value) * count for count, value in rows) == raw
    return rows


def draw_slot(scene, anim, idle_image, label, case):
    from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
    from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_ECX, UC_X86_REG_EAX
    from tools.procedural_drawing_oracle import shroud
    from tools.projectile_oracle.bridge_render_shape import prepare_surface, PIXELS, Z, A, SURFACE
    from tools.spatial_oracle.building_body_rules import dwords
    m, u, clip = scene['m'], scene['u'], scene['clip']
    length = 160 * 160 * 2
    prepare_surface(u, dict(depth_baseline=32768, old_z=case['initial_z'],
                           a_value=case['a_value'], background=case['background']), size=(160, 160))
    u.mem_write(SURFACE, dwords(scene['vtable']))

    def planes():
        return {key: bytes(u.mem_read(ptr, length)) for key, ptr in
                (('color', PIXELS), ('z', Z), ('a', A))}

    def encoded(raw):
        return {key + '_words_rle': word_runs(value) for key, value in raw.items()}

    before = planes()
    performance_before = performance_state(m)
    fps_calls, pending_fps = [], []
    draws, pending, stores, rng, received = [], [], [], [], []
    selected_entries, selected_vtables, pending_selectors = set(), {}, []

    def observe(_u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if pending_fps and pc == pending_fps[-1][0] and sp == pending_fps[-1][1]:
            _, _, fps = pending_fps.pop()
            fps.update(returned=True, threshold=u.reg_read(UC_X86_REG_EAX), after=performance_state(m))
        if pc == 0x55AF60:
            fps = dict(entry='0055AF60', caller=f'{m.read32(sp):08X}', before=performance_state(m))
            fps_calls.append(fps)
            pending_fps.append((m.read32(sp), sp + 4, fps))
        if pending_selectors and pc == pending_selectors[-1][0] and sp == pending_selectors[-1][1]:
            _, _, row = pending_selectors.pop()
            blitter = u.reg_read(UC_X86_REG_EAX)
            row['returned_blitter'] = f'{blitter:08X}'
            if blitter:
                vt = m.read32(blitter)
                row['vtable'] = f'{vt:08X}'
                entries = [m.read32(vt + 4 * i) for i in range(3)]
                row['first_three_entries'] = [f'{entry:08X}' for entry in entries]
                selected_entries.update(entries)
                selected_vtables.setdefault(vt, bytes(u.mem_read(vt, 12)))
                if row['flags'] & 6:
                    row['mask_word'] = struct.unpack('<H', u.mem_read(blitter + 0xC, 2))[0]
        if pending and pc == pending[-1][0] and sp == pending[-1][1]:
            _, _, row = pending.pop()
            row['returned'] = True
            row['writes'] = summarize_stores(stores[row.pop('store_begin'):])
            row['after_planes'] = encoded(planes())
        if pc in (0x65C780, 0x65C7E0):
            rng.append(f'{pc:08X}')
        if pc == 0x422CA0:
            received.append(dict(caller=f'{m.read32(sp):08X}',
                point=shroud.rally.ints(u, m.read32(sp + 4), 2),
                clip=shroud.rally.ints(u, m.read32(sp + 8), 4),
                stored_xyz=shroud.rally.ints(u, anim + 0x9C, 3)))
        if pc == 0x4AED70:
            args = shroud.rally.ints(u, sp + 4, 14)
            row = dict(caller=f'{m.read32(sp):08X}', frame=args[1],
                source_is_physical_idle=args[0] == idle_image,
                point=shroud.rally.ints(u, args[2], 2), clip=shroud.rally.ints(u, args[3], 4),
                flags=args[4], stack_argument_5=args[5], z_adjust=args[6],
                gradient=args[7], intensity=args[8], color=args[9],
                has_zshape=bool(args[10]), zshape_frame=args[11], zshape_offset=args[12:14],
                source_frame=dict(canvas=list(struct.unpack('<2h', u.mem_read(idle_image + 2, 4))),
                    crop=list(struct.unpack('<4h', u.mem_read(idle_image + 8 + args[1] * 24, 8)))),
                before_planes=encoded(planes()), store_begin=len(stores), selectors=[], leaves=[])
            draws.append(row)
            pending.append((m.read32(sp), sp + 60, row))
        if pc in (0x490B90, 0x490E50):
            assert pending, 'Selected pixel object outside an observed Surface call'
            converter = u.reg_read(UC_X86_REG_ECX)
            row = dict(entry=f'{pc:08X}', caller=f'{m.read32(sp):08X}', flags=m.read32(sp + 4),
                converter=f'{converter:08X}', converter_identity=converter_identity(m, converter),
                anim_palette_at_selection=palette_receipt(m, anim), shade_count=m.read32(converter + 0x16C),
                half_mask=m.read32(converter + 0x180), quarter_mask=m.read32(converter + 0x184))
            assert row['half_mask'] == 0x7BEF and row['quarter_mask'] == 0x39E7
            pending[-1][2]['selectors'].append(row)
            pending_selectors.append((m.read32(sp), sp + 8, row))
        if pc in selected_entries and pending:
            leaves = pending[-1][2]['leaves']
            if f'{pc:08X}' not in leaves:
                leaves.append(f'{pc:08X}')

    def written(_u, _access, address, size, value, _data):
        for plane, start in (('color', PIXELS), ('z', Z), ('a', A)):
            if start <= address < start + length:
                stores.append([plane, address - start, size, value])
                break

    hooks = [u.hook_add(UC_HOOK_CODE, observe), u.hook_add(UC_HOOK_MEM_WRITE, written)]
    try:
        u.mem_write(clip, dwords(0, 0, 160, 160))
        returned = m.invoke(0x422C70, anim, (clip, 1, 0),
            instruction_limit=12000000, timeout_us=120000000)
    finally:
        for hook in hooks:
            u.hook_del(hook)
    if not draws:
        error = AssertionError(('Original DrawIfVisible produced no Surface call', label))
        error.native_draw_failure = dict(name=label, received_drawit=received,
            performance_before=performance_before, performance_after=performance_state(m),
            original_fps_gate_calls=fps_calls, after_planes=encoded(planes()))
        raise error
    assert not pending and not pending_selectors and not pending_fps
    assert len(fps_calls) == 1 and fps_calls[0].get('returned'), fps_calls
    assert fps_calls[0]['before']['current_fps'] >= fps_calls[0]['threshold'], fps_calls
    assert len(draws) == 1, ('Expected physical GAPOWR_A nonFlat body and no Shadow call', label, draws)
    assert all(row.get('returned') and row['source_is_physical_idle'] and row['leaves'] for row in draws)
    assert not rng
    assert all(raw == bytes(u.mem_read(p, len(raw))) for p, raw in selected_vtables.items())
    after = planes()
    return dict(name=label, entry='00422C70', returned_al=returned & 255,
        received_drawit=received, calls=draws, size=[160, 160],
        performance_before=performance_before, performance_after=performance_state(m),
        original_fps_gate_calls=fps_calls,
        before_planes=encoded(before), after_planes=encoded(after),
        a_unchanged=before['a'] == after['a'], z_unchanged=before['z'] == after['z'],
        rng_entries=rng, camera=[-384, 380],
        underlay='Supplied uniform color/A/Z; no terrain or Building-body draw',
        checked_selector_vtables_unchanged=True)


def _public_observations(value):
    """Keep measured data, excluding inherited raw resource-header snippets."""
    if isinstance(value, dict):
        return {key: _public_observations(item) for key, item in value.items()
                if key != 'header8_hex'}
    if isinstance(value, list):
        return [_public_observations(item) for item in value]
    return value


def generate():
    from tools.spatial_oracle.building_anim_translucency_setup import physical_inputs

    assets, physical = physical_inputs(configured_gamemd().parent)
    cases = [dict(name=name, background=background, a_value=a, initial_z=65535)
             for name, background, a in [('black_a127', 0, 127),
                 ('midtone_a127', 0xBDF7, 127), ('midtone_a63', 0xBDF7, 63)]]
    return _public_observations(dict(schema=1,
        cases=[execute_case(assets, physical, case) for case in cases]))


def _plane_text(runs):
    """Lossless decimal count:word pairs; reject incomplete/unbounded planes."""
    total = 0
    for run in runs:
        if not isinstance(run, list) or len(run) != 2:
            raise ValueError('Expected a two-integer plane run')
        count, word = run
        if type(count) is not int or type(word) is not int:
            raise ValueError('Plane runs require integer counts and uint16 words')
        if not 0 < count <= 25600 - total or not 0 <= word <= 65535:
            raise ValueError('Plane run exceeds the 160x160 uint16 boundary')
        total += count
    if total != 25600:
        raise ValueError('Plane must cover exactly 160x160 words')
    text = ' '.join(f'{count}:{word}' for count, word in runs)
    decoded = [[int(value) for value in pair.split(':')] for pair in text.split()]
    if decoded != runs:
        raise ValueError('Decimal plane encoding changed native run words')
    return text


def compact_projection(data):
    """Retain all draws/states; remove only duplicate planes and JSON padding.

    finish_vectors archives the complete unprojected packet deterministically.
    The sole native Surface call's planes are byte-equivalent to whole-draw
    planes for this physical noShadow type; verify that before dropping copies.
    """
    if data['schema'] != 1 or len(data['cases']) != 3:
        raise ValueError('Unexpected populated-slot corpus coverage')
    projected = dict(data, plane_encoding='decimal count:word RLE, row-major uint16')
    cases = []
    for case in data['cases']:
        if len(case['rows']) != 26:
            raise ValueError('Every case must retain all eight stage triplets and replacement draws')
        rows = []
        for row in case['rows']:
            if row['size'] != [160, 160] or len(row['calls']) != 1:
                raise ValueError('Projection requires a complete single-call 160x160 draw')
            call = row['calls'][0]
            projected_call = dict(call)
            for boundary in ('before_planes', 'after_planes'):
                if row[boundary] != call[boundary]:
                    raise ValueError('Whole-draw and Surface-call planes differ')
                planes = call[boundary]
                if set(planes) != {'color_words_rle', 'z_words_rle', 'a_words_rle'}:
                    raise ValueError('Expected complete color, Z and A planes')
                projected_call[boundary] = {key: _plane_text(runs)
                                            for key, runs in planes.items()}
            projected_row = {key: value for key, value in row.items()
                             if key not in ('before_planes', 'after_planes')}
            projected_row['calls'] = [projected_call]
            rows.append(projected_row)
        cases.append(dict(case, rows=rows))
    projected['cases'] = cases
    return projected


def metadata():
    return provenance(
        scope='Physical GAPOWR ActiveAnim slot creation, populated temporal notice, '
              'LetGo, AI and whole GAPOWR_A DrawIfVisible/raster; three supplied '
              'RGB565/A/Z underlays at eight actual AI frames and slot replacement.',
        assumptions=[
            'Each case uses a fresh shared TMP/palette VM. Original selected '
            'BuildingType/ART readers, Country5113F0, House4F54A0, Building43B740 '
            'and idle44D6A0 run. Physical GAPOWR must retain Enslaves=NULL; no slave '
            'manager or Infantry construction is suppressed. Selected physical '
            'layers are RULESMD, optional loose LANGRULE, MPBattleMD and XMP03T4.',
            'Country uses constructor defaults. Human/current-player/LightGold '
            'House fields and admitted alive/placed/Guard Building state are '
            'supplied. This is not Country/House INI loading, a complete stock '
            'type registry, map loading, Unlimbo or GrandOpening.',
            'Original mask startup4BAA73..4BAB06 runs before full palette/Convert '
            'construction. The actual selected LightGold converter and its masks '
            'are observed, not inferred from shade count. Original discovery '
            'geometry and Tactical matrix startup execute. Sparse flat revealed '
            'Cells, camera(-384,380), clock1000, game speed4, detail2 and uniform '
            '160x160 surfaces are supplied component inputs.',
            'With explicit empty counters/timer and Options detail2, original '
            'setters55AF40/55AF50 install minimum15/buffer5; original Logic '
            'counter55AFB0..55AFD9 and Throttle55E33B tail produce FPS60 from '
            '60 visits and supplied timeGetTime0/960ms. Original6C8C40 and '
            'GetMinFrameRate55AF60 stay live. Each draw records that admission '
            'call and its latch before/after. No physical DetailLevel, FPS '
            'result or gate answer is patched; full Logic/Options startup '
            'and actual host frame rate are excluded.',
            'Original451890 creates physical slot3 GAPOWR_A. AI423AC0 reaches '
            'frames0..7 without frame/timer writes. A declared already-linked '
            'zeroed lone Temporal head enters notice71B0EA..71B14E, which calls '
            'populated4521C0. Whole LetGo71ABC0 calls452210 and clears the native '
            'parent/head link. Four paused AI calls and resumed AI are measured. '
            'Replacement during erasure also uses original451890 and retirement.',
            'The original noShadow, nonFlat physical type must issue exactly one '
            'Surface call per complete DrawIfVisible422C70/DrawIt422CA0. The '
            'original Convert selector, decoder, row walker and pixel bodies run. '
            'Each draw starts with uniform inputs and retains full before/after '
            'color/Z/A RLE planes, actual arguments and store coverage; expected '
            'pixels never come from a host blend formula. No terrain or Building '
            'body is included in these reset draw surfaces.',
            'Only pristine missing Object-destructor vector headers receive '
            'their original CRT prefixes, before any Anim creation. Established '
            'live headers remain unchanged. Original text and listed class and '
            'observed pixel-object vtables are checked unchanged. No full CLEG '
            'shot/admission, Temporal constructor/AI, Display traversal or '
            'whole-scene equivalence is claimed.',
        ],
        substitutions=[
            'Shared cached lexical INI and immutable named-file transports '
            'supply physical strings and bytes; native archive search and the '
            'physical INI parser are not executed. Heap/delete/TLS/Win32 '
            'interlocked/GUID/exit-registration and setup clock transports are '
            'inherited and recorded. Empty spare registries and component '
            'Surface storage/vtable adapters are supplied.',
            'Performance timer initialization admits successful CRT atexit '
            'registration7C978A only from caller55E36F and records its callback. '
            'The exit callback is not invoked, and cdecl argument cleanup stays '
            'with the original caller. Throttle and FPS admission remain live.',
            'Object Mark5F5850 and Display registration4A9720 return1 and are '
            'recorded presentation sinks. No coordinate, slot/frame/pause/forced '
            'state, palette, selector, decoder, destructor or pixel answer is '
            'replaced. Complete original native code/vtables remain unchanged.',
        ],
        entry_points=dict(building_type=0x45DD90, building_reader=0x45FE50,
            country=0x5113F0, house=0x4F54A0, building=0x43B740,
            anim_type_factory=0x428B80, anim_type_reader=0x427D00,
            image_load=0x427B50, slot=0x451890, anim=0x421EA0,
            start=0x424CE0, ai=0x423AC0, notice_begin=0x71B0EA,
            notice_end=0x71B14E, building_notice=0x4521C0,
            let_go=0x71ABC0, building_release=0x452210,
            draw_if_visible=0x422C70, draw=0x422CA0, shape=0x4AED70,
            plain_selector=0x490B90, rle_selector=0x490E50,
            logic_counter=0x55AFB0, throttle_tail=0x55E33B,
            minimum_fps=0x55AF60,
            format_masks_begin=0x4BAA73, format_masks_end=0x4BAB06))


def source_paths():
    """Guard the selected native setup/transport owners, not Rust consumers."""
    root = Path(__file__).resolve().parents[2]
    names = (
        'tools/spatial_oracle/building_anim_translucency.py',
        'tools/spatial_oracle/building_anim_translucency_setup.py',
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
                   provenance=metadata, source_paths=source_paths(),
                   projection=compact_projection)
