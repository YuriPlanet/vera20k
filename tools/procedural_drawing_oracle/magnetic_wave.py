"""Original stock TELE Wave3 components; no Python wave or pixel algorithm.

Native readers, Unit/Drive FLH, constructor/update/AI and packed draws execute
against existing Reader/LaserPixels owners; magnetic_wave.meta.json records the
actor, cached-INI, map, scheduler and surface boundaries. Import is inert.

Replay from the repository root with PYTHONPATH=.:tools/spatial_oracle,
VERA20K_GAMEMD_EXE naming the original executable and
VERA20K_PROJECTILE_RENDER_ASSETS naming extracted RULESMD.INI, MPBattleMD.ini,
ARTMD.INI and optional LANGRULE.INI:

    python -B -m tools.procedural_drawing_oracle.magnetic_wave --check
"""

from pathlib import Path
import hashlib
import os
import struct
import sys

from unicorn import UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI,
    UC_X86_REG_ESP,
)

from tools import native_oracle as native
from tools.native_slope import initialize_slope_tables
from tools.procedural_drawing_oracle import electric_bolt as electric, rally
from tools.projectile_oracle.bridge_render_inputs import BulletReader, assets_root, lexical
from tools.projectile_oracle.ifv_fire_coord import native_get_flh
from tools.projectile_oracle.ifv_impact import (
    initialize_effect_registries, initialize_effect_world, retirement_transport,
)
from tools.rules_oracle.bridge_anim_inputs import Reader
from tools.rules_oracle.bridge_anim_lists import HEAP
from tools.spatial_oracle import building_prism as prism, map_queries
from tools.spatial_oracle.building_body_rules import INI, RULES, SP

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
RETAINED_MAP = HERE / 'magnetic_wave' / 'ordinary-tele-magnetic.map'
MAP_SHA256 = '25e1567d85d2d0cd4e47ad4b734a97f8519e34466d33a6b3970e2b37c62cc74b'
CTOR, UPDATE, AI, DRAW, MAGNETIC, NONMAGNETIC = (
    0x75E950, 0x762AF0, 0x760F50, 0x75F9F0, 0x762070, 0x761640)
PIXEL, TABLE_INIT, ZERO_COORD, OFFSETS, Z_BUFFER = (
    0x760190, 0x75F020, 0x75E8F0, 0x75F140, 0x7BD130)
VECTOR, MAGNETIC_TABLE, DISTANCE_TABLE, BRIGHTNESS = (
    0xA8EC38, 0xB45E68, 0xB4669C, 0xB46648)
MAGNETIC_END, SONIC_TABLE, SONIC_END, DISTANCE_END = (
    0xB46248, 0xB46254, 0xB4663C, 0xB725BC)
TAIL_BEGIN, TAIL_END = 0x6FF5F5, 0x6FF656
UNIT_VTABLE, DRIVE_VTABLE = 0x7F5C70, 0x7E7EB0
TYPE_KEYS = {'Image', 'Strength', 'Armor', 'Size', 'Locomotor', 'Turret'}
TELE_KEYS = TYPE_KEYS | {'Primary', 'ElitePrimary'}
ART_KEYS = {'Image', 'Voxel', 'TurretOffset', 'PrimaryFireFLH'}
WEAPON_KEYS = {'Damage', 'ROF', 'Range', 'MinimumRange', 'Speed', 'Projectile',
               'Warhead', 'IsSonic', 'IsLaser', 'IsElectricBolt', 'IsRadBeam',
               'IsRadEruption', 'IsMagBeam'}
WEAPONS = ('MagneticBeam', 'MagneticBeamE')
WARHEAD_KEYS = {'IsLocomotor', 'Locomotor', 'Verses'}
PROJECTILE_KEYS = {'Inviso', 'Image', 'SubjectToElevation'}
JUMPJET_KEYS = {'JumpjetTurnRate', 'JumpjetSpeed', 'JumpjetClimb', 'JumpjetCrash',
               'JumpjetHeight', 'JumpjetAccel', 'JumpjetWobbles',
               'JumpjetNoWobbles', 'JumpjetDeviation', 'JumpJet'}
FILTERS = {'TELE': TELE_KEYS, 'MTNK': TYPE_KEYS | JUMPJET_KEYS,
           'LocomotorBeam': WARHEAD_KEYS, 'InvisibleHigh': PROJECTILE_KEYS,
           **{name: WEAPON_KEYS for name in WEAPONS}}
READ_KEYS = set().union(*FILTERS.values(), ART_KEYS)
COMPARES = {
    0x7604E5: ('fast', UC_X86_REG_EBX, UC_X86_REG_ECX),
    0x760638: ('fallback', UC_X86_REG_EAX, UC_X86_REG_ECX),
    0x7607DE: ('fast', UC_X86_REG_ECX, UC_X86_REG_EAX),
    0x76093D: ('fallback', UC_X86_REG_EAX, UC_X86_REG_ECX),
}
CLIP_BRANCHES = {0x76043D: 'fast', 0x760557: 'fallback',
                 0x7606FC: 'fast', 0x760853: 'fallback'}
CALLS = {
    CTOR: ('constructor', 5, 20), UPDATE: ('update', 0, 0),
    AI: ('ai', 0, 0), 0x6F3AD0: ('unit_flh', 5, 20),
    MAGNETIC: ('magnetic_geometry', 2, 8),
    NONMAGNETIC: ('nonmagnetic_geometry', 2, 8),
    0x4AFF60: ('drive_matrix', 3, 12), 0x75F230: ('direction', 0, 0),
    0x410230: ('create_id', 1, 4), 0x75F8B0: ('unlimbo', 2, 8),
    0x75F980: ('limbo', 0, 0), 0x5F65F0: ('uninit', 0, 0),
    0x7258D0: ('dispatch_expiry', 0, 0),  # ECX=expired, DL=removed.
    0x6DA560: ('tactical_pointer_expired', 2, 8),
    0x75F610: ('wave_pointer_expired', 2, 8),
    0x7610F0: ('update_cells', 0, 0),
    0x75F540: ('right_edge_lerp', 2, 8),
    0x7077C0: ('techno_pointer_expired', 2, 8),
    0x763200: ('scalar_dtor', 1, 4),
}
words, u32, ints, signed = electric.words, electric.u32, electric.ints, electric.signed


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def unchanged(u, pointer, before, label):
    """Exact-span guard with a bounded byte witness, not a native output."""
    after = bytes(u.mem_read(pointer, len(before)))
    if before != after:
        offset = native.changed_byte_spans(before, after)[0][0]
        end = min(offset + 16, len(before))
        raise AssertionError(dict(object=label, pointer=hex(pointer),
            span_bytes=len(before), first_offset=hex(offset),
            before=before[offset:end].hex(), after=after[offset:end].hex()))


def u16(u, pointer):
    return struct.unpack('<H', u.mem_read(pointer, 2))[0]


def selected(raw):
    sections, lines = lexical(raw, set(FILTERS))
    return ({name: {key: value for key, value in values.items() if key in FILTERS[name]}
             for name, values in sections.items()},
            [row for row in lines if row['key'] in FILTERS[row['section']]])


def physical_files(root, map_path):
    raw = map_path.read_bytes()
    if digest(raw) != MAP_SHA256 or raw != RETAINED_MAP.read_bytes():
        raise ValueError('C32 map must equal the exact retained TELE/MTNK input')
    files = []
    for name in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini'):
        p = root / name
        if not p.exists() and name != 'LANGRULE.INI':
            raise ValueError('missing required physical layer: ' + name)
        files.append((name, p.read_bytes() if p.exists() else None))
    files.append((RETAINED_MAP.relative_to(REPO).as_posix(), raw))
    return files


def identities(root, map_path):
    return [(name, None if raw is None else digest(raw))
            for name, raw in [*physical_files(root, map_path),
                              ('ARTMD.INI', (root / 'ARTMD.INI').read_bytes())]]


def wave_state(u, p):
    """Offset identities, independent of historical Ghidra endpoint labels."""
    count, polygon = u32(u, p + 0x148), u32(u, p + 0x14C)
    if count not in (0, 4, 6):
        raise ValueError(('unexpected native polygon size', count))
    cells, cell_count = u32(u, p + 0x1F4), u32(u, p + 0x200)
    assert cell_count <= 128, ('unexpected component cell count', cell_count)
    return dict(
        wave_type=u32(u, p + 0xB0), native_id=signed(u32(u, p + 0x10)),
        source=ints(u, p + 0xB4, 3) if count else None,
        target=ints(u, p + 0xC0, 3) if count else None,
        object_location=ints(u, p + 0x9C, 3),
        source_point=ints(u, p + 0xCC, 2) if count else None,
        target_point=ints(u, p + 0xD4, 2) if count else None,
        corners=[ints(u, p + 0xFC + i * 12, 3) for i in range(4)] if count else [],
        corner_points=[ints(u, p + 0xDC + i * 8, 2) for i in range(4)] if count else [],
        polygon=[ints(u, polygon + i * 8, 2) for i in range(count)] if polygon else [],
        direction=signed(u32(u, p + 0x1CC)) if count else None,
        phase=signed(u32(u, p + 0x130)), restart=signed(u32(u, p + 0x134)),
        front_f64_le_hex=bytes(u.mem_read(p + 0x138, 8)).hex(),
        back_f64_le_hex=bytes(u.mem_read(p + 0x140, 8)).hex(),
        active=bool(u.mem_read(p + 0x12C, 1)[0]),
        decay=bool(u.mem_read(p + 0x12D, 1)[0]),
        alive=bool(u.mem_read(p + 0x90, 1)[0]),
        in_limbo=bool(u.mem_read(p + 0x81, 1)[0]),
        owner_present=bool(u32(u, p + 0x1D4)),
        target_present=bool(u32(u, p + 0xAC)),
        cells=[list(struct.unpack('<hh', u.mem_read(u32(u, cells + i * 4) + 0x24, 4)))
               for i in range(cell_count)])


