//! Focused GSI-04.18 persisted-shroud and aggregate SpySat contracts.

use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{BuildingUp, Health};
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};

fn spy_sat_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=GASPYSAT\n1=GAGAP\n\
         [GASPYSAT]\nName=Spy Satellite\nSpySat=yes\nPowered=yes\nPower=-100\n\
         Strength=100\nCost=1000\nFoundation=1x1\n\
         [GAGAP]\nName=Gap Generator\nGapGenerator=yes\nGapRadiusInCells=3\n\
         Powered=yes\nPower=-100\nStrength=100\nCost=1000\nFoundation=1x1\n",
    );
    RuleSet::from_ini(&ini).expect("GSI-04.18 rules")
}

fn fixture() -> (Simulation, RuleSet, InternedId) {
    let mut sim = Simulation::with_seed(0x418);
    sim.fog.width = 24;
    sim.fog.height = 24;
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 10_000, 10));
    sim.session.house_order.push(owner);
    (sim, spy_sat_rules(), owner)
}

fn insert_structure(
    sim: &mut Simulation,
    stable_id: u64,
    owner: InternedId,
    type_name: &str,
    rx: u16,
) {
    let type_ref = sim.interner.intern(type_name);
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        stable_id,
        rx,
        12,
        0,
        0,
        owner,
        Health { current: 100 },
        type_ref,
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    entity.lifecycle.in_limbo = false;
    entity.lifecycle.cell_marked = false;
    sim.substrate.next_stable_object_id = sim.substrate.next_stable_object_id.max(stable_id + 1);
    sim.substrate.entities.insert(entity);
    sim.add_entity_occupancy(stable_id);
}

fn insert_sight_unit(sim: &mut Simulation, stable_id: u64, owner: InternedId, rx: u16, ry: u16) {
    let type_ref = sim.interner.intern("GSI418SIGHT");
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        stable_id,
        rx,
        ry,
        0,
        0,
        owner,
        Health { current: 100 },
        type_ref,
        EntityCategory::Unit,
        0,
        3,
        false,
    );
    entity.lifecycle.in_limbo = false;
    entity.lifecycle.cell_marked = false;
    sim.substrate.next_stable_object_id = sim.substrate.next_stable_object_id.max(stable_id + 1);
    sim.substrate.entities.insert(entity);
    sim.add_entity_occupancy(stable_id);
}

fn set_missions(entity: &mut GameEntity, current: MissionId, queued: MissionId) {
    entity.mission.apply_test_fixture(MissionTestFixture {
        current,
        suspended: MissionId::NONE,
        queued,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    });
}

#[test]
fn gsi_04_18_two_uplinks_reshroud_only_after_the_last_provider_is_concealed() {
    let (mut sim, rules, owner) = fixture();
    insert_structure(&mut sim, 1, owner, "GASPYSAT", 6);
    insert_structure(&mut sim, 2, owner, "GASPYSAT", 8);
    let scenario_rng_before = sim.scenario_rng.state();
    let main_rng_before = sim.main_rng.state();

    sim.reconcile_active_vision_structures(&rules);
    assert!(sim.houses[&owner].spy_sat_active);
    assert!(sim.houses[&owner].map_is_clear);
    assert!(sim.fog.is_cell_revealed(owner, 23, 23));

    sim.uninit(1);
    sim.reconcile_active_vision_structures(&rules);
    assert!(sim.houses[&owner].spy_sat_active);
    assert!(sim.houses[&owner].map_is_clear);
    assert!(sim.fog.is_cell_revealed(owner, 23, 23));

    sim.uninit(2);
    sim.reconcile_active_vision_structures(&rules);
    assert!(!sim.houses[&owner].spy_sat_active);
    assert!(!sim.houses[&owner].map_is_clear);
    assert!(!sim.fog.is_cell_revealed(owner, 23, 23));
    assert_eq!(sim.scenario_rng.state(), scenario_rng_before);
    assert_eq!(sim.main_rng.state(), main_rng_before);
}

