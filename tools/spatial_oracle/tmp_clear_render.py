"""Ordinary Cell/TMP raster mode of the shared bridge/ore native caller owner.

No VM, archive parser, TMP decoder, palette kernel or pixel implementation lives
here. Original lazy CCFile/TMP loading, lighting, selector and raster execute.
See tmp_clear_render.md for supplied boundaries.
"""
import hashlib
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import NATIVE_SHA256, RET_MAGIC, configured_gamemd, provenance, run_checked
from tools.spatial_oracle.bridge_shadow_render import (
    prepare as prepare_shared, CELL, POINT, CLIP, WIDTH, HEIGHT, COUNT,
    PIXELS, Z, A, ints, snapshot, physical_assets,
)
from tools.spatial_oracle.building_body_rules import INI, SP, dwords
from tools.spatial_oracle.engineer_repair_admission import NativeAudioPlatform
from tools.spatial_oracle.bridge_recalc_presentation import SELECTOR_TABLE, SELECTOR_FLAG, LOAD_MAIN
from tools.rmg_oracle.gen_rng_vectors import STRUCT_LEN
from tools.rules_oracle.theater_general_reader import (
    TheaterReader, lexical_records, BEGIN, END, PROJECT_BEGIN, PROJECT_END,
)
from tools.projectile_oracle.bridge_render_inputs import lexical
from tools.bridge_click_oracle import MATRIX_INITIALIZER


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def scenario_defaults(m):
    u = m.u
    scenario = m.read32(0xA8B230)
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_EBP, scenario)
    u.reg_write(UC_X86_REG_EBX, 0)
    u.reg_write(UC_X86_REG_EAX, 100)
    run_checked(u, 0x6838D3, 0x6839BD)
    return scenario


def profile(m, raw=None):
    """Original defaults and complete normal Lighting read/conversion block."""
    scenario = scenario_defaults(m)
    u = m.u
    sections = {} if raw is None else lexical(raw, {'Lighting'})[0]
    m.make_ini(sections)
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ESI, scenario)
    u.reg_write(UC_X86_REG_EDI, INI)
    run_checked(u, 0x68A817, 0x68A979, required_addresses=(0x5283D0, 0x7C5F00))
    # Publish Level with the original store, skipping only the following
    # profile's default setup which otherwise leaves an unrelated x87 value.
    run_checked(u, 0x68A984, 0x68A98A)
    offsets = (0x352C, 0x3534, 0x3538, 0x353C, 0x3540, 0x3544)
    keys = ('ambient_percent', 'red_percent', 'green_percent', 'blue_percent', 'ground_units', 'level_units')
    return dict(zip(keys, (struct.unpack('<i', u.mem_read(scenario + off, 4))[0] for off in offsets)))


def load_tmp_files(m, files, *, chain=False, prepared_files=None):
    """Shared original TMP constructor/lazy IO owner; no host TMP decoding.

    A variant chain constructs all descriptors before loading. Independent
    registrations preserve the historical construct/load interleave. These
    are supplied registration boundaries, not a second theater loader.
    Optional prepared_files admit other physical/absent RawFile probes on
    this same transport; they do not register additional TMP descriptors.
    """
    u = m.u
    platform = NativeAudioPlatform(m, configured_gamemd().parent)
    transport_files = dict(files)
    if prepared_files is not None:
        assert not {name.lower() for name in files} & {name.lower() for name in prepared_files}
        transport_files.update(prepared_files)
    platform.configure_transport(prepared_files=transport_files)
    u.hook_add(UC_HOOK_CODE, lambda u, pc, n, d: platform.hook(u, pc, n) if pc != 0x7C9430 else None)
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, 0x4E76E0, 0x4E7716)
    u.mem_write(0xA8ED28, dwords(0x7EB6D4, m.alloc(64), 16, 1, 0, 10))
    heads = {}
    def load(name, head):
        image = m.invoke(0x544CB0, head)
        assert image == m.read32(head + 0xA4) and m.read32(head + 0xA8) & 255
        assert bytes(u.mem_read(image, 16)) == files[name][:16]
    for i, name in enumerate(files):
        head = m.alloc(0x400)
        m.invoke(0x5447C0, head, (0, -65, 0, m.cstring(name[:-4]), chain and i != 0))
        u.mem_write(head + 0x2F4, b'\1' + name.encode() + b'\0')
        heads[name] = head
        if not chain:
            load(name, head)
    if chain:
        values = list(heads.values())
        for i, (name, head) in enumerate(heads.items()):
            u.mem_write(head + 0x2BC, dwords(values[i + 1] if i + 1 < len(values) else 0))
            u.mem_write(head + 0x2F0, dwords(len(values) - i))
            load(name, head)
    return heads, [{k: v for k, v in event.items() if k not in ('returned_bytes_hex', 'args')}
                   for event in platform.file_io]


