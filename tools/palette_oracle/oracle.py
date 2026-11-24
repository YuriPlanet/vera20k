"""Read-only original gamemd x86 execution for palette table research.

No Ghidra/process mutation. Only isolated Unicorn memory is initialized. Output
goldens are bytes executed from the configured image.
"""
from pathlib import Path
import hashlib
import json
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import *

import argparse
from contextlib import contextmanager
from datetime import datetime, timezone
import os
import tempfile

# Keep the historical direct-script entry point usable; all ownership stays in
# the package modules, just as for `python -m tools.palette_oracle.oracle`.
if __package__ in (None, ''):
    import sys
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from tools import native_oracle
from tools.palette_oracle.check import FIXTURES, verify

HEAP = 0x1000000
STACK = 0x2000000
STOP = 0x2100000


def require(condition, detail):
    if not condition:
        raise native_oracle.OracleError(f'Palette comparison failed: {detail}')


def machine():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    native_oracle.load_image(u)
    u.mem_map(HEAP, 0x100000)
    u.mem_map(STACK, 0x100000)
    u.mem_map(STOP, 0x1000)
    # Normal game x87 policy. Math__ftol retains this word after forcing it.
    # Also record the cached word in the report; a live scene read must check it.
    u.reg_write(UC_X86_REG_FPCW, 0x0e7f)
    return u


def put32(u, a, v):
    u.mem_write(a, struct.pack('<I', v & 0xffffffff))


def call(u, address, args, ecx=0):
    sp = STACK + 0x80000
    u.mem_write(sp, struct.pack('<' + 'I' * (len(args) + 1), STOP, *[a & 0xffffffff for a in args]))
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_ECX, ecx)
    native_oracle.run_checked(u, address, STOP, count=15000000)


