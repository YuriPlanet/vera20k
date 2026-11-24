"""Original repair-zone activation and direct ordered hierarchy edges.

Run python -m tools.spatial_oracle.bridge_repair_zones --check (or --write).
The direct-edge and matching-record validation cases execute all reached callees.
"""
import struct
from pathlib import Path

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EIP
from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.bridge_records import MAP, TABLE, CELLS, RECORDS, DUMMY
from tools.spatial_oracle.tube_hierarchy import initialized_process_fixture

PLANE, GRAPHS, EDGES = 0xBA0000, 0xBB0000, 0xBC0000
QUERY, BASE, RAW = 0xBDA000, 0xBDB000, 0xBDD000
COUNT, CAPACITY = 32, 32


def u32(uc, address):
    return struct.unpack('<I', uc.mem_read(address, 4))[0]


def base_fixture(case):
    uc, sp = initialized_process_fixture()
    w, h = case.get('size', [8, 8]); width = w + h; side = width + 1
    uc.mem_write(MAP + 0xF4, dwords(w, h, *case.get('bounds', [0, 0, w, h])))
    uc.mem_write(MAP + 0x13C, dwords(TABLE, 0x40000))
    uc.mem_write(TABLE, bytes(0x40000 * 4))
    uc.mem_write(0xAA0E28, dwords(100)); uc.mem_write(0xABAD1C, dwords(200))
    uc.mem_write(DUMMY, bytes(0x200)); uc.mem_write(DUMMY + 0x38, dwords(65535))
    uc.mem_write(DUMMY + 0x24, packed(1234, -2345))
    for y in range(width):
        for x in range(width):
            ptr = CELLS + (y * width + x) * 0x200
            uc.mem_write(ptr, bytes(0x200)); uc.mem_write(ptr + 0x24, packed(x, y))
            uc.mem_write(TABLE + (y * 512 + x) * 4, dwords(ptr))
    a = case.get('a', [5, 5]); linear = a[1] * 512 + a[0]
    if 0 <= linear < 0x40000:
        ptr = u32(uc, TABLE + linear * 4)
        if not ptr:
            ptr = CELLS + width * width * 0x200
            uc.mem_write(ptr, bytes(0x200)); uc.mem_write(ptr + 0x24, packed(*a))
            uc.mem_write(TABLE + linear * 4, dwords(ptr))
        uc.mem_write(ptr + 0x38, dwords(case.get('tile', 106)))
    uc.mem_write(MAP + 0x68, dwords(BASE, side * side, PLANE))
    uc.mem_write(BASE, b'\x00\x00\x01\x00' * (side * side))
    uc.mem_write(MAP + 0x18, dwords(RAW))
    uc.mem_write(RAW, struct.pack('<HHH', 0, *case.get('raw', [2, 3])))
    b = case.get('b', [10, 5])
    index = lambda p: min(max(p[1] * side + p[0], 0), side * side - 1)
    uc.mem_write(BASE + index(b) * 4 + 2, struct.pack('<H', 2))
    mode = case.get('mode', 'varied')
    zones = [[0 if mode == 'zero' else 1 if mode == 'equal' else
              1 + (x + y * 17 + level * 3) % 31
              for y in range(width) for x in range(width)] for level in range(3)]
    plane = bytearray(side * side * 10)
    for level in range(3):
        for y in range(width):
            for x in range(width):
                struct.pack_into('<H', plane, (y * side + x) * 10 + level * 2,
                                 zones[level][y * width + x])
        uc.mem_write(MAP + 0x90 + level * 24, dwords(GRAPHS + level * 0x1000))
        for zone in range(COUNT):
            ptr = EDGES + (level * COUNT + zone) * CAPACITY * 8
            uc.mem_write(GRAPHS + level * 0x1000 + zone * 36,
                         dwords(0, ptr, CAPACITY, 0, 1, 16, 0, 0, 0))
            uc.mem_write(ptr, dwords(zone, 1))  # Retained edge with nonzero flag.
    uc.mem_write(PLANE, bytes(plane))
    return uc, sp, width, side, zones


