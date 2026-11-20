//! Interceptor tests through the production scan, weapon selector, warhead
//! head and bullet AI owners.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::TargetKind;
use crate::sim::projectile::{
    ProjectileCollisionPolicy, ProjectileCoord, ProjectileDetonationReason, ProjectilePayload,
    ProjectileSpawn, ProjectileTarget, ProjectileTrajectory, ProjectileVelocity,
    ProjectileVisualState, TargetExpiryPolicy, intercepted_status,
};

const RULES: &str = "[VehicleTypes]\n0=INT\n1=ENEMY\n2=ALLY\n\
    [Projectiles]\n0=IBULLET\n1=OBULLET\n2=ABULLET\n3=TBULLET\n\
    [INT]\nStrength=100\nPrimary=IGUN\nSecondary=SGUN\n\
    [ENEMY]\nStrength=100\nPrimary=TGUN\nSecondary=AGUN\n\
    [ALLY]\nStrength=100\nPrimary=TGUN\nSecondary=OGUN\n\
    [Weapons]\n0=IGUN\n1=SGUN\n2=TGUN\n3=OGUN\n4=AGUN\n\
    [IGUN]\nDamage=40\nRange=8\nWarhead=IWH\nProjectile=IBULLET\n\
    [SGUN]\nDamage=5\nRange=8\nWarhead=IWH\nProjectile=IBULLET\n\
    [TGUN]\nDamage=5\nRange=8\nWarhead=IWH\nProjectile=TBULLET\n\
    [AGUN]\nDamage=5\nRange=8\nWarhead=IWH\nProjectile=ABULLET\n\
    [OGUN]\nDamage=30\nRange=8\nWarhead=EWH\nProjectile=OBULLET\nSpeed=100\nBright=yes\n\
    [Warheads]\n0=IWH\n1=EWH\n2=ZWH\n3=RWH\n\
    [IWH]\nVerses=100%,100%,100%,50%,50%,50%,100%,100%,100%,0%,0%\n\
    [EWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
    [ZWH]\nVerses=0%,0%,0%,0%,0%,0%,0%,0%,0%,0%,0%\n\
    [RWH]\nAffectsAbovePercent=0.9\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
    [IBULLET]\nInviso=yes\n\
    [OBULLET]\nInterceptable=yes\n\
    [ABULLET]\nInterceptable=yes\n\
    [TBULLET]\nInterceptable=yes\nArmor=heavy\nStrength=100\n";

fn scene(keys: &str) -> (Simulation, RuleSet) {
    let text = RULES.replace(
        "[INT]\nStrength=100\n",
        &format!("[INT]\nStrength=100\n{keys}\n"),
    );
    let ini = IniFile::from_str(&text);
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &IniFile::empty())
        .expect("interceptor rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x1e5c3a70);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session.set_binary_frame_for_test(0);
    (sim, rules)
}

fn unit(sim: &mut Simulation, rules: &RuleSet, type_name: &str, owner: &str, rx: u16) -> u64 {
    sim.spawn_object(type_name, owner, rx, 10, 0, rules)
        .expect("unit spawns")
}

