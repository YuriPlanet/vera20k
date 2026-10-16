"""Original high bridge Cell callers, retail SHP and RGB565/Z pixels.

Run with --check (default) or --write. The shared native image, lexical INI,
archive, Convert-construction and shape owners provide the fixture boundaries;
this module executes original bridge caller and raster instructions unchanged.
"""
from pathlib import Path
import hashlib
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EBP,
    UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import (
    NATIVE_SHA256, RET_MAGIC, configured_gamemd, finish_vectors, provenance,
    run_checked,
)
from tools.projectile_oracle.bridge_render_inputs import lexical
from tools.projectile_oracle.bridge_render_inputs_palette import PaletteReader, initialize
from tools.palette_oracle import oracle as palette_oracle
from tools.projectile_oracle.bridge_render_shape import (
    SURFACE, ZOBJ, AOBJ, ZSURF, ASURF, PIXELS, Z, A, packed_words,
)
from tools.sidebar_oracle.stock import mix, mix_hash
from tools.spatial_oracle.building_body_rules import RULES, SP, dwords
from tools.spatial_oracle.bridge_constructor import OriginalBridgeConstructor, D as CONSTRUCTOR_MEM

WIDTH, HEIGHT = 192, 160
COUNT = WIDTH * HEIGHT
MEM = 0x21000000
CELL, POINT, CLIP, RECT = [MEM + n for n in (0, 0x1000, 0x1020, 0x1040)]
LEAVES = {0x493830: 'plain_shadow', 0x497390: 'rle_shadow'}
BODY_LEAVES = {0x4990E0: 'rle_body'}
BRIDGE_TYPES = ('BRIDGE1', 'BRIDGE2', 'BRIDGEB1', 'BRIDGEB2')
ORE_TYPES = tuple(f'TIB{index:02}' for index in range(1, 13))


def ints(u, address, count):
    return list(struct.unpack('<' + 'i' * count, u.mem_read(address, count * 4)))


def physical_assets(*, body=False, ore=False, tile=False, placement=False, radar=False,
                    frontier=False, tmp_names=(), smudge=False, hills=False):
    root = configured_gamemd().parent
    sources, assets = [], {}
    containers = (
        ('ra2.mix', {'temperat.mix': ('bridge.tem', 'bridgb.tem'),
                     'snow.mix': ('bridge.sno', 'bridgb.sno'),
                     'urban.mix': ('bridge.urb', 'bridgb.urb'),
                     'cache.mix': ('palette.pal', 'anim.pal')
                         + (('isotem.pal', 'temperat.pal') if body else ())}),
        ('ra2md.mix', {'localmd.mix': ('RULESMD.INI', 'ARTMD.INI', 'MPBattleMD.ini'),
                       'urbann.mix': ('bridge.ubn', 'bridgb.ubn'),
                       'desert.mix': ('bridge.des', 'bridgb.des'),
                       'lunar.mix': ('bridge.lun', 'bridgb.lun')}),
    )
    if ore:
        containers = (
            ('ra2.mix', {'temperat.mix': tuple(f'{name.lower()}.tem' for name in ORE_TYPES),
                         'urban.mix': tuple(f'{name.lower()}.urb' for name in ORE_TYPES),
                         'cache.mix': ('palette.pal', 'anim.pal', 'temperat.pal', 'urban.pal')}),
            ('ra2md.mix', {'localmd.mix': ('ARTMD.INI', 'MPBattleMD.ini')}),
        )
    if tile:
        containers = (
            ('ra2.mix', {'isotemp.mix': tuple(tmp_names) or tuple(f'clear01{s}.tem' for s in ('', 'a', 'b', 'c', 'd', 'e', 'f', 'g')),
                         'cache.mix': ('isotem.pal', 'temperat.pal'),
                         'conquer.mix': ('SHROUD.SHP',)}),
            ('ra2md.mix', {'localmd.mix': ('temperatmd.ini',)}),
            ('multimd.mix', {None: ('XMP03T4.MAP',)}),
        )
    if placement:
        containers = (
            ('ra2.mix', {'conquer.mix': ('PLACE.SHP',),
                         'cache.mix': ('palette.pal', 'anim.pal', 'temperat.pal'),
                         'isourb.mix': ('rmpfx03.urb', 'clat03.urb', 'clat04.urb', 'proad02.urb')}),
            ('ra2md.mix', {'localmd.mix': ('ARTMD.INI', 'MPBattleMD.ini', 'URBANMD.INI')}),
        )
    if radar:
        containers = (
            ('ra2.mix', {'isourb.mix': ('clat10.urb',)}),
            ('ra2md.mix', {'localmd.mix': ('URBANMD.INI',)}),
        )
    if frontier:
        containers = (
            ('ra2.mix', {'isourb.mix': tuple(tmp_names),
                         'cache.mix': ('isourb.pal', 'temperat.pal', 'palette.pal', 'anim.pal'),
                         'conquer.mix': ('SHROUD.SHP', 'FOG.SHP')}),
            ('ra2md.mix', {'localmd.mix': ('ARTMD.INI', 'MPBattleMD.ini', 'URBANMD.INI')}),
        )
    if smudge:
        containers = (('ra2.mix', {'temperat.mix': ('CRATER11.TEM',)}),
                      ('ra2md.mix', {'localmd.mix': ('ARTMD.INI', 'MPBattleMD.ini')}),
                      ('Hills.mmx', {None: ('Hills.map',)}))
    if hills:
        containers += (('Hills.mmx', {None: ('Hills.map',)}),)
    for outer_name, children in containers:
        outer_raw = (root / outer_name).read_bytes()
        outer = mix(outer_raw)
        for inner_name, names in children.items():
            inner_raw = outer[mix_hash(inner_name)] if inner_name is not None else outer_raw
            inner = mix(inner_raw)
            if tile and inner_name == 'isotemp.mix':
                assert mix_hash('clear01h.tem') not in inner
            if frontier and inner_name == 'isourb.mix':
                assert mix_hash('clear01h.urb') not in inner
            for name in names:
                raw = inner[mix_hash(name)]
                assets[name.upper()] = raw
                sources.append(dict(name=name, outer=outer_name, inner=inner_name,
                                    outer_sha256=hashlib.sha256(outer_raw).hexdigest(),
                                    inner_sha256=hashlib.sha256(inner_raw).hexdigest(),
                                    sha256=hashlib.sha256(raw).hexdigest(), bytes=len(raw)))
    if ore or placement or frontier or smudge:
        # Active retail's expansion archive overrides the base RULESMD.
        raw = (root / 'expandmd01.mix').read_bytes()
        rules = mix(raw)[mix_hash('RULESMD.INI')]
        assets['RULESMD.INI'] = rules
        sources.append(dict(name='RULESMD.INI', outer='expandmd01.mix', inner=None,
                            outer_sha256=hashlib.sha256(raw).hexdigest(),
                            sha256=hashlib.sha256(rules).hexdigest(), bytes=len(rules)))
        if not smudge:
            raw = (root / 'GoldSt.mmx').read_bytes()
            assets['GOLDST.MMX'] = raw
            sources.append(dict(name='GoldSt.mmx', kind='loose', outer=None, inner=None,
                                sha256=hashlib.sha256(raw).hexdigest(), bytes=len(raw)))
    return assets, sources


