"""Original physical SMKPUFF selection, animation and RGB565 translucency.

Imports and --help perform no native execution. See anim_translucency.md for
supplied component inputs and the boundary of the actual-emitter GPU comparison.
"""
from pathlib import Path
import json
import struct

from tools.native_oracle import configured_gamemd, finish_vectors, provenance


SIZE = (160, 160)
STAGES = (0, 5, 9, 13)


def physical_inputs(game):
    """Transport selected physical bytes; original scalar readers run below."""
    from tools.sidebar_oracle.stock import mix, mix_hash
    from tools.projectile_oracle.bridge_render_inputs import lexical

    outer = {name: mix((game / name).read_bytes())
             for name in ('ra2.mix', 'ra2md.mix', 'expandmd01.mix')}
    containers = [('expandmd01.mix', outer['expandmd01.mix'])]
    for top, children in (('ra2md.mix', ('conqmd.mix', 'genermd.mix', 'localmd.mix')),
                          ('ra2.mix', ('conquer.mix', 'generic.mix', 'cache.mix'))):
        containers += [(top + '/' + child, mix(outer[top][mix_hash(child)]))
                       for child in children]
    assets, sources = {}, []
    for name in ('SMKPUFF.SHP', 'ARTMD.INI', 'RULESMD.INI', 'ANIM.PAL', 'MPBattleMD.ini'):
        hits = [(archive, members[mix_hash(name)]) for archive, members in containers
                if mix_hash(name) in members]
        if not hits:
            raise ValueError('Missing physical input: ' + name)
        archive, raw = hits[0]
        assets[name.upper()] = raw
        sources.append(dict(name=name.upper(), archive=archive, bytes=len(raw)))
    art, _ = lexical(assets['ARTMD.INI'], {'SMKPUFF'})
    assert art['SMKPUFF'] == {'Translucent': 'yes', 'Translucency': '50'}
    rules, _ = lexical(assets['RULESMD.INI'],
        {'Animations', 'FlakWH', 'FlakGuyWH', 'NAFLAK', 'FlakWeapon'})
    assert 'SMKPUFF' in rules['Animations'].values()
    assert rules['FlakWH']['AnimList'] == rules['FlakGuyWH']['AnimList'] == 'SMKPUFF'
    assert rules['NAFLAK']['Primary'] == 'FlakWeapon'
    assert rules['FlakWeapon']['Warhead'] == 'FlakWH'
    layers = [dict(name='RULESMD.INI', sections={'FlakWH': rules['FlakWH']})]
    lang = game / 'LANGRULE.INI'
    if lang.exists():
        raw = lang.read_bytes()
        selected, _ = lexical(raw, {'FlakWH'})
        layers.append(dict(name='LANGRULE.INI', sections=selected))
        sources.append(dict(name='LANGRULE.INI', archive='loose', bytes=len(raw)))
    battle, _ = lexical(assets['MPBATTLEMD.INI'], {'FlakWH'})
    layers.append(dict(name='MPBattleMD.ini', sections=battle))
    return assets, dict(sources=sources, lexical_art=art, rules_layers=layers)


def anim_state(u, anim):
    """The constructor/AI fields consumed by this comparison, read live."""
    from tools.spatial_oracle.anim_bouncer_launch import constructor_state
    state = constructor_state(u, anim)
    return {key: state[key] for key in
            ('location', 'alive', 'limbo', 'marked', 'draw_flags', 'z_adjust', 'runtime')}


def word_runs(raw):
    """Lossless full row-major uint16 plane, as [run length, word] pairs."""
    result = []
    for (word,) in struct.iter_unpack('<H', raw):
        if result and result[-1][1] == word:
            result[-1][0] += 1
        else:
            result.append([1, word])
    assert b''.join(struct.pack('<H', word) * count for count, word in result) == raw
    return result


