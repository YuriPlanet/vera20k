"""Original theater545150 General integer reads and ordinal projection.

The complete56-read block545535..545C3F runs, including native defaults,
read order, scalar parser and result stores. Supplied INI caches retain source
order and duplicate records; original sort/search executes. This is not full
INI file loading or a full theater/TMP loader. See the adjacent Markdown.
"""
from pathlib import Path
import hashlib
import os
import struct
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
from capstone.x86_const import X86_OP_MEM, X86_OP_REG
from unicorn import UC_HOOK_MEM_WRITE
from unicorn.x86_const import *
from tools.native_oracle import run_checked, provenance, finish_vectors
from tools.rules_oracle.bridge_anim_inputs import Reader, crc
from tools.spatial_oracle.building_body_rules import INI, SP, dwords

ROOT = Path(os.environ.get('VERA20K_THEATER_GENERAL_ASSETS', 'ini'))
FILES = ('temperatmd.ini', 'snowmd.ini', 'urbanmd.ini', 'lunarmd.ini',
         'desertmd.ini', 'urbannmd.ini')
BEGIN, END = 0x545535, 0x545C3F
PROJECT_BEGIN, PROJECT_END = 0x545CEF, 0x545FA3


def signed(value):
    return struct.unpack('<i', dwords(value & 0xFFFFFFFF))[0]


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def lexical_records(text):
    """Prepare ordered physical records; never select a duplicate winner."""
    sections = []
    current = None
    for line in text.splitlines():
        assert len(line.encode('latin1')) <= 511, 'long physical lines are outside this fixture'
        line = line.strip(''.join(map(chr, range(33))))
        if line.startswith('[') and ']' in line:
            current = [line[1:line.index(']')], []]
            sections.append(current)
            continue
        if current is None:
            continue
        line = line.split(';', 1)[0]
        if '=' not in line:
            continue
        key, value = [part.strip(''.join(map(chr, range(33)))) for part in line.split('=', 1)]
        if key and value:
            current[1].append([key, value])
    return [section for section in sections if section[1]]


def general_text(raw):
    result = []
    active = False
    for line in raw.decode('latin1').splitlines(keepends=True):
        stripped = line.strip()
        if stripped.startswith('[') and ']' in stripped:
            active = stripped[1:stripped.index(']')] == 'General'
        if active:
            result.append(line)
    return ''.join(result)


