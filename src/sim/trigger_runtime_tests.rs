use super::*;
use std::collections::HashMap;

use crate::map::actions::MapAction;
use crate::map::entities::EntityCategory;
use crate::map::events::MapEvent;
use crate::map::trigger_graph::build_trigger_graph;
use crate::map::triggers::{MapTrigger, TriggerDifficulty};
use crate::map::variable_names::{LocalVariable, LocalVariableMap};
use crate::sim::game_entity::GameEntity;
use crate::sim::projectile::{
    ProjectileCollisionPolicy, ProjectileCoord, ProjectilePayload, ProjectileSpawn,
    ProjectileTarget, TargetExpiryPolicy,
};
use crate::sim::replay::{ReplayHeader, ReplayLog, ReplayRunner};
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::{MasterFrameTestRung, Simulation, TickLane, TriggerInputs};
use std::collections::BTreeMap;

fn flat_trigger_playfield_terrain(
    width: u16,
    height: u16,
) -> crate::map::resolved_terrain::ResolvedTerrainGrid {
    use crate::map::resolved_terrain::{ResolvedTerrainCell, zone_class};
    use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};

    let prototype = ResolvedTerrainCell {
        rx: 0,
        ry: 0,
        source_tile_index: 0,
        source_sub_tile: 0,
        final_tile_index: 0,
        final_sub_tile: 0,
        is_wood_bridge_repair_tile: false,
        level: 0,
        filled_clear: false,
        tileset_index: Some(0),
        land_type: 0,
        yr_cell_land_type: 0,
        slope_type: 0,
        template_height: 0,
        height_in_pixels: 0,
        render_offset_x: 0,
        render_offset_y: 0,
        terrain_class: TerrainClass::Clear,
        speed_costs: SpeedCostProfile::default(),
        is_water: false,
        is_cliff_like: false,
        is_rough: false,
        is_road: false,
        accepts_smudge: false,
        allows_tiberium: false,
        variant: 0,
        has_ramp: false,
        canonical_ramp: None,
        ground_walk_blocked: false,
        terrain_object_blocks: false,
        terrain_object_occupation: None,
        overlay_blocks: false,
        overlay_zone_type: None,
        outside_playfield: false,
        zone_type: zone_class::GROUND,
        base_ground_walk_blocked: false,
        base_build_blocked: false,
        base_land_type: 0,
        base_yr_cell_land_type: 0,
        base_terrain_class: TerrainClass::Clear,
        base_speed_costs: SpeedCostProfile::default(),
        build_blocked: false,
        has_bridge_deck: false,
        bridge_walkable: false,
        bridge_transition: false,
        bridge_deck_level: 0,
        bridge_layer: None,
        bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
        tube_index: None,
        radar_left: [0; 3],
        radar_right: [0; 3],
        has_damaged_data: false,
        bridgehead_anchor_class_at_load: None,
    };
    let cells = (0..height)
        .flat_map(|ry| {
            let prototype = prototype.clone();
            (0..width).map(move |rx| {
                let mut cell = prototype.clone();
                cell.rx = rx;
                cell.ry = ry;
                cell
            })
        })
        .collect();
    crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(width, height, cells)
}

fn make_trigger(
    id: &str,
    linked_trigger_id: Option<&str>,
    name: &str,
    enabled: bool,
    repeating: bool,
) -> MapTrigger {
    MapTrigger {
        id: id.to_string(),
        fields: vec![
            "Neutral".to_string(),
            linked_trigger_id.unwrap_or("<none>").to_string(),
            name.to_string(),
            if enabled { "0" } else { "1" }.to_string(),
            "1".to_string(),
            "1".to_string(),
            "1".to_string(),
            if repeating { "2" } else { "0" }.to_string(),
        ],
        owner: Some("Neutral".to_string()),
        linked_trigger_id: linked_trigger_id.map(|value| value.to_ascii_uppercase()),
        name: Some(name.to_string()),
        enabled,
        difficulty: TriggerDifficulty {
            easy: true,
            medium: true,
            hard: true,
        },
        repeating,
    }
}

fn spawn_type(sim: &mut Simulation, type_id: &str) -> u64 {
    let sid = sim.allocate_stable_id();
    let owner_id = sim.interner.intern("Neutral");
    let type_id_interned = sim.interner.intern(type_id);
    let ge = GameEntity::new_at_frame_zero_for_test(
        sid,
        0,
        0,
        0,
        0,
        owner_id,
        crate::sim::components::Health { current: 100 },
        type_id_interned,
        EntityCategory::Unit,
        0,
        5,
        false,
    );
    sim.substrate.entities.insert(ge);
    sid
}

