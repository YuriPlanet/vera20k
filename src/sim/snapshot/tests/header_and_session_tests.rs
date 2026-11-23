//! Snapshot tests for the envelope and session: header identity and version
//! gates, the state-hash round trip, validated loads, game modes, custom
//! rules and network identity, and the saved-type layout.

use super::*;

#[test]
fn custom_snapshot_refuses_a_malformed_retained_rng_cursor() {
    use crate::sim::scenario_session::ScenarioGameMode;

    let mut sim = Simulation::new();
    let mode = ScenarioGameMode::Skirmish;
    let policy = mode
        .stock_rule_policy(None)
        .with_allows_manual_alliances(false);
    sim.install_session_admission(mode, None, policy);
    let map = sim.session.map_name().to_owned();
    sim.scenario_rng = serde_json::from_value(serde_json::json!({
        "disabled": 0, "index_a": 250, "index_b": 0, "state": vec![0u32; 250]
    }))
    .unwrap();
    let bytes = GameSnapshot::save(&sim, 1, 2, &map, 0);
    assert!(matches!(
        GameSnapshot::load_validated(&bytes, 1, 2, &map),
        Err(SnapshotError::InvalidRandomCursor)
    ));
}

/// A save whose Scenario RNG table is short is refused while it is read,
/// with the table's length in the message.
#[test]
fn snapshot_refuses_a_short_rng_table() {
    let mut sim = Simulation::new();
    sim.scenario_rng = crate::sim::rng::SimRng::new(0xC0FF_EE11);
    let map = sim.session.map_name().to_owned();
    let bytes = GameSnapshot::save(&sim, 1, 2, &map, 0);
    // The table's wire form is a length and then the words, as a Vec's.
    let saved = save_codec::serialize(&sim.scenario_rng).unwrap();
    let at = bytes
        .windows(saved.len())
        .position(|window| window == saved)
        .expect("the Scenario RNG is saved");
    let short = save_codec::serialize(&(0u8, 0i32, 0i32, vec![0u32; 3])).unwrap();
    let mut damaged = bytes[..at].to_vec();
    damaged.extend_from_slice(&short);
    damaged.extend_from_slice(&bytes[at + saved.len()..]);
    let error = GameSnapshot::load_validated(&damaged, 1, 2, &map)
        .err()
        .expect("a short table is refused");
    assert!(
        error.to_string().contains("holds 3 words, not 250"),
        "{error}"
    );
}

/// Prove snapshot round-trip preserves all authoritative state.
///
/// 1. Create a Simulation, advance N ticks
/// 2. Save snapshot -> bytes -> load snapshot
/// 3. Advance both the loaded sim and a reference sim for M more ticks
/// 4. Assert both reach the same state hash
#[test]
fn round_trip_preserves_state_hash() {
    // Create two identical simulations from the same seed.
    let mut sim_a = Simulation::new();
    let mut sim_b = Simulation::new();

    // Advance both for 50 ticks to build up some state.
    for _ in 0..50 {
        tick(&mut sim_a);
        tick(&mut sim_b);
    }

    // Native in-scenario load restarts Scenario RNG from Seed0. Put both
    // source/reference branches on that cursor before testing unrelated
    // snapshot persistence and continued deterministic execution.
    sim_a.scenario_rng = crate::sim::rng::SimRng::new(0);
    sim_b.scenario_rng = crate::sim::rng::SimRng::new(0);

    // Snapshot sim_a at tick 50.
    let hash_at_50 = sim_a.state_hash();
    let bytes = GameSnapshot::save(&sim_a, 0, 0, "test_map", 0);

    // Load the snapshot.
    let snapshot = GameSnapshot::load(&bytes).expect("load should succeed");
    let mut sim_loaded = snapshot.sim;

    // Verify the loaded sim has the same state hash as the original at tick 50.
    assert_eq!(
        sim_loaded.state_hash(),
        hash_at_50,
        "loaded snapshot must match original state hash at save point"
    );

    // Advance both the original and loaded sims for 50 more ticks.
    for _ in 0..50 {
        tick(&mut sim_a);
        tick(&mut sim_loaded);
    }

    // Both must reach the same state hash at tick 100.
    assert_eq!(
        sim_a.state_hash(),
        sim_loaded.state_hash(),
        "original and loaded sim must reach identical state after continued ticking"
    );

    // The reference sim (never serialized) must also match.
    for _ in 0..50 {
        tick(&mut sim_b);
    }
    assert_eq!(
        sim_a.state_hash(),
        sim_b.state_hash(),
        "reference sim (never serialized) must match serialized sim"
    );
}

