//! Bounded original Building44D880/Techno6FA5BE and Unit739EC0 comparisons.
//! The saved UnitUnlimboControls continuation executes nine producer Unload
//! leaves, two shared Door completion callers and two miner primary controls.
//! UnitAI, AStar setup and subsequent harvesting are separate boundaries.

use super::{ObjectAiCtx, RuleSet, Simulation, mission_unload};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::art_data::ArtRegistry;
use crate::rules::foundation::FOUNDATION_TABLE;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::retail_ini_fixture::retail_rules_and_art;
use crate::rules::terrain_rules::TerrainRules;
use crate::sim::components::{DriveCoord, NavTargetRef, TrackProgress};
use crate::sim::door::{DoorClass, DoorPhase};
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId};
use crate::sim::movement::DriveLocomotionRuntime;
use crate::sim::movement::{PerCellReason, factory_exit_track_coordinate, ground_pose};
use crate::sim::radio::{self, RadioMessage, RadioPayload};
use crate::sim::rng::SimRng;
use crate::sim::timer::CdTimer;
use crate::util::fixed_math::SimFixed;
use serde_json::{Value, json};

const NATIVE_SHA256: &str = "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c";

fn corpus() -> Value {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/anytown_damage/unit_unlimbo.json",
    ))
    .unwrap();
    assert_eq!(corpus["schema_version"], 1);
    assert_eq!(corpus["native_sha256"], NATIVE_SHA256);
    assert_eq!(corpus["factory_unload_rows"].as_array().unwrap().len(), 1);
    assert_eq!(
        corpus["factory_miner_per_cell_rows"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
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

fn xyz(value: DriveCoord) -> Value {
    json!([value.x, value.y, value.z])
}

fn bytes(hex: &str) -> Vec<u8> {
    assert_eq!(hex.len() % 2, 0);
    (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).unwrap())
        .collect()
}

fn dword(raw: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(raw[offset..offset + 4].try_into().unwrap())
}

fn bits(hex: &Value) -> u64 {
    u64::from_le_bytes(bytes(hex.as_str().unwrap()).try_into().unwrap())
}

fn f64_hex(value: f64) -> String {
    value
        .to_bits()
        .to_le_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Decode only the receipt's representation of an existing RNG object. No
/// Rust seed/search or replacement Random2 decision supplies native answers.
fn rng_input(hex: &Value) -> SimRng {
    let raw = bytes(hex.as_str().unwrap());
    assert_eq!(raw.len(), 0x3f4);
    let words: Vec<u32> = raw[12..]
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect();
    serde_json::from_value(json!({
        "disabled": dword(&raw, 0),
        "index_a": dword(&raw, 4),
        "index_b": dword(&raw, 8),
        "state": words,
    }))
    .unwrap()
}

fn import_rng(sim: &mut Simulation, native: &Value) {
    sim.main_rng = rng_input(&native["main"]["before_hex"]);
    sim.scenario_rng = rng_input(&native["scenario"]["before_hex"]);
    sim.mapgen_rng = rng_input(&native["mapgen"]["before_hex"]);
}

fn assert_rng(sim: &Simulation, native: &Value, boundary: &str, name: &str) {
    for (stream, rng) in [
        ("main", &sim.main_rng),
        ("scenario", &sim.scenario_rng),
        ("mapgen", &sim.mapgen_rng),
    ] {
        assert!(
            rng.native_state_hex() == native[stream][boundary].as_str().unwrap(),
            "{name}: complete {stream} {boundary} native RNG"
        );
    }
}

fn parsed_rules(ini: &IniFile, art: &IniFile, patch: Option<&str>) -> RuleSet {
    let mut layers = RulesLayerStack::new(ini.clone());
    if let Some(patch) = patch {
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(patch));
    }
    let mut rules =
        RuleSet::from_processed_rules(&layers.process_with_fixed_art(art).unwrap()).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(art));
    rules
}

fn assert_door(phase: DoorPhase, fields: (i32, i32, i32), hex: &Value, name: &str) {
    let raw = bytes(hex.as_str().unwrap());
    assert_eq!(raw.len(), 0x1c);
    assert_eq!(
        fields,
        (dword(&raw, 8), dword(&raw, 16), dword(&raw, 20)),
        "{name}: shared Door anchor/duration/total"
    );
    let flags = match phase {
        DoorPhase::ClosedStable => (0, 0),
        DoorPhase::Opening => (1, 1),
        DoorPhase::OpenStable => (0, 1),
        DoorPhase::Closing => (1, 0),
    };
    assert_eq!(flags, (raw[24], raw[25]), "{name}: Door active/direction");
    // Constructor4A50F0 does not initialize the first double or timer aux+C.
    // Drawing progress4A52F0 and those undefined bytes are not gameplay state.
}

