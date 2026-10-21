"""Execute original Anim AI boundary instructions on supplied runtime/type state.

No full-AI, constructor or session claim. No branch-result hooks or Rust model.
Imports and --help are inert; reference publication uses tools.native_oracle.
"""
from collections import Counter
from itertools import product
from pathlib import Path
import hashlib
import struct

import capstone
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EDI, UC_X86_REG_EFLAGS,
    UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import (
    OracleError, finish_vectors, load_image, provenance, run_checked,
)

MEM = 0x21000000
ANIM, TYPE = MEM, MEM + 0x1000
ENTRY = 0x42468C
STOPS = {
    0x4246DC: 'bounce',
    0x4247B1: 'boundary_loop',
    0x4247F3: 'boundary_terminal',
    0x424B42: 'continue',
}


def signed(value):
    return ((value + 0x80000000) & 0xFFFFFFFF) - 0x80000000


def generate():
    machine = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(machine)
    machine.mem_map(MEM, 0x10000)

    def put(address, value):
        machine.mem_write(address, struct.pack('<I', value & 0xFFFFFFFF))

    def get(address):
        return struct.unpack('<i', machine.mem_read(address, 4))[0]

    put(ANIM + 0xC8, TYPE)
    rows = []
    counts = Counter()
    bounds = [
        ('stock_FH', 0, 0, 32, 15),
        ('stock_FDHD', 16, 0, 32, 31),
        ('stock_EG', 0, 0, 1, 2),
        ('negative_start', -3, -2, 8, 10),
        ('wrapped_loop_difference', -2147483648, 0, 2147483647, 2147483647),
        ('negative_end', 3, -4, -2, -1),
    ]
    for name, start, loop_start, loop_end, end in bounds:
        difference = signed(loop_end - start)
        stages = sorted({signed(x + y) for x in (0, start, end, difference, loop_start)
                         for y in (-1, 0, 1)})
        for loop, stage, shadow, reverse, ctor_reverse, ping, step in product(
                (0, 1, 2, 255), stages, (0, 1), (0, 1), (0, 1), (0, 1),
                (-2147483648, -1, 0, 1)):
            for offset, value in ((0x2B4, start), (0x2B8, loop_start),
                                  (0x2BC, loop_end), (0x2C0, end)):
                put(TYPE + offset, value)
            for offset, value in ((0x370, ping), (0x371, reverse), (0x372, shadow)):
                machine.mem_write(TYPE + offset, bytes([value]))
            machine.mem_write(ANIM + 0x120, bytes([ctor_reverse]))
            machine.mem_write(ANIM + 0x195, bytes([loop]))
            put(ANIM + 0xC4, step)
            put(ANIM + 0xAC, stage)
            for register, value in (
                    (UC_X86_REG_ESI, ANIM), (UC_X86_REG_EAX, TYPE),
                    (UC_X86_REG_EBX, stage & 0xFFFFFFFF), (UC_X86_REG_EDI, 0),
                    (UC_X86_REG_ESP, MEM + 0xFF00), (UC_X86_REG_EFLAGS, 2)):
                machine.reg_write(register, value)
            endpoint = STOPS[run_checked(machine, ENTRY, tuple(STOPS), count=100,
                                         required_addresses=(ENTRY,))]
            result = [endpoint, get(ANIM + 0xC4), machine.mem_read(ANIM + 0x195, 1)[0],
                      get(ANIM + 0xAC)]
            rows.append([name, loop, stage, shadow, reverse, ctor_reverse, ping, step, *result])
            counts[endpoint] += 1
    for name, stage in [('stock_FH', 15), ('stock_FDHD', 16)]:
        row = next(row for row in rows if row[:8] == [name, 1, stage, 1, 0, 0, 0, 1])
        if row[8:] != ['boundary_terminal', 1, 0, stage]:
            raise OracleError(f'Stock-shaped shadow endpoint changed: {row}')
    # Preserve the original recorded disassembly span. Later stops execute from
    # the loaded image without a recorded digest.
    raw = bytes(machine.mem_read(ENTRY, 0x4247B7 - ENTRY))
    native = [f'{instruction.address:08X}: {instruction.mnemonic} {instruction.op_str}'
              for instruction in Cs(CS_ARCH_X86, CS_MODE_32).disasm(raw, ENTRY)]
    return dict(
        region_sha256=hashlib.sha256(raw).hexdigest(),
        entry='0042468C', stops={
            'bounce': '004246DC after original neg/store',
            'boundary_loop': '004247B1 after original loop decrement and stage reset',
            'boundary_terminal': '004247F3 after original loop decrement; before Next/terminal consumers',
            'continue': '00424B42 before epilogue',
        }, native=native, bounds_columns=['name', 'start', 'loop_start', 'loop_end', 'end'],
        bounds=bounds, columns=['bounds', 'loop', 'stage', 'shadow', 'reverse',
                              'constructor_reverse', 'ping_pong', 'step', 'decision',
                              'step_after', 'loop_after', 'stage_after'],
        counts=dict(counts), rows=rows,
    )


def metadata():
    result = provenance(
        scope='13,312 original Anim AI boundary observations: continue, bounce, loop/reset and terminal decisions with step, loop and stage stores. Not full Anim AI or whole-animation parity.',
        assumptions=[
            'Fresh verified PE image and synthetic Anim/type memory. Each case supplies type bounds/flags, committed stage, frame step, remaining loop byte and constructor direction; entry ESI/EAX/EBX/EDI/ESP/EFLAGS match the previous fixture.',
            'One machine is reused across cases in the original order. The original case inputs are rewritten before each entry. No native constructor, CRT, floating-point startup, OS or scenario initialization executes.',
            'Each call must execute42468C and reach4246DC,4247B1,4247F3 or424B42 before the100-instruction/10-second limits. Boundary instructions themselves do not execute.',
            'Recorded region_sha256/disassembly cover42468C..4247B7 only; the later terminal/continue paths execute from the loaded image without a recorded digest.',
            'Stock-shaped bounds are supplied examples, not an ART/type loader witness. Signed/wrapping bounds characterize arithmetic and are not claimed to arise in retail.',
            'Stops precede downstream Next/destruction/epilogue consumers. No timer cadence, RNG ordering, detach calls, constructor or whole-frame lifecycle equivalence is claimed.',
        ],
        substitutions=['None; fixture state is supplied, but no native instruction, branch decision or callee result is replaced.'],
        entry_points=dict(entry=ENTRY, bounce=0x4246DC, loop=0x4247B1,
                          terminal=0x4247F3, continuation=0x424B42),
    )
    result.update(capstone=capstone.__version__)
    return result


def main(argv=None):
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata, argv=argv)


if __name__ == '__main__':
    main()
