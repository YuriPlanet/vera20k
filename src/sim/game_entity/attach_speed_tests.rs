//! Native executed speed values through the retained reader and production
//! attachment/crate/getter owners; a separate bounded Drive integration.
use super::*;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::{
    combat::{TargetKind, world_fire::FireVisit},
    components::DriveCoord,
    projectile::ProjectileCoord,
    world::{FrameEffects, ObjectAiCtx, Simulation},
};
use crate::util::{fixed_math::SIM_ONE, native_x87::NativeF64Bits};

fn scene() -> (Simulation, RuleSet, u64, u64) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[VehicleTypes]\n0=EMITTER\n1=MOVER\n[Warheads]\n0=SLOW\n1=NEUTRAL\n\
         [EMITTER]\nStrength=400\nArmor=heavy\nSpeed=6\nPrimary=PULSE\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Normal\n\
         [MOVER]\nStrength=1000\nArmor=heavy\nSpeed=4\nROT=255\nAccelerates=no\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Normal\n\
         [PULSE]\nDamage=0\nROF=50\nRange=9.5\nSpeed=100\nProjectile=INVIS\nWarhead=SLOW\n\
         [INVIS]\nAirburst=yes\nInviso=yes\nAG=yes\nAA=no\n\
         [SLOW]\nCellSpread=.7\nVerses=3%,3%,3%,3%,3%,3%,3%,3%,3%,3%,3%\n\
         AffectsAllies=yes\nAttachEffect.Duration=60\nAttachEffect.SpeedMultiplier=0.2\nAttachEffect.DiscardOnEntry=yes\n\
         [NEUTRAL]\nCellSpread=.7\nVerses=3%,3%,3%,3%,3%,3%,3%,3%,3%,3%,3%\n\
         AffectsAllies=yes\nAttachEffect.Duration=5\n",
    );
    let rules = RuleSet::from_ini(&ini).unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    let target = sim
        .spawn_object("MOVER", "Russians", 10, 10, 0, &rules)
        .unwrap();
    let source = sim
        .spawn_object("EMITTER", "Americans", 12, 10, 0, &rules)
        .unwrap();
    sim.resolve_type_handles(&rules);
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .foot_speed
        .set_speed_fraction(SIM_ONE);
    (sim, rules, target, source)
}

fn hit(sim: &mut Simulation, rules: &RuleSet, target: u64, source: u64, wh: &str) {
    let coord = crate::sim::movement::ground_pose::position_world_coord(
        &sim.entities().get(target).unwrap().position,
    );
    let house = sim.entities().get(source).unwrap().owner();
    sim.apply_ares_attach_effect_detonation(
        ProjectileCoord::new(coord.x, coord.y, coord.z),
        Some(house),
        rules.warhead(wh).unwrap(),
        rules,
    );
}

fn bits(value: &serde_json::Value) -> NativeF64Bits {
    let text = value.as_str().unwrap();
    let bytes = std::array::from_fn(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap());
    NativeF64Bits::from_bits(u64::from_le_bytes(bytes))
}

#[test]
fn attached_speed_native_histories_include_crates_refresh_expiry_and_discard() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/ares_attach_speed.json",
    ))
    .unwrap();
    for row in corpus["controls"].as_array().unwrap() {
        let (mut sim, rules, target, source) = scene();
        let configured = rules
            .warhead("SLOW")
            .unwrap()
            .ares_rules()
            .unwrap()
            .attach_effect();
        assert_eq!(
            configured.speed_multiplier(),
            bits(&row["descriptor"]["speed_bits"])
        );
        assert_eq!(
            configured.duration(),
            row["descriptor"]["duration"].as_i64().unwrap() as i32
        );
        assert_eq!(
            configured.discard_on_entry(),
            row["descriptor"]["discard"].as_bool().unwrap()
        );
        let rng = sim.scenario_rng.logical_state();
        for step in row["steps"].as_array().unwrap() {
            let action = step["action"].as_str().unwrap();
            match action {
                "prepared" => {}
                "first_hit"
                | "noncumulative_reapply"
                | "hit_after_crate"
                | "slow_hit_with_neutral" => {
                    hit(&mut sim, &rules, target, source, "SLOW");
                }
                "neutral_hit" => hit(&mut sim, &rules, target, source, "NEUTRAL"),
                "tick_1" => sim.update_ares_attach_effects(target, &rules),
                "sixty_visits_expire"
                | "five_visits_neutral_expired"
                | "sixty_total_visits_all_expired" => {
                    let count = match action {
                        "sixty_visits_expire" => 60,
                        "five_visits_neutral_expired" => 5,
                        _ => 55,
                    };
                    for _ in 0..count {
                        sim.update_ares_attach_effects(target, &rules);
                    }
                }
                "speed_crate_before_hit"
                | "accept_crate_while_slowed"
                | "reject_duplicate_crate" => {
                    let slot = crate::sim::crates::multiplayer_eligible_slot(
                        &sim,
                        &rules,
                        target,
                        crate::rules::terrain_rules::LandType::Clear.as_index(),
                        crate::rules::powerups::POWERUP_SPEED,
                    );
                    assert_eq!(
                        slot == crate::rules::powerups::POWERUP_SPEED,
                        step["crate"]["picker_continuation"] == "00481D52"
                    );
                    let p = crate::sim::movement::ground_pose::position_world_coord(
                        &sim.entities().get(target).unwrap().position,
                    );
                    let before = sim
                        .entities()
                        .get(target)
                        .unwrap()
                        .foot_speed
                        .crate_multiplier();
                    crate::sim::crates::apply_speed_crate(
                        &mut sim.substrate,
                        &sim.houses,
                        &rules,
                        &sim.interner,
                        DriveCoord {
                            x: p.x,
                            y: p.y,
                            z: p.z,
                        },
                        1,
                        NativeF64Bits::from_bits(0x3ff3_3333_3333_3333),
                    );
                    assert_eq!(
                        sim.entities()
                            .get(target)
                            .unwrap()
                            .foot_speed
                            .crate_multiplier()
                            != before,
                        step["crate"]["recipient_continuation"] == "00483078",
                        "the native accepted/rejected recipient continuation agrees"
                    );
                }
                "discard_on_entry_and_aggregate" => sim.limbo_ares_attach_effects(target, &rules),
                action => panic!("unknown native speed action {action}"),
            }
            let victim = sim.entities().get(target).unwrap();
            let label = format!("{} {action}", row["name"]);
            assert_eq!(
                victim.foot_speed.crate_multiplier(),
                bits(&step["crate_base_bits"]),
                "{label}"
            );
            assert_eq!(
                victim.foot_speed.effective_multiplier(),
                bits(&step["derived_speed_bits"]),
                "{label}"
            );
            assert_eq!(
                sim.current_speed_for_test(target, &rules),
                step["foot_current_speed"].as_i64().unwrap() as i32,
                "{label}"
            );
            assert_eq!(
                serde_json::json!(
                    victim
                        .attach_effects
                        .entries
                        .iter()
                        .map(|e| e.remaining)
                        .collect::<Vec<_>>()
                ),
                step["remaining"],
                "{label}"
            );
            assert_eq!(
                sim.scenario_rng.logical_state(),
                rng,
                "{label}: no animation or added draw"
            );
            sim.validate_attached_speed_after_restore(&rules).unwrap();
        }
    }
}

