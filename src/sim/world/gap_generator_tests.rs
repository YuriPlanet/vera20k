//! Original-backed operational publication through the actual world owners.
use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
use crate::sim::{combat::Health, game_entity::GameEntity, house_state::HouseState};

fn fixture() -> (Simulation, RuleSet, InternedId, InternedId, InternedId) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n0=SIGHT\n[AircraftTypes]\n\
         [BuildingTypes]\n0=GAGAP\n1=POWER\n2=GASPYSAT\n\
         [Warheads]\n0=WH\n[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [Clear]\nBuildable=yes\n\
         [SIGHT]\nStrength=100\nSight=3\nSpeed=0\n\
         [GAGAP]\nGapGenerator=yes\nGapRadiusInCells=10\nPowered=yes\nPower=-100\nStrength=100\nFoundation=1x1\n\
         [POWER]\nPower=200\nStrength=100\nFoundation=1x1\n\
         [GASPYSAT]\nSpySat=yes\nStrength=100\nFoundation=1x1\n",
    )).unwrap();
    let mut sim = Simulation::with_seed(0x143);
    sim.fog.set_width_for_test(32);
    sim.fog.set_height_for_test(32);
    let a = sim.interner.intern("Americans");
    let b = sim.interner.intern("Soviet");
    let c = sim.interner.intern("Third");
    for owner in [a, b, c] {
        sim.houses.register(HouseState::new(
            owner,
            owner.index() as u8,
            None,
            true,
            10_000,
            10,
        ));
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
        Health::new(100),
        ty,
        kind,
        0,
        3,
        false,
    );
    entity.lifecycle.set_in_limbo_for_test(false);
    entity.lifecycle.set_cell_marked_for_test(false);
    entity.set_in_playfield(true);
    if name == "SIGHT" {
        let mut loco = LocomotorState::for_test_kind(LocomotorKind::Jumpjet);
        loco.set_layer_for_test(MovementLayer::Air);
        entity.position.set_exact_z_leptons_for_test(Some(208));

        // The kernel target height is left unset on purpose: the corpus outcome
        // below holds only while the kernel is not holding the unit at 208.
        let runtime = loco.jumpjet_runtime_mut().expect("jumpjet runtime");
        *runtime = runtime.clone().with_phase_for_test(
            crate::sim::movement::jumpjet_movement::jumpjet_flight::STATE_HOLD,
        );
        entity.locomotor = Some(loco);
    }
    sim.set_next_stable_object_id_for_test(sim.next_stable_object_id().max(id + 1));
    sim.substrate.entities.insert(entity);
    sim.add_entity_occupancy(id);
    // Unlimbo's House+0x68 append, which the SpySat scan walks.
    sim.append_house_base_building_for_test(id);
}

fn power(sim: &mut Simulation, rules: &RuleSet) {
    crate::sim::power_system::tick_power_states(
        &mut sim.power_states,
        &mut sim.substrate.entities,
        rules,
        &sim.interner,
        sim.session.binary_frame(),
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
        crate::sim::world::FrameEffects::default(),
    );
}

fn refresh(sim: &mut Simulation, rules: &RuleSet) {
    sim.refresh_fog(&vision::VisionConfig::default(), Some(rules));
}

/// The sight reveal of the Unlimbo `insert` stands for (`0x006F6D27`),
/// through the sight owner.
fn reveal_inserted_sight(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    sim.update_object_sight_in_view(
        id,
        0,
        None,
        &vision::VisionConfig::default(),
        None,
        UninitContext::with_rules(rules),
    );
}

#[test]
fn explicit_power_off_removes_gap_before_the_operational_visit_and_on_defers_add() {
    let (mut sim, rules, a, b, _) = fixture();
    insert(&mut sim, 1, a, "GAGAP", 12, 12);
    insert(&mut sim, 2, a, "POWER", 16, 12);
    power(&mut sim, &rules);
    sim.visit_building_operational(1, &rules, None, FrameEffects::default());
    assert!(sim.fog.is_cell_gap_covered(b, 12, 12));
    let rng = sim.rng_state();
    assert!(sim.go_building_offline(1, &rules, FrameEffects::default()));
    assert!(
        !sim.fog.is_cell_gap_covered(b, 12, 12),
        "DisableStuff4524E9 removes immediately"
    );
    sim.visit_building_operational(1, &rules, None, FrameEffects::default());
    assert!(sim.go_building_online(1, &rules, FrameEffects::default()));
    assert!(
        !sim.fog.is_cell_gap_covered(b, 12, 12),
        "GoOnline has no gap-add callback"
    );
    sim.visit_building_operational(1, &rules, None, FrameEffects::default());
    assert!(sim.fog.is_cell_gap_covered(b, 12, 12));
    assert_eq!(sim.rng_state(), rng);
}