#[test]
fn gsi_04_18_last_uplink_loss_preserves_surviving_techno_sight() {
    let (mut sim, rules, owner) = fixture();
    insert_structure(&mut sim, 1, owner, "GASPYSAT", 6);
    insert_sight_unit(&mut sim, 2, owner, 4, 4);
    sim.reconcile_active_vision_structures(&rules);
    sim.refresh_fog(None, &vision::VisionConfig::default(), Some(&rules));
    assert!(sim.fog.is_cell_visible(owner, 4, 4));
    assert!(sim.fog.is_cell_revealed(owner, 23, 23));
    assert!(!sim.fog.is_cell_visible(owner, 23, 23));
    let scenario_rng_before = sim.scenario_rng.state();
    let main_rng_before = sim.main_rng.state();

    sim.uninit(1);
    sim.reconcile_active_vision_structures(&rules);

    assert!(!sim.houses[&owner].spy_sat_active);
    assert!(!sim.houses[&owner].map_is_clear);
    assert!(!sim.fog.is_cell_revealed(owner, 23, 23));
    assert!(sim.fog.is_cell_visible(owner, 4, 4));
    assert!(sim.fog.is_cell_revealed(owner, 4, 4));
    assert_eq!(sim.scenario_rng.state(), scenario_rng_before);
    assert_eq!(sim.main_rng.state(), main_rng_before);
}

#[test]
fn gsi_04_18_spy_sat_candidate_is_independent_of_low_or_offline_power() {
    let (mut sim, rules, owner) = fixture();
    insert_structure(&mut sim, 1, owner, "GASPYSAT", 6);
    let power = sim.power_states.entry(owner).or_default();
    power.is_low_power = true;
    power.power_blackout_remaining = 30;

    sim.reconcile_active_vision_structures(&rules);

    assert!(sim.houses[&owner].spy_sat_active);
    assert!(sim.houses[&owner].map_is_clear);
    assert!(sim.fog.is_cell_revealed(owner, 23, 23));
}

#[test]
fn gsi_04_18_sale_waits_for_the_next_house_rung_before_reshrouding() {
    let (mut sim, rules, owner) = fixture();
    insert_structure(&mut sim, 1, owner, "GASPYSAT", 6);
    sim.reconcile_active_vision_structures(&rules);
    assert!(sim.fog.is_cell_revealed(owner, 23, 23));

    assert!(crate::sim::production::sell_building(&mut sim, &rules, 1));
    assert!(sim.houses[&owner].spy_sat_active);
    assert!(sim.houses[&owner].map_is_clear);
    assert!(
        sim.fog.is_cell_revealed(owner, 23, 23),
        "EventClass sale must not bypass the earlier House-rung aggregate scan"
    );

    sim.reconcile_active_vision_structures(&rules);
    assert!(!sim.houses[&owner].spy_sat_active);
    assert!(!sim.houses[&owner].map_is_clear);
    assert!(!sim.fog.is_cell_revealed(owner, 23, 23));
}

#[test]
fn gsi_04_18_first_warping_candidate_blocks_later_uplink_but_selling_is_skipped() {
    let (mut sim, rules, owner) = fixture();
    insert_structure(&mut sim, 1, owner, "GASPYSAT", 6);
    insert_structure(&mut sim, 2, owner, "GASPYSAT", 8);
    sim.substrate.entities.get_mut(2).unwrap().building_up =
        Some(BuildingUp::completing_in_ticks(20, 0));

    sim.reconcile_active_vision_structures(&rules);
    assert!(sim.houses[&owner].spy_sat_active);
    assert!(sim.houses[&owner].map_is_clear);

    sim.substrate.entities.get_mut(1).unwrap().teleport_state = Some(TeleportState {
        phase: TeleportPhase::Relocate,
        target_rx: 10,
        target_ry: 10,
        being_warped_ticks: 0,
    });

    sim.reconcile_active_vision_structures(&rules);
    assert!(!sim.houses[&owner].spy_sat_active);
    assert!(!sim.houses[&owner].map_is_clear);

    let selling = MissionId::from_known(MissionType::Selling);
    set_missions(
        sim.substrate.entities.get_mut(1).unwrap(),
        MissionId::NONE,
        selling,
    );
    sim.reconcile_active_vision_structures(&rules);
    assert!(
        sim.houses[&owner].spy_sat_active,
        "queued Selling skips the first uplink, and BuildingUp does not exclude the second"
    );

    set_missions(
        sim.substrate.entities.get_mut(1).unwrap(),
        selling,
        MissionId::NONE,
    );
    sim.reconcile_active_vision_structures(&rules);
    assert!(
        sim.houses[&owner].spy_sat_active,
        "current Selling also skips to the later eligible uplink"
    );
}

