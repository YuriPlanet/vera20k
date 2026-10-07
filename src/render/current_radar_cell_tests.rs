use super::*;

use crate::map::bridge_facts::{
    BRIDGE_FLAG_DESTROYED_OR_RAMP, BRIDGE_FLAG_EXTRA_SIDE, BRIDGE_FLAG_STRUCTURAL,
    BridgeAnchorRelation, BridgeStampFamily, BridgeStampSlot,
};
use crate::map::playfield::PlayfieldBounds;
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::map::terrain::{TerrainGrid, build_terrain_grid_from_resolved};
use crate::render::minimap::{MinimapCellRadarSource, MinimapOverlayDatum};
use crate::render::minimap_helpers::OverlayClassification;
use crate::render::minimap_projection::MinimapPlayfieldProjection;
use crate::render::radar_terrain_updates::{
    RadarTerrainUpdateLayers, apply_radar_terrain_dirty_cells,
};
use crate::sim::bridge_state::BridgeRuntimeState;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::Simulation;

const SIDE: u16 = 64;
const BRIDGE_COLOR: [u8; 3] = [5, 6, 7];
const LOW_BRIDGE_COLOR: [u8; 3] = [31, 32, 33];
const LOW_BRIDGE_CD_COLOR: [u8; 3] = [41, 42, 43];
const OVERLAY_COLOR: [u8; 3] = [11, 12, 13];
const BASE_COLOR: [u8; 3] = [10, 20, 30];
const SAVED_COLLAPSED_TMP_RAW: [u8; 3] = [200, 100, 50];
const SAVED_COLLAPSED_TMP_COLOR: [u8; 3] = [100, 50, 25];

fn expanded_bounds() -> PlayfieldBounds {
    PlayfieldBounds {
        base: 40,
        off_fc: 2,
        off_100: 2,
        off_104: 36,
        off_108: 30,
    }
}

fn shrunken_bounds() -> PlayfieldBounds {
    PlayfieldBounds {
        base: 40,
        off_fc: 10,
        off_100: 10,
        off_104: 10,
        off_108: 8,
    }
}

fn fixture(static_bridge: Option<(u16, u16)>) -> (TerrainGrid, ResolvedTerrainGrid) {
    let cells = (0..SIDE)
        .flat_map(|ry| {
            (0..SIDE).map(move |rx| {
                let mut cell = flat_cell(rx, ry);
                if static_bridge == Some((rx, ry)) {
                    mark_high_bridge_source(&mut cell);
                }
                cell
            })
        })
        .collect();
    let resolved = ResolvedTerrainGrid::from_cells(SIDE, SIDE, cells);
    let grid = build_terrain_grid_from_resolved(&resolved, None, None);
    (grid, resolved)
}

fn mark_high_bridge_source(cell: &mut ResolvedTerrainCell) {
    cell.bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
    cell.bridge_facts.family = BridgeStampFamily::Nesw;
    cell.bridge_facts.direction = Some(0);
    cell.bridge_facts.overlay_id = Some(0xCD);
    cell.has_bridge_deck = true;
    cell.bridge_walkable = true;
    cell.bridge_deck_level = cell.level.saturating_add(4);
}

fn mark_extra_dir6_family_only(cell: &mut ResolvedTerrainCell) {
    cell.bridge_facts.raw_flags = BRIDGE_FLAG_EXTRA_SIDE;
    cell.bridge_facts.family = BridgeStampFamily::Nesw;
    cell.bridge_facts.direction = Some(6);
    cell.bridge_facts.anchor = Some(BridgeAnchorRelation {
        anchor: (cell.rx, cell.ry),
        slot: BridgeStampSlot::ExtraDir6,
        family: BridgeStampFamily::Nesw,
        direction: 6,
    });
}

