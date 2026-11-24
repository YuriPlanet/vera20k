"""Check directed headings and Dropping origin reset.

Use ``--radial`` for the Aegis fan, signed counter and pre/post-BulletFire controls.
Both modes default to comparison; ``--write`` explicitly records new references.
"""

from pathlib import Path
import sys

from tools.native_oracle import finish_vectors
from tools.projectile_oracle import fireat_fixture


def run(dx, dy, dz, speed=100, arcing=True, lobber=False, floater=False,
        dropping=False, rot=-1, turret=False, hull=0, barrel=0):
    return fireat_fixture.run(
        dx, dy, dz, speed, arcing, lobber, floater, variant="directed",
        dropping=dropping, rot=rot, turret=turret, hull=hull, barrel=barrel,
    )


def generate():
    return [
        run(500, 200, dz, arcing=arc, dropping=drop, rot=rot,
            turret=turret, hull=hull, barrel=barrel)
        for dz in (0, -200)
        for arc in (False, True)
        for drop, rot in ((True, 0), (False, -1))
        for turret in (False, True)
        for hull, barrel in ((0, 16384), (16384, 49152), (65535, 0x1234), (0x8123, 0xCDEF))
    ]


def metadata():
    return fireat_fixture.launch_provenance(
        scope="64 synthetic directed FireAt-tail/BulletFire cases using original Unit heading receivers and Dropping origin reset. Full source construction, FireAt admission and world insertion are excluded.",
        assumptions=[
            "Source coordinates [1280,1280,0] differ from supplied launch origin [1390,1320,80]. Successful Dropping launches must retain source coordinates; other successful launches must retain the supplied launch origin.",
            "Original Unit +308 at 0x00740F80 and +2A8 at 0x00746E30 execute on supplied source type, hull/barrel words and Turret byte. The matrix crosses two Z deltas, Arcing, Dropping/ROT pairs, Turret, and four facing pairs at speed 100.",
        ],
        entry_points={"unit_heading_308": 0x740F80, "unit_facing_2a8": 0x746E30},
    )


def radial_generate():
    """Small original-code controls for the Aegis fan and signed edge cases."""
    cases = [(10, index, 0, False, 20, False, False) for index in range(10)]
    cases += [(10, index, facing, turret, 20, False, False)
              for index, facing, turret in ((0, 16384, False), (9, 65535, True),
                                           (10, 0x8123, True), (-1, 0, False),
                                           (0x7FFFFFFF, 0xFFFF, False))]
    cases += [(segments, 3, 16384, True, rot, vertical, False)
              for segments in (0, -10)
              for rot, vertical in ((20, False), (0, True), (0, False))]
    rows = [fireat_fixture.run(1200, 800, 500, speed=255, arcing=arcing,
                              variant="radial", rot=rot, turret=turret,
                              hull=facing, barrel=0x4321, vertical=vertical,
                              radial_segments=segments, radial_index=index)
            for segments, index, facing, turret, rot, vertical, arcing in cases]
    rows.append(fireat_fixture.run(1200, 800, 0, speed=1, arcing=True,
                                  variant="radial", rot=20,
                                  radial_segments=10, radial_index=9))
    return rows


def radial_metadata():
    result = fireat_fixture.launch_provenance(
        scope="22 synthetic original FireAt radial-launch and BulletFire controls: retail segments10/ROT20/speed255, complete ten-index fan, body-facing and signed counter edges, zero/negative segment and vertical controls, and a post-fan arc failure.",
        assumptions=[
            "RadialFireSegments and old per-source counter are supplied. The ten-index rows cover the stock Aegis fan; no Python arithmetic determines the observed heading or next counter.",
            "Body and turret Facing objects have supplied destination words and zero turn rates; original Facing::Current4C93D0 returns the body word. Distinct turret words verify that the fan ignores turret facing.",
            "Both native RNG streams start from original constructor seed31; their complete 0x3F4-byte states are hashed before and after. Entering Random65C780 or RandomRanged65C7E0 is an error.",
            "bits is the FireAt pre-Fire stack velocity; fired_bits is the persisted Bullet velocity after ROT normalization. launch_scalar and radial_next are observed original fields.",
        ],
        entry_points={"radial_begin": 0x6FEAC5, "radial_heading": 0x6FEB1B,
                      "radial_counter_store": 0x6FEBC8, "facing_current": 0x4C93D0},
    )
    result["assumptions"][3] = (
        "Execution must reach 0x006FF01A or the failed-launch boundary 0x006FF93C "
        "within 200000 instructions and the checked runner's time limit. Both "
        "FireAt stack and persistent Bullet velocity are retained because positive "
        "ROT renormalizes the copied vector. Failed-launch vector/pitch bytes are "
        "observations, not valid launched velocities.")
    return result


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    if "--radial" in argv:
        argv.remove("--radial")
        finish_vectors(radial_generate, Path(__file__).with_name("radial_launch.json"),
                       provenance=radial_metadata, argv=argv)
        return
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
                   provenance=metadata, argv=argv)


if __name__ == "__main__":
    main()
