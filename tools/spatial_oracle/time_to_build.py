"""Original TechnoClass::Time_To_Build (0x6F47A0) totals for chosen inputs."""
from pathlib import Path
import struct
from tools.native_oracle import call, finish_vectors, provenance, SCRATCH

OBJECT, TYPE, HOUSE, COUNTRY, RULES = [SCRATCH + n for n in (0, 0x1000, 0x3000, 0x9000, 0xA000)]
RULES_GLOBAL = 0x8871E0

# kind: object vtable, the object's type-pointer offset, type vtable.
KINDS = {
    'infantry': (0x7EB058, 0x6C0, 0x7EB610),
    'unit': (0x7F5C70, 0x6C4, 0x7F6218),
    'aircraft': (0x7E22A4, 0x6C4, 0x7E2868),
    'building': (0x7E3EBC, 0x520, 0x7E4570),
}


def f32(value):
    return struct.unpack('<I', struct.pack('<f', value))[0]


def f64(value):
    return struct.unpack('<Q', struct.pack('<d', value))[0]


def widened(value):
    """INIClass::ReadDouble 0x5283D0 scans %f into a float and widens it."""
    return struct.unpack('<f', struct.pack('<f', value))[0]


def case(kind, cost, *, build_speed=widened(0.7), btm=1.0, country=1.0, power=(100, 100),
         count=1, multiple_factory=0.8, penalty=1.0, min_speed=0.5, max_speed=0.8,
         naval=False, defense=False, wall=False, wall_coefficient=3.0):
    """One row; floats are stored as the bits the native fields hold."""
    return dict(
        kind=kind, cost=cost, build_speed_bits=f64(build_speed), btm_bits=f32(btm),
        country_bits=f32(country), power_output=power[0], power_drain=power[1],
        factory_count=count, multiple_factory_bits=f32(multiple_factory),
        penalty_bits=f32(penalty), min_speed_bits=f32(min_speed),
        max_speed_bits=f32(max_speed), naval=naval, defense=defense, wall=wall,
        wall_coefficient_bits=f64(wall_coefficient))


def fixture_writes(row):
    """The object, type, house, country and Rules bytes for one row."""
    object_vtable, type_offset, type_vtable = KINDS[row['kind']]
    # The five house factory counters (+0x5378 aircraft, +0x537C infantry,
    # +0x5380 vehicles, +0x5384 buildings, +0x5388 ships) all hold a marker
    # except the one 0x500910 must read, so a wrong counter shows up.
    counters = [97, 97, 97, 97, 97]
    slot = {'aircraft': 0, 'infantry': 1, 'building': 3}.get(
        row['kind'], 4 if row['naval'] else 2)
    counters[slot] = row['factory_count']
    # Country build-time multipliers +0x134 infantry, +0x138 units, +0x13C
    # aircraft, +0x140 buildings, +0x144 defenses; the others hold 64.0.
    country = [64.0] * 5
    country_slot = {'infantry': 0, 'unit': 1, 'aircraft': 2}.get(
        row['kind'], 4 if row['defense'] else 3)
    country_values = [struct.pack('<f', v) for v in country]
    country_values[country_slot] = struct.pack('<I', row['country_bits'])
    return {
        OBJECT: struct.pack('<I', object_vtable),
        OBJECT + 0x21C: struct.pack('<I', HOUSE),
        OBJECT + type_offset: struct.pack('<I', TYPE),
        TYPE: struct.pack('<I', type_vtable),
        TYPE + 0x608: struct.pack('<I', row['btm_bits']),
        TYPE + 0x610: struct.pack('<i', row['cost']),
        TYPE + 0xCCE: bytes([1 if row['naval'] else 0]),
        TYPE + 0xE08: struct.pack('<i', 5 if row['defense'] else 0),
        TYPE + 0x1571: bytes([1 if row['wall'] else 0]),
        HOUSE + 0x34: struct.pack('<I', COUNTRY),
        HOUSE + 0x5378: struct.pack('<5i', *counters),
        HOUSE + 0x53A4: struct.pack('<ii', row['power_output'], row['power_drain']),
        COUNTRY + 0x134: b''.join(country_values),
        RULES + 0x570: struct.pack('<4I', row['min_speed_bits'], row['max_speed_bits'],
                                   row['penalty_bits'], row['multiple_factory_bits']),
        RULES + 0x758: struct.pack('<Q', row['wall_coefficient_bits']),
        RULES + 0x1748: struct.pack('<Q', row['build_speed_bits']),
        RULES_GLOBAL: struct.pack('<I', RULES),
    }


def run(row):
    result = call(0x6F47A0, ecx=OBJECT, writes=fixture_writes(row),
                  required_addresses=[0x711EE0, 0x50C0A0, 0x4FCE30, 0x500910],
                  timeout_instr=200_000)
    return dict(row, time_to_build=struct.unpack('<i', struct.pack('<I', result['eax']))[0])


