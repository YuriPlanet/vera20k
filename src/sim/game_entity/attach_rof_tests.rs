//! A real ground Bullet applies ROF, then normal ObjectAI expires it.
use crate::rules::ruleset::RuleSet;
use crate::sim::{
    combat::{TargetKind, world_fire::FireVisit},
    rng::SimRng,
    world::{FrameEffects, ObjectAiCtx, Simulation},
};
use crate::util::native_x87::NativeF64Bits;

fn shoot(sim: &mut Simulation, rules: &RuleSet, firer: u64, target: u64) -> u64 {
    let before: std::collections::BTreeSet<_> = sim.projectiles.iter().map(|(&id, _)| id).collect();
    sim.commit_fire_visit(
        FireVisit::Direct {
            id: firer,
            target: TargetKind::Entity(target),
            weapon_index: 0,
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
    assert_eq!(bullets.len(), 1, "FireAt emits one real Bullet");
    bullets[0]
}

#[test]
fn attached_rof_parent_hit_fireat_expiry_and_restore_share_one_owner() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[VehicleTypes]\n0=FIRER\n1=EMITTER\n2=DUMMY\n[Warheads]\n0=AE\n1=HIT\n\
         [FIRER]\nStrength=1000\nArmor=heavy\nSpeed=4\nPrimary=GUN\nCanPassiveAquire=no\n\
         [EMITTER]\nStrength=1000\nArmor=heavy\nPrimary=PULSE\nCanPassiveAquire=no\n\
         [DUMMY]\nStrength=10000\nArmor=heavy\nCanPassiveAquire=no\n\
         [GUN]\nDamage=10\nROF=45\nRange=9.5\nSpeed=100\nProjectile=INVIS\nWarhead=HIT\n\
         [PULSE]\nDamage=0\nROF=10000\nRange=9.5\nSpeed=100\nProjectile=INVIS\nWarhead=AE\n\
         [INVIS]\nInviso=yes\nAG=yes\n\
         [AE]\nCellSpread=.7\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         AttachEffect.Duration=2\nAttachEffect.ROFMultiplier=1.75\n\
         [HIT]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let rules = RuleSet::from_ini(&ini).unwrap();
    let settings = rules
        .warhead("AE")
        .unwrap()
        .ares_rules()
        .unwrap()
        .attach_effect();
    assert_eq!(
        settings.rof_multiplier(),
        NativeF64Bits::from_bits(0x3ffc_0000_0000_0000)
    );
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    let firer = sim
        .spawn_object("FIRER", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let emitter = sim
        .spawn_object("EMITTER", "Russians", 12, 10, 0, &rules)
        .unwrap();
    let dummy = sim
        .spawn_object("DUMMY", "Russians", 10, 12, 0, &rules)
        .unwrap();
    sim.resolve_type_handles(&rules);
    sim.scenario_rng = SimRng::new(31);
    shoot(&mut sim, &rules, firer, dummy);
    let ordinary_timer = sim.entities().get(firer).unwrap().rearm_timer;
    assert!((45..=47).contains(&ordinary_timer.duration()));
    let parent = shoot(&mut sim, &rules, emitter, firer);
    sim.object_ai_visit_one(parent, Some(&rules), ObjectAiCtx::default());
    assert_eq!(
        sim.entities().get(firer).unwrap().rearm_timer,
        ordinary_timer,
        "applying an effect does not restart a running reload"
    );
    assert_eq!(
        sim.entities().get(firer).unwrap().attached_rof_multiplier(),
        settings.rof_multiplier()
    );
    sim.session.set_binary_frame_for_test(46);
    sim.scenario_rng = SimRng::new(31);
    shoot(&mut sim, &rules, firer, dummy);
    let affected_timer = sim.entities().get(firer).unwrap().rearm_timer;
    assert!(
        (78..=80).contains(&affected_timer.duration()),
        "FireAt consumes the attached multiplier before its own jitter"
    );

    let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "attached ROF", 0);
    let mut loaded = crate::sim::snapshot::GameSnapshot::load(&saved)
        .unwrap()
        .sim;
    loaded.restore_after_snapshot_load().unwrap();
    loaded
        .validate_attached_stats_after_restore(&rules)
        .unwrap();
    assert_eq!(
        loaded.entities().get(firer).unwrap().rearm_timer,
        affected_timer
    );
    let hash = loaded.state_hash();
    loaded
        .substrate
        .entities
        .get_mut(firer)
        .unwrap()
        .ares
        .attach_effects
        .rof_multiplier = NativeF64Bits::ONE;
    assert_ne!(loaded.state_hash(), hash);
    assert!(
        loaded
            .validate_attached_stats_after_restore(&rules)
            .is_err()
    );

    for frame in [47, 48] {
        sim.session.set_binary_frame_for_test(frame);
        sim.object_ai_visit_one(firer, Some(&rules), ObjectAiCtx::default());
    }
    let actor = sim.entities().get(firer).unwrap();
    assert_eq!(actor.attached_rof_multiplier(), NativeF64Bits::ONE);
    assert_eq!(
        actor.rearm_timer, affected_timer,
        "expiry leaves the previously sampled timer intact"
    );
    assert_eq!(
        actor.rearm_timer.remaining(48),
        affected_timer.duration() - 2
    );
    sim.session
        .set_binary_frame_for_test(46 + affected_timer.duration() as u32);
    sim.scenario_rng = SimRng::new(31);
    shoot(&mut sim, &rules, firer, dummy);
    assert!((45..=47).contains(&sim.entities().get(firer).unwrap().rearm_timer.duration()));
}