#[test]
fn gap_operational_original_gate_and_native_order_corpus() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/gap_admission.json",
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
        entity
            .health
            .set_for_test(i32::try_from(v["health"].as_i64().unwrap()).unwrap());
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
        let power = sim
            .power_states
            .map_mut_for_test()
            .entry(owner)
            .or_default();
        power.set_total_output_for_test(v["output"].as_i64().unwrap() as i32);
        power.set_total_drain_for_test(v["drain"].as_i64().unwrap() as i32);
        power.set_is_low_power_for_test(!power.has_full_power());
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
pub fn gap_operational_power_loss_views() -> Vec<(
    crate::sim::vision::FogState,
    crate::sim::intern::InternedId,
    bool,
)> {
    let (mut sim, rules, viewer, first, last) = fixture();
    insert(&mut sim, 40, first, "POWER", 2, 2);
    insert(&mut sim, 50, last, "POWER", 2, 25);
    insert(&mut sim, 10, viewer, "SIGHT", 13, 13);
    for id in [40, 50, 10] {
        reveal_inserted_sight(&mut sim, &rules, id); // original reveal
    }
    refresh(&mut sim, &rules);
    insert(&mut sim, 20, first, "GAGAP", 10, 12);
    power(&mut sim, &rules);
    sim.visit_building_operational(20, &rules, None, crate::sim::world::FrameEffects::default()); // gap
    sim.techno_limbo_with_rules(10, &rules, None); // leave, with its stored sight
    insert(&mut sim, 30, last, "GAGAP", 14, 12);
    power(&mut sim, &rules);
    sim.visit_building_operational(30, &rules, None, crate::sim::world::FrameEffects::default()); // second gap
    assert!(matches!(sim.reveal(10), RevealOutcome::Revealed { .. }));
    // A Reveal without a map commits no exact Z (`unlimbo_z`); SetHeight puts
    // the jumpjet back at 208, the high-flight threshold.
    sim.set_object_height(
        10,
        208,
        None,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    refresh(&mut sim, &rules); // real return, not a raw counter assignment
    damage(&mut sim, &rules, 40, 75);
    assert_eq!(sim.substrate.entities.get(40).unwrap().health.current(), 25);
    power(&mut sim, &rules);
    sim.visit_building_operational(20, &rules, None, crate::sim::world::FrameEffects::default()); // remove first gap
    sim.fog.flush_pending_gap_conceal(120);
    assert!(!sim.fog.is_cell_revealed(viewer, 12, 12));

    // Actual nonlethal ReceiveDamage followed by aggregate power assessment.
    // House reconciliation must retain the deposit until its Building turn.
    sim.session.set_binary_frame_for_test(238);
    damage(&mut sim, &rules, 50, 75);
    power(&mut sim, &rules);
    assert!(sim.power_states[&last].is_low_power());
    sim.reconcile_active_vision_structures(&rules);
    assert!(
        sim.substrate
            .entities
            .get(30)
            .unwrap()
            .gap_generator
            .viewers()[&viewer]
            .active()
    );
    assert!(!sim.fog.is_cell_revealed(viewer, 12, 12));
    sim.scenario_rng = crate::sim::rng::SimRng::new(0); // native Scenario load reset
    let before = sim.state_hash();
    let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "gap-power-order", 0);
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/gap_admission.json",
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
        live.session.set_binary_frame_for_test(239);
        let runtime = live
            .substrate
            .entities
            .get_mut(10)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap()
            .jumpjet_runtime_mut()
            .unwrap();
        *runtime = runtime.clone().with_phase_for_test(
            crate::sim::movement::jumpjet_movement::jumpjet_flight::STATE_TRANSLATE,
        );
        live.set_logic_order_for_test(order);
        live.advance_live_object_pass(
            Some(&rules),
            None,
            crate::sim::world::FrameEffects::default(),
        )
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
        live.session.set_binary_frame_for_test(240);
        live.advance_tick(&[], Some(&rules), None, None, 67);
        let shrouded = case["observations"].as_array().unwrap().last().unwrap()["shrouded"] == 1;
        assert_eq!(
            !live.fog.is_cell_revealed(viewer, 12, 12),
            shrouded,
            "{}",
            case["name"]
        );
        views.push((live.fog, viewer, shrouded));
    }
    views
}