#[test]
fn current_header_with_missing_body_reports_deserialization_failure() {
    let bytes = save_codec::serialize(&GameSnapshotHeader {
        product_magic: SNAPSHOT_PRODUCT_MAGIC,
        envelope_version: SNAPSHOT_ENVELOPE_VERSION,
        layout: snapshot_layout(),
        description: "current fixture".to_string(),
        map_hash: 1,
        rules_hash: 2,
        tick: 3,
        save_timestamp: 4,
        map_name: "current-layout".to_string(),
        compatibility: RulesCompatibility::default(),
    })
    .expect("serialize current header only");

    assert!(matches!(
        GameSnapshot::load(&bytes),
        Err(SnapshotError::DeserializeFailed(_))
    ));
}

#[test]
fn gsi_17_02_header_roundtrip_carries_identity_versions_and_description() {
    let mut sim = Simulation::new();
    sim.session
        .set_map_name_for_test("OFFICIAL.MAP".to_string());
    let bytes =
        GameSnapshot::save_validated(&sim, 0x1234, 0x5678, "Hold the northern ridge", 0x9abc);

    let header = GameSnapshot::read_header(&bytes).expect("current VERA header");
    assert_eq!(header.product_magic, SNAPSHOT_PRODUCT_MAGIC);
    assert_eq!(header.envelope_version, SNAPSHOT_ENVELOPE_VERSION);
    assert_eq!(header.layout, snapshot_layout());
    assert_eq!(header.compatibility, RulesCompatibility::default());
    assert_eq!(header.description, "Hold the northern ridge");
    assert_eq!(header.map_name, "OFFICIAL.MAP");
}

#[test]
fn snapshot_capability_rejects_foreign_mode_before_body_decode() {
    let bytes = GameSnapshot::save_validated(&Simulation::new(), 0, 0, "synthetic", 0);
    let mut header = GameSnapshot::read_header(&bytes).unwrap();
    // Flip every capability bit: the result is a foreign mode for any build.
    // No build records the Ares bit any more, so it flips to set.
    header.compatibility = RulesCompatibility::from_capabilities(
        true,
        !crate::rules::PHOBOS_ENABLED,
        !crate::rules::NPATCH_ENABLED,
        !crate::rules::VERAX_ENABLED,
        !crate::rules::NPBUG_ENABLED,
    );
    // A complete header with no body proves capability admission runs before
    // Simulation deserialization, even without bound scenario resources.
    let bytes = save_codec::serialize(&header).unwrap();
    for result in [
        GameSnapshot::load(&bytes),
        GameSnapshot::load_with_resources(&bytes),
    ] {
        assert!(matches!(
            result,
            Err(SnapshotError::CapabilityMismatch { expected, found })
                if expected == RulesCompatibility::default() && found == header.compatibility
        ));
    }
    let metadata = GameSnapshot::read_metadata(&bytes).unwrap();
    assert_eq!(metadata.compatibility, header.compatibility);
    assert!(!metadata.is_compatible());
}

#[test]
fn snapshot_capability_keeps_legacy_metadata_readable_without_admitting_it() {
    let bytes = save_codec::serialize(&(
        SNAPSHOT_PRODUCT_MAGIC,
        1_u32,
        457_u32,
        "older layout",
        1_u64,
        2_u64,
        3_u64,
        4_u64,
        "OLD.MAP",
    ))
    .unwrap();
    let header = GameSnapshot::read_metadata(&bytes).unwrap();
    assert_eq!(header.layout, 457);
    assert_eq!(header.description, "older layout");
    assert_eq!(header.map_name, "OLD.MAP");
    assert!(!header.is_compatible());
    assert!(matches!(
        GameSnapshot::read_header(&bytes),
        Err(SnapshotError::EnvelopeVersionMismatch { found: 1, .. })
    ));
}

