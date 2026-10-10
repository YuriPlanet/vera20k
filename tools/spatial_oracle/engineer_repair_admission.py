"""Original519948 effective-mission and ground-building target admission.

This is a supplied INTERIOR PerCellProcess frame, not full PerCell2 entry.
The upstream reason, spy/thief/transporter/zone/path/Walk admission and cached
physical-cell producer have NOT executed. Original Infantry Mission5B3040,
Building457620/465D40 and Cell47C520 execute unchanged; hooks only observe.
Stops before519B58 (Engineer/type/BridgeRepairHut and effects not executed),
or before the original rejection continuations51A071/51A0D4.
"""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_ESP
from tools.native_oracle import load_image, run_checked, STACK_BASE, STACK_SIZE, SCRATCH, RET_MAGIC, finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords

ACTOR, HUT, OTHER, KIND, CELL = [SCRATCH + n * 0x2000 for n in range(1, 6)]
BEGIN, ADMITTED, NO_TARGET, NO_MISSION = 0x519948, 0x519B58, 0x51A071, 0x51A0D4
INF_VTABLE, BUILDING_VTABLE = 0x7EB058, 0x7E3EBC


def execute(case):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x10000)
    u.mem_map(RET_MAGIC, 0x1000)
    sp = STACK_BASE + STACK_SIZE - 0x1000
    identities = {0: None, ACTOR: 'infantry', HUT: 'hut', OTHER: 'other', CELL: 'cached_cell'}
    pointers = {None: 0, 'hut': HUT, 'other': OTHER}

    def read32(address):
        return struct.unpack('<I', u.mem_read(address, 4))[0]

    def signed_value(value):
        return value if value < 0x80000000 else value - 0x100000000

    assert read32(INF_VTABLE + 0x184) == 0x5B3040
    assert read32(BUILDING_VTABLE + 0x80) == 0x457620
    building_what_am_i = read32(BUILDING_VTABLE + 0x2C)
    u.mem_write(ACTOR, dwords(INF_VTABLE))
    u.mem_write(ACTOR + 0xAC, dwords(case['current']))
    u.mem_write(ACTOR + 0xB4, dwords(case['queued']))
    u.mem_write(ACTOR + 0x5A4, dwords(pointers[case['nav_com']]))
    u.mem_write(ACTOR + 0x2B4, dwords(pointers[case['attack_target']]))
    for address in (HUT, OTHER):
        u.mem_write(address, dwords(BUILDING_VTABLE))
        # Abstract+14 bit1 is Techno RTTI, not Object cell-marked state.
        u.mem_write(address + 0x14, dwords(1))
        u.mem_write(address + 0x520, dwords(KIND))
        u.mem_write(address + 0x34, dwords(0))  # No attached Tag.
    u.mem_write(KIND + 0x408, dwords(0))  # UndeploysInto=null, ordinary branch.
    u.mem_write(0xA8E9A0, bytes([case.get('object_iteration_enabled', True)]))
    ground = case.get('ground_list', ['hut'])
    for index, name in enumerate(ground):
        following = ground[index + 1] if index + 1 < len(ground) else None
        u.mem_write(pointers[name] + 0x30, dwords(pointers[following]))
    u.mem_write(CELL + 0xE4, dwords(pointers[ground[0]] if ground else 0))
    # Explicit upper-only control; the original47C520 reads ground+E4.
    u.mem_write(CELL + 0xE8, dwords(pointers[case.get('upper_head')]))
    u.mem_write(sp + 0x14, dwords(CELL))
    state_before = bytes(u.mem_read(ACTOR, 0x800))
    spans = [(BEGIN, 0x519B58), (0x5B3040, 0x5B3052),
             (0x457620, 0x45762B), (0x465D40, 0x465D6E),
             (0x47C520, 0x47C54F), (building_what_am_i, building_what_am_i + 6)]
    code = [bytes(u.mem_read(a, b - a)) for a, b in spans]
    trace = []

    def observe(_u, address, _size, _data):
        if address == 0x5B3040:
            assert u.reg_read(UC_X86_REG_ECX) == ACTOR
        elif address in (0x519952, 0x519961, 0x519970):
            trace.append(dict(kind='effective_mission', comparison={0x519952: 8, 0x519961: 11, 0x519970: 25}[address],
                              value=signed_value(u.reg_read(UC_X86_REG_EAX))))
        elif address == 0x457620:
            trace.append(dict(kind='nav_1x1_undeploy_query', target=identities[u.reg_read(UC_X86_REG_ECX)]))
        elif address == 0x51999E:
            trace.append(dict(kind='nav_1x1_undeploy_result', value=u.reg_read(UC_X86_REG_EAX) & 255))
        elif address == 0x47C520:
            assert u.reg_read(UC_X86_REG_ECX) == CELL
            trace.append(dict(kind='first_ground_building_query', cell='cached_cell'))
        elif address == building_what_am_i:
            trace.append(dict(kind='what_am_i', object=identities[u.reg_read(UC_X86_REG_ECX)]))
        elif address == 0x519B20:
            trace.append(dict(kind='first_ground_building_result', object=identities[u.reg_read(UC_X86_REG_EAX)]))
        elif address in (0x6E53A0, 0x65C780, 0x65C7E0, 0x570050, 0x573540):
            raise AssertionError('interior gate unexpectedly entered a side-effect receiver')

    u.hook_add(UC_HOOK_CODE, observe)
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_ESI, ACTOR)
    end = run_checked(u, BEGIN, (ADMITTED, NO_TARGET, NO_MISSION), count=5000,
                      required_addresses=(BEGIN, 0x5B3040))
    assert [bytes(u.mem_read(a, b - a)) for a, b in spans] == code
    assert bytes(u.mem_read(ACTOR, 0x800)) == state_before
    assert u.reg_read(UC_X86_REG_ESP) == sp
    return dict(input=case, output=dict(
        boundary=f'{end:08X}',
        outcome={ADMITTED: 'admitted_to_engineer_type_gate', NO_TARGET: 'no_matching_first_ground_building', NO_MISSION: 'mission_not_admitted'}[end],
        retained_building=identities[u.reg_read(UC_X86_REG_EDI)],
        trace=trace))


def inputs():
    rows = []
    for current, queued in [(5, 8), (8, 5), (8, -1), (8, 25),
                            (-1, 8), (-1, 11), (-1, 25), (-1, 5), (-1, -1),
                            (11, 5), (25, 5), (5, -1)]:
        rows.append(dict(name=f'mission_current_{current}_queued_{queued}', current=current, queued=queued,
                         nav_com='hut', attack_target=None))
    for nav_com, attack_target, name in [
        (None, 'hut', 'attack_only'), ('other', 'hut', 'other_nav_matching_attack'),
        ('hut', 'other', 'matching_nav_other_attack'),
        ('other', 'other', 'neither_matches'), (None, None, 'no_targets'),
    ]:
        rows.append(dict(name=name, current=8, queued=-1, nav_com=nav_com, attack_target=attack_target))
    for extra in [
        dict(name='empty_ground', ground_list=[]),
        dict(name='upper_only_hut', ground_list=[], upper_head='hut'),
        dict(name='different_first_building', ground_list=['other', 'hut']),
        dict(name='matching_first_building', ground_list=['hut', 'other']),
        dict(name='object_iteration_disabled', object_iteration_enabled=False),
    ]:
        rows.append(dict(current=8, queued=-1, nav_com='hut', attack_target=None, **extra))
    return rows


def generate():
    return [execute(case) for case in inputs()]


def primitive_main(argv=None):
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope=__doc__,
        entry_points={'supplied_interior_mission_gate': BEGIN, 'effective_mission': 0x5B3040,
                      'building_undeploy_predicate': 0x457620, 'type_undeploy_predicate': 0x465D40,
                      'first_ground_building': 0x47C520,
                      'engineer_type_gate_not_executed': ADMITTED,
                      'target_rejection_continuation_not_executed': NO_TARGET,
                      'mission_rejection_continuation_not_executed': NO_MISSION},
        assumptions=['ESI Infantry and ESP local frame already established; ESP+14 supplied cached physical CellClass.',
                     'Original Infantry/Building vtables; Techno RTTI bit1 set on Building objects; null UndeploysInto and attached Tags.',
                     'Supplied linked ground/upper objects and active-object lookup global; constructors and map loader omitted.',
                     'Mission current+AC, queued+B4, NavCom+5A4 and attack target+2B4 supplied independently.'],
        substitutions=['No instruction patches or native call answers. Observation-only hooks. All paths stop before suffix side effects.',
                       'This does not validate full519630 PerCell2 admission, Walk entry, Engineer/type flags, notification, repair or consumption.'],
    ), argv=argv)


# Additive whole-caller corpus. The original interior22 rows above and their
# files are independent. Owners are composed, not copied: Construction owns
# runtime arena/map/ART/Anim/RNG observation, Sound/Reader own lexical caches,
# original readers, image bytes and input allocation; Mission owns GUID OS IO.
import hashlib
import json
import os
import sys
from collections import deque
from types import MappingProxyType
from unicorn import UC_HOOK_MEM_INVALID, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EBX, UC_X86_REG_EDX, UC_X86_REG_EBP, UC_X86_REG_EIP
from tools.spatial_oracle import building_construction as construction
from tools.spatial_oracle.building_body_rules import INI, SP
from tools.spatial_oracle.refinery_dock import HOUSE, HTYPE, BLD, OTHER as OTHER_BUILDING, BLD_ITEMS, OTHER_ITEMS, cell
from tools.rules_oracle import bridge_child_sound
from tools.rules_oracle.bridge_anim_lists import HEAP
from tools.native_oracle import NATIVE_SHA256

JOINED_ROOT = Path(os.environ.get('VERA20K_ENGINEER_REPAIR_ASSETS',
                                 str(Path(os.environ.get('CARGO_TARGET_DIR', 'target')) / 'asset/engineer-repair/extract')))
REPAIR_READ_BEGIN, REPAIR_READ_END = 0x669C59, 0x669CA9


