"""Original Techno6FDB80 estimated damage, including native warhead489180.

Every numeric input is recorded as a raw bit pattern or signed integer. Original
type/rank/house-category/ftol/warhead bodies execute; no Python damage model or
arithmetic callback produces outputs. Direct warhead rows are a separate scope.

    python -m tools.spatial_oracle.estimated_damage --check
"""
from itertools import product
from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI,
    UC_X86_REG_ESP, UC_X86_REG_FPCW, UC_X86_REG_FPSW,
)
from tools.native_oracle import RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors, load_image, provenance, run_checked
from tools.spatial_oracle.map_queries import dwords

ENTRY, KERNEL, HOUSE_CATEGORY, FTOL = 0x6FDB80, 0x489180, 0x50BD30, 0x7C5F00
FPCW, CACHED_FPCW = 0x0E7F, 0x822D80
ATTACKER, TARGET, ATTACKER_TYPE, TARGET_TYPE = (SCRATCH + x for x in (0x1000, 0x2000, 0x3000, 0x4000))
ATTACKER_HOUSE, TARGET_HOUSE, ATTACKER_COUNTRY, TARGET_COUNTRY = (SCRATCH + x for x in (0x5000, 0x6000, 0x7000, 0x8000))
WEAPON, WARHEAD, RULES, SCENARIO = (SCRATCH + x for x in (0x9000, 0xA000, 0xC000, 0xF000))
OTHER_TYPE_VTABLE = SCRATCH + 0x11000
SP = STACK_BASE + STACK_SIZE - 0x1000
CLASSES = {
    'unit': dict(object_vtable=0x7F5C70, type_vtable=0x7F6218, type_offset=0x6C4,
                 getter=0x741490, rtti=0x748170, type_rtti=40),
    'infantry': dict(object_vtable=0x7EB058, type_vtable=0x7EB610, type_offset=0x6C0,
                     getter=0x51FAF0, rtti=0x524D40, type_rtti=16),
    'aircraft': dict(object_vtable=0x7E22A4, type_vtable=0x7E2868, type_offset=0x6C4,
                     getter=0x41C200, rtti=0x41CFB0, type_rtti=3),
    'building': dict(object_vtable=0x7E3EBC, type_vtable=0x7E4570, type_offset=0x520,
                     getter=0x459EE0, rtti=0x465D90, type_rtti=7),
}
CODE_RANGES = ((ENTRY, 0x6FDD42), (KERNEL, 0x48926D), (HOUSE_CATEGORY, 0x50BDEA),
               (0x74FF90, 0x74FFB7), (0x750010, 0x750028), (FTOL, 0x7C5F3D),
               (0x6F3270, 0x6F3278), (0x746E20, 0x746E26),
               *[(c['getter'], c['getter'] + 7) for c in CLASSES.values()],
               *[(c['rtti'], c['rtti'] + 6) for c in CLASSES.values()])
FTOL_RETURNS = {
    0x6FDBE2: 'house_times_actor_firepower_times_raw_damage',
    0x6FDC56: 'attacker_veteran_combat',
    0x6FDC87: 'divide_by_target_country_times_attacker_armor',
    0x6FDCFE: 'target_veteran_armor',
    0x4891E9: 'warhead_spread_times_256',
    0x489225: 'warhead_distance_interpolation',
    0x489249: 'warhead_verse',
}
SAVED = ((UC_X86_REG_EBX, 0x12345678), (UC_X86_REG_EBP, 0x23456789),
         (UC_X86_REG_ESI, 0x3456789A), (UC_X86_REG_EDI, 0x456789AB))


def bits64(value):
    return struct.pack('>d', value).hex()


def bits32(value):
    return struct.pack('>f', value).hex()


