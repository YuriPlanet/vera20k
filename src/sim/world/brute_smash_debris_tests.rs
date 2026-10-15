//! Stock Brute Attack -> Infantry FireAt -> InvisibleLow Bullet -> Smashing.
//!
//! Native: Infantry5206B0/51DF60 -> Techno6FDD50 -> Bullet468D80/4690B0,
//! with warhead debris469D06..469EBA. Encounter tests exercise production
//! integration; the tail comparison consumes the bounded original corpus.
//! Retail Hills/Battle rules, ART sequences and physical SHP bindings enter
//! through existing fixtures. Arena, placements and initial rank are supplied.

use super::{FrameEffects, TickLane};
use crate::sim::combat::TargetKind;
use crate::sim::combat::combat_weapon::WeaponSlot;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::mission::{MissionId, MissionType};

fn bound_retail_rules(counts: Option<(i32, i32)>) -> Option<crate::rules::ruleset::RuleSet> {
    let retail = if let Some((minimum, maximum)) = counts {
        crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(
            &crate::rules::ini_parser::IniFile::from_str(&format!(
                "[Smashing]\nMinDebris={minimum}\nMaxDebris={maximum}\n"
            )),
        )?
    } else {
        crate::rules::retail_ini_fixture::retail_battle_rules()?
    };
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()
        .expect("the Battle fixture already opened the retail install");
    let mut rules = retail.rules;
    let roots = crate::rules::effect_asset_catalog::anim_class_roots(&rules);
    rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
    Some(rules)
}