class NativeAudioPlatform:
    """Raw-file/Win32/DirectSound transport; every native audio decision runs.

    Opaque device buffers are byte storage. Legacy controls keep zero cursors;
    configured controls admit explicit OS frequency/counter/status/cursors and
    immutable file bytes. Native409360 clock math, worker4095B0, codecs, queues
    and callbacks execute. Caller visits retain the worker across the original
    Sleep40983E seam through this owner's callsite transport; real concurrency
    and hardware progression are excluded. CRT malloc/free use the existing
    owning arena hook. See _factory_infantry_output/consumer-meta.json.
    """
    def __init__(self, owner, root):
        self.owner, self.root = owner, root
        self.files, self.calls, self.methods, self.buffers = {}, [], {}, {}
        self.cursor, self.hardware_mapped = 0x32010000, False
        self.prepared_files = MappingProxyType({})
        self.os_clock, self.os_device = None, None
        self.os_buffer_devices = None
        self.critical_calls = frozenset()
        self.clock_calls, self.file_io, self.device_io = [], [], []
        self.device_stop_updates_status = False

    def configure_transport(self, *, prepared_files=None, clock=None,
                            device=None, buffer_devices=None, critical_calls=None):
        """Admit raw OS inputs; native readers, clocks and consumers still run.

        Files are immutable bytes selected/pinned by the caller. Clock values
        are raw QPF/QPC inputs, not converted milliseconds. Device values are
        explicit OS status/cursors. Optional per-buffer inputs are keyed only
        by device-buffer identities returned by this transport; unlisted
        buffers retain the configured global behavior. Existing controls retain their zero-cursor
        and unsupported-clock defaults unless these inputs are configured.
        """
        if prepared_files is not None:
            values = {str(name).lower(): bytes(raw) for name, raw in prepared_files.items()}
            if len(values) != len(prepared_files):
                raise ValueError('Case-colliding prepared RawFile names')
            self.prepared_files = MappingProxyType(values)
        if clock is not None:
            self.os_clock = clock
        if device is not None:
            self.os_device = device
        if buffer_devices is not None:
            if not set(buffer_devices) <= set(self.buffers):
                raise ValueError('Per-buffer OS inputs require returned device-buffer identities')
            self.os_buffer_devices = buffer_devices
        if critical_calls is not None:
            self.critical_calls = frozenset(critical_calls)

    def string(self, pointer):
        raw = bytearray()
        while (value := bytes(self.owner.u.mem_read(pointer + len(raw), 1))) != b'\0':
            raw.extend(value)
        return raw.decode('latin1')

    def callsite(self, pc, size, args, value=0):
        u = self.owner.u
        sp = u.reg_read(UC_X86_REG_ESP)
        self.calls.append(dict(pc=f'0x{pc:08X}', args=args, result=value))
        u.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_ESP, sp + len(args) * 4)
        u.reg_write(UC_X86_REG_EIP, pc + size)

    def interface(self, kind, length=0):
        u = self.owner.u
        if not self.hardware_mapped:
            u.mem_map(0x32000000, 0x400000)
            self.hardware_mapped = True
        pointer = self.cursor
        self.cursor += 0x100
        vtable = 0x32001000 if kind == 'dsound' else 0x32002000
        u.mem_write(pointer, dwords(vtable))
        for index in range(25):
            method = vtable + 0x200 + index * 4
            u.mem_write(vtable + index * 4, dwords(method))
            self.methods[method] = kind, index
        if kind == 'buffer':
            data = self.cursor
            self.cursor += (length + 0xFFF) & ~0xFFF
            assert self.cursor < 0x32400000
            self.buffers[pointer] = data, length
        return pointer

    def hook(self, u, pc, size):
        sp, read = u.reg_read(UC_X86_REG_ESP), self.owner.read32
        if self.os_clock is not None and pc in (0x409368, 0x4093C8):
            pointer = read(sp)
            value = self.os_clock['frequency' if pc == 0x409368 else 'counter']
            if not 0 <= value < 1 << 64:
                raise ValueError('QPF/QPC input outside unsigned QWORD')
            u.mem_write(pointer, struct.pack('<Q', value))
            self.clock_calls.append(dict(phase=self.owner.phase, pc=f'0x{pc:08X}',
                kind='QueryPerformanceFrequency' if pc == 0x409368 else 'QueryPerformanceCounter',
                destination=pointer, value=value, os_success=1))
            self.callsite(pc, size, [pointer], 1)
            return True
        if pc in self.critical_calls or ((self.os_device is not None or self.os_buffer_devices is not None)
                                       and pc in (0x4095F5, 0x409828)):
            args = [read(sp)]
            if pc in (0x4095F5, 0x409828):
                self.device_io.append(dict(phase=self.owner.phase, pc=f'0x{pc:08X}',
                    kind='EnterCriticalSection' if pc == 0x4095F5 else 'LeaveCriticalSection',
                    args=args, single_threaded=True))
            self.callsite(pc, size, args, 0)
            return True
        if pc in (0x7C9430, 0x7C93E8):
            u.reg_write(UC_X86_REG_EIP, 0x7C8E17 if pc == 0x7C9430 else 0x7C8B3D)
            return True
        if pc in (0x65CC59, 0x65CBBB):
            args = list(struct.unpack('<7I', u.mem_read(sp, 28)))
            requested_name = self.string(args[0])
            name = requested_name.lower()
            if name not in ('audio.idx', 'audio.bag') and name not in self.prepared_files:
                raise ValueError('Unadmitted RawFile name: ' + name)
            handle = len(self.files) + 1
            raw = self.prepared_files[name] if name in self.prepared_files else (self.root / name).read_bytes()
            self.files[handle] = dict(name=name, raw=raw, position=0)
            if name in self.prepared_files:
                self.file_io.append(dict(phase=self.owner.phase, pc=f'0x{pc:08X}',
                    kind='CreateFile', name=requested_name, args=args, result=handle,
                    bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(), prepared_physical_winner=True))
            self.callsite(pc, 6, args, handle)
            return True
        if pc in (0x65CC31, 0x65CC6F, 0x65CA17, 0x65CCB0):
            args = [read(sp)]
            assert args[0] in self.files
            self.callsite(pc, 6, args, 1)
            return True
        if pc == 0x65CD5D:
            args = list(struct.unpack('<5I', u.mem_read(sp, 20)))
            handle, destination, length, read_pointer, overlap = args
            assert not overlap
            entry = self.files[handle]
            position = entry['position']
            raw = entry['raw'][entry['position']:entry['position'] + length]
            entry['position'] += len(raw)
            if raw:
                u.mem_write(destination, raw)
            u.mem_write(read_pointer, dwords(len(raw)))
            self.callsite(pc, 6, args, 1)
            if self.prepared_files:
                self.file_io.append(dict(phase=self.owner.phase, pc=f'0x{pc:08X}',
                    kind='ReadFile', name=entry['name'], args=args, offset_before=position,
                    offset_after=entry['position'], reported_bytes=len(raw), returned_bytes_hex=raw.hex(),
                    source_sha256=hashlib.sha256(entry['raw']).hexdigest()))
            return True
        if pc in (0x65CF8B, 0x65CFD4, 0x65D030, 0x65D0A5):
            args = list(struct.unpack('<4I', u.mem_read(sp, 16)))
            handle, offset, high, origin = args
            assert high == 0
            offset = offset if offset < 0x80000000 else offset - 0x100000000
            entry = self.files[handle]
            entry['position'] = (0, entry['position'], len(entry['raw']))[origin] + offset
            assert entry['position'] >= 0
            self.callsite(pc, 6 if pc == 0x65D0A5 else 2, args, entry['position'])
            return True
        if pc == 0x65D0EC:
            args = list(struct.unpack('<2I', u.mem_read(sp, 8)))
            assert args[1] == 0
            self.callsite(pc, 6, args, len(self.files[args[0]]['raw']))
            return True
        if pc in (0x40947F, 0x409511, 0x409548, 0x40A785, 0x40A795):
            count = {0x40947F: 1, 0x409511: 6, 0x409548: 2, 0x40A785: 1, 0x40A795: 1}[pc]
            args = list(struct.unpack('<' + str(count) + 'I', u.mem_read(sp, count * 4)))
            if pc == 0x409511:
                u.mem_write(args[5], dwords(1))
            self.callsite(pc, 6, args, 1 if pc in (0x409511, 0x409548) else 0)
            return True
        if pc == 0x7C89DA:
            null, output, outer = struct.unpack('<3I', u.mem_read(sp + 4, 12))
            assert (null, outer) == (0, 0)
            u.mem_write(output, dwords(self.interface('dsound')))
            self.owner.ret(0, 12)
            return True
        if pc == 0x4068E0:
            self.owner.ret()  # cdecl debug-log transport.
            return True
        if pc not in self.methods:
            return False
        kind, method = self.methods[pc]
        count = {('dsound', 3): 4, ('dsound', 4): 2, ('dsound', 6): 3}.get(
            (kind, method), {2: 1, 4: 3, 9: 2, 11: 8, 12: 4, 13: 2, 14: 2,
                             15: 2, 16: 2, 17: 2, 18: 1, 19: 5}.get(method))
        assert count is not None, (kind, method)
        args = list(struct.unpack('<' + str(count) + 'I', u.mem_read(sp + 4, count * 4)))
        device = (self.os_buffer_devices.get(args[0], self.os_device)
                  if self.os_buffer_devices is not None else self.os_device)
        if kind == 'dsound' and method == 3:
            _, descriptor, output, outer = args
            assert outer == 0
            u.mem_write(output, dwords(self.interface('buffer', read(descriptor + 8) or 4096)))
        elif kind == 'dsound' and method == 4:
            assert read(args[1]) >= 4  # zero-filled capabilities supplied by device.
        elif kind == 'buffer' and method == 4:
            cursors = [device['play_cursor'], device['write_cursor']] if device else [0, 0]
            u.mem_write(args[1], dwords(cursors[0]))
            u.mem_write(args[2], dwords(cursors[1]))
            if device is not None:
                self.device_io.append(dict(phase=self.owner.phase, pc=f'0x{pc:08X}',
                    caller=f'0x{read(sp):08X}', kind='IDirectSoundBuffer::GetCurrentPosition',
                    args=args, play_cursor=cursors[0], write_cursor=cursors[1], os_success=0))
                self.calls.append(dict(method='buffer:4', args=args, result=0, os_cursors=cursors))
                self.owner.ret(0, count * 4)
                return True
        elif kind == 'buffer' and method == 9:
            if device is None:
                raise ValueError('GetStatus requires explicit OS device input')
            status = device['value']
            u.mem_write(args[1], dwords(status))
            self.device_io.append(dict(phase=self.owner.phase, pc=f'0x{pc:08X}',
                caller=f'0x{read(sp):08X}', kind='IDirectSoundBuffer::GetStatus', args=args,
                status=status, os_success=0))
            self.calls.append(dict(method='buffer:9', args=args, result=0, os_status=status))
            self.owner.ret(0, count * 4)
            return True
        elif kind == 'buffer' and method == 18:
            # Original40A62B pushes only its COM receiver before vt+48.
            self.calls.append(dict(method='buffer:18', args=args, result=0,
                                   abi='original40A62B_receiver_only'))
            if self.device_stop_updates_status:
                if device is None:
                    raise ValueError('Stop status transition needs an OS device')
                device['value'] = 0
                self.device_io.append(dict(phase=self.owner.phase, pc=f'0x{pc:08X}',
                    caller=f'0x{read(sp):08X}', kind='IDirectSoundBuffer::Stop', args=args,
                    os_success=0, os_playing_status_after=0))
            self.owner.ret(0, count * 4)
            return True
        elif kind == 'buffer' and method == 11:
            this, offset, length, out1, len1, out2, len2, flags = args
            data, capacity = self.buffers[this]
            assert offset < capacity
            length = capacity if flags & 2 else length
            first = min(length, capacity - offset)
            second = length - first
            assert second <= capacity
            u.mem_write(out1, dwords(data + offset))
            u.mem_write(len1, dwords(first))
            u.mem_write(out2, dwords(data if second else 0))
            u.mem_write(len2, dwords(second))
        self.calls.append(dict(method=f'{kind}:{method}', args=args, result=0))
        self.owner.ret(0, count * 4)
        return True


