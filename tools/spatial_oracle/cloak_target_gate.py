"""Original SUB CanAutoCloak, including its actual retained-target range query.

The existing composed reader owns the VM, native INI caches and sparse world.
Placement, live actor membership and timers are declared component inputs.
No cloak/range/weapon-selection result is supplied by Python.
"""

import hashlib
import os
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_EIP,
    UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FPCW,
)

from tools.native_oracle import (
    NativeCallTrace, RET_MAGIC, changed_byte_spans, finish_vectors,
    image_sha256, provenance, run_checked,
)
from tools.projectile_oracle.bridge_render_inputs import assets_root, lexical
from tools.rules_oracle.bridge_anim_inputs import Reader
from tools.spatial_oracle import bridge_target_composed as composed
from tools.spatial_oracle.building_body_rules import RULES, SP, dwords


HERE = Path(__file__).resolve().parent
CAN_AUTO = 0x6FBDC0
GENERAL_KEYS = {'CloakingStages', 'CloakDelay'}
READ_KEYS = GENERAL_KEYS | {
    'Cloakable', 'CloakStop', 'CloakingSpeed', 'Primary', 'Secondary',
    'ElitePrimary', 'EliteSecondary', 'Strength', 'Armor', 'Naval',
    'Underwater', 'MovementZone', 'LandTargeting', 'AirRangeBonus',
    'ROF', 'Range', 'MinimumRange', 'DecloakToFire', 'Projectile',
    'Warhead', 'AA', 'AG', 'Image',
}
CALLS = {
    CAN_AUTO: ('can_auto_cloak', 0, 0),
    0x4DBDA0: ('is_cloakable', 0, 0),
    0x70C5A0: ('has_stealth_ability', 0, 0),
    0x74FF90: ('is_veteran', 0, 0),
    0x750010: ('is_elite', 0, 0),
    0x4870B0: ('cell_cloaked_by_house', 1, 4),
    0x6F7780: ('can_fire_at_target', 1, 4),
    0x6F77B0: ('can_fire_at', 2, 8),
    0x746CD0: ('unit_select_weapon', 1, 4),
    0x6F3330: ('techno_select_weapon', 1, 4),
    0x70E140: ('get_weapon', 1, 4),
    0x7177C0: ('normal_weapon_slot', 1, 4),
    0x7177E0: ('elite_weapon_slot', 1, 4),
    0x6F7220: ('in_range', 3, 12),
    0x565730: ('coordinate_cell', 1, 4),
    0x5657A0: ('map_cell', 1, 4),
    0x5F6960: ('object_cell', 0, 0),
    0x5F5F40: ('height', 0, 0),
}
GATES = {
    0x6FBEAC: 'state', 0x6FBEBE: 'weapon_rearm',
    0x6FBEEA: 'target', 0x6FBF0C: 'progress',
    0x6FBF2B: 'recloak', 0x6FBF57: 'lift', 0x6FBF7E: 'height',
}


