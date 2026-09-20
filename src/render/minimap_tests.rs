//! Unit tests for the minimap renderer.
//!
//! Tests coordinate mapping, owner color logic, pixel setting, and viewport math.
//! GPU-dependent tests (full MinimapRenderer construction) are not possible here.

use super::*;
use crate::map::houses::HouseAllianceMap;
use crate::map::playfield::PlayfieldBounds;
use crate::map::terrain::{PlayfieldPresentationGeometry, TerrainCell, TerrainGrid};
use crate::render::minimap_helpers::*;
use crate::rules::color_scheme::ColorSchemeEntry;
use crate::rules::house_colors::{HouseColorIndex, HouseColorRamps};
use crate::sim::components::Position;
use crate::sim::intern::test_intern;
use crate::sim::vision::FogState;
use std::collections::{BTreeMap, BTreeSet, HashMap};

fn overlay_datum(
    rx: u16,
    ry: u16,
    classification: OverlayClassification,
    overlay_id: u8,
    frame: u8,
    is_tiberium: bool,
) -> MinimapOverlayDatum {
    MinimapOverlayDatum {
        rx,
        ry,
        classification,
        source: MinimapCellRadarSource::Overlay {
            overlay_id,
            frame,
            is_tiberium,
            has_cell_anim: false,
            has_tiberium_type: is_tiberium,
        },
    }
}

fn playfield_projection_grid(side: u16) -> TerrainGrid {
    let mut cells = Vec::new();
    for ry in 0..side {
        for rx in 0..side {
            let (screen_x, screen_y) = crate::map::terrain::iso_to_screen(rx, ry, 0);
            cells.push(TerrainCell {
                screen_x,
                screen_y,
                tile_id: 1,
                sub_tile: 0,
                z: 0,
                rx,
                ry,
                is_water: false,
                variant: 0,
                tint: [1.0; 3],
                radar_left: [20, 80, 20],
                radar_right: [40, 100, 40],
                has_damaged_data: false,
            });
        }
    }
    TerrainGrid {
        cells,
        world_width: 1.0,
        world_height: 1.0,
        origin_x: 0.0,
        origin_y: 0.0,
        local_bounds: None,
        anchor_variant_table: None,
    }
}

fn flat_resolved_terrain(side: u16) -> crate::map::resolved_terrain::ResolvedTerrainGrid {
    use crate::map::bridge_facts::BridgeCellFacts;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};

    let cells = (0..side)
        .flat_map(|ry| {
            (0..side).map(move |rx| ResolvedTerrainCell {
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
                height_in_pixels: 0,
                render_offset_x: 0,
                render_offset_y: 0,
                terrain_class: TerrainClass::Clear,
                speed_costs: SpeedCostProfile::default(),
                is_water: false,
                is_cliff_like: false,
                is_rough: false,
                is_road: false,
                accepts_smudge: true,
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
                zone_type: 0,
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
                bridge_facts: BridgeCellFacts::default(),
                tube_index: None,
                radar_left: [0; 3],
                radar_right: [0; 3],
                has_damaged_data: false,
                bridgehead_anchor_class_at_load: None,
            })
        })
        .collect();
    ResolvedTerrainGrid::from_cells(side, side, cells)
}

fn expanded_playfield() -> PlayfieldBounds {
    PlayfieldBounds {
        base: 40,
        off_fc: 2,
        off_100: 2,
        off_104: 36,
        off_108: 30,
    }
}

fn shrunken_playfield() -> PlayfieldBounds {
    PlayfieldBounds {
        base: 40,
        off_fc: 10,
        off_100: 10,
        off_104: 10,
        off_108: 8,
    }
}

fn make_pixel(rx: u16, ry: u16, color: [u8; 4]) -> TerrainPixel {
    TerrainPixel {
        rx,
        ry,
        px: 0,
        py: 0,
        color,
    }
}

#[test]
fn test_world_to_minimap_pixel_origin() {
    // World position at origin maps to minimap pixel (0, 0).
    let (px, py): (u32, u32) =
        world_to_minimap_pixel(0.0, 0.0, 0.0, 0.0, 1000.0, 1000.0, 0.0, 0.0, 200.0, 200.0);
    assert_eq!(px, 0);
    assert_eq!(py, 0);
}

