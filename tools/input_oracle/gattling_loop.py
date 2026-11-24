"""Stock YTNK Loop1: original readers, admitted Unit trigger and audio cleanup.

The native sound service, ring fill, raw/IMA callbacks, soft-release replacement,
worker endpoint and pool retirement execute original instructions. Supplied
world and single-thread OS boundaries exclude complete combat/world scheduling
and actual hardware concurrency. PCM bytes and RNG states are native outputs.
"""
from pathlib import Path
from types import SimpleNamespace
import hashlib
import os
import struct

from tools.input_oracle import unit_voice_playback as voice
from tools.input_oracle.area_guard import source_paths
from tools.rules_oracle.bridge_child_sound import sections
from tools.spatial_oracle.building_body_rules import INI, SP, dwords
from tools.spatial_oracle.refinery_dock import HOUSE
from tools.spatial_oracle._factory_infantry_output.pcm import NativePcmObserver, observed_bytes
from tools.spatial_oracle._factory_infantry_output.runtime import require
from tools.native_oracle import RET_MAGIC, finish_vectors, provenance
from unicorn import UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP

SOUNDS = ('GattlingGunAttackLoop1', 'GattlingGunAttackLoop2', 'GattlingGunAttackLoop3')
LAYERS = ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'Hills.mmx')
VOXELS = ('YTNK.VXL', 'YTNK.HVA', 'YTNKTUR.VXL', 'YTNKTUR.HVA')
ABSENT = ('YTNKBARL.VXL', 'YTNKBARL.HVA')
HERE = Path(__file__).resolve().parent


def prepare(root, voxels, *, sound_overrides=None):
    m, inherited = voice.prepare(root)
    raw_files = {name: (voxels / name).read_bytes() for name in VOXELS}
    raw_files.update({name: None for name in ABSENT})
    require(not any((voxels / name).exists() for name in ABSENT), 'Declared missing voxel exists')
    m.platform_audio.configure_transport(prepared_files=raw_files)
    m.phase = 'original_selected_YTNK_readers'
    physical_sound = sections((root / 'SOUNDMD.INI').read_bytes())
    selected = {name: physical_sound[name] for name in ('Defaults', *SOUNDS)}
    for name, values in (sound_overrides or {}).items():
        selected[name] = {**selected[name], **values}
    selected['SoundList'] = {key: value for key, value in physical_sound['SoundList'].items() if value in SOUNDS}
    m.make_ini(selected)
    m.invoke(0x7510D0, INI)
    ids = {name: m.invoke(0x7514D0, m.cstring(name)) for name in SOUNDS}
    m.gattling_sound_ids = ids
    typ = m.alloc(0x2000)
    m.invoke(0x7470D0, typ, (m.cstring('YTNK'),))
    m.types['YTNK'] = typ
    art = sections((root / 'ARTMD.INI').read_bytes())
    layers = []
    for filename in LAYERS:
        path = root / filename
        if not path.exists():
            require(filename == 'LANGRULE.INI', 'Required physical reader input is absent')
            layers.append(dict(file=filename, absent=True))
            continue
        raw = path.read_bytes()
        physical = sections(raw)
        m.make_ini({'YTNK': physical['YTNK']} if 'YTNK' in physical else {})
        rules_ini = m.alloc(0x40)
        m.u.mem_write(rules_ini, bytes(m.u.mem_read(INI, 0x40)))
        m.make_ini({'YTNK': art['YTNK']})
        m.invoke(0x674000, 0, (rules_ini,))
        admitted = m.invoke(0x747620, typ, (rules_ini,)) & 255
        layers.append(dict(file=filename, sha256=hashlib.sha256(raw).hexdigest(), admitted=admitted,
            type=typ, is_gattling=m.u.mem_read(typ + 0xCD5, 1)[0],
            thresholds_rates=list(struct.unpack('<15i', m.u.mem_read(typ + 0xCD8, 60)))))
    weapons = []
    ptrs = {m.read32(typ + base + i * 28) for base in (0x898, 0xA94) for i in range(6)} - {0}
    for weapon in sorted(ptrs):
        name = m.string(weapon + 0x24)
        for filename in LAYERS:
            path = root / filename
            if not path.exists():
                continue
            physical = sections(path.read_bytes())
            m.make_ini({name: physical[name]} if name in physical else {})
            rules_ini = m.alloc(0x40)
            m.u.mem_write(rules_ini, bytes(m.u.mem_read(INI, 0x40)))
            admitted = m.invoke(0x772080, weapon, (rules_ini,)) & 255
            weapons.append(dict(file=filename, weapon=name, pointer=weapon, admitted=admitted,
                reports=[m.read32(m.read32(weapon + 0xC0) + 4 * i) for i in range(m.read32(weapon + 0xCC))]))
    m.make_ini({})
    m.voice_theme_ini = m.alloc(0x40)
    m.u.mem_write(m.voice_theme_ini, bytes(m.u.mem_read(INI, 0x40)))
    sounds = []
    for name, index in ids.items():
        pointer = m.read32(m.read32(m.read32(0xB1D37C) + index * 4))
        sounds.append(dict(name=name, registry_index=index, pointer=pointer,
            fields={key: m.read32(pointer + off) for key, off in {
                'control': 0x10, 'type': 0x14, 'volume_fixed16': 0x1C, 'priority': 0x40,
                'limit': 0x48, 'loop': 0x4C, 'range': 0x50, 'delay_low': 0x58,
                'delay_high': 0x5C, 'fshift_low': 0x60, 'fshift_high': 0x64,
                'vshift': 0x68, 'attack': 0x138, 'decay': 0x13C, 'sample_count': 0x134}.items()},
            sorted_sample_indices=[m.read32(pointer + 0xB4 + 4 * i) for i in range(m.read32(pointer + 0x134))]))
    return m, dict(inherited=inherited, layers=layers, weapons=weapons, sound_ids=ids,
                  sound_sections=selected, sounds=sounds, file_io=m.platform_audio.file_io,
                  declared_sound_overrides=sound_overrides or {})