def raw(bits):
    return int(bits, 16).to_bytes(len(bits) // 2, 'little')


def signed32(value):
    return struct.unpack('<i', dwords(value))[0]


def u32(u, address):
    return int.from_bytes(u.mem_read(address, 4), 'little')


def base(name, **changes):
    # Every live factor is explicit in each saved row. Distinct opposite-owner
    # values make the estimator's unusual ownership observable, not default1.
    row = dict(name=name, scope='estimator', attacker_class='unit', target_class='infantry',
        attacker_type_rtti_override=None, attacker_build_category=0,
        target_present=True, weapon_present=True, weapon_damage=101,
        weapon_flag_130=0, weapon_flag_129=0, warhead_present=True,
        attacker_house_firepower=bits64(1.25), target_house_firepower=bits64(17),
        attacker_firepower=bits64(1.5), target_firepower=bits64(19),
        attacker_armor=bits64(0.75), target_armor=bits64(23),
        attacker_rank=bits32(0), target_rank=bits32(0),
        attacker_veteran_combat=0, attacker_elite_combat=0,
        target_veteran_armor=0, target_elite_armor=0,
        rules_veteran_combat=bits64(1.6), rules_veteran_armor=bits64(1.3),
        attacker_country_armor=[bits32(x) for x in (4, 5, 6, 7, 8)],
        target_country_armor=[bits32(x) for x in (0.8, 1.2, 1.4, 1.6, 1.8)],
        target_armor_index=2, warhead_verses=[bits64(x) for x in (0.25, 0.5, 1, 1.5, 2, 0, -1, 0.75, 1.25, 1.75, 3)],
        warhead_cell_spread=bits32(1), warhead_percent_at_max=bits32(0.5),
        rules_max_damage=10000, scenario_flags=0, kernel_damage=101, kernel_distance=0)
    row.update(changes)
    return row


def unit_factors(name, **changes):
    row = base(name, attacker_house_firepower=bits64(1), attacker_firepower=bits64(1),
               attacker_armor=bits64(1), target_country_armor=[bits32(1)] * 5,
               warhead_verses=[bits64(1)] * 11, warhead_cell_spread=bits32(0),
               warhead_percent_at_max=bits32(1), rules_max_damage=2147483647)
    row.update(changes)
    return row


def execute(row):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x20000)
    u.mem_map(RET_MAGIC, 0x1000)
    original_code = [bytes(u.mem_read(a, b-a)) for a, b in CODE_RANGES]
    tables = {address: bytes(u.mem_read(address, 0x500))
              for c in CLASSES.values() for address in (c['object_vtable'], c['type_vtable'])}
    reads, callbacks, conversions, stores, pending_ftol = [], [], [], [], []
    fields = {}

    def put(name, address, data):
        u.mem_write(address, data)
        fields[(address, len(data))] = name

    for who, obj, type_obj, house, country in (
            ('attacker', ATTACKER, ATTACKER_TYPE, ATTACKER_HOUSE, ATTACKER_COUNTRY),
            ('target', TARGET, TARGET_TYPE, TARGET_HOUSE, TARGET_COUNTRY)):
        cls = CLASSES[row[f'{who}_class']]
        assert u32(u, cls['object_vtable'] + 0x84) == 0x6F3270
        assert u32(u, cls['object_vtable'] + 0x88) == cls['getter']
        assert u32(u, cls['type_vtable'] + 0x2C) == cls['rtti']
        put(f'{who}_object_vtable', obj, dwords(cls['object_vtable']))
        put(f'{who}_type', obj + cls['type_offset'], dwords(type_obj))
        put(f'{who}_type_vtable', type_obj, dwords(cls['type_vtable']))
        put(f'{who}_house', obj + 0x21C, dwords(house))
        put(f'{who}_country', house + 0x34, dwords(country))
        put(f'{who}_house_firepower', house + 0x188, raw(row[f'{who}_house_firepower']))
        put(f'{who}_firepower', obj + 0x160, raw(row[f'{who}_firepower']))
        put(f'{who}_armor', obj + 0x158, raw(row[f'{who}_armor']))
        put(f'{who}_rank', obj + 0x150, raw(row[f'{who}_rank']))
        for index, bits in enumerate(row[f'{who}_country_armor']):
            put(f'{who}_country_armor_{0x100 + index*4:03X}', country + 0x100 + index*4, raw(bits))
    if row['attacker_type_rtti_override'] is not None:
        assert row['attacker_type_rtti_override'] == 1
        u.mem_write(OTHER_TYPE_VTABLE, tables[CLASSES['unit']['type_vtable']])
        # Original Unit object's literal RTTI1 leaf, deliberately routed by a
        # supplied type table only to exercise50BD30's default category branch.
        u.mem_write(OTHER_TYPE_VTABLE + 0x2C, dwords(0x746E20))
        put('attacker_type_vtable', ATTACKER_TYPE, dwords(OTHER_TYPE_VTABLE))
    put('attacker_build_category', ATTACKER_TYPE + 0xE08, dwords(row['attacker_build_category']))
    for name, address in (('attacker_veteran_combat', ATTACKER_TYPE + 0x29E),
                          ('attacker_elite_combat', ATTACKER_TYPE + 0x2B0),
                          ('target_veteran_armor', TARGET_TYPE + 0x29D),
                          ('target_elite_armor', TARGET_TYPE + 0x2AF),
                          ('weapon_flag_130', WEAPON + 0x130), ('weapon_flag_129', WEAPON + 0x129)):
        put(name, address, bytes((row[name],)))
    put('weapon_damage', WEAPON + 0xA4, dwords(row['weapon_damage']))
    put('weapon_warhead', WEAPON + 0xAC, dwords(WARHEAD if row['warhead_present'] else 0))
    put('target_armor_index', TARGET_TYPE + 0x9C, dwords(row['target_armor_index']))
    for i, bits in enumerate(row['warhead_verses']):
        put(f'warhead_verse_{i}', WARHEAD + 0xA0 + 8*i, raw(bits))
    put('warhead_cell_spread', WARHEAD + 0x124, raw(row['warhead_cell_spread']))
    put('warhead_percent_at_max', WARHEAD + 0x12C, raw(row['warhead_percent_at_max']))
    put('rules_veteran_combat', RULES + 0x670, raw(row['rules_veteran_combat']))
    put('rules_veteran_armor', RULES + 0x688, raw(row['rules_veteran_armor']))
    put('rules_max_damage', RULES + 0x16C8, dwords(row['rules_max_damage']))
    put('scenario_flags', SCENARIO, bytes((row['scenario_flags'],)))
    u.mem_write(0x8871E0, dwords(RULES))
    u.mem_write(0xA8B230, dwords(SCENARIO))
    assert u32(u, CACHED_FPCW) == FPCW
    u.reg_write(UC_X86_REG_FPCW, FPCW)
    if row['scope'] == 'estimator':
        entry = ENTRY
        u.mem_write(SP, dwords(RET_MAGIC, TARGET if row['target_present'] else 0,
                              WEAPON if row['weapon_present'] else 0))
        u.reg_write(UC_X86_REG_ECX, ATTACKER)
    else:
        assert row['scope'] == 'direct_warhead'
        entry = KERNEL
        u.mem_write(SP, dwords(RET_MAGIC, row['target_armor_index'], row['kernel_distance']))
        u.reg_write(UC_X86_REG_ECX, row['kernel_damage'] & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_EDX, WARHEAD if row['warhead_present'] else 0)
    u.reg_write(UC_X86_REG_ESP, SP)
    for register, value in SAVED:
        u.reg_write(register, value)
    before = bytes(u.mem_read(SCRATCH, 0x20000))
    observed = {ENTRY: 'estimate', KERNEL: 'warhead', HOUSE_CATEGORY: 'house_category',
                0x74FF90: 'is_veteran', 0x750010: 'is_elite', 0x6F3270: 'get_type',
                0x746E20: 'supplied_type_rtti1_original_leaf',
                **{c['getter']: f'{name}_type_getter' for name, c in CLASSES.items()},
                **{c['rtti']: f'{name}_type_rtti' for name, c in CLASSES.items()}}

    def observe(uc, address, _size, _data):
        if pending_ftol and address == pending_ftol[0]:
            pending_ftol.pop()
            eax, edx = uc.reg_read(UC_X86_REG_EAX), uc.reg_read(UC_X86_REG_EDX)
            wide = (edx << 32) | eax
            conversions.append(dict(return_address=f'{address:08X}', stage=FTOL_RETURNS[address],
                                    result_i64=wide - (1 << 64) if wide >> 63 else wide,
                                    low_i32=signed32(eax), eax=f'{eax:08X}', edx=f'{edx:08X}'))
        if address == FTOL:
            assert not pending_ftol
            destination = u32(uc, uc.reg_read(UC_X86_REG_ESP))
            assert destination in FTOL_RETURNS
            pending_ftol.append(destination)
        if address not in observed:
            return
        event = dict(name=observed[address], address=f'{address:08X}', ecx=uc.reg_read(UC_X86_REG_ECX))
        sp = uc.reg_read(UC_X86_REG_ESP)
        if address == HOUSE_CATEGORY:
            event['type_argument'] = u32(uc, sp + 4)
            assert event['ecx'] == TARGET_HOUSE and event['type_argument'] == ATTACKER_TYPE
        elif address == KERNEL:
            event.update(damage=signed32(event['ecx']), warhead=uc.reg_read(UC_X86_REG_EDX),
                         armor=signed32(u32(uc, sp + 4)), distance=signed32(u32(uc, sp + 8)))
            if row['scope'] == 'estimator':
                assert event['damage'] >= 1 and event['distance'] == 0
        callbacks.append(event)

    def observe_read(_uc, _access, address, size, _value, _data):
        if (address, size) in fields:
            reads.append(fields[(address, size)])

    def observe_write(uc, _access, address, size, value, _data):
        assert STACK_BASE <= address < STACK_BASE + STACK_SIZE, (hex(address), size)
        pc = uc.reg_read(UC_X86_REG_EIP)
        if pc in (0x6FDC62, 0x4891CA, 0x4891D4):
            stores.append(dict(instruction=f'{pc:08X}', size=size,
                               bits=f'{value & ((1 << (8*size))-1):0{2*size}x}'))

    u.hook_add(UC_HOOK_CODE, observe)
    u.hook_add(UC_HOOK_MEM_READ, observe_read)
    u.hook_add(UC_HOOK_MEM_WRITE, observe_write)
    run_checked(u, entry, RET_MAGIC, count=3000, required_addresses=(entry,))
    assert not pending_ftol
    assert u.reg_read(UC_X86_REG_ESP) == SP + 12
    assert all(u.reg_read(register) == value for register, value in SAVED)
    assert bytes(u.mem_read(SCRATCH, 0x20000)) == before
    assert u.reg_read(UC_X86_REG_FPCW) == FPCW
    assert (u.reg_read(UC_X86_REG_FPSW) >> 11) & 7 == 0
    assert all(bytes(u.mem_read(a, b-a)) == original
               for (a, b), original in zip(CODE_RANGES, original_code))
    assert all(bytes(u.mem_read(address, len(data))) == data for address, data in tables.items())
    eax = u.reg_read(UC_X86_REG_EAX)
    return dict(input=row, result_i32=signed32(eax), result_eax=f'{eax:08X}',
                callbacks=callbacks, ftol_conversions=conversions, floating_stores=stores,
                factor_reads=reads, final_fpcw=f'{u.reg_read(UC_X86_REG_FPCW):04X}',
                x87_exception_status=u.reg_read(UC_X86_REG_FPSW) & 0x3F,
                input_state_preserved=True, original_code_and_tables_unchanged=True)


