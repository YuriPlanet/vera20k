//! Sight moves at reveal events only: a mover keeps the footprint its last
//! `UpdateReveal @ 0x0070AF50` stored until its next cell arrival (Unit
//! `0x0073AC74`, Infantry `0x0051A8EB`), not as the frame pass sees it
//! cross into a new cell.

use super::*;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::FrameEffects;

/// Retail rules on the flat arena, with a human Americans House.
fn retail_arena() -> Option<(Simulation, RuleSet, PathGrid)> {
    let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()?;
    let mut sim = Simulation::new();
    let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    let owner = sim.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    sim.session.set_current_house_for_test(Some(owner));
    Some((sim, rules, grid))
}

/// GrowShroud4ADFF0 calls the object's See slot, not UpdateReveal488.
/// Aircraft41ADF0 forces radius1 at GetHeight==0, even for raw Sight0;
/// its Map5678E0 final0/final1 pair has no border or retained source write.
#[test]
fn aircraft_uncached_ground_sight_uses_one_cell_radius_without_replacing_its_source() {
    for (kind, sight, resize) in [
        ("ORCA", 8, false),
        ("SPYP", 0, false),
        ("ORCA", 8, true),
        ("SPYP", 0, true),
    ] {
        let Some((mut sim, rules, _)) = retail_arena() else {
            return;
        };
        sim.session
            .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
        sim.fog = vision::FogState::new(32, 32);
        assert_eq!(rules.object(kind).unwrap().sight, sight);
        let owner = sim.intern("Americans");
        let id = sim
            .spawn_object(kind, "Americans", 16, 16, 0, &rules)
            .unwrap();
        let entity = sim.entities().get(id).unwrap();
        assert_eq!(entity.category(), EntityCategory::Aircraft);
        assert_eq!(
            crate::sim::movement::air_movement::current_fly_height(
                entity,
                sim.resolved_terrain.as_ref()
            ),
            0
        );
        assert!(entity.discovery.discovered_by_current_house());
        assert!(
            sim.substrate
                .display
                .members(super::super::display_layers::DisplayLayer::GROUND)
                .contains(&id)
        );
        //4ADFF0 admits a known Ground object without consulting3D5;
        //Aircraft See itself has no Techno70ADC0 playfield gate either.
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .in_playfield
            .store(false);
        let held = sim.fog.sight_admissions_for_test().clone();
        let rng = sim.scenario_rng.native_state_hex();
        let ((), trace) = trace_reveal_calls(|| {
            if resize {
                assert!(sim.change_visible_map_area([2, 2, 16, 12], &rules));
                assert!(sim.entities().get(id).unwrap().in_playfield());
            } else {
                sim.grow_shroud(UninitContext::with_rules(&rules));
            }
        });
        // Native radius1 offset order, also consumed by reveal_area_tests.
        let cells = vec![
            (16, 16),
            (17, 15),
            (16, 15),
            (15, 15),
            (15, 16),
            (17, 16),
            (15, 17),
            (16, 17),
            (17, 17),
        ];
        assert_eq!(
            trace.walks,
            [cells.clone(), cells],
            "{kind}, resize={resize}"
        );
        assert!(
            trace.border_receivers.is_empty(),
            "Aircraft See has no567DA0"
        );
        assert!(sim.fog.is_ground_unshrouded(owner, 17, 17));
        assert_eq!(sim.fog.sight_admissions_for_test(), &held);
        assert_eq!(sim.scenario_rng.native_state_hex(), rng);
    }
}

/// Aircraft41AE00 retains raw Type Sight: no Techno elevation scaling.
/// An airborne zero-Sight plane returns with ordinary skirmish fog disabled.
#[test]
fn aircraft_uncached_airborne_sight_uses_raw_type_range_and_skips_zero() {
    for (kind, sight) in [("ORCA", 8), ("SPYP", 0)] {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()
        else {
            return;
        };
        let mut sim = crate::sim::arena_fixture::rival_arena_with_size(&rules, 1, 128);
        sim.session
            .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
        let owner = sim.intern("Americans");
        sim.session.set_current_house_for_test(Some(owner));
        sim.set_playfield_bounds_for_test(Some(crate::sim::cell_rect::PlayfieldBounds {
            base: 50,
            ..crate::sim::arena_fixture::OPEN_PLAYFIELD
        }));
        sim.set_playfield_size_height_for_test(Some(50));
        sim.fog = vision::FogState::new(128, 128);
        let id = sim
            .spawn_object(kind, "Americans", 75, 50, 0, &rules)
            .unwrap();
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .position
            .put_z(4000);
        assert_eq!(rules.object(kind).unwrap().sight, sight);
        assert!(!sim.session.game_options().fog_of_war);
        let held = sim.fog.sight_admissions_for_test().clone();
        let ((), trace) = trace_reveal_calls(|| {
            sim.reveal_uncached_object_sight(
                id,
                false,
                false,
                None,
                UninitContext::with_rules(&rules),
            );
        });
        if sight == 0 {
            assert!(trace.walks.is_empty());
        } else {
            assert_eq!(trace.walks.len(), 2);
            assert_eq!(trace.walks[0], trace.walks[1]);
            // Original See+Map execution:4000 shifts19 cells, radius8
            //visits205 cells; Techno's elevation scaling would produce9.
            assert_eq!(trace.walks[0][0], (56, 31));
            assert_eq!(trace.walks[0].len(), 205);
            assert!(trace.walks[0].contains(&(64, 31)));
            assert!(
                !trace.walks[0].contains(&(65, 31)),
                "no elevation-added ninth ring"
            );
        }
        assert!(trace.border_receivers.is_empty());
        assert_eq!(sim.fog.sight_admissions_for_test(), &held);
    }
}

///41AE63..41AE95 rereads live Location and Owner for the second map call,
/// retaining the first raw radius even if a synchronous notification changes
/// the source. The supplied callback isolates that transport boundary.
#[test]
fn aircraft_uncached_sight_rereads_its_location_between_map_passes() {
    let Some((mut sim, rules, _)) = retail_arena() else {
        return;
    };
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
    let id = sim
        .spawn_object("ORCA", "Americans", 16, 16, 0, &rules)
        .unwrap();
    // Make the first pass produce a fresh CellChangeNotify callback.
    sim.fog = vision::FogState::new(32, 32);
    let rng = sim.scenario_rng.native_state_hex();
    let ((), trace) = trace_reveal_calls(|| {
        with_first_cell_receiver(
            move |sim| {
                let entity = sim.substrate.entities.get_mut(id).unwrap();
                entity.position.set_rx_for_test(20);
                entity.position.put_z(416);
                entity.sight.set_vision_range_for_test(10);
            },
            || {
                sim.reveal_uncached_object_sight(
                    id,
                    false,
                    false,
                    None,
                    UninitContext::with_rules(&rules),
                )
            },
        )
    });
    assert_eq!(trace.walks.len(), 2);
    assert_eq!(trace.walks[0][0], (16, 16));
    assert_eq!(trace.walks[1][0], (18, 14));
    assert_eq!(trace.walks[0].len(), 9);
    assert_eq!(
        trace.walks[1].len(),
        9,
        "radius1 was retained before the callback"
    );
    assert!(trace.border_receivers.is_empty());
    assert!(sim.fog.sight_admissions_for_test().is_empty());
    assert_eq!(sim.scenario_rng.native_state_hex(), rng);
}

/// The two Map5678E0 calls independently apply their owner/player gate.
/// The supplied callback changes only the owner-store boundary, without
/// claiming that native discovery naturally performs an ownership transfer.
#[test]
fn aircraft_uncached_sight_rereads_owner_before_the_second_house_admission() {
    let Some((mut sim, rules, _)) = retail_arena() else {
        return;
    };
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
    let other = add_computer_house(&mut sim, "Russians");
    let id = sim
        .spawn_object("ORCA", "Americans", 16, 16, 0, &rules)
        .unwrap();
    sim.fog = vision::FogState::new(32, 32);
    let ((), trace) = trace_reveal_calls(|| {
        with_first_cell_receiver(
            move |sim| sim.substrate.entities.change_owner(id, other),
            || {
                sim.reveal_uncached_object_sight(
                    id,
                    false,
                    false,
                    None,
                    UninitContext::with_rules(&rules),
                )
            },
        )
    });
    assert_eq!(trace.walks.len(), 2);
    assert_eq!(
        trace.notifications.len(),
        9,
        "the nonallied second owner cannot notify the player"
    );
    assert_eq!(sim.entities().get(id).unwrap().owner(), other);
    assert!(sim.fog.sight_admissions_for_test().is_empty());
}

/// The cell source `id`'s own sight was last revealed around.
fn sight_origin(sim: &Simulation, id: u64) -> (u16, u16) {
    let owner = sim.substrate.entities.get(id).unwrap().owner();
    let admission = &sim.fog.sight_admissions_for_test()[&(id, owner)];
    (admission.origin.0, admission.origin.1)
}

