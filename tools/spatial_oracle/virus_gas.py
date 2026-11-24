"""Original Virus Gas constructor, AI/Move and reverse child lifecycle controls.

Uses the existing DamageSmoke/SparkBirth runtime, retail readers and RNG observer.
The flat cell world and optional damage receiver sink are declared inputs, not a
native whole-match or combat-receiver comparison.
"""
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP

from tools import native_oracle as native
from tools.native_slope import initialize_slope_tables
from tools.spatial_oracle.building_prism import SP
from tools.procedural_drawing_oracle.electric_bolt import ints, words
from tools.spatial_oracle.building_prism import FRAME
from tools.spatial_oracle.damage_smoke import DamageSmoke, ORIGIN


class VirusGas(DamageSmoke):
    def __init__(self, case):
        self.damage_calls = []
        self.receiver = 0
        super().__init__(case, system_names=('GasCloudSys',),
                         particle_names=('VirusCloud1', 'VirusCloudD1'))
        # SparkBirth already executes ReadAudioVisual's Gravity reader. Carry
        # those physical inputs into the consumed Rust rules fixture as well.
        for layer, spark_layer in zip(self.smoke_layers, self.type_layers):
            audiovisual = spark_layer.get('sections', {}).get('AudioVisual')
            if audiovisual:
                layer['sections']['AudioVisual'] = audiovisual
        for name in self.particle_names:
            state = self.type_state()[name]
            if not state['image_present'] or state['image_frames'] != 20:
                raise ValueError(f'{name}: original TXGASG image admission failed')
        initialize_slope_tables(self.u, SP, native.RET_MAGIC)
        self.system_id = self.system('GasCloudSys')
        self.u.mem_write(0xA8ED78, words(self.system_id))

    def particle_state(self, pointer):
        row = super().particle_state(pointer)
        row['direction_bits'] = [self.bits(pointer + 0xCC + i * 4) for i in range(3)]
        row['damage_counter'] = struct.unpack('<h', self.u.mem_read(pointer + 0x12A, 2))[0]
        return row

    def hook(self, u, pc, size, data):
        # The original Gas body selects and calls this concrete Infantry
        # receiver. Its arguments are observed; the downstream receiver is a
        # declared sink, so these rows do not claim native HP/death behavior.
        if pc == 0x517FA0 and self.receiver and u.reg_read(UC_X86_REG_ECX) == self.receiver:
            sp = u.reg_read(UC_X86_REG_ESP)
            damage = ints(u, self.read32(sp + 4), 1)[0]
            self.damage_calls.append(dict(damage=damage, distance=ints(u, sp + 8, 1)[0],
                flags=ints(u, sp + 20, 2), source=self.read32(sp + 16),
                house=self.read32(sp + 28)))
            self.ret(0, 28)
            return
        super().hook(u, pc, size, data)

    def add_receiver(self, cell=(44, 48)):
        self.receiver = self.foot_owner('infantry', ORIGIN)
        self.u.mem_write(self.receiver + 0x6C, words(125))
        self.u.mem_write(self.cells[cell] + 0xE4, words(self.receiver))


def history(case):
    m = VirusGas(case)
    if case.get('bridge'):
        m.u.mem_write(m.cells[44, 48] + 0x140, words(0x100))
    if case.get('receiver'):
        m.add_receiver()
    for prior in case.get('parents', [{}]):
        p = m.particle(m.system_id, prior.get('type', 'VirusCloud1'),
                       coords=prior.get('coords', ORIGIN))
        if 'remaining_ec' in prior:
            m.u.mem_write(p + 0x128, struct.pack('<H', prior['remaining_ec']))
        if 'damage_counter' in prior:
            m.u.mem_write(p + 0x12A, struct.pack('<h', prior['damage_counter']))
        if 'drift' in prior:
            m.u.mem_write(p + 0xC0, words(*prior['drift']))
    m.begin(case.get('raw_state_words'))
    before = m.system_state(m.system_id)
    visits = []
    for frame in case.get('frames', [36, 37]):
        m.u.mem_write(FRAME, words(frame))
        m.invoke(0x62FD60, m.system_id)
        visits.append(dict(frame=frame, state=m.system_state(m.system_id),
                           damage_calls=list(m.damage_calls)))
    return dict(input=case, before=before, visits=visits, **m.result())