def execute_case(assets, metadata, case):
    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_ECX,
        UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP)
    from tools import native_oracle as native
    from tools.procedural_drawing_oracle import shroud
    from tools.spatial_oracle import tmp_clear_render as tmp
    from tools.spatial_oracle.bridge_constructor import OriginalBridgeConstructor
    from tools.spatial_oracle.building_body_rules import INI, RULES, SP, dwords
    from tools.spatial_oracle.trigger_action_values import initialize_discovery_geometry

    # Established palette/asset/surface VM, with no Building construction.
    m, _terrain, _files = tmp.prepare(tmp_names=('clear01.tem',), prepared_files=assets,
                                     native_format_masks=True)
    u = m.u
    format_masks = dict(m.format_mask_startup)
    format_masks['live_globals'] = dict(zip(('half', 'quarter'),
        struct.unpack('<2H', u.mem_read(0x8A0DE8, 4))))
    format_masks['converts'] = []
    for name, global_address in (('ANIM.PAL', 0x87F6C0), ('PALETTE.PAL', 0x87F6C4)):
        converter = m.read32(global_address)
        format_masks['converts'].append(dict(name=name,
            half=m.read32(converter + 0x180), quarter=m.read32(converter + 0x184)))
    assert format_masks['live_globals'] == dict(half=0x7BEF, quarter=0x39E7)
    assert all(row['half'] == 0x7BEF and row['quarter'] == 0x39E7
               for row in format_masks['converts'])
    # Draw422CA0 calls Cell487950 -> IsShrouded586360, which divides by the
    # original Map startup's ABDE88. TMP-only setup does not initialize it.
    # Run the shared exact startup owner, including the native Z multiplier;
    # do not supply a shroud result or manually replace the divisor.
    geometry = initialize_discovery_geometry(m.invoke, m.read32, u)
    assert m.read32(0x89E7C0) == m.read32(0xABDE88) == 104
    m.assets.update(assets)
    m.rules = m.alloc(0x5000)
    u.mem_write(0x8871E0, dwords(m.rules))
    m.invoke(0x665650, m.rules)
    OriginalBridgeConstructor.empty_registries(u, m.alloc, capacity=64,
                                               preserve=(0xB0F670,))
    u.mem_write(0xA8E9A8, dwords(0x7E9F24, m.alloc(256), 64, 1, 0, 10))
    u.mem_write(0xA8E9A0, b'\1')
    u.mem_write(0xA8E7AC, dwords(0))
    u.mem_write(0xA8ED84, dwords(1000))
    u.mem_write(0xA8EB60, dwords(4))
    scenario = m.read32(0xA8B230)
    m.invoke(0x65C6D0, scenario + 0x218, (0,))
    u.mem_write(scenario + 0x214, dwords(1000))
    _table, cells = shroud._prepare_flat_cells(m, range(7, 16), range(17, 26))
    for cell in cells:
        # Declared clear daylight receiver state; not a full lighting scenario.
        u.mem_write(cell + 0x10A, struct.pack('<h', 1000))
    tactical, point, clip, coord = (m.alloc(n) for n in (0x200, 8, 16, 12))
    u.mem_write(tactical, dwords(0x7F4348))
    u.mem_write(tactical + 0xB0, dwords(-384, 376))
    u.mem_write(0x887324, dwords(tactical))
    u.mem_write(0xB73550, dwords(1))
    u.mem_write(point, dwords(80, 70))
    u.mem_write(clip, dwords(0, 0, 160, 160))
    u.mem_write(0x886FA0, dwords(0, 0, 160, 160))
    u.mem_write(0xB0CE30, dwords(160, 160))
    u.mem_write(coord, dwords(2688, 5248, 0))
    code_before = bytes(u.mem_read(0x401000, 0x3E0000))
    vtables = {p: bytes(u.mem_read(p, n)) for p, n in (
        (0x7E3354, 0x210), (0x7E3608, 0xA0), (0x7E2070, 0x100))}
    setup_events, reached, phase = [], set(), 'type_reader'
    last_pcs = []

    def setup_transport(_u, pc, _size, _data):
        last_pcs.append(pc)
        if len(last_pcs) > 24:
            del last_pcs[0]
        if pc in (0x428B80, 0x427530, 0x427D00, 0x427B50, 0x421EA0,
                  0x424CE0, 0x423AC0, 0x422CA0, 0x4AED70, 0x75E3B0,
                  0x75CEC0, 0x75D3A0, 0x48A4F0):
            reached.add(pc)
        # Existing AnimationFixture boundaries only: Mark redraw admission and
        # Display registration. Neither supplies a shape/raster/frame result.
        if pc in (0x5F5850, 0x4A9720):
            sp = u.reg_read(UC_X86_REG_ESP)
            setup_events.append(dict(phase=phase, entry=f'{pc:08X}',
                caller=f'{m.read32(sp):08X}', argument=m.read32(sp + 4)))
            m.ret(1, 4)

    transport_hook = u.hook_add(UC_HOOK_CODE, setup_transport)
    try:
        # Full physical FlakWH reader owns AnimList factory binding. Incoming
        # damage/land/XYZ are an explicit SelectAnim boundary, not a fired shot.
        selected = dict(metadata['lexical_art']['SMKPUFF'])
        m.make_ini({'SMKPUFF': selected})
        assert m.read32(0x8874D0) == 0, 'do not reset a live Warhead registry'
        u.mem_write(0x8874C0, dwords(0x7EB6D4, m.alloc(256), 64, 1, 0, 10))
        warhead = m.invoke(0x75E3B0, m.cstring('FlakWH'))
        warhead_vtable = m.read32(warhead)
        vtables[warhead_vtable] = bytes(u.mem_read(warhead_vtable, 0x80))
        rules_reads = []
        for layer in metadata['rules_layers']:
            m.rules_cache(layer['sections'])
            admitted = m.invoke(0x75D3A0, warhead, (RULES,)) & 255
            vector, count = m.read32(warhead + 0x108), m.read32(warhead + 0x114)
            names = [m.string(m.read32(vector + 4*i) + 0x24) for i in range(count)]
            rules_reads.append(dict(file=layer['name'], body_admitted=bool(admitted),
                                    anim_list=names))
        assert names == ['SMKPUFF'], rules_reads
        u.mem_write(SP, dwords(native.RET_MAGIC, case['land'], coord))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, case['damage'])
        u.reg_write(UC_X86_REG_EDX, warhead)
        native.run_checked(u, 0x48A4F0, native.RET_MAGIC,
                           required_addresses=(0x48A4F0,))
        typ = u.reg_read(UC_X86_REG_EAX)
        assert typ and m.string(typ + 0x24) == 'SMKPUFF'
        assert typ == m.invoke(0x428B80, m.cstring('SMKPUFF'))
        type_vtable = m.read32(typ)
        vtables[type_vtable] = bytes(u.mem_read(type_vtable, 0xA4))
        selection = dict(damage=case['damage'], land=case['land'],
            coordinate=list(struct.unpack('<3i', u.mem_read(coord, 12))),
            warhead='FlakWH', selected=m.string(typ + 0x24), rules_reads=rules_reads)
        mark = len(m.asset_loaded)
        admitted = m.invoke(0x427D00, typ, (INI,))
        reader = m.result('SMKPUFF', typ, admitted)
        reader.update(translucent=bool(u.mem_read(typ + 0x36A, 1)[0]),
                      translucency=struct.unpack('<i', u.mem_read(typ + 0x2EC, 4))[0],
                      translucency_detail_level=m.read32(typ + 0x2D8),
                      flat=bool(u.mem_read(typ + 0x369, 1)[0]),
                      tiled=bool(u.mem_read(typ + 0x35B, 1)[0]),
                      y_draw_offset=struct.unpack('<i', u.mem_read(typ + 0x344, 4))[0],
                      z_adjust=struct.unpack('<i', u.mem_read(typ + 0x348, 4))[0],
                      asset_loads=[dict(name=entry['name'], bytes=entry['bytes'])
                                   for entry in m.asset_loaded[mark:]])
        reader = {key: reader[key] for key in (
            'name', 'art_body_read', 'start', 'loop_start', 'loop_end', 'end',
            'loop_count', 'rate', 'image', 'raw_shp_frame_count', 'middle',
            'random_rate', 'shadow', 'translucent', 'translucency',
            'translucency_detail_level', 'flat', 'tiled', 'y_draw_offset',
            'z_adjust', 'asset_loads')}
        image = m.read32(typ + 0xA4)
        assert image and bytes(u.mem_read(image, len(assets['SMKPUFF.SHP']))) == assets['SMKPUFF.SHP']
        assert reader['art_body_read'] and reader['raw_shp_frame_count'] == 20
        assert reader['shadow'] == (selected.get('Shadow', '').lower() == 'yes')
        assert reader['flat'] == (selected.get('Flat', '').lower() == 'yes')
        assert not reader['tiled']
        assert reader['translucent'] and reader['translucency'] == 50
        anim = m.alloc(0x1C8)
        phase = 'constructor'
        m.invoke(0x421EA0, anim, (typ, coord, 0, 1, case['constructor_flags'], 0, 0))
        assert m.read32(anim) == 0x7E3354
        constructed = anim_state(u, anim)
        assert constructed['runtime']['current_frame'] == 0
        assert constructed['draw_flags'] == case['constructor_flags']
        visits, rows = 0, []
        frame_counter = 1000
        stages = STAGES
        for requested_stage in stages:
            phase = 'ai_to_next_frame'
            while m.read32(anim + 0xAC) != requested_stage:
                frame_counter += 1
                assert frame_counter <= 1512, ('frame_not_reached', requested_stage)
                u.mem_write(0xA8ED84, dwords(frame_counter))
                m.invoke(0x423AC0, anim)
                state = anim_state(u, anim)
                visits += 1
                assert state['alive'] and not state['runtime']['inactive'], state
            phase = 'draw_stage_' + str(requested_stage)
            row = draw_frame(m, anim, image, point, clip, requested_stage,
                             code_before, vtables, case)
            row.update(clock=frame_counter, ai_visits=visits, runtime=anim_state(u, anim)['runtime'])
            rows.append(row)
        required = {0x428B80, 0x427530, 0x427D00, 0x427B50, 0x421EA0,
                    0x424CE0, 0x422CA0, 0x4AED70, 0x75E3B0,
                    0x75CEC0, 0x75D3A0, 0x48A4F0}
        if any(stage != 0 for stage in stages):
            required.add(0x423AC0)
        assert required <= reached, reached
        assert code_before == bytes(u.mem_read(0x401000, 0x3E0000))
        assert all(raw == bytes(u.mem_read(p, len(raw))) for p, raw in vtables.items())
        return dict(input=case, selection=selection, type_reader=reader, constructor=constructed,
            draws=rows, setup_boundaries=setup_events,
            geometry_startup=geometry, format_mask_startup=format_masks,
            required_reached=[f'{pc:08X}' for pc in sorted(reached)],
            checked_vtable_ranges=[dict(address=f'{p:08X}', bytes=len(raw))
                                   for p, raw in sorted(vtables.items())],
            checked_text_and_vtables_unchanged=True)
    except Exception as error:
        error.add_note(json.dumps(dict(case=case, phase=phase,
            eip=f'{u.reg_read(UC_X86_REG_EIP):08X}',
            esp=f'{u.reg_read(UC_X86_REG_ESP):08X}',
            last_pcs=[f'{pc:08X}' for pc in last_pcs],
            setup_boundaries=setup_events[-12:]), indent=2))
        raise
    finally:
        u.hook_del(transport_hook)


