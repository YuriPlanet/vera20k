//! Bounded original Unit737BA0/Object5F4EC0 admission and factory443C60
//! comparisons. The oracle executes the original class receivers; these tests
//! use the shared Rust Unlimbo and production-delivery owners. Supplied bridge
//! flags, caller pose and raw bytes are branch controls, not a bridge-loader
//! comparison. The composed factory/Door comparisons live in
//! techno_ai/building_missions/factory_unload_tests. Whole BuildingAI, legal
//! naval production, AStar continuation and the Scenario-inactive prerequisite
//! remain outside the compared boundaries.

use super::{PlacementEvidence, Simulation, entry_test_fixture};
use crate::map::entities::parse_map_entities;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainGrid};
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::retail_ini_fixture::retail_rules_and_art;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::TerrainRules;
use crate::sim::components::DriveCoord;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::timer::MissionDispatchTimer;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::ground_pose;
use crate::sim::movement::infantry_entry::InfantryEntryArgs;
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
use crate::sim::pathfinding::PathGrid;
use crate::sim::production::{self, ProductionCategory};
use crate::sim::radio::{self, RadioMessage, RadioPayload};
use crate::sim::rng::SimRng;
use crate::sim::stage::StageClass;
use crate::sim::timer::CdTimer;
use serde_json::{Value, json};

const NATIVE_SHA256: &str = "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c";

fn corpus() -> Value {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/anytown_damage/unit_unlimbo.json",
    ))
    .unwrap();
    assert_eq!(corpus["schema_version"], 1);
    assert_eq!(corpus["native_sha256"], NATIVE_SHA256);
    assert_eq!(corpus["direct_rows"].as_array().unwrap().len(), 9);
    assert_eq!(corpus["factory_rows"].as_array().unwrap().len(), 8);
    corpus
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().expect("native signed scalar")).unwrap()
}

fn coord(value: &Value) -> DriveCoord {
    DriveCoord {
        x: int(&value[0]),
        y: int(&value[1]),
        z: int(&value[2]),
    }
}

fn in_scopes<R>(
    sim: &mut Simulation,
    depth: u32,
    operation: &mut impl FnMut(&mut Simulation) -> R,
) -> R {
    if depth == 0 {
        operation(sim)
    } else {
        sim.with_object_placement_scope(|sim| in_scopes(sim, depth - 1, operation))
    }
}

fn retail_rules() -> Option<(RuleSet, OverlayTypeRegistry, TerrainRules)> {
    let (ini, art) = retail_rules_and_art()?;
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    let overlays = OverlayTypeRegistry::from_ini(&ini, Some(&art));
    Some((rules, overlays, TerrainRules::from_ini(&ini)))
}

/// The compared target has physical level4/slope0/Road1 in the native crop.
/// The surrounding allocated cells and playable bounds are fixture inputs:
/// this exercises admission/Mark, not the full native map-load hierarchy.
fn retail_world(rules: &RuleSet, terrain_rules: &TerrainRules) -> Simulation {
    let mut sim = Simulation::with_seed(0);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let road = terrain_rules.semantics_for_land_type(1).unwrap();
    let cells = (0..128)
        .flat_map(|ry| {
            (0..128).map(move |rx| {
                let mut cell = super::lifecycle_tests::common_raw_terrain_cell(rx, ry, 4, false);
                cell.land_type = 1;
                cell.yr_cell_land_type = 1;
                cell.terrain_class = road.terrain_class;
                cell.base_terrain_class = road.terrain_class;
                cell.speed_costs = road.speed_costs.clone();
                cell.base_speed_costs = road.speed_costs.clone();
                cell.final_tile_index = 294;
                cell.final_sub_tile = 1;
                cell
            })
        })
        .collect();
    let terrain = ResolvedTerrainGrid::from_cells(128, 128, cells);
    let path = PathGrid::from_resolved_terrain(&terrain);
    sim.install_fixture_path_grid(Some(&path));
    sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -128,
        off_100: -128,
        off_104: 256,
        off_108: 256,
    });
    sim.session.map_width = 128;
    sim.session.map_height = 128;
    sim.install_resolved_terrain_for_new_map(terrain);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 50_000, 10));
    sim
}

fn apply_cell_inputs(sim: &mut Simulation, cell: (u16, u16), input: &Value) {
    let terrain = sim.resolved_terrain.as_mut().unwrap();
    let target = terrain.cell_mut(cell.0, cell.1).unwrap();
    let has_bridge = input["bridge"].as_bool().unwrap_or(false);
    target.bridge_facts.raw_flags = if has_bridge { 0x100 } else { 0 };
    target.has_bridge_deck = has_bridge;
    target.bridge_walkable = has_bridge;
    target.bridge_deck_level = if has_bridge {
        target.level + 4
    } else {
        target.level
    };
    sim.substrate.raw_cell_occupation.mark_ground(
        cell.0,
        cell.1,
        input["ground_bits"].as_u64().unwrap() as u8,
    );
    sim.substrate.raw_cell_occupation.mark_deck(
        cell.0,
        cell.1,
        input["deck_bits"].as_u64().unwrap() as u8,
    );
    sim.session.game_mode_nonzero = int(&input["actual_game_mode"]) != 0;
}

fn assert_actor(sim: &Simulation, id: u64, native: &Value, name: &str) {
    let actor = sim.substrate.entities.get(id).unwrap();
    let actual = ground_pose::position_world_coord(&actor.position);
    assert_eq!(
        json!([actual.x, actual.y, actual.z]),
        native["position"],
        "{name}: Location"
    );
    for (label, actual) in [
        ("alive", u8::from(actor.lifecycle.object_alive)),
        ("limbo", u8::from(actor.lifecycle.in_limbo)),
        ("marked", u8::from(actor.lifecycle.cell_marked)),
        ("logic_registered", u8::from(actor.in_logic_vector)),
    ] {
        assert_eq!(json!(actual), native[label], "{name}: {label}");
    }
    assert_eq!(
        json!(actor.health.current),
        native["health"],
        "{name}: Health"
    );
    assert_eq!(
        json!(actor.mission.current().raw()),
        native["mission"],
        "{name}: Mission"
    );
    assert_eq!(
        json!(actor.mission.queued().raw()),
        native["queued"],
        "{name}: QueueMission"
    );
    assert_eq!(
        json!(actor.techno_ctor_random_word),
        native["random_phase_raw_u16"],
        "{name}: retained constructor phase"
    );
    assert_eq!(
        json!([
            actor.body_facing.destination(),
            actor.body_facing.start_word()
        ]),
        native["primary_facing"],
        "{name}: body facing words"
    );
    let turret = actor
        .barrel_facing
        .as_ref()
        .expect("retail MTNK turret owner");
    assert_eq!(
        json!([turret.destination(), turret.start_word()]),
        native["turret_facing"],
        "{name}: turret facing words"
    );
}

