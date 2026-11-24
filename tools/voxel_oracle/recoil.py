"""Original Techno recoil readers, constructor, firing and ordered AI updates.

Uses the shared native INI cache/CRT and checked executable owners. Physical
RULESMD and mode bytes are supplied by VERA20K_RECOIL_ASSETS; the map by
VERA20K_RECOIL_MAP. No Rust field values initialize native reference state.
"""
import hashlib
import os
from pathlib import Path
import struct

from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FPCW,
)
from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, run_checked
from tools.projectile_oracle.bridge_render_inputs import lexical
from tools.rules_oracle.bridge_anim_inputs import Reader
from tools.spatial_oracle.building_body_rules import INI, SP, TYPE, dwords

ACTOR = TYPE + 0x6000
TYPE_CTOR = (0x711412, 0x711454)
READER = (0x715269, 0x7153DA)
ACTOR_CTOR = (0x6F2F61, 0x6F2FA9)
INIT = (0x6F4277, 0x6F42E3)
FIRE = (0x6FF0B7, 0x6FF15B)
AI = (0x6FA4D1, 0x6FA4FB)


class Machine(Reader):
    def __init__(self, name='GTGCAN'):
        super().__init__(Path(__file__).with_suffix('.no-assets'), {})
        self.u.mem_write(TYPE, bytes(0x2000))
        self.u.mem_write(TYPE + 0x24, name.encode('ascii') + b'\0')
        self.regs(esi=TYPE, eax=1, ecx=2, ebx=0)
        run_checked(self.u, *TYPE_CTOR)
        self.u.mem_write(ACTOR, bytes(0x800))
        self.u.mem_write(ACTOR, dwords(0x7E3EBC))
        self.u.mem_write(ACTOR + 0x520, dwords(TYPE))
        self.regs(esi=ACTOR, eax=2, edi=1, ebx=0)
        run_checked(self.u, *ACTOR_CTOR)
        self.name = name

    def hook(self, u, pc, size, user):
        if pc in (0x65C780, 0x65C7E0):
            raise AssertionError('unexpected Scenario RNG in recoil component')
        super().hook(u, pc, size, user)

    def regs(self, **values):
        registers = dict(eax=UC_X86_REG_EAX, ebp=UC_X86_REG_EBP, ebx=UC_X86_REG_EBX,
                         ecx=UC_X86_REG_ECX, edi=UC_X86_REG_EDI,
                         esi=UC_X86_REG_ESI, esp=UC_X86_REG_ESP)
        for key, value in values.items():
            self.u.reg_write(registers[key], value & 0xFFFFFFFF)

    def config(self):
        return dict(enabled=bool(self.u.mem_read(TYPE + 0xCA2, 1)[0]),
                    turret=list(struct.unpack('<4i', self.u.mem_read(TYPE + 0xCA4, 16))),
                    barrel=list(struct.unpack('<4i', self.u.mem_read(TYPE + 0xCB8, 16))))

    def actor_state(self):
        result = []
        for offset in (0x3D8, 0x3F8):
            data = bytes(self.u.mem_read(ACTOR + offset, 32))
            result.append(dict(config=list(struct.unpack('<4i', data[:16])),
                               step_bits=struct.unpack_from('<I', data, 16)[0],
                               travel_bits=struct.unpack_from('<I', data, 20)[0],
                               state=struct.unpack_from('<i', data, 24)[0],
                               left=struct.unpack_from('<i', data, 28)[0]))
        return result

    def read_layer(self, sections):
        self.make_ini(sections)
        before = self.config()
        self.u.mem_write(SP, dwords(RET_MAGIC, INI))
        self.regs(esp=SP, ecx=TYPE)
        stop = run_checked(self.u, 0x410A60, (0x410A8C, 0x410B7D),
                           required_addresses=(0x526810,))
        admitted = stop == 0x410A8C
        if admitted:
            self.u.mem_write(SP + 0x380, dwords(INI))
            self.regs(esp=SP, ebp=TYPE, edi=INI, ebx=TYPE + 0x24)
            run_checked(self.u, *READER, required_addresses=(0x5295F0, 0x5276D0,
                                                            0x717A50, 0x717A80, 0x717AB0))
            assert self.u.reg_read(UC_X86_REG_ESP) == SP
        return dict(sections=sections, admitted=admitted, before=before, after=self.config())

    def initialize(self):
        self.regs(esp=SP, esi=ACTOR)
        run_checked(self.u, *INIT)

    def fire(self, has_turret=True):
        self.u.mem_write(TYPE + 0xCA1, bytes([has_turret]))
        self.regs(esp=SP, esi=ACTOR)
        run_checked(self.u, *FIRE, required_addresses=(0x4527D0,))

    def tick(self):
        self.regs(esp=SP, esi=ACTOR)
        run_checked(self.u, *AI)


