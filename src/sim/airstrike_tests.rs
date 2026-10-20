use super::*;
use crate::sim::combat::world_receiver;
use crate::sim::projectile::{
    ProjectileCoord, ProjectileDetonation, ProjectileDetonationReason, ProjectilePayload,
    ProjectileTarget,
};

struct Fixture {
    sim: Simulation,
    rules: RuleSet,
    boris: u64,
    target: u64,
}
impl Fixture {
    fn new() -> Option<Self> {
        let rules = crate::rules::retail_ini_fixture::retail_battle_rules()?.rules;
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 7);
        let boris = sim
            .spawn_object("BORIS", "Americans", 12, 12, 0, &rules)
            .unwrap();
        let target = sim
            .spawn_object("GAPOWR", "Russians", 15, 12, 0, &rules)
            .unwrap();
        Some(Self {
            sim,
            rules,
            boris,
            target,
        })
    }
    fn hit(&mut self, firer: u64, target: u64) {
        let weapon = self.rules.weapon("Flare").unwrap();
        let c = crate::sim::movement::ground_pose::object_get_coords(
            self.sim.substrate.entities.get(target).unwrap(),
            self.sim.resolved_terrain.as_ref(),
        );
        let payload = ProjectilePayload::new(
            weapon.damage,
            self.sim.interner.intern(weapon.warhead.as_deref().unwrap()),
            weapon
                .projectile
                .as_deref()
                .map(|name| self.sim.interner.intern(name)),
            weapon.bright,
        )
        .with_installed_weapon(Some(self.sim.interner.intern("Flare")));
        world_receiver::commit_projectiles(
            &mut self.sim,
            &mut world_receiver::ReceiverRun::default(),
            &[ProjectileDetonation {
                projectile_id: 999,
                source_id: firer,
                target: ProjectileTarget::Entity(target),
                impact: ProjectileCoord::new(c.x, c.y, c.z),
                payload,
                reason: ProjectileDetonationReason::ReachedTarget,
            }],
            &self.rules,
            None,
        );
    }
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn retail_flare_starts_two_aircraft_and_preserves_special_damage_suppression() {
    let Some(mut f) = Fixture::new() else { return };
    assert!(f.sim.airstrike_manager(f.boris).is_some());
    let health = f
        .sim
        .substrate
        .entities
        .get(f.target)
        .unwrap()
        .health
        .current();
    f.hit(f.boris, f.target);
    let m = f.sim.airstrike_manager(f.boris).unwrap();
    assert!(m.busy && m.active);
    assert_eq!(m.target, Some(f.target));
    assert_eq!(m.aircraft.len(), 2);
    assert_eq!(m.remaining, 2);
    for &plane in &m.aircraft {
        let e = f.sim.substrate.entities.get(plane).unwrap();
        assert_eq!(f.sim.interner.resolve(e.type_ref()), "BPLN");
        assert_eq!(e.airstrike_link(), Some(f.boris));
        assert!(matches!(
            e.attack_target.target(),
            Some(TargetKind::Cell(_, _))
        ));
        assert_eq!(e.veterancy.rank_kind(), VeterancyRank::Rookie);
    }
    assert_eq!(
        f.sim
            .substrate
            .entities
            .get(f.target)
            .unwrap()
            .airstrike_link(),
        Some(f.boris)
    );
    assert_eq!(
        f.sim
            .substrate
            .entities
            .get(f.target)
            .unwrap()
            .health
            .current(),
        health
    );
    assert!(f.sim.airstrike_tint_active(f.target));
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn last_departure_starts_recharge_without_erasing_target() {
    let Some(mut f) = Fixture::new() else { return };
    f.hit(f.boris, f.target);
    let aircraft = f.sim.airstrike_manager(f.boris).unwrap().aircraft.clone();
    f.sim.session.set_binary_frame_for_test(40);
    crate::sim::world::Simulation::airstrike_remove_aircraft(
        &f.sim.session,
        &mut f.sim.substrate,
        f.boris,
        aircraft[0],
    );
    assert!(f.sim.airstrike_manager(f.boris).unwrap().busy);
    crate::sim::world::Simulation::airstrike_remove_aircraft(
        &f.sim.session,
        &mut f.sim.substrate,
        f.boris,
        aircraft[1],
    );
    let m = f.sim.airstrike_manager(f.boris).unwrap();
    assert!(!m.busy && !m.active);
    assert_eq!(m.target, Some(f.target));
    assert_eq!(m.returned_at, Some(40));
    f.sim.session.set_binary_frame_for_test(139);
    f.hit(f.boris, f.target);
    assert!(!f.sim.airstrike_manager(f.boris).unwrap().busy);
    f.sim.session.set_binary_frame_for_test(140);
    f.hit(f.boris, f.target);
    assert!(f.sim.airstrike_manager(f.boris).unwrap().busy);
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn unequal_ammo_retarget_cancels_and_empties_entire_sortie() {
    let Some(mut f) = Fixture::new() else { return };
    f.hit(f.boris, f.target);
    let aircraft = f.sim.airstrike_manager(f.boris).unwrap().aircraft.clone();
    f.sim
        .substrate
        .entities
        .get_mut(aircraft[0])
        .unwrap()
        .ammo_mut()
        .unwrap()
        .empty();
    let next = f
        .sim
        .spawn_object("GAPOWR", "Russians", 18, 12, 0, &f.rules)
        .unwrap();
    f.hit(f.boris, next);
    let m = f.sim.airstrike_manager(f.boris).unwrap();
    assert!(m.busy);
    assert!(!m.active);
    assert_eq!(m.target, None);
    for id in aircraft {
        let e = f.sim.substrate.entities.get(id).unwrap();
        assert_eq!(e.ammo().unwrap().current(), 0);
        assert_eq!(e.attack_target.target(), None);
    }
    assert_eq!(
        f.sim
            .substrate
            .entities
            .get(f.target)
            .unwrap()
            .airstrike_link(),
        None
    );
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn marker_restoration_uses_last_other_manager_even_after_its_sortie_finished() {
    let Some(mut f) = Fixture::new() else { return };
    f.hit(f.boris, f.target);
    let second = f
        .sim
        .spawn_object("BORIS", "Americans", 12, 14, 0, &f.rules)
        .unwrap();
    f.hit(second, f.target);
    let first_planes = f.sim.airstrike_manager(f.boris).unwrap().aircraft.clone();
    for id in first_planes {
        crate::sim::world::Simulation::airstrike_remove_aircraft(
            &f.sim.session,
            &mut f.sim.substrate,
            f.boris,
            id,
        );
    }
    f.sim
        .airstrike_clear_target(second, Some(&f.rules), FrameEffects::default());
    assert_eq!(
        f.sim
            .substrate
            .entities
            .get(f.target)
            .unwrap()
            .airstrike_link(),
        Some(f.boris)
    );
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn live_target_expiry_preserves_strike_dead_target_expiry_cancels_it() {
    let Some(mut f) = Fixture::new() else { return };
    f.hit(f.boris, f.target);
    f.sim
        .airstrike_pointer_expired(f.boris, f.target, Some(&f.rules), FrameEffects::default());
    assert_eq!(
        f.sim.airstrike_manager(f.boris).unwrap().target,
        Some(f.target)
    );
    f.sim
        .substrate
        .entities
        .get_mut(f.target)
        .unwrap()
        .health
        .set(0);
    f.sim
        .airstrike_pointer_expired(f.boris, f.target, Some(&f.rules), FrameEffects::default());
    assert_eq!(f.sim.airstrike_manager(f.boris).unwrap().target, None);
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn owner_target_change_cancels_but_leaves_planes_until_their_departure() {
    let Some(mut f) = Fixture::new() else { return };
    f.sim
        .assign_target_represented(
            f.boris,
            Some(TargetKind::Entity(f.target)),
            Some(&f.rules),
            None,
            FrameEffects::default(),
        )
        .unwrap();
    f.hit(f.boris, f.target);
    f.sim
        .assign_target_represented(f.boris, None, Some(&f.rules), None, FrameEffects::default())
        .unwrap();
    let m = f.sim.airstrike_manager(f.boris).unwrap();
    assert!(m.busy);
    assert_eq!(m.target, None);
    assert_eq!(m.aircraft.len(), 2);
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn equal_ammo_retarget_transfers_marker_and_gives_planes_object_targets() {
    let Some(mut f) = Fixture::new() else { return };
    f.hit(f.boris, f.target);
    let next = f
        .sim
        .spawn_object("GAPOWR", "Russians", 18, 12, 0, &f.rules)
        .unwrap();
    let original = f.sim.airstrike_manager(f.boris).unwrap().aircraft.clone();
    f.hit(f.boris, next);
    let m = f.sim.airstrike_manager(f.boris).unwrap();
    assert_eq!(m.aircraft, original);
    assert_eq!(m.target, Some(next));
    assert_eq!(
        f.sim
            .substrate
            .entities
            .get(f.target)
            .unwrap()
            .airstrike_link(),
        None
    );
    for plane in original {
        assert_eq!(
            f.sim
                .substrate
                .entities
                .get(plane)
                .unwrap()
                .attack_target
                .target(),
            Some(TargetKind::Entity(next))
        );
    }
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn owner_expiry_unlinks_surviving_planes_and_clears_the_designation() {
    let Some(mut f) = Fixture::new() else { return };
    f.hit(f.boris, f.target);
    let planes = f.sim.airstrike_manager(f.boris).unwrap().aircraft.clone();
    f.sim
        .airstrike_pointer_expired(f.boris, f.boris, Some(&f.rules), FrameEffects::default());
    let house = f.sim.substrate.entities.get(f.boris).unwrap().owner();
    assert!(f.sim.sound_events.iter().any(|event| matches!(event,
        SimSoundEvent::VocAt { sound_id, audible_to: Some(houses), .. }
            if sound_id == "MIGMissionAborted" && *houses == [house, house]
    )));
    let m = f.sim.airstrike_manager(f.boris).unwrap();
    assert!(m.aircraft.is_empty());
    assert!(!m.busy && !m.active);
    for id in planes {
        assert_eq!(
            f.sim.substrate.entities.get(id).unwrap().airstrike_link(),
            None
        );
    }
    assert_eq!(
        f.sim
            .substrate
            .entities
            .get(f.target)
            .unwrap()
            .airstrike_link(),
        None
    );
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn snapshot_preserves_cancelled_sortie_and_rejects_unresolved_manager_references() {
    use crate::sim::snapshot::GameSnapshot;
    let Some(mut f) = Fixture::new() else { return };
    f.hit(f.boris, f.target);
    f.sim
        .airstrike_clear_target(f.boris, Some(&f.rules), FrameEffects::default());
    // Scenario RNG intentionally reseeds on native-shaped load; compare the
    // manager's retained state, not the whole pre-load simulation hash.
    let before = f
        .sim
        .substrate
        .entities
        .get(f.boris)
        .unwrap()
        .airstrike
        .clone();
    let bytes = GameSnapshot::save(&f.sim, 0, 0, "airstrike", 0);
    let mut loaded = GameSnapshot::load(&bytes).unwrap().sim;
    loaded.restore_after_snapshot_load().unwrap();
    assert_eq!(
        loaded.substrate.entities.get(f.boris).unwrap().airstrike,
        before
    );
    let m = loaded.airstrike_manager(f.boris).unwrap();
    assert!(m.busy);
    assert!(!m.active);
    assert!(m.target.is_none());
    assert_eq!(m.aircraft.len(), 2);
    for &plane in &m.aircraft {
        assert_eq!(
            loaded
                .substrate
                .entities
                .get(plane)
                .unwrap()
                .airstrike_link(),
            Some(f.boris)
        );
    }
    f.sim
        .substrate
        .airstrike_manager_mut(f.boris)
        .unwrap()
        .aircraft
        .push(u64::MAX);
    let bytes = GameSnapshot::save(&f.sim, 0, 0, "invalid", 0);
    let mut invalid = GameSnapshot::load(&bytes).unwrap().sim;
    assert!(invalid.restore_after_snapshot_load().is_err());
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn live_detach_keeps_designator_and_aircraft_aimed_at_the_strike_target() {
    let Some(mut f) = Fixture::new() else { return };
    f.sim
        .assign_target_represented(
            f.boris,
            Some(TargetKind::Entity(f.target)),
            Some(&f.rules),
            None,
            FrameEffects::default(),
        )
        .unwrap();
    f.hit(f.boris, f.target);
    f.sim
        .stop_all_targeting_on_detach(f.target, Some(&f.rules), None, FrameEffects::default());
    assert_eq!(
        f.sim
            .substrate
            .entities
            .get(f.boris)
            .unwrap()
            .attack_target
            .target(),
        Some(TargetKind::Entity(f.target))
    );
    assert!(f.sim.airstrike_manager(f.boris).unwrap().active);
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn elite_boris_selects_four_rookie_planes_and_the_shorter_recharge() {
    let Some(mut f) = Fixture::new() else { return };
    f.sim
        .substrate
        .entities
        .get_mut(f.boris)
        .unwrap()
        .veterancy
        .set_elite();
    f.hit(f.boris, f.target);
    let aircraft = f.sim.airstrike_manager(f.boris).unwrap().aircraft.clone();
    assert_eq!(aircraft.len(), 4);
    for &id in &aircraft {
        assert_eq!(
            f.sim
                .substrate
                .entities
                .get(id)
                .unwrap()
                .veterancy
                .rank_kind(),
            VeterancyRank::Rookie
        );
    }
    f.sim.session.set_binary_frame_for_test(10);
    for id in aircraft {
        crate::sim::world::Simulation::airstrike_remove_aircraft(
            &f.sim.session,
            &mut f.sim.substrate,
            f.boris,
            id,
        );
    }
    f.sim.session.set_binary_frame_for_test(59);
    f.hit(f.boris, f.target);
    assert!(!f.sim.airstrike_manager(f.boris).unwrap().busy);
    f.sim.session.set_binary_frame_for_test(60);
    f.hit(f.boris, f.target);
    assert_eq!(f.sim.airstrike_manager(f.boris).unwrap().aircraft.len(), 4);
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn a_different_target_is_admitted_before_same_target_recharge() {
    let Some(mut f) = Fixture::new() else { return };
    f.hit(f.boris, f.target);
    let planes = f.sim.airstrike_manager(f.boris).unwrap().aircraft.clone();
    f.sim.session.set_binary_frame_for_test(20);
    for id in planes {
        crate::sim::world::Simulation::airstrike_remove_aircraft(
            &f.sim.session,
            &mut f.sim.substrate,
            f.boris,
            id,
        );
    }
    let other = f
        .sim
        .spawn_object("GAPOWR", "Russians", 18, 12, 0, &f.rules)
        .unwrap();
    f.hit(f.boris, other);
    let m = f.sim.airstrike_manager(f.boris).unwrap();
    assert!(m.busy);
    assert_eq!(m.target, Some(other));
}

#[test]
fn designation_coordinates_follow_the_existing_sortie_target_through_departure_and_clear() {
    let Some(mut f) = Fixture::new() else { return };
    assert!(
        f.sim
            .airstrike_designation_coordinates(f.boris, &f.rules)
            .is_none()
    );
    assert_eq!(
        f.rules.object("BORIS").unwrap().weapon_list[1].as_deref(),
        Some("Flare")
    );
    let flare = f.rules.weapon("Flare").unwrap();
    assert!(flare.is_line && flare.is_house_color && !flare.is_laser);
    f.hit(f.boris, f.target);
    let before = f.sim.rng_state();
    let first = f
        .sim
        .airstrike_designation_coordinates(f.boris, &f.rules)
        .unwrap();
    assert_eq!(f.sim.rng_state(), before);
    assert_eq!(
        first[1],
        crate::sim::movement::ground_pose::object_get_coords(
            f.sim.substrate.entities.get(f.target).unwrap(),
            f.sim.resolved_terrain.as_ref()
        )
    );
    assert!(
        f.sim
            .airstrike_designation_coordinates(f.target, &f.rules)
            .is_none(),
        "borrowed marker link is not the manager owner"
    );
    let aircraft = f.sim.airstrike_manager(f.boris).unwrap().aircraft.clone();
    for id in aircraft {
        assert!(
            f.sim
                .airstrike_designation_coordinates(id, &f.rules)
                .is_none()
        );
        crate::sim::world::Simulation::airstrike_remove_aircraft(
            &f.sim.session,
            &mut f.sim.substrate,
            f.boris,
            id,
        );
    }
    assert!(!f.sim.airstrike_manager(f.boris).unwrap().active);
    assert_eq!(
        f.sim.airstrike_designation_coordinates(f.boris, &f.rules),
        Some(first)
    );
    let next = f
        .sim
        .spawn_object("GAPOWR", "Russians", 18, 12, 0, &f.rules)
        .unwrap();
    f.hit(f.boris, next);
    assert_ne!(
        f.sim
            .airstrike_designation_coordinates(f.boris, &f.rules)
            .unwrap()[1],
        first[1]
    );
    f.sim
        .airstrike_clear_target(f.boris, Some(&f.rules), FrameEffects::default());
    assert!(
        f.sim
            .airstrike_designation_coordinates(f.boris, &f.rules)
            .is_none()
    );
    let unit = f
        .sim
        .spawn_object("HTNK", "Russians", 20, 12, 0, &f.rules)
        .unwrap();
    f.hit(f.boris, unit);
    assert_eq!(f.sim.airstrike_manager(f.boris).unwrap().target, Some(unit));
    assert!(
        f.sim
            .airstrike_designation_coordinates(f.boris, &f.rules)
            .is_none(),
        "nonbuilding manager target has no Tactical beam"
    );
}
