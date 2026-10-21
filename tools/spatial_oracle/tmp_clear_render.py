"""Ordinary Cell/TMP raster mode of the shared bridge/ore native caller owner.

No VM, archive parser, TMP decoder, palette kernel or pixel implementation lives
here. Original lazy CCFile/TMP loading, lighting, selector and raster execute.
See tmp_clear_render.md for supplied boundaries.
"""
import hashlib
import struct
from pathlib import Path

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FPCW,
)
from tools.native_oracle import RET_MAGIC, NativeCallTrace, configured_gamemd, finish_vectors, provenance, run_checked
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


def load_tmp_files(m, files, *, chain=False, prepared_files=None, variant_families=None,
                   tile_ids=None):
    """Shared original TMP constructor/lazy IO owner; no host TMP decoding.

    A variant chain constructs all descriptors before loading. Independent
    registrations preserve the historical construct/load interleave. These
    are supplied registration boundaries, not a second theater loader.
    Optional prepared_files admit other physical/absent RawFile probes on
    this same transport; they do not register additional TMP descriptors.
    Optional variant_families groups selected physical files on this same
    transport; other files retain independent construction/loading.
    Optional tile_ids supplies each registration's native constructor first
    argument (5447D3/D9 -> TileType+294). Raster callers retain historical0;
    materialization callers use identities from the native theater reader.
    """
    tile_ids = {} if tile_ids is None else dict(tile_ids)
    if not set(tile_ids) <= set(files):
        raise ValueError('TMP identities must name registered physical files')
    if any(type(value) is not int or not 0 <= value <= 0x7FFFFFFF
           for value in tile_ids.values()):
        raise ValueError('TMP registration identities must be nonnegative signed32 integers')
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
    variant_families = variant_families or {}
    assert not chain or not variant_families
    members = [name for family in variant_families.values() for name in family]
    assert len(members) == len(set(members)) and set(members) <= set(files)
    assert all(family and family[0] == base for base, family in variant_families.items())
    groups = [list(files)] if chain else [variant_families.get(name, [name])
                                         for name in files if name not in members or name in variant_families]
    for group in groups:
        linked = chain or group[0] in variant_families
        for i, name in enumerate(group):
            head = m.alloc(0x400)
            m.invoke(0x5447C0, head, (tile_ids.get(name, 0), -65, 0,
                                   m.cstring(name[:-4]), linked and i != 0))
            u.mem_write(head + 0x2F4, b'\1' + name.encode() + b'\0')
            heads[name] = head
            if not linked:
                load(name, head)
        if not linked: continue
        values = [heads[name] for name in group]
        for i, name in enumerate(group):
            head = heads[name]
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


