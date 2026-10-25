//! ForceDecloak's common new/refresh tail calls the existing cloak owner.
use crate::rules::ruleset::RuleSet;
use crate::sim::{cloak_disguise::CloakRuntime, projectile::ProjectileCoord, world::Simulation};

fn scene() -> (Simulation, RuleSet, u64, u64) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[General]\nCloakingStages=6\n\
         [VehicleTypes]\n0=EMITTER\n1=VICTIM\n[Warheads]\n0=FORCE\n1=PLAIN\n\
         [EMITTER]\nStrength=1000\nArmor=heavy\n\
         [VICTIM]\nStrength=1000\nArmor=heavy\nSpeed=4\nCloakable=yes\nCloakingSpeed=3\n\
         [FORCE]\nCellSpread=.7\nVerses=3%,3%,3%,3%,3%,3%,3%,3%,3%,3%,3%\n\
         AttachEffect.Duration=6\nAttachEffect.ForceDecloak=yes\n\
         [PLAIN]\nCellSpread=.7\nVerses=3%,3%,3%,3%,3%,3%,3%,3%,3%,3%,3%\n\
         AttachEffect.Duration=6\n",
    );
    let rules = RuleSet::from_ini(&ini).unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    let target = sim
        .spawn_object("VICTIM", "Russians", 10, 10, 0, &rules)
        .unwrap();
    let source = sim
        .spawn_object("EMITTER", "Americans", 12, 10, 0, &rules)
        .unwrap();
    sim.resolve_type_handles(&rules);
    sim.session.set_binary_frame_for_test(100);
    (sim, rules, target, source)
}

fn hit(sim: &mut Simulation, rules: &RuleSet, target: u64, source: u64, wh: &str) {
    let p = crate::sim::movement::ground_pose::position_world_coord(
        &sim.entities().get(target).unwrap().position,
    );
    // Call the production pre-damage receiver directly. Ordinary ReceiveDamage
    // itself decloaks victims, so a zero-damage Bullet alone could mask this bug.
    sim.apply_ares_attach_effect_detonation(
        ProjectileCoord::new(p.x, p.y, p.z),
        Some(sim.entities().get(source).unwrap().owner()),
        rules.warhead(wh).unwrap(),
        rules,
    );
}

fn cloak(sim: &Simulation, target: u64) -> CloakRuntime {
    sim.entities().get(target).unwrap().cloak.unwrap()
}

#[test]
fn attached_force_decloak_new_hit_and_refresh_preserve_the_transition_clock() {
    let (mut sim, rules, target, source) = scene();
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .cloak
        .as_mut()
        .unwrap()
        .establish_unlimbo_fully_cloaked();
    let sounds = sim.sound_events.len();
    hit(&mut sim, &rules, target, source, "FORCE");
    let started = cloak(&sim, target);
    assert_eq!(
        (started.state(), started.depth(), started.step_delta()),
        (3, 5, -1)
    );
    assert_eq!(started.step_timer().speed(), 3);
    assert_eq!(started.step_timer().timer().start_frame(), 100);
    assert_eq!(started.step_timer().timer().duration(), 3);
    sim.session.set_binary_frame_for_test(103);
    hit(&mut sim, &rules, target, source, "FORCE");
    assert_eq!(
        cloak(&sim, target),
        started,
        "state three does not restart its timer"
    );
    assert_eq!(
        sim.entities()
            .get(target)
            .unwrap()
            .attach_effects
            .entries
            .len(),
        1
    );
    assert_eq!(sim.sound_events.len(), sounds, "Uncloak(true) is silent");
}

#[test]
fn attached_force_decloak_reverses_cloaking_but_not_visible_or_flag_off_targets() {
    let (mut sim, rules, target, source) = scene();
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .cloak
        .as_mut()
        .unwrap()
        .set_state_for_test(0);
    let visible = cloak(&sim, target);
    hit(&mut sim, &rules, target, source, "FORCE");
    assert_eq!(cloak(&sim, target), visible);
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .cloak
        .as_mut()
        .unwrap()
        .start_cloaking(100, 3, true);
    let cloaking = cloak(&sim, target);
    assert_eq!(cloaking.state(), 1);
    hit(&mut sim, &rules, target, source, "PLAIN");
    assert_eq!(cloak(&sim, target), cloaking, "the flag is not implicit");
    hit(&mut sim, &rules, target, source, "FORCE");
    let reversed = cloak(&sim, target);
    assert_eq!(reversed.state(), 3, "a refresh reaches the common tail too");
    for _ in 0..6 {
        sim.update_ares_attach_effects(target, &rules);
    }
    assert!(
        sim.entities()
            .get(target)
            .unwrap()
            .attach_effects
            .entries
            .is_empty()
    );
    assert_eq!(
        cloak(&sim, target),
        reversed,
        "expiry does not undo the cloak transition"
    );
}
