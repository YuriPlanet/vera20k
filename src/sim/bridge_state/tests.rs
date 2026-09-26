//! Tests for bridge runtime state construction and state transitions.

use super::*;
use crate::map::resolved_terrain::{
    BridgeDirection, BridgeLayer, ResolvedTerrainCell, ResolvedTerrainGrid, YR_CELL_LAND_TUNNEL,
};
use crate::map::tube_facts::{TubeFact, TubeId};
use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};

include!("record_native_tests.rs");
include!("gap_restamp_tests.rs");

#[test]
fn playfield_retail_high_bridge_walks_make_native_records_monotone() {
    assert!(HIGH_BRIDGE_WALK_DIRECTION
        .iter()
        .copied()
        .filter(|direction| *direction >= 0)
        .all(|direction| matches!(direction, 2 | 4)));
}

/// 5x1 grid: ground at (0,0), bridge at (1,0)-(3,0), ground at (4,0).
fn make_bridge_terrain() -> ResolvedTerrainGrid {
    const BRIDGE_SET_START: u16 = 100;
    const EAST_WALK_SLOT: i32 = 6;
    let mut cells = Vec::new();
    for rx in 0..5u16 {
        let on_bridge = (1..=3).contains(&rx);
        let is_record_tile = rx == 0 || rx == 3;
        cells.push(ResolvedTerrainCell {
            rx,
            ry: 0,
            source_tile_index: 0,
            source_sub_tile: 0,
            final_tile_index: if is_record_tile {
                i32::from(BRIDGE_SET_START) + EAST_WALK_SLOT
            } else {
                0
            },
            final_sub_tile: if is_record_tile { 4 } else { 0 },
            is_wood_bridge_repair_tile: false,
            level: 0,
            filled_clear: false,
            tileset_index: Some(0),
            land_type: 0,
            yr_cell_land_type: 0,
            slope_type: 0,
            template_height: 0,
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: TerrainClass::Clear,
            speed_costs: SpeedCostProfile::default(),
            is_water: false,
            is_cliff_like: false,
            height_in_pixels: 0,
            variant: 0,
            is_rough: false,
            is_road: false,
            accepts_smudge: false,
            allows_tiberium: false,
            has_ramp: false,
            canonical_ramp: None,
            ground_walk_blocked: on_bridge,
            terrain_object_blocks: false,
            terrain_object_occupation: None,
            overlay_blocks: false,
            overlay_zone_type: None,
            outside_playfield: false,
            zone_type: if on_bridge { 6 } else { 0 },
            base_ground_walk_blocked: false,
            base_build_blocked: false,
            base_land_type: 0,
            base_yr_cell_land_type: 0,
            base_terrain_class: Default::default(),
            base_speed_costs: Default::default(),
            build_blocked: on_bridge,
            has_bridge_deck: on_bridge,
            bridge_walkable: on_bridge,
            bridge_transition: rx == 1 || rx == 3,
            bridge_deck_level: if on_bridge { 4 } else { 0 },
            bridge_layer: None,
            bridge_facts: crate::map::bridge_facts::BridgeCellFacts {
                raw_flags: if on_bridge {
                    crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL
                } else {
                    0
                },
                ..Default::default()
            },
            tube_index: None,
            radar_left: [0, 0, 0],
            radar_right: [0, 0, 0],
            has_damaged_data: false,
            bridgehead_anchor_class_at_load: None,
        });
    }
    let mut terrain = ResolvedTerrainGrid::from_cells(5, 1, cells);
    terrain.test_set_high_bridge_set_starts(Some(BRIDGE_SET_START), None);
    terrain
}

fn make_low_bridge_terrain() -> ResolvedTerrainGrid {
    let mut tubes = Vec::new();
    let cells = make_bridge_terrain()
        .iter()
        .cloned()
        .map(|mut cell| {
            if (1..=3).contains(&cell.rx) {
                cell.bridge_deck_level = cell.level;
                cell.bridge_layer = Some(BridgeLayer {
                    overlay_id: 0x4a,
                    overlay_name: "LOBRDG01".to_string(),
                    deck_level: cell.level,
                    direction: BridgeDirection::Low,
                });
                cell.yr_cell_land_type = YR_CELL_LAND_TUNNEL;
                let tube_id = TubeId(tubes.len() as u16);
                tubes.push(TubeFact::auto_low_bridge((cell.rx, cell.ry), 2));
                cell.tube_index = Some(tube_id);
            }
            cell
        })
        .collect();
    ResolvedTerrainGrid::from_cells_with_tubes(5, 1, cells, tubes)
}

/// 5x1 grid: ground(0,0), bridgehead(1,0), body(2,0), bridgehead(3,0),
/// ground(4,0). Bridgeheads carry realistic resolved-terrain shape:
/// bridge_walkable=true, has_bridge_deck=false, transition=true,
/// bridge_deck_level=4. Body at (2,0) has has_bridge_deck=true.
fn make_bridge_with_bridgeheads_terrain() -> ResolvedTerrainGrid {
    let mut cells = Vec::new();
    for rx in 0..5u16 {
        let is_body = rx == 2;
        let is_head = rx == 1 || rx == 3;
        cells.push(ResolvedTerrainCell {
            rx,
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
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: TerrainClass::Clear,
            speed_costs: SpeedCostProfile::default(),
            is_water: false,
            is_cliff_like: false,
            height_in_pixels: 0,
            variant: 0,
            is_rough: false,
            is_road: false,
            accepts_smudge: false,
            allows_tiberium: false,
            has_ramp: false,
            canonical_ramp: None,
            ground_walk_blocked: is_body,
            terrain_object_blocks: false,
            terrain_object_occupation: None,
            overlay_blocks: false,
            overlay_zone_type: None,
            outside_playfield: false,
            zone_type: 0,
            base_ground_walk_blocked: false,
            base_build_blocked: false,
            base_land_type: 0,
            base_yr_cell_land_type: 0,
            base_terrain_class: Default::default(),
            base_speed_costs: Default::default(),
            build_blocked: is_body,
            has_bridge_deck: is_body,
            bridge_walkable: is_body || is_head,
            bridge_transition: is_head,
            bridge_deck_level: if is_body || is_head { 4 } else { 0 },
            bridge_layer: None,
            bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
            tube_index: None,
            radar_left: [0, 0, 0],
            radar_right: [0, 0, 0],
            has_damaged_data: false,
            bridgehead_anchor_class_at_load: None,
        });
    }
    ResolvedTerrainGrid::from_cells(5, 1, cells)
}

fn high_record_fixture(
    width: u16,
    height: u16,
    bridge_set_start: Option<u16>,
    wood_bridge_set_start: Option<u16>,
    mut configure: impl FnMut(&mut ResolvedTerrainCell),
) -> ResolvedTerrainGrid {
    let mut template = make_bridge_terrain().cell(4, 0).unwrap().clone();
    template.final_tile_index = 0;
    template.final_sub_tile = 0;
    template.has_bridge_deck = false;
    template.bridge_walkable = false;
    template.bridge_transition = false;
    template.bridge_deck_level = 0;
    template.bridge_facts = Default::default();

    let mut cells = Vec::with_capacity(usize::from(width) * usize::from(height));
    for ry in 0..height {
        for rx in 0..width {
            let mut cell = template.clone();
            cell.rx = rx;
            cell.ry = ry;
            configure(&mut cell);
            cells.push(cell);
        }
    }
    let mut terrain = ResolvedTerrainGrid::from_cells(width, height, cells);
    terrain.test_set_high_bridge_set_starts(bridge_set_start, wood_bridge_set_start);
    terrain
}

#[test]
fn bridgeheads_registered_with_bridgehead_role() {
    let state = BridgeRuntimeState::from_resolved_terrain(
        &make_bridge_with_bridgeheads_terrain(),
        true,
        300,
    );
    for rx in [1u16, 3] {
        let cell = state.cell(rx, 0).expect("bridgehead cell must register");
        assert!(matches!(cell.role, BridgeCellRole::Bridgehead));
        assert!(cell.deck_present, "bridgeheads carry deck_present=true");
        assert!(matches!(
            cell.damage_state,
            DamageState::Healthy { variant: 0 }
        ));
        assert!(cell.bridge_group_id.is_none());
        assert!(cell.anchor_span_id.is_none());
        assert!(cell.axis.is_none());
        assert_eq!(cell.deck_level, 4);
    }
}

#[test]
fn bridgehead_is_bridge_walkable_returns_true() {
    let state = BridgeRuntimeState::from_resolved_terrain(
        &make_bridge_with_bridgeheads_terrain(),
        true,
        300,
    );
    assert!(state.is_bridge_walkable(1, 0));
    assert!(state.is_bridge_walkable(3, 0));
}

#[test]
fn bridgehead_survives_body_cell_collapse() {
    let mut state = BridgeRuntimeState::from_resolved_terrain(
        &make_bridge_with_bridgeheads_terrain(),
        true,
        50,
    );
    if let Some(c) = state.cell_mut(2, 0) {
        c.damage_state = DamageState::Destroyed;
    }
    assert!(state.is_bridge_walkable(1, 0));
    assert!(state.is_bridge_walkable(3, 0));
    assert!(matches!(
        state.cell(1, 0).unwrap().damage_state,
        DamageState::Healthy { variant: 0 }
    ));
    assert!(matches!(
        state.cell(3, 0).unwrap().damage_state,
        DamageState::Healthy { variant: 0 }
    ));
    assert!(!state.is_bridge_walkable(2, 0));
}

#[test]
fn repaired_overlay_is_walkable_even_with_stale_destroyed_state() {
    let mut state = BridgeRuntimeState::default();
    state.test_seed_cell(
        2,
        2,
        BridgeRuntimeCell {
            deck_present: true,
            destroyable: true,
            deck_level: 4,
            bridge_group_id: Some(1),
            damage_state: DamageState::Destroyed,
            axis: Some(Axis::NS),
            role: BridgeCellRole::Body,
            anchor_span_id: Some(1),
            overlay_byte: 0xCD,
            bridgehead_anchor_class: BridgeheadAnchorClass::Variant0,
        },
    );

    assert_eq!(
        BridgeRuntimeState::effective_render_state(state.cell(2, 2).unwrap()),
        Some(DamageState::Healthy { variant: 0 })
    );
    assert!(state.is_bridge_walkable(2, 2));
}

#[test]
fn ns_walker_triple_writes_bridgehead_neighbors() {
    // BR-11: the HIGH NS walker triple-writes (this, north=(2,1),
    // south=(2,3)) UNCONDITIONALLY. Bridge destruction keys purely on the
    // overlay band with no per-cell role concept, so the bridgehead neighbors
    // in the triple must receive the destroy overlay and (on a final collapse)
    // a BlowUpBridge action — they are NOT left standing.
    let mut state = BridgeRuntimeState::default();
    state.test_seed_cell(
        2,
        2,
        BridgeRuntimeCell {
            deck_present: true,
            destroyable: true,
            deck_level: 4,
            bridge_group_id: Some(1),
            damage_state: DamageState::Healthy { variant: 0 },
            axis: Some(Axis::NS),
            role: BridgeCellRole::Body,
            anchor_span_id: Some(1),
            // 0xD3 ∈ [0xD3..=0xD5] → final-collapse case: triple writes 0xE7.
            overlay_byte: 0xD3,
            bridgehead_anchor_class: BridgeheadAnchorClass::Variant0,
        },
    );
    for ry in [1u16, 3] {
        state.test_seed_cell(
            2,
            ry,
            BridgeRuntimeCell {
                deck_present: true,
                destroyable: true,
                deck_level: 4,
                bridge_group_id: None,
                damage_state: DamageState::Healthy { variant: 0 },
                axis: None,
                role: BridgeCellRole::Bridgehead,
                anchor_span_id: None,
                overlay_byte: 0,
                bridgehead_anchor_class: BridgeheadAnchorClass::Variant0,
            },
        );
    }
    let terrain = crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(3, 4, Vec::new());

    let outcome = state.destroy_bridge_walker_ns_high(2, 2, &terrain);

    // Every triple cell — including the two bridgeheads — gets the 0xE7
    // destroy overlay and a Destroyed damage_state.
    for (rx, ry) in [(2u16, 1u16), (2, 2), (2, 3)] {
        let c = state.cell(rx, ry).expect("triple cell present");
        assert_eq!(
            c.overlay_byte, 0xE7,
            "({rx},{ry}) gets the destroy overlay band"
        );
        assert_eq!(
            c.damage_state,
            DamageState::Destroyed,
            "({rx},{ry}) marked Destroyed"
        );
    }
    // The bridgehead cells keep their role tag (role is derived/internal, not
    // authoritative) but are now part of the collapse + BlowUpBridge cascade.
    for ry in [1u16, 3] {
        assert!(matches!(
            state.cell(2, ry).unwrap().role,
            BridgeCellRole::Bridgehead
        ));
    }
    match outcome {
        StateOutcome::Collapsed {
            binary_success,
            destroyed_cells,
            set_bridge_direction,
            zones_dirty,
            radar_cells,
            ..
        } => {
            assert!(binary_success);
            assert!(zones_dirty, "final collapse marks zones dirty");
            for pos in [(2u16, 1u16), (2, 2), (2, 3)] {
                assert!(destroyed_cells.contains(&pos), "{pos:?} in destroyed_cells");
                // BR-16: every triple cell the walker wrote is minimap-dirty.
                assert!(radar_cells.contains(&pos), "{pos:?} in radar_cells");
            }
            assert_eq!(
                set_bridge_direction.actions.len(),
                3,
                "one BlowUpBridge per triple cell, bridgeheads included"
            );
            let blown: Vec<(u16, u16)> = set_bridge_direction
                .actions
                .iter()
                .map(|(pos, _, _)| *pos)
                .collect();
            for pos in [(2u16, 1u16), (2, 2), (2, 3)] {
                assert!(blown.contains(&pos), "{pos:?} has a BlowUpBridge action");
                assert!(
                    set_bridge_direction
                        .actions
                        .iter()
                        .all(|(_, _, action)| matches!(
                            action,
                            crate::sim::bridge_specs::CellAction::BlowUpBridge
                        ))
                );
            }
        }
        other => panic!("expected Collapsed, got {other:?}"),
    }
}

