"""Original IFV turret readers, selected-weapon lifecycle, and draw decisions.

VERA20K_IFV_ASSETS selects a flat physical asset extract. No Rust scalar result
initializes these native comparisons. --write publishes; --check reproduces.
Native executable IO, rasterization and the complete temporal rearm chain are
outside this component corpus; the sidecar declares every supplied boundary.
"""
import hashlib
import os
from pathlib import Path
import struct
import sys

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI,
    UC_X86_REG_ESP, UC_X86_REG_FPCW,
)
from tools.native_oracle import (
    RET_MAGIC, finish_vectors, provenance, run_checked, file_span, image_bytes,
    NativeCallTrace,
)
from tools.projectile_oracle.bridge_render_inputs import BulletReader, lexical
from tools.rules_oracle.bridge_anim_inputs import Reader
from tools.spatial_oracle.building_body_rules import INI, RULES, SP, TYPE, dwords

ACTOR, PASSENGER, PASSENGER_TYPE, TEMPORAL = (
    TYPE + 0x6000, TYPE + 0x7000, TYPE + 0x8000, TYPE + 0x9000,
)
SET_GUNNER, RECEIVE_GUNNER, REMOVE_GUNNER = 0x70DC70, 0x746420, 0x7464E0
PAIR_NAMES = [
    'Normal', 'Repair', 'MachineGun', 'Flak', 'Pistol', 'Sniper', 'Shock',
    'Explode', 'BrainBlast', 'RadCannon', 'Chrono', 'TerroristExplode', 'Cow',
    'Initiate', 'Virus', 'YuriPrime', 'Guardian',
]
SPANS = [
    (0x6F2B99, 0x6F2BCA), (0x7110F2, 0x7110F8),
    (0x71136F, 0x71137B), (0x711781, 0x711790),
    (0x71284A, 0x712898), (0x714780, 0x71479A),
    (0x747BBD, 0x747E90), (0x717890, 0x7178A2),
    (0x7178B0, 0x7178BE), (0x70DC70, 0x70DCDE),
    (0x735678, 0x735691), (0x74689B, 0x7468B4),
    (0x746420, 0x7464DC), (0x7464E0, 0x7465A6),
    (0x4DE710, 0x4DE74E), (0x473430, 0x473453), (0x5F8640, 0x5F8696),
    (0x5F8844, 0x5F88AC), (0x73B7A3, 0x73B7CB),
    (0x73B8D9, 0x73B94F), (0x73BC26, 0x73BD79),
    (0x6FA4FB, 0x6FA5BE), (0x6F2E98, 0x6F2E9E),
    (0x6FE4A4, 0x6FE4C8), (0x6FF28F, 0x6FF2BE),
]


def signed(value):
    return struct.unpack('<i', dwords(value))[0]


class Machine(Reader):
    def __init__(self, name='FV'):
        self.phase = 'setup'
        self.reads, self.events, self.writes = [], [], []
        self.supply_rof = False
        self.loader_failure = None
        # The inherited owner installs CRT/TLS and original INI lookup support.
        # A nonexistent directory intentionally supplies no external file IO.
        super().__init__(Path(__file__).with_suffix('.no-assets'), {})
        self.u.hook_add(UC_HOOK_MEM_WRITE, self.on_write)
        self.u.mem_write(TYPE, bytes(0x1000))
        self.u.mem_write(TYPE, dwords(0x7F6218))
        self.u.mem_write(TYPE + 0x24, name.encode('ascii') + b'\0')
        self.u.mem_write(TYPE + 0x1F8, name.encode('ascii') + b'\0')
        self.regs(esi=TYPE, ebx=0, ebp=-1)
        for begin, end in [(0x7110F2, 0x7110F8), (0x71136F, 0x71137B),
                           (0x711781, 0x711790)]:
            run_checked(self.u, begin, end)
        self.constructor = self.type_state()
        self.u.mem_write(ACTOR, dwords(0x7F5C70))
        self.u.mem_write(ACTOR + 0x6C4, dwords(TYPE))
        self.u.mem_write(PASSENGER, dwords(0x7EB058))
        self.u.mem_write(PASSENGER + 0x6C0, dwords(PASSENGER_TYPE))
        self.rof_entries = {
            self.read32(0x7F5C70 + 0x318), self.read32(0x7EB058 + 0x318),
        }
        self.code_before = [bytes(self.u.mem_read(a, b-a)) for a, b in SPANS]

    def regs(self, **values):
        registers = dict(eax=UC_X86_REG_EAX, ebp=UC_X86_REG_EBP,
                         ebx=UC_X86_REG_EBX, ecx=UC_X86_REG_ECX,
                         edi=UC_X86_REG_EDI, edx=UC_X86_REG_EDX,
                         esi=UC_X86_REG_ESI, esp=UC_X86_REG_ESP)
        for key, value in values.items():
            self.u.reg_write(registers[key], value & 0xFFFFFFFF)

    def hook(self, u, pc, size, user):
        if self.phase != 'setup':
            sp = u.reg_read(UC_X86_REG_ESP)
            if pc in (0x5276D0, 0x5295F0):
                self.reads.append(dict(reader=hex(pc),
                    section=self.string(self.read32(sp+4)),
                    key=self.string(self.read32(sp+8)),
                    default=signed(self.read32(sp+12))))
            elif pc == 0x717890:
                self.events.append(dict(event='map_write', index=signed(self.read32(sp+4)),
                                        weapon=signed(self.read32(sp+8))))
            elif pc == SET_GUNNER:
                self.events.append(dict(event='set_gunner', mode=signed(self.read32(sp+4))))
            elif pc == 0x70DCF0:
                self.events.append(dict(event='get_current_turret'))
            elif pc in (0x65C780, 0x65C7E0):
                self.events.append(dict(event='scenario_random', entry=hex(pc)))
                raise AssertionError('unexpected RNG in bounded IFV component')
            elif self.supply_rof and pc in self.rof_entries:
                self.events.append(dict(event='supplied_get_rof',
                    receiver='transport' if u.reg_read(UC_X86_REG_ECX) == ACTOR else 'passenger',
                    slot=signed(self.read32(sp+4)), result=19))
                self.ret(19, 4)
                return
            elif self.phase == 'loader' and pc in (0x5F7A90, 0x5F7DB0):
                part = 'turret' if pc == 0x5F7A90 else 'barrel'
                index = signed(self.read32(sp+8))
                accepted = self.loader_failure != (part, index)
                self.events.append(dict(event='supplied_part_load', part=part, index=index,
                    image=self.string(self.read32(sp+4)), accepted=accepted))
                self.ret(int(accepted), 8)
                return
        super().hook(u, pc, size, user)

    def on_write(self, u, access, address, size, value, data):
        if self.phase == 'setup':
            return
        for owner, base, fields in [
            ('type', TYPE, [0x688, 0x808, 0x80C, 0x810, *range(0x814, 0x85C, 4)]),
            ('transport', ACTOR, [0x124, 0x138, 0x274, 0x2EC, 0x2F4, 0x2F8, 0x3B8]),
            ('passenger', PASSENGER, [0x274, 0x2EC, 0x2F4, 0x3B8]),
            ('temporal', TEMPORAL, [0x24]),
        ]:
            if any(address <= base+field < address+size for field in fields):
                self.writes.append(dict(owner=owner, offset=hex(address-base),
                    pc=hex(u.reg_read(UC_X86_REG_EIP)), size=size, value=signed(value)))

    def reset_trace(self, phase):
        self.phase = phase
        self.reads, self.events, self.writes = [], [], []

    def type_state(self):
        return dict(turrets=list(struct.unpack('<18i', self.u.mem_read(TYPE+0x814, 72))),
                    charge_raw_dword=self.read32(TYPE+0x810),
                    is_charge_turret=bool(self.u.mem_read(TYPE+0x810, 1)[0]),
                    turret_count=signed(self.read32(TYPE+0x808)),
                    weapon_count=signed(self.read32(TYPE+0x80C)),
                    ifv_mode=signed(self.read32(TYPE+0x688)))

    def object_state(self):
        return dict(turret=signed(self.read32(ACTOR+0x124)),
                    weapon=signed(self.read32(ACTOR+0x138)))

    def read_layer(self, sections, label):
        self.make_ini(sections)
        self.reset_trace('reader')
        before = self.type_state()
        self.u.mem_write(SP, dwords(RET_MAGIC, INI))
        self.regs(esp=SP, ecx=TYPE)
        end = run_checked(self.u, 0x410A60, (0x410A8C, 0x410B7D),
                          required_addresses=(0x526810,))
        admitted = end == 0x410A8C
        if admitted:
            self.regs(esp=SP, ebp=TYPE, esi=INI, ebx=TYPE+0x24)
            run_checked(self.u, 0x71284A, 0x712898)
            self.regs(esp=SP, ebp=TYPE, edi=INI, ebx=TYPE+0x24)
            run_checked(self.u, 0x714780, 0x71479A)
            self.regs(esp=SP, esi=TYPE, ebx=INI, ebp=TYPE+0x24, eax=0)
            run_checked(self.u, 0x747BBD, 0x747E90)
            assert self.u.reg_read(UC_X86_REG_ESP) == SP
        assert self.code_before == [bytes(self.u.mem_read(a,b-a)) for a,b in SPANS]
        return dict(name=label, sections=sections, admitted=admitted, before=before,
                    after=self.type_state(), reads=self.reads.copy(),
                    writes=self.writes.copy(), events=self.events.copy())

    def initialize(self, mapping, *, gunner=True, charge=False, count=4):
        self.phase = 'setup'
        self.u.mem_write(TYPE+0x814, dwords(*mapping))
        self.u.mem_write(TYPE+0x805, bytes([gunner]))
        self.u.mem_write(TYPE+0x808, dwords(count))
        self.u.mem_write(TYPE+0x810, dwords(charge))
        self.u.mem_write(ACTOR+0x124, dwords(987))
        self.u.mem_write(ACTOR+0x138, dwords(654))
        self.regs(esp=SP, esi=ACTOR, ebx=0)
        run_checked(self.u, 0x6F2B99, 0x6F2BCA)
        base = self.object_state()
        self.reset_trace('initialize')
        self.regs(esp=SP, esi=ACTOR)
        run_checked(self.u, 0x735678, 0x735691)
        return dict(base_constructor=base, after=self.object_state(),
                    writes=self.writes.copy(), events=self.events.copy())

    def operation(self, entry, args=()):
        self.reset_trace('operation')
        before = self.object_state()
        self.invoke(entry, ACTOR, args)
        return dict(entry=hex(entry), before=before, after=self.object_state(),
                    writes=self.writes.copy(), events=self.events.copy())


