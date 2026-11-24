"""Original FireAt FireOnce continuation and concrete team target release.

The existing Anytown FootMissions owner supplies construction, readers and world.
This additive fixture supplies the already-admitted FireAt caller frame, team
roster/prior orders and accepted/refused Bullet result. It runs original cleanup
instructions; it does not establish the preceding shot or whole-scenario parity.
"""

import hashlib
import os
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
)

from tools.native_oracle import (
    RET_MAGIC, finish_vectors, provenance, run_checked,
)
from tools import native_oracle
from tools.projectile_oracle.bridge_render_inputs import lexical
from tools.spatial_oracle.anytown_damage.foot_missions import FootMissions
from tools.spatial_oracle.anytown_damage import mission as native_owner
from tools.spatial_oracle.building_body_rules import RULES, SP, dwords


ROLES = ('firer', 'unit_both', 'infantry_nav', 'unit_unrelated')
ART_SECTIONS = {
    'GI', 'GISequence', 'MTNK', 'GTNK', 'IVAN', 'IvanSequence',
    'ENGINEER', 'EngineerSequence', 'HTNK',
}
WEAPON_SECTIONS = {
    'IvanBomber', 'IvanBomberE', 'DefuseKit', 'VirtualScanner',
    'Invisible', 'InvisibleAll', 'IvanBomb', 'BombDisarm', 'DummyWarhead',
}
SHAPES = ('GI.SHP', 'IVAN.SHP', 'ENGINEER.SHP')
CASES = (
    dict(name='ivan_no_team', team=False),
    dict(name='ivan_ready_no_team', team=False, action=0),
    dict(name='engineer_no_team', source='ENGINEER', team=False),
    dict(name='ivan_fire_once_false', fire_once=False),
    dict(name='ivan_team_clear_old_focus'),
    dict(name='ivan_team_clear_null_focus', focus=None),
    dict(name='ivan_team_preserve_other_focus', focus='other_target'),
    dict(name='ivan_team_no_old_target', old_target=None),
    dict(name='ivan_team_already_pending', pending=True),
    dict(name='ivan_failed_launch_team', continuation='failed_launch'),
    dict(name='ivan_failed_ballistic_team', continuation='failed_ballistic'),
    dict(name='engineer_team_clear_old_focus', source='ENGINEER'),
    dict(name='ivan_non_foot_flag', foot_flag=False),
)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


