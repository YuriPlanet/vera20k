//! Original deployed Infantry Guard521320 / predicate522510 comparisons.
//!
//! The saved companion supplies actor/type/sequence/site state; it does not
//! execute a retail scenario loader. Whole returned shim rows are compared
//! through the production mission dispatcher, including its timer epilogue.
//! Accepted prefixes compare the real Direct FireAt entry observation, before
//! base6FDD50. Callback rows supplied only that native base return: their Guard
//! and class suffix effects are compared separately from real Bullet effects
//! and RNG. Direct Bullet delivery has its own combat regression coverage.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde_json::{Value, json};

use super::dispatch_foot_mission;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::receiver_fixture::trace_fire_visits;
use crate::sim::combat::{AttackTarget, TargetKind};
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::mission::leaf::MissionLeafState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionTimer};
use crate::sim::movement::ground_pose;
use crate::sim::movement::locomotion::piggyback::{LocomotorRuntimePayload, WalkRuntime};
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::radiation::{RadSite, RadiationState};
use crate::sim::rng::SimRng;
use crate::sim::stage::StageClass;
use crate::sim::timer::CdTimer;
use crate::sim::world::{ObjectAiCtx, Simulation};

fn corpus() -> &'static Value {
    static CORPUS: OnceLock<Value> = OnceLock::new();
    CORPUS.get_or_init(|| {
        let meta: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/infantry_deployed_guard.meta.json",
        ))
        .unwrap();
        assert_eq!(
            meta["native_sha256"],
            "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
        );
        let value: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/infantry_deployed_guard.json",
        ))
        .unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["guard_rows"].as_array().unwrap().len(), 64);
        assert_eq!(value["predicate_rows"].as_array().unwrap().len(), 47);
        value
    })
}

