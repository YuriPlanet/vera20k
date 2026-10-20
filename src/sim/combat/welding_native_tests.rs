//! Original constructor, retained-link and uninterrupted Welding AI controls.
//! The mobile object prefix/poses and admitted birth are declared component
//! priors; ordinary boarding, friendly input and healing have an app test.

use super::*;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::native_identity::NativeUniqueIdCursor;
use crate::sim::particles::ParticleSystem;
use crate::sim::rng::SimRng;
use glam::IVec3;
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn native() -> &'static Value {
    static CORPUS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    CORPUS.get_or_init(|| {
        serde_json::from_str(crate::test_fixture::text(
            "tools/procedural_drawing_oracle/welding.json",
        ))
        .unwrap()
    })
}

fn int(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}
fn xyz(value: &Value) -> IVec3 {
    IVec3::new(int(&value[0]), int(&value[1]), int(&value[2]))
}

type Roles = BTreeMap<String, u64>;

fn target(roles: &Roles, value: &Value) -> Option<TargetKind> {
    value.as_str().map(|role| {
        if role == "cell" {
            TargetKind::Cell(44, 48)
        } else {
            TargetKind::Entity(roles[role])
        }
    })
}

fn target_role(roles: &Roles, target: Option<TargetKind>) -> Value {
    match target {
        None => Value::Null,
        Some(TargetKind::Cell(44, 48)) => json!("cell"),
        Some(TargetKind::Entity(id)) => {
            json!(roles.iter().find(|(_, value)| **value == id).unwrap().0)
        }
        other => panic!("undeclared component target {other:?}"),
    }
}

fn scene(rules: &RuleSet, cursor: &Value) -> (Simulation, Roles) {
    let mut sim = Simulation::new();
    sim.session.set_binary_frame_for_test(36);
    sim.session.set_map_width_for_test(64);
    sim.session.set_map_height_for_test(64);
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_grid(
        64,
        64,
        crate::map::resolved_terrain::test_clear_cell,
    ));
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(64, 64));
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let mut roles = Roles::new();
    for row in native()["constructor_priors"].as_array().unwrap() {
        let position = xyz(&row["position"]);
        let id = sim.allocate_stable_id();
        let mut entity = GameEntity::test_default(
            id,
            row["type_id"].as_str().unwrap(),
            "Americans",
            (position.x / 256) as u16,
            (position.y / 256) as u16,
        );
        entity.set_owner_for_test(owner);
        entity.set_type_ref_for_test(sim.interner.intern(row["type_id"].as_str().unwrap()));
        entity.set_native_unique_id_for_test(int(&row["native_id"]));
        entity
            .position
            .set_sub_x_for_test(SimFixed::from_num(position.x % 256));
        entity
            .position
            .set_sub_y_for_test(SimFixed::from_num(position.y % 256));
        entity
            .position
            .set_exact_z_leptons_for_test(Some(position.z));
        entity.health.set_for_test(100);
        sim.substrate.entities.insert(entity);
        roles.insert(row["role"].as_str().unwrap().to_owned(), id);
    }
    // These actors are constructor/coordinate priors, not live Unit AI.
    // Only the real constructed System enters Logic in this comparison.
    sim.set_logic_order_for_test(Vec::new());
    sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(
        int(cursor) as u32
    ));
    (sim, roles)
}

fn system_state(system: &ParticleSystem, roles: &Roles) -> Value {
    json!({
        "native_id":system.native_unique_id(), "position":system.coords().to_array(),
        "offset":system.offset().to_array(), "target_coords":system.target_coords().to_array(),
        "owner":system.owner_entity().map(|id|roles.iter().find(|(_,value)|**value==id).unwrap().0),
        "retained_target":target_role(roles,system.target_ref()),
        "owner_house_present":system.owner_house().is_some(), "lifetime":system.lifetime(),
        "spark_spawn_frames":system.spark_spawn_frames(), "stage":system.facing(),
        "spawn_timer_f32_bits":system.spawn_timer().bits(), "done":system.done_spawning(),
        // Original ctor62DD0A writes zero; the retained Spark owner documents
        // the absence of an active setter and does not store a second flag.
        "direction_used":false, "particle_count":system.particles().len(),
        "particle_native_ids":system.particles().iter().map(|p|p.native_unique_id()).collect::<Vec<_>>(),
        "alive":system.alive(), "logic":system.in_logic_vector(),
    })
}

