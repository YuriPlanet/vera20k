//! Original Bullet4666E0 trailer constructor and independent Anim lifetime.
//! Native witnesses supply an in-flight Bullet boundary for exact headers and
//! Anim lifetimes. Scalable fixtures also exercise actual UnitTarget/Direct
//! FireAt, shared expiry and load normalization. Release profiles cover SUB.

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
use crate::util::save_codec;
use serde_json::{Value, json};

fn native() -> &'static Value {
    crate::test_fixture::json("tools/projectile_oracle/projectile_trailer.json")
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
        let mut art = (*fixture.fixed_art).clone();
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
    let p = anim.world_coord();
    json!({
        "native_id": anim.native_unique_id(),
        "alive": !sim.substrate.pending_delete.contains_object(&anim.stable_id()),
        "position": [p.x,p.y,p.z],
        "stage": anim.runtime().current_frame,
        "timer": [anim.runtime().frame_timer.start_frame(), anim.runtime().frame_timer.duration()],
        "rate": anim.runtime().rate_reload,
        "step": anim.runtime().frame_step,
        "owner_object": anim.owner_entity().unwrap_or(0),
        "bullet": anim.attached_bullet().unwrap_or(0),
        "z_adjust": anim.z_adjust(),
        "delay": anim.runtime().delay_remaining,
        "draw_flags": anim.draw_flags(),
        "loops": anim.runtime().loop_remaining,
        "brand_new": anim.runtime().first_ai_guard,
    })
}

fn counts(sim: &Simulation) -> Value {
    json!({"bullets":sim.projectiles.len(), "anims":sim.substrate.anims.len(),
        "pending":sim.substrate.pending_delete.len()})
}

fn scene(row: &Value, terminal: bool, rules: &RuleSet) -> (Simulation, u64) {
    let mut sim = Simulation::with_seed(31);
    sim.main_rng = SimRng::new(31).into();
    sim.scenario_rng = SimRng::new(31);
    sim.session
        .set_binary_frame_for_test(row["supplied"]["frame"].as_i64().unwrap() as u32);
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
        rules
            .weapon("SubTorpedo")
            .unwrap()
            .projectile
            .as_deref()
            .map(|name| sim.interner.intern(name)),
        rules.weapon("SubTorpedo").unwrap().bright,
    )
    .with_installed_weapon(Some(sim.interner.intern("SubTorpedo")));
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
    let id = sim.substrate.allocate_stable_id();
    sim.admit_projectile(id, spawn);
    (sim, id)
}

fn only_bubble(sim: &Simulation) -> u64 {
    let ids: Vec<_> = sim
        .substrate
        .anims
        .iter()
        .filter_map(|(id, anim)| {
            (sim.interner.resolve(anim.type_id()) == "BBBLELRG").then_some(*id)
        })
        .collect();
    assert_eq!(ids.len(), 1);
    ids[0]
}

