"""Unicorn x87 observations from isolated, pinned retail instructions.

The harness supplies operand bits and sequences original instruction fragments;
it does not execute their original parent control flow or model expected math.
Unicorn2.1.4 fails to quiet signaling NaNs on load; x87_masked_hardware records
actual hardware references and retains every disagreement with these observations.
"""
from itertools import product
from pathlib import Path
import hashlib

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_ESP,
    UC_X86_REG_FPCW, UC_X86_REG_FPSW,
)
from tools.native_oracle import (
    RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors, load_image,
    provenance, run_checked,
)

FPCW = 0x0E7F
OWNER, LOCO = SCRATCH + 0x1000, SCRATCH + 0x2000
SP = STACK_BASE + STACK_SIZE - 0x1000
FRAGMENTS = {
    'load_f32': (0x70B743, 'd98630030000'),
    'load_f64': (0x4B1150, 'dd4550'),
    'add': (0x4B1064, 'dec1'),
    'sub': (0x4223F2, 'dee9'),
    'mul': (0x4B105E, 'd8c9'),
    'div': (0x4C2147, 'def9'),
    'compare': (0x412073, 'ded9'),
    'pop': (0x4B106F, 'ddd8'),
    'store_f32': (0x70B812, 'd99e30030000'),
}


def execute(row):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x10000)
    u.mem_map(RET_MAGIC, 0x1000)
    u.reg_write(UC_X86_REG_FPCW, FPCW)
    u.reg_write(UC_X86_REG_ESI, OWNER)
    u.reg_write(UC_X86_REG_EBP, LOCO)
    u.reg_write(UC_X86_REG_ESP, SP)
    assert int.from_bytes(u.mem_read(0x822D80, 4), 'little') == FPCW
    ftol_before = bytes(u.mem_read(0x7C5F00, 0x3D))
    for address, encoded in FRAGMENTS.values():
        assert bytes(u.mem_read(address, len(bytes.fromhex(encoded)))) == bytes.fromhex(encoded)
    visited = []

    def step(name):
        address, encoded = FRAGMENTS[name]
        run_checked(u, address, address + len(bytes.fromhex(encoded)), count=4,
                    required_addresses=(address,))
        visited.append(name)

    def operand(key):
        fmt, bits = row[key + '_format'], row[key + '_bits']
        slot = OWNER + 0x330 if fmt == 'f32' else LOCO + 0x50
        size = 4 if fmt == 'f32' else 8
        u.mem_write(slot, int(bits, 16).to_bytes(size, 'little'))
        step('load_' + fmt)

    operation = row['operation']
    # FCOMPP compares ST0 to ST1; arithmetic pop forms compute ST1 op ST0.
    if operation == 'compare':
        operand('rhs')
        operand('lhs')
        step('compare')
        condition = u.reg_read(UC_X86_REG_FPSW) & 0x4500
        result = dict(ordering={0: 'greater', 0x100: 'less', 0x4000: 'equal',
                               0x4500: 'unordered'}[condition])
    else:
        operand('lhs')
        if operation not in ('load', 'ftol'):
            operand('rhs')
            step(operation)
        if operation == 'ftol':
            u.mem_write(SP - 4, RET_MAGIC.to_bytes(4, 'little'))
            u.reg_write(UC_X86_REG_ESP, SP - 4)
            run_checked(u, 0x7C5F00, RET_MAGIC, count=100,
                        required_addresses=(0x7C5F00,))
            visited.append('ftol_7C5F00')
            raw = u.reg_read(UC_X86_REG_EAX) & 0xFFFFFFFF
            result = dict(integer_low=raw if raw < 0x80000000 else raw - 0x100000000)
        else:
            step('store_f32')
            result = dict(output_bits=f'{int.from_bytes(u.mem_read(OWNER + 0x330, 4), "little"):08x}')
            if operation == 'mul':
                step('pop')
    assert u.reg_read(UC_X86_REG_ESP) == SP
    assert u.reg_read(UC_X86_REG_FPCW) == FPCW
    assert (u.reg_read(UC_X86_REG_FPSW) >> 11) & 7 == 0
    assert bytes(u.mem_read(0x7C5F00, 0x3D)) == ftol_before
    assert int.from_bytes(u.mem_read(0x822D80, 4), 'little') == FPCW
    for address, encoded in FRAGMENTS.values():
        assert bytes(u.mem_read(address, len(bytes.fromhex(encoded)))) == bytes.fromhex(encoded)
    return dict(input=row, **result, original_fragments=visited,
                original_opcodes_unchanged=True)