def prepare(*, tmp_names=(), hills=False, prepared_files=None, native_format_masks=False):
    m, _types, inputs = prepare_shared(body=True, native_format_masks=native_format_masks)
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
    return dict(inputs=inputs,
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


def generate_preview():
    """Original TMP height tables and selected Cell preview, in the same VM.

    No interpolation, RGB packing or expected level-table implementation is
    present here. Physical registration/lazy loading stays with prepare().
    """
    m, inputs, files = prepare()
    u = m.u
    # Native primary-surface mask derivation, on the existing owner's declared
    # enrolled RGB565 descriptor masks. This is not universal Windows startup.
    descriptor_masks = [0xF800, 0x07E0, 0x001F]
    u.mem_write(0x8A0958, dwords(*descriptor_masks))
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, 0x4BA9CD, 0x4BAA89,
                required_addresses=(0x4BA9EE, 0x4BAA27, 0x4BAA61, 0x4BAA83))
    format_globals = ints(u, 0x8A0DD0, 6)
    assert format_globals == [11, 3, 0, 3, 5, 2]
    inputs['surface_format'] = dict(descriptor_masks=descriptor_masks,
        shift_loss_globals=format_globals, derivation=['004BA9CD', '004BAA89'],
        boundary='Existing PaletteReader RGB565 enrollment; original mask derivation executes on those declared device masks, not a full DirectDraw device negotiation.')
    inputs['numeric_environment'] = dict(fpcw=f'{u.reg_read(UC_X86_REG_FPCW):04x}',
        boundary='Actual shared native VM x87 control word; no full WinMain floating-point startup claimed.')
    registry = m.read32(0xA8ED2C)
    head = m.read32(registry)
    assert head and m.read32(0xAA10B0) == 0
    names = inputs['tmp_loader']['names']
    assert names[0] == 'clear01.tem'
    assert m.string(head + 0x2F5).lower() == names[0]
    image = m.read32(head + 0xA4)
    assert image and bytes(u.mem_read(image, 16)) == files[names[0]][:16]
    source = m.read32(image + 16)
    assert source
    pair_rgb = [list(u.mem_read(source + offset, 3)) for offset in (0x2B, 0x2E)]
    inputs['selected_tmp'] = dict(tile=0, subtile=0, name=names[0],
        physical_sha256=sha(files[names[0]]), dimensions=ints(u, image, 2),
        raw_pair_rgb=pair_rgb, registered_file_count=m.read32(head + 0x2F0),
        boundary='Original theater General/TileSet0000 reads and constructors publish the selected ClearTile0 registry head; original lazy loader binds physical bytes. Contiguous family links remain the existing supplied registration boundary.')
    # Main is the actual warm selector continuation; the other two streams
    # receive declared native seeds. Snapshot all bytes, not only cursors.
    streams = dict(main=LOAD_MAIN, scenario=m.read32(0xA8B230) + 0x218, mapgen=0xABE890)
    for name in ('scenario', 'mapgen'):
        m.invoke(0x65C6D0, streams[name], (0,))
    before = {name: bytes(u.mem_read(address, STRUCT_LEN)).hex()
              for name, address in streams.items()}
    reached, draws, lookups = set(), [], []
    phase = ['producer']
    trace = NativeCallTrace(u, m.read32)
    produced_colors, brightened_pair, particle_returns = [], [], []
    def observe(uc, pc, _size, _data):
        if pc in (0x549E90, 0x549E50, 0x47BDB0, 0x661020, 0x661190, 0x544E00, 0x4814F0):
            reached.add(pc)
        sp = uc.reg_read(UC_X86_REG_ESP)
        for index in trace.returned(pc, sp):
            call = trace.calls[index]
            rgb = list(u.mem_read(call['return_eax'], 3))
            if call['name'] == 'scale':
                brightened_pair.append(rgb)
            elif call['name'] == 'particle':
                particle_returns.append(dict(fraction_f32_bits=f'{call["args"][2]:08x}', rgb=rgb))
            else:
                produced_colors.append(dict(level=call['level'],
                    side='left' if call['caller'] == '0x549fd1' else 'right',
                    fraction_f32_bits=f'{call["args"][2]:08x}',
                    left_rgb=call['left_rgb'], right_rgb=call['right_rgb'], rgb=rgb))
        if phase[0] == 'producer' and pc in (0x661190, 0x661020):
            index = trace.entered(pc, sp,
                ('scale', 2, 8) if pc == 0x661190 else ('weighted', 3, 12))
            call = trace.calls[index]
            if pc == 0x661020:
                assert call['caller'] in ('0x549fd1', '0x54a03c')
                call.update(level=uc.reg_read(UC_X86_REG_EBX),
                    left_rgb=list(u.mem_read(call['args'][0], 3)),
                    right_rgb=list(u.mem_read(call['args'][1], 3)))
        elif phase[0] == 'particle' and pc == 0x661020:
            trace.entered(pc, sp, ('particle', 3, 12))
        if pc in (0x65C780, 0x65C7E0, 0x65C837):
            draws.append(hex(pc))
        if pc == 0x549E50:
            lookups.append(dict(subtile=m.read32(sp + 4), level=m.read32(sp + 8),
                                selected_name=m.string(uc.reg_read(UC_X86_REG_ECX) + 0x2F5)))
    hook = u.hook_add(UC_HOOK_CODE, observe)
    try:
        m.invoke(0x549E90, head)
        phase[0] = 'consumers'
        assert len(brightened_pair) == 2 and len(produced_colors) == 26
        assert not trace.pending
        count = m.read32(head + 0x2B4)
        assert count == 1
        table = m.read32(m.read32(head + 0x2A8))
        assert table
        packed_table = bytes(u.mem_read(table, 13 * 4)).hex()
        # The original warm selector supplies the selected base file. Search
        # declared small coordinates; never force its answer or rewrite table.
        coords = None
        for y in range(8):
            for x in range(8):
                u.mem_write(CELL + 0x24, struct.pack('<hh', x, y))
                if m.invoke(0x4814F0, CELL, (0, len(names))) == 0:
                    coords = [x, y]
                    break
            if coords is not None:
                break
        assert coords is not None
        u.mem_write(CELL + 0x38, dwords(0))
        u.mem_write(CELL + 0x44, dwords(-1))
        u.mem_write(CELL + 0xE4, dwords(0))
        u.mem_write(CELL + 0x140, dwords(0))
        marker = m.alloc(4)
        rows = []
        for level in range(13):
            pointer = m.invoke(0x549E50, head, (0, level))
            words = list(struct.unpack('<2H', u.mem_read(pointer, 4)))
            u.mem_write(CELL + 0x11A, bytes((0, level)))
            u.mem_write(marker, b'\xFF' * 4)
            start = len(lookups)
            packed = m.invoke(0x47BDB0, CELL, (marker, 0))
            assert lookups[start:] == [dict(subtile=0, level=level, selected_name=names[0])]
            assert u.mem_read(marker, 1) == b'\0'
            colors = [row for row in produced_colors if row['level'] == level]
            assert [row['side'] for row in colors] == ['left', 'right']
            rows.append(dict(input=dict(coords=coords, tile=0, subtile=0, level=level,
                raw_flags=0, overlay=-1, ground_first_object=0, bypass_building=0),
                output=dict(preview_rgb_pair=[row['rgb'] for row in colors],
                    fraction_f32_bits=colors[0]['fraction_f32_bits'],
                    lookup_words=words, cell_packed=packed, marker=0)))
        # Numeric inputs are supplied controls. Every output executes the
        # unchanged original kernel, including clamp and ftol instructions.
        color_a, color_b, output = (m.alloc(16) for _ in range(3))
        scale = []
        scale_bits = m.read32(0x7ECBD4)
        for rgb, factor in ((pair_rgb[0], scale_bits), ([181, 182, 183], scale_bits),
                            ([162, 168, 80], 0x3F800000), ([80, 162, 255], 0x3F4CCCCD),
                            ([0, 1, 255], 0x3F000000), ([255, 128, 1], 0xBF800000)):
            u.mem_write(color_a, bytes(rgb))
            pointer = m.invoke(0x661190, output, (color_a, factor))
            assert pointer == output
            scale.append(dict(input=dict(rgb=rgb, factor_f32_bits=f'{factor:08x}'),
                              output_rgb=list(u.mem_read(output, 3))))
        interpolation = []
        for left, right, fraction in ((pair_rgb[0], brightened_pair[0], 0x3F000000),
                ([0, 1, 255], [255, 0, 0], 0x3F000000),
                ([1, 2, 3], [4, 5, 6], 0x3EAAAAAA),
                ([1, 2, 3], [4, 5, 6], 0x3EAAAAAB),
                ([0, 1, 255], [255, 0, 0], 0xBF000000),
                ([0, 1, 255], [255, 0, 0], 0x3FC00000)):
            u.mem_write(color_a, bytes(left))
            u.mem_write(color_b, bytes(right))
            pointer = m.invoke(0x661020, output, (color_a, color_b, fraction))
            assert pointer == output
            interpolation.append(dict(input=dict(left_rgb=left, right_rgb=right,
                fraction_f32_bits=f'{fraction:08x}'), output_rgb=list(u.mem_read(output, 3))))
        particle, particle_type = m.alloc(0x180), m.alloc(0x400)
        start_1, start_2 = [80, 255, 255], [255, 255, 100]
        u.mem_write(particle + 0xAC, dwords(particle_type))
        u.mem_write(particle_type + 0x2D4, bytes(start_1 + start_2))
        particle_controls = []
        phase[0] = 'particle'
        for name, sample in (('zero', 0), ('one', 1), ('mid', 1073741823),
                             ('upper', 2147483646), ('welding_first_produced', 19015573)):
            # Boundary is AFTER native RandomRanged returns. Actual FILD,
            # double multiply, narrowing FSTPfloat, 661020 and Particle stores
            # execute. No RNG return or caller is hooked/substituted.
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_ESI, particle)
            u.reg_write(UC_X86_REG_EAX, sample)
            run_checked(u, 0x62BAC5, 0x62BB06,
                        required_addresses=(0x62BADF, 0x62BAE5, 0x661020, 0x62BB00))
            returned = particle_returns[-1]
            rgb = list(u.mem_read(particle + 0xB0, 3))
            assert rgb == returned['rgb']
            particle_controls.append(dict(input=dict(name=name, sample=sample,
                start_color1=start_1, start_color2=start_2),
                output=dict(fraction_f32_bits=returned['fraction_f32_bits'], start_rgb=rgb)))
        assert particle_controls[-1]['output']['start_rgb'] == [81, 255, 253]
        assert not trace.pending
    finally:
        u.hook_del(hook)
    after = {name: bytes(u.mem_read(address, STRUCT_LEN)).hex()
             for name, address in streams.items()}
    assert before == after and not draws
    assert reached >= {0x549E90, 0x549E50, 0x47BDB0, 0x661020, 0x661190, 0x544E00, 0x4814F0}
    return dict(inputs=inputs,
        producer=dict(entry='00549E90', subtile_count=count, levels=13,
            bytes_per_level=4, table_hex=packed_table, brightness_f32_bits=f'{scale_bits:08x}',
            fraction_multiplier_f64_bits=f'{struct.unpack("<Q", u.mem_read(0x7ECD28, 8))[0]:016x}',
            brightened_pair_rgb=brightened_pair, interpolation_returns=produced_colors),
        rows=rows, scale_controls=scale, interpolation_controls=interpolation,
        particle_color_controls=particle_controls,
        rng=dict(before=before, after=after, calls=draws,
            boundary='Main follows native seed0/128 selector draws; Scenario and MapGen native seeds0. Full preview/table/numeric calls leave all three complete states unchanged.'),
        native_entries=sorted(f'{pc:08X}' for pc in reached))