#[test]
fn original_foundation_startup_rows_feed_the_factory_track_head() {
    let corpus = corpus();
    let native = &corpus["factory_unload_rows"][0];
    let rows = native["foundation_prerequisite"]["original_startup_rows"]
        .as_array()
        .unwrap();
    assert_eq!(rows.len(), FOUNDATION_TABLE.len());
    assert_eq!(rows.len(), 22);
    let ini = IniFile::from_str("[BuildingTypes]\n0=GAWEAP\n[GAWEAP]\nWeaponsFactory=yes\n");
    for (def, row) in FOUNDATION_TABLE.iter().zip(rows) {
        assert_eq!(json!(def.id), row["id"]);
        let art = IniFile::from_str(&format!("[GAWEAP]\nFoundation={}\n", def.name));
        let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
        let object = rules.object("GAWEAP").expect("fixture GAWEAP");
        assert_eq!(object.foundation, def.name, "real ART foundation reader");
        let pairs = row["all_pairs"].as_array().unwrap();
        assert_eq!(pairs.len(), 30);
        for (index, expected) in pairs.iter().enumerate() {
            let actual =
                crate::sim::docking::building_dock::foundation_exit_pair(&object.foundation, index)
                    .unwrap();
            assert_eq!(
                json!([actual.0, actual.1]),
                *expected,
                "{} slot{index}",
                def.name
            );
        }
        assert_eq!(
            json!(crate::sim::docking::building_dock::foundation_exit_list(
                &object.foundation
            )),
            row["list_before_terminator"],
            "{} original sentinel list",
            def.name
        );
        let terminator = row["terminator_index"].as_u64().unwrap() as usize;
        assert_eq!(pairs[terminator], json!([0x7fff, 0x7fff]));
        assert_eq!(pairs[10], row["element10"]);
        assert_eq!(
            crate::sim::docking::building_dock::foundation_exit_pair(&object.foundation, 30),
            None
        );
    }

    // All22 rows above compare original CRT startup bytes including padding.
    // Only physical GAWEAP5x3 has an executed ForceTrack head in this packet;
    // no hand-derived heads for the other foundations are treated as goldens.
    let Some((ini, art)) = retail_rules_and_art() else {
        return;
    };
    let rules = parsed_rules(&ini, &art, None);
    let head = factory_exit_track_coordinate(
        coord(&native["input"]["producer_xyz"]),
        rules.object("GAWEAP").unwrap(),
    );
    let force = native["journal"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["operation"] == "original_mission_unload" && row["after"]["drive"]["selector"] == 66
        })
        .unwrap();
    assert_eq!(xyz(head), force["after"]["drive"]["head"]);
}

