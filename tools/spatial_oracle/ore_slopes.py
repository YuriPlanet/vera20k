"""Sloped stock ore through the existing original Cell/SHP raster owner."""
from pathlib import Path
import argparse
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EDI,
                              UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_EIP)
from tools.native_oracle import (SCRATCH, SCRATCH_SIZE, STACK_BASE, STACK_SIZE, call,
    load_image, finish_vectors, provenance, run_checked, configured_gamemd)
from tools.spatial_oracle import bridge_shadow_render as raster
from tools.spatial_oracle.engineer_repair_admission import NativeAudioPlatform


def prepare_slope_shapes(m):
    files = {f'SLOP0{slope}Z.TEM': m.assets[f'SLOP0{slope}Z.TEM']
             for slope in range(1, 5)}
    platform = NativeAudioPlatform(m, configured_gamemd().parent)
    platform.configure_transport(prepared_files=files)
    m.phase = 'ore_slope_shape_loading'
    hook = m.u.hook_add(UC_HOOK_CODE,
        lambda u, pc, n, d: platform.hook(u, pc, n) if pc != 0x7C9430 else None)
    # Supply only sprintf("SLOP01Z.%s", "TEM")'s string. Original CCFile
    # construction, size/allocation/read, subsequent filename mutations and
    # all four pointer publications execute unchanged.
    m.u.mem_write(raster.SP + 0x1F4, b'SLOP01Z.TEM\0')
    m.u.reg_write(UC_X86_REG_ESP, raster.SP)
    run_checked(m.u, 0x54530E, 0x54545D,
        required_addresses=(0x4739F0, 0x473C00, 0x473B10, 0x54533E, 0x545394,
                            0x5453EA, 0x54543F))
    m.u.hook_del(hook)
    for slope in range(1, 5):
        raw = files[f'SLOP0{slope}Z.TEM']
        pointer = m.read32(0xAA105C + slope * 4)
        assert bytes(m.u.mem_read(pointer, len(raw))) == raw
    return [{k: v for k, v in event.items() if k not in ('returned_bytes_hex', 'args')}
            for event in platform.file_io]


def prepare():
    m, types, inputs = raster.prepare(ore=True, ore_slopes=True)
    inputs['slope_shape_io'] = prepare_slope_shapes(m)
    shapes = []
    for slope in range(1, 5):
        name = f'SLOP0{slope}Z.TEM'
        raw = m.assets[name]
        pointer = m.read32(0xAA105C + slope * 4)
        m.invoke(0x69E7E0, pointer, (raster.RECT, 0))
        shapes.append(dict(slope=slope, name=name,
            canvas=list(struct.unpack('<2H', m.u.mem_read(pointer + 2, 4))),
            native_rect=raster.ints(m.u, raster.RECT, 4)))
    inputs['slope_shapes'] = shapes
    return m, types, inputs


def generate():
    m, types, inputs = prepare()
    return collect(m, types, inputs)


def controlled_raster(m, types, case):
    slope = case['slope']
    pointer = m.read32(0xAA105C + slope * 4)
    raw = m.assets[f'SLOP0{slope}Z.TEM']
    data = m.invoke(0x69E740, pointer, (0,))
    m.invoke(0x69E7E0, pointer, (raster.RECT, 0))
    rect = raster.ints(m.u, raster.RECT, 4)
    end = pointer + len(raw)
    surface_end = data + rect[2] * rect[3]
    length = max(0, surface_end - end)
    saved = bytes(m.u.mem_read(end, length))
    m.u.mem_write(end, bytes(length))
    reads = [0, 0]

    def observe(u, access, address, size, value, unused):
        # Both original4990E0 signed-shape reads, before comparison/store.
        if u.reg_read(UC_X86_REG_EIP) not in (0x4991DC, 0x499226):
            return
        reads[0] += 1
        if pointer <= address and address + size <= end:
            return
        if not (end <= address and address + size <= surface_end):
            raise ValueError(f'Uncontrolled native shape address {address:08X}/{size}')
        if bytes(u.mem_read(address, size)) != bytes(size):
            raise ValueError('Native shape read beyond file was not controlled zero')
        reads[1] += 1

    hook = m.u.hook_add(UC_HOOK_MEM_READ, observe)
    try:
        row = raster.execute(m, types, case, ore=True, allow_shape_clip=True)
    finally:
        m.u.hook_del(hook)
        m.u.mem_write(end, saved)
    row['shape_reads'] = dict(total=reads[0], supplied_zero_tail=reads[1])
    return row


