"""Bounded Ares EVA catalog, Side tag/binding and signed scalar persistence.

The catalog experiment composes the existing ActionFixture with the shared
Ares mapper/services. The scalar experiment reuses Probe and the existing
ScenarioFixture IStream transport. Neither runs a full startup/save or audio.
"""
import argparse
import os
import struct
from pathlib import Path

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBX,
    UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_ESP)

from tools import native_oracle as native
from tools.native_oracle import finish_vectors, map_pe32_image, provenance, run_checked
from tools.native_oracle import require
from tools.rules_oracle.ares_prerequisites import ARES_SHA256, Probe, identified_bytes

VOICE_CACHE = 0xB1D4C8
RNG_ENTRIES = (0x65C780, 0x65C7E0)


def signed(value):
    return struct.unpack('<i', struct.pack('<I', value & 0xFFFFFFFF))[0]


def read_bind_controls(ares):
    from tools.spatial_oracle.trigger_action_values import (
        ActionFixture, INI, SCENARIO, dwords, initialize_eva,
        return_from_sink)

    f = ActionFixture(stack_base=0x31000000)
    u = f.u
    map_pe32_image(u, ares, image_base=0x10000000, image_size=0xE0000,
                   header_size=0x400)
    original_text = bytes(u.mem_read(0x10001000, 0x92000))
    # Reuse services only, with the existing allocator in this same VM.
    services = Probe.__new__(Probe)
    services.u, services.alloc, services.events = u, f.allocate, []
    services.specific_visits = set()
    reads, pending_reads, cache_writes, rng_calls = [], [], [], []
    side_read_visits = []
    phase_read_visits = []
    extension_ids = {}
    phase = 'initialize'

    def observe(vm, pc, size, data):
        for pending in list(pending_reads):
            if pc == pending['return_address']:
                reads.append({key: value for key, value in pending.items()
                              if key not in ('return_address', 'target')} |
                             dict(output=f.string(pending['target']),
                                  length=u.reg_read(UC_X86_REG_EAX)))
                pending_reads.remove(pending)
        if pc == 0x528A10:
            sp = u.reg_read(UC_X86_REG_ESP)
            section, key, default, target, capacity = [f.read32(sp+4+i*4) for i in range(5)]
            pending_reads.append(dict(section=f.string(section), key=f.string(key),
                default=f.string(default), capacity=capacity, target=target,
                return_address=f.read32(sp)))
        if pc in (0x10076F08, 0x10076F38, 0x10061960):
            Probe.hook(services, vm, pc, size, data)
        elif pc in (0x7C9430, 0x10001B50):
            count = f.read32(u.reg_read(UC_X86_REG_ESP)+4)
            return_from_sink(u, 0, f.allocate(count))
        elif pc == 0x1002B450 and phase == 'side_load_all':
            # Observe the outer hook's real map Find and visit order. Full
            # extension dispatch (including unrelated Side keys) is a sink.
            sp = u.reg_read(UC_X86_REG_ESP)
            extension = u.reg_read(UC_X86_REG_ECX)
            side_read_visits.append(dict(
                side_id=extension_ids[extension],
                ini_is_current=f.read32(sp+4) == INI))
            return_from_sink(u, 4, 1)
        elif pc == 0x1002E450 and phase == 'side_phase_dispatch':
            extension = u.reg_read(UC_X86_REG_ECX)
            phase_read_visits.append(dict(phase=f.integer(extension+4),
                voice_index=f.integer(extension+0x90)))
            # The whole Side reader owns many unrelated keys. This control
            # executes phase/default decisions and observes the reader call.
            return_from_sink(u, 4, 1)
        if pc in RNG_ENTRIES:
            rng_calls.append(dict(entry=f'{pc:08X}', stream=u.reg_read(UC_X86_REG_ECX)))

    def observe_cache(vm, access, address, size, value, data):
        if address == VOICE_CACHE:
            cache_writes.append(dict(phase=phase,
                writer=f'{u.reg_read(UC_X86_REG_EIP):08X}', value=signed(value)))

    u.hook_add(UC_HOOK_CODE, observe)
    u.hook_add(UC_HOOK_MEM_WRITE, observe_cache)

    def call(address, args=(), ecx=0):
        u.mem_write(f.sp, dwords(native.RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ESP, f.sp)
        u.reg_write(UC_X86_REG_ECX, ecx)
        run_checked(u, address, native.RET_MAGIC, count=3_000_000,
                    timeout_us=10_000_000)
        return u.reg_read(UC_X86_REG_EAX)

    def hook(address, **registers):
        frame = f.allocate(0x30)
        u.mem_write(frame, bytes(0x30))
        for name, value in registers.items():
            f.write(frame+{'ecx': 0x20, 'esi': 0xC, 'ebp': 0x10,
                          'esp': 0x14}[name], value)
        return call(address, [frame]), frame

    def tags():
        return [f.string(f.read32(at)) for at in
                range(f.read32(0x100C2100), f.read32(0x100C2104), 4)]

    def columns(event):
        return [f.string(at) for at in
                range(f.read32(event+0x54), f.read32(event+0x58), 9)]

    def filename(event):
        continuation, frame = hook(0x10063920, ecx=event)
        assert continuation == 0x753398
        return f.string(f.read32(frame+0x24))

    initialize_eva(f, {})
    for stream in (SCENARIO+0x218, 0x886B88, 0xABE890):
        f.call(0x65C6D0, [31], ecx=stream)
    rng_before = f.rng_hashes()
    controls = []
    physical = {
        'EVATypes': {'0': 'Allied', '1': 'VoiceA', '2': 'Russian',
                     '3': 'vOICEa', '4': 'VoiceB', '5': 'Yuri', '6': '',
                     '7': 'VoiceA', '8': 'X'*205},
        'DialogList': {'0': 'EVA_TEST', '1': 'eva_test', '2': 'N'*45},
        'EVA_TEST': {'Allied': 'STOCK_A', 'Russian': 'STOCK_R', 'Yuri': 'STOCK_Y',
                     'VoiceA': 'AbCdEf123456789XYZTAIL', 'VoiceB': 'EXTRA_B'}}
    f.ordered_ini(physical)
    phase = 'registration'
    continuation, _ = hook(0x10063660, ecx=INI)
    entries = [f.read32(f.read32(0xB1D4A4)+i*4)
               for i in range(f.integer(0xB1D4B0))]
    controls.append(dict(kind='registration', input=physical,
        tags=tags(), event_names=[f.string(at) for at in entries],
        continuation=f'{continuation:08X}'))
    event = entries[0]
    phase = 'extra_columns'
    returned, _ = hook(0x100639C0, esi=event, ebp=INI)
    controls.append(dict(kind=phase, samples=columns(event), return_value=returned))
    for voice in (0, 1, 2, 3, 4, -1, -2, 6):
        f.write(VOICE_CACHE, voice)
        controls.append(dict(kind='filename', voice_index=voice, filename=filename(event)))

    # Native map operations and actual Find; no supplied association lookup.
    call(0x100576D0, ecx=0x100C1BE8)
    sides = []
    for name in ('GDI', 'Nod', 'ThirdSide', 'SyntheticSide'):
        side = f.allocate(0x60)
        u.mem_write(side, bytes(0x60))
        u.mem_write(side+0x24, name.encode()+b'\0')
        extension = f.allocate(0x280)
        call(0x1002D9F0, [side], ecx=extension)
        extension_ids[extension] = name
        constructor_voice = f.integer(extension+0x90)
        call(0x10057CF0, [side, extension], ecx=0x100C1BE8)
        call(0x1002E190, [INI], ecx=extension)
        require(call(0x10057C70, [side], ecx=0x100C1BE8) == extension, 'native 0x10057C70 returned an unexpected result')
        sides.append((side, extension))
        controls.append(dict(kind='side_default', id=name,
                             constructor_voice_index=constructor_voice,
                             voice_index=f.integer(extension+0x90)))
    array = f.allocate(20)
    u.mem_write(array, dwords(*(side for side, extension in sides), 0))
    f.write(0x8B4124, array)
    f.write(0x8B4130, 5)
    phase = 'side_load_all'
    f.ordered_ini({'SyntheticSide': {'EVA.Tag': 'VoiceB'}})
    original_stack = f.allocate(8)
    f.write(original_stack+4, INI)
    continuation, _ = hook(0x1002EFC0, esp=original_stack)
    controls.append(dict(kind=phase, sides=['GDI', 'Nod', 'ThirdSide',
        'SyntheticSide', None], has_sides_section=False,
        visits=list(side_read_visits), continuation=f'{continuation:08X}'))
    target = sides[3][1]
    saved_phase, saved_voice = f.integer(target+4), f.integer(target+0x90)
    for initial_phase in (0, 1, 2, -1, 3):
        phase = 'side_phase_dispatch'
        f.write(target+4, initial_phase)
        f.write(target+0x90, 4)
        visit_start = len(phase_read_visits)
        call(0x1002B450, [INI], ecx=target)
        controls.append(dict(kind=phase, initial_phase=initial_phase,
            initial_voice_index=4, final_phase=f.integer(target+4),
            final_voice_index=f.integer(target+0x90),
            reader_visits=phase_read_visits[visit_start:]))
    f.write(target+4, saved_phase)
    f.write(target+0x90, saved_voice)
    reader, key = f.allocate(4), f.allocate(8)
    f.write(reader, INI)
    u.mem_write(key, b'EVA.Tag\0')
    for value in (None, 'vOICEa', '', 'unknown', '<none>', 'rUSsian', 'VoiceB'):
        f.ordered_ini({'SyntheticSide': {} if value is None else {'EVA.Tag': value}})
        phase = 'side_read_and_bind'
        before = f.integer(target+0x90)
        cache_before = f.integer(VOICE_CACHE)
        write_start = len(cache_writes)
        call(0x1002EC00, [reader, sides[3][0]+0x24, key], ecx=target+0x90)
        cache_after_read = f.integer(VOICE_CACHE)
        resolved = f.integer(target+0x90)
        continuation, _ = hook(0x1002FB00, ecx=3)
        controls.append(dict(kind=phase, side_index=3, input=value, prior=before,
            resolved=resolved, cache_before_read=cache_before,
            cache_after_read=cache_after_read, bound=f.integer(VOICE_CACHE),
            filename=filename(event), continuation=f'{continuation:08X}',
            cache_writes=cache_writes[write_start:]))
    for side in (0, 1, 2, 4, -1):
        phase = 'set_side'
        before, write_start = f.integer(VOICE_CACHE), len(cache_writes)
        continuation, _ = hook(0x1002FB00, ecx=side)
        controls.append(dict(kind=phase, side_index=side, prior=before,
            bound=f.integer(VOICE_CACHE), filename=filename(event),
            continuation=f'{continuation:08X}', cache_writes=cache_writes[write_start:]))
    physical = {'EVA_TEST': {'VoiceB': 'SECOND12345'}}
    f.ordered_ini(physical)
    hook(0x100639C0, esi=event, ebp=INI)
    controls.append(dict(kind='extra_columns_reread', input=physical, samples=columns(event)))
    physical = {'EVATypes': {'0': 'VoiceB', '1': 'VoiceA', '2': 'voiceb'}}
    f.ordered_ini(physical)
    hook(0x10063660, ecx=INI)
    controls.append(dict(kind='catalog_reregister', input=physical, tags=tags(),
                         event_count=f.integer(0xB1D4B0)))
    # Lookup precedes none-token handling. Execute both unregistered clears
    # and a catalog containing these literal spellings, rather than assuming
    # that every none token necessarily clears the retained index.
    for registered in (False, True):
        if registered:
            physical = {'EVATypes': {'0': 'none', '1': '<none>'}}
            f.ordered_ini(physical)
            hook(0x10063660, ecx=INI)
        for value in ('none', '<none>'):
            phase = 'none_tag_precedence'
            f.ordered_ini({'SyntheticSide': {'EVA.Tag': value}})
            f.write(target+0x90, 2)
            call(0x1002EC00, [reader, sides[3][0]+0x24, key], ecx=target+0x90)
            controls.append(dict(kind=phase, registered=registered,
                tags=tags(), input=value, prior=2,
                resolved=f.integer(target+0x90)))
    f.check_code()
    assert bytes(u.mem_read(0x10001000, 0x92000)) == original_text
    assert not pending_reads
    return dict(controls=controls, native_reads=reads,
                rng=dict(seed=31, before=rng_before, after=f.rng_hashes(), calls=rng_calls))


def scalar_roundtrips():
    from tools.spatial_oracle import native_id_snapshot as stream_owner

    p = Probe()
    u = p.u
    original_text = bytes(u.mem_read(0x10001000, 0x92000))
    transport = stream_owner.ScenarioFixture.__new__(stream_owner.ScenarioFixture)
    transport.u, transport.data, transport.position = u, bytearray(), 0
    read_callback, write_callback = 0x30000400, 0x30000500
    u.mem_write(stream_owner.STREAM, struct.pack('<I', stream_owner.VTABLE))
    u.mem_write(stream_owner.VTABLE+12, struct.pack('<2I', read_callback, write_callback))
    transfers, cache_writes, rng_calls = [], [], []
    phase = 'setup'

    def observe(vm, pc, size, data):
        if pc in (read_callback, write_callback):
            sp = u.reg_read(UC_X86_REG_ESP)
            length = p.read(sp+12)
            transfers.append(dict(operation='read' if pc == read_callback else 'write', length=length))
            # Dispatch to the shared transport's existing logical callbacks.
            # These addresses avoid Probe's unrelated virtual type service.
            logical = stream_owner.READ if pc == read_callback else stream_owner.WRITE
            stream_owner.ScenarioFixture.hook(transport, vm, logical, size, data)
        if pc in RNG_ENTRIES:
            rng_calls.append(f'{pc:08X}')

    def observe_cache(vm, access, address, size, value, data):
        if address == VOICE_CACHE:
            cache_writes.append(dict(phase=phase,
                writer=f'{u.reg_read(UC_X86_REG_EIP):08X}', value=signed(value)))

    u.hook_add(UC_HOOK_CODE, observe)
    u.hook_add(UC_HOOK_MEM_WRITE, observe_cache)
    p.call(0x100576D0, ecx=0x100C1BE8)
    side = p.alloc(0x60)
    u.mem_write(side, bytes(0x60))
    u.mem_write(side+0x24, b'SyntheticSide\0')
    extension = p.alloc(0x280)
    p.call(0x1002D9F0, side, ecx=extension)
    p.call(0x10057CF0, side, extension, ecx=0x100C1BE8)
    require(p.call(0x10057C70, side, ecx=0x100C1BE8) == extension, 'native 0x10057C70 returned an unexpected result')
    array = p.alloc(16)
    u.mem_write(array, struct.pack('<4I', 0, 0, 0, side))
    p.write(0x8B4124, array)
    p.write(0x8B4130, 4)
    holder, frame = p.alloc(4), p.alloc(0x30)
    u.mem_write(frame, bytes(0x30))
    p.write(frame+0x20, 3)

    def scalar(begin, end):
        u.reg_write(UC_X86_REG_EBX, extension)
        u.reg_write(UC_X86_REG_EDI, holder)
        u.reg_write(UC_X86_REG_ESP, 0x2000F000)
        run_checked(u, begin, end, count=100_000, timeout_us=5_000_000)
        assert u.reg_read(UC_X86_REG_ESP) == 0x2000F000

    rows = []
    for value in (0, 1, 4, -1):
        start, write_start = len(transfers), len(cache_writes)
        rng_start = len(rng_calls)
        p.write(extension+0x90, value)
        p.write(VOICE_CACHE, 2)
        phase = 'save'
        output = p.alloc(16)
        p.call(0x1006B4E0, 4, ecx=output)
        p.write(holder, output)
        scalar(0x1002D516, 0x1002D526)
        payload = bytes(u.mem_read(p.read(output), p.read(output+4)-p.read(output)))
        after_save = signed(p.read(VOICE_CACHE))
        transport.data, transport.position = bytearray(), 0
        result = p.call(0x1006B7D0, stream_owner.STREAM, ecx=output)
        emitted = bytes(transport.data)
        p.write(extension+0x90, 99)
        phase = 'load'
        input_buffer = p.alloc(16)
        p.call(0x1006B5C0, stream_owner.STREAM, ecx=input_buffer)
        p.write(holder, input_buffer)
        scalar(0x1002D2EE, 0x1002D2FE)
        restored, after_load = signed(p.read(extension+0x90)), signed(p.read(VOICE_CACHE))
        phase = 'explicit_set_side'
        continuation = p.call(0x1002FB00, frame)
        rows.append(dict(input=value, overwrite_before_load=99,
            payload_hex=payload.hex(), framed_stream_hex=emitted.hex(),
            writer_al=result & 255, stream_read_bytes=transport.position,
            buffer_consumed=p.read(input_buffer+12), restored=restored,
            cache=dict(before=2, after_save=after_save, after_load=after_load,
                       after_explicit_set_side=signed(p.read(VOICE_CACHE))),
            continuation=f'{continuation:08X}', transfers=transfers[start:],
            cache_writes=cache_writes[write_start:], rng_calls=rng_calls[rng_start:]))
    assert bytes(u.mem_read(0x10001000, 0x92000)) == original_text
    return rows


def generate(ares):
    Probe.input_bytes = ares, native.image_bytes()
    return dict(schema_version=1, read_bind=read_bind_controls(ares),
                scalar_roundtrips=scalar_roundtrips())


def metadata():
    return provenance(
        scope='Ares EVA catalog/extra columns, retained Side tag/default/SetSide filename selection, and four bounded signed SideExt EVA scalar save/load roundtrips.',
        assumptions=[
            'Fixed EVAMD input is supplied as synthetic source-order cached INI through ActionFixture.ordered_ini. Original ReadString executes. EVATypes capacity200, DialogList40 and extra sample16 are observed before actual8-byte filename storage. Physical file ingestion/full startup are excluded.',
            'Original stock event reader runs before the explicitly invoked Ares extra-column hook. Hook frames declare the original register inputs; complete installed hook dispatch is excluded. Actual SideExt constructor/default initializer, container insert/Find, scalar tag reader and SetSide execute on valid supplied Side shells. One Side_LoadAll hook control executes its actual outer traversal/Find with the full extension dispatcher as a recorded sink, independently of the absent Sides section. Five separate controls execute actual phase/default decisions with the unrelated full Side reader as a recorded sink.',
            'SideExt+90 retains resolved signed voice index; actual Side.ID supplies defaults. Field rereads do not rebind processB1D4C8. Negative SetSide clears to-1, null in-bounds Side retains, and invalid extra filename index yields empty. Unallocated positive Side index is excluded.',
            'Four scalar controls execute1002D516..1002D526/1002D2EE..1002D2FE with actual ByteBuffer helpers and emitted IStream bytes. Outer magic, pointer/swizzle, phase, neighboring fields, whole Side/OLE save/load, failed transport and savefile parity are excluded.',
            'Read/bind RNG streams are seeded31 by native65C6D0 and their original-layout state hashes and draw entries are observed. Scalar controls observe draw entries only. Playback timers, queue/decoder/audio device, whole-match startup and audible output are excluded. No selected-chain detach call is introduced.'],
        substitutions=[
            'Existing ActionFixture owns VM/heap/CRT/TLS/INI transport, preallocated registry and stream shell. Its explicit stack_base avoids Ares preferred base; module STACK/SP globals are unchanged. Shared map_pe32_image loads DLL bytes.',
            'Existing Probe services supply Ares new/delete backing and diagnostics in the ActionFixture VM, with allocation delegated to ActionFixture.allocate. Native malloc7C9430/Ares allocation10001B50 use that same backing; actual strdup/strcpy, stricmp, ReadString, vectors and secure copy execute.',
            'Scalar experiment reuses Probe as its VM owner and ScenarioFixture.hook as IStream byte transport attached via__new__ to that VM. Callback dispatch avoids Probe virtual-category addresses without replacing its transport or changing module globals. Probe constructor string-copy/allocator services remain inherited boundaries.'],
        entry_points=dict(registration=0x10063660, column_read=0x100639C0,
            filename=0x10063920, side_constructor=0x1002D9F0,
            side_default=0x1002E190, side_load_all=0x1002EFC0,
            side_phase_dispatch=0x1002B450,
            side_tag_read=0x1002EC00,
            side_ext_find=0x10057C70, set_side=0x1002FB00,
            scalar_save=0x1002D516, scalar_load=0x1002D2EE,
            buffer_write=0x1006B7B0, buffer_read=0x1006B6F0,
            stream_write=0x1006B7D0, stream_read=0x1006B5C0))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ares-dll', type=Path,
                        default=os.environ.get('VERA20K_ARES_DLL'))
    parser.add_argument('--gamemd-exe', type=Path)
    args, remaining = parser.parse_known_args()
    if args.ares_dll is None:
        parser.error('pass --ares-dll or set VERA20K_ARES_DLL')
    if args.gamemd_exe is not None:
        os.environ['VERA20K_GAMEMD_EXE'] = str(args.gamemd_exe.resolve())
    ares = identified_bytes(args.ares_dll, ARES_SHA256)
    finish_vectors(lambda: generate(ares), Path(__file__).with_suffix('.json'),
        provenance=metadata, argv=remaining, source_paths={
            'producer': Path(__file__), 'runner': Path(native.__file__),
            'ares_services': Path(__file__).parents[1]/'rules_oracle/ares_prerequisites.py',
            'action_fixture': Path(__file__).with_name('trigger_action_values.py'),
            'stream_transport': Path(__file__).with_name('native_id_snapshot.py')})


if __name__ == '__main__':
    main()