#[test]
fn test_world_to_minimap_pixel_center() {
    // Position at center of world maps to center of minimap.
    let (px, py): (u32, u32) = world_to_minimap_pixel(
        500.0, 500.0, 0.0, 0.0, 1000.0, 1000.0, 0.0, 0.0, 200.0, 200.0,
    );
    assert_eq!(px, 100);
    assert_eq!(py, 100);
}

#[test]
fn test_world_to_minimap_pixel_clamps_negative() {
    // Positions outside world bounds are clamped to 0.
    let (px, py): (u32, u32) = world_to_minimap_pixel(
        -500.0, -500.0, 0.0, 0.0, 1000.0, 1000.0, 0.0, 0.0, 200.0, 200.0,
    );
    assert_eq!(px, 0);
    assert_eq!(py, 0);
}

#[test]
fn test_world_to_minimap_pixel_clamps_overflow() {
    // Positions beyond world extent are clamped to the native aperture edge.
    let (px, py): (u32, u32) = world_to_minimap_pixel(
        2000.0, 2000.0, 0.0, 0.0, 1000.0, 1000.0, 0.0, 0.0, 200.0, 200.0,
    );
    assert_eq!(px, MINIMAP_WIDTH - 1);
    assert_eq!(py, MINIMAP_HEIGHT - 1);
}

#[test]
fn test_world_to_minimap_pixel_with_offset() {
    // World origin is offset — position at origin_x maps to pixel 0.
    let (px, py): (u32, u32) = world_to_minimap_pixel(
        -500.0, 200.0, -500.0, 200.0, 2000.0, 2000.0, 0.0, 0.0, 200.0, 200.0,
    );
    assert_eq!(px, 0);
    assert_eq!(py, 0);
}

/// Ramps where entry 0 = Gold, entry 1 = DarkBlue, entry 2 = DarkRed (HSV from
/// the retail `[Colors]` list) so dot colors carry the expected hue dominance.
fn test_ramps() -> HouseColorRamps {
    let mk = |name: &str, hsv: [u8; 3]| ColorSchemeEntry {
        name: name.into(),
        hsv,
    };
    HouseColorRamps::from_schemes(&[
        mk("Gold", [43, 239, 255]),
        mk("DarkBlue", [153, 214, 212]),
        mk("DarkRed", [0, 230, 255]),
    ])
}

#[test]
fn test_owner_dot_color_uses_house_map() {
    let mut map: HouseColorMap = HouseColorMap::new();
    map.insert("Americans".to_string(), HouseColorIndex(1)); // DarkBlue entry
    map.insert("Russians".to_string(), HouseColorIndex(2)); // DarkRed entry
    let ramps = test_ramps();

    let blue: [u8; 4] = owner_dot_color("Americans", &map, &ramps);
    let red: [u8; 4] = owner_dot_color("Russians", &map, &ramps);
    // Blue should have more B than R, red should have more R than B.
    assert!(blue[2] > blue[0], "Americans should be blue-ish: {blue:?}");
    assert!(red[0] > red[2], "Russians should be red-ish: {red:?}");
}

#[test]
fn test_owner_dot_color_unknown_defaults_to_default_scheme() {
    let mk = |name: &str, hsv: [u8; 3]| ColorSchemeEntry {
        name: name.into(),
        hsv,
    };
    // Entry 2 = DEFAULT_SCHEME_ENTRY (LightGrey-ish): the producers' fallback.
    let ramps = HouseColorRamps::from_schemes(&[
        mk("LightGold", [25, 255, 255]),
        mk("Gold", [43, 239, 255]),
        mk("LightGrey", [0, 0, 240]),
    ]);
    let map: HouseColorMap = HouseColorMap::new();
    let dot: [u8; 4] = owner_dot_color("Unknown", &map, &ramps);
    // Unknown owner resolves to the default scheme (entry 2), not entry 0.
    assert_eq!(
        dot,
        owner_dot_color_for_index(&ramps, HouseColorIndex(2)),
        "unknown owner should use DEFAULT_SCHEME_ENTRY (2)"
    );
    assert_eq!(dot[3], 255, "Alpha should be fully opaque");
}