fn rng(sim: &Simulation) -> [String; 3] {
    [
        sim.main_rng.native_state_hex(),
        sim.scenario_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
    ]
}

fn restore_rng(sim: &mut Simulation, state: &Value) {
    sim.main_rng = SimRng::from_native_state_hex_for_test(state["main"].as_str().unwrap()).into();
    sim.scenario_rng = SimRng::from_native_state_hex_for_test(state["scenario"].as_str().unwrap());
    sim.mapgen_rng = SimRng::from_native_state_hex_for_test(state["mapgen"].as_str().unwrap());
}

fn word_hex(bits: u64) -> String {
    bits.to_le_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn physical_repair_and_particle_readers_match_original_layers_and_constructor_defaults() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let system = rules.particle_system_type(rules.ps_type_id_by_name("WeldingSys").unwrap());
    let particle = rules.particle_type(system.holds_what.unwrap());
    assert_eq!(particle.name, "WeldingSpark");
    let weapon = rules.weapon("RepairBullet").unwrap();
    for profile in native()["type_inputs"]["profiles"].as_array().unwrap() {
        let expected = &profile["effective_types"];
        // All three physical histories were executed by the original readers.
        // The selected map sections author no overrides of these fields;
        // Hills' production reader is also the ordinary input test's owner.
        for (field, actual) in [
            ("particle_cap", json!(system.particle_cap)),
            ("behavior", json!(system.behaves_like as i32)),
            ("lifetime", json!(system.lifetime)),
            ("spark_spawn_frames", json!(system.spark_spawn_frames)),
            ("light_size", json!(system.light_size)),
            ("one_frame_light", json!(system.one_frame_light)),
            (
                "spawn_spark_percentage_bits",
                json!(word_hex(system.spawn_spark_percentage.bits())),
            ),
            (
                "spawn_direction_bits",
                json!(
                    system
                        .spawn_direction
                        .to_array()
                        .map(|value| (value as f32).to_bits())
                ),
            ),
        ] {
            assert_eq!(
                actual, expected["system"][field],
                "{} {field}",
                profile["name"]
            );
        }
        for (field, actual) in [
            ("behavior", json!(particle.behaves_like as i32)),
            ("max_ec", json!(particle.max_ec)),
            ("x_velocity", json!(particle.x_velocity)),
            ("y_velocity", json!(particle.y_velocity)),
            ("min_z_velocity", json!(particle.min_z_velocity)),
            ("z_velocity_range", json!(particle.z_velocity_range)),
            (
                "color_speed_bits",
                json!(word_hex(particle.color_speed.bits())),
            ),
            ("color_list", json!(particle.color_list)),
            ("start_color1", json!(particle.start_color_1)),
            ("start_color2", json!(particle.start_color_2)),
        ] {
            assert_eq!(
                actual, expected["particle"][field],
                "{} {field}",
                profile["name"]
            );
        }
        let expected = &profile["weapon"]["effective"];
        for (field, actual) in [
            ("use_spark_particles", json!(weapon.use_spark_particles)),
            (
                "attached_particle_system",
                json!(weapon.attached_particle_system),
            ),
            ("rof", json!(weapon.rof)),
            ("burst", json!(weapon.burst)),
            ("damage", json!(weapon.damage)),
            ("range_leptons", json!(weapon.range_leptons)),
            ("projectile", json!(weapon.projectile)),
            ("warhead", json!(weapon.warhead)),
        ] {
            assert_eq!(actual, expected[field], "{} {field}", profile["name"]);
        }
        for name in ["FV", "ENGINEER"] {
            let object = rules.object(name).unwrap();
            let expected = &profile["ifv"][name]["result"];
            for (field, actual) in [
                ("ifv_mode", json!(object.ifv_mode)),
                ("turret_count", json!(object.turret_count)),
                ("weapon_count", json!(object.weapon_count)),
                (
                    "is_charge_turret",
                    json!(object.gunner_turrets.is_charge_turret()),
                ),
                (
                    "turrets",
                    json!(
                        (0..crate::rules::object_type::WEAPON_SLOT_COUNT as i32)
                            .map(|slot| object.gunner_turrets.turret_for_weapon(slot))
                            .collect::<Vec<_>>()
                    ),
                ),
            ] {
                assert_eq!(
                    actual, expected[field],
                    "{} {name} {field}",
                    profile["name"]
                );
            }
        }
        let ini = crate::rules::ini_parser::IniFile::from_str("[P]\nFixtureOnly=1\n");
        let defaults = crate::rules::particle_type::ParticleType::from_ini_section(
            "P",
            ini.section("P").unwrap(),
        );
        for (field, actual) in [
            ("x_velocity", defaults.x_velocity),
            ("y_velocity", defaults.y_velocity),
            ("z_velocity_range", defaults.z_velocity_range),
            ("max_ec", defaults.max_ec),
        ] {
            assert_eq!(
                actual,
                int(&profile["constructor"]["particle"][field]),
                "original absent {field} default"
            );
        }
    }
}

