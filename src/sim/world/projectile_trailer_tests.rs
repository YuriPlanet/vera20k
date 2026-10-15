//! Original Bullet4666E0 trailer constructor and independent Anim lifetime.
//! The native witness supplies an in-flight Bullet boundary; these fixtures do
//! likewise. The release map profiles separately exercise actual SUB FireAt.

use super::{Simulation, techno_ai::ObjectAiCtx};
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::anim_class::AnimObject;
use crate::sim::native_identity::NativeUniqueIdCursor;
use crate::sim::projectile::{
    ProjectileCollisionPolicy, ProjectileCoord, ProjectilePayload, ProjectileSpawn,
    ProjectileTarget, ProjectileTrajectory, ProjectileVelocity, ProjectileVisualState,
    TargetExpiryPolicy,
};
use crate::sim::rng::SimRng;
use serde_json::{Value, json};

fn native() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/projectile_trailer.json",
    ))
    .unwrap()
}

fn case<'a>(corpus: &'a Value, name: &str) -> &'a Value {
    corpus["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == name)
        .unwrap()
}

fn coord(value: &Value) -> ProjectileCoord {
    ProjectileCoord::new(
        value[0].as_i64().unwrap() as i32,
        value[1].as_i64().unwrap() as i32,
        value[2].as_i64().unwrap() as i32,
    )
}

fn bound_retail_rules(random_rate: Option<&str>) -> Option<RuleSet> {
    let fixture = crate::rules::retail_ini_fixture::retail_battle_rules()?;
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    let corpus = native();
    for asset in corpus["retail"]["anim_type"]["asset_loads"]
        .as_array()
        .unwrap()
    {
        let name = asset["name"].as_str().unwrap();
        let loaded = assets
            .load_file_from_mix(name)
            .expect("physical retail trail SHP");
        assert_eq!(loaded.bytes.len() as u64, asset["bytes"].as_u64().unwrap());
        assert_eq!(
            crate::util::sha256::sha256_hex(&loaded.bytes),
            asset["sha256"].as_str().unwrap()
        );
    }
    let mut rules = fixture.rules;
    if let Some(random_rate) = random_rate {
        // The control changes only the physical ART input sent through the
        // original reader in the corpus. It does not assign a runtime rate.
        let mut art = fixture.fixed_art;
        art.merge(&IniFile::from_str(&format!(
            "[BBBLELRG]\nRandomRate={random_rate}\n"
        )));
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    }
    let roots = crate::rules::effect_asset_catalog::anim_class_roots(&rules);
    assert!(roots.iter().any(|name| name == "BBBLELRG"));
    rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
    let config = rules.art().anim_runtime_config("BBBLELRG").unwrap();
    let original = &corpus["retail"]["anim_type"];
    for (value, key) in [
        (config.start, "start"),
        (config.end, "end"),
        (config.loop_start, "loop_start"),
        (config.loop_end, "loop_end"),
        (config.loop_count, "loop_count"),
        (i32::from(config.rate_logic_frames), "rate"),
    ] {
        assert_eq!(
            i64::from(value),
            original[key].as_i64().unwrap(),
            "BBBLELRG {key}"
        );
    }
    Some(rules)
}

fn rng_state(state: crate::sim::rng::SimRngLogicalState) -> Value {
    let hex = state.native_state_hex();
    let raw: Vec<u8> = hex
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    json!({"cursor": [state.index_a, state.index_b],
        "sha256": crate::util::sha256::sha256_hex(&raw)})
}

fn rngs(sim: &Simulation) -> Value {
    json!({"main": rng_state(sim.main_rng.logical_state()), "scenario": rng_state(sim.scenario_rng.logical_state())})
}

fn anim_state(sim: &Simulation, anim: &AnimObject) -> Value {
    let p = anim.world_coord;
    json!({
        "native_id": anim.native_unique_id,
        "alive": !sim.substrate.pending_delete.contains_object(&anim.stable_id),
        "position": [p.x,p.y,p.z],
        "stage": anim.runtime.current_frame,
        "timer": [anim.runtime.frame_timer.start_frame(), anim.runtime.frame_timer.duration()],
        "rate": anim.runtime.rate_reload,
        "step": anim.runtime.frame_step,
        "owner_object": anim.owner_entity.unwrap_or(0),
        "bullet": anim.attached_bullet().unwrap_or(0),
        "z_adjust": anim.z_adjust,
        "delay": anim.runtime.delay_remaining,
        "draw_flags": anim.draw_flags,
        "loops": anim.runtime.loop_remaining,
        "brand_new": anim.runtime.first_ai_guard,
    })
}

fn counts(sim: &Simulation) -> Value {
    json!({"bullets":sim.projectiles.len(), "anims":sim.substrate.anims.len(),
        "pending":sim.substrate.pending_delete.len()})
}

