"""Read-only original ScoldSound reader and Foot latch constructor audit.

Run with PYTHONPATH=. and the shared native environment. Default --check is
read-only; explicit --write records this corpus. See foot_scold_latch.md.
"""
from pathlib import Path
import os
import hashlib
import json
import struct
from capstone import Cs, CS_ARCH_X86, CS_MODE_32, CS_AC_WRITE
from capstone.x86_const import X86_OP_MEM
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
from tools.native_oracle import (image_bytes, _sections,
                                run_checked, RET_MAGIC, finish_vectors, provenance)
from tools.rules_oracle.bridge_child_sound import Sound, sections
from tools.rules_oracle.bridge_anim_inputs import Reader
from tools.spatial_oracle.building_body_rules import Fixture, INI, SP, SCRATCH, dwords

HERE = Path(__file__).resolve().parent
RETAIL = Path(os.environ.get('VERA20K_PROJECTILE_RENDER_ASSETS',
    str(Path(os.environ.get('CARGO_TARGET_DIR', 'target')) / 'asset' / 'projectile-render-assets' / 'extract')))
SOUND = Path(os.environ.get('VERA20K_SCOLD_SOUND_INI', str(HERE.parents[1] / 'ini' / 'soundmd.ini')))


class ScoldReader(Sound):
    def __init__(self):
        self.samples = []
        self.calls = []
        raw = SOUND.read_bytes()
        physical = sections(raw)
        selected = {key: physical[key] for key in ('Defaults', 'MenuScold')}
        selected['SoundList'] = {k: v for k, v in physical['SoundList'].items() if v == 'MenuScold'}
        assert len(selected['SoundList']) == 1
        Reader.__init__(self, SOUND.parent, selected)
        self.selected = selected
        self.raw = raw
        self.u.mem_write(0x87E2A0, dwords(1))
        self.u.mem_write(0x87E294, dwords(self.alloc(0x100)))
        self.invoke(0x4072C0, 0x87E250)
        self.u.mem_write(0xB1D378, dwords(0x7EB6D4, self.alloc(64), 16, 1, 0, 10))
        self.invoke(0x7510D0, INI)

    def hook(self, u, pc, size, data):
        if pc == 0x4015C0:
            name = self.string(u.reg_read(UC_X86_REG_EDX))
            if name not in self.samples:
                self.samples.append(name)
            self.ret(self.samples.index(name))
            return
        Reader.hook(self, u, pc, size, data)

    def scold_block(self, ini, source):
        self.make_ini(ini)
        rules = SCRATCH + 0x8000
        u = self.u
        u.reg_write(UC_X86_REG_ESI, rules)
        u.reg_write(UC_X86_REG_EDI, INI)
        u.reg_write(UC_X86_REG_ESP, SP)
        before = struct.unpack('<i', u.mem_read(rules + 0x700, 4))[0]
        run_checked(u, 0x66ABCD, 0x66AC18, count=200000,
                    required_addresses=(0x528A10, 0x66AC12))
        after = struct.unpack('<i', u.mem_read(rules + 0x700, 4))[0]
        return dict(source=source, physical_value=ini.get('AudioVisual', {}).get('ScoldSound'),
                    native_buffer=self.string(SP + 0x14), before=before, after=after)


def constructor():
    f = Fixture()
    u = f.u
    foot = SCRATCH + 0x8000
    u.mem_write(foot, bytes([0xA5]) * 0x800)
    u.reg_write(UC_X86_REG_ESI, foot)
    u.reg_write(UC_X86_REG_EBX, 0xA5A5A5A5)
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, 0x4D31EF, 0x4D33BA, count=10000,
                required_addresses=(0x4D31EF, 0x4D33B4))
    return dict(entry='0x004D31EF', endpoint='0x004D33BA',
                scope='Original Foot constructor after its Techno parent call, through latch initialization',
                initial_byte=0xA5, final_byte=u.mem_read(foot + 0x68A, 1)[0],
                final_ebx=u.reg_read(UC_X86_REG_EBX))