#[test]
fn previous_snapshot_version_stays_listed_and_refuses_body_load() {
    let bytes = GameSnapshot::save_validated(&Simulation::new(), 0, 0, "previous", 0);
    let mut header = GameSnapshot::read_header(&bytes).unwrap();
    header.envelope_version = 1;
    // Keep the current layout id to prove the outer version itself gates
    // admission before the loader can ask the decoder for a body.
    let bytes = save_codec::serialize(&header).unwrap();
    assert!(matches!(
        GameSnapshot::load_with_resources(&bytes),
        Err(SnapshotError::EnvelopeVersionMismatch {
            expected: 2,
            found: 1,
        })
    ));
    let metadata = GameSnapshot::read_metadata(&bytes).unwrap();
    assert_eq!(metadata.description, "previous");
    assert!(!metadata.is_compatible());
}

#[test]
fn gsi_17_02_wrong_product_is_rejected_before_missing_body_decode() {
    let foreign_preamble = GameSnapshotPreamble {
        product_magic: *b"NOTVERA\0",
        envelope_version: SNAPSHOT_ENVELOPE_VERSION,
        layout: snapshot_layout(),
    };
    let preamble_only = save_codec::serialize(&foreign_preamble).expect("foreign preamble");

    assert!(matches!(
        GameSnapshot::load(&preamble_only),
        Err(SnapshotError::ProductMismatch { found }) if found == *b"NOTVERA\0"
    ));
}

#[test]
fn gsi_17_02_public_and_internal_versions_gate_before_missing_body_decode() {
    let wrong_public = GameSnapshotPreamble {
        product_magic: SNAPSHOT_PRODUCT_MAGIC,
        envelope_version: SNAPSHOT_ENVELOPE_VERSION + 1,
        layout: snapshot_layout(),
    };
    let preamble_only = save_codec::serialize(&wrong_public).expect("public-version preamble");
    assert!(matches!(
        GameSnapshot::load(&preamble_only),
        Err(SnapshotError::EnvelopeVersionMismatch {
            expected: SNAPSHOT_ENVELOPE_VERSION,
            found: 3,
        })
    ));

    let wrong_internal = GameSnapshotPreamble {
        product_magic: SNAPSHOT_PRODUCT_MAGIC,
        envelope_version: SNAPSHOT_ENVELOPE_VERSION,
        layout: snapshot_layout() ^ 1,
    };
    let preamble_only = save_codec::serialize(&wrong_internal).expect("schema-version preamble");
    assert!(matches!(
        GameSnapshot::load(&preamble_only),
        Err(SnapshotError::LayoutMismatch {
            expected,
            found,
        }) if expected == snapshot_layout() && found == snapshot_layout() ^ 1
    ));
}

