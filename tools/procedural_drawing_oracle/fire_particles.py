"""Original Fire particle system: AI_Fire 0x62F9A0 and its particles.

Retail FireStreamSys/FireStream come from the physical rules layers through
SparkBirth's reader VM, and the Sonic loop of WaveClass's table setup
(0x75F07C..0x75F0C7 in 0x75F020) fills the table AI_Fire reads. The whole ParticleSystem constructor 0x62DC50 runs as
FireAt 0x6FF19E calls it: launch coordinate, target, owning Techno, the
0xB0EA90 coordinate and no House. Live Logic then runs System AI 0x62FD60 ->
AI_Fire (particle AI 0x62CE40 -> 0x62CB10, Move_Dispatch 0x62D5E0, the
destroy walk, the owner sweep and SpawnParticleWithInsert 0x62E4C0) until the
system retires.

The owner, its target and the cell objects are stub Technos. Each visit
scripts what AI_Fire reads from the owner: IsAlive +0x90, Target +0x2B4,
Location +0x9C, the answers of IsRotating 0x4C9480 and Current 0x4C93D0 on
+0x388 (both replaced for the owner's facing only), the target's GetCoords
(vt+0x48, its Location) and GetFLH (vt+0xB0). Stubs linked into the flat
cells record their ReceiveDamage (vt+0x16C) and zero the damage word instead
of running it.
"""
from pathlib import Path
import struct
import sys

from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP

from tools import native_oracle as native
from tools.procedural_drawing_oracle.electric_bolt import READER_SP, SparkBirth, ints, signed, words

SYSTEM_CTOR, SONIC_LOOP = 0x62DC50, (0x75F07C, 0x75F0C7)
IS_ROTATING, CURRENT = 0x4C9480, 0x4C93D0
FIRST_FRAME, VISITS = 36, 120
SNAPSHOTS = (3, 12, 31, 60)
# SparkBirth's flat cells span x 30..48 (cell_x_min) and y 46..52.
CELL_X_MIN = 30


def center(x, y):
    return [x * 256 + 128, y * 256 + 128, 0]


def sweep_script(visit):
    # The body turns through east (0x4000) from visit 4 to 28.
    return dict(alive=True, has_target=True, rotating=4 <= visit <= 28,
                facing=(0x3C00 + 0x60 * visit) & 0xFFFF)


def still_script(visit):
    # No target, then a target it does not turn to, then dead.
    return dict(alive=visit < 15, has_target=visit >= 9, rotating=False, facing=0)


CASES = [
    dict(name='sweep', script=sweep_script, owner=center(33, 49), target=center(37, 49),
         flh=[8576 + 96, 12672 - 24, 144], objects=[
             dict(name='ahead', location=center(35, 49)),
             dict(name='wrecked', location=center(36, 49), strength=0),
             dict(name='south', location=center(37, 50)),
             dict(name='dead', location=center(38, 49), alive=False),
             dict(name='far', location=center(39, 49))]),
    dict(name='still_then_dies', script=still_script, owner=center(33, 52), target=center(36, 49),
         flh=[8576 + 64, 13440 - 64, 120], objects=[
             dict(name='diagonal', location=center(35, 50)),
             dict(name='at_target', location=center(36, 49))]),
]


