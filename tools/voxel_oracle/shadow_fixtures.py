"""Generated-art shadow fixtures; no commercial art is embedded.

python -m tools.voxel_oracle.shadow_fixtures --write / --check
Original VXL/HVA loader, shadow quad, raster and optional BSurface reader run.
The supplied mask is an explicit input, not a proof of Unit part production.
"""
from pathlib import Path
import struct

from tools.native_oracle import finish_vectors, provenance
from tools.voxel_oracle.raster_fixtures import test_files
from tools.voxel_oracle.lighting import native_camera, native_facing, native_multiply
from tools.voxel_oracle.shadow_raster import render_shadow_native


def single_section():
    vxl, hva, vpl = test_files()
    count, length = struct.unpack_from('<II', vxl, 24)
    body = vxl[802 + count * 28:802 + count * 28 + length]
    tailer = vxl[802 + count * 28 + length:][:92]
    vxl = vxl[:20] + struct.pack('<3I', 1, 1, len(body)) + vxl[32:802] + vxl[802:830] + body + tailer
    hva = hva[:20] + struct.pack('<I', 1) + hva[24:40] + hva[24 + count * 16:][:48]
    return vxl, hva, vpl


def generate():
    vxl, hva, vpl = single_section()
    body = bytes(27 if ((x * 3 + y) % 7 < 3) else 0 for y in range(256) for x in range(256))
    camera = native_camera()
    cases = []
    for step in range(32):
        matrix = native_multiply(camera, native_facing(step))
        for masked in (False, True):
            result = render_shadow_native(vxl, hva, vpl, matrix, body if masked else None)
            cases.append({'step': step, 'masked': masked, **result})
    return {'vxl': vxl.hex(), 'hva': hva.hex(), 'vpl': vpl.hex(), 'body': body.hex(), 'cases': cases}


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='64 original shadow core cases: generated single-section art, all 32 facings, null or supplied indexed body mask',
        assumptions=['flat scale-one viewer, ShadowIndex/frame zero', 'original x87 0E7F and startup camera/light',
                     'supplied body mask is a patterned 256-square input; Unit composition is a separate gate'],
        substitutions=['immutable file vtable IO', 'operator new mapped heap allocation'],
        entry_points={'quad': 0x753f90, 'crop': 0x754510, 'raster': 0x756860, 'surface_ctor': 0x43ad00}))
