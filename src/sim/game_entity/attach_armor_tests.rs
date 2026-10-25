//! Ground warhead armor effects through real Bullet damage and crate/restore owners.
use crate::rules::ruleset::RuleSet;
use crate::sim::{
    combat::{TargetKind, world_fire::FireVisit},
    components::DriveCoord,
    projectile::ProjectileCoord,
    world::{FrameEffects, ObjectAiCtx, Simulation},
};
use crate::util::native_x87::NativeF64Bits;

fn scene() -> (Simulation, RuleSet, u64, u64) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[VehicleTypes]\n0=EMITTER\n1=VICTIM\n[Warheads]\n0=AE\n1=STACK\n2=HIT\n\
         [EMITTER]\nStrength=1000\nArmor=heavy\nPrimary=PULSE\nSecondary=GUN\n\
         [VICTIM]\nStrength=1000\nArmor=heavy\nSpeed=4\n\
         [PULSE]\nDamage=0\nROF=1\nRange=10\nSpeed=100\nProjectile=INVIS\nWarhead=AE\n\
         [GUN]\nDamage=96\nROF=1\nRange=10\nSpeed=100\nProjectile=INVIS\nWarhead=HIT\n\
         [INVIS]\nInviso=yes\nAG=yes\n\
         [AE]\nCellSpread=.7\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         AttachEffect.Duration=4\nAttachEffect.ArmorMultiplier=2\nAttachEffect.DiscardOnEntry=yes\n\
         [STACK]\nCellSpread=.7\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         AttachEffect.Duration=2\nAttachEffect.ArmorMultiplier=.5\nAttachEffect.Cumulative=yes\n\
         [HIT]\nCellSpread=0\nPercentAtMax=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let rules = RuleSet::from_ini(&ini).unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    let emitter = sim
        .spawn_object("EMITTER", "Americans", 12, 10, 0, &rules)
        .unwrap();
    let victim = sim
        .spawn_object("VICTIM", "Russians", 10, 10, 0, &rules)
        .unwrap();
    sim.resolve_type_handles(&rules);
    (sim, rules, emitter, victim)
}

fn hit(sim: &mut Simulation, rules: &RuleSet, emitter: u64, victim: u64, wh: &str) {
    let p = crate::sim::movement::ground_pose::position_world_coord(
        &sim.entities().get(victim).unwrap().position,
    );
    sim.apply_ares_attach_effect_detonation(
        ProjectileCoord::new(p.x, p.y, p.z),
        Some(sim.entities().get(emitter).unwrap().owner()),
        rules.warhead(wh).unwrap(),
        rules,
    );
}

fn shoot(sim: &mut Simulation, rules: &RuleSet, emitter: u64, victim: u64, weapon_index: i32) {
    let before: std::collections::BTreeSet<_> = sim.projectiles.iter().map(|(&id, _)| id).collect();
    sim.commit_fire_visit(
        FireVisit::Direct {
            id: emitter,
            target: TargetKind::Entity(victim),
            weapon_index,
        },
        rules,
        None,
        FrameEffects::empty(),
    );
    let bullets: Vec<_> = sim
        .projectiles
        .iter()
        .map(|(&id, _)| id)
        .filter(|id| !before.contains(id))
        .collect();
    assert_eq!(bullets.len(), 1);
    sim.object_ai_visit_one(bullets[0], Some(rules), ObjectAiCtx::default());
}

fn armor(sim: &Simulation, victim: u64) -> f64 {
    f64::from_bits(
        sim.entities()
            .get(victim)
            .unwrap()
            .crate_multipliers
            .armor()
            .bits(),
    )
}

fn armor_crate(sim: &mut Simulation, rules: &RuleSet, victim: u64) -> bool {
    let p = crate::sim::movement::ground_pose::position_world_coord(
        &sim.entities().get(victim).unwrap().position,
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
        NativeF64Bits::from_bits(1.5_f64.to_bits()),
        crate::sim::crates::Multiplier::Armor,
    )
    .is_empty()
}

