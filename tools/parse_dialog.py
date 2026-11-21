"""Parse a Win32 RT_DIALOG resource from a PE and dump its controls.

Usage: python parse_dialog.py <exe> <dialog_id>
Dumps every DLGITEMTEMPLATE's control id and rectangle (x, y, w, h in DLUs).
"""
import struct
import sys


def u16(b, o):
    return struct.unpack_from('<H', b, o)[0]


def u32(b, o):
    return struct.unpack_from('<I', b, o)[0]


def read_pe(path):
    with open(path, 'rb') as f:
        data = f.read()
    assert data[:2] == b'MZ', 'not a PE'
    pe = u32(data, 0x3C)
    assert data[pe:pe + 4] == b'PE\0\0', 'bad PE sig'
    coff = pe + 4
    nsec = u16(data, coff + 2)
    opt_size = u16(data, coff + 16)
    opt = coff + 20
    magic = u16(data, opt)
    pe32plus = magic == 0x20B
    # Data directory index 2 = resource. offset in optional header differs.
    dd = opt + (112 if pe32plus else 96)
    rsrc_rva = u32(data, dd + 2 * 8)
    rsrc_size = u32(data, dd + 2 * 8 + 4)
    sections = []
    so = opt + opt_size
    for i in range(nsec):
        s = so + i * 40
        name = data[s:s + 8].rstrip(b'\0').decode('latin1')
        vsize = u32(data, s + 8)
        vaddr = u32(data, s + 12)
        rawsize = u32(data, s + 16)
        rawptr = u32(data, s + 20)
        sections.append((vaddr, vsize, rawptr, rawsize, name))
    return data, sections, rsrc_rva, rsrc_size


def rva_to_off(sections, rva):
    for vaddr, vsize, rawptr, rawsize, name in sections:
        if vaddr <= rva < vaddr + max(vsize, rawsize):
            return rawptr + (rva - vaddr)
    raise ValueError(f'RVA {rva:#x} not in any section')


def walk_rsrc(data, sections, rsrc_rva, rsrc_size):
    base = rva_to_off(sections, rsrc_rva)
    # Level 1: type directory. Our target type = RT_DIALOG (5).
    def dir_entries(off):
        named = u16(data, off + 12)
        ids = u16(data, off + 14)
        out = []
        p = off + 16
        for _ in range(named + ids):
            name = u32(data, p)
            target = u32(data, p + 4)
            out.append((name, target))
            p += 8
        return out

    root = base
    dialog_type_off = None
    for name, target in dir_entries(root):
        if name == 5:  # RT_DIALOG
            dialog_type_off = base + (target & 0x7FFFFFFF)
            break
    if dialog_type_off is None:
        return []
    results = []
    # Level 2: dialog name/id directory
    for name, target in dir_entries(dialog_type_off):
        if name & 0x80000000:
            continue  # named dialogs; our target is numeric
        dialog_id = name
        lang_dir_off = base + (target & 0x7FFFFFFF)
        # Level 3: language directory -> data entry
        for lname, ltarget in dir_entries(lang_dir_off):
            data_entry_off = base + (ltarget & 0x7FFFFFFF)
            rva = u32(data, data_entry_off)
            size = u32(data, data_entry_off + 4)
            off = rva_to_off(sections, rva)
            results.append((dialog_id, data[off:off + size]))
            break
    return results


def skip_unicode_str(b, o):
    # 0xFFFF = no string; otherwise a UTF-16 string terminated by NUL.
    if u16(b, o) == 0xFFFF:
        return o + 2
    p = o
    while u16(b, p) != 0:
        p += 2
    return p + 2


def parse_dialog(b):
    o = 0
    # Detect DLGTEMPLATEEX: wVersion=1, wSignature=0xFFFF
    is_ex = u16(b, 0) == 1 and u16(b, 2) == 0xFFFF
    if is_ex:
        o = 4  # skip version + signature
    style = u32(b, o); o += 4
    exstyle = u32(b, o); o += 4
    x = u16(b, o); y = u16(b, o + 2); cx = u16(b, o + 4); cy = u16(b, o + 6)
    o += 8
    # menu, windowClass, title (all unicode strings)
    o = skip_unicode_str(b, o)  # menu
    o = skip_unicode_str(b, o)  # windowClass
    o = skip_unicode_str(b, o)  # title
    if is_ex:
        # DLGTEMPLATEEX font: pointsize(2) weight(2) italic(1) charset(1) typeface
        if style & 0x40:  # DS_SETFONT
            o += 2 + 2 + 1 + 1
            o = skip_unicode_str(b, o)  # typeface
    else:
        if style & 0x40:
            o += 2
            o = skip_unicode_str(b, o)
    dlu = u16(b, o); o += 2  # number of items
    items = []
    for _ in range(dlu):
        o = (o + 3) & ~3  # DWORD align
        if is_ex:
            # DLGITEMTEMPLATEEX: helpID(4), exStyle(4), style(4), rect(8), id(2)
            helpid = u32(b, o); o += 4
            iexstyle = u32(b, o); o += 4
            istyle = u32(b, o); o += 4
            ix = u16(b, o); iy = u16(b, o + 2); icx = u16(b, o + 4); icy = u16(b, o + 6)
            o += 8
            cid = u16(b, o); o += 2
        else:
            istyle = u32(b, o); o += 4
            iexstyle = u32(b, o); o += 4
            ix = u16(b, o); iy = u16(b, o + 2); icx = u16(b, o + 4); icy = u16(b, o + 6)
            o += 8
            cid = u16(b, o); o += 2
        o = skip_unicode_str(b, o)  # windowClass
        o = skip_unicode_str(b, o)  # title
        extra = u16(b, o); o += 2
        o += extra
        items.append((cid, ix, iy, icx, icy, istyle))
    return (x, y, cx, cy, style), items


def main():
    exe = sys.argv[1]
    want = int(sys.argv[2], 0)
    data, sections, rva, size = read_pe(exe)
    dialogs = walk_rsrc(data, sections, rva, size)
    found = False
    for did, raw in dialogs:
        if did != want:
            continue
        found = True
        hdr, items = parse_dialog(raw)
        print(f'Dialog 0x{did:X}: rect=({hdr[0]},{hdr[1]},{hdr[2]},{hdr[3]}) style=0x{hdr[4]:X}')
        for cid, x, y, cx, cy, style in items:
            print(f'  id=0x{cid:04X} ({cid:5d})  x={x:4d} y={y:4d} w={cx:4d} h={cy:4d}  style=0x{style:08X}')
    if not found:
        print(f'Dialog 0x{want:X} not found. Available ids:')
        print('  ' + ', '.join(f'0x{d:X}' for d, _ in dialogs[:80]))


if __name__ == '__main__':
    main()