#[test]
fn bridge_runtime_initializes_intact_groups() {
    let state = BridgeRuntimeState::from_resolved_terrain(&make_bridge_terrain(), true, 300);
    let cell = state.cell(1, 0).expect("bridge cell");
    assert!(cell.deck_present);
    assert!(matches!(cell.damage_state, DamageState::Healthy { .. }));
    assert_eq!(cell.deck_level, 4);
    assert_eq!(cell.bridge_group_id, Some(1));
    assert!(state.cell(0, 0).is_none());
}

#[test]
fn marking_group_cells_destroyed_makes_them_unwalkable() {
    // Direct mutation replaces the legacy `apply_damage`. The
    // orchestrator's walker performs the per-cell damage-state
    // transitions through `body_cell_advance_state`; this lower-
    // level test just asserts the read paths (is_bridge_walkable)
    // honor `DamageState::Destroyed`.
    let mut state = BridgeRuntimeState::from_resolved_terrain(&make_bridge_terrain(), true, 50);
    for (rx, ry) in [(1u16, 0u16), (2, 0), (3, 0)] {
        if let Some(cell) = state.cell_mut(rx, ry) {
            cell.damage_state = DamageState::Destroyed;
        }
    }
    assert!(!state.is_bridge_walkable(1, 0));
    assert!(!state.is_bridge_walkable(2, 0));
    assert!(!state.is_bridge_walkable(3, 0));
    assert_eq!(
        state.cell(1, 0).map(|c| c.damage_state),
        Some(DamageState::Destroyed)
    );
}

#[test]
fn indestructible_bridge_outer_gate_is_clear() {
    // The orchestrator's outer gate is `is_destroyable()`. When a
    // bridge runtime is built with `destroyable=false`, the gate
    // closes and the dispatcher bails before any path fires.
    let state = BridgeRuntimeState::from_resolved_terrain(&make_bridge_terrain(), false, 50);
    assert!(!state.is_destroyable());
    assert!(state.is_bridge_walkable(1, 0));
}

#[test]
fn bridge_endpoints_detected() {
    let state = BridgeRuntimeState::from_resolved_terrain(&make_bridge_terrain(), true, 300);
    let records = state.endpoint_records();
    assert_eq!(
        records.len(),
        1,
        "should have exactly one bridge endpoint record"
    );
    let rec = &records[0];
    assert!(rec.active);
    assert_eq!(rec.group_id, 1);
    assert_eq!(rec.bridge_kind, BridgeRecordKind::High);
    assert_eq!(rec.endpoint_a, (0, 0));
    assert_eq!(rec.endpoint_b, (3, 0));
}

#[test]
fn gsi_04_12_topology_concrete_and_wood_membership_build_records() {
    let terrain = high_record_fixture(4, 2, Some(100), Some(200), |cell| {
        let base = if cell.ry == 0 { 100 } else { 200 };
        if cell.rx == 0 || cell.rx == 2 {
            cell.final_tile_index = base + 6;
            cell.final_sub_tile = 4;
        }
        if cell.rx == 1 {
            cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
        }
    });

    let state = BridgeRuntimeState::from_resolved_terrain(&terrain, true, 300);
    let records = state.endpoint_records();
    assert_eq!(records.len(), 2);
    assert_eq!(
        (records[0].endpoint_a, records[0].endpoint_b),
        ((0, 0), (2, 0))
    );
    assert_eq!(
        (records[1].endpoint_a, records[1].endpoint_b),
        ((0, 1), (2, 1))
    );
    assert!(records.iter().all(|record| record.active));
}

#[test]
fn gsi_04_12_topology_uses_final_subtile_and_requires_active_set_base() {
    let exact = high_record_fixture(4, 1, Some(100), None, |cell| {
        cell.source_sub_tile = 1;
        cell.template_height = 9;
        if cell.rx == 0 || cell.rx == 2 {
            cell.final_tile_index = 106;
            cell.final_sub_tile = 4;
        }
    });
    assert_eq!(
        BridgeRuntimeState::from_resolved_terrain(&exact, true, 300)
            .endpoint_records()
            .len(),
        1
    );

    let wrong_subtile = high_record_fixture(4, 1, Some(100), None, |cell| {
        if cell.rx == 0 || cell.rx == 2 {
            cell.final_tile_index = 106;
            cell.final_sub_tile = if cell.rx == 0 { 3 } else { 4 };
        }
    });
    assert!(
        BridgeRuntimeState::from_resolved_terrain(&wrong_subtile, true, 300)
            .endpoint_records()
            .is_empty()
    );

    let no_base = high_record_fixture(4, 1, None, None, |cell| {
        if cell.rx == 0 || cell.rx == 2 {
            cell.final_tile_index = 106;
            cell.final_sub_tile = 4;
        }
    });
    assert!(
        BridgeRuntimeState::from_resolved_terrain(&no_base, true, 300)
            .endpoint_records()
            .is_empty()
    );
}

#[test]
fn gsi_04_12_topology_structural_gap_preserves_intact_plain_gap_clears_it() {
    let make = |structural: bool| {
        high_record_fixture(4, 1, Some(100), None, |cell| {
            if cell.rx == 0 || cell.rx == 2 {
                cell.final_tile_index = 106;
                cell.final_sub_tile = 4;
            }
            if structural && cell.rx == 1 {
                cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
            }
        })
    };

    let intact = BridgeRuntimeState::from_resolved_terrain(&make(true), true, 300);
    assert!(intact.endpoint_records()[0].active);
    assert_ne!(intact.endpoint_records()[0].group_id, 0);

    let mut broken = BridgeRuntimeState::from_resolved_terrain(&make(false), true, 300);
    assert!(!broken.endpoint_records()[0].active);
    assert_eq!(broken.endpoint_records()[0].group_id, 0);
    broken.refresh_endpoint_active_flags();
    assert!(!broken.endpoint_records()[0].active);

    let mixed_gap = high_record_fixture(5, 1, Some(100), None, |cell| {
        if cell.rx == 0 || cell.rx == 3 {
            cell.final_tile_index = 106;
            cell.final_sub_tile = 4;
        }
        if cell.rx == 2 {
            cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
        }
    });
    let mut mixed = BridgeRuntimeState::from_resolved_terrain(&mixed_gap, true, 300);
    assert!(!mixed.endpoint_records()[0].active);
    assert_eq!(mixed.endpoint_records()[0].group_id, 0);
    assert!(mixed.cell(2, 0).unwrap().bridge_group_id.is_some());
    mixed.refresh_endpoint_active_flags();
    assert!(!mixed.endpoint_records()[0].active);
}

#[test]
fn gsi_04_12_topology_native_diagonal_scan_order_keeps_each_start() {
    let terrain = high_record_fixture(4, 4, Some(100), None, |cell| match (cell.rx, cell.ry) {
        (0, 2) => {
            cell.final_tile_index = 106;
            cell.final_sub_tile = 4;
        }
        (2, 0) | (2, 2) => {
            cell.final_tile_index = 111;
            cell.final_sub_tile = 2;
        }
        _ => {}
    });

    let state = BridgeRuntimeState::from_resolved_terrain(&terrain, true, 300);
    let endpoints: Vec<_> = state
        .endpoint_records()
        .iter()
        .map(|record| (record.endpoint_a, record.endpoint_b))
        .collect();
    assert_eq!(endpoints, vec![((0, 2), (2, 2)), ((2, 0), (2, 2))]);
}

#[test]
fn automatic_shells_do_not_invent_bridge_records() {
    let state = BridgeRuntimeState::from_resolved_terrain(&make_low_bridge_terrain(), true, 300);
    let records = state.endpoint_records();
    assert!(records.is_empty(), "same-cell Tube exits fail native strict ordinal order");
}

#[test]
fn low_bridge_tube_record_requires_opposite_neighbors() {
    let mut terrain = make_low_bridge_terrain();
    let cell = terrain.cell_mut(3, 0).expect("right low bridge cell");
    cell.tube_index = None;
    cell.yr_cell_land_type = 0;

    let state = BridgeRuntimeState::from_resolved_terrain(&terrain, true, 300);
    assert!(
        state.endpoint_records().is_empty(),
        "low bridge records require the verified opposite low-neighbor pattern"
    );
}

#[test]
fn bridge_destruction_deactivates_endpoints() {
    // Endpoint deactivation is now driven by the orchestrator's
    // `refresh_bridge_zones_if_dirty`, which calls
    // `refresh_endpoint_active_flags` whenever a walker / state-machine
    // collapse marks `zones_dirty`. This in-module test exercises the
    // deactivation logic in isolation: mutate cells to Destroyed (the
    // dispatcher's terminal effect) and call the refresh helper
    // directly. The full pipeline is covered by world-level integration
    // tests in `world_tests.rs`.
    let mut state = BridgeRuntimeState::from_resolved_terrain(&make_bridge_terrain(), true, 50);
    // Pre-condition: endpoint exists and is active.
    let records = state.endpoint_records();
    assert_eq!(records.len(), 1);
    assert!(records[0].active);
    let group_id = records[0].group_id;

    // Mark every cell of the group destroyed (simulates a final-stage
    // walker cascade landing on the entire group).
    for (rx, ry) in [(1u16, 0u16), (2, 0), (3, 0)] {
        if let Some(c) = state.cell_mut(rx, ry) {
            c.damage_state = DamageState::Destroyed;
        }
    }
    state.refresh_endpoint_active_flags();

    let records = state.endpoint_records();
    assert!(
        !records[0].active,
        "endpoint of destroyed group {group_id} must deactivate"
    );
    assert_eq!(records[0].bridge_kind, BridgeRecordKind::High);
}

#[test]
fn refresh_endpoint_active_flags_deactivates_on_first_destroyed_cell() {
    // Per the new state-machine semantic: a single destroyed cell in a
    // group severs the bridge — the endpoint flips inactive immediately,
    // not just when the entire group is destroyed.
    let mut state = BridgeRuntimeState::from_resolved_terrain(&make_bridge_terrain(), true, 50);
    assert!(state.endpoint_records()[0].active);

    // Destroy only ONE cell of the 3-cell group.
    if let Some(c) = state.cell_mut(2, 0) {
        c.damage_state = DamageState::Destroyed;
    }
    state.refresh_endpoint_active_flags();

    assert!(
        !state.endpoint_records()[0].active,
        "first destroyed cell must already deactivate the endpoint"
    );
}

#[test]
fn refresh_endpoint_active_flags_leaves_intact_groups_active() {
    // No destroyed cells anywhere — refresh must not flip anything.
    let mut state = BridgeRuntimeState::from_resolved_terrain(&make_bridge_terrain(), true, 50);
    state.refresh_endpoint_active_flags();
    assert!(state.endpoint_records()[0].active);
}