fn scene(row: &Value, terminal: bool) -> (Simulation, u64) {
    let mut sim = Simulation::with_seed(31);
    sim.main_rng = SimRng::new(31).into();
    sim.scenario_rng = SimRng::new(31);
    sim.session.binary_frame = row["supplied"]["frame"].as_i64().unwrap() as u32;
    sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(
        row["before_native_id"].as_u64().unwrap() as u32,
    ));
    assert_eq!(
        rngs(&sim),
        row["before_rng"],
        "original seeded entry boundary"
    );
    let position = coord(&row["supplied"]["position"]);
    let target = if terminal {
        ProjectileTarget::Entity(999)
    } else {
        ProjectileTarget::Cell { rx: 16, ry: 20 }
    };
    let payload = ProjectilePayload::new(
        0,
        sim.interner.intern("AP"),
        sim.interner.intern("SubTorpedo"),
    );
    // Supplied header state, not a second launch implementation. A missing
    // target expires in the terminal control without unrelated damage effects.
    let spawn = ProjectileSpawn {
        native_unique_id: row["bullet_native_id"].as_i64().unwrap() as i32,
        line_trail: None,
        flat: false,
        source_id: 0,
        origin: position,
        target,
        initial_target_position: ProjectileCoord::new(4224, 5248, 624),
        payload,
        speed_leptons_per_frame: 64,
        velocity: ProjectileVelocity::new(64, 0, 0),
        trajectory: ProjectileTrajectory::Straight,
        guidance: None,
        visual: ProjectileVisualState::new(0, 0, 0),
        arm_frames: 0,
        fuse_frames: None,
        ranged_fuse: false,
        tracks_target: false,
        target_expiry: TargetExpiryPolicy::Expire,
        collision: ProjectileCollisionPolicy::NONE,
    };
    let id = sim.allocate_stable_id();
    sim.admit_projectile(id, spawn);
    (sim, id)
}

