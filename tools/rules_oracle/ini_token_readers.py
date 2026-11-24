"""Original CRT atoi/strtok and CCINIClass ReadString/ReadInt/ReadBool on raw values.

`atoi`: 0x007C9BFD (-> atol 0x007C9B72) executes whole with the image's static C
locale tables (`__mb_cur_max` 1 at 0x0087C79C, `_pctype` at 0x0087C590).

`strtok`: 0x007C9CC2 executes whole with the comma delimiter at 0x00817F70. Its
per-thread context comes from `_getptd` 0x007D140B, which reaches TlsGetValue; a
hook answers that call with a zeroed thread block instead. Each row calls
strtok(text) once, then strtok(NULL) until it returns NULL.

`read_string`, `read_int`, `read_bool`: ReadString 0x00528A10, ReadInt 0x005276D0
and ReadBool 0x005295F0 execute whole on the building_body_rules cached section,
index and entry objects (native key CRC 0x004A1DE0). A `null` raw value is an
absent key. Values are Latin-1 bytes, the widening VERA's INI loader applies.
"""
import struct
from pathlib import Path

from unicorn import UC_HOOK_CODE, Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP

from tools.native_oracle import (RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE,
                                 finish_vectors, load_image, provenance, run_checked)
from tools.spatial_oracle.building_body_rules import (ENTRY, INDEX, INI, RAW, SECTION, SP,
                                                       TYPE, Fixture, dwords)

ATOI = 0x7C9BFD
STRTOK = 0x7C9CC2
GETPTD = 0x7D140B
COMMA = 0x817F70
READ_STRING = 0x528A10
READ_INT = 0x5276D0
READ_BOOL = 0x5295F0
KEY = SCRATCH + 0x7000
DEFAULT = SCRATCH + 0x7100
BUFFER = SCRATCH + 0x8000
THREAD_BLOCK = SCRATCH + 0xA000
TEXT = SCRATCH + 0xB000

ATOI_ROWS = [
    '', '0', '12', '-7', '+5', '  42', '\t\n\v\f\r9', '\x1f8', '\xa07', '12abc', 'abc',
    '-', '+', '+-1', '--1', '- 1', '2147483647', '2147483648', '-2147483648',
    '-2147483649', '4294967295', '4294967296', '4294967297', '99999999999', '0x10',
    '010', '1 2', ' -0', '$10', '10h', '1e3', '3.9']
STRTOK_ROWS = [
    '', ',', ',,,', 'a', 'a,b', ',a,,b,', ' a , b ', 'a,,b', 'a,b,', '   ', '\ta\t,\tb',
    'FIRST, SECOND ,,FIRST,,,none,<NoNe>, none , THIRD', '\xe9,\xa0,x']
CAPACITIES = [0x14, 0x18, 0x19, 0x20, 0x40, 0x80]
# strtrim 0x00727CF0 around the old start offset (capacity 0x80).
TRIM_ROWS = [' x ', 'x  ', '  x', '  x ', '   x ', '   x   ', '  x     ', ' ab  ', '  ab ',
             '    ab  ', '    abcd    ', ' a b ', '\t\tx\t\t', '  \xe9 ', '  \xe9\xe9  ',
             '   \xe9 \xe9  ']
# Values and defaults whose ReadString copy is empty (return value 0), the
# gate of every `if (ReadString(...))` caller (capacity 0x80).
EMPTY_ROWS = ['', ' ', '   ', '\t', '\x01\x1f', ' \t ']
EMPTY_DEFAULTS = ['', '   ']
READ_INT_ROWS = [
    None, '9', '-1', '$10', '$ff', '$FFFFFFFF', '$100000000', '$-1', '$', '$g', '$ 5',
    '10h', 'FFh', 'ffH', 'h', 'xh', '-10h', '0x10', ' 12 ', '12junk', '-2147483649',
    '2147483648', '+3', '  -4', '1h2', '$1h', '0$1', '1,2']
READ_BOOL_ROWS = [
    None, 'yes', 'Y', 'n', 'T', 'F', 'true', 'false', '1', '0', '2', ' yes', 'YES',
    'junk', 'Nope', 'Yep', '0anything']


def read_c_string(uc, address, limit=0x800):
    raw = bytes(uc.mem_read(address, limit))
    return raw.split(b'\0', 1)[0].decode('latin-1')


class Crt:
    def __init__(self):
        u = self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(SCRATCH, 0x10000)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(RET_MAGIC, 0x1000)
        self.thread_block_calls = 0
        u.hook_add(UC_HOOK_CODE, self.thread_block, begin=GETPTD, end=GETPTD)

    def thread_block(self, uc, _address, _size, _data):
        # _getptd returns the thread's zero-initialized _tiddata; strtok keeps
        # its continuation pointer there.
        self.thread_block_calls += 1
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(UC_X86_REG_ESP, esp + 4)
        uc.reg_write(UC_X86_REG_EAX, THREAD_BLOCK)
        uc.reg_write(UC_X86_REG_EIP, ret)

    def cdecl(self, func, *args):
        u = self.u
        u.mem_write(SP, dwords(RET_MAGIC, *args))
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, func, RET_MAGIC, count=100_000)
        return u.reg_read(UC_X86_REG_EAX)

    def atoi(self, raw):
        self.u.mem_write(TEXT, raw.encode('latin-1') + b'\0')
        return struct.unpack('<i', dwords(self.cdecl(ATOI, TEXT)))[0]

    def strtok(self, raw):
        self.u.mem_write(THREAD_BLOCK, bytes(0x100))
        self.u.mem_write(TEXT, raw.encode('latin-1') + b'\0')
        before = self.thread_block_calls
        tokens = []
        pointer = self.cdecl(STRTOK, TEXT, COMMA)
        while pointer:
            tokens.append(read_c_string(self.u, pointer))
            pointer = self.cdecl(STRTOK, 0, COMMA)
        assert self.thread_block_calls == before + len(tokens) + 1, raw
        return tokens


