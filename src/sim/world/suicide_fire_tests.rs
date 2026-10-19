//! Stock TERROR FireAt through the existing synchronous receiver and death owners.

use super::{FrameEffects, Simulation};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{TargetKind, world_fire::FireVisit};

struct TerrorScene {
    rules: RuleSet,
    sim: Simulation,
    source: u64,
    target: u64,
}

fn retail_scene() -> Option<TerrorScene> {
    let mut rules = crate::rules::retail_ini_fixture::retail_battle_rules()?.rules;
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
    let mut roots = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&rules);
    roots.extend(
        rules
            .general
            .damage_fire_types
            .iter()
            .map(|anim| anim.name.clone()),
    );
    rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
    rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
    let object = rules.object("TERROR").unwrap();
    assert_eq!(object.strength, 75);
    assert_eq!(object.primary(), Some("TerrorBomb"));
    assert_eq!(object.death_weapon.as_deref(), Some("TerrorBomb"));
    assert!(rules.weapon("TerrorBomb").unwrap().suicide);
    assert_eq!(rules.bridge_warheads.c4_name, "Super");

    // Declared flat admitted-world prior. Both actors and their effects use
    // retail definitions; the separate app captures use the authored Cuba map.
    let mut sim = Simulation::with_seed(31);
    crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    for (name, human) in [("Confederation", true), ("Americans", false)] {
        let id = sim.interner.intern(name);
        let side = rules.country_side_index_raw(name).unwrap();
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                id,
                side,
                Some(id),
                human,
                10_000,
                10,
            ));
    }
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.resolve_rule_animation_lists(&rules);
    let target = sim
        .spawn_object("GAPOWR", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let held_source = sim
        .spawn_object_limbo_at_height("TERROR", "Confederation", 9, 10, 0, 0, &rules)
        .unwrap();
    // Match the original Unlimbo request, rather than asking the grid API
    // for its different cell-center input and randomly selected subcell.
    let source = sim
        .reveal_constructed_object_at_coord_with_overlay_context(
            held_source,
            crate::sim::components::DriveCoord {
                x: 2496,
                y: 2752,
                z: 0,
            },
            0,
            super::PlacementEvidence::EvaluateMark,
            &rules,
            None,
            FrameEffects::default(),
        )
        .unwrap();
    let owner = sim.substrate.entities.get(source).unwrap().owner();
    sim.session.set_current_house_for_test(Some(owner));
    for y in 10..12 {
        for x in 10..12 {
            sim.fog.mark_visible_for_owner(owner, x, y);
        }
    }
    Some(TerrorScene {
        rules,
        sim,
        source,
        target,
    })
}

fn native_phase<'a>(row: &'a serde_json::Value, label: &str) -> &'a serde_json::Value {
    row["phases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|phase| phase["label"] == label)
        .unwrap_or_else(|| panic!("missing native phase {label}"))
}

fn compare_native_fields(actual: &serde_json::Value, expected: &serde_json::Value, path: &str) {
    if let Some(fields) = actual.as_object() {
        for (key, value) in fields {
            compare_native_fields(value, &expected[key], &format!("{path}.{key}"));
        }
    } else {
        assert_eq!(actual, expected, "{path}");
    }
}

fn compare_native_rng(sim: &Simulation, phase: &serde_json::Value) {
    for (stream, state) in [
        ("main", sim.main_rng.native_state_hex()),
        ("scenario", sim.scenario_rng.native_state_hex()),
        ("mapgen", sim.mapgen_rng.native_state_hex()),
    ] {
        assert_eq!(
            state,
            phase["rng"][stream].as_str().unwrap(),
            "{} {stream}",
            phase["label"]
        );
    }
}

