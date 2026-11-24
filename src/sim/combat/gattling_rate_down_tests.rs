//! Regressions for the Phobos `RateDown.*` chain on the gattling state:
//! the delay before a decay starts, the low-ammo cover value, the full reset
//! on a target change and the negative-delay disable.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::{FrameEffects, Simulation};

const STAGES: &str = "IsGattling=yes\nWeaponStages=3\nStage1=200\nStage2=400\n\
                      Stage3=600\nEliteStage1=100\nEliteStage2=200\nEliteStage3=300\n\
                      RateUp=1\nRateDown=4\n";

fn scene(rate_lines: &str) -> (Simulation, RuleSet, u64) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[Countries]\n0=Americans\n[BuildingTypes]\n0=GUN\n1=SHED\n\
         [GUN]\nStrength=1000\nFoundation=1x1\nAmmo=10\n{STAGES}{rate_lines}\n\
         [SHED]\nStrength=1000\nFoundation=1x1\n\
         [Warheads]\n0=WH\n[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [Weapons]\n0=W0\n[W0]\nDamage=1\nROF=10\nRange=5\nWarhead=WH\n"
    ));
    let art = IniFile::from_str("[GUN]\nFoundation=1x1\n[SHED]\nFoundation=1x1\n");
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("rate down rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 3);
    let gun = sim
        .spawn_object_at_height("GUN", "Americans", 10, 10, 0, 0, &rules)
        .expect("gattling building");
    (sim, rules, gun)
}

fn set_state(sim: &mut Simulation, gun: u64, stage: i32, value: i32) {
    sim.substrate.entities.get_mut(gun).unwrap().gattling =
        GattlingState::from_fields(stage, value, false);
}

/// Change only the stage/value, keeping the Phobos rate-down accumulator and
/// last-target state, as a running gattling would.
fn set_value_stage(sim: &mut Simulation, gun: u64, stage: i32, value: i32) {
    let entity = sim.substrate.entities.get_mut(gun).unwrap();
    let mut state = entity.gattling;
    state.set_stage(stage);
    state.set_value(value);
    entity.gattling = state;
}

fn value(sim: &Simulation, gun: u64) -> i32 {
    sim.substrate.entities.get(gun).unwrap().gattling.value()
}

fn tick(sim: &mut Simulation, rules: &RuleSet, gun: u64) {
    sim.gattling_update(gun, rules, 1, &mut FrameEffects::default());
}

/// `RateDown.Delay=3`: the first three decay calls keep the value, the fourth
/// subtracts one tick of `RateDown` (4).
#[test]
fn rate_down_delay_holds_the_value_then_catches_up() {
    let (mut sim, rules, gun) = scene("RateDown.Delay=3\n");
    set_state(&mut sim, gun, 3, 700);
    for _ in 0..3 {
        tick(&mut sim, &rules, gun);
        assert_eq!(value(&sim, gun), 700, "still inside the delay");
    }
    tick(&mut sim, &rules, gun);
    assert_eq!(value(&sim, gun), 696, "the first decay tick lands");
    tick(&mut sim, &rules, gun);
    assert_eq!(value(&sim, gun), 692, "later calls decay every tick");
}

/// `RateDown.Delay=-1` disables the value decay entirely.
#[test]
fn a_negative_rate_down_delay_disables_the_decay() {
    let (mut sim, rules, gun) = scene("RateDown.Delay=-1\n");
    set_state(&mut sim, gun, 3, 700);
    for _ in 0..5 {
        tick(&mut sim, &rules, gun);
    }
    assert_eq!(value(&sim, gun), 700);
}