#[test]
fn original_particle_constructor_separates_supplied_coordinate_from_owner_target() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let rows = native()["constructor_cases"].as_array().unwrap();
    assert_eq!(rows.len(), 12);
    for row in rows {
        let (mut sim, roles) = scene(rules, &row["native_id_before"]);
        let input = &row["input"];
        sim.substrate
            .entities
            .get_mut(roles["owner"])
            .unwrap()
            .attack_target
            .set(target(&roles, &input["tarcom"]));
        if input["cell_bridge"] == true {
            sim.resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut_for_test(44, 48)
                .unwrap()
                .bridge_facts
                .raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
        }
        let before = rng(&sim);
        let id = sim
            .spawn_particle_system(
                rules
                    .ps_type_id_by_name(input["system_type"].as_str().unwrap())
                    .unwrap(),
                xyz(&input["position"]),
                target(&roles, &input["supplied_target"]),
                input["owner"].as_str().map(|role| roles[role]),
                xyz(&input["fifth"]),
                None,
                rules,
            )
            .unwrap();
        assert_eq!(
            system_state(sim.particle_systems().get(id).unwrap(), &roles),
            row["state"],
            "{}",
            row["name"]
        );
        assert_eq!(
            sim.native_unique_ids.as_ref().unwrap().current_raw(),
            int(&row["native_id_after"]) as u32
        );
        assert_eq!(rng(&sim), before, "constructor consumes no RNG");
        assert_eq!(row["rng_unchanged"], true);
        assert!(sim.display_layers().ordered_ids().any(|&value| value == id));
    }
}

fn birth(rules: &RuleSet, row: &Value) -> (Simulation, Roles, u64) {
    let (mut sim, roles) = scene(rules, &row["native_id_before"]);
    let owner = roles["owner"];
    sim.substrate
        .entities
        .get_mut(owner)
        .unwrap()
        .attack_target
        .set(Some(TargetKind::Entity(roles["target"])));
    let id = sim
        .spawn_particle_system(
            rules.ps_type_id_by_name("WeldingSys").unwrap(),
            xyz(&row["supplied_muzzle"]),
            Some(TargetKind::Entity(roles["target"])),
            Some(owner),
            IVec3::ZERO,
            None,
            rules,
        )
        .unwrap();
    sim.substrate
        .entities
        .get_mut(owner)
        .unwrap()
        .spark_particle_system
        .bind(Some(id));
    (sim, roles, id)
}