fn compare_native_phase(scene: &TerrorScene, phase: &serde_json::Value) {
    use serde_json::json;

    let sim = &scene.sim;
    let source = sim.substrate.entities.get(scene.source).unwrap();
    let target = sim.substrate.entities.get(scene.target).unwrap();
    let location = crate::sim::movement::ground_pose::position_world_coord(&source.position);
    let stage = source.native_stage();
    let actual = json!({
        "frame": sim.session.binary_frame(),
        "health": source.health.current(),
        "alive": u8::from(source.lifecycle.object_alive()),
        "limbo": u8::from(source.lifecycle.in_limbo()),
        "marked": u8::from(source.lifecycle.cell_marked()),
        "logic": sim.live_object_order_snapshot().contains(&scene.source),
        "location": [location.x, location.y, location.z],
        "target": if source.attack_target.target().is_none() { "null" } else { "target" },
        "current_weapon": source.current_weapon_number(),
        "mission": source.mission.current().raw(),
        "pending": source.mission_leaf.foot_firing_sequence_latch(),
        "doing": source.mission_leaf.as_infantry().unwrap().doing(),
        "stage": stage.value(),
        "sequence": {"start":stage.timer().start_frame(), "duration":stage.timer().duration(), "rate":stage.rate()},
        "rearm": {"start":source.rearm_timer.start_frame(), "duration":source.rearm_timer.duration()},
        "burst": source.weapon_burst.index(),
        "last_fire": source.last_fire_frame.frame(),
        "target_health": target.health.current(),
        "target_alive": u8::from(target.lifecycle.object_alive()),
    });
    compare_native_fields(&actual, &phase["source"], phase["label"].as_str().unwrap());
    compare_native_rng(sim, phase);
}

