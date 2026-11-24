"""Original CellIterator and ComputeBridgeZones record-production comparison.

Run python -m tools.spatial_oracle.bridge_records --check (or --write).
The vector reset executes natively; storage is then supplied before the scan.
"""

import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EBP, UC_X86_REG_ESP

from tools.native_oracle import (
    RET_MAGIC, STACK_BASE, STACK_SIZE, finish_vectors, load_image, provenance,
    run_checked,
)
from tools.spatial_oracle.map_queries import dwords, packed

MAP, TABLE, CELLS = 0x0087F7E8, 0x00C00000, 0x00B00000
TUBES, TUBE_TABLE, RECORDS, DUMMY = 0x00B80000, 0x00B88000, 0x00B90000, 0x00ABDC50
ENTRY, AFTER_CLEAR = 0x0056D6E0, 0x0056D6F7
ENTRIES = {"compute_bridge_zones": ENTRY, "iterator_init": 0x00578350,
           "direction_initializer": 0x0049F2F0,
           "iterator_next": 0x00578290, "vector_clear": 0x00588CE0,
           "ordinal": 0x0042B1C0, "is_tube": 0x00484AB0,
           "get_tube": 0x00484F20, "step": 0x00481810}


def compute_on_map(native_map):
    """Execute the existing original record producer on retained native Cells.

    The empty MapClass565090 vector header is an explicit constructor boundary.
    Unlike the scalar comparison below, the full56D6E0 clear/scan/growth executes
    through the bound VM's existing successful allocator; no records are supplied.
    """
    u = native_map.uc
    u.mem_write(MAP + 0x50, dwords(0x007ED4C0, 0, 0, 0, 1, 10))
    native_map.call(ENTRY, this=MAP, count=5000000)
    header = struct.unpack('<6I', u.mem_read(MAP + 0x50, 24))
    pointer, capacity, count = header[1], header[2], header[4]
    assert count <= capacity
    records = []
    for index in range(count):
        raw = bytes(u.mem_read(pointer + index * 16, 16))
        records.append(dict(a=list(struct.unpack('<hh', raw[:4])),
                            b=list(struct.unpack('<hh', raw[4:8])),
                            active=bool(raw[8]), kind=struct.unpack('<I', raw[12:])[0]))
    return dict(entry=hex(ENTRY), count=count, capacity=capacity, records=records)


def fixture(name, *, size=(8, 8), cells=(), tubes=(), holes=()):
    # Cell rows: x,y,tile,subtile,flags,land,tube-index. Explicit rows overlay
    # a fully allocated Size diamond, as produced by a successful Resize.
    return dict(name=name, size=size, cells=cells, tubes=tubes, holes=holes)


def tube_row(x, y, ordinal):
    return (x, y, 0, 0, 0, 10, ordinal)