class UnitInputs(BulletReader):
    """Existing cached-INI/CRT/TLS/allocator owner, selected physical fields."""
    def __init__(self, root, map_path):
        self.phase, self.layer = 'setup', 'constructor'
        self.platform, self.selected_reads = {}, []
        self.allocation_spans = {}
        self.types, self.weapons, self.warheads, self.projectiles = {}, {}, {}, {}
        self.layers = physical_files(root, map_path)
        self.art_raw = (root / 'ARTMD.INI').read_bytes()
        art, art_lines = lexical(self.art_raw, {'TELE', 'GTNK'})
        self.art = {name: {key: value for key, value in values.items() if key in ART_KEYS}
                    for name, values in art.items()}
        self.art_lines = [row for row in art_lines if row['key'] in ART_KEYS]
        super().__init__(self.art, root, arena_size=0x800000)
        u = self.u
        self.text = bytes(u.mem_read(prism.TEXT_BEGIN, prism.TEXT_SIZE))
        for registry in (0x887568, 0x8874C0, 0xA8EB00, 0xA83CE0, 0xA8ED40,
                         0xA8E348, 0xA83DE8, 0xA83C98, 0x8B4120, 0xB0EC60):
            u.mem_write(registry, words(0x7EB6D4, self.alloc(4096), 1024, 1, 0, 10))
        self.rules = self.alloc(0x2000)
        u.mem_write(0x8871E0, words(self.rules))
        self.invoke(0x665650, self.rules)
        for name in ('TELE', 'MTNK'):
            p = self.alloc(0x1900)
            self.invoke(0x7470D0, p, (self.cstring(name),))
            self.types[name] = p
        self.inputs = dict(constructor=self.fields(), layers=[],
            art=dict(file='ARTMD.INI', bytes=len(self.art_raw),
                     sha256=digest(self.art_raw), sections=self.art,
                     source_lines=self.art_lines))
        for name, raw in self.layers:
            self.layer = name
            if raw is None:
                self.inputs['layers'].append(dict(file=name, absent=True))
                continue
            sections, lines = selected(raw)
            self.make_ini(self.art)
            self.rules_cache(sections)
            mark, new = len(self.selected_reads), {}
            admitted = {n: bool(self.invoke(0x747620, p, (RULES,)) & 255)
                        for n, p in self.types.items()}
            for offset in (0x898, 0xA94):
                p = self.read32(self.types['TELE'] + offset)
                if p:
                    label = self.reference(p)
                    assert label in WEAPONS, label
                    if label not in self.weapons:
                        self.weapons[label] = p
                        new.setdefault('weapons', {})[label] = self.weapon_fields(p)
            for p in self.weapons.values():
                self.invoke(0x772080, p, (RULES,))
                for offset, store, key in ((0xAC, self.warheads, 'warheads'),
                                            (0xA0, self.projectiles, 'projectiles')):
                    q = self.read32(p + offset)
                    if q and self.reference(q) not in store:
                        label = self.reference(q)
                        store[label] = q
                        new.setdefault(key, {})[label] = (
                            self.warhead_fields(q) if key == 'warheads' else self.projectile_fields(q))
            for p in self.warheads.values():
                self.invoke(0x75D3A0, p, (RULES,))
            for p in self.projectiles.values():
                self.invoke(0x46BEE0, p, (RULES,))
            self.inputs['layers'].append(dict(file=name, bytes=len(raw), sha256=digest(raw),
                sections=sections, source_lines=lines, admitted=admitted,
                newly_constructed=new, reads=self.selected_reads[mark:], after=self.fields()))
        final = self.fields()
        assert set(self.weapons) == set(WEAPONS)
        assert all(w['is_mag_beam'] and not any(w[k] for k in (
            'is_sonic', 'is_laser', 'is_electric_bolt', 'is_rad_beam', 'is_rad_eruption'))
            for w in final['weapons'].values())
        assert final['warheads']['LocomotorBeam']['is_locomotor']
        assert final['projectiles']['InvisibleHigh']['inviso']
        assert all(t['locomotor_clsid_hex'] == '4127584a3998d111b70900a024ddafd1'
                   for t in final['types'].values())
        assert final['warheads']['LocomotorBeam']['locomotor_clsid_hex'] == (
            '462c61921ff7d111ac9f006008055bb5')
        self.inputs.update(final=final, platform_boundaries=dict(self.platform))
        physical_before = [(f'{kind}:{name}', p,
            bytes(u.mem_read(p, self.allocation_spans[p]['requested_bytes'])))
            for kind, objects in (('unit_type', self.types), ('weapon', self.weapons))
            for name, p in objects.items()]
        self.defaults()  # Reader self-check; Rust reads no default rows.
        for label, pointer, before in physical_before:
            unchanged(u, pointer, before, 'default controls: readonly ' + label)
        self.false_weapon = self.invoke(0x772FA0, self.cstring('C32ConstructorFalse'))
        self.inputs['false_flag_constructor'] = self.weapon_fields(self.false_weapon)
        self.source, self.target, self.temp = self.alloc(0x1000), self.alloc(0x1000), self.alloc(64)
        self.locomotors = []
        for p, name in ((self.source, 'TELE'), (self.target, 'MTNK')):
            # Supplied native Unit shell, not native Unit construction/placement.
            u.mem_write(p, words(UNIT_VTABLE))
            u.mem_write(p + 0x14, words(7))
            u.mem_write(p + 0x6C4, words(self.types[name]))
            u.mem_write(p + 0x90, b'\1')
            # Whole7077C0 unconditionally invokes history(+470).FindIndex,
            # even when empty. Execute the original three-vector constructor
            # region without the unrelated Techno constructor/RNG prefix.
            u.reg_write(UC_X86_REG_ESI, p)
            u.reg_write(UC_X86_REG_EBX, 0)
            native.run_checked(u, 0x6F3041, 0x6F30D6, count=100,
                required_addresses=(0x6F3041, 0x6F30C4),
                context=dict(component='magnetic_wave', case='actor target-vector setup',
                             actor=name, storage=hex(p)))
            self.inputs.setdefault('actor_vector_setup', []).append(dict(actor=name,
                entry=hex(0x6F3041), stop=hex(0x6F30D6),
                headers={label: bytes(u.mem_read(p + offset, 24)).hex()
                         for label, offset in (('scores', 0x440), ('candidates', 0x458),
                                               ('history', 0x470))}))
            loco = self.alloc(0x100)
            self.invoke(0x4AF540, loco)
            u.mem_write(loco + 0xC, words(p))
            u.mem_write(loco + 0x14, words(1))
            u.mem_write(p + 0x674, words(loco + 4))
            assert self.read32(loco + 4) == DRIVE_VTABLE
            assert self.read32(DRIVE_VTABLE + 0x24) == 0x4AFF60
            self.locomotors.append(loco)
        assert self.text == bytes(u.mem_read(prism.TEXT_BEGIN, prism.TEXT_SIZE))
        self.inputs['component_allocations'] = self.component_allocations()

    def alloc(self, size):
        # Observe the inherited owner; never reproduce its cursor policy.
        p = Reader.alloc(self, size)
        self.record_allocation(p, size, self.cursor - p, 'Reader.alloc')
        return p

    def record_allocation(self, pointer, requested, reserved, owner, caller=None):
        if not requested:
            return
        assert pointer not in self.allocation_spans, ('reused component allocation', hex(pointer))
        assert 0 < requested <= reserved
        self.allocation_spans[pointer] = dict(requested_bytes=requested,
            reserved_bytes=reserved, owner=owner, caller=caller)

    def readonly_specs(self):
        # 772FF6..77300A requests exactly160h before whole Weapon771C70.
        # Unit/Drive storage below is explicitly requested through Reader.alloc.
        specs = [(f'unit_type:{name}', p, 0x1900) for name, p in self.types.items()]
        specs += [(f'weapon:{name}', p, 0x160) for name, p in self.weapons.items()]
        specs += [('weapon:C32CurrentDefault', self.default_weapon, 0x160),
                  ('weapon:C32ConstructorFalse', self.false_weapon, 0x160)]
        specs += [(f'drive:{name}', p, 0x100)
                  for name, p in zip(('TELE', 'MTNK'), self.locomotors)]
        for label, p, size in specs:
            allocation = self.allocation_spans[p]
            assert allocation['requested_bytes'] == size, (label, hex(p), allocation, size)
            assert all(q == p or p + size <= q or q + a['reserved_bytes'] <= p
                       for q, a in self.allocation_spans.items()), ('overlapping guard allocation', label)
        assert len({p for _label, p, _size in specs}) == len(specs), 'aliased readonly objects'
        return specs

    def component_allocations(self):
        specs = [*self.readonly_specs(), ('actor:TELE', self.source, 0x1000),
                 ('actor:MTNK', self.target, 0x1000), ('scratch:flh', self.temp, 64)]
        return {label: dict(pointer=hex(p), **self.allocation_spans[p])
                for label, p, _size in specs}

    def hook(self, u, pc, size, user):
        sp = u.reg_read(UC_X86_REG_ESP)
        allocation = ((self.cursor, self.read32(sp + 4), self.read32(sp))
                      if pc == 0x7C8E17 else None)
        if Reader.guid_transport(self, u, pc, sp, self.events):
            self.platform['GUID'] = self.platform.get('GUID', 0) + 1
            return
        if pc in (0x5F8110, 0x5F8CE0, 0x7C978A):
            self.platform[hex(pc)] = self.platform.get(hex(pc), 0) + 1
            self.ret(0)
            return
        if pc in (0x5276D0, 0x5295F0, 0x528A10, 0x5283D0, 0x529CA0, 0x527920,
                  0x474810, 0x474620):
            key = self.string(self.read32(sp + 8))
            if key in READ_KEYS:
                row = dict(file=self.layer, reader=hex(pc),
                    section=self.string(self.read32(sp + 4)), key=key,
                    raw_default_words=[self.read32(sp + n) for n in (12, 16)])
                if pc == 0x528A10:
                    default = self.read32(sp + 12)
                    row['default_text'] = self.string(default) if default else None
                self.selected_reads.append(row)
        assert pc not in (0x65C780, 0x65C7E0), ('reader/setup RNG', hex(pc))
        super().hook(u, pc, size, user)
        if allocation is not None:
            pointer, requested, caller = allocation
            assert u.reg_read(UC_X86_REG_EAX) == pointer
            self.record_allocation(pointer, requested, self.cursor - pointer,
                                   'Reader/Lists7C8E17', hex(caller))

    def reference(self, p):
        return self.string(p + 0x24) if p else None

    def weapon_fields(self, p):
        # Original7722EE..77235B stores parsed Speed/ROF/Range/MinimumRange.
        # Speed here is the reader result, before unrelated7729F0 postprocessing.
        return dict(damage=signed(self.read32(p + 0xA4)),
            reader_speed=signed(self.read32(p + 0xA8)), rof=signed(self.read32(p + 0xB0)),
            range_leptons=signed(self.read32(p + 0xB4)),
            minimum_range_leptons=signed(self.read32(p + 0xB8)),
            projectile=self.reference(self.read32(p + 0xA0)),
            warhead=self.reference(self.read32(p + 0xAC)),
            **{name: bool(self.u.mem_read(p + offset, 1)[0]) for name, offset in (
                ('is_sonic', 0x130), ('is_laser', 0x149), ('is_electric_bolt', 0x151),
                ('is_rad_beam', 0x154), ('is_rad_eruption', 0x155), ('is_mag_beam', 0x15C))})

    def warhead_fields(self, p):
        return dict(is_locomotor=bool(self.u.mem_read(p + 0x15B, 1)[0]),
                    locomotor_clsid_hex=bytes(self.u.mem_read(p + 0x15C, 16)).hex())

    def projectile_fields(self, p):
        return dict(inviso=bool(self.u.mem_read(p + 0x29E, 1)[0]),
                    subject_to_elevation=bool(self.u.mem_read(p + 0x297, 1)[0]))

    def fields(self):
        return dict(types={name: dict(primary=self.reference(self.read32(p + 0x898)),
                elite_primary=self.reference(self.read32(p + 0xA94)),
                primary_flh=ints(self.u, p + 0x89C, 3),
                turret_offset=signed(self.read32(p + 0x720)),
                strength=signed(self.read32(p + 0xA0)),
                locomotor_clsid_hex=bytes(self.u.mem_read(p + 0x34C, 16)).hex(),
                jumpjet_block_hex=bytes(self.u.mem_read(p + 0xD70, 0x28)).hex())
                for name, p in self.types.items()},
            weapons={name: self.weapon_fields(p) for name, p in self.weapons.items()},
            warheads={name: self.warhead_fields(p) for name, p in self.warheads.items()},
            projectiles={name: self.projectile_fields(p) for name, p in self.projectiles.items()})

    def defaults(self):
        p = self.invoke(0x772FA0, self.cstring('C32CurrentDefault'))
        self.default_weapon = p
        rows = [dict(name='constructor', sections=None, after=self.weapon_fields(p))]
        for name, keys in (
            ('present_missing_false', {'Damage': '1'}),
            ('present_enabled', {'IsMagBeam': 'yes'}),
            ('present_missing_retains_true', {'Damage': '1'}),
        ):
            self.layer = 'default_control/' + name
            sections = {'C32CurrentDefault': keys}
            self.rules_cache(sections)
            mark = len(self.selected_reads)
            before = self.weapon_fields(p)
            admitted = self.invoke(0x772080, p, (RULES,)) & 255
            rows.append(dict(name=name, sections=sections, before=before,
                admitted=bool(admitted), reads=self.selected_reads[mark:], after=self.weapon_fields(p)))
        return rows


