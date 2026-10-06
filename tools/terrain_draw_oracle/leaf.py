"""Original TREE ordinary body/shadow leaf semantics on synthetic surface data.

Set VERA20K_GAMEMD_EXE or RA2_DIR to the pinned executable. No retail assets
are copied. Prepared Convert/A/Z tables isolate pixel compare/write behavior;
the full draw caller and row walker are separate evidence boundaries.
"""
from pathlib import Path
import argparse
import hashlib
import json
import struct
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.native_oracle import *
from unicorn.x86_const import *


def main(output):
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

    obj, dest, source, z, zs = 0x20000000, 0x20001000, 0x20041000, 0x20061000, 0x200a1000
    zobj, a, aobj, pal, lut = 0x200b2000, 0x200b4000, 0x200d4000, 0x200d5000, 0x200e0000
    put32(0x887644, zobj)
    put32(zobj + 0x1c, z + 0x20000, 0x20000, 0x8000, 1024)
    put32(0x87e8a4, aobj)
    put32(aobj + 0x1c, a + 0x20000, 0x20000)
    u.mem_write(a, struct.pack('<65536H', *([127] * 65536)))
    u.mem_write(lut, bytes(0x20000))
    u.mem_write(pal, struct.pack('<256H', *([0x55aa] * 256)))
    # Execute original RGB565 mask construction, with actual shift/loss inputs.
    for register, value in ((UC_X86_REG_EDX, 3), (UC_X86_REG_EAX, 0), (UC_X86_REG_ESI, 2)):
        u.reg_write(register, value)
    for address, value in ((0x8a0de0, 5), (0x8a0dd4, 3), (0x8a0dd0, 11)):
        put32(address, value)
    run_checked(u, 0x4baa73, 0x4baac8, required_addresses=(0x4baac1,))
    mask = struct.unpack('<H', u.mem_read(0x8a0de8, 2))[0]
    assert mask == 0x7bef
    put32(obj, 0x7e54b0)
    u.mem_write(obj + 4, struct.pack('<H', mask))
    u.mem_write(dest, struct.pack('<65536H', *range(65536)))
    u.mem_write(source, bytes(i % 255 + 1 for i in range(65536)))
    u.mem_write(z, struct.pack('<65536H', *([5000] * 65536)))
    u.mem_write(zs, bytes(65536))
    invoke(0x497390, [dest, source, 65536, 0, 4096, z, a, 1000, 0, zs], obj)
    packed = bytes(u.mem_read(dest, 131072))
    assert packed == struct.pack('<65536H', *((v >> 1) & mask for v in range(65536)))
    cases = []
    for shadow, address in ((False, 0x4990e0), (True, 0x497390)):
        if shadow:
            put32(obj, 0x7e54b0)
            u.mem_write(obj + 4, struct.pack('<H', mask))
        else:
            put32(obj, 0x7e53a0, pal, lut)
        for candidate_base in (-65537, -32769, -129, -1, 0, 1, 32767, 32768, 65535, 65536, 100000):
            for depth_byte in (0, 1, 127, 128, 255):
                signed = depth_byte if depth_byte < 128 else depth_byte - 256
                candidate = candidate_base - signed
                for old in (0, 1, 32767, 32768, 65535):
                    u.mem_write(dest, struct.pack('<H', 0xffff))
                    u.mem_write(source, b'\xff')
                    u.mem_write(z, struct.pack('<H', old))
                    u.mem_write(zs, bytes([depth_byte]))
                    invoke(address, [dest, source, 1, 0, candidate_base, z, a, 1000, 0, zs], obj)
                    color, stored = (struct.unpack('<H', u.mem_read(pointer, 2))[0] for pointer in (dest, z))
                    expected = ((mask if shadow else 0x55aa), candidate & 0xffff) if candidate < old else (0xffff, old)
                    assert (color, stored) == expected
                    # A negative signed candidate accepts again after its low16
                    # store; equal in-range candidate rejects. Preserve both.
                    invoke(address, [dest, source, 1, 0, candidate_base, z, a, 1000, 0, zs], obj)
                    repeat = struct.unpack('<H', u.mem_read(dest, 2))[0]
                    expected_repeat = ((color >> 1) & mask if shadow else 0x55aa) if candidate < stored else color
                    assert repeat == expected_repeat
                    cases.append([int(shadow), candidate_base, signed, old, color, stored, repeat])
    # Real compressed skip opcode: one source pixel, skip two, one source pixel.
    # Transparent pixels preserve both attachments even where depth would pass.
    stencil_cases = []
    for shadow, address in ((False, 0x4990e0), (True, 0x497390)):
        if shadow:
            put32(obj, 0x7e54b0)
            u.mem_write(obj + 4, struct.pack('<H', mask))
        else:
            put32(obj, 0x7e53a0, pal, lut)
        u.mem_write(dest, struct.pack('<4H', 0xffff, 0x39e7, 0x07e0, 0xf800))
        u.mem_write(source, bytes([1, 0, 2, 255]))
        u.mem_write(z, struct.pack('<4H', 5000, 5000, 5000, 5000))
        u.mem_write(zs, bytes(4))
        invoke(address, [dest, source, 4, 0, 4096, z, a, 1000, 0, zs], obj)
        colors = list(struct.unpack('<4H', u.mem_read(dest, 8)))
        depths = list(struct.unpack('<4H', u.mem_read(z, 8)))
        assert colors[1:3] == [0x39e7, 0x07e0] and depths == [4096, 5000, 5000, 4096]
        stencil_cases.append({'shadow': shadow, 'decoded_indices': [1, 0, 0, 255],
                              'colors': colors, 'depths': depths})
    output.mkdir(parents=True, exist_ok=True)
    (output / 'half-rgb565.bin').write_bytes(packed)
    result = {'executable_sha256': image_sha256(), 'mask': mask,
              'leaves': ['004990E0', '00497390'], 'packed_cases': 65536,
              'packed_sha256': hashlib.sha256(packed).hexdigest(),
              'depth_record': 'shadow, row candidate, signed shape byte, old u16 Z, output color, stored u16 Z, repeat output color',
              'depth_cases': cases,
              'stencil_cases': stencil_cases,
              'limits': 'prepared synthetic Convert/palette/A/Z and candidate inputs; original leaf calls and mask block; excludes full caller/row walker/scene'}
    (output / 'leaf.json').write_text(json.dumps(result, indent=2) + '\n')
    print(f'Original leaves: {len(cases)} signed/store/repeat cases, all65536 packed colors match')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path)
    main(parser.parse_args().output)