fn central_cell(grid: &TerrainGrid, bounds: PlayfieldBounds) -> (u16, u16) {
    grid.cells
        .iter()
        .find(|cell| {
            bounds.contains_geometry_packed(i32::from(cell.rx), i32::from(cell.ry))
                && cell.rx > 8
                && cell.ry > 8
                && cell.rx < SIDE - 8
                && cell.ry < SIDE - 8
        })
        .map(|cell| (cell.rx, cell.ry))
        .expect("interior playfield cell")
}

fn live_runtime(resolved: ResolvedTerrainGrid, overlay_grid: OverlayGrid) -> SimRuntime {
    let mut sim = Simulation::new();
    sim.resolved_terrain = Some(resolved);
    sim.bridge_state = Some(BridgeRuntimeState::default());
    sim.overlay_grid = Some(overlay_grid);
    SimRuntime::from_simulation(sim)
}

/// CellClass+44 of an intact (0xCD) or collapsed (0xE7) bridge cell.
fn with_bridge_overlay(
    mut resolved: ResolvedTerrainGrid,
    cell: (u16, u16),
    intact: bool,
) -> ResolvedTerrainGrid {
    resolved
        .cell_mut(cell.0, cell.1)
        .unwrap()
        .bridge_facts
        .overlay_id = Some(if intact { 0xCD } else { 0xE7 });
    resolved
}

/// Native47E040 destruction: bit100 clears and bit400 records the collapse.
fn collapse_flags(resolved: &mut ResolvedTerrainGrid, cell: (u16, u16)) {
    let facts = &mut resolved.cell_mut(cell.0, cell.1).unwrap().bridge_facts;
    facts.raw_flags = (facts.raw_flags & !BRIDGE_FLAG_STRUCTURAL) | BRIDGE_FLAG_DESTROYED_OR_RAMP;
}

fn presentation_overlay(cell: (u16, u16), overlay_id: u8, frame: u8) -> MinimapOverlayDatum {
    MinimapOverlayDatum {
        rx: cell.0,
        ry: cell.1,
        classification: OverlayClassification::Wall,
        source: MinimapCellRadarSource::Overlay {
            overlay_id,
            frame,
            is_tiberium: false,
            has_cell_anim: false,
            has_tiberium_type: false,
        },
    }
}

fn colors() -> HashMap<(u8, u8), [u8; 3]> {
    HashMap::from([
        ((24, 0), BRIDGE_COLOR),
        ((0x4A, 1), LOW_BRIDGE_COLOR),
        ((0xCD, 1), LOW_BRIDGE_CD_COLOR),
        ((10, 4), OVERLAY_COLOR),
    ])
}

fn projection<'a>(
    grid: &TerrainGrid,
    runtime: &'a SimRuntime,
    presentation: &[MinimapOverlayDatum],
    bounds: PlayfieldBounds,
    colors: &HashMap<(u8, u8), [u8; 3]>,
) -> MinimapPlayfieldProjection {
    MinimapPlayfieldProjection::derive(
        grid,
        runtime.simulation.resolved_terrain.as_ref(),
        presentation,
        colors,
        "TEMPERATE",
        Some(bounds),
        Some(CurrentRadarCellAuthority::from_runtime(runtime)),
    )
}

fn raw_pair(projection: &MinimapPlayfieldProjection, cell: (u16, u16)) -> [[u8; 3]; 2] {
    let geometry = projection
        .native_radar_surface
        .expect("native radar surface");
    let raw = projection
        .native_radar_terrain
        .as_ref()
        .expect("native radar terrain")
        .raw_rgb();
    let (x, y) = geometry.cell_to_raw_pixel(cell);
    assert!(x >= 0 && x + 1 < geometry.raw_size().0);
    assert!(y >= 0 && y < geometry.raw_size().1);
    let index = (y * geometry.raw_size().0 + x) as usize;
    [raw[index], raw[index + 1]]
}

