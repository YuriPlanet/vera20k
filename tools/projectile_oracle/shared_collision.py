"""Execute 468BB0 and all reached receivers without behavioral hooks.

Synthetic object/cell/house state is the input. Original 565730/5657A0/578080,
47B3A0, 4CC360, 486840/4867E0, 5F6B90/41B920/661F90, 410540/447AC0,
5F6360 and 4F9A50 execute against the retail image. The only code hook records
addresses and cell-query inputs; it never changes registers, memory or control.
This is a post-commit probe oracle, not an upstream trajectory oracle.
"""
import itertools
import sys
from pathlib import Path
from tools.native_oracle import NATIVE_FPCW, finish_vectors, provenance
from .collision_fixture import shared_probe, line_walk


def generate():
    cases = []
    for z in (-417, -416, -415, -1, 0, 1):
        cases.append(dict(candidate=[640, 640, z]))
        for target_z in (-1, 0, 1):
            cases.append(dict(candidate=[640, 640, z], flak=True, target=dict(coord=[640, 640, target_z])))
    for tile in (99, 100, 113, 114, 65535):
        cases.append(dict(level=True, cells={'2,2': dict(tile=tile)}))
    for marked, height, distance in itertools.product((False, True), (207, 208, 209), (127, 128)):
        cases.append(dict(aa=True, candidate=[640 + distance, 640, height], target=dict(coord=[640, 640, height], marked=marked)))
    for phase in range(7):
        for dmisl in (False, True):
            cases.append(dict(aa=True, candidate=[640, 640, 0], target=dict(category='aircraft', rocket=True, dmisl=dmisl, phase=phase, marked=False, coord=[640, 640, 0])))
    for foundation, distance in itertools.product((0, 1, 4), (127, 255, 383)):
        cases.append(dict(aa=True, candidate=[640 + distance, 640, 300], target=dict(category='building', foundation=foundation, coord=[640, 640, 300])))
    for source_level, previous_level, candidate_level, flags in itertools.product((0, 4), (0, 1, 4), (3, 4, 5), (0, 128)):
        cases.append(dict(cliffs=True, cells={'0,0':dict(level=source_level), '1,2':dict(level=previous_level), '2,2':dict(level=candidate_level, flags=flags)}))
    for offset, source_allied, wall_allied, target_same in itertools.product(([0,0], [100,0], [0,100]), (False, True), (False, True), (False, True)):
        cases.append(dict(walls=True, transparency=True, source_allied=source_allied, wall_allied=wall_allied,
            candidate=[640+offset[0],640+offset[1],0], launch_target=[640 if target_same else 1408,640,0], cells={'2,2':dict(wall=True)}))
    for missing in ([[0,0]], [[5,2]], [[1,2]], [[2,2]], [[0,0],[5,2]], [[0,0],[5,2],[1,2],[2,2]]):
        cases.append(dict(walls=True, cliffs=True, missing=missing, candidate=[640,740,500], cells={'2,2':dict(wall=True,level=4)}))
    # Retained Bullet OnBridge is distinct from target OnBridge. These isolate
    # the downstream helper; they do not establish a stock trajectory.
    for z in (0, 1):
        cases.append(dict(on_bridge=True, candidate=[640, 640, z]))
    for z in (415, 416):
        cases.append(dict(on_bridge=True, flak=True, candidate=[640, 640, z], target=dict(coord=[640, 640, 500])))
    cases.append(dict(on_bridge=True, flak=True, candidate=[640, 640, 415], target=dict(coord=[640, 640, 415])))
    cases.append(dict(candidate=[640, 640, 0], target=dict(on_bridge=True)))
    rows = [shared_probe(case) for case in cases]
    output = dict(fpcw=NATIVE_FPCW, hooks='observation only', cases=rows)
    return output


def metadata():
    return provenance(
        scope='136 original Bullet468BB0 post-commit collision probes and reached receivers, including terrain/cliff/wall and aircraft/building cases. Not upstream trajectory, construction or downstream damage parity.',
        assumptions=[
            'collision_fixture supplies synthetic cell table, Bullet/type/source/target/house/object layouts, map dimensions, levels, overlays, alliance masks and Rules fields. Unspecified fixture memory is zero-filled.',
            'Live FPCW and cached control822D80 are both0E7F. Native receiver trace and cell queries are observed in execution order. Retail foundation dimensions come from the verified image.',
            'The appended retained OnBridge controls supply Bullet+8C separately from target+8C and cover native height<=-416, Flak height<0 and raw candidateZ<targetAimZ prerequisite; not stock trajectory admission.',
            'No assertion is made about whole-game RNG ordering, timers, detach effects or frame scheduling; this corpus observes only admission, coordinates, receiver visits and queries.',
        ],
        substitutions=['None; the code hook observes receiver visits and query inputs without changing registers, memory or control flow.'],
        entry_points=dict(probe=0x468BB0, cell_by_coord=0x565730, cell_by_index=0x5657A0,
                          ground=0x578080, cliff=0x47B3A0, aircraft=0x4CC360,
                          building_first=0x486840, building_second=0x4867E0,
                          height=0x5F6B90, building_coord=0x447AC0, alliance=0x4F9A50),
    )


