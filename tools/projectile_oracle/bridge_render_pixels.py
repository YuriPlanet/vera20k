"""Original Bullet-selected RGB565 scanlines, with prepared pixel inputs.

Default/--check never writes. --write records JSON, provenance and the exhaustive
65536-word little-endian shadow output. Full shape decoding is a separate corpus.
"""
from functools import lru_cache
from pathlib import Path
import argparse
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESI,
    UC_X86_REG_ESP,
)
from tools.native_oracle import (
    RET_MAGIC, STACK_BASE, STACK_SIZE, finish_vectors,
    load_image, provenance, run_checked,
)

LEAVES = {'plain_body': 0x494B60, 'rle_body': 0x497FD0,
          'plain_shadow': 0x492D20, 'rle_shadow': 0x496820}


def sequence_inputs():
    """Prepared spans/palettes only; all resulting colors come from the leaves."""
    palettes = (
        {0: 0xffff, 1: 0xf800, 2: 0x07e0, 3: 0x001f, 255: 0xffff},
        {0: 0xffff, 1: 0xffe0, 2: 0xf81f, 3: 0x07ff, 255: 0x39e7},
        {0: 0xffff, 1: 0x801f, 2: 0x0841, 3: 0x55aa, 255: 0xaaaa},
        {0: 0xffff, 1: 0, 2: 0, 3: 0, 255: 0},
    )

    def step(leaf, *, candidate=4096, offset=0, indices=(1, 2, 3, 255),
             page=0, identity=7):
        return dict(leaf=leaf, candidate=candidate, destination_offset=offset,
                    decoded_indices=list(indices), palette_page=page,
                    instance_id=identity,
                    palette_colors=[dict(index=i, color=c)
                                    for i, c in palettes[page].items()])

    def shadows(count, *, offset=0, indices=(1, 2, 3, 255)):
        return [step('rle_shadow' if i % 2 else 'plain_shadow',
                     offset=offset, indices=indices, identity=500 - i)
                for i in range(count)]

    def case(name, steps, *, depths=(5000, 5000, 5000, 5000)):
        return dict(name=name, width=4,
                    initial_colors=[0xffff, 0xf800, 0x07e0, 0x001f],
                    initial_depths=list(depths), steps=steps)

    body = step('plain_body')
    other = step('rle_body', page=1, indices=(255, 3, 2, 1), identity=3)
    interleaved = [body, *shadows(1), other, *shadows(2),
                   step('plain_body', page=2, indices=(3, 2, 1, 255)),
                   *shadows(1)]
    forward = [step('plain_body', offset=0, indices=(1, 2), identity=8),
               step('rle_body', offset=2, indices=(3, 255), page=1, identity=2),
               step('plain_shadow', offset=1, indices=(1, 1)),
               step('rle_body', offset=1, indices=(255, 1), page=2, identity=8),
               step('rle_shadow', offset=2, indices=(1, 0))]
    reverse = [step('rle_body', offset=2, indices=(255, 3), page=1, identity=2),
               step('plain_body', offset=0, indices=(2, 1), identity=8),
               step('plain_shadow', offset=1, indices=(1, 1)),
               step('rle_body', offset=1, indices=(1, 255), page=2, identity=8),
               step('rle_shadow', offset=0, indices=(0, 1))]
    cases = [
        case('shadow_only_eighty', shadows(80)),
        case('body_shadow_palette_interleave', interleaved),
        case('same_pixels_reversed_order', list(reversed(interleaved))),
        case('shadows_before_last_body', [*shadows(8), other, *shadows(5)]),
        case('six_shadows_after_last_body', [body, *shadows(2), other, *shadows(6)]),
        case('transparent_body_does_not_replace', [body, *shadows(1),
             step('rle_body', page=1, indices=(0, 0, 0, 0)), *shadows(1)]),
        case('depth_rejected_body_and_shadow', [
            step('plain_body', candidate=-1),
            step('rle_shadow', candidate=0),
            step('rle_body', candidate=32768, page=1),
            step('plain_shadow', candidate=65536),
            step('plain_body', candidate=1000, page=2),
            step('rle_shadow', candidate=-65537)], depths=(1000, 0, 32768, 65535)),
        case('opaque_black_body_replaces', [body, *shadows(2),
             step('rle_body', page=3), *shadows(1)]),
        case('transparent_palette_zero_is_still_skipped', [body,
             step('plain_shadow', indices=(0, 2, 0, 255)),
             step('plain_body', page=1, indices=(1, 0, 3, 0)),
             step('rle_shadow', indices=(1, 0, 3, 0))]),
        case('overlapping_spans_forward', forward),
        case('overlapping_spans_reversed_indices', reverse),
        case('depth_remains_read_only_between_draws', [
            step('plain_body', candidate=4999),
            step('rle_shadow', candidate=5000),
            step('rle_body', candidate=4998, page=1),
            step('plain_body', candidate=4999, page=2),
            step('plain_shadow', candidate=4999)]),
    ]
    # The original scanline ABI has no object identity. Preserve the same span
    # order with deliberately permuted labels for the GPU batching regression.
    cases.append(case('same_spans_different_instance_ids',
                      [dict(s, instance_id=(i * 7) % 3) for i, s in enumerate(forward)]))
    return cases