def prepare(*, body=False, ore=False, placement=False):
    assert sum((body, ore, placement)) <= 1
    assets, sources = physical_assets(body=body, ore=ore, placement=placement)
    shadow_bytes, shape_aliases = {}, []
    for name, raw in assets.items():
        image_name = name.split('.')[0]
        if image_name not in ('BRIDGE', 'BRIDGB'):
            continue
        count = struct.unpack_from('<H', raw, 6)[0]
        normalized = bytearray()
        # Compare exact crop/format metadata and compressed bytes, omitting only
        # frame file offsets (body compression changes their absolute values).
        # This is byte equivalence, not a second source-image decoder.
        for frame in range(count // 2, count):
            normalized.extend(raw[8 + 24 * frame:28 + 24 * frame])
            start = struct.unpack_from('<I', raw, 28 + 24 * frame)[0]
            end = (struct.unpack_from('<I', raw, 28 + 24 * (frame + 1))[0]
                   if frame + 1 < count else len(raw))
            normalized.extend(raw[start:end])
        shadow_bytes.setdefault(image_name, []).append(bytes(normalized))
        shape_aliases.append(dict(name=name, raw_frame_count=count,
                                   shadow_frames_sha256=hashlib.sha256(normalized).hexdigest()))
    assert all(len(images) == 6 and all(raw == images[0] for raw in images)
               for images in shadow_bytes.values())
    selected_types = ('GAPOWR',) if placement else ORE_TYPES if ore else BRIDGE_TYPES
    art, art_lines = lexical(assets['ARTMD.INI'], set(selected_types)
                             | (set() if ore else {'BRIDGE', 'BRIDGB'}))
    m = PaletteReader(art)
    m.assets.update(assets)
    initialize(m, ordinary=ore, early_temperat=placement)
    u = m.u
    u.mem_map(MEM, 0x200000)
    if placement:
        # Same native RGB565/Z/A surface owner; selected type construction is
        # the placement module's BuildingType reader, not OverlayType.
        selected_types = ()
    # Existing registry constructor, then actual physical registry-prefix names.
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, 0x4E71E0, 0x4E7216)
    declared, _ = lexical(assets['RULESMD.INI'], {'OverlayTypes'})
    names = list(declared['OverlayTypes'].values())
    names = names[:1 + max(names.index(name) for name in selected_types)] if selected_types else []
    types = {}
    for name in names:
        typ = m.alloc(0x300)
        m.invoke(0x5FE250, typ, (m.cstring(name),))
        types[name] = typ
    m.make_ini(art)
    layers = []
    for filename in ('RULESMD.INI', 'MPBattleMD.ini', *(('GoldSt.mmx',) if ore else ())):
        sections, lines = lexical(assets[filename.upper()], set(selected_types))
        m.rules_cache(sections)
        for name in selected_types:
            typ = types[name]
            mark = len(m.asset_loaded)
            admitted = m.invoke(0x5FE770, typ, (RULES,)) & 255
            layers.append(dict(file=filename, name=name, admitted=admitted,
                               physical_keys=sections.get(name), source_lines=lines,
                               image_name=m.string(typ + 0x1F8),
                               image_present=bool(m.read32(typ + 0xA4)),
                               index=m.read32(typ + 0x294), land=m.read32(typ + 0x298),
                               wall=u.mem_read(typ + 0x2A8, 1)[0],
                               tiberium=u.mem_read(typ + 0x2A9, 1)[0],
                               crate=u.mem_read(typ + 0x2AA, 1)[0],
                               asset_loads=m.asset_loaded[mark:]))
    for address, buffer in ((SURFACE, PIXELS), (ZSURF, Z), (ASURF, A)):
        u.mem_write(address, dwords(0x7E2070, WIDTH, HEIGHT, 0, 2,
                                    buffer, COUNT * 2, 0))
    u.mem_write(0x887314, dwords(SURFACE))
    for address, backing, buffer, global_ in (
        (ZOBJ, ZSURF, Z, 0x887644), (AOBJ, ASURF, A, 0x87E8A4),
    ):
        u.mem_write(address, dwords(0, 0, WIDTH, HEIGHT, 0, backing,
                                    buffer, buffer + COUNT * 2, COUNT * 2, 32768, WIDTH))
        u.mem_write(global_, dwords(address))
    u.mem_write(CELL, dwords(0x7E4EEC))
    u.mem_write(CELL + 0x34, dwords(m.read32(0x87F6C4)))
    inputs = dict(sources=sources, shape_aliases=shape_aliases, registry_prefix=names,
                  physical_art=art, art_source_lines=art_lines, layers=layers)
    if body:
        inputs['cell_convert'] = initialize_cell_convert(m)
    if ore:
        inputs['ordinary_convert'] = ordinary_convert_state(m)
        inputs['tiberium'] = prepare_tiberium(m, assets)
        from tools.spatial_oracle.shrapnel_repair.map_facts import decode_cells
        parsed, cells = decode_cells(assets['GOLDST.MMX'])
        inputs['map'] = dict(name='GoldSt.mmx', physical_map_section=parsed['Map'],
                             cells=[dict(coords=list(xy), **cells[xy])
                                    for xy in ((123, 33), (124, 33))],
                             boundary='Authored pack data only; live slope0/flags0 are supplied component inputs, not native map-loader results.')
        assert all(assets[f'{name}.TEM'] == assets[f'{name}.URB'] for name in ORE_TYPES)
        assert assets['TEMPERAT.PAL'] == assets['URBAN.PAL']
        inputs['urban_aliases'] = dict(images=list(ORE_TYPES), byte_identical=True,
                                       palettes=['TEMPERAT.PAL', 'URBAN.PAL'])
    return m, types, inputs


def ordinary_convert_state(m):
    convert = m.read32(0x87F6B8)
    assert m.read32(convert) == 0x7E5358 and m.read32(convert + 4) == 2
    count, table = m.read32(convert + 0x16C), m.read32(convert + 0x170)
    assert count == 53
    return dict(global_address='0087F6B8', source_palette='TEMPERAT.PAL',
                destination_palette='TEMPERAT.PAL', shade_count=count,
                middle_row=(m.read32(convert + 0x174) - table) // 512,
                expanded_palette_sha256=hashlib.sha256(bytes(m.u.mem_read(0x885780, 768))).hexdigest(),
                table_sha256=hashlib.sha256(bytes(m.u.mem_read(table, count * 512))).hexdigest())


