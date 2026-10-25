//! Original Foot Hunt's no-target join, with live destination and mission owners.
//!
//! `campaign_hunt.py` runs the original prologue then resumes at 4D54EE.
//! StupidHunt supplies that same branch boundary here; this is not a native
//! scanner, Unit constructor, ALL06 scenario, or subsequent Process comparison.
//! The separate retail Action6 integration test covers the stock producer.

use serde_json::{Value, json};

use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId};
use crate::sim::movement::DriveLocomotionRuntime;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::rng::SimRng;
use crate::sim::slave_manager::{ManagerState, SlaveManager};
use crate::sim::timer::CdTimer;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;

const ACTOR: u64 = 1;
const ZERO: DriveCoord = DriveCoord { x: 0, y: 0, z: 0 };

fn signed(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

fn cell(value: &Value) -> (u16, u16) {
    (signed(&value[0]) as u16, signed(&value[1]) as u16)
}

fn coordinate(value: &Value) -> Option<DriveCoord> {
    let coord = DriveCoord {
        x: signed(&value[0]),
        y: signed(&value[1]),
        z: signed(&value[2]),
    };
    (coord != ZERO).then_some(coord)
}

fn fixture(row: &Value, corpus: &Value) -> (Simulation, RuleSet) {
    let input = &row["input"];
    let before = &row["before"];
    // These are the oracle's supplied MissionControl rates, not retail INI
    // evidence. StupidHunt avoids the scanner's independent RNG draw.
    let rates = &corpus["inputs"]["rates"];
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[VehicleTypes]\n0=MOVER\n[MOVER]\nStupidHunt=yes\n\
         [Hunt]\nRate={}\n[Move]\nRate={}\n[Guard]\nRate={}\n",
        rates["15"], rates["2"], rates["5"],
    )))
    .unwrap();
    let mut sim = Simulation::with_seed(input["seed"].as_u64().unwrap());
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let now = signed(&input["frame"]);
    sim.session.set_binary_frame_for_test(now as u32);
    sim.session.set_game_mode(
        crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(
            signed(&input["game_mode"]) != 0,
        ),
    );
    let player = sim.intern("Americans");
    let owner = sim.intern("Russians");
    let mut player_house = HouseState::new(player, 0, None, true, 0, 10);
    player_house.set_base_center(cell(&input["primary"]));
    player_house.set_alternate_base(cell(&input["alternate"]));
    sim.houses.register(player_house);
    let mut owner_house = HouseState::new(owner, 1, None, input["human"] == true, 0, 10);
    owner_house.set_player_control_for_test(input["player_control"] == true);
    owner_house.set_base_center(cell(&input["owner_primary"]));
    owner_house.set_alternate_base(cell(&input["owner_alternate"]));
    sim.houses.register(owner_house);
    sim.session.set_current_house_for_test(Some(player));

    let (width, height) = cell(&corpus["inputs"]["allocated_map"]);
    sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(
        width,
        height,
        (0..height)
            .flat_map(|y| {
                (0..width)
                    .map(move |x| crate::sim::world::common_raw_test_terrain_cell(x, y, 0, false))
            })
            .collect(),
    ));
    sim.install_fixture_path_grid(Some(&crate::sim::pathfinding::PathGrid::new(width, height)));
    let (x, y) = cell(&corpus["inputs"]["actor_cell"]);
    let mut actor = GameEntity::test_default(ACTOR, "MOVER", "Russians", x, y);
    actor.set_type_ref_for_test(sim.intern("MOVER"));
    actor.set_owner_for_test(owner);
    actor.lifecycle.set_in_limbo_for_test(false);
    actor.position.set_sub_x_for_test(SimFixed::from_num(128));
    actor.position.set_sub_y_for_test(SimFixed::from_num(128));
    actor.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_raw(signed(&before["mission"])),
        queued: MissionId::from_raw(signed(&before["queued"])),
        suspended: MissionId::NONE,
        movement_bypass_latch: u8::from(before["ready"] == true),
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(now as u32),
    });
    actor
        .mission_leaf
        .set_unit_deploy_begin_active(u8::from(input["deploying"] == true));
    actor.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    assert!(
        actor
            .locomotor
            .as_mut()
            .unwrap()
            .install_drive_state_for_test(Some(
                DriveLocomotionRuntime::default()
                    .with_destination_for_test(coordinate(&before["destination"]))
                    .with_head_to_for_test(coordinate(&before["head"]))
            ))
    );
    // Match the sparse native Foot priors, independently of Rust defaults.
    let path = actor.navigation.path_runtime_mut();
    path.set_retries_left_for_test(0);
    path.set_path_blocked_for_test(false);
    path.set_movement_timer_for_test(CdTimer::from_raw(-1, 0));
    path.set_blocked_timer_for_test(CdTimer::from_raw(-1, 0));
    if !input["manager"].is_null() {
        let prior = &input["manager"];
        assert_eq!(signed(&prior["state"]), ManagerState::Working as i32);
        let mut manager = SlaveManager::new(sim.intern("SLAV"), [], 0, 0, now);
        manager.set_for_test(
            ManagerState::Working,
            signed(&prior["frame"]),
            vec![],
            CdTimer::from_raw(-1, 0),
        );
        actor.slave_manager = Some(manager);
    }
    sim.substrate.entities.insert(actor);
    sim.scenario_rng = SimRng::from_native_state_hex_for_test(row["rng_before"].as_str().unwrap());
    (sim, rules)
}

