//! Phobos `OmniFire.TurnToTarget` through the production
//! `FireVisit::UnitTarget` path: `UnitClass_UpdateFiring_OmniFireTurnToTarget`
//! (`0x736E6E`, `Ext/Unit/Hooks.Jumpjet.cpp:51-95`) turns an omni-fire unit's
//! hull toward its target; a turreted type skips it.

use super::world_fire;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::world::{FrameEffects, Simulation};

const EAST: u16 = 0x4000;

/// A turretless `FIRER` unit holding a rival target two cells south, aimed
/// east. `global_extra` appends `[General]` keys, `weapon_extra` appends
/// `[GUN]` keys.
fn arena(
    global_extra: &str,
    weapon_extra: &str,
    turret: bool,
) -> (Simulation, RuleSet, u64, u64, u16) {
    arena_with_type(global_extra, weapon_extra, turret, "")
}

fn arena_with_type(
    global_extra: &str,
    weapon_extra: &str,
    turret: bool,
    type_extra: &str,
) -> (Simulation, RuleSet, u64, u64, u16) {
    let turret_key = if turret { "Turret=yes\n" } else { "" };
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[Warheads]\n0=HIT\n[BuildingTypes]\n0=DUMMY\n\
         [VehicleTypes]\n0=FIRER\n\
         [FIRER]\nStrength=1000\nArmor=heavy\nSpeed=4\nPrimary=GUN\nCanPassiveAquire=no\n\
         {turret_key}{type_extra}\
         [DUMMY]\nStrength=10000\nArmor=heavy\nCanPassiveAquire=no\n\
         [WeaponTypes]\n0=GUN\n\
         [GUN]\nDamage=10\nROF=40\nRange=9.5\nSpeed=100\nProjectile=INVIS\nWarhead=HIT\n\
         OmniFire=yes\n{weapon_extra}\
         [INVIS]\nInviso=yes\nAG=yes\n\
         [HIT]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [General]\n{global_extra}"
    ));
    let rules = RuleSet::from_ini(&ini).expect("rules read");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
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
        .spawn_object("DUMMY", "Russians", 10, 12, 0, &rules)
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
    let actor = sim.entities_mut_for_test().get_mut(firer).expect("firer");
    actor.body_facing.snap(EAST, 100);
    assert_eq!(
        actor.barrel_facing.is_some(),
        turret,
        "the turret slot follows the type"
    );
    assert!(
        sim.assign_target_represented(
            firer,
            Some(TargetKind::Entity(target)),
            &rules,
            FrameEffects::empty()
        )
        .is_ok(),
        "the firer holds the target as a mission would"
    );
    let heading = crate::sim::movement::turret::facing_toward_target(
        sim.entities().get(firer).unwrap(),
        &TargetKind::Entity(target),
        &sim,
    )
    .expect("the target has coordinates");
    assert_ne!(heading, EAST, "the target sits off the initial heading");
    (sim, rules, firer, target, heading)
}

fn destination_after_firing(sim: &mut Simulation, rules: &RuleSet, firer: u64) -> u16 {
    sim.commit_fire_visit(
        world_fire::FireVisit::UnitTarget(firer),
        rules,
        None,
        FrameEffects::empty(),
    );
    sim.entities()
        .get(firer)
        .map(|entity| entity.body_facing.destination())
        .expect("firer")
}

/// The `[General]` key off (the constructor default) leaves the hull alone.
#[test]
fn the_default_key_keeps_the_hull_heading() {
    let (mut sim, rules, firer, _target, _heading) = arena("", "", false);
    assert_eq!(destination_after_firing(&mut sim, &rules, firer), EAST);
}

/// The `[General]` key on turns the hull toward the target.
#[test]
fn the_global_key_turns_the_hull_toward_the_target() {
    let (mut sim, rules, firer, _target, heading) = arena("OmniFire.TurnToTarget=yes\n", "", false);
    assert_eq!(destination_after_firing(&mut sim, &rules, firer), heading);
}

/// A weapon key of `no` overrides the global `yes`.
#[test]
fn a_weapon_key_overrides_the_global() {
    let (mut sim, rules, firer, _target, _heading) = arena(
        "OmniFire.TurnToTarget=yes\n",
        "OmniFire.TurnToTarget=no\n",
        false,
    );
    assert_eq!(destination_after_firing(&mut sim, &rules, firer), EAST);
}

/// A weapon key of `yes` enables the turn with the global unset.
#[test]
fn a_weapon_key_enables_the_turn_alone() {
    let (mut sim, rules, firer, _target, heading) = arena("", "OmniFire.TurnToTarget=yes\n", false);
    assert_eq!(destination_after_firing(&mut sim, &rules, firer), heading);
}

/// A turreted type skips the hull turn: native's `HasTurret` is `!Turret`,
/// so `Turret && !HasTurret` is just `Turret`.
#[test]
fn a_turreted_type_keeps_its_hull_heading() {
    let (mut sim, rules, firer, _target, _heading) = arena("OmniFire.TurnToTarget=yes\n", "", true);
    assert_eq!(destination_after_firing(&mut sim, &rules, firer), EAST);
}

/// A jumpjet sets `LocomotionFacing.SetDesired`, not the body's facing; its
/// Update copies the locomotion facing to the body.
#[test]
fn a_jumpjet_turns_its_locomotion_facing() {
    let (mut sim, rules, firer, _target, heading) = arena_with_type(
        "OmniFire.TurnToTarget=yes\n",
        "",
        false,
        "Locomotor={92612C46-F71F-11d1-AC9F-006008055BB5}\nJumpjetHeight=500\n",
    );
    let body_before = sim.entities().get(firer).unwrap().body_facing.destination();
    sim.commit_fire_visit(
        world_fire::FireVisit::UnitTarget(firer),
        &rules,
        None,
        FrameEffects::empty(),
    );
    let entity = sim.entities().get(firer).expect("firer");
    let runtime = entity
        .locomotor
        .as_ref()
        .and_then(|loco| loco.jumpjet_runtime())
        .expect("the type's locomotor is a Jumpjet");
    assert_eq!(runtime.facing_destination(), heading);
    assert_eq!(entity.body_facing.destination(), body_before);
}
