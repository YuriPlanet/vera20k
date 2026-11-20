//! Phobos `UnlimboDetonate.*` through the production fire and detonation
//! paths (`Ext/Bullet/Hooks.DetonateLogics.cpp:502-592`,
//! `Ext/WarheadType/Hooks.cpp:609-657`, `Ext/Techno/Hooks.Firing.cpp:588-607`).

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::projectile::{
    ProjectileCoord, ProjectileDetonation, ProjectileDetonationReason, ProjectilePayload,
    ProjectileTarget,
};
use crate::sim::world::{FrameEffects, Simulation};

/// Two rival houses on the flat arena; `warhead_ini` is `[WH]`'s body.
/// `PARAGUN` is the `LimboLaunch=` copy of `GUN`, so a fired shot can limbo
/// its firer.
fn scene(warhead_ini: &str) -> (Simulation, RuleSet, u64, u64) {
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[Countries]\n0=Americans\n1=Russians\n\
         [VehicleTypes]\n0=FIRER\n1=TARGET\n\
         [WeaponTypes]\n0=GUN\n1=PARAGUN\n\
         [GUN]\nDamage=10\nRange=5\nROF=20\nWarhead=WH\n\
         [PARAGUN]\nDamage=10\nRange=5\nROF=20\nWarhead=WH\nLimboLaunch=yes\n\
         [Warheads]\n0=WH\n\
         [WH]\n{warhead_ini}\
         [FIRER]\nStrength=500\nArmor=none\nSpeed=6\nPrimary=GUN\nSecondary=PARAGUN\nMovementZone=Normal\n\
         [TARGET]\nStrength=500\nArmor=none\nSpeed=6\n"
    )))
    .expect("rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x5A_AA_2C);
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

/// Limbos the firer through the production `TechnoClass::Limbo` owner, as a
/// `LimboLaunch=` shot does.
fn limboed_scene(warhead_ini: &str) -> (Simulation, RuleSet, u64, u64) {
    let (mut sim, rules, firer, target) = scene(warhead_ini);
    sim.techno_limbo_with_rules(firer, &rules);
    assert!(
        sim.substrate
            .entities
            .get(firer)
            .unwrap()
            .lifecycle
            .in_limbo()
    );
    (sim, rules, firer, target)
}

fn cell_centre(rx: i32, ry: i32) -> ProjectileCoord {
    ProjectileCoord::new(rx * 256 + 128, ry * 256 + 128, 0)
}

/// One `ReachedTarget` detonation of `WH`, sourced from the (limboed) firer.
fn detonate(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: u64,
    target: u64,
    impact: ProjectileCoord,
) {
    let warhead = sim.interner.intern("WH");
    let detonation = ProjectileDetonation {
        projectile_id: u64::MAX,
        source_id: owner,
        target: ProjectileTarget::Entity(target),
        impact,
        payload: ProjectilePayload::new(10, warhead, None, false),
        reason: ProjectileDetonationReason::ReachedTarget,
    };
    let mut run = super::world_receiver::ReceiverRun::default();
    super::world_receiver::commit_projectiles(sim, &mut run, &[detonation], rules, None);
    run.finish();
}

/// The firer's location after a detonation.
fn firer_cell(sim: &Simulation, firer: u64) -> (u16, u16, i32) {
    let position = sim.substrate.entities.get(firer).unwrap().position;
    (
        position.rx(),
        position.ry(),
        position.sub_x().to_num::<i32>(),
    )
}

/// 1. A bullet whose warhead has `UnlimboDetonate` puts its limboed
/// `LimboLaunch` firer back when it detonates
/// (`Ext/Bullet/Hooks.DetonateLogics.cpp:507-550`), and the weapon's own
/// warhead still damages the target.
#[test]
fn unlimbo_detonate_reappears_the_limbo_launched_firer() {
    // CellSpread=4 puts the target, three cells away, inside the blast; the special arm is independent of it.
    let (mut sim, rules, firer, target) =
        limboed_scene("UnlimboDetonate=yes\nUnlimboDetonate.ForceLocation=yes\nCellSpread=4\n");
    detonate(&mut sim, &rules, firer, target, cell_centre(25, 20));
    assert!(
        !sim.substrate
            .entities
            .get(firer)
            .unwrap()
            .lifecycle
            .in_limbo(),
        "the detonating warhead unlimbos its firer"
    );
    assert_eq!(
        firer_cell(&sim, firer),
        (25, 20, 128),
        "ForceLocation reappears at the impact coordinate"
    );
    assert_eq!(
        sim.substrate.entities.get(target).unwrap().health.current(),
        490,
        "the ordinary detonation still applies its 10 damage"
    );
}

/// 2. `UnlimboDetonate.ForceLocation`: on, the firer takes the impact
/// coordinate itself; off, `NearByLocation`'s cell centre
/// (`Ext/Bullet/Hooks.DetonateLogics.cpp:514-550`).
#[test]
fn force_location_keeps_the_impact_coordinate() {
    let impact = ProjectileCoord::new(21 * 256 + 144, 20 * 256 + 144, 0);
    let (mut sim, rules, firer, target) =
        limboed_scene("UnlimboDetonate=yes\nUnlimboDetonate.ForceLocation=yes\n");
    detonate(&mut sim, &rules, firer, target, impact);
    assert_eq!(
        firer_cell(&sim, firer),
        (21, 20, 144),
        "ForceLocation keeps the exact impact coordinate"
    );

    let (mut sim, rules, firer, target) = limboed_scene("UnlimboDetonate=yes\n");
    detonate(&mut sim, &rules, firer, target, impact);
    let (rx, ry, sub_x) = firer_cell(&sim, firer);
    assert_eq!(
        sub_x, 128,
        "without ForceLocation the searched cell's GetCoords replaces the impact"
    );
    assert_eq!(
        (rx, ry),
        (21, 20),
        "the open impact cell is its own nearest passable cell"
    );
}

/// 3. `UnlimboDetonate.KeepTarget` (default yes) keeps the detonating
/// bullet's target; `no` clears it (`Ext/Bullet/Hooks.DetonateLogics.cpp:562-570`).
#[test]
fn keep_target_retains_the_bullet_target() {
    let (mut sim, rules, firer, target) =
        limboed_scene("UnlimboDetonate=yes\nUnlimboDetonate.ForceLocation=yes\n");
    detonate(&mut sim, &rules, firer, target, cell_centre(25, 20));
    assert_eq!(
        sim.substrate
            .entities
            .get(firer)
            .unwrap()
            .attack_target
            .target(),
        Some(TargetKind::Entity(target)),
        "KeepTarget defaults to yes"
    );

    let (mut sim, rules, firer, target) = limboed_scene(
        "UnlimboDetonate=yes\nUnlimboDetonate.ForceLocation=yes\nUnlimboDetonate.KeepTarget=no\n",
    );
    detonate(&mut sim, &rules, firer, target, cell_centre(25, 20));
    assert_eq!(
        sim.substrate
            .entities
            .get(firer)
            .unwrap()
            .attack_target
            .target(),
        None,
        "KeepTarget=no clears the target"
    );
}

/// 4. `UnlimboDetonate.KeepSelected`: the fire hook remembers the pre-launch
/// selection (`Ext/WarheadType/Hooks.cpp:609-657`) and the detonation
/// reselects the firer (`Ext/Bullet/Hooks.DetonateLogics.cpp:572-578`). A
/// `Parasite=` warhead takes neither arm (`:507`).
#[test]
fn keep_selected_reappears_selected_and_parasite_skips() {
    let (mut sim, rules, firer, target) =
        scene("UnlimboDetonate=yes\nUnlimboDetonate.ForceLocation=yes\n");
    sim.substrate
        .entities
        .get_mut(firer)
        .unwrap()
        .selection
        .set_selected_for_test(true);
    let weapon = rules.weapon("PARAGUN").expect("PARAGUN");
    // A real Bullet, so Limbo's Detach_All clears its Owner and the fire hook
    // must write it back (`Ext/WarheadType/Hooks.cpp:649-654`).
    let payload = ProjectilePayload::new(10, sim.interner.intern("WH"), None, false);
    let bullet = sim.projectiles.construct(
        1000,
        0,
        sim.session.binary_frame(),
        firer,
        ProjectileTarget::Entity(target),
        payload,
    );
    sim.parasite_limbo_launch(
        firer,
        TargetKind::Entity(target),
        weapon,
        Some(bullet),
        &rules,
        FrameEffects::default(),
    );
    assert!(
        sim.substrate
            .entities
            .get(firer)
            .unwrap()
            .lifecycle
            .in_limbo()
    );
    assert_eq!(
        sim.projectiles.get(bullet).unwrap().source_id(),
        firer,
        "Limbo's Detach_All cleared the bullet Owner; the fire hook restores it"
    );
    assert!(
        sim.substrate
            .entities
            .get(firer)
            .unwrap()
            .unlimbo_detonate
            .is_remembered(),
        "KeepSelected remembers the pre-launch selection"
    );
    assert!(
        !sim.substrate
            .entities
            .get(firer)
            .unwrap()
            .selection
            .is_selected(),
        "the LimboLaunch Limbo clears the object's own byte"
    );
    detonate(&mut sim, &rules, firer, target, cell_centre(25, 20));
    assert!(
        sim.substrate
            .entities
            .get(firer)
            .unwrap()
            .selection
            .is_selected(),
        "the reappear restores the remembered selection"
    );

    let (mut sim, rules, firer, target) =
        scene("UnlimboDetonate=yes\nUnlimboDetonate.ForceLocation=yes\nParasite=yes\n");
    sim.substrate
        .entities
        .get_mut(firer)
        .unwrap()
        .selection
        .set_selected_for_test(true);
    let weapon = rules.weapon("PARAGUN").expect("PARAGUN");
    sim.parasite_limbo_launch(
        firer,
        TargetKind::Entity(target),
        weapon,
        Some(1),
        &rules,
        FrameEffects::default(),
    );
    assert!(
        sim.substrate
            .entities
            .get(firer)
            .unwrap()
            .lifecycle
            .in_limbo()
    );
    assert!(
        !sim.substrate
            .entities
            .get(firer)
            .unwrap()
            .unlimbo_detonate
            .is_remembered(),
        "a Parasite warhead takes neither UnlimboDetonate arm"
    );
}

/// 5. Without the key the stock LimboLaunch behavior is unchanged: the firer
/// stays in limbo after the bullet detonates.
#[test]
fn without_unlimbo_detonate_a_limbo_firer_stays_off_the_map() {
    let (mut sim, rules, firer, target) = limboed_scene("");
    detonate(&mut sim, &rules, firer, target, cell_centre(25, 20));
    assert!(
        sim.substrate
            .entities
            .get(firer)
            .unwrap()
            .lifecycle
            .in_limbo(),
        "a warhead without the key leaves the firer in limbo"
    );
    assert_eq!(
        firer_cell(&sim, firer),
        (20, 20, 128),
        "the stock path never moves the limboed firer"
    );
}
