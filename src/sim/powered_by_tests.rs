//! Ares PoweredBy=: PoweredUnitClass::Update1006A6F0 through the actual
//! Techno AI head, its EnableEMPEffect2/DisableEMPEffect2 transitions, the
//! ValidateThings invalidation and the Unit spark veto (hook736135).

use super::*;
use crate::sim::world::ObjectAiCtx;

fn scene() -> (Simulation, RuleSet, u64, u64) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[BuildingTypes]\n0=PWR\n[VehicleTypes]\n0=DRONE\n\
         [PWR]\nStrength=500\nPower=100\n\
         [DRONE]\nStrength=100\nSpeed=4\nPoweredBy=PWR\nPoweredUnit=yes\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Normal\n",
    );
    let rules = RuleSet::from_ini(&ini).unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 23);
    sim.intern_rule_type_ids(&rules);
    let pwr = sim
        .spawn_object("PWR", "Russians", 4, 4, 0, &rules)
        .unwrap();
    let drone = sim
        .spawn_object("DRONE", "Russians", 14, 14, 0, &rules)
        .unwrap();
    sim.resolve_type_handles(&rules);
    (sim, rules, pwr, drone)
}

fn visit(sim: &mut Simulation, rules: &RuleSet, id: u64, frame: u32) {
    sim.session.set_binary_frame_for_test(frame);
    sim.object_ai_visit_one(id, Some(rules), ObjectAiCtx::default());
}

#[test]
fn ares_powered_by_scans_every_fifteen_frames_and_toggles_through_the_shared_owners() {
    let (mut sim, rules, pwr, drone) = scene();
    // ValidateThings100677B0: PoweredBy replaces the stock PoweredUnit flag.
    assert!(!rules.object("DRONE").unwrap().powered_unit);
    assert_eq!(sim.building_operational_state(pwr, &rules), Some(true));
    visit(&mut sim, &rules, drone, 15);
    let state = sim
        .substrate
        .entities
        .get(drone)
        .unwrap()
        .powered_by
        .unwrap();
    assert_eq!((state.last_scan(), state.powered()), (15, true));

    sim.substrate
        .entities
        .get_mut(pwr)
        .unwrap()
        .set_building_online_for_test(false);
    visit(&mut sim, &rules, drone, 29);
    assert!(
        !sim.substrate
            .entities
            .get(drone)
            .unwrap()
            .deactivated
            .is_offline(),
        "fourteen frames after the last scan nothing is asked"
    );
    visit(&mut sim, &rules, drone, 30);
    let entity = sim.substrate.entities.get(drone).unwrap();
    assert!(entity.deactivated.is_offline());
    assert!(!entity.powered_by.unwrap().powered());
    assert!(!sim.ares_is_powered(drone, &rules));
    assert_eq!(
        entity.mission.current().known(),
        Some(MissionType::Sleep),
        "EnableEMPEffect2 queues and commences Sleep for a Foot"
    );

    sim.substrate
        .entities
        .get_mut(pwr)
        .unwrap()
        .set_building_online_for_test(true);
    visit(&mut sim, &rules, drone, 45);
    let entity = sim.substrate.entities.get(drone).unwrap();
    assert!(!entity.deactivated.is_offline());
    assert_eq!(
        entity.mission.queued().known(),
        Some(MissionType::Hunt),
        "DisableEMPEffect2 sends a computer house's unit hunting"
    );
}

#[test]
fn ares_deactivated_units_spark_only_without_power() {
    let (mut sim, rules, _, drone) = scene();
    sim.substrate
        .entities
        .get_mut(drone)
        .unwrap()
        .deactivated
        .set_for_test(true);
    let draws_over_a_cycle = |sim: &mut Simulation| {
        let before = sim.scenario_rng.logical_state();
        let mut changes = 0;
        for frame in 100..130 {
            sim.session.set_binary_frame_for_test(frame);
            let state = sim.scenario_rng.logical_state();
            sim.deactivated_unit_sparks(drone, &rules);
            changes += usize::from(sim.scenario_rng.logical_state() != state);
        }
        (changes, sim.scenario_rng.logical_state() != before)
    };
    // No scan yet: IsPowered100454F0 answers true, like an EMP victim.
    assert_eq!(draws_over_a_cycle(&mut sim), (0, false));
    sim.substrate.entities.get_mut(drone).unwrap().powered_by = Some(PoweredBySource {
        last_scan: 0,
        powered: false,
    });
    assert_eq!(draws_over_a_cycle(&mut sim), (1, true));
}

/// EnableEMPEffect2 (10062980) takes a Foot out of its team, selects a
/// selected victim again after Deactivate, and suspends its slave manager's
/// AI timer (start -1, duration kept); DisableEMPEffect2 (10062650) resumes
/// that timer when the power returns.
#[test]
fn ares_powered_by_power_loss_updates_team_selection_and_slaves() {
    let (mut sim, rules, pwr, drone) = scene();
    visit(&mut sim, &rules, drone, 15);
    crate::sim::team_script_vm::join_team_for_test(&mut sim, &[drone], false, false);
    let slave_type = sim.interner.intern("DRONE");
    let entity = sim.substrate.entities.get_mut(drone).unwrap();
    entity.selection.set_selected_for_test(true);
    entity.slave_manager = Some(Box::new(crate::sim::slave_manager::SlaveManager::new(
        slave_type,
        std::iter::empty(),
        10,
        10,
        30,
    )));
    sim.substrate
        .entities
        .get_mut(pwr)
        .unwrap()
        .set_building_online_for_test(false);
    visit(&mut sim, &rules, drone, 30);
    let entity = sim.substrate.entities.get(drone).unwrap();
    assert!(entity.deactivated.is_offline());
    assert!(
        entity.selection.is_selected(),
        "reselected after Deactivate"
    );
    assert!(sim.team_script_vm.team_for_member(drone).is_none());
    let timer = entity.slave_manager.as_ref().unwrap().ai_timer();
    assert_eq!((timer.start_frame(), timer.duration()), (-1, 10));

    sim.substrate
        .entities
        .get_mut(pwr)
        .unwrap()
        .set_building_online_for_test(true);
    visit(&mut sim, &rules, drone, 45);
    let entity = sim.substrate.entities.get(drone).unwrap();
    assert!(!entity.deactivated.is_offline());
    let timer = entity.slave_manager.as_ref().unwrap().ai_timer();
    assert_eq!((timer.start_frame(), timer.duration()), (45, 10));
}
