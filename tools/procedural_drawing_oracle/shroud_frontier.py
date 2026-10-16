"""Stationary retail AMCV admission through current Cell knowledge, A and TMP.

One existing Mission/PaletteReader VM owns the joined case. The independent
callback control starts the same owner again. No renderer, parser or sight
kernel is implemented here; all expected state/pixels come from original code.
"""
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import hashlib
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_EIP
from tools import native_oracle as native
from tools.projectile_oracle.bridge_render_inputs import lexical
from tools.projectile_oracle.bridge_render_inputs_palette import PaletteReader, initialize
from tools.projectile_oracle.bridge_render_shape import prepare_surface, AOBJ, A, PIXELS, Z
from tools.spatial_oracle.anytown_damage.mission import Mission
from tools.spatial_oracle.anytown_damage import mtnk_attack as base
from tools.spatial_oracle.bridge_shadow_render import physical_assets, initialize_cell_convert, ints, WIDTH, HEIGHT, COUNT
from tools.spatial_oracle.building_body_rules import SP, RULES, dwords
from tools.spatial_oracle.shrapnel_repair.map_facts import decode_cells
from tools.spatial_oracle.tmp_clear_render import theater_tile_files, load_tmp_files, profile, execute as draw_tmp
from tools.spatial_oracle.trigger_action_values import DiscoveryFixture, initialize_discovery_geometry
from tools.procedural_drawing_oracle.shroud import Shroud

SOURCE_CELL = (109, 27)
MATERIAL_CELL = (111, 22)
VIEW_ANCHOR = (113, 22)
CROP = (90, 8, 128, 46)
CLEAR_NAMES = tuple(f'clear01{s}.urb' for s in ('', 'a', 'b', 'c', 'd', 'e', 'f', 'g'))
CALLBACK = 0x210F0000


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def cache(m, source):
    return dict(latch=m.u.mem_read(source + 0x250, 1)[0],
        xyz=ints(m.u, source + 0x254, 3), radius=ints(m.u, source + 0x260, 1)[0])


def rng_hashes(f):
    return {name: sha(base.sr.state_bytes(base.sr.rng_state(f.u, pointer)))
            for name, pointer in f.resident.rngs.items()}


def cell_states(f):
    proxy = SimpleNamespace(uc=f.u, read32=f.m.read32)
    return {xy: DiscoveryFixture.pointer_state(proxy, pointer, include_dirty=True)
            for xy, pointer in sorted(f.resident.ptrs.items())}


def layers(assets, directory):
    result = []
    for name in ('RULESMD.INI', 'MPBATTLEMD.INI', 'GOLDST.MMX'):
        path = Path(directory) / name
        path.write_bytes(assets[name])
        result.append((name, path))
    return result


def geometry(f):
    return initialize_discovery_geometry(lambda a, receiver, args: f.m.invoke(a, receiver, args),
                                         f.m.read32, f.u)


def waypoint(m, raw):
    """Original scenario Waypoint3 parser/packed stores, existing INI transport."""
    sections, _ = lexical(raw, {'Waypoints'})
    m.rules_cache(sections)
    target = m.alloc(4)
    m.u.mem_write(SP + 0x10, b'3\0')
    for register, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBX, RULES),
                            (UC_X86_REG_ESI, 3), (UC_X86_REG_EDI, target)):
        m.u.reg_write(register, value)
    native.run_checked(m.u, 0x68BDE7, 0x68BE35, required_addresses=(0x5276D0,))
    result = list(struct.unpack('<hh', m.u.mem_read(target, 4)))
    assert result == list(SOURCE_CELL)
    return dict(index=3, physical_literal=sections['Waypoints']['3'], native_cell=result,
                entry='0068BDE7', end='0068BE35')