#[test]
fn original_door_controls_match_shared_timer_and_production_deploy_time() {
    let Some((ini, art)) = retail_rules_and_art() else {
        return;
    };
    let corpus = corpus();
    let controls = &corpus["factory_unload_rows"][0]["timer_controls"];
    let rows = controls["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 19);
    let stock_rules = parsed_rules(&ini, &art, None);
    let mut portable = 0;
    let mut raw_leaf_inputs = 0;
    for row in rows {
        let name = row["kind"].as_str().unwrap();
        let native_start = bytes(row["start"].as_str().unwrap());
        let mut door = DoorClass::at_frame(0);
        assert_door(
            door.phase(),
            door.timer_fields(),
            &row["native_default"],
            name,
        );
        let reader = &row["reader"];
        let ticks = if reader.is_object() && reader["portable_reader_result"] == true {
            portable += 1;
            assert_eq!(reader["section"], "GAWEAP");
            assert_eq!(reader["key"], "DeployTime");
            let patch = format!("[GAWEAP]\nDeployTime={}\n", reader["raw"].as_str().unwrap());
            parsed_rules(&ini, &art, Some(&patch))
                .object("GAWEAP")
                .unwrap()
                .deploy_time_ticks
        } else if name.starts_with("zero_") {
            portable += 1;
            parsed_rules(&ini, &art, Some("[GAWEAP]\nDeployTime=0\n"))
                .object("GAWEAP")
                .unwrap()
                .deploy_time_ticks
        } else if matches!(name, "opening" | "closing" | "reversal_at10") {
            portable += 1;
            stock_rules.object("GAWEAP").unwrap().deploy_time_ticks
        } else {
            raw_leaf_inputs += 1;
            // Literal nan/inf assign no float in the original scanner and
            // widen unchanged stack bytes: portable_reader_result=false.
            // Four further rows override the type's raw IEEE input directly.
            // These eight controls pin Door at its observed converted tick
            // argument; they cannot establish a portable INI parse result.
            if reader.is_object() {
                assert_eq!(reader["portable_reader_result"], false);
                assert_eq!(reader["scan"]["sscanf_returned_eax"], 0);
            } else {
                assert!(name.starts_with("raw_nan_") || name.starts_with("raw_infinite_"));
            }
            dword(&native_start, 16) as u32
        };
        assert_eq!(
            ticks as i32,
            dword(&native_start, 16),
            "{name}: signed raw ticks"
        );
        if name.ends_with("closing") {
            // Native explicitly calls ForceOpen4A52D0 before Close4A5240.
            // Existing owner operations establish its stable-open predicate.
            door.open(0, 0);
            door.advance(0);
            assert_eq!(door.phase(), DoorPhase::OpenStable);
            door.close(ticks, 0);
        } else {
            door.open(ticks, 0);
        }
        assert_door(door.phase(), door.timer_fields(), &row["start"], name);
        if row["reversal"].is_object() {
            let reversal = &row["reversal"];
            assert_door(door.phase(), door.timer_fields(), &reversal["before"], name);
            door.reverse(reversal["frame"].as_u64().unwrap() as u32);
            assert_door(door.phase(), door.timer_fields(), &reversal["after"], name);
        }
        let probes = row["probes"].as_array().unwrap();
        for probe in probes {
            assert_door(door.phase(), door.timer_fields(), &probe["before"], name);
            let phase = door.phase();
            for (key, predicate) in [
                ("opening", phase == DoorPhase::Opening),
                ("closing", phase == DoorPhase::Closing),
                ("open_stable", phase == DoorPhase::OpenStable),
                ("closed_stable", phase == DoorPhase::ClosedStable),
            ] {
                assert_eq!(json!(u8::from(predicate)), probe[key], "{name}: {key}");
            }
            let (start, duration, _) = door.timer_fields();
            assert_eq!(
                json!(u8::from(
                    CdTimer::from_raw(start, duration).expired(int(&probe["frame"]))
                )),
                probe["due"],
                "{name}: due does not itself finish the Door"
            );
            assert_door(door.phase(), door.timer_fields(), &probe["after"], name);
        }
        door.advance(probes.last().unwrap()["frame"].as_u64().unwrap() as u32);
        assert_door(
            door.phase(),
            door.timer_fields(),
            &row["after_finish"],
            name,
        );
    }
    assert_eq!((portable, raw_leaf_inputs), (11, 8));
}

struct Fixture {
    sim: Simulation,
    product: u64,
    producer: u64,
}

fn fixture(rules: &RuleSet, ini: &IniFile) -> Fixture {
    let mut sim = Simulation::with_seed(0);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let terrain_rules = TerrainRules::from_ini(ini);
    let road = terrain_rules.semantics_for_land_type(1).unwrap();
    let cells = (0..128)
        .flat_map(|ry| {
            (0..128).map(move |rx| {
                let mut cell =
                    crate::sim::world::lifecycle_tests::common_raw_terrain_cell(rx, ry, 4, false);
                cell.land_type = 1;
                cell.yr_cell_land_type = 1;
                cell.terrain_class = road.terrain_class;
                cell.base_terrain_class = road.terrain_class;
                cell.speed_costs = road.speed_costs.clone();
                cell.base_speed_costs = road.speed_costs.clone();
                cell
            })
        })
        .collect();
    sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(128, 128, cells));
    sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -128,
        off_100: -128,
        off_104: 256,
        off_108: 256,
    });
    sim.session.map_width = 128;
    sim.session.map_height = 128;
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 50_000, 10));
    let product = sim
        .construct_object_limbo_at_height("MTNK", "Americans", 0, 0, 0, 0, rules)
        .unwrap();
    let producer = sim
        .construct_object_limbo_at_height("GAWEAP", "Americans", 0, 0, 0, 0, rules)
        .unwrap();
    Fixture {
        sim,
        product,
        producer,
    }
}

