//! Retail charging through the shared Bullet receiver and live state owners.

use super::*;
use crate::sim::combat::{combat_weapon, world_receiver};
use crate::sim::power_system::PowerState;
use crate::sim::projectile::{
    ProjectileCoord, ProjectileDetonation, ProjectileDetonationReason, ProjectilePayload,
    ProjectileTarget,
};
use crate::sim::snapshot::{GameSnapshot, SnapshotRestoreError};
use crate::sim::world::ObjectAiCtx;

struct Fixture {
    sim: Simulation,
    rules: RuleSet,
    coil: u64,
    troopers: [u64; 3],
}

impl Fixture {
    fn new() -> Option<Self> {
        let rules = crate::rules::retail_ini_fixture::retail_battle_rules()?.rules;
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x7E51A);
        let coil = sim
            .spawn_object("TESLA", "Americans", 12, 12, 0, &rules)
            .unwrap();
        let troopers = std::array::from_fn(|i| {
            sim.spawn_object("SHK", "Americans", 11, 12 + i as u16, 0, &rules)
                .unwrap()
        });
        let owner = sim.interner.intern("Americans");
        sim.power_states.insert(owner, PowerState::default());
        Some(Self {
            sim,
            rules,
            coil,
            troopers,
        })
    }

    fn assign(&mut self, source: u64, target: Option<u64>) {
        self.sim
            .assign_target_represented(
                source,
                target.map(TargetKind::Entity),
                Some(&self.rules),
                None,
                FrameEffects::default(),
            )
            .unwrap();
    }

    fn hit(&mut self, source: u64, target: u64) {
        let assault = self.rules.weapon("AssaultBolt").unwrap();
        assert!(
            self.rules
                .warhead(assault.warhead.as_deref().unwrap())
                .unwrap()
                .electric_assault
        );
        let at = ground_pose::object_get_coords(
            self.sim.substrate.entities.get(target).unwrap(),
            self.sim.resolved_terrain.as_ref(),
        );
        let payload = ProjectilePayload::new(
            assault.damage,
            self.sim
                .interner
                .intern(assault.warhead.as_deref().unwrap()),
            assault
                .projectile
                .as_deref()
                .map(|name| self.sim.interner.intern(name)),
            assault.bright,
        )
        .with_installed_weapon(Some(self.sim.interner.intern("AssaultBolt")));
        world_receiver::commit_projectiles(
            &mut self.sim,
            &mut world_receiver::ReceiverRun::default(),
            &[ProjectileDetonation {
                projectile_id: 999,
                source_id: source,
                target: ProjectileTarget::Entity(target),
                impact: ProjectileCoord::new(at.x, at.y, at.z),
                payload,
                reason: ProjectileDetonationReason::ReachedTarget,
            }],
            &self.rules,
            None,
        );
    }

    fn charge(&self) -> &BuildingCharge {
        &self
            .sim
            .substrate
            .entities
            .get(self.coil)
            .unwrap()
            .tesla_charge
    }

    fn set_power(&mut self, output: i32, drain: i32) {
        let owner = self.sim.substrate.entities.get(self.coil).unwrap().owner();
        let power = self.sim.power_states.get_mut(&owner).unwrap();
        power.total_output = output;
        power.total_drain = drain;
    }

    fn dispatch(&mut self, source: u64, mission: MissionType) {
        let now = self.sim.session.binary_frame();
        self.sim
            .substrate
            .entities
            .get_mut(source)
            .unwrap()
            .mission
            .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
                current: MissionId::from_known(mission),
                suspended: MissionId::NONE,
                queued: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: now,
                ai_counter: 0,
                dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(now),
            });
        crate::sim::world::dispatch_foot_mission(
            &mut self.sim,
            source,
            &self.rules,
            ObjectAiCtx::default(),
        );
    }
}