/// Helper: dot color for a known index (mirrors owner_dot_color's ramp[0] pick).
fn owner_dot_color_for_index(ramps: &HouseColorRamps, idx: HouseColorIndex) -> [u8; 4] {
    let c = ramps.ramp(idx)[0];
    [c.r, c.g, c.b, 255]
}

#[test]
fn test_minimap_entity_visible_for_allied_owner() {
    let mut alliances = HouseAllianceMap::default();
    let allied_names = BTreeSet::from(["AMERICANS".to_string(), "BRITISH".to_string()]);
    alliances.insert("AMERICANS".to_string(), allied_names.clone());
    alliances.insert("BRITISH".to_string(), allied_names);

    let fog = FogState {
        width: 64,
        height: 64,
        by_owner: BTreeMap::new(),
        alliances,
        ..Default::default()
    };
    let pos = Position {
        rx: 10,
        ry: 12,
        z: 0,
        exact_z_leptons: None,
        sub_x: crate::util::lepton::CELL_CENTER_LEPTON,
        sub_y: crate::util::lepton::CELL_CENTER_LEPTON,
    };
    assert!(minimap_entity_visible(
        test_intern("Americans"),
        &fog,
        &pos,
        test_intern("British"),
    ));
}

#[test]
fn test_cell_visibility_color_visible_uses_base_color() {
    let mut fog = FogState {
        width: 16,
        height: 16,
        ..Default::default()
    };
    fog.mark_visible_for_owner(test_intern("Americans"), 5, 7);
    let pixel = make_pixel(5, 7, [40, 120, 40, 255]);
    assert_eq!(
        cell_visibility_color(test_intern("Americans"), &fog, &pixel),
        Some([40, 120, 40, 255])
    );
}

#[test]
fn test_cell_visibility_color_revealed_shows_full_color() {
    // In standard YR (FogOfWar=false), revealed cells show at full brightness.
    let mut fog = FogState {
        width: 16,
        height: 16,
        ..Default::default()
    };
    fog.mark_visible_for_owner(test_intern("Americans"), 5, 7);
    fog.by_owner
        .get_mut(&test_intern("Americans"))
        .expect("owner present")
        .clear_all_visible();
    let pixel = make_pixel(5, 7, [100, 50, 25, 255]);
    assert_eq!(
        cell_visibility_color(test_intern("Americans"), &fog, &pixel),
        Some([100, 50, 25, 255])
    );
}

#[test]
fn test_cell_visibility_color_shrouded_returns_none() {
    let fog = FogState::default();
    let pixel = make_pixel(9, 9, [40, 120, 40, 255]);
    assert_eq!(
        cell_visibility_color(test_intern("Americans"), &fog, &pixel),
        None
    );
}

#[test]
fn test_set_pixel_in_bounds() {
    let mut rgba: Vec<u8> = vec![0u8; 16]; // 2x2 pixel buffer
    set_pixel(&mut rgba, 2, 1, 0, [255, 128, 64, 255]);
    // Pixel at (1,0) -> offset = (0*2 + 1)*4 = 4
    assert_eq!(rgba[4], 255);
    assert_eq!(rgba[5], 128);
    assert_eq!(rgba[6], 64);
    assert_eq!(rgba[7], 255);
}

#[test]
fn test_set_pixel_out_of_bounds_does_nothing() {
    let mut rgba: Vec<u8> = vec![0u8; 16]; // 2x2 pixel buffer
    // Writing to x=5 in a 2-wide buffer should be silently ignored.
    set_pixel(&mut rgba, 2, 5, 0, [255, 255, 255, 255]);
    assert!(rgba.iter().all(|&b| b == 0));
}

#[test]
fn test_single_cell_map_pixel_mapping() {
    // A map with one cell: its position should map to a valid minimap pixel.
    // If world_width is small (e.g., just one tile = 60px), the cell still
    // maps to pixel (0,0) since it IS the origin.
    let (px, py): (u32, u32) =
        world_to_minimap_pixel(0.0, 0.0, 0.0, 0.0, 60.0, 30.0, 0.0, 0.0, 200.0, 200.0);
    assert_eq!(px, 0);
    assert_eq!(py, 0);
}

