//! Original E1 whole-AI firing observations at the existing Infantry host.
//!
//! Native owner: tools/spatial_oracle/anytown_damage/foot_missions.py and its
//! saved ground_firing_receipt/ground_emission_receipt. These fixtures transport
//! successful constructor/Unlimbo registrations and explicitly supplied clocks,
//! poses, Houses and reader state. They do not establish full Scenario loading,
//! all-object scheduling, bridge placement or rendered/audio-device behavior.

use super::foot_mission_oracle_tests::{
    SuppliedFootFixture, assert_rng, install_recorded_cell_coordinates, oracle, retail_rules,
    signed, xyz,
};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::Health;
use crate::sim::combat::fire_error_world::FireSubject;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::authority::EntityReadyInputProvider;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId};
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::projectile::ProjectileTarget;
use crate::sim::stage::StageClass;
use crate::sim::timer::CdTimer;
use crate::sim::world::display_layers::DisplayLayer;
use crate::sim::world::lifecycle::PointerExpiryControl;
use crate::sim::world::{ObjectAiCtx, Simulation};
use crate::util::fixed_math::SimFixed;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Only field/identity transport into the shared constructor fixture. Native
/// expected outputs are read directly from the saved receipt, never recomputed.
fn source_as_mission_input(source: &Value) -> Value {
    let mut before = oracle()["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "E1_area_guard_post")
        .unwrap()["before"]
        .clone();
    for key in [
        "doing",
        "frame_f8",
        "sequence_timer_words",
        "prone_6db",
        "position",
        "on_bridge",
        "mission",
        "queued",
        "status",
        "target",
        "archive",
        "nav",
    ] {
        before[key] = source[key].clone();
    }
    before["visit"] = source["mission_visit"].clone();
    before["firing"] = source["firing_68d"].clone();
    before["loco_head"] = source["walk_head"].clone();
    before["primary_facing_words"] = source["body_facing_words"].clone();
    before["dispatch"] = json!([
        source["dispatch_timer_words"][0],
        source["dispatch_timer_words"][2],
    ]);
    before["idle_timer"] = source["idle_timer_words"].clone();
    before["targeting_timer"] = source["targeting_timer_words"].clone();
    before["scan"] = json!(0);
    before
}

fn firing_fixture(case: &Value, rules: &RuleSet) -> SuppliedFootFixture {
    let mut row = oracle()["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "E1_area_guard_post")
        .unwrap()
        .clone();
    row["input"]["name"] = case["input"]["name"].clone();
    row["input"]["candidate_live"] = json!(true);
    row["input"]["candidate_xyz"] = case["target_before"]["position"].clone();
    row["before"] = source_as_mission_input(&case["actor_before"]);
    row["rng_before"] = case["rng_initial"].clone();
    let mut fixture = SuppliedFootFixture::new(&row, rules);
    install_recorded_cell_coordinates(&mut fixture);
    let actor = fixture
        .sim
        .substrate
        .entities
        .get_mut(fixture.actor)
        .unwrap();
    let before = &case["actor_before"];
    let words = &before["stage_timer_words"];
    actor.install_native_stage_fixture(StageClass::from_native_fixture(
        signed(&before["frame_f8"]),
        signed(&before["stage_changed_fc"]) as u8,
        CdTimer::from_raw(signed(&words[0]), signed(&words[2])),
        signed(&words[3]),
        signed(&words[4]),
    ));
    actor.rearm_timer.set_for_test(CdTimer::from_raw(
        signed(&before["rearm_timer_words"][0]),
        signed(&before["rearm_timer_words"][2]),
    ));
    actor.health = Health::new(signed(&before["health"]));
    actor.lifecycle.set_in_limbo_for_test(before["limbo"] != 0);
    actor
        .lifecycle
        .set_object_alive_for_test(before["alive"] != 0);
    let target_id = fixture.id(&case["input"]["target"]).unwrap();
    let target = fixture.sim.substrate.entities.get_mut(target_id).unwrap();
    let before = &case["target_before"];
    let position = xyz(&before["position"]);
    target.position.set_rx_for_test((position[0] / 256) as u16);
    target.position.set_ry_for_test((position[1] / 256) as u16);
    target
        .position
        .set_sub_x_for_test(SimFixed::from_num(position[0] % 256));
    target
        .position
        .set_sub_y_for_test(SimFixed::from_num(position[1] % 256));
    target
        .position
        .set_exact_z_leptons_for_test(Some(position[2]));
    target.set_on_bridge(before["on_bridge"] != 0);
    target.health = Health::new(signed(&before["health"]));
    target.lifecycle.set_in_limbo_for_test(before["limbo"] != 0);
    target
        .lifecycle
        .set_object_alive_for_test(before["alive"] != 0);
    target.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_raw(signed(&before["mission"])),
        suspended: MissionId::NONE,
        queued: MissionId::from_raw(signed(&before["queued"])),
        movement_bypass_latch: 0,
        handler_state: signed(&before["status"]) as u32,
        mission_start_frame: 0,
        ai_counter: signed(&before["visit"]) as u32,
        dispatch_timer: MissionDispatchTimer::from_raw(
            signed(&before["dispatch"][0]),
            signed(&before["dispatch"][1]),
        ),
    });
    // These supplied flags follow successful ground Unlimbo. Preserve the
    // original ground object heads and EMPTY upper lists, including deck rows.
    for pose in case["poses"].as_array().unwrap() {
        let id = fixture.id(&pose["actor"]).unwrap();
        let actor = fixture.sim.substrate.entities.get_mut(id).unwrap();
        if !pose["supplied_xyz"].is_null() {
            set_recorded_position(actor, &pose["supplied_xyz"]);
        }
        actor.set_on_bridge(pose["supplied_on_bridge"] != 0);
        let cell = &pose["cell_after"];
        let x = signed(&cell["cell"][0]) as i16;
        let y = signed(&cell["cell"][1]) as i16;
        let terrain = fixture.sim.resolved_terrain.as_mut().unwrap();
        let identity = terrain.native_cell_identity((x, y));
        terrain.write_native_cell_flags(identity, cell["flags"].as_u64().unwrap() as u32);
        let occupants = fixture
            .sim
            .substrate
            .occupancy
            .get(x as u16, y as u16)
            .unwrap();
        assert_eq!(
            occupants.first_on_layer(MovementLayer::Ground),
            fixture.id(&pose["ground_object_head"])
        );
        assert_eq!(
            occupants.first_on_layer(MovementLayer::Bridge),
            fixture.id(&pose["upper_object_head"])
        );
    }
    fixture
}

