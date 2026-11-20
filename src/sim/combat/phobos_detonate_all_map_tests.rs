//! Phobos `DetonateOnAllMapObjects.*` through the production bullet
//! detonation path (`commit_projectiles` -> `emit_detonation_receivers`).
use super::world_receiver::{ReceiverRun, commit_projectiles};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::ground_pose::object_get_coords;
use crate::sim::projectile::{
    ProjectileCoord, ProjectileDetonation, ProjectileDetonationReason, ProjectilePayload,
    ProjectileTarget,
};
use crate::sim::world::Simulation;

/// FIRER (Americans) with `GUN`/`WH`, an enemy TARGET at (22,20), an enemy
/// BYSTANDER at (30,30) and an allied FRIEND at (24,26). `wh_ini` appends
/// `DetonateOnAllMapObjects.*` keys to `[WH]`.
fn scene(wh_ini: &str) -> (Simulation, RuleSet, u64, u64, u64, u64) {
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[Countries]\n0=Americans\n1=Russians\n\
         [VehicleTypes]\n0=FIRER\n1=TARGET\n2=BYSTANDER\n3=FRIEND\n\
         [WeaponTypes]\n0=GUN\n\
         [GUN]\nDamage=0\nRange=5\nROF=20\nWarhead=WH\n\
         [Warheads]\n0=WH\n\
         [WH]\nCellSpread=0\n{wh_ini}\
         [FIRER]\nStrength=500\nArmor=none\nSpeed=6\nPrimary=GUN\n\
         [TARGET]\nStrength=500\nArmor=none\nSpeed=6\n\
         [BYSTANDER]\nStrength=500\nArmor=none\nSpeed=6\n\
         [FRIEND]\nStrength=500\nArmor=none\nSpeed=6\n"
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
    let bystander = sim
        .spawn_object("BYSTANDER", "Russians", 30, 30, 0, &rules)
        .expect("bystander");
    let friend = sim
        .spawn_object("FRIEND", "Americans", 24, 26, 0, &rules)
        .expect("friend");
    (sim, rules, firer, target, bystander, friend)
}

fn health(sim: &Simulation, id: u64) -> i32 {
    sim.entities().get(id).expect("entity").health.current()
}

fn shoot(sim: &mut Simulation, rules: &RuleSet, source: u64, target: u64, damage: i32) {
    let coords = {
        let target = sim.entities().get(target).expect("target");
        object_get_coords(target, sim.resolved_terrain.as_ref())
    };
    let detonation = ProjectileDetonation {
        projectile_id: target,
        source_id: source,
        target: ProjectileTarget::Entity(target),
        impact: ProjectileCoord::new(coords.x, coords.y, coords.z),
        payload: ProjectilePayload::new(damage, sim.interner.intern("WH"), None, false),
        reason: ProjectileDetonationReason::ReachedTarget,
    };
    let mut run = ReceiverRun::default();
    let _ = commit_projectiles(sim, &mut run, &[detonation], rules, None);
    run.finish();
}

/// `Full=yes` (the constructor default) re-detonates the bullet at every
/// eligible target of the affected class arrays; a house-excluded ally and a
/// class-excluded type are untouched, and the impact target takes the damage
/// exactly once.
#[test]
fn full_detonation_hits_every_eligible_enemy_once() {
    let (mut sim, rules, firer, target, bystander, friend) = scene(
        "DetonateOnAllMapObjects=yes\n\
         DetonateOnAllMapObjects.AffectsTarget=units\n\
         DetonateOnAllMapObjects.AffectsHouse=enemies\n",
    );
    shoot(&mut sim, &rules, firer, target, 20);
    assert_eq!(health(&sim, target), 480, "the impact target is hit once");
    assert_eq!(health(&sim, bystander), 480, "every enemy unit is hit");
    assert_eq!(health(&sim, friend), 500, "the firer's ally is excluded");
}

/// `Full=no` sends the bullet's area damage (`Health * DamageMultiplier >>
/// 8`) to each eligible target instead; `IgnoreTypes` removes one, and
/// `RequireVerses=yes` skips a 0% armor row.
#[test]
fn damage_area_arm_respects_ignore_types_and_verses() {
    let (mut sim, rules, firer, target, bystander, friend) = scene(
        "DetonateOnAllMapObjects=yes\nDetonateOnAllMapObjects.Full=no\n\
         DetonateOnAllMapObjects.AffectsTarget=units\n\
         DetonateOnAllMapObjects.AffectsHouse=enemies\n\
         DetonateOnAllMapObjects.IgnoreTypes=BYSTANDER\n",
    );
    shoot(&mut sim, &rules, firer, target, 20);
    assert_eq!(health(&sim, target), 480);
    assert_eq!(health(&sim, bystander), 500, "IgnoreTypes skips the type");
    assert_eq!(health(&sim, friend), 500);

    // `Armor=none` is Verses slot 0; a 0% row skips every target.
    let (mut sim, rules, firer, target, bystander, friend) = scene(
        "DetonateOnAllMapObjects=yes\nDetonateOnAllMapObjects.Full=no\n\
         DetonateOnAllMapObjects.AffectsTarget=units\n\
         DetonateOnAllMapObjects.AffectsHouse=enemies\n\
         DetonateOnAllMapObjects.RequireVerses=yes\n\
         Verses=0%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    shoot(&mut sim, &rules, firer, target, 20);
    assert_eq!(
        (
            health(&sim, target),
            health(&sim, bystander),
            health(&sim, friend)
        ),
        (500, 500, 500),
        "RequireVerses skips a 0% target"
    );
}

/// The claim gate: a non-`None` `AffectsTarget` with a class that matches no
/// walked array still claims the detonation (nothing is hit, not even the
/// impact target), while `AffectsHouse=none` leaves the ordinary impact.
#[test]
fn the_claim_gate_needs_both_flags_but_an_empty_class_match_still_claims() {
    let (mut sim, rules, firer, target, bystander, friend) = scene(
        "DetonateOnAllMapObjects=yes\n\
         DetonateOnAllMapObjects.AffectsTarget=buildings\n\
         DetonateOnAllMapObjects.AffectsHouse=enemies\n",
    );
    shoot(&mut sim, &rules, firer, target, 20);
    assert_eq!(
        (
            health(&sim, target),
            health(&sim, bystander),
            health(&sim, friend)
        ),
        (500, 500, 500),
        "a claimed detonation with no matching class hits nothing"
    );

    // A valid token set that names no Techno class still claims: the native
    // gate only tests "not None", so the impact itself is suppressed.
    let (mut sim, rules, firer, target, bystander, friend) = scene(
        "DetonateOnAllMapObjects=yes\n\
         DetonateOnAllMapObjects.AffectsTarget=land\n\
         DetonateOnAllMapObjects.AffectsHouse=enemies\n",
    );
    shoot(&mut sim, &rules, firer, target, 20);
    assert_eq!(
        (
            health(&sim, target),
            health(&sim, bystander),
            health(&sim, friend)
        ),
        (500, 500, 500),
        "a land-only token set claims the detonation and walks nothing"
    );

    let (mut sim, rules, firer, target, bystander, friend) = scene(
        "DetonateOnAllMapObjects=yes\n\
         DetonateOnAllMapObjects.AffectsTarget=units\n\
         DetonateOnAllMapObjects.AffectsHouse=none\n",
    );
    shoot(&mut sim, &rules, firer, target, 20);
    assert_eq!(
        health(&sim, target),
        480,
        "AffectsHouse=None disables the pass; the ordinary impact applies"
    );
    assert_eq!((health(&sim, bystander), health(&sim, friend)), (500, 500));
}