fn only_bubble(sim: &Simulation) -> u64 {
    let ids: Vec<_> = sim
        .substrate
        .anims
        .iter()
        .filter_map(|(id, anim)| (sim.interner.resolve(anim.type_id) == "BBBLELRG").then_some(*id))
        .collect();
    assert_eq!(ids.len(), 1);
    ids[0]
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn projectile_trailer_retail_constructor_rng_and_complete_lifetime_match_native() {
    let corpus = native();
    for name in ["stock_lifetime", "constructor_random_rate"] {
        let row = case(&corpus, name);
        let Some(rules) = bound_retail_rules(row["supplied"]["random_rate"].as_str()) else {
            return;
        };
        let (mut sim, bullet) = scene(row, false);
        sim.object_ai_visit_one_with_effects(bullet, Some(&rules), ObjectAiCtx::default());
        let bubble = only_bubble(&sim);
        assert_eq!(
            anim_state(&sim, sim.anim(bubble).unwrap()),
            row["emissions"][0]["constructed"],
            "{name}: original full constructor"
        );
        assert_eq!(rngs(&sim), row["after_rng"], "{name}: constructor streams");
        assert_eq!(
            sim.native_unique_ids.as_ref().unwrap().current_raw(),
            row["after_native_id"].as_u64().unwrap() as u32
        );
        assert_ne!(
            sim.projectiles.get(bullet).unwrap().position,
            coord(&row["emissions"][0]["position"]),
            "trail stands at pre-flight Location"
        );
        assert!(sim.substrate.logic.as_slice().contains(&bubble));

        assert!(sim.retire_non_entity_object(bullet));
        assert_eq!(counts(&sim), row["after_bullet_uninit"]["counts"]);
        assert_eq!(
            anim_state(&sim, sim.anim(bubble).unwrap()),
            row["after_bullet_uninit"]["anim"]
        );
        sim.process_pending_delete_with(
            Some(&rules),
            None,
            crate::sim::world::FrameEffects::default(),
            1,
            false,
        );
        assert_eq!(counts(&sim), row["after_bullet_drain"]["counts"]);
        assert_eq!(
            anim_state(&sim, sim.anim(bubble).unwrap()),
            row["after_bullet_drain"]["anim"]
        );
        for frame in row["anim_frames"].as_array().unwrap() {
            sim.session.binary_frame = frame["frame"].as_u64().unwrap() as u32;
            sim.visit_anim(
                bubble,
                &rules,
                None,
                crate::sim::world::FrameEffects::default(),
            );
            assert_eq!(
                anim_state(&sim, sim.anim(bubble).unwrap()),
                frame["anim"],
                "{name}: frame {}",
                frame["frame"]
            );
            assert_eq!(
                counts(&sim),
                frame["counts"],
                "{name}: frame {}",
                frame["frame"]
            );
            sim.process_pending_delete_with(
                Some(&rules),
                None,
                crate::sim::world::FrameEffects::default(),
                1,
                false,
            );
        }
        assert_eq!(counts(&sim), row["final_counts"]);
        assert_eq!(rngs(&sim), row["final_rng"], "{name}: lifecycle streams");
        assert!(sim.substrate.logic.as_slice().is_empty());
    }
}

#[test]
fn projectile_trailer_is_registered_before_terminal_logic_compaction() {
    let Some(rules) = bound_retail_rules(None) else {
        return;
    };
    let corpus = native();
    let row = case(&corpus, "stock_lifetime");
    for terminal in [false, true] {
        let (mut sim, bullet) = scene(row, terminal);
        // Existing live tail shares production ObjectAI, append and compacting
        // removal. Header corpus does not execute the complete native Logic loop;
        // its ordering rests on 55B613 / 55BAE0 instruction-level evidence.
        sim.visit_combat_tail(
            bullet,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        let bubble = only_bubble(&sim);
        assert_eq!(
            sim.anim(bubble).unwrap().runtime.first_ai_guard,
            terminal,
            "a terminal Bullet's removal shifts its new Anim behind the cursor"
        );
        assert_eq!(
            sim.substrate.pending_delete.contains_object(&bullet),
            terminal
        );
        assert_eq!(
            sim.anim(bubble).unwrap().world_coord.x,
            coord(&row["supplied"]["position"]).x
        );
        assert_eq!(rngs(&sim), row["after_rng"]);
    }
}

fn restored(sim: &Simulation) -> Simulation {
    let bytes = crate::sim::snapshot::GameSnapshot::save(sim, 0, 0, "trailer", 0);
    let mut result = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    // Production restores live process streams around the native Scenario
    // seed-zero reset. This continuation has no constructor RNG left to draw.
    // These loaded controls run as independent test processes.
    result.main_rng = sim.main_rng.snapshot_for_test().into();
    result.mapgen_rng = sim.mapgen_rng.clone();
    result.restore_after_snapshot_load().unwrap();
    result
}

#[test]
fn projectile_trailer_save_load_preserves_live_and_pending_cleanup_boundaries() {
    let Some(rules) = bound_retail_rules(None) else {
        return;
    };
    let corpus = native();
    let row = case(&corpus, "stock_lifetime");
    let (mut sim, bullet) = scene(row, false);
    sim.object_ai_visit_one_with_effects(bullet, Some(&rules), ObjectAiCtx::default());
    let bubble = only_bubble(&sim);
    // Persist both live, then a Bullet already UnInit but not yet drained.
    for pending in [false, true] {
        if pending {
            assert!(sim.retire_non_entity_object(bullet));
        }
        let mut first = restored(&sim);
        let mut second = restored(&sim);
        assert_eq!(
            first.substrate.logic.snapshot(),
            sim.substrate.logic.snapshot()
        );
        assert_eq!(
            bincode::serialize(&first.substrate.display).unwrap(),
            bincode::serialize(&sim.substrate.display).unwrap()
        );
        assert_eq!(
            anim_state(&first, first.anim(bubble).unwrap()),
            anim_state(&sim, sim.anim(bubble).unwrap())
        );
        assert_eq!(first.substrate.pending_delete, sim.substrate.pending_delete);
        assert_eq!(
            first.scenario_rng.logical_state(),
            SimRng::new(0).logical_state()
        );
        assert_eq!(first.state_hash(), second.state_hash());
        // Close the Bullet boundary in both loaded controls, then continue the
        // native-pinned independent animation history through its final drain.
        for loaded in [&mut first, &mut second] {
            if !pending {
                assert!(loaded.retire_non_entity_object(bullet));
            }
            loaded.process_pending_delete_with(
                Some(&rules),
                None,
                crate::sim::world::FrameEffects::default(),
                1,
                false,
            );
        }
        for frame in row["anim_frames"].as_array().unwrap() {
            for loaded in [&mut first, &mut second] {
                loaded.session.binary_frame = frame["frame"].as_u64().unwrap() as u32;
                loaded.visit_anim(
                    bubble,
                    &rules,
                    None,
                    crate::sim::world::FrameEffects::default(),
                );
                assert_eq!(
                    anim_state(loaded, loaded.anim(bubble).unwrap()),
                    frame["anim"]
                );
                loaded.process_pending_delete_with(
                    Some(&rules),
                    None,
                    crate::sim::world::FrameEffects::default(),
                    1,
                    false,
                );
            }
            assert_eq!(first.state_hash(), second.state_hash());
            assert_eq!(rngs(&first), rngs(&second));
        }
        assert_eq!(counts(&first), row["final_counts"]);
    }
}
