"""Installed Ares multi-BAG parser/merge controls using synthetic bytes only.

Set VERA20K_ARES_DLL and VERA20K_GAMEMD_EXE, then run with --check.
CCFile, heap and bounded snprintf are explicit seams; no audio device runs.
"""
import argparse
import os
import struct
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_ECX, UC_X86_REG_EDX
from tools import native_oracle as native
from tools.native_oracle import finish_vectors, provenance
from tools.rules_oracle.ares_country_veterancy import CountryFixture
from tools.rules_oracle.ares_prerequisites import identified_bytes, ARES_SHA256

def merge_controls(images):
    results = []
    for spec in [[('audio', [('VOICE', 101), ('BASE', 102)]), ('ares', [('voice', 201), ('EXT', 202)]), ('audio01', [('Voice', 301)])], [('audio', []), ('ares', [('UNIQUE', 401)]), ('audio99', [('FINAL', 501), ('unique', 502)])]]:
        p = CountryFixture(images)
        u = p.u
        allocs = []

        def seam(u, pc, size, data):
            sp = u.reg_read(UC_X86_REG_ESP)
            if pc == 0x10001b60:
                n = p.read(sp + 4)
                allocs.append(n)
                p.service_return(4, p.alloc(n))
        u.hook_add(UC_HOOK_CODE, seam)
        luggage = p.alloc(64)
        bags = p.alloc(16 * len(spec))
        file_ids = {}
        inputs = []
        for i, (stem, entries) in enumerate(spec):
            start = p.alloc(36 * len(entries))
            file_id = 0x20100000 + 16 * i
            file_ids[file_id] = stem
            for j, (name, off) in enumerate(entries):
                u.mem_write(start + 36 * j, struct.pack('<16s5I', name.encode(), off, 8, 22050, 4, 0))
            p.put(bags + 16 * i, file_id, start, start + 36 * len(entries), start + 36 * len(entries))
            inputs.append(dict(stem=stem, file_token=file_id, entries=entries))
        p.put(luggage, bags, bags + 16 * len(spec), bags + 16 * len(spec))
        out = p.call(0x1005a170, (0,), ecx=luggage)
        count = p.read(out + 4)
        entries = p.read(out)
        files = p.read(luggage + 12)
        rows = []
        for i in range(count):
            row = struct.unpack('<16s5I', u.mem_read(entries + 36 * i, 36))
            filetoken = p.read(files + 4 * i)
            rows.append(dict(name=row[0].split(b'\x00')[0].decode(), offset=row[1], size=row[2], sample_rate=row[3], flags=row[4], chunk_size=row[5], file_stem=file_ids[filetoken]))
        lookup = []
        for query in ['voice', 'VOICE', 'unique', 'UNIQUE', 'missing', 'FINAL']:
            u.reg_write(UC_X86_REG_EDX, p.text(query))
            index = p.call(0x4015c0, ecx=out)
            lookup.append(dict(query=query, index=index if index < 0x80000000 else index - 0x100000000))
        results.append(dict(inputs=inputs, rows=rows, lookup=lookup, allocations=allocs, code_unchanged=True))
    return results

