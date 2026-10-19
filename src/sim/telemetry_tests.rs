//! Tests for the dashboard telemetry snapshot.
//!
//! Snapshot bounds, retained-owner observations and simulation invariance.

use crate::map::entities::EntityCategory;
use crate::rules::art_data::ArtRegistry;
use crate::rules::infantry_sequence::parse_infantry_sequence_registry;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::Health;
use crate::sim::house_state::HouseState;
use crate::sim::telemetry::{
    CategoryCounts, HousePopulation, TELEMETRY_SCHEMA_VERSION, TelemetrySnapshot,
};
use crate::sim::world::Simulation;

/// Minimal one-vehicle rules fixture, mirroring the animation tests' spawn
/// setup: a voxel Unit with the fields the constructor reads.
fn vehicle_rules() -> RuleSet {
    let rules_ini = IniFile::from_str(
        "[VehicleTypes]\n0=ACTOR\n[ACTOR]\nImage=IMAGE\nStrength=100\nWalkRate=3\nIdleRate=4\n",
    );
    let art = IniFile::from_str("[IMAGE]\nVoxel=yes\nFacings=8\nWalkFrames=6\nStandingFrames=1\n");
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    rules.bind_animation_sequences(&parse_infantry_sequence_registry(&art));
    rules
}

fn house_row<'a>(snapshot: &'a TelemetrySnapshot, name: &str) -> &'a HousePopulation {
    snapshot
        .houses
        .iter()
        .find(|house| house.name == name)
        .unwrap_or_else(|| panic!("no house row for {name} in {snapshot:?}"))
}

fn house_index(snapshot: &TelemetrySnapshot, name: &str) -> u16 {
    snapshot
        .houses
        .iter()
        .position(|house| house.name == name)
        .unwrap_or_else(|| panic!("no house row for {name} in {snapshot:?}")) as u16
}

#[test]
fn category_counts_add_covers_every_category() {
    let mut counts = CategoryCounts::default();
    counts.add(EntityCategory::Unit);
    counts.add(EntityCategory::Infantry);
    counts.add(EntityCategory::Aircraft);
    counts.add(EntityCategory::Structure);
    assert_eq!(
        (
            counts.unit,
            counts.infantry,
            counts.aircraft,
            counts.structure
        ),
        (1, 1, 1, 1)
    );
    assert_eq!(counts.total(), 4);
}

#[test]
fn fresh_simulation_snapshot_is_empty() {
    let sim = Simulation::new();
    let snapshot = sim.telemetry_snapshot();
    assert_eq!(snapshot.version, TELEMETRY_SCHEMA_VERSION);
    assert_eq!(snapshot.tick, 0);
    assert_eq!(snapshot.entities_total, 0);
    assert!(snapshot.houses.is_empty());
    assert!(snapshot.spots.is_empty());
    assert_eq!(snapshot.timing, None);
    assert_eq!(
        snapshot.path_batch, None,
        "a simulation that searches serially runs no pass to report"
    );
    assert_eq!(snapshot.map_width, sim.session.map_width);
    assert_eq!(snapshot.map_height, sim.session.map_height);
}

#[test]
fn snapshot_tallies_entities_per_house() {
    let rules = vehicle_rules();
    let mut sim = Simulation::with_seed(31);
    let americans = sim.interner.intern("Americans");
    let soviets = sim.interner.intern("Soviets");
    sim.houses.insert(
        americans,
        HouseState::new(americans, 0, None, true, 1000, 10),
    );
    sim.houses
        .insert(soviets, HouseState::new(soviets, 1, None, false, 1000, 10));

    for _ in 0..2 {
        sim.spawn_object_limbo_at_height("ACTOR", "Americans", 3, 3, 0, 0, &rules)
            .expect("vehicle spawn");
    }
    sim.spawn_object_limbo_at_height("ACTOR", "Soviets", 40, 40, 0, 0, &rules)
        .expect("vehicle spawn");
    // An owner with no house row: counted in the total, absent from the
    // house rows — the anomaly stays visible instead of being hidden.
    sim.spawn_object_limbo_at_height("ACTOR", "French", 60, 60, 0, 0, &rules)
        .expect("vehicle spawn");

    let snapshot = sim.telemetry_snapshot();
    assert_eq!(snapshot.entities_total, 4);
    assert_eq!(snapshot.houses.len(), 2);

    let americans_row = house_row(&snapshot, "Americans");
    assert_eq!(americans_row.counts.unit, 2);
    assert_eq!(americans_row.counts.total(), 2);
    assert_eq!(americans_row.side_index, 0);
    assert!(americans_row.is_human);

    let soviets_row = house_row(&snapshot, "Soviets");
    assert_eq!(soviets_row.counts.unit, 1);
    assert_eq!(soviets_row.side_index, 1);
    assert!(!soviets_row.is_human);

    let housed: u32 = snapshot.houses.iter().map(|h| h.counts.total()).sum();
    assert_eq!(housed, 3, "house rows miss the orphan owner by design");
}

