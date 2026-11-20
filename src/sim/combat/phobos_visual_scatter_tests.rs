//! Phobos `VisualScatter=` through the production `FireVisit::UnitTarget` path.
//!
//! `VisualScatter` replaces the *drawn* endpoint of a laser or bolt
//! (`Ext/Techno/Hooks.WeaponEffects.cpp:220-248`, hooks `0x6FD38D`/`0x6FD514`)
//! with a point inside `[AudioVisual] VisualScatter.Min/Max` of the bullet's
//! firing coordinate; the shot keeps its real target.

use super::world_fire;
use super::{TargetKind, laser};
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::FacingClass;
use crate::sim::projectile::ProjectileTarget;
use crate::sim::world::{FrameEffects, LifecycleOutput, Simulation};
use std::collections::BTreeSet;

/// A `FIRER` vehicle with the laser `GUN` aimed east at a rival `DUMMY` two
/// cells out. `weapon_extra` appends the Phobos keys under test.
fn arena(weapon_extra: &str) -> (Simulation, RuleSet, u64, u64) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[Warheads]\n0=HIT\n[BuildingTypes]\n0=DUMMY\n\
         [VehicleTypes]\n0=FIRER\n\
         [FIRER]\nStrength=1000\nArmor=heavy\nSpeed=4\nPrimary=GUN\nOmniFire=yes\nCanPassiveAquire=no\n\
         [DUMMY]\nStrength=10000\nArmor=heavy\nCanPassiveAquire=no\n\
         [WeaponTypes]\n0=GUN\n1=FEEDBACK\n\
         [GUN]\nDamage=10\nROF=40\nRange=9.5\nSpeed=100\nProjectile=INVIS\nWarhead=HIT\n\
         IsLaser=yes\nLaserDuration=10\nIsHouseColor=yes\n{weapon_extra}\
         [FEEDBACK]\nDamage=30\nROF=10\nRange=9.5\nSpeed=100\nProjectile=INVIS\nWarhead=HIT\n\
         [INVIS]\nInviso=yes\nAG=yes\n\
         [HIT]\nCellSpread=0\nAffectsAllies=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    ));
    let mut rules = RuleSet::from_ini(&ini).expect("rules read");
    rules.install_art_data(crate::rules::art_data::ArtRegistry::empty());
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x6FF4_CC00);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session
        .set_current_house_for_test(sim.interner.get("Americans"));
    sim.session.set_binary_frame_for_test(100);
    let firer = sim
        .spawn_object("FIRER", "Americans", 10, 10, 0, &rules)
        .expect("firer");
    let target = sim
        .spawn_object("DUMMY", "Russians", 12, 10, 0, &rules)
        .expect("target");
    let east = 0x4000;
    for id in [firer, target] {
        let entity = sim
            .entities_mut_for_test()
            .get_mut(id)
            .expect("placed object");
        entity.lifecycle.set_in_limbo_for_test(false);
    }
    let owner = sim.entities().get(target).map(|entity| entity.owner());
    let victim = sim
        .entities()
        .get(target)
        .map(|entity| (entity.position.rx(), entity.position.ry()))
        .expect("target");
    sim.fog
        .mark_visible_for_owner(owner.expect("owner"), victim.0, victim.1);
    let actor = sim.entities_mut_for_test().get_mut(firer).expect("firer");
    actor.body_facing.snap(east, 100);
    actor.barrel_facing = Some(FacingClass::new(east, 0));
    assert!(
        sim.assign_target_represented(
            firer,
            Some(TargetKind::Entity(target)),
            &rules,
            FrameEffects::empty()
        )
        .is_ok(),
        "the firer holds the target as a mission would"
    );
    (sim, rules, firer, target)
}

/// One production `FireVisit::UnitTarget`, and every LaserDraw it created.
fn visit_laser(sim: &mut Simulation, rules: &RuleSet, firer: u64) -> Vec<laser::LaserBirth> {
    sim.lifecycle_outputs.clear_for_test();
    sim.commit_fire_visit(
        world_fire::FireVisit::UnitTarget(firer),
        rules,
        None,
        FrameEffects::empty(),
    );
    sim.lifecycle_outputs
        .iter()
        .filter_map(|output| match output {
            LifecycleOutput::LaserCreated(birth) => Some(*birth),
            _ => None,
        })
        .collect()
}

fn health(sim: &Simulation, id: u64) -> i32 {
    sim.entities()
        .get(id)
        .expect("live entity")
        .health
        .current()
}

/// `VisualScatter=no` keeps `SpawnLaser`'s own `GetTargetCoordsForFiring`
/// endpoint; `yes` replaces it with a point inside the configured radius.
#[test]
fn a_visual_scatter_laser_ends_away_from_its_real_target() {
    let (mut sim, rules, firer, target) = arena("");
    let real =
        super::fire_coord::effect_target_coordinate(&sim, &rules, ProjectileTarget::Entity(target))
            .expect("the target has a coordinate");
    let plain = visit_laser(&mut sim, &rules, firer);
    let [plain] = plain.as_slice() else {
        panic!("one laser per shot, got {}", plain.len());
    };
    assert_eq!(plain.to, real, "VisualScatter=no must keep the target");

    let (mut sim, rules, firer, _) = arena("VisualScatter=yes\n");
    let scattered = visit_laser(&mut sim, &rules, firer);
    let [scattered] = scattered.as_slice() else {
        panic!("one laser per shot, got {}", scattered.len());
    };
    assert_ne!(
        scattered.to, real,
        "VisualScatter=yes replaces the drawn endpoint"
    );
    // Both draws come from the Scenario RNG inside this radius; the helper
    // truncates, so allow one lepton of slack.
    let max = rules.phobos_globals.visual_scatter_max;
    let (dx, dy) = (
        (scattered.to.x - real.x).abs(),
        (scattered.to.y - real.y).abs(),
    );
    assert!(
        dx <= max + 1 && dy <= max + 1,
        "the scatter stays inside the configured radius: {dx},{dy} > {max}"
    );
}