class TheaterReader(Reader):
    def __init__(self):
        self.phase = 'setup'
        self.reads = []
        self.pending_returns = {}
        super().__init__(ROOT, {})
        md = Cs(CS_ARCH_X86, CS_MODE_32)
        md.detail = True
        instructions = list(md.disasm(bytes(self.u.mem_read(BEGIN, END - BEGIN)), BEGIN))
        self.result_stores = {
            i.address for i in instructions
            if i.mnemonic == 'mov' and i.operands[0].type == X86_OP_MEM
            and i.operands[1].type == X86_OP_REG and i.reg_name(i.operands[1].reg) == 'eax'
        }
        self.role_globals = {}
        offset = None
        for i in md.disasm(bytes(self.u.mem_read(PROJECT_BEGIN, PROJECT_END - PROJECT_BEGIN)), PROJECT_BEGIN):
            if i.mnemonic in ('mov', 'cmp') and len(i.operands) == 2:
                source = i.operands[1]
                if source.type == X86_OP_MEM and i.reg_name(source.mem.base) == 'esp':
                    offset = source.mem.disp
                if (i.mnemonic == 'mov' and i.operands[0].type == X86_OP_MEM
                        and source.type == X86_OP_REG and i.reg_name(source.reg) == 'ebx'):
                    assert offset is not None
                    self.role_globals[offset] = i.operands[0].mem.disp
        assert len(self.role_globals) == 46
        self.spans = ((BEGIN, END), (PROJECT_BEGIN, PROJECT_END), (0x5276D0, 0x5278EA))
        self.code_before = [bytes(self.u.mem_read(a, b - a)) for a, b in self.spans]
        self.u.hook_add(UC_HOOK_MEM_WRITE, self.record_store)

    def hook(self, u, pc, size, data):
        if self.phase == 'read' and pc == 0x5276D0:
            sp = u.reg_read(UC_X86_REG_ESP)
            ret = self.read32(sp)
            row = dict(key=self.string(self.read32(sp + 8)),
                       section=self.string(self.read32(sp + 4)),
                       default=signed(self.read32(sp + 12)), call=hex(ret - 5))
            self.reads.append(row)
            self.pending_returns[ret] = row
        elif self.phase == 'read' and pc in self.pending_returns:
            self.pending_returns.pop(pc)['value'] = signed(u.reg_read(UC_X86_REG_EAX))
        super().hook(u, pc, size, data)

    def record_store(self, u, _access, address, size, value, _data):
        if self.phase != 'read' or u.reg_read(UC_X86_REG_EIP) not in self.result_stores:
            return
        row = self.reads[-1]
        assert size == 4 and signed(value) == row['value'], row
        row['store'] = hex(u.reg_read(UC_X86_REG_EIP))
        row['storage'] = 'frame' if SP <= address < SP + 0x1000 else 'global'
        row['location'] = hex(address - SP if row['storage'] == 'frame' else address)

    def prepare_records(self, records):
        self.u.mem_write(INI, bytes(0x40))
        section_index = []
        for name, keys in records:
            sec = self.alloc(0x44)
            self.u.mem_write(sec + 0xC, dwords(self.cstring(name)))
            entries = []
            for key, value in keys:
                entry = self.alloc(0x28)
                self.u.mem_write(entry + 0xC, dwords(self.cstring(key), self.cstring(value)))
                entries.append((crc(key), entry))
            items = self.alloc(len(entries) * 8)
            for index, (key_crc, pointer) in enumerate(entries):
                self.u.mem_write(items + index * 8, dwords(key_crc, pointer))
            # Ordered, not pre-sorted: native FindEntry performs its own sort.
            self.u.mem_write(sec + 0x2C, dwords(items, len(entries), len(entries), 0, 0))
            section_index.append((crc(name), sec))
        items = self.alloc(len(section_index) * 8)
        for index, (name_crc, pointer) in enumerate(section_index):
            self.u.mem_write(items + index * 8, dwords(name_crc, pointer))
        self.u.mem_write(INI + 0x28, dwords(items, len(section_index), len(section_index), 0, 0))

    def read(self, text):
        self.phase = 'setup'
        records = lexical_records(text)
        self.prepare_records(records)
        self.u.mem_write(SP, bytes(0x1000))
        self.u.mem_write(SP + 0x38, bytes(self.u.mem_read(INI, 0x40)))
        self.u.reg_write(UC_X86_REG_ESP, SP)
        self.reads = []
        self.pending_returns = {}
        self.phase = 'read'
        run_checked(self.u, BEGIN, END, count=3000000,
                    required_addresses=(0x5276D0, 0x545978, 0x545C3A))
        self.phase = 'setup'
        assert len(self.reads) == 56 and not self.pending_returns
        assert self.u.reg_read(UC_X86_REG_ESP) == SP
        for row in self.reads:
            assert row['section'] == 'General' and 'storage' in row
            location = int(row['location'], 16)
            if row['storage'] == 'frame':
                row['resolved_global'] = hex(self.role_globals[location])
                location += SP
            assert signed(self.read32(location)) == row['value']
        assert self.code_before == [bytes(self.u.mem_read(a, b - a)) for a, b in self.spans]
        return dict(values={r['key']: r['value'] for r in self.reads})

    def project(self, ordinal, base):
        self.u.reg_write(UC_X86_REG_ESP, SP)
        self.u.reg_write(UC_X86_REG_EDI, ordinal)
        self.u.reg_write(UC_X86_REG_EBX, base)
        run_checked(self.u, PROJECT_BEGIN, PROJECT_END, count=10000)
        return dict(supplied_ordinal=ordinal, supplied_cumulative_base=base,
                    values={r['key']: signed(self.read32(int(r['resolved_global'], 16)))
                            for r in self.reads if r['storage'] == 'frame'})


def uniform(keys, value, section='General', key_transform=lambda key: key):
    return '[' + section + ']\n' + ''.join(f'{key_transform(k)}={value}\n' for k in keys)