def theater_tile_files(m, raw, selected_tile, extension):
    """Existing original General/ordinal prefix with supplied registration sum.

    FileName/TilesInSet are native reader outputs; Python accumulates the
    registration prefix, not a second native scalar or ordinal parser.
    """
    u = m.u
    records = lexical_records(raw.decode('latin1'))
    TheaterReader.prepare_records(m, records)
    u.mem_write(SP + 0x38, bytes(u.mem_read(INI, 0x40)))
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, BEGIN, END, required_addresses=(0x5276D0,))
    defined = {name for name, _ in records}
    base, tiles, mapping = 0, {}, None
    for ordinal in range(300):
        section_name = f'TileSet{ordinal:04}'
        if section_name not in defined:
            break
        section = m.cstring(section_name)
        count = m.invoke(0x5276D0, INI, (section, m.cstring('TilesInSet'), 0))
        assert 0 <= count < 1000
        name = m.alloc(64)
        m.invoke(0x528A10, INI, (section, m.cstring('FileName'), m.cstring('TILE'), name, 64))
        filename = m.string(name)
        for n in range(count):
            tiles[base + n] = f'{filename}{n + 1:02}.{extension}'.lower()
        if base <= selected_tile < base + count:
            mapping = dict(section=section_name, base=base, tiles_in_set=count,
                file_name=filename, tile_id=selected_tile,
                boundary='Native INI scalar/string reads; prefix accumulation is the supplied selected theater registration boundary.')
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_EDI, ordinal)
        u.reg_write(UC_X86_REG_EBX, base)
        run_checked(u, PROJECT_BEGIN, PROJECT_END)
        base += count
    assert mapping is not None
    return tiles, mapping