#[test]
fn test_degenerate_world_size_no_panic() {
    // Zero-size world should not panic (clamped to 1.0 in MinimapRenderer::new).
    let (px, py): (u32, u32) =
        world_to_minimap_pixel(100.0, 100.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 200.0, 200.0);
    // Should clamp to max pixel.
    assert_eq!(px, MINIMAP_WIDTH - 1);
    assert_eq!(py, MINIMAP_HEIGHT - 1);
}

#[test]
fn playfield_projection_shrinks_and_reexpands_exact_mode_zero_membership() {
    let grid = playfield_projection_grid(64);
    let expanded = expanded_playfield();
    let shrunken = shrunken_playfield();
    let shared = grid
        .cells
        .iter()
        .find(|cell| {
            expanded.contains_geometry_packed(cell.rx.into(), cell.ry.into())
                && shrunken.contains_geometry_packed(cell.rx.into(), cell.ry.into())
        })
        .map(|cell| (cell.rx, cell.ry))
        .expect("shared valid cell");
    let readded = grid
        .cells
        .iter()
        .find(|cell| {
            expanded.contains_geometry_packed(cell.rx.into(), cell.ry.into())
                && !shrunken.contains_geometry_packed(cell.rx.into(), cell.ry.into())
        })
        .map(|cell| (cell.rx, cell.ry))
        .expect("cell excluded by contraction");
    let outside = grid
        .cells
        .iter()
        .find(|cell| !expanded.contains_geometry_packed(cell.rx.into(), cell.ry.into()))
        .map(|cell| (cell.rx, cell.ry))
        .expect("diamond filler cell");
    let overlays = [
        overlay_datum(shared.0, shared.1, OverlayClassification::Ore, 102, 5, true),
        overlay_datum(readded.0, readded.1, OverlayClassification::Gem, 27, 7, true),
        overlay_datum(outside.0, outside.1, OverlayClassification::Wall, 0, 0, false),
    ];
    let colors = HashMap::new();

    let initial = MinimapPlayfieldProjection::derive(
        &grid, None, &overlays, &colors, "TEMPERATE", Some(expanded), None,
    );
    let shrunk = MinimapPlayfieldProjection::derive(
        &grid, None, &overlays, &colors, "TEMPERATE", Some(shrunken), None,
    );
    let reexpanded = MinimapPlayfieldProjection::derive(
        &grid, None, &overlays, &colors, "TEMPERATE", Some(expanded), None,
    );

    let initial_cells: BTreeSet<_> = initial
        .terrain_pixels
        .iter()
        .map(|pixel| (pixel.rx, pixel.ry))
        .collect();
    let expected_cells: BTreeSet<_> = grid
        .cells
        .iter()
        .filter(|cell| expanded.contains_geometry_packed(cell.rx.into(), cell.ry.into()))
        .map(|cell| (cell.rx, cell.ry))
        .collect();
    assert_eq!(
        initial_cells, expected_cells,
        "membership must be exact mode 0"
    );
    assert!(initial_cells.contains(&readded));
    assert!(!initial_cells.contains(&outside));
    assert!(
        initial
            .overlay_pixels
            .iter()
            .any(|p| (p.rx, p.ry) == readded)
    );
    assert!(
        !initial
            .overlay_pixels
            .iter()
            .any(|p| (p.rx, p.ry) == outside)
    );
    assert!(
        !shrunk
            .terrain_pixels
            .iter()
            .any(|p| (p.rx, p.ry) == readded)
    );
    assert!(
        !shrunk
            .overlay_pixels
            .iter()
            .any(|p| (p.rx, p.ry) == readded)
    );
    assert!(
        reexpanded
            .terrain_pixels
            .iter()
            .any(|p| (p.rx, p.ry) == readded)
    );
    assert!(
        reexpanded
            .overlay_pixels
            .iter()
            .any(|p| (p.rx, p.ry) == readded)
    );
    assert!(shrunk.world_width < initial.world_width);
    assert!(shrunk.world_height < initial.world_height);
    assert_eq!(reexpanded.world_origin_x, initial.world_origin_x);
    assert_eq!(reexpanded.world_origin_y, initial.world_origin_y);
}