class GattlingHistory(voice.VoiceHistory):
    """Domain observation only; state, invocation, device and PCM retain their owners."""
    WATCH = {**voice.VoiceHistory.WATCH,
        0x404700: ('PreparePlayout', 0), 0x70DE70: ('GattlingIncrease', 4),
        0x70E000: ('GattlingUpdate', 4), 0x750D40: ('RedriveLoop', 0),
        0x7353C0: ('UnitConstructor', 8), 0x4052E0: ('SkipAttack', 0),
        0x409880: ('FillDeviceRing', 0), 0x40A340: ('DriverStart', 0),
        0x40A610: ('DriverStop', 0)}

    def __init__(self, m, name, *, seed=31, construct_gattling=True):
        self.gattling_actor = 0
        self.sites, self.raw_copies, self.fills = [], [], []
        self.ima_copies, self.decoder_identity = [], {}
        self.loaded_clips = {}
        self.fill_pending, self.copy_pending = [], None
        if not construct_gattling:
            self.WATCH = {**self.WATCH, 0x405FD0: ('HandleDetach', 0)}
        super().__init__(m, name, seed=seed, actor_count=0 if construct_gattling else 1)
        self.pcm = NativePcmObserver(raw_pcm=True)
        self.pcm.install(self.f, self.u, self.r)
        self.u.hook_add(UC_HOOK_MEM_WRITE, self.observe_silence)
        if not construct_gattling:
            self.initial = self.snapshot()
            return
        self.gattling_actor = self.f.allocate(0x1000)
        self.invoke('original_unit_constructor_NULLHouse', 0x7353C0,
                    self.gattling_actor, m.types['YTNK'], 0)
        self.u.mem_write(self.gattling_actor + 0x21C, dwords(HOUSE))
        self.u.mem_write(self.gattling_actor + 0x14C, dwords(HOUSE))
        self.u.mem_write(self.gattling_actor + 0x9C, dwords(3456, 3712, 0))
        self.inputs.append(dict(kind='declared_stationary_world_prior', house=HOUSE,
            actor=self.gattling_actor, house_fields=[0x21C, 0x14C], coord=[3456, 3712, 0],
            registration='Original NULL-House constructor and Drive COM; no Unlimbo/full House or world insertion'))
        self.initial = self.snapshot()

    def observe_silence(self, u, access, address, size, value, data):
        pc = u.reg_read(UC_X86_REG_EIP)
        if pc not in (0x409B0E, 0x409B15):
            return
        require(self.fill_pending, 'Original silence write outside ring fill')
        row = self.fill_pending[-1]
        pointer, length = self.f.platform_audio.buffers[self.r(row['backend'] + 0x64)]
        offset = address - pointer
        require(0 <= offset <= length - size, 'Original silence write outside device ring')
        values = row.setdefault('silence_write_ranges', [])
        value &= (1 << (8 * size)) - 1
        if values and values[-1]['pc'] == pc and values[-1]['offset'] + values[-1]['bytes'] == offset and values[-1]['element_bytes'] == size and values[-1]['value'] == value:
            values[-1]['writes'] += 1
            values[-1]['bytes'] += size
        else:
            values.append(dict(pc=pc, offset=offset, element_bytes=size, value=value, writes=1, bytes=size))

    def sample_identity(self, sample):
        index = self.r(sample + 0x14)
        audio_index = self.r(self.r(sample + 0x3C))
        entry = self.r(audio_index) + index * 36
        return dict(cached_sample=sample, sorted_index=index,
            name=bytes(self.u.mem_read(entry, 16)).split(b'\0', 1)[0].decode('latin1'),
            bag_offset=self.r(entry + 0x10), bytes=self.r(entry + 0x14),
            rate=self.r(entry + 0x18), flags=self.r(entry + 0x1C), chunk_bytes=self.r(entry + 0x20),
            native_format=self.ints(sample + 0x40, 8))

    def backend(self, backend):
        channel = self.r(backend)
        return dict(pointer=backend, channel=channel, buffer=self.r(backend + 0x64),
            played_valid_bytes=voice.signed(self.u, backend + 8), cursor_quarter=self.r(backend + 0xC),
            source=self.r(backend + 0x28), source_chunk_remaining=voice.signed(self.u, backend + 0x2C),
            write_offset=self.r(backend + 0x30), bytes_to_ring_end=self.r(backend + 0x38),
            end_padding_remaining=voice.signed(self.u, backend + 0x3C),
            ring_bytes=self.r(backend + 0x68), quantum_bytes=self.r(backend + 0x6C),
            rate=self.r(backend + 0x4C), channels=self.r(backend + 0x50), sample_bytes=self.r(backend + 0x54),
            callback=self.r(backend + 0x70), source_buffer=self.r(channel + 0x168),
            source_chunk=self.r(channel + 0x16C), channel_source=self.r(channel + 0x170),
            channel_source_bytes=self.r(channel + 0x174), sample_end_gate=self.r(backend + 0xCC))

    def snapshot(self):
        state = super().snapshot()
        p = self.gattling_actor
        if p:
            state['gattling'] = dict(pointer=p, type=self.r(p + 0x6C4), stage=self.r(p + 0x140),
                value=self.r(p + 0x144), latch=self.u.mem_read(p + 0x4B8, 1)[0],
                coord=self.ints(p + 0x9C, 3), handle=self.ints(p + 0x4A4, 4),
                veterancy_hex=bytes(self.u.mem_read(p + 0x150, 4)).hex())
        for event in state['events']:
            ptr = event['pointer']
            event.update(index=voice.signed(self.u, ptr + 0xAC),
                playlist=[self.r(ptr + 0x160 + i * 4) for i in range(self.r(ptr + 0x1E0))],
                remaining=self.r(ptr + 0x1E0), next_sample=self.r(ptr + 0x148), loop_count=self.r(ptr + 0x1E4))
            for sample in event['loaded_samples']:
                if sample:
                    identity = self.sample_identity(sample)
                    self.loaded_clips.setdefault(identity['name'], identity)
            if event['channel']:
                event['driver'] = self.backend(self.r(event['channel'] + 0x158))
        return state

    def observe(self, u, pc, size, data):
        if pc in (0x41C27D, 0x41C2CB, 0x41C28C, 0x55A965, 0x55A987):
            voice.Mission.observe(SimpleNamespace(trace=self.recent, m=self.f, pending={},
                events=self.events, frame=self.r(voice.bc.FRAME), phase='setup'), u, pc, size, data)
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        for row in self.fill_pending[:]:
            if pc == row['caller'] and sp == row['sp'] + 4:
                row.update(return_pc=pc, result=u.reg_read(UC_X86_REG_EAX),
                    after=self.backend(row['backend']), rng_after=self.rng())
                buffer = row['after']['buffer']
                ptr, length = self.f.platform_audio.buffers[buffer]
                raw = bytes(u.mem_read(ptr, length))
                row.update(ring_sha256=hashlib.sha256(raw).hexdigest(), ring_hex=raw.hex())
                self.fill_pending.remove(row)
        if self.copy_pending and pc == self.copy_pending['caller']:
            row = self.copy_pending
            require(sp == row['sp'] + 16, 'Original PCM return ABI differs')
            count = self.r(row['output_count_pointer']) if row['output_count_pointer'] else None
            source_count = self.r(row['source_count_pointer']) if row['source_count_pointer'] else None
            row.update(output_bytes=count, source_bytes=source_count, rng_after=self.rng(),
                       result=u.reg_read(UC_X86_REG_EAX), after=self.backend(row['backend']))
            self.copy_pending = None
        if pc == 0x409880:
            row = dict(phase=self.phase, entry=pc, caller=self.r(sp), sp=sp,
                backend=u.reg_read(UC_X86_REG_ECX), requested_bytes=u.reg_read(UC_X86_REG_EDX), rng_before=self.rng())
            row['before'] = self.backend(row['backend'])
            self.fills.append(row)
            self.fill_pending.append(row)
        if pc in (0x409D90, 0x409DE0):
            require(self.copy_pending is None, 'Nested native PCM callback')
            backend = u.reg_read(UC_X86_REG_ECX)
            channel = self.r(backend)
            output_count_pointer, source, source_count_pointer = self.ints(sp + 4, 3)
            row = dict(phase=self.phase, entry=pc, caller=self.r(sp), sp=sp, backend=backend,
                channel=channel, source=source, destination=u.reg_read(UC_X86_REG_EDX),
                output_count_pointer=output_count_pointer, source_count_pointer=source_count_pointer,
                requested_bytes=self.r(output_count_pointer) if output_count_pointer else None,
                offered_bytes=self.r(source_count_pointer) if source_count_pointer else None,
                before=self.backend(backend), rng_before=self.rng(),
                pcm_observer_index=len(self.pcm.state['callbacks']))
            if source:
                sample_buffer = self.r(channel + 0x168)
                sample = sample_buffer - 0x1C
                row['identity'] = self.sample_identity(sample)
                current_chunk = self.r(channel + 0x16C)
                chunk, chunk_prior_bytes = self.r(sample_buffer + 8), 0
                seen = []
                while chunk and chunk != self.r(chunk + 8) and chunk != current_chunk:
                    require(chunk not in seen and len(seen) < 512, 'Native sample chunk cycle')
                    seen.append(chunk)
                    chunk_prior_bytes += self.r(chunk + 0x10)
                    chunk = self.r(chunk)
                require(chunk == current_chunk, 'Native channel chunk is not in its SampleBuffer')
                in_chunk = source - self.r(current_chunk + 0xC)
                require(0 <= in_chunk <= self.r(current_chunk + 0x10), 'Original source outside current chunk')
                row.update(sample_buffer=sample_buffer, sample_chunk=current_chunk,
                    chunk_prior_bytes=chunk_prior_bytes, in_chunk_offset=in_chunk,
                    source_offset=chunk_prior_bytes + in_chunk)
                source_raw = bytes(u.mem_read(source, row['offered_bytes']))
                bag_offset = row['identity']['bag_offset'] + row['source_offset']
                require(source_raw == (self.f.platform_audio.root / 'audio.bag').read_bytes()[bag_offset:bag_offset + len(source_raw)],
                        'Native callback source differs from selected physical cached span')
                row['source_sha256'] = hashlib.sha256(source_raw).hexdigest()
                self.decoder_identity[backend] = row['identity']
            elif backend in self.decoder_identity:
                row.update(identity=self.decoder_identity[backend], decoder_flush=True)
            (self.raw_copies if pc == 0x409D90 else self.ima_copies).append(row)
            self.copy_pending = row
        if pc in (0x4045D6, 0x404678, 0x404664, 0x4047EA, 0x4047EF, 0x405AC0,
                  0x405A00, 0x40A512, 0x40A536, 0x40A55A, 0x40A5AF):
            self.sites.append(dict(pc=pc, phase=self.phase, ecx=u.reg_read(UC_X86_REG_ECX),
                edx=u.reg_read(UC_X86_REG_EDX), eax=u.reg_read(UC_X86_REG_EAX),
                state=self.snapshot()))
        super().observe(u, pc, size, data)

    def fire_tail(self, label='original_admitted_Unit_fire_tail'):
        return self.region(label, 0x737063, 0x7370AE,
            registers=((UC_X86_REG_ESI, self.gattling_actor), (UC_X86_REG_EBP, 2)), required=(0x70DE70,))

    def audio(self, milliseconds, *, wall=60_000_000):
        call_start = len(self.f.platform_audio.calls)
        result = super().audio(milliseconds, wall=wall)
        for row in self.f.platform_audio.calls[call_start:]:
            if row.get('method') == 'buffer:12':
                require(row['result'] == 0, 'Original device Play failed')
                buffer = row['args'][0]
                if hasattr(self, 'devices') and buffer in self.devices:
                    self.devices[buffer]['value'] = 1
                    self.inputs.append(dict(kind='os_playing_after_successful_Play', buffer=buffer, status=1))
        return result

    def redrive(self):
        self.u.mem_write(SP, dwords(RET_MAGIC))
        return self.region('original_audible_loop_reallocation', 0x750D40, RET_MAGIC,
            registers=((UC_X86_REG_ECX, self.gattling_actor + 0x9C),
                       (UC_X86_REG_EDX, self.gattling_actor + 0x4A4)), required=(0x4052E0,))

    def exact_fill(self, label, capacity):
        event = self.snapshot()['events'][0]
        backend = event['driver']['pointer']
        self.u.mem_write(SP, dwords(RET_MAGIC))
        self.inputs.append(dict(kind='bounded_original_fill_capacity', label=label,
            backend=backend, capacity=capacity))
        result = self.region(label, 0x409880, RET_MAGIC,
            registers=((UC_X86_REG_ECX, backend), (UC_X86_REG_EDX, capacity)))
        # emu_stop at the successful top-level return is outside UC code hooks;
        # observe that already reached boundary without executing or mutating it.
        self.observe(self.u, RET_MAGIC, 0, None)
        return result

    def release(self, milliseconds):
        self.invoke('original_Unit_no_target_soft_release', 0x736DF0, self.gattling_actor)
        for buffer, device in self.devices.items():
            device.update(play_cursor=0, write_cursor=0)
            self.inputs.append(dict(kind='os_restarted_cursor', buffer=buffer,
                play_cursor=0, write_cursor=0, prior_status=device['value']))
        call_start = len(self.f.platform_audio.calls)
        self.audio(milliseconds)
        plays = [row for row in self.f.platform_audio.calls[call_start:] if row.get('method') == 'buffer:12']
        require(plays and all(row['result'] == 0 for row in plays), 'Original decay did not successfully Play')
        require(all(row['args'][0] in self.devices for row in plays), 'Decay uses unobserved device')

    def project(self):
        require(not self.fill_pending and self.copy_pending is None, 'Native audio boundary did not return')
        for row in self.pcm.state['callbacks']:
            raw = bytes.fromhex(row['output_bytes_hex'])
            require(observed_bytes(row['output_writes'], len(raw)) == raw, 'Original PCM writes lack coverage')
        for row in self.raw_copies + self.ima_copies:
            observed = self.pcm.state['callbacks'][row.pop('pcm_observer_index')]
            require(observed['backend'] == row['backend'] and observed['caller'] == row['caller']
                    and observed['source'] == row['source']
                    and observed['returned_output_bytes'] == (row['output_bytes'] or 0)
                    and observed['consumed_source_bytes'] == (row['source_bytes'] or 0),
                    'Domain PCM association differs from its existing output owner')
            row.update(pcm_hex=observed['output_bytes_hex'], pcm_sha256=observed['output_sha256'])
            if row.get('identity') and row['entry'] == 0x409D90:
                identity = row['identity']
                offset = identity['bag_offset'] + row['source_offset']
                raw = bytes.fromhex(row['pcm_hex'])
                require(raw == (self.f.platform_audio.root / 'audio.bag').read_bytes()[offset:offset + len(raw)],
                        'Native PCM differs from selected physical AudioIndex span')
        result = super().project()
        for row in self.fills:
            for span in row.get('silence_write_ranges', []):
                offset, length = span['offset'], span['bytes']
                raw = bytes.fromhex(row['ring_hex'])[offset:offset + length]
                require(raw == span['value'].to_bytes(span['element_bytes'], 'little') * span['writes'],
                        'Native silence writes differ from the returned ring')
        result.update(native_sites=self.sites, native_ring_fills=self.fills,
                      raw_pcm_copies=self.raw_copies, ima_pcm_copies=self.ima_copies,
                      native_loaded_clip_identities=self.loaded_clips)
        return result