fn actor_input(actor: &mut GameEntity, native: &Value) {
    ground_pose::put_location(&mut actor.position, coord(&native["position"]));
    actor.position.z = (int(&native["position"][2]) / 104) as u8;
    actor.lifecycle.object_alive = native["alive"] == 1;
    actor.lifecycle.in_limbo = native["limbo"] == 1;
    actor.lifecycle.cell_marked = native["marked"] == 1;
    actor.in_logic_vector = native["logic_registered"] == 1;
    actor.in_playfield = native["in_playfield"] == 1;
    actor.on_bridge = native["on_bridge"] == 1;
    actor.health.current = int(&native["health"]);
    actor.body_facing.snap(
        native["primary_facing"][0].as_u64().unwrap() as u16,
        int(&native["frame"]) as u32,
    );
    if let Some(barrel) = actor.barrel_facing.as_mut() {
        barrel.snap(
            native["turret_facing"][0].as_u64().unwrap() as u16,
            int(&native["frame"]) as u32,
        );
    }
    assert_eq!(native["target"], "0x0");
    actor.attack_target = None;
    actor.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_raw(int(&native["mission"])),
        suspended: MissionId::NONE,
        queued: MissionId::from_raw(int(&native["queued"])),
        movement_bypass_latch: 0,
        handler_state: native["status"].as_u64().unwrap() as u32,
        mission_start_frame: 0,
        ai_counter: native["mission_visit_count"].as_u64().unwrap() as u32,
        dispatch_timer: MissionDispatchTimer::from_raw(
            int(&native["dispatch"][0]),
            int(&native["dispatch"][1]),
        ),
    });
}

fn native_nav(native: &Value) -> Option<NavTargetRef> {
    match native["navcom"].as_str().unwrap() {
        "0x0" => None,
        // Original73AADB Scatter publishes this fixture Cell89,51 before
        // the bounded AStar42A5B0 stop. This is an explicit native seam input,
        // not an address-to-cell decoder or a Rust route-selection answer.
        "0x40006e00" => Some(NavTargetRef::cell(89, 51)),
        other => panic!("unmeasured continuation NavCom {other}"),
    }
}

fn fixture_input(fixture: &mut Fixture, rules: &RuleSet, native: &Value) {
    let Fixture {
        sim,
        product,
        producer,
    } = fixture;
    sim.session.binary_frame = native["product"]["frame"].as_u64().unwrap() as u32;
    actor_input(
        sim.substrate.entities.get_mut(*product).unwrap(),
        &native["product"],
    );
    actor_input(
        sim.substrate.entities.get_mut(*producer).unwrap(),
        &native["producer"],
    );
    let drive = &native["drive"];
    let runtime = DriveLocomotionRuntime::default()
        .with_destination_for_test(
            (drive["destination"] != json!([0, 0, 0])).then(|| coord(&drive["destination"])),
        )
        .with_head_to_for_test((drive["head"] != json!([0, 0, 0])).then(|| coord(&drive["head"])))
        .with_track_for_test(TrackProgress {
            turn_index: int(&drive["selector"]),
            cursor: int(&drive["cursor"]),
            reversed: drive["reversed"] == 1,
            residual: int(&drive["residual"]),
        })
        .with_track_valid_for_test(drive["valid"] == 1)
        .with_target_speed_fraction_for_test(SimFixed::from_num(f64::from_bits(bits(
            &drive["target_fraction_bits"],
        ))));
    let unit = sim.substrate.entities.get_mut(*product).unwrap();
    assert!(
        unit.locomotor
            .as_mut()
            .unwrap()
            .install_drive_state_for_test(Some(runtime))
    );
    unit.foot_speed
        .set_speed_fraction_native_bits(bits(&drive["applied_fraction_bits"]));
    unit.navigation.nav_com = native_nav(native);
    assert_eq!(native["producer_archive"], "0x0");
    sim.substrate.occupancy =
        crate::sim::occupancy::OccupancyGrid::rebuild(&sim.substrate.entities);

    let linked = native["radio_pair"]["unit"]["contacts"][0] != "0x0";
    assert_eq!(
        linked,
        native["radio_pair"]["producer"]["contacts"][0] != "0x0"
    );
    if linked
        && sim
            .substrate
            .entities
            .get(*product)
            .unwrap()
            .radio_contacts
            .slot(0)
            .is_none()
    {
        for message in [RadioMessage::Hello, RadioMessage::Tether] {
            assert_eq!(
                radio::transmit(
                    sim,
                    *producer,
                    *product,
                    message,
                    RadioPayload::default(),
                    Some(rules)
                )
                .code(),
                1
            );
        }
    } else if !linked
        && sim
            .substrate
            .entities
            .get(*product)
            .unwrap()
            .radio_contacts
            .slot(0)
            .is_some()
    {
        // Import the declared post-UnitAI radio seam using the existing
        // reciprocal cleanup owner; no raw tether mutation is introduced.
        assert_eq!(
            radio::transmit_to_contact(sim, *product, RadioMessage::RequestClearance, Some(rules))
                .code(),
            23
        );
    }
    let _ = radio::take_transmit_log();
}