fn signed(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn coordinate(value: &Value) -> DriveCoord {
    DriveCoord {
        x: signed(&value[0]),
        y: signed(&value[1]),
        z: signed(&value[2]),
    }
}

fn nonnull_coordinate(value: &Value) -> Option<DriveCoord> {
    let coord = coordinate(value);
    (coord != DriveCoord { x: 0, y: 0, z: 0 }).then_some(coord)
}

/// Mechanical identity translation for the companion's two supplied Cells.
/// Their coordinates are explicit in the existing native fixture, not inferred
/// from native pointer arithmetic or VERA's target-selection behavior.
fn target(pointer: &Value, row: &Value) -> Option<TargetKind> {
    let pointer = pointer.as_u64().unwrap();
    let first = &corpus()["guard_rows"][0];
    if pointer == 0 {
        None
    } else if row["input"]["target_kind"] == "entity"
        && pointer == row["guard_before"]["target"].as_u64().unwrap()
    {
        Some(TargetKind::Entity(2))
    } else if row["input"]["target_kind"] == "cell"
        && pointer == row["guard_before"]["target"].as_u64().unwrap()
    {
        Some(TargetKind::Cell(15, 10))
    } else if pointer == first["query_order"][0]["returned_cell"].as_u64().unwrap() {
        Some(TargetKind::Cell(10, 10))
    } else {
        assert_eq!(pointer, first["guard_before"]["target"].as_u64().unwrap());
        Some(TargetKind::Cell(11, 10))
    }
}

/// Native controls supply these fields directly. Transport their authored
/// values through the sole production Type, weapon, mission and ART readers.
fn supplied_rules(input: &Value) -> RuleSet {
    let name = input["type_id"].as_str().unwrap();
    let mission = MissionId::from_raw(signed(&input["mission"]))
        .known()
        .unwrap();
    let range = f64::from(signed(&input["range"])) / 256.0;
    let secondary_range = input["secondary_range"]
        .as_i64()
        .map_or(range, |value| value as f64 / 256.0);
    let deploy_fire_weapon = i32::from(input["secondary_range"].is_number());
    let jumpjet = input["jumpjet"].as_i64().unwrap_or(0);
    let area_fire = i32::from(!input["secondary_range"].is_number());
    let ini = IniFile::from_str(&format!(
        "[AI]\nBlockagePathDelay={}\n[InfantryTypes]\n0={name}\n\
         [{name}]\nStrength=100\nSpeed=4\n\
         Locomotor={{4A582744-9839-11D1-B709-00A024DDAFD1}}\n\
         MobileFire=yes\nDeployer={}\nDeployedCrushable=no\nDeployFire={}\n\
         UndeployDelay={}\nImmuneToRadiation={}\nDeployFireWeapon={deploy_fire_weapon}\nJumpJet={jumpjet}\n\
         Primary=SUPPLIED0\nSecondary=SUPPLIED1\n\
         [{}]\nRate={}\n\
         [SUPPLIED0]\nDamage=1\nSpeed=50\nROF=50\nAreaFire={area_fire}\n\
         Projectile=SUPPLIED_PROJECTILE\nWarhead=SUPPLIED_WARHEAD\nRange={range}\nRadLevel={}\n\
         [SUPPLIED1]\nDamage=1\nSpeed=50\nROF=50\nAreaFire={area_fire}\n\
         Projectile=SUPPLIED_PROJECTILE\nWarhead=SUPPLIED_WARHEAD\nRange={secondary_range}\nRadLevel={}\n\
         [SUPPLIED_PROJECTILE]\nAA={area_fire}\nAG=yes\n\
         [SUPPLIED_WARHEAD]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        input["blockage_path_delay"],
        input["deployer"],
        input["deploy_fire"],
        input["undeploy_delay"],
        input["immune"],
        mission.ini_section(),
        input["mission_rate"],
        input["rad_level"],
        input["rad_level"],
    ));
    let mut counts = [signed(&input["count"]); 42];
    for row in input["counts"].as_array().unwrap() {
        counts[row[0].as_u64().unwrap() as usize] = signed(&row[1]);
    }
    let mut text = format!("[{name}]\nSequence=SuppliedSequence\n[SuppliedSequence]\n");
    for (action, count) in crate::rules::infantry_sequence::NATIVE_SEQUENCE_NAMES
        .iter()
        .zip(counts)
    {
        text.push_str(&format!("{action}=100,{count},6\n"));
    }
    let art = IniFile::from_str(&text);
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
    );
    let object = rules.object(name).unwrap();
    assert_eq!(object.deploy_fire, input["deploy_fire"] != 0);
    assert_eq!(object.immune_to_radiation, input["immune"] != 0);
    assert_eq!(object.undeploy_delay, signed(&input["undeploy_delay"]));
    assert_eq!(
        object.ammo, -1,
        "live Ammo0 is a separate generic Techno owner residual"
    );
    let weapon = rules.weapon("SUPPLIED1").unwrap();
    assert_eq!(
        weapon.range_leptons,
        input["secondary_range"]
            .as_i64()
            .unwrap_or_else(|| input["range"].as_i64().unwrap()) as i32
    );
    assert_eq!(weapon.rad_level, signed(&input["rad_level"]));
    for (action, count) in counts.into_iter().enumerate() {
        assert_eq!(
            rules
                .animation_sequence(name)
                .unwrap()
                .infantry_action(action as i32)
                .unwrap()
                .frames_per_facing,
            count
        );
    }
    rules
}

