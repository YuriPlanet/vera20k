"""Original GI order acknowledgements on the existing joined audio fixture.

G/selection/event transport, object AI, the audio pool, codecs and worker execute
original instructions. Explicit map/House and single-thread OS inputs bound the
comparison; the post-delivery movement route stops before held FindPath work.
"""
# --slave-miner-pack-voice selects a separate small request comparison, with
# VERA20K_SLAVE_PACK_INPUTS and VERA20K_SLAVE_PACK_MAP physical inputs. The
# original module docstring is retained because the GI sidecar records it.
from collections import deque
from pathlib import Path
from types import SimpleNamespace
import copy
import hashlib
import os
import struct
import sys

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import *
from tools import native_inspect as inspect
from tools.native_oracle import (RET_MAGIC, SCRATCH, call as native_call,
                                finish_vectors, image_bytes, provenance, run_checked)
from tools.rules_oracle.bridge_anim_inputs import Reader
from tools.spatial_oracle.anytown_damage.mtnk_attack import sound_inputs
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

    def __init__(self, m, name, *, seed=31, actor_count=2, initial_pump=True,
                 fixture=None, coordinate_profile=False):
        require(fixture is None or actor_count == 0,
                'An attached native world must retain its existing actors')
        f = fixture if fixture is not None else er.EngineerJoinedFixture(m, seed=seed, arena_size=0x400000)
        self.coordinate_profile, self.compact_observation = coordinate_profile, False
        self.bulk_observation = False
        self.coordinate_writes, self.coordinate_observation_limits = [], []
        if coordinate_profile:
            self.WATCH = dict(self.WATCH)
            self.WATCH.update({0x750E20: ('CoordinateRequest', 4),
                0x750EC0: ('CoordinateService', 0), 0x750FA0: ('CoordinateClear', 0),
                0x751040: ('CoordinateSuspend', 0), 0x751080: ('CoordinateResume', 0),
                0x750D40: ('RedrivePosition', 0), 0x750AC0: ('SpatialGain', 4),
                0x6D2140: ('CoordsToClient', 8), 0x405190: ('AllocateEvent', 0),
                0x4052F0: ('StopEvent', 0), 0x4053E0: ('SuspendEvent', 0),
                0x405430: ('ResumeEvent', 0), 0x404FD0: ('SuspendEvents', 0),
                0x405040: ('ResumeEvents', 0), 0x4061D0: ('PrepareVolume', 0),
                0x406270: ('PreparePan', 0), 0x4060F0: ('BindHandle', 4),
                0x442230: ('BuildingReceiver', 28), 0x701900: ('TechnoReceiver', 28),
                0x5F5390: ('ObjectReceiver', 28), 0x741340: ('UnitFireAt', 8),
                0x489180: ('WarheadDamageKernel', 4),
                0x405BE0: ('HandleConstructor', 0), 0x7518C0: ('AudioEnabled', 0),
                0x406F00: ('GamePause', 0), 0x406F40: ('GameResume', 0),
                0x43FB20: ('BuildingAI', 0), 0x55B610: ('LogicVisit', 0)})
        self.f, self.u, self.r, self.bc = f, f.u, f.read32, bc
        self.existing_transport_entries = getattr(f, 'transport_entries', frozenset())
        self.require, self.copy, self.hashlib = require, copy, hashlib
        self.unit, self.name = False, name
        self.sequence, self.phase = 0, 'created'
        self.events, self.writes, self.pending, self.boundaries = [], [], [], []
        self.inputs, self.streams, self.recent = [], {}, deque(maxlen=80)
        self.draw_start, self.advance_start = len(f.draws), len(f.advances)
        # Inherited physical Cell grid; declared nonzero outer map dimensions
        # prevent original568350 treating every idle GI as off-map.
        if fixture is None:
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
        if fixture is None:
            surface = f.allocate(0x40)
            self.u.mem_write(surface, dwords(0x7E2070, 640, 480, 0, 2, 0, 0, 0))
            self.u.mem_write(0x880A04, dwords(surface))
            self.u.mem_write(0xA8B238, dwords(5))
            self.u.mem_write(0xAC4CF4, b'\0')
            self.u.mem_write(0x822CF2, b'\1')
        self.hooks = [self.u.hook_add(UC_HOOK_CODE, self.observe)]
        if coordinate_profile:
            self.hooks.append(self.u.hook_add(UC_HOOK_MEM_WRITE, self.coordinate_written))
        self.clock = dict(frequency=1000000, counter=1000000)
        calls = inspect.decode_ranges(inspect.selected_ranges(image_bytes(), 0x407550, 0xE00,
            code_only=True), lambda ins: [inspect.instruction_row(ins)])['matches']
        critical = {row['address']: row for row in calls if row['mnemonic'] == 'call' and
            row['operands'] in ('dword ptr [0x7e11e8]', 'dword ptr [0x7e11ec]', 'dword ptr [0x7e11f4]')}
        f.platform_audio.configure_transport(clock=self.clock, critical_calls=critical)
        if coordinate_profile:
            device_template = dict(value=1, play_cursor=0, write_cursor=0)
            f.platform_audio.configure_transport(new_buffer_device=device_template)
            self.inputs.append(dict(kind='declared_new_buffer_OS_input_template',
                device=device_template,
                limits='Explicit device consumer inputs, not physical playback progression. Original Stop alone clears status.'))
            # Original7510D0 owns the <=200 request-pool allocation as well as
            # registry reading. The inherited world copies the existing sound
            # registry, but deliberately has no request allocator. An empty
            # original INI pass initializes it without replacing that registry.
            empty = f.allocate(0x40)
            self.u.mem_write(empty, bytes(m.u.mem_read(m.voice_theme_ini, 0x40)))
            count = self.r(0xB1D388)
            bindings = [(self.r(self.r(self.r(0xB1D37C) + i * 4)),
                         bytes(self.u.mem_read(self.r(self.r(
                             self.r(0xB1D37C) + i * 4)), 0x138))) for i in range(count)]
            self.inputs.append(dict(kind='declared_empty_sound_catalog', sections={}))
            self.invoke('original_empty_catalog_coordinate_pool_init', 0x7510D0, empty)
            require(self.r(0xB1D3A4) != 0 and self.r(0xB1D388) == count,
                    'Original request-pool initialization changed the registry count')
            require(bindings == [(self.r(self.r(self.r(0xB1D37C) + i * 4)),
                                  bytes(self.u.mem_read(self.r(self.r(
                                      self.r(0xB1D37C) + i * 4)), 0x138)))
                                 for i in range(count)],
                    'Empty sound reader changed a retained native binding')
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

    def finish_returns(self, pc, sp):
        pending = tuple(self.pending) if self.coordinate_profile else ()
        super().finish_returns(pc, sp)
        for row in pending:
            if row.get('packet_pointer') and 'returned_eax' in row:
                row.update(packet_after=signed(self.u, row['packet_pointer']),
                           health_after=signed(self.u, row['this'] + 0x6C))

    def observe(self, u, pc, size, data):
        if pc in (0x655560, 0x655740):
            # Existing selected radar tracker presentation boundary, not a
            # second implementation or a gameplay return substitution.
            if pc not in self.existing_transport_entries:
                Mission.observe(SimpleNamespace(trace=self.recent, m=self.f, pending={},
                    events=self.events, frame=self.r(bc.FRAME), phase=self.phase), u, pc, size, data)
            return
        self.compact_observation = self.coordinate_profile
        try:
            super().observe(u, pc, size, data)
        finally:
            self.compact_observation = False
        if pc in (0x6F9EBB, 0x6F9F0D, 0x40971A, 0x40974F):
            self.events.append(dict(sequence=self.next_sequence(), kind='native_site', pc=pc,
                phase=self.phase, this=u.reg_read(UC_X86_REG_ESI), state=self.snapshot()))
        if pc in (0x4036C0, 0x402440):
            self.events[-1]['requested_group'] = u.reg_read(UC_X86_REG_EDX)
        if pc == 0x7509E0:
            self.events[-1]['coordinate'] = self.ints(u.reg_read(UC_X86_REG_EDX), 3)
        if not self.coordinate_profile:
            return
        if pc in (0x442230, 0x701900, 0x5F5390):
            row = self.events[-1]
            row.update(packet_pointer=row['args'][0],
                packet_before=signed(u, row['args'][0]),
                health_before=signed(u, row['this'] + 0x6C),
                ignore_defenses=bool(row['args'][4]))
        if pc == 0x489180:
            row = self.events[-1]
            row.update(raw_damage=struct.unpack('<i', dwords(row['this']))[0],
                warhead=u.reg_read(UC_X86_REG_EDX), armor=row['args'][0])
        if pc in (0x7019E3, 0x5F5414, 0x5F5416):
            packet = u.reg_read(UC_X86_REG_EBX if pc == 0x7019E3 else UC_X86_REG_EDI)
            receiver = u.reg_read(UC_X86_REG_ESI)
            self.events.append(dict(sequence=self.next_sequence(), kind='receiver_packet_site',
                pc=pc, phase=self.phase, packet_pointer=packet, packet=signed(u, packet),
                receiver=receiver, health=signed(u, receiver + 0x6C),
                eax_signed=struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0]))
        if pc in (0x750E20, 0x750D40, 0x750AC0):
            row = self.events[-1]
            coordinate = u.reg_read(UC_X86_REG_EDX if pc == 0x750E20 else UC_X86_REG_ECX)
            row.update(coordinate=self.ints(coordinate, 3),
                register_ecx=u.reg_read(UC_X86_REG_ECX), register_edx=u.reg_read(UC_X86_REG_EDX))
        if pc == 0x6D2140:
            row = self.events[-1]
            row.update(coordinate=self.ints(row['args'][0], 3), output_pointer=row['args'][1],
                       listener=self.coordinate_listener())
        if pc in (0x6D21CE, 0x6D21F7, 0x6D2202, 0x6D221E,
                  0x750BBE, 0x750BCF, 0x750BED, 0x750D29):
            sp = u.reg_read(UC_X86_REG_ESP)
            row = dict(sequence=self.next_sequence(), kind='native_projection_site', pc=pc,
                phase=self.phase, stack_hex=bytes(u.mem_read(sp, 0x40)).hex(),
                eax=u.reg_read(UC_X86_REG_EAX), ecx=u.reg_read(UC_X86_REG_ECX),
                esi=u.reg_read(UC_X86_REG_ESI), edi=u.reg_read(UC_X86_REG_EDI))
            if pc == 0x750D29:
                row.update(gain_f32_bits=bytes(u.mem_read(sp + 0x18, 4)).hex(),
                           pan=u.reg_read(UC_X86_REG_EAX))
            if pc in (0x6D2202, 0x6D221E):
                row['output'] = self.ints(u.reg_read(UC_X86_REG_EAX), 2)
            self.events.append(row)

    def coordinate_listener(self):
        r, u = self.r, self.u
        tact, surface = r(0x887324), r(0x880A04)
        return dict(tactical=tact, view_xy=self.ints(tact + 0xB0, 2),
            viewport_886fa8_fac=self.ints(0x886FA8, 2),
            backbuffer_b0ce30_34=self.ints(0xB0CE30, 2), surface=surface,
            surface_words=self.ints(surface, 8) if surface else [],
            fpcw=u.reg_read(UC_X86_REG_FPCW),
            constants={hex(p): bytes(u.mem_read(p, n)).hex() for p, n in
                ((0xB0CD48, 8), (0x7E1738, 8), (0x7E5168, 4), (0x7F68E8, 4),
                 (0x7EF38C, 4), (0x7E8AE8, 8), (0x7E1748, 4))})

    def projection_controls(self):
        """Declared listener/Coord inputs through whole original6D2140."""
        require(self.coordinate_profile, 'Projection readbacks require the existing coordinate observer')
        initial = self.coordinate_listener()
        tact, surface = initial['tactical'], initial['surface']
        coordinate, output = self.f.allocate(12), self.f.allocate(8)
        controls = []
        coordinates = ((0, 0, 0), (128, 128, 0), (256, 256, 0),
            (-1, -1, -1), (-255, 1, -1), (1, -255, 1),
            (15232, 28032, 104), (0, 0, 727), (0, 0, 728), (0, 0, 729),
            (128, -256, -728), (65536, -32768, 1024))
        for label, viewport, backbuffer, view in (
            ('declared_zoom1_origin', (640, 480), (640, 480), (0, 0)),
            ('declared_production_frame_mapping', (640, 600), (800, 600), (-1726, 2371)),
        ):
            self.u.mem_write(0x886FA8, dwords(*viewport))
            self.u.mem_write(0xB0CE30, dwords(*backbuffer))
            self.u.mem_write(surface + 4, dwords(*backbuffer))
            self.u.mem_write(tact + 0xB0, dwords(*view))
            self.inputs.append(dict(kind='declared_projection_listener', label=label,
                viewport=list(viewport), backbuffer=list(backbuffer), view=list(view),
                limits='Consumer inputs only; native scenario/camera producer is not executed. VERA worldY and cameraY are native tacticalY+15.'))
            for values in coordinates:
                self.u.mem_write(coordinate, dwords(*values))
                before, start = self.coordinate_listener(), len(self.events)
                returned = self.invoke(label + '_' + '_'.join(map(str, values)),
                    0x6D2140, tact, coordinate, output)
                controls.append(dict(input=dict(coordinate=list(values), listener=before),
                    returned_eax=returned, output=self.ints(output, 2),
                    intermediates=[row for row in self.events[start:]
                                   if row.get('kind') == 'native_projection_site']))
        self.u.mem_write(0x886FA8, dwords(*initial['viewport_886fa8_fac']))
        self.u.mem_write(0xB0CE30, dwords(*initial['backbuffer_b0ce30_34']))
        self.u.mem_write(surface + 4, dwords(*initial['surface_words'][1:3]))
        self.u.mem_write(tact + 0xB0, dwords(*initial['view_xy']))
        return controls

    def listener_input(self, label, *, viewport=(640, 600), backbuffer=(800, 600),
                       view=(-1726, 2371)):
        """Explicit consumer input; never a native camera-producer claim."""
        tactical, surface = self.r(0x887324), self.r(0x880A04)
        self.u.mem_write(0x886FA8, dwords(*viewport))
        self.u.mem_write(0xB0CE30, dwords(*backbuffer))
        self.u.mem_write(surface + 4, dwords(*backbuffer))
        self.u.mem_write(tactical + 0xB0, dwords(*view))
        self.inputs.append(dict(kind='declared_projection_listener', label=label,
            viewport=list(viewport), backbuffer=list(backbuffer), view=list(view),
            limits='Original projection consumer only; scenario/camera producer excluded.'))

    def natural_coordinate_end(self, *, first_ms=1034, reap=True):
        """Existing pump/worker/PCM owners, with explicit OS clock/cursor inputs."""
        observer = NativePcmObserver(raw_pcm=True)
        observer.install(self.f, self.u, self.r)
        self.audio(first_ms - 1)
        self.audio(first_ms)
        milliseconds = first_ms + 26
        for step in range(16):
            self.worker(milliseconds, step % 4)
            if all(event['state'] == 4 or event['flags'] & 1
                   for event in self.snapshot()['events']):
                break
            milliseconds += 256
        else:
            raise AssertionError('Selected PCM requests did not naturally end in16worker iterations')
        ended = self.snapshot()
        reaped = None
        if reap:
            self.audio(milliseconds + 34)
            reaped = self.snapshot()
            require(reaped['coordinate_request_count'] == 0,
                    'Naturally finished retained-coordinate requests were not reaped')
        return dict(ended_before_main=ended, after_main_reap=reaped, last_worker_ms=milliseconds,
            pcm_observation={k: v for k, v in observer.state.items() if k != 'fixture'},
            all_native_rng_requests=self.f.draws, all_raw_advances=self.f.advances)

    def audio_core_controls(self, index, other_index):
        """Whole original core/request controls on this one joined audio world."""
        require(self.coordinate_profile, 'Core controls need the existing coordinate observer')
        coordinate = self.f.allocate(12)
        self.u.mem_write(coordinate, dwords(15232, 28032, 104))
        self.listener_input('declared_production_API_coordinate')
        handles = [self.f.allocate(16) for _ in range(3)]
        for handle in handles:
            self.invoke('original_handle_constructor_' + str(handle), 0x405BE0, handle)
        rows = []
        def play(label, entry, sound, handle=0, *, pan=0):
            self.u.reg_write(UC_X86_REG_EDX, coordinate if entry == 0x7509E0 else pan)
            returned = self.invoke(label, entry, sound,
                *((handle,) if entry == 0x7509E0 else (0x3F800000, handle)))
            rows.append(dict(label=label, entry=entry, sound_index=sound, returned_event=returned,
                handle_pointer=handle, handle=self.ints(handle, 4) if handle else [],
                state=self.snapshot()))
            return returned
        def request(label, sound=index):
            self.u.reg_write(UC_X86_REG_EDX, coordinate)
            returned = self.invoke(label, 0x750E20, sound, 0)
            rows.append(dict(label=label, entry=0x750E20, sound_index=sound,
                caller_argument=0, returned_event=returned, state=self.snapshot()))
            return returned
        play('position_first', 0x7509E0, index, handles[0])
        play('position_same_entry_reuses', 0x7509E0, index, handles[0])
        play('position_different_entry_stops_old', 0x7509E0, other_index, handles[0])
        play('position_negative_id_unbinds', 0x7509E0, 0xFFFFFFFF, handles[0])
        play('position_out_of_range_id_unbinds', 0x7509E0, self.r(0xB1D388), handles[0])
        play('position_valid_after_invalid', 0x7509E0, index, handles[0])
        self.invoke('original_disable_audio', 0x7518C0, 0)
        play('disabled_position_retains_handle', 0x7509E0, other_index, handles[0])
        play('disabled_nonpositional_retains_handle', 0x750920, other_index, handles[0])
        self.invoke('original_enable_audio', 0x7518C0, 1)
        self.listener_input('declared_inaudible_view', view=(10000, 10000))
        play('position_inaudible_stops_retains_entry', 0x7509E0, index, handles[0])
        request('negative_request_no_node', 0xFFFFFFFF)
        request('out_of_range_request_node_no_event', self.r(0xB1D388))
        request('inaudible_request_node_no_event')
        self.audio(1034)
        rows.append(dict(label='invalid_inaudible_nodes_reaped', state=self.snapshot()))
        # Both callers reach shared405190. No pool state or event result is
        # supplied: whole originals create300 queued events before the retry.
        self.bulk_observation = True
        try:
            fill = [play('full_pool_nonpositional_' + str(i), 0x750920, index)
                    for i in range(300)]
        finally:
            self.bulk_observation = False
        rows.append(dict(label='native300event_pool_full', returns=fill, state=self.snapshot()))
        self.listener_input('declared_audible_pool_retry_view')
        play('full_pool_position_bind', 0x7509E0, index, handles[1])
        play('full_pool_nonpositional_bind', 0x750920, index, handles[2])
        require(self.r(0x87E28C) == 300 and all(fill), 'Native event pool did not fill')
        self.invoke('original_stop_for_position_retry', 0x4052F0, fill[0])
        play('dead_only_position_retry', 0x7509E0, index, handles[1])
        self.invoke('original_stop_for_nonpositional_retry', 0x4052F0, fill[1])
        play('dead_only_nonpositional_retry', 0x750920, index, handles[2])
        self.invoke('original_clear_event_pool', 0x406E80, 0)
        # Request-depth and event-depth belong to their existing separate
        # native owners. Whole GamePause adds both layers in original order.
        request('coordinate_before_nested_pause')
        self.invoke('whole_game_pause_first', 0x406F00, 0)
        rows.append(dict(label='after_game_pause_first', state=self.snapshot()))
        self.invoke('whole_game_pause_second', 0x406F00, 0)
        rows.append(dict(label='after_game_pause_second', state=self.snapshot()))
        request('new_request_during_nested_pause_depth_zero')
        self.listener_input('declared_view_during_suspend', view=(-2126, 2371))
        self.audio(1068)
        rows.append(dict(label='suspended_request_service_and_new_request_redrive', state=self.snapshot()))
        self.invoke('whole_game_resume_first', 0x406F40, 0)
        rows.append(dict(label='after_game_resume_first', state=self.snapshot()))
        self.invoke('whole_game_resume_second', 0x406F40, 0)
        rows.append(dict(label='after_game_resume_second', state=self.snapshot()))
        self.invoke('whole_game_resume_extra_clamps', 0x406F40, 0)
        rows.append(dict(label='after_game_resume_extra', state=self.snapshot()))
        self.invoke('original_request_clear_while_live', 0x750FA0, 0)
        self.audio(1102)
        rows.append(dict(label='clear_requests_and_dead_event_reap', state=self.snapshot()))
        self.listener_input('declared_natural_end_view')
        request('coordinate_natural_end')
        natural = self.natural_coordinate_end(first_ms=1136, reap=False)
        ended_events = self.snapshot()['events']
        require(len(ended_events) == 1 and ended_events[0]['state'] == 4
                and not ended_events[0]['flags'] & 1, 'Natural state4 control is not merely non-DEAD')
        self.bulk_observation = True
        try:
            state4_fill = [play('state4_pool_nonpositional_' + str(i), 0x750920, index)
                           for i in range(299)]
        finally:
            self.bulk_observation = False
        rows.append(dict(label='state4_plus299_pool_full', returns=state4_fill, state=self.snapshot()))
        play('state4_is_not_DEAD_position_fails', 0x7509E0, index, handles[1])
        play('state4_is_not_DEAD_nonpositional_fails', 0x750920, index, handles[2])
        self.invoke('original_stop_naturally_ended_event', 0x4052F0, ended_events[0]['pointer'])
        play('state4_now_DEAD_position_retry', 0x7509E0, index, handles[1])
        self.invoke('original_request_clear_after_state4_retry', 0x750FA0, 0)
        self.invoke('original_event_clear_after_state4_retry', 0x406E80, 0)
        self.audio(natural['last_worker_ms'] + 34)
        rows.append(dict(label='after_natural_end_and_reap', state=self.snapshot()))
        self.listener_input('declared_inaudible_request_capacity', view=(10000, 10000))
        self.bulk_observation = True
        try:
            capacity = [request('inaudible_request_capacity_' + str(i)) for i in range(201)]
        finally:
            self.bulk_observation = False
        rows.append(dict(label='native200request_pool_full', returns=capacity, state=self.snapshot()))
        self.audio(3500)
        rows.append(dict(label='inaudible200requests_reaped', state=self.snapshot()))
        self.listener_input('declared_three_request_limit2')
        for i in range(3):
            request('registered_Limit2_request_' + str(i))
        self.audio(3534)
        rows.append(dict(label='registered_Limit2_service', state=self.snapshot()))
        for ms, view in ((3568, (-2126, 2371)), (3602, (-2526, 2371)),
                         (3636, (10000, 10000))):
            self.listener_input('declared_camera_redrive_' + str(ms), view=view)
            self.audio(ms)
            rows.append(dict(label='native_camera_redrive_' + str(ms), state=self.snapshot()))
        self.listener_input('declared_camera_return_after_inaudible_stop')
        self.audio(3670)
        rows.append(dict(label='stopped_one_shot_stays_reaped_after_view_returns', state=self.snapshot()))
        require(not self.pending and not self.f.pending, 'Unreturned native audio control')
        return dict(input=dict(sound_index=index, other_sound_index=other_index,
            coordinate=[15232, 28032, 104], caller_argument=0), controls=rows,
            natural_end=natural, history=self.project())

    def coordinate_written(self, u, _access, address, size, value, _data):
        # Observation only, scoped to the selected existing world and native
        # request/event owners. No timer, RNG or lifecycle state is supplied.
        ranges = [(er.BLD, er.BLD + 0x800, 'building')]
        if getattr(self.f, 'source', 0):
            ranges.append((self.f.source, self.f.source + 0x1000, 'source'))
        node = self.r(0xB1D320)
        seen = set()
        while node and node != self.r(node + 8):
            if node in seen or node == self.r(node) or len(seen) >= 200:
                self.coordinate_observation_limits.append(dict(
                    kind='partial_coordinate_request_list',
                    pc=u.reg_read(UC_X86_REG_EIP), phase=self.phase,
                    address=address, node=node, traversed_nodes=sorted(seen),
                    reported_count=self.r(0xB1D3A8),
                    reason='cycle/self-link or bound during native memory-write observation'))
                break
            seen.add(node)
            ranges.append((node, node + 0x3C, 'coordinate_request'))
            node = self.r(node)
        node = self.r(0x87E180)
        seen = set()
        while node and node != 0x87E180:
            if node in seen or len(seen) >= 300:
                self.coordinate_observation_limits.append(dict(
                    kind='partial_event_list', pc=u.reg_read(UC_X86_REG_EIP),
                    phase=self.phase, address=address, node=node,
                    traversed_nodes=sorted(seen), reported_count=self.r(0x87E28C),
                    reason='cycle or bound during native memory-write observation'))
                break
            seen.add(node)
            ranges.append((node + 0x138, node + 0x15C, 'event_clock_pause'))
            node = self.r(node)
        owners = [name for lo, hi, name in ranges if address < hi and lo < address + size]
        if owners:
            self.coordinate_writes.append(dict(pc=u.reg_read(UC_X86_REG_EIP), phase=self.phase,
                frame=self.r(bc.FRAME), address=address, size=size, value=value,
                before=bytes(u.mem_read(address, size)).hex(), owners=owners))

    def snapshot(self):
        u, r = self.u, self.r
        if self.compact_observation or self.bulk_observation:
            return dict(rng=self.rng(), event_count=r(0x87E28C),
                        coordinate_request_count=r(0xB1D3A8))
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
            if self.coordinate_profile:
                event.update(suspend_depth=r(p + 0x158),
                    volume_controller_words=self.ints(p + 0xB8, 3),
                    pan_controller_words=self.ints(p + 0x108, 3),
                    deadline_140_144=list(struct.unpack('<2I', u.mem_read(p + 0x140, 8))))
        if self.coordinate_profile:
            result.update(listener=self.coordinate_listener(), coordinate_requests=[],
                coordinate_request_count=r(0xB1D3A8), coordinate_request_high_water=r(0xB1D3AC),
                building=dict(health=signed(u, er.BLD + 0x6C), sampled_health=signed(u, er.BLD + 0x544),
                    native_id=r(er.BLD + 0x10), location=self.ints(er.BLD + 0x9C, 3)))
            node = r(0xB1D320)
            while node and node != r(node + 8):
                require(len(result['coordinate_requests']) < 200, 'Unexpected request census')
                result['coordinate_requests'].append(dict(pointer=node, handle=self.ints(node + 0xC, 4),
                    location=self.ints(node + 0x20, 3), caller_argument=r(node + 0x2C),
                    suspend_depth=r(node + 0x30)))
                node = r(node)
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

    def audio(self, milliseconds, *, wall=None):
        if wall is None:
            wall = 180_000_000 if self.coordinate_profile else 60_000_000
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
            self.devices = (platform.os_buffer_devices if self.coordinate_profile
                            and platform.os_buffer_devices is not None else {})
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
                    timeout_us=180_000_000 if self.coordinate_profile else 60_000_000,
                    required_addresses=[0x4095F5])
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
        result = dict(name=self.name, initial=self.initial, inputs=self.inputs,
            boundaries=self.boundaries, ordered_calls=calls, final=self.snapshot(),
            complete_rng_states=self.streams, original_code_unchanged=True)
        if self.coordinate_profile:
            result['native_coordinate_writes'] = self.coordinate_writes
            result['coordinate_observation_limits'] = self.coordinate_observation_limits
            platform = self.f.platform_audio
            result['platform_transport'] = dict(
                buffer_bindings=[dict(buffer=p, data=v[0], bytes=v[1])
                                 for p, v in sorted(platform.buffers.items())],
                final_OS_devices=copy.deepcopy(platform.os_buffer_devices),
                clock_calls=copy.deepcopy(platform.clock_calls),
                file_io=copy.deepcopy(platform.file_io),
                device_io=copy.deepcopy(platform.device_io))
        return result


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