#[test]
fn parsed_map_trigger_flags_gate_production_frames_and_survive_restore() {
    // Use actual map text, not hand-built MapTrigger booleans. The enabled
    // and difficulty expectations are established by trigger_type_flags.json.
    let map = crate::map::map_file::MapFile::from_bytes(
        b"[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
          [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
          [Triggers]\n\
          ACTIVE=Neutral,<none>,Active,0,1,1,1,0\n\
          DISABLED=Neutral,<none>,Disabled,1,1,1,1,0\n\
          NO_MEDIUM=Neutral,<none>,Other difficulty,0,1,0,1,0\n\
          MISSING=Neutral,<none>,Missing flags\n\
          SHIFTED=Neutral,<none>,Empty token,,0,0,-2,0,0\n\
          [Events]\n\
          ACTIVE=1,47,0,0\nDISABLED=1,47,0,0\nNO_MEDIUM=1,47,0,0\n\
          MISSING=1,47,0,0\nSHIFTED=1,47,0,0\n\
          [Actions]\n\
          ACTIVE=1,112,0,0,0,0,0,0,A\n\
          DISABLED=1,112,0,0,0,0,0,0,B\n\
          NO_MEDIUM=1,112,0,0,0,0,0,0,C\n\
          MISSING=1,112,0,0,0,0,0,0,D\n\
          SHIFTED=1,112,0,0,0,0,0,0,E\n",
    )
    .expect("map loader accepts trigger fixture");
    let mut original = Simulation::new();
    // Same initialization as app/loading/init.rs.
    original.trigger_runtime = TriggerRuntime::from_map(&map.triggers, &map.local_variables);
    assert_eq!(
        original.trigger_runtime.disabled_triggers,
        ["DISABLED", "MISSING", "NO_MEDIUM"]
            .into_iter()
            .map(str::to_string)
            .collect()
    );
    let bytes = GameSnapshot::save(&original, 0, 0, "trigger_flags", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    for sim in [&mut original, &mut restored] {
        let frame = sim
            .advance_master_frame(
                &[],
                None,
                &BTreeMap::new(),
                None,
                None,
                67,
                TickLane::Ordinary,
                Some(TriggerInputs {
                    graph: &map.trigger_graph,
                    triggers: &map.triggers,
                    events: &map.events,
                    actions: &map.actions,
                    waypoints: &map.waypoints,
                    rules: None,
                }),
            )
            .expect("production frame completes");
        assert!(frame.frame_committed);
        assert_eq!(
            sim.drain_trigger_effects(),
            vec![
                TriggerEffect::CenterCameraAtWaypoint {
                    waypoint: 0,
                    immediate: true,
                },
                TriggerEffect::CenterCameraAtWaypoint {
                    waypoint: 4,
                    immediate: true,
                },
            ]
        );
    }
    assert_eq!(original.trigger_runtime, restored.trigger_runtime);
}

#[test]
fn change_visible_map_area_uses_native_parameter_indices_and_atoi() {
    let fields = vec![
        "991".to_string(),
        "-774".to_string(),
        "2cells".to_string(),
        "41".to_string(),
        "junk".to_string(),
        "+38tail".to_string(),
        "A".to_string(),
    ];
    assert_eq!(parse_visible_map_area(&fields), Some([2, 41, 0, 38]));
    assert_eq!(parse_visible_map_area(&fields[..5]), None);
}

#[test]
fn trigger_action_40_normalizes_and_refreshes_authority_same_frame() {
    use crate::map::map_file::MapHeader;
    use crate::map::playfield::PlayfieldBounds;
    use crate::map::resolved_terrain::zone_class;
    use crate::rules::locomotor_type::MovementZone;
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::pathfinding::PathGrid;
    use crate::sim::pathfinding::zone_map::ZONE_INVALID;

    let triggers: TriggerMap = [(
        "AREA".to_string(),
        make_trigger("AREA", None, "Change visible map area", true, false),
    )]
    .into_iter()
    .collect();
    let events: EventMap = [(
        "AREA".to_string(),
        MapEvent {
            id: "AREA".to_string(),
            fields: vec![],
            conditions: vec![EventCondition {
                kind: 47,
                params: vec!["0".to_string(), "0".to_string()],
            }],
        },
    )]
    .into_iter()
    .collect();
    // Stock all06umd-shaped payload: 40,0,0,2,41,56,38,A. With params equal
    // to chunk[1..], ParamType/Param3 are the first two zeroes and LocalSize
    // is exactly params[2..=5].
    let actions: ActionMap = [(
        "AREA".to_string(),
        MapAction {
            id: "AREA".to_string(),
            fields: vec![],
            entries: vec![ActionEntry {
                kind: 40,
                params: ["0", "0", "2", "41", "56", "38", "A"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                waypoint_index: Some(0),
            }],
        },
    )]
    .into_iter()
    .collect();
    let graph = build_trigger_graph(
        &HashMap::new(),
        &HashMap::new(),
        &triggers,
        &events,
        &actions,
    );
    let header = MapHeader {
        theater: "TEMPERATE".to_string(),
        fill: "Clear".to_string(),
        level: 0,
        width: 80,
        height: 58,
        local_left: 2,
        local_top: 4,
        local_width: 76,
        local_height: 48,
    };
    let mut sim = Simulation::new();
    sim.install_playfield_from_map_header(&header);
    let initial_bounds = sim.playfield_bounds.expect("initial playfield");
    let mut terrain = flat_trigger_playfield_terrain(100, 100);
    terrain.recalc_playfield_attributes(initial_bounds);
    let path_grid = PathGrid::from_resolved_terrain(&terrain);
    sim.resolved_terrain = Some(terrain);
    sim.rebuild_zone_grid(&path_grid);
    sim.trigger_runtime = TriggerRuntime::from_map(&triggers, &HashMap::new());

    let before_zone = sim
        .zone_grid
        .as_ref()
        .and_then(|zones| zones.map_for(MovementZone::Normal))
        .expect("normal zone")
        .zone_at(50, 50, MovementLayer::Ground);
    assert_ne!(before_zone, ZONE_INVALID);
    assert!(
        !sim.resolved_terrain
            .as_ref()
            .unwrap()
            .cell(50, 50)
            .unwrap()
            .outside_playfield
    );

    let tick = sim
        .advance_master_frame(
            &[],
            None,
            &BTreeMap::new(),
            None,
            None,
            67,
            TickLane::Ordinary,
            Some(TriggerInputs {
                graph: &graph,
                triggers: &triggers,
                events: &events,
                actions: &actions,
                waypoints: &HashMap::new(),
                rules: None,
            }),
        )
        .expect("fixture frame must complete");

    assert_eq!(
        sim.playfield_bounds,
        Some(PlayfieldBounds {
            base: 80,
            off_fc: 2,
            off_100: 41,
            off_104: 56,
            // Raw 38 clips to Size bottom 17, then the six-cell margin caps 11.
            off_108: 11,
        })
    );
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    assert!(terrain.cell(50, 50).unwrap().outside_playfield);
    assert_eq!(terrain.cell(50, 50).unwrap().zone_type, zone_class::OUTSIDE);
    assert!(!terrain.cell(85, 85).unwrap().outside_playfield);
    assert_eq!(terrain.cell(85, 85).unwrap().zone_type, zone_class::GROUND);
    let zones = sim.zone_grid.as_ref().expect("zones rebuilt");
    let normal = zones.map_for(MovementZone::Normal).expect("normal zone");
    assert_eq!(normal.zone_at(50, 50, MovementLayer::Ground), ZONE_INVALID);
    assert_ne!(normal.zone_at(85, 85, MovementLayer::Ground), ZONE_INVALID);
    assert!(sim.radar_terrain_dirty_cells.is_empty());
    assert_eq!(sim.radar_terrain_dirty_generation, 0);
    assert_eq!(sim.playfield_revision, 1);
    assert_eq!(tick.state_hash, sim.state_hash());

    // FUN_006E21E0 rebuilds radar surfaces for every firing. A second writer
    // must not disappear behind the persistent per-cell dirty-list dedup gate.
    assert!(sim.change_visible_map_area([4, 40, 54, 12], None));
    assert_eq!(sim.playfield_revision, 2);
    assert!(sim.change_visible_map_area([4, 40, 54, 12], None));
    assert_eq!(sim.playfield_revision, 3);
    assert!(sim.radar_terrain_dirty_cells.is_empty());

    let bytes = GameSnapshot::save(&sim, 0, 0, "all06umd.map", 0);
    let restored = GameSnapshot::load(&bytes)
        .expect("action-40 snapshot loads")
        .sim;
    assert_eq!(restored.playfield_bounds, sim.playfield_bounds);
    assert_eq!(restored.playfield_size_height, sim.playfield_size_height);
    assert_eq!(restored.playfield_revision, sim.playfield_revision);
}

#[test]
fn techno_playfield_action_40_exact_recompute_and_mobile_reveal_callback() {
    use crate::map::map_file::MapHeader;
    use crate::map::playfield::PlayfieldBounds;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::components::Health;
    use crate::sim::house_state::HouseState;

    let header = MapHeader {
        theater: "TEMPERATE".to_string(),
        fill: "Clear".to_string(),
        level: 0,
        width: 80,
        height: 58,
        local_left: 30,
        local_top: 20,
        local_width: 8,
        local_height: 6,
    };
    let mut sim = Simulation::new();
    sim.install_playfield_from_map_header(&header);
    let initial = sim.playfield_bounds.unwrap();
    let expanded = PlayfieldBounds::from_raw_local_size(80, 58, [2, 2, 76, 48]);
    let candidates: Vec<(u16, u16)> = (0u16..100)
        .flat_map(|ry| (0u16..100).map(move |rx| (rx, ry)))
        .filter(|&(rx, ry)| {
            expanded.contains_height_aware_packed(rx.into(), ry.into(), 0, 0)
                && !initial.contains_height_aware_packed(rx.into(), ry.into(), 0, 0)
        })
        .collect();
    let mobile_cell = candidates
        .get(candidates.len() / 2)
        .copied()
        .expect("expansion adds cells");
    let building_cell = candidates
        .iter()
        .copied()
        .max_by_key(|&(rx, ry)| {
            i32::from(rx).abs_diff(i32::from(mobile_cell.0))
                + i32::from(ry).abs_diff(i32::from(mobile_cell.1))
        })
        .expect("distant expansion cell");
    assert!(mobile_cell.0.abs_diff(building_cell.0) + mobile_cell.1.abs_diff(building_cell.1) > 10);
    let veteran_probe = [(3i32, 0i32), (-3, 0), (0, 3), (0, -3)]
        .into_iter()
        .find_map(|(dx, dy)| {
            let rx = i32::from(mobile_cell.0) + dx;
            let ry = i32::from(mobile_cell.1) + dy;
            (rx >= 0
                && ry >= 0
                && rx < 100
                && ry < 100
                && expanded.contains_height_aware_packed(rx, ry, 0, 0))
            .then_some((rx as u16, ry as u16))
        })
        .expect("effective veteran sight probe");
    // `UpdateReveal @ 0x0070AF50`: the veteran bonus is `ftol(Sight *
    // VeteranSight)` for a `SIGHT`-ability type, so the probe at distance 3
    // needs `1 * 3` and the ability — not the old additive `1 + 3`.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nVeteranSight=3\nRevealByHeight=no\n\
         [VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=300\nVeteranAbilities=SIGHT\n",
    ))
    .expect("effective-sight action fixture");

    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, Some(owner), true, 0, 10));
    sim.fog.width = 100;
    sim.fog.height = 100;
    let mobile_type = sim.interner.intern("MTNK");
    let building_type = sim.interner.intern("GAPOWR");
    let mut mobile = GameEntity::new_at_frame_zero_for_test(
        1,
        mobile_cell.0,
        mobile_cell.1,
        0,
        0,
        owner,
        Health { current: 100 },
        mobile_type,
        EntityCategory::Unit,
        100,
        1,
        true,
    );
    mobile.lifecycle.object_alive = true;
    mobile.lifecycle.in_limbo = false;
    let mut building = GameEntity::new_at_frame_zero_for_test(
        2,
        building_cell.0,
        building_cell.1,
        0,
        0,
        owner,
        Health { current: 100 },
        building_type,
        EntityCategory::Structure,
        0,
        1,
        false,
    );
    building.lifecycle.object_alive = true;
    building.lifecycle.in_limbo = false;
    sim.substrate.entities.insert(mobile);
    sim.substrate.entities.insert(building);
    sim.resolved_terrain = Some(flat_trigger_playfield_terrain(100, 100));

    assert!(sim.change_visible_map_area([2, 2, 76, 48], Some(&rules)));
    assert!(sim.substrate.entities.get(1).unwrap().in_playfield);
    assert!(sim.substrate.entities.get(2).unwrap().in_playfield);
    assert!(
        sim.fog
            .is_cell_revealed(owner, mobile_cell.0, mobile_cell.1)
    );
    assert!(
        sim.fog
            .is_cell_revealed(owner, veteran_probe.0, veteran_probe.1),
        "0x0070ADC0 callback must use the shared veteran-adjusted sight, not raw VisionRange"
    );
    assert!(
        !sim.fog
            .is_cell_revealed(owner, building_cell.0, building_cell.1),
        "FUN_006E21E0 excludes BuildingClass from the false-to-true callback"
    );

    assert!(sim.change_visible_map_area([30, 20, 8, 6], Some(&rules)));
    assert!(!sim.substrate.entities.get(1).unwrap().in_playfield);
    assert!(!sim.substrate.entities.get(2).unwrap().in_playfield);
    assert!(sim.change_visible_map_area([2, 2, 76, 48], Some(&rules)));
    assert!(sim.substrate.entities.get(1).unwrap().in_playfield);
    assert!(sim.substrate.entities.get(2).unwrap().in_playfield);
}