/// Authored Scalable+Trailer combination through the real Unit FireAt
/// receiver. Original Unit741439 enrolls each returned Bullet; InitScalable
/// 46B280 updates this retained type's cadence at the eighth live member.
/// This is not a stock Medusa visual claim (its Trailer is null).
fn scalable_unit_scene(shots: usize) -> Option<(Simulation, RuleSet, u64, u64)> {
    // Four tests start from this patched load, so it is read once.
    let fixture = crate::test_fixture::once_per_process("scalable Cannon Battle rules", || {
        crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(&IniFile::from_str(
            "[Cannon]\nImage=SUBT\nScalable=yes\n",
        ))
    })?
    .with_fresh_serial();
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
    let mut rules = fixture.rules;
    assert_eq!(rules.object("MTNK").unwrap().primary(), Some("105mm"));
    assert_eq!(
        rules.weapon("105mm").unwrap().projectile.as_deref(),
        Some("Cannon")
    );
    let kind = rules.projectile("Cannon").unwrap();
    assert!(kind.scalable);
    assert_eq!(kind.trailer.as_deref(), Some("BBBLELRG"));
    assert_eq!(kind.spawn_delay, 3);
    let roots = crate::rules::effect_asset_catalog::anim_class_roots(&rules);
    rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");

    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 31);
    let tank = sim
        .spawn_object("MTNK", "Americans", 10, 20, 0, &rules)
        .unwrap();
    let target = sim
        .spawn_object("MTNK", "Russians", 13, 20, 0, &rules)
        .unwrap();
    // Native70BCB0 aims through live TarCom independently of FireAt's
    // explicit target. A direct mission call starts after this assignment.
    sim.assign_target_represented(
        tank,
        Some(crate::sim::combat::TargetKind::Entity(target)),
        Some(&rules),
        None,
        super::FrameEffects::default(),
    )
    .unwrap();
    for _ in 0..shots {
        let before = sim.projectiles.len();
        sim.commit_fire_visit(
            crate::sim::combat::world_fire::FireVisit::Direct {
                id: tank,
                target: crate::sim::combat::TargetKind::Entity(target),
                weapon_index: 0,
            },
            &rules,
            None,
            super::FrameEffects::default(),
        );
        assert_eq!(
            sim.projectiles.len(),
            before + 1,
            "each actual FireAt returned a Bullet"
        );
    }
    assert!(
        sim.substrate
            .anims
            .iter()
            .all(|(_, anim)| { sim.interner.resolve(anim.type_id()) != "BBBLELRG" })
    );
    Some((sim, rules, tank, target))
}

