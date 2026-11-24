"""Exhaust the original Walk direction-to-trig lookup over all 16-bit headings.

Enters the original numeric block at 75C067 with SI supplied. Observes the two
original lookup indexes, not a reimplementation of their calculations. The
retail table is extracted unchanged for the shared Rust native_trig owner.
"""
from pathlib import Path
import hashlib
import struct
import sys

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import (
    NATIVE_FPCW, RET_MAGIC, STACK_BASE, STACK_SIZE, finish_vectors, load_image, provenance, run_checked,
)

TABLE = Path(__file__).parents[2] / 'src/util/native_trig_table.bin'


def generate():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)
    u.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    table = bytes(u.mem_read(0x84F084, 10241 * 4))
    indexes = []
    def observe(uc, _address, _size, _data):
        indexes.append(uc.reg_read(UC_X86_REG_EAX))
    u.hook_add(UC_HOOK_CODE, observe, begin=0x4CACEA, end=0x4CACEA)
    u.hook_add(UC_HOOK_CODE, observe, begin=0x4CAD41, end=0x4CAD41)
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.mem_write(sp + 0x14, struct.pack('<i', 1))
    u.mem_write(sp + 0x3C, struct.pack('<ii', 0, 0))
    index_hash, value_hash = hashlib.sha256(), hashlib.sha256()
    facing = 0
    def next_case(uc, _address, _size, _data):
        nonlocal facing
        assert len(indexes) == 2
        index_hash.update(struct.pack('<HH', *indexes))
        for index in indexes:
            value_hash.update(table[index*4:index*4+4])
        indexes.clear()
        facing += 1
        uc.reg_write(UC_X86_REG_ESI, facing)
        uc.reg_write(UC_X86_REG_ESP, sp)
        uc.reg_write(UC_X86_REG_EIP, RET_MAGIC if facing == 65536 else 0x75C067)
    # This hook supplies the NEXT independent CPU frame only after the observed
    # original block ends. One emulator run avoids per-call timeout-thread cost.
    u.hook_add(UC_HOOK_CODE, next_case, begin=0x75C0F3, end=0x75C0F3)
    u.reg_write(UC_X86_REG_ESI, 0)
    u.reg_write(UC_X86_REG_ESP, sp)
    run_checked(u, 0x75C067, RET_MAGIC, count=30_000_000, timeout_us=120_000_000)
    assert facing == 65536
    if '--write' in sys.argv:
        TABLE.write_bytes(table)
    elif '--check' in sys.argv:
        assert TABLE.read_bytes() == table, 'Rust trig table differs from original retail bytes'
    return dict(facing_count=65536, table_address='0084F084', table_length=10241,
                table_sha256=hashlib.sha256(table).hexdigest(),
                sin_cos_index_sha256=index_hash.hexdigest(),
                sin_cos_bits_sha256=value_hash.hexdigest())


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'walk_angle_to_displacement': 0x75C067, 'before_cell_comparison': 0x75C0F3,
                      'sin_read': 0x4CACEA, 'cos_read': 0x4CAD41},
        assumptions=['Supplied SI for each u16 heading; post-getter frame at original numeric block.',
                     'Original 53-bit/chop control; the corpus observes indexes/table bits, not movement admission or placement.'],
        substitutions=['No code patches or supplied results within the executed block; a boundary hook supplies each next independent SI/stack frame.'],
        scope='All 65536 heading words: SHA256 of little-endian u16 sin/cos index pairs and u32 table-value pairs in ascending input order; exact raw source table.',
    ))
