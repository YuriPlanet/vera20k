"""Extract one named entry from a Westwood MIX (with XCC name database)."""
import os
import struct
import sys

MIX_MAGIC = {0x00010000, 0x00020000}


def read_names(db_path):
    names = {}
    if not db_path or not os.path.isfile(db_path):
        return names
    with open(db_path, "rb") as f:
        data = f.read()
    import re
    for m in re.finditer(rb"[ -~]{2,64}\.SHP\x00", data, re.I):
        stem = m.group()[:-5].decode("ascii", "ignore").lower()
        # XCC stores name then hash in 4-byte little endian after the name
        h = struct.unpack_from("<I", data, m.end())[0]
        names[h] = stem + ".shp"
    for m in re.finditer(rb"[ -~]{2,64}\.PAL\x00", data, re.I):
        stem = m.group()[:-5].decode("ascii", "ignore").lower()
        h = struct.unpack_from("<I", data, m.end())[0]
        names[h] = stem + ".pal"
    return names


def parse_mix(path):
    with open(path, "rb") as f:
        data = f.read()
    magic = struct.unpack_from("<I", data, 0)[0]
    if magic not in MIX_MAGIC:
        print("not a MIX:", hex(magic), file=sys.stderr)
        return []
    count = struct.unpack_from("<H", data, 4)[0]
    # Format 0: header 8 bytes, then count*(4+4), then data
    entries = []
    off = 8
    for i in range(count):
        h, o = struct.unpack_from("<II", data, off + i * 8)
        nxt = struct.unpack_from("<I", data, off + (i + 1) * 8)[0] if i + 1 < count else len(data)
        entries.append((h, o, nxt - o))
    return data, entries


if __name__ == "__main__":
    mix = sys.argv[1]
    want = sys.argv[2].lower()
    out = sys.argv[3]
    db = sys.argv[4] if len(sys.argv) > 4 else None
    names = read_names(db)
    data, entries = parse_mix(mix)
    for h, o, size in entries:
        name = names.get(h, "")
        if name.lower() == want or f"{h:08x}" == want:
            body = data[o:o + size]
            open(out, "wb").write(body)
            print(f"wrote {out}: {size} bytes, hash={h:#010x}, name={name}")
            break
    else:
        print(f"not found {want}; {len(entries)} entries", file=sys.stderr)