#[test]
fn gsi_04_18_house_rung_applies_spy_sat_before_gap_and_recovers_after_gap_conceal() {
    let (mut sim, rules, owner) = fixture();
    let gapper = sim.interner.intern("Soviet");
    insert_structure(&mut sim, 1, owner, "GASPYSAT", 6);
    insert_structure(&mut sim, 2, gapper, "GAGAP", 12);
    sim.visit_building_operational(2, &rules);

    sim.reconcile_active_vision_structures(&rules);
    assert!(sim.fog.is_cell_revealed(owner, 23, 23));
    assert!(!sim.fog.is_cell_revealed(owner, 12, 12));
    assert!(sim.fog.is_cell_gap_covered(owner, 12, 12));

    sim.uninit(2);
    sim.fog.mark_visible_for_owner(owner, 3, 3);
    sim.reconcile_active_vision_structures(&rules);
    assert!(sim.houses[&owner].spy_sat_active);
    assert!(sim.fog.is_cell_revealed(owner, 12, 12));
    assert!(!sim.fog.is_cell_gap_covered(owner, 12, 12));
    assert!(
        sim.fog.is_cell_visible(owner, 3, 3),
        "House-rung Gap replacement must preserve Phase-3 line of sight"
    );
}

#[test]
fn gsi_04_18_spy_sat_latch_and_persisted_fog_are_hash_authority() {
    let (mut sim, _rules, owner) = fixture();
    let baseline = sim.state_hash();
    sim.houses.get_mut(&owner).unwrap().spy_sat_active = true;
    let latch_hash = sim.state_hash();
    assert_ne!(latch_hash, baseline);
    sim.houses.get_mut(&owner).unwrap().spy_sat_active = false;
    assert_eq!(sim.state_hash(), baseline);

    sim.fog.reveal_all_for_owner(owner);
    let revealed_hash = sim.state_hash();
    let gapper = sim.interner.intern("Soviet");
    crate::sim::vision::apply_gap_generators(&mut sim.fog, &[(gapper, 12, 12, 3)], &sim.interner);
    assert_ne!(sim.state_hash(), revealed_hash);
}