#[test]
fn time_trigger_can_center_camera_at_waypoint() {
    let triggers: TriggerMap = [(
        "TRIG_A".to_string(),
        make_trigger("TRIG_A", None, "Timer A", true, false),
    )]
    .into_iter()
    .collect();
    let events: EventMap = [(
        "TRIG_A".to_string(),
        MapEvent {
            id: "TRIG_A".to_string(),
            fields: vec![
                "1".to_string(),
                "47".to_string(),
                "3".to_string(),
                "0".to_string(),
            ],
            conditions: vec![EventCondition {
                kind: 47,
                params: vec!["3".to_string(), "0".to_string()],
            }],
        },
    )]
    .into_iter()
    .collect();
    let actions: ActionMap = [(
        "TRIG_A".to_string(),
        MapAction {
            id: "TRIG_A".to_string(),
            fields: vec![
                "1".to_string(),
                "112".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "J".to_string(),
            ],
            entries: vec![ActionEntry {
                kind: 112,
                // Runtime owns the reader-materialized +0x44 value below;
                // changing retained source text must not trigger re-decoding.
                params: vec![
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "ZZ".to_string(),
                ],
                waypoint_index: Some(9),
            }],
        },
    )]
    .into_iter()
    .collect();
    let graph = build_trigger_graph(
        &HashMap::new(),
        &HashMap::new(),
        &triggers,
        &events,
        &actions,
    );
    let mut runtime = TriggerRuntime::from_map(&triggers, &HashMap::new());

    assert!(
        runtime
            .advance_at_frame(
                44,
                &graph,
                &triggers,
                &events,
                &actions,
                None,
                None,
                &HashMap::new(),
            )
            .is_empty()
    );
    assert_eq!(
        runtime.advance_at_frame(
            45,
            &graph,
            &triggers,
            &events,
            &actions,
            None,
            None,
            &HashMap::new(),
        ),
        vec![TriggerEffect::CenterCameraAtWaypoint {
            waypoint: 9,
            immediate: true,
        }]
    );
    assert!(
        runtime
            .advance_at_frame(
                46,
                &graph,
                &triggers,
                &events,
                &actions,
                None,
                None,
                &HashMap::new(),
            )
            .is_empty()
    );
}

#[test]
fn master_frame_polls_triggers_before_logic_houses_commit_and_delete() {
    let triggers: TriggerMap = [(
        "TRIG_A".to_string(),
        make_trigger("TRIG_A", None, "Frame Zero", true, false),
    )]
    .into_iter()
    .collect();
    let events: EventMap = [(
        "TRIG_A".to_string(),
        MapEvent {
            id: "TRIG_A".to_string(),
            fields: vec![],
            conditions: vec![EventCondition {
                kind: 47,
                params: vec!["0".to_string(), "0".to_string()],
            }],
        },
    )]
    .into_iter()
    .collect();
    let actions: ActionMap = [(
        "TRIG_A".to_string(),
        MapAction {
            id: "TRIG_A".to_string(),
            fields: vec![],
            entries: vec![ActionEntry {
                kind: 112,
                params: vec![
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "J".to_string(),
                ],
                waypoint_index: Some(9),
            }],
        },
    )]
    .into_iter()
    .collect();
    let graph = build_trigger_graph(
        &HashMap::new(),
        &HashMap::new(),
        &triggers,
        &events,
        &actions,
    );
    let mut sim = Simulation::new();
    sim.trigger_runtime = TriggerRuntime::from_map(&triggers, &HashMap::new());

    let tick = sim
        .advance_master_frame(
            &[],
            None,
            &BTreeMap::new(),
            None,
            None,
            67,
            TickLane::Ordinary,
            Some(TriggerInputs {
                graph: &graph,
                triggers: &triggers,
                events: &events,
                actions: &actions,
                waypoints: &HashMap::new(),
                rules: None,
            }),
        )
        .expect("fixture frame must complete");

    assert!(tick.frame_committed);
    assert_eq!(
        sim.take_master_frame_test_trace(),
        vec![
            MasterFrameTestRung::SessionCommands,
            MasterFrameTestRung::Triggers,
            MasterFrameTestRung::TeamScript,
            MasterFrameTestRung::LogicVector,
            MasterFrameTestRung::Houses,
            MasterFrameTestRung::FrameCommit,
            MasterFrameTestRung::PendingDelete,
        ]
    );
    assert_eq!(
        sim.drain_trigger_effects(),
        vec![TriggerEffect::CenterCameraAtWaypoint {
            waypoint: 9,
            immediate: true,
        }]
    );
}