#[allow(clippy::too_many_arguments)]
fn spawn_bullet(
    sim: &mut Simulation,
    rules: &RuleSet,
    source: u64,
    type_name: &str,
    at: [i32; 3],
) -> u64 {
    let type_id = sim.interner.intern(type_name);
    let warhead = sim.interner.intern("IWH");
    let id = sim.substrate.allocate_stable_id();
    sim.projectiles.spawn(
        id,
        ProjectileSpawn {
            native_unique_id: 0,
            line_trail: None,
            flat: false,
            source_id: source,
            origin: ProjectileCoord::new(at[0], at[1], at[2]),
            target: ProjectileTarget::None,
            initial_target_position: ProjectileCoord::new(at[0] + 64, at[1], at[2]),
            payload: ProjectilePayload::new(10, warhead, Some(type_id), false),
            speed_leptons_per_frame: 64,
            velocity: ProjectileVelocity::new(0, 0, 0),
            trajectory: ProjectileTrajectory::Straight,
            guidance: None,
            visual: ProjectileVisualState::new(0, 0, 0),
            arm_frames: 0,
            fuse_frames: None,
            ranged_fuse: false,
            tracks_target: false,
            target_expiry: TargetExpiryPolicy::Expire,
            collision: ProjectileCollisionPolicy::NONE,
        },
    );
    let strength = rules.projectile(type_name).map_or(0, |kind| kind.strength);
    sim.projectiles.set_phobos_current_strength(id, strength);
    if let Some(house) = sim
        .substrate
        .entities
        .get(source)
        .map(|entity| entity.owner())
    {
        sim.projectiles.set_phobos_firer_house(id, Some(house));
    }
    id
}

fn scan(sim: &mut Simulation, rules: &RuleSet, interceptor: u64) {
    sim.apply_interceptor(interceptor, rules, None, FrameEffects::empty());
}

fn intercept(sim: &mut Simulation, rules: &RuleSet, interceptor: u64, warhead: &str) {
    let detonation = {
        let bullet = sim
            .projectiles
            .get(interceptor)
            .expect("interceptor bullet");
        crate::sim::projectile::detonation(
            bullet,
            bullet.position(),
            ProjectileDetonationReason::ReachedTarget,
        )
    };
    let warhead = rules.warhead(warhead).expect("test warhead");
    sim.intercept_bullets(rules, None, &detonation, warhead, FrameEffects::empty());
}

fn make_interceptor_bullet(
    sim: &mut Simulation,
    rules: &RuleSet,
    interceptor: u64,
    target: u64,
    at: [i32; 3],
) -> u64 {
    let bullet = spawn_bullet(sim, rules, interceptor, "IBULLET", at);
    // The interceptor bullet's Health is its firing weapon's damage (IGUN).
    if let Some(projectile) = sim.projectiles.get_mut(bullet) {
        projectile.payload_mut().base_damage = 40;
    }
    sim.projectiles
        .set_phobos_interceptor_type(bullet, Some(sim.interner.intern("INT")));
    sim.projectiles
        .set_phobos_interceptor_target(bullet, Some(target));
    bullet
}