#[test]
fn original_fireat_live_spark_guard_refuses_a_direct_shot_without_launch_side_effects() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let control = native()["admission_cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["input"]["use_spark_particles"] == true
                && row["input"]["live"] == true
                && row["input"]["berserk"] == false
        })
        .unwrap();
    let guard = control["controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "fireat_guard")
        .unwrap();
    assert_eq!(guard["continued"], false);
    assert_eq!(guard["returned_eax"], 0);
    let (mut sim, roles, id) = birth(rules, &native()["live_case"]["birth"]);
    // The native control enters after FireAt's earlier target-Limbo gate.
    // Supply an admitted live target to reach that same guard in production.
    sim.substrate
        .entities
        .get_mut(roles["target"])
        .unwrap()
        .lifecycle
        .set_in_limbo_for_test(false);
    let actor = sim.entities().get(roles["owner"]).unwrap();
    let object = rules.object("FV").unwrap();
    let target = TargetKind::Entity(roles["target"]);
    let shot = AdmittedFire {
        snap: build_attacker_snapshot(actor, target, None),
        obj: object,
        selected: combat_weapon::resolve_weapon_index(
            rules,
            actor,
            object,
            1,
            sim.entities(),
            &sim.interner,
        )
        .unwrap(),
        target_coords: resolve_target_coords(&target, &sim).unwrap(),
        target_type_ref: sim.entities().get(roles["target"]).unwrap().type_ref(),
        is_garrison: false,
    };
    assert!(shot.selected.weapon.use_spark_particles);
    let before = sim.state_hash();
    let before_rng = rng(&sim);
    let mut out = CombatEmit::default();
    assert_eq!(
        emit_admitted_fire(
            &mut sim,
            rules,
            shot,
            36,
            &mut out,
            None,
            crate::sim::world::FrameEffects::default(),
        ),
        None
    );
    assert_eq!(sim.state_hash(), before);
    assert_eq!(rng(&sim), before_rng);
    assert!(out.fire_events.is_empty() && out.damage_events.is_empty());
    assert_eq!(sim.particle_systems().iter().count(), 1);
    assert_eq!(
        sim.entities()
            .get(roles["owner"])
            .unwrap()
            .spark_particle_system
            .system(),
        Some(id)
    );
}

#[test]
fn original_welding_pointer_expiry_and_target_setters_preserve_their_distinct_authorities() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let ordinary = native()["birth_cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "ordinary")
        .unwrap();
    let mut systems = 0;
    let mut setters = 0;
    for row in native()["lifecycle_cases"].as_array().unwrap() {
        let input = &row["input"];
        let (mut sim, roles, id) = birth(rules, ordinary);
        let before = rng(&sim);
        if input["receiver"] == "system" {
            systems += 1;
            assert_eq!(
                system_state(sim.particle_systems().get(id).unwrap(), &roles),
                row["before"]
            );
            // Original62FE90 does not read the removal-control argument.
            // Broadcast integration (both controls) also has lifecycle tests.
            assert!(
                sim.particle_systems_mut()
                    .pointer_expired(id, roles[input["expired"].as_str().unwrap()])
            );
            assert_eq!(
                system_state(sim.particle_systems().get(id).unwrap(), &roles),
                row["after"]
            );
            assert_eq!(
                sim.entities()
                    .get(roles["owner"])
                    .unwrap()
                    .spark_particle_system
                    .system(),
                Some(id),
                "the System callback does not own the source Techno's link"
            );
        } else if let Some(steps) = row["steps"].as_array() {
            setters += 1;
            for (requested, expected) in input["targets"].as_array().unwrap().iter().zip(steps) {
                sim.assign_target_represented(
                    roles["owner"],
                    target(&roles, requested),
                    Some(rules),
                    None,
                    crate::sim::world::FrameEffects::default(),
                )
                .unwrap();
                let actor = sim.entities().get(roles["owner"]).unwrap();
                assert_eq!(
                    target_role(&roles, actor.attack_target.target()),
                    expected["target"]
                );
                assert_eq!(
                    actor.spark_particle_system.system().map(|_| "system"),
                    expected["spark_link"].as_str()
                );
                assert_eq!(
                    system_state(sim.particle_systems().get(id).unwrap(), &roles),
                    ordinary["systems"][0],
                    "changing TarCom does not rewrite the constructed System's target"
                );
            }
        } else if input["entry"] == "0x4c75e6" {
            setters += 1;
            // Exercise the production Event Stop owner. The original control
            // bounds its claim to Stop's destination/target setter fragment.
            sim.substrate
                .entities
                .get_mut(roles["owner"])
                .unwrap()
                .lifecycle
                .set_in_limbo_for_test(false);
            assert!(sim.apply_command(
                "Americans",
                &crate::sim::command::Command::Stop {
                    entity_id: roles["owner"]
                },
                Some(rules)
            ));
            let actor = sim.entities().get(roles["owner"]).unwrap();
            assert_eq!(
                target_role(&roles, actor.attack_target.target()),
                row["after"]["target"]
            );
            assert_eq!(
                actor.spark_particle_system.system().map(|_| "system"),
                row["after"]["spark_link"].as_str()
            );
            assert_eq!(
                system_state(sim.particle_systems().get(id).unwrap(), &roles),
                ordinary["systems"][0]
            );
        }
        assert_eq!(rng(&sim), before, "callback/setter consumes no RNG");
        assert_eq!(row["rng_unchanged"], true);
    }
    assert_eq!((systems, setters), (4, 2));
}

