//! Native519948 interior controls, through the production PerCell prefix.
//! The cached physical cell/object lists and mission state are supplied. This
//! does not execute Walk/PerCell2 entry, attached Tags or the repair suffix.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::{AttackTarget, TargetKind};
use crate::sim::components::NavTargetRef;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId};
use crate::sim::occupancy::CellListInsertion;
use serde_json::Value;

const INFANTRY: u64 = 70;
const HUT: u64 = 71;
const OTHER: u64 = 72;

fn identity(value: &Value) -> Option<u64> {
    match value.as_str() {
        Some("hut") => Some(HUT),
        Some("other") => Some(OTHER),
        None if value.is_null() => None,
        _ => panic!("unknown native object identity: {value}"),
    }
}

fn fixture(input: &Value, other_flags: &str) -> (Simulation, RuleSet) {
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[InfantryTypes]\n0=ENGINEER\n[VehicleTypes]\n0=TRUCK\n\
         [BuildingTypes]\n0=CABHUT\n1=OTHER\n\
         [ENGINEER]\nEngineer=yes\nStrength=75\nSpeed=4\n\
         [TRUCK]\nStrength=100\nSpeed=4\n\
         [CABHUT]\nBridgeRepairHut=yes\nFoundation=1x1\nStrength=100\n\
         [OTHER]\nFoundation=1x1\nStrength=100\n{other_flags}\n"
    )))
    .unwrap();
    let mut sim = Simulation::with_seed(31);
    sim.resolve_type_handles(&rules);
    for (id, name, owner, category) in [
        (INFANTRY, "ENGINEER", "Americans", EntityCategory::Infantry),
        (HUT, "CABHUT", "Neutral", EntityCategory::Structure),
        (OTHER, "OTHER", "Neutral", EntityCategory::Structure),
    ] {
        let mut entity = GameEntity::test_default(id, name, owner, 10, 10);
        entity.owner = sim.intern(owner);
        entity.type_ref = sim.intern(name);
        entity.category = category;
        sim.substrate.entities.insert(entity);
    }
    let infantry = sim.substrate.entities.get_mut(INFANTRY).unwrap();
    infantry.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_raw(input["current"].as_i64().unwrap() as i32),
        suspended: MissionId::NONE,
        queued: MissionId::from_raw(input["queued"].as_i64().unwrap() as i32),
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    });
    infantry.navigation.nav_com = identity(&input["nav_com"]).map(NavTargetRef::building);
    infantry.attack_target = identity(&input["attack_target"]).map(|id| AttackTarget {
        target: TargetKind::Entity(id),
    });
    let ground = input["ground_list"]
        .as_array()
        .map(|members| {
            members
                .iter()
                .map(|value| identity(value).unwrap())
                .collect()
        })
        .unwrap_or_else(|| vec![HUT]);
    for id in ground {
        sim.substrate.occupancy.add(
            10,
            10,
            id,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );
    }
    if let Some(id) = identity(&input["upper_head"]) {
        sim.substrate.occupancy.add(
            10,
            10,
            id,
            MovementLayer::Bridge,
            None,
            CellListInsertion::AppendBuilding,
        );
    }
    (sim, rules)
}

#[test]
fn ordinary_per_cell_prefix_matches_original_effective_mission_and_ground_target() {
    let rows: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/engineer_repair_admission.json",
    ))
    .unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 22);
    let mut compared = 0;
    let mut inactive_controls = Vec::new();
    for row in rows.as_array().unwrap() {
        let input = &row["input"];
        let output = &row["output"];
        if input["object_iteration_enabled"] == false {
            // A8E9A0=false is retained native evidence, not represented by
            // current Simulation/OccupancyGrid. MainTick55D360..55D371 gates
            // active-frame entry on this flag; teardown queries remain a
            // separate lifecycle boundary. Never manufacture state to pass it.
            inactive_controls.push(input["name"].as_str().unwrap());
            continue;
        }
        let (sim, rules) = fixture(input, "");
        let infantry = sim.substrate.entities.get(INFANTRY).unwrap();
        let mission_before = infantry.mission;
        let rng_before = (
            sim.scenario_rng.logical_state(),
            sim.main_rng.logical_state(),
            sim.mapgen_rng.logical_state(),
        );
        let actual = sim.infantry_per_cell_building_admission(infantry, &rules);
        let expected = match output["outcome"].as_str().unwrap() {
            "mission_not_admitted" => InfantryPerCellBuildingAdmission::MissionNotAdmitted,
            "no_matching_first_ground_building" => {
                InfantryPerCellBuildingAdmission::NoMatchingGroundBuilding
            }
            "admitted_to_engineer_type_gate" => InfantryPerCellBuildingAdmission::GroundBuilding(
                identity(&output["retained_building"]).unwrap(),
            ),
            other => panic!("unexpected native boundary {other}"),
        };
        assert_eq!(actual, expected, "{input}");
        assert_eq!(infantry.mission, mission_before, "{input}");
        assert_eq!(
            (
                sim.scenario_rng.logical_state(),
                sim.main_rng.logical_state(),
                sim.mapgen_rng.logical_state()
            ),
            rng_before,
            "{input}"
        );
        compared += 1;
    }
    assert_eq!(compared, 21);
    assert_eq!(inactive_controls, ["object_iteration_disabled"]);
}

#[test]
fn one_cell_undeploy_navcom_stays_at_the_separate_conversion_boundary() {
    let input = serde_json::json!({
        "current": 8, "queued": -1, "nav_com": "other", "attack_target": "hut"
    });
    let (mut sim, rules) = fixture(&input, "UndeploysInto=TRUCK");
    assert!(rules.object("OTHER").unwrap().is_1x1_with_undeploy());
    // +14 bit1 is Techno RTTI. Cell marking must not bypass Building+80.
    sim.substrate
        .entities
        .get_mut(OTHER)
        .unwrap()
        .lifecycle
        .cell_marked = false;
    assert_eq!(
        sim.infantry_per_cell_building_admission(
            sim.substrate.entities.get(INFANTRY).unwrap(),
            &rules
        ),
        InfantryPerCellBuildingAdmission::UndeployBuilding
    );
    assert!(
        !sim.infantry_per_cell_engineer_entry(INFANTRY, &rules, None)
            .unwrap()
            .bridge_state_changed
    );
    assert!(sim.substrate.entities.contains(INFANTRY));
    assert!(sim.sound_events.is_empty());
    assert_eq!(
        sim.substrate
            .entities
            .get(INFANTRY)
            .unwrap()
            .navigation
            .nav_com,
        Some(NavTargetRef::building(OTHER))
    );
}