#[test]
fn gap_operational_spy_sat_rechecks_live_candidates_per_viewer() {
    let (mut sim, rules, a, owner, b) = fixture();
    insert(&mut sim, 1, owner, "GAGAP", 12, 12);
    sim.visit_building_operational(1, &rules, None, crate::sim::world::FrameEffects::default());
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
            .is_operational()
    );
    assert!(!gap.viewers()[&a].active());
    assert!(gap.viewers()[&b].active());
    assert!(
        gap.viewers()[&owner].active(),
        "friendly269 is independent of hostile receipts"
    );
    assert!(
        sim.fog
            .gap_sources()
            .get(&a)
            .is_none_or(|sources| sources.is_empty())
    );
    assert_eq!(sim.fog.gap_sources()[&b].len(), 1);

    // This viewer's inactive generator must still be called by a live bracket
    //after power restores, even while6C8 remains true and no receipt exists.
    insert(&mut sim, 3, owner, "POWER", 25, 25);
    power(&mut sim, &rules);
    sim.uninit(2); // actual loss invokes the reset bracket at House
    sim.reconcile_active_vision_structures(&rules);
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .gap_generator
            .viewers()[&a]
            .active()
    );
    assert_eq!(sim.fog.gap_sources()[&a].len(), 1);
}

/// A lost Spy Sat's gap re-entry asks from the generator's house whether the
/// viewer is its ally, as the add does (`0x006FB2CF`): the viewer's own
/// one-way alliance with it still takes the gap.
#[test]
fn spy_sat_gap_reentry_asks_the_generators_house_about_the_viewer() {
    for (asker, other, receipts) in [("SOVIET", "AMERICANS", 0), ("AMERICANS", "SOVIET", 1)] {
        let (mut sim, rules, a, owner, _) = fixture();
        *sim.house_alliances_mut_for_test() =
            HouseAllianceMap::from([(asker.into(), [other.into()].into())]);
        insert(&mut sim, 1, owner, "GAGAP", 12, 12);
        sim.visit_building_operational(1, &rules, None, crate::sim::world::FrameEffects::default());
        power(&mut sim, &rules);
        insert(&mut sim, 2, a, "GASPYSAT", 1, 1);
        sim.reconcile_active_vision_structures(&rules);
        insert(&mut sim, 3, owner, "POWER", 25, 25);
        power(&mut sim, &rules);
        sim.uninit(2);
        sim.reconcile_active_vision_structures(&rules);
        assert_eq!(
            sim.fog
                .gap_sources()
                .get(&a)
                .map_or(0, |sources| sources.len()),
            receipts,
            "{asker} allies {other}"
        );
    }
}

