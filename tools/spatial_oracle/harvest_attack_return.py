"""Original War Miner Attack -> expiry -> idle/Harvest mission continuation.

This extends harvest_field's sole map/Drive/ore fixture. Complete original event
execution, concrete pointer expiry, targetless Attack/idle, Ready/Commence and
Harvest bodies run. Live-target Approach is an explicit declared return seam;
full firing, damage, world detach traversal and whole UnitAI are excluded.
Run python -m tools.spatial_oracle.harvest_attack_return --check; see the sibling .md.
"""
import hashlib
import os
from pathlib import Path
import struct
import sys
from itertools import product

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX,
                               UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_ESI,
                               UC_X86_REG_EDI, UC_X86_REG_EBP, UC_X86_REG_EBX)
from tools.native_oracle import (NATIVE_SHA256, RET_MAGIC, finish_vectors,
                                 provenance, run_checked)
from tools.projectile_oracle.bridge_render_inputs import lexical
from tools.projectile_oracle.guided_step import i32
from tools.rules_oracle.bridge_landing_inputs import Landing
from tools.spatial_oracle.building_body_rules import INI, SP as READER_SP
from tools.spatial_oracle.harvest_field import fixture, field_state
from tools.spatial_oracle.refinery_dock import (ACTOR, TYPE, HOUSE, RULES, EXTRA,
                                               MINER_ITEMS, BLD_ITEMS, cell)
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.shrapnel_repair.shrapnel_repair import rng_state
from tools.spatial_oracle.unit_scatter_state import SP
from tools.spatial_oracle.unit_source_scatter import SCENARIO

FRAME, CONTROLS, UNIT_VTABLE = 0xA8ED84, 0xA8E3A8, 0x7F5C70
DISPATCH, ATTACK, FOOT_ATTACK, IDLE = 0x5B3060, 0x7447A0, 0x4D4DC0, 0x738970
READY, COMMENCE, QUEUE, SET_TARGET, SET_DEST = 0x744270, 0x5B3570, 0x5B35E0, 0x6FCDB0, 0x741970
APPROACH, EXPIRY, EVENT_CTOR, EVENT_EXECUTE = 0x7414E0, 0x7446E0, 0x4C6860, 0x4C6CB0
STOP_CTOR, POST_FOOT_AI = 0x4C65E0, 0x7365BB
BREAK_CONTACT = 0x65ACB0
TARGET, TARGET_TYPE, ENEMY, EVENT, HOUSE_ARRAY, ID_INDEX = (EXTRA + n for n in
                                                         (0x22000, 0x24000, 0x26000, 0x28000, 0x29000, 0x2A000))
SECOND = TARGET + 0x800
APPROACH_SEAM = RET_MAGIC + 0x100
MISSIONS = {'sleep': 0, 'attack': 1, 'move': 2, 'guard': 5, 'harvest': 10, 'stop': 13}
SELECT = {'HARV': {'Harvester', 'Weeder', 'Storage', 'MovementZone', 'Dock'},
          'General': {'HarvesterLoadRate', 'TiberiumShortScan', 'TiberiumLongScan'},
          **{s: {'Rate', 'AARate', 'NoThreat', 'Zombie', 'Recruitable', 'Paralyzed', 'Retaliate', 'Scatter'}
             for s in ('Sleep', 'Attack', 'Guard', 'Harvest', 'Move', 'Stop')}}


