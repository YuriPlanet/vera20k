"""Execute original extended-SHP row initialization and walk on synthetic rects.

No retail asset is copied. Prepared walker locals bypass shape decoding and
surface allocation. Original addresses 437B70..437CD4 and 437E82..437EAE
execute without substituted calls. Requires the pinned local gamemd.exe.
"""
import argparse
import json
import struct
from pathlib import Path

from tools.native_oracle import (
    image_sha256, RET_MAGIC, STACK_BASE, STACK_SIZE,
    Uc, UC_ARCH_X86, UC_MODE_32, load_image, run_checked,
)
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EDI,
    UC_X86_REG_ESI, UC_X86_REG_ESP,
)


def generate():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(0x20000000, 0x200000)
    u.mem_map(RET_MAGIC, 0x1000)

    def put(address, *values):
        u.mem_write(address, struct.pack('<' + 'I' * len(values),
                                       *(v & 0xffffffff for v in values)))

    def read(address):
        return struct.unpack('<I', u.mem_read(address, 4))[0]

    zobj, z, rect, clip = 0x200b2000, 0x20061000, 0x20120000, 0x20120020
    put(0x887644, zobj)
    put(zobj + 0x1c, z + 0x20000, 0x20000, 32768, 1024)
    sp = STACK_BASE + 0x2000
    results = []
    for gradient in (0, 2):
        table = 0x817710 + gradient * 24
        for top, height, adjust in ((328, 78, -12), (370, 36, -3),
                                    (3, 1, -12), (3, 2, -12), (3, 3, -12),
                                    (3, 79, -27), (-7, 4, -40000)):
            for skip in range(height):
                put(zobj + 4, 0)
                put(rect, 360, top, 33, height)
                put(clip, 0, 0, 800, 600)
                u.mem_write(sp, bytes(0xc0))
                for offset, value in ((0x38, table), (0x60, top + skip),
                                      (0x80, rect), (0x94, adjust), (0xa4, 0)):
                    put(sp + offset, value)
                for reg, value in ((UC_X86_REG_ESP, sp), (UC_X86_REG_EBP, table),
                                   (UC_X86_REG_EDI, clip), (UC_X86_REG_ESI, top + skip),
                                   (UC_X86_REG_EAX, z)):
                    u.reg_write(reg, value & 0xffffffff)
                run_checked(u, 0x437b70, 0x437cd4)
                seed = u.reg_read(UC_X86_REG_ESI)
                accumulator = read(sp + 0x8c)
                rows = [seed]
                for _ in range(skip + 1, height):
                    run_checked(u, 0x437e82, 0x437eae)
                    rows.append(u.reg_read(UC_X86_REG_ESI))
                results.append([gradient, top, height, adjust, skip, accumulator, rows])
    return {'executable_sha256': image_sha256(),
            'regions': ['00437B70..00437CD4', '00437E82..00437EAE'],
            'case_fields': ['gradient', 'top', 'height', 'adjust', 'skip', 'accumulator', 'native_u32_rows'],
            'cases': results}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = generate()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(f"Original extended-SHP row walker: {len(result['cases'])} clipped initialization/walk cases")
