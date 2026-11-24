"""Original Drive/Ship Process_Track entry eligibility and residual reset.

The original prologue, conditionals, Unit type getters and early return execute.
Execution stops before the speed-prefix body on admission. No branch result,
virtual return or direction is supplied by a hook, and no code is substituted.
"""
from itertools import product
from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import (
    RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors, load_image,
    provenance, run_checked,
)
from tools.spatial_oracle.map_queries import dwords

LOCO, OWNER, TYPE = SCRATCH + 0x1000, SCRATCH + 0x2000, SCRATCH + 0x4000
SP = STACK_BASE + STACK_SIZE - 0x1000
UNIT_VTABLE, GET_TYPE, UNIT_TYPE = 0x7F5C70, 0x6F3270, 0x741490
RESIDUAL = 0x12345678
FAMILIES = {
    'drive': dict(entry=0x4B0F20, prefix=0x4B0F69, early_clear=0x4B25F2,
                  early_ret=0x4B2605, type_call=0x4B0F55),
    'ship': dict(entry=0x6A05F0, prefix=0x6A0639, early_clear=0x6A1C43,
                 early_ret=0x6A1C56, type_call=0x6A0625),
}
CODE_RANGES = ((0x4B0F20, 0x4B0F69), (0x4B25F2, 0x4B2608),
               (0x6A05F0, 0x6A0639), (0x6A1C43, 0x6A1C59),
               (GET_TYPE, GET_TYPE + 8), (UNIT_TYPE, UNIT_TYPE + 7))


def read_i32(u, address):
    return struct.unpack('<i', u.mem_read(address, 4))[0]


def execute(row):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x10000)
    u.mem_map(RET_MAGIC, 0x1000)
    original_code = [bytes(u.mem_read(a, b - a)) for a, b in CODE_RANGES]
    original_vtable = bytes(u.mem_read(UNIT_VTABLE, 0x600))
    u.mem_write(OWNER, dwords(UNIT_VTABLE))
    u.mem_write(OWNER + 0x6C4, dwords(TYPE))
    u.mem_write(OWNER + 0x90, b'\x01')
    u.mem_write(OWNER + 0x5E0, dwords(row['queue_head']))
    u.mem_write(TYPE + 0xCA1, bytes((row['turret'],)))
    u.mem_write(LOCO + 0xC, dwords(OWNER))
    u.mem_write(LOCO + 0x4C, dwords(RESIDUAL))
    u.mem_write(LOCO + 0x58, dwords(row['selector'], 0x76543210))
    u.mem_write(LOCO + 0x62, bytes((row['class_flag_62'], row['track_valid'])))
    u.mem_write(SP, dwords(RET_MAGIC, 0))
    u.reg_write(UC_X86_REG_ECX, LOCO)
    u.reg_write(UC_X86_REG_ESP, SP)
    saved_registers = ((UC_X86_REG_EBX, 0x13579BDF), (UC_X86_REG_EBP, 0x2468ACE0),
                       (UC_X86_REG_ESI, 0x31415926), (UC_X86_REG_EDI, 0x27182818))
    for register, value in saved_registers:
        u.reg_write(register, value)
    before_loco = bytes(u.mem_read(LOCO, 0x80))
    before_owner = bytes(u.mem_read(OWNER, 0x800))
    family = FAMILIES[row['family']]
    visits, writes = [], []
    observed = {family['entry'], family['early_clear'], family['early_ret'],
                family['type_call'], GET_TYPE, UNIT_TYPE}

    def observe(uc, address, _size, _data):
        if address in observed:
            visits.append(address)
        if address in (GET_TYPE, UNIT_TYPE):
            assert uc.reg_read(UC_X86_REG_ECX) == OWNER

    def observe_write(_uc, _access, address, size, value, _data):
        if LOCO <= address < LOCO + 0x80:
            writes.append(dict(offset=address - LOCO, size=size, value=value))

    u.hook_add(UC_HOOK_CODE, observe)
    u.hook_add(UC_HOOK_MEM_WRITE, observe_write)
    run_checked(u, family['entry'], (family['prefix'], RET_MAGIC), count=100,
                required_addresses=(family['entry'],))
    end = u.reg_read(UC_X86_REG_EIP)
    admitted = end == family['prefix']
    after_loco = bytearray(u.mem_read(LOCO, 0x80))
    residual = read_i32(u, LOCO + 0x4C)
    if admitted:
        assert writes == []
        assert residual == RESIDUAL
    else:
        assert family['early_clear'] in visits and family['early_ret'] in visits
        assert writes == [dict(offset=0x4C, size=4, value=0)]
        assert residual == 0
        assert u.reg_read(UC_X86_REG_EAX) & 0xFF == 0
        assert u.reg_read(UC_X86_REG_ESP) == SP + 8
        assert all(u.reg_read(register) == value for register, value in saved_registers)
        after_loco[0x4C:0x50] = dwords(RESIDUAL)
    assert bytes(after_loco) == before_loco
    assert bytes(u.mem_read(OWNER, 0x800)) == before_owner
    assert bytes(u.mem_read(UNIT_VTABLE, 0x600)) == original_vtable
    assert all(bytes(u.mem_read(a, b - a)) == original
               for (a, b), original in zip(CODE_RANGES, original_code))
    assert visits.count(GET_TYPE) == visits.count(UNIT_TYPE)
    return dict(input=row, prefix_eligible=admitted,
                boundary='before_speed_prefix' if admitted else 'original_return',
                boundary_address=f'{end:08X}', residual_before=RESIDUAL,
                residual_after=residual, returned_al=None if admitted else 0,
                type_getter_calls=visits.count(UNIT_TYPE), loco_writes=writes,
                observed_calls=[f'{address:08X}' for address in visits],
                owner_and_other_loco_fields_unchanged=True,
                original_code_and_vtable_unchanged=True)