#[test]
fn stock_offline_modes_and_custom_rules_survive_snapshot() {
    use crate::rules::rule_policy::{AllowWinTagsPolicy, RulePolicy};
    use crate::sim::scenario_session::{ExecutionTopology, ScenarioDescriptor, ScenarioGameMode};

    let save = |sim: &Simulation| {
        GameSnapshot::save_validated_with_presentation(
            sim,
            0,
            0,
            "offline rules",
            0,
            &[],
            &[],
            None,
            None,
            None,
            None,
        )
    };
    for (raw_mode, rule_policy) in [
        (0, RulePolicy::stock_campaign()),
        (5, RulePolicy::stock_multiplayer()),
    ] {
        let sim = Simulation::synthetic(
            0,
            &ScenarioDescriptor {
                rule_policy,
                ..Default::default()
            },
        );
        let bytes = save(&sim).unwrap();
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(
            restored.session.game_mode(),
            ScenarioGameMode::from_game_mode(raw_mode),
        );
        assert_eq!(
            restored.session.execution_topology(),
            ExecutionTopology::LocalOffline
        );
        assert_eq!(restored.session.rule_policy(), rule_policy);
        assert_eq!(restored.state_hash(), sim.state_hash());
    }

    let custom =
        RulePolicy::stock_multiplayer().with_allow_win_tags(AllowWinTagsPolicy::RequireClear);
    let mut sim = Simulation::synthetic(
        0,
        &ScenarioDescriptor {
            rule_policy: custom,
            ..Default::default()
        },
    );
    assert_eq!(sim.session.rule_policy(), custom);
    let mode = ScenarioGameMode::from_game_mode(5);
    let class = crate::skirmish_modes::MpModeClass::Battle;
    let mut stock = sim.clone();
    stock.install_session_mode(mode);
    stock
        .session
        .install_multiplayer_house_identity(class, vec![]);
    sim.install_session_admission(mode, Some(class), custom);
    sim.session
        .install_multiplayer_house_identity(class, vec![]);
    assert_eq!(sim.session.rule_policy(), custom);
    assert_ne!(sim.state_hash(), stock.state_hash());
    let bytes = save(&sim).unwrap();
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(restored.session.rule_policy(), custom);
    assert_eq!(restored.state_hash(), sim.state_hash());
}

#[test]
fn custom_rules_and_canonical_control_grants_survive_snapshot() {
    use crate::rules::rule_policy::AllowWinTagsPolicy;
    use crate::sim::control_authority::{ControlGrant, ControlGrantLevel, ControlScope};
    use crate::sim::scenario_session::ScenarioGameMode;

    let mut sim = Simulation::with_seed(29);
    for (name, human) in [("Shared", true), ("Third", false)] {
        let owner = sim.intern(name);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, human, 0, 1,
            ));
    }
    sim.install_session_mode(ScenarioGameMode::Lan);
    let class = crate::skirmish_modes::MpModeClass::Battle;
    let policy = sim
        .session
        .rule_policy()
        .with_allow_win_tags(AllowWinTagsPolicy::RequireClear);
    sim.install_session_admission(ScenarioGameMode::Lan, Some(class), policy);
    let policy = sim.session.rule_policy();
    sim.session.install_network_timing(73);
    sim.session
        .install_shared_house_sources(vec![0, 0], Some(0));
    sim.scenario_rng.next_u32();
    sim.scenario_rng.next_u32();
    sim.mapgen_rng.next_u32();
    let stock = sim.state_hash();
    let units = ControlGrant {
        source: 1,
        house: 1,
        level: ControlGrantLevel::UnitsOnly,
    };
    let full = ControlGrant {
        source: 0,
        level: ControlGrantLevel::FullControl,
        ..units
    };
    let mut equivalent = sim.clone();
    sim.install_control_grants(vec![units, full]).unwrap();
    equivalent
        .install_control_grants(vec![full, units, full])
        .unwrap();
    assert_ne!(sim.state_hash(), stock);
    let granted = sim.state_hash();
    assert_eq!(equivalent.state_hash(), granted);
    let target = sim.interner.get("Third").unwrap();
    assert_eq!(
        sim.control_scope_for_source(0, target),
        ControlScope::FullControl
    );
    assert_eq!(
        sim.control_scope_for_source(1, target),
        ControlScope::UnitsOnly
    );
    sim.session
        .install_shared_house_sources(vec![0, 0], Some(1));
    assert_eq!(sim.state_hash(), granted);

    let bytes = GameSnapshot::save_validated_with_presentation(
        &sim,
        1,
        2,
        "custom policy and grants",
        0,
        &[],
        &[],
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let mut restored = GameSnapshot::load_validated(&bytes, 1, 2, sim.session.map_name())
        .unwrap()
        .sim;
    restored.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Cold(
        &crate::sim::rng::MainRng::new(79),
    ));
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(restored.session.seed(), 29);
    assert_eq!(restored.session.rule_policy(), policy);
    assert_eq!(restored.session.local_shared_house_source(), Some(1));
    assert_eq!(restored.state_hash(), granted);
    assert_eq!(
        sim.session.rule_policy().allow_win_tags(),
        AllowWinTagsPolicy::RequireClear
    );
    assert_eq!(
        restored.control_scope_for_source(0, target),
        ControlScope::FullControl
    );
    assert_eq!(
        restored.control_scope_for_source(1, target),
        ControlScope::UnitsOnly
    );
}

