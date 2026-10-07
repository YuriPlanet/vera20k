//! Original full IFV impact reduced to the production receiver boundary.
//! The native companion executes launch/flight too; this test supplies its
//! final live Bullet and damage coordinates and compares the synchronous
//! bridge -> effect-construction tail. It does not compare map loading, the
//! supplied native zone hierarchy, or later Anim AI.

use super::*;
use crate::rules::art_data::ArtRegistry;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::retail_ini_fixture::retail_rules_and_art;
use crate::sim::combat::world_receiver::{ReceiverRun, commit_projectiles};
use crate::sim::native_identity::NativeUniqueIdCursor;
use crate::sim::projectile::{
    ProjectileCollisionPolicy, ProjectileCoord, ProjectileDetonation, ProjectileDetonationReason,
    ProjectilePayload, ProjectileSpawn, ProjectileTarget, ProjectileTrajectory, ProjectileVelocity,
    ProjectileVisualState, TargetExpiryPolicy,
};

fn translated_stock() -> Value {
    let mut input: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_rim_stock_inputs.json",
    ))
    .unwrap();
    // Exactly the supplied crop transformation in ifv_bridge_impact.prepare;
    // native map dimensions and tile/rim inputs remain unchanged.
    for row in input["cells"].as_array_mut().unwrap() {
        row[0] = json!(row[0].as_i64().unwrap() - 102);
        row[1] = json!(row[1].as_i64().unwrap() - 120);
        if let Some(anchor) = row[7].as_array_mut() {
            anchor[0] = json!(anchor[0].as_i64().unwrap() - 102);
            anchor[1] = json!(anchor[1].as_i64().unwrap() - 120);
        }
    }
    input
}

fn coord(value: &Value) -> ProjectileCoord {
    ProjectileCoord::new(
        value[0].as_i64().unwrap() as i32,
        value[1].as_i64().unwrap() as i32,
        value[2].as_i64().unwrap() as i32,
    )
}

fn retail_rules(native: &Value) -> Option<(RuleSet, OverlayTypeRegistry)> {
    let (ini, art) = retail_rules_and_art()?;
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    let overlays = OverlayTypeRegistry::from_ini(&ini, Some(&art));
    let initial = &native["rows"][0]["initial"];
    let fv = rules.object("FV").unwrap();
    assert_eq!(fv.weapon_list[0].as_deref(), Some("HoverMissile"));
    let weapon = rules.weapon(fv.weapon_list[0].as_deref().unwrap()).unwrap();
    assert_eq!(weapon.projectile.as_deref(), Some("AAHeatSeeker2"));
    assert_eq!(weapon.warhead.as_deref(), Some("HE"));
    assert_eq!(weapon.damage, 25);
    assert_eq!(rules.projectile("AAHeatSeeker2").unwrap().cluster, 1);
    assert_eq!(
        json!(rules.bridge_rules.strength),
        initial["bridge_strength"]
    );
    // The native full readers apply RULESMD, MPBattleMD and Hills. These
    // selected HE fields remain unchanged on the omitted later bodies; this
    // regular test uses the physical base INI, not a fabricated merged INI.
    let he = rules.warhead("HE").unwrap();
    for layer in initial["warhead_layers"].as_array().unwrap() {
        assert_eq!(json!(he.wall), layer["wall"]);
        assert_eq!(json!(he.em_effect), layer["em_effect"]);
        assert_eq!(
            format!("{:08x}", (he.cell_spread_f64 as f32).to_bits()),
            layer["cell_spread_bits"]
        );
        assert_eq!(
            format!("{:08x}", (he.percent_at_max_f64 as f32).to_bits()),
            layer["percent_at_max_bits"]
        );
    }
    for (index, name) in initial["overlay_types"]["dense_prefix"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        assert_eq!(overlays.name(index as u8), name.as_str());
    }
    for layer in initial["overlay_types"]["read_layers"].as_array().unwrap() {
        let flags = overlays
            .flags(layer["index"].as_u64().unwrap() as u8)
            .unwrap();
        assert_eq!(json!(flags.wall), layer["wall"]);
        assert_eq!(json!(flags.land.as_index()), layer["land"]);
    }
    // Independent original ART/image reads establish actual frame counts.
    // Shared bridge_anim_inputs also establishes the Bouncer constructor
    // scalars; no frame count or Bounce value comes from a Rust golden.
    let bridge: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/bridge_anim_inputs.json",
    ))
    .unwrap();
    for row in bridge["rows"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        let frames = row["raw_shp_frame_count"].as_i64().unwrap();
        if frames > 0 {
            rules.bind_anim_frame_count_for_test(name, frames as i32);
        }
        let config = rules.art().anim_runtime_config(name).unwrap();
        assert_eq!(json!(config.bouncer), row["bouncer"], "{name}");
        assert_eq!(json!(config.rate_logic_frames), row["rate"], "{name}");
        assert_eq!(
            json!(config.elasticity.bits()),
            row["elasticity_f64_bits"],
            "{name}"
        );
        assert_eq!(
            json!(config.min_z_vel.bits()),
            row["min_z_vel_f64_bits"],
            "{name}"
        );
        assert_eq!(
            json!(config.max_xy_vel.bits()),
            row["max_xy_vel_f64_bits"],
            "{name}"
        );
    }
    for row in initial["impact_anim_art"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        let frames = row["frame_count"].as_i64().unwrap() as i32;
        rules.bind_anim_frame_count_for_test(name, frames);
        let config = rules.art().anim_runtime_config(name).unwrap();
        assert_eq!(json!(config.end), row["end"], "{name}");
        assert_eq!(json!(config.rate_logic_frames), row["rate"], "{name}");
        assert_eq!(json!(config.scorch), row["scorch"], "{name}");
        assert_eq!(json!(config.crater), row["crater"], "{name}");
    }
    Some((rules, overlays))
}

