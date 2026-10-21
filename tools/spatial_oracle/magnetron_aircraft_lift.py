"""Original locomotor-hit admission and installed Aircraft locomotor routing."""
from pathlib import Path
from collections import deque
import struct
import json

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_INVALID, UC_HOOK_MEM_WRITE
from unicorn.x86_const import *
from tools.native_oracle import (RET_MAGIC, run_checked, initialize_empty_windows_seh,
                                finish_vectors, provenance)
from tools.rules_oracle.bridge_anim_inputs import Reader
from tools.spatial_oracle.track_destination import make_destination_fixture
from tools.spatial_oracle.unit_scatter_state import ACTOR, TYPE, LOCO, SP
from tools.spatial_oracle.unit_entry import HOUSE, EXTRA
from tools.spatial_oracle.unit_source_scatter import CELLS, MAP, SCENARIO
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.crate_speed_effect import VTABLES, TYPE_VTABLES
from tools.spatial_oracle.house_base_projection import initialize_nearby_zone_plane
from tools.spatial_oracle.object_flight_height import initialize_object_scalars
from tools.spatial_oracle.jumpjet_coordinates import initialize_jumpjet_levels
from tools.projectile_oracle.bridge_render_inputs import BulletReader, lexical
from tools.spatial_oracle.building_body_rules import INI, RULES
from tools.spatial_oracle.fly_landing_phase import initialize_air_tracker

TYPE_KEYS = {'Strength', 'Armor', 'Size', 'Locomotor', 'ROT', 'Landable',
             'AirportBound', 'ResourceGatherer', 'Naval', 'MovementZone',
             'JumpjetTurnRate', 'JumpjetSpeed', 'JumpjetClimb', 'JumpjetCrash',
             'JumpjetHeight', 'JumpjetAccel', 'JumpjetWobbles',
             'JumpjetNoWobbles', 'JumpjetDeviation', 'JumpJet'}
FILTERS = {name: TYPE_KEYS for name in ('ORCA', 'BEAG', 'MTNK', 'E1')}
FILTERS.update(TELE=TYPE_KEYS | {'Primary', 'ElitePrimary'},
               MagneticBeam={'Damage', 'Projectile', 'Warhead', 'Range', 'MinimumRange'},
               MagneticBeamE={'Damage', 'Projectile', 'Warhead', 'Range', 'MinimumRange'},
               LocomotorBeam={'IsLocomotor', 'Locomotor', 'Verses'},
               InvisibleHigh={'Inviso', 'Image', 'SubjectToElevation'})


