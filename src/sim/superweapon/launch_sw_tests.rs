//! Phobos `WarheadTypeExt` LaunchSW through the production detonation path.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::movement::ground_pose::object_get_coords;
use crate::sim::projectile::{
    ProjectileCoord, ProjectileDetonation, ProjectileDetonationReason, ProjectilePayload,
    ProjectileTarget,
};
use crate::sim::world::Simulation;

fn rules(wh_keys: &str) -> RuleSet {
    RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[VehicleTypes]\n0=FIRER\n[FIRER]\nStrength=500\nSpeed=4\n\
         [WeaponTypes]\n0=GUN\n[GUN]\nDamage=0\nRange=5\nROF=20\nWarhead=WH\n\
         [Warheads]\n0=WH\n[WH]\nCellSpread=0\n{wh_keys}\
         [SuperWeaponTypes]\n0=Reveal\n[Reveal]\nUIName=Reveal\n"
    )))
    .expect("launch sw rules")
}

fn world(rules: &RuleSet) -> (Simulation, InternedId, u64) {
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    for (name, side) in [("Americans", 0), ("Russians", 1)] {
        let id = sim.intern(name);
        sim.houses
            .register(HouseState::new(id, side, None, false, 0, 10));
    }
    let americans = sim.intern("Americans");
    let firer = sim
        .spawn_object("FIRER", "Americans", 10, 10, 0, rules)
        .expect("FIRER spawns");
    (sim, americans, firer)
}

fn install(sim: &mut Simulation, owner: InternedId, name: &str, ready: bool) -> InternedId {
    let sw_type = sim.intern(name);
    let mut instance = crate::sim::superweapon::SuperWeaponInstance::new(sw_type, owner, 0);
    instance.is_active = true;
    instance.is_ready = ready;
    // A distinctive running recharge so a save/restore is observable.
    instance.charge_start_tick = -5;
    instance.charge_duration = 100;
    sim.super_weapons
        .map_mut_for_test()
        .entry(owner)
        .or_default()
        .insert(sw_type, instance);
    sw_type
}

fn is_ready(sim: &Simulation, owner: InternedId, sw: InternedId) -> bool {
    sim.super_weapons
        .get(&owner)
        .and_then(|supers| supers.get(&sw))
        .is_some_and(|instance| instance.is_ready())
}

fn charge_timer(sim: &Simulation, owner: InternedId, sw: InternedId) -> (i32, i32) {
    let instance = &sim.super_weapons[&owner][&sw];
    (instance.charge_start_tick(), instance.charge_duration())
}

/// One `ReachedTarget` detonation of `WH` at the firer's cell.
fn detonate(sim: &mut Simulation, rules: &RuleSet, firer: u64) {
    let coords = {
        let firer = sim.entities().get(firer).expect("firer");
        object_get_coords(firer, sim.resolved_terrain.as_ref())
    };
    let warhead = sim.interner.intern("WH");
    let detonation = ProjectileDetonation {
        projectile_id: u64::MAX,
        source_id: firer,
        target: ProjectileTarget::Entity(firer),
        impact: ProjectileCoord::new(coords.x, coords.y, coords.z),
        payload: ProjectilePayload::new(0, warhead, None, false),
        reason: ProjectileDetonationReason::ReachedTarget,
    };
    let mut run = crate::sim::combat::world_receiver::ReceiverRun::default();
    crate::sim::combat::world_receiver::commit_projectiles(
        sim,
        &mut run,
        &[detonation],
        rules,
        None,
    );
    run.finish();
}

/// The default `RealLaunch=true` launches a ready Super and `Reset()`
/// rewrites its charge; an uncharged one is left alone.
#[test]
fn real_launch_consumes_a_charged_super() {
    let rules = rules("LaunchSW=Reveal\n");
    assert_eq!(
        rules.warhead("WH").unwrap().phobos_launch_sw.super_types,
        vec!["Reveal".to_string()]
    );
    let (mut sim, americans, firer) = world(&rules);
    let sw = install(&mut sim, americans, "Reveal", true);
    let before = charge_timer(&sim, americans, sw);
    detonate(&mut sim, &rules, firer);
    assert!(!is_ready(&sim, americans, sw), "Reset cleared readiness");
    assert_ne!(
        charge_timer(&sim, americans, sw),
        before,
        "the launch reset the recharge timer"
    );
}

/// `RealLaunch` refuses an uncharged Super and leaves its state untouched.
#[test]
fn real_launch_refuses_an_uncharged_super() {
    let rules = rules("LaunchSW=Reveal\n");
    let (mut sim, americans, firer) = world(&rules);
    let sw = install(&mut sim, americans, "Reveal", false);
    let before = charge_timer(&sim, americans, sw);
    detonate(&mut sim, &rules, firer);
    assert!(!is_ready(&sim, americans, sw));
    assert_eq!(
        charge_timer(&sim, americans, sw),
        before,
        "nothing launched, so the timer stayed put"
    );
}

/// `LaunchSW.RealLaunch=no` launches and restores the saved recharge; `Reset`
/// still clears the readiness byte.
#[test]
fn non_real_launch_preserves_a_charged_super() {
    let rules = rules("LaunchSW=Reveal\nLaunchSW.RealLaunch=no\n");
    let (mut sim, americans, firer) = world(&rules);
    let sw = install(&mut sim, americans, "Reveal", true);
    let before = charge_timer(&sim, americans, sw);
    detonate(&mut sim, &rules, firer);
    assert_eq!(
        charge_timer(&sim, americans, sw),
        before,
        "the saved recharge timer was written back"
    );
    assert!(!is_ready(&sim, americans, sw), "Reset ran");
}