#[test]
fn retail_electric_assault_enables_the_coils_overpowered_weapon() {
    let Some(mut f) = Fixture::new() else { return };
    let target = f
        .sim
        .spawn_object("MTNK", "Russians", 20, 12, 0, &f.rules)
        .unwrap();
    f.assign(f.troopers[0], Some(f.coil));
    let health = f
        .sim
        .substrate
        .entities
        .get(f.coil)
        .unwrap()
        .health
        .current();
    f.hit(f.troopers[0], f.coil);
    assert_eq!(
        f.sim
            .substrate
            .entities
            .get(f.coil)
            .unwrap()
            .health
            .current(),
        health
    );
    assert!(
        f.sim
            .object_ai_visit_one(f.coil, Some(&f.rules), ObjectAiCtx::default())
    );
    let coil = f.sim.substrate.entities.get(f.coil).unwrap();
    let object = f.rules.object("TESLA").unwrap();
    let selected = combat_weapon::select_weapon_against(
        &f.rules,
        coil,
        object,
        &combat_weapon::attacker_facts(coil, object),
        Some(&TargetKind::Entity(target)),
        &f.sim,
        &f.sim.interner,
        f.sim.resolved_terrain.as_ref(),
        Some(&f.sim.house_alliances),
    )
    .unwrap();
    assert_eq!(
        selected.0, 1,
        "one retained charger at full power sets Building+661"
    );
    assert_eq!(selected.1.unwrap().weapon_id, "OPCoilBolt");
}

#[test]
fn impact_refreshes_order_without_damage_or_rearm_and_class_refusals_claim_it() {
    let Some(mut f) = Fixture::new() else { return };
    let [a, b, c] = f.troopers;
    let health = f
        .sim
        .substrate
        .entities
        .get(f.coil)
        .unwrap()
        .health
        .current();
    let rearm = f.sim.substrate.entities.get(a).unwrap().rearm_timer;
    for source in [a, b, c, a] {
        f.hit(source, f.coil);
    }
    assert_eq!(f.charge().chargers(), &[b, c, a]);
    assert!(
        !f.charge().is_overpowered(),
        "impact does not sample the AI latch"
    );
    assert_eq!(
        f.sim
            .substrate
            .entities
            .get(f.coil)
            .unwrap()
            .health
            .current(),
        health
    );
    // The complete detonation still runs its existing animation tail. Only
    // ElectricAssault's list refresh itself must leave RNG untouched.
    let rng = f.sim.scenario_rng.logical_state();
    f.sim.electric_assault_detonation(a, Some(f.coil));
    assert_eq!(f.sim.scenario_rng.logical_state(), rng);
    assert_eq!(f.sim.substrate.entities.get(a).unwrap().rearm_timer, rearm);

    let tank = f
        .sim
        .spawn_object("MTNK", "Americans", 20, 12, 0, &f.rules)
        .unwrap();
    let tank_health = f.sim.substrate.entities.get(tank).unwrap().health.current();
    f.hit(tank, f.coil); // wrong firer class
    f.hit(a, tank); // wrong target class, still no ordinary area damage
    assert_eq!(f.charge().chargers(), &[b, c, a]);
    assert_eq!(
        f.sim.substrate.entities.get(tank).unwrap().health.current(),
        tank_health
    );
    // Impact has no Overpowerable or alliance gate: these belong to targeting.
    let hostile_plant = f
        .sim
        .spawn_object("GAPOWR", "Russians", 24, 24, 0, &f.rules)
        .unwrap();
    f.hit(a, hostile_plant);
    assert_eq!(
        f.sim
            .substrate
            .entities
            .get(hostile_plant)
            .unwrap()
            .tesla_charge
            .chargers(),
        &[a]
    );
}

