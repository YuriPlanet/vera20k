"""Synthetic PE inputs for portable reader tests; never retail evidence."""
import struct

from tools.native_oracle import IMAGE_BASE


def pe_image(sections=None):
    """Build PE32 bytes from (RVA, raw pointer, bytes, virtual size, flags) rows."""
    if sections is None:
        sections = [(0x1000, 0x200, b"\x90\x90\xc3\xcc", 0x20, 0x60000020),
                    (0x3000, 0x400, b"retail-fixture", 0x100, 0xC0000040)]
    sections = list(sections)
    pe, optional_size = 0x80, 0xE0
    table = pe + 24 + optional_size
    size = max([table + len(sections) * 40]
               + [raw + len(payload) for _, raw, payload, _, _ in sections])
    data = bytearray(size)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, pe)
    data[pe:pe + 4] = b"PE\0\0"
    struct.pack_into("<HH", data, pe + 4, 0x14C, len(sections))
    struct.pack_into("<H", data, pe + 20, optional_size)
    struct.pack_into("<H", data, pe + 24, 0x10B)
    struct.pack_into("<I", data, pe + 24 + 28, IMAGE_BASE)
    for index, (rva, raw, payload, virtual_size, flags) in enumerate(sections):
        offset = table + index * 40
        struct.pack_into("<IIII", data, offset + 8, virtual_size, rva, len(payload), raw)
        struct.pack_into("<I", data, offset + 36, flags)
        data[raw:raw + len(payload)] = payload
    return bytes(data)