class FireOnce:
    """Composition of unchanged fixture owners; no second native VM/INI owner."""

    def __init__(self):
        self.q = q = FootMissions()
        self.m, self.u = m, u = q.m, q.u
        self.input_root = Path(os.environ['VERA20K_FIRE_ONCE_INPUTS'])
        self.shapes = []
        for name in SHAPES:
            raw = (self.input_root / name).read_bytes()
            m.assets[name] = raw
            self.shapes.append(dict(name=name, bytes=len(raw), sha256=sha(raw),
                                    frame_count=struct.unpack_from('<H', raw, 6)[0]))
        q.initialize_companion()
        art_path = Path(os.environ['VERA20K_SHRAPNEL_INPUTS']) / 'ARTMD.INI'
        art, _ = lexical(art_path.read_bytes(), ART_SECTIONS)
        m.make_ini(art)
        self.types, self.weapons, self.reader_layers, self.weapon_defaults = {}, {}, [], []
        for name in ('IVAN', 'ENGINEER', 'HTNK'):
            typ = m.alloc(0x1900)
            m.invoke(0x7470D0 if name == 'HTNK' else 0x5236A0, typ,
                     (m.cstring(name),))
            self.types[name] = typ
            for file, path in native_owner.base.layers():
                if not path.exists():
                    self.reader_layers.append(dict(type_id=name, file=file, absent=True))
                    continue
                sections, _ = lexical(path.read_bytes(), {name})
                m.rules_cache(sections)
                q.phase = 'setup'
                admitted = m.invoke(0x747620 if name == 'HTNK' else 0x5240A0,
                                    typ, (RULES,)) & 255
                self.reader_layers.append(dict(type_id=name, file=file,
                    sha256=sha(path.read_bytes()), type_reader_admitted_al=admitted,
                    image=m.string(typ + 0x1F8)))
            if name == 'HTNK':
                continue
            weapon = m.read32(typ + 0x898)
            self.weapons[name] = weapon
            self.weapon_defaults.append(dict(type_id=name, weapon=m.string(weapon + 0x24),
                fire_once=bool(u.mem_read(weapon + 0x135, 1)[0])))
            for file, path in native_owner.base.layers():
                if not path.exists():
                    continue
                sections, _ = lexical(path.read_bytes(), WEAPON_SECTIONS)
                m.rules_cache(sections)
                admitted = m.invoke(0x772080, weapon, (RULES,)) & 255
                m.invoke(0x46BEE0, m.read32(weapon + 0xA0), (RULES,))
                m.invoke(0x75D3A0, m.read32(weapon + 0xAC), (RULES,))
                m.invoke(0x7729F0, weapon)
                self.reader_layers.append(dict(type_id=name, file=file,
                    weapon=m.string(weapon + 0x24), weapon_reader_admitted_al=admitted,
                    fire_once=bool(u.mem_read(weapon + 0x135, 1)[0])))
        self.actors, self.placements = {}, []
        for name, xy in (('IVAN', (86, 49)), ('ENGINEER', (88, 49)), ('HTNK', (86, 51))):
            actor = m.alloc(0x1000)
            self.actors[name] = actor
            m.invoke(0x7353C0 if name == 'HTNK' else 0x517A50, actor,
                     (self.types[name], 0))
            u.mem_write(actor + 0x21C, dwords(q.house))
            u.mem_write(actor + 0x14C, dwords(q.house))
            q.phase = 'placement'
            m.invoke(0x486840, q.resident.ptrs[xy], (q.coord,))
            admitted = m.invoke(0x737BA0 if name == 'HTNK' else 0x51DFF0,
                                actor, (q.coord, 0x80)) & 255
            if not admitted:
                raise AssertionError(('FireOnce actor placement refused', name, xy))
            self.placements.append(dict(type_id=name, cell=list(xy), unlimbo_al=admitted))
        self.team = m.alloc(0x100)
        output = m.alloc(4)
        factory_result = m.invoke(0x6C5090, 0, (0, 0, 0x7F7C90, output))
        self.bullet = m.read32(output)
        if factory_result or not self.bullet:
            raise AssertionError(('Original Bullet factory refused fixture interface', factory_result))
        self.bullet_refcount = m.read32(self.bullet + 0x1C)
        self.actor_types = {
            self.actors['IVAN']: 'IVAN', self.actors['ENGINEER']: 'ENGINEER',
            q.src: 'MTNK', q.e1: 'E1', q.victim: 'MTNK', q.candidate: 'MTNK',
            self.actors['HTNK']: 'HTNK',
        }
        spans = {p: 0x1000 for p in self.actor_types}
        spans.update({m.read32(p + 0x674) - 4: 0x100 for p in self.actor_types})
        spans.update({w: 0x200 for w in self.weapons.values()})
        spans.update({self.team: 0x100, self.bullet: 0x180})
        spans.update({p: 1012 for p in q.resident.rngs.values()})
        self.baseline = {p: bytes(u.mem_read(p, size)) for p, size in spans.items()}
        self.text_hash = sha(bytes(u.mem_read(0x401000, 0x3E0000)))
        self.vtables = {}
        for actor in self.actor_types:
            for table, size in ((m.read32(actor), 0x600),
                                (m.read32(m.read32(actor + 0x674)), 0x100)):
                self.vtables[table] = bytes(u.mem_read(table, size))
        self.vtables[m.read32(self.bullet)] = bytes(u.mem_read(m.read32(self.bullet), 0x200))
        q.phase = 'logic'

    def rng(self):
        return {name: native_owner.base.sr.rng_state(self.u, ptr)
                for name, ptr in self.q.resident.rngs.items()}

    def state(self, roster, labels, has_team):
        q, m, u = self.q, self.m, self.u
        def label(pointer):
            return labels.get(pointer, hex(pointer))
        members = []
        for role, actor in zip(ROLES, roster):
            snap = q.snap(actor)
            members.append(dict(role=role, type_id=self.actor_types[actor],
                abstract_flags=m.read32(actor + 0x14), health=native_owner.base.i32(u, actor + 0x6C),
                alive=u.mem_read(actor + 0x90, 1)[0], limbo=u.mem_read(actor + 0x81, 1)[0],
                position=snap['position'], on_bridge=snap['on_bridge'],
                locomotor_head=snap['loco_head'],
                target=label(m.read32(actor + 0x2B4)),
                destination=label(m.read32(actor + 0x5A4)),
                archive=label(m.read32(actor + 0x218)),
                mission=snap['mission'], queued_mission=snap['queued'], status=snap['status'],
                burst=m.read32(actor + 0x3B8), rearm_timer=list(struct.unpack('<3i', u.mem_read(actor + 0x2EC, 12))),
                dispatch_timer=list(struct.unpack('<3i', u.mem_read(actor + 0xC8, 12))),
                targeting_timer=snap['targeting_timer'], idle_timer=snap['idle_timer'],
                mission_visit=snap['visit'],
                doing=snap['doing'], firing=snap['firing'], scan=snap['scan'],
                path=list(struct.unpack('<24i', u.mem_read(actor + 0x5E0, 96))),
                path_reference=list(struct.unpack('<2h', u.mem_read(actor + 0x558, 4))),
                sequence_timer=list(struct.unpack('<4i', u.mem_read(actor + 0x100, 16))),
                frame_f8=native_owner.base.i32(u, actor + 0xF8),
                movement_timer=list(struct.unpack('<3i', u.mem_read(actor + 0x640, 12))),
                movement_retry=native_owner.base.i32(u, actor + 0x64C),
                nav_queue_count=m.read32(actor + 0x598)))
        team = None
        if has_team:
            team = dict(mission_target=label(m.read32(self.team + 0x3C)),
                focus=label(m.read32(self.team + 0x40)),
                advance_pending=bool(u.mem_read(self.team + 0x80, 1)[0]),
                is_leaving_map=bool(u.mem_read(self.team + 0x82, 1)[0]))
        return dict(current_frame=m.read32(0xA8ED84), team=team, members=members)

    def case(self, control):
        q, m, u = self.q, self.m, self.u
        for pointer, raw in self.baseline.items():
            u.mem_write(pointer, raw)
        settings = dict(source='IVAN', team=True, fire_once=True, focus='old_target',
                        old_target='old_target', pending=False, foot_flag=True,
                        continuation='accepted', action=4)
        settings.update(control)
        source = self.actors[settings['source']]
        infantry_peer = self.actors['ENGINEER' if settings['source'] == 'IVAN' else 'IVAN']
        roster = [source, q.src, infantry_peer, q.e1]
        old, other = self.actors['HTNK'], q.candidate
        labels = {0: None, old: 'old_target', other: 'other_target', self.team: 'team',
                  self.bullet: 'bullet', **dict(zip(roster, ROLES))}
        pointer_by_label = {value: pointer for pointer, value in labels.items()}
        weapon = self.weapons[settings['source']]
        if not settings['fire_once']:
            # Read the only changed key through the existing original reader.
            m.rules_cache({m.string(weapon + 0x24): {'FireOnce': 'no'}})
            m.invoke(0x772080, weapon, (RULES,))
        u.mem_write(0xA8ED84, dwords(1337))
        u.mem_write(self.team + 0x3C, dwords(pointer_by_label[settings['old_target']],
                                            pointer_by_label[settings['focus']]))
        u.mem_write(self.team + 0x54, dwords(source))
        u.mem_write(self.team + 0x80, bytes([settings['pending']]))
        u.mem_write(self.team + 0x82, b'\1')
        for index, actor in enumerate(roster):
            u.mem_write(actor + 0x5D4, dwords(self.team if settings['team'] else 0,
                roster[index + 1] if index + 1 < len(roster) else 0))
            u.mem_write(actor + 0xAC, dwords(1))
            u.mem_write(actor + 0xB4, dwords(-1))
            m.invoke(m.read32(m.read32(actor) + 0x3C8), actor,
                     (old if index < 2 else other,))
            if index:
                m.invoke(m.read32(m.read32(actor) + 0x480), actor,
                         (old if index < 3 else other, 1))
        m.invoke(0x51D6F0, source, (settings['action'], 1, 0))
        u.mem_write(source + 0x68D, bytes([settings['action'] == 4]))
        if not settings['foot_flag']:
            u.mem_write(source + 0x14, dwords(m.read32(source + 0x14) & ~4))
        before = self.state(roster, labels, settings['team'])
        rng_before = self.rng()
        u.mem_write(SP - 0x1000, bytes(0x3000))
        u.mem_write(SP + 0x1000, dwords(0, RET_MAGIC, old, 0))
        u.mem_write(SP + 0x40, dwords(weapon))
        u.mem_write(SP + 0x3C, dwords(self.bullet))
        entry = {'accepted': 0x6FF749, 'failed_launch': 0x6FF01A,
                 'failed_ballistic': 0x6FF93C}[settings['continuation']]
        for register, value in ((UC_X86_REG_ESI, source), (UC_X86_REG_EBP, SP + 0x1000),
                                (UC_X86_REG_ESP, SP), (UC_X86_REG_EBX, self.bullet),
                                (UC_X86_REG_EAX, 0)):
            u.reg_write(register, value)
        calls, writes, rng_calls, journal = [], [], [], []
        instruction = [0]
        entries = {
            0x6E9050: ('team_assign_mission_target', 1),
            0x51B1F0: ('infantry_assign_target', 1), 0x6FCDB0: ('techno_assign_target', 1),
            0x5B35E0: ('queue_mission', 2), 0x741970: ('unit_destination', 2),
            0x51AA40: ('infantry_destination', 2), 0x4D94B0: ('foot_destination', 2),
            0x51D6F0: ('infantry_action', 3), 0x466560: ('bullet_destructor', 0),
            0x7258D0: ('expiry_broadcast', 0), 0x5F3B80: ('detach_all', 0),
        }
        scalar_fields = {0xAC: 'mission', 0xB4: 'queued_mission', 0xBC: 'status',
            0x2B4: 'target', 0x5A4: 'destination', 0x3B8: 'burst', 0x68D: 'firing',
            0xF8: 'frame_f8', 0x100: 'sequence_start', 0x108: 'sequence_duration',
            0x2EC: 'rearm_start', 0x2F4: 'rearm_duration', 0xC8: 'dispatch_start',
            0xD0: 'dispatch_duration', 0x6C4: 'doing'}
        def observe(uc, address, size, data):
            instruction[0] += 1
            stack = uc.reg_read(UC_X86_REG_ESP)
            if address in entries:
                kind, count = entries[address]
                raw_args = [m.read32(stack + 4 + index * 4) for index in range(count)]
                args = list(raw_args)
                if kind in ('team_assign_mission_target', 'infantry_assign_target',
                            'techno_assign_target', 'unit_destination',
                            'infantry_destination', 'foot_destination'):
                    args[0] = labels.get(args[0], args[0])
                calls.append(dict(instruction=instruction[0], pc=hex(address), kind=kind,
                    role=labels.get(uc.reg_read(UC_X86_REG_ECX), hex(uc.reg_read(UC_X86_REG_ECX))),
                    args=args, raw_args=raw_args))
            if address in (0x65C640, 0x65C660, 0x65C780, 0x65C7E0):
                rng_calls.append(dict(instruction=instruction[0], pc=hex(address),
                    stream=next((name for name, ptr in q.resident.rngs.items()
                                 if ptr == uc.reg_read(UC_X86_REG_ECX)),
                                hex(uc.reg_read(UC_X86_REG_ECX)))))
        def written(uc, access, address, size, value, data):
            journal.append((address, bytes(uc.mem_read(address, size))))
            role = None
            offset = 0
            field = None
            if self.team <= address < self.team + 0x100:
                role, offset = 'team', address - self.team
                field = {0x3C: 'mission_target', 0x40: 'focus', 0x80: 'advance_pending',
                         0x82: 'is_leaving_map'}.get(offset)
            else:
                for member_role, actor in zip(ROLES, roster):
                    if actor <= address < actor + 0x1000:
                        role, offset = member_role, address - actor
                        field = scalar_fields.get(offset)
                        break
            if field:
                writes.append(dict(instruction=instruction[0], pc=hex(uc.reg_read(UC_X86_REG_EIP)),
                    role=role, field=field, offset=hex(offset), size=size,
                    value=labels.get(value, value) if field in ('target', 'destination', 'focus', 'mission_target') else value))
        q.events.clear()
        q.pending.clear()
        h = u.hook_add(UC_HOOK_CODE, observe)
        w = u.hook_add(UC_HOOK_MEM_WRITE, written)
        try:
            required = (0x6FF749, 0x6FF8F1, 0x6FF92F)
            if settings['continuation'] != 'accepted':
                required += (0x466560,)
            run_checked(u, entry, RET_MAGIC, count=2_000_000,
                        required_addresses=required)
            after = self.state(roster, labels, settings['team'])
            result = dict(name=control['name'], supplied=dict(**settings, frame=1337,
                member_order=list(ROLES), infantry_initial_action=settings['action'],
                source_target='old_target', source_initial_nav=None,
                boundaries='FireAt caller continuation and team prior orders supplied'),
                entry=hex(entry), before=before, after=after,
                return_bullet=bool(u.reg_read(UC_X86_REG_EAX)),
                calls=calls, writes=writes, rng_before=rng_before, rng_after=self.rng(),
                rng_calls=rng_calls, instruction_count=instruction[0],
                native_text_unchanged=self.text_hash == sha(bytes(u.mem_read(0x401000, 0x3E0000))),
                native_vtables_unchanged=all(raw == bytes(u.mem_read(ptr, len(raw)))
                                             for ptr, raw in self.vtables.items()))
            if not result['native_text_unchanged'] or not result['native_vtables_unchanged']:
                raise AssertionError('Native code or class vtable changed')
            return result
        finally:
            u.hook_del(h)
            u.hook_del(w)
            for pointer, raw in reversed(journal):
                u.mem_write(pointer, raw)

    def corpus(self):
        return dict(schema_version=1,
            inputs=dict(shapes=self.shapes, reader_layers=self.reader_layers,
                        placements=self.placements, weapon_defaults=self.weapon_defaults,
                        house=dict(is_human=self.u.mem_read(self.q.house + 0x1EC, 1)[0],
                                   player_control=self.u.mem_read(self.q.house + 0x1ED, 1)[0]),
                        bullet_factory_refcount=self.bullet_refcount),
            cases=[self.case(control) for control in CASES])