/// A tank and an infantryman sent three cells east keep their sight where
/// it was while they cross into each next cell, and take it there when they
/// arrive: once per cell, in path order.
#[test]
fn a_movers_sight_moves_at_each_cell_arrival() {
    for kind in ["MTNK", "E1"] {
        let Some((mut sim, rules, grid)) = retail_arena() else {
            return;
        };
        let id = sim
            .spawn_object(kind, "Americans", 10, 10, 0, &rules)
            .unwrap();
        let owner = sim.substrate.entities.get(id).unwrap().owner();
        let order = CommandEnvelope::new(
            owner,
            sim.session.tick() + 1,
            Command::Move {
                entity_id: id,
                target_rx: 13,
                target_ry: 10,
            },
        );
        let _ = sim.advance_tick(&[order], Some(&rules), Some(&grid), None, 67);
        let mut origins = vec![sight_origin(&sim, id)];
        let mut trailed = false;
        for _ in 0..400 {
            let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
            let entity = sim.substrate.entities.get(id).unwrap();
            let origin = sight_origin(&sim, id);
            trailed |= origin != (entity.position.rx(), entity.position.ry());
            if origins.last() != Some(&origin) {
                origins.push(origin);
            }
        }
        let entity = sim.substrate.entities.get(id).unwrap();
        assert_eq!(
            (entity.position.rx(), entity.position.ry()),
            (13, 10),
            "{kind}: precondition, it arrived"
        );
        assert!(trailed, "{kind}: its sight trails it into each next cell");
        assert_eq!(
            origins,
            [(10, 10), (11, 10), (12, 10), (13, 10)],
            "{kind}: one reveal per arrival"
        );
    }
}

/// Original70B202 clears250 alone. Inactive254/260 still drive a later
/// border and therefore must survive frames, hashing and the saved game.
#[test]
fn source_release_retains_its_cached_geometry_until_teardown() {
    let Some((mut sim, rules, _)) = retail_arena() else {
        return;
    };
    sim.fog = vision::FogState::new(32, 32);
    let owner = sim.intern("Americans");
    let id = sim
        .spawn_object("AMCV", "Americans", 16, 16, 0, &rules)
        .unwrap();
    let held = sim.fog.sight_admissions_for_test()[&(id, owner)].clone();
    assert!(held.latched);
    sim.techno_limbo_with_rules(id, &rules, None);
    let cache = sim.fog.sight_admissions_for_test()[&(id, owner)].clone();
    assert!(!cache.latched);
    assert_eq!((cache.origin, cache.radius), (held.origin, held.radius));
    assert!(
        cache.cells.is_empty(),
        "inactive cache retains no active footprint index"
    );
    let config = sim.sight_reveal_config(&rules);
    sim.refresh_sight_sources(&config, &rules);
    assert_eq!(sim.fog.sight_admissions_for_test()[&(id, owner)], cache);
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "inactive-sight-cache", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    //The existing native Scenario load owner resets its RNG independently;
    //do not alter the live constructor stream to make two phases hash alike.
    let hash = restored.state_hash();
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(
        restored.fog.sight_admissions_for_test()[&(id, owner)],
        cache
    );
    assert_eq!(restored.state_hash(), hash);
    // No competing cache: changing retained260 changes the lockstep hash.
    restored
        .fog
        .sight_admissions_mut_for_test()
        .get_mut(&(id, owner))
        .unwrap()
        .radius += 1;
    assert_ne!(restored.state_hash(), hash);
    let pending_cache = restored.fog.sight_admissions_for_test()[&(id, owner)].clone();
    restored.uninit_with_rules(id, &rules);
    // ObjectUnInit5F6625 clears alive and queues the object; it does not
    // free its storage. The inactive254/260 cache lives through that window.
    assert!(restored.entities().get(id).is_some());
    assert!(restored.substrate.pending_delete.contains_object(&id));
    assert_eq!(
        restored.fog.sight_admissions_for_test()[&(id, owner)],
        pending_cache
    );
    // Main_Tick55DE9F reaches the shared725C70 drain after frame commit.
    // Use that real rules-bearing destructor owner, not a direct store/cache
    // removal or the test-only rules-less flush wrapper.
    restored.process_pending_delete_with(
        &rules,
        None,
        FrameEffects::default(),
        1,
        restored.session.started(),
    );
    assert!(restored.entities().get(id).is_none());
    assert!(!restored.substrate.pending_delete.contains_object(&id));
    assert!(
        !restored
            .fog
            .sight_admissions_for_test()
            .keys()
            .any(|key| key.0 == id)
    );
}

/// The exact raw Type Sight zero test70AF87 precedes the admission latch;
/// it differs from a nonzero raw value even when VERA's safe map walk is empty.
/// The synthetic negative radius tests cache admission, not native traversal:
/// native567A63 indexes before its count table for a negative radius.
#[test]
fn exact_zero_sight_gate_differs_from_empty_walk_and_override() {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[VehicleTypes]\n0=ZERO\n1=NEGATIVE\n[ZERO]\nStrength=100\nSight=0\nSpeed=0\n\
         [NEGATIVE]\nStrength=100\nSight=-1\nSpeed=0\n",
    ))
    .unwrap();
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    sim.fog = vision::FogState::new(32, 32);
    let owner = sim.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    let zero = sim
        .spawn_object("ZERO", "Americans", 16, 16, 0, &rules)
        .unwrap();
    assert!(!sim.fog.sight_is_latched(zero, owner));
    assert!(!sim.fog.is_cell_revealed(owner, 16, 16));
    let negative = sim
        .spawn_object("NEGATIVE", "Americans", 24, 24, 0, &rules)
        .unwrap();
    let admission = &sim.fog.sight_admissions_for_test()[&(negative, owner)];
    assert!(admission.latched);
    assert_eq!(admission.radius, -1);
    assert!(admission.cells.is_empty());
    sim.refresh_object_sight(zero, &rules, 3, crate::sim::world::FrameEffects::default());
    assert!(sim.fog.sight_is_latched(zero, owner));
    assert_eq!(
        sim.fog.sight_admissions_for_test()[&(zero, owner)].radius,
        3
    );
    assert!(sim.fog.is_cell_revealed(owner, 16, 16));
    let held = sim.fog.sight_admissions_for_test()[&(zero, owner)].clone();
    let config = sim.sight_reveal_config(&rules);
    sim.refresh_sight_sources(&config, &rules);
    assert_eq!(
        sim.fog.sight_admissions_for_test()[&(zero, owner)],
        held,
        "rawSight0 override remains active across projection frames"
    );
}

/// Regression of the native oracle's supplied first5865F0 nested-release
/// control. This synthetic flat scene tests ordering/lifetime; native output
/// comparison remains in the consumed frontier corpus, not hand-made goldens.
#[test]
fn a_nested_source_release_is_not_resurrected_by_outer_walk_or_next_frame() {
    let Some((mut sim, rules, _)) = retail_arena() else {
        return;
    };
    sim.fog = vision::FogState::new(32, 32);
    let owner = sim.intern("Americans");
    let id = sim
        .spawn_object("AMCV", "Americans", 16, 16, 0, &rules)
        .unwrap();
    sim.techno_limbo_with_rules(id, &rules, None);
    //Reset the synthetic knowledge fixture, retaining the same real source
    //object in Limbo; its next successful Unlimbo is the producer under test.
    sim.fog = vision::FogState::new(32, 32);
    let callback_rules = rules.clone();
    with_first_cell_receiver(
        move |sim| {
            let source = sim.fog.sight_admissions_for_test()[&(id, owner)].clone();
            assert!(
                source.latched,
                "full source metadata precedes the first receiver"
            );
            assert!(source.cells.len() > 1);
            let later = source.cells[1];
            assert!(
                !sim.fog.is_ground_open(owner, later.0, later.1),
                "metadata must not publish later Cell ground early"
            );
            sim.release_object_sight_with_context(id, UninitContext::with_rules(&callback_rules));
            let released = &sim.fog.sight_admissions_for_test()[&(id, owner)];
            assert!(!released.latched);
            assert_eq!(
                (released.origin, released.radius),
                (source.origin, source.radius)
            );
        },
        || {
            sim.reveal_entity_with_context(id, UninitContext::with_rules(&rules));
            assert!(!sim.substrate.entities.get(id).unwrap().lifecycle.in_limbo());
        },
    );
    assert!(!sim.fog.sight_is_latched(id, owner));
    assert!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .lifecycle
            .object_alive()
    );
    let cache = sim.fog.sight_admissions_for_test()[&(id, owner)].clone();
    let before = sim.fog.by_owner()[&owner].shroud_knowledge_raw().to_vec();
    let config = sim.sight_reveal_config(&rules);
    sim.refresh_sight_sources(&config, &rules);
    assert_eq!(sim.fog.sight_admissions_for_test()[&(id, owner)], cache);
    assert!(!sim.fog.sight_is_latched(id, owner));
    assert_eq!(sim.fog.by_owner()[&owner].shroud_knowledge_raw(), before);
}

/// Original Techno6F6D04/D0C admits its whole suffix before the first sight
/// receiver. This supplied callback control changes +90 during that receiver;
/// it does not claim an ordinary Discover naturally kills the source.
#[test]
fn unlimbo_keeps_its_single_alive_admission_across_sight_callbacks() {
    let Some((mut sim, rules, _)) = retail_arena() else {
        return;
    };
    sim.fog = vision::FogState::new(32, 32);
    let owner = sim.intern("Americans");
    let id = sim
        .spawn_object("AMCV", "Americans", 16, 16, 0, &rules)
        .unwrap();
    let type_id = sim.substrate.entities.get(id).unwrap().type_ref();
    let category = crate::map::entities::EntityCategory::Unit;
    sim.techno_limbo_with_rules(id, &rules, None);
    let before = sim.houses[&owner]
        .tracking()
        .active_count(category, type_id);
    sim.fog = vision::FogState::new(32, 32);
    with_first_cell_receiver(
        move |sim| {
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .lifecycle
                .set_object_alive_for_test(false);
        },
        || {
            sim.reveal_entity_with_context(
                id,
                UninitContext::with_rules(&rules).with_unlimbo_facing(Some(37)),
            );
        },
    );
    let source = sim.substrate.entities.get(id).unwrap();
    assert!(!source.lifecycle.object_alive());
    assert_eq!(
        sim.houses[&owner]
            .tracking()
            .active_count(category, type_id),
        before + 1,
        "6F6D8F Added_To_Game follows the pre-callback alive admission"
    );
    assert_eq!(
        source.body_facing_current(sim.session.binary_frame()),
        u16::from(37_u8) << 8,
        "6F6DAA facing belongs to the same admitted suffix"
    );
}