def reader_receipts(*, refinery_dock=False, root=None):
    """Physical lexical strings, original constructors and key-block readers."""
    root = Path(root or os.environ.get('VERA20K_HARVEST_ATTACK_INPUTS',
                                      'target/asset/harvest-attack-return/extract'))
    m = Landing()
    u = m.u
    for registry in (0x887568, 0xA8EB00, 0xA83CE0, 0xA8ED40, 0xA83C68):
        u.mem_write(registry, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
    typ, rules = m.alloc(0xF00), m.alloc(0x5000)
    m.invoke(0x7470D0, typ, [m.cstring('HARV')])
    m.invoke(0x665650, rules)
    m.invoke(0x4E7CF0, 0)
    docking = None
    selected, missions = SELECT, MISSIONS
    if refinery_dock:
        from tools.spatial_oracle.refinery_dock import DockInputReader
        docking = DockInputReader(m, typ, rules)
        selected = {name: set(keys) for name, keys in SELECT.items()}
        for name, keys in docking.selected.items():
            selected.setdefault(name, set()).update(keys)
        missions = MISSIONS | {'enter': 7, 'unload': 16}

    def snap():
        return dict(harvester=u.mem_read(typ + 0xE0E, 1)[0],
                    weeder=u.mem_read(typ + 0xE0F, 1)[0],
                    storage=i32(u, typ + 0x800), movement_zone=i32(u, typ + 0x5B4),
                    dock=[dict(name=m.string(m.read32(m.read32(typ + 0x3EC) + n * 4) + 0x24),
                               fixture_type_index=i32(u, m.read32(m.read32(typ + 0x3EC) + n * 4) + 0xDF8))
                          for n in range(m.read32(typ + 0x3F8))],
                    load_rate=i32(u, rules + 0x1520),
                    short_scan=i32(u, rules + 0x1778), long_scan=i32(u, rules + 0x177C),
                    mission_controls={name: bytes(u.mem_read(CONTROLS + n * 32, 32)).hex()
                                      for name, n in missions.items()})

    constructor = snap()
    rows, calls, writes = [], [], []
    current = [None]
    readers = {0x5295F0: 'ReadBool', 0x5276D0: 'ReadInt', 0x474620: 'ReadRange',
               0x474E40: 'ReadMovementZone', 0x5283D0: 'ReadDouble',
               0x528A10: 'ReadString', 0x7C9CC2: 'strtok', 0x4653C0: 'BuildingTypeFindOrAllocate'}

    def observe(uc, address, size, data):
        if address in readers:
            sp = uc.reg_read(UC_X86_REG_ESP)
            count = {0x5283D0: 4, 0x528A10: 5, 0x7C9CC2: 2, 0x4653C0: 0}.get(address, 3)
            calls.append(dict(layer=current[0], pc=hex(address), reader=readers[address],
                              caller=hex(m.read32(sp)), this=hex(uc.reg_read(UC_X86_REG_ECX)),
                              args=[m.read32(sp + 4 * (i + 1)) for i in range(count)]))

    fields = {typ + 0xE0E: 'harvester', typ + 0xE0F: 'weeder', typ + 0x800: 'storage',
              typ + 0x5B4: 'movement_zone', typ + 0x3EC: 'dock_items', typ + 0x3F8: 'dock_count',
              rules + 0x1520: 'load_rate',
              rules + 0x1778: 'short_scan', rules + 0x177C: 'long_scan'}

    def written(uc, access, address, size, value, data):
        if address in fields or CONTROLS <= address < CONTROLS + 32 * 32:
            writes.append(dict(layer=current[0], pc=hex(uc.reg_read(UC_X86_REG_EIP)),
                               field=fields.get(address, 'mission_control'),
                               address=hex(address), size=size, value=value))

    h, w = u.hook_add(UC_HOOK_CODE, observe), u.hook_add(UC_HOOK_MEM_WRITE, written)
    try:
        for filename in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'XMP03T4.MAP'):
            path = root / filename
            if not path.is_file():
                assert filename == 'LANGRULE.INI', str(path)
                rows.append(dict(file=filename, absent=True))
                if docking:
                    docking.layers.append(dict(file=filename, absent=True))
                continue
            current[0] = filename
            raw = path.read_bytes()
            sections, lines = lexical(raw, set(selected))
            m.make_ini(sections)
            before, start_calls, start_writes = snap(), len(calls), len(writes)
            if docking:
                docking.begin_layer(filename, raw, sections, lines)
            # 526810 returns a Section pointer, not a boolean AL. A valid
            # aligned section can have lowbyte0 (refinery_dock reader controls).
            if m.invoke(0x526810, INI, [m.cstring('HARV')]):
                for register, value in ((UC_X86_REG_ESP, READER_SP), (UC_X86_REG_EDI, typ),
                                        (UC_X86_REG_EBX, INI), (UC_X86_REG_EBP, typ + 0x24)):
                    u.reg_write(register, value)
                run_checked(u, 0x74769F, 0x7476D3, required_addresses=(0x7476AE, 0x7476C8))
                for register, value in ((UC_X86_REG_ESP, READER_SP), (UC_X86_REG_EBP, typ),
                                        (UC_X86_REG_ESI, INI), (UC_X86_REG_EBX, typ + 0x24)):
                    u.reg_write(register, value)
                run_checked(u, 0x713129, 0x713143, required_addresses=(0x5276D0,))
                # Reader-local vector constructor and unchanged Dock block.
                # Preserve preceding DebrisMaximums result across its store.
                m.invoke(0x5AC900, READER_SP + 0x20, [0, 0])
                for register, value in ((UC_X86_REG_ESP, READER_SP), (UC_X86_REG_EBP, typ),
                                        (UC_X86_REG_ESI, INI), (UC_X86_REG_EBX, typ + 0x24),
                                        (UC_X86_REG_EAX, m.read32(typ + 0x3BC))):
                    u.reg_write(register, value)
                run_checked(u, 0x713171, 0x713264, required_addresses=(0x71318E, 0x713239))
                for register, value in ((UC_X86_REG_ESP, READER_SP), (UC_X86_REG_EBP, typ),
                                        (UC_X86_REG_EBX, typ + 0x24)):
                    u.reg_write(register, value)
                # The original 524EC0 getter leaves two pushed arguments live.
                u.mem_write(READER_SP + 0x380, dwords(INI))
                run_checked(u, 0x71605E, 0x716090, required_addresses=(0x474E40, 0x716081))
            if m.invoke(0x526810, INI, [m.cstring('General')]):
                for begin, end in ((0x670CE7, 0x670D07), (0x67028C, 0x6702CB)):
                    for register, value in ((UC_X86_REG_ESP, READER_SP), (UC_X86_REG_ESI, rules),
                                            (UC_X86_REG_EDI, INI)):
                        u.reg_write(register, value)
                    run_checked(u, begin, end)
            if docking:
                docking.read_layer()
            u.reg_write(UC_X86_REG_ESP, READER_SP)
            u.reg_write(UC_X86_REG_ESI, INI)
            run_checked(u, 0x679C92, 0x679CAF, required_addresses=(0x5B3760,))
            if docking:
                docking.end_layer()
            rows.append(dict(file=filename, bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(),
                             selected={s: {k: v for k, v in values.items() if k in selected[s]}
                                       for s, values in sections.items()},
                             lines=[r for r in lines if r['key'] in selected[r['section']]],
                             before=before, after=snap(), calls=calls[start_calls:],
                             writes=writes[start_writes:]))
        if docking:
            docking.read_art(root / 'ARTMD.INI' if (root / 'ARTMD.INI').is_file()
                             else Path('ini/ARTMD.INI'))
    finally:
        u.hook_del(h)
        u.hook_del(w)
        if docking:
            docking.close()
    result = dict(constructor=constructor, layers=rows, after=snap())
    if docking:
        result['refinery_dock'] = dict(constructor=docking.constructor, layers=docking.layers,
                                       art=docking.art, after=docking.snap())
    return result


OBSERVED = {DISPATCH: ('mission_dispatch', 0), ATTACK: ('unit_attack', 0), FOOT_ATTACK: ('foot_attack', 0),
            IDLE: ('unit_idle', 2), 0x4D82B0: ('foot_idle', 2), READY: ('ready', 0),
            COMMENCE: ('commence', 0), QUEUE: ('queue', 2), SET_TARGET: ('set_target', 1),
            SET_DEST: ('set_destination', 2), APPROACH: ('live_approach_boundary', 1),
            EXPIRY: ('unit_pointer_expired', 2), 0x4D9960: ('foot_pointer_expired', 2),
            0x7077C0: ('techno_pointer_expired', 2), 0x65AAC0: ('radio_pointer_expired', 2),
            0x5F5230: ('object_pointer_expired', 2), 0x70FEE0: ('linked_detach', 1),
            0x70D4A0: ('world_detach', 1), 0x5B3A00: ('current_mission_control', 0),
            0x65C7E0: ('scenario_range', 2), 0x65C780: ('scenario_next', 0),
            EVENT_CTOR: ('event_constructor', 10), STOP_CTOR: ('stop_constructor', 4),
            EVENT_EXECUTE: ('event_execute', 0),
            0x4DF0E0: ('foot_command_setup', 1), 0x4DF1A0: ('clear_saved_command', 0),
            0x6386E0: ('clear_planning', 0), 0x65A970: ('radio_transmit', 3),
            BREAK_CONTACT: ('transmit_to_contact', 1),
            0x73E5E0: ('harvest', 0), 0x73D450: ('harvest_ore_tick', 0),
            0x4DCE80: ('is_cell_harvestable', 2), 0x4DD0A0: ('scan_ore', 3),
            0x4DCFE0: ('search_ore', 2), 0x70C610: ('archive_setter', 1)}
