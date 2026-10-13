"""Native references for an offline Robot Tank (sim::powered_unit).

Run python -m tools.robot_offline_oracle --check (or explicit --write).
Rust consumers: src/sim/movement/hover_tests.rs (hover_power_off) and
src/sim/powered_unit_tests.rs (sparks).

Sections, each case executed in a fresh emulator over a Scenario Random
seeded by 0x65C6D0 (RandomRanged 0x65C7E0 runs natively; each draw's bounds
and value are recorded):
- hover_power_off: HoverLocomotionClass::Power_Off 0x516BF0, the ILocomotion
  +0x5C that TechnoClass::Deactivate 0x70FC90 calls. Its Is_Powered
  0x516C70, Is_Moving 0x514C30, Stop_Moving 0x516320 and the base Power_Off
  0x55A910 run natively on the interface (vtable 0x7EACFC; offsets here are
  the interface's, the object's less 4).
- sparks: the head of UnitClass::AI 0x7360C0 up to 0x7361A9, an offline
  Unit's (Techno+0x1C8) DefaultSparkSystem (Rules+0x1020) phase test and
  its two draws.

Recorded stubs: the Foot's GetHeight (vt+0x1C8) answers the row's height,
the Unit's Fetch_ID (the vtable at +4, slot +0x10) the row's ID, and the
ParticleSystemClass constructor 0x62DC50 records its arguments.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_FPCW

from tools.ai_base_building_oracle import FAKE, RULES, SCENARIO, STUBS, Emu, u32
from tools.native_oracle import (NATIVE_FPCW, RET_MAGIC, STACK_BASE, STACK_SIZE, OracleError,
                                 finish_vectors, provenance, run_checked)

POWER_OFF = 0x516BF0
UNIT_AI = 0x7360C0
SPARKS_END = 0x7361A9
RANDOM_SEED = 0x65C6D0
RANDOM_RANGED = 0x65C7E0
PARTICLE_SYSTEM = 0x62DC50
HOVER_VTABLE = 0x7EACFC
FRAME = 0xA8ED84
SCENARIO_RANDOM = SCENARIO + 0x218

LOCO = FAKE + 0x40000
FOOT = FAKE + 0x42000
FOOT_VTABLE = FAKE + 0x44000
UNIT = FAKE + 0x46000
UNIT_VTABLE = FAKE + 0x48000
SPARK_TYPE = FAKE + 0x49000
STUB_HEIGHT = STUBS + 0x300
STUB_FETCH_ID = STUBS + 0x310

# The Foot's path head word (+0x5E0) before the call: a direction.
PATH_HEAD = 2

SLEEP, MOVE, GUARD, ENTER, NONE = 0, 2, 5, 7, -1


def i32(value):
    return struct.unpack('<i', u32(value))[0]


def write8(emu, address, value):
    emu.uc.mem_write(address, bytes([int(value) & 0xFF]))


def read8(emu, address):
    return emu.uc.mem_read(address, 1)[0]


def write_coord(emu, address, coord):
    """A null coordinate is the zero CoordStruct (0xA8F180, 0xB1CFE8)."""
    for index, value in enumerate(coord or (0, 0, 0)):
        emu.write32(address + 4 * index, value)


def read_coord(emu, address):
    return [emu.read_i32(address + 4 * index) for index in range(3)]


def seeded(seed):
    emu = Emu()
    emu.invoke(RANDOM_SEED, ecx=SCENARIO_RANDOM, args=[seed])
    return emu


def record_draws(emu, returns):
    """Each RandomRanged call's bounds at entry and its value where the
    caller resumes."""
    def entered(uc, _address, _size, _data):
        if uc.reg_read(UC_X86_REG_ECX) != SCENARIO_RANDOM:
            raise OracleError('a draw from another stream')
        sp = uc.reg_read(UC_X86_REG_ESP)
        emu.events.append([i32(emu.read32(sp + 4)), i32(emu.read32(sp + 8))])

    def returned(uc, _address, _size, _data):
        emu.events[-1].append(i32(uc.reg_read(UC_X86_REG_EAX)))
    emu.uc.hook_add(UC_HOOK_CODE, entered, begin=RANDOM_RANGED, end=RANDOM_RANGED)
    for address in returns:
        emu.uc.hook_add(UC_HOOK_CODE, returned, begin=address, end=address)


def rng_indices(emu):
    """Random2Class +4 and +8, the two lag cursors."""
    return [emu.read_i32(SCENARIO_RANDOM + 4), emu.read_i32(SCENARIO_RANDOM + 8)]


# ---------------------------------------------------------------- Hover Power_Off


def hover_row(name, *, powered, height, mission, destination=None, head=None,
              shove=(False, 0), seed=1):
    case = dict(name=name, seed=seed, powered=powered, height=height, mission=mission,
                destination=list(destination or (0, 0, 0)), head=list(head or (0, 0, 0)),
                shove=list(shove), path_head=PATH_HEAD)
    emu = seeded(seed)
    emu.write32(LOCO, HOVER_VTABLE)
    emu.write32(LOCO + 0x08, FOOT)
    write8(emu, LOCO + 0x0C, powered)
    write_coord(emu, LOCO + 0x14, destination)
    write_coord(emu, LOCO + 0x20, head)
    write8(emu, LOCO + 0x64, shove[0])
    emu.write32(LOCO + 0x68, shove[1])
    emu.write32(FOOT, FOOT_VTABLE)
    emu.write32(FOOT_VTABLE + 0x1C8, STUB_HEIGHT)
    emu.write32(FOOT + 0xAC, mission)
    emu.write32(FOOT + 0x5E0, PATH_HEAD)

    def get_height(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != FOOT:
            raise OracleError('GetHeight on another object')
        return height
    emu.hook(STUB_HEIGHT, get_height, 0)
    record_draws(emu, (0x516C2A, 0x516C41))
    emu.invoke(POWER_OFF, args=(LOCO,))
    case['expected'] = dict(
        shove=[bool(read8(emu, LOCO + 0x64)), emu.read_i32(LOCO + 0x68)],
        powered=bool(read8(emu, LOCO + 0x0C)),
        destination=read_coord(emu, LOCO + 0x14),
        head=read_coord(emu, LOCO + 0x20),
        path_head=emu.read_i32(FOOT + 0x5E0),
        draws=emu.events,
        rng_indices=rng_indices(emu),
    )
    return case


DESTINATION = (2560 + 128, 3072 + 128, 0)
HEAD = (2816 + 128, 3072 + 128, 0)
AIRBORNE = (('powered, on the ground', True, 0), ('unpowered, one lepton up', False, 1),
            ('unpowered, on the ground', False, 0), ('unpowered, below the ground', False, -8),
            ('powered, high', True, 240))
MOTIONS = (('still', None, None), ('a destination', DESTINATION, None),
           ('a head', None, HEAD), ('the destination its head', HEAD, HEAD),
           ('a destination and another head', DESTINATION, HEAD))


def hover_power_off():
    rows = []
    seed = 1
    for state, powered, height in AIRBORNE:
        for mission_name, mission in (('None', NONE), ('Sleep', SLEEP), ('Guard', GUARD),
                                      ('Enter', ENTER), ('Move', MOVE)):
            rows.append(hover_row(f'{state}, mission {mission_name}', powered=powered,
                                  height=height, mission=mission, destination=DESTINATION,
                                  head=HEAD, seed=seed))
            seed += 1
    for state, powered, height in (AIRBORNE[0], AIRBORNE[2]):
        for motion, destination, head in MOTIONS:
            rows.append(hover_row(f'{state}, Guard, {motion}', powered=powered, height=height,
                                  mission=GUARD, destination=destination, head=head,
                                  seed=seed))
            seed += 1
    for state, powered, height in (AIRBORNE[1], AIRBORNE[3]):
        rows.append(hover_row(f'{state}, Guard, an earlier spin', powered=powered,
                              height=height, mission=GUARD, shove=(True, -7), seed=seed))
        seed += 1
    for seed in range(1000, 1012):
        rows.append(hover_row(f'powered, Guard, seed {seed}', powered=True, height=0,
                              mission=GUARD, seed=seed))
    return rows


# ---------------------------------------------------------------- sparks


def c_div(value, divisor):
    """C's signed division, toward zero."""
    quotient = abs(value) // abs(divisor)
    return quotient if (value < 0) == (divisor < 0) else -quotient


