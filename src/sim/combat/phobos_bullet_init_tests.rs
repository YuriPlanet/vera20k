//! Phobos bullet regressions: the retained FirerHouse, the owner's firepower
//! multiplier, and the vanilla Inviso scatter Phobos skips.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::projectile::{
    ProjectileCoord, ProjectileDetonation, ProjectileDetonationReason, ProjectilePayload,
    ProjectileTarget,
};
use crate::util::native_x87::NativeF64Bits;

fn scene() -> (Simulation, RuleSet, u64, u64, InternedId) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=SOURCE\n[SOURCE]\nPrimary=GUN\n\
         [GUN]\nProjectile=SHOT\nWarhead=WH\nDamage=10\n\
         [Warheads]\n0=WH\n[WH]\n",
    ))
    .unwrap();
    let mut world = Simulation::with_seed(7);
    world.intern_rule_type_ids(&rules);
    world.resolve_type_handles(&rules);
    let source = world
        .spawn_object("SOURCE", "Americans", 5, 5, 0, &rules)
        .unwrap();
    let warhead = world.interner.intern("WH");
    let payload = ProjectilePayload::new(10, warhead, None, false);
    let bullet = world.construct_projectile(source, ProjectileTarget::None, payload, &rules);
    (world, rules, source, bullet, warhead)
}

#[test]
fn bullet_retains_its_firers_house_after_the_owner_expires() {
    let (mut world, _rules, source, bullet, warhead) = scene();
    let americans = world.interner.intern("Americans");
    assert_eq!(
        world.projectiles.get(bullet).unwrap().phobos_firer_house(),
        Some(americans)
    );
    let detonation = ProjectileDetonation {
        projectile_id: bullet,
        source_id: source,
        target: ProjectileTarget::None,
        impact: ProjectileCoord::new(0, 0, 0),
        payload: ProjectilePayload::new(10, warhead, None, false),
        reason: ProjectileDetonationReason::ReachedTarget,
    };
    assert_eq!(
        detonation_source_house(&world, &detonation),
        Some(americans)
    );

    world.substrate.entities.remove(source);
    assert_eq!(
        detonation_source_house(&world, &detonation),
        Some(americans),
        "the retained House stands in for the expired owner"
    );
}

/// A simulated detonation has no projectile to read `FirerHouse` from, so
/// `BulletExt::Detonate`'s `pFiringHouse` rides on its payload instead.
#[test]
fn simulated_detonation_keeps_the_shots_firing_house() {
    let (mut world, _rules, source, _bullet, warhead) = scene();
    let americans = world.interner.intern("Americans");
    world.substrate.entities.remove(source);
    let detonation = ProjectileDetonation {
        projectile_id: u64::MAX,
        source_id: source,
        target: ProjectileTarget::None,
        impact: ProjectileCoord::new(0, 0, 0),
        payload: ProjectilePayload::new(10, warhead, None, false)
            .with_source_house(Some(americans)),
        reason: ProjectileDetonationReason::AttachEffectShot,
    };
    assert_eq!(
        detonation_source_house(&world, &detonation),
        Some(americans),
        "the retained firing House stands in for the expired invoker"
    );
}

#[test]
fn reconstruct_without_a_live_owner_keeps_the_saved_house() {
    let (mut world, rules, source, bullet, _) = scene();
    let americans = world.interner.intern("Americans");
    world.substrate.entities.remove(source);
    Simulation::construct_bullet_scheme(
        &world.interner,
        &world.substrate,
        &world.houses,
        &mut world.projectiles,
        bullet,
        &rules,
    );
    assert_eq!(
        world.projectiles.get(bullet).unwrap().phobos_firer_house(),
        Some(americans),
        "InitializeState writes only when the Owner exists"
    );
}

#[test]
fn bullet_init_captures_the_owner_firepower_multiplier() {
    let (mut world, rules, source, _, warhead) = scene();
    let two = NativeF64Bits::from_bits(2.0_f64.to_bits());
    world
        .substrate
        .entities
        .get_mut(source)
        .unwrap()
        .crate_multipliers
        .set_firepower_multiplier_for_test(two);
    let bullet = world.construct_projectile(
        source,
        ProjectileTarget::None,
        ProjectilePayload::new(10, warhead, None, false),
        &rules,
    );
    assert_eq!(
        world
            .projectiles
            .get(bullet)
            .unwrap()
            .payload()
            .firepower_multiplier(),
        two,
        "BulletClass_Init captures TechnoExt::GetCurrentFirepowerMultiplier"
    );

    // FireAt's launch capture writes the payload first and is retained.
    let launch = NativeF64Bits::from_bits(1.5_f64.to_bits());
    let bullet = world.construct_projectile(
        source,
        ProjectileTarget::None,
        ProjectilePayload::new(10, warhead, None, false).with_firepower_multiplier(launch),
        &rules,
    );
    assert_eq!(
        world
            .projectiles
            .get(bullet)
            .unwrap()
            .payload()
            .firepower_multiplier(),
        launch
    );
}

/// Phobos `DEFINE_JUMP(LJMP, 0x469AC1, 0x469AF0)` removes the vanilla Inviso
/// scatter: a detonation that selects no animation draws nothing.
#[test]
fn an_inviso_detonation_takes_no_vanilla_scatter_draw() {
    let (mut world, rules, source, bullet, warhead) = scene();
    let detonation = ProjectileDetonation {
        projectile_id: bullet,
        source_id: source,
        target: ProjectileTarget::None,
        impact: ProjectileCoord::new(5 * 256 + 128, 5 * 256 + 128, 0),
        payload: ProjectilePayload::new(10, warhead, None, false),
        reason: ProjectileDetonationReason::ReachedTarget,
    };
    let before = world.scenario_rng.clone();
    emit_detonation_anim(
        &mut world,
        &rules,
        &detonation,
        rules.warhead("WH").unwrap(),
        true,
        false,
        None,
        false,
        &mut CombatEmit::default(),
    );
    assert_eq!(world.scenario_rng.state(), before.state());
}