/// One normal production session, shared by the source and uncached-caller
/// comparisons. Native VM controls remain supplied cropped-scene inputs;
/// this fixture does not claim that its whole-map startup was replayed there.
fn retail_goldst_initial_scenario() -> crate::headless_scenario::HeadlessScenario {
    use crate::skirmish_launch::{
        AiDifficulty, LaunchCountry, LaunchStartPosition, SkirmishLaunchOptions,
    };
    let retail = std::path::PathBuf::from(
        std::env::var("RA2_DIR").expect("RA2_DIR names the retail install"),
    );
    let mut session = crate::headless_scenario::battle_session(
        "GoldSt.mmx",
        &[(LaunchCountry::France, AiDifficulty::Easy)],
    );
    session.local_player_mut().name = "VERA-OBSERVER".to_owned();
    session.local_slot_mut().country = LaunchCountry::America;
    session.local_slot_mut().start_position = LaunchStartPosition::Position(3);
    session.opponents[0].start_position = LaunchStartPosition::Position(0);
    session.options = SkirmishLaunchOptions {
        starting_credits: 10_000,
        unit_count: 0,
        tech_level: 10,
        game_speed: 1,
        default_ai_difficulty: 0,
        short_game: false,
        bases: true,
        bridges_destroyable: true,
        super_weapons: true,
        build_off_ally: true,
        crates: true,
        mcv_redeploy: true,
        fog_of_war: false,
        shroud: true,
        tiberium_grows: true,
        multi_engineer: false,
        harvester_truce: false,
        ally_change_allowed: true,
    };
    let scenario =
        crate::headless_scenario::load_session(&retail, "GoldSt.mmx", 0x1234_5678, session)
            .expect("ordinary GoldSt Battle load");
    scenario
}

/// Normal production GoldSt Battle initialization, before any frame or draw.
/// Original6F6D27/5678E0/4A9CA0 plus6F6D61/567DA0 observations are retained
/// by the existing procedural drawing oracle. Its original scene supplies a
/// cropped physical map and bounded successful placement, not whole-map boot;
/// this joins only the selected source and eight pre-presentation Cell states.
#[test]
#[ignore = "requires RA2_DIR with installed retail RA2/YR assets and stock GoldSt.mmx"]
fn retail_goldst_initial_amcv_matches_native_before_presentation() {
    let scenario = retail_goldst_initial_scenario();
    let _ = assert_retail_goldst_initial_amcv(scenario.sim());
}

/// Shared native1777 prerequisites, before either normal-map experiment runs.
fn assert_retail_goldst_initial_amcv(sim: &Simulation) -> (u64, Vec<(u16, u16)>) {
    use serde_json::json;
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/shroud_frontier.json",
    ))
    .unwrap();
    assert_eq!(corpus["inputs"]["startup"]["b0cd48"], "60493474535ec23f");
    let history = &corpus["histories"][0];
    assert_eq!(history["name"], "initial_amcv");
    assert_eq!(
        sim.session.tick(),
        0,
        "no Logic frame supplies initial knowledge"
    );
    assert!(sim.session.game_options().shroud);
    assert!(!sim.session.game_options().fog_of_war);
    let owner = sim.session.current_house().expect("normal local player");
    assert_eq!(sim.interner.resolve(owner), "VERA-OBSERVER");
    let amcv = sim.interner.get("AMCV").expect("retail AMCV type");
    let sources: Vec<_> = sim
        .entities()
        .iter_sorted()
        .filter(|(_, entity)| entity.owner() == owner && entity.type_ref() == amcv)
        .collect();
    assert_eq!(sources.len(), 1, "one ordinary local starting MCV");
    let (id, source) = sources[0];
    assert!(source.lifecycle.object_alive());
    assert!(!source.lifecycle.in_limbo());
    assert!(source.in_playfield());
    assert_eq!(
        json!([
            u8::from(source.discovery.owned_by_current_house()),
            u8::from(source.discovery.discovered_by_current_house()),
            u8::from(source.discovery.discovered_by_other_house()),
        ]),
        history["source_states"]["discovery_bytes_41a_41c"]["after_unlimbo"],
        "the source's native discovery consumers complete before return",
    );
    let location =
        crate::sim::movement::ground_pose::object_location(source, sim.resolved_terrain.as_ref());
    let expected = &history["source_states"]["cache_and_rng"]["after_unlimbo"];
    assert_eq!(json!([location.x, location.y, location.z]), expected["xyz"]);
    let cache = &sim.fog.sight_admissions_for_test()[&(id, owner)];
    assert_eq!(
        u64::from(cache.latched),
        expected["latch"].as_u64().unwrap()
    );
    assert_eq!(
        i64::from(cache.radius),
        expected["radius"].as_i64().unwrap()
    );
    // Native254 XYZ is represented by a cell origin plus exact Z. This
    // ordinary center-position source is also checked in full leptons above.
    assert_eq!(
        json!([cache.origin.0, cache.origin.1]),
        corpus["inputs"]["source"]["coords"]
    );
    assert_eq!(
        i64::from(cache.origin.2),
        expected["xyz"][2].as_i64().unwrap()
    );
    let view = &sim.fog.by_owner()[&owner];
    let rows = history["before_draw_cells"].as_array().unwrap();
    assert_eq!(rows.len(), 8);
    let mut witnesses = Vec::with_capacity(rows.len());
    for row in rows {
        let cell = (
            row["cell"][0].as_u64().unwrap() as u16,
            row["cell"][1].as_u64().unwrap() as u16,
        );
        witnesses.push(cell);
        let index = usize::from(cell.1) * usize::from(sim.fog.width()) + usize::from(cell.0);
        let state = view.shroud_knowledge_raw()[index];
        let (frames, flags) = view.map_cell_edge_cache_for_test(cell);
        assert_eq!(json!(frames), row["cache"], "edge cache at {cell:?}");
        assert_eq!(
            u64::from(flags),
            row["flags"].as_u64().unwrap() & 3,
            "140 at {cell:?}"
        );
        assert_eq!(
            i64::from(state.counter()),
            row["counter"].as_i64().unwrap(),
            "counter at {cell:?}"
        );
        assert_eq!(
            i64::from(state.gap_counter()),
            row["gap"].as_i64().unwrap(),
            "gap at {cell:?}"
        );
        let native_flags = row["alt"].as_u64().unwrap();
        assert_eq!(
            u64::from(view.ground_flags_raw()[index]),
            native_flags & 0x18,
            "12C ground at {cell:?}"
        );
        assert_eq!(
            state.open(),
            native_flags & 8 != 0,
            "knowledge open at {cell:?}"
        );
        assert_eq!(
            state.pending(),
            row["flags"].as_u64().unwrap() & 0x20 != 0,
            "pending at {cell:?}"
        );
        assert_eq!(
            u64::from(
                sim.resolved_terrain
                    .as_ref()
                    .unwrap()
                    .cell(cell.0, cell.1)
                    .unwrap()
                    .level
            ),
            row["level"].as_u64().unwrap(),
            "physical level at {cell:?}",
        );
    }
    // No refresh, terrain edits, visibility hooks, commands or presentation
    // ran between normal load and these producer/cache observations.
    (id, witnesses)
}