#[test]
fn playfield_revision_rebuild_gate_observes_two_successive_writers() {
    let bounds = Some(expanded_playfield());
    let initial = PlayfieldAuthorityStamp {
        bounds,
        revision: 0,
    };
    assert!(!playfield_authority_needs_reconcile(
        Some(initial),
        bounds,
        0
    ));
    assert!(playfield_authority_needs_reconcile(
        Some(initial),
        bounds,
        1
    ));
    let second = PlayfieldAuthorityStamp {
        bounds,
        revision: 1,
    };
    assert!(playfield_authority_needs_reconcile(Some(second), bounds, 2));
    assert!(playfield_authority_needs_reconcile(None, bounds, 2));
}

#[test]
fn native_radar_event_surface_rebuild_uses_current_playfield_without_baseline_replay() {
    let grid = playfield_projection_grid(64);
    let terrain = flat_resolved_terrain(64);
    let cell = grid
        .cells
        .iter()
        .find(|cell| {
            shrunken_playfield()
                .contains_geometry_packed(i32::from(cell.rx), i32::from(cell.ry))
        })
        .map(|cell| (cell.rx, cell.ry))
        .expect("cell retained by action-40 contraction");
    let overlays = [overlay_datum(
        cell.0,
        cell.1,
        OverlayClassification::Ore,
        102,
        3,
        true,
    )];
    let colors = HashMap::from([((102, 3), [77, 88, 99])]);
    let initial = MinimapPlayfieldProjection::derive(
        &grid,
        Some(&terrain),
        &overlays,
        &colors,
        "TEMPERATE",
        Some(expanded_playfield()), None,
    );
    let rebuilt = MinimapPlayfieldProjection::derive(
        &grid,
        Some(&terrain),
        &overlays,
        &colors,
        "TEMPERATE",
        Some(shrunken_playfield()), None,
    );
    let initial_surface = initial.native_radar_surface.expect("initial primary surface");
    let rebuilt_surface = rebuilt.native_radar_surface.expect("rebuilt primary surface");
    assert_eq!(initial_surface.raw_size(), (72, 62));
    assert_eq!(initial_surface.generated_size(), (125, 108));
    assert_eq!(rebuilt_surface.raw_size(), (20, 18));
    assert_eq!(rebuilt_surface.generated_size(), (120, 108));
    let initial_copy = generated_primary_copy_frame(initial_surface, 280.0, 216.0);
    let rebuilt_copy = generated_primary_copy_frame(rebuilt_surface, 280.0, 216.0);
    assert_eq!(initial_copy.offset, [14.0, 0.0]);
    assert_eq!(initial_copy.size, [250.0, 216.0]);
    assert_eq!(rebuilt_copy.offset, [20.0, 0.0]);
    assert_eq!(rebuilt_copy.size, [240.0, 216.0]);

    assert_ne!(
        initial_surface.cell_to_surface_pixel(cell),
        rebuilt_surface.cell_to_surface_pixel(cell),
        "new events must use the rebuilt generated projection"
    );
    let rebuilt_terrain = rebuilt
        .terrain_pixels
        .iter()
        .find(|pixel| (pixel.rx, pixel.ry) == cell)
        .expect("rebuilt terrain pixel");
    let rebuilt_overlay = rebuilt
        .overlay_pixels
        .iter()
        .find(|pixel| (pixel.rx, pixel.ry) == cell)
        .expect("rebuilt overlay pixel");
    let rebuilt_local = rebuilt_surface.cell_to_surface_pixel(cell);
    assert_eq!(
        (rebuilt_terrain.px, rebuilt_terrain.py),
        (rebuilt_local.0 as u32, rebuilt_local.1 as u32)
    );
    assert_eq!(
        (rebuilt_overlay.px, rebuilt_overlay.py),
        (rebuilt_local.0 as u32, rebuilt_local.1 as u32),
        "terrain and overlay rebuild in the same generated-primary frame"
    );
    let destination = rebuilt_surface.surface_to_aperture_pixel(rebuilt_local);
    let offset = ((destination.1 as u32 * MINIMAP_WIDTH + destination.0 as u32) * 4) as usize;
    assert_eq!(
        &rebuilt.base_rgba[offset..offset + 4],
        &[168, 168, 128, 255],
        "stock no-CellAnim tiberium reaches the aperture as packed khaki"
    );
    assert_ne!(
        initial
            .native_radar_terrain
            .as_ref()
            .expect("initial terrain surface")
            .generated_rgb565(),
        rebuilt
            .native_radar_terrain
            .as_ref()
            .expect("rebuilt terrain surface")
            .generated_rgb565(),
        "action 40 must regenerate the full sampled surface, not retain stale pixels"
    );

    let mut events = ClientRadarEvents::default();
    let config = crate::rules::radar_event_config::RadarEventConfig::default();
    let source = RadarEventSource {
        cell,
        radar_pixel: rebuilt_surface.cell_to_surface_pixel(cell),
    };
    assert!(!events.create_enemy_sensed(
        source,
        20,
        rebuilt_surface.generated_size(),
        &config,
    ));
    events.finish_baseline();
    assert!(events.create_enemy_sensed(
        source,
        21,
        rebuilt_surface.generated_size(),
        &config,
    ));
}