fn assert_stage(sim: &Simulation, id: u64, native: &Value, name: &str) {
    let retained =
        serde_json::to_value(sim.substrate.entities.get(id).unwrap().native_stage()).unwrap();
    for (label, actual, expected) in [
        ("value", &retained["value"], &native["value"]),
        ("changed", &retained["changed"], &native["changed"]),
        (
            "start",
            &retained["timer"]["start_frame"],
            &native["timer"]["start"],
        ),
        (
            "duration",
            &retained["timer"]["duration"],
            &native["timer"]["duration"],
        ),
        ("rate", &retained["rate"], &native["rate"]),
        ("increment", &retained["increment"], &native["increment"]),
    ] {
        assert_eq!(actual, expected, "{name}: retained Unit Stage {label}");
    }
    // Native +104 is copied stack padding, not a StageClass timer owner.
}

fn assert_rng(sim: &Simulation, native: &Value, boundary: &str, name: &str) {
    for (stream, rng) in [
        ("main", &sim.main_rng),
        ("scenario", &sim.scenario_rng),
        ("mapgen", &sim.mapgen_rng),
    ] {
        // Avoid printing all 250 table words on a failed comparison.
        assert!(
            rng.native_state_hex() == native[stream][boundary].as_str().unwrap(),
            "{name}: {stream} {boundary} complete native RNG"
        );
    }
}

fn assert_planes(
    sim: &Simulation,
    id: u64,
    cell: (u16, u16),
    raw: &Value,
    lists: &Value,
    name: &str,
) {
    for (index, layer, bits) in [
        (
            0,
            MovementLayer::Ground,
            sim.substrate
                .raw_cell_occupation
                .ground_bits(cell.0, cell.1),
        ),
        (
            1,
            MovementLayer::Bridge,
            sim.substrate.raw_cell_occupation.deck_bits(cell.0, cell.1),
        ),
    ] {
        assert_eq!(json!(bits), raw[index], "{name}: {layer:?} raw byte");
        let members = sim
            .substrate
            .occupancy
            .get(cell.0, cell.1)
            .map_or_else(Vec::new, |list| list.snapshot_layer(layer));
        let expected = if lists[index].as_str().unwrap() == "0x0" {
            Vec::new()
        } else {
            vec![id]
        };
        assert_eq!(members, expected, "{name}: {layer:?} object list");
    }
    // Native retained infantry-owner words are independent from the vehicle
    // bits. No Infantry writes or owner-producer lifecycle is compared here.
}

fn radio_role(state: &Value, pointer: &str) -> Option<&'static str> {
    if pointer == "0x0" {
        None
    } else if pointer == state["unit"]["address"].as_str().unwrap() {
        Some("unit")
    } else if pointer == state["producer"]["address"].as_str().unwrap() {
        Some("producer")
    } else {
        panic!("unmapped original radio endpoint {pointer}");
    }
}