#[test]
fn house_rows_carry_economy_and_roster() {
    let rules = vehicle_rules();
    let mut sim = Simulation::with_seed(37);
    let americans = sim.interner.intern("Americans");
    sim.houses.insert(
        americans,
        HouseState::new(americans, 0, None, true, 1000, 10),
    );
    for _ in 0..3 {
        sim.spawn_object_limbo_at_height("ACTOR", "Americans", 3, 3, 0, 0, &rules)
            .expect("vehicle spawn");
    }
    if let Some(house) = sim.houses.get_mut(&americans) {
        house.economy.add_credits(500);
        house.economy.add_score(42);
    }

    let snapshot = sim.telemetry_snapshot();
    let row = house_row(&snapshot, "Americans");
    assert_eq!(row.credits, 1500, "seeded 1000 plus the 500 deposit");
    assert_eq!(row.score, 42);
    assert_eq!(row.spent, 0);
    assert!(!row.defeated);
    assert_eq!(row.roster.len(), 1, "one type in this fixture");
    assert_eq!(row.roster[0].type_name, "ACTOR");
    assert_eq!(row.roster[0].category, EntityCategory::Unit);
    assert_eq!(row.roster[0].count, 3);
}

#[test]
fn production_rows_report_the_active_build_its_queue_and_ready_stock() {
    use crate::sim::production::PRODUCTION_STEPS;
    use crate::sim::production::ProductionCategory;

    let rules = vehicle_rules();
    let mut sim = Simulation::with_seed(53);
    let americans = sim.interner.intern("Americans");
    let soviets = sim.interner.intern("Soviets");
    sim.houses.insert(
        americans,
        HouseState::new(americans, 0, None, true, 1000, 10),
    );
    sim.houses
        .insert(soviets, HouseState::new(soviets, 1, None, false, 1000, 10));
    sim.spawn_object_limbo_at_height("ACTOR", "Americans", 3, 3, 0, 0, &rules)
        .expect("vehicle spawn");

    let actor = sim.interner.intern("ACTOR");
    // Two factories for one house: the projection groups by owner, not by
    // category, so both must appear rather than one shadowing the other.
    sim.production.factories.test_enqueue_kernel(
        americans,
        ProductionCategory::Infantry,
        actor,
        0,
        100,
    );
    sim.production.factories.test_enqueue_kernel(
        americans,
        ProductionCategory::Vehicle,
        actor,
        1,
        100,
    );
    // A different house's factory must land on that house's row.
    sim.production.factories.test_enqueue_kernel(
        soviets,
        ProductionCategory::Infantry,
        actor,
        2,
        100,
    );

    let snapshot = sim.telemetry_snapshot();
    let row = house_row(&snapshot, "Americans");
    assert_eq!(
        row.production.len(),
        2,
        "both of this house's factories are reported"
    );
    let infantry = row
        .production
        .iter()
        .find(|line| line.category == "Infantry")
        .expect("the infantry line");
    assert_eq!(infantry.active_type.as_deref(), Some("ACTOR"));
    assert!(
        infantry.progress < PRODUCTION_STEPS,
        "a fresh enqueue has not completed"
    );

    // Finishing the build moves the type out of `active_type` and into the
    // ready-to-place stock: that is the state a player can act on, and the
    // panel has to show it as such rather than as a full progress bar.
    sim.production
        .factories
        .test_factory_mut(americans, ProductionCategory::Infantry)
        .unwrap()
        .progress = PRODUCTION_STEPS;
    let snapshot = sim.telemetry_snapshot();
    let infantry = house_row(&snapshot, "Americans")
        .production
        .iter()
        .find(|line| line.category == "Infantry")
        .cloned()
        .expect("the infantry line");
    assert_eq!(infantry.active_type, None);
    assert_eq!(infantry.ready_to_place, vec!["ACTOR".to_string()]);

    assert_eq!(
        house_row(&snapshot, "Soviets").production.len(),
        1,
        "each house's row carries only its own factories"
    );
}

