//! Building virtual+A0 and Object Select's state-admission prefix.
//!
//! Steam gamemd: original44F5C0,455DA0,50B730 and5F4520..5F45A0.
//! Corpus: tools/procedural_drawing_oracle/factory_destination.py --control.
//! Prepared live state, not constructor/INI execution or full UI selection.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::capture_manager::MindControlLink;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use serde_json::Value;

const ACTOR: u64 = 70;

fn corpus() -> Vec<Value> {
    let packet: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/factory_destination.control.json",
    ))
    .unwrap();
    let rows = packet["cases"].as_array().unwrap().clone();
    assert_eq!(rows.len(), 37);
    rows
}

fn fixture(input: &Value) -> (Simulation, RuleSet) {
    let profile = match input["profile"].as_str().unwrap() {
        "factory" => "Factory=UnitType\n",
        "barracks" => "Factory=InfantryType\n",
        "repair" => "UnitRepair=yes\n",
        "cloning" => "Cloning=yes\n",
        "construction_yard" => "ConstructionYard=yes\n",
        "ordinary" => "",
        "aircraft_factory" => {
            assert_eq!(input["factory_value"], 3);
            "Factory=AircraftType\n"
        }
        other => panic!("unknown native building profile {other}"),
    };
    let undeploy = if input["undeploy"] == true {
        "UndeploysInto=VEHICLE\n"
    } else {
        ""
    };
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[BuildingTypes]\n0=BUILDING\n[VehicleTypes]\n0=VEHICLE\n\
         [BUILDING]\nStrength=1000\nSelectable=yes\n{profile}{undeploy}\
         [VEHICLE]\nStrength=100\nSpeed=4\n"
    )))
    .unwrap();
    let mut sim = Simulation::with_seed(31);
    sim.resolve_type_handles(&rules);
    let owner = sim.intern("Americans");
    let mut house = HouseState::new(
        owner,
        0,
        None,
        input["human"].as_bool().unwrap_or(true),
        0,
        1,
    );
    house.set_player_control_for_test(input["player_control"].as_bool().unwrap_or(false));
    sim.houses.register(house);
    sim.session.set_current_house_for_test(Some(owner));
    sim.session.set_game_mode(
        crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(
            input["session_mode"].as_u64().unwrap_or(0) != 0,
        ),
    );
    sim.session.game_options_mut_for_test().mcv_redeploy =
        input["mcv_redeploy"].as_bool().unwrap_or(false);
    let mut actor = GameEntity::test_default_of_category(
        ACTOR,
        "BUILDING",
        "Americans",
        10,
        20,
        EntityCategory::Structure,
    );
    actor.set_owner_for_test(owner);
    actor.set_type_ref_for_test(sim.intern("BUILDING"));
    // Match the oracle's prepared Object+81 byte. The constructor leaves
    // Rust objects in limbo; this comparison starts after that lifecycle.
    actor
        .lifecycle
        .set_in_limbo_for_test(input["limbo"].as_bool().unwrap_or(false));
    if input["mind_controlled"] == true {
        // Prepared native +2C0 pointer control. This does not execute capture.
        actor.mind_control = MindControlLink::controlled_by_for_test(ACTOR + 1);
    }
    sim.substrate.entities.insert(actor);
    (sim, rules)
}

#[test]
fn building_command_capability_matches_native_rally_and_redeploy_gates() {
    let mut compared = 0;
    let mut positive_emp_controls = 0;
    for row in corpus() {
        let input = &row["input"];
        let expected = &row["expected"];
        if input["emp"].as_i64().unwrap_or(0) > 0 {
            // EMP+504 has no runtime producer. Preserve these native controls,
            // but never manufacture a field or claim this refusal is ported.
            positive_emp_controls += 1;
            continue;
        }
        // EMP zero and negative use the same native signed <=0 branch. Rust's
        // constructor-zero state represents this decision, not negative timers.
        let (sim, rules) = fixture(input);
        assert_eq!(
            sim.techno_player_controllable(ACTOR, &rules),
            expected["controllable"].as_bool().unwrap(),
            "{input}"
        );
        assert!(
            expected["changed_actor_bytes"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        compared += 1;
    }
    assert_eq!((compared, positive_emp_controls), (29, 8));
}

#[test]
fn building_mission_only_selection_uses_native_virtual_control_admission() {
    let mut compared = 0;
    for row in corpus() {
        let input = &row["input"];
        if input["emp"].as_i64().unwrap_or(0) > 0 {
            continue;
        }
        for mission_only in [false, true] {
            let (mut sim, rules) = fixture(input);
            if mission_only {
                // Explicit original +3D4 state control. No Building producer
                // for this flag is asserted by this consumer regression.
                sim.substrate
                    .entities
                    .get_mut(ACTOR)
                    .unwrap()
                    .mission_only
                    .mark();
            }
            let key = if mission_only { "true" } else { "false" };
            assert_eq!(
                sim.can_select_object(ACTOR, &rules),
                row["expected"]["selection_admission"][key]
                    .as_bool()
                    .unwrap(),
                "mission_only={mission_only}, {input}"
            );
            assert!(
                !sim.substrate
                    .entities
                    .get(ACTOR)
                    .unwrap()
                    .selection
                    .is_selected()
            );
            compared += 1;
        }
    }
    assert_eq!(compared, 58);
}