/// `0x4690D4` runs before the interceptor head `0x46920B`: a detonation whose
/// warhead trigger conditions refuse the Techno target must not intercept a
/// nearby prey, while the same shot against an admitted target does.
#[test]
fn a_trigger_refusing_target_skips_the_interceptor_head() {
    use crate::sim::combat::world_receiver::{ReceiverRun, commit_projectiles};

    let (mut sim, rules) = scene("Interceptor=yes\n");
    let interceptor = unit(&mut sim, &rules, "INT", "Americans", 10);
    let enemy = unit(&mut sim, &rules, "ENEMY", "Russians", 20);
    sim.substrate
        .entities
        .get_mut(enemy)
        .unwrap()
        .health
        .set_for_test(50);
    let prey = spawn_bullet(
        &mut sim,
        &rules,
        enemy,
        "ABULLET",
        [10 * 256 + 128, 10 * 256 + 128, 0],
    );
    let bullet = make_interceptor_bullet(
        &mut sim,
        &rules,
        interceptor,
        prey,
        [10 * 256 + 128, 10 * 256 + 128, 0],
    );
    let warhead = sim.interner.intern("RWH");
    let projectile_type = sim.interner.intern("IBULLET");
    sim.projectiles
        .set_target(bullet, ProjectileTarget::Entity(enemy));
    sim.projectiles
        .get_mut(bullet)
        .unwrap()
        .payload_mut()
        .warhead = warhead;
    let position = sim.projectiles.get(bullet).unwrap().position();
    let detonation = crate::sim::projectile::ProjectileDetonation {
        projectile_id: bullet,
        source_id: interceptor,
        target: ProjectileTarget::Entity(enemy),
        impact: position,
        payload: ProjectilePayload::new(40, warhead, Some(projectile_type), false),
        reason: ProjectileDetonationReason::ReachedTarget,
    };
    let mut run = ReceiverRun::default();
    commit_projectiles(&mut sim, &mut run, &[detonation], &rules, None);
    run.finish();
    assert_eq!(
        sim.projectiles.get(prey).unwrap().intercepted_status() & intercepted_status::INTERCEPTED,
        0,
        "the refused target skips interception"
    );

    // Control: the same shot at an admitted (full-health) target intercepts.
    let (mut sim, rules) = scene("Interceptor=yes\n");
    let interceptor = unit(&mut sim, &rules, "INT", "Americans", 10);
    let enemy = unit(&mut sim, &rules, "ENEMY", "Russians", 20);
    let prey = spawn_bullet(
        &mut sim,
        &rules,
        enemy,
        "ABULLET",
        [10 * 256 + 128, 10 * 256 + 128, 0],
    );
    let bullet = make_interceptor_bullet(
        &mut sim,
        &rules,
        interceptor,
        prey,
        [10 * 256 + 128, 10 * 256 + 128, 0],
    );
    let warhead = sim.interner.intern("RWH");
    let projectile_type = sim.interner.intern("IBULLET");
    sim.projectiles
        .set_target(bullet, ProjectileTarget::Entity(enemy));
    sim.projectiles
        .get_mut(bullet)
        .unwrap()
        .payload_mut()
        .warhead = warhead;
    let position = sim.projectiles.get(bullet).unwrap().position();
    let detonation = crate::sim::projectile::ProjectileDetonation {
        projectile_id: bullet,
        source_id: interceptor,
        target: ProjectileTarget::Entity(enemy),
        impact: position,
        payload: ProjectilePayload::new(40, warhead, Some(projectile_type), false),
        reason: ProjectileDetonationReason::ReachedTarget,
    };
    let mut run = ReceiverRun::default();
    commit_projectiles(&mut sim, &mut run, &[detonation], &rules, None);
    run.finish();
    assert_ne!(
        sim.projectiles.get(prey).unwrap().intercepted_status() & intercepted_status::INTERCEPTED,
        0,
        "an admitted target still intercepts"
    );
}

#[test]
fn a_bullet_target_selects_the_interceptor_weapon_slot() {
    use crate::sim::combat::combat_weapon;
    let (mut sim, rules) = scene("Interceptor=yes\nInterceptor.Weapon=1\n");
    let interceptor = unit(&mut sim, &rules, "INT", "Americans", 10);
    let enemy = unit(&mut sim, &rules, "ENEMY", "Russians", 20);
    let bullet = spawn_bullet(
        &mut sim,
        &rules,
        enemy,
        "ABULLET",
        [12 * 256 + 128, 10 * 256 + 128, 0],
    );
    let entity = sim.substrate.entities.get(interceptor).unwrap();
    let obj = sim.object_type(entity.type_ref(), &rules).unwrap();
    let attacker = combat_weapon::attacker_facts(entity, obj);
    let select = |target| {
        combat_weapon::select_weapon_against(
            &rules,
            entity,
            obj,
            &attacker,
            Some(&target),
            &sim,
            &sim.interner,
            sim.resolved_terrain.as_ref(),
            Some(&sim.house_alliances),
        )
    };
    let (index, selected) = select(TargetKind::Bullet(bullet)).expect("bullet target selects");
    assert_eq!(index, 1);
    assert_eq!(selected.unwrap().weapon_id, "SGUN");
    let (normal, selected) = select(TargetKind::Entity(enemy)).expect("normal target selects");
    assert_eq!(normal, 0, "a normal target keeps the stock ladder");
    assert_eq!(selected.unwrap().weapon_id, "IGUN");
}