class CloakTargetGate:
    """Compose existing owners, rather than subclassing their setup API."""

    def __init__(self, root, map_path):
        (self.m, self.source, self.target, _, _, self.cells, self.rules,
         inherited) = composed.setup()
        self.u = self.m.u
        self.phase, self.read_layer = 'setup', None
        self.platform, self.reads, self.calls, self.gate_path, self.writes = [], [], [], [], []
        self.trace = None
        self.code = bytes(self.u.mem_read(0x401000, 0x3E0000))
        self.u.hook_add(UC_HOOK_CODE, self.observe)
        self.u.hook_add(UC_HOOK_MEM_WRITE, self.observe_write)
        self.layers = []
        for name in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini'):
            path = root / name
            if not path.exists():
                assert name == 'LANGRULE.INI', ('missing required layer', path)
                self.layers.append((name, None))
            else:
                self.layers.append((name, path.read_bytes()))
        self.layers.append((map_path.name, map_path.read_bytes()))
        self.art_raw = (root / 'ARTMD.INI').read_bytes()
        self.art, self.art_lines = {}, []
        self.types, self.weapons, self.bullets, self.warheads = {}, {}, {}, {}
        self.materialized = {}
        u, m = self.u, self.m
        u.mem_write(0xA8B238, dwords(5))
        # Existing registry transport: original factories still find/construct
        # each object. Warhead capacity must be initialized too, so references
        # from different weapons share the same original registry identity.
        for registry in (0x887568, 0x8874C0, 0xA8EB00, 0xA83CE0, 0xA8ED40,
                         0xA8E348, 0xA83DE8, 0xA83C98, 0x8B4120, 0xB0EC60):
            u.mem_write(registry, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
        m.invoke(0x665650, self.rules)
        self.refresh_art()
        for name in ('SUB', 'DEST'):
            pointer = m.alloc(0x1900)
            m.invoke(0x7470D0, pointer, (m.cstring(name),))
            self.types[name] = pointer
        self.inputs = dict(inherited_setup=inherited, constructor=self.physical_state(), layers=[])
        for index, (name, raw) in enumerate(self.layers):
            if raw is None:
                self.inputs['layers'].append(dict(file=name, absent=True))
                continue
            first_read = len(self.reads)
            self.select_cache(index, {'General'})
            general = m.invoke(0x66D530, self.rules, (RULES,), timeout_us=30_000_000) & 255
            self.select_cache(index, set(self.types))
            admitted = {label: m.invoke(0x747620, pointer, (RULES,)) & 255
                        for label, pointer in self.types.items()}
            # Type readers/factories establish actual native weapon identities.
            # A newly referenced object receives preceding physical layers too;
            # original registry discovery/sweep order is a supplied boundary.
            for pointer in self.types.values():
                for offset in (0x898, 0x8B4, 0xA94, 0xAB0):
                    reference = m.read32(pointer + offset)
                    if reference:
                        self.materialize(reference, self.weapons, 0x772080, index)
            for pointer in self.weapons.values():
                for offset, registry in ((0xA0, self.bullets), (0xAC, self.warheads)):
                    reference = m.read32(pointer + offset)
                    if reference:
                        label = m.string(reference + 0x24)
                        assert registry.get(label, reference) == reference, ('unshared native identity', label)
                        registry[label] = reference
            self.refresh_art()
            for registry, reader in ((self.bullets, 0x46BEE0), (self.warheads, 0x75D3A0)):
                for pointer in registry.values():
                    self.materialize(pointer, registry, reader, index)
            wanted = {'General', *self.types, *self.weapons, *self.bullets, *self.warheads}
            sections, lines = lexical(raw, wanted)
            if 'General' in sections:
                sections['General'] = {k: v for k, v in sections['General'].items() if k in GENERAL_KEYS}
            lines = [row for row in lines if row['section'] != 'General' or row['key'] in GENERAL_KEYS]
            self.inputs['layers'].append(dict(file=name, bytes=len(raw),
                sha256=hashlib.sha256(raw).hexdigest(), sections=sections, source_lines=lines,
                admitted=dict(general=general, types=admitted),
                reads=self.reads[first_read:], after=self.physical_state()))
        self.inputs['final'] = self.physical_state()
        self.inputs['art'] = dict(file='ARTMD.INI', bytes=len(self.art_raw),
            sha256=hashlib.sha256(self.art_raw).hexdigest(), sections=self.art,
            source_lines=self.art_lines)
        for actor, label in ((self.source, 'SUB'), (self.target, 'DEST')):
            u.mem_write(actor + 0x6C4, dwords(self.types[label]))
            u.mem_write(actor + 0x14, dwords(7))
            u.mem_write(actor + 0x150, struct.pack('<f', 0.0))
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_ESI, actor)
            u.reg_write(UC_X86_REG_EDI, 0xFFFFFFFF)
            run_checked(u, 0x735592, 0x7355C6)
            run_checked(u, 0x735416, 0x73541C)
            # Native Ship locomotor constructor/owner link; no COM/world load.
            loco = m.alloc(0x100)
            m.invoke(0x69EC50, loco)
            assert m.read32(loco + 4) == 0x7F2D8C
            u.mem_write(loco + 0xC, dwords(actor))
            u.mem_write(actor + 0x674, dwords(loco + 4))
            u.mem_write(actor + 0x74, b'\x01')
            u.mem_write(actor + 0x3D5, b'\x01')
        for cell in self.cells.values():
            u.mem_write(cell + 0xEC, dwords(2))
            u.mem_write(cell + 0x140, dwords(0))
        self.rngs = {'scenario': m.read32(0xA8B230) + 0x218,
                     'main': 0x886B88, 'mapgen': 0xABE890}
        for name, pointer in self.rngs.items():
            m.invoke(0x65C6D0, pointer, (31 if name == 'scenario' else 0,))
        self.inputs['world_boundary'] = dict(
            actor_types=['SUB', 'DEST'], flags=7, rank_bits='00000000',
            marked=True, in_playfield=True, house_indices=[0, 1],
            flat_cell_level=6, flat_ground_z=624, land_type=2, bridge_flags=0,
            source_cell=[10, 20], near_target_cell=[14, 20], far_target_cell=[24, 20],
            native_type_initialization=['0x735592..0x7355c6', '0x735416..0x73541c'],
            ship_constructor='0x69ec50', rng_seeds=dict(scenario=31, main=0, mapgen=0),
            game_mode=5, fpcw=hex(u.reg_read(UC_X86_REG_FPCW)))
        self.inputs['platform'] = self.platform.copy()
        self.inputs['asset_loads'] = m.asset_loaded.copy()
        assert self.code == bytes(u.mem_read(0x401000, 0x3E0000)), 'setup changed code'
        self.saved = [(start, bytes(u.mem_read(start, end - start + 1)))
                      for start, end, _ in u.mem_regions()]
        self.cpu, self.saved_cursor = u.context_save(), m.cursor

    def select_cache(self, index, names):
        self.read_layer, raw = self.layers[index]
        assert raw is not None
        sections, _ = lexical(raw, names)
        if 'General' in sections:
            sections['General'] = {k: v for k, v in sections['General'].items() if k in GENERAL_KEYS}
        self.m.rules_cache(sections)

    def refresh_art(self):
        # Image strings only select fixed ART sections; native readers parse
        # every field. No Python interpretation of a scalar/timer/range value.
        wanted = {'SUB', 'DEST', *self.bullets}
        for _, raw in self.layers:
            if raw is None:
                continue
            sections, _ = lexical(raw, wanted)
            wanted |= {section['Image'] for section in sections.values() if section.get('Image')}
        self.art, self.art_lines = lexical(self.art_raw, wanted)
        self.m.make_ini(self.art)

    def materialize(self, pointer, registry, reader, index):
        name = self.m.string(pointer + 0x24)
        prior = registry.get(name)
        assert prior is None or prior == pointer, ('unshared native registry identity', name)
        registry[name] = pointer
        # Distinct reader/object pairs, since Bullet/Warhead references are
        # gathered before their first read, and a weapon can appear twice.
        token = (reader, pointer)
        for layer in range(self.materialized.get(token, -1) + 1, index + 1):
            if self.layers[layer][1] is not None:
                self.select_cache(layer, {name})
                self.m.invoke(reader, pointer, (RULES,))
        self.materialized[token] = index

    def reference(self, pointer):
        return self.m.string(pointer + 0x24) if pointer else None

    def physical_state(self):
        m, u = self.m, self.u
        types = {}
        for name, pointer in self.types.items():
            types[name] = dict(
                cloakable=bool(u.mem_read(pointer + 0xCD0, 1)[0]),
                cloak_stop=bool(u.mem_read(pointer + 0xC93, 1)[0]),
                cloaking_speed=composed.signed(m, pointer + 0x310),
                naval=bool(u.mem_read(pointer + 0xCCE, 1)[0]),
                underwater=bool(u.mem_read(pointer + 0xD69, 1)[0]),
                strength=composed.signed(m, pointer + 0xA0),
                armor=composed.signed(m, pointer + 0x9C),
                land_targeting=composed.signed(m, pointer + 0x604),
                air_range_bonus=composed.signed(m, pointer + 0x68C),
                weapons=[self.reference(m.read32(pointer + 0x898 + 28 * i)) for i in range(2)],
                elite_weapons=[self.reference(m.read32(pointer + 0xA94 + 28 * i)) for i in range(2)])
        weapons = {name: dict(
            range=composed.signed(m, pointer + 0xB4), minimum_range=composed.signed(m, pointer + 0xB8),
            rof=composed.signed(m, pointer + 0xB0), damage=composed.signed(m, pointer + 0xA4),
            decloak_to_fire=bool(u.mem_read(pointer + 0x133, 1)[0]),
            projectile=self.reference(m.read32(pointer + 0xA0)),
            warhead=self.reference(m.read32(pointer + 0xAC))) for name, pointer in self.weapons.items()}
        bullets = {name: dict(aa=bool(u.mem_read(pointer + 0x2A4, 1)[0]),
            ag=bool(u.mem_read(pointer + 0x2A5, 1)[0]),
            arcing=bool(u.mem_read(pointer + 0x29B, 1)[0]), image=m.string(pointer + 0x1F8))
            for name, pointer in self.bullets.items()}
        return dict(types=types, weapons=weapons, projectiles=bullets,
            warheads={name: dict(verses_bits=[bytes(u.mem_read(pointer + 0xA0 + 8 * i, 8)).hex()
                for i in range(11)]) for name, pointer in self.warheads.items()},
            general=dict(cloaking_stages=composed.signed(m, self.rules + 0x628),
                cloak_delay_bits=f'{int.from_bytes(u.mem_read(self.rules + 0x1410, 8), "little"):016x}'))

    def observe(self, u, pc, size, user):
        m, sp = self.m, u.reg_read(UC_X86_REG_ESP)
        if self.phase == 'setup':
            if Reader.guid_transport(m, u, pc, sp, self.platform):
                return
            if pc in (0x5F8110, 0x5F8CE0):
                self.platform.append(dict(kind='type_visual_asset_boundary', pc=hex(pc)))
                m.ret(0)
                return
            if pc == 0x7C978A:
                self.platform.append(dict(kind='CRT_atexit_boundary'))
                m.ret(0)
                return
            if pc in (0x5276D0, 0x5295F0, 0x5283D0, 0x528A10):
                key = m.string(m.read32(sp + 8))
                if key in READ_KEYS:
                    row = dict(file=self.read_layer, reader=hex(pc),
                        section=m.string(m.read32(sp + 4)), key=key)
                    if pc == 0x5283D0:
                        row['default_f64_le_hex'] = bytes(u.mem_read(sp + 12, 8)).hex()
                    elif pc != 0x528A10:
                        row['default_u32'] = m.read32(sp + 12)
                    self.reads.append(row)
            return
        # No setup substitute is allowed inside the measured leaf/callees.
        assert pc not in (0x7C8E17, 0x7C8B3D, 0x7D140B, 0x5B40B0,
                          0x527AF9, 0x527B0C, 0x5F8110, 0x5F8CE0, 0x7C978A), hex(pc)
        assert pc not in (0x703770, 0x7036C0, 0x7258D0, 0x4D9720, 0x5F5280), hex(pc)
        assert pc not in (0x65C640, 0x65C660, 0x65C780, 0x65C7E0), ('unexpected query RNG', hex(pc))
        self.trace.returned(pc, sp)
        if pc in CALLS:
            index = self.trace.entered(pc, sp, CALLS[pc])
            call = self.calls[index]
            call['receiver'] = ('source' if u.reg_read(UC_X86_REG_ECX) == self.source else
                                'target' if u.reg_read(UC_X86_REG_ECX) == self.target else None)
            if pc == 0x6F7220:
                call['source_xyz'] = composed.coord(m, m.read32(sp + 4))
                call['weapon'] = self.reference(m.read32(sp + 12))
        if pc in GATES:
            self.gate_path.append(GATES[pc])

    def observe_write(self, u, access, address, size, value, user):
        if self.phase != 'measure':
            return
        for name, pointer in (('source', self.source), ('target', self.target)):
            if pointer <= address < pointer + 0x1000:
                self.writes.append(dict(actor=name, offset=hex(address - pointer),
                    size=size, value=value & ((1 << (size * 8)) - 1),
                    pc=hex(u.reg_read(UC_X86_REG_EIP))))

    def rng(self):
        return {name: bytes(self.u.mem_read(pointer, 0x3F4)).hex()
                for name, pointer in self.rngs.items()}

    def actor_state(self):
        m, u = self.m, self.u
        return dict(frame=composed.signed(m, 0xA8ED84),
            state=composed.signed(m, self.source + 0x220),
            progress=composed.signed(m, self.source + 0x224),
            raw_cloakable=bool(u.mem_read(self.source + 0x3D2, 1)[0]),
            target=m.read32(self.source + 0x2B4) == self.target,
            rearm=[composed.signed(m, self.source + x) for x in (0x2EC, 0x2F4)],
            recloak=[composed.signed(m, self.source + x) for x in (0x240, 0x248)],
            source_xyz=composed.coord(m, self.source + 0x9C),
            target_xyz=composed.coord(m, self.target + 0x9C),
            source_health=composed.signed(m, self.source + 0x6C),
            target_health=composed.signed(m, self.target + 0x6C))

    def execute(self, row):
        m, u = self.m, self.u
        self.phase = 'setup'
        for address, raw in self.saved:
            u.mem_write(address, raw)
        u.context_restore(self.cpu)
        m.cursor = self.saved_cursor
        u.mem_write(0xA8ED84, dwords(row['frame']))
        u.mem_write(self.source + 0x9C, dwords(*row['source_xyz']))
        u.mem_write(self.target + 0x9C, dwords(*row['target_xyz']))
        u.mem_write(self.source + 0x220, dwords(row['state'], row['progress']))
        u.mem_write(self.source + 0x2B4, dwords(self.target if row['target'] else 0))
        for offset, field in ((0x2EC, 'rearm'), (0x240, 'recloak')):
            start, duration = row[field]
            u.mem_write(self.source + offset, dwords(start, 0, duration))
        u.mem_write(self.source + 0x2B0, dwords(0))
        before, rng_before = self.actor_state(), self.rng()
        actor_bytes = {name: bytes(u.mem_read(pointer, 0x1000))
                       for name, pointer in (('source', self.source), ('target', self.target))}
        self.calls, self.gate_path, self.writes = [], [], []
        self.trace = NativeCallTrace(u, m.read32, self.calls)
        self.phase = 'measure'
        answer = m.invoke(CAN_AUTO, self.source) & 255
        self.trace.returned(RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
        self.phase = 'setup'
        assert not self.trace.pending, self.trace.pending
        assert u.reg_read(UC_X86_REG_ESP) == SP + 4
        rng_after = self.rng()
        assert rng_before == rng_after, 'pure-query RNG changed'
        assert self.code == bytes(u.mem_read(0x401000, 0x3E0000)), 'query changed code'
        changes = {name: changed_byte_spans(actor_bytes[name], bytes(u.mem_read(pointer, 0x1000)))
                   for name, pointer in (('source', self.source), ('target', self.target))}
        assert not any(changes.values()), ('query mutated actors', changes)
        range_calls = [call for call in self.calls if call['name'] == 'can_fire_at_target']
        assert len(range_calls) <= 1
        if row['name'] in ('retained_far', 'retained_near'):
            # Admission, not a supplied verdict: both ordinary controls must
            # actually enter every selected original query dependency.
            reached = {call['name'] for call in self.calls}
            assert {'can_fire_at_target', 'unit_select_weapon', 'get_weapon', 'in_range'} <= reached
        return dict(input=row, before=before, after=self.actor_state(), can_auto=bool(answer),
            can_fire_at_target=bool(range_calls[0]['return_eax'] & 255) if range_calls else None,
            calls=self.calls, gate_path=self.gate_path, writes=self.writes,
            actor_byte_changes=changes, rng_before=rng_before, rng_after=rng_after)


def cases():
    base = dict(frame=173, source_xyz=[2688, 5248, 624], target_xyz=[6272, 5248, 624],
                target=True, state=0, progress=0, rearm=[-1, 0], recloak=[-1, 0])
    return [
        dict(base, name='retained_far'),
        dict(base, name='retained_near', target_xyz=[3712, 5248, 624]),
        dict(base, name='null_target', target=False),
        dict(base, name='fully_cloaked_early', state=2),
        dict(base, name='live_weapon_rearm_early', rearm=[172, 2]),
        dict(base, name='paused_weapon_rearm_early', rearm=[-1, 1]),
        dict(base, name='weapon_rearm_exact_deadline', rearm=[161, 12]),
        dict(base, name='far_then_progress', progress=1),
        dict(base, name='far_then_live_recloak', recloak=[172, 2]),
        dict(base, name='far_then_recloak_exact_deadline', recloak=[161, 12]),
    ]


def generate():
    root = assets_root()
    map_path = Path(os.environ['VERA20K_CLOAK_MAP'])
    assert map_path.is_file() and map_path.read_bytes().lstrip().startswith((b';', b'[')), \
        'VERA20K_CLOAK_MAP must be the selected physical inner map INI'
    machine = CloakTargetGate(root, map_path)
    return dict(schema_version=1, native_sha256=image_sha256(), inputs=machine.inputs,
                rows=[machine.execute(row) for row in cases()], native_code_unchanged=True)


def metadata():
    return provenance(
        scope='Whole original SUB CanAutoCloak with real retained-target SelectWeapon/GetWeapon/CanFireAt/InRange and native-read physical inputs',
        assumptions=[
            'Existing bridge_target_composed.setup/BulletReader own the VM, native scalar CRT groups, sparse flat Cell table, cached lexical INI transport, allocation and physical SHP transport. Its earlier FV/HoverMissile setup is retained provenance, not a C28 actor/type/whole-scenario claim.',
            'Fresh Rules665650 and SUB/DEST UnitType7470D0 constructors execute. Whole General66D530 sees only physical CloakingStages/CloakDelay; whole UnitType747620, selected Weapon772080, Bullet46BEE0 and Warhead75D3A0 see their physical selected sections across RULESMD, optional LANGRULE, MPBattleMD and the exact selected map, in that order. Fixed ART is not layered. Native registry discovery/order and archive/file parsing remain supplied boundaries.',
            'Selected normal/elite slots0/1 establish dependencies using original native pointers/names. Newly encountered dependencies receive earlier physical layers through original readers. Unrelated General keys are excluded. Type visual/voxel loading is an explicit setup boundary; this is not an asset-rendering or whole Rules Process closure.',
            'Source SUB and target DEST are supplied live marked actors, hostile human House indexes0/1, rookie rank, in-playfield membership and coordinates, with original type-to-Ammo/Cloak/health initialization735592..7355C6 and Ship69EC50 constructors. Their full construction, Unlimbo, world-list admission, terrain loading, visibility, diplomacy admission, targeting orders and movement are not executed.',
            'Ten independent component inputs supply cloak state/progress, current frame, weapon-rearm/recloak timers and target identity. These are not gameplay injections in a production capture. All selected CanAuto6FBDC0 instructions and reached real target range/weapon/map query callees execute. CanFireAtTarget6F7780 asks weapon selection/range, not full weapon legality/GetFireError.',
            'Scenario31, Main0 and Mapgen0 are seeded by original65C6D0; each row records all1012 bytes of every stream before/after and rejects a query draw or mutation. No CloakingTick, transition, timer producer, DetachAll, sensor outer admission, audio, drawing or whole-game RNG claim is made.',
        ],
        substitutions=[
            'Inherited allocation/delete/CRT-TLS and physical cached-INI/SHP transports; Windows GUID conversion uses shared Reader.guid_transport. Type visual asset functions and CRT atexit are setup-only boundaries, recorded and forbidden during measured query execution.',
            'Explicit sparse flat water Cell/actor/House/timer inputs. No gameplay callable, RNG, range value, result, weapon selection or cloak verdict is replaced.',
        ],
        entry_points=dict(can_auto_cloak=CAN_AUTO, can_fire_at_target=0x6F7780,
            can_fire_at=0x6F77B0, unit_select_weapon=0x746CD0,
            techno_select_weapon=0x6F3330, get_weapon=0x70E140, in_range=0x6F7220,
            unit_type_ctor=0x7470D0, unit_type_read=0x747620,
            rules_ctor=0x665650, general_read=0x66D530,
            weapon_read=0x772080, bullet_read=0x46BEE0, warhead_read=0x75D3A0))


if __name__ == '__main__':
    finish_vectors(generate, HERE / 'cloak_target_gate.json', provenance=metadata)