fn stock_brute_smash_emits_debris(elite: bool) {
    let Some(rules) = bound_retail_rules(None) else {
        return;
    };
    assert_eq!(rules.general.metallic_debris.len(), 15);
    assert_eq!(rules.general.metallic_debris.last().unwrap(), "D");
    for name in &rules.general.metallic_debris {
        let config = rules.art().anim_runtime_config(name).unwrap();
        if name != "D" {
            assert!(config.bouncer, "retail {name} is a bouncing chunk");
            assert!(
                config.raw_shp_frame_count.is_some_and(|frames| frames > 0),
                "{name} must bind its physical retail SHP, not a supplied frame count"
            );
        }
    }
    let weapon_name = if elite { "SmashE" } else { "Smash" };
    let weapon = rules.weapon(weapon_name).unwrap();
    assert_eq!(weapon.projectile.as_deref(), Some("InvisibleLow"));
    assert_eq!(weapon.warhead.as_deref(), Some("Smashing"));
    let warhead = rules.warhead("Smashing").unwrap();
    assert_eq!((warhead.min_debris, warhead.max_debris), (1, 3));
    assert!(warhead.debris_types.is_empty());
    assert_eq!(rules.object("MTNK").unwrap().armor, "heavy");

    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 31);
    // Match the map/startup owners before any live combat: canonical object
    // IDs, derived type/warhead handles, and world-local ordered Anim lists.
    // Asset binding above changes ART/image metadata, not the object catalog.
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.resolve_rule_animation_lists(&rules);
    let brute = sim
        .spawn_object("BRUTE", "Americans", 10, 10, 0, &rules)
        .expect("stock Brute Unlimbo on Clear ground");
    let tank = sim
        .spawn_object("MTNK", "Russians", 11, 10, 0, &rules)
        .expect("stock heavy tank Unlimbo on Clear ground");
    if elite {
        // An explicitly elite map/fixture actor, through the veterancy owner.
        sim.substrate
            .entities
            .get_mut(brute)
            .unwrap()
            .veterancy
            .set_elite();
    }
    let source = sim.substrate.entities.get(brute).unwrap();
    let owner = source.owner();
    assert!(sim.houses.get(&owner).unwrap().is_human);
    assert_eq!(source.attack_target.target(), None);
    let target = sim.substrate.entities.get(tank).unwrap();
    let target_owner = target.owner();
    // Unit EnterIdle738B67..738BD3 picks AreaGuard for this arena's armed,
    // unteamed computer tank: its stock skirmish IQ reaches GuardArea.
    // Techno Unlimbo6F6E2A..6F6E4F promotes that ordinary idle selection.
    assert!(sim.houses.get(&target_owner).unwrap().current_iq >= rules.general.iq_guard_area);
    assert_eq!(
        target.mission.current(),
        MissionId::from_known(MissionType::AreaGuard)
    );
    let initial_tank_health = target.health.current();
    assert_eq!(initial_tank_health, 300);
    // Supplied local visibility is scene knowledge. Neither actor receives a
    // raw target/mission/timer edit: the tank keeps its AreaGuard/counterfire.
    sim.fog.mark_visible_for_owner(owner, 11, 10);
    sim.fog.mark_visible_for_owner(target_owner, 10, 10);
    sim.session.current_house = Some(owner);
    sim.queue_command(CommandEnvelope::new(
        owner,
        1,
        Command::Attack {
            attacker_id: brute,
            target_id: tank,
        },
    ));
    assert!(
        sim.anims().next().is_none(),
        "no editor-created or death debris"
    );
    let mut saw_smash = false;
    for _ in 0..180 {
        let commands = sim.take_due_commands();
        let output = sim
            .advance_app_frame(
                &commands,
                Some(&rules),
                None,
                33,
                TickLane::Ordinary,
                None,
                FrameEffects::default(),
            )
            .expect("ordinary Brute combat frame");
        assert!(output.tick.frame_committed);
        for shot in output
            .fire_events
            .iter()
            .filter(|shot| shot.attacker_id == brute)
        {
            assert_eq!(sim.interner.resolve(shot.weapon_id), weapon_name);
            assert_eq!(shot.weapon_slot, WeaponSlot::Secondary);
            assert_eq!(shot.target, TargetKind::Entity(tank));
            saw_smash = true;
        }
        let source = sim
            .substrate
            .entities
            .get(brute)
            .expect("Brute survives its first smash");
        assert!(source.lifecycle.object_alive() && source.health.current() > 0);
        let target = sim
            .substrate
            .entities
            .get(tank)
            .expect("first smash must not kill the tank");
        assert!(target.lifecycle.object_alive() && target.health.current() > 0);
        if target.health.current() < initial_tank_health {
            assert!(
                saw_smash,
                "a real admitted {weapon_name} FireAt precedes tank damage"
            );
            let debris: Vec<_> =
                sim.anims()
                    .filter(|(_, anim)| {
                        rules.general.metallic_debris.iter().any(|name| {
                            name.eq_ignore_ascii_case(sim.interner.resolve(anim.type_id))
                        })
                    })
                    .collect();
            assert!(
                !debris.is_empty(),
                "nonlethal stock {weapon_name} hit must construct Smashing MetallicDebris before the frame ends; tank health {} -> {}, Brute health {}",
                initial_tank_health,
                target.health.current(),
                source.health.current(),
            );
            let ids: Vec<_> = debris.iter().map(|(id, _)| **id).collect();
            let mut expected_children = std::collections::BTreeSet::new();
            for (_, anim) in &debris {
                assert_eq!(anim.draw_flags, 0x600);
                assert_eq!(anim.owner_entity, None);
                assert_eq!(anim.owner_house(), None);
                assert_eq!(anim.attached_bullet(), None);
                assert!(
                    anim.in_logic_vector,
                    "constructor admits each piece to live Logic"
                );
                let config = rules
                    .art()
                    .anim_runtime_config(sim.interner.resolve(anim.type_id))
                    .unwrap();
                expected_children.extend(config.trailer_anim.iter().cloned());
                expected_children.extend(config.expire_anim.iter().cloned());
            }
            // Leave through an ordinary player Move; do not freeze the tank,
            // rewrite missions/targets, or visit a deleted Anim manually.
            sim.queue_command(CommandEnvelope::new(
                owner,
                sim.session.tick + 1,
                Command::Move {
                    entity_id: brute,
                    target_rx: 3,
                    target_ry: 3,
                },
            ));
            // Internal GameSnapshot continuation only: reattach the immutable
            // terrain/rules and independent fixture process streams through
            // the existing restore owners. This does not test app/native
            // SaveGame loading or lift either hold.
            let mut restored = if elite {
                None
            } else {
                let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "brute-debris", 0);
                let mut loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
                    .unwrap()
                    .sim;
                // Full Scenario loading intentionally consumes the saved RNG
                // bytes and restarts at Seed0 (world::deserialize_scenario_rng_reset).
                // This is a Rust live-debris continuation property with an
                // explicitly supplied cursor, not native SaveGame semantics.
                assert_eq!(
                    loaded.scenario_rng.logical_state(),
                    crate::sim::rng::SimRng::new(0).logical_state()
                );
                loaded.scenario_rng = sim.scenario_rng.clone();
                let main = sim.main_rng.snapshot_for_test();
                loaded.main_rng = main.clone().into();
                sim.main_rng = main.into();
                loaded.mapgen_rng = sim.mapgen_rng.clone();
                loaded.restore_after_snapshot_load().unwrap();
                loaded.rebuild_caches_after_load(
                    sim.resolved_terrain.clone().unwrap(),
                    sim.terrain_speed_config.clone(),
                    &rules,
                );
                loaded.resolve_type_handles(&rules);
                assert!(loaded.rebuild_dynamic_navigation(&rules));
                assert_eq!(
                    loaded.state_hash(),
                    sim.state_hash(),
                    "live debris snapshot state; source owners {:?}; restored owners {:?}",
                    sim.hash_owner_diagnostics_for_test(),
                    loaded.hash_owner_diagnostics_for_test(),
                );
                Some(loaded)
            };
            let mut seen_children = std::collections::BTreeSet::new();
            let mut child_ids = std::collections::BTreeSet::new();
            for _ in 0..240 {
                let commands = sim.take_due_commands();
                sim.advance_app_frame(
                    &commands,
                    Some(&rules),
                    None,
                    33,
                    TickLane::Ordinary,
                    None,
                    FrameEffects::default(),
                )
                .expect("live debris continuation frame");
                if let Some(loaded) = &mut restored {
                    let commands = loaded.take_due_commands();
                    loaded
                        .advance_app_frame(
                            &commands,
                            Some(&rules),
                            None,
                            33,
                            TickLane::Ordinary,
                            None,
                            FrameEffects::default(),
                        )
                        .expect("restored live debris continuation frame");
                    assert_eq!(
                        loaded.state_hash(),
                        sim.state_hash(),
                        "debris continuation after internal snapshot"
                    );
                }
                for id in [brute, tank] {
                    let actor = sim
                        .substrate
                        .entities
                        .get(id)
                        .expect("both actors survive debris cleanup");
                    assert!(actor.lifecycle.object_alive() && actor.health.current() > 0);
                }
                for (id, anim) in sim.anims() {
                    let name = sim.interner.resolve(anim.type_id);
                    if expected_children.contains(name) {
                        seen_children.insert(name.to_owned());
                        child_ids.insert(*id);
                    }
                }
                if ids.iter().all(|&id| sim.anim(id).is_none())
                    && child_ids.iter().all(|&id| sim.anim(id).is_none())
                {
                    assert_eq!(
                        seen_children, expected_children,
                        "produced pieces run their retail smoke/landing children"
                    );
                    assert!(
                        ids.iter()
                            .chain(&child_ids)
                            .all(|id| !sim.substrate.pending_delete.contains_object(id))
                    );
                    return;
                }
            }
            panic!(
                "produced chunks and their observed smoke/landing children must retire through ordinary live Logic and pending-delete cleanup"
            );
        }
    }
    panic!("ordinary stock {weapon_name} Attack must produce a nonlethal tank hit");
}