class Reader(Fixture):
    def __init__(self):
        super().__init__()
        self.u.mem_write(KEY, b'Key\0')

    def entry(self, raw):
        # building_body_rules.Fixture.ini with Latin-1 value bytes.
        self.ini(KEY, None if raw is None else '')
        if raw is not None:
            self.u.mem_write(RAW, raw.encode('latin-1') + b'\0')

    def read_string(self, raw, default, capacity):
        u = self.u
        self.entry(raw)
        u.mem_write(DEFAULT, default.encode('latin-1') + b'\0')
        u.mem_write(BUFFER, b'\xcc' * 0x400)
        u.mem_write(SP, dwords(RET_MAGIC, TYPE + 0x1F8, KEY, DEFAULT, BUFFER, capacity))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, INI)
        run_checked(u, READ_STRING, RET_MAGIC, count=100_000)
        return read_c_string(u, BUFFER), u.reg_read(UC_X86_REG_EAX)

    def scalar_latin1(self, raw, default, reader):
        u = self.u
        self.entry(raw)
        u.mem_write(SP, dwords(RET_MAGIC, TYPE + 0x1F8, KEY, default))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, INI)
        run_checked(u, reader, RET_MAGIC, count=100_000)
        value = u.reg_read(UC_X86_REG_EAX)
        return struct.unpack('<i', dwords(value))[0] if reader == READ_INT else bool(value & 255)


def read_string_rows(reader):
    rows = []
    for capacity in CAPACITIES:
        payload = capacity - 1
        values = ['abc', 'A' * payload, 'A' * capacity, 'A' * (capacity + 5),
                  'B' * (payload - 1) + ' Tail', '  x  ', '\x01x\x1f', 'x' + ' ' * capacity + 'y',
                  '\xe9t\xe9']
        for raw in values:
            output, length = reader.read_string(raw, '', capacity)
            rows.append(dict(raw=raw, default='', capacity=capacity, output=output, length=length))
        for default in ('Easy', ' pad ', '  x  ', 'D' * (capacity + 3)):
            output, length = reader.read_string(None, default, capacity)
            rows.append(dict(raw=None, default=default, capacity=capacity, output=output,
                             length=length))
    for raw in TRIM_ROWS + EMPTY_ROWS:
        output, length = reader.read_string(raw, '', 0x80)
        rows.append(dict(raw=raw, default='', capacity=0x80, output=output, length=length))
    for default in EMPTY_DEFAULTS:
        output, length = reader.read_string(None, default, 0x80)
        rows.append(dict(raw=None, default=default, capacity=0x80, output=output,
                         length=length))
    return rows


def generate():
    crt = Crt()
    reader = Reader()
    present, _ = reader.read_string('abc', 'default', 0x20)
    assert present == 'abc', 'the cached-entry fixture must reach the present value'
    return {
        'atoi': [dict(raw=raw, output=crt.atoi(raw)) for raw in ATOI_ROWS],
        'strtok': [dict(raw=raw, tokens=crt.strtok(raw)) for raw in STRTOK_ROWS],
        'read_string': read_string_rows(reader),
        'read_int': [dict(raw=raw, default=default, output=reader.scalar_latin1(raw, default, READ_INT))
                     for raw in READ_INT_ROWS for default in (9, -1)],
        'read_bool': [dict(raw=raw, default=default,
                           output=reader.scalar_latin1(raw, int(default), READ_BOOL))
                      for raw in READ_BOOL_ROWS for default in (False, True)],
    }


def metadata():
    return provenance(
        scope='Original CRT atoi and strtok, and CCINIClass ReadString, ReadInt and ReadBool on raw values',
        assumptions=[
            'atoi 0x007C9BFD and atol 0x007C9B72 execute whole on the static C-locale tables the image initializes: __mb_cur_max 1 (0x0087C79C) and _pctype 0x0087C59A (0x0087C590).',
            'strtok 0x007C9CC2 executes whole with the retail comma delimiter string 0x00817F70; each row starts from a zeroed per-thread block.',
            'ReadString 0x00528A10, ReadInt 0x005276D0 and ReadBool 0x005295F0 execute whole on one present or absent key; value bytes are Latin-1, as VERA widens INI bytes.'],
        substitutions=[
            '_getptd 0x007D140B (TlsGetValue and calloc) is answered by a hook that returns a zeroed thread block.',
            'building_body_rules.Fixture supplies the cached INI section, index and entry objects; no physical INI load.'],
        entry_points={'atoi': ATOI, 'strtok': STRTOK, 'read_string': READ_STRING,
                      'read_int': READ_INT, 'read_bool': READ_BOOL, 'ini_crc': 0x4A1DE0})


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