class EngineerInputs(bridge_child_sound.Sound):
    def hook(self, u, pc, size, data):
        if getattr(self, 'platform_audio', None) and self.platform_audio.hook(u, pc, size):
            return
        if pc in (0x527AF9, 0x527B0C):
            # Reader owns these two Windows GUID import transports. Do not
            # shadow its method with the superseded fake Mission observer.
            if not hasattr(self, 'guid_transport_events'):
                self.guid_transport_events = []
            handled = self.guid_transport(u, pc, u.reg_read(UC_X86_REG_ESP),
                                          self.guid_transport_events)
            assert handled
            return
        probe = getattr(self, 'reference_probe', None)
        if probe is not None:
            if pc == 0x528A10:
                sp = u.reg_read(UC_X86_REG_ESP)
                section, key, default, destination, capacity = struct.unpack('<5I', u.mem_read(sp + 4, 20))
                probe['read_string'] = dict(section=self.string(section), key=self.string(key),
                                           default=self.string(default), capacity=capacity)
                probe['destination'] = destination
            elif pc == 0x669C86:
                probe['read_string'].update(result=u.reg_read(UC_X86_REG_EAX),
                                             buffer=self.string(probe.pop('destination')))
            elif pc == 0x7514D0:
                probe['voc_find_input'] = self.string(u.reg_read(UC_X86_REG_ECX))
            elif pc == 0x669C93:
                result = u.reg_read(UC_X86_REG_EAX)
                probe['voc_find_result'] = result if result < 0x80000000 else result - 0x100000000
        if pc == 0x4015C0:
            # Retain the full original binary lookup, unlike the historical
            # Explosion-only Sound fixture's local two-name sample mapping.
            return
        if not 0x400000 <= pc < 0x7E1000 and pc != RET_MAGIC:
            raise AssertionError(('non_native_input_code', f'0x{pc:08X}'))
        super().hook(u, pc, size, data)


def read_repair_reference(inputs, sections):
    inputs.make_ini(sections)
    u = inputs.u
    u.mem_write(SP, bytes(0x300))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ESI, inputs.rules)
    u.reg_write(UC_X86_REG_EDI, INI)
    u.reg_write(UC_X86_REG_EAX, inputs.read32(inputs.rules + 0x1C0))
    inputs.reference_probe = {}
    run_checked(u, REPAIR_READ_BEGIN, REPAIR_READ_END)
    inputs.last_reference_probe = inputs.reference_probe
    inputs.reference_probe = None
    return struct.unpack('<i', u.mem_read(inputs.rules + 0x1C4, 4))[0]


def prepare_joined_inputs(root):
    # Construction dynamically imports Sound, so this selects an extension of
    # that one reader/allocator owner. No existing primitive is changed.
    original = bridge_child_sound.Sound
    bridge_child_sound.Sound = EngineerInputs
    try:
        inputs = construction.joined_inputs(root)
    finally:
        bridge_child_sound.Sound = original
    u, read = inputs.u, inputs.read32
    inputs.platform_audio = NativeAudioPlatform(inputs, root)
    inputs.constructor_repair_sound = struct.unpack('<i', u.mem_read(inputs.rules + 0x1C4, 4))[0]
    # Original physical IDX constructor includes the actual CRT qsort; its
    # 2,285 entries need a longer instruction/time budget than small readers.
    u.mem_write(SP, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ECX, inputs.cstring('audio'))
    u.reg_write(UC_X86_REG_EDX, 0)
    run_checked(u, 0x4011C0, RET_MAGIC, count=200_000_000, timeout_us=50_000_000,
                required_addresses=(0x4011C0, 0x473D10, 0x7C8B48))
    inputs.audio_index = u.reg_read(UC_X86_REG_EAX)
    assert inputs.audio_index
    u.mem_write(0x87E294, dwords(inputs.audio_index))
    physical_sound = bridge_child_sound.sections((root / 'SOUNDMD.INI').read_bytes())
    selected = {name: physical_sound[name] for name in ('Defaults', 'Dummy', 'BuildingRepaired')}
    selected['SoundList'] = {key: value for key, value in physical_sound['SoundList'].items()
                             if value in ('Dummy', 'BuildingRepaired')}
    inputs.make_ini(selected)
    inputs.invoke(0x4072C0, 0x87E250)
    u.mem_write(0xB1D378, dwords(0x7EB6D4, inputs.alloc(128), 32, 1, 0, 10))
    inputs.invoke(0x7510D0, INI)
    inputs.sound_rows = []
    for name in ('Dummy', 'BuildingRepaired'):
        index = inputs.invoke(0x7514D0, inputs.cstring(name))
        pointer = read(read(read(0xB1D37C) + index * 4))
        samples = [read(pointer + 0xB4 + i * 4) for i in range(read(pointer + 0x134))]
        inputs.sound_rows.append(dict(name=name, fixture_index=index, control=read(pointer + 0x10),
            type_flags=read(pointer + 0x14), volume_fixed16=read(pointer + 0x1C),
            priority=read(pointer + 0x40), limit=read(pointer + 0x48),
            delay=list(struct.unpack('<2i', u.mem_read(pointer + 0x58, 8))),
            pitch=list(struct.unpack('<2i', u.mem_read(pointer + 0x60, 8))),
            volume_shift=read(pointer + 0x68), sample_indices=samples))
    inputs.reference_controls = []
    for label, section, key, value in (
        ('stock_name', 'AudioVisual', 'BuildingRepairedSound', 'BuildingRepaired'),
        ('absent_layer', 'AudioVisual', None, None),
        ('empty_value', 'AudioVisual', 'BuildingRepairedSound', ''),
        ('unknown_name', 'AudioVisual', 'BuildingRepairedSound', 'UnknownEngineerRepair'),
        ('mixed_case_name', 'AudioVisual', 'BuildingRepairedSound', 'bUiLdInGrEpAiReD'),
        ('different_key_case', 'AudioVisual', 'buildingrepairedsound', 'Dummy'),
        ('different_section_case', 'audiovisual', 'BuildingRepairedSound', 'Dummy'),
        ('capacity_127', 'AudioVisual', 'BuildingRepairedSound', 'x' * 127),
        ('capacity_128', 'AudioVisual', 'BuildingRepairedSound', 'x' * 128),
        ('capacity_129', 'AudioVisual', 'BuildingRepairedSound', 'x' * 129),
        ('dummy_replacement', 'AudioVisual', 'BuildingRepairedSound', 'Dummy'),
        ('unknown_after_replacement', 'AudioVisual', 'BuildingRepairedSound', 'UnknownEngineerRepair'),
    ):
        before = struct.unpack('<i', u.mem_read(inputs.rules + 0x1C4, 4))[0]
        result = read_repair_reference(inputs, {section: {} if key is None else {key: value}})
        inputs.reference_controls.append(dict(name=label, section=section, key=key, raw=value,
                                               before=before, after=result, **inputs.last_reference_probe))
    # Native constructors establish defaults independently of retail values.
    u.mem_write(0xA8E348, dwords(0x7EB6D4, inputs.alloc(128 * 4), 128, 1, 0, 10))
    engineer = inputs.alloc(0x1900)
    inputs.invoke(0x5236A0, engineer, (inputs.cstring('ENGINEER'),))
    inputs.types['ENGINEER'] = engineer
    default_building = inputs.alloc(0x2000)
    inputs.invoke(0x45DD90, default_building, (inputs.cstring('ENGINEER_INPUT_CONTROL'),))
    country = inputs.alloc(0x400)
    u.mem_write(0xA83C98, dwords(0x7EB6D4, inputs.alloc(128 * 4), 128, 1, 0, 10))
    inputs.invoke(0x5113F0, country, (inputs.cstring('Americans'),))
    inputs.country = country
    inputs.defaults = dict(engineer=u.mem_read(engineer + 0xEC3, 1)[0],
        capturable=u.mem_read(default_building + 0x1572, 1)[0],
        repairable=u.mem_read(default_building + 0xCCC, 1)[0],
        can_be_occupied=u.mem_read(default_building + 0x157B, 1)[0],
        bridge_repair_hut=u.mem_read(default_building + 0x16B6, 1)[0],
        multiplay_passive=u.mem_read(country + 0x1A6, 1)[0],
        engineer_capture_level_f32_bits=bytes(u.mem_read(inputs.rules + 0x17F8, 4)).hex(),
        country_cost_f32_bits=bytes(u.mem_read(country + 0x114, 20)).hex())
    all_art = bridge_child_sound.sections((root / 'ARTMD.INI').read_bytes())
    art = {name: all_art[name] for name in (*construction.JOINED_ART_NAMES, 'ENGINEER', 'EngineerSequence')
           if name in all_art}
    land_names = [inputs.string(read(0x839D68 + i * 4)) for i in range(12)]
    inputs.engineer_layers = []
    for filename in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'Hills.mmx'):
        path = root / filename
        if not path.exists():
            assert filename == 'LANGRULE.INI'
            inputs.engineer_layers.append(dict(file=filename, absent=True))
            continue
        physical = bridge_child_sound.sections(path.read_bytes())
        retained = {name: physical[name] for name in ('ENGINEER', 'General', 'AudioVisual', 'Americans', *land_names)
                    if name in physical}
        inputs.make_ini(retained)
        rule_ini = inputs.alloc(0x40)
        u.mem_write(rule_ini, bytes(u.mem_read(INI, 0x40)))
        inputs.make_ini(art)
        inputs.invoke(0x674000, 0, (rule_ini,))
        mark = len(inputs.asset_loaded)
        admitted = inputs.invoke(0x5240A0, engineer, (rule_ini,)) & 255
        # Country MultiplayPassive reader with original retained constructor
        # default. Other country/Side mechanisms are supplied prior state.
        u.reg_write(UC_X86_REG_EBX, country)
        u.reg_write(UC_X86_REG_ESI, rule_ini)
        u.reg_write(UC_X86_REG_EDI, country + 0x24)
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, 0x511A75, 0x511A95)
        u.reg_write(UC_X86_REG_ESI, inputs.rules)
        u.reg_write(UC_X86_REG_EDI, rule_ini)
        u.reg_write(UC_X86_REG_EAX, read(inputs.rules + 0x1C0))
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, REPAIR_READ_BEGIN, REPAIR_READ_END)
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, 0x671DF1, 0x671E16)
        inputs.engineer_layers.append(dict(file=filename, absent=False,
            sha256=hashlib.sha256(path.read_bytes()).hexdigest(), read_admitted=bool(admitted),
            strength=read(engineer + 0xA0), engineer=u.mem_read(engineer + 0xEC3, 1)[0],
            country_multiplay_passive=u.mem_read(country + 0x1A6, 1)[0],
            repair_sound=struct.unpack('<i', u.mem_read(inputs.rules + 0x1C4, 4))[0],
            engineer_capture_level_f32_bits=bytes(u.mem_read(inputs.rules + 0x17F8, 4)).hex(),
            locomotor_guid=bytes(u.mem_read(engineer + 0x34C, 16)).hex(),
            physical_keys={name: retained.get(name) for name in ('ENGINEER', 'AudioVisual', 'Americans')},
            asset_loads=inputs.asset_loaded[mark:]))
    # Execute original type-registration tail against prior spare storage, so
    # capture counters use a native-produced selected local ArrayIndex.
    u.mem_write(0xA83C68, dwords(0x7EB6D4, inputs.alloc(128 * 4), 128, 1, 0, 10))
    u.reg_write(UC_X86_REG_ESI, inputs.types['GAPOWR'])
    u.reg_write(UC_X86_REG_EBX, 0)
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, 0x45E2ED, 0x45E362)
    pointer = inputs.types['GAPOWR']
    inputs.building_entry = dict(capturable=u.mem_read(pointer + 0x1572, 1)[0],
        repairable=u.mem_read(pointer + 0xCCC, 1)[0],
        can_be_occupied=u.mem_read(pointer + 0x157B, 1)[0],
        bridge_repair_hut=u.mem_read(pointer + 0x16B6, 1)[0],
        dont_score=u.mem_read(pointer + 0xC9F, 1)[0],
        insignificant=u.mem_read(pointer + 0x232, 1)[0],
        cost=read(pointer + 0x610), native_array_index=read(pointer + 0xDF8))
    from tools.native_oracle import image_bytes, _sections
    raw = image_bytes()
    inputs.code_identity = []
    for rva, source, length, _, flags in _sections(raw):
        if not length or not flags & 0x20000000:
            continue
        original = raw[source:source + length]
        assert bytes(u.mem_read(0x400000 + rva, length)) == original
        inputs.code_identity.append(dict(address=f'0x{0x400000 + rva:08X}', length=length,
                                         sha256=hashlib.sha256(original).hexdigest()))
    return inputs