#[test]
fn master_frame_save_load_continues_trigger_projectile_and_delete_state() {
    let triggers: TriggerMap = [(
        "TRIG_A".to_string(),
        make_trigger("TRIG_A", None, "Set Global", true, false),
    )]
    .into_iter()
    .collect();
    let events: EventMap = [(
        "TRIG_A".to_string(),
        MapEvent {
            id: "TRIG_A".to_string(),
            fields: vec![],
            conditions: vec![EventCondition {
                kind: 47,
                params: vec!["0".to_string(), "0".to_string()],
            }],
        },
    )]
    .into_iter()
    .collect();
    let actions: ActionMap = [(
        "TRIG_A".to_string(),
        MapAction {
            id: "TRIG_A".to_string(),
            fields: vec![],
            entries: vec![ActionEntry {
                kind: 28,
                params: vec!["13".to_string()],
                waypoint_index: Some(0),
            }],
        },
    )]
    .into_iter()
    .collect();
    let graph = build_trigger_graph(
        &HashMap::new(),
        &HashMap::new(),
        &triggers,
        &events,
        &actions,
    );
    let height_map = BTreeMap::new();
    let waypoints = HashMap::new();
    let trigger_inputs = TriggerInputs {
        graph: &graph,
        triggers: &triggers,
        events: &events,
        actions: &actions,
        waypoints: &waypoints,
        rules: None,
    };

    let mut original = Simulation::new();
    original.trigger_runtime = TriggerRuntime::from_map(&triggers, &HashMap::new());
    original
        .advance_master_frame(
            &[],
            None,
            &height_map,
            None,
            None,
            67,
            TickLane::Ordinary,
            Some(trigger_inputs),
        )
        .expect("fixture frame must complete");
    assert!(original.trigger_runtime.globals_set.contains(&13));

    let projectile_id = original.allocate_stable_id();
    original.admit_projectile(
        projectile_id,
        ProjectileSpawn {
            flat: false,
            source_id: crate::sim::combat::RAD_NO_ATTACKER,
            origin: ProjectileCoord::new(0, 0, 0),
            target: ProjectileTarget::Cell { rx: 4, ry: 0 },
            initial_target_position: ProjectileCoord::new(1024, 0, 0),
            payload: ProjectilePayload {
                base_damage: 40,
                warhead: crate::sim::intern::InternedId::from_index(0),
                weapon: crate::sim::intern::InternedId::from_index(0),
                owner: crate::sim::intern::InternedId::from_index(0),
            },
            speed_leptons_per_frame: 64,
            velocity: crate::sim::projectile::ProjectileVelocity::new(64, 0, 0),
            trajectory: crate::sim::projectile::ProjectileTrajectory::Straight,
            guidance: None,
            visual: crate::sim::projectile::ProjectileVisualState::new(0, 0, 0),
            arm_frames: 0,
            fuse_frames: None,
            ranged_fuse: false,
            tracks_target: false,
            target_expiry: TargetExpiryPolicy::Expire,
            collision: ProjectileCollisionPolicy::NONE,
        },
    );
    let deleted_id = spawn_type(&mut original, "GAPOWR");
    original.uninit(deleted_id);
    assert_eq!(original.projectiles.len(), 1);
    assert!(original.substrate.pending_delete.contains(&deleted_id));

    // Native in-scenario load restarts Scenario RNG from Seed0. Continue both
    // branches from that cursor while testing unrelated trigger/lifecycle state.
    original.scenario_rng = crate::sim::rng::SimRng::new(0);
    let hash_before_save = original.state_hash();
    let bytes = GameSnapshot::save(&original, 0, 0, "test_map", 0);
    let mut restored = GameSnapshot::load(&bytes).expect("snapshot loads").sim;
    restored
        .restore_after_snapshot_load()
        .expect("snapshot references and Logic membership restore");
    assert_eq!(restored.state_hash(), hash_before_save);
    assert!(restored.trigger_runtime.globals_set.contains(&13));
    assert_eq!(restored.projectiles.len(), 1);
    assert!(restored.substrate.pending_delete.contains(&deleted_id));

    let original_tick = original
        .advance_master_frame(
            &[],
            None,
            &height_map,
            None,
            None,
            67,
            TickLane::Ordinary,
            Some(trigger_inputs),
        )
        .expect("fixture frame must complete");
    let mut replay_log = ReplayLog::new(ReplayHeader {
        pixel_conversion_bounds: Default::default(),
        version: 1,
        tick_hz: 15,
        seed: original.session.seed,
        map_name: original.session.map_name.clone(),
        rules_hash: 0,
    });
    replay_log.record_tick(original_tick.tick, Vec::new(), original_tick.state_hash);
    let restored_tick = restored
        .advance_master_frame(
            &[],
            None,
            &height_map,
            None,
            None,
            67,
            TickLane::Ordinary,
            Some(trigger_inputs),
        )
        .expect("fixture frame must complete");

    assert!(original_tick.frame_committed);
    assert!(restored_tick.frame_committed);
    assert_eq!(original.state_hash(), restored.state_hash());
    assert_eq!(original.projectiles.len(), 1);
    assert!(original.substrate.pending_delete.is_empty());

    let mut replayed = GameSnapshot::load(&bytes).expect("snapshot reloads").sim;
    replayed
        .restore_after_snapshot_load()
        .expect("replay snapshot references and Logic membership restore");
    assert_eq!(
        ReplayRunner::run_fixture_master_frame(
            &mut replayed,
            &replay_log,
            None,
            &height_map,
            None,
            None,
            67,
            Some(trigger_inputs),
        ),
        vec![original_tick.state_hash],
    );
}

#[test]
fn trigger_runtime_latches_participate_in_state_hash() {
    let mut sim = Simulation::new();
    let before = sim.state_hash();

    sim.trigger_runtime.globals_set.insert(13);

    assert_ne!(sim.state_hash(), before);
}