fn supplied_fixture(row: &Value) -> (Simulation, RuleSet, u64) {
    let input = &row["input"];
    let before = &row["guard_before"];
    let rules = supplied_rules(input);
    let mut sim = Simulation::with_seed(input["seed"].as_u64().unwrap());
    sim.session.binary_frame = input["now"].as_u64().unwrap() as u32;
    sim.session.game_options.game_speed = signed(&input["game_speed_index"]);
    let owner = sim.interner.intern("SuppliedHouse");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, false, 0, 10));
    let id = sim
        .construct_object_limbo_at_height(
            input["type_id"].as_str().unwrap(),
            "SuppliedHouse",
            10,
            10,
            0,
            0,
            &rules,
        )
        .unwrap();
    let mut terrain = crate::map::resolved_terrain::test_grid(32, 32, |x, y| {
        let mut cell = crate::map::resolved_terrain::test_flat_cell(x, y);
        cell.land_type = input["land"].as_u64().unwrap() as u8;
        cell.yr_cell_land_type = cell.land_type;
        cell
    });
    let current = terrain.native_cell_identity((10, 10));
    terrain.write_native_cell_flags(current, input["cell_flags"].as_u64().unwrap() as u32);
    sim.resolved_terrain = Some(terrain);
    if input["site"].is_object() {
        let mut sites = BTreeMap::new();
        sites.insert(
            (10u16, 10u16),
            RadSite {
                center: (10, 10),
                spread: 0,
                radius_leptons: 128,
                level: signed(&input["site"]["level"]),
                level_steps: 0,
                duration: signed(&input["site"]["duration"]),
                remaining: signed(&input["site"]["remaining"]),
                level_timer: CdTimer::default(),
            },
        );
        // Supplied retained site words, transported with the owner's existing
        // serialization. No detonation/activation producer is certified here.
        let cells = BTreeMap::<(u16, u16), f64>::new();
        sim.radiation =
            bincode::deserialize::<RadiationState>(&bincode::serialize(&(cells, sites)).unwrap())
                .unwrap();
    }
    let actor = sim.substrate.entities.get_mut(id).unwrap();
    actor.health.current = 100;
    actor.lifecycle.in_limbo = false;
    actor.lifecycle.object_alive = true;
    // The companion supplies a post-constructor Location at Cell10,10;
    // transport the complete saved XYZ through the existing coordinate owner.
    ground_pose::put_location(&mut actor.position, coordinate(&before["position"]));
    actor.on_bridge = input["bridge"] != 0;
    actor.set_falling_down_for_test(input["falling"].as_i64().unwrap_or(0) != 0);
    actor.infantry.as_mut().unwrap().is_prone = before["prone"] != 0;
    actor.mission_leaf = MissionLeafState::infantry_raw_for_test(
        signed(&before["firing"]) as u8,
        signed(&before["doing"]),
    );
    actor
        .mission_leaf
        .set_infantry_pending_deploy(signed(&before["pending"]) as u8);
    actor.set_infantry_deploy_crush_immunity(signed(&before["crush"]) as u8);
    actor.install_native_stage_fixture(StageClass::from_native_fixture(
        signed(&before["frame"]),
        signed(&before["changed"]) as u8,
        CdTimer::from_raw(signed(&before["stage"][0]), signed(&before["stage"][1])),
        signed(&before["stage"][2]),
        signed(&before["stage"][3]),
    ));
    actor.passive_scan_timer = MissionTimer::armed(
        signed(&before["reload"][0]) as u32,
        signed(&before["reload"][1]) as u32,
    );
    actor.rearm_timer = CdTimer::from_raw(signed(&before["rearm"][0]), signed(&before["rearm"][1]));
    actor.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_raw(signed(&before["mission"])),
        suspended: MissionId::from_raw(signed(&before["suspended"])),
        queued: MissionId::from_raw(signed(&before["queued"])),
        movement_bypass_latch: 0,
        handler_state: signed(&before["handler_state"]) as u32,
        mission_start_frame: signed(&before["mission_start"]) as u32,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    });
    actor.attack_target = target(&before["target"], row).map(|target| AttackTarget { target });
    actor.navigation.nav_com = target(&before["nav"], row).map(|target| match target {
        TargetKind::Cell(x, y) => NavTargetRef::cell(x, y),
        TargetKind::Entity(_) => unreachable!(),
    });
    let mut loco = LocomotorState::from_object_type(
        rules.object(input["type_id"].as_str().unwrap()).unwrap(),
        0,
    );
    // The supplied Walk has a retained destination with moving=0. Calling
    // MoveTo would invent a transition and overwrite that native premise.
    loco.runtime_payload = LocomotorRuntimePayload::Walk(WalkRuntime {
        head: nonnull_coordinate(&before["head"]),
        destination: nonnull_coordinate(&before["destination"]),
        moving: before["moving"] != 0,
        animation_moving: before["motion"] != 0,
    });
    actor.locomotor = Some(loco);
    actor
        .foot_speed
        .set_speed_fraction_native_bits(input["speed_fraction"].as_f64().unwrap().to_bits());
    if input["target"] == true && input["target_kind"] == "entity" {
        let mut supplied_target = actor.clone();
        supplied_target.stable_id = 2;
        let mut target_coord = ground_pose::position_world_coord(&actor.position);
        target_coord.x = target_coord.x.wrapping_add(signed(&input["delta"]));
        ground_pose::put_location(&mut supplied_target.position, target_coord);
        supplied_target.attack_target = None;
        supplied_target.mission_leaf = MissionLeafState::infantry_raw_for_test(0, 0);
        sim.substrate.entities.insert(supplied_target);
    }
    // All three original Random2Class objects execute Seed(input.seed) after
    // the supplied actor is prepared. Constructor draws are outside this seam.
    sim.main_rng = SimRng::new(input["seed"].as_u64().unwrap());
    sim.scenario_rng = SimRng::new(input["seed"].as_u64().unwrap());
    sim.mapgen_rng = SimRng::new(input["seed"].as_u64().unwrap());
    sim.resolve_type_handles(&rules);
    assert_actor(sim.substrate.entities.get(id).unwrap(), before, row, true);
    assert_rng(
        &sim,
        &row["rng_streams_before"],
        input["name"].as_str().unwrap(),
    );
    (sim, rules, id)
}