#[test]
fn shroud_current_sight_world_collector_and_native_frame_restore() {
    use crate::sim::snapshot::GameSnapshot;
    let (mut sim, rules, owner) = fixture();
    let gapper = sim.interner.intern("Soviet");
    insert_structure(&mut sim, 1, gapper, "GAGAP", 12);
    sim.visit_building_operational(1, &rules);
    insert_sight_unit(&mut sim, 2, owner, 12, 12);
    sim.refresh_fog(None, &vision::VisionConfig::default(), Some(&rules));
    sim.reconcile_active_vision_structures(&rules);
    assert!(sim.fog.is_cell_visible(owner, 12, 12));
    assert!(!sim.fog.is_cell_gap_covered(owner, 12, 12));
    // Actual entity-source departure occurs after an early frame120 sweep.
    sim.session.binary_frame = 120;
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .lifecycle
        .in_limbo = true;
    sim.advance_tick(&[], None, &BTreeMap::new(), None, None, 67);
    assert!(
        sim.fog.is_cell_revealed(owner, 12, 12),
        "current-pass departure waits for the next boundary"
    );
    sim.reconcile_active_vision_structures(&rules);
    sim.uninit(1);
    sim.reconcile_active_vision_structures(&rules); // pending survives generator removal
    // Native Scenario deserialization resets only this RNG to Seed(0).
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let hash = sim.state_hash();
    let bytes = GameSnapshot::save(&sim, 0, 0, "shroud-current-sight", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    assert_eq!(restored.state_hash(), hash);
    restored.restore_after_snapshot_load().unwrap();
    restored.session.binary_frame = 239;
    restored.advance_tick(&[], None, &BTreeMap::new(), None, None, 67);
    assert!(restored.fog.is_cell_revealed(owner, 12, 12));
    restored.advance_tick(&[], None, &BTreeMap::new(), None, None, 67);
    assert!(
        !restored.fog.is_cell_revealed(owner, 12, 12),
        "frame240 consumes persisted pending conceal before object work"
    );
}

#[test]
fn shroud_current_sight_psychic_mapping_does_not_gain_gap_immunity() {
    let (mut sim, rules, owner) = fixture();
    let gapper = sim.interner.intern("Soviet");
    let sw = sim.interner.intern("PsychicRevealSpecial");
    assert!(crate::sim::superweapon::psychic_reveal::launch(
        &mut sim, &rules, owner, 12, 12, sw
    ));
    assert!(sim.fog.is_cell_revealed(owner, 12, 12));
    insert_structure(&mut sim, 1, gapper, "GAGAP", 12);
    sim.visit_building_operational(1, &rules);
    sim.reconcile_active_vision_structures(&rules);
    assert!(!sim.fog.is_cell_revealed(owner, 12, 12));
}

#[test]
fn shroud_current_sight_new_generator_identity_consumes_pending() {
    let (mut sim, rules, owner) = fixture();
    let gapper = sim.interner.intern("Soviet");
    insert_structure(&mut sim, 1, gapper, "GAGAP", 12);
    sim.visit_building_operational(1, &rules);
    insert_sight_unit(&mut sim, 2, owner, 12, 12);
    sim.refresh_fog(None, &vision::VisionConfig::default(), Some(&rules));
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .lifecycle
        .in_limbo = true;
    sim.refresh_fog(None, &vision::VisionConfig::default(), Some(&rules));
    sim.reconcile_active_vision_structures(&rules);
    assert!(
        sim.fog.is_cell_revealed(owner, 12, 12),
        "same generator is materialized twice, not activated twice"
    );
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "gap-admissions", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.reconcile_active_vision_structures(&rules);
    assert!(
        restored.fog.is_cell_revealed(owner, 12, 12),
        "restored admission must not become a fresh native write"
    );
    // Same owner/geometry, different stable identity: replacement is a new write.
    restored.uninit(1);
    insert_structure(&mut restored, 3, gapper, "GAGAP", 12);
    restored.visit_building_operational(3, &rules);
    restored.reconcile_active_vision_structures(&rules);
    assert!(!restored.fog.is_cell_revealed(owner, 12, 12));
}

#[test]
fn shroud_current_sight_psychic_under_existing_gap_waits_for_native_boundary() {
    let (mut sim, rules, owner) = fixture();
    let gapper = sim.interner.intern("Soviet");
    let sw = sim.interner.intern("PsychicRevealSpecial");
    sim.fog.reveal_all_for_owner(owner);
    insert_structure(&mut sim, 1, gapper, "GAGAP", 12);
    sim.visit_building_operational(1, &rules);
    sim.reconcile_active_vision_structures(&rules);
    assert!(!sim.fog.is_cell_revealed(owner, 12, 12));
    assert!(crate::sim::superweapon::psychic_reveal::launch(
        &mut sim, &rules, owner, 12, 12, sw
    ));
    sim.reconcile_active_vision_structures(&rules);
    assert!(sim.fog.is_cell_revealed(owner, 12, 12));
    assert!(!sim.fog.is_cell_gap_covered(owner, 12, 12));
    sim.session.binary_frame = 119;
    sim.advance_tick(&[], None, &BTreeMap::new(), None, None, 67);
    assert!(sim.fog.is_cell_revealed(owner, 12, 12));
    sim.advance_tick(&[], None, &BTreeMap::new(), None, None, 67);
    assert!(!sim.fog.is_cell_revealed(owner, 12, 12));
}

#[test]
fn shroud_current_sight_fresh_direct_allied_psychic_reaches_authoritative_view() {
    let (mut sim, rules, a) = fixture();
    let b = sim.interner.intern("Alliance");
    let c = sim.interner.intern("Soviet");
    let gapper = sim.interner.intern("Neutral");
    let sw = sim.interner.intern("PsychicRevealSpecial");
    for (left, right) in [
        ("AMERICANS", "ALLIANCE"),
        ("ALLIANCE", "AMERICANS"),
        ("ALLIANCE", "SOVIET"),
        ("SOVIET", "ALLIANCE"),
    ] {
        sim.house_alliances
            .entry(left.into())
            .or_default()
            .insert(right.into());
    }
    // Match the production alliance authority and its fog projection.
    sim.fog.alliances = sim.house_alliances.clone();
    for owner in [a, b, c] {
        sim.fog.reveal_all_for_owner(owner);
    }
    insert_structure(&mut sim, 1, gapper, "GAGAP", 12);
    sim.visit_building_operational(1, &rules);
    sim.reconcile_active_vision_structures(&rules);
    assert!(!sim.fog.is_cell_revealed(a, 12, 12));
    assert!(crate::sim::superweapon::psychic_reveal::launch(
        &mut sim, &rules, a, 12, 12, sw
    ));
    sim.reconcile_active_vision_structures(&rules);
    sim.fog.build_merged_for(b, &sim.interner);
    assert!(sim.fog.is_cell_revealed(b, 12, 12));
    sim.fog.build_merged_for(c, &sim.interner);
    assert!(
        !sim.fog.is_cell_revealed(c, 12, 12),
        "A's fresh reveal may reach B, never B's ally C"
    );
    assert!(crate::sim::superweapon::psychic_reveal::launch(
        &mut sim, &rules, b, 12, 12, sw
    ));
    sim.reconcile_active_vision_structures(&rules);
    sim.fog.build_merged_for(a, &sim.interner);
    assert!(
        sim.fog.is_cell_revealed(a, 12, 12),
        "fresh direct ally mapping opens the current gap"
    );
    assert!(!sim.fog.is_cell_gap_covered(a, 12, 12));
    sim.fog.flush_pending_gap_conceal(120);
    sim.uninit(1);
    sim.reconcile_active_vision_structures(&rules);
    sim.fog.build_merged_for(a, &sim.interner);
    assert!(!sim.fog.is_cell_revealed(a, 12, 12));
    assert!(crate::sim::superweapon::psychic_reveal::launch(
        &mut sim, &rules, b, 12, 12, sw
    ));
    sim.reconcile_active_vision_structures(&rules);
    sim.fog.build_merged_for(a, &sim.interner);
    assert!(
        sim.fog.is_cell_revealed(a, 12, 12),
        "fresh direct ally mapping also reaches retained post-gap knowledge authority"
    );
}

#[test]
fn shroud_current_sight_psychic_never_gapped_and_mixed_views_are_nontransitive() {
    for with_gap in [false, true] {
        let (mut sim, rules, a) = fixture();
        let b = sim.interner.intern("Alliance");
        let c = sim.interner.intern("Soviet");
        let gapper = sim.interner.intern("Neutral");
        let sw = sim.interner.intern("PsychicRevealSpecial");
        for (left, right) in [
            ("AMERICANS", "ALLIANCE"),
            ("ALLIANCE", "AMERICANS"),
            ("ALLIANCE", "SOVIET"),
            ("SOVIET", "ALLIANCE"),
        ] {
            sim.house_alliances
                .entry(left.into())
                .or_default()
                .insert(right.into());
        }
        // Match the production alliance authority and its fog projection.
        sim.fog.alliances = sim.house_alliances.clone();
        for owner in [a, b, c] {
            sim.fog.mark_visible_for_owner(owner, 0, 0);
        }
        if with_gap {
            insert_structure(&mut sim, 1, gapper, "GAGAP", 12);
            sim.visit_building_operational(1, &rules);
        }
        sim.reconcile_active_vision_structures(&rules);
        assert!(crate::sim::superweapon::psychic_reveal::launch(
            &mut sim, &rules, a, 16, 12, sw
        ));
        for _ in 0..3 {
            sim.reconcile_active_vision_structures(&rules);
            sim.fog.build_merged_for(b, &sim.interner);
            for x in [12, 20] {
                assert!(sim.fog.is_cell_revealed(b, x, 12));
                assert!(sim.fog.is_cell_visible(b, x, 12));
            }
            sim.fog.build_merged_for(c, &sim.interner);
            for x in [12, 20] {
                assert!(
                    !sim.fog.is_cell_revealed(c, x, 12),
                    "with_gap={with_gap},cell={x}: derived B knowledge is never a fresh B source"
                );
                assert!(!sim.fog.is_cell_visible(c, x, 12));
            }
        }
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "viewer-knowledge", 0);
        let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        restored.fog.build_merged_for(c, &restored.interner);
        assert!(!restored.fog.is_cell_revealed(c, 20, 12));
        restored.fog.build_merged_for(b, &restored.interner);
        assert!(restored.fog.is_cell_revealed(b, 20, 12));
    }
}