fn native_id(sim: &Simulation, id: u64) -> i32 {
    if let Some(system) = sim.particle_systems().get(id) {
        return system.native_unique_id();
    }
    if let Some(entity) = sim.entities().get(id) {
        return entity.native_unique_id();
    }
    if let Some((particle, _)) = sim.particle_systems().detached_particle(id) {
        return particle.native_unique_id();
    }
    sim.particle_systems()
        .iter()
        .flat_map(|(_, system)| system.particles())
        .find(|particle| particle.stable_id() == id)
        .unwrap()
        .native_unique_id()
}

fn assert_logical_state(sim: &Simulation, roles: &Roles, id: u64, expected: &Value) {
    assert_eq!(
        json!(sim.particle_systems().get(id).is_some()),
        expected["present_in_system_registry"]
    );
    let logic = sim.live_object_order_snapshot();
    assert_eq!(json!(logic.contains(&id)), expected["present_in_logic"]);
    let pending = sim.substrate.pending_delete.object_ids();
    assert_eq!(json!(pending.contains(&id)), expected["pending_system"]);
    for (field, actual) in [
        (
            "system_registry_native_ids",
            sim.particle_systems()
                .iter()
                .map(|(_, system)| system.native_unique_id())
                .collect::<Vec<_>>(),
        ),
        (
            "logic_native_ids",
            logic.iter().map(|&id| native_id(sim, id)).collect(),
        ),
        (
            "pending_native_ids",
            pending.iter().map(|&id| native_id(sim, id)).collect(),
        ),
    ] {
        assert_eq!(json!(actual), expected[field], "{field}");
    }
    assert_eq!(
        sim.entities()
            .get(roles["owner"])
            .unwrap()
            .spark_particle_system
            .system()
            .map(|_| "system"),
        expected["link"]["spark_link"].as_str(),
        "link clears in the original phase"
    );
    assert_eq!(
        sim.native_unique_ids.as_ref().unwrap().current_raw(),
        int(&expected["native_id_cursor"]) as u32
    );
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        expected["scenario_rng_hex"].as_str().unwrap()
    );
}