#[test]
fn a_house_with_no_factory_reports_no_production() {
    let mut sim = Simulation::with_seed(59);
    let americans = sim.interner.intern("Americans");
    sim.houses.insert(
        americans,
        HouseState::new(americans, 0, None, true, 1000, 10),
    );
    let snapshot = sim.telemetry_snapshot();
    let row = house_row(&snapshot, "Americans");
    assert!(row.production.is_empty());
    assert!(row.squads.is_empty());
    assert!(row.base_plan.is_empty());
}

#[test]
fn spots_carry_world_positions_and_owner_indices() {
    // The diagnostic insert keeps its requested cell (unlike the limbo
    // spawn, whose native CoordStruct default is (0,0) off-map).
    let mut sim = Simulation::with_seed(31);
    let americans = sim.interner.intern("Americans");
    sim.houses.insert(
        americans,
        HouseState::new(americans, 0, None, true, 1000, 10),
    );
    let french = sim.interner.intern("French");
    let type_ref = sim.interner.intern("ACTOR");
    for (owner, cell) in [(americans, 3u16), (french, 60u16)] {
        sim.insert_synthetic_techno_for_diagnostics(
            cell,
            cell,
            0,
            0,
            owner,
            Health::new(1000),
            type_ref,
            EntityCategory::Unit,
            0,
            6,
            false,
        );
    }

    let snapshot = sim.telemetry_snapshot();
    assert_eq!(snapshot.spots.len(), 2);
    let americans_index = house_index(&snapshot, "Americans");

    let americans_spot = snapshot
        .spots
        .iter()
        .find(|spot| spot.owner_index == americans_index)
        .expect("the American's spot");
    let orphan_spot = snapshot
        .spots
        .iter()
        .find(|spot| spot.owner_index == u16::MAX)
        .expect("the orphan owner's spot");

    for (spot, cell) in [(americans_spot, 3.0), (orphan_spot, 60.0)] {
        assert_eq!(spot.category, EntityCategory::Unit);
        assert_eq!(spot.z, 0);
        assert!(
            (cell..cell + 1.0).contains(&spot.x),
            "x {} is inside cell {} with its sub-cell offset",
            spot.x,
            cell
        );
        assert!(
            (cell..cell + 1.0).contains(&spot.y),
            "y {} is inside cell {} with its sub-cell offset",
            spot.y,
            cell
        );
    }
}

#[test]
fn snapshot_round_trips_through_save_codec() {
    let rules = vehicle_rules();
    let mut sim = Simulation::with_seed(31);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 1000, 10));
    sim.spawn_object_limbo_at_height("ACTOR", "Americans", 3, 3, 0, 0, &rules)
        .expect("vehicle spawn");

    let snapshot = sim.telemetry_snapshot();
    let bytes = crate::util::save_codec::serialize(&snapshot).expect("serialize");
    let restored: TelemetrySnapshot =
        crate::util::save_codec::deserialize(&bytes).expect("deserialize");
    assert_eq!(snapshot, restored);
}

#[test]
fn houses_carry_their_difficulty() {
    use crate::sim::house_state::HouseDifficulty;

    let mut sim = Simulation::with_seed(41);
    let tough = sim.interner.intern("Computer1");
    let mut tough_house = HouseState::new(tough, 1, None, false, 1000, 10);
    tough_house.difficulty = HouseDifficulty::Hard;
    sim.houses.insert(tough, tough_house);

    let easy = sim.interner.intern("Computer2");
    let mut easy_house = HouseState::new(easy, 1, None, false, 1000, 10);
    easy_house.difficulty = HouseDifficulty::Easy;
    sim.houses.insert(easy, easy_house);

    let snapshot = sim.telemetry_snapshot();
    assert_eq!(snapshot.houses[0].difficulty, "hard");
    assert_eq!(snapshot.houses[1].difficulty, "easy");
    // The app fills the side's [Sides] spelling from the loaded rules.
    assert_eq!(snapshot.houses[0].side_name, None);
}

#[test]
fn repeated_captures_are_equal() {
    let rules = vehicle_rules();
    let mut sim = Simulation::with_seed(31);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 1000, 10));
    sim.spawn_object_limbo_at_height("ACTOR", "Americans", 3, 3, 0, 0, &rules)
        .expect("vehicle spawn");

    assert_eq!(sim.telemetry_snapshot(), sim.telemetry_snapshot());
}