fn state(sim: &Simulation) -> Value {
    let actor = sim.entities().get(ACTOR).unwrap();
    let drive = actor
        .locomotor
        .as_ref()
        .and_then(|loco| loco.selected_drive_runtime())
        .and_then(|runtime| runtime.retained())
        .unwrap();
    let nav = actor.navigation.nav_com().map(|target| match target {
        NavTargetRef::Cell { rx, ry } => [rx, ry],
        NavTargetRef::Object { .. } => panic!("Hunt's supplied base is a Cell"),
    });
    let xyz = |coord: Option<DriveCoord>| {
        let coord = coord.unwrap_or(ZERO);
        [coord.x, coord.y, coord.z]
    };
    json!({
        "mission": actor.mission.current().raw(),
        "queued": actor.mission.queued().raw(),
        "ready": actor.mission.movement_bypass_latch() != 0,
        "nav": nav,
        "destination": xyz(drive.destination()),
        "head": xyz(drive.head_to()),
        "manager": actor.slave_manager.as_ref().map(|manager| json!({
            "state": manager.state() as i32,
            "frame": manager.frame(),
        })),
    })
}

#[test]
fn campaign_hunt_no_target_join_matches_original_destination_queue_and_cadence() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/campaign_hunt.json",
    ))
    .unwrap();
    assert_eq!(corpus["schema_version"], 1);
    let rows = corpus["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 12);
    for row in rows {
        let name = row["input"]["name"].as_str().unwrap();
        let (mut sim, rules) = fixture(row, &corpus);
        assert_eq!(state(&sim), row["before"], "{name}: supplied prestate");
        assert_eq!(sim.scenario_rng.native_state_hex(), row["rng_before"]);
        let main = sim.main_rng.state();
        let mapgen = sim.mapgen_rng.state();
        let result = super::evaluate_foot_hunt(&mut sim, ACTOR, &rules, Default::default());
        assert_eq!(state(&sim), row["after"], "{name}: branch effects");
        assert!(
            sim.entities()
                .get(ACTOR)
                .unwrap()
                .attack_target
                .target()
                .is_none()
        );
        if row["idle_boundary"] == false {
            assert_eq!(result.delay(), signed(&row["returned"]), "{name}: cadence");
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                row["rng_after"].as_str().unwrap(),
                "{name}: full Scenario RNG"
            );
        } else {
            // Native stops before vt+478. Unit's Rust idle receiver is a
            // no-op, but its later cadence/RNG is outside this comparison.
            assert!(row["returned"].is_null());
            assert_eq!(row["before"], row["after"], "{name}: idle boundary");
        }
        assert_eq!(sim.main_rng.state(), main, "{name}: main RNG untouched");
        assert_eq!(
            sim.mapgen_rng.state(),
            mapgen,
            "{name}: mapgen RNG untouched"
        );
    }
}