FIELDS = {0xAC: 'mission', 0xB0: 'suspended', 0xB4: 'queued', 0xB8: 'force_mission',
          0xBC: 'status', 0xC0: 'mission_started', 0xC4: 'mission_visits',
          0xC8: 'dispatch_start', 0xCC: 'dispatch_aux', 0xD0: 'dispatch_duration',
          0xF8: 'stage', 0xFC: 'stage_changed', 0x100: 'stage_start', 0x108: 'stage_duration', 0x10C: 'stage_rate', 0x110: 'stage_step',
          0x180: 'targeting_start', 0x184: 'targeting_aux', 0x188: 'targeting_duration',
          0x218: 'archive', 0x2B4: 'target', 0x2B8: 'suspended_target',
          0x598: 'nav_queue_count', 0x5A0: 'aux_destination', 0x5A4: 'destination',
          0x5A8: 'suspended_destination', 0x5C4: 'saved_mission', 0x5C8: 'saved_destination',
          0x5CC: 'saved_target', 0x5D1: 'attack_move_engaged', 0x6B3: 'idle_latch',
          0x6D1: 'unloading', 0x6D2: 'harvesting'}


class AttackReturn:
    def __init__(self, case, inputs):
        self.case = case
        base = dict(case, mission=case.get('mission', 'guard'), native_idle=True, native_ready=True,
                    miner_cell=[15, 15], linked=case.get('linked', False))
        self.u, self.call, self.read32, self.callbacks, self.unused = fixture(base)
        u, r = self.u, self.read32
        for name, off, size in (('harvester', 0xE0E, 1), ('weeder', 0xE0F, 1),
                                ('storage', 0x800, 4), ('movement_zone', 0x5B4, 4)):
            if name in ('harvester', 'weeder') and name in case:
                value = int(case[name])
            else:
                value = inputs['after'][name]
            u.mem_write(TYPE + off, bytes([value]) if size == 1 else dwords(value))
        for name, off in (('load_rate', 0x1520), ('short_scan', 0x1778), ('long_scan', 0x177C)):
            u.mem_write(RULES + off, dwords(case.get(name, inputs['after'][name])))
        for name, number in MISSIONS.items():
            u.mem_write(CONTROLS + number * 32, bytes.fromhex(inputs['after']['mission_controls'][name]))
        for obj, typ, house, xyz in ((ACTOR, TYPE, HOUSE, [3968, 3968, 0]),
                                    (TARGET, TARGET_TYPE, ENEMY, [4224, 3968, 0]),
                                    (SECOND, TARGET_TYPE, ENEMY, [3968, 4224, 0])):
            for off, value in ((0, UNIT_VTABLE), (0x14, 7), (0x6C, 1000), (0x21C, house),
                               (0x6C4, typ), (0xB0, -1), (0x458, 0x7E91EC), (0x470, 0x7E91EC)):
                u.mem_write(obj + off, dwords(value))
            u.mem_write(obj + 0x90, b'\x01')
            u.mem_write(obj + 0x9C, dwords(*xyz))
            if obj != ACTOR:
                u.mem_write(obj + 0xB4, dwords(-1))
            # Original clear establishes the no-saved-MegaMission gate.
            self.call(0x4DF1A0, obj, [])
            # Original Foot constructor's literal legacy team/planning slot.
            u.reg_write(UC_X86_REG_ESI, obj)
            run_checked(u, 0x4D31F1, 0x4D31FB, required_addresses=(0x4D31F1,))
        u.mem_write(TARGET_TYPE, dwords(0x7F6218))
        u.mem_write(ID_INDEX, dwords(101, ACTOR, 202, TARGET, 303, SECOND))
        u.mem_write(0xB0E840, dwords(ID_INDEX, 3))
        u.mem_write(0xB0E84C, b'\x01')
        u.mem_write(0xB0E850, dwords(0))
        u.mem_write(0xB78828, dwords(0))
        u.mem_write(HOUSE_ARRAY, dwords(HOUSE))
        u.mem_write(0xA8022C, dwords(HOUSE_ARRAY))
        u.mem_write(0xA83D4C, dwords(HOUSE))
        u.mem_write(HOUSE + 0x1ED, b'\x01')
        u.mem_write(0xA8E7AC, dwords(0))
        u.mem_map(0, 0x1000)
        u.mem_write(0, dwords(-1))
        u.mem_write(APPROACH_SEAM, b'\xB8' + dwords(case.get('approach_return', 0)) + b'\xC2\x04\x00')
        if 'current' in case:
            u.mem_write(ACTOR + 0xAC, dwords(case['current']))
        if 'queued_value' in case:
            u.mem_write(ACTOR + 0xB4, dwords(case['queued_value']))
        for name, offset in (('saved_mission', 0x5C4), ('targeting_timer', 0x180),
                             ('dispatch_timer', 0xC8)):
            if name in case:
                value = case[name]
                u.mem_write(ACTOR + offset, dwords(*(value if isinstance(value, list) else [value])))
        if case.get('target'):
            self.call(SET_TARGET, ACTOR, [TARGET])
        if case.get('sensor'):
            u.mem_write(cell(16, 15) + 0x7C, struct.pack('<h', case['sensor']))
        if case.get('same_owner'):
            u.mem_write(TARGET + 0x21C, dwords(HOUSE))
        self.idle_control_receipt = None
        if 'idle_control' in case:
            control = case['idle_control']
            # Read-only critic controls: original IsArmed701120 follows
            # GetCurrentWeapon70E1A0 -> GetWeapon70E140 -> Type7177C0.
            # HasTurrets=0 selects the supplied slot0 WeaponStruct pointer;
            # no weapon body executes. The scalar controls force Guard rather
            # than the separate IQ/DefaultToGuardArea branch.
            u.mem_write(TYPE + 0x898, dwords(TYPE + 0x1800 if control['armed'] else 0))
            u.mem_write(HOUSE + 0x24C, dwords(control['house_iq']))
            u.mem_write(RULES + 0x1440, dwords(control['guard_area_iq']))
            u.mem_write(ACTOR + 0x3B8, dwords(control['burst']))
            u.mem_write(TYPE + 0x404, dwords(TYPE + 0x1C00 if control['deploys_into'] else 0))
            self.call(0x701120, ACTOR, [])
            self.idle_control_receipt = dict(
                is_armed_receiver=hex(r(UNIT_VTABLE + 0x2AC)),
                is_armed_eax=u.reg_read(UC_X86_REG_EAX),
                has_turrets=r(TYPE + 0x808),
                primary_weapon_pointer=hex(r(TYPE + 0x898)),
                default_to_guard_area=u.mem_read(TYPE + 0xD39, 1)[0],
                house_iq=i32(u, HOUSE + 0x24C),
                guard_area_iq=i32(u, RULES + 0x1440),
                deploys_into=hex(r(TYPE + 0x404)))
        if 'alive' in case:
            u.mem_write(ACTOR + 0x90, bytes([case['alive']]))
        if 'health' in case:
            u.mem_write(ACTOR + 0x6C, dwords(case['health']))
        self.observed = OBSERVED
        if case.get('area_guard_comparison'):
            # This mode retains complete independent streams. Original seeding
            # gives Main/MapGen valid runtime states; no draw answer is supplied.
            self.call(0x65C6D0, 0x886B88, [31])
            self.call(0x65C6D0, 0xABE890, [42])
            self.observed = OBSERVED | {
                0x744100: ('unit_area_guard', 0),
                0x4D6AA0: ('foot_area_guard', 0),
                0x50B730: ('controlled_by_human', 0),
            }
            assert r(UNIT_VTABLE + 0x220) == 0x744100
        self.events, self.writes, self.pending, self.steps = [], [], {}, []
        self.inline_draw = None
        self.instruction = 0
        self.phase = 'fixture'
        self.original_text = hashlib.sha256(bytes(u.mem_read(0x401000, 0x3E0000))).hexdigest()
        self.original_vtable = bytes(u.mem_read(UNIT_VTABLE, 0x600))
        assert r(UNIT_VTABLE + 0x210) == ATTACK
        assert r(UNIT_VTABLE + 0x484) == IDLE
        assert r(UNIT_VTABLE + 0x3C8) == SET_TARGET
        assert r(UNIT_VTABLE + 0x28) == EXPIRY
        assert r(UNIT_VTABLE + 0x274) == BREAK_CONTACT
        u.hook_add(UC_HOOK_CODE, self.observe)
        u.hook_add(UC_HOOK_MEM_WRITE, self.written)
        self.callbacks.clear()
        self.before = self.state()

    def state(self):
        u, r = self.u, self.read32
        result = field_state(u, r, self.case)
        result.update(frame=i32(u, FRAME), target=hex(r(ACTOR + 0x2B4)), suspended=i32(u, ACTOR + 0xB0),
                      alive=u.mem_read(ACTOR + 0x90, 1)[0], health=i32(u, ACTOR + 0x6C),
                      saved_mission=i32(u, ACTOR + 0x5C4), saved_target=hex(r(ACTOR + 0x5CC)),
                      saved_destination=hex(r(ACTOR + 0x5C8)), nav_queue_count=r(ACTOR + 0x598),
                      dispatch_words=list(struct.unpack('<3i', u.mem_read(ACTOR + 0xC8, 12))),
                      targeting_words=list(struct.unpack('<3i', u.mem_read(ACTOR + 0x180, 12))),
                      event_bytes=bytes(u.mem_read(EVENT, 0x6F)).hex(),
                      stage_changed=u.mem_read(ACTOR + 0xFC, 1)[0], stage_step=i32(u, ACTOR + 0x110),
                      target_alive=u.mem_read(TARGET + 0x90, 1)[0], target_health=i32(u, TARGET + 0x6C),
                      scenario_rng=rng_state(u, SCENARIO + 0x218))
        if self.idle_control_receipt is not None:
            result['burst_index'] = i32(u, ACTOR + 0x3B8)
        if self.case.get('area_guard_comparison'):
            result['other_rng'] = {name: rng_state(u, address) for name, address in
                                   (('main', 0x886B88), ('mapgen', 0xABE890))}
        return result

    def resolve_returns(self, address):
        for event in self.pending.pop(address, []):
            event['returned_eax'] = self.u.reg_read(UC_X86_REG_EAX)
            if event['kind'] in ('ready', 'commence'):
                event['returned_al'] = event['returned_eax'] & 255

    def observe(self, u, address, size, data):
        self.instruction += 1
        self.resolve_returns(address)
        # RandomRanged65C7E0 inlines its Scenario raw generator and rejection
        # loop; these checkpoints record original register results and branch.
        if self.inline_draw is not None:
            if address == 0x65C882:
                self.inline_draw['masked_eax'] = u.reg_read(UC_X86_REG_EAX)
            elif address in (0x65C82E, 0x65C884):
                self.inline_draw['accepted'] = address == 0x65C884
                self.inline_draw = None
        if address == 0x65C87E:
            pointer = u.reg_read(UC_X86_REG_EDX)
            assert pointer == SCENARIO + 0x218
            self.inline_draw = dict(kind='scenario_inline_draw', pc=hex(address),
                                    phase=self.phase, frame=i32(u, FRAME),
                                    instruction=self.instruction, raw_eax=u.reg_read(UC_X86_REG_EAX),
                                    mask=u.reg_read(UC_X86_REG_EBP), span=u.reg_read(UC_X86_REG_EDI),
                                    indices_after=[self.read32(pointer + 4), self.read32(pointer + 8)])
            self.events.append(self.inline_draw)
        if address not in self.observed:
            return
        kind, count = self.observed[address]
        sp, r = u.reg_read(UC_X86_REG_ESP), self.read32
        event = dict(instruction=self.instruction, phase=self.phase, pc=hex(address), kind=kind,
                     frame=i32(u, FRAME), caller=hex(r(sp)), this=hex(u.reg_read(UC_X86_REG_ECX)),
                     args=[r(sp + 4 * (i + 1)) for i in range(count)],
                     target=hex(r(ACTOR + 0x2B4)), current=i32(u, ACTOR + 0xAC), queued=i32(u, ACTOR + 0xB4))
        if address in (0x65C7E0, 0x65C780):
            assert u.reg_read(UC_X86_REG_ECX) == SCENARIO + 0x218
        self.events.append(event)
        if address != APPROACH:
            self.pending.setdefault(r(sp), []).append(event)

    def written(self, u, access, address, size, value, data):
        # These controls expose sender-before-receiver BREAK contact clears;
        # retain historical rows' writer schema and observations unchanged.
        if self.phase == 'break_contact' and address in (MINER_ITEMS, BLD_ITEMS):
            self.writes.append(dict(instruction=self.instruction, phase=self.phase,
                                    pc=hex(u.reg_read(UC_X86_REG_EIP)),
                                    field='miner_contact_slot' if address == MINER_ITEMS else 'refinery_contact_slot',
                                    address=hex(address), size=size, value=value))
        if address - ACTOR in FIELDS or ACTOR + 0x33C <= address < ACTOR + 0x34C:
            self.writes.append(dict(instruction=self.instruction, phase=self.phase,
                                    pc=hex(u.reg_read(UC_X86_REG_EIP)),
                                    field=FIELDS.get(address - ACTOR, 'cargo'),
                                    offset=hex(address - ACTOR), size=size, value=value))
        if self.idle_control_receipt is not None and address == ACTOR + 0x3B8:
            self.writes.append(dict(instruction=self.instruction, phase=self.phase,
                                    pc=hex(u.reg_read(UC_X86_REG_EIP)), field='burst_index',
                                    offset='0x3b8', size=size, value=value))

    def invoke(self, entry, this=ACTOR, args=(), *, seam=False):
        end = self.call(entry, this, list(args), (RET_MAGIC, APPROACH) if seam else RET_MAGIC)
        if end == APPROACH:
            # The original virtual CALL reached its unchanged concrete receiver.
            # Execute a declared RET4 outside the image, then the original caller.
            sp = self.u.reg_read(UC_X86_REG_ESP)
            return_pc = self.read32(sp)
            self.events.append(dict(kind='supplied_approach_return', phase=self.phase,
                                    original_receiver=hex(APPROACH), caller=hex(return_pc),
                                    flag=self.read32(sp + 4), returned_eax=self.case.get('approach_return', 0)))
            run_checked(self.u, APPROACH_SEAM, return_pc, required_addresses=(APPROACH_SEAM,))
            run_checked(self.u, return_pc, RET_MAGIC, count=300000, required_addresses=(return_pc,))
        self.resolve_returns(RET_MAGIC)
        return self.u.reg_read(UC_X86_REG_EAX)

    def step(self, instruction):
        self.phase = instruction['op']
        u, r = self.u, self.read32
        before, start, wstart, cstart = self.state(), len(self.events), len(self.writes), len(self.callbacks)
        if 'frame' in instruction:
            u.mem_write(FRAME, dwords(instruction['frame']))
        op = instruction['op']
        answer = None
        if op == 'command':
            if instruction['mission'] == 'area_guard':
                # G730D60 emits mission11 with the current Cell as Target.
                # Execute the same original event constructor/receiver as every
                # other command; Event4C73EF owns the archive/destination writes.
                post = instruction['post']
                args = [0, 101, 0x34, 11, post[1] * 1000 + post[0], 0xB, 0, 0, 0, 0]
            else:
                mission = MISSIONS[instruction['mission']]
                target_id = 303 if instruction.get('target') == 'second' else 202 if instruction.get('target') else 0
                destination = instruction.get('destination')
                dest_id = (destination[1] * 1000 + destination[0]) if destination else 0
                args = [0, 101, 0x34, mission, target_id, 0x34 if target_id else 0,
                        dest_id, 0xB if destination else 0, 0, 0]
            self.invoke(EVENT_CTOR, EVENT, args)
            answer = self.invoke(EVENT_EXECUTE, EVENT)
        elif op == 'stop':
            self.invoke(STOP_CTOR, EVENT, [0, 6, 101, 0x34])
            answer = self.invoke(EVENT_EXECUTE, EVENT)
        elif op == 'break_contact':
            # Unchanged Unit+274 receiver used by Foot's4D92D0 exit caller:
            # first transmit BREAK3, then caller invokes UnitIdle(0,1).
            answer = self.invoke(BREAK_CONTACT, args=[3])
        elif op == 'dispatch':
            answer = self.invoke(DISPATCH, seam=True)
        elif op == 'techno_stage_ai':
            # Original TechnoAI dispatches at6FA655, then advances Stage here.
            u.reg_write(UC_X86_REG_ESI, ACTOR)
            u.reg_write(UC_X86_REG_EBP, 0)  # original TechnoAI head XOR EBP,EBP
            u.reg_write(UC_X86_REG_ESP, SP)
            u.mem_write(SP + 0x2C, dwords(0))  # declared caller-local timer padding
            run_checked(u, 0x6FABB8, 0x6FAC31, required_addresses=(0x6FABB8,))
            answer = {'original_stage_prefix_end': hex(0x6FAC31)}
        elif op == 'post_foot_ai':
            # Original alive/effective-mission/latch prefix before Unit Fire.
            assert u.mem_read(ACTOR + 0x90, 1)[0]
            u.reg_write(UC_X86_REG_ESI, ACTOR)
            u.reg_write(UC_X86_REG_ESP, SP)
            run_checked(u, POST_FOOT_AI, 0x7365DF, required_addresses=(POST_FOOT_AI,))
            answer = {'original_prefix_end': hex(0x7365DF)}
        elif op == 'ready':
            answer = {'ready_al': self.invoke(READY) & 255}
        elif op == 'promote':
            ready = self.invoke(READY)
            answer = {'ready_al': ready & 255}
            if ready & 255:
                answer['commence_al'] = self.invoke(COMMENCE) & 255
        elif op == 'expiry':
            victim = SECOND if instruction.get('pointer') == 'second' else TARGET
            if instruction.get('dead', False):
                u.mem_write(victim + 0x6C, dwords(0))
                u.mem_write(victim + 0x90, b'\x00')
            answer = self.invoke(EXPIRY, args=[victim, instruction.get('control', 1)])
        elif op == 'deadline':
            start_frame, _, duration = struct.unpack('<3i', u.mem_read(ACTOR + 0xC8, 12))
            assert start_frame >= 0 and duration >= 0
            frame = start_frame + duration + instruction.get('delta', 0)
            u.mem_write(FRAME, dwords(frame))
            answer = {'supplied_frame_from_previous_native_timer': frame}
        elif op == 'idle':
            answer = self.invoke(IDLE, args=instruction.get('args', [0, 1]))
        else:
            raise AssertionError(op)
        self.steps.append(dict(input=instruction, before=before, after=self.state(), returned=answer,
                               events=self.events[start:], writes=self.writes[wstart:],
                               callback_events=self.callbacks[cstart:]))

    def run(self):
        for step in self.case['steps']:
            self.step(step)
        assert not self.pending, self.pending
        assert self.inline_draw is None
        assert not any(self.unused[0]), self.unused
        assert hashlib.sha256(bytes(self.u.mem_read(0x401000, 0x3E0000))).hexdigest() == self.original_text
        assert bytes(self.u.mem_read(UNIT_VTABLE, 0x600)) == self.original_vtable
        result = dict(input=self.case, before=self.before, after=self.state(), steps=self.steps,
                      native_text_sha256=self.original_text, original_code_and_vtable_unchanged=True,
                      instruction_count=self.instruction)
        if self.idle_control_receipt is not None:
            result['idle_control_receipt'] = self.idle_control_receipt
        return result


