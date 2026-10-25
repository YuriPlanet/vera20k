//! Two executed native histories plus one ordinary Bullet/HP/scan chain.
use crate::rules::ruleset::{CountryDifficultyBiases, RuleSet};
use crate::sim::{
    combat::{ScanMission, TargetKind, world_fire::FireVisit},
    components::DriveCoord,
    house_state::HouseDifficulty,
    projectile::ProjectileCoord,
    world::{FrameEffects, ObjectAiCtx, Simulation},
};
use crate::util::native_x87::NativeF64Bits;

fn scene(discard: bool, veteran: bool) -> (Simulation, RuleSet, u64, u64, u64) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[General]\nVeteranCombat=1.5\nVeteranArmor=1\n\
         [VehicleTypes]\n0=FIRER\n1=EMITTER\n2=DUMMY\n[Warheads]\n0=FACTOR_A\n1=FACTOR_B\n2=HIT\n\
         [FIRER]\nStrength=1000\nArmor=heavy\nSpeed=4\nPrimary=GUN\nSecondary=HEAL\nVeteranAbilities=FIREPOWER\n\
         [EMITTER]\nStrength=1000\nArmor=heavy\nPrimary=PULSE\n\
         [DUMMY]\nStrength=10000\nArmor=heavy\n\
         [GUN]\nDamage=101\nROF=50\nRange=9.5\nSpeed=100\nProjectile=INVIS\nWarhead=HIT\n\
         [HEAL]\nDamage=-7\nROF=50\nRange=9.5\nSpeed=100\nProjectile=INVIS\nWarhead=HIT\n\
         [PULSE]\nDamage=0\nROF=10000\nRange=9.5\nSpeed=100\nProjectile=INVIS\nWarhead=FACTOR_A\n\
         [INVIS]\nInviso=yes\nAG=yes\nAA=no\n\
         [FACTOR_A]\nCellSpread=.7\nVerses=3%,3%,3%,3%,3%,3%,3%,3%,3%,3%,3%\nAffectsAllies=yes\n\
         AttachEffect.Duration=4\nAttachEffect.FirepowerMultiplier=0.2\nAttachEffect.DiscardOnEntry={discard}\n\
         [FACTOR_B]\nCellSpread=.7\nVerses=3%,3%,3%,3%,3%,3%,3%,3%,3%,3%,3%\nAffectsAllies=yes\n\
         AttachEffect.Duration=2\nAttachEffect.FirepowerMultiplier=1.1\nAttachEffect.Cumulative=yes\nAttachEffect.DiscardOnEntry=yes\n\
         [HIT]\nCellSpread=0\nPercentAtMax=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    ));
    let rules = RuleSet::from_ini(&ini).unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    let firer = sim
        .spawn_object("FIRER", "Russians", 10, 10, 0, &rules)
        .unwrap();
    let emitter = sim
        .spawn_object("EMITTER", "Americans", 12, 10, 0, &rules)
        .unwrap();
    let dummy = sim
        .spawn_object("DUMMY", "Americans", 10, 12, 0, &rules)
        .unwrap();
    sim.resolve_type_handles(&rules);
    let observer = sim.entities().get(firer).unwrap().owner();
    sim.fog.mark_visible_for_owner(observer, 10, 12);
    let owner = sim.entities().get(firer).unwrap().owner();
    let mut general = rules.general.clone();
    general.difficulty_rows[1].firepower = 1.25;
    sim.houses.get_mut(&owner).unwrap().set_difficulty(
        HouseDifficulty::Normal,
        &general,
        CountryDifficultyBiases::default(),
        false,
        0,
        0,
    );
    if veteran {
        sim.substrate
            .entities
            .get_mut(firer)
            .unwrap()
            .veterancy
            .set_rank_for_test(100);
    }
    (sim, rules, firer, emitter, dummy)
}

