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
WALL_TYPES = ('CAFNCB', 'CAFNCW', 'CAKRMW')
# Explicit Snow radiation component resource selection. Original theater1
# record7E1BE8 plus formats8295E8/8295F4/827D3C supplies these theater names;
# the selected archived map is a fixture input, not a gameplay decision.
RADIATION_SNOW_SOURCES = dict(map='XXmas.MAP', theater=1, name='SNOW',
    ini='SNOWMD.INI', extension='sno', archive='isosnow.mix',
    source_palette='ISOSNO.PAL', destination_palette='SNOW.PAL')


def ints(u, address, count):
    return list(struct.unpack('<' + 'i' * count, u.mem_read(address, count * 4)))


def physical_assets(*, body=False, ore=False, tile=False, placement=False, radar=False,
                    frontier=False, tmp_names=(), smudge=False, hills=False, ore_natural=False, wall=False,
                    wall_materials=(), radiation=False, tmp_families=(), snow=False, rubble=False, crate=False):
    assert wall or not wall_materials
    assert tile or not tmp_families
    assert not snow or tile or radiation
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
    if wall:
        wall_archives = {'isotemp.mix': ('ctfncb.shp', 'ctfncw.shp', 'ctkrmw.shp'),
                         'cache.mix': ('palette.pal', 'anim.pal', 'temperat.pal', 'isotem.pal', 'unittem.pal')}
        if wall_materials:
            wall_archives['temperat.mix'] = tuple(wall_materials)
        containers = (
            ('ra2.mix', wall_archives),
            ('ra2md.mix', {'localmd.mix': ('ARTMD.INI', 'MPBattleMD.ini', 'TEMPERATMD.INI')}),
            ('multimd.mix', {None: ('XMP03T4.MAP',)}),
        )
    if rubble:
        containers = (
            ('ra2.mix', {'isotemp.mix': ('cthse05.shp',),
                         'temperat.mix': ('srock01.tem',),
                         'cache.mix': ('palette.pal', 'anim.pal', 'temperat.pal', 'isotem.pal')}),
            ('ra2md.mix', {'localmd.mix': ('ARTMD.INI', 'MPBattleMD.ini')}),
            ('multimd.mix', {None: ('XMP03T4.MAP',)}),
        )
    if crate:
        containers = (
            ('ra2.mix', {'temperat.mix': ('crate.tem',),
                         'cache.mix': ('palette.pal', 'anim.pal', 'temperat.pal', 'isotem.pal')}),
            ('ra2md.mix', {'localmd.mix': ('ARTMD.INI', 'MPBattleMD.ini')}),
            ('multimd.mix', {None: ('XMP03T4.MAP',)}),
        )
    if ore_natural:
        assert ore
        containers += (('multimd.mix', {None: ('XMP03T4.MAP',)}),)
    if tile:
        extension = RADIATION_SNOW_SOURCES['extension'] if snow else 'tem'
        archive = RADIATION_SNOW_SOURCES['archive'] if snow else 'isotemp.mix'
        containers = (
            ('ra2.mix', {archive: tuple(tmp_names) or tuple(f'clear01{s}.{extension}' for s in ('', 'a', 'b', 'c', 'd', 'e', 'f', 'g')),
                         'cache.mix': ('isosno.pal', 'snow.pal') if snow else ('isotem.pal', 'temperat.pal'),
                         'conquer.mix': ('SHROUD.SHP',)}),
            ('ra2md.mix', {'localmd.mix': (RADIATION_SNOW_SOURCES['ini'],) if snow else ('temperatmd.ini',)}),
            ('multimd.mix', {None: (RADIATION_SNOW_SOURCES['map'],) if snow else ('XMP03T4.MAP',)}),
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
    if radiation:
        # The radiation component uses this archive/cache owner and the same
        # palette/TMP runtime; it enrolls no unrelated overlay material/type.
        containers = (
            ('ra2.mix', {'cache.mix': ('palette.pal', 'anim.pal', 'isosno.pal', 'snow.pal') if snow
                         else ('palette.pal', 'anim.pal', 'isotem.pal', 'temperat.pal')}),
            ('ra2md.mix', {'localmd.mix': ('ARTMD.INI', 'MPBattleMD.ini', RADIATION_SNOW_SOURCES['ini']) if snow
                           else ('ARTMD.INI', 'MPBattleMD.ini', 'TEMPERATMD.INI')}),
            ('multimd.mix', {None: (RADIATION_SNOW_SOURCES['map'],) if snow else ('XMP03T4.MAP',)}),
        )
    for outer_name, children in containers:
        outer_raw = (root / outer_name).read_bytes()
        outer = mix(outer_raw)
        for inner_name, names in children.items():
            inner_raw = outer[mix_hash(inner_name)] if inner_name is not None else outer_raw
            inner = mix(inner_raw)
            families = {}
            if tile and inner_name == archive and tmp_families:
                names = list(names)
                for base in tmp_families:
                    assert base in names and base.endswith('.' + extension), base
                    family = [base]
                    for suffix in 'abcdefghijklmnopqrstuvwxyz':
                        name = base[:-4] + suffix + '.' + extension
                        if mix_hash(name) not in inner: break
                        family.append(name)
                    else:
                        raise AssertionError('Selected TMP family has no bounded absent suffix')
                    families[base] = dict(names=family, next_absent=name)
                    names.extend(name for name in family if name not in names)
            if tile and inner_name == archive:
                assert mix_hash('clear01h.' + extension) not in inner
            if frontier and inner_name == 'isourb.mix':
                assert mix_hash('clear01h.urb') not in inner
            for name in names:
                raw = inner[mix_hash(name)]
                assets[name.upper()] = raw
                source = dict(name=name, outer=outer_name, inner=inner_name,
                              outer_sha256=hashlib.sha256(outer_raw).hexdigest(),
                              inner_sha256=hashlib.sha256(inner_raw).hexdigest(),
                              sha256=hashlib.sha256(raw).hexdigest(), bytes=len(raw))
                if name in families: source['variant_family'] = families[name]
                sources.append(source)
    if ore or placement or frontier or smudge or wall or radiation or rubble or crate:
        # Active retail's expansion archive overrides the base RULESMD.
        raw = (root / 'expandmd01.mix').read_bytes()
        rules = mix(raw)[mix_hash('RULESMD.INI')]
        assets['RULESMD.INI'] = rules
        sources.append(dict(name='RULESMD.INI', outer='expandmd01.mix', inner=None,
                            outer_sha256=hashlib.sha256(raw).hexdigest(),
                            sha256=hashlib.sha256(rules).hexdigest(), bytes=len(rules)))
        if not smudge and not radiation and not rubble and not crate:
            raw = (root / 'GoldSt.mmx').read_bytes()
            assets['GOLDST.MMX'] = raw
            sources.append(dict(name='GoldSt.mmx', kind='loose', outer=None, inner=None,
                                sha256=hashlib.sha256(raw).hexdigest(), bytes=len(raw)))
    return assets, sources


def prepare(*, body=False, ore=False, placement=False, ore_natural=False, wall=False,
            wall_types=(), wall_materials=(), radiation=False, snow=False, rubble=False, crate=False):
    assert sum((body, ore, placement, wall, radiation, rubble, crate)) <= 1
    assert wall or not (wall_types or wall_materials)
    assert not snow or radiation
    assets, sources = physical_assets(body=body, ore=ore, placement=placement,
        ore_natural=ore_natural, wall=wall, wall_materials=wall_materials, radiation=radiation, snow=snow, rubble=rubble, crate=crate)
    map_filename = 'XMP03T4.MAP' if ore_natural or wall or rubble or crate else 'GoldSt.mmx'
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
    selected_types = ('CRATE',) if crate else ('RUBBLE_OVERLAY',) if rubble else () if radiation else ('GAPOWR',) if placement else (wall_types or WALL_TYPES) if wall else ORE_TYPES if ore else BRIDGE_TYPES
    art, art_lines = lexical(assets['ARTMD.INI'], set(selected_types)
                             | ({'CAHSE05', 'SROCK01'} if rubble else set() if ore or wall or radiation or crate else {'BRIDGE', 'BRIDGB'}))
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
    if crate:
        crate_defaults = [dict(name=name, crate=u.mem_read(types[name]+0x2AA,1)[0])
                          for name in selected_types]
    m.make_ini(art)
    layers = []
    for filename in ('RULESMD.INI', 'MPBattleMD.ini', *((map_filename,) if ore or wall or rubble or crate else ())):
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
    if wall:
        from tools.spatial_oracle.anytown_damage.navigation_inputs import Inputs
        wall_theater = Inputs.read_map_theater(m,assets[map_filename.upper()])
        inputs_theater = initialize_overlay_theater(m,wall_theater['value'])
        image_states = []
        for name in selected_types:
            pointer=types[name]
            mark = len(m.asset_loaded)
            getter = m.read32(m.read32(pointer)+0x9C)
            image = m.invoke(getter,pointer)
            if wall_types or wall_materials:
                assert getter == 0x5FEDE0 and image == m.read32(pointer+0xA4)
                image_states.append(dict(name=name, getter=f'{getter:08X}',
                    image=m.string(pointer+0x1F8), image_present=bool(image),
                    theater=u.mem_read(pointer+0x22C,1)[0],
                    new_theater=u.mem_read(pointer+0x237,1)[0],
                    flag_2af=u.mem_read(pointer+0x2AF,1)[0],
                    getter_loads=m.asset_loaded[mark:]))
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
    if crate:inputs['constructor_flags']=crate_defaults
    if wall:inputs.update(overlay_theater=inputs_theater,theater=wall_theater)
    if wall_types or wall_materials:inputs['wall_images']=image_states
    if body or wall or radiation or rubble or crate:
        if snow:
            from tools.spatial_oracle.anytown_damage.navigation_inputs import Inputs
            theater = Inputs.read_map_theater(m, assets[RADIATION_SNOW_SOURCES['map'].upper()])
            assert theater['source_value'] == RADIATION_SNOW_SOURCES['name']
            assert theater['value'] == RADIATION_SNOW_SOURCES['theater']
            inputs['theater'] = theater
            inputs['cell_convert'] = initialize_cell_convert(m,
                source_palette=RADIATION_SNOW_SOURCES['source_palette'],
                destination_palette=RADIATION_SNOW_SOURCES['destination_palette'])
        else:
            inputs['cell_convert'] = initialize_cell_convert(m)
    if ore:
        inputs['ordinary_convert'] = ordinary_convert_state(m)
        inputs['tiberium'] = prepare_tiberium(m, assets, map_filename=map_filename)
        from tools.spatial_oracle.shrapnel_repair.map_facts import decode_cells
        parsed, cells = decode_cells(assets[map_filename.upper()])
        inputs['map'] = dict(name=map_filename, physical_map_section=parsed['Map'],
                             cells=[dict(coords=list(xy), **cells[xy])
                                    for xy in (((72, 25),) if ore_natural else ((123, 33), (124, 33)))],
                             boundary='Authored pack data only; live slope0/flags0 are supplied component inputs, not native map-loader results.')
        assert all(assets[f'{name}.TEM'] == assets[f'{name}.URB'] for name in ORE_TYPES)
        assert assets['TEMPERAT.PAL'] == assets['URBAN.PAL']
        inputs['urban_aliases'] = dict(images=list(ORE_TYPES), byte_identical=True,
                                       palettes=['TEMPERAT.PAL', 'URBAN.PAL'])
    return m, types, inputs


def initialize_overlay_theater(m, theater):
    """Original4AD065..71 producer call on the existing populated registry.

    The caller supplies its independently read Scenario theater. The original
    full5FE620 produces filenames, theater/generic fallback and Image stores;
    archive transport remains Reader's existing physical byte owner.
    """
    mark = len(m.asset_loaded)
    m.invoke(0x5FE620, theater)
    return dict(entry='005FE620', caller='004AD065..004AD071', theater=theater,
                loads=m.asset_loaded[mark:])


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


def prepare_tiberium(m, assets, *, map_filename='GoldSt.mmx'):
    """Full original Tiberium constructor and physical selected layer readers."""
    u = m.u
    u.mem_write(0xB0F4E8, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
    typ = m.alloc(0x200)
    m.invoke(0x7216C0, typ, (m.cstring('Riparius'),))
    layers = []
    for filename in ('RULESMD.INI', 'MPBattleMD.ini', map_filename):
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


def execute(m, types, case, *, body=False, ore=False, wall=False, shadow=False,
            boundary_only=False, reset_surface=True, allow_empty_frame=False, rubble_type=None, crate=False):
    assert wall or crate or rubble_type is not None or not shadow
    assert body or ore or wall or not boundary_only
    case = dict(type='TIB01' if ore else 'BRIDGE1',
                coords=[123, 33] if ore else [10, 20], level=5 if ore else 0, state=0,
                flags=0 if ore else 0x180,
                cell_point=[66, 141] if ore else [66, 81], clip=[0, 0, WIDTH, HEIGHT],
                viewport_y=0, background=0xFFFF, old_z=65535, repeat=1) | case
    u = m.u
    typ = types[case.get('type', 'BRIDGE1')]
    shp = m.read32(typ + 0xA4)
    assert shp, 'Physical image must bind through original reader'
    u.mem_write(CELL + 0x44, dwords(case.get('overlay_id', m.read32(typ + 0x294))))
    if rubble_type is not None:
        u.mem_write(CELL + 0x40, dwords(0 if case.get('clear_rubble_type') else rubble_type))
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
    if (wall or crate or rubble_type is not None) and case.get('overlay_id') == -1:
        #47F6A0 requires a valid type. Reuse the existing actual Tactical
        # caller's -1 admission, as the historical ore empty control does.
        native_traversal=traversal(m,origin=tuple(case['coords']),require_draws=False)
        assert native_traversal['calls']==[]
        return dict(name=case['name'],input=case,draws=[],leaf_rows=[],
                    native_rect=[0,0,0,0],native_traversal=native_traversal,
                    output=snapshot(u,background,old_z))
    draws, rows, reached, steps = [], [], set(), []
    leaves = (BODY_LEAVES | {0x4978C0: 'rle_alpha_body'}) if wall and not shadow else BODY_LEAVES if body or ore else LEAVES
    entry = 0x47F6A0 if (body or ore or wall) and not shadow else 0x47F510

    def observe(_u, address, _size, _data):
        if address in (entry, 0x480110, 0x5FDCC0, 0x4AED70, 0x69E7E0,
                       0x69E740, 0x69E900, 0x490E50, 0x437A10, 0x45F160, 0x45F1D0, *leaves):
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
        if body or ore or wall:
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
        if (body or ore or wall or crate or rubble_type is not None) and not boundary_only and case['repeat'] > 1:
            steps.append(snapshot(u, background, old_z))
    u.hook_del(hook)
    if rubble_type is not None and not draws:
        assert case.get('expect_no_draw'), (case['name'], sorted(reached))
        assert not rows
        return dict(name=case['name'], input=case, draws=[], leaf_rows=[],
                    reached=[f'{p:08X}' for p in sorted(reached)],
                    output=snapshot(u, background, old_z))
    required = {entry, 0x480110, 0x5FDCC0, 0x4AED70}
    if rubble_type is not None:
        required.discard(0x5FDCC0)
        required.add(0x45F160 if body else 0x45F1D0)
        shp = draws[0]['raw_args'][0]
    if not boundary_only:
        required |= {0x490E50, 0x437A10, 0x4990E0 if body or ore or wall else 0x497390}
    if wall:
        required.discard(0x4990E0)
        if not allow_empty_frame:
            assert any(p in reached for p in leaves)
    # Only the opt-in low bridge component accepts a native empty SHP crop.
    # A bound physical image and actual Shape call are still mandatory; zero
    # crop is read by original69E7E0, never manufactured by the host.
    if allow_empty_frame:
        assert draws
        m.invoke(0x69E7E0, shp, (RECT, draws[0]['frame']))
        if ints(u, RECT, 4)[2:] == [0, 0]:
            required -= {0x490E50, 0x437A10, 0x4990E0, 0x497390}
            assert not rows and not any(p in reached for p in leaves)
        else:
            assert any(p in reached for p in leaves)
    if (rubble_type is not None or crate) and case.get('expect_clipped'):
        assert not rows
        required -= {0x490E50, 0x437A10, 0x497390}
    assert draws and required.issubset(reached), (case['name'], sorted(required-reached))
    if ore:
        # The original coordinate selector can bind another image than the
        # gameplay Cell's live overlay identity. Observe its actual pointer.
        shp = draws[0]['raw_args'][0]
        typ = next(pointer for pointer in types.values() if m.read32(pointer + 0xA4) == shp)
    m.invoke(0x69E7E0, shp, (RECT, draws[0]['frame']))
    result = dict(name=case['name'], input=case, draws=draws, leaf_rows=rows,
                  image_name=m.string((rubble_type or typ) + 0x1F8),
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


def traversal(m, *, origin=(10, 20), view_origin=(0, 0), require_draws=True,
              native_cells=None, native_tactical=None, native_dummy=None, native_rectangles=True):
    """Original Tactical loop order; map/visibility and draw calls are sinks."""
    u = m.u
    tactical = m.alloc(0xE20) if native_tactical is None else native_tactical
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
    rectangles, pending = [], {}
    if native_cells is not None:
        assert native_dummy is not None and m.read32(native_dummy + 0x44) == 0xFFFFFFFF
        u.mem_write(0x887324, dwords(tactical))

    def observe(_u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address in pending:
            piece, output, xy = pending.pop(address)
            rectangles.append(dict(piece=piece, coords=xy, rect=ints(u,output,4)))
        if address == 0x568300:
            m.ret(1, 4)
        elif address == 0x5657A0:
            coords[:] = struct.unpack('<2h', u.mem_read(m.read32(sp + 4), 4))
            if native_cells is None:
                u.mem_write(CELL + 0x24, struct.pack('<2h', *coords))
                m.ret(CELL, 4)
            else:
                m.ret(native_cells.get(tuple(coords),native_dummy),4)
        elif address in (0x47FB90, 0x47FDE0):
            output = m.read32(sp + 4)
            if native_cells is None or not native_rectangles:
                u.mem_write(output, dwords(0, 0, WIDTH, HEIGHT))
                m.ret(output, 4)
            else:
                pending[m.read32(sp)] = ('body' if address == 0x47FB90 else 'shadow',output,list(coords))
        elif address in (0x47F6A0, 0x47F510):
            calls.append(dict(piece='body' if address == 0x47F6A0 else 'shadow',
                              coords=list(coords), point=ints(u, m.read32(sp + 4), 2)))
            m.ret(0, 8)

    hook = u.hook_add(UC_HOOK_CODE, observe)
    run_checked(u, 0x6D6E5B, 0x6D71D3, count=200000,
                required_addresses=((0x6D7001, 0x6D7031, 0x6D719F) if require_draws
                                    else (0x6D6EDA, 0x6D7031)))
    u.hook_del(hook)
    result = dict(entry='006D6E5B', end='006D71D3', origin=list(origin),
                  max_diagonal=2, cells_per_diagonal=3, calls=calls)
    if native_cells is not None:
        assert not pending
        result.update(rectangles=rectangles,view_origin=list(view_origin),
            boundary='Supplied sparse selected Cell lookup/native-constructed empty Dummy and map visibility admission. Original47FB90/47FDE0/6D2140/rectangle intersection execute; actual admitted body/shadow calls are recorded sinks.')
    return result


def prepare_crate():
    """Original stock CRATE readers and physical theater image in this VM."""
    m, types, inputs = prepare(crate=True)
    u = m.u
    inputs['overlay_theater'] = initialize_overlay_theater(m, 0)
    typ = types['CRATE']
    getter = m.read32(m.read32(typ)+0x9C)
    image = m.invoke(getter, typ)
    raw = m.assets['CRATE.TEM']
    assert image == m.asset_ptr['CRATE.TEM']
    assert bytes(u.mem_read(image, len(raw))) == raw
    inputs['crate_overlay'] = dict(name='CRATE', index=m.read32(typ+0x294),
        image=m.string(typ+0x1F8), image_getter=f'{getter:08X}',
        raw_frame_count=struct.unpack('<h',u.mem_read(image+6,2))[0],
        crate=u.mem_read(typ+0x2AA,1)[0], wall=u.mem_read(typ+0x2A8,1)[0],
        is_rubble=u.mem_read(typ+0x2B4,1)[0])
    assert inputs['crate_overlay']['index'] == 179
    assert inputs['crate_overlay']['raw_frame_count'] == 2
    assert inputs['crate_overlay']['crate'] == 1
    assert inputs['crate_overlay']['wall'] == inputs['crate_overlay']['is_rubble'] == 0
    assert inputs['constructor_flags'] == [dict(name='CRATE',crate=0)]
    assert not (configured_gamemd().parent/'LANGRULE.INI').exists()
    inputs['langrule_absent'] = True
    return m, types, inputs


def generate_crate():
    m, types, inputs = prepare_crate()
    u = m.u
    # Coordinate/level match the separately observed ordinary stock crate.
    # Projection, visibility, Mark/placement and pickup remain component priors.
    base = dict(type='CRATE', coords=[52,120], level=4, flags=0, state=255,
                cell_point=[66,130])
    cases = [dict(name='live_ff'), dict(name='data_zero',state=0),
             dict(name='level_zero',level=0), dict(name='level_six',level=6),
             dict(name='level_minus_one',level=-1),
             dict(name='left_clip',cell_point=[-45,130]),
             dict(name='all_clipped',cell_point=[-200,130],expect_clipped=True),
             dict(name='repeat',repeat=2)]
    rows = [execute(m, types, base|case, shadow=True, crate=True) for case in cases]
    positive = rows[0]
    assert positive['draws'][0]['frame'] == 1
    assert positive['draws'][0]['flags'] == 0x4601
    assert rows[1]['draws'] == positive['draws']
    assert rows[1]['output'] == positive['output']
    point, canvas = positive['draws'][0]['point'], positive['canvas']
    canvas_top = [point[i]-canvas[i]//2 for i in range(2)]
    shape = dict(image_name=positive['image_name'],frame=1,canvas=canvas,
        native_rect=positive['native_rect'],
        mask_runs=[[x-canvas_top[0],y-canvas_top[1],n]
                   for x,y,n,_color,_depth in positive['output']['runs']])
    assert positive['native_rect'] == [29,37,30,18]
    assert sum(run[2] for run in shape['mask_runs']) == 199
    sample = next(row for row in positive['leaf_rows'] if row['count'])
    candidate, scan_y = sample['candidate'],sample['y']
    for delta,name in ((-1,'depth_far'),(0,'depth_equal'),(1,'depth_near')):
        rows.append(execute(m,types,base|dict(name=name,old_z=candidate+delta,
            clip=[0,scan_y,WIDTH,1]),shadow=True,crate=True))
    # Original rectangle/real Tactical intersection must ALSO ignore rawFF.
    execute(m,types,base|dict(name='outer_setup'),shadow=True,crate=True)
    outer = live_overlay_traversals(m,base,crate=True)
    assert outer['positive'] == outer['supplied_zero_data']
    rows.append(execute(m,types,base|dict(name='cleared_overlay',overlay_id=-1),
                        shadow=True,crate=True))
    # Current Cell Crate branch forces body0 independently of retained rawFF.
    body = execute(m,types,base|dict(name='crate_body'),body=True,crate=True)
    combined = execute(m,types,base|dict(name='body_then_shadow'),shadow=True,
                       crate=True,reset_surface=False)
    assert body['draws'][0]['frame'] == 0
    assert body['draws'][0]['point'] == positive['draws'][0]['point']
    assert combined['draws'] == positive['draws']
    return dict(native_sha256=NATIVE_SHA256,inputs=inputs,shapes=[shape],
        surface=dict(width=WIDTH,height=HEIGHT,baseline=32768,bytes_per_pixel=2),
        rows=rows,outer_admission=outer,
        composition=dict(body=body,body_then_shadow=combined))


def crate_metadata():
    return provenance(scope='Bounded original ordinary CRATE Cell47F6A0 body0 and47F510 shadow1 on physical CRATE.TEM, including same-surface body then shadow; actual47FDE0 bounds/Tactical intersection with real rawFF versus supplied0, signed levels/clip/depth/repeat/erased identity. No full native map, Mark, RNG, pickup or scene claim.',
        assumptions=[
            'Existing archive/lexical INI/PaletteReader/Overlay constructor/full reader/theater binding/Cell LightConvert/shape/raster/surface owners are reused, with no second VM, decoder or expected pixel math added. Physical expansion RULESMD, MPBattleMD and stock XMP03T4 layers execute for CRATE; fixed ARTMD supplies Theater=yes. Optional LANGRULE is physically absent. Original constructor Cratefalse and each native reader retained flag/image are recorded.',
            'Cell52,120, level4, flags0, rawFF and liveCRATE179 match separately observed ordinary stock production; local projected point66,130, clip192x160/viewport0, A127, unity Cell brightness1000, backgroundFFFF and oldZFFFF are explicit component priors. Original scenario creation/Mark/pickup/RNG/timers/slot lifecycle are excluded. Mark5FD105 instruction-level evidence establishes rawFF; this component does not execute that producer.',
            'Original47F510 Crate+2AA arm precedes rubble and forces selected data0 before signed image half+data; original47FDE0 independently forces0 before actual shape bounds. RawFF versus supplied0 must produce identical draw args, pixels/depth and outer Tactical admission. Original47F6A0 crate arm separately forces bodyframe0. No hybrid overlay flags are represented as stock.',
            'Actual47FDE0 bounds, projection and Tactical intersection run with existing supplied sparse lookup/map admission and draw sinks. Erased identity uses original Tactical live overlay admission, never a direct -1 Cell caller. Original shadow stencil199 comes from admitted native output, not a host source-image decoder; composed visible changes are observed separately and need not equal199.',
            'Signed0/6/-1 levels, left/all clipping, repeat and one-row near/equal/far oldZ are supplied controls; candidate/scanline come from original positive leaf arguments. Actual body0 then shadow1 execute on SAME color/Z/A and physical CellConvert without reset. No body/shadow math or composed-pixel rule is injected. No gameplay RNG draw, gameplay timer write or detach call occurs in this rendering component.',
        ],substitutions=[
            'Existing allocator/TLS/archive-resource transport and supplied scene/Cell/A/Z/surface priors only. Original reached Cell body/shadow/draw-offset, shape/getters/selector/rowwalker/pixel leaves execute unchanged. Only the Tactical suffix uses its documented map admission/sparse lookup/draw recording transports; original rectangles/projection/intersection execute.',
        ],entry_points=dict(cell_body=0x47F6A0,cell_shadow=0x47F510,
            shadow_bounds=0x47FDE0,cell_draw_offset=0x480110,shape=0x4AED70,
            overlay_constructor=0x5FE250,overlay_reader=0x5FE770,theater_binding=0x5FE620,
            rle_selector=0x490E50,rowwalker=0x437A10,body_leaf=0x4990E0,
            shadow_leaf=0x497390,tactical_sweeps_begin=0x6D6E5B,tactical_sweeps_end=0x6D71D3))


def prepare_rubble():
    """CAHSE05 original readers in the existing Cell/raster VM.

    Native foundation/Rules/Building registry prerequisites follow the shared
    placement component. No separate reader/VM or host SHP decoder is added.
    """
    m, types, inputs = prepare(rubble=True)
    u = m.u
    inputs['overlay_theater'] = initialize_overlay_theater(m, 0)
    overlay = types['RUBBLE_OVERLAY']
    overlay_image = m.invoke(m.read32(m.read32(overlay) + 0x9C), overlay)
    assert overlay_image == m.asset_ptr['SROCK01.TEM']
    assert u.mem_read(overlay + 0x2AA, 1) == b'\0'
    assert u.mem_read(overlay + 0x2B4, 1) == b'\1'
    inputs['rubble_overlay'] = dict(index=m.read32(overlay+0x294),
        image=m.string(overlay+0x1F8), raw_frame_count=struct.unpack('<h',u.mem_read(overlay_image+6,2))[0],
        is_rubble=u.mem_read(overlay+0x2B4,1)[0], crate=u.mem_read(overlay+0x2AA,1)[0])
    m.invoke(0x45B1C0, 0)
    rules = m.alloc(0x5000)
    u.mem_write(0x8871E0, dwords(rules))
    m.invoke(0x665650, rules)
    m.invoke(0x4E7CF0, 0)
    typ = m.alloc(0x2000)
    m.invoke(0x45DD90, typ, (m.cstring('CAHSE05'),))
    def state():
        image = m.read32(typ + 0xA4)
        return dict(leave_rubble=u.mem_read(typ + 0x1768, 1)[0],
                    custom_rubble_present=bool(m.read32(typ + 0x14F4)),
                    image=m.string(typ + 0x1F8), image_present=bool(image),
                    frame_count=struct.unpack('<h', u.mem_read(image + 6, 2))[0] if image else None)
    inputs['building'] = dict(name='CAHSE05', constructor=state(), layers=[])
    assert not (configured_gamemd().parent / 'LANGRULE.INI').exists()
    inputs['building']['langrule_absent'] = True
    for filename in ('RULESMD.INI', 'MPBattleMD.ini', 'XMP03T4.MAP'):
        sections, lines = lexical(m.assets[filename.upper()], {'CAHSE05'})
        m.rules_cache(sections)
        mark = len(m.asset_loaded)
        admitted = m.invoke(0x45FE50, typ, (RULES,)) & 255
        inputs['building']['layers'].append(dict(file=filename, admitted=admitted,
            physical_keys=sections.get('CAHSE05'), source_lines=lines,
            retained=state(), loads=m.asset_loaded[mark:]))
    assert state()['leave_rubble'] == 1 and not state()['custom_rubble_present']
    getter = m.read32(m.read32(typ) + 0x9C)
    image = m.invoke(getter, typ)
    raw = m.assets['CTHSE05.SHP']
    assert image and bytes(u.mem_read(image, len(raw))) == raw
    assert hashlib.sha256(raw).hexdigest() == 'ebc7b1bb38fb8028d442e10facb2edb3799b9b1e470c436134514ab588f9f0fb'
    inputs['building'].update(image_getter=f'{getter:08X}', effective=state())
    return m, types, typ, inputs


def live_overlay_traversals(m, case, *, crate=False):
    """Existing real Tactical suffix, using actual SROCK shadow bounds.

    Supplied sparse lookup/admission and draw sinks are the existing traversal
    boundary. Actual Cell bounds, projection and rectangle intersection run.
    """
    u = m.u
    dummy = m.alloc(0x148)
    m.invoke(0x47BBF0, dummy)
    tactical = m.alloc(0xE20)
    cells = {tuple(case['coords']): CELL}
    projection = traversal(m, origin=tuple(case['coords']), require_draws=False,
        native_cells=cells, native_dummy=dummy, native_tactical=tactical,
        native_rectangles=False)
    point = next(call['point'] for call in projection['calls'] if call['piece']=='shadow')
    view = tuple(point[i]-case['cell_point'][i] for i in range(2))
    positive = traversal(m, origin=tuple(case['coords']), view_origin=view,
        require_draws=False, native_cells=cells, native_dummy=dummy, native_tactical=tactical)
    assert any(call['piece']=='shadow' for call in positive['calls'])
    # Normal rubble retains data0; real crate Mark5FD105 writesFF. The
    # alternate byte is a supplied contrast, never an inferred producer.
    saved = bytes(u.mem_read(CELL+0x11E,1))
    u.mem_write(CELL+0x11E,b'\0' if crate else b'\xff')
    invalid = traversal(m, origin=tuple(case['coords']), view_origin=view,
        require_draws=False, native_cells=cells, native_dummy=dummy, native_tactical=tactical)
    u.mem_write(CELL+0x11E,saved)
    assert any(call['piece']=='shadow' for call in invalid['calls']) == crate
    return dict(positive=positive, **({'supplied_zero_data':invalid} if crate else
                {'supplied_out_of_range_data':invalid}),
                projection_point_at_zero_view=point)


def generate_rubble():
    m, types, typ, inputs = prepare_rubble()
    u = m.u
    base = dict(type='RUBBLE_OVERLAY', coords=[68, 66], level=5, flags=0, state=0,
                cell_point=[66, 130])
    cases = [dict(name='own_image'), dict(name='level_zero', level=0),
             dict(name='level_six', level=6), dict(name='level_minus_one', level=-1),
             dict(name='left_clip', cell_point=[-5, 130]),
             dict(name='dirty_clip', clip=[65, 45, 30, 20]),
             dict(name='all_clipped', cell_point=[-200, 130], expect_clipped=True),
             dict(name='repeat', repeat=2)]
    rows = [execute(m, types, base | case, shadow=True, rubble_type=typ) for case in cases]
    positive = rows[0]
    assert positive['draws'][0]['frame'] == 7 and positive['draws'][0]['flags'] == 0x4601
    assert positive['native_rect'] == [12, 43, 74, 37]
    point, canvas = positive['draws'][0]['point'], positive['canvas']
    canvas_top = [point[i] - canvas[i] // 2 for i in range(2)]
    shape = dict(image_name=positive['image_name'], frame=7, canvas=canvas,
                 native_rect=positive['native_rect'],
                 mask_runs=[[x-canvas_top[0], y-canvas_top[1], n]
                            for x, y, n, _color, _depth in positive['output']['runs']])
    assert sum(run[2] for run in shape['mask_runs']) == 129
    sample = next(row for row in positive['leaf_rows'] if row['count'])
    candidate, scan_y = sample['candidate'], sample['y']
    for delta, name in ((-1, 'depth_far'), (0, 'depth_equal'), (1, 'depth_near')):
        rows.append(execute(m, types, base | dict(name=name, old_z=candidate+delta,
            clip=[0, scan_y, WIDTH, 1]), shadow=True, rubble_type=typ))
    for name, offset, value in (('leave_rubble_false', 0x1768, b'\0'),
                                ('custom_rubble_false', 0x14F4, dwords(m.read32(typ+0xA4)))):
        saved = bytes(u.mem_read(typ+offset, len(value)))
        u.mem_write(typ+offset, value)
        rows.append(execute(m, types, base | dict(name=name, expect_no_draw=True),
                            shadow=True, rubble_type=typ))
        u.mem_write(typ+offset, saved)
    rows.append(execute(m, types, base | dict(name='cleared_type',
        clear_rubble_type=True, expect_no_draw=True), shadow=True, rubble_type=typ))
    # Restore normal input before observing the actual outer draw admission.
    execute(m, types, base | dict(name='outer_setup'), shadow=True, rubble_type=typ)
    outer = live_overlay_traversals(m, base)
    rows.append(execute(m, types, base | dict(name='cleared_overlay', overlay_id=-1),
                        shadow=True, rubble_type=typ))
    # Required same-cell dependency: original480110's literal overlayEF arm
    # subtracts15Y for both body and shadow. Retain actual47F6A0/45F160 body,
    # then47F510/45F1D0 on the SAME physical ISOConvert and color/Z/A surface.
    # No copied body math, pixel decoder or host-composition rule is supplied.
    body = execute(m, types, base | dict(name='rubble_body'), body=True, rubble_type=typ)
    combined = execute(m, types, base | dict(name='body_then_shadow'), shadow=True,
                       rubble_type=typ, reset_surface=False)
    assert body['draws'][0]['frame'] == 3
    assert body['draws'][0]['point'] == positive['draws'][0]['point']
    assert combined['draws'] == positive['draws']
    return dict(native_sha256=NATIVE_SHA256, inputs=inputs, shapes=[shape],
                surface=dict(width=WIDTH, height=HEIGHT, baseline=32768, bytes_per_pixel=2),
                rows=rows, outer_admission=outer,
                composition=dict(body=body, body_then_shadow=combined))


def rubble_metadata():
    return provenance(scope='Bounded original Cell47F6A0/BuildingType45F160 body frame3 followed by Cell47F510/BuildingType45F1D0 shadow frame7 on the same surface, sharing Cell480110 on physical CAHSE05/CTHSE05; actual SROCK01.TEM outer Cell shadow bounds/Tactical intersection; original RGB565/Z raster and supplied level/clip/depth/admission/clear controls. No native building death, whole map load, GPU or scene claim.',
        assumptions=[
            'Existing physical archive, lexical INI, PaletteReader, full native constructors/readers, Cell Convert, SHP metadata/selector/rowwalker/leaves and RGB565/Z surface owners are reused. No host image decoder or expected raster arithmetic. Physical RULESMD, MPBattleMD and XMP03T4 CAHSE05 and RUBBLE_OVERLAY layers execute; selected optional LANGRULE is physically absent; ART is fixed.',
            'Original BuildingType constructor and full reader run in the existing raster VM with native foundation/Rules/registry prerequisites used by placement_render.prepare. Actual own-image getter and physical CTHSE05 bytes must match. Cell overlay/type pointer/data0, signed level5, ground projected point, clip, prior color/Z and A127 are supplied component inputs. Level0/6/-1 and admission fields are explicit contrasts. Fatal damage/rubble publication/map initialization remain separate evidence.',
            'Actual RUBBLE_OVERLAY binds Image=SROCK01, ART Theater=yes and physical SROCK01.TEM. Its two-frame image and frame1 nonempty crop supply the ORIGINAL47FDE0 outer shadow bounds. Original Tactical suffix executes these actual bounds, projection and rectangle intersection; map-bound admission/sparse lookup/draw sinks retain existing traversal substitutions. A supplied data255 control asks out-of-range frame256, produces empty bounds and no shadow call. Production data0 must be separately observed, never inferred from these priors.',
            'Original47F510 calls45F1D0: LeaveRubble, nonnull Cell type, own-image count half greater than3 select half+3. CustomRubble writes outputs but returns false. Selected admission field mutations are supplied contrasts, not alternate retail reader results. Erased overlay uses native Tactical admission, never a -1 direct caller.',
            'Additional same-surface composition executes original47F6A0/45F160 body frame3 on fresh color/Z/A, then original47F510/45F1D0 shadow frame7 without resetting the framebuffer or changing the physical ISO Cell Convert. Actual body and shadow draw points must agree; both include original480110 literal-overlayEF minus15Y admission. Actual body/combined color and depth outputs are retained. There is no host image decoder, body math or composed-pixel rule.',
            'Positive stencil runs come from native output at oldZ65535. Near/equal/far candidate and one-scanline clip come from an original positive leaf. Original calls and shape args/crop, signed comparison, destination darkening, depth stores, repeat and clipping run unchanged. No RNG draw, gameplay timer write or detach call belongs to this selected rendering component.',
        ], substitutions=['Existing allocator/TLS/archive-resource services and supplied scene/Cell/A/Z/surface priors. No reached Cell, BuildingType rubble getter, shape/selector/rowwalker or shadow leaf is substituted. Tactical suffix uses supplied map admission/sparse Cell lookup and actual draw sinks, while native bounds/projection/intersection execute.'],
        entry_points=dict(cell_shadow=0x47F510, rubble_shadow=0x45F1D0,
            shadow_bounds=0x47FDE0, cell_draw_offset=0x480110, shape=0x4AED70,
            building_constructor=0x45DD90, building_reader=0x45FE50,
            cell_body=0x47F6A0, rubble_body=0x45F160,
            rle_selector=0x490E50, rowwalker=0x437A10,
            plain_shadow_leaf=0x493830, rle_shadow_leaf=0x497390,
            tactical_sweeps_begin=0x6D6E5B, tactical_sweeps_end=0x6D71D3))


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


def prepare_selected_tiles(m, assets, inputs, selected, *, variant_tile=None, snow=False):
    """One selected-theater/TMP/lighting owner for Cell raster components."""
    from tools.spatial_oracle import tmp_clear_render as tile
    u = m.u
    initial = selected[0][1]
    theater_ini = RADIATION_SNOW_SOURCES['ini'] if snow else 'TEMPERATMD.INI'
    extension = RADIATION_SNOW_SOURCES['extension'] if snow else 'tem'
    map_name = RADIATION_SNOW_SOURCES['map'] if snow else 'XMP03T4.MAP'
    # Same original theater reader and TMP loader, with one sparse registry
    # entry per actual selected tile. Different wall cells need not share TMP.
    tiles, mapping = tile.theater_tile_files(m, assets[theater_ini], initial['tile'], extension)
    tile_ids = list(dict.fromkeys(cell['tile'] for _,cell in selected))
    mappings = [mapping]
    for tile_id in tile_ids:
        if tile_id != initial['tile']:
            other_tiles, other_mapping = tile.theater_tile_files(m, assets[theater_ini],tile_id,extension)
            assert other_tiles == tiles
            mappings.append(other_mapping)
    filenames = tuple(dict.fromkeys(tiles[tile_id] for tile_id in tile_ids))
    filename = tiles[initial['tile']]
    assert variant_tile is None or variant_tile in tile_ids
    families = () if variant_tile is None else (tiles[variant_tile],)
    extra, extra_sources = physical_assets(tile=True, tmp_names=filenames, tmp_families=families, snow=snow)
    variant_families = {row['name']: row['variant_family']['names'] for row in extra_sources
                        if 'variant_family' in row}
    if variant_families:
        inputs['tmp_families'] = {row['name']: row['variant_family'] for row in extra_sources
                                 if 'variant_family' in row}
        filenames = tuple(dict.fromkeys((*filenames, *(name for family in variant_families.values() for name in family))))
    m.assets.update(extra)
    inputs['sources'].extend(row for row in extra_sources if row['name'].lower() in filenames)
    for entry in (0x7C5F3D,0x7C5FFF): m.invoke(entry,0)
    m.phase='authored_fence_tmp_load' # Shared physical-file receipt label.
    if variant_families:
        heads, file_io = tile.load_tmp_files(m,{name:extra[name.upper()] for name in filenames},
                                            variant_families=variant_families)
    else:
        heads, file_io = tile.load_tmp_files(m,{name:extra[name.upper()] for name in filenames})
    registry = m.alloc(len(tiles)*4)
    for tile_id in tile_ids:u.mem_write(registry+tile_id*4,dwords(heads[tiles[tile_id]]))
    u.mem_write(0xA8ED2C,dwords(registry));u.mem_write(0xA8ED38,dwords(len(tiles)))
    for entry in (0x6D1830,0x6D18C0,0x6D1BB0):m.invoke(entry,0)
    lighting = tile.profile(m,assets[map_name.upper()])
    return tiles, mapping, mappings, filenames, filename, heads, file_io, lighting


def prepare_lit_cell(m, cell, head, lighting, name):
    """Original Cell constructor/coordinate, TMP metadata and480350 lighting."""
    from tools.spatial_oracle import tmp_clear_render as tile
    from tools.spatial_oracle.bridge_recalc_presentation import SELECTOR_FLAG
    u = m.u
    m.invoke(0x47BBF0,CELL)
    pair=m.alloc(4);u.mem_write(pair,struct.pack('<hh',*cell['cell']))
    m.invoke(0x485240,CELL,(pair,))
    u.mem_write(CELL+0x38,dwords(cell['tile']))
    slope=m.invoke(0x5471B0,head,(cell['subtile'],))&255
    land=m.invoke(0x544BE0,head,(cell['subtile'],))
    assert slope==cell['slope']
    u.mem_write(CELL+0x11A,bytes((cell['subtile'],cell['level'],slope,0)))
    u.mem_write(CELL+0xEC,dwords(land))
    u.mem_write(SELECTOR_FLAG,b'\1')
    # Original Cell lighting/TMP path produces top +10A and common +10C.
    # The ordinary low-bridge branch consumes common +10C. Discard this
    # framebuffer before the separate overlay caller component executes.
    floor=tile.execute(m,dict(name=name+'_lighting',coords=cell['cell'],level=cell['level'],
        cell_point=[96,120],scene_lighting=lighting),current_cell=CELL,
        current_cells={tuple(cell['cell']):CELL})
    return floor


def generate_authored_fence(*, cpu_filename='authored_fence.json', wall_name='CAFNCB',
                            selected_states=(), wall_materials=()):
    """Native wall states through the same physical Cell/material/raster owner.

    Optional selections name paths into independent CPU histories; they do not
    write new wall states or prescribe colors. Default CAFNCB output is retained.
    """
    import json
    from tools.spatial_oracle.shrapnel_repair.map_facts import decode_cells
    from tools.projectile_oracle.bridge_render_inputs_palette import color_scheme_palette
    cpu_path = Path(__file__).with_name('anytown_damage').joinpath(cpu_filename)
    cpu = json.loads(cpu_path.read_text())
    history = cpu['cases'][0]
    wall_types = (wall_name,) if selected_states else ()
    m, types, inputs = prepare(wall=True,wall_types=wall_types,wall_materials=wall_materials)
    u = m.u
    assets, _ = physical_assets(wall=True,wall_materials=wall_materials)
    parsed, physical = decode_cells(assets['XMP03T4.MAP'])
    native_map = next(row for row in inputs['sources'] if row['name'] == 'XMP03T4.MAP')
    cpu_map = next(row for row in history['input']['manifest'] if row['name'].upper() == 'XMP03T4.MAP')
    assert native_map['sha256'] == cpu_map['sha256']
    initial = next(row for row in history['initial_patch'] if row['cell'] == history['input']['impact_cell'])
    if not selected_states:
        assert (physical[tuple(initial['cell'])]['overlay'], physical[tuple(initial['cell'])]['frame'], physical[tuple(initial['cell'])]['level']) == (initial['overlay'], initial['data'], initial['level'])
    typ = types[wall_name]
    cpu_type = next(row for row in history['input']['wall_types'] if row['index'] == initial['overlay'])
    assert cpu_type['name'] == wall_name
    assert m.read32(typ+0x294) == cpu_type['index']
    assert m.read32(typ+0x2A0) == cpu_type['damage_levels']
    assert m.read32(typ+0x2A4) == cpu_type['strength']
    assert u.mem_read(typ+0x2A8,1)[0] == cpu_type['wall']
    selected, selections = [], []
    if selected_states:
        assert m.string(typ+0x1F8) == cpu_type['image']
        assert u.mem_read(typ+0x22C,1)[0] == cpu_type['theater']
        assert u.mem_read(typ+0x237,1)[0] == cpu_type['new_theater']
        for name, seed, path in selected_states:
            case_index, case = next((index, case) for index, case in enumerate(cpu['cases'])
                                    if case['input']['seed'] == seed)
            assert case['input']['manifest'] == history['input']['manifest']
            assert next(row for row in case['input']['wall_types']
                        if row['index'] == initial['overlay']) == cpu_type
            state = case
            for part in path: state = state[part]
            assert state['overlay'] in (-1, cpu_type['index'])
            authored = physical[tuple(state['cell'])]
            assert authored['overlay'] is None, 'new GAWALL is constructed, not map authored'
            assert (authored['tile'],authored['subtile'],authored['level']) == (state['tile'],state['subtile'],state['level'])
            ref = dict(file=cpu_filename,seed=seed,path=['cases',case_index,*path])
            if path[0] == 'steps':ref['hit']=case['steps'][path[1]]['hit']
            selections.append(ref)
            selected.append((name,state))
        states = dict(selected)
        assert len(states) == len(selected) == 5
        assert set(states) == {'initial_source','partial_neighbor','stage_two_neighbor',
                               'changed_endpoint','removed_source'}
        for name in ('initial_source','partial_neighbor','stage_two_neighbor','changed_endpoint'):
            assert states[name]['overlay'] == cpu_type['index']
        assert states['initial_source']['data'] >> 4 == 0
        assert states['initial_source']['data'] & 15, 'connected initial stage-zero wall'
        assert states['partial_neighbor']['data'] >> 4 == 1
        assert states['stage_two_neighbor']['data'] >> 4 == 2
        assert states['changed_endpoint']['data'] == 0
        assert (states['removed_source']['overlay'],states['removed_source']['data']) == (-1,0)
        assert len(wall_materials) == 1
        image_name = wall_materials[0].upper()
        source = next(row for row in inputs['sources'] if row['name'].upper() == image_name)
        cpu_source = next(row for row in history['input']['manifest'] if row['name'].upper() == image_name)
        assert source['sha256'] == cpu_source['sha256'] and source['bytes'] == cpu_source['bytes']
        assert m.read32(typ+0xA4) == m.asset_ptr[image_name]
        assert bytes(u.mem_read(m.read32(typ+0xA4),len(assets[image_name]))) == assets[image_name]
        loads = [row for row in inputs['overlay_theater']['loads'] if row['name'].upper() == image_name]
        assert len(loads) == 1 and loads[0]['sha256'] == source['sha256']
    else:
        selected=[('initial_source',initial)]
        partial=next(change['after'] for step in history['steps'] for change in step['changed']
            if change['after']['overlay']==initial['overlay'] and change['after']['cell']!=initial['cell']
            and change['after']['data']//16 > change['before']['data']//16)
        selected.append(('partial_neighbor',partial))
        endpoint=next(change['after'] for step in history['steps'] for change in step['changed']
            if change['after']['overlay']==initial['overlay'] and change['after']['cell']!=initial['cell']
            and (change['after']['data']&15)!=(change['before']['data']&15))
        selected.append(('changed_endpoint',endpoint))
        selected.append(('removed_source',history['steps'][-1]['source']))
    tiles, mapping, mappings, filenames, filename, heads, file_io, lighting = prepare_selected_tiles(
        m,assets,inputs,selected)
    (_unused, _globals), scheme = color_scheme_palette('LightGold',owner=m,physical_assets=assets)
    house=m.alloc(0x16060);u.mem_write(house+0x16054,dwords(scheme['color_index']))
    u.mem_write(0xA83D4C,dwords(house))
    image_name = wall_materials[0].upper() if wall_materials else 'CTFNCB.SHP'
    inputs['material_sources']=[row['name'] for row in inputs['sources']
        if row['name'].upper() in (image_name,'UNITTEM.PAL')]
    assert len(inputs['material_sources'])==2
    inputs['current_player']=dict(color_scheme=scheme,boundary='Supplied current Player/House index1; actual selected physical Colors reader and paired native ColorScheme constructors. Full House construction/ownership is excluded.')
    inputs['lighting']=lighting
    inputs['tmp']=dict(name=filename,mapping=mapping,file_io=file_io,
        boundary='Native selected theater scalar/string reads and full TMP constructor/lazy loader; sparse selected primary registry is supplied.')
    if selected_states:
        inputs['tmp'].update(names=list(filenames),mappings=mappings)
    rows=[]
    for index,(name, cell) in enumerate(selected):
        if not selected_states:assert cell['tile']==initial['tile'] and cell['subtile']==initial['subtile']
        floor=prepare_lit_cell(m,cell,heads[tiles[cell['tile']]],lighting,name)
        row=execute(m,types,dict(name=name,type=wall_name,coords=cell['cell'],level=cell['level'],
            flags=cell['flags'],state=cell['data'],overlay_id=cell['overlay'],cell_point=[96,120]),wall=True)
        row['native_cell']=cell
        row['lighting_output']={key:floor['output'][key] for key in
            ('top_scalar','bottom_scalar','common_scalar','cell_normalized_rgb','convert_rgb_key')}
        if selected_states:
            row['native_cpu_source']=selections[index]
            # Both native Cell sweeps share the same physical image/Convert,
            # live state and surface. Preserve the body-only row as a control.
            case = row['input']
            row['shadow'] = execute(m,types,case | dict(name=name+'_shadow'),wall=True,shadow=True)
            repeated_body = execute(m,types,case,wall=True)
            assert repeated_body['output'] == row['output']
            assert repeated_body['draws'] == row['draws']
            row['body_then_shadow'] = execute(m,types,case | dict(name=name+'_body_then_shadow'),
                wall=True,shadow=True,reset_surface=False)
            if cell['overlay'] == -1:
                assert not row['shadow']['draws'] and not row['body_then_shadow']['draws']
                assert row['shadow']['output']['runs'] == row['body_then_shadow']['output']['runs'] == []
            else:
                assert row['shadow']['draws'] and row['body_then_shadow']['draws']
                assert row['shadow']['output']['runs'], 'selected physical wall shadow must draw'
                assert row['body_then_shadow']['output'] != row['output'], 'shadow changes the body surface'
        rows.append(row)
    inputs['native_cpu_source']=dict(seed=history['input']['seed'],source=initial,
        map_payload_match=True,type_reader_match=True)
    if selected_states:
        inputs['native_cpu_source'].update(file=cpu_filename,selections=selections,
            map_overlay_authored=False,
            boundary='Map payload identity and terrain tuples match; native decoded GAWALL construction is a seam on authored empty cells. Selected AreaDamage histories are not normal paid World timing or RNG.')
    values=set(struct.unpack('<'+'H'*COUNT,u.mem_read(A,COUNT*2)))
    assert len(values)==1
    return dict(native_sha256=NATIVE_SHA256,inputs=inputs,
        surface=dict(width=WIDTH,height=HEIGHT,baseline=32768,bytes_per_pixel=2,a_uniform=True,a_value=values.pop()),rows=rows)


def authored_fence_metadata():
    return provenance(scope='Physical XMP03T4 CAFNCB native initial/partial-neighbor/endpoint/removed wall raster components, consuming native AreaDamage states. No whole Tactical/frame/World/GPU claim.',
        assumptions=[
            'Shared physical OverlayType/ART/NewTheater image reader, palette, shape and raster owner. CPU map hash and effective id/Strength/DamageLevels/Wall are compared to native AreaDamage inputs.',
            'Existing original theater readers and TMP constructor/lazy loader resolve the source tile; supplied sparse registry and native-constructed Cell enroll its physical tuple. Original map Lighting and Cell480350 produce retained top scalar. Floor color/Z are cleared before wall raster, not claimed as a composed frame.',
            'Existing sole ReadColor owner parses the selected physical Colors entry in its small INI Fixture; its byte result enters the existing raster VM, where physical UNITTEM expansion and original paired1/53 ColorScheme constructors execute. Supplied current Player/House index1 is an explicit component prior; original wall caller reads its Convert. Full House startup/ownership is excluded.',
            'Local projected point96,120, viewport0, clip192x160, A127, backgroundFFFF and oldZ65535 are supplied component priors; actual native A is read back. Occupied rows execute actual47F6A0/shape/selector/rowwalker/raster; the erased row reuses the actual Tactical -1 admission and observes no draw. No Rust expected pixels or source states.',
        ],substitutions=['Existing bounded allocation, TLS, archive/cache and surface transport; sparse TMP/Cell/current Player enrollment and supplied projection/A/Z. Original occupied wall caller, lighting, palette generation, shape and raster are not substituted. Only the erased row reuses the existing traversal map/visibility transports and observes no body/shadow entry.'],
        entry_points=dict(cell_body=0x47F6A0,cell_lighting=0x480350,wall_branch=0x47F96A,
            shape=0x4AED70,type_reader=0x5FE770,tmp_lazy=0x544CB0,empty_caller=0x6D6E5B))


def concrete_wall_metadata():
    data = authored_fence_metadata()
    data['scope'] = ('Physical TEMPERATE GAWALL initial/partial/stage-two/connection-change/erased '
        'Cell body, standalone shadow and retained body-then-shadow raster components from selected independent native AreaDamage states. '
        'No native paid Factory, whole Tactical/frame/World/GPU or recursive admission from pixels claim.')
    data['assumptions'] = [
        'The same prepare/type/theater/image/Convert/Cell/raster owner as authored CAFNCB. Full5FE770 reads GAWALL using physical expansion RULESMD/Battle/XMP and fixed ART. Original5FE620 constructs the TEMPERATE filename; existing archive transport supplies physical GTWALL.SHP. Actual selected Image/flags/type values and loaded payload identity are compared to the consumed concrete_wall CPU inputs. Native5FEDE0 cached getter executes; 5B40B0 remains the existing physical-byte transport, not full native archive/cache/SHPReference loading.',
        'The five source states name exact CPU seed/path/hit references. Initial source33,88 comes from seed0 initial_patch; north33,87 partial and stage-two states from seed1 hits3/8; endpoint33,90 connection change from seed0 hit5; erased source33,88 from seed0 hit6. These independent component states are also selected for a separate normal paid World consumer; no shared shot timing/RNG or positive recursive200 claim follows from pixels.',
        'Physical map hashes and selected tile/subtile/level tuples match native CPU states. These map Cells are authored empty; actual GAWALL CPU construction is an explicit decoded Overlay seam. Existing original theater readers resolve each unique selected TMP, and full native TMP constructor/lazy loader publishes each descriptor to a supplied sparse registry. Each row chooses its actual tile head for slope/land and original Cell480350 lighting; no CAFNCB/source-tile reuse or hand color is supplied.',
        *data['assumptions'][2:],
        'Each row retains its original body-only fields. Standalone shadow executes original47F510, raw state plus signed SHP frame count/2, flags4601 and original493830/497390 leaves against a reset surface. The body is independently repeated and required to reproduce its body-only draws/output, then original47F510 executes again on that same PIXELS/Z/A without resetting. Actual draw args, rectangle, rowwalker/leaf and final color/Z spans are retained. No host shadow pixels/composition math or second VM is supplied.',
        'The original shadow caller and shadow-rectangle body47FDE0 have no Wall exclusion; Tactical6D719F calls47F510 in its second sweep. Standalone/composed rows are bounded direct-caller components with supplied visible clip/point, not full native shadow visibility/rectangle/bootstrap. Erased rows use the existing actual Tactical suffix with declared map/visibility/rectangle transports and observe no body/shadow entry; no invalid id-minus-one direct caller executes.',
    ]
    data['entry_points'].update(cell_shadow='0x0047F510',shadow_rectangle='0x0047FDE0',
        tactical_shadow_call='0x006D719F',plain_shadow_leaf='0x00493830',rle_shadow_leaf='0x00497390')
    return data


def generate_low_concrete_bridge():
    """Retained native mission states through the shared Cell/ISO raster owner."""
    from tools.spatial_oracle.shrapnel_repair.packet_io import read_result
    from tools.spatial_oracle.shrapnel_repair.map_facts import decode_cells
    cpu_path = Path(__file__).with_name('anytown_damage').joinpath('mission.json.gz')
    cpu_hash = hashlib.sha256(cpu_path.read_bytes()).hexdigest()
    cpu = read_result(cpu_path)
    assert cpu['failure'] is None
    # These paths refer to actual executed native states, not new fixture states.
    paths = (
        ('loaded_center', ('frames',0,'target')),
        ('damaged_center', ('impacts',1,'after')),
        ('collapsed_center', ('impacts',2,'after')),
        ('damaged_west_empty', ('resident_trace',56,'before')),
        ('damaged_east_empty', ('resident_trace',60,'before')),
    )
    selected, refs = [], []
    for name,path in paths:
        state = cpu
        for key in path:state=state[key]
        selected.append((name,dict(cell=state['coord'],data=state['state'],
            **{key:state[key] for key in ('overlay','tile','subtile','level','slope','flags')})))
        refs.append(dict(file='anytown_damage/mission.json.gz',path=list(path)))
    assert [(c['overlay'],c['data']) for _,c in selected] == [(216,1),(220,1),(232,1),(220,0),(220,2)]
    names=('LOBRDB12','LOBRDB16','LOBRDB28')
    materials=tuple(name+'.TEM' for name in names)
    # Reuse the existing selected-theater resource bundle. These actual types
    # are NOT Wall types: original readers below establish their own flags.
    m,types,inputs=prepare(wall=True,wall_types=names,wall_materials=materials)
    u=m.u
    assets,_=physical_assets(wall=True,wall_materials=materials)
    _,physical=decode_cells(assets['XMP03T4.MAP'])
    source=next(row for row in inputs['sources'] if row['name']=='XMP03T4.MAP')
    assert source['sha256']==cpu['world']['physical_map_sha256']
    inputs['overlay_images']=inputs.pop('wall_images')
    inputs['effective_types']=[]
    for name in names:
        typ=types[name]; image=m.read32(typ+0xA4);raw=assets[name+'.TEM']
        assert image and image==m.asset_ptr[name+'.TEM']
        assert bytes(u.mem_read(image,len(raw)))==raw
        flags={key:u.mem_read(typ+offset,1)[0] for key,offset in (
            ('wall',0x2A8),('tiberium',0x2A9),('crate',0x2AA),('draw_flat',0x2B3),
            ('is_rubble',0x2B4),('is_a_rock',0x2B5))}
        assert flags==dict(wall=0,tiberium=0,crate=0,draw_flat=1,is_rubble=0,is_a_rock=0)
        assert u.mem_read(typ+0x22C,1)[0]==1
        inputs['effective_types'].append(dict(name=name,index=m.read32(typ+0x294),
            image=m.string(typ+0x1F8),flags=flags,raw_frames=struct.unpack_from('<H',raw,6)[0],
            image_payload_sha256=hashlib.sha256(bytes(u.mem_read(image,len(raw)))).hexdigest()))
    for _,cell in selected:
        authored=physical[tuple(cell['cell'])]
        assert (authored['tile'],authored['subtile'],authored['level'])==(cell['tile'],cell['subtile'],cell['level'])
        assert cell['flags']==2 and cell['level']==4 and cell['slope']==0
    tiles,mapping,mappings,filenames,filename,heads,file_io,lighting=prepare_selected_tiles(m,assets,inputs,selected)
    inputs.update(lighting=lighting,tmp=dict(names=list(filenames),mappings=mappings,file_io=file_io,
        boundary='Shared original theater reader/TMP loader and supplied sparse selected primary registry. Original480350 supplies Cell lighting before each separate overlay raster.'))
    dummy=m.alloc(0x148);m.invoke(0x47BBF0,dummy)
    assert m.read32(dummy+0x44)==0xFFFFFFFF
    tactical=m.alloc(0xE20)
    rows=[]
    for (name,cell),ref in zip(selected,refs):
        floor=prepare_lit_cell(m,cell,heads[tiles[cell['tile']]],lighting,name)
        typ=next(name for name,pointer in types.items() if m.read32(pointer+0x294)==cell['overlay'])
        case=dict(name=name,type=typ,coords=cell['cell'],level=cell['level'],
            flags=cell['flags'],state=cell['data'],overlay_id=cell['overlay'],cell_point=[96,120])
        row=execute(m,types,case,wall=True,allow_empty_frame=True)
        row['native_cell']=cell;row['native_cpu_source']=ref
        row['lighting_output']={key:floor['output'][key] for key in
            ('top_scalar','bottom_scalar','common_scalar','cell_normalized_rgb','convert_rgb_key')}
        row['shadow']=execute(m,types,case|dict(name=name+'_shadow'),wall=True,shadow=True,allow_empty_frame=True)
        repeated=execute(m,types,case,wall=True,allow_empty_frame=True)
        assert repeated['output']==row['output'] and repeated['draws']==row['draws']
        row['body_then_shadow']=execute(m,types,case|dict(name=name+'_body_then_shadow'),
            wall=True,shadow=True,reset_surface=False,allow_empty_frame=True)
        assert row['shadow']['native_rect'][2:]==[0,0] and not row['shadow']['output']['runs']
        assert row['body_then_shadow']['output']==row['output']
        # Obtain the selected point from the original Tactical suffix, using
        # its existing explicit full-rectangle transport only for this point
        # observation. Then execute the suffix AGAIN with actual Cell bounds.
        cells={tuple(cell['cell']):CELL}
        projection=traversal(m,origin=tuple(cell['cell']),require_draws=False,
            native_cells=cells,native_dummy=dummy,native_tactical=tactical,native_rectangles=False)
        point=next(call['point'] for call in projection['calls'] if call['piece']=='body')
        view=tuple(point[i]-case['cell_point'][i] for i in range(2))
        row['native_traversal']=traversal(m,origin=tuple(cell['cell']),view_origin=view,require_draws=False,
            native_cells=cells,native_dummy=dummy,native_tactical=tactical)
        row['native_traversal']['projection_point_at_zero_view']=point
        body_calls=[call for call in row['native_traversal']['calls'] if call['piece']=='body']
        assert bool(body_calls)==bool(row['output']['runs'])
        assert not any(call['piece']=='shadow' for call in row['native_traversal']['calls'])
        rows.append(row)
    assert [bool(row['output']['runs']) for row in rows]==[True,True,False,False,False]
    assert hashlib.sha256(cpu_path.read_bytes()).hexdigest()==cpu_hash,'native CPU gzip changed during raster execution'
    inputs['native_cpu_source']=dict(file='anytown_damage/mission.json.gz',sha256=cpu_hash,selections=refs,
        boundary='Executed isolated native mission states. World rawflags0 versus native flags2 is draw-equivalent only in the selected47F6A0/47F510/480110/47FB90/47FDE0 bodies: only bit80 is read there. No whole map loader, flag producer or native/full-match epoch equivalence is claimed.')
    values=set(struct.unpack('<'+'H'*COUNT,u.mem_read(A,COUNT*2)))
    assert values=={127}
    return dict(native_sha256=NATIVE_SHA256,inputs=inputs,
        surface=dict(width=WIDTH,height=HEIGHT,baseline=32768,bytes_per_pixel=2,a_uniform=True,a_value=values.pop()),rows=rows)


def low_concrete_bridge_metadata():
    return provenance(scope='Five TEMPERATE ordinary low-concrete bridge native raster components:216/220/232 center data1 and220 flank data0/2. Actual positive/empty image, Cell body/shadow, same-surface composition, selected TMP lighting and original Tactical bounds. No whole frame/map/gameplay equivalence.',
        assumptions=[
            'Existing mission.json.gz original executed states and physical XMP03T4 tile/level tuples supply inputs. Native flags2 are retained; production flags0 relation is a static bit80-only read proof in the selected drawing bodies, excluding unrelated upstream flags/visibility.',
            'Shared full Overlay registry/reader, ART Image/Theater reads, original5FE620 and5FEDE0 bind physical LOBRDB12/16/28.TEM. Actual image payload bytes, default flags and six-frame domain are asserted. ISO default LightConvert and selected TMP loader/480350 Cell scalar execute; no current-House remap or Rust-provided material input is used.',
            'Each row directly executes original47F6A0 and47F510, repeats body to check equality, then executes shadow on the same PIXELS/Z/A without reset. Original69E7E0 zero crop legitimizes missing leaf work; bound image and actual Shape entry remain required. Two bodies draw, three bodies and all shadows have actual empty crops. No synthetic empty JSON or invalid minus-one direct caller is used.',
            'Original Tactical6D6E5B..6D71D3 suffix uses supplied sparse selected Cell lookup, native-constructed empty Dummy and true map visibility. A declared full-rectangle point observation obtains native projection before the final pass; final47FB90/47FDE0/6D2140/69E7E0 and intersections execute. Draw calls in that final suffix are recorded sinks, not a whole Tactical execution. Raster components execute the real draw bodies separately.',
            'Supplied192x160 surface, cell point96,120, white background, oldZ65535 and neutral A127 bound the component. Native color/Z runs are observed, not computed. Normal production visitor/GPU tests separately test live identity/data, actual A and presentation determinism.',
        ],substitutions=['Shared PaletteReader successful allocator/TLS/physical resource transports and selected Cell/TMP surface priors. Tactical map/visibility lookup and draw sinks are explicit; the final bounds, projection, clipping, palette, rowwalker and raster leaves are original.'],
        entry_points=dict(cell_body=0x47F6A0,cell_shadow=0x47F510,body_bounds=0x47FB90,
            shadow_bounds=0x47FDE0,shape_bounds=0x69E7E0,projection=0x6D2140,
            cell_tile=0x480350,type_ctor=0x5FE250,type_read=0x5FE770,theater=0x5FE620,get_image=0x5FEDE0))


def generate_ore_natural():
    """Two original raster components at the native physical growth witness."""
    import json
    cpu = json.loads(Path(__file__).with_name('ore_authored_natural.json').read_text())
    source = next(row for row in cpu['steps'][0]['changed'] if row['before']['cell'] == [72, 25])
    m, types, inputs = prepare(ore=True, ore_natural=True)
    map_source = next(row for row in inputs['sources'] if row['name'] == 'XMP03T4.MAP')
    cpu_map = next(row for row in cpu['inputs']['manifest'] if row['name'] == 'XMP03T4.MAP')
    assert map_source['sha256'] == cpu_map['sha256']
    # Link actual native reader outputs, not the former GoldSt layer assumptions.
    native_type, cpu_type = inputs['tiberium']['layers'][-1], cpu['inputs']['effective_types'][0]
    keys = dict(index='type_id', base_overlay_id='base_overlay', num_images='images',
                num_slopes='extra', value='value', growth='growth', spread='spread',
                growth_percentage_bits='growth_percentage_bits', spread_percentage_bits='spread_percentage_bits')
    assert all(native_type[key] == cpu_type[other] for key, other in keys.items())
    physical = inputs['map']['cells'][0]
    assert physical['coords'] == source['before']['cell']
    assert physical['overlay'] == source['before']['overlay']
    assert physical['frame'] == source['before']['density']
    assert physical['level'] == source['before']['level']
    rows = []
    for name in ('before', 'after'):
        cell = source[name]
        assert cell['slope'] == 0 and cell['flags'] == 0
        typ = next(name for name, pointer in types.items() if m.read32(pointer + 0x294) == cell['overlay'])
        rows.append(execute(m, types, dict(name=f'natural_growth_{name}', type=typ,
            coords=cell['cell'], level=cell['level'], flags=cell['flags'], state=cell['density']), ore=True))
    inputs['native_cpu_source'] = dict(before=source['before'], after=source['after'],
                                       reader_fields_match=True, map_payload_match=True)
    # Read the supplied A plane from the existing native surface owner. This
    # records a component prior, not a native tactical/shroud producer claim.
    a_values = set(struct.unpack('<' + 'H' * COUNT, m.u.mem_read(A, COUNT * 2)))
    assert len(a_values) == 1, 'natural raster requires a uniform supplied A prior'
    return dict(native_sha256=NATIVE_SHA256, inputs=inputs,
                surface=dict(width=WIDTH, height=HEIGHT, baseline=32768, bytes_per_pixel=2,
                             a_value=a_values.pop(), a_uniform=True), rows=rows)


def ore_natural_metadata():
    return provenance(scope='Two original flat Riparius rasters at physical XMP03T4 source72,25, level4/slope0/flags0, density4 then5. Native CPU and raster are separate bounded components; no whole Tactical/frame/GPU claim.',
        assumptions=[
            'Existing physical archive/OverlayType/Tiberium/ART/image/ordinary Convert/shape/surface owner, selecting XMP03T4.MAP in the existing layer loop instead of GoldSt. Actual physical map hash, source identity/data/level and effective type fields are compared to consumed ore_authored_natural native outputs. No Rust inputs or expected pixels.',
            'Existing original47F6A0,480110,5FDCC0,4AED70,490E50,437A10 and4990E0 raster pipeline executes. Cell data4 and5 come from independently executed native growth; live map loader, normal Tactical projection and World AI are excluded.',
            'Existing VM A plane is read back after the rows and required uniform; surface.a_value records its supplied neutral prior, not native shroud generation. Projected local cell_point66,141, clip192x160, viewport0, A127, backgroundFFFF and oldZ65535 remain explicit raster priors. Actual selected image/frame/rect/color/Z runs are native outputs. Original surface numeric comparison does not itself prove production fog/A upload.',
        ], substitutions=['Existing PaletteReader successful allocator/TLS/archive transport and supplied Cell/A/Z/surface inputs. Original caller, display selector, shape/raster and palette generation are not substituted.'],
        entry_points=dict(cell_body=0x47F6A0, draw_offset=0x480110, selector=0x5FDCC0,
                          shape=0x4AED70, rle_selector=0x490E50, rowwalker=0x437A10,
                          opaque=0x4990E0, type_reader=0x721A50))


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
    modes.add_argument('--rubble', action='store_true')
    modes.add_argument('--crate', action='store_true')
    modes.add_argument('--ore', action='store_true')
    modes.add_argument('--ore-natural', action='store_true')
    modes.add_argument('--authored-fence', action='store_true')
    modes.add_argument('--concrete-wall', action='store_true')
    modes.add_argument('--low-concrete-bridge', action='store_true')
    modes.add_argument('--tile', action='store_true')
    modes.add_argument('--tile-partial', action='store_true')
    modes.add_argument('--placement', action='store_true')
    modes.add_argument('--smudge', action='store_true')
    modes.add_argument('--cliff', action='store_true')
    selected, remaining = parser.parse_known_args()
    if selected.tile_partial:
        from tools.spatial_oracle.tmp_clear_render import partial_redraw_main
        partial_redraw_main(remaining)
        raise SystemExit(0)
    if selected.rubble or selected.crate:
        # Guard the actual shared reader/runtime/asset/raster inputs only for
        # this invocation. No repository-source hashes enter the sidecar.
        source_names = (
            'native_oracle.py', 'oracle_sources.py',
            'spatial_oracle/bridge_shadow_render.py',
            'spatial_oracle/building_body_rules.py', 'spatial_oracle/placement_render.py',
            'spatial_oracle/bridge_constructor.py', 'spatial_oracle/map_queries.py',
            'spatial_oracle/bridge_damage_admission.py',
            'projectile_oracle/bridge_render_inputs.py',
            'projectile_oracle/bridge_render_inputs_palette.py',
            'projectile_oracle/bridge_render_shape.py', 'projectile_oracle/flat_art.py',
            'projectile_oracle/bridge_render.py',
            'rules_oracle/bridge_anim_inputs.py', 'rules_oracle/bridge_anim_lists.py',
            'rules_oracle/bridge_child_sound.py',
            'palette_oracle/oracle.py', 'palette_oracle/check.py', 'sidebar_oracle/stock.py',
        )
        finish_vectors(generate_crate if selected.crate else generate_rubble,
                       Path(__file__).with_name('crate_shadow_render.json' if selected.crate
                                                else 'building_rubble_shadow_render.json'),
                       provenance=crate_metadata if selected.crate else rubble_metadata, argv=remaining,
                       source_paths={name: Path('tools') / name for name in source_names})
        raise SystemExit(0)
    if selected.authored_fence or selected.concrete_wall or selected.low_concrete_bridge:
        from functools import partial
        generator, metadata_selected, output = generate_authored_fence, authored_fence_metadata, 'wall_render.json'
        if selected.concrete_wall:
            generator = partial(generate_authored_fence,
                cpu_filename='concrete_wall.json',wall_name='GAWALL',wall_materials=('GTWALL.SHP',),
                selected_states=(
                    ('initial_source',0,('initial_patch',17)),
                    ('partial_neighbor',1,('steps',2,'changed',0,'after')),
                    ('stage_two_neighbor',1,('steps',7,'changed',0,'after')),
                    ('changed_endpoint',0,('steps',4,'changed',7,'after')),
                    ('removed_source',0,('steps',5,'source')),
                ))
            metadata_selected, output = concrete_wall_metadata, 'concrete_wall_render.json'
        if selected.low_concrete_bridge:
            generator, metadata_selected, output = generate_low_concrete_bridge, low_concrete_bridge_metadata, 'low_concrete_bridge_render.json'
        source_names = (
                           'native_oracle.py','oracle_sources.py',
                           'spatial_oracle/bridge_shadow_render.py','spatial_oracle/bridge_constructor.py',
                           'spatial_oracle/tmp_clear_render.py','spatial_oracle/bridge_recalc_presentation.py',
                           'spatial_oracle/engineer_repair_admission.py','spatial_oracle/building_body_rules.py',
                           'spatial_oracle/anytown_damage/navigation_inputs.py',
                           'spatial_oracle/anytown_damage/authored_fence.json',
                           'spatial_oracle/shrapnel_repair/retail_inputs.py',
                           'spatial_oracle/shrapnel_repair/map_facts.py',
                           'projectile_oracle/bridge_render_inputs.py',
                           'projectile_oracle/bridge_render_inputs_palette.py',
                           'projectile_oracle/bridge_render_shape.py','palette_oracle/oracle.py',
                           'procedural_drawing_oracle/house_color.py','sidebar_oracle/stock.py',
                           'rules_oracle/bridge_anim_inputs.py','rules_oracle/bridge_child_sound.py')
        if selected.concrete_wall:
            # Transient before/after guards include selected CPU bytes and the
            # actual shared numeric/material/Cell/IO owners. No source pins
            # are retained in the sidecar. Physical file hashes are outputs.
            source_names += (
                'spatial_oracle/anytown_damage/concrete_wall.json',
                'spatial_oracle/anytown_damage/concrete_wall.meta.json',
                'spatial_oracle/anytown_damage/mtnk_attack.py',
                'spatial_oracle/anytown_damage/navigation.py',
                'rules_oracle/theater_general_reader.py','rules_oracle/bridge_anim_lists.py',
                'projectile_oracle/flat_art.py','projectile_oracle/bridge_render.py',
                'spatial_oracle/map_queries.py','spatial_oracle/terrain_recalc.py',
                'spatial_oracle/bridge_damage_admission.py','native_slope.py',
                'bridge_click_oracle.py','rmg_oracle/gen_rng_vectors.py','palette_oracle/check.py',
            )
        if selected.low_concrete_bridge:
            # UTF-8 source guards only: read_result's gzip input is guarded
            # as binary before/after generate_low_concrete_bridge, not decoded
            # by finish_vectors/source_paths.
            source_names += (
                'spatial_oracle/anytown_damage/mission.py',
                'spatial_oracle/anytown_damage/mission_publication.py',
                'spatial_oracle/anytown_damage/mission.meta.json',
                'spatial_oracle/anytown_damage/mission_promotion.json',
                'spatial_oracle/shrapnel_repair/packet_io.py',
                'spatial_oracle/shrapnel_repair/promotion.json',
                'rules_oracle/theater_general_reader.py','native_slope.py',
                'rules_oracle/bridge_anim_lists.py','rules_oracle/bridge_landing_inputs.py',
                'projectile_oracle/flat_art.py','projectile_oracle/bridge_render.py',
                'spatial_oracle/bridge_damage_admission.py','bridge_click_oracle.py',
                'rmg_oracle/gen_rng_vectors.py','palette_oracle/check.py',
                'spatial_oracle/map_queries.py','spatial_oracle/terrain_recalc.py',
            )
        finish_vectors(generator,Path(__file__).with_name(output),
                       provenance=metadata_selected,argv=remaining,
                       source_paths={name:Path('tools')/name for name in source_names})
        raise SystemExit(0)
    if selected.ore_natural:
        finish_vectors(generate_ore_natural, Path(__file__).with_name('ore_render_natural.json'),
                       provenance=ore_natural_metadata, argv=remaining,
                       source_paths={name: Path('tools') / name for name in (
                           'native_oracle.py', 'oracle_sources.py', 'spatial_oracle/bridge_shadow_render.py',
                           'spatial_oracle/bridge_constructor.py', 'palette_oracle/oracle.py',
                           'projectile_oracle/bridge_render_inputs.py',
                           'projectile_oracle/bridge_render_inputs_palette.py',
                           'projectile_oracle/bridge_render_shape.py')})
        raise SystemExit(0)
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
