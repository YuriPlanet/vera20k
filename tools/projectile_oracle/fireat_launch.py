"""Check 144 synthetic ordinary FireAt-tail/BulletFire launch observations.

Run ``python -m tools.projectile_oracle.fireat_launch``; writing needs --write.
"""

from pathlib import Path

from tools.native_oracle import finish_vectors
from tools.projectile_oracle import fireat_fixture


def run(dx, dy, dz, speed=100, arcing=True, lobber=False, floater=False):
    return fireat_fixture.run(dx, dy, dz, speed, arcing, lobber, floater)


def generate():
    return [
        run(*delta, speed=speed, arcing=arc, lobber=lob, floater=floater)
        for delta in (
            (500, 0, 0), (500, 300, 0), (500, 0, -200), (500, 0, 300),
            (20, 0, 300), (-500, 300, 0), (-500, -300, 0), (500, -300, 0),
            (0, 500, 0), (0, -500, 0), (0, 0, 0), (5000, 0, 0),
        )
        for speed in (0, 10, 100)
        for arc, lob, floater in (
            (True, False, False), (True, True, False),
            (False, False, False), (True, False, True),
        )
    ]


def metadata():
    return fireat_fixture.launch_provenance(
        scope="144 synthetic ordinary FireAt-tail/BulletFire cases: launch success, velocity bits, pitch and selected arithmetic visits. Full FireAt, retail input construction and world insertion are excluded.",
        assumptions=[
            "Twelve supplied deltas cross zero, quadrants, height and distant targets at speeds 0, 10 and 100, each with ordinary arcing, Lobber, non-arcing and Floater configurations. Source and launch origin are [1280,1280,0].",
            "The calls list records visits to 0x0070D590, 0x0048A8D0, 0x0048A9D0 and 0x004CB3D0 without substituting their outputs; branch-dependent absence is retained.",
        ],
    )


def main(argv=None):
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
                   provenance=metadata, argv=argv)


if __name__ == "__main__":
    main()