fn assert_source(fixture: &SuppliedFootFixture, expected: &Value, name: &str) {
    let actor = fixture.sim.substrate.entities.get(fixture.actor).unwrap();
    let leaf = actor.mission_leaf.as_infantry().unwrap();
    let stage = actor.native_stage();
    let words = &expected["stage_timer_words"];
    let position = position_world_coord(&actor.position);
    assert_eq!(
        [position.x, position.y, position.z],
        xyz(&expected["position"]),
        "{name}: physical source XYZ"
    );
    assert_eq!(
        actor.on_bridge(),
        expected["on_bridge"] != 0,
        "{name}: source layer"
    );
    assert_eq!(
        actor.infantry.as_ref().unwrap().is_prone(),
        expected["prone_6db"] != 0,
        "{name}: prone"
    );
    assert_eq!(leaf.doing(), signed(&expected["doing"]), "{name}: Doing");
    assert_eq!(
        actor.mission_leaf.foot_firing_sequence_latch(),
        signed(&expected["firing_68d"]) as u8,
        "{name}: raw Foot68D"
    );
    assert_eq!(stage.value(), signed(&expected["frame_f8"]), "{name}: F8");
    let serialized = serde_json::to_value(stage).unwrap();
    assert_eq!(
        serialized["changed"], expected["stage_changed_fc"],
        "{name}: FC"
    );
    assert_eq!(serialized["increment"], words[4], "{name}: increment");
    assert_eq!(
        (
            stage.timer().start_frame(),
            stage.timer().duration(),
            stage.rate()
        ),
        (signed(&words[0]), signed(&words[2]), signed(&words[3])),
        "{name}: shared signed Stage clock"
    );
    // +104 is caller stack residue, excluded by the shared timer owner.
    assert_eq!(
        actor.mission.current().raw(),
        signed(&expected["mission"]),
        "{name}: mission"
    );
    assert_eq!(
        actor.mission.queued().raw(),
        signed(&expected["queued"]),
        "{name}: queue"
    );
    assert_eq!(
        actor.mission.handler_state(),
        signed(&expected["status"]) as u32,
        "{name}: status"
    );
    assert_eq!(
        actor.mission.ai_counter(),
        signed(&expected["mission_visit"]) as u32,
        "{name}: visits"
    );
    assert_eq!(
        (
            actor.mission.dispatch_timer().start_frame(),
            actor.mission.dispatch_timer().delay()
        ),
        (
            signed(&expected["dispatch_timer_words"][0]),
            signed(&expected["dispatch_timer_words"][2])
        ),
        "{name}: dispatch clock"
    );
    assert_eq!(
        actor.attack_target.target(),
        fixture.target(&expected["target"]),
        "{name}: target"
    );
    assert_eq!(
        actor.archive_target(),
        fixture.target(&expected["archive"]),
        "{name}: archive"
    );
    assert_eq!(
        actor.navigation.nav_com(),
        fixture.nav(&expected["nav"]),
        "{name}: NavCom"
    );
    assert_eq!(
        actor.body_facing_current(fixture.sim.session.binary_frame),
        signed(&expected["body_facing_words"][0]) as u16,
        "{name}: primary facing"
    );
    assert_eq!(
        actor.body_facing.destination(),
        signed(&expected["body_facing_words"][0]) as u16,
        "{name}: desired primary facing"
    );
    assert_eq!(
        (
            actor.rearm_timer.start_frame(),
            actor.rearm_timer.duration()
        ),
        (
            signed(&expected["rearm_timer_words"][0]),
            signed(&expected["rearm_timer_words"][2])
        ),
        "{name}: rearm"
    );
    assert_eq!(
        (
            actor.passive_scan.timer().start_frame() as i32,
            actor.passive_scan.timer().duration() as i32
        ),
        (
            signed(&expected["targeting_timer_words"][0]),
            signed(&expected["targeting_timer_words"][2])
        ),
        "{name}: passive targeting"
    );
    assert_eq!(
        actor.health.current(),
        signed(&expected["health"]),
        "{name}: HP"
    );
    assert_eq!(
        actor.lifecycle.in_limbo(),
        expected["limbo"] != 0,
        "{name}: limbo"
    );
    assert_eq!(
        actor.lifecycle.object_alive(),
        expected["alive"] != 0,
        "{name}: alive"
    );
    assert_eq!(
        crate::sim::movement::motion_query::is_moving(actor),
        Some(expected["walk_moving"] != 0),
        "{name}: actual Walk IsMoving"
    );
}

