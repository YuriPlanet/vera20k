"""Original dirty-Z clear stores, with prepared locked buffer/ring/rectangle.

This region deliberately excludes BSurface locking/unlocking and the dirty
rectangle producer. It establishes literal FFFF stores separately from the
8000 row seed, including the original aligned-width-one optimization edge.
"""
import argparse
import json
import struct
from pathlib import Path
from tools.native_oracle import (
    image_sha256, STACK_BASE, STACK_SIZE, Uc, UC_ARCH_X86, UC_MODE_32,
    load_image, run_checked,
)
from unicorn.x86_const import UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP


def generate():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(0x20000000, 0x20000)
    obj, buf, sp = 0x20000000, 0x20001000, STACK_BASE + 0x1000

    def put(address, value):
        u.mem_write(address, struct.pack('<I', value))

    cases = []
    for offset, width, height in ((0,8,2), (2,7,2), (120,8,1), (124,8,2),
                                  (0,3,1), (2,1,1), (0,1,1)):
        u.mem_write(buf, struct.pack('<64H', *([0x1234] * 64)))
        for field, value in ((0x10,0), (0x18,buf), (0x1c,buf+128),
                             (0x20,128), (0x24,0x8000), (0x28,16)):
            put(obj + field, value)
        u.mem_write(sp, bytes(0x100))
        for field, value in ((8,obj), (0x24,width), (0x28,height)):
            put(sp + field, value)
        for reg, value in ((UC_X86_REG_EDI,obj), (UC_X86_REG_ESI,buf+offset),
                           (UC_X86_REG_ESP,sp)):
            u.reg_write(reg, value)
        run_checked(u, 0x7bcfd7, 0x7bd0d7)
        words = list(struct.unpack('<64H', u.mem_read(buf,128)))
        assert all(v in (0x1234,0xffff) for v in words)
        cases.append({'byte_offset': offset, 'width': width, 'height': height,
                      'changed_indices': [i for i,v in enumerate(words) if v == 0xffff],
                      'output_words': words})
    assert cases[0]['changed_indices'] == list(range(8)) + list(range(16,24))
    assert cases[2]['changed_indices'] == list(range(4)) + list(range(60,64))
    assert cases[-2]['changed_indices'] == [1]
    assert cases[-1]['changed_indices'] == []
    return {'executable_sha256': image_sha256(),
            'region': '007BCFD7..007BD0D7', 'empty_word': 65535,
            'separate_row_seed': 32768, 'cases': cases,
            'limit': 'Prepared lock result and ring/rect fields. Does not claim whole dirty-clear optimization equivalence or emulate its width-one aligned edge in the GPU full-frame clear.'}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = generate()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    print('Original dirty-clear stores: 7 cases, FFFF distinct from seed8000')
