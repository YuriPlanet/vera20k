"""Original player Area Guard on the existing constructed Foot/event fixture.

GIMoveHistory remains the owner of native invocation, event rings and local
dispatch. FootMissions remains the constructor/map/mission fixture. This module
adds selected original Guard callers and observations, not another native VM.
"""
from pathlib import Path
from collections import deque
import copy
import hashlib
import json
import os
import struct
import sys

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI,
    UC_X86_REG_ESP,
)
from tools.native_oracle import (
    RET_MAGIC, SCRATCH, finish_vectors, image_sha256, provenance, run_checked,
)
from tools.storage_oracle.keyboard_bindings import KeyboardFixture
from tools.rules_oracle.bridge_anim_inputs import Reader
from tools.spatial_oracle.anytown_damage.foot_missions import FootMissions
from tools.spatial_oracle.anytown_damage.mtnk_attack import sound_inputs
from tools.spatial_oracle.building_body_rules import INI, SP, dwords
from tools.spatial_oracle.walk_first_path import GIMoveHistory
from tools.spatial_oracle._factory_infantry_output.runtime import require
# attach_world invokes this owner lazily; import it before the source guard.
from tools.spatial_oracle import fire_error as _fire_error
from tools.spatial_oracle.engineer_repair_admission import EngineerJoinedFixture
from tools.projectile_oracle.bridge_render_inputs import lexical

HERE = Path(__file__).resolve().parent
SOUNDS = {'CommandBar', 'GIMove'}


def i32(u, pointer):
    return struct.unpack('<i', u.mem_read(pointer, 4))[0]


def sound_name(m, index):
    if index < 0 or index >= m.read32(0xB1D388):
        return None
    return m.string(m.read32(m.read32(m.read32(0xB1D37C) + 4 * index)) + 0x6C)


def voice_binding(m, typ):
    count, data = m.read32(typ + 0x4B0), m.read32(typ + 0x4A4)
    assert count <= 128
    ids = [i32(m.u, data + 4 * index) for index in range(count)]
    return dict(count=count, ids=ids, names=[sound_name(m, index) for index in ids])


def read_voice(m, typ, sections):
    """Original balanced VoiceSpecialAttack ReadSoundList/copy/destructor block."""
    m.make_ini(sections)
    for register, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBP, typ),
                            (UC_X86_REG_EBX, typ + 0x24), (UC_X86_REG_ESI, INI)):
        m.u.reg_write(register, value)
    run_checked(m.u, 0x712CC5, 0x712D2F,
                required_addresses=[0x525430, 0x478720, 0x477D20])
    assert m.u.reg_read(UC_X86_REG_ESP) == SP
    return voice_binding(m, typ)


def read_guard(m, rules, sections):
    # The adjacent single-reference readers share pushes. Running the complete
    # AudioVisual body avoids inventing an interior stack/prefix contract.
    m.make_ini(sections)
    admitted = m.invoke(0x6691E0, rules, (INI,)) & 255
    index = i32(m.u, rules + 0x724)
    return dict(admitted=admitted, id=index, name=sound_name(m, index))