fn command_attack(fixture: &mut SuppliedFootFixture, case: &Value, rules: &RuleSet) {
    let target = fixture.target(&case["input"]["target"]);
    fixture
        .sim
        .assign_target_represented(
            fixture.actor,
            target,
            Some(rules),
            None,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    let queue = &case["input"]["attack_queue"];
    fixture
        .sim
        .mission_queue_exact(
            fixture.actor,
            MissionId::from_raw(signed(&queue[0])),
            signed(&queue[1]),
            fixture.sim.session.binary_frame,
            &EntityReadyInputProvider,
        )
        .unwrap();
    assert_source(
        fixture,
        &case["command"]["after"],
        case["input"]["name"].as_str().unwrap(),
    );
}

fn recorded_fire_error(fixture: &SuppliedFootFixture, case: &Value, rules: &RuleSet) -> i32 {
    let actor = fixture.sim.substrate.entities.get(fixture.actor).unwrap();
    FireSubject {
        world: &fixture.sim,
        rules,
        overlay_registry: None,
        fog: None,
        firer: actor,
        obj: fixture
            .sim
            .interner
            .object_type(actor.type_ref(), rules)
            .unwrap(),
        target: fixture.target(&case["input"]["target"]),
        weapon_index: 0,
    }
    .fire_error(true) as i32
}

fn source_ai_visit(sim: &mut Simulation, actor: u64, rules: &RuleSet) {
    sim.advance_live_object_turn(actor, Some(rules), ObjectAiCtx::default())
        .unwrap();
}

fn emission_rules() -> Option<RuleSet> {
    let mut rules = retail_rules()?;
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    let input = &oracle()["ground_emission_receipt"]["inputs"];
    for asset in input["physical_assets"].as_array().unwrap() {
        let name = asset["name"].as_str().unwrap();
        let loaded = assets
            .load_file_from_mix(name)
            .unwrap_or_else(|| panic!("native emission asset {name} is unavailable"));
        assert_eq!(
            loaded.bytes.len() as u64,
            asset["bytes"].as_u64().unwrap(),
            "{name}"
        );
        assert_eq!(
            crate::util::sha256::sha256_hex(&loaded.bytes),
            asset["sha256"].as_str().unwrap(),
            "{name}: original physical SHP bytes"
        );
        let header = loaded.bytes[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(
            header,
            asset["header8_hex"].as_str().unwrap(),
            "{name}: header"
        );
    }
    let roots = input["anim_types"]
        .as_array()
        .unwrap()
        .iter()
        .map(|anim| anim["name"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE"),
        0
    );
    for anim in input["anim_types"].as_array().unwrap() {
        let name = anim["name"].as_str().unwrap();
        let config = rules.art().anim_runtime_config(name).unwrap();
        for (actual, key) in [
            (config.start, "start"),
            (config.loop_start, "loop_start"),
            (config.loop_end, "loop_end"),
            (config.end, "end"),
            (config.loop_count, "loop_count"),
            (i32::from(config.rate_logic_frames), "rate"),
        ] {
            assert_eq!(actual, signed(&anim[key]), "{name}: native AnimType {key}");
        }
        assert_eq!(
            config.raw_shp_frame_count,
            Some(signed(&anim["raw_shp_frame_count"])),
            "{name}: native SHP count"
        );
        assert_eq!(
            config.art_body_read,
            anim["art_body_read"].as_bool().unwrap(),
            "{name}: ART read"
        );
        assert_eq!(
            config.normalized,
            anim["normalized"].as_bool().unwrap(),
            "{name}: Normalized"
        );
    }
    Some(rules)
}

fn set_recorded_position(actor: &mut GameEntity, position: &Value) {
    let position = xyz(position);
    actor.position.set_rx_for_test((position[0] / 256) as u16);
    actor.position.set_ry_for_test((position[1] / 256) as u16);
    actor
        .position
        .set_sub_x_for_test(SimFixed::from_num(position[0] % 256));
    actor
        .position
        .set_sub_y_for_test(SimFixed::from_num(position[1] % 256));
    actor
        .position
        .set_exact_z_leptons_for_test(Some(position[2]));
}

fn emission_fixture(case: &Value, rules: &RuleSet) -> SuppliedFootFixture {
    let native = oracle();
    let firing = &native["ground_firing_receipt"];
    // The fresh VM repeats the same four physical constructors/Unlimbos. The
    // full initial target clock receipt supplies the constructor fields omitted
    // from emission's compact target read view; emission's observed fields win.
    let mut input = firing["cases"][0].clone();
    input["input"]["name"] = case["input"]["name"].clone();
    input["actor_before"] = case["initial"]["source"].clone();
    for (key, value) in case["initial"]["target"].as_object().unwrap() {
        input["target_before"][key] = value.clone();
    }
    input["rng_initial"] = case["rng_before"].clone();
    input["poses"] = json!([]);
    let mut fixture = firing_fixture(&input, rules);
    let fresh = case["original_actor_prefix"]["actors"].as_array().unwrap();
    assert_eq!(fresh.len(), 4);
    assert_eq!(fresh[1], case["initial"]["source"]["actor"]);
    assert_eq!(fresh[2], case["initial"]["target"]["pointer"]);
    // The firing-specific setup records only E1 and its target. All four
    // constructor roles belong to the shared original setup used by new().
    fixture.remap_native_objects(&[
        (&fresh[0], &native["setup"]["source"]),
        (&fresh[1], &native["setup"]["e1"]),
        (&fresh[2], &native["setup"]["candidate"]),
        (&fresh[3], &native["setup"]["victim"]),
    ]);
    assert!(
        fixture
            .sim
            .session
            .game_options
            .apply_in_game_speed(case["input"]["game_speed_index"].as_u64().unwrap() as u8)
    );
    for pose in case["poses"].as_array().unwrap() {
        let id = fixture.id(&pose["actor"]).unwrap();
        let actor = fixture.sim.substrate.entities.get_mut(id).unwrap();
        set_recorded_position(actor, &pose["position"]);
        actor.set_on_bridge(
            case["input"][if id == fixture.actor {
                "source_on_bridge"
            } else {
                "target_on_bridge"
            }] != 0,
        );
        let cell = &pose["cell_after"];
        let (x, y) = (
            signed(&cell["cell"][0]) as u16,
            signed(&cell["cell"][1]) as u16,
        );
        let terrain = fixture.sim.resolved_terrain.as_mut().unwrap();
        let identity = terrain.native_cell_identity((x as i16, y as i16));
        terrain.write_native_cell_flags(identity, cell["flags"].as_u64().unwrap() as u32);
        // The original controls alter XYZ/OnBridge AFTER ground registration.
        // They explicitly retain the ground head and an EMPTY upper list.
        let occupants = fixture.sim.substrate.occupancy.get(x, y).unwrap();
        assert_eq!(
            occupants.first_on_layer(MovementLayer::Ground),
            fixture.id(&pose["ground_head"])
        );
        assert_eq!(
            occupants.first_on_layer(MovementLayer::Bridge),
            fixture.id(&pose["upper_head"])
        );
    }
    fixture
}

/// Constructor identities are transported by original registration and ordered
/// ctor events. Native absolute Abstract IDs and allocator pointer values are
/// process history, so the Rust fixture uses the corresponding ordinal IDs.
fn emission_object_ids(fixture: &SuppliedFootFixture, case: &Value) -> BTreeMap<String, u64> {
    let mut ids = case["original_actor_prefix"]["actors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pointer| {
            (
                pointer.as_str().unwrap().to_owned(),
                fixture.id(pointer).unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut next = ids.len() as u64 + 1;
    for event in case["events"].as_array().unwrap() {
        if event["kind"] == "bullet_ctor" || event["kind"] == "anim_ctor" {
            assert!(
                ids.insert(event["this"].as_str().unwrap().to_owned(), next)
                    .is_none()
            );
            next += 1;
        }
    }
    ids
}

fn recorded_ids(ids: &BTreeMap<String, u64>, list: &Value) -> Vec<u64> {
    let pointers = list["actors"].as_array().unwrap();
    assert_eq!(pointers.len() as u64, list["count"].as_u64().unwrap());
    pointers
        .iter()
        .map(|pointer| ids[pointer.as_str().unwrap()])
        .collect()
}

fn assert_memberships(sim: &Simulation, expected: &Value, ids: &BTreeMap<String, u64>, name: &str) {
    assert_eq!(
        sim.logic_order(),
        recorded_ids(ids, &expected["logic"]),
        "{name}: ordered live Logic"
    );
    assert_eq!(
        sim.substrate.pending_delete.object_ids(),
        recorded_ids(ids, &expected["deferred"]),
        "{name}: ordered deferred finalization"
    );
    assert_eq!(
        sim.substrate
            .anims
            .iter()
            .map(|(&id, _)| id)
            .collect::<Vec<_>>(),
        recorded_ids(ids, &expected["anims"]),
        "{name}: physical Anim vector"
    );
    assert_eq!(
        sim.projectiles
            .iter()
            .map(|(&id, _)| id)
            .collect::<Vec<_>>(),
        recorded_ids(ids, &expected["bullets"]),
        "{name}: physical Bullet vector"
    );
    for (index, layer) in expected["display"].as_array().unwrap().iter().enumerate() {
        assert_eq!(
            sim.display_layers()
                .members(DisplayLayer::from_index(index as u8).unwrap()),
            recorded_ids(ids, layer),
            "{name}: ordered Display layer{index}"
        );
    }
}

fn assert_target(fixture: &SuppliedFootFixture, expected: &Value, name: &str) {
    let id = fixture.id(&expected["pointer"]).unwrap();
    let target = fixture.sim.substrate.entities.get(id).unwrap();
    let position = position_world_coord(&target.position);
    assert_eq!(
        [position.x, position.y, position.z],
        xyz(&expected["position"]),
        "{name}: target XYZ"
    );
    assert_eq!(
        target.health.current(),
        signed(&expected["health"]),
        "{name}: MTNK damage"
    );
    assert_eq!(
        target.mission.current().raw(),
        signed(&expected["mission"]),
        "{name}: MTNK mission"
    );
    assert_eq!(
        target.mission.queued().raw(),
        signed(&expected["queued"]),
        "{name}: MTNK queue"
    );
    assert_eq!(
        target.attack_target.target(),
        fixture.target(&expected["target"]),
        "{name}: synchronous MTNK retaliation target"
    );
    assert_eq!(
        target.on_bridge(),
        expected["on_bridge"] != 0,
        "{name}: target layer"
    );
    assert_eq!(
        target.lifecycle.object_alive(),
        expected["alive"] != 0,
        "{name}: target alive"
    );
    assert_eq!(
        target.lifecycle.in_limbo(),
        expected["limbo"] != 0,
        "{name}: target limbo"
    );
}

fn assert_emission_boundary(
    fixture: &SuppliedFootFixture,
    expected: &Value,
    ids: &BTreeMap<String, u64>,
    name: &str,
) {
    assert_source(fixture, &expected["source"], name);
    assert_target(fixture, &expected["target"], name);
    assert_memberships(&fixture.sim, &expected["memberships"], ids, name);
}

fn assert_fired_bullet(
    fixture: &SuppliedFootFixture,
    event: &Value,
    ids: &BTreeMap<String, u64>,
    name: &str,
) {
    let expected = &event["bullet_after"];
    let bullet = fixture
        .sim
        .projectiles
        .get(ids[expected["pointer"].as_str().unwrap()])
        .unwrap();
    assert_eq!(
        [bullet.position.x, bullet.position.y, bullet.position.z],
        xyz(&expected["position"]),
        "{name}: native Bullet launch XYZ"
    );
    assert_eq!(
        bullet.source_id,
        fixture.id(&expected["owner"]).unwrap(),
        "{name}: Bullet owner"
    );
    assert_eq!(
        bullet.target,
        ProjectileTarget::Entity(fixture.id(&expected["target"]).unwrap()),
        "{name}: Bullet target"
    );
    assert_eq!(
        bullet.payload.base_damage,
        signed(&expected["damage"]),
        "{name}: M60 raw damage"
    );
    assert_eq!(
        fixture
            .sim
            .interner
            .resolve(bullet.payload.installed_weapon().unwrap()),
        expected["weapon"]["name"].as_str().unwrap(),
        "{name}: weapon identity"
    );
    assert_eq!(
        fixture.sim.interner.resolve(bullet.payload.warhead),
        expected["warhead"]["name"].as_str().unwrap(),
        "{name}: warhead identity"
    );
    assert_eq!(
        bullet.in_logic_vector,
        expected["logic_registered"] != 0,
        "{name}: Bullet Logic flag"
    );
    for (actual, expected) in bullet
        .velocity
        .native()
        .iter()
        .zip(expected["velocity_f64_bits"].as_array().unwrap())
    {
        let bits = u64::from_str_radix(expected.as_str().unwrap(), 16)
            .unwrap()
            .swap_bytes();
        assert_eq!(actual.bits(), bits, "{name}: original Bullet velocity bits");
    }
}

fn assert_recorded_anim_states(
    fixture: &SuppliedFootFixture,
    case: &Value,
    ids: &BTreeMap<String, u64>,
    memberships: &Value,
    event_end: usize,
    name: &str,
) {
    let events = case["events"].as_array().unwrap();
    for pointer in memberships["anims"]["actors"].as_array().unwrap() {
        let constructor = events
            .iter()
            .find(|event| event["kind"] == "anim_ctor" && event["this"] == *pointer)
            .unwrap();
        let latest = events[..event_end]
            .iter()
            .rev()
            .find(|event| event["kind"] == "anim_ai" && event["this"] == *pointer);
        let context = format!(
            "{name}: {} {} id {} original AI frame {}",
            constructor["name"].as_str().unwrap(),
            pointer.as_str().unwrap(),
            ids[pointer.as_str().unwrap()],
            latest.map_or_else(
                || "not visited".to_owned(),
                |event| event["frame"].to_string()
            )
        );
        // MGUN is attached after construction, then its first live AI is
        // skipped by Bullet's ordered compact removal. That first original
        // AI's BEFORE readback directly records the retained attached state;
        // no coordinate/timer result is calculated from Rust or the ctor.
        let expected = if let Some(event) = latest {
            &event["anim_after"]
        } else {
            &events
                .iter()
                .find(|event| event["kind"] == "anim_ai" && event["this"] == *pointer)
                .unwrap()["anim_before"]
        };
        let anim = fixture
            .sim
            .substrate
            .anims
            .get(ids[pointer.as_str().unwrap()])
            .unwrap();
        assert_eq!(
            fixture.sim.interner.resolve(anim.type_id),
            constructor["name"].as_str().unwrap(),
            "{name}: Anim type"
        );
        assert_eq!(
            [anim.world_coord.x, anim.world_coord.y, anim.world_coord.z],
            xyz(&expected["location"]),
            "{name}: retained Anim XYZ"
        );
        assert_eq!(
            anim.draw_flags,
            expected["draw_flags"].as_u64().unwrap() as u32,
            "{name}: Anim draw flags"
        );
        assert_eq!(
            anim.z_adjust,
            signed(&expected["z_adjust"]),
            "{name}: Anim z adjust"
        );
        let owner = events[..event_end]
            .iter()
            .rev()
            .find(|event| event["kind"] == "anim_attach" && event["this"] == *pointer)
            .and_then(|event| {
                let raw = event["args"][0].as_u64().unwrap();
                (raw != 0).then(|| fixture.id(&json!(format!("0x{raw:x}"))).unwrap())
            });
        assert_eq!(anim.owner_entity, owner, "{name}: Anim attachment");
        let expected = &expected["runtime"];
        let runtime = &anim.runtime;
        for (actual, key) in [
            (runtime.current_frame, "current_frame"),
            (runtime.frame_step, "frame_step"),
            (i32::from(runtime.delay_remaining), "delay_remaining"),
            (i32::from(runtime.rate_reload), "rate_reload"),
            (i32::from(runtime.loop_remaining), "loop_remaining"),
        ] {
            assert_eq!(actual, signed(&expected[key]), "{name}: Anim {key}");
        }
        for (actual, key) in [
            (runtime.first_ai_guard, "first_ai_guard"),
            (runtime.constructor_reverse, "constructor_reverse"),
            (runtime.inactive, "inactive"),
            (runtime.paused, "paused"),
        ] {
            assert_eq!(actual, expected[key] != 0, "{context}: Anim {key}");
        }
        assert_eq!(
            [
                runtime.frame_timer.start_frame(),
                runtime.frame_timer.duration()
            ],
            [
                signed(&expected["frame_timer"][0]),
                signed(&expected["frame_timer"][1])
            ],
            "{name}: original Anim clock"
        );
    }
}

#[test]
fn original_ground_and_supplied_layer_fire_error_uses_the_live_class_owner() {
    let Some(rules) = retail_rules() else { return };
    let mut compared = 0;
    for case in oracle()["ground_firing_receipt"]["cases"]
        .as_array()
        .unwrap()
    {
        let name = case["input"]["name"].as_str().unwrap();
        let mut fixture = firing_fixture(case, &rules);
        command_attack(&mut fixture, case, &rules);
        assert_eq!(
            recorded_fire_error(&fixture, case, &rules),
            signed(&case["command"]["diagnostic_fire_error"]),
            "{name}: original51C8B0/6FC0B0"
        );
        assert_rng(&fixture.sim, &case["rng_initial"], name);
        compared += 1;
    }
    // Two opposite-layer controls execute only GetFireError. No approach,
    // pathfinding or whole-AI refusal result is supplied by the native corpus.
    assert_eq!(compared, 8);
}

#[test]
fn original_completed_prelaunch_ai_visits_match_doing_clock_latch_and_full_rng() {
    let Some(rules) = retail_rules() else { return };
    let mut compared = 0;
    for case in oracle()["ground_firing_receipt"]["cases"]
        .as_array()
        .unwrap()
    {
        if !case["input"]["expiry_after_completed_ai_calls"].is_null() {
            continue;
        }
        let name = case["input"]["name"].as_str().unwrap();
        let mut fixture = firing_fixture(case, &rules);
        command_attack(&mut fixture, case, &rules);
        for frame in case["frames"].as_array().unwrap() {
            if frame["returned"] != true {
                // Original final visit stops BEFORE6FDD50, after51DF70. The
                // production host continues through launch, so only complete
                // returned visits can compare this entire state/RNG boundary.
                assert_eq!(frame["stop"], "0x6fdd50");
                assert_eq!(frame["base_fire_entry"]["unexecuted"], true);
                break;
            }
            fixture.sim.session.binary_frame = frame["native_frame"].as_u64().unwrap() as u32;
            assert_source(&fixture, &frame["before"], name);
            assert_rng(&fixture.sim, &frame["rng_before"], name);
            source_ai_visit(&mut fixture.sim, fixture.actor, &rules);
            assert_source(&fixture, &frame["after"], name);
            assert_rng(&fixture.sim, &frame["rng_after"], name);
            compared += 1;
        }
    }
    assert_eq!(compared, 9);
}

#[test]
fn original_single_listener_expiry_clears_pending_shot_and_shortens_passive_timer() {
    let Some(rules) = retail_rules() else { return };
    let mut compared = 0;
    for case in oracle()["ground_firing_receipt"]["cases"]
        .as_array()
        .unwrap()
    {
        if case["expiry"].is_null() {
            continue;
        }
        let name = case["input"]["name"].as_str().unwrap();
        let mut fixture = firing_fixture(case, &rules);
        command_attack(&mut fixture, case, &rules);
        let expiry_after = case["input"]["expiry_after_completed_ai_calls"]
            .as_u64()
            .unwrap() as usize;
        for (calls, frame) in case["frames"].as_array().unwrap().iter().enumerate() {
            if calls == expiry_after {
                let expiry = &case["expiry"];
                fixture.sim.session.binary_frame =
                    expiry["callback_events"][0]["frame"].as_u64().unwrap() as u32;
                assert_source(&fixture, &expiry["before"], name);
                assert_rng(&fixture.sim, &expiry["rng_before"], name);
                let target_id = fixture.id(&case["input"]["target"]).unwrap();
                let target = fixture.sim.substrate.entities.get(target_id).unwrap();
                let owner = target.owner();
                let health = target.health.current();
                let alive = target.lifecycle.object_alive();
                let xyz = xyz(&case["target_before"]["position"]);
                // Original direct51AA10(control1), with the expired pointer
                // STILL alive and registered. Do not substitute UnInit or a
                // control0 Detach_All broadcast for this receiver premise.
                assert_eq!(case["input"]["expiry_control"], 1);
                fixture.sim.notify_entity_pointer_expired(
                    fixture.actor,
                    target_id,
                    Some(((xyz[0] / 256) as u16, (xyz[1] / 256) as u16)),
                    alive,
                    health,
                    false,
                    Some(owner),
                    PointerExpiryControl::Uninit,
                    Some(&rules),
                    None,
                    crate::sim::world::FrameEffects::default(),
                );
                assert_source(&fixture, &expiry["after"], name);
                assert_rng(&fixture.sim, &expiry["rng_after"], name);
                assert!(
                    fixture
                        .sim
                        .substrate
                        .entities
                        .get(target_id)
                        .unwrap()
                        .lifecycle
                        .object_alive()
                );
                assert!(fixture.sim.logic_order().contains(&target_id));
                compared += 1;
            }
            fixture.sim.session.binary_frame = frame["native_frame"].as_u64().unwrap() as u32;
            assert!(frame["returned"].as_bool().unwrap());
            assert_source(&fixture, &frame["before"], name);
            assert_rng(&fixture.sim, &frame["rng_before"], name);
            source_ai_visit(&mut fixture.sim, fixture.actor, &rules);
            assert_source(&fixture, &frame["after"], name);
            assert_rng(&fixture.sim, &frame["rng_after"], name);
        }
    }
    assert_eq!(compared, 2);
}

#[test]
fn original_whole_ai_shot_dynamic_logic_damage_and_deferred_lifetimes_match_native() {
    let Some(rules) = emission_rules() else {
        return;
    };
    let mut visits = 0;
    let mut suffixes = 0;
    let mut shots = 0;
    for case in oracle()["ground_emission_receipt"]["cases"]
        .as_array()
        .unwrap()
    {
        let name = case["input"]["name"].as_str().unwrap();
        let mut fixture = emission_fixture(case, &rules);
        let ids = emission_object_ids(&fixture, case);
        assert_memberships(&fixture.sim, &case["initial"]["memberships"], &ids, name);
        assert_rng(&fixture.sim, &case["rng_before"], name);
        let events = case["events"].as_array().unwrap();
        let queue = events
            .iter()
            .find(|event| event["kind"] == "queue_mission")
            .unwrap();
        let command = json!({
            "input": {
                "name": name,
                "target": case["initial"]["target"]["pointer"],
                "attack_queue": queue["args"],
            },
            "command": { "after": case["command"]["after"]["source"] },
        });
        command_attack(&mut fixture, &command, &rules);
        assert_emission_boundary(&fixture, &case["command"]["after"], &ids, name);
        assert_rng(&fixture.sim, &case["command"]["rng_after"], name);
        for visit in case["ai_visits"].as_array().unwrap() {
            fixture.sim.session.binary_frame = visit["frame"].as_u64().unwrap() as u32;
            assert_emission_boundary(&fixture, &visit["before"], &ids, name);
            assert_rng(&fixture.sim, &visit["rng_before"], name);
            source_ai_visit(&mut fixture.sim, fixture.actor, &rules);
            assert_emission_boundary(&fixture, &visit["after"], &ids, name);
            assert_rng(&fixture.sim, &visit["rng_after"], name);
            let start = visit["event_range"][0].as_u64().unwrap() as usize;
            let end = visit["event_range"][1].as_u64().unwrap() as usize;
            assert_recorded_anim_states(
                &fixture,
                case,
                &ids,
                &visit["after"]["memberships"],
                end,
                name,
            );
            for event in &events[start..end] {
                if event["kind"] == "bullet_fire" {
                    assert_fired_bullet(&fixture, event, &ids, name);
                    shots += 1;
                }
            }
            visits += 1;
            let Some(suffix) = case["logic_suffix"]
                .as_array()
                .unwrap()
                .iter()
                .find(|suffix| suffix["frame"] == visit["frame"])
            else {
                continue;
            };
            assert_emission_boundary(&fixture, &suffix["before"], &ids, name);
            assert_rng(&fixture.sim, &suffix["rng_before"], name);
            // Existing production dynamic Logic owner: index increments after
            // stable compact removal, and newly appended effects remain live.
            assert_eq!(
                suffix["supplied_start_index"],
                case["original_actor_prefix"]["count"]
            );
            assert!(!fixture.sim.visit_combat_tail(
                case["original_actor_prefix"]["count"].as_u64().unwrap() + 1,
                &rules,
                None,
                crate::sim::world::FrameEffects::default()
            ));
            assert_emission_boundary(&fixture, &suffix["after_pass"], &ids, name);
            let start = suffix["event_range"][0].as_u64().unwrap() as usize;
            let end = suffix["event_range"][1].as_u64().unwrap() as usize;
            let before_drain = (start..end)
                .find(|&index| events[index]["kind"] == "deferred_drain")
                .unwrap();
            assert_recorded_anim_states(
                &fixture,
                case,
                &ids,
                &suffix["after_pass"]["memberships"],
                before_drain,
                name,
            );
            // The original caller increments A8ED84 before725C70. Object AI
            // and both clocks saw the previous absolute frame throughout.
            fixture.sim.session.binary_frame += 1;
            fixture.sim.process_pending_delete_with(
                Some(&rules),
                None,
                crate::sim::world::FrameEffects::default(),
                1,
                false,
            );
            assert_emission_boundary(&fixture, &suffix["after_drain"], &ids, name);
            assert_rng(&fixture.sim, &suffix["rng_after"], name);
            suffixes += 1;
        }
        assert_emission_boundary(&fixture, &case["final"], &ids, name);
        assert_rng(&fixture.sim, &case["rng_after"], name);
        // The deck input deliberately has empty upper occupation. Its saved
        // HP300 is not a comparison of genuine bridge-deck area damage.
    }
    assert_eq!((visits, suffixes, shots), (65, 56, 4));
}
