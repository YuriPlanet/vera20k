//! An unarmed scanner's candidates survive weapon selection.
//! `Evaluate_Candidate` asks SelectWeapon's index and carries a null
//! GetWeapon through the fire-error probe (6F7CE4) and past the verses floor
//! (`test ebp,ebp ; je 6F7D55`): only the class gate 6F821A and the
//! zero-range `Is_Armed` arm 6F817A..6F8198 decide. The scenes use map-style
//! types without a weapon — retail engineers and spies carry kit weapons
//! (DefuseKit, MakeupKit) and stay with `engineer_tests`.
use super::*;
use crate::sim::house_state::HouseState;
use crate::sim::world::Simulation;

/// `VehicleThief=` needs `GuardRange=` for Area Guard: the mask-2 radius is
/// `Threat_Range` of GuardRange or the (zero) weapon range, so a thief with
/// neither keeps only the zero-range arm's same-cell compare.
const EXTRA: &str = "[InfantryTypes]\n2=THIEFMAN\n3=SPYMAN\n4=PLAINMAN\n5=GUNMAN\n\
     [VehicleTypes]\n0=SCOUTCAR\n\
     [THIEFMAN]\nVehicleThief=yes\nSpeed=4\nSpeedType=Foot\nStrength=75\nGuardRange=5\n\
     Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
     [SPYMAN]\nInfiltrate=yes\nSpeed=4\nSpeedType=Foot\nStrength=75\n\
     Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
     [PLAINMAN]\nSpeed=4\nSpeedType=Foot\nStrength=75\n\
     Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
     [GUNMAN]\nSpeed=4\nSpeedType=Foot\nStrength=75\nPrimary=TestGun\n\
     Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
     [TestGun]\nDamage=10\nRange=6\nROF=60\nProjectile=TestShot\nWarhead=TestWH\n\
     [TestShot]\nROT=0\n[TestWH]\nVerses=100%\n\
     [SCOUTCAR]\nStrength=500\nSpeed=6\n\
     Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n";

fn unarmed_scene() -> (
    Simulation,
    RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
) {
    let (mut sim, rules, registry) =
        crate::sim::world::entry_test_fixture::fixture_with_rules(EXTRA);
    for (index, (name, human)) in [("Americans", false), ("Russians", true)]
        .into_iter()
        .enumerate()
    {
        let owner = sim.intern(name);
        sim.houses.get_or_register_for_test(owner, || {
            HouseState::new(owner, index as u8, None, human, 0, 10)
        });
    }
    (sim, rules, registry)
}

fn pick(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    scanner: u64,
    mission: super::super::ScanMission,
) -> Option<u64> {
    sim.greatest_threat_represented(rules, Some(registry), scanner, mission, None)
}

#[test]
fn unarmed_vehicle_thief_hunt_picks_the_enemy_vehicle_over_closer_infantry() {
    let (mut sim, rules, registry) = unarmed_scene();
    let thief = sim
        .spawn_object("THIEFMAN", "Americans", 15, 15, 0, &rules)
        .unwrap();
    let closer_infantry = sim
        .spawn_object("ENGINEER", "Russians", 16, 15, 0, &rules)
        .unwrap();
    let car = sim
        .spawn_object("SCOUTCAR", "Russians", 17, 15, 0, &rules)
        .unwrap();
    let actor = sim.substrate.entities.get(thief).unwrap();
    let object = rules.object("THIEFMAN").unwrap();
    assert!(!is_armed(actor, object));
    // Hunt pushes mask 0; the Infantry override rewrites the thief's mask to
    // 0x10 (`0x0051E288..0x0051E2C4`), so the flags word holds only the Unit
    // bit and the closer enemy infantry falls at the class gate 6F821A. The
    // flat walk passes range -1 (0x006F9D70): the null-weapon ladder — probe
    // CANT, verses skipped — carries the car to the pick.
    let held = pick(
        &mut sim,
        &rules,
        &registry,
        thief,
        super::super::ScanMission::Hunt,
    );
    assert_eq!(held, Some(car));
    assert_ne!(held, Some(closer_infantry));
}

#[test]
fn unarmed_vehicle_thief_area_guard_picks_the_vehicle_within_guard_range() {
    let (mut sim, rules, registry) = unarmed_scene();
    let thief = sim
        .spawn_object("THIEFMAN", "Americans", 15, 15, 0, &rules)
        .unwrap();
    let car = sim
        .spawn_object("SCOUTCAR", "Russians", 17, 15, 0, &rules)
        .unwrap();
    // Area Guard pushes mask 2: the thief's rewrite leaves 0x12, the radius
    // is Threat_Range mode 1 of GuardRange 5 (doubled to 2560 leptons), and
    // two cells is well inside.
    let held = pick(
        &mut sim,
        &rules,
        &registry,
        thief,
        super::super::ScanMission::AreaGuard,
    );
    assert_eq!(held, Some(car));
}