fn assert_actor(actor: &GameEntity, expected: &Value, row: &Value, rearm: bool) {
    let name = row["input"]["name"].as_str().unwrap();
    let stage = actor.native_stage();
    let raw = serde_json::to_value(stage).unwrap();
    let loco = actor.locomotor.as_ref().unwrap();
    let actual = json!({
        "doing": actor.mission_leaf.as_infantry().unwrap().doing(),
        "pending": actor.mission_leaf.as_infantry().unwrap().pending_deploy(),
        "firing": actor.mission_leaf.foot_firing_sequence_latch(),
        "prone": u8::from(actor.infantry.as_ref().unwrap().is_prone),
        "crush": actor.native_crush_immunity(), "frame": stage.value(), "changed": raw["changed"],
        "stage": [stage.timer().start_frame(), stage.timer().duration(), stage.rate(), signed(&raw["increment"])],
        "reload": [actor.passive_scan_timer.start_frame as i32, actor.passive_scan_timer.duration as i32],
        "mission": actor.mission.current().raw(), "suspended": actor.mission.suspended().raw(),
        "queued": actor.mission.queued().raw(), "handler_state": actor.mission.handler_state(),
        "mission_start": actor.mission.mission_start_frame(),
        "on_bridge": u8::from(actor.on_bridge),
        "position": [i32::from(actor.position.rx)*256+actor.position.sub_x.to_num::<i32>(), i32::from(actor.position.ry)*256+actor.position.sub_y.to_num::<i32>(), actor.position.exact_z_leptons.unwrap()],
        "moving": u8::from(loco.walk_is_moving().unwrap()), "motion": u8::from(loco.walk_animation_moving().unwrap()),
    });
    for (key, value) in actual.as_object().unwrap() {
        assert_eq!(value, &expected[key], "{name}: {key}");
    }
    assert_eq!(
        loco.step_head(),
        nonnull_coordinate(&expected["head"]),
        "{name}: paid head"
    );
    assert_eq!(
        loco.walk_destination(),
        nonnull_coordinate(&expected["destination"]),
        "{name}: retained destination"
    );
    assert_eq!(
        actor.attack_target.as_ref().map(|a| a.target),
        target(&expected["target"], row),
        "{name}: TarCom"
    );
    let nav = target(&expected["nav"], row).map(|target| match target {
        TargetKind::Cell(x, y) => NavTargetRef::cell(x, y),
        TargetKind::Entity(_) => unreachable!(),
    });
    assert_eq!(actor.navigation.nav_com, nav, "{name}: NavCom");
    assert!(actor.archive_target().is_none(), "{name}: archive");
    assert_eq!(expected["archive"], 0);
    if rearm {
        assert_eq!(
            json!([
                actor.rearm_timer.start_frame(),
                actor.rearm_timer.duration()
            ]),
            expected["rearm"],
            "{name}: rearm"
        );
    }
}

