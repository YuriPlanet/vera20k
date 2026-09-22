"""Infantry forced-scatter gates through the real Jumpjet IsMoving query."""
from pathlib import Path
from tools.infantry_scatter_oracle import scatter_gates
from tools.native_oracle import finish_vectors, provenance


def generate():
    return scatter_gates('jumpjet')


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original Infantry51D162..51D226/51D6E6 force demotion and gates with Jumpjet54AE50',
        entry_points={'scatter_gate': 0x51D162, 'jumpjet_moving': 0x54AE50},
        assumptions=['Shared infantry_scatter_oracle supplied interior state; Jumpjet actual vtable7ECD68, phase2, independent moving byte and null destination.',
                     '64 boolean combinations of moving/force/mission-scatter/Fraidycat/target/global-scatter; second flag true and Doing=-1.',
                     'Stops before direction/RNG/destination work; excludes early human/Doing gates and complete Scatter.'],
        substitutions=['Mission table query51D176 returns supplied record; HasWeaponAbility(3)51D1DC returns false. Actual Jumpjet IsMoving executes.']))