#[test]
fn original_welding_live_logic_boundaries_match_rng_ids_children_and_same_pass_link_expiry() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let row = &native()["live_case"];
    let (mut sim, roles, id) = birth(&retail.rules, &row["birth"]);
    assert_eq!(
        system_state(sim.particle_systems().get(id).unwrap(), &roles),
        row["birth"]["systems"][0]
    );
    sim.scenario_rng =
        SimRng::from_native_state_hex_for_test(row["scenario_rng_initial_hex"].as_str().unwrap());
    let other_before = [
        sim.main_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
    ];
    let boundaries = row["boundaries"].as_array().unwrap();
    let last = int(&boundaries.last().unwrap()["tick"]);
    let retirement = &row["retirement_before_drain"];
    let mut compared_produced_child = false;
    for tick in 1..=last {
        sim.session
            .set_binary_frame_for_test(int(&row["input"]["first_frame"]) as u32 + tick as u32 - 1);
        sim.object_ai_stage(Some(&retail.rules));
        if !compared_produced_child
            && let Some(particle) = sim
                .particle_systems()
                .get(id)
                .and_then(|system| system.particles().first())
        {
            let spark = particle.spark().unwrap();
            let expected = &row["first_produced_particle"];
            assert_eq!(json!(particle.coords().to_array()), expected["world"]);
            assert_eq!(json!(spark.start_rgb), expected["start_rgb"]);
            assert_eq!(json!(spark.color_index), expected["color_index"]);
            assert_eq!(
                format!("{:016x}", spark.color_accumulator.bits()),
                expected["color_accumulator_f64_bits"].as_str().unwrap()
            );
            compared_produced_child = true;
        }
        if tick == int(&retirement["tick"]) {
            assert_eq!(
                system_state(sim.particle_systems().get(id).unwrap(), &roles),
                retirement["system"],
                "last allocated body before the actual drain"
            );
            assert_logical_state(&sim, &roles, id, &retirement["logical"]);
            let children = retirement["pending_particles"].as_array().unwrap();
            assert_eq!(
                sim.particle_systems().detached_particles().len(),
                children.len()
            );
            for expected in children {
                let (child_id, particle, alive, in_limbo) = sim
                    .particle_systems()
                    .detached_particles()
                    .find(|(_, particle, _, _)| {
                        particle.native_unique_id() == int(&expected["native_id"])
                    })
                    .unwrap();
                // The one retained Rust body represents the native Object,
                // Particle and expiry registrations until scalar destruction.
                let body_retained = sim.particle_systems().detached_particle(child_id).is_some();
                for (field, actual) in [
                    ("alive", alive),
                    ("in_limbo", in_limbo),
                    ("marked_for_deletion", particle.marked_for_deletion()),
                    (
                        "display_registered",
                        sim.display_layers().layer_of(child_id).is_some(),
                    ),
                    (
                        "in_logic",
                        sim.live_object_order_snapshot().contains(&child_id),
                    ),
                    (
                        "in_parent_vector",
                        sim.particle_systems().iter().any(|(_, system)| {
                            system.particles().iter().any(|p| p.stable_id() == child_id)
                        }),
                    ),
                    ("object_registered", body_retained),
                    ("particle_array_registered", body_retained),
                    ("expiry_registered", body_retained),
                ] {
                    assert_eq!(
                        json!(actual),
                        expected[field],
                        "child{} {field}",
                        particle.native_unique_id()
                    );
                }
            }
        }
        sim.process_pending_delete();
        if let Some(expected) = boundaries
            .iter()
            .find(|expected| int(&expected["tick"]) == tick)
        {
            assert_eq!(
                sim.particle_systems()
                    .get(id)
                    .map(|system| system_state(system, &roles))
                    .unwrap_or(Value::Null),
                expected["system"],
                "visit{tick}"
            );
            assert_logical_state(&sim, &roles, id, &expected["logical"]);
        }
    }
    assert!(compared_produced_child);
    assert_eq!(
        [
            sim.main_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex()
        ],
        other_before
    );
    assert!(!sim.live_object_order_snapshot().contains(&id));
    assert!(!sim.display_layers().ordered_ids().any(|&value| value == id));
    assert!(sim.particle_systems().get(id).is_none());
}

/// Rust persistence regression, separate from the original executable's
/// queue/AI comparison above. Native in-scenario load owns RNG reseeding.
#[test]
fn detached_particle_snapshot_keeps_bodies_and_queue_until_the_common_drain() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let native = &native()["live_case"];
    let (mut sim, _, _) = birth(&retail.rules, &native["birth"]);
    sim.scenario_rng = SimRng::from_native_state_hex_for_test(
        native["scenario_rng_initial_hex"].as_str().unwrap(),
    );
    let last = int(&native["retirement_before_drain"]["tick"]);
    for tick in 1..=last {
        sim.session.set_binary_frame_for_test(
            int(&native["input"]["first_frame"]) as u32 + tick as u32 - 1,
        );
        sim.object_ai_stage(Some(&retail.rules));
        if tick != last {
            sim.process_pending_delete();
        }
    }
    let retired: Vec<_> = sim
        .particle_systems()
        .detached_particles()
        .map(|(id, particle, alive, in_limbo)| (id, particle.native_unique_id(), alive, in_limbo))
        .collect();
    assert!(
        !retired.is_empty(),
        "the final native cleanup has retained children"
    );
    // Account for the existing native load reinitialization independently of
    // the retained-particle schema. This is no native saved-resource claim.
    sim.scenario_rng = SimRng::new(0);
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "welding-pending", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(
        restored
            .particle_systems()
            .detached_particles()
            .map(|(id, particle, alive, in_limbo)| (
                id,
                particle.native_unique_id(),
                alive,
                in_limbo
            ))
            .collect::<Vec<_>>(),
        retired
    );
    assert_eq!(
        restored.substrate.pending_delete.object_ids(),
        sim.substrate.pending_delete.object_ids()
    );
    assert_eq!(restored.state_hash(), sim.state_hash());
    let mut invalid = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    invalid.substrate.pending_delete = Default::default();
    assert!(matches!(
        invalid.restore_after_snapshot_load(),
        Err(
            crate::sim::snapshot::SnapshotRestoreError::MissingDeferredDeleteIdentity {
                registry: "ParticleSystemStore.detached",
                ..
            }
        )
    ));
    sim.process_pending_delete();
    restored.process_pending_delete();
    assert_eq!(restored.particle_systems().detached_particles().len(), 0);
    assert!(restored.substrate.pending_delete.is_empty());
    assert_eq!(restored.state_hash(), sim.state_hash());
}