@lru_cache
def generate():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(0x20000000, 0x200000)
    u.mem_map(RET_MAGIC, 0x1000)

    def put32(address, *values):
        u.mem_write(address, struct.pack('<' + 'I' * len(values),
                                        *[v & 0xffffffff for v in values]))

    def invoke(address, args, ecx=0):
        sp = STACK_BASE + STACK_SIZE - 0x1000 - 4 * (len(args) + 1)
        put32(sp, RET_MAGIC, *args)
        u.reg_write(UC_X86_REG_ESP, sp)
        u.reg_write(UC_X86_REG_ECX, ecx)
        run_checked(u, address, RET_MAGIC)

    obj, dest, src, z, zs, zobj, a, aobj, pal, lut = [
        0x20000000 + x for x in
        (0, 0x1000, 0x41000, 0x61000, 0xA1000, 0xB2000, 0xB4000,
         0xD4000, 0xE0000, 0x110000)]
    put32(0x887644, zobj)
    put32(zobj + 0x1c, z + 0x20000, 0x20000, 0x8000, 1024)
    put32(0x87E8A4, aobj)
    put32(aobj + 0x1c, a + 0x20000, 0x20000)
    u.mem_write(a, struct.pack('<65536H', *([127] * 65536)))
    u.mem_write(lut, bytes(0x20000))
    u.mem_write(pal, struct.pack('<65536H', *([0x55AA] * 65536)))
    for reg, value in ((UC_X86_REG_EDX, 3), (UC_X86_REG_EAX, 0),
                       (UC_X86_REG_ESI, 2)):
        u.reg_write(reg, value)
    for address, value in ((0x8A0DE0, 5), (0x8A0DD4, 3), (0x8A0DD0, 11)):
        put32(address, value)
    run_checked(u, 0x4BAA73, 0x4BAAC8, required_addresses=(0x4BAAC1,))
    mask = struct.unpack('<H', u.mem_read(0x8A0DE8, 2))[0]

    def setconvert(name):
        if name.endswith('shadow'):
            put32(obj, 0x7E5540 if name.startswith('rle') else 0x7E5948)
            u.mem_write(obj + 4, struct.pack('<H', mask))
        else:
            put32(obj, 0x7E5420 if name.startswith('rle') else 0x7E56F0, pal, lut)

    def draw(name, count, base, offset=0):
        args = ([dest + offset * 2, src, count, 0, base,
                 z + offset * 2, a + offset * 2, 1000, 0, zs]
                if name.startswith('rle') else
                [dest + offset * 2, src, count, base,
                 z + offset * 2, a + offset * 2, 1000, 0])
        invoke(LEAVES[name], args, obj)

    def word(address):
        return struct.unpack('<H', u.mem_read(address, 2))[0]

    rows = []
    for name in LEAVES:
        setconvert(name)
        for candidate in (-65537, -1, 0, 999, 1000, 1001, 32767, 32768, 65535, 65536):
            for old in (0, 1000, 1001, 32768, 65535):
                for transparent in (False, True):
                    u.mem_write(dest, struct.pack('<H', 0xFFFF))
                    u.mem_write(z, struct.pack('<H', old))
                    u.mem_write(zs, b'\0')
                    u.mem_write(src, b'\0\1' if transparent and name.startswith('rle')
                                else bytes([0 if transparent else 1]))
                    draw(name, 1, candidate)
                    first, first_z = word(dest), word(z)
                    draw(name, 1, candidate)
                    rows.append(dict(leaf=name, candidate=candidate, old_z=old,
                                     transparent=transparent, first_color=first,
                                     first_z=first_z, repeat_color=word(dest), repeat_z=word(z)))

    stencil = []
    for name in LEAVES:
        setconvert(name)
        u.mem_write(dest, struct.pack('<4H', 0xffff, 0x39e7, 0x07e0, 0xf800))
        u.mem_write(src, bytes([1, 0, 2, 255]) if name.startswith('rle')
                    else bytes([1, 0, 0, 255]))
        u.mem_write(z, struct.pack('<4H', 5000, 5000, 5000, 5000))
        u.mem_write(zs, bytes(4))
        draw(name, 4, 4096)
        stencil.append(dict(leaf=name, decoded_indices=[1, 0, 0, 255],
                            colors=list(struct.unpack('<4H', u.mem_read(dest, 8))),
                            depths=list(struct.unpack('<4H', u.mem_read(z, 8)))))

    packed, binaries = [], []
    for name in ('plain_shadow', 'rle_shadow'):
        setconvert(name)
        u.mem_write(dest, struct.pack('<65536H', *range(65536)))
        u.mem_write(src, b'\1' * 65536)
        old = struct.pack('<65536H', *([5000] * 65536))
        u.mem_write(z, old)
        u.mem_write(zs, bytes(65536))
        draw(name, 65536, 4096)
        pixels, depth = bytes(u.mem_read(dest, 131072)), bytes(u.mem_read(z, 131072))
        binaries.append(pixels)
        packed.append(dict(leaf=name, pixel_sha256=hashlib.sha256(pixels).hexdigest(),
                           z_sha256=hashlib.sha256(depth).hexdigest(),
                           all_z_unchanged=depth == old, colors=65536))
    assert binaries[0] == binaries[1], 'Plain and RLE original shadow output differs'

    sequences = []
    for case in sequence_inputs():
        width = case['width']
        u.mem_write(dest, struct.pack('<' + 'H' * width, *case['initial_colors']))
        u.mem_write(z, struct.pack('<' + 'H' * width, *case['initial_depths']))
        steps = []
        for prepared in case['steps']:
            name = prepared['leaf']
            indices = prepared['decoded_indices']
            assert prepared['destination_offset'] + len(indices) <= width
            setconvert(name)
            for entry in prepared['palette_colors']:
                u.mem_write(pal + entry['index'] * 2, struct.pack('<H', entry['color']))
            # Encode a zero as the actual RLE skip-one opcode; nonzero bytes
            # are literal source indices. This prepares inputs, not outputs.
            encoded = (b''.join(bytes((value,)) if value else b'\0\1'
                                for value in indices)
                       if name.startswith('rle') else bytes(indices))
            u.mem_write(src, encoded)
            u.mem_write(zs, bytes(len(indices)))
            draw(name, len(indices), prepared['candidate'], prepared['destination_offset'])
            steps.append(dict(prepared,
                              colors=list(struct.unpack('<' + 'H' * width,
                                                        u.mem_read(dest, width * 2))),
                              depths=list(struct.unpack('<' + 'H' * width,
                                                        u.mem_read(z, width * 2)))))
        sequences.append(dict(case, steps=steps, final_colors=steps[-1]['colors'],
                              final_depths=steps[-1]['depths']))
    return dict(leaves={k: f'{v:08X}' for k, v in LEAVES.items()},
                mask=mask, rows=rows, packed=packed, stencil=stencil,
                sequence_cases=sequences), binaries[0]