def cases():
    rows = [base('asymmetric_base')]
    for damage, flags in product((-2147483648, -7, 0, 1, 101, 2147483647), ((0, 0), (0, 1), (1, 0), (1, 1))):
        rows.append(base(f'gate_damage{damage}_flags{flags[0]}{flags[1]}',
                         weapon_damage=damage, weapon_flag_130=flags[0], weapon_flag_129=flags[1]))
    rows.extend(base(f'null_target_weapon{present}', target_present=False, weapon_present=present)
                for present in (False, True))
    for damage, wh, flags in product((-7, 0, 101), (False, True), (0, 0x20)):
        rows.append(base(f'warhead_gate_{damage}_{wh}_{flags}', weapon_damage=damage,
                         warhead_present=wh, scenario_flags=flags))
    for attacker, target in product(CLASSES, CLASSES):
        for buildcat in ((0, 5) if attacker == 'building' else (0,)):
            rows.append(base(f'category_{attacker}_{target}_{buildcat}', attacker_class=attacker,
                             target_class=target, attacker_build_category=buildcat))
    rows.append(base('category_default_rtti_boundary', attacker_type_rtti_override=1))
    for armor in range(11):
        rows.append(base(f'armor_verse_{armor}', target_armor_index=armor))
    rank_bits = ('be800000', '00000000', '00000001', '3f7fffff', '3f800000',
                 '3fffffff', '40000000', '40000001')
    for who, rank, abilities in product(('attacker', 'target'), rank_bits, ((0, 0), (1, 0), (0, 1), (1, 1))):
        suffix = 'combat' if who == 'attacker' else 'armor'
        rows.append(base(f'rank_{who}_{rank}_{abilities[0]}{abilities[1]}', **{
            f'{who}_rank': rank, f'{who}_veteran_{suffix}': abilities[0],
            f'{who}_elite_{suffix}': abilities[1]}))
    for a, t in product((0, 1, 2), repeat=2):
        rows.append(base(f'both_rank_{a}_{t}', attacker_rank=bits32(a), target_rank=bits32(t),
                         attacker_veteran_combat=1, attacker_elite_combat=0,
                         target_veteran_armor=0, target_elite_armor=1))
    finite64 = ('0000000000000000', '8000000000000000', '0000000000000001',
                '8000000000000001', '000fffffffffffff', '0010000000000000',
                '3fe0000000000000', '3fefffffffffffff', '3ff0000000000000',
                '3ff0000000000001', 'bff0000000000000', '41dfffffffc00000',
                '41e0000000000000', '41f0000000000000', '43dfffffffffffff',
                '43e0000000000000', '7fefffffffffffff')
    for field, bits in product(('attacker_house_firepower', 'attacker_firepower', 'attacker_armor',
                                'rules_veteran_combat', 'rules_veteran_armor'), finite64):
        rows.append(unit_factors(f'finite64_{field}_{bits}', **{field: bits,
            'attacker_rank': bits32(2), 'target_rank': bits32(2),
            'attacker_veteran_combat': 1, 'target_veteran_armor': 1,
            **({} if field.startswith('rules_') else {
                'rules_veteran_combat': bits64(1), 'rules_veteran_armor': bits64(1)})}))
    for bits in finite64:
        verses = [bits64(1)] * 11
        verses[2] = bits
        rows.append(unit_factors(f'finite64_verse_{bits}', warhead_verses=verses))
    finite32 = ('00000000', '80000000', '00000001', '80000001', '007fffff', '00800000',
                '3effffff', '3f000000', '3f000001', '3f7fffff', '3f800000', 'bf800000', '7f7fffff')
    for cls, index, buildcat in (('infantry', 0, 0), ('unit', 1, 0), ('aircraft', 2, 0),
                                  ('building', 3, 0), ('building', 4, 5)):
        for bits in finite32:
            factors = [bits32(1)] * 5
            factors[index] = bits
            rows.append(unit_factors(f'country_{cls}_{buildcat}_{bits}', attacker_class=cls,
                                     attacker_build_category=buildcat, target_country_armor=factors))
    for field, bits in product(('warhead_cell_spread', 'warhead_percent_at_max'), finite32):
        rows.append(unit_factors(f'warhead_{field}_{bits}', **{
            'warhead_cell_spread': bits32(1), 'warhead_percent_at_max': bits32(0.25), field: bits}))
    for damage, spread, percent in product((1, 3, 99, 16777217, 2147483647), (0, 1), (0, 0.1, 1)):
        rows.append(unit_factors(f'float_store_boundary_{damage}_{spread}_{percent}',
            weapon_damage=damage, warhead_cell_spread=bits32(spread), warhead_percent_at_max=bits32(percent)))
    for limit in (-1, 0, 1, 50, 2147483647):
        rows.append(base(f'maxdamage_{limit}', rules_max_damage=limit))
    # Finite staged-rounding cases and ownership asymmetry controls.
    for raw_damage, fire, combat, armor, veteran_armor in (
            (3, 0.6, 1.9, 0.6, 1.9), (99, 1.01, 1.1, 0.99, 1.1),
            (101, 0.9999999999999999, 1.0000000000000002, 1.5, 0.75),
            (2147483647, 2, 1, 1, 1), (2147483647, 4, 1, 1, 1)):
        rows.append(unit_factors(f'staged_{raw_damage}_{fire}_{combat}_{armor}_{veteran_armor}',
            weapon_damage=raw_damage, attacker_firepower=bits64(fire),
            attacker_rank=bits32(2), target_rank=bits32(2),
            attacker_veteran_combat=1, target_veteran_armor=1,
            rules_veteran_combat=bits64(combat), attacker_armor=bits64(armor),
            rules_veteran_armor=bits64(veteran_armor)))
    for field in ('target_house_firepower', 'target_firepower', 'target_armor'):
        rows.append(base(f'unused_asymmetric_{field}', **{field: bits64(0.125)}))
    rows.append(base('unused_attacker_country', attacker_country_armor=[bits32(0.125)] * 5))
    for damage, distance, armor in product((-7, 0, 7), (-1, 0, 7, 8), (0, 8)):
        rows.append(unit_factors(f'direct_kernel_{damage}_{distance}_armor{armor}',
            scope='direct_warhead', kernel_damage=damage, kernel_distance=distance,
            target_armor_index=armor, warhead_cell_spread=bits32(1), warhead_percent_at_max=bits32(0.25)))
    # Frozen405 prefix: estimator evidence and its24 original direct-kernel rows.
    assert len(rows) == 405
    for damage, percent, distance in product((1, 3, 99, 16777217, 2147483647),
                                           (0, 0.1, 0.25, 1, 1.5),
                                           (-257, 1, 127, 255, 256, 257, 512)):
        rows.append(unit_factors(f'kernel_extension_falloff_{damage}_{percent}_{distance}',
            scope='direct_warhead', kernel_damage=damage, kernel_distance=distance,
            warhead_cell_spread=bits32(1), warhead_percent_at_max=bits32(percent)))
    for bits, distance in product(finite32, (-2147483648, -1, 0, 257, 2147483647)):
        rows.append(unit_factors(f'kernel_extension_spread_{bits}_{distance}',
            scope='direct_warhead', kernel_damage=99, kernel_distance=distance,
            warhead_cell_spread=bits, warhead_percent_at_max=bits32(0.1)))
    for bits, distance in product((*finite32, 'ff7fffff'), (-1, 128, 512)):
        rows.append(unit_factors(f'kernel_extension_percent_{bits}_{distance}',
            scope='direct_warhead', kernel_damage=2147483647, kernel_distance=distance,
            warhead_cell_spread=bits32(1), warhead_percent_at_max=bits))
    for bits, damage in product(finite64, (1, 99, 2147483647)):
        verses = [bits64(1)] * 11
        verses[2] = bits
        rows.append(unit_factors(f'kernel_extension_verse_{bits}_{damage}',
            scope='direct_warhead', kernel_damage=damage, kernel_distance=128,
            warhead_cell_spread=bits32(1), warhead_percent_at_max=bits32(0.25),
            warhead_verses=verses))
    # Integer SUB wraps before FIMUL; low32 spread may itself be negative.
    for spread, distance in product((bits32(-1), bits32(8388607.5), bits32(8388608), bits32(16777216)),
                                   (-2147483648, -2147483647, 2147483647)):
        rows.append(unit_factors(f'kernel_extension_wrapping_{spread}_{distance}',
            scope='direct_warhead', kernel_damage=101, kernel_distance=distance,
            warhead_cell_spread=spread, warhead_percent_at_max=bits32(0.5)))
    for limit, damage, wh, no_damage in product((-1, 0, 1, 50, 100, 101, 2147483647),
                                               (-2147483648, -7, 0, 100), (False, True), (False, True)):
        rows.append(unit_factors(f'kernel_extension_gate_{limit}_{damage}_{wh}_{no_damage}',
            scope='direct_warhead', kernel_damage=damage, kernel_distance=0,
            warhead_present=wh, rules_max_damage=limit, scenario_flags=0x20 if no_damage else 0))
    # Frozen862 finite prefix: preserve the reviewed estimator and kernel rows.
    assert len(rows) == 862
    exceptional32 = ('7f800000', 'ff800000', '7fc12345', 'ffc23456',
                     '7f812345', 'ff823456')
    exceptional64 = ('7ff0000000000000', 'fff0000000000000',
                     '7ff8123456789abc', 'fff823456789abcd',
                     '7ff0123456789abc', 'fff023456789abcd')
    # PAM +/-infinity is admitted by decimal-overflow get_f32, including the
    # complete WarheadType parser. NaN encodings are raw-domain probes only:
    # the leading-number parser maps the literal NaN string to zero.
    for bits, spread, distance in product(exceptional32, (0, 1, -1), (-1, 0, 128, 256, 512)):
        rows.append(unit_factors(f'kernel_exceptional_percent_{bits}_{spread}_{distance}',
            scope='direct_warhead', kernel_damage=100, kernel_distance=distance,
            warhead_cell_spread=bits32(spread), warhead_percent_at_max=bits))
    # CellSpread get_f32 can overflow, but its separate SimFixed projection is
    # outside this receiver fixture; these rows do not assert parser admission.
    for bits, percent, distance in product(exceptional32,
            (bits32(0), bits32(1), '7f800000', '7fc12345'), (0, 257)):
        rows.append(unit_factors(f'kernel_exceptional_spread_{bits}_{percent}_{distance}',
            scope='direct_warhead', kernel_damage=100, kernel_distance=distance,
            warhead_cell_spread=bits, warhead_percent_at_max=percent))
    for bits, (spread, percent, distance) in product(exceptional64,
            ((0, 1, 0), (1, 0.25, 128), (1, 0, 256), (1, 0, 512))):
        verses = [bits64(1)] * 11
        verses[2] = bits
        rows.append(unit_factors(f'kernel_exceptional_verse_{bits}_{spread}_{percent}_{distance}',
            scope='direct_warhead', kernel_damage=100, kernel_distance=distance,
            warhead_cell_spread=bits32(spread), warhead_percent_at_max=bits32(percent),
            warhead_verses=verses))
    # Admission gates must precede exceptional floating loads/arithmetic.
    for bits, (damage, distance, wh, flags) in product(exceptional32,
            ((-7, 7, True, 0), (-7, 8, True, 0), (0, 0, True, 0),
             (100, 0, True, 0x20), (100, 0, False, 0))):
        rows.append(unit_factors(f'kernel_exceptional_gate_{bits}_{damage}_{distance}_{wh}_{flags}',
            scope='direct_warhead', kernel_damage=damage, kernel_distance=distance,
            warhead_present=wh, scenario_flags=flags,
            warhead_cell_spread=bits, warhead_percent_at_max=bits,
            warhead_verses=[exceptional64[exceptional32.index(bits)]] * 11))
    for bits, limit in product(exceptional64, (-1, 1)):
        rows.append(unit_factors(f'kernel_exceptional_signed_cap_{bits}_{limit}',
            scope='direct_warhead', kernel_damage=100, kernel_distance=0,
            warhead_verses=[bits] * 11, rules_max_damage=limit))
    return rows