def constructor_rows():
    """Execute the common Event constructor in the existing keyboard VM."""
    q = KeyboardFixture()
    u = q.uc
    u.mem_map(RET_MAGIC, 0x1000)
    pointer = SCRATCH + 0xF000
    frame = 0x12345678
    q.put(0xA8ED84, frame)
    before = bytes((index * 17 + 0xA5) & 255 for index in range(111))
    rows = []
    controls=[(name,[house,0x10203040,0x34,mission,0x55667788,11,0,0,0x10203040,0],
               'Raw constructor fields; Cell value outside ordinary signed-cell codec')
              for name,house,mission in [('move',3,2),('guard',3,11),
                ('move_null_house',0xFFFFFFFF,2),('guard_null_house',0xFFFFFFFF,11)]]
    controls += [(name,args,'Ordinary observed MEGAMISSION token layout; declared numeric actor/cell inputs')
                 for name,args in [
                  ('ordinary_move',[0,173,0x34,2,0,0,50087,11,173,0]),
                  ('ordinary_guard_cell',[0,173,0x34,11,50087,11,0,0,173,0]),
                  ('ordinary_guard_object',[0,173,0x34,11,194,0x34,0,0,173,0]),
                  ('ordinary_guard_null',[0,173,0x34,11,0,0,0,0,173,0])]]
    for name,args,boundary in controls:
        u.mem_write(pointer, before)
        u.mem_write(q.stack, dwords(RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ESP, q.stack)
        u.reg_write(UC_X86_REG_ECX, pointer)
        run_checked(u, 0x4C6860, RET_MAGIC, required_addresses=[0x4C6860])
        assert u.reg_read(UC_X86_REG_ESP) == q.stack + 44
        after = bytes(u.mem_read(pointer, 111))
        rows.append(dict(name=name, boundary=boundary, frame=frame, args=args, before=before.hex(),
                         after=after.hex(), returned_receiver=u.reg_read(UC_X86_REG_EAX) == pointer,
                         changed_offsets=[index for index, (a, b) in
                                          enumerate(zip(before, after)) if a != b]))
    return rows


def reader_rows():
    root = Path(os.environ['VERA20K_SHRAPNEL_INPUTS'])
    m = Reader(root, {})
    registry = sound_inputs(m, root, {}, wanted_names=SOUNDS)
    rules = m.alloc(0x3000)
    m.invoke(0x665650, rules)
    types = {}
    for name in ('MTNK', 'E1'):
        typ = m.alloc(0x1900)
        m.invoke(0x710AF0, typ, (m.cstring(name),))
        types[name] = typ
    constructor = dict(guard=i32(m.u, rules + 0x724),
                       voices={name: voice_binding(m, typ) for name, typ in types.items()})
    layers = []
    map_path = Path(os.environ.get('VERA20K_AREA_GUARD_MAP',
                                  str(Path(os.environ['VERA20K_PROJECTILE_RENDER_ASSETS']) / 'Hills.map')))
    for name, path in [(name, root / name) for name in
                       ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini')] + [('Hills.map', map_path)]:
        if not path.exists():
            assert name == 'LANGRULE.INI', str(path)
            layers.append(dict(file=name, absent=True))
            continue
        raw = path.read_bytes()
        sections, lines = lexical(raw, {'AudioVisual', 'MTNK', 'E1'})
        selected = {name: {key: value for key, value in values.items()
                          if key == ('GuardSound' if name == 'AudioVisual' else 'VoiceSpecialAttack')}
                    for name, values in sections.items()}
        guard = read_guard(m, rules, {'AudioVisual': selected['AudioVisual']}
                           if 'AudioVisual' in selected else {})
        voices = {name: read_voice(m, typ, {name: selected[name]} if name in selected else {})
                  for name, typ in types.items()}
        layers.append(dict(file=name, sha256=hashlib.sha256(raw).hexdigest(), bytes=len(raw),
                           sections=selected, source_lines=lines, guard=guard, voices=voices))
    controls = []
    for label, value in (
        ('valid_list', 'GIMove,CommandBar,GIMove'), ('missing', None),
        ('empty', ''), ('whitespace', '   '), ('unknown', 'UnknownGuardVoice'),
        ('duplicates_case', 'gImOvE,GIMove,commandbar'), ('internal_space', 'GIMove, CommandBar'),
        ('none_literals', 'none,<none>,GIMove'), ('commas', ',,,'),
        ('capacity', 'GIMove,' * 30),
    ):
        before = voice_binding(m, types['E1'])
        values = {} if value is None else {'VoiceSpecialAttack': value}
        after = read_voice(m, types['E1'], {'E1': values})
        controls.append(dict(name=label,key='VoiceSpecialAttack',value=value,before=before,after=after))
    before=voice_binding(m,types['E1'])
    controls.append(dict(name='wrong_case_key',key='voicespecialattack',value='CommandBar',before=before,
                         after=read_voice(m,types['E1'],{'E1':{'voicespecialattack':'CommandBar'}})))
    references = []
    for label, value in (('valid', 'CommandBar'), ('missing', None), ('empty', ''),
                         ('whitespace', '  '), ('unknown', 'UnknownGuardSound'),
                         ('case', 'commandbar'), ('none', '<none>')):
        before = i32(m.u, rules + 0x724)
        values = {} if value is None else {'GuardSound': value}
        references.append(dict(name=label,key='GuardSound',value=value,before=before,
                               after=read_guard(m, rules, {'AudioVisual': values})))
    references.append(dict(name='wrong_case_key',key='guardsound',value='GIMove',before=i32(m.u,rules+0x724),
                           after=read_guard(m,rules,{'AudioVisual':{'guardsound':'GIMove'}})))
    return dict(constructor=constructor, sound_registry=registry, layers=layers,
                voice_controls=controls, guard_controls=references)


class FootObservation:
    """Read-only shape adaptation for the existing Move history transport."""

    def __init__(self, owner):
        self.owner = owner
        self.u, self.read32 = owner.u, owner.m.read32
        self.allocate = owner.m.alloc
        self.trail, self.trace = deque(maxlen=80), []
        self.platform_audio, self.WATCH = None, {}

    @property
    def phase(self):
        return self.owner.phase

    @phase.setter
    def phase(self, value):
        self.owner.phase = value

    @property
    def draws(self):
        return [row for row in self.owner.events if row.get('kind') == 'rng']

    @property
    def advances(self):
        return [row for row in self.owner.events if row.get('kind') == 'raw']

    def rng(self):
        return {name: bytes(self.owner.u.mem_read(pointer, 0x3F4)).hex()
                for name, pointer in self.owner.resident.rngs.items()}


class GuardHistory(GIMoveHistory):
    """Keep native event transport; replace only fixture-specific observations."""

    WATCH = dict(GIMoveHistory.WATCH, **{})
    WATCH.update(GIMoveHistory.UNIT_WATCH)
    WATCH.update({
        0x536D00: ('GuardCommand', 4), 0x730D60: ('GuardSelection', 0),
        0x700C40: ('IsControllable', 0), 0x7010D0: ('IsActive', 0),
        0x5F6A10: ('CurrentCell', 0), 0x70C610: ('Archive_SetTarget', 4),
        0x4DBDF0: ('FootNavigationCoord', 8), 0x5657A0: ('MapCell', 4),
        0x708D90: ('QueueVoice', 4), 0x750920: ('GuardSound', 8),
        0x5B3570: ('Commence', 0), 0x5B3060: ('MissionDispatch', 0),
        0x4D6AA0: ('FootAreaGuard', 0), 0x744100: ('UnitAreaGuard', 0),
        0x51F640: ('InfantryAreaGuard', 0),
    })

    def __init__(self, owner, name):
        from tools.spatial_oracle import building_construction as bc
        self.owner, self.u, self.r = owner, owner.u, owner.m.read32
        self.f, self.bc = FootObservation(owner), bc
        self.require, self.copy, self.hashlib = require, copy, hashlib
        self.name, self.unit = name, True
        self.actor, self.loco = owner.src, self.r(owner.src + 0x674) - 4
        self.actors = {'MTNK': owner.src, 'E1': owner.e1,
                       'allied_MTNK': owner.victim, 'enemy_MTNK': owner.candidate}
        self.actor_bytes, self.loco_bytes = 0x1000, 0x70
        self.sequence, self.phase = 0, 'created'
        self.events, self.writes, self.pending, self.boundaries = [], [], [], []
        self.inputs, self.streams, self.recent = [], {}, deque(maxlen=80)
        self.draw_start, self.advance_start = len(self.f.draws), len(self.f.advances)
        self.cell = lambda x, y: owner.resident.ptrs[x, y]
        self.hooks = [self.u.hook_add(UC_HOOK_CODE, self.observe),
                      self.u.hook_add(UC_HOOK_MEM_WRITE, self.written)]

    def snapshot(self):
        states = {}
        for name, pointer in self.actors.items():
            row = self.owner.snap(pointer)
            row.update(id=self.r(pointer + 0x10), flags=self.r(pointer + 0x14),
                       alive=self.u.mem_read(pointer + 0x90, 1)[0],
                       limbo=self.u.mem_read(pointer + 0x81, 1)[0],
                       suspended_mission=i32(self.u, pointer + 0xB0),
                       mission_flag_b8=self.u.mem_read(pointer+0xB8,1)[0],
                       suspended_target=self.r(pointer + 0x2B8),
                       suspended_nav=self.r(pointer + 0x5A8),
                       queued_voice=i32(self.u, pointer + 0x4F0),
                       type_name='E1' if pointer == self.owner.e1 else 'MTNK',
                       owner=self.r(pointer + 0x21C),
                       human=self.u.mem_read(self.r(pointer + 0x21C) + 0x1EC, 1)[0],
                       player_control=self.u.mem_read(self.r(pointer + 0x21C) + 0x1ED, 1)[0],
                       offline=self.u.mem_read(pointer + 0x1C8, 1)[0],
                       navigation=dict(aux=self.r(pointer+0x5A0),
                         destination=self.ints(self.r(pointer+0x674)+(0x18 if pointer==self.owner.e1 else 0x30),3),
                         path=self.ints(pointer+0x5E0,24),
                         reference_cell=list(struct.unpack('<2h',self.u.mem_read(pointer+0x558,4))),
                         movement_timer=self.ints(pointer+0x640,3),blocked_timer=self.ints(pointer+0x668,3),
                         retry=i32(self.u,pointer+0x64C),blocked=self.u.mem_read(pointer+0x6B7,1)[0],
                         queue_count=self.r(pointer+0x598)),
                       sequence_timer=self.ints(pointer+0x100,4),
                       dispatch_timer=self.ints(pointer+0xC8,3),
                       voice=voice_binding(self.owner.m, self.owner.e1_type if pointer == self.owner.e1 else self.owner.typ))
            states[name] = row
        return dict(frame=self.r(self.bc.FRAME), actors=states, rng=self.rng(),
                    rings=self.rings(),game_mode=self.r(0xA8B238),current_house=self.r(0xA83D4C),
                    planning=self.u.mem_read(0xAC4CF4, 1)[0],
                    voice_enabled=self.u.mem_read(0x822CF2, 1)[0])

    def written(self, u, _access, address, size, value, _data):
        labels = [name for name, pointer in self.actors.items()
                  if address < pointer + 0x1000 and pointer < address + size]
        if labels:
            self.writes.append(dict(sequence=self.next_sequence(), pc=u.reg_read(UC_X86_REG_EIP),
                                    frame=self.r(self.bc.FRAME), phase=self.phase,
                                    address=address, size=size, value=value,
                                    before=bytes(u.mem_read(address, size)).hex(), owners=labels))

    def observe(self, u, pc, size, data):
        if pc in (0x53EC9A, 0x646F20):
            # One existing owner supplies the OS import and exact stack effect.
            # G has no key query; Ctrl+Alt comparisons use live_cell_input.
            EngineerJoinedFixture.hook(self.f, u, pc, size, data)
            return
        super().observe(u, pc, size, data)
        if pc == 0x750920:
            value=u.reg_read(UC_X86_REG_ECX)
            index=value if value < 0x80000000 else value - 0x100000000
            self.events[-1].update(flags=u.reg_read(UC_X86_REG_EDX),
                                   sound_id=index, sound_name=sound_name(self.owner.m, index))
        elif pc == 0x708D90:
            index=i32(u, u.reg_read(UC_X86_REG_ESP) + 4)
            self.events[-1].update(sound_id=index, sound_name=sound_name(self.owner.m, index))

    def finish(self):
        for hook in self.hooks:
            self.u.hook_del(hook)
        voices=[]
        for call in self.events:
            if call['kind']!='Techno_ClickedMission' or call['args'][0]!=11:continue
            actor=next(name for name,p in self.actors.items() if p==call['this'])
            before,after=call['before'],call['after']
            nested=[row for row in self.events if call['sequence']<row['sequence']<call['return_sequence']]
            old=before['actors'][actor]['queued_voice'];new=after['actors'][actor]['queued_voice']
            voices.append(dict(sequence=call['sequence'],phase=call['phase'],actor=actor,
                voice_enabled=before['voice_enabled'],game_mode=before['game_mode'],current_house=before['current_house'],
                owner=before['actors'][actor]['owner'],human=before['actors'][actor]['human'],
                player_control=before['actors'][actor]['player_control'],vector=before['actors'][actor]['voice'],
                queued_before=dict(id=old,name=sound_name(self.owner.m,old)),
                queued_after=dict(id=new,name=sound_name(self.owner.m,new)),
                main_raw=[row['returned_eax'] for row in nested if row['kind']=='RNG_Random' and row['this']==0x886B88],
                queue_calls=[dict(id=row['sound_id'],name=row['sound_name']) for row in nested if row['kind']=='QueueVoice'],
                rng_before=before['rng'],rng_after=after['rng']))
        # Entry/return snapshots contain the same whole-world state many
        # times. Retain the actual receiver and RNG at every call; complete
        # actor/ring states remain at each externally invoked boundary.
        for call in self.events:
            receiver=next((name for name,p in self.actors.items() if p==call.get('this')),None)
            for field in ('before','after','at_ret8'):
                if field not in call or not isinstance(call[field],dict):continue
                state=call[field]
                if 'actors' not in state:continue
                call[field]={key:value for key,value in state.items() if key not in ('actors','rings')}
                call[field]['actors']={receiver:state['actors'][receiver]} if receiver is not None else {}
        return dict(voice_receipts=voices,name=self.name, boundaries=self.boundaries, ordered_calls=self.events,
                    ordered_writes=self.writes, final=self.snapshot(),
                    complete_rng_states=self.streams, pending_calls=self.pending)


def histories():
    q = FootMissions()
    root = Path(os.environ['VERA20K_SHRAPNEL_INPUTS'])
    # Keep the inherited impact names and add the two actual Guard consumers
    # before the complete Unit/Infantry type readers bind their voice vectors.
    art,_ = lexical((root/'ARTMD.INI').read_bytes(), {'MTNK','GTNK','E1','GI','GISequence',
                       'Cannon','120MM','GUNFIRE','S_CLSN16','S_CLSN22','H2O_EXP1','H2O_EXP2','H2O_EXP3'})
    registry = sound_inputs(q.m, root, art, wanted_names=SOUNDS | {
        'GrizzlyTankAttack','Explosion14','ExplosionWaterLarge','ExplosionWaterMed','ExplosionWaterSmall'})
    q.initialize_companion()
    handler_readers=q.rules_reader_receipts()
    weapon_readers={'E1':q.weapon_reader_receipts()}
    # The legacy observer names its receiver fields after E1, but invokes
    # shared Techno getters and type readers. Reuse that same owner for MTNK;
    # only Python receiver aliases change, never native actor/type identity.
    # This also rebinds the inherited 105mm Report after SoundList expansion.
    companion=q.e1,q.e1_type,q.e1_weapon_read_attempts
    try:
        q.e1,q.e1_type,q.e1_weapon_read_attempts=q.src,q.typ,[]
        observed=q.weapon_reader_receipts()
        weapon_readers['MTNK']={key:observed[key] for key in
            ('before','after','layers','selected_sections','rules_context','idle_action_frequency_bits')}
        weapon_readers['MTNK']['receiver_boundary']='Existing shared reader/getter owner, with fixture receiver aliases scoped to actual MTNK; no E1 historical-read claim'
    finally:
        q.e1,q.e1_type,q.e1_weapon_read_attempts=companion
    assert q.m.read32(0x8871E0)==q.rules
    runtime_layers=[]
    from tools.spatial_oracle.anytown_damage.mtnk_attack import layers
    for name,path in layers():
        if not path.exists():
            runtime_layers.append(dict(file=name,absent=True));continue
        raw=path.read_bytes();sections,lines=lexical(raw,{'AudioVisual'})
        value=sections.get('AudioVisual',{}).get('GuardSound')
        selected={'AudioVisual':{'GuardSound':value}} if value is not None else {}
        runtime_layers.append(dict(file=name,sha256=hashlib.sha256(raw).hexdigest(),sections=selected,
                                   guard=read_guard(q.m,q.rules,selected)))
    q.m.invoke(0x4E7E20,0)
    selected=q.m.alloc(64)
    q.u.mem_write(0xA8ECBC,dwords(selected,16,1,0))
    q.u.mem_write(0xAC4CF4,b'\0')
    q.u.mem_write(0x822CF2,b'\1')
    q.u.mem_write(0x87E2A0,dwords(0))
    # Independently observe the physical input cells before any Guard case.
    # In particular, the other post's height is not inferred from the later
    # destination-setter result. Original virtual+48 writes its actual XYZ.
    post_cells=[]
    for xy in ((87,50),(87,49),(87,52)):
        pointer=q.resident.ptrs[xy]
        getter=q.m.read32(q.m.read32(pointer)+0x48)
        before=q.cell_readback(pointer)
        before_rng=FootObservation(q).rng()
        coordinate=bytes(q.u.mem_read(q.coord,12))
        cpu=q.u.context_save()
        try:
            returned=q.m.invoke(getter,pointer,(q.coord,))
            assert returned==q.coord
            raw=bytes(q.u.mem_read(returned,12))
            after_rng=FootObservation(q).rng()
            assert before_rng==after_rng
            after=q.cell_readback(pointer)
            assert before==after
            post_cells.append(dict(before=before,after=after,getter=hex(getter),
                returned_xyz=list(struct.unpack('<3i',raw)),returned_bytes=raw.hex(),
                rng_before=before_rng,rng_after=after_rng))
        finally:
            q.u.mem_write(q.coord,coordinate)
            q.u.context_restore(cpu)
    original_code=bytes(q.u.mem_read(0x401000,0x3E0000))
    # This is experiment rollback only, not a game-state or save implementation.
    # It restores every mapped byte and CPU state between independent inputs.
    saved=[(a,bytes(q.u.mem_read(a,b-a+1))) for a,b,_ in q.u.mem_regions()]
    cpu=q.u.context_save();cursor=q.m.cursor
    rows=[]
    cases=[dict(name='stock_mtnk_current_cell',selection=['MTNK'],dispatch=True,handler=True,acquire_in_range=True),
           dict(name='stock_e1_current_cell',selection=['E1'],dispatch=True,handler=True,acquire_in_range=True),
           dict(name='stock_ordered_selection',selection=['E1','MTNK','allied_MTNK'],dispatch=True),
           dict(name='empty_selection',selection=[]),
           dict(name='all_skipped_null_cell_foreign',selection=[None,'Cell','enemy_MTNK']),
           dict(name='mixed_eligible_selection',selection=[None,'enemy_MTNK','E1','Cell','MTNK']),
           dict(name='offline_selection',selection=['E1','MTNK'],offline=True),
           dict(name='voices_disabled',selection=['E1','MTNK'],voice_enabled=False),
           dict(name='ordered_authored_voice_lists',selection=['MTNK','E1','allied_MTNK'],
                voice_lists={'MTNK':'GIMove,CommandBar,GIMove','E1':'CommandBar,GIMove'}),
           dict(name='repeated_voice_pending_overwrite',selection=['E1'],repeat=4,
                voice_lists={'E1':'GIMove,CommandBar,GIMove'}),
           dict(name='direct_noncontrolled_default_voice',selection=['E1'],direct='current_cell',human=False,player_control=False,game_mode=0),
           dict(name='guard_other_cell',selection=['MTNK'],direct='other_cell',dispatch=True),
           dict(name='guard_allied_object',selection=['MTNK'],direct='allied_MTNK',dispatch=True),
           dict(name='guard_enemy_object',selection=['MTNK'],direct='enemy_MTNK',dispatch=True),
           dict(name='guard_null',selection=['MTNK'],direct='null',dispatch=True),
           dict(name='guard_suspended_cleanup',selection=['MTNK'],suspended=True,dispatch=True),
           dict(name='ordinary_move_event',selection=['MTNK'],direct='other_cell',mission=2,dispatch=True),
           dict(name='outlist_full_still_acknowledges',selection=['E1','MTNK'],outlist_full=True),
           dict(name='guard_then_move_clears_post',selection=['MTNK'],dispatch=True,retask_move=True)]
    for case in cases:
        for a,raw in saved:q.u.mem_write(a,raw)
        q.u.context_restore(cpu);q.m.cursor=cursor
        q.events.clear();q.pending.clear();q.resident.trace.clear();q.resident.pending.clear()
        authored_readers=[]
        if case.get('voice_lists'):
            # Original405170 exposes a Voc name only with backend present.
            # These controls model a rules read before runtime device disable.
            q.u.mem_write(0x87E2A0,dwords(1))
            for name,value in case['voice_lists'].items():
                authored_readers.append(dict(type=name,value=value,
                    result=read_voice(q.m,q.e1_type if name=='E1' else q.typ,{name:{'VoiceSpecialAttack':value}})))
            q.u.mem_write(0x87E2A0,dwords(0))
        h=GuardHistory(q,case['name'])
        pointers={**h.actors,None:0,'Cell':q.resident.ptrs[87,52]}
        actors=[pointers[name] for name in case['selection']]
        q.u.mem_write(selected,dwords(*actors))
        q.u.mem_write(0xA8ECC8,dwords(len(actors)))
        fields=[]
        if case.get('offline'):fields += [(p+0x1C8,1,1) for p in actors]
        if not case.get('voice_enabled',True):fields.append((0x822CF2,1,0))
        if not case.get('human',True):fields.append((q.house+0x1EC,1,0))
        if not case.get('player_control',True):fields.append((q.house+0x1ED,1,0))
        if 'game_mode' in case:fields.append((0xA8B238,4,case['game_mode']))
        if case.get('outlist_full'):
            fields.extend([(0xA802C8,4,128),(0xA802CC,4,0),(0xA802D0,4,0)])
        if case.get('suspended'):
            fields.extend([(q.src+0xB0,4,1),(q.src+0x5A8,4,q.victim),(q.src+0x2B8,4,q.candidate)])
        if fields:h.supply_fields(fields,bounds='Explicit retained scalar/reference priors, not their lifecycle producers')
        try:
            if case.get('direct'):
                post={'null':0,'current_cell':q.resident.ptrs[87,49],
                      'other_cell':q.resident.ptrs[87,52],**h.actors}[case['direct']]
                mission=case.get('mission',11)
                args=(mission,0,post,0) if mission==2 else (mission,post,0,0)
                h.invoke('original_direct_Player_Send_Command',0x6FFBE0,actors[0],*args)
            else:
                for repeat in range(case.get('repeat',1)):
                    h.invoke('original_Guard_execute'+('' if repeat==0 else '_'+str(repeat+1)),0x536D00,0,0)
            if case.get('dispatch'):
                # Existing Mission setup likewise supplies Event delivery.
                # Keep original issuer bytes and execute each actual OutList
                # record, without claiming the omitted network/House pump.
                for event in h.rings()['outlist']['events']:
                    pointer=0xA802D4+event['slot']*0x6F
                    h.invoke('delivered_original_Event_'+str(event['slot']),0x4C6CB0,pointer)
            if case.get('handler'):
                actor=actors[0]
                h.invoke('original_Commence',0x5B3570,actor)
                h.invoke('next_ordinary_Mission_dispatch',0x5B3060,actor)
                if case.get('acquire_in_range'):
                    h.frame(50)
                    h.invoke('later_expired_Mission_dispatch_in_range',0x5B3060,actor)
                    h.frame(100)
                    h.invoke('next_retained_target_Mission_dispatch_in_range',0x5B3060,actor)
            if case.get('retask_move'):
                h.invoke('original_Move_retask',0x6FFBE0,actors[0],2,0,q.resident.ptrs[87,52],0)
                event=h.rings()['outlist']['events'][-1]
                h.invoke('delivered_original_Move_Event',0x4C6CB0,0xA802D4+event['slot']*0x6F)
            result=h.finish();result['input']=case;result['authored_reader_controls']=authored_readers
            result['receipts']=[dict(sequence=call['sequence'],kind=call['kind'],
                actor=next((name for name,p in h.actors.items() if p==call.get('this')),None),
                sound_id=call.get('sound_id'),sound_name=call.get('sound_name'),
                returned_eax=call.get('returned_eax'),
                current_cell=q.cell_readback(call['returned_eax']) if call['kind']=='CurrentCell' else None,
                event_bytes=call.get('constructed_event_bytes'))
                for call in result['ordered_calls'] if call['kind'] in
                ('CurrentCell','QueueVoice','GuardSound','Event_Construct')]
            rows.append(result)
        except Exception:
            print('guard_case',case,'recent',[hex(pc) for pc in h.recent],file=sys.stderr)
            for hook in h.hooks:q.u.hook_del(hook)
            raise
        assert bytes(q.u.mem_read(0x401000,0x3E0000))==original_code
    return dict(inputs=q.inputs,world=q.world,sound_registry=registry,guard_layers=runtime_layers,post_cells=post_cells,
        actor_pointers={name:hex(pointer) for name,pointer in
            (('MTNK',q.src),('E1',q.e1),('allied_MTNK',q.victim),('enemy_MTNK',q.candidate))},
        handler_rules=handler_readers['handler_rules_after_physical'],
        weapon_reader_projection={name:reader['after'] for name,reader in weapon_readers.items()},
        reader_receipts=dict(handler=handler_readers,weapons=weapon_readers),rows=rows)


def generate():
    result=dict(native_sha256=image_sha256(),constructor_rows=constructor_rows(),
                retail=reader_rows(),histories=histories())
    global coverage
    coverage=dict(constructors=len(result['constructor_rows']),histories=len(result['histories']['rows']),
        list_histories=len(result['retail']['voice_controls']),reference_histories=len(result['retail']['guard_controls']),
        voice_receipts=sum(len(row['voice_receipts']) for row in result['histories']['rows']),
        main_voice_draws=sum(len(voice['main_raw']) for row in result['histories']['rows'] for voice in row['voice_receipts']))
    return result


def source_paths():
    root = HERE.parents[1]
    return {str(path.relative_to(root)): path for module in tuple(sys.modules.values())
            if (name := getattr(module, '__file__', None))
            and (path := Path(name).resolve()).is_relative_to(root / 'tools')
            and path.suffix == '.py'}


def metadata():
    return provenance(scope=__doc__, entry_points={
        'guard_command': 0x536D00, 'guard_selection': 0x730D60,
        'current_cell': 0x5F6A10, 'player_command': 0x6FFBE0,
        'event_issuer': 0x646E90, 'event_ctor': 0x4C6860,
        'event_execute': 0x4C6CB0, 'area_guard': 0x4D6AA0,
        'rules_ctor': 0x665650, 'audio_visual_reader': 0x6691E0,
        'techno_type_ctor': 0x710AF0, 'special_voice_reader': 0x712CC5,
        'sound_list': 0x525430, 'sound_registry': 0x7510D0,
    }, assumptions=[
        'Existing FootMissions constructs and admits actual MTNK/E1 on its retained physical Anytown crop. Prior House and world bootstrap bounds remain explicit in that owner.',
        'Existing GIMoveHistory owns original entry invocation, OutList observation and exact Event byte capture. Each original issued record is supplied to original4C6CB0, like the existing Mission setup; full local/network/House event scheduling is outside this corpus. No alternate event constructor or native gameplay answer is supplied.',
        'Constructor rows supply nonzero Event storage and arguments. Only original4C6860 writes the recorded after bytes.',
        'Authored runtime VoiceSpecialAttack controls execute the same original block with the backend present for original405170 name resolution, then disable the backend before input. Native sound playback remains disabled, not mocked.',
    ], substitutions=[
        'Selected local input list, absolute frame, no Planning Mode and disabled audio backend are supplied boundaries. This is not a HWND/game-wide input or whole-match native capture.',
        'Reader source-order/CRC caches and successful allocation are the existing Reader/Sound transport. Original scalar/reference/list parsing and native field writes execute.',
        'Locomotor Process and held movement/height/Fly behavior are excluded. Required class destination calls execute original instructions.',
    ])


if __name__ == '__main__':
    guarded_sources=source_paths()
    def checked_metadata():
        assert set(source_paths()) <= set(guarded_sources), sorted(set(source_paths())-set(guarded_sources))
        result=metadata();result['command']='python -m tools.input_oracle.area_guard --check'
        result['coverage_counts']=coverage
        return result
    finish_vectors(generate, HERE / 'area_guard.json', provenance=checked_metadata,
                   source_paths=guarded_sources)
