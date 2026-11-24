"""Check synthetic Voxel/Vertical FireAt-tail launch vectors and retained speed."""

from pathlib import Path

from tools.native_oracle import finish_vectors
from tools.projectile_oracle import fireat_fixture


def run(dx, dy, dz, speed=100, arcing=False, lobber=False, floater=False,
        voxel=True, vertical=False):
    return fireat_fixture.run(
        dx, dy, dz, speed, arcing, lobber, floater, variant="voxel",
        voxel=voxel, vertical=vertical,
    )


def generate():
    return [
        run(*delta, voxel=vox, vertical=vert)
        for delta in ((500, 0, -300), (500, 0, 0), (500, 0, 300),
                      (0, 0, -300), (0, 0, 300))
        for vox in (False, True)
        for vert in (False, True)
    ]


def metadata():
    return fireat_fixture.launch_provenance(
        scope="20 synthetic Voxel/Vertical FireAt-tail/BulletFire cases: launch velocity bits, pitch and retained maximum speed. Full FireAt, physical voxel assets, rendering and world insertion are excluded.",
        assumptions=[
            "Five supplied deltas cross above/below/same-height and zero-horizontal-distance targets. Every case uses speed 100, non-arcing, no Lobber/Floater, source and launch origin [1280,1280,0], and crosses both Voxel and Vertical flags.",
            "The target pointer is supplied in the FireAt frame argument at EBP+8. Type Voxel+0x236 and Vertical+0x2C0 select the native branches; maximum speed is observed from Bullet+0x110 after launch.",
        ],
    )


def main(argv=None):
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
                   provenance=metadata, argv=argv)


if __name__ == "__main__":
    main()
