"""Original high bridge Cell shadow caller, retail SHP and RGB565/Z pixels.

Run with --check (default) or --write. The shared native image, lexical INI,
archive, Convert-construction and shape owners provide the fixture boundaries;
this module executes original bridge caller and raster instructions unchanged.
"""
from pathlib import Path
import hashlib
import struct
import sys

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_READ
from unicorn.x86_const import (
    UC_X86_REG_EBP, UC_X86_REG_EDI, UC_X86_REG_ESP, UC_X86_REG_EIP,
)
from tools.native_oracle import (
    NATIVE_SHA256, configured_gamemd, finish_vectors, provenance,
    run_checked,
)
from tools.projectile_oracle.bridge_render_inputs import lexical
from tools.projectile_oracle.bridge_render_inputs_palette import PaletteReader, initialize
from tools.projectile_oracle.bridge_render_shape import (
    SURFACE, ZOBJ, AOBJ, ZSURF, ASURF, PIXELS, Z, A, packed_words,
)
from tools.sidebar_oracle.stock import mix, mix_hash
from tools.spatial_oracle.building_body_rules import RULES, SP, dwords

WIDTH, HEIGHT = 192, 160
COUNT = WIDTH * HEIGHT
MEM = 0x21000000
CELL, POINT, CLIP, RECT = [MEM + n for n in (0, 0x1000, 0x1020, 0x1040)]
LEAVES = {0x493830: 'plain_shadow', 0x497390: 'rle_shadow'}
BRIDGE_TYPES = ('BRIDGE1', 'BRIDGE2', 'BRIDGEB1', 'BRIDGEB2')
LOW_BRIDGE_TYPES = ('LOBRDG01', 'LOBRDG02', 'LOBRDG08', 'LOBRDG09')


def ints(u, address, count):
    return list(struct.unpack('<' + 'i' * count, u.mem_read(address, count * 4)))


def physical_assets(extra_types=()):
    root = configured_gamemd().parent
    sources, assets = [], {}
    for outer_name, children in (
        ('ra2.mix', {'temperat.mix': ('bridge.tem', 'bridgb.tem'),
                     **({'isotemp.mix': tuple(name.lower() + '.tem' for name in extra_types)}
                        if extra_types else {}),
                     'snow.mix': ('bridge.sno', 'bridgb.sno'),
                     'urban.mix': ('bridge.urb', 'bridgb.urb'),
                     'cache.mix': ('palette.pal', 'anim.pal')}),
        ('ra2md.mix', {'localmd.mix': ('RULESMD.INI', 'ARTMD.INI', 'MPBattleMD.ini'),
                       'urbann.mix': ('bridge.ubn', 'bridgb.ubn'),
                       'desert.mix': ('bridge.des', 'bridgb.des'),
                       'lunar.mix': ('bridge.lun', 'bridgb.lun')}),
    ):
        outer_raw = (root / outer_name).read_bytes()
        outer = mix(outer_raw)
        for inner_name, names in children.items():
            inner_raw = outer[mix_hash(inner_name)]
            inner = mix(inner_raw)
            for name in names:
                raw = inner[mix_hash(name)]
                assets[name.upper()] = raw
                sources.append(dict(name=name, outer=outer_name, inner=inner_name,
                                    outer_sha256=hashlib.sha256(outer_raw).hexdigest(),
                                    inner_sha256=hashlib.sha256(inner_raw).hexdigest(),
                                    sha256=hashlib.sha256(raw).hexdigest(), bytes=len(raw)))
    return assets, sources


def prepare(extra_types=()):
    assets, sources = physical_assets(extra_types)
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
    selected_types = (*BRIDGE_TYPES, *extra_types)
    art, art_lines = lexical(assets['ARTMD.INI'], {*selected_types, 'BRIDGE', 'BRIDGB'})
    m = PaletteReader(art)
    m.assets.update(assets)
    initialize(m)
    u = m.u
    u.mem_map(MEM, 0x200000)
    # Existing registry constructor, then actual physical registry-prefix names.
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, 0x4E71E0, 0x4E7216)
    declared, _ = lexical(assets['RULESMD.INI'], {'OverlayTypes'})
    names = list(declared['OverlayTypes'].values())
    names = names[:1 + max(names.index(name) for name in selected_types)]
    types = {}
    for name in names:
        typ = m.alloc(0x300)
        m.invoke(0x5FE250, typ, (m.cstring(name),))
        types[name] = typ
    m.make_ini(art)
    layers = []
    for filename in ('RULESMD.INI', 'MPBattleMD.ini'):
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
    return m, types, dict(sources=sources, shape_aliases=shape_aliases, registry_prefix=names,
                          physical_art=art, art_source_lines=art_lines, layers=layers)


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


