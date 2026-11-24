"""Original56C510 over retained class/height bytes and bridge records.

The input edge buckets are empty; successful heap allocation/free are supplied.
Original flood fill, bridge pair insertion and all13 movement-row producers run.
"""
import struct
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP, UC_X86_REG_EIP
from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.bridge_records import MAP, CELLS, RECORDS
from tools.spatial_oracle.bridge_repair_zones import base_fixture, BASE, PLANE, GRAPHS, EDGES, u32

BUCKETS, ROOT, DATA, HEAP = 0xB80000, 0xB8F000, 0xCFE000, 0x20000000


def execute(case):
    uc, sp, width, side, _ = base_fixture(dict(a=[5,5], b=[10,5]))
    uc.mem_map(HEAP, 0x800000)
    classes = case['classes']; levels = case['levels']
    cell_levels = case.get('cell_levels', levels)
    assert len(classes) == len(levels) == width * width
    nodes = bytearray(b'\x07\x00\xcd\xab' * (side * side))
    for y in range(width):
        for x in range(width):
            i = y * width + x
            nodes[(y * side + x) * 4] = classes[i]
            nodes[(y * side + x) * 4 + 1] = levels[i]
            uc.mem_write(PLANE + (y * side + x) * 10 + 8, bytes([levels[i]]))
            uc.mem_write(CELLS + i * 0x200 + 0x11B, bytes([cell_levels[i]]))
    uc.mem_write(BASE, bytes(nodes))
    uc.mem_write(MAP + 0x18, bytes(13 * 4))
    # An already empty input bucket array: count0 skips redundant clearing.
    # The producer still addresses all256 preallocated buckets normally.
    uc.mem_write(MAP + 0x14, dwords(ROOT)); uc.mem_write(ROOT, dwords(BUCKETS, 256, 0))
    for bucket in range(256):
        uc.mem_write(BUCKETS + bucket * 24, dwords(0, DATA + bucket * 256, 32, 0, 0, 16))
    uc.mem_write(MAP + 0x54, dwords(RECORDS, 64, 0, len(case['records'])))
    for i, (a, b, active, kind) in enumerate(case['records']):
        uc.mem_write(RECORDS + i * 16, packed(*a) + packed(*b) + dwords(active, kind))
    before_plane = bytes(uc.mem_read(PLANE, side * side * 10))
    before_graphs = bytes(uc.mem_read(GRAPHS, 3 * 0x1000))
    before_edges = bytes(uc.mem_read(EDGES, 3 * 32 * 32 * 8))
    cursor = HEAP
    calls = set()
    def observe(_uc, address, _size, _data):
        nonlocal cursor
        if address in (0x56CB90, 0x56C6CB): calls.add(address)
        if address not in (0x7C8E17, 0x7C8B3D): return
        esp = uc.reg_read(UC_X86_REG_ESP)
        if address == 0x7C8E17:
            size = u32(uc, esp + 4)
            allocation = cursor; cursor += (max(size, 1) + 15) & ~15
            assert cursor < HEAP + 0x800000
            uc.mem_write(allocation, bytes(max(size, 1)))
            uc.reg_write(UC_X86_REG_EAX, allocation)
        uc.reg_write(UC_X86_REG_EIP, u32(uc, esp)); uc.reg_write(UC_X86_REG_ESP, esp + 4)
    uc.hook_add(UC_HOOK_CODE, observe)
    uc.mem_write(sp, dwords(RET_MAGIC)); uc.reg_write(UC_X86_REG_ESP, sp); uc.reg_write(UC_X86_REG_ECX, MAP)
    run_checked(uc, 0x56C510, RET_MAGIC, count=3000000,
                required_addresses=[0x56CB90, 0x56C6CB])
    after = bytes(uc.mem_read(BASE, side * side * 4))
    assert after[0::4] == nodes[0::4] and after[1::4] == nodes[1::4]
    assert bytes(uc.mem_read(PLANE, side * side * 10)) == before_plane
    assert bytes(uc.mem_read(GRAPHS, 3 * 0x1000)) == before_graphs
    assert bytes(uc.mem_read(EDGES, 3 * 32 * 32 * 8)) == before_edges
    count = u32(uc, MAP + 0x4C)
    rows = [list(struct.unpack('<' + 'H' * count, uc.mem_read(u32(uc, MAP + 0x18 + row * 4), count * 2)))
            for row in range(13)]
    ids = [struct.unpack_from('<H', after, (y * side + x) * 4 + 2)[0]
           for y in range(width) for x in range(width)]
    return dict(input=case, base_ids=ids, raw_rows=rows, zone_count=count - 1,
                returned=uc.reg_read(UC_X86_REG_EAX), reached=sorted(hex(a) for a in calls))


def cases():
    result = []
    for mode in ('clear', 'stripes', 'islands', 'heights'):
        classes = [0 if mode in ('clear','heights') else x % 8 if mode == 'stripes' else
                   (x // 3 + y // 3) % 8 for y in range(16) for x in range(16)]
        # Native flood relies on class7 outside the allocated Size diamond,
        # including the leading border; these are initialized map node bytes.
        classes = [value if 8 < x + y <= 24 and abs(x - y) < 8 else 7
                   for (y, x), value in zip(((y,x) for y in range(16) for x in range(16)), classes)]
        levels = [((x // 4 + y // 4) % 2) * 4 if mode == 'heights' else 0 for y in range(16) for x in range(16)]
        for active in (False, True):
            result.append(execute(dict(name=f'{mode}_{active}', classes=classes, levels=levels,
                records=[[[5,5],[10,5],active,0],[[4,6],[11,6],active,1]])))
        if mode == 'heights':
            result.append(execute(dict(name='retained_heights_differ_from_cells',
                classes=classes, levels=levels, cell_levels=[0] * 256,
                records=[[[5,5],[10,5],False,0],[[4,6],[11,6],False,1]])))
    return dict(cases=result)


if __name__ == '__main__':
    finish_vectors(cases, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original56C510 retained class/unsigned height bytes, base IDs and13 raw movement rows with active bridge records',
        assumptions=['Supplied16x16 cells, Size8,8, native17x17 node plane with class7 outside the Size diamond and in padding',
                     'Input adjacency buckets empty, root count0 skips redundant virtual clearing; all256 bucket storage/capacities supplied',
                     'No memory exhaustion; input raw rows empty; native hierarchy plane remains unchanged',
                     'Both node planes receive retained heights; one case supplies different current Cell heights',
                     'All actual flood, bridge insertion and movement-row instructions execute; not a full map loading/gameplay proof'],
        substitutions=['7C8E17 successful bounded allocation and7C8B3D no-op free; native vector growth/copy executes'],
        entry_points={'connectivity':0x56C510,'flood':0x56CB90,'bridge_pairs':0x56C6CB}))
