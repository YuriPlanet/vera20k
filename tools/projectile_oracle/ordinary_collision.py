"""Execute retail ordinary Bullet AI predicates with controlled world receivers.

The original ordinary AI tail, map-size predicate and math execute. Hooks provide
height, cell identity, first building, nearest selected object and alliance results;
those receivers are not covered by the admission vectors. Reflection vectors additionally
execute the original matrix helpers with supplied matrix bytes. This oracle does
not certify the upstream launch/trajectory or downstream damage mechanisms.
"""

import itertools
from pathlib import Path
from tools.native_oracle import NATIVE_FPCW, finish_vectors, provenance
from tools.native_slope import slope_matrices
from .collision_fixture import admission, reflection, nearest, final_handoff, geometry


def generate():
    rows = []
    for same_cell, same_building, height, vertical in itertools.product(
            (False, True), (False, True), (207, 208, 209), (False, True)):
        rows.append(admission(dict(candidate=(640 if same_cell else 384, 128, 0),
            same_building=same_building, old_height=height, vertical=vertical)))
    for selected, source, allied, inaccurate, distance in itertools.product(
            ('none', 'source', 'object'), (False, True), (False, True), (False, True), (127, 128, 129)):
        rows.append(admission(dict(selected=selected, source=source, allied=allied,
            inaccurate=inaccurate, object_coord=(384 + distance, 128, 0))))
    for velocity, height in itertools.product(((0, 0, 9), (6, 8, 0), (10, 0, 0), (3, 4, 0)), (9, 10)):
        rows.append(admission(dict(velocity=velocity, old_height=height)))
    for x, y in itertools.product((-257, -256, -255, -1, 0, 255, 256, 512, 768, 1024), (128, 512, 1024)):
        rows.append(admission(dict(candidate=(x, y, 500))))
    matrices = slope_matrices()
    reflections = [reflection(matrix, slope, velocity, elasticity)
        for slope, matrix in enumerate(matrices)
        for velocity in [(20, 3, -6), (-100, 71, -41), (0, 0, -6)]
        for elasticity in (0.0, 0.75, 1.0)]
    output = dict(
        fpcw=NATIVE_FPCW, instruction_range=['004677D3', '00467B7A'], admissions=rows,
        slope_matrices=matrices, reflections=reflections,
        geometry=[geometry(case,matrices) for case in [
            *[dict(candidate=[640,640,z],source=source,cells={'2,2':dict(overlay=overlay)})
                for z,source,overlay in itertools.product((-100.5,-100,-99.5,-0.5,0,149.5,150,150.5),(False,True),(-1,2,26,243,0))],
            *[dict(candidate=[640,640,z],old=[640,640,old_z],source=source,cells={'2,2':dict(flags=256)})
                for z,old_z,source in itertools.product((415,416,417),(415,416,417),(False,True))],
            *[dict(candidate=[640,640,149.5],source=source,source_allied=allied,building=dict(undeploy=undeploy,foundation=foundation))
                for source,allied,undeploy,foundation in itertools.product((False,True),(False,True),(False,True),(0,1))],
            dict(candidate=[640,640,149.5],source=True,building=dict(source_identity=True)),
        ]],
        final_handoffs=[final_handoff(dict(candidate=[640+distance,128,z], velocity=velocity,
            mode=mode, near_target=near, airburst=airburst, inaccurate=inaccurate, target_present=present))
            for distance,z,velocity in itertools.product((383,384,385,1200,1203), (623,624,625), ((4,0,0),(200,0,0)))
            for mode,near,airburst,inaccurate,present in [(0,True,False,False,True), (0,False,False,False,True),
                (1,False,False,False,True),(0,True,True,False,True),(0,True,False,True,True),(0,True,False,False,False)]],
        nearest=[nearest(objects) for objects in [
            [], [dict(coord=[384,128,900]), dict(coord=[640,128,0])],
            [dict(coord=[384,128,0], terrain=True), dict(coord=[640,128,900])],
            [dict(coord=[256,0,0], eligible=False), dict(coord=[511,255,0])],
            [dict(coord=[511,255,0]), dict(coord=[384,128,0], terrain=True)],
            [dict(coord=[384,128,0]), dict(coord=[384,128,0], building=True, foundation=1)],
            [dict(coord=[384,128,0]), dict(coord=[384,128,0], building=True, foundation=4)],
        ]])
    return output


def metadata():
    return provenance(
        scope='134 ordinary Bullet AI admissions, 21 native startup matrices, 189 reflections, 115 geometry cases, 180 final coordinate handoffs and 7 nearest-object selections. Bounded instruction regions only; no full trajectory or damage parity.',
        assumptions=[
            'collision_fixture owns fresh synthetic Bullet/type/source/object/cell/stack state and exact output observations. Query order, integer truncation and raw floating-point result bits are retained.',
            'Admission/reflection use live FPCW0E7F without changing cached control822D80. Geometry/nearest set both live and cached control0E7F. Final handoffs inherit the homing layout and its unchanged cached control.',
            'native_slope executes one machine from FPCW037F through7CEAAF,7CBF49(0300,0300),7C5EE4,754910,7549A0,7549C0,7549E0,754A20,754A50,754CB0; reads21x48bytes atB45188. This is the original fixture chronology, not complete Windows startup.',
            'Geometry uses the shared observation-only cell/house fixture; nearest executes original list traversal and virtual coordinate receivers. Retail foundation dimensions are read from the checked image.',
            'Regions stop before downstream detonation. These observations do not establish full lifecycle, RNG ordering, timer writes, detach effects or runtime scheduling.',
        ],
        substitutions=[
            'Admission/reflection substitute height5F5F40, cell5657A0/565730, building47C520, nearest47C3D0 and alliance4F9A90 from the case; substituted receivers are not certified.',
            'Final handoff substitutes floor578080 with208, target aim with(640,128,624), target location with(640,128,208), and synthetic source type when reached. Geometry/nearest have no behavioral hooks.',
        ],
        entry_points=dict(admission=0x4677D3, admission_end=0x467B7A,
                          reflection=0x467666, reflection_end=0x467778,
                          geometry=0x467494, geometry_end=0x4677D3,
                          nearest=0x47C3D0, handoff=0x467CA9, handoff_end=0x467E53),
    )


def main(argv=None):
    finish_vectors(generate, Path(__file__).with_name('ordinary_collision_vectors.json'),
                   provenance=metadata, argv=argv)


if __name__ == '__main__':
    main()
