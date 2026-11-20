//! Phobos `FeedbackWeapon=` through the production `FireVisit::UnitTarget`
//! path.
//!
//! `TechnoClass_FireAt_FeedbackWeapon` (`0x6FF43F`,
//! `Ext/Techno/Hooks.Firing.cpp:802-812`): after a successful shot, the
//! fired weapon's `FeedbackWeapon=` detonates through
//! `WeaponTypeExt::DetonateAt(weapon, pThis, pThis)` on the firer itself,
//! with the firer's coordinates, the firer as owner and target, and no
//! firing house.

use super::world_fire;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::movement::FacingClass;
use crate::sim::world::{FrameEffects, Simulation};

/// A `FIRER` vehicle firing `GUN` east at a `DUMMY` two cells out.
/// `gun_keys` appends keys to the primary weapon; the named `FEEDBACK`
/// weapon carries `feedback_keys`.
fn arena(gun_keys: &str, feedback_keys: &str) -> (Simulation, RuleSet, u64, u64) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[Warheads]\n0=HIT\n1=FEEDBACKWH\n\
         [BuildingTypes]\n0=DUMMY\n\
         [VehicleTypes]\n0=FIRER\n\
         [FIRER]\nStrength=1000\nArmor=heavy\nSpeed=4\nPrimary=GUN\nOmniFire=yes\nCanPassiveAquire=no\n\
         [DUMMY]\nStrength=10000\nArmor=heavy\nCanPassiveAquire=no\n\
         [WeaponTypes]\n0=GUN\n1=FEEDBACK\n\
         [GUN]\nDamage=10\nROF=40\nRange=9.5\nSpeed=100\nProjectile=INVIS\nWarhead=HIT\n{gun_keys}\
         [FEEDBACK]\nDamage=30\nROF=10\nRange=9.5\nSpeed=100\nProjectile=INVIS\nWarhead=FEEDBACKWH\n{feedback_keys}\
         [INVIS]\nInviso=yes\nAG=yes\n\
         [HIT]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [FEEDBACKWH]\nCellSpread=0\nAffectsAllies=yes\nAllowDamageOnSelf=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    ));
    let mut rules = RuleSet::from_ini(&ini).expect("rules read");
    rules.install_art_data(crate::rules::art_data::ArtRegistry::empty());
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x6FF4_3F00);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session
        .set_current_house_for_test(sim.interner.get("Americans"));
    sim.session.set_binary_frame_for_test(100);
    let firer = sim
        .spawn_object("FIRER", "Americans", 10, 10, 0, &rules)
        .expect("firer");
    let target = sim
        .spawn_object("DUMMY", "Russians", 12, 10, 0, &rules)
        .expect("target");
    for id in [firer, target] {
        let entity = sim
            .entities_mut_for_test()
            .get_mut(id)
            .expect("placed object");
        entity.lifecycle.set_in_limbo_for_test(false);
    }
    let owner = sim.entities().get(target).map(|entity| entity.owner());
    let victim = sim
        .entities()
        .get(target)
        .map(|entity| (entity.position.rx(), entity.position.ry()))
        .expect("target");
    sim.fog
        .mark_visible_for_owner(owner.expect("owner"), victim.0, victim.1);
    let east = 0x4000;
    let actor = sim.entities_mut_for_test().get_mut(firer).expect("firer");
    actor.body_facing.snap(east, 100);
    actor.barrel_facing = Some(FacingClass::new(east, 0));
    assert!(
        sim.assign_target_represented(
            firer,
            Some(TargetKind::Entity(target)),
            &rules,
            FrameEffects::empty()
        )
        .is_ok(),
        "the firer holds the target"
    );
    (sim, rules, firer, target)
}