fn assert_actor(actor: &GameEntity, native: &Value, name: &str) {
    assert_eq!(
        xyz(ground_pose::position_world_coord(&actor.position)),
        native["position"],
        "{name}: Location"
    );
    for (key, actual) in [
        ("alive", actor.lifecycle.object_alive),
        ("limbo", actor.lifecycle.in_limbo),
        ("marked", actor.lifecycle.cell_marked),
        ("logic_registered", actor.in_logic_vector),
        ("in_playfield", actor.in_playfield),
        ("on_bridge", actor.on_bridge),
    ] {
        assert_eq!(json!(u8::from(actual)), native[key], "{name}: {key}");
    }
    for (key, actual) in [
        ("mission", actor.mission.current().raw()),
        ("queued", actor.mission.queued().raw()),
        ("health", actor.health.current),
    ] {
        assert_eq!(json!(actual), native[key], "{name}: {key}");
    }
    assert_eq!(
        json!(actor.mission.handler_state()),
        native["status"],
        "{name}: handler state"
    );
    let dispatch = actor.mission.dispatch_timer();
    assert_eq!(
        json!([dispatch.start_frame(), dispatch.delay()]),
        native["dispatch"],
        "{name}: dispatch timer"
    );
    assert_eq!(
        json!(actor.techno_ctor_random_word),
        native["random_phase_raw_u16"],
        "{name}: retained original constructor phase"
    );
    assert_eq!(
        json!([
            actor.body_facing.destination(),
            actor.body_facing.start_word()
        ]),
        native["primary_facing"],
        "{name}: body facing"
    );
    let turret = actor
        .barrel_facing
        .as_ref()
        .map_or(json!([0, 0]), |facing| {
            json!([facing.destination(), facing.start_word()])
        });
    assert_eq!(turret, native["turret_facing"], "{name}: turret facing");
}

fn assert_snapshot(fixture: &Fixture, native: &Value, name: &str) {
    let sim = &fixture.sim;
    let product = sim.substrate.entities.get(fixture.product).unwrap();
    let producer = sim.substrate.entities.get(fixture.producer).unwrap();
    assert_actor(product, &native["product"], name);
    assert_actor(producer, &native["producer"], name);
    assert_door(
        producer.door_phase(),
        producer.door_timer_fields(),
        &native["door"],
        name,
    );
    for (role, actor, other) in [
        ("unit", product, fixture.producer),
        ("producer", producer, fixture.product),
    ] {
        let expected = &native["radio_pair"][role];
        assert_eq!(
            json!(actor.radio_contacts.capacity()),
            expected["contact_capacity"]
        );
        let contact = (expected["contacts"][0] != "0x0").then_some(other);
        assert_eq!(
            actor.radio_contacts.slot(0),
            contact,
            "{name}: {role} Contact0"
        );
        assert_eq!(
            actor.dock_entered_with,
            (expected["tether"] == 1).then_some(other),
            "{name}: {role} tether"
        );
    }
    assert_eq!(
        product.navigation.nav_com,
        native_nav(native),
        "{name}: Unit NavCom"
    );
    let actual = product
        .locomotor
        .as_ref()
        .and_then(|l| l.selected_drive_runtime())
        .and_then(|r| r.retained())
        .unwrap();
    let drive = &native["drive"];
    for (key, value) in [
        ("selector", actual.track().turn_index),
        ("cursor", actual.track().cursor),
        ("residual", actual.track().residual),
    ] {
        assert_eq!(json!(value), drive[key], "{name}: Drive {key}");
    }
    assert_eq!(json!(u8::from(actual.track().reversed)), drive["reversed"]);
    assert_eq!(json!(u8::from(actual.track_valid())), drive["valid"]);
    let zero = DriveCoord { x: 0, y: 0, z: 0 };
    assert_eq!(
        xyz(actual.head_to().unwrap_or(zero)),
        drive["head"],
        "{name}: Drive head"
    );
    assert_eq!(
        xyz(actual.destination().unwrap_or(zero)),
        drive["destination"],
        "{name}: Drive destination"
    );
    assert_eq!(
        f64_hex(actual.target_speed_fraction().to_num::<f64>()),
        drive["target_fraction_bits"].as_str().unwrap(),
        "{name}: Drive target fraction"
    );
    assert_eq!(
        f64_hex(product.foot_speed.applied_fraction().to_num::<f64>()),
        drive["applied_fraction_bits"].as_str().unwrap(),
        "{name}: Foot applied fraction"
    );
}