class WaveMachine(prism.LaserPixels):
    """One shared surface VM, copied original-reader arena, unchanged native code."""
    alloc, cstring, string, invoke = Reader.alloc, Reader.cstring, Reader.string, Reader.invoke
    return_boundary = electric.ElectricPixels.return_boundary

    def read32(self, p):
        return u32(self.u, p)

    def __init__(self, donor, case=None):
        self.measuring, self.phase = False, 'surface_setup'
        super().__init__(dict(case or {}, size=(80, 80), seed=31))
        u = self.u
        self.arena_size, self.cursor = donor.arena_size, donor.cursor
        u.mem_map(HEAP, self.arena_size)
        u.mem_write(HEAP, bytes(donor.u.mem_read(HEAP, self.arena_size)))
        self.rules, self.source, self.target = donor.rules, donor.source, donor.target
        self.types, self.weapons, self.locomotors = donor.types, donor.weapons, donor.locomotors
        self.false_weapon = donor.false_weapon
        self.temp, self.scenario = donor.temp, donor.read32(0xA8B230)
        u.mem_write(0xA8B230, words(self.scenario))
        u.mem_write(0x8871E0, words(self.rules))
        self.events, self.deleted, self.wave_pointers = [], [], []
        self.geometry_arguments = {}
        self.current_case = dict(name='wave_setup')
        self.records, self.calls_pending = [], None
        self.initial_geometry, self.constructing = None, None
        self.operations, self.comparisons, self.draws = [], [], []
        self.current_operation, self.current_draw, self.last_compare = None, None, None
        self.current_cell_call = None
        self.current_update_call = None
        self.transport_calls = {}
        self.lookup_writes = []
        u.hook_add(UC_HOOK_MEM_WRITE, self.observe_write)
        u.hook_add(UC_HOOK_MEM_READ, self.observe_read)
        initialize_slope_tables(u, SP, native.RET_MAGIC)
        initialize_effect_registries(self)
        initialize_effect_world(self, {}, clear_terrain=False)
        self.invoke(0x40CB80, 0)
        self.invoke(0x4A8630, 0)
        # Actual native Cell ctor, declared sparse flat component topology.
        u.mem_write(map_queries.TABLE, map_queries.EMPTY_TABLE)
        u.mem_write(0x87F924, words(map_queries.TABLE, 0x40000))
        self.cells = {}
        for y in range(36, 61):
            for x in range(36, 61):
                p = self.alloc(0x148)
                self.invoke(0x47BBF0, p)
                u.mem_write(p + 0x24, struct.pack('<hh', x, y))
                u.mem_write(map_queries.TABLE + (y * 512 + x) * 4, words(p))
                self.cells[x, y] = p
        self.invoke(0x47BBF0, map_queries.DUMMY)
        # Original startup constants; no host projected/matrix output.
        for address in (0x561710, 0x5617A0, 0x5617C0, 0x5617E0):
            self.invoke(address, 0)
        self.invoke(0x489100, 0)
        u.mem_write(0x87F7E8 + 0xFC, words(0, 0, 64, 64))
        u.mem_write(0x87F7E8 + 0xF4, words(64))
        u.mem_write(0xA8E7AC, words(0))
        u.mem_write(0x887324, words(rally.TACTICAL))
        # The native expiry broadcaster reaches Tactical+28. Execute its
        # original four-vptr header stores, not a supplied cleanup callback.
        # Existing selection_navigation owns the same literal/callback boundary.
        u.reg_write(UC_X86_REG_ESI, rally.TACTICAL)
        native.run_checked(u, 0x6D1D54, 0x6D1D6F, count=4,
            required_addresses=(0x6D1D54,), context=dict(component='magnetic_wave',
                case='Tactical type-header setup', storage=hex(rally.TACTICAL)))
        assert self.read32(rally.TACTICAL) == 0x7F4348
        assert self.read32(0x7F4348 + 0x28) == 0x6DA560
        u.reg_write(UC_X86_REG_ESI, rally.TACTICAL)
        u.reg_write(UC_X86_REG_EBX, 0)
        native.run_checked(u, 0x6D1DC5, 0x6D1E1E, count=30)
        u.mem_write(rally.TACTICAL + 0xB0, words(0, 0))
        u.mem_write(0x88731C, words(rally.SURFACE))
        u.mem_write(0x887314, words(rally.SURFACE))
        u.mem_write(0x886FA0, words(0, 0, *self.surface.size))
        u.mem_write(0xA8ED84, words(200))
        # Original Wave registry initializer4E67E0, including typed growth.
        self.invoke(0x4E67E0, 0)
        # Object's full destructor searches all five listener registries,
        # including the type-listener vector missing from inherited IFV setup.
        # Preserve every established header; execute only this original prefix
        # on pristine missing storage, stopping before CRT atexit registration.
        registry_bases = (0xB0F698, 0xA8E360, 0xB0F720, 0xB0F670, 0xB0F618, VECTOR)
        registry_before = {p: bytes(u.mem_read(p, 24)) for p in registry_bases}
        missing_type_listener = self.read32(0xB0F670) == 0
        if missing_type_listener:
            assert registry_before[0xB0F670] == bytes(24), 'partial missing listener header'
            u.reg_write(UC_X86_REG_ESP, SP)
            native.run_checked(u, 0x725350, 0x725386, count=20,
                required_addresses=(0x72536D,), context=dict(component='magnetic_wave',
                    case='Object retirement type-listener setup', registry='0xb0f670'))
        for pointer, before in registry_before.items():
            if pointer != 0xB0F670 or not missing_type_listener:
                unchanged(u, pointer, before, 'retirement setup: preserve registry header')
        assert self.read32(0xB0F670) == 0x7E91EC
        donor.inputs['object_retirement_registry_setup'] = dict(
            entry=hex(0x725350), stop_before_atexit=hex(0x725386),
            executed=missing_type_listener,
            headers=[dict(address=hex(p), before=registry_before[p].hex(),
                          after=bytes(u.mem_read(p, 24)).hex()) for p in registry_bases])
        native.initialize_empty_windows_seh(u)
        self.phase = 'table_init'
        # Whole original Wave header startup sequence. Its native table owns
        # diagonal/scaling constants and the six-u16 zero-coordinate gap.
        self.wave_startup = list(struct.unpack('<14I', u.mem_read(0x815798, 56)))
        assert self.wave_startup[0] == 0x75E740 and self.wave_startup[-1] == 0x75E930
        assert ZERO_COORD in self.wave_startup
        for address in self.wave_startup:
            assert 0x75E740 <= address <= 0x75E930
            self.invoke(address, 0)
        self.invoke(TABLE_INIT, 0, instruction_limit=15_000_000, timeout_us=60_000_000)
        self.tables = self.table_outputs()
        self.phase = 'setup'
        self.trace = electric.RngTrace(u, self.scenario, 31)
        self.rng_initial = self.trace.states()
        self.lookup_snapshot = bytes(u.mem_read(MAGNETIC_TABLE, DISTANCE_END - MAGNETIC_TABLE))
        self.readonly_objects = {p: dict(label=label, allocation=donor.allocation_spans[p],
                before=bytes(u.mem_read(p, size))) for label, p, size in donor.readonly_specs()}
        self.vtables = {p: bytes(u.mem_read(p, n)) for p, n in (
            (UNIT_VTABLE, 0x5A0), (DRIVE_VTABLE, 0xC8), (0x7F6BF4, 0x1D0),
            (0x7E4EEC, 0x100), (rally.VTABLE, 0x84), (0x7E2070, 0x84),
            (0x7F4348, 0x2C), (0x7E91EC, 0x20), (0x7E4E78, 0x20))}
        assert self.read32(0x7E91EC + 0x10) == 0x4E0550
        self.measuring = True
        self.set_pose()

    def table_outputs(self):
        u = self.u
        return dict(
            distance=dict(address=hex(DISTANCE_TABLE), shape=[300, 300],
                encoding='u16_le_hex', hex=bytes(u.mem_read(DISTANCE_TABLE, 180000)).hex()),
            magnetic=dict(address=hex(MAGNETIC_TABLE), count=496, encoding='i16_le_hex',
                hex=bytes(u.mem_read(MAGNETIC_TABLE, 992)).hex()),
            magnetic_window=dict(address=hex(MAGNETIC_TABLE), count=1002,
                encoding='i16_le_hex', hex=bytes(u.mem_read(MAGNETIC_TABLE, 2004)).hex()),
            brightness=dict(address=hex(BRIGHTNESS), values=ints(u, BRIGHTNESS, 13)),
            adjacent=dict(address=hex(MAGNETIC_END), encoding='bytes_hex',
                hex=bytes(u.mem_read(MAGNETIC_END, DISTANCE_TABLE - MAGNETIC_END)).hex(),
                zero_coord=[hex(MAGNETIC_END), hex(SONIC_TABLE)],
                sonic=[hex(SONIC_TABLE), hex(SONIC_END)],
                brightness=[hex(BRIGHTNESS), hex(BRIGHTNESS + 52)]),
            startup_calls=[hex(p) for p in self.wave_startup],
            native_writer_ranges=[dict(begin=hex(a), end_exclusive=hex(b), writes=n)
                                  for a, b, n in self.writer_ranges()])

    def writer_ranges(self):
        ranges = []
        for address, size in sorted(self.lookup_writes):
            if ranges and address <= ranges[-1][1]:
                a, b, n = ranges[-1]
                ranges[-1] = (a, max(b, address + size), n + 1)
            else:
                ranges.append((address, address + size, 1))
        return ranges

    def observe_write(self, u, _access, address, size, _value, _user):
        if self.phase == 'table_init' and MAGNETIC_TABLE <= address < DISTANCE_END:
            self.lookup_writes.append((address, size))
        if self.phase in ('raster', 'pixel_component'):
            assert all(not (address < p + self.pixel_bytes and p < address + size)
                       for p in (self.z_pixels, rally.ALPHA)), 'native Wave wrote A/Z'

    def observe_read(self, _u, _access, address, size, _value, _user):
        if self.phase in ('raster', 'pixel_component'):
            assert not (address < rally.ALPHA + self.pixel_bytes
                        and rally.ALPHA < address + size), 'native Wave read A'

    def complete_calls(self, pc, sp):
        """Use the shared return matcher also at the runner's stop boundary."""
        for index in self.calls_pending.returned(pc, sp):
            row = self.records[index]
            if row['name'] == 'unit_flh':
                row['coordinate'] = ints(self.u, row['args'][0], 3)
            if row['name'] == 'drive_matrix':
                row['matrix_f32_le_hex'] = bytes(self.u.mem_read(row['args'][1], 48)).hex()
            if row['name'] == 'constructor':
                self.constructing = None
            if row['name'] == 'right_edge_lerp':
                row['coordinate'] = ints(self.u, int(row['ecx'], 16), 3)
            if row['name'] == 'update_cells':
                row['cells'] = wave_state(self.u, int(row['ecx'], 16))['cells']
                self.current_cell_call = None
            if row['name'] == 'update':
                self.current_update_call = None

    def observe(self, u, pc, size, user):
        # Existing BSurface/allocator setup. This hook replaces no math/draw body.
        super().observe(u, pc, size, user)
        if not hasattr(self, 'cursor'):
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc in (prism.OPERATOR_NEW, prism.OPERATOR_DELETE, prism.ATEXIT, 0x7D140B):
            label = hex(pc)
            self.transport_calls[label] = self.transport_calls.get(label, 0) + 1
        if pc == prism.OPERATOR_NEW:
            self.return_boundary(value=self.alloc(u32(u, sp + 4)))
            return
        if pc == prism.OPERATOR_DELETE:
            self.deleted.append(u32(u, sp + 4))
            self.return_boundary()
            return
        if pc == 0x7D140B:
            # Same existing Reader CRT TLS cell; geometry itself does not call it.
            self.return_boundary(value=HEAP + 0x8000)
            return
        if retirement_transport(self, pc, self.events):
            return
        if not self.measuring:
            return
        assert pc not in (0x5F8110, 0x5F8CE0, 0x5B40B0, 0x527AF9, 0x527B0C,
                          electric.CTOR, electric.INIT, electric.MANAGER,
                          prism.LASER_DRAW_ALL, 0x62DC50), ('unrelated seam in measured Wave', hex(pc))
        assert pc not in (0x65C780, 0x65C7E0), ('Wave RNG', hex(pc))
        if self.calls_pending is not None:
            self.complete_calls(pc, sp)
            if pc in CALLS:
                index = self.calls_pending.entered(pc, sp, CALLS[pc])
                row = self.records[index]
                if pc == CTOR:
                    self.constructing = u.reg_read(UC_X86_REG_ECX)
                    row['coordinates'] = [ints(u, row['args'][i], 3) for i in (0, 1)]
                if pc in (MAGNETIC, NONMAGNETIC):
                    row['coordinates'] = [ints(u, pointer, 3) for pointer in row['args']]
                if pc == 0x75F230:
                    row['points'] = [ints(u, u.reg_read(reg), 2)
                                     for reg in (UC_X86_REG_ECX, UC_X86_REG_EDX)]
                if pc == 0x7258D0:
                    row['removed_dl'] = u.reg_read(UC_X86_REG_EDX) & 255
                if pc == 0x7610F0:
                    row['samples'] = []
                    self.current_cell_call = index
                if pc == UPDATE:
                    row['phase_thresholds'] = []
                    self.current_update_call = index
                if pc == 0x75F540:
                    row.update(first=ints(u, u.reg_read(UC_X86_REG_EDX), 3),
                        second=ints(u, row['args'][0], 3),
                        t_f32_le_hex=bytes(u.mem_read(sp + 8, 4)).hex())
        if self.current_update_call is not None and pc in (0x762B3A, 0x762B8F):
            self.records[self.current_update_call]['phase_thresholds'].append(dict(
                native_pc=hex(pc), low32=u.reg_read(UC_X86_REG_EAX),
                high32=u.reg_read(UC_X86_REG_EDX)))
        if self.current_cell_call is not None:
            samples = self.records[self.current_cell_call]['samples']
            if pc in (0x76117F, 0x761381):
                samples.append(dict(native_pc=hex(pc),
                    t_f64_le_hex=bytes(u.mem_read(sp + 0x40, 8)).hex(), points=[]))
            if pc == 0x7614CB:
                samples[-1]['right_edge_t_f32_le_hex'] = bytes(u.mem_read(sp + 0x20, 4)).hex()
            if pc in (0x7611E0, 0x76140D, 0x761528):
                samples[-1]['points'].append(dict(native_pc=hex(pc),
                    coordinate=ints(u, u.reg_read(UC_X86_REG_EAX), 3)))
        if pc == 0x75EBC5 and self.constructing:
            self.initial_geometry = wave_state(u, self.constructing)
        if self.phase not in ('raster', 'pixel_component'):
            return
        if pc in CLIP_BRANCHES:
            self.current_draw['clip_paths'].add(CLIP_BRANCHES[pc])
        if pc == OFFSETS:
            p = u.reg_read(UC_X86_REG_ECX)
            count, ptr = u32(u, p + 0x1A0), u32(u, p + 0x1A8)
            assert count < 1024
            self.current_draw['spans'] = dict(top=signed(u32(u, p + 0x1A4)),
                rows=[ints(u, ptr + i * 8, 2) for i in range(count)] if ptr else [])
        if pc in COMPARES:
            path, pointer_reg, candidate_reg = COMPARES[pc]
            ptr, candidate = u.reg_read(pointer_reg), u.reg_read(candidate_reg) & 0xFFFF
            self.last_compare = dict(compare_pc=hex(pc), clip_path=path,
                candidate_z=candidate, stored_z=u16(u, ptr), z_pointer=ptr)
            self.comparisons.append(self.last_compare)
            self.current_draw['clip_paths'].add(path)
        if pc == PIXEL:
            x, offset_x, abs_y, dest = ints(u, sp + 4, 4)
            index = (dest - rally.PIXELS) // 2
            assert rally.PIXELS <= dest < rally.PIXELS + self.pixel_bytes
            assert self.last_compare is not None
            self.current_operation = dict(point=[index % self.surface.size[0], index // self.surface.size[0]],
                native_x=x, draw_offset_x=offset_x, abs_y=abs_y,
                destination_word_before=u16(u, dest), **self.last_compare)
            self.operations.append(self.current_operation)
        if self.current_operation is not None:
            row = self.current_operation
            if pc == 0x7601C7:
                index = u.reg_read(UC_X86_REG_EDX)
                address = (DISTANCE_TABLE + index * 2) & 0xFFFFFFFF
                row.update(distance_index=index, distance_address=hex(address),
                    distance_value=u16(u, address), distance_in_domain=index < 90000)
            elif pc == 0x7601D1:
                index = u.reg_read(UC_X86_REG_EAX)
                address = (MAGNETIC_TABLE + index * 2) & 0xFFFFFFFF
                row.update(sine_index=index, sine_address=hex(address),
                    sine_value=struct.unpack('<h', u.mem_read(address, 2))[0],
                    sine_in_magnetic_domain=index < 496)
            elif pc == 0x7601E8:
                row['displacement_index'] = u.reg_read(UC_X86_REG_EAX)
            elif pc == 0x7601F4:
                index = u.reg_read(UC_X86_REG_EAX)
                row['brightness'] = signed(u32(u, BRIGHTNESS + index * 4))
            elif pc == 0x7601FB:
                dest, offset = u.reg_read(UC_X86_REG_EBX), signed(u.reg_read(UC_X86_REG_ECX))
                address = dest + offset * 2
                index = (address - rally.PIXELS) // 2
                row.update(source_linear_index=index, source_word=u16(u, address),
                    source=[index % self.surface.size[0], index // self.surface.size[0]],
                    source_in_plane=0 <= index < self.pixel_bytes // 2)
            elif pc == 0x7602D0:
                row['destination_word'] = u.reg_read(UC_X86_REG_EDI) & 0xFFFF
                self.current_operation = None

    def checked(self, entry, obj, args=(), *, limit=2_000_000):
        actor_before = [bytes(self.u.mem_read(p, 0x1000)) for p in (self.source, self.target)]
        id_before = self.read32(self.scenario + 0x214)
        self.records = []
        self.calls_pending = native.NativeCallTrace(self.u, self.read32, self.records)
        context = dict(component='magnetic_wave', case=dict(self.current_case),
            entry=hex(entry), object=hex(obj), args=[hex(value & 0xFFFFFFFF) for value in args])
        try:
            value = self.invoke(entry, obj, args, instruction_limit=limit, context=context)
        except native.NativeExecutionError as error:
            # Keep the shared runner's diagnostics/report intact, and make the
            # exact caller visible even when no failure directory is requested.
            error.args = (f'{error}; component_case={context["case"]!r}',)
            raise
        self.complete_calls(native.RET_MAGIC, self.u.reg_read(UC_X86_REG_ESP))
        assert not self.calls_pending.pending
        self.calls_pending = None
        if entry not in (0x7077C0, 0x763200):
            assert actor_before == [bytes(self.u.mem_read(p, 0x1000))
                                    for p in (self.source, self.target)]
        if entry != CTOR:
            assert id_before == self.read32(self.scenario + 0x214)
        self.check(stage=f'native {entry:#x} {self.current_case!r}')
        return value

    def check(self, *, stage='checkpoint'):
        assert digest(self.u.mem_read(prism.TEXT_BEGIN, prism.TEXT_SIZE)) == self.text_sha256
        for p, raw in self.vtables.items():
            unchanged(self.u, p, raw, f'{stage}: vtable')
        for p, row in self.readonly_objects.items():
            unchanged(self.u, p, row['before'],
                f"{stage}: readonly {row['label']} ({row['allocation']['owner']})")
        unchanged(self.u, MAGNETIC_TABLE, self.lookup_snapshot, f'{stage}: lookup tables')
        assert not self.trace.requests and not self.trace.pending
        assert self.trace.states() == self.rng_initial

    def set_pose(self, *, source=(11392, 12416, 0), target=(12928, 12416, 0), heading=0x4000):
        u = self.u
        for p, xyz in ((self.source, source), (self.target, target)):
            u.mem_write(p + 0x9C, words(*xyz))
            for offset in (0x388, 0x3A0):
                self.invoke(0x4C91E0, p + offset, (0,))
                u.mem_write(self.temp, words(heading))
                self.invoke(0x4C9300, p + offset, (self.temp,))
        u.mem_write(self.source + 0x2B4, words(self.target))
        self.check(stage='set_pose')
        return dict(source=list(source), target=list(target), heading=heading,
                    source_type='TELE', target_type='MTNK', rank='rookie', burst=0)

    def flh(self):
        before = bytes(self.u.mem_read(self.source, 0x1000))
        self.records = []
        self.calls_pending = native.NativeCallTrace(self.u, self.read32, self.records)
        native_get_flh(self, self.source, self.temp)
        self.complete_calls(native.RET_MAGIC, self.u.reg_read(UC_X86_REG_ESP))
        assert not self.calls_pending.pending
        self.calls_pending = None
        assert before == bytes(self.u.mem_read(self.source, 0x1000))
        self.check(stage='GetFLH')
        return ints(self.u, self.temp, 3)

    def continuation_inputs(self):
        source_flh = self.flh()
        out = self.alloc(12)
        entry = self.read32(self.read32(self.target) + 0x58)
        self.checked(entry, self.target, (out,))
        return dict(source_flh=source_flh, target_center=ints(self.u, out, 3),
                    target_center_getter=hex(entry))

    def registered(self):
        return [u32(self.u, u32(self.u, VECTOR + 4) + i * 4)
                for i in range(u32(self.u, VECTOR + 0x10))]

    def birth(self, heading=0x4000):
        self.current_case = dict(name=f'physical_heading_{heading:04x}', heading=heading)
        pose = self.set_pose(heading=heading)
        u = self.u
        muzzle = self.flh()
        flh_calls = list(self.records)
        target_out = self.alloc(12)
        self.invoke(self.read32(UNIT_VTABLE + 0xA4), self.target, (target_out,))
        target = ints(u, target_out, 3)
        u.mem_write(self.source + 0x324, words(0))
        actors_before = [bytes(u.mem_read(p, 0x1000)) for p in (self.source, self.target)]
        id_before, count_before = self.read32(self.scenario + 0x214), len(self.registered())
        self.fire_tail(self.weapons['MagneticBeam'], muzzle, target,
                       required=(CTOR, MAGNETIC, UPDATE, 0x6F3AD0, 0x4AFF60))
        p = self.read32(self.source + 0x324)
        assert p in self.registered() and len(self.registered()) == count_before + 1
        self.wave_pointers.append(p)
        self.geometry_arguments[p] = (3, target, muzzle)
        after_source = bytearray(u.mem_read(self.source, 0x1000))
        after_source[0x324:0x328] = actors_before[0][0x324:0x328]
        assert bytes(after_source) == actors_before[0]
        assert bytes(u.mem_read(self.target, 0x1000)) == actors_before[1]
        self.check()
        return p, dict(input=dict(name=f'physical_heading_{heading:04x}', **pose,
                source_flh=muzzle, target_aim=target), initial_geometry=self.initial_geometry,
            after=wave_state(u, p), registered_before=count_before, registered_after=len(self.registered()),
            id_before=id_before, id_after=self.read32(self.scenario + 0x214),
            flh_calls=flh_calls, calls=self.records, actors_unchanged_except_wave_link=True)

    def fire_tail(self, weapon, muzzle, target, *, required=()):
        u = self.u
        # FireAt's launch-source (GetFLH, 0x6FE274) and target-coordinate
        # (0x6FE204) locals; 0x6FF63C passes &+0x88 first, &+0x44 second.
        u.mem_write(SP + 0x44, words(*muzzle))
        u.mem_write(SP + 0x88, words(*target))
        u.mem_write(SP, words(native.RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ESI, self.source)
        u.reg_write(UC_X86_REG_EDI, self.target)
        u.reg_write(UC_X86_REG_EBX, weapon)
        self.records, self.initial_geometry = [], None
        self.calls_pending = native.NativeCallTrace(u, self.read32, self.records)
        native.run_checked(u, TAIL_BEGIN, TAIL_END, count=2_000_000,
                           required_addresses=required)
        assert not self.calls_pending.pending
        self.calls_pending = None
        self.check()

    def admission_controls(self):
        rows = []
        for name, weapon in (('constructor_false_flag', self.false_weapon),
                             ('physical_mag_beam_busy', self.weapons['MagneticBeam'])):
            actors = [bytes(self.u.mem_read(p, 0x1000)) for p in (self.source, self.target)]
            before, counter = self.registered(), self.read32(self.scenario + 0x214)
            self.fire_tail(weapon, [0, 0, 0], [0, 0, 0])
            assert before == self.registered() and counter == self.read32(self.scenario + 0x214)
            assert actors == [bytes(self.u.mem_read(p, 0x1000)) for p in (self.source, self.target)]
            assert not any(row['name'] == 'constructor' for row in self.records)
            rows.append(dict(name=name, supplied_unused_coordinates=[[0, 0, 0], [0, 0, 0]],
                wave_link_present=bool(self.read32(self.source + 0x324)),
                registered_before=len(before), registered_after=len(self.registered()),
                native_id_unchanged=True, actors_unchanged=True, calls=self.records))
        return rows

    def construct(self, wave_type, first, second, *, target_pointer=None, name='constructor'):
        self.current_case = dict(name=name, wave_type=wave_type,
            raw_first=list(first), raw_second=list(second),
            target_kind='Unit' if target_pointer is None else 'supplied non-Unit')
        a, b, p = self.alloc(12), self.alloc(12), self.alloc(0x240)
        self.u.mem_write(a, words(*first))
        self.u.mem_write(b, words(*second))
        self.initial_geometry = None
        id_before = self.read32(self.scenario + 0x214)
        self.checked(CTOR, p, (a, b, self.source, wave_type,
                              self.target if target_pointer is None else target_pointer))
        self.last_constructor_ids = dict(id_before=id_before,
            id_after=self.read32(self.scenario + 0x214))
        if self.initial_geometry is None:
            assert self.last_constructor_ids['id_before'] == self.last_constructor_ids['id_after']
        self.wave_pointers.append(p)
        self.geometry_arguments[p] = (wave_type, list(first), list(second))
        return p, self.initial_geometry

    def advance(self, p, count=1):
        for _ in range(count):
            self.checked(AI, p)
        return wave_state(self.u, p)

    def phase_control(self, wave_type, phase):
        """Supply one phase dword, then let the unchanged whole Update decide."""
        pose = self.set_pose()
        context = self.continuation_inputs()
        first, second = pose['source'], pose['target']
        p, initial = self.construct(wave_type, first, second, name=f'phase_{phase}')
        self.current_case['supplied_phase'] = phase
        self.u.mem_write(p + 0x130, words(phase))
        before = wave_state(self.u, p)
        self.checked(UPDATE, p)
        return dict(input=dict(wave_type=wave_type, phase=phase,
                constructor_source=first, constructor_target=second, **pose, **context),
            initial_geometry=initial, before=before, after=wave_state(self.u, p),
            calls=self.records)

    def update_only_history(self, first, second, name):
        """Type0 Update/UpdateCells without the AI's separate damage owner."""
        pose = self.set_pose()
        context = self.continuation_inputs()
        p, initial = self.construct(0, first, second, name=name)
        history = [dict(visit=0, after=wave_state(self.u, p), calls=self.records)]
        for visit in range(1, 9):
            self.current_case.update(stage='active Update', visit=visit)
            self.checked(UPDATE, p)
            if visit in (1, 7, 8):
                history.append(dict(visit=visit, after=wave_state(self.u, p), calls=self.records))
        assert history[-1]['after']['active'], 'type0 control never reached active release seam'
        self.u.mem_write(self.source + 0x2B4, words(0))
        fade = []
        for visit in range(1, 24):
            self.current_case.update(stage='external clear fade', visit=visit)
            self.checked(UPDATE, p)
            after = wave_state(self.u, p)
            fade.append(dict(visit=visit, after=after, calls=self.records))
            if not after['alive']:
                break
        assert fade and not fade[-1]['after']['alive'], 'type0 component fade did not retire'
        return dict(name=name, input=dict(wave_type=0, constructor_source=first,
                constructor_target=second, **pose, **context), initial_geometry=initial,
            history=history, fade=fade,
            supplied_release='clear owner+2B4 after eight whole Update calls; no AI/damage/Move/lift execution')

    def direct_geometry(self, wave_type, first, second):
        """Whole native helper on admitted storage, including masked zero-XY."""
        admitted = geometry_cases()[0]
        p, initial = self.construct(wave_type, admitted['source'], admitted['target'],
                                    name='direct_zero_xy_storage_admission')
        before = wave_state(self.u, p)
        a, b = self.alloc(12), self.alloc(12)
        self.u.mem_write(a, words(*first))
        self.u.mem_write(b, words(*second))
        self.current_case.update(name='direct_zero_xy_geometry', raw_first=list(first),
                                 raw_second=list(second))
        self.checked(MAGNETIC if wave_type == 3 else NONMAGNETIC, p, (a, b))
        self.geometry_arguments[p] = (wave_type, list(first), list(second))
        return p, dict(input=dict(name='zero_xy_admitted_storage', wave_type=wave_type,
                source=list(first), target=list(second), constructor_source=admitted['source'],
                constructor_target=admitted['target']), initial_geometry=initial,
            before=before, after=wave_state(self.u, p), calls=self.records)

    def pointer_controls(self, p):
        rows = []
        for name, expired in (('unrelated', self.temp), ('target', self.target), ('owner', self.source)):
            self.current_case = dict(name='wave_pointer_expired_' + name, stage='wave callback',
                wave_type=3, receiver=hex(p), expired=hex(expired), removed=False)
            before = wave_state(self.u, p)
            self.checked(0x75F610, p, (expired, 0))
            rows.append(dict(name=name, before=before, after=wave_state(self.u, p), calls=self.records))
        return rows

    def retire(self, p):
        """Original callbacks/destructor; actor callback delivery is component input."""
        u = self.u
        rows = []
        for name, expired in (('unrelated_wave_link', self.temp), ('exact_wave_link', p)):
            self.current_case = dict(name=name, stage='Techno Wave-link callback',
                receiver=hex(self.source), expired=hex(expired), removed=False)
            before = self.read32(self.source + 0x324)
            self.checked(0x7077C0, self.source, (expired, 0))
            rows.append(dict(name=name, link_before_is_wave=before == p,
                link_after_is_wave=self.read32(self.source + 0x324) == p,
                link_after_null=self.read32(self.source + 0x324) == 0,
                calls=self.records))
        before, deleted = len(self.registered()), len(self.deleted)
        self.current_case = dict(name='wave_scalar_destructor', stage='registry retirement',
                                wave_type=3, receiver=hex(p), deleting=True)
        self.checked(0x763200, p, (1,))
        assert p not in self.registered() and len(self.registered()) == before - 1
        assert p in self.deleted[deleted:]
        return dict(techno_callbacks=rows, registered_before=before,
            registered_after=len(self.registered()), deleted_wave=True, calls=self.records,
            callback_delivery='supplied direct Techno callback; whole actor listener publication excluded')

    def lookup_controls(self, p):
        """Full original Pixel on declared arguments; no draw/Z admission claim."""
        u = self.u
        source_point = wave_state(u, p)['source_point']
        dest = rally.PIXELS + (40 * self.surface.size[0] + 40) * 2
        original_phase = bytes(u.mem_read(p + 0x130, 4))
        u.mem_write(p + 0x130, words(100))
        wave_before = bytes(u.mem_read(p, 0x240))
        rows = []
        for name, dx, abs_y in (('linear_x300_alias', 300, 0),
                                ('linear_x600_alias', 600, 0),
                                ('last_distance_cell', 299, 299),
                                ('linear_last_cell_alias', 599, 298)):
            self.current_case = dict(name=name, wave_type=3, phase=100,
                                     delta_x=dx, abs_y=abs_y)
            before = bytes(u.mem_read(rally.PIXELS, self.pixel_bytes))
            self.operations, self.current_operation = [], None
            self.last_compare = dict(compare_pc=None, clip_path=None,
                                     candidate_z=None, stored_z=None, z_pointer=None)
            self.phase = 'pixel_component'
            args = (source_point[0] + dx, 0, abs_y, dest)
            self.checked(PIXEL, p, args)
            self.phase = 'setup'
            assert self.current_operation is None and len(self.operations) == 1
            rows.append(dict(input=dict(name=name, delta_x=dx, abs_y=abs_y,
                draw_offset_x=0, phase=wave_state(u, p)['phase'],
                source_point=source_point, destination=[40, 40], format='rgb565',
                size=list(self.surface.size), pitch_words=self.surface.size[0],
                direction=wave_state(u, p)['direction'],
                native_displacement_words=ints(u, p + 0x208, 10)),
                operation=self.operations[0]))
            assert wave_before == bytes(u.mem_read(p, 0x240))
            u.mem_write(rally.PIXELS, before)
        u.mem_write(p + 0x130, original_phase)
        return rows

    def prepare_raster_geometry(self, pointers):
        """Choose a camera input, then obtain every projected output natively."""
        u = self.u
        u.mem_write(rally.TACTICAL + 0xB0, words(0, 0))
        for p in pointers:
            wave_type, first, second = self.geometry_arguments[p]
            assert wave_type == 3
            a, b = self.alloc(12), self.alloc(12)
            u.mem_write(a, words(*first))
            u.mem_write(b, words(*second))
            self.checked(MAGNETIC, p, (a, b))
        points = [point for p in pointers for point in wave_state(u, p)['polygon']]
        if points:
            xs, ys = zip(*points)
            camera = [(min(xs) + max(xs)) // 2 - self.surface.size[0] // 2,
                      (min(ys) + max(ys)) // 2 - self.surface.size[1] // 2]
        else:
            camera = [0, 0]
        u.mem_write(rally.TACTICAL + 0xB0, words(*camera))
        for p in pointers:
            _, first, second = self.geometry_arguments[p]
            a, b = self.alloc(12), self.alloc(12)
            u.mem_write(a, words(*first))
            u.mem_write(b, words(*second))
            self.checked(MAGNETIC, p, (a, b))
        return camera

    def raster(self, pointers, *, name, format='rgb565', ring_rows=0,
               alpha='mixed', z_words=None, clip=None):
        self.current_case = dict(name=name, wave_type=3,
            raw_geometry=[self.geometry_arguments[p] for p in pointers])
        u, (w, h) = self.u, self.surface.size
        if format not in ('rgb565', 'rgb555'):
            raise ValueError('unsupported component packed format')
        camera = self.prepare_raster_geometry(pointers)
        rng_before, id_before = self.trace.states(), self.read32(self.scenario + 0x214)
        for address, value in zip((0x8A0DD0, 0x8A0DD4, 0x8A0DD8, 0x8A0DDC, 0x8A0DE0, 0x8A0DE4),
                                  (11, 3, 0, 3, 5, 2) if format == 'rgb565' else (10, 3, 0, 3, 5, 3)):
            u.mem_write(address, words(value))
        # Literal repeating component inputs. No expected color is computed here.
        pattern = [0x0000, 0xFFFF, 0x1357, 0x2468, 0x7BEF, 0xA514, 0x39E7, 0xC618]
        background = [pattern[(x + y * 3) % len(pattern)] for y in range(h) for x in range(w)]
        alphas = ([0] * (w * h) if alpha == 'black' else
                  [v for y in range(h) for x in range(w)
                   for v in [(0, 1, 127, 255)[(x // 3 + y // 5) % 4]]])
        physical_z = z_words if z_words is not None else [65535] * (w * h)
        assert len(physical_z) == len(background)
        raw_rgb = struct.pack('<' + 'H' * len(background), *background)
        raw_z = struct.pack('<' + 'H' * len(physical_z), *physical_z)
        raw_alpha = struct.pack('<' + 'H' * len(alphas), *alphas)
        for plane in (self.z_pixels, rally.ALPHA):
            u.mem_write(plane - 32, self.guard)
            u.mem_write(plane + self.pixel_bytes, self.guard)
        u.mem_write(rally.PIXELS, raw_rgb)
        u.mem_write(self.z_pixels, raw_z)
        u.mem_write(rally.ALPHA, raw_alpha)
        u.mem_write(self.z_object + 0x10, words(ring_rows * w * 2))
        clip = list(clip or [0, 0, w, h])
        logical_z = []
        self.phase = 'buffer_export'
        for y in range(h):
            for x in range(w):
                ptr = self.invoke(Z_BUFFER, self.z_object, (x, y))
                logical_z.append(u16(u, ptr))
        self.phase = 'setup'
        input_row = dict(name=name, format=format, size=[w, h], pitch_words=w,
            background_words=background, z_words=logical_z, alpha_words=alphas,
            z_origin_y=self.read32(self.z_object + 0x24) - 32768, clip=clip,
            camera=camera, tactical_y=signed(self.read32(0x886FA4)),
            physical_z_words=physical_z,
            z_ring=dict(byte_offset=ring_rows * w * 2, size_bytes=self.pixel_bytes,
                        logical_samples_from_original=hex(Z_BUFFER)))
        draws, ops, comparisons = self.draw_sequence(pointers, clip)
        final = bytes(u.mem_read(rally.PIXELS, self.pixel_bytes))
        z_after, alpha_after = bytes(u.mem_read(self.z_pixels, self.pixel_bytes)), bytes(u.mem_read(rally.ALPHA, self.pixel_bytes))
        assert raw_z == z_after and raw_alpha == alpha_after
        # Separate all-depth-open pass observes every admissible lookup domain;
        # it is not the actual mixed-Z output and never supplies expected math.
        u.mem_write(rally.PIXELS, raw_rgb)
        u.mem_write(self.z_pixels, b'\xff\xff' * (w * h))
        _probe_draws, probe_ops, probe_comparisons = self.draw_sequence(pointers, clip)
        always_blocked = sum(row['candidate_z'] == 65535 for row in probe_comparisons)
        assert len(probe_comparisons) - always_blocked == len(probe_ops)
        u.mem_write(rally.PIXELS, final)
        u.mem_write(self.z_pixels, raw_z)
        assert bytes(u.mem_read(rally.PIXELS, self.pixel_bytes)) == final
        assert bytes(u.mem_read(self.z_pixels, self.pixel_bytes)) == raw_z
        assert bytes(u.mem_read(rally.ALPHA, self.pixel_bytes)) == raw_alpha
        self.check()
        assert bytes(u.mem_read(rally.PIXELS - 32, 32)) == self.guard
        assert bytes(u.mem_read(rally.PIXELS + self.pixel_bytes, 32)) == self.guard
        assert all(bytes(u.mem_read(plane + offset, 32)) == self.guard
                   for plane in (self.z_pixels, rally.ALPHA)
                   for offset in (-32, self.pixel_bytes))
        output = dict(input=input_row, draws=draws, operations=ops,
            after=dict(pixels=[[i % w, i // w, value[0]]
                for i, value in enumerate(struct.iter_unpack('<H', final))
                if final[i * 2:i * 2 + 2] != raw_rgb[i * 2:i * 2 + 2]],
                pixel_sha256=digest(final), z_before_sha256=digest(raw_z),
                z_after_sha256=digest(z_after), alpha_before_sha256=digest(raw_alpha),
                alpha_after_sha256=digest(alpha_after)),
            candidate_domains=self.domain_summary(probe_ops, probe_comparisons),
            candidate_count=len(comparisons), always_blocked_u16_max=always_blocked,
            rng_before=rng_before, rng_after=self.trace.states(),
            native_id_before=id_before, native_id_after=self.read32(self.scenario + 0x214),
            guards=dict(original_text=True, native_vtables=True, rgb_edges=True,
                        z_unchanged=True, alpha_unchanged=True, rng_unchanged=True))
        return output

    def draw_sequence(self, pointers, clip):
        u = self.u
        self.operations, self.comparisons, self.draws = [], [], []
        for p in pointers:
            before = wave_state(u, p)
            assert before['wave_type'] == 3
            polygon = before['polygon']
            # Original geometry was re-executed with the camera input above;
            # no translated polygon/span output is synthesized by Python.
            point = before['source_point']
            point_ptr, bounds_ptr = self.alloc(8), self.alloc(16)
            u.mem_write(point_ptr, words(*point))
            u.mem_write(bounds_ptr, words(*clip))
            self.current_draw = dict(polygon=polygon,
                source_point=point, source_z=before['source'][2], phase=before['phase'],
                direction=before['direction'], spans=dict(top=0, rows=[]),
                native_polygon=polygon, native_source_point=before['source_point'],
                caller_point=point, draw_offset=[0, 0], clip_paths=set())
            self.draws.append(self.current_draw)
            self.phase = 'raster'
            self.checked(DRAW, p, (point_ptr, bounds_ptr), limit=3_000_000)
            self.phase = 'setup'
            paths = self.current_draw.pop('clip_paths')
            assert len(paths) <= 1, paths
            self.current_draw['clip_path'] = next(iter(paths)) if paths else 'fast'
            self.current_draw['native_clip_branch_reached'] = bool(paths)
            assert wave_state(u, p) == before, 'draw changed retained Wave state'
        assert self.current_operation is None
        assert all('destination_word' in row and 'source_word' in row for row in self.operations)
        self.current_draw = None
        return list(self.draws), list(self.operations), list(self.comparisons)

    @staticmethod
    def domain_summary(operations, comparisons):
        keys = ('point', 'distance_index', 'distance_address', 'distance_value',
                'sine_index', 'sine_address', 'sine_value', 'source_linear_index')
        footprint = [{k: row[k] for k in keys} for row in operations]
        return dict(probe='whole DrawIt with identical geometry and supplied all65535 Z',
            candidate_count=len(comparisons), pixel_count=len(operations),
            always_blocked_u16_max=sum(row['candidate_z'] == 65535 for row in comparisons),
            ordered_footprint_sha256=digest(native._canonical(footprint)),
            distance_index_min=min((row['distance_index'] for row in operations), default=None),
            distance_index_max=max((row['distance_index'] for row in operations), default=None),
            sine_index_min=min((row['sine_index'] for row in operations), default=None),
            sine_index_max=max((row['sine_index'] for row in operations), default=None),
            adjacent_reads=sorted({(row['sine_address'], row['sine_value']) for row in operations
                                   if not row['sine_in_magnetic_domain']}),
            distance_escapes=[row for row in footprint if not 0 <= row['distance_index'] < 90000],
            source_escapes=[row for row in operations if not row['source_in_plane']])


def geometry_cases():
    source = [11392, 12416, 0]
    octants = [(1, 0), (1, 1), (0, 1), (-1, 1),
               (-1, 0), (-1, -1), (0, -1), (1, -1)]
    intermediates = [(1536, 768), (768, 1536), (-768, 1536), (-1536, 768),
                     (-1536, -768), (-768, -1536), (768, -1536), (1536, -768)]
    # One fixed six-cell radius is an input family, never an expected screen
    # direction. Preserve the shorter family below: corner/endpoint offsets
    # and native projection can put its world octants in repeated sectors.
    return [dict(name=f'raw_octant_{i}', source=source,
                 target=[source[0] + dx * 1536, source[1] + dy * 1536, 0])
            for i, (dx, dy) in enumerate(octants)] + [
        dict(name='elevated', source=[11392, 12416, 208], target=[12928, 12416, 708]),
        dict(name='zero_xy', source=source, target=source),
        *[dict(name=f'min_xy_{delta}', source=source,
               target=[source[0] + delta, source[1], source[2]]) for delta in (239, 240, 241)],
        *[dict(name=f'short_raw_octant_{i}', source=source,
               target=[source[0] + dx * 512, source[1] + dy * 512, 0])
          for i, (dx, dy) in enumerate(octants)],
        *[dict(name=f'intermediate_raw_octant_{i}', source=source,
               target=[source[0] + dx, source[1] + dy, 0])
          for i, (dx, dy) in enumerate(intermediates)]]


def require_direction_coverage(rows):
    """Admission depends on observed native results, never assigned directions."""
    for wave_type in (3, 0):
        cases = [row for row in rows if row['input']['wave_type'] == wave_type
                 and row['input']['name'].startswith(('raw_octant_', 'intermediate_raw_octant_'))]
        evidence = []
        for row in cases:
            initial = row['initial_geometry']
            returns = [dict(points=call['points'], result=call.get('return_eax'))
                       for call in row['calls'] if call['name'] == 'direction']
            evidence.append(dict(name=row['input']['name'], source=row['input']['source'],
                target=row['input']['target'], direction=None if initial is None else initial['direction'],
                projected_source=None if initial is None else initial['source_point'],
                projected_corner1=None if initial is None else initial['corner_points'][1],
                constructor_direction_calls=returns))
        observed = {row['direction'] for row in evidence}
        assert len(cases) == 16 and observed == set(range(8)), dict(
            problem='native constructor direction family did not cover all eight sectors',
            wave_type=wave_type, observed=sorted(value for value in observed if value is not None),
            rejected=any(row['direction'] is None for row in evidence), rows=evidence)
        assert all(len(row['constructor_direction_calls']) == 1
                   and row['constructor_direction_calls'][0]['result'] == row['direction']
                   for row in evidence), dict(
            problem='pre-Update snapshot disagrees with original constructor direction return',
            wave_type=wave_type, rows=evidence)
        if wave_type == 3:
            raster_cases = [row for row in evidence if row['name'].startswith('raw_octant_')]
            assert len(raster_cases) == 8 and {row['direction'] for row in raster_cases} == set(range(8)), dict(
                problem='unchanged Mag raster selection did not cover all eight sectors', rows=raster_cases)


RASTER_INPUT_FIELDS = ('name', 'format', 'size', 'pitch_words', 'clip', 'z_origin_y',
                       'background_words', 'z_words', 'alpha_words')
RASTER_OPERATION_FIELDS = ('point', 'source', 'candidate_z', 'brightness', 'distance_index',
                           'sine_index', 'source_word', 'destination_word')


def consumed_raster(row):
    """The raster fields the Rust raster/GPU tests read; the rest stay in the VM."""
    return dict(input={key: row['input'][key] for key in RASTER_INPUT_FIELDS},
                draws=row['draws'], after=row['after'],
                operations=[{key: op[key] for key in RASTER_OPERATION_FIELDS}
                            for op in row['operations']],
                rng_before=row['rng_before'], rng_after=row['rng_after'])


def generate():
    root = assets_root()
    map_path = Path(os.environ.get('VERA20K_MAGNETIC_WAVE_MAP', str(RETAINED_MAP)))
    physical_inputs_before = identities(root, map_path)
    donor = UnitInputs(root, map_path)
    machine = WaveMachine(donor)
    births, birth_pointers = [], []
    for heading in (0, 0x4000, 0x8000, 0xC000):
        p, row = machine.birth(heading)
        births.append(row)
        birth_pointers.append(p)
    machine.admission_controls()  # Gate self-check; Rust reads no admission rows.
    nonunit_pose = machine.set_pose()
    cell = machine.cells[50, 48]
    machine.u.mem_write(machine.source + 0x2B4, words(cell))
    target_out = machine.alloc(12)
    machine.invoke(machine.read32(machine.read32(cell) + 0x58), cell, (target_out,))
    cell_target = ints(machine.u, target_out, 3)
    source_flh = machine.flh()
    # FireAt's constructor order: the target coordinate, then the launch source.
    _cell_wave, cell_initial = machine.construct(3, cell_target, source_flh,
                                               target_pointer=cell, name='cell_nonunit_tracking')
    nonunit_row = dict(input=dict(name='cell_nonunit_tracking', target_cell=[50, 48],
        target_kind='Cell', external_owner_target='supplied Cell pointer',
        source=nonunit_pose['source'], source_flh=source_flh, target_center=cell_target,
        constructor_source=cell_target, constructor_target=source_flh),
        initial_geometry=cell_initial, after=wave_state(machine.u, _cell_wave), calls=machine.records)
    machine.set_pose()
    tracking = []
    for name, xyz in (('ground', [12928, 12416, 0]),
                      ('supplied_current_height250', [12928, 12416, 250]),
                      ('supplied_current_height500', [12928, 12416, 500])):
        pose = machine.set_pose(target=xyz)
        context = machine.continuation_inputs()
        machine.current_case = dict(name=name, wave_type=3, **pose)
        machine.checked(UPDATE, birth_pointers[0])
        tracking.append(dict(input=dict(name=name, **pose, **context),
            after=wave_state(machine.u, birth_pointers[0]), calls=machine.records))
    machine.set_pose()
    geometries, geometric_pointers, sonic_inputs = [], [], []
    for wave_type in (3, 0):
        for case in geometry_cases():
            p, initial = machine.construct(wave_type, case['source'], case['target'], name=case['name'])
            geometries.append(dict(input=dict(case, wave_type=wave_type),
                initial_geometry=initial, after=wave_state(machine.u, p),
                calls=machine.records, **machine.last_constructor_ids))
            if wave_type == 3 and initial is not None:
                geometric_pointers.append(p)
            if wave_type == 0 and case['name'].startswith('raw_octant'):
                sonic_inputs.append((case, initial['direction']))
    require_direction_coverage(geometries)
    phase_rows = [machine.phase_control(wave_type, phase)
                  for wave_type in (0, 3) for phase in (0, 18, 19, 20, 21, 100, -1)]
    type0_lifecycles = []
    for name, right_edge in (('type0_left_edge_double', False), ('type0_right_edge_float', True)):
        case, _direction = next((case, direction) for case, direction in sonic_inputs
                                if (direction >= 4) == right_edge)
        type0_lifecycles.append(machine.update_only_history(case['source'], case['target'], name))
    direct_geometry_rows = []
    for wave_type in (0, 3):
        degenerate, row = machine.direct_geometry(wave_type, geometry_cases()[0]['source'],
                                                 geometry_cases()[0]['source'])
        direct_geometry_rows.append(row)
    # One compact native history retains state; no phase/front output is forced.
    p = birth_pointers[-1]
    continuation_pose = machine.set_pose()
    continuation_context = machine.continuation_inputs()
    machine.current_case = dict(name='native_phase_reset_and_external_target_clear',
                                wave_type=3, birth_row_index=len(births) - 1)
    history = [dict(visit=0, after=wave_state(machine.u, p))]
    for visit in range(1, 83):
        machine.current_case.update(stage='AI', visit=visit)
        after = machine.advance(p)
        if visit in (1, 19, 20, 79, 80, 81, 82):
            history.append(dict(visit=visit, after=after))
    machine.u.mem_write(machine.source + 0x2B4, words(0))
    fade = []
    for visit in range(1, 24):
        machine.current_case.update(stage='external clear fade', visit=visit)
        after = machine.advance(p)
        if visit in (1, 2, 10, 19, 20, 21) or not after['alive']:
            fade.append(dict(visit=visit, after=after))
        if not after['alive']:
            break
    assert not fade[-1]['after']['alive'], 'bounded native fade failed to retire'
    machine.set_pose()
    # These callbacks retire the separate lifecycle receiver, not any raster
    # Wave. Fail cleanup prerequisites before expensive pixel/ring controls.
    # Guard all future raster storage and supplied actor/surface inputs so the
    # order change cannot silently become a different drawing fixture.
    raster_wave_before = {q: bytes(machine.u.mem_read(q, 0x240))
                          for q in (*geometric_pointers, degenerate)}
    assert p not in raster_wave_before, 'lifecycle retirement aliases a raster Wave'
    actor_before = {actor: bytes(machine.u.mem_read(actor, 0x1000))
                    for actor in (machine.source, machine.target)}
    surface_before = {plane: bytes(machine.u.mem_read(plane, machine.pixel_bytes))
                      for plane in (rally.PIXELS, machine.z_pixels, rally.ALPHA)}
    tactical_before = bytes(machine.u.mem_read(rally.TACTICAL + 0xB0, 8))
    id_before = machine.read32(machine.scenario + 0x214)
    # Callback and destructor self-checks; Rust reads neither result.
    machine.pointer_controls(p)
    machine.retire(p)
    for q, before in raster_wave_before.items():
        unchanged(machine.u, q, before, 'retirement: independent raster Wave')
    for actor, before in actor_before.items():
        after = bytearray(machine.u.mem_read(actor, 0x1000))
        if actor == machine.source:
            # Exact Wave-link removal is the measured callback result.
            after[0x324:0x328] = before[0x324:0x328]
        assert bytes(after) == before, dict(problem='retirement changed supplied raster actor',
                                            actor=hex(actor))
    for plane, before in surface_before.items():
        unchanged(machine.u, plane, before, 'retirement: independent raster plane')
    unchanged(machine.u, rally.TACTICAL + 0xB0, tactical_before, 'retirement: raster camera')
    assert id_before == machine.read32(machine.scenario + 0x214)
    machine.check(stage='retirement before independent rasters')
    # Preserve constructor-selected directions, but measure the settled native
    # polygon from each independent raw input before whole DrawIt.
    raster_rows = []
    for index, q in enumerate(geometric_pointers[:8]):
        # Rebuild raw geometry without changing its constructor direction. This
        # is a declared full-geometry component call, not a tracking result.
        case = geometry_cases()[index]
        a, b = machine.alloc(12), machine.alloc(12)
        machine.u.mem_write(a, words(*case['source']))
        machine.u.mem_write(b, words(*case['target']))
        machine.checked(MAGNETIC, q, (a, b))
        raster_rows.append(machine.raster([q], name=f'packed_direction_{index}'))
    q = geometric_pointers[0]
    raster_rows.append(machine.raster([q], name='rgb555', format='rgb555'))
    raster_rows.append(machine.raster([geometric_pointers[8]], name='elevated_long_component'))
    raster_rows.append(machine.raster([q], name='alpha_black_ignored', alpha='black'))
    raster_rows.append(machine.raster([q], name='ring_fallback_clip', ring_rows=70,
                                     clip=[8, 8, 64, 64]))
    native_z = [0] * (80 * 80)
    for row in raster_rows[0]['operations']:
        x, y = row['point']
        native_z[y * 80 + x] = row['candidate_z']
    raster_rows.append(machine.raster([q], name='strict_equal_native_candidates', z_words=native_z))
    raster_rows.append(machine.raster([geometric_pointers[0], geometric_pointers[2]], name='overlap_order_0_2'))
    raster_rows.append(machine.raster([geometric_pointers[2], geometric_pointers[0]], name='overlap_order_2_0'))
    # The type3 direct-geometry control above already owns admitted storage.
    raster_rows.append(machine.raster([degenerate], name='zero_xy_geometry'))
    lookup_rows = machine.lookup_controls(q)
    machine.check()
    raster_rows = [consumed_raster(row) for row in raster_rows]
    assert physical_inputs_before == identities(root, map_path), 'physical input drift'
    return dict(schema_version=1, native_sha256=native.image_sha256(), inputs=donor.inputs,
        tables=machine.tables,
        numeric_constants=dict(step_f32_le_hex=bytes(machine.u.mem_read(0x7F6BB0, 4)).hex(),
            snap_f64_le_hex=bytes(machine.u.mem_read(0x7F6DE8, 8)).hex(),
            half_f64_le_hex=bytes(machine.u.mem_read(0x7E1738, 8)).hex()),
        birth_rows=births, geometry_rows=geometries, direct_geometry_rows=direct_geometry_rows,
        nonunit_rows=[nonunit_row],
        tracking_rows=tracking, lookup_rows=lookup_rows, phase_rows=phase_rows,
        lifecycle_rows=[dict(name='native_phase_reset_and_external_target_clear',
            input=dict(wave_type=3, birth_row_index=len(births) - 1,
                **continuation_pose, **continuation_context),
            history=history, supplied_release='clear owner+2B4 after visit82; no native Move/lift execution', fade=fade),
            *type0_lifecycles],
        raster_rows=raster_rows,
        rng_before=machine.rng_initial, rng_after=machine.trace.states())


def metadata():
    return native.provenance(
        scope='Original selected TELE/MTNK readers and Wave3 construction/tracking/lifetime, shared type0 geometry, packed framebuffer distortion',
        assumptions=[
            'Physical RULESMD, optional LANGRULE, MPBattleMD and retained ordinary-tele-magnetic.map are separate cached lexical reader passes; fixed ARTMD is not layered. Rules665650/UnitType7470D0 constructors and whole747620/772080/75D3A0/46BEE0 execute on explicitly filtered selected fields. Native current-defaults, factories/pointers and actual fixed-ART FLH are measured. This is not archive/file loading, complete discovery/Rules Process or scenario initialization.',
            'Supplied rookie TELE/MTNK Unit-vtable shells have native-read type pointers and actual original Drive4AF540 instances. Original6F3041..6F30D6 initializes their empty scores/candidates/history vectors so whole Techno7077C0 can execute its unconditional history FindIndex4E0550. Whole GetFLH6F3AD0, Drive4AFF60, native slope startup, Facing and Tactical matrix/projection execute. Whole actor construction/House/Unlimbo, normal legal firing, Bullet launch, lift, movement and height updates are external prerequisites. Physical source/target roster positions are ordinary44,48 and50,48; component numeric rows declare their own raw coordinates.',
            'The measured FireAt fragment enters6FF5F5 with already launched-success continuation state, original native-read IsMagBeam and supplied shot-local FLH/target from original getters, and stops before6FF656. Whole Wave constructor75E950 and its geometry/Unlimbo/Update execute; this is not whole FireAt. Direct constructor controls include all eight raw octants for types3/0 and elevated/degenerate inputs. Initial geometry is observed at75EBC5, before original first Update, with exact native CC->corner1 direction. Later Update retains the actual Unit reversal.',
            'WaveMachine uses the existing LaserPixels/Rally VM and memory surfaces, copies the original-reader arena, and supplies a sparse native-constructed flat Cell window, Map descriptor and empty growable effect/Display/Logic registries. Tactical type-header6D1D54..6D1D6F and matrix6D1DC5..6D1E1E run on supplied storage; whole native6DA560 handles actual expiry callbacks over empty screen records. Original725350..725386 initializes only the missing pristine B0F670 listener header before any Wave constructor, preserving all established registry headers and stopping before CRT atexit. Whole Object5F3B80 destruction/FindIndex and deletion remain native. Original startup constants, Cell constructors and complete Wave graph run unchanged; these component admissions are not a physical map/scenario loader or whole-object identity prefix. The inherited unrelated LaserDraw constructor runs only as surface setup and its managers are forbidden during measurement.',
            'Original zero-coordinate initializer75E8F0 and whole75F020 produce all90000 distance entries,496 magnetic entries,500 adjacent Sonic entries and13 brightness entries. Exact adjacent bytes and actual writer extents are retained. No host sqrt/sin, modulo, clamping, expected phase, coordinate, table, color or raster algorithm supplies native outputs.',
            'Original Update762AF0 and AI760F50 retain state across82 type3 component visits and a bounded fade. Shared type0 Update-only histories measure active/inactive cells without Sonic AI/damage. Phase0/18/19/20/21/100/-1 controls run whole Update, observing original reciprocal ftol and raw double front/back/loop state plus the inactive right-edge float spill. Clearing owner+2B4 is an explicit external target-clear input, not native Move or lift release. PointerExpired75F610 executes separately; full Techno damage/movement, whole Logic scheduling, save/load and scenario/network timing remain outside this corpus.',
            'Whole DrawIt75F9F0->7602E0->63F080/63F2E0->760190 executes with original BSurface/Z services. Packed RGB565/RGB555, literal patterned destination/A/Z, ring descriptor, caller point/crop and input phase/geometry are explicit. Native spans, traversal, strict unsigned depth admission, sampled source words and all stores are observed passively. Physical A/Z remain unchanged. GPU/live retained Display composition needs independent production validation.',
            'A separate original all65535-Z DrawIt probe observes lookup domains for every admissible geometric candidate, including those blocked by actual mixed/equal Z. CandidateZ65535 is always blocked for any u16 Z and needs no lookup. Raw linear distance and adjacent sine addresses are recorded, never clamped; domain escapes remain explicit. Probe pixels do not replace actual raster goldens.',
        ],
        substitutions=[
            'Existing Reader cached-INI CRC indexes, bump allocation/delete, CRT TLS and guid_transport Windows ASCII/GUID services; selected type visual setup5F8110/5F8CE0 and exit registration are setup-only recorded boundaries. No type reader, GetFLH, matrix, Wave/math/raster return is supplied.',
            'Existing LaserPixels/Rally BSurface memory storage/locks and Reader allocation/delete/TLS transport stand in for platform resources. No native instruction or vtable is patched after declared surface setup. Three full Main/Scenario/Mapgen RNG objects and native ID cursor are observed; no RNG draw or gameplay result is substituted.',
        ],
        entry_points=dict(unit_type_ctor=0x7470D0, unit_type_reader=0x747620,
            weapon_reader=0x772080, warhead_reader=0x75D3A0, projectile_reader=0x46BEE0,
            drive_ctor=0x4AF540, drive_matrix=0x4AFF60, get_flh=0x6F3AD0,
            fire_fragment=TAIL_BEGIN, fire_stop=TAIL_END, constructor=CTOR,
            magnetic_geometry=MAGNETIC, nonmagnetic_geometry=NONMAGNETIC,
            direction=0x75F230, update=UPDATE, update_cells=0x7610F0,
            right_edge_lerp=0x75F540, ai=AI, draw=DRAW,
            magnetic_draw=0x7602E0, spans=0x63F080, span_edge=0x63F2E0,
            pixel=PIXEL, offsets=OFFSETS, tables=TABLE_INIT, zero_coord=ZERO_COORD,
            z_get_buffer=Z_BUFFER, pointer_expired=0x75F610,
            tactical_type_header=0x6D1D54, tactical_type_header_stop=0x6D1D6F,
            tactical_pointer_expired=0x6DA560, actor_target_vectors=0x6F3041,
            actor_target_vectors_stop=0x6F30D6, empty_pointer_find=0x4E0550,
            object_type_listener_setup=0x725350, object_type_listener_stop=0x725386,
            object_destructor=0x5F3B80))


if __name__ == '__main__':
    sources = {name: Path(module.__file__) for name, module in sorted(sys.modules.items())
               if name.startswith('tools.') and getattr(module, '__file__', None)
               and str(module.__file__).endswith('.py')}
    sources.update(producer=Path(__file__), retained_map=RETAINED_MAP)
    native.finish_vectors(generate, HERE / 'magnetic_wave.json', provenance=metadata,
                          source_paths=sources)