def open_controls(images):
    results = []

    def idx(entries):
        return struct.pack('<3I', 0x41424147, 2, len(entries)) + b''.join((struct.pack('<16s5I', n.encode(), off, 8, 22050, 4, 0) for n, off in entries))
    for name, files in [('missing_index', {'probe.bag': b'bag'}), ('missing_bag', {'probe.idx': idx([('A', 1)])}), ('empty_pair', {'probe.idx': idx([]), 'probe.bag': b'bag'}), ('sorted_casefold', {'probe.idx': idx([('z', 1), ('A', 2), ('m', 3)]), 'probe.bag': b'bag'}), ('short_header', {'probe.idx': b'x', 'probe.bag': b'bag'}), ('short_entries', {'probe.idx': idx([('A', 1)])[:-2], 'probe.bag': b'bag'}), ('gaba_version1_32byte', {'probe.idx': struct.pack('<3I', 0x41424147, 1, 1) + struct.pack('<16s4I', b'V1', 1, 8, 22050, 4), 'probe.bag': b'bag'}), ('first_dword1_32byte', {'probe.idx': struct.pack('<3I', 1, 1, 1) + struct.pack('<16s4I', b'V1', 1, 8, 22050, 4), 'probe.bag': b'bag'}), ('gaba_version77_36byte', {'probe.idx': struct.pack('<3I', 0x41424147, 77, 1) + struct.pack('<16s5I', b'V77', 1, 8, 22050, 4, 0), 'probe.bag': b'bag'})]:
        p = CountryFixture(images)
        u = p.u
        vt = p.alloc(128)
        objects = {}
        events = []
        for slot, at in [(0, 0x30000300), (20, 0x30000310), (28, 0x30000320), (36, 0x30000330)]:
            p.put(vt + slot, at)

        def seam(u, pc, size, data):
            sp = u.reg_read(UC_X86_REG_ESP)
            ecx = u.reg_read(UC_X86_REG_ECX)
            arg = lambda i: p.read(sp + 4 + 4 * i)
            if pc == 0x10003a40:
                fmt = p.string(arg(3))
                assert fmt in ('%s.idx', '%s.bag')
                raw = (fmt % p.string(arg(4))).encode()
                assert len(raw) < arg(1)
                u.mem_write(arg(0), raw + b'\x00')
                p.service_return(0, len(raw))
            elif pc == 0x10002ba0:
                f = p.string(arg(0))
                objects[ecx] = [f, 0]
                p.put(ecx, vt)
                events.append(dict(call='CCFile_ctor', filename=f))
                p.service_return(4, ecx)
            elif pc == 0x10001b60:
                p.service_return(4, p.alloc(arg(0)))
            elif pc == 0x100033c0:
                p.service_return(0, 0)
            elif pc == 0x30000300:
                events.append(dict(call='CCFile_dtor', filename=objects[ecx][0]))
                p.service_return(4, 0)
            elif pc in [0x30000310, 0x30000320]:
                f = objects[ecx][0]
                events.append(dict(call='Exists' if pc == 0x30000310 else 'Open', filename=f, value=f in files))
                p.service_return(4, int(f in files))
            elif pc == 0x30000330:
                f, pos = objects[ecx]
                target, n = (arg(0), arg(1))
                b = files.get(f, b'')[pos:pos + n]
                u.mem_write(target, b)
                objects[ecx][1] += len(b)
                events.append(dict(call='Read', filename=f, requested=n, returned=len(b)))
                p.service_return(8, len(b))
        u.hook_add(UC_HOOK_CODE, seam)
        bag = p.alloc(32)
        p.call(0x1005aa00, (p.text('probe'),), ecx=bag)
        fp, begin, end, cap = [p.read(bag + i * 4) for i in range(4)]
        rows = []
        for i in range((end - begin) // 36):
            raw = struct.unpack('<16s5I', u.mem_read(begin + 36 * i, 36))
            rows.append(dict(name=raw[0].split(b'\x00')[0].decode(), offset=raw[1], size=raw[2], sample_rate=raw[3], flags=raw[4], chunk_size=raw[5]))
        results.append(dict(name=name, inputs={k: v.hex() for k, v in files.items()}, output=dict(file_retained=bool(fp), entries=rows), events=events, code_unchanged=True))
    return results

def order_control(images):
    p = CountryFixture(images)
    u = p.u
    calls = []
    formats = []
    bags = p.alloc(16 * 101)
    p.put(0x100c20b0, bags, bags, bags + 16 * 101)

    def seam(u, pc, size, data):
        sp = u.reg_read(UC_X86_REG_ESP)
        arg = lambda i: p.read(sp + 4 + 4 * i)
        if pc == 0x10003a40:
            fmt = p.string(arg(3))
            assert fmt == 'audio%02d'
            value = arg(4)
            raw = (fmt % value).encode()
            assert len(raw) < arg(1)
            u.mem_write(arg(0), raw + b'\x00')
            formats.append(dict(format=fmt, value=value))
            p.service_return(0, len(raw))
        elif pc == 0x1005aa00:
            calls.append(p.string(arg(0)))
            p.service_return(4, 0)
        elif pc == 0x1005a170:
            p.service_return(4, 0x20110000)
    u.hook_add(UC_HOOK_CODE, seam)
    frame = p.alloc(64)
    resume = p.call(0x1005add0, (frame,))
    result = dict(scope='Actual installed Audio_Load body executes vector append order and integer loop. AudioBag::Open/Create are observation seams, snprintf audio%02d is supplied; not formatter or VFS parity.', stems=calls, formats=formats, resume=f'{resume:08X}', saved_eax=p.read(frame + 36), code_unchanged=True)
    return result

def generate(ares):
    images = (ares, native.image_bytes())
    p = CountryFixture(images)
    wanted = {'Audio_Load', 'VocClassData_AddSample', 'IDXContainer_LoadSample', 'AudioIndex_GetSampleInformation'}
    hooks = [dict(site=f'{p.read(at):08X}', length=p.read(at + 4), name=p.string(p.read(at + 8))) for at in range(0x100c3000, 0x100c8a80, 16) if p.string(p.read(at + 8)) in wanted]
    return dict(schema_version=1, ares_sha256=ARES_SHA256, installed_hooks=hooks, filename_literals={f'{at:08X}': p.string(at) for at in (0x100a2c7c, 0x100a2c84, 0x100a2c8c, 0x100a2c94, 0x100a2c9c)}, order=order_control(images), open=open_controls(images), merge=merge_controls(images))

def metadata():
    return provenance(scope='Installed Ares Audio_Load loop, paired BAG admission/IDX layout, casefold sort/merge and original FindSampleIndex, bounded synthetic component execution.', assumptions=['Synthetic CCFile transport objects and pre-opened bags are inputs; native MIX resolver, OS handles, whole startup/hook installer, driver and audible output are not executed.', 'Open controls supply byte-exact synthetic files, Exists/Open outcomes and read transfer. The actual DLL tests read lengths, owns paired admission and sorts metadata. Malformed sizes large enough to exhaust memory are outside scope.', 'Merge controls supply existing entry arrays and opaque file tokens; actual native casefold tree merge overwrites complete metadata and backing file, then original FindSampleIndex executes.', 'No whole mechanism or audio RNG/mixer parity is claimed; no mod payloads are used. All selected text is checked unchanged by CountryFixture. Existing fixture rejects RNG and detach entries.'], substitutions=['Reuses public CountryFixture/Probe image mapping, arena, call execution and heap leaves; no second VM owner.', 'Additional game allocator10001B60 and delete100033C0 are heap boundaries. CCFile ctor/Exists/Open/Read/destructor are declared in-memory transport leaves.', 'Bounded snprintf for literal %s.idx, %s.bag and audio%02d is supplied. Thus order control proves loop bounds/order, not formatter equivalence.'], entry_points=dict(audio_load=0x1005add0, bag_open=0x1005aa00, merge=0x1005a170, find=0x4015c0))

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ares-dll', type=Path, default=os.environ.get('VERA20K_ARES_DLL'))
    parser.add_argument('--gamemd-exe', type=Path)
    args, remaining = parser.parse_known_args()
    if args.ares_dll is None:
        parser.error('Set VERA20K_ARES_DLL or supply --ares-dll')
    if args.gamemd_exe:
        os.environ['VERA20K_GAMEMD_EXE'] = str(args.gamemd_exe)
    ares = identified_bytes(args.ares_dll, ARES_SHA256)
    finish_vectors(lambda: generate(ares), Path(__file__).with_suffix('.json'), provenance=metadata, argv=remaining, source_paths={'producer': Path(__file__), 'runner': Path(native.__file__), 'vm_owner': Path(__file__).with_name('ares_prerequisites.py'), 'fixture_owner': Path(__file__).with_name('ares_country_veterancy.py'), 'return_owner': Path('tools/input_oracle/fast_scroll.py')})
if __name__ == '__main__':
    main()