fn bits(value: &serde_json::Value) -> NativeF64Bits {
    let raw = value.as_str().unwrap();
    NativeF64Bits::from_bits(u64::from_le_bytes(std::array::from_fn(|i| {
        u8::from_str_radix(&raw[i * 2..i * 2 + 2], 16).unwrap()
    })))
}
fn hit(sim: &mut Simulation, rules: &RuleSet, firer: u64, emitter: u64, wh: &str) {
    let p = crate::sim::movement::ground_pose::position_world_coord(
        &sim.entities().get(firer).unwrap().position,
    );
    sim.apply_ares_attach_effect_detonation(
        ProjectileCoord::new(p.x, p.y, p.z),
        Some(sim.entities().get(emitter).unwrap().owner()),
        rules.warhead(wh).unwrap(),
        rules,
    );
}
fn shoot(sim: &mut Simulation, rules: &RuleSet, firer: u64, target: u64, weapon_index: i32) -> u64 {
    let before: std::collections::BTreeSet<_> = sim.projectiles.iter().map(|(&id, _)| id).collect();
    sim.commit_fire_visit(
        FireVisit::Direct {
            id: firer,
            target: TargetKind::Entity(target),
            weapon_index,
        },
        rules,
        None,
        FrameEffects::empty(),
    );
    let ids: Vec<_> = sim
        .projectiles
        .iter()
        .map(|(&id, _)| id)
        .filter(|id| !before.contains(id))
        .collect();
    assert_eq!(ids.len(), 1, "ordinary FireAt produces one real Bullet");
    ids[0]
}
fn firepower_crate(sim: &mut Simulation, rules: &RuleSet, firer: u64) -> bool {
    let p = crate::sim::movement::ground_pose::position_world_coord(
        &sim.entities().get(firer).unwrap().position,
    );
    !crate::sim::crates::apply_multiplier_crate(
        &mut sim.substrate,
        rules,
        &sim.interner,
        DriveCoord {
            x: p.x,
            y: p.y,
            z: p.z,
        },
        1,
        NativeF64Bits::from_bits(0x3ff3_3333_3333_3333),
        crate::sim::crates::Multiplier::Firepower,
    )
    .is_empty()
}

#[test]
fn attached_firepower_two_native_histories_reach_live_shot_and_estimate() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/ares_attach_firepower.json",
    ))
    .unwrap();
    for row in corpus["controls"].as_array().unwrap() {
        let first = row["name"] == "live_aggregate_fire_estimate_refresh_crate_expiry";
        let (mut sim, rules, firer, emitter, dummy) = scene(first, !first);
        for descriptor in row["descriptors"].as_array().unwrap() {
            let ae = rules
                .warhead(descriptor["name"].as_str().unwrap())
                .unwrap()
                .ares_rules()
                .unwrap()
                .attach_effect();
            assert_eq!(
                ae.firepower_multiplier(),
                bits(&descriptor["firepower_bits_le"])
            );
        }
        // Ares Firepower1 goes directly to481D52 after baseline==1;
        // the native fixture's ordinary Unit type has no weapon installed.
        assert_eq!(
            crate::sim::crates::multiplayer_eligible_slot(
                &sim,
                &rules,
                dummy,
                crate::rules::terrain_rules::LandType::Clear.as_index(),
                crate::rules::powerups::POWERUP_FIREPOWER
            ),
            crate::rules::powerups::POWERUP_FIREPOWER
        );
        for step in row["steps"].as_array().unwrap() {
            let action = step["action"].as_str().unwrap();
            let rng = sim.scenario_rng.logical_state();
            match action {
                "prepared" | "negative_damage_bypasses_factor" => {}
                "first_hit" | "noncumulative_refresh" | "first_factor" => {
                    hit(&mut sim, &rules, firer, emitter, "FACTOR_A")
                }
                "ordered_second_factor" | "cumulative_third_entry" => {
                    hit(&mut sim, &rules, firer, emitter, "FACTOR_B")
                }
                "tick_1" => sim.update_ares_attach_effects(firer, &rules),
                "four_visits_expire" | "four_visits_expire_first" => {
                    for _ in 0..4 {
                        sim.update_ares_attach_effects(firer, &rules);
                    }
                }
                "discard_removes_second_and_third" => sim.limbo_ares_attach_effects(firer, &rules),
                "crate_during_effect" | "duplicate_crate_rejected" | "crate_before_effect" => {
                    let slot = crate::sim::crates::multiplayer_eligible_slot(
                        &sim,
                        &rules,
                        firer,
                        crate::rules::terrain_rules::LandType::Clear.as_index(),
                        crate::rules::powerups::POWERUP_FIREPOWER,
                    );
                    assert_eq!(
                        slot == crate::rules::powerups::POWERUP_FIREPOWER,
                        step["crate"]["picker"] == "00481D52"
                    );
                    assert_eq!(
                        firepower_crate(&mut sim, &rules, firer),
                        step["crate"]["recipient"] == "00483258"
                    );
                }
                _ => panic!("unknown native action {action}"),
            }
            assert_eq!(
                sim.scenario_rng.logical_state(),
                rng,
                "{action}: AE/crate adds no draw"
            );
            let actor = sim.entities().get(firer).unwrap();
            assert_eq!(
                actor.crate_multipliers.crate_firepower(),
                bits(&step["base_bits_le"]),
                "{action}"
            );
            assert_eq!(
                actor.crate_multipliers.firepower(),
                bits(&step["derived_bits_le"]),
                "{action}"
            );
            assert_eq!(
                serde_json::json!(
                    actor
                        .attach_effects
                        .entries
                        .iter()
                        .map(|e| e.remaining)
                        .collect::<Vec<_>>()
                ),
                step["effects"]["remaining"],
                "{action}"
            );
            sim.validate_attached_stats_after_restore(&rules).unwrap();
            let negative = action == "negative_damage_bypasses_factor";
            let weapon = rules.weapon(if negative { "HEAL" } else { "GUN" }).unwrap();
            assert_eq!(
                crate::sim::combat::estimated_damage_on(&sim, &rules, firer, dummy, weapon),
                step["combat"]["estimated_damage"].as_i64().unwrap() as i32,
                "{action}"
            );
            let bullet = shoot(&mut sim, &rules, firer, dummy, i32::from(negative));
            assert_eq!(
                sim.projectiles.get(bullet).unwrap().payload().base_damage,
                step["combat"]["fire_damage"].as_i64().unwrap() as i32,
                "{action}"
            );
        }
    }
}