#[test]
fn custom_player_file_resumes_live_navigation_and_fallback_cell() {
    use crate::map::cell_index::NativeCellIdentity;
    use crate::rules::rule_policy::AllowWinTagsPolicy;
    use crate::sim::cell_rect::PlayfieldBounds;
    use crate::sim::control_authority::{ControlGrant, ControlGrantLevel};
    use crate::sim::occupancy::RawCellKey;
    use crate::sim::overlay_grid::OverlayGrid;
    use crate::sim::radiation::RadDetonation;
    use crate::sim::scenario_session::ScenarioGameMode;
    use crate::sim::world::SnapshotProcessState;

    let resources = crate::sim::runtime::SimResources::empty();
    let rules = &resources.rules;
    let mut live = Simulation::with_seed(29);
    let local = live.intern("Shared");
    let delegated = live.intern("Delegate");
    for (owner, human) in [(local, true), (delegated, false)] {
        live.houses
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, human, 0, 1,
            ));
    }
    let mode = ScenarioGameMode::Lan;
    let policy = mode
        .stock_rule_policy(None)
        .with_allow_win_tags(AllowWinTagsPolicy::RequireClear);
    live.install_session_admission(mode, None, policy);
    live.session.install_network_timing(20);
    live.session
        .install_shared_house_sources(vec![0, 0], Some(0));
    live.install_control_grants(vec![ControlGrant {
        source: 0,
        house: 1,
        level: ControlGrantLevel::FullControl,
    }])
    .unwrap();
    live.intern_rule_type_ids(rules);
    live.resolve_type_handles(rules);
    let template = flat_terrain(16, 16);
    live.install_resolved_terrain_for_new_map(flat_terrain(16, 16));
    live.overlay_grid = Some(OverlayGrid::new(16, 16));
    let bounds = PlayfieldBounds {
        base: 8,
        off_fc: -1_000,
        off_100: -1_000,
        off_104: 2_000,
        off_108: 2_000,
    };
    live.set_playfield_bounds_for_test(Some(bounds));
    live.set_playfield_size_height_for_test(Some(8));
    live.restore_map_authority_after_snapshot_load(rules, rules.overlay_registry(), None)
        .unwrap();
    let terrain = live.resolved_terrain.as_ref().unwrap();
    let zones = live.zone_grid.as_mut().unwrap();
    let before_slots = zones
        .hierarchy_for(MovementZone::Normal)
        .unwrap()
        .level(1)
        .unwrap()
        .record_slot_count();
    crate::sim::pathfinding::zone_incremental::repair_zone_hierarchy_around_cell(
        zones,
        (5, 5),
        Some(bounds),
        terrain,
        &[],
    );
    let after_slots = zones
        .hierarchy_for(MovementZone::Normal)
        .unwrap()
        .level(1)
        .unwrap()
        .record_slot_count();
    assert!(
        after_slots > before_slots,
        "live topology repair retains ordered history"
    );
    let land = template
        .capture_user_saved_template(terrain)
        .unwrap()
        .user_load_land_values()
        .unwrap();

    live.substrate
        .base_reservations
        .reserve(Some(terrain), -1, -1, 0);
    let dummy = live.effective_shared_cell_dummy();
    dummy.stamp_coord(-2, 7);
    dummy.set_level_slope(-7, 11);
    dummy.write_native_anchor(Some(NativeCellIdentity::Dummy));
    dummy.adjust_neighbor_count(true);
    live.substrate.raw_cell_occupation.write_occupant(
        RawCellKey::Dummy,
        MovementLayer::Ground,
        1 << 3,
        Some(delegated),
        true,
    );
    live.radiation.apply_detonation(
        RadDetonation {
            rx: 6,
            ry: 6,
            rad_level: 20,
            spread: 1,
            rad_type: None,
            firing_house: None,
            invoker: None,
        },
        live.session.binary_frame(),
        "TEMPERATE",
        rules,
        Some(terrain),
        &mut live.lighting_sources,
    );
    assert_ne!(live.substrate.base_reservations.dummy_mask(), 0);
    let mut outgoing = Simulation::new();
    outgoing.substrate.raw_cell_occupation.write_occupant(
        RawCellKey::Dummy,
        MovementLayer::Bridge,
        1 << 4,
        None,
        true,
    );

    let bytes = GameSnapshot::save_validated(&live, 1, 2, "custom map history", 0);
    let snapshot = GameSnapshot::load_validated(&bytes, 1, 2, live.session.map_name()).unwrap();
    let (restored, _) = snapshot
        .restore_player_world(
            SnapshotProcessState::Live(&outgoing),
            rules,
            template,
            Default::default(),
            PlayerSaveCells::Saved {
                land,
                prepare_assets: &mut |_, _, _| Ok(()),
            },
        )
        .unwrap();
    assert_eq!(
        restored.effective_shared_cell_dummy().state_words(),
        dummy.state_words()
    );
    assert_eq!(
        restored.substrate.raw_cell_occupation.dummy_for_hash(),
        live.substrate.raw_cell_occupation.dummy_for_hash()
    );
    assert_zone_grids_equivalent(
        restored.zone_grid.as_ref().unwrap(),
        live.zone_grid.as_ref().unwrap(),
    );
    assert_eq!(restored.state_hash(), live.state_hash());
}