fn apply_incremental(
    projection: &mut MinimapPlayfieldProjection,
    runtime: &SimRuntime,
    cell: (u16, u16),
    colors: &HashMap<(u8, u8), [u8; 3]>,
) {
    apply_radar_terrain_dirty_cells(
        RadarTerrainUpdateLayers {
            base_rgba: &mut projection.base_rgba,
            terrain_pixels: &projection.terrain_pixels,
            surface_pixels: &mut projection.surface_pixels,
            overlay_pixels: &mut projection.overlay_pixels,
            native_surface: projection.native_radar_surface,
            native_terrain: &mut projection.native_radar_terrain,
        },
        CurrentRadarCellAuthority::from_runtime(runtime),
        BRIDGE_COLOR,
        colors,
        &[cell],
    );
}

fn runtime_with_overlay(
    resolved: ResolvedTerrainGrid,
    cell: (u16, u16),
    bridge: Option<bool>,
    overlay: Option<(u8, u8)>,
) -> SimRuntime {
    let mut overlay_grid = OverlayGrid::new(SIDE, SIDE);
    if let Some((overlay_id, overlay_data)) = overlay {
        overlay_grid.place_overlay(cell.0, cell.1, overlay_id, overlay_data);
    }
    let resolved = match bridge {
        Some(intact) => with_bridge_overlay(resolved, cell, intact),
        None => resolved,
    };
    live_runtime(resolved, overlay_grid)
}

fn structural_runtime_with_stale_grid(
    mut resolved: ResolvedTerrainGrid,
    cell: (u16, u16),
    live_overlay: u8,
) -> SimRuntime {
    resolved
        .cell_mut(cell.0, cell.1)
        .unwrap()
        .bridge_facts
        .overlay_id = (live_overlay != u8::MAX).then_some(live_overlay);
    let mut stale_overlay_grid = OverlayGrid::new(SIDE, SIDE);
    stale_overlay_grid.place_overlay(cell.0, cell.1, 0xCD, 0);
    live_runtime(resolved, stale_overlay_grid)
}

fn snapshot_restored_collapsed_high_runtime(
    terrain_template: &ResolvedTerrainGrid,
    cell: (u16, u16),
    live_overlay: u8,
) -> SimRuntime {
    let mut sim = Simulation::new();
    sim.session.map_name = "RADARLOAD.MAP".to_string();
    sim.install_resolved_terrain_for_new_map(terrain_template.clone());
    sim.bridge_state = Some(BridgeRuntimeState::from_resolved_terrain(
        terrain_template,
        true,
        1500,
    ));

    let mut stale_overlay_grid = OverlayGrid::new(SIDE, SIDE);
    stale_overlay_grid.place_overlay(cell.0, cell.1, 0xCD, 0);
    sim.overlay_grid = Some(stale_overlay_grid);
    let saved_dynamic_cell = {
        let live_terrain = sim.resolved_terrain.as_mut().expect("installed terrain");
        collapse_flags(live_terrain, cell);
        let saved_cell = live_terrain
            .cell_mut(cell.0, cell.1)
            .expect("saved high bridge cell");
        saved_cell.bridge_facts.overlay_id = (live_overlay != u8::MAX).then_some(live_overlay);
        saved_cell.radar_left = SAVED_COLLAPSED_TMP_RAW;
        saved_cell.radar_right = [7, 8, 9];
        crate::map::resolved_terrain::DynamicTerrainCellState::capture(saved_cell)
    };
    sim.dynamic_terrain_cells.insert(cell, saved_dynamic_cell);
    sim.real_cell_bridge_flags_0x1180 = sim
        .resolved_terrain
        .as_ref()
        .expect("installed terrain")
        .capture_real_cell_bridge_flags_0x1180();

    let terrain_speed_config = sim.terrain_speed_config.clone();
    let bytes = GameSnapshot::save_validated(&sim, 1, 2, "Radar load", 3);
    let mut restored = GameSnapshot::load_validated(&bytes, 1, 2, "RADARLOAD.MAP")
        .expect("validated current snapshot")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("restored object graph");
    let resources = SimResources::empty();
    restored.rebuild_caches_after_load(
        terrain_template.clone(),
        terrain_speed_config,
        &resources.rules,
    );
    restored
        .restore_map_authority_after_snapshot_load(&resources.rules, &resources.overlay_registry)
        .expect("restored current map authority");
    SimRuntime {
        simulation: restored,
        resources,
    }
}