#[test]
fn minimap_techno_membership_is_height_aware_at_raised_slope_edge() {
    let bounds = expanded_playfield();
    let (rx, ry) = (0u16..64)
        .flat_map(|ry| (0u16..64).map(move |rx| (rx, ry)))
        .find(|&(rx, ry)| {
            bounds.contains_geometry_packed(rx.into(), ry.into())
                && !bounds.contains_height_aware_packed(rx.into(), ry.into(), 10, 1)
        })
        .expect("raised/slope edge where native mode 0 and mode 1 differ");
    assert!(bounds.contains_geometry_packed(rx.into(), ry.into()));
    assert!(!bounds.contains_height_aware_packed(rx.into(), ry.into(), 10, 1));
    assert!(bounds.contains_height_aware_packed(rx.into(), ry.into(), 0, 0));

    let mut entity =
        crate::sim::game_entity::GameEntity::test_default(1, "MTNK", "Americans", rx, ry);
    entity.in_playfield = false;
    assert!(!minimap_entity_in_playfield(true, &entity));
    entity.in_playfield = true;
    assert!(minimap_entity_in_playfield(true, &entity));
    entity.in_playfield = false;
    assert!(minimap_entity_in_playfield(false, &entity));
}

#[test]
fn playfield_projection_updates_camera_bounds_and_click_inverse_mapping() {
    let mut grid = playfield_projection_grid(64);
    let initial_geometry = PlayfieldPresentationGeometry::from_grid(&grid, expanded_playfield())
        .expect("expanded geometry");
    let shrunk_geometry = PlayfieldPresentationGeometry::from_grid(&grid, shrunken_playfield())
        .expect("shrunken geometry");
    let camera_bounds = shrunk_geometry.camera_local_bounds();
    assert_eq!(
        camera_bounds.pixel_x - crate::map::terrain::TILE_WIDTH / 2.0,
        shrunk_geometry.origin_x
    );
    assert_eq!(camera_bounds.pixel_y, shrunk_geometry.origin_y);
    assert_eq!(camera_bounds.pixel_w, shrunk_geometry.world_width);
    assert_eq!(camera_bounds.pixel_h, shrunk_geometry.world_height);
    grid.install_playfield_local_bounds(Some(expanded_playfield()));
    let installed_initial = grid.local_bounds.expect("initial camera authority");
    grid.install_playfield_local_bounds(Some(shrunken_playfield()));
    assert_eq!(grid.local_bounds, Some(camera_bounds));
    assert_ne!(grid.local_bounds, Some(installed_initial));

    let projection = MinimapPlayfieldProjection::derive(
        &grid,
        None,
        &[],
        &HashMap::new(),
        "TEMPERATE",
        Some(shrunken_playfield()), None,
    );
    let rect = (10.0, 20.0, 300.0, 240.0);
    let tex_x = projection.map_offset_x + projection.map_pixel_w * 0.5;
    let tex_y = projection.map_offset_y + projection.map_pixel_h * 0.5;
    let click_x = rect.0 + tex_x / MINIMAP_WIDTH as f32 * rect.2;
    let click_y = rect.1 + tex_y / MINIMAP_HEIGHT as f32 * rect.3;
    let camera = minimap_screen_point_to_camera_top_left(
        click_x,
        click_y,
        800.0,
        600.0,
        rect.0,
        rect.1,
        rect.2,
        rect.3,
        projection.world_origin_x,
        projection.world_origin_y,
        projection.world_width,
        projection.world_height,
        projection.map_offset_x,
        projection.map_offset_y,
        projection.map_pixel_w,
        projection.map_pixel_h,
    );
    assert_eq!(
        camera,
        (
            projection.world_origin_x + projection.world_width * 0.5 - 400.0,
            projection.world_origin_y + projection.world_height * 0.5 - 300.0,
        )
    );
    assert_ne!(initial_geometry.camera_local_bounds(), camera_bounds);
}

