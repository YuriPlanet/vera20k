"""Original Fly target speed with real Aircraft/Unit, IFlyControl and Object methods.

Runs the bounded 4CE145..4CE2E5 range of FlyLocomotionClass::Process: the per-frame
writer of the target speed (+0x40) and its current-speed (+0x48) side effects, behind
its own health/landing/takeoff/destination gate. The distance local [ESP+0x20]
(4CDDD3) and the type local [ESP+0x2C] (4CDA24) are supplied; 4D0180, Aircraft
QueryInterface 414290, IFlyControl +18/+1C/+20, GetWeapon 70E140 and GetHeight 5F5F40
execute. No calls or instructions are substituted.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_AL, UC_X86_REG_EBP, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import SCRATCH, RET_MAGIC, run_checked, finish_vectors, provenance
from tools.spatial_oracle.crate_speed_effect import fixture, CELL, RULES
from tools.spatial_oracle.map_queries import dwords, EMPTY_TABLE, TABLE, GLOBAL_TABLE

LOCO = SCRATCH + 0x1B000
WEAPON, PROJECTILE, TARGET = SCRATCH + 0x1C000, SCRATCH + 0x1C400, SCRATCH + 0x1C800
NULL_COORD_INIT = 0x4CC940          # static initializer of the Fly null coordinate 8B3C78
FLY_CONSTRUCTOR = 0x4CC9A0
# Weapon 0's projectile ROT/Inviso and Type Fighter (+0xE0E) per class, as IFlyControl
# +0x18 (41B7F0) and +0x1C (41B840) read them.
CLASSES = {
    'strafe': dict(rot=1, inviso=0, fighter=0),     # HORNET/ASW: NormalBomb/DepthCharge
    'fighter': dict(rot=100, inviso=0, fighter=1),  # ORCA/BEAG/BPLN
    'neither': dict(rot=100, inviso=0, fighter=0),
    'inviso': dict(rot=1, inviso=1, fighter=0),     # ROT 1 but Inviso: not a strafer
    'unarmed': None,                                # no weapon 0: 41B7F0 returns 0
}
LANDMARKS = {0x4CE1E9: 'full', 0x4CE21F: 'hunter_chase', 0x4CE22E: 'hunter_stop',
             0x4CE239: 'slowdown', 0x4CE27C: 'tenth', 0x4CE288: 'halve',
             0x4CE2A4: 'distance_clamp', 0x4CE2D1: 'creep'}


def q16_double(bits):
    return struct.pack('<d', bits / 65536)


def read_double(u, address):
    raw = bytes(u.mem_read(address, 8))
    value = struct.unpack('<d', raw)[0]
    return f'{struct.unpack("<Q", raw)[0]:016x}', value


def execute(case):
    kind = case.get('kind', 'aircraft')
    u, sp, actors, _ = fixture(dict(actors=[dict(kind=kind)]))
    owner = actors[0]
    object_type = owner + 0x800
    u.mem_write(sp, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ESP, sp)
    run_checked(u, NULL_COORD_INIT, RET_MAGIC, count=20)
    # Takeoff rows read GetHeight over one real cell, as tools.spatial_oracle.fly_height.
    u.mem_write(TABLE, EMPTY_TABLE)
    u.mem_write(GLOBAL_TABLE, dwords(TABLE, 0x40000))
    u.mem_write(TABLE + (10 * 512 + 10) * 4, dwords(CELL))
    u.mem_write(CELL + 0x11B, bytes([0, 0]))
    u.mem_write(CELL + 0x140, dwords(0))
    for address, value in [(0xAC13C8, 104), (0xAC13BC, 416), (0xABC5DC, 416), (0x8B3CAC, 416)]:
        u.mem_write(address, dwords(value))
    u.mem_write(RULES + 0x7B4, dwords(1500))
    u.mem_write(owner + 0x6C, dwords(case.get('health', 100)))
    u.mem_write(owner + 0x74, b'\0')
    u.mem_write(owner + 0x9C, dwords(2688, 2688, case.get('z', 1500)))
    u.mem_write(owner + 0x2B4, dwords(TARGET if case.get('target', False) else 0))
    u.mem_write(owner + 0x2FC, dwords(case.get('ammo', 1)))
    u.mem_write(object_type + 0x2F8, dwords(case.get('slowdown', 500)))
    u.mem_write(object_type + 0xD27, bytes([int(case.get('hunter_seeker', False))]))
    if kind == 'aircraft':
        # Real Aircraft constructor 413DA2 installs the IFlyControl subobject here;
        # QueryInterface 414290 returns it for IID 822410.
        u.mem_write(owner + 0x6C0, dwords(0x7E2250))
        u.mem_write(owner + 0x6D2, bytes([int(case.get('locked', False))]))
        u.mem_write(object_type + 0xE0B, bytes([int(case.get('fly_by', False))]))
        cls = CLASSES[case.get('class', 'neither')]
        if cls is not None:
            u.mem_write(object_type + 0xE0E, bytes([cls['fighter']]))
            u.mem_write(object_type + 0x898, dwords(WEAPON))
            u.mem_write(WEAPON + 0xA0, dwords(PROJECTILE))
            u.mem_write(PROJECTILE + 0x2DC, dwords(cls['rot']))
            u.mem_write(PROJECTILE + 0x29E, bytes([cls['inviso']]))
    u.mem_write(sp, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_ECX, LOCO)
    run_checked(u, FLY_CONSTRUCTOR, RET_MAGIC, count=200)
    u.mem_write(LOCO + 0xC, dwords(owner))
    u.mem_write(LOCO + 0x1C, dwords(*case.get('destination', [2944, 2688, 0])))
    u.mem_write(LOCO + 0x38, dwords(case.get('target_height', 1500)))
    u.mem_write(LOCO + 0x40, q16_double(case.get('target_speed', 65536)))
    u.mem_write(LOCO + 0x48, q16_double(case.get('current', 32768)))
    u.mem_write(LOCO + 0x50, bytes([int(case.get('taking_off', False)),
                                    int(case.get('landing', False))]))
    u.mem_write(LOCO + 0x5C, bytes([int(case.get('cruise', False))]))
    u.mem_write(sp + 0x20, dwords(case['distance']))
    u.mem_write(sp + 0x2C, dwords(object_type))
    path = []

    def observe(_u, address, _size, _data):
        if address == 0x4CE1E5:
            path.append('may_slow' if _u.reg_read(UC_X86_REG_AL) else 'may_not_slow')
        elif address in LANDMARKS:
            path.append(LANDMARKS[address])
    u.hook_add(UC_HOOK_CODE, observe)
    u.reg_write(UC_X86_REG_ESI, LOCO)
    u.reg_write(UC_X86_REG_EBP, 0)
    u.reg_write(UC_X86_REG_ESP, sp)
    run_checked(u, 0x4CE145, 0x4CE2E5, count=20000, required_addresses=[0x4CE145])
    assert u.reg_read(UC_X86_REG_ESP) == sp
    target_bits, target = read_double(u, LOCO + 0x40)
    current_bits, current = read_double(u, LOCO + 0x48)
    return dict(input=case, path=path, target_speed=target, target_speed_bits=target_bits,
                target_speed_q16=int(target * 65536 // 1) if target == target else None,
                current=current, current_bits=current_bits,
                current_q16=int(current * 65536 // 1) if current == current else None)


def generate():
    cases = []
    # The slowdown law: ratio, cap, the 0.1 floor, the stop-and-halve arm and the
    # 0x4CE29A/0x4CE2AB current-speed tails, around SlowdownDistance=500 (retail default).
    for distance in (0, 1, 2, 49, 50, 51, 84, 85, 86, 87, 125, 128, 250, 333, 499, 500,
                     501, 700, 32767, 32768, 100000, 2147483647):
        for current in (0, 1, 3, 3276, 6554, 32768, 65536):
            cases.append(dict(name=f'slowdown_{distance}_{current}', distance=distance,
                              current=current))
    for slowdown in (-500, -1, 0, 1, 10, 50, 850, 851, 860, 1000, 65536):
        for distance in (0, 1, 5, 85, 86, 99, 100, 101, 849, 850, 851, 5000):
            cases.append(dict(name=f'authored_{slowdown}_{distance}', slowdown=slowdown,
                              distance=distance, current=16384))
    # 0x004D0180: lock, landing/cruise, FlyBy, class and Ammo precedence.
    for cls in CLASSES:
        for cruise in (False, True):
            for ammo in (1, 0, -1):
                for fly_by in (False, True):
                    cases.append(dict(name=f'may_slow_{cls}_{cruise}_{ammo}_{fly_by}',
                                      distance=250, current=32768, cruise=cruise,
                                      ammo=ammo, fly_by=fly_by, **{'class': cls}))
    for locked in (False, True):
        for landing_cruise in ((False, False), (False, True)):
            cases.append(dict(name=f'locked_{locked}_{landing_cruise[1]}', distance=250,
                              current=32768, locked=locked, cruise=landing_cruise[1]))
    cases.append(dict(name='strafe_cruise_armed_target', distance=40, current=6554,
                      cruise=True, target=True, **{'class': 'strafe'}))
    cases.append(dict(name='unit_owner_ammo', kind='unit', distance=250, current=32768,
                      cruise=True, ammo=3))
    cases.append(dict(name='unit_owner_empty', kind='unit', distance=250, current=32768,
                      cruise=True, ammo=0))
    cases.append(dict(name='unit_owner_not_cruising', kind='unit', distance=250,
                      current=32768, ammo=3))
    # HunterSeeker (type+0xD27): chase while not taking off with a Target, else stop.
    for target in (False, True):
        for taking_off in (False, True):
            cases.append(dict(name=f'hunter_{target}_{taking_off}', distance=250,
                              current=32768, hunter_seeker=True, target=target,
                              taking_off=taking_off, z=1500))
    # The gate: health, landing, takeoff height against half the target, null destination.
    for health in (0, -1, 1):
        cases.append(dict(name=f'health_{health}', distance=250, current=32768,
                          target_speed=13107, health=health))
    cases.append(dict(name='landing', distance=250, current=32768, target_speed=13107,
                      landing=True))
    for target_height, z in ((1500, 749), (1500, 750), (1501, 749), (1501, 750),
                             (1500, 0), (-3, -2), (-3, -1), (0, 0)):
        cases.append(dict(name=f'takeoff_{target_height}_{z}', distance=250, current=32768,
                          target_speed=13107, taking_off=True, target_height=target_height,
                          z=z))
    for destination in ([0, 0, 0], [0, 0, 1], [1, 0, 0], [0, 1, 0]):
        cases.append(dict(name=f'destination_{destination}', distance=250, current=32768,
                          target_speed=13107, destination=destination))
    return [execute(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'target_speed': 0x4CE145, 'after_release': 0x4CE2E5,
                      'may_slow': 0x4D0180, 'aircraft_query_interface': 0x414290,
                      'is_strafe': 0x41B7F0, 'is_fighter': 0x41B840, 'is_locked': 0x41B860,
                      'get_weapon': 0x70E140, 'get_height': 0x5F5F40,
                      'fly_constructor': FLY_CONSTRUCTOR, 'null_coordinate': NULL_COORD_INIT},
        assumptions=['Supplied post-vertical-step frame: EBP zero as both entry paths leave it, the distance local [ESP+0x20] and the type local [ESP+0x2C].',
                     'Original Fly constructor, then supplied owner, destination, target height, target and current speed, takeoff/landing bytes +50/+51 and cruise +5C. Target and current speeds are exact Q16 values, matching the SimFixed policy; unrestricted binary64 inputs are excluded.',
                     'Real Aircraft/Unit vtables; the Aircraft IFlyControl subobject 7E2250 at +6C0 as its constructor installs it; supplied type HunterSeeker +D27, SlowdownDistance +2F8, FlyBy +E0B, Fighter +E0E and weapon 0 with its projectile ROT +2DC and Inviso +29E. Rookie owner (base weapon).',
                     'Takeoff rows read GetHeight over one real level-0 cell with the Map+13C table and +140 length initialized, as fly_height. PC53/chop ambient state.',
                     'The following attitude, landing trigger, ramp and Horizontal_Step are outside this range.'],
        substitutions=[],
        scope='The per-frame Fly target-speed writer: SlowdownDistance ratio and cap (including zero and negative authored distances), the strict 0.1 floor and its 85-lepton stop-and-halve arm, the zero-distance current clamp and the 0.05 creep; 0x004D0180 lock/landing/cruise/FlyBy/strafe/fighter/Inviso/unarmed/Ammo precedence for Aircraft and Unit owners; HunterSeeker; and the health, landing, half-height takeoff and null-destination gate.',
    ))
