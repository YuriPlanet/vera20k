use super::*;
use crate::rules::ruleset::RuleSet;
use crate::sim::{
    projectile::ProjectileCoord,
    world::{ObjectAiCtx, Simulation},
};

fn scene(self_effect: bool) -> (Simulation, RuleSet, u64) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[General]\nCloakingStages=6\n[VehicleTypes]\n0=SELF_TEST\n1=BLOCKED\n[Warheads]\n0=CLOAK_WH\n\
         [SELF_TEST]\nStrength=100\nSpeed=4\nCloakingSpeed=1\n\
         Locomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\nMovementZone=Normal\n\
         AttachEffect.Duration={}\nAttachEffect.InitialDelay=2\nAttachEffect.Delay=2\n\
         AttachEffect.Cloakable=yes\nAttachEffect.PenetratesIronCurtain=yes\n\
         [BLOCKED]\nStrength=100\nSpeed=4\nCloakable.Allowed=no\n\
         [CLOAK_WH]\nCellSpread=.7\nAffectsAllies=yes\n\
         Verses=3%,3%,3%,3%,3%,3%,3%,3%,3%,3%,3%\n\
         AttachEffect.Duration=2\nAttachEffect.Cloakable=yes\n",
        if self_effect { 3 } else { 0 }
    ));
    let rules = RuleSet::from_ini(&ini).unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    let actor = sim
        .spawn_object("SELF_TEST", "Americans", 10, 10, 0, &rules)
        .unwrap();
    sim.resolve_type_handles(&rules);
    (sim, rules, actor)
}

fn visit(sim: &mut Simulation, rules: &RuleSet, actor: u64, frame: u32) {
    sim.session.set_binary_frame_for_test(frame);
    sim.object_ai_visit_one(actor, Some(rules), ObjectAiCtx::default());
}

#[test]
fn self_cloak_reaches_normal_ai_and_restores_the_initial_and_repeat_waits() {
    let (mut sim, rules, actor) = scene(true);
    visit(&mut sim, &rules, actor, 100);
    assert_eq!(
        sim.entities()
            .get(actor)
            .unwrap()
            .attach_effects
            .self_effect,
        SelfEffectState::Pending(1)
    );
    visit(&mut sim, &rules, actor, 101);
    assert_eq!(
        sim.entities()
            .get(actor)
            .unwrap()
            .attach_effects
            .self_effect,
        SelfEffectState::Pending(0)
    );
    let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "self-cloak", 0);
    let mut sim = crate::sim::snapshot::GameSnapshot::load(&saved)
        .unwrap()
        .sim;
    visit(&mut sim, &rules, actor, 102);
    let entity = sim.entities().get(actor).unwrap();
    assert_eq!(entity.attach_effects.entries[0].remaining, 3);
    assert_eq!(entity.attach_effects.self_effect, SelfEffectState::Applied);
    assert!(entity.stealth_ability.active(false));
    assert_eq!(
        entity.cloak.unwrap().state(),
        1,
        "the existing cloak AI consumes the new ability"
    );
    sim.substrate
        .entities
        .get_mut(actor)
        .unwrap()
        .cloak
        .as_mut()
        .unwrap()
        .establish_unlimbo_fully_cloaked();
    visit(&mut sim, &rules, actor, 103);
    visit(&mut sim, &rules, actor, 104);
    visit(&mut sim, &rules, actor, 105);
    let entity = sim.entities().get(actor).unwrap();
    assert!(entity.attach_effects.entries.is_empty());
    assert_eq!(
        entity.attach_effects.self_effect,
        SelfEffectState::Pending(1),
        "expiry consumes the first repeat-wait tick in the same head"
    );
    assert!(!entity.stealth_ability.active(false));
    assert_eq!(
        entity.cloak.unwrap().state(),
        3,
        "losing ability must continue the existing uncloak clock"
    );
    visit(&mut sim, &rules, actor, 106);
    assert_eq!(
        sim.entities()
            .get(actor)
            .unwrap()
            .attach_effects
            .self_effect,
        SelfEffectState::Pending(0)
    );
    visit(&mut sim, &rules, actor, 107);
    assert_eq!(
        sim.entities().get(actor).unwrap().attach_effects.entries[0].remaining,
        3
    );
    sim.validate_attached_stats_after_restore(&rules).unwrap();
}

fn hit(sim: &mut Simulation, rules: &RuleSet, actor: u64) {
    let entity = sim.entities().get(actor).unwrap();
    let p = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
    sim.apply_ares_attach_effect_detonation(
        ProjectileCoord::new(p.x, p.y, p.z),
        Some(entity.owner()),
        rules.warhead("CLOAK_WH").unwrap(),
        rules,
    );
}

#[test]
fn temporary_cloak_keeps_permanent_crate_eligibility_and_respects_type_prohibition() {
    let (mut sim, rules, actor) = scene(false);
    hit(&mut sim, &rules, actor);
    assert!(
        sim.entities()
            .get(actor)
            .unwrap()
            .stealth_ability
            .active(false)
    );
    assert_eq!(
        crate::sim::crates::multiplayer_eligible_slot(&sim, &rules, actor, 0, 3),
        3,
        "a temporary ability is not a permanent crate"
    );
    sim.substrate
        .entities
        .get_mut(actor)
        .unwrap()
        .stealth_ability
        .grant();
    sim.update_ares_attach_effects(actor, &rules);
    sim.update_ares_attach_effects(actor, &rules);
    let entity = sim.entities().get(actor).unwrap();
    assert!(!entity.stealth_ability.attached());
    assert!(
        entity.stealth_ability.active(false),
        "expiry keeps the permanent grant"
    );
    assert_eq!(
        crate::sim::crates::multiplayer_eligible_slot(&sim, &rules, actor, 0, 3),
        0
    );
    let blocked = sim
        .spawn_object("BLOCKED", "Americans", 14, 14, 0, &rules)
        .unwrap();
    hit(&mut sim, &rules, blocked);
    visit(&mut sim, &rules, blocked, 100);
    assert_eq!(
        sim.entities().get(blocked).unwrap().cloak.unwrap().state(),
        0
    );
    assert_eq!(
        crate::sim::crates::multiplayer_eligible_slot(&sim, &rules, blocked, 0, 3),
        0,
        "Cloakable.Allowed=no rejects cloak crates too"
    );
    sim.validate_attached_stats_after_restore(&rules).unwrap();
}