#[test]
fn elapsed_time_uses_signed_current_frame_divided_by_fifteen() {
    let runtime = TriggerRuntime::default();
    let one_second = EventCondition {
        kind: 47,
        params: vec!["1".to_string(), "0".to_string()],
    };
    let zero_seconds = EventCondition {
        kind: 47,
        params: vec!["0".to_string(), "0".to_string()],
    };

    assert!(runtime.evaluate_event(&zero_seconds, 0, None));
    assert!(!runtime.evaluate_event(&one_second, 14, None));
    assert!(runtime.evaluate_event(&one_second, 15, None));
    assert!(
        !runtime.evaluate_event(&one_second, 0x8000_0000, None),
        "the native frame counter is divided as a signed 32-bit value"
    );
}

#[test]
fn global_actions_can_enable_and_force_followup_trigger() {
    let triggers: TriggerMap = [
        (
            "TRIG_A".to_string(),
            make_trigger("TRIG_A", None, "Set Global", true, false),
        ),
        (
            "TRIG_B".to_string(),
            make_trigger("TRIG_B", None, "Camera", false, false),
        ),
    ]
    .into_iter()
    .collect();
    let events: EventMap = [
        (
            "TRIG_A".to_string(),
            MapEvent {
                id: "TRIG_A".to_string(),
                fields: vec![
                    "1".to_string(),
                    "47".to_string(),
                    "1".to_string(),
                    "0".to_string(),
                ],
                conditions: vec![EventCondition {
                    kind: 47,
                    params: vec!["1".to_string(), "0".to_string()],
                }],
            },
        ),
        (
            "TRIG_B".to_string(),
            MapEvent {
                id: "TRIG_B".to_string(),
                fields: vec![
                    "1".to_string(),
                    "27".to_string(),
                    "7".to_string(),
                    "0".to_string(),
                ],
                conditions: vec![EventCondition {
                    kind: 27,
                    params: vec!["7".to_string(), "0".to_string()],
                }],
            },
        ),
    ]
    .into_iter()
    .collect();
    let actions: ActionMap = [
        (
            "TRIG_A".to_string(),
            MapAction {
                id: "TRIG_A".to_string(),
                fields: vec![
                    "D".to_string(),
                    "28".to_string(),
                    "7".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "53".to_string(),
                    "TRIG_B".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "22".to_string(),
                    "TRIG_B".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                ],
                entries: vec![
                    ActionEntry {
                        kind: 28,
                        params: vec![
                            "7".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                        ],
                        waypoint_index: None,
                    },
                    ActionEntry {
                        kind: 53,
                        params: vec![
                            "TRIG_B".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                        ],
                        waypoint_index: None,
                    },
                    ActionEntry {
                        kind: 22,
                        params: vec![
                            "TRIG_B".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                        ],
                        waypoint_index: None,
                    },
                ],
            },
        ),
        (
            "TRIG_B".to_string(),
            MapAction {
                id: "TRIG_B".to_string(),
                fields: vec![
                    "1".to_string(),
                    "112".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "D".to_string(),
                ],
                entries: vec![ActionEntry {
                    kind: 112,
                    params: vec![
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "D".to_string(),
                    ],
                    waypoint_index: Some(3),
                }],
            },
        ),
    ]
    .into_iter()
    .collect();
    let graph = build_trigger_graph(
        &HashMap::new(),
        &HashMap::new(),
        &triggers,
        &events,
        &actions,
    );
    let mut runtime = TriggerRuntime::from_map(&triggers, &HashMap::new());

    assert_eq!(
        runtime.advance_at_frame(
            15,
            &graph,
            &triggers,
            &events,
            &actions,
            None,
            None,
            &HashMap::new(),
        ),
        vec![TriggerEffect::CenterCameraAtWaypoint {
            waypoint: 3,
            immediate: true,
        }]
    );
}

#[test]
fn linked_trigger_field_queues_followup_trigger() {
    let triggers: TriggerMap = [
        (
            "TRIG_A".to_string(),
            make_trigger("TRIG_A", Some("TRIG_B"), "Primary", true, false),
        ),
        (
            "TRIG_B".to_string(),
            make_trigger("TRIG_B", None, "Followup", true, false),
        ),
    ]
    .into_iter()
    .collect();
    let events: EventMap = [
        (
            "TRIG_A".to_string(),
            MapEvent {
                id: "TRIG_A".to_string(),
                fields: vec![
                    "1".to_string(),
                    "47".to_string(),
                    "1".to_string(),
                    "0".to_string(),
                ],
                conditions: vec![EventCondition {
                    kind: 47,
                    params: vec!["1".to_string(), "0".to_string()],
                }],
            },
        ),
        (
            "TRIG_B".to_string(),
            MapEvent {
                id: "TRIG_B".to_string(),
                fields: vec![
                    "1".to_string(),
                    "28".to_string(),
                    "9".to_string(),
                    "0".to_string(),
                ],
                conditions: vec![EventCondition {
                    kind: 28,
                    params: vec!["9".to_string(), "0".to_string()],
                }],
            },
        ),
    ]
    .into_iter()
    .collect();
    let actions: ActionMap = [
        (
            "TRIG_A".to_string(),
            MapAction {
                id: "TRIG_A".to_string(),
                fields: vec![
                    "1".to_string(),
                    "28".to_string(),
                    "5".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                ],
                entries: vec![ActionEntry {
                    kind: 28,
                    params: vec![
                        "5".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                    ],
                    waypoint_index: Some(0),
                }],
            },
        ),
        (
            "TRIG_B".to_string(),
            MapAction {
                id: "TRIG_B".to_string(),
                fields: vec![
                    "1".to_string(),
                    "112".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "E".to_string(),
                ],
                entries: vec![ActionEntry {
                    kind: 112,
                    params: vec![
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "E".to_string(),
                    ],
                    waypoint_index: Some(4),
                }],
            },
        ),
    ]
    .into_iter()
    .collect();
    let graph = build_trigger_graph(
        &HashMap::new(),
        &HashMap::new(),
        &triggers,
        &events,
        &actions,
    );
    let mut runtime = TriggerRuntime::from_map(&triggers, &HashMap::new());

    assert_eq!(
        runtime.advance_at_frame(
            15,
            &graph,
            &triggers,
            &events,
            &actions,
            None,
            None,
            &HashMap::new(),
        ),
        vec![TriggerEffect::CenterCameraAtWaypoint {
            waypoint: 4,
            immediate: true,
        }]
    );
}