/// The unarmed infiltrator is the case native lets *scan* without holding
/// anything: the override continues past `0x0051E288` (Infiltrate=yes), but
/// the scanner's weapon class bits are zero, so the flags word
/// (`0x006F8EC8..0x006F8F72`) admits no class and the walk returns empty —
/// which still clears Foot's `+0x688` latch (`0x004D9951..0x004D9955`).
#[test]
fn unarmed_infiltrator_scan_runs_clears_the_latch_and_holds_nothing() {
    let (mut sim, rules, registry) = unarmed_scene();
    let spy = sim
        .spawn_object("SPYMAN", "Americans", 15, 15, 0, &rules)
        .unwrap();
    sim.spawn_object("SCOUTCAR", "Russians", 17, 15, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(spy)
        .unwrap()
        .foot_retarget_after_stop
        .mark_stopped_cannot_fire();
    assert_eq!(
        pick(
            &mut sim,
            &rules,
            &registry,
            spy,
            super::super::ScanMission::Hunt
        ),
        None
    );
    assert!(
        !sim.substrate
            .entities
            .get(spy)
            .unwrap()
            .foot_retarget_after_stop(),
        "the scan entered Foot and its empty result cleared +0x688"
    );
    // With the latch clear the mask stays 0 and the flat walk carries the car
    // through the whole null-weapon ladder to the class gate.
    assert_eq!(
        pick(
            &mut sim,
            &rules,
            &registry,
            spy,
            super::super::ScanMission::Hunt
        ),
        None
    );
}

/// A plain unarmed infantryman is the control: the Infantry override breaks
/// before Foot (`0x0051E288`), so the latch write Foot would make never
/// happens.
#[test]
fn unarmed_plain_infantryman_returns_before_the_foot_scan() {
    let (mut sim, rules, registry) = unarmed_scene();
    let plain = sim
        .spawn_object("PLAINMAN", "Americans", 15, 15, 0, &rules)
        .unwrap();
    sim.spawn_object("SCOUTCAR", "Russians", 17, 15, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(plain)
        .unwrap()
        .foot_retarget_after_stop
        .mark_stopped_cannot_fire();
    assert_eq!(
        pick(
            &mut sim,
            &rules,
            &registry,
            plain,
            super::super::ScanMission::Hunt
        ),
        None
    );
    assert!(
        sim.substrate
            .entities
            .get(plain)
            .unwrap()
            .foot_retarget_after_stop(),
        "the override returned before Foot 0x004D9920; +0x688 stands"
    );
}

#[test]
fn an_armed_scanner_keeps_its_pick() {
    let (mut sim, rules, registry) = unarmed_scene();
    let gunman = sim
        .spawn_object("GUNMAN", "Americans", 15, 15, 0, &rules)
        .unwrap();
    let car = sim
        .spawn_object("SCOUTCAR", "Russians", 17, 15, 0, &rules)
        .unwrap();
    let actor = sim.substrate.entities.get(gunman).unwrap();
    let object = rules.object("GUNMAN").unwrap();
    assert!(is_armed(actor, object));
    assert_eq!(
        pick(
            &mut sim,
            &rules,
            &registry,
            gunman,
            super::super::ScanMission::Hunt
        ),
        Some(car)
    );
}

/// The base fixture's ENGINEER carries no weapon, unlike retail's DefuseKit
/// engineer. A computer `Engineer=` answers House+0x54E0's building before
/// the unarmed test turns it away (`0x0051E16E..0x0051E216`): the early
/// return's order is what keeps a lost building reachable for a scanner the
/// walk itself would refuse.
#[test]
fn unarmed_computer_engineer_returns_its_house_lost_building() {
    let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture();
    let owner = sim.intern("Americans");
    sim.houses
        .get_or_register_for_test(owner, || HouseState::new(owner, 0, None, false, 0, 10));
    let building = sim
        .spawn_object("CABHUT", "Americans", 17, 15, 0, &rules)
        .unwrap();
    let engineer = sim
        .spawn_object("ENGINEER", "Americans", 14, 15, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(building)
        .unwrap()
        .health
        .set_for_test(1);
    let actor = sim.substrate.entities.get(engineer).unwrap();
    let object = rules.object("ENGINEER").unwrap();
    assert!(!is_armed(actor, object));
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .record_lost_building(building);
    // Three cells apart: whole-lepton distance 768 < 0xF00.
    assert_eq!(
        pick(
            &mut sim,
            &rules,
            &registry,
            engineer,
            super::super::ScanMission::AreaGuard
        ),
        Some(building)
    );
}

/// Without a lost building the same computer engineer still does not take
/// the plain man's break: `Engineer=` implies the infiltrate flag
/// (`InfantryTypeClass::ReadINI` `0x0052466E..0x0052469A` forces Infiltrate
/// for any role), so the unarmed
/// override continues with mask `0x202` and the walk runs — buildings only
/// (`0x006F8F29..0x006F8F72`). With no `GuardRange=` its radius is zero and
/// the zero-range arm's unarmed compare admits a same-cell candidate only:
/// the scan returns empty and Foot's latch clear (`0x004D9951`) happens.
#[test]
fn unarmed_engineer_scans_holds_nothing_and_clears_the_latch() {
    let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture();
    let owner = sim.intern("Americans");
    sim.houses
        .get_or_register_for_test(owner, || HouseState::new(owner, 0, None, false, 0, 10));
    let building = sim
        .spawn_object("CABHUT", "Americans", 17, 15, 0, &rules)
        .unwrap();
    let engineer = sim
        .spawn_object("ENGINEER", "Americans", 14, 15, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(building)
        .unwrap()
        .health
        .set_for_test(1);
    sim.substrate
        .entities
        .get_mut(engineer)
        .unwrap()
        .foot_retarget_after_stop
        .mark_stopped_cannot_fire();
    assert_eq!(
        pick(
            &mut sim,
            &rules,
            &registry,
            engineer,
            super::super::ScanMission::AreaGuard
        ),
        None
    );
    assert!(
        !sim.substrate
            .entities
            .get(engineer)
            .unwrap()
            .foot_retarget_after_stop(),
        "the implied-infiltrate scan entered Foot; its empty result cleared +0x688"
    );
}

/// The `#1477` remainder: an unarmed computer `Engineer=` with a
/// `GuardRange=` takes a critically damaged own-house building through the
/// null-weapon ladder — verses skipped (`0x006F7D1B`), the mask's `0x200`
/// taking a `Capturable=` building (`0x006F8551..0x006F8589`), the allied
/// engineer arm `0x006F860C..0x006F8667` admitting it, the zero range never
/// asked because the mask-2 radius `Threat_Range` is positive. Retail
/// engineers carry DefuseKit and reach the same arm armed (`engineer_tests`).
#[test]
fn unarmed_engineer_takes_a_damaged_own_building_in_guard_range() {
    let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture_with_rules(
        "[ENGINEER]\nGuardRange=5\n[CABHUT]\nCapturable=yes\nCost=800\n",
    );
    let owner = sim.intern("Americans");
    sim.houses
        .get_or_register_for_test(owner, || HouseState::new(owner, 0, None, false, 0, 10));
    let building = sim
        .spawn_object("CABHUT", "Americans", 17, 15, 0, &rules)
        .unwrap();
    let engineer = sim
        .spawn_object("ENGINEER", "Americans", 14, 15, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(building)
        .unwrap()
        .health
        .set_for_test(1);
    assert_eq!(
        pick(
            &mut sim,
            &rules,
            &registry,
            engineer,
            super::super::ScanMission::AreaGuard
        ),
        Some(building)
    );
}

/// House+0x54E0's arm is the computer's (`0x0051E16E` reads
/// IsControlledByHuman): a human-owned engineer never takes it. It scans —
/// the implied infiltrate flag keeps the unarmed break away — but without the
/// computer's `0x200` its flags word holds no class bit, so the walk holds
/// nothing. The Area Guard *mission's* CanAcquireTarget
/// (`0x004D6ED9` → `0x007091D0`) refuses the human engineer earlier still —
/// a separate admission owner with its own tests.
#[test]
fn human_engineer_scans_nothing_even_with_a_lost_building() {
    let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture();
    let owner = sim.intern("Russians");
    sim.houses
        .get_or_register_for_test(owner, || HouseState::new(owner, 1, None, true, 0, 10));
    let building = sim
        .spawn_object("CABHUT", "Russians", 17, 15, 0, &rules)
        .unwrap();
    let engineer = sim
        .spawn_object("ENGINEER", "Russians", 14, 15, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(building)
        .unwrap()
        .health
        .set_for_test(1);
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .record_lost_building(building);
    assert_eq!(
        pick(
            &mut sim,
            &rules,
            &registry,
            engineer,
            super::super::ScanMission::AreaGuard
        ),
        None
    );
}