#[test]
fn stock_terror_suicide_short_circuits_normal_fire() {
    use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
    use crate::sim::mission::{MissionId, MissionType};
    use crate::sim::rng::SimRng;

    let corpus = crate::test_fixture::json("tools/spatial_oracle/suicide_fire.json");
    assert_eq!(corpus["rows"].as_array().unwrap().len(), 3);
    for row in corpus["rows"].as_array().unwrap() {
        let Some(mut scene) = retail_scene() else {
            return;
        };
        let object = scene.rules.object("TERROR").unwrap();
        let weapon = scene.rules.weapon(object.primary().unwrap()).unwrap();
        compare_native_fields(
            &serde_json::json!({
                "strength": object.strength,
                "primary": object.primary(),
                "death_weapon": object.death_weapon,
                "explodes": u8::from(object.explodes),
                "suicide": u8::from(weapon.suicide),
                "damage": weapon.damage,
                "rof": weapon.rof,
                "projectile": weapon.projectile,
                "warhead": weapon.warhead,
                "c4_warhead": scene.rules.bridge_warheads.c4_name,
                "corpse_names": scene.rules.general.dead_bodies,
            }),
            &corpus["inputs"]["native"],
            "physical native readers",
        );
        let ordinary = row["name"] == "ordinary_5206b0";
        let initial = native_phase(row, "before_preparation");
        for (id, key) in [(scene.source, "source_xyz"), (scene.target, "target_xyz")] {
            let entity = scene.sim.substrate.entities.get(id).unwrap();
            let location =
                crate::sim::movement::ground_pose::position_world_coord(&entity.position);
            assert_eq!(
                serde_json::json!([location.x, location.y, location.z]),
                row["input"][key]
            );
        }
        // Original constructor/Unlimbo owns placement. Only the three measured
        // stream priors are imported; the real Rust RNG produces every word.
        scene.sim.main_rng =
            SimRng::from_native_state_hex_for_test(initial["rng"]["main"].as_str().unwrap()).into();
        scene.sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(initial["rng"]["scenario"].as_str().unwrap());
        scene.sim.mapgen_rng =
            SimRng::from_native_state_hex_for_test(initial["rng"]["mapgen"].as_str().unwrap());
        let (_, draws) = crate::sim::rng::trace_draws(|| {
            compare_native_phase(&scene, initial);
            if !row["preparation"].is_null() {
                let c4 = scene
                    .sim
                    .interner
                    .intern(&scene.rules.bridge_warheads.c4_name);
                let event = EntityDamageEvent::direct_receiver(
                    scene.source,
                    row["preparation"]["damage"].as_i64().unwrap() as i32,
                    0,
                    RAD_NO_ATTACKER,
                    None,
                    c4,
                    ReceiverCallFlags {
                        ignore_defenses: true,
                        arg6: false,
                    },
                );
                scene.sim.commit_direct_damage_receiver(
                    &scene.rules,
                    None,
                    event,
                    FrameEffects::default(),
                );
            }
            scene
                .sim
                .assign_target_represented(
                    scene.source,
                    Some(TargetKind::Entity(scene.target)),
                    Some(&scene.rules),
                    None,
                    FrameEffects::default(),
                )
                .unwrap();
            if ordinary {
                scene
                    .sim
                    .mission_queue_exact(
                        scene.source,
                        MissionId::from_known(MissionType::Attack),
                        0,
                        scene.sim.session.binary_frame(),
                        &crate::sim::mission::authority::EntityReadyInputProvider,
                    )
                    .unwrap();
                scene
                    .sim
                    .mission_commence_exact(scene.source, scene.sim.session.binary_frame())
                    .unwrap();
            }
            compare_native_phase(&scene, native_phase(row, "before_fire"));
            scene.sim.commit_fire_visit(
                if ordinary {
                    FireVisit::InfantryTarget(scene.source)
                } else {
                    FireVisit::Direct {
                        id: scene.source,
                        target: TargetKind::Entity(scene.target),
                        weapon_index: 0,
                    }
                },
                &scene.rules,
                None,
                FrameEffects::default(),
            );
            let ai = super::techno_ai::ObjectAiCtx {
                effects: FrameEffects::default(),
                overlay_registry: None,
                terrain_spawner_cells: None,
                miner_config: None,
            };
            if ordinary {
                compare_native_phase(&scene, native_phase(row, "after_schedule_start"));
                for frame in 1..=native_phase(row, "after_fire")["source"]["frame"]
                    .as_u64()
                    .unwrap() as u32
                {
                    scene.sim.session.set_binary_frame_for_test(frame);
                    scene
                        .sim
                        .advance_live_object_turn(scene.source, Some(&scene.rules), ai)
                        .unwrap();
                }
            }
            compare_native_phase(&scene, native_phase(row, "after_fire"));
            // The suicide leaves a corpse that keeps IsAlive at Health 0.
            let source = scene.sim.substrate.entities.get(scene.source).unwrap();
            assert!(source.lifecycle.object_alive() && !source.death.is_dying());
            assert_eq!(source.health.current(), 0);
            assert!(
                scene
                    .sim
                    .projectiles
                    .iter()
                    .all(|(_, bullet)| bullet.source_id != scene.source),
                "the nested DeathWeapon finishes synchronously; no ordinary outgoing shot remains"
            );
            for visit in row["death_visits"].as_array().unwrap() {
                scene
                    .sim
                    .session
                    .set_binary_frame_for_test(visit["frame"].as_u64().unwrap() as u32);
                scene
                    .sim
                    .advance_live_object_turn(scene.source, Some(&scene.rules), ai)
                    .unwrap();
                let source = scene.sim.substrate.entities.get(scene.source).unwrap();
                compare_native_fields(
                    &serde_json::json!({"health":source.health.current(),"alive":u8::from(source.lifecycle.object_alive()),"logic":scene.sim.live_object_order_snapshot().contains(&scene.source),"doing":source.mission_leaf.as_infantry().unwrap().doing(),"stage":source.native_stage().value()}),
                    visit,
                    "death visit",
                );
            }
            compare_native_phase(&scene, native_phase(row, "before_deferred_destruction"));
            scene.sim.process_pending_delete_with(
                Some(&scene.rules),
                None,
                FrameEffects::default(),
                1,
                true,
            );
            assert!(scene.sim.substrate.entities.get(scene.source).is_none());
            compare_native_rng(&scene.sim, native_phase(row, "after_deferred_destruction"));
        });
        let expected_words: Vec<_> = row["rng_requests"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|request| request["raw_draws"].as_array().unwrap().iter().cloned())
            .collect();
        assert_eq!(
            draws
                .iter()
                .map(|draw| draw["value"].clone())
                .collect::<Vec<_>>(),
            expected_words,
            "{} ordered draws",
            row["name"]
        );
        // This native list is constructor history. Building active anims may
        // be replaced during the four receivers; it is not a final live list.
        // Compare this source's impact and corpse. The admitted building's
        // earlier active-anim history is a separate declared fixture prior.
        let source_anims: Vec<_> = row["anims"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|anim| anim["type"] == "MININUKE" || anim["phase"] == "dying_ai")
            .collect();
        assert_eq!(source_anims.len(), 2);
        for expected in source_anims {
            assert!(
                scene.sim.substrate.anims.iter().any(|(_, anim)| {
                    scene.sim.interner.resolve(anim.type_id) == expected["type"].as_str().unwrap()
                        && serde_json::json!([
                            anim.world_coord.x,
                            anim.world_coord.y,
                            anim.world_coord.z
                        ]) == expected["location"]
                }),
                "{} anim {}",
                row["name"],
                expected["type"]
            );
        }
    }
}