def ordinary(m):
    h = GattlingHistory(m, 'stock_initial_audible_loop1_soft_release')
    h.fire_tail()
    h.audio(1034)
    for ms, quarter in ((1060, 0), (1290, 1), (1545, 2), (1801, 3), (2056, 0), (2312, 1)):
        h.worker(ms, quarter)
    h.release(2346)
    for ms, quarter in ((2602, 1), (2858, 2), (3114, 3), (3370, 0), (3404, 0)):
        h.worker(ms, quarter)
    h.audio(3438)
    require(not h.snapshot()['events'], 'Original decay did not retire from event pool')
    return h.project()


def reallocated(m):
    h = GattlingHistory(m, 'stock_inaudible_then_loop1_reallocation')
    h.u.mem_write(h.gattling_actor + 0x9C, dwords(256000, 256000, 0))
    h.inputs.append(dict(kind='declared_inaudible_coordinate', coord=[256000, 256000, 0]))
    h.fire_tail('original_Unit_fire_tail_inaudible')
    require(not h.snapshot()['events'], 'Declared outer coordinate is still audible')
    h.u.mem_write(h.gattling_actor + 0x9C, dwords(3456, 3712, 0))
    h.inputs.append(dict(kind='declared_audible_coordinate', coord=[3456, 3712, 0]))
    h.redrive()
    require(h.snapshot()['events'][0]['flags'] & 8, 'Original reallocation did not SkipAttack')
    h.audio(1034)
    for ms, quarter in ((1060, 0), (1290, 1), (1545, 2)):
        h.worker(ms, quarter)
    h.release(1579)
    for ms, quarter in ((1835, 1), (2091, 2), (2347, 3), (2603, 0), (2637, 0)):
        h.worker(ms, quarter)
    h.audio(2671)
    require(not h.snapshot()['events'], 'Original reallocated decay did not retire')
    return h.project()