#[test]
fn exact_network_mode_timing_and_observer_identity_survive_snapshot() {
    use crate::sim::scenario_session::ScenarioGameMode;
    use crate::skirmish_modes::MpModeClass;
    for (mode, class, cooperative) in [
        (ScenarioGameMode::Lan, MpModeClass::Battle, false),
        (ScenarioGameMode::Wol, MpModeClass::Cooperative, true),
    ] {
        // Native Scenario load reseeds the RNG to zero. Isolate the
        // persisted session fields from that intentional load-time change.
        let mut sim = Simulation::with_seed(0);
        let owner = sim.intern(if cooperative { "Player" } else { "Observer" });
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, true, 0, 1,
            ));
        sim.session.set_current_house_for_test(Some(owner));
        sim.session.set_game_mode(mode);
        sim.session.install_network_timing(73);
        let observers = if cooperative { vec![] } else { vec![owner] };
        sim.session
            .install_multiplayer_house_identity(class, observers.clone());
        let mut header = crate::map::preview::ScenarioStartHeader::default();
        header.read_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[Header]\nNumCoopHumanStartSpots=2\n",
        ));
        sim.session.install_start_header(header);
        let bytes = GameSnapshot::save(&sim, 0, 0, "network-mode", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(restored.session.game_mode(), mode);
        assert!(restored.session.execution_topology().is_lockstep());
        assert_eq!(restored.session.start_header().coop_human_start_spots(), 2);
        assert_eq!(restored.session.network_max_ahead_frames(), Some(73));
        assert_eq!(restored.session.observer_houses(), observers.as_slice());
        assert_eq!(restored.session.multiplayer_mode_class(), Some(class));
        assert_eq!(
            restored.session.rule_policy().shared_allied_victory(),
            cooperative,
        );
        assert_eq!(
            restored.session.rule_policy().skips_network_score_dialog(),
            cooperative,
        );
        assert_eq!(restored.state_hash(), sim.state_hash());
    }
}