#[test]
fn refresh_endpoint_active_flags_reactivates_after_repair() {
    // BR-08: re-activation must be keyed on the authoritative overlay byte
    // (effective_render_state), NOT damage_state. The real engineer-repair path
    // restores the overlay byte to a healthy band but leaves damage_state STALE
    // at Destroyed (the original engine never resets the body damage byte). This
    // test mirrors that exactly, so it FAILS if the recompute is keyed on
    // damage_state and only passes under the overlay-derived predicate.
    let mut state = BridgeRuntimeState::from_resolved_terrain(&make_bridge_terrain(), true, 50);
    assert!(state.endpoint_records()[0].active);

    // Collapse (2,0) the way the body-SM does (BR-09): destroyed overlay + state.
    {
        let c = state.cell_mut(2, 0).unwrap();
        c.damage_state = DamageState::Destroyed;
        c.overlay_byte = 0xFF;
    }
    state.refresh_endpoint_active_flags();
    assert!(
        !state.endpoint_records()[0].active,
        "destroyed cell must deactivate the record"
    );

    // Repair the REAL way: restore the overlay byte to a healthy body value,
    // leaving damage_state stale at Destroyed.
    {
        let c = state.cell_mut(2, 0).unwrap();
        c.overlay_byte = 0xCD; // healthy high-bridge body overlay (variant 0)
        assert!(
            matches!(c.damage_state, DamageState::Destroyed),
            "repair leaves damage_state stale (matches the original engine)"
        );
    }
    state.refresh_endpoint_active_flags();
    assert!(
        state.endpoint_records()[0].active,
        "repaired (overlay-restored, damage_state stale) group must re-activate"
    );
}

#[test]
fn direction_offsets_match_compass() {
    assert_eq!(Direction::N.offset(), (0, -1));
    assert_eq!(Direction::E.offset(), (1, 0));
    assert_eq!(Direction::S.offset(), (0, 1));
    assert_eq!(Direction::W.offset(), (-1, 0));
}

#[test]
fn direction_opposite_is_idempotent() {
    for dir in [
        Direction::N,
        Direction::NE,
        Direction::E,
        Direction::SE,
        Direction::S,
        Direction::SW,
        Direction::W,
        Direction::NW,
    ] {
        assert_eq!(dir.opposite().opposite(), dir);
    }
}

#[test]
fn direction_opposite_pairs() {
    assert_eq!(Direction::N.opposite(), Direction::S);
    assert_eq!(Direction::E.opposite(), Direction::W);
    assert_eq!(Direction::NE.opposite(), Direction::SW);
    assert_eq!(Direction::SE.opposite(), Direction::NW);
}

fn make_test_span() -> AnchorSpan {
    AnchorSpan {
        id: 1,
        anchor: (5, 5),
        cells: [
            Some((5, 5)), // slot 0 = anchor
            Some((6, 5)), // slot 1 = +E × 1
            Some((7, 5)), // slot 2 = +E × 2
            Some((8, 5)), // slot 3 = +E × 3 (FLAG ONLY)
            Some((4, 5)), // slot 4 = -E × 1 = +W × 1
            None,         // slot 5 = optional W-direction fixed offset
        ],
        axis: Axis::NS,
        direction: Direction::E,
        damage_state: DamageState::Healthy { variant: 0 },
        bridge_group_id: 1,
    }
}

#[test]
fn anchor_span_blow_up_cells_excludes_slot_3() {
    let span = make_test_span();
    let cells: Vec<_> = span.blow_up_cells().collect();
    // Cells 1, 2, 3, 5 in 1-indexed numbering = our slots 0, 1, 2, 4.
    // NOT slot 3 (cell 4, flag-only).
    assert_eq!(cells, vec![(5, 5), (6, 5), (7, 5), (4, 5)]);
}

#[test]
fn anchor_span_iter_cells_skips_none() {
    let span = make_test_span();
    let count = span.iter_cells().count();
    assert_eq!(count, 5); // 6 slots, 1 None
}

#[test]
fn walk_anchor_pattern_dir_w_extra_slot_is_anchor_plus_2e() {
    // BR-39: the dir-W anchor's extra cell (slot 5) is `anchor + 2·E`, one cell
    // BEYOND the slot-4 opposite cell — not a duplicate of slot 4. The previous
    // `+1` aliased slot 4, so in the pass-2 tagging loop (slot 4 -> Tail, else
    // -> Body, keyed on slot INDEX) the alias (a) left the true extra cell
    // untagged and (b) overwrote the opposite cell's Tail role with Body via
    // last-write-wins. Distinct slots fix both.
    let span = walk_anchor_pattern(1, (5, 5), Axis::EW, Direction::W, 1, 12, 12);
    assert_eq!(span.cells[0], Some((5, 5)), "slot 0 = anchor");
    assert_eq!(span.cells[1], Some((4, 5)), "slot 1 = +W×1");
    assert_eq!(span.cells[2], Some((3, 5)), "slot 2 = +W×2");
    assert_eq!(span.cells[3], Some((2, 5)), "slot 3 = +W×3");
    assert_eq!(span.cells[4], Some((6, 5)), "slot 4 = opposite (+E)×1");
    assert_eq!(
        span.cells[5],
        Some((7, 5)),
        "slot 5 = anchor + 2·E, distinct from slot 4"
    );
    assert_ne!(
        span.cells[4], span.cells[5],
        "extra cell must not alias the slot-4 opposite cell"
    );
}

#[test]
fn anchor_spans_empty_when_bridge_layer_none() {
    // The default test fixture sets bridge_layer: None, so pass 2 emits no
    // anchor spans. Verifies the constructor still wires everything else.
    let state = BridgeRuntimeState::from_resolved_terrain(&make_bridge_terrain(), true, 300);
    assert!(state.anchor_spans().is_empty());
    let cell = state.cell(1, 0).expect("bridge cell");
    assert!(cell.deck_present);
    assert!(matches!(cell.damage_state, DamageState::Healthy { .. }));
}

#[test]
fn stamped_high_bridge_facts_create_anchor_span_without_bridge_layer() {
    use crate::map::bridge_facts::{
        BridgeCellFacts, BridgeStampFamily, stamp_set_bridge_direction,
    };

    let width = 10u16;
    let height = 10u16;
    let mut facts = vec![BridgeCellFacts::default(); width as usize * height as usize];
    stamp_set_bridge_direction(
        &mut facts,
        width,
        height,
        (5, 5),
        BridgeStampFamily::Nesw,
        0,
        true,
    );
    facts[5usize * width as usize + 5].overlay_id = Some(0x18);

    let mut cells = Vec::new();
    for ry in 0..height {
        for rx in 0..width {
            let idx = ry as usize * width as usize + rx as usize;
            let structural = facts[idx].has_structural_bridge();
            cells.push(ResolvedTerrainCell {
                rx,
                ry,
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
                render_offset_x: 0,
                render_offset_y: 0,
                terrain_class: TerrainClass::Clear,
                speed_costs: SpeedCostProfile::default(),
                is_water: false,
                is_cliff_like: false,
                height_in_pixels: 0,
                variant: 0,
                is_rough: false,
                is_road: false,
                accepts_smudge: false,
                allows_tiberium: false,
                has_ramp: false,
                canonical_ramp: None,
                ground_walk_blocked: structural,
                terrain_object_blocks: false,
                terrain_object_occupation: None,
                overlay_blocks: false,
                overlay_zone_type: None,
                outside_playfield: false,
                zone_type: 0,
                base_ground_walk_blocked: false,
                base_build_blocked: false,
                base_land_type: 0,
                base_yr_cell_land_type: 0,
                base_terrain_class: Default::default(),
                base_speed_costs: Default::default(),
                build_blocked: structural,
                has_bridge_deck: false,
                bridge_walkable: structural,
                bridge_transition: facts[idx].has_transition_flag(),
                bridge_deck_level: if structural { 4 } else { 0 },
                bridge_layer: None,
                bridge_facts: facts[idx],
                tube_index: None,
                radar_left: [0, 0, 0],
                radar_right: [0, 0, 0],
                has_damaged_data: false,
                bridgehead_anchor_class_at_load: None,
            });
        }
    }

    let terrain = ResolvedTerrainGrid::from_cells(width, height, cells);
    let state = BridgeRuntimeState::from_resolved_terrain(&terrain, true, 1500);

    assert_eq!(state.anchor_spans().len(), 1);
    let span = state.anchor_spans().values().next().expect("span");
    assert_eq!(span.anchor, (5, 5));
    assert_eq!(span.axis, Axis::NS);
    assert_eq!(span.direction, Direction::N);
    assert_eq!(
        span.cells,
        [
            Some((5, 5)),
            Some((5, 4)),
            Some((5, 3)),
            Some((5, 2)),
            Some((5, 6)),
            None,
        ]
    );
    assert_eq!(state.cell(5, 5).expect("anchor").overlay_byte, 0x18);
    assert!(matches!(
        state.cell(5, 5).expect("anchor").role,
        BridgeCellRole::Anchor
    ));
    assert!(state.cell(5, 4).is_some());
    assert!(state.cell(5, 3).is_some());
    assert!(state.cell(5, 6).is_some());
    assert!(
        state.cell(5, 2).is_none(),
        "slot 3 is flag-only and must not create a runtime bridge cell"
    );
}

#[test]
fn bridge_runtime_state_snapshot_round_trip() {
    let state = BridgeRuntimeState::from_resolved_terrain(&make_bridge_terrain(), true, 1500);
    let json = serde_json::to_string(&state).expect("serialize");
    let restored: BridgeRuntimeState = serde_json::from_str(&json).expect("deserialize");
    // Compare cell-by-cell across the full grid.
    for (rx, ry) in [(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)] {
        assert_eq!(
            state.cell(rx, ry),
            restored.cell(rx, ry),
            "cell ({rx},{ry})"
        );
    }
    // Compare anchor spans.
    assert_eq!(state.anchor_spans().len(), restored.anchor_spans().len());
    for (id, span) in state.anchor_spans() {
        assert_eq!(restored.anchor_span(*id), Some(span));
    }
    // is_bridge_walkable behavior parity.
    for (rx, ry) in [(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)] {
        assert_eq!(
            state.is_bridge_walkable(rx, ry),
            restored.is_bridge_walkable(rx, ry),
            "walkability ({rx},{ry})"
        );
    }
}

#[test]
fn overlay_byte_populated_at_map_load() {
    // make_bridge_terrain in this file creates a 5x1 strip; the constructor
    // populates overlay_byte from bridge_layer.overlay_id (or 0 if none).
    let state = BridgeRuntimeState::from_resolved_terrain(&make_bridge_terrain(), true, 1500);
    // Field is reachable on every populated bridge cell; type is u8.
    for (_, cell) in state.iter_cells() {
        let _byte: u8 = cell.overlay_byte;
    }
}

#[test]
fn overlay_byte_round_trips_via_snapshot() {
    let state = BridgeRuntimeState::from_resolved_terrain(&make_bridge_terrain(), true, 1500);
    let json = serde_json::to_string(&state).expect("serialize");
    let restored: BridgeRuntimeState = serde_json::from_str(&json).expect("deserialize");
    for ((rx, ry), cell) in state.iter_cells() {
        let r = restored.cell(rx, ry).expect("restored cell present");
        assert_eq!(
            cell.overlay_byte, r.overlay_byte,
            "overlay_byte at ({rx},{ry})"
        );
    }
}

#[test]
fn test_seed_cell_grows_grid_to_fit() {
    let mut state = BridgeRuntimeState::default();
    let cell = BridgeRuntimeCell {
        deck_present: true,
        destroyable: true,
        deck_level: 0,
        bridge_group_id: Some(1),
        damage_state: DamageState::Healthy { variant: 0 },
        axis: Some(Axis::NS),
        role: BridgeCellRole::Anchor,
        anchor_span_id: Some(1),
        overlay_byte: 0x18,
        bridgehead_anchor_class: BridgeheadAnchorClass::Variant0,
    };
    state.test_seed_cell(5, 5, cell);
    let read = state.cell(5, 5).expect("seeded cell present");
    assert_eq!(read.overlay_byte, 0x18);
    assert_eq!(read.role, BridgeCellRole::Anchor);
}

#[test]
fn cell_mut_writes_visible_through_cell_read() {
    let mut state = BridgeRuntimeState::default();
    let cell = BridgeRuntimeCell {
        deck_present: true,
        destroyable: true,
        deck_level: 0,
        bridge_group_id: Some(1),
        damage_state: DamageState::Healthy { variant: 0 },
        axis: Some(Axis::NS),
        role: BridgeCellRole::Anchor,
        anchor_span_id: Some(1),
        overlay_byte: 0x18,
        bridgehead_anchor_class: BridgeheadAnchorClass::Variant0,
    };
    state.test_seed_cell(2, 2, cell);
    state.cell_mut(2, 2).unwrap().overlay_byte = 0xD2;
    assert_eq!(state.cell(2, 2).unwrap().overlay_byte, 0xD2);
}