def execute_edges(case):
    uc, sp, width, side, zones = base_fixture(case)
    records = case.get('records', [[case['a'], case['b'], False, 0]])
    def write_records(rows):
        for i, (a, b, active, kind) in enumerate(rows):
            uc.mem_write(RECORDS + i * 16, packed(*a) + packed(*b) + dwords(active, kind))
        uc.mem_write(MAP + 0x54, dwords(RECORDS, 64, 0, len(rows)))
    write_records(records)
    uc.mem_write(QUERY, packed(*case.get('query', case['a'])))
    transcript, writes = [], []
    def observe(_uc, address, _size, _data):
        if address in (0x5851B0, 0x56D100, 0x56D230, 0x56D6E0):
            transcript.append(hex(address))
        if address == 0x56D6E0 and 'recomputed' in case:
            write_records(case['recomputed'])
            esp = uc.reg_read(UC_X86_REG_ESP)
            uc.reg_write(UC_X86_REG_EIP, u32(uc, esp)); uc.reg_write(UC_X86_REG_ESP, esp + 4)
    def edge_write(_uc, _access, address, size, value, _data):
        if EDGES <= address < EDGES + 3 * COUNT * CAPACITY * 8 and (address - EDGES) % 8 == 0:
            assert size == 4
            slot, ordinal = divmod((address - EDGES) // 8, CAPACITY)
            level, zone = divmod(slot, COUNT)
            writes.append([level, zone, ordinal, value])
    uc.hook_add(UC_HOOK_CODE, observe); uc.hook_add(UC_HOOK_MEM_WRITE, edge_write)
    validate = case.get('validate', False)
    uc.mem_write(sp, dwords(RET_MAGIC, QUERY if validate else RECORDS))
    uc.reg_write(UC_X86_REG_ESP, sp); uc.reg_write(UC_X86_REG_ECX, MAP)
    run_checked(uc, 0x56DB70 if validate else 0x5851B0, RET_MAGIC, count=150000)
    result = bool(uc.reg_read(UC_X86_REG_EAX) & 255) if validate else None
    graph = []
    for level in range(3):
        nodes = []
        for zone in range(COUNT):
            count = u32(uc, GRAPHS + level * 0x1000 + zone * 36 + 16)
            assert count <= CAPACITY
            ptr = EDGES + (level * COUNT + zone) * CAPACITY * 8
            nodes.append([[u32(uc, ptr + n * 8), u32(uc, ptr + n * 8 + 4) & 255] for n in range(count)])
        graph.append(nodes)
    return dict(input=case, width=width, zones=zones, edges=graph, writes=writes,
                returned=result, calls=transcript,
                active=[bool(uc.mem_read(RECORDS + i * 16 + 8, 1)[0]) for i in range(u32(uc, MAP + 0x60))],
                dummy=list(struct.unpack('<hh', uc.mem_read(DUMMY + 0x24, 4))))


def inputs():
    for base in (100, 200):
        for offset in range(16):
            yield dict(name=f'tile_{base + offset}', a=[5, 5], b=[10, 5], tile=base + offset)
    for mode in ('equal', 'zero'):
        yield dict(name=mode, a=[5, 5], b=[10, 5], mode=mode)
    for a, b in (([0, 0], [3, 3]), ([-1, 1], [18, 0]), ([32767, 1], [32767, 3])):
        yield dict(name=f'boundary_{a}', a=a, b=b, tile=103)
    for raw in ([2, 3], [2, 2], [0, 0], [1, 1], [65535, 65535]):
        yield dict(name=f'validate_raw_{raw}', a=[5, 5], b=[10, 5], validate=True, raw=raw)
    for name, records in (
        ('active', [[[5, 5], [10, 5], True, 0]]),
        ('multiple', [[[5, 5], [10, 5], False, 0], [[5, 5], [10, 5], True, 0], [[5, 5], [10, 5], False, 0]]),
        ('recompute_miss', []),
        ('recompute_kind1', [[[5, 5], [10, 5], False, 1]]),
    ):
        row = dict(name=name, a=[5, 5], b=[10, 5], validate=True, records=records)
        if name.startswith('recompute'):
            row['recomputed'] = [[[5, 5], [10, 5], False, 0]] if name.endswith('miss') else records
        yield row
    yield dict(name='source_fringe', a=[5, 5], b=[10, 5], validate=True, bounds=[2, 5, 4, 1])
    yield dict(name='missing_destination', a=[5, 5], b=[20, 5], validate=True)


def cases():
    return dict(edges=[execute_edges(case) for case in inputs()])


if __name__ == '__main__':
    finish_vectors(cases, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original5851B0 direct hierarchy append and56DB70 matching-record activation',
        assumptions=[
            'Pinned original retail image; original direction initializers and captured startup constants',
            'Supplied concretebase100/woodbase200, all32 raw tile offsets; adequately sized edge vectors retain seed flag1',
            'Graph IDs0..31; native signed-negative graph pointers and allocation failure are outside this fixture',
            'Endpoint A has a high tile; nonhigh endpoint stack reads are not claimed',
            'Supplied16x16 cells, Size8,8 and movement-row labels; not a native map-loader/fullgame proof',
        ], substitutions=[
            'Only recompute_miss and recompute_kind1 substitute56D6E0 with explicit supplied records; record_scan corpus covers its separate original body',
        ], entry_points={'add_edges':0x5851B0, 'validate':0x56DB70, 'reach':0x56D100,
                         'get_zone':0x56D230, 'playfield':0x578460}))
