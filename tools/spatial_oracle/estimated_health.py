"""Original TechnoAI estimated-health maintenance slice, not the whole AI body."""
from itertools import product
from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_EIP
from tools.native_oracle import (
    SCRATCH, STACK_BASE, STACK_SIZE, load_image, run_checked, finish_vectors, provenance,
)
from tools.spatial_oracle.map_queries import dwords

ENTRY, STOP, OWNER = 0x6F9F6E, 0x6F9F9F, SCRATCH + 0x1000


def execute(actual, estimated, frame):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x10000)
    original = bytes(u.mem_read(ENTRY, STOP - ENTRY))
    u.mem_write(OWNER + 0x68, dwords(0xAABBCCDD, actual, estimated, 0xEEFF0011))
    u.mem_write(0xA8ED84, dwords(frame))
    u.reg_write(UC_X86_REG_ESI, OWNER)
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.reg_write(UC_X86_REG_ESP, sp)
    writes = []

    def observe(uc, _access, address, size, value, _data):
        assert address == OWNER + 0x70 and size == 4
        writes.append(dict(instruction=f'{uc.reg_read(UC_X86_REG_EIP):08X}',
                           value=struct.unpack('<i', dwords(value))[0]))

    u.hook_add(UC_HOOK_MEM_WRITE, observe)
    run_checked(u, ENTRY, STOP, count=50, required_addresses=(ENTRY, 0x6F9F7B))
    assert u.reg_read(UC_X86_REG_ESI) == OWNER and u.reg_read(UC_X86_REG_ESP) == sp
    assert bytes(u.mem_read(ENTRY, STOP - ENTRY)) == original
    assert u.mem_read(OWNER + 0x68, 8) == dwords(0xAABBCCDD, actual)
    assert u.mem_read(OWNER + 0x74, 4) == dwords(0xEEFF0011)
    return dict(actual=actual, estimated=estimated, frame=frame,
                result=struct.unpack('<i', u.mem_read(OWNER + 0x70, 4))[0], writes=writes)


def generate():
    return [execute(actual, estimated, frame) for actual, estimated, frame in product(
        (-2147483648, -1, 0, 1, 100, 65535, 2147483647),
        (-2147483648, -2147483619, -31, -30, -29, -1, 0, 99, 100, 101,
         2147483617, 2147483618, 2147483647),
        (0, 3, 4, 7, 8, 0xFFFFFFFF))]


def metadata():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    result = provenance(
        scope='Original TechnoAI6F9F6E..6F9F9F estimated-health clamp/recovery slice',
        assumptions=[
            'ESI points to supplied object; actual+6C and estimate+70 are signed dwords; absolute binary frame supplied atA8ED84',
            'All91 actual/estimate pairs cross6 frame masks,546 cases; includes raw signed/wrapping boundary probes outside current u16 Rust health range',
            'Stops before6F9F9F without executing class/type callback or whole TechnoAI; earlier berserk and class admission are outside coverage',
            'Original instructions unchanged, hooks only observe; every write must target+70, adjacent owner bytes/ESI/ESP preserved',
        ], substitutions=[], entry_points={'entry': ENTRY, 'stop_before': STOP},
    )
    result['original_code_sha256'] = hashlib.sha256(bytes(u.mem_read(ENTRY, STOP-ENTRY))).hexdigest()
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