def intensity_table(n):
    u = machine()
    # Exact generator loop, no allocator/cache scaffolding: EDI=N-1,
    # ESI=0, EBX=output. Stop before native cache append.
    u.reg_write(UC_X86_REG_EDI, n - 1)
    u.reg_write(UC_X86_REG_ESI, 0)
    u.reg_write(UC_X86_REG_EBX, HEAP)
    native_oracle.run_checked(u, 0x420196, 0x4201d5, count=2000000)
    result = bytes(u.mem_read(HEAP, 0x20000))
    require(all(v == min((i & 255) * (i >> 8) * (n - 1) // 32258, n - 1) << 8
               for i, (v,) in enumerate(struct.iter_unpack('<H', result))), "all((v == min((i & 255) * (i >> 8) * (n - 1) // 32258, n - 1) << 8 for (i, (v,)) in enumerate(struct.iter_unpack('<H', result))))")
    return result


def base_scales():
    u = machine()
    out = []
    num, den = struct.unpack('<d', u.mem_read(0x7ed0b0, 8))[0].as_integer_ratio()
    for light in range(2001):
        # Native LEA/SHL, FILD, FMUL, original Math__ftol call.
        u.reg_write(UC_X86_REG_ESP, STACK + 0x80000)
        u.reg_write(UC_X86_REG_EAX, light)
        native_oracle.run_checked(u, 0x556192, 0x5561b1, count=200)
        actual = u.reg_read(UC_X86_REG_EAX)
        require(actual == light * 1000 * num // den, (light, actual))
        require(actual == max(light * 65536 - 1, 0) // 1000, (light, actual))
        out.append(actual)
    return out


def palette_table(n, rgb, palette, mode, mask=None):
    u = machine()
    obj, pal, dest = HEAP, HEAP + 0x1000, HEAP + 0x10000
    put32(u, obj + 0x16c, n)
    put32(u, obj + 0x170, dest)
    put32(u, obj + 0x188, pal)
    if mask is not None:
        put32(u, obj + 0x190, HEAP + 0x2000)
        u.mem_write(HEAP + 0x2000, mask)
    u.mem_write(pal, palette)
    put32(u, 0x829d20, 2)  # RGB565 format dispatch, conditional native mode.
    u.mem_write(0x84e860, bytes((mode == 'cmov', 0, mode == 'mmx')))
    call(u, 0x556090, rgb + (0,), obj)
    return bytes(u.mem_read(dest, n * 512))


def plain_palette_table(n, palette):
    # ConvertClass constructor 0048E740 uses 004BBB00 for RGB565. This route
    # does not dispatch to MMX and has an exact 65536 neutral scale.
    u = machine()
    dest, pal = HEAP, HEAP + 0x10000
    u.mem_write(pal, palette)
    for address, value in ((0x8a0dd0,11),(0x8a0dd4,3),(0x8a0dd8,0),(0x8a0ddc,3),(0x8a0de0,5),(0x8a0de4,2)):
        put32(u,address,value)
    u.reg_write(UC_X86_REG_EDX,n)
    call(u,0x4bbb00,(pal,),dest)
    return bytes(u.mem_read(dest,n*512))


def scanline(n, lut, palette, brightness, abuf):
    u = machine()
    obj, pal, lookup, remap_ptr, remap, dst, src, aa, circ = [HEAP + x for x in (0, 0x1000, 0x10000, 0x31000, 0x32000, 0x33000, 0x34000, 0x35000, 0x36000)]
    for off, value in ((4, remap_ptr), (8, pal), (12, lookup)):
        put32(u, obj + off, value)
    put32(u, remap_ptr, remap)
    u.mem_write(remap, bytes(range(256)))
    u.mem_write(pal, palette)
    u.mem_write(lookup, lut)
    u.mem_write(src, bytes(range(256)))
    u.mem_write(aa, struct.pack('<256H', *([abuf] * 256)))
    u.mem_write(dst, bytes.fromhex('efbe') * 256)
    put32(u, 0x87e8a4, circ)
    put32(u, circ + 0x1c, aa + 512)
    put32(u, circ + 0x20, 512)
    call(u, 0x493f30, (dst, src, 256, 0, 0, aa, brightness, 0), obj)
    return bytes(u.mem_read(dst, 512))


def direct_scanline(lut, palette, brightness, abuf, mode):
    u = machine()
    obj, pal, lookup, dst, src, aa, acirc, zz, zcirc, zsrc = [HEAP + x for x in (0, 0x1000, 0x10000, 0x33000, 0x34000, 0x35000, 0x36000, 0x37000, 0x38000, 0x39000)]
    put32(u, obj + 4, pal)
    put32(u, obj + 8, lookup)
    u.mem_write(pal, palette)
    u.mem_write(lookup, lut)
    u.mem_write(src, bytes(range(1, 256)))
    u.mem_write(aa, struct.pack('<255H', *([abuf] * 255)))
    u.mem_write(zz, b'\xff\xff' * 255)
    for global_ptr, circ, data in ((0x87e8a4, acirc, aa), (0x887644, zcirc, zz)):
        put32(u, global_ptr, circ)
        put32(u, circ + 0x1c, data + 510)
        put32(u, circ + 0x20, 510)
    if mode == 'voxel_cached':
        call(u, 0x497fd0, (dst, src, 255, 0, 0, zz, aa, brightness, 0, zsrc), obj)
    else:
        address = 0x493df0 if mode == 'shp_opaque' else 0x494b60
        call(u, address, (dst, src, 255, 0, zz, aa, brightness, 0), obj)
    return bytes(u.mem_read(dst, 510))


def cell_light_samples(extra_cases=None):
    # Original call setup establishes scale/additive/top/common/bottom/R/G/B
    # ownership. Execute finalization with supplied post-gather inputs, then
    # original Cell stores; no gather, Convert allocation or call substitution.
    u = machine()
    frame, cell, output = STACK + 0x1000, HEAP + 0x1000, HEAP + 0x100
    u.reg_write(UC_X86_REG_ESP, frame)
    u.reg_write(UC_X86_REG_ESI, cell)
    native_oracle.run_checked(u, 0x483eb0, 0x483eda, count=100)
    sp = u.reg_read(UC_X86_REG_ESP)
    offsets = [v - frame for v in struct.unpack('<8I', u.mem_read(sp, 32))]
    require(offsets == [0x20, 0x24, 0x28, 0x2c, 0x30, 0x1c, 4, 8], 'offsets == [32, 36, 40, 44, 48, 28, 4, 8]')
    cases = [
        ((1000, 1000, 1000), 0, 1000, 1000, 2),
        ((1000, 1000, 1000), 0, 600, 600, 2),
        ((1000, 1000, 1000), 0, 900, 900, 2),
        ((290, 390, 740), 0, 863, 871, 2),
        ((1050, 1050, 1010), 200, 1150, 1182, 2),
        ((800, 600, 500), 200, 1000, 1080, 2),
        ((2000, 1000, 1500), 600, 900, 932, 2),
        ((0, 0, 0), 200, 1000, 1080, 2),
        ((240, 480, 800), 0, 1000, 1000, 2),
        ((-100, 0, -1), -200, -100, -50, 2),
        ((3000, 2000, 1999), 3000, 2500, 2600, 2),
    ]
    for maximum in range(2001):
        rgb = (maximum, maximum // 2, maximum * 3 // 4)
        top = (-50, 0, 1, 600, 1000, 1999, 2000, 2500)[maximum % 8]
        for rotation in range(3):
            cases.append((rgb[rotation:] + rgb[:rotation], 200, top, top + 32, maximum % 3))
    if extra_cases is not None:
        cases = extra_cases
    result = bytearray()
    for rgb, additive, top, bottom, detail in cases:
        sp = frame + 0x100
        u.mem_write(output, struct.pack('<8i', 0, additive, top, 0, bottom, *rgb))
        for off, value in ((0x40, output), (0x4c, output + 12), (0x54, output + 20), (0x58, output + 24), (0x5c, output + 28)):
            put32(u, sp + off, value)
        for register, value in ((UC_X86_REG_ESP, sp), (UC_X86_REG_EBX, output + 8), (UC_X86_REG_ESI, output + 16), (UC_X86_REG_FPCW, 0x0e7f)):
            u.reg_write(register, value)
        native_oracle.run_checked(u, 0x4845a2, 0x484617, count=10000)
        actual = list(struct.unpack('<8i', u.mem_read(output, 32)))
        for off, value in zip(offsets, actual):
            put32(u, frame + off, value)
        u.reg_write(UC_X86_REG_ESP, frame)
        u.reg_write(UC_X86_REG_ESI, cell)
        native_oracle.run_checked(u, 0x483f7a, 0x483fd8, count=100)
        stored = [struct.unpack('<I', u.mem_read(cell + 0x104, 4))[0], *struct.unpack('<7H', u.mem_read(cell + 0x108, 14))]
        require(stored == [actual[0], *[v & 0xffff for v in actual[1:]]], 'stored == [actual[0], *[v & 65535 for v in actual[1:]]]')
        # 544E70's neutral fast path retains the identity key. Other keys pass
        # through original 555AC0, whose detail owner is A8EB78.
        if actual[5:] != [1000, 1000, 1000]:
            put32(u, 0xa8eb78, detail)
            u.reg_write(UC_X86_REG_EDX, output + 24)
            call(u, 0x555ac0, (output + 28,), output + 20)
            actual[5:] = struct.unpack('<3i', u.mem_read(output + 20, 12))
        result.extend(struct.pack('<15i', *rgb, additive, top, bottom, detail, *actual))
    return bytes(result), len(cases)


def ground_level_samples():
    # Prepared ReadDouble returns (native float scan widened to double).
    # Execute original FMUL/FADD/full Math__ftol for all four active fields.
    # CCINI allocation/token scanning is outside this isolated fixture.
    u = machine()
    require(struct.unpack('<d', u.mem_read(0x7e4658, 8))[0] == 1000.0, 'native Ground/Level multiplier')
    require(bytes(u.mem_read(0x7f0e78, 4)).hex() == '6f12833a', 'native Ground/Level inverse multiplier')
    u.reg_write(UC_X86_REG_EBP, HEAP)
    native_oracle.run_checked(u, 0x6838f7, 0x68390b, count=10)
    defaults = struct.unpack('<2i', u.mem_read(HEAP + 0x3540, 8))
    require(defaults == (50, 8), 'defaults == (50, 8)')

    def set_st0(value):
        bits = struct.unpack('<Q', struct.pack('<d', value))[0]
        exponent, fraction = (bits >> 52) & 2047, bits & ((1 << 52) - 1)
        significand = (fraction | ((1 << 52) if exponent else 0)) << 11
        extended_exponent = ((bits >> 63) << 15) | (exponent - 1023 + 16383 if exponent else 0)
        u.reg_write(UC_X86_REG_FPSW, 0)
        u.reg_write(UC_X86_REG_FPTAG, 0xfffc)
        u.reg_write(UC_X86_REG_FP0, (significand, extended_exponent))

    def quantize(value, start, end):
        set_st0(value)
        u.reg_write(UC_X86_REG_ESP, STACK + 0x80000)
        native_oracle.run_checked(u, start, end, count=200)
        return struct.unpack('<i', struct.pack('<I', u.reg_read(UC_X86_REG_EAX)))[0]

    sites = ((0x68a92e, 0x68a93f), (0x68a968, 0x68a979),
             (0x68aa8a, 0x68aa9b), (0x68aac4, 0x68aad5))
    tokens = ['0', '.008', '.05', '.20', '.032', '.40', '.5', '.0039', '.0319',
              '.0079', '.0119', '-.032', '-.20', '1.99999', '.00099', '.000989']
    records = []
    for token in tokens:
        value = struct.unpack('<f', struct.pack('<f', float(token)))[0]
        output = [quantize(value, *site) for site in sites]
        require(len(set(output)) == 1, 'len(set(output)) == 1')
        records.append({'token': token, 'f32_widened': value, 'units': output[0]})
    roundtrips = []
    for offset, initial, start, end, site in (
        (0x3540, defaults[0], 0x68a905, 0x68a91f, sites[0]),
        (0x3544, defaults[1], 0x68a93f, 0x68a959, sites[1]),
    ):
        put32(u, HEAP + offset, initial)
        u.reg_write(UC_X86_REG_ESI, HEAP)
        u.reg_write(UC_X86_REG_ESP, STACK + 0x80000)
        u.reg_write(UC_X86_REG_FPSW, 0)
        u.reg_write(UC_X86_REG_FPTAG, 0xffff)
        native_oracle.run_checked(u, start, end, count=20)
        value = struct.unpack('<d', u.mem_read(u.reg_read(UC_X86_REG_ESP), 8))[0]
        actual = quantize(value, *site)
        require(actual == initial, 'actual == initial')
        roundtrips.append({'offset': hex(offset), 'internal': initial,
                           'original_default_double': value, 'units': actual})
    # Scenario arithmetic into the finalizer is separately traced in 484180;
    # these prepared cases cover production grid expectations after parsing.
    cases = [((1000, 1000, 1000), 0, top, bottom, 2) for top, bottom in
             ((950, 982), (982, 1014), (800, 928), (928, 1056), (600, 600), (1000, 1128))]
    cases.append(((290, 390, 740), 0, 875, 919, 2))
    cells, count = cell_light_samples(cases)
    return {'sites': [[hex(a), hex(b)] for a, b in sites], 'authored': records,
            'missing_key_roundtrips': roundtrips,
            'cell_finalizations': [list(row) for row in struct.iter_unpack('<15i', cells)],
            'limits': 'prepared ReadDouble returns and post-gather Cell values; original reset/default inverse/quantization/finalization instructions; excludes CCINI token scanning and map gather'}


def generate():
    outputs = {}
    # Every palette channel takes all 256 byte values, with decorrelated RGB.
    pal = bytes(c for i in range(256) for c in (i, (i * 73) % 256, 255 - i))
    scales = base_scales()
    report = {'exe': native_oracle.configured_gamemd().name, 'unicorn': __import__('unicorn').__version__, 'x87_control_word': '0x0e7f', 'cached_x87_control_word': hex(struct.unpack('<I', native_oracle.file_span(native_oracle.image_bytes(), 0x822d80, 4)[1])[0]), 'scope': 'isolated original x86 table/scanline execution; RGB565 only; no retail scene capture', 'tables': [], 'scanlines': []}
    ground = ground_level_samples()
    # This original-output golden is pinned with CRLF bytes in provenance.json.
    # Emit the same bytes on every platform, independent of text-mode newlines.
    outputs['ground-level.json'] = (json.dumps(ground, indent=2) + '\n').replace('\n', '\r\n').encode('utf-8')
    report['ground_level'] = ground
    cells, cell_count = cell_light_samples()
    outputs['cell-light-finalization.bin'] = cells
    report['cell_light_finalization'] = {'cases': cell_count, 'bytes_sha256': hashlib.sha256(cells).hexdigest(), 'record': '15 little-endian i32: input RGB/additive/top/bottom/detail; output scale/additive/top/common/bottom/quantized RGB', 'limits': 'prepared post-gather values; original call setup/finalization/Cell stores and key quantizer; excludes gather/Convert allocation/scene'}
    outputs['base-scales-0-2000.bin'] = struct.pack('<2001I', *scales)
    report['base_scale_samples'] = {str(i): scales[i] for i in (1, 32, 128, 512, 768, 992, 999, 1000, 1200, 1999, 2000)}
    for n, rgb in ((53, (1000, 1000, 1000)), (53, (992, 768, 512)), (27, (512, 640, 768)), (53, (2000, 1, 1999))):
        lut = intensity_table(n)
        outputs[f'intensity-{n}.bin'] = lut
        tables = {mode: palette_table(n, rgb, pal, mode) for mode in ('scalar', 'cmov', 'mmx')}
        for mode, data in tables.items():
            name = f'palette-{n}-' + '-'.join(map(str, rgb)) + f'-{mode}.bin'
            outputs[name] = data
            words, ref = list(struct.iter_unpack('<H', data)), list(struct.iter_unpack('<H', tables['scalar']))
            diffs = [i for i, pair in enumerate(zip(words, ref)) if pair[0] != pair[1]]
            report['tables'].append({'file': name, 'n': n, 'rgb': rgb, 'mode': mode, 'bytes_sha256': hashlib.sha256(data).hexdigest(), 'scalar_difference_count': len(diffs), 'first_differences': [{'row': i // 256, 'index': i % 256, 'value': words[i][0], 'scalar': ref[i][0]} for i in diffs[:12]]})
        if rgb in ((1000, 1000, 1000), (512, 640, 768)):
            for b in (-1, 0, 1, 992, 999, 1000, 1001, 1199, 1200, 1201, 1999, 2000, 32767):
                for a in (0, 1, 63, 126, 127, 128, 254, 255):
                    data = scanline(n, lut, tables['mmx'], b, a)
                    q = min(max(b, 0) * 261 >> 11, 254)
                    row = min(a * q * (n - 1) // 32258, n - 1)
                    wanted = b'\xef\xbe' + tables['mmx'][row * 512 + 2:(row + 1) * 512]
                    require(data == wanted, (n, b, a, row))
                    for mode in ('shp_opaque', 'voxel_uncached', 'voxel_cached'):
                        actual = direct_scanline(lut, tables['mmx'], b, a, mode)
                        require(actual == wanted[2:], (n, b, a, row, mode))
                    report['scanlines'].append({'n': n, 'brightness': b, 'abuf': a, 'q': q, 'row': row, 'bytes_sha256': hashlib.sha256(data).hexdigest(), 'shp_00493f30': 'match', 'shp_opaque_00493df0': 'match', 'voxel_uncached_00494b60': 'match', 'voxel_cached_00497fd0': 'match'})
    mask = native_oracle.file_span(native_oracle.image_bytes(), 0x83e1ac, 256)[1]
    require([i for i, v in enumerate(mask) if v == 0] == list(range(240, 255)), '[i for (i, v) in enumerate(mask) if v == 0] == list(range(240, 255))')
    report['house_mask_zero_indices'] = list(range(240, 255))
    for rgb in ((1000, 1000, 1000), (992, 768, 512)):
        for mode in ('scalar', 'cmov', 'mmx'):
            data = palette_table(53, rgb, pal, mode, mask)
            name = 'palette-53-' + '-'.join(map(str, rgb)) + f'-{mode}-house.bin'
            outputs[name] = data
            # Verify decoded formula independently against native-executed bytes.
            for row in range(53):
                for i in range(1, 256):
                    channels = pal[i*3:i*3+3]
                    row_scales = [scales[v] * 2 * row // 52 for v in rgb]
                    if not mask[i]:
                        row_scales = [min(row * 65536 // 6, 65536)] * 3
                    if mode == 'mmx':
                        lit = [min(c * (s >> 4) >> 12, 255) for c, s in zip(channels, row_scales)]
                    else:
                        lit = [min(c * s >> 16, 255) for c, s in zip(channels, row_scales)]
                    wanted = (lit[0] >> 3) << 11 | (lit[1] >> 2) << 5 | (lit[2] >> 3)
                    actual = struct.unpack_from('<H', data, (row*256+i)*2)[0]
                    require(actual == wanted, (mode, rgb, row, i, actual, wanted))
            report['tables'].append({'file': name, 'n': 53, 'rgb': rgb, 'mode': mode, 'mask': 'retail0x0083e1ac', 'decoded_formula_all_entries': 'match', 'bytes_sha256': hashlib.sha256(data).hexdigest()})
    # First ColorScheme is N=1 (AltPalette), with the same retail mask.
    data = palette_table(1, (1000,1000,1000), pal, 'mmx', mask)
    name = 'palette-1-1000-1000-1000-mmx-house.bin'
    outputs[name] = data
    report['tables'].append({'file':name,'n':1,'rgb':[1000]*3,'mode':'mmx','mask':'retail0x0083e1ac','bytes_sha256':hashlib.sha256(data).hexdigest()})
    for n in (1,53):
        data = plain_palette_table(n,pal)
        name = f'palette-{n}-plain.bin'
        outputs[name] = data
        for row in range(n):
            scale = 65536 if n==1 else row*131072//(n-1)
            for i in range(256):
                lit = [min(c*scale>>16,255) for c in pal[i*3:i*3+3]]
                wanted = (lit[0]>>3)<<11 | (lit[1]>>2)<<5 | lit[2]>>3
                require(struct.unpack_from('<H',data,(row*256+i)*2)[0] == wanted, "struct.unpack_from('<H', data, (row * 256 + i) * 2)[0] == wanted")
        report['tables'].append({'file':name,'n':n,'rgb':[1000]*3,'mode':'plain','bytes_sha256':hashlib.sha256(data).hexdigest()})
    outputs['oracle-results.json'] = json.dumps(report, indent=2).encode('utf-8')
    print(f"Native original-byte oracle: {len(report['tables'])} complete palettes, {len(report['scanlines']) * 4} scanlines, 2001 base scales, {cell_count} cell finalizations passed")

    return outputs


@contextmanager
def executable_override(path):
    # Resolution and identity remain native_oracle's responsibility. Its cache
    # is keyed by the resolved path, so another prior CLI call cannot mask --exe.
    old = os.environ.get('VERA20K_GAMEMD_EXE')
    if path is not None:
        os.environ['VERA20K_GAMEMD_EXE'] = str(path)
    try:
        yield
    finally:
        if path is not None:
            if old is None:
                os.environ.pop('VERA20K_GAMEMD_EXE', None)
            else:
                os.environ['VERA20K_GAMEMD_EXE'] = old


def source_identity():
    root = Path(__file__).resolve().parents[2]
    return {
        'sources': {name: hashlib.sha256((root / name).read_bytes()).hexdigest() for name in
                    ('tools/palette_oracle/oracle.py', 'tools/palette_oracle/check.py', 'tools/native_oracle.py')},
        'historical_provenance_sha256': hashlib.sha256((FIXTURES / 'provenance.json').read_bytes()).hexdigest(),
    }


def require_unchanged(identity):
    require(source_identity() == identity, 'source or historical provenance changed during replay')


def receipt(outputs, fixtures, identity):
    result = native_oracle.provenance(
        scope='Isolated original palette/scanline and post-gather Cell instructions; RGB565 only.',
        assumptions=[
            'Synthetic RGB(index,index*73 mod256,255-index); supplied scalar/CMOV/MMX dispatch.',
            'Live x87 control word 0x0E7F; cached word and Math__ftol chronology retained from original image.',
            'Prepared ReadDouble returns and Cell inputs; no CCINI scanning or map gather.',
        ],
        substitutions=[],
        entry_points={
            'intensity': 0x420196, 'base_scale': 0x556192, 'lightconvert': 0x556090,
            'shp': 0x493F30, 'shp_opaque': 0x493DF0, 'voxel_uncached': 0x494B60,
            'voxel_cached': 0x497FD0, 'convert': 0x4BBB00, 'cell_call_setup': 0x483EB0,
            'cell_finalize': 0x4845A2, 'cell_stores': 0x483F7A, 'detail_quantizer': 0x555AC0,
            'ground_defaults': 0x6838F7, 'ground': 0x68A92E, 'level': 0x68A968,
            'ion_ground': 0x68AA8A, 'ion_level': 0x68AAC4,
            'ground_default_inverse': 0x68A905, 'level_default_inverse': 0x68A93F,
        },
    )
    result.update({
        'checked_at_utc': datetime.now(timezone.utc).isoformat(),
        'historical_provenance_sha256': identity['historical_provenance_sha256'],
        'fixtures_reproduced': fixtures,
        'generated_sha256': {name: hashlib.sha256(data).hexdigest() for name, data in outputs.items()},
        'limits': 'Fresh isolated execution does not refresh historical active-runtime observations, establish scene reachability, or certify non-clear A-buffer composition, map gathering or whole-scene parity.',
    })
    return result


def export_new(outputs, output):
    """Stage complete bytes, then reserve a new directory without replacing one.

    Execution/comparison failures never publish. I/O failures remove only files
    still matching this export; concurrent modifications are preserved.
    """
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.palette-', dir=output.parent) as temporary:
        staged = Path(temporary)
        for name, data in outputs.items():
            (staged / name).write_bytes(data)
        output.mkdir()  # Exclusive reservation, including a concurrent creator.
        published = []
        try:
            for name, data in outputs.items():
                path = output / name
                with path.open('xb') as stream:
                    published.append((path, data))
                    stream.write((staged / name).read_bytes())
        except BaseException:
            for path, data in reversed(published):
                if path.is_file() and path.read_bytes() == data:
                    path.unlink()
            try:
                output.rmdir()
            except OSError:
                pass  # Preserve partial I/O or concurrent changes for inspection.
            raise


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--check', action='store_true', help='replay and compare; default, no writes')
    mode.add_argument('--write', action='store_true', help='export a checked candidate to a new directory')
    parser.add_argument('--output', type=Path, help='new directory required with --write')
    parser.add_argument('--exe', type=Path, help='override VERA20K_GAMEMD_EXE for this invocation')
    args = parser.parse_args(argv)
    if args.write != (args.output is not None):
        parser.error('--write and --output must be used together')
    if args.output is not None and (args.output.exists() or args.output.is_symlink()):
        parser.error('--output must be a new directory')
    identity = source_identity()
    fixtures = verify(lambda name: (FIXTURES / name).read_bytes(), 'checked-in fixtures')
    require_unchanged(identity)
    with executable_override(args.exe):
        outputs = generate()
        verify(outputs.__getitem__, 'checked native execution')
        require_unchanged(identity)
        if args.write:
            outputs['checked-run.json'] = (json.dumps(receipt(outputs, fixtures, identity), indent=2) + '\n').encode('utf-8')
            require_unchanged(identity)
            export_new(outputs, args.output)
    print(f'{len(fixtures)} historical palette fixtures reproduced byte-for-byte' +
          (f'; exported to {args.output}' if args.write else '; no files written'))


if __name__ == '__main__':
    main()