#[test]
fn gsi_04_01_structural_high_bridge_wins_in_full_and_incremental_paths() {
    let (grid, mut resolved) = fixture(None);
    let cell = central_cell(&grid, expanded_bounds());
    mark_high_bridge_source(resolved.cell_mut(cell.0, cell.1).unwrap());
    let colors = colors();
    let mut collapsed = resolved.clone();
    // Native47E040 clears bit100 before the collapsed radar read.
    collapse_flags(&mut collapsed, cell);
    let stale_destroyed = runtime_with_overlay(collapsed, cell, Some(false), Some((0x4A, 9)));
    let current_high = runtime_with_overlay(resolved, cell, Some(true), Some((0x4A, 9)));

    let full = projection(&grid, &current_high, &[], expanded_bounds(), &colors);
    assert_eq!(raw_pair(&full, cell), [BRIDGE_COLOR; 2]);

    let mut incremental = projection(&grid, &stale_destroyed, &[], expanded_bounds(), &colors);
    assert_eq!(raw_pair(&incremental, cell), [BASE_COLOR; 2]);
    apply_incremental(&mut incremental, &current_high, cell, &colors);
    assert_eq!(raw_pair(&incremental, cell), [BRIDGE_COLOR; 2]);
}

#[test]
fn native_constructor_side_cells_use_structural_radar_without_own_sprite() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_constructor.json",
    ))
    .unwrap();
    let colors = HashMap::new();
    let mut visited = 0;
    for case in &corpus["cases"].as_array().unwrap()[8..12] {
        assert_eq!(case["kind"], "success");
        for native in case["cells"].as_array().unwrap() {
            let flags = native["flags"].as_u64().unwrap() as u32;
            if flags & BRIDGE_FLAG_STRUCTURAL == 0 || native["overlay"] != -1 {
                continue;
            }
            let cell = (
                native["coord"][0].as_u64().unwrap() as u16,
                native["coord"][1].as_u64().unwrap() as u16,
            );
            let (_, mut resolved) = fixture(None);
            let current = resolved.cell_mut(cell.0, cell.1).unwrap();
            current.bridge_facts.raw_flags = flags;
            current.bridge_facts.state_byte = native["state"].as_u64().unwrap() as u8;
            current.bridge_facts.overlay_id = None;
            assert!(crate::sim::bridge_state::cell_render_state(current.bridge_facts).is_none());

            // 47C060 takes its structural color branch before testing +44.
            // No sprite of its own is required for this CellClass read.
            let authority = CurrentRadarCellAuthority::new(Some(&resolved), None, None, None);
            assert_eq!(
                authority.source(cell.0, cell.1, BRIDGE_COLOR, &colors),
                Some((BRIDGE_COLOR, OverlayClassification::Bridge)),
                "{native}"
            );
            resolved
                .cell_mut(cell.0, cell.1)
                .unwrap()
                .bridge_facts
                .raw_flags &= !BRIDGE_FLAG_STRUCTURAL;
            assert_eq!(
                CurrentRadarCellAuthority::new(Some(&resolved), None, None, None).source(
                    cell.0,
                    cell.1,
                    BRIDGE_COLOR,
                    &colors
                ),
                None,
                "no retained deck data can revive a cleared native structural bit"
            );
            visited += 1;
        }
    }
    assert_eq!(visited, 12);
}