/// Ordinary runtime Move -> two Unit arrivals -> Stop/idle on stock GoldSt.
/// Native1777 supplies the initial prerequisites; the independently executed
/// moving corpus supplies event-relative Drive/Unit cache and Cell witnesses.
/// Prepared native startup timing is not a whole-match timing golden.
#[test]
#[ignore = "requires RA2_DIR with installed retail RA2/YR assets and stock GoldSt.mmx"]
fn retail_goldst_amcv_sight_follows_arrivals_then_survives_stop() {
    use crate::sim::components::{DriveCoord, TrackProgress};
    use crate::sim::intern::InternedId;
    use crate::sim::movement::ground_pose::object_location;
    use crate::sim::movement::track_process::TrackFamily;
    use crate::sim::world::TickLane;
    let native =
        crate::test_fixture::json("tools/procedural_drawing_oracle/shroud_frontier_moving.json");
    assert_eq!(native["flight_queries"]["height"], 0);
    assert_eq!(native["flight_queries"]["high"], 0);

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct CellObservation {
        cell: (u16, u16),
        counter: i32,
        gap: i32,
        local_sources: u32,
        allied_sources: u32,
        open: bool,
        pending: bool,
        ground: u8,
        edge: ([i8; 2], u32),
    }

    #[derive(Debug)]
    struct Observation {
        tick: u64,
        xyz: DriveCoord,
        physical_cell: (u16, u16),
        admission: vision::SightAdmission,
        head: Option<DriveCoord>,
        destination: Option<DriveCoord>,
        progress: Option<TrackProgress>,
        moving: Option<bool>,
        cells: Vec<CellObservation>,
    }

    fn observe(
        sim: &Simulation,
        id: u64,
        owner: InternedId,
        selected: &[(u16, u16)],
    ) -> Observation {
        let source = sim.entities().get(id).expect("starting AMCV survives");
        assert!(source.lifecycle.object_alive());
        assert!(!source.lifecycle.in_limbo());
        assert!(source.in_playfield());
        assert_eq!(source.owner(), owner);
        let admission = sim.fog.sight_admissions_for_test()[&(id, owner)].clone();
        assert!(admission.latched);
        assert_eq!(admission.owner, owner);
        assert!(!admission.cells.is_empty());
        let local_sources: Vec<_> = sim
            .fog
            .sight_admissions_for_test()
            .iter()
            .filter(|&(&(_, viewer), admission)| viewer == owner && admission.latched)
            .map(|(&(source, _), _)| source)
            .collect();
        assert_eq!(local_sources, [id], "one ordinary local sustained source");
        assert!(
            sim.fog
                .alternate_sight_admissions_for_test()
                .keys()
                .all(|&(_, viewer)| viewer != owner),
            "this control has no alternate local sight source"
        );
        let view = &sim.fog.by_owner()[&owner];
        // The normal frame already validates the whole derived source
        // index. Keep this integration observation on named Cell controls.
        let cells = selected
            .iter()
            .copied()
            .map(|cell| {
                let index =
                    usize::from(cell.1) * usize::from(sim.fog.width()) + usize::from(cell.0);
                let state = view.shroud_knowledge_raw()[index];
                assert_eq!(
                    state.local_sources(),
                    u32::from(admission.cells.contains(&cell)),
                    "tick{}: retained source index at{cell:?}",
                    sim.session.tick(),
                );
                assert_eq!(state.allied_sources(), 0, "no allied source at{cell:?}");
                if state.local_sources() == 1 {
                    assert_eq!(
                        state.counter(),
                        -1,
                        "one Cell487630 contribution at{cell:?}"
                    );
                    assert_eq!(state.gap_counter(), 0);
                    assert!(state.open());
                }
                CellObservation {
                    cell,
                    counter: state.counter(),
                    gap: state.gap_counter(),
                    local_sources: state.local_sources(),
                    allied_sources: state.allied_sources(),
                    open: state.open(),
                    pending: state.pending(),
                    ground: view.ground_flags_raw()[index],
                    edge: view.map_cell_edge_cache_for_test(cell),
                }
            })
            .collect();
        let loco = source.locomotor.as_ref().expect("retail AMCV Drive");
        Observation {
            tick: sim.session.tick(),
            xyz: object_location(source, sim.resolved_terrain.as_ref()),
            physical_cell: (source.position.rx(), source.position.ry()),
            admission,
            head: loco.track_head(TrackFamily::Drive),
            destination: loco.track_destination(TrackFamily::Drive),
            progress: loco.track_progress(TrackFamily::Drive),
            moving: crate::sim::movement::motion_query::is_moving(source),
            cells,
        }
    }

    let mut scene = retail_goldst_initial_scenario();
    let (id, mut selected) = assert_retail_goldst_initial_amcv(scene.sim());
    let owner = scene.sim().session.current_house().unwrap();
    let cache = &scene.sim().fog.sight_admissions_for_test()[&(id, owner)];
    let left = *cache.cells.iter().min().unwrap();
    let right = *cache.cells.iter().max().unwrap();
    // Probe two departure/entry fringes around the loaded footprint. These
    // choose observation cells only: actual cache membership below decides
    // whether each probe left/entered. No expected footprint is computed.
    selected.extend([
        left,
        (left.0 + 1, left.1),
        (right.0 + 1, right.1),
        (right.0 + 2, right.1),
        (108, 18),
    ]);
    selected.sort_unstable();
    selected.dedup();
    let initial = observe(scene.sim(), id, owner, &selected);
    assert_eq!(initial.physical_cell, (109, 27));
    assert_eq!(
        (initial.admission.origin.0, initial.admission.origin.1),
        (109, 27)
    );
    let radius = initial.admission.radius;
    let z = initial.xyz.z;
    let order = CommandEnvelope::new(
        owner,
        scene.sim().session.tick() + 1,
        Command::Move {
            entity_id: id,
            target_rx: 111,
            target_ry: 27,
        },
    );
    let mut observations = vec![initial];
    let mut origins = vec![(109, 27)];
    let mut crossings = std::collections::BTreeSet::new();
    let mut completed = false;
    let mut release_witnesses = 0;
    let mut new_witnesses = 0;
    // This is an exploration bound, not an expected native arrival frame.
    // Game-speed1 captures use22ms; HeadlessScenario::tick defaults to66ms.
    for step in 0..160 {
        let commands = if step == 0 {
            std::slice::from_ref(&order)
        } else {
            &[]
        };
        let (output, calls) = trace_reveal_calls(|| {
            scene
                .runtime
                .advance_frame(commands, 22, TickLane::Ordinary, FrameEffects::default())
                .expect("normal AMCV movement frame")
        });
        assert!(output.tick.frame_committed);
        let current = observe(scene.sim(), id, owner, &selected);
        let previous = observations.last().unwrap();
        assert_eq!(current.admission.radius, radius);
        assert_eq!(current.admission.origin.2, z);
        assert_eq!(current.xyz.z, z, "selected ordinary level ground");
        let origin = (current.admission.origin.0, current.admission.origin.1);
        if current.physical_cell != origin {
            if crossings.insert(current.physical_cell) {
                eprintln!(
                    "AMCV crossing tick{} XYZ{:?} cell{:?} retained{:?} head{:?} track{:?}",
                    current.tick,
                    current.xyz,
                    current.physical_cell,
                    current.admission.origin,
                    current.head,
                    current.progress,
                );
            }
            assert_eq!(
                current.admission, previous.admission,
                "physical cell crossing alone does not refresh retained sight"
            );
        }
        if current.admission.origin != previous.admission.origin {
            assert_eq!(origin, current.physical_cell);
            let arrival = &native["arrivals"][origins.len() - 1];
            let expected = &arrival["after_border"];
            let cache = &expected["cache"];
            assert_eq!(
                current.admission.radius,
                cache["radius"].as_i64().unwrap() as i32
            );
            assert_eq!(
                current.admission.origin,
                (
                    (cache["xyz"][0].as_i64().unwrap() >> 8) as u16,
                    (cache["xyz"][1].as_i64().unwrap() >> 8) as u16,
                    cache["xyz"][2].as_i64().unwrap() as i32,
                )
            );
            assert_eq!(
                serde_json::json!([current.xyz.x, current.xyz.y, current.xyz.z]),
                arrival["after_pass"]["actor"]["position"],
                "event-relative postpass pose"
            );
            for row in expected["cells"].as_array().unwrap() {
                let cell = (
                    row["cell"][0].as_u64().unwrap() as u16,
                    row["cell"][1].as_u64().unwrap() as u16,
                );
                let Some(actual) = current.cells.iter().find(|state| state.cell == cell) else {
                    continue;
                };
                assert_eq!(
                    actual.counter,
                    row["counter"].as_i64().unwrap() as i32,
                    "native counter {cell:?}"
                );
                assert_eq!(
                    actual.gap,
                    row["gap"].as_i64().unwrap() as i32,
                    "native gap {cell:?}"
                );
                assert_eq!(actual.ground, (row["alt"].as_u64().unwrap() & 0x18) as u8);
                assert_eq!(
                    actual.edge.0[0],
                    row["cache"][0].as_i64().unwrap() as i8,
                    "arrival updates native ground edge immediately at {cell:?}"
                );
                // Native warm drawing also refreshes fog-edge cache. This
                // pre-presentation test compares the shared ground channel.
            }
            // End-of-frame XYZ may already include the next paid points;
            // do not equate it with a subframe arrival's exact coordinate.
            assert!(calls.walks.len() >= 2, "release and update reach Map5678E0");
            // Trace walks are aggregate: no source/final-pass tag is claimed.
            // Cell state and retained geometry independently guard the source
            // handoff; the native producer must still establish exact order.
            let released: Vec<_> = previous
                .admission
                .cells
                .iter()
                .filter(|cell| !current.admission.cells.contains(cell))
                .collect();
            let added: Vec<_> = current
                .admission
                .cells
                .iter()
                .filter(|cell| !previous.admission.cells.contains(cell))
                .collect();
            assert!(!released.is_empty(), "arrival releases an old-only fringe");
            assert!(!added.is_empty(), "arrival adds a new-only fringe");
            let released_before = release_witnesses;
            let added_before = new_witnesses;
            for before in &previous.cells {
                let cell = before.cell;
                if !previous.admission.cells.contains(&cell)
                    || current.admission.cells.contains(&cell)
                {
                    continue;
                }
                let after = current
                    .cells
                    .iter()
                    .find(|state| state.cell == cell)
                    .unwrap();
                assert_eq!(before.counter, -1);
                assert_eq!(before.local_sources, 1);
                assert_eq!(after.local_sources, 0);
                assert_eq!(after.counter, 0, "released ordinary Cell487690 at{cell:?}");
                assert_eq!(after.gap, 0);
                release_witnesses += 1;
            }
            for after in &current.cells {
                let cell = after.cell;
                if !current.admission.cells.contains(&cell)
                    || previous.admission.cells.contains(&cell)
                {
                    continue;
                }
                let before = previous
                    .cells
                    .iter()
                    .find(|state| state.cell == cell)
                    .unwrap();
                assert_eq!(before.local_sources, 0);
                assert_eq!(after.local_sources, 1);
                assert_eq!(after.counter, -1, "one ordinary Cell487630 at{cell:?}");
                assert_eq!(after.gap, 0);
                assert!(after.open);
                new_witnesses += 1;
            }
            assert!(
                release_witnesses > released_before,
                "observed departure fringe at each arrival"
            );
            assert!(
                new_witnesses > added_before,
                "observed entry fringe at each arrival"
            );
            let changed_cells = previous
                .cells
                .iter()
                .zip(&current.cells)
                .filter(|(before, after)| before != after)
                .count();
            eprintln!(
                "AMCV sight tick{} XYZ{:?} origin{:?}->{:?} head{:?} track{:?} walks{} notifications{} changed_cells{} released{} added{}",
                current.tick,
                current.xyz,
                previous.admission.origin,
                current.admission.origin,
                current.head,
                current.progress,
                calls.walks.len(),
                calls.notifications.len(),
                changed_cells,
                released.len(),
                added.len(),
            );
            let before = previous
                .cells
                .iter()
                .find(|state| {
                    previous.admission.cells.contains(&state.cell)
                        && !current.admission.cells.contains(&state.cell)
                })
                .unwrap();
            let after = current
                .cells
                .iter()
                .find(|state| state.cell == before.cell)
                .unwrap();
            eprintln!("AMCV released Cell{:?}: {before:?}->{after:?}", before.cell);
            origins.push(origin);
        } else {
            assert_eq!(
                current.admission, previous.admission,
                "an unchanged arrival cache keeps its footprint"
            );
        }
        let arrived = current.physical_cell == (111, 27)
            && origin == (111, 27)
            && current.head.is_none()
            && current.destination.is_none()
            && current.moving == Some(false);
        observations.push(current);
        if arrived {
            completed = true;
            break;
        }
    }
    assert!(completed, "ordinary Move did not finish within160 frames");
    assert_eq!(origins, [(109, 27), (110, 27), (111, 27)]);
    assert!(
        crossings.contains(&(110, 27)),
        "first crossing precedes its reveal arrival"
    );
    assert!(
        crossings.contains(&(111, 27)),
        "second crossing precedes its reveal arrival"
    );
    assert!(release_witnesses > 0 && new_witnesses > 0);
    let settled = observations.last().unwrap();
    // Original Drive -> Unit739EC0 ->567DA0 changes this ground frame
    // immediately, before Logic578100 can repair a missed suffix at120.
    let native_border = native["arrivals"][1]["after_border"]["cells"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["cell"] == serde_json::json!([108, 18]))
        .unwrap();
    let border = settled
        .cells
        .iter()
        .find(|state| state.cell == (108, 18))
        .unwrap();
    assert_eq!(
        border.edge,
        (
            [
                native_border["cache"][0].as_i64().unwrap() as i8,
                native_border["cache"][1].as_i64().unwrap() as i8
            ],
            native_border["flags"].as_u64().unwrap() as u32 & 3
        ),
        "original Unit arrival border, before Stop/120"
    );
    assert_eq!(
        border.counter,
        native_border["counter"].as_i64().unwrap() as i32
    );
    assert_eq!(
        border.ground,
        native_border["alt"].as_u64().unwrap() as u8 & 0x18
    );
    let stop = CommandEnvelope::new(
        owner,
        scene.sim().session.tick() + 1,
        Command::Stop { entity_id: id },
    );
    let until_boundary = (120 - scene.sim().session.binary_frame() % 120) % 120;
    let idle_frames = (until_boundary + 2).max(8);
    let mut crossed_boundary = false;
    for idle in 0..idle_frames {
        let binary_frame = scene.sim().session.binary_frame();
        crossed_boundary |= binary_frame % 120 == 0;
        let commands = if idle == 0 {
            std::slice::from_ref(&stop)
        } else {
            &[]
        };
        let (output, _calls) = trace_reveal_calls(|| {
            scene
                .runtime
                .advance_frame(commands, 22, TickLane::Ordinary, FrameEffects::default())
                .expect("ordinary Stop then idle frame")
        });
        assert!(output.tick.frame_committed);
        let current = observe(scene.sim(), id, owner, &selected);
        assert_eq!(
            current.xyz, settled.xyz,
            "Stop after completion does not move the source"
        );
        assert_eq!(
            current.admission, settled.admission,
            "Stop/idle retains the last admitted footprint"
        );
        assert_eq!(current.cells.len(), settled.cells.len());
        for (after, before) in current.cells.iter().zip(&settled.cells) {
            assert_eq!(
                after, before,
                "Stop/idle tick{} prebinary{binary_frame} idle{idle} Cell{:?}: no duplicate contribution or cache change",
                current.tick, after.cell,
            );
        }
        assert!(current.head.is_none());
        assert!(current.destination.is_none());
        assert_eq!(current.moving, Some(false));
    }
    assert!(
        crossed_boundary,
        "Stop/idle includes the real Logic120 boundary"
    );
}