#[test]
fn damage_state_to_byte_ns_axis() {
    assert_eq!(
        DamageState::Healthy { variant: 0 }.to_state_byte(Axis::NS),
        0
    );
    assert_eq!(
        DamageState::Healthy { variant: 3 }.to_state_byte(Axis::NS),
        3
    );
    assert_eq!(
        DamageState::Healthy { variant: 5 }.to_state_byte(Axis::NS),
        5
    );
    assert_eq!(DamageState::Damaged.to_state_byte(Axis::NS), 6);
    assert_eq!(DamageState::PartialCollapseA.to_state_byte(Axis::NS), 7);
    assert_eq!(DamageState::PartialCollapseB.to_state_byte(Axis::NS), 8);
    assert_eq!(DamageState::Destroyed.to_state_byte(Axis::NS), 0);
}

#[test]
fn damage_state_to_byte_ew_axis() {
    assert_eq!(
        DamageState::Healthy { variant: 0 }.to_state_byte(Axis::EW),
        9
    );
    assert_eq!(
        DamageState::Healthy { variant: 5 }.to_state_byte(Axis::EW),
        14
    );
    assert_eq!(DamageState::Damaged.to_state_byte(Axis::EW), 0xF);
    assert_eq!(DamageState::PartialCollapseA.to_state_byte(Axis::EW), 0x11);
    assert_eq!(DamageState::PartialCollapseB.to_state_byte(Axis::EW), 0x10);
    assert_eq!(DamageState::Destroyed.to_state_byte(Axis::EW), 0);
}

#[test]
fn damage_state_to_byte_clamps_healthy_variant() {
    // Variant > 5 is invalid input; should clamp to 5 (max defined healthy).
    assert_eq!(
        DamageState::Healthy { variant: 7 }.to_state_byte(Axis::NS),
        5
    );
    assert_eq!(
        DamageState::Healthy { variant: 10 }.to_state_byte(Axis::EW),
        14
    );
}

#[test]
fn damage_state_from_byte_ns_range() {
    assert_eq!(
        DamageState::from_state_byte(0),
        Some(DamageState::Healthy { variant: 0 })
    );
    assert_eq!(
        DamageState::from_state_byte(3),
        Some(DamageState::Healthy { variant: 3 })
    );
    assert_eq!(
        DamageState::from_state_byte(5),
        Some(DamageState::Healthy { variant: 5 })
    );
    assert_eq!(DamageState::from_state_byte(6), Some(DamageState::Damaged));
    assert_eq!(
        DamageState::from_state_byte(7),
        Some(DamageState::PartialCollapseA)
    );
    assert_eq!(
        DamageState::from_state_byte(8),
        Some(DamageState::PartialCollapseB)
    );
}

#[test]
fn damage_state_from_byte_ew_range() {
    assert_eq!(
        DamageState::from_state_byte(9),
        Some(DamageState::Healthy { variant: 0 })
    );
    assert_eq!(
        DamageState::from_state_byte(14),
        Some(DamageState::Healthy { variant: 5 })
    );
    assert_eq!(
        DamageState::from_state_byte(0xF),
        Some(DamageState::Damaged)
    );
    assert_eq!(
        DamageState::from_state_byte(0x10),
        Some(DamageState::PartialCollapseB)
    );
    assert_eq!(
        DamageState::from_state_byte(0x11),
        Some(DamageState::PartialCollapseA)
    );
}

#[test]
fn damage_state_from_byte_out_of_range_returns_none() {
    assert_eq!(DamageState::from_state_byte(0x12), None);
    assert_eq!(DamageState::from_state_byte(0xFF), None);
}

#[test]
fn render_state_byte_strips_healthy_variant() {
    assert_eq!(
        DamageState::Healthy { variant: 0 }.render_state_byte(Axis::NS),
        0
    );
    assert_eq!(
        DamageState::Healthy { variant: 5 }.render_state_byte(Axis::NS),
        0
    );
    assert_eq!(
        DamageState::Healthy { variant: 0 }.render_state_byte(Axis::EW),
        9
    );
    assert_eq!(
        DamageState::Healthy { variant: 5 }.render_state_byte(Axis::EW),
        9
    );
    assert_eq!(DamageState::Damaged.render_state_byte(Axis::NS), 6);
    assert_eq!(DamageState::Damaged.render_state_byte(Axis::EW), 0xF);
    assert_eq!(DamageState::Destroyed.render_state_byte(Axis::NS), 0);
}

#[test]
fn damage_state_round_trip_for_each_variant_per_axis() {
    // For every (axis × variant) pair where Destroyed is excluded (it's the
    // ambiguous post-collapse state).
    for axis in [Axis::NS, Axis::EW] {
        for state in [
            DamageState::Healthy { variant: 0 },
            DamageState::Healthy { variant: 5 },
            DamageState::Damaged,
            DamageState::PartialCollapseA,
            DamageState::PartialCollapseB,
        ] {
            let byte = state.to_state_byte(axis);
            let decoded = DamageState::from_state_byte(byte)
                .expect("decode succeeds for byte produced by encode");
            assert_eq!(decoded, state, "round-trip {state:?} via {axis:?}");
        }
    }
}

fn make_body_driver_test_state() -> BridgeRuntimeState {
    // Uses test_seed_cell + test_seed_anchor_span from Task 1 Step 5.
    // Layout for the body-driver tests:
    //   (5,5)  → anchor cell, axis NS, anchor_span_id=1
    //   (4,5), (6,5) → perpendicular anchor partners (axis NS, separate
    //                  span_id) — UpdateRamp_*A walks E, _*B walks W from
    //                  (5,5), so these are the wrappers' targets.
    //   (5,4)  → non-anchor body cell, anchor_span_id=1 — exercises the
    //                  "follow to anchor" path in the driver.
    // Slots (7,5), (8,5) are seeded so collapse can clear the full
    // AnchorSpan overlay-byte surface, not just the anchor.
    let mut state = BridgeRuntimeState::default();

    let healthy_template = BridgeRuntimeCell {
        deck_present: true,
        destroyable: true,
        deck_level: 0,
        bridge_group_id: Some(1),
        damage_state: DamageState::Healthy { variant: 0 },
        axis: Some(Axis::NS),
        role: BridgeCellRole::Anchor,
        anchor_span_id: Some(1),
        overlay_byte: 0x18,
        bridgehead_anchor_class: BridgeheadAnchorClass::Variant0,
    };

    // Anchor at (5,5).
    state.test_seed_cell(5, 5, healthy_template);

    // Perpendicular anchor partners. They are anchors of their own spans
    // (binary's `+0x80` flag is set), so use anchor_span_id=2.
    let perp = BridgeRuntimeCell {
        anchor_span_id: Some(2),
        ..healthy_template
    };
    state.test_seed_cell(4, 5, perp);
    state.test_seed_cell(6, 5, perp);

    // Non-anchor body cell with anchor_span_id=1 — used by
    // body_driver_non_anchor_body_cell_follows_to_anchor.
    state.test_seed_cell(
        5,
        4,
        BridgeRuntimeCell {
            role: BridgeCellRole::Body,
            ..healthy_template
        },
    );

    for rx in [7, 8] {
        state.test_seed_cell(
            rx,
            5,
            BridgeRuntimeCell {
                role: BridgeCellRole::Body,
                ..healthy_template
            },
        );
    }

    // AnchorSpan registry entry. The driver looks up by anchor_span_id
    // and reads `span.anchor` to resolve. Slot positions beyond (5,5),
    // (4,5), (6,5) aren't seeded as cells because the driver doesn't
    // touch them in the body-cell branch.
    state.test_seed_anchor_span(AnchorSpan {
        id: 1,
        anchor: (5, 5),
        cells: [
            Some((5, 5)),
            Some((6, 5)),
            Some((7, 5)),
            Some((8, 5)),
            Some((4, 5)),
            None,
        ],
        axis: Axis::NS,
        direction: Direction::E,
        damage_state: DamageState::Healthy { variant: 0 },
        bridge_group_id: 1,
    });

    state
}

#[test]
fn body_driver_anchor_healthy_advances_to_damaged_returns_absorbed() {
    let mut state = make_body_driver_test_state();
    let outcome = state.body_cell_advance_state(5, 5, true, &mut flood_fill_terrain(20, 20, 0));
    assert!(matches!(outcome, StateOutcome::Absorbed { .. }));
    assert_eq!(state.cell(5, 5).unwrap().damage_state, DamageState::Damaged);
}

#[test]
fn body_driver_non_anchor_body_cell_follows_to_anchor() {
    let mut state = make_body_driver_test_state();
    // Damage on a body cell, not the anchor.
    let outcome = state.body_cell_advance_state(5, 4, true, &mut flood_fill_terrain(20, 20, 0));
    assert!(matches!(outcome, StateOutcome::Absorbed { .. }));
    // Anchor's damage_state advanced, not the input body cell's.
    assert_eq!(state.cell(5, 5).unwrap().damage_state, DamageState::Damaged);
    assert_eq!(
        state.cell(5, 4).unwrap().damage_state,
        DamageState::Healthy { variant: 0 }
    );
}

#[test]
fn body_driver_damaged_anchor_collapses_and_emits_set_bridge_direction() {
    let mut state = make_body_driver_test_state();
    state.cell_mut(5, 5).unwrap().damage_state = DamageState::Damaged;
    let outcome = state.body_cell_advance_state(5, 5, true, &mut flood_fill_terrain(20, 20, 0));
    match outcome {
        StateOutcome::Collapsed {
            binary_success,
            destroyed_cells,
            set_bridge_direction,
            adjacent_bridges_dirty,
            zones_dirty,
            radar_cells,
            ..
        } => {
            assert!(binary_success);
            assert!(destroyed_cells.contains(&(5, 5)));
            // BR-16: the collapsed anchor is fed to the minimap radar channel.
            assert!(radar_cells.contains(&(5, 5)));
            // 4 BlowUpBridge actions per Task 12 invariant.
            let blow_ups = set_bridge_direction
                .actions
                .iter()
                .filter(|(_, _, a)| matches!(a, crate::sim::bridge_specs::CellAction::BlowUpBridge))
                .count();
            assert_eq!(blow_ups, 4);
            // 2 perpendicular cells flagged dirty (E and W of (5,5)).
            assert_eq!(adjacent_bridges_dirty.len(), 2);
            assert!(zones_dirty);
        }
        other => panic!("expected Collapsed, got {other:?}"),
    }
    assert_eq!(
        state.cell(5, 5).unwrap().damage_state,
        DamageState::Destroyed
    );
}

#[test]
fn body_driver_partial_collapse_a_collapses_with_single_ramp_call() {
    let mut state = make_body_driver_test_state();
    state.cell_mut(5, 5).unwrap().damage_state = DamageState::PartialCollapseA;
    let outcome = state.body_cell_advance_state(5, 5, true, &mut flood_fill_terrain(20, 20, 0));
    assert!(matches!(outcome, StateOutcome::Collapsed { .. }));
    assert_eq!(
        state.cell(5, 5).unwrap().damage_state,
        DamageState::Destroyed
    );
}

#[test]
fn body_driver_partial_collapse_b_collapses_with_single_ramp_call() {
    let mut state = make_body_driver_test_state();
    state.cell_mut(5, 5).unwrap().damage_state = DamageState::PartialCollapseB;
    let outcome = state.body_cell_advance_state(5, 5, true, &mut flood_fill_terrain(20, 20, 0));
    assert!(matches!(outcome, StateOutcome::Collapsed { .. }));
    assert_eq!(
        state.cell(5, 5).unwrap().damage_state,
        DamageState::Destroyed
    );
}

#[test]
fn body_driver_destroyed_anchor_returns_no_change() {
    let mut state = make_body_driver_test_state();
    state.cell_mut(5, 5).unwrap().damage_state = DamageState::Destroyed;
    let outcome = state.body_cell_advance_state(5, 5, true, &mut flood_fill_terrain(20, 20, 0));
    assert!(matches!(outcome, StateOutcome::NoChange));
}

#[test]
fn body_driver_bridgehead_cell_returns_no_change() {
    let mut state = make_body_driver_test_state();
    state.cell_mut(5, 5).unwrap().role = BridgeCellRole::Bridgehead;
    let outcome = state.body_cell_advance_state(5, 5, true, &mut flood_fill_terrain(20, 20, 0));
    assert!(matches!(outcome, StateOutcome::NoChange));
}