def generate():
    constructor = Machine().config()
    cases = [
        {}, {'TurretRecoil': 'yes'},
        {'TurretRecoil': 'no', 'BarrelTravel': '8'},
        {'TurretRecoil': 'yes', 'TurretTravel': '0', 'BarrelTravel': '8',
         'BarrelCompressFrames': '3', 'BarrelHoldFrames': '2', 'BarrelRecoverFrames': '20'},
        {'TurretRecoil': 'yes', 'TurretTravel': '-7', 'TurretCompressFrames': '3',
         'TurretHoldFrames': '-4', 'TurretRecoverFrames': '5'},
        {'TurretRecoil': 'yes', 'TurretCompressFrames': '0', 'TurretRecoverFrames': '-1',
         'BarrelCompressFrames': '-2147483648', 'BarrelHoldFrames': '0'},
        {'TurretRecoil': 'yes', 'TurretTravel': '17h', 'BarrelTravel': 'junk',
         'TurretCompressFrames': '3 trailing', 'TurretRecoverFrames': '3'},
        {'turretrecoil': 'yes', 'barreltravel': '99'},
    ]
    readers = []
    histories = []
    for index, keys in enumerate(cases):
        m = Machine()
        readers.append(dict(name=f'control-{index}', **m.read_layer({'GTGCAN': keys})))
        m.initialize()
        initial = m.actor_state()
        m.fire()
        frames = [m.actor_state()]
        for _ in range(35):
            m.tick()
            frames.append(m.actor_state())
        histories.append(dict(name=f'control-{index}', config=m.config(), initial=initial,
                              refires=[], frames=frames))
    m = Machine()
    layers = [m.read_layer({'GTGCAN': {'TurretRecoil': 'yes', 'TurretTravel': '4',
                                     'BarrelTravel': '9'}}),
              m.read_layer({}), m.read_layer({'GTGCAN': {'TurretCompressFrames': '3'}}),
              m.read_layer({'GTGCAN': {'BarrelHoldFrames': '4'}})]
    # Refiring never resets displacement; the original inline producer only
    # changes state, duration and velocity. Exercise that retained state.
    m = Machine()
    m.read_layer({'GTGCAN': {'TurretRecoil': 'yes', 'TurretTravel': '0',
                            'BarrelTravel': '8', 'BarrelCompressFrames': '3',
                            'BarrelHoldFrames': '2', 'BarrelRecoverFrames': '20'}})
    m.initialize()
    initial = m.actor_state()
    m.fire()
    frames = [m.actor_state()]
    for tick in range(1, 41):
        m.tick()
        if tick in (2, 9):
            m.fire()
        frames.append(m.actor_state())
    histories.append(dict(name='refire', config=m.config(), initial=initial,
                          refires=[2, 9], frames=frames))
    gates = []
    for enabled in (False, True):
        for has_turret in (False, True):
            m = Machine()
            m.read_layer({'GTGCAN': {'TurretRecoil': 'yes' if enabled else 'no'}})
            m.initialize()
            before = m.actor_state()
            m.fire(has_turret)
            armed = m.actor_state()
            m.tick()
            gates.append(dict(enabled=enabled, has_turret=has_turret, before=before,
                              armed=armed, tick=m.actor_state()))
    root = Path(os.environ.get('VERA20K_RECOIL_ASSETS', 'target/asset/building-barrel/extract'))
    files = {p.name.lower(): p for p in root.glob('*') if p.is_file()}
    assert 'rulesmd.ini' in files, 'VERA20K_RECOIL_ASSETS needs original physical RULESMD.INI'
    physical = []
    for name in ('GTGCAN', 'CAEAST02', 'NASAM', 'YAGGUN'):
        m = Machine(name)
        selected = []
        for filename in ('rulesmd.ini', 'langrule.ini', 'mpbattlemd.ini'):
            if filename not in files:
                selected.append(dict(file=filename, absent=True))
                continue
            raw = files[filename].read_bytes()
            sections, _ = lexical(raw, {name})
            selected.append(dict(file=filename, bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(),
                                 **m.read_layer(sections)))
        path = Path(os.environ.get('VERA20K_RECOIL_MAP', str(Path(__file__).resolve().parents[1] / 'map_observation/examples/grand_cannon_barrel.map')))
        raw = path.read_bytes()
        sections, _ = lexical(raw, {name})
        selected.append(dict(file=path.name, bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(),
                             **m.read_layer(sections)))
        physical.append(dict(name=name, layers=selected, config=m.config()))
        m.initialize()
        initial = m.actor_state()
        m.fire()
        frames = [m.actor_state()]
        for _ in range(60):
            m.tick()
            frames.append(m.actor_state())
        histories.append(dict(name=name, config=m.config(), initial=initial,
                              refires=[], frames=frames))
    # The active FireAt body inlines the same arm helper used by the
    # EMPulseCannon path. Compare their effects without a second algorithm.
    arm_controls = []
    for travel, compress in ((0, 3), (8, 3), (-7, 3), (17, 0)):
        m = Machine()
        m.u.mem_write(ACTOR + 0x3D8, dwords(travel, compress, 5, 2))
        m.u.mem_write(ACTOR + 0x3EC, struct.pack('<f', 1.25))
        m.u.mem_write(TYPE + 0xCA2, bytes([1]))
        before = m.actor_state()
        m.fire()
        inline = m.actor_state()
        n = Machine()
        n.u.mem_write(ACTOR + 0x3D8, dwords(travel, compress, 5, 2))
        n.u.mem_write(ACTOR + 0x3EC, struct.pack('<f', 1.25))
        n.invoke(0x70ECE0, ACTOR + 0x3D8)
        helper = n.actor_state()[0]
        assert inline[0] == helper
        arm_controls.append(dict(travel=travel, compress=compress, before=before[0],
                                 inline=inline[0], helper=helper))
    # Execute the same original arithmetic under a deliberately non-retail
    # nearest-rounding control to bound the chosen Rust presentation residual.
    # This is not a Python recoil implementation or a replacement native golden.
    nearest = Machine('GTGCAN')
    for layer in physical[0]['layers']:
        if 'sections' in layer:
            nearest.read_layer(layer['sections'])
    nearest.initialize()
    nearest.u.reg_write(UC_X86_REG_FPCW, 0x027F)
    nearest.fire()
    nearest_frames = [nearest.actor_state()]
    for _ in range(60):
        nearest.tick()
        nearest_frames.append(nearest.actor_state())
    native_frames = next(r['frames'] for r in histories if r['name'] == 'GTGCAN')
    largest = 0.0
    for left, right in zip(native_frames, nearest_frames, strict=True):
        for native, alternate in zip(left, right, strict=True):
            assert (native['state'], native['left']) == (alternate['state'], alternate['left'])
            for key in ('step_bits', 'travel_bits'):
                a = struct.unpack('<f', dwords(native[key]))[0]
                b = struct.unpack('<f', dwords(alternate[key]))[0]
                largest = max(largest, abs(a-b))
    rounding = dict(name='GTGCAN', retail_fpcw='0x0E7F', alternate_fpcw='0x027F',
                    compared_updates=60, states_and_countdowns_equal=True,
                    maximum_step_or_travel_absolute_difference=largest,
                    samples=[dict(update=i, retail=native_frames[i], alternate=nearest_frames[i])
                             for i in (0, 3, 6, 45, 46, 60)])
    return dict(schema_version=1, constructor=constructor, readers=readers,
                layers=layers, gates=gates, histories=histories, physical=physical,
                arm_controls=arm_controls, rounding_comparison=rounding)


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Techno recoil retained readers, InitManagers copies, HasTurret/FireAt gating and ordered recoil AI updates',
        assumptions=[
            'Original type/object constructor slices with their established zero, one and two input registers; unused constructor words normalized to zero and excluded from inactive semantic claims.',
            'Original cached INI admission, ReadBool/ReadInt and duration-clamping setters execute. Physical lexical strings supply caches; archive/INI file IO and full pass scheduling are outside this corpus.',
            'Original Building vtable and type pointer provide GetType/HasTurret; no upgrades. FireAt prefix is supplied as an already successful bullet launch; AI prefix as an admitted Techno visit.',
            'Original full recoil arithmetic executes under native FPCW0x0E7F. The explicitly labelled rounding_comparison also executes the same original blocks with alternate0x027F to bound a presentation precision residual; it is not retail behavior.',
            'No random calls, clock timers, object detach or rendering execute in these selected state blocks. Each AI visit updates turret then barrel; physical rows include60visits and the inactive tail.',
        ], substitutions=['Existing Reader bump allocator, CRT/TLS and cached section/key preparation only.'],
        entry_points={'reader': READER[0], 'type_constructor': TYPE_CTOR[0],
                      'object_constructor': ACTOR_CTOR[0], 'init_managers': INIT[0],
                      'fire_at': FIRE[0], 'techno_ai': AI[0],
                      'arm': 0x70ECE0, 'update': 0x70ED10}),
        source_paths={'producer': Path(__file__),
            'native_ini_owner': Path('tools/rules_oracle/bridge_anim_inputs.py'),
            'native_list_owner': Path('tools/rules_oracle/bridge_anim_lists.py'),
            'lexical_input_owner': Path('tools/projectile_oracle/bridge_render_inputs.py')})
