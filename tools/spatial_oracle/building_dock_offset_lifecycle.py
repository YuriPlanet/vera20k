"""Original BuildingType docking offset loop across retained vector passes.

The count and allocated vector are supplied at the post-resize boundary. This
executes original sprintf/ReadCoord/sscanf and slot writes, not allocation or
Image selection. Sparse key fixtures use the original cached INI lookup.
"""
from pathlib import Path
import struct
from unicorn.x86_const import UC_X86_REG_EBP, UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, run_checked, finish_vectors, provenance
from tools.spatial_oracle.building_body_rules import Fixture, TYPE, SP, dwords

KEY = SCRATCH + 0x7000
ARRAY = SCRATCH + 0x8000
CAPACITY = 300


def generate():
    results = []
    for passes in [
        [(4, 2, '11,22,33')],
        [(4, 0, '11,22,33'), (4, 2, '-7,-8,-9')],
        [(4, 2, '11,22,33'), (2, 2, '99,98,97'), (4, 1, '5,6,7')],
        [(0, 0, '11,22,33'), (-7, 0, '4,5,6'), (4, 3, '7,8,9')],
        [(300, 299, '11,22,33')],
        [(4, 2, '11,22,33'), (4, 2, ''), (4, 2, None)],
    ]:
        f = Fixture()
        u = f.u
        initial = [[i + 1, -(i + 1), 1000 + i] for i in range(CAPACITY)]
        u.mem_write(ARRAY, b''.join(dwords(*xyz) for xyz in initial))
        f.write(TYPE + 0x1788, ARRAY)
        f.write(TYPE + 0x178C, CAPACITY)
        outputs = []
        for count, key_index, raw in passes:
            u.mem_write(KEY, f'DockingOffset{key_index}\0'.encode('ascii'))
            f.ini(KEY, raw)
            f.write(TYPE + 0x1780, count)
            u.reg_write(UC_X86_REG_EBP, TYPE)
            u.reg_write(UC_X86_REG_ESP, SP)
            run_checked(u, 0x46499D, 0x464A47, count=3000000,
                        required_addresses=[0x46499D])
            assert u.reg_read(UC_X86_REG_ESP) == SP
            slots = [list(struct.unpack('<iii', u.mem_read(ARRAY + 12*i, 12)))
                     for i in range(CAPACITY)]
            outputs.append(dict(count=count, key_index=key_index, raw=raw, slots=slots))
        results.append(dict(initial=initial, passes=outputs))
    return results


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='BuildingType46499D..464A47 offset loop over a supplied retained 300-slot allocation, across sequential count/key passes. No allocation, Image selection or complete Rules layering claim.',
        assumptions=['Supplied post-resize count/vector; native sprintf, cached INI lookup, ReadCoord and slot writes execute.',
                     'Single-key ART fixture per pass; omitted keys use original per-slot defaults.',
                     'Valid triples and empty/absent values only; malformed stack-dependent partial scans are covered separately.'],
        substitutions=[], entry_points={'offset_loop':0x46499D,'read_coord':0x529CA0}))