#[test]
fn snapshot_and_profiling_switches_leave_hash_and_saved_bytes_unchanged() {
    let rules = vehicle_rules();
    let mut sim = Simulation::with_seed(71);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 1000, 10));
    sim.spawn_object_limbo_at_height("ACTOR", "Americans", 3, 3, 0, 0, &rules)
        .unwrap();
    let hash = sim.state_hash();
    let saved = crate::sim::snapshot::GameSnapshot::save_validated(&sim, 0, 0, "telemetry", 0);
    sim.set_frame_profiling(true);
    for _ in 0..3 {
        let snapshot = sim.telemetry_snapshot();
        assert_eq!(snapshot.entities_total, 1);
        assert!(snapshot.profile.is_none());
    }
    assert_eq!(sim.state_hash(), hash);
    assert_eq!(
        crate::sim::snapshot::GameSnapshot::save_validated(&sim, 0, 0, "telemetry", 0),
        saved
    );
    sim.set_frame_profiling(false);
    assert_eq!(sim.state_hash(), hash);
    assert_eq!(
        crate::sim::snapshot::GameSnapshot::save_validated(&sim, 0, 0, "telemetry", 0),
        saved
    );
}

#[test]
fn profiled_frames_preserve_gameplay_and_reset_only_at_observer_transitions() {
    let mut observed = Simulation::with_seed(79);
    let mut control = Simulation::with_seed(79);
    observed.set_frame_profiling(true);
    for _ in 0..3 {
        let actual = observed.advance_tick(&[], None, None, None, 67);
        let expected = control.advance_tick(&[], None, None, None, 67);
        assert_eq!(actual.tick, expected.tick);
        assert_eq!(actual.frame_committed, expected.frame_committed);
        assert_eq!(observed.state_hash(), control.state_hash());
        assert_eq!(
            crate::util::save_codec::serialize(&observed).unwrap(),
            crate::util::save_codec::serialize(&control).unwrap()
        );
    }
    assert_eq!(control.frame_profile().frames(), 0);
    let report = observed.frame_profile().report().unwrap();
    assert_eq!(report.frames, 3);
    assert!((report.phases_ms.iter().sum::<f64>() - report.mean_frame_ms).abs() < 1e-6);
    observed.set_frame_profiling(true);
    assert_eq!(observed.frame_profile().frames(), 3);
    let mut restored: Simulation = crate::util::save_codec::deserialize(
        &crate::util::save_codec::serialize(&observed).unwrap(),
    )
    .unwrap();
    assert_eq!(restored.frame_profile().frames(), 0);
    restored.advance_tick(&[], None, None, None, 67);
    assert_eq!(
        restored.frame_profile().frames(),
        0,
        "load defaults to disabled"
    );
    observed.set_frame_profiling(false);
    observed.advance_tick(&[], None, None, None, 67);
    assert_eq!(observed.frame_profile().frames(), 0);
}

#[test]
fn roster_keeps_identical_names_in_distinct_categories_and_reads_team_flags() {
    let mut sim = Simulation::with_seed(83);
    let owner = sim.interner.intern("Americans");
    let kind = sim.interner.intern("SharedName");
    let script = sim.interner.intern("Script");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, false, 0, 10));
    let mut members = Vec::new();
    for category in [EntityCategory::Unit, EntityCategory::Infantry] {
        members.push(sim.insert_synthetic_techno_for_diagnostics(
            3,
            3,
            0,
            0,
            owner,
            Health::new(100),
            kind,
            category,
            0,
            1,
            false,
        ));
    }
    sim.team_script_vm.create_team(owner, script, members, 0);
    let snapshot = sim.telemetry_snapshot();
    let house = house_row(&snapshot, "Americans");
    assert_eq!(house.roster.len(), 2);
    assert_eq!(house.counts.unit, 1);
    assert_eq!(house.counts.infantry, 1);
    assert_eq!(house.squads[0].composition.len(), 2);
    assert_eq!(house.squads[0].members, 2);
    assert!(house.squads[0].formed, "read the actual Team owner");
    assert!(!house.squads[0].under_strength);
    assert!(!house.squads[0].suspended);
}