def exact_end(m):
    h = GattlingHistory(m, 'stock_loop1_exact_source_end_fill_control')
    h.fire_tail()
    h.audio(1034)
    h.worker(1060, 0)
    last = h.raw_copies[-1]
    capacity = last['identity']['bytes'] - last['source_offset'] - last['source_bytes']
    require(capacity > 0, 'Initial ring has no current sample remainder')
    exact = h.exact_fill('original_fill_exact_current_sample_remainder', capacity)
    require(not exact['requests'], 'Exact capacity unexpectedly advanced playlist')
    h.exact_fill('original_fill_first_bytes_after_exact_sample_end', 2)
    h.release(1094)
    for ms, quarter in ((1350, 1), (1606, 2), (1862, 3), (2118, 0), (2152, 0)):
        h.worker(ms, quarter)
    h.audio(2186)
    require(not h.snapshot()['events'], 'Original exact-end control did not retire decay')
    return h.project()


def stage_stop(m):
    h = GattlingHistory(m, 'stock_loop1_hard_stage_stop_then_loop2_control')
    h.fire_tail()
    h.audio(1034)
    h.worker(1060, 0)
    # A bounded component call supplies the original public tick multiplier;
    # original Increase produces the threshold state before the next real Unit
    # tail. It makes no claim about 199 elapsed world/shot frames.
    h.invoke('original_Increase_threshold_component_ticks199', 0x70DE70, h.gattling_actor, 199)
    h.fire_tail('original_Unit_tail_hard_stage_transition')
    require(h.snapshot()['gattling']['stage'] == 1, 'Stock threshold did not advance to Loop2')
    h.audio(1094)
    h.worker(1120, 0)
    h.release(1128)
    # Loop2 has a different physical decay length. Keep advancing actual device
    # quarter inputs until its own Stop and next-visit endpoint, within a bound.
    for i, quarter in enumerate((1, 2, 3, 0, 1, 2, 3, 0)):
        h.worker(1384 + 256 * i, quarter)
        if all(event['state'] == 4 for event in h.snapshot()['events']):
            break
    h.audio(3466)
    require(not h.snapshot()['events'], 'Original hard-stage control did not retire')
    return h.project()