/// SystemPointerExpired62FEA4..62FEE2 removes a reference, independently
/// of UnInit. Its stable vector erase must not destroy the allocated body.
#[test]
fn system_child_reference_expiry_retains_the_live_body_until_common_uninit() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let row = &native()["live_case"];
    let (mut sim, _, system_id) = birth(&retail.rules, &row["birth"]);
    sim.scenario_rng =
        SimRng::from_native_state_hex_for_test(row["scenario_rng_initial_hex"].as_str().unwrap());
    sim.object_ai_stage(Some(&retail.rules));
    let children: Vec<_> = sim
        .particle_systems()
        .get(system_id)
        .unwrap()
        .particles()
        .iter()
        .map(|particle| particle.stable_id())
        .collect();
    let child_id = children[0];
    assert!(sim.display_layers().layer_of(child_id).is_some());
    let queues = sim.substrate.pending_delete.object_ids().to_vec();
    assert!(
        sim.particle_systems_mut()
            .pointer_expired(system_id, child_id)
    );
    assert_eq!(
        sim.particle_systems()
            .get(system_id)
            .unwrap()
            .particles()
            .iter()
            .map(|particle| particle.stable_id())
            .collect::<Vec<_>>(),
        children[1..]
    );
    assert!(
        sim.particle_systems()
            .detached_particle(child_id)
            .unwrap()
            .1
    );
    assert!(sim.display_layers().layer_of(child_id).is_some());
    assert_eq!(sim.substrate.pending_delete.object_ids(), queues);
    assert!(
        sim.particle_systems()
            .display_particles()
            .any(|(system, particle)| { system.is_none() && particle.stable_id() == child_id })
    );
    sim.scenario_rng = SimRng::new(0);
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "welding-detached", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(restored.state_hash(), sim.state_hash());

    // The same ObjectUnInit5F65F0 entry that owns entity/Smudge removal
    // must accept this retained Particle, rather than requiring an AI-only
    // copy of its expiry/Limbo/Alive/queue sequence. The native queued-body
    // observations in live_case establish that storage survives this call.
    for world in [&mut sim, &mut restored] {
        // Main is intentionally not serialized. Compare each world's
        // lifecycle calls with its own complete three-stream prior.
        let before_rng = rng(world);
        // ObjectLimbo5F4D30 alone does not clear Alive or queue UnInit.
        // Persisting this boundary must not reconstruct Limbo from Alive.
        world.object_conceal(child_id);
        assert_eq!(
            world
                .particle_systems()
                .detached_particle_lifecycle(child_id),
            Some((true, true))
        );
        assert!(world.display_layers().layer_of(child_id).is_none());
        assert_eq!(world.substrate.pending_delete.object_ids(), queues);
        assert_eq!(rng(world), before_rng);
    }
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "welding-limbo", 0);
    let mut concealed = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    concealed.restore_after_snapshot_load().unwrap();
    assert_eq!(concealed.state_hash(), sim.state_hash());
    for world in [&mut sim, &mut restored] {
        let before_rng = rng(world);
        world.uninit_with_context(
            child_id,
            crate::sim::world::UninitContext::with_rules(&retail.rules),
        );
        assert!(
            !world
                .particle_systems()
                .detached_particle(child_id)
                .unwrap()
                .1
        );
        assert!(world.display_layers().layer_of(child_id).is_none());
        assert_eq!(world.substrate.pending_delete.object_ids(), [child_id]);
        assert_eq!(rng(world), before_rng);
        // ObjectUnInit has no duplicate-call guard: another call appends
        // another ticket, which the existing common drain collapses.
        world.uninit_with_context(
            child_id,
            crate::sim::world::UninitContext::with_rules(&retail.rules),
        );
        assert_eq!(
            world.substrate.pending_delete.object_ids(),
            [child_id, child_id]
        );
        world.process_pending_delete();
        assert!(
            world
                .particle_systems()
                .detached_particle(child_id)
                .is_none()
        );
        assert!(world.substrate.pending_delete.is_empty());
        assert_eq!(rng(world), before_rng);
    }
    assert_eq!(restored.state_hash(), sim.state_hash());
}

