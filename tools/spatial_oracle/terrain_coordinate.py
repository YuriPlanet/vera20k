"""TerrainType's native placement coordinate and retained Object GetCoords.

The map table and level/slope prestate are supplied. Original type adjustment,
Map ground getter, slope math, Object coordinate write and getter execute.
"""
import struct
from pathlib import Path
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.bridge_damage_admission import (
    MEM, CELL, base, call, words,
)
from tools.native_slope import slope_matrices


def execute(case):
    u = base(case)
    u.mem_write(0x89E7C0, words(104))
    for index, matrix in enumerate(slope_matrices()):
        u.mem_write(0xB45188 + 48 * index, struct.pack('<12I', *matrix))
    source, adjusted, obj, observed = (MEM + n for n in (0x18000, 0x18020, 0x18100, 0x18300))
    u.mem_write(source, words(2688, 5248, case['input_z']))
    call(u, 0x71E0D0, args=(adjusted, source))
    placement = list(struct.unpack('<3i', u.mem_read(adjusted, 12)))
    call(u, 0x5F6940, obj, (adjusted,))
    # A later map mutation must not change the retained object coordinate.
    u.mem_write(CELL + 0x11B, bytes((7, 0)))
    call(u, 0x5F65A0, obj, (observed,))
    return dict(input=case, placement=placement,
                after_ground_change=list(struct.unpack('<3i', u.mem_read(observed, 12))))


def generate():
    cases = [dict(level=level, slope=slope, flags=flags, input_z=0)
             for level in (-128, -1, 0, 2, 127)
             for slope in (0, 1, 2, 15)
             for flags in (0, 0x100)]
    cases.append(dict(level=2, slope=0, flags=0, input_z=999))
    return dict(cases=[execute(case) for case in cases])


def metadata():
    return provenance(
        scope='Original TerrainType71E0D0 placement adjustment, Map578080 ground getter, Object5F6940 raw write and Object5F65A0 retained GetCoords; 41 signed-level/slope/bridge-flag cases. Not complete Terrain construction, placement admission, observers or rendering.',
        assumptions=[
            'Terrain constructor71BC4A..71BC76 supplies signed cell-center XY and inputZ0. Supplied cell10,20 has center2688,5248; extra inputZ999 row characterizes the type callback beyond map constructor inputs.',
            'Original startup slope matrices initialized by native_slope.slope_matrices and installed atB45188. LevelHeight104 supplied at89E7C0; native x87 control0x0E7F.',
            'Terrain VT7F522C+48 is5F65A0,+1B4 is5F6940; TerrainType VT7F5458+6C is71E0D0. Calls execute these original leaves directly. The source cell level then changes to7/flat before the GetCoords read.',
        ],
        substitutions=['None; map table and Object storage are supplied fixture state. No code patches or gameplay hooks.'],
        entry_points={'terrain_type_coordinate':0x71E0D0,'ground_height':0x578080,
                      'set_raw_coords':0x5F6940,'get_coords':0x5F65A0},
    )


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
