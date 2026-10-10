//! Joined paid GAPILE -> two GI output through the ordinary master-frame owner.
//!
//! Native local goldens: original ExitObject443C60 -> Infantry51DFF0,
//! InfantryIdle51CBA0, the next live InfantryAI51BAB0 -> Scatter51D0D0,
//! Walk75AC80 and automatic InfantryPerCell519630 -> radio8/25/3. The fixture
//! is mechanically selected from corrected original-byte observations with
//! the registered Walk CRT and Foot EMPTY startup prerequisites executed.
//! Rally adds original SetRally443860 and the later InfantryMovement520F40 ->
//! InfantryIdle51CBA0 -> Foot4D82B0 Archive consumption/destination handoff.
//!
//! This is structural production integration with native local comparisons.
//! P2 excludes whole producer/HouseAI and unrelated admitted actor AI, whereas
//! this test runs the canonical whole tick. Absolute P2 frames, the particular
//! Scatter destination and complete RNG streams are therefore not compared.
//! Corrected native coverage follows both rally GIs through Archive consumption,
//! actual paid Walk, final Guard and cleared navigation/Walk/radio state.

use super::Simulation;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::retail_ini_fixture::retail_battle_rules_for_map;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::TerrainRules;
use crate::sim::bridge_state::BridgeRuntimeState;
use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::NavTargetRef;
use crate::sim::house_state::{HouseDifficulty, HouseState};
use crate::sim::movement::ground_pose;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::production::ProductionCategory;
use crate::sim::radio::{self, TransmitRecord};
use serde_json::{Value, json};

const TICK_MS: u32 = 67;

fn ground_command_corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/walk_first_path.json",
    ))
    .unwrap()
}

fn ground_command_history<'a>(corpus: &'a Value, name: &str) -> &'a Value {
    let row = corpus["gi_reissue"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == name)
        .unwrap_or_else(|| panic!("missing original ground-command history {name}"));
    assert_eq!(
        row["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    assert_eq!(row["status"], "PASS");
    row
}

fn ground_command_coordinate(value: &Value) -> crate::sim::components::DriveCoord {
    crate::sim::components::DriveCoord {
        x: value[0].as_i64().unwrap().try_into().unwrap(),
        y: value[1].as_i64().unwrap().try_into().unwrap(),
        z: value[2].as_i64().unwrap().try_into().unwrap(),
    }
}

fn ground_command_optional_coordinate(value: &Value) -> Option<crate::sim::components::DriveCoord> {
    let coord = ground_command_coordinate(value);
    (coord.x != 0 || coord.y != 0 || coord.z != 0).then_some(coord)
}

fn ground_command_nav_target(state: &Value) -> Option<NavTargetRef> {
    ground_command_cell_reference(state, state["nav"].as_u64().unwrap())
}

fn ground_command_cell_reference(state: &Value, pointer: u64) -> Option<NavTargetRef> {
    if pointer == 0 {
        return None;
    }
    let cell = state["cells"]
        .as_array()
        .unwrap()
        .iter()
        .find(|cell| cell["pointer"] == pointer)
        .expect("ordinary ground command NavCom names an observed Cell");
    let (rx, ry) = fixture_cell(&cell["xy"]);
    Some(NavTargetRef::cell(rx, ry))
}

fn ground_command_archive_target(state: &Value) -> Option<TargetKind> {
    ground_command_cell_reference(state, state["archive"].as_u64().unwrap()).map(|target| {
        match target {
            NavTargetRef::Cell { rx, ry } => TargetKind::Cell(rx, ry),
            _ => unreachable!("original GI archive control supplies a Cell reference"),
        }
    })
}

fn ground_command_rng_hex<'a>(row: &'a Value, state: &Value, stream: &str) -> &'a str {
    let reference = state["rng"][stream].as_str().unwrap();
    row["complete_rng_states"][reference]["bytes"]
        .as_str()
        .unwrap()
}

fn ground_command_double_bits(value: &Value) -> u64 {
    // The sole native projector exports eight memory bytes in address order.
    // This decodes a fixture, not simulation arithmetic.
    let bytes = value.as_str().expect("observed native double bytes");
    assert_eq!(bytes.len(), 16);
    u64::from_str_radix(bytes, 16).unwrap().swap_bytes()
}

fn ground_command_fraction(value: &Value) -> crate::util::fixed_math::SimFixed {
    // Drive retains its target fraction in the existing fixed-point storage.
    // Command comparisons do not certify the floating-point conversion.
    crate::util::fixed_math::SimFixed::from_num(f64::from_bits(ground_command_double_bits(value)))
}