#[test]
fn original_live_spark_rof_and_fireat_rearm_use_the_retained_link_and_exact_rng() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let ordinary = native()["birth_cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "ordinary")
        .unwrap();
    for row in native()["admission_cases"].as_array().unwrap() {
        let (mut sim, roles, id) = birth(rules, ordinary);
        let owner = roles["owner"];
        let live = row["input"]["live"].as_bool().unwrap();
        sim.substrate
            .entities
            .get_mut(owner)
            .unwrap()
            .spark_particle_system
            .bind(live.then_some(id));
        restore_rng(&mut sim, &row["rng_before"]);
        let mut weapon = rules.weapon("RepairBullet").unwrap().clone();
        weapon.use_spark_particles = row["input"]["use_spark_particles"].as_bool().unwrap();
        let object = rules.object("FV").unwrap();
        let snap = build_attacker_snapshot(
            sim.substrate.entities.get(owner).unwrap(),
            TargetKind::Entity(roles["target"]),
            None,
        );
        let ((rof, rearm), draws) = crate::sim::rng::trace_draws(|| {
            let rof = fireat_get_rof(
                &mut sim,
                rules,
                &snap,
                object,
                &weapon,
                Some(&weapon),
                int(&row["input"]["queried_burst"]),
            );
            let tail_rof =
                fireat_get_rof(&mut sim, rules, &snap, object, &weapon, Some(&weapon), 1);
            let rearm = fireat_rearm_frames(tail_rof, row["input"]["berserk"].as_bool().unwrap());
            let actor = sim.substrate.entities.get_mut(owner).unwrap();
            actor.rearm_after_fire(int(&row["input"]["frame"]), rearm);
            actor.weapon_burst.complete_shot(weapon.burst.max(1));
            (rof, rearm)
        });
        assert_eq!(rof, int(&row["get_rof"]));
        assert_eq!(rearm, int(&row["rearm"]["rof"]));
        let actor = sim.substrate.entities.get(owner).unwrap();
        assert_eq!(
            actor.rearm_timer.timer().start_frame(),
            int(&row["rearm"]["rearm"][0])
        );
        assert_eq!(
            actor.rearm_timer.timer().duration(),
            int(&row["rearm"]["rearm"][2])
        );
        assert_eq!(actor.weapon_burst.index(), int(&row["rearm"]["burst"]));
        let raw: Vec<_> = row["get_rof_requests"]
            .as_array()
            .unwrap()
            .iter()
            .chain(row["rearm_requests"].as_array().unwrap())
            .flat_map(|request| request["raw_draws"].as_array().unwrap())
            .cloned()
            .collect();
        assert_eq!(
            draws
                .iter()
                .map(|draw| draw["value"].clone())
                .collect::<Vec<_>>(),
            raw
        );
        let expected = &row["rng_after"];
        assert_eq!(
            rng(&sim),
            [
                expected["main"].as_str().unwrap().to_owned(),
                expected["scenario"].as_str().unwrap().to_owned(),
                expected["mapgen"].as_str().unwrap().to_owned()
            ]
        );
    }
}