#[test]
fn network_local_view_identity_round_trips_outside_shared_hash() {
    use crate::sim::scenario_session::ScenarioGameMode;

    let mut sim = Simulation::with_seed(0);
    let owner = sim.intern("SharedPlayer");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 1,
        ));
    sim.session.install_current_house(Some(owner));
    sim.session.set_game_mode(ScenarioGameMode::Lan);
    sim.session.install_network_timing(73);
    sim.session
        .install_network_command_timing(0, std::num::NonZeroU8::MIN, [0, 1]);
    sim.session
        .install_multiplayer_house_identity(crate::skirmish_modes::MpModeClass::Battle, vec![]);
    sim.session
        .install_shared_house_sources(vec![0, 0], Some(0));
    let shared_hash = sim.state_hash();

    sim.session
        .install_shared_house_sources(vec![0, 0], Some(1));
    sim.install_watcher();
    assert!(sim.observe());
    assert_eq!(sim.state_hash(), shared_hash);

    let bytes = GameSnapshot::save(&sim, 0, 0, "network-local-view", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    let view = restored.session.local_view_identity();
    assert_eq!(view.current_house, Some(owner));
    assert_eq!(view.shared_house_source, Some(1));
    assert!(view.watching);
    assert!(view.observer_sidebar);
    assert_eq!(restored.state_hash(), shared_hash);
}

#[test]
fn current_house_is_saved_client_input_with_validated_reference() {
    let mut sim = Simulation::with_seed(0);
    let mut owners = Vec::new();
    for name in ["First", "Second"] {
        let owner = sim.intern(name);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, true, 0, 1,
            ));
        owners.push(owner);
    }
    let shared_hash = sim.state_hash();
    let rng = sim.rng_state();
    let mut saved_identities = Vec::new();
    for owner in owners.iter().copied() {
        sim.session.set_current_house_for_test(Some(owner));
        assert_eq!(sim.state_hash(), shared_hash);
        assert_eq!(sim.rng_state(), rng);
        let bytes = GameSnapshot::save(&sim, 0, 0, "current-house", 0);
        let mut restored = GameSnapshot::load(&bytes).expect("current schema").sim;
        restored
            .restore_after_snapshot_load()
            .expect("valid House input");
        assert_eq!(restored.session.current_house(), Some(owner));
        assert_eq!(restored.state_hash(), shared_hash);
        saved_identities.push(restored.session.current_house());
        assert_eq!(restored.session.current_house(), Some(owner));
        assert_eq!(restored.state_hash(), shared_hash);
        assert_eq!(restored.rng_state(), rng);
    }
    assert_ne!(saved_identities[0], saved_identities[1]);

    let missing = sim.intern("Missing");
    sim.session.set_current_house_for_test(Some(missing));
    assert_eq!(
        sim.restore_after_snapshot_load(),
        Err(SnapshotRestoreError::InvalidCurrentHouse { owner: missing })
    );
}

#[test]
fn scenario_side_snapshot_matches_original_signed_retention() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/eva_selection.json",
    ))
    .unwrap();
    // Native Scenario Load reseeds its saved RNG to0. Isolate Side
    // persistence from that independently established load behavior.
    let mut sim = Simulation::with_seed(0);
    let owner = sim.intern("DifferentHouseSide");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 2, None, true, 0, 1,
        ));
    sim.session.install_current_house(Some(owner));
    let hash = sim.state_hash();
    let rng = sim.rng_state();
    for row in corpus["scenario_round_trips"].as_array().unwrap() {
        sim.session
            .install_scenario_side(row["input_side"].as_i64().unwrap() as i32);
        assert_eq!(sim.state_hash(), hash);
        assert_eq!(sim.rng_state(), rng);
        let bytes = GameSnapshot::save(&sim, 0, 0, "scenario-side", 0);
        sim.session
            .install_scenario_side(row["overwritten_before_load"].as_i64().unwrap() as i32);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(
            restored.session.scenario_side(),
            row["restored_side"].as_i64().unwrap() as i32
        );
        assert_eq!(restored.session.current_house(), Some(owner));
        assert_eq!(restored.houses[&owner].side_index(), 2);
        assert_eq!(
            restored.state_hash(),
            hash,
            "client presentation input is not shared CRC state"
        );
        assert_eq!(restored.rng_state(), rng);
    }
}