fn assert_radio_pair(sim: &Simulation, ids: (u64, u64), native: &Value, name: &str) {
    assert_actor(sim, ids.0, &native["actor"], name);
    for (role, id, peer) in [("unit", ids.0, ids.1), ("producer", ids.1, ids.0)] {
        let endpoint = sim.substrate.entities.get(id).unwrap();
        let expected = &native[role];
        let position = ground_pose::position_world_coord(&endpoint.position);
        assert_eq!(
            json!([position.x, position.y, position.z]),
            expected["position"],
            "{name}: {role} Location"
        );
        assert_eq!(
            json!(endpoint.radio_contacts.capacity()),
            expected["contact_capacity"],
            "{name}: {role} sparse contact capacity"
        );
        let actual_contacts: Vec<_> = (0..endpoint.radio_contacts.capacity())
            .map(|slot| {
                endpoint.radio_contacts.slot(slot).map(|target| {
                    if target == ids.0 {
                        "unit"
                    } else {
                        assert_eq!(target, ids.1, "{name}: retained radio endpoint");
                        "producer"
                    }
                })
            })
            .collect();
        let expected_contacts: Vec<_> = expected["contacts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pointer| radio_role(native, pointer.as_str().unwrap()))
            .collect();
        assert_eq!(
            actual_contacts, expected_contacts,
            "{name}: {role} Contacts"
        );
        assert_eq!(
            endpoint.dock_entered_with,
            (expected["tether"] == 1).then_some(peer),
            "{name}: {role} tether owner"
        );
        for (label, actual) in [
            ("alive", i32::from(endpoint.lifecycle.object_alive)),
            ("limbo", i32::from(endpoint.lifecycle.in_limbo)),
            ("marked", i32::from(endpoint.lifecycle.cell_marked)),
            ("in_playfield", i32::from(endpoint.in_playfield)),
            ("health", endpoint.health.current),
            ("mission", endpoint.mission.current().raw()),
            ("queued", endpoint.mission.queued().raw()),
        ] {
            assert_eq!(json!(actual), expected[label], "{name}: {role} {label}");
        }
        assert_eq!(
            json!([
                endpoint.mission.dispatch_timer().start_frame(),
                endpoint.mission.dispatch_timer().delay()
            ]),
            expected["dispatch"],
            "{name}: {role} Mission dispatch timer"
        );
        assert_eq!(
            json!([
                endpoint.body_facing.destination(),
                endpoint.body_facing.start_word()
            ]),
            expected["primary_facing"],
            "{name}: {role} retained body facing"
        );
    }
}

fn assert_radio_transmits(native: &Value, ids: (u64, u64), name: &str) {
    let actual: Vec<_> = radio::take_transmit_log()
        .into_iter()
        .map(|record| {
            let role = |id| {
                if id == ids.0 {
                    "unit"
                } else {
                    assert_eq!(id, ids.1, "{name}: transmitted endpoint");
                    "producer"
                }
            };
            (
                role(record.sender_sid),
                record.msg,
                role(record.target_sid),
                record.reply.unwrap(),
            )
        })
        .collect();
    let before = &native["before"];
    let mut null_attempts = 0;
    let expected: Vec<_> = native["trace"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["kind"] == "transmit")
        .filter_map(|entry| {
            let sender = radio_role(before, entry["this"].as_str().unwrap()).unwrap();
            let target = entry["args"][2].as_u64().unwrap();
            let receiver = if target == 0 {
                assert_eq!(native["route"], "null_sender");
                // Original65A970 accepts a null target and resolves Contact0.
                // These fixed controls either retain the same single contact
                // until BREAK or have none for both sends; no changing-slot
                // broadcast or arbitrary fallback sequence is certified here.
                radio_role(before, before[sender]["contacts"][0].as_str().unwrap())
            } else {
                radio_role(before, &format!("0x{target:x}"))
            };
            let Some(receiver) = receiver else {
                // The native trace includes two null-target attempts replying
                // zero. Rust's contact owner skips dispatch for an empty slot;
                // its log records actual receiver dispatches only.
                assert_eq!(entry["returned_eax"], 0);
                null_attempts += 1;
                return None;
            };
            Some((
                sender,
                entry["args"][0].as_u64().unwrap() as u8,
                receiver,
                entry["returned_eax"].as_u64().unwrap() as u8,
            ))
        })
        .collect();
    if null_attempts != 0 {
        assert_eq!(null_attempts, 2, "{name}: original null-target control");
        assert!(
            actual.is_empty(),
            "{name}: no represented receiver dispatch"
        );
    }
    assert_eq!(
        actual, expected,
        "{name}: nested native transmit order and replies"
    );
}

fn compare_factory_radio_call(
    sim: &mut Simulation,
    rules: &RuleSet,
    ids: (u64, u64),
    native: &Value,
    name: &str,
) {
    assert_radio_pair(sim, ids, &native["before"], name);
    assert_rng(sim, &native["rng"], "before_hex", name);
    let prior_rng = sim.rng_state();
    let _ = radio::take_transmit_log();
    let scope = native["before"]["scope_counter"].as_u64().unwrap() as u32;
    let reply = in_scopes(sim, scope, &mut |sim| {
        let reply = if native["route"] == "null_sender" {
            radio::receive_radio(
                sim,
                ids.1,
                None,
                RadioMessage::RequestClearance,
                RadioPayload::default(),
                Some(rules),
            )
        } else {
            // The caller-CMP control stops after Unit73A943's original CMP
            // returns. Its answer is compared here through that common owner;
            // the subsequent Unit/Building Unload routing remains unported.
            assert!(native["route"] == "contact0" || native["route"] == "caller_cmp");
            radio::transmit_to_contact(sim, ids.0, RadioMessage::RequestClearance, Some(rules))
        };
        assert_eq!(
            sim.object_placement_scope_active(),
            scope != 0,
            "{name}: radio preserves the enclosing placement scope"
        );
        reply
    });
    assert_eq!(
        json!(reply.code()),
        native["eax"],
        "{name}: original radio reply"
    );
    assert_eq!(
        native["before"]["scope_counter"],
        native["after"]["scope_counter"]
    );
    assert!(!sim.object_placement_scope_active());
    assert_radio_pair(sim, ids, &native["after"], name);
    assert_radio_transmits(native, ids, name);
    assert_eq!(
        sim.rng_state(),
        prior_rng,
        "{name}: all RNG owners unchanged"
    );
    assert_rng(sim, &native["rng"], "after_hex", name);
}

#[test]
fn retail_unit_unlimbo_matches_original_admission_and_caller_pose() {
    let Some((rules, registry, terrain_rules)) = retail_rules() else {
        return;
    };
    let corpus = corpus();
    for row in corpus["direct_rows"].as_array().unwrap() {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let mut sim = retail_world(&rules, &terrain_rules);
        let id = sim
            .construct_object_limbo_at_height("MTNK", "Americans", 0, 0, 0, 0, &rules)
            .unwrap();
        assert_actor(&sim, id, &row["before"]["actor"], name);
        assert_stage(&sim, id, &row["stage_before"], name);
        assert_rng(&sim, &row["rng"], "before_hex", name);
        let object = rules.object("MTNK").unwrap();
        assert_eq!(json!(object.strength), row["inherited_inputs"]["strength"]);
        assert_eq!(object.locomotor, LocomotorKind::Drive);
        assert_eq!(
            json!(object.image),
            row["inherited_inputs"]["full_type_layers"][0]["resolved_image"]
        );
        let requested = coord(&row["before"]["requested_xyz"]);
        let cell = ((requested.x / 256) as u16, (requested.y / 256) as u16);
        apply_cell_inputs(&mut sim, cell, input);
        sim.substrate.entities.get_mut(id).unwrap().on_bridge =
            input["caller_on_bridge"].as_bool().unwrap();
        let prior_rng = sim.rng_state();
        let placed = in_scopes(
            &mut sim,
            input["scenario_init_counter"].as_u64().unwrap() as u32,
            &mut |sim| {
                sim.reveal_constructed_object_at_coord_with_overlay_context(
                    id,
                    requested,
                    128,
                    PlacementEvidence::EvaluateMark,
                    &rules,
                    Some(&registry),
                )
            },
        );
        assert_eq!(
            json!(u8::from(placed.is_some())),
            row["returned_al"],
            "{name}: original Unit return"
        );
        assert!(
            !sim.object_placement_scope_active(),
            "{name}: restored placement scope"
        );
        let native = &row["after_unlimbo"];
        assert_actor(&sim, id, &native["actor"], name);
        assert_stage(&sim, id, &row["stage_after"], name);
        assert_eq!(
            json!(u8::from(sim.substrate.entities.get(id).unwrap().on_bridge)),
            native["on_bridge"],
            "{name}: caller OnBridge"
        );
        assert_planes(
            &sim,
            id,
            cell,
            &native["occupation"],
            &native["lists"],
            name,
        );
        assert_eq!(
            sim.rng_state(),
            prior_rng,
            "{name}: all represented RNG owners unchanged"
        );
        assert_rng(&sim, &row["rng"], "after_hex", name);
    }
}

/// Original Unit7434EC..743514 HIGH preparation and 7435C6..7435D6 caller,
/// followed by complete 737BA0. The six saved controls supply parsed HIGH,
/// HasBridge, raw planes and scope independently. Four compare the production
/// authored projection using a real [Units] line; this does not certify a
/// complete ReadScenario, lexical native map reader or other class projection.
/// The two scope0 refusals enter the shared exact-coordinate owner directly,
/// outside authored projection's required placement bracket.
#[test]
fn retail_authored_unit_high_matches_original_pose_scope_and_lists() {
    let Some((rules, registry, terrain_rules)) = retail_rules() else {
        return;
    };
    let corpus = corpus();
    let rows = corpus["authored_rows"].as_array().unwrap();
    assert_eq!(rows.len(), 6);
    let mut projected = 0;
    let mut refused = 0;
    for row in rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let requested = coord(&row["pre_unlimbo"]["requested_xyz"]);
        let cell = ((requested.x / 256) as u16, (requested.y / 256) as u16);
        let mut sim = retail_world(&rules, &terrain_rules);
        // Actual GameMode retains the original fixture's zero default; the
        // separately supplied A8E7AC scope is applied through its own owner.
        apply_cell_inputs(
            &mut sim,
            cell,
            &json!({
                "bridge": input["has_bridge"],
                "ground_bits": input["ground_bits"],
                "deck_bits": input["deck_bits"],
                "actual_game_mode": 0,
            }),
        );
        let id = if input["scope"] == 2 {
            projected += 1;
            let entities = parse_map_entities(&IniFile::from_str(&format!(
                "[Units]\n0=Americans,MTNK,256,{},{},128,Guard,None,0,-1,{},-1,1,1\n",
                cell.0, cell.1, input["parsed_high"],
            )));
            assert_eq!(entities.len(), 1);
            assert_eq!(entities[0].high, input["parsed_high"] != 0);
            let terrain = sim.resolved_terrain.as_ref().unwrap().clone();
            let count = sim.with_object_placement_scope(|sim| {
                let count = sim.spawn_from_map_with_resolved_and_overlay_registry(
                    &entities,
                    Some(&rules),
                    Some(&terrain),
                    Some(&registry),
                );
                assert!(
                    sim.object_placement_scope_active(),
                    "{name}: retain the outer scope"
                );
                count
            });
            assert_eq!(
                json!(count),
                row["returned_al"],
                "{name}: authored projection admission"
            );
            assert_eq!(sim.substrate.entities.len(), 1);
            // Native before_hex is already POST-constructor. The whole map
            // call starts at seed0 and spends that original constructor draw;
            // compare its retained phase and final complete RNG below, without
            // adopting a post-constructor cursor or fitting a Rust answer.
            sim.substrate.entities.values().next().unwrap().stable_id
        } else {
            refused += 1;
            assert_eq!(input["scope"], 0);
            let id = sim
                .construct_object_limbo_at_height("MTNK", "Americans", 0, 0, 0, 0, &rules)
                .unwrap();
            assert_actor(&sim, id, &row["before"], name);
            assert_rng(&sim, &row["rng"], "before_hex", name);
            sim.substrate.entities.get_mut(id).unwrap().on_bridge =
                row["pre_unlimbo"]["actor"]["on_bridge"] != 0;
            let result = sim.reveal_constructed_object_at_coord_with_overlay_context(
                id,
                requested,
                128,
                PlacementEvidence::EvaluateMark,
                &rules,
                Some(&registry),
            );
            assert_eq!(
                json!(u8::from(result.is_some())),
                row["returned_al"],
                "{name}: original scope0 admission"
            );
            id
        };
        assert!(
            !sim.object_placement_scope_active(),
            "{name}: restore all caller scopes"
        );
        assert_actor(&sim, id, &row["after"], name);
        assert_stage(&sim, id, &row["stage_after"], name);
        let actor = sim.substrate.entities.get(id).unwrap();
        assert_eq!(
            json!(u8::from(actor.on_bridge)),
            row["after"]["on_bridge"],
            "{name}: HIGH owns OnBridge"
        );
        assert_eq!(
            json!(u8::from(actor.in_playfield)),
            row["after"]["in_playfield"],
            "{name}: retained Techno3D5"
        );
        assert_planes(&sim, id, cell, &row["raw_after"], &row["lists_after"], name);
        assert_rng(&sim, &row["rng"], "after_hex", name);
    }
    assert_eq!((projected, refused), (4, 2));
}

