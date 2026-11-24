"""Conservative PE debug-closure admission for Cargo cache retention.

Only ordinary images without a debug directory are admitted here. Embedded
DWARF still needs the shared streamed split-DWARF check. CodeView/PDB (including
FASTLINK), legacy DBG and unknown debug formats remain blocked: a PDB pathname
alone does not establish that its original compiler objects are disposable.

PE32/PE32+ headers, COFF strings and image debug directory layouts:
https://learn.microsoft.com/en-us/windows/win32/debug/pe-format
"""
import mmap
from pathlib import Path
import struct


def _range(offset: int, size: int, limit: int, what: str):
    if offset < 0 or size < 0 or offset > limit or size > limit - offset:
        raise ValueError(f'Unsupported PE: malformed {what} extent')


def embedded_dwarf(binary: Path) -> bool:
    """Validate the image and return whether it contains DWARF compilation units.

    A false result proves no declared debug closure, never just an MZ prefix.
    This parser does not load other files or follow embedded paths.
    """
    with binary.open('rb') as source:
        if source.seek(0, 2) < 64:
            raise ValueError('Unsupported PE: truncated DOS header')
        with mmap.mmap(source.fileno(), 0, access=mmap.ACCESS_READ) as data:
            return _image(data)


def _image(data) -> bool:
    if data[:2] != b'MZ':
        raise ValueError('Unsupported PE: missing DOS signature')
    pe, = struct.unpack_from('<I', data, 0x3c)
    _range(pe, 24, len(data), 'PE header')
    if pe < 64 or data[pe:pe + 4] != b'PE\0\0':
        raise ValueError('Unsupported PE: invalid image signature')
    _, count, _, symbols, symbol_count, optional_size, flags = struct.unpack_from('<HHIIIHH', data, pe + 4)
    if not flags & 2 or not 1 <= count <= 96:
        raise ValueError('Unsupported PE: non-executable or invalid section count')
    optional = pe + 24
    _range(optional, optional_size, len(data), 'optional header')
    if optional_size < 2:
        raise ValueError('Unsupported PE: missing optional header')
    magic, = struct.unpack_from('<H', data, optional)
    fixed = {0x10b: 96, 0x20b: 112}.get(magic)
    if fixed is None or optional_size < fixed:
        raise ValueError('Unsupported PE optional header format')
    directories, = struct.unpack_from('<I', data, optional + fixed - 4)
    if directories > (optional_size - fixed) // 8:
        raise ValueError('Unsupported PE: truncated data directories')
    headers, = struct.unpack_from('<I', data, optional + 60)
    sections = optional + optional_size
    _range(sections, count * 40, len(data), 'section table')
    if not sections + count * 40 <= headers <= len(data):
        raise ValueError('Unsupported PE: invalid image header size')

    # GNU PE long section names use /decimal offsets into the COFF string table.
    strings = None
    strings_end = 0
    if symbols:
        _range(symbols, symbol_count * 18 + 4, len(data), 'COFF symbol table')
        strings = symbols + symbol_count * 18
        string_size, = struct.unpack_from('<I', data, strings)
        if string_size < 4:
            raise ValueError('Unsupported PE: invalid COFF string table size')
        _range(strings, string_size, len(data), 'COFF string table')
        strings_end = strings + string_size
    elif symbol_count:
        raise ValueError('Unsupported PE: missing COFF symbol table')

    raw_ranges, virtual_ranges = [], []
    has_dwarf = False
    for index in range(count):
        section = sections + index * 40
        name, virtual_size, rva, size, offset = struct.unpack_from('<8sIIII', data, section)
        name = name.rstrip(b'\0')
        if name.startswith(b'/'):
            digits = name[1:]
            if not digits.isdigit() or strings is None:
                raise ValueError('Unsupported PE: unresolved long section name')
            start = strings + int(digits)
            if not strings + 4 <= start < strings_end:
                raise ValueError('Unsupported PE: invalid section-name string offset')
            end = data.find(b'\0', start, strings_end)
            if end < 0:
                raise ValueError('Unsupported PE: unterminated long section name')
            name = data[start:end]
        if not name or any(byte < 33 or byte > 126 for byte in name):
            raise ValueError('Unsupported PE: invalid section name')
        if name == b'.debug' or name.startswith((b'.gnu_debug', b'.debug_sup', b'.debug_cu_index',
                            b'.debug_tu_index', b'.zdebug_', b'.stab', b'.debug$')):
            raise ValueError('Unsupported external/compressed/legacy PE debug closure')
        if name.endswith(b'.dwo'):
            raise ValueError('Unsupported split-DWARF PE debug closure')
        has_dwarf |= name in (b'.debug_info', b'.debug_types')
        if size:
            _range(offset, size, len(data), 'section data')
            if offset < headers:
                raise ValueError('Unsupported PE: section overlaps image headers')
            raw_ranges.append((offset, offset + size))
        span = max(virtual_size, size)
        if span:
            if rva < headers or rva + span > 0x100000000:
                raise ValueError('Unsupported PE: invalid section virtual extent')
            virtual_ranges.append((rva, rva + span))
    for ranges in (raw_ranges, virtual_ranges):
        ordered = sorted(ranges)
        if any(a[1] > b[0] for a, b in zip(ordered, ordered[1:])):
            raise ValueError('Unsupported PE: overlapping image sections')
    if symbols and (symbols < headers or any(symbols < end and strings_end > start
                                             for start, end in raw_ranges)):
        raise ValueError('Unsupported PE: COFF table overlaps image data')

    if directories > 6:
        debug_rva, debug_size = struct.unpack_from('<II', data, optional + fixed + 6 * 8)
        if debug_rva or debug_size:
            # The whole directory is unsupported, including malformed pairs.
            # Never infer an empty closure from missing/unknown CodeView data.
            raise ValueError('Unsupported PE debug directory closure (PDB/DBG/unknown format)')
    # GNU ld also sets IMAGE_FILE_DEBUG_STRIPPED on ordinary no-debug images.
    # The bit alone is not a DBG reference. Microsoft's locator requires a
    # CodeView/MISC directory entry to find debug information in this image:
    # https://github.com/microsoft/microsoft-pdb/blob/master/PDB/dbi/locator.cpp
    return has_dwarf
