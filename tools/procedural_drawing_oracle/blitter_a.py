"""Original per-blitter A selection over existing raw surface pixels.

Reuses the native palette oracle's actual Convert/LightConvert/LUT producers.
Only synthetic source/A/Z/destination state is supplied; no game process or
original instruction is patched. This is leaf composition, not a whole scene.
"""
from functools import lru_cache
from itertools import product
from pathlib import Path
import argparse
import struct

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE, UC_MEM_WRITE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EDX, UC_X86_REG_ESI,
                              UC_X86_REG_ESP, UC_X86_REG_EDI)

from tools import native_oracle
from tools.palette_oracle import oracle as palette

PROFILES = {
    'plain1': (1, False, True),
    'light27': (27, False, False),
    'scheme53': (53, True, False),
    'plain53': (53, False, True),
}
RAW = 0xF940
RGB = [200, 100, 50]


@lru_cache(None)
def tables(profile, rgb=tuple(RGB)):
    rows, scheme, plain = PROFILES[profile]
    colors = bytes(rgb) * 256
    mask = native_oracle.file_span(native_oracle.image_bytes(), 0x83E1AC, 256)[1] if scheme else None
    converted = (palette.plain_palette_table(rows, colors) if plain else
                 palette.palette_table(rows, (1000, 1000, 1000), colors, 'mmx', mask))
    return palette.intensity_table(rows), converted


def execute(profile, a, index, brightness):
    u = palette.machine()
    obj, dest, src, z, shape, aobj, zobj, alpha, colors, lut = [palette.HEAP + offset for offset in
        (0, 0x1000, 0x2000, 0x3000, 0x4000, 0x5000, 0x6000, 0x7000, 0x8000, 0x10000)]
    lookup, converted = tables(profile)
    u.mem_write(colors, converted)
    u.mem_write(lut, lookup)
    palette.put32(u, 0x887644, zobj)
    palette.put32(u, zobj + 0x1C, z + 0x10000)
    palette.put32(u, zobj + 0x20, 0x10000)
    palette.put32(u, 0x87E8A4, aobj)
    palette.put32(u, aobj + 0x1C, alpha + 0x10000)
    palette.put32(u, aobj + 0x20, 0x10000)
    u.mem_write(src, bytes([index, 0, 1, index]))  # decoded source, hole, depth reject
    u.mem_write(shape, bytes(3))
    u.mem_write(alpha, struct.pack('<3H', *([a] * 3)))
    results = {}
    for shadow, address in ((False, 0x4990E0), (True, 0x497390)):
        u.mem_write(dest, struct.pack('<3H', *([RAW] * 3)))
        u.mem_write(z, struct.pack('<3H', 65535, 65535, 0))
        if shadow:
            palette.put32(u, obj, 0x7E54B0)
            # Original RGB565 mask established by terrain_draw_oracle/leaf.py.
            u.mem_write(obj + 4, struct.pack('<H', 0x7BEF))
        else:
            u.mem_write(obj, struct.pack('<3I', 0x7E53A0, colors, lut))
        palette.call(u, address, (dest, src, 3, 0, 4096, z, alpha, brightness, 0, shape), obj)
        results['shadow' if shadow else 'body'] = {
            'colors': list(struct.unpack('<3H', u.mem_read(dest, 6))),
            'depths': list(struct.unpack('<3H', u.mem_read(z, 6))),
        }
    return dict(profile=profile, a=a, index=index, brightness=brightness, **results)


def generate():
    return dict(raw=RAW, rgb=RGB, decoded_stencil=['source', 'hole', 'depth_reject'],
                candidate=4096, old_depth=[65535, 65535, 0],
                cases=[execute(*case) for case in product(
                    PROFILES, (0, 1, 2, 63, 126, 127, 128, 254, 255), (1, 240), (1000, 1500))])