class EngineerJoinedFixture(construction.JoinedFixture):
    WATCH = {
        0x517A50: 'infantry_ctor', 0x51DFF0: 'infantry_unlimbo',
        0x51E49E: 'ordinary_action_suffix', 0x51F1A4: 'resolved_action_dispatch',
        0x4D74E0: 'capture_command', 0x6FFBE0: 'queue_megamission',
        0x4C6860: 'event_constructor', 0x4C6CB0: 'event_delivery',
        0x5B3570: 'mission_commence', 0x75AEC0: 'walk_process',
        0x519630: 'infantry_per_cell', 0x565730: 'physical_cell_lookup',
        0x47C520: 'first_ground_building', 0x4F9A90: 'current_alliance',
        0x701410: 'full_engineer_repair', 0x451EE0: 'damaged_slot_transition',
        0x446FF0: 'paid_repair_toggle', 0x448260: 'building_change_owner',
        0x7014A0: 'techno_change_owner', 0x4FF980: 'remove_owned',
        0x4FFA50: 'add_owned', 0x4FD150: 'house_building_membership',
        0x51D0D0: 'infantry_scatter', 0x51AA40: 'infantry_destination',
        0x4D85D0: 'foot_per_cell_tail', 0x4DE5D0: 'foot_uninit',
        0x5F65F0: 'object_uninit', 0x7258D0: 'deferred_expiry',
        0x51DF10: 'infantry_limbo', 0x4DB260: 'foot_limbo',
        0x6F6AC0: 'techno_limbo', 0x5F4D30: 'object_limbo',
        0x725C70: 'deferred_drain', 0x523350: 'infantry_delete',
        0x517D90: 'infantry_destructor', 0x75BE3C: 'walk_after_arrival_callback',
        0x75BE4D: 'walk_consumed_exit', 0x75BF6B: 'walk_survivor_mark_put',
        0x7509E0: 'repair_sound_play', 0x405190: 'sound_event_allocate',
        0x4041D0: 'sound_scheduler', 0x4055C0: 'sound_event_update',
        0x4035F0: 'native_channel_selection', 0x4048B0: 'sound_load_samples',
        0x4015C0: 'audio_index_lookup', 0x4016F0: 'audio_index_open_sample',
        0x4018C0: 'audio_index_read_sample', 0x404700: 'sound_playlist',
        0x4054A0: 'sound_start', 0x40A340: 'native_backend_start',
        0x40AA70: 'native_ima_decode', 0x508C30: 'house_power_recompute',
        0x44E7B0: 'building_power_output', 0x440042: 'building_health_sample',
        0x4F84D9: 'house_power_dirty_gate',
        0x702D40: 'capture_null_source_record_kill',
        0x5025F0: 'remove_live_quantity', 0x502A80: 'add_live_quantity',
        0x4FF550: 'remove_tracking', 0x4FF700: 'add_tracking',
        0x7015C9: 'capture_score_store', 0x70164D: 'capture_kill_count_store',
        0x50BF60: 'factory_plant_factor_fold', 0x45EDD0: 'building_cost_of',
        0x711F00: 'techno_type_cost_of',
        0x4576F0: 'building_infantry_evacuation',
        0x6F4960: 'techno_discovery',
    }

    def __init__(self, inputs, *, seed=31, arena_size=0x200000):
        self.phase, self.trail, self.trace = 'setup', deque(maxlen=40), []
        self.platform_audio = None
        super().__init__(inputs, seed=seed, arena_size=arena_size)
        u = self.u
        u.hook_add(UC_HOOK_MEM_INVALID, self.invalid)
        u.hook_add(UC_HOOK_MEM_WRITE, self.write)
        # The same original Object translation-unit CRT table exercised by
        # object_flight_height.py. A cold AC13C8=0 makes a marked ground
        # Building at height0 report AIR during ChangeOwner's Mark(3), so
        # PlaceDown skips its ground membership and suppresses evacuation.
        # Execute the actual startup producers instead of supplying a layer,
        # a height, a scalar calculation or a Scatter admission answer.
        self.phase = 'object_crt_startup'
        initializers = struct.unpack('<14I', u.mem_read(0x8141D8, 56))
        height_globals = (0xAC13C8, 0xAC13BC)
        self.original_object_startup = dict(
            table='0x008141D8', initializers=[f'0x{entry:08X}' for entry in initializers],
            before={f'0x{address:08X}': self.read32(address) for address in height_globals})
        for entry in initializers:
            construction.invoke(u, entry, 0)
        self.original_object_startup['after'] = {
            f'0x{address:08X}': self.read32(address) for address in height_globals}
        assert [self.read32(address) for address in height_globals] == [104, 416]
        self.phase = 'setup'
        for address in (0xA83DE8, 0xA8EC78, 0xB0F6C8, 0x8B3DC0, 0xB0F5D8):
            u.mem_write(address, dwords(0x7EB6D4, self.heap, 128, 1, 0, 10))
            self.heap += 0x200
        for address, length in ((0xA8E348, 24), (0xA83C68, 24), (0xA83C98, 24), (0x89EA40, 12 * 36)):
            u.mem_write(address, bytes(inputs.u.mem_read(address, length)))
        u.mem_write(HOUSE + 0x34, dwords(inputs.country))
        u.mem_write(0x887324, dwords(construction.JOINED_MEMORY + 0x5000))
        u.mem_write(0x87F924, dwords(0xC00000))
        u.mem_map(0x31000000, 0x600000)
        u.mem_write(0x87F7E8 + 0x68, dwords(0x31000000, 0x40000, 0x31100000))
        for entry in (0x561710, 0x5617A0, 0x5617C0, 0x5617E0):
            construction.invoke(u, entry, 0)
        u.mem_map(0, 0x1000)
        u.mem_write(0, dwords(-1))  # Win32 SEH prior empty chain.
        for entry in (0x40B540, 0x54E260, 0x633900):
            construction.invoke(u, entry, 0)
        construction.invoke(u, 0x65C6D0, 0xABE890, 31)
        u.mem_write(0xA83D4C, dwords(HOUSE))
        u.mem_write(HOUSE + 0x1ED, b'\1')
        items = self.heap
        self.heap += 0x20
        u.mem_write(items, dwords(HOUSE))
        u.mem_write(0xA8022C, dwords(items))
        u.mem_write(HOUSE + 0x30, dwords(0))
        u.mem_write(HOUSE + 0x5788, dwords(1))
        for offset in (0x5528, 0x5578):
            u.mem_write(HOUSE + offset, dwords(0x7EB6D4, self.heap, 128, 1, 0, 10))
            self.heap += 0x200
        self.platform_audio = NativeAudioPlatform(self, inputs.platform_audio.root)
        self.platform_audio.files = {key: {**entry} for key, entry in inputs.platform_audio.files.items()}
        construction.invoke(u, 0x7C8F5E, 0)
        self.phase = 'audio_device_init'
        construction.invoke(u, 0x402940, 0)
        construction.invoke(u, 0x402AF0, 0x816358)
        self.phase = 'audio_native_format_parent_channels'
        u.mem_write(SP, bytes(0x1000))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_EDI, 0)
        run_checked(u, 0x406BD9, 0x406C3A)
        self.audio_parent = self.read32(0x87E728)
        assert self.audio_parent and self.read32(self.audio_parent + 0xF0) == 16
        self.phase = 'audio_pools'
        u.reg_write(UC_X86_REG_EDX, inputs.audio_index)
        assert construction.invoke(u, 0x403ED0, self.audio_parent) == 1
        # 403ED0 resets the type-list sentinel. The actual constructed Sound
        # objects remain valid prior input; original node insertion reinstalls
        # those same objects. No selected sound fields or samples are supplied.
        for index in range(self.read32(0xB1D388)):
            pointer = self.read32(self.read32(self.read32(0xB1D37C) + index * 4))
            construction.invoke(u, 0x4072D0, pointer)
            u.reg_write(UC_X86_REG_EDX, pointer)
            construction.invoke(u, 0x407420, 0x87E250)
        self.phase = 'setup'

    def invalid(self, u, access, address, size, value, data):
        raise AssertionError(('unmapped_native_access', self.phase, f'0x{u.reg_read(UC_X86_REG_EIP):08X}',
                              f'0x{address:08X}', size, [f'0x{x:08X}' for x in self.trail]))

    def write(self, u, access, address, size, value, data):
        watched = {BLD + offset for offset in (0x544, 0x6C, 0x70, 0x21C, 0x41A, 0x41B, 0x41C)}
        for house in getattr(self, 'owner_houses', (HOUSE,)):
            watched.update(house + offset for offset in (0x5778, 0x5779, 0x5488, 0x548C, 0x54E8, 0x5438, 0x1F4))
        if address in watched:
            self.trace.append(dict(kind='native_write', phase=self.phase, pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}',
                                   address=f'0x{address:08X}', size=size, value=value & ((1 << (8 * size)) - 1)))

    def hook(self, u, pc, size, data):
        self.trail.append(pc)
        if self.platform_audio and self.platform_audio.hook(u, pc, size):
            return
        if pc == 0x7D140B:
            self.ret(HEAP + 0x8000)
            return
        if pc in self.WATCH:
            self.trace.append(dict(kind=self.WATCH[pc], phase=self.phase, pc=f'0x{pc:08X}',
                                   this=u.reg_read(UC_X86_REG_ECX)))
            if pc == 0x7015C9:
                self.trace[-1].update(price=u.reg_read(UC_X86_REG_EAX),
                                      priced_old_house=self.read32(BLD + 0x21C))
            if pc == 0x6F4960:
                actor = u.reg_read(UC_X86_REG_ECX)
                sp = u.reg_read(UC_X86_REG_ESP)
                self.trace[-1].update(viewer=self.read32(sp + 4), return_address=self.read32(sp),
                    current_owner=self.read32(actor + 0x21C),
                    discovery=list(u.mem_read(actor + 0x41A, 3)), tag=self.read32(actor + 0x34))
            if pc in (0x519630, 0x51D0D0):
                actor = u.reg_read(UC_X86_REG_ECX)
                interface = self.read32(actor + 0x674)
                self.trace[-1].update(doing=struct.unpack('<i', u.mem_read(actor + 0x6C4, 4))[0],
                    current_mission=struct.unpack('<i', u.mem_read(actor + 0xAC, 4))[0],
                    nav_com=self.read32(actor + 0x5A4),
                    walk_head=list(struct.unpack('<3i', u.mem_read(interface - 4 + 0x28, 12))),
                    # Original Walk75AB30 reads interface+30 (concrete+34).
                    walk_is_moving=u.mem_read(interface + 0x30, 1)[0])
                if pc == 0x51D0D0:
                    sp = u.reg_read(UC_X86_REG_ESP)
                    source, force, no_kidding = struct.unpack('<3I', u.mem_read(sp + 4, 12))
                    self.trace[-1].update(source_coord=list(struct.unpack('<3i', u.mem_read(source, 12))),
                                             force=force, no_kidding=no_kidding)
        if pc == 0x457719:
            pointer = u.reg_read(UC_X86_REG_EAX)
            self.trace.append(dict(kind='building_evacuation_navigation_coordinate', phase=self.phase,
                pc=f'0x{pc:08X}', actor=u.reg_read(UC_X86_REG_ESI),
                coord=list(struct.unpack('<3i', u.mem_read(pointer, 12)))))
        elif pc == 0x45772B:
            actor = u.reg_read(UC_X86_REG_ESI)
            self.trace.append(dict(kind='building_evacuation_physical_building', phase=self.phase,
                pc=f'0x{pc:08X}', actor=actor, first_building=u.reg_read(UC_X86_REG_EAX),
                alive=u.mem_read(actor + 0x90, 1)[0], nav_com=self.read32(actor + 0x5A4)))
        elif pc == 0x51D16E:
            self.trace.append(dict(kind='scatter_original_walk_moving_result', phase=self.phase,
                pc=f'0x{pc:08X}', moving=u.reg_read(UC_X86_REG_EAX) & 255))
        elif pc == 0x51D17B:
            self.trace.append(dict(kind='scatter_original_mission_control', phase=self.phase,
                pc=f'0x{pc:08X}', scatter=u.mem_read(u.reg_read(UC_X86_REG_EAX) + 9, 1)[0],
                effective_force=u.reg_read(UC_X86_REG_EBX) & 255))
        elif pc == 0x51D184:
            self.trace.append(dict(kind='scatter_mission_force_gate', phase=self.phase,
                pc=f'0x{pc:08X}', effective_force=u.reg_read(UC_X86_REG_EBX) & 255))
        if pc in (0x519B43, 0x51A015, 0x519F34):
            self.trace.append(dict(kind='null_tag_gate', phase=self.phase, pc=f'0x{pc:08X}',
                                   tag=u.reg_read(UC_X86_REG_ECX)))
        if pc == 0x6E53A0:
            raise AssertionError('Tag=NULL corpus unexpectedly entered live Tag receiver')
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc in (0x7C978A, 0x6C8C40):
            self.trace.append(dict(kind='platform_registration_or_clock', phase=self.phase, pc=f'0x{pc:08X}'))
            self.ret()
            return
        if pc == 0x53EC9A:
            # The shared VM owns this import and its stdcall cleanup. Input
            # continuations supply SHORT words here; observers never pop the
            # same argument a second time. Unconfigured controls keep their
            # original literal-zero result and exact trace payload.
            input_transport = getattr(self, 'os_input_transport', None)
            key, value = self.read32(sp), 0
            if input_transport is None:
                self.trace.append(dict(kind='platform_no_modifier_key', phase=self.phase, pc=f'0x{pc:08X}'))
            else:
                value = input_transport['key_words'].get(key, 0)
                input_transport['observe'](dict(kind='OS_GetKeyState', pc=f'0x{pc:08X}',
                    key=key, supplied_short=value, sp=sp, returned_sp=sp+4,
                    import_slot=0x7E13E0))
                self.trace.append(dict(kind='platform_key_state', phase=self.phase,
                    pc=f'0x{pc:08X}', key=key, supplied_short=value))
            u.reg_write(UC_X86_REG_EAX, value)
            u.reg_write(UC_X86_REG_ESP, sp + 4)
            u.reg_write(UC_X86_REG_EIP, pc + 6)
            return
        if pc == 0x646F20:
            self.trace.append(dict(kind='platform_event_clock', phase=self.phase, pc=f'0x{pc:08X}'))
            input_transport = getattr(self, 'os_input_transport', None)
            if input_transport is not None:
                input_transport['observe'](dict(kind='OS_timeGetTime', pc=f'0x{pc:08X}',
                    supplied_wall_ms=0, sp=sp, returned_sp=sp, import_slot=0x7E1530))
            u.reg_write(UC_X86_REG_EAX, 0)
            u.reg_write(UC_X86_REG_EIP, pc + 6)
            return
        if pc == 0x517B74:
            clsid, outer, context, iid, output = struct.unpack('<5I', u.mem_read(sp, 20))
            assert outer == 0 and context == 7
            self.trace.append(dict(kind='COM_to_original_walk_factory', phase=self.phase,
                                   clsid=bytes(u.mem_read(clsid, 16)).hex(), iid=bytes(u.mem_read(iid, 16)).hex()))
            u.mem_write(sp, dwords(pc + 6, 0, outer, iid, output))
            u.reg_write(UC_X86_REG_EIP, 0x6C4790)
            return
        if pc == 0x517B83:
            u.reg_write(UC_X86_REG_EAX, 0)
            u.reg_write(UC_X86_REG_ESP, sp + 4)
            u.reg_write(UC_X86_REG_EIP, pc + 6)
            return
        if pc != RET_MAGIC and not any(start <= pc < start + length for start, length in self.code_spans):
            raise AssertionError(('non_original_runtime_code', self.phase, f'0x{pc:08X}',
                                  [f'0x{x:08X}' for x in self.trail]))
        super().hook(u, pc, size, data)

    def rng(self):
        return dict(super().rng(), mapgen=bytes(self.u.mem_read(0xABE890, 0x3F4)).hex())

    def house_prior(self, house, buildings=()):
        u = self.u
        items = self.heap
        self.heap += 0x200
        u.mem_write(house + 0x68, dwords(0x7E4488, items, 128, 1, len(buildings), 10))
        if buildings:
            u.mem_write(items, dwords(*buildings))
        for offset in (0x5500, 0x5550):
            items = self.heap
            self.heap += 0x200
            u.mem_write(house + offset, dwords(0x7E449C, items, 128, 1, 1, 10))
            u.mem_write(items, dwords(len(buildings)))
        # Supplied already-admitted ordinary building quantity. Original
        # Remove_Tracking4FF550/Add_Tracking4FF700 mutate this native total.
        u.mem_write(house + 0x2F0, dwords(len(buildings)))
        # No FactoryPlant membership is an explicit ordinary prior. Its one
        # existing native owner produces the five neutral cost factors.
        u.mem_write(house + 0x150, dwords(0))
        construction.invoke(u, 0x50BF60, house)

    def second_house_prior(self, allied=False):
        u = self.u
        house = self.heap
        self.heap += 0x6000
        u.mem_write(house, bytes(u.mem_read(HOUSE, 0x6000)))
        u.mem_write(house + 0x30, dwords(1))
        u.mem_write(house + 0x1ED, b'\0')
        u.mem_write(house + 0x1EC, b'\0')
        u.mem_write(house + 0x5788, dwords(3 if allied else 2))
        u.mem_write(HOUSE + 0x5788, dwords(3 if allied else 1))
        self.house_prior(HOUSE)
        self.house_prior(house, (BLD,))
        u.mem_write(BLD + 0x21C, dwords(house))
        return house

    def actor(self, start):
        u = self.u
        actor = self.heap
        self.heap += 0x1000
        self.phase = 'infantry_constructor'
        construction.invoke(u, 0x517A50, actor, self.types['ENGINEER'], HOUSE)
        self.phase = 'infantry_unlimbo'
        position = self.heap
        self.heap += 0x20
        u.mem_write(position, dwords(start[0] * 256 + 128, start[1] * 256 + 128, 0))
        assert construction.invoke(u, 0x51DFF0, actor, position, 128) & 255 == 1
        assert self.read32(actor + 0x34) == 0
        return actor

    def ordinary_action(self, actor, target=BLD):
        u = self.u
        self.phase = 'ordinary_action_suffix'
        # Same original interior frame as engineer_bridge_cursor_caller;
        # supplied resolved Building/Foot-action1, no upstream cursor claim.
        u.mem_write(SP, bytes(0x1000))
        u.mem_write(SP + 0x38, dwords(RET_MAGIC, target, 0))
        for register, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EDI, actor),
                                (UC_X86_REG_ESI, target), (UC_X86_REG_EBP, 1)):
            u.reg_write(register, value)
        end = run_checked(u, 0x51E49E, (RET_MAGIC, 0x51E668))
        assert end == RET_MAGIC, f'ordinary suffix entered unexecuted fallback {end:08X}'
        return u.reg_read(UC_X86_REG_EAX)

    def repair_target_prior(self):
        u = self.u
        for y in range(32):
            for x in range(32):
                u.mem_write(cell(x, y) + 0xE4, dwords(0))
                u.mem_write(cell(x, y) + 0x12C, dwords(0x18))
                u.mem_write(cell(x, y) + 0x48, dwords(-1))
        self.building('GAPOWR', BLD, BLD_ITEMS, health=374, coord=(10, 10))
        typ = self.types['GAPOWR']
        self.building_foundation_dimensions = (
            construction.invoke(u, 0x45EC90, typ), construction.invoke(u, 0x45ECA0, typ, 0))
        for y in range(10, 10 + self.building_foundation_dimensions[1]):
            for x in range(10, 10 + self.building_foundation_dimensions[0]):
                u.mem_write(cell(x, y) + 0xE4, dwords(BLD))
        for offset, value in ((0x70, 611), (0x544, 374), (0xAC, 5), (0x534, 1)):
            u.mem_write(BLD + offset, dwords(value))
        u.mem_write(BLD + 0x6E6, b'\1')
        u.mem_write(BLD + 0x6E8, b'\1')
        self.house_prior(HOUSE, (BLD,))
        self.owner_houses = [HOUSE]
        self.phase = 'damaged_art_prior'
        construction.invoke(u, construction.BEGIN_MODE, BLD, 1)
        construction.invoke(u, 0x445F80, BLD, 0)
        # Already admitted target identity; command lookup itself executes.
        count, items = self.read32(0xB0E844), self.read32(0xB0E840)
        u.mem_write(BLD + 0x10, dwords(1000))
        u.mem_write(items + count * 8, dwords(1000, BLD))
        u.mem_write(0xB0E844, dwords(count + 1))
        u.mem_write(0xB0E84C, b'\0')
        self.phase = 'initial_power_consumer'
        construction.invoke(u, 0x508C30, HOUSE)
        u.mem_write(HOUSE + 0x5778, b'\0\0')

    def resolved_command(self, actor, action=29):
        u = self.u
        self.phase = 'resolved_action_command'
        u.mem_write(SP, bytes(0x1000))
        u.mem_write(SP, dwords(0, 0, 0, RET_MAGIC, 0, BLD, 0))
        for register, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EAX, action),
                                (UC_X86_REG_ESI, actor), (UC_X86_REG_EDI, BLD), (UC_X86_REG_EBP, 0)):
            u.reg_write(register, value)
        run_checked(u, 0x51F1A4, RET_MAGIC)
        event = 0xA802D4 + (self.read32(0xA802C8) - 1) * 0x6F
        raw = bytes(u.mem_read(event, 0x1E)).hex()
        construction.invoke(u, 0x4C6CB0, event)
        construction.invoke(u, 0x5B3570, actor)
        return dict(action=action, event_prefix=raw, delivered_mission=self.read32(actor + 0xAC),
                    delivered_destination=self.read32(actor + 0x5A4))

    def walk_arrival(self, actor, y=10):
        u = self.u
        locomotor = self.read32(actor + 0x674) - 4
        head = 10 * 256 + 128, y * 256 + 128, 0
        # Explicit already-paid head/path prior. Original Walk's <17 distance
        # gate, position commit, physical Map lookup and PerCell2 run unchanged.
        u.mem_write(actor + 0x9C, dwords(head[0] - 16, head[1], 0))
        u.mem_write(locomotor + 0x28, dwords(*head))
        u.mem_write(actor + 0x5E0, dwords(-1) * 24)
        construction.invoke(u, 0x75AEC0, locomotor, 0)

    def state(self, actor, owner_houses):
        u, read = self.u, self.read32
        byte = lambda pointer, offset: u.mem_read(pointer + offset, 1)[0]
        signed = lambda pointer, offset: struct.unpack('<i', u.mem_read(pointer + offset, 4))[0]
        # Original Building+48=447AC0 computes GetCoords from Location and
        # the native Foundation dimension readers. This query is not a Rust
        # or hand-calculated Scatter direction input.
        coords = construction.invoke(u, read(read(BLD) + 0x48), BLD, SP + 0x200)
        interface = read(actor + 0x674)
        locomotor = None if not interface else dict(interface=interface,
            head=list(struct.unpack('<3i', u.mem_read(interface - 4 + 0x28, 12))),
            moving=byte(interface, 0x30))
        registry = [list(struct.unpack('<2I', u.mem_read(read(0xB0E840) + i * 8, 8)))
                    for i in range(read(0xB0E844))]
        return dict(game_mode=read(0xA8B238),
            building=dict(actual_hp=read(BLD + 0x6C), estimated_hp=read(BLD + 0x70),
            position=list(struct.unpack('<3i', u.mem_read(BLD + 0x9C, 12))),
            get_coords=list(struct.unpack('<3i', u.mem_read(coords, 12))),
            foundation_index=read(self.types['GAPOWR'] + 0xEF0),
            foundation_dimensions=list(self.building_foundation_dimensions),
            sampled_hp=read(BLD + 0x544), owner=read(BLD + 0x21C),
            discovery=dict(owned_by_current_house=byte(BLD, 0x41A),
                discovered_by_current_house=byte(BLD, 0x41B),
                discovered_by_other_house=byte(BLD, 0x41C)),
            initial_owner_type_index=signed(BLD, 0x338), paid_repair=byte(BLD, 0x6E8),
            damaged_mode=byte(BLD, 0x6E6), slots=self.snapshot(BLD)['anims']),
            engineer=dict(pointer=actor, uid=read(actor + 0x10), actual_hp=read(actor + 0x6C),
            alive=byte(actor, 0x90), limbo=byte(actor, 0x81), marked=byte(actor, 0x74),
            logic_registered=byte(actor, 0x98), current_mission=signed(actor, 0xAC),
            queued_mission=signed(actor, 0xB4), destination=read(actor + 0x5A4), target=read(actor + 0x2B4),
            doing=signed(actor, 0x6C4), locomotor=locomotor,
            position=list(struct.unpack('<3i', u.mem_read(actor + 0x9C, 12))),
            mission_timer=list(struct.unpack('<3i', u.mem_read(actor + 0xC8, 12)))),
            houses=[dict(pointer=house, power=read(house + 0x53A4), drain=read(house + 0x53A8),
                dirty=[byte(house, 0x5778), byte(house, 0x5779)], captured_latch=byte(house, 0x244),
                first_current_viewer_discovery_1f4=byte(house, 0x1F4),
                building_losses=read(house + 0x5488), score=read(house + 0x54E8),
                last_killer_house_index=signed(house, 0x548C),
                building_kills_by_house=[read(house + 0x5438 + i * 4) for i in range(20)],
                country_cost_f32_bits=bytes(u.mem_read(read(house + 0x34) + 0x114, 20)).hex(),
                factory_cost_f32_bits=bytes(u.mem_read(house + 0x5390, 20)).hex(),
                tracked_ordinary_buildings=signed(house, 0x2F0),
                buildings=[read(read(house + 0x6C) + i * 4) for i in range(read(house + 0x78))],
                owned_building_count=read(read(house + 0x5504)), live_building_count=read(read(house + 0x5554)))
                for house in owner_houses],
            pending_delete_count=read(0xB0F6A8), infantry_registry_count=read(0xA83DF8),
            object_uid_registry=registry, rng=self.rng())

    def sound_state(self):
        read, u = self.read32, self.u
        pointer, events = read(0x87E180), []
        while pointer != 0x87E180:
            assert len(events) < 300
            sound = read(pointer + 0x24)
            events.append(dict(pointer=pointer, sound_name=bytes(u.mem_read(sound + 0x6C, 32)).split(b'\0', 1)[0].decode('latin1'),
                state=read(pointer + 0x1C), flags=read(pointer + 0x18), serial=read(pointer + 0x138),
                channel=read(pointer + 0xB0), playlist_result_148=read(pointer + 0x148), pitch=read(pointer + 0x14C),
                volume_shift=read(pointer + 0x150), sample_count=read(pointer + 0xA8),
                controller_hex=bytes(u.mem_read(pointer + 0xB8, 0x80)).hex()))
            pointer = read(pointer)
        return dict(events=events, live_count=read(0x87E28C), frame=read(0xA8ED84),
                    scheduler_reentry=read(0x87E2B4), rng=self.rng())