def prepare(*, tmp_names=(), hills=False, prepared_files=None):
    m, _types, inputs = prepare_shared(body=True)
    u = m.u
    # Actual WinMain6BDF5F ->5355D0 ->4E6190 calls both original CPU probes.
    # Their MMX/CMOV bytes govern7DE200 palette construction, independently
    # of the initiallyzero TMP alternate byteABC55C. No emulated flag result
    # or CPU feature policy is supplied by Python.
    for entry in (0x7C5F3D, 0x7C5FFF):
        m.invoke(entry, 0)
    cpu = list(u.mem_read(0x84E860, 5))
    assert cpu[2] == 1, 'This RGB565/MMX corpus requires an MMX-capable emulated CPU'
    assert u.mem_read(0xABC55C, 1) == b'\0'
    assets, sources = physical_assets(tile=True, tmp_names=tmp_names, hills=hills)
    files = {source['name']: assets[source['name'].upper()] for source in sources}
    m.assets.update(assets)
    inputs = dict(sources=sources, cell_convert_startup=inputs['cell_convert'],
        capability_prerequisites=dict(probes=['007C5F3D', '007C5FFF'],
            capability_bytes_84e860=cpu, mmx_address='0084E862', mmx=cpu[2],
            tmp_dispatch_address='00ABC55C', tmp_dispatch=0,
            boundary='Original probes execute in the shared VM on Unicorn CPU; full WinMain is not executed.'))
    # Reuse ordered physical-cache preparation; original56 reads execute in
    # the same renderer VM. No second reader or lexical winner implementation.
    TheaterReader.prepare_records(m, lexical_records(files['temperatmd.ini'].decode('latin1')))
    u.mem_write(SP + 0x38, bytes(u.mem_read(INI, 0x40)))
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, BEGIN, END, required_addresses=(0x5276D0,))
    clear_ordinal = m.read32(SP + 0xC0)
    # Original ClearTile ordinal projection: read actual reader's result slot
    # from the fixed instruction at5455BF, not a retail-value default.
    # Selected TileSet0000 is the first/only loaded type prefix; original
    # General ClearTile read produces ordinal0, so published type base is0.
    inputs['theater'] = dict(name='TEMPERATE', clear_tile_ordinal=clear_ordinal)
    assert clear_ordinal == 0
    u.reg_write(UC_X86_REG_EDI, 0)
    u.reg_write(UC_X86_REG_EBX, 0)
    run_checked(u, PROJECT_BEGIN, PROJECT_END)
    assert m.read32(0xAA10B0) == 0
    section = m.cstring('TileSet0000')
    count = m.invoke(0x5276D0, INI, (section, m.cstring('TilesInSet'), -1))
    filename = m.alloc(64)
    m.invoke(0x528A10, INI, (section, m.cstring('FileName'), m.cstring('TILE'), filename, 64))
    inputs['theater']['tileset'] = dict(section='TileSet0000', tiles_in_set=count,
        file_name=m.string(filename), integer_default=-1, string_default='TILE', string_capacity=64)
    assert count == 1 and m.string(filename) == 'Clear'
    m.phase = 'tmp_loading'
    names = [name for name in files if name.endswith('.tem')]
    _heads, file_io = load_tmp_files(m, {name: files[name] for name in names}, chain=True,
                                   prepared_files=prepared_files)
    inputs['tmp_loader'] = dict(names=names, next_absent='clear01h.tem', constructor='005447C0', lazy='00547020',
        file_io=file_io,
        boundary='Original constructors and full lazy loaders; chain/filename/count metadata supplied from physical contiguous filenames, not full tile-set loader.')
    if tmp_names:
        inputs['tmp_loader']['next_absent'] = None
        inputs['tmp_loader']['boundary'] = 'Original constructor and full lazy loader; explicitly selected physical file registration, not full tile-set loader or alternate-name search.'
    for entry in (0x6D1830, 0x6D18C0, 0x6D1BB0):
        m.invoke(entry, 0)
    profile(m)
    # Generate the persistent process table with actual4814F0/Main draws.
    u.mem_write(CELL + 0x24, struct.pack('<hh', 10, 20))
    m.invoke(0x65C6D0, LOAD_MAIN, (0,))
    u.mem_write(SELECTOR_FLAG, b'\0')
    before = bytes(u.mem_read(LOAD_MAIN, STRUCT_LEN))
    raw_draws = []
    hook = u.hook_add(UC_HOOK_CODE, lambda u, pc, _n, _d:
                     raw_draws.append(u.reg_read(UC_X86_REG_EAX)) if pc == 0x48152B else None)
    selected = m.invoke(0x4814F0, CELL, (0, 8))
    u.hook_del(hook)
    inputs['selector_table'] = ints(u, SELECTOR_TABLE, 64)
    inputs['selector_generation'] = dict(seed=0, coords=[10, 20], tile=0, file_count=8,
        selected_file_index=selected, main_before_sha256=sha(before),
        raw_main_draw_count=len(raw_draws),
        main_after_sha256=sha(bytes(u.mem_read(LOAD_MAIN, STRUCT_LEN))), initialized=u.mem_read(SELECTOR_FLAG, 1)[0])
    return m, inputs, files