#[test]
fn forced_trigger_with_unmet_conditions_does_not_fire() {
    let triggers: TriggerMap = [
        (
            "TRIG_A".to_string(),
            make_trigger("TRIG_A", None, "Force", true, false),
        ),
        (
            "TRIG_B".to_string(),
            make_trigger("TRIG_B", None, "Blocked", true, false),
        ),
    ]
    .into_iter()
    .collect();
    let events: EventMap = [
        (
            "TRIG_A".to_string(),
            MapEvent {
                id: "TRIG_A".to_string(),
                fields: vec![
                    "1".to_string(),
                    "47".to_string(),
                    "1".to_string(),
                    "0".to_string(),
                ],
                conditions: vec![EventCondition {
                    kind: 47,
                    params: vec!["1".to_string(), "0".to_string()],
                }],
            },
        ),
        (
            "TRIG_B".to_string(),
            MapEvent {
                id: "TRIG_B".to_string(),
                fields: vec![
                    "1".to_string(),
                    "27".to_string(),
                    "99".to_string(),
                    "0".to_string(),
                ],
                conditions: vec![EventCondition {
                    kind: 27,
                    params: vec!["99".to_string(), "0".to_string()],
                }],
            },
        ),
    ]
    .into_iter()
    .collect();
    let actions: ActionMap = [
        (
            "TRIG_A".to_string(),
            MapAction {
                id: "TRIG_A".to_string(),
                fields: vec![
                    "1".to_string(),
                    "22".to_string(),
                    "TRIG_B".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                ],
                entries: vec![ActionEntry {
                    kind: 22,
                    params: vec![
                        "TRIG_B".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                    ],
                    waypoint_index: None,
                }],
            },
        ),
        (
            "TRIG_B".to_string(),
            MapAction {
                id: "TRIG_B".to_string(),
                fields: vec![
                    "1".to_string(),
                    "112".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "I".to_string(),
                ],
                entries: vec![ActionEntry {
                    kind: 112,
                    params: vec![
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "I".to_string(),
                    ],
                    waypoint_index: Some(8),
                }],
            },
        ),
    ]
    .into_iter()
    .collect();
    let graph = build_trigger_graph(
        &HashMap::new(),
        &HashMap::new(),
        &triggers,
        &events,
        &actions,
    );
    let mut runtime = TriggerRuntime::from_map(&triggers, &HashMap::new());

    assert_eq!(
        runtime.advance_at_frame(
            15,
            &graph,
            &triggers,
            &events,
            &actions,
            None,
            None,
            &HashMap::new(),
        ),
        Vec::<TriggerEffect>::new()
    );
}

#[test]
fn mission_announce_then_force_end_emits_result_effects() {
    let triggers: TriggerMap = [(
        "TRIG_A".to_string(),
        make_trigger("TRIG_A", None, "End Mission", true, false),
    )]
    .into_iter()
    .collect();
    let events: EventMap = [(
        "TRIG_A".to_string(),
        MapEvent {
            id: "TRIG_A".to_string(),
            fields: vec![
                "1".to_string(),
                "47".to_string(),
                "1".to_string(),
                "0".to_string(),
            ],
            conditions: vec![EventCondition {
                kind: 47,
                params: vec!["1".to_string(), "0".to_string()],
            }],
        },
    )]
    .into_iter()
    .collect();
    let actions: ActionMap = [(
        "TRIG_A".to_string(),
        MapAction {
            id: "TRIG_A".to_string(),
            fields: vec![
                "2".to_string(),
                "67".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "69".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
            ],
            entries: vec![
                ActionEntry {
                    kind: 67,
                    params: vec![
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                    ],
                    waypoint_index: None,
                },
                ActionEntry {
                    kind: 69,
                    params: vec![
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                    ],
                    waypoint_index: None,
                },
            ],
        },
    )]
    .into_iter()
    .collect();
    let graph = build_trigger_graph(
        &HashMap::new(),
        &HashMap::new(),
        &triggers,
        &events,
        &actions,
    );
    let mut runtime = TriggerRuntime::from_map(&triggers, &HashMap::new());

    assert_eq!(
        runtime.advance_at_frame(
            15,
            &graph,
            &triggers,
            &events,
            &actions,
            None,
            None,
            &HashMap::new(),
        ),
        vec![
            TriggerEffect::MissionAnnouncement {
                text: "Mission Accomplished".to_string(),
            },
            TriggerEffect::MissionResult {
                title: "Mission Accomplished".to_string(),
                detail: "The scenario ended after a victory announcement.".to_string(),
            },
        ]
    );
}

#[test]
fn local_variables_seed_and_gate_followup_triggers() {
    let triggers: TriggerMap = [
        (
            "TRIG_A".to_string(),
            make_trigger("TRIG_A", None, "Flip Local", true, false),
        ),
        (
            "TRIG_B".to_string(),
            make_trigger("TRIG_B", None, "Uses Local", true, false),
        ),
    ]
    .into_iter()
    .collect();
    let events: EventMap = [
        (
            "TRIG_A".to_string(),
            MapEvent {
                id: "TRIG_A".to_string(),
                fields: vec![
                    "1".to_string(),
                    "37".to_string(),
                    "2".to_string(),
                    "0".to_string(),
                ],
                conditions: vec![EventCondition {
                    kind: 37,
                    params: vec!["2".to_string(), "0".to_string()],
                }],
            },
        ),
        (
            "TRIG_B".to_string(),
            MapEvent {
                id: "TRIG_B".to_string(),
                fields: vec![
                    "1".to_string(),
                    "36".to_string(),
                    "2".to_string(),
                    "0".to_string(),
                ],
                conditions: vec![EventCondition {
                    kind: 36,
                    params: vec!["2".to_string(), "0".to_string()],
                }],
            },
        ),
    ]
    .into_iter()
    .collect();
    let actions: ActionMap = [
        (
            "TRIG_A".to_string(),
            MapAction {
                id: "TRIG_A".to_string(),
                fields: vec![
                    "1".to_string(),
                    "56".to_string(),
                    "2".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                ],
                entries: vec![ActionEntry {
                    kind: 56,
                    params: vec![
                        "2".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                    ],
                    waypoint_index: Some(0),
                }],
            },
        ),
        (
            "TRIG_B".to_string(),
            MapAction {
                id: "TRIG_B".to_string(),
                fields: vec![
                    "1".to_string(),
                    "112".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "G".to_string(),
                ],
                entries: vec![ActionEntry {
                    kind: 112,
                    params: vec![
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "G".to_string(),
                    ],
                    waypoint_index: Some(6),
                }],
            },
        ),
    ]
    .into_iter()
    .collect();
    let local_variables: LocalVariableMap = [(
        2,
        LocalVariable {
            index: 2,
            name: "BridgeDone".to_string(),
            initially_set: false,
        },
    )]
    .into_iter()
    .collect();
    let graph = build_trigger_graph(
        &HashMap::new(),
        &HashMap::new(),
        &triggers,
        &events,
        &actions,
    );
    let mut runtime = TriggerRuntime::from_map(&triggers, &local_variables);

    assert_eq!(
        runtime.advance_at_frame(
            0,
            &graph,
            &triggers,
            &events,
            &actions,
            None,
            None,
            &HashMap::new(),
        ),
        Vec::<TriggerEffect>::new()
    );
    assert!(runtime.locals_set.contains(&2));
    assert_eq!(
        runtime.advance_at_frame(
            0,
            &graph,
            &triggers,
            &events,
            &actions,
            None,
            None,
            &HashMap::new(),
        ),
        vec![TriggerEffect::CenterCameraAtWaypoint {
            waypoint: 6,
            immediate: true,
        }]
    );
}