JOINED_CASES = (
    'own_damaged', 'allied_damaged', 'stale_second_engineer',
    'repair_order_enemy_owner_race', 'enemy_selling_refusal',
    'enemy_warped_refusal', 'missing_physical_building',
    'different_first_physical_building', 'enemy_noncapturable_consumes',
)


def joined_route(inputs, name):
    f = EngineerJoinedFixture(inputs)
    f.repair_target_prior()
    u, read = f.u, f.read32
    if name == 'allied_damaged':
        f.owner_houses.append(f.second_house_prior(allied=True))
    actors = [f.actor((9, 10))]
    if name == 'stale_second_engineer':
        actors.append(f.actor((9, 11)))
    commands = []
    for actor in actors:
        action = f.ordinary_action(actor)
        commands.append(f.resolved_command(actor, action))
    if name in ('repair_order_enemy_owner_race', 'enemy_selling_refusal',
                'enemy_warped_refusal', 'enemy_noncapturable_consumes'):
        f.owner_houses.append(f.second_house_prior(allied=False))
        if name == 'enemy_selling_refusal':
            u.mem_write(BLD + 0xAC, dwords(19))
        elif name == 'enemy_warped_refusal':
            u.mem_write(BLD + 0x270, b'\1')
        elif name == 'enemy_noncapturable_consumes':
            # Explicit asymmetric prior flag, not a retail/default claim.
            u.mem_write(f.types['GAPOWR'] + 0x1572, b'\0')
    if name == 'missing_physical_building':
        for y in range(10, 10 + f.building_foundation_dimensions[1]):
            for x in range(10, 10 + f.building_foundation_dimensions[0]):
                u.mem_write(cell(x, y) + 0xE4, dwords(0))
    elif name == 'different_first_physical_building':
        f.building('GAPOWR', OTHER_BUILDING, OTHER_ITEMS, coord=(10, 10), health=374)
        u.mem_write(OTHER_BUILDING + 0x30, dwords(BLD))
        u.mem_write(cell(10, 10) + 0xE4, dwords(OTHER_BUILDING))
    # Supplied already-settled House dirty flags before this object visit.
    # Initial cached output itself is produced by the complete native owner.
    for house in f.owner_houses:
        construction.invoke(u, 0x508C30, house)
        u.mem_write(house + 0x5778, b'\0\0')
    before = [f.state(actor, f.owner_houses) for actor in actors]
    arrivals = []
    for index, actor in enumerate(actors):
        f.phase = 'physical_walk_arrival' if index == 0 else 'stale_full_health_arrival'
        mark = len(f.trace), len(f.events), len(f.draws), len(f.advances)
        start = f.state(actor, f.owner_houses)
        f.walk_arrival(actor, 10 + index)
        arrivals.append(dict(before=start, after=f.state(actor, f.owner_houses),
            trace=f.trace[mark[0]:], lifecycle=f.events[mark[1]:],
            draws=f.draws[mark[2]:], raw_advances=f.advances[mark[3]:]))
    f.phase = 'next_sound_scheduler_visit'
    sound_before = f.sound_state()
    mark = len(f.trace), len(f.draws), len(f.advances), len(f.platform_audio.calls)
    construction.invoke(u, 0x4041D0, 0)
    audio = dict(before=sound_before, after=f.sound_state(), trace=f.trace[mark[0]:],
                 draws=f.draws[mark[1]:], raw_advances=f.advances[mark[2]:],
                 platform_calls=f.platform_audio.calls[mark[3]:])
    f.phase = 'next_house_visit_before_building_health_sample'
    mark = len(f.trace)
    for house in f.owner_houses:
        u.reg_write(UC_X86_REG_ESI, house)
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, 0x4F84D9, 0x4F84F1)
    consumer_before_sample = f.state(actors[0], f.owner_houses)
    f.phase = 'next_building_health_sample'
    u.reg_write(UC_X86_REG_ESI, BLD)
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, 0x440042, 0x440074)
    after_sample = f.state(actors[0], f.owner_houses)
    f.phase = 'next_house_visit_after_building_health_sample'
    for house in f.owner_houses:
        u.reg_write(UC_X86_REG_ESI, house)
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, 0x4F84D9, 0x4F84F1)
    consumers = dict(before_sample=consumer_before_sample, after_sample=after_sample,
                     after_house=f.state(actors[0], f.owner_houses), trace=f.trace[mark:])
    f.phase = 'deferred_cleanup_drain'
    mark = len(f.trace), len(f.events)
    construction.invoke(u, 0x725C70, 0)
    cleanup = dict(after=[f.state(actor, f.owner_houses) for actor in actors],
                   trace=f.trace[mark[0]:], lifecycle=f.events[mark[1]:])
    assert not f.pending and f.code_unchanged()
    return dict(name=name, commands=commands, before_arrivals=before, arrivals=arrivals,
                original_object_startup=f.original_object_startup,
                audio=audio, power_consumers=consumers, cleanup=cleanup,
                all_native_draws=f.draws, all_raw_advances=f.advances,
                unchanged_executable_sections=True, executed_instruction_count=len(f.executed),
                executed_instruction_pc_sha256=hashlib.sha256(dwords(*sorted(f.executed))).hexdigest())


