"""Original DESO mode0 or opt-in CLEG mode1/kind1 beam component controls.

Cached physical INI/ART inputs and surface/actor prestates are explicit component
boundaries. All emitted numeric values/pixels come from original instructions.
See rad_beam.md / chrono_beam.md; generation requires the source/input freeze.
"""
import argparse
import hashlib
import os
from pathlib import Path
import struct
import sys

from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX,
    UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
)

from tools import native_oracle as native
from tools.procedural_drawing_oracle import electric_bolt as electric, rally
from tools.projectile_oracle.bridge_render_inputs import BulletReader, assets_root, lexical
from tools.rules_oracle.bridge_anim_inputs import Reader
from tools.spatial_oracle import building_prism as prism
from tools.spatial_oracle.building_body_rules import INI, RULES, SP

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
RETAINED_MAP = HERE / 'rad_beam' / 'ordinary-deso-radbeam.map'
MAP_SHA256 = '565b61d34519b501a3d7b249fd7566e0621832719402b04a369df405e38f87b5'
CHRONO_MAP = HERE / 'rad_beam' / 'ordinary-cleg-chrono.map'
CHRONO_MAP_SHA256 = 'cd2a316907719f79cd43f67bcad45076e107677b8327e66fcdc6e99ffeccfda5'
CHRONO_WEAPONS = ('NeutronRifle', 'NeutronRifleE')
AV_GATE, AV_PRESENT, AV_ABSENT = 0x6691E0, 0x66920D, 0x66B8F1
CHRONO_COLOR_BEGIN, CHRONO_COLOR_END = 0x66B7A7, 0x66B7D2
WRAPPER, ALLOCATE, CTOR, VECTOR_INIT, VECTOR = (
    0x6FD620, 0x659110, 0x6593F0, 0x659090, 0xB04A60)
MANAGER, DRAW, SEGMENT, DIVIDE = 0x6591B0, 0x659650, 0x659AC0, 0x65A1B0
TYPE_KEYS = {'Primary', 'ElitePrimary', 'Image', 'Strength', 'Armor', 'TurretOffset'}
WEAPON_KEYS = {
    'Damage', 'ROF', 'Range', 'MinimumRange', 'Projectile', 'Warhead',
    'IsLaser', 'IsElectricBolt', 'IsRadBeam', 'IsRadEruption',
}
WEAPONS = ('RadBeamWeapon', 'RadBeamWeaponE')
WARHEAD_KEYS = {'Radiation', 'Temporal', 'Verses'}
ART_KEYS = {'Image', 'Sequence', 'PrimaryFireFLH', 'Crawls', 'FireUp'}
READ_KEYS = TYPE_KEYS | WEAPON_KEYS | WARHEAD_KEYS | ART_KEYS | {'RadColor'}
BIRTH_CALLS = {
    WRAPPER: ('spawn_rad_beam', 2, 8),
    ALLOCATE: ('allocate', 0, 0),
    CTOR: ('constructor', 1, 4),
    0x523250: ('infantry_flh', 5, 20),
    0x6F3AD0: ('techno_flh', 5, 20),
    0x41BE00: ('render_coord', 1, 4),
    0x41BDD0: ('target_coord', 1, 4),
    0x5F65A0: ('object_coord', 1, 4),
    0x6D1F10: ('project_for_source_adjust', 2, 8),
    0x6594A0: ('set_source', 1, 4),
    0x6594C0: ('set_target', 1, 4),
    0x6594E0: ('set_duration', 1, 4),
    0x6594F0: ('set_amplitude', 2, 8),
    0x659470: ('set_color', 1, 4),
    0x659490: ('set_z_adjust', 1, 4),
}
words, u32, ints, signed = electric.words, electric.u32, electric.ints, electric.signed


def bits(u, address):
    return bytes(u.mem_read(address, 8)).hex()


def beam_state(u, p, *, geometry=False):
    """Read only initialized, known fields; never serialize freed/padding data."""
    out = dict(kind=signed(u32(u, p + 0x10)),
        source_link=u32(u, p), target_link=u32(u, p + 4),
        geometry_ready=bool(u.mem_read(p + 8, 1)[0]),
        draw_enabled=bool(u.mem_read(p + 9, 1)[0]),
        z_adjust=signed(u32(u, p + 0xC)), spacing_f64_le_hex=bits(u, p + 0x18),
        rgb=list(u.mem_read(p + 0x20, 3)),
        source=ints(u, p + 0x24, 3), target=ints(u, p + 0x30, 3),
        duration=signed(u32(u, p + 0x3C)), amplitude_f64_le_hex=bits(u, p + 0x40),
        travel_scale_f64_le_hex=bits(u, p + 0x48),
        travel_rate=signed(u32(u, p + 0x50)),
        progress_step=signed(u32(u, p + 0x54)),
        retain_endpoint=bool(u.mem_read(p + 0x58, 1)[0]),
        progress=signed(u32(u, p + 0x7C)))
    if geometry and out['geometry_ready']:
        out.update(delta=ints(u, p + 0x5C, 3), length_f64_le_hex=bits(u, p + 0x68),
            segment_count=signed(u32(u, p + 0x8C)),
            current_amplitude_f64_le_hex=bits(u, p + 0x80))
    return out


def selected(raw, *, chrono=False):
    filters = ({'CLEG': TYPE_KEYS | {'Locomotor', 'Teleporter'},
                'ChronoBeam': WARHEAD_KEYS, 'AudioVisual': {'ChronoBeamColor'},
                **{name: WEAPON_KEYS for name in CHRONO_WEAPONS}}
               if chrono else
               {'DESO': TYPE_KEYS, 'RadBeamWarhead': WARHEAD_KEYS,
                'Radiation': {'RadColor'}, **{name: WEAPON_KEYS for name in WEAPONS}})
    sections, lines = lexical(raw, set(filters))
    # Section existence survives filtering. Native section admission still runs.
    sections = {name: {k: v for k, v in values.items() if k in filters[name]}
                for name, values in sections.items()}
    lines = [row for row in lines if row['key'] in filters[row['section']]]
    return sections, lines


def physical_files(root, map_path, *, chrono=False):
    files = []
    for name in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini'):
        p = root / name
        if not p.exists():
            assert name == 'LANGRULE.INI', ('missing required physical layer', name)
            files.append((name, None))
        else:
            files.append((name, p.read_bytes()))
    raw = map_path.read_bytes()
    retained, digest = (CHRONO_MAP, CHRONO_MAP_SHA256) if chrono else (RETAINED_MAP, MAP_SHA256)
    assert hashlib.sha256(raw).hexdigest() == digest, 'selected map differs from retained input'
    assert raw == retained.read_bytes(), 'configured map is not byte-identical to retained map'
    files.append((retained.relative_to(REPO).as_posix(), raw))
    return files