#[test]
fn shroud_current_sight_live_foot_timer_keeps_viewer_histories_and_snapshot() {
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
    use crate::sim::timer::CdTimer;
    let (mut sim, rules, a) = fixture();
    let b = sim.interner.intern("Alliance");
    let c = sim.interner.intern("Third");
    let reverse = sim.interner.intern("ReverseOnly");
    sim.house_alliances
        .entry("REVERSEONLY".into())
        .or_default()
        .insert("AMERICANS".into());
    let gapper = sim.interner.intern("Soviet");
    for (left, right) in [("Americans", "Alliance"), ("Alliance", "Americans")] {
        sim.house_alliances
            .entry(left.to_ascii_uppercase())
            .or_default()
            .insert(right.to_ascii_uppercase());
    }
    sim.fog.reveal_all_for_owner(b);
    sim.fog.reveal_all_for_owner(c);
    sim.fog.reveal_all_for_owner(reverse);
    insert_sight_unit(&mut sim, 2, a, 13, 13);
    {
        let entity = sim.substrate.entities.get_mut(2).unwrap();
        entity.in_playfield = true;
        let mut loco = LocomotorState::for_test_kind(LocomotorKind::Jumpjet);
        loco.layer = MovementLayer::Air;
        loco.altitude = crate::util::fixed_math::SimFixed::from_num(208);
        // Keep the admitted high-flight height through the actual Process below.

        let runtime = loco.jumpjet_runtime_mut().expect("jumpjet runtime");
        runtime.phase = crate::sim::movement::jumpjet_flight::STATE_TRANSLATE;
        // The kernel flies toward its own retained target height.
        runtime.flight.target_height = 208;
        entity.locomotor = Some(loco);
    }
    sim.refresh_fog(None, &vision::VisionConfig::default(), Some(&rules));
    insert_structure(&mut sim, 1, gapper, "GAGAP", 12);
    sim.visit_building_operational(1, &rules);
    sim.reconcile_active_vision_structures(&rules);
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .lifecycle
        .in_limbo = true;
    sim.refresh_fog(None, &vision::VisionConfig::default(), Some(&rules));
    insert_structure(&mut sim, 3, gapper, "GAGAP", 12);
    sim.visit_building_operational(3, &rules);
    sim.reconcile_active_vision_structures(&rules);
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .lifecycle
        .in_limbo = false;
    sim.refresh_fog(None, &vision::VisionConfig::default(), Some(&rules));
    {
        let clocks = &mut sim
            .substrate
            .entities
            .get_mut(2)
            .unwrap()
            .sight_refresh_timers;
        clocks.by_viewer.insert(a, CdTimer::started(104, 15));
        clocks.by_viewer.insert(b, CdTimer::started(119, 15));
    }
    // Native Scenario deserialization resets only this RNG to Seed(0).
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let hash = sim.state_hash();
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "foot-sight-timer", 0);
    let mut sim = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    assert_eq!(sim.state_hash(), hash);
    // Passive House and restore/cache materialization cannot consume either clock.
    sim.reconcile_active_vision_structures(&rules);
    sim.fog.build_merged_for(a, &sim.interner);
    assert_eq!(
        sim.substrate
            .entities
            .get(2)
            .unwrap()
            .sight_refresh_timers
            .timer(a),
        CdTimer::started(104, 15)
    );
    sim.session.binary_frame = 119;
    sim.set_logic_order_for_test(vec![2]);
    sim.advance_live_object_pass(None, None, None)
        .expect("fixture frame must complete");
    let clocks = &sim.substrate.entities.get(2).unwrap().sight_refresh_timers;
    assert_eq!(clocks.timer(a), CdTimer::started(119, 15));
    assert_eq!(clocks.timer(b), CdTimer::started(119, 15));
    assert!(
        !clocks.by_viewer.contains_key(&c),
        "nonallied viewer never borrows another viewer's countdown"
    );
    assert!(
        !clocks.by_viewer.contains_key(&reverse),
        "reverse-only alliance must not consume source-house timer gate"
    );
    sim.fog.flush_pending_gap_conceal(120);
    assert!(
        sim.fog.is_cell_revealed(a, 12, 12),
        "due same-footprint release/admit canceled pending before frame120"
    );
    assert!(
        !sim.fog.is_cell_revealed(b, 12, 12),
        "not-due viewer retains original pending and conceals"
    );
    assert!(
        !sim.fog.is_cell_revealed(reverse, 12, 12),
        "reverse-only viewer's pending is not canceled by the timer event"
    );
    // A newly direct allied viewer still has its constructor-equivalent due
    // timer, even though other viewers reloaded on this same binary frame.
    for (left, right) in [("Americans", "Third"), ("Third", "Americans")] {
        sim.house_alliances
            .entry(left.to_ascii_uppercase())
            .or_default()
            .insert(right.to_ascii_uppercase());
    }
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .jumpjet_runtime_mut()
        .unwrap()
        .phase = crate::sim::movement::jumpjet_flight::STATE_TRANSLATE;
    sim.refresh_high_flying_sight_before_process(2, None, None);
    assert_eq!(
        sim.substrate
            .entities
            .get(2)
            .unwrap()
            .sight_refresh_timers
            .timer(c),
        CdTimer::started(119, 15)
    );
    assert!(sim.fog.is_cell_revealed(c, 12, 12));
}