/// The existing ramp corpus executes original 747EB0/5247D0 placement leaves.
/// Its nine placement_input controls measure the retained Unit XYZ for exact
/// requested Z, including negative values and i32MAX, on the supplied ramp.
/// This compares that numeric leaf through shared Reveal. The placement scope
/// bypasses class admission which those vectors do not measure; no whole-Unit,
/// arbitrary range, RNG or lifecycle equivalence is claimed by this sample.
#[test]
fn exact_unit_placement_input_z_matches_existing_original_ramp_controls() {
    let Some((rules, registry, terrain_rules)) = retail_rules() else {
        return;
    };
    let native: Value =
        serde_json::from_str(crate::test_fixture::text("tools/ramp_height_vectors.json")).unwrap();
    assert_eq!(native["schema"], 1);
    assert_eq!(native["native_sha256"], NATIVE_SHA256);
    let cases = native["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 158);
    let mut compared = 0;
    for case in cases.iter().filter(|case| {
        case["name"]
            .as_str()
            .unwrap()
            .starts_with("placement_input_")
    }) {
        compared += 1;
        let name = case["name"].as_str().unwrap();
        let requested = coord(&case["coord"]);
        let cell = ((requested.x / 256) as u16, (requested.y / 256) as u16);
        assert_eq!(case["missing"], false, "{name}: supplied real cell");
        let mut sim = retail_world(&rules, &terrain_rules);
        let id = sim
            .construct_object_limbo_at_height("MTNK", "Americans", 0, 0, 0, 0, &rules)
            .unwrap();
        let on_bridge = case["on_bridge"].as_bool().unwrap();
        {
            let target = sim
                .resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(cell.0, cell.1)
                .unwrap();
            target.level = int(&case["level"]) as u8;
            target.slope_type = case["ramp"].as_u64().unwrap() as u8;
            target.bridge_facts.raw_flags = if on_bridge { 0x100 } else { 0 };
            target.has_bridge_deck = on_bridge;
            target.bridge_walkable = on_bridge;
            target.bridge_deck_level = target.level.wrapping_add(if on_bridge { 4 } else { 0 });
        }
        sim.substrate.entities.get_mut(id).unwrap().on_bridge = on_bridge;
        assert!(
            sim.with_object_placement_scope(|sim| {
                sim.reveal_constructed_object_at_coord_with_overlay_context(
                    id,
                    requested,
                    128,
                    PlacementEvidence::EvaluateMark,
                    &rules,
                    Some(&registry),
                )
                .is_some()
            }),
            "{name}: admitted comparison seam"
        );
        let actual =
            ground_pose::position_world_coord(&sim.substrate.entities.get(id).unwrap().position);
        assert_eq!(
            json!([actual.x, actual.y, actual.z]),
            case["native"]["placement"]["unit"],
            "{name}: original exact input clamp"
        );
    }
    assert_eq!(compared, 9);
}

/// Unit737BF5..737C75 reads the retained UnitType E18/E19 booleans, not
/// General's SmallVisceroid/LargeVisceroid type references. Native eight
/// controls reuse an actually placed/limboed MTNK, poison the declared Stage
/// fields, then execute whole737BA0 at frame123 with clear/busy admission.
/// The inherited frame1 Attack command's target/mission lifecycle is not the
/// compared mechanism; its complete RNG continuation is adopted through the
/// persistence owner before the second call, without seed fitting or draws.
#[test]
fn retail_reused_unit_unlimbo_matches_original_stage_and_rng_tail() {
    let Some((physical_ini, art)) = retail_rules_and_art() else {
        return;
    };
    let mut base_rules = RuleSet::from_ini_with_fixed_art_for_test(&physical_ini, &art).unwrap();
    base_rules.install_art_data(ArtRegistry::from_ini(&art));
    let registry = OverlayTypeRegistry::from_ini(&physical_ini, Some(&art));
    let terrain_rules = TerrainRules::from_ini(&physical_ini);
    let corpus = corpus();
    let receipt = &corpus["visceroid_reader_receipt"];
    let constructor_flags = json!([
        receipt["constructor_flags"]["small_visceroid"],
        receipt["constructor_flags"]["large_visceroid"],
    ]);
    assert_eq!(constructor_flags, json!([0, 0]));
    assert_eq!(receipt["rows"].as_array().unwrap().len(), 6);
    let flags = |rules: &RuleSet| {
        let object = rules.object("MTNK").unwrap();
        json!([
            u8::from(object.small_visceroid),
            u8::from(object.large_visceroid)
        ])
    };
    assert_eq!(flags(&base_rules), constructor_flags);

    // Original747862..74789C/ReadBool5295F0 supplies the retained bytes as
    // defaults on EACH pass. Exercise the production ordered Rules owner,
    // whose projected values retain missing/invalid reads. Each native raw
    // prior is established here by a valid Unit-section reader patch; no
    // ObjectType mutation or second retention implementation is introduced.
    let mut reader_passes = 0;
    for history in receipt["rows"].as_array().unwrap() {
        let name = history["input"]["name"].as_str().unwrap();
        let initial = &history["input"]["initial"];
        let mut layers = RulesLayerStack::new(physical_ini.clone());
        layers.push(
            RulesLayerKind::LangRule,
            IniFile::from_str(&format!(
                "[MTNK]\nSmallVisceroid={}\nLargeVisceroid={}\n",
                if initial[0] == 1 { "yes" } else { "no" },
                if initial[1] == 1 { "true" } else { "no" },
            )),
        );
        let mut parsed =
            RuleSet::from_processed_rules(&layers.process_with_fixed_art(&art).unwrap()).unwrap();
        assert_eq!(
            flags(&parsed),
            *initial,
            "{name}: declared native reader prior"
        );
        for pass in history["passes"].as_array().unwrap() {
            assert_eq!(
                flags(&parsed),
                pass["before"],
                "{name}: current-byte defaults"
            );
            let mut patch = String::from("[MTNK]\n");
            for (key, value) in pass["fields"].as_object().unwrap() {
                patch.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
            }
            // Repeated Scenario-shaped passes stand for the receipt's ordered
            // type-reader calls, not a complete native Rules/Scenario load.
            layers.push(RulesLayerKind::Scenario, IniFile::from_str(&patch));
            parsed = RuleSet::from_processed_rules(&layers.process_with_fixed_art(&art).unwrap())
                .unwrap();
            assert_eq!(
                flags(&parsed),
                pass["after"],
                "{name}: original Unit bool read"
            );
            reader_passes += 1;
        }
    }
    assert_eq!(reader_passes, 8);

    let initial_input = &corpus["direct_rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "ground_clear")
        .unwrap()["input"];
    let rows = corpus["stage_rows"].as_array().unwrap();
    assert_eq!(rows.len(), 8);
    let mut admitted = 0;
    let mut refused = 0;
    for row in rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let small = input["small_visceroid"].as_bool().unwrap();
        let large = input["large_visceroid"].as_bool().unwrap();
        let mut variant_ini = physical_ini.clone();
        variant_ini.merge(&IniFile::from_str(&format!(
            "[MTNK]\nSmallVisceroid={}\nLargeVisceroid={}\n",
            if small { "yes" } else { "no" },
            if large { "true" } else { "no" },
        )));
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&variant_ini, &art).unwrap();
        rules.install_art_data(ArtRegistry::from_ini(&art));
        assert_eq!(flags(&rules), json!([u8::from(small), u8::from(large)]));
        let mut sim = retail_world(&base_rules, &terrain_rules);
        let id = sim
            .construct_object_limbo_at_height("MTNK", "Americans", 0, 0, 0, 0, &base_rules)
            .unwrap();
        let initial = &row["initial_placement"];
        assert_stage(&sim, id, &initial["stage_before"], name);
        let requested = coord(&initial["before"]["requested_xyz"]);
        let cell = ((requested.x / 256) as u16, (requested.y / 256) as u16);
        apply_cell_inputs(&mut sim, cell, initial_input);
        assert!(
            sim.reveal_constructed_object_at_coord_with_overlay_context(
                id,
                requested,
                128,
                PlacementEvidence::EvaluateMark,
                &base_rules,
                Some(&registry),
            )
            .is_some()
        );
        assert_stage(&sim, id, &initial["stage_after"], name);
        assert_stage(&sim, id, &row["limbo"]["stage_before"], name);
        assert_eq!(
            sim.techno_limbo_with_rules(id, &base_rules, Some(&registry)),
            super::ConcealOutcome::Concealed,
            "{name}: shared Limbo owner prepares the reused object"
        );
        assert_stage(&sim, id, &row["limbo"]["stage_after"], name);
        let poisoned = &input["poisoned_stage"];
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .install_native_stage_fixture(StageClass::from_native_fixture(
                int(&poisoned["value"]),
                poisoned["changed"].as_u64().unwrap() as u8,
                CdTimer::from_raw(int(&poisoned["start"]), int(&poisoned["duration"])),
                int(&poisoned["rate"]),
                int(&poisoned["increment"]),
            ));
        sim.session.binary_frame = input["frame"].as_u64().unwrap() as u32;
        sim.main_rng = serde_json::from_value::<SimRng>(row["rng_before"]["main"].clone()).unwrap();
        sim.scenario_rng =
            serde_json::from_value::<SimRng>(row["rng_before"]["scenario"].clone()).unwrap();
        sim.mapgen_rng =
            serde_json::from_value::<SimRng>(row["rng_before"]["mapgen"].clone()).unwrap();
        apply_cell_inputs(&mut sim, cell, input);
        assert_stage(&sim, id, &row["stage_before"], name);
        assert_rng(&sim, &row["rng"], "before_hex", name);
        let before_rng = sim.rng_state();
        let result = sim.reveal_constructed_object_at_coord_with_overlay_context(
            id,
            requested,
            128,
            PlacementEvidence::EvaluateMark,
            &rules,
            Some(&registry),
        );
        assert_eq!(
            json!(u8::from(result.is_some())),
            row["returned_al"],
            "{name}: reused original Unit return"
        );
        assert_stage(&sim, id, &row["stage_after"], name);
        assert_rng(&sim, &row["rng"], "after_hex", name);
        if row["returned_al"] == 0 {
            refused += 1;
            assert_eq!(
                sim.rng_state(),
                before_rng,
                "{name}: refused Stage keeps all RNG owners"
            );
        } else {
            admitted += 1;
        }
    }
    assert_eq!((admitted, refused), (4, 4));
}