def cell_transport(m, pc, cells, *, admission=True):
    """Existing bounded current-Cell lookup/admission fixture transport.

    Coordinates and results come from supplied native-constructed registrations;
    this does not execute Map population or its playfield admission mechanism.
    Shared by the full-view caller and selected native Cell resource consumers.
    """
    if pc not in (0x568300, 0x5657A0) or (pc == 0x568300 and not admission):
        return False
    pointer = m.read32(m.u.reg_read(UC_X86_REG_ESP) + 4)
    xy = struct.unpack('<hh', m.u.mem_read(pointer, 4))
    if pc == 0x5657A0:
        assert xy in cells, xy
        m.ret(cells[xy], 4)
    else:
        m.ret(int(xy in cells), 4)
    return True


def execute(m, case, *, current_cell=None, current_tactical=None, preserve_alpha=False,
            current_cells=None, allow_empty=False):
    """Draw either the legacy prepared Cell or an already-produced live Cell.

    A composed caller supplies its own Cell/Tactical and leaves the native A
    plane intact. The original caller, lighting, selector and raster below are
    shared; only the legacy component enrollment is conditional.
    """
    u = m.u
    cell = CELL if current_cell is None else current_cell
    case = dict(coords=[10, 20], level=0, cell_point=[66, 81], viewport_y=0,
                clip=[0, 0, WIDTH, HEIGHT], old_z=65535, background=65535, a=127,
                repeat=1, type='ClearTile', state=0, detail=2) | case
    if preserve_alpha:
        case.pop('a', None)
        case.pop('a_hex', None)
    scenario = scenario_defaults(m)
    offsets = (0x352C, 0x3534, 0x3538, 0x353C, 0x3540, 0x3544)
    keys = ('ambient_percent', 'red_percent', 'green_percent', 'blue_percent', 'ground_units', 'level_units')
    for offset, key in zip(offsets, keys):
        u.mem_write(scenario + offset, dwords(case['scene_lighting'][key]))
    if current_cell is None:
        u.mem_write(cell + 0x24, struct.pack('<hh', *case['coords']))
        u.mem_write(cell + 0x34, dwords(0))
        u.mem_write(cell + 0x38, dwords(0))
        u.mem_write(cell + 0x48, dwords(-1))
        u.mem_write(cell + 0x11A, bytes((0, case['level'] & 255, 0, 0)))
        u.mem_write(cell + 0x140, dwords(0))
    else:
        assert list(struct.unpack('<hh', u.mem_read(cell + 0x24, 4))) == case['coords']
        assert u.mem_read(cell + 0x11B, 1)[0] == case['level'] & 255
    u.mem_write(POINT, dwords(*case['cell_point']))
    u.mem_write(CLIP, dwords(*case['clip']))
    u.mem_write(0x886FA4, dwords(case['viewport_y']))
    u.mem_write(0xA8EB78, dwords(case['detail']))
    u.mem_write(PIXELS, struct.pack('<H', case['background']) * COUNT)
    u.mem_write(Z, struct.pack('<H', case['old_z']) * COUNT)
    if not preserve_alpha:
        alpha = bytes.fromhex(case['a_hex']) if 'a_hex' in case else bytes([case['a']]) * COUNT
        assert len(alpha) == COUNT
        u.mem_write(A, b''.join(struct.pack('<H', value) for value in alpha))
    alpha_before = bytes(u.mem_read(A, COUNT * 2)) if preserve_alpha else None
    if 'selector_table' in case:
        u.mem_write(SELECTOR_TABLE, dwords(*case['selector_table']))
    u.mem_write(SELECTOR_FLAG, b'\1')
    before = bytes(u.mem_read(LOAD_MAIN, STRUCT_LEN))
    draws, reached, cell_points = [], set(), []
    tactical = current_tactical
    if 'full_view' in case:
        if tactical is None:
            tactical = m.alloc(0x1000)
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_ESI, tactical)
            u.reg_write(UC_X86_REG_EBX, 0)
            run_checked(u, *MATRIX_INITIALIZER)
            u.mem_write(tactical + 0xB0, dwords(*case['full_view']['view_origin']))
            u.mem_write(0x887324, dwords(tactical))
        else:
            assert ints(u, tactical + 0xB0, 2) == case['full_view']['view_origin']
            assert m.read32(0x887324) == tactical
        u.mem_write(0x886FA0, dwords(0))
        u.mem_write(POINT + 0x40, dwords(*case['full_view']['redraw_rect']))
    def observe(_u, pc, _size, _data):
        if pc in (0x6D7560, 0x47FF80, 0x480350, 0x483E30, 0x484180, 0x544E70, 0x4814F0, 0x547CF0, 0x548D90, 0x54980E):
            reached.add(pc)
        if current_cells is not None and cell_transport(m, pc, current_cells):
            return
        if tactical is not None and pc in (0x568300, 0x5657A0):
            pointer = m.read32(u.reg_read(UC_X86_REG_ESP) + 4)
            xy = list(struct.unpack('<hh', u.mem_read(pointer, 4)))
            assert pc != 0x5657A0 or xy == case['coords']
            if current_cell is not None and pc == 0x5657A0:
                return  # the composed world's original lookup owns this Cell
            m.ret(int(xy == case['coords']) if pc == 0x568300 else cell, 4)
            return
        if pc == 0x480350:
            cell_points.append(ints(u, m.read32(u.reg_read(UC_X86_REG_ESP) + 4), 2))
        if pc == 0x547CF0:
            args = ints(u, u.reg_read(UC_X86_REG_ESP) + 4, 17)
            draws.append(dict(x=args[3], y=args[4], clip=args[5:9], level=args[9],
                              common_scalar=args[10], z_enabled=args[11], file_index=args[12], flags=args[13:]))
    hook = u.hook_add(UC_HOOK_CODE, observe)
    for _ in range(case['repeat']):
        u.mem_write(SP, dwords(RET_MAGIC, POINT + 0x40, CLIP) if tactical is not None
                    else dwords(RET_MAGIC, POINT, CLIP, 0))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, tactical if tactical is not None else cell)
        run_checked(u, 0x6D7560 if tactical is not None else 0x480350, RET_MAGIC,
                    count=12000000, timeout_us=120000000)
    u.hook_del(hook)
    if allow_empty and not draws:
        assert not cell_points
        case['cell_point'] = None
    else:
        assert len(cell_points) == case['repeat'] * (len(current_cells) if current_cells is not None else 1) and draws
        case['cell_point'] = cell_points[0]
    assert bytes(u.mem_read(LOAD_MAIN, STRUCT_LEN)) == before
    if preserve_alpha:
        assert bytes(u.mem_read(A, COUNT * 2)) == alpha_before
    convert = m.read32(cell + 0x34)
    assert ints(u, convert + 0x188, 2) == [0xABBED0, 0x885780]
    return dict(name=case['name'], input=case, draws=draws, reached=[f'{pc:08X}' for pc in sorted(reached)],
        output=dict(snapshot(u, case['background'], case['old_z']),
                    selected_file_index=draws[0]['file_index'] if draws else None,
                    cell_normalized_rgb=list(struct.unpack('<3h', u.mem_read(cell + 0x110, 6))),
                    convert_rgb_key=ints(u, convert + 0x198, 3),
                    top_scalar=struct.unpack('<h', u.mem_read(cell + 0x10A, 2))[0],
                    bottom_scalar=struct.unpack('<h', u.mem_read(cell + 0x10E, 2))[0],
                    common_scalar=struct.unpack('<h', u.mem_read(cell + 0x10C, 2))[0]))


