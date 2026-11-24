"""Original AircraftClass::Unlimbo (0x00414310): the coordinate Z it hands
Foot Unlimbo, and the Stage and speed fraction its tail writes.

Run: python -m tools.spatial_oracle.aircraft_unlimbo_height [--check | --write]

Executes the whole function with the real Aircraft vtable. Stubbed callees
keep their own evidence: the floor (MapClass 0x00578080,
tools/ramp_height_vectors.json), playfield membership (0x005785F0,
tools/spatial_oracle/map_queries.py), FootClass::Unlimbo (0x004D7170: commits
the coordinate it receives as the Location, +0x9C..+0xA4, and returns the
supplied result) and the Secondary facing setter (0x004C9300). Real: the
type's FlightLevel (vt+0xBC, 0x00717800), the +3D4 suffix with GetWeapon
(0x0070E140, no weapon, so +3D4 keeps its input), GetHeight (vt+0x1C8,
0x005F5F40, over the stubbed floor) and SetSpeedFraction (vt+0x544,
0x004D3710).
"""
from itertools import product
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EIP

from tools.native_oracle import (
    RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors, load_image, provenance,
    run_checked,
)
from tools.spatial_oracle.map_queries import dwords

OWNER, TYPE, RULES, COORD, DIR = [SCRATCH + n for n in (0, 0x2000, 0x4000, 0x6000, 0x6100)]
UNLIMBO, FLOOR, PLAYFIELD, FOOT_UNLIMBO, FACING = (
    0x00414310, 0x00578080, 0x005785F0, 0x004D7170, 0x004C9300)
FRAME = 1000
# Prior Stage and speed fraction, so an unwritten field stays visible.
STAGE_BEFORE = (7, -1, 9, 3)
FRACTION_BEFORE = struct.pack('<d', 0.5)


def u32(u, address):
    return struct.unpack('<I', u.mem_read(address, 4))[0]


def i32(u, address):
    return struct.unpack('<i', u.mem_read(address, 4))[0]


def execute(case):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(SCRATCH, 0x10000)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)
    u.mem_write(OWNER, dwords(0x7E22A4))
    u.mem_write(OWNER + 0x6C4, dwords(TYPE))
    u.mem_write(OWNER + 0x3D4, bytes([case['mission_only']]))
    value, start, left, rate = STAGE_BEFORE
    u.mem_write(OWNER + 0xF8, dwords(value))
    u.mem_write(OWNER + 0x100, dwords(start, 0, left, rate))
    u.mem_write(OWNER + 0x578, FRACTION_BEFORE)
    u.mem_write(TYPE, dwords(0x7E2868))
    u.mem_write(TYPE + 0xD68, bytes([case['missile_spawn']]))
    u.mem_write(TYPE + 0x618, dwords(case['type_flight_level']))
    u.mem_write(TYPE + 0x230, bytes([1]))
    u.mem_write(TYPE + 0xE0A, bytes([1]))
    u.mem_write(0x8871E0, dwords(RULES))
    u.mem_write(RULES + 0x7B4, dwords(case['rules_flight_level']))
    u.mem_write(0xA8ED84, dwords(FRAME))
    u.mem_write(0xAC13BC, dwords(416))
    u.mem_write(COORD, dwords(*case['input']))

    received = []

    def returns(eax, pops):
        sp = u.reg_read(UC_X86_REG_ESP)
        if eax is not None:
            u.reg_write(UC_X86_REG_EAX, eax & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_EIP, u32(u, sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + pops)

    def arg(n):
        return u32(u, u.reg_read(UC_X86_REG_ESP) + 4 + 4 * n)

    def on_floor(*_):
        returns(case['ground'], 4)

    def on_playfield(*_):
        returns(int(case['in_playfield']), 4)

    def on_foot_unlimbo(*_):
        assert u.reg_read(UC_X86_REG_ECX) == OWNER
        coord = struct.unpack('<iii', u.mem_read(arg(0), 12))
        received.append(list(coord))
        if case['success']:
            u.mem_write(OWNER + 0x9C, struct.pack('<iii', *coord))
        returns(int(case['success']), 8)

    def on_facing(*_):
        returns(None, 4)

    for address, hook in ((FLOOR, on_floor), (PLAYFIELD, on_playfield),
                          (FOOT_UNLIMBO, on_foot_unlimbo), (FACING, on_facing)):
        u.hook_add(UC_HOOK_CODE, hook, begin=address, end=address)

    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.mem_write(sp, dwords(RET_MAGIC, COORD, case['dir']))
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_ECX, OWNER)
    run_checked(u, UNLIMBO, RET_MAGIC, count=20_000)
    assert u.reg_read(UC_X86_REG_ESP) == sp + 12
    assert len(received) == 1
    return dict(
        input=case,
        result=u.reg_read(UC_X86_REG_EAX) & 0xFF,
        unlimbo_coord=received[0],
        mission_only=bool(u.mem_read(OWNER + 0x3D4, 1)[0]),
        stage=dict(value=i32(u, OWNER + 0xF8), start=i32(u, OWNER + 0x100),
                   left=i32(u, OWNER + 0x108), rate=i32(u, OWNER + 0x10C)),
        speed_fraction_bits=f"0x{struct.unpack('<Q', u.mem_read(OWNER + 0x578, 8))[0]:016X}",
    )


def generate():
    rows = []
    for missile_spawn, mission_only, in_playfield, ground, type_level, input_z in product(
            (False, True), (False, True), (False, True), (0, 52, 416), (-1, 0, 2000),
            (0, 999)):
        rows.append(dict(missile_spawn=missile_spawn, mission_only=mission_only,
                         in_playfield=in_playfield, ground=ground,
                         type_flight_level=type_level, rules_flight_level=1500,
                         input=[1408, 1664, input_z], dir=0x40, success=True))
    # A failed Foot placement returns before the tail.
    for missile_spawn in (False, True):
        rows.append(dict(missile_spawn=missile_spawn, mission_only=False, in_playfield=True,
                         ground=52, type_flight_level=-1, rules_flight_level=1500,
                         input=[1408, 1664, 0], dir=0x40, success=False))
    return [execute(case) for case in rows]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'aircraft_unlimbo': UNLIMBO, 'type_flight_level': 0x717800,
                      'get_height': 0x5F5F40, 'set_speed_fraction': 0x4D3710},
        assumptions=['Supplied Aircraft object/type, Rules FlightLevel and frame; original '
                     'Aircraft and AircraftType vtables; Selectable, Landable, no weapon.',
                     'Flat supplied floor at every query, not OnBridge.'],
        substitutions=['MapClass floor 0x00578080 returns the supplied ground.',
                       'Playfield membership 0x005785F0 returns the supplied answer.',
                       'FootClass::Unlimbo 0x004D7170 commits the received coordinate as '
                       'the Location and returns the supplied result.',
                       'Secondary facing setter 0x004C9300 returns without effect.'],
        scope='146 whole AircraftClass::Unlimbo runs over MissileSpawn, +3D4, playfield, '
              'floor, type/Rules FlightLevel and input Z, two with a failed Foot '
              'placement. Covers the coordinate handed to Foot Unlimbo, the +3D4 result, '
              'the Stage writes and the stored speed fraction. No Foot/Techno Unlimbo, '
              'facing, +0x6C9 or OnBridge claim.',
    ))