def reader_controls():
    pairs = {name+'TurretWeapon': str(i) for i, name in enumerate(PAIR_NAMES)}
    cases = [
        ('missing_section', 'FV', {}), ('empty_section', 'FV', {'FV': {}}),
        ('all_fixed_index_defaults', 'FV', {'FV': pairs}),
        ('wrong_key_case', 'FV', {'FV': {'normalturretweapon': '0', 'normalturretindex': '3'}}),
        ('case_insensitive_fv_gate', 'fv', {'fv': pairs}),
        ('non_fv_gate', 'FVCOPY', {'FVCOPY': pairs}),
        ('duplicate_slot_last_writer', 'FV', {'FV': {
            'NormalTurretWeapon': '4', 'NormalTurretIndex': '3',
            'RepairTurretWeapon': '4', 'RepairTurretIndex': '2',
            'GuardianTurretWeapon': '4', 'GuardianTurretIndex': '1'}}),
        ('signed_index', 'FV', {'FV': {'NormalTurretWeapon': '0', 'NormalTurretIndex': '-1'}}),
    ]
    for value in ['-1', '18', '2147483647', '2147483648', '$10', '2junk', '', 'bogus']:
        cases.append(('signed_ifv_mode_'+repr(value), 'E1', {'E1': {'IFVMode': value}}))
    for value in ['0', '1', '256', '257', '-1']:
        cases.append(('missing_weapon_alias_'+value, 'FV', {'FV': {
            'IsChargeTurret': 'yes', 'GuardianTurretIndex': value}}))
    cases += [
        ('charge_kept_all_weapons_present', 'FV', {'FV': {**pairs, 'IsChargeTurret': 'yes'}}),
        ('invalid_weapon_minus_two_alias', 'FV', {'FV': {'GuardianTurretWeapon': '-2', 'GuardianTurretIndex': '6'}}),
        ('invalid_weapon_eighteen', 'FV', {'FV': {'GuardianTurretWeapon': '18', 'GuardianTurretIndex': '2'}}),
    ]
    rows = []
    for label, name, sections in cases:
        m = Machine(name)
        rows.append(m.read_layer(sections, label))
    m = Machine()
    history = []
    for label, keys in [
        ('base', {'NormalTurretWeapon': '0', 'NormalTurretIndex': '3', 'IFVMode': '6'}),
        ('mode', {'RepairTurretWeapon': '0'}),
        ('map_no_section', None),
        ('map_section_missing_keys', {'Other': 'yes'}),
        ('map_normal_index_missing', {'NormalTurretWeapon': '0'}),
    ]:
        history.append(m.read_layer({} if keys is None else {'FV': keys}, label))
    return dict(constructor=Machine().constructor, rows=rows, layer_history=history)