def slave_pack_voice_inputs():
    """Reuse the selected Sound/INI owner for original scalar reference reads."""
    root = Path(os.environ['VERA20K_SLAVE_PACK_INPUTS'])
    map_path = Path(os.environ['VERA20K_SLAVE_PACK_MAP'])
    m = Reader(root, {})
    physical = sections((root / 'RULESMD.INI').read_bytes())['YAREFN']
    wanted = {physical[key] for key in ('VoiceDeploy', 'VoiceHarvest', 'DeploySound')}
    registry = sound_inputs(m, root, {}, wanted_names=wanted)
    typ = m.alloc(0x2000)
    m.invoke(0x45DD90, typ, (m.cstring('YAREFN'),))

    def binding():
        result = {key: signed(m.u, typ + offset) for key, offset in (
            ('deploy_sound', 0x56C), ('voice_harvest', 0x57C),
            ('voice_deploy', 0x590), ('voice_undeploy', 0x594))}
        for key, index in tuple(result.items()):
            result[key + '_name'] = sound_name(m, index)
        result.update(resource_gatherer=bool(m.u.mem_read(typ + 0x5EC, 1)[0]),
                      resource_destination=bool(m.u.mem_read(typ + 0x5ED, 1)[0]))
        return result

    constructor = binding()

    def read(values):
        m.make_ini({'YAREFN': values} if values else {})
        # Adjacent single-reference readers share pushes and delayed stores.
        # Each complete balanced block retains the preceding slot's value.
        for begin, end, preceding in (
            (0x713553, 0x7135CB, None),
            (0x71363D, 0x7136C1, 0x578),
            (0x713769, 0x713823, 0x58C),
            (0x7143D0, 0x714404, None),
        ):
            for register, value in (
                (UC_X86_REG_ESP, SP), (UC_X86_REG_EBP, typ),
                (UC_X86_REG_EBX, typ + 0x24), (UC_X86_REG_ESI, INI),
            ):
                m.u.reg_write(register, value)
            if preceding is not None:
                m.u.reg_write(UC_X86_REG_EAX, m.read32(typ + preceding))
            run_checked(m.u, begin, end, required_addresses=(
                0x5295F0 if begin == 0x7143D0 else 0x528A10,))
            require(m.u.reg_read(UC_X86_REG_ESP) == SP,
                    'Pack voice reader block changed its stack boundary')
        return binding()

    layers = []
    for filename, path in [(name, root / name) for name in (
        'RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini',
    )] + [(map_path.name, map_path)]:
        if not path.exists():
            require(filename == 'LANGRULE.INI', 'Missing physical input: ' + str(path))
            layers.append(dict(file=filename, absent=True))
            continue
        raw = path.read_bytes()
        values = sections(raw).get('YAREFN', {})
        layers.append(dict(file=filename, bytes=len(raw),
            sha256=hashlib.sha256(raw).hexdigest(),
            keys={key: value for key, value in values.items() if key in (
                'VoiceDeploy', 'VoiceUndeploy', 'VoiceHarvest', 'DeploySound',
                'ResourceGatherer', 'ResourceDestination', 'UndeploysInto')},
            after=read(values)))
    stock = binding()
    controls = []
    for name, values in (
        ('missing', {}), ('invalid', {'VoiceDeploy': 'AbsentSlaveVoice'}),
        ('empty', {'VoiceDeploy': ''}),
        ('mixed_sound_case_changes_binding', {'VoiceDeploy': 'sLaVeMiNeRhArVeSt'}),
        ('wrong_key_case_retains_binding', {'voicedeploy': physical['VoiceDeploy']}),
        ('invalid_after_replacement', {'VoiceDeploy': 'AbsentSlaveVoice'}),
        ('restore_stock', {'VoiceDeploy': physical['VoiceDeploy']}),
        ('undeploy_valid', {'VoiceUndeploy': physical['VoiceHarvest']}),
        ('undeploy_missing_retains_binding', {}),
        ('undeploy_invalid_retains_binding', {'VoiceUndeploy': 'AbsentSlaveVoice'}),
        ('undeploy_wrong_key_case_retains_binding', {'voiceundeploy': physical['VoiceDeploy']}),
        ('undeploy_mixed_sound_case_changes_binding', {'VoiceUndeploy': 'sLaVeMiNeRuNdEpLoYvOiCe'}),
    ):
        before = binding()
        controls.append(dict(name=name, keys=values, before=before, after=read(values)))
    read(physical)
    return m, typ, dict(type_constructor=constructor, sound_registry=registry,
                        retail_layers=layers, reader_controls=controls), stock