class FireBirth(SparkBirth):
    def __init__(self, case):
        super().__init__(dict(name=case['name'], frame=FIRST_FRAME, detail=2,
                              cell_x_min=CELL_X_MIN), None,
                         system_name='FireStreamSys', particle_name='FireStream')
        self.runtime = False
        self.u.reg_write(UC_X86_REG_ESP, READER_SP)
        native.run_checked(self.u, *SONIC_LOOP, count=200000)
        self.stubs, self.names, self.hits, self.reads = {}, {}, [], {}
        self.unit_vtable = self.vtable()
        self.owner = None
        self.runtime = True

    def hook(self, u, pc, size, data):
        if self.runtime:
            if pc in self.stubs:
                self.stubs[pc]()
                return
            if pc in (IS_ROTATING, CURRENT) and self.owner is not None \
                    and u.reg_read(UC_X86_REG_ECX) == self.owner + 0x388:
                self.facing(pc)
                return
        super().hook(u, pc, size, data)

    def stub(self, handler):
        address = self.alloc(16)
        self.stubs[address] = handler
        return address

    def vtable(self):
        def unexpected():
            raise AssertionError(('unexpected virtual call', hex(self.read32(self.u.reg_read(UC_X86_REG_ESP)))))
        table = self.alloc(0x400)
        self.u.mem_write(table, words(*([self.stub(unexpected)] * 0x100)))
        self.u.mem_write(table + 0x2C, words(self.stub(lambda: self.ret(1, 0))))  # Unit
        self.u.mem_write(table + 0x48, words(self.stub(self.get_coords)))
        self.u.mem_write(table + 0xB0, words(self.stub(self.get_flh)))
        self.u.mem_write(table + 0x16C, words(self.stub(self.receive_damage)))
        return table

    def get_coords(self):
        sp, this = self.u.reg_read(UC_X86_REG_ESP), self.u.reg_read(UC_X86_REG_ECX)
        out = self.read32(sp + 4)
        self.u.mem_write(out, bytes(self.u.mem_read(this + 0x9C, 12)))
        self.ret(out, 4)

    def get_flh(self):
        sp, this = self.u.reg_read(UC_X86_REG_ESP), self.u.reg_read(UC_X86_REG_ECX)
        assert this == self.owner
        out = self.read32(sp + 4)
        self.reads['flh'] = dict(weapon=signed(self.read32(sp + 8)), base=ints(self.u, sp + 12, 3))
        self.u.mem_write(out, words(*self.fire['flh']))
        self.ret(out, 0x14)

    def facing(self, pc):
        script = self.script
        if pc == IS_ROTATING:
            self.reads['rotating'] = script['rotating']
            self.ret(int(script['rotating']), 0)
            return
        out = self.read32(self.u.reg_read(UC_X86_REG_ESP) + 4)
        self.reads['facing'] = script['facing']
        self.u.mem_write(out, struct.pack('<H', script['facing']))
        self.ret(out, 4)

    def receive_damage(self):
        sp, this = self.u.reg_read(UC_X86_REG_ESP), self.u.reg_read(UC_X86_REG_ECX)
        args = [self.read32(sp + 4 + i * 4) for i in range(7)]
        assert args[2] == self.warhead and args[3:] == [0, 0, 0, 0], args
        self.hits.append([self.visit, self.names[this], signed(self.read32(args[0])), signed(args[1])])
        # A receiver may rewrite the damage; the walk reloads it per object.
        self.u.mem_write(args[0], words(0))
        self.ret(0, 0x1C)

    def techno(self, name, location, strength=100, alive=True, placed=True):
        pointer = self.alloc(0x400)
        self.u.mem_write(pointer, words(self.unit_vtable))
        self.u.mem_write(pointer + 0x14, bytes([3]))  # Techno | Object
        self.u.mem_write(pointer + 0x6C, words(strength))
        self.u.mem_write(pointer + 0x90, bytes([int(alive)]))
        self.u.mem_write(pointer + 0x9C, words(*location))
        if placed:
            link = self.cells[location[0] >> 8, location[1] >> 8] + 0xE4
            while self.read32(link):
                link = self.read32(link) + 0x30
            self.u.mem_write(link, words(pointer))
        self.names[pointer] = name
        return pointer

    def particle(self, p):
        u = self.u
        return dict(native_id=signed(self.read32(p + 0x10)), location=ints(u, p + 0x9C, 3),
            step=ints(u, p + 0x100, 3), direction_bits=[self.read32(p + 0x10C + i * 4) for i in range(3)],
            speed_bits=self.read32(p + 0xE4),
            remaining=struct.unpack('<H', u.mem_read(p + 0x128, 2))[0],
            damage_counter=struct.unpack('<h', u.mem_read(p + 0x12A, 2))[0],
            advance=u.mem_read(p + 0x12C, 1)[0], state=u.mem_read(p + 0x12E, 1)[0],
            translucency=u.mem_read(p + 0x12F, 1)[0], marked=bool(u.mem_read(p + 0x131, 1)[0]))

    def live(self, system):
        data, count = self.read32(system + 0xC0), self.read32(system + 0xCC)
        return [self.read32(data + i * 4) for i in range(count)]

    def run(self, case):
        self.fire = case
        self.warhead = self.read32(self.particle_type + 0x2E4)
        assert self.warhead
        self.owner = self.techno('owner', case['owner'])
        target = self.techno('target', case['target'], placed=False)
        for o in case['objects']:
            self.techno(o['name'], o['location'], o.get('strength', 100), o.get('alive', True))
        self.u.mem_write(self.owner + 0x2B4, words(target))
        launch = self.alloc(12)
        self.u.mem_write(launch, words(*case['flh']))
        system = self.alloc(0x100)
        main = self.trace.states()['main']
        initial = self.trace.states()['scenario']
        self.visit = 0
        self.invoke(SYSTEM_CTOR, system, (self.system_type, launch, target, self.owner, 0xB0EA90, 0))
        # Unicorn stops before the RET hook; observe the native return here.
        self.observe_return(self.u.reg_read(UC_X86_REG_EIP), self.u.reg_read(UC_X86_REG_ESP))
        visits, counts, swept, systems, flh_reads = {}, [], [], [], []
        for visit in range(1, VISITS + 1):
            self.visit, self.script, self.reads = visit, case['script'](visit), {}
            self.u.mem_write(self.owner + 0x90, bytes([int(self.script['alive'])]))
            self.u.mem_write(self.owner + 0x2B4, words(target if self.script['has_target'] else 0))
            self.logic_visit(f'visit{visit}', FIRST_FRAME + visit)
            alive = self.read32(0x87F788) and system in [
                self.read32(self.read32(0x87F77C) + i * 4) for i in range(self.read32(0x87F788))]
            counts.append(len(self.live(system)) if alive else -1)
            swept.append('facing' in self.reads)
            assert ('flh' in self.reads) == swept[-1], self.reads
            if 'flh' in self.reads:
                flh_reads.append(self.reads['flh'])
            if alive:
                systems.append([*ints(self.u, system + 0x9C, 3), *ints(self.u, system + 0xD4, 3),
                                int(self.u.mem_read(system + 0xF8, 1)[0])])
            if visit in SNAPSHOTS:
                visits[str(visit)] = [self.particle(p) for p in self.live(system)] if alive else []
        assert counts[-1] == -1, 'the system retires within the visits'
        assert self.trace.states()['main'] == main
        assert not self.trace.pending and not self.returns
        assert all(read == dict(weapon=0, base=[0, 0, 0]) for read in flh_reads)
        scripts = [case['script'](visit) for visit in range(1, VISITS + 1)]
        return dict(name=case['name'], owner=case['owner'], target=case['target'], flh=case['flh'],
            objects=case['objects'], script=scripts,
            initial_scenario_rng=initial, final_scenario_rng=self.trace.states()['scenario'],
            swept=swept, systems=systems, live_counts=counts, particles_after_visit=visits,
            hits=self.hits)