/// Compare an input/class boundary from initialized original GI or MTNK. Map,
/// constructor and Mark use their existing production owners; retained
/// mission/locomotor/path/Stage/RNG priors come from the saved original boundary.
/// This does not replay HouseAI, factory startup or the preceding whole actor AI.
fn ground_command_fixture(
    row: &Value,
    state: &Value,
) -> Option<(Simulation, RuleSet, OverlayTypeRegistry, u64)> {
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId};
    use crate::sim::rng::SimRng;
    use crate::sim::stage::StageClass;
    use crate::sim::timer::CdTimer;

    let retail = retail_battle_rules_for_map("Hills.mmx")?;
    let type_name = state["type"]["name"]
        .as_str()
        .expect("observed original type name");
    let unit = state.get("drive").is_some();
    let context = state.get("input_context");
    let rules = if !unit && context.is_some_and(|context| context["deployer"] == 0) {
        // The original contrast supplies Type+EC8=0 after retail startup.
        // Reproduce just that type prior through the existing native ReadBool
        // production reader; the original flag store is not an INI-read claim.
        let mut ini = retail.processed_rules.clone();
        ini.merge(&crate::rules::ini_parser::IniFile::from_str(
            "[E1]\nDeployer=no\n",
        ));
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &retail.fixed_art)
            .expect("original non-Deployer component prior parses");
        rules.install_art_data(retail.rules.art().clone());
        rules.replace_animation_sequences_for_test(retail.rules.animation_sequences().clone());
        rules
    } else {
        retail.rules
    };
    let object = rules.object(type_name).expect("observed retail actor type");
    assert_eq!(
        object.speed_type as i32,
        state["type"]["speed_type"].as_i64().unwrap() as i32
    );
    assert_eq!(
        object.movement_zone as i32,
        state["type"]["movement_zone"].as_i64().unwrap() as i32
    );
    if unit {
        assert_eq!(
            state["raw_techno_2b0"], 0,
            "this Unit comparison excludes the native deployed6E0 exemption"
        );
        assert_eq!(
            object.is_simple_deployer,
            state["type"]["simple_deployer"] != 0
        );
        assert_eq!(object.teleporter, state["type"]["teleporter"] != 0);
        assert_eq!(object.balloon_hover, state["type"]["balloon_hover"] != 0);
        assert_eq!(
            object.passengers,
            state["type"]["passengers"].as_i64().unwrap() as i32
        );
        assert_eq!(object.move_to_shroud, state["type"]["move_to_shroud"] != 0);
    } else {
        assert_eq!(object.crawls, state["type"]["crawls"] != 0);
        assert_eq!(object.cyborg, state["type"]["cyborg"] != 0);
        assert_eq!(object.fraidycat, state["type"]["fraidycat"] != 0);
    }
    if let Some(context) = context.filter(|_| !unit) {
        assert_eq!(object.deployer, context["deployer"] != 0);
    }
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let terrain_rules = TerrainRules::from_ini(&retail.processed_rules);
    let mut sim = Simulation::with_seed(2);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    install_ground(
        &mut sim,
        &rules,
        &terrain_rules,
        row.get("resolver_map_prior"),
    );
    let owner = sim.interner.intern("Americans");
    let human = state["house"]["human"] != 0;
    let mut house = HouseState::new(owner, 0, Some(owner), human, 10_000, 10);
    if let Some(context) = context {
        assert_eq!(human, context["human"] != 0);
        assert_eq!(state["house"]["pointer"], context["actor_owner"]);
        assert_eq!(state["house"]["current"], context["current_house"]);
        house.player_control = context["player_control"] != 0;
    }
    house.difficulty = HouseDifficulty::Hard;
    house.project_country_mults(&rules, &sim.interner);
    sim.houses.insert(owner, house);
    sim.session.house_order.push(owner);
    sim.session.current_house =
        (state["house"]["current"] == state["house"]["pointer"]).then_some(owner);
    // The complete original no_rally bootstrap records mode1 -> Skirmish5
    // at52E10F..52E119, before these continuations. New contrast snapshots
    // carry the exact mode input; VERA stores its native zero/nonzero decision.
    sim.session.game_mode_nonzero = context
        .map(|context| context["game_mode"] != 0)
        .unwrap_or(true);
    sim.session.binary_frame = state["frame"].as_u64().unwrap().try_into().unwrap();
    sim.fog.width = sim.session.map_width;
    sim.fog.height = sim.session.map_height;
    sim.fog.reveal_all_for_owner(owner);
    let location = ground_command_coordinate(&state["location"]);
    let id = sim
        .spawn_object_at_height_with_overlay_registry(
            type_name,
            "Americans",
            (location.x / 256).try_into().unwrap(),
            (location.y / 256).try_into().unwrap(),
            64,
            0,
            &rules,
            &registry,
        )
        .expect("observed retail actor enters the supplied original clear ground");
    // Retain the observed subcell Location without leaving a competing raw
    // occupation projection. These setup calls are outside the comparison.
    assert!(sim.foot_mark_remove(id, Some(&rules), Some(&registry)));
    ground_pose::foot_set_location(
        &mut sim.substrate.entities,
        id,
        location,
        Some(&rules),
        &sim.interner,
    );
    assert!(sim.foot_mark_put(id, Some(&rules), Some(&registry)));
    if unit && state["target"] != 0 {
        // Recreate the observed admitted GAPOWR TarCom prior. The owner label
        // maps its native pointer identity; it supplies no acquisition history.
        let prior = &row["initial_target_prior"];
        assert_eq!(state["target"], prior["pointer"]);
        assert_ne!(prior["owner"], state["house"]["pointer"]);
        let target_location = ground_command_coordinate(&prior["location"]);
        // Building Reveal needs the mapped House's scenario index and base
        // reservation state. The fixture House does not reconstruct the
        // original target House's flags, HouseAI or startup.
        let target_owner = sim.interner.intern("Original-target-owner");
        sim.houses.insert(
            target_owner,
            HouseState::new(target_owner, 1, None, false, 0, 10),
        );
        sim.session.house_order.push(target_owner);
        let target = sim
            .spawn_object_at_height_with_overlay_registry(
                prior["type"]["name"].as_str().unwrap(),
                "Original-target-owner",
                (target_location.x / 256).try_into().unwrap(),
                (target_location.y / 256).try_into().unwrap(),
                128,
                0,
                &rules,
                &registry,
            )
            .expect("observed original target enters the supplied ground");
        sim.mission_commence_exact(target, sim.session.binary_frame)
            .unwrap();
        let target_actor = sim.substrate.entities.get(target).unwrap();
        assert_ne!(target_actor.owner(), owner);
        assert_eq!(
            target_actor.health.current,
            prior["health"].as_i64().unwrap() as i32
        );
        assert_eq!(target_actor.is_object_alive(), prior["alive"] != 0);
        assert_eq!(target_actor.lifecycle.in_limbo, prior["limbo"] != 0);
        assert_eq!(target_actor.lifecycle.cell_marked, prior["marked"] != 0);
        assert_eq!(
            ground_pose::position_world_coord(&target_actor.position),
            target_location
        );
        assert_eq!(
            target_actor.mission.current().raw(),
            prior["mission"].as_i64().unwrap() as i32
        );
        assert_eq!(
            target_actor.mission.queued().raw(),
            prior["queued"].as_i64().unwrap() as i32
        );
        sim.assign_target_represented(id, Some(TargetKind::Entity(target)), Some(&rules))
            .unwrap();
    }
    if row["name"] == "current_cell_raw20_B" && state["walk"]["moving"] != 0 {
        // The original contrast changes only +124; membership and the
        // retained infantry-owner slot do not change. Preserve that separation
        // through the canonical raw-byte owner rather than claiming a cache
        // or a live vehicle generated this supplied word.
        let current = ((location.x / 256) as u16, (location.y / 256) as u16);
        let cell = state["cells"]
            .as_array()
            .unwrap()
            .iter()
            .find(|cell| fixture_cell(&cell["xy"]) == current)
            .unwrap();
        assert_eq!(cell["raw_ground"], 0x20);
        sim.substrate
            .raw_cell_occupation
            .clear_ground(current.0, current.1, u8::MAX);
        sim.substrate
            .raw_cell_occupation
            .mark_ground(current.0, current.1, 0x20);
    }
    let timer = |value: &Value| {
        // +644/+66C/+104 are copied stack residue. Timer owners retain the
        // original anchor and duration (+640/+648 etc.), never this padding.
        CdTimer::from_raw(
            value[0].as_i64().unwrap().try_into().unwrap(),
            value[2].as_i64().unwrap().try_into().unwrap(),
        )
    };
    let actor = sim.substrate.entities.get_mut(id).unwrap();
    assert_eq!(
        actor.locomotor.as_ref().unwrap().active_kind(),
        if unit {
            LocomotorKind::Drive
        } else {
            LocomotorKind::Walk
        }
    );
    actor.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_raw(state["mission"].as_i64().unwrap() as i32),
        suspended: state
            .get("suspended_mission")
            .map_or(MissionId::NONE, |value| {
                MissionId::from_raw(value.as_i64().unwrap() as i32)
            }),
        queued: MissionId::from_raw(state["queued"].as_i64().unwrap() as i32),
        movement_bypass_latch: 0,
        handler_state: state["mission_status"].as_u64().unwrap() as u32,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::from_raw(
            state["mission_timer"][0].as_i64().unwrap() as i32,
            state["mission_timer"][2].as_i64().unwrap() as i32,
        ),
    });
    if unit {
        actor.set_unit_simple_deploy_for_test(
            state["deploy_bytes"][0] != 0,
            state["deploy_bytes"][1] != 0,
            state["deploy_bytes"][2] != 0,
        );
        actor.setter_force_reassign = state["force_reassign"] != 0;
        assert_eq!(
            state["skip_move"], 0,
            "stock MTNK has no producer for Foot+6AC"
        );
        assert_eq!(
            actor.health.current,
            state["health"].as_i64().unwrap() as i32
        );
        assert_eq!(actor.lifecycle.object_alive, state["alive"] != 0);
        assert_eq!(actor.lifecycle.in_limbo, state["limbo"] != 0);
    } else {
        actor
            .mission_leaf
            .set_infantry_doing_verified(state["doing"].as_i64().unwrap() as i32)
            .unwrap();
        actor.infantry.as_mut().unwrap().is_prone = state["prone"] != 0;
        actor.infantry.as_mut().unwrap().cell_entry_blocked = state["entry_blocked"] != 0;
    }
    if let Some(context) = context {
        actor.berserk.active = context["berserk"] != 0;
    }
    actor.install_native_stage_fixture(StageClass::from_native_fixture(
        state["stage_words"][0].as_i64().unwrap() as i32,
        state["stage_words"][1].as_u64().unwrap() as u8,
        timer(&state["sequence_timer"]),
        state["sequence_timer"][3].as_i64().unwrap() as i32,
        // The shared Stage constructor6F2B5E retains +110=1. The original
        // initialized GI and these input histories contain no increment write.
        1,
    ));
    actor.navigation.nav_com = ground_command_nav_target(state);
    actor.set_archive_target(ground_command_archive_target(state));
    actor.navigation.path_replay.directions = state["path"]
        .as_array()
        .unwrap()
        .iter()
        .map(|word| {
            let word = word.as_i64().unwrap();
            assert!((-1..=8).contains(&word));
            word as u8
        })
        .collect();
    let reference = fixture_cell(&state["reference_cell"]);
    actor.navigation.path_replay.reference_cell = Some((reference.0 as i16, reference.1 as i16));
    actor.navigation.path_runtime.movement_timer = timer(&state["movement_timer"]);
    actor.navigation.path_runtime.blocked_timer = timer(&state["blocked_timer"]);
    actor.navigation.path_runtime.path_blocked = state["raw_6b7"] != 0;
    actor.navigation.path_runtime.retries_left = state["retry"].as_u64().unwrap() as u32;
    assert_eq!(state["nav_queue"]["count"], 0);
    assert_eq!(state["destination_history"]["count"], 0);
    let loco = actor.locomotor.as_mut().unwrap();
    if unit {
        use crate::sim::components::TrackProgress;
        use crate::sim::movement::DriveLocomotionRuntime;
        let drive = &state["drive"];
        loco.powered = drive["power"] != 0;
        assert!(
            loco.install_drive_state_for_test(Some(
                DriveLocomotionRuntime::default()
                    .with_destination_for_test(ground_command_optional_coordinate(
                        &drive["destination"]
                    ))
                    .with_head_to_for_test(ground_command_optional_coordinate(&drive["head"]))
                    .with_track_for_test(TrackProgress {
                        turn_index: drive["selector"].as_i64().unwrap() as i32,
                        cursor: drive["cursor"].as_i64().unwrap() as i32,
                        reversed: drive["reversed"] != 0,
                        residual: drive["word_4c"].as_u64().unwrap() as i32
                    })
                    .with_track_valid_for_test(drive["valid"] != 0)
                    .with_turn_latched_for_test(drive["latch"] != 0)
                    .with_end_permitted_for_test(drive["flag_65"] != 0)
                    .with_target_speed_fraction_for_test(ground_command_fraction(
                        &drive["target_speed_fraction_bits"]
                    ))
            ))
        );
        actor
            .foot_speed
            .set_speed_fraction_native_bits(ground_command_double_bits(
                &state["speed_fraction_bits"],
            ));
        if ground_command_optional_coordinate(&drive["head"]).is_some() {
            actor.movement_target = Some(crate::sim::components::MovementTarget {
                speed: crate::sim::movement::order_speed(
                    actor,
                    Some(object),
                    Some(&rules),
                    &sim.houses,
                ),
                final_goal: None,
            });
        }
    } else {
        loco.set_step_head(ground_command_optional_coordinate(&state["walk"]["head"]));
        loco.set_walk_destination(ground_command_optional_coordinate(
            &state["walk"]["destination"],
        ));
        if state["walk"]["motion"] != 0 {
            loco.begin_walk_motion();
        }
        assert_eq!(loco.walk_is_moving(), Some(state["walk"]["moving"] != 0));
    }
    if let Some(occupant) = row["ordered_calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|call| call["kind"] == "existing_original_redirect_occupant")
    {
        // These are the real original FindPath target inputs, not a supplied
        // CanEnter answer. Recreate only the target object needed by the
        // redirect; unrelated factory objects and the AStar continuation are
        // outside this class-boundary comparison.
        let type_id = match row["name"].as_str().unwrap() {
            "findpath_existing_idle_MTNK" => "MTNK",
            "findpath_existing_GAPILE" => "GAPILE",
            name => panic!("unexpected original redirect occupant {name}"),
        };
        let cell = fixture_cell(&occupant["cell"]["xy"]);
        let target = sim
            .spawn_object_at_height_with_overlay_registry(
                type_id,
                "Americans",
                cell.0,
                cell.1,
                128,
                0,
                &rules,
                &registry,
            )
            .expect("original redirect target enters the supplied clear ground");
        // The original live target has already commenced Guard. Runtime
        // Building placement leaves that mission queued; finish it through
        // the mission owner before this component-boundary comparison. This
        // does not compare the target's whole initialization or AI history.
        sim.mission_commence_exact(target, sim.session.binary_frame)
            .unwrap();
        let target_actor = sim.substrate.entities.get(target).unwrap();
        let ambient = occupant
            .get("ambient")
            .expect("refreshed original redirect exports its live target inputs");
        assert_eq!(ambient["owner"], ambient["gi_owner"]);
        assert_eq!(target_actor.owner(), owner);
        assert_eq!(
            target_actor.health.current,
            ambient["health"].as_i64().unwrap() as i32
        );
        assert_eq!(target_actor.is_object_alive(), ambient["alive"] != 0);
        assert_eq!(target_actor.lifecycle.in_limbo, ambient["limbo"] != 0);
        assert_eq!(target_actor.lifecycle.cell_marked, ambient["marked"] != 0);
        assert_eq!(target_actor.on_bridge, ambient["on_bridge"] != 0);
        assert_eq!(
            target_actor.in_logic_vector,
            ambient["logic_registered"] != 0
        );
        assert_eq!(
            target_actor.mission.current().raw(),
            ambient["mission"].as_i64().unwrap() as i32
        );
        assert_eq!(
            target_actor.mission.queued().raw(),
            ambient["queued"].as_i64().unwrap() as i32
        );
        assert_eq!(
            ground_pose::position_world_coord(&target_actor.position),
            ground_command_coordinate(&occupant["location"])
        );
        if type_id == "MTNK" {
            let native_type = &ambient["type"];
            let object = rules.object(type_id).unwrap();
            assert_eq!(object.speed_type as i32, native_type["speed_type"]);
            assert_eq!(object.movement_zone as i32, native_type["movement_zone"]);
            assert_eq!(object.balloon_hover, native_type["balloon_hover"] != 0);
            assert_eq!(object.teleporter, native_type["teleporter"] != 0);
            assert_eq!(object.passengers, native_type["passengers"]);
            assert_eq!(
                target_actor.locomotor.as_ref().unwrap().active_kind(),
                LocomotorKind::Drive
            );
            assert_eq!(ambient["foot"]["nav"], 0);
            assert!(target_actor.navigation.nav_com.is_none());
            assert_eq!(
                crate::sim::movement::motion_query::is_moving(target_actor),
                Some(false)
            );
        }
        for (layer, field) in [
            (
                crate::sim::movement::locomotor::MovementLayer::Ground,
                "raw_ground",
            ),
            (
                crate::sim::movement::locomotor::MovementLayer::Bridge,
                "raw_upper",
            ),
        ] {
            assert_eq!(
                u64::from(sim.substrate.raw_cell_occupation.bits_at(
                    crate::sim::occupancy::RawCellKey::Real(cell.0, cell.1),
                    layer,
                )),
                occupant["cell"][field].as_u64().unwrap()
            );
        }
    }
    if let Some(map) = row.get("resolver_map_prior") {
        // Import the original retained Map+68/Map+18 inputs through the
        // existing save/load owner. Rebuilding connectivity from this test's
        // terrain would invent the original cluster numbering/history.
        // This is an input/resolver comparison, not a map-load/zone-producer
        // parity claim. Actor/target Mark setup has completed before import.
        let navigation = &map["navigation"];
        let mut zones: crate::sim::pathfinding::zone_map::ZoneGrid =
            serde_json::from_value(json!({
                "width": sim.session.map_width,
                "height": sim.session.map_height,
                "base_topology": {
                    "native_bridge_source_size": map["map_size"],
                    "movement_classes": navigation["classes"],
                    "levels": navigation["levels"],
                    "zone_ids": navigation["base_ids"],
                    "raw_zone_ids_by_row": navigation["raw_rows"],
                },
            }))
            .expect("observed original retained navigation inputs parse");
        zones
            .finish_native_load(
                sim.path_grid.as_deref().unwrap(),
                sim.resolved_terrain.as_ref().unwrap(),
                &[],
                sim.playfield_bounds,
            )
            .expect("the existing load owner binds the supplied flat map");
        sim.zone_grid = Some(zones);
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        for cell in map["cells"].as_array().unwrap() {
            let (rx, ry) = fixture_cell(&cell["xy"]);
            let actual = terrain.cell(rx, ry).unwrap();
            assert_eq!(json!(actual.yr_cell_land_type), cell["land"]);
            assert_eq!(json!(actual.level), cell["level"]);
            assert_eq!(json!(actual.slope_type), cell["slope"]);
            for (layer, field) in [
                (
                    crate::sim::movement::locomotor::MovementLayer::Ground,
                    "raw_ground",
                ),
                (
                    crate::sim::movement::locomotor::MovementLayer::Bridge,
                    "raw_upper",
                ),
            ] {
                assert_eq!(
                    json!(
                        sim.substrate
                            .raw_cell_occupation
                            .bits_at(crate::sim::occupancy::RawCellKey::Real(rx, ry), layer,)
                    ),
                    cell[field],
                );
            }
        }
    }
    sim.main_rng =
        SimRng::from_native_state_hex_for_test(ground_command_rng_hex(row, state, "main"));
    sim.mapgen_rng =
        SimRng::from_native_state_hex_for_test(ground_command_rng_hex(row, state, "mapgen"));
    sim.scenario_rng =
        SimRng::from_native_state_hex_for_test(ground_command_rng_hex(row, state, "scenario"));
    Some((sim, rules, registry, id))
}