#[test]
fn gsi_04_01_intact_low_overlay_stays_overlay_in_full_and_incremental_paths() {
    let (grid, resolved) = fixture(None);
    let cell = central_cell(&grid, expanded_bounds());
    let colors = colors();
    let absent = runtime_with_overlay(resolved.clone(), cell, None, None);
    for (overlay_id, expected) in [(0x4A, LOW_BRIDGE_COLOR), (0xCD, LOW_BRIDGE_CD_COLOR)] {
        let intact_low =
            runtime_with_overlay(resolved.clone(), cell, Some(true), Some((overlay_id, 9)));

        let full = projection(&grid, &intact_low, &[], expanded_bounds(), &colors);
        assert_eq!(raw_pair(&full, cell), [expected; 2]);
        assert_ne!(raw_pair(&full, cell), [BRIDGE_COLOR; 2]);

        let mut incremental = projection(&grid, &absent, &[], expanded_bounds(), &colors);
        assert_eq!(raw_pair(&incremental, cell), [BASE_COLOR; 2]);
        apply_incremental(&mut incremental, &intact_low, cell, &colors);
        assert_eq!(raw_pair(&incremental, cell), [expected; 2]);
    }
}

#[test]
fn gsi_04_01_extra_dir6_family_without_deck_flags_keeps_live_overlay() {
    let (grid, mut resolved) = fixture(None);
    let cell = central_cell(&grid, expanded_bounds());
    let auxiliary = resolved
        .cell_mut(cell.0, cell.1)
        .expect("ExtraDir6 auxiliary cell");
    mark_extra_dir6_family_only(auxiliary);
    assert!(!auxiliary.bridge_facts.has_structural_bridge());
    assert!(!auxiliary.has_bridge_deck);

    let colors = colors();
    let absent = runtime_with_overlay(resolved.clone(), cell, None, None);
    let live_overlay = runtime_with_overlay(resolved, cell, None, Some((10, 4)));

    let full = projection(&grid, &live_overlay, &[], expanded_bounds(), &colors);
    assert_eq!(raw_pair(&full, cell), [OVERLAY_COLOR; 2]);

    let mut incremental = projection(&grid, &absent, &[], expanded_bounds(), &colors);
    assert_eq!(raw_pair(&incremental, cell), [BASE_COLOR; 2]);
    apply_incremental(&mut incremental, &live_overlay, cell, &colors);
    assert_eq!(raw_pair(&incremental, cell), [OVERLAY_COLOR; 2]);
}

#[test]
fn gsi_04_01_high_collapse_uses_cellclass_overlay_then_repairs_in_full_and_incremental_paths() {
    let (grid, mut resolved) = fixture(None);
    let cell = central_cell(&grid, expanded_bounds());
    mark_high_bridge_source(resolved.cell_mut(cell.0, cell.1).unwrap());
    let colors = colors();
    let repaired = structural_runtime_with_stale_grid(resolved.clone(), cell, 0xCD);

    let repaired_full = projection(&grid, &repaired, &[], expanded_bounds(), &colors);
    assert_eq!(raw_pair(&repaired_full, cell), [BRIDGE_COLOR; 2]);

    for destroyed_overlay in [0xE7, 0xE8, u8::MAX] {
        let mut collapsed = resolved.clone();
        // Keep the same native collapse input as the snapshot case below:
        // the structural flag is cleared independently of overlay identity.
        collapse_flags(&mut collapsed, cell);
        let destroyed = structural_runtime_with_stale_grid(collapsed, cell, destroyed_overlay);
        // This is the same full projection used for load/action-40 rebuilds.
        let full = projection(&grid, &destroyed, &[], expanded_bounds(), &colors);
        assert_eq!(raw_pair(&full, cell), [BASE_COLOR; 2]);
        assert!(
            full.overlay_pixels
                .iter()
                .all(|pixel| (pixel.rx, pixel.ry) != cell),
            "live collapsed overlay {destroyed_overlay:#04X} must beat stale OverlayGrid 0xCD",
        );

        let mut incremental = projection(&grid, &repaired, &[], expanded_bounds(), &colors);
        assert_eq!(raw_pair(&incremental, cell), [BRIDGE_COLOR; 2]);
        apply_incremental(&mut incremental, &destroyed, cell, &colors);
        assert_eq!(raw_pair(&incremental, cell), [BASE_COLOR; 2]);
        apply_incremental(&mut incremental, &repaired, cell, &colors);
        assert_eq!(raw_pair(&incremental, cell), [BRIDGE_COLOR; 2]);
    }
}