fn observation<'a>(observations: &'a [Value], phase: &str, row: &Value) -> &'a Value {
    let mut matches = observations.iter().filter(|state| state["phase"] == phase);
    let result = matches
        .next()
        .unwrap_or_else(|| panic!("{}: missing {phase} observation", row["input"]["name"]));
    assert!(
        matches.next().is_none(),
        "{}: duplicate {phase} visit",
        row["input"]["name"]
    );
    result
}

fn rng_streams(sim: &Simulation) -> Value {
    json!({"main": sim.main_rng.native_state_hex(), "scenario": sim.scenario_rng.native_state_hex(), "mapgen": sim.mapgen_rng.native_state_hex()})
}

fn assert_rng(sim: &Simulation, expected: &Value, name: &str) {
    let actual = rng_streams(sim);
    for stream in ["main", "scenario", "mapgen"] {
        assert_eq!(
            actual[stream], expected[stream],
            "{name}: complete {stream} RNG"
        );
    }
}

#[test]
fn native_gi_reacquire_uses_deployed_weapon_range_and_clears_firing_latch() {
    let data = super::automatic_deploy_oracle_tests::corpus();
    let rows = data["reacquire_rows"].as_array().unwrap();
    assert_eq!(rows.len(), 10);
    for row in rows {
        let (mut sim, rules, id) = supplied_fixture(row);
        dispatch_foot_mission(&mut sim, id, &rules, ObjectAiCtx::default());
        assert_actor(
            sim.substrate.entities.get(id).unwrap(),
            &row["guard_after"],
            row,
            true,
        );
        assert_rng(
            &sim,
            &row["rng_streams_after"],
            row["input"]["name"].as_str().unwrap(),
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .mission
                .dispatch_timer(),
            MissionDispatchTimer::from_raw(
                sim.session.binary_frame as i32,
                signed(&row["return_signed"])
            )
        );
        assert_eq!(row["execution"]["gameplay_return_supplied"], false);
        assert_eq!(row["original_text_and_vtables_unchanged"], true);
    }
}

#[test]
fn stock_deployed_guard_readers_bind_physical_type_weapon_and_raw_action_gates() {
    let Some(rules_bytes) = crate::rules::retail_ini_fixture::retail_ini_bytes("rulesmd.ini")
    else {
        return;
    };
    let Some(art_bytes) = crate::rules::retail_ini_fixture::retail_ini_bytes("artmd.ini") else {
        return;
    };
    for (name, bytes) in [("RULESMD.INI", &rules_bytes), ("ARTMD.INI", &art_bytes)] {
        assert_eq!(
            crate::util::sha256::sha256_hex(bytes),
            corpus()["retail_lexical_inputs"][name]["sha256"]
                .as_str()
                .unwrap(),
            "the native companion records these physical lexical inputs"
        );
    }
    let ini = IniFile::from_bytes(&rules_bytes).unwrap();
    let art = IniFile::from_bytes(&art_bytes).unwrap();
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
    );
    // Physical values are separate from the synthetic signed-record controls.
    // This checks their production binding; the companion does not execute INI
    // loading and cannot certify the complete retail reader/scenario chain.
    for (name, immune, undeploy, action, count) in [
        ("DESO", true, -1, 29, 7),
        ("YURI", false, 150, 31, 6),
        ("YURIPR", false, 75, 31, 6),
    ] {
        let object = rules.object(name).unwrap();
        assert!(object.deployer && object.deploy_fire);
        assert_eq!(object.immune_to_radiation, immune, "{name}");
        assert_eq!(object.undeploy_delay, undeploy, "{name}");
        assert_eq!(object.ammo, -1, "{name}");
        assert_eq!(
            rules
                .animation_sequence(name)
                .unwrap()
                .infantry_action(action)
                .unwrap()
                .frames_per_facing,
            count,
            "{name}: physical ART raw action count"
        );
    }
    let deso = rules.object("DESO").unwrap();
    assert_eq!(deso.secondary.as_deref(), Some("RadEruptionWeapon"));
    let eruption = rules.weapon("RadEruptionWeapon").unwrap();
    assert_eq!(eruption.range_leptons, 1024);
    assert_eq!(eruption.rad_level, 500);
    assert!(eruption.area_fire);
    assert!(!eruption.is_rad_eruption);
    assert_eq!(eruption.projectile.as_deref(), Some("InvisibleLow"));
    assert_eq!(eruption.warhead.as_deref(), Some("RadEruptionWarhead"));
    assert!(rules.projectile("InvisibleLow").unwrap().inviso);
    assert_eq!(
        rules.warhead("RadEruptionWarhead").unwrap().cell_spread_f64,
        10.0
    );
}

