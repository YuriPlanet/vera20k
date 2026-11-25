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
        .seldom
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

/// Phobos `ObjectClass_Unlimbo_Parachuted_Wrapper` (`Ext/Bullet/Hooks.cpp:
/// 573-583`) with `BulletClass_AI_Parabomb`'s terminal-rate resolution
/// (`:130-131`): a `Parachuted=yes` type starts its accumulated rate at 0
/// (`Ext/Bullet/Body.h:61`) and takes `Parachuted.MaxFallRate=`, else
/// `[General] ParachuteMaxFallRate`.
#[test]
fn a_parachuted_bullet_type_starts_its_descent_at_the_general_cap() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nParachuteMaxFallRate=-4\n\
         [VehicleTypes]\n0=SOURCE\n[SOURCE]\nPrimary=GUN\n\
         [GUN]\nProjectile=SHOT\nWarhead=WH\nDamage=10\n\
         [Warheads]\n0=WH\n[WH]\n\
         [SHOT]\nParachuted=yes\nParachuted.FallRate=2\n",
    ))
    .unwrap();
    let mut world = Simulation::with_seed(7);
    world.intern_rule_type_ids(&rules);
    world.resolve_type_handles(&rules);
    let source = world
        .spawn_object("SOURCE", "Americans", 5, 5, 0, &rules)
        .unwrap();
    let kind = world.interner.intern("SHOT");
    let payload = ProjectilePayload::new(10, world.interner.intern("WH"), Some(kind), false);
    let bullet = world.construct_projectile(source, ProjectileTarget::None, payload, &rules);
    assert_eq!(
        world.projectiles.get(bullet).unwrap().parabomb(),
        None,
        "a bullet is unlimboed before its type is read"
    );

    world.init_parabomb(bullet, &rules);
    let state = world
        .projectiles
        .get(bullet)
        .unwrap()
        .parabomb()
        .expect("Parachuted=yes starts the descent");
    assert_eq!(state.accumulated_rate, 0);
    assert_eq!(state.fall_rate_step, 2);
    assert_eq!(
        state.max_fall_rate, -4,
        "an unset Parachuted.MaxFallRate= keeps [General] ParachuteMaxFallRate"
    );
}

/// The same source/gun scene as the cap test, with the Phobos parabomb keys
/// on `[SHOT]` (`Ext/BulletType/Body.cpp:79-82`).
fn parabomb_scene(bullet_keys: &str) -> (Simulation, RuleSet, u64, u64) {
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\nParachuteMaxFallRate=-4\n\
         [VehicleTypes]\n0=SOURCE\n[SOURCE]\nPrimary=GUN\n\
         [GUN]\nProjectile=SHOT\nWarhead=WH\nDamage=10\n\
         [Warheads]\n0=WH\n[WH]\n\
         [SHOT]\n{bullet_keys}"
    )))
    .unwrap();
    let mut world = Simulation::with_seed(7);
    world.intern_rule_type_ids(&rules);
    world.resolve_type_handles(&rules);
    let source = world
        .spawn_object("SOURCE", "Americans", 5, 5, 0, &rules)
        .unwrap();
    let kind = world.interner.intern("SHOT");
    let payload = ProjectilePayload::new(10, world.interner.intern("WH"), Some(kind), false);
    let bullet = world.construct_projectile(source, ProjectileTarget::None, payload, &rules);
    (world, rules, source, bullet)
}

fn chute_art() -> crate::rules::art_data::ArtRegistry {
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
        "[CHUTE]\nRate=900\nLoopStart=0\nLoopEnd=1\nLoopCount=-1\n",
    ));
    art.bind_anim_frame_count_for_test("CHUTE", 6);
    art
}

/// Phobos `ObjectClass_SpawnParachuted_BombParachute` (`Ext/Bullet/Hooks.cpp:
/// 585-608`, hook `0x5F5A62`): a `Parachuted=yes` bullet constructs its
/// `BombParachute=` AnimType at the spawn coordinate and remaps it to the
/// firing House.
#[test]
fn a_parachuted_bullet_spawns_its_bomb_parachute_canopy() {
    let (mut world, mut rules, source, bullet) =
        parabomb_scene("Parachuted=yes\nParachuted.FallRate=2\nBombParachute=CHUTE\n");
    rules.install_art_fixture(chute_art());
    let origin = world.projectiles.get(bullet).unwrap().launch_origin();

    world.init_parabomb(bullet, &rules);

    assert_eq!(world.anims().count(), 1, "BombParachute= builds one canopy");
    let anim = world.anims().next().unwrap().1;
    assert_eq!(world.interner.resolve(anim.type_id()), "CHUTE");
    assert_eq!(anim.draw_flags(), 0x600);
    assert_eq!(
        anim.world_coord(),
        crate::sim::anim_class::AnimWorldCoord {
            x: origin.x,
            y: origin.y,
            z: origin.z,
        }
    );
    assert_eq!(
        anim.remap(),
        Some(crate::sim::anim_class::AnimRemap::House(
            world.substrate.entities.get(source).unwrap().owner()
        )),
        "the canopy takes the firing House's remap"
    );
}

/// `Parachuted=` is the wrapper's admission gate (`Ext/Bullet/Hooks.cpp:
/// 577-580`); without it `BulletClass::Unlimbo` runs unchanged, so the bullet
/// keeps its ordinary trajectory and no `BombParachute=` anim is built.
#[test]
fn an_unset_parachuted_type_keeps_the_straight_trajectory() {
    let (mut world, mut rules, _source, bullet) =
        parabomb_scene("Parachuted.FallRate=2\nBombParachute=CHUTE\n");
    rules.install_art_fixture(chute_art());

    world.init_parabomb(bullet, &rules);

    assert_eq!(
        world.projectiles.get(bullet).unwrap().parabomb(),
        None,
        "an unset Parachuted= never starts the descent"
    );
    assert_eq!(world.anims().count(), 0, "no canopy without Parachuted=yes");
}