fn assert_ground_command_fields(sim: &Simulation, id: u64, row: &Value, state: &Value) {
    let actor = sim.substrate.entities.get(id).unwrap();
    let walk = actor.locomotor.as_ref().unwrap();
    let context = row["name"].as_str().unwrap();
    assert_eq!(
        ground_pose::position_world_coord(&actor.position),
        ground_command_coordinate(&state["location"]),
        "{context}: Location"
    );
    assert_eq!(
        actor.mission.current().raw(),
        state["mission"].as_i64().unwrap() as i32,
        "{context}: current mission"
    );
    assert_eq!(
        actor.mission.queued().raw(),
        state["queued"].as_i64().unwrap() as i32,
        "{context}: queued mission"
    );
    if let Some(drive) = state.get("drive") {
        use crate::sim::components::TrackProgress;
        use crate::sim::movement::track_process::TrackFamily;
        let leaf = actor.mission_leaf.as_unit().unwrap();
        assert_eq!(
            json!([
                leaf.deployed(),
                leaf.deploy_begin_active(),
                leaf.deploy_reverse_active()
            ]),
            state["deploy_bytes"],
            "{context}: Unit deploy bytes"
        );
        assert_eq!(
            actor.setter_force_reassign,
            state["force_reassign"] != 0,
            "{context}: force reassign"
        );
        assert_eq!(
            actor.mission.suspended().raw(),
            state["suspended_mission"].as_i64().unwrap() as i32,
            "{context}: suspended mission"
        );
        assert_eq!(
            actor.navigation.nav_com_aux,
            ground_command_cell_reference(state, state["aux_nav"].as_u64().unwrap()),
            "{context}: auxiliary NavCom"
        );
        match actor.attack_target.as_ref().map(|target| target.target) {
            None => assert_eq!(
                state["target"], 0,
                "{context}: Target cleared before destination"
            ),
            Some(TargetKind::Entity(target)) => {
                let prior = &row["initial_target_prior"];
                assert_eq!(
                    state["target"], prior["pointer"],
                    "{context}: retained Target identity"
                );
                let target = sim.substrate.entities.get(target).unwrap();
                assert_eq!(
                    sim.interner.resolve(target.type_ref()),
                    prior["type"]["name"].as_str().unwrap()
                );
                assert_eq!(
                    ground_pose::position_world_coord(&target.position),
                    ground_command_coordinate(&prior["location"])
                );
                assert_ne!(target.owner(), actor.owner());
            }
            Some(TargetKind::Cell(..)) => panic!("{context}: original Unit TarCom is not a Cell"),
        }
        assert_eq!(
            walk.track_destination(TrackFamily::Drive),
            ground_command_optional_coordinate(&drive["destination"]),
            "{context}: Drive destination"
        );
        assert_eq!(
            walk.track_head(TrackFamily::Drive),
            ground_command_optional_coordinate(&drive["head"]),
            "{context}: paid Drive head"
        );
        assert_eq!(
            walk.track_progress(TrackFamily::Drive),
            Some(TrackProgress {
                turn_index: drive["selector"].as_i64().unwrap() as i32,
                cursor: drive["cursor"].as_i64().unwrap() as i32,
                reversed: drive["reversed"] != 0,
                residual: drive["word_4c"].as_u64().unwrap() as i32
            }),
            "{context}: retained track progress"
        );
        assert_eq!(
            walk.track_valid(TrackFamily::Drive),
            Some(drive["valid"] != 0),
            "{context}: track-valid byte"
        );
        assert_eq!(
            walk.track_turn_latched(TrackFamily::Drive),
            Some(drive["latch"] != 0),
            "{context}: turn latch"
        );
        assert_eq!(
            walk.drive_end_permitted(),
            drive["flag_65"] != 0,
            "{context}: END permission"
        );
        assert_eq!(
            walk.powered,
            drive["power"] != 0,
            "{context}: locomotor power"
        );
        assert_eq!(
            walk.track_target_fraction(TrackFamily::Drive),
            Some(ground_command_fraction(
                &drive["target_speed_fraction_bits"]
            )),
            "{context}: Drive target fraction +50"
        );
        let mut native_fraction = crate::sim::components::FootSpeedState::default();
        native_fraction.set_speed_fraction_native_bits(ground_command_double_bits(
            &state["speed_fraction_bits"],
        ));
        assert_eq!(
            actor.foot_speed.applied_fraction(),
            native_fraction.applied_fraction(),
            "{context}: Foot applied fraction"
        );
        assert!(
            actor
                .movement_target
                .as_ref()
                .is_none_or(|target| target.final_goal.is_none()),
            "{context}: no copied Drive goal"
        );
    } else {
        assert_eq!(
            actor.mission_leaf.as_infantry().unwrap().doing(),
            state["doing"].as_i64().unwrap() as i32,
            "{context}: Doing"
        );
        assert_eq!(
            actor.infantry.as_ref().unwrap().is_prone,
            state["prone"] != 0,
            "{context}: prone"
        );
        assert_eq!(
            actor.infantry.as_ref().unwrap().cell_entry_blocked,
            state["entry_blocked"] != 0,
            "{context}: entry latch"
        );
    }
    assert_eq!(
        actor.navigation.nav_com,
        ground_command_nav_target(state),
        "{context}: NavCom"
    );
    assert_eq!(
        actor.archive_target(),
        ground_command_archive_target(state),
        "{context}: ArchiveTarget"
    );
    assert!(actor.navigation.nav_queue.is_empty(), "{context}: NavQueue");
    if state.get("drive").is_none() {
        assert_eq!(
            walk.walk_destination(),
            ground_command_optional_coordinate(&state["walk"]["destination"]),
            "{context}: Walk destination"
        );
        assert_eq!(
            walk.step_head(),
            ground_command_optional_coordinate(&state["walk"]["head"]),
            "{context}: paid head"
        );
        assert_eq!(
            walk.walk_is_moving(),
            Some(state["walk"]["moving"] != 0),
            "{context}: IsMoving"
        );
        assert_eq!(
            walk.walk_animation_moving(),
            Some(state["walk"]["motion"] != 0),
            "{context}: motion"
        );
    }
    let native_path: Vec<u8> = state["path"]
        .as_array()
        .unwrap()
        .iter()
        .map(|word| word.as_i64().unwrap() as u8)
        .collect();
    assert_eq!(
        &actor.navigation.path_replay.directions
            [usize::from(actor.navigation.path_replay.cursor)..],
        native_path,
        "{context}: full path backing suffix"
    );
    let reference = fixture_cell(&state["reference_cell"]);
    assert_eq!(
        actor.navigation.path_replay.reference_cell,
        Some((reference.0 as i16, reference.1 as i16)),
        "{context}: path reference"
    );
    for (actual, expected) in [
        (
            actor.navigation.path_runtime.movement_timer,
            &state["movement_timer"],
        ),
        (
            actor.navigation.path_runtime.blocked_timer,
            &state["blocked_timer"],
        ),
    ] {
        assert_eq!(
            json!([actual.start_frame(), actual.duration()]),
            json!([expected[0], expected[2]]),
            "{context}: Foot timer"
        );
    }
    assert_eq!(
        actor.navigation.path_runtime.path_blocked,
        state["raw_6b7"] != 0
    );
    assert_eq!(
        actor.navigation.path_runtime.retries_left,
        state["retry"].as_u64().unwrap() as u32
    );
    assert_eq!(
        actor.mission.handler_state(),
        state["mission_status"].as_u64().unwrap() as u32,
        "{context}: mission status"
    );
    let mission_timer = actor.mission.dispatch_timer();
    assert_eq!(
        json!([mission_timer.start_frame(), mission_timer.delay()]),
        json!([state["mission_timer"][0], state["mission_timer"][2]]),
        "{context}: mission timer"
    );
    let stage = actor.native_stage();
    assert_eq!(
        stage.value(),
        state["stage_words"][0].as_i64().unwrap() as i32,
        "{context}: Stage"
    );
    assert_eq!(
        stage.rate(),
        state["sequence_timer"][3].as_i64().unwrap() as i32,
        "{context}: Stage rate"
    );
    assert_eq!(
        json!([stage.timer().start_frame(), stage.timer().duration()]),
        json!([state["sequence_timer"][0], state["sequence_timer"][2]]),
        "{context}: Stage timer"
    );
    for (stream, actual) in [
        ("main", &sim.main_rng),
        ("mapgen", &sim.mapgen_rng),
        ("scenario", &sim.scenario_rng),
    ] {
        assert_eq!(
            actual.native_state_hex(),
            ground_command_rng_hex(row, state, stream),
            "{context}: complete {stream} RNG"
        );
    }
}