def imported_latch(value):
    """Execute raw Abstract load and original no-init Foot reconstruction.

    This is a declared imported-state boundary, not a normal-game arming writer
    or an execution of the intervening dynamic Foot/Infantry load suffixes.
    """
    f = ScoldReader()
    u = f.u
    foot, stream, table = f.alloc(0x800), f.alloc(0x20), f.alloc(0x40)
    read_entry = SCRATCH + 0xF000
    u.mem_write(foot, dwords(0x7EB058))
    u.mem_write(foot + 0x1C, dwords(0x11223344))
    u.mem_write(stream, dwords(table))
    u.mem_write(table + 0xC, dwords(read_entry))
    raw = bytearray(0x6F0)
    struct.pack_into('<I', raw, 0, 0x7EB058)
    struct.pack_into('<I', raw, 0x1C, 0x66778899)
    raw[0x68A] = value
    payload = dwords(0x12345678) + raw
    consumed, calls = 0, []

    def read_stream(u, pc, size, data):
        nonlocal consumed
        if pc != read_entry:
            return
        receiver, destination, length, actual = struct.unpack(
            '<4I', u.mem_read(u.reg_read(UC_X86_REG_ESP) + 4, 16))
        assert receiver == stream
        block = bytes(payload[consumed:consumed + length])
        assert len(block) == length
        u.mem_write(destination, block)
        if actual:
            u.mem_write(actual, dwords(length))
        consumed += length
        calls.append(dict(length=length,
                          destination='foot' if destination == foot else 'saved-this token'))
        f.ret(0, 16)

    hook = u.hook_add(UC_HOOK_CODE, read_stream)
    u.mem_write(SP, dwords(RET_MAGIC, foot, stream))
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, 0x410380, RET_MAGIC, count=200000,
                required_addresses=(0x6CF2C0, 0x5232F0, 0x4103CD))
    u.hook_del(hook)
    loaded = u.mem_read(foot + 0x68A, 1)[0]
    # Infantry Load 521A0C calls this exact constructor (not ordinary 4D31E0).
    f.invoke(0x4D3540, foot, (0,))
    return dict(supplied_saved_byte=value, initial_live_byte=0,
                stream_calls=calls, consumed_bytes=consumed,
                after_abstract_load=loaded,
                after_original_noinit_constructor=u.mem_read(foot + 0x68A, 1)[0],
                preserved_live_1c=hex(f.read32(foot + 0x1C)))


def scold_guard(value):
    f = Fixture()
    u = f.u
    foot, loco, rules = SCRATCH + 0x8000, SCRATCH + 0x9000, SCRATCH + 0xA000
    u.mem_write(foot + 0x68A, bytes([value]))
    u.mem_write(loco + 0xC, dwords(foot))
    u.mem_write(rules + 0x700, dwords(0))  # Native reader's single-entry registry index.
    u.reg_write(UC_X86_REG_EAX, foot)
    u.reg_write(UC_X86_REG_EBP, loco)
    u.reg_write(UC_X86_REG_EDI, rules)
    u.reg_write(UC_X86_REG_ESP, SP)
    calls = []

    def sound_entry(u, pc, size, data):
        if pc != 0x750920:
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        volume, trailing = struct.unpack('<2I', u.mem_read(sp + 4, 8))
        calls.append(dict(sound_index=u.reg_read(UC_X86_REG_ECX),
                          pan=u.reg_read(UC_X86_REG_EDX),
                          volume_bits=hex(volume), trailing=trailing))
        destination = struct.unpack('<I', u.mem_read(sp, 4))[0]
        u.reg_write(UC_X86_REG_ESP, sp + 12)
        u.reg_write(UC_X86_REG_EAX, 0)
        u.reg_write(UC_X86_REG_EIP, destination)

    u.hook_add(UC_HOOK_CODE, sound_entry)
    run_checked(u, 0x75B085, 0x75B0B0, count=1000,
                required_addresses=(0x75B085, 0x75B0A9))
    return dict(supplied_byte=value, final_byte=u.mem_read(foot + 0x68A, 1)[0],
                sound_entry_calls=calls)