def phase(location, frame, ident):
    x, y, _z = location
    return i32(c_div(x, 10) + (c_div(y, 10) << 16) + frame + ident)


def frame_for(location, ident, base, remainder=0):
    """The first frame from `base` whose phase leaves `remainder` (C's sign)
    after division by 30; the native run decides the row either way."""
    for frame in range(base, base + 60):
        value = phase(location, frame, ident)
        if value - 30 * c_div(value, 30) == remainder:
            return frame
    raise OracleError('no frame gives that remainder')


def run_unit_ai_head(emu):
    uc = emu.uc
    sp = STACK_BASE + STACK_SIZE - 0x1000 - 4
    uc.mem_write(sp, u32(RET_MAGIC))
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    uc.reg_write(UC_X86_REG_ECX, UNIT)
    run_checked(uc, UNIT_AI, SPARKS_END)


def spark_row(name, *, location, ident, frame, deactivated=True, seed=1):
    case = dict(name=name, seed=seed, deactivated=deactivated, location=list(location),
                id=ident, frame=frame)
    emu = seeded(seed)
    write8(emu, UNIT + 0x1C8, deactivated)
    write_coord(emu, UNIT + 0x9C, location)
    emu.write32(UNIT + 4, UNIT_VTABLE)
    emu.write32(UNIT_VTABLE + 0x10, STUB_FETCH_ID)
    emu.write32(FRAME, frame)
    emu.write32(RULES + 0x1020, SPARK_TYPE)
    systems = []

    def fetch_id(e):
        if e.arg(0) != UNIT + 4:
            raise OracleError('Fetch_ID on another interface')
        return ident

    def construct(e):
        type_, coords, target, owner, target_coords, house = (e.arg(index) for index in range(6))
        if type_ != SPARK_TYPE:
            raise OracleError('a system of another type')
        systems.append(dict(coords=read_coord(e, coords), target=target, owner=owner,
                            target_coords=read_coord(e, target_coords), house=house))
        return e.uc.reg_read(UC_X86_REG_ECX)
    emu.hook(STUB_FETCH_ID, fetch_id, 4)
    emu.hook(PARTICLE_SYSTEM, construct, 0x18)
    record_draws(emu, (0x73614A, 0x736162))
    run_unit_ai_head(emu)
    case['expected'] = dict(systems=systems, draws=emu.events, rng_indices=rng_indices(emu))
    return case