def collect(m, types, inputs):
    aliases = []
    for slope in range(1, 5):
        tem, urb = f'SLOP0{slope}Z.TEM', f'SLOP0{slope}Z.URB'
        if m.assets[tem] != m.assets[urb]:
            raise ValueError(f'Different physical slope resources: {tem}/{urb}')
        aliases.append(dict(temperate=tem, urban=urb, byte_identical=True))
    inputs['urban_shape_aliases'] = aliases
    inputs['shape_tail_prior'] = 'signed zero beyond each physical file, restored after each case'
    inputs['material_sources'] = [f'TIB{index:02}.URB' for index in range(1, 21)] + [
        f'SLOP0{slope}Z.URB' for slope in range(1, 5)] + ['TEMPERAT.PAL', 'URBAN.PAL']
    rows = []
    for slope in range(1, 5):
        cases = [dict(name=f'slope{slope}_density{state}', slope=slope, state=state)
                 for state in (0, 4, 11)]
        cases.append(dict(name=f'slope{slope}_clipped', slope=slope, state=4,
                          clip=[80, 50, 30, 80]))
        cases.append(dict(name=f'slope{slope}_alternate_image', slope=slope,
                          coords=[124, 33], state=4))
        for case in cases:
            row = controlled_raster(m, types, case)
            args = row['draws'][0]['raw_args']
            assert args[10] == m.read32(0xAA105C + slope * 4)
            assert args[11:14] == [0, 0, 0]
            rows.append(row)
    source = next(row for row in rows if row['name'] == 'slope2_density4')
    stored = source['output']['runs'][0][4]
    for name, old_z in [('near', stored - 1), ('equal', stored), ('far', stored + 1)]:
        rows.append(controlled_raster(m, types,
            dict(name=f'slope2_depth_{name}', slope=2, state=4, old_z=old_z)))
    return dict(inputs=inputs,
        surface=dict(width=raster.WIDTH, height=raster.HEIGHT, baseline=32768,
                     bytes_per_pixel=2, a_value=127, a_uniform=True), rows=rows)


def metadata():
    return provenance(
        scope='Four original Riparius slope bodies, both extra-image variants, densities0/4/11, clipping and strict-depth controls under an explicitly zeroed surrounding-memory prior; not stock heap, whole native map or game parity.',
        assumptions=[
            'Existing physical RULESMD/MPBattleMD/GoldSt readers, OverlayType constructors/readers and Riparius constructor/readers select TIB01..20. All20 physical TEM art files and their URB aliases are checked by the existing archive owner. No Rust art or output is an input.',
            'Original54530E..545458 constructs CCFiles, reads physical isotemp.mix SLOP01Z.TEM..SLOP04Z.TEM and publishes AA1060..AA106C; the initial sprintf string and OS file transport are supplied. All four loaded buffers must match physical bytes exactly. Original69E7E0, optional-shape setup, clipping4AF08C and extended raster execute unchanged.',
            'Cell coordinates123,33, level5, slope1..4, raw density, projected point66,141, RGB565 backgroundFFFF, oldZ65535 and uniform A127 are supplied component priors. No native map-loader slope production, shroud, harvest scheduling or full Tactical traversal is claimed.',
            'Original shape surfaces use encoded frame bytes without RLE decoding and can read beyond the physical allocation. This corpus supplies signedzero in that trailing nominal surface extent and restores prior memory after each case. Original leaf memory hooks reject reads outside file bytes or the declared zero tail. This compares the explicit Rust safety boundary, not an unknown stock heap layout.',
            'All four physical URB slope files must be byte-identical to TEM before the shared GoldSt/URBAN production witness consumes these component rasters. The three depth priors come from the first stored native slope2/density4 output, not a Rust formula.',
        ],
        substitutions=['Shared PaletteReader allocation/archive services, stated Cell/surface priors and explicitly zeroed memory beyond each slope file; no body, shape, gradient, leaf or expected pixel calculation.'],
        entry_points=dict(cell_body=0x47F6A0, slope_shape_load=0x5452F2,
            shape=0x4AED70, second_shape_clip=0x4AF08C,
            rle_rowwalker=0x437A10, opaque=0x4990E0))


