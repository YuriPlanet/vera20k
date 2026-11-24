"""Execute retail BulletClass AI admission/clamp/snap instruction ranges.

Only external floor and target-coordinate inputs are supplied by hooks; the
branching, ObjectClass height getter/setter, and Bullet coordinate setter run
the original executable bytes. This does not certify the upstream homing
trajectory, target-coordinate producers, or downstream warhead implementation.
"""

import itertools
from pathlib import Path
from tools.native_oracle import finish_vectors, provenance
from .collision_fixture import homing_admission, homing_handoff, homing_source_mode


def generate():
    admissions = [homing_admission(height, distance, velocity, airburst, empty_target)
                  for velocity, height, distance, airburst, empty_target in itertools.product(
                      [(4, 0, 0), (3, 4, 0), (0, 0, 4), (3, 4, 12)],
                      (-1, 0, 1), (1, 2, 3, 6, 7, 1000), (False, True), (False, True))]
    handoffs = [homing_handoff(height, mode, airburst, inaccurate, present)
                for height, mode, airburst, inaccurate, present in itertools.product(
                    (-1, 0, 1), (0, 1, 2), (False, True), (False, True), (False, True))]
    source_modes = [homing_source_mode(present, jumpjet, mode)
                    for present, jumpjet, mode in itertools.product(
                        (False, True), (False, True), (0, 1, 2))]
    return dict(
        coverage="admission 0x466DB1..0x466E6B; common impact clamp 0x467BF0..0x467C0C; mode1 final snap 0x467CA9..0x467E53 (near-object flag zero)",
        admissions=admissions, handoffs=handoffs, source_modes=source_modes)


def metadata():
    return provenance(
        scope='288 homing Bullet admission cases, 72 impact clamp/final-handoff observations, and 12 source fuse-mode cases. Not homing trajectory, target-coordinate production or downstream warhead parity.',
        assumptions=[
            'collision_fixture supplies fresh synthetic Bullet/type/target/source/stack state. Live FPCW is0E7F; cached control822D80 remains the verified image value, as in the original fixture.',
            'The clamp runs before the mode handoff on the same machine. Near-object stack flag remains zero in these handoffs. Coordinate writes and floor query order are observed.',
            'The72handoff rows are retained native observations without a direct Rust test consumer; admission and source-mode tables have Rust regression consumers.',
            'Regions stop before downstream detonation. No full lifecycle, RNG ordering, timer, detach or frame-scheduling equivalence is established.',
        ],
        substitutions=[
            'Floor578080 returns208 and records the input coordinate; synthetic target virtual+58 returns(640,128,624), virtual+48 returns(640,128,208); source virtual+84 returns the supplied type. These coordinate/type producers are not certified.',
        ],
        entry_points=dict(admission=0x466DB1, admission_end=0x466E6B,
                          clamp=0x467BF0, clamp_end=0x467C0C,
                          handoff=0x467CA9, handoff_end=0x467E53,
                          source_mode=0x467C3C, source_mode_end=0x467C6A),
    )


def main(argv=None):
    finish_vectors(generate, Path(__file__).with_name('homing_impact_vectors.json'),
                   provenance=metadata, argv=argv)


if __name__ == '__main__':
    main()
