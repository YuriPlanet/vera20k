"""Original height arithmetic of the group spread `0x0064CDA0`.

Two slices of the original function run on supplied Cells, with no call,
instruction or return value substituted:
- `0x0064D296..0x0064D2E2`: both target Cell lookups (`0x005657A0`) and the
  target height EBX, the Cell's signed level plus 4 on flag `0x100`;
- `0x0064D53D` up to its first exit, with ESI the candidate Cell and EBX that
  height: the reservation bit `0x8000` (`0x0064D5A7`), the height band that ends
  the member's probes (`0x0064D650`), or the Can_Enter_Cell code test
  (`0x0064D598`).
The zone compare before the second slice and the code test after it are not
executed here.
"""
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_EBX, UC_X86_REG_ESI, UC_X86_REG_ESP
from tools.native_oracle import run_checked, finish_vectors, provenance
from tools.spatial_oracle.aircraft_fire_location import Fixture, cell, i32
from tools.spatial_oracle.map_queries import dwords

TARGET_XY = (64, 64)
CANDIDATE_XY = (65, 64)
OUTCOMES = {0x0064D598: 'code_test', 0x0064D5A7: 'reserved', 0x0064D650: 'end_probes'}


def execute(case):
    f = Fixture({})
    u = f.u
    target, candidate = cell(*TARGET_XY), cell(*CANDIDATE_XY)
    u.mem_write(target + 0x11B, struct.pack('<b', case['target_level']))
    u.mem_write(target + 0x140, dwords(case['target_flags']))
    u.mem_write(candidate + 0x11B, struct.pack('<b', case['candidate_level']))
    u.mem_write(candidate + 0x140, dwords(case['candidate_flags']))
    u.mem_write(f.sp + 0x14, struct.pack('<hh', *TARGET_XY))
    u.reg_write(UC_X86_REG_ESP, f.sp)
    run_checked(u, 0x0064D296, 0x0064D2E2, count=100_000)
    assert u.reg_read(UC_X86_REG_ESP) == f.sp
    height = i32(u.reg_read(UC_X86_REG_EBX))
    u.reg_write(UC_X86_REG_ESI, candidate)
    end = run_checked(u, 0x0064D53D, tuple(OUTCOMES), count=1_000)
    return dict(input=case, target_height=height, outcome=OUTCOMES[end])


def generate():
    cases = []
    for target_level in (0, 1, 4, 9, -3):
        for target_flags in (0, 0x100):
            for delta in range(-4, 9):
                for candidate_flags in (0, 0x100, 0x8000, 0x8100):
                    cases.append(dict(target_level=target_level, target_flags=target_flags,
                                      candidate_level=target_level + delta,
                                      candidate_flags=candidate_flags))
    # Only bit 8 and bit 15 are read: every other flag bit is set here.
    cases += [dict(target_level=2, target_flags=0x7EFF, candidate_level=level,
                   candidate_flags=0xFFFF7EFF) for level in (0, 2, 4, 5)]
    return [execute(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'target_height': 0x0064D296, 'candidate_gates': 0x0064D53D},
        assumptions=['Original MapClass lookups on the shared 128x128 fixture map; supplied Cell+0x11B levels and Cell+0x140 flags.',
                     'Slices start mid-function with only ESP, EBX and ESI supplied; registers the slices do not read are left as the fixture sets them.'],
        substitutions=[],
        scope='524 target/candidate Cell pairs: signed levels, flag 0x100 on either Cell, reservation 0x8000, and all other flag bits set. Excludes the zone compare, Can_Enter_Cell and the distributor.'),
        source_paths={'oracle': Path(__file__)})