def generate():
    m = TheaterReader()
    m.read('')
    contract = [{key: value for key, value in r.items() if key != 'value'} for r in m.reads]
    keys = [r['key'] for r in contract]
    assert all(r['default'] == (-2 if r['key'] == 'DestroyableCliffs' else -1) for r in contract)
    cases = [('missing_section', '[Other]\nBridgeSet=19\n'),
             ('missing_keys', '[General]\nUnrelated=1\n'),
             ('lowercase_section', uniform(keys, '19', section='general')),
             ('lowercase_keys', uniform(keys, '19', key_transform=str.lower)),
             ('exact_and_lowercase_keys', '[General]\nbridgeset=9\nBridgeSet=19\n'),
             ('exact_and_lowercase_sections', '[general]\nBridgeSet=9\n[General]\nBridgeSet=19\n')]
    cases += [(name, uniform(keys, value)) for name, value in [
        ('decimal', '19'), ('zero', '0'), ('hex_dollar', '$13'), ('hex_suffix', '13h'),
        ('hex_suffix_upper', '13H'), ('numeric_suffix', '19tail'), ('decimal_with_fraction', '19.75'),
        ('zero_x_prefix', '0x13'), ('invalid_text', 'bogus'), ('failed_hex', '$nothex'),
        ('missing_empty', ''), ('missing_whitespace', ' \t '), ('negative_one', '-1'),
        ('negative_two', '-2'), ('negative_seven', '-7'), ('signed_plus', '+19'),
        ('u16_max', '65535'), ('beyond_u16', '65537'), ('i32_max', '2147483647'),
        ('i32_min', '-2147483648'), ('decimal_wrap', '4294967295'),
        ('hex_signed', '$FFFFFFFF'), ('inline_comment', '19 ; ignored')]]
    cases += [
        ('duplicate_key', '[General]\nBridgeSet=19\nBridgeSet=23\n'),
        ('duplicate_key_reversed', '[General]\nBridgeSet=23\nBridgeSet=19\n'),
        ('duplicate_empty_then_value', '[General]\nBridgeSet=\nBridgeSet=19\n'),
        ('duplicate_section', '[General]\nBridgeSet=19\n[General]\nBridgeSet=23\n'),
        ('duplicate_section_reversed', '[General]\nBridgeSet=23\n[General]\nBridgeSet=19\n')]
    rows = []
    for name, text in cases:
        rows.append(dict(name=name, input_text=text, **m.read(text)))
    physical = []
    for name in FILES:
        path = ROOT / name
        raw = path.read_bytes()
        text = general_text(raw)
        records = lexical_records(text)
        assert len(records) == 1 and records[0][0] == 'General'
        assert len({key for key, value in records[0][1]}) == len(records[0][1])
        physical.append(dict(file=name, bytes=len(raw), sha256=sha(raw),
                             general_text=text, **m.read(text)))
    projection = []
    text = '[General]\nBridgeSet=65537\nWoodBridgeSet=-1\nCliffSet=1\nDestroyableCliffs=-2\nBridgeMiddle1=-7\n'
    for ordinal, base in ((0, 0), (1, 70000), (65537, 123456)):
        row = dict(name='signed_ordinal_' + str(ordinal), input_text=text, **m.read(text))
        row['ordinal_projection'] = m.project(ordinal, base)
        projection.append(row)
    ordinal_keys = [r['key'] for r in contract if r['storage'] == 'frame']
    text = uniform(ordinal_keys, '$13')
    row = dict(name='all_ordinals_hex_19', input_text=text, **m.read(text))
    row['ordinal_projection'] = m.project(19, 1234)
    projection.append(row)
    text = '[General]\nBridgeSet=$13\nWoodBridgeSet=1\nCliffSet=bogus\nDestroyableCliffs=$nothex\n'
    for ordinal, base in ((0, 0), (1, 7), (19, 1234)):
        row = dict(name='mixed_ordinal_' + str(ordinal), input_text=text, **m.read(text))
        row['ordinal_projection'] = m.project(ordinal, base)
        projection.append(row)
    return dict(schema_version=1, read_contract=contract,
                cases=rows, physical=physical, projection_cases=projection,
                instruction_spans=[dict(begin=hex(a), end=hex(b), sha256=sha(raw))
                                   for (a, b), raw in zip(m.spans, m.code_before)])


def metadata():
    return provenance(scope=__doc__,
        entry_points={'theater_loader': 0x545150, 'general_begin': BEGIN, 'general_stop': END,
                      'read_int': 0x5276D0, 'ordinal_projection_begin': PROJECT_BEGIN,
                      'ordinal_projection_stop': PROJECT_END},
        assumptions=[
            'The complete contiguous56-read General block executes with a supplied loader frame and INI cache. Every call/default/key/result store is observed, with no replacement integer parser. Original executable bytes remain unchanged.',
            'Physical six-theater General text comes from retail files whose full-file hashes are retained. Unique General section/keys are asserted. The supplied lexical boundary trims ASCII controls, strips entry comments, omits empty entries/sections and retains duplicate records; long physical lines are excluded.',
            'INI CRC arrays preserve physical order and begin unsorted, so original native sort/search chooses duplicate winners. This covers only the stated bounded duplicate layouts; full physical INI loading, arbitrary duplicate/collision behavior and archive precedence are not certified.',
            'All results are original signed DWORDs. Ten bridge piece globals retain them directly. Ordinal controls execute the original compare/publish block with explicitly supplied loop ordinal and cumulative base; they do not execute TMP construction, count accumulation, loader termination or later lunar global zeroing.',
            'Seven ordinal controls retain three signed-width cases and add four controls within current u16 tile projections: all46 ordinal keys=$13 at ordinal19/base1234, plus mixed BridgeSet=$13, WoodBridgeSet=1, CliffSet=bogus and DestroyableCliffs=$nothex at ordinal/base0/0,1/7,19/1234.',
            'Python success establishes bounded native evidence only. Production Rust reader comparisons and the release theater/map loader are separate validation obligations.'],
        substitutions=[
            'Shared Reader/Fixture supplies the original image, bump allocator, inert delete and CRT TLS context. Prepared INI records substitute physical archive/file loading; no Reader result is overridden.'])


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