/// The native packet runs whole443C60; this comparison covers the five stock
/// GAWEAP controls and three original zero/missing/Z-only controls at Rust's
/// shared ExitObject boundary, including the final queued Unload. The original
/// fixture has no House+68 producer membership: it supplies the constructed
/// producer's pose and Guard without a successful producer Unlimbo. Factory
/// release follows the native comparison through the existing lifecycle owner;
/// queue setup and whole producer activation are not compared. Two
/// inactive-Scenario failures have no Rust Scenario-active owner; GAYARD+MTNK
/// is not legal retail naval production. Their saved native rows are retained
/// and explicitly excluded.
#[test]
fn retail_land_factory_delivery_matches_original_unit_unlimbo_suffix() {
    let Some((ini, art)) = retail_rules_and_art() else {
        return;
    };
    let corpus = corpus();
    let rows = corpus["factory_rows"].as_array().unwrap();
    assert_eq!(
        rows.iter()
            .filter(|row| row["input"]["scenario_active"] == false)
            .count(),
        2
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row["input"]["producer_type"] == "GAYARD")
            .count(),
        1
    );
    let controls = corpus["factory_exit_coordinate_rows"].as_array().unwrap();
    assert_eq!(controls.len(), 5);
    let delivery_controls: Vec<_> = controls
        .iter()
        .filter(|row| row["input"]["route"] == "delivery")
        .collect();
    assert_eq!(delivery_controls.len(), 3);
    let mut compared = 0;
    for (row, control) in rows.iter().map(|row| (row, None)).chain(
        delivery_controls
            .iter()
            .map(|control| (&control["whole_exit"], Some(*control))),
    ) {
        let input = &row["input"];
        if input["scenario_active"] == false || input["producer_type"] != "GAWEAP" {
            continue;
        }
        compared += 1;
        let name = input["name"].as_str().unwrap();
        let mut fixture_ini = ini.clone();
        if let Some(control) = control {
            let setup = &control["setup"]["exit_coord"];
            if input["exit_coord_control"] == "constructor_missing" {
                fixture_ini = fixture_ini.without_entry_for_test("GAWEAP", "ExitCoord");
            } else {
                let raw = setup["read_passes"]
                    .as_array()
                    .unwrap()
                    .last()
                    .unwrap()["supplied_section"]["ExitCoord"]
                    .as_str()
                    .unwrap();
                fixture_ini.merge(&IniFile::from_str(&format!("[GAWEAP]\nExitCoord={raw}\n")));
            }
        }
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&fixture_ini, &art).unwrap();
        rules.install_art_data(ArtRegistry::from_ini(&art));
        let registry = OverlayTypeRegistry::from_ini(&fixture_ini, Some(&art));
        let terrain_rules = TerrainRules::from_ini(&fixture_ini);
        let mut sim = retail_world(&rules, &terrain_rules);
        let owner = sim.interner.intern("Americans");
        let unit_type = sim.interner.intern("MTNK");
        assert!(sim.production.factory_shadow.test_enqueue_kernel(
            owner,
            ProductionCategory::Vehicle,
            unit_type,
            1,
            rules.object("MTNK").unwrap().cost,
        ));
        let id = production::construct_active_factory_fixture(
            &mut sim,
            &rules,
            owner,
            ProductionCategory::Vehicle,
            unit_type,
        )
        .unwrap();
        // Original fixture constructs the actor first and the producer second.
        // The existing Rust constructor spends the same two Scenario draws;
        // no fixture RNG draw or alternate RNG implementation is needed.
        let producer_id = sim
            .construct_object_limbo_at_height("GAWEAP", "Americans", 0, 0, 0, 0, &rules)
            .unwrap();
        let producer_coord = coord(&input["producer_xyz"]);
        {
            let producer = sim.substrate.entities.get_mut(producer_id).unwrap();
            ground_pose::put_location(&mut producer.position, producer_coord);
            producer.position.z = 4;
        }
        sim.mission_assign_exact(producer_id, MissionId::from_known(MissionType::Guard), 0)
            .unwrap();
        assert!(
            sim.houses[&owner].base_projection.buildings().is_empty(),
            "{name}: direct native ExitObject has no House producer membership"
        );
        let producer_type = rules.object("GAWEAP").unwrap();
        let exit = producer_type.exit_coord;
        if let Some(control) = control {
            assert_eq!(
                exit.is_none(),
                input["exit_coord_control"] == "constructor_missing",
                "{name}: fresh absence remains distinct from a parsed zero vector"
            );
            let supplied = exit.map_or_else(
                || control["setup"]["exit_coord"]["ctor_exit_coord"].clone(),
                |(x, y, z)| json!([x, y, z]),
            );
            assert_eq!(supplied, control["setup"]["exit_coord"]["final_coord"]);
        }
        let exit = exit.unwrap_or((0, 0, 0));
        assert_eq!(
            json!([exit.0, exit.1, exit.2]),
            row["producer"]["exit_coord"],
            "{name}: physical Rules ExitCoord"
        );
        assert_eq!(
            producer_type.foundation, "5x3",
            "{name}: physical ART Foundation"
        );
        assert_eq!(
            json!(u8::from(producer_type.weapons_factory)),
            row["producer"]["flags"]["0x16bd"]
        );
        assert_eq!(
            json!(u8::from(producer_type.refinery)),
            row["producer"]["flags"]["0x16bb"]
        );
        assert_eq!(
            json!(u8::from(producer_type.weeder)),
            row["producer"]["flags"]["0x16bc"]
        );
        assert_eq!(
            json!(u8::from(producer_type.naval)),
            row["producer"]["flags"]["0xcce"]
        );
        assert_actor(&sim, id, &row["before"], name);
        assert_stage(&sim, id, &row["stage_before"], name);
        assert_rng(&sim, &row["rng"], "before_hex", name);
        let expected_position = coord(&row["after"]["position"]);
        let cell = (
            (expected_position.x / 256) as u16,
            (expected_position.y / 256) as u16,
        );
        apply_cell_inputs(&mut sim, cell, input);
        if let Some(control) = control {
            let native = &control["actual_cell"]["before"];
            assert_eq!(json!([cell.0, cell.1]), control["actual_cell"]["coord"]);
            let land = native["land"].as_u64().unwrap() as u8;
            let semantics = terrain_rules.semantics_for_land_type(land).unwrap();
            let target = sim
                .resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(cell.0, cell.1)
                .unwrap();
            target.level = native["level"].as_u64().unwrap() as u8;
            target.slope_type = native["slope"].as_u64().unwrap() as u8;
            target.land_type = land;
            target.yr_cell_land_type = land;
            target.zone_type = native["zone_type"].as_u64().unwrap() as u8;
            target.terrain_class = semantics.terrain_class;
            target.base_terrain_class = semantics.terrain_class;
            target.speed_costs = semantics.speed_costs.clone();
            target.base_speed_costs = semantics.speed_costs.clone();
            target.final_tile_index = int(&native["tile"]);
            target.final_sub_tile = native["subtile"].as_u64().unwrap() as u8;
            target.bridge_facts.raw_flags = native["flags"].as_u64().unwrap() as u32;
        }
        assert!(
            sim.production
                .factory_shadow
                .test_arm_ready(owner, ProductionCategory::Vehicle)
        );
        let prior_rng = sim.rng_state();
        let prior_tick = sim.session.tick;
        let prior_frame = sim.session.binary_frame;
        assert_eq!(
            json!(prior_frame),
            row["before"]["frame"],
            "{name}: original supplied ExitObject frame"
        );
        let scope_start = input["scope_start"].as_u64().unwrap() as u32;
        let exit = in_scopes(&mut sim, scope_start, &mut |sim| {
            let result =
                production::exit_produced_object(sim, &rules, producer_id, id, Some(&registry));
            assert_eq!(
                sim.object_placement_scope_active(),
                scope_start != 0,
                "{name}: delivery restores the enclosing scope"
            );
            result
        });
        assert_eq!(
            exit,
            crate::sim::ai_base_building::BuildingExit::Placed,
            "{name}: original successful ExitObject return"
        );
        assert_eq!(row["eax"], 2, "{name}: native ExitObject success");
        assert_eq!(
            json!(sim.session.binary_frame),
            row["after"]["frame"],
            "{name}: direct ExitObject preserves its native frame"
        );
        assert_eq!(
            row["counter_after"], input["scope_start"],
            "{name}: native caller scope restored"
        );
        assert!(
            !sim.object_placement_scope_active(),
            "{name}: fixture outer scopes restored"
        );
        assert_actor(&sim, id, &row["after"], name);
        assert_stage(&sim, id, &row["stage_after"], name);
        let actor = sim.substrate.entities.get(id).unwrap();
        assert_eq!(
            json!([
                actor.mission.dispatch_timer().start_frame(),
                actor.mission.dispatch_timer().delay()
            ]),
            row["after"]["dispatch"],
            "{name}: original ExitObject mission timer"
        );
        assert_eq!(
            json!([
                actor.rearm_timer.start_frame(),
                actor.rearm_timer.duration()
            ]),
            row["after"]["rearm"],
            "{name}: original ExitObject rearm timer"
        );
        assert_eq!(
            json!(u8::from(actor.on_bridge)),
            row["after"]["on_bridge"],
            "{name}: caller OnBridge"
        );
        assert_eq!(
            json!(u8::from(actor.in_playfield)),
            row["after"]["in_playfield"],
            "{name}: retained Techno3D5"
        );
        assert!(
            actor.has_live_contact_with(producer_id),
            "{name}: Radio HELLO contact"
        );
        assert_eq!(
            actor.dock_entered_with,
            Some(producer_id),
            "{name}: Radio TETHER receiver"
        );
        assert!(
            sim.substrate
                .entities
                .get(producer_id)
                .unwrap()
                .has_live_contact_with(id),
            "{name}: reciprocal Radio HELLO contact"
        );
        assert_eq!(
            json!(
                sim.substrate
                    .entities
                    .get(producer_id)
                    .unwrap()
                    .mission
                    .queued()
                    .raw()
            ),
            row["producer_queued"],
            "{name}: original producer QueueMission(Unload16)"
        );
        let plane = control.map_or(
            &row["plane_after"],
            |control| &control["actual_cell"]["plane_after"],
        );
        let lists = json!([
            format!("0x{:x}", plane[0].as_u64().unwrap()),
            format!("0x{:x}", plane[1].as_u64().unwrap()),
        ]);
        assert_planes(&sim, id, cell, &json!([plane[2], plane[3]]), &lists, name);
        assert_eq!(
            sim.rng_state(),
            prior_rng,
            "{name}: all represented RNG owners unchanged"
        );
        assert_rng(&sim, &row["rng"], "after_hex", name);
        production::release_delivered_mobile(&mut sim, &rules, owner, ProductionCategory::Vehicle);
        assert!(
            sim.production
                .factory_shadow
                .view(owner, ProductionCategory::Vehicle)
                .is_none_or(|factory| factory.object.is_none()),
            "{name}: successful caller releases its held identity"
        );
        assert_eq!(
            (sim.session.tick, sim.session.binary_frame),
            (prior_tick, prior_frame),
            "{name}: ExitObject and caller release do not advance a frame"
        );
        assert_eq!(
            sim.rng_state(),
            prior_rng,
            "{name}: caller release does not add RNG draws"
        );
    }
    assert_eq!(compared, 8);
}