/// A supplied callback changes PlayerPtr during a native source event. The
/// admitted original player remains the sole notification receiver across
/// later logical planes; this is control flow, not a natural Tag trigger claim.
#[test]
fn a_source_event_keeps_one_notification_admission_across_logical_planes() {
    let Some((mut sim, rules, _)) = retail_arena() else {
        return;
    };
    let owner = sim.intern("Americans");
    let ally = sim.intern("Alliance");
    assert!(
        owner < ally,
        "the fixture visits the original player plane first"
    );
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            ally, 0, None, false, 0, 10,
        ));
    // Supplied direct alliance prestate; the test exercises source event
    // admission, not the separate diplomacy producer.
    sim.house_alliances_mut_for_test()
        .entry("AMERICANS".into())
        .or_default()
        .insert("ALLIANCE".into());
    sim.fog = vision::FogState::new(32, 32);
    let id = sim
        .spawn_object("AMCV", "Americans", 16, 16, 0, &rules)
        .unwrap();
    sim.techno_limbo_with_rules(id, &rules, None);
    sim.fog = vision::FogState::new(32, 32);
    let ((), calls) = trace_reveal_calls(|| {
        with_first_cell_receiver(
            move |sim| sim.session.set_current_house_for_test(Some(ally)),
            || {
                sim.reveal_entity_with_context(id, UninitContext::with_rules(&rules));
            },
        );
    });
    assert_eq!(sim.session.current_house(), Some(ally), "the callback ran");
    let source = &sim.fog.sight_admissions_for_test()[&(id, owner)];
    assert!(source.cells.len() > 1);
    assert_eq!(
        calls.notifications.len(),
        source.cells.len(),
        "one native admitted walk; the later projected ally is not a second callback walk",
    );
    assert!(sim.fog.sight_is_latched(id, ally));
    assert!(sim.fog.is_cell_revealed(owner, 16, 16));
    assert!(sim.fog.is_cell_revealed(ally, 16, 16));
}