def generate_line_walk():
    cases = [
        dict(name='descending_nondivisible', source_coord=[128, 128, 501], target_coord=[1153, 641, -502], cliffs=True),
        dict(name='ascending_nondivisible', source_coord=[1153, 641, -502], target_coord=[128, 128, 501], cliffs=True),
        dict(name='negative_xy_truncation', source_coord=[-129, 128, 5], target_coord=[897, 641, -6], cliffs=True),
        dict(name='wrapped_z_delta', source_coord=[128, 128, 2147483646], target_coord=[1153, 641, -2147483647], cliffs=True),
        dict(name='wrapped_z_accumulator', source_coord=[128, 128, 2147483646], target_coord=[1153, 641, -2147483641], cliffs=True),
        dict(name='same_cell', source_coord=[128, 128, 501], target_coord=[129, 129, -502], cliffs=True),
        dict(name='same_cell_missing', source_coord=[-640, 640, 501], target_coord=[-639, 641, -502], cliffs=True),
        dict(name='flags_unset', source_coord=[128, 128, 501], target_coord=[1153, 641, -502]),
        dict(name='wall_far_from_center', source_coord=[128, 640, 501], target_coord=[1408, 640, -502], walls=True, cells={'2,2': dict(wall=True)}),
        dict(name='wall_center_height_zero', source_coord=[128, 640, 0], target_coord=[1408, 640, 0], walls=True, cells={'2,2': dict(wall=True)}),
        dict(name='cliff_first_rise', source_coord=[128, 640, 501], target_coord=[1408, 640, -502], cliffs=True, cells={'2,2': dict(level=4)}),
        dict(name='shared_dummy_endpoints', source_coord=[-640, 640, 5], target_coord=[2432, 640, -6], cliffs=True),
    ]
    return dict(fpcw=NATIVE_FPCW, hooks='observation only', cases=[line_walk(case) for case in cases])


def line_walk_metadata():
    return provenance(
        scope='12 original4CC100 XYZ line walks through reached4CC360 and Cell/House receivers. Samples establish signed32 delta subtraction, signed IDIV and wrapping accumulators under supplied inputs; not retail trajectory or full InRange parity.',
        assumptions=[
            'Uses collision_fixture.prepare_shared synthetic 8x8 allocation, retained shared DummyCell, BulletType flags, wall types/houses and Rules fields. Source and target XYZ are supplied signed32 lepton coordinates; unmentioned state is zero-filled.',
            'The INT32 boundary cases are synthetic arithmetic controls admitted by4CC100 itself, not coordinates demonstrated in stock gameplay. NegativeXY and missing endpoints exercise native signed16 CellCoord narrowing and shared DummyCell stamping.',
            'Full original4CC100 and reached4CC360/Cell/House receivers execute. Hooks read each4CC360 candidateXYZ before its prologue and query order without changing native state. No RNG/timer/detach or upstream FireAt/GetFireError scheduling is modeled.',
            'FPCW and cached822D80 are both0E7F. Original Cell+48 coordinates, level/slope evaluation and distance/major-axis/requery control flow execute.',
        ],
        substitutions=['None; observation hooks do not change registers, memory or control flow.'],
        entry_points=dict(line_walk=0x4CC100, sample=0x4CC360, cell_by_index=0x5657A0,
                          level=0x487D50, alliance=0x4F9A50),
    )


def main(argv=None):
    args = list(sys.argv[1:] if argv is None else argv)
    line = '--line-walk' in args
    if line:
        args.remove('--line-walk')
    finish_vectors(generate_line_walk if line else generate,
                   Path(__file__).with_name('line_walk_vectors.json' if line else 'shared_collision_vectors.json'),
                   provenance=line_walk_metadata if line else metadata, argv=args)


if __name__ == '__main__':
    main()