#[test]
fn stock_normal_brute_nonlethal_smash_emits_warhead_debris() {
    stock_brute_smash_emits_debris(false);
}

#[test]
fn stock_elite_brute_nonlethal_smash_emits_warhead_debris() {
    stock_brute_smash_emits_debris(true);
}

/// Native starts at an already-admitted Bullet, through the outer impact or
/// bare4690B0 boundary. Unit ReceiveDamage is a declared native transport;
/// Rust uses a real healthy MTNK receiver without crossing its damage-smoke
/// threshold. Health/Brute launch are covered by the ordinary encounter above.
fn compare_native_smashing_tail(retired: bool) {
    use crate::sim::combat::RAD_NO_ATTACKER;
    use crate::sim::combat::world_receiver::{ReceiverRun, commit_projectiles};
    use crate::sim::native_identity::NativeUniqueIdCursor;
    use crate::sim::projectile::{
        ProjectileCollisionPolicy, ProjectileCoord, ProjectileDetonation,
        ProjectileDetonationReason, ProjectilePayload, ProjectileSpawn, ProjectileTarget,
        ProjectileTrajectory, ProjectileVelocity, ProjectileVisualState, TargetExpiryPolicy,
    };
    use crate::sim::rng::SimRng;
    use serde_json::{Value, json};

    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/brute_smash_debris.json",
    ))
    .unwrap();
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    for art in corpus["initial"]["art"].as_array().unwrap() {
        for asset in art["assets"].as_array().unwrap() {
            let name = asset["name"].as_str().unwrap();
            let loaded = assets
                .load_file_from_mix(name)
                .expect("native corpus physical SHP");
            assert_eq!(json!(loaded.bytes.len()), asset["bytes"], "{name}");
            assert_eq!(
                crate::util::sha256::sha256_hex(&loaded.bytes),
                asset["sha256"].as_str().unwrap(),
                "{name}: same native input bytes"
            );
        }
    }
    let coord = |value: &Value| {
        ProjectileCoord::new(
            value[0].as_i64().unwrap() as i32,
            value[1].as_i64().unwrap() as i32,
            value[2].as_i64().unwrap() as i32,
        )
    };
    for row in corpus["rows"].as_array().unwrap() {
        let input = &row["input"];
        let context = input["name"].as_str().unwrap();
        if retired && context != "single_lg_no_anim" {
            continue;
        }
        let counts = (
            input["minimum"].as_i64().unwrap() as i32,
            input["maximum"].as_i64().unwrap() as i32,
        );
        let Some(rules) = bound_retail_rules((counts != (1, 3)).then_some(counts)) else {
            return;
        };
        assert_eq!(
            json!(rules.general.metallic_debris),
            corpus["initial"]["metallic_pool"]
        );
        let warhead = rules.warhead("Smashing").unwrap();
        assert_eq!((warhead.min_debris, warhead.max_debris), counts);
        assert!(warhead.debris_types.is_empty());
        assert_eq!(
            json!(warhead.debris_types.len()),
            corpus["initial"]["warhead"]["typed_count"]
        );
        let projectile = rules.projectile("InvisibleLow").unwrap();
        assert_eq!(json!(projectile.inviso), input["inviso"]);
        assert_eq!(json!(projectile.cluster), input["cluster"]);
        let mut world = crate::sim::arena_fixture::rival_arena(&rules, 1);
        for cell in &mut world.resolved_terrain.as_mut().unwrap().cells {
            cell.level = 6;
        }
        world.intern_rule_type_ids(&rules);
        world.resolve_type_handles(&rules);
        world.resolve_rule_animation_lists(&rules);
        world.session.binary_frame = input["binary_frame"].as_u64().unwrap() as u32;
        let live = coord(&input["live"]);
        let placement = coord(&input["placement"]);
        let receiver = if input["receiver"] != "empty" {
            let id = world
                .spawn_object("MTNK", "Americans", 10, 20, 0, &rules)
                .unwrap();
            if input["receiver"] == "iron_curtain" {
                world
                    .substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .invulnerability = Some(
                    crate::sim::superweapon::invulnerability::InvulnerabilityState::new(
                        crate::sim::timer::CdTimer::started(
                            input["ic_start"].as_i64().unwrap() as i32,
                            input["ic_duration"].as_i64().unwrap() as i32,
                        ),
                        crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain,
                    ),
                );
            }
            Some(id)
        } else {
            None
        };
        world.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(
            row["native_cursor_before"].as_u64().unwrap() as u32,
        ));
        world.scenario_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["scenario"].as_str().unwrap());
        world.main_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["main"].as_str().unwrap())
                .into();
        world.mapgen_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["mapgen"].as_str().unwrap());
        let payload = ProjectilePayload::new(
            input["damage"].as_i64().unwrap() as i32,
            world.interner.intern("Smashing"),
            Some(world.interner.intern("InvisibleLow")),
            false,
        )
        .with_installed_weapon(Some(world.interner.intern("Smash")));
        let target = receiver.map_or(
            ProjectileTarget::Cell { rx: 10, ry: 20 },
            ProjectileTarget::Entity,
        );
        let id = world.allocate_stable_id();
        world.admit_projectile(
            id,
            ProjectileSpawn {
                native_unique_id: row["bullet_native_id"].as_i64().unwrap() as i32,
                line_trail: None,
                flat: false,
                source_id: RAD_NO_ATTACKER,
                origin: live,
                target,
                initial_target_position: placement,
                payload,
                speed_leptons_per_frame: 1,
                velocity: ProjectileVelocity::new(0, 0, 0),
                trajectory: ProjectileTrajectory::Straight,
                guidance: None,
                visual: ProjectileVisualState::new(0, 0, 0),
                arm_frames: 0,
                fuse_frames: None,
                ranged_fuse: false,
                tracks_target: false,
                target_expiry: TargetExpiryPolicy::DetonateAtLastKnown,
                collision: ProjectileCollisionPolicy::NONE,
            },
        );
        // The corpus starts with a supplied already-admitted Inviso Bullet
        // at +9C=live, rather than executing Bullet::Fire. Common admission
        // applies Unlimbo's floor+1; Fire's existing Inviso placement owner
        // then overwrites it (4688B7..468A39). Keep the native input unchanged.
        world.projectiles.fire_inviso(id, live, false);
        assert_eq!(world.projectiles.get(id).unwrap().position, live);
        if retired {
            // Production retirement clears live membership and queues late
            // deletion; retained store presence must not resurrect the tail.
            assert!(world.retire_non_entity_object(id, super::UninitContext::with_rules(&rules)));
            assert!(world.projectiles.get(id).is_some());
            assert!(world.substrate.pending_delete.contains_object(&id));
        }
        let result = commit_projectiles(
            &mut world,
            &mut ReceiverRun::default(),
            &[ProjectileDetonation {
                projectile_id: id,
                source_id: RAD_NO_ATTACKER,
                target,
                impact: placement,
                payload,
                // Existing DeathWeapon reason selects only the bare native entry,
                // not a claim to temporary death-weapon creation in this fixture.
                reason: if input["route"] == "detonate" {
                    ProjectileDetonationReason::DeathWeapon
                } else {
                    ProjectileDetonationReason::ReachedTarget
                },
            }],
            &rules,
            None,
        );
        assert!(
            result.effects.explosion_effects.is_empty(),
            "{context}: inline constructors"
        );
        if retired {
            // Plain469A88..94/469038..40 control flow: no Inviso, selected
            // Anim, debris budget/pick/constructor or cluster successor runs.
            assert!(world.anims().next().is_none());
            assert_eq!(
                world.scenario_rng.native_state_hex(),
                row["rng_before"]["scenario"].as_str().unwrap()
            );
            assert_eq!(
                json!(world.native_unique_ids.as_ref().unwrap().current_raw()),
                row["native_cursor_before"]
            );
            assert!(world.substrate.pending_delete.contains_object(&id));
            continue;
        }
        for (actual, key) in [
            (world.scenario_rng.native_state_hex(), "scenario"),
            (world.main_rng.native_state_hex(), "main"),
            (world.mapgen_rng.native_state_hex(), "mapgen"),
        ] {
            assert_eq!(
                actual,
                row["rng_after"][key].as_str().unwrap(),
                "{context}/{key} full stream state"
            );
        }
        assert_eq!(
            json!(world.native_unique_ids.as_ref().unwrap().current_raw()),
            row["native_cursor_after"],
            "{context}: common identity cursor"
        );
        let actual: Vec<_> = world.anims().map(|(_, anim)| anim).collect();
        let expected = row["anims"].as_array().unwrap();
        assert_eq!(
            actual.len(),
            expected.len(),
            "{context}: synchronous births"
        );
        for (anim, expected) in actual.iter().zip(expected) {
            assert_eq!(
                world.interner.resolve(anim.type_id),
                expected["type"].as_str().unwrap(),
                "{context}: constructor order"
            );
            assert_eq!(
                json!([anim.world_coord.x, anim.world_coord.y, anim.world_coord.z]),
                expected["location"],
                "{context}: live GetCoords versus placement"
            );
            assert_eq!(
                json!(anim.native_unique_id),
                expected["native_id"],
                "{context}"
            );
            assert_eq!(json!(anim.draw_flags), expected["draw_flags"], "{context}");
            assert_eq!(json!(anim.z_adjust), expected["z_adjust"], "{context}");
            assert_eq!(
                json!(anim.in_logic_vector),
                expected["logic_registered"],
                "{context}"
            );
            assert_eq!(
                json!({
                    "current_frame": anim.runtime.current_frame,
                    "frame_step": anim.runtime.frame_step,
                    "delay_remaining": anim.runtime.delay_remaining,
                    "rate_reload": anim.runtime.rate_reload,
                    "frame_timer": [anim.runtime.frame_timer.start_frame(), anim.runtime.frame_timer.duration()],
                    "loop_remaining": anim.runtime.loop_remaining,
                    "first_ai_guard": u8::from(anim.runtime.first_ai_guard),
                    "constructor_reverse": u8::from(anim.runtime.constructor_reverse),
                    "inactive": u8::from(anim.runtime.inactive),
                    "paused": u8::from(anim.runtime.paused),
                }),
                expected["runtime"],
                "{context}: full constructor runtime"
            );
            assert_eq!(anim.owner_entity, None);
            assert_eq!(anim.owner_house(), None);
            assert_eq!(anim.attached_bullet(), None);
            assert_eq!(
                anim.bounce.is_some(),
                expected["is_bouncing"] == 1,
                "{context}"
            );
            if let Some(body) = anim.bounce {
                assert_eq!(
                    json!(body.position.map(|v| v.bits())),
                    expected["bounce"]["position_bits"],
                    "{context}"
                );
                assert_eq!(
                    json!(body.velocity.map(|v| v.bits())),
                    expected["bounce"]["velocity_bits"],
                    "{context}"
                );
                assert_eq!(
                    json!(body.elasticity.bits()),
                    expected["bounce"]["elasticity_bits"],
                    "{context}"
                );
                assert_eq!(
                    json!(body.gravity.bits()),
                    expected["bounce"]["gravity_bits"],
                    "{context}"
                );
            }
        }
    }
}

#[test]
fn original_smashing_tail_preserves_constructor_rng_ids_and_live_coordinates() {
    compare_native_smashing_tail(false);
}

#[test]
fn retired_bullet_retains_storage_but_cannot_run_detonation_tail_or_cluster_rng() {
    compare_native_smashing_tail(true);
}