def metadata():
    return provenance(
        scope='400 original Bullet-selected plain/RLE body/shadow compare, transparency and repeat rows; four multi-pixel transparency rows; both original shadow leaves over all65536 RGB565 destination words; ordered body/shadow pixel sequences. No full DrawIt/rowwalker/scene/GPU claim.',
        assumptions=[
            'Prepared 16-bit surface/A/Z buffers and Convert leaf instances use actual original vtables. Body palette table is deliberately synthetic0x55AA, A127 and lookup offsets zero; no retail color or Convert-construction claim.',
            'Depth candidates and old u16 depths deliberately cross signed/u16 boundaries. RLE Z-shape bytes are zero. Transparent plain index0 and RLE skip opcodes execute unchanged.',
            'RGB565 mask is produced by original4BAA73..4BAAC8 with loss/shift inputs red5/11,green6/5,blue5/0; both exhaustive outputs must agree. No expected pixel output is synthesized.',
            'Sequence cases execute the original leaves in listed order on retained destination/depth buffers, recording every intermediate and final word. Each step supplies exact projected spans, indices and synthetic palette-page entries. Instance IDs and palette-page labels are opaque upstream metadata absent from the native scanline ABI; they do not influence native execution. Sequences include eighty shadows, body resets, rejected depth, index zero, black palette colors and overlapping/reversed spans.',
        ], substitutions=[],
        entry_points=dict(LEAVES, rgb565_mask_begin=0x4BAA73, rgb565_mask_end=0x4BAAC8),
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--write', action='store_true')
    mode.add_argument('--check', action='store_true')
    parser.add_argument('--output', type=Path, default=Path(__file__).with_suffix('.json'))
    args = parser.parse_args()
    data, packed = generate()
    binary = args.output.with_suffix('.rgb565.bin')
    if not args.write:
        assert binary.read_bytes() == packed, f'Original pixel bytes differ: {binary}'
    finish_vectors(data, args.output, provenance=metadata,
                   argv=['--write' if args.write else '--check', '--output', str(args.output)])
    if args.write:
        binary.write_bytes(packed)
        print(f'WROTE {binary}: 65536 native little-endian RGB565 words')


if __name__ == '__main__':
    main()