/// BR-09: each of the three collapse arms (Damaged, PartialCollapseA,
/// PartialCollapseB) must clear the anchor's visible overlay to the
/// no-overlay sentinel. We seed the anchor with a Healthy-mapping overlay
/// byte (0xD6 ∈ the 0xD6..=0xD9 Healthy range of `effective_render_state`)
/// so that, BEFORE the fix, the collapsed cell would still render Healthy +
/// stay walkable. After collapse the byte must be 0xFF, render state None,
/// and the cell non-walkable.
fn assert_collapse_clears_anchor_overlay(start: DamageState) {
    let mut state = make_body_driver_test_state();
    {
        let anchor = state.cell_mut(5, 5).unwrap();
        anchor.damage_state = start;
        // Healthy-mapping loaded byte: without the fix, effective_render_state
        // maps this back to Healthy and is_bridge_walkable stays true.
        anchor.overlay_byte = 0xD6;
    }
    // Pre-condition sanity: the seeded byte renders as Healthy + walkable.
    assert!(matches!(
        BridgeRuntimeState::effective_render_state(state.cell(5, 5).unwrap()),
        Some(DamageState::Healthy { .. })
    ));
    assert!(state.is_bridge_walkable(5, 5));

    let outcome = state.body_cell_advance_state(5, 5, true, &mut flood_fill_terrain(20, 20, 0));
    assert!(
        matches!(outcome, StateOutcome::Collapsed { .. }),
        "start={start:?} must collapse"
    );

    let anchor = state.cell(5, 5).unwrap();
    assert_eq!(anchor.damage_state, DamageState::Destroyed);
    assert_eq!(
        anchor.overlay_byte, 0xFF,
        "collapsed anchor overlay must clear to 0xFF sentinel (start={start:?})"
    );
    assert!(
        BridgeRuntimeState::effective_render_state(anchor).is_none(),
        "collapsed anchor must render None (start={start:?})"
    );
    assert!(
        !state.is_bridge_walkable(5, 5),
        "collapsed anchor must not be walkable (start={start:?})"
    );
}

#[test]
fn body_collapse_from_damaged_clears_anchor_overlay() {
    assert_collapse_clears_anchor_overlay(DamageState::Damaged);
}

#[test]
fn body_collapse_from_partial_a_clears_anchor_overlay() {
    assert_collapse_clears_anchor_overlay(DamageState::PartialCollapseA);
}

#[test]
fn body_collapse_from_partial_b_clears_anchor_overlay() {
    assert_collapse_clears_anchor_overlay(DamageState::PartialCollapseB);
}

#[test]
fn bridge_collapse_clears_overlay_on_full_span() {
    let mut state = make_body_driver_test_state();
    state.cell_mut(5, 5).unwrap().damage_state = DamageState::Damaged;

    let outcome = state.body_cell_advance_state(5, 5, true, &mut flood_fill_terrain(20, 20, 0));

    let StateOutcome::Collapsed {
        destroyed_cells,
        radar_cells,
        ..
    } = outcome
    else {
        panic!("expected collapsed span");
    };
    for pos in [(5, 5), (6, 5), (7, 5), (8, 5), (4, 5)] {
        let cell = state.cell(pos.0, pos.1).expect("seeded span cell");
        assert_eq!(
            cell.overlay_byte, 0xFF,
            "span cell {pos:?} must clear the bridge overlay byte"
        );
        assert!(
            BridgeRuntimeState::effective_render_state(cell).is_none(),
            "span cell {pos:?} must render as collapsed"
        );
        assert!(
            destroyed_cells.contains(&pos),
            "destroyed_cells should include full span cell {pos:?}"
        );
        assert!(
            radar_cells.contains(&pos),
            "radar_cells should include full span cell {pos:?}"
        );
    }
}

#[test]
fn body_driver_out_of_bounds_returns_no_change() {
    let mut state = make_body_driver_test_state();
    let outcome = state.body_cell_advance_state(99, 99, true, &mut flood_fill_terrain(20, 20, 0));
    assert!(matches!(outcome, StateOutcome::NoChange));
}

/// 5x5 grid; column X=2 carries the NS bridgehead walk:
/// (2,4)=8 (bridgehead high-ramp peak), (2,3)=6, (2,2)=4 (anchor body),
/// (2,1)=0, (2,0)=0. Walk N from (2,4) terminates at (2,2).
fn make_bridgehead_terrain_ns() -> crate::map::resolved_terrain::ResolvedTerrainGrid {
    let mut cells = Vec::with_capacity(25);
    for ry in 0..5u16 {
        for rx in 0..5u16 {
            let template_height: u8 = if rx == 2 {
                match ry {
                    4 => 8,
                    3 => 6,
                    2 => 4,
                    _ => 0,
                }
            } else {
                0
            };
            cells.push(ResolvedTerrainCell {
                rx,
                ry,
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
                template_height,
                render_offset_x: 0,
                render_offset_y: 0,
                terrain_class: TerrainClass::Clear,
                speed_costs: SpeedCostProfile::default(),
                is_water: false,
                is_cliff_like: false,
                height_in_pixels: 0,
                variant: 0,
                is_rough: false,
                is_road: false,
                accepts_smudge: false,
                allows_tiberium: false,
                has_ramp: false,
                canonical_ramp: None,
                ground_walk_blocked: false,
                terrain_object_blocks: false,
                terrain_object_occupation: None,
                overlay_blocks: false,
                overlay_zone_type: None,
                outside_playfield: false,
                zone_type: 0,
                base_ground_walk_blocked: false,
                base_build_blocked: false,
                base_land_type: 0,
                base_yr_cell_land_type: 0,
                base_terrain_class: Default::default(),
                base_speed_costs: Default::default(),
                build_blocked: false,
                has_bridge_deck: false,
                bridge_walkable: false,
                bridge_transition: false,
                bridge_deck_level: 0,
                bridge_layer: None,
                bridge_facts: crate::map::bridge_facts::BridgeCellFacts {
                    raw_flags: if ry == 2 && (1..=3).contains(&rx) {
                        crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF
                    } else {
                        0
                    },
                    ..Default::default()
                },
                tube_index: None,
                radar_left: [0, 0, 0],
                radar_right: [0, 0, 0],
                has_damaged_data: false,
                bridgehead_anchor_class_at_load: None,
            });
        }
    }
    ResolvedTerrainGrid::from_cells(5, 5, cells)
}

/// Bridgehead at (2,4) NS, anchor at (2,2) NS, perpendicular partner
/// anchors at (1,2) west and (3,2) east. All cells start `Healthy{0}`.
/// Walk N from (2,4) h=8 → (2,3) h=6 → (2,2) h=4 (anchor).
fn make_bridgehead_state_ns() -> BridgeRuntimeState {
    let mut state = BridgeRuntimeState::default();
    // Bridgehead at (2, 4).
    state.test_seed_cell(
        2,
        4,
        BridgeRuntimeCell {
            deck_present: true,
            destroyable: true,
            deck_level: 0,
            bridge_group_id: Some(1),
            damage_state: DamageState::Healthy { variant: 0 },
            axis: Some(Axis::NS),
            role: BridgeCellRole::Bridgehead,
            anchor_span_id: None,
            overlay_byte: 0x18,
            bridgehead_anchor_class: BridgeheadAnchorClass::Variant0,
        },
    );
    // Anchor at (2, 2).
    state.test_seed_cell(
        2,
        2,
        BridgeRuntimeCell {
            deck_present: true,
            destroyable: true,
            deck_level: 0,
            bridge_group_id: Some(1),
            damage_state: DamageState::Healthy { variant: 0 },
            axis: Some(Axis::NS),
            role: BridgeCellRole::Anchor,
            anchor_span_id: Some(1),
            overlay_byte: 0x20,
            bridgehead_anchor_class: BridgeheadAnchorClass::Variant0,
        },
    );
    // DamageA neighbor (east of anchor) at (3, 2).
    state.test_seed_cell(
        3,
        2,
        BridgeRuntimeCell {
            deck_present: true,
            destroyable: true,
            deck_level: 0,
            bridge_group_id: Some(1),
            damage_state: DamageState::Healthy { variant: 0 },
            axis: Some(Axis::NS),
            role: BridgeCellRole::Anchor,
            anchor_span_id: Some(1),
            overlay_byte: 0x21,
            bridgehead_anchor_class: BridgeheadAnchorClass::Variant0,
        },
    );
    // DamageB neighbor (west of anchor) at (1, 2).
    state.test_seed_cell(
        1,
        2,
        BridgeRuntimeCell {
            deck_present: true,
            destroyable: true,
            deck_level: 0,
            bridge_group_id: Some(1),
            damage_state: DamageState::Healthy { variant: 0 },
            axis: Some(Axis::NS),
            role: BridgeCellRole::Anchor,
            anchor_span_id: Some(1),
            overlay_byte: 0x22,
            bridgehead_anchor_class: BridgeheadAnchorClass::Variant0,
        },
    );
    // Sentinel to grow state to 5x5 (matches terrain dimensions).
    state.test_seed_cell(
        0,
        4,
        BridgeRuntimeCell {
            deck_present: false,
            destroyable: false,
            deck_level: 0,
            bridge_group_id: None,
            damage_state: DamageState::Healthy { variant: 0 },
            axis: None,
            role: BridgeCellRole::Body,
            anchor_span_id: None,
            overlay_byte: 0,
            bridgehead_anchor_class: BridgeheadAnchorClass::Variant0,
        },
    );
    state
}

#[test]
fn bridgehead_advance_first_hit_writes_anchor_damaged() {
    let mut state = make_bridgehead_state_ns();
    let mut terrain = make_bridgehead_terrain_ns();
    let pre_hit_bridgehead = *state.cell(2, 4).unwrap();
    assert_eq!(
        state.cell(2, 2).unwrap().bridgehead_anchor_class,
        BridgeheadAnchorClass::Variant0
    );

    let outcome = state.bridgehead_advance_state(2, 4, true, &mut terrain);
    assert!(matches!(outcome, StateOutcome::Absorbed { .. }));

    // Bridgehead's own damage_state is NOT modified.
    let post_bridgehead = *state.cell(2, 4).unwrap();
    assert_eq!(
        post_bridgehead.damage_state,
        pre_hit_bridgehead.damage_state
    );

    // Anchor's bridgehead_anchor_class becomes AboutToFall (4th slot —
    // first-hit writes the most-damaged variant directly, skipping
    // intermediate slots).
    assert_eq!(
        state.cell(2, 2).unwrap().bridgehead_anchor_class,
        BridgeheadAnchorClass::AboutToFall
    );

    // East perpendicular partner (DamageA) — state byte 0 → 4 → Healthy{4}.
    assert_eq!(
        state.cell(3, 2).unwrap().damage_state,
        DamageState::Healthy { variant: 4 }
    );
    // West perpendicular partner (DamageB) — state byte 0 → 5 → Healthy{5}.
    assert_eq!(
        state.cell(1, 2).unwrap().damage_state,
        DamageState::Healthy { variant: 5 }
    );
}

#[test]
fn bridgehead_advance_repeat_high_hit_collapses_about_to_fall_slot() {
    let mut state = make_bridgehead_state_ns();
    let mut terrain = make_bridgehead_terrain_ns();
    let first = state.bridgehead_advance_state(2, 4, true, &mut terrain);
    assert!(matches!(first, StateOutcome::Absorbed { .. }));

    let second = state.bridgehead_advance_state(2, 4, true, &mut terrain);
    match second {
        StateOutcome::Collapsed {
            binary_success,
            destroyed_cells,
            set_bridge_direction,
            adjacent_bridges_dirty,
            zones_dirty,
            radar_cells,
            ..
        } => {
            assert!(binary_success);
            assert_eq!(destroyed_cells, vec![(2, 1), (2, 2), (2, 3)]);
            // BR-16: the collapsed BlowUpBridge triple is minimap-dirty.
            assert_eq!(radar_cells, vec![(2, 1), (2, 2), (2, 3)]);
            assert_eq!(set_bridge_direction.actions.len(), 3);
            assert!(
                set_bridge_direction
                    .actions
                    .iter()
                    .all(|(_, _, action)| matches!(
                        action,
                        crate::sim::bridge_specs::CellAction::BlowUpBridge
                    ))
            );
            assert_eq!(adjacent_bridges_dirty, vec![(3, 2), (1, 2)]);
            assert!(zones_dirty);
        }
        other => panic!("expected high bridgehead collapse, got {other:?}"),
    }

    assert_eq!(
        state.cell(2, 2).unwrap().bridgehead_anchor_class,
        BridgeheadAnchorClass::AboutToFall
    );
    assert!(matches!(
        state.cell(2, 2).unwrap().damage_state,
        DamageState::Destroyed
    ));
    assert!(matches!(
        state.cell(2, 4).unwrap().damage_state,
        DamageState::Healthy { .. }
    ));
}

#[test]
fn bridgehead_advance_repeat_low_hit_collapses_but_returns_false() {
    let mut state = make_bridgehead_state_ns();
    let mut terrain = make_bridgehead_terrain_ns();
    assert!(matches!(
        state.bridgehead_advance_state(2, 4, false, &mut terrain),
        StateOutcome::Absorbed { .. }
    ));

    let second = state.bridgehead_advance_state(2, 4, false, &mut terrain);
    match second {
        StateOutcome::Collapsed {
            binary_success,
            destroyed_cells,
            zones_dirty,
            ..
        } => {
            assert!(
                !binary_success,
                "low bridgehead slot +3 collapses but gamemd returns false"
            );
            assert_eq!(destroyed_cells, vec![(2, 1), (2, 2), (2, 3)]);
            assert!(zones_dirty);
        }
        other => panic!("expected low bridgehead collapse side effects, got {other:?}"),
    }
}

