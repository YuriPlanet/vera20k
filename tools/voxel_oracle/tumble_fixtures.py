"""Original VoxelAnimClass::DrawIt voxel calls at bounce drawing matrices.

python -m tools.voxel_oracle.tumble_fixtures --write / --check
DrawIt (0x00749B70) draws its piece's VXL without an HVA at the bounce body's
drawing matrix (BounceClass::GetDrawingMatrix 0x004399E0): the ground shadow
through 753F90 with the viewer identity (0x00749CBE..0x00749CE8), then the body
lit by 753C80 and rasterized through 7540F0 (0x00749D97..0x00749DD4). The art is
shadow_fixtures' generated single section, not commercial art. The matrices are
the native drawing matrices bounce_tumble.json recorded along its flights.

Row schema: `cases[i]` = {flight, tick, shadow: render_shadow_native output,
body: render_native output}.

Rust consumers: src/render/vxl_native.rs and src/render/vxl_shadow_tests.rs.
"""
from pathlib import Path
import json
import struct

from tools.native_oracle import finish_vectors, provenance
from tools.voxel_oracle.raster import render_native
from tools.voxel_oracle.shadow_fixtures import single_section
from tools.voxel_oracle.shadow_raster import render_shadow_native

FLIGHTS = Path(__file__).parents[1] / 'spatial_oracle' / 'bounce_tumble.json'


def drawing_matrices():
    """Two ticks of each of the first twelve flights: a third of the way in and
    the last, so rest poses and mid-tumble poses both appear."""
    flights = json.loads(FLIGHTS.read_text())['flight']
    for index, flight in enumerate(flights[:12]):
        ticks = flight['ticks']
        for tick in (len(ticks) // 3, len(ticks) - 1):
            yield index, tick, struct.pack('<12I', *ticks[tick][9:21])


def generate():
    vxl, _hva, vpl = single_section()
    cases = []
    for flight, tick, matrix in drawing_matrices():
        cases.append({'flight': flight, 'tick': tick,
                      'shadow': render_shadow_native(vxl, None, vpl, matrix),
                      'body': render_native(vxl, None, vpl, matrix, diffuse=True)})
    assert any(p['entry'] == 0x7dfae0 for case in cases for p in case['body']['raster_params']), \
        'fixture must exercise native backward encoded traversal'
    return {'vxl': vxl.hex(), 'vpl': vpl.hex(), 'cases': cases}


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope=('24 VoxelAnimClass::DrawIt voxel draws of generated single-section art: the '
               'no-HVA shadow quad, crop and stencil, and the 753C80-lit body raster and crop, '
               'at 24 native bounce drawing matrices from bounce_tumble.json'),
        assumptions=['original x87 0E7F and Init_Game startup camera, light and shadow '
                     'translation (0x0052BDDD..0x0052BE0A)',
                     'section zero is drawn; DrawIt passes VoxelIndex, zero for every stock type',
                     'no shadow mask: DrawIt passes a null surface to 753F90'],
        substitutions=['immutable file vtable IO', 'operator new mapped heap allocation'],
        entry_points={'diffuse_lighting': 0x753c80, 'shadow_quad': 0x753f90,
                      'section_draw': 0x7540f0, 'crop': 0x754510}))