#[test]
fn attached_firepower_parent_hit_scan_hp_and_restore_share_effective_owner() {
    let (mut sim, rules, firer, emitter, dummy) = scene(true, false);
    let parent = shoot(&mut sim, &rules, emitter, firer, 0);
    sim.object_ai_visit_one(parent, Some(&rules), ObjectAiCtx::default());
    assert_eq!(
        sim.entities()
            .get(firer)
            .unwrap()
            .attach_effects
            .entries
            .len(),
        1
    );
    // Remove the producer from Logic without deleting the real attachment.
    sim.techno_limbo_with_rules(emitter, &rules, None);
    let estimate = crate::sim::combat::estimated_damage_on(
        &sim,
        &rules,
        firer,
        dummy,
        rules.weapon("GUN").unwrap(),
    );
    assert_eq!(estimate, 25, "first-hit native damage/estimate golden");
    sim.assign_target_represented(firer, None, &rules, None, FrameEffects::empty())
        .unwrap();
    let estimated_before = sim.entities().get(dummy).unwrap().estimated_health.get();
    assert!(crate::sim::world::retaliate_and_scan(
        &mut sim,
        firer,
        &rules,
        ScanMission::Guard,
        ObjectAiCtx::default(),
        None
    ));
    assert_eq!(
        sim.entities().get(dummy).unwrap().estimated_health.get(),
        estimated_before - estimate
    );
    let hp = sim.entities().get(dummy).unwrap().health.current();
    let bullet = shoot(&mut sim, &rules, firer, dummy, 0);
    assert_eq!(
        sim.projectiles.get(bullet).unwrap().payload().base_damage,
        estimate
    );
    sim.object_ai_visit_one(bullet, Some(&rules), ObjectAiCtx::default());
    assert_eq!(
        sim.entities().get(dummy).unwrap().health.current(),
        hp - estimate
    );
    let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "attached firepower", 0);
    let mut loaded = crate::sim::snapshot::GameSnapshot::load(&saved)
        .unwrap()
        .sim;
    loaded.restore_after_snapshot_load().unwrap();
    loaded
        .validate_attached_stats_after_restore(&rules)
        .unwrap();
    assert_eq!(
        loaded.entities().get(firer).unwrap().crate_multipliers,
        sim.entities().get(firer).unwrap().crate_multipliers
    );
    let hash = loaded.state_hash();
    loaded
        .substrate
        .entities
        .get_mut(firer)
        .unwrap()
        .crate_multipliers
        .set_attached_firepower(NativeF64Bits::ONE);
    assert_ne!(loaded.state_hash(), hash);
    assert!(
        loaded
            .validate_attached_stats_after_restore(&rules)
            .is_err()
    );
    assert!(firepower_crate(&mut sim, &rules, firer));
    sim.limbo_ares_attach_effects(firer, &rules);
    assert_eq!(
        crate::sim::combat::estimated_damage_on(
            &sim,
            &rules,
            firer,
            dummy,
            rules.weapon("GUN").unwrap()
        ),
        151,
        "crate baseline restored after Discard"
    );
    hit(&mut sim, &rules, firer, emitter, "FACTOR_A");
    for _ in 0..4 {
        sim.update_ares_attach_effects(firer, &rules);
    }
    assert_eq!(
        sim.entities()
            .get(firer)
            .unwrap()
            .crate_multipliers
            .firepower(),
        sim.entities()
            .get(firer)
            .unwrap()
            .crate_multipliers
            .crate_firepower()
    );
}