#[test]
fn techtype_exists_and_not_exists_query_simulation_world() {
    let triggers: TriggerMap = [
        (
            "TRIG_A".to_string(),
            make_trigger("TRIG_A", None, "Need Two Power Plants", true, false),
        ),
        (
            "TRIG_B".to_string(),
            make_trigger("TRIG_B", None, "No Radar", true, false),
        ),
    ]
    .into_iter()
    .collect();
    let events: EventMap = [
        (
            "TRIG_A".to_string(),
            MapEvent {
                id: "TRIG_A".to_string(),
                fields: vec![
                    "1".to_string(),
                    "60".to_string(),
                    "2".to_string(),
                    "GAPOWR".to_string(),
                ],
                conditions: vec![EventCondition {
                    kind: 60,
                    params: vec!["2".to_string(), "GAPOWR".to_string()],
                }],
            },
        ),
        (
            "TRIG_B".to_string(),
            MapEvent {
                id: "TRIG_B".to_string(),
                fields: vec![
                    "1".to_string(),
                    "61".to_string(),
                    "0".to_string(),
                    "GAAIRC".to_string(),
                ],
                conditions: vec![EventCondition {
                    kind: 61,
                    params: vec!["0".to_string(), "GAAIRC".to_string()],
                }],
            },
        ),
    ]
    .into_iter()
    .collect();
    let actions: ActionMap = [
        (
            "TRIG_A".to_string(),
            MapAction {
                id: "TRIG_A".to_string(),
                fields: vec![
                    "1".to_string(),
                    "112".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "L".to_string(),
                ],
                entries: vec![ActionEntry {
                    kind: 112,
                    params: vec![
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "L".to_string(),
                    ],
                    waypoint_index: Some(11),
                }],
            },
        ),
        (
            "TRIG_B".to_string(),
            MapAction {
                id: "TRIG_B".to_string(),
                fields: vec![
                    "1".to_string(),
                    "112".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "M".to_string(),
                ],
                entries: vec![ActionEntry {
                    kind: 112,
                    params: vec![
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "M".to_string(),
                    ],
                    waypoint_index: Some(12),
                }],
            },
        ),
    ]
    .into_iter()
    .collect();
    let graph = build_trigger_graph(
        &HashMap::new(),
        &HashMap::new(),
        &triggers,
        &events,
        &actions,
    );
    let mut runtime = TriggerRuntime::from_map(&triggers, &HashMap::new());
    let mut sim = Simulation::new();
    spawn_type(&mut sim, "GAPOWR");
    spawn_type(&mut sim, "GAPOWR");

    assert_eq!(
        runtime.advance_at_frame(
            0,
            &graph,
            &triggers,
            &events,
            &actions,
            Some(&mut sim),
            None,
            &HashMap::new(),
        ),
        vec![
            TriggerEffect::CenterCameraAtWaypoint {
                waypoint: 11,
                immediate: true,
            },
            TriggerEffect::CenterCameraAtWaypoint {
                waypoint: 12,
                immediate: true,
            },
        ]
    );
}

fn run_waypoint_action(
    kind: i32,
    token: Option<&str>,
    trigger_country: Option<&str>,
    sim: &mut Simulation,
    waypoints: &HashMap<u32, crate::map::waypoints::Waypoint>,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> Vec<TriggerEffect> {
    let mut trigger = make_trigger("WAYPOINT_ACTION", None, "Waypoint action", true, false);
    trigger.owner = trigger_country.map(str::to_string);
    trigger.fields[0] = trigger_country.unwrap_or("").to_string();
    let triggers: TriggerMap = [(trigger.id.clone(), trigger)].into_iter().collect();
    let events: EventMap = [(
        "WAYPOINT_ACTION".to_string(),
        MapEvent {
            id: "WAYPOINT_ACTION".to_string(),
            fields: Vec::new(),
            conditions: vec![EventCondition {
                kind: 47,
                params: vec!["0".to_string()],
            }],
        },
    )]
    .into_iter()
    .collect();
    let mut params = vec!["0".to_string(); 6];
    if let Some(token) = token {
        params.push(token.to_string());
    }
    let actions: ActionMap = [(
        "WAYPOINT_ACTION".to_string(),
        MapAction {
            id: "WAYPOINT_ACTION".to_string(),
            fields: Vec::new(),
            entries: vec![ActionEntry {
                kind,
                params,
                waypoint_index: crate::map::actions::read_waypoint_token(token),
            }],
        },
    )]
    .into_iter()
    .collect();
    let graph = build_trigger_graph(
        &HashMap::new(),
        &HashMap::new(),
        &triggers,
        &events,
        &actions,
    );
    let mut runtime = TriggerRuntime::from_map(&triggers, &HashMap::new());
    runtime.advance_at_frame(
        0,
        &graph,
        &triggers,
        &events,
        &actions,
        Some(sim),
        rules,
        waypoints,
    )
}

fn waypoint_action_rules() -> crate::rules::ruleset::RuleSet {
    crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[Countries]\n0=Americans\n1=Russians\n\
         [Americans]\nName=United States\n\
         [Russians]\nName=Russian Federation\n",
    ))
    .expect("waypoint-action country registry parses")
}

fn register_waypoint_action_house(
    sim: &mut Simulation,
    name: &str,
    country: Option<&str>,
    alternate_base_center: (u16, u16),
) -> crate::sim::intern::InternedId {
    let owner = sim.interner.intern(name);
    let country = country.map(|country| sim.interner.intern(country));
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, country, false, 0, 10);
    house.base_center = Some((40, 50));
    house.alternate_base_center = alternate_base_center;
    sim.houses.insert(owner, house);
    sim.session.house_order.push(owner);
    owner
}

#[test]
fn camera_actions_use_alpha_tokens_and_ctor_zero_not_decimal_indices() {
    for (kind, token, waypoint, immediate) in [(48, "NZ", 389, false), (112, "AA", 26, true)] {
        let mut sim = Simulation::new();
        assert_eq!(
            run_waypoint_action(kind, Some(token), None, &mut sim, &HashMap::new(), None),
            vec![TriggerEffect::CenterCameraAtWaypoint {
                waypoint,
                immediate,
            }]
        );
    }

    for token in [None, Some("")] {
        let mut sim = Simulation::new();
        assert_eq!(
            run_waypoint_action(112, token, None, &mut sim, &HashMap::new(), None),
            vec![TriggerEffect::CenterCameraAtWaypoint {
                waypoint: 0,
                immediate: true,
            }]
        );
    }
    for token in [Some("15"), Some("   ")] {
        let mut sim = Simulation::new();
        assert!(run_waypoint_action(112, token, None, &mut sim, &HashMap::new(), None).is_empty());
    }
}

#[test]
fn action_137_replays_all_three_retail_waypoint_fixtures() {
    for (token, index, cell) in [
        ("P", 15, (93, 106)),
        ("NZ", 389, (122, 135)),
        ("AA", 26, (105, 194)),
    ] {
        let mut sim = Simulation::new();
        let rules = waypoint_action_rules();
        let house = register_waypoint_action_house(&mut sim, "HouseA", Some("Americans"), (0, 0));
        let waypoints = [(
            index,
            crate::map::waypoints::Waypoint {
                index,
                rx: cell.0,
                ry: cell.1,
            },
        )]
        .into_iter()
        .collect();

        assert!(
            run_waypoint_action(
                137,
                Some(token),
                Some("americans"),
                &mut sim,
                &waypoints,
                Some(&rules),
            )
            .is_empty()
        );
        assert_eq!(sim.houses[&house].alternate_base_center, cell);
        assert_eq!(sim.houses[&house].base_center, Some((40, 50)));
    }
}