def metadata():
    return native_oracle.provenance(
        scope='Original extended body4990E0 and shadow497390 over raw destination pixels, using native-generated palette and A lookup tables.',
        assumptions=[
            'Pinned active-retail image, RGB565/MMX and x87 CW0E7F as established by the palette owner runtime capture.',
            'Synthetic palette RGB200,100,50; native mask83E1AC; N1/N27/N53, plain/LightConvert/ColorScheme producers, brightness1000/1500, indices1/240.',
            'Prepared raw destinationF940, A words0/1/2/63/126/127/128/254/255, row candidate4096, signed shape zero, old Z[65535,65535,0], actual compressed source skip.',
            'The raw destination models an earlier DSurface store. Rally producer execution and its pass admission/order remain in rally.json; this corpus does not execute a whole native frame or derive shroud cells.',
            'Mask7BEF is supplied from the existing original RGB565 mask-constructor corpus. No timer, RNG or detach is involved.',
        ], substitutions=[],
        entry_points=dict(extended_body=0x4990E0, shadow=0x497390,
                          light_convert=0x556090, plain_convert=0x4BBB00, intensity=0x420196))


# Convert48EBF0's RGB565 branch initializes these actual objects. The selector
# executes unchanged, with the executable's file-backed81DC24/28 mask0x3000;
# in particular Unit2804 uses+A4, not the zero-mask alternative+78.
TRANSLUCENT_ROUTES = {
    'shp': (0x490E50, 0x2E00, {
        0x140: 0x7E5400, 0x144: 0x7E53F0, 0x148: 0x7E53E0,
        0x14C: 0x7E53D0, 0x150: 0x7E53C0, 0x154: 0x7E53B0,
        0x160: 0x7E5380, 0x164: 0x7E5370, 0x168: 0x7E5360,
    }),
    'voxel': (0x490B90, 0x2800, {
        0xA0: 0x7E56C0, 0xA4: 0x7E56A8, 0xA8: 0x7E5690,
        0xAC: 0x7E5678, 0xB0: 0x7E5660, 0xB4: 0x7E5648,
        0xC4: 0x7E5600, 0xC8: 0x7E55E8, 0xCC: 0x7E55D0,
    }),
}


@lru_cache(None)
def translucent_selector(route, selector_bits, writes_depth):
    u = palette.machine()
    converter = palette.HEAP
    address, base, slots = TRANSLUCENT_ROUTES[route]
    for slot, vtable in slots.items():
        pointer = palette.HEAP + 0x1000 + slot * 16
        palette.put32(u, converter + slot, pointer)
        palette.put32(u, pointer, vtable)
    palette.put32(u, converter + 8, 1)  # Already initialized; no allocator substitution.
    flags = base | selector_bits | (0x4000 if writes_depth else 0)
    palette.call(u, address, (flags,), converter)
    selected = u.reg_read(UC_X86_REG_EAX)
    slot = next(slot for slot in slots if selected == palette.HEAP + 0x1000 + slot * 16)
    vtable = struct.unpack('<I', u.mem_read(selected, 4))[0]
    leaf = struct.unpack('<I', u.mem_read(vtable + 4, 4))[0]
    return dict(route=route, selector=address, flags=flags, slot=slot, vtable=vtable,
                leaf=leaf, selector_bits=selector_bits, writes_depth=writes_depth)


def translucent_masks(u, converter):
    # Reuse the exact RGB565 initialization range established by terrain/leaf.py,
    # continuing through the quarter-word mask store. Then execute Convert's
    # real+180/+184 stores; only this constructor fragment is isolated.
    for register, value in ((UC_X86_REG_EDX, 3), (UC_X86_REG_EAX, 0), (UC_X86_REG_ESI, 2)):
        u.reg_write(register, value)
    for address, value in ((0x8A0DE0, 5), (0x8A0DD4, 3), (0x8A0DD0, 11)):
        palette.put32(u, address, value)
    native_oracle.run_checked(u, 0x4BAA73, 0x4BAB06, count=200,
                              required_addresses=(0x4BAAC1, 0x4BAAFF))
    u.reg_write(UC_X86_REG_ESI, converter)
    native_oracle.run_checked(u, 0x48EB56, 0x48EB73, count=200,
                              required_addresses=(0x48EB60, 0x48EB6D))
    return {bits: struct.unpack('<H', u.mem_read(converter + offset, 2))[0]
            for bits, offset in ((2, 0x184), (4, 0x180), (6, 0x184))}