class LiftInputs(BulletReader):
    """Shared physical lexical cache and original selected-type/weapon readers."""
    def __init__(self):
        self.platform = []
        super().__init__({}, Path('ini'), arena_size=0x800000)
        u = self.u
        for registry in (0x887568, 0x8874C0, 0xA8EB00, 0xA83CE0, 0xA8ED40,
                         0xA8E348, 0xA83DE8, 0xA83C98, 0x8B4120, 0xB0EC60,
                         0xA8B218, 0xA8E390, 0xA83D50):
            u.mem_write(registry, dwords(0x7EB6D4, self.alloc(4096), 1024, 1, 0, 10))
        self.rules = self.alloc(0x2000)
        u.mem_write(0x8871E0, dwords(self.rules))
        self.invoke(0x665650, self.rules)
        self.types = {}
        for name in ('TELE', 'MTNK', 'ORCA', 'BEAG', 'E1'):
            p = self.alloc(0x1900)
            ctor = (0x41C8B0 if name in ('ORCA', 'BEAG') else
                    0x5236A0 if name == 'E1' else 0x7470D0)
            self.invoke(ctor, p, (self.cstring(name),))
            self.types[name] = p
        self.layers = []
        for name in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini'):
            path = Path('ini') / name
            if not path.exists():
                assert name == 'LANGRULE.INI'
                self.layers.append(dict(file=name, absent=True))
                continue
            sections, _lines = lexical(path.read_bytes(), set(FILTERS))
            sections = {n: {k: v for k, v in keys.items() if k in FILTERS[n]}
                        for n, keys in sections.items()}
            self.make_ini({})
            self.rules_cache(sections)
            admitted = {}
            for n, p in self.types.items():
                entry = (0x41CC20 if n in ('ORCA', 'BEAG') else
                         0x712170 if n == 'E1' else 0x747620)
                admitted[n] = bool(self.invoke(entry, p, (RULES,)) & 255)
            self.weapons = {n: self.invoke(0x772FA0, self.cstring(n))
                            for n in ('MagneticBeam', 'MagneticBeamE')}
            for p in self.weapons.values():
                self.invoke(0x772080, p, (RULES,))
            self.warhead = self.read32(self.weapons['MagneticBeam'] + 0xAC)
            self.invoke(0x75D3A0, self.warhead, (RULES,))
            self.layers.append(dict(file=name, sections=sections, admitted=admitted))
        self.fields = {n: dict(size=struct.unpack('<d', u.mem_read(p + 0x380, 8))[0],
                strength=self.read32(p + 0xA0),
                locomotor=bytes(u.mem_read(p + 0x34C, 16)).hex(),
                jumpjet=bytes(u.mem_read(p + 0xD70, 0x28)).hex(),
                landable=bool(u.mem_read(p + 0xE0A, 1)[0]) if n in ('ORCA', 'BEAG') else None,
                airport_bound=bool(u.mem_read(p + 0xE0D, 1)[0]) if n in ('ORCA', 'BEAG') else None)
            for n, p in self.types.items()}

    def hook(self, u, pc, size, data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if Reader.guid_transport(self, u, pc, sp, self.platform):
            return
        # Same optional asset/cache/CRT boundaries as magnetic_wave.UnitInputs.
        if pc in (0x5F8110, 0x5F8CE0, 0x7C978A):
            self.ret(0)
            return
        super().hook(u, pc, size, data)


class Fixture:
    def __init__(self, case):
        self.case = case
        self.m = LiftInputs()
        u = self.u = self.m.u
        initialize_empty_windows_seh(u)
        self.read32 = self.m.read32
        rules = self.m.rules
        self.rules = rules
        self.u, self.call, self.read32 = make_destination_fixture(
            dict(family='drive', mission=5, queued=-1, head=[0, 0, 0],
                 prior=[0, 0, 0], rules_pointer=rules), uc=u)
        self.firer = self.m.alloc(0x800)
        self.firer_type = self.m.types['TELE']
        self.bullet = self.m.alloc(0x180)
        self.warhead = self.m.warhead
        self.pad = 0
        u.mem_write(self.firer, bytes(u.mem_read(ACTOR, 0x700)))
        # The prepared objects have independent RadioClass storage, as real
        # constructors do; the pad control must not also contact the firer.
        u.mem_write(self.firer + 0xE4, dwords(self.m.alloc(16)))
        u.mem_write(self.firer + 4, dwords(0x7F5C54))
        u.mem_write(self.firer + 0x6C4, dwords(self.firer_type))
        u.mem_write(self.firer + 0x9C, dwords(1408, 2688, 0))
        u.mem_write(self.firer + 0x5A4, dwords(0))
        u.mem_write(self.firer + 0x6C, dwords(self.read32(self.firer_type + 0xA0)))
        u.mem_write(self.firer + 0x90, b'\x01')
        self.firer_loco = self.m.alloc(0x100)
        self.call(0x4AF540, self.firer_loco, [])
        self.call(0x55A710, 0, [self.firer_loco + 4, self.firer])
        u.mem_write(self.firer_loco + 0x14, dwords(1))
        u.mem_write(self.firer + 0x674, dwords(self.firer_loco + 4))
        category = case.get('category', 'aircraft')
        self.victim_type = self.m.types[case.get('type', 'ORCA')]
        u.mem_write(ACTOR, dwords(VTABLES[category], VTABLES[category] - 0x1C))
        u.mem_write(ACTOR + 0x14, dwords(7))
        u.mem_write(self.firer + 0x14, dwords(7))
        u.mem_write(ACTOR + (0x6C0 if category == 'infantry' else 0x6C4), dwords(self.victim_type))
        if category == 'aircraft':
            u.mem_write(ACTOR + 8, dwords(0x7E2280))
            u.mem_write(ACTOR + 0x6C0, dwords(0x7E2250))
            self.call(0x4CC9A0, LOCO, [])
            self.call(0x4CCA20, 0, [LOCO + 4, ACTOR])
        elif category == 'infantry':
            self.call(0x75AA90, LOCO, [])
            self.call(0x55A710, 0, [LOCO + 4, ACTOR])
        u.mem_write(LOCO + 0x14, dwords(1))
        u.mem_write(ACTOR + 0x674, dwords(LOCO + 4))
        u.mem_write(ACTOR + 0x5A0, dwords(0))
        u.mem_write(ACTOR + 0x5A4, dwords(0))
        u.mem_write(ACTOR + 0x5C4, dwords(-1))
        u.mem_write(ACTOR + 0x9C, dwords(2688, 2688, case.get('z', 0)))
        u.mem_write(ACTOR + 0x6C, dwords(self.read32(self.victim_type + 0xA0)))
        u.mem_write(ACTOR + 0x90, b'\x01')
        u.mem_write(ACTOR + 0x94, dwords(-1))
        u.mem_write(ACTOR + 0x6AD, bytes([case.get('already_lifted', False)]))
        if 'size' in case:
            u.mem_write(self.victim_type + 0x380, struct.pack('<d', case['size']))
        u.mem_write(HOUSE + 0x1EC, b'\x01')
        initialize_object_scalars(u, self.call)
        initialize_jumpjet_levels(self.call)
        # Original CRT table812B28 initializes all five Infantry placement
        # offsets used by Jumpjet's non-Unit destination arm. Sparse image
        # globals start at zero and are not a valid runtime placement table.
        assert self.read32(0x812B28) == 0x48E480
        self.call(0x48E480, 0, [])
        initialize_nearby_zone_plane(u, size=(16, 16), plane=self.m.alloc(0x20000),
                                    rows=self.m.alloc(0x100))
        u.mem_write(MAP + 0xF4, dwords(16, 16, 0, 0, 32, 32))
        u.mem_write(0x87F924, dwords(0xC00000))
        for entry in (0x561710, 0x5617A0, 0x5617C0, 0x5617E0):
            self.call(entry, 0, [])
        u.mem_write(self.bullet + 0x10C, dwords(ACTOR))
        u.mem_write(self.bullet + 0xB0, dwords(self.firer))
        self.strength = case.get('strength', self.read32(self.m.weapons['MagneticBeam'] + 0xA4))
        u.mem_write(self.bullet + 0x6C, dwords(self.strength))
        u.mem_write(self.bullet + 0x128, dwords(self.warhead))
        if case.get('docked'):
            self.pad = self.m.alloc(0x800)
            contacts = self.m.alloc(16)
            pad_type = self.m.invoke(0x4653C0, self.m.cstring('PreparedReloadPad'))
            u.mem_write(pad_type + 0x16AA, b'\x01')
            u.mem_write(self.pad, dwords(0x7E3EBC, 0x7E3EA0))
            u.mem_write(self.pad + 0x14, dwords(3))
            u.mem_write(self.pad + 0x6C, dwords(1000))
            u.mem_write(self.pad + 0x90, b'\x01')
            u.mem_write(self.pad + 0x9C, dwords(2688, 2688, 0))
            u.mem_write(self.pad + 0x21C, dwords(HOUSE))
            u.mem_write(self.pad + 0x520, dwords(pad_type))
            u.mem_write(self.pad + 0xE0, dwords(0x7E180C, contacts, 1))
            u.mem_write(contacts, dwords(ACTOR))
            u.mem_write(self.read32(ACTOR + 0xE4), dwords(self.pad))
            u.mem_write(CELLS + (10 * 32 + 10) * 0x200 + 0xE4, dwords(self.pad))
        self.trace, self.events = deque(maxlen=40), []
        self.jumpjet = None
        self.original = bytes(u.mem_read(0x401000, 0x3E0000))
        self.original_vtables = bytes(u.mem_read(0x7E1000, 0x16000))
        u.hook_add(UC_HOOK_CODE, self.observe)
        u.hook_add(UC_HOOK_MEM_INVALID, self.invalid)
        u.hook_add(UC_HOOK_MEM_WRITE, self.written)

    def written(self, u, access, address, size, value, data):
        if address == ACTOR + 0x5A4:
            self.events.append(dict(nav_write=hex(u.reg_read(UC_X86_REG_EIP)), value=self.ref(value)))

    def ref(self, pointer):
        if not pointer:
            return None
        if pointer in (ACTOR, self.firer, self.pad):
            return {'object': {ACTOR: 'victim', self.firer: 'firer', self.pad: 'pad'}[pointer]}
        if CELLS <= pointer < CELLS + 32 * 32 * 0x200:
            return {'cell': list(struct.unpack('<hh', self.u.mem_read(pointer + 0x24, 4)))}
        raise AssertionError(f'Unclassified object reference {pointer:#x}')

    def invalid(self, u, access, address, size, value, data):
        print('INVALID', hex(address), 'PC', hex(u.reg_read(UC_X86_REG_EIP)),
              'TRACE', [hex(x) for x in self.trace])
        return False

    def observe(self, u, pc, size, data):
        self.trace.append(pc)
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc in (0x4B0500, 0x4CCB40, 0x54AEC0, 0x4D3920):
            raise AssertionError(f'Full movement/search outside fixture boundary: {pc:#x}')
        if pc in (0x41C27D, 0x41C2CB):
            clsid, outer, context, iid, ppv = struct.unpack('<5I', u.mem_read(sp, 20))
            assert (outer, context) == (0, 7)
            assert bytes(u.mem_read(clsid, 16)) == bytes(u.mem_read(0x7E9AC0, 16))
            self.events.append('COM_original_jumpjet_factory')
            u.mem_write(sp, dwords(pc + 6, 0, outer, iid, ppv))
            u.reg_write(UC_X86_REG_EIP, 0x6C4190)
        elif pc == 0x41C28C:
            self.events.append('OS_OleRun')
            u.reg_write(UC_X86_REG_EAX, 0)
            u.reg_write(UC_X86_REG_ESP, sp + 4)
            u.reg_write(UC_X86_REG_EIP, pc + 6)
        if pc == 0x54AC40:
            self.jumpjet = u.reg_read(UC_X86_REG_ECX)
        if pc in (0x710000, 0x41AA80, 0x4D94B0, 0x54AD30, 0x54B1C0,
                  0x54DA00, 0x70FEE0, 0x54DA50, 0x54DB00, 0x56DC20,
                  0x6FCD70, 0x65A970):
            self.events.append(hex(pc))
        if pc == 0x54B1C0:
            interface, x, y, z = struct.unpack('<4I', u.mem_read(sp + 4, 16))
            assert interface == self.jumpjet + 4
            self.events.append(dict(move_args=dict(interface='jumpjet', coordinate=[x, y, z])))

    def snapshot(self):
        u = self.u
        values = lambda a, n: list(struct.unpack('<' + 'i' * n, u.mem_read(a, 4 * n)))
        active = self.read32(ACTOR + 0x674)
        active_kind = 'jumpjet' if self.jumpjet and active == self.jumpjet + 4 else self.case.get('category', 'aircraft')
        active_kind = {'aircraft': 'fly', 'unit': 'drive', 'infantry': 'walk'}.get(active_kind, active_kind)
        return dict(position=values(ACTOR + 0x9C, 3), health=self.read32(ACTOR + 0x6C),
                    nav=self.ref(self.read32(ACTOR + 0x5A4)),
                    source=self.ref(self.read32(ACTOR + 0x2B0)), target=self.ref(self.read32(self.firer + 0x2AC)),
                    lifted=bool(u.mem_read(ACTOR + 0x6AD, 1)[0]),
                    released=bool(u.mem_read(ACTOR + 0x6AE, 1)[0]),
                    crashing=bool(u.mem_read(ACTOR + 0x425, 1)[0]),
                    manipulated=bool(u.mem_read(ACTOR + 0x427, 1)[0]),
                    credit=self.ref(self.read32(ACTOR + 0x428)),
                    active_kind=active_kind,
                    contact=self.ref(self.read32(self.read32(ACTOR + 0xE4))),
                    pad_contact=self.ref(self.read32(self.read32(self.pad + 0xE4))) if self.pad else None,
                    jumpjet=None if self.jumpjet is None else dict(
                        destination=values(self.jumpjet + 0x40, 3),
                        moving=bool(u.mem_read(self.jumpjet + 0x4C, 1)[0]),
                        phase=self.read32(self.jumpjet + 0x50),
                        suspended_kind=('fly' if self.case.get('category', 'aircraft') == 'aircraft' else 'drive')
                        if self.read32(self.jumpjet + 0x94) else None),
                    mission=values(ACTOR + 0xAC, 1)[0], queued=values(ACTOR + 0xB4, 1)[0])

    def execute(self):
        before = self.snapshot()
        self.u.reg_write(UC_X86_REG_ESI, self.bullet)
        self.u.reg_write(UC_X86_REG_ECX, ACTOR)
        self.u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(self.u, 0x46963A, 0x469AA4, count=1000000)
        assert self.original == bytes(self.u.mem_read(0x401000, 0x3E0000))
        row = dict(input={'type': 'ORCA', 'category': 'aircraft', 'z': 0, **self.case},
                   before=before, after_hit=self.snapshot())
        if 'release_height' in self.case:
            self.u.mem_write(ACTOR + 0xA4, dwords(self.case['release_height']))
            row['before_release'] = self.snapshot()
            self.u.mem_write(SP, dwords(RET_MAGIC, 0))
            self.u.reg_write(UC_X86_REG_ECX, self.firer)
            self.u.reg_write(UC_X86_REG_ESP, SP)
            run_checked(self.u, 0x70FEE0, RET_MAGIC, count=2000000)
            row['after_release'] = self.snapshot()
            if self.case['release_height'] == 0:
                row['after_restore_gate'] = self.restore_gate()
                self.u.mem_write(self.jumpjet + 0x4C, b'\x00')
                row['before_supplied_settled_restore'] = self.snapshot()
                row['after_supplied_settled_restore'] = self.restore_gate()
        row['events'] = self.events
        self.call(0x65C780, SCENARIO + 0x218, [])
        row['next_random'] = self.u.reg_read(UC_X86_REG_EAX)
        assert self.original == bytes(self.u.mem_read(0x401000, 0x3E0000))
        assert self.original_vtables == bytes(self.u.mem_read(0x7E1000, 0x16000))
        return row

    def restore_gate(self):
        u = self.u
        u.reg_write(UC_X86_REG_ESI, ACTOR)
        u.reg_write(UC_X86_REG_EBX, 0)
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, 0x4DAE5F, 0x4DAEC6, count=100000)
        run_checked(u, 0x4DAEF6, 0x4DAF00, count=100000)
        return self.snapshot()

    def landing_control(self, land):
        self.execute()
        u = self.u
        cell = CELLS + (10 * 32 + 10) * 0x200
        u.mem_write(cell + 0xEC, dwords(land))
        result = dict(input=dict(name='clear' if land == 0 else 'impassable',
                                 land_type=land, speed_rows=[1.0 if land == 0 else 0.0] * 9))
        def observe(u, pc, size, data):
            if pc == 0x54CE3D:
                result['can_enter_cell'] = u.reg_read(UC_X86_REG_EAX)
            elif pc == 0x54CE61:
                result['wheel_clear'] = bool(u.reg_read(UC_X86_REG_EAX) & 255)
        hook = u.hook_add(UC_HOOK_CODE, observe)
        u.reg_write(UC_X86_REG_EDI, self.jumpjet)
        u.reg_write(UC_X86_REG_ESI, cell)
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, 0x54CE26, 0x54CE6A, count=100000)
        result['entry'] = u.reg_read(UC_X86_REG_EBP)
        u.hook_del(hook)
        self.assert_immutable()
        return result

    def assert_immutable(self):
        assert self.original == bytes(self.u.mem_read(0x401000, 0x3E0000))
        assert self.original_vtables == bytes(self.u.mem_read(0x7E1000, 0x16000))

    def impact_control(self, case):
        """Original terminal dispatch/Notice body, observing declared bullet/lifecycle leaves."""
        self.execute()
        u = self.u
        u.mem_write(ACTOR + 0x6AD, bytes([case['lifted']]))
        u.mem_write(ACTOR + 0x6C, dwords(case['health']))
        u.mem_write(ACTOR + 0xA4, dwords(case['z']))
        u.mem_write(ACTOR + 0x428, dwords(self.firer if case['credit'] else 0))
        u.mem_write(self.rules + 0xF68, bytes([case['current_strength_damage']]))
        u.mem_write(self.rules + 0xF64, struct.pack('<f', case['multiplier']))
        c4, death_weapon, projectile = (self.m.alloc(0x100) for _ in range(3))
        u.mem_write(self.rules + 0xFA8, dwords(c4))
        u.mem_write(self.rules + 0xFDC, dwords(death_weapon))
        u.mem_write(death_weapon + 0xA0, dwords(projectile))
        effect_bullet = self.m.alloc(0x180)
        u.mem_write(effect_bullet, dwords(0x7E46E4))
        bullet_virtuals = {self.read32(0x7E46E4 + 0xD4): ('bullet_mark', 0),
                           self.read32(0x7E46E4 + 8): ('bullet_release', 4)}
        calls = []
        def ret(pops, eax=0):
            sp = u.reg_read(UC_X86_REG_ESP)
            u.reg_write(UC_X86_REG_EAX, eax)
            u.reg_write(UC_X86_REG_ESP, sp + 4 + pops)
            u.reg_write(UC_X86_REG_EIP, self.read32(sp))
        def observe(u, pc, size, data):
            sp = u.reg_read(UC_X86_REG_ESP)
            if pc == 0x46B050:
                owner, damage, wh, speed, bright = struct.unpack('<5I', u.mem_read(sp + 4, 20))
                assert u.reg_read(UC_X86_REG_ECX) == projectile and wh == c4
                calls.append(dict(call='create_bullet', target=self.ref(u.reg_read(UC_X86_REG_EDX)),
                                  owner=self.ref(owner), damage=damage, warhead='rules_c4',
                                  speed=speed, bright=bool(bright)))
                ret(20, effect_bullet)
            elif pc == 0x4690B0:
                calls.append(dict(call='detonate', coordinate=list(struct.unpack(
                    '<3i', u.mem_read(self.read32(sp + 4), 12)))))
                ret(4)
            elif pc in bullet_virtuals and (u.reg_read(UC_X86_REG_ECX) == effect_bullet or
                                           self.read32(sp + 4) == effect_bullet):
                name, pops = bullet_virtuals[pc]
                calls.append(dict(call=name))
                ret(pops)
            elif pc == 0x702D40:
                calls.append(dict(call='record_kill', killer=self.ref(self.read32(sp + 4))))
                ret(4)
            elif pc == 0x4DE5D0:
                calls.append(dict(call='uninit'))
                ret(0)
        hook = u.hook_add(UC_HOOK_CODE, observe)
        initialize_air_tracker(u, SP, buffers=self.m.alloc(0x10000), extent=(32, 32))
        u.reg_write(UC_X86_REG_EDI, self.jumpjet)
        u.reg_write(UC_X86_REG_EBX, 0)
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, 0x54D045, 0x41BC30, count=100000)
        dispatch = dict(receiver='aircraft_notice', interface_offset=u.reg_read(UC_X86_REG_ECX) - ACTOR,
                        arguments=list(struct.unpack('<2I', u.mem_read(u.reg_read(UC_X86_REG_ESP) + 4, 8))),
                        phase=self.read32(self.jumpjet + 0x50))
        run_checked(u, 0x41BC30, 0x54D097, count=100000)
        returned = u.reg_read(UC_X86_REG_EAX) & 255
        u.hook_del(hook)
        self.assert_immutable()
        return dict(input=case, dispatch=dispatch, return_value=returned, calls=calls)