/// Action40's actual70ADC0/5673A0 caller transports a translated local XYZ
/// to567DA0, unlike the raw-XYZ Unlimbo suffix. The native controls supply
/// input overrides explicitly; no poisoned Cell-cache state or hand-derived
/// footprint is used. Warm native prior includes presentation caches; this
/// core comparison checks executed receiver order, resulting ground frames
/// and persistent source metadata, not full prior/draw-cache equivalence.
#[test]
#[ignore = "requires RA2_DIR with installed retail RA2/YR assets and stock GoldSt.mmx"]
fn retail_goldst_uncached_reveal_border_matches_native_admission_and_transport() {
    use crate::sim::components::DriveCoord;
    use serde_json::json;
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/shroud_frontier.json",
    ))
    .unwrap();
    let cases = corpus["controls"]["uncached_reveal"]["cases"]
        .as_array()
        .unwrap();
    assert_eq!(cases.len(), 5);
    for case in cases {
        let mut scenario = retail_goldst_initial_scenario();
        let runtime = &mut scenario.runtime;
        let rules = &runtime.resources.rules;
        let sim = &mut runtime.simulation;
        let local = sim.session.current_house().unwrap();
        let amcv = sim.interner.get("AMCV").unwrap();
        let id = sim
            .entities()
            .iter_sorted()
            .find_map(|(id, entity)| {
                (entity.owner() == local && entity.type_ref() == amcv).then_some(id)
            })
            .unwrap();
        let foreign = sim
            .houses
            .order()
            .iter()
            .copied()
            .find(|owner| *owner != local)
            .unwrap();
        let cache_value = |sim: &Simulation| {
            let cache = &sim.fog.sight_admissions_for_test()[&(id, local)];
            let xyz = DriveCoord::cell(cache.origin.0, cache.origin.1, cache.origin.2);
            json!({"latch": u8::from(cache.latched), "xyz": [xyz.x, xyz.y, xyz.z], "radius": cache.radius})
        };
        let name = case["name"].as_str().unwrap();
        assert!(matches!(
            name,
            "admitted_empty_height_walk"
                | "elevated_goldst"
                | "zero_sight"
                | "foreign_owner_rejected"
                | "invalid_lifted_center"
        ));
        assert_eq!(
            cache_value(sim),
            case["source_cache_before"],
            "cache before {name}"
        );
        let input = &case["input"];
        assert_eq!(input["entry"], "0070ADC0");
        assert_eq!(input["args"], json!([0, 0]));
        let xyz = input["source_xyz"].as_array().unwrap();
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.position.put_location(DriveCoord {
            x: i32::try_from(xyz[0].as_i64().unwrap()).unwrap(),
            y: i32::try_from(xyz[1].as_i64().unwrap()).unwrap(),
            z: i32::try_from(xyz[2].as_i64().unwrap()).unwrap(),
        });
        entity
            .sight
            .stamp_type_sight(i32::try_from(input["type_sight"].as_i64().unwrap()).unwrap());
        // Declared native owner-field control, kept consistent with the store
        // column. This does not pretend to execute the ChangeOwner mechanism.
        let supplied_owner = match input["owner"].as_str().unwrap() {
            "current_player" => local,
            "supplied_nonallied_house" => foreign,
            other => panic!("unexpected native House boundary {other}"),
        };
        sim.substrate.entities.change_owner(id, supplied_owner);
        if let Some(level) = input["registered_level_byte"].as_u64() {
            let level = u8::try_from(level).unwrap();
            let terrain = sim.resolved_terrain.as_mut().unwrap();
            // Native overrides registered Cells only. The allocation-aware
            // iterator is also the existing PathGrid reader's authority.
            let coords: Vec<_> = terrain.iter().map(|cell| (cell.rx, cell.ry)).collect();
            for (x, y) in coords {
                terrain.cell_mut_for_test(x, y).unwrap().level = level;
            }
            // Derive the LOS cache through its actual existing terrain reader.
            // Full Rust map vs sparse native crop is bounded to these receivers.
            sim.path_grid = Some(std::sync::Arc::new(
                PathGrid::from_resolved_terrain_with_bridges(terrain, sim.overlay_grid.as_ref()),
            ));
        }
        let before_view = serde_json::to_value(&sim.fog.by_owner()[&local]).unwrap();
        let before_rng = (
            sim.scenario_rng.native_state_hex(),
            sim.main_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        );
        let ((), trace) = trace_reveal_calls(|| {
            sim.reveal_uncached_object_sight(
                id,
                false,
                false,
                None,
                UninitContext::with_rules(rules),
            );
        });
        assert_eq!(
            trace.walks.len(),
            case["map_calls"].as_array().unwrap().len(),
            "Map call count {name}"
        );
        let border = case["border"].as_array().unwrap();
        let targets: Vec<(u16, u16)> = border
            .iter()
            .flat_map(|call| {
                call["targets"].as_array().unwrap().iter().map(|cell| {
                    (
                        u16::try_from(cell[0].as_u64().unwrap()).unwrap(),
                        u16::try_from(cell[1].as_u64().unwrap()).unwrap(),
                    )
                })
            })
            .collect();
        assert_eq!(
            trace.border_receivers, targets,
            "actual border receivers {name}"
        );
        let ground_after: Vec<i8> = border
            .iter()
            .flat_map(|call| {
                call["cells"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|cell| i8::try_from(cell[3].as_i64().unwrap()).unwrap())
            })
            .collect();
        assert_eq!(
            trace.border_ground_after, ground_after,
            "ground cache after each receiver {name}"
        );
        assert_eq!(
            cache_value(sim),
            case["source_cache_after"],
            "cache after {name}"
        );
        assert_eq!(
            case["source_cache_before"], case["source_cache_after"],
            "native uncached call retains250/254/260 {name}"
        );
        assert_eq!(case["rng_unchanged"], true);
        assert_eq!(
            (
                sim.scenario_rng.native_state_hex(),
                sim.main_rng.native_state_hex(),
                sim.mapgen_rng.native_state_hex()
            ),
            before_rng,
            "no RNG {name}"
        );
        if matches!(
            name,
            "zero_sight" | "foreign_owner_rejected" | "invalid_lifted_center"
        ) {
            assert!(
                case["changed_cells"]["after_area"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            assert!(
                case["changed_cells"]["after_border"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                serde_json::to_value(&sim.fog.by_owner()[&local]).unwrap(),
                before_view,
                "native rejected call has no current-player cache/knowledge effect {name}"
            );
        }
        if name == "admitted_empty_height_walk" {
            assert!(trace.walks.iter().all(Vec::is_empty));
            assert!(
                !trace.border_receivers.is_empty(),
                "empty LOS is admitted, not rejected"
            );
        }
    }
}

/// Executed original Map5673A0→border6→rawborder4→Logic578100.
/// This component starts from actual native constructor/prefix knowledge;
/// the separate loaded Carrier test proves ordinary FireAt admission. Native
/// outputs never take expected knowledge, frames or callbacks from Rust.
#[test]
#[ignore = "requires RA2_DIR with installed retail RA2/YR assets and stock GoldSt.mmx"]
fn retail_goldst_cold_fire_borders_and_conceal_notifications_match_native() {
    for indexed in [false, true] {
        use serde_json::{Value, json};
        use std::collections::BTreeMap;
        let native: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/procedural_drawing_oracle/shroud_frontier_fire.json",
        ))
        .unwrap();
        // The full physical level/coordinate input comes from these exact retail
        // bytes, independently identified by native execution's source record.
        let retail = std::path::PathBuf::from(std::env::var("RA2_DIR").unwrap());
        let assets = crate::assets::asset_manager::AssetManager::new(
            &retail,
            crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
        )
        .unwrap();
        let selected = crate::map::source::load_map_by_name_or_path_with_assets(
            &retail,
            "GoldSt.mmx",
            &assets,
        )
        .unwrap();
        let native_map = native["inputs"]["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|source| {
                source["name"]
                    .as_str()
                    .unwrap()
                    .eq_ignore_ascii_case("GoldSt.mmx")
            })
            .unwrap();
        assert_eq!(
            serde_json::to_value(selected.source).unwrap()["source_sha256"],
            native_map["sha256"]
        );
        let mut scenario = retail_goldst_initial_scenario();
        let rules = &scenario.runtime.resources.rules;
        let sim = &mut scenario.runtime.simulation;
        let viewer = sim.session.current_house().unwrap();
        let coord = |v: &Value| (v[0].as_i64().unwrap() as u16, v[1].as_i64().unwrap() as u16);
        let row = |v: &Value| {
            json!({
                "cell": [v[0],v[1]], "level": v[2], "counter":v[3],
                "gap":v[4], "alt":v[5], "flags":v[6], "cache":[v[7],v[8]],
            })
        };
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let cells: Vec<_> = terrain
            .native_cell_iterator(103)
            .map(|cell| ((cell.rx, cell.ry), cell.level))
            .collect();
        assert_eq!(
            cells.len() as u64,
            native["inputs"]["allocated_knowledge_cells"]
                .as_u64()
                .unwrap()
        );
        let order_bytes: Vec<_> = cells
            .iter()
            .flat_map(|&((x, y), _)| {
                (x as i16)
                    .to_le_bytes()
                    .into_iter()
                    .chain((y as i16).to_le_bytes())
            })
            .collect();
        let order_hash = crate::util::sha256::sha256_hex(&order_bytes);
        for sweep in native["sweeps"].as_array().unwrap() {
            for pass in sweep["passes"].as_array().unwrap() {
                assert_eq!(pass["ordered_coords_sha256"], order_hash);
                assert_eq!(pass["count"], cells.len());
            }
        }
        let cold = row(&native["initial_source_cell"]);
        let mut expected: BTreeMap<_, _> = cells
            .iter()
            .map(|&(xy, level)| {
                let mut state = cold.clone();
                state["cell"] = json!([xy.0, xy.1]);
                state["level"] = json!(level);
                (xy, state)
            })
            .collect();
        for state in native["initial_nonconstructor_cells"].as_array().unwrap() {
            expected.insert(coord(state), row(state));
        }
        // The selected visible neighborhoods are entirely real. Neither an
        // outside/Dummy prior nor its final stamping is certified by this corpus.
        for xy in native["borders"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|b| b["cells"].as_array().unwrap())
            .map(coord)
            .chain(
                native["initial_nonconstructor_cells"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(coord),
            )
        {
            for dx in -1i16..=1 {
                for dy in -1i16..=1 {
                    assert!(
                        expected
                            .contains_key(&((xy.0 as i16 + dx) as u16, (xy.1 as i16 + dy) as u16))
                    );
                }
            }
        }
        for notification in native["notifications"].as_array().unwrap() {
            let xy = coord(&notification["cell"]);
            for rung in 0u16..7 {
                assert!(expected.contains_key(&(xy.0 + rung, xy.1 + rung)));
            }
        }
        let topology = json!({
            "allocated_cells": expected.values().collect::<Vec<_>>(),
            "dimensions": [sim.fog.width(),sim.fog.height()],
            // Existing executed receiver fixture supplies the unexamined Dummy.
            "dummy_prior": vision::discovery_corpus()["cache_controls"]["topology"]["dummy_prior"],
        });
        let (fog, _) = vision::native_map_cell_fixture(&topology, viewer);
        sim.fog = fog;
        let actual_xyz = {
            let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(
                sim.resolved_terrain.as_ref().unwrap(),
            );
            let p = coord(&native["inputs"]["physical_fire_cell"]["coords"]);
            crate::sim::cell_kernel::native_cell_own_coords(
                cells.lookup((p.0 as i16, p.1 as i16)),
                &cells,
            )
            .unwrap()
        };
        assert_eq!(
            json!([actual_xyz.0, actual_xyz.1, actual_xyz.2]),
            native["inputs"]["native_coordinate"]["xyz"]
        );
        let origin = (
            (actual_xyz.0 / 256) as u16,
            (actual_xyz.1 / 256) as u16,
            actual_xyz.2 as i32,
        );
        let config = sim.sight_reveal_config(rules);
        let heights = sim.sight_height_grid(&config);
        let context = UninitContext::with_rules(rules);
        let rng = (
            sim.scenario_rng.native_state_hex(),
            sim.main_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        );
        if indexed {
            // Establish the derived index through its production owner. All native
            // input fields and callback history must stay unchanged while priming.
            let (_, trace) = trace_reveal_calls(|| sim.update_shroud_edges(viewer, context));
            assert_eq!(trace.edge_visits, 2 * cells.len());
            assert!(trace.notifications.is_empty());
            for state in expected.values() {
                vision::assert_native_cell_history(
                    &sim.fog,
                    viewer,
                    state,
                    sim.resolved_terrain.as_ref().unwrap(),
                    false,
                );
            }
        }
        let mut stage_index = 0;
        let mut compare_stage = |sim: &Simulation| {
            let stage = &native["stages"][stage_index];
            stage_index += 1;
            for state in stage["changes"].as_array().unwrap() {
                let xy = coord(state);
                assert_eq!(expected[&xy]["level"], state[2], "physical level");
                expected.insert(xy, row(state));
            }
            for state in expected.values() {
                vision::assert_native_cell_history(
                    &sim.fog,
                    viewer,
                    state,
                    sim.resolved_terrain.as_ref().unwrap(),
                    false,
                );
            }
        };
        for phase in ["cold_fire", "warm_repeat"] {
            let (local, trace) = trace_reveal_calls(|| {
                sim.reveal_area(
                    &[viewer],
                    (origin.0, origin.1),
                    origin.2,
                    3,
                    Some(&heights),
                    false,
                    &config,
                    context,
                    vision::MapRevealProjection::Fire,
                    Some(viewer),
                )
            });
            let leaf: Vec<_> = native["leaf_receivers"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|r| r["phase"] == phase)
                .map(|r| coord(&r["cell"]))
                .collect();
            assert_eq!(trace.walks, [leaf]);
            assert!(
                trace.notifications.is_empty(),
                "fire leaf never calls5865F0"
            );
            compare_stage(sim);
            for (border_origin, radius) in [(local.unwrap(), 6), (origin, 4)] {
                let (_, trace) = trace_reveal_calls(|| {
                    Simulation::refresh_sight_border(
                        &mut sim.fog,
                        &sim.resolved_terrain,
                        viewer,
                        border_origin,
                        radius,
                        context,
                    )
                });
                let border = native["borders"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|b| b["phase"] == phase && b["radius"] == radius)
                    .unwrap();
                let receivers: Vec<_> = border["cells"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(coord)
                    .collect();
                let frames: Vec<_> = border["cells"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| r[3].as_i64().unwrap() as i8)
                    .collect();
                assert_eq!(trace.border_receivers, receivers);
                assert_eq!(trace.border_ground_after, frames);
                assert!(trace.notifications.is_empty());
                compare_stage(sim);
            }
        }
        for frame in [119, 120, 240] {
            sim.session.set_binary_frame_for_test(frame);
            let (_, trace) = trace_reveal_calls(|| sim.tick_pending_shroud_conceal(context));
            if frame == 120 {
                if indexed {
                    assert!(
                        trace.edge_visits > 0 && trace.edge_visits < 2 * cells.len(),
                        "the completed index must use the sparse production walk"
                    );
                } else {
                    assert_eq!(trace.edge_visits, 2 * cells.len());
                }
            }
            let phase = format!("logic_{frame}");
            let expected_calls: Vec<_> = native["notifications"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|r| r["phase"] == phase)
                .map(|r| {
                    assert_eq!(r["house_is_current"], true);
                    (r["sweep_pass"] == 1, coord(&r["cell"]), r["fresh"] == 1)
                })
                .collect();
            assert_eq!(
                trace.edge_notifications, expected_calls,
                "actual notification pass/order at{frame}"
            );
            assert_eq!(
                trace.notifications,
                expected_calls
                    .iter()
                    .map(|&(_, cell, fresh)| (cell, fresh))
                    .collect::<Vec<_>>()
            );
            compare_stage(sim);
        }
        assert_eq!(stage_index, native["stages"].as_array().unwrap().len());
        assert_eq!(
            rng,
            (
                sim.scenario_rng.native_state_hex(),
                sim.main_rng.native_state_hex(),
                sim.mapgen_rng.native_state_hex()
            )
        );
        assert_eq!(native["rng"]["before"], native["rng"]["after"]);
        assert_eq!(native["rng"]["requests"], json!([]));
    }
}

/// Native releases a Techno's sight only at its Limbo (`TechnoClass::Limbo`,
/// `0x006F6B06`, `0x006F6B16`): an infantry corpse keeps its sight through
/// its death sequence until the sequencer's UnInit.
#[test]
fn an_infantry_corpse_keeps_its_sight_until_its_uninit() {
    let art = crate::rules::ini_parser::IniFile::from_str(
        &crate::rules::retail_ini_fixture::GI_ART_EXCERPT.replace("Die1=134,15,0", "Die1=134,3,0"),
    );
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[InfantryTypes]\n0=E1\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [E1]\nImage=GI\nStrength=100\nSpeed=4\nSight=5\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
    );
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    sim.fog = vision::FogState::new(32, 32);
    let owner = sim.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    let id = sim
        .spawn_object("E1", "Americans", 16, 16, 0, &rules)
        .unwrap();
    assert!(sim.fog.sight_is_latched(id, owner));
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .health
        .set_for_test(0);
    sim.begin_infantry_death_sequence(
        id,
        crate::sim::world::InfantryDeathSequence::Die1,
        &rules,
        FrameEffects::default(),
    );
    let mut visits = 0;
    while sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|corpse| corpse.lifecycle.object_alive())
    {
        assert!(
            sim.fog.sight_is_latched(id, owner),
            "visit {visits}: the corpse holds its sight"
        );
        sim.advance_tick(&[], Some(&rules), None, None, 100);
        visits += 1;
        assert!(visits < 10, "the three-frame Die1 ends");
    }
    assert!(visits > 1, "the corpse took visits");
    assert!(!sim.fog.sight_is_latched(id, owner));
}