def slave_pack_voice_controls(stock):
    """Complete original voice bodies in the shared fresh-call runner."""
    actor, typ, house, cell = (SCRATCH + offset for offset in (0x1000, 0x3000, 0x6000, 0xD000))
    names = {stock[key]: stock[key + '_name'] for key in (
        'voice_deploy', 'voice_harvest', 'deploy_sound')}
    cases = (
        dict(name='ordinary_without_archive'),
        dict(name='ordinary_clear_cell', archive=True),
        dict(name='resource_override', archive=True, resource=True),
        dict(name='ordinary_suppressed', suppressed=True),
        dict(name='resource_override_keeps_suppression', archive=True, resource=True, suppressed=True),
        dict(name='resource_missing_harvest_falls_back', archive=True, resource=True, harvest=-1),
        dict(name='resource_missing_harvest_consumes_suppression', archive=True, resource=True, harvest=-1, suppressed=True),
        dict(name='deploy_slot_missing', deploy=-1),
        dict(name='queue_disabled', enabled=False),
        dict(name='non_local_house', local=False),
        dict(name='resource_not_destination', archive=True, resource=True, destination=False),
        dict(name='resource_not_gatherer', archive=True, resource=True, gatherer=False),
        dict(name='suppressed_retains_previous_pending', suppressed=True, pending=stock['deploy_sound']),
        dict(name='ordinary_replaces_previous_pending', pending=stock['deploy_sound']),
    )
    rows = []
    for case in cases:
        entity, kind, owner, archived = bytearray(0x800), bytearray(0x2000), bytes(0x6000), bytearray(0x200)
        for offset, value in ((0, 0x7E3EBC), (0x520, typ), (0x21C, house),
                              (0x218, cell if case.get('archive') else 0)):
            struct.pack_into('<I', entity, offset, value)
        struct.pack_into('<i', entity, 0x4F0, case.get('pending', -1))
        struct.pack_into('<i', entity, 0x4F4, -1)
        entity[0x4F8] = case.get('suppressed', False)
        struct.pack_into('<i', kind, 0x590, case.get('deploy', stock['voice_deploy']))
        struct.pack_into('<i', kind, 0x57C, case.get('harvest', stock['voice_harvest']))
        kind[0x5EC] = case.get('gatherer', stock['resource_gatherer'])
        kind[0x5ED] = case.get('destination', stock['resource_destination'])
        struct.pack_into('<I', archived, 0, 0x7E4EEC)
        struct.pack_into('<I', archived, 0xEC, 5 if case.get('resource') else 0)
        writes = {actor: bytes(entity), typ: bytes(kind), house: owner, cell: bytes(archived),
                  0x822CF2: bytes([case.get('enabled', True)]), 0xA8B238: dwords(5),
                  0xA83D4C: dwords(house if case.get('local', True) else house + 0x100)}
        output = native_call(0x459C20, ecx=actor, writes=writes,
            dumps={'pending': (actor + 0x4F0, 4), 'last': (actor + 0x4F4, 4),
                   'suppression': (actor + 0x4F8, 1), 'archive': (actor + 0x218, 4)},
            required_addresses=(0x459C20,), context=case)['dumps']
        pending = struct.unpack('<i', bytes.fromhex(output['pending']))[0]
        rows.append(dict(name=case['name'], input=dict(
            archive_cell=bool(case.get('archive')), resource_cell=bool(case.get('resource')),
            suppression=bool(case.get('suppressed')), voices_enabled=case.get('enabled', True),
            owner_is_local=case.get('local', True),
            resource_gatherer=bool(case.get('gatherer', stock['resource_gatherer'])),
            resource_destination=bool(case.get('destination', stock['resource_destination'])),
            voice_deploy=names.get(case.get('deploy', stock['voice_deploy'])),
            voice_harvest=names.get(case.get('harvest', stock['voice_harvest'])),
            pending=names.get(case.get('pending', -1))), output=dict(
                pending=names.get(pending), pending_index=pending,
                last=struct.unpack('<i', bytes.fromhex(output['last']))[0],
                suppression=bool(int(output['suppression'], 16)),
                archive_retained=int.from_bytes(bytes.fromhex(output['archive']), 'little')
                                 == (cell if case.get('archive') else 0))))
    return rows


