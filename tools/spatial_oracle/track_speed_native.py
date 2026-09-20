"""Original Drive/Ship speed prefixes and complete live Foot speed getter.

Supplied live object/type/house state, original vtables and callees. Prefix rows
stop after the original retry mask and residual addition, before point dispatch.
No hooks change CPU/memory or substitute callees. Flag producers and linked-unit
lifecycles are outside this numeric comparison.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP
from tools.native_oracle import RET_MAGIC, SCRATCH, finish_vectors, provenance, run_checked
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.locomotor_force_track import OriginalForceTrack, FOOT, LOCO, SP

TYPE, HOUSE, HOUSE_TYPE, RULES = (SCRATCH + n for n in (0x3000, 0x5000, 0x6000, 0x8000))


def bits(value):
    return f'{struct.unpack("<Q", struct.pack("<d", value))[0]:016x}'


def signed(value):
    return struct.unpack('<i', struct.pack('<I', value & 0xFFFFFFFF))[0]


def seed(row):
    n = OriginalForceTrack(row)
    u = n.uc
    u.mem_write(TYPE, dwords(0x7F6218))  # actual UnitType vtable / WhatAmI40
    u.mem_write(FOOT + 0x6C4, dwords(TYPE))
    u.mem_write(FOOT + 0x21C, dwords(HOUSE))
    u.mem_write(HOUSE + 0x34, dwords(HOUSE_TYPE))
    u.mem_write(HOUSE_TYPE + 0x12C, struct.pack('<f', row.get('house', 1.0)))
    u.mem_write(0x8871E0, dwords(RULES))
    u.mem_write(RULES + 0x678, struct.pack('<d', row.get('veteran', 1.5)))
    u.mem_write(TYPE + 0x678, dwords(row.get('raw', 17)))
    u.mem_write(TYPE + 0x29C, bytes((int(row.get('faster', False)),)))
    u.mem_write(TYPE + 0x2AE, bytes((int(row.get('elite_faster', False)),)))
    u.mem_write(FOOT + 0x150, struct.pack('<f', row.get('rank', 1.0)))
    u.mem_write(FOOT + 0x580, struct.pack('<d', row.get('crate', 1.0)))
    u.mem_write(FOOT + 0x578, struct.pack('<d', row.get('applied', 0.25)))
    u.mem_write(FOOT + 0x6CC, dwords(row.get('flag_owner', -1)))
    return n


def speed_inputs(n, row):
    return dict(raw=row.get('raw', 17),
                house_bits=f'{struct.unpack("<I", struct.pack("<f", row.get("house", 1.0)))[0]:08x}',
                crate_bits=bits(row.get('crate', 1.0)),
                veteran_bits=bits(row.get('veteran', 1.5)),
                applied_bits=n.double_bits(FOOT + 0x578),
                flag_owner=row.get('flag_owner', -1),
                faster=((row.get('rank', 1.0) >= 1 and row.get('faster', False)) or
                        (row.get('rank', 1.0) >= 2 and row.get('elite_faster', False))))


def getter(row):
    n = seed(row)
    u = n.uc
    initial = bytes(u.mem_read(FOOT, 0x800))
    u.reg_write(UC_X86_REG_ECX, FOOT)
    u.reg_write(UC_X86_REG_ESP, SP)
    u.mem_write(SP, dwords(RET_MAGIC))
    run_checked(u, 0x4DB1A0, RET_MAGIC, count=3000,
                required_addresses=(0x50C050, 0x70EFE0, 0x70D0D0, 0x7C5F00))
    assert u.reg_read(UC_X86_REG_ESP) == SP + 4
    assert bytes(u.mem_read(FOOT, 0x800)) == initial
    return dict(input=speed_inputs(n, row), state=row,
                output=signed(u.reg_read(UC_X86_REG_EAX)))


def prefix(row):
    n = seed(row)
    u = n.uc
    ship = row['family'] == 'ship'
    if ship:
        u.reg_write(UC_X86_REG_ECX, LOCO)
        u.reg_write(UC_X86_REG_ESP, SP)
        u.mem_write(SP, dwords(RET_MAGIC))
        run_checked(u, 0x69EC50, RET_MAGIC, count=150, required_addresses=(0x55A6C0,))
        u.mem_write(LOCO + 0xC, dwords(FOOT))
    u.mem_write(TYPE + 0xDBD, bytes((int(row.get('accelerates', True)),)))
    u.mem_write(TYPE + 0xE0C, bytes((int(row.get('passive', False)),)))
    u.mem_write(TYPE + 0x2F8, dwords(row.get('slowdown', 500)))
    u.mem_write(TYPE + 0x300, struct.pack('<d', row.get('decel', 0.002)))
    u.mem_write(TYPE + 0x308, struct.pack('<d', row.get('accel', 0.03)))
    u.mem_write(FOOT + 0x3CD, bytes((int(row.get('sinking', False)),)))
    u.mem_write(FOOT + 0x6B5, bytes((int(row.get('crush', False)),)))
    u.mem_write(FOOT + 0x5E0, dwords(-1))
    u.mem_write(FOOT + 0x9C, dwords(*row.get('current', [2176, 2176, 208])))
    destination = row.get('destination', [2688, 2176, -731])
    u.mem_write(LOCO + 0x34, dwords(*destination))
    u.mem_write(LOCO + 0x50, struct.pack('<d', row.get('target', 1.0)))
    u.mem_write(LOCO + 0x58, dwords(row.get('selector', 1)))
    u.mem_write(LOCO + 0x63, b'\x01')
    u.mem_write(LOCO + 0x4C, dwords(row.get('residual', 7)))
    for address, value in ((0x8A07D0, 104), (0x8A07C4, 416), (0xB0782C, 416)):
        u.mem_write(address, dwords(value))
    observations = dict(setters=0, getters=0, distance=None, propagate=False)
    def observe(uc, address, size, _):
        if address == 0x4D3710:
            observations['setters'] += 1
        if address == 0x4DB1A0:
            observations['getters'] += 1
        if address == (0x6A0757 if ship else 0x4B1087):
            observations['distance'] = signed(uc.reg_read(UC_X86_REG_ESI if ship else UC_X86_REG_EDI))
        if address == (0x6A08ED if ship else 0x4B1225):
            observations['propagate'] = True
    u.hook_add(UC_HOOK_CODE, observe)
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ECX, LOCO)
    u.mem_write(SP, dwords(RET_MAGIC, int(row.get('retry', False))))
    run_checked(u, 0x6A05F0 if ship else 0x4B0F20,
                0x6A095F if ship else 0x4B1297, count=10000,
                required_addresses=(0x4DB1A0, 0x50C050, 0x7C5F00))
    assert observations['getters'] == 1
    return dict(input=dict(row, target_bits=bits(row.get('target', 1.0)),
                           applied_bits=bits(row.get('applied', 0.25)),
                           accel_bits=bits(row.get('accel', 0.03)),
                           decel_bits=bits(row.get('decel', 0.002)),
                           resolved_destination=[destination[0], destination[1], 208 + (416 if row.get('bridge', False) else 0)]),
                getter=speed_inputs(n, row),
                output=dict(target_bits=n.double_bits(LOCO + 0x50),
                            applied_bits=n.double_bits(FOOT + 0x578),
                            budget=signed(u.reg_read(UC_X86_REG_EDX)), **observations))


def generate():
    getters = []
    for raw in (0, 1, 10, 17, 25, 255, -17):
        for applied in (0.0, 0.03, 0.2, 0.9999999999999999, 1.0, -0.25):
            getters.append(getter(dict(raw=raw, applied=applied)))
    for house, crate in ((1.15, 1.2), (0.75, 1.5), (1.0000001, 0.9999999999999999)):
        for rank, faster, elite in ((0.0, True, False), (1.0, True, False), (2.0, False, True), (2.0, True, False)):
            for flag in (-1, 0):
                getters.append(getter(dict(raw=17, applied=0.75, house=house, crate=crate, rank=rank, faster=faster, elite_faster=elite, flag_owner=flag)))
    for raw in (17, -17, 2147483647):
        getters.append(getter(dict(raw=raw, applied=1.0, crate=3.0, flag_owner=0)))
    cases = []
    base_cases = [dict(selector=s, accelerates=a, passive=p) for s in (-1, 1, 63, 64, 71) for a in (False, True) for p in (False, True)]
    # Selector-1 is admitted by the native path-head8 alternative.
    base_cases = [c for c in base_cases if c['selector'] != -1]
    base_cases += [dict(applied=a, target=t, crush=True, sinking=s, slowdown=d)
                   for a, t in ((0.75, 0.125), (0.125, 0.75), (0.75, 0.75))
                   for s in (False, True) for d in (0, 1000)]
    base_cases += [dict(applied=a, target=t, slowdown=d, sinking=s)
                   for a, t in ((0.0, 1.0), (0.75, 0.5), (0.5, 0.5))
                   for d in (511, 512, 513) for s in (False, True)]
    base_cases += [dict(current=[2176, 2176, z], bridge=b, faster=True, crate=1.2,
                       retry=r, residual=-3, applied=0.75)
                   for z in (-731, 208, 624) for b in (False, True) for r in (False, True)]
    for family in ('drive', 'ship'):
        cases.extend(prefix(dict(case, family=family)) for case in base_cases)
    return dict(getters=getters, prefixes=cases)


def metadata():
    return provenance(scope='Complete live Foot getter and original Drive/Ship speed prefixes through retry mask/residual addition',
        assumptions=['Supplied Unit/UnitType/House memory with original vtables; house binary32, crate/rules/applied binary64 and FASTER arrays/rank are explicit inputs',
                     'Startup x87 chop53 and ftol control0E7F, flat level2 cells/104 height and structural bridge416 globals are supplied; lifecycle/map/rules parsing are excluded',
                     'Prefix admits selector and valid state, sets class destination and retained target independently; linked-member chain is empty',
                     'Sinking/crush bytes are supplied; flag producers, world callbacks, ProcessMovement target publication and later paid points are outside coverage',
                     'Finite normal/zero numeric inputs and signed64-convertible products; NaN/infinity/subnormal/ftol invalid cases excluded'],
        substitutions=['No instruction patches, custom vtables or hooks replacing return values/control flow; observations only'],
        entry_points={'drive_prefix':0x4B0F20,'ship_prefix':0x6A05F0,'foot_getter':0x4DB1A0,
                      'house_bonus':0x50C050,'ability':0x70D0D0,'setter':0x4D3710,'ftol':0x7C5F00,
                      'drive_stop_after_budget':0x4B1297,'ship_stop_after_budget':0x6A095F})


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
