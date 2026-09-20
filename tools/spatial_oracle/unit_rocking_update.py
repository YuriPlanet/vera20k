"""Original Techno RockingUpdate, including its real Unit type getters.

All expected bits come from unmodified 70B570..70BCA9 execution. Only facing
and ReceiveDamage callees are supplied boundaries. This is a bounded numeric
and caller-contract corpus, not AI scheduling, impulse or damage-body parity.
"""
from itertools import product
from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
    UC_X86_REG_FPCW,
)
from tools.native_oracle import (
    RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors, load_image,
    provenance, run_checked,
)
from tools.spatial_oracle.map_queries import dwords

ENTRY, END = 0x70B570, 0x70BCAA
UNIT_VTABLE, GET_TYPE, UNIT_TYPE = 0x7F5C70, 0x6F3270, 0x741490
FACING, DAMAGE, RULES_GLOBAL = 0x4C93D0, 0x737C90, 0x8871E0
OWNER, TYPE, RULES, LINK, WARHEAD = (
    SCRATCH + n for n in (0x1000, 0x2000, 0x4000, 0x7000, 0x8000)
)
SP = STACK_BASE + STACK_SIZE - 0x1000
FPCW = 0x0E7F
FIELDS = dict(side_angle=0x328, forward_angle=0x32C,
              side_velocity=0x330, forward_velocity=0x334)
CODE_RANGES = ((ENTRY, END), (GET_TYPE, GET_TYPE + 8),
               (UNIT_TYPE, UNIT_TYPE + 7))
CONSTANTS = {
    0x7EC0B0: (8, '3ef4f8b588e368f1'),
    0x7F4E78: (8, 'bef4f8b588e368f1'),
    0x7E1748: (4, '00000000'),
    0x7E897C: (4, '3fc90fdb'), 0x7E8980: (4, 'bfc90fdb'),
    0x7EF8F8: (4, '3f490fdb'), 0x7F4E74: (4, 'bf490fdb'),
    0x7F4E64: (4, '3ea0d97c'),
    0x7F4E60: (4, '40490fdb'), 0x7F4E5C: (4, 'c0490fdb'),
    0x7F4E70: (4, '3b03126f'), 0x7F4E68: (8, '3f747ae158000000'),
    0x7E3808: (8, '3f847ae147ae147b'),
}
CHECKPOINTS = {
    0x70B5BA: 'sinking', 0x70B619: 'sinking_add',
    0x70B631: 'sinking_subtract', 0x70B649: 'crash',
    0x70B683: 'balloon_clamp', 0x70B700: 'normal',
    0x70B78B: 'side_positive_cap', 0x70B7C1: 'side_negative_cap',
    0x70B7CB: 'side_cap_velocity_zero',
    0x70B921: 'side_zero_velocity', 0x70B957: 'side_crossing_reset',
    0x70BA1D: 'forward_foot_crush_cap',
    0x70BA49: 'forward_positive_cap', 0x70BA85: 'forward_negative_cap',
    0x70BA8B: 'forward_cap_velocity_zero',
    0x70BBE1: 'forward_zero_velocity', 0x70BC17: 'forward_crossing_reset',
    0x70BC23: 'normal_damage_test', 0x70BC9E: 'receive_damage_call',
}
# Every instruction in the original body that stores one of the four owner
# fields. Requiring each at least once prevents accidental fixture narrowing.
OWNER_STORE_SITES = {
    0x70B619, 0x70B631, 0x70B659, 0x70B66B, 0x70B6A4, 0x70B6CB,
    0x70B6E0, 0x70B6F4, 0x70B75F, 0x70B78B, 0x70B7C1, 0x70B7CB,
    0x70B812, 0x70B82D, 0x70B844, 0x70B874, 0x70B8AA, 0x70B8C5,
    0x70B8ED, 0x70B905, 0x70B919, 0x70B921, 0x70B957, 0x70B95D,
    0x70B9F7, 0x70BA49, 0x70BA85, 0x70BA8B, 0x70BAD2, 0x70BAED,
    0x70BB17, 0x70BB32, 0x70BB49, 0x70BB7F, 0x70BBAD, 0x70BBC5,
    0x70BBD9, 0x70BBE1, 0x70BC17, 0x70BC1D,
}