def world(f, assets):
    """Supplied bounded registration of native-constructed physical Cell fields."""
    m, u = f.m, f.u
    parsed, authored = decode_cells(assets['GOLDST.MMX'])
    crop = {xy: row for xy, row in authored.items()
            if CROP[0] <= xy[0] <= CROP[2] and CROP[1] <= xy[1] <= CROP[3]}
    tiles, mapping = theater_tile_files(m, assets['URBANMD.INI'], authored[SOURCE_CELL]['tile'], 'urb')
    names = sorted({tiles[row['tile'] if row['tile'] != 65535 else 0]
                    for row in crop.values()} | set(CLEAR_NAMES))
    tmp_assets, sources = physical_assets(frontier=True, tmp_names=names)
    files = {name: tmp_assets[name.upper()] for name in names}
    m.assets.update(tmp_assets)
    heads, _file_io = load_tmp_files(m, files)
    registry = m.alloc(len(tiles) * 4)
    for tile, name in tiles.items():
        if name in heads:u.mem_write(registry + tile * 4, dwords(heads[name]))
    u.mem_write(0xA8ED2C, dwords(registry))
    u.mem_write(0xA8ED38, dwords(len(tiles)))
    table = m.read32(0x87F7E8 + 0x13C)
    u.mem_write(table, bytes(0x100000))
    f.resident.ptrs = {}
    native_fields = {}
    for xy, row in crop.items():
        pointer = m.alloc(0x148)
        m.invoke(0x47BBF0, pointer)
        pair = m.alloc(4)
        u.mem_write(pair, struct.pack('<hh', *xy))
        m.invoke(0x485240, pointer, (pair,))
        tile = row['tile'] if row['tile'] != 65535 else 0
        head = heads[tiles[tile]]
        slope = m.invoke(0x5471B0, head, (row['subtile'],)) & 255
        land = m.invoke(0x544BE0, head, (row['subtile'],))
        u.mem_write(pointer + 0x38, dwords(tile))
        u.mem_write(pointer + 0x11A, bytes((row['subtile'], row['level'], slope, 0)))
        u.mem_write(pointer + 0xEC, dwords(land))
        u.mem_write(pointer + 0x4C, dwords(0))
        u.mem_write(pointer + 0x116, b'\xff\xff')
        u.mem_write(table + (xy[1] * 512 + xy[0]) * 4, dwords(pointer))
        f.resident.ptrs[xy] = pointer
        native_fields[xy] = dict(tile=tile, subtile=row['subtile'], level=row['level'], slope=slope, land=land)
    u.mem_write(0x87F7E8 + 0xF4, dwords(103, 82, 2, 4, 99, 72))
    u.mem_write(0x87F7E8 + 0x124, dwords(0, 0, 511, 511))
    # Existing Resident zone-storage format; outside this physical crop stays
    # blocked. This supplied transport is not a native map population sweep.
    stride = 186
    count = stride * stride
    u.mem_write(0x87F7E8 + 0x68, dwords(0x44200000, count, 0x44300000))
    u.mem_write(0x44200000, b'\x07\0\0\0' * count)
    for xy, row in crop.items():
        u.mem_write(0x44200000 + (xy[1] * stride + xy[0]) * 4, bytes((0, row['level'])))
        u.mem_write(0x44300000 + (xy[1] * stride + xy[0]) * 10 + 8, bytes((row['level'],)))
    return authored, native_fields, heads, registry, dict(
        name='GoldSt.mmx', crop=list(CROP), cell_count=len(crop),
        physical_map_section=parsed['Map'], waypoint=waypoint(m, assets['GOLDST.MMX']),
        source=dict(authored=authored[SOURCE_CELL], native=native_fields[SOURCE_CELL], file=tiles[native_fields[SOURCE_CELL]['tile']], mapping=mapping),
        material=dict(coords=list(MATERIAL_CELL), authored=authored[MATERIAL_CELL], native=native_fields[MATERIAL_CELL]),
        boundary='Physical authored fields and original TMP getters/native Cell constructors; supplied sparse registration, empty live overlays/residents and blocked outside zones. Authored FFFF enrolls native ClearTile0. Not full native Recalc/LAT, map/zone boot or starting-force chronology.'), sources


