"""Check FireAt second-arc-solver boundary observations with seeded scratch words."""

from pathlib import Path

from tools.native_oracle import finish_vectors
from tools.projectile_oracle import fireat_fixture


def run(dx, dy, dz, speed=100, arcing=True, lobber=False, floater=False, seed=0):
    return fireat_fixture.run(
        dx, dy, dz, speed, arcing, lobber, floater,
        variant="second-probe", seed=seed,
    )


def generate():
    return [
        run(distance, 0, height, seed=seed)
        for distance in range(1660, 1676)
        for height in (0, -1)
        for seed in (0, 0xFFFFFFFF, 0x12345678)
    ]


def metadata():
    return fireat_fixture.launch_provenance(
        scope="96 synthetic FireAt-tail/BulletFire cases near the second arc-solver boundary, preserving output bytes even on solver/launch failure. Full FireAt and world insertion are excluded.",
        assumptions=[
            "Horizontal distances 1660..1675 cross target height 0/-1 at speed 100, Arcing enabled, Lobber/Floater disabled, and source/launch origin [1280,1280,0].",
            "The same supplied seed word (0, 0xFFFFFFFF or 0x12345678) initializes stack addresses SP-32 and SP-28. This is scratch-memory initialization, not an RNG seed or native initialization claim.",
            "The observer at 0x0048A954 records AL and raw eight-byte output/first-solver slots at current ESP+0x10/+0x24. Empty observation lists mean that address was not reached; observed failure bytes are not a successful solver result. No arithmetic output is supplied by the hook.",
        ],
        entry_points={"second_solver_observation": 0x48A954},
    )


def main(argv=None):
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
                   provenance=metadata, argv=argv)


if __name__ == "__main__":
    main()