#[test]
fn bridgehead_advance_odd_h_ns_absorbs_with_no_change() {
    // Bridgehead at h=5 (odd NS ramp): parity gate fires.
    let mut state = make_bridgehead_state_ns();
    let mut terrain = make_bridgehead_terrain_ns();
    // Override (2, 4) height to 5 — odd, parity-gated.
    if let Some(cell) = terrain.cells.get_mut(4 * 5 + 2) {
        cell.template_height = 5;
    }
    let outcome = state.bridgehead_advance_state(2, 4, true, &mut terrain);
    assert_eq!(outcome, StateOutcome::NoChange);
    // Anchor's tile class unchanged.
    assert_eq!(
        state.cell(2, 2).unwrap().bridgehead_anchor_class,
        BridgeheadAnchorClass::Variant0
    );
}

#[test]
fn bridgehead_advance_h_gt_4_ew_absorbs_with_no_change() {
    // Bridgehead at h=0xC (EW high-ramp peak): upper-bound gate fires.
    // Use a fresh setup since the shared fixture is NS-axis.
    let mut state = BridgeRuntimeState::default();
    state.test_seed_cell(
        2,
        2,
        BridgeRuntimeCell {
            deck_present: true,
            destroyable: true,
            deck_level: 0,
            bridge_group_id: Some(1),
            damage_state: DamageState::Healthy { variant: 0 },
            axis: Some(Axis::EW),
            role: BridgeCellRole::Bridgehead,
            anchor_span_id: None,
            overlay_byte: 0x18,
            bridgehead_anchor_class: BridgeheadAnchorClass::Variant0,
        },
    );
    // 3x3 terrain with cell (2,2) h=0xC.
    let mut cells = Vec::with_capacity(9);
    for ry in 0..3u16 {
        for rx in 0..3u16 {
            cells.push(ResolvedTerrainCell {
                rx,
                ry,
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
                template_height: if rx == 2 && ry == 2 { 0x0C } else { 0 },
                render_offset_x: 0,
                render_offset_y: 0,
                terrain_class: TerrainClass::Clear,
                speed_costs: SpeedCostProfile::default(),
                is_water: false,
                is_cliff_like: false,
                height_in_pixels: 0,
                variant: 0,
                is_rough: false,
                is_road: false,
                accepts_smudge: false,
                allows_tiberium: false,
                has_ramp: false,
                canonical_ramp: None,
                ground_walk_blocked: false,
                terrain_object_blocks: false,
                terrain_object_occupation: None,
                overlay_blocks: false,
                overlay_zone_type: None,
                outside_playfield: false,
                zone_type: 0,
                base_ground_walk_blocked: false,
                base_build_blocked: false,
                base_land_type: 0,
                base_yr_cell_land_type: 0,
                base_terrain_class: Default::default(),
                base_speed_costs: Default::default(),
                build_blocked: false,
                has_bridge_deck: false,
                bridge_walkable: false,
                bridge_transition: false,
                bridge_deck_level: 0,
                bridge_layer: None,
                bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
                tube_index: None,
                radar_left: [0, 0, 0],
                radar_right: [0, 0, 0],
                has_damaged_data: false,
                bridgehead_anchor_class_at_load: None,
            });
        }
    }
    let mut terrain = ResolvedTerrainGrid::from_cells(3, 3, cells);
    let outcome = state.bridgehead_advance_state(2, 2, true, &mut terrain);
    assert_eq!(outcome, StateOutcome::NoChange);
}

#[test]
fn bridgehead_advance_walks_through_odd_intermediate() {
    // Mid-walk parity tolerance: walk passes through an odd h=5
    // intermediate between h=8 start and h=4 anchor.
    let mut state = make_bridgehead_state_ns();
    let mut terrain = make_bridgehead_terrain_ns();
    // Patch the walk path: (2,4)=8, (2,3)=5 (odd!), (2,2)=4.
    if let Some(c) = terrain.cells.get_mut(3 * 5 + 2) {
        c.template_height = 5;
    }
    let outcome = state.bridgehead_advance_state(2, 4, true, &mut terrain);
    assert!(matches!(outcome, StateOutcome::Absorbed { .. }));
    assert_eq!(
        state.cell(2, 2).unwrap().bridgehead_anchor_class,
        BridgeheadAnchorClass::AboutToFall,
        "walk must pass through odd-h intermediate and damage the anchor",
    );
}

#[test]
fn bridgehead_advance_non_bridgehead_role_no_change() {
    let mut state = make_bridgehead_state_ns();
    state.cell_mut(2, 4).unwrap().role = BridgeCellRole::Body;
    let mut terrain = make_bridgehead_terrain_ns();
    let outcome = state.bridgehead_advance_state(2, 4, true, &mut terrain);
    assert_eq!(outcome, StateOutcome::NoChange);
}

#[test]
fn bridgehead_advance_anchor_walk_failure_no_change() {
    // All heights = 10: start cell is even (passes parity gate) but walk
    // never converges to h=4 within the 16-iter cap (heights stay 10
    // along the column / walking off-map).
    let mut state = make_bridgehead_state_ns();
    let mut terrain = make_bridgehead_terrain_ns();
    for c in terrain.cells.iter_mut() {
        c.template_height = 10;
    }
    let outcome = state.bridgehead_advance_state(2, 4, true, &mut terrain);
    assert_eq!(outcome, StateOutcome::NoChange);
    // Anchor's tile class unchanged.
    assert_eq!(
        state.cell(2, 2).unwrap().bridgehead_anchor_class,
        BridgeheadAnchorClass::Variant0
    );
}

#[test]
fn bridgehead_advance_off_map_no_change() {
    let mut state = make_bridgehead_state_ns();
    let mut terrain = make_bridgehead_terrain_ns();
    let outcome = state.bridgehead_advance_state(99, 99, true, &mut terrain);
    assert_eq!(outcome, StateOutcome::NoChange);
}

// Admission and callback order compare original instructions in damage_dispatch_tests.

#[test]
fn dispatch_path_is_state_machine() {
    assert!(DispatchPath::HighStateMachine.is_state_machine());
    assert!(DispatchPath::LowStateMachine.is_state_machine());
    assert!(!DispatchPath::HighDirect.is_state_machine());
    assert!(!DispatchPath::LowDirect.is_state_machine());
}

#[test]
fn bridge_state_getters_return_construction_values() {
    let state = BridgeRuntimeState::from_resolved_terrain(&make_bridge_terrain(), true, 1500);
    assert!(state.is_destroyable());
    assert_eq!(state.bridge_strength(), 1500);
    assert!(state.width() >= 5);
    assert!(state.height() >= 1);
}

#[test]
fn bridge_state_destroyable_flag_disabled() {
    let state = BridgeRuntimeState::from_resolved_terrain(&make_bridge_terrain(), false, 800);
    assert!(!state.is_destroyable());
    assert_eq!(state.bridge_strength(), 800);
}

// ---- G4 damaged-variant flood-fill tests ------------------------------------

/// Build a flat `width × height` `ResolvedTerrainGrid` where every cell
/// shares `final_tile_index = tile_id`, `has_damaged_data = true`, and
/// all other fields are zero/default. Suitable for flood-fill unit tests
/// that only care about tile_id equality + has_damaged_data gating.
fn flood_fill_terrain(width: u16, height: u16, tile_id: i32) -> ResolvedTerrainGrid {
    let mut cells = Vec::with_capacity(width as usize * height as usize);
    for ry in 0..height {
        for rx in 0..width {
            cells.push(ResolvedTerrainCell {
                rx,
                ry,
                source_tile_index: tile_id,
                source_sub_tile: 0,
                final_tile_index: tile_id,
                final_sub_tile: 0,
                is_wood_bridge_repair_tile: false,
                level: 0,
                filled_clear: false,
                tileset_index: Some(0),
                land_type: 0,
                yr_cell_land_type: 0,
                slope_type: 0,
                template_height: 0,
                render_offset_x: 0,
                render_offset_y: 0,
                terrain_class: TerrainClass::Clear,
                speed_costs: SpeedCostProfile::default(),
                is_water: false,
                is_cliff_like: false,
                height_in_pixels: 0,
                variant: 0,
                is_rough: false,
                is_road: false,
                accepts_smudge: false,
                allows_tiberium: false,
                has_ramp: false,
                canonical_ramp: None,
                ground_walk_blocked: false,
                terrain_object_blocks: false,
                terrain_object_occupation: None,
                overlay_blocks: false,
                overlay_zone_type: None,
                outside_playfield: false,
                zone_type: 0,
                base_ground_walk_blocked: false,
                base_build_blocked: false,
                base_land_type: 0,
                base_yr_cell_land_type: 0,
                base_terrain_class: Default::default(),
                base_speed_costs: Default::default(),
                build_blocked: false,
                has_bridge_deck: false,
                bridge_walkable: false,
                bridge_transition: false,
                bridge_deck_level: 0,
                bridge_layer: None,
                bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
                tube_index: None,
                radar_left: [0, 0, 0],
                radar_right: [0, 0, 0],
                has_damaged_data: true,
                bridgehead_anchor_class_at_load: None,
            });
        }
    }
    ResolvedTerrainGrid::from_cells(width, height, cells)
}

/// Build a `BridgeRuntimeState` with healthy body cells at the given coords.
fn flood_fill_bridge_state(coords: &[(u16, u16)]) -> BridgeRuntimeState {
    let mut state = BridgeRuntimeState::default();
    for &(rx, ry) in coords {
        state.test_seed_cell(
            rx,
            ry,
            BridgeRuntimeCell {
                deck_present: true,
                destroyable: true,
                deck_level: 0,
                bridge_group_id: Some(1),
                damage_state: DamageState::Healthy { variant: 0 },
                axis: Some(Axis::NS),
                role: BridgeCellRole::Body,
                anchor_span_id: Some(1),
                overlay_byte: 0,
                bridgehead_anchor_class: BridgeheadAnchorClass::Variant0,
            },
        );
    }
    state
}

fn pavement_test_terrain(coords: &[(u16, u16)]) -> ResolvedTerrainGrid {
    let mut terrain = flood_fill_terrain(10, 10, 0);
    for &(rx, ry) in coords {
        terrain.cell_mut(rx, ry).unwrap().final_tile_index = 42;
    }
    terrain
}

#[test]
fn flood_fill_kickoff_skips_when_no_damaged_data() {
    let mut bs = flood_fill_bridge_state(&[(5, 5), (5, 6)]);
    let mut terrain = pavement_test_terrain(&[(5, 5), (5, 6), (5, 7)]);
    if let Some(c) = terrain.cell_mut(5, 5) {
        c.has_damaged_data = false;
    }
    let changed = bs.apply_damaged_variant_flood_fill(5, 5, true, &mut terrain);
    assert!(changed.is_empty());
    assert!(!terrain.pavement_damaged_at(5, 5));
    assert!(!terrain.pavement_damaged_at(5, 6));
}

#[test]
fn flood_fill_propagates_to_same_tile_id_neighbors() {
    let coords = [
        (4, 4),
        (5, 4),
        (6, 4),
        (4, 5),
        (5, 5),
        (6, 5),
        (4, 6),
        (5, 6),
        (6, 6),
    ];
    let mut bs = flood_fill_bridge_state(&coords);
    let mut terrain = pavement_test_terrain(&coords);
    let changed = bs.apply_damaged_variant_flood_fill(5, 5, true, &mut terrain);
    assert_eq!(
        changed,
        vec![
            (5, 5),
            (5, 4),
            (6, 4),
            (6, 5),
            (6, 6),
            (5, 6),
            (4, 6),
            (4, 5),
            (4, 4),
        ],
        "0x0056E990 marks before direction-0..7 recursive descent",
    );
    for &(rx, ry) in &coords {
        assert!(
            terrain.pavement_damaged_at(rx, ry),
            "cell ({},{}) should be damaged",
            rx,
            ry
        );
    }
}

#[test]
fn flood_fill_stops_at_different_tile_id_boundary() {
    let mut bs = flood_fill_bridge_state(&[(5, 5), (5, 6), (5, 7)]);
    let mut terrain = pavement_test_terrain(&[(5, 5), (5, 6), (5, 7)]);
    if let Some(c) = terrain.cell_mut(5, 6) {
        c.final_tile_index = 99;
    }
    let _ = bs.apply_damaged_variant_flood_fill(5, 5, true, &mut terrain);
    assert!(terrain.pavement_damaged_at(5, 5));
    assert!(
        !terrain.pavement_damaged_at(5, 6),
        "boundary cell stays pristine"
    );
    assert!(
        !terrain.pavement_damaged_at(5, 7),
        "downstream cell stays pristine"
    );
}