#[test]
fn operational_power_and_overpowered_weapon_use_different_thresholds() {
    let Some(mut f) = Fixture::new() else { return };
    for source in f.troopers {
        f.assign(source, Some(f.coil));
    }
    for (count, output, drain, operational, overpowered) in [
        (0, 0, 0, true, false),
        (1, 0, 100, false, false),
        (2, 0, 100, true, false),
        (3, 0, 100, true, true),
        (1, 100, 100, true, true),
        (1, 200, 100, true, true),
        // has_full_power is true for negative drain, but native ratio is2.
        (1, -2, -1, true, false),
    ] {
        f.sim
            .substrate
            .entities
            .get_mut(f.coil)
            .unwrap()
            .tesla_charge = BuildingCharge::default();
        f.set_power(output, drain);
        for i in 0..count {
            f.hit(f.troopers[i], f.coil);
        }
        assert_eq!(
            f.sim.building_operational_state(f.coil, &f.rules),
            Some(operational)
        );
        f.sim.update_tesla_charge(f.coil, &f.rules);
        assert_eq!(
            f.charge().is_overpowered(),
            overpowered,
            "{count}/{output}/{drain}"
        );
        let owner = f.sim.substrate.entities.get(f.coil).unwrap().owner();
        let power = &f.sim.power_states[&owner];
        assert_eq!((power.total_output, power.total_drain), (output, drain));
    }
}

