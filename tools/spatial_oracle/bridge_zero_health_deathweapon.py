"""Original zero-HP Infantry/Unit/Aircraft enter the death-weapon producer.

Companion to bridge_zero_health_receiver; the producer body and detonation are
excluded. Rust production tests separately exercise repeated TerrorBomb damage.
"""
from pathlib import Path
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.bridge_zero_health_receiver import original_case


def cases():
    return dict(cases=[original_case(alive, False, entry=entry, death_weapon=True)
                      for entry in (0x517FA0, 0x737C90, 0x4165C0) for alive in (0, 1)])


if __name__ == '__main__':
    finish_vectors(cases, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original zero-Health Infantry517FA0, Unit737C90, Aircraft4165C0 -> Foot/Techno/Object -> death-weapon producer70D690 entry',
        assumptions=[
            'Health0, Alive0/1, Type.Explodes1, stock-Super InfDeath2, nullsource/house, forcedtrue/true',
            'Shared supplied state/virtual results from bridge_zero_health_receiver; no DieSound entries',
            'Producer body and repeated detonation are excluded; no complete explosion parity claim',
        ], substitutions=[
            'Supplied predicate/Type/mission/height/emptyWeaponRecord virtual results as disclosed by bridge_zero_health_receiver',
            'No original receiver instructions replaced; producer70D690 is a stop boundary, not a return-value substitution',
        ], entry_points={'infantry_receive':0x517FA0, 'unit_receive':0x737C90,
                         'aircraft_receive':0x4165C0,
                         'foot_receive':0x4D7330, 'techno_receive':0x701900,
                         'object_receive':0x5F5390}))