#[test]
fn gsi_04_01_snapshot_high_collapse_uses_cellclass_overlay_after_structural_flag_clear() {
    let (grid, mut terrain_template) = fixture(None);
    let cell = central_cell(&grid, expanded_bounds());
    let source_cell = terrain_template
        .cell_mut(cell.0, cell.1)
        .expect("high bridge source cell");
    mark_high_bridge_source(source_cell);

    let colors = colors();
    let repaired = structural_runtime_with_stale_grid(terrain_template.clone(), cell, 0xCD);
    assert_eq!(
        raw_pair(
            &projection(&grid, &repaired, &[], expanded_bounds(), &colors),
            cell,
        ),
        [BRIDGE_COLOR; 2],
    );

    for saved_overlay in [0xE7, 0xE8, u8::MAX] {
        let restored =
            snapshot_restored_collapsed_high_runtime(&terrain_template, cell, saved_overlay);
        let restored_cell = restored
            .simulation
            .resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.cell(cell.0, cell.1))
            .expect("restored high bridge cell");
        assert!(!restored_cell.bridge_facts.has_structural_bridge());
        assert_eq!(restored_cell.bridge_facts.family, BridgeStampFamily::Nesw);
        assert_eq!(restored_cell.radar_left, SAVED_COLLAPSED_TMP_RAW);
        assert_eq!(
            restored
                .simulation
                .overlay_grid
                .as_ref()
                .expect("restored overlay grid")
                .cell(cell.0, cell.1)
                .overlay_id,
            Some(0xCD),
            "high-bridge load intentionally leaves the original deck byte in OverlayGrid",
        );
        assert!(restored.simulation.radar_terrain_dirty_cells.is_empty());

        let full = projection(&grid, &restored, &[], expanded_bounds(), &colors);
        assert_eq!(
            raw_pair(&full, cell),
            [SAVED_COLLAPSED_TMP_COLOR; 2],
            "saved CellClass overlay {saved_overlay:#04X} must fall through to saved TMP",
        );

        let mut incremental = projection(&grid, &repaired, &[], expanded_bounds(), &colors);
        apply_incremental(&mut incremental, &restored, cell, &colors);
        assert_eq!(
            raw_pair(&incremental, cell),
            [SAVED_COLLAPSED_TMP_COLOR; 2],
            "incremental source must share the snapshot-restored high-family authority",
        );
    }
}

#[test]
fn gsi_04_01_destroyed_low_overlay_falls_through_in_full_and_incremental_paths() {
    let (grid, resolved) = fixture(None);
    let cell = central_cell(&grid, expanded_bounds());
    let colors = colors();
    let intact_low = runtime_with_overlay(resolved.clone(), cell, Some(true), Some((0x4A, 0)));
    let destroyed_low = runtime_with_overlay(resolved, cell, Some(false), Some((100, 1)));

    let full = projection(&grid, &destroyed_low, &[], expanded_bounds(), &colors);
    assert_eq!(raw_pair(&full, cell), [BASE_COLOR; 2]);

    let mut incremental = projection(&grid, &intact_low, &[], expanded_bounds(), &colors);
    assert_eq!(raw_pair(&incremental, cell), [LOW_BRIDGE_COLOR; 2]);
    apply_incremental(&mut incremental, &destroyed_low, cell, &colors);
    assert_eq!(raw_pair(&incremental, cell), [BASE_COLOR; 2]);
}