fn assert_cells(world: &Simulation, rows: &Value, label: &str) {
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 225);
    for row in rows {
        let xy = serde_json::from_value(row["coord"].clone()).unwrap();
        assert_eq!(snapshot(world, xy), *row, "{label} at {xy:?}");
    }
}

#[test]
fn native_ifv_bridge_impact_orders_live_selection_debris_ids_and_rng() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/ifv_bridge_impact.json",
    ))
    .unwrap();
    let Some((rules, overlays)) = retail_rules(&native) else {
        return;
    };
    let stock = translated_stock();
    for row in native["rows"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        let mut world = stock_world(&rules, &stock);
        world.resolve_rule_animation_lists(&rules);
        world.bridge_state = Some(BridgeRuntimeState::from_resolved_terrain_with_map_size(
            world.resolved_terrain.as_ref().unwrap(),
            true,
            rules.bridge_rules.strength,
            serde_json::from_value(stock["size"].clone()).unwrap(),
        ));
        if name == "collapsed" {
            // Same original HighBridgeBody576BA0 priming boundary as native:
            // 9 -> 15, without a preceding AreaDamage admission or effect.
            let result = super::super::super::run_state_machine(
                &mut world,
                &rules,
                Some(&overlays),
                (10, 20),
                Family::High,
            );
            let body: Value = serde_json::from_str(crate::test_fixture::text(
                "tools/spatial_oracle/bridge_rim_body.json",
            ))
            .unwrap();
            // The first original body call changes 9 -> 15 but returns zero;
            // its boolean reports the later collapse, not any state change.
            assert_eq!(
                json!(u8::from(result.returned)),
                body["cases"][0]["hits"][0]["returned"]
            );
            assert!(!result.collapsed);
        }
        assert_cells(&world, &row["initial"]["bridge_before"], name);
        let seed = row["supplied"]["scenario_rng_seed"].as_u64().unwrap();
        world.scenario_rng = SimRng::new(seed);
        world.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(
            row["launch"]["unique_id"].as_u64().unwrap() as u32,
        ));
        world.session.binary_frame = row["frames"].as_array().unwrap().last().unwrap()["frame"]
            .as_u64()
            .unwrap() as u32;
        let selector = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["event"] == "SelectAnim")
            .unwrap();
        let damage = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["event"] == "AreaDamage")
            .unwrap();
        let live = coord(&selector["retained_bullet"]);
        let impact = coord(&damage["position"]);
        assert_eq!(live, ProjectileCoord::new(2688, 5248, 104));
        assert_eq!(impact, ProjectileCoord::new(2688, 5248, 520));
        let payload = ProjectilePayload::new(
            rules.weapon("HoverMissile").unwrap().damage,
            world.interner.intern("HE"),
            world.interner.intern("HoverMissile"),
        );
        let target = ProjectileTarget::Cell { rx: 10, ry: 20 };
        let stable_id = world.allocate_stable_id();
        // Only retained location/OnBridge and payload are read by this handoff.
        // Motion fields below are inert: this test never calls Bullet AI.
        world.admit_projectile(
            stable_id,
            ProjectileSpawn {
                native_unique_id: row["launch"]["unique_id"].as_i64().unwrap() as i32,
                line_trail: None,
                flat: false,
                source_id: 0,
                origin: live,
                target,
                initial_target_position: impact,
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
        let receipt = commit_projectiles(
            &mut world,
            &mut ReceiverRun::default(),
            &[ProjectileDetonation {
                projectile_id: stable_id,
                source_id: 0,
                target,
                impact,
                payload,
                reason: ProjectileDetonationReason::ReachedTarget,
            }],
            &rules,
            Some(&overlays),
        );
        assert_eq!(
            receipt.effects.bridge_state_changed,
            name == "collapsed",
            "{name}"
        );
        assert!(
            receipt.effects.explosion_effects.is_empty(),
            "{name}: effects admitted inline"
        );
        assert_cells(&world, &row["bridge_after"], name);
        let actual = world
            .substrate
            .anims
            .iter()
            .map(|(_, a)| a)
            .collect::<Vec<_>>();
        let expected = row["retained_after"]["retained_animations"]
            .as_array()
            .unwrap();
        assert_eq!(actual.len(), expected.len(), "{name}");
        for (anim, expected) in actual.into_iter().zip(expected) {
            assert_eq!(
                json!(anim.native_unique_id),
                expected["native_id"],
                "{name}"
            );
            assert_eq!(
                world.interner.resolve(anim.type_id),
                expected["type"].as_str().unwrap(),
                "{name}"
            );
            assert_eq!(
                json!([anim.world_coord.x, anim.world_coord.y, anim.world_coord.z]),
                expected["location"],
                "{name}"
            );
            assert_eq!(
                anim.bounce.is_some(),
                expected["is_bouncing"] == 1,
                "{name}"
            );
            if let Some(body) = anim.bounce {
                for (actual, key) in [
                    (body.position, "position_bits"),
                    (body.velocity, "velocity_bits"),
                ] {
                    assert_eq!(
                        json!(actual.map(|value| value.bits())),
                        expected["bounce"][key],
                        "{name}/{key}"
                    );
                }
                for (actual, key) in [
                    (body.elasticity.bits(), "elasticity_bits"),
                    (body.gravity.bits(), "gravity_bits"),
                    (body.angular_velocity_magnitude.bits(), "clamp_bits"),
                ] {
                    assert_eq!(json!(actual), expected["bounce"][key], "{name}/{key}");
                }
            }
        }
        assert_eq!(
            json!(world.native_unique_ids.as_ref().unwrap().current_raw()),
            row["final_scenario_native_id"],
            "{name}"
        );
        let hex = world.scenario_rng.native_state_hex();
        let bytes: Vec<u8> = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|digits| u8::from_str_radix(std::str::from_utf8(digits).unwrap(), 16).unwrap())
            .collect();
        assert_eq!(
            crate::util::sha256::sha256_hex(&bytes),
            row["rng_state_sha256"].as_str().unwrap(),
            "{name} whole Scenario RNG"
        );
        let mut original_draws = SimRng::new(seed);
        for draw in row["raw_rng_words"].as_array().unwrap() {
            assert_eq!(
                json!(original_draws.next_u32()),
                draw["word"],
                "{name} native raw transcript"
            );
        }
        assert_eq!(
            world.scenario_rng.logical_state(),
            original_draws.logical_state(),
            "{name} draw count/stream"
        );
        assert_eq!(
            world.main_rng.logical_state(),
            main_before,
            "{name} Main unchanged"
        );
        // Quaternion bits are not represented by the SHP Bouncer owner. The
        // selected zero-spin SHP does not consume orientation. Later flight,
        // child AI and physical Bullet drain have separate comparisons.
    }
}