def lifecycle(mapping):
    rows = []
    for charge in [False, True]:
        for mode in [*range(18), -1, -2, 18, 255, 2147483647, -2147483648]:
            m = Machine()
            initial = m.initialize(mapping, charge=charge)
            m.u.mem_write(PASSENGER_TYPE+0x688, dwords(mode))
            receive = m.operation(RECEIVE_GUNNER, [PASSENGER])
            remove = m.operation(REMOVE_GUNNER, [PASSENGER])
            # Failed vehicle placement re-adopts the removed passenger.
            retry = m.operation(RECEIVE_GUNNER, [PASSENGER])
            rows.append(dict(mode=mode, charge=charge, initialization=initial,
                             receive=receive, remove=remove, retry=retry))
    controls = []
    for gunner, count in [(False, 4), (True, 0), (True, -1)]:
        m = Machine()
        initial = m.initialize(mapping, gunner=gunner, count=count)
        m.regs(esp=SP, esi=ACTOR)
        m.reset_trace('init_from_type')
        run_checked(m.u, 0x74689B, 0x7468B4)
        controls.append(dict(gunner=gunner, count=count, initialization=initial,
                             init_from_type=m.object_state(), events=m.events.copy(),
                             remove_null=m.operation(REMOVE_GUNNER, [0])))
    # IsGattling belongs to consumers, not the selector's write gate.
    m = Machine()
    initial = m.initialize(mapping)
    m.u.mem_write(TYPE+0xCD5, b'\x01')
    gattling = dict(initialization=initial, select=m.operation(SET_GUNNER, [6]))
    temporal = []
    for running in [False, True]:
        m = Machine()
        m.initialize(mapping)
        m.u.mem_write(0xA8ED84, dwords(200))
        m.u.mem_write(PASSENGER_TYPE+0x688, dwords(10))
        m.u.mem_write(PASSENGER+0x274, dwords(TEMPORAL))
        m.u.mem_write(TEMPORAL+0x24, dwords(PASSENGER, 0))
        m.u.mem_write(PASSENGER+0x2EC, dwords(190, 0, 20 if running else 0))
        m.supply_rof = True
        receive = m.operation(RECEIVE_GUNNER, [PASSENGER])
        remove = m.operation(REMOVE_GUNNER, [PASSENGER])
        temporal.append(dict(running=running, supplied_rof=19, receive=receive, remove=remove))
    cargo = []
    for gunner in [False, True]:
        for count in [0, 1, 2]:
            m = Machine()
            m.initialize(mapping, gunner=gunner)
            m.operation(SET_GUNNER, [6])
            second = PASSENGER+0x300
            m.u.mem_write(ACTOR+0x114, dwords(count, PASSENGER if count else 0))
            m.u.mem_write(PASSENGER+0x30, dwords(second if count == 2 else 0))
            result = m.operation(0x4DE710)
            cargo.append(dict(gunner=gunner, initial_count=count, result=result,
                removed_first=m.u.reg_read(UC_X86_REG_EAX) == PASSENGER,
                count=signed(m.read32(ACTOR+0x114)),
                head='second' if m.read32(ACTOR+0x118) == second else 'none',
                removed_next_is_null=m.read32(PASSENGER+0x30) == 0))
    return dict(mapping=mapping, rows=rows, initialization_controls=controls,
                gattling_selector=gattling, temporal_boundary=temporal, cargo_removal=cargo)


def draw_decisions():
    rows = []
    for count, gattling in [(0, False), (-1, False), (4, False), (4, True)]:
        for turret in [False, True]:
            for index in [-1, 0, 1, 2, 3, 4, 17, 18]:
                m = Machine()
                u = m.u
                m.initialize([-1]*18, count=count)
                u.mem_write(TYPE+0xCA1, bytes([turret]))
                u.mem_write(TYPE+0xCD5, bytes([gattling]))
                u.mem_write(ACTOR+0x124, dwords(index))
                # Every possible supplied part pair in this bounded range is
                # nonnull. No pointer is dereferenced as a voxel by this slice.
                u.mem_write(TYPE+0xB0, dwords(*([0x12345678]*82)))
                m.reset_trace('draw')
                m.regs(esp=SP, ebp=ACTOR, ebx=TYPE)
                admitted = run_checked(u, 0x73B7A3, (0x73B8D9, 0x73B7CB)) == 0x73B8D9
                result = None
                if admitted:
                    m.regs(esp=SP, ebp=ACTOR, ebx=TYPE, esi=0x123456)
                    run_checked(u, 0x73B8D9, 0x73B94F)
                    selected = signed(m.read32(SP+0x1C))
                    key = u.reg_read(UC_X86_REG_ESI)
                    m.regs(esi=selected, ebx=TYPE)
                    run_checked(u, 0x73BD15, 0x73BD1C)
                    pointer_offset = signed(u.reg_read(UC_X86_REG_ECX)-TYPE)
                    result = dict(selected=selected, cache_key=key,
                                  indexed_turret_pointer_offset=pointer_offset,
                                  indexed_consumer=any(event['event'] == 'get_current_turret'
                                                       for event in m.events))
                rows.append(dict(turret_count=count, is_gattling=gattling,
                                 turret=turret, selected_index=index,
                                 gun_branch=admitted, result=result))
    filenames = []
    for index in range(4):
        for part, begin, end in [('turret', 0x5F7A90, 0x5F7AE6),
                                  ('barrel', 0x5F7DB0, 0x5F7E06)]:
            m = Machine()
            m.u.mem_write(SP, dwords(RET_MAGIC, TYPE+0x1F8, index))
            m.regs(esp=SP, ecx=TYPE)
            run_checked(m.u, begin, end)
            output = m.string(m.u.reg_read(UC_X86_REG_ESP)+0xEC)
            filenames.append(dict(index=index, part=part, native_basename=output))
    return dict(rows=rows, filenames=filenames)


def loader_decisions():
    rows = []
    for count, gattling, turret, failure in [
        (4, False, True, None), (4, False, False, None),
        (4, True, True, None), (4, True, False, None),
        (0, False, True, None), (-1, False, True, None),
        (1, False, True, None), (18, False, True, None),
        (4, False, True, ('turret', 2)), (4, False, True, ('barrel', 1)),
        (4, False, False, ('turret', 1)),
    ]:
        m = Machine()
        m.u.mem_write(TYPE+0x808, dwords(count))
        m.u.mem_write(TYPE+0xCD5, bytes([gattling]))
        m.u.mem_write(TYPE+0xCA1, bytes([turret]))
        m.loader_failure = failure
        m.reset_trace('loader')
        m.regs(esp=SP, esi=TYPE, ebx=0)
        turret_boundary = run_checked(m.u, 0x5F8640, (0x5F8844, 0x5F8696))
        # The base TUR asset IO for the plain branch is a supplied boundary.
        # Indexed failure keeps its actual failure byte while barrel loading runs.
        m.regs(esp=SP, esi=TYPE, ebx=0)
        barrel_boundary = run_checked(m.u, 0x5F8844, (0x5F8A60, 0x5F8A6A, 0x5F88AC))
        rows.append(dict(count=count, gattling=gattling, turret=turret,
            supplied_failure=list(failure) if failure else None,
            turret_boundary=hex(turret_boundary), barrel_boundary=hex(barrel_boundary),
            prior_failure=bool(m.u.mem_read(SP+0x13, 1)[0]), events=m.events.copy()))
    return rows


