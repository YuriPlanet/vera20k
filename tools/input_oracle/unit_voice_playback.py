"""Original GI order acknowledgements on the existing joined audio fixture.

G/selection/event transport, object AI, the audio pool, codecs and worker execute
original instructions. Explicit map/House and single-thread OS inputs bound the
comparison; the post-delivery movement route stops before held FindPath work.
"""
from collections import deque
from pathlib import Path
from types import SimpleNamespace
import copy
import hashlib
import os
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
from tools import native_inspect as inspect
from tools.native_oracle import RET_MAGIC, finish_vectors, image_bytes, provenance, run_checked
from tools.spatial_oracle import engineer_repair_admission as er
from tools.spatial_oracle import building_construction as bc
# JoinedFixture's lazy dependencies must be loaded before the source guard.
from tools.spatial_oracle import refinery_dock, building_slot_replacement, anim_bouncer_launch
from tools.spatial_oracle.building_body_rules import INI, SP, dwords
from tools.spatial_oracle.walk_first_path import GIMoveHistory
from tools.spatial_oracle.anytown_damage.mission import Mission
from tools.spatial_oracle._factory_infantry_output.runtime import require
from tools.spatial_oracle._factory_infantry_output.pcm import NativePcmObserver
from tools.rules_oracle.bridge_child_sound import sections
from tools.input_oracle.area_guard import read_guard, source_paths, sound_name, voice_binding

HERE = Path(__file__).resolve().parent
SOUND_NAMES = ('GIMove', 'CommandBar', 'GIFear')


def signed(u, address):
    return struct.unpack('<i', u.mem_read(address, 4))[0]


def feedback_binding(m, typ):
    ids = [signed(m.u, m.read32(typ + 0x4DC) + 4 * i)
           for i in range(m.read32(typ + 0x4E8))]
    return dict(ids=ids, names=[sound_name(m, i) for i in ids])


def read_feedback(m, typ, values):
    m.make_ini(values)
    for register, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBP, typ),
                            (UC_X86_REG_EBX, typ + 0x24), (UC_X86_REG_ESI, INI)):
        m.u.reg_write(register, value)
    run_checked(m.u, 0x712D99, 0x712E03,
                required_addresses=[0x525430, 0x478720, 0x477D20])
    require(m.u.reg_read(UC_X86_REG_ESP) == SP, 'VoiceFeedback reader stack differs')
    return feedback_binding(m, typ)


def prepare(root):
    """Add physical selected type/sound readers to the canonical input owner."""
    m = er.prepare_joined_inputs(root)
    physical = sections((root / 'SOUNDMD.INI').read_bytes())
    selected = {name: physical[name] for name in ('Defaults', *SOUND_NAMES)}
    selected['SoundList'] = {key: value for key, value in physical['SoundList'].items()
                             if value in SOUND_NAMES}
    m.make_ini(selected)
    m.invoke(0x7510D0, INI)
    m.voice_sound_ids = {name: m.invoke(0x7514D0, m.cstring(name)) for name in SOUND_NAMES}
    e1 = m.alloc(0x1900)
    m.invoke(0x5236A0, e1, (m.cstring('E1'),))
    m.types['E1'] = e1
    feedback_constructor = feedback_binding(m, e1)
    art_all = sections((root / 'ARTMD.INI').read_bytes())
    art = {name: art_all[name] for name in ('E1', 'GI', 'GISequence') if name in art_all}
    layers = []
    for filename in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'Hills.mmx'):
        path = root / filename
        if not path.exists():
            require(filename == 'LANGRULE.INI', 'Missing physical input: ' + str(path))
            layers.append(dict(file=filename, absent=True))
            continue
        raw = path.read_bytes()
        sec = sections(raw)
        m.make_ini({'E1': sec['E1']} if 'E1' in sec else {})
        rules_ini = m.alloc(0x40)
        m.u.mem_write(rules_ini, bytes(m.u.mem_read(INI, 0x40)))
        m.make_ini(art)
        m.invoke(0x674000, 0, (rules_ini,))
        admitted = m.invoke(0x5240A0, e1, (rules_ini,)) & 255
        guard = read_guard(m, m.rules, {'AudioVisual': {
            'GuardSound': sec.get('AudioVisual', {}).get('GuardSound', '')}})
        m.make_ini({name: sec[name] for name in ('General', 'Radiation') if name in sec})
        m.invoke(0x66D530, m.rules, (INI,), timeout_us=30_000_000)
        m.invoke(0x66CF70, m.rules, (INI,))
        layers.append(dict(file=filename, sha256=hashlib.sha256(raw).hexdigest(),
            admitted=admitted, voice_special_attack=voice_binding(m, e1), guard=guard,
            voice_feedback=feedback_binding(m, e1)))
    sounds = []
    for name, index in m.voice_sound_ids.items():
        p = m.read32(m.read32(m.read32(0xB1D37C) + index * 4))
        sounds.append(dict(name=name, index=index, pointer=p,
            fields={key: m.read32(p + off) for key, off in {
                'control': 0x10, 'type': 0x14, 'volume_fixed16': 0x1C,
                'priority': 0x40, 'limit': 0x48, 'delay_low': 0x58,
                'delay_high': 0x5C, 'fshift_low': 0x60, 'fshift_high': 0x64,
                'vshift': 0x68, 'sample_count': 0x134}.items()},
            sample_indices=[m.read32(p + 0xB4 + i * 4)
                            for i in range(m.read32(p + 0x134))]))
    control_type = m.alloc(0x1900)
    m.invoke(0x710AF0, control_type, (m.cstring('FEEDBACK_READER_CONTROL'),))
    feedback_readers = []
    for label, key, raw in (
        ('single', 'VoiceFeedback', 'GIFear'), ('multiple', 'VoiceFeedback', 'GIFear,GIMove,GIFear'),
        ('missing', None, None), ('empty', 'VoiceFeedback', ''),
        ('unknown_only', 'VoiceFeedback', 'AbsentFeedback'),
        ('duplicates_case', 'VoiceFeedback', 'gIfEaR,GIFear,commandbar'),
        ('wrong_case_key', 'voicefeedback', 'CommandBar'),
        ('none_token', 'VoiceFeedback', '<none>,GIFear'),
    ):
        before = feedback_binding(m, control_type)
        after = read_feedback(m, control_type, {'FEEDBACK_READER_CONTROL': {key: raw} if key else {}})
        feedback_readers.append(dict(name=label, key=key, raw=raw, before=before, after=after))
    # A declared empty catalog isolates original720590's unconditional stream
    # allocation. It does not replace or claim physical Theme catalog parsing.
    m.make_ini({})
    m.voice_theme_ini = m.alloc(0x40)
    m.u.mem_write(m.voice_theme_ini, bytes(m.u.mem_read(INI, 0x40)))
    return m, dict(layers=layers, sound_sections=selected, sounds=sounds,
                  feedback_constructor=feedback_constructor, feedback_reader_controls=feedback_readers)