#[test]
fn the_scan_targets_an_enemy_bullet_inside_the_guard_range() {
    let (mut sim, rules) = scene("Interceptor=yes\nInterceptor.GuardRange=4\n");
    let interceptor = unit(&mut sim, &rules, "INT", "Americans", 10);
    let enemy = unit(&mut sim, &rules, "ENEMY", "Russians", 20);
    let inside = spawn_bullet(
        &mut sim,
        &rules,
        enemy,
        "ABULLET",
        [12 * 256 + 128, 10 * 256 + 128, 0],
    );
    scan(&mut sim, &rules, interceptor);
    assert_eq!(
        sim.substrate
            .entities
            .get(interceptor)
            .unwrap()
            .attack_target
            .target(),
        Some(TargetKind::Bullet(inside))
    );

    let (mut sim, rules) = scene("Interceptor=yes\nInterceptor.GuardRange=4\n");
    let interceptor = unit(&mut sim, &rules, "INT", "Americans", 10);
    let enemy = unit(&mut sim, &rules, "ENEMY", "Russians", 20);
    spawn_bullet(
        &mut sim,
        &rules,
        enemy,
        "ABULLET",
        [16 * 256 + 128, 10 * 256 + 128, 0],
    );
    scan(&mut sim, &rules, interceptor);
    assert!(
        sim.substrate
            .entities
            .get(interceptor)
            .unwrap()
            .attack_target
            .target()
            .is_none(),
        "a bullet outside GuardRange is ignored"
    );

    let (mut sim, rules) =
        scene("Interceptor=yes\nInterceptor.GuardRange=4\nInterceptor.MinimumGuardRange=3\n");
    let interceptor = unit(&mut sim, &rules, "INT", "Americans", 10);
    let enemy = unit(&mut sim, &rules, "ENEMY", "Russians", 20);
    spawn_bullet(
        &mut sim,
        &rules,
        enemy,
        "ABULLET",
        [12 * 256 + 128, 10 * 256 + 128, 0],
    );
    scan(&mut sim, &rules, interceptor);
    assert!(
        sim.substrate
            .entities
            .get(interceptor)
            .unwrap()
            .attack_target
            .target()
            .is_none(),
        "a bullet inside MinimumGuardRange is ignored"
    );
}

#[test]
fn cylindrical_guard_range_ignores_height() {
    let high = [11 * 256 + 128, 10 * 256 + 128, 2000];
    let (mut sim, rules) = scene("Interceptor=yes\nInterceptor.GuardRange=4\n");
    let interceptor = unit(&mut sim, &rules, "INT", "Americans", 10);
    let enemy = unit(&mut sim, &rules, "ENEMY", "Russians", 20);
    spawn_bullet(&mut sim, &rules, enemy, "ABULLET", high);
    scan(&mut sim, &rules, interceptor);
    assert!(
        sim.substrate
            .entities
            .get(interceptor)
            .unwrap()
            .attack_target
            .target()
            .is_none(),
        "the spherical range rejects the high bullet"
    );

    let (mut sim, rules) = scene(
        "Interceptor=yes\nInterceptor.GuardRange=4\nInterceptor.GuardRange.IsCylindrical=yes\n",
    );
    let interceptor = unit(&mut sim, &rules, "INT", "Americans", 10);
    let enemy = unit(&mut sim, &rules, "ENEMY", "Russians", 20);
    let high_id = spawn_bullet(&mut sim, &rules, enemy, "ABULLET", high);
    scan(&mut sim, &rules, interceptor);
    assert_eq!(
        sim.substrate
            .entities
            .get(interceptor)
            .unwrap()
            .attack_target
            .target(),
        Some(TargetKind::Bullet(high_id)),
        "IsCylindrical drops the Z difference"
    );
}

