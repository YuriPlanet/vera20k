"""Original56D460/56D5A0 retained-cache selectors and adoption stores.

Fallback cases stop at56C510 entry; bridge_connectivity separately executes that
rebuild. Run python -m tools.spatial_oracle.base_zone_repair --check (or --write).
"""
import struct
from pathlib import Path

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle.bridge_repair_zones import base_fixture, BASE, RAW, QUERY
from tools.spatial_oracle.bridge_records import MAP, CELLS, DUMMY
from tools.spatial_oracle.map_queries import dwords, packed

NEIGHBORS = ((0, -1), (1, -1), (1, 0), (1, 1),
             (0, 1), (-1, 1), (-1, 0), (-1, -1))


def execute(case):
    uc, sp, width, side, _ = base_fixture(case)
    coord = case['query']
    index = min(max(coord[1] * side + coord[0], 0), side * side - 1)
    nodes = bytearray(b'\x07\x00\x00\x00' * (side * side))
    struct.pack_into('<BBH', nodes, index * 4, *case['target'])
    for (dx, dy), neighbor in zip(NEIGHBORS, case['neighbors']):
        neighbor_index = index + dy * side + dx
        assert 0 <= neighbor_index < side * side
        struct.pack_into('<BBH', nodes, neighbor_index * 4, *neighbor)
    uc.mem_write(BASE, bytes(nodes))
    raw = struct.pack('<' + 'H' * len(case['row0']), *case['row0'])
    uc.mem_write(RAW, raw)
    uc.mem_write(QUERY, packed(*coord))
    # Current Cell height deliberately disagrees with the retained height.
    x, y = index % side, index // side
    if x < width and y < width:
        uc.mem_write(CELLS + (y * width + x) * 0x200 + 0x11B,
                     bytes([case.get('live_height', 20)]))
    calls, dummy_writes = [], []

    def observe(_uc, address, _size, _data):
        if address in (0x578460, 0x47D2B0, 0x56C510):
            calls.append(hex(address))

    def write(_uc, _access, address, size, value, _data):
        if address == DUMMY + 0x24:
            dummy_writes.append([size, value])

    uc.hook_add(UC_HOOK_CODE, observe)
    uc.hook_add(UC_HOOK_MEM_WRITE, write)
    uc.mem_write(sp, dwords(RET_MAGIC, QUERY))
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_ECX, MAP)
    stop = run_checked(uc, 0x56D460 if case['kind'] == 'assign' else 0x56D5A0,
                       (RET_MAGIC, 0x56C510), count=50000)
    after = bytes(uc.mem_read(BASE, len(nodes)))
    assert after[0::4] == nodes[0::4] and after[1::4] == nodes[1::4]
    assert bytes(uc.mem_read(RAW, len(raw))) == raw
    assert calls == ([] if stop == RET_MAGIC else ['0x56c510'])
    assert not dummy_writes
    changed = [i for i in range(side * side) if after[i*4:i*4+4] != nodes[i*4:i*4+4]]
    assert changed in ([], [index])
    return dict(input=case, canonical=[x, y], fallback=stop == 0x56C510,
                target_after=list(struct.unpack_from('<BBH', after, index * 4)),
                changed=changed, calls=calls, dummy_writes=dummy_writes)


def inputs():
    outside = [7, 0, 0]
    def neighbors(entries):
        return [[cls, 255 if i % 2 else 0, cluster]
                for i, (cls, cluster) in enumerate(entries)] + [outside] * (8-len(entries))
    patterns = {
        'three_transitions': neighbors([(0, 1), (2, 2), (0, 1)]),
        'four_transitions': neighbors([(0, 1), (2, 2), (0, 1), (2, 2)]),
        'same_labels_distinct_clusters': neighbors([(0, 1), (0, 2), (0, 3), (0, 4)]),
        'sentinel_does_not_reset': neighbors([(0, 1), (7, 0), (0, 1), (2, 2), (0, 1)]),
        'no_closing_transition': neighbors([(0, 1), (0, 2)] + [(0, 3)] * 6),
        'first_candidate_zero': neighbors([(0, 0), (0, 1)]),
        'retained_row0_zero_override': neighbors([(0, 1), (0, 2), (0, 1), (0, 2)]),
        'no_candidate': neighbors([(2, 1), (2, 2)]),
        'sentinel_target': neighbors([(0, 1)]),
        'non_ground_target': neighbors([(2, 2), (0, 1)]),
    }
    for kind in ('assign', 'merge'):
        for name, adjacent in patterns.items():
            row0 = [65535, 2, 3, 4, 5, 6]
            if name == 'same_labels_distinct_clusters':
                row0[1:5] = [2] * 4
            if name == 'retained_row0_zero_override':
                row0[0] = 2  # Supplied retained row; normal56C510 writesFFFF here.
            yield dict(name=f'{kind}_{name}', kind=kind, size=[8, 8], query=[5, 5],
                       target=[7 if name == 'sentinel_target' else
                               2 if name == 'non_ground_target' else 0, 4, 5],
                       neighbors=adjacent, row0=row0)
        for query in ([22, 4], [-12, 6]):
            yield dict(name=f'{kind}_alias_{query}', kind=kind, size=[8, 8], query=query,
                       target=[0, 4, 5], neighbors=neighbors([(0, 1)]),
                       row0=[65535, 2, 3, 4, 5, 6])
        for query in ([-32768, -32768], [32767, 32767]):
            yield dict(name=f'{kind}_clamped_sentinel_{query}', kind=kind, size=[8, 8],
                       query=query, target=[7, 0, 0], neighbors=[], row0=[65535])


def cases():
    return dict(cases=[execute(case) for case in inputs()])


if __name__ == '__main__':
    finish_vectors(cases, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original56D460/56D5A0 retained-cache selection and adoption; stop before56C510 fallback',
        assumptions=[
            'Supplied Size8,8, retained class/height/base-ID records and row0; current Cell height20 differs from retained4',
            'Twenty-eight branch witnesses, including ordered transitions, zero candidate, one-time packed clamp and raw neighbor offsets',
            'One case per helper intentionally supplies row0[0]=2; native56C510 normally initializes this sentinel toFFFF',
            'Fallback classification stops before original56C510; saved bridge_connectivity corpus executes its separate rebuild',
            'No out-of-allocation neighbor reads or malformed Size claims; extreme packed coordinates hit retained class7 and return before neighbors',
        ], substitutions=[], entry_points={'assign':0x56D460, 'merge':0x56D5A0,
                                            'fallback_boundary':0x56C510}))