fn radar_gate_projection() -> RadarProjectionFacts {
    RadarProjectionFacts {
        native_surface: None,
        world_origin_x: 0.0,
        world_origin_y: 0.0,
        world_width: 1.0,
        world_height: 1.0,
        map_offset_x: 0.0,
        map_offset_y: 0.0,
        map_pixel_w: 1.0,
        map_pixel_h: 1.0,
    }
}

fn radar_gate_rules(fields: &str) -> RuleSet {
    RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(&format!(
        "[VehicleTypes]\n0=DOT\n[DOT]\nStrength=100\n{fields}\n"
    )))
    .expect("radar gate fixture rules")
}

fn radar_gate_fixture(
    owner: &str,
) -> (
    crate::sim::entity_store::EntityStore,
    crate::sim::intern::StringInterner,
    BTreeMap<crate::sim::intern::InternedId, crate::sim::house_state::HouseState>,
) {
    let mut entity = crate::sim::game_entity::GameEntity::test_default(1, "DOT", owner, 0, 0);
    entity.in_playfield = true;
    entity.lifecycle.in_limbo = false;
    let mut entities = crate::sim::entity_store::EntityStore::new();
    entities.insert(entity);
    let interner = crate::sim::intern::test_interner();
    let owner_id = interner.get(owner).expect("fixture owner interned");
    let mut houses = BTreeMap::new();
    houses.insert(
        owner_id,
        crate::sim::house_state::HouseState::new(owner_id, 0, None, false, 0, 10),
    );
    (entities, interner, houses)
}

#[test]
fn radar_pixel_shroud_exception_distinguishes_local_pointer_from_singleplayer_human() {
    let local = test_intern("Local");
    let (entities, interner, mut houses) = radar_gate_fixture("SecondHuman");
    let second = test_intern("SecondHuman");
    let house = houses.get_mut(&second).unwrap();
    house.is_human = false;
    house.player_control = true;
    let fog = FogState::default();
    let entry = RadarTrackerEntry {
        stable_id: 1,
        x: 0,
        y: 0,
    };
    let rules = radar_gate_rules("");

    assert!(radar_pixel_candidate_eligible(
        entry,
        &entities,
        &houses,
        Some(local),
        &fog,
        false,
        false,
        Some(&rules),
        Some(&interner),
        radar_gate_projection(),
    ));
    assert!(!radar_pixel_candidate_eligible(
        entry,
        &entities,
        &houses,
        Some(local),
        &fog,
        false,
        true,
        Some(&rules),
        Some(&interner),
        radar_gate_projection(),
    ));
}

#[test]
fn radar_pixel_radar_invisible_precedes_radar_visible_but_alliance_restores() {
    let local = test_intern("Local");
    let enemy = test_intern("Enemy");
    let (entities, interner, houses) = radar_gate_fixture("Enemy");
    let entry = RadarTrackerEntry {
        stable_id: 1,
        x: 0,
        y: 0,
    };
    let rules = radar_gate_rules("RadarInvisible=yes\nRadarVisible=yes");
    let mut fog = FogState::default();
    assert!(!radar_pixel_candidate_eligible(
        entry,
        &entities,
        &houses,
        Some(local),
        &fog,
        true,
        true,
        Some(&rules),
        Some(&interner),
        radar_gate_projection(),
    ));

    let allies = BTreeSet::from(["LOCAL".to_string(), "ENEMY".to_string()]);
    fog.alliances.insert("LOCAL".to_string(), allies.clone());
    fog.alliances.insert("ENEMY".to_string(), allies);
    assert!(fog.is_friendly_id(local, enemy, &interner));
    assert!(radar_pixel_candidate_eligible(
        entry,
        &entities,
        &houses,
        Some(local),
        &fog,
        true,
        true,
        Some(&rules),
        Some(&interner),
        radar_gate_projection(),
    ));
}