#[test]
fn retail_factory_unload_leaves_match_original_door_track_and_cadence() {
    let Some((ini, art)) = retail_rules_and_art() else {
        return;
    };
    let rules = parsed_rules(&ini, &art, None);
    let corpus = corpus();
    let row = &corpus["factory_unload_rows"][0];
    let mut fixture = fixture(&rules, &ini);
    let mut leaves = 0;
    let mut door_calls = 0;
    let mut commence = 0;
    let mut unit_ai_boundaries = 0;
    for call in row["journal"].as_array().unwrap() {
        let operation = call["operation"].as_str().unwrap();
        if operation == "original_unit_ai" {
            unit_ai_boundaries += 1;
            continue;
        }
        let name = format!("{operation}@{}", call["frame"]);
        // The saved native producer remains constructor-only, in limbo,
        // unmarked and Strength0. These are raw leaf inputs; neither the
        // production activation prefix nor the object dispatcher is claimed.
        fixture_input(&mut fixture, &rules, &call["before"]);
        assert_snapshot(&fixture, &call["before"], &name);
        import_rng(&mut fixture.sim, &call["rng"]);
        assert_rng(&fixture.sim, &call["rng"], "before_hex", &name);
        match operation {
            "producer_commence_queued_unload" => {
                commence += 1;
                let now = fixture.sim.session.binary_frame;
                let result = fixture
                    .sim
                    .mission_commence_exact(fixture.producer, now)
                    .unwrap();
                assert_eq!(json!(u8::from(result)), call["returned_eax"]);
            }
            "original_mission_unload" => {
                leaves += 1;
                let result = mission_unload(
                    &mut fixture.sim,
                    fixture.producer,
                    &rules,
                    ObjectAiCtx::default(),
                )
                .unwrap();
                assert_eq!(
                    json!(result),
                    call["returned_eax"],
                    "{name}: native handler delay"
                );
            }
            "original_techno_door_caller" => {
                door_calls += 1;
                let now = fixture.sim.session.binary_frame;
                fixture
                    .sim
                    .substrate
                    .entities
                    .get_mut(fixture.producer)
                    .unwrap()
                    .advance_door(now);
            }
            other => panic!("unmeasured factory operation {other}"),
        }
        assert_snapshot(&fixture, &call["after"], &name);
        assert_rng(&fixture.sim, &call["rng"], "after_hex", &name);
    }
    assert_eq!(
        (leaves, door_calls, commence, unit_ai_boundaries),
        (9, 2, 1, 75)
    );
    assert_eq!(row["consumer_route_boundary"]["stop_before"], "0x42a5b0");
    // Carrying Door across the real producer calls pins completion ordering.
    // Per-leaf full RNG imports measure each cadence draw without attributing
    // the 75 unexecuted UnitAI turns to this Rust comparison.
}