class VoiceHistory(GIMoveHistory):
    """Observation projection; invocation and event rings keep their owner."""
    WATCH = dict(GIMoveHistory.WATCH)
    WATCH.update({
        0x536D00: ('GuardCommand', 4), 0x730D60: ('GuardSelection', 0),
        0x708D90: ('QueueVoice', 4), 0x750920: ('PlaySound', 8),
        0x7509E0: ('PlayAtPosition', 4), 0x50B6F0: ('HouseInputOwner', 0),
        0x51BAB0: ('InfantryAI', 0), 0x4DA530: ('FootAI', 0),
        0x6F9E50: ('TechnoAI', 0), 0x48D080: ('NetworkService', 0),
        0x406F70: ('AudioService', 0), 0x4041D0: ('SoundService', 0),
        0x4048B0: ('LoadSamples', 0), 0x4047B0: ('AdvancePlaylist', 0),
        0x4055C0: ('UpdateEvent', 0), 0x4054A0: ('StartEvent', 0),
        0x4035F0: ('SelectChannel', None), 0x405A00: ('EventEndpoint', None),
        0x405AC0: ('EventNextSample', None), 0x405C00: ('HandleDestructor', 0),
        0x406060: ('HandleRelease', 0), 0x405D40: ('HandleStopClear', 0),
        0x4DE5D0: ('FootUnInit', 0), 0x5F65F0: ('ObjectUnInit', 0),
        0x4D3590: ('FootDestructor', 0), 0x6F4500: ('TechnoDestructor', 0),
        0x517D90: ('InfantryDestructor', 0), 0x406E80: ('ClearSoundPool', 0),
        0x720A80: ('ThemeNextSong', 4), 0x721140: ('ThemeAllowed', 4),
        0x752290: ('VoxInit', 0), 0x752AD0: ('SpeechInit', 0),
        0x720960: ('ThemeConstructor', 0), 0x720590: ('ThemeReadINI', 4),
        0x407860: ('StreamConstructor', 8), 0x4036C0: ('ReserveChannel', 0),
        0x402440: ('AssignChannelGroup', 0),
    })

    def __init__(self, m, name, *, seed=31, actor_count=2, initial_pump=True):
        f = er.EngineerJoinedFixture(m, seed=seed, arena_size=0x400000)
        self.f, self.u, self.r, self.bc = f, f.u, f.read32, bc
        self.require, self.copy, self.hashlib = require, copy, hashlib
        self.unit, self.name = False, name
        self.sequence, self.phase = 0, 'created'
        self.events, self.writes, self.pending, self.boundaries = [], [], [], []
        self.inputs, self.streams, self.recent = [], {}, deque(maxlen=80)
        self.draw_start, self.advance_start = len(f.draws), len(f.advances)
        # Inherited physical Cell grid; declared nonzero outer map dimensions
        # prevent original568350 treating every idle GI as off-map.
        self.u.mem_write(0x87F7E8 + 0xF4, dwords(16, 16))
        self.actors = []
        prior = f.types['ENGINEER']
        try:
            f.types['ENGINEER'] = m.types['E1']
            for i in range(actor_count):
                self.actors.append(f.actor((13 + i % 3, 14 + i // 3)))
        finally:
            f.types['ENGINEER'] = prior
        self.actor = self.actors[0] if self.actors else 0
        surface = f.allocate(0x40)
        self.u.mem_write(surface, dwords(0x7E2070, 640, 480, 0, 2, 0, 0, 0))
        self.u.mem_write(0x880A04, dwords(surface))
        self.u.mem_write(0xA8B238, dwords(5))
        self.u.mem_write(0xAC4CF4, b'\0')
        self.u.mem_write(0x822CF2, b'\1')
        self.hooks = [self.u.hook_add(UC_HOOK_CODE, self.observe)]
        self.clock = dict(frequency=1000000, counter=1000000)
        calls = inspect.decode_ranges(inspect.selected_ranges(image_bytes(), 0x407550, 0xE00,
            code_only=True), lambda ins: [inspect.instruction_row(ins)])['matches']
        critical = {row['address']: row for row in calls if row['mnemonic'] == 'call' and
            row['operands'] in ('dword ptr [0x7e11e8]', 'dword ptr [0x7e11ec]', 'dword ptr [0x7e11f4]')}
        f.platform_audio.configure_transport(clock=self.clock, critical_calls=critical)
        self.invoke('native_theme_constructor', 0x720960, 0xA83D10)
        self.invoke('native_clock_init', 0x409360, 0)
        self.region('native_stream_list_init', 0x407550, 0x40756C)
        self.invoke('native_voice_init', 0x752290, 0)
        self.invoke('native_speech_init', 0x752AD0, 0)
        self.inputs.append(dict(kind='declared_empty_theme_catalog', sections={}))
        self.invoke('native_empty_catalog_theme_read', 0x720590, 0xA83D10, m.voice_theme_ini)
        if initial_pump:
            self.invoke('empty_audio_pump_1000', 0x406F70, 0)
        self.initial = self.snapshot()

    def observe(self, u, pc, size, data):
        if pc in (0x655560, 0x655740):
            # Existing selected radar tracker presentation boundary, not a
            # second implementation or a gameplay return substitution.
            Mission.observe(SimpleNamespace(trace=self.recent, m=self.f, pending={},
                events=self.events, frame=self.r(bc.FRAME), phase=self.phase), u, pc, size, data)
            return
        super().observe(u, pc, size, data)
        if pc in (0x6F9EBB, 0x6F9F0D, 0x40971A, 0x40974F):
            self.events.append(dict(sequence=self.next_sequence(), kind='native_site', pc=pc,
                phase=self.phase, this=u.reg_read(UC_X86_REG_ESI), state=self.snapshot()))
        if pc in (0x4036C0, 0x402440):
            self.events[-1]['requested_group'] = u.reg_read(UC_X86_REG_EDX)
        if pc == 0x7509E0:
            self.events[-1]['coordinate'] = self.ints(u.reg_read(UC_X86_REG_EDX), 3)

    def snapshot(self):
        u, r = self.u, self.r
        result = self.f.sound_state()
        result['pump_last_ms'] = struct.unpack('<Q', u.mem_read(0x87E760, 8))[0]
        result['rng'] = self.rng()
        result['channels'] = []
        channel = r(self.f.audio_parent + 0xF8)
        while channel and channel != r(channel + 8):
            require(len(result['channels']) < 64, 'Unexpected native channel census')
            result['channels'].append(dict(pointer=channel, group=r(channel + 0xC),
                flags=r(channel + 0xA4), priority=r(channel + 0xA0),
                event=r(channel + 0xC0)))
            channel = r(channel)
        result['actors'] = [dict(pointer=p, id=r(p + 0x10), alive=u.mem_read(p + 0x90, 1)[0],
            limbo=u.mem_read(p + 0x81, 1)[0], logic=u.mem_read(p + 0x98, 1)[0],
            location=self.ints(p + 0x9C, 3), health=signed(u, p + 0x6C),
            mission=signed(u, p + 0xAC), queued=signed(u, p + 0xB4),
            pending=signed(u, p + 0x4F0), last=signed(u, p + 0x4F4),
            handle=list(struct.unpack('<4I', u.mem_read(p + 0x4DC, 16)))) for p in self.actors]
        result['rings'] = self.rings()
        result['logic_members'] = [r(r(0x87F77C) + i * 4) for i in range(r(0x87F788))]
        for event in result['events']:
            p = event['pointer']
            event.update(volume_fixed16=r(p + 0xBC), loaded_samples=[r(p + 0x28 + i * 4)
                for i in range(r(p + 0xA8))], sound_pointer=r(p + 0x24))
            # Original401D11 stores the selected AudioIndex entry at cached+14;
            # cached+3C -> cache+0 -> AudioIndex+0 owns the sorted36-byte table.
            # Read this native association, never infer it from a PCM digest.
            event['loaded_sample_identities'] = []
            for sample in event['loaded_samples']:
                index = r(sample + 0x14)
                audio_index = r(r(sample + 0x3C))
                entry = r(audio_index) + index * 36
                event['loaded_sample_identities'].append(dict(pointer=sample, index=index,
                    name=bytes(u.mem_read(entry, 16)).split(b'\0', 1)[0].decode('latin1'),
                    source_offset=r(entry + 0x10), source_bytes=r(entry + 0x14)))
            if event['channel']:
                backend = r(event['channel'] + 0x158)
                event['channel_parameters'] = dict(
                    volume_words=list(struct.unpack('<3I', u.mem_read(event['channel'] + 0x10, 12))),
                    pitch_words=list(struct.unpack('<3I', u.mem_read(event['channel'] + 0x38, 12))))
                event['backend'] = dict(pointer=backend, buffer=r(backend + 0x64),
                    decoded_remaining=signed(u, backend + 8), cursor_quarter=r(backend + 0xC),
                    ring_bytes=r(backend + 0x68), quantum_bytes=r(backend + 0x6C))
        return result

    def region(self, label, begin, end, *, registers=(), required=(), count=10_000_000):
        self.phase = self.f.phase = label
        row = dict(label=label, entry=begin, stop=end, before=self.snapshot(),
            first_sequence=self.sequence + 1, draw_start=len(self.f.draws),
            advance_start=len(self.f.advances))
        self.boundaries.append(row)
        self.u.reg_write(UC_X86_REG_ESP, SP)
        for register, value in registers:
            self.u.reg_write(register, value)
        reached = run_checked(self.u, begin, end, count=count, timeout_us=60_000_000,
                              required_addresses=[begin, *required])
        self.finish_returns(reached, self.u.reg_read(UC_X86_REG_ESP))
        row.update(reached=reached, after=self.snapshot(), last_sequence=self.sequence,
            requests=copy.deepcopy(self.f.draws[row['draw_start']:]),
            advances=copy.deepcopy(self.f.advances[row['advance_start']:]))
        return row

    def select(self, actors):
        items = self.f.allocate(max(64, 4 * len(actors)))
        self.u.mem_write(items, dwords(*actors))
        self.u.mem_write(0xA8ECBC, dwords(items, max(16, len(actors)), 1, len(actors)))
        self.inputs.append(dict(kind='selection_array', actors=actors))

    def logic(self, label):
        return self.region(label, 0x55B5FF, 0x55B61B,
            registers=((UC_X86_REG_EDI, 0x87F778),), required=(0x51BAB0, 0x6F9EBB))

    def audio(self, milliseconds, *, wall=60_000_000):
        self.clock['counter'] = milliseconds * 1000
        self.inputs.append(dict(kind='os_counter', milliseconds=milliseconds,
                                value=self.clock['counter']))
        return self.invoke('audio_pump_' + str(milliseconds), 0x406F70, 0,
                           count=20_000_000, wall=wall)

    def worker(self, milliseconds, quarter):
        """Resume the actual created device worker through its Sleep seam.

        OS cursor quanta are explicit inputs. Only the original40971A Stop may
        clear a playing status; the next worker iteration calls its endpoint.
        CPU context and a separate existing mapped stack retain the thread.
        """
        platform = self.f.platform_audio
        if not hasattr(self, 'worker_context'):
            created = [row for row in platform.calls if row.get('pc') == '0x00409511']
            require(len(created) == 1 and created[0]['args'][2] == 0x4095B0,
                    'Original device worker creation differs')
            self.worker_parameter = created[0]['args'][3]
            self.devices = {}
            self.worker_context = None
            platform.configure_transport(buffer_devices=self.devices)
            platform.device_stop_updates_status = True
        for event in self.snapshot()['events']:
            if 'backend' not in event:
                continue
            backend = event['backend']
            if not backend['buffer']:
                continue
            require(backend['buffer'] in platform.buffers, 'Unreturned native buffer identity')
            device = self.devices.setdefault(backend['buffer'], dict(value=1, play_cursor=0, write_cursor=0))
            device.update(play_cursor=quarter * backend['quantum_bytes'],
                          write_cursor=quarter * backend['quantum_bytes'])
        main_context = self.u.context_save()
        label = 'original_worker_' + str(milliseconds)
        self.phase = self.f.phase = label
        self.clock['counter'] = milliseconds * 1000
        row = dict(label=label, entry=0x4095B0 if self.worker_context is None else 0x409844,
            stop=0x40983E, before=self.snapshot(), first_sequence=self.sequence + 1,
            draw_start=len(self.f.draws), advance_start=len(self.f.advances),
            worker_parameter=self.worker_parameter, os_devices=copy.deepcopy(self.devices),
            os_counter=self.clock['counter'], device_io_start=len(platform.device_io))
        self.boundaries.append(row)
        if self.worker_context is None:
            worker_sp = SP - 0x10000
            self.u.mem_write(worker_sp, dwords(RET_MAGIC, self.worker_parameter))
            self.u.reg_write(UC_X86_REG_ESP, worker_sp)
            self.u.reg_write(UC_X86_REG_ECX, 0)
        else:
            self.u.context_restore(self.worker_context)
            sp = self.u.reg_read(UC_X86_REG_ESP)
            platform.callsite(0x40983E, 6, [self.r(sp)], 0)
        run_checked(self.u, row['entry'], 0x40983E, count=20_000_000,
                    timeout_us=60_000_000, required_addresses=[0x4095F5])
        self.finish_returns(0x40983E, self.u.reg_read(UC_X86_REG_ESP))
        self.worker_context = self.u.context_save()
        row.update(after=self.snapshot(), os_devices_after=copy.deepcopy(self.devices),
            last_sequence=self.sequence, device_io=copy.deepcopy(platform.device_io[row['device_io_start']:]),
            requests=copy.deepcopy(self.f.draws[row['draw_start']:]),
            advances=copy.deepcopy(self.f.advances[row['advance_start']:]))
        self.u.context_restore(main_context)
        return row

    def project(self):
        require(self.f.code_unchanged(), 'Original executable instructions changed')
        # Boundary states retain complete semantics. Repeated per-call actor/
        # ring snapshots are mechanically omitted except RNG and head sites.
        calls = [{k: v for k, v in row.items() if k not in ('before', 'after', 'at_ret8')}
                 for row in self.events]
        return dict(name=self.name, initial=self.initial, inputs=self.inputs,
            boundaries=self.boundaries, ordered_calls=calls, final=self.snapshot(),
            complete_rng_states=self.streams, original_code_unchanged=True)


def core(m):
    h = VoiceHistory(m, 'two_live_E1_G_logic_pool')
    h.select(list(reversed(h.actors)))
    h.invoke('actual_G', 0x536D00, 0, 0)
    h.pump()
    h.logic('original_pre_delivery_Logic_object_loop')
    require(all(row['alive'] and not row['limbo'] for row in h.snapshot()['actors']),
            'Ordinary fixture actors unexpectedly left the world')
    require(not any(row.get('kind') == 'AudioService' and
                    row.get('phase') == 'original_pre_delivery_Logic_object_loop'
                    for row in h.events), 'Unexpected service inside the selected Logic pass')
    observer = NativePcmObserver()
    observer.install(h.f, h.u, h.r)
    h.audio(1033)
    h.audio(1034)
    h.invoke('different_pending_while_playing', 0x708D90, h.actors[0], m.voice_sound_ids['GIFear'])
    h.invoke('same_pending_while_playing', 0x708D90, h.actors[1], m.voice_sound_ids['GIMove'])
    h.logic('original_Logic_holds_different_clears_same')
    for milliseconds, quarter in ((1060, 0), (1290, 1), (1545, 2), (1801, 3), (2056, 0),
                                   (2312, 1), (2568, 2), (2601, 2)):
        step = h.worker(milliseconds, quarter)
        require(not step['requests'] and not step['advances'],
                'Unexpected worker RNG draw in the selected natural end sequence')
    require(all(event['state'] == 4 for event in h.snapshot()['events']),
            'Original worker did not naturally finish all three sounds')
    h.logic('original_Logic_holds_until_pool_retirement')
    h.audio(2635)
    h.logic('original_Logic_dispatches_held_after_pool_retirement')
    h.audio(2669)
    # Explicit event delivery is later than the acknowledgement drain in this
    # bounded composition. It does not assert native whole-frame scheduling.
    for event in h.rings()['dolist']['events']:
        h.invoke('delivered_actual_DoList_' + str(event['slot']), 0x4C6CB0,
                 0x8B4204 + event['slot'] * 0x6F)
    h.region('post_delivery_AI_head_until_held_FindPath', 0x51BAB0, 0x4D3920,
             registers=((UC_X86_REG_ECX, h.actors[0]),), required=(0x6F9EBB,))
    # The bounded incomplete outer AI stack is not continued. Subsequent
    # controls start fresh original function calls and keep this limit visible.
    h.pending.clear()
    row = h.project()
    row['pcm_observation'] = {k: v for k, v in observer.state.items() if k != 'fixture'}
    row['bounds'] = ['Complete original pre-delivery Logic object loop; no audio service reached inside it.',
        'Actual G events remain pending until explicit original delivery. This is not ordinary event timing.',
        'Post-delivery Infantry AI reaches the voice head then stops before held Foot FindPath4D3920.',
        'Supplied16x16 map extent, inherited Cells/House/visibility and existing radar tracker presentation boundaries.']
    return row


def voice_controls(m):
 rows=[]
 for name in ('six_G_requests','destroy_pending_before_head','destroy_while_playing','load_clear_and_reset'):
  h=VoiceHistory(m,name,actor_count=6 if name=='six_G_requests' else 2)
  h.select(list(reversed(h.actors)))
  h.invoke('actual_G',0x536D00,0,0)
  if name=='destroy_pending_before_head':
   h.invoke('original_Foot_UnInit',0x4DE5D0,h.actors[0])
   h.invoke('original_deferred_destructor',0x725C70,0)
   h.logic('original_survivor_Logic')
   h.audio(1034)
  else:
   h.logic('original_Logic')
   h.audio(1034)
   if name=='destroy_while_playing':
    h.invoke('original_Foot_UnInit',0x4DE5D0,h.actors[0])
    h.invoke('original_deferred_destructor',0x725C70,0)
    h.audio(1068)
   if name=='load_clear_and_reset':
    h.invoke('held_pending',0x708D90,h.actors[0],m.voice_sound_ids['GIFear'])
    h.invoke('original_outer_load_pool_clear',0x406E80,0)
    for p in h.actors:
     h.region('original_TechnoLoad_voice_reset_'+str(p),0x70C21F,0x70C246,
       registers=((UC_X86_REG_ESI,p),(UC_X86_REG_EBX,0)),required=(0x405BE0,))
    h.audio(1068)
  rows.append(h.project())
 return rows


def theme_cases(m):
    rows = []
    def entry(**values):
        return dict(available=True, normal=True, repeat=False, side=-1, scenario=0, **values)
    # Each catalog below is a declared component input, not a retail Theme
    # reader claim. Constructor and both NextSong/IsAllowed bodies execute.
    specs = [
        dict(name='shuffle_five_seed1', seed=1, previous=-1, entries=[entry() for _ in range(5)]),
        dict(name='shuffle_reject_previous', seed=1, previous=1, entries=[entry() for _ in range(5)]),
        dict(name='shuffle_reject_unavailable', seed=1, previous=-1,
             entries=[entry() if i == 4 else {**entry(), 'available': False} for i in range(5)]),
        dict(name='global_repeat', seed=31, previous=1, repeat=True, entries=[entry(), entry()]),
        dict(name='entry_repeat', seed=31, previous=1, entries=[entry(), {**entry(), 'repeat': True}]),
        dict(name='sequential_skips', seed=31, previous=0, shuffle=False,
             entries=[entry(), {**entry(), 'normal': False}, entry()]),
        dict(name='shuffle_1000_disallowed', seed=1, previous=-1,
             entries=[{**entry(), 'available': False} for _ in range(5)]),
        dict(name='sequential_1000_not_needed', seed=1, previous=0, shuffle=False,
             entries=[{**entry(), 'available': False} for _ in range(3)]),
        dict(name='side_and_scenario_gates', seed=1, previous=-1, game_mode=0,
             current_side=1, scenario_number=3, entries=[{**entry(), 'side': 0},
                {**entry(), 'scenario': 4}, {**entry(), 'side': 1, 'scenario': 3}]),
    ]
    for spec in specs:
        h = VoiceHistory(m, spec['name'], seed=spec['seed'], actor_count=0)
        u, r = h.u, h.r
        typ = h.f.allocate(0x100)
        h.invoke('Theme_constructor', 0x720960, typ)
        items = h.f.allocate(len(spec['entries']) * 4)
        for index, values in enumerate(spec['entries']):
            p = h.f.allocate(0x300)
            u.mem_write(p + 0x280, dwords(values['scenario']))
            u.mem_write(p + 0x288, bytes([values['normal'], values['repeat'], values['available']]))
            u.mem_write(p + 0x28C, dwords(values['side']))
            u.mem_write(items + index * 4, dwords(p))
        shuffle, repeat = spec.get('shuffle', True), spec.get('repeat', False)
        u.mem_write(typ + 0x18, dwords(items, len(spec['entries']), 1, len(spec['entries'])))
        u.mem_write(typ + 0x10, bytes([repeat, 0, shuffle]))
        u.mem_write(0xA8B238, dwords(spec.get('game_mode', 5)))
        u.mem_write(r(er.HOUSE + 0x34) + 0xBC, dwords(spec.get('current_side', 0)))
        u.mem_write(r(0xA8B230) + 0x1254, dwords(spec.get('scenario_number', 1)))
        before = h.rng()
        ds, ads = len(h.f.draws), len(h.f.advances)
        returned = h.invoke('original_NextSong', 0x720A80, typ, spec['previous'], count=20_000_000)
        after = h.rng()
        draws, advances = copy.deepcopy(h.f.draws[ds:]), copy.deepcopy(h.f.advances[ads:])
        continuation = h.invoke('same_Main_raw_continuation', 0x65C780, 0x886B88)
        rows.append(dict(name=spec['name'], input=dict(seed=spec['seed'], previous=spec['previous'],
            shuffle=shuffle, repeat=repeat, entries=spec['entries'], game_mode=spec.get('game_mode', 5),
            current_side=spec.get('current_side', 0), scenario_number=spec.get('scenario_number', 1)),
            returned=returned, main_before=before['main'], main_after=after['main'],
            other_streams_before={k: v for k, v in before.items() if k != 'main'},
            other_streams_after={k: v for k, v in after.items() if k != 'main'},
            ranged_draws=draws, raw_draws=advances,
            continuation=dict(returned=continuation, main_after=h.rng()['main']),
            complete_rng_states=h.streams, original_code_unchanged=h.f.code_unchanged()))
    return rows


def feedback_cases(m):
    rows=[]
    specs=[
      dict(name='physical_singleton_accepted',seed=2),
      dict(name='percent29_accepted',seed=199),
      dict(name='percent30_rejected',seed=17),
      dict(name='empty_no_draw',seed=2,raw='AbsentFeedback'),
      dict(name='authored_ordered_duplicates',seed=2,raw='GIFear,GIMove,GIFear'),
      dict(name='mode5_nonhuman_current',seed=2,human=False,player_control=False),
      dict(name='mode5_not_current',seed=2,current=False),
      dict(name='mode0_noncontrolled',seed=2,mode=0,human=False,player_control=False),
      dict(name='mode0_player_control',seed=2,mode=0,human=False,player_control=True),
      dict(name='global_voice_disabled',seed=2,enabled=False),
    ]
    for spec in specs:
      binding=read_feedback(m,m.types['E1'],{'E1':{'VoiceFeedback':spec.get('raw','GIFear')}})
      h=VoiceHistory(m,'feedback_'+spec['name'],seed=spec['seed'],actor_count=1)
      u=h.u;p=h.actors[0]
      u.mem_write(0xA8B238,dwords(spec.get('mode',5)))
      u.mem_write(er.HOUSE+0x1EC,bytes([spec.get('human',True)]))
      u.mem_write(er.HOUSE+0x1ED,bytes([spec.get('player_control',True)]))
      u.mem_write(0xA83D4C,dwords(er.HOUSE if spec.get('current',True) else 0))
      u.mem_write(0x822CF2,bytes([spec.get('enabled',True)]))
      before=h.snapshot()
      row=h.region('original_VoiceFeedback_damage_result2_branch',0x702695,0x7027F7,
         registers=((UC_X86_REG_ESI,p),))
      after=h.snapshot()
      continuation=h.invoke('same_Main_raw_continuation',0x65C780,0x886B88)
      rows.append(dict(name=spec['name'],input=dict(seed=spec['seed'],
        voice_feedback=binding,
        game_mode=spec.get('mode',5),human=spec.get('human',True),
        player_control=spec.get('player_control',True),current_house_equal=spec.get('current',True),
        voice_enabled=spec.get('enabled',True),coordinate=before['actors'][0]['location']),
        before=before,after=after,requests=row['requests'],advances=row['advances'],
        ordered_calls=[{k:v for k,v in r.items() if k not in ('before','after','at_ret8')}
                       for r in h.events if r.get('phase')==row['label']],
        continuation=dict(returned=continuation,main_after=h.rng()['main']),
        complete_rng_states=h.streams,original_code_unchanged=h.f.code_unchanged()))
    read_feedback(m,m.types['E1'],{'E1':{'VoiceFeedback':'GIFear'}})
    return rows


def pool_controls(m):
    physical=sections((m.platform_audio.root/'SOUNDMD.INI').read_bytes())
    authored={
      'VOICE_PROBE_SHIFTS':{**physical['GIMove'],'FShift':'-5 5','VShift':'10'},
      'VOICE_PROBE_BUSY_LOW':{'Sounds':'$igimoa','Priority':'low','Limit':'32','Volume':'85'},
      'VOICE_PROBE_BUSY_HIGH':{'Sounds':'$igimoa','Priority':'high','Limit':'32','Volume':'85'},
    }
    m.make_ini({'Defaults':physical['Defaults'],'SoundList':{str(i):name for i,name in enumerate(authored)},**authored})
    m.invoke(0x7510D0,INI)
    ids={name:m.invoke(0x7514D0,m.cstring(name))for name in authored}
    rows=[]
    for name,filler in (('authored_shifts',None),('channel_rejected_by_high','VOICE_PROBE_BUSY_HIGH'),('channel_preempts_low','VOICE_PROBE_BUSY_LOW')):
        h=VoiceHistory(m,name,seed=31,actor_count=0)
        def play(label,index):
            h.u.reg_write(UC_X86_REG_EDX,2000)
            return h.invoke(label,0x750920,index,0x3F800000,0)
        if filler:
            for i in range(16):play('fill_channel_'+str(i),ids[filler])
            h.audio(1034,wall=180_000_000)
            require(any(e['state']==3 for e in h.snapshot()['events']),
                'Original channel filler did not start any event')
            play('queue_selected_GIMove',m.voice_sound_ids['GIMove'])
            h.audio(1068)
        else:
            play('queue_authored_shifts',ids['VOICE_PROBE_SHIFTS'])
            h.audio(1034,wall=180_000_000)
        row=h.project();row['authored_sections']=authored;row['authored_ids']=ids
        rows.append(row)
    return rows



def clock_controls(m):
    h = VoiceHistory(m, 'empty_service_64bit_clock', actor_count=0, initial_pump=False)
    require(h.snapshot()['pump_last_ms'] == 0, 'Initial native service timestamp differs')
    for milliseconds in (0, 33, 34, 67, 68, 0xFFFFFFDE, 0xFFFFFFFF,
                         0x100000000, 0x100000021, 0x100000022, 0x200000064):
        h.audio(milliseconds)
    return [h.project()]


def generate():
    root = Path(os.environ['VERA20K_UNIT_VOICE_INPUTS'])
    m, retail = prepare(root)
    files = [dict(name=p.name, bytes=p.stat().st_size, sha256=hashlib.sha256(p.read_bytes()).hexdigest())
             for p in sorted(root.iterdir()) if p.is_file()]
    return dict(schema_version=1, physical=files, retail=retail,
                rows=[core(m)], voice_controls=voice_controls(m),
                theme_cases=theme_cases(m), feedback_cases=feedback_cases(m),
                pool_controls=pool_controls(m), clock_controls=clock_controls(m))


def metadata():
    require(set(source_paths()) <= set(GUARDED), 'Native producer imported unguarded dependencies')
    result = provenance(scope=__doc__, assumptions=[
        'Reuse EngineerJoinedFixture and original audio index, channel/pool constructors, E1 constructor and Unlimbo. Selected physical type, General, Radiation and AudioVisual reader layers execute.',
        'Existing GIMoveHistory owns function invocation, native Event rings and local OutList transfer; event delivery is explicitly separate from the pre-delivery Logic pass.',
        'All three RandomClass states are retained. Numeric outputs and draw requests come only from original execution.',
        'Original six-G Limit5, actual Foot UnInit/deferred destructor and whole sound-pool clear controls retain live tagged handles. Only the TechnoLoad voice-reset suffix is composed after pool clear; whole save/load is excluded.',
        'Authored sound sections pass through the original registry and reader for bounded FShift/VShift and16-channel priority rejection/preemption controls; sample bytes remain physical GIMove assets.',
        'Empty service-clock controls start from the original zero timestamp and execute64-bit timestamp boundaries through the same original406F70 service.',
        'VoiceFeedback result2 branch input is supplied; exact original list/percent/House/positional request ordering executes before subsequent raw Main continuation.',
        'Theme cases supply authored initialized catalog fields to original constructor/NextSong/IsAllowed, not a Theme reader or whole music playback proof.'],
        substitutions=['Inherited supplied Cells/House/map prior and explicit16x16 outer map extent; no whole scenario load.',
            'Original Vox752290 and Speech752AD0 plus Theme constructor720960/ReadINI720590 initialize three shared stream reservations. ThemeControl is a declared empty catalog; whole physical music catalog loading is excluded.',
            'Canonical NativeAudioPlatform supplies immutable file and explicit single-thread Win32/DirectSound clock/device transport. No real hardware concurrency or audible-output claim.',
            'Existing Mission radar tracker presentation calls655560/655740 are boundaries.',
            'Post-delivery Infantry AI stops before held FindPath4D3920; complete post-G movement is excluded.'],
        entry_points={'G': 0x536D00, 'selection': 0x730D60, 'clicked_mission': 0x6FFBE0,
            'queue_voice': 0x708D90, 'logic_loop': 0x55B5FF, 'techno_voice_head': 0x6F9EBB,
            'vox_init': 0x752290, 'speech_init': 0x752AD0,
            'theme_constructor': 0x720960, 'theme_read_ini': 0x720590,
            'stream_constructor': 0x407860, 'reserve_channel': 0x4036C0,
            'assign_channel_group': 0x402440,
            'service': 0x406F70, 'pool': 0x4041D0, 'worker': 0x4095B0,
            'cached_sample_load': 0x401C00, 'foot_uninit': 0x4DE5D0,
            'deferred_destructor': 0x725C70, 'techno_destructor': 0x6F4500,
            'clear_pool': 0x406E80, 'techno_load_voice_reset': 0x70C21F,
            'voice_feedback_reader': 0x712D99, 'voice_feedback': 0x702695,
            'theme_next_song': 0x720A80, 'theme_allowed': 0x721140})
    result['command'] = 'python -m tools.input_oracle.unit_voice_playback --check'
    return result


if __name__ == '__main__':
    GUARDED = source_paths()
    finish_vectors(generate, HERE / 'unit_voice_playback.json', provenance=metadata,
                   source_paths=GUARDED)
