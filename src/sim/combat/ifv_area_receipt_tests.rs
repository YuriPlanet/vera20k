//! Full original AreaDamage489280 receipt controls. Native substitutes the
//! Unit ReceiveDamage return ABI; Rust executes its real receiver. The
//! comparison is receipt/collection/isolation, never native health or kills.

use super::*;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};
use serde_json::Value;

/// The native companion enters DetonateAtCoord directly. The existing
/// DeathWeapon reason selects that same bare-entry boundary in Rust (without
/// ResolveImpactCoordAndDetonate's later Cluster draws). No death-weapon factory
/// or receiver health mutation parity is claimed by these six comparisons.
#[test]
fn original_area_receipt_selects_nullify_after_em_effect_rng_and_before_return() {
    use crate::sim::native_identity::NativeUniqueIdCursor;
    use crate::sim::projectile::{
        ProjectileCollisionPolicy, ProjectileDetonationReason, ProjectilePayload, ProjectileSpawn,
        ProjectileTrajectory, ProjectileVelocity, ProjectileVisualState, TargetExpiryPolicy,
    };
    let Some((base, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/ifv_isolated_tail.json",
    ))
    .unwrap();
    assert_eq!(native["rows"].as_array().unwrap().len(), 6);
    for row in native["rows"].as_array().unwrap() {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let mut layers = RulesLayerStack::new(base.clone());
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(&format!(
                "[HE]\nEMEffect={}\n",
                if input["em_effect"] == true {
                    "yes"
                } else {
                    "no"
                }
            )),
        );
        let processed = layers.process_with_fixed_art(&art).unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        rules.install_art_data(ArtRegistry::from_ini(&art));
        for item in native["initial"]["impact_anim_art"].as_array().unwrap() {
            rules.bind_anim_frame_count_for_test(
                item["name"].as_str().unwrap(),
                item["frame_count"].as_i64().unwrap() as i32,
            );
        }
        rules.bind_anim_frame_count_for_test(
            "IRONFX",
            native["nullify_art"]["raw_shp_frame_count"]
                .as_i64()
                .unwrap() as i32,
        );
        assert_eq!(rules.general.weapon_nullify_anim, "IRONFX");
        let mut world = Simulation::with_seed(31);
        world.session.binary_frame = input["binary_frame"].as_u64().unwrap() as u32;
        crate::sim::arena_fixture::flat_arena(&mut world, &rules);
        for cell in &mut world.resolved_terrain.as_mut().unwrap().cells {
            cell.level = 6;
        }
        if input["receiver_mode"] != "empty" {
            let id = world
                .spawn_object("FV", "Americans", 10, 20, 0, &rules)
                .unwrap();
            let target = world.substrate.entities.get_mut(id).unwrap();
            target.position.sub_x = SimFixed::from_num(128);
            target.position.sub_y = SimFixed::from_num(128);
            target.position.exact_z_leptons = Some(624);
            target.health.current = input["receiver_health"].as_i64().unwrap() as i32;
            target.invulnerability = Some(InvulnerabilityState::new(
                crate::sim::timer::CdTimer::started(
                    input["ic_start"].as_u64().unwrap() as i32,
                    input["ic_duration"].as_u64().unwrap() as i32,
                ),
                InvulnKind::IronCurtain,
            ));
        }
        world.intern_rule_type_ids(&rules);
        world.resolve_type_handles(&rules);
        world.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(
            row["native_cursor_before"].as_u64().unwrap() as u32,
        ));
        world.scenario_rng = SimRng::new(input["scenario_rng_seed"].as_u64().unwrap());
        let coordinate = ProjectileCoord::new(2688, 5248, 624);
        assert_eq!(
            serde_json::json!([coordinate.x, coordinate.y, coordinate.z]),
            input["live"]
        );
        assert_eq!(input["live"], input["placement"]);
        let payload = ProjectilePayload::new(
            rules.weapon("HoverMissile").unwrap().damage,
            world.interner.intern("HE"),
            world.interner.intern("HoverMissile"),
        );
        let target = ProjectileTarget::Cell { rx: 10, ry: 20 };
        let id = world.allocate_stable_id();
        world.admit_projectile(
            id,
            ProjectileSpawn {
                native_unique_id: row["bullet_native_id"].as_i64().unwrap() as i32,
                line_trail: None,
                flat: false,
                source_id: RAD_NO_ATTACKER,
                origin: coordinate,
                target,
                initial_target_position: coordinate,
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
        let main_before = world.main_rng.logical_state();
        let output = commit_projectiles(
            &mut world,
            &mut ReceiverRun::default(),
            &[ProjectileDetonation {
                projectile_id: id,
                source_id: RAD_NO_ATTACKER,
                target,
                impact: coordinate,
                payload,
                reason: ProjectileDetonationReason::DeathWeapon,
            }],
            &rules,
            None,
        );
        assert!(
            output.effects.explosion_effects.is_empty(),
            "{name}: constructed inline"
        );
        let anims = world
            .substrate
            .anims
            .iter()
            .map(|(_, a)| a)
            .collect::<Vec<_>>();
        assert_eq!(
            anims.len(),
            row["anim_count"].as_u64().unwrap() as usize,
            "{name}"
        );
        let anim = anims[0];
        assert_eq!(
            world.interner.resolve(anim.type_id),
            row["constructed"].as_str().unwrap(),
            "{name}"
        );
        assert_eq!(
            serde_json::json!([anim.world_coord.x, anim.world_coord.y, anim.world_coord.z]),
            row["constructor"]["position"],
            "{name}"
        );
        assert_eq!(
            serde_json::json!(anim.draw_flags),
            row["constructor"]["flags"],
            "{name}"
        );
        assert_eq!(
            serde_json::json!(anim.z_adjust),
            row["constructor"]["z_adjust"],
            "{name}"
        );
        assert_eq!(
            serde_json::json!(anim.native_unique_id),
            row["native_cursor_after"],
            "{name}"
        );
        assert_eq!(
            serde_json::json!(world.native_unique_ids.as_ref().unwrap().current_raw()),
            row["native_cursor_after"],
            "{name}"
        );
        let hex = world.scenario_rng.native_state_hex();
        let bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|digits| u8::from_str_radix(std::str::from_utf8(digits).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            crate::util::sha256::sha256_hex(&bytes),
            row["rng_after_sha256"].as_str().unwrap(),
            "{name}"
        );
        assert_eq!(
            row["constructor"]["rng_hash"], row["rng_after_sha256"],
            "{name}: constructor adds no draws"
        );
        assert_eq!(world.main_rng.logical_state(), main_before, "{name}");
        assert!(
            world.projectiles.get(id).is_some(),
            "{name}: bare entry does not retire the Bullet"
        );
    }
}

