"""Original Railgun particle system and line damage.

Retail SmallRailgunSys/SmallRailgunPart come from the physical rules layers
through SparkBirth's existing reader VM. The whole ParticleSystem constructor
0x62DC50 runs as FireAt 0x6FF223..0x6FF26E calls it: no target object, owner
or House. Live Logic then runs System AI 0x62FD60 -> AI_Railgun 0x62F230
(spiral lay, LaserDraw 0x54FE60, forward particle AI 0x62CE40 -> 0x62C3A0,
backward destroy walk) until every particle has expired.

Line damage runs the whole TechnoClass 0x70C690 on the same flat cells. Its
objects are stub Technos (WhatAmI Unit or Building, Location +0x9C, NextObject
+0x30) linked into the cell lists +0xE4; GetCoords (vt+0x48) returns the
Location and ReceiveDamage (vt+0x16C) records its arguments instead of
running. Object positions are integer points chosen to sit just inside or
outside RailgunDamageRadius, so the native float kernel decides each one.
"""
from pathlib import Path
import itertools
import math
import struct
import sys

from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP

from tools import native_oracle as native
from tools.procedural_drawing_oracle.electric_bolt import (
    SparkBirth, ints, prepare_spark_capacity, signed, words)

LASERS, LASER_CTOR = 0xABC878, 0x54FE60
LINE_DAMAGE = 0x70C690
VISITS = 85
# Flat fixture cells cover x 42..48, y 46..52 (leptons 10752..12543, 11776..13567).
CASES = [
    dict(name='diagonal_descending', source=[10900, 11900, 400], target=[11250, 12250, 96]),
    dict(name='westward_rising', source=[12300, 13300, 40], target=[12000, 13050, 300]),
    dict(name='vertical', source=[11500, 12500, 100], target=[11500, 12500, 400]),
    dict(name='zero_length', source=[11500, 12500, 100], target=[11500, 12500, 100]),
]


class RailgunBirth(SparkBirth):
    def __init__(self, case):
        super().__init__(dict(name=case['name'], frame=36, detail=2), None,
                         system_name='SmallRailgunSys', particle_name='SmallRailgunPart')
        self.runtime = False
        prepare_spark_capacity(self, (LASERS,))
        self.laser_args = []
        self.runtime = True

    def hook(self, u, pc, size, data):
        if self.runtime and pc == LASER_CTOR:
            sp = self.u.reg_read(UC_X86_REG_ESP)
            self.laser_args.append([self.read32(sp + 4 + i * 4) for i in range(16)])
        super().hook(u, pc, size, data)

    def particle(self, p):
        u = self.u
        return dict(native_id=signed(self.read32(p + 0x10)), location=ints(u, p + 0x9C, 3),
            direction_bits=[self.read32(p + 0x10C + i * 4) for i in range(3)],
            speed_bits=self.read32(p + 0xE4),
            position_bits=[self.read32(p + 0x118 + i * 4) for i in range(3)],
            color=list(u.mem_read(p + 0xB0, 3)), color_index=signed(self.read32(p + 0xB4)),
            color_accumulator_bits=struct.unpack('<Q', u.mem_read(p + 0xB8, 8))[0],
            remaining=struct.unpack('<H', u.mem_read(p + 0x128, 2))[0])

    def live(self, system):
        data, count = self.read32(system + 0xC0), self.read32(system + 0xCC)
        return [self.read32(data + i * 4) for i in range(count)]

    def run(self, case):
        coords, target = self.alloc(12), self.alloc(12)
        self.u.mem_write(coords, words(*case['source']))
        self.u.mem_write(target, words(*case['target']))
        system = self.alloc(0x100)
        main = self.trace.states()['main']
        initial = self.trace.states()['scenario']
        self.invoke(0x62DC50, system, (self.system_type, coords, 0, 0, target, 0))
        # Unicorn stops before the RET hook; observe the native return here.
        self.observe_return(self.u.reg_read(UC_X86_REG_EIP), self.u.reg_read(UC_X86_REG_ESP))
        visits, counts = {}, []
        for visit in range(1, VISITS + 1):
            self.logic_visit(f'visit{visit}', 36 + visit)
            alive = self.read32(0x87F788) and system in [
                self.read32(self.read32(0x87F77C) + i * 4) for i in range(self.read32(0x87F788))]
            counts.append(len(self.live(system)) if alive else -1)
            if visit in (1, 30):
                visits[str(visit)] = [self.particle(p) for p in self.live(system)]
        assert self.trace.states()['main'] == main
        assert not self.trace.pending and not self.returns
        return dict(name=case['name'], source=case['source'], target=case['target'],
            initial_scenario_rng=initial, final_scenario_rng=self.trace.states()['scenario'],
            laser_args=self.laser_args, particles_after_visit=visits, live_counts=counts)


def line_cells(source, target):
    """The cells the 50 Lerp steps visit, approximately (fixture choice only)."""
    cells = []
    for step in range(50):
        point = [s + (t - s) * step / 49 for s, t in zip(source, target)]
        cell = (int(point[0]) >> 8, int(point[1]) >> 8)
        if cell not in cells:
            cells.append(cell)
    return cells