def action_controls(inputs):
    # Original reader stores f64 ReadDouble into the required f32 field.
    # Native suffix executes the asymmetric equality/above-threshold controls.
    retained = bytes(inputs.u.mem_read(inputs.rules + 0x17F8, 4))
    inputs.make_ini({'General': {'EngineerCaptureLevel': '0.5'}})
    for reg, value in ((UC_X86_REG_ESI, inputs.rules), (UC_X86_REG_EDI, INI), (UC_X86_REG_ESP, SP)):
        inputs.u.reg_write(reg, value)
    run_checked(inputs.u, 0x671DF1, 0x671E16)
    threshold = bytes(inputs.u.mem_read(inputs.rules + 0x17F8, 4)).hex()
    f = EngineerJoinedFixture(inputs)
    f.repair_target_prior()
    actor = f.actor((9, 10))
    enemy = f.second_house_prior(allied=False)
    rows = []
    for health in (374, 375, 376, 750):
        f.u.mem_write(BLD + 0x6C, dwords(health))
        before = f.rng()
        rows.append(dict(actual_hp=health, strength=f.read32(f.types['GAPOWR'] + 0xA0),
                         action=f.ordinary_action(actor), rng_unchanged=before == f.rng()))
    f.u.mem_write(BLD + 0x21C, dwords(HOUSE))
    f.u.mem_write(BLD + 0x6C, dwords(374))
    own_action = f.ordinary_action(actor)
    assert f.code_unchanged()
    inputs.u.mem_write(inputs.rules + 0x17F8, retained)
    return dict(raw_threshold='0.5', stored_threshold_f32_bits=threshold,
                enemy_house=enemy, enemy_rows=rows, own_damaged_action=own_action,
                original_retained_threshold_f32_bits=retained.hex(),
                unchanged_executable_sections=True)