/// A computer House added to the arena's House array.
fn add_computer_house(sim: &mut Simulation, name: &str) -> crate::sim::intern::InternedId {
    let id = sim.intern(name);
    let side = sim.houses.order().len() as u8;
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            id, side, None, false, 0, 10,
        ));
    id
}

/// #1886: an Unlimbo before `ScenarioClass::Post_Map_Init` publishes the
/// path grid tests its cells' line of sight against the levels the map read
/// gave them (`Cell+0x11B`, `0x00567CB5`), as one after it does, so the
/// first release after publication walks the cells the reveal walked and
/// leaves none of them in sight.
#[test]
fn a_reveal_before_navigation_publication_tests_the_cell_heights() {
    let Some((mut sim, rules, _)) = retail_arena() else {
        return;
    };
    sim.fog = vision::FogState::new(32, 32);
    let owner = sim.intern("Americans");
    let id = sim
        .spawn_object("MTNK", "Americans", 12, 16, 0, &rules)
        .unwrap();
    let flat = sim.fog.sight_admission(id, owner).unwrap().cells.clone();
    sim.techno_limbo_with_rules(id, &rules, None);
    // A cliff east of the tank, read with the map, before navigation is
    // published.
    let terrain = sim.resolved_terrain.as_mut().unwrap();
    for y in 0..32 {
        terrain.cell_mut_for_test(15, y).unwrap().level = 8;
    }
    sim.path_grid = None;
    sim.reveal_entity_with_context(id, UninitContext::with_rules(&rules));
    assert!(!sim.substrate.entities.get(id).unwrap().lifecycle.in_limbo());
    let early = sim.fog.sight_admission(id, owner).unwrap().cells.clone();
    assert_ne!(early, flat, "the cliff hides cells before publication");
    let grid = PathGrid::from_resolved_terrain(sim.resolved_terrain.as_ref().unwrap());
    sim.install_fixture_path_grid(Some(&grid));
    sim.refresh_object_sight(id, &rules, 0, crate::sim::world::FrameEffects::default());
    assert_eq!(
        sim.fog.sight_admission(id, owner).unwrap().cells,
        early,
        "the same cells after publication"
    );
    sim.techno_limbo_with_rules(id, &rules, None);
    let config = sim.sight_reveal_config(&rules);
    sim.refresh_sight_sources(&config, &rules);
    for y in 0..32 {
        for x in 0..32 {
            assert!(!sim.fog.is_cell_visible(owner, x, y), "({x}, {y})");
        }
    }
}