def edge_points(source, target, fractions, direction, below, above, used):
    """Integer points near the line whose exact distance is just below and
    just above 128, each in its own cell the line visits (not the source's)."""
    d = [t - s for s, t in zip(source, target)]
    length = math.sqrt(sum(a * a for a in d))
    unit = [a / length for a in d]
    cells = line_cells(source, target)[1:]
    picks = {}
    for fraction, offset in itertools.product(fractions, itertools.product(range(-6, 7), repeat=3)):
        base = [s + a * fraction for s, a in zip(source, d)]
        point = [round(b + 128 * u + o) for b, u, o in zip(base, direction, offset)]
        cell = (point[0] >> 8, point[1] >> 8)
        if cell not in cells or cell in used:
            continue
        a = [p - s for p, s in zip(point, source)]
        along = sum(x * y for x, y in zip(a, unit))
        distance = math.sqrt(max(sum(x * x for x in a) - along * along, 0.0))
        for name, low, high in ((below, 127.99, 128.0), (above, 128.0, 128.01)):
            if low < distance < high and name not in picks:
                picks[name] = point
                used.add(cell)
                break
    assert set(picks) == {below, above}, picks
    return [dict(name=below, kind='unit', location=picks[below]),
            dict(name=above, kind='unit', location=picks[above])]


def perpendicular(d, axis):
    length = math.sqrt(sum(a * a for a in d))
    unit = [a / length for a in d]
    dot = sum(a * b for a, b in zip(axis, unit))
    u = [a - dot * b for a, b in zip(axis, unit)]
    norm = math.sqrt(sum(a * a for a in u))
    return [a / norm for a in u]


def line_cases():
    diagonal = ([10900, 11900, 500], [12300, 13300, 100])
    d = [t - s for s, t in zip(*diagonal)]
    up = perpendicular(d, [0, 0, 1])
    side = perpendicular(d, [up[1] * d[2] - up[2] * d[1], up[2] * d[0] - up[0] * d[2],
                             up[0] * d[1] - up[1] * d[0]])
    # Production holds one vehicle per cell, so each object takes its own.
    used = {(diagonal[0][0] >> 8, diagonal[0][1] >> 8), (diagonal[1][0] >> 8, diagonal[1][1] >> 8)}
    fractions = [step / 20 for step in range(2, 19)]
    return [
        dict(name='along_x', source=[10880, 12672, 200], target='target', objects=[
            dict(name='source_cell', kind='unit', location=[10900, 12700, 200]),
            dict(name='inside_127', kind='unit', location=[11392, 12799, 200]),
            dict(name='edge_128', kind='unit', location=[11648, 12544, 200]),
            dict(name='above_129', kind='unit', location=[11904, 12672, 329]),
            dict(name='below_127', kind='unit', location=[11040, 12672, 73]),
            dict(name='building', kind='building', location=[12160, 12672, 900]),
            dict(name='target', kind='unit', location=[12416, 12672, 200])]),
        dict(name='diagonal_edges', source=diagonal[0], target='target', objects=[
            *edge_points(*diagonal, fractions, up, 'up_inside', 'up_outside', used),
            *edge_points(*diagonal, fractions, side, 'side_inside', 'side_outside', used),
            dict(name='target', kind='unit', location=diagonal[1])]),
        dict(name='ground_stop', source=[10880, 12928, 150], target='target', objects=[
            dict(name='before_ground', kind='unit', location=[11136, 12948, 120]),
            dict(name='after_ground', kind='unit', location=[12160, 12928, -110]),
            dict(name='target', kind='unit', location=[12416, 12928, -150])]),
        dict(name='unplaced_target', source=[10880, 13184, 200], target='target', objects=[
            dict(name='on_line', kind='unit', location=[11648, 13184, 200]),
            dict(name='target', kind='unit', location=[12416, 13184, 200], placed=False)]),
    ]


