//! Rust regression tests for a hunting computer engineer's way to a capture:
//! Mission_Hunt's engineer arm sends it to capture what its scan holds
//! (`0x004D53BA`), and that scan returns the building its house last lost
//! while it lies in reach (House+54E0, `0x0051E16E`). A `VehicleThief=` man
//! takes the same tail from his own arm (`0x004D546C`). Expected values come
//! from instruction reading; the only draw on these paths is the shared rate
//! epilogue's.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags, TargetKind};
use crate::sim::components::NavTargetRef;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::world::ObjectAiCtx;

/// Retail's engineer, DefuseKit and power plant, trimmed, and a vehicle thief
/// with a plain gun and a tank, which no retail section is. Hunt's rate is
/// raised from retail's `.016` so the cadence shows whose rate it took.
const RULES: &str = "\
[InfantryTypes]
0=ENGINEER
1=THIEF
[VehicleTypes]
0=TANK
[BuildingTypes]
0=GAPOWR
[Warheads]
0=HITWH
1=BombDisarm
[Guard]
Rate=.030
[Hunt]
Rate=.030
[Capture]
Rate=.016
[ENGINEER]
Strength=75
Speed=4
Engineer=yes
Primary=DefuseKit
GuardRange=9
ThreatPosed=0
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[THIEF]
Strength=75
Speed=4
VehicleThief=yes
Primary=ThiefGun
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[TANK]
Strength=300
Armor=heavy
Speed=6
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}
[GAPOWR]
Strength=750
Armor=wood
Capturable=true
[ThiefGun]
Damage=10
ROF=20
Range=1.5
Projectile=InvisibleAll
Warhead=HITWH
[DefuseKit]
Damage=1
Range=1.5
Projectile=InvisibleAll
Warhead=BombDisarm
[InvisibleAll]
Inviso=yes
AA=yes
AG=yes
[BombDisarm]
BombDisarm=yes
[HITWH]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
";

const CAPTURE: MissionId = MissionId::from_known(MissionType::Capture);

fn rules() -> RuleSet {
    RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(RULES),
        &IniFile::from_str("[GAPOWR]\nFoundation=2x2\n"),
    )
    .expect("engineer rules")
}