def missing_middle(m):
    h = GattlingHistory(m, 'nonretail_unresolved_middle_index_native_slot_control')
    type_index = m.gattling_sound_ids[SOUNDS[0]]
    sound = h.r(h.r(h.r(0xB1D37C) + type_index * 4))
    before = h.r(sound + 0xB4 + 2 * 4)
    h.u.mem_write(sound + 0xB4 + 2 * 4, dwords(0xFFFFFFFF))
    h.inputs.append(dict(kind='declared_unresolved_AudioIndex_component', sound=sound,
        sound_name=SOUNDS[0], physical_list_slot=2, original_sorted_index=before, supplied_index=-1,
        coverage='All physical clips remain present; this bounded resolved-resource failure input is not a retail absence'))
    h.fire_tail()
    h.audio(1034)
    event = h.snapshot()['events'][0]
    require(event['sample_count'] == 4, 'Original failed sample load did not compact successful slots')
    h.worker(1060, 0)
    h.release(1094)
    for ms, quarter in ((1350, 1), (1606, 2), (1862, 3), (2118, 0), (2152, 0)):
        h.worker(ms, quarter)
    h.audio(2186)
    require(not h.snapshot()['events'], 'Original missing-slot control did not retire decay')
    return h.project()


def counted(m):
    h = GattlingHistory(m, 'declared_Loop3_original_reader_release_and_budget_control')
    h.fire_tail()
    h.audio(1034)
    event = h.snapshot()['events'][0]
    flags = event['flags']
    h.invoke('original_Unit_no_target_counted_loop_release', 0x736DF0, h.gattling_actor)
    require(h.snapshot()['events'][0]['flags'] == flags,
            'Counted-loop release unexpectedly requests owner-loop decay')
    ended = False
    for i in range(32):
        milliseconds, quarter = 1060 + 256 * i, i & 3
        h.worker(milliseconds, quarter)
        if all(event['state'] == 4 for event in h.snapshot()['events']):
            ended = True
            break
    require(ended, 'Original counted-loop budget did not reach its endpoint')
    h.audio(milliseconds + 34)
    require(not h.snapshot()['events'], 'Original counted loop did not retire')
    return h.project()