#[test]
fn native_area_receipt_tracks_dispatch_and_strict_iron_curtain_boundary() {
    let Some((base, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/ifv_area_receipt.json",
    ))
    .unwrap();
    assert_eq!(native["rows"].as_array().unwrap().len(), 22);
    for row in native["rows"].as_array().unwrap() {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let mut layers = RulesLayerStack::new(base.clone());
        // Same independent original HE reread as the native control, through
        // the existing production layer owner and exact numeric parser.
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(&format!(
                "[HE]\nCellSpread={}\n",
                input["spread"].as_str().unwrap()
            )),
        );
        let processed = layers.process_with_fixed_art(&art).unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        rules.install_art_data(ArtRegistry::from_ini(&art));
        let mut world = Simulation::with_seed(31);
        world.session.binary_frame = 100;
        world.session.no_damage = input["scenario_flags"].as_u64().unwrap() & 0x20 != 0;
        let owner = world.interner.intern("Americans");
        world.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
        );
        world.session.house_order.push(owner);
        crate::sim::arena_fixture::flat_arena(&mut world, &rules);
        for cell in &mut world.resolved_terrain.as_mut().unwrap().cells {
            cell.level = 6;
        }
        let target = if input["empty"] == true {
            None
        } else {
            let id = world
                .spawn_object("FV", "Americans", 10, 20, 0, &rules)
                .unwrap();
            let entity = world.substrate.entities.get_mut(id).unwrap();
            entity.position.sub_x =
                SimFixed::from_num(128 + input["distance"].as_i64().unwrap() as i32);
            entity.position.sub_y = SimFixed::from_num(128);
            entity.position.exact_z_leptons = Some(624);
            entity.health.current = input["health"].as_i64().unwrap() as i32;
            entity.lifecycle.object_alive = input["alive"].as_bool().unwrap();
            entity.lifecycle.cell_marked = input["marked"].as_bool().unwrap();
            entity.lifecycle.in_limbo = input["limbo"].as_bool().unwrap();
            entity.invulnerability = (input["ic"] == true).then(|| {
                InvulnerabilityState::new(
                    crate::sim::timer::CdTimer::started(
                        input["start"].as_u64().unwrap() as i32,
                        input["duration"].as_u64().unwrap() as i32,
                    ),
                    if input["ic_kind"] == 0 {
                        InvulnKind::IronCurtain
                    } else {
                        InvulnKind::ForceShield
                    },
                )
            });
            Some(id)
        };
        world.intern_rule_type_ids(&rules);
        world.resolve_type_handles(&rules);
        let he = world.interner.intern("HE");
        if input["null_wh"] == true {
            // A null C++ pointer cannot enter collect_area(&WarheadType).
            // Its production analogue is the existing missing-rule caller
            // exit. Exercise that caller, without inventing a receipt value
            // or a placeholder WarheadType solely to enter the typed API.
            let missing = world.interner.intern("MISSING_AREA_RECEIPT_WARHEAD");
            let weapon = world.interner.intern("HoverMissile");
            let health = world
                .substrate
                .entities
                .get(target.unwrap())
                .unwrap()
                .health
                .current;
            let rng = world.scenario_rng.logical_state();
            let output = commit_projectiles(
                &mut world,
                &mut ReceiverRun::default(),
                &[ProjectileDetonation {
                    projectile_id: 999,
                    source_id: RAD_NO_ATTACKER,
                    target: ProjectileTarget::Cell { rx: 10, ry: 20 },
                    impact: ProjectileCoord::new(2688, 5248, 624),
                    payload: crate::sim::projectile::ProjectilePayload::new(25, missing, weapon),
                    reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
                }],
                &rules,
                None,
            );
            assert!(output.effects.explosion_effects.is_empty());
            assert_eq!(world.substrate.anims.len(), 0);
            assert_eq!(
                world
                    .substrate
                    .entities
                    .get(target.unwrap())
                    .unwrap()
                    .health
                    .current,
                health
            );
            assert_eq!(world.scenario_rng.logical_state(), rng);
            assert_eq!(row["dispatch"].as_array().unwrap().len(), 0);
            assert_eq!(row["result"], 1);
            continue;
        }
        let before = world.scenario_rng.logical_state();
        let collected = collect_area(
            &mut world,
            &rules,
            None,
            (10, 20),
            input["damage"].as_i64().unwrap() as i32,
            rules.warhead("HE").unwrap(),
            (RAD_NO_ATTACKER, None, he),
            Some(combat_aoe::AoEAirImpact {
                sub_x: SimFixed::from_num(128),
                sub_y: SimFixed::from_num(128),
                z_leptons: 624,
            }),
            6,
        );
        assert_eq!(
            world.scenario_rng.logical_state(),
            before,
            "{name} collection RNG"
        );
        // Receiver bodies are different explicit proof boundaries. Do not
        // require Rust to return the oracle's supplied0/1/2/4 or to preserve
        // its unmodified Health. All four native outputs establish the same
        // enclosing AreaDamage result regardless of that callee return.
        if !row["dispatch"].as_array().unwrap().is_empty() {
            assert_eq!(collected.receivers.len(), 1, "{name}");
            let combat_aoe::AreaDamageReceiver::Entity(record) = collected.receivers[0] else {
                panic!("{name}: expected supplied Unit record");
            };
            assert_eq!(Some(record.target_id), target, "{name}");
            assert_eq!(
                record.distance_leptons,
                Some(input["distance"].as_i64().unwrap() as i32),
                "{name}"
            );
        }
        let (_, _, result) = commit_area_with_dispatch(
            &mut world,
            &mut ReceiverRun::default(),
            &collected.receivers,
            &rules,
            None,
        );
        assert_eq!(
            i64::from(result as u8),
            row["result"].as_i64().unwrap(),
            "{name}"
        );
    }
}
