//! Original-backed operational publication through the actual world owners.
use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
use crate::sim::{components::Health, game_entity::GameEntity, house_state::HouseState};

fn fixture() -> (Simulation, RuleSet, InternedId, InternedId, InternedId) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n0=SIGHT\n[AircraftTypes]\n\
         [BuildingTypes]\n0=GAGAP\n1=POWER\n2=GASPYSAT\n\
         [Warheads]\n0=WH\n[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [SIGHT]\nStrength=100\nSight=3\nSpeed=0\n\
         [GAGAP]\nGapGenerator=yes\nGapRadiusInCells=10\nPowered=yes\nPower=-100\nStrength=100\nFoundation=1x1\n\
         [POWER]\nPower=200\nStrength=100\nFoundation=1x1\n\
         [GASPYSAT]\nSpySat=yes\nStrength=100\nFoundation=1x1\n",
    )).unwrap();
    let mut sim = Simulation::with_seed(0x143);
    sim.fog.width = 32;
    sim.fog.height = 32;
    let a = sim.interner.intern("Americans");
    let b = sim.interner.intern("Soviet");
    let c = sim.interner.intern("Third");
    for owner in [a, b, c] {
        sim.houses.insert(
            owner,
            HouseState::new(owner, owner.index() as u8, None, true, 10_000, 10),
        );
        sim.session.house_order.push(owner);
    }
    (sim, rules, a, b, c)
}

fn insert(sim: &mut Simulation, id: u64, owner: InternedId, name: &str, x: u16, y: u16) {
    let kind = if name == "SIGHT" {
        EntityCategory::Unit
    } else {
        EntityCategory::Structure
    };
    let ty = sim.interner.intern(name);
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        id,
        x,
        y,
        0,
        0,
        owner,
        Health { current: 100 },
        ty,
        kind,
        0,
        3,
        false,
    );
    entity.lifecycle.in_limbo = false;
    entity.lifecycle.cell_marked = false;
    entity.in_playfield = true;
    if name == "SIGHT" {
        let mut loco = LocomotorState::for_test_kind(LocomotorKind::Jumpjet);
        loco.layer = MovementLayer::Air;
        loco.altitude = crate::util::fixed_math::SimFixed::from_num(208);
        loco.target_altitude = loco.altitude;
        // The kernel target height is left unset on purpose: the corpus outcome
        // below holds only while the kernel is not holding the unit at 208.
        loco.jumpjet_runtime_mut().expect("jumpjet runtime").phase =
            crate::sim::movement::jumpjet_flight::STATE_HOLD;
        entity.locomotor = Some(loco);
    }
    sim.substrate.next_stable_object_id = sim.substrate.next_stable_object_id.max(id + 1);
    sim.substrate.entities.insert(entity);
    sim.add_entity_occupancy(id);
}

fn power(sim: &mut Simulation, rules: &RuleSet) {
    crate::sim::power_system::tick_power_states(
        &mut sim.power_states,
        &mut sim.substrate.entities,
        rules,
        &sim.interner,
    );
}

fn damage(sim: &mut Simulation, rules: &RuleSet, id: u64, amount: i32) {
    let wh = sim.interner.intern("WH");
    sim.commit_direct_damage_receiver(
        rules,
        None,
        crate::sim::combat::EntityDamageEvent::direct_receiver(
            id,
            amount,
            0,
            0,
            None,
            wh,
            crate::sim::combat::ReceiverCallFlags {
                ignore_defenses: true,
                arg6: false,
            },
        ),
    );
}

fn refresh(sim: &mut Simulation, rules: &RuleSet) {
    sim.refresh_fog(None, &vision::VisionConfig::default(), Some(rules));
}

#[test]
fn gap_operational_original_gate_and_native_order_corpus() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/gap_admission.json"
    ))
    .unwrap();
    let (mut sim, rules, _, owner, _) = fixture();
    insert(&mut sim, 1, owner, "GAGAP", 12, 12);
    for case in corpus["operational"].as_array().unwrap() {
        let v = &case["inputs"];
        // Remaining raw inputs have no represented ordinary stock producer.
        if v["has_power"] != 1
            || v["special_state"] != 0
            || v["disabled_count"] != 0
            || v["health"].as_i64().unwrap() < 0
            || v["needs_engineer"] != 0
            || v["powered"] != 1
        {
            continue;
        }
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.health.current = i32::try_from(v["health"].as_i64().unwrap()).unwrap();
        entity
            .mission
            .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
                current: crate::sim::mission::MissionId::from_raw(
                    v["current_mission"].as_i64().unwrap() as i32,
                ),
                queued: crate::sim::mission::MissionId::from_raw(
                    v["queued_mission"].as_i64().unwrap() as i32,
                ),
                suspended: crate::sim::mission::MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
            });
        let power = sim.power_states.entry(owner).or_default();
        power.total_output = v["output"].as_i64().unwrap() as i32;
        power.total_drain = v["drain"].as_i64().unwrap() as i32;
        power.is_low_power = !power.has_full_power();
        assert_eq!(
            sim.gap_operational_state(1, &rules).unwrap().0,
            case["admitted"] == 1,
            "{}",
            case["name"]
        );
    }
}