#[test]
fn radar_pixel_insignificant_passive_owner_requires_radar_visible() {
    let local = test_intern("Local");
    let enemy = test_intern("Passive");
    let (entities, interner, mut houses) = radar_gate_fixture("Passive");
    houses.get_mut(&enemy).unwrap().multiplay_passive = true;
    let entry = RadarTrackerEntry {
        stable_id: 1,
        x: 0,
        y: 0,
    };
    let fog = FogState::default();
    let hidden = radar_gate_rules("Insignificant=yes\nRadarVisible=no");
    let restored = radar_gate_rules("Insignificant=yes\nRadarVisible=yes");
    assert!(!radar_pixel_candidate_eligible(
        entry,
        &entities,
        &houses,
        Some(local),
        &fog,
        true,
        true,
        Some(&hidden),
        Some(&interner),
        radar_gate_projection(),
    ));
    assert!(radar_pixel_candidate_eligible(
        entry,
        &entities,
        &houses,
        Some(local),
        &fog,
        true,
        true,
        Some(&restored),
        Some(&interner),
        radar_gate_projection(),
    ));
}

#[test]
fn radar_pixel_fogged_but_explored_cell_passes_render_gate_without_gap_proxy() {
    let local = test_intern("Local");
    let (entities, interner, houses) = radar_gate_fixture("Enemy");
    let mut fog = FogState::default();
    fog.mark_visible_for_owner(local, 0, 0);
    fog.by_owner.get_mut(&local).unwrap().clear_all_visible();
    assert!(fog.is_cell_revealed(local, 0, 0));
    assert!(!fog.is_cell_visible(local, 0, 0));
    assert!(radar_pixel_candidate_eligible(
        RadarTrackerEntry {
            stable_id: 1,
            x: 0,
            y: 0,
        },
        &entities,
        &houses,
        Some(local),
        &fog,
        false,
        true,
        Some(&radar_gate_rules("")),
        Some(&interner),
        radar_gate_projection(),
    ));
}

#[test]
fn radar_building_tracker_pixels_use_owner_color_not_khaki() {
    let mut entity = crate::sim::game_entity::GameEntity::test_default(
        1,
        "GAPOWR",
        "Americans",
        0,
        0,
    );
    entity.category = crate::map::entities::EntityCategory::Structure;
    let interner = crate::sim::intern::test_interner();
    let mut colors = HouseColorMap::new();
    colors.insert("Americans".to_string(), HouseColorIndex(1));
    let color = radar_entity_owner_color(&entity, Some(&interner), &colors, &test_ramps());
    assert_eq!(color, owner_dot_color("Americans", &colors, &test_ramps()));
    assert_ne!(color, [200, 200, 160, 255]);
}

#[test]
fn radar_tracker_color_uses_active_disguise_house_without_changing_priority_owner() {
    let mut entity = crate::sim::game_entity::GameEntity::test_default(
        1,
        "MGTK",
        "Americans",
        0,
        0,
    );
    let soviet = test_intern("Soviet");
    let mut disguise = crate::sim::cloak_disguise::DisguiseRuntime::default();
    disguise.disguised = true;
    disguise.disguised_as_house = Some(soviet);
    entity.disguise = Some(disguise);
    let interner = crate::sim::intern::test_interner();
    let mut colors = HouseColorMap::new();
    colors.insert("Americans".to_string(), HouseColorIndex(1));
    colors.insert("Soviet".to_string(), HouseColorIndex(2));
    let ramps = test_ramps();
    assert_eq!(
        radar_entity_owner_color(&entity, Some(&interner), &colors, &ramps),
        owner_dot_color("Soviet", &colors, &ramps)
    );
    assert_eq!(entity.owner, test_intern("Americans"));
}
