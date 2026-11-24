//! Phobos `FireUp.ResetInRetarget` on `TechnoClass_SetTarget_After`
//! (`0x6FCF3E`, `Ext/Techno/Hooks.cpp:1176-1184`): a turretless Unit with an
//! active `CurrentFiringFrame` clears it for a new target, unless the key is
//! false and the target is already within the selected weapon's range.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::world::{FrameEffects, Simulation};

fn scene(art_keys: &str, turret: bool) -> (Simulation, RuleSet, u64, u64, u64) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[VehicleTypes]\n0=RAIDER\n1=VICTIM\n\
         [RAIDER]\nStrength=500\nArmor=heavy\nSpeed=4\nPrimary=GUN\nTurret={turret}\n\
         [VICTIM]\nStrength=500\nArmor=heavy\nSpeed=4\n\
         [Warheads]\n0=WH\n\
         [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [Weapons]\n0=GUN\n[GUN]\nDamage=1\nROF=10\nRange=5\nWarhead=WH\n"
    ));
    let art = IniFile::from_str(&format!("[RAIDER]\n{art_keys}\n[VICTIM]\n"));
    let mut rules = RuleSet::from_ini(&ini).expect("FireUp.ResetInRetarget rules");
    rules.install_art_fixture(crate::rules::art_data::ArtRegistry::from_ini(&art));
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 3);
    sim.intern_rule_type_ids(&rules);
    let unit = sim
        .spawn_object("RAIDER", "Americans", 10, 10, 0, &rules)
        .expect("raider");
    let near = sim
        .spawn_object("VICTIM", "Russians", 12, 10, 0, &rules)
        .expect("near victim");
    let far = sim
        .spawn_object("VICTIM", "Russians", 25, 25, 0, &rules)
        .expect("far victim");
    (sim, rules, unit, near, far)
}

fn counter(sim: &Simulation, unit: u64) -> i32 {
    sim.substrate
        .entities
        .get(unit)
        .unwrap()
        .unit_firing_counter
        .get()
}

fn set_counter(sim: &mut Simulation, unit: u64, value: i32) {
    sim.substrate
        .entities
        .get_mut(unit)
        .unwrap()
        .unit_firing_counter
        .set_for_test(value);
}

fn set_target(sim: &mut Simulation, rules: &RuleSet, unit: u64, target: u64) {
    sim.assign_target_represented(
        unit,
        Some(TargetKind::Entity(target)),
        rules,
        &mut FrameEffects::default(),
    )
    .expect("the setter admits the target");
}

/// The missing key constructs true: every new target clears an active
/// sequence, even one already in range.
#[test]
fn the_default_true_key_resets_in_range_retargets() {
    let (mut sim, rules, unit, near, _) = scene("", false);
    set_counter(&mut sim, unit, 9);
    set_target(&mut sim, &rules, unit, near);
    assert_eq!(counter(&sim, unit), -1);
}

/// `FireUp.ResetInRetarget=no` keeps the sequence when the new target is in
/// range and clears it when the target is not.
#[test]
fn a_false_key_keeps_only_an_in_range_retarget() {
    let (mut sim, rules, unit, near, far) = scene("FireUp.ResetInRetarget=no", false);
    let raider = rules.object("RAIDER").unwrap();
    assert!(
        !rules
            .art()
            .resolve_metadata_entry(&raider.id, &raider.image)
            .unwrap()
            .phobos_fire_up_reset_in_retarget
    );
    assert!(
        crate::sim::keep_target_on_move::is_close_enough_with_weapon(
            &sim,
            unit,
            &TargetKind::Entity(near),
            &rules,
            None,
            -1
        )
    );
    set_counter(&mut sim, unit, 9);
    set_target(&mut sim, &rules, unit, near);
    assert_eq!(counter(&sim, unit), 9, "in range keeps the sequence");
    set_target(&mut sim, &rules, unit, far);
    assert_eq!(counter(&sim, unit), -1, "out of range clears it");
}

/// The native `!pUnit->Type->Turret` gate: a turreted Unit is untouched.
#[test]
fn a_turreted_unit_keeps_its_sequence() {
    let (mut sim, rules, unit, near, _) = scene("FireUp.ResetInRetarget=no", true);
    set_counter(&mut sim, unit, 9);
    set_target(&mut sim, &rules, unit, near);
    assert_eq!(counter(&sim, unit), 9);
}

/// `Assign_Target`'s same-target early-out (`0x6FCDCA..0x6FCDCC`) returns
/// before the store and the `0x6FCF3E` hook body: re-issuing the current
/// target keeps an active sequence.
#[test]
fn a_repeated_same_target_assignment_keeps_the_sequence() {
    let (mut sim, rules, unit, near, _) = scene("", false);
    set_counter(&mut sim, unit, 9);
    set_target(&mut sim, &rules, unit, near);
    assert_eq!(counter(&sim, unit), -1, "the first changed store resets");
    set_counter(&mut sim, unit, 9);
    set_target(&mut sim, &rules, unit, near);
    assert_eq!(
        counter(&sim, unit),
        9,
        "the same-target assignment is a no-op"
    );
}

/// A stored null takes the hook's `ResetDelayedFireTimer` arm; the firing
/// sequence arm only runs for a stored non-null target.
#[test]
fn a_null_store_leaves_the_sequence_alone() {
    let (mut sim, rules, unit, near, _) = scene("", false);
    set_counter(&mut sim, unit, 9);
    set_target(&mut sim, &rules, unit, near);
    set_counter(&mut sim, unit, 9);
    sim.assign_target_represented(unit, None, &rules, &mut FrameEffects::default())
        .expect("the null setter runs");
    assert_eq!(counter(&sim, unit), 9);
}
