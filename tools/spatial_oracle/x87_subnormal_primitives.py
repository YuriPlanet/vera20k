"""Finite subnormal primitive comparison using original gamemd instructions.

The harness sequences isolated original FLD/FADDP/FCHS/FMUL/FSTP fragments.
This tests their numeric contract, not the original bodies' control flow. There
is no Python arithmetic model or synthesized x87 code generating expected bits.
"""
from itertools import product
from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_ESP,
    UC_X86_REG_FPCW, UC_X86_REG_FPSW, UC_X86_REG_EIP,
)
from tools.native_oracle import (
    SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors, load_image,
    provenance, run_checked,
)

FPCW = 0x0E7F
OWNER, LOCO = SCRATCH + 0x1000, SCRATCH + 0x2000
SP = STACK_BASE + STACK_SIZE - 0x1000
FRAGMENTS = {
    'load_f32': (0x70B743, 'd98630030000'),
    'load_f64': (0x4B1150, 'dd4550'),
    'add_pop': (0x4B1064, 'dec1'),
    'negate': (0x70B5D3, 'd9e0'),
    'multiply': (0x4B105E, 'd8c9'),
    'pop': (0x4B106F, 'ddd8'),
    'store_f32': (0x70B812, 'd99e30030000'),
    'store_f64': (0x4B10F6, 'dd5c2424'),
}


def execute(row):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x10000)
    u.reg_write(UC_X86_REG_FPCW, FPCW)
    u.reg_write(UC_X86_REG_ESI, OWNER)
    u.reg_write(UC_X86_REG_EBP, LOCO)
    u.reg_write(UC_X86_REG_ESP, SP)
    for address, encoded in FRAGMENTS.values():
        expected = bytes.fromhex(encoded)
        assert bytes(u.mem_read(address, len(expected))) == expected
    visited, writes = [], []

    def step(name):
        address, encoded = FRAGMENTS[name]
        run_checked(u, address, address + len(bytes.fromhex(encoded)), count=4,
                    required_addresses=(address,))
        visited.append(name)

    def operand(bits, format):
        slot = OWNER + 0x330 if format == 'f32' else LOCO + 0x50
        size = 4 if format == 'f32' else 8
        u.mem_write(slot, int(bits, 16).to_bytes(size, 'little'))
        step('load_' + format)

    operand(row['lhs_bits'], row['format'])
    if row['operation'] != 'load':
        operand(row['rhs_bits'], row['format'])
        if row['operation'] == 'sub':
            step('negate')
        step('multiply' if row['operation'] == 'mul' else 'add_pop')
    slot = OWNER + 0x330 if row['store'] == 'f32' else SP + 0x24
    size = 4 if row['store'] == 'f32' else 8

    def observe_write(uc, _access, address, width, value, _data):
        assert address == slot and width == size
        writes.append(dict(instruction=f'{uc.reg_read(UC_X86_REG_EIP):08X}',
                           bits=f'{value & ((1 << (8 * width)) - 1):0{2 * width}x}'))

    u.hook_add(UC_HOOK_MEM_WRITE, observe_write)
    step('store_' + row['store'])
    if row['operation'] == 'mul':
        step('pop')
    assert len(writes) == 1
    assert u.reg_read(UC_X86_REG_ESP) == SP
    assert u.reg_read(UC_X86_REG_FPCW) == FPCW
    assert (u.reg_read(UC_X86_REG_FPSW) >> 11) & 7 == 0
    for address, encoded in FRAGMENTS.values():
        expected = bytes.fromhex(encoded)
        assert bytes(u.mem_read(address, len(expected))) == expected
    return dict(input=row, output_bits=f'{int.from_bytes(u.mem_read(slot, size), "little"):0{2 * size}x}',
                original_fragments=visited, store=writes[0],
                original_opcodes_unchanged=True)