def execute(m, types, case):
    case = dict(type='BRIDGE1', coords=[10, 20], level=0, state=0,
                flags=0x180, cell_point=[66, 81], clip=[0, 0, WIDTH, HEIGHT],
                viewport_y=0, background=0xFFFF, old_z=65535, repeat=1) | case
    u = m.u
    typ = types[case.get('type', 'BRIDGE1')]
    shp = m.read32(typ + 0xA4)
    assert shp, 'Physical bridge image must bind through original reader'
    u.mem_write(CELL + 0x44, dwords(m.read32(typ + 0x294)))
    u.mem_write(CELL + 0x24, struct.pack('<2h', *case['coords']))
    u.mem_write(CELL + 0x11B, bytes((case.get('level', 0) & 255,)))
    u.mem_write(CELL + 0x11E, bytes((case.get('state', 0),)))
    u.mem_write(CELL + 0x140, dwords(case.get('flags', 0x180)))
    u.mem_write(0x886FA0, dwords(0, case.get('viewport_y', 0), WIDTH, HEIGHT))
    u.mem_write(POINT, dwords(*case.get('cell_point', [66, 81])))
    u.mem_write(CLIP, dwords(*case.get('clip', [0, 0, WIDTH, HEIGHT])))
    background, old_z = case.get('background', 0xFFFF), case.get('old_z', 65535)
    u.mem_write(PIXELS, packed_words([background] * COUNT))
    u.mem_write(Z, packed_words([old_z] * COUNT))
    u.mem_write(A, packed_words([127] * COUNT))
    draws, rows, reached = [], [], set()

    def observe(_u, address, _size, _data):
        if address in (0x47F510, 0x480110, 0x5FDCC0, 0x4AED70, 0x69E7E0,
                       0x69E740, 0x69E900, 0x490E50, 0x437A10, *LEAVES):
            reached.add(address)
        if address == 0x4AED70:
            sp = u.reg_read(UC_X86_REG_ESP)
            args = ints(u, sp + 4, 14)
            draws.append(dict(frame=args[1], point=ints(u, args[2], 2),
                              clip=ints(u, args[3], 4), flags=args[4],
                              z_adjust=args[6], gradient=args[7], brightness=args[8],
                              zshape_argument=args[12], raw_args=args))
        elif address in LEAVES:
            sp = u.reg_read(UC_X86_REG_ESP)
            args = ints(u, sp + 4, 10)
            offset = (args[0] - PIXELS) // 2
            rows.append(dict(leaf=LEAVES[address], x=offset % WIDTH,
                             y=offset // WIDTH, count=args[2], skip=args[3],
                             candidate=args[4], zshape_address=args[9],
                             zshape_values=sorted(set(struct.unpack(
                                 '<' + 'b' * args[2], u.mem_read(args[9], args[2]))))))

    hook = u.hook_add(UC_HOOK_CODE, observe)
    for _ in range(case.get('repeat', 1)):
        m.invoke(0x47F510, CELL, (POINT, CLIP))
    u.hook_del(hook)
    assert draws and {0x47F510, 0x480110, 0x5FDCC0, 0x4AED70,
                      0x490E50, 0x437A10, 0x497390}.issubset(reached)
    m.invoke(0x69E7E0, shp, (RECT, draws[0]['frame']))
    result = dict(name=case['name'], input=case, draws=draws, leaf_rows=rows,
                  image_name=m.string(typ + 0x1F8),
                  native_rect=ints(u, RECT, 4),
                  reached=[f'{p:08X}' for p in sorted(reached)],
                  output=snapshot(u, background, old_z))
    result['canvas'] = list(struct.unpack('<2H', u.mem_read(shp + 2, 4)))
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