#[test]
fn gap_operational_owner_change_reveals_sight_before_new_gap_and_death_removes() {
    let (mut sim, rules, a, owner, _) = fixture();
    insert(&mut sim, 1, owner, "GAGAP", 12, 12);
    refresh(&mut sim, &rules);
    sim.visit_building_operational(1, &rules, None, crate::sim::world::FrameEffects::default());
    sim.change_owner_with_rules(
        1,
        a,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    assert!(sim.fog.is_cell_visible(a, 12, 12));
    assert!(sim.fog.is_cell_revealed(a, 12, 12));
    assert!(sim.fog.gap_sources()[&a].is_empty());
    assert_eq!(sim.fog.gap_sources()[&owner].len(), 1);
    let radius = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .gap_generator
        .viewers()[&owner]
        .radius();
    damage(&mut sim, &rules, 1, 1000); // ordinary Explodes=no GAGAP synchronously UnInits
    assert!(
        !sim.substrate
            .entities
            .get(1)
            .unwrap()
            .gap_generator
            .viewers()[&owner]
            .active()
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .gap_generator
            .viewers()[&owner]
            .radius(),
        radius
    );
    assert!(sim.fog.gap_sources()[&owner].is_empty());
    assert!(
        !sim.fog
            .sight_admissions_for_test()
            .iter()
            .any(|(key, source)| key.0 == 1 && source.latched)
    );
}

#[test]
fn gap_operational_events_invalidate_map_reveal_latch_including_friendly_reentry() {
    let (mut sim, rules, viewer, owner, _) = fixture();
    sim.session.game_options_mut_for_test().shroud = false;
    sim.session.set_current_house_for_test(Some(viewer));
    crate::sim::scenario_bootstrap::apply_launch_shroud_option(
        &mut sim,
        crate::sim::world::UninitContext::with_rules(&rules),
    );
    assert!(sim.fog.whole_map_revealed_for_owner(viewer));
    insert(&mut sim, 1, owner, "GAGAP", 12, 12);
    sim.visit_building_operational(1, &rules, None, crate::sim::world::FrameEffects::default());
    assert!(!sim.fog.whole_map_revealed_for_owner(viewer));
    assert!(!sim.fog.is_cell_revealed(viewer, 12, 12));
    power(&mut sim, &rules); // low power, but no Building turn has removed it
    insert(&mut sim, 2, viewer, "GASPYSAT", 1, 1);
    sim.reconcile_active_vision_structures(&rules);
    assert!(sim.fog.is_cell_revealed(viewer, 12, 12));
    assert!(
        !sim.substrate
            .entities
            .get(1)
            .unwrap()
            .gap_generator
            .viewers()[&viewer]
            .active()
    );
    assert!(
        sim.fog.whole_map_revealed_for_owner(viewer),
        "rejected post-bulk add must not clear the new mapping latch"
    );

    let (mut sim, rules, viewer, _, _) = fixture();
    sim.session.game_options_mut_for_test().shroud = false;
    sim.session.set_current_house_for_test(Some(viewer));
    crate::sim::scenario_bootstrap::apply_launch_shroud_option(
        &mut sim,
        crate::sim::world::UninitContext::with_rules(&rules),
    );
    insert(&mut sim, 1, viewer, "GAGAP", 12, 12);
    sim.visit_building_operational(1, &rules, None, crate::sim::world::FrameEffects::default());
    assert!(
        !sim.fog.whole_map_revealed_for_owner(viewer),
        "friendly admitted add clears240 without a hostile receipt"
    );
    insert(&mut sim, 2, viewer, "GASPYSAT", 1, 1);
    sim.reconcile_active_vision_structures(&rules);
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .gap_generator
            .viewers()[&viewer]
            .active()
    );
    assert!(
        !sim.fog.whole_map_revealed_for_owner(viewer),
        "successful friendly re-admission follows the bulk240=true store"
    );
}

/// Consumer fixture: one persistent world, ordinary source Unlimbo, hostile
/// operational gap, actual Techno Limbo, and the shared119/120 sweep gate.
/// The returned snapshots let client tests inspect pixels without owning sight.
pub fn sustained_source_gap_limbo_views() -> Vec<(vision::FogState, InternedId, bool)> {
    let (mut sim, rules, viewer, gapper, _) = fixture();
    crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    sim.fog = vision::FogState::new(32, 32);
    sim.session.set_current_house_for_test(Some(viewer));
    let source = sim
        .spawn_object("SIGHT", "Americans", 16, 16, 0, &rules)
        .expect("source Unlimbo");
    let mut views = vec![(sim.fog.clone(), viewer, true)];
    sim.spawn_object("POWER", "Soviet", 2, 2, 0, &rules)
        .expect("power Unlimbo");
    let gap = sim
        .spawn_object("GAGAP", "Soviet", 18, 16, 0, &rules)
        .expect("gap Unlimbo");
    power(&mut sim, &rules);
    sim.visit_building_operational(gap, &rules, None, FrameEffects::default());
    assert!(
        sim.substrate
            .entities
            .get(gap)
            .unwrap()
            .gap_generator
            .viewers()[&viewer]
            .active()
    );
    assert_ne!(viewer, gapper);
    views.push((sim.fog.clone(), viewer, true));
    sim.techno_limbo_with_rules(source, &rules, None);
    refresh(&mut sim, &rules);
    assert!(!sim.fog.sight_is_latched(source, viewer));
    views.push((sim.fog.clone(), viewer, true));
    for (frame, open) in [(119, true), (120, false)] {
        sim.session.set_binary_frame_for_test(frame);
        sim.tick_pending_shroud_conceal(UninitContext::with_rules(&rules));
        assert_eq!(sim.fog.is_cell_revealed(viewer, 16, 16), open);
        views.push((sim.fog.clone(), viewer, open));
    }
    views
}