def generate():
    return dict(first_frame=FIRST_FRAME, visits=VISITS,
                cases=[FireBirth(case).run(case) for case in CASES])


def metadata():
    out = native.provenance(
        scope='Original FireAt-built Fire particle system: constructor, AI_Fire sweep/wobble/insert, Fire particle AI, move and damage walk to retirement',
        assumptions=[
            'Physical RULESMD/LANGRULE/MPBattleMD layers through SparkBirth read retail FireStreamSys and FireStream with the original type readers 6440A0/6442D0 and 644BE0/644F50; the Sonic loop 75F07C..75F0C7 of 75F020 fills the wave table AI_Fire reads.',
            'Whole ParticleSystem 62DC50 runs with the FireAt 6FF19E arguments. Live Logic 55B5FF..55B61B runs every visit; System AI 62FD60, AI_Fire 62F9A0, particle AI 62CB10, Move_Dispatch 62D5E0, SpawnParticleWithInsert 62E4C0, particle constructor 62B5E0, Sqrt_Approx and the trig tables execute unchanged on the flat-cell fixture.',
            'Scenario RNG starts from the SparkBirth seed; Main is asserted unchanged.',
        ], substitutions=[
            'Reuse SparkBirth allocator, cached INI and flat-cell world, with cells from x 30.',
            'Owner, target and cell objects are stub Technos (WhatAmI Unit, Strength +0x6C, IsAlive +0x90, Location +0x9C, NextObject +0x30). GetCoords returns Location; GetFLH returns the case FLH; ReceiveDamage records its arguments and zeroes the damage word.',
            'IsRotating 0x4C9480 and Current 0x4C93D0 return the case script for the owner facing only; each visit also scripts the owner IsAlive and Target before Logic.',
        ], entry_points=dict(system_ctor=SYSTEM_CTOR, system_ai=0x62FD60, fire_ai=0x62F9A0,
            particle_ai=0x62CB10, move=0x62D5E0, insert=0x62E4C0, particle_ctor=0x62B5E0,
            sonic_table_loop=SONIC_LOOP[0], logic=0x55B5FF))
    out['reproduction_commands'] = dict(
        working_directory='repository root with the configured retail installation',
        check='python -m tools.procedural_drawing_oracle.fire_particles --check',
        write='python -m tools.procedural_drawing_oracle.fire_particles --write')
    return out


if __name__ == '__main__':
    sources = {name: Path(module.__file__) for name, module in sorted(sys.modules.items())
               if name.startswith('tools.') and getattr(module, '__file__', None)
               and str(module.__file__).endswith('.py')}
    sources['producer'] = Path(__file__)
    native.finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata,
                          argv=sys.argv[1:], source_paths=sources)