/// `TechnoClass::ChangeOwner @ 0x007014A0` moves an object's sight within
/// the call: its ReReveal before the owner store (`0x0070172F`) takes it
/// off the old owner's map, and its UpdateReveal (`0x0070187B`) opens it on
/// the new owner's.
#[test]
fn an_owner_change_moves_the_sight_to_the_new_owner_within_the_call() {
    let Some((mut sim, rules, _)) = retail_arena() else {
        return;
    };
    sim.fog = vision::FogState::new(32, 32);
    let americans = sim.intern("Americans");
    let russians = add_computer_house(&mut sim, "Russians");
    let id = sim
        .spawn_object("MTNK", "Americans", 16, 16, 0, &rules)
        .unwrap();
    assert!(sim.fog.sight_is_latched(id, americans));
    sim.change_owner_with_rules(id, russians, &rules, None, FrameEffects::default());
    assert!(!sim.fog.sight_is_latched(id, americans), "0x0070172F");
    assert!(sim.fog.sight_is_latched(id, russians), "0x0070187B");
    assert!(!sim.fog.is_cell_visible(americans, 16, 16));
    assert!(sim.fog.is_cell_visible(russians, 16, 16));
}

/// MakeAlly and MakeEnemy bracket their ally-bit write with the map
/// callbacks (`0x004ADEE0` before it, at `0x004F9B9A`/`0x004F9FD8`;
/// `0x004ADCD0` after it, at `0x004F9F4F`/`0x004FA1E3`), whose
/// allied-Building arm alone takes another House's object (`0x004ADDA6`,
/// `0x004ADF7D`). A Building's sight leaves the map of a House an alliance
/// break stops counting as an ally within the call, and opens there again
/// within the next alliance's call, that release having cleared its latch
/// (`0x0070B202`). A mobile takes no sight callback: its sight reaches the
/// ally's map at its next reveal event, and no frame pass replaces the
/// footprint its last one stored.
#[test]
fn an_alliance_moves_the_allys_building_sight_within_the_call() {
    let Some((mut sim, rules, _)) = retail_arena() else {
        return;
    };
    sim.fog = vision::FogState::new(32, 32);
    let americans = sim.intern("Americans");
    let russians = add_computer_house(&mut sim, "Russians");
    // Can_Ally refuses an alliance that leaves no live enemy.
    add_computer_house(&mut sim, "Confederation");
    let standing = sim
        .spawn_object("NAPOWR", "Russians", 22, 8, 0, &rules)
        .unwrap();
    let tank = sim
        .spawn_object("MTNK", "Russians", 8, 22, 0, &rules)
        .unwrap();
    // The Spy Plane's kind of reveal: a radius that replaces its Sight=.
    sim.refresh_object_sight(tank, &rules, 7, crate::sim::world::FrameEffects::default());
    let tank_sight = sim.fog.sight_admission(tank, russians).unwrap().clone();
    assert_eq!(tank_sight.radius, 7);
    assert!(!sim.fog.sight_is_latched(standing, americans));

    sim.make_ally(
        russians,
        americans,
        &rules,
        None,
        FrameEffects::default(),
        false,
    );
    // RESIDUAL (`Simulation::admit_alliance_sight`): native's latch
    // (`+0x250`), set by the plant's Unlimbo, makes this `UpdateReveal`
    // return (`0x0070B0B1`); VERA opens it here.
    assert!(sim.fog.sight_is_latched(standing, americans));
    // The re-admission gives a computer House's non-Building only its gap
    // callback (`0x004ADE2D..0x004ADEBF`).
    assert!(!sim.fog.sight_is_latched(tank, americans));
    let config = sim.sight_reveal_config(&rules);
    sim.refresh_sight_sources(&config, &rules);
    assert!(
        !sim.fog.sight_is_latched(tank, americans),
        "the frame pass reveals nothing"
    );
    assert_eq!(
        sim.fog.sight_admission(tank, russians),
        Some(&tank_sight),
        "the frame pass keeps the last reveal's footprint"
    );

    // A plant placed while the alliance stands opens on the ally's map at
    // its Unlimbo: RevealArea's house gate admits the ally under
    // `AllyReveal=` (`0x00567AEF`).
    let plant = sim
        .spawn_object("NAPOWR", "Russians", 22, 22, 0, &rules)
        .unwrap();
    let origin = sim.fog.sight_admission(plant, russians).unwrap().origin;
    assert!(sim.fog.sight_is_latched(plant, americans));
    assert!(sim.fog.is_cell_visible(americans, origin.0, origin.1));

    sim.make_enemy(
        russians,
        americans,
        &rules,
        None,
        FrameEffects::default(),
        false,
    );
    assert!(
        !sim.fog.sight_is_latched(plant, americans),
        "0x004F9FD8's release precedes the bit write"
    );
    assert!(!sim.fog.is_cell_visible(americans, origin.0, origin.1));
    assert!(sim.fog.sight_is_latched(plant, russians));

    sim.make_ally(
        russians,
        americans,
        &rules,
        None,
        FrameEffects::default(),
        false,
    );
    assert!(
        sim.fog.sight_is_latched(plant, americans),
        "0x004ADDE4 passes UpdateReveal's latch test"
    );
    assert!(sim.fog.is_cell_visible(americans, origin.0, origin.1));
    assert!(!sim.fog.sight_is_latched(tank, americans));
}

/// Playfield promotion (`0x006F511A`) follows the arrival's reveal (Unit
/// `0x0073AC74`), which the stored playfield byte refuses: a tank that
/// drives into the playfield opens its sight at its next arrival.
#[test]
fn a_tank_promoted_into_the_playfield_reveals_at_its_next_arrival() {
    let Some((mut sim, rules, grid)) = retail_arena() else {
        return;
    };
    sim.fog = vision::FogState::new(32, 32);
    let owner = sim.intern("Americans");
    let id = sim
        .spawn_object("MTNK", "Americans", 10, 10, 0, &rules)
        .unwrap();
    // Fixture: a tank Unlimboed off the playfield, whose stored byte refused
    // its Unlimbo reveal.
    sim.release_object_sight_with_context(id, UninitContext::with_rules(&rules));
    sim.fog.forget_sight_source(id);
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .set_in_playfield(false);
    let order = CommandEnvelope::new(
        owner,
        sim.session.tick() + 1,
        Command::Move {
            entity_id: id,
            target_rx: 13,
            target_ry: 10,
        },
    );
    let _ = sim.advance_tick(&[order], Some(&rules), Some(&grid), None, 67);
    let (mut promoted_at, mut sight_at) = (None, None);
    for _ in 0..400 {
        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
        let entity = sim.substrate.entities.get(id).unwrap();
        if entity.in_playfield() && promoted_at.is_none() {
            promoted_at = Some((entity.position.rx(), entity.position.ry()));
        }
        if sim.fog.sight_is_latched(id, owner) {
            sight_at = Some(sight_origin(&sim, id));
            break;
        }
    }
    assert_eq!(promoted_at, Some((11, 10)), "promoted at its first arrival");
    assert_eq!(sight_at, Some((12, 10)), "its sight opens at the next one");
}

/// Infantry Per_Cell_Process ends its reveal with the Map567DA0 border
/// (`0x0051A8F1..0x0051A925`): around his current XYZ, out to his cached
/// radius (`+0x260`) + 3, as the Unit tail's (`0x0073AC7A..0x0073ACAE`).
#[test]
fn an_infantry_arrival_refreshes_its_sight_border() {
    let Some((mut sim, rules, grid)) = retail_arena() else {
        return;
    };
    let man = sim
        .spawn_object("E1", "Americans", 12, 12, 0, &rules)
        .unwrap();
    let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    let ((), trace) = trace_reveal_calls(|| {
        sim.per_cell_process(
            man,
            crate::sim::movement::PerCellReason::Arrival,
            &rules,
            None,
            FrameEffects::default(),
        )
        .unwrap();
    });

    let owner = sim.substrate.entities.get(man).unwrap().owner();
    let radius = sim.fog.sight_admission(man, owner).unwrap().radius;
    assert!(radius > 0);
    let z = crate::sim::movement::ground_pose::object_world_z_leptons(
        sim.substrate.entities.get(man).unwrap(),
        sim.resolved_terrain.as_ref(),
    );
    let border = vision::sight_border_cells((12, 12, z), radius + 3, 0);
    assert!(!border.is_empty());
    assert_eq!(trace.border_receivers, border);
}

#[test]
fn make_ally_refreshes_own_sight_after_admission_and_skips_a_refused_repeat() {
    let Some((mut sim, rules, _)) = retail_arena() else {
        return;
    };
    sim.fog = vision::FogState::new(32, 32);
    let player = sim.intern("Americans");
    let other = add_computer_house(&mut sim, "Russians");
    add_computer_house(&mut sim, "Confederation");
    let id = sim
        .spawn_object("MTNK", "Americans", 8, 22, 0, &rules)
        .unwrap();
    assert!(
        sim.entities()
            .get(id)
            .unwrap()
            .discovery
            .discovered_by_current_house()
    );
    sim.refresh_object_sight(id, &rules, 7, crate::sim::world::FrameEffects::default());
    let before = sim.fog.sight_admission(id, player).unwrap().clone();
    assert_eq!(before.radius, 7);
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .position
        .set_rx_for_test(9);
    sim.make_ally(other, player, &rules, None, FrameEffects::default(), false);
    let refreshed = sim.fog.sight_admission(id, player).unwrap().clone();
    assert_eq!((refreshed.origin.0, refreshed.origin.1), (9, 22));
    assert_eq!(refreshed.radius, rules.object("MTNK").unwrap().sight);
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .position
        .set_rx_for_test(10);
    // CanAlly rejects the existing edge before the pre-release visitor.
    sim.make_ally(other, player, &rules, None, FrameEffects::default(), false);
    assert_eq!(sim.fog.sight_admission(id, player), Some(&refreshed));
}