#[test]
fn original_harvester_and_weeder_primary_controls_use_shared_mission_authority() {
    let Some((ini, art)) = retail_rules_and_art() else {
        return;
    };
    let corpus = corpus();
    let rows = corpus["factory_miner_per_cell_rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        let name = row["name"].as_str().unwrap();
        let reader = &row["reader"];
        assert_eq!(reader["section"], "MTNK");
        let mut patch = String::from("[MTNK]\n");
        for (key, value) in reader["keys"].as_object().unwrap() {
            patch.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
        }
        let rules = parsed_rules(&ini, &art, Some(&patch));
        let object = rules.object("MTNK").unwrap();
        assert_eq!(
            vec![u8::from(object.harvester), u8::from(object.weeder)],
            bytes(reader["after_flags_hex"].as_str().unwrap()),
            "{name}: original Unit reader flags"
        );
        assert_eq!(reader["after_default_mission"], 10);
        assert_eq!(row["ready_slot"], "0x744270");
        assert_eq!(row["stop_before"], "0x73acd7");
        assert!(
            row["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| event["kind"] == "queue_mission"
                    && event["pc"] == "0x5b35e0"
                    && event["args"] == json!([10, 1]))
        );
        assert!(
            row["trace"]
                .as_array()
                .unwrap()
                .iter()
                .any(|point| point["pc"] == "0x73acc8" && point["eax"] == 1)
        );
        let mut fixture = fixture(&rules, &ini);
        fixture_input(&mut fixture, &rules, &row["before"]);
        assert_snapshot(&fixture, &row["before"], name);
        import_rng(&mut fixture.sim, &row["rng"]);
        assert_rng(&fixture.sim, &row["rng"], "before_hex", name);
        let miner = serde_json::to_value(
            &fixture
                .sim
                .substrate
                .entities
                .get(fixture.product)
                .unwrap()
                .miner,
        )
        .unwrap();
        fixture
            .sim
            .per_cell_process(fixture.product, PerCellReason::Arrival, Some(&rules), None)
            .unwrap();
        assert_snapshot(&fixture, &row["after"], name);
        assert_rng(&fixture.sim, &row["rng"], "after_hex", name);
        assert_eq!(
            serde_json::to_value(
                &fixture
                    .sim
                    .substrate
                    .entities
                    .get(fixture.product)
                    .unwrap()
                    .miner
            )
            .unwrap(),
            miner,
            "{name}: existing miner lifecycle is retained"
        );
        let sends = radio::take_transmit_log();
        assert_eq!(
            sends.first().map(|send| (send.msg, send.reply)),
            Some((8, Some(23))),
            "{name}: clearance precedes Harvest"
        );
        // Native stops before refinery-release/crush/Foot73ACD7. The Rust
        // public PerCell route continues that tail on an empty victim list;
        // only represented primary mission/radio/Nav/Drive/pose and all3 RNG
        // fields above are compared. No full UnitAI or harvesting claim.
    }
}

#[test]
fn unit_move_deployment_guard_preserves_door_navigation_and_rng() {
    // Unit740A93..740AB6 returns1 after QueueMission(Guard,0) when any
    // of its existing +6E0/+6E1/+6E2 deployment bytes is set. It never
    // reaches Door Close or the Foot Move cadence/arrival owner.
    for flags in [
        (true, false, false),
        (false, true, false),
        (false, false, true),
        (true, true, true),
    ] {
        for navigation in [None, Some(NavTargetRef::cell(11, 10))] {
            let ini = IniFile::from_str(
                "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=300\nSpeed=6\n\
                 SpeedType=Track\nMovementZone=Normal\nDeployTime=.044\n\
                 Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
                 [Clear]\nBuildable=yes\n",
            );
            let rules = parsed_rules(&ini, &IniFile::from_str(""), None);
            let mut sim = Simulation::with_seed(0);
            crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
            let id = sim
                .spawn_object("MTNK", "Americans", 10, 10, 64, &rules)
                .unwrap();
            sim.session.binary_frame = 200;
            sim.mission_assign_exact(
                id,
                MissionId::from_known(crate::sim::mission::MissionType::Move),
                200,
            )
            .unwrap();
            sim.mission_queue_exact(
                id,
                MissionId::from_known(crate::sim::mission::MissionType::Attack),
                0,
                200,
                &crate::sim::mission::authority::EntityReadyInputProvider,
            )
            .unwrap();
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity.set_unit_simple_deploy_for_test(flags.0, flags.1, flags.2);
            entity.navigation.nav_com = navigation;
            entity.open_door(40, 190);
            let door_before = (entity.door_phase(), entity.door_timer_fields());
            let rng_before = sim.rng_state();

            super::super::dispatch_foot_mission(&mut sim, id, &rules, ObjectAiCtx::default());

            let entity = sim.substrate.entities.get(id).unwrap();
            assert_eq!(
                (entity.door_phase(), entity.door_timer_fields()),
                door_before,
                "{flags:?} {navigation:?}: deployment guard skips Door"
            );
            assert_eq!(entity.navigation.nav_com, navigation);
            assert_eq!(
                entity.mission.current().known(),
                Some(crate::sim::mission::MissionType::Move)
            );
            assert_eq!(
                entity.mission.queued().known(),
                Some(crate::sim::mission::MissionType::Guard)
            );
            assert_eq!(entity.mission.dispatch_timer().start_frame(), 200);
            assert_eq!(entity.mission.dispatch_timer().delay(), 1);
            assert_eq!(
                sim.rng_state(),
                rng_before,
                "deployment guard has no jitter"
            );
        }
    }
}