/// Nontransitive alliance consumer fixture. Only the diplomacy owner installs
/// A↔B↔C; ordinary A sight admits B but never becomes B's exported source.
pub fn nontransitive_source_gap_views() -> Vec<(vision::FogState, InternedId, InternedId)> {
    let (mut sim, rules, a, b, c) = fixture();
    crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    sim.fog = vision::FogState::new(32, 32);
    let hostile = sim.intern("Neutral");
    sim.houses
        .register(HouseState::new(hostile, 3, None, false, 10_000, 10));
    let graph = HouseAllianceMap::from([
        ("AMERICANS".into(), ["SOVIET".into()].into()),
        ("SOVIET".into(), ["AMERICANS".into(), "THIRD".into()].into()),
        ("THIRD".into(), ["SOVIET".into()].into()),
    ]);
    sim.install_house_alliances(
        crate::sim::house_diplomacy::HouseAllianceAdmission::Admitted(graph),
        &rules,
    );
    sim.spawn_object("SIGHT", "Soviet", 25, 25, 0, &rules)
        .expect("B source");
    sim.spawn_object("SIGHT", "Third", 25, 10, 0, &rules)
        .expect("C source");
    let source = sim
        .spawn_object("SIGHT", "Americans", 16, 16, 0, &rules)
        .expect("A source");
    assert!(sim.fog.sight_is_latched(source, a));
    assert!(sim.fog.sight_is_latched(source, b));
    assert!(!sim.fog.sight_is_latched(source, c));
    sim.spawn_object("POWER", "Neutral", 2, 25, 0, &rules)
        .expect("hostile power");
    let gap = sim
        .spawn_object("GAGAP", "Neutral", 18, 16, 0, &rules)
        .expect("hostile gap");
    power(&mut sim, &rules);
    let mut views = Vec::new();
    for _ in 0..3 {
        sim.visit_building_operational(gap, &rules, None, FrameEffects::default());
        refresh(&mut sim, &rules);
        assert!(sim.fog.is_cell_visible(b, 16, 16));
        assert!(!sim.fog.is_cell_visible(c, 16, 16));
        assert!(!sim.fog.is_cell_revealed(c, 16, 16));
        views.push((sim.fog.clone(), b, c));
    }
    sim.techno_limbo_with_rules(gap, &rules, None);
    assert!(!sim.fog.is_cell_revealed(c, 16, 16));
    views.push((sim.fog.clone(), b, c));
    views
}

/// CreateGap centres its circle on the Location shifted for its height
/// (`0x006FB1CB..0x006FB254`), as does DestroyGap: four levels up, the disc
/// sits two Cells north of the generator's own Cell. The view rebuild keeps
/// that disc for the hostile viewer and the owner's friendly gap fog alike,
/// and taking the generator offline removes it.
#[test]
fn a_raised_gap_generator_centres_its_circle_by_its_height() {
    let (mut sim, rules, a, b, _) = fixture();
    insert(&mut sim, 1, a, "GAGAP", 16, 16);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .position
        .set_exact_z_leptons_for_test(Some(4 * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS));
    insert(&mut sim, 2, a, "POWER", 20, 16);
    power(&mut sim, &rules);
    sim.visit_building_operational(1, &rules, None, FrameEffects::default());
    let disc = |sim: &Simulation| {
        (
            sim.fog.is_cell_gap_covered(b, 4, 14),
            sim.fog.is_cell_gap_covered(b, 24, 14),
            sim.fog.is_cell_gap_covered(b, 26, 16),
            sim.fog.is_cell_gap_covered(b, 16, 16),
        )
    };
    assert_eq!(
        disc(&sim),
        (true, true, false, true),
        "radius 10 around (14, 14), not (16, 16)"
    );
    refresh(&mut sim, &rules);
    assert_eq!(disc(&sim), (true, true, false, true));
    assert_eq!(
        (
            sim.fog.is_cell_gap_fog(a, 4, 14),
            sim.fog.is_cell_gap_fog(a, 26, 16)
        ),
        (true, false),
        "the owner's friendly gap fog takes the same disc"
    );
    assert!(sim.go_building_offline(1, &rules, FrameEffects::default()));
    assert_eq!(disc(&sim), (false, false, false, false));
}