#[test]
fn can_target_houses_filters_the_bullet_firer() {
    let (mut sim, rules) = scene("Interceptor=yes\nInterceptor.GuardRange=4\n");
    let interceptor = unit(&mut sim, &rules, "INT", "Americans", 10);
    let ally = unit(&mut sim, &rules, "ALLY", "Americans", 9);
    spawn_bullet(
        &mut sim,
        &rules,
        ally,
        "ABULLET",
        [12 * 256 + 128, 10 * 256 + 128, 0],
    );
    scan(&mut sim, &rules, interceptor);
    assert!(
        sim.substrate
            .entities
            .get(interceptor)
            .unwrap()
            .attack_target
            .target()
            .is_none(),
        "an allied bullet is not an enemy"
    );

    let (mut sim, rules) =
        scene("Interceptor=yes\nInterceptor.GuardRange=4\nInterceptor.CanTargetHouses=all\n");
    let interceptor = unit(&mut sim, &rules, "INT", "Americans", 10);
    let ally = unit(&mut sim, &rules, "ALLY", "Americans", 9);
    let allied_bullet = spawn_bullet(
        &mut sim,
        &rules,
        ally,
        "ABULLET",
        [12 * 256 + 128, 10 * 256 + 128, 0],
    );
    scan(&mut sim, &rules, interceptor);
    assert_eq!(
        sim.substrate
            .entities
            .get(interceptor)
            .unwrap()
            .attack_target
            .target(),
        Some(TargetKind::Bullet(allied_bullet)),
        "CanTargetHouses=all admits it"
    );
}

#[test]
fn an_armorless_bullet_is_intercepted_unless_keep_intact() {
    let at = [12 * 256 + 128, 10 * 256 + 128, 0];
    for (keys, expect) in [
        ("Interceptor=yes\n", true),
        ("Interceptor=yes\nInterceptor.KeepIntact=yes\n", false),
    ] {
        let (mut sim, rules) = scene(keys);
        let interceptor = unit(&mut sim, &rules, "INT", "Americans", 10);
        let enemy = unit(&mut sim, &rules, "ENEMY", "Russians", 20);
        let target = spawn_bullet(&mut sim, &rules, enemy, "ABULLET", at);
        let bullet = make_interceptor_bullet(&mut sim, &rules, interceptor, target, at);
        intercept(&mut sim, &rules, bullet, "IWH");
        let status = sim.projectiles.get(target).unwrap().intercepted_status();
        assert_eq!(
            status & intercepted_status::INTERCEPTED != 0,
            expect,
            "{keys:?}"
        );
        assert!(
            sim.projectiles
                .get(target)
                .unwrap()
                .detonate_on_interception(),
            "the bullet type's DeleteOnIntercept defaults to no"
        );
    }
}

#[test]
fn armor_and_strength_take_repeated_hits_until_depleted() {
    let at = [12 * 256 + 128, 10 * 256 + 128, 0];
    let (mut sim, rules) = scene("Interceptor=yes\n");
    let interceptor = unit(&mut sim, &rules, "INT", "Americans", 10);
    let enemy = unit(&mut sim, &rules, "ENEMY", "Russians", 20);
    let target = spawn_bullet(&mut sim, &rules, enemy, "TBULLET", at);
    let bullet = make_interceptor_bullet(&mut sim, &rules, interceptor, target, at);
    // IWH versus heavy is 50%; the interceptor bullet's Health is its
    // payload damage (40), so each hit removes 20 of 100.
    for _ in 0..4 {
        intercept(&mut sim, &rules, bullet, "IWH");
    }
    assert_eq!(sim.projectiles.get(target).unwrap().current_strength(), 20);
    assert_eq!(
        sim.projectiles.get(target).unwrap().intercepted_status() & intercepted_status::INTERCEPTED,
        0
    );
    intercept(&mut sim, &rules, bullet, "IWH");
    assert_eq!(sim.projectiles.get(target).unwrap().current_strength(), 0);
    assert_ne!(
        sim.projectiles.get(target).unwrap().intercepted_status() & intercepted_status::INTERCEPTED,
        0
    );

    // A warhead with zero versus leaves the bullet untouched.
    let (mut sim, rules) = scene("Interceptor=yes\n");
    let interceptor = unit(&mut sim, &rules, "INT", "Americans", 10);
    let enemy = unit(&mut sim, &rules, "ENEMY", "Russians", 20);
    let target = spawn_bullet(&mut sim, &rules, enemy, "TBULLET", at);
    let bullet = make_interceptor_bullet(&mut sim, &rules, interceptor, target, at);
    intercept(&mut sim, &rules, bullet, "ZWH");
    assert_eq!(sim.projectiles.get(target).unwrap().current_strength(), 100);
    assert_eq!(
        sim.projectiles.get(target).unwrap().intercepted_status() & intercepted_status::INTERCEPTED,
        0
    );
}