#[test]
fn scalable_unit_shots_emit_a_real_trailer_on_the_shared_eighth_member_cadence() {
    let Some((mut sim, rules, _, _)) = scalable_unit_scene(8) else {
        return;
    };
    let ids: Vec<_> = sim.projectiles.iter().map(|(&id, _)| id).collect();
    assert_eq!(sim.projectiles.scalable_members(), ids);
    let kind = sim.interner.get("Cannon").unwrap();
    assert_eq!(sim.projectiles.scaled_spawn_delay(kind), 4);
    let (&bullet, projectile) = sim.projectiles.iter().next().unwrap();
    let origin = projectile.position();
    sim.session.set_binary_frame_for_test(4);
    sim.object_ai_visit_one_with_effects(bullet, Some(&rules), ObjectAiCtx::default());
    let bubble = only_bubble(&sim);
    let position = sim.anim(bubble).unwrap().world_coord();
    assert_eq!(
        [position.x, position.y, position.z],
        [origin.x, origin.y, origin.z],
        "the real Anim constructor receives pre-flight Location"
    );
    assert!(sim.substrate.logic.as_slice().contains(&bubble));
    assert_ne!(sim.projectiles.get(bullet).unwrap().position(), origin);
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
        let (mut sim, bullet) = scene(row, false, &rules);
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
            sim.projectiles.get(bullet).unwrap().position(),
            coord(&row["emissions"][0]["position"]),
            "trail stands at pre-flight Location"
        );
        assert!(sim.substrate.logic.as_slice().contains(&bubble));

        assert!(sim.retire_non_entity_object(bullet, super::UninitContext::with_rules(&rules)));
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
            sim.session
                .set_binary_frame_for_test(frame["frame"].as_u64().unwrap() as u32);
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
        let (mut sim, bullet) = scene(row, terminal, &rules);
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
            sim.anim(bubble).unwrap().runtime().first_ai_guard,
            terminal,
            "a terminal Bullet's removal shifts its new Anim behind the cursor"
        );
        assert_eq!(
            sim.substrate.pending_delete.contains_object(&bullet),
            terminal
        );
        assert_eq!(
            sim.anim(bubble).unwrap().world_coord().x,
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
    let (mut sim, bullet) = scene(row, false, &rules);
    sim.object_ai_visit_one_with_effects(bullet, Some(&rules), ObjectAiCtx::default());
    let bubble = only_bubble(&sim);
    // Persist both live, then a Bullet already UnInit but not yet drained.
    for pending in [false, true] {
        if pending {
            assert!(sim.retire_non_entity_object(bullet, super::UninitContext::with_rules(&rules)));
        }
        let mut first = restored(&sim);
        let mut second = restored(&sim);
        assert_eq!(
            first.substrate.logic.snapshot(),
            sim.substrate.logic.snapshot()
        );
        assert_eq!(
            save_codec::serialize(&first.substrate.display).unwrap(),
            save_codec::serialize(&sim.substrate.display).unwrap()
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
                assert!(
                    loaded
                        .retire_non_entity_object(bullet, super::UninitContext::with_rules(&rules))
                );
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
                loaded
                    .session
                    .set_binary_frame_for_test(frame["frame"].as_u64().unwrap() as u32);
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

#[test]
fn scalable_unit_target_and_direct_share_enrollment_but_failed_launch_does_not() {
    let Some((mut sim, rules, tank, target)) = scalable_unit_scene(0) else {
        return;
    };
    let source = sim.substrate.entities.get_mut(tank).unwrap();
    source.body_facing.snap(0x4000, 0);
    source.barrel_facing.as_mut().unwrap().snap(0x4000, 0);
    sim.commit_fire_visit(
        crate::sim::combat::world_fire::FireVisit::UnitTarget(tank),
        &rules,
        None,
        super::FrameEffects::default(),
    );
    let ids: Vec<_> = sim.projectiles.iter().map(|(&id, _)| id).collect();
    assert_eq!(
        ids.len(),
        1,
        "the actual UnitTarget admission fires one shot"
    );
    assert_eq!(sim.projectiles.scalable_members(), ids);
    //70BCB0's null TarCom aim is zero, independently of Direct's target;
    // this exact supplied history previously demonstrated failed arc launch.
    sim.assign_target_represented(
        tank,
        None,
        Some(&rules),
        None,
        super::FrameEffects::default(),
    )
    .unwrap();
    sim.commit_fire_visit(
        crate::sim::combat::world_fire::FireVisit::Direct {
            id: tank,
            target: crate::sim::combat::TargetKind::Entity(target),
            weapon_index: 0,
        },
        &rules,
        None,
        super::FrameEffects::default(),
    );
    assert_eq!(sim.projectiles.len(), 1);
    assert_eq!(
        sim.projectiles.scalable_members(),
        ids,
        "a null returned Bullet cannot enroll"
    );
}

#[test]
fn scalable_uninit_self_callback_removes_before_drain_and_keeps_type_override() {
    let Some((mut sim, rules, _, _)) = scalable_unit_scene(1) else {
        return;
    };
    let id = sim.projectiles.scalable_members()[0];
    let type_id = sim
        .projectiles
        .get(id)
        .unwrap()
        .payload()
        .projectile_type()
        .unwrap();
    let corpus = native();
    let rows = corpus["scalable_controls"]["sole_retirement"]["rows"]
        .as_array()
        .unwrap();
    assert_eq!(
        sim.projectiles.scaled_spawn_delay(type_id),
        rows[0]["after"]["types"]["Torpedo"]["scaled_delay"]
            .as_i64()
            .unwrap() as i32
    );
    assert!(sim.retire_non_entity_object(id, super::UninitContext::with_rules(&rules)));
    assert!(
        sim.projectiles.get(id).is_some(),
        "native UnInit keeps the expiring object represented"
    );
    assert_eq!(
        sim.projectiles.scalable_members().len() as u64,
        rows[1]["after"]["count"].as_u64().unwrap()
    );
    assert_eq!(sim.projectiles.scaled_spawn_delay(type_id), 3);
    assert!(sim.substrate.pending_delete.contains_object(&id));
    sim.process_pending_delete_with(Some(&rules), None, super::FrameEffects::default(), 1, false);
    assert!(sim.projectiles.get(id).is_none());
    assert_eq!(
        sim.projectiles.scalable_members().len() as u64,
        rows[2]["after"]["count"].as_u64().unwrap()
    );
    assert_eq!(
        sim.projectiles.scaled_spawn_delay(type_id),
        3,
        "destructor does not reset type+2E8"
    );
}

#[test]
fn scalable_snapshot_mechanical_state_and_native_load_have_distinct_membership() {
    let Some((mut sim, rules, _, _)) = scalable_unit_scene(8) else {
        return;
    };
    let type_id = sim.interner.get("Cannon").unwrap();
    let encoded = save_codec::serialize(&sim.projectiles).unwrap();
    let mechanical: crate::sim::projectile::ProjectileStore =
        save_codec::deserialize(&encoded).unwrap();
    // in_logic_vector is an existing serde-skipped derived flag rebuilt by
    // the world. Compare persisted state, not that deliberately absent flag.
    assert_eq!(
        mechanical.scalable_members(),
        sim.projectiles.scalable_members()
    );
    assert_eq!(mechanical.scaled_spawn_delay(type_id), 4);
    assert_eq!(save_codec::serialize(&mechanical).unwrap(), encoded);
    let mut loaded = restored(&sim);
    assert!(
        loaded.projectiles.scalable_members().is_empty(),
        "normal native load does not rerun Unit741439"
    );
    assert_eq!(loaded.projectiles.scaled_spawn_delay(type_id), 4);
    let mut control = restored(&sim);
    assert_eq!(loaded.state_hash(), control.state_hash());
    let members: Vec<_> = sim.projectiles.scalable_members().to_vec();
    for id in members {
        assert!(sim.retire_non_entity_object(id, super::UninitContext::with_rules(&rules)));
    }
    let mut no_members = restored(&sim);
    assert!(no_members.projectiles.scalable_members().is_empty());
    assert_eq!(no_members.projectiles.scaled_spawn_delay(type_id), 4);
    let retained = no_members.projectiles.clone();
    let retained_hash = no_members.state_hash();
    let mut raw = serde_json::to_value(&no_members.projectiles).unwrap();
    raw["scaled_spawn_delays"] = json!({});
    no_members.projectiles = serde_json::from_value(raw).unwrap();
    assert_ne!(
        retained_hash,
        no_members.state_hash(),
        "retained type cadence is hashed even without members"
    );
    no_members.projectiles = retained;
    assert_eq!(no_members.state_hash(), retained_hash);
    // Loaded membership is empty but the old type cadence remains effective
    // until another Unit shot overwrites it, through the same AI/Anim owner.
    let bullet = *loaded.projectiles.iter().next().unwrap().0;
    for world in [&mut loaded, &mut control] {
        world.session.set_binary_frame_for_test(4);
        world.object_ai_visit_one_with_effects(bullet, Some(&rules), ObjectAiCtx::default());
        only_bubble(world);
    }
    assert_eq!(loaded.state_hash(), control.state_hash());

    let mut invalid = serde_json::to_value(&mechanical).unwrap();
    invalid["scalable_members"] = json!([u64::MAX]);
    let mut rejected = no_members;
    rejected.projectiles = serde_json::from_value(invalid).unwrap();
    assert!(matches!(
        rejected.restore_after_snapshot_load(),
        Err(crate::sim::snapshot::SnapshotRestoreError::InvalidScalableProjectileRuntime(_))
    ));
    let mut invalid = serde_json::to_value(&mechanical).unwrap();
    invalid["scaled_spawn_delays"] = json!({"4294967295": 4});
    rejected.projectiles = serde_json::from_value(invalid).unwrap();
    assert!(matches!(
        rejected.restore_after_snapshot_load(),
        Err(crate::sim::snapshot::SnapshotRestoreError::InvalidScalableProjectileRuntime(_))
    ));
}
