"""Original retained Terrain coordinate -> projection -> body/shadow draw calls.

Run with --check (default) or --write. The original Terrain Render suffix
71CD22..71CD81 executes after its visibility and rectangle-overlap admission.
Caller rows replace CC_Draw_Shape with a recorded sink. Stock frame witnesses
separately execute the original shape/RLE path with an identity-index palette.
"""
import hashlib
import struct
from pathlib import Path

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_READ
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP,
    UC_X86_REG_EIP,
)
from tools.native_oracle import (
    RET_MAGIC, configured_gamemd, finish_vectors, provenance, run_checked,
)
from tools.native_slope import slope_matrices
from tools.projectile_oracle.bridge_render_shape import (
    SURFACE, ZOBJ, AOBJ, ZSURF, ASURF, PIXELS, Z, A, packed_words,
)
from tools.sidebar_oracle.stock import mix, mix_hash
from tools.spatial_oracle.bridge_damage_admission import (
    CELL, MEM, SP, base, call, read32, words,
)
from tools.spatial_oracle.terrain_coordinate import generate as coordinate_vectors


def signed_words(u, address, count):
    return list(struct.unpack('<' + 'i' * count, u.mem_read(address, count * 4)))


def execute(case, viewport_y, dirty_y, animation=None):
    u = base(case)
    u.mem_write(0x89E7C0, words(104))
    u.mem_write(0xB0CD48, struct.pack('<Q', 0x3FC25E5374344960))
    for index, matrix in enumerate(slope_matrices()):
        u.mem_write(0xB45188 + 48 * index, struct.pack('<12I', *matrix))
    source, adjusted, obj, observed, typ, shp, tactical, clip = (
        MEM + offset for offset in
        (0x18000, 0x18020, 0x18100, 0x18300, 0x19000, 0x19400, 0x1A000, 0x1C000)
    )
    u.mem_write(source, words(2688, 5248, case['input_z']))
    call(u, 0x71E0D0, args=(adjusted, source))
    u.mem_write(obj, words(0x7F522C))
    u.mem_write(obj + 0xC8, words(typ))
    u.mem_write(obj + 0x6C, words(200))
    u.mem_write(typ, words(0x7F5458))
    u.mem_write(typ + 0xA4, words(shp))
    u.mem_write(shp + 6, struct.pack('<h', 4))
    if animation is not None:
        u.mem_write(typ + 0x2B1, b'\x01')
        u.mem_write(typ + 0x2B3, b'\x01')
        u.mem_write(obj + 0xAC, words(animation['stage']))
        u.mem_write(shp + 6, struct.pack('<h', animation['raw_frame_count']))
    call(u, 0x5F6940, obj, (adjusted,))
    placement = signed_words(u, adjusted, 3)
    # Change the original cell after construction. Rendering reads retained XYZ.
    u.mem_write(CELL + 0x11B, bytes((7, 0)))
    call(u, 0x5F65A0, obj, (observed,))
    retained = signed_words(u, observed, 3)
    u.mem_write(CELL + 0x34, words(MEM + 0x1D000))
    u.mem_write(CELL + 0x10A, struct.pack('<hh', 1000, 1000))
    if animation is not None:
        u.mem_write(0x87F6BC, words(MEM + 0x1D100))
        u.mem_write(CELL + 0x10A, struct.pack(
            '<hh', animation['top_brightness'], animation['ground_brightness']))
    u.mem_write(0x822CF1, b'\x01')
    u.mem_write(0x887324, words(tactical))
    u.mem_write(tactical + 0xB0, words(0, 0))
    u.mem_write(0xB0CE30, words(800, 600))
    u.mem_write(0x886FA0, words(0, viewport_y, 800, 600))
    u.mem_write(clip, words(0, dirty_y, 800, 600))
    # Explicit caller-local state at the suffix entry, after admission.
    u.mem_write(SP - 128, bytes(256))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_EDI, obj)
    u.reg_write(UC_X86_REG_ESI, clip)
    draws, projection, height = [], [], []

    def observe(_uc, address, _size, _data):
        if address == 0x71CD42:
            projection.extend(signed_words(u, SP + 0x10, 2))
        if address == 0x71C256:
            height.append(struct.unpack('<i', words(u.reg_read(UC_X86_REG_EAX)))[0])
        if address != 0x4AED70:
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        raw = bytes(u.mem_read(sp + 4, 56))
        args = list(struct.unpack('<14i', raw))
        draws.append(dict(
            caller=f'{read32(u, sp):08X}', frame=args[1],
            point=signed_words(u, args[2], 2), flags=args[4],
            z_adjust=args[6], gradient=args[7], brightness=args[8],
            raw_args_hex=raw.hex(),
            point_bytes=bytes(u.mem_read(args[2], 8)).hex(),
        ))
        if animation is not None:
            draws[-1]['convert_address'] = u.reg_read(UC_X86_REG_EDX)
        u.reg_write(UC_X86_REG_EAX, 0)
        u.reg_write(UC_X86_REG_EIP, read32(u, sp))
        u.reg_write(UC_X86_REG_ESP, sp + 60)

    u.hook_add(UC_HOOK_CODE, observe)
    required = (
        0x71CD30, 0x41BE00, 0x5F65A0, 0x71CD3D, 0x6D2140,
        0x71CD7B, 0x71C1B0, 0x5F5F30, 0x6D20E0, 0x71C304, 0x71C34E,
    )
    if animation is not None:
        required += (0x71C208, 0x71C2A9)
    run_checked(u, 0x71CD22, 0x71CD81, required_addresses=required)
    assert retained == placement and len(draws) == 2 and len(height) == 1
    result = dict(
        input=case, viewport_y=viewport_y, dirty_y=dirty_y,
        placement=placement, retained=retained,
        retained_bytes=bytes(u.mem_read(observed, 12)).hex(),
        projected_point=projection, projection_bytes=words(*projection).hex(),
        lift_px=height[0], draws=draws,
    )
    if animation is not None:
        result['animation'] = dict(animation, spawns_tiberium=True, is_animated=True)
    return result


