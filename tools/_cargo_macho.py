"""Read Mach-O debug inputs without opening (or requiring) those input files.

This is a conservative retention inventory, not a DWARF linker. LLVM dsymutil's
MachODebugMapParser::handleStabSymbolTableEntry loads N_OSO and N_AST references;
its normal debug-map output can OMIT missing objects and duplicate OSO entries.
We retain every such reference, including duplicates and all universal slices.
https://github.com/llvm/llvm-project/blob/release/17.x/llvm/tools/dsymutil/MachODebugMapParser.cpp

Wire layouts come from Apple's loader.h, nlist.h and fat.h (not host C layouts):
https://github.com/apple-oss-distributions/cctools/blob/main/include/mach-o/loader.h
https://github.com/apple-oss-distributions/cctools/blob/main/include/mach-o/nlist.h
https://github.com/apple-oss-distributions/cctools/blob/main/include/mach-o/fat.h
"""
import mmap
import os
from pathlib import Path, PurePosixPath
import stat
import struct


_THIN = {
    b'\xce\xfa\xed\xfe': ('<', False), b'\xfe\xed\xfa\xce': ('>', False),
    b'\xcf\xfa\xed\xfe': ('<', True), b'\xfe\xed\xfa\xcf': ('>', True),
}
_FAT = {
    b'\xca\xfe\xba\xbe': ('>', False), b'\xbe\xba\xfe\xca': ('<', False),
    b'\xca\xfe\xba\xbf': ('>', True), b'\xbf\xba\xfe\xca': ('<', True),
}


def _range(offset: int, size: int, length: int, what: str) -> None:
    if offset < 0 or size < 0 or offset > length or size > length - offset:
        raise ValueError(f'Malformed Mach-O {what}: out of bounds')


def _path(name: bytes, kind: int) -> Path:
    # Strict UTF-8 is deliberate: unresolved filenames must block deletion,
    # rather than be silently replaced or normalized into a different path.
    text = name.decode('utf-8', errors='strict')
    if not text or any(ord(char) < 32 or ord(char) == 127 for char in text):
        raise ValueError('Malformed Mach-O debug dependency path')
    if kind == 0x66 and text.endswith(')'):
        # BinaryHolder::isArchive/getArchiveAndObjectName uses the LAST '('.
        # N_AST paths are direct module files, not BinaryHolder archive inputs.
        # https://github.com/llvm/llvm-project/blob/release/17.x/llvm/tools/dsymutil/BinaryHolder.cpp
        opening = text.rfind('(')
        if opening < 1 or opening == len(text) - 2:
            raise ValueError('Malformed Mach-O archive dependency')
        text = text[:opening]
    path = PurePosixPath(text)
    if not path.is_absolute() or '..' in path.parts or path == PurePosixPath('/'):
        raise ValueError(f'Unresolved Mach-O debug dependency: {text}')
    return Path(text)