def traversal(m):
    """Original Tactical loop order; map/visibility and draw calls are sinks."""
    u = m.u
    tactical = m.alloc(0xE20)
    u.mem_write(0xB0CE30, dwords(WIDTH, HEIGHT))
    u.mem_write(0xB0CD48, struct.pack('<Q', 0x3FC25E5374344960))
    u.mem_write(SP, bytes(0x180))
    u.mem_write(SP + 0x28, struct.pack('<hh', 10, 20))
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
                required_addresses=(0x6D7001, 0x6D7031, 0x6D719F))
    u.hook_del(hook)
    return dict(entry='006D6E5B', end='006D71D3', origin=[10, 20],
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


def shroud_admission():
    """Execute real rectangle and draw callers, varying only Cell visibility words."""
    from tools import native_oracle as native

    m, types, inputs = prepare(LOW_BRIDGE_TYPES)
    u = m.u
    tactical, map_object = m.alloc(0xE20), m.alloc(0x200)
    u.mem_write(0x887324, dwords(tactical))
    u.mem_write(tactical + 0xB0, dwords(-366, 369))
    u.mem_write(0xB0CE30, dwords(WIDTH, HEIGHT))
    u.mem_write(0xB0CD48, struct.pack('<Q', 0x3FC25E5374344960))
    u.mem_write(map_object + 0xF4, dwords(16, 16))
    u.mem_write(CELL + 0x24, struct.pack('<2h', 10, 20))
    u.mem_write(CELL + 0x10A, struct.pack('<3h', 1000, 1000, 1000))
    u.mem_write(CELL + 0x11B, b'\0')
    u.mem_write(0x886FA0, dwords(0, 0, WIDTH, HEIGHT))
    u.mem_write(POINT, dwords(66, 81))
    u.mem_write(CLIP, dwords(0, 0, WIDTH, HEIGHT))
    rows = []
    for name in (*LOW_BRIDGE_TYPES, 'BRIDGEB1'):
        typ = types[name]
        index = m.read32(typ + 0x294)
        image = m.read32(typ + 0xA4)
        assert image, name
        u.mem_write(CELL + 0x44, dwords(index))
        state = 0 if name == 'BRIDGEB1' else 1
        u.mem_write(CELL + 0x11E, bytes((state,)))
        u.mem_write(CELL + 0x140, dwords(0x180 if name == 'BRIDGEB1' else 0))
        for cell_flags, cell_flags2 in ((0, 0), (0, 1), (8, 0), (8, 1), (0x18, 0), (0x18, 1)):
            u.mem_write(CELL + 0x12C, dwords(cell_flags, cell_flags2))
            u.mem_write(CELL + 100, dwords(-1))  # supplied invalid redraw-frame cache
            u.mem_write(PIXELS, packed_words([0xFFFF] * COUNT))
            u.mem_write(Z, packed_words([65535] * COUNT))
            u.mem_write(A, packed_words([127] * COUNT))
            flag_reads, draws, rects = [], [], {}

            def read_flags(_u, _access, address, size, _value, _data):
                if address < CELL + 0x134 and address + size > CELL + 0x12C:
                    flag_reads.append(dict(pc=f'{u.reg_read(UC_X86_REG_EIP):08X}',
                                           offset=address - CELL, bytes=size))

            def draw(_u, address, _size, _data):
                if address == 0x4AED70:
                    args = ints(u, u.reg_read(UC_X86_REG_ESP) + 4, 14)
                    draws.append(dict(piece='body' if len(draws) == 0 else 'shadow',
                                      frame=args[1], point=ints(u, args[2], 2),
                                      clip=ints(u, args[3], 4), flags=args[4],
                                      z_adjust=args[6], brightness=args[8]))

            h_read = u.hook_add(UC_HOOK_MEM_READ, read_flags)
            h_draw = u.hook_add(UC_HOOK_CODE, draw)
            for address in (0x47FB90, 0x47FDE0):
                m.invoke(address, CELL, (RECT,))
                rects[f'{address:08X}'] = ints(u, RECT, 4)
            for address in (0x47F6A0, 0x47F510):
                m.invoke(address, CELL, (POINT, CLIP))
            u.hook_del(h_draw)
            u.hook_del(h_read)
            assert len(draws) == 2, (name, draws)
            assert not flag_reads, (name, flag_reads)
            output = snapshot(u, 0xFFFF, 65535)
            rows.append(dict(type=name, ordinal=index, state=state, cell_flags=cell_flags,
                             cell_flags2=cell_flags2, rects=rects, draws=draws,
                             visibility_word_reads=flag_reads,
                             output={key:value for key, value in output.items() if key != 'runs'}))
    map_bounds = []
    for coords in ((10, 20), (0, 0), (16, 16), (33, 33)):
        u.mem_write(RECT, struct.pack('<2h', *coords))
        admitted = m.invoke(0x568300, map_object, (RECT,)) & 255
        map_bounds.append(dict(cell=list(coords), admitted=bool(admitted)))
    static = []
    for begin, size in ((0x6D6D10, 0x4C5), (0x6D3290, 0x1D2),
                        (0x47FB90, 0x1F4), (0x47FDE0, 0x194),
                        (0x568300, 0x47), (0x480110, 0x64),
                        (0x47F6A0, 0x4EE), (0x47F510, 0x183)):
        _, raw = native.file_span(native.image_bytes(), begin, size)
        assert bytes(u.mem_read(begin, size)) == raw, f'Native code changed at{begin:08X}'
        static.append(dict(address=f'{begin:08X}', bytes=size, raw_hex=raw.hex(),
                           sha256=hashlib.sha256(raw).hexdigest()))
    return dict(native_sha256=native.image_sha256(), inputs=inputs,
                supplied=dict(cell=[10, 20], level=0, camera=[-366, 369],
                              point=[66, 81], clip=[0, 0, WIDTH, HEIGHT],
                              brightness=1000, a_buffer=127, map_width=16, map_height=16),
                rows=rows, map_bounds=map_bounds, static_native_bytes=static)


def shroud_admission_metadata():
    return provenance(
        scope='30 low/high wood bridge visibility-word controls through complete original body/shadow rectangles and draw/raster callers, plus four original map-bound controls. Not a complete Tactical traversal or native Scenario.',
        assumptions=[
            'Existing bridge fixture loads retail declared OverlayTypes prefix, original type constructor and full rules readers against RULESMD and MPBattleMD, original ART/image binding and palette Convert construction. Extra low-bridge physical .TEM files come from ra2.mix/isotemp.mix; no map overrides or other theater loaders claimed.',
            'Supplied live Cell coords10,20, level0, low wood state1(the only nonempty retail body frame) / high wood state0, brightness1000, invalid redraw-frame cache(-1), exploration words12C/130, Tactical camera and RGB565/A/Z memory surfaces. Original projection6D2140, draw offset480110, rectangle47FB90/47FDE0, body47F6A0, shadow47F510 and raster execute. Low wood shadow halves are empty; shadow caller is reached directly, while original Tactical rectangle gate would omit it.',
            '30 rows cover LOBRDG01/02/08/09 and shared high wood BRIDGEB1, each with12C=0/8/18 and130=0/1. Memory observer checks reads overlapping both visibility words; unchanged output across controls is a bounded admission witness, not shroud pixel coverage.',
            '568300 is executed with prepared Map+F4/F8=16/16. Its original instructions test diamond map bounds using coordinates; it does not query explored state. Saved unmodified native caller bytes support the complete6D6D10 two-sweep control-flow reading, not its execution or all aliases/callers.',
        ],
        substitutions=[
            'Reuse existing PaletteReader allocator, TLS, archive/resource and lexical INI boundaries. No reached Cell rectangle, projection, body, shadow or raster is replaced. No568300/47FB90/47FDE0 visibility stub from the older traversal fixture applies.',
        ],
        entry_points={'body_rect':0x47FB90, 'shadow_rect':0x47FDE0,
                      'body':0x47F6A0, 'shadow':0x47F510, 'map_bounds':0x568300,
                      'tactical_content':0x6D6D10, 'tactical_dirty_rects':0x6D3290})


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


if __name__ == '__main__':
    argv = sys.argv[1:]
    if '--shroud-admission' in argv:
        argv.remove('--shroud-admission')
        finish_vectors(shroud_admission,
                       Path(__file__).with_suffix('.shroud-admission.json'),
                       provenance=shroud_admission_metadata, argv=argv,
                       source_paths={'bridge_shadow_render.py':Path(__file__)})
    else:
        finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata,
                       argv=argv)