#[test]
fn ordinary_terror_attack_finishes_die2_and_retires_its_source() {
    use crate::sim::command::{Command, CommandEnvelope};

    let Some(mut scene) = retail_scene() else {
        return;
    };
    // Stock power plants keep both houses alive under ShortGame while the
    // source's own death sequence runs. Neither has a weapon.
    for (name, house, cell) in [
        ("NAPOWR", "Confederation", (4, 4)),
        ("GAPOWR", "Americans", (27, 27)),
    ] {
        scene
            .sim
            .spawn_object(name, house, cell.0, cell.1, 0, &scene.rules)
            .unwrap();
    }
    let source = scene.sim.substrate.entities.get(scene.source).unwrap();
    let owner = source.owner();
    let rearm = source.rearm_timer;
    let last_fire = source.last_fire_frame;
    scene.sim.queue_command(CommandEnvelope::new(
        owner,
        1,
        Command::Attack {
            attacker_id: scene.source,
            target_id: scene.target,
        },
    ));
    let mut saw_die2 = false;
    let mut retired = false;
    for _ in 0..100 {
        let commands = scene.sim.take_due_commands();
        let output = scene
            .sim
            .advance_app_frame(
                &commands,
                Some(&scene.rules),
                None,
                33,
                super::TickLane::Ordinary,
                FrameEffects::default(),
            )
            .unwrap();
        assert!(output.tick.frame_committed);
        assert!(
            !scene.sim.houses.get(&owner).unwrap().is_defeated,
            "ordinary source cleanup must not be a house-defeat removal"
        );
        assert!(
            output
                .fire_events
                .iter()
                .all(|shot| shot.attacker_id != scene.source),
            "Suicide has no ordinary outgoing shot"
        );
        match scene.sim.substrate.entities.get(scene.source) {
            Some(source) if source.health.current() == 0 => {
                saw_die2 = true;
                assert!(!source.death.is_dying() && source.lifecycle.object_alive());
                assert_eq!(source.mission_leaf.as_infantry().unwrap().doing(), 12);
                assert_eq!(source.rearm_timer, rearm);
                assert_eq!(source.last_fire_frame.frame(), last_fire.frame());
                assert!(source.attack_target.target().is_none());
            }
            Some(source) => assert_eq!(source.health.current(), 75),
            None => {
                retired = true;
                break;
            }
        }
    }
    assert!(
        saw_die2,
        "the fatal receiver must retain the ordinary Die2 sequence"
    );
    assert!(
        retired,
        "the sequence must finish through ordinary UnInit and retirement"
    );
    assert!(
        scene.sim.substrate.anims.iter().any(|(_, anim)| {
            let name = scene.sim.interner.resolve(anim.type_id);
            scene
                .rules
                .general
                .dead_bodies
                .iter()
                .any(|body| body == name)
        }),
        "the completed sequence must leave its retail corpse"
    );
    assert!(
        scene
            .sim
            .substrate
            .entities
            .get(scene.target)
            .unwrap()
            .health
            .current()
            > 0
    );
}