def _thin(data: mmap.mmap, base: int, size: int,
          architecture: tuple[int, int] | None = None,
          *, require_executable: bool = False) -> set[Path]:
    _range(base, size, len(data), 'slice')
    if size < 4 or data[base:base + 4] not in _THIN:
        raise ValueError('Unsupported Mach-O slice magic')
    endian, wide = _THIN[data[base:base + 4]]
    header_size = 32 if wide else 28
    _range(0, header_size, size, 'header')
    cpu, subtype, filetype, commands, command_bytes = struct.unpack_from(endian + '5I', data, base + 4)
    if architecture is not None and architecture != (cpu, subtype):
        raise ValueError('Mach-O universal architecture/header mismatch')
    # Filesets contain nested images; old prebound/core formats are outside
    # this executable/object inspector's contract and must not appear empty.
    if require_executable and filetype != 2:
        raise ValueError(f'Mach-O export must be MH_EXECUTE, got {filetype}')
    if filetype not in {1, 2, 6, 7, 8, 10}:
        raise ValueError(f'Unsupported Mach-O file type: {filetype}')
    _range(header_size, command_bytes, size, 'load commands')
    command_end = header_size + command_bytes
    if commands > command_bytes // 8:
        raise ValueError('Malformed Mach-O load command count')
    position = header_size
    table = None
    for _ in range(commands):
        _range(position, 8, command_end, 'load command header')
        kind, length = struct.unpack_from(endian + '2I', data, base + position)
        if length < 8 or length % (8 if wide else 4):
            raise ValueError('Malformed Mach-O load command size/alignment')
        _range(position, length, command_end, 'load command')
        if kind == 2:  # LC_SYMTAB: offsets are relative to this thin slice.
            if length != 24 or table is not None:
                raise ValueError('Malformed or duplicate Mach-O LC_SYMTAB')
            table = struct.unpack_from(endian + '4I', data, base + position + 8)
        position += length
    if position != command_end:
        raise ValueError('Malformed Mach-O load command extent')
    if table is None:
        return set()  # A valid stripped image has no symbol-based debug inputs.
    symbols, count, strings, string_bytes = table
    entry_size = 16 if wide else 12
    symbol_bytes = count * entry_size
    _range(symbols, symbol_bytes, size, 'symbol table')
    _range(strings, string_bytes, size, 'string table')
    if (symbol_bytes and symbols < command_end) or (string_bytes and strings < command_end):
        raise ValueError('Mach-O symbol/string table overlaps load commands')
    if symbol_bytes and string_bytes and max(symbols, strings) < min(symbols + symbol_bytes, strings + string_bytes):
        raise ValueError('Mach-O symbol and string tables overlap')
    if count and not string_bytes:
        raise ValueError('Mach-O symbols have no string table')
    refs = set()
    index_reader = struct.Struct(endian + 'I')
    string_end = base + strings + string_bytes
    # Check every index, but decode only OSO/AST paths. Million-symbol libtests
    # otherwise pay for parsing/printing full dsymutil maps on every cleanup.
    for position in range(base + symbols, base + symbols + symbol_bytes, entry_size):
        index = index_reader.unpack_from(data, position)[0]
        if index >= string_bytes:
            raise ValueError('Mach-O symbol string index out of bounds')
        kind = data[position + 4]
        if kind not in (0x66, 0x32):  # N_OSO, N_AST
            continue
        start = base + strings + index
        end = data.find(b'\0', start, string_end)
        if end < 0:
            raise ValueError('Unterminated Mach-O debug dependency path')
        refs.add(_path(data[start:end], kind))
    return refs


def dependencies(binary: Path, *, require_executable: bool = False) -> set[Path]:
    """Return every OSO/AST input; missing inputs are still returned.

    require_executable rejects non-MH_EXECUTE files in every slice.
    Raises OSError/ValueError for unreadable, unsupported or malformed files.
    The caller owns the build lock and file-identity checks against mutation.
    """
    with binary.open('rb') as source:
        status = source.fileno()
        info = os.fstat(status)
        if not stat.S_ISREG(info.st_mode) or info.st_size < 4:
            raise ValueError('Mach-O input must be a nonempty regular file')
        with mmap.mmap(status, 0, access=mmap.ACCESS_READ) as data:
            magic = data[:4]
            if magic in _THIN:
                return _thin(data, 0, len(data), require_executable=require_executable)
            if magic not in _FAT:
                raise ValueError('Unsupported Mach-O magic')
            endian, wide = _FAT[magic]
            _range(0, 8, len(data), 'universal header')
            count = struct.unpack_from(endian + 'I', data, 4)[0]
            entry_size = 32 if wide else 20
            _range(8, count * entry_size, len(data), 'universal architecture table')
            if not count:
                raise ValueError('Empty Mach-O universal architecture table')
            header_end = 8 + count * entry_size
            slices = []
            for index in range(count):
                fields = struct.unpack_from(endian + ('2I2Q2I' if wide else '5I'),
                                            data, 8 + index * entry_size)
                cpu, subtype, offset, size, alignment = fields[:5]
                if wide and fields[5] != 0:
                    raise ValueError('Nonzero Mach-O universal reserved field')
                _range(offset, size, len(data), 'universal slice')
                if size < 4 or offset < header_end or alignment > 63 or offset % (1 << alignment):
                    raise ValueError('Malformed Mach-O universal slice layout')
                slices.append((offset, size, cpu, subtype))
            slices.sort()
            previous_end = header_end
            refs = set()
            for offset, size, cpu, subtype in slices:
                if offset < previous_end:
                    raise ValueError('Overlapping Mach-O universal slices')
                refs.update(_thin(data, offset, size, (cpu, subtype), require_executable=require_executable))
                previous_end = offset + size
            return refs