def cases():
    ore = [[15, 15, 0, 0, 5]]
    rows = []
    for land, cargo in product(('ore', 'clear'), (0, 12, 40)):
        steps = [dict(op='command', mission='attack', target='first'), dict(op='promote'),
                 dict(op='dispatch'), dict(op='post_foot_ai'), dict(op='expiry', dead=True), dict(op='dispatch'),
                 dict(op='deadline'), dict(op='dispatch'), dict(op='post_foot_ai'), dict(op='promote')]
        if land == 'ore':
            steps += [dict(op='dispatch'), dict(op='techno_stage_ai')]
            if cargo < 40:
                steps += [step for _ in range(19) for step in
                          (dict(op='deadline'), dict(op='dispatch'), dict(op='techno_stage_ai'))]
        rows.append(dict(name=f'attack_expiry_{land}_cargo_{cargo}', ore=ore if land == 'ore' else [],
                         storage=[cargo, 0, 0, 0], steps=steps))
    for name, frame, timer in [('before_deadline', 214, [200, 0, 15]), ('exact_deadline', 215, [200, 0, 15]),
                                ('after_deadline', 216, [200, 0, 15]), ('paused_positive', 999, [-1, 0, 1]),
                                ('paused_zero', 999, [-1, 0, 0])]:
        rows.append(dict(name=name, ore=ore, current=1, frame=frame, dispatch_timer=timer,
                         steps=[dict(op='dispatch')]))
    for seed in (0, 1, 31):
        rows.append(dict(name=f'targetless_seed_{seed}', seed=seed, ore=ore, current=1,
                         dispatch_timer=[200, 0, 0], steps=[dict(op='dispatch'), dict(op='promote')]))
    for name, changes in [('current_harvest', dict(current=10)), ('queued_harvest', dict(current=1, queued_value=10)),
                           ('radio_contact', dict(current=1, linked=True)), ('installed_navcom', dict(current=1, nav=[17, 15])),
                           ('saved_attack_move', dict(current=1, saved_mission=29))]:
        rows.append(dict(name=name, ore=ore, steps=[dict(op='idle')], **changes))
    for name, changes in [('human_clear', dict(human=True)), ('ai_clear', dict(human=False)),
                           ('forced_human_clear', dict(human=True))]:
        args = [1, 1] if name == 'forced_human_clear' else [0, 1]
        rows.append(dict(name=name, ore=[], current=1, steps=[dict(op='idle', args=args)], **changes))
    for name, changes in [('dead_source', dict(alive=0)), ('zero_health_source', dict(health=0))]:
        rows.append(dict(name=name, ore=ore, current=1, dispatch_timer=[200, 0, 0],
                         steps=[dict(op='dispatch')], **changes))
    for name, control, timer, sensor, same in [('expiry_active_scan', 1, [200, 0, 30], 0, False),
                                               ('expiry_remaining_ten', 1, [200, 0, 10], 0, False),
                                               ('expiry_live_enemy', 0, [200, 0, 30], 0, False),
                                               ('expiry_live_sensor_retains', 0, [200, 0, 30], 1, False),
                                               ('expiry_live_same_owner_retains', 0, [200, 0, 30], 0, True)]:
        rows.append(dict(name=name, ore=ore, current=1, target=True, targeting_timer=timer,
                         dispatch_timer=[200, 0, 0], sensor=sensor, same_owner=same,
                         steps=[dict(op='expiry', control=control), dict(op='dispatch')]))
    initial = [dict(op='command', mission='attack', target='first'), dict(op='promote'), dict(op='dispatch')]
    for land in ('ore', 'clear'):
        rows.append(dict(name=f'stop_attack_{land}', ore=ore if land == 'ore' else [],
                         steps=initial + [dict(op='stop'), dict(op='dispatch'), dict(op='deadline'),
                                          dict(op='dispatch'), dict(op='promote')]))
    rows.append(dict(name='stop_current_harvest', ore=ore, current=10, status=1, harvesting=True,
                     steps=[dict(op='stop')]))
    for name, command in [('retask_move', dict(op='command', mission='move', destination=[17, 15])),
                           ('retask_attack', dict(op='command', mission='attack', target='second'))]:
        rows.append(dict(name=name, ore=ore, steps=initial + [dict(op='expiry', dead=True), dict(op='deadline'),
                         dict(op='dispatch'), command, dict(op='promote')]))
    rows.append(dict(name='non_harvester_harvest_hold', ore=ore, current=10, harvester=False, weeder=False,
                     dispatch_timer=[200, 0, 0], steps=[dict(op='dispatch')]))
    rows.append(dict(name='attack_retained_harvest_latch', ore=ore, current=1, harvesting=True,
                     dispatch_timer=[200, 0, 0], steps=[dict(op='dispatch'), dict(op='post_foot_ai'),
                                                      dict(op='promote'), dict(op='post_foot_ai')]))
    rows.append(dict(name='harvest_retains_harvest_latch', ore=ore, current=10, harvesting=True,
                     steps=[dict(op='post_foot_ai')]))
    for name, harvester in (('depot_break_nav_plain', False), ('depot_break_nav_harv', True)):
        rows.append(dict(name=name, ore=ore, current=7, linked=True, nav=[17, 15],
                         harvester=harvester, weeder=False, human=True,
                         steps=[dict(op='break_contact'), dict(op='idle'), dict(op='promote')]))
    for human, land in product((True, False), ('ore', 'clear')):
        rows.append(dict(name=f'depot_break_no_nav_{"human" if human else "ai"}_{land}',
                         ore=ore if land == 'ore' else [], current=7, linked=True,
                         harvester=True, weeder=False, human=human,
                         steps=[dict(op='break_contact'), dict(op='idle'), dict(op='promote')]))
    for name, current, armed, deploys_into in (
            ('armed_move', 2, True, False), ('unarmed_patrol', 25, False, False),
            ('armed_patrol', 25, True, False), ('unarmed_guard', 5, False, False),
            ('unarmed_areaguard', 11, False, False), ('unarmed_wait', 28, False, False),
            ('unarmed_sleep', 0, False, False), ('armed_sleep', 0, True, False),
            ('unarmed_unload_deploys_into', 16, False, True), ('unarmed_unload', 16, False, False)):
        rows.append(dict(name=f'plain_idle_{name}', ore=[], current=current, target=True,
                         harvester=False, weeder=False,
                         idle_control=dict(armed=armed, burst=7, deploys_into=deploys_into,
                                           house_iq=0, guard_area_iq=1),
                         steps=[dict(op='idle', args=[0, 1])]))
    return rows