#[test]
fn snapshot_bounds_long_names_house_rows_and_production_queues() {
    use crate::sim::production::ProductionCategory;
    use crate::sim::telemetry::{
        TELEMETRY_MAX_HOUSES, TELEMETRY_MAX_NAME_BYTES, TELEMETRY_MAX_QUEUE,
    };
    let mut sim = Simulation::with_seed(89);
    let kind = sim.interner.intern(&"é".repeat(200));
    let mut first = None;
    for index in 0..TELEMETRY_MAX_HOUSES + 3 {
        let owner = sim.interner.intern(&format!("House{index}"));
        first.get_or_insert(owner);
        sim.houses
            .insert(owner, HouseState::new(owner, 0, None, false, 0, 10));
    }
    let owner = first.unwrap();
    sim.production
        .factories
        .test_enqueue_kernel(owner, ProductionCategory::Vehicle, kind, 0, 1);
    for index in 0..TELEMETRY_MAX_QUEUE + 4 {
        sim.production.factories.test_enqueue_kernel(
            owner,
            ProductionCategory::Vehicle,
            kind,
            index as u64 + 1,
            1,
        );
    }
    let snapshot = sim.telemetry_snapshot();
    assert_eq!(snapshot.houses.len(), TELEMETRY_MAX_HOUSES);
    let production = &snapshot.houses[0].production[0];
    assert_eq!(production.queue.len(), TELEMETRY_MAX_QUEUE);
    assert_eq!(
        production.active_type.as_ref().unwrap().len(),
        TELEMETRY_MAX_NAME_BYTES
    );
    assert!(
        production
            .queue
            .iter()
            .all(|name| name.len() <= TELEMETRY_MAX_NAME_BYTES)
    );
    let json = serde_json::to_value(&snapshot).unwrap();
    assert!(json.get("cheats").is_none());
    assert_eq!(json["version"], TELEMETRY_SCHEMA_VERSION);
}

#[test]
fn capped_core_payload_leaves_room_in_the_eight_mib_transport_budget() {
    use crate::sim::telemetry::*;
    let name = "\0".repeat(TELEMETRY_MAX_NAME_BYTES);
    let count = TypeCount {
        type_name: name.clone(),
        category: EntityCategory::Structure,
        count: u32::MAX,
    };
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Owner");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, false, 0, 10));
    let mut snapshot = sim.telemetry_snapshot();
    let mut house = snapshot.houses[0].clone();
    house.name = name.clone();
    house.side_name = Some(name.clone());
    house.roster = vec![count.clone(); TELEMETRY_MAX_ROSTER];
    house.production = vec![
        ProductionLine {
            category: "Infantry".to_owned(),
            active_type: Some(name.clone()),
            progress: 54,
            on_hold: false,
            suspended: false,
            queue: vec![name.clone(); TELEMETRY_MAX_QUEUE],
            ready_to_place: vec![name.clone()],
        };
        TELEMETRY_MAX_PRODUCTION
    ];
    house.squads = vec![
        AiSquad {
            id: u64::MAX,
            team_type: Some(name.clone()),
            task_force: Some(name.clone()),
            members: u32::MAX,
            composition: vec![count; TELEMETRY_MAX_COMPOSITION],
            script_step: i32::MIN,
            formed: false,
            under_strength: true,
            suspended: false,
        };
        TELEMETRY_MAX_SQUADS
    ];
    house.base_plan = vec![
        BasePlan {
            x: u16::MAX,
            y: u16::MAX,
            building: Some(name),
            is_base: true
        };
        TELEMETRY_MAX_BASE_PLAN
    ];
    snapshot.houses = vec![house; TELEMETRY_MAX_HOUSES];
    snapshot.spots = vec![
        EntitySpot {
            x: 65535.996,
            y: 65535.996,
            z: u8::MAX,
            category: EntityCategory::Structure,
            owner_index: u16::MAX
        };
        TELEMETRY_MAX_SPOTS
    ];
    // Control bytes expand to six-byte JSON escapes: this also checks the
    // worst string-escaping case, leaving at least one MiB for app observations.
    assert!(serde_json::to_vec(&snapshot).unwrap().len() < 7 * 1024 * 1024);
}