def slave_pack_selling_cues(m, typ):
    """Original admitted stage0 branch to its positional PlayAt call boundary."""
    actor, house, cell = m.alloc(0x800), m.alloc(0x6000), m.alloc(0x200)
    m.u.mem_write(actor, dwords(0x7E3EBC))
    m.u.mem_write(actor + 0x520, dwords(typ))
    m.u.mem_write(actor + 0x21C, dwords(house))
    m.u.mem_write(actor + 0x9C, dwords(1664, 1920, 0))
    m.u.mem_write(cell, dwords(0x7E4EEC))
    m.u.mem_write(0x822CF2, b'\1')
    m.u.mem_write(0xA8B238, dwords(5))
    m.u.mem_write(0xA83D4C, dwords(house))
    rows = []
    for name, resource in (('ordinary_destination', False), ('resource_destination', True)):
        m.u.mem_write(actor + 0x218, dwords(cell))
        m.u.mem_write(actor + 0x4F0, dwords(-1, -1))
        m.u.mem_write(actor + 0x4F8, b'\0')
        m.u.mem_write(cell + 0xEC, dwords(5 if resource else 0))
        calls = []

        def observe(u, pc, _size, _data):
            if pc == 0x708D90:
                index = signed(u, u.reg_read(UC_X86_REG_ESP) + 4)
                calls.append(dict(kind='QueueVoice', sound=sound_name(m, index)))

        hook = m.u.hook_add(UC_HOOK_CODE, observe)
        try:
            m.u.reg_write(UC_X86_REG_ESP, SP)
            m.u.reg_write(UC_X86_REG_EBP, actor)
            run_checked(m.u, 0x44A9DA, 0x44AA38,
                        required_addresses=(0x459C20, 0x708D90, 0x44AA30))
        finally:
            m.u.hook_del(hook)
        calls.append(dict(kind='positional_PlayAt_boundary',
            sound=sound_name(m, m.u.reg_read(UC_X86_REG_ECX)),
            coords=list(struct.unpack('<3i', m.u.mem_read(m.u.reg_read(UC_X86_REG_EDX), 12)))))
        rows.append(dict(name=name, calls=calls,
                         pending=sound_name(m, signed(m.u, actor + 0x4F0)),
                         suppression=bool(m.u.mem_read(actor + 0x4F8, 1)[0])))
    return rows