def generate():
    cases = [dict(name='landed_orca'), dict(name='landed_beag', type='BEAG'),
             dict(name='prepared_reload_pad', docked=True),
             dict(name='airborne_orca', z=300),
             dict(name='unit_regression', type='MTNK', category='unit'),
             dict(name='infantry_refused', type='E1', category='infantry'),
             dict(name='strength_equals_size_refused', strength=1),
             dict(name='strength_exceeds_size', strength=2),
             dict(name='ground_release', release_height=0),
             dict(name='supplied_altitude_release', release_height=300)]
    fixtures = [Fixture(case) for case in cases]
    model = fixtures[0].m
    return dict(schema_version=1,
                inputs=dict(seed=1, firer_position=[1408, 2688, 0], victim_position=[2688, 2688, 0],
                            mission=5, queued=-1, map_size=[16, 16], allocated_size=[32, 32],
                            clear_speed_rows=[1.0] * 9, blocked_speed_rows=[0.0] * 9,
                            layers=model.layers, types=model.fields,
                            placement_offsets=[list(struct.unpack('<3i', model.u.mem_read(0x89E9F0 + i * 12, 12)))
                                               for i in range(5)],
                            magnetic_beam_damage=model.read32(model.weapons['MagneticBeam'] + 0xA4)),
                cases=[f.execute() for f in fixtures],
                landing_controls=[Fixture(dict(name='landing_gate')).landing_control(land) for land in (0, 1)],
                impact_controls=[Fixture(dict(name='impact')).impact_control(case) for case in impact_cases()])


