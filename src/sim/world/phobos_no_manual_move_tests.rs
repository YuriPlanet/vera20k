//! Phobos `NoManualMove` at the shared vt+0xA0 predicate.
//!
//! `TechnoClass_CanPlayerMove @ 0x700C40` (hooked at `0x700C58`, jumping to
//! `0x700C62`) answers the player's order paths; `BuildingClass` overrides the
//! same slot at `0x44F5C0`, whose tail `0x44F62B` is what the building hook
//! forces to 0. Neither is on the mission path an AI or a script takes.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::TargetKind;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::mission::{MissionId, MissionType};

const UNIT: u64 = 90;
const BUILDING: u64 = 91;

fn scene(unit_keys: &str, building_keys: &str) -> (Simulation, RuleSet) {
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[VehicleTypes]\n0=MTNK\n[BuildingTypes]\n0=GACNST\n\
         [MTNK]\nStrength=100\nSpeed=4\n{unit_keys}\
         [GACNST]\nStrength=1000\nSelectable=yes\n{building_keys}"
    )))
    .unwrap();
    let mut sim = Simulation::with_seed(31);
    sim.resolve_type_handles(&rules);
    let owner = sim.intern("Americans");
    let mut house = HouseState::new(owner, 0, None, true, 0, 1);
    house.set_player_control_for_test(true);
    sim.houses.register(house);
    sim.session.set_current_house_for_test(Some(owner));
    let mut unit = GameEntity::test_default_of_category(
        UNIT,
        "MTNK",
        "Americans",
        10,
        20,
        EntityCategory::Unit,
    );
    unit.set_owner_for_test(owner);
    unit.set_type_ref_for_test(sim.intern("MTNK"));
    sim.substrate.entities.insert(unit);
    let mut building = GameEntity::test_default_of_category(
        BUILDING,
        "GACNST",
        "Americans",
        14,
        20,
        EntityCategory::Structure,
    );
    building.set_owner_for_test(owner);
    building.set_type_ref_for_test(sim.intern("GACNST"));
    sim.substrate.entities.insert(building);
    (sim, rules)
}

/// `0x700C58` returns false from the techno's vt+0xA0 answer, so the player's
/// admission refuses the object while the same type without the key admits it.
#[test]
fn no_manual_move_refuses_the_player_predicate_on_a_unit() {
    let (sim, rules) = scene("NoManualMove=yes\n", "");
    assert!(!sim.techno_player_controllable(UNIT, &rules));
    let (sim, rules) = scene("", "");
    assert!(sim.techno_player_controllable(UNIT, &rules));
}

/// The keys gate the player admission only: a Move mission queued through the
/// shared AI/script funnel still lands on the same object.
#[test]
fn no_manual_move_unit_still_takes_an_ai_move_mission() {
    let (mut sim, rules) = scene("NoManualMove=yes\n", "");
    sim.queue_megamission(
        UNIT,
        MissionType::Move,
        None,
        Some(TargetKind::Cell(20, 20)),
        &rules,
        FrameEffects::default(),
    );
    let unit = sim.entities().get(UNIT).expect("unit");
    let move_mission = MissionId::from_known(MissionType::Move);
    assert!(
        [unit.mission.current(), unit.mission.queued()].contains(&move_mission),
        "AI Move mission must still apply: {:?}/{:?}",
        unit.mission.current(),
        unit.mission.queued()
    );
}

/// The building override: `0x44F62B` is forced to 0, so the `UndeploysInto`
/// arm refuses while a normal deployable building still answers the click.
#[test]
fn no_manual_move_deployable_building_refuses_the_player_cell_click() {
    let (sim, rules) = scene("", "UndeploysInto=MTNK\nNoManualMove=yes\n");
    assert!(!sim.techno_player_controllable(BUILDING, &rules));
    assert!(!sim.takes_rally_point(BUILDING, "Americans", &rules));
    let (sim, rules) = scene("", "UndeploysInto=MTNK\n");
    assert!(sim.techno_player_controllable(BUILDING, &rules));
    assert!(sim.takes_rally_point(BUILDING, "Americans", &rules));
}