/// Shared simulation fixture for the rendering boundary's consumer check.
/// All event-order and snapshot assertions remain owned by the simulation.
pub(crate) fn gap_operational_power_loss_views() -> Vec<(
    crate::sim::vision::FogState,
    crate::sim::intern::InternedId,
    bool,
)> {
    let (mut sim, rules, viewer, first, last) = fixture();
    insert(&mut sim, 40, first, "POWER", 2, 2);
    insert(&mut sim, 50, last, "POWER", 2, 25);
    insert(&mut sim, 10, viewer, "SIGHT", 13, 13);
    refresh(&mut sim, &rules); // original reveal
    insert(&mut sim, 20, first, "GAGAP", 10, 12);
    power(&mut sim, &rules);
    sim.visit_building_operational(20, &rules); // gap
    sim.techno_limbo_with_rules(10, &rules); // leave, with its stored sight
    insert(&mut sim, 30, last, "GAGAP", 14, 12);
    power(&mut sim, &rules);
    sim.visit_building_operational(30, &rules); // second gap
    assert!(matches!(sim.reveal(10), RevealOutcome::Revealed { .. }));
    refresh(&mut sim, &rules); // real return, not a raw counter assignment
    damage(&mut sim, &rules, 40, 75);
    assert_eq!(sim.substrate.entities.get(40).unwrap().health.current, 25);
    power(&mut sim, &rules);
    sim.visit_building_operational(20, &rules); // remove first gap
    sim.fog.flush_pending_gap_conceal(120);
    assert!(!sim.fog.is_cell_revealed(viewer, 12, 12));

    // Actual nonlethal ReceiveDamage followed by aggregate power assessment.
    // House reconciliation must retain the deposit until its Building turn.
    sim.session.binary_frame = 238;
    damage(&mut sim, &rules, 50, 75);
    power(&mut sim, &rules);
    assert!(sim.power_states[&last].is_low_power);
    sim.reconcile_active_vision_structures(&rules);
    assert!(
        sim.substrate
            .entities
            .get(30)
            .unwrap()
            .gap_generator
            .viewers[&viewer]
            .active
    );
    assert!(!sim.fog.is_cell_revealed(viewer, 12, 12));
    sim.scenario_rng = crate::sim::rng::SimRng::new(0); // native Scenario load reset
    let before = sim.state_hash();
    let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "gap-power-order", 0);
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/gap_admission.json"
    ))
    .unwrap();
    let mut views = Vec::new();
    for (case, order) in corpus["order"]
        .as_array()
        .unwrap()
        .iter()
        .zip([vec![10, 30], vec![30, 10]])
    {
        let mut live = crate::sim::snapshot::GameSnapshot::load(&saved)
            .unwrap()
            .sim;
        assert_eq!(live.state_hash(), before);
        live.restore_after_snapshot_load().unwrap();
        assert_eq!(live.state_hash(), before);
        live.reconcile_active_vision_structures(&rules);
        live.session.binary_frame = 239;
        live.substrate
            .entities
            .get_mut(10)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap()
            .jumpjet_runtime_mut()
            .unwrap()
            .phase = crate::sim::movement::jumpjet_flight::STATE_TRANSLATE;
        live.set_logic_order_for_test(order);
        live.advance_live_object_pass(Some(&rules), None, None)
            .expect("fixture frame must complete");
        assert_eq!(
            live.substrate
                .entities
                .get(10)
                .unwrap()
                .sight_refresh_timers
                .timer(viewer),
            crate::sim::timer::CdTimer::started(239, 15)
        );
        live.reconcile_active_vision_structures(&rules);
        live.session.binary_frame = 240;
        live.advance_tick(&[], Some(&rules), &BTreeMap::new(), None, None, 67);
        let shrouded = case["observations"].as_array().unwrap().last().unwrap()["shrouded"] == 1;
        assert_eq!(
            !live.fog.is_cell_revealed(viewer, 12, 12),
            shrouded,
            "{}",
            case["name"]
        );
        live.fog.build_merged_for(viewer, &live.interner);
        views.push((live.fog, viewer, shrouded));
    }
    views
}

#[test]
fn gap_operational_power_loss_preserves_object_order_and_snapshot_continuation() {
    let _ = gap_operational_power_loss_views();
}

#[test]
fn gap_operational_first_visit_creates_viewers_and_house_is_passive() {
    let (mut sim, rules, viewer, owner, _) = fixture();
    insert(&mut sim, 1, owner, "GAGAP", 12, 12);
    assert!(sim.fog.by_owner.is_empty());
    sim.set_logic_order_for_test(vec![1]);
    sim.advance_live_object_pass(Some(&rules), None, None)
        .expect("fixture frame must complete");
    assert!(sim.fog.is_cell_gap_covered(viewer, 12, 12));
    let saved = sim.fog.gap_sources.clone();
    power(&mut sim, &rules); // no plant: now low power
    refresh(&mut sim, &rules);
    sim.reconcile_active_vision_structures(&rules);
    assert_eq!(sim.fog.gap_sources, saved);
    sim.advance_live_object_pass(Some(&rules), None, None)
        .expect("fixture frame must complete");
    assert!(sim.fog.gap_sources[&viewer].is_empty());
}

