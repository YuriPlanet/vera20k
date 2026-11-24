"""Original idle SQD counter/MoveSound tail and handle/load lifecycle controls.

Invoked by ``move_sound --foot-tail``. This composes existing native fixture
owners; Ship Process and accepted device playback remain declared boundaries.
No Ship movement, whole FootAI, whole save/load or audible-output claim is made.
"""
from pathlib import Path
import hashlib
import json
import os
import struct
import sys

from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import NativeCallTrace, provenance, run_checked
from tools.projectile_oracle.bridge_render_inputs import assets_root, lexical
from tools.rules_oracle.bridge_child_sound import Sound, playing_handle
from tools.spatial_oracle.anytown_damage.mtnk_attack import sound_inputs
from tools.spatial_oracle.naval_occupants import Native, ASSETS, INI, SP, dwords
from tools.spatial_oracle.naval_lifetime_controls import raw_load_sound_reset
from tools.spatial_oracle.unit_simple_deploy import run_foot_counter_slice

HERE = Path(__file__).resolve().parent
SOUND_NAMES = {'SquidMove', 'GenLargeWaterDie'}
SOUND_FIELDS = {
    'control': 0x10, 'type_flags': 0x14, 'volume_fixed16_raw': 0x1C,
    'priority': 0x40, 'limit': 0x48, 'loop_count': 0x4C, 'range': 0x50,
    'delay_min': 0x58, 'delay_max': 0x5C, 'fshift_min': 0x60,
    'fshift_max': 0x64, 'vshift': 0x68, 'sample_count': 0x134,
}


def signed(value):
    return struct.unpack('<i', dwords(value))[0]