/// Real original input -> MegaMission -> class51AA40 returns, joined twice in
/// Rust without an intervening AI visit. The later-frame control advances
/// only the recorded frame, not whole AI. The Shift producer/codec choice is
/// separately checked by the app input test; decoded Cell commands here have
/// native ordinary (flag1) semantics regardless of the physical modifier.
#[test]
fn initialized_gi_reissues_match_original_command_returns() {
    check_initialized_ground_reissues(
        &[
            "plain_A_B",
            "shift_A_B",
            "other12_A_B",
            "plain_A_A",
            "plain_A_later_B",
            "attack_same_A",
            "prone_A_A",
        ],
        simulation_ground_command,
    );
}

/// The upper-layer boundary tests supply their real producer/codec here;
/// this fixture retains all simulation setup and native field comparisons.
pub(crate) type GroundCommandProducer = fn(
    &Simulation,
    &RuleSet,
    crate::sim::intern::InternedId,
    u64,
    (u16, u16),
    bool,
) -> CommandEnvelope;

fn simulation_ground_command(
    sim: &Simulation,
    rules: &RuleSet,
    owner: crate::sim::intern::InternedId,
    id: u64,
    clicked: (u16, u16),
    _shift: bool,
) -> CommandEnvelope {
    let cell = sim
        .ordinary_ground_foot_cell_input(owner, id, clicked, rules)
        .unwrap()
        .unwrap()
        .unwrap();
    CommandEnvelope::new(
        owner,
        sim.session.tick,
        Command::Move {
            entity_id: id,
            target_rx: cell.0,
            target_ry: cell.1,
            queue: false,
        },
    )
}

/// Actual Unit query2/Shift on allocated Cell4,20 fails Map578460's
/// height-aware LocalSize gate. Original Foot4DE1D0/FNPC56DC20 chooses4,19
/// before encoding ordinary Move; the Unit setter receives that chosen Cell.
/// Replay the saved header/navigation inputs through the existing owners.
pub(crate) fn check_initialized_unit_shift_resolver(produce: GroundCommandProducer) {
    let corpus = ground_command_corpus();
    let row = ground_command_history(&corpus, "unit_shift_outside_playfield_B");
    assert_eq!(row["inputs"][1]["queried_action"], 2);
    assert_eq!(
        row["inputs"][1]["clicked"],
        row["resolver_map_prior"]["requested"]
    );
    let dispatch = row["boundaries"]
        .as_array()
        .unwrap()
        .iter()
        .rfind(|boundary| boundary["label"] == "actual_local_OutList_DoList_dispatch")
        .unwrap();
    assert_ne!(
        ground_command_nav_target(&dispatch["after"]).unwrap(),
        NavTargetRef::cell(
            fixture_cell(&row["inputs"][1]["clicked"]).0,
            fixture_cell(&row["inputs"][1]["clicked"]).1,
        )
    );
    check_initialized_ground_reissues(&["unit_shift_outside_playfield_B"], produce);
}