def interleaved(m):
    h = GattlingHistory(m, 'stock_loop1_then_GIMove_same_service_order_control')
    h.fire_tail()
    h.u.reg_write(UC_X86_REG_EDX, 0x2000)
    h.inputs.append(dict(kind='declared_GIMove_global_request', native_entry=0x750920,
        registry_index=m.voice_sound_ids['GIMove'], pan=0x2000, volume_bits=0x3F800000, handle=0,
        caller_coverage='GI voice admission/head is covered by unit_voice_playback; request boundary supplied here'))
    h.invoke('original_GIMove_global_request_after_loop1', 0x750920,
             m.voice_sound_ids['GIMove'], 0x3F800000, 0)
    h.audio(1034)
    for ms, quarter in ((1060, 0), (1290, 1), (1545, 2), (1801, 3), (2056, 0), (2312, 1)):
        h.worker(ms, quarter)
    h.release(2346)
    for ms, quarter in ((2602, 1), (2858, 2), (3114, 3), (3370, 0), (3404, 0)):
        h.worker(ms, quarter)
    h.audio(3438)
    require(not h.snapshot()['events'], 'Original interleaved cues did not retire')
    return h.project()


def one_shot(m, *, positional, detach):
    route = 'positional' if positional else 'global'
    action = 'generic_detach' if detach else 'release'
    h = GattlingHistory(m, f'stock_GIMove_{route}_{action}_cleanup_control',
                        construct_gattling=False)
    handle = h.actor + 0x4DC
    sound = m.voice_sound_ids['GIMove']
    require(h.ints(handle, 3) == [0, 0, 0], 'Original GI handle is not initially clear')
    h.inputs.append(dict(kind='declared_GIMove_request_with_original_GI_handle',
        route=route, native_entry=0x7509E0 if positional else 0x750920,
        registry_index=sound, handle=handle, actor=h.actor,
        caller_coverage='Original GI construction owns handle; voice caller/head is covered by unit_voice_playback. Only this request boundary is supplied'))
    if positional:
        h.u.reg_write(UC_X86_REG_EDX, h.actor + 0x9C)
        h.invoke('original_positional_GIMove_request', 0x7509E0, sound, handle)
    else:
        h.u.reg_write(UC_X86_REG_EDX, 0x2000)
        h.invoke('original_global_GIMove_request', 0x750920, sound, 0x3F800000, handle)
    require(h.r(handle) and h.r(handle + 8), 'Original request did not bind live sound handle')
    h.audio(1034)
    h.worker(1060, 0)
    h.invoke('original_generic_handle_detach' if detach else 'original_handle_release',
             0x405FD0 if detach else 0x406060, handle)
    h.audio(1094)
    for i in range(16):
        milliseconds, quarter = 1326 + 256 * i, (i + 1) & 3
        h.worker(milliseconds, quarter)
        if all(event['state'] == 4 for event in h.snapshot()['events']):
            break
    else:
        raise AssertionError('Original one-shot cleanup did not reach its endpoint')
    h.audio(milliseconds + 34)
    require(not h.snapshot()['events'], 'Original one-shot cleanup did not retire from pool')
    return h.project()


def generate():
    root = Path(os.environ['VERA20K_GATTLING_INPUTS'])
    voxels = Path(os.environ['VERA20K_GATTLING_VOXELS'])
    m, retail = prepare(root, voxels)
    physical = [dict(name=p.name, bytes=p.stat().st_size, sha256=hashlib.sha256(p.read_bytes()).hexdigest())
                for p in sorted(root.iterdir()) if p.is_file()]
    voxel_manifest = [dict(name=name, bytes=len((voxels / name).read_bytes()), sha256=hashlib.sha256((voxels / name).read_bytes()).hexdigest()) for name in VOXELS]
    rows = [ordinary(m), reallocated(m), exact_end(m), stage_stop(m), missing_middle(m), interleaved(m)]
    counted_inputs, counted_retail = prepare(root, voxels, sound_overrides={SOUNDS[0]: {'Loop': '3'}})
    rows.append(counted(counted_inputs))
    shared = [one_shot(m, positional=positional, detach=detach)
              for positional in (False, True) for detach in (False, True)]
    result = dict(schema_version=1, physical=physical, voxel_manifest=voxel_manifest, physically_absent=list(ABSENT),
                  retail=retail, counted_control_reader=counted_retail,
                  physical_clips=physical_pcm_bank(rows + shared, (root / 'audio.bag').read_bytes()),
                  rows=rows, shared_one_shot_controls=shared)
    return compact_observations(result)