#[test]
fn two_retained_chargers_bypass_the_buildings_offline_latch() {
    let Some(mut f) = Fixture::new() else { return };
    f.sim
        .substrate
        .entities
        .get_mut(f.coil)
        .unwrap()
        .set_temporal_for_test(crate::sim::temporal::TemporalState::warped_by_for_test(
            f.troopers[2],
        ));
    assert!(
        !f.sim
            .substrate
            .entities
            .get(f.coil)
            .unwrap()
            .building_online()
    );
    f.hit(f.troopers[0], f.coil);
    assert_eq!(
        f.sim.building_operational_state(f.coil, &f.rules),
        Some(false)
    );
    f.hit(f.troopers[1], f.coil);
    assert_eq!(
        f.sim.building_operational_state(f.coil, &f.rules),
        Some(true)
    );
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn retarget_waits_for_building_turn_and_both_expiry_controls_remove_indexed_chargers() {
    for uninit in [false, true] {
        let Some(mut f) = Fixture::new() else { return };
        let a = f.troopers[0];
        f.assign(a, Some(f.coil));
        f.hit(a, f.coil);
        f.sim.update_tesla_charge(f.coil, &f.rules);
        assert!(f.charge().is_overpowered());
        // No other Building reference names a: the derived expiry index must
        // discover this listener from the new charger list alone.
        if uninit {
            f.sim.uninit_with_rules(a, &f.rules);
        } else {
            f.sim
                .detach_all_pointer_expired(a, &f.rules, None, FrameEffects::default());
        }
        assert!(f.charge().chargers().is_empty());
        assert!(f.charge().is_overpowered(), "expiry does not write +661");
        f.sim.update_tesla_charge(f.coil, &f.rules);
        assert!(!f.charge().is_overpowered());
    }
    let Some(mut f) = Fixture::new() else { return };
    for source in f.troopers {
        f.assign(source, Some(f.coil));
        f.hit(source, f.coil);
    }
    f.set_power(0, 100);
    f.sim.update_tesla_charge(f.coil, &f.rules);
    f.assign(f.troopers[1], None);
    assert_eq!(f.charge().chargers(), &f.troopers);
    assert!(f.charge().is_overpowered());
    f.sim.update_tesla_charge(f.coil, &f.rules);
    assert_eq!(f.charge().chargers(), &[f.troopers[0], f.troopers[2]]);
    assert!(!f.charge().is_overpowered());
}

#[test]
fn snapshot_preserves_order_intent_and_stale_latch_then_rejects_invalid_references() {
    let Some(mut f) = Fixture::new() else { return };
    for source in f.troopers {
        f.assign(source, Some(f.coil));
        f.hit(source, f.coil);
    }
    f.hit(f.troopers[0], f.coil);
    f.sim.update_tesla_charge(f.coil, &f.rules);
    for source in f.troopers {
        f.assign(source, None);
    }
    f.sim
        .substrate
        .entities
        .get_mut(f.troopers[0])
        .unwrap()
        .tesla_charger
        .active = true;
    let bytes = GameSnapshot::save(&f.sim, 0, 0, "Tesla", 0);
    let mut loaded = GameSnapshot::load(&bytes).unwrap().sim;
    loaded.restore_after_snapshot_load().unwrap();
    assert_eq!(
        loaded.substrate.entities.get(f.coil).unwrap().tesla_charge,
        *f.charge()
    );
    assert!(
        loaded
            .substrate
            .entities
            .get(f.troopers[0])
            .unwrap()
            .tesla_charger
            .is_active()
    );
    loaded.update_tesla_charge(f.coil, &f.rules);
    assert!(
        loaded
            .substrate
            .entities
            .get(f.coil)
            .unwrap()
            .tesla_charge
            .is_default()
    );

    for invalid in [
        vec![u64::MAX],
        vec![f.coil],
        vec![f.troopers[0], f.troopers[0]],
    ] {
        f.sim
            .substrate
            .entities
            .get_mut(f.coil)
            .unwrap()
            .tesla_charge
            .chargers = invalid;
        let bytes = GameSnapshot::save(&f.sim, 0, 0, "Tesla-invalid", 0);
        let mut loaded = GameSnapshot::load(&bytes).unwrap().sim;
        assert!(matches!(
            loaded.restore_after_snapshot_load(),
            Err(SnapshotRestoreError::UnresolvedObjectReference { .. }
                | SnapshotRestoreError::InvalidTeslaChargingState { .. })
        ));
    }
}

#[test]
fn hash_distinguishes_charger_order_weapon_latch_and_foot_intent() {
    let Some(mut f) = Fixture::new() else { return };
    let initial = f.sim.state_hash();
    f.sim
        .substrate
        .entities
        .get_mut(f.coil)
        .unwrap()
        .tesla_charge
        .chargers = f.troopers.to_vec();
    let list = f.sim.state_hash();
    assert_ne!(initial, list);
    f.sim
        .substrate
        .entities
        .get_mut(f.coil)
        .unwrap()
        .tesla_charge
        .chargers
        .reverse();
    let order = f.sim.state_hash();
    assert_ne!(list, order);
    f.sim
        .substrate
        .entities
        .get_mut(f.coil)
        .unwrap()
        .tesla_charge
        .overpowered = true;
    let latch = f.sim.state_hash();
    assert_ne!(order, latch);
    f.sim
        .substrate
        .entities
        .get_mut(f.troopers[0])
        .unwrap()
        .tesla_charger
        .active = true;
    assert_ne!(latch, f.sim.state_hash());
}

#[test]
fn adjacency_uses_native_direction_order_and_same_owner_not_alliance() {
    let Some(mut f) = Fixture::new() else { return };
    let (_, height) = crate::rules::foundation::foundation_dimensions(
        &f.rules.object("TESLA").unwrap().foundation,
    );
    let north = f
        .sim
        .spawn_object("TESLA", "Russians", 20, 20 - u16::from(height), 0, &f.rules)
        .unwrap();
    let east = f
        .sim
        .spawn_object("TESLA", "Americans", 21, 20, 0, &f.rules)
        .unwrap();
    let foot = f
        .sim
        .spawn_object("SHK", "Americans", 20, 20, 0, &f.rules)
        .unwrap();
    f.sim
        .house_alliances
        .entry("Americans".into())
        .or_default()
        .insert("Russians".into());
    assert!(
        f.sim
            .acquire_adjacent_tesla_coil(foot, &f.rules, None, FrameEffects::default())
    );
    assert_eq!(
        f.sim
            .substrate
            .entities
            .get(foot)
            .unwrap()
            .attack_target
            .target(),
        Some(TargetKind::Entity(east))
    );
    assert!(
        f.sim
            .substrate
            .entities
            .get(foot)
            .unwrap()
            .tesla_charger
            .is_active()
    );
    // Reverse the ownership assignment by using a fresh same-owner northern
    // coil in a separate scene; N must beat E.
    f.sim.uninit_with_rules(north, &f.rules);
    f.sim.process_pending_delete();
    let north = f
        .sim
        .spawn_object(
            "TESLA",
            "Americans",
            20,
            20 - u16::from(height),
            0,
            &f.rules,
        )
        .unwrap();
    f.assign(foot, None);
    assert!(
        f.sim
            .acquire_adjacent_tesla_coil(foot, &f.rules, None, FrameEffects::default())
    );
    assert_eq!(
        f.sim
            .substrate
            .entities
            .get(foot)
            .unwrap()
            .attack_target
            .target(),
        Some(TargetKind::Entity(north))
    );
}

#[test]
fn infantry_same_target_clears_intent_but_deployment_refusal_preserves_it() {
    let Some(mut f) = Fixture::new() else { return };
    let a = f.troopers[0];
    assert!(
        f.sim
            .acquire_adjacent_tesla_coil(a, &f.rules, None, FrameEffects::default())
    );
    f.assign(a, Some(f.coil));
    assert!(
        !f.sim
            .substrate
            .entities
            .get(a)
            .unwrap()
            .tesla_charger
            .is_active()
    );
    f.sim
        .substrate
        .entities
        .get_mut(a)
        .unwrap()
        .tesla_charger
        .active = true;
    f.sim
        .substrate
        .entities
        .get_mut(a)
        .unwrap()
        .mission_leaf
        .set_infantry_doing_verified(27)
        .unwrap();
    f.assign(a, Some(f.coil));
    assert!(
        f.sim
            .substrate
            .entities
            .get(a)
            .unwrap()
            .tesla_charger
            .is_active()
    );
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn guard_and_area_guard_queue_charging_before_their_normal_cadence() {
    for mission in [MissionType::Guard, MissionType::AreaGuard] {
        let Some(mut f) = Fixture::new() else { return };
        let source = f.troopers[0];
        let now = f.sim.session.binary_frame();
        f.sim
            .substrate
            .entities
            .get_mut(source)
            .unwrap()
            .rearm_timer
            .start(now as i32, 7);
        let rng = f.sim.scenario_rng.logical_state();
        f.dispatch(source, mission);
        let actor = f.sim.substrate.entities.get(source).unwrap();
        assert_eq!(
            actor.attack_target.target(),
            Some(TargetKind::Entity(f.coil))
        );
        assert!(actor.tesla_charger.is_active());
        assert_eq!(
            actor.mission.queued(),
            MissionId::from_known(MissionType::Attack)
        );
        let delay = actor
            .mission
            .dispatch_timer()
            .remaining_if_pending(now)
            .unwrap();
        if mission == MissionType::Guard {
            assert_eq!(delay, 7, "Guard keeps the running rearm delay");
            assert_eq!(
                f.sim.scenario_rng.logical_state(),
                rng,
                "found charging target skips idle and jitter"
            );
        } else {
            assert!(
                delay > 1,
                "AreaGuard charging does not take the enemy-scan return1"
            );
        }
    }
}

#[test]
fn charging_attack_keeps_its_coil_until_a_direct_enemy_scan_succeeds() {
    let Some(mut f) = Fixture::new() else { return };
    let source = f.troopers[0];
    assert!(
        f.sim
            .acquire_adjacent_tesla_coil(source, &f.rules, None, FrameEffects::default())
    );
    let passive = f.sim.substrate.entities.get(source).unwrap().passive_scan;
    f.dispatch(source, MissionType::Attack);
    assert_eq!(
        f.sim
            .substrate
            .entities
            .get(source)
            .unwrap()
            .attack_target
            .target(),
        Some(TargetKind::Entity(f.coil))
    );
    assert!(
        f.sim
            .substrate
            .entities
            .get(source)
            .unwrap()
            .tesla_charger
            .is_active()
    );
    assert_eq!(
        f.sim.substrate.entities.get(source).unwrap().passive_scan,
        passive
    );

    let enemy = f
        .sim
        .spawn_object("MTNK", "Russians", 10, 11, 0, &f.rules)
        .unwrap();
    let owner = f.sim.substrate.entities.get(source).unwrap().owner();
    f.sim.fog.reveal_all_for_owner(owner);
    f.dispatch(source, MissionType::Attack);
    let source = f.sim.substrate.entities.get(source).unwrap();
    assert_eq!(
        source.attack_target.target(),
        Some(TargetKind::Entity(enemy))
    );
    assert!(!source.tesla_charger.is_active());
    assert_eq!(
        source.passive_scan.timer(),
        passive.timer(),
        "direct scan never rearms the passive timer"
    );
}

#[test]
fn freezing_a_retail_charger_clears_its_intent_before_release() {
    let Some(mut f) = Fixture::new() else { return };
    let source = f.troopers[0];
    let cleg = f
        .sim
        .spawn_object("CLEG", "Russians", 10, 11, 0, &f.rules)
        .unwrap();
    let owner = f.sim.substrate.entities.get(source).unwrap().owner();
    f.sim.fog.reveal_all_for_owner(owner);
    assert!(
        f.sim
            .acquire_adjacent_tesla_coil(source, &f.rules, None, FrameEffects::default(),)
    );
    f.hit(source, f.coil);
    f.sim.update_tesla_charge(f.coil, &f.rules);
    assert!(f.charge().is_overpowered());
    f.assign(cleg, Some(source));
    f.sim
        .temporal_initiate_warp(cleg, Some(source), &f.rules, None, FrameEffects::default());
    assert!(
        f.sim
            .temporal_ai_prologue(source, &f.rules, ObjectAiCtx::default(), &mut false,)
    );
    let actor = f.sim.substrate.entities.get(source).unwrap();
    assert!(actor.attack_target.target().is_none());
    // Infantry51BBEF calls its concrete +3C8 setter, whose reached tail
    // clears +68E at51B33F before the frozen object returns from AI.
    assert!(!actor.tesla_charger.is_active());
    f.sim.update_tesla_charge(f.coil, &f.rules);
    assert!(f.charge().chargers().is_empty());
    assert!(!f.charge().is_overpowered());

    f.sim.temporal_let_go(cleg);
    assert!(!f.sim.substrate.entities.get(source).unwrap().ai_frozen());
    f.dispatch(source, MissionType::Attack);
    assert!(
        f.sim
            .substrate
            .entities
            .get(source)
            .unwrap()
            .attack_target
            .target()
            .is_none(),
        "released Attack takes the ordinary no-target idle path, not the charger scan",
    );
}

#[test]
fn retail_fireat_charges_low_power_coil_and_pruning_disables_it_again() {
    use crate::sim::combat::world_fire::FireVisit;
    let Some(mut f) = Fixture::new() else { return };
    f.set_power(0, 100);
    for source in [f.troopers[0], f.troopers[1]] {
        f.assign(source, Some(f.coil));
        f.sim.commit_fire_visit(
            FireVisit::Direct {
                id: source,
                target: TargetKind::Entity(f.coil),
                weapon_index: 1,
            },
            &f.rules,
            None,
            FrameEffects::default(),
        );
    }
    assert_eq!(
        f.sim.projectiles.len(),
        2,
        "FireAt admits the two real Inviso bullets"
    );
    // Even Inviso detonates through its Bullet AI visit. Keep the unused
    // third trooper asleep while the production frame visits those bullets.
    f.dispatch(f.troopers[2], MissionType::Sleep);
    f.sim.advance_tick(&[], Some(&f.rules), None, None, 66);
    assert_eq!(
        f.charge().chargers(),
        &f.troopers[..2],
        "real SHK FireAt routes Inviso into ElectricAssault"
    );
    assert_eq!(
        f.sim.building_operational_state(f.coil, &f.rules),
        Some(true)
    );
    f.sim.update_tesla_charge(f.coil, &f.rules);
    assert!(
        !f.charge().is_overpowered(),
        "two at low power supply normal firing only"
    );
    f.assign(f.troopers[0], None);
    assert_eq!(
        f.sim.building_operational_state(f.coil, &f.rules),
        Some(true),
        "retained list is sampled before its later prune"
    );
    f.sim.update_tesla_charge(f.coil, &f.rules);
    assert_eq!(
        f.sim.building_operational_state(f.coil, &f.rules),
        Some(false)
    );
}
