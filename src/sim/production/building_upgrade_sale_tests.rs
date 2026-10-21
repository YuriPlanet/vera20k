//! Selling's upgrade branch through the live building visit. Native control
//! flow: Steam44AAB5..44AB59, ordinary status write44ABAC, Queue5B35F7.
//! Refund arithmetic remains covered by building_sale_oracle_tests.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::power_system::PowerState;

fn sale_world() -> (Simulation, RuleSet, InternedId) {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nRefundPercent=50%\n[Countries]\n0=C0\n\
         [BuildingTypes]\n0=HOST\n1=UP_A\n2=UP_B\n\
         [HOST]\nStrength=1000\nCost=800\nUpgrades=3\n\
         [UP_A]\nStrength=100\nCost=200\nPowersUpBuilding=HOST\n\
         [UP_B]\nStrength=100\nCost=400\nPowersUpBuilding=HOST\n",
    ))
    .unwrap();
    rules.set_buildup_control_for_test("HOST", [0, 4, 1]);
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let owner = sim.interner.intern("H0");
    let country = sim.interner.intern("C0");
    sim.houses
        .register(HouseState::new(owner, 0, Some(country), true, 1000, 10));
    crate::sim::production::tests::spawn_structure(&mut sim, &rules, 1, "H0", "HOST", 10, 10);
    // A supplied damage-fire ownership slot isolates stage zero's cleanup:
    // upgrade visits retain it; the ordinary host-sale tail clears it.
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .damage_fires
        .set_slot_for_test(0, Some(99));
    *sim.power_states.house_mut(owner) = PowerState::settled_for_test();
    (sim, rules, owner)
}

#[test]
fn selling_visits_refund_one_upgrade_each_before_the_host_sale_tail() {
    let (mut sim, rules, owner) = sale_world();
    let a = sim.interner.get("UP_A").unwrap();
    let b = sim.interner.get("UP_B").unwrap();
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_building_upgrades_for_test(2, [Some(a), Some(b), None]);
    begin_selling(&mut sim, &rules, 1, false);
    let next_id = sim.next_stable_object_id();
    let native_id = sim.native_identity_cursor().unwrap();
    let mut constructor_rng = sim.clone_scenario_rng();
    for (frame, count, credits, types) in
        [(1, 1, 1200, [Some(a), None, None]), (2, 0, 1300, [None; 3])]
    {
        constructor_rng.next_u32();
        sim.session.set_binary_frame_for_test(frame);
        sim.visit_building_down(1, &rules, None, FrameEffects::default());
        let host = sim.entities().get(1).unwrap();
        assert_eq!(host.building_upgrades().count(), count);
        assert_eq!(*host.building_upgrades().types(), types);
        assert_eq!(host.health.current(), 1000);
        assert!(host.is_object_alive() && !host.lifecycle.in_limbo());
        assert_eq!(host.mission.current().known(), Some(MissionType::Selling));
        assert_eq!(host.mission.queued(), MissionId::NONE);
        assert_eq!(host.mission.handler_state(), 0, "Guard Queue is refused");
        assert_eq!(host.damage_fires.anim_ids()[0], Some(99));
        assert_eq!(sim.houses[&owner].economy().credits(), credits);
        assert_eq!(
            sim.sound_events
                .iter()
                .filter(|event| matches!(event, SimSoundEvent::StructureSold { owner: speaker } if *speaker == owner))
                .count(),
            frame as usize
        );
        assert_eq!(sim.next_stable_object_id(), next_id + u64::from(frame));
        assert_eq!(sim.native_identity_cursor(), Some(native_id + frame));
        assert_eq!(
            sim.scenario_rng().logical_state(),
            constructor_rng.logical_state()
        );
        assert!(!sim.entities().contains(next_id + u64::from(frame) - 1));
    }
    let house = &sim.houses[&owner];
    assert_eq!(house.base_projection().buildings(), [1]);
    assert_eq!(house.stats().buildings_lost(), 0);
    for upgrade in [a, b] {
        assert_eq!(
            house
                .tracking()
                .owned_count(EntityCategory::Structure, upgrade),
            0
        );
        assert_eq!(
            house
                .tracking()
                .active_count(EntityCategory::Structure, upgrade),
            0
        );
    }
    assert!(sim.substrate.pending_delete.object_ids().is_empty());
    assert!(sim.power_states[&owner].power_recheck_for_test());
    assert!(sim.power_states[&owner].radar_recheck_for_test());

    *sim.power_states.house_mut(owner) = PowerState::settled_for_test();
    sim.session.set_binary_frame_for_test(3);
    sim.visit_building_down(1, &rules, None, FrameEffects::default());
    let host = sim.entities().get(1).unwrap();
    assert_eq!(
        host.mission.handler_state(),
        1,
        "empty stack reaches the ordinary tail"
    );
    assert_eq!(host.damage_fires.anim_ids()[0], None);
    assert_eq!(sim.houses[&owner].economy().credits(), 1300);
    assert_eq!(sim.next_stable_object_id(), next_id + 2);
    assert!(!sim.power_states[&owner].power_recheck_for_test());
    assert!(!sim.power_states[&owner].radar_recheck_for_test());
}

#[test]
fn selling_a_sparse_top_slot_stays_safe_without_compacting_or_refunding() {
    let (mut sim, rules, owner) = sale_world();
    let a = sim.interner.get("UP_A").unwrap();
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_building_upgrades_for_test(3, [Some(a), None, None]);
    begin_selling(&mut sim, &rules, 1, false);
    let next_id = sim.next_stable_object_id();
    let native_id = sim.native_identity_cursor();
    let rng = sim.scenario_rng().logical_state();
    for frame in [1, 2] {
        sim.session.set_binary_frame_for_test(frame);
        sim.visit_building_down(1, &rules, None, FrameEffects::default());
    }
    let host = sim.entities().get(1).unwrap();
    assert_eq!(host.building_upgrades().count(), 3);
    assert_eq!(*host.building_upgrades().types(), [Some(a), None, None]);
    assert_eq!(host.mission.handler_state(), 0);
    assert_eq!(host.damage_fires.anim_ids()[0], Some(99));
    assert_eq!(sim.houses[&owner].economy().credits(), 1000);
    assert_eq!(sim.next_stable_object_id(), next_id);
    assert_eq!(sim.native_identity_cursor(), native_id);
    assert_eq!(sim.scenario_rng().logical_state(), rng);
    assert!(!sim.power_states[&owner].power_recheck_for_test());
    assert!(!sim.power_states[&owner].radar_recheck_for_test());
}