/// Original Unit's Radio65ACB0 -> Building43C2D0 -> Techno6F4C29 -> Radio65A970,
/// captured after the original factory443C60 transaction. The producer's
/// selected native type-reader fixture leaves Strength0/InLimbo1/3D5=0 and
/// retains Guard5/queued-Unload16. Those scalars are declared prior inputs;
/// this does not claim a full producer activation or Unload FSM comparison.
/// HELLO/TETHER links are established through their shared radio owners.
#[test]
fn retail_factory_exit_radio_matches_original_reciprocal_cleanup() {
    let Some((rules, registry, terrain_rules)) = retail_rules() else {
        return;
    };
    let corpus = corpus();
    let rows = corpus["factory_exit_radio_rows"].as_array().unwrap();
    assert_eq!(rows.len(), 7);
    let mut calls = 0;
    let mut null_sender_controls = 0;
    for row in rows {
        let name = row["input"]["name"].as_str().unwrap();
        let primary = &row["primary"];
        let before = &primary["before"];
        let mut sim = retail_world(&rules, &terrain_rules);
        // Original constructor order supplies Scenario cursor2, while Main
        // and MapGen stay at their full original seed0 buffers.
        let unit = sim
            .construct_object_limbo_at_height("MTNK", "Americans", 0, 0, 0, 0, &rules)
            .unwrap();
        let producer = sim
            .construct_object_limbo_at_height("GAWEAP", "Americans", 0, 0, 0, 0, &rules)
            .unwrap();
        let ids = (unit, producer);
        {
            let endpoint = sim.substrate.entities.get_mut(producer).unwrap();
            ground_pose::put_location(
                &mut endpoint.position,
                coord(&before["producer"]["position"]),
            );
            endpoint.health.current = int(&before["producer"]["health"]);
            let mission = &before["producer"];
            endpoint.mission.apply_test_fixture(MissionTestFixture {
                current: MissionId::from_raw(int(&mission["mission"])),
                suspended: MissionId::from_raw(-1),
                queued: MissionId::from_raw(int(&mission["queued"])),
                movement_bypass_latch: 0,
                handler_state: mission["handler_state"].as_u64().unwrap() as u32,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: MissionDispatchTimer::from_raw(
                    int(&mission["dispatch"][0]),
                    int(&mission["dispatch"][1]),
                ),
            });
        }
        let requested = coord(&before["unit"]["position"]);
        let cell = ((requested.x / 256) as u16, (requested.y / 256) as u16);
        apply_cell_inputs(&mut sim, cell, &row["input"]);
        assert!(sim.with_object_placement_scope(|sim| {
            sim.reveal_constructed_object_at_coord_with_overlay_context(
                unit,
                requested,
                64,
                PlacementEvidence::EvaluateMark,
                &rules,
                Some(&registry),
            )
            .is_some()
        }));
        let flags = &before["producer_type_flags"];
        let object = rules.object("GAWEAP").unwrap();
        assert_eq!(
            json!(u8::from(object.weapons_factory)),
            flags["WeaponsFactory"]
        );
        assert_eq!(json!(u8::from(object.helipad)), flags["Helipad"]);
        assert_eq!(json!(u8::from(object.unit_repair)), flags["UnitRepair"]);
        assert_eq!(
            radio::transmit(
                &mut sim,
                producer,
                unit,
                RadioMessage::Hello,
                RadioPayload::default(),
                Some(&rules),
            )
            .code(),
            1,
            "{name}: native factory HELLO direction"
        );
        assert_eq!(
            radio::transmit(
                &mut sim,
                producer,
                unit,
                RadioMessage::Tether,
                RadioPayload::default(),
                Some(&rules),
            )
            .code(),
            1,
            "{name}: native factory TETHER direction"
        );
        if row["preliminary"].is_object() {
            compare_factory_radio_call(&mut sim, &rules, ids, &row["preliminary"], name);
            calls += 1;
        } else if before["unit"]["contacts"][0] == "0x0" {
            // Explicit adversarial prior: remove only the Unit contact, while
            // retaining both native tethers and the producer's reciprocal slot.
            sim.substrate
                .entities
                .get_mut(unit)
                .unwrap()
                .radio_contacts
                .remove(producer);
        }
        compare_factory_radio_call(&mut sim, &rules, ids, primary, name);
        calls += 1;
        null_sender_controls += usize::from(primary["route"] == "null_sender");
        if row["already_cleared"].is_object() {
            compare_factory_radio_call(&mut sim, &rules, ids, &row["already_cleared"], name);
            calls += 1;
        }
    }
    assert_eq!(null_sender_controls, 2);
    assert_eq!(calls, 9);
}