def execute_translucent(route, selector_bits, writes_depth, profile, a, index,
                        brightness, raw, rgb=tuple(RGB), displacement=0, trace=False,
                        overlap=False, replays=1):
    if replays < 1:
        raise ValueError('A retained destination must receive at least one native leaf call')
    selected = translucent_selector(route, selector_bits, writes_depth)
    u = palette.machine()
    obj, dest, src, z, shape, aobj, zobj, alpha, colors, lut, converter = [
        palette.HEAP + offset for offset in
        (0, 0x1000, 0x2000, 0x3000, 0x4000, 0x5000, 0x6000, 0x7000,
         0x8000, 0x10000, 0x40000)]
    lookup, converted = tables(profile, rgb)
    u.mem_write(colors, converted)
    u.mem_write(lut, lookup)
    palette.put32(u, 0x887644, zobj)
    palette.put32(u, zobj + 0x1C, z + 0x10000)
    palette.put32(u, zobj + 0x20, 0x10000)
    palette.put32(u, 0x87E8A4, aobj)
    palette.put32(u, aobj + 0x1C, alpha + 0x10000)
    palette.put32(u, aobj + 0x20, 0x10000)
    source = ([index, 240, index] if overlap else
              [index, 0, 1, index] if route == 'shp' else [index, 0, index])
    u.mem_write(src, bytes(source))
    u.mem_write(shape, bytes(3))
    u.mem_write(alpha, struct.pack('<3H', *([a] * 3)))
    old_depth = [65535, 65535, 65535 if overlap else 0]
    # Guarded destination window admits signed offsets and a synthetic eight-
    # word row pitch. These are original leaf inputs, not70BE50 producer values.
    u.mem_write(dest - 16, struct.pack('<19H', *([raw] * 19)))
    if displacement:
        u.mem_write(dest + displacement * 2, struct.pack('<H', raw ^ 0x07E0))
    old_colors = list(struct.unpack('<3H', u.mem_read(dest, 6)))
    u.mem_write(dest, struct.pack('<3H', *old_colors))
    u.mem_write(z, struct.pack('<3H', *old_depth))
    masks = translucent_masks(u, converter)
    mask = masks[selector_bits & 6]
    u.mem_write(obj, struct.pack('<3IH', selected['vtable'], colors, lut, mask))
    accesses = []
    regions = [('destination', dest - 16, 38), ('source', src, 4), ('depth', z, 6),
               ('shape', shape, 3), ('a', alpha, 6), ('palette', colors, len(converted)),
               ('lut', lut, len(lookup))]

    def access(_u, operation, address, size, value, _data):
        for name, start, length in regions:
            if start <= address < start + length:
                read = operation != UC_MEM_WRITE
                actual = int.from_bytes(_u.mem_read(address, size), 'little') if read else value
                accesses.append([name, 'read' if read else 'write',
                                 address - (dest if name == 'destination' else start), size, actual])
                break

    if trace:
        u.hook_add(UC_HOOK_MEM_READ | UC_HOOK_MEM_WRITE, access)
    args = ((dest, src, 3, 0, 4096, z, alpha, brightness, displacement, shape)
            if route == 'shp' else (dest, src, 3, 4096, z, alpha, brightness, displacement))
    # Separate consecutive parent draws retain the same destination and Z.
    # Repeat the original leaf, not the fixture initialization or an inferred
    # blend. A depth-writing first draw can reject a subsequent equal-Z draw.
    for _ in range(replays):
        palette.call(u, selected['leaf'], args, obj)
    result = dict(route=route, selector_bits=selector_bits, writes_depth=writes_depth,
                  profile=profile, a=a, index=index, brightness=brightness,
                  raw=raw, rgb=list(rgb), displacement=displacement, mask=mask,
                  prior_colors=old_colors, prior_depths=old_depth,
                  colors=list(struct.unpack('<3H', u.mem_read(dest, 6))),
                  depths=list(struct.unpack('<3H', u.mem_read(z, 6))))
    if replays != 1:
        result['replay_count'] = replays
        result['replay_native_leaf'] = selected['leaf']
    if overlap:
        result['overlap_sources'] = source
    if trace:
        result['accesses'] = accesses
    return result