def draw_frame(m, anim, image, point, clip, stage, code_before, vtables, case):
    from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
    from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ESP
    from tools.projectile_oracle.bridge_render_shape import prepare_surface, PIXELS, Z, A
    from tools.spatial_oracle.bridge_shadow_render import ints
    u = m.u
    size, length = list(SIZE), SIZE[0] * SIZE[1] * 2
    old_z = case['initial_z']
    prepare_surface(u, dict(depth_baseline=32768, old_z=old_z, a_value=case['a_value'],
                            background=case['background']), size=size)
    before = {key: bytes(u.mem_read(ptr, length)) for key, ptr in (
        ('color', PIXELS), ('z', Z), ('a', A))}
    draws, pending, stores, rng, selectors = [], [], [], [], []
    # These known retained leaves are observation addresses, never selected by
    # Python. Dynamic selector outputs below discover other exact leaf entries.
    known_leaves = {0x494B60, 0x497FD0, 0x492D20, 0x496820,
                    0x493830, 0x497390, 0x4990E0, 0x4958D0,
                    0x499280, 0x4959F0}
    selected_entries, pending_selectors = set(), []

    def observe(_u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if pending_selectors and pc == pending_selectors[-1][0] and sp == pending_selectors[-1][1]:
            _, _, row = pending_selectors.pop()
            blitter = u.reg_read(UC_X86_REG_EAX)
            row['returned_blitter'] = f'{blitter:08X}'
            if blitter:
                vt = m.read32(blitter)
                row['vtable'] = f'{vt:08X}'
                entries = [m.read32(vt + i * 4) for i in range(3)]
                row['vtable_first_three_entries'] = [f'{p:08X}' for p in entries]
                if row['flags'] & 6:
                    row['mask_word'] = struct.unpack('<H', u.mem_read(blitter + 0xC, 2))[0]
                    expected = 0x7BEF if (row['flags'] & 6) == 4 else 0x39E7
                    assert row['mask_word'] == expected, row
                selected_entries.update(entries)
                vtables.setdefault(vt, bytes(u.mem_read(vt, 12)))
        if pending and pc == pending[-1][0] and sp == pending[-1][1]:
            _, _, row = pending.pop()
            row['returned'] = True
            row['store_count'] = len(stores) - row.pop('store_begin')
        if pc in (0x65C780, 0x65C7E0):
            rng.append(f'{pc:08X}')
        if pc in (0x490B90, 0x490E50):
            row = dict(entry=f'{pc:08X}', caller=f'{m.read32(sp):08X}',
                       flags=m.read32(sp + 4))
            selectors.append(row)
            pending_selectors.append((m.read32(sp), sp + 8, row))
        if pc == 0x4AED70:
            args = ints(u, sp + 4, 14)
            row = dict(caller=f'{m.read32(sp):08X}', frame=args[1],
                point=ints(u, args[2], 2), clip=ints(u, args[3], 4),
                flags=args[4], z_adjust=args[6], gradient=args[7], intensity=args[8],
                color=args[9], has_zshape=bool(args[10]), zshape_frame=args[11],
                zshape_offset=args[12:14], source_is_physical_smkpuff=args[0] == image,
                source_frame=dict(canvas=ints16(u, image + 2, 2),
                    crop=ints16(u, image + 8 + args[1] * 24, 4)),
                store_begin=len(stores), leaves=[])
            draws.append(row)
            pending.append((m.read32(sp), sp + 60, row))
        if pc in known_leaves | selected_entries and draws:
            if f'{pc:08X}' not in draws[-1]['leaves']:
                draws[-1]['leaves'].append(f'{pc:08X}')

    def written(_u, _access, address, count, value, _data):
        for label, start in (('color', PIXELS), ('z', Z), ('a', A)):
            if start <= address < start + length:
                stores.append(dict(plane=label, byte_offset=address - start,
                                   byte_count=count, value=value))
                break

    hooks = [u.hook_add(UC_HOOK_CODE, observe), u.hook_add(UC_HOOK_MEM_WRITE, written)]
    try:
        m.invoke(0x422CA0, anim, (point, clip), instruction_limit=12000000,
                 timeout_us=120000000)
    finally:
        for hook in hooks:
            u.hook_del(hook)
    assert not pending, '4AED70 call did not return through its observed original caller'
    assert not pending_selectors, 'Convert selector did not return'
    assert draws and all(row['source_is_physical_smkpuff'] and row.get('returned') for row in draws)
    assert all(row['leaves'] for row in draws), 'Draw admitted but no original raster leaf reached'
    assert len(draws) == 1, 'stock SMKPUFF has no authored Shadow companion'
    assert draws[0]['frame'] == stage
    assert not rng
    assert code_before == bytes(u.mem_read(0x401000, 0x3E0000))
    assert all(raw == bytes(u.mem_read(p, len(raw))) for p, raw in vtables.items())
    after = {key: bytes(u.mem_read(ptr, length)) for key, ptr in (
        ('color', PIXELS), ('z', Z), ('a', A))}
    assert before['color'] == struct.pack('<H', case['background']) * (SIZE[0] * SIZE[1])
    assert before['z'] == struct.pack('<H', old_z) * (SIZE[0] * SIZE[1])
    assert before['a'] == struct.pack('<H', case['a_value']) * (SIZE[0] * SIZE[1])
    assert before['a'] == after['a'], 'Unexpected A-plane change'
    assert before['z'] == after['z'], 'Unexpected Z-plane change'
    counts = {plane: sum(store['plane'] == plane for store in stores)
              for plane in ('color', 'z', 'a')}
    assert counts['color'] > 0 and counts['z'] == counts['a'] == 0
    return dict(stage=stage, size=size, world_coord=ints(u, anim + 0x9C, 3),
        anim_draw_flags=m.read32(anim + 0x190), call=draws[0], selectors=selectors,
        color_words_rle=word_runs(after['color']), z_unchanged=True, a_unchanged=True,
        color_store_count=counts['color'], z_store_count=counts['z'],
        a_store_count=counts['a'], rng_entries=rng)


def ints16(u, pointer, count):
    return list(struct.unpack('<' + 'h' * count, u.mem_read(pointer, 2 * count)))


def generate():
    assets, physical = physical_inputs(configured_gamemd().parent)
    cases = [dict(name=name, constructor_flags=0x600, damage=40, land=-1,
                  background=background, a_value=a, initial_z=65535)
             for name, background, a in [('stock_black', 0, 127),
                 ('stock_midtone', 0xBDF7, 127), ('stock_dim_a', 0xBDF7, 63)]]
    return dict(schema=1, inputs=dict(size=list(SIZE), world_coord=[2688, 5248, 0],
        screen_point=[80, 70], clip=[0, 0, 160, 160], frame=1000,
        constructor_flags=0x600, loop_count=1, delay=0, detail_level=2,
        game_speed=4, scenario_seed=0, cell_intensity=1000,
        depth_baseline=32768, sources=physical['sources']),
        cases=[execute_case(assets, physical, case) for case in cases])


def metadata():
    return provenance(
        scope='Physical FlakWH selection and SMKPUFF reader/constructor/AI/DrawIt/raster, '
              'three supplied color/A surfaces at four actual animation stages.',
        assumptions=[
            'Each case uses a fresh shared TMP/palette VM. Original physical palette startup, '
            'Rules665650, Warhead75CEC0 and layered75D3A0, AnimType427530/427D00/427B50, '
            'SelectAnim48A4F0, Anim421EA0/424CE0/423AC0, DrawIt422CA0 and its selected '
            '4AED70/Convert/raster bodies execute unchanged.',
            'The shared palette owner explicitly opts into original RGB565 mask startup '
            '4BAA73..4BAB06 with EDX3/EAX0/ESI2 before full palette/Convert construction. '
            'Global8A0DE8/EA, Convert+180/+184 and actual selected blitter+C are observed; '
            'no constructed mask field is patched. Other existing fixture callers retain '
            'their historical default setup. Original text is checked across startup.',
            'Selected retail bytes come from the configured executable directory. '
            'MIX lookup and physical strings are transported through existing cached INI/asset owners; '
            'the original archive/physical INI parser is not executed. '
            'RULESMD, optional loose LANGRULE, and MPBattleMD supply the FlakWH read sequence. '
            'There is no scenario rules layer.',
            'Inputs supply damage40, land-1, XYZ2688/5248/0, flags0x600, loop1, delay0, '
            'clock1000, game speed4, detail2, flat sparse cells7..15/17..25 and daylight intensity1000. '
            'Original discovery geometry startup establishes its projection divisors. '
            'Screen point80/70 and clip160x160 are supplied DrawIt inputs; '
            'no fired shot, whole map, Display traversal or building-slot/temporal composition runs.',
            'Every draw starts from uniform RGB565/A/Z input. Complete final RGB565 words are '
            'losslessly run-length encoded; exact Z/A equality and zero stores are checked. '
            'Z65535 does not establish occluded-depth behavior. Draws must consume no RNG. '
            'Original text401000..7E0FFF and the listed original Anim/AnimType/Warhead/surface '
            'and observed raster-vtable ranges remain byte-for-byte unchanged.',
        ],
        substitutions=[
            'Shared PaletteReader heap/delete/TLS/platform and immutable named-file transports; '
            'the existing TMP startup also supplies its sparse registration/map state. '
            'RGB565 surface/format inputs and spare-capacity registry storage are supplied; '
            'original mask startup, palette expansion and full Convert constructors '
            'compute the masks and tables.',
            'Object Mark5F5850 returns1 and Display registration4A9720 returns1, as in '
            'AnimationFixture; both entries are recorded. No type/frame/selector/palette/decoder '
            'or pixel result is supplied.',
        ],
        entry_points=dict(warhead_factory=0x75E3B0, warhead_reader=0x75D3A0,
            select_anim=0x48A4F0, anim_type_factory=0x428B80, anim_type_reader=0x427D00,
            image_load=0x427B50, constructor=0x421EA0, start=0x424CE0, ai=0x423AC0,
            draw=0x422CA0, shape=0x4AED70, plain_selector=0x490B90, rle_selector=0x490E50,
            format_masks_begin=0x4BAA73, format_masks_end=0x4BAB06))


if __name__ == '__main__':
    root = Path(__file__).resolve().parents[2]
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata,
        source_paths={name: root / name for name in (
            'tools/spatial_oracle/anim_translucency.py',
            'tools/native_oracle.py',
            'tools/oracle_sources.py',
            'tools/sidebar_oracle/stock.py',
            'tools/projectile_oracle/bridge_render_inputs.py',
            'tools/projectile_oracle/bridge_render_inputs_palette.py',
            'tools/projectile_oracle/bridge_render_shape.py',
            'tools/rules_oracle/bridge_anim_inputs.py',
            'tools/rules_oracle/bridge_anim_lists.py',
            'tools/procedural_drawing_oracle/shroud.py',
            'tools/spatial_oracle/anim_bouncer_launch.py',
            'tools/spatial_oracle/tmp_clear_render.py',
            'tools/spatial_oracle/bridge_constructor.py',
            'tools/spatial_oracle/building_body_rules.py',
            'tools/spatial_oracle/trigger_action_values.py',
            'tools/spatial_oracle/bridge_shadow_render.py',
        )})
