"""Compose checked GetSpeed, FireAt-tail and motion outputs for the runtime test.

This joins existing fixture owners; it does not emulate full FireAt or Logic.
"""
from pathlib import Path

from tools.native_oracle import OracleError, finish_vectors
from tools.projectile_oracle import fireat_fixture, fireat_speed
from tools.projectile_oracle.ordinary_motion import run as ordinary_motion
from tools.projectile_oracle.vertical_motion import run as vertical_motion
from tools.rules_oracle.weapon_speed_order import fresh, metadata as reader_metadata, s32
from tools.spatial_oracle.building_body_rules import RULES


def native_inputs(voxel):
    """Use the existing native reader fixture for this test's weapon/type keys."""
    keys = {
        "OrderProbe": {"Speed": "100", "Range": "10", "Projectile": "SHOT"},
        "SHOT": {"Arcing": "no" if voxel else "yes", "Vertical": "yes" if voxel else "no"},
        "AudioVisual": {"Gravity": "6"},
    }
    machine, weapon, rules = fresh()
    machine.rules_cache(keys)
    machine.invoke(0x668BF0, rules, (RULES,))
    projectile = machine.read32(weapon + 0xA0)
    return dict(keys=keys, stored_speed=s32(machine, weapon + 0xA8),
                rot=s32(machine, projectile + 0x2DC),
                acceleration=s32(machine, projectile + 0x2D0),
                floater=bool(machine.u.mem_read(projectile + 0x295, 1)[0]),
                gravity=s32(machine, rules + 0x16B8))


def generate():
    rows = []
    for voxel in (False, True):
        origin = [3200, 3200, 1000 if voxel else 100]
        delta = [500, 0, -300 if voxel else 0]
        distance = fireat_speed.distance(dict(source=origin[:2], target=[3700, 3200]))
        inputs = native_inputs(voxel)
        speed = fireat_speed.get_speed(dict(projectile=True, rot=inputs['rot'],
                                            floater=inputs['floater'], gravity=inputs['gravity'],
                                            speed=inputs['stored_speed'], distance=distance['distance']))
        launch = fireat_fixture.run(*delta, speed['speed'], arcing=not voxel,
                                   variant='voxel' if voxel else 'ordinary',
                                   voxel=voxel, vertical=voxel,
                                   stored_weapon_speed=inputs['stored_speed'])
        if not launch['success']:
            raise OracleError('Runtime fixture launch must succeed')
        motion = (vertical_motion(launch['velocity'], inputs['acceleration'], launch['max_speed'], origin)
                  if voxel else ordinary_motion(launch['velocity'], [6, 3, 1, 0, -1, 2, 5, 6],
                                                False, origin))
        rows.append(dict(voxel=voxel, inputs=inputs, distance=distance, speed=speed, launch=launch, motion=motion))
    return rows


def metadata():
    return fireat_fixture.launch_provenance(
        scope='Two composed native launch/motion cases consumed by the headless runtime gravity/snapshot test. Full FireAt, Logic dispatch, collision and physical INI loading are excluded.',
        assumptions=[
            'Original Rules constructor and Process run in the weapon_speed_order fixture on the recorded numeric input keys. Preconstructed OrderProbe stands for the test GUN; Image/AA/DetonationAltitude and other combat admission keys are outside this numeric fixture. Native default prior Gravity=3 supplies the weapon postpass before Process reads Gravity=6. Asset/ART reading is excluded and the tail Voxel flag is supplied.',
            'Original launch distance block and GetSpeed run in fresh fireat_speed fixtures using the reader-produced stored speed, ROT and Floater. The observed launch speed and retained weapon speed feed separate slots in the shared tail fixture.',
            'The observed speed feeds the shared FireAt-tail fixture. Its ordinary source origin [1280,1280,0] and delta are translated to [3200,3200,100] for motion, or [3200,3200,1000] for Voxel/Vertical. The tail velocity depends on the supplied delta; no source-dependent building or heading branch is selected.',
            'Native binary64 launch velocities feed the existing ordinary_motion or vertical_motion fixture. Ordinary gravity sequence is [6,3,1,0,-1,2,5,6]; Vertical Acceleration=3 and its native retained maximum speed are supplied. Eight motion visits execute.',
            'Motion fixtures supply admitted position/velocity commits between visits. No native world scheduling, collision, target admission, snapshot serialization, RNG or detach behavior is established by this composition.',
        ],
        substitutions=reader_metadata()['substitutions'],
        entry_points={'weapon_reader': 0x772080, 'bullet_reader': 0x46BEE0,
                      'rules_process': 0x668BF0, 'weapon_postpass': 0x7729F0, 'get_speed': fireat_speed.GET_SPEED,
                      'launch_distance_begin': fireat_speed.DISTANCE_BEGIN,
                      'launch_distance_end': fireat_speed.DISTANCE_END,
                      'ordinary_motion_begin': 0x46718F, 'ordinary_motion_end': 0x467494,
                      'vertical_motion_begin': 0x4671E0, 'vertical_motion_end': 0x467334},
    )


def main(argv=None):
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata, argv=argv)


if __name__ == '__main__':
    main()