def generate():
    inputs = reader_receipts()
    rows = [AttackReturn(case, inputs).run() for case in cases()]
    return dict(schema_version=1, native_sha256=NATIVE_SHA256, reader_receipts=inputs,
                row_count=len(rows), rows=rows)


def metadata():
    return provenance(scope=__doc__, entry_points={name: address for address, (name, _) in OBSERVED.items()}
                      | dict(unit_type_constructor=0x7470D0, rules_constructor=0x665650,
                             mission_static_constructor=0x4E7CF0, mission_reader=0x5B3760,
                             is_armed=0x701120, current_weapon=0x70E1A0,
                             post_foot_ai_prefix=POST_FOOT_AI, post_foot_ai_prefix_end=0x7365DF,
                             techno_stage_prefix=0x6FABB8, techno_stage_prefix_end=0x6FAC31,
                             harvester_read=0x74769F, storage_read=0x713129, movement_zone_read=0x71605E,
                             dock_read=0x713171, dock_read_end=0x713264,
                             scan_ranges_read=0x67028C, load_rate_read=0x670CE7),
                      assumptions=[
                          'Reuses harvest_field/refinery_dock: 32x32 supplied map, original Cell/Unit/Drive/Building vtables; original Drive, Facing and native vector constructors. Full Unit/House/Scenario/map initialization and world membership are excluded.',
                          'Original UnitType HARV and Rules constructors and MissionControl static construction execute in a separate reader VM. Physical RULESMD, optional LANGRULE, MPBattleMD and XMP03T4 strings enter existing original-CRC lookup caches; selected HARV/General blocks and complete MissionControl table read loop run in that order. Final scalar bytes and all six selected MissionControl entries transfer to each ore VM; original name table816CAC establishes Sleep0/Stop13. Original Dock713171..713264 resolves NAREFN/GAREFN through ReadString/strtok and real BuildingType factories; the ore VM retains the one harvest_field supplied Dock typeordinal0 with owned count1, matching the first native list item but excluding full Dock/refinery type/House lifecycle. Other type keys/weapons and whole Rules Process are excluded.',
                          'Each source has declared live health1000, native constructor-equivalent flags7, no suspended mission, no saved attack-move and legacy team slot -1. Original4DF1A0 and Foot constructor store4D31F1 establish the latter two. Houses are supplied human/current-player links. All dynamic target and command inputs are stored per row.',
                          'Sorted native ID records101->source,202->target,303->second and Houseindex0 are supplied; native token resolver/RTTI, Event4C6860, Stop4C65E0 and EventExecute4C6CB0 run. Target fields are supplied real-vtable Units with coordinates, type, owner and health/alive. A dead=true control supplies health0/alive0 then executes complete Unit7446E0 PointerExpired hierarchy; damage and whole world detach traversal are excluded.',
                          'Monotonic or explicit prior-timer controls invoke original Mission5B3060, Ready744270 and conditional Commence5B3570. This runs the original dispatch epilogue, promotion bodies and selected TechnoAI6FABB8..6FAC31 stage update and UnitAI7365BB..7365DF alive/effective-mission/harvesting-latch prefix, stopping before Fire7365E1. Stage runs after mission dispatch as in the caller6FA655 ->6FABB8; Stage timerpadding+104 comes from supplied zero stacklocal, increment+110 is inherited1. This excludes whole UnitAI and Logic scheduling. A deadline step supplies the frame from the previous native timer. The Attack cadence reads CURRENT+AC, even after Harvest is queued.',
                          'Scenario RNG seeded by original65C6D0 in the inherited fixture; full state, cursors, every raw/ranged call and actual field writes are retained at every step. FPCW inherited0E7F. No code or original vtable bytes are patched.',
                          'Six shared-owner exit controls supply currentEnter7 and linked refinery contacts, execute unchanged Unitvtable+274=65ACB0 BREAK3, then originalUnit738970 idle(0,1), as in caller4D92D0..4D92E8. Contact slots and installed NavCom before/after are recorded. Full FootEnter admission/exit, depot repair/sale and subsequent Move/Harvest gameplay are excluded.',
                          'Ten shared Unit idle controls supply Harvester/Weeder0, target, burst7, optional slot0 WeaponStruct weapon pointer, optional DeploysInto pointer, HasTurrets0, DefaultToGuardArea0 and houseIQ0/GuardAreaIQ1. Original IsArmed701120 executes and its EAX is recorded. Original Unit idle establishes armed target/burst retention and unarmed setter admission before the queue tail. Weapon bodies, broader AreaGuard/transport gameplay and native Move acquisition are excluded.',
                          'Ore/gem registry/values, cells, cargo, queues, refinery and passability retain the declared harvest_field inputs. The corpus does not establish a shot, combat damage, full target destruction, long refinery return, full match or rendered output.',
                      ], substitutions=[
                          'Live-target Attack stops before original UnitApproach7414E0 after original virtual dispatch. A supplied RET4 outside the original image returns declaredEAX0 without target mutation, then original Attack resumes. This caller comparison excludes Approach, range/admission, movement and firing.',
                          'Inherited CanReachZone56D100 answers declared reachability; field Recalc47D2B0 supplies bare LandType. Radar/tactical callbacks are recorded service sinks. Docking/nearby answers, UnitScatter and refinery animation producers remain inherited declared boundaries when reached. Unit idle and Ready execute unchanged in this extension.',
                          'Inherited OS Interlocked imports update pointed refcounts. The reader VM retains Landing allocator/delete/TLS services; physical INI file loading and cached-index creation are supplied.',
                      ])