#[test]
fn delete_on_intercept_falls_back_from_the_interceptor_to_the_bullet() {
    let at = [12 * 256 + 128, 10 * 256 + 128, 0];
    for (keys, bullet_keys, expect) in [
        ("Interceptor=yes\n", "", true),
        (
            "Interceptor=yes\nInterceptor.DeleteOnIntercept=no\n",
            "",
            true,
        ),
        (
            "Interceptor=yes\n",
            "Interceptable.DeleteOnIntercept=yes\n",
            false,
        ),
        (
            "Interceptor=yes\nInterceptor.DeleteOnIntercept=no\n",
            "Interceptable.DeleteOnIntercept=yes\n",
            true,
        ),
    ] {
        let text = RULES.replace(
            "[ABULLET]\nInterceptable=yes\n",
            &format!("[ABULLET]\nInterceptable=yes\n{bullet_keys}"),
        );
        let ini = IniFile::from_str(&text.replace(
            "[INT]\nStrength=100\n",
            &format!("[INT]\nStrength=100\n{keys}\n"),
        ));
        let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &IniFile::empty()).unwrap();
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x1e5c3a70);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.session.set_binary_frame_for_test(0);
        let interceptor = unit(&mut sim, &rules, "INT", "Americans", 10);
        let enemy = unit(&mut sim, &rules, "ENEMY", "Russians", 20);
        let target = spawn_bullet(&mut sim, &rules, enemy, "ABULLET", at);
        let bullet = make_interceptor_bullet(&mut sim, &rules, interceptor, target, at);
        intercept(&mut sim, &rules, bullet, "IWH");
        assert_eq!(
            sim.projectiles
                .get(target)
                .unwrap()
                .detonate_on_interception(),
            expect,
            "{keys:?} / {bullet_keys:?}"
        );
    }
}

#[test]
fn weapon_override_rearms_the_bullet_and_can_replace_its_projectile() {
    let at = [12 * 256 + 128, 10 * 256 + 128, 0];
    let (mut sim, rules) = scene(
        "Interceptor=yes\nInterceptor.WeaponOverride=OGUN\n\
         Interceptor.WeaponCumulativeDamage=yes\n\
         Interceptor.WeaponReplaceProjectile=yes\n",
    );
    let interceptor = unit(&mut sim, &rules, "INT", "Americans", 10);
    let enemy = unit(&mut sim, &rules, "ENEMY", "Russians", 20);
    let target = spawn_bullet(&mut sim, &rules, enemy, "ABULLET", at);
    let bullet = make_interceptor_bullet(&mut sim, &rules, interceptor, target, at);
    intercept(&mut sim, &rules, bullet, "IWH");
    let target_bullet = sim.projectiles.get(target).unwrap();
    assert_eq!(
        target_bullet.payload().base_damage,
        40,
        "10 carried + 30 override"
    );
    assert_eq!(
        target_bullet.payload().installed_weapon(),
        Some(sim.interner.intern("OGUN"))
    );
    assert_eq!(target_bullet.payload().warhead, sim.interner.intern("EWH"));
    assert!(target_bullet.payload().bright());
    assert_eq!(
        target_bullet.payload().projectile_type(),
        Some(sim.interner.intern("OBULLET"))
    );
    assert_eq!(
        target_bullet.speed_leptons_per_frame(),
        rules.weapon("OGUN").unwrap().speed as u16,
        "the replaced projectile takes the override weapon's stored Speed"
    );
}