#[test]
fn flood_fill_idempotent_when_already_in_target_state() {
    let mut bs = flood_fill_bridge_state(&[(5, 5)]);
    let mut terrain = pavement_test_terrain(&[(5, 5), (5, 6), (5, 7)]);
    terrain.cell_mut(5, 5).unwrap().bridge_facts.raw_flags |= 0x2000;
    let changed = bs.apply_damaged_variant_flood_fill(5, 5, true, &mut terrain);
    assert!(changed.is_empty(), "no mutation when already in target state");
}

#[test]
fn flood_fill_eight_directions_includes_diagonals() {
    let coords = [
        (4, 4),
        (5, 4),
        (6, 4),
        (4, 5),
        (5, 5),
        (6, 5),
        (4, 6),
        (5, 6),
        (6, 6),
    ];
    let mut bs = flood_fill_bridge_state(&coords);
    let mut terrain = pavement_test_terrain(&coords);
    let changed = bs.apply_damaged_variant_flood_fill(5, 5, true, &mut terrain);
    assert_eq!(changed.len(), 9);
    assert!(terrain.pavement_damaged_at(4, 4), "NW diagonal hit");
    assert!(terrain.pavement_damaged_at(6, 4), "NE diagonal hit");
    assert!(terrain.pavement_damaged_at(4, 6), "SW diagonal hit");
    assert!(terrain.pavement_damaged_at(6, 6), "SE diagonal hit");
}

#[test]
fn flood_fill_clear_propagates_state_false() {
    let coords = [(5u16, 5u16), (5, 6), (5, 7)];
    let mut bs = flood_fill_bridge_state(&coords);
    let mut terrain = pavement_test_terrain(&coords);
    for &(rx, ry) in &coords {
        terrain.cell_mut(rx, ry).unwrap().bridge_facts.raw_flags |= 0x2000;
    }
    let changed = bs.apply_damaged_variant_flood_fill(5, 5, false, &mut terrain);
    assert_eq!(changed, vec![(5, 5), (5, 6), (5, 7)]);
    for &(rx, ry) in &coords {
        assert!(!terrain.pavement_damaged_at(rx, ry));
    }
}

#[test]
fn flood_fill_off_map_returns_zero() {
    let mut bs = flood_fill_bridge_state(&[(5, 5)]);
    let mut terrain = pavement_test_terrain(&[(5, 5), (5, 6), (5, 7)]);
    let changed = bs.apply_damaged_variant_flood_fill(99, 99, true, &mut terrain);
    assert!(changed.is_empty());
}

#[test]
fn flood_fill_sentinel_tile_id_returns_zero() {
    let mut bs = flood_fill_bridge_state(&[(5, 5)]);
    let mut terrain = pavement_test_terrain(&[(5, 5), (5, 6), (5, 7)]);
    if let Some(c) = terrain.cell_mut(5, 5) {
        c.final_tile_index = 0xFFFF;
    }
    let changed = bs.apply_damaged_variant_flood_fill(5, 5, true, &mut terrain);
    assert!(changed.is_empty());
}

/// Synthetic 3x3 grid with a single bridge anchor cell at (1,1).
/// `pre_class` is written to that cell's bridgehead_anchor_class_at_load.
fn make_pre_class_terrain(pre_class: Option<BridgeheadAnchorClass>) -> ResolvedTerrainGrid {
    use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
    let mut cells = Vec::with_capacity(9);
    for ry in 0..3u16 {
        for rx in 0..3u16 {
            let is_anchor = rx == 1 && ry == 1;
            cells.push(ResolvedTerrainCell {
                rx,
                ry,
                source_tile_index: 0,
                source_sub_tile: 0,
                final_tile_index: 0,
                final_sub_tile: 0,
                is_wood_bridge_repair_tile: false,
                level: 0,
                filled_clear: false,
                tileset_index: None,
                land_type: 0,
                yr_cell_land_type: 0,
                slope_type: 0,
                template_height: 0,
                render_offset_x: 0,
                render_offset_y: 0,
                terrain_class: TerrainClass::Clear,
                speed_costs: SpeedCostProfile::default(),
                is_water: false,
                is_cliff_like: false,
                height_in_pixels: 0,
                variant: 0,
                is_rough: false,
                is_road: false,
                accepts_smudge: false,
                allows_tiberium: false,
                has_ramp: false,
                canonical_ramp: None,
                ground_walk_blocked: false,
                terrain_object_blocks: false,
                terrain_object_occupation: None,
                overlay_blocks: false,
                overlay_zone_type: None,
                outside_playfield: false,
                zone_type: 0,
                base_ground_walk_blocked: false,
                base_build_blocked: false,
                base_land_type: 0,
                base_yr_cell_land_type: 0,
                base_terrain_class: Default::default(),
                base_speed_costs: Default::default(),
                build_blocked: false,
                has_bridge_deck: is_anchor,
                bridge_walkable: is_anchor,
                bridge_transition: false,
                bridge_deck_level: 0,
                bridge_layer: None,
                bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
                tube_index: None,
                radar_left: [0, 0, 0],
                radar_right: [0, 0, 0],
                has_damaged_data: false,
                bridgehead_anchor_class_at_load: if is_anchor { pre_class } else { None },
            });
        }
    }
    ResolvedTerrainGrid::from_cells(3, 3, cells)
}

#[test]
fn from_resolved_terrain_copies_pre_damaged_anchor_class() {
    let terrain = make_pre_class_terrain(Some(BridgeheadAnchorClass::AboutToFall));
    let state = BridgeRuntimeState::from_resolved_terrain(&terrain, true, 1500);
    let cell = state.cell(1, 1).expect("bridge cell exists");
    assert_eq!(
        cell.bridgehead_anchor_class,
        BridgeheadAnchorClass::AboutToFall
    );
}

#[test]
fn from_resolved_terrain_defaults_to_variant0_when_pre_class_is_none() {
    let terrain = make_pre_class_terrain(None);
    let state = BridgeRuntimeState::from_resolved_terrain(&terrain, true, 1500);
    let cell = state.cell(1, 1).expect("bridge cell exists");
    assert_eq!(
        cell.bridgehead_anchor_class,
        BridgeheadAnchorClass::Variant0
    );
}

/// `ApplyDamageToCell` 0x00587180 routes to a walker on a narrower band than
/// the walkers themselves accept. Its tests are `(0x49 < o) && (o < 100)` and
/// `(0xCC < o) && (o < 0xE7)`, so 0x64, 0x65, 0xE7 and 0xE8 fall through to
/// the tileset-family branch even though `DestroyBridge_Low` 0x0057BAA0 and
/// `DestroyBridge_High` 0x0057CCF0 name them as valid axis classes
/// (0x64/0xE7 NS, 0x65/0xE8 EW) once already running.
#[test]
fn apply_damage_dispatch_bands_are_narrower_than_the_walker_bands() {
    assert!(!is_low_dispatch_overlay(0x49));
    assert!(is_low_dispatch_overlay(0x4A));
    assert!(is_low_dispatch_overlay(0x63));
    assert!(!is_low_dispatch_overlay(0x64));
    assert!(!is_low_dispatch_overlay(0x65));

    assert!(!is_high_dispatch_overlay(0xCC));
    assert!(is_high_dispatch_overlay(0xCD));
    assert!(is_high_dispatch_overlay(0xE6));
    assert!(!is_high_dispatch_overlay(0xE7));
    assert!(!is_high_dispatch_overlay(0xE8));

    // The walker-side bands, which legitimately include those four values.
    assert!(BridgeRuntimeState::is_low_destroy_overlay(0x64));
    assert!(BridgeRuntimeState::is_low_destroy_overlay(0x65));
    assert!(BridgeRuntimeState::is_high_destroy_overlay(0xE7));
    assert!(BridgeRuntimeState::is_high_destroy_overlay(0xE8));
}

/// RESIDUAL — gamemd addresses 0x00572230 (and its fifteen compiled twins,
/// enumerated on `crate::sim::bridge_specs::RampOutcome`).
///
/// Mechanism: after the state-byte promotion, every ramp updater computes
/// `cell+0x38 - g_BridgeSet_TileSetBase + 1` on the perpendicular target and
/// branches on three runtime tile-class constants. VERA now models the
/// `MapClass::ToggleBridgePavement` 0x0056E990 damaged-TMP selector and exact
/// flood/dirty order. The alternate calls to `MapClass::FloodFillIsoTileType`
/// that repaint a connected ramp iso-tile remain unported.
///
/// Trigger: any damage or collapse step on a bridge whose anchor has a ramp
/// or bridgehead neighbour — i.e. essentially every hit that damages a span.
///
/// Effect: affected alternate-class ramps keep their prior iso-tile art after
/// the span changes; damaged-TMP ramps do switch and now redraw correctly.
///
/// Frequency: high on any map with a bridge that gets attacked, which is most
/// matches on most retail maps carrying one. The residual is purely visual —
/// no pathing, damage or RNG consequence was found in 0x00572230.
#[test]
#[ignore = "gamemd's alternate ramp tile classes call FloodFillIsoTileType; only the damaged-TMP ToggleBridgePavement branch is ported"]
fn ramp_isotile_repaint_branch_is_unported() {
    panic!("unimplemented: FloodFillIsoTileType branch of the ramp updaters");
}

/// RESIDUAL - gamemd addresses 0x00576770 `MapClass::UpdateAdjacentBridges_High`,
/// 0x00571050 `MapClass::UpdateAdjacentBridges`, and the edge writers they
/// drive, 0x00576200 `UpdateBridgeEdgeTiles_High` and 0x00570AE0
/// `UpdateBridgeEdgeTiles_Low`.
///
/// **Both bodies re-decompiled 2026-08-19. The earlier version of this note
/// called the gap purely visual and said the writers touch no damage state,
/// pathing field or RNG. That is wrong**, and it is the reason this is being
/// corrected rather than implemented: a port written against the old note
/// would have been scoped as a tile-art fix and would have silently taken on
/// bridge-flag and overlay writes.
///
/// Entry (0x00576770): walk the eight directions from the damaged cell until
/// a neighbour carries `+0x140 & 0x500`; pick a start coordinate from that
/// neighbour's 0x100 / 0x400 / 0x80 bits; walk along the axis chosen by the
/// neighbour's `0x800` bit while cells stay inside the map rect and carry a
/// non-null entry in `MapClass+0x13C`; classify the landed cell's
/// `cell+0x38 - g_BridgeSet_TileSetBase + 1` against SIX theater tileset
/// globals paired with a `+0x11A` sub-tile value of 8, 5, 12 or 7; and call
/// `UpdateBridgeEdgeTiles_High` with mode 2 or 4, dirtying the screen rect
/// through `TacticalClass::DirtyScreenRect` only when the returned rect
/// actually changed.
///
/// The globals are theater INI tileset indices written by
/// `Read_Theater_TileSets_INI`; `DAT_00ABAD30` is `[General] BridgeMiddle1`
/// (key string 0x008292C4, stored at 0x00545C1E), which `map::theater`
/// already parses. The remaining five were not resolved to their keys.
///
/// **What 0x00576200 actually does, and why this is not a visual slice:** it
/// walks up to 30 cells along the span looking for the tile class matching
/// its mode, unions a screen rect through
/// `TacticalClass::CoordsToClient2`, and at the anchor-flag transition it
/// calls `NotifyBridgeSpanCollapse`, then
/// `CellClass::SetBridgeDirection_NESW` with direction 0 (mode 2) or 6
/// (mode 4), writes `cell+0x11E = 0`, writes `cell+0x44 = -1` clearing the
/// overlay, calls `RadarClass::MarkTerrainDirty`, and re-enters itself.
///
/// `SetBridgeDirection` is the writer of the `0x100` and `0x800` bits in
/// `cell+0x140`. Those bits are read by the pathfinding zone build, the
/// destroy and repair walkers, `CheckBridgeTraversal` 0x004D9C60 and the
/// render predicate `IsOnBridge_ForFiring` 0x00703B10. `cell+0x44` is the
/// overlay byte the destroy walkers match their bands against. So this
/// function re-stamps sim-visible bridge topology at collapse time; it is not
/// rim art.
///
/// `compute_adjacent_bridges_dirty` reproduces only the first step's
/// *result* - which two perpendicular cells to mark dirty. Neither the span
/// walk, the tile classification, the re-stamp, nor the overlay clear
/// happens.
///
/// Trigger: every bridge collapse. Confirmed from callers - eight call sites,
/// all in `ProcessBridgeDamageStateMachine_High` 0x00576BB0 and both hut-death
/// destroy paths `MapClass::DestroyBridge_High_OnHutDeath` 0x005745B4 and
/// `_Low_OnHutDeath` 0x005751D0.
///
/// Effect: the rim tiles at the break keep their intact art, AND the cells at
/// the break keep bridge flags and an overlay id that gamemd clears. The
/// second half is the part that matters: VERA's own predicates keep reading a
/// span the engine has already unstamped.
///
/// Frequency: every collapse on any map with a bridge.
///
/// Blocker, restated: this is not a slice. It needs the five unresolved
/// theater tileset keys, a `+0x11A` sub-tile reader, the `MapClass+0x13C`
/// span array, a `SetBridgeDirection` re-stamp path, and a screen-rect union
/// - and because it writes flags the pathfinder and the render predicate
/// read, it needs the same evidence bar as a sim change rather than a render
/// one.
#[test]
#[ignore = "gamemd 0x00576770 -> 0x00576200 re-stamps bridge flags and clears the overlay at a collapse break, not just edge art; VERA does neither"]
fn bridge_edge_tile_rewrite_after_collapse_is_unported() {
    panic!("unimplemented: UpdateAdjacentBridges / UpdateBridgeEdgeTiles span walk and re-stamp");
}