#[test]
fn original_unit_move_deployment_guards_match_mission_dispatch() {
    let data = corpus();
    let rows = data["unit_move_guard_rows"].as_array().unwrap();
    assert_eq!(rows.len(), 8);
    let Some((ini, art)) = retail_rules_and_art() else {
        return;
    };
    let rules = parsed_rules(&ini, &art, None);
    for row in rows {
        let name = row["input"]["name"].as_str().unwrap();
        let before = &row["before"];
        let after = &row["after"];
        let actor = &before["actor"];
        let now = row["input"]["frame"].as_u64().unwrap() as u32;
        let mut sim = Simulation::with_seed(0);
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let id = sim
            .spawn_object("MTNK", "Americans", 10, 10, 64, &rules)
            .unwrap();
        sim.session.binary_frame = now;
        let nav = |value: &Value| {
            value.as_array().map(|pair| {
                NavTargetRef::cell(
                    pair[0].as_u64().unwrap() as u16,
                    pair[1].as_u64().unwrap() as u16,
                )
            })
        };
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_raw(int(&actor["mission"])),
            suspended: MissionId::NONE,
            queued: MissionId::from_raw(int(&actor["queued"])),
            movement_bypass_latch: 0,
            handler_state: int(&actor["status"]) as u32,
            mission_start_frame: 0,
            ai_counter: int(&actor["mission_visit_count"]) as u32,
            dispatch_timer: MissionDispatchTimer::from_raw(
                int(&actor["dispatch"][0]),
                int(&actor["dispatch"][1]),
            ),
        });
        let flags = &row["input"]["flags"];
        entity.set_unit_simple_deploy_for_test(flags[0] != 0, flags[1] != 0, flags[2] != 0);
        entity.navigation.nav_com = nav(&before["navcell"]);
        let door = bytes(before["door"].as_str().unwrap());
        // The original reader/4A51F0 setup supplies these inputs. This pins
        // the dispatch decision; the separate19 controls compare Door math.
        assert_eq!(door[24..26], [1, 1]);
        assert_eq!(dword(&door, 16), dword(&door, 20));
        entity.open_door(dword(&door, 16) as u32, dword(&door, 8) as u32);
        assert_door(
            entity.door_phase(),
            entity.door_timer_fields(),
            &before["door"],
            name,
        );
        import_rng(&mut sim, &row["rng_pair"]);

        // The arena's pose differs from the native crop. Only dispatch inputs
        // and effects are compared: this guard never resolves coordinates.
        super::super::dispatch_foot_mission(&mut sim, id, &rules, ObjectAiCtx::default());

        let entity = sim.substrate.entities.get(id).unwrap();
        let expected = &after["actor"];
        assert_eq!(
            entity.mission.current().raw(),
            int(&expected["mission"]),
            "{name}"
        );
        assert_eq!(
            entity.mission.queued().raw(),
            int(&expected["queued"]),
            "{name}"
        );
        assert_eq!(
            entity.mission.handler_state(),
            int(&expected["status"]) as u32,
            "{name}"
        );
        let timer = entity.mission.dispatch_timer();
        assert_eq!(
            json!([timer.start_frame(), timer.delay()]),
            expected["dispatch"],
            "{name}"
        );
        assert_eq!(
            timer.delay(),
            int(&row["handler_return_eax"]),
            "{name}: original handler return"
        );
        assert_eq!(entity.navigation.nav_com, nav(&after["navcell"]), "{name}");
        let leaf = entity.mission_leaf.as_unit().unwrap();
        for (key, value) in [
            ("0x6e0", leaf.deployed()),
            ("0x6e1", leaf.deploy_begin_active()),
            ("0x6e2", leaf.deploy_reverse_active()),
        ] {
            assert_eq!(json!(value), after["unit_bytes"][key], "{name}: {key}");
        }
        assert_eq!(
            before["door"], after["door"],
            "{name}: original skipped Door"
        );
        assert_door(
            entity.door_phase(),
            entity.door_timer_fields(),
            &after["door"],
            name,
        );
        assert_rng(&sim, &row["rng_pair"], "after_hex", name);
    }
}