#[test]
fn action_137_uses_first_registration_order_country_match_only() {
    let mut sim = Simulation::new();
    let rules = waypoint_action_rules();
    let first = register_waypoint_action_house(&mut sim, "First", Some("Americans"), (1, 2));
    let second = register_waypoint_action_house(&mut sim, "Second", Some("AMERICANS"), (3, 4));
    let waypoints = [(
        15,
        crate::map::waypoints::Waypoint {
            index: 15,
            rx: 93,
            ry: 106,
        },
    )]
    .into_iter()
    .collect();

    run_waypoint_action(
        137,
        Some("P"),
        Some("americans"),
        &mut sim,
        &waypoints,
        Some(&rules),
    );

    assert_eq!(sim.houses[&first].alternate_base_center, (93, 106));
    assert_eq!(sim.houses[&second].alternate_base_center, (3, 4));
    assert_eq!(sim.houses[&first].base_center, Some((40, 50)));
    assert_eq!(sim.houses[&second].base_center, Some((40, 50)));
}

#[test]
fn action_137_canonicalizes_house_type_alias_and_none_owner() {
    let rules = waypoint_action_rules();
    let waypoints = [(
        15,
        crate::map::waypoints::Waypoint {
            index: 15,
            rx: 93,
            ry: 106,
        },
    )]
    .into_iter()
    .collect();

    let mut alias_sim = Simulation::new();
    let alias_house =
        register_waypoint_action_house(&mut alias_sim, "AliasHouse", Some("Americans"), (0, 0));
    run_waypoint_action(
        137,
        Some("P"),
        Some("united states"),
        &mut alias_sim,
        &waypoints,
        Some(&rules),
    );
    assert_eq!(
        alias_sim.houses[&alias_house].alternate_base_center,
        (93, 106)
    );

    let mut none_sim = Simulation::new();
    let russian =
        register_waypoint_action_house(&mut none_sim, "RussianHouse", Some("Russians"), (7, 8));
    let first_type =
        register_waypoint_action_house(&mut none_sim, "AmericanHouse", Some("Americans"), (0, 0));
    run_waypoint_action(
        137,
        Some("P"),
        Some("<none>"),
        &mut none_sim,
        &waypoints,
        Some(&rules),
    );
    assert_eq!(none_sim.houses[&russian].alternate_base_center, (7, 8));
    assert_eq!(
        none_sim.houses[&first_type].alternate_base_center,
        (93, 106)
    );
}

#[test]
fn action_137_rejects_only_exact_packed_zero_cell() {
    let rules = waypoint_action_rules();
    for cell in [(0, 17), (23, 0)] {
        let mut sim = Simulation::new();
        let house =
            register_waypoint_action_house(&mut sim, "AxisHouse", Some("Americans"), (9, 9));
        let waypoints = [(
            15,
            crate::map::waypoints::Waypoint {
                index: 15,
                rx: cell.0,
                ry: cell.1,
            },
        )]
        .into_iter()
        .collect();
        run_waypoint_action(
            137,
            Some("P"),
            Some("Americans"),
            &mut sim,
            &waypoints,
            Some(&rules),
        );
        assert_eq!(sim.houses[&house].alternate_base_center, cell);
    }
}

#[test]
fn action_137_writes_signed_waypoint_halves_as_raw_nonzero_cell() {
    let rules = waypoint_action_rules();
    let mut sim = Simulation::new();
    let house =
        register_waypoint_action_house(&mut sim, "SignedWaypointHouse", Some("Americans"), (9, 9));
    let waypoints = crate::map::waypoints::parse_waypoints(
        &crate::rules::ini_parser::IniFile::from_str("[Waypoints]\n15=-1001\n"),
    );

    run_waypoint_action(
        137,
        Some("P"),
        Some("Americans"),
        &mut sim,
        &waypoints,
        Some(&rules),
    );

    assert_eq!(
        sim.houses[&house].alternate_base_center,
        (u16::MAX, u16::MAX),
        "signed -1 quotient/remainder halves are nonzero and must be written"
    );
    assert_eq!(sim.houses[&house].base_center, Some((40, 50)));
}

#[test]
fn action_137_invalid_resolution_and_waypoints_do_not_mutate_either_base_cell() {
    fn assert_no_write(
        trigger_country: Option<&str>,
        house_country: Option<&str>,
        token: Option<&str>,
        waypoints: HashMap<u32, crate::map::waypoints::Waypoint>,
        register: bool,
        with_rules: bool,
    ) {
        let mut sim = Simulation::new();
        let rules = waypoint_action_rules();
        let house = register
            .then(|| register_waypoint_action_house(&mut sim, "Americans", house_country, (7, 8)));
        run_waypoint_action(
            137,
            token,
            trigger_country,
            &mut sim,
            &waypoints,
            with_rules.then_some(&rules),
        );
        if let Some(house) = house {
            assert_eq!(sim.houses[&house].alternate_base_center, (7, 8));
            assert_eq!(sim.houses[&house].base_center, Some((40, 50)));
        }
    }

    let valid = || {
        [(
            15,
            crate::map::waypoints::Waypoint {
                index: 15,
                rx: 93,
                ry: 106,
            },
        )]
        .into_iter()
        .collect()
    };
    assert_no_write(None, Some("Americans"), Some("P"), valid(), true, true);
    assert_no_write(
        Some("Americans"),
        Some("Americans"),
        Some("P"),
        valid(),
        true,
        false,
    );
    assert_no_write(Some("Americans"), None, Some("P"), valid(), true, true);
    assert_no_write(
        Some("Americans"),
        Some("Americans"),
        Some("P"),
        valid(),
        false,
        true,
    );
    assert_no_write(
        Some("Americans"),
        Some("Americans"),
        Some("15"),
        valid(),
        true,
        true,
    );
    assert_no_write(
        Some("Americans"),
        Some("Americans"),
        Some("P"),
        HashMap::new(),
        true,
        true,
    );
    assert_no_write(
        Some("Americans"),
        Some("Americans"),
        Some("P"),
        [(
            15,
            crate::map::waypoints::Waypoint {
                index: 15,
                rx: 0,
                ry: 0,
            },
        )]
        .into_iter()
        .collect(),
        true,
        true,
    );
    // Matching the House name is insufficient: only HouseState.country owns
    // the native Spring/Find_By_Country_Index resolution.
    assert_no_write(
        Some("Americans"),
        Some("Russians"),
        Some("P"),
        valid(),
        true,
        true,
    );
}

#[test]
fn action_138_clears_only_first_country_match_without_token_or_rng_use() {
    let mut sim = Simulation::new();
    let rules = waypoint_action_rules();
    let first = register_waypoint_action_house(&mut sim, "First", Some("Americans"), (93, 106));
    let second = register_waypoint_action_house(&mut sim, "Second", Some("Americans"), (122, 135));
    let rng_before = sim.scenario_rng.state();

    run_waypoint_action(
        138,
        Some("present but ignored"),
        Some("AMERICANS"),
        &mut sim,
        &HashMap::new(),
        Some(&rules),
    );

    assert_eq!(sim.houses[&first].alternate_base_center, (0, 0));
    assert_eq!(sim.houses[&second].alternate_base_center, (122, 135));
    assert_eq!(sim.houses[&first].base_center, Some((40, 50)));
    assert_eq!(sim.houses[&second].base_center, Some((40, 50)));
    assert_eq!(sim.scenario_rng.state(), rng_before);

    let mut no_match = Simulation::new();
    let house =
        register_waypoint_action_house(&mut no_match, "OnlyHouse", Some("Russians"), (105, 194));
    run_waypoint_action(
        138,
        None,
        Some("Americans"),
        &mut no_match,
        &HashMap::new(),
        Some(&rules),
    );
    assert_eq!(no_match.houses[&house].alternate_base_center, (105, 194));
}