#[test]
fn shroud_current_sight_live_refresh_gates_preserve_or_reload_native_timer() {
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
    use crate::sim::timer::CdTimer;
    use crate::util::fixed_math::SimFixed;
    let (mut sim, _rules, owner) = fixture();
    insert_sight_unit(&mut sim, 2, owner, 13, 13);
    let mut loco = LocomotorState::for_test_kind(LocomotorKind::Fly);
    loco.layer = MovementLayer::Air;
    loco.altitude = SimFixed::from_num(207);
    loco.fly_current_speed = SimFixed::from_num(1);
    sim.substrate.entities.get_mut(2).unwrap().locomotor = Some(loco);
    sim.session.binary_frame = 119;
    sim.refresh_high_flying_sight_before_process(2, None, None);
    assert_eq!(
        sim.substrate
            .entities
            .get(2)
            .unwrap()
            .sight_refresh_timers
            .timer(owner),
        CdTimer::started(0, 0),
        "height below208 rejects without touching the clock"
    );
    {
        let loco = sim
            .substrate
            .entities
            .get_mut(2)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap();
        loco.altitude = SimFixed::from_num(208);
        loco.fly_current_speed = SimFixed::from_num(0);
    }
    sim.refresh_high_flying_sight_before_process(2, None, None);
    assert_eq!(
        sim.substrate
            .entities
            .get(2)
            .unwrap()
            .sight_refresh_timers
            .timer(owner),
        CdTimer::started(0, 0),
        "Fly current speed zero rejects; commanded speed is irrelevant"
    );
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .fly_current_speed = SimFixed::from_num(-1);
    // Membership is false: the admitted FootAI event calls leaves which reject,
    // then still reloads15. Native moving compares !=0, so negative speed admits.
    sim.refresh_high_flying_sight_before_process(2, None, None);
    assert_eq!(
        sim.substrate
            .entities
            .get(2)
            .unwrap()
            .sight_refresh_timers
            .timer(owner),
        CdTimer::started(119, 15)
    );
    assert!(sim.fog.sight_admissions.is_empty());
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .lifecycle
        .in_limbo = true;
    sim.session.binary_frame = 134;
    sim.refresh_high_flying_sight_before_process(2, None, None);
    assert_eq!(
        sim.substrate
            .entities
            .get(2)
            .unwrap()
            .sight_refresh_timers
            .timer(owner),
        CdTimer::started(119, 15),
        "temporary limbo preserves the due clock"
    );
    {
        let entity = sim.substrate.entities.get_mut(2).unwrap();
        entity.lifecycle.in_limbo = false;
        entity.in_playfield = true;
        entity.vision_range = 0;
    }
    sim.refresh_high_flying_sight_before_process(2, None, None);
    assert_eq!(
        sim.substrate
            .entities
            .get(2)
            .unwrap()
            .sight_refresh_timers
            .timer(owner),
        CdTimer::started(134, 15)
    );
    assert!(
        sim.fog.sight_admissions[&(2, owner)].cells.is_empty(),
        "zero radius still admits the source latch"
    );
}

