//! Stock Brute Attack -> Infantry FireAt -> InvisibleLow Bullet -> Smashing.
//!
//! Native: Infantry5206B0/51DF60 -> Techno6FDD50 -> Bullet468D80/4690B0,
//! with warhead debris469D06..469EBA. Encounter tests exercise production
//! integration; the tail comparison consumes the bounded original corpus.
//! Retail Hills/Battle rules, ART sequences and physical SHP bindings enter
//! through existing fixtures. Arena, placements and initial rank are supplied.
//! An explicit authored TIRE list patch also uses this ordinary encounter.

use super::{FrameEffects, TickLane};
use crate::sim::combat::TargetKind;
use crate::sim::combat::combat_weapon::WeaponSlot;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::mission::{MissionId, MissionType};

fn bound_retail_rules(
    patch: Option<&crate::rules::ini_parser::IniFile>,
) -> Option<crate::rules::ruleset::RuleSet> {
    let retail = if let Some(patch) = patch {
        crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(patch)?
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

enum SmashDebrisExpectation {
    StockMetallic,
    AuthoredTire,
}

fn brute_smash_emits_debris(elite: bool, expected: SmashDebrisExpectation) {
    let typed_tire = matches!(expected, SmashDebrisExpectation::AuthoredTire);
    // Explicit authored scenario input. Normal stock Brutes have an empty
    // typed list; all weapon, count, TIRE and combat tuning stays retail.
    let patch = typed_tire.then(|| {
        crate::rules::ini_parser::IniFile::from_str(
            "[Smashing]\nDebrisTypes=TIRE\nDebrisMaximums=4\n",
        )
    });
    let Some(rules) = bound_retail_rules(patch.as_ref()) else {
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
    let tire_id = if typed_tire {
        assert_eq!(warhead.debris_types, vec!["TIRE".to_string()]);
        assert_eq!(warhead.debris_maximums, vec![4]);
        let id = rules.voxel_anim_type_id_by_name("TIRE").unwrap();
        let tire = rules.voxel_anim_type(id);
        assert_eq!(tire.name, "TIRE");
        assert_eq!(tire.image, "TIRE");
        assert_eq!(tire.duration, 150);
        assert!(!tire.is_meteor && !tire.is_tiberium);
        assert!(tire.attached_system.is_none() && tire.start_sound.is_none());
        let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
        let model = crate::assets::vxl_file::VxlFile::from_bytes(
            assets
                .get_ref("TIRE.VXL")
                .expect("physical retail TIRE model"),
        )
        .unwrap();
        assert!(!model.limbs.is_empty());
        assert!(model.limbs.iter().any(|limb| !limb.voxels.is_empty()));
        Some(id)
    } else {
        assert!(warhead.debris_types.is_empty());
        None
    };
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
    assert!(sim.voxel_anims().iter().next().is_none());
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
            if let Some(tire_id) = tire_id {
                assert_eq!(initial_tank_health, 300);
                assert_eq!(
                    target.health.current(),
                    200,
                    "ordinary nonlethal Smash damage"
                );
                assert!(
                    debris.is_empty(),
                    "typed list suppresses MetallicDebris fallback"
                );
                let pieces: Vec<_> = sim.voxel_anims().iter().collect();
                assert!(
                    !pieces.is_empty(),
                    "authored Smashing DebrisTypes=TIRE hit must construct VoxelAnim debris before the frame ends"
                );
                let target_coord = crate::sim::movement::ground_pose::object_get_coords(
                    target,
                    sim.resolved_terrain.as_ref(),
                );
                for (_, piece) in pieces {
                    assert_eq!(piece.type_id, tire_id);
                    assert_eq!(piece.owner_house, None, "Bullet passes a null debris owner");
                    assert!(piece.in_logic_vector && !piece.marked_for_deletion);
                    assert!(
                        piece.duration > 0
                            && piece.duration <= rules.voxel_anim_type(tire_id).duration
                    );
                    // The frame may already visit the newborn Bounce body.
                    // Check its live Inviso target region, not an invented
                    // inverse trajectory or an exact constructor golden.
                    let coord = piece.world_coord();
                    assert_eq!(coord.x.div_euclid(256), target_coord.x.div_euclid(256));
                    assert_eq!(coord.y.div_euclid(256), target_coord.y.div_euclid(256));
                    assert!(coord.z > target_coord.z);
                }
            } else {
                assert!(
                    !debris.is_empty(),
                    "nonlethal stock {weapon_name} hit must construct Smashing MetallicDebris before the frame ends; tank health {} -> {}, Brute health {}",
                    initial_tank_health,
                    target.health.current(),
                    source.health.current(),
                );
            }
            let ids: Vec<_> = if typed_tire {
                sim.voxel_anims().iter().map(|(&id, _)| id).collect()
            } else {
                debris.iter().map(|(id, _)| **id).collect()
            };
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
                if ids
                    .iter()
                    .all(|&id| sim.anim(id).is_none() && sim.voxel_anims().get(id).is_none())
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
                    assert!(
                        ids.iter()
                            .all(|&id| sim.display_layers().layer_of(id).is_none())
                    );
                    sim.debug_assert_logic_membership_consistent();
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
    brute_smash_emits_debris(false, SmashDebrisExpectation::StockMetallic);
}

#[test]
fn stock_elite_brute_nonlethal_smash_emits_warhead_debris() {
    brute_smash_emits_debris(true, SmashDebrisExpectation::StockMetallic);
}

/// Original typed tail469D40..469E18 constructs VoxelAnim7493B0 at live
/// Bullet GetCoords with null owner. This scenario authors only the two
/// Smashing list keys; it is not an unmodified stock Brute effect.
#[test]
fn authored_tire_brute_nonlethal_smash_emits_typed_warhead_debris() {
    brute_smash_emits_debris(false, SmashDebrisExpectation::AuthoredTire);
}

fn native_bounce_fields(body: &crate::sim::bounce::BounceState) -> serde_json::Value {
    serde_json::json!({
        "elasticity_bits": body.elasticity.bits(),
        "gravity_bits": body.gravity.bits(),
        "clamp_bits": body.angular_velocity_magnitude.bits(),
        "position_bits": body.position.map(|v| v.bits()),
        "velocity_bits": body.velocity.map(|v| v.bits()),
        "orientation_bits": body.orientation.map(|v| v.bits()),
        "rotation_bits": body.rotation.map(|v| v.bits()),
    })
}

/// Native starts at an already-admitted Bullet, through the outer impact or
/// bare4690B0 boundary. Unit ReceiveDamage is a declared native transport;
/// Rust uses a real healthy MTNK receiver without crossing its damage-smoke
/// threshold. Health/Brute launch are covered by the ordinary encounter above.
fn compare_native_smashing_tail(retired: bool, typed: bool) {
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

    let corpus: Value = serde_json::from_str(crate::test_fixture::text(if typed {
        "tools/projectile_oracle/typed_warhead_debris.json"
    } else {
        "tools/projectile_oracle/brute_smash_debris.json"
    }))
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
    if typed {
        for asset in corpus["initial"]["tire"]["assets"].as_array().unwrap() {
            let name = asset["name"].as_str().unwrap();
            let loaded = assets
                .load_file_from_mix(name)
                .expect("native corpus physical TIRE model");
            assert_eq!(json!(loaded.bytes.len()), asset["size"], "{name}");
            assert_eq!(
                crate::util::sha256::sha256_hex(&loaded.bytes),
                asset["sha256"].as_str().unwrap(),
                "{name}: same native model bytes"
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
        let patch = (typed || counts != (1, 3)).then(|| {
            let mut text = format!(
                "[Smashing]\nMinDebris={}\nMaxDebris={}\n",
                counts.0, counts.1
            );
            if typed {
                let names: Vec<_> = input["debris_types"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|name| name.as_str().unwrap())
                    .collect();
                let maximums: Vec<_> = input["debris_maximums"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| n.as_i64().unwrap().to_string())
                    .collect();
                text.push_str(&format!(
                    "DebrisTypes={}\nDebrisMaximums={}\n",
                    names.join(","),
                    maximums.join(",")
                ));
            }
            crate::rules::ini_parser::IniFile::from_str(&text)
        });
        let Some(rules) = bound_retail_rules(patch.as_ref()) else {
            return;
        };
        assert_eq!(
            json!(rules.general.metallic_debris),
            corpus["initial"]["metallic_pool"]
        );
        let warhead = rules.warhead("Smashing").unwrap();
        assert_eq!((warhead.min_debris, warhead.max_debris), counts);
        if typed {
            let read = &row["warhead_after_read"];
            assert_eq!(
                json!(warhead.debris_types),
                read["debris_types"],
                "{context}"
            );
            assert_eq!(
                json!(warhead.debris_maximums),
                read["debris_maximums"],
                "{context}"
            );
            assert_eq!(json!(warhead.min_debris), read["minimum"], "{context}");
            assert_eq!(json!(warhead.max_debris), read["maximum"], "{context}");
            let tire = rules.voxel_anim_type(rules.voxel_anim_type_id_by_name("TIRE").unwrap());
            let expected = &corpus["initial"]["tire"];
            assert_eq!(json!(tire.duration), expected["duration"]);
            for (field, bits) in [
                ("elasticity", tire.elasticity.bits()),
                ("min_angular", tire.min_angular_velocity.bits()),
                ("max_angular", tire.max_angular_velocity.bits()),
                ("min_z", tire.min_z_vel.bits()),
                ("max_z", tire.max_z_vel.bits()),
                ("max_xy", tire.max_xy_vel.bits()),
            ] {
                assert_eq!(
                    format!("{bits:016x}"),
                    expected["physics_bits"][field].as_str().unwrap(),
                    "{context}/{field}"
                );
            }
        } else {
            assert!(warhead.debris_types.is_empty());
            assert_eq!(
                json!(warhead.debris_types.len()),
                corpus["initial"]["warhead"]["typed_count"]
            );
        }
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
        let (result, draws) = crate::sim::rng::trace_draws(|| {
            commit_projectiles(
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
            )
        });
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
        assert!(
            result.effects.voxel_debris.is_empty(),
            "{context}: inline voxel constructors"
        );
        let native_words: Vec<_> = row["rng_requests"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|request| request["raw_draws"].as_array().unwrap().iter().cloned())
            .collect();
        let rust_words: Vec<_> = draws.iter().map(|draw| draw["value"].clone()).collect();
        assert_eq!(
            rust_words, native_words,
            "{context}: every raw word in constructor/selector/successor order"
        );
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
        if typed {
            let actual: Vec<_> = world.voxel_anims().iter().collect();
            let expected = row["voxels"].as_array().unwrap();
            assert_eq!(actual.len(), expected.len(), "{context}: typed births");
            for ((stable_id, piece), native) in actual.iter().zip(expected) {
                assert_eq!(
                    rules.voxel_anim_type(piece.type_id).name,
                    native["type"].as_str().unwrap()
                );
                assert_eq!(
                    json!(piece.native_unique_id()),
                    native["native_id"],
                    "{context}: constructor identity order"
                );
                assert_eq!(
                    json!(piece.owner_house.is_none()),
                    native["owner_is_null"],
                    "{context}"
                );
                assert_eq!(json!(piece.duration), native["duration"], "{context}");
                assert_eq!(
                    json!(piece.in_logic_vector),
                    native["logic_registered"],
                    "{context}"
                );
                assert_eq!(
                    json!(piece.marked_for_deletion),
                    native["marked_for_deletion"]
                );
                assert_eq!(
                    json!(world.voxel_anims().get(**stable_id).is_some()),
                    native["registry_registered"]
                );
                let layers: Vec<_> = world
                    .display_layers()
                    .layer_of(**stable_id)
                    .map(|layer| layer.index())
                    .into_iter()
                    .collect();
                assert_eq!(
                    json!(layers),
                    native["display_layers"],
                    "{context}: native Display admission"
                );
                let body = &piece.bounce;
                assert_eq!(
                    native_bounce_fields(body),
                    native["bounce"],
                    "{context}: complete native Bounce body"
                );
                // Native ObjectXYZ remains the original Unlimbo coordinate
                // until first AI, while Rust reads the +10 Bounce position.
                // The known first-frame ~1px presentation residual is not a
                // claim of ObjectXYZ equality; physics state is compared above.
            }
        }
    }
}

#[test]
fn original_smashing_tail_preserves_constructor_rng_ids_and_live_coordinates() {
    compare_native_smashing_tail(false, false);
}

#[test]
fn retired_bullet_retains_storage_but_cannot_run_detonation_tail_or_cluster_rng() {
    compare_native_smashing_tail(true, false);
}

#[test]
fn original_authored_typed_smashing_tail_preserves_rng_ids_and_complete_bounce_state() {
    compare_native_smashing_tail(false, true);
}

/// Original full Unit737C90 -> Techno701900 fatal receiver, with an explicit
/// MTNK typed TIRE list. This component boundary supplies the recorded direct
/// damage call and admitted source prior; it is not a native weapon launch.
#[test]
fn original_fatal_mtnk_tires_precede_the_concrete_death_anim_identity() {
    use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
    use crate::sim::native_identity::NativeUniqueIdCursor;
    use crate::sim::rng::SimRng;
    use serde_json::{Value, json};

    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_death_anims_typed_debris.json",
    ))
    .unwrap();
    for row in corpus["rows"].as_array().unwrap() {
        let input = &row["input"];
        let prior = &row["source_before"];
        let context = input["name"].as_str().unwrap();
        let type_name = prior["type"].as_str().unwrap();
        let names: Vec<_> = input["debris_types"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap())
            .collect();
        let maximums: Vec<_> = input["debris_maximums"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_i64().unwrap().to_string())
            .collect();
        let patch = crate::rules::ini_parser::IniFile::from_str(&format!(
            "[{type_name}]\nDebrisTypes={}\nDebrisMaximums={}\nMinDebris={}\nMaxDebris={}\n",
            names.join(","),
            maximums.join(","),
            input["minimum"],
            input["maximum"],
        ));
        let Some(rules) = bound_retail_rules(Some(&patch)) else {
            return;
        };
        let object = rules.object(type_name).unwrap();
        assert_eq!(json!(object.debris_types), input["debris_types"]);
        assert_eq!(json!(object.debris_maximums), input["debris_maximums"]);
        assert_eq!(json!(object.min_debris), input["minimum"]);
        assert_eq!(json!(object.max_debris), input["maximum"]);
        assert_eq!(json!(object.strength), prior["health"]);
        for (key, names) in [
            ("voice_die", &object.voice_die),
            ("die_sound", &object.die_sounds),
        ] {
            let vector = &prior["sound_vectors"][key];
            let native_names: Vec<_> = vector["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| entry["name"].as_str().unwrap())
                .collect();
            assert_eq!(
                json!(names),
                json!(native_names),
                "{context}: bound {key} names"
            );
            assert_eq!(
                json!(names.len()),
                vector["count"],
                "{context}: bound {key} count"
            );
        }

        let tire_id = rules.voxel_anim_type_id_by_name("TIRE").unwrap();
        let tire = rules.voxel_anim_type(tire_id);
        assert_eq!(tire.image, "TIRE");
        let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
        assert_eq!(
            crate::util::sha256::sha256_hex(assets.get_ref("SOUNDMD.INI").unwrap()),
            corpus["sound_sha256"].as_str().unwrap(),
            "{context}: physical sound registry input"
        );
        let model = crate::assets::vxl_file::VxlFile::from_bytes(
            assets
                .get_ref("TIRE.VXL")
                .expect("physical retail TIRE model"),
        )
        .unwrap();
        assert!(model.limbs.iter().any(|limb| !limb.voxels.is_empty()));
        for native in row["anims"].as_array().unwrap() {
            let name = native["type"].as_str().unwrap();
            assert!(
                object
                    .explosion_anims
                    .iter()
                    .any(|candidate| candidate == name)
            );
            assert!(
                rules
                    .art()
                    .anim_runtime_config(name)
                    .unwrap()
                    .raw_shp_frame_count
                    .is_some_and(|frames| frames > 0),
                "{context}: concrete death Anim binds physical retail SHP"
            );
        }
        let mut world = crate::sim::arena_fixture::rival_arena(&rules, 1);
        world.intern_rule_type_ids(&rules);
        world.resolve_type_handles(&rules);
        world.resolve_rule_animation_lists(&rules);
        let coord = |axis: usize| prior["location"][axis].as_i64().unwrap() as i32;
        assert_eq!(coord(0).rem_euclid(256), 128);
        assert_eq!(coord(1).rem_euclid(256), 128);
        // Supply this already-admitted source's constructor identity through
        // the allocator, then use ordinary spawn/Unlimbo for its live state.
        world.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(
            (prior["native_id"].as_i64().unwrap() as u32).wrapping_sub(1),
        ));
        let source = world
            .spawn_object(
                type_name,
                "Americans",
                coord(0).div_euclid(256) as u16,
                coord(1).div_euclid(256) as u16,
                0,
                &rules,
            )
            .unwrap();
        let entity = world.substrate.entities.get(source).unwrap();
        let owner = entity.owner();
        assert_eq!(json!(entity.native_unique_id()), prior["native_id"]);
        assert_eq!(json!(entity.health.current()), prior["health"]);
        let actual_location = crate::sim::movement::ground_pose::object_get_coords(
            entity,
            world.resolved_terrain.as_ref(),
        );
        assert_eq!(
            json!([actual_location.x, actual_location.y, actual_location.z]),
            prior["location"]
        );
        assert_eq!(
            json!(entity.lifecycle.object_alive()),
            json!(prior["alive"] == 1)
        );
        assert_eq!(
            json!(entity.lifecycle.in_limbo()),
            json!(prior["limbo"] == 1)
        );
        assert_eq!(
            json!(entity.lifecycle.cell_marked()),
            json!(prior["marked"] == 1)
        );
        assert_eq!(
            json!(entity.in_logic_vector.is_member()),
            prior["logic_registered"]
        );
        assert!(world.voxel_anims().iter().next().is_none() && world.anims().next().is_none());
        world.session.binary_frame = input["binary_frame"].as_u64().unwrap() as u32;
        world.session.game_mode_nonzero = prior["session_mode"].as_u64().unwrap() != 0;
        assert_eq!(json!(world.owner_is_human(owner)), prior["owner_human"]);

        // Import all streams AFTER ordinary fixture startup. No seeded-Rust
        // output or guessed constructor state supplies a native comparison.
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
        assert_eq!(input["attacker_is_null"], true);
        assert_eq!(input["source_house_is_null"], true);
        let warhead = world.interner.intern(input["warhead"].as_str().unwrap());
        let (_, draws) = crate::sim::rng::trace_draws(|| {
            world.commit_direct_damage_receiver(
                &rules,
                None,
                EntityDamageEvent::direct_receiver(
                    source,
                    input["damage"].as_i64().unwrap() as i32,
                    input["distance"].as_i64().unwrap() as i32,
                    RAD_NO_ATTACKER,
                    None,
                    warhead,
                    ReceiverCallFlags {
                        ignore_defenses: input["ignore_defenses"].as_bool().unwrap(),
                        arg6: input["prevent_passenger_escape"].as_bool().unwrap(),
                    },
                ),
                FrameEffects::default(),
            )
        });
        let native_words: Vec<_> = row["rng_requests"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|request| request["raw_draws"].as_array().unwrap().iter().cloned())
            .collect();
        let rust_words: Vec<_> = draws.iter().map(|draw| draw["value"].clone()).collect();
        assert_eq!(rust_words, native_words, "{context}: all ordered raw words");
        for (actual, key) in [
            (world.scenario_rng.native_state_hex(), "scenario"),
            (world.main_rng.native_state_hex(), "main"),
            (world.mapgen_rng.native_state_hex(), "mapgen"),
        ] {
            assert_eq!(
                actual,
                row["rng_after"][key].as_str().unwrap(),
                "{context}/{key}"
            );
        }
        assert_eq!(
            json!(world.native_unique_ids.as_ref().unwrap().current_raw()),
            row["native_cursor_after"],
            "{context}: no deferred second allocation"
        );
        let source_after = world
            .substrate
            .entities
            .get(source)
            .expect("dead source retained until pending-delete drain");
        assert_eq!(
            json!(source_after.health.current()),
            row["source_after"]["health"]
        );
        assert_eq!(
            source_after.lifecycle.object_alive(),
            row["source_after"]["alive"] == 1
        );
        assert_eq!(
            source_after.lifecycle.in_limbo(),
            row["source_after"]["limbo"] == 1
        );
        let pieces: Vec<_> = world.voxel_anims().iter().collect();
        assert_eq!(pieces.len(), row["voxels"].as_array().unwrap().len());
        let mut ordered_births = Vec::new();
        for ((id, piece), native) in pieces.iter().zip(row["voxels"].as_array().unwrap()) {
            assert_eq!(piece.type_id, tire_id);
            assert_eq!(json!(piece.native_unique_id()), native["native_id"]);
            assert_eq!(
                piece.owner_house,
                Some(owner),
                "{context}: Techno passes source owner"
            );
            assert_eq!(native["owner_is_null"], false);
            assert_eq!(
                native["owner_native_id"], prior["owner_native_id"],
                "{context}: logical owner maps to the supplied native source house"
            );
            assert_eq!(json!(piece.duration), native["duration"]);
            assert_eq!(json!(piece.in_logic_vector), native["logic_registered"]);
            assert_eq!(
                json!(piece.marked_for_deletion),
                native["marked_for_deletion"]
            );
            assert_eq!(
                json!(world.voxel_anims().get(**id).is_some()),
                native["registry_registered"]
            );
            let layers: Vec<_> = world
                .display_layers()
                .layer_of(**id)
                .map(|layer| layer.index())
                .into_iter()
                .collect();
            assert_eq!(json!(layers), native["display_layers"]);
            assert_eq!(
                native_bounce_fields(&piece.bounce),
                native["bounce"],
                "{context}: complete constructor body"
            );
            ordered_births.push((**id, piece.native_unique_id()));
        }
        let anims: Vec<_> = world.anims().collect();
        assert_eq!(anims.len(), row["anims"].as_array().unwrap().len());
        for ((id, anim), native) in anims.iter().zip(row["anims"].as_array().unwrap()) {
            assert_eq!(
                world.interner.resolve(anim.type_id),
                native["type"].as_str().unwrap()
            );
            assert_eq!(json!(anim.native_unique_id), native["native_id"]);
            assert_eq!(
                json!([anim.world_coord.x, anim.world_coord.y, anim.world_coord.z]),
                native["location"]
            );
            assert_eq!(json!(anim.draw_flags), native["draw_flags"]);
            assert_eq!(json!(anim.z_adjust), native["z_adjust"]);
            assert_eq!(anim.bounce.is_some(), native["is_bouncing"] == 1);
            assert_eq!(json!(anim.in_logic_vector), native["logic_registered"]);
            let layers: Vec<_> = world
                .display_layers()
                .layer_of(**id)
                .map(|layer| layer.index())
                .into_iter()
                .collect();
            assert_eq!(json!(layers), native["display_layers"]);
            ordered_births.push((**id, anim.native_unique_id));
        }
        // Inspect the shared live Logic order, not two independently sorted
        // stores: both typed pieces precede the actual concrete Unit death Anim.
        let actual_order: Vec<_> = world
            .substrate
            .logic
            .as_slice()
            .iter()
            .filter_map(|id| {
                ordered_births
                    .iter()
                    .find(|(birth, _)| birth == id)
                    .map(|(_, native)| *native)
            })
            .collect();
        let native_order: Vec<_> = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| {
                matches!(
                    event["event"].as_str(),
                    Some("VoxelCtor_return" | "AnimCtor_return")
                )
            })
            .map(|event| event["native_id"].as_i64().unwrap() as i32)
            .collect();
        assert_eq!(
            actual_order, native_order,
            "{context}: constructor/admission order"
        );
        world.debug_assert_logic_membership_consistent();
    }
}