def physical_shroud_edge(m, raw, *, point=(66, 81)):
    """Original47EFE0 physical edge in the caller's existing A surface."""
    # Reuse the original shroud leaf in this SAME surface/raster VM. Its
    # physical SHP owner supplies immutable bytes; no second A implementation.
    from tools.procedural_drawing_oracle.shroud import asset
    supplied = raw
    raw, identity = asset()
    assert raw == supplied
    image = m.alloc(len(raw))
    m.u.mem_write(image, raw)
    m.u.mem_write(0x89E7C5, b'\1')
    m.u.mem_write(0x89E794, dwords(image))
    m.u.mem_write(A, struct.pack('<H', 127) * COUNT)
    m.u.mem_write(POINT, dwords(*point))
    m.u.mem_write(CLIP, dwords(0, 0, WIDTH, HEIGHT))
    m.invoke(0x47EFE0, 0, (POINT, CLIP, 15))
    alpha = bytes(value[0] for value in struct.iter_unpack('<H', m.u.mem_read(A, COUNT * 2)))
    assert len(set(alpha)) > 1
    return alpha, dict(source=identity, entry='0047EFE0', frame=15, initial=127,
                       point=list(point), clip=[0, 0, WIDTH, HEIGHT], alpha_sha256=sha(alpha))