#[test]
fn direct_suicide_blast_reports_building_loss() {
    assert_suicide_blast_reports_building_loss(true);
}

#[test]
fn ordinary_suicide_blast_reports_building_loss() {
    assert_suicide_blast_reports_building_loss(false);
}

fn assert_suicide_blast_reports_building_loss(direct: bool) {
    use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
    use crate::sim::command::{Command, CommandEnvelope};

    let Some(mut scene) = retail_scene() else {
        return;
    };
    // The target entered Logic before its attacker: a later target AI visit
    // cannot accidentally supply the firing caller's missing notification.
    // The same stock receiver reaches a real wounded target. No HP field
    // write or replacement damage/death producer supplies the outcome.
    let c4 = scene
        .sim
        .interner
        .intern(&scene.rules.bridge_warheads.c4_name);
    scene.sim.commit_direct_damage_receiver(
        &scene.rules,
        None,
        EntityDamageEvent::direct_receiver(
            scene.target,
            650,
            0,
            RAD_NO_ATTACKER,
            None,
            c4,
            ReceiverCallFlags {
                ignore_defenses: true,
                arg6: false,
            },
        ),
        FrameEffects::default(),
    );
    assert_eq!(
        scene
            .sim
            .substrate
            .entities
            .get(scene.target)
            .unwrap()
            .health
            .current(),
        100
    );
    if direct {
        let receipt = scene.sim.commit_fire_visit(
            FireVisit::Direct {
                id: scene.source,
                target: TargetKind::Entity(scene.target),
                weapon_index: 0,
            },
            &scene.rules,
            None,
            FrameEffects::default(),
        );
        assert!(
            scene
                .sim
                .substrate
                .entities
                .get(scene.target)
                .unwrap()
                .death
                .is_dying()
        );
        assert!(
            receipt.structure_destroyed,
            "nested DeathWeapon loss must reach its direct FireAt caller"
        );
        return;
    }
    for (name, house, cell) in [
        ("NAPOWR", "Confederation", (4, 4)),
        ("GAPOWR", "Americans", (27, 27)),
    ] {
        scene
            .sim
            .spawn_object(name, house, cell.0, cell.1, 0, &scene.rules)
            .unwrap();
    }
    let owner = scene
        .sim
        .substrate
        .entities
        .get(scene.source)
        .unwrap()
        .owner();
    scene.sim.queue_command(CommandEnvelope::new(
        owner,
        1,
        Command::Attack {
            attacker_id: scene.source,
            target_id: scene.target,
        },
    ));
    let mut killed = false;
    for _ in 0..100 {
        let commands = scene.sim.take_due_commands();
        let output = scene
            .sim
            .advance_app_frame(
                &commands,
                Some(&scene.rules),
                None,
                33,
                super::TickLane::Ordinary,
                FrameEffects::default(),
            )
            .unwrap();
        if scene
            .sim
            .substrate
            .entities
            .get(scene.target)
            .is_none_or(|target| target.health.current() <= 0)
        {
            assert!(
                output.tick.destroyed_structure,
                "ordinary Infantry fire must publish the same-frame building loss"
            );
            killed = true;
            break;
        }
    }
    assert!(
        killed,
        "the ordinary Suicide blast must kill the wounded building"
    );
}