#[test]
fn gap_operational_spy_sat_rechecks_live_candidates_per_viewer() {
    let (mut sim, rules, a, owner, b) = fixture();
    insert(&mut sim, 1, owner, "GAGAP", 12, 12);
    sim.visit_building_operational(1, &rules);
    power(&mut sim, &rules); // newly low, before another Building turn
    insert(&mut sim, 2, a, "GASPYSAT", 1, 1);
    sim.reconcile_active_vision_structures(&rules);
    let gap = &sim.substrate.entities.get(1).unwrap().gap_generator;
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .building_last_operational
    );
    assert!(!gap.viewers[&a].active);
    assert!(gap.viewers[&b].active);
    assert!(
        gap.viewers[&owner].active,
        "friendly269 is independent of hostile receipts"
    );
    assert!(
        sim.fog
            .gap_sources
            .get(&a)
            .is_none_or(|sources| sources.is_empty())
    );
    assert_eq!(sim.fog.gap_sources[&b].len(), 1);

    // This viewer's inactive generator must still be called by a live bracket
    //after power restores, even while6C8 remains true and no receipt exists.
    insert(&mut sim, 3, owner, "POWER", 25, 25);
    power(&mut sim, &rules);
    sim.uninit(2); // actual loss invokes the reset bracket at House
    sim.reconcile_active_vision_structures(&rules);
    assert!(sim.substrate.entities.get(1).unwrap().gap_generator.viewers[&a].active);
    assert_eq!(sim.fog.gap_sources[&a].len(), 1);
}

#[test]
fn gap_operational_owner_change_reveals_sight_before_new_gap_and_death_removes() {
    let (mut sim, rules, a, owner, _) = fixture();
    insert(&mut sim, 1, owner, "GAGAP", 12, 12);
    refresh(&mut sim, &rules);
    sim.visit_building_operational(1, &rules);
    sim.change_owner_with_rules(1, a, &rules);
    assert!(sim.fog.is_cell_visible(a, 12, 12));
    assert!(sim.fog.is_cell_revealed(a, 12, 12));
    assert!(sim.fog.gap_sources[&a].is_empty());
    assert_eq!(sim.fog.gap_sources[&owner].len(), 1);
    let radius = sim.substrate.entities.get(1).unwrap().gap_generator.viewers[&owner].radius;
    damage(&mut sim, &rules, 1, 1000); // ordinary Explodes=no GAGAP synchronously UnInits
    assert!(!sim.substrate.entities.get(1).unwrap().gap_generator.viewers[&owner].active);
    assert_eq!(
        sim.substrate.entities.get(1).unwrap().gap_generator.viewers[&owner].radius,
        radius
    );
    assert!(sim.fog.gap_sources[&owner].is_empty());
    assert!(!sim.fog.sight_admissions.keys().any(|key| key.0 == 1));
}

#[test]
fn gap_operational_events_invalidate_map_reveal_latch_including_friendly_reentry() {
    let (mut sim, rules, viewer, owner, _) = fixture();
    sim.session.game_options.shroud = false;
    crate::sim::scenario_bootstrap::apply_launch_shroud_option(&mut sim, Some("Americans"));
    assert!(sim.fog.whole_map_revealed_owners.contains(&viewer));
    insert(&mut sim, 1, owner, "GAGAP", 12, 12);
    sim.visit_building_operational(1, &rules);
    assert!(!sim.fog.whole_map_revealed_owners.contains(&viewer));
    assert!(!sim.fog.is_cell_revealed(viewer, 12, 12));
    power(&mut sim, &rules); // low power, but no Building turn has removed it
    insert(&mut sim, 2, viewer, "GASPYSAT", 1, 1);
    sim.reconcile_active_vision_structures(&rules);
    assert!(sim.fog.is_cell_revealed(viewer, 12, 12));
    assert!(!sim.substrate.entities.get(1).unwrap().gap_generator.viewers[&viewer].active);
    assert!(
        sim.fog.whole_map_revealed_owners.contains(&viewer),
        "rejected post-bulk add must not clear the new mapping latch"
    );

    let (mut sim, rules, viewer, _, _) = fixture();
    sim.session.game_options.shroud = false;
    crate::sim::scenario_bootstrap::apply_launch_shroud_option(&mut sim, Some("Americans"));
    insert(&mut sim, 1, viewer, "GAGAP", 12, 12);
    sim.visit_building_operational(1, &rules);
    assert!(
        !sim.fog.whole_map_revealed_owners.contains(&viewer),
        "friendly admitted add clears240 without a hostile receipt"
    );
    insert(&mut sim, 2, viewer, "GASPYSAT", 1, 1);
    sim.reconcile_active_vision_structures(&rules);
    assert!(sim.substrate.entities.get(1).unwrap().gap_generator.viewers[&viewer].active);
    assert!(
        !sim.fog.whole_map_revealed_owners.contains(&viewer),
        "successful friendly re-admission follows the bulk240=true store"
    );
}
