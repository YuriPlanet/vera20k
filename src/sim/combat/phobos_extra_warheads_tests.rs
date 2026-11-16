//! Phobos ExtraWarheads through the production detonation path
//! (`Ext/Bullet/Hooks.DetonateLogics.cpp:367-470`).

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::ground_pose::object_get_coords;
use crate::sim::projectile::{
    ProjectileCoord, ProjectileDetonation, ProjectileDetonationReason, ProjectilePayload,
    ProjectileTarget,
};
use crate::sim::world::Simulation;

/// Two rival houses; `weapon_ini` extends `[GUN]` and `eh2_ini` `[EH2]`.
fn scene(weapon_ini: &str, eh2_ini: &str) -> (Simulation, RuleSet, u64, u64) {
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[Countries]\n0=Americans\n1=Russians\n\
         [VehicleTypes]\n0=FIRER\n1=TARGET\n\
         [WeaponTypes]\n0=GUN\n\
         [GUN]\nDamage=10\nRange=5\nROF=20\nWarhead=WH\n{weapon_ini}\
         [Warheads]\n0=WH\n1=EH1\n2=EH2\n\
         [WH]\nCellSpread=0\n\
         [EH1]\nCellSpread=0\n\
         [EH2]\nCellSpread=0\n{eh2_ini}\
         [FIRER]\nStrength=500\nArmor=none\nSpeed=6\nPrimary=GUN\n\
         [TARGET]\nStrength=500\nArmor=none\nSpeed=6\n"
    )))
    .expect("rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x5A_AA_1F);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let firer = sim
        .spawn_object("FIRER", "Americans", 20, 20, 0, &rules)
        .expect("firer");
    let target = sim
        .spawn_object("TARGET", "Russians", 22, 20, 0, &rules)
        .expect("target");
    (sim, rules, firer, target)
}

/// One `ReachedTarget` detonation of `WH` at `target`, carrying the fired
/// weapon so the extras hook can read its `ExtraWarheads=`.
fn shoot(sim: &mut Simulation, rules: &RuleSet, firer: u64, target: u64, damage: i32) {
    let coords = {
        let target = sim.entities().get(target).expect("target");
        object_get_coords(target, sim.resolved_terrain.as_ref())
    };
    let warhead = sim.interner.intern("WH");
    let weapon = sim.interner.intern("GUN");
    let detonation = ProjectileDetonation {
        projectile_id: u64::MAX,
        source_id: firer,
        target: ProjectileTarget::Entity(target),
        impact: ProjectileCoord::new(coords.x, coords.y, coords.z),
        payload: ProjectilePayload::new(damage, warhead, None, false)
            .with_installed_weapon(Some(weapon)),
        reason: ProjectileDetonationReason::ReachedTarget,
    };
    let mut run = super::world_receiver::ReceiverRun::default();
    super::world_receiver::commit_projectiles(sim, &mut run, &[detonation], rules, None);
    run.finish();
}

fn damage_taken(weapon_ini: &str) -> i32 {
    let (mut sim, rules, firer, target) = scene(weapon_ini, "");
    shoot(&mut sim, &rules, firer, target, 10);
    500 - sim.entities().get(target).unwrap().health.current()
}

/// `ExtraWarheads=` detonates every listed warhead after the weapon's own,
/// in listed order, at the weapon's `Damage` unless overridden
/// (`Hooks.DetonateLogics.cpp:373-427`).
#[test]
fn extra_warheads_fire_after_the_main_warhead_in_listed_order() {
    assert_eq!(
        damage_taken("ExtraWarheads=EH1,EH2\n"),
        30,
        "main 10 + EH1 10 + EH2 10"
    );
    assert_eq!(
        damage_taken("ExtraWarheads=EH1,EH2\nExtraWarheads.DamageOverrides=3\n"),
        16,
        "a shorter override list repeats its last entry for both extras"
    );
    assert_eq!(
        damage_taken("ExtraWarheads=EH1,EH2\nExtraWarheads.DamageOverrides=7,9\n"),
        26,
        "one override per index"
    );
}

/// Phobos' `ValueableVector<WarheadTypeClass*>` resolves through
/// `FindOrAllocate`, so an unknown name keeps its list index as an empty
/// constructor-default warhead (100% `Verses`) and still detonates.
#[test]
fn an_unknown_warhead_name_allocates_an_empty_type() {
    assert_eq!(
        damage_taken("ExtraWarheads=EH1,MISSING\n"),
        30,
        "main 10 + EH1 10 + the allocated empty type's 10"
    );
    assert_eq!(
        damage_taken("ExtraWarheads=EH1,MISSING\nExtraWarheads.DamageOverrides=3,9\n"),
        22,
        "the allocated entry keeps its per-index override slot"
    );
}

