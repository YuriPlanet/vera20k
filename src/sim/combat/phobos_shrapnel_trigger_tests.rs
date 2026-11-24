//! Phobos `Shrapnel.ObeyWarheadTriggerConditions` (`Ext/BulletType/Body.cpp:45`,
//! global `Ext/Rules/Body.cpp:557`, hook `0x4690D4`,
//! `Ext/Bullet/Hooks.DetonateLogics.cpp:22-29`): a bullet whose type (else the
//! `[CombatDamage]` global) obeys the warhead's trigger conditions skips the
//! whole detonation body for a failing Techno target and lands at the impact
//! extras (`GoToExtras 0x469AA4`) without even the shake draws.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::arena_fixture::rival_arena;
use crate::sim::projectile::{
    ProjectileCoord, ProjectileDetonation, ProjectileDetonationReason, ProjectilePayload,
    ProjectileTarget,
};

/// `DIRECT` is at full health and fails `AffectsBelowPercent=0.5`; `NEAR`
/// sits two cells out at 25% and passes it. CellSpread 3 puts NEAR inside the
/// area. `ShakeXlo/Hi` give the body a Main RNG draw the skip must not take.
fn rules(bullet_keys: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\n[CombatDamage]\n\
         [VehicleTypes]\n0=DIRECT\n1=NEAR\n\
         [DIRECT]\nStrength=1000\nArmor=heavy\nSpeed=6\nPrimary=Gun\n\
         [NEAR]\nStrength=1000\nArmor=heavy\nSpeed=6\nPrimary=Gun\n\
         [WeaponTypes]\n0=Gun\n\
         [Gun]\nDamage=100\nROF=50\nRange=10\nProjectile=Shell\nWarhead=WH\n\
         [Projectiles]\n0=Shell\n\
         [Shell]\n{bullet_keys}\
         [Warheads]\n0=WH\n\
         [WH]\nCellSpread=3\nPercentAtMax=1\nAffectsBelowPercent=0.5\n\
         ShakeXlo=1\nShakeXhi=3\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    )))
    .expect("shrapnel trigger rules")
}

/// Default (global `yes`): the failing direct target sends the whole body —
/// area damage and the shake draw included — to the extras.
#[test]
fn a_failing_trigger_target_skips_the_body_and_its_shake_draw() {
    let rules = rules("");
    let mut sim = rival_arena(&rules, 0x4690D4);
    sim.resolve_type_handles(&rules);
    let direct = sim
        .spawn_object_at_height("DIRECT", "Russians", 8, 10, 0, 0, &rules)
        .unwrap();
    let near = sim
        .spawn_object_at_height("NEAR", "Russians", 10, 10, 0, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(near)
        .unwrap()
        .health
        .set_for_test(250);
    let payload = ProjectilePayload::new(
        100,
        sim.interner.intern("WH"),
        Some(sim.interner.intern("Shell")),
        false,
    )
    .with_installed_weapon(Some(sim.interner.intern("Gun")));
    let main = sim.main_rng.native_state_hex();
    sim.commit_logic_projectile_detonations(
        &rules,
        None,
        &[ProjectileDetonation {
            projectile_id: 1,
            source_id: 0,
            target: ProjectileTarget::Entity(direct),
            impact: ProjectileCoord::new(8 * 256 + 128, 10 * 256 + 128, 0),
            payload,
            reason: ProjectileDetonationReason::ReachedTarget,
        }],
        &mut crate::sim::world::FrameEffects::default(),
    );
    assert_eq!(
        sim.substrate.entities.get(near).unwrap().health.current(),
        250,
        "the area damage is part of the skipped body"
    );
    assert_eq!(sim.main_rng.native_state_hex(), main, "no shake draw");
}

/// `Shrapnel.ObeyWarheadTriggerConditions=no` on the bullet type: the body
/// runs, so the bystander inside CellSpread takes area damage and the shake
/// draws advance Main.
#[test]
fn the_bullet_key_off_runs_the_body_and_its_shake_draw() {
    let rules = rules("Shrapnel.ObeyWarheadTriggerConditions=no\n");
    assert_eq!(
        rules
            .projectile("Shell")
            .expect("Shell projectile type")
            .shrapnel_obey_warhead_trigger_conditions,
        Some(false),
        "the bullet type reads the key"
    );
    let mut sim = rival_arena(&rules, 0x4690D4);
    sim.resolve_type_handles(&rules);
    let direct = sim
        .spawn_object_at_height("DIRECT", "Russians", 8, 10, 0, 0, &rules)
        .unwrap();
    let near = sim
        .spawn_object_at_height("NEAR", "Russians", 10, 10, 0, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(near)
        .unwrap()
        .health
        .set_for_test(250);
    let payload = ProjectilePayload::new(
        100,
        sim.interner.intern("WH"),
        Some(sim.interner.intern("Shell")),
        false,
    )
    .with_installed_weapon(Some(sim.interner.intern("Gun")));
    let main = sim.main_rng.native_state_hex();
    sim.commit_logic_projectile_detonations(
        &rules,
        None,
        &[ProjectileDetonation {
            projectile_id: 1,
            source_id: 0,
            target: ProjectileTarget::Entity(direct),
            impact: ProjectileCoord::new(8 * 256 + 128, 10 * 256 + 128, 0),
            payload,
            reason: ProjectileDetonationReason::ReachedTarget,
        }],
        &mut crate::sim::world::FrameEffects::default(),
    );
    assert!(
        sim.substrate.entities.get(near).unwrap().health.current() < 250,
        "the area damage runs when the key is off"
    );
    assert_ne!(sim.main_rng.native_state_hex(), main, "the shake draws run");
}