def constructors(count):
    m = VirusGas({})
    coord = m.alloc(12)
    m.u.mem_write(coord, words(*ORIGIN))
    m.begin([5, 7, 9])
    before = m.read32(m.scenario + 0x214)
    for _ in range(count):
        m.invoke(0x62E430, m.system_id, (m.types['VirusCloud1'], coord))
    return dict(count=count, cursor_before=before,
                cursor_after=m.read32(m.scenario + 0x214),
                state=m.system_state(m.system_id), **m.result())


def generate():
    cases = [
        dict(name='empty_gas_does_not_spawn', parents=[], frames=[36, 37],
             type_overrides={'GasCloudSys': {'Spawns': 'yes', 'SpawnCutoff': '0'}}),
        dict(name='gas_wind_even_and_odd', frames=[36, 37, 38], wind=3,
             type_overrides={'VirusCloud1': {'WindEffect': '5'}}, raw_state_words=[1, 1]),
        dict(name='bridge_crossing', frames=[36, 37], bridge=True, raw_state_words=[1],
             parents=[dict(coords=[ORIGIN[0], ORIGIN[1], 420])]),
        dict(name='even_no_jitter_then_odd_move', raw_state_words=[1]),
        dict(name='even_x_jitter', raw_state_words=[0, 0, 2]),
        dict(name='even_y_jitter', raw_state_words=[0, 1, 0]),
        dict(name='clamped_horizontal_drift', raw_state_words=[0, 0, 2],
             parents=[dict(drift=[2, -2, 0])]),
        dict(name='elevated_gravity_and_descent', frames=[36, 37, 38, 39],
             raw_state_words=[1, 1], parents=[dict(coords=[ORIGIN[0], ORIGIN[1], 200])]),
        dict(name='ground_damage_due', receiver=True, raw_state_words=[1],
             parents=[dict(damage_counter=1)]),
        dict(name='zero_damage_count_wraps', raw_state_words=[1],
             parents=[dict(damage_counter=0)]),
        dict(name='reverse_parent_replacement', frames=[36, 37], raw_state_words=[1] * 12,
             parents=[dict(remaining_ec=1), dict(remaining_ec=1)]),
        dict(name='particle_cap_does_not_limit_gas', frames=[36], raw_state_words=[1] * 12,
             type_overrides={'GasCloudSys': {'ParticleCap': '0'}},
             parents=[dict(remaining_ec=1), dict(remaining_ec=1)]),
        dict(name='short_lived_child_cleanup', frames=list(range(36, 40)),
             raw_state_words=[1] * 12,
             parents=[dict(type='VirusCloudD1', remaining_ec=1)]),
    ]
    rows = [history(case) for case in cases]
    m = VirusGas({})
    return dict(type_layers=m.smoke_layers,
                controls=rows, constructors=[constructors(1), constructors(3)])


if __name__ == '__main__':
    native.finish_vectors(generate, Path(__file__).with_suffix('.json'),
        provenance=lambda: native.provenance(
            scope='Original retail VirusCloud1/VirusCloudD1 constructor and complete Gas System AI, Particle AI/Move, child retirement and RNG histories in a supplied flat world.',
            assumptions=['Existing DamageSmoke flat cells, original type readers, original slope/CRT startup via native_slope.initialize_slope_tables, PC53/chop and native ID/Scenario prior-state inputs.',
                         'Physical TXGASG.SHP from RA2MD.MIX/CONQMD.MIX is admitted by the original type/image reader (20 frames); drawing is not executed.',
                         'Optional Infantry receiver has supplied Object construction, concrete vtable, location, Alive and Health.'],
            substitutions=['Existing shared archive/INI, allocator and OS transport only.',
                           'Infantry ReceiveDamage517FA0 records its original Gas caller arguments and returns zero; no native receiver HP/death claim.'],
            entry_points=dict(system=0x62FD60, gas_system=0x62E6D0,
                particle=0x62CE40, gas_ai=0x62BD50, move=0x62D2A0,
                spawn=0x62E430, constructor=0x62B5E0)))