def generate_translucent():
    selectors = [translucent_selector(*row) for row in product(
        TRANSLUCENT_ROUTES, (2, 4, 6, 10, 12), (False, True))]
    cases = []
    for row in product(TRANSLUCENT_ROUTES, (2, 4, 6, 10, 12), (False, True),
                       PROFILES, (0, 1, 2, 63, 127, 255), (1, 240), (1000, 1500),
                       (0, RAW, 65535)):
        route, bits, write, profile, a, index, brightness, raw = row
        cases.append(execute_translucent(*row, trace=(profile == 'light27' and a in (2, 127)
            and index == 1 and brightness == 1000 and raw == RAW)))
    # Destination channel boundaries and native-produced source boundaries,
    # rather than a second implementation of packed blend arithmetic.
    boundaries = (0, 1, 0x1F, 0x20, 0x7E0, 0x800, 0xF800, 0xFFFF)
    rgbs = ((0, 0, 0), (8, 4, 8), (248, 252, 248), (255, 255, 255),
            (248, 0, 0), (0, 252, 0), (0, 0, 248))
    for route, bits, rgb, raw in product(TRANSLUCENT_ROUTES, (2, 4, 6), rgbs, boundaries):
        cases.append(execute_translucent(route, bits, False, 'plain1', 127, 1, 1000, raw, rgb))
    for route, bits, a, displacement in product(TRANSLUCENT_ROUTES, (10, 12), (2, 127),
                                              (-8, -1, 1, 8)):
        cases.append(execute_translucent(route, bits, False, 'light27', a, 1, 1000, RAW,
                                        displacement=displacement, trace=True))
    for route, bits, a in product(TRANSLUCENT_ROUTES, (10, 12), (2, 127)):
        cases.append(execute_translucent(route, bits, False, 'scheme53', a, 1, 1000, RAW,
                                        displacement=-1, trace=True, overlap=True))
    replay_controls = [execute_translucent(route, bits, write, 'light27', a, 1,
                                          1000, RAW, replays=3, trace=True)
                       for route, bits, write, a in product(
                           TRANSLUCENT_ROUTES, (2, 4, 6), (False, True), (0, 2, 127))]
    replay_controls += [execute_translucent(route, bits, False, 'light27', a, 1,
                                           1500, RAW, replays=3, trace=True)
                        for route, bits, a in product(
                            TRANSLUCENT_ROUTES, (2, 4, 6), (0, 2, 127))]
    return dict(selectors=selectors, decoded_stencil=['source', 'hole', 'depth_reject'],
                candidate=4096, cases=cases, replay_controls=replay_controls,
                cloak_transition=generate_cloak_transition(),
                cloak_offsets=generate_cloak_offsets())


def signed(value):
    return struct.unpack('<i', struct.pack('<I', value & 0xFFFFFFFF))[0]


