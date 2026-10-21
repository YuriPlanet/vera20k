//! Bounded original Building44D880/Techno6FA5BE and Unit739EC0 comparisons.
//! The saved UnitUnlimboControls continuation executes nine producer Unload
//! leaves, two shared Door completion callers and two miner primary controls.
//! UnitAI, AStar setup and subsequent harvesting are separate boundaries.

use super::{ObjectAiCtx, RuleSet, Simulation, mission_unload};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
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

fn corpus() -> &'static Value {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/anytown_damage/unit_unlimbo.json");
    assert_eq!(corpus["schema_version"], 1);
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

/// Import the receipt through the RNG owner's complete native-state seam;
/// no Rust seed/search supplies a replacement Random2 decision.
fn import_rng(sim: &mut Simulation, native: &Value) {
    sim.main_rng =
        SimRng::from_native_state_hex_for_test(native["main"]["before_hex"].as_str().unwrap())
            .into();
    sim.scenario_rng =
        SimRng::from_native_state_hex_for_test(native["scenario"]["before_hex"].as_str().unwrap());
    sim.mapgen_rng =
        SimRng::from_native_state_hex_for_test(native["mapgen"]["before_hex"].as_str().unwrap());
}

fn assert_rng(sim: &Simulation, native: &Value, boundary: &str, name: &str) {
    for (stream, rng) in [
        ("main", sim.main_rng.native_state_hex()),
        ("scenario", sim.scenario_rng.native_state_hex()),
        ("mapgen", sim.mapgen_rng.native_state_hex()),
    ] {
        assert!(
            rng == native[stream][boundary].as_str().unwrap(),
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
    rules.install_art_data(crate::rules::retail_ini_fixture::art_registry(art));
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
                cell.speed_costs = road.speed_costs;
                cell.base_speed_costs = road.speed_costs;
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
    sim.session.set_map_width_for_test(128);
    sim.session.set_map_height_for_test(128);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 50_000, 10));
    let product = sim
        .construct_object_limbo_at_height(
            "MTNK",
            "Americans",
            0,
            0,
            0,
            0,
            rules,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    let producer = sim
        .construct_object_limbo_at_height(
            "GAWEAP",
            "Americans",
            0,
            0,
            0,
            0,
            rules,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    Fixture {
        sim,
        product,
        producer,
    }
}

fn actor_input(actor: &mut GameEntity, native: &Value) {
    actor.position.put_location(coord(&native["position"]));
    actor
        .position
        .set_z_for_test((int(&native["position"][2]) / 104) as u8);
    actor
        .lifecycle
        .set_object_alive_for_test(native["alive"] == 1);
    actor.lifecycle.set_in_limbo_for_test(native["limbo"] == 1);
    actor
        .lifecycle
        .set_cell_marked_for_test(native["marked"] == 1);
    actor
        .in_logic_vector
        .set_for_test(native["logic_registered"] == 1);
    actor.set_in_playfield(native["in_playfield"] == 1);
    actor.set_on_bridge(native["on_bridge"] == 1);
    actor.health.set_for_test(int(&native["health"]));
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
    actor.attack_target.set_for_test(None);
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
    sim.session
        .set_binary_frame_for_test(native["product"]["frame"].as_u64().unwrap() as u32);
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
        .with_track_for_test(TrackProgress::for_test(
            int(&drive["selector"]),
            int(&drive["cursor"]),
            drive["reversed"] == 1,
            int(&drive["residual"]),
        ))
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
    unit.navigation.set_nav_com_for_test(native_nav(native));
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
                    rules,
                    crate::sim::world::FrameEffects::default()
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
            radio::transmit_to_contact(
                sim,
                *product,
                RadioMessage::RequestClearance,
                rules,
                crate::sim::world::FrameEffects::default()
            )
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
        ("alive", actor.lifecycle.object_alive()),
        ("limbo", actor.lifecycle.in_limbo()),
        ("marked", actor.lifecycle.cell_marked()),
        ("logic_registered", actor.in_logic_vector.is_member()),
        ("in_playfield", actor.in_playfield()),
        ("on_bridge", actor.on_bridge()),
    ] {
        assert_eq!(json!(u8::from(actual)), native[key], "{name}: {key}");
    }
    for (key, actual) in [
        ("mission", actor.mission.current().raw()),
        ("queued", actor.mission.queued().raw()),
        ("health", actor.health.current()),
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
        json!(actor.techno_ctor_random_word()),
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
            actor.tether.partner(),
            (expected["tether"] == 1).then_some(other),
            "{name}: {role} tether"
        );
    }
    assert_eq!(
        product.navigation.nav_com(),
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
        ("selector", actual.track().turn_index()),
        ("cursor", actual.track().cursor()),
        ("residual", actual.track().residual()),
    ] {
        assert_eq!(json!(value), drive[key], "{name}: Drive {key}");
    }
    assert_eq!(
        json!(u8::from(actual.track().reversed())),
        drive["reversed"]
    );
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
                let now = fixture.sim.session.binary_frame();
                let result = fixture
                    .sim
                    .substrate
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
                let now = fixture.sim.session.binary_frame();
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

/// Two original44D880 leaves, not a replay of the intervening BuildingAI,
/// UnitAI or AnimAI visits. Each prior (including all three RNG objects) is
/// imported from that actual call's entry, independently of the Rust result.
#[test]
fn stock_naval_unload_leaves_match_original_mission_door_slots_and_rng() {
    let native = crate::test_fixture::json("tools/sidebar_oracle/factory_naval_output.json");
    assert_eq!(native["schema"], 1);
    let control = native["controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "stock_dest_no_rally")
        .expect("original stock GAYARD/DEST control");
    let calls: Vec<_> = control["calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|call| call["name"] == "mission_unload")
        .collect();
    assert_eq!(
        calls
            .iter()
            .map(|call| int(&call["frame"]))
            .collect::<Vec<_>>(),
        [0, 15]
    );
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("Hills.mmx")
    else {
        return;
    };
    let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let mut rules = retail.rules;
    crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    rules
        .bind_scheduler_anim_assets(
            &["GAYARD_A".into(), "GAYARD_C".into(), "GAYARD_D".into()],
            &assets,
            "tem",
            "TEMPERATE",
        )
        .expect("native-read slot names need their physical retail SHP bounds");

    let assert_boundary = |sim: &Simulation, yard: u64, expected: &Value| {
        let entity = sim.substrate.entities.get(yard).unwrap();
        let producer = &expected["producer"];
        assert_eq!(
            json!([
                entity.mission.current().raw(),
                entity.mission.queued().raw(),
                entity.mission.handler_state(),
                entity.mission.mission_start_frame(),
                entity.mission.ai_counter(),
            ]),
            json!([
                producer["current"],
                producer["queued"],
                producer["status"],
                producer["mission_start"],
                producer["ai_counter"],
            ]),
            "original current/queued mission and retained dispatch state"
        );
        let dispatch = entity.mission.dispatch_timer();
        assert_eq!(
            json!([dispatch.start_frame(), dispatch.delay()]),
            producer["dispatch_timer"],
            "the leaf returns a delay; the enclosing Mission AI owns its timer write"
        );
        assert_eq!(
            json!([
                entity.building_body_state().unwrap(),
                entity.queued_building_body_state().unwrap(),
                entity.building_ready_latch(),
            ]),
            json!([
                expected["bstate"],
                expected["queued_bstate"],
                expected["ready"]
            ]),
            "body mode and its sole ready byte are independent of mission status"
        );
        let flags = match entity.door_phase() {
            DoorPhase::ClosedStable => (0, 0),
            DoorPhase::Opening => (1, 1),
            DoorPhase::OpenStable => (0, 1),
            DoorPhase::Closing => (1, 0),
        };
        let (anchor, duration, total) = entity.door_timer_fields();
        assert_eq!(
            json!([flags.0, flags.1, anchor, duration, total]),
            json!([
                expected["door"]["active"],
                expected["door"]["opening"],
                expected["door"]["timer"][0],
                expected["door"]["timer"][1],
                expected["door"]["retained_duration"],
            ]),
            "original Door transition and meaningful retained timer fields"
        );
        for slot in expected["slots"].as_array().unwrap() {
            let index = int(&slot["slot"]) as usize;
            let actual = entity.building_art.anim_slots()[index]
                .map(|id| sim.interner.resolve(sim.anim(id).unwrap().type_id()));
            assert_eq!(json!(actual), slot["type"], "native slot{index}");
            if let Some(id) = entity.building_art.anim_slots()[index] {
                let anim = sim.anim(id).unwrap();
                let native_anim = expected["selected_anims"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|anim| anim["pointer"] == slot["pointer"])
                    .unwrap();
                let mut runtime = serde_json::to_value(&anim.runtime()).unwrap();
                for flag in [
                    "first_ai_guard",
                    "constructor_reverse",
                    "inactive",
                    "paused",
                ] {
                    runtime[flag] = json!(u8::from(runtime[flag].as_bool().unwrap()));
                }
                runtime["frame_timer"] = json!([
                    anim.runtime().frame_timer.start_frame(),
                    anim.runtime().frame_timer.duration(),
                ]);
                // The capture predates Anim+11A; no Temporal erase is live.
                assert_eq!(
                    runtime["temporal_paused"],
                    json!(false),
                    "native slot{index}"
                );
                runtime.as_object_mut().unwrap().remove("temporal_paused");
                assert_eq!(runtime, native_anim["runtime"], "native slot{index} clock");
            }
        }
    };

    for call in calls {
        assert_eq!(call["entry"], "0x44d880");
        let before = &call["entry_state"];
        let after = &call["return_state"];
        let now = int(&call["frame"]) as u32;
        let mut sim = Simulation::with_seed(0);
        // The joined original fixture supplies its stored speed separately
        // from retail lobby defaults. Normalized Anim constructor clocks
        // consume the value actually read at this native boundary.
        assert!(
            sim.session
                .game_options_mut_for_test()
                .apply_in_game_speed(int(&before["game_speed"]) as u8)
        );
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("Americans");
        sim.houses
            .register(HouseState::new(owner, 0, None, true, 50_000, 10));
        // Both receivers come from their real retail constructors. The DEST
        // starts held in Limbo; this leaf imports its measured post-Exit prior
        // rather than substituting a land tank or claiming to replay Exit.
        let yard = sim
            .construct_object_limbo_at_height(
                "GAYARD",
                "Americans",
                0,
                0,
                0,
                0,
                &rules,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        let product = sim
            .construct_object_limbo_at_height(
                "DEST",
                "Americans",
                0,
                0,
                0,
                0,
                &rules,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        for (id, actor) in [(yard, &before["producer"]), (product, &before["product"])] {
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity.position.put_location(coord(&actor["location"]));
            entity.health.set_for_test(int(&actor["health"]));
            entity
                .lifecycle
                .set_object_alive_for_test(actor["alive"] == 1);
            entity.lifecycle.set_in_limbo_for_test(actor["limbo"] == 1);
            entity
                .lifecycle
                .set_cell_marked_for_test(actor["marked"] == 1);
            entity.in_logic_vector.set_for_test(actor["logic"] == 1);
            entity.set_in_playfield(actor["in_playfield"] == 1);
            assert_eq!(actor["contacts"], json!([0]));
            assert!(entity.radio_contacts.is_empty());
            assert_eq!(
                json!(entity.radio_contacts.capacity()),
                actor["radio_capacity"]
            );
            assert_eq!(actor["tether"], 0);
            assert_eq!(entity.tether.partner(), None);
            entity.mission.apply_test_fixture(MissionTestFixture {
                current: MissionId::from_raw(int(&actor["current"])),
                suspended: MissionId::NONE,
                queued: MissionId::from_raw(int(&actor["queued"])),
                movement_bypass_latch: 0,
                handler_state: int(&actor["status"]) as u32,
                mission_start_frame: int(&actor["mission_start"]) as u32,
                ai_counter: int(&actor["ai_counter"]) as u32,
                dispatch_timer: MissionDispatchTimer::from_raw(
                    int(&actor["dispatch_timer"][0]),
                    int(&actor["dispatch_timer"][1]),
                ),
            });
        }
        {
            let entity = sim.substrate.entities.get_mut(yard).unwrap();
            assert_eq!(before["bstate"], 1);
            assert_eq!(before["queued_bstate"], -1);
            entity.initialize_building_idle_body(0);
            entity
                .mission_leaf
                .set_building_ready_latch(int(&before["ready"]) as u8);
            entity
                .building_art
                .set_actually_placed_for_test(before["lifecycle_bytes"]["6E4"] == 1);
            assert_eq!(
                json!(u8::from(entity.building_art.damage_state_active())),
                before["lifecycle_bytes"]["6E6"]
            );
            assert_eq!(
                json!(u8::from(entity.building_art.stuff_enabled())),
                before["lifecycle_bytes"]["6EA"]
            );
            if before["door"]["opening"] == 1 {
                entity.open_door(
                    int(&before["door"]["retained_duration"]) as u32,
                    int(&before["door"]["timer"][0]) as u32,
                );
                entity.advance_door(now);
            }
        }
        sim.substrate.occupancy =
            crate::sim::occupancy::OccupancyGrid::rebuild(&sim.substrate.entities);
        for slot in before["slots"].as_array().unwrap() {
            let Some(name) = slot["type"].as_str() else {
                continue;
            };
            let id = sim
                .set_building_anim_slot(yard, int(&slot["slot"]) as u8, false, false, 0, &rules)
                .unwrap();
            assert_eq!(sim.interner.resolve(sim.anim(id).unwrap().type_id()), name);
            let prior = before["selected_anims"]
                .as_array()
                .unwrap()
                .iter()
                .find(|anim| anim["pointer"] == slot["pointer"])
                .unwrap();
            let runtime = &prior["runtime"];
            let anim = sim.substrate.anims.get_mut(id).unwrap();
            anim.runtime_mut_for_test().current_frame = int(&runtime["current_frame"]);
            anim.runtime_mut_for_test().frame_step = int(&runtime["frame_step"]);
            anim.runtime_mut_for_test().delay_remaining = int(&runtime["delay_remaining"]) as u16;
            anim.runtime_mut_for_test().rate_reload = int(&runtime["rate_reload"]) as u16;
            anim.runtime_mut_for_test().frame_timer = CdTimer::from_raw(
                int(&runtime["frame_timer"][0]),
                int(&runtime["frame_timer"][1]),
            );
            anim.runtime_mut_for_test().loop_remaining = int(&runtime["loop_remaining"]) as u8;
            anim.runtime_mut_for_test().first_ai_guard = runtime["first_ai_guard"] == 1;
            anim.runtime_mut_for_test().constructor_reverse = runtime["constructor_reverse"] == 1;
            anim.runtime_mut_for_test().inactive = runtime["inactive"] == 1;
            anim.runtime_mut_for_test().paused = runtime["paused"] == 1;
        }
        sim.session.set_binary_frame_for_test(now);
        sim.main_rng =
            SimRng::from_native_state_hex_for_test(before["rng"]["main"].as_str().unwrap()).into();
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(before["rng"]["scenario"].as_str().unwrap());
        sim.mapgen_rng =
            SimRng::from_native_state_hex_for_test(before["rng"]["mapgen"].as_str().unwrap());
        assert_boundary(&sim, yard, before);
        let slots_before = *sim
            .substrate
            .entities
            .get(yard)
            .unwrap()
            .building_art
            .anim_slots();
        let production_before =
            slots_before[8].map(|id| (id, sim.anim(id).unwrap().runtime().clone()));
        let delay = mission_unload(&mut sim, yard, &rules, ObjectAiCtx::default()).unwrap();
        assert_eq!(
            json!(delay),
            call["return_eax"],
            "frame{now}: original handler delay"
        );
        assert_eq!(call["return_eax"], 15);
        assert_boundary(&sim, yard, after);
        for (name, actual) in [
            ("main", sim.main_rng.native_state_hex()),
            ("scenario", sim.scenario_rng.native_state_hex()),
            ("mapgen", sim.mapgen_rng.native_state_hex()),
        ] {
            assert!(
                actual == after["rng"][name].as_str().unwrap(),
                "frame{now}: exact original {name} RNG output"
            );
        }
        if let Some(idle) = slots_before[18] {
            assert!(
                sim.anim(idle).is_none(),
                "the displaced Idle18 is scalar-deleted"
            );
        }
        if let Some((id, runtime)) = production_before {
            assert_eq!(
                sim.substrate
                    .entities
                    .get(yard)
                    .unwrap()
                    .building_art
                    .anim_slots()[8],
                Some(id)
            );
            assert_eq!(
                *sim.anim(id).unwrap().runtime(),
                runtime,
                "state4 retains the actual running Production8"
            );
        }
    }
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
            .per_cell_process(
                fixture.product,
                PerCellReason::Arrival,
                &rules,
                None,
                crate::sim::world::FrameEffects::default(),
            )
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
        sim.session.set_binary_frame_for_test(now);
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
        entity
            .navigation
            .set_nav_com_for_test(nav(&before["navcell"]));
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
        assert_eq!(
            entity.navigation.nav_com(),
            nav(&after["navcell"]),
            "{name}"
        );
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

/// ClearBib449540 with a busy exit: the head's occupant takes the null
/// Scatter, then each object beside the head scatters away from the head
/// Cell's centre (`0x004495AD..0x0044968F`). The track sits in the head's
/// north-east corner, where the octants to the neighbours would turn, so for
/// every jitter draw a computer tank east of the head heads further east and
/// one north of it further north, each with a queued Move.
#[test]
fn a_busy_factory_exit_pushes_its_neighbours_away_from_the_head() {
    use crate::sim::mission::MissionType;
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=200\nSpeed=6\nSpeedType=Track\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
    ))
    .unwrap();
    for seed in 0..16 {
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, seed);
        let spawn = |sim: &mut Simulation, x, y| {
            let id = sim
                .spawn_object("MTNK", "Russians", x, y, 0, &rules)
                .unwrap();
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .mission
                .set_current_for_test(MissionId::from_known(MissionType::Guard), 0);
            id
        };
        spawn(&mut sim, 15, 15);
        let east = spawn(&mut sim, 16, 15);
        let north = spawn(&mut sim, 15, 14);
        let producer = sim.next_stable_object_id();
        let track = DriveCoord {
            x: 15 * 256 + 255,
            y: 15 * 256 + 1,
            z: 0,
        };
        assert!(super::clear_factory_exit_bib(
            &mut sim,
            producer,
            track,
            &rules,
            ObjectAiCtx::default(),
        ));
        let cell = |id: u64| match sim.substrate.entities.get(id).unwrap().navigation.nav_com() {
            Some(NavTargetRef::Cell { rx, ry }) => (rx, ry),
            other => panic!("seed {seed}: tank {id} took no cell: {other:?}"),
        };
        assert_eq!(
            cell(east).0,
            17,
            "seed {seed}: east of the head, further east"
        );
        assert_eq!(
            cell(north).1,
            13,
            "seed {seed}: north of the head, further north"
        );
        for id in [east, north] {
            assert_eq!(
                sim.substrate.entities.get(id).unwrap().mission.queued(),
                MissionId::from_known(MissionType::Move)
            );
        }
    }
}

/// Mission_Unload state 2 for a Teleport product (`0x0044DFAD..0x0044E163`):
/// the retail Chrono Miner takes a Drive piggyback over its Teleport and
/// leaves the bay on track 0x42, as a Drive product does
/// (`0x0044DF72..0x0044DFA4`); once it stops past the bay the Drive ends and
/// its Teleport is back.
#[test]
fn a_chrono_miner_leaves_the_war_factory_on_the_bay_track() {
    use crate::rules::locomotor_type::LocomotorKind;
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(&rules, 0x0044_DFAD, 48);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session
        .set_current_house_for_test(sim.interner.get("Americans"));
    let grid = sim
        .path_grid_snapshot()
        .map(|grid| (*grid).clone())
        .unwrap();
    let factory = sim
        .spawn_object("GAWEAP", "Americans", 20, 20, 0, &rules)
        .unwrap();
    // A rival building keeps the skirmish from ending in victory.
    sim.spawn_object("NAPOWR", "Russians", 14, 30, 0, &rules)
        .unwrap();
    // ExitObject's War Factory arm (`0x00444492`) places the product in the
    // bay, makes the factory its contact and queues the factory's Unload.
    let miner = sim
        .construct_object_limbo_at_height(
            "CMIN",
            "Americans",
            0,
            0,
            0,
            0,
            &rules,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    assert!(matches!(
        crate::sim::production::exit_produced_object(
            &mut sim,
            &rules,
            factory,
            miner,
            (0, 0),
            None,
            crate::sim::world::FrameEffects::default(),
        ),
        crate::sim::ai_base_building::BuildingExit::Placed
    ));
    let active = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(miner)
            .unwrap()
            .locomotor
            .as_ref()
            .unwrap()
            .active_kind()
    };
    let bay_track = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(miner)
            .unwrap()
            .locomotor
            .as_ref()
            .and_then(|loco| loco.selected_drive_runtime())
            .and_then(|drive| drive.retained())
            .is_some_and(|drive| drive.track().turn_index() == 0x42)
    };
    assert_eq!(active(&sim), LocomotorKind::Teleport);

    let mut on_track = false;
    for _ in 0..600 {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
        if active(&sim) == LocomotorKind::Drive && bay_track(&sim) {
            on_track = true;
            break;
        }
    }
    assert!(on_track, "the Chrono Miner runs the bay track on a Drive");

    // Past the bay the Drive stops and ends back onto the Teleport.
    let mut teleport_again = false;
    for _ in 0..600 {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
        if active(&sim) == LocomotorKind::Teleport {
            teleport_again = true;
            break;
        }
    }
    assert!(teleport_again, "the Drive piggyback ends past the bay");
    let unit = sim.substrate.entities.get(miner).unwrap();
    let (rx, ry) = (unit.position.rx(), unit.position.ry());
    assert!(
        sim.substrate.occupancy.first_building_on_layer(
            rx,
            ry,
            crate::sim::movement::locomotor::MovementLayer::Ground
        ) != Some(factory),
        "out of the bay at ({rx}, {ry})"
    );
}