#[test]
fn native_deployed_guard_predicate_matches_all47_signed_controls() {
    for row in corpus()["predicate_rows"].as_array().unwrap() {
        let (sim, _rules, id) = supplied_fixture(row);
        let actor = sim.substrate.entities.get(id).unwrap();
        assert_eq!(
            i32::from(actor.infantry_deploy_doing()),
            signed(&row["return_signed"]),
            "{}",
            row["input"]["name"]
        );
        assert_actor(actor, &row["guard_after"], row, true);
        assert_rng(
            &sim,
            &row["rng_streams_after"],
            row["input"]["name"].as_str().unwrap(),
        );
    }
}

/// Native full-return controls execute no omitted base FireAt. −1 is the shim
/// decline sentinel; its wrapper's Foot continuation is not in this receipt.
#[test]
fn native_deployed_guard_full_returns_match_state_cadence_and_three_rng_streams() {
    let mut compared = 0;
    let mut declined = 0;
    let mut unsupported = Vec::new();
    for row in corpus()["guard_rows"].as_array().unwrap() {
        if row["execution"]["returned"] != true
            || row["execution"]["gameplay_return_supplied"] == true
        {
            continue;
        }
        let name = row["input"]["name"].as_str().unwrap();
        if matches!(
            name,
            "fire_error_ammo_zero"
                | "fire_error_speed_above_point_one"
                | "deployed_DeployFire_false"
        ) {
            unsupported.push(name);
            continue;
        }
        let (mut sim, rules, id) = supplied_fixture(row);
        let (_, observations) = trace_fire_visits(|| {
            dispatch_foot_mission(&mut sim, id, &rules, ObjectAiCtx::default())
        });
        assert!(
            observations.iter().all(|state| state["phase"] != "entry"),
            "{name}: FireAt refused or bypassed"
        );
        let shim = observation(&observations, "guard-shim-return", row);
        let shim_actor: GameEntity = serde_json::from_value(shim["actor"].clone()).unwrap();
        assert_actor(&shim_actor, &row["guard_after"], row, true);
        assert_eq!(
            shim["rng_streams"], row["rng_streams_after"],
            "{name}: shim full three RNG objects"
        );
        let actor = sim.substrate.entities.get(id).unwrap();
        if signed(&row["return_signed"]) == -1 {
            // This full original shim row ends before Guard's Foot fallback.
            // Its action and RNG remain comparable at the observed boundary;
            // the separate wrapper continuation must not keep delay−1.
            assert_ne!(
                actor.mission.dispatch_timer().delay(),
                -1,
                "{name}: Foot fallback epilogue"
            );
            declined += 1;
            continue;
        }
        assert_actor(actor, &row["guard_after"], row, true);
        assert_eq!(
            actor.mission.dispatch_timer(),
            MissionDispatchTimer::from_raw(
                sim.session.binary_frame as i32,
                signed(&row["return_signed"])
            ),
            "{name}: Mission dispatcher epilogue"
        );
        assert_rng(&sim, &row["rng_streams_after"], name);
        compared += 1;
    }
    assert_eq!((compared, declined), (36, 1));
    assert_eq!(
        unsupported,
        [
            "fire_error_ammo_zero",
            "fire_error_speed_above_point_one",
            "deployed_DeployFire_false"
        ]
    );
}