def joined_generate():
    inputs = prepare_joined_inputs(JOINED_ROOT)
    controls = action_controls(inputs)
    files = [dict(name=p.name, size=p.stat().st_size, sha256=hashlib.sha256(p.read_bytes()).hexdigest())
             for p in sorted(JOINED_ROOT.iterdir()) if p.is_file() and p.suffix.lower() != '.json']
    return dict(schema_version=1, source='unicorn/gamemd.exe', native_sha256=NATIVE_SHA256,
        retail_files=files, constructor_defaults=inputs.defaults,
        original_executable_sections=inputs.code_identity,
        constructor_repair_sound=inputs.constructor_repair_sound,
        rules_building_layers=inputs.layers, engineer_layers=inputs.engineer_layers,
        native_building_inputs=inputs.type_rows, native_anim_inputs=inputs.anim_rows,
        native_building_entry_flags=inputs.building_entry,
        selected_animation_list=inputs.animation_list, selected_sound_inputs=inputs.sound_rows,
        original_sound_reference_controls=inputs.reference_controls,
        original_action_controls=controls,
        native_asset_requests=inputs.asset_loaded,
        rules=dict(repair_sound=struct.unpack('<i', inputs.u.mem_read(inputs.rules + 0x1C4, 4))[0],
                   engineer_capture_level_f32_bits=bytes(inputs.u.mem_read(inputs.rules + 0x17F8, 4)).hex(),
                   condition_yellow_f64_bits=bytes(inputs.u.mem_read(inputs.rules + 0x1700, 8)).hex()),
        routes=[joined_route(inputs, name) for name in JOINED_CASES])