def native_unit(u):
    obj, object_type, rules, drive = [palette.HEAP + offset
                                    for offset in (0x50000, 0x52000, 0x56000, 0x59000)]
    palette.put32(u, obj, 0x7F5C70)
    palette.put32(u, obj + 4, 0x7F5C54)
    palette.put32(u, obj + 0x6C4, object_type)
    palette.put32(u, 0x8871E0, rules)
    # Original Drive constructor establishes its ILoco vtable. The actual
    #55ABC0 virtual override returns zero, so4DA4E0 delegates to703860.
    palette.call(u, 0x4AF540, (), drive)
    palette.put32(u, obj + 0x674, drive + 4)
    return obj, object_type, rules


def execute_cloak_transition(stage, depth, owned, force, state=1):
    u = palette.machine()
    obj, object_type, rules = native_unit(u)
    palette.put32(u, obj + 0x220, state)
    palette.put32(u, obj + 0x224, depth)
    palette.put32(u, rules + 0x628, stage)
    u.mem_write(obj + 0x41A, bytes([owned]))
    u.mem_write(object_type + 0xC9A, bytes([0]))
    u.mem_write(0xA8ED6B, bytes([0]))
    palette.put32(u, 0xB73550, 1)  # Active tactical-screen branch; never dereferenced here.
    phases = []

    def capture(_u, address, _size, _data):
        if address == 0x703A94:
            phases.append(signed(_u.reg_read(UC_X86_REG_EAX)))

    u.hook_add(UC_HOOK_CODE, capture)
    palette.call(u, 0x4DA4E0, (force, 0), obj)
    character = u.reg_read(UC_X86_REG_EAX)
    # Execute the exact Unit final-composite selector ladder. Character5 has
    # already suppressed temporary base-source production; its fallthrough is
    # recorded, not treated as permission to emit an opaque Rust base draw.
    u.reg_write(UC_X86_REG_ESI, obj)
    u.reg_write(UC_X86_REG_EAX, character)
    u.reg_write(UC_X86_REG_EDI, 0x2800)
    u.reg_write(UC_X86_REG_ESP, palette.STACK + 0x70000)
    native_oracle.run_checked(u, 0x73B21F, 0x73B259, count=1000)
    flags = u.reg_read(UC_X86_REG_EDI)
    displacement = signed(struct.unpack('<I', u.mem_read(palette.STACK + 0x70010, 4))[0])
    return dict(stage=stage, depth=depth, owned=owned, force=force, state=state,
                scaled=phases[0] if phases else None, character=character,
                final_flags=flags, offset_words=displacement)


def generate_cloak_transition():
    inputs = [(stage, depth) for stage in (0, 1, 9, 12, 13, -1, -9)
              for depth in sorted(set(range(0, max(stage, 0) + 2)) | {-1, 0x7FFFFFFF})]
    rows = [execute_cloak_transition(stage, depth, owned, force)
            for (stage, depth), owned, force in product(inputs, (False, True), (False, True))]
    # Fully cloaked current-house-owned active-screen rendering returns3;
    # explicit force-visible with a null house returns5 before sensors. Allied
    # and detected non-owned branches require their actual producer fixture.
    for state, force in product((0, 2), (False, True)):
        rows.append(execute_cloak_transition(12, 12, True, force, state))
    return rows


def generate_cloak_offsets():
    cases = []
    inputs = [(value, 0.0) for value in (0, 1, 399, 400, 401, 1010667, -1, -400, -401)]
    inputs += [(1, 1.9), (1, -1.9), (0, 399.9), (0x7FFFFFFF, 1.9), (-0x80000000, -1.9)]
    for identity, value in inputs:
        u = palette.machine()
        obj, _type, _rules = native_unit(u)
        palette.put32(u, obj + 0x10, identity)
        raw_float = struct.pack('<f', value)
        u.mem_write(obj + 0x24C, raw_float)
        palette.call(u, 0x70BE50, (), obj)
        cases.append(dict(native_unique_id=identity, float_24c=value,
                          raw_f32_hex=raw_float.hex(),
                          offset_words=signed(u.reg_read(UC_X86_REG_EAX))))
    return cases