def cases():
    rows = []
    magnitudes = {
        'f32': (0, 1, 2, 3, 0x12345, 0x3FFFFF, 0x400000,
                0x7FFFFE, 0x7FFFFF, 0x800000, 0x800001, 0xFFFFFF,
                0x37A7C5AC, 0x3B03126F, 0x3CA3D70A, 0x3E800000, 0x3F800000),
        'f64': (0, 1, 2, 3, 0x123456789, 0x7FFFFFFFFFFFF, 0x8000000000000,
                0xFFFFFFFFFFFFE, 0xFFFFFFFFFFFFF, 0x10000000000000,
                0x10000000000001, 0x1FFFFFFFFFFFFF,
                0x3690000000000000, 0x36A0000000000000, 0x380FFFFFC0000000,
                0x3EF4F8B588E368F1, 0x3F747AE158000000, 0x3FD0000000000000,
                0x3FF0000000000000),
    }
    for format, values in magnitudes.items():
        sign = 0x80000000 if format == 'f32' else 0x8000000000000000
        digits = 8 if format == 'f32' else 16
        for value, negative, store in product(values, (0, sign), ('f32', 'f64')):
            rows.append(dict(name=f'load_{format}_{value | negative:0{digits}x}_{store}',
                             operation='load', format=format,
                             lhs_bits=f'{value | negative:0{digits}x}', store=store))
        # Cancellation and exponent-gap matrix, including both signed zeroes.
        selected = (values[0], values[1], values[2], values[7], values[8],
                    values[9], values[10], values[-1])
        for n, (operation, lhs, rhs, left_sign, right_sign) in enumerate(product(
                ('add', 'sub'), selected, selected, (0, sign), (0, sign))):
            rows.append(dict(name=f'arithmetic_{format}_{n:04}', operation=operation,
                             format=format, lhs_bits=f'{lhs | left_sign:0{digits}x}',
                             rhs_bits=f'{rhs | right_sign:0{digits}x}', store=format))
        factors = ((0, 1, 0x3E800000, 0x3EFFFFFF, 0x3F000000, 0x3F000001,
                    0x3F7FFFFF, 0x3F800000, 0x40000000) if format == 'f32' else
                   (0, 1, 0x3FD0000000000000, 0x3FDFFFFFFFFFFFFF,
                    0x3FE0000000000000, 0x3FE0000000000001,
                    0x3FEFFFFFFFFFFFFF, 0x3FF0000000000000, 0x4000000000000000))
        for n, (lhs, rhs, left_sign, right_sign) in enumerate(product(
                selected, factors, (0, sign), (0, sign))):
            rows.append(dict(name=f'multiply_{format}_{n:04}', operation='mul',
                             format=format, lhs_bits=f'{lhs | left_sign:0{digits}x}',
                             rhs_bits=f'{rhs | right_sign:0{digits}x}', store=format))
    # f64 -> f32 store thresholds at half/one/two minimum subnormals and the
    # maximum-subnormal/minimum-normal transition; +/- one input ULP each.
    for n, (base, delta, sign) in enumerate(product(
            (0x3680000000000000, 0x3690000000000000, 0x36A0000000000000,
             0x36B0000000000000, 0x380FFFFFC0000000, 0x3810000000000000),
            (-1, 0, 1), (0, 0x8000000000000000))):
        rows.append(dict(name=f'store32_threshold_{n:02}', operation='load',
                         format='f64', lhs_bits=f'{(base + delta) | sign:016x}', store='f32'))
    return rows


def generate():
    result = [execute(row) for row in cases()]
    assert len({row['input']['name'] for row in result}) == len(result)
    assert {fragment for row in result for fragment in row['original_fragments']} == set(FRAGMENTS)
    return result


def metadata():
    result = provenance(
        scope=f'{len(cases())} finite subnormal load/add/sub/mul/store primitive cases, executing isolated original gamemd x87 instruction fragments under FPCW0E7F',
        assumptions=[
            'Harness deliberately sequences one-instruction fragments from original Drive TrackProcess and Techno RockingUpdate. This is a primitive numeric contract, not an execution of their original whole-function control flow or gameplay reachability',
            'All fragment bytes are asserted against pinned retail bytes before and after each case. No executable code or constant table is patched. Native execution alone determines output bits; Python only supplies raw operands and records stores',
            'Supplied FPCW0E7F models normal YR startup PC53/chop with masked exceptions. x87 status flags are not comparison outputs; this establishes numeric values under that policy, not full status/trap emulation',
            'Binary32/binary64 loads and same-format add/sub/mul cross signed zeros, minimum subnormals, adjacent subnormal/normal boundaries, cancellation and large exponent gaps. Factors cross zero, least subnormal, quarter, half and adjacent ULPs, one and two',
            'Both destination formats are observed for load rows. Additional f64-to-f32 rows bracket underflow-to-zero, minimum subnormal and minimum normal boundaries. A multiply result may remain nonzero internally far below binary64 storage range before the final store',
            'FADDP operates on two original loaded values; subtraction first executes original FCHS on the right operand. FMUL ST1 keeps the left operand below its result, removed by original FSTP ST0 after the result store. x87 TOP returns to its initial zero',
            'NaN/infinity input, store overflow, full 80-bit exponent under/overflow, divide-by-zero, transcendental instructions, unmasked exceptions and original parent-function control flow are excluded. Existing normal-domain native corpora remain separate regression evidence',
        ],
        substitutions=[
            'The harness supplies ESI/EBP/ESP, operand memory and fragment entry sequencing. There are no receiver stubs, synthesized arithmetic instructions, patched retail instructions or Python-computed expected numeric results',
        ],
        entry_points={name: address for name, (address, _) in FRAGMENTS.items()},
    )
    result['x87_control_word'] = f'{FPCW:04X}'
    result['original_fragments'] = {
        name: dict(address=f'{address:08X}', bytes=encoded,
                   sha256=hashlib.sha256(bytes.fromhex(encoded)).hexdigest())
        for name, (address, encoded) in FRAGMENTS.items()}
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