#[test]
fn native_deployed_guard_accepted_prefix_matches_actual_fireat_entry() {
    let mut compared = 0;
    for row in corpus()["guard_rows"].as_array().unwrap() {
        if row["execution"]["returned"] == true {
            continue;
        }
        assert_eq!(row["execution"]["stop"], "006FDD50");
        assert!(row["return_signed"].is_null());
        let (mut sim, rules, id) = supplied_fixture(row);
        let (_, observations) = trace_fire_visits(|| {
            dispatch_foot_mission(&mut sim, id, &rules, ObjectAiCtx::default())
        });
        let entry = observation(&observations, "entry", row);
        let _return = observation(&observations, "return", row);
        let _shim = observation(&observations, "guard-shim-return", row);
        let actor: GameEntity = serde_json::from_value(entry["actor"].clone()).unwrap();
        assert_actor(&actor, &row["guard_after"], row, true);
        assert_eq!(
            entry["rng_streams"], row["rng_streams_after"],
            "{}: prefix full three RNG objects",
            row["input"]["name"]
        );
        compared += 1;
    }
    assert_eq!(compared, 13);
}

/// The native callback omits every base6FDD50 effect/draw. Only the Guard and
/// class suffix is comparable; real Bullet emission remains enabled in Rust.
#[test]
fn native_deployed_guard_callback_tail_matches_projected_class_effects() {
    let mut compared = 0;
    let mut declined = 0;
    for row in corpus()["guard_rows"].as_array().unwrap() {
        if row["execution"]["gameplay_return_supplied"] != true {
            continue;
        }
        assert_eq!(row["execution"]["base_fire_callback_count"], 1);
        let (mut sim, rules, id) = supplied_fixture(row);
        let (_, observations) = trace_fire_visits(|| {
            dispatch_foot_mission(&mut sim, id, &rules, ObjectAiCtx::default())
        });
        let _entry = observation(&observations, "entry", row);
        let _return = observation(&observations, "return", row);
        let shim = observation(&observations, "guard-shim-return", row);
        let actor: GameEntity = serde_json::from_value(shim["actor"].clone()).unwrap();
        assert_actor(&actor, &row["guard_after"], row, false);
        if signed(&row["return_signed"]) == -1 {
            // Native521320 returns−1 after its effects; Guard then runs Foot.
            // This receipt stopped before that separate wrapper continuation.
            declined += 1;
        } else {
            assert_eq!(
                sim.substrate
                    .entities
                    .get(id)
                    .unwrap()
                    .mission
                    .dispatch_timer()
                    .delay(),
                signed(&row["return_signed"]),
                "{}: signed Count29 cadence",
                row["input"]["name"]
            );
        }
        compared += 1;
    }
    assert_eq!((compared, declined), (11, 1));
}

#[test]
fn native_deployed_guard_unsupported_controls_remain_declared() {
    let rows = corpus()["guard_rows"].as_array().unwrap();
    let ammo = rows
        .iter()
        .find(|row| row["input"]["name"] == "fire_error_ammo_zero")
        .unwrap();
    let (sim, _rules, id) = supplied_fixture(ammo);
    // Original6FCA0D reads Techno+2FC; current FireSubject reads only the
    // AircraftAmmo owner and maps Infantry's absent component to−1. Generic
    // finite Techno ammo lifecycle is required separately; never fake it here.
    assert_eq!(ammo["guard_before"]["ammo"], 0);
    assert!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .aircraft_ammo
            .is_none()
    );
    let speed = rows
        .iter()
        .find(|row| row["input"]["name"] == "fire_error_speed_above_point_one")
        .unwrap();
    let (sim, _rules, id) = supplied_fixture(speed);
    // The supplied native double is one ULP above0.1; the existing documented
    // fixed-point speed owner cannot retain that control's admission boundary.
    assert!(speed["input"]["speed_fraction"].as_f64().unwrap() > 0.1);
    assert!(
        !sim.substrate
            .entities
            .get(id)
            .unwrap()
            .foot_speed
            .above_tenth()
    );
}