def stock_shape_frames():
    """Decode all stock frames through native frame/row/RLE instructions.

    RGB565 output words deliberately equal source indices. This is an index
    witness, not the retail Convert palette or Terrain shadow-color path.
    """
    root = configured_gamemd().parent
    outer_bytes = {name: (root / name).read_bytes()
                   for name in ('ra2.mix', 'ra2md.mix')}
    archives = {name: mix(raw) for name, raw in outer_bytes.items()}
    assets, unique = [], {}
    for theater, outer, inner, ext in (
        ('TEMPERATE', 'ra2.mix', 'temperat.mix', 'tem'),
        ('SNOW', 'ra2.mix', 'snow.mix', 'sno'),
        ('URBAN', 'ra2.mix', 'urban.mix', 'urb'),
        ('NEWURBAN', 'ra2md.mix', 'urbann.mix', 'ubn'),
        ('DESERT', 'ra2md.mix', 'desert.mix', 'des'),
        ('LUNAR', 'ra2md.mix', 'lunar.mix', 'lun'),
    ):
        inner_bytes = archives[outer][mix_hash(inner)]
        entries = mix(inner_bytes)
        for index in range(1, 4):
            name = f'TIBTRE{index:02}.{ext}'
            raw = entries[mix_hash(name)]
            digest = hashlib.sha256(raw).hexdigest()
            assets.append(dict(
                theater=theater, outer_archive=outer, inner_archive=inner,
                inner_sha256=hashlib.sha256(inner_bytes).hexdigest(), name=name,
                entry_id=f'{mix_hash(name):08X}', bytes=len(raw), shp_sha256=digest))
            unique.setdefault(digest, raw)
    assert len(assets) == 18 and len(unique) == 2

    width, height = 128, 64
    count = width * height
    shapes = []
    for digest, raw in unique.items():
        zero, canvas_width, canvas_height, frame_count = struct.unpack_from('<4H', raw)
        assert (zero, canvas_width, canvas_height, frame_count) == (0, 84, 56, 22)
        u = base(dict(level=0, flags=0, input_z=0))
        shp, clip, point, convert, blitter, pal, lut, rect = [MEM + offset for offset in
            (0x80000, 0x18000, 0x18020, 0x90000, 0x91000, 0x92000, 0xA0000, 0x18040)]
        # Stock SHP is too large for bridge_render_shape's small120MM slot.
        # Its own backing prevents aliasing the tactical/clip fixture locals.
        u.mem_write(shp, raw)
        u.mem_write(clip, words(0, 0, width, height))
        u.mem_write(point, words(canvas_width // 2, canvas_height // 2))
        for address, buffer in ((SURFACE, PIXELS), (ZSURF, Z), (ASURF, A)):
            u.mem_write(address, words(0x7E2070, width, height, 0, 2,
                                       buffer, count * 2, 0))
        for address, backing, buffer, global_ in (
            (ZOBJ, ZSURF, Z, 0x887644), (AOBJ, ASURF, A, 0x87E8A4),
        ):
            u.mem_write(address, words(0, 0, width, height, 0, backing,
                                       buffer, buffer + count * 2, count * 2, 32768, width))
            u.mem_write(global_, words(address))
        u.mem_write(convert + 4, words(2, blitter))
        u.mem_write(convert + 0x138, words(blitter))
        u.mem_write(blitter, words(0x7E5420, pal, lut))
        u.mem_write(pal, packed_words(list(range(256))))
        u.mem_write(lut, bytes(0x20000))
        depth = packed_words([65535] * count)
        u.mem_write(Z, depth)
        u.mem_write(A, packed_words([127] * count))
        rows, frames = [], []

        def observe(_u, address, _size, _data):
            if address == 0x497FD0:
                sp = u.reg_read(UC_X86_REG_ESP)
                destination, _source, length, skip = struct.unpack(
                    '<4I', u.mem_read(sp + 4, 16))
                offset = (destination - PIXELS) // 2
                rows.append([offset % width, offset // width, length, skip])

        u.hook_add(UC_HOOK_CODE, observe)
        for frame in range(frame_count):
            assert struct.unpack_from('<I', raw, 16 + frame * 24)[0] == 3
            call(u, 0x69E7E0, shp, (rect, frame))
            x, y, w, h = signed_words(u, rect, 4)
            rows.clear()
            u.mem_write(PIXELS, bytes(count * 2))
            u.mem_write(SP, words(RET_MAGIC, shp, frame, point, clip,
                                  0x2E00, 0, -12, 2, 1000, 0, 0, 0, 0, 0))
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_ECX, SURFACE)
            u.reg_write(UC_X86_REG_EDX, convert)
            run_checked(u, 0x4AED70, RET_MAGIC, count=1_000_000, required_addresses=(
                0x69E7E0, 0x69E740, 0x69E900, 0x490E50, 0x437A10, 0x497FD0))
            assert rows == [[x, y + row, w, 0] for row in range(h)]
            assert 0 <= x < x + w <= width and 0 <= y < y + h <= height
            assert bytes(u.mem_read(Z, len(depth))) == depth
            indices = bytearray()
            for row in range(h):
                pixels = bytes(u.mem_read(PIXELS + ((y + row) * width + x) * 2, w * 2))
                assert pixels[1::2] == bytes(w)
                indices.extend(pixels[::2])
            frames.append(dict(
                frame=frame, piece='body' if frame < 11 else 'shadow', stage=frame % 11,
                native_rect=[x, y, w, h], raw_format=3,
                indices_sha256=hashlib.sha256(indices).hexdigest(),
                nonzero_count=len(indices) - indices.count(0), native_rle_rows=len(rows),
                z_buffer_unchanged=True))
        shapes.append(dict(shp_sha256=digest, canvas=[canvas_width, canvas_height],
                           raw_frame_count=frame_count, frames=frames))
    return dict(
        source_archives=[dict(name=name, bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest())
                         for name, raw in outer_bytes.items()], assets=assets, decoded_shapes=shapes)


def generate():
    # Share the tracked native coordinate corpus's input enumeration.
    cases = [row['input'] for row in coordinate_vectors()['cases']]
    return dict(
        rows=[execute(case, viewport, dirty) for case in cases
              for viewport, dirty in ((0, 0), (37, 100))],
        animated_rows=[
            execute(case, viewport, dirty, dict(
                stage=stage, raw_frame_count=22,
                top_brightness=700, ground_brightness=400))
            for case in (dict(level=0, slope=0, flags=0, input_z=0),
                         dict(level=2, slope=1, flags=0, input_z=0))
            for viewport, dirty in ((0, 0), (37, 100))
            for stage in range(11)
        ],
        stock_shapes=stock_shape_frames(),
    )


def metadata():
    return provenance(
        scope='82 preserved ordinary Terrain render captures plus 44 animated SpawnsTiberium caller captures, and 44 original format-3 index-decode witnesses from the two unique stock TIBTRE byte variants across 18 theater/type assets. Caller rows execute placement/storage/projection/DrawIt arguments; stock rows execute frame/clip/selector/row/RLE-leaf instructions. No visibility admission, retail Convert colors, Terrain shadow pixels, GPU or complete scene parity.',
        assumptions=[
            'Input enumeration comes from tracked terrain_coordinate.generate: levels -128,-1,0,2,127; slopes0,1,2,15; structural flags0/0x100; inputZ0, plus level2/inputZ999. All sourceXY2688,5248 is cell10,20 center. Original71E0D0 clamps placement to ground, then5F6940 stores it. Cell changes to level7/flat before native retained getter and rendering.',
            'Original vtables: Terrain7F522C+48=5F65A0,+AC=41BE00,+104=71CC50,+114=71C1B0,+1D0=5F5F30; TerrainType7F5458+9C=41CFA0. Render suffix71CD22..71CD81 bypasses prior visibility and render-rectangle admission, supplies EDI=Terrain and ESI=clip. Native71CD30 calls center/getter,71CD3D projects,71CD7B invokes DrawIt.',
            'Ordinary nonanimated, non-SpawnsTiberium Terrain with health200, death byte0; synthetic SHP frame count4 and no decoded image; supplied nonnull Cell Convert and both brightness fields1000; shadow enable byte1. No constructors, retail type/image binding or gameplay state transitions claimed here.',
            'Animated rows supply SpawnsTiberium1 and IsAnimated1, stage0..10 and rawSHP count22; these are caller inputs, not executed AI or image-binding results. Cell top/ground brightness700/400 distinguish the native top-brightness choice; body uses supplied global Tiberium Convert2101D100, shadow supplied Cell Convert2101D000 and literal brightness1000. Draw sink also observes the EDX Convert argument. Neither pointer is dereferenced by a blitter.',
            'stock_shapes extracts all 18 TIBTRE01/02/03 assets from ra2.mix and ra2md.mix through existing stock.mix/mix_hash. Binary-identical files share one native execution; recorded outer/inner/SHP hashes bind each alias. Original 69E7E0 supplies frame rectangles; 4AED70 executes physical format-3 frame access 69E740/69E900, clip, selector 490E50, row walker 437A10 and RLE body 497FD0 for all 22 frames of each unique file. No Python RLE decoder is present.',
            'Stock index witnesses use prepared 128x64 RGB565 BSurface and circular A/Z buffer fields shared with bridge_render_shape: background 0, A 127, old Z 65535, baseline 32768, full clip and center 42,28; flags 2E00, Z adjustment -12, gradient 2, brightness 1000 for both body and shadow source frames. Prepared Convert+138 points to original 7E5420 leaf object with zero shade lookup and identity u16 palette. Native output low bytes are cropped by the original frame rectangle, after asserting original row destination/width/left skip and unchanged Z. Tracked witnesses retain the complete crop SHA-256 and counts, not index bytes. This deliberately isolates indices; shadow frames execute the body leaf, not their actual Terrain shadow-color path.',
            'LevelHeight104 and startup AdjustForZ multiplier bits0x3FC25E5374344960 supplied; slope matrices produced by original initializer. Native x87 control0x0E7F. Tactical camera offsets0,0 and dimensions800,600; viewport/dirtyY pairs0/0 and37/100. Native pixels exclude VERA world-row bias15.',
        ],
        substitutions=[
            'In rows and animated_rows, CC_Draw_Shape4AED70 records original caller, all14 raw stack arguments and pointed coordinate bytes; supplies return0 and callee cleanup56. No actual shape decode, blitter, surface, pixels or Z-buffer write. All other reached native calls execute unchanged.',
            'Prepared map/cell/object/type/shape/tactical storage and caller locals are fixture inputs. Original executable code is not patched. Visibility admission and image/resource loading precede the bounded entry and are not emulated.',
            'Stock frame witnesses supply physical SHP/archive bytes and prepared surface/Convert/A/Z data, but replace no reached shape/frame/clip/selector/row/leaf call. They bypass file-resource construction, Terrain AI/DrawIt and retail palette selection; identity Convert output is not a native scene-color claim.',
        ],
        entry_points={
            'type_coordinate': 0x71E0D0, 'set_coords': 0x5F6940,
            'get_coords': 0x5F65A0, 'terrain_render': 0x71CC50,
            'render_suffix_begin': 0x71CD22, 'render_suffix_end': 0x71CD81,
            'projection': 0x6D2140, 'terrain_draw': 0x71C1B0,
            'get_height': 0x5F5F30, 'adjust_for_z': 0x6D20E0,
            'draw_sink': 0x4AED70,
            'stock_frame_rect': 0x69E7E0, 'stock_frame_data': 0x69E740,
            'stock_zshape': 0x69E900, 'stock_selector': 0x490E50,
            'stock_rowwalker': 0x437A10, 'stock_index_leaf': 0x497FD0,
        },
    )


def execute_shroud_admission(case):
    """Whole ordinary static-ground scan, Render and DrawIt; shape is a sink."""
    u = base(dict(level=0, flags=0))
    obj, typ, shp, tactical, clip, source, point, members = (
        MEM + offset for offset in
        (0x18100, 0x19000, 0x19400, 0x1A000, 0x1C000, 0x18000,
         0x18020, 0x1E000))
    u.mem_write(0x89E7C0, words(104))
    u.mem_write(0xB0CD48, struct.pack('<Q', 0x3FC25E5374344960))
    u.mem_write(0x887324, words(tactical))
    u.mem_write(tactical + 0xB0, words(-400, 300))
    u.mem_write(0xB0CE30, words(800, 600))
    u.mem_write(0x886FA0, words(0, 0, 800, 600))
    u.mem_write(clip, words(0, 0, 800, 600))
    u.mem_write(obj, words(0x7F522C))
    u.mem_write(obj + 0x14, words(2))
    u.mem_write(obj + 0x6C, words(200))
    u.mem_write(obj + 0x74, bytes([case.get('marked', 1)]))
    u.mem_write(obj + 0x80, b'\x01')
    u.mem_write(obj + 0x81, bytes([case.get('limbo', 0)]))
    u.mem_write(obj + 0x90, bytes([case.get('alive', 1)]))
    u.mem_write(obj + 0xC8, words(typ))
    u.mem_write(obj + 0xCD, bytes([case.get('crumbling', 0)]))
    u.mem_write(typ, words(0x7F5458))
    u.mem_write(typ + 0xA4, words(shp))
    u.mem_write(typ + 0x2B3, bytes([case.get('animated', 0)]))
    # Synthetic physical header/records; original69E7E0 derives body/shadow
    # extents and original71D160 unites their render rectangle.
    u.mem_write(shp, struct.pack('<4H', 0, 84, 56, 4))
    for frame, rect in ((0, (24, 4, 35, 48)), (2, (34, 28, 44, 16))):
        u.mem_write(shp + 16 + 24 * frame, struct.pack('<4H', *rect))
    u.mem_write(source, words(2688, 5248, 0))
    call(u, 0x5F6940, obj, (source,))
    # Seed the retained screen-coordinate cache from the original projector;
    # its lifecycle/refresh producer is outside this admission fixture.
    call(u, 0x6D2140, tactical, (source, point))
    projected = signed_words(u, point, 2)
    u.mem_write(obj + 0xD8, words(projected[0] - 400,
                                 projected[1] + 300))
    if case.get('offscreen'):
        u.mem_write(obj + 0xD8, words(4000, 4000))
    u.mem_write(CELL + 0x12C, words(case['cell_flags']))
    u.mem_write(CELL + 0x130, words(case['shroud_counter']))
    u.mem_write(CELL + 0x34, words(MEM + 0x1D000))
    u.mem_write(CELL + 0x10A, struct.pack('<hh', 1000, 1000))
    u.mem_write(0x822CF1, b'\x01')
    u.mem_write(0xA8ED6B, b'\x00')
    u.mem_write(0xB73550, words(1))
    u.mem_write(members, words(obj))
    u.mem_write(0x8A0394, words(members))
    u.mem_write(0x8A03A0, words(int(case.get('registered', True))))
    draws, visited, visibility_reads = [], set(), []

    def visibility_read(_uc, _access, address, size, _value, _data):
        visibility_reads.append(dict(address=f'{address:08X}', bytes=size,
                                     instruction=f'{u.reg_read(UC_X86_REG_EIP):08X}'))

    def observe(_uc, address, _size, _data):
        visited.add(address)
        if address != 0x4AED70:
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        raw = bytes(u.mem_read(sp + 4, 56))
        args = list(struct.unpack('<14i', raw))
        draws.append(dict(caller=f'{read32(u, sp):08X}', frame=args[1],
                          point=signed_words(u, args[2], 2), flags=args[4],
                          z_adjust=args[6], gradient=args[7], brightness=args[8],
                          raw_args_hex=raw.hex()))
        u.reg_write(UC_X86_REG_EAX, 0)
        u.reg_write(UC_X86_REG_EIP, read32(u, sp))
        u.reg_write(UC_X86_REG_ESP, sp + 60)

    u.hook_add(UC_HOOK_CODE, observe)
    u.hook_add(UC_HOOK_MEM_READ, visibility_read,
               begin=CELL + 0x12C, end=CELL + 0x133)
    call(u, 0x6D97D0, tactical, (1, clip, clip))
    landmarks = (0x6D97D0, 0x6D9827, 0x71D300, 0x5F6690,
                 0x71D160, 0x69E7E0, 0x6D98E9, 0x71CC50,
                 0x71CD30, 0x71CD7B, 0x71C1B0, 0x71C304, 0x71C34E)
    return dict(input=case, projected_point=projected, draws=draws,
                visibility_reads=visibility_reads,
                reached=[f'{address:08X}' for address in landmarks
                         if address in visited],
                cell_flags_after=read32(u, CELL + 0x12C),
                shroud_counter_after=read32(u, CELL + 0x130))


def generate_shroud_admission():
    cases = [dict(name=f'flags_{flags:02X}_counter_{counter}',
                  cell_flags=flags, shroud_counter=counter)
             for flags in (0, 8, 16, 24) for counter in (0, 1, 255)]
    cases += [dict(name=name, cell_flags=0, shroud_counter=1, **control)
              for name, control in (
                  ('unregistered', dict(registered=False)),
                  ('dead', dict(alive=0)), ('unmarked', dict(marked=0)),
                  ('limbo', dict(limbo=1)), ('animated', dict(animated=1)),
                  ('crumbling', dict(crumbling=1)),
                  ('offscreen', dict(offscreen=True)))]
    rows = [execute_shroud_admission(case) for case in cases]
    assert all(len(row['draws']) == 2 for row in rows[:12])
    assert all(not row['draws'] for row in rows[12:])
    assert all(row['draws'] == rows[0]['draws'] for row in rows[:12])
    assert all(not row['visibility_reads'] for row in rows)
    assert all(len(row['reached']) == 13 for row in rows[:12])
    return dict(rows=rows)


def shroud_admission_metadata():
    return provenance(
        scope='12 ordinary static Terrain6D97D0->71CC50->71C1B0 shroud admission contrasts and seven refusal controls; original rectangle, retained coordinate/projection and draw arguments, not pixels or complete scene parity.',
        assumptions=[
            'Whole static-ground6D97D0 traverses one prepared Ground display member in reverse order. Original Terrain vtable7F522C+2C=71D300 returns24, +44=5F6690 tests Object+90 liveness, +104=71CC50, +12C=71D160 derives the rectangle, +114=71C1B0 draws. This is the static path, not dynamic6D9224.',
            'Original Tactical Draw6D3D10 calls6D3AC0 at6D44D3. The latter calls6D97D0 at6D3BB9/6D3C8E/6D3CB3/6D3CDA/6D3CF7; its full-redraw call supplies viewport rectangles. These callers were instruction/read inspected, not executed here.',
            'Object alive/marked/limbo, display membership, retained XYZ2688,5248,0, flags2 and nonanimated/noncrumbling type state are supplied. Active graphical-clientB73550=1 and ArmageddonA8ED6B=0 select ordinary lifecycle checks. The screen-cacheD8/DC is seeded from actual6D2140 output and camera; its update lifecycle is not established. Camera-400,300 and viewport800x600 place the object onscreen.',
            'Synthetic SHP84x56/rawcount4 records supply body24,4,35,48 and shadow34,28,44,16. Original71D160/69E7E0 derive the rectangle without substitution. Physical retail asset/type binding is outside this admission fixture.',
            'Cell12C flags0/8/10/18 crossed with Cell130 counter0/1/255 are explicit prepared visibility states, not executed reveal producers. Alive0, marked0, limbo1, unregistered, animated1, crumbling1 and offscreen contrasts establish refusals only in this ordinary static scan; animated/crumbling use a different active rendering path.',
            'Native .text is unchanged. Full FogOfWar, gap generators, arbitrary slopes/bridges, palette output, ABuffer/ZBuffer pixels and whole native map loading are outside coverage.',
        ],
        substitutions=[
            'Only CC_Draw_Shape4AED70 is a recording terminal with original56-byte callee cleanup; all14 arguments, caller and pointed draw coordinates are retained. No raster, surface or depth writes execute.',
        ],
        entry_points={'static_scan':0x6D97D0, 'terrain_render':0x71CC50,
                      'terrain_draw':0x71C1B0, 'rectangle':0x71D160,
                      'frame_rect':0x69E7E0, 'is_dead':0x5F6690,
                      'shape_sink':0x4AED70})


if __name__ == '__main__':
    import sys
    if '--shroud-admission' in sys.argv[1:]:
        finish_vectors(generate_shroud_admission,
                       Path(__file__).with_name('terrain_reveal.json'),
                       argv=[arg for arg in sys.argv[1:]
                             if arg != '--shroud-admission'],
                       provenance=shroud_admission_metadata,
                       source_paths={'oracle': Path(__file__),
                                     'fixture': Path('tools/spatial_oracle/bridge_damage_admission.py')})
        raise SystemExit(0)
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