#[test]
fn fire_at_marks_the_interceptor_bullet_and_applies_firepower_mult() {
    use crate::sim::combat::world_fire::FireVisit;
    let at = [12 * 256 + 128, 10 * 256 + 128, 0];
    for (keys, expect_damage) in [
        ("Interceptor=yes\n", 80),
        ("Interceptor=yes\nInterceptor.ApplyFirepowerMult=no\n", 40),
    ] {
        let (mut sim, rules) = scene(keys);
        let interceptor = unit(&mut sim, &rules, "INT", "Americans", 10);
        let enemy = unit(&mut sim, &rules, "ENEMY", "Russians", 20);
        let target = spawn_bullet(&mut sim, &rules, enemy, "ABULLET", at);
        let multiplier = crate::util::native_x87::NativeF64Bits::from_bits(2.0f64.to_bits());
        sim.substrate
            .entities
            .get_mut(interceptor)
            .unwrap()
            .crate_multipliers
            .set_firepower_multiplier_for_test(multiplier);
        sim.commit_fire_visit(
            FireVisit::Direct {
                id: interceptor,
                target: TargetKind::Bullet(target),
                weapon_index: 0,
            },
            &rules,
            None,
            FrameEffects::empty(),
        );
        let fired = sim
            .projectiles
            .iter()
            .map(|(&id, _)| id)
            .filter(|&id| id != target)
            .max()
            .expect("interceptor bullet fired");
        let bullet = sim.projectiles.get(fired).unwrap();
        assert_eq!(bullet.payload().base_damage, expect_damage);
        assert_eq!(bullet.interceptor_type(), Some(sim.interner.intern("INT")));
        assert_ne!(
            bullet.intercepted_status() & intercepted_status::TARGETED,
            0
        );
        assert_ne!(
            sim.projectiles.get(target).unwrap().intercepted_status() & intercepted_status::LOCKED,
            0,
            "an armorless target is locked by the FireAt late logic"
        );
    }
}

#[test]
fn a_whole_interception_cycle_keeps_the_rng_and_other_bullets_intact() {
    let at = [12 * 256 + 128, 10 * 256 + 128, 0];
    let (mut sim, rules) = scene("Interceptor=yes\nInterceptor.GuardRange=4\n");
    let interceptor = unit(&mut sim, &rules, "INT", "Americans", 10);
    let enemy = unit(&mut sim, &rules, "ENEMY", "Russians", 20);
    let target = spawn_bullet(&mut sim, &rules, enemy, "ABULLET", at);
    let unrelated = spawn_bullet(
        &mut sim,
        &rules,
        enemy,
        "TBULLET",
        [18 * 256 + 128, 10 * 256 + 128, 0],
    );
    let irrelevant_before = {
        let bullet = sim.projectiles.get(unrelated).unwrap();
        (bullet.current_strength(), bullet.intercepted_status())
    };
    let before = sim.scenario_rng.state();
    scan(&mut sim, &rules, interceptor);
    let bullet = make_interceptor_bullet(&mut sim, &rules, interceptor, target, at);
    intercept(&mut sim, &rules, bullet, "IWH");
    assert_eq!(sim.scenario_rng.state(), before, "the chain draws no RNG");
    let irrelevant = sim.projectiles.get(unrelated).unwrap();
    assert_eq!(
        (
            irrelevant.current_strength(),
            irrelevant.intercepted_status()
        ),
        irrelevant_before
    );
    assert_ne!(
        sim.projectiles.get(target).unwrap().intercepted_status() & intercepted_status::INTERCEPTED,
        0
    );
}