def unsigned(u, address):
    return struct.unpack('<I', u.mem_read(address, 4))[0]


def signed(u, address):
    return struct.unpack('<i', u.mem_read(address, 4))[0]


def field_bits(u):
    return {name + '_bits': f'{unsigned(u, OWNER + offset):08x}'
            for name, offset in FIELDS.items()}


def case(name, **changes):
    row = dict(name=name, side_angle_bits='00000000',
               forward_angle_bits='00000000', side_velocity_bits='00000000',
               forward_velocity_bits='00000000', direct_rocker_link=False,
               fallback_bits='3f800000', abstract_flags=4, crush_flag=0,
               sinking=0, crash=0, balloon_hover=0, facing_raw=0,
               strength=271, damage_return=0)
    row.update(changes)
    return row


def axis_case(name, axis, angle, velocity, **changes):
    return case(name, **{axis + '_angle_bits': angle,
                         axis + '_velocity_bits': velocity}, **changes)


def execute(row):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x10000)
    u.mem_map(RET_MAGIC, 0x1000)
    u.reg_write(UC_X86_REG_FPCW, FPCW)
    original_code = [bytes(u.mem_read(a, b - a)) for a, b in CODE_RANGES]
    original_vtable = bytes(u.mem_read(UNIT_VTABLE, 0x600))
    assert unsigned(u, UNIT_VTABLE + 0x84) == GET_TYPE
    assert unsigned(u, UNIT_VTABLE + 0x88) == UNIT_TYPE
    assert unsigned(u, UNIT_VTABLE + 0x16C) == DAMAGE
    for address, (size, bits) in CONSTANTS.items():
        assert int.from_bytes(u.mem_read(address, size), 'little') == int(bits, 16)
    u.mem_write(OWNER, dwords(UNIT_VTABLE))
    u.mem_write(OWNER + 0x6C4, dwords(TYPE))
    u.mem_write(OWNER + 0x14, dwords(row['abstract_flags']))
    u.mem_write(OWNER + 0x90, b'\x01')
    u.mem_write(OWNER + 0x2A8, dwords(LINK if row['direct_rocker_link'] else 0))
    for name, offset in FIELDS.items():
        u.mem_write(OWNER + offset, dwords(int(row[name + '_bits'], 16)))
    for offset, name in ((0x3CD, 'sinking'), (0x425, 'crash'), (0x6B5, 'crush_flag')):
        u.mem_write(OWNER + offset, bytes((row[name],)))
    u.mem_write(TYPE + 0xA0, dwords(row['strength']))
    u.mem_write(TYPE + 0xD6A, bytes((row['balloon_hover'],)))
    u.mem_write(RULES_GLOBAL, dwords(RULES))
    u.mem_write(RULES + 0x18B8, dwords(int(row['fallback_bits'], 16)))
    u.mem_write(RULES + 0xFA8, dwords(WARHEAD))
    u.mem_write(SP, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ECX, OWNER)
    saved = ((UC_X86_REG_EBX, 0x13579BDF), (UC_X86_REG_EBP, 0x2468ACE0),
             (UC_X86_REG_ESI, 0x31415926), (UC_X86_REG_EDI, 0x27182818))
    for register, value in saved:
        u.reg_write(register, value)
    before_owner = bytearray(u.mem_read(OWNER, 0x800))
    before_type = bytes(u.mem_read(TYPE, 0x1000))
    before_rules = bytes(u.mem_read(RULES, 0x2000))
    visits, checkpoints, events, owner_writes = [], [], [], []

    def return_from_stub(uc, value, expected_return, argument_bytes):
        esp = uc.reg_read(UC_X86_REG_ESP)
        assert unsigned(uc, esp) == expected_return
        uc.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        uc.reg_write(UC_X86_REG_ESP, esp + 4 + argument_bytes)
        uc.reg_write(UC_X86_REG_EIP, expected_return)

    def observe(uc, address, _size, _data):
        # Reject unexpected calls: this function is small enough to enumerate
        # every permitted instruction region rather than accepting silent stubs.
        assert (ENTRY <= address < END or GET_TYPE <= address < GET_TYPE + 8
                or UNIT_TYPE <= address < UNIT_TYPE + 7
                or address in (FACING, DAMAGE, RET_MAGIC)), hex(address)
        visits.append(address)
        if address in CHECKPOINTS:
            checkpoints.append(CHECKPOINTS[address])
        if address in (GET_TYPE, UNIT_TYPE):
            assert uc.reg_read(UC_X86_REG_ECX) == OWNER
            if address == GET_TYPE:
                events.append(dict(kind='get_type', original_getter=True))
        elif address == FACING:
            esp = uc.reg_read(UC_X86_REG_ESP)
            assert uc.reg_read(UC_X86_REG_ECX) == OWNER + 0x388
            out = unsigned(uc, esp + 4)
            assert out == SP - 4
            uc.mem_write(out, dwords(row['facing_raw']))
            events.append(dict(kind='facing', returned_raw=row['facing_raw']))
            return_from_stub(uc, out, 0x70B5F6, 4)
        elif address == DAMAGE:
            esp = uc.reg_read(UC_X86_REG_ESP)
            assert uc.reg_read(UC_X86_REG_ECX) == OWNER
            args = [unsigned(uc, esp + 4 + 4 * n) for n in range(7)]
            assert args == [SP - 4, 0, WARHEAD, 0, 1, 0, 0]
            amount = signed(uc, args[0])
            assert amount == row['strength']
            events.append(dict(kind='receive_damage', amount=amount, distance=0,
                               warhead='supplied_rules_c4', source_object=None,
                               ignore_defenses=True, arg6=False, source_house=None,
                               returned_eax=row['damage_return'],
                               state_at_call=field_bits(uc)))
            # Receiver effects and ObjectAlive changes are deliberately absent.
            return_from_stub(uc, row['damage_return'], 0x70BCA4, 28)

    def observe_write(uc, _access, address, size, value, _data):
        if OWNER <= address < OWNER + 0x800:
            assert address - OWNER in FIELDS.values() and size == 4
            write = dict(offset=address - OWNER, bits=f'{value & 0xFFFFFFFF:08x}',
                         instruction=f'{uc.reg_read(UC_X86_REG_EIP):08X}')
            owner_writes.append(write)
            events.append(dict(kind='owner_write', **write))

    u.hook_add(UC_HOOK_CODE, observe)
    u.hook_add(UC_HOOK_MEM_WRITE, observe_write)
    run_checked(u, ENTRY, RET_MAGIC, count=700, required_addresses=(ENTRY,))
    assert u.reg_read(UC_X86_REG_ESP) == SP + 4
    assert u.reg_read(UC_X86_REG_FPCW) == FPCW
    assert all(u.reg_read(register) == value for register, value in saved)
    assert visits.count(GET_TYPE) == visits.count(UNIT_TYPE)
    after_owner = bytearray(u.mem_read(OWNER, 0x800))
    for offset in FIELDS.values():
        after_owner[offset:offset + 4] = before_owner[offset:offset + 4]
    assert after_owner == before_owner
    assert bytes(u.mem_read(TYPE, 0x1000)) == before_type
    assert bytes(u.mem_read(RULES, 0x2000)) == before_rules
    assert unsigned(u, RULES_GLOBAL) == RULES
    assert bytes(u.mem_read(UNIT_VTABLE, 0x600)) == original_vtable
    assert all(bytes(u.mem_read(a, b - a)) == original
               for (a, b), original in zip(CODE_RANGES, original_code))
    for address, (size, bits) in CONSTANTS.items():
        assert int.from_bytes(u.mem_read(address, size), 'little') == int(bits, 16)
    mode = 'sinking' if row['sinking'] else 'crash' if row['crash'] else 'normal'
    assert ('normal_damage_test' in checkpoints) == (mode == 'normal')
    assert not row['sinking'] or not any(e['kind'] == 'get_type' for e in events)
    return dict(input=row, output=dict(
        **field_bits(u), mode=mode, checkpoints=checkpoints, events=events,
        owner_writes=owner_writes,
        damage_called=any(e['kind'] == 'receive_damage' for e in events),
        original_return=f'{next(a for a in reversed(visits) if ENTRY <= a < END):08X}',
        original_code_vtable_constants_unchanged=True,
        other_owner_type_rules_unchanged=True))