def generate():
    m, inputs, files = prepare()
    neutral = profile(m)
    authored = profile(m, files['XMP03T4.MAP'])
    inputs['profiles'] = dict(default=neutral, XMP03T4=authored)
    from tools.spatial_oracle.shrapnel_repair.map_facts import decode_cells
    parsed, cells = decode_cells(files['XMP03T4.MAP'])
    inputs['map'] = dict(name='XMP03T4.MAP', physical_map_section=parsed['Map'],
                         cells=[dict(coords=list(xy), **cells[xy]) for xy in ((64, 31), (66, 34))],
                         boundary='Authored packs only; selected flat/no-smudge live Cell fields are component enrollment, not native map construction.')
    table = inputs['selector_table']
    rows = []
    # Rotations of the original-generated table exercise every independent
    # physical file while the actual4814F0 owner still performs selection.
    for shift in range(8):
        rotated = [(value + shift) % 8 for value in table]
        rows.append(execute(m, dict(name=f'variant_{shift}', selector_table=rotated, scene_lighting=neutral)))
    m.u.mem_write(SELECTOR_TABLE, dwords(*table))
    for level in (2, 6, -1):
        rows.append(execute(m, dict(name=f'level_{level}', level=level, cell_point=[66, 81 + 15 * level], scene_lighting=neutral)))
    rows.append(execute(m, dict(name='a_zero', a=0, scene_lighting=neutral)))
    rows.append(execute(m, dict(name='clipped_edge', clip=[79, 86, 10, 7], scene_lighting=neutral)))
    rows.append(execute(m, dict(name='repeat', repeat=2, scene_lighting=neutral)))
    rows.append(execute(m, dict(name='full_view_XMP03T4_clear', coords=[64, 31], level=6,
        full_view=dict(view_origin=[894, 1254], redraw_rect=[0, 0, WIDTH, HEIGHT]),
        scene_lighting=authored)))
    alpha, inputs['shroud_edge'] = physical_shroud_edge(m, files['SHROUD.SHP'])
    rows.append(execute(m, dict(name='physical_shroud_edge', a_hex=alpha.hex(), scene_lighting=neutral)))
    probe = rows[0]
    x, y, _length, _color, candidate = probe['output']['runs'][0]
    for name, old_z in (('near', candidate + 1), ('equal', candidate), ('far', candidate - 1)):
        row = execute(m, dict(name=f'depth_{name}', old_z=old_z, clip=[x, y, 1, 1],
                             selector_table=probe['input']['selector_table'], scene_lighting=neutral))
        row['depth_probe'] = dict(pixel=[x, y], candidate=candidate, source='original variant_0 raster output')
        rows.append(row)
    # Shared numeric signed-byte boundary; no256 raster replay. Every result
    # comes through the same full original cold lighting owner/cache.
    profile(m)
    m.u.mem_write(CELL + 0x24, struct.pack('<hh', 10, 20))
    light_controls = []
    for raw_level in range(256):
        m.u.mem_write(CELL + 0x34, dwords(0))
        m.u.mem_write(CELL + 0x11B, bytes((raw_level,)))
        m.invoke(0x483E30, CELL, (0, 0x10000, 0, 1000, 1000, 1000))
        top, common, bottom = struct.unpack('<3h', m.u.mem_read(CELL + 0x10A, 6))
        light_controls.append(dict(input=dict(raw_level_byte=raw_level,
            level=struct.unpack('<b', bytes((raw_level,)))[0], scene_lighting=neutral, detail=2),
            output=dict(top_scalar=top, common_scalar=common, bottom_scalar=bottom,
                cell_normalized_rgb=list(struct.unpack('<3h', m.u.mem_read(CELL + 0x110, 6))),
                convert_rgb_key=ints(m.u, m.read32(CELL + 0x34) + 0x198, 3))))
    return dict(native_sha256=NATIVE_SHA256, inputs=inputs,
                surface=dict(width=WIDTH, height=HEIGHT, baseline=32768, bytes_per_pixel=2), rows=rows,
                light_height_scene_lighting=neutral, light_height_controls=light_controls)