def observe_admission(f, stages, seen, geometry_calls, area, targets, notifications, discovers, border):
    m, u = f.m, f.u
    pending = {}
    def observe(_u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc == 0x737BA0:
            stages['before_unlimbo'] = dict(cache=cache(m, f.src), rng=rng_hashes(f),
                discovery=list(u.mem_read(f.src + 0x41A, 3)))
        if pc == 0x5678E0:
            assert bytes(u.mem_read(0xB0CD48, 8)).hex() == '60493474535ec23f'
            args = ints(u, sp + 4, 8)
            area.append(dict(coords=ints(u, args[0], 3), radius=args[1], by_height=args[-2], final=args[-1]))
        if pc == 0x6D20E0:
            pending[m.read32(sp)] = dict(input_z=u.reg_read(UC_X86_REG_ECX))
        if pc in pending:
            row = pending.pop(pc)
            row['output'] = u.reg_read(UC_X86_REG_EAX)
            if row not in geometry_calls:geometry_calls.append(row)
        if pc == 0x653830:targets.append(list(struct.unpack('<hh', u.mem_read(m.read32(sp + 4), 4))))
        if pc == 0x5865F0:
            pointer = m.read32(sp + 4)
            notifications.append(list(struct.unpack('<hh', u.mem_read(pointer + 0x24, 4))))
        if pc == 0x6F4960:
            discovers.append(dict(source=u.reg_read(UC_X86_REG_ECX) == f.src, current_house=m.read32(sp + 4) == f.house))
        if pc == 0x6F6D2D:
            stages['after_area'] = dict(cache=cache(m, f.src), cells=cell_states(f))
        if pc == 0x567DA0:
            args = ints(u, sp + 4, 4)
            border.append(dict(coords=ints(u, args[0], 3), height=args[1], radius=args[2], skip=args[3]))
        if pc == 0x6F6D66:
            stages['after_border'] = dict(cache=cache(m, f.src), cells=cell_states(f))
        if pc in (0x70AF50, 0x5678E0, 0x487630, 0x567DA0, 0x6D3660, 0x6D71E0, 0x4801F0, 0x47EFE0):
            seen[f'{pc:08X}'] = seen.get(f'{pc:08X}', 0) + 1
    return u.hook_add(UC_HOOK_CODE, observe)


def joined(assets, sources, selected_layers):
    f = Mission(reader_factory=PaletteReader)
    m, u = f.m, f.u
    m.phase = 'frontier'
    startup = geometry(f)
    f.resident.phase = 'frontier'  # no inherited measurement/gameplay/projection substitutions
    m.assets.update(assets)
    authored, fields, heads, registry, map_input, tmp_sources = world(f, assets)
    scenario = m.read32(0xA8B230)
    m.invoke(0x6B8AE0, scenario)
    special_constructor = m.read32(scenario)
    special, _ = lexical(assets['GOLDST.MMX'], {'SpecialFlags'})
    m.rules_cache(special)
    u.mem_write(0xA8B238, dwords(5))
    m.invoke(0x6B8CA0, scenario, (RULES,))
    special_flags = dict(constructor=special_constructor, physical_map=special,
                         after_physical_map=m.read32(scenario))
    assert m.read32(scenario) & 1 == 0  # native ordinary FogOfWar=no
    stages = dict(before=dict(cells=cell_states(f)))
    seen, height_calls, areas, targets, notifications, discovers, border = {}, [], [], [], [], [], []
    observer = observe_admission(f, stages, seen, height_calls, areas, targets, notifications, discovers, border)
    f.setup(source_cell=SOURCE_CELL, source_type_name='AMCV', source_art_bytes=assets['ARTMD.INI'],
            source_art_names=('AMCV', 'MCV'), source_layer_paths=selected_layers,
            classify_source_owner=True, stop_after_placement=True)
    assert m.read32(f.typ + 0x5E8) == 6
    stages['after_unlimbo'] = dict(cache=cache(m, f.src), rng=rng_hashes(f),
        discovery=list(u.mem_read(f.src + 0x41A, 3)))
    assert stages['before_unlimbo']['rng'] == stages['after_unlimbo']['rng']
    assert f.after_placement['position'] == [28032, 7040, 624]
    # Shared BSurface/circular A/Z owner. Native A fill/draw follows this setup;
    # no subsequent material setup may replace this plane.
    u.mem_map(0x21000000, 0x200000)
    prepare_surface(u, dict(depth_baseline=32768, old_z=65535), size=(WIDTH, HEIGHT),
                    initialize_buffer_height=True)
    u.mem_write(0x89E7C5, b'\0')  # original cold47EFE0 loads both physical resources
    u.mem_write(0x89E794, dwords(0))
    u.mem_write(0x89E790, dwords(0))
    u.mem_write(0xB73550, dwords(1))
    u.mem_write(0x88A100, dwords(0))
    u.mem_write(0xB0CE88, dwords(0))
    u.mem_write(0x886FA0, dwords(0, 0, WIDTH, HEIGHT))
    u.mem_write(0xB0CE30, dwords(WIDTH, HEIGHT))
    tactical = m.read32(0x887324)
    point, screen = m.alloc(12), m.alloc(8)
    m.invoke(0x486840, f.resident.ptrs[VIEW_ANCHOR], (point,))
    m.invoke(0x6D2140, tactical, (point, screen))
    projected = ints(u, screen, 2)
    camera = [projected[0] - 96, projected[1] - 80]
    assert camera == [2634, 1870]
    u.mem_write(tactical + 0xB0, dwords(*camera))
    drawing = Shroud.attach(m, size=(WIDTH, HEIGHT), tactical=tactical,
        abuffer=AOBJ, alpha=A, cells=f.resident.ptrs)
    a_output = drawing.draw_current(dict(full_redraw=True, instruction_limit=12000000,
        timeout_us=120000000), preserve_dirty=True)
    stages['after_draw'] = dict(cells=cell_states(f))
    alpha_words = bytes(u.mem_read(A, COUNT * 2))
    assert a_output['alpha_hex'] == bytes(value[0] for value in struct.iter_unpack('<H', alpha_words)).hex()
    initialize(m)
    palette = initialize_cell_convert(m, source_palette='ISOURB.PAL')
    lighting = profile(m, assets['GOLDST.MMX'])
    for entry in (0x7C5F3D, 0x7C5FFF):m.invoke(entry, 0)
    assert u.mem_read(0x84E862, 1) == b'\1' and u.mem_read(0xABC55C, 1) == b'\0'
    assert bytes(u.mem_read(A, COUNT * 2)) == alpha_words
    for index, name in enumerate(CLEAR_NAMES):
        u.mem_write(heads[name] + 0x2BC, dwords(heads[CLEAR_NAMES[index + 1]] if index + 1 < len(CLEAR_NAMES) else 0))
        u.mem_write(heads[name] + 0x2F0, dwords(len(CLEAR_NAMES) - index))
    u.mem_write(registry, dwords(heads[CLEAR_NAMES[0]]))
    rows = []
    cell = f.resident.ptrs[MATERIAL_CELL]
    for index in range(8):
        row = draw_tmp(m, dict(name=f'clear_variant_{index}', coords=list(MATERIAL_CELL), level=6,
            tile_id=0, subtile=0, detail=2, scene_lighting=lighting,
            full_view=dict(view_origin=camera, redraw_rect=[0, 0, WIDTH, HEIGHT]),
            selector_table=[index] * 64), current_cell=cell, current_tactical=tactical, preserve_alpha=True)
        row['history'] = 'initial_amcv'
        z_values = struct.unpack('<' + 'H' * COUNT, u.mem_read(Z, COUNT * 2))
        a_values = struct.unpack('<' + 'H' * COUNT, alpha_words)
        row['output']['sampled_a_values'] = sorted({a_values[i] for i, depth in enumerate(z_values) if depth != 65535})
        row['output']['drawn_pixels'] = sum(depth != 65535 for depth in z_values)
        assert row['output']['drawn_pixels'] == 900
        assert len(row['output']['sampled_a_values']) > 1
        assert row['output']['selected_file_index'] == index
        rows.append(row)
    u.hook_del(observer)
    before = stages['before']['cells']
    common = {k: value for k, value in next(iter(before.values())).items() if k not in ('cell', 'level')}
    assert all({k: value for k, value in state.items() if k not in ('cell', 'level')} == common for state in before.values())
    prior, changes = before, {}
    for name in ('after_area', 'after_border', 'after_draw'):
        current = stages[name]['cells']
        changes[name] = [state for xy, state in current.items() if state != prior[xy]]
        prior = current
    samples = {(106,24), (109,27), (110,22), (111,21), (111,22), (112,22), (113,22), (120,30)}
    cache_and_rng = {name: stages[name]['cache'] for name in
                     ('before_unlimbo', 'after_area', 'after_border', 'after_unlimbo')}
    cache_and_rng['rng_unlimbo'] = {name: dict(before_sha256=value,
        after_sha256=stages['after_unlimbo']['rng'][name])
        for name,value in stages['before_unlimbo']['rng'].items()}
    history = dict(name='initial_amcv', input=dict(camera=camera, clip=[0,0,WIDTH,HEIGHT]),
        initial_cell_common=common, source_states=dict(after_ctor=f.after_ctor, after_unlimbo=f.after_placement,
            cache_and_rng=cache_and_rng,
            discovery_bytes_41a_41c=dict(before_unlimbo=stages['before_unlimbo']['discovery'],
                                       after_unlimbo=stages['after_unlimbo']['discovery'])),
        cell_states=changes,
        before_draw_cells=[state for xy, state in stages['after_border']['cells'].items() if xy in samples],
        after_draw_cells=[state for xy, state in stages['after_draw']['cells'].items() if xy in samples],
        notifications=dict(cells=notifications, count=len(notifications), source_discover=discovers),
        border=border, output={k: a_output[k] for k in ('alpha_hex', 'alpha_sha256')},
        draws=a_output['draws'], draw_cell_order=a_output['cell_order'])
    table_raw = bytes(u.mem_read(0xABD490, 369 * 4))
    counts = ints(u, 0x7ED3D0, 12)
    assert counts[-1] == 369
    spiral = dict(counts=counts, pairs=list(struct.iter_unpack('<hh', table_raw)),
        raw_sha256=sha(table_raw), prior309_raw_sha256=sha(table_raw[:309 * 4]))
    wanted = {'RULESMD.INI','ARTMD.INI','MPBATTLEMD.INI','GOLDST.MMX','URBANMD.INI',
              'SHROUD.SHP','FOG.SHP','ISOURB.PAL','TEMPERAT.PAL','PALETTE.PAL','ANIM.PAL'} | {name.upper() for name in heads}
    all_sources = {source['name'].upper(): source for source in sources + tmp_sources if source['name'].upper() in wanted}
    inputs = dict(sources=list(all_sources.values()), startup=startup, map=map_input,
        source=dict(type='AMCV', coords=list(SOURCE_CELL), native_xyz=f.after_placement['position'],
            type_constructor='007470D0', type_reader='00747620', sight=m.read32(f.typ+0x5E8),
            full_type_layers=f.inputs['full_type_layers'], mission_layers=f.inputs['mission_layers']),
        session=dict(mode=5, human_country='Americans', shroud=True, fog_of_war=False,
            boundary='Supplied eligible single local House/current-player and successful placement; distant enemy/starting-force/world bootstrap excluded.'),
        special_flags=special_flags,
        langrule_absent=not (native.configured_gamemd().parent/'LANGRULE.INI').exists(),
        shroud=dict(source_sha256=sha(assets['SHROUD.SHP']), canvas=list(struct.unpack_from('<HH',assets['SHROUD.SHP'],2)),
            fog_source_sha256=sha(assets['FOG.SHP']), fog_of_war=False),
        scene_lighting=lighting, palette=palette, area_inputs=areas, area_targets=targets,
        height_calls=height_calls, reached=seen,
        clear_files=[dict(name=name,bytes=len(m.assets[name.upper()]),sha256=sha(m.assets[name.upper()])) for name in CLEAR_NAMES])
    assert inputs['langrule_absent']
    packet = dict(surface=dict(width=WIDTH,height=HEIGHT), inputs=inputs, spiral=spiral,
                  histories=[history], rows=rows)
    # Execute independent declared controls only after every central output has
    # been captured. No original admission, A or material prior is changed.
    packet['controls'] = dict(uncached_reveal=uncached_reveal(f, drawing, history))
    return packet


def uncached_reveal(f, drawing, history):
    """Original70ADC0/5673A0, including its already-lifted border argument.

    The same completed GoldSt VM supplies the warm prior. Temporary numeric,
    position and minimal nonallied House inputs are explicitly component
    controls; no expected Cell knowledge/cache or renderer output is written.
    """
    m, u = f.m, f.u
    source_xyz = bytes(u.mem_read(f.src + 0x9C, 12))
    source_owner = m.read32(f.src + 0x21C)
    sight = m.read32(f.typ + 0x5E8)
    height_byte = bytes(u.mem_read(f.src + 0x420, 1))
    foreign = m.alloc(0x16000)
    u.mem_write(foreign + 0x34, dwords(f.country))
    u.mem_write(foreign + 0x30, dwords(m.read32(f.house + 0x30) + 1))
    u.mem_write(foreign + 0x54E4, dwords(0))
    u.mem_write(foreign + 0x5788, dwords(0))
    cases = []
    inputs = (
        dict(name='admitted_empty_height_walk', registered_level_byte=127),
        dict(name='elevated_goldst'),
        dict(name='zero_sight', type_sight=0),
        dict(name='foreign_owner_rejected', owner='supplied_nonallied_house'),
        dict(name='invalid_lifted_center', source_xyz=[128,128,624]),
    )
    for specification in inputs:
        case_input = dict(entry='0070ADC0', args=[0,0],
            source_xyz=list(struct.unpack('<iii', source_xyz)), type_sight=sight,
            owner='current_player')
        case_input.update({k:v for k,v in specification.items() if k != 'name'})
        u.mem_write(f.src + 0x9C, struct.pack('<iii', *case_input['source_xyz']))
        u.mem_write(f.typ + 0x5E8, dwords(case_input['type_sight']))
        u.mem_write(f.src + 0x21C, dwords(foreign if case_input['owner'] != 'current_player' else source_owner))
        levels = {p:bytes(u.mem_read(p + 0x11B, 1)) for p in f.resident.ptrs.values()}
        if 'registered_level_byte' in case_input:
            for p in levels:u.mem_write(p + 0x11B, bytes((case_input['registered_level_byte'],)))
        before, source_before, rng_before = cell_states(f), cache(m, f.src), rng_hashes(f)
        reached, map_calls, border, targets, after_area, before_border = {}, [], [], [], {}, {}
        in_border = False
        def observe(_u, pc, _size, _data):
            nonlocal in_border
            sp = u.reg_read(UC_X86_REG_ESP)
            if pc in (0x70ADC0,0x5673A0,0x653830,0x567DA0,0x6DA7D0):
                reached[f'{pc:08X}'] = reached.get(f'{pc:08X}', 0) + 1
            if pc == 0x5673A0:
                args = ints(u, sp + 4, 8)
                map_calls.append(dict(coords=ints(u,args[0],3), radius=args[1],
                    house_is_current=args[2] == m.read32(0xA83D4C), raw_remaining_args=args[3:]))
            if pc == 0x5678B7:after_area.update(cell_states(f))
            if pc == 0x567DA0:
                args = ints(u, sp + 4, 4)
                border.append(dict(coords=ints(u,args[0],3), height=args[1],radius=args[2],skip=args[3]))
                before_border.update(cell_states(f))
                in_border = True
            if pc == 0x6D8700 and in_border:
                targets.append(list(struct.unpack('<hh', u.mem_read(m.read32(sp + 4), 4))))
        observer = u.hook_add(UC_HOOK_CODE, observe)
        m.invoke(0x70ADC0, f.src, (0,0))
        u.hook_del(observer)
        after = cell_states(f)
        source_after, rng_after = cache(m, f.src), rng_hashes(f)
        assert source_before == source_after and rng_before == rng_after
        area = after_area or before
        row = dict(name=specification['name'], input=case_input,
            source_cache_before=source_before, source_cache_after=source_after,
            source_height_byte=dict(before=height_byte[0], after=u.mem_read(f.src + 0x420,1)[0]),
            rng_unchanged=True, reached=reached, map_calls=map_calls, border=border,
            changed_cells=dict(after_area=[v for xy,v in area.items() if v != before[xy]],
                               after_border=[v for xy,v in after.items() if v != area[xy]]))
        if border:
            assert len(border) == 1
            border[0]['targets'] = targets
            # Compact actual receiver/cache facts, in original visitation order.
            border[0]['cells'] = [[x,y,before_border[x,y]['cache'][0],after[x,y]['cache'][0],
                before_border[x,y]['dirty'],after[x,y]['dirty']] for x,y in targets]
        else:
            assert after == before
        if specification['name'] == 'admitted_empty_height_walk':
            assert border and reached.get('00653830',0) == 0
        if specification['name'] == 'elevated_goldst':
            a_output = drawing.draw_current(dict(full_redraw=True,instruction_limit=12000000,
                timeout_us=120000000), preserve_dirty=True)
            row['draw_output'] = {k:a_output[k] for k in ('alpha_sha256','alpha_hex')}
            if a_output['alpha_hex'] == history['output']['alpha_hex']:
                row['draw_output'] = dict(history='initial_amcv',alpha_sha256=a_output['alpha_sha256'])
            row['after_draw_cells'] = [v for xy,v in cell_states(f).items() if v != after[xy]]
            assert cache(m,f.src) == source_after
        cases.append(row)
        for p,raw in levels.items():u.mem_write(p + 0x11B, raw)
        u.mem_write(f.src + 0x9C, source_xyz)
        u.mem_write(f.src + 0x21C, dwords(source_owner))
        u.mem_write(f.typ + 0x5E8, dwords(sight))
        u.mem_write(f.src + 0x420, height_byte)
    return dict(prior='same central GoldSt VM after initial admission and retained viewport/material draws; cases are sequential',
        surface_transport=dict(owner='bridge_render_shape.prepare_surface',initialize_buffer_height=True,
                               backing_height=drawing.size[1]),
        foreign_house=dict(boundary='supplied minimal House fields, not a full House constructor/bootstrap',
            id=m.read32(foreign+0x30),country='Americans',special_reveal_bits=0,allies=0),cases=cases)


def cached_release(assets, selected_layers):
    """Independent supplied callback control, using the same Mission owner."""
    f = Mission(reader_factory=PaletteReader, code_ranges=((CALLBACK,CALLBACK+0x100),))
    m, u = f.m, f.u
    startup = geometry(f)
    f.resident.phase = 'frontier'
    # Real canonical Cell knowledge, retaining only this owner's supplied
    # Anytown physical material/content/position transport.
    for pointer in f.resident.ptrs.values():
        fields = [(off,bytes(u.mem_read(pointer+off,size))) for off,size in
                  ((0x24,4),(0x38,4),(0x44,4),(0x4C,4),(0xEC,4),(0x11A,6))]
        m.invoke(0x47BBF0,pointer)
        for off,raw in fields:u.mem_write(pointer+off,raw)
    u.mem_map(0x21000000,0x200000)
    nested, calls, border, selected = [], {}, [], {}
    proxy = SimpleNamespace(uc=u,read32=m.read32)
    def state(pointer):return DiscoveryFixture.pointer_state(proxy,pointer,include_dirty=True)
    def observe(_u,pc,_size,_data):
        sp=u.reg_read(UC_X86_REG_ESP)
        if pc==0x5865F0 and not nested:
            pointer=m.read32(sp+4)
            selected.update(pointer=pointer,before_nested=state(pointer))
            nested.append(dict(before=cache(m,f.src)))
            # Supplied callback ABI only; call the unchanged original release
            # then resume the unchanged original notification/outer walk.
            code=b'\x9c\x60'+b'\x6a\x00'*4+b'\xb9'+struct.pack('<I',f.src)
            call_at=CALLBACK+len(code)
            code+=b'\xe8'+struct.pack('<i',0x70B1D0-call_at-5)
            nested[0]['return_hook']=CALLBACK+len(code)
            code+=b'\x61\x9d'
            jump_at=CALLBACK+len(code)
            code+=b'\xe9'+struct.pack('<i',0x5865F0-jump_at-5)
            u.mem_write(CALLBACK,code)
            u.reg_write(UC_X86_REG_EIP,CALLBACK)
            return
        if nested and pc==nested[0].get('return_hook'):
            nested[0]['after']=cache(m,f.src)
            selected['after_nested']=state(selected['pointer'])
        if nested and 'after' not in nested[0] and pc in (0x70B1D0,0x5678E0,0x4A9CA0,0x4A9DD0,0x487690):
            entry=f'{pc:08X}'
            row=calls.setdefault(entry,dict(count=0));row['count']+=1
            if row['count']==1:
                if pc==0x5678E0:
                    args=ints(u,sp+4,8)
                    row.update(radius=args[1],coords=ints(u,args[0],3),by_height=args[-2],final=args[-1])
                elif pc in (0x4A9CA0,0x4A9DD0):
                    row.update(coords=list(struct.unpack('<hh',u.mem_read(m.read32(sp+4),4))),mode=m.read32(sp+12))
                elif pc==0x487690:row['first_cell_before']=state(u.reg_read(UC_X86_REG_ECX))
        if pc==0x567DA0:
            args=ints(u,sp+4,4)
            border.append(dict(cache=cache(m,f.src),coords=ints(u,args[0],3),height=args[1],radius=args[2],skip=args[3]))
    observer=u.hook_add(UC_HOOK_CODE,observe)
    f.setup(source_type_name='AMCV',source_art_bytes=assets['ARTMD.INI'],source_art_names=('AMCV','MCV'),
            source_layer_paths=selected_layers,classify_source_owner=True,stop_after_placement=True)
    selected['after_unlimbo']=state(selected.pop('pointer'))
    after=cache(m,f.src)
    sight=m.read32(f.typ+0x5E8)
    u.mem_write(f.typ+0x5E8,dwords(0))
    m.invoke(0x70AF50,f.src,(0,0,0,0,0))
    gated=cache(m,f.src)
    u.mem_write(f.typ+0x5E8,dwords(sight))
    u.hook_del(observer)
    nested[0].pop('return_hook')
    assert after['latch']==0 and after==gated and after['radius']==6
    assert calls['00487690']['count']==121
    return dict(startup=startup,input=dict(source_cell=[87,50],world='existing Anytown Resident crop',
        layers=['RULESMD.INI','MPBATTLEMD.INI','GOLDST.MMX'],callback='supplied ABI thunk at first5865F0; no stock Tag-cause claim',
        later_type_sight=0),nested=nested,release_calls=calls,border=border,selected_cells=selected,
        after_unlimbo=after,after_sight0_gate=gated,unlimbo=f.placement_result&255)


def generate():
    assets,sources=physical_assets(frontier=True)
    with TemporaryDirectory(prefix='vera-shroud-frontier-') as directory:
        selected=layers(assets,directory)
        packet=joined(assets,sources,selected)
        packet['controls']['cached_release']=cached_release(assets,selected)
    return packet


def metadata():
    return native.provenance(
        scope='Ordinary stationary local AMCV successful Unlimbo through canonical Cell knowledge/cache/notification, full native SHROUD A and current flat Clear TMP RGB565/Z; supplied bounded world and House transport, not native whole scenario startup.',
        assumptions=[
            'One existing Mission/PaletteReader VM owns the central join. Its historical MTNK/type/weapon/Anytown Resident preparation prefix is retained as an explicit inherited fixture boundary. Complete original Discovery/Tactical/Cell startup executes before selected AMCV type/source construction; B0CD48 and ordered Map targets are captured. Source/main selector chronology is not a complete natural starting-force bootstrap.',
            'Physical active RULESMD (expandmd01), ARTMD, MPBATTLEMD and loose GoldSt bytes are pinned; LANGRULE is physically absent. Original AMCV7470D0/747620 and Mission/General/Country readers execute every selected layer. Mission supplies House after original Unit(type,0); the existing original6F3F40..65 InitManagers prefix classifies that owner before Unlimbo, preserving41B/C. Full natural House/constructor chronology is excluded. Native Waypoint3 parser and real Cell486840 derive109,27 and28032,7040,624. Successful737BA0 return/alive/nonlimbo/mark/Logic registration are asserted before drawing.',
            'Physical authored GoldSt crop90,8..128,46 supplies levels/tile/subtile and original TMP getters supply slope/land. Real47BBF0/485240 execute for every enrolled Cell. Registry prefix, ClearTile0 enrollment for authoredFFFF, bounded zone registration, empty live overlays/residents and eligible local House are supplied; native full map Recalc/LAT, point-light creation and distant objects are excluded.',
            'Native70AF50,5678E0,4A9CA0,487630,5865F0/Discover and567DA0 execute synchronously. Before/area/border/full-draw snapshots remain distinct; default FogOfWar=no still updates fog cache. Original561910 supplies all369 offsets/counts; ordinary sight clamps10 while border supports11. Source constructor Scenario random phase remains separate; captured supplied streams are unchanged across selected Unlimbo. Unit stage/timer initialization remains the existing UnitUnlimbo owner; no reveal timer/detach body is invented.',
            'Full6D3660→4801F0/47EFE0 draws cold physical SHROUD and FOG resources on one192x160 native A surface. Camera2634,1870 derives from original projection plus supplied view anchor96,80. A is never rewritten by material setup or drawing. Original6D7560 admits only physical clear111,22 under a supplied one-cell bounds transport; actual lookup, projection,480350/cold CellConvert and547CF0/548D90 execute.',
            'Physical ISOURB source and literal TEMPERAT destination palettes use existing original expansion/LightConvert owners. Explicit normal High detail2, no live point lights/effects and RGB565/MMX gates are bounded inputs. Eight supplied warm selector tables exercise all independently loaded physical clear files. Production chooses its natural variant; this corpus does not certify whole process selector bootstrap.',
            'Independent cached-release control starts the same Mission owner on its Anytown crop with the declared selected AMCV/GoldSt layers. A supplied ABI-only callback invokes original70B1D0 at the first notification then resumes original5865F0/outer traversal; it is not a claim that stock GoldSt naturally triggers a Tag. Actual final1 Cell writes, persistent cache/latch, border args and later supplied Sight0 early gate are retained.',
            'Uncached controls execute original70ADC0/5673A0 in the completed central GoldSt VM after every retained central output. Actual567DA0 receives the local CoordStruct already Z-shifted by5673F8/56742A, and projects Z again. Ordered border receivers and immediate cache/dirty priors/results are retained even when a warm prior needs no additional stores. Zero Sight, supplied minimal nonallied House, invalid lifted-center and registered-level127 empty-walk inputs distinguish rejection from an admitted empty walk. Source250/254/260 and supplied RNG streams remain unchanged. Temporary scalar/position/House priors are restored between sequential cases; no expected knowledge/cache/A is supplied.',
            'The existing prepared-surface owner initializes the circular backing height for this mode, enabling original4112D0 to fill A before each full draw. Legacy prepared-only defaults are unchanged. The uncached followup A redraw uses the same Shroud owner; its output references initial_amcv when byte-identical.',
        ],
        substitutions=[
            'Inherited Mission native-image/CRT/heap/TLS/cached-INI/RawFile and Windows COM/OleRun transports; existing radar-tracker presentation sinks, setup type-visual-asset and wall-clock boundaries, sound-playback sink and existing Resident/House/zone registration boundaries. Measurement phase projection/gameplay substitutions are disabled for this mode.',
            'One-cell568300 bounds admission only for material full-view transport; no Cell knowledge, radius, geometry, raster, palette or original code/vtable byte is replaced. Callback control adds only a supplied out-of-image call-ABI thunk; original notification/release bodies are unchanged.'
        ],
        entry_points=dict(cell_ctor=0x47BBF0,cell_set_coords=0x485240,cell_coords=0x486840,
            waypoint=0x68BDE7,type_ctor=0x7470D0,type_reader=0x747620,unit_ctor=0x7353C0,
            unit_unlimbo=0x737BA0,techno_unlimbo=0x6F6CA0,update_reveal=0x70AF50,
            release=0x70B1D0,area=0x5678E0,cell_reveal=0x4A9CA0,counter_add=0x487630,
            uncached_reveal=0x70ADC0,conditional_area=0x5673A0,
            counter_release=0x487690,border=0x567DA0,notify=0x5865F0,discover=0x6F4960,
            full_shroud=0x6D3660,cell_shroud=0x4801F0,shroud_leaf=0x47EFE0,
            full_tile=0x6D7560,cell_tile=0x480350,tmp=0x547CF0,tmp_leaf=0x548D90))