def paid_tail(name, value):
    f = Fixture()
    u = f.u
    foot, loco, vtable = SCRATCH + 0x8000, SCRATCH + 0x9000, SCRATCH + 0xA000
    entries = dict(no_head_speed_zero=0x75BCE3, no_head_speed_positive=0x75BCE3,
                   arrival_mark=0x75BF64, predicate_true=0x75BF85,
                   predicate_false=0x75BF85, same_cell_commit=0x75C1FB,
                   common_return=0x75C1E7, dead_post_percell=0x75BE42,
                   limbo_post_percell=0x75BE42, falling_post_percell=0x75BE42,
                   dead_fresh_head=0x75BC2A)
    endpoints = (0x75BD16, 0x75C1F1, 0x75BF7E, 0x75BFA2, 0x75BFA9, 0x75C236)
    callbacks = {SCRATCH + 0xF000: ('mark', 4),
                 SCRATCH + 0xF010: ('predicate_37c', 0),
                 SCRATCH + 0xF020: ('set_coords', 4),
                 SCRATCH + 0xF030: ('set_height', 4)}
    u.mem_write(vtable, bytes(u.mem_read(0x7EB058, 0x600)))
    for slot, address in ((0x124, SCRATCH + 0xF000), (0x37C, SCRATCH + 0xF010),
                          (0x1B4, SCRATCH + 0xF020), (0x1CC, SCRATCH + 0xF030)):
        u.mem_write(vtable + slot, dwords(address))
    u.mem_write(foot, dwords(vtable))
    u.mem_write(foot + 0x68A, bytes([value]))
    u.mem_write(foot + 0x74, b'\x00')
    u.mem_write(foot + 0x90, bytes([name not in ('dead_post_percell', 'dead_fresh_head')]))
    u.mem_write(foot + 0x81, bytes([name == 'limbo_post_percell']))
    u.mem_write(foot + 0x8D, bytes([name == 'falling_post_percell']))
    speed = 0.0 if name == 'no_head_speed_zero' else 0.75
    u.mem_write(foot + 0x578, struct.pack('<d', speed))
    u.mem_write(loco + 0xC, dwords(foot))
    u.mem_write(loco + 0x36, bytes([name != 'dead_fresh_head']))
    u.mem_write(SP + 0x30, dwords(2496, 2624, 104))
    u.reg_write(UC_X86_REG_EBP, loco)
    u.reg_write(UC_X86_REG_ECX, foot)
    u.reg_write(UC_X86_REG_ESP, SP)
    events = []

    def callback(u, pc, size, data):
        if pc not in callbacks:
            return
        kind, cleanup = callbacks[pc]
        sp = u.reg_read(UC_X86_REG_ESP)
        args = list(struct.unpack('<' + 'I' * (cleanup // 4),
                                  u.mem_read(sp + 4, cleanup))) if cleanup else []
        if kind == 'set_coords':
            args = [list(struct.unpack('<3i', u.mem_read(args[0], 12)))]
        events.append(dict(callback=kind, latch=u.mem_read(foot + 0x68A, 1)[0],
                           object_74=u.mem_read(foot + 0x74, 1)[0], args=args))
        destination = struct.unpack('<I', u.mem_read(sp, 4))[0]
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)
        u.reg_write(UC_X86_REG_EAX, int(kind == 'predicate_37c' and name == 'predicate_true'))
        u.reg_write(UC_X86_REG_EIP, destination)

    u.hook_add(UC_HOOK_CODE, callback)
    run_checked(u, entries[name], endpoints, count=1000,
                required_addresses=(entries[name],))
    return dict(case=name, supplied_byte=value,
                final_byte=u.mem_read(foot + 0x68A, 1)[0],
                motion=u.mem_read(loco + 0x36, 1)[0],
                object_74=u.mem_read(foot + 0x74, 1)[0],
                speed_fraction=struct.unpack('<d', u.mem_read(foot + 0x578, 8))[0],
                events=events, endpoint=hex(u.reg_read(UC_X86_REG_EIP)))


def track_guard(family, branch, value):
    f = Fixture()
    u = f.u
    foot, loco, rules = SCRATCH + 0x8000, SCRATCH + 0x9000, SCRATCH + 0xA000
    ranges = {('drive', 'exhausted'): (0x4B2E47, 0x4B2E77),
              ('ship', 'exhausted'): (0x6A2497, 0x6A24C7),
              ('drive', 'first_rejection'): (0x4B3AA1, 0x4B3BEF),
              ('ship', 'first_rejection'): (0x6A30F0, 0x6A323E)}
    entry, endpoint = ranges[family, branch]
    u.mem_write(foot + 0x68A, bytes([value]))
    u.mem_write(loco + 0xC, dwords(foot))
    u.mem_write(rules + 0x700, dwords(0))
    u.mem_write(0x8871E0, dwords(rules))
    u.reg_write(UC_X86_REG_EAX, foot)
    u.reg_write(UC_X86_REG_EBP, loco)
    u.reg_write(UC_X86_REG_ESP, SP)
    calls = []

    def sound_entry(u, pc, size, data):
        if pc != 0x750920:
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        volume, trailing = struct.unpack('<2I', u.mem_read(sp + 4, 8))
        calls.append(dict(sound_index=u.reg_read(UC_X86_REG_ECX),
                          pan=u.reg_read(UC_X86_REG_EDX),
                          volume_bits=hex(volume), trailing=trailing))
        destination = struct.unpack('<I', u.mem_read(sp, 4))[0]
        u.reg_write(UC_X86_REG_ESP, sp + 12)
        u.reg_write(UC_X86_REG_EAX, 0)
        u.reg_write(UC_X86_REG_EIP, destination)

    u.hook_add(UC_HOOK_CODE, sound_entry)
    run_checked(u, entry, endpoint, count=1000, required_addresses=(entry,))
    return dict(family=family, branch=branch, supplied_byte=value,
                final_byte=u.mem_read(foot + 0x68A, 1)[0],
                sound_entry_calls=calls, endpoint=hex(endpoint))