def preview_metadata():
    return provenance(scope='Original physical temperate ClearTile0 TMP preview level table549E90, lookup549E50 and complete selected no-Building/no-overlay Cell47BDB0 path for levels0..12; original scale661190/interpolation661020 controls. No whole RMG generation, thumbnail surface, World startup or GPU claim.',
        assumptions=[
            'Existing tmp_clear_render prepare() shares PaletteReader, physical archive/theater registration readers, original TMP constructors/full lazy loader and warm native selector. File-family links/counts are the existing supplied registration metadata; selected actual clear01.tem bytes and native registered head/header are checked together.',
            'Existing RGB565 enrollment is conditional device input, not intrinsic native format. Original4BA9CD..4BAA89 derives shifts/losses from declared masksF800/07E0/001F; full DirectDraw descriptor negotiation is outside this component.',
            'Full original549E90 frees/rebuilds the native13x4-byte table from physical TMP RGB pairs and original661190/661020, with native x87 state and ftol. All expected words/RGB are original output, no copied host math or packing.',
            'Cell coordinate, tile0, subtile0, rawlevel0..12, rawflags0, overlay-1, empty ground list and bypass-building0 are declared component inputs. Original Cell body traverses its real no-Building/no-bridge branch, native warm selector/file walk and height lookup; no Cell getter is replaced.',
            'Numeric clamp/rounding RGB and rawf32 inputs are supplied controls. Main warm selector continuation and separately native-seeded Scenario/MapGen full states are captured before/after all preview work. No full-game RNG-startup equivalence follows.',
            'Particle color controls start after the declared RandomRanged result at62BAC5, executing original sample double scale/FSTPfloat/661020/output stores through62BB06 without RNG interception. The first retained Welding sample19015573 and typeRGB inputs are replayed numerically, not through a new Particle/world fixture.',
        ], substitutions=['Existing bounded allocator/free/TLS/CRT and immutable RawFile Windows byte transport only; native table/color/Cell/selector instructions execute unchanged.'],
        entry_points=dict(table=0x549E90, lookup=0x549E50, cell=0x47BDB0,
            scale=0x661190, interpolate=0x661020, mask_derivation=0x4BA9CD,
            tmp_constructor=0x5447C0, tmp_loader=0x547020, selector=0x4814F0,
            file_walk=0x544E00, particle_color_prefix=0x62BAC5))