#[test]
fn project_scale_capture_counts_thirty_houses_and_caps_twenty_thousand_spots() {
    use crate::sim::telemetry::TELEMETRY_MAX_SPOTS;
    let mut sim = Simulation::with_seed(97);
    let kind = sim.interner.intern("ACTOR");
    let owners: Vec<_> = (0..30)
        .map(|index| {
            let owner = sim.interner.intern(&format!("Player{index}"));
            sim.houses
                .insert(owner, HouseState::new(owner, 0, None, false, 1000, 10));
            owner
        })
        .collect();
    for index in 0..=TELEMETRY_MAX_SPOTS {
        sim.insert_synthetic_techno_for_diagnostics(
            (index % 100) as u16,
            (index / 100) as u16,
            0,
            0,
            owners[index % owners.len()],
            Health::new(100),
            kind,
            EntityCategory::Unit,
            0,
            1,
            false,
        );
    }
    let hash = sim.state_hash();
    let snapshot = sim.telemetry_snapshot();
    assert_eq!(snapshot.entities_total, (TELEMETRY_MAX_SPOTS + 1) as u32);
    assert_eq!(snapshot.spots.len(), TELEMETRY_MAX_SPOTS);
    assert_eq!(snapshot.houses.len(), 30);
    assert_eq!(
        snapshot
            .houses
            .iter()
            .map(|house| house.counts.total())
            .sum::<u32>(),
        snapshot.entities_total
    );
    assert!(snapshot.spots.iter().all(|spot| spot.owner_index < 30));
    assert!(serde_json::to_vec(&snapshot).unwrap().len() < 8 * 1024 * 1024);
    assert_eq!(sim.state_hash(), hash);
}

#[test]
fn debug_panel_round_trips_through_save_codec() {
    use crate::sim::telemetry::{
        DebugPanel, EntityDesc, LandRow, MinerDebug, OverlayFlags, PathStep, SelectedDebug,
    };
    let debug = DebugPanel {
        cursor_cell: Some((12, 34)),
        cursor_elevation: 1,
        grid: Some((120, 120)),
        cursor_walkable: Some(true),
        land_rows: vec![LandRow {
            speed_type: "Foot".to_string(),
            cost: 100,
        }],
        active_speed_type: "Track".to_string(),
        vxl_sprites: Some(42),
        entities_at_cursor: vec![EntityDesc {
            type_name: "E1".to_string(),
            category: "Inf".to_string(),
            owner: "Americans".to_string(),
            hp: 90,
            max_hp: 125,
        }],
        footprint: Some("GAWALL (2x1) @ (10,20) Americans".to_string()),
        selected: vec![SelectedDebug {
            stable_id: 7,
            type_name: "HARV".to_string(),
            cell: (30, 40),
            sub: (128, 200),
            moving: true,
            path_steps_left: 3,
            path_blocked: true,
            goal: Some((35, 42)),
            path_preview: vec![PathStep {
                cell: (31, 40),
                walkable: false,
                first: true,
            }],
            miner: Some(MinerDebug {
                kind: "Ore".to_string(),
                state: "Harvest".to_string(),
                cargo: 3,
                capacity: 7,
                stage: 5,
                rate: 1,
                harvesting: true,
                archived_ore: Some((50, 60)),
                teleport_active: false,
            }),
            #[cfg(feature = "debug-tools")]
            inspector_active: false,
            #[cfg(feature = "debug-tools")]
            events: Vec::new(),
        }],
        overlays: OverlayFlags {
            terrain_costs: true,
            heightmap: false,
            cell_grid: true,
            fog: true,
            foot_path: false,
            paused: false,
            #[cfg(feature = "debug-tools")]
            unit_inspector: false,
        },
    };
    let bytes = crate::util::save_codec::serialize(&debug).expect("serialize");
    let restored: DebugPanel = crate::util::save_codec::deserialize(&bytes).expect("deserialize");
    assert_eq!(debug, restored);
    let json = serde_json::to_value(&debug).unwrap();
    #[cfg(not(feature = "debug-tools"))]
    {
        assert!(json["selected"][0].get("inspector_active").is_none());
        assert!(json["selected"][0].get("events").is_none());
        assert!(json["overlays"].get("unit_inspector").is_none());
    }
    #[cfg(feature = "debug-tools")]
    {
        assert_eq!(json["selected"][0]["inspector_active"], false);
        assert!(json["selected"][0]["events"].as_array().unwrap().is_empty());
        assert_eq!(json["overlays"]["unit_inspector"], false);
    }
}

#[cfg(not(feature = "debug-tools"))]
#[test]
fn dashboard_only_payload_contains_no_trainer_or_inspector() {
    let value = serde_json::to_value(Simulation::new().telemetry_snapshot()).unwrap();
    assert!(value.get("debug_tools").is_none());
    assert!(value.get("cheats").is_none());
    let flags = serde_json::to_value(crate::sim::telemetry::OverlayFlags::default()).unwrap();
    assert!(flags.get("unit_inspector").is_none());
}
