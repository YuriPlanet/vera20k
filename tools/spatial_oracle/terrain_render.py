"""Original retained Terrain coordinate -> projection -> body/shadow draw calls.

Run with --check (default) or --write. The original Terrain Render suffix
71CD22..71CD81 executes after its visibility and rectangle-overlap admission.
Caller rows replace CC_Draw_Shape with a recorded sink. Stock frame witnesses
separately execute the original shape/RLE path with an identity-index palette.
"""
import hashlib
import struct
from pathlib import Path

from unicorn import UC_HOOK_CODE
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


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