def physical_pcm_bank(rows, bag):
    """Associate original cached identities and write-witness output once."""
    clips = {}
    for row in rows:
        for identity in row['native_loaded_clip_identities'].values():
            raw = bag[identity['bag_offset']:identity['bag_offset'] + identity['bytes']]
            clips.setdefault(identity['name'], {**identity, 'sha256': hashlib.sha256(raw).hexdigest(), 'source_hex': raw.hex()})
    decoded_names = {callback['identity']['name'] for row in rows for callback in row['ima_pcm_copies']}
    for name, clip in clips.items():
        if name not in decoded_names:
            clip['pcm_hex'] = clip['source_hex']
    for row in rows:
        decoded = {}
        for callback in row['ima_pcm_copies']:
            require('identity' in callback, 'Original decoder output has no native cached identity')
            decoded.setdefault(callback['identity']['name'], []).append(bytes.fromhex(callback['pcm_hex']))
        for name, segments in decoded.items():
            raw = b''.join(segments)
            if 'pcm_hex' in clips[name]:
                require(bytes.fromhex(clips[name]['pcm_hex']) == raw,
                        'Original repeated decoded bank identity has different PCM output')
                continue
            clips[name].update(pcm_hex=raw.hex(), decoded_pcm_bytes=len(raw), decoded_pcm_sha256=hashlib.sha256(raw).hexdigest(),
                decode_provenance='Actual original409DE0 callbacks and40AA70 block output/write witness; cached AudioIndex identity')
    return clips


def compact_observations(result):
    """Remove repeated native PCM hex; keep its identities and one physical bank.

    GattlingHistory has already checked original write coverage and physical
    AudioIndex/BAG association. No scalar, request, RNG bank, callback offset,
    ring digest or boundary is inferred or recomputed by this projection.
    """
    for row in result['rows'] + result.get('shared_one_shot_controls', []):
        for fill in row['native_ring_fills']:
            del fill['ring_hex']
        for callback in row['raw_pcm_copies'] + row['ima_pcm_copies']:
            del callback['pcm_hex']
    for clip in result['physical_clips'].values():
        if 'decoded_pcm_bytes' not in clip:
            require(clip['source_hex'] == clip['pcm_hex'], 'Raw physical PCM bank differs')
            del clip['source_hex']
    return result


