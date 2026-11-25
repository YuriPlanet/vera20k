//! Phobos `NoTurret.TrackTarget` through the `0x7410BB` hook of
//! `UnitClass::GetFireError` (`Ext/Techno/Hooks.Misc.cpp:752-769`): a keyed
//! turretless unit that is not warping in continues its REARM at U5, so the
//! facing check turns it toward the target while the weapon cools down.

use super::TargetKind;
use super::fire_error::FireError;
use super::fire_error_world::FireSubject;
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::teleport_movement::ChronoWarp;
use crate::sim::world::Simulation;

/// `TRK` with the given UnitType key and `[General]` line, plus an unarmed
/// `TGT` three cells south of a north-facing attacker.
fn scene(
    unit_key: &str,
    general_line: &str,
    turret: &str,
    facing: u8,
) -> (Simulation, RuleSet, u64, u64) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[Countries]\n0=Americans\n1=Russians\n\
         [VehicleTypes]\n0=TRK\n1=TGT\n\
         [WeaponTypes]\n0=GUN\n\
         [GUN]\nDamage=5\nROF=40\nRange=9.5\nSpeed=100\nProjectile=INVIS\nWarhead=HIT\n\
         [INVIS]\nInviso=yes\nAG=yes\n\
         [HIT]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [TRK]\nStrength=200\nArmor=none\nSpeed=4\nPrimary=GUN\nTurret={turret}\n{unit_key}\
         [TGT]\nStrength=200\nArmor=none\nSpeed=4\n\
         [General]\n{general_line}"
    ));
    let rules = RuleSet::from_ini(&ini).expect("rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x0D_EE_77);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session.set_binary_frame_for_test(100);
    let attacker = sim
        .spawn_object("TRK", "Americans", 10, 10, facing, &rules)
        .expect("attacker");
    let target = sim
        .spawn_object("TGT", "Russians", 10, 13, 0, &rules)
        .expect("target");
    (sim, rules, attacker, target)
}

/// The attacker's own ROF timer is still running (T45).
fn hold_rearm(sim: &mut Simulation, id: u64) {
    let frame = sim.session.binary_frame() as i32;
    sim.substrate
        .entities
        .get_mut(id)
        .expect("attacker")
        .seldom.rearm_timer
        .start(frame, 40);
}

fn fire_error(sim: &Simulation, rules: &RuleSet, attacker: u64, target: u64) -> FireError {
    let firer = sim.substrate.entities.get(attacker).expect("attacker");
    FireSubject {
        world: sim,
        rules,
        fog: None,
        firer,
        obj: rules.object("TRK").expect("TRK"),
        target: Some(TargetKind::Entity(target)),
        weapon_index: 0,
    }
    .fire_error(true)
}

#[test]
fn an_unkeyed_turretless_unit_keeps_the_stock_rearm_gate() {
    let (mut sim, rules, attacker, target) = scene("", "", "no", 0);
    hold_rearm(&mut sim, attacker);
    assert_eq!(
        fire_error(&sim, &rules, attacker, target),
        FireError::Rearm,
        "no key: the facing check never runs while cooling down"
    );
}

#[test]
fn a_keyed_turretless_unit_checks_facing_during_rearm() {
    let (mut sim, rules, attacker, target) = scene("NoTurret.TrackTarget=yes\n", "", "no", 0);
    hold_rearm(&mut sim, attacker);
    assert_eq!(
        fire_error(&sim, &rules, attacker, target),
        FireError::Facing,
        "the hull is off-facing, so the hook continues into U12"
    );

    // An aligned hull reaches the U13 tail and keeps the base REARM instead
    // of turning into a shot.
    let (mut sim, rules, attacker, target) = scene("NoTurret.TrackTarget=yes\n", "", "no", 0x80);
    hold_rearm(&mut sim, attacker);
    assert_eq!(fire_error(&sim, &rules, attacker, target), FireError::Rearm);
}

#[test]
fn the_general_line_is_the_unset_type_fallback() {
    let (mut sim, rules, attacker, target) = scene("", "NoTurret.TrackTarget=yes\n", "no", 0);
    hold_rearm(&mut sim, attacker);
    assert_eq!(
        fire_error(&sim, &rules, attacker, target),
        FireError::Facing,
        "an unset UnitType key takes [General]"
    );

    let (mut sim, rules, attacker, target) = scene(
        "NoTurret.TrackTarget=no\n",
        "NoTurret.TrackTarget=yes\n",
        "no",
        0,
    );
    hold_rearm(&mut sim, attacker);
    assert_eq!(
        fire_error(&sim, &rules, attacker, target),
        FireError::Rearm,
        "the type's own no overrides [General]"
    );
}

#[test]
fn a_turreted_or_warping_unit_ignores_the_key() {
    let (mut sim, rules, attacker, target) = scene("NoTurret.TrackTarget=yes\n", "", "yes", 0);
    hold_rearm(&mut sim, attacker);
    assert_eq!(
        fire_error(&sim, &rules, attacker, target),
        FireError::Rearm,
        "pType->Turret keeps the stock gate"
    );

    let (mut sim, rules, attacker, target) = scene("NoTurret.TrackTarget=yes\n", "", "no", 0);
    hold_rearm(&mut sim, attacker);
    let frame = sim.session.binary_frame();
    let destination = crate::sim::movement::ground_pose::object_get_coords(
        sim.substrate.entities.get(attacker).expect("attacker"),
        None,
    );
    let mut locomotor =
        LocomotorState::for_test_kind(crate::rules::locomotor_type::LocomotorKind::Teleport);
    locomotor
        .teleport_runtime_mut()
        .expect("teleport runtime")
        .arm_chrono(ChronoWarp::reinforcement(destination, frame));
    sim.substrate
        .entities
        .get_mut(attacker)
        .expect("attacker")
        .locomotor = Some(locomotor);
    assert_eq!(
        fire_error(&sim, &rules, attacker, target),
        FireError::Rearm,
        "IsWarpingIn keeps the stock gate"
    );
}