def prepare_tiberium(m, assets):
    """Full original Tiberium constructor and physical selected layer readers."""
    u = m.u
    u.mem_write(0xB0F4E8, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
    typ = m.alloc(0x200)
    m.invoke(0x7216C0, typ, (m.cstring('Riparius'),))
    layers = []
    for filename in ('RULESMD.INI', 'MPBattleMD.ini', 'GoldSt.mmx'):
        sections, lines = lexical(assets[filename.upper()], {'Riparius'})
        m.rules_cache(sections)
        admitted = m.invoke(0x721A50, typ, (RULES,)) & 255
        layers.append(dict(file=filename, physical_keys=sections.get('Riparius'),
                           source_lines=lines, admitted=admitted,
                           index=m.read32(typ + 0x98),
                           base_overlay_id=m.read32(m.read32(typ + 0xE0) + 0x294),
                           num_images=m.read32(typ + 0xE8), num_slopes=m.read32(typ + 0xEC),
                           value=m.read32(typ + 0xB8), growth=m.read32(typ + 0xA8),
                           spread=m.read32(typ + 0x9C),
                           growth_percentage_bits=f'{struct.unpack("<Q", u.mem_read(typ + 0xB0, 8))[0]:016x}',
                           spread_percentage_bits=f'{struct.unpack("<Q", u.mem_read(typ + 0xA0, 8))[0]:016x}'))
    assert layers[-1]['index'] == 0 and layers[-1]['base_overlay_id'] == 102
    assert layers[-1]['num_images'] == 12
    return dict(constructor='007216C0', reader='00721A50', layers=layers)


def initialize_cell_convert(m, *, source_palette="ISOTEM.PAL", destination_palette="TEMPERAT.PAL"):
    """Original theater palette expansion, default LightConvert and Cell binding.

    Physical palette IO is supplied; the original 5454DB expansion loop,
    545000..5450EF default converter publication and 483E30 Cell writer run.
    The full theater/TMP loader and scenario lighting graph are excluded.
    """
    u = m.u
    palettes = []
    for name, address in ((source_palette, 0xABBED0), (destination_palette, 0x885780)):
        raw = m.assets[name]
        assert len(raw) == 768
        u.mem_write(address, raw)
        u.reg_write(UC_X86_REG_EAX, address)
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, 0x5454DB, 0x5454EB,
                    required_addresses=(0x5454E0, 0x5454E2, 0x5454E5))
        expanded = bytes(u.mem_read(address, 768))
        palettes.append(dict(name=name, address=f'{address:08X}',
                             sha256=hashlib.sha256(raw).hexdigest(),
                             expanded_sha256=hashlib.sha256(expanded).hexdigest()))
    # Empty preallocated native registry; the original initializer appends
    # its freshly constructed unity converter before the stop boundary.
    u.mem_write(0x87F698, dwords(0x7EB6D4, m.alloc(32), 8, 1, 0, 10))
    # Same captured RGB565/MMX dispatch inputs as tools.palette_oracle.oracle;
    # the whole ctor/UpdateColors still generates this VM's actual table.
    u.mem_write(0x829D20, dwords(2))
    u.mem_write(0x84E860, bytes((0, 0, 1)))
    u.reg_write(UC_X86_REG_ESP, SP)
    reached = set()
    def observe(_u, pc, _size, _data):
        if pc in (0x54508F, 0x555DA0, 0x556090, 0x7DE200, 0x48EBF0):
            reached.add(pc)
    hook = u.hook_add(UC_HOOK_CODE, observe)
    run_checked(u, 0x545000, 0x5450EF, count=10000000, timeout_us=30000000,
                required_addresses=(0x54508F, 0x555DA0, 0x556090, 0x7DE200, 0x5450EC))
    u.hook_del(hook)
    assert m.read32(0x87F6A8) == 1
    convert = m.read32(m.read32(0x87F69C))
    assert [m.read32(convert + off) for off in (0x188, 0x18C)] == [0xABBED0, 0x885780]
    assert ints(u, convert + 0x198, 3) == [1000] * 3
    assert m.read32(convert + 0x16C) == 53
    palette_table = bytes(u.mem_read(m.read32(convert + 0x170), 53 * 512))
    assert palette_table == palette_oracle.palette_table(
        53, (1000, 1000, 1000), bytes(u.mem_read(0xABBED0, 768)), 'mmx')
    # Explicit unity cell-lighting input; the native owner binds the produced
    # converter, increments its reference and writes all brightness scalars.
    m.invoke(0x483E30, CELL, (convert, 0x10000, 0, 1000, 1000, 1000))
    assert m.read32(CELL + 0x34) == convert
    assert struct.unpack('<3h', u.mem_read(CELL + 0x10A, 6)) == (1000, 1000, 1000)
    return dict(palettes=palettes, rgb=[1000] * 3, shade_count=53,
                source_palette=source_palette, destination_palette=destination_palette,
                cell_brightness=[1000] * 3,
                palette_table_sha256=hashlib.sha256(palette_table).hexdigest(),
                palette_dispatch='RGB565/MMX',
                reached=[f'{pc:08X}' for pc in sorted(reached)])


def snapshot(u, background, old_z):
    colors = list(struct.unpack('<' + 'H' * COUNT, u.mem_read(PIXELS, COUNT * 2)))
    depths = list(struct.unpack('<' + 'H' * COUNT, u.mem_read(Z, COUNT * 2)))
    # Lossless horizontal runs of native output, omitting only unchanged input.
    runs = []
    for y in range(HEIGHT):
        x = 0
        while x < WIDTH:
            offset = y * WIDTH + x
            pair = colors[offset], depths[offset]
            if pair == (background, old_z):
                x += 1
                continue
            start = x
            while x < WIDTH and (colors[y * WIDTH + x], depths[y * WIDTH + x]) == pair:
                x += 1
            runs.append([start, y, x - start, *pair])
    return dict(runs=runs,
                color_sha256=hashlib.sha256(bytes(u.mem_read(PIXELS, COUNT * 2))).hexdigest(),
                depth_sha256=hashlib.sha256(bytes(u.mem_read(Z, COUNT * 2))).hexdigest())


def execute(m, types, case, *, body=False, ore=False, boundary_only=False, reset_surface=True):
    assert body or ore or not boundary_only
    case = dict(type='TIB01' if ore else 'BRIDGE1',
                coords=[123, 33] if ore else [10, 20], level=5 if ore else 0, state=0,
                flags=0 if ore else 0x180,
                cell_point=[66, 141] if ore else [66, 81], clip=[0, 0, WIDTH, HEIGHT],
                viewport_y=0, background=0xFFFF, old_z=65535, repeat=1) | case
    u = m.u
    typ = types[case.get('type', 'BRIDGE1')]
    shp = m.read32(typ + 0xA4)
    assert shp, 'Physical image must bind through original reader'
    u.mem_write(CELL + 0x44, dwords(m.read32(typ + 0x294)))
    u.mem_write(CELL + 0x24, struct.pack('<2h', *case['coords']))
    u.mem_write(CELL + 0x11B, bytes((case.get('level', 0) & 255,)))
    u.mem_write(CELL + 0x11E, bytes((case.get('state', 0),)))
    u.mem_write(CELL + 0x140, dwords(case.get('flags', 0x180)))
    if ore:
        u.mem_write(CELL + 0x11C, bytes((0,)))
    u.mem_write(0x886FA0, dwords(0, case.get('viewport_y', 0), WIDTH, HEIGHT))
    u.mem_write(POINT, dwords(*case.get('cell_point', [66, 81])))
    u.mem_write(CLIP, dwords(*case.get('clip', [0, 0, WIDTH, HEIGHT])))
    background, old_z = case.get('background', 0xFFFF), case.get('old_z', 65535)
    if reset_surface:
        u.mem_write(PIXELS, packed_words([background] * COUNT))
        u.mem_write(Z, packed_words([old_z] * COUNT))
        u.mem_write(A, packed_words([127] * COUNT))
    draws, rows, reached, steps = [], [], set(), []
    leaves = BODY_LEAVES if body or ore else LEAVES
    entry = 0x47F6A0 if body or ore else 0x47F510

    def observe(_u, address, _size, _data):
        if address in (entry, 0x480110, 0x5FDCC0, 0x4AED70, 0x69E7E0,
                       0x69E740, 0x69E900, 0x490E50, 0x437A10, *leaves):
            reached.add(address)
        if address == 0x4AED70:
            sp = u.reg_read(UC_X86_REG_ESP)
            args = ints(u, sp + 4, 14)
            draws.append(dict(frame=args[1], point=ints(u, args[2], 2),
                              clip=ints(u, args[3], 4), flags=args[4],
                              z_adjust=args[6], gradient=args[7], brightness=args[8],
                              zshape_argument=args[12], raw_args=args))
        elif address in leaves:
            sp = u.reg_read(UC_X86_REG_ESP)
            args = ints(u, sp + 4, 10)
            offset = (args[0] - PIXELS) // 2
            rows.append(dict(leaf=leaves[address], x=offset % WIDTH,
                             y=offset // WIDTH, count=args[2], skip=args[3],
                             candidate=args[4], zshape_address=args[9],
                             zshape_values=sorted(set(struct.unpack(
                                 '<' + 'b' * args[2], u.mem_read(args[9], args[2]))))))

    hook = u.hook_add(UC_HOOK_CODE, observe)
    for _ in range(case.get('repeat', 1)):
        if body or ore:
            # Supplied dirty-cache miss forces every repeat through the
            # original raster, rather than measuring its redundant-draw cache.
            u.mem_write(CELL + 0x64, dwords(0xFFFFFFFF))
        if boundary_only:
            u.mem_write(SP, dwords(RET_MAGIC, POINT, CLIP))
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_ECX, CELL)
            run_checked(u, entry, 0x4AED70)
            observe(u, 0x4AED70, 0, None)
        else:
            m.invoke(entry, CELL, (POINT, CLIP))
        if (body or ore) and not boundary_only and case['repeat'] > 1:
            steps.append(snapshot(u, background, old_z))
    u.hook_del(hook)
    required = {entry, 0x480110, 0x5FDCC0, 0x4AED70}
    if not boundary_only:
        required |= {0x490E50, 0x437A10, 0x4990E0 if body or ore else 0x497390}
    assert draws and required.issubset(reached)
    if ore:
        # The original coordinate selector can bind another image than the
        # gameplay Cell's live overlay identity. Observe its actual pointer.
        shp = draws[0]['raw_args'][0]
        typ = next(pointer for pointer in types.values() if m.read32(pointer + 0xA4) == shp)
    m.invoke(0x69E7E0, shp, (RECT, draws[0]['frame']))
    result = dict(name=case['name'], input=case, draws=draws, leaf_rows=rows,
                  image_name=m.string(typ + 0x1F8),
                  native_rect=ints(u, RECT, 4),
                  reached=[f'{p:08X}' for p in sorted(reached)])
    if not boundary_only:
        result['output'] = snapshot(u, background, old_z)
    result['canvas'] = list(struct.unpack('<2H', u.mem_read(shp + 2, 4)))
    if steps:
        result['steps'] = steps
    if ore:
        result['display_overlay_id'] = m.read32(typ + 0x294)
    return result


