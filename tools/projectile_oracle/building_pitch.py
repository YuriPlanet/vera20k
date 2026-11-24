"""Check the base Techno pivot getter and building-height FireAt pitch cases."""

from pathlib import Path

from tools.native_oracle import finish_vectors
from tools.projectile_oracle import fireat_fixture


def run(dx, dy, dz, speed=100, arcing=True, lobber=False, floater=False,
        source_z=0, building_height=2):
    return fireat_fixture.run(
        dx, dy, dz, speed, arcing, lobber, floater, variant="building",
        source_z=source_z, building_height=building_height,
    )


def generate():
    return [
        run(500, 0, dz, arcing=False, source_z=z, building_height=h)
        for dz in (-300, 300)
        for h in (0, 1, 2, 5)
        for z in (0, h * 200 - 21, h * 200 - 20, h * 200 - 19,
                  h * 200, h * 200 + 19, h * 200 + 20, h * 200 + 21)
    ]


def metadata():
    return fireat_fixture.launch_provenance(
        scope="64 synthetic non-arcing FireAt-tail/BulletFire building-pitch cases with original base Techno coordinate getter and +AC redispatch. Full building construction, FireAt admission and world insertion are excluded.",
        assumptions=[
            "The source has no locomotor. Original +300 getter 0x006F3D60 and +AC redispatch 0x0041BE00 execute against supplied target type and height at type+0xEF4.",
            "Source and launch coordinates are [1280,1280,source_z]; target Z is source_z+delta_z. Cases cross delta_z +/-300, heights 0/1/2/5, and eight source-Z positions around height*200 and its +/-20 boundary, all at speed 100.",
        ],
        substitutions=[
            "The class virtual returns 6 and +2A8 supplies SOURCE+0x388 as the facing reference, popping its argument. These replace type/facing leaves only; pitch arithmetic and original +300/+AC bodies execute.",
        ],
        entry_points={"base_techno_pivot_300": 0x6F3D60, "redispatch_ac": 0x41BE00},
    )


def main(argv=None):
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
                   provenance=metadata, argv=argv)


if __name__ == "__main__":
    main()
