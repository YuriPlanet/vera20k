//! The MultiEngineer option's Engineer damage (`InfantryClass` PerCell
//! building entry, `0x00519D82..0x00519EA5`), through the production entry on
//! retail rules.
use super::*;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::mission::MissionId;
use crate::sim::scenario_session::ScenarioGameMode;

/// A retail GAPOWR (Strength 750) owned by the human Americans, with the
/// session's mode and MultiEngineer option.
fn scene(rules: &RuleSet, mode: ScenarioGameMode, multi_engineer: bool) -> (Simulation, u64) {
    scene_with(rules, "GAPOWR", mode, multi_engineer)
}

fn scene_with(
    rules: &RuleSet,
    type_id: &str,
    mode: ScenarioGameMode,
    multi_engineer: bool,
) -> (Simulation, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x519D82);
    sim.session.set_game_mode(mode);
    sim.session.game_options_mut_for_test().multi_engineer = multi_engineer;
    let building = sim
        .spawn_object(type_id, "Americans", 10, 10, 0, rules)
        .expect("building spawns");
    (sim, building)
}

/// A Russian Engineer arriving in the building's cell on Capture with the
/// building as NavCom, then PerCell's building entry. Returns whether the
/// Engineer is still alive.
fn enter(sim: &mut Simulation, rules: &RuleSet, building: u64) -> bool {
    let engineer = sim
        .spawn_object("ENGINEER", "Russians", 13, 10, 0, rules)
        .expect("ENGINEER spawns");
    sim.substrate
        .mission_assign_exact(engineer, MissionId::from_known(MissionType::Capture), 0)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(engineer)
        .unwrap()
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::Object { id: building }));
    sim.foot_set_location_marked(
        engineer,
        DriveCoord {
            x: 10 * 256 + 128,
            y: 10 * 256 + 128,
            z: 0,
        },
        rules,
        None,
        FrameEffects::default(),
    );
    let result = sim
        .infantry_per_cell_building_entry(engineer, rules, None, FrameEffects::default())
        .unwrap();
    assert!(result.return_before_foot);
    sim.substrate
        .entities
        .get(engineer)
        .is_some_and(|engineer| engineer.is_object_alive())
}

fn health_and_owner(sim: &Simulation, building: u64) -> (i32, &str) {
    let building = sim.substrate.entities.get(building).unwrap();
    (
        building.health.current(),
        sim.interner.resolve(building.owner()),
    )
}

/// ConditionRed=25%: each Engineer above the red line deals
/// `min(750 * 0.4375, Health - ftol(750 * 0.125))` = `min(328, Health - 93)`
/// and is spent; at or below it the next one captures.
#[test]
fn multi_engineers_wear_a_hostile_building_to_its_red_line_then_capture_it() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    assert_eq!(rules.general.condition_red, 0.25);
    assert_eq!(rules.object("GAPOWR").unwrap().strength, 750);
    let (mut sim, building) = scene(rules, ScenarioGameMode::Skirmish, true);
    let mut seen = Vec::new();
    for _ in 0..3 {
        let alive = enter(&mut sim, rules, building);
        let (health, owner) = health_and_owner(&sim, building);
        seen.push((alive, health, owner.to_string()));
    }
    assert_eq!(
        seen,
        [
            (false, 422, "Americans".to_string()),
            (false, 94, "Americans".to_string()),
            (false, 94, "Russians".to_string()),
        ]
    );

    // Short of a full share, the damage stops at the red line's half.
    let (mut sim, building) = scene(rules, ScenarioGameMode::Skirmish, true);
    sim.substrate
        .entities
        .get_mut(building)
        .unwrap()
        .health
        .set_for_test(400);
    assert!(!enter(&mut sim, rules, building));
    assert_eq!(health_and_owner(&sim, building), (93, "Americans"));
}

/// The arm precedes the `Capturable=`, Selling and warp tests
/// (`0x00519EAA..0x00519F2F`): a selling GAPOWR and an uncapturable GAPILL
/// above the red line are damaged too, and the Engineer is spent rather
/// than turned away.
#[test]
fn multi_engineers_damage_selling_and_uncapturable_buildings_too() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, plant) = scene(rules, ScenarioGameMode::Skirmish, true);
    sim.substrate
        .mission_assign_exact(plant, MissionId::from_known(MissionType::Selling), 0)
        .unwrap();
    assert!(!enter(&mut sim, rules, plant));
    assert_eq!(health_and_owner(&sim, plant), (422, "Americans"));

    let pillbox = rules.object("GAPILL").unwrap();
    assert!(!pillbox.capturable);
    let (mut sim, building) = scene_with(rules, "GAPILL", ScenarioGameMode::Skirmish, true);
    assert!(!enter(&mut sim, rules, building));
    // 400 - min(ftol(400 * 0.4375), 400 - ftol(400 * 0.125)) = 400 - 175.
    assert_eq!(health_and_owner(&sim, building), (225, "Americans"));
}

/// Without the option, or in a campaign, the first Engineer captures.
#[test]
fn without_multi_engineer_the_first_engineer_captures() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    for (mode, multi_engineer) in [
        (ScenarioGameMode::Skirmish, false),
        (ScenarioGameMode::Campaign, true),
    ] {
        let (mut sim, building) = scene(rules, mode, multi_engineer);
        assert!(!enter(&mut sim, rules, building));
        assert_eq!(health_and_owner(&sim, building), (750, "Russians"));
    }
}

/// The native slice `0x00519DB6..0x00519E82` executed under Unicorn
/// (`tools/spatial_oracle/multi_engineer_damage.py`): the retail rows and
/// inexact thresholds whose PC53/chop products differ from binary64 rounding
/// (ConditionRed 0.3 deals 424, not 425).
#[test]
fn multi_engineer_damage_matches_the_native_slice() {
    let native = crate::test_fixture::json("tools/spatial_oracle/multi_engineer_damage.json");
    let rows = native["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 15);
    for row in rows {
        let int = |key: &str| i32::try_from(row[key].as_i64().unwrap()).unwrap();
        let red = f64::from_bits(
            u64::from_str_radix(row["condition_red_bits"].as_str().unwrap(), 16).unwrap(),
        );
        let expected = row["damage"]
            .as_i64()
            .map(|damage| i32::try_from(damage).unwrap());
        assert_eq!(
            multi_engineer_damage(combat::Health::new(int("health")), int("strength"), red),
            expected,
            "{row}"
        );
    }
}