def charge_turrets():
    """Shared indexed consumer: original frame arithmetic, no weapon/ROF port."""
    m = Machine()
    m.regs(esi=ACTOR, ebx=0)
    run_checked(m.u, 0x6F2E98, 0x6F2E9E)
    constructor = signed(m.read32(ACTOR+0x2F8))
    writers = []
    for disk_laser in [False, True]:
        for berserk in [False, True]:
            for rof in [-2147483648, -101, -1, 0, 1, 99, 100, 2147483647]:
                m = Machine()
                m.u.mem_write(ACTOR+0x298, bytes([berserk]))
                m.u.mem_write(0xA8ED84, dwords(200))
                m.regs(esp=SP, esi=ACTOR, eax=rof, edi=0)
                m.reset_trace('fire_rearm_copy')
                begin, end = (0x6FE4A4, 0x6FE4C8) if disk_laser else (0x6FF28F, 0x6FF2BE)
                run_checked(m.u, begin, end)
                writers.append(dict(disk_laser=disk_laser, berserk=berserk,
                    supplied_get_rof=rof, frame=200,
                    charge_delay=signed(m.read32(ACTOR+0x2F8)),
                    timer_start=signed(m.read32(ACTOR+0x2EC)),
                    timer_duration=signed(m.read32(ACTOR+0x2F4)), writes=m.writes.copy()))
    base = dict(charge=True, count=4, gattling=False, frame=200, start=200,
                duration=100, charge_delay=100, initial_turret=2, initial_weapon=8)
    cases = [dict(name='ordinary_frame_'+str(frame), frame=frame)
             for frame in [199, 200, 201, 224, 225, 226, 249, 250, 251,
                           274, 275, 276, 299, 300, 301]]
    cases += [dict(name='startup', charge_delay=0, duration=0, initial_turret=-1),
              dict(name='no_charge', charge=False), dict(name='gattling', gattling=True),
              dict(name='zero_count', count=0), dict(name='negative_count', count=-1),
              dict(name='negative_delay', charge_delay=-1),
              dict(name='stopped_timer', start=-1, duration=25),
              dict(name='stopped_negative_timer', start=-1, duration=-25),
              dict(name='running_negative_timer', duration=-25),
              dict(name='future_timer', start=300),
              dict(name='frame_wrap', frame=-2147483648, start=2147483640),
              dict(name='elapsed_signed_wrap', frame=2147483647, start=-10),
              dict(name='product_wrap_negative', count=2147483647, start=-1, duration=2, charge_delay=2),
              dict(name='product_wrap_zero', count=65536, start=-1, duration=65536, charge_delay=100),
              dict(name='single_turret', count=1),
              dict(name='three_turrets_truncation', count=3, duration=99, charge_delay=100)]
    rows = []
    for sparse in cases:
        inputs = {**base, **sparse}
        m = Machine()
        m.u.mem_write(TYPE+0x810, bytes([inputs['charge']]))
        m.u.mem_write(TYPE+0x808, dwords(inputs['count']))
        m.u.mem_write(TYPE+0xCD5, bytes([inputs['gattling']]))
        m.u.mem_write(ACTOR+0x124, dwords(inputs['initial_turret']))
        m.u.mem_write(ACTOR+0x138, dwords(inputs['initial_weapon']))
        m.u.mem_write(ACTOR+0x2EC, dwords(inputs['start'], 0, inputs['duration'], inputs['charge_delay']))
        m.u.mem_write(0xA8ED84, dwords(inputs['frame']))
        m.regs(esp=SP, esi=ACTOR, ebp=0)
        m.reset_trace('charge_ai')
        run_checked(m.u, 0x6FA4FB, 0x6FA5BE)
        rows.append(dict(inputs=inputs, after=m.object_state(),
                         writes=m.writes.copy(), events=m.events.copy()))
    return dict(constructor_delay=constructor, fire_writers=writers, ai_rows=rows)


def physical_inputs():
    root = Path(os.environ.get('VERA20K_IFV_ASSETS', 'target/asset/ifv-turret-switching/extract'))
    files = {p.name.lower(): p for p in root.glob('*') if p.is_file()}
    assert 'rulesmd.ini' in files, 'VERA20K_IFV_ASSETS needs a physical RULESMD.INI'
    m = Machine()
    layers = []
    for file in ['rulesmd.ini', 'langrule.ini', 'mpbattlemd.ini']:
        if file not in files:
            layers.append(dict(file=file, absent=True))
            continue
        raw = files[file].read_bytes()
        sections, lines = lexical(raw, {'FV'})
        layers.append(dict(file=file, bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(),
                           source_lines=lines, **m.read_layer(sections, file)))
    map_path = Path(os.environ.get('VERA20K_IFV_MAP', str(root/'ifv-turret-switching-owned.map')))
    assert map_path.is_file(), 'VERA20K_IFV_MAP needs the production observation map INI'
    raw = map_path.read_bytes()
    assert raw.lstrip().startswith((b';', b'[')), 'map input must be INI text, not a MIX archive'
    sections, lines = lexical(raw, {'FV'})
    layers.append(dict(file=map_path.name, bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(),
                       source_lines=lines, **m.read_layer(sections, 'production_observation_map')))
    infantry = []
    for name in ['E1', 'E2', 'ENGINEER', 'GGI', 'SHK', 'CLEG', 'IVAN']:
        p = Machine(name)
        raw = files['rulesmd.ini'].read_bytes()
        sections, _ = lexical(raw, {name})
        infantry.append(dict(type=name, **p.read_layer(sections, name)))
    assets = []
    for stem in ['fv', 'fvtur', 'fvtur1', 'fvtur2', 'fvtur3',
                 'fvbarl', 'fvbarl1', 'fvbarl2', 'fvbarl3']:
        for ext in ['vxl', 'hva']:
            name = stem+'.'+ext
            path = files.get(name)
            if path is None:
                assets.append(dict(file=name, present_in_extract=False))
            else:
                raw = path.read_bytes()
                assets.append(dict(file=name, present_in_extract=True,
                    bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(),
                    header24_hex=raw[:24].hex()))
    p = Machine('SREF')
    sref_sections, _ = lexical(files['rulesmd.ini'].read_bytes(), {'SREF'})
    return dict(layers=layers, final_type=m.type_state(), infantry=infantry, assets=assets,
                sref=p.read_layer(sref_sections, 'SREF'))


def generate():
    physical = physical_inputs()
    return dict(schema_version=1, pair_names=PAIR_NAMES, readers=reader_controls(),
                physical=physical, lifecycle=lifecycle(physical['final_type']['turrets']),
                draw=draw_decisions(), loader=loader_decisions(), charge=charge_turrets())


def metadata():
    result = provenance(
        scope='IFV native scalar/mapping readers, ordinary gunner cycle, initialization and bounded voxel draw decisions',
        assumptions=[
            'Original gamemd.exe bytes only. Supplied cached INI section/key indexes contain physical lexical values, never Rust-parsed scalars; original section admission, ReadInt, ReadBool and FV identity compare execute.',
            'Reader executes selected TechnoType blocks then complete FV pair block in original order. Intervening unrelated keys, original archive/INI loading and layer scheduling are not executed. Missing extracted layer is reported, not inferred to contain no override.',
            'Constructor slices execute with original established EBX=0, EBP=-1; supplied initialized Unit/Infantry vtables and type pointers. Unit constructor and InitFromType Gunner suffixes execute separately, not their whole bodies.',
            'Ordinary ReceiveGunner/RemoveGunner execute whole bodies with null TemporalClass. Failed vehicle-unload retry is represented by receive/remove/receive; six additional rows execute FootClass RemoveFirstPassenger and CargoClass head removal. Spatial placement remains outside this component.',
            'Temporal contrasts supply existing TemporalClass with no target. Active GetROF is answered19 and recorded: its real RNG and full temporal scheduling are not claimed. Native rearm arithmetic and ownership writes around that boundary execute.',
            'Draw rows execute admission and cache/index selection slices with supplied nonnull part pointers, then original indexed turret LEA. For the plain branch that LEA is a diagnostic only, not its production consumer. No renderer/raster, shadows, palette or crash parity claim.',
            'Loader rows execute indexed-loop admission, count limits, first-failure exits and the independent Turret flag gate before barrels. Each part-load result and plain TUR asset IO are supplied; filename rows separately execute original sprintf. Final failure cleanup is established statically, not executed here.',
            'Negative/oversized turret or mapping indices intentionally expose native unchecked memory aliasing. Rust must choose and document its safe malformed-mod policy, not mistake these rows for valid asset-table indices.',
            'Charge-turret shared consumer executes complete original6FA4FB..6FA5BE. Fire writer slices start after existing virtual GetROF and consume supplied signed ROF; the existing GetROF RNG/cadence owner is not reimplemented or claimed. Native multiplication, signed division, clamp and timer subtraction execute, including overflow controls.',
        ],
        substitutions=[
            'Existing Reader supplies bump allocator, no-op deallocator and CRT TLS while preparing INI caches; selected scalar/mapping/lifecycle/draw slices use no file or allocation substitution.',
            'Only temporal-boundary rows intercept actual virtual GetROF entries with result19; ordinary lifecycle has no substituted calls or RNG draws.',
            'Loader loop rows answer5F7A90/5F7DB0 with their declared boolean part-load verdict; these rows do not claim native VXL/HVA parsing or archive IO.',
        ],
        entry_points={'set_gunner': SET_GUNNER, 'receive_gunner': RECEIVE_GUNNER,
                      'remove_gunner': REMOVE_GUNNER, 'fv_reader': 0x747BBD,
                      'ifv_mode_reader': 0x714780, 'turret_draw_admission': 0x73B7A3,
                      'turret_draw_key': 0x73B8D9, 'turret_filename': 0x5F7A90,
                      'barrel_filename': 0x5F7DB0})
    original = image_bytes()
    result['original_slices'] = [dict(start=hex(a), end_exclusive=hex(b),
        sha256=hashlib.sha256(file_span(original,a,b-a)[1]).hexdigest(),
        hex=file_span(original,a,b-a)[1].hex()) for a,b in SPANS]
    return result