#[test]
fn validated_load_requires_exact_content_and_session_metadata() {
    use crate::sim::command::{Command, CommandEnvelope};

    let mut sim = Simulation::new();
    let owner = sim.interner.intern("AMERICANS");
    sim.session.set_map_name_for_test("MAP01.MAP".to_string());
    sim.session.set_tick_for_test(0x1_0000_0007);
    sim.session.set_binary_frame_for_test(u32::MAX - 2);
    sim.session.set_total_sim_ms_for_test(12_345);
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, false, 0, 10,
        ));
    sim.queue_command(CommandEnvelope::new(
        owner,
        sim.session.tick() + 3,
        Command::Stop { entity_id: 71 },
    ));
    sim.scenario_rng.next_u32();
    sim.main_rng.next_u32();
    sim.mapgen_rng.next_u32();
    let process_default = crate::sim::rng::SimRng::new(0).logical_state();
    assert_ne!(sim.rng_state().scenario, process_default);

    let bytes = GameSnapshot::save_validated(&sim, 0x11, 0x22, "Campaign foothold", 0x33);
    let restored =
        GameSnapshot::load_validated(&bytes, 0x11, 0x22, "MAP01.MAP").expect("exact metadata");
    assert_eq!(restored.tick, sim.session.tick());
    assert_eq!(restored.map_name, sim.session.map_name());
    assert_eq!(restored.sim.session.binary_frame(), u32::MAX - 2);
    assert_eq!(restored.sim.session.total_sim_ms(), 12_345);
    assert_eq!(restored.sim.houses.order(), vec![owner]);
    assert_eq!(
        restored.sim.pending_command_snapshot(),
        sim.pending_command_snapshot()
    );
    assert_eq!(
        restored.sim.scenario_rng.logical_state(),
        process_default,
        "native post-read Scenario reinitialization must reset the saved cursor"
    );
    assert_eq!(restored.sim.main_rng.logical_state(), process_default);
    assert_eq!(restored.sim.mapgen_rng.logical_state(), process_default);

    assert!(matches!(
        GameSnapshot::load_validated(&bytes, 0x12, 0x22, "MAP01.MAP"),
        Err(SnapshotError::MapMismatch { .. })
    ));
    assert!(matches!(
        GameSnapshot::load_validated(&bytes, 0x11, 0x23, "MAP01.MAP"),
        Err(SnapshotError::RulesMismatch { .. })
    ));
    assert!(matches!(
        GameSnapshot::load_validated(&bytes, 0x11, 0x22, "MAP02.MAP"),
        Err(SnapshotError::MapNameMismatch { .. })
    ));

    let mut inconsistent = GameSnapshot::load_unchecked(&bytes).expect("current snapshot");
    inconsistent.tick = inconsistent.tick.wrapping_add(1);
    let inconsistent_bytes = save_codec::serialize(&inconsistent).expect("corrupt tick metadata");
    assert!(matches!(
        GameSnapshot::load_validated(&inconsistent_bytes, 0x11, 0x22, "MAP01.MAP"),
        Err(SnapshotError::TickMetadataMismatch { .. })
    ));

    inconsistent.tick = inconsistent.sim.session.tick();
    inconsistent
        .sim
        .session
        .set_map_name_for_test("OTHER.MAP".to_string());
    let inconsistent_bytes = save_codec::serialize(&inconsistent).expect("corrupt map metadata");
    assert!(matches!(
        GameSnapshot::load_validated(&inconsistent_bytes, 0x11, 0x22, "MAP01.MAP"),
        Err(SnapshotError::MapNameMetadataMismatch { .. })
    ));
}

/// Every saved type traces to one description, and its id can never equal
/// a counter from the hand-kept scheme.
#[test]
fn snapshot_layout_describes_every_saved_type() {
    let description =
        save_codec::layout::describe::<GameSnapshot>().unwrap_or_else(|error| panic!("{error}"));
    assert!(description.starts_with("GameSnapshot\n"));
    assert!(description.contains("\nSimulation struct{"));
    assert_ne!(super::snapshot_layout() & 0x8000_0000, 0);
    assert_eq!(super::snapshot_layout(), super::snapshot_layout());
}