#[test]
fn gsi_04_01_absent_live_cell_clears_full_and_incremental_bridge_sources() {
    let (grid, resolved) = fixture(None);
    let cell = central_cell(&grid, expanded_bounds());
    let colors = colors();
    let intact_low = runtime_with_overlay(resolved.clone(), cell, Some(true), Some((0x4A, 0)));
    let absent = runtime_with_overlay(resolved, cell, None, None);

    let full = projection(&grid, &absent, &[], expanded_bounds(), &colors);
    assert_eq!(raw_pair(&full, cell), [BASE_COLOR; 2]);

    let mut incremental = projection(&grid, &intact_low, &[], expanded_bounds(), &colors);
    assert_eq!(raw_pair(&incremental, cell), [LOW_BRIDGE_COLOR; 2]);
    apply_incremental(&mut incremental, &absent, cell, &colors);
    assert_eq!(raw_pair(&incremental, cell), [BASE_COLOR; 2]);
}

#[test]
fn gsi_04_01_load_intact_bridge_discards_abandoned_destroyed_pixels() {
    let (grid, mut resolved) = fixture(None);
    let cell = central_cell(&grid, expanded_bounds());
    mark_high_bridge_source(resolved.cell_mut(cell.0, cell.1).unwrap());
    let overlay = OverlayGrid::new(SIDE, SIDE);
    let mut collapsed = resolved.clone();
    collapse_flags(&mut collapsed, cell);
    let abandoned = live_runtime(with_bridge_overlay(collapsed, cell, false), overlay.clone());
    let restored = live_runtime(with_bridge_overlay(resolved, cell, true), overlay);
    assert!(restored.simulation.radar_terrain_dirty_cells.is_empty());

    let colors = colors();
    let stale_destroyed = [presentation_overlay(cell, 239, 0)];
    assert_eq!(
        raw_pair(
            &projection(
                &grid,
                &abandoned,
                &stale_destroyed,
                expanded_bounds(),
                &colors
            ),
            cell,
        ),
        [BASE_COLOR; 2],
        "abandoned destroyed timeline used its current fallen bridge state",
    );
    assert_eq!(
        raw_pair(
            &projection(
                &grid,
                &restored,
                &stale_destroyed,
                expanded_bounds(),
                &colors
            ),
            cell,
        ),
        [BRIDGE_COLOR; 2],
        "first restored primary surface comes from saved intact bridge state",
    );
}

#[test]
fn gsi_04_01_load_destroyed_bridge_discards_abandoned_repair_pixels() {
    let (grid, mut resolved) = fixture(None);
    let cell = central_cell(&grid, expanded_bounds());
    mark_high_bridge_source(resolved.cell_mut(cell.0, cell.1).unwrap());
    let overlay = OverlayGrid::new(SIDE, SIDE);
    let abandoned = live_runtime(
        with_bridge_overlay(resolved.clone(), cell, true),
        overlay.clone(),
    );
    collapse_flags(&mut resolved, cell);
    let restored = live_runtime(with_bridge_overlay(resolved, cell, false), overlay);
    assert!(restored.simulation.radar_terrain_dirty_cells.is_empty());

    let colors = colors();
    let stale_repaired = [presentation_overlay(cell, 0xCD, 1)];
    assert_eq!(
        raw_pair(
            &projection(
                &grid,
                &abandoned,
                &stale_repaired,
                expanded_bounds(),
                &colors
            ),
            cell,
        ),
        [BRIDGE_COLOR; 2],
    );
    let restored_projection = projection(
        &grid,
        &restored,
        &stale_repaired,
        expanded_bounds(),
        &colors,
    );
    assert_eq!(raw_pair(&restored_projection, cell), [BASE_COLOR; 2]);
    assert!(
        restored_projection
            .overlay_pixels
            .iter()
            .all(|pixel| (pixel.rx, pixel.ry) != cell),
        "static bridge facts and abandoned repaired overlay cannot revive the saved collapse",
    );
}