def generate_partial_redraw(*, detail=False):
    """Observe dirty maintenance and terrain in the existing surface/VM owner."""
    from tools import native_oracle as native
    from tools.native_inspect import section_rows
    from tools.projectile_oracle import bridge_render_shape as surfaces
    from tools.procedural_drawing_oracle.shroud import Shroud, _prepare_flat_cells, asset
    from tools.spatial_oracle.bridge_recalc_presentation import LOAD_MAPGEN

    m, inputs, files = prepare()
    u, lighting = m.u, profile(m)
    raw = native.image_bytes()
    executable = [r for r in section_rows(raw) if r['flags'] & 0x20000000]

    def original_code():
        for r in executable:
            expected = raw[r['file_offset']:r['file_offset'] + r['file_bytes']]
            assert bytes(u.mem_read(r['address'], len(expected))) == expected

    # Reuse native Cell construction and sparse Map enrollment. The flat scene,
    # knowledge and camera are declared component priors, not a map/reveal load.
    _table, pointers = _prepare_flat_cells(m, range(32), range(32))
    cells = {struct.unpack('<hh', u.mem_read(p + 0x24, 4)): p for p in pointers}
    real_pointers = frozenset(pointers)
    map_size = ints(u, 0x87F7E8 + 0xF4, 2)
    assert map_size == [16, 16] and len(cells) == 1024
    m.invoke(0x47BBF0, 0xABDC50)
    scene = []
    for xy, p in sorted(cells.items(), key=lambda item: (item[0][1], item[0][0])):
        flags = 0x18 if 8 <= xy[0] <= 12 and 18 <= xy[1] <= 22 else 0
        u.mem_write(p + 0x12C, dwords(flags))
        scene.append(dict(coords=list(xy), tile_id=0, sub_tile=0, level=0, flags=flags))
    tactical = m.alloc(0x1000)
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ESI, tactical)
    u.reg_write(UC_X86_REG_EBX, 0)
    run_checked(u, *MATRIX_INITIALIZER)
    camera = [-396, 354]
    u.mem_write(tactical + 0xB0, dwords(*camera))
    u.mem_write(0x887324, dwords(tactical))
    # Only the nonzero HWND test is reached; this is not a native window.
    u.mem_write(0xB73550, dwords(1))
    for p in (0x87F7F4, 0xB0CE88, 0x88A100):
        u.mem_write(p, dwords(0))
    u.mem_write(0x886FA0, dwords(0, 0, WIDTH, HEIGHT))
    u.mem_write(0xB0CE30, dwords(WIDTH, HEIGHT))
    u.mem_write(0xB0CD48, struct.pack('<Q', 0x3FC25E5374344960))
    streams = dict(main=LOAD_MAIN, scenario=m.read32(0xA8B230) + 0x218, mapgen=LOAD_MAPGEN)
    assert m.read32(m.read32(0xA8B230)) & 0x1000 == 0
    for name in ('scenario', 'mapgen'):
        m.invoke(0x65C6D0, streams[name], (0,))

    def rng():
        return {name: bytes(u.mem_read(p, STRUCT_LEN)).hex() for name, p in streams.items()}

    shp, identity = asset()
    assert shp == files['SHROUD.SHP']
    image = m.alloc(len(shp))
    u.mem_write(image, shp)
    u.mem_write(0x89E794, dwords(image))
    u.mem_write(0x89E7C5, b'\1')
    drawing = Shroud.attach(m, size=(WIDTH, HEIGHT), tactical=tactical,
                           abuffer=surfaces.AOBJ, alpha=A, cells=cells)
    zero, clip = m.alloc(16), m.alloc(16)
    u.mem_write(zero, bytes(16))
    target = cells[10, 20]
    states, alphas, histories, state_refs, alpha_refs = [], [], [], {}, {}
    phase, calls, requests = 'setup', [], []

    def queue():
        n = m.read32(tactical + 0xE0)
        assert n <= 799
        reverse = {p: list(xy) for xy, p in cells.items()}
        refs = ints(u, tactical + 0xE4, n)
        assert all(p in reverse for p in refs)
        return dict(cells=[reverse[p] for p in refs], count=n,
                    redraw=u.mem_read(tactical + 0xD7D, 1)[0],
                    cell_frame=m.read32(target + 0x5C),
                    cell_dirty=u.mem_read(target + 0x138, 1)[0])

    def capture(name):
        state = snapshot(u, 0, 65535)
        key = state['color_sha256'], state['depth_sha256']
        if key not in state_refs:
            state_refs[key] = len(states)
            states.append(state)
        alpha = drawing.alpha()
        key_a = sha(alpha)
        if key_a not in alpha_refs:
            alpha_refs[key_a] = len(alphas)
            alphas.append(dict(hex=alpha.hex(), sha256=key_a))
        return dict(name=name, surface_ref=state_refs[key], alpha_ref=alpha_refs[key_a],
                    queue=queue(), rng=rng())

    entries = {0x6DA7D0, 0x6D2B60, 0x480180, 0x6D3660, 0x6D2DE0,
               0x6D7560, 0x6D7F20, 0x480350, 0x547CF0}

    def observe(_u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc in (0x65C780, 0x65C7E0):
            requests.append(dict(phase=phase, entry=f'{pc:08X}', caller=f'{m.read32(sp):08X}',
                                 stream=u.reg_read(UC_X86_REG_ECX)))
        if pc not in entries:
            return
        row = dict(phase=phase, entry=f'{pc:08X}', caller=f'{m.read32(sp):08X}')
        if pc == 0x547CF0:
            args = ints(u, sp + 4, 17)
            row.update(x=args[3], y=args[4], clip=args[5:9], level=args[9],
                       common_scalar=args[10], z_enabled=args[11],
                       file_index=args[12], flags=args[13:])
        elif pc in (0x480180, 0x480350):
            point, rectangle = ints(u, sp + 4, 2)
            p = u.reg_read(UC_X86_REG_ECX)
            assert p in real_pointers, f'{pc:08X} drew an unenrolled Cell {p:08X}'
            row.update(cell=list(struct.unpack('<hh', u.mem_read(p + 0x24, 4))),
                       point=ints(u, point, 2), clip=ints(u, rectangle, 4))
        calls.append(row)

    hook = u.hook_add(UC_HOOK_CODE, observe)

    def draw(full):
        nonlocal phase
        phase = 'shroud'
        m.invoke(0x6D3660, tactical, (zero, zero, clip, int(full)),
                 instruction_limit=12_000_000, timeout_us=120_000_000)
        phase = 'terrain'
        m.invoke(0x6D2DE0, tactical, (zero, zero, int(full)),
                 instruction_limit=12_000_000, timeout_us=120_000_000)

    def drain():
        # Execute original unconditional stores; unrelated upper-object/shell
        # portions of Tactical.Draw are outside this component.
        u.reg_write(UC_X86_REG_EBP, tactical)
        run_checked(u, 0x6D4571, 0x6D457B)
        run_checked(u, 0x6D4B37, 0x6D4B3E)
        assert queue()['count'] == queue()['redraw'] == 0

    try:
        original_code()
        for ordinal, (name, full, dirty, rectangle) in enumerate((
                ('no_dirty', False, False, [0, 0, WIDTH, HEIGHT]),
                ('incremental', False, True, [0, 0, WIDTH, HEIGHT]),
                ('full_redraw', True, True, [0, 0, WIDTH, HEIGHT]),
                ('clipped', False, True, [80, 84, 40, 20]))):
            # Each valid prior is produced natively, never restored from an
            # expected snapshot. All subsequent calls retain RGB, Z and A.
            surfaces.prepare_surface(u, dict(depth_baseline=32768, background=0,
                old_z=65535, a_value=127), size=(WIDTH, HEIGHT),
                initialize_buffer_height=True)
            assert queue()['count'] == 0
            u.mem_write(clip, dwords(0, 0, WIDTH, HEIGHT))
            u.mem_write(0xA8ED84, dwords(2 * ordinal))
            draw(True)
            drain()
            assert len(set(drawing.alpha())) > 1, 'native physical A must be nonuniform'
            phases = [capture('prior')]
            assert states[phases[0]['surface_ref']]['runs']
            calls.clear()
            requests.clear()
            u.mem_write(0xA8ED84, dwords(2 * ordinal + 1))
            phase = 'enqueue'
            before = queue()
            if dirty:
                m.invoke(0x6DA7D0, tactical, (target,))
                assert queue()['cells'] == [[10, 20]], queue()
            after = queue()
            u.mem_write(clip, dwords(*rectangle))
            phase = 'maintenance'
            m.invoke(0x6D2B60, tactical, (int(full), 0, 0, clip),
                     instruction_limit=12_000_000, timeout_us=120_000_000)
            phases.append(capture('after_maintenance'))
            special = [r for r in calls if r['phase'] == phase and r['entry'] == '00547CF0']
            assert bool(special) == (dirty and not full)
            if special:
                assert all(r['flags'] == [0, 1, 0, 0] for r in special)
                prior, maintained = (states[p['surface_ref']] for p in phases)
                assert prior['color_sha256'] == maintained['color_sha256']
                assert phases[0]['alpha_ref'] == phases[1]['alpha_ref']
                assert prior['depth_sha256'] != maintained['depth_sha256']
            draw(full)
            phases.append(capture('after_terrain'))
            phase = 'cleanup'
            drain()
            phases.append(capture('after_cleanup'))
            assert phases[0]['rng'] == phases[-1]['rng'] and not requests
            original_code()
            histories.append(dict(name=name, full_redraw=full, enqueue=dirty,
                maintenance_clip=rectangle, enqueue_before=before, enqueue_after=after,
                phases=phases, calls=list(calls), rng_requests=list(requests)))
    finally:
        u.hook_del(hook)
    data = dict(schema_version=1, inputs=inputs,
        scene=dict(cells=scene, map_size=map_size, registration_bounds=[0, 0, 32, 32],
                   every_draw_receiver_real=True,
                   camera=camera, lighting=lighting, native_shroud=identity,
                   selector_table=inputs['selector_table']),
        surface=dict(width=WIDTH, height=HEIGHT, baseline=32768,
                     background=0, old_z=65535, bytes_per_pixel=2),
        surface_states=states, alpha_planes=alphas, histories=histories,
        original_executable_sections_unchanged=True)
    return data if detail else partial_redraw_projection(data)


def partial_redraw_projection(data):
    """Pure observed-data selection; the native producer is unchanged.

    Rust reconstructs prior/final frames, never an intermediate dirty diamond.
    Keep their full lossless runs and the intermediate native hashes. Intern
    repeated complete RNG states rather than writing them at every boundary.
    """
    retained = {phase['surface_ref'] for history in data['histories']
                for phase in history['phases'] if phase['name'] != 'after_maintenance'}
    states, refs, histories = [], {}, []
    for history in data['histories']:
        phases = []
        for phase in history['phases']:
            rng = {}
            for name, raw in phase['rng'].items():
                if raw not in refs:
                    refs[raw] = len(states)
                    states.append(raw)
                rng[name] = refs[raw]
            phases.append(phase | dict(rng=rng))
        histories.append(history | dict(phases=phases))
    surfaces = [{k: v for k, v in surface.items() if k != 'runs' or i in retained}
                for i, surface in enumerate(data['surface_states'])]
    return data | dict(schema_version=2, histories=histories,
                       surface_states=surfaces, rng_streams=states)


def partial_redraw_metadata():
    return provenance(
        scope='Original6DA7D0 ->6D2B60/480180 ->6D3660 ->6D2DE0/6D7F20 retained RGB565/Z/A on physical ClearTile0.',
        assumptions=[
            'Existing PaletteReader, physical TMP/palette/SHROUD loaders, native Cell constructors/Map lookups/projection and BSurface/circular planes are reused. All1024 Cells in the declared32x32 registration are real; every prior/measured terrain receiver is checked by address. Map Size16x16 is separately retained, not inferred from draw calls. Flat supplied knowledge/geometry, camera and nonzero HWND are component priors, not native map/window or arrival/reveal bootstrap.',
            'Original full drawing generates valid priors from black/FFFF/127. Subsequent enqueue/maintenance/shroud/terrain calls retain all three planes. No dirty result, raster output or callback return is supplied.',
            'Unconditional original cleanup stores6D4571/6D4B37 execute separately; unrelated upper-object/shell paths are excluded. No custom/no-Z first image, other theaters, Fog/AlphaShapes, scrolling or universal reconstruction claim.',
            'Surface/A interning selects observed native bytes only. All three complete RNG streams are recorded unchanged across the measured transition; Main retains its native warm selector table.',
            'Default schema2 retains complete prior/final surface runs, intermediate RGB/Z hashes and interned complete RNG states. It is a pure projection of the same original execution. --partial-detail preserves the full schema1 trace in external scratch.',
        ], substitutions=['Existing shared allocator/CRT/file/cache/platform and BSurface transport only; original executable sections remain byte-identical.'],
        entry_points=dict(enqueue=0x6DA7D0, maintenance=0x6D2B60, cell=0x480180,
                          shroud=0x6D3660, terrain=0x6D2DE0, full=0x6D7560,
                          dirty=0x6D7F20, tmp=0x547CF0,
                          queue_clear=0x6D4571, redraw_clear=0x6D4B37))


def partial_redraw_main(argv):
    import argparse
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--partial-output-dir', type=Path, required=True)
    parser.add_argument('--partial-detail', action='store_true')
    args, remaining = parser.parse_known_args(argv)
    repo = Path(__file__).resolve().parents[2]
    directory = args.partial_output_dir.resolve()
    if directory.is_relative_to(repo) or any(
            arg == '--output' or arg.startswith('--output=') for arg in remaining):
        raise ValueError('Partial redraw controls require an external scratch directory')
    names = ('native_oracle.py', 'oracle_sources.py', 'native_inspect.py',
             'spatial_oracle/tmp_clear_render.py', 'spatial_oracle/bridge_shadow_render.py',
             'projectile_oracle/bridge_render_shape.py',
             'projectile_oracle/bridge_render_inputs.py',
             'projectile_oracle/bridge_render_inputs_palette.py',
             'procedural_drawing_oracle/shroud.py',
             'rules_oracle/theater_general_reader.py', 'bridge_click_oracle.py',
             'spatial_oracle/bridge_recalc_presentation.py',
             'spatial_oracle/building_body_rules.py',
             'spatial_oracle/engineer_repair_admission.py',
             'rmg_oracle/gen_rng_vectors.py')
    filename = 'tmp_partial_redraw_detail.json' if args.partial_detail else 'tmp_partial_redraw.json'
    finish_vectors(lambda: generate_partial_redraw(detail=args.partial_detail), directory / filename,
                   provenance=partial_redraw_metadata, argv=remaining,
                   source_paths={f'tools/{name}': repo / 'tools' / name for name in names})


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--preview', action='store_true')
    args, rest = parser.parse_known_args()
    finish_vectors(generate_preview if args.preview else generate,
        Path(__file__).with_name('tmp_preview.json') if args.preview else Path(__file__).with_suffix('.json'),
        provenance=preview_metadata if args.preview else metadata, argv=rest)