def metadata():
    return provenance(scope='Original ordinary temperate flat ClearTile Cell480350 and TMP547CF0 raster on eight physical files; cold Cell light gathering, native Main selector and exact RGB565/Z output. No native whole map bootstrap claim.',
        assumptions=[
            'Shared PaletteReader VM, physical archive extraction, palette expansion/Convert construction and RGB565/Z/A surface owner are reused. TMP constructors and full lazy CCFile reader/relocation/radar rebuild/destruction execute; linked variant filename/count metadata is supplied from the physical contiguous family.',
            'Original Scenario defaults6838D3..6839BD, normal map Lighting reader68A817..68A979 and Level publication68A984..68A98A execute. Full cold483E30/484180/544E70 obtains the actual ISO-source Cell LightConvert. Detail2 is supplied; Cell normalized RGB and actual quantized Convert key are recorded separately. Empty local-light/effect registries and RGB565/MMX startup dispatch remain declared component inputs.',
            'Original4814F0 initializes its64-entry persistent table from Main seeded0. Warm-table rotations are supplied inputs for physical-file coverage; warm calls must leave Main unchanged. Coordinates, signed levels, no-smudge state, screen point, viewport, clip and initial A/Z/color are supplied component state.',
            'Original native raster creates every expected pixel/depth run. No Rust values, decoded host pixels or host lighting/projection/depth formula generate goldens. Original CPU probes7C5F3D/7C5FFF execute in the shared VM before TMP; MMX84E862 must be1 and independent TMP dispatchABC55C must remain0. WinMain6BDF5F→5355D0→4E6190 supplies the active startup caller chain; full WinMain and other TMP branches are not executed.',
            'One full6D7560 row enrolls only64,31 from physical XMP03T4 packs atlevel6. Map568300/5657A0 transport admits exactly that cell; original inverse projection, traversal,47FF80, rectangle intersection, ground projection and480350/raster execute. Authored inputs are not native Recalc/map construction. Physical SHROUD.SHP frame15 executes47EFE0 in the same VM to produce one captured A input; reveal scheduling remains outside scope.',
            'All256 raw level bytes execute full cold483E30 under the default no-source profile, recording normalized RGB, effective Convert key and top/common/bridge scalars. These are supplied signed-height numeric boundaries, not proof that retail map loading produces negative heights. Repeated rows retain actual A/Z/color across their second original call; no timer or detach body is passed by this selected draw chain.',
        ], substitutions=['Existing allocator/TLS and RawFile Windows byte transport; immutable physical archive winners. Original Cell/TMP/lighting/raster bodies are not substituted.'],
        entry_points=dict(cell=0x480350, cell_cold_light=0x483E30, gather=0x484180,
                          convert_select=0x544E70, tmp_get_image=0x544CB0, tmp_loader=0x547020,
                          tmp=0x547CF0, scalar=0x548D90, selector=0x4814F0,
                          normal_lighting_begin=0x68A817, normal_lighting_end=0x68A98A,
                          full_view=0x6D7560, tile_bounds=0x47FF80, shroud_leaf=0x47EFE0,
                          winmain_cpu_profile_call=0x6BDF5F, cpu_profile=0x5355D0,
                          cpu_dispatch=0x4E6190, cpu_mmx=0x7C5F3D, cpu_cmov=0x7C5FFF))
