//! `TechnoClass::ThreatAvoidance_Modifier @ 0x006F79A0`: a `Supress=`
//! scanner halves a candidate's score for each allied building cell on the
//! rings 1 to `FireSupress=` less one around it.
use crate::sim::house_state::HouseState;

/// `FireSupress=3`: rings 1 and 2. `OWNHUT` is 2x2.
const EXTRA: &str = "[CombatDamage]\nFireSupress=3\n\
     [InfantryTypes]\n2=SUPMAN\n[VehicleTypes]\n0=SCOUTCAR\n[BuildingTypes]\n1=OWNHUT\n\
     [SUPMAN]\nSpeed=4\nSpeedType=Foot\nStrength=75\nPrimary=SupGun\n\
     Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
     [SupGun]\nDamage=10\nRange=8\nROF=60\nProjectile=TestShot\nWarhead=TestWH\nSupress=yes\n\
     [TestShot]\nROT=0\n[TestWH]\nVerses=100%\n\
     [SCOUTCAR]\nStrength=500\nSpeed=6\n\
     Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
     [OWNHUT]\nFoundation=2x2\nStrength=200\n";

/// A `SUPMAN` at (15,15) hunting two cars, the nearer one at (17,15) and the
/// other at (12,15), with its own 2x2 building at `hut`.
fn hunt_with_hut(hut: (u16, u16)) -> (Option<u64>, u64, u64) {
    let (mut sim, rules, registry) =
        crate::sim::world::entry_test_fixture::fixture_with_rules(EXTRA);
    for (index, name) in ["Americans", "Russians"].into_iter().enumerate() {
        let owner = sim.intern(name);
        sim.houses.get_or_register_for_test(owner, || {
            HouseState::new(owner, index as u8, None, false, 0, 10)
        });
    }
    let scanner = sim
        .spawn_object("SUPMAN", "Americans", 15, 15, 0, &rules)
        .unwrap();
    let near = sim
        .spawn_object("SCOUTCAR", "Russians", 17, 15, 0, &rules)
        .unwrap();
    let far = sim
        .spawn_object("SCOUTCAR", "Russians", 12, 15, 0, &rules)
        .unwrap();
    sim.spawn_object("OWNHUT", "Americans", hut.0, hut.1, 0, &rules)
        .unwrap();
    let held = sim
        .greatest_threat_represented(
            &rules,
            Some(&registry),
            scanner,
            super::super::ScanMission::Hunt,
            None,
        )
        .and_then(crate::sim::combat::TargetKind::object_id);
    (held, near, far)
}

/// The hut's cells sit on the nearer car's rings 1 and 2 (`0x006F7AB0`,
/// `0x006F7B24`, `0x006F7BDC`, `0x006F7C50`): its score falls to a
/// sixteenth and the farther car is held.
#[test]
fn a_supress_scanner_avoids_a_car_beside_its_own_building() {
    let (held, _, far) = hunt_with_hut((18, 16));
    assert_eq!(held, Some(far));
}

/// Ring 3 is `FireSupress=` itself, past the last ring (`0x006F7C71`): a hut
/// there leaves the nearer car's score whole.
#[test]
fn a_building_on_the_fire_supress_ring_is_not_counted() {
    let (held, near, _) = hunt_with_hut((20, 15));
    assert_eq!(held, Some(near));
}