def cases():
    rows = []
    # Inputs are raw binary32 patterns, not a Python model of the result.
    for n, (axis, angle, velocity, linked) in enumerate(product(
            ('side', 'forward'),
            ('00000000', '80000000', '3e800000', 'be800000',
             '3fc90fdb', 'bfc90fdb', '40000000', 'c0000000'),
            ('00000000', '80000000', '3ca3d70a', 'bca3d70a', '3f000000', 'bf000000'),
            (False, True))):
        rows.append(axis_case(f'normal_matrix_{n:03}', axis, angle, velocity,
                              direct_rocker_link=linked))
    # Adjacent stored floats around epsilon, pi/4 and pi/2, both signs.
    for n, (axis, base, sign, adjacent, velocity) in enumerate(product(
            ('side', 'forward'), (0x37A7C5AC, 0x3F490FDB, 0x3FC90FDB),
            (0, 0x80000000), (-1, 0, 1),
            ('00000001', '80000001', '3ca3d70a', 'bca3d70a', '3f800000', 'bf800000'))):
        angle = f'{(base + adjacent) | sign:08x}'
        rows.append(axis_case(f'threshold_{n:03}', axis, angle, velocity))
    numeric = (
        '00000000', '80000000', '00000001', '80000001',
        '007fffff', '807fffff', '00800000', '80800000',
        '3b03126e', '3b03126f', '3b031270', 'bb03126e', 'bb03126f', 'bb031270',
        '3ca3d709', '3ca3d70a', '3ca3d70b', 'bca3d709', 'bca3d70a', 'bca3d70b',
        '3f800001', 'bf800001', '7f7fffff', 'ff7fffff',
    )
    for n, (axis, angle, velocity) in enumerate(product(
            ('side', 'forward'), ('00000000', '3e800000', 'be800000'), numeric)):
        rows.append(axis_case(f'numeric_{n:03}', axis, angle, velocity))
    for n, (axis, fallback, angle, velocity) in enumerate(product(
            ('side', 'forward'),
            ('00000000', '80000000', '00000001', '007fffff',
             '3dcccccd', '3f800000', 'bf800000', '7f7fffff'),
            ('3e800000', 'be800000'), ('3ca3d70a', 'bca3d70a'))):
        rows.append(axis_case(f'linked_fallback_{n:03}', axis, angle, velocity,
                              direct_rocker_link=True, fallback_bits=fallback))
    for n, (flags, crush, linked, sign) in enumerate(product(
            (0, 1, 4, 7), (0, 1, 255), (False, True), (0, 0x80000000))):
        rows.append(axis_case(f'forward_cap_identity_{n:03}', 'forward',
                              f'{0x3E99999A | sign:08x}', f'{0x3CA3D70A | sign:08x}',
                              abstract_flags=flags, crush_flag=crush,
                              direct_rocker_link=linked))
    for axis, sign in product(('side', 'forward'), (0, 0x80000000)):
        rows.append(axis_case(f'unrounded_cap_{axis}_{sign:08x}', axis,
                              f'{0x3F490FDA | sign:08x}', f'{0x33C00000 | sign:08x}'))
        # A cross through zero larger than the epsilon band must still reset.
        rows.append(axis_case(f'large_crossing_{axis}_{sign:08x}', axis,
                              f'{0x3E800000 | sign:08x}', f'{0x3F800000 | (sign ^ 0x80000000):08x}'))
    for n, (axis, sign, adjacent, strength, returned) in enumerate(product(
            ('side', 'forward'), (0, 0x80000000), (-1, 0, 1),
            (17, 271), (0, 4))):
        rows.append(axis_case(f'damage_threshold_{n:03}', axis,
                              f'{(0x40490FDB + adjacent) | sign:08x}',
                              f'{1 | sign:08x}', strength=strength, damage_return=returned))
    for n, (balloon, angle, velocity) in enumerate(product(
            (0, 1, 255),
            ('00000000', '3f490fda', '3f490fdb', '3f490fdc',
             'bf490fda', 'bf490fdb', 'bf490fdc', '40800000', 'c0800000'),
            ('00000000', '00000001', '80000001', '3ca3d70a', 'bca3d70a'))):
        rows.append(case(f'crash_{n:03}', crash=1, balloon_hover=balloon,
                         side_angle_bits=angle, forward_angle_bits=angle,
                         side_velocity_bits=velocity, forward_velocity_bits=velocity))
    for n, (facing, angle) in enumerate(product(
            (0, 0x0FFF, 0x1000, 0x1FFF, 0x2000, 0x3000, 0x4000, 0x6000,
             0x8000, 0xA000, 0xB000, 0xC000, 0xD000, 0xE000, 0xEFFF, 0xF000, 0xFFFF),
            ('00000000', '80000000', '00000001', '80000001',
             '3f490fda', '3f490fdb', '3f490fdc', 'bf490fda', 'bf490fdb', 'bf490fdc'))):
        rows.append(case(f'sinking_{n:03}', sinking=1, crash=1, balloon_hover=1,
                         facing_raw=facing, forward_angle_bits=angle,
                         side_angle_bits='40800000', side_velocity_bits='bdcccccd',
                         forward_velocity_bits='3f800000'))
    rows += [case('both_axes_opposed', side_angle_bits='3e800000',
                  forward_angle_bits='be800000', side_velocity_bits='3ca3d70a',
                  forward_velocity_bits='3ca3d70a'),
             case('both_axes_linked', side_angle_bits='be800000',
                  forward_angle_bits='3e800000', side_velocity_bits='bca3d70a',
                  forward_velocity_bits='bca3d70a', direct_rocker_link=True,
                  fallback_bits='3dcccccd'),
             case('both_axes_subnormal', side_angle_bits='00000001',
                  forward_angle_bits='80000001', side_velocity_bits='00000001',
                  forward_velocity_bits='80000001')]
    return rows