#[test]
fn attached_speed_parent_bullet_reaches_drive_and_validated_snapshot() {
    let (mut sim, rules, target, source) = scene();
    sim.commit_fire_visit(
        FireVisit::Direct {
            id: source,
            target: TargetKind::Entity(target),
            weapon_index: 0,
        },
        &rules,
        None,
        FrameEffects::empty(),
    );
    let bullets: Vec<_> = sim.projectiles.iter().map(|(&id, _)| id).collect();
    assert_eq!(bullets.len(), 1);
    for id in bullets {
        sim.object_ai_visit_one(id, Some(&rules), ObjectAiCtx::default());
    }
    assert_eq!(
        sim.entities().get(target).unwrap().attach_effects.entries[0].remaining,
        60
    );
    assert_eq!(sim.current_speed_for_test(target, &rules), 1);
    let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "attached speed", 0);
    let mut loaded = crate::sim::snapshot::GameSnapshot::load(&saved)
        .unwrap()
        .sim;
    loaded.restore_after_snapshot_load().unwrap();
    loaded
        .validate_attached_speed_after_restore(&rules)
        .unwrap();
    assert_eq!(
        loaded.entities().get(target).unwrap().foot_speed,
        sim.entities().get(target).unwrap().foot_speed
    );
    loaded
        .substrate
        .entities
        .get_mut(target)
        .unwrap()
        .foot_speed
        .set_attached_speed(NativeF64Bits::ONE);
    assert!(
        loaded
            .validate_attached_speed_after_restore(&rules)
            .is_err(),
        "stale derived speed is rejected, not silently repaired"
    );

    // The frame fixture reaches the actual object-turn Drive Process. Keep
    // the real hit's attachment, but limbo both emitters through the lifecycle
    // owner so passive enemy fire cannot refresh it during the comparison.
    // This is Rust integration, not a native movement-distance golden.
    let (mut ordinary, _, normal_target, normal_source) = scene();
    sim.techno_limbo_with_rules(source, &rules, None);
    ordinary.techno_limbo_with_rules(normal_source, &rules, None);
    let start = crate::sim::movement::ground_pose::position_world_coord(
        &sim.entities().get(target).unwrap().position,
    );
    for (world, id, attached) in [
        (&mut sim, target, true),
        (&mut ordinary, normal_target, false),
    ] {
        assert!(world.apply_command(
            "Russians",
            &crate::sim::command::Command::Move {
                entity_id: id,
                target_rx: 10,
                target_ry: 20,
            },
            Some(&rules)
        ));
        let first_tick = world.session.tick();
        for visit in 1..=16_u64 {
            let result = world.advance_tick(&[], Some(&rules), None, None, 67);
            assert!(
                result.frame_committed,
                "movement frame {visit} was admitted"
            );
            assert_eq!(result.tick, first_tick + visit);
            if attached {
                assert_eq!(
                    world.entities().get(id).unwrap().attach_effects.entries[0].remaining,
                    60 - visit as i32,
                    "one attachment update per admitted Drive visit"
                );
            }
        }
    }
    let distance = |world: &Simulation, id| {
        let p = crate::sim::movement::ground_pose::position_world_coord(
            &world.entities().get(id).unwrap().position,
        );
        (p.x - start.x).abs() + (p.y - start.y).abs()
    };
    let slowed = distance(&sim, target);
    assert!(slowed > 0, "the slowed Drive still advances");
    assert!(
        distance(&ordinary, normal_target) > slowed,
        "Drive uses the attachment, not only the read-only getter"
    );
    assert_eq!(
        sim.entities().get(target).unwrap().attach_effects.entries[0].remaining,
        44
    );
}