def cases():
    values = {
        'f32': (0, 0x80000000, 1, 0x3F800000, 0xBF800000, 0x7F7FFFFF,
                0x7F800000, 0xFF800000, 0x7FC00000, 0xFFC00000,
                0x7FC12345, 0xFFC12345, 0x7F812345, 0xFF812346),
        'f64': (0, 0x8000000000000000, 1, 0x3FF0000000000000,
                0xBFF0000000000000, 0x7FEFFFFFFFFFFFFF,
                0x7FF0000000000000, 0xFFF0000000000000,
                0x7FF8000000000000, 0xFFF8000000000000,
                0x7FF82468A0000000, 0xFFF82468A0000000,
                0x7FF02468A0000000, 0xFFF02468C0000001),
    }
    rows = []
    for fmt, operands in values.items():
        digits = 8 if fmt == 'f32' else 16
        for operation, lhs in product(('load', 'ftol'), operands):
            rows.append(dict(name=f'{operation}_{fmt}_{lhs:0{digits}x}',
                             operation=operation, lhs_format=fmt,
                             lhs_bits=f'{lhs:0{digits}x}'))
        for operation, lhs, rhs in product(('add', 'sub', 'mul', 'div', 'compare'), operands, operands):
            rows.append(dict(name=f'{operation}_{fmt}_{lhs:0{digits}x}_{rhs:0{digits}x}',
                             operation=operation, lhs_format=fmt, rhs_format=fmt,
                             lhs_bits=f'{lhs:0{digits}x}', rhs_bits=f'{rhs:0{digits}x}'))
    # Cross-format NaN payload arbitration: equal payload/sign, unequal payload,
    # and tiny f64 payload differences that disappear on the final f32 store.
    for operation, left_format in product(('add', 'sub', 'mul', 'div', 'compare'), values):
        right_format = 'f64' if left_format == 'f32' else 'f32'
        for lhs, rhs in product(values[left_format][8:], values[right_format][8:]):
            rows.append(dict(name=f'mixed_{operation}_{left_format}_{lhs:x}_{rhs:x}',
                             operation=operation, lhs_format=left_format,
                             rhs_format=right_format, lhs_bits=f'{lhs:x}', rhs_bits=f'{rhs:x}'))
    return rows


def generate():
    result = [execute(row) for row in cases()]
    assert len({row['input']['name'] for row in result}) == len(result)
    assert {fragment for row in result for fragment in row['original_fragments']} == set(FRAGMENTS) | {'ftol_7C5F00'}
    return result


def metadata():
    result = provenance(
        scope=f'{len(cases())} Unicorn observations of original gamemd x87 primitive fragments; signaling-NaN load discrepancies require the separate hardware references',
        assumptions=[
            'Harness sequences original instructions with supplied memory/register operands. This proves a bounded primitive value contract, not parent function control flow, gameplay reachability, or an entire x87 stack emulator',
            'Every fragment is asserted against pinned original PE bytes before and after execution; the original _ftol body also executes unchanged with its original cached0E7F control word',
            'Both input precisions cross signed zeros, least positive subnormal, +/-one, maximum finite positive, signed infinity, quiet/signaling NaNs, signs and payloads. Binary operations and compare use every pair of the representative same-format set; mixed rows focus NaN arbitration',
            'Subtraction executes original FSUBP, not FCHS plus FADD, so source NaN signs are independently observed. Division executes FDIVP; FCOMPP condition bits supply ordering and unordered results',
            'Arithmetic results are observed by original binary32 FSTP; load rows also cover quieting and store overflow. _ftol rows observe low EAX from original signed64 conversion. Full extended exponent overflow/underflow, status flags other than comparison conditions, unmasked traps and arbitrary operation chains are excluded',
            'FPCW0E7F is supplied normal startup PC53/chop policy. Actual x87 loads quiet SNaNs, but Unicorn2.1.4 preserves their signaling classification, affecting later NaN selection. These rows deliberately retain raw emulator observations; x87_masked_hardware supplies actual reference values and identifies228 disagreements',
        ],
        substitutions=['Harness chooses operand bits and fragment sequencing; no executable bytes are patched and no Python arithmetic model generates reference outputs'],
        entry_points={**{name: address for name, (address, _) in FRAGMENTS.items()}, 'ftol': 0x7C5F00},
    )
    result['x87_control_word'] = f'{FPCW:04X}'
    result['original_fragments'] = {
        name: dict(address=f'{address:08X}', bytes=encoded,
                   sha256=hashlib.sha256(bytes.fromhex(encoded)).hexdigest())
        for name, (address, encoded) in FRAGMENTS.items()}
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    ftol = bytes(u.mem_read(0x7C5F00, 0x3D))
    result['original_ftol'] = dict(address='007C5F00', end='007C5F3D',
                                  bytes=ftol.hex(), sha256=hashlib.sha256(ftol).hexdigest())
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