/// `ExtraWarheads.FullDetonation=` selects the area-damage arm
/// (`DamageAreaWithTarget`) for the marked index; the default is the full
/// `DetonateAt` arm. Both still damage the intended target here.
#[test]
fn full_detonation_false_takes_the_area_arm() {
    assert_eq!(
        damage_taken("ExtraWarheads=EH2\nExtraWarheads.FullDetonation=no\n"),
        20,
        "main 10 + area EH2 10"
    );
    assert_eq!(
        damage_taken("ExtraWarheads=EH2\nExtraWarheads.FullDetonation=yes\n"),
        20,
        "the full arm applies the same weapon damage"
    );
}

/// `ExtraWarheads.DetonationChances=`: each warhead draws one Scenario
/// `RandomDouble`, and a value below the roll skips it
/// (`Hooks.DetonateLogics.cpp:456-471`).
#[test]
fn detonation_chances_draw_and_can_skip() {
    let (mut sim, rules, firer, target) = scene(
        "ExtraWarheads=EH2\nExtraWarheads.DetonationChances=0.0\n",
        "",
    );
    let mut expected = sim.scenario_rng.clone();
    expected.random_double();
    shoot(&mut sim, &rules, firer, target, 10);
    assert_eq!(
        sim.entities().get(target).unwrap().health.current(),
        490,
        "a zero chance skips the extra warhead"
    );
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected.logical_state(),
        "the skipped roll still drew once"
    );
}

/// `ExtraWarheads.RandomWeights=`: one roll's dice also selects the weighted
/// index; `RollChances` absent means one sure roll
/// (`Hooks.DetonateLogics.cpp:429-455`).
#[test]
fn random_weights_pick_one_index_from_the_single_roll() {
    let (mut sim, rules, firer, target) = scene(
        "ExtraWarheads=EH1,EH2\nExtraWarheads.DamageOverrides=5,9\n\
         ExtraWarheads.RandomWeights=0,100\n",
        "",
    );
    let mut expected = sim.scenario_rng.clone();
    expected.random_double();
    shoot(&mut sim, &rules, firer, target, 10);
    assert_eq!(
        sim.entities().get(target).unwrap().health.current(),
        500 - 10 - 9,
        "weights 0,100 always pick the second warhead"
    );
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected.logical_state(),
        "no chance list means no second draw"
    );
}

/// The per-target gate is only the three trigger thresholds: a warhead with
/// zero Verses against the target still detonates (its damage is zero, but
/// its other effects run). `TransactMoney=50` proves the detonation happened.
/// (`Hooks.DetonateLogics.cpp:392-396` vs `CanAffectTarget`.)
#[test]
fn the_target_gate_has_no_verses_test() {
    let (mut sim, rules, firer, target) = scene(
        "ExtraWarheads=EH2\n",
        "Verses=0%,0%,0%,0%,0%,0%,0%,0%,0%,0%,0%\nTransactMoney=50\n",
    );
    let owner = sim.entities().get(firer).unwrap().owner();
    shoot(&mut sim, &rules, firer, target, 10);
    assert_eq!(
        sim.entities().get(target).unwrap().health.current(),
        490,
        "the zero-Verses extra warhead deals no damage"
    );
    assert_eq!(
        sim.houses.get(&owner).unwrap().economy().available_money(),
        50,
        "the extra warhead still detonated"
    );
}

/// `ExtraWarheads.RollChances` gates the weighted rolls: a `0.0` roll always
/// fails and a `1.0` roll always passes, each drawing once
/// (`Hooks.DetonateLogics.cpp:433-441`).
#[test]
fn roll_chances_skip_one_roll_and_draw_for_both() {
    let (mut sim, rules, firer, target) = scene(
        "ExtraWarheads=EH1,EH2\nExtraWarheads.RandomWeights=1,1\n\
         ExtraWarheads.RollChances=0.0,1.0\n",
        "",
    );
    let mut expected = sim.scenario_rng.clone();
    expected.random_double();
    expected.random_double();
    shoot(&mut sim, &rules, firer, target, 10);
    assert_eq!(
        sim.entities().get(target).unwrap().health.current(),
        500 - 10 - 10,
        "only the second roll detonates one warhead"
    );
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected.logical_state(),
        "both rolls drew"
    );
}