def clip_vectors():
    destination, destination_bounds, source, source_bounds = [SCRATCH + 16 * n for n in range(4)]
    cases = [
        ('inside', [1, 2, 8, 6], [0, 0, 20, 20], [3, 4, 8, 6], [0, 0, 40, 30]),
        ('source_left_top', [-3, -2, 8, 6], [0, 0, 20, 20], [3, 4, 8, 6], [0, 0, 40, 30]),
        ('source_right_bottom', [17, 18, 8, 6], [0, 0, 20, 20], [3, 4, 8, 6], [0, 0, 40, 30]),
        ('destination_left_top', [1, 2, 8, 6], [0, 0, 20, 20], [-3, -2, 8, 6], [0, 0, 40, 30]),
        ('destination_right_bottom', [1, 2, 8, 6], [0, 0, 20, 20], [37, 28, 8, 6], [0, 0, 40, 30]),
        ('outside_source', [21, 2, 8, 6], [0, 0, 20, 20], [3, 4, 8, 6], [0, 0, 40, 30]),
        ('outside_destination', [1, 2, 8, 6], [0, 0, 20, 20], [41, 4, 8, 6], [0, 0, 40, 30]),
        ('unequal', [-3, 2, 8, 6], [0, 0, 20, 20], [37, -2, 10, 7], [0, 0, 40, 30]),
        ('unequal_nonzero_bounds', [1, 2, 8, 6], [3, 4, 20, 20], [37, 28, 10, 7], [5, 6, 40, 30]),
        ('equal_nonzero_bounds', [1, 2, 8, 6], [3, 4, 20, 20], [37, 28, 8, 6], [5, 6, 40, 30]),
        ('empty', [0, 0, 0, 0], [0, 0, 20, 20], [0, 0, 0, 0], [0, 0, 40, 30]),
        ('slope1_intersection', [21, 0, 12, 21], [21, 0, 12, 21], [75, 74, 12, 21], [0, 0, 192, 160]),
        ('slope2_intersection', [14, 0, 22, 13], [14, 0, 22, 13], [82, 73, 22, 13], [0, 0, 192, 160]),
        ('slope3_intersection', [20, 0, 24, 19], [20, 0, 24, 19], [75, 75, 24, 19], [0, 0, 192, 160]),
        ('negative_wrap', [-2147483648, 2, 2147483647, 6], [0, 0, 20, 20], [3, 4, 2147483647, 6], [0, 0, 40, 30]),
        ('positive_wrap', [2147483647, 2, 8, 6], [0, 0, 20, 20], [3, 4, 8, 6], [0, 0, 40, 30]),
        ('unequal_destination_x_wrap', [3, 4, 10, 7], [0, 0, 40, 30], [2147483647, 2, 8, 6], [0, 0, 20, 20]),
        ('unequal_source_x_wrap', [2147483647, 4, 10, 7], [0, 0, 40, 30], [1, 2, 8, 6], [0, 0, 20, 20]),
        ('unequal_destination_y_wrap', [3, 4, 10, 7], [0, 0, 40, 30], [1, 2147483647, 8, 6], [0, 0, 20, 20]),
        ('unequal_source_y_wrap', [3, 2147483647, 10, 7], [0, 0, 40, 30], [1, 2, 8, 6], [0, 0, 20, 20]),
    ]
    rows = []
    for name, src, sb, dst, db in cases:
        out = call(0x7BBE20, ecx=destination, edx=destination_bounds,
            stack_args=[source, source_bounds],
            writes={source: raster.dwords(*src), source_bounds: raster.dwords(*sb),
                    destination: raster.dwords(*dst), destination_bounds: raster.dwords(*db)},
            dumps={'source': (source, 16), 'destination': (destination, 16)},
            timeout_instr=2000)
        rows.append(dict(name=name, source=src, source_bounds=sb, destination=dst,
            destination_bounds=db, accepted=bool(out['eax']),
            clipped_source=list(struct.unpack('<4i', bytes.fromhex(out['dumps']['source']))),
            clipped_destination=list(struct.unpack('<4i', bytes.fromhex(out['dumps']['destination'])))))
    return dict(rows=rows, shape_seeds=shape_seed_vectors())