def metadata():
    require(set(source_paths()) <= set(GUARDED), 'Native producer imported unguarded dependencies')
    result = provenance(scope=__doc__, assumptions=[
        'Existing unit_voice_playback/GIMoveHistory, EngineerJoinedFixture and NativeAudioPlatform own construction, invocation, the audio registry/cache/channel pool and worker transport; this producer adds domain inputs and read-only observations.',
        'Selected original UnitType7470D0/747620 and WeaponType772080 readers execute physical RULESMD, optional absent LANGRULE, MPBattleMD and Hills layers; ARTMD and SOUNDMD are fixed physical inputs. Report IDs and Gattling thresholds/rates are original reader outputs.',
        'Original Unit7353C0 constructs a NULL-House Unit and actual Drive COM. The admitted Unit fire suffix737063..7370AE executes Increase70DE70 and its real positional Loop1 report request; whole Unit736DF0 executes the no-target release path.',
        'Original SoundService4041D0 executes both start preparations4045D1/404673 and the initial StartPlayback ring fill before the next ready event is loaded. Reallocation750D40 reaches SkipAttack4052E0; initial7509E0 does not.',
        'All three RandomClass buffers, inclusive requests, raw/rejected advances and ordered native call boundaries are retained. Every numeric value and PCM digest in the payload comes from original execution or exact physical bytes.',
        'Original409880 ring fills and409D90 raw copies retain source offsets, fill capacity, returned driver state and actual ring SHA256. NativePcmObserver explicitly opts into raw observation and validates original REP writes. GIMove409DE0/40AA70 callbacks retain actual decoded and flush output; one decoded PCM bank is composed only from those original writes.',
        'Soft release406060, driver Stop40A610, its40A63B endpoint call, decay restart, worker Stop and later405A00/service retirement are distinct executed boundaries. Native silence409B0E/409B15 writes are observed and checked against the actual ring.',
        'Seven rows cover ordinary Loop1, inaudible reallocation, exact-capacity no-advance, hard stage stop into stock Loop2, an unresolved middle-index control, Loop1 then stock GIMove in one service, and a Loop=3 original-reader release control.',
        'Four separate shared_one_shot_controls execute stock GIMove global750920 and positional7509E0 requests with a live original GI-owned handle. Release406060 and generic detach405FD0 are distinct original bodies; UpdateState4055C0, driver stop, real worker endpoint and later pool retirement execute without a fake endpoint.'], substitutions=[
        'Inherited supplied Cells/House/listener/visibility and16x16 map extent; selected Hills reader input is not a whole scenario. Unit House fields21C/14C and stationary coordinate3456,3712,0 are declared after NULL-House construction; no Unlimbo/full House or world insertion.',
        'Already-admitted fire code2 is supplied to the original Unit suffix. Target search, GreatestThreat, FindPath, complete projectile/damage and full Scenario scheduling are excluded; original selected Report/type reads are not replaced.',
        'Physical YTNK/YTNKTUR voxel/HVA files are pinned byte inputs. Physically absent YTNKBARL.VXL/HVA are explicit None inputs at the existing RawFile OS seam, so original availability/read failure branches still execute. This absence is separate from the authored unresolved audio-index control.',
        'Original Vox, Speech and declared empty ThemeControl initialize shared stream reservations. Physical retail Theme catalog and music playback are excluded.',
        'NativeAudioPlatform supplies immutable files, QPF/QPC and per-buffer OS status/cursors; the actual created4095B0 worker is resumed across its original Sleep seam. Successful Play status and restart cursors are explicit OS inputs. No native callback, endpoint, gameplay return or arithmetic result is substituted; no real concurrent hardware or audible-output claim.',
        'Exact-source-end uses declared FillDeviceRing capacities8966 then2, not a hardware-quarter cadence. Hard stage control supplies Increase ticks199, not199 elapsed world frames. Nonretail failure supplies one resolved sample index-1, preserving all physical clips. Counted-loop control overrides only Loop=3 through the original sound reader.',
        'The interleaved second request enters original global Play750920 with stock GIMove ID, pan0x2000, float volume1.0 and NULL handle. Its GI voice caller/head is covered separately by unit_voice_playback; this row supplies the request boundary.',
        'Shared one-shot controls use the existing original E1/Infantry/Walk construction and Unlimbo fixture and its handle at4DC. The global or positional sound request is the supplied boundary; no GI voice or movement caller is claimed by these controls. Cleanup enters original Release406060 or generic Detach405FD0 directly on that original handle.',
        'Repeated ring/callback PCM hex is mechanically omitted after original write/identity validation; scalar states, offsets, counts, digests, all RNG banks and one physical PCM bank remain.'], entry_points={
        'sound_registry_reader': 0x7510D0, 'sound_type_reader': 0x750440,
        'unit_type_constructor': 0x7470D0, 'unit_type_reader': 0x747620,
        'weapon_type_reader': 0x772080, 'unit_constructor': 0x7353C0,
        'admitted_unit_fire_suffix': 0x737063, 'unit_fire_at_target': 0x736DF0,
        'gattling_increase': 0x70DE70, 'gattling_update': 0x70E000,
        'play_at_position': 0x7509E0, 'redrive_loop': 0x750D40,
        'global_play': 0x750920, 'skip_attack': 0x4052E0,
        'audio_service': 0x406F70, 'sound_service': 0x4041D0,
        'cached_sample_load': 0x401C00, 'prepare_playout': 0x404700,
        'advance_playlist': 0x4047B0, 'start_playback': 0x4054A0,
        'release_handle': 0x406060, 'generic_detach_handle': 0x405FD0,
        'stop_clear_handle': 0x405D40,
        'driver_start': 0x40A340, 'driver_stop': 0x40A610,
        'fill_device_ring': 0x409880, 'raw_pcm_copy': 0x409D90,
        'ima_pcm_callback': 0x409DE0, 'ima_block': 0x40AA70,
        'worker': 0x4095B0, 'event_endpoint': 0x405A00})
    result.update(command='python -m tools.input_oracle.gattling_loop --check',
        input_environment=['VERA20K_GAMEMD_EXE or RA2_DIR', 'VERA20K_GATTLING_INPUTS', 'VERA20K_GATTLING_VOXELS'],
        shared_owner_extensions={
            'tools/spatial_oracle/engineer_repair_admission.py': {
                'purpose': 'Explicit pinned absent prepared-file input at existing immutable RawFile OS seam; original reader and failure branch remain active'},
            'tools/spatial_oracle/_factory_infantry_output/pcm.py': {
                'purpose': 'Opt-in observation of original409D90 raw copy/REP writes; default IMA-only observation and old corpus projection retained'}},
        legacy_GI_payload_identity={
            'file': 'tools/input_oracle/unit_voice_playback.json',
            'canonical_payload_sha256': 'eba4bdbfcc88cf89c04f26ca063d17c06808aab3080397e962c8f608d34ec47c',
            'file_sha256': '22fa256b15963d8bfb09f93cdefaf38cea29cea6196345a1638d707e3be1c945'},
        field_mapping={
            'unit': {'stage': '140', 'value': '144', 'latch': '4B8', 'handle_words': '4A4..4B0', 'coord': '9C..A4', 'type': '6C4', 'veterancy': '150'},
            'event': {'flags': '18', 'state': '1C', 'sound': '24', 'samples': '28', 'sample_count': 'A8', 'sample_index': 'AC', 'channel': 'B0', 'next_buffer': '148', 'playlist': '160', 'remaining': '1E0', 'loop_pass': '1E4'},
            'cached_sample': {'sorted_index': '14', 'loaded': '18', 'sample_buffer': '1C', 'tracker': '3C', 'format_words': '40'},
            'driver': {'valid_counter': '08 signed', 'cursor_quarter': '0C', 'source': '28', 'chunk_remaining': '2C signed', 'write_offset': '30', 'bytes_to_ring_end': '38', 'end_padding': '3C signed', 'ring_bytes': '68', 'quantum_bytes': '6C', 'copy_callback': '70', 'rate': '4C', 'channels': '50', 'sample_bytes': '54', 'sample_end_gate': 'CC'},
            'native_format': {'words': 'CachedSample+40: 8 literal words', 'callback_selector': 'word1: 0 raw409D90; 1 IMA409DE0/40AA70'}})
    result['field_mapping']['sound_handle'] = {
        'words': 'event0, serial4, sound_type8, pool_tag0C',
        'GI_handle_offset': 'Infantry+4DC', 'Unit_gattling_handle_offset': 'Unit+4A4'}
    result['nullable_callback_counts'] = 'NULL native count pointers project as JSON null; address0 contains fixture SEH state, not a sample byte counter. Shared NativePcmObserver uses0 for that absent counter; its PCM output/write witness remains unchanged.'
    return result


if __name__ == '__main__':
    GUARDED = source_paths()
    # Explicit module ownership also permits scratch rehearsal before this file
    # is copied into its stable package path. Do not introspect patched callables.
    GUARDED['tools/input_oracle/gattling_loop.py'] = Path(__file__).resolve()
    finish_vectors(generate, HERE / 'gattling_loop.json', provenance=metadata,
                   source_paths=GUARDED)
