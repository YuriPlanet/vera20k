//! Tests for bridge runtime state construction and state transitions.

use super::*;
use crate::map::resolved_terrain::{
    BridgeDirection, BridgeLayer, ResolvedTerrainCell, ResolvedTerrainGrid, YR_CELL_LAND_TUNNEL,
};
use crate::map::tube_facts::{TubeFact, TubeId};

include!("record_native_tests.rs");
include!("gap_restamp_tests.rs");

#[test]
fn playfield_retail_high_bridge_walks_make_native_records_monotone() {
    assert!(
        HIGH_BRIDGE_WALK_DIRECTION
            .iter()
            .copied()
            .filter(|direction| *direction >= 0)
            .all(|direction| matches!(direction, 2 | 4))
    );
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
            final_tile_index: if is_record_tile {
                i32::from(BRIDGE_SET_START) + EAST_WALK_SLOT
            } else {
                0
            },
            final_sub_tile: if is_record_tile { 4 } else { 0 },
            ground_walk_blocked: on_bridge,
            zone_type: if on_bridge { 6 } else { 0 },
            has_bridge_deck: on_bridge,
            bridge_walkable: on_bridge,
            bridge_transition: rx == 1 || rx == 3,
            bridge_deck_level: if on_bridge { 4 } else { 0 },
            bridge_facts: crate::map::bridge_facts::BridgeCellFacts {
                raw_flags: if on_bridge {
                    crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL
                } else {
                    0
                },
                ..Default::default()
            },
            ..crate::map::resolved_terrain::test_flat_cell(rx, 0)
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
            ground_walk_blocked: is_body,
            has_bridge_deck: is_body,
            bridge_walkable: is_body || is_head,
            bridge_transition: is_head,
            bridge_deck_level: if is_body || is_head { 4 } else { 0 },
            ..crate::map::resolved_terrain::test_flat_cell(rx, 0)
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
fn bridgeheads_stay_bridge_walkable_when_a_legacy_body_is_destroyed() {
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::pathfinding::PathGrid;

    let mut terrain = make_bridge_with_bridgeheads_terrain();
    // A legacy body deck (no bit0x100) is intact while its overlay decodes.
    terrain.cell_mut(2, 0).unwrap().bridge_facts.overlay_id = Some(0x18);
    let path = PathGrid::from_resolved_terrain_with_bridges(&terrain);
    for rx in [1u16, 2, 3] {
        assert!(path.is_walkable_on_layer(rx, 0, MovementLayer::Bridge));
    }

    // 47E040's destroyed stamp: state 0, 0x100 clear, 0x400 set.
    let body = &mut terrain.cell_mut(2, 0).unwrap().bridge_facts;
    body.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_DESTROYED_OR_RAMP;
    body.state_byte = 0;
    let path = PathGrid::from_resolved_terrain_with_bridges(&terrain);
    assert!(path.is_walkable_on_layer(1, 0, MovementLayer::Bridge));
    assert!(path.is_walkable_on_layer(3, 0, MovementLayer::Bridge));
    assert!(!path.is_walkable_on_layer(2, 0, MovementLayer::Bridge));
}

fn render_facts(overlay: Option<u8>, raw_flags: u32, state_byte: u8) -> BridgeCellFacts {
    BridgeCellFacts {
        overlay_id: overlay,
        raw_flags,
        state_byte,
        ..Default::default()
    }
}

#[test]
fn repaired_overlay_renders_even_with_a_destroyed_state_byte() {
    // An ordinary low identity carries its own state; the destroyed body
    // stamp on +11E/+140 does not veto it.
    assert_eq!(
        cell_render_state(render_facts(Some(0xCD), BRIDGE_FLAG_DESTROYED_OR_RAMP, 0)),
        Some((DamageState::Healthy { variant: 0 }, Axis::NS))
    );
}

#[test]
fn destroyed_stamp_hides_a_state_byte_identity() {
    // 47E040 clears 0x100 and sets 0x400 with state 0 on every BlowUpBridge slot.
    assert_eq!(
        cell_render_state(render_facts(Some(0x18), BRIDGE_FLAG_DESTROYED_OR_RAMP, 0)),
        None
    );
    // State 0 without the destroyed stamp is the healthy NS byte.
    assert_eq!(
        cell_render_state(render_facts(Some(0x18), 0, 0)),
        Some((DamageState::Healthy { variant: 0 }, Axis::NS))
    );
    assert_eq!(
        cell_render_state(render_facts(Some(0x18), BRIDGE_FLAG_STRUCTURAL, 0)),
        Some((DamageState::Healthy { variant: 0 }, Axis::NS))
    );
    // The byte range selects the axis.
    assert_eq!(
        cell_render_state(render_facts(Some(0x19), BRIDGE_FLAG_STRUCTURAL, 15)),
        Some((DamageState::Damaged, Axis::EW))
    );
    // No identity, terminal identities and undecodable bytes draw nothing.
    assert_eq!(
        cell_render_state(render_facts(None, BRIDGE_FLAG_STRUCTURAL, 0)),
        None
    );
    assert_eq!(cell_render_state(render_facts(Some(0xE7), 0, 0)), None);
    assert_eq!(cell_render_state(render_facts(Some(0x64), 0, 0)), None);
    assert_eq!(
        cell_render_state(render_facts(Some(0x18), BRIDGE_FLAG_STRUCTURAL, 18)),
        None
    );
}

#[test]
fn indestructible_bridge_outer_gate_is_clear() {
    // The orchestrator's outer gate is `is_destroyable()`. When a
    // bridge runtime is built with `destroyable=false`, the gate
    // closes and the dispatcher bails before any path fires.
    let state = BridgeRuntimeState::from_resolved_terrain(&make_bridge_terrain(), false, 50);
    assert!(!state.is_destroyable());
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

    let broken = BridgeRuntimeState::from_resolved_terrain(&make(false), true, 300);
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
    let mixed = BridgeRuntimeState::from_resolved_terrain(&mixed_gap, true, 300);
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
    assert!(
        records.is_empty(),
        "same-cell Tube exits fail native strict ordinal order"
    );
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
fn invalidate_bridge_zones_deactivates_records_near_query() {
    // MapClass::InvalidateBridgeZones 0x0056DAE0: every active high record
    // within FindBridgeRecord's radius 3 of the query deactivates, and the
    // return requests RebuildZoneConnectivity only when one changed.
    let terrain = make_bridge_terrain();
    let mut state = BridgeRuntimeState::from_resolved_terrain(&terrain, true, 50);
    assert!(state.endpoint_records()[0].active);

    assert!(state.invalidate_bridge_zones(&terrain, (2, 0)));
    assert!(!state.endpoint_records()[0].active);
    assert_eq!(
        state.endpoint_records()[0].bridge_kind,
        BridgeRecordKind::High
    );
    assert!(
        !state.invalidate_bridge_zones(&terrain, (2, 0)),
        "an already inactive record requests no rebuild"
    );
}

#[test]
fn invalidate_bridge_zones_ignores_records_outside_radius() {
    let terrain = make_bridge_terrain();
    let mut state = BridgeRuntimeState::from_resolved_terrain(&terrain, true, 50);
    assert!(!state.invalidate_bridge_zones(&terrain, (2, 4)));
    assert!(state.endpoint_records()[0].active);
}

#[test]
fn bridge_runtime_state_snapshot_round_trip() {
    let state = BridgeRuntimeState::from_resolved_terrain(&make_bridge_terrain(), true, 1500);
    let json = serde_json::to_string(&state).expect("serialize");
    let restored: BridgeRuntimeState = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(restored.endpoint_records(), state.endpoint_records());
    assert_eq!(restored.bridge_strength(), state.bridge_strength());
    assert_eq!(restored.is_destroyable(), state.is_destroyable());
    assert_eq!(
        restored.native_zone_source_size(),
        state.native_zone_source_size()
    );
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
}

#[test]
fn bridge_state_destroyable_flag_disabled() {
    let state = BridgeRuntimeState::from_resolved_terrain(&make_bridge_terrain(), false, 800);
    assert!(!state.is_destroyable());
    assert_eq!(state.bridge_strength(), 800);
}

// Native repair coverage follows the production owners: ordinary_repair_tests
// checks strip overlays, RNG and callback order; ramp_repair_tests checks the
// 573540/570050 recovery controllers and their terrain-restoration callbacks.
// The shared iso_tile_flood tests cover connected tile replacement. Live
// Engineer integration is in world_orders_bridge_repair_tests.
//
// High rim refresh (576770 -> 576200) is covered by rim_tests and published by
// world::bridge_rim_publication. Those comparisons do not establish low-rim
// 571050 -> 570AE0 or every high selector. Tagged event31 delivery through
// the live trigger owner is tested by world::bridge_rim_publication_tests and
// bridge_publication_tests. Keep these bounds with the
// owning mechanisms rather than obsolete ignored tests claiming all repair
// and rim writes are absent.

// ---- MapClass::FindBridgeConnection_Predicate 0x00587410, overlay branch ----

fn hut_terrain() -> ResolvedTerrainGrid {
    let mut grid = crate::map::resolved_terrain::test_grid(16, 16, crate::map::resolved_terrain::test_flat_cell);
    grid.test_set_high_bridge_set_starts(Some(100), Some(200));
    grid
}

fn seed_overlay_row(
    terrain: &mut ResolvedTerrainGrid,
    y: u16,
    xs: std::ops::Range<u16>,
    overlay: u8,
) {
    for x in xs {
        terrain.cell_mut(x, y).unwrap().bridge_facts.overlay_id = Some(overlay);
    }
}

/// An intact high span carries healthy body overlays, which ARE inside the
/// 0xCD..=0xE8 family band but are not the 0xE7 / 0xE8 collapsed anchors, so
/// the native predicate walks the span and finds nothing.
#[test]
fn hut_span_scan_rejects_an_intact_high_span() {
    let mut state = hut_terrain();
    seed_overlay_row(&mut state, 6, 0..14, 0xCD);

    assert!(
        !crate::sim::world::bridge_orchestrator::hut_span_can_repair(&state, (6, 6)),
        "an intact span offers nothing to repair"
    );
}

/// A collapsed anchor anywhere along the walked span returns true, including
/// well outside the 5x5 block the scan starts from.
#[test]
fn hut_span_scan_finds_a_collapsed_high_anchor_down_the_span() {
    let mut state = hut_terrain();
    seed_overlay_row(&mut state, 6, 0..14, 0xCD);
    seed_overlay_row(&mut state, 6, 11..12, 0xE7);

    assert!(
        crate::sim::world::bridge_orchestrator::hut_span_can_repair(&state, (6, 6)),
        "0xE7 is what DestroyBridgeWalker_NS_High writes on final collapse"
    );
}

/// Low family, same shape, with the 0x64 anchor.
#[test]
fn hut_span_scan_finds_a_collapsed_low_anchor() {
    let mut state = hut_terrain();
    seed_overlay_row(&mut state, 6, 0..10, 0x4A);
    seed_overlay_row(&mut state, 6, 8..9, 0x64);

    assert!(
        crate::sim::world::bridge_orchestrator::hut_span_can_repair(&state, (6, 6)),
        "0x64 is the low-side collapsed anchor"
    );
    let mut intact = hut_terrain();
    seed_overlay_row(&mut intact, 6, 0..10, 0x4A);
    assert!(
        !crate::sim::world::bridge_orchestrator::hut_span_can_repair(&intact, (6, 6)),
        "an intact low span offers nothing to repair"
    );
}

/// The walk stops when the overlay leaves the family band, so a collapsed
/// anchor on the far side of a gap is not reached.
#[test]
fn hut_span_scan_stops_at_the_band_edge() {
    let mut state = hut_terrain();
    seed_overlay_row(&mut state, 6, 4..9, 0xCD);
    seed_overlay_row(&mut state, 6, 9..10, 0x00);
    seed_overlay_row(&mut state, 6, 10..13, 0xE7);

    assert!(
        !crate::sim::world::bridge_orchestrator::hut_span_can_repair(&state, (6, 6)),
        "an out-of-band cell ends the walk before the anchor"
    );
}

/// Cells with no overlay identity at all contribute nothing.
#[test]
fn hut_span_scan_on_empty_state_is_false() {
    let state = hut_terrain();
    assert!(!crate::sim::world::bridge_orchestrator::hut_span_can_repair(&state, (6, 6)));
}

/// 0x00587410's 5x5 loop has no break, so the cell it walks from is the LAST
/// match in Y-major order, not any match. Row y=4 leads to an anchor; row y=8
/// does not. Y-major makes (8, 8) the surviving seed, so the predicate must
/// answer false even though a reachable anchor exists from an earlier cell.
#[test]
fn hut_span_scan_walks_only_the_last_y_major_seed() {
    let mut state = hut_terrain();
    seed_overlay_row(&mut state, 4, 4..9, 0xCD);
    seed_overlay_row(&mut state, 4, 9..12, 0xCD);
    seed_overlay_row(&mut state, 4, 12..13, 0xE7);
    seed_overlay_row(&mut state, 8, 4..9, 0xCD);

    assert!(
        !crate::sim::world::bridge_orchestrator::hut_span_can_repair(&state, (6, 6)),
        "scanning every cell and accepting any hit is more permissive than the binary"
    );

    // Same map with the anchor moved onto the surviving seed's row.
    let mut reachable = hut_terrain();
    seed_overlay_row(&mut reachable, 4, 4..9, 0xCD);
    seed_overlay_row(&mut reachable, 8, 4..12, 0xCD);
    seed_overlay_row(&mut reachable, 8, 12..13, 0xE7);
    assert!(
        crate::sim::world::bridge_orchestrator::hut_span_can_repair(&reachable, (6, 6)),
        "the surviving seed's own span is walked"
    );
}

/// The EW-class arm: an overlay in {0xD6..=0xDE} u {0xE3..=0xE6} u {0xE8} walks
/// along Y, so the anchor has to be down a column rather than along a row.
#[test]
fn hut_span_scan_walks_ew_class_overlays_along_y() {
    let mut column = hut_terrain();
    for y in 4..14u16 {
        seed_overlay_row(&mut column, y, 6..7, 0xD6);
    }
    seed_overlay_row(&mut column, 13, 6..7, 0xE8);
    assert!(
        crate::sim::world::bridge_orchestrator::hut_span_can_repair(&column, (6, 6)),
        "an EW-class overlay is walked along Y"
    );

    // The same overlays laid out as a row leave the Y walk with nothing.
    let mut row = hut_terrain();
    seed_overlay_row(&mut row, 6, 4..14, 0xD6);
    seed_overlay_row(&mut row, 6, 14..15, 0xE8);
    assert!(
        !crate::sim::world::bridge_orchestrator::hut_span_can_repair(&row, (6, 6)),
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
    let mut state = hut_terrain();
    // (8, 6): NS-class, walks X, reaches an anchor outside the 5x5.
    seed_overlay_row(&mut state, 6, 8..11, 0xCD);
    seed_overlay_row(&mut state, 6, 11..12, 0xE7);
    // (6, 8): NS-class, walks X, isolated - no neighbour east or west.
    seed_overlay_row(&mut state, 8, 6..7, 0xCD);

    assert!(
        !crate::sim::world::bridge_orchestrator::hut_span_can_repair(&state, (6, 6)),
        "Y-major keeps (6, 8), which dead-ends; an X-major scan would keep          (8, 6) and wrongly answer true"
    );
}