def joined_metadata():
    return provenance(scope='Additive ordinary untagged ENGINEER to damaged GAPOWR resolved action/event/Walk/PerCell repair or current-owner capture, sound scheduler, delayed health/power consumers and deferred cleanup',
        assumptions=[
            'Original Rules, selected BuildingType/AnimType/Sound and ENGINEER InfantryType constructors/readers execute from exact-case cached physical RULESMD, optional LANGRULE, MPBattleMD and Hills strings; ARTMD and SOUNDMD are independent inputs. Physical INI/MIX traversal is supplied. Selected registries use fixture-relative native-produced identities; wider type registries are excluded.',
            'Original ENGINEER Infantry517A50 constructor and51DFF0 Unlimbo execute. COM transport selects the actual Walk6C4790 factory from the original parsed locomotor GUID; OleRun, GUID conversion, modifier-key IO and command wall clock are platform boundaries. Country5113F0 constructor and original511A75..511A95 MultiplayPassive reader execute; other country/Side startup remains supplied.',
            'Native runtime modeA8B238=1 is the explicit inherited make_dock_fixture default, read back in every state snapshot. Mode selection/startup is supplied; the runtime CellPUT/discovery/House predicates execute against that mode unchanged.',
            'Original Object translation-unit14CRT entries from table8141D8 execute in table order, as in the existing object_flight_height owner. Native5F37C0 produces AC13C8=104 and5F3860 produces AC13BC=416. No layer, height threshold or scalar result is substituted. This replaces the superseded cold-scalar fixture fault which incorrectly prevented ground Building reinsertion during ChangeOwner.',
            'Ordinary WhatAction51E49E executes an interior suffix with resolved Building, prior Foot action1 and the native frame. Full upstream cursor/base action selection is excluded. Original resolved dispatch51F1A4 through Capture command/event constructor/delivery and Commence execute unchanged; no Repair/Capture decision is answered.',
            'Target is an already admitted damaged ordinary GAPOWR, physical flat32x32 ground cells, native vtables, registered UID1000 and Building/House vector prior. Native45EC90/45ECA0 read the selected ART Foundation2x2 (index3) used for supplied physical membership; original BuildingGetCoords447AC0 supplies each recorded center. Mission/stage constructor stores and sound handle constructors are reused from Construction. Full Building constructor/Unlimbo/map loader and wider House startup are excluded. Actual HP374 versus estimated611 and sampled374 intentionally distinguish the native arrival/repair/consumer authorities.',
            'Native type registration45E2ED..45E362 produces selected GAPOWR ArrayIndex0 with prior spare storage. House owned/live Counter vectors and Building list use original vtables; original50BF60 with supplied empty FactoryPlant membership produces neutral cost factors. Ordinary source Country defaults come from its native constructor.',
            'Original whole Walk75AEC0 receives an already-paid physical head within16 leptons and absent further path heads; Walk position commit, physicalMap565730, full PerCell519630 and terminal disposition execute. Path search/payment and travel to this final head are supplied prior, not claimed complete.',
            'Repair order destination-owner/Selling/warped transitions occur externally while enroute and are supplied race inputs. Arrival reevaluates current alliance, Capturable, Selling and warped predicates. Full448260/7014A0 ChangeOwner, RecordKill702D40, original CostOf, old/new quantities/tracking and House Building membership execute, including score/loss/kill stores. The noncapturable control supplies one false type flag; it is not a retail/default claim.',
            'Owner-race ChangeOwner restores native ground membership before Building4576F0 visits the Engineer registry. Foot+4C returns its current physical coordinate after the Walk head clears. Real Infantry51D0D0(NULL,true,true) runs: original75AB30 still reports moving1 during the PerCell callback, demoting effective force at51D172; native Capture MissionControlScatter0 rejects at51D184 before RNG. Constructor-produced Doing-1 and Walk/membership/GetCoords inputs are recorded; full preceding infantry animation/travel is excluded.',
            'ChangeOwner701691 Mark3 reaches original43F691/5683C0/5684BB/47E8A0 then CellPUT47E9DC virtual discovery6F4960 with Building receiver and explicit PlayerPtr viewer before owner store701735. The race first enters with discovery41A/B/C000 and old House1F4=0;6F49D8 writes41B1,6F49DE/EA dirties the still-old House and6F4A25 sets its1F4 latch. Subsequent calls return through already-discovered41B. Snapshot bytes and original writes are recorded; no live tagged continuation is claimed.',
            'Both attached Tags are NULL, as asserted after native Engineer construction and in target prior. Original Building Tag event1 gates519B43/519F34 and Engineer Tag event48 gate51A015 execute in observed order. A nonnull Tag receiver would require live Tag/Trigger/TEvent/TAction/expiry behavior; no live receiver is replaced or certified.',
            'Full701410 repair writes actual and estimated HP, toggles paid repair, replaces damaged ART slot through original Anim lifecycle, then creates the real selected sound event. It does not write sampled Building HP or House power flags. Subsequent Building health sample440042..440074 and House dirty gate4F84D9..4F84F1 are exact original interiors; full508C30/44E7B0 consumers execute. Full Building/House AI scheduling is excluded.',
            'Original physical AudioIndex4011C0 executes CRT qsort over exact IDX bytes and original lookup1394 for UREPAIR. Native device registry, primary format/parent16 channels/pools initialize; whole4041D0 scheduler executes event/channel/sample selection, pitch/volume MainRng draws, BAG reading, original IMA decode and backend start. Hardware buffers transport bytes with supplied initial playback cursors0; clock/critical section/thread/DirectSound device output are platform boundaries, not audible equivalence.',
            'Whole UnInit/limbo/expiry/deferred725C70 drain and Infantry/Foot/Techno/Object/Walk cleanup execute. Draw requests retain callsite, order and complete before/after state; all Main/Scenario/Mapgen buffers and underlying raw advances are saved. PC53/chop FPCW0E7F is asserted by the reused fixture. Every PE executable section is checked against original bytes before and after execution; runtime PCs outside original code or explicit platform interfaces are rejected.',
        ],
        substitutions=[
            'One existing Construction arena/map/ART/Anim/RNG owner and existing Sound/Reader lexical/input arena owner are composed; Mission owns GUID platform conversion. Operator new/delete use existing recorded arena allocation/release; wider native CRT allocator/TLS are supplied.',
            'Construction presentation451F60/452170/456FB0/705D70 are argument-cleanup sinks. No command, Map/PerCell admission, repair, health, ChangeOwner, lifecycle, sound scheduling, channel/sample selection, numeric or RNG result is supplied.',
            'Win32 physical file seam returns unchanged IDX/BAG bytes and offsets. DirectSound/CriticalSection/CreateThread/device transport returns platform success/storage; worker/device progression after the first playing state is excluded.',
        ],
        entry_points={'rules_ctor': 0x665650, 'repair_sound_read': REPAIR_READ_BEGIN,
            'object_crt_table': 0x8141D8, 'object_level_height_init': 0x5F37C0,
            'object_deck_height_init': 0x5F3860, 'building_get_coords': 0x447AC0,
            'cell_put_discovery_call': 0x47E9DC, 'techno_discovery': 0x6F4960,
            'foundation_width': 0x45EC90, 'foundation_height': 0x45ECA0,
            'capture_level_read': 0x671DF1, 'building_type_ctor': 0x45DD90,
            'building_type_read': 0x45FE50, 'infantry_type_ctor': 0x5236A0,
            'infantry_type_read': 0x5240A0, 'country_ctor': 0x5113F0,
            'country_passive_read': 0x511A75, 'land_read': 0x674000,
            'ordinary_action_suffix': 0x51E49E, 'resolved_action_dispatch': 0x51F1A4,
            'capture_command': 0x4D74E0, 'event_ctor': 0x4C6860,
            'event_delivery': 0x4C6CB0, 'mission_commence': 0x5B3570,
            'infantry_ctor': 0x517A50, 'infantry_unlimbo': 0x51DFF0,
            'walk_factory': 0x6C4790, 'walk_process': 0x75AEC0,
            'per_cell': 0x519630, 'physical_cell': 0x565730,
            'repair': 0x701410, 'building_change_owner': 0x448260,
            'techno_change_owner': 0x7014A0, 'record_kill': 0x702D40,
            'scatter': 0x51D0D0, 'building_evacuation': 0x4576F0,
            'walk_is_moving': 0x75AB30, 'uninit': 0x4DE5D0, 'expiry': 0x7258D0,
            'drain': 0x725C70, 'audio_index': 0x4011C0, 'sound_read': 0x750440,
            'sound_play': 0x7509E0, 'sound_scheduler': 0x4041D0,
            'backend_start': 0x40A340, 'building_health_sample': 0x440042,
            'house_power_dirty_gate': 0x4F84D9, 'house_power': 0x508C30,
            'building_power_output': 0x44E7B0})


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    if '--joined' not in argv:
        return primitive_main(argv)
    argv.remove('--joined')
    root = Path(__file__).resolve().parents[2]
    sources = (
        'tools/spatial_oracle/engineer_repair_admission.py',
        'tools/spatial_oracle/building_construction.py', 'tools/native_oracle.py',
        'tools/rules_oracle/bridge_anim_inputs.py', 'tools/rules_oracle/bridge_child_sound.py',
        'tools/rules_oracle/bridge_anim_lists.py', 'tools/projectile_oracle/flat_art.py',
        'tools/spatial_oracle/refinery_dock.py', 'tools/spatial_oracle/track_destination.py',
        'tools/spatial_oracle/unit_entry.py', 'tools/spatial_oracle/unit_scatter_state.py',
        'tools/spatial_oracle/unit_source_scatter.py', 'tools/spatial_oracle/building_body_rules.py',
        'tools/spatial_oracle/map_queries.py',
        'tools/spatial_oracle/anim_bouncer_launch.py', 'tools/spatial_oracle/building_slot_replacement.py',
        'tools/spatial_oracle/anytown_damage/mission.py',
    )
    finish_vectors(joined_generate, Path(__file__).with_name('engineer_repair_joined.json'),
                   provenance=joined_metadata, argv=argv,
                   source_paths={name: root / name for name in sources})


if __name__ == '__main__':
    main()