class TemporalGunnerReader(BulletReader):
    """Compose the existing reader VM; never answer a gameplay/ROF query.

    Separate from Machine's historical GetROF=19 controls so neither their
    input setup nor their saved output changes. Original concrete vtables,
    GetWeapon, rank predicates, GetROF and lone-Unit LetGo all execute.
    """

    READ_KEYS = {
        'Gunner', 'Passengers', 'Size', 'SizeLimit', 'IFVMode', 'TurretCount',
        'WeaponCount', 'IsChargeTurret', 'IsGattling', 'Primary', 'Secondary',
        'ElitePrimary', 'EliteSecondary', 'VeteranAbilities', 'EliteAbilities',
        'Burst', 'ROF', 'IsSonic', 'UseFireParticles', 'UseSparkParticles',
        'IsRailgun', 'Warhead', 'Temporal', 'VeteranROF', 'GameSpeedBias',
        'TeamDelays', 'OccupyROFMultiplier', 'BunkerROFMultiplier',
        'FirePower', 'Groundspeed', 'Airspeed', 'Armor', 'Cost', 'BuildTime',
        'RepairDelay', 'BuildDelay', 'Firepower', 'Groundspeed',
    } | {f'{prefix}Weapon{i}' for prefix in ('', 'Elite') for i in range(1, 18)} | {
        f'BurstDelay{i}' for i in range(4)
    }
    CALLS = {
        RECEIVE_GUNNER: ('receive_gunner', 1, 4),
        REMOVE_GUNNER: ('remove_gunner', 1, 4),
        SET_GUNNER: ('set_gunner', 1, 4),
        0x6FCFA0: ('get_rof', 1, 4),
        0x70E140: ('get_weapon', 1, 4),
        0x7177C0: ('normal_weapon_slot', 1, 4),
        0x7177E0: ('elite_weapon_slot', 1, 4),
        0x74FF90: ('is_veteran', 0, 0),
        0x750010: ('is_elite', 0, 0),
        0x71ABC0: ('let_go', 0, 0),
        0x4D3780: ('mark', 1, 4),
        0x65C780: ('next', 0, 0),
        0x65C7E0: ('range', 2, 8),
    }

    def __init__(self, root, layers):
        self.phase = 'setup'
        self.read_receipts, self.platform, self.calls, self.raw_draws = [], [], [], []
        self.trace = None
        art_path = root / 'ARTMD.INI'
        raw = art_path.read_bytes()
        art, lines = lexical(raw, {'FV', 'CLEG', 'ClegSequence'})
        super().__init__(art, root)
        u = self.u
        self.code = bytes(u.mem_read(0x401000, 0x3E0000))
        u.reg_write(UC_X86_REG_FPCW, 0x0E3F)
        u.mem_write(0x822D80, struct.pack('<H', 0x0E3F))
        u.mem_write(0xA8B238, dwords(5))
        for registry in (0x887568, 0x8874C0, 0xA8EB00, 0xA83CE0, 0xA8ED40,
                         0xA8E348, 0xA83DE8, 0xA83C98, 0x8B4120, 0xB0EC60):
            u.mem_write(registry, dwords(0x7EB6D4, self.alloc(4096), 1024, 1, 0, 10))
        self.rules = self.alloc(0x2000)
        u.mem_write(0x8871E0, dwords(self.rules))
        self.invoke(0x665650, self.rules)
        self.types = {'FV': self.alloc(0x1900), 'CLEG': self.alloc(0x1900)}
        self.invoke(0x7470D0, self.types['FV'], (self.cstring('FV'),))
        self.invoke(0x5236A0, self.types['CLEG'], (self.cstring('CLEG'),))
        self.invoke(0x523D00, self.types['CLEG'])
        self.inputs = dict(
            art=dict(file=art_path.name, bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(),
                     sections=art, source_lines=lines),
            constructor=self.physical_state(), layers=[])
        declarations, declaration_lines = lexical(layers[0][1].read_bytes(), {'Countries', 'Sides'})
        self.countries = {}
        for name in declarations['Countries'].values():
            pointer = self.alloc(0x300)
            self.invoke(0x5113F0, pointer, (self.cstring(name),))
            self.countries[name] = pointer
        for name in declarations['Sides']:
            self.invoke(0x6A4550, self.alloc(0xC0), (self.cstring(name),))
        self.inputs['declarations'] = dict(sections=declarations, source_lines=declaration_lines)
        self.weapons = {}
        wanted_weapons = ('CRNuke', 'CRNeutronRifle', 'NeutronRifle', 'NeutronRifleE')
        wanted = {'FV', 'CLEG', *wanted_weapons, 'ChronoBeam', 'General',
                  'CombatDamage', 'Easy', 'Normal', 'Difficult', 'Americans', 'Russians'}
        for name, path in layers:
            if not path.exists():
                assert name == 'LANGRULE.INI', ('required physical layer missing', path)
                self.inputs['layers'].append(dict(file=name, absent=True))
                continue
            raw = path.read_bytes()
            sections, source_lines = lexical(raw, wanted)
            # Preserve section presence and physical strings, but omit unrelated
            # General/Combat references from this component's load boundary.
            for section, keys in (
                ('General', {'VeteranROF', 'GameSpeedBias', 'TeamDelays'}),
                ('CombatDamage', {'OccupyROFMultiplier', 'BunkerROFMultiplier'}),
            ):
                if section in sections:
                    sections[section] = {k: v for k, v in sections[section].items() if k in keys}
            self.rules_cache(sections)
            mark = len(self.read_receipts)
            before = self.physical_state()
            general = self.invoke(0x66D530, self.rules, (RULES,), timeout_us=30_000_000)
            self.invoke(0x674500, self.rules, (RULES,))
            combat = self.invoke(0x66BBB0, self.rules, (RULES,))
            type_results = {
                'FV': self.invoke(0x747620, self.types['FV'], (RULES,)) & 255,
                'CLEG': self.invoke(0x5240A0, self.types['CLEG'], (RULES,)) & 255,
            }
            for weapon_name in wanted_weapons:
                if weapon_name not in self.weapons:
                    self.weapons[weapon_name] = self.invoke(0x772FA0, self.cstring(weapon_name))
                self.invoke(0x772080, self.weapons[weapon_name], (RULES,))
            chrono_refs = [self.read32(self.weapons[weapon_name] + 0xAC)
                           for weapon_name in ('CRNeutronRifle', 'NeutronRifle', 'NeutronRifleE')]
            assert len(set(chrono_refs)) == 1, ('unshared ChronoBeam registry identity', chrono_refs)
            chrono = chrono_refs[0]
            assert chrono and self.string(chrono + 0x24) == 'ChronoBeam'
            self.invoke(0x75D3A0, chrono, (RULES,))
            country_results = {
                label: self.invoke(0x511850, self.countries[label], (RULES,)) & 255
                for label in ('Americans', 'Russians')
            }
            self.inputs['layers'].append(dict(
                file=name, bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(),
                sections=sections, source_lines=source_lines, before=before,
                after=self.physical_state(), admitted=dict(general=general & 255,
                    combat=combat & 255, types=type_results, countries=country_results),
                reads=self.read_receipts[mark:]))
        self.inputs['final'] = self.physical_state()
        self.actors = {name: self.alloc(0x1000) for name in ('transport', 'passenger', 'victim')}
        self.houses = {name: self.alloc(0x6000) for name in ('transport', 'passenger')}
        for name, pointer in self.actors.items():
            typ = self.types['CLEG' if name == 'passenger' else 'FV']
            u.mem_write(pointer, dwords(0x7EB058 if name == 'passenger' else 0x7F5C70))
            u.mem_write(pointer + (0x6C0 if name == 'passenger' else 0x6C4), dwords(typ))
            u.mem_write(pointer + 0x6C, bytes(u.mem_read(typ + 0xA0, 4)))
            u.mem_write(pointer + 0x90, b'\x01')
        # Execute the original admitted InitManagers Temporal producer. Its
        # real GetWeapon/Warhead test, allocation, ctor and weapon Damage copy
        # establish the retained manager before supplied world/warp controls.
        u.mem_write(0xA8ED84, dwords(0))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ESI, self.actors['passenger'])
        run_checked(u, 0x6F4154, 0x6F41C0,
                    required_addresses=(0x70E140, 0x71A4E0))
        assert u.reg_read(UC_X86_REG_ESP) == SP
        self.link = self.read32(self.actors['passenger'] + 0x274)
        assert self.link and self.read32(self.link + 0x24) == self.actors['passenger']
        self.inputs['manager_producer'] = dict(entry='0x6f4154', end_exclusive='0x6f41c0',
            owner='passenger', damage=signed(self.read32(self.link + 0x4C)),
            target_is_null=self.read32(self.link + 0x28) == 0,
            registry_count=self.read32(0xB0EC70))
        self.inputs['platform'] = self.platform.copy()
        self.inputs['asset_loads'] = self.asset_loaded.copy()
        self.rngs = {'scenario': self.read32(0xA8B230) + 0x218,
                     'main': 0x886B88, 'mapgen': 0xABE890}
        assert self.code == bytes(u.mem_read(0x401000, 0x3E0000)), 'native setup changed code'
        self.saved = [(start, bytes(u.mem_read(start, end - start + 1)))
                      for start, end, _ in u.mem_regions()]
        self.cpu, self.saved_cursor = u.context_save(), self.cursor
        self.u.hook_add(UC_HOOK_MEM_WRITE, self.observe_write)

    def hook(self, u, pc, size, user):
        sp = u.reg_read(UC_X86_REG_ESP)
        if Reader.guid_transport(self, u, pc, sp, self.platform):
            return
        if pc in (0x5F8110, 0x5F8CE0):
            assert self.phase == 'setup'
            self.platform.append(dict(kind='type_visual_asset_boundary', pc=hex(pc)))
            self.ret(0)
            return
        if pc == 0x7C978A:
            self.platform.append(dict(kind='CRT_atexit_boundary'))
            self.ret(0)
            return
        if pc in (0x5276D0, 0x5295F0, 0x5283D0, 0x528A10):
            key = self.string(self.read32(sp + 8))
            if key in self.READ_KEYS or key.endswith(('TurretWeapon', 'TurretIndex')):
                default = (bytes(u.mem_read(sp + 12, 8)).hex() if pc == 0x5283D0
                           else self.read32(sp + 12))
                self.read_receipts.append(dict(reader=hex(pc),
                    section=self.string(self.read32(sp + 4)), key=key, default=default))
        if self.phase != 'setup':
            assert self.trace is not None
            completed = self.trace.returned(pc, sp)
            self.active_rng = [i for i in self.active_rng if i not in completed]
            if pc in self.CALLS:
                index = self.trace.entered(pc, sp, self.CALLS[pc])
                call = self.calls[index]
                receiver = next((name for name, pointer in self.actors.items()
                                 if pointer == u.reg_read(UC_X86_REG_ECX)), None)
                if receiver:
                    call['receiver'] = receiver
                if pc == 0x6FCFA0:
                    pointer = u.reg_read(UC_X86_REG_ECX)
                    call.update(burst_index=signed(self.read32(pointer + 0x3B8)),
                        rank_bits=self.bits(pointer + 0x150, 4),
                        house_rof_bits=self.bits(self.read32(pointer + 0x21C) + 0x1A8, 8))
                if pc in (0x65C780, 0x65C7E0):
                    call['stream'] = next(name for name, pointer in self.rngs.items()
                                          if pointer == u.reg_read(UC_X86_REG_ECX))
                    self.active_rng.append(index)
            if pc in (0x65C79D, 0x65C84B):
                assert self.active_rng, ('raw draw without original RNG call', hex(pc))
                self.raw_draws.append(dict(stream=self.calls[self.active_rng[-1]]['stream'],
                    pc=hex(pc), raw_u32=u.reg_read(UC_X86_REG_ESI)))
        super().hook(u, pc, size, user)

    def bits(self, pointer, size):
        """Serialize native little-endian storage as a canonical hex integer."""
        return f'{int.from_bytes(self.u.mem_read(pointer, size), "little"):0{size * 2}x}'

    def physical_state(self):
        u = self.u

        def reference(pointer):
            return self.string(pointer + 0x24) if pointer else None

        types = {}
        for name, pointer in self.types.items():
            types[name] = dict(
                ifv_mode=signed(self.read32(pointer + 0x688)),
                gunner=bool(u.mem_read(pointer + 0x805, 1)[0]),
                passengers=signed(self.read32(pointer + 0x5E0)),
                veteran_rof=bool(u.mem_read(pointer + 0x2A0, 1)[0]),
                elite_rof=bool(u.mem_read(pointer + 0x2B2, 1)[0]),
                burst_delays=list(struct.unpack('<4i', u.mem_read(pointer + 0xE48, 16)))
                    if name == 'FV' else None,
                turret_count=signed(self.read32(pointer + 0x808)),
                weapon_count=signed(self.read32(pointer + 0x80C)),
                turrets=list(struct.unpack('<18i', u.mem_read(pointer + 0x814, 72))),
                weapons=[reference(self.read32(pointer + 0x898 + 28 * i)) for i in range(17)],
                elite_weapons=[reference(self.read32(pointer + 0xA94 + 28 * i)) for i in range(17)])
        weapons = {}
        for name, pointer in getattr(self, 'weapons', {}).items():
            warhead = self.read32(pointer + 0xAC)
            weapons[name] = dict(rof=signed(self.read32(pointer + 0xB0)),
                burst=signed(self.read32(pointer + 0x9C)), warhead=reference(warhead),
                temporal=bool(u.mem_read(warhead + 0x15A, 1)[0]) if warhead else None,
                flags={key: bool(u.mem_read(pointer + offset, 1)[0]) for key, offset in
                       (('sonic', 0x130), ('spark', 0x12A), ('fire', 0x129), ('railgun', 0x12D))})
        return dict(types=types, weapons=weapons,
            rules=dict(veteran_rof_bits=self.bits(self.rules + 0x690, 8),
                occupy_rof_bits=self.bits(self.rules + 0xF44, 4),
                bunker_rof_bits=self.bits(self.rules + 0xF50, 4),
                difficulty_rows=[bytes(u.mem_read(self.rules + 0x1538 + i * 0x50, 0x50)).hex()
                                 for i in range(3)]),
            countries={name: dict(native_index=signed(self.read32(pointer + 0xB8)),
                rof_bits=self.bits(pointer + 0xE8, 8))
                for name, pointer in getattr(self, 'countries', {}).items()
                if name in ('Americans', 'Russians')})

    def rng(self):
        return {name: bytes(self.u.mem_read(pointer, 0x3F4)).hex()
                for name, pointer in self.rngs.items()}

    def gunner_state(self):
        result = {}
        for name in ('transport', 'passenger'):
            pointer = self.actors[name]
            result[name] = dict(burst_index=signed(self.read32(pointer + 0x3B8)),
                rearm=[signed(self.read32(pointer + offset)) for offset in (0x2EC, 0x2F4)],
                charge_delay=signed(self.read32(pointer + 0x2F8)),
                temporal=self.read32(pointer + 0x274) == self.link)
        pointer = self.actors['transport']
        result['transport'].update(weapon_index=signed(self.read32(pointer + 0x138)),
                                   turret_index=signed(self.read32(pointer + 0x124)))
        owner = self.read32(self.link + 0x24)
        result['link'] = dict(owner=next((name for name in ('transport', 'passenger')
            if self.actors[name] == owner), None), target=bool(self.read32(self.link + 0x28)),
            victim_head=self.read32(self.actors['victim'] + 0x278) == self.link,
            victim_warped=bool(self.u.mem_read(self.actors['victim'] + 0x270, 1)[0]),
            fields={hex(offset): self.read32(self.link + offset)
                    for offset in (0x38, 0x3C, 0x40, 0x44, 0x48, 0x4C)})
        return result

    def observe_write(self, u, access, address, size, value, user):
        if self.phase == 'setup':
            return
        for name, pointer in [*self.actors.items(), ('link', self.link)]:
            if pointer <= address < pointer + (0x50 if name == 'link' else 0x1000):
                self.writes.append(dict(owner=name, offset=hex(address - pointer), size=size,
                    value=value & ((1 << (size * 8)) - 1), pc=hex(u.reg_read(UC_X86_REG_EIP))))
                break

    def execute(self, row):
        u = self.u
        self.phase = 'setup'
        for address, raw in self.saved:
            u.mem_write(address, raw)
        u.context_restore(self.cpu)
        self.cursor = self.saved_cursor
        self.calls, self.raw_draws, self.writes, self.active_rng = [], [], [], []
        self.trace = NativeCallTrace(u, self.read32, self.calls)
        u.mem_write(0xA8ED84, dwords(row['frame']))
        for index, name in enumerate(('transport', 'passenger')):
            pointer, house = self.actors[name], self.houses[name]
            u.mem_write(house + 0x30, dwords(index, self.countries[row[name + '_country']]))
            self.invoke(0x4F6EC0, house, (row[name + '_difficulty'],))
            u.mem_write(pointer + 0x21C, dwords(house))
            u.mem_write(pointer + 0x150, struct.pack('<f', row[name + '_rank']))
            u.mem_write(pointer + 0x3B8, dwords(row[name + '_burst']))
            start, duration = row[name + '_rearm']
            u.mem_write(pointer + 0x2EC, dwords(start, 0, duration, row[name + '_charge_delay']))
        for name in ('transport', 'passenger'):
            u.mem_write(self.actors[name] + 0x274, dwords(0))
        u.mem_write(self.link + 0x24,
                    dwords(self.actors[row['manager']] if row['manager'] else 0))
        if row['manager']:
            u.mem_write(self.actors[row['manager']] + 0x274, dwords(self.link))
        if row['live_target']:
            assert row['manager'] is not None
            u.mem_write(self.link + 0x28, dwords(self.actors['victim']))
            u.mem_write(self.link + 0x48, dwords(row['warp_remaining']))
            u.mem_write(self.actors['victim'] + 0x278, dwords(self.link))
            u.mem_write(self.actors['victim'] + 0x270, b'\x01')
        self.invoke(SET_GUNNER, self.actors['transport'], (10 if row['manager'] == 'transport' else 0,))
        for name, pointer in self.rngs.items():
            self.invoke(0x65C6D0, pointer, (row['seed'] if name == 'scenario' else 0,))
        initial_rng, initial = self.rng(), self.gunner_state()
        houses = {name: dict(country=row[name + '_country'], difficulty=row[name + '_difficulty'],
                    mode=5, rof_bits=self.bits(pointer + 0x1A8, 8))
                  for name, pointer in self.houses.items()}
        operations = []
        for name in row['operations']:
            self.phase = name
            before, before_rng = self.gunner_state(), self.rng()
            self.calls, self.raw_draws, self.writes, self.active_rng = [], [], [], []
            self.trace = NativeCallTrace(u, self.read32, self.calls)
            # Fix the caller's otherwise unspecified scratch stack bytes. Native
            # callbacks copy an uninitialized word into timer+2F0; it remains a
            # recorded raw write, not a logical timer field or golden requirement.
            u.mem_write(SP - 0x1000, bytes(0x2000))
            if name == 'fire_rearm':
                from tools.spatial_oracle.techno_burst_index import START, END
                owner = self.actors['transport']
                slot = self.read32(owner + 0x138)
                self.phase = 'setup'
                weapon = self.read32(self.invoke(0x70E140, owner, (slot,)))
                self.phase = name
                u.mem_write(SP + 0x10C, dwords(slot))
                for register, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBP, SP + 0x100),
                                        (UC_X86_REG_ESI, owner), (UC_X86_REG_EBX, weapon)):
                    u.reg_write(register, value)
                run_checked(u, START, END, required_addresses=(0x6FCFA0, 0x70E140))
                entry = START
            else:
                entry = RECEIVE_GUNNER if name in ('receive', 'retry') else REMOVE_GUNNER
                self.invoke(entry, self.actors['transport'],
                            (0 if name == 'remove_null' else self.actors['passenger'],))
                self.trace.returned(RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
            assert not self.trace.pending, ('unreturned observed native call', self.trace.pending)
            after, after_rng = self.gunner_state(), self.rng()
            assert after_rng['main'] == before_rng['main'] == initial_rng['main']
            assert after_rng['mapgen'] == before_rng['mapgen'] == initial_rng['mapgen']
            if name != 'fire_rearm':
                for actor in ('transport', 'passenger'):
                    assert after[actor]['charge_delay'] == before[actor]['charge_delay']
            operations.append(dict(name=name, entry=hex(entry), frame=row['frame'],
                before=before, after=after, rng_after=after_rng,
                raw_draw_count={stream: sum(draw['stream'] == stream for draw in self.raw_draws)
                                for stream in self.rngs},
                calls=self.calls.copy(), raw_draws=self.raw_draws.copy(), writes=self.writes.copy()))
        self.phase = 'setup'
        assert self.code == bytes(u.mem_read(0x401000, 0x3E0000)), 'native code changed'
        return dict(input=row, houses=houses, initial=initial, rng_initial=initial_rng,
                    operations=operations, code_unchanged=True)


def temporal_gunner_cases():
    base = dict(frame=200, seed=31, transport_rank=0, passenger_rank=0,
        transport_country='Americans', passenger_country='Americans',
        transport_difficulty=1, passenger_difficulty=1,
        transport_burst=0, passenger_burst=0,
        transport_rearm=[190, 20], passenger_rearm=[190, 20],
        transport_charge_delay=73, passenger_charge_delay=91,
        manager='passenger', live_target=True, warp_remaining=700,
        operations=['receive', 'remove', 'retry', 'remove'])
    rows = [
        dict(name='active_rookie'),
        dict(name='expired_exact_deadline', transport_rearm=[100, 100], passenger_rearm=[100, 100]),
        dict(name='stopped_nonzero_duration', transport_rearm=[-1, 20], passenger_rearm=[-1, 20]),
        dict(name='no_manager', manager=None, live_target=False),
        dict(name='null_passenger_remove', manager='transport', operations=['remove_null']),
        dict(name='receiving_veterans', transport_rank=1, passenger_rank=1),
        dict(name='receiving_elite_transport', transport_rank=2),
        dict(name='receiving_elite_passenger', passenger_rank=2, seed=1),
        dict(name='receiving_house_context', transport_difficulty=0,
             passenger_difficulty=2, passenger_country='Russians'),
        dict(name='burst_not_modulo', transport_burst=255, passenger_burst=256),
        dict(name='burst_signed_overflow', transport_burst=2147483647, passenger_burst=2147483647),
        dict(name='actual_fire_rearm_tail', transport_rearm=[0, 1], passenger_rearm=[0, 1],
             operations=['receive', 'fire_rearm', 'remove', 'retry', 'remove']),
    ]
    return [{**base, **row} for row in rows]


def generate_temporal_gunner():
    root = Path(os.environ['VERA20K_SHRAPNEL_INPUTS'])
    map_path = Path(os.environ['VERA20K_IFV_MAP'])
    assert map_path.is_file(), 'VERA20K_IFV_MAP must be physical selected map INI text'
    assert map_path.read_bytes().lstrip().startswith((b';', b'[')), 'MIX is not map INI'
    layers = [(name, root / name) for name in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini')]
    layers.append((map_path.name, map_path))
    m = TemporalGunnerReader(root, layers)
    return dict(schema_version=1, inputs=m.inputs,
                rows=[m.execute(row) for row in temporal_gunner_cases()],
                native_code_unchanged=True)


def temporal_gunner_metadata():
    return provenance(
        scope='Original Unit Temporal gunner receive/remove/retry with real GetWeapon/GetROF, receiver rank/House context, Scenario RNG and lone live Unit LetGo',
        assumptions=[
            'Existing BulletReader owns the VM, cached physical lexical INI strings, allocation, CRT/TLS and fixed ART. Whole FV7470D0/747620 and CLEG5236A0/5240A0, ClegSequence523D00, Weapon constructors/readers772FA0/771C70/772080 and ChronoBeam75D3A0 execute. Archive loading, full registry discovery/order and Scenario layer scheduling are supplied boundaries.',
            'Rules665650, whole66D530 with only VeteranROF/GameSpeedBias/TeamDelays, wholeCombatDamage66BBB0 with only the two ROF multipliers, original difficulty readers66D270, Country5113F0/511850 and wholeHouseSetDifficulty4F6EC0 establish consumed scalar/default inputs. Full House construction, GameMode5, House indexes0/1 and chosen country/difficulty/rank contexts are supplied; cross-House context is a component contrast, not ordinary boarding admission.',
            'Concrete original Unit/Infantry vtables and type pointers with native-read full health are supplied live actor boundaries. Original InitManagers Temporal producer6F4154..6F41C0 executes actual GetWeapon/Warhead admission, allocation, Temporal71A4E0 constructor, owner binding and weapon Damage copy. Cargo admission, parent pointers, placement and successful/failed Unlimbo are not executed; receive/remove/retry represents the callback order after those existing owners admit it.',
            'Native receive selects temporary weapon slot7 (CRNuke) before final CLEG mode10 (CRNeutronRifle). Native removal samples the donor timer before manager transfer/LetGo, then queries receiver CLEG slot0 at its current rank/House/burst. No ROF, slot, rank predicate, LetGo, Mark(2) or RNG return is replaced.',
            'Explicit retained frame/duration and burst inputs bound component cases. The actual_fire_rearm_tail control additionally executes existing original6FF274..6FF2D1 with real GetROF, timer stores, charge-delay copy and signed burst remainder, starting after shot effects. Full FireAt admission/projectile/Temporal shot creation is outside that producer fragment.',
            'A supplied single-head live Unit target with remaining700 has no other managers, incoming/outgoing warp chain or building/House advisory. Original LetGo clears the target head/warped flag and calls original FootMark(2), whose immediate true arm needs no world reconstruction. Target progression, erase, other links and rendering remain separate Temporal owners.',
            'Scenario is originally seeded31 or1; Main/MapGen originally seeded0 after setup for each case. Full1012-byte logical states at the initial boundary and every operation boundary, original ranged/next calls, actual raw draws/rejections and ordered writes are retained. Raw draw counts are measured at the native consumer sites, not inferred from an entry counter.',
            'FPCW0E3F and ftol cache are supplied as in the existing ordinary Mission owner. Caller scratch stack is cleared, making native uninitialized timer+2F0 writes reproducible but not a logical timer expectation. Callbacks do not write charge delay+2F8; no complete-shot modulo is added to their INC. Signed overflow inputs2147483647 expose the actual native INC wrap and subsequent real GetROF branch.',
        ],
        substitutions=[
            'Inherited BulletReader allocator/delete/TLS/physical-file byte transport; original GUID reader Windows ASCII/CLSID services. Setup-only visual asset5F8110/5F8CE0 returns0 and CRT atexit registration is a no-op. No native gameplay/RNG result or original instruction is patched.',
        ],
        entry_points=dict(receive=RECEIVE_GUNNER, remove=REMOVE_GUNNER, set_gunner=SET_GUNNER,
            get_rof=0x6FCFA0, get_weapon=0x70E140, let_go=0x71ABC0, mark=0x4D3780,
            temporal_ctor=0x71A4E0, fire_rearm=0x6FF274, fire_rearm_end=0x6FF2D1,
            temporal_producer=0x6F4154, temporal_producer_end=0x6F41C0,
            unit_type_ctor=0x7470D0, unit_type_reader=0x747620,
            infantry_type_ctor=0x5236A0, infantry_type_reader=0x5240A0,
            weapon_reader=0x772080, rules_ctor=0x665650,
            general=0x66D530, combat=0x66BBB0, difficulties=0x674500, difficulty_reader=0x66D270,
            country_ctor=0x5113F0, country_reader=0x511850, house_difficulty=0x4F6EC0))


if __name__ == '__main__':
    if '--temporal-gunner' in sys.argv:
        finish_vectors(generate_temporal_gunner, Path(__file__).with_name('ifv_temporal_gunner.json'),
            provenance=temporal_gunner_metadata,
            argv=[arg for arg in sys.argv[1:] if arg != '--temporal-gunner'])
    else:
        finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