class FootSound(Native):
    """Extend the existing constructor-built naval actor, never copy its VM."""

    event_bytes = 0x300  # Shared tagged-event fixture allocation.

    def __init__(self, case=None):
        self.trace_active = False
        self.sound_events = []
        self.sound_calls = []
        self.sound_event_pointers = []
        self.process_entry = self.moving_entry = None
        self.moving_answers = None
        self.playback = 'disabled'
        case = {} if case is None else case
        super().__init__(dict(name='idle_squid', type='SQD', land=2))
        self.phase = 'setup'
        self.original_text = bytes(self.u.mem_read(0x401000, 0x3E0000))
        self.constructor = self.foot_state()
        self.constructor['walk_rate'] = signed(self.read32(self.typ + 0x294))
        self.constructor['idle_rate'] = signed(self.read32(self.typ + 0x298))
        self.constructor['sound_vector_count'] = self.read32(self.typ + 0x504)
        self.sound_registry = sound_inputs(self, ASSETS, {}, wanted_names=SOUND_NAMES)
        self.sound_types = {}
        self.sound_fields = []
        for row in self.sound_registry['rows']:
            index = row['fixture_index']
            pointer = self.read32(self.read32(self.read32(0xB1D37C) + index * 4))
            self.sound_types[row['name']] = pointer
            self.sound_fields.append(dict(row, **{
                key: signed(self.read32(pointer + offset))
                for key, offset in SOUND_FIELDS.items()}))
        map_path = Path(os.environ.get('VERA20K_MOVE_SOUND_MAP', str(assets_root() / 'Hills.map')))
        self.type_layers = []
        for name, path in [(name, ASSETS / name) for name in
                           ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini')] + [('Hills.map', map_path)]:
            if not path.exists():
                assert name == 'LANGRULE.INI', ('missing physical input', path)
                self.type_layers.append(dict(file=name, absent=True))
                continue
            raw = path.read_bytes()
            sections, lines = lexical(raw, {'SQD'})
            before = self.binding()
            self.read_type_sound(sections, rates=True)
            self.type_layers.append(dict(file=name, sha256=hashlib.sha256(raw).hexdigest(),
                                         bytes=len(raw), sections=sections, source_lines=lines,
                                         before=before, after=self.binding()))
        self.process_entry = self.read32(self.read32(self.loco + 4) + 0x40)
        self.moving_entry = self.read32(self.read32(self.loco + 4) + 0x80)
        self.u.mem_write(0x8464AC, b'\0')
        for pointer in self.rngs.values():
            self.invoke(0x65C6D0, pointer, (case.get('seed', 31),))
        self.apply_inputs(case)
        self.phase = 'measure'

    def binding(self):
        count = self.read32(self.typ + 0x504)
        data = self.read32(self.typ + 0x4F8)
        indices = [signed(self.read32(data + i * 4)) for i in range(count)]
        return dict(walk_rate=signed(self.read32(self.typ + 0x294)),
                    idle_rate=signed(self.read32(self.typ + 0x298)),
                    indices=indices, names=[self.sound_name(index) for index in indices])

    def sound_name(self, index):
        if index < 0 or index >= self.read32(0xB1D388):
            return None
        pointer = self.read32(self.read32(self.read32(0xB1D37C) + index * 4))
        return self.string(pointer + 0x6C)

    def read_type_sound(self, sections, *, rates=False):
        self.make_ini(sections)
        if 'SQD' not in sections:
            return
        registers = {UC_X86_REG_EBP: self.typ, UC_X86_REG_ESI: INI,
                     UC_X86_REG_EBX: self.typ + 0x24}
        if rates:
            self.block(0x712222, 0x712256, registers)
        # This entry also stores the preceding scalar result at +568. Preserve
        # that unrelated retained value instead of inventing a reader result.
        self.block(0x713459, 0x7134D9,
                   registers | {UC_X86_REG_EAX: self.read32(self.typ + 0x568)})
        assert self.u.reg_read(UC_X86_REG_ESP) == SP

    def apply_inputs(self, case):
        u = self.u
        if 'move_sound' in case:
            self.read_type_sound({'SQD': {'MoveSound': case['move_sound']}})
        rate_keys = {key: str(case[field]) for key, field in
                     [('WalkRate', 'walk_rate'), ('IdleRate', 'idle_rate')] if field in case}
        if rate_keys:
            self.read_type_sound({'SQD': rate_keys}, rates=True)
        for field, offset in [('body_counter', 0x538), ('countdown', 0x540)]:
            if field in case:
                u.mem_write(self.actor + offset, dwords(case[field]))
        for field, offset in [('active', 0x53C), ('falling', 0x8D), ('crashing', 0x425),
                              ('sinking', 0x3CD), ('limbo', 0x81), ('alive', 0x90)]:
            if field in case:
                u.mem_write(self.actor + offset, bytes([case[field]]))
        if 'field_2a8_present' in case:
            u.mem_write(self.actor + 0x2A8, dwords(self.actor if case['field_2a8_present'] else 0))
        if 'type_692' in case:
            u.mem_write(self.typ + 0x692, bytes([case['type_692']]))
        if case.get('playing_handle'):
            self.seed_playing_event()
        self.playback = case.get('playback', 'disabled')

    def seed_playing_event(self, *, name='SquidMove', state=3, flags=8,
                           control=None, loop_count=None, invalid=None):
        pointer = self.sound_types[name]
        if control is not None:
            self.u.mem_write(pointer + 0x10, dwords(control))
        if loop_count is not None:
            self.u.mem_write(pointer + 0x4C, dwords(loop_count))
        event, handle = playing_handle(self, pointer, handle=self.actor + 0x544)
        self.sound_event_pointers.append(event)
        self.u.mem_write(event + 0x1C, dwords(state))
        self.u.mem_write(event + 0x18, dwords(flags))
        if invalid == 'serial':
            self.u.mem_write(handle + 4, dwords(42))
        elif invalid == 'tag':
            self.u.mem_write(handle + 0xC, dwords(0))
        elif invalid == 'entry':
            self.u.mem_write(handle + 8, dwords(self.sound_types['GenLargeWaterDie']))
        elif invalid == 'null':
            self.u.mem_write(handle, dwords(0))
        elif invalid == 'backend':
            self.u.mem_write(0x87E2A0, dwords(0))
        return event

    def event_state(self, pointer):
        return dict(state=signed(self.read32(pointer + 0x1C)), flags=self.read32(pointer + 0x18),
                    serial=self.read32(pointer + 0x138),
                    bytes_sha256=hashlib.sha256(bytes(self.u.mem_read(pointer, self.event_bytes))).hexdigest())

    def foot_state(self):
        u = self.u
        handle = self.actor + 0x544
        return dict(body_counter=self.read32(self.actor + 0x538),
                    active_raw=u.mem_read(self.actor + 0x53C, 1)[0],
                    countdown=signed(self.read32(self.actor + 0x540)),
                    xyz=list(struct.unpack('<3i', u.mem_read(self.actor + 0x9C, 12))),
                    handle_fields=dict(event_present=bool(self.read32(handle)),
                                       serial=self.read32(handle + 4),
                                       sound_present=bool(self.read32(handle + 8)),
                                       manager_tag=self.read32(handle + 0xC)),
                    event_fields=[self.event_state(pointer) for pointer in self.sound_event_pointers],
                    rng={name: bytes(u.mem_read(pointer, 0x3F4)).hex()
                         for name, pointer in self.rngs.items()},
                    rng_cursors={name: list(struct.unpack('<ii', u.mem_read(pointer + 4, 8)))
                                 for name, pointer in self.rngs.items()})

    def start_trace(self):
        self.sound_events = []
        self.sound_calls = []
        self.writes = []
        self.calls = []
        self.native_calls = NativeCallTrace(self.u, self.read32, self.sound_calls)
        self.trace_active = True

    def finish_trace(self):
        self.native_calls.returned(self.u.reg_read(UC_X86_REG_EIP), self.u.reg_read(UC_X86_REG_ESP))
        assert not self.native_calls.pending, self.native_calls.pending
        assert bytes(self.u.mem_read(0x401000, 0x3E0000)) == self.original_text
        self.trace_active = False

    def hook(self, u, pc, size, data):
        if self.trace_active:
            sp = u.reg_read(UC_X86_REG_ESP)
            for index in self.native_calls.returned(pc, sp):
                call = self.sound_calls[index]
                if call['name'] == 'random_next':
                    self.sound_events.append(dict(kind='random_return', stream=call['stream'],
                                                  raw=call['return_eax']))
                elif call['name'] == 'is_moving_now':
                    self.sound_events.append(dict(kind='is_moving_now_return',
                                                  raw_eax=call['return_eax'],
                                                  al=call['return_eax'] & 255,
                                                  caller=call['caller']))
                elif call['name'] == 'playback':
                    self.sound_events.append(dict(kind='playback_return', raw_eax=call['return_eax']))
            specs = {0x405D40: ('hard_stop', 0, 0), 0x405FD0: ('decay_stop', 0, 0),
                     0x406060: ('release', 0, 0), 0x405C00: ('handle_destructor', 0, 0),
                     0x7509E0: ('playback', 1, 4), 0x65C780: ('random_next', 0, 0),
                     self.process_entry: ('supplied_process', 1, 4),
                     self.moving_entry: ('is_moving_now', 1, 4)}
            if pc in specs:
                index = self.native_calls.entered(pc, sp, specs[pc])
                call = self.sound_calls[index]
                event = dict(kind=call['name'], pc=hex(pc), caller=call['caller'])
                if pc == 0x65C780:
                    call['stream'] = next((name for name, pointer in self.rngs.items()
                                           if pointer == u.reg_read(UC_X86_REG_ECX)), 'other')
                    event['stream'] = call['stream']
                elif pc == 0x7509E0:
                    sound_index = signed(u.reg_read(UC_X86_REG_ECX))
                    event.update(index=sound_index, name=self.sound_name(sound_index),
                                 xyz=list(struct.unpack('<3i', u.mem_read(u.reg_read(UC_X86_REG_EDX), 12))),
                                 handle_is_foot=self.read32(sp + 4) == self.actor + 0x544,
                                 boundary=self.playback)
                self.sound_events.append(event)
                if pc == self.process_entry:
                    # Held Ship movement is not executed by this sound fixture.
                    self.ret(0, 4)
                    return
                if pc == self.moving_entry and self.moving_answers is not None:
                    assert self.moving_answers, 'missing declared IsMovingNow answer'
                    answer = self.moving_answers.pop(0)
                    event['supplied_al'] = answer
                    self.ret(answer, 4)
                    return
                if pc == 0x7509E0 and self.playback == 'accepted_boundary':
                    # Existing tagged event boundary; native device/mixer/sample
                    # service and its later Main draws are deliberately excluded.
                    event_pointer = self.seed_playing_event(name=event['name'])
                    self.ret(event_pointer, 4)
                    return
            if pc == 0x4DAAE0:
                self.sound_events.append(dict(kind='native_vector_selection',
                                              slot=u.reg_read(UC_X86_REG_EDX),
                                              index=signed(u.reg_read(UC_X86_REG_ECX)),
                                              name=self.sound_name(signed(u.reg_read(UC_X86_REG_ECX)))))
        super().hook(u, pc, size, data)

    def visit(self, frame, *, moving_answers=None):
        self.u.mem_write(0xA8ED84, dwords(frame))
        self.moving_answers = None if moving_answers is None else list(moving_answers)
        before = self.foot_state()
        self.start_trace()
        endpoint = run_foot_counter_slice(self.u, self.actor, SP, through_move_sound=True)
        self.finish_trace()
        if self.moving_answers is not None:
            assert not self.moving_answers, ('unused declared IsMovingNow answers', self.moving_answers)
        return dict(frame=frame, before=before, after=self.foot_state(), endpoint=hex(endpoint),
                    events=self.sound_events, calls=self.sound_calls, writes=self.writes,
                    supplied_moving_answers=moving_answers)


def timeline(name, frames=None, *, start_frame=None, frame_count=None, **inputs):
    supplied = dict(seed=31, **inputs)
    if frames is None:
        if start_frame is None or frame_count is None or frame_count <= 0:
            raise ValueError('A timeline interval requires start_frame and positive frame_count')
        frames = range(start_frame, start_frame + frame_count)
        supplied.update(start_frame=start_frame, frame_count=frame_count)
    elif start_frame is not None or frame_count is not None:
        raise ValueError('Use explicit frames or an interval, not both')
    fixture = FootSound(inputs)
    return dict(name=name, input=supplied, binding=fixture.binding(),
                steps=[fixture.visit(frame) for frame in frames])


def control(name, *, frame=1, moving_answers=None, **inputs):
    fixture = FootSound(inputs)
    row = fixture.visit(frame, moving_answers=moving_answers)
    return dict(name=name, input=dict(seed=31, frame=frame, moving_answers=moving_answers, **inputs),
                binding=fixture.binding(), **row)


def handle_controls():
    rows = []
    conditions = [
        ('stock_playing', {}), ('supplied_pending_state2', dict(state=2)),
        ('authored_infinite_loop', dict(control=1, loop_count=0)),
        ('authored_finite_loop', dict(control=1, loop_count=2)),
        ('already_decay_flag', dict(flags=0x28)),
        ('invalid_serial', dict(invalid='serial')), ('invalid_registry_tag', dict(invalid='tag')),
        ('invalid_sound_entry', dict(invalid='entry')), ('null_event', dict(invalid='null')),
        ('backend_unavailable', dict(invalid='backend')),
    ]
    operations = [('hard_stop', 0x405D40), ('decay_stop', 0x405FD0), ('release', 0x406060),
                  ('foot_limbo_tail', None), ('foot_destructor_tail', None)]
    for condition, fields in conditions:
        for operation, entry in operations:
            fixture = FootSound()
            fixture.seed_playing_event(**fields)
            before = fixture.foot_state()
            fixture.start_trace()
            if operation == 'foot_limbo_tail':
                fixture.block(0x4DB34D, 0x4DB358, {UC_X86_REG_EDI: fixture.actor})
            elif operation == 'foot_destructor_tail':
                fixture.block(0x4D366E, 0x4D3683, {UC_X86_REG_ESI: fixture.actor, UC_X86_REG_EBX: 0})
            else:
                fixture.invoke(entry, fixture.actor + 0x544)
            fixture.finish_trace()
            rows.append(dict(name=condition + '_' + operation, operation=operation,
                             input=fields, before=before, after=fixture.foot_state(),
                             events=fixture.sound_events, calls=fixture.sound_calls))
    return rows


def load_controls():
    rows = []
    for name, active, countdown, counter in [('active', 1, 3, 37), ('inactive', 0, 5, 9)]:
        fixture = FootSound(dict(active=active, countdown=countdown, body_counter=counter,
                                 playing_handle=True))
        fixture.phase = 'setup'
        before = fixture.foot_state()
        size = fixture.invoke(fixture.read32(fixture.read32(fixture.actor) + 0x30), fixture.actor)
        assert size == 0x8E8
        raw = bytes(fixture.u.mem_read(fixture.actor, 0x800)) + bytes(size - 0x800)
        loaded = raw_load_sound_reset(fixture, raw)
        after = fixture.foot_state()
        assert fixture.read32(fixture.actor + 0x674) == 0
        # NoInit deliberately clears the saved COM pointer. The intervening
        # COM loader is outside this raw-load fixture; reconnect its existing
        # constructor-built idle Ship as an explicit next-visit boundary.
        fixture.u.mem_write(fixture.actor + 0x674, dwords(fixture.loco + 4))
        fixture.u.mem_write(fixture.loco + 0xC, dwords(fixture.actor))
        fixture.read_ranges = [(name, fixture.actor if name == 'actor' else pointer, length)
                               for name, pointer, length in fixture.read_ranges]
        fixture.phase = 'measure'
        rows.append(dict(name=name, input=dict(saved_active=active, saved_countdown=countdown,
                                               saved_body_counter=counter, ordinary_sinking=False),
                         before=before, raw_load=loaded, after=after,
                         next_visit_boundary='Existing constructor-built idle Ship is reattached after NoInit clears the saved COM pointer; COM loading is not executed.',
                         next_visit=fixture.visit(4)))
    return rows


def reader_controls():
    fixture = FootSound()
    fixture.phase = 'setup'
    rows = []
    for raw in ['SquidMove,GenLargeWaterDie', None, '', '   ', 'not-registered',
                'SquidMove,SQUIDMOVE,GenLargeWaterDie', 'SquidMove, GenLargeWaterDie',
                ',,', '<none>', 'none', ',SquidMove,,GenLargeWaterDie,',
                'SquidMove,' * 12 + 'GenLargeWaterDie']:
        before = fixture.binding()
        fixture.read_type_sound({'SQD': {} if raw is None else {'MoveSound': raw}})
        rows.append(dict(raw=raw, before=before, after=fixture.binding()))
    return rows


def registered_null_name_controls():
    """Separate authored catalog: original registry accepts both literal names.

    The list resolver, rather than the registry constructor, reserves <none>.
    Preserve the physical catalog fixture and use its existing source-order
    writer; no host tokenization or replacement parser supplies the results.
    """
    fixture = FootSound()
    fixture.phase = 'setup'
    sections = {'SoundList': {'authored0': 'none', 'authored1': '<none>'},
                'none': {'Control': 'random'}, '<none>': {'Control': 'random'}}
    proxy = Sound.__new__(Sound)
    proxy.__dict__ = fixture.__dict__
    Sound.make_ini(proxy, sections)
    fixture.invoke(0x7510D0, INI)
    catalog = [fixture.sound_name(index) for index in range(fixture.read32(0xB1D388))]
    rows = []
    for raw in ('none', '<none>', 'none,<none>,SquidMove', 'NoNe,<NoNe>'):
        before = fixture.binding()
        fixture.read_type_sound({'SQD': {'MoveSound': raw}})
        rows.append(dict(raw=raw, before=before, after=fixture.binding()))
    return dict(authored_sections=sections, catalog=catalog, histories=rows)


class QueuedSound(FootSound):
    """Original start-pass suffix using an existing, loaded sample boundary.

    The shared native pool/event/list bodies initialize these events. Ready
    state, one preloaded sample and ranking buckets are declared inputs; sample
    decoding, ranking production and accepted device playback are excluded.
    """

    event_bytes = 0x280  # Original405190 SoundEvent pool record.

    def __init__(self, victims=()):
        self.queued_trace = False
        super().__init__()
        self.phase = 'setup'
        u = self.u
        u.reg_write(UC_X86_REG_EDX, 0x280)
        self.pool = self.invoke(0x4074D0, 6)
        u.mem_write(0x87E2A8, dwords(self.pool))
        u.mem_write(0x87E2A4, dwords(1))
        u.mem_write(0x816108, dwords(1))
        u.mem_write(0x81610C, dwords(1))
        self.invoke(0x4072C0, 0x87E180)
        for index in range(70):
            self.invoke(0x4072C0, 0x87DE38 + index * 12)
        self.queued_event = self.invoke(0x405190, self.sound_types['SquidMove'])
        assert self.queued_event
        playing_handle(self, self.sound_types['SquidMove'], handle=self.actor + 0x544,
                       event=self.queued_event, serial=self.read32(self.queued_event + 0x138))
        self.sound_event_pointers.append(self.queued_event)
        u.mem_write(self.queued_event + 0x1C, dwords(1))
        u.mem_write(self.queued_event + 0x18, dwords(0))
        self.sample = self.alloc(0x80)
        u.mem_write(self.sample + 0xC, dwords(1))
        u.mem_write(self.queued_event + 0xA8, dwords(1))
        u.mem_write(self.queued_event + 0x28, dwords(self.sample))
        self.victims = []
        self.victim_priority = None
        if victims:
            # Original Voc reader supplies the authored lower-priority contrast.
            self.make_ini({'GenLargeWaterDie': {'Priority': 'LOW'}})
            pointer = self.sound_types['GenLargeWaterDie']
            index = self.invoke(0x7514D0, self.cstring('GenLargeWaterDie'))
            wrapper = self.read32(self.read32(0xB1D37C) + index * 4)
            self.invoke(0x750440, wrapper, (INI,))
            self.victim_priority = signed(self.read32(pointer + 0x40))
            for state, flags in victims:
                event = self.invoke(0x405190, pointer)
                assert event
                self.victims.append(event)
                self.sound_event_pointers.append(event)
                u.mem_write(event + 0x1C, dwords(state))
                u.mem_write(event + 0x18, dwords(flags))
                if flags & 1:
                    u.mem_write(event + 0x138, dwords(0))
            # Supplied bucket0 membership after the excluded ranking pass.
            u.reg_write(UC_X86_REG_EDX, pointer + 0xA4)
            self.invoke(0x407420, 0x87DE38 + self.victim_priority * 0x78)
        self.phase = 'measure'
        self.queued_trace = True

    def hook(self, u, pc, size, data):
        if self.queued_trace and self.trace_active:
            names = {0x404700: 'prepare_playout', 0x4048B0: 'load_samples',
                     0x404E20: 'find_lowest_priority', 0x4052F0: 'stop_event',
                     0x404DD0: 'return_to_pool', 0x404140: 'release_channel',
                     0x404D80: 'unload_samples', 0x4054A0: 'start_playback'}
            if pc in names:
                this = u.reg_read(UC_X86_REG_ECX)
                event_index = self.sound_event_pointers.index(this) if this in self.sound_event_pointers else None
                index = self.native_calls.entered(pc, u.reg_read(UC_X86_REG_ESP), (names[pc], 0, 0))
                self.sound_calls[index]['event_index'] = event_index
                self.sound_events.append(dict(kind=names[pc], this=hex(this), event_index=event_index))
                if pc == 0x4054A0:
                    # Only admission to the original playback owner is measured.
                    self.ret(1)
                    return
        super().hook(u, pc, size, data)

    def queue_state(self):
        live = []
        pointer = self.read32(0x87E180)
        while pointer != 0x87E180:
            assert len(live) < 6
            live.append(self.sound_event_pointers.index(pointer))
            pointer = self.read32(pointer)
        return dict(foot=self.foot_state(), live_event_indices=live,
                    live_count=self.read32(0x87E28C), sample_references=self.read32(self.sample + 0xC),
                    loaded_sample_count=self.read32(self.queued_event + 0xA8),
                    playlist_result_present=bool(self.read32(self.queued_event + 0x148)))

    def run_start_pass(self, operation):
        before = self.queue_state()
        self.start_trace()
        if operation is not None:
            self.invoke(operation, self.actor + 0x544)
            self.native_calls.returned(self.u.reg_read(UC_X86_REG_EIP), self.u.reg_read(UC_X86_REG_ESP))
        after_handle = self.queue_state()
        self.block(0x4045A6, 0x4046C6, {UC_X86_REG_ESI: self.queued_event})
        self.finish_trace()
        return dict(before=before, after_handle=after_handle, after=self.queue_state(),
                    events=self.sound_events, calls=self.sound_calls)


def queued_controls():
    rows = []
    for name, operation, victims in [
            ('ordinary_ready', None, ()), ('released_ready', 0x406060, ()),
            ('detached_ready', 0x405FD0, ()),
            ('detached_with_lower_entry', 0x405FD0, ((3, 8), (1, 0), (4, 1))),
            ('released_with_lower_entry', 0x406060, ((3, 8), (1, 0), (4, 1)))]:
        fixture = QueuedSound(victims)
        result = fixture.run_start_pass(operation)
        rows.append(dict(name=name, input=dict(operation=None if operation is None else hex(operation),
                                               preloaded_samples=1, current_state=1, current_flags=0,
                                               victims=[dict(state=state, flags=flags) for state, flags in victims],
                                               authored_victim_priority='LOW' if victims else None,
                                               native_victim_priority=fixture.victim_priority,
                                               victim_rank_bucket=0 if victims else None), **result))
    return rows


def generate():
    fixture = FootSound()
    retail = dict(constructor=fixture.constructor, type_layers=fixture.type_layers,
                  sound_registry=fixture.sound_registry, sound_fields=fixture.sound_fields,
                  process_entry=hex(fixture.process_entry), is_moving_now_entry=hex(fixture.moving_entry),
                  inherited_fixture=dict(actor_constructor=fixture.actor_ctor,
                                         inputs=fixture.inputs, layers=fixture.layers))
    controls = [
        control('empty_qualifies', frame=4, move_sound='not-registered'),
        control('empty_active_qualifies', frame=4, move_sound='not-registered', active=1, countdown=0),
        control('inactive_lapse_retains_countdown', active=0, countdown=5),
        control('active_positive_lapse', active=1, countdown=3),
        control('active_zero_releases', active=1, countdown=0, playing_handle=True),
        control('active_signed_min_wraps', active=1, countdown=-2147483648),
        control('active_negative_decrements', active=1, countdown=-1),
        control('falling_releases', frame=4, active=1, countdown=3, falling=1, playing_handle=True),
        control('crashing_releases', frame=4, active=1, countdown=3, crashing=1, playing_handle=True),
        control('falling_inactive', frame=4, falling=1),
        control('crashing_inactive', frame=4, crashing=1),
        control('native_wrap_counter', frame=4, body_counter=0xFFFFFFFF),
        control('old_handle_stopped_before_rejected_start', frame=4, playing_handle=True),
        control('moving_without_counter_delta', moving_answers=[1, 1, 1]),
        control('fresh_moving_now_only', moving_answers=[0, 0, 1]),
        control('counter_delta_short_circuits_fresh_query', frame=4, moving_answers=[0, 0]),
        control('pair_marker_type692_false', frame=4, field_2a8_present=True, type_692=0),
        control('pair_marker_type692_true', frame=4, field_2a8_present=True, type_692=1),
    ]
    return dict(schema_version=1, retail=retail, reader_histories=reader_controls(),
                registered_null_names=registered_null_name_controls(),
                timelines=[timeline('stock_idle_disabled', range(1, 17)),
                           timeline('stock_idle_accepted_boundary', range(1, 13), playback='accepted_boundary'),
                           timeline('authored_idle_rate5_lapse', range(1, 12), idle_rate=5,
                                    playback='accepted_boundary'),
                           timeline('ordered_two_sounds', [4], move_sound='SquidMove,GenLargeWaterDie'),
                           timeline('ordered_duplicate_sounds', [4],
                                    move_sound='GenLargeWaterDie,SquidMove,GenLargeWaterDie'),
                           timeline('stock_idle_frame_zero', start_frame=0, frame_count=16)],
                controls=controls, handles=handle_controls(), load=load_controls(),
                queued_playout=queued_controls())


def source_paths():
    root = HERE.parents[2]
    # All fixture imports above are loaded before the publisher snapshots this
    # closure. Include the CLI owner explicitly; no Rust or transient run log.
    paths = {str(path.relative_to(root)): path for module in tuple(sys.modules.values())
             if (name := getattr(module, '__file__', None))
             and (path := Path(name).resolve()).is_relative_to(root / 'tools')
             and path.suffix == '.py'}
    paths['tools/spatial_oracle/fv_cell_attack/move_sound.py'] = HERE / 'move_sound.py'
    return paths


def metadata():
    result = provenance(
        scope=__doc__,
        entry_points=dict(foot_counter_and_sound=0x4DA806, sound_tail=0x4DAA01,
                          counter_update=0x4DA886, idle_rate_division=0x4DA989,
                          body_counter_increment=0x4DA9FB, body_rate_reader=0x712222,
                          move_sound_reader=0x713459, sound_list=0x525430,
                          sound_registry=0x7510D0, voc_reader=0x750440,
                          ship_constructor=0x69EC50, ship_is_moving_now=0x69F330,
                          random_next=0x65C780, playback=0x7509E0,
                          hard_stop=0x405D40, decay_stop=0x405FD0, release=0x406060,
                          foot_limbo_tail=0x4DB34D, foot_destructor_tail=0x4D366E,
                          raw_load=0x410380, foot_load_reset=0x4DB60D),
        assumptions=[
            'Existing naval_occupants VM owns original UnitType/base Unit/Foot constructor prefix and Ship constructor, supplied actor/house/cell placement and its inherited selected type/rules reads. This is an already-live healthy SQD boundary, not full Unlimbo or physical map placement.',
            'Selected physical RULESMD, absent LANGRULE, MPBattleMD and Hills inner-map INI caches execute original WalkRate then IdleRate/current defaults and MoveSound ordered-vector blocks. Fixed SOUNDMD original selected SoundList/Defaults/Voc bodies bind SquidMove and GenLargeWaterDie; indexes are fixture-relative and sample lookup maps physical names locally.',
            'Original4DA806..4DAB3C captures the actual pre-Process body counter, executes original gates, original body counter and complete sound tail. Stock idle IsMovingNow executes original69F330 from original Ship ctor state. Explicit callback variants are supplied retained responses. Earlier Foot/Techno AI and full object/world scheduling are excluded.',
            'stock_idle_frame_zero supplies globalA8ED84 values0..15 to the same already-live SQD boundary. Original idle-rate division4DA989 and counter increment4DA9FB execute at frame0; no host modulo or counter delta is supplied. Its before/after visit states bracket the AI body, while production exact-step receipts expose the subsequently committed frame. Native scenario frame initialization and whole first-frame AI are excluded.',
            'All three whole0x3F4 RNG states are seeded through original65C6D0. Draw results, modulo-selected list slot, order and counter/timer writes come from original execution. No host arithmetic supplies an expected output. Foot selection draws are distinct from later audio-service Main RNG.',
            'Tagged playing/pending and authored loop controls are explicit event/type boundary fields, with no attached channel/sample buffer. Original405D40/405FD0/406060 and bounded FootLimbo/destructor suffixes execute all cleanup writes. Pending state2 is a supplied state, not an event-allocation lifetime claim.',
            'Raw-load controls reuse naval_lifetime_controls: original AbstractLoad, unconditional Foot audio reset, no-init Foot ctor and Unit vtables execute against supplied Unit-size bytes. Dynamic vectors, COM/pointer swizzling and whole old-world Clear are excluded. NoInit clears the saved locomotor pointer; the next visit explicitly reattaches the existing constructor-built idle Ship and uses the declared Process boundary. Pair+2A8/type+692 controls supply presence/byte only and do not establish reciprocal-link lifecycle.',
            'Queued controls construct the original six-record pool4074D0 and SoundEvent405190, then supply ready state1, one already-loaded sample and optional lower-entry bucket0 membership. Original4048B0 takes its already-loaded short path; full404700 and start-pass4045A6..4046C6 perform rejection, priority lookup, retries, collateral cleanup and pool return. Lower-entry Priority=LOW is read by original750440; its playing/ready/dead records are explicit inputs. Sample loading, whole ranking pass and the surrounding audio scheduler are excluded.',
        ],
        substitutions=[
            'Inherited allocator, CRT/TLS, Interlocked and prepared INI/source-order cache boundaries remain unchanged. Original .text is checked byte-for-byte.',
            'Ship Process69FC10 returns through an explicit RET4 transport without movement. Ordinary IsMovingNow executes original code; separately named authored callback controls declare every supplied answer.',
            'Default playback executes original7509E0 through supplied8464AC=0 disabled gate. Accepted-boundary timelines attach the shared tagged playing-event fixture and return from7509E0; decoding, device mixing, audio scheduling and their later Main draws are excluded. They prove caller latch/handle lifetime, not audible sound.',
            'Queued ordinary/released controls observe admission to4054A0 and supply success without running device playback. The one preloaded SampleTracker record and its reference count are supplied decoder boundaries; original sample-reference release executes. Global pool-enable flags and bucket membership are supplied startup/ranking state.',
        ])
    result['entry_points'].update(prepare_playout='0x00404700',
                                  start_pass='0x004045A6', find_lowest_priority='0x00404E20',
                                  return_to_pool='0x00404DD0', sound_event_allocate='0x00405190')
    result['command'] = 'python -m tools.spatial_oracle.fv_cell_attack.move_sound --foot-tail --check'
    result['input_environment'] = ['VERA20K_GAMEMD_EXE or RA2_DIR', 'VERA20K_SHRAPNEL_INPUTS',
                                   'VERA20K_ANYTOWN_INPUTS', 'VERA20K_PROJECTILE_RENDER_ASSETS',
                                   'optional VERA20K_MOVE_SOUND_MAP']
    return result