def source_paths():
    owner = Path('tools/spatial_oracle')
    return {
                          'generator': Path(__file__),
                          **{name: owner / f'{name}.py' for name in ('harvest_field', 'refinery_dock',
                                                                   'track_destination', 'unit_source_scatter', 'unit_scatter_state')},
                          'landing_reader': Path('tools/rules_oracle/bridge_landing_inputs.py'),
                          'reader_heap': Path('tools/rules_oracle/bridge_anim_lists.py'),
                          'ini_index': Path('tools/spatial_oracle/building_body_rules.py'),
                          'physical_lexical': Path('tools/projectile_oracle/bridge_render_inputs.py'),
                          'native_words': Path('tools/spatial_oracle/map_queries.py'),
                          'signed_word': Path('tools/projectile_oracle/guided_step.py'),
                          'rng_snapshot': owner / 'shrapnel_repair/shrapnel_repair.py',
                          'unit_entry': owner / 'unit_entry.py',
                          'native_runner': Path('tools/native_oracle.py'),
                      }


def generate_area_guard():
    """Small additive corpus of the original stock-Harvester AreaGuard exit."""
    inputs = reader_receipts()
    cases = []
    for human, on_ore, seed, cargo in ((True, True, 1, 0), (True, False, 4, 12),
                                      (False, True, 5, 40), (False, False, 8, 0)):
        cases.append(dict(name=f"guard_{'human' if human else 'computer'}_{'ore' if on_ore else 'clear'}",
                          human=human, seed=seed, storage=[cargo, 0, 0, 0],
                          ore=[[15, 15, 0, 0, 5]] if on_ore else [],
                          steps=[dict(op='command', mission='area_guard', post=[15, 15]),
                                 dict(op='promote'), dict(op='dispatch')]))
    cases.append(dict(name='guard_commence_bypasses_refused_readiness', current=11,
                      dispatch_timer=[200, 0, 0], status=2, seed=4, unloading=True,
                      ore=[], storage=[0, 0, 0, 0],
                      target=True, nav=[17, 15], archive=[20, 12],
                      steps=[dict(op='ready'), dict(op='dispatch')]))
    cases.append(dict(name='guard_waits_for_dispatch_deadline', current=11,
                      dispatch_timer=[200, 0, 9], frame=208, seed=8,
                      ore=[], storage=[0, 0, 0, 0],
                      steps=[dict(op='dispatch'), dict(op='deadline'), dict(op='dispatch')]))
    rows = [AttackReturn(dict(case, area_guard_comparison=True), inputs).run() for case in cases]
    return dict(schema_version=1, native_sha256=NATIVE_SHA256, reader_receipts=inputs,
                row_count=len(rows), rows=rows)