def generate():
    return FireOnce().corpus()


def metadata():
    result = provenance(scope=__doc__, entry_points={
        'accepted_fireat_common_tail': 0x6FF749, 'refused_bullet_fire_result': 0x6FF01A,
        'failed_ballistic_cleanup': 0x6FF93C, 'fire_once': 0x6FF8F1,
        'team_assign_mission_target': 0x6E9050, 'team_pending_write': 0x6FF91C,
        'infantry_target': 0x51B1F0, 'techno_target': 0x6FCDB0,
        'queue_mission': 0x5B35E0, 'unit_destination': 0x741970,
        'infantry_destination': 0x51AA40, 'foot_destination': 0x4D94B0,
        'infantry_action': 0x51D6F0, 'weapon_reader': 0x772080,
        'bullet_factory': 0x6C5090, 'bullet_destructor': 0x466560,
        'bullet_addref': 0x46AFD0, 'bullet_release': 0x46AFF0,
    }, assumptions=[
        'Unchanged Anytown FootMissions/Mission owners build the physical crop, source MTNK/E1, enemy candidate, class constructors, actual Drive/Walk factories, original scalar/type readers and Unlimbo/registration. Additional original IVAN/ENGINEER/HTNK type readers and constructors, weapon/projectile/warhead readers and original placement execute. The single supplied human House remains inherited.',
        'Selected retail RULESMD, optional LANGRULE, stock MPBattleMD and XMP03T4.MAP strings pass through original INI readers using supplied lexical CRC caches. ARTMD is fixed. Physical GI/IVAN/ENGINEER SHPs load through the existing Reader file-buffer owner; no Rust-produced scalar initializes native type/weapon/action fields.',
        'Each control restores actor/locomotor/weapon/team/Bullet and complete RNG baselines. Native setters produce member targets and destinations; then prior Attack1/queuedNone, team roster/pointers/focus, LeavingMap1, frame1337 and firer original DoAction FireUp4/firing1 or Ready0/firing0 are explicit supplied continuations. The non-Foot bit and FireOnce=no are synthetic controls, with the changed FireOnce key read by original772080.',
        'Original6C5090 factory constructs a Bullet and returns its interface with the original COM reference count. The already-admitted accepted route begins at6FF749 with that supplied nonnull Bullet result. Refused BulletFire begins at6FF01A with supplied AL0; the failed ballistic route begins at6FF93C. Both must execute original Bullet466560 destruction/null result and the common FireOnce tail. No FireAt prefix, actual launch, bomb attach/defuse effect or input admission runs in these controls; refused and failed-ballistic results are not claims about stock Inviso Ivan launch frequency.',
        'Original6E9050 queues Guard5 and calls original concrete member destination/target setters in linked member order, preserving unrelated focus/targets; the native FireAt pending write and final repeated firer target setter execute. Complete current/queued mission, action/path/timers, raw call/write order and all three RNG states are observations. Guard is queued; no TeamAI or immediate Script cursor step is executed.',
        'The prepared Team storage is not a Team constructor/Add proof. Tests exclude outside-map target assignment, aircraft/buildings, spawned/temporal/airstrike/subordinate target lifecycles, deployment, other FireOnce weapons, world scheduling, persistence, rendering, audio-device and whole-engine/whole-scene parity. Native text and the actual actor/locomotor/Bullet class vtables are checked unchanged.',
    ], substitutions=[
        'Inherited Mission/Reader successful bump allocation, inert operator-delete, CRT TLS/file buffers/INI caches, empty Windows SEH, wall-clock/atexit/ASCII/CLSID/Interlocked/OleRun/COM activation transports remain. Actual class constructors and concrete gameplay cleanup bodies are not substituted.',
        'Existing native fixture visual asset/radar/audio sinks, cropped zone/theater input and single-House boundary remain. Team object/prior roster and FireAt caller/launch result are supplied; original queue, destination, action, target, RNG and timer results are never replaced.',
    ])
    result['commands'] = ['python -m tools.projectile_oracle.fire_once --write',
                          'python -m tools.projectile_oracle.fire_once --check']
    result['required_inputs'] = ['VERA20K_GAMEMD_EXE', 'VERA20K_SHRAPNEL_INPUTS',
                                 'VERA20K_ANYTOWN_INPUTS', 'VERA20K_FIRE_ONCE_INPUTS']
    result['field_mapping'] = {
        'current_frame': dict(address='0x00A8ED84', format='u32'),
        'house': {
            'is_human': dict(offset='0x1EC', format='u8'),
            'player_control': dict(offset='0x1ED', format='u8'),
        },
        'members': {
            'abstract_flags': dict(offset='0x14', format='u32'),
            'health': dict(offset='0x6C', format='i32'),
            'alive': dict(offset='0x90', format='u8'),
            'limbo': dict(offset='0x81', format='u8'),
            'position': dict(offset='0x9C', format='3xi32', units='world leptons'),
            'on_bridge': dict(offset='0x8C', format='u8'),
            'locomotor_head': dict(base='member+0x674 interface',
                                  offset='Walk+0x24 or Drive+0x3C', format='3xi32', units='world leptons'),
            'target': dict(offset='0x2B4', format='pointer label'),
            'destination': dict(offset='0x5A4', format='pointer label'),
            'archive': dict(offset='0x218', format='pointer label'),
            'mission': dict(offset='0xAC', format='i32'),
            'queued_mission': dict(offset='0xB4', format='i32'),
            'status': dict(offset='0xBC', format='i32'),
            'mission_visit': dict(offset='0xC4', format='u32'),
            'burst': dict(offset='0x3B8', format='u32'),
            'rearm_timer': dict(offset='0x2EC', format='3xi32'),
            'dispatch_timer': dict(offset='0xC8', format='3xi32'),
            'targeting_timer': dict(offset='0x180', format='3xi32'),
            'idle_timer': dict(offset='0x168', format='3xi32'),
            'doing': dict(offset='0x6C4', format='i32', applies_to='Infantry only; null for Unit'),
            'firing': dict(offset='0x68D', format='u8'),
            'scan': dict(offset='0x688', format='u8'),
            'path': dict(offset='0x5E0', format='24xi32'),
            'path_reference': dict(offset='0x558', format='2xi16'),
            'sequence_timer': dict(offset='0x100', format='4xi32'),
            'frame_f8': dict(offset='0xF8', format='i32'),
            'movement_timer': dict(offset='0x640', format='3xi32'),
            'movement_retry': dict(offset='0x64C', format='i32'),
            'nav_queue_count': dict(offset='0x598', format='u32'),
        },
        'team': {
            'mission_target': dict(offset='0x3C', format='pointer label'),
            'focus': dict(offset='0x40', format='pointer label'),
            'advance_pending': dict(offset='0x80', format='u8 boolean'),
            'is_leaving_map': dict(offset='0x82', format='u8 boolean'),
        },
        'rng_stream': {
            'disabled': dict(offset='0x0', format='u8 then three padding bytes'),
            'index_a': dict(offset='0x4', format='i32'),
            'index_b': dict(offset='0x8', format='i32'),
            'state': dict(offset='0xC', format='250xu32'),
        },
        'bullet_factory_refcount': dict(offset='0x1C', format='u32', source='Original46AFD0/46AFF0 Interlocked increment/decrement operand'),
        'timer_words': 'Three words are start-frame, raw unused native auxiliary, duration; Sequence adds its repeat rate as word four (DoAction copies selected duration into it). Auxiliary words retain copied caller scratch as observations, not authoritative Rust timer state. Original51DA3A loads uninitialized caller[ESP+10];51DA3E copies it to Sequence+104. Rust imports/compares timer words0/2 and Sequence rate3.',
        'pointer_labels': 'null is NULL; old_target is the original placed HTNK; other_target is the inherited original enemy MTNK. Member roles follow supplied.member_order and type_id; unit_unrelated is E1 despite its stable role name.',
    }
    files = []
    for name, path in native_owner.base.layers():
        files.append(dict(name=name, absent=True) if not path.exists()
                     else dict(name=name, bytes=path.stat().st_size, sha256=sha(path.read_bytes())))
    for name in ('ARTMD.INI', 'SOUNDMD.INI', 'TEMPERATMD.INI'):
        path = Path(os.environ['VERA20K_SHRAPNEL_INPUTS']) / name
        files.append(dict(name=name, bytes=path.stat().st_size, sha256=sha(path.read_bytes())))
    for name in SHAPES:
        path = Path(os.environ['VERA20K_FIRE_ONCE_INPUTS']) / name
        files.append(dict(name=name, bytes=path.stat().st_size, sha256=sha(path.read_bytes())))
    result['physical_files'] = files
    return result


if __name__ == '__main__':
    sources = {'fire_once': Path(__file__), 'native_oracle': Path(native_oracle.__file__),
               'foot_missions': Path(FootMissions.__init__.__code__.co_filename).with_name('foot_missions.py'),
               'mission': Path(native_owner.__file__), 'mtnk_attack': Path(native_owner.base.__file__)}
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata,
                   source_paths=sources)