def shape_seed_vectors():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(SCRATCH, SCRATCH_SIZE)
    u.mem_map(STACK_BASE, STACK_SIZE)
    sp = STACK_BASE + STACK_SIZE - 0x1000
    zbuffer, destination, source = [SCRATCH + 0x100 * n for n in range(3)]
    u.mem_write(0x887644, raster.dwords(zbuffer))
    u.mem_write(zbuffer + 4, raster.dwords(0))
    u.mem_write(zbuffer + 0x24, raster.dwords(32768, 192))
    rows = []
    for gradient in range(3):
        table = 0x817710 + gradient * 24
        fields = struct.unpack('<6i', u.mem_read(table, 24))
        for screen_top, height, z_adjust in [(0, 8, -1), (73, 13, -77), (75, 19, -77),
                                            (-2, 7, -2), (32768, 3, -2)]:
            u.mem_write(sp, bytes(0xB0))
            u.mem_write(sp + 0x20, raster.dwords(fields[2]))
            u.mem_write(sp + 0x80, raster.dwords(source))
            u.mem_write(sp + 0x94, raster.dwords(z_adjust))
            u.mem_write(sp + 0xA4, raster.dwords(SCRATCH + 0x300))
            u.mem_write(destination, raster.dwords(0, screen_top))
            u.mem_write(source, raster.dwords(0, 0, 1, height))
            for register, value in [(UC_X86_REG_ESP, sp), (UC_X86_REG_EAX, zbuffer),
                    (UC_X86_REG_EBP, table), (UC_X86_REG_EDI, destination),
                    (UC_X86_REG_ESI, screen_top & 0xFFFFFFFF)]:
                u.reg_write(register, value)
            # Stop before the optional shape's virtual GetRect. The original
            # gradient branch and all seed arithmetic execute, with no hooks.
            run_checked(u, 0x437B8B, 0x437BDF, count=1000,
                required_addresses=(0x437BAB if fields[5] else 0x437C39,))
            seed = struct.unpack('<i', struct.pack('<I', u.reg_read(UC_X86_REG_ESI)))[0]
            rows.append(dict(gradient=gradient, screen_top=screen_top, height=height,
                             z_adjust=z_adjust, seed=seed))
    return rows


def clip_metadata():
    return provenance(scope='Original coupled source/destination clipping and optional-shape gradient seeding; supplied component priors, not full SHP raster parity.',
        assumptions=['Four separate supplied signed32 rectangles; original7BBE20 and its421B60 calls execute unchanged, with complete returned rectangles recorded.',
            'The slope-named controls use the observed rectangle dimensions as component priors. Their inclusion does not itself establish a complete caller state.',
            'Shape seed controls execute437B8B until437BDF, before the virtual shape GetRect. Original817710 gradient entries are read from the image; registers/stack provide the declared top/height/Z term, a nonnull shape and a supplied ZBuffer with default32768/YOrigin0. No seed result or instruction is substituted.'],
        substitutions=[], entry_points=dict(clip_blit_rects=0x7BBE20, clip_rect=0x421B60,
            shape_gradient_seed=0x437B8B))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--clip-only', action='store_true')
    args, remaining = parser.parse_known_args()
    finish_vectors(clip_vectors if args.clip_only else generate,
        Path(__file__).with_name('ore_shape_clip.json') if args.clip_only else Path(__file__).with_suffix('.json'),
        provenance=clip_metadata if args.clip_only else metadata, argv=remaining)