def direct_scan():
    image = image_bytes()
    scanner = Cs(CS_ARCH_X86, CS_MODE_32)
    scanner.skipdata = True
    detail = Cs(CS_ARCH_X86, CS_MODE_32)
    detail.detail = True
    matches, overlapping, address_formations = [], [], []
    count = 0
    for rva, raw, size, _, flags in _sections(image):
        if not flags & 0x20000000:
            continue
        code = image[raw:raw + size]
        for address, length, mnemonic, operands in scanner.disasm_lite(code, 0x400000 + rva):
            count += 1
            if '0x6' not in operands:
                continue
            at = address - 0x400000 - rva
            ins = next(detail.disasm(code[at:at + length], address), None)
            if ins is None:
                continue
            for op in ins.operands:
                if op.type != X86_OP_MEM:
                    continue
                record = dict(address=hex(address), mnemonic=mnemonic, operands=operands,
                              bytes=bytes(ins.bytes).hex(), displacement=op.mem.disp,
                              width=op.size, write=bool(op.access & CS_AC_WRITE))
                if op.mem.disp == 0x68A:
                    matches.append(record)
                if op.access & CS_AC_WRITE and op.mem.disp < 0x68A < op.mem.disp + op.size:
                    overlapping.append(record)
                if mnemonic == 'lea' and op.mem.disp in range(0x680, 0x68B):
                    address_formations.append(record)
    return dict(decoded_items=count, exact_displacement=matches,
                overlapping_direct_writes=overlapping, nearby_address_formations=address_formations,
                limit='Literal field-displacement/overlap scan of executable PE sections; not general alias/dataflow proof. Non-Foot owners are listed separately, not presumed to mutate this Foot field.')


def generate():
    for name in ('RULESMD.INI', 'MPBattleMD.ini', 'Hills.map'):
        assert (RETAIL / name).is_file(), ('Set VERA20K_PROJECTILE_RENDER_ASSETS to the extracted retail directory', name)
    m = ScoldReader()
    rules = SCRATCH + 0x8000
    # Rules constructor666027 stores EBP=-1 (665650 prefix) at+700.
    m.u.mem_write(rules + 0x700, dwords(-1))
    layers = []
    reads = []
    for name in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'Hills.map'):
        path = RETAIL / name
        if not path.exists():
            layers.append(dict(file=name, absent=True))
            continue
        raw = path.read_bytes()
        physical = sections(raw)
        selected = {'AudioVisual': physical.get('AudioVisual', {})}
        layers.append(dict(file=name, sha256=hashlib.sha256(raw).hexdigest()))
        reads.append(m.scold_block(selected, name))
    resolved = struct.unpack('<i', m.u.mem_read(rules + 0x700, 4))[0]
    assert resolved >= 0
    voc = m.read32(m.read32(0xB1D37C) + resolved * 4)
    sound_name = m.string(m.read32(voc) + 0x6C)
    missing = m.scold_block({'AudioVisual': {}}, 'missing-key control')
    empty = m.scold_block({'AudioVisual': {'ScoldSound': ''}}, 'empty-value control')
    invalid = m.scold_block({'AudioVisual': {'ScoldSound': 'NotARegisteredSound'}}, 'unknown-name control')
    return dict(constructor=constructor(),
                imported_latch=[imported_latch(v) for v in (0, 1, 255)],
                scold_guard=[scold_guard(v) for v in (0, 1, 255)],
                paid_tails=[paid_tail(n, v) for n in (
                    'no_head_speed_zero', 'no_head_speed_positive', 'arrival_mark',
                    'predicate_true', 'predicate_false', 'same_cell_commit',
                    'common_return', 'dead_post_percell', 'limbo_post_percell',
                    'falling_post_percell', 'dead_fresh_head') for v in (0, 1, 255)],
                track_guards=[track_guard(f, b, v) for f in ('drive', 'ship')
                              for b in ('exhausted', 'first_rejection') for v in (0, 1, 255)],
                sound=dict(file=SOUND.name, sha256=hashlib.sha256(m.raw).hexdigest(),
                           physical=m.selected, resolved_index_fixture_relative=resolved,
                           resolved_name=sound_name, sample_lookup_inputs=m.samples),
                layers=layers, rules_reads=reads, retention_controls=[missing, empty, invalid],
                direct_scan=direct_scan())


