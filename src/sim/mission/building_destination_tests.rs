//! Native Building455D50 goldens through shared destination and Stop consumers.
//! The native Event6 fixture ends after its destination call; target clearing,
//! nonempty radio contacts and full input/event publication are separate owners.

use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::command::Command;
use crate::sim::components::{Health, NavTargetRef};
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::MissionId;
use crate::sim::world::Simulation;
use serde_json::Value;

fn corpus() -> Value {
    let metadata: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/factory_destination.meta.json",
    ))
    .unwrap();
    assert_eq!(
        metadata["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/factory_destination.json",
    ))
    .unwrap()
}

fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=FACTORY\n1=BARRACKS\n2=REPAIR\n3=CLONING\n\
         4=CONSTRUCTION_YARD\n5=ORDINARY\n6=AIRCRAFT_FACTORY\n\
         [FACTORY]\nFactory=UnitType\n\
         [BARRACKS]\nFactory=InfantryType\n\
         [REPAIR]\nUnitRepair=yes\n\
         [CLONING]\nCloning=yes\n\
         [CONSTRUCTION_YARD]\nConstructionYard=yes\n\
         [ORDINARY]\nStrength=1000\n\
         [AIRCRAFT_FACTORY]\nFactory=AircraftType\n",
    ))
    .unwrap()
}

fn target(label: &str, shape: u8) -> Option<NavTargetRef> {
    let id = match label {
        "null" => return None,
        "old" => 2,
        "new" => 3,
        _ => panic!("unknown native pointer label {label}"),
    };
    Some(match shape {
        0 => NavTargetRef::cell(40 + id as u16, 41),
        1 => NavTargetRef::Entity { id },
        2 => NavTargetRef::Object { id },
        3 => NavTargetRef::Building { id },
        _ => unreachable!(),
    })
}

fn fixture(input: &Value, shape: u8) -> Simulation {
    let mut sim = Simulation::new();
    sim.session.binary_frame = 100;
    let owner = sim.interner.intern("Americans");
    let type_ref = sim
        .interner
        .intern(&input["profile"].as_str().unwrap().to_uppercase());
    let mut actor = GameEntity::new_at_frame_zero_for_test(
        1,
        10,
        20,
        0,
        0,
        owner,
        Health { current: 1000 },
        type_ref,
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    actor.lifecycle.in_limbo = input["limbo"].as_bool().unwrap_or(false);
    actor.lifecycle.object_alive = input["alive"].as_bool().unwrap_or(true);
    actor.dock_entered_with = input["tethered"].as_bool().unwrap_or(false).then_some(4);
    actor.set_archive_target(target("old", shape).map(TargetKind::from));
    // These Foot-only fields catch the removed represented-NavCom fallback.
    // The native Building body never touches them, regardless of admission.
    actor.navigation.nav_com = Some(NavTargetRef::cell(70, 71));
    actor.navigation.nav_com_aux = Some(NavTargetRef::cell(72, 73));
    actor.navigation.nav_queue = vec![NavTargetRef::cell(74, 75)];
    actor.navigation.pending_arrival_clear = true;
    actor.navigation.path_runtime.start_movement(40, 30);
    sim.substrate.entities.insert(actor);
    sim.mission_assign_exact(
        1,
        MissionId::from_raw(input["mission"].as_i64().unwrap_or(5) as i32),
        70,
    )
    .unwrap();
    sim
}

#[test]
fn building_destination_matches_64_native_mode_one_rows_for_all_target_shapes() {
    let corpus = corpus();
    let rules = rules();
    let cases: Vec<_> = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["input"]["kind"] == "destination" && row["input"]["mode"] != 0)
        .collect();
    assert_eq!(cases.len(), 64);
    for shape in 0..4 {
        for row in &cases {
            let input = &row["input"];
            let mut sim = fixture(input, shape);
            let actor = sim.substrate.entities.get(1).unwrap();
            let navigation = actor.navigation.clone();
            let mission = actor.mission;
            let rng = sim.scenario_rng.state();
            sim.assign_destination_represented(
                1,
                target(input["requested"].as_str().unwrap(), shape),
                Some(&rules),
                None,
            )
            .unwrap();
            let actor = sim.substrate.entities.get(1).unwrap();
            assert_eq!(
                actor.archive_target(),
                target(row["expected"]["archive"].as_str().unwrap(), shape).map(TargetKind::from),
                "{input}, target shape {shape}"
            );
            assert_eq!(actor.navigation, navigation, "{input}");
            assert_eq!(actor.mission, mission, "{input}");
            assert_eq!(sim.scenario_rng.state(), rng, "{input}");
        }
    }
}

#[test]
fn factory_stop_matches_24_native_event_destination_rows() {
    let corpus = corpus();
    let rules = rules();
    let cases: Vec<_> = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["input"]["kind"] == "stop_event")
        .collect();
    assert_eq!(cases.len(), 24);
    for row in cases {
        let input = &row["input"];
        let mut sim = fixture(input, 0);
        let actor = sim.substrate.entities.get(1).unwrap();
        let navigation = actor.navigation.clone();
        let mission = actor.mission;
        let rng = sim.scenario_rng.state();
        sim.apply_command("Americans", &Command::Stop { entity_id: 1 }, Some(&rules));
        let actor = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            actor.archive_target(),
            target(row["expected"]["archive"].as_str().unwrap(), 0).map(TargetKind::from),
            "{input}"
        );
        assert_eq!(actor.navigation, navigation, "{input}");
        assert_eq!(actor.mission, mission, "{input}");
        assert_eq!(sim.scenario_rng.state(), rng, "{input}");
    }
}
