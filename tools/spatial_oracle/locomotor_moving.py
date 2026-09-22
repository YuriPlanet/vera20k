"""Original Drive/Ship/Walk IsMoving and Walk IsMovingNow, supplied state.

Both slots use actual retail vtables. No executable bytes or callees replaced.
Destinations/heads are independent of the Foot order; lifecycle is not emulated.
"""
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_EAX
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.locomotor_at_coord import OriginalQuery, fixture, LOCO, FOOT
from tools.spatial_oracle.map_queries import dwords

ENTRIES = {'drive': 0x4AFB80, 'ship': 0x69F290, 'walk': 0x75AB30}


def inputs():
    zero = [0, 0, 0]
    for family in ('drive', 'ship'):
        for current in ([2944, 2688, 0], [0, 0, 17]):
            for destination in (zero, [0, 0, 1], [3200, 2688, 0]):
                for head in (zero, current, [*current[:2], current[2]+104],
                             [current[0]+1, current[1], 0], [current[0], current[1]-1, 0]):
                    yield dict(family=family, current=current, destination=destination, head=head)
    for moving in (False, True):
        for speed in (-1, 0, 1/65536, 1):
            for head in (zero, [2944, 2688, 0], [0, 0, 1]):
                yield dict(family='walk', current=[2944, 2688, 0],
                           moving=moving, speed=speed, head=head)


def query(case):
    runner = OriginalQuery(fixture(case['family'], 'moving_query',
                                   current=case['current'], head=case['head']))
    u = runner.uc
    if case['family'] == 'walk':
        u.mem_write(LOCO+0x30, bytes([case['moving']]))
        u.mem_write(FOOT+0x578, struct.pack('<d', case['speed']))
    else:
        u.mem_write(LOCO+0x30, dwords(*case['destination']))
    before = bytes(u.mem_read(LOCO, 0x100)), bytes(u.mem_read(FOOT, 0x700))
    entry = runner.read32(runner.read32(LOCO)+0x10)
    assert entry == ENTRIES[case['family']]
    runner.call(entry, [LOCO], [entry])
    output = dict(moving=bool(u.reg_read(UC_X86_REG_EAX) & 255))
    if case['family'] == 'walk':
        now = runner.read32(runner.read32(LOCO)+0x80)
        assert now == 0x75AB40
        runner.call(now, [LOCO], [entry, now])
        output['moving_now'] = bool(u.reg_read(UC_X86_REG_EAX) & 255)
    assert before == (bytes(u.mem_read(LOCO, 0x100)), bytes(u.mem_read(FOOT, 0x700)))
    return dict(input=case, **output)


def generate():
    return [query(case) for case in inputs()]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original Drive/Ship/Walk IsMoving and Walk IsMovingNow with supplied retained state',
        assumptions=['Original interface vtables and owner pointer, explicit NullCoord=(0,0,0).',
                     'Supplied destination/head/moving byte and finite Q16-exact Foot+578 fraction; no lifecycle, movement Process or scenario load.',
                     'All queries must preserve locomotor and owner memory; head Z-only differences and partially zero XYZ are retained.'],
        substitutions=[], entry_points={**ENTRIES, 'walk_now': 0x75AB40}))