#[test]
fn the_bullet_ai_retires_intercepted_bullets_and_inviso_interceptors_detonate() {
    let at = [12 * 256 + 128, 10 * 256 + 128, 0];
    let ctx = super::super::ObjectAiCtx::default();

    // An Intercepted bullet detonates and leaves the store on its next AI.
    let (mut sim, rules) = scene("Interceptor=yes\n");
    let interceptor = unit(&mut sim, &rules, "INT", "Americans", 10);
    let enemy = unit(&mut sim, &rules, "ENEMY", "Russians", 20);
    let target = spawn_bullet(&mut sim, &rules, enemy, "ABULLET", at);
    let bullet = make_interceptor_bullet(&mut sim, &rules, interceptor, target, at);
    intercept(&mut sim, &rules, bullet, "IWH");
    assert!(
        sim.projectiles.get(target).is_some(),
        "the warhead head marks, it does not remove"
    );
    assert!(sim.bullet_interceptor_pretick(target, &rules, ctx));
    assert!(
        sim.substrate.pending_delete.contains_object(&target),
        "0x4666F7 limbo/uninit retires the intercepted bullet"
    );

    // An Inviso interceptor detonates on its next AI (0x4668BD) and its own
    // warhead marks the prey.
    let (mut sim, rules) = scene("Interceptor=yes\n");
    let interceptor = unit(&mut sim, &rules, "INT", "Americans", 10);
    let enemy = unit(&mut sim, &rules, "ENEMY", "Russians", 20);
    let target = spawn_bullet(&mut sim, &rules, enemy, "ABULLET", at);
    let bullet = make_interceptor_bullet(&mut sim, &rules, interceptor, target, at);
    assert!(sim.bullet_interceptor_pretick(bullet, &rules, ctx));
    assert!(
        sim.substrate.pending_delete.contains_object(&bullet),
        "the Inviso interceptor detonates and retires"
    );
    assert_ne!(
        sim.projectiles.get(target).unwrap().intercepted_status() & intercepted_status::INTERCEPTED,
        0,
        "the interceptor's warhead intercepted the prey"
    );
}

/// `FootClass::Approach_Target` on a Bullet target reads the target object's
/// GetCoords, so an interceptor whose GuardRange exceeds its weapon Range
/// drives toward the projectile's current cell. VERA owns it by converting
/// the Bullet target to that cell each approach visit.
#[test]
fn a_foot_interceptor_approaches_the_bullets_cell() {
    let (mut sim, rules) = scene("Interceptor=yes\nInterceptor.GuardRange=12\n");
    let interceptor = unit(&mut sim, &rules, "INT", "Americans", 12);
    let enemy = unit(&mut sim, &rules, "ENEMY", "Russians", 20);
    let bullet = spawn_bullet(
        &mut sim,
        &rules,
        enemy,
        "OBULLET",
        [25 * 256 + 128, 10 * 256 + 128, 0],
    );
    let entity = sim
        .substrate
        .entities
        .get_mut(interceptor)
        .expect("interceptor");
    entity
        .attack_target
        .set_for_test(Some(TargetKind::Bullet(bullet)));
    entity.locomotor = Some(
        crate::sim::movement::locomotor::LocomotorState::for_test_kind(
            crate::rules::locomotor_type::LocomotorKind::Drive,
        ),
    );
    assert!(
        sim.owns_foot_approach(interceptor, &rules),
        "a live bullet target is approachable"
    );
    let destination = sim
        .approach_foot_target(interceptor, &rules, None, FrameEffects::empty())
        .expect("approach runs");
    assert!(
        matches!(
            destination,
            Some(crate::sim::components::NavTargetRef::Cell { rx, .. }) if rx > 12
        ),
        "the destination is a cell toward the projectile, got {destination:?}"
    );
}