pub(crate) fn check_initialized_ground_reissues(names: &[&str], produce: GroundCommandProducer) {
    use crate::sim::mission::MissionId;
    use crate::sim::mission::authority::EntityReadyInputProvider;

    let corpus = ground_command_corpus();
    for &name in names {
        let row = ground_command_history(&corpus, name);
        let Some((mut sim, rules, registry, id)) = ground_command_fixture(row, &row["initial"])
        else {
            return;
        };
        let owner = sim.interner.get("Americans").unwrap();
        let dispatches: Vec<_> = row["boundaries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|boundary| boundary["label"] == "actual_local_OutList_DoList_dispatch")
            .collect();
        assert_eq!(dispatches.len(), 2);
        for (index, (input, dispatch)) in row["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .zip(dispatches)
            .enumerate()
        {
            if name == "attack_same_A" && index == 1 {
                let producer = row["boundaries"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|boundary| boundary["label"] == "actual_QueueMission_Attack")
                    .unwrap();
                sim.mission_queue_exact(
                    id,
                    MissionId::from_raw(producer["args"][0].as_i64().unwrap().try_into().unwrap()),
                    producer["args"][1].as_i64().unwrap().try_into().unwrap(),
                    sim.session.binary_frame,
                    &EntityReadyInputProvider,
                )
                .unwrap();
                assert_ground_command_fields(&sim, id, row, &producer["after"]);
            }
            if name == "prone_A_A" && index == 1 {
                let down = row["boundaries"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|boundary| boundary["label"] == "actual_GI_DoAction_Down")
                    .unwrap();
                assert_eq!(down["args"], json!([5, 1, 0]));
                let _ = sim.infantry_do_action(id, 5, true, &rules).unwrap();
                assert_ground_command_fields(&sim, id, row, &down["after"]);
            }
            sim.session.binary_frame = dispatch["before"]["frame"]
                .as_u64()
                .unwrap()
                .try_into()
                .unwrap();
            let clicked = fixture_cell(&input["clicked"]);
            let shift = input["keys"]
                .as_array()
                .unwrap()
                .iter()
                .any(|key| key["key"] == 16 && key["short"].as_u64().unwrap() & 0x8000 != 0);
            let decoded = produce(&sim, &rules, owner, id, clicked, shift);
            let Command::Move {
                entity_id,
                target_rx,
                target_ry,
                queue,
            } = decoded.payload
            else {
                panic!("{name}: original input emits an ordinary Move");
            };
            assert_eq!(entity_id, id);
            assert!(!queue, "{name}: native Shift still emits ordinary Move");
            let expected = ground_command_nav_target(&dispatch["after"]).unwrap();
            assert_eq!(NavTargetRef::cell(target_rx, target_ry), expected);
            assert!(sim.apply_command_with_overlays(
                "Americans",
                &decoded.payload,
                Some(&rules),
                Some(&registry)
            ));
            assert_ground_command_fields(&sim, id, row, &dispatch["after"]);
        }
    }
}

/// Original UnitAI7360C0 produces these paid priors. Import its returned
/// boundary, then compare the actual Event6 Stop and Move command returns.
/// The next original AI is saved in the receipt; this test does not replay
/// its map/background/facing inputs as a whole-world equivalence claim.
pub(crate) fn check_initialized_unit_paid_reissues(produce: GroundCommandProducer) {
    let corpus = ground_command_corpus();
    for name in [
        "unit_paid_head_B",
        "unit_paid_head_Stop_B",
        "unit_paid_head_same_A",
    ] {
        let row = ground_command_history(&corpus, name);
        let boundaries = row["boundaries"].as_array().unwrap();
        let stop = boundaries
            .iter()
            .find(|boundary| boundary["label"] == "actual_StopEvent_execute");
        let click = boundaries
            .iter()
            .rfind(|boundary| boundary["label"] == "actual_Unit_CellClick")
            .unwrap();
        let prior = &stop.unwrap_or(click)["before"];
        assert!(
            ground_command_optional_coordinate(&prior["drive"]["head"]).is_some(),
            "{name}: original AI produced the paid head"
        );
        let Some((mut sim, rules, registry, id)) = ground_command_fixture(row, prior) else {
            return;
        };
        if let Some(stop) = stop {
            assert!(sim.apply_command_with_overlays(
                "Americans",
                &Command::Stop { entity_id: id },
                Some(&rules),
                Some(&registry)
            ));
            assert_ground_command_fields(&sim, id, row, &stop["after"]);
        }
        let input = row["inputs"].as_array().unwrap().last().unwrap();
        let owner = sim.interner.get("Americans").unwrap();
        let decoded = produce(
            &sim,
            &rules,
            owner,
            id,
            fixture_cell(&input["clicked"]),
            false,
        );
        assert!(matches!(
            decoded.payload,
            Command::Move { queue: false, .. }
        ));
        assert!(sim.apply_command_with_overlays(
            "Americans",
            &decoded.payload,
            Some(&rules),
            Some(&registry)
        ));
        let dispatch = boundaries
            .iter()
            .rfind(|boundary| boundary["label"] == "actual_local_OutList_DoList_dispatch")
            .unwrap();
        assert_ground_command_fields(&sim, id, row, &dispatch["after"]);
    }
}

/// The original input emitted this Move before the explicit Unit6E0 prior
/// changed. Decoded commands retain the Event prefix and reach the void
/// Unit setter's refusal without re-running physical input admission.
#[test]
fn initialized_unit_delayed_move_reaches_original_class_refusal() {
    let corpus = ground_command_corpus();
    let row = ground_command_history(&corpus, "unit_deployed_delayed_A");
    let dispatch = row["boundaries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|boundary| boundary["label"] == "actual_local_OutList_DoList_dispatch")
        .unwrap();
    let Some((mut sim, rules, registry, id)) = ground_command_fixture(row, &dispatch["before"])
    else {
        return;
    };
    let class = row["ordered_calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|call| call["kind"] == "Unit_SetDestination")
        .unwrap();
    assert_eq!(class["args"][1], 1);
    assert_eq!(class["true_ret8_bytes"], "c20800");
    let NavTargetRef::Cell { rx, ry } =
        ground_command_cell_reference(&class["before"], class["args"][0].as_u64().unwrap())
            .unwrap()
    else {
        panic!("original emitted Move destination is a Cell");
    };
    assert!(
        sim.apply_command_with_overlays(
            "Americans",
            &Command::Move {
                entity_id: id,
                target_rx: rx,
                target_ry: ry,
                queue: false
            },
            Some(&rules),
            Some(&registry)
        ),
        "the void class refusal is still an executed Move"
    );
    assert_ground_command_fields(&sim, id, row, &dispatch["after"]);
}

/// An actual70C610(Cell) call supplies the retained Archive prior. Real
/// ordinary input/Event then clears it at4C7448 before the class destination.
/// This is a free-GI component prior, not replay of the early factory rally.
#[test]
fn initialized_gi_move_clears_original_archive_before_destination() {
    let corpus = ground_command_corpus();
    let row = ground_command_history(&corpus, "archive_A_B");
    let click = row["boundaries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|boundary| boundary["label"] == "actual_GI_CellClick")
        .unwrap();
    let Some((mut sim, rules, registry, id)) = ground_command_fixture(row, &click["before"]) else {
        return;
    };
    assert!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .archive_target()
            .is_some()
    );
    let owner = sim.interner.get("Americans").unwrap();
    let input = row["inputs"].as_array().unwrap().last().unwrap();
    let resolved = sim
        .ordinary_ground_foot_cell_input(owner, id, fixture_cell(&input["clicked"]), &rules)
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(sim.apply_command_with_overlays(
        "Americans",
        &Command::Move {
            entity_id: id,
            target_rx: resolved.0,
            target_ry: resolved.1,
            queue: false
        },
        Some(&rules),
        Some(&registry)
    ));
    assert_ground_command_fields(&sim, id, row, &row["final"]);
}

/// The preceding whole original51BAB0 visit owns the actual paid head and
/// RNG advances. Import that boundary for paid-head controls; the separate
/// prone control executes the existing DoAction owner against original Down5.
/// The raw-word contrast imports an explicit component prior. Stop is the
/// class51DAF0 method boundary, not keyboard/event Stop ingress.
#[test]
fn initialized_gi_retained_priors_and_stop_reissues_match_original_returns() {
    let corpus = ground_command_corpus();
    for name in [
        "paid_head_B",
        "paid_head_Stop_B",
        "prone_B",
        "current_cell_raw20_B",
    ] {
        let row = ground_command_history(&corpus, name);
        let boundaries = row["boundaries"].as_array().unwrap();
        let prior = if name == "prone_B" {
            &row["initial"]
        } else if name == "current_cell_raw20_B" {
            &boundaries
                .iter()
                .rfind(|boundary| boundary["label"] == "actual_GI_CellClick")
                .unwrap()["before"]
        } else {
            &boundaries
                .iter()
                .find(|boundary| boundary["label"] == "whole_original_GI_AI_1")
                .unwrap()["after"]
        };
        let Some((mut sim, rules, registry, id)) = ground_command_fixture(row, prior) else {
            return;
        };
        if name == "prone_B" {
            let down = boundaries
                .iter()
                .find(|boundary| boundary["label"] == "actual_GI_DoAction_Down")
                .unwrap();
            assert_eq!(down["args"], json!([5, 1, 0]));
            let _ = sim.infantry_do_action(id, 5, true, &rules).unwrap();
            assert_ground_command_fields(&sim, id, row, &down["after"]);
        }
        if name == "paid_head_Stop_B" {
            let stop = boundaries
                .iter()
                .find(|boundary| boundary["label"] == "actual_GI_Stop")
                .unwrap();
            sim.infantry_stop_driver(id, &rules, Some(&registry))
                .unwrap();
            assert_ground_command_fields(&sim, id, row, &stop["after"]);
        }
        let input = row["inputs"].as_array().unwrap().last().unwrap();
        let owner = sim.interner.get("Americans").unwrap();
        let resolved = sim
            .ordinary_ground_foot_cell_input(owner, id, fixture_cell(&input["clicked"]), &rules)
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(sim.apply_command_with_overlays(
            "Americans",
            &Command::Move {
                entity_id: id,
                target_rx: resolved.0,
                target_ry: resolved.1,
                queue: false
            },
            Some(&rules),
            Some(&registry)
        ));
        assert_ground_command_fields(&sim, id, row, &row["final"]);
    }
}

/// Actual4D3CC7/4D3DF9 redirects from the initialized original GI. Compare
/// the real target CanEnter, FNPC selection and class return. The subsequent
/// AStar path and the unrelated factory map objects are not replayed here.
#[test]
fn initialized_gi_findpath_redirects_match_original_class_returns() {
    use crate::sim::movement::infantry_entry::InfantryEntryArgs;

    let corpus = ground_command_corpus();
    for name in ["findpath_existing_idle_MTNK", "findpath_existing_GAPILE"] {
        let row = ground_command_history(&corpus, name);
        let calls = row["ordered_calls"].as_array().unwrap();
        let class = calls
            .iter()
            .find(|call| call["kind"] == "GI_SetDestination")
            .unwrap();
        assert_eq!(class["args"][1], 1);
        assert_eq!(class["true_ret8_bytes"], "c20800");
        let Some((mut sim, rules, registry, id)) = ground_command_fixture(row, &class["before"])
        else {
            return;
        };
        let target = calls
            .iter()
            .find(|call| call["kind"] == "existing_original_redirect_occupant")
            .unwrap();
        let xy = fixture_cell(&target["cell"]["xy"]);
        let cell = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_identity((xy.0.try_into().unwrap(), xy.1.try_into().unwrap()));
        let answer = sim
            .foot_can_enter(id, cell, InfantryEntryArgs::REPAIR, &rules, Some(&registry))
            .unwrap();
        let native_entry = calls
            .iter()
            .find(|call| call["kind"] == "GI_CanEnter")
            .unwrap();
        assert_eq!(
            u64::from(answer),
            native_entry["returned_eax"].as_u64().unwrap()
        );
        let goal = sim
            .find_path_goal_for_answer(
                id,
                crate::sim::components::DriveCoord::cell(xy.0, xy.1, 0),
                answer,
                &rules,
                Some(&registry),
            )
            .unwrap();
        let native_near = calls
            .iter()
            .find(|call| call["kind"] == "Map_FindNearbyPassable")
            .unwrap();
        let near = fixture_cell(&native_near["chosen_cell"]);
        assert_eq!(
            goal,
            crate::sim::components::DriveCoord::cell(near.0, near.1, 0)
        );
        assert_ground_command_fields(&sim, id, row, &class["at_ret8"]);
    }
}

/// Original input emits no Move for the recorded local-house, Berserk and
/// Deployer/Doing contexts. Action0 can return CellClick AL1 without issuing
/// an order; the latter's query2 has a different refusal path.51AA40 owns its
/// separate later human-class guard.
#[test]
fn initialized_gi_cell_input_no_order_contexts_match_original_state() {
    check_initialized_no_order_contexts(&[
        "human_Doing27_B",
        "human_Doing27_shift_B",
        "mode5_current_mismatch_B",
        "mode5_current_nonhuman_Doing27_input_class_B",
        "berserk_B",
    ]);
}

/// Unit query7404B0 and click738910 have their own pre-Event guards.
/// Explicit native byte priors exercise them without claiming that MTNK's
/// retail type can itself produce deployment transitions.
#[test]
fn initialized_unit_cell_input_no_order_contexts_match_original_state() {
    check_initialized_no_order_contexts(&[
        "unit_owner_mismatch_B",
        "unit_berserk_B",
        "unit_deployed_B",
        "unit_deploying_B",
        "unit_undeploying_B",
    ]);
}

fn check_initialized_no_order_contexts(names: &[&str]) {
    let corpus = ground_command_corpus();
    for &name in names {
        let row = ground_command_history(&corpus, name);
        let unit = row["initial"].get("drive").is_some();
        let click = row["boundaries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|boundary| {
                boundary["label"]
                    == if unit {
                        "actual_Unit_CellClick"
                    } else {
                        "actual_GI_CellClick"
                    }
            })
            .unwrap();
        let Some((sim, rules, _, id)) = ground_command_fixture(row, &click["before"]) else {
            return;
        };
        if !unit {
            assert!(
                rules.object("E1").unwrap().deployer,
                "original layered E1 Deployer=yes"
            );
        }
        if name.contains("Doing27") {
            assert_eq!(row["inputs"][0]["queried_action"], 2);
        }
        assert!(
            !row["ordered_calls"]
                .as_array()
                .unwrap()
                .iter()
                .any(|call| call["kind"] == "Event_Construct")
        );
        let owner = sim.interner.get("Americans").unwrap();
        assert_eq!(
            sim.ordinary_ground_foot_cell_input(
                owner,
                id,
                fixture_cell(&row["inputs"][0]["clicked"]),
                &rules
            )
            .unwrap()
            .unwrap(),
            None,
            "{name}: original input emits no Move event"
        );
        assert_ground_command_fields(&sim, id, row, &click["after"]);
    }
}

/// Original Type+EC8=0 is a component contrast after retail initialization.
/// With Doing27, the input query can emit Move while the independent human
/// destination-class guard refuses. Compare the actual Event/class outcome.
#[test]
fn initialized_gi_non_deployer_input_reaches_original_class_refusal() {
    let corpus = ground_command_corpus();
    let row = ground_command_history(&corpus, "nonDeployer_Doing27_B");
    let click = row["boundaries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|boundary| boundary["label"] == "actual_GI_CellClick")
        .unwrap();
    let Some((mut sim, rules, registry, id)) = ground_command_fixture(row, &click["before"]) else {
        return;
    };
    assert!(!rules.object("E1").unwrap().deployer);
    let owner = sim.interner.get("Americans").unwrap();
    let resolved = sim
        .ordinary_ground_foot_cell_input(
            owner,
            id,
            fixture_cell(&row["inputs"][0]["clicked"]),
            &rules,
        )
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(sim.apply_command_with_overlays(
        "Americans",
        &Command::Move {
            entity_id: id,
            target_rx: resolved.0,
            target_ry: resolved.1,
            queue: false,
        },
        Some(&rules),
        Some(&registry),
    ));
    assert_ground_command_fields(&sim, id, row, &row["final"]);
}

/// The same original DoAction-produced prior at the class boundary has no
/// Event prelude. House50B730 decides the class's independent human guard.
/// The setter is void; compare true-RET8 state rather than incidental EAX.
#[test]
fn initialized_gi_deploy_class_contexts_match_original_returns() {
    let corpus = ground_command_corpus();
    for name in [
        "human_Doing27_class_B",
        "mode5_current_nonhuman_Doing27_input_class_B",
    ] {
        let row = ground_command_history(&corpus, name);
        let call = row["ordered_calls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|call| call["kind"] == "GI_SetDestination")
            .unwrap();
        let Some((mut sim, rules, registry, id)) = ground_command_fixture(row, &call["before"])
        else {
            return;
        };
        assert_eq!(call["args"][1], 1);
        assert_eq!(call["true_ret8_bytes"], "c20800");
        let requested =
            ground_command_cell_reference(&call["before"], call["args"][0].as_u64().unwrap())
                .unwrap();
        let _ = sim
            .set_infantry_destination(id, requested, &rules, Some(&registry))
            .unwrap();
        assert_ground_command_fields(&sim, id, row, &call["after"]);
    }
}

pub(super) fn corpus() -> Value {
    let data: Value = serde_json::from_str(crate::test_fixture::text(
        "src/sim/world/fixtures/factory_infantry_local_native.json",
    ))
    .unwrap();
    assert_eq!(data["schema_version"], 2);
    assert_eq!(
        data["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    assert_eq!(data["products"].as_array().unwrap().len(), 2);
    assert_eq!(data["rally"]["products"].as_array().unwrap().len(), 2);
    data
}

fn fixture_cell(value: &Value) -> (u16, u16) {
    (
        value[0].as_u64().unwrap().try_into().unwrap(),
        value[1].as_u64().unwrap().try_into().unwrap(),
    )
}

fn assert_native_fields(actual: &Value, expected: &Value, context: &str) {
    for (field, value) in expected.as_object().unwrap() {
        assert_eq!(
            &actual[field], value,
            "{context}: native local field {field}; actual state={actual}; expected state={expected}"
        );
    }
}

/// Supplied clear33x33/diamond16 map from the native fixture's local ground
/// prior. The physical map/theater startup is outside this test; path/zone,
/// terrain-cost and occupation updates still use their production owners.
pub(super) fn install_ground(
    sim: &mut Simulation,
    rules: &RuleSet,
    terrain_rules: &TerrainRules,
    observed_map: Option<&Value>,
) {
    let size = observed_map.map_or((16, 16), |map| {
        (
            map["map_size"][0].as_i64().unwrap() as i32,
            map["map_size"][1].as_i64().unwrap() as i32,
        )
    });
    let side = observed_map.map_or(33, |_| u16::try_from(size.0 + size.1).unwrap());
    let clear = terrain_rules.semantics_for_land_type(0).unwrap();
    let cells = (0..side)
        .flat_map(|y| {
            (0..side).map(move |x| {
                let mut cell = super::lifecycle_tests::common_raw_terrain_cell(x, y, 0, false);
                cell.terrain_class = clear.terrain_class;
                cell.base_terrain_class = clear.terrain_class;
                cell.speed_costs = clear.speed_costs;
                cell.base_speed_costs = clear.speed_costs;
                // Existing GI/factory controls retain their supplied border.
                // The joined Unit resolver exports its exact class plane.
                let outside = observed_map.map_or(
                    x == 0 || x == side - 1 || y == 0 || y == side - 1,
                    |map| {
                        let index = usize::from(y) * usize::from(side) + usize::from(x);
                        let class = map["navigation"]["classes"][index].as_u64().unwrap();
                        assert!(matches!(class, 0 | 7), "supplied clear/outside domain");
                        assert_eq!(map["navigation"]["levels"][index], 0);
                        class == 7
                    },
                );
                if outside {
                    cell.outside_playfield = true;
                    cell.zone_type = 7;
                    cell.ground_walk_blocked = true;
                }
                cell
            })
        })
        .collect();
    let terrain = ResolvedTerrainGrid::from_cells(side, side, cells);
    let local = observed_map.map_or([0, 0, 16, 16], |map| {
        std::array::from_fn(|index| map["local_size"][index].as_i64().unwrap() as i32)
    });
    sim.playfield_bounds = Some(
        crate::map::playfield::PlayfieldBounds::from_normalized_local_size(
            size.0, local[0], local[1], local[2], local[3],
        ),
    );
    sim.playfield_size_height = Some(size.1);
    sim.session.map_width = side;
    sim.session.map_height = side;
    sim.overlay_grid = Some(OverlayGrid::new(side, side));
    sim.bridge_state = Some(BridgeRuntimeState::from_resolved_terrain_with_map_size(
        &terrain,
        true,
        rules.bridge_rules.strength,
        size,
    ));
    sim.install_resolved_terrain_for_new_map(terrain);
    assert!(sim.rebuild_dynamic_navigation(rules));
}

fn local_state(sim: &Simulation, id: u64) -> Value {
    let entity = sim.substrate.entities.get(id).unwrap();
    let p = ground_pose::position_world_coord(&entity.position);
    let walk = entity.locomotor.as_ref().unwrap();
    json!({
        "location": [p.x, p.y, p.z],
        "cell": [entity.position.rx, entity.position.ry],
        "health": entity.health.current,
        "mission": entity.mission.current().raw(),
        "queued": entity.mission.queued().raw(),
        "nav_is_set": entity.navigation.nav_com.is_some(),
        "archive_is_set": entity.archive_target().is_some(),
        "tether": entity.dock_entered_with.is_some(),
        "contact_count": entity.radio_contacts.len(),
        "doing": entity.mission_leaf.as_infantry().unwrap().doing(),
        "idle_entry_latch": entity.mission_leaf.foot_idle_entry_latch(),
        "paid_head_is_set": walk.step_head().is_some(),
        "walk_is_moving": walk.walk_is_moving().unwrap(),
        "walk_destination_is_set": walk.walk_destination().is_some(),
        "paid_head": walk.step_head().map(|c| [c.x, c.y, c.z]),
        "walk_destination": walk.walk_destination().map(|c| [c.x, c.y, c.z]),
        "nav_target": entity.navigation.nav_com,
        "archive_target": entity.archive_target(),
        "stage": entity.native_stage().value(),
        "stage_rate": entity.native_stage().rate(),
    })
}

pub(super) fn pair_radio(log: &[TransmitRecord], producer: u64, product: u64) -> Vec<Value> {
    let participant = |id| {
        if id == producer {
            "producer"
        } else {
            assert_eq!(id, product);
            "product"
        }
    };
    log.iter()
        .filter(|e| {
            (e.sender_sid == producer && e.target_sid == product)
                || (e.sender_sid == product && e.target_sid == producer)
        })
        .map(|e| {
            json!([
                participant(e.sender_sid),
                participant(e.target_sid),
                e.msg,
                e.reply.unwrap()
            ])
        })
        .collect()
}

#[derive(Default)]
struct ProductProgress {
    id: u64,
    completed_frame: Option<u32>,
    placed_frame: Option<u32>,
    first_live_frame: Option<u32>,
    release_frame: Option<u32>,
    archive_handoff_frame: Option<u32>,
    settled_frame: Option<u32>,
    changed_position: bool,
    paid_rally_walk: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputRoute {
    NoRally,
    Rally,
    RallyBetweenCompletionAndPlace,
}

#[test]
fn no_rally_two_paid_gi_walk_out_and_release_their_factory_contacts() {
    joined_two_paid_gi(OutputRoute::NoRally);
}

#[test]
fn rally_two_paid_gi_consume_archive_walk_to_rally_and_settle() {
    joined_two_paid_gi(OutputRoute::Rally);
}

/// Building443860 appends SetRally before the next Strip6A8B30 appends PLACE.
/// The already admitted player event must reach the tail first: Exit443C60
/// copies the new Archive to the retained GI instead of its previous value.
#[test]
fn rally_between_factory_completion_and_place_reaches_the_held_gi() {
    joined_two_paid_gi(OutputRoute::RallyBetweenCompletionAndPlace);
}

fn joined_two_paid_gi(route: OutputRoute) {
    let native = corpus();
    let rally_cell =
        (route != OutputRoute::NoRally).then(|| fixture_cell(&native["rally"]["request"]["cell"]));
    let exit_cell = fixture_cell(&native["rally"]["exit_cell"]);
    let native_products = if rally_cell.is_some() {
        &native["rally"]["products"]
    } else {
        &native["products"]
    };
    let Some(retail) = retail_battle_rules_for_map("Hills.mmx") else {
        return;
    };
    let rules = retail.rules;
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let terrain_rules = TerrainRules::from_ini(&retail.processed_rules);
    let mut sim = Simulation::with_seed(2);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    install_ground(&mut sim, &rules, &terrain_rules, None);
    let owner = sim.interner.intern("Americans");
    // Human/currentHouse/difficulty0 and cash10000 are retained native priors.
    // TechLevel10 is explicit structural eligibility, not a measured P2 field.
    let mut house = HouseState::new(owner, 0, Some(owner), true, 10_000, 10);
    house.difficulty = HouseDifficulty::Hard;
    house.project_country_mults(&rules, &sim.interner);
    sim.houses.insert(owner, house);
    sim.session.house_order.push(owner);
    sim.session.current_house = Some(owner);
    sim.session.game_mode_nonzero = true;
    sim.session.game_options.game_speed = 3;
    let producer = sim
        .spawn_object_at_height_with_overlay_registry(
            "GAPILE",
            "Americans",
            14,
            14,
            64,
            0,
            &rules,
            &registry,
        )
        .expect("retail GAPILE enters through its shared constructor/admission");
    sim.advance_tick(&[], Some(&rules), None, Some(&registry), TICK_MS);
    let p =
        ground_pose::position_world_coord(&sim.substrate.entities.get(producer).unwrap().position);
    assert_eq!(json!([p.x, p.y, p.z]), native["producer_prior"]["location"]);
    assert_eq!(
        json!(sim.substrate.entities.get(producer).unwrap().health.current),
        native["producer_prior"]["health"]
    );
    assert_eq!(
        json!(sim.power_states[&owner].total_output),
        native["producer_prior"]["power"]
    );
    assert_eq!(
        json!(sim.power_states[&owner].total_drain),
        native["producer_prior"]["drain"]
    );
    if route == OutputRoute::Rally
        && let Some((rx, ry)) = rally_cell
    {
        let command = CommandEnvelope::new(
            owner,
            sim.session.tick + 1,
            Command::SetRally {
                rx,
                ry,
                producer_ids: vec![producer],
            },
        );
        sim.advance_tick(&[command], Some(&rules), None, Some(&registry), TICK_MS);
        assert_eq!(
            sim.substrate.entities.get(producer).unwrap().rally_cell(),
            rally_cell,
            "the ordinary player event archives the rally before Begin"
        );
    }
    let e1 = sim.interner.get("E1").unwrap();
    let commands = [
        CommandEnvelope::new(
            owner,
            sim.session.tick + 1,
            Command::QueueProduction { type_id: e1 },
        ),
        CommandEnvelope::new(
            owner,
            sim.session.tick + 1,
            Command::QueueProduction { type_id: e1 },
        ),
    ];
    radio::take_transmit_log();
    sim.advance_tick(&commands, Some(&rules), None, Some(&registry), TICK_MS);
    let head = sim
        .production
        .factories
        .view(owner, ProductionCategory::Infantry)
        .expect("real production event starts the first GI");
    let first = head.object.unwrap().entity_id.unwrap();
    assert_eq!(
        head.queue.len(),
        1,
        "the second Begin waits as an unconstructed tail"
    );
    assert_eq!(head.progress, 0);
    let mut products = vec![ProductProgress {
        id: first,
        ..Default::default()
    }];

    for _ in 0..10_000 {
        let frame = sim.session.binary_frame;
        if route == OutputRoute::RallyBetweenCompletionAndPlace
            && products[0].completed_frame.is_some()
            && products[0].placed_frame.is_none()
        {
            let (rx, ry) = rally_cell.unwrap();
            sim.queue_command(CommandEnvelope::new(
                owner,
                sim.session.tick + 1,
                Command::SetRally {
                    rx,
                    ry,
                    producer_ids: vec![producer],
                },
            ));
        }
        // The ordinary app drains player ingress before advancing each frame.
        let due = sim.take_due_commands();
        if route == OutputRoute::RallyBetweenCompletionAndPlace {
            let mut output = sim
                .advance_app_frame(
                    &due,
                    Some(&rules),
                    Some(&registry),
                    TICK_MS,
                    super::TickLane::Ordinary,
                    None,
                )
                .expect("the ordinary app frame completes");
            let admitted = output.take_admitted_commands();
            if products[0].completed_frame.is_some() && products[0].placed_frame.is_none() {
                let phase: Value = serde_json::from_str(crate::test_fixture::text(
                    "src/sim/world/fixtures/factory_infantry_publication_native.json",
                ))
                .unwrap();
                assert_eq!(phase["native_sha256"], native["native_sha256"]);
                let event_types: Vec<_> = admitted
                    .iter()
                    .map(|command| match command.payload {
                        Command::SetRally { .. } => 0x1E,
                        Command::PlaceProducedMobile { .. } => 0x0B,
                        _ => panic!("unexpected admitted race command: {command:?}"),
                    })
                    .collect();
                let before = &phase["cases"]["before_strip"];
                assert_eq!(json!(event_types), before["executed_event_types"]);
                let state = local_state(&sim, first);
                assert_eq!(state["location"], before["placed"]["position"]);
                for field in [
                    "mission",
                    "queued",
                    "archive_is_set",
                    "nav_is_set",
                    "tether",
                ] {
                    assert_eq!(
                        state[field], before["placed"][field],
                        "native phase field {field}"
                    );
                }
                assert_eq!(state["walk_is_moving"], before["placed"]["walk_moving"]);
                assert_eq!(
                    state["walk_destination"],
                    before["placed"]["walk_destination"]
                );
            }
        } else {
            sim.advance_tick(&due, Some(&rules), None, Some(&registry), TICK_MS);
        }
        let log = radio::take_transmit_log();
        if let Some(view) = sim
            .production
            .factories
            .view(owner, ProductionCategory::Infantry)
            && let Some(held) = view.object.and_then(|object| object.entity_id)
        {
            if !products.iter().any(|p| p.id == held) {
                assert_eq!(products.len(), 1, "exactly two identities are constructed");
                assert!(
                    !sim.substrate
                        .entities
                        .get(first)
                        .unwrap()
                        .lifecycle
                        .in_limbo,
                    "the FIFO successor constructs only after the first PLACE admits"
                );
                assert!(sim.substrate.entities.get(held).unwrap().lifecycle.in_limbo);
                assert_eq!(view.progress, 0);
                assert!(view.queue.is_empty());
                products.push(ProductProgress {
                    id: held,
                    ..Default::default()
                });
            }
            let progress = products.iter_mut().find(|p| p.id == held).unwrap();
            if view.complete_object().is_some() && progress.completed_frame.is_none() {
                progress.completed_frame = Some(frame);
                let entity = sim.substrate.entities.get(held).unwrap();
                assert!(entity.lifecycle.in_limbo && !entity.in_logic_vector);
                assert!(
                    !sim.pending_command_snapshot().iter().any(|command| {
                        matches!(command.payload, Command::PlaceProducedMobile { .. })
                    }),
                    "Factory completion retains its change flag until the next Strip prefix"
                );
            }
        }
        for (index, progress) in products.iter_mut().enumerate() {
            let entity = sim.substrate.entities.get(progress.id).unwrap();
            if entity.lifecycle.in_limbo {
                assert!(!entity.in_logic_vector && !entity.lifecycle.cell_marked);
                continue;
            }
            let golden = &native_products[index];
            let state = local_state(&sim, progress.id);
            let context = format!("{route:?} GI[{index}]={} frame={frame}", progress.id);
            assert!(entity.lifecycle.cell_marked && entity.in_logic_vector);
            assert!(sim.substrate.occupancy.contains_entity(
                entity.position.rx,
                entity.position.ry,
                progress.id
            ));
            let transmissions = pair_radio(&log, producer, progress.id);
            if progress.placed_frame.is_none() {
                assert_eq!(frame, progress.completed_frame.unwrap() + 1);
                assert_native_fields(&state, &golden["placed"], &format!("{context} PLACE"));
                assert_eq!(
                    json!(entity.body_facing_byte(frame)),
                    golden["unlimbo_facing"]
                );
                assert_eq!(json!(transmissions), golden["delivery_radio"]);
                assert_eq!(
                    sim.substrate
                        .entities
                        .get(producer)
                        .unwrap()
                        .radio_contacts
                        .slot(0),
                    Some(progress.id)
                );
                assert_eq!(entity.radio_contacts.slot(0), Some(producer));
                if let Some((rx, ry)) = rally_cell {
                    // Exit443C60 copies the producer's Archive at44499C.
                    // Unlimbo Idle consumes it; Exit restores that Nav to
                    // Archive at444CE7 before sending the GI to its exit.
                    assert_eq!(entity.archive_target(), Some(TargetKind::Cell(rx, ry)));
                    assert_eq!(
                        entity.navigation.nav_com,
                        Some(NavTargetRef::cell(exit_cell.0, exit_cell.1))
                    );
                }
                if index == 1 {
                    // Native House4FAC21 and Strip6ABBDF release the final
                    // factory during this PLACE, before any later Strip visit.
                    assert!(
                        sim.production
                            .factories
                            .view(owner, ProductionCategory::Infantry)
                            .is_none()
                    );
                }
                progress.placed_frame = Some(frame);
            } else if progress.first_live_frame.is_none() {
                assert_eq!(frame, progress.placed_frame.unwrap() + 1);
                assert_native_fields(
                    &state,
                    &golden["first_live"],
                    &format!("{context} next live post-Foot output"),
                );
                let walk = entity.locomotor.as_ref().unwrap();
                assert_eq!(walk.active_kind(), LocomotorKind::Walk);
                progress.first_live_frame = Some(frame);
            }
            if state["location"] != golden["placed"]["location"] {
                progress.changed_position = true;
            }
            let release_start = transmissions.iter().position(|row| row[2] == 8);
            if let Some((rx, ry)) = rally_cell
                && progress.release_frame.is_none()
                && release_start.is_none()
            {
                assert_eq!(
                    entity.archive_target(),
                    Some(TargetKind::Cell(rx, ry)),
                    "{context}: rally Archive must survive until actual radio8; state={state}"
                );
                assert_eq!(
                    entity.navigation.nav_com,
                    Some(NavTargetRef::cell(exit_cell.0, exit_cell.1)),
                    "{context}: exit Nav must survive until actual radio8; state={state}"
                );
            }
            if let Some(start) = release_start {
                assert!(
                    progress.release_frame.is_none(),
                    "arrival cleanup happens once"
                );
                assert!(
                    progress.changed_position,
                    "actual paid Walk reaches PerCell"
                );
                assert!(frame > progress.first_live_frame.unwrap());
                assert_eq!(json!(&transmissions[start..]), golden["arrival_radio"]);
                assert!(entity.radio_contacts.is_empty() && entity.dock_entered_with.is_none());
                assert!(
                    sim.substrate
                        .entities
                        .get(producer)
                        .unwrap()
                        .radio_contacts
                        .is_empty()
                );
                progress.release_frame = Some(frame);
                assert_native_fields(
                    &state,
                    &golden["released"],
                    &format!("{context} radio8 release"),
                );
                if let Some((rx, ry)) = rally_cell {
                    // Correct native Walk CRT supplies height104, so paid
                    // completion can Stop before this same visit's Idle
                    // consumes Archive. The historical height0 fixture
                    // skipped that comparison and delayed it a frame.
                    assert!(entity.archive_target().is_none());
                    assert_eq!(entity.navigation.nav_com, Some(NavTargetRef::cell(rx, ry)));
                } else {
                    assert!(
                        entity.archive_target().is_none() && entity.navigation.nav_com.is_none()
                    );
                }
            }
            if let Some((rx, ry)) = rally_cell {
                let walk = entity.locomotor.as_ref().unwrap();
                if progress.release_frame.is_some()
                    && progress.archive_handoff_frame.is_none()
                    && entity.archive_target().is_none()
                {
                    assert!(frame >= progress.release_frame.unwrap());
                    assert_native_fields(
                        &state,
                        &golden["archive_handoff"],
                        &format!("{context} Archive handoff"),
                    );
                    assert_eq!(entity.navigation.nav_com, Some(NavTargetRef::cell(rx, ry)));
                    let destination = walk.walk_destination().unwrap();
                    assert_eq!(
                        json!([destination.x, destination.y, destination.z]),
                        golden["rally_walk_destination"]
                    );
                    assert_eq!(walk.walk_is_moving(), Some(true));
                    assert!(walk.step_head().is_none());
                    progress.archive_handoff_frame = Some(frame);
                }
                if progress
                    .archive_handoff_frame
                    .is_some_and(|handoff| frame > handoff)
                    && entity.navigation.nav_com == Some(NavTargetRef::cell(rx, ry))
                    && walk.step_head().is_some()
                    && state["location"] != golden["archive_handoff"]["location"]
                {
                    progress.paid_rally_walk = true;
                }
            }
            if progress.settled_frame.is_none()
                && progress.release_frame.is_some()
                && rally_cell.is_none_or(|cell| (entity.position.rx, entity.position.ry) == cell)
                && entity.navigation.nav_com.is_none()
                && entity.archive_target().is_none()
                && entity.locomotor.as_ref().unwrap().walk_is_moving() == Some(false)
                && json!(entity.mission.current().raw()) == golden["terminal"]["mission"]
            {
                if rally_cell.is_some() {
                    assert!(
                        progress.archive_handoff_frame.is_some() && progress.paid_rally_walk,
                        "{context}: terminal rally state requires actual handoff/paid Walk; state={state}"
                    );
                }
                // Both products' Guard/cleared Nav, Archive, Walk and radio
                // states are selected from corrected original execution.
                // No-rally Scatter coordinates are RNG-dependent, so only
                // rally asserts the actual player-requested destination cell.
                assert_native_fields(
                    &state,
                    &golden["terminal"],
                    &format!("{context} terminal output"),
                );
                progress.settled_frame = Some(frame);
            }
        }
        if products.len() == 2 && products.iter().all(|p| p.settled_frame.is_some()) {
            break;
        }
    }
    assert_eq!(products.len(), 2);
    assert!(
        products
            .iter()
            .all(|p| p.changed_position && p.release_frame.is_some() && p.settled_frame.is_some()),
        "both paid GI must naturally walk to automatic PerCell/radio cleanup and terminal Guard"
    );
    assert!(products[0].release_frame.unwrap() < products[1].placed_frame.unwrap());
    if rally_cell.is_some() {
        assert!(
            products.iter().all(|p| p.archive_handoff_frame.is_some()
                && p.paid_rally_walk
                && p.settled_frame.is_some()),
            "both GI must consume Archive, walk to the requested rally and settle"
        );
        assert_eq!(
            sim.substrate.entities.get(producer).unwrap().rally_cell(),
            rally_cell,
            "each product consumes its own Archive while the producer retains its rally"
        );
    }
    let factory = sim
        .production
        .factories
        .view(owner, ProductionCategory::Infantry);
    assert!(factory.is_none_or(|view| view.object.is_none() && view.queue.is_empty()));
    assert!(sim.pending_command_snapshot().is_empty());
    assert_eq!(
        json!(sim.houses[&owner].economy.credits()),
        native["final_wallet"]["credits"]
    );
    assert_eq!(
        json!(sim.houses[&owner].economy.spent_credits()),
        native["final_wallet"]["spent"]
    );
}