#[test]
fn attached_armor_parent_hit_changes_received_damage_then_expires() {
    let (mut sim, rules, emitter, victim) = scene();
    shoot(&mut sim, &rules, emitter, victim, 0);
    assert_eq!(armor(&sim, victim), 2.0);
    let hp = sim.entities().get(victim).unwrap().health.current();
    shoot(&mut sim, &rules, emitter, victim, 1);
    assert_eq!(
        sim.entities().get(victim).unwrap().health.current(),
        hp - 48
    );
    for _ in 0..4 {
        sim.update_ares_attach_effects(victim, &rules);
    }
    assert_eq!(armor(&sim, victim), 1.0);
    let hp = sim.entities().get(victim).unwrap().health.current();
    shoot(&mut sim, &rules, emitter, victim, 1);
    assert_eq!(
        sim.entities().get(victim).unwrap().health.current(),
        hp - 96
    );
}

#[test]
fn attached_armor_refresh_stack_and_crates_keep_the_permanent_baseline() {
    let (mut sim, rules, emitter, victim) = scene();
    hit(&mut sim, &rules, emitter, victim, "AE");
    assert_eq!(armor(&sim, victim), 2.0);
    assert_eq!(
        crate::sim::crates::multiplayer_eligible_slot(
            &sim,
            &rules,
            victim,
            0,
            crate::rules::powerups::POWERUP_ARMOR
        ),
        crate::rules::powerups::POWERUP_ARMOR
    );
    assert!(armor_crate(&mut sim, &rules, victim));
    assert_eq!(armor(&sim, victim), 3.0);
    hit(&mut sim, &rules, emitter, victim, "AE");
    hit(&mut sim, &rules, emitter, victim, "STACK");
    hit(&mut sim, &rules, emitter, victim, "STACK");
    assert_eq!(armor(&sim, victim), 0.75);
    let hp = sim.entities().get(victim).unwrap().health.current();
    shoot(&mut sim, &rules, emitter, victim, 1);
    assert_eq!(
        sim.entities().get(victim).unwrap().health.current(),
        hp - 128
    );
    assert!(!armor_crate(&mut sim, &rules, victim));
    for _ in 0..2 {
        sim.update_ares_attach_effects(victim, &rules);
    }
    assert_eq!(armor(&sim, victim), 3.0);
    for _ in 0..2 {
        sim.update_ares_attach_effects(victim, &rules);
    }
    assert_eq!(armor(&sim, victim), 1.5);
}

#[test]
fn attached_armor_restore_checks_both_values_and_discard_recovers_the_crate() {
    let (mut sim, rules, emitter, victim) = scene();
    assert!(armor_crate(&mut sim, &rules, victim));
    hit(&mut sim, &rules, emitter, victim, "AE");
    let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "attached armor", 0);
    let mut loaded = crate::sim::snapshot::GameSnapshot::load(&saved)
        .unwrap()
        .sim;
    loaded.restore_after_snapshot_load().unwrap();
    assert_eq!(armor(&loaded, victim), 3.0);
    loaded
        .validate_attached_stats_after_restore(&rules)
        .unwrap();
    let hash = loaded.state_hash();
    loaded
        .substrate
        .entities
        .get_mut(victim)
        .unwrap()
        .crate_multipliers
        .set_armor_multiplier_for_test(NativeF64Bits::ONE);
    loaded
        .substrate
        .entities
        .get_mut(victim)
        .unwrap()
        .crate_multipliers
        .set_attached_armor(NativeF64Bits::from_bits(3.0_f64.to_bits()));
    assert_eq!(
        armor(&loaded, victim),
        3.0,
        "only the baseline was corrupted"
    );
    assert_ne!(loaded.state_hash(), hash);
    assert!(
        loaded
            .validate_attached_stats_after_restore(&rules)
            .is_err()
    );
    sim.limbo_ares_attach_effects(victim, &rules);
    assert_eq!(armor(&sim, victim), 1.5);
}