def cases():
    out = [fixture("empty"), fixture("empty_first_slot", holes=[(1, 8)])]
    # Three adjacent Tunnel cells; automatic shells have the same entry/exit.
    mouths = [tube_row(x, 5, x - 4) for x in range(4, 7)]
    shells = [(x, 5, x, 5, 0) for x in range(4, 7)]
    out.append(fixture("automatic_shells", cells=mouths, tubes=shells))
    for exit_xy in [(8, 5), (2, 5), (5, 5), (4, 6), (6, 4),
                    (-32768, 32767), (32767, -32768), (32767, 32767), (-1, -1)]:
        tubes = shells.copy()
        tubes[1] = (5, 5, *exit_xy, 0)
        out.append(fixture(f"tube_exit_{exit_xy[0]}_{exit_xy[1]}", cells=mouths, tubes=tubes))
    tubes = shells.copy()
    tubes[1] = (5, 5, 8, 5, 3)
    out.append(fixture("tube_nonzero_path", cells=mouths, tubes=tubes))
    out.append(fixture("tube_missing_east", cells=mouths[:2], tubes=tubes))
    out.append(fixture("tube_stale_east_index", cells=[*mouths[:2], tube_row(6, 5, 3)], tubes=tubes))
    out.append(fixture("tube_wrong_land_east", cells=[*mouths[:2], (6, 5, 0, 0, 0, 9, 2)], tubes=tubes))
    vertical = [tube_row(5, y, y - 4) for y in range(4, 7)]
    out.append(fixture("vertical_tube", cells=vertical,
                       tubes=[(5, 4, 5, 4, 0), (5, 5, 5, 8, 0), (5, 6, 5, 6, 0)]))
    # Native table offset 6 has start/end subtile 4 and walks direction 2.
    high = [(4, 7, 106, 4, 0, 0, -1), (5, 7, 0, 0, 0x100, 0, -1),
            (6, 7, 106, 4, 0, 0, -1)]
    out.append(fixture("high_intact", cells=high))
    out.append(fixture("high_gap", cells=[high[0], high[2]]))
    out.append(fixture("mixed_tube_before_high", cells=[*mouths, *high], tubes=tubes))
    out.append(fixture("mixed_hole_truncates", cells=[*mouths, *high], tubes=tubes, holes=[(2, 8)]))
    out.append(fixture("wood_intact", cells=[tuple([*c[:2], 206 if c[2] == 106 else c[2], *c[3:]]) for c in high]))
    out.append(fixture("high_priority_over_tube", cells=[*mouths[:1], (5, 5, 106, 0, 0, 10, 1), mouths[2]], tubes=tubes))
    # A far match at the outer boundary still commits after the next miss.
    out.append(fixture("high_far_on_boundary", size=(4, 4), cells=[
        (4, 4, 106, 4, 0, 0, -1), (5, 4, 0, 0, 0x100, 0, -1),
        (6, 4, 0, 0, 0x100, 0, -1), (7, 4, 106, 4, 0, 0, -1)]))
    out.append(fixture("high_no_far", cells=high[:2]))
    south = [(7, 4, 111, 2, 0, 0, -1), (7, 5, 0, 0, 0x100, 0, -1),
             (7, 6, 111, 2, 0, 0, -1)]
    out.append(fixture("south_high", cells=south))
    out.append(fixture("south_far_on_boundary", size=(4, 4), cells=[
        (4, 4, 111, 2, 0, 0, -1), (4, 5, 0, 0, 0x100, 0, -1),
        (4, 6, 0, 0, 0x100, 0, -1), (4, 7, 111, 2, 0, 0, -1)]))
    late_mouths = [tube_row(x, 9, x - 8) for x in range(8, 11)]
    out.append(fixture("mixed_high_before_tube", cells=[*high, *late_mouths],
                       tubes=[(8, 9, 8, 9, 0), (9, 9, 12, 9, 0), (10, 9, 10, 9, 0)]))
    # Enumerate every table offset, with both a matching and rejected subtile;
    # these are supplied inputs, the expected decisions come only from x86.
    starts = [7, 7, 255, 7, 7, 255, 4, 4, 4, 4, 4, 2, 2, 2, 2, 2]
    for offset in range(16):
        south_axis = offset in [3, 4, 5, 11, 12, 13, 14, 15]
        mid, end = ((5, 6), (5, 7)) if south_axis else ((6, 5), (7, 5))
        for subtile in [starts[offset], 0]:
            out.append(fixture(f"start_table_{offset}_{subtile}", cells=[
                (5, 5, 100 + offset, subtile, 0, 0, -1),
                (*mid, 0, 0, 0x100, 0, -1),
                (*end, 111 if south_axis else 106, 2 if south_axis else 4, 0, 0, -1)]))
        out.append(fixture(f"end_table_{offset}", cells=[
            (5, 5, 111 if south_axis else 106, 2 if south_axis else 4, 0, 0, -1),
            (*mid, 0, 0, 0x100, 0, -1),
            (*end, 100 + offset, 2 if south_axis else 4, 0, 0, -1)]))
    out.append(fixture("high_internal_hole", cells=high, holes=[(5, 7)]))
    for size in [(1, 1), (1, 3), (2, 2), (2, 3), (3, 4), (12, 7)]:
        out.append(fixture(f"iterator_size_{size[0]}_{size[1]}", size=size))
    return out