def pixel_rows(m):
    """Use original constructor-owned selected leaves on explicit small spans."""
    u = m.u
    convert = m.read32(0x87F6C4)
    dest, source, depth, zshape = [MEM + n for n in (0x90000, 0x91000, 0x92000, 0x93000)]
    u.mem_write(ZOBJ + 0x1C, dwords(depth + 0x1000, 0x1000))
    controls = [
        dict(name='near_equal_far_transparent', candidate=1000,
             colors=[65535] * 4, depths=[999, 1000, 1001, 1001], indices=[1, 1, 1, 0]),
        dict(name='signed_zshape', candidate=1000, colors=[65535] * 3,
             depths=[1000] * 3, indices=[1] * 3, zshape=[1, 0, -1]),
    ]
    controls += [dict(name=f'signed_candidate_{candidate}', candidate=candidate,
                     colors=[65535, 0x39E7, 0x7E0, 0xF800],
                     depths=[0, 1000, 32768, 65535], indices=[1, 1, 1, 1])
                 for candidate in (-65537, -1, 0, 32768, 65535, 65536)]
    rows = []
    for format_, selector in (('plain', 0x490B90), ('rle', 0x490E50)):
        obj = m.invoke(selector, convert, (0x4601,))
        vtable = m.read32(obj)
        leaf = m.read32(vtable + 4)
        assert leaf == (0x493830 if format_ == 'plain' else 0x497390)
        for control in controls:
            if format_ == 'plain' and 'zshape' in control:
                continue
            n = len(control['indices'])
            encoded = (b''.join(bytes((i,)) if i else b'\0\1' for i in control['indices'])
                       if format_ == 'rle' else bytes(control['indices']))
            u.mem_write(source, encoded)
            u.mem_write(dest, packed_words(control['colors']))
            u.mem_write(depth, packed_words(control['depths']))
            u.mem_write(zshape, struct.pack('<' + 'b' * n, *control.get('zshape', [0] * n)))
            args = ((dest, source, n, 0, control['candidate'], depth, A, 1000, 0, zshape)
                    if format_ == 'rle' else
                    (dest, source, n, control['candidate'], depth, A, 1000, 0))
            steps = []
            for _ in range(2):
                m.invoke(leaf, obj, args)
                steps.append(dict(colors=list(struct.unpack('<' + 'H' * n, u.mem_read(dest, 2 * n))),
                                  depths=list(struct.unpack('<' + 'H' * n, u.mem_read(depth, 2 * n)))))
            rows.append(dict(format=format_, selector=f'{selector:08X}',
                             vtable=f'{vtable:08X}', leaf=f'{leaf:08X}', input=control, steps=steps))
    return rows