#[test]
fn shroud_current_sight_spy_sat_event_preserves_registration_order_and_restore() {
    let native: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/shroud_current_sight.json"
    ))
    .unwrap();
    for gap_first in [false, true] {
        let (mut sim, rules, owner) = fixture();
        let gapper = sim.interner.intern("Soviet");
        let (source, first_gap) = if gap_first { (2, 1) } else { (1, 2) };
        insert_sight_unit(&mut sim, source, owner, 12, 12);
        sim.refresh_fog(None, &vision::VisionConfig::default(), Some(&rules));
        insert_structure(&mut sim, first_gap, gapper, "GAGAP", 12);
        sim.visit_building_operational(first_gap, &rules);
        sim.reconcile_active_vision_structures(&rules);
        sim.substrate
            .entities
            .get_mut(source)
            .unwrap()
            .lifecycle
            .in_limbo = true;
        sim.refresh_fog(None, &vision::VisionConfig::default(), Some(&rules));
        insert_structure(&mut sim, 3, gapper, "GAGAP", 12);
        sim.visit_building_operational(3, &rules);
        sim.reconcile_active_vision_structures(&rules);
        sim.substrate
            .entities
            .get_mut(source)
            .unwrap()
            .lifecycle
            .in_limbo = false;
        sim.refresh_fog(None, &vision::VisionConfig::default(), Some(&rules));
        // Actual House transition invokes the map bracket before577A changes.
        // This uplink's own sight is outside the observed cell.
        insert_structure(&mut sim, 4, owner, "GASPYSAT", 2);
        sim.reconcile_active_vision_structures(&rules);
        assert!(sim.houses[&owner].spy_sat_active);
        assert!(
            !sim.fog.whole_map_revealed_owners.contains(&owner),
            "successful gap readmission clears House240 after the map reveal; SpySat577A stays active"
        );
        let case_name = if gap_first {
            "spysat_gap_before_source"
        } else {
            "spysat_source_before_gaps"
        };
        let expected = native["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["name"] == case_name)
            .unwrap()["observations"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["shrouded"]
            .as_u64()
            .unwrap()
            == 0;
        sim.fog.flush_pending_gap_conceal(120);
        assert_eq!(
            sim.fog.is_cell_revealed(owner, 12, 12),
            expected,
            "original {case_name}"
        );
        for _ in 0..2 {
            sim.reconcile_active_vision_structures(&rules);
            sim.fog.build_merged_for(owner, &sim.interner);
            assert_eq!(
                sim.fog.is_cell_revealed(owner, 12, 12),
                expected,
                "repeat House is not another577D90 map event"
            );
        }
        // Native Scenario deserialization resets only this RNG to Seed(0).
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let hash = sim.state_hash();
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "spysat-map-latch", 0);
        let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        assert_eq!(restored.state_hash(), hash);
        restored.restore_after_snapshot_load().unwrap();
        restored.reconcile_active_vision_structures(&rules);
        assert_eq!(
            restored.fog.is_cell_revealed(owner, 12, 12),
            expected,
            "restore retains Map240 and cannot replay activation"
        );
        restored
            .substrate
            .entities
            .get_mut(4)
            .unwrap()
            .lifecycle
            .in_limbo = true;
        restored.reconcile_active_vision_structures(&rules);
        assert!(!restored.houses[&owner].spy_sat_active);
        assert!(!restored.fog.whole_map_revealed_owners.contains(&owner));
        assert!(
            !restored.fog.is_cell_revealed(owner, 23, 23),
            "actual loss executes reset; passive House did not"
        );
    }
}