/// `RateDown.Cover.Value` replaces the type's `RateDown` at or below
/// `RateDown.Cover.AmmoBelow`.
#[test]
fn rate_down_cover_value_applies_at_low_ammo() {
    let (mut sim, rules, gun) = scene("RateDown.Cover.Value=9\nRateDown.Cover.AmmoBelow=1\n");
    sim.substrate
        .entities
        .get_mut(gun)
        .unwrap()
        .ammo_mut()
        .unwrap()
        .set_current_for_test(1);
    set_state(&mut sim, gun, 3, 700);
    tick(&mut sim, &rules, gun);
    assert_eq!(
        value(&sim, gun),
        691,
        "ammo at the threshold uses the cover"
    );
    sim.substrate
        .entities
        .get_mut(gun)
        .unwrap()
        .ammo_mut()
        .unwrap()
        .set_current_for_test(2);
    tick(&mut sim, &rules, gun);
    assert_eq!(value(&sim, gun), 687, "ammo above the threshold is stock");
}

/// `RateDown.Reset=yes`: a new, changed or lost target zeroes the value and
/// stage; re-setting the same target keeps them.
#[test]
fn rate_down_reset_zeroes_on_a_target_change() {
    use crate::sim::combat::TargetKind;
    let (mut sim, rules, gun) = scene("RateDown.Reset=yes\n");
    let first = sim
        .spawn_object_at_height("SHED", "Americans", 14, 10, 0, 0, &rules)
        .unwrap();
    let second = sim
        .spawn_object_at_height("SHED", "Americans", 16, 10, 0, 0, &rules)
        .unwrap();
    let mut effects = FrameEffects::default();
    set_value_stage(&mut sim, gun, 2, 500);
    let _ = sim.assign_target_represented(gun, Some(TargetKind::Entity(first)), &rules, &mut effects);
    assert_eq!(
        (
            sim.substrate.entities.get(gun).unwrap().gattling.stage(),
            value(&sim, gun)
        ),
        (0, 0),
        "a new target resets the gattling"
    );
    set_value_stage(&mut sim, gun, 2, 500);
    let _ = sim.assign_target_represented(gun, Some(TargetKind::Entity(first)), &rules, &mut effects);
    assert_eq!(value(&sim, gun), 500, "the same target does not reset");
    let _ = sim.assign_target_represented(gun, Some(TargetKind::Entity(second)), &rules, &mut effects);
    assert_eq!(value(&sim, gun), 0, "a changed target resets");
    set_value_stage(&mut sim, gun, 2, 500);
    let _ = sim.assign_target_represented(gun, None, &rules, &mut effects);
    assert_eq!(value(&sim, gun), 0, "a lost target resets");
}

/// The `DecreaseValue` hook path (`idle_decay`): the delay holds the idle
/// decay too, then the rate applies to the accumulated ticks.
#[test]
fn rate_down_delay_holds_the_idle_decay() {
    let (sim, rules, gun) = scene("RateDown.Delay=2\n");
    let (settings, table) = {
        let entity = sim.substrate.entities.get(gun).unwrap();
        let obj = sim.object_type(entity.type_ref(), &rules).unwrap();
        (obj.phobos_rate_down, obj.gattling_stages.clone())
    };
    let mut state = GattlingState::from_fields(3, 700, false);
    state.idle_decay_phobos(&table, false, &settings, 10);
    state.idle_decay_phobos(&table, false, &settings, 10);
    assert_eq!(state.value(), 700, "two calls stay inside the delay");
    state.idle_decay_phobos(&table, false, &settings, 10);
    assert_eq!(state.value(), 696, "the third call decays one tick");
}

/// A spin-up restarts the rate-down delay (`0x70DE70`): the next decay
/// stretch waits `RateDown.Delay` again.
#[test]
fn a_spin_up_restarts_the_rate_down_delay() {
    let (mut sim, rules, gun) = scene("RateDown.Delay=3\n");
    set_state(&mut sim, gun, 3, 700);
    tick(&mut sim, &rules, gun);
    tick(&mut sim, &rules, gun);
    // A charge at the cap leaves the value but resets the accumulator.
    sim.gattling_increase(gun, &rules, 1, &mut FrameEffects::default());
    for _ in 0..3 {
        tick(&mut sim, &rules, gun);
        assert_eq!(value(&sim, gun), 700, "the delay restarted");
    }
    tick(&mut sim, &rules, gun);
    assert_eq!(value(&sim, gun), 696);
}