if __name__ == '__main__':
    finish_vectors(generate, HERE / 'foot_scold_latch.json', provenance=lambda: provenance(
        scope=__doc__, assumptions=[
            'Foot constructor prefix after original parent call executes with valid supplied object storage initiallyA5; original XOR EBX at4D31EF and all reached scalar/vector initialization through4D33B4 execute.',
            'Physical SOUNDMD Defaults/MenuScold and actual SoundList key populate native7510D0 registry using source-order linked/CRC INI caches. Registry contains only MenuScold, so returned index0 is fixture-relative; retained identity is the evidence.',
            'Original Rules AudioVisual block66ABCD..66AC18 executes its ReadString528A10, FindByName7514D0 and+700 store against each applicable physical Hills layer. Initial-1 comes from Rules constructor666027 EBP default. Missing/empty/unknown name controls preserve the prior binding.',
            'Capstone literal memory-field scan includes wider writes covering68A and nearby LEA formations. Its limits are explicit: other class offsets and generic raw stream deserialization require owner/caller interpretation.'],
        substitutions=[
            'INI caches are lexically prepared from physical text; no full physical INI/archive loading. Existing allocator/free/TLS seams are inherited.',
            'AudioIndex::FindSample4015C0 records native reader sample names and supplies local sample indices; no sample data, device output or audible parity claim.',
            'The raw-load controls supply a serialized token and 0x6F0-byte Infantry body through IStream::Read; original Abstract load, Here_I_Am and Infantry size leaf execute. Original no-init Foot constructor then executes. Intervening dynamic Foot/Infantry Load suffixes are instruction-audited, not emulated in these controls.',
            'The optional sound guard controls record entry to 750920 and return without audio playback. Guard, arguments and latch-clear execute unchanged. A nonzero imported/supplied byte is not proof of a normal-game arming producer.',
            'Paid-tail controls supply reached interior branch state, positive/zero applied speed and lifecycle flags; original Foot speed setter executes. Fresh fixture virtual slots Mark+124, predicate+37C, SetCoords+1B4 and SetHeight+1CC observe request arguments/latch then return. These controls establish tail ordering/clears, not the callbacks effects or preceding complete Walk movement.',
            'Dead fresh-head controls enter75BC2A with Object+90 false and motion initially zero. Original motion write and75BC36 guard reach75C1F1 without clearing the supplied latch; no facing/speed callback executes and ordinary dead-owner reachability is not claimed.',
            'Drive/Ship controls enter after the exhausted-path alive gate or at the first-rejection sound fragment. They stop immediately after exhausted latch clear or before first-rejection retry continuation; the latter retains the byte and may recurse before a later clear. Full retry routing is not emulated here.',
            'No latch writer, native ScoldSound reader return or original executable instruction is replaced.'],
        entry_points={'foot_ctor_after_parent':0x4D31EF,'latch_init':0x4D33B4,
                      'rules_ctor_default_store':0x666027,'scold_read':0x66ABCD,
                      'read_string':0x528A10,'sound_registry':0x7510D0,
                      'sound_find':0x7514D0,'active_walk_wrapper':0x75AC80,
                      'walk_scold_guard':0x75B085,'abstract_load':0x410380,
                      'infantry_object_size':0x5232F0,'noinit_foot_ctor':0x4D3540,
                      'foot_checksum_byte':0x4DBCFE,'crc_byte_helper':0x4A1CA0,
                      'walk_no_head_tail':0x75BCE3,'walk_arrival_mark':0x75BF64,
                      'walk_predicate_tail':0x75BF85,'walk_same_cell_tail':0x75C1FB,
                      'walk_common_tail':0x75C1E7,'walk_post_percell':0x75BE42,
                      'walk_fresh_head_tail':0x75BC2A,
                      'drive_exhausted_guard':0x4B2E47,'drive_first_guard':0x4B3AA1,
                      'ship_exhausted_guard':0x6A2497,'ship_first_guard':0x6A30F0}))