def traversal(m, *, origin=(10, 20), view_origin=(0, 0), require_draws=True):
    """Original Tactical loop order; map/visibility and draw calls are sinks."""
    u = m.u
    tactical = m.alloc(0xE20)
    u.mem_write(tactical + 0xB0, dwords(*view_origin))
    u.mem_write(0xB0CE30, dwords(WIDTH, HEIGHT))
    u.mem_write(0xB0CD48, struct.pack('<Q', 0x3FC25E5374344960))
    u.mem_write(SP, bytes(0x180))
    u.mem_write(SP + 0x28, struct.pack('<hh', *origin))
    u.mem_write(SP + 0x38, dwords(3))
    u.mem_write(SP + 0x64, dwords(0, 0, WIDTH, HEIGHT))
    u.mem_write(SP + 0x74, dwords(2))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_EDI, tactical)
    u.reg_write(UC_X86_REG_EBP, 2)
    calls, coords = [], []

    def observe(_u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address == 0x568300:
            m.ret(1, 4)
        elif address == 0x5657A0:
            coords[:] = struct.unpack('<2h', u.mem_read(m.read32(sp + 4), 4))
            u.mem_write(CELL + 0x24, struct.pack('<2h', *coords))
            m.ret(CELL, 4)
        elif address in (0x47FB90, 0x47FDE0):
            output = m.read32(sp + 4)
            u.mem_write(output, dwords(0, 0, WIDTH, HEIGHT))
            m.ret(output, 4)
        elif address in (0x47F6A0, 0x47F510):
            calls.append(dict(piece='body' if address == 0x47F6A0 else 'shadow',
                              coords=list(coords), point=ints(u, m.read32(sp + 4), 2)))
            m.ret(0, 8)

    hook = u.hook_add(UC_HOOK_CODE, observe)
    run_checked(u, 0x6D6E5B, 0x6D71D3, count=200000,
                required_addresses=((0x6D7001, 0x6D7031, 0x6D719F) if require_draws
                                    else (0x6D6EDA, 0x6D7031)))
    u.hook_del(hook)
    return dict(entry='006D6E5B', end='006D71D3', origin=list(origin),
                max_diagonal=2, cells_per_diagonal=3, calls=calls)


def generate():
    m, types, inputs = prepare()
    cases = [dict(name=f'state_{state}', state=state) for state in range(18)]
    cases += [dict(name=f'level_{level}', level=level) for level in (-1, 2, 6)]
    cases += [dict(name=f'flags_{flags}_state_{state}', flags=flags, state=state)
              for flags in (0, 0x80, 0x100) for state in (0, 8, 9, 17)]
    cases += [dict(name=f'depth_{depth}', old_z=depth) for depth in (32714, 32715, 32716)]
    cases += [dict(name='equal_repeat', repeat=2),
              dict(name='bridge2_state9', type='BRIDGE2', state=9),
              dict(name='viewport_y_23', viewport_y=23),
              dict(name='viewport_y_minus17', viewport_y=-17),
              dict(name='dirty_rebase', clip=[11, 13, 140, 100]),
              dict(name='top_left_clip', cell_point=[-18, 6]),
              dict(name='bottom_right_clip', cell_point=[150, 150])]
    cases += [dict(name=f'wood_state_{state}', type='BRIDGEB1', state=state,
                   cell_point=[36, 81]) for state in range(18)]
    cases += [dict(name=f'wood_type2_state_{state}', type='BRIDGEB2', state=state,
                   cell_point=[36, 81]) for state in (0, 9)]
    rows = [execute(m, types, case) for case in cases]
    shapes = []
    for row in (row for row in rows if row['name'].startswith(('state_', 'wood_state_'))):
        point = row['draws'][0]['point']
        canvas_top = [point[i] - row['canvas'][i] // 2 for i in range(2)]
        assert len(row['leaf_rows']) == row['native_rect'][3]
        assert all(leaf['skip'] == 0 and leaf['count'] == row['native_rect'][2]
                   for leaf in row['leaf_rows']), 'Mask witness must not be clipped'
        shapes.append(dict(image_name=row['image_name'], frame=row['draws'][0]['frame'], canvas=row['canvas'],
                           native_rect=row['native_rect'],
                           mask_runs=[[x - canvas_top[0], y - canvas_top[1], n]
                                      for x, y, n, _color, _depth in row['output']['runs']]))
    return dict(native_sha256=NATIVE_SHA256, inputs=inputs, shapes=shapes,
                surface=dict(width=WIDTH, height=HEIGHT, baseline=32768, bytes_per_pixel=2),
                rows=rows, traversal=traversal(m), pixel_rows=pixel_rows(m))


def metadata():
    return provenance(
        scope='43 original high concrete and 20 affected high wood bridge Cell shadow callers through physical bridge.tem/bridgb.tem SHP, clip, selector, RLE rowwalker, shadow color and depth writes; 15 selected plain/RLE leaf controls; original two-sweep Tactical traversal suffix with visibility/map and draw sinks. No complete scene, map loader, GPU or gameplay parity.',
        assumptions=[
            'Physical archives and lexical INI entries are supplied using existing archive/lexical owners. Original Overlay registry constructors, BRIDGE1/2 and BRIDGEB1/2 full rules readers, ART/image binding and Convert constructors execute. Selected RULESMD and MPBattleMD layers; no map overrides or LANGRULE are claimed.',
            'Prepared live Cell level/state/flags, projected zero-height top-left cell point, dirty rectangle, viewport Y and RGB565 surfaces are explicit inputs. Original Get_Draw_Offset, shadow frame selection, shape clipping, selector, rowwalker and pixel leaves execute unchanged.',
            'A/Z buffers use original BSurface virtual methods, baseline32768, A127. Packed output runs are lossless observations of original final colors/depth relative to the input background/oldZ.',
            'All 18 selected retail shadow frames in each family use native format3; native shape selects gradient0, optional Zshape0 and the rowwalker fallback89C568, whose observed bytes are zero. shapes.mask_runs are obtained from native admitted shadow pixels at oldZ65535, translated into physical180x180 BRIDGE or253x242 BRIDGB canvas; there is no Python SHP pixel decoder or expected-color implementation. Woodrows use a supplied36,81 point to keep their wider shadow crop uncut.',
            'Within each physical BRIDGE and BRIDGB family, shadow halves in all six theaters are byte-identical after excluding absolute frame-data offsets: exact frame crop/format metadata and compressed bytes compare equal, even where body bytes differ. This static source-equivalence witness does not execute the other five theater loaders.',
            'pixel_rows invoke constructor-owned Convert selectors490B90/490E50 with4601 and their selected original leaf methods, using explicit input source indices, RGB565 destinations, signed candidate depths and optional signedZshape. Negative candidate controls show strict signed comparison precedes the low16 store; a negative repeated candidate can remain admitted after wrapping.',
            'traversal starts at6D6E5B after the visible region bounds were calculated; supplied origin10,20/max diagonal2/three cells per diagonal, original coordinate arithmetic, projection and rectangle intersection execute. Original loops emit all body calls, then all shadow calls, descending cellX+cellY and ascending cellX within each diagonal. No native map visibility admission or full Display order outside this bounded suffix is claimed.',
        ],
        substitutions=[
            'Existing PaletteReader allocator, TLS and archive-resource boundaries supply memory and physical named asset bytes. No reached bridge caller, shape, selector, rowwalker or shadow leaf is substituted.',
            'Only traversal substitutes568300 map-bound admission(true),5657A0 cell lookup(prepared Cell with requested coordinates),47FB90/47FDE0 visibility rectangles(full target), and47F6A0/47F510 draw sinks recording arguments. These substitutions never apply to raster rows or pixel_rows.',
        ],
        entry_points={'cell_shadow':0x47F510, 'cell_draw_offset':0x480110,
                      'overlay_draw_offset':0x5FDCC0, 'shape':0x4AED70,
                      'overlay_type_ctor':0x5FE250, 'overlay_type_read':0x5FE770,
                      'convert_ctor':0x48E740, 'blitter_init':0x48EBF0,
                      'tactical_sweeps_begin':0x6D6E5B, 'tactical_sweeps_end':0x6D71D3,
                      'rle_selector':0x490E50, 'rle_rowwalker':0x437A10,
                      'plain_shadow_leaf':0x493830, 'rle_shadow_leaf':0x497390})


def body_overlap_scene(m, types):
    """Native constructor publications, Tactical order and retained raster.

    Reuses the existing constructor fixture's sparse map and resident Road TMP
    boundary. Only produced overlay/state/flags and supplied level cross VMs;
    the rendering VM retains the physical type/image/Cell Convert owners.
    """
    producer = OriginalBridgeConstructor('success', 25)
    for pointer in producer.ptrs.values():
        producer.uc.mem_write(pointer + 0x11B, bytes((1,)))
    producer.construct()
    producer.obj = CONSTRUCTOR_MEM + 0x8000
    producer.uc.mem_write(producer.coord, struct.pack('<2h', 16, 17))
    producer.construct()
    facts = {}
    for coords in ((16, 16), (16, 17)):
        pointer = producer.ptrs[coords]
        overlay = ints(producer.uc, pointer + 0x44, 1)[0]
        assert overlay == m.read32(types['BRIDGE2'] + 0x294)
        facts[coords] = dict(coords=list(coords), overlay_id=overlay, type='BRIDGE2',
                             flags=producer.read_u32(pointer + 0x140),
                             state=producer.uc.mem_read(pointer + 0x11E, 1)[0],
                             level=producer.uc.mem_read(pointer + 0x11B, 1)[0])
    view_origin = [-90, 375]
    native_traversal = traversal(m, origin=(16, 16), view_origin=view_origin)
    cells = [facts[tuple(call['coords'])] | dict(cell_point=call['point'])
             for call in native_traversal['calls']
             if call['piece'] == 'body' and tuple(call['coords']) in facts]
    assert len(cells) == 2 and len({tuple(cell['coords']) for cell in cells}) == 2
    shared = dict(background=65535, old_z=65535, clip=[0, 0, WIDTH, HEIGHT],
                  viewport_y=0, repeat=1)
    for index, cell in enumerate(cells):
        composed = execute(m, types, shared | {key: value for key, value in cell.items()
                                               if key != 'overlay_id'}
                           | dict(name=f'overlap_{index}'), body=True,
                           reset_surface=index == 0)
    # An observed source/old-Z tie certificate keeps this scene relevant to
    # strict body depth and order. These are original isolated/final outputs,
    # not a second SHP decoder or an expected-composition implementation.
    isolated = [execute(m, types, shared | {key: value for key, value in cell.items()
                                           if key != 'overlay_id'}
                        | dict(name=f'isolated_{index}'), body=True)
                for index, cell in enumerate(cells)]
    maps = [{(x + offset, y): (color, depth)
             for x, y, count, color, depth in row['output']['runs']
             for offset in range(count)} for row in isolated + [composed]]
    overlap = maps[0].keys() & maps[1].keys()
    ties = [pixel for pixel in sorted(overlap)
            if maps[0][pixel][1] == maps[1][pixel][1]
            and maps[0][pixel][0] != maps[1][pixel][0]]
    assert ties and all(maps[2][pixel] == maps[0][pixel] for pixel in ties)
    return dict(name='adjacent_concrete_anchors', input=shared, cells=cells,
                native_order=[cell['coords'] for cell in cells],
                native_traversal=native_traversal,
                producer=dict(entry='005FC380', owner='bridge_constructor.OriginalBridgeConstructor',
                              calls=[[16, 16], [16, 17]], supplied_level=1,
                              view_origin=view_origin,
                              facts=list(facts.values()),
                              boundary='Existing sparse real9x9/type-index25/synthetic Road TMP success fixture; original constructors/Mark/setters execute. No native retail map load or xbayopigs input claim.'),
                tie_witness=dict(opaque_overlap=len(overlap), different_color_equal_z=len(ties),
                                 samples=[dict(pixel=list(pixel), first=list(maps[0][pixel]),
                                               second=list(maps[1][pixel]),
                                               native_composed=list(maps[2][pixel]))
                                          for pixel in ties[:8]]),
                output=composed['output'])


def generate_body():
    m, types, inputs = prepare(body=True)
    controls = [execute(m, types, dict(name=f'raw_state_{state}', coords=[1, 2],
                                     state=state), body=True, boundary_only=True)
                for state in range(18)]
    controls += [execute(m, types, dict(name=f'level_{level}', level=level, state=1),
                         body=True, boundary_only=True) for level in (-1, 2, 6)]
    controls += [execute(m, types, dict(name=f'flags_{flags}', flags=flags, state=0,
                                      coords=[1, 2]), body=True, boundary_only=True)
                 for flags in (0, 0x80, 0x100, 0x400)]
    controls += [execute(m, types, dict(name=f'latin_{x}_{y}', state=0, coords=[x, y]),
                         body=True, boundary_only=True)
                 for y in range(4) for x in range(4)]
    cases = [
        dict(name='concrete_ns_healthy', coords=[1, 2], cell_point=[66, 111]),
        dict(name='concrete_ew_damaged', type='BRIDGE2', state=15, cell_point=[66, 111]),
        dict(name='wood_ns_partial', type='BRIDGEB1', state=7, cell_point=[66, 141]),
        dict(name='wood_ew_healthy', type='BRIDGEB2', state=9, cell_point=[66, 141]),
    ]
    rows = [execute(m, types, case, body=True) for case in cases]
    assert all(len(row['leaf_rows']) == row['native_rect'][3] for row in rows), 'Full crop controls'
    # Original raster itself supplies the candidate for three one-row Z
    # controls. No Rust depth formula or hand-computed expected pixel is used.
    probe_case = dict(name='depth_probe', clip=[0, 0, WIDTH, 1])
    probe = execute(m, types, probe_case, body=True)
    x, y, _count, _color, candidate = next(
        run for run in probe['output']['runs'] if run[3] not in (0, 65535))
    assert 0 < candidate < 65535 and y == 0
    for name, old_z in (('near', candidate + 1), ('equal', candidate), ('far', candidate - 1)):
        row = execute(m, types, dict(name=f'depth_{name}', old_z=old_z,
                                    clip=[0, 0, WIDTH, 1]), body=True)
        row['depth_probe'] = dict(pixel=[x, y], candidate=candidate,
                                  source='original depth_probe raster output')
        rows.append(row)
    assert rows[-3]['output']['runs'] and not rows[-2]['output']['runs'] and not rows[-1]['output']['runs']
    repeat = execute(m, types, dict(name='equal_repeat', repeat=2,
                                   clip=[0, 0, WIDTH, 24]), body=True)
    assert len(repeat['steps']) == 2 and repeat['steps'][0] == repeat['steps'][1]
    rows.append(repeat)
    return dict(native_sha256=NATIVE_SHA256, inputs=inputs,
                surface=dict(width=WIDTH, height=HEIGHT, baseline=32768, bytes_per_pixel=2),
                rows=rows, controls=controls, scenes=[body_overlap_scene(m, types)])


def ore_selector_controls(m):
    """Original flat selector, including signed-coordinate boundary inputs."""
    u = m.u
    tib = m.read32(m.read32(0xB0F4EC))
    rows = []
    for xy in [(x, 1) for x in range(12)] + [(123, 33), (124, 33), (-1, 1),
                                           (-32768, 32767), (32767, 32767)]:
        u.mem_write(CELL + 0x24, struct.pack('<2h', *xy))
        u.reg_write(UC_X86_REG_ESI, CELL)
        u.reg_write(UC_X86_REG_EBX, tib)
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, 0x47F8D7, 0x47F900, required_addresses=(0x47F8E3, 0x47F8FD))
        selected = u.reg_read(UC_X86_REG_ECX)
        rows.append(dict(coords=list(xy), display_overlay_id=m.read32(selected + 0x294)))
    return rows


def ore_reduction_sequence(m, types):
    """Original Cell reduction and full Recalc in the same physical-art VM.

    Sparse map/zone storage and a sentinel TMP are explicit component state;
    this neither constructs GoldSt terrain nor reimplements the reduction.
    """
    from tools.spatial_oracle.terrain_recalc import LAT_GLOBALS
    u = m.u
    initial = execute(m, types, dict(name='density_1', state=1), ore=True)
    u.mem_write(CELL + 0x38, dwords(0xFFFF))
    u.mem_write(CELL + 0xEC, dwords(5))
    u.mem_write(0x887324, dwords(m.alloc(0xE20)))
    u.mem_write(0x8871E0, dwords(m.alloc(0x2000)))
    u.mem_write(0xB0CD48, struct.pack('<Q', 0x3FC25E5374344960))
    u.mem_write(0xB0CE30, dwords(WIDTH, HEIGHT))
    # Physical map Size is103x82, but only this component's cell is resident.
    # Native map lookups return the shared empty Dummy for all neighbors.
    u.mem_write(0x87F7E8 + 0xF4, dwords(103, 82))
    u.mem_write(0x87F7E8 + 0x68, dwords(m.alloc(1000000), 100000, m.alloc(1000000)))
    table = m.alloc(0x100000)
    u.mem_write(table + (33 * 512 + 123) * 4, dwords(CELL))
    u.mem_write(0x87F7E8 + 0x13C, dwords(table, 0x40000))
    u.mem_write(0xABDC50 + 0x44, dwords(-1))
    for address in LAT_GLOBALS:
        u.mem_write(address, dwords(-1))
    tib = m.read32(m.read32(0xB0F4EC))
    u.mem_write(tib + 0xF8, dwords(m.alloc(0x10000)))
    m.invoke(0x49F2F0, 0)
    scenario = m.read32(0xA8B230)
    u.mem_write(scenario, dwords(0x80))
    m.invoke(0x65C6D0, scenario + 0x218, (1,))
    rng_before = bytes(u.mem_read(scenario + 0x218, 0x3F4))
    events = []

    def observe(_u, pc, _size, _data):
        if pc in (0x47D2B0, 0x722AB0, 0x722AF0, 0x7235A0, 0x65C780):
            events.append(dict(entry=f'{pc:08X}'))
        elif pc in (0x6551C0, 0x6D2790):
            sp = u.reg_read(UC_X86_REG_ESP)
            events.append(dict(entry=f'{pc:08X}',
                               args=ints(u, sp + 4, 1 if pc == 0x6551C0 else 5)))
            m.ret(0, 4 if pc == 0x6551C0 else 0x14)

    hook = u.hook_add(UC_HOOK_CODE, observe)
    steps, rows = [], [initial]
    for index in range(2):
        start = len(events)
        removed = m.invoke(0x480A80, CELL, (1,))
        state = dict(overlay_id=ints(u, CELL + 0x44, 1)[0],
                     data=u.mem_read(CELL + 0x11E, 1)[0],
                     land=ints(u, CELL + 0xEC, 1)[0],
                     level=struct.unpack('<b', u.mem_read(CELL + 0x11B, 1))[0])
        steps.append(dict(amount=1, removed=removed, state=state, events=events[start:]))
        if index == 0:
            assert state['overlay_id'] == 102
            rows.append(execute(m, types, dict(name='density_0', state=state['data']), ore=True))
    u.hook_del(hook)
    assert bytes(u.mem_read(scenario + 0x218, 0x3F4)) == rng_before
    assert not any(event['entry'] == '0065C780' for event in events)
    assert steps[-1]['state']['overlay_id'] == -1
    # The original active caller's own -1 test skips body and shadow calls.
    # Do not invoke47F6A0 with an invalid identity or manufacture its result.
    u.mem_write(PIXELS, packed_words([65535] * COUNT))
    u.mem_write(Z, packed_words([65535] * COUNT))
    native_traversal = traversal(m, origin=(123, 33), require_draws=False)
    assert native_traversal['calls'] == []
    empty_input = initial['input'] | dict(name='empty', state=0, overlay_id=-1)
    rows.append(dict(name='empty', input=empty_input, draws=[], leaf_rows=[],
                     native_rect=[0, 0, 0, 0], canvas=initial['canvas'],
                     output=snapshot(u, 65535, 65535), native_traversal=native_traversal))
    return rows, dict(entry='00480A80', cell=[123, 33], initial_data=1, steps=steps,
                      rng_before_sha256=hashlib.sha256(rng_before).hexdigest(),
                      rng_after_sha256=hashlib.sha256(bytes(u.mem_read(scenario + 0x218, 0x3F4))).hexdigest(),
                      boundary='Supplied sparse103x82 map, empty neighbors/shared Dummy, sentinel TMPFFFF, CliffBack0 and no LAT; full original Recalc executes. Radar/tactical dirtiness are transport sinks. Not GoldSt terrain construction or normal harvester scheduling.')


def generate_ore():
    m, types, inputs = prepare(ore=True)
    controls = ore_selector_controls(m)
    rows = [execute(m, types, dict(name='density_4', state=4), ore=True)]
    sequence_rows, sequence = ore_reduction_sequence(m, types)
    rows += sequence_rows
    rows.append(execute(m, types, dict(name='coordinate_variant', coords=[124, 33], state=4), ore=True))
    probe = rows[0]
    x, y, _count, _color, candidate = next(run for run in probe['output']['runs']
                                          if run[3] not in (0, 65535))
    assert 0 < candidate < 65535
    for name, old_z in (('near', candidate + 1), ('equal', candidate), ('far', candidate - 1)):
        row = execute(m, types, dict(name=f'depth_{name}', state=4, old_z=old_z,
                                    clip=[0, y, WIDTH, 1]), ore=True)
        row['depth_probe'] = dict(pixel=[x, y], candidate=candidate,
                                  source='original density_4 raster output')
        rows.append(row)
    assert rows[-3]['output']['runs'] and not rows[-2]['output']['runs'] and not rows[-1]['output']['runs']
    from tools.spatial_oracle.ore_radar import generate as generate_radar
    return dict(native_sha256=NATIVE_SHA256, inputs=inputs,
                surface=dict(width=WIDTH, height=HEIGHT, baseline=32768, bytes_per_pixel=2),
                rows=rows, controls=controls, lifecycle=sequence, radar=generate_radar(m, types))


def ore_metadata():
    return provenance(
        scope='Original flat Riparius Cell body on physical TIB01..12 and global ordinary Convert, reduction1→0→empty, physical current-TMP radar color/dirty/generated stores, and two full weighted radar samplers; bounded RGB565/Z rows and signed image controls. No whole map loader, online radar or normal harvest scheduling claim.',
        assumptions=[
            'Shares physical_assets/prepare/execute/snapshot and the original raster VM with bridge modes. Full original OverlayType constructors/readers and Riparius7216C0/721A50 execute on physical expansion RULESMD, MPBattleMD and GoldSt lexical layers. Original physical ART/image readers bind TEMPERATE art; all12 URBAN aliases and URBAN.PAL are byte-identical. Map pack coordinates/id/data/level are physical inputs, not derived native slope/flags.',
            'Ordinary global87F6B8 executes original Init_Game TEMPERAT.PAL load/expansion52BC49..52BCBB and full basic Convert construction/publication52BE0F..52BE48 through48E740/4BBB00/48EBF0. Source/destination885780, RGB565 surface and53shades. No ISO Cell LightConvert or generated table is injected.',
            'The8rows cover densities4/1/0 and empty at123,33 level5, a124,33 display variant and three one-scanline near/equal/far controls. Cell slope0/flags0/level/projected point, viewport/clip, A127, backgroundFFFF and oldZ are supplied component state. These are not native GoldSt constructed cells. Numeric controls execute47F8D7..47F900 directly with native reader-derived divisor/base and signed16 Cell coordinates.',
            'The same VM executes original480A80 twice with amount1 and full original47D2B0 against supplied sentinel TMPFFFF, sparse103x82 map, empty neighbors/Dummy, CliffBack0 and no LAT. Original queue/bitmap gates and native direction initialization run; both native RNG states are unchanged. Reduction result drives the next raster density. Post-clear native Tactical suffix6D6E5B..6D71D3 itself skips both calls because identity=-1. Its map/admission/rectangle/draw transport remains the existing traversal owner. Actual GoldSt terrain recalc and full harvester scheduler remain with existing consumed terrain_recalc/harvest_field/ore_queue comparisons.',
            'The caller, shape metadata/clipping, selector490E50, rowwalker437A10 and opaque4990E0 execute unchanged. Native pixels/Z supply all output runs; depth control candidates come from original raster output, not a host depth formula. This fixture does not certify GPU submission, shroud composition, all ordinary overlay branches, FoggedObject lifecycle or other theaters.',
            'Additive radar payload runs only after all historical ore fields are captured. Existing shared TMP file transport executes original5447C0/544CB0/547020 on physical clat10.urb, with original URBAN General/ordinal and selected FileName/TilesInSet reads. Sparse selected141 registry and physical neighbor tile/level publications are supplied. Actual authored neighbor overlays/data are recorded separately from supplied empty-overlay live priors, excluding natural patch queue/RNG claims. Original full47D2B0/LAT retains selected141; current ground47C060 reads actual Left metadata and original URBAN brightness1, not a host RGB formula.',
            'Original6551C0/655250/47C060/6547C0 execute with initialized spare dirty-vector storage and explicit173x113 radar geometry/black background, including native raw stores, generated RGB565 pixels and dirty-vector clear. Partial1to0 produces no radar entry; full clear does. Original BSurface construction/locking/writes execute. Separate full6547C0 controls consume the existing coordinate-pattern raw inputs at173x113 and137x149 and retain hashes of every store, without copied sampler math. These are component registrations/surfaces, not whole native radar bounds, viewer/object/fog composition, GPU upload or a normal harvester comparison.',
            'Each radar row also executes original6547C0 for the full raw rectangle on that exact post-dirty buffer and records rebuild_output. The incremental surface bytes are restored, with unchanged surface/header and radar cache assertions, so original output/dirty history is retained. This compares the current Rust full-rebuild strategy directly to native full generation. Original incremental density1 differs in5pixels within3x2/one fringe row (maximum expanded channel differences about25/21/17); exact incremental drawing strategy remains a bounded presentation residual, not a fitted weight correction.',
        ],
        substitutions=[
            'Existing PaletteReader allocation/TLS/archive-resource/RGB565 surface services remain. Full original Cell body, palette generation and raster execute. Historical lifecycle retains Radar6551C0/Tactical6D2790 transport sinks; additive radar executes actual6551C0/655250 and retains only Tactical6D2790 as a sink. No reduction, Recalc, resource queue, color or RNG result is substituted.',
        ],
        entry_points=dict(cell_body=0x47F6A0, flat_selector=0x47F8D7,
                          overlay_type_constructor=0x5FE250, overlay_reader=0x5FE770,
                          tiberium_constructor=0x7216C0, tiberium_reader=0x721A50,
                          ordinary_palette_begin=0x52BC49, ordinary_palette_end=0x52BCBB,
                          ordinary_convert_begin=0x52BE0F, ordinary_convert_end=0x52BE48,
                          convert_constructor=0x48E740, shade_builder=0x4BBB00,
                          reduce_tiberium=0x480A80, recalc=0x47D2B0,
                          radar_mark_dirty=0x6551C0, radar_process_dirty=0x655250,
                          cell_radar_color=0x47C060, radar_generate=0x6547C0,
                          rle_selector=0x490E50, rle_rowwalker=0x437A10,
                          opaque_body_leaf=0x4990E0))


def body_metadata():
    return provenance(
        scope='Eight original high-bridge body caller/raster controls and one adjacent-anchor composition on physical BRIDGE/BRIDGB art, native default Cell LightConvert and strict RGB565/Z leaves; 41 caller-boundary controls including all16 LatinSquare positions. No whole scenario, GPU, snapshot drawing or complete47F6A0 parity.',
        assumptions=[
            'Shares physical_assets, prepare, execute and snapshot with the retained shadow oracle. Original four OverlayType constructors/full rules readers and ART/image binding execute on physical RULESMD/MPBattleMD and ARTMD lexical inputs. No map override, LANGRULE or complete map-loader execution is claimed.',
            'Physical ISOTEM.PAL and TEMPERAT.PAL are supplied at original theater source/destination storage. Original5454DB..5454EB expansion loop executes for each palette, then545000..5450EF constructs/publishes the default ISO-source LightConvert through555DA0/556090 and native blitter initialization. Original483E30 binds it to the Cell and writes explicitly supplied unity lighting/brightness1000. Full scenario lighting, type/tile loading and asynchronous lighting history are excluded.',
            'RGB565 format2 and MMX dispatch bytes are supplied as established by the existing palette owner capture. Actual full-constructor table is compared byte-for-byte to tools.palette_oracle.oracle.palette_table original execution with the same physical expanded source and unity triple. No generated table is injected into the body VM.',
            'Cell current overlay/type, raw state, signed byte level, flags, coordinates, z0-projected point, clip and viewport are explicit component inputs. All raster rows use ordinary0x180 high-anchor flags and valid stock state/orientation/material combinations; no natural producer is claimed for extra boundary flags/signed-level controls.',
            'Original47F6A0,480110,5FDCC0,4AED70, shape metadata/pixels, selector490E50, rowwalker437A10 and opaque4990E0 execute unchanged. controls stop BEFORE4AED70 and therefore make no raster claim. Body dirty cache+64 is explicitly invalidated before each invocation, including repeats, so both repeats reach the original raster.',
            'A127, backgroundFFFF and oldZ are supplied RGB565 surface state. Final color/Z runs are lossless original output. Three one-row near/equal/far oldZ controls derive their candidate from an original probe raster pixel, then rerun the caller; equality/far emit no stores. Full shape crop is retained in the four material/orientation controls. Repeat output remains unchanged after an actual second body raster.',
            'Physical body artwork is temperate; the shared loader also records other theater asset identities but body equivalence across theaters is not claimed. Ordinary0x80-clear fallback is caller-boundary research only; its renderer and FoggedObjectClass4D1AB5 consumer remain separate open branches.',
            'The adjacent-anchor scene reuses OriginalBridgeConstructor success fixture: two original5FC380/Mark/setter calls at16,16 and16,17 on its supplied sparse9x9 map, rawtype25 and synthetic resident Road TMP, with supplied level1. Produced overlay/state/flags cross into the existing physical-body raster VM. Its existing original Tactical suffix6D6E5B..6D71D3 supplies body order and call points with origin16,16/vieworigin-90,375; map lookup/admission/visibility/draws are sinks only for this traversal. Raster executes separately in that observed order on one retained framebuffer/Z. It is not a native map loader or xbayopigs whole-scene claim.',
        ],
        substitutions=[
            'Existing PaletteReader allocator, TLS, archive-resource and RGB565 primary surface transport are retained. Spare native registries and explicit Cell/A/Z/surface state are supplied. No body caller, palette expansion/color generation, shape, selector, rowwalker or opaque pixel leaf is substituted.',
        ],
        entry_points=dict(cell_body=0x47F6A0, cell_draw_offset=0x480110,
                          overlay_draw_offset=0x5FDCC0, shape=0x4AED70,
                          theater_palette_expand=0x5454DB, default_cell_convert=0x545000,
                          default_cell_convert_stop=0x5450EF, cell_bind=0x483E30,
                          light_convert_ctor=0x555DA0, light_convert_colors=0x556090,
                          mmx_rgb565=0x7DE200, rle_selector=0x490E50,
                          rle_rowwalker=0x437A10, opaque_body_leaf=0x4990E0,
                          overlay_constructor=0x5FC380, overlay_mark=0x5FC570,
                          tactical_sweeps_begin=0x6D6E5B, tactical_sweeps_end=0x6D71D3))


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(add_help=False)
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument('--body', action='store_true')
    modes.add_argument('--ore', action='store_true')
    modes.add_argument('--tile', action='store_true')
    modes.add_argument('--placement', action='store_true')
    modes.add_argument('--smudge', action='store_true')
    modes.add_argument('--cliff', action='store_true')
    selected, remaining = parser.parse_known_args()
    if selected.cliff:
        from tools.spatial_oracle.cliff_render import generate as generate_cliff, metadata as cliff_metadata
        finish_vectors(generate_cliff, Path(__file__).with_name('cliff_render.json'),
                       provenance=cliff_metadata, argv=remaining)
        raise SystemExit(0)
    if selected.smudge:
        from tools.spatial_oracle.smudge_render import generate as generate_smudge, metadata as smudge_metadata
        finish_vectors(generate_smudge, Path(__file__).with_name('smudge_render.json'),
                       provenance=smudge_metadata, argv=remaining)
        raise SystemExit(0)
    if selected.placement:
        from tools.spatial_oracle.placement_render import generate as generate_placement, metadata as placement_metadata
        finish_vectors(generate_placement, Path(__file__).with_name('placement_render.json'),
                       provenance=placement_metadata, argv=remaining)
        raise SystemExit(0)
    if selected.tile:
        from tools.spatial_oracle.tmp_clear_render import generate as generate_tile, metadata as tile_metadata
        finish_vectors(generate_tile, Path(__file__).with_name('tmp_clear_render.json'),
                       provenance=tile_metadata, argv=remaining)
        raise SystemExit(0)
    generate_selected = generate_ore if selected.ore else generate_body if selected.body else generate
    path = (Path(__file__).with_name('ore_render.json') if selected.ore else
            Path(__file__).with_name('bridge_body_render.json') if selected.body else
            Path(__file__).with_suffix('.json'))
    metadata_selected = ore_metadata if selected.ore else body_metadata if selected.body else metadata
    finish_vectors(generate_selected, path, provenance=metadata_selected, argv=remaining)