class LineDamage(RailgunBirth):
    """Whole TechnoClass 0x70C690 over stub Technos in the flat cells."""
    def __init__(self, case):
        super().__init__(case)
        self.stubs, self.hits, self.names = {}, [], {}
        self.vtables = {kind: self.vtable(code) for kind, code in (('unit', 1), ('building', 6))}

    def hook(self, u, pc, size, data):
        if self.runtime and pc in self.stubs:
            self.stubs[pc]()
            return
        super().hook(u, pc, size, data)

    def stub(self, handler):
        address = self.alloc(16)
        self.stubs[address] = handler
        return address

    def vtable(self, what_am_i):
        def unexpected():
            raise AssertionError(('unexpected virtual call', hex(self.read32(self.u.reg_read(UC_X86_REG_ESP)))))
        table = self.alloc(0x400)
        self.u.mem_write(table, words(*([self.stub(unexpected)] * 0x100)))
        self.u.mem_write(table + 0x2C, words(self.stub(lambda: self.ret(what_am_i, 0))))
        self.u.mem_write(table + 0x48, words(self.stub(self.get_coords)))
        self.u.mem_write(table + 0x16C, words(self.stub(self.receive_damage)))
        return table

    def get_coords(self):
        sp, this = self.u.reg_read(UC_X86_REG_ESP), self.u.reg_read(UC_X86_REG_ECX)
        out = self.read32(sp + 4)
        self.u.mem_write(out, bytes(self.u.mem_read(this + 0x9C, 12)))
        self.ret(out, 4)

    def receive_damage(self):
        sp, this = self.u.reg_read(UC_X86_REG_ESP), self.u.reg_read(UC_X86_REG_ECX)
        args = [self.read32(sp + 4 + i * 4) for i in range(7)]
        self.hits.append(dict(object=self.names[this], damage=signed(self.read32(args[0])),
            distance=signed(args[1]), weapon_warhead=args[2] == self.warhead,
            firer=self.names.get(args[3]), ignore_defenses=args[4], arg6=args[5], house=args[6]))
        self.ret(0, 0x1C)

    def techno(self, name, kind, location, placed=True):
        pointer = self.alloc(0x200)
        self.u.mem_write(pointer, words(self.vtables[kind]))
        self.u.mem_write(pointer + 0x14, bytes([3]))  # Techno | Object
        self.u.mem_write(pointer + 0x9C, words(*location))
        if placed:
            link = self.cells[location[0] >> 8, location[1] >> 8] + 0xE4
            while self.read32(link):
                link = self.read32(link) + 0x30
            self.u.mem_write(link, words(pointer))
        self.names[pointer] = name
        return pointer

    def run_line(self, case):
        self.warhead = self.alloc(16)
        weapon = self.alloc(0x200)
        self.u.mem_write(weapon + 0x98, words(150))
        self.u.mem_write(weapon + 0xAC, words(self.warhead))
        firer = self.techno('firer', 'unit', case['source'], placed=False)
        objects = {o['name']: self.techno(o['name'], o['kind'], o['location'], o.get('placed', True))
                   for o in case['objects']}
        out, source = self.alloc(12), self.alloc(12)
        self.u.mem_write(source, words(*case['source']))
        before = self.trace.states()
        assert self.invoke(LINE_DAMAGE, firer, (out, source, objects[case['target']], weapon)) == out
        assert not self.trace.pending
        return dict(case, radius=signed(self.read32(self.rules + 0x494)),
            returned=ints(self.u, out, 3), hits=self.hits, rng_unchanged=before == self.trace.states())


def generate():
    return dict(visits=VISITS, cases=[RailgunBirth(case).run(case) for case in CASES],
                line_cases=[LineDamage(case).run_line(case) for case in line_cases()])


def metadata():
    out = native.provenance(
        scope='Original Railgun line damage, ParticleSystem constructor, AI_Railgun lay/laser and Railgun particle AI to expiry',
        assumptions=[
            'Physical RULESMD/LANGRULE/MPBattleMD layers through SparkBirth read retail SmallRailgunSys and SmallRailgunPart with the original type readers 6440A0/6442D0 and 644BE0/644F50.',
            'Whole ParticleSystem 62DC50 runs with no target object, owner or House and the target coordinate FireAt passes. Live Logic 55B5FF..55B61B runs every visit; the system AI, laser constructor, particle constructor 62B5E0 and particle AI 62C3A0 execute unchanged on the flat-cell fixture.',
            'Scenario RNG starts from the SparkBirth seed; Main is asserted unchanged. Each laser constructor call records its 16 raw argument dwords.',
        ], substitutions=[
            'Reuse SparkBirth allocator, cached INI and flat-cell world; prepare_spark_capacity supplies the empty LaserDraw vector 0xABC878.',
            'Line damage replaces only the three object virtuals (WhatAmI, GetCoords, ReceiveDamage) with recording stubs; cell lookup, ground height, Lerp, Sqrt_Approx and the hit list execute unchanged.',
            'Line damage runs whole 70C690 with a stub firer, weapon (AmbientDamage 150) and stub Technos linked into the flat cells; GetCoords returns Location and ReceiveDamage records its arguments without running. Fixture positions come from an exact-distance search, never from expected outputs.',
        ], entry_points=dict(line_damage=LINE_DAMAGE, system_ctor=0x62DC50, system_ai=0x62FD60, railgun_ai=0x62F230,
            laser_ctor=LASER_CTOR, particle_ctor=0x62B5E0, particle_ai=0x62C3A0, logic=0x55B5FF))
    out['reproduction_commands'] = dict(
        working_directory='repository root with the configured retail installation',
        check='python -m tools.procedural_drawing_oracle.railgun --check',
        write='python -m tools.procedural_drawing_oracle.railgun --write')
    return out


if __name__ == '__main__':
    sources = {name: Path(module.__file__) for name, module in sorted(sys.modules.items())
               if name.startswith('tools.') and getattr(module, '__file__', None)
               and str(module.__file__).endswith('.py')}
    sources['producer'] = Path(__file__)
    native.finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata,
                          argv=sys.argv[1:], source_paths=sources)