def area_guard_metadata():
    return provenance(
        scope='Stock War Miner AreaGuard event and due mission handoff to Harvest, through original '
              'Event, Unit/Foot handlers, Queue/Commence and dispatch epilogue. Not whole-world or movement parity.',
        entry_points={name: address for address, (name, _) in OBSERVED.items()} | {
            'unit_area_guard': 0x744100, 'foot_area_guard': 0x4D6AA0,
            'harvester_gate': 0x4D6D1A, 'harvest_queue': 0x4D6D3B,
            'harvest_commence': 0x4D6D45, 'harvest_cadence': 0x4D6D5B,
            'controlled_by_human': 0x50B730, 'random_seed': 0x65C6D0,
            'unit_type_constructor': 0x7470D0, 'harvester_read': 0x74769F,
        },
        assumptions=[
            'The existing AttackReturn/harvest_field/refinery_dock VM is reused unchanged. It supplies a '
            '32x32 map, live War Miner receiver, original Unit/Drive/Cell/Building vtables and original '
            'Drive/Facing/vector constructors. Full Unit/House/Scenario/map construction is excluded.',
            'Existing reader_receipts executes original HARV UnitType/Rules/MissionControl constructors '
            'and selected original key readers over physical RULESMD, optional LANGRULE, MPBattleMD '
            'and XMP03T4 lexical strings, in that order. Harvester default0 becomes1. Physical file '
            'loading and index construction are supplied; no Rust-derived type values are used.',
            'Four histories execute Event4C6860/4C6CB0 with mission11 and the current Cell Target, '
            'the body-established G730D60 payload. Original UnitReady/Commence promotes AreaGuard; '
            'Mission5B3060 dispatches through unchanged Unit744100 to Foot4D6AA0. Keyboard selection, '
            'G input voice/OutList delivery and whole Logic scheduling are outside this corpus.',
            'Both skirmish human and computer houses and ore/clear land are supplied. No SlaveManager, '
            'bunker, building-entry flag, active navigation list or archived allied-building collision '
            'is supplied, so those earlier independent arms are not certified. Original heads execute.',
            'One already-current AreaGuard control supplies Unit unloading+6D1, target, destination '
            'and archive; original Ready744270 refuses, but original Foot calls Commence directly. '
            'The unloading producer and that cross-state history are excluded. Another control visits '
            'before and exactly at the original retained dispatch deadline.',
            'Scenario seed is the row seed; Main31 and MapGen42 are seeded by original65C6D0 before '
            'observation. All three full logical states are retained. Native RandomRanged1..10 '
            'rejection and raw draw order execute; no cadence/queue/commence result is supplied. '
            'Original text and Unit vtable bytes are checked unchanged after each history.',
            'The branch leaves target, navigation, archive, cargo and harvesting/unloading latches '
            'intact, resets the mission cursor through Commence and writes its returned delay through '
            'the shared dispatcher. No subsequent Harvest dispatch, movement, ore collection, '
            'refinery return or rendered output is claimed by these six controls.',
        ],
        substitutions=[
            'Inherited OS Interlocked services and declared native-reader allocator/delete/TLS '
            'services remain. Physical INI bytes are placed in existing CRC lookup caches.',
            'Inherited map reachability, recalculation, radar, sound, dock and animation service '
            'boundaries remain as recorded by the existing fixture. The selected AreaGuard handoff '
            'does not take live-target Approach or its legacy external return seam. Reached callbacks '
            'are retained, including any detach; none is inferred from an unreached body.',
        ],
    )


if __name__ == '__main__':
    args = sys.argv[1:]
    if '--area-guard' in args:
        args.remove('--area-guard')
        finish_vectors(generate_area_guard, Path(__file__).with_name('harvest_area_guard.json'),
                       provenance=area_guard_metadata, source_paths=source_paths(), argv=args)
    else:
        finish_vectors(generate, Path('tools/spatial_oracle/harvest_attack_return.json'),
                       provenance=metadata, source_paths=source_paths(), argv=args)