def translucent_metadata():
    return native_oracle.provenance(
        scope='Original RGB565 SHP/voxel translucent selector and pixel leaves, including neighbor-destination bit8; isolated from full scene production.',
        assumptions=[
            'Actual Convert490E50/490B90 selectors execute over constructor-derived slot/vtable fixtures; original file-backed mask81DC24/28=0x3000 is retained.',
            'Original4BAA73..4BAB06 produces masks and48EB56..48EB73 writes Convert+180/+184. Blitter+4/+8/+C values follow48EBF0 RGB565 constructor bytes; full allocation is not executed.',
            'N1/N27/N53 palettes/LUTs use existing native palette owner. Synthetic RGB200,100,50 and carry-boundary RGB palettes, A0/1/2/63/127/255, indices1/240, brightness1000/1500, destination0/F940/FFFF and packed channel boundaries.',
            'SHP compressed zero run versus voxel raw zero source; old Z[65535,65535,0], candidate4096, zero signed shape. Depth-read and depth-write controls both execute; write controls do not establish Unit final-composite usage.',
            'Bit8 offset0/plus-or-minus1/plus-or-minus8 controls isolate neighbor destination reads with a synthetic eight-word pitch; offsets are supplied, not claimed as executed cloak70BE50 outputs. Destination memory-trace offsets are relative to the passed destination pointer. Wholebody Unit73B140 caller reading separately establishes flags280x; full Unit temp-cache/row-walker production is not executed.',
            'Separate original4DA4E0->703860 transition controls use real Unit/Drive vtables, TypeInvisible0, state1, current-house-owned flag41A, force0/1, stages0/1/9/12/13/-1/-9 and signed progress; they execute x87 FIDIV/FMUL/ftol and Unit73B21F..73B259 selector, including the native invalid-stage results. State0/2 controls supply owned41A and a nonnull tactical screen; allied/sensor visibility branches are not executed.',
            'Separate70BE50 executes actual Unit secondary vtable7F5C54 getter410220 and ftol7C5F00 over supplied native_unique_id and raw f32+24C. The constructor/world producer lifecycle of the supplied fields is not executed.',
            'No native instruction is changed and no leaf call is substituted. Original pixel leaves contain no RNG, timer or detach calls. Palette/mask/selector ranges are separate executable comparisons.',
            '54 separate sequential parent replay controls invoke the same original SHP/voxel bits2/4/6 leaf three times on one retained destination and depth array: read/write Z, A0/2/127, brightness1000, plus brightness1500 read controls. Source holes and initial strict depth rejection remain; no expected result is calculated in Python.',
        ], substitutions=[],
        entry_points=dict(shp_selector=0x490E50, voxel_selector=0x490B90,
                          mask_begin=0x4BAA73, mask_end=0x4BAB06,
                          convert_masks_begin=0x48EB56, convert_masks_end=0x48EB73,
                          drive_constructor=0x4AF540, foot_visual=0x4DA4E0,
                          visual_character=0x703860, visual_scaled=0x703A94,
                          unit_selector_begin=0x73B21F, unit_selector_end=0x73B259,
                          cloak_offset=0x70BE50, native_id=0x410220, ftol=0x7C5F00,
                          light_convert=0x556090, plain_convert=0x4BBB00, intensity=0x420196))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--translucent', action='store_true')
    options, remaining = parser.parse_known_args()
    native_oracle.finish_vectors(generate_translucent if options.translucent else generate,
        Path(__file__).with_name('translucent_blitter_a.json') if options.translucent else Path(__file__).with_suffix('.json'),
        provenance=translucent_metadata if options.translucent else metadata, argv=remaining,
        source_paths={'oracle': Path(__file__), 'palette_owner': Path(palette.__file__),
                      'native_owner': Path(native_oracle.__file__)})