def execute(case):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(RET_MAGIC, 0x1000)
    sp = STACK_BASE + STACK_SIZE - 0x1000
    uc.mem_write(sp, dwords(RET_MAGIC))
    uc.reg_write(UC_X86_REG_ESP, sp)
    # This CRT initializer spans Ghidra's incorrectly split 49F2F0/49F300
    # functions. The file/BSS table is zero until these original stores run.
    run_checked(uc, 0x0049F2F0, RET_MAGIC, count=100, required_addresses=[0x0049F388])
    w, h = case["size"]
    uc.mem_write(MAP + 0xF4, dwords(w, h))
    uc.mem_write(MAP + 0x13C, dwords(TABLE, 0x40000))
    uc.mem_write(TABLE, bytes(0x40000 * 4))
    supplied = {(c[0], c[1]): c for c in case["cells"]}
    cells = []
    for y in range(32):
        for x in range(32):
            if w < x + y <= w + 2 * h and abs(x - y) < w and (x, y) not in case["holes"]:
                cells.append(supplied.get((x, y), (x, y, 0, 0, 0, 0, -1)))
    for ordinal, (x, y, tile, subtile, flags, land, tube) in enumerate(cells):
        ptr = CELLS + ordinal * 0x200
        uc.mem_write(ptr, bytes(0x200))
        uc.mem_write(ptr + 0x24, packed(x, y))
        uc.mem_write(ptr + 0x38, dwords(tile))
        uc.mem_write(ptr + 0xEC, dwords(land))
        uc.mem_write(ptr + 0x116, struct.pack("<H", tube & 0xFFFF))
        uc.mem_write(ptr + 0x11A, bytes([subtile]))
        uc.mem_write(ptr + 0x140, dwords(flags))
        uc.mem_write(TABLE + (y * 512 + x) * 4, dwords(ptr))
    uc.mem_write(0x00AA0E28, dwords(100))
    uc.mem_write(0x00ABAD1C, dwords(200))
    uc.mem_write(DUMMY, bytes(0x200))
    uc.mem_write(DUMMY + 0x24, packed(1234, -2345))
    uc.mem_write(DUMMY + 0x38, dwords(65535))
    uc.mem_write(DUMMY + 0x116, b"\xff\xff")
    uc.mem_write(0x008B413C, dwords(TUBE_TABLE))
    uc.mem_write(0x008B4148, dwords(len(case["tubes"])))
    for index, (x, y, ex, ey, length) in enumerate(case["tubes"]):
        ptr = TUBES + index * 0x200
        uc.mem_write(TUBE_TABLE + index * 4, dwords(ptr))
        uc.mem_write(ptr + 0x24, packed(x, y) + packed(ex, ey))
        uc.mem_write(ptr + 0x1C0, dwords(length))
    # Real vector type from MapClass constructor 565090: vtable 7ED4C0.
    # No heap-owned storage, so native clear does not call operator delete.
    uc.mem_write(MAP + 0x50, dwords(0x007ED4C0, 0, 0, 0, 7, 10))
    sp = STACK_BASE + STACK_SIZE - 0x1000
    uc.mem_write(sp, dwords(RET_MAGIC))
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_ECX, MAP)
    run_checked(uc, ENTRY, AFTER_CLEAR, count=100, required_addresses=[0x00588CE0])
    if struct.unpack("<I", uc.mem_read(MAP + 0x60, 4))[0] != 0:
        raise RuntimeError("native record reset failed")
    # Supply adequate storage after clear: allocator/growth is outside this
    # comparison. All producer instructions, predicates and writes now run.
    uc.mem_write(MAP + 0x54, dwords(RECORDS, 256))
    uc.mem_write(RECORDS, b"\xcd" * (256 * 16))
    visited = []
    def observe(machine, address, _size, _data):
        if address == 0x0056D73B:
            visited.append(struct.unpack("<hh", machine.mem_read(machine.reg_read(UC_X86_REG_EBP) + 0x24, 4)))
    uc.hook_add(UC_HOOK_CODE, observe)
    run_checked(uc, AFTER_CLEAR, RET_MAGIC, count=200000,
                required_addresses=[0x00578290])
    count = struct.unpack("<I", uc.mem_read(MAP + 0x60, 4))[0]
    if count > 256:
        raise RuntimeError("unexpected record count")
    records = []
    for index in range(count):
        raw = uc.mem_read(RECORDS + index * 16, 16)
        records.append(dict(a=struct.unpack("<hh", raw[:4]), b=struct.unpack("<hh", raw[4:8]),
                            active=bool(raw[8]), kind=struct.unpack("<I", raw[12:])[0]))
    return {**case, "records": records, "visited": visited,
            "dummy_coord": struct.unpack("<hh", uc.mem_read(DUMMY + 0x24, 4))}


if __name__ == "__main__":
    finish_vectors(lambda: {"cases": [execute(case) for case in cases()]},
                   Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope="Bounded original ComputeBridgeZones record production and CellIterator order",
        assumptions=["Successful sparse Size-diamond allocation supplied; hole cases deliberately truncate traversal",
                     "Original CRT InitializeDirectionOffsets49F2F0 executes before neighbor queries",
                     "Cell/tile/tube fixtures supplied; no constructors, INI parsing or terrain resolution executed",
                     "Native vector clear executes; adequate external record storage is installed at 56D6F7",
                     "No executable patches or substituted function returns; vector allocation/failure excluded",
                     "Only record semantic fields are compared; unused stack padding bytes excluded",
                     "Dummy starts with no tile, no tube and zero land/flags; retained coordinate writes observed"],
        substitutions=["Record vector storage and capacity supplied after original clear, before original scan"],
        entry_points=ENTRIES))