/// The flat arena with the computer Russians and the human Americans.
fn arena(rules: &RuleSet) -> Simulation {
    let mut sim = Simulation::with_seed(0x51E140);
    for (name, side, human) in [("Russians", 1, false), ("Americans", 0, true)] {
        let id = sim.interner.intern(name);
        sim.houses
            .insert(id, HouseState::new(id, side, None, human, 0, 10));
        sim.session.house_order.push(id);
    }
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    sim
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, rx: u16, ry: u16) -> u64 {
    sim.spawn_object_at_height(kind, owner, rx, ry, 0, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

fn house(sim: &Simulation, name: &str) -> InternedId {
    sim.interner.get(name).expect("house")
}

fn give(sim: &mut Simulation, rules: &RuleSet, building: u64, owner: &str) {
    let owner = house(sim, owner);
    sim.change_owner_with_rules(building, owner, rules, None, FrameEffects::default());
}

fn lost_building(sim: &Simulation, name: &str) -> Option<u64> {
    sim.houses[&house(sim, name)].lost_building()
}

fn assign(sim: &mut Simulation, id: u64, mission: MissionType) {
    let now = sim.session.binary_frame;
    sim.mission_assign_exact(id, MissionId::from_known(mission), now)
        .unwrap();
}

fn kill(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    let warhead = sim.interner.intern("HITWH");
    let flags = ReceiverCallFlags {
        ignore_defenses: false,
        arg6: false,
    };
    let hit =
        EntityDamageEvent::direct_receiver(id, 100_000, 0, RAD_NO_ATTACKER, None, warhead, flags);
    sim.commit_noncombat_aoe_hits(rules, None, &[hit], FrameEffects::default());
}

fn target(sim: &Simulation, id: u64) -> Option<TargetKind> {
    let entity = sim.substrate.entities.get(id)?;
    entity.attack_target.target()
}

fn heading(sim: &Simulation, id: u64) -> Option<u64> {
    match sim.substrate.entities.get(id)?.navigation.nav_com {
        Some(
            NavTargetRef::Entity { id }
            | NavTargetRef::Object { id }
            | NavTargetRef::Building { id },
        ) => Some(id),
        _ => None,
    }
}

/// The engineer arm takes the scanned building as the destination and
/// commences Capture, so the epilogue runs at Capture's rate:
/// `ftol(.016 × 900)` = 14 frames plus `RandomRanged(0, 2)`, where Hunt's
/// would be 27.
#[test]
fn a_hunting_engineer_commences_capture_of_what_its_scan_holds() {
    let rules = rules();
    let mut sim = arena(&rules);
    let plant = spawn(&mut sim, &rules, "GAPOWR", "Americans", 16, 10);
    let engineer = spawn(&mut sim, &rules, "ENGINEER", "Russians", 10, 10);
    assign(&mut sim, engineer, MissionType::Hunt);

    sim.object_ai_visit_one(engineer, Some(&rules), ObjectAiCtx::default());

    assert_eq!(target(&sim, engineer), Some(TargetKind::Entity(plant)));
    assert_eq!(heading(&sim, engineer), Some(plant));
    let mission = &sim.substrate.entities.get(engineer).unwrap().mission;
    assert_eq!(
        (mission.current(), mission.queued()),
        (CAPTURE, MissionId::NONE)
    );
    let delay = mission.dispatch_timer().delay();
    assert!((14..=16).contains(&delay), "delay {delay}");
}

/// The VehicleThief arm (`0x004D546C..0x004D548F`) asks nothing of the
/// target: the thief takes the tank his scan holds as his destination
/// (vt+0x480(target, 1)) and joins the engineer arm's tail, which commences
/// Capture at Capture's rate.
#[test]
fn a_hunting_vehicle_thief_commences_capture_of_what_its_scan_holds() {
    let rules = rules();
    let mut sim = arena(&rules);
    let tank = spawn(&mut sim, &rules, "TANK", "Americans", 16, 10);
    let thief = spawn(&mut sim, &rules, "THIEF", "Russians", 10, 10);
    assign(&mut sim, thief, MissionType::Hunt);

    sim.object_ai_visit_one(thief, Some(&rules), ObjectAiCtx::default());

    assert_eq!(target(&sim, thief), Some(TargetKind::Entity(tank)));
    assert_eq!(heading(&sim, thief), Some(tank));
    let mission = &sim.substrate.entities.get(thief).unwrap().mission;
    assert_eq!(
        (mission.current(), mission.queued()),
        (CAPTURE, MissionId::NONE)
    );
    let delay = mission.dispatch_timer().delay();
    assert!((14..=16).contains(&delay), "delay {delay}");
}

/// House+54E0 in reach is returned before the scan, over a nearer
/// capturable building the scan itself takes; out of reach (`0xF00`
/// leptons) the scan runs.
#[test]
fn a_computer_engineer_goes_back_for_the_building_its_house_lost() {
    let rules = rules();
    for (lost_at, lost_by_russians, goes_back) in
        [(18, true, true), (18, false, false), (26, true, false)]
    {
        let mut sim = arena(&rules);
        let near = spawn(&mut sim, &rules, "GAPOWR", "Americans", 11, 12);
        let first_owner = if lost_by_russians {
            "Russians"
        } else {
            "Americans"
        };
        let lost = spawn(&mut sim, &rules, "GAPOWR", first_owner, lost_at, 10);
        give(&mut sim, &rules, lost, "Americans");
        let engineer = spawn(&mut sim, &rules, "ENGINEER", "Russians", 8, 10);
        assign(&mut sim, engineer, MissionType::Hunt);

        sim.object_ai_visit_one(engineer, Some(&rules), ObjectAiCtx::default());

        let expected = if goes_back { lost } else { near };
        assert_eq!(
            target(&sim, engineer),
            Some(TargetKind::Entity(expected)),
            "lost at {lost_at}, by the Russians: {lost_by_russians}"
        );
    }
}

/// `0x00448C70`, `0x00448C8C..0x00448C94` and `0x004FB9BB..0x004FB9C3`: the
/// losing house records the building, the house that takes it forgets it
/// only when it names that building, and its expiry clears it whatever the
/// control.
#[test]
fn the_lost_building_follows_owner_changes_and_expiry() {
    let rules = rules();
    let mut sim = arena(&rules);
    let plant = spawn(&mut sim, &rules, "GAPOWR", "Russians", 16, 10);
    let other = spawn(&mut sim, &rules, "GAPOWR", "Russians", 20, 10);

    give(&mut sim, &rules, plant, "Americans");
    assert_eq!(lost_building(&sim, "Russians"), Some(plant));
    assert_eq!(lost_building(&sim, "Americans"), None);

    give(&mut sim, &rules, other, "Americans");
    give(&mut sim, &rules, plant, "Russians");
    assert_eq!(lost_building(&sim, "Russians"), Some(other));
    assert_eq!(lost_building(&sim, "Americans"), Some(plant));

    give(&mut sim, &rules, other, "Russians");
    assert_eq!(lost_building(&sim, "Russians"), None);
    assert_eq!(lost_building(&sim, "Americans"), Some(other));

    // Detach_All(0), as a cloaking object runs, and a death.
    sim.detach_all_pointer_expired(other, &rules, None, FrameEffects::default());
    assert_eq!(lost_building(&sim, "Americans"), None);
    give(&mut sim, &rules, plant, "Americans");
    kill(&mut sim, &rules, plant);
    assert!(
        sim.substrate
            .entities
            .get(plant)
            .is_none_or(|entity| !entity.lifecycle.object_alive())
    );
    assert_eq!(lost_building(&sim, "Russians"), None);
}
