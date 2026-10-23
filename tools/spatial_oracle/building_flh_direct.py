"""Original Building/GetFLH identity-matrix arm, selected coordinate controls.

Uses the existing supplied Building fixture and original getters, matrix
kernels, trigonometric table and ftol. No coordinate answer is supplied.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from tools.native_oracle import finish_vectors, provenance, image_bytes, file_span
from tools.spatial_oracle import building_prism as prism
from tools.spatial_oracle.building_voxel_flh import (
    GET_FLH, BUILDING_GET_COORDS,
    FACING_RATE_CTOR, FACING_SET_CURRENT, COORD, OBJECT_COORDINATE, HEADING,
)
from tools.spatial_oracle.map_queries import dwords


def generate():
    # GetFLH needs the Building fixture, not LaserScene's unrelated draw and
    # allocation setup or its historical whole-image hash pin.
    scene = prism.Scene(dict(name='direct_matrix', native_flh=True))
    u, actor = scene.u, scene.address[scene.first]
    _, text = file_span(image_bytes(), prism.TEXT_BEGIN, prism.TEXT_SIZE)
    if bytes(u.mem_read(prism.TEXT_BEGIN, prism.TEXT_SIZE)) != text:
        raise RuntimeError('Fixture changed executable instructions')
    for offset in (0x16C5, 0x16C6, 0x1764):
        u.mem_write(scene.kind + offset, b'\0')
    u.mem_write(scene.kind + 0xE44, dwords(0xFFFF, 0xFFFF))
    scenario = scene.read32(0xA8B230)
    seen = []
    u.hook_add(UC_HOOK_CODE, lambda _u, at, _size, _data:
               seen.append(at) if at == 0x6F3C1A else None)
    scene.invoke(FACING_RATE_CTOR, actor + 0x388, 0)
    rows = []
    # Cardinal, diagonal and nearby stock poses; asymmetric/pivot controls
    # retain both burst parities across selected turns instead of a sweep.
    controls = (
        ([50, 0, 300], 0, (0, 0x0800, 0x2000, 0x4000, 0x8000, 0xC000), (0,)),
        ([-250, 37, -23], 95, (0, 0x1800, 0x4800, 0x8800, 0xE800), (0, 1)),
    )
    for flh, offset, headings, bursts in controls:
        u.mem_write(scene.kind + 0x89C, dwords(*flh))
        u.mem_write(scene.kind + 0x720, dwords(offset))
        for heading in headings:
            u.mem_write(HEADING, dwords(heading))
            scene.invoke(FACING_SET_CURRENT, actor + 0x388, HEADING)
            for burst in bursts:
                u.mem_write(actor + 0x3B8, dwords(burst))
                seen.clear()
                before = prism.laser_rng_state(u, scenario)
                actor_before = bytes(u.mem_read(actor, 0x720))
                type_before = bytes(u.mem_read(scene.kind, 0x1800))
                # Building453840 drops the incoming base triple before
                # forwarding into TechnoGetFLH. Keep it nonzero as a control.
                scene.invoke(GET_FLH, actor, COORD, 0, 7, 11, 13)
                if not seen or before != prism.laser_rng_state(u, scenario):
                    raise RuntimeError('Expected original identity arm with no RNG')
                if actor_before != bytes(u.mem_read(actor, 0x720)) or type_before != bytes(u.mem_read(scene.kind, 0x1800)):
                    raise RuntimeError('GetFLH changed supplied actor/type state')
                source = list(struct.unpack('<3i', u.mem_read(COORD, 12)))
                scene.invoke(BUILDING_GET_COORDS, actor, OBJECT_COORDINATE)
                rows.append(dict(input=dict(location=[3200, 3200, 0],
                    flh=list(flh), turret_offset=offset, heading=heading,
                    burst=burst, base=[7, 11, 13]), source=source,
                    object_y=struct.unpack('<3i', u.mem_read(OBJECT_COORDINATE, 12))[1]))
    if bytes(u.mem_read(prism.TEXT_BEGIN, prism.TEXT_SIZE)) != text:
        raise RuntimeError('Original execution changed executable instructions')
    return rows


def metadata():
    return provenance(
        scope='16 selected original Building453840/GetFLH6F3AD0 no-locomotor coordinate controls: stock cardinal/diagonal/nearby poses and asymmetric FLH/TurretOffset poses with both burst parities.',
        assumptions=[
            'Existing Scene supplied Building/type bytes and original Building vtable are reused; Building/type constructors and INI reads do not execute. Target is null; heading is installed by original FacingClass methods. Location3200,3200,0 and weapon0/FLH/type values are supplied.',
            'Primary pixel offset is unset; occupancy, BarrelAnimIsVoxel and TurretAnimIsVoxel are clear. The base7,11,13 control is supplied to the Building override. Its actual forwarding, GetWeapon, direction, matrix, table, ftol and GetCoords bodies execute.',
            'Selected headings cover cardinal, diagonal and nearby stock poses plus asymmetric FLH/pivot inputs in several quadrants; this is not an exhaustive heading comparison or coverage of interpolated FacingClass timing/all authored FLH integers. FPCW0x0E7F.',
            'Every row requires actual entry at6F3C1A, unchanged actor/type storage and untouched full Main/Scenario/mapgen RNG storage. The entire mapped text must match the source PE before and after execution; no historical whole-image hash is pinned. Complete source AI, FireAt, collision, damage and disk persistence are outside the fixture.',
        ],
        substitutions=[
            'Inherited Scene unrelated SelectWeapon/PlayAnim/operational boundaries remain installed but are not reached by GetFLH. No GetFLH, coordinate, matrix, trigonometry or conversion result is substituted.',
        ],
        entry_points=dict(building_get_flh=0x453840, techno_get_flh=0x6F3AD0,
                          identity_arm=0x6F3C1A, building_coords=BUILDING_GET_COORDS),
    )


def main(argv=None):
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata, argv=argv)


if __name__ == '__main__':
    main()