def generate():
    rows = []
    # Stock [General] BuildSpeed=.7, full power, one factory: the costs and
    # BuildTimeMultiplier values retail types use, per production class.
    for kind in ('infantry', 'unit', 'aircraft', 'building'):
        for cost in (0, 1, 50, 100, 120, 200, 300, 400, 500, 600, 700, 750, 800, 900,
                     1000, 1200, 1500, 1750, 2000, 2500, 3000, 5000):
            rows.append(case(kind, cost))
    for btm in (0.8, 1.1, 1.15, 1.2, 1.3, 1.5, 2.0):
        for cost in (200, 600, 700, 900, 1750):
            rows.append(case('unit', cost, btm=btm))
    # BuildSpeed other than stock (float-widened as ReadDouble stores it), the
    # constructor's 1.0, and doubles only a percent value (x 0.01) can hold.
    for build_speed in (1.0, widened(0.5), widened(0.58), widened(0.35), 2.0, 0.0,
                        0.7, 70 * 0.01, 58 * 0.01):
        for cost in (100, 700, 1000, 1500):
            rows.append(case('infantry', cost, build_speed=build_speed))
    # Country BuildTime multipliers per class, and the defense slot.
    for country in (0.75, 1.25, 0.5, 0.0):
        for kind in ('infantry', 'unit', 'aircraft', 'building'):
            rows.append(case(kind, 700, country=country))
        rows.append(case('building', 700, country=country, defense=True))
    # Power: full, surplus, blackout, partial and no-drain houses, with the
    # stock low-power clamps and with other clamp values.
    for power in ((100, 100), (200, 100), (0, 100), (50, 100), (99, 100), (1, 3),
                  (75, 100), (80, 100), (100, 0), (0, 0), (-50, 100), (2, 3)):
        rows.append(case('unit', 1000, power=power))
        rows.append(case('infantry', 300, power=power, btm=1.2))
    for penalty, lo, hi in ((2.0, 0.5, 0.8), (0.5, 0.5, 0.8), (1.0, 0.0, 1.0), (1.0, 0.2, 0.9),
                            (1.0, 0.9, 0.5), (4.0, 0.0, 1.0), (1.0, 0.0, 0.0)):
        for power in ((50, 100), (0, 100), (99, 100), (100, 100)):
            rows.append(case('unit', 1000, power=power, penalty=penalty,
                             min_speed=lo, max_speed=hi))
    # MultipleFactory: counts 0..6, stock .8 and other values, both classes.
    for multiple_factory in (0.8, 0.5, 1.0, 0.0, -0.5, 1.25):
        for count in (0, 1, 2, 3, 4, 6):
            rows.append(case('unit', 1000, count=count, multiple_factory=multiple_factory))
            rows.append(case('infantry', 200, count=count, multiple_factory=multiple_factory))
    rows.append(case('unit', 900, naval=True, count=3))
    rows.append(case('unit', 900, naval=False, count=3))
    rows.append(case('aircraft', 1200, count=2))
    rows.append(case('building', 1500, count=2))
    # Walls: stock WallBuildSpeedCoefficient=3.0, the constructor's 0.5 and others,
    # after the other factors.
    for coefficient in (3.0, 1.0, 0.5, 2.5, widened(1.3)):
        for cost in (0, 50, 100, 150):
            rows.append(case('building', cost, wall=True, wall_coefficient=coefficient))
    rows.append(case('building', 100, wall=True, count=3, power=(50, 100)))
    rows.append(case('unit', 100, wall=True))
    # The retail test world (production_replay_tests): retail MTNK, FV and E1
    # beside three GAPOWR and one factory, with stock [General] values.
    rows.append(case('unit', 700, btm=1.5, power=(600, 25)))
    rows.append(case('unit', 600, power=(600, 25)))
    rows.append(case('infantry', 200, power=(600, 10)))
    return [run(row) for row in rows]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original TechnoClass::Time_To_Build 0x6F47A0 on fixture objects: the '
              'type getter 0x711EE0, country multiplier 0x50C0A0, power ratio 0x4FCE30, '
              'factory counter 0x500910 and the Rules factors it reads. No claim about '
              'which objects reach it, the factory rate division or the step cadence.',
        assumptions=['Retail vtables supply WhatAmI and the type getters: Infantry '
                     '0x7EB058 (type +0x6C0), Unit 0x7F5C70 and Aircraft 0x7E22A4 (+0x6C4), '
                     'Building 0x7E3EBC (+0x520); type vtables 0x7EB610, 0x7F6218, '
                     '0x7E2868, 0x7E4570. Owner +0x21C; type Cost +0x610, '
                     'BuildTimeMultiplier +0x608, Naval +0xCCE, building BuildCat +0xE08, '
                     'wall +0x1571; house country +0x34, power +0x53A4/+0x53A8, factory '
                     'counters +0x5378..+0x5388 (unread counters hold 97); country '
                     'multipliers +0x134..+0x144 (unread slots hold 64.0); Rules at '
                     '[0x8871E0] with +0x570..+0x57C, +0x758 and +0x1748. Default native '
                     'FPCW 0x0E7F.',
                     'Inputs are fixture values chosen to cover stock rules and the '
                     'branches (power, clamps, counts, walls); they are not a claim that '
                     'every combination occurs in retail play. Double fields hold values '
                     'as INIClass::ReadDouble 0x5283D0 stores them (a %f scan widened '
                     'to double); three BuildSpeed values are doubles only a percent '
                     'value (x 0.01) produces.'],
        substitutions=[], entry_points={'time_to_build': 0x6F47A0}))