#[test]
fn unit_entry_scope_and_actual_game_mode_match_independent_original_controls() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/unit_entry_counter_mode.json",
    ))
    .unwrap();
    assert_eq!(native["schema_version"], 1);
    assert_eq!(native["native_sha256"], NATIVE_SHA256);
    let rows = native["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 8);
    for row in rows {
        let input = &row["input"];
        let name = format!(
            "{} scope{} mode{}",
            input["kind"], input["scenario_init_counter"], input["actual_game_mode"]
        );
        let (mut sim, rules, registry) = entry_test_fixture::fixture_with_rules(
            "[VehicleTypes]\n0=MOVER\n[MOVER]\nSpeedType=Track\n[O0]\nCrate=yes\n",
        );
        sim.mapgen_rng = crate::sim::rng::SimRng::new(31);
        let owner = sim.interner.intern("Americans");
        sim.houses
            .insert(owner, HouseState::new(owner, 0, None, false, 0, 10));
        let mut actor = GameEntity::test_default(90, "MOVER", "Americans", 10, 10);
        actor.type_ref = sim.interner.intern("MOVER");
        actor.owner = owner;
        actor.category = crate::map::entities::EntityCategory::Unit;
        actor.in_playfield = input["fixture"]["in_playfield"].as_bool().unwrap();
        actor.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        sim.substrate.entities.insert(actor);
        sim.mission_assign_exact(90, MissionId::from_known(MissionType::Guard), 100)
            .unwrap();
        let bounds = &input["fixture"]["bounds"];
        sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
            base: int(&bounds[0]),
            off_fc: int(&bounds[1]),
            off_100: int(&bounds[2]),
            off_104: int(&bounds[3]),
            off_108: int(&bounds[4]),
        });
        if input["kind"] == "crate" {
            sim.resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(11, 10)
                .unwrap()
                .bridge_facts
                .overlay_id = Some(0);
        }
        sim.session.game_mode_nonzero = int(&input["actual_game_mode"]) != 0;
        assert_rng(&sim, &row["rng"], "before_hex", &name);
        let before = sim.rng_state();
        let result = in_scopes(
            &mut sim,
            input["scenario_init_counter"].as_u64().unwrap() as u32,
            &mut |sim| {
                let cell = NativeCellQuery::canonical(sim.resolved_terrain.as_ref().unwrap())
                    .lookup((11, 10));
                sim.foot_can_enter(90, cell, InfantryEntryArgs::REPAIR, &rules, Some(&registry))
                    .unwrap()
            },
        );
        assert_eq!(json!(result), row["result"], "{name}");
        assert!(
            !sim.object_placement_scope_active(),
            "{name}: restored caller scope"
        );
        assert_eq!(
            sim.rng_state(),
            before,
            "{name}: all represented RNG unchanged"
        );
        assert_rng(&sim, &row["rng"], "after_hex", &name);
    }
}

#[test]
fn nested_placement_scope_restores_the_caller_after_failed_reveal() {
    let rules = RuleSet::from_ini(&IniFile::from_str("")).unwrap();
    let mut sim = Simulation::new();
    let before = sim.rng_state();
    sim.with_object_placement_scope(|sim| {
        assert!(sim.object_placement_scope_active());
        let result = in_scopes(sim, 3, &mut |sim| {
            sim.reveal_constructed_object_at_height(
                999,
                0,
                0,
                0,
                0,
                PlacementEvidence::RejectedEarly,
                &rules,
            )
        });
        assert!(result.is_none());
        assert!(
            sim.object_placement_scope_active(),
            "retain the enclosing caller scope"
        );
    });
    assert!(!sim.object_placement_scope_active());
    assert_eq!(sim.rng_state(), before);
}
