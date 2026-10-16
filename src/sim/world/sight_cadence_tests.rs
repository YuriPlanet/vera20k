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
    sim.houses.insert(
        owner,
        crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
    );
    sim.session.house_order.push(owner);
    sim.session.current_house = Some(owner);
    Some((sim, rules, grid))
}

/// The cell source `id`'s own sight was last revealed around.
fn sight_origin(sim: &Simulation, id: u64) -> (u16, u16) {
    let owner = sim.substrate.entities.get(id).unwrap().owner();
    let admission = &sim.fog.sight_admissions[&(id, owner)];
    (admission.origin.0, admission.origin.1)
}

/// TechnoUnlimbo6F6D27 dispatches70AF50 before AddedToGame/facing/idle,
/// not from the next Logic frame. Its5678E0 receiver reaches4A9CA0 and
/// initializes both Cell edge channels even with FogOfWar=no.
#[test]
fn successful_unlimbo_reveals_its_cells_before_returning() {
    let Some((mut sim, rules, _grid)) = retail_arena() else {
        return;
    };
    sim.fog = vision::FogState::new(32, 32);
    let owner = sim.intern("Americans");
    sim.session.current_house = Some(owner);
    assert!(!sim.fog.is_cell_revealed(owner, 16, 16));
    let id = sim
        .spawn_object("AMCV", "Americans", 16, 16, 0, &rules)
        .expect("ordinary successful Unlimbo");
    assert_eq!(sim.session.tick, 0, "no frame pass supplies the reveal");
    assert!(sim.entities().get(id).unwrap().in_playfield());
    assert!(
        sim.fog.is_cell_revealed(owner, 16, 16),
        "native Unlimbo opens Cell knowledge synchronously"
    );
    assert_eq!(sight_origin(&sim, id), (16, 16));
    let (_, flags) = sim.fog.by_owner[&owner].map_cell_edge_cache_for_test((16, 16));
    assert_ne!(flags & 2, 0, "Map4A9CA0 initializes the Cell cache");
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
            sim.session.tick + 1,
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
    let held = sim.fog.sight_admissions[&(id, owner)].clone();
    assert!(held.latched);
    sim.techno_limbo_with_rules(id, &rules, None);
    let cache = sim.fog.sight_admissions[&(id, owner)].clone();
    assert!(!cache.latched);
    assert_eq!((cache.origin, cache.radius), (held.origin, held.radius));
    assert!(
        cache.cells.is_empty(),
        "inactive cache retains no active footprint index"
    );
    let config = sim.sight_reveal_config(Some(&rules));
    sim.refresh_sight_sources(&config, Some(&rules));
    assert_eq!(sim.fog.sight_admissions[&(id, owner)], cache);
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "inactive-sight-cache", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    //The existing native Scenario load owner resets its RNG independently;
    //do not alter the live constructor stream to make two phases hash alike.
    let hash = restored.state_hash();
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(restored.fog.sight_admissions[&(id, owner)], cache);
    assert_eq!(restored.state_hash(), hash);
    // No competing cache: changing retained260 changes the lockstep hash.
    restored
        .fog
        .sight_admissions
        .get_mut(&(id, owner))
        .unwrap()
        .radius += 1;
    assert_ne!(restored.state_hash(), hash);
    let pending_cache = restored.fog.sight_admissions[&(id, owner)].clone();
    restored.uninit_with_rules(id, &rules);
    // ObjectUnInit5F6625 clears alive and queues the object; it does not
    // free its storage. The inactive254/260 cache lives through that window.
    assert!(restored.entities().get(id).is_some());
    assert!(restored.substrate.pending_delete.contains_object(&id));
    assert_eq!(restored.fog.sight_admissions[&(id, owner)], pending_cache);
    // Main_Tick55DE9F reaches the shared725C70 drain after frame commit.
    // Use that real rules-bearing destructor owner, not a direct store/cache
    // removal or the test-only rules-less flush wrapper.
    restored.process_pending_delete_with(
        Some(&rules),
        None,
        FrameEffects::default(),
        1,
        restored.session.started(),
    );
    assert!(restored.entities().get(id).is_none());
    assert!(!restored.substrate.pending_delete.contains_object(&id));
    assert!(!restored.fog.sight_admissions.keys().any(|key| key.0 == id));
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
    sim.houses.insert(
        owner,
        crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
    );
    sim.session.house_order.push(owner);
    let zero = sim
        .spawn_object("ZERO", "Americans", 16, 16, 0, &rules)
        .unwrap();
    assert!(!sim.fog.sight_is_latched(zero, owner));
    assert!(!sim.fog.is_cell_revealed(owner, 16, 16));
    let negative = sim
        .spawn_object("NEGATIVE", "Americans", 24, 24, 0, &rules)
        .unwrap();
    let admission = &sim.fog.sight_admissions[&(negative, owner)];
    assert!(admission.latched);
    assert_eq!(admission.radius, -1);
    assert!(admission.cells.is_empty());
    sim.refresh_object_sight(zero, Some(&rules), 3);
    assert!(sim.fog.sight_is_latched(zero, owner));
    assert_eq!(sim.fog.sight_admissions[&(zero, owner)].radius, 3);
    assert!(sim.fog.is_cell_revealed(owner, 16, 16));
    let held = sim.fog.sight_admissions[&(zero, owner)].clone();
    let config = sim.sight_reveal_config(Some(&rules));
    sim.refresh_sight_sources(&config, Some(&rules));
    assert_eq!(
        sim.fog.sight_admissions[&(zero, owner)],
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
            let source = sim.fog.sight_admissions[&(id, owner)].clone();
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
            let released = &sim.fog.sight_admissions[&(id, owner)];
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
    let cache = sim.fog.sight_admissions[&(id, owner)].clone();
    let before = sim.fog.by_owner[&owner].shroud_knowledge_raw().to_vec();
    let config = sim.sight_reveal_config(Some(&rules));
    sim.refresh_sight_sources(&config, Some(&rules));
    assert_eq!(sim.fog.sight_admissions[&(id, owner)], cache);
    assert!(!sim.fog.sight_is_latched(id, owner));
    assert_eq!(sim.fog.by_owner[&owner].shroud_knowledge_raw(), before);
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
    let before = sim.houses[&owner].tracking.active_count(category, type_id);
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
        sim.houses[&owner].tracking.active_count(category, type_id),
        before + 1,
        "6F6D8F Added_To_Game follows the pre-callback alive admission"
    );
    assert_eq!(
        source.body_facing_current(sim.session.binary_frame),
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
    session.player_name = "VERA-OBSERVER".to_owned();
    session.local.country = LaunchCountry::America;
    session.local.start_position = LaunchStartPosition::Position(3);
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
    use serde_json::json;
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/shroud_frontier.json",
    ))
    .unwrap();
    assert_eq!(corpus["inputs"]["startup"]["b0cd48"], "60493474535ec23f");
    let history = &corpus["histories"][0];
    assert_eq!(history["name"], "initial_amcv");
    let scenario = retail_goldst_initial_scenario();
    let sim = scenario.sim();
    assert_eq!(
        sim.session.tick, 0,
        "no Logic frame supplies initial knowledge"
    );
    assert!(sim.session.game_options.shroud);
    assert!(!sim.session.game_options.fog_of_war);
    let owner = sim.session.current_house.expect("normal local player");
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
    let cache = &sim.fog.sight_admissions[&(id, owner)];
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
    let view = &sim.fog.by_owner[&owner];
    let rows = history["before_draw_cells"].as_array().unwrap();
    assert_eq!(rows.len(), 8);
    for row in rows {
        let cell = (
            row["cell"][0].as_u64().unwrap() as u16,
            row["cell"][1].as_u64().unwrap() as u16,
        );
        let index = usize::from(cell.1) * usize::from(sim.fog.width) + usize::from(cell.0);
        let state = view.shroud_knowledge_raw()[index];
        let (frames, flags) = view.map_cell_edge_cache_for_test(cell);
        assert_eq!(json!(frames), row["cache"], "edge cache at {cell:?}");
        assert_eq!(
            u64::from(flags),
            row["flags"].as_u64().unwrap() & 3,
            "140 at {cell:?}"
        );
        assert_eq!(
            i64::from(state.counter),
            row["counter"].as_i64().unwrap(),
            "counter at {cell:?}"
        );
        assert_eq!(
            i64::from(state.gap_counter),
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
            state.open,
            native_flags & 8 != 0,
            "knowledge open at {cell:?}"
        );
        assert_eq!(
            state.pending,
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
    sim.houses.insert(
        ally,
        crate::sim::house_state::HouseState::new(ally, 0, None, false, 0, 10),
    );
    // Supplied direct alliance prestate; the test exercises source event
    // admission, not the separate diplomacy producer.
    sim.house_alliances
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
            move |sim| sim.session.current_house = Some(ally),
            || {
                sim.reveal_entity_with_context(id, UninitContext::with_rules(&rules));
            },
        );
    });
    assert_eq!(sim.session.current_house, Some(ally), "the callback ran");
    let source = &sim.fog.sight_admissions[&(id, owner)];
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
        let local = sim.session.current_house.unwrap();
        let amcv = sim.interner.get("AMCV").unwrap();
        let id = sim
            .entities()
            .iter_sorted()
            .find_map(|(id, entity)| {
                (entity.owner() == local && entity.type_ref() == amcv).then_some(id)
            })
            .unwrap();
        let foreign = sim
            .session
            .house_order
            .iter()
            .copied()
            .find(|owner| *owner != local)
            .unwrap();
        let cache_value = |sim: &Simulation| {
            let cache = &sim.fog.sight_admissions[&(id, local)];
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
                terrain.cell_mut(x, y).unwrap().level = level;
            }
            // Derive the LOS cache through its actual existing terrain reader.
            // Full Rust map vs sparse native crop is bounded to these receivers.
            sim.path_grid = Some(std::sync::Arc::new(
                PathGrid::from_resolved_terrain_with_bridges(terrain),
            ));
        }
        let before_view = serde_json::to_value(&sim.fog.by_owner[&local]).unwrap();
        let before_rng = (
            sim.scenario_rng.native_state_hex(),
            sim.main_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        );
        let ((), trace) = trace_reveal_calls(|| {
            sim.reveal_uncached_object_sight(id, UninitContext::with_rules(rules));
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
            trace.uncached_border_receivers, targets,
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
            trace.uncached_border_ground_after, ground_after,
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
                serde_json::to_value(&sim.fog.by_owner[&local]).unwrap(),
                before_view,
                "native rejected call has no current-player cache/knowledge effect {name}"
            );
        }
        if name == "admitted_empty_height_walk" {
            assert!(trace.walks.iter().all(Vec::is_empty));
            assert!(
                !trace.uncached_border_receivers.is_empty(),
                "empty LOS is admitted, not rejected"
            );
        }
    }
}