class PhysicalBirth(BulletReader):
    """One existing Reader VM; original readers, FLH and wrapper share pointers."""
    def __init__(self, root, map_path, *, chrono=False):
        self.chrono = chrono
        self.type_name = 'CLEG' if chrono else 'DESO'
        self.weapon_names = CHRONO_WEAPONS if chrono else WEAPONS
        self.warhead_name = 'ChronoBeam' if chrono else 'RadBeamWarhead'
        self.color_offset = 0x1866 if chrono else 0x1830
        self.read_keys = READ_KEYS | ({'Locomotor', 'Teleporter', 'ChronoBeamColor'} if chrono else set())
        self.birth_specs = dict(BIRTH_CALLS)
        if chrono:
            self.birth_specs[0x55A730] = ('locomotor_matrix', 3, 12)
        self.phase, self.platform_counts, self.layer_name = 'setup', {}, 'constructor'
        self.call_trace, self.birth_calls = None, []
        self.selected_reads = []
        self.layers = physical_files(root, map_path, chrono=chrono)
        self.art_raw = (root / 'ARTMD.INI').read_bytes()
        sequence = 'ClegSequence' if chrono else 'DesoSequence'
        art, lines = lexical(self.art_raw, {self.type_name, sequence})
        self.art = {name: (dict(values) if name != self.type_name else
                          {k: v for k, v in values.items() if k in ART_KEYS})
                    for name, values in art.items()}
        self.art_lines = [row for row in lines if row['section'] != self.type_name
                          or row['key'] in ART_KEYS]
        super().__init__(self.art, root, arena_size=0x800000)
        u = self.u
        self.text = bytes(u.mem_read(prism.TEXT_BEGIN, prism.TEXT_SIZE))
        self.vtables = {p: bytes(u.mem_read(p, size)) for p, size in (
            (0x7EB058, 0x59C), (0x7EB610, 0x190))}
        if chrono:
            self.vtables[0x7F4FDC] = bytes(u.mem_read(0x7F4FDC, 0x150))
        self.rules = self.alloc(0x2000)
        u.mem_write(0x8871E0, words(self.rules))
        # Declared empty capacity; native factories still construct/find/append.
        for registry in (0x887568, 0x8874C0, 0xA8EB00, 0xA83CE0, 0xA8ED40,
                         0xA8E348, 0xA83DE8, 0xA83C98, 0x8B4120, 0xB0EC60):
            u.mem_write(registry, words(0x7EB6D4, self.alloc(4096), 1024, 1, 0, 10))
        self.invoke(0x665650, self.rules)
        self.type = self.alloc(0x1900)
        self.invoke(0x5236A0, self.type, (self.cstring(self.type_name),))
        self.weapons, self.warheads = {}, {}
        self.reader_inputs = dict(constructor=self.fields(), layers=[])
        self.latest_type = {}
        for name, raw in self.layers:
            self.layer_name = name
            if raw is None:
                self.reader_inputs['layers'].append(dict(file=name, absent=True))
                continue
            sections, source_lines = selected(raw, chrono=chrono)
            self.latest_type.update(sections.get(self.type_name, {}))
            self.make_ini(self.art)
            self.rules_cache(sections)
            mark = len(self.selected_reads)
            if chrono:
                color_read = self.read_chrono_color(self.rules)
                admitted = dict(audio_visual=color_read['admitted'])
            else:
                admitted = dict(radiation=self.invoke(0x66CF70, self.rules, (RULES,)) & 255)
            type_admitted = self.invoke(0x5240A0, self.type, (RULES,)) & 255
            for offset in (0x898, 0xA94):
                p = self.read32(self.type + offset)
                if p:
                    label = self.string(p + 0x24)
                    assert label in self.weapon_names, ('selected physical weapon changed', label)
                    assert self.weapons.get(label, p) == p
                    self.weapons[label] = p
            for p in self.weapons.values():
                self.invoke(0x772080, p, (RULES,))
                wh = self.read32(p + 0xAC)
                if wh:
                    label = self.string(wh + 0x24)
                    assert label == self.warhead_name, label
                    assert self.warheads.get(label, wh) == wh
                    self.warheads[label] = wh
            for p in self.warheads.values():
                self.invoke(0x75D3A0, p, (RULES,))
            admitted['type'] = type_admitted
            layer = dict(file=name, bytes=len(raw),
                sha256=hashlib.sha256(raw).hexdigest(), sections=sections,
                source_lines=source_lines, admitted=admitted,
                reads=self.selected_reads[mark:], after=self.fields())
            if chrono:
                layer['color_fragment'] = color_read
            self.reader_inputs['layers'].append(layer)
        assert set(self.weapons) == set(self.weapon_names)
        final = self.fields()
        assert all(v['is_rad_beam'] and not v['is_rad_eruption']
                   and not v['is_laser'] and not v['is_electric_bolt']
                   for v in final['weapons'].values()), 'physical direct-beam arm changed'
        assert final['warheads'][self.warhead_name]['temporal'] == chrono, 'physical beam mode gate changed'
        self.reader_inputs.update(final=final, art=dict(file='ARTMD.INI',
            bytes=len(self.art_raw), sha256=hashlib.sha256(self.art_raw).hexdigest(),
            sections=self.art, source_lines=self.art_lines),
            platform_boundaries=dict(self.platform_counts))
        # Supplied Infantry components, not ctor/Unlimbo/world admission.
        self.source, self.target = self.alloc(0x1000), self.alloc(0x1000)
        for p in (self.source, self.target):
            u.mem_write(p, words(0x7EB058))
            u.mem_write(p + 0x14, words(7))
            u.mem_write(p + 0x6C0, words(self.type))
            u.mem_write(p + 0x150, struct.pack('<f', 0.0))
        self.locomotor = self.alloc(0x100)
        self.invoke(0x718000 if chrono else 0x75AA90, self.locomotor)
        u.mem_write(self.locomotor + 0xC, words(self.source))
        u.mem_write(self.source + 0x674, words(self.locomotor + 4))
        if chrono:
            assert self.read32(self.locomotor) == 0x7F50CC
            assert self.read32(self.locomotor + 4) == 0x7F5000
            assert self.read32(0x7F5000 + 0x24) == 0x55A730
            assert final['cleg']['locomotor_clsid_hex'] == '4727584a3998d111b70900a024ddafd1'
            assert final['cleg']['teleporter'], 'selected physical CLEG Teleporter changed'
            self.reader_inputs['actor_components'] = dict(
                source=dict(type='CLEG', vtable=hex(0x7EB058), rank='rookie', burst=0,
                    passengers=False, locomotor_ctor=hex(0x718000),
                    locomotor_vtable=hex(self.read32(self.locomotor)),
                    ilocomotion_vtable=hex(self.read32(self.locomotor + 4)),
                    draw_matrix=hex(self.read32(0x7F5000 + 0x24)),
                    linked_to_source=True,
                    constructor_selection='declared Teleport identity from physical native-read GUID; native COM factory not executed'),
                target=dict(type='CLEG', vtable=hex(0x7EB058), rank='rookie',
                    locomotor=None, passengers=False),
                actor_constructor_executed=False, native_com_factory_executed=False)
        self.temp = self.alloc(64)
        self.tactical = self.alloc(0x1000)
        u.mem_write(0x887324, words(self.tactical))
        # Same original matrix initializer as LaserScene; no host matrix.
        u.reg_write(UC_X86_REG_ESI, self.tactical)
        u.reg_write(UC_X86_REG_EBX, 0)
        native.run_checked(u, 0x6D1DC5, 0x6D1E1E, count=30)
        u.mem_write(0xB0CD48, struct.pack('<Q', 0x3FC25E5374344960))
        u.mem_write(0xB0CE30, words(160, 120))
        u.mem_write(0xA8ED84, words(200))
        self.invoke(VECTOR_INIT, 0)
        self.rng_trace = electric.RngTrace(u, self.read32(0xA8B230), 31)
        self.rng_initial = self.rng_trace.states()

    def read_chrono_color(self, rules):
        """Original AV section gate, then the exact color block in a supplied frame.

        The gate stops before any AV scalar read. The separate block receives
        only its live ESI/EDI/EDX/frame inputs; whole ReadColorRGB and CRT run.
        Neither stop boundary pretends to return from the whole AV reader.
        """
        assert self.chrono
        u, previous_phase = self.u, self.phase
        before = bytes(u.mem_read(rules, 0x2000))
        self.phase = 'color'
        self.color_calls = []
        mark = len(self.selected_reads)
        u.mem_write(SP, words(native.RET_MAGIC, RULES))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, rules)
        native.run_checked(u, AV_GATE, (AV_PRESENT, AV_ABSENT),
            required_addresses=(0x526810,), count=10000,
            context=dict(component='chrono_color_section_gate', file=self.layer_name))
        stop = u.reg_read(UC_X86_REG_EIP)
        admitted = int(stop == AV_PRESENT)
        assert u.reg_read(UC_X86_REG_ESP) == SP - 0xCC
        assert u.reg_read(UC_X86_REG_ESI) == rules
        assert u.reg_read(UC_X86_REG_EDI) == RULES
        assert (u.reg_read(UC_X86_REG_EAX) & 255) == admitted
        assert before == bytes(u.mem_read(rules, 0x2000)), 'section gate wrote Rules'
        receipt = dict(admitted=admitted, current_default=list(before[0x1866:0x1869]),
            gate=dict(entry=hex(AV_GATE), stop_before=hex(stop),
                supplied_entry='ECX=Rules; [entry ESP+4]=cached layer INI',
                stack_delta=-0xCC, whole_audio_visual=False),
            fragment=None)
        if admitted:
            # Original66B79D supplies EDX=ESP+10, before the preceding color's
            # final store. Start66B7A7 with that same live pointer, not its RGB.
            guard = bytes.fromhex('d6a93c75')
            u.mem_write(SP + 0xC, guard + b'\xA5' * 3 + guard)
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_ESI, rules)
            u.reg_write(UC_X86_REG_EDI, RULES)
            u.reg_write(UC_X86_REG_EDX, SP + 0x10)
            native.run_checked(u, CHRONO_COLOR_BEGIN, CHRONO_COLOR_END,
                required_addresses=(0x474B50, 0x528A10), count=200000,
                context=dict(component='chrono_color_fragment', file=self.layer_name))
            assert u.reg_read(UC_X86_REG_ESP) == SP
            assert bytes(u.mem_read(SP + 0xC, 4)) == guard
            assert bytes(u.mem_read(SP + 0x13, 4)) == guard
            assert bytes(u.mem_read(SP + 0x10, 3)) == bytes(u.mem_read(rules + 0x1866, 3))
            receipt['fragment'] = dict(entry=hex(CHRONO_COLOR_BEGIN),
                stop_before=hex(CHRONO_COLOR_END),
                supplied_frame='ESP=component frame; ESI=Rules; EDI=cached layer INI; EDX=ESP+10',
                whole_audio_visual=False, whole_color_reader=True, local_guards_unchanged=True)
        after = bytes(u.mem_read(rules, 0x2000))
        assert before[:0x1866] == after[:0x1866] and before[0x1869:] == after[0x1869:]
        receipt.update(rgb=list(after[0x1866:0x1869]), reads=self.selected_reads[mark:],
                       calls=self.color_calls, other_rules_bytes_unchanged=True)
        self.phase = previous_phase
        self.unchanged()
        return receipt

    def hook(self, u, pc, size, user):
        sp = u.reg_read(UC_X86_REG_ESP)
        if self.phase in ('setup', 'color'):
            if self.phase == 'color':
                assert pc not in (0x5F8110, 0x5F8CE0, 0x5B40B0, 0x527AF9,
                                  0x527B0C, 0x7C978A, 0x65C780, 0x65C7E0,
                                  0x7258D0, 0x70D4A0), ('setup/gameplay seam in color', hex(pc))
                if pc in (0x526810, 0x474B50, 0x528A10, 0x7CA530, 0x7D140B):
                    self.color_calls.append(hex(pc))
            if self.phase == 'setup' and Reader.guid_transport(self, u, pc, sp, self.events):
                self.platform_counts['GUID'] = self.platform_counts.get('GUID', 0) + 1
                return
            if pc in (0x5F8110, 0x5F8CE0, 0x7C978A):
                key = hex(pc)
                self.platform_counts[key] = self.platform_counts.get(key, 0) + 1
                self.ret(0)
                return
            if pc == 0x474B50:
                self.selected_reads.append(dict(file=self.layer_name, reader=hex(pc),
                    section=self.string(self.read32(sp + 8)),
                    key=self.string(self.read32(sp + 12)),
                    default_rgb=list(u.mem_read(self.read32(sp + 16), 3))))
            elif pc in (0x5276D0, 0x5295F0, 0x528A10):
                key = self.string(self.read32(sp + 8))
                if key in self.read_keys:
                    row = dict(file=self.layer_name, reader=hex(pc),
                               section=self.string(self.read32(sp + 4)), key=key)
                    if pc != 0x528A10:
                        row['default_u32'] = self.read32(sp + 12)
                    self.selected_reads.append(row)
        elif self.phase == 'birth':
            assert pc not in (0x5F8110, 0x5F8CE0, 0x5B40B0, 0x7D140B,
                              0x527AF9, 0x527B0C, 0x7C978A), ('setup seam in birth', hex(pc))
            assert pc not in (0x65C780, 0x65C7E0, 0x7258D0, 0x70D4A0), hex(pc)
            for index in self.call_trace.returned(pc, sp):
                row = self.birth_calls[index]
                if row['name'] == 'constructor':
                    row['constructor_state'] = beam_state(u, int(row['ecx'], 16))
                if row['name'] in ('infantry_flh', 'techno_flh', 'render_coord',
                                   'target_coord', 'object_coord'):
                    row['coordinate'] = ints(u, row['args'][0], 3)
                if row['name'] == 'locomotor_matrix':
                    assert row['args'][0] == self.locomotor + 4
                    row['matrix_f32_le_hex'] = bytes(u.mem_read(row['args'][1], 48)).hex()
            if pc in self.birth_specs:
                index = self.call_trace.entered(pc, sp, self.birth_specs[pc])
                self.birth_calls[index]['receiver'] = (
                    'source' if u.reg_read(UC_X86_REG_ECX) == self.source else
                    'target' if u.reg_read(UC_X86_REG_ECX) == self.target else None)
        super().hook(u, pc, size, user)

    def reference(self, p):
        return self.string(p + 0x24) if p else None

    def fields(self):
        u = self.u
        actor_type = dict(primary=self.reference(self.read32(self.type + 0x898)),
                elite_primary=self.reference(self.read32(self.type + 0xA94)),
                primary_flh=ints(u, self.type + 0x89C, 3),
                turret_offset=signed(self.read32(self.type + 0x720)),
                strength=signed(self.read32(self.type + 0xA0)))
        if self.chrono:
            # Whole TechnoType reader71241D..2437 copies ReadGUID to+34C;
            # literal Teleporter843E60 -> ReadBool713FF1 -> store+CD4 at713FF6.
            actor_type['locomotor_clsid_hex'] = bytes(u.mem_read(self.type + 0x34C, 16)).hex()
            actor_type['teleporter'] = bool(u.mem_read(self.type + 0xCD4, 1)[0])
        return {
            'chrono_beam_color' if self.chrono else 'rad_color':
                list(u.mem_read(self.rules + self.color_offset, 3)),
            'cleg' if self.chrono else 'deso': actor_type,
            'weapons': {name: dict(is_rad_beam=bool(u.mem_read(p + 0x154, 1)[0]),
                is_rad_eruption=bool(u.mem_read(p + 0x155, 1)[0]),
                is_laser=bool(u.mem_read(p + 0x149, 1)[0]),
                is_electric_bolt=bool(u.mem_read(p + 0x151, 1)[0]),
                damage=signed(self.read32(p + 0xA4)), range=signed(self.read32(p + 0xB4)),
                projectile=self.reference(self.read32(p + 0xA0)),
                warhead=self.reference(self.read32(p + 0xAC)))
                for name, p in self.weapons.items()},
            'warheads': {name: dict(temporal=bool(u.mem_read(p + 0x15A, 1)[0]),
                radiation=bool(u.mem_read(p + 0x177, 1)[0]))
                for name, p in self.warheads.items()}}

    def unchanged(self):
        assert self.text == bytes(self.u.mem_read(prism.TEXT_BEGIN, prism.TEXT_SIZE))
        assert all(raw == bytes(self.u.mem_read(p, len(raw))) for p, raw in self.vtables.items())
        if hasattr(self, 'rng_trace'):
            assert not self.rng_trace.requests and not self.rng_trace.pending
            assert self.rng_trace.states() == self.rng_initial

    def color_controls(self):
        self.phase = 'setup'
        p = self.alloc(0x2000)
        self.invoke(0x665650, p)
        if self.chrono:
            rows = [dict(name='constructor', sections=None, current_default=None,
                         rgb=list(self.u.mem_read(p + self.color_offset, 3)), reads=[])]
            for name, sections in (
                ('absent_section', {}),
                ('present_missing_key', {'AudioVisual': {'C30KeepSection': '1'}}),
                ('retained_nondefault', {'AudioVisual': {'ChronoBeamColor': '11,22,33'}}),
                ('missing_retains_current', {'AudioVisual': {'C30KeepSection': '1'}}),
            ):
                self.layer_name = 'color_control/' + name
                self.rules_cache(sections)
                rows.append(dict(name=name, sections=sections, **self.read_chrono_color(p)))
            self.unchanged()
            return rows
        rows = [dict(name='constructor', rgb=list(self.u.mem_read(p + 0x1830, 3)))]
        for name, sections in (
            ('absent_section', {}),
            ('present_missing_key', {'Radiation': {'C29KeepSection': '1'}}),
            ('retained_nondefault', {'Radiation': {'RadColor': '11,22,33'}}),
            ('missing_retains_current', {'Radiation': {'C29KeepSection': '1'}}),
            ('wrong_case_retains_current', {'Radiation': {'radcolor': '200,100,50'}}),
        ):
            self.layer_name = 'color_control/' + name
            self.rules_cache(sections)
            mark = len(self.selected_reads)
            admitted = self.invoke(0x66CF70, p, (RULES,)) & 255
            rows.append(dict(name=name, sections=sections, admitted=admitted,
                rgb=list(self.u.mem_read(p + 0x1830, 3)), reads=self.selected_reads[mark:]))
        self.unchanged()
        return rows

    def birth(self, case):
        self.phase = 'setup'
        art = {name: dict(values) for name, values in self.art.items()}
        if 'art_flh_control' in case:
            art[self.type_name]['PrimaryFireFLH'] = case['art_flh_control']
        self.make_ini(art)
        self.rules_cache({self.type_name: dict(self.latest_type)})
        assert self.invoke(0x5240A0, self.type, (RULES,)) & 255
        u = self.u
        for p, xyz in ((self.source, case['location']), (self.target, case['target'])):
            u.mem_write(p + 0x9C, words(*xyz))
            u.mem_write(p + 0x510, words(0))
        self.invoke(0x4C91E0, self.source + 0x388, (0,))
        u.mem_write(self.temp, words(case['heading']))
        self.invoke(0x4C9300, self.source + 0x388, (self.temp,))
        u.mem_write(self.source + 0x3B8, words(case.get('burst', 0)))
        before = [bytes(u.mem_read(p, 0x1000)) for p in (self.source, self.target)]
        before_locomotor = bytes(u.mem_read(self.locomotor, 0x100)) if self.chrono else None
        count_before = self.read32(VECTOR + 0x10)
        id_ptr = self.read32(0xA8B230) + 0x214
        id_before = self.read32(id_ptr)
        self.birth_calls = []
        self.call_trace = native.NativeCallTrace(u, self.read32, self.birth_calls)
        self.phase = 'birth'
        self.invoke(WRAPPER, self.source, (self.target, int(self.chrono)))
        self.call_trace.returned(native.RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
        self.phase = 'setup'
        assert not self.call_trace.pending
        assert u.reg_read(UC_X86_REG_ESP) == SP + 12
        assert before == [bytes(u.mem_read(q, 0x1000)) for q in (self.source, self.target)]
        assert id_before == self.read32(id_ptr)
        assert self.read32(VECTOR + 0x10) == count_before + 1
        # Registry membership, rather than the raw EAX receipt, owns birth.
        p = self.read32(self.read32(VECTOR + 4) + count_before * 4)
        state = beam_state(u, p)
        assert state['kind'] == 1 and not state['source_link'] and not state['target_link']
        assert {'infantry_flh', 'techno_flh', 'set_source', 'set_target'} <= {
            row['name'] for row in self.birth_calls}
        if self.chrono:
            assert before_locomotor == bytes(u.mem_read(self.locomotor, 0x100))
            assert 'locomotor_matrix' in {row['name'] for row in self.birth_calls}
            assert state['rgb'] == self.fields()['chrono_beam_color']
        self.unchanged()
        return dict(input=case, physical_flh=ints(u, self.type + 0x89C, 3),
            beam=state, registered_before=count_before, registered_after=count_before + 1,
            native_id_unchanged=True, actors_unchanged=True, rng_unchanged=True,
            calls=[{k: v for k, v in row.items() if k not in (
                'ecx', 'entry_sp', 'return_sp', 'entry_fpcw', 'return_fpcw')}
                for row in self.birth_calls])


class RadPixels(prism.LaserPixels):
    """Original RadBeam draw on existing surface and allocator/RNG owners."""
    allocate = electric.ElectricPixels.allocate
    return_boundary = electric.ElectricPixels.return_boundary

    def __init__(self, case):
        self.rad_active = False
        # LaserPixels constructs an unrelated LaserDraw as surface setup.
        # Its draw/update managers never execute. No EBolt/palette VM is needed.
        super().__init__(dict(case, seed=31))
        self.heap_cursor = prism.HEAP + 0x1000
        self.deleted = []
        self.scenario = prism.PRISM + 0x50000
        self.u.mem_write(0xA8B230, words(self.scenario))
        self.u.mem_write(self.scenario + 0x214, words(1000))
        self.trace = electric.RngTrace(self.u, self.scenario, 31)
        self.rad_pointers, self.rad_order, self.rad_geometry, self.rad_returns = [], [], [], []
        self.manager_scalars, self.length_inputs = {}, []
        self.current_rad = None
        self.call(VECTOR_INIT)
        self.rad_clips = []
        self.clip_trace = native.NativeCallTrace(self.u, lambda p: u32(self.u, p), self.rad_clips)
        self.rad_active = True
        self.rng_initial = self.trace.states()
        self.id_initial = u32(self.u, self.scenario + 0x214)

    def observe(self, u, pc, size, data):
        active = self.rad_active
        if active:
            sp = u.reg_read(UC_X86_REG_ESP)
            for index in self.clip_trace.returned(pc, sp):
                row = self.rad_clips[index]
                row['points_after'] = [ints(u, p, 2) for p in row['point_pointers']]
                row['accepted_al'] = row['return_eax'] & 255
            if pc == 0x7BC2B0:
                # Existing action_lines observer establishes this fastcall ABI.
                index = self.clip_trace.entered(pc, sp, ('clip_line', 1, 4))
                row = self.rad_clips[index]
                ptrs = [u.reg_read(UC_X86_REG_ECX), u.reg_read(UC_X86_REG_EDX)]
                row.update(point_pointers=ptrs, points_before=[ints(u, p, 2) for p in ptrs],
                           clip=ints(u, row['args'][0], 4))
            if pc == prism.OPERATOR_NEW:
                self.return_boundary(value=self.allocate(u32(u, sp + 4)))
            elif pc == prism.OPERATOR_DELETE:
                self.deleted.append(u32(u, sp + 4))
                self.return_boundary()
            assert pc not in (electric.CTOR, electric.INIT, electric.MANAGER, 0x62DC50,
                              0x65C780, 0x65C7E0, 0x7258D0, 0x70D4A0), hex(pc)
            if pc == DRAW:
                self.current_rad = u.reg_read(UC_X86_REG_ECX)
                self.rad_order.append(self.rad_pointers.index(self.current_rad))
            elif pc == 0x659270:
                self.manager_scalars['reciprocal_f64_le_hex'] = bits(
                    u, u.reg_read(UC_X86_REG_ESP) + 0x20)
            elif pc == 0x6592A0:
                self.manager_scalars.update(ftol_low_u32=u.reg_read(UC_X86_REG_EAX),
                                           ftol_high_u32=u.reg_read(UC_X86_REG_EDX))
            elif pc == 0x6596C5:
                self.length_inputs.append(signed(u32(u, u.reg_read(UC_X86_REG_ESP) + 0x18)))
            elif pc == 0x659845:
                p = self.current_rad
                self.rad_geometry.append(dict(
                    beam_index=self.rad_pointers.index(p),
                    index=signed(u32(u, p + 0x88)),
                    from_coord=ints(u, p + 0x9C, 3), to_coord=ints(u, p + 0xA8, 3),
                    rgb=list(u.mem_read(p + 0xC0, 3)),
                    rgb_factor_f64_le_hex=bits(u, p + 0xB8)))
            elif pc == 0x6592CB:
                self.rad_returns.append(dict(beam_index=self.rad_pointers.index(self.current_rad),
                    expired=bool(u.reg_read(UC_X86_REG_EAX) & 255)))
        super().observe(u, pc, size, data)

    def construct(self, birth, *, progress=0, duration=None, draw_enabled=True):
        self.call(ALLOCATE, 1)
        p = u32(self.u, VECTOR + 4)
        p = u32(self.u, p + (u32(self.u, VECTOR + 0x10) - 1) * 4)
        self.rad_pointers.append(p)
        post_allocate = beam_state(self.u, p)
        coord = self.allocate(32)
        self.u.mem_write(coord, words(*birth['source'], *birth['target']))
        self.call(0x6594A0, p, coord)
        self.call(0x6594C0, p, coord + 12)
        self.call(0x6594E0, p, birth['duration'] if duration is None else duration)
        self.call(0x6594F0, p, *struct.unpack('<2I', bytes.fromhex(birth['amplitude_f64_le_hex'])))
        color = coord + 24
        self.u.mem_write(color, bytes(birth['rgb']))
        self.call(0x659470, p, color)
        self.call(0x659490, p, birth['z_adjust'])
        # Prepared stage/draw flag controls, not natural native timing outputs.
        self.u.mem_write(p + 0x7C, words(progress))
        self.u.mem_write(p + 9, bytes([draw_enabled]))
        self.check()
        return p, post_allocate

    def registered(self):
        u = self.u
        return [u32(u, u32(u, VECTOR + 4) + i * 4) for i in range(u32(u, VECTOR + 0x10))]

    def check(self):
        u = self.u
        assert not self.trace.requests and not self.trace.pending
        assert not self.clip_trace.pending
        assert self.trace.states() == self.rng_initial
        assert self.id_initial == u32(u, self.scenario + 0x214)
        assert bytes(u.mem_read(self.z_pixels, self.pixel_bytes)) == self.before_z
        assert bytes(u.mem_read(rally.ALPHA, self.pixel_bytes)) == self.before_alpha
        assert bytes(u.mem_read(rally.PIXELS - 32, 32)) == self.guard
        assert bytes(u.mem_read(rally.PIXELS + self.pixel_bytes, 32)) == self.guard
        assert hashlib.sha256(u.mem_read(prism.TEXT_BEGIN, prism.TEXT_SIZE)).hexdigest() == self.text_sha256
        assert all(raw == bytes(u.mem_read(p, len(raw))) for p, raw in self.surface_tables.items())

    def center_camera(self, source, *, partial=False, offscreen=False):
        # Native projection chooses a declared crop; it supplies no expected pixel.
        temp = self.allocate(32)
        self.u.mem_write(rally.TACTICAL + 0xB0, words(0, 0))
        self.u.mem_write(temp, words(*source))
        self.call(0x6D2140, rally.TACTICAL, temp, temp + 16)
        projected = ints(self.u, temp + 16, 2)
        camera = [projected[0] - 20, projected[1] - 60]
        if offscreen:
            camera = [n + 3000 for n in camera]
        self.u.mem_write(rally.TACTICAL + 0xB0, words(*camera))
        clip = [45, 0, 100, self.surface.size[1]] if partial else self.surface.clip
        self.u.mem_write(0x886FA0, words(*clip))
        return dict(native_projected_source=projected, camera=camera, clip=clip)

    def visit(self, *, manager=True, pointer=None, pixels=False, geometry=False):
        before = self.registered()
        self.calls, self.rad_order, self.rad_geometry, self.rad_returns = [], [], [], []
        self.rad_clips = []
        self.clip_trace = native.NativeCallTrace(self.u, lambda p: u32(self.u, p), self.rad_clips)
        self.manager_scalars = {}
        if manager:
            self.call(MANAGER)
        else:
            self.current_rad = pointer
            self.call(DRAW, pointer)
            self.rad_returns.append(dict(beam_index=self.rad_pointers.index(pointer),
                expired=bool(self.u.reg_read(UC_X86_REG_EAX) & 255)))
        self.check()
        after = self.registered()
        result = dict(
            registered_before=[self.rad_pointers.index(p) for p in before],
            registered_after=[self.rad_pointers.index(p) for p in after],
            draw_order=list(self.rad_order), returns=list(self.rad_returns),
            states=[dict(beam_index=self.rad_pointers.index(p),
                         state=beam_state(self.u, p, geometry=True)) for p in after],
            freed=[self.rad_pointers.index(p) for p in self.deleted if p in self.rad_pointers],
            segment_count=len(self.rad_geometry), raster_call_count=len(self.calls),
            manager_scalars=self.manager_scalars, rng_unchanged=True,
            native_id_unchanged=True, alpha_unchanged=True, z_unchanged=True,
            text_unchanged=True, vtables_unchanged=True, guards_unchanged=True)
        if geometry:
            # Retain the raw pushed dword and its actual packed-word ABI view.
            segments = [dict(row, packed_word=row['color'] & 0xFFFF) for row in self.calls]
            clips = [{k: v for k, v in row.items() if k not in (
                'ecx', 'entry_sp', 'return_sp', 'entry_fpcw', 'return_fpcw',
                'point_pointers', 'args')} for row in self.rad_clips]
            result.update(geometry=list(self.rad_geometry), segments=segments, clips=clips)
        if pixels:
            raw = bytes(self.u.mem_read(rally.PIXELS, self.pixel_bytes))
            alpha = bytes(self.u.mem_read(rally.ALPHA, self.pixel_bytes))
            z = bytes(self.u.mem_read(self.z_pixels, self.pixel_bytes))
            width, _ = self.surface.size
            result.update(pixel_sha256=hashlib.sha256(raw).hexdigest(),
                initial_pixel_sha256=hashlib.sha256(self.before).hexdigest(),
                alpha_before_sha256=hashlib.sha256(self.before_alpha).hexdigest(),
                alpha_after_sha256=hashlib.sha256(alpha).hexdigest(),
                z_before_sha256=hashlib.sha256(self.before_z).hexdigest(),
                z_after_sha256=hashlib.sha256(z).hexdigest(),
                pixels=[[i % width, i // width, x[0]]
                    for i, x in enumerate(struct.iter_unpack('<H', raw))
                    if raw[i*2:i*2+2] != self.before[i*2:i*2+2]])
        return result


def birth_cases():
    base = dict(location=[9856, 12416, 0], target=[10496, 12416, 0], burst=0)
    rows = [dict(base, name=f'physical_deso_heading_{h:04x}', heading=h)
            for h in (0, 0x4000, 0x8000, 0xC000)]
    rows += [
        dict(base, name='positive_depth_control', heading=0,
             art_flh_control='100,-25,-135'),
        dict(base, name='same_raw_y_control', heading=0, art_flh_control='0,0,135'),
    ]
    return rows


def fps_rows(birth):
    rows = []
    for name, current, requested in [
        ('startup_zero', 0, 30), ('very_low', 1, 30), ('low', 14, 30),
        ('clamp_neighborhood', 15, 30), ('middle', 20, 30), ('below_nominal', 29, 30),
        ('nominal', 30, 30), ('above_nominal', 31, 30), ('high', 60, 30),
        ('unsigned_high_bit', 0x80000000, 30), ('unsigned_max', 0xFFFFFFFF, 30),
        ('zero_over_zero', 0, 0), ('positive_over_zero', 30, 0),
        ('negative_requested', 30, -30),
    ]:
        m = RadPixels(dict(name=name, camera=[3000, 3000]))
        p, _ = m.construct(birth)
        m.u.mem_write(0xABCD44, words(current))
        m.u.mem_write(0xA8B558, words(requested))
        observed = m.visit()
        assert observed['draw_order'] == [0]
        assert {'reciprocal_f64_le_hex', 'ftol_low_u32', 'ftol_high_u32'} <= set(
            observed['manager_scalars']), 'FPS scalar observation was not reached'
        rows.append(dict(input=dict(name=name, current_u32=current, requested_i32=requested),
            spacing_f64_le_hex=bits(m.u, p + 0x18),
            manager_scalars=observed['manager_scalars'],
            progress=signed(u32(m.u, p + 0x7C)), rng_unchanged=True))
    return rows


def length_rows(birth):
    rows = []
    for name, delta in [
        ('zero', [0, 0, 0]), ('short', [1, 0, 0]), ('nonintegral', [25, 0, 0]),
        ('last_positive_square', [46340, 0, 0]), ('negative_square_wrap', [46341, 0, 0]),
        ('zero_square_wrap', [65536, 0, 0]), ('three_axis_wrap', [50000, 50000, 50000]),
        ('signed_coordinate_extreme', [-2147483648, 0, 0]),
    ]:
        m = RadPixels(dict(name=name))
        p, _ = m.construct(dict(birth, source=[0, 0, 0], target=delta))
        m.u.mem_write(prism.SP, words(native.RET_MAGIC))
        m.u.reg_write(UC_X86_REG_ESP, prism.SP)
        m.u.reg_write(UC_X86_REG_ECX, p)
        native.run_checked(m.u, DRAW, 0x65978C, count=10000)
        assert m.u.reg_read(UC_X86_REG_EIP) == 0x65978C
        assert len(m.length_inputs) == 1
        m.check()
        rows.append(dict(input=dict(name=name, delta=delta),
            entry='0x00659650', stop_before='0x0065978c', whole_draw=False,
            squared_low32_signed=m.length_inputs[0],
            length_f64_le_hex=bits(m.u, p + 0x68),
            actual_delta=ints(m.u, p + 0x5C, 3), rng_unchanged=True))
    return rows


def division_rows():
    m = RadPixels(dict(name='coord_division'))
    p = m.allocate(12)
    rows = []
    for value, divisor in [
        ([11, -11, 256], 0), ([11, -11, 256], 1), ([11, -11, 256], 3),
        ([11, -11, 256], -3), ([-2147483648, 2147483647, -1], 2),
        ([-2147483648, 2147483647, -1], -2147483648),
    ]:
        m.u.mem_write(p, words(*value))
        m.call(DIVIDE, p, divisor)
        m.check()
        rows.append(dict(input=dict(value=value, divisor=divisor), result=ints(m.u, p, 3)))
    return rows


def geometry_rows(birth, *, cases=None):
    rows = []
    base = dict(name='physical_birth_progress7')
    if cases is None:
        cases = [
            base, dict(name='mixed_alpha_depth', alpha='mixed', z=[0, 32760, 65535]),
            dict(name='black_alpha', alpha='black'),
            dict(name='partial_clip_changing_z', partial=True),
            dict(name='short_zero_divisor', coords=[[3038, 3038, 0], [3039, 3038, 0]]),
            dict(name='nonintegral_divisor_overshoot', coords=[[3038, 3038, 0], [3063, 3038, 0]]),
            dict(name='zero_length', coords=[[3038, 3038, 0], [3038, 3038, 0]]),
            dict(name='disabled_draw_advances', draw_enabled=False),
        ]
    for case in cases:
        m = RadPixels(case)
        supplied = dict(birth)
        if 'coords' in case:
            supplied.update(source=case['coords'][0], target=case['coords'][1])
        p, ctor = m.construct(supplied, progress=7, draw_enabled=case.get('draw_enabled', True))
        viewport = m.center_camera(supplied['source'], partial=case.get('partial', False))
        observed = m.visit(manager=False, pointer=p, pixels=True, geometry=True)
        if case.get('alpha') == 'black':
            assert not observed['pixels'], 'black A unexpectedly stored pixels'
        if case.get('partial'):
            assert any(row['accepted_al'] and row['points_before'] != row['points_after']
                       and DRAW <= int(row['caller'], 16) < SEGMENT
                       for row in observed['clips']), 'native preclip control missed its changing-XY arm'
            assert any(row['z_start'] != row['z_end'] for row in observed['segments']), (
                'native preclip control did not retain changing endpoint Z')
        rows.append(dict(input=case, supplied_birth=supplied, post_allocate=ctor,
            surface=dict(size=list(m.surface.size), background=case.get('background', rally.BACKGROUND),
                alpha=case.get('alpha', 'clear'), z=case.get('z', 65535),
                alpha_pattern_owner='rally.Rally.alpha_at',
                z_pattern_owner='building_prism.LaserPixels.__init__'),
            viewport=viewport, before_progress=7, after=observed))
    return rows


def lifetime_rows(birth):
    rows = []
    for name, specifications in [
        ('ordinary_same_frame_lifetime', [(15, 0)]),
        ('reverse_order_compaction', [(2, 0), (3, 0)]),
        ('clipped_lifetime', [(15, 0)]),
    ]:
        m = RadPixels(dict(name=name))
        births = []
        for duration, progress in specifications:
            p, ctor = m.construct(birth, duration=duration, progress=progress)
            births.append(dict(post_allocate=ctor, birth=beam_state(m.u, p)))
        m.center_camera(birth['source'], offscreen=name == 'clipped_lifetime')
        m.u.mem_write(0xABCD44, words(30))
        m.u.mem_write(0xA8B558, words(30))
        rng_before = m.trace.states()
        visits = [m.visit() for _ in range(16 if len(specifications) == 1 else 4)]
        assert not m.registered(), ('bounded lifetime did not retire', name)
        assert len(m.deleted) == len(specifications)
        rows.append(dict(name=name, binary_frame=200, births=births, visits=visits,
            rng_before=rng_before, rng_after=m.trace.states()))
    return rows


def input_identities(root, map_path, *, chrono=False):
    files = physical_files(root, map_path, chrono=chrono)
    art = (root / 'ARTMD.INI').read_bytes()
    return [(name, None if raw is None else hashlib.sha256(raw).hexdigest())
            for name, raw in [*files, ('ARTMD.INI', art)]]


def generate():
    root = assets_root()
    map_path = Path(os.environ.get('VERA20K_RAD_BEAM_MAP', str(RETAINED_MAP)))
    before_inputs = input_identities(root, map_path)
    machine = PhysicalBirth(root, map_path)
    colors = machine.color_controls()
    births = [machine.birth(case) for case in birth_cases()]
    by_name = {row['input']['name']: row for row in births}
    negative = by_name['physical_deso_heading_0000']
    positive = by_name['positive_depth_control']
    equal = by_name['same_raw_y_control']
    assert negative['beam']['z_adjust'] < 0 and positive['beam']['z_adjust'] > 0
    assert all(row['beam']['source'][1] != row['input']['location'][1]
               for row in (negative, positive)), 'raw-Y unequal branch was not reached'
    assert equal['beam']['source'][1] == equal['input']['location'][1]
    assert equal['beam']['z_adjust'] == 0
    assert not any(row['name'] == 'project_for_source_adjust' for row in equal['calls'])
    birth = births[0]['beam']
    result = dict(schema_version=1,
        inputs=machine.reader_inputs, color_controls=colors,
        birth_rng_before=machine.rng_initial, birth_rng_after=machine.rng_trace.states(),
        birth_rows=births, fps_rows=fps_rows(birth), length_rows=length_rows(birth),
        division_rows=division_rows(), geometry_rows=geometry_rows(birth),
        lifetime_rows=lifetime_rows(birth), original_text_and_vtables_unchanged=True)
    machine.unchanged()
    assert before_inputs == input_identities(root, map_path), 'physical input drift'
    return result


def generate_chrono():
    root = assets_root()
    map_path = Path(os.environ.get('VERA20K_CHRONO_BEAM_MAP', str(CHRONO_MAP)))
    before_inputs = input_identities(root, map_path, chrono=True)
    machine = PhysicalBirth(root, map_path, chrono=True)
    colors = machine.color_controls()
    cases = [dict(case, name=case['name'].replace('deso', 'cleg'))
             for case in birth_cases()[:4]]
    births = [machine.birth(case) for case in cases]
    result = dict(schema_version=1,
        inputs=machine.reader_inputs, color_controls=colors,
        birth_rng_before=machine.rng_initial, birth_rng_after=machine.rng_trace.states(),
        birth_rows=births, geometry_rows=geometry_rows(births[0]['beam'], cases=[
            dict(name='physical_birth_progress7'),
            dict(name='mixed_alpha_depth', alpha='mixed', z=[0, 32760, 65535]),
            dict(name='black_alpha', alpha='black'),
            dict(name='partial_clip_changing_z', partial=True),
        ]), original_text_and_vtables_unchanged=True)
    machine.unchanged()
    assert before_inputs == input_identities(root, map_path, chrono=True), 'physical input drift'
    return result


def metadata():
    return native.provenance(
        scope='Original DESO mode0/kind1 RadBeam wrapper, selected physical readers, FPS spacing, straight geometry, packed pixels and draw-count retirement',
        assumptions=[
            'Selected physical RULESMD, optional LANGRULE, MPBattleMD and retained ordinary-deso-radbeam.map execute in separate reader passes; fixed ARTMD is not layered. Native Rules665650/InfantryType5236A0 constructors and whole66CF70/5240A0/772080/75D3A0 readers execute with explicitly filtered selected keys/sections. This is cached lexical input, not native file/archive loading, complete Rules Process/discovery/order or startup.',
            'PhysicalBirth reuses BulletReader/Reader native INI/CRT/TLS/allocation/GUID transport. Selected DESO Primary/ElitePrimary/native pointers, fixed ART FLH, weapon beam flags, warhead Temporal/Radiation and RadColor outputs/defaults are measured. Projectile constructors may run for references; projectile reading/firing/damage are outside this component. Voice/sequence/visual assets and unrelated scalar keys are filtered or setup-only boundaries.',
            'Actors are supplied native Infantry-vtable/physical-type pointers, rookie rank, coordinates, zero burst, no passenger/open-topped links, native Walk75AA90 and initialized Facing state. They are not native Infantry construction, Unlimbo, placement, House/AI/fog or input admission. Source Tactical matrix executes the same original6D1DC5..6D1E1E initializer as LaserScene; default projection scalar/camera inputs remain explicit.',
            'Whole SpawnRadBeam6FD620 executes mode0 and original GetFLH/coordinate/projection/setters/allocator/constructor. Synthetic fixed-ART controls run original type readers before the wrapper; no FLH, coordinate, depth or beam output is substituted. Original registry growth/insertion runs; mode0 actor links remain zero.',
            'RadPixels composes existing LaserPixels/Rally surface runtime and borrows unchanged ElectricPixels allocator/return methods plus RngTrace. Its inherited unrelated LaserDraw constructor executes before measurement; its managers never run. No bolt or unrelated palette VM is constructed. RadBeam births are original Allocate/constructor/setters with values copied from independent native wrapper output or explicitly declared component input coordinates/progress/duration.',
            'Full6591B0/659650/659AC0/65A1B0 run as declared. Length overflow rows stop at65978C before the subsequent draw branch and are not whole-draw results. FPS/current/requested, stage7, RGB565 destination, raw-u16 A/Z, crop, clipping and FPCW0E7F are component inputs; startup/current0 and requested0 retain original masked arithmetic/ftol. Network timing-event producer and scenario save/load remain open.',
            'Native manager reverse traversal, compaction, return expiry and deletion execute. Complete lifetime controls keep binary frame200 unchanged; source/target links are copied null, not world attachments. No Abstract identity, CdTimer, RNG or detach is added. Existing RngTrace owns seeded Main/Scenario/Mapgen prestates; before/after full1012-byte objects are retained per birth/lifetime family and all rows reject draws or mutation.',
            'Native projection/clipping and packed4BFD30 execute against existing BSurface lock/stride/unlock and immutable A/Z; all sparse pixels and hashes are original outputs. The partly clipped changing-Z row retains native preclipped XY and original endpoint depth arguments, without normalizing them. Whole tactical composition, GPU upload, live fog/terrain production, audio, eruption/kind2, Chrono/magnetic attachment modes and complete Scene startup are excluded.',
        ],
        substitutions=[
            'Existing Reader bump allocator/no-op delete, native cached-INI/CRT/TLS and Reader.guid_transport Windows services. Type visual asset entries5F8110/5F8CE0 and atexit are recorded setup-only boundaries and forbidden inside measured wrapper. No measured FLH/getter/math/manager/draw return or numeric value is supplied.',
            'Existing ElectricPixels surface allocations/delete/atexit transport and Rally memory surfaces replace DirectDraw storage only; original packed DSurface body remains installed. Inherited Spark/bolt boundaries are guarded unreachable during RadBeam measurements. No instruction bytes, source vtables, sine table, expected color, raster, RNG or gameplay result are patched.',
        ],
        entry_points=dict(wrapper=WRAPPER, allocate=ALLOCATE, constructor=CTOR,
            vector_initializer=VECTOR_INIT, manager=MANAGER, draw=DRAW, segment=SEGMENT,
            coordinate_divide=DIVIDE, rules_ctor=0x665650, radiation_reader=0x66CF70,
            infantry_type_ctor=0x5236A0, infantry_type_reader=0x5240A0,
            weapon_reader=0x772080, warhead_reader=0x75D3A0, infantry_flh=0x523250,
            techno_flh=0x6F3AD0, sine=0x4CACB0, sqrt=0x4CAC40, ftol=0x7C5F00,
            projection=0x6D2140, source_projection=0x6D1F10, packed_line=0x4BFD30))


def chrono_metadata():
    return native.provenance(
        scope='Original CLEG mode1/kind1 copied beam, selected physical readers, native Teleport FLH and multichannel packed pixels',
        assumptions=[
            'Opt-in --chrono uses only the retained ordinary-cleg-chrono.map as the final layer after physical RULESMD, optional LANGRULE and MPBattleMD. Fixed ARTMD is not layered. Original Rules665650 and InfantryType5236A0 constructors plus whole5240A0/772080/75D3A0 readers execute on explicitly filtered CLEG/NeutronRifle/NeutronRifleE/ChronoBeam sections; native INI parses measured fields. This is cached lexical preparation, not physical file loading, full type discovery, complete Rules Process or startup.',
            'ChronoBeamColor is AudioVisual, offset1866. Each layer runs original section-gate prefix6691E0 and stops before66920D or66B8F1. An admitted layer then runs original66B7A7..66B7D2 with ESI=Rules, EDI=cached INI, component ESP and EDX=ESP+10 from the preceding66B79D LEA. Whole474B50/528A10 and native CRT parse/default handling execute. Neither prefix is represented as a whole AudioVisual return. Constructor/current RGB is never supplied; all other Rules bytes and output guards are checked. Color precedes selected type reads in this component; full Process read ordering is outside it.',
            'Selected CLEG Primary/ElitePrimary, native locomotor GUID, fixed ART ClegSequence/FLH, effect flags, Temporal/Radiation and ChronoBeamColor defaults/outputs are retained. Native factories construct/find/insert referenced types in supplied empty registries with spare capacity. Projectile reference constructors may run; projectile reading/firing/damage and visual/sound setup are outside this component.',
            'Source and target are explicit supplied rookie CLEG Infantry-vtable/native-read-type actors, not native Infantry construction, Unlimbo, House, legal firing/first-shot admission or world placement. Target has no locomotor. Source constructs actual Teleport718000, links its original ILocomotion7F5000 to the actor and verifies +24=55A730. Whole mode1 SpawnRadBeam6FD620 reaches that actual native slot through original GetFLH. Native Facing and Tactical matrix initializer execute; pose, camera/scalar and binary frame200 are supplied. Settled native Teleport is not a teleport movement simulation or COM factory execution.',
            'Four physical cardinal mode1 births run whole allocation/constructor/FLH/getters/projection/setters/registry insertion. Both actor links remain null, copied endpoints and ChronoBeamColor come from native readers/wrapper, and source/target/locomotor bytes, original text/vtables, ID cursor and full Main/Scenario/Mapgen RNG objects remain unchanged. No measured getter, FLH, depth, color or wrapper return is replaced. No RNG draw or detach is admitted.',
            'The unchanged RadPixels/LaserPixels/Rally allocator, memory surfaces, native projection, sine/math, DrawStraight/ComputeStraight and packed4BFD30 owners run for four progress7 controls: clear destination, mixed A/Z, black A and partly clipped changing Z. All endpoints, color attenuation, ordered primitive arguments, whole RGB565 hashes, sparse pixel witnesses and immutable A/Z identities are original outputs. An unrelated LaserDraw constructor runs only during inherited surface setup; its managers and bolt/spark hooks must remain unreachable.',
            'The shared mode1 kind1 renderer/lifetime arithmetic uses the consumed C29 rad_beam.json FPS/length/division/lifetime controls rather than duplicating those families. This variant does not execute whole FireAt, native Temporal detonation/warp/rearm, timing events, scenario load/save, actual retained-map MTNK target construction, whole Tactical/GPU composition, live terrain/fog, sound, eruption/kind2 or magnetic mode2 attachment. Those production/lifecycle gates need separate evidence.',
        ],
        substitutions=metadata()['substitutions'],
        entry_points=dict(wrapper=WRAPPER, allocate=ALLOCATE, constructor=CTOR,
            vector_initializer=VECTOR_INIT, draw=DRAW, segment=SEGMENT,
            rules_ctor=0x665650, audio_visual_gate=AV_GATE,
            audio_visual_present_stop=AV_PRESENT, audio_visual_absent_stop=AV_ABSENT,
            chrono_color_fragment=CHRONO_COLOR_BEGIN, chrono_color_stop=CHRONO_COLOR_END,
            color_reader=0x474B50, read_string=0x528A10, section_find=0x526810,
            infantry_type_ctor=0x5236A0, infantry_type_reader=0x5240A0,
            weapon_reader=0x772080, warhead_reader=0x75D3A0,
            teleport_ctor=0x718000, locomotor_matrix=0x55A730,
            infantry_flh=0x523250, techno_flh=0x6F3AD0,
            projection=0x6D2140, source_projection=0x6D1F10, packed_line=0x4BFD30))


if __name__ == '__main__':
    selector = argparse.ArgumentParser(add_help=False)
    selector.add_argument('--chrono', action='store_true')
    selected_mode, remaining = selector.parse_known_args()
    sources = {name: Path(module.__file__) for name, module in sorted(sys.modules.items())
               if name.startswith('tools.') and getattr(module, '__file__', None)
               and str(module.__file__).endswith('.py')}
    sources.update(producer=Path(__file__), retained_map=CHRONO_MAP if selected_mode.chrono else RETAINED_MAP)
    if selected_mode.chrono:
        native.finish_vectors(generate_chrono, HERE / 'chrono_beam.json',
            provenance=chrono_metadata, argv=remaining, source_paths=sources)
    else:
        native.finish_vectors(generate, HERE / 'rad_beam.json', provenance=metadata,
                              argv=remaining, source_paths=sources)