#[test]
fn shroud_current_sight_spy_sat_replays_allied_buildings_but_not_mobile_admissions() {
    for building in [false, true] {
        let (mut sim, rules, viewer) = fixture();
        let ally = sim.interner.intern("Alliance");
        let hostile = sim.interner.intern("Soviet");
        sim.house_alliances
            .entry("ALLIANCE".into())
            .or_default()
            .insert("AMERICANS".into());
        sim.house_alliances
            .entry("AMERICANS".into())
            .or_default()
            .insert("ALLIANCE".into());
        sim.fog.mark_visible_for_owner(viewer, 0, 0);
        if building {
            insert_structure(&mut sim, 1, ally, "GSI418SIGHT", 12);
        } else {
            insert_sight_unit(&mut sim, 1, ally, 12, 12);
        }
        sim.refresh_fog(None, &vision::VisionConfig::default(), Some(&rules));
        let admission = sim.fog.sight_admissions[&(1, viewer)].clone();
        insert_structure(&mut sim, 2, hostile, "GAGAP", 12);
        sim.visit_building_operational(2, &rules);
        sim.reconcile_active_vision_structures(&rules);
        assert!(sim.fog.is_cell_visible(viewer, 12, 12));
        insert_structure(&mut sim, 3, viewer, "GASPYSAT", 2);
        sim.reconcile_active_vision_structures(&rules);
        //4ADEE0/4ADCD0 replay the directional allied Building arm only.
        //Untouched mobile latch survives, but577D90 overwrote its raw counter;
        //the later Gap callback therefore conceals that viewer's cell.
        assert_eq!(sim.fog.is_cell_revealed(viewer, 12, 12), building);
        assert_eq!(sim.fog.sight_admissions[&(1, viewer)], admission);
        for _ in 0..2 {
            sim.refresh_fog(None, &vision::VisionConfig::default(), Some(&rules));
            sim.reconcile_active_vision_structures(&rules);
            assert_eq!(
                sim.fog.is_cell_revealed(viewer, 12, 12),
                building,
                "unchanged allied mobile admission must not reopen after the bulk overwrite"
            );
            assert_eq!(sim.fog.is_cell_visible(viewer, 12, 12), building);
        }
        let bytes = bincode::serialize(&sim.fog).unwrap();
        sim.fog = bincode::deserialize(&bytes).unwrap();
        sim.refresh_fog(None, &vision::VisionConfig::default(), Some(&rules));
        sim.reconcile_active_vision_structures(&rules);
        assert_eq!(sim.fog.is_cell_revealed(viewer, 12, 12), building);
        assert_eq!(sim.fog.sight_admissions[&(1, viewer)], admission);
    }
}