def generate():
    rows = [execute(row) for row in cases()]
    assert len({row['input']['name'] for row in rows}) == len(rows)
    assert {item['stage'] for row in rows for item in row['ftol_conversions']} == set(FTOL_RETURNS.values())
    return dict(schema_version=1, row_count=len(rows), rows=rows)


def metadata():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    result = provenance(
        scope='Complete original estimated-damage6FDB80 with nested rank/type/house-category/ftol/warhead bodies; separate direct489180 distance-boundary rows. Supplied live input records, no gameplay producer or full scanner claim',
        assumptions=[
            'Every row records all fixture factors and gates, with raw binary32/binary64 bits: attacker/target object and house multipliers, both country category arrays, rank and ability bytes, Rules combat/armor/MaxDamage, weapon damage/flags, warhead pointer/spread/percent/eleven verses, target armor and Scenario flags. No missing live multiplier is silently modeled as a gameplay default1',
            'Ordinary Unit, Infantry, Aircraft and Building use original object and type vtables/getters/RTTI. Attacker+160 and attackerHouse+188 multiply damage. TargetHouse category query is passed attacker type, then multiplies attacker+158. Contrasting target+158/+160/house+188 and attacker-country values are recorded but unused; these are intentionally different ownership paths from delivered damage',
            'Original74FF90 checks binary32 rank >=1 and <2;750010 checks >=2. Original ability branches use attacker+29E/+2B0 and target+29D/+2AF, including elite inheriting veteran ability. Constructors, parsed ability lists, crate/country/difficulty/veterancy multiplier producers and real house setup are excluded',
            'Original489180 always receives distance0 and positive>=1 damage when reached from6FDB80. Nonpositive weapon damage returns raw from6FDB80 before factors/kernel; null target and weapon bytes130/129 return0. A null weapon is supplied only with null target; no unsupported null-weapon guard is invented',
            'For direct489180 rows, entry ECX is damage, EDX warhead, stack arg1 armor, arg2 distance. After SUB ESP,0Ch and two pushes, ESP+18h is armor (verses at489229), ESP+1Ch is distance (negative gate4891AF and spread489202). Negative damage returns raw only for signed distance<8, otherwise0; this is not an armor restriction or a reachable negative nested-estimator path',
            'x87 ambient and cached control are0E7F (PC53/chop, masked exceptions). The unchanged retail cache822D80 is asserted. Startup hardware captures bounce_startup_capture.json and tube_startup_capture.json record0E7F before WinMain; FOOTCLASS_GET_CURRENT_SPEED_EXACT_GHIDRA_REPORT.md documents later chop request/cache setup. This is pinned ambient execution, not a live6FDB80 CW capture or runtime immutability proof',
            'Original7C5F00 FISTP stores signed64 then returns EDX:EAX; the caller uses only low EAX. Each conversion records the full signed64 and low signedi32, preserving wrap/indefinite behavior. Finite subnormals, signed zero denominators, i32/i64 overflow boundaries and finite inputs causing masked intermediate infinity/invalid conversion are arithmetic-domain probes, not proof such rules are retail-authored or supported by current Rust',
            'Native float stores, callback order, selected factor reads, original return and register/stack/input preservation are observed. Hooks never change registers, memory, execution addresses or arithmetic outcomes. No Python arithmetic generates expected damage',
            'Invalid armor indices, malformed/null positive-path house/type pointers, unmasked traps, full extended-exponent overflow, runtime FP-status hardware parity, weapon selection, scanner/debit scheduling, actual FireAt/ReceiveDamage, persistence and factor producer parity are excluded. Separate direct-kernel rows do not certify the entire warhead function domain',
            'The original405 payload rows and subsequent finite862 rows are unchanged prefixes. Appended direct-kernel cases cross positive nonzero distance before/at/beyond spread, negative-result floor, binary32 spill equality, non-dyadic PercentAtMax, finite signed zero/subnormal/normal/max inputs, signed SUB wrap, low32 ftol wrap, masked signed64 indefinite and signed MaxDamage/gates. These are numeric receiver fixtures, not proof that the area collector generates every supplied distance',
            'Appended exceptional direct489180 rows supply signed infinities and quiet/signaling NaN payloads as raw binary32 PAM/CellSpread and binary64 Verses. PAM decimal +/-1e40 is admitted by the current complete Rust WarheadType parser. Literal NaN parses as0, so raw NaNs are separate value-domain evidence. CellSpread get_f32 overflow has a separate SimFixed projection gate outside this fixture; spread-infinity rows do not establish complete rules-parser admission',
            'FCOMP at4891ED followed by FNSTSW/TEST AH,40h branches on C3: equal and unordered spills both bypass interpolation. A nonfinite spread converts through original ftol to low0. Interpolation can generate invalid infinity cancellation or zero-times-infinity, and the final verse can multiply either zero or positive falloff by infinity/NaN. Original observed low EAX and both f32 store bits are asserted; exception-status hardware parity and generic two-NaN arithmetic selection are not covered',
        ],
        substitutions=[
            'One named category-default boundary row routes a copied type vtable+2C to original literal-RTTI1 leaf746E20; this is a supplied category identity and not a real UnitType RTTI. Other rows retain actual class/type vtables; no arithmetic, rank, house, ftol or warhead callee is substituted',
        ],
        entry_points=dict(estimated_damage=ENTRY, warhead=KERNEL, house_category=HOUSE_CATEGORY,
                          ftol=FTOL, is_veteran=0x74FF90, is_elite=0x750010,
                          **{f'{name}_type_getter': c['getter'] for name, c in CLASSES.items()}))
    result['row_counts'] = {scope: sum(row['scope'] == scope for row in cases())
                            for scope in ('estimator', 'direct_warhead')}
    result['x87_control_word'] = f'{FPCW:04X}'
    result['cached_x87_control_word'] = f'{u32(u, CACHED_FPCW):04X}'
    result['original_code_sha256'] = {f'{a:08X}..{b:08X}': hashlib.sha256(bytes(u.mem_read(a, b-a))).hexdigest()
                                      for a, b in CODE_RANGES}
    result['original_constants'] = {f'{a:08X}': bytes(u.mem_read(a, n)).hex()
                                    for a, n in ((0x7E2224, 4), (0x7E2AC8, 4),
                                                 (0x7E37B4, 4), (CACHED_FPCW, 4))}
    result['original_classes'] = CLASSES
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