def sparks():
    rows = []
    add = rows.append
    cell = (40 * 256 + 128, 52 * 256 + 64, 0)
    add(spark_row('online', location=cell, ident=17, frame=frame_for(cell, 17, 3000),
                  deactivated=False))
    for seed in (1, 2, 3, 4, 5, 6):
        add(spark_row(f'offline, a spark frame, seed {seed}', location=cell, ident=17,
                      frame=frame_for(cell, 17, 3000 + 60 * seed), seed=seed))
    for remainder in (1, 29):
        add(spark_row(f'offline, phase remainder {remainder}', location=cell, ident=17,
                      frame=frame_for(cell, 17, 3000, remainder)))
    hover = (12 * 256 + 5, 7 * 256 + 250, 4 * 104 + 120)
    add(spark_row('offline, its height above the ground', location=hover, ident=9,
                  frame=frame_for(hover, 9, 100), seed=7))
    west = (-1234, 9 * 256, 0)
    add(spark_row('offline, a negative X divides toward zero', location=west, ident=3,
                  frame=frame_for(west, 3, 500), seed=8))
    north = (9 * 256, -4321, 0)
    add(spark_row('offline, a negative Y', location=north, ident=3,
                  frame=frame_for(north, 3, 500), seed=9))
    for remainder in (-1, -29):
        add(spark_row(f'offline, a negative phase, remainder {remainder}', location=north,
                      ident=3, frame=frame_for(north, 3, 500, remainder)))
    far = (100 * 256, 5_000_005, 0)
    add(spark_row('offline, Y shifted past 32 bits', location=far, ident=40,
                  frame=frame_for(far, 40, 900), seed=10))
    late = 0xFFFFFF00
    add(spark_row('offline, a frame past 2^31', location=cell, ident=17,
                  frame=frame_for(cell, 17, late) & 0xFFFFFFFF, seed=11))
    big = 0x7FFFFFF0
    add(spark_row('offline, an ID near 2^31', location=cell, ident=big,
                  frame=frame_for(cell, big, 77), seed=12))
    return rows


def generate():
    return dict(hover_power_off=hover_power_off(), sparks=sparks())


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope=('HoverLocomotionClass::Power_Off on a synthetic interface and Foot over its '
               'powered byte, height, mission and motion; the spark block at the head of '
               'UnitClass::AI on a synthetic Unit over its latch, Location, Frame and ID.'),
        assumptions=['The Scenario Random is seeded by its own Seed 0x65C6D0 with the row seed.',
                     'The null coordinate globals 0xA8F180 and 0xB1CFE8 are zero, as their '
                     'static initializers 0x513BC0 and 0x735330 leave them.'],
        substitutions=["The Foot's GetHeight (vt+0x1C8) answers the row's height.",
                       "The Unit's Fetch_ID (the vtable at +4, slot +0x10) answers the row's ID.",
                       'The ParticleSystemClass constructor 0x62DC50 records its arguments and '
                       'returns; operator new allocates from the fixture heap.'],
        entry_points={'hover_power_off': POWER_OFF, 'sparks': UNIT_AI},
    ))