/// RESIDUAL — gamemd addresses 0x00570050 `ProcessBridgeDestruction_Low`,
/// 0x00573540 `ProcessBridgeDestruction_High`, the span walkers they drive
/// (0x00569760 `MapClass::BridgePavementSpanWalker` and 0x00568E40 its high
/// twin), `MapClass::ToggleBridgePavement` 0x0056E990 and
/// `MapClass::ValidateBridgeZones` 0x0056DB70.
///
/// The names mislead: 0x00570050 decompiled 2026-08-19 is the REPAIR entry.
/// Its 5x5 overlay scan hands the first cell in `[0x4A..=0x65]` to
/// `MapClass::RepairBridge_Low` 0x0057F200 and returns. Everything after that
/// is the terrain restoration the repair needs:
/// - `ToggleBridgePavement(coord, 0, 0)` on the ramp cell, then
///   `BridgePavementSpanWalker(cell, 2 or 4, &rect)` and a
///   `TacticalClass::DirtyScreenRect` over the rect it returns;
/// - on the `+4` tile-class variants, `FloodFillIsoTileType` back to the
///   pre-collapse iso-tile and `cell+0x11B += 4` on three neighbours — the
///   deck-level raise being put back;
/// - `ValidateBridgeZones`, a recursive call two cells back along the span,
///   and `RebuildZoneConnectivity` when validation reports a change.
///
/// `repair_bridge_from_engineer_scan` reproduces the scan and the
/// `RepairBridge_*` dispatch. None of the restoration tail exists.
///
/// Trigger: every successful engineer repair of a collapsed bridge.
///
/// Effect: the deck becomes walkable again but the approach keeps its
/// collapsed appearance, and the `+0x11B += 4` level raise is not re-applied,
/// so the ramp cells stay at the dropped height. That is not purely visual —
/// cell level feeds ground height, which feeds the bridge transition predicate
/// and unit Z.
///
/// **Blocker, established 2026-08-19.** This cannot be ported as written.
/// Every branch in the tail is selected by comparing
/// `cell+0x38 - g_WoodBridgeSet_TileSetBase + 1` against runtime tile-class
/// constants - `DAT_00ABAD30`, `DAT_00ABC2B4`, `DAT_00AA1130`, `DAT_00AA1028`,
/// `DAT_00AA1548`, `DAT_00AA0740` - and the `+0x11B += 4` raise fires only on
/// the `DAT_00ABAD30 + 4` variant paired with a `+0x11A` sub-tile of 5. Those
/// constants live in BSS and are written per theater by
/// `Read_Theater_TileSets_INI`, so they cannot be read out of the binary
/// statically. `BridgeRampTile::relative_tile_index` is the right field to
/// hold the comparison, but the values to compare against have to come from a
/// live observation or from porting the theater tileset reader first.
/// Guessing them would put a fabricated constant into sim state that feeds
/// ground height. Recorded rather than approximated.
///
/// Frequency: every successful engineer repair of a collapsed bridge. Repairs
/// are uncommon per match, but the effect persists for the rest of the game
/// once one happens, and a repaired bridge is something both players then
/// path over.
#[test]
#[ignore = "gamemd restores pavement, iso-tile and the +4 level raise after a repair (0x00570050); VERA only re-dispatches RepairBridge"]
fn bridge_repair_terrain_restoration_is_unported() {
    panic!("unimplemented: ProcessBridgeDestruction_* repair restoration tail");
}

// ---- MapClass::FindBridgeConnection_Predicate 0x00587410, overlay branch ----

fn seed_overlay_row(state: &mut BridgeRuntimeState, y: u16, xs: std::ops::Range<u16>, overlay: u8) {
    for x in xs {
        state.test_seed_cell(
            x,
            y,
            BridgeRuntimeCell {
                deck_present: true,
                destroyable: true,
                deck_level: 5,
                bridge_group_id: Some(1),
                damage_state: DamageState::Healthy { variant: 0 },
                axis: Some(Axis::NS),
                role: BridgeCellRole::Body,
                anchor_span_id: Some(1),
                overlay_byte: overlay,
                bridgehead_anchor_class: BridgeheadAnchorClass::Variant0,
            },
        );
    }
}

/// An intact high span carries healthy body overlays, which ARE inside the
/// 0xCD..=0xE8 family band but are not the 0xE7 / 0xE8 collapsed anchors, so
/// the native predicate walks the span and finds nothing.
#[test]
fn hut_span_scan_rejects_an_intact_high_span() {
    let mut state = BridgeRuntimeState::default();
    seed_overlay_row(&mut state, 6, 0..14, 0xCD);

    assert!(
        !crate::sim::world::bridge_orchestrator::hut_span_has_collapsed_anchor(&state, (6, 6)),
        "an intact span offers nothing to repair"
    );
}

/// A collapsed anchor anywhere along the walked span returns true, including
/// well outside the 5x5 block the scan starts from.
#[test]
fn hut_span_scan_finds_a_collapsed_high_anchor_down_the_span() {
    let mut state = BridgeRuntimeState::default();
    seed_overlay_row(&mut state, 6, 0..14, 0xCD);
    seed_overlay_row(&mut state, 6, 11..12, 0xE7);

    assert!(
        crate::sim::world::bridge_orchestrator::hut_span_has_collapsed_anchor(&state, (6, 6)),
        "0xE7 is what DestroyBridgeWalker_NS_High writes on final collapse"
    );
}

/// Low family, same shape, with the 0x64 anchor.
#[test]
fn hut_span_scan_finds_a_collapsed_low_anchor() {
    let mut state = BridgeRuntimeState::default();
    seed_overlay_row(&mut state, 6, 0..10, 0x4A);
    seed_overlay_row(&mut state, 6, 8..9, 0x64);

    assert!(
        crate::sim::world::bridge_orchestrator::hut_span_has_collapsed_anchor(&state, (6, 6)),
        "0x64 is the low-side collapsed anchor"
    );
    let mut intact = BridgeRuntimeState::default();
    seed_overlay_row(&mut intact, 6, 0..10, 0x4A);
    assert!(
        !crate::sim::world::bridge_orchestrator::hut_span_has_collapsed_anchor(&intact, (6, 6)),
        "an intact low span offers nothing to repair"
    );
}

/// The walk stops when the overlay leaves the family band, so a collapsed
/// anchor on the far side of a gap is not reached.
#[test]
fn hut_span_scan_stops_at_the_band_edge() {
    let mut state = BridgeRuntimeState::default();
    seed_overlay_row(&mut state, 6, 4..9, 0xCD);
    seed_overlay_row(&mut state, 6, 9..10, 0x00);
    seed_overlay_row(&mut state, 6, 10..13, 0xE7);

    assert!(
        !crate::sim::world::bridge_orchestrator::hut_span_has_collapsed_anchor(&state, (6, 6)),
        "an out-of-band cell ends the walk before the anchor"
    );
}

/// Cells with no bridge runtime entry at all contribute nothing.
#[test]
fn hut_span_scan_on_empty_state_is_false() {
    let state = BridgeRuntimeState::default();
    assert!(!crate::sim::world::bridge_orchestrator::hut_span_has_collapsed_anchor(&state, (6, 6)));
}

/// 0x00587410's 5x5 loop has no break, so the cell it walks from is the LAST
/// match in Y-major order, not any match. Row y=4 leads to an anchor; row y=8
/// does not. Y-major makes (8, 8) the surviving seed, so the predicate must
/// answer false even though a reachable anchor exists from an earlier cell.
#[test]
fn hut_span_scan_walks_only_the_last_y_major_seed() {
    let mut state = BridgeRuntimeState::default();
    seed_overlay_row(&mut state, 4, 4..9, 0xCD);
    seed_overlay_row(&mut state, 4, 9..12, 0xCD);
    seed_overlay_row(&mut state, 4, 12..13, 0xE7);
    seed_overlay_row(&mut state, 8, 4..9, 0xCD);

    assert!(
        !crate::sim::world::bridge_orchestrator::hut_span_has_collapsed_anchor(&state, (6, 6)),
        "scanning every cell and accepting any hit is more permissive than the binary"
    );

    // Same map with the anchor moved onto the surviving seed's row.
    let mut reachable = BridgeRuntimeState::default();
    seed_overlay_row(&mut reachable, 4, 4..9, 0xCD);
    seed_overlay_row(&mut reachable, 8, 4..12, 0xCD);
    seed_overlay_row(&mut reachable, 8, 12..13, 0xE7);
    assert!(
        crate::sim::world::bridge_orchestrator::hut_span_has_collapsed_anchor(&reachable, (6, 6)),
        "the surviving seed's own span is walked"
    );
}

/// The EW-class arm: an overlay in {0xD6..=0xDE} u {0xE3..=0xE6} u {0xE8} walks
/// along Y, so the anchor has to be down a column rather than along a row.
#[test]
fn hut_span_scan_walks_ew_class_overlays_along_y() {
    let mut column = BridgeRuntimeState::default();
    for y in 4..14u16 {
        seed_overlay_row(&mut column, y, 6..7, 0xD6);
    }
    seed_overlay_row(&mut column, 13, 6..7, 0xE8);
    assert!(
        crate::sim::world::bridge_orchestrator::hut_span_has_collapsed_anchor(&column, (6, 6)),
        "an EW-class overlay is walked along Y"
    );

    // The same overlays laid out as a row leave the Y walk with nothing.
    let mut row = BridgeRuntimeState::default();
    seed_overlay_row(&mut row, 6, 4..14, 0xD6);
    seed_overlay_row(&mut row, 6, 14..15, 0xE8);
    assert!(
        !crate::sim::world::bridge_orchestrator::hut_span_has_collapsed_anchor(&row, (6, 6)),
        "walking Y off a single row finds nothing, which is what proves the axis"
    );
}

/// The one layout where Y-major and X-major disagree, and therefore the only
/// test that actually pins 0x00587410's scan order.
///
/// The 5x5 around (6, 6) spans x 4..=8, y 4..=8. Exactly two destroy-band cells
/// sit inside it: (8, 6) = (cx+2, cy) and (6, 8) = (cx, cy+2). The native's
/// outer counter is Y (0x00587443) and its inner is X (0x00587447), so the
/// dy=+2 row is visited after the dy=0 row and **(6, 8) is the surviving
/// seed**. The CABHUT death path's `hut_destroy_5x5_scan` is dx-outer and would
/// visit the dx=+2 column last, keeping (8, 6) instead.
///
/// Both are NS-class overlays, so both walk along X. (8, 6) continues east out
/// of the block to a collapsed anchor at (11, 6); (6, 8) dead-ends with no
/// neighbour either way. A port using the wrong order answers true.
#[test]
fn hut_span_scan_order_is_y_major_not_x_major() {
    let mut state = BridgeRuntimeState::default();
    // (8, 6): NS-class, walks X, reaches an anchor outside the 5x5.
    seed_overlay_row(&mut state, 6, 8..11, 0xCD);
    seed_overlay_row(&mut state, 6, 11..12, 0xE7);
    // (6, 8): NS-class, walks X, isolated - no neighbour east or west.
    seed_overlay_row(&mut state, 8, 6..7, 0xCD);

    assert!(
        !crate::sim::world::bridge_orchestrator::hut_span_has_collapsed_anchor(&state, (6, 6)),
        "Y-major keeps (6, 8), which dead-ends; an X-major scan would keep          (8, 6) and wrongly answer true"
    );
}