#[test]
fn gsi_04_01_load_live_overlay_absence_beats_stale_presentation_tombstone() {
    let (grid, resolved) = fixture(None);
    let cell = central_cell(&grid, expanded_bounds());
    let mut prior_overlay = OverlayGrid::new(SIDE, SIDE);
    prior_overlay.place_overlay(cell.0, cell.1, 10, 4);
    let prior = live_runtime(resolved.clone(), prior_overlay);
    let restored = live_runtime(resolved, OverlayGrid::new(SIDE, SIDE));
    let stale = [presentation_overlay(cell, 10, 4)];
    let colors = colors();

    assert_eq!(
        raw_pair(
            &projection(&grid, &prior, &stale, expanded_bounds(), &colors),
            cell,
        ),
        [OVERLAY_COLOR; 2],
    );
    let restored_projection = projection(&grid, &restored, &stale, expanded_bounds(), &colors);
    assert_eq!(raw_pair(&restored_projection, cell), [BASE_COLOR; 2]);
    assert!(
        restored_projection
            .overlay_pixels
            .iter()
            .all(|pixel| (pixel.rx, pixel.ry) != cell),
        "full rebuild enumerates restored OverlayGrid, not retained presentation entries",
    );
}

#[test]
fn gsi_04_01_load_restored_local_size_builds_geometry_before_live_cell_pixels() {
    let (grid, resolved) = fixture(None);
    let cell = central_cell(&grid, shrunken_bounds());
    let mut overlay = OverlayGrid::new(SIDE, SIDE);
    overlay.place_overlay(cell.0, cell.1, 10, 4);
    let runtime = live_runtime(resolved, overlay);
    let colors = colors();

    let abandoned = projection(&grid, &runtime, &[], expanded_bounds(), &colors);
    let restored = projection(&grid, &runtime, &[], shrunken_bounds(), &colors);
    let abandoned_geometry = abandoned.native_radar_surface.expect("old geometry");
    let restored_geometry = restored.native_radar_surface.expect("restored geometry");
    assert_ne!(abandoned_geometry.raw_size(), restored_geometry.raw_size());
    assert_ne!(
        abandoned_geometry.cell_to_raw_pixel(cell),
        restored_geometry.cell_to_raw_pixel(cell),
    );
    assert_eq!(raw_pair(&restored, cell), [OVERLAY_COLOR; 2]);
    assert!(
        restored
            .overlay_pixels
            .iter()
            .any(|pixel| (pixel.rx, pixel.ry) == cell),
        "restored current source is projected through restored LocalSize",
    );
}

#[test]
fn gsi_04_01_full_and_incremental_paths_share_current_cell_source_precedence() {
    let (grid, mut resolved) = fixture(None);
    let cell = central_cell(&grid, expanded_bounds());
    resolved
        .cell_mut(cell.0, cell.1)
        .unwrap()
        .terrain_object_occupation = Some(9);
    let mut overlay = OverlayGrid::new(SIDE, SIDE);
    overlay.place_overlay(cell.0, cell.1, 10, 4);
    let runtime = live_runtime(with_bridge_overlay(resolved, cell, true), overlay);
    let authority = CurrentRadarCellAuthority::from_runtime(&runtime);
    let colors = colors();
    assert_eq!(
        authority.source(cell.0, cell.1, BRIDGE_COLOR, &colors),
        Some(([200, 200, 160], OverlayClassification::TerrainObject)),
    );
    assert_eq!(
        raw_pair(
            &projection(&grid, &runtime, &[], expanded_bounds(), &colors),
            cell,
        ),
        [[200, 200, 160]; 2],
    );
}

fn flat_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        radar_left: [20, 40, 60],
        radar_right: [90, 90, 90],
        ..crate::map::resolved_terrain::test_smudge_cell(rx, ry)
    }
}