def impact_cases():
    base = dict(lifted=True, current_strength_damage=False, health=60, multiplier=1.5, z=300, credit=True)
    return [dict(base, name='unlifted_ignores_notice', lifted=False),
            dict(base, name='type_strength_two_c4'),
            dict(base, name='current_strength_two_c4', current_strength_damage=True),
            dict(base, name='negative_damage_clamps', multiplier=-1.5),
            dict(base, name='no_manipulator_self_owner', credit=False)]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=provenance(
        scope='Original locomotor hit admission joined to Imbue and Aircraft/Foot/Jumpjet destination; bounded release, restore, aircraft landing gate and terminal impact dispatch/Notice body.',
        assumptions=[
            'Selected physical RULESMD/LANGRULE/MPBattleMD lexical fields pass original class/type/weapon readers; E1 uses common TechnoType reader for the early Infantry refusal.',
            'Prepared live objects on original vtables, original Drive/Fly/Walk constructors, flat clear supplied map and zone plane; no full object constructor, map loading, shot acquisition, movement Process or full flight is claimed.',
            'Original CRT initializer48E480 from table812B28 produces the five Infantry placement offsets; original Scenario RNG is seeded1 and each hit history records the next original Random result after its last snapshot.',
            'Reload-pad row supplies reciprocal contact with a constructed synthetic UnitReload building type.',
            'Altitude release supplies XYZ.Z after hit. Settled restore supplies only Jumpjet moving=false after ground release; neither is evidence of a complete fall.',
            'Landing rows enter State5 at its CanEnterCell/wheel-clear join with EDI=installed Jumpjet and ESI=prepared owner cell. Post-hit Foot+6B6 is1 (original Imbue7101CF); cell ground/deck occupation and first member are0. Earlier State5 movement/Mark work is omitted. Terminal impact rows enter the reached-landing tail with original AirTracker initialized; lifted/health/height/credit and damage rules are explicit inputs.'
        ], substitutions=[
            'Shared original-reader fixture supplies CRT allocation, TLS, INI cache and optional asset/cache boundaries.',
            'CoCreateInstance transport enters original Jumpjet COM factory/constructor/QueryInterface; OleRun returns S_OK. Gameplay bodies and original vtables remain unchanged.',
            'Impact rows supply distinct Rules C4/death-weapon/projectile identities. Bullet Create returns a prepared Bullet-vtable object; its mark, detonation and release are observed no-op leaves. RecordKill and UnInit are observed no-op leaves. No explosion damage propagation, deletion or post-impact map state is claimed.'
        ], entry_points=dict(hit=0x46963A, hit_end=0x469AA4, imbue=0x710000,
                             aircraft_destination=0x41AA80, foot_destination=0x4D94B0,
                             jumpjet_move_to=0x54B1C0, release=0x70FEE0,
                             restore_gate=0x4DAE5F, landing_gate=0x54CE26,
                             landing_gate_end=0x54CE6A, cell_clear=0x4834A0,
                             terminal_impact=0x54D045, aircraft_notice=0x41BC30,
                             notice_return=0x54D097, placement_offsets_init=0x48E480,
                             random=0x65C780)))