def generate():
    return [execute(dict(family=family, track_valid=valid, selector=selector,
                         queue_head=queue, class_flag_62=class_flag, turret=turret))
            for family in FAMILIES
            for valid, selector, queue, class_flag, turret in product(
                (0, 1), (-1, 0, -2), (-1, 2, 8), (0, 1), (0, 1))]


def metadata():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    result = provenance(
        scope='Original Drive/Ship Process_Track entry gate through speed-prefix admission or complete early residual-clear return;144 finite input combinations',
        assumptions=[
            'Supplied disjoint owner/class/type memory and original Unit vtable7F5C70. Original GetType6F3270 jumps through the original+88 slot to741490, which reads supplied owner+6C4; no virtual return stub is used',
            'Each family exhausts72 combinations: valid+63=0/1, signed selector+58=-1/0/-2, Foot queue head+5E0=-1/2/8, class+62=0/1 and Type Turret+CA1=0/1. This is exhaustive only over those listed values, not all machine state or stock-type reachability',
            'Residual+4C is supplied12345678hex, cursor+5C76543210hex, head XYZ null and owner alive+90=1. The boundary does not read head coordinates; selector-2 rows demonstrate a raw !=-1 comparison but do not claim downstream descriptor validity',
            'Entry is a full native-shaped thiscall frame with a supplied unused argument0. Early exits execute the original residual store, epilogue and RET4 to an external stop sentinel; admitted rows stop before4B0F69/6A0639',
            'Read-only observers record actual getter/return visits and locomotor writes. Code and original vtable bytes are checked unchanged, and only residual may change in the supplied owner/class state',
            'Speed target/application, later turning/passive/acceleration gates, tube execution, Process_Movement, caller lifecycle and full Process_Track are excluded. Prefix eligibility is not successful movement',
        ],
        substitutions=[],
        entry_points={'unit_vtable': UNIT_VTABLE, 'get_type': GET_TYPE,
                      'unit_type': UNIT_TYPE,
                      **{f'{name}_{key}': value for name, family in FAMILIES.items()
                         for key, value in family.items()}},
    )
    result['original_code_range_sha256'] = {
        f'{a:08X}..{b:08X}': hashlib.sha256(bytes(u.mem_read(a, b - a))).hexdigest()
        for a, b in CODE_RANGES
    }
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