def generate():
    result = [execute(row) for row in cases()]
    assert len({row['input']['name'] for row in result}) == len(result)
    observed = {point for row in result for point in row['output']['checkpoints']}
    assert observed == set(CHECKPOINTS.values()), set(CHECKPOINTS.values()) - observed
    observed_stores = {int(write['instruction'], 16) for row in result
                       for write in row['output']['owner_writes']}
    assert observed_stores == OWNER_STORE_SITES, OWNER_STORE_SITES - observed_stores
    # These are execution-coverage checks, not computed expected numeric values.
    for axis in ('side', 'forward'):
        assert any(row['output'][axis + '_velocity_bits'] != '00000000'
                   and axis + '_cap_velocity_zero' in row['output']['checkpoints']
                   for row in result)
        for row in result:
            if axis + '_crossing_reset' in row['output']['checkpoints']:
                assert row['output'][axis + '_angle_bits'] == '00000000'
                assert row['output'][axis + '_velocity_bits'] == '00000000'
    assert any(row['output']['damage_called'] for row in result)
    assert all(not row['output']['damage_called'] for row in result
               if row['output']['mode'] != 'normal')
    return result


def metadata():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    result = provenance(
        scope=f'{len(cases())} complete original Techno RockingUpdate70B570 executions with real Unit GetType/UnitType; native binary32 retained state, ordinary/special branches, write ordering and self-damage call arguments',
        assumptions=[
            'Pinned original70B570..70BCA9, original Unit vtable7F5C70, getters6F3270/741490 and all13 numeric constants execute/read unchanged. Each row checks their integrity plus preservation of unrelated owner/type/rules fields, callee-saved registers and thiscall stack balance',
            'Each row starts with a fresh supplied Techno/Unit-shaped owner, live Type Strength/BalloonHover, Rules FallBackCoefficient/C4Warhead and direct-rocker link nullness. Type getters execute original instructions; only the link predicate is exercised, not peer lifetime or reciprocal linkage',
            'x87 FPCW0E7F supplies the normal startup 53-bit/chop contract. Raw float32 input/output bits and original write instructions are retained. Expected values are generated only by original execution, with no Python arithmetic implementation or current Rust result involved',
            'Normal cases cross both axes, signed angle/velocity/zero, linked/unlinked state, pre-angle range gates and eight fallback patterns. Adjacent binary32 values cover epsilon, pi/4, pi/2 and strict pi damage thresholds; exact f64 epsilon equality cannot be represented in retained f32 state',
            'Dedicated rows distinguish the sideways unrounded sum cap from the forward stored/reloaded sum cap, cap zero stores followed by damping reloads, large sign crossings, and Foot flag&4 plus truthy crush flag. Output checkpoints are observed native instructions, not predicted branches',
            'Numeric cases include signed subnormals, minimum normals, adjacent damping/overlay-step floats, maximum finite velocities/fallback and both axes active. Finite input is the covered domain; NaN/infinity input, arithmetic fault delivery and all possible bit patterns are excluded',
            'Crash rows cover BalloonHover0/1/255, both axes integration and asymmetric clamps including values beyond normal death threshold. Sinking rows cover eight facing sectors and rounding boundaries, signed/tiny angles and limit equality/adjacency. Sinking is simultaneously supplied with crash to check native precedence',
            'Normal threshold rows vary live Type Strength17/271 and stub return0/4. The original caller copies Strength and calls the original Unit ReceiveDamage slot with distance0, Rules C4Warhead, no source object/house, ignore_defenses1 and arg6false. Damage body, lifecycle effects and the subsequent Techno AI ObjectAlive guard are excluded',
            'Full AI scheduling/eligibility gate6F9E10, loaded voxel state, all angle/velocity/flag/link producers, save/hash, render transforms/cache, actual facing evolution, damage lifecycle and Rust integration are excluded. Samples establish only the supplied states; no whole-mechanism or retail-reachability certification is claimed',
        ],
        substitutions=[
            'Facing4C93D0: assert ECX=owner+388 and original output buffer/return address, write supplied raw facing DWORD, return buffer with RET4 semantics. Original sector rounding and sinking arithmetic execute; facing interpolation/timer receiver is excluded',
            'ReceiveDamage737C90: reached through untouched Unit vtable+16C; assert original return and seven arguments, record live copied Strength and pre-call rocking state, return supplied EAX with RET1C semantics. No damage, owner writes, callbacks or ObjectAlive changes are supplied',
        ],
        entry_points=dict(entry=ENTRY, original_last_ret=END - 1,
                          unit_vtable=UNIT_VTABLE, get_type=GET_TYPE,
                          unit_type=UNIT_TYPE, facing=FACING, receive_damage=DAMAGE,
                          damage_call=0x70BC9E),
    )
    result['x87_control_word'] = f'{FPCW:04X}'
    result['required_owner_store_sites'] = [f'{address:08X}' for address in sorted(OWNER_STORE_SITES)]
    result['numeric_constants'] = {
        f'{address:08X}': dict(size=size, bits=bits)
        for address, (size, bits) in CONSTANTS.items()}
    result['original_code_range_sha256'] = {
        f'{a:08X}..{b:08X}': hashlib.sha256(bytes(u.mem_read(a, b - a))).hexdigest()
        for a, b in CODE_RANGES}
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