/// An open-topped `CARR` carrying a `RIDER` whose `GUN` names `FEEDBACK`.
/// `feedback_keys` appends the feedback weapon's `FireInTransport=`.
fn carrier_arena(feedback_keys: &str) -> (Simulation, RuleSet, u64, u64) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[Warheads]\n0=HIT\n1=FEEDBACKWH\n\
         [VehicleTypes]\n0=CARR\n1=RIDER\n2=DUMMY\n\
         [WeaponTypes]\n0=GUN\n1=FEEDBACK\n\
         [GUN]\nDamage=10\nROF=40\nRange=9.5\nSpeed=100\nProjectile=INVIS\nWarhead=HIT\n\
         FireInTransport=yes\nFeedbackWeapon=FEEDBACK\n\
         [FEEDBACK]\nDamage=30\nROF=10\nRange=9.5\nSpeed=100\nProjectile=INVIS\nWarhead=FEEDBACKWH\n{feedback_keys}\
         [INVIS]\nInviso=yes\nAG=yes\n\
         [HIT]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [FEEDBACKWH]\nCellSpread=0\nAffectsAllies=yes\nAllowDamageOnSelf=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [CARR]\nStrength=800\nArmor=heavy\nSpeed=5\nPassengers=1\nOpenTopped=yes\n\
         [RIDER]\nStrength=200\nArmor=none\nSpeed=4\nPrimary=GUN\nOmniFire=yes\n\
         [DUMMY]\nStrength=1000\nArmor=heavy\nCanPassiveAquire=no\n"
    ));
    let mut rules = RuleSet::from_ini(&ini).expect("rules read");
    rules.install_art_data(crate::rules::art_data::ArtRegistry::empty());
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x6FF4_3F01);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session
        .set_current_house_for_test(sim.interner.get("Americans"));
    sim.session.set_binary_frame_for_test(100);
    let carrier = sim
        .spawn_object("CARR", "Americans", 10, 10, 0, &rules)
        .expect("carrier");
    let rider = sim
        .spawn_object("RIDER", "Americans", 12, 10, 0, &rules)
        .expect("rider");
    let target = sim
        .spawn_object("DUMMY", "Russians", 14, 10, 0, &rules)
        .expect("target");
    for id in [carrier, rider, target] {
        let entity = sim
            .entities_mut_for_test()
            .get_mut(id)
            .expect("placed object");
        entity.lifecycle.set_in_limbo_for_test(false);
    }
    let owner = sim.entities().get(target).map(|entity| entity.owner());
    let victim = sim
        .entities()
        .get(target)
        .map(|entity| (entity.position.rx(), entity.position.ry()))
        .expect("target");
    sim.fog
        .mark_visible_for_owner(owner.expect("owner"), victim.0, victim.1);
    let east = 0x4000;
    let actor = sim.entities_mut_for_test().get_mut(rider).expect("rider");
    actor.body_facing.snap(east, 100);
    actor.barrel_facing = Some(FacingClass::new(east, 0));
    actor.passenger_role.set_inside_transport(carrier, true);
    assert!(
        sim.assign_target_represented(
            rider,
            Some(TargetKind::Entity(target)),
            &rules,
            FrameEffects::empty()
        )
        .is_ok(),
        "the rider holds the target"
    );
    (sim, rules, carrier, rider)
}

fn health(sim: &Simulation, id: u64) -> i32 {
    sim.entities()
        .get(id)
        .expect("live entity")
        .health
        .current()
}

fn visit(sim: &mut Simulation, rules: &RuleSet, firer: u64) {
    sim.commit_fire_visit(
        world_fire::FireVisit::UnitTarget(firer),
        rules,
        None,
        FrameEffects::empty(),
    );
}

/// `FeedbackWeapon=FEEDBACK` detonates the feedback weapon's warhead on the
/// firer in the same FireAt, after the primary bullet exists.
#[test]
fn a_feedback_weapon_detonates_on_the_firer() {
    let (mut sim, rules, firer, _) = arena("FeedbackWeapon=FEEDBACK\n", "");
    let before = health(&sim, firer);
    visit(&mut sim, &rules, firer);
    assert_eq!(sim.fire_events.len(), 1, "the primary shot was admitted");
    assert_eq!(sim.projectiles().len(), 1, "the primary bullet exists");
    assert_eq!(
        health(&sim, firer),
        before - 30,
        "the feedback weapon's 30-damage warhead hit the firer"
    );
}

/// A weapon that names no `FeedbackWeapon=` fires only the primary shot.
#[test]
fn without_a_feedback_weapon_only_the_primary_shot_fires() {
    let (mut sim, rules, firer, _) = arena("", "");
    let before = health(&sim, firer);
    visit(&mut sim, &rules, firer);
    assert_eq!(sim.fire_events.len(), 1, "the primary shot was admitted");
    assert_eq!(sim.projectiles().len(), 1, "the primary bullet exists");
    assert_eq!(
        health(&sim, firer),
        before,
        "no second detonation without FeedbackWeapon="
    );
}

/// An open-topped passenger keeps the feedback shot unless the *feedback*
/// weapon's `FireInTransport=no` (`Hooks.Firing.cpp:806-807`).
#[test]
fn a_feedback_weapon_respects_the_open_topped_fire_in_transport_gate() {
    let (mut sim, rules, _carrier, rider) = carrier_arena("FireInTransport=no\n");
    let before = health(&sim, rider);
    visit(&mut sim, &rules, rider);
    assert_eq!(sim.fire_events.len(), 1, "the primary shot was admitted");
    assert_eq!(
        health(&sim, rider),
        before,
        "FireInTransport=no skips the feedback shot"
    );

    let (mut sim, rules, _carrier, rider) = carrier_arena("");
    let before = health(&sim, rider);
    visit(&mut sim, &rules, rider);
    assert_eq!(sim.fire_events.len(), 1, "the primary shot was admitted");
    assert_eq!(
        health(&sim, rider),
        before - 30,
        "the default FireInTransport=yes fires the feedback shot"
    );
}