def slave_pack_voice_generate():
    m, typ, retail, stock = slave_pack_voice_inputs()
    rows = slave_pack_voice_controls(stock)
    selling = slave_pack_selling_cues(m, typ)
    actor = m.alloc(0x800)
    m.u.mem_write(actor + 0x4F0, dwords(stock['voice_deploy'], stock['voice_harvest']))
    m.u.mem_write(actor + 0x4F8, b'\1')
    for register, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_ESI, actor), (UC_X86_REG_EBX, 0)):
        m.u.reg_write(register, value)
    run_checked(m.u, 0x70C21F, 0x70C246, required_addresses=(0x405BE0, 0x70C240))
    reset = dict(pending=signed(m.u, actor + 0x4F0), last=signed(m.u, actor + 0x4F4),
                 suppression=bool(m.u.mem_read(actor + 0x4F8, 1)[0]))
    return dict(schema_version=1, **retail, voice_controls=rows,
                selling_cue_controls=selling, loaded_voice_reset=reset)


def slave_pack_voice_metadata():
    return dict(provenance(
        scope='YAREFN original layered sound references and complete undeploy voice requests; bounded Selling stage0 cue ordering and loaded audio-reset suffix',
        assumptions=[
            'Original BuildingType45DD90/TechnoType710AF0 constructors establish defaults independently of physical values. Selected balanced scalar-reader blocks consume YAREFN RULESMD, optional LANGRULE, MPBattleMD and the explicitly selected physical map; original parsers/name lookup own results.',
            'Fresh native_oracle.call instances execute complete459C20/708E00/708D90 and House50B6F0 with supplied object/type/House/Cell priors and original Building/Cell vtables. This does not establish their startup, placement, ArchiveTarget writer or a manual input route.',
            'Selling rows enter the already-admitted UndeploysInto stage0 branch44A9DA and stop before original positional PlayAt7509E0 at44AA38. Original voice request runs before that boundary; positional playback, full Selling/Logic frames and audio RNG are excluded.',
            'Original loaded voice-reset suffix70C21F..70C246 reconstructs the handle and resets pending/last, retaining supplied suppression. Full native save/load and stream swizzling are excluded.',
            'The complete request bodies contain no RNG draw or timer write. Downstream PlayAt/audio sampling is covered separately by the existing unit voice/audio comparisons, not newly certified for these Slave Miner samples.'],
        substitutions=[
            'Existing Reader/Sound owners supply physical lexical INI caches and arena/TLS transport. Existing sound_inputs maps selected physical sample names to fixture-relative AudioIndex ordinals; no sample decoding or device output runs.',
            'Object/House/Cell fields are declared priors. No request-body instruction, gameplay return or vtable is substituted.'],
        entry_points=dict(type_constructor=0x45DD90, voice_deploy_reader=0x713769,
            voice_harvest_reader=0x71363D, resource_flag_readers=0x7143D0,
            building_undeploy_voice=0x459C20, techno_voice_deploy=0x708E00,
            queue_voice=0x708D90, selling_cue=0x44A9DA,
            positional_call_boundary=0x44AA38, loaded_voice_reset=0x70C21F)),
        command='python -m tools.input_oracle.unit_voice_playback --slave-miner-pack-voice --check',
        input_environment=['VERA20K_SLAVE_PACK_INPUTS', 'VERA20K_SLAVE_PACK_MAP'])


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
    argv = list(sys.argv[1:])
    if '--slave-miner-pack-voice' in argv:
        argv.remove('--slave-miner-pack-voice')
        finish_vectors(slave_pack_voice_generate, HERE / 'slave_miner_pack_voice.json',
                       provenance=slave_pack_voice_metadata, argv=argv, source_paths=GUARDED)
    else:
        finish_vectors(generate, HERE / 'unit_voice_playback.json', provenance=metadata,
                       argv=argv, source_paths=GUARDED)
