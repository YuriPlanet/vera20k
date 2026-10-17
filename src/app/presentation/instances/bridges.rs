//! Bridge instance emission — body, shadow, deck-variant overrides.
//!
//! Reads live CellClass bridge fields and ordinary overlay-grid state, emitting sprite
//! instances for the bridge body and body shadow passes per the
//! per-frame draw chain in `BRIDGE_DISPLAY_TABLE_GHIDRA_REPORT.md` §3.3, §3.4.
//!
//! Shadow caller/raster execution: `tools/spatial_oracle/bridge_shadow_render.md`.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.
//! - Read-only access to sim state via `AppState`.

use std::collections::BTreeMap;

use crate::app::AppState;
use crate::map::lighting::CellLightGrid;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::terrain::{self, TILE_HEIGHT, TILE_WIDTH};
use crate::render::batch::SpriteInstance;
use crate::render::bridge_atlas::{BridgeAtlasLookup, is_high_bridge_body_identity};

use super::helpers::{compute_sprite_depth_params, in_view};

/// Latin-square jitter for healthy bridge body frames at base state byte 0
/// (NS) or 9 (EW). Verified raw memory read at gamemd's `g_LatinSquare`
/// (RE doc §5; ledger #1).
const BRIDGE_BODY_LATIN_SQUARE: [u8; 16] = [0, 1, 2, 3, 3, 2, 1, 0, 2, 3, 0, 1, 1, 0, 3, 2];

/// Body Y offset for bridge state bytes 0..=8.
/// Verified in `CellClass__Get_Draw_Offset @ 0x00480110`: HasBridge applies
/// `-0x10`, and only state bytes 9..=17 receive the extra `-0x0f`.
const BRIDGE_BODY_Y_OFFSET_STATE_0_TO_8: f32 = -16.0;
/// Body Y offset for bridge state bytes 9..=17.
/// This follows the state byte, not the flipped SHP frame range.
const BRIDGE_BODY_Y_OFFSET_STATE_9_TO_17: f32 = -31.0;

/// Shadow X displacement on EW states 9..17.
///
/// Settled at -15: the native overlay-shadow draw subtracts 0xF from the draw
/// X for bridge cells whose state byte is in 9..=0x11, in the same branch that
/// adds the +7 to Y. The RE doc's open question between -15 and -45 is closed.
pub const BRIDGE_SHADOW_EW_DX: i32 = -15;
/// Shadow Y displacement on flagged EW states 9..17: original 0047F510 adds 7.
/// Full caller execution is pinned in tools/spatial_oracle/bridge_shadow_render.json.
pub const BRIDGE_SHADOW_EW_DY: i32 = 7;

/// CellClass::DrawOverlay_Body @ 0047F6A0 uses the raw +11E byte.
/// Only states 0 and 9 add g_LatinSquare; native caller controls exercise
/// every table entry in tools/spatial_oracle/bridge_body_render.json.
fn compute_bridge_body_shp_frame(state: u8, rx: u16, ry: u16) -> u8 {
    if state == 0 || state == 9 {
        state + BRIDGE_BODY_LATIN_SQUARE[usize::from((ry & 3) * 4 + (rx & 3))]
    } else {
        state
    }
}

fn bridge_anchor_y_offset(state_byte: u8) -> f32 {
    match state_byte {
        9..=17 => BRIDGE_BODY_Y_OFFSET_STATE_9_TO_17,
        _ => BRIDGE_BODY_Y_OFFSET_STATE_0_TO_8,
    }
}

/// Build sprite instances for the bridge body pass (RE doc §3.3, Step 5
/// pass 1). Reads the live CellClass state byte post-tick.
///
/// Takes only the fields the body builder actually needs from `AppState`
/// so the function is exercisable in unit tests with a pure-data mock atlas
/// (`BridgeAtlasLookup` trait). The shadow builder uses the same lookup seam.
pub fn build_bridge_body_instances_inner(
    terrain: &ResolvedTerrainGrid,
    atlas: &dyn BridgeAtlasLookup,
    overlay_names: &BTreeMap<u8, String>,
    lighting_grid: &CellLightGrid,
    origin_y: f32,
    world_height: f32,
    cam_x: f32,
    cam_y: f32,
    sw: f32,
    sh: f32,
    out: &mut Vec<SpriteInstance>,
) {
    let mut cells: Vec<_> = terrain
        .iter()
        .filter(|cell| {
            // 0047F6A0 tests live Cell.flags 0x80, not structural admission or
            // a decoded simulation damage state. Frozen overlays retain these live fields.
            cell.bridge_facts.raw_flags & crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF != 0
        })
        .collect();
    // Tactical::DrawAll body sweep: descending diagonals, then X.
    // Original traversal/composition: bridge_body_render.json scenes[0].
    // Strict Less means the first body owns equal-depth overlaps.
    cells.sort_unstable_by_key(|cell| {
        (
            std::cmp::Reverse(u32::from(cell.rx) + u32::from(cell.ry)),
            cell.rx,
        )
    });
    for cell in cells {
        let (rx, ry) = (cell.rx, cell.ry);
        let Some(overlay) = cell.bridge_facts.overlay_id else {
            continue;
        };
        let Some(name) = overlay_names.get(&overlay) else {
            continue;
        };
        if !is_high_bridge_body_identity(overlay, name) {
            continue;
        }

        let frame = compute_bridge_body_shp_frame(cell.bridge_facts.state_byte, rx, ry);
        let y_offset = bridge_anchor_y_offset(cell.bridge_facts.state_byte);
        let z = cell.level;
        let (sx, sy) = terrain::iso_to_screen(rx, ry, z);
        let sy = sy + y_offset;
        if !in_view(sx, sy, 120.0, 120.0, cam_x, cam_y, sw, sh, 120.0) {
            continue;
        }

        let Some(spr) = atlas.body_entry(name, frame) else {
            log::warn!("bridge body atlas miss: name={name} frame={frame} cell=({rx},{ry})");
            continue;
        };

        let body_x = sx + TILE_WIDTH / 2.0 + spr.offset_x;
        let body_y = sy + TILE_HEIGHT / 2.0 + spr.offset_y;
        // 0047F7DA..0047F80B: signed live Cell.level + 4, gradient 0,
        // no z-shape. The shared SHP row kernel supplies per-pixel depth.
        let z_adjust = crate::render::native_z::ground_anchored_z_adjust(
            i32::from(z as i8) + 4,
            crate::render::native_z::SHP_DRAW_Z_ADJUST_PX,
        );
        let depth = compute_sprite_depth_params(origin_y, world_height, body_y, z);
        let tint: [f32; 3] = lighting_grid.bridge_body_tint_at((rx, ry));
        out.push(SpriteInstance {
            position: [body_x, body_y],
            size: spr.pixel_size,
            uv_origin: spr.uv_origin,
            uv_size: spr.uv_size,
            depth,
            tint,
            // Active high-bridge body 0047F7DA uses cell Convert/bottom10E.
            palette_light: crate::render::palette_light::PaletteLight::cell(
                lighting_grid,
                (rx, ry),
                false,
            )
            .with_brightness(
                lighting_grid
                    .cell_light_at((rx, ry))
                    .map_or(1000, |l| l.bottom_scalar),
            ),
            alpha: 1.0,
            z_adjust: z_adjust as f32,
            z_gradient: crate::render::native_z::ZGradient::Flat as u32,
            ..Default::default()
        });
    }
}

/// Thin `AppState` wrapper around `build_bridge_body_instances_inner`. Pulls
/// the live fields the inner function needs out of `state` and forwards.
pub(crate) fn build_bridge_body_instances(
    state: &AppState,
    sw: f32,
    sh: f32,
    out: &mut Vec<SpriteInstance>,
) {
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return;
    };
    let Some(terrain) = sim.resolved_terrain.as_ref() else {
        return;
    };
    let Some(atlas) = state.match_state.match_presentation.bridge_atlas.as_ref() else {
        return;
    };
    let (origin_y, world_height) = state
        .match_state
        .match_presentation
        .terrain_grid
        .as_ref()
        .map(|g| (g.origin_y, g.world_height))
        .unwrap_or((0.0, 1.0));
    build_bridge_body_instances_inner(
        terrain,
        atlas,
        &state.match_state.match_presentation.overlay_names,
        state.match_state.match_presentation.lighting.grid(),
        origin_y,
        world_height,
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
        sw,
        sh,
        out,
    );
}

/// CellClass::DrawOverlay_Shadow @ 0047F510, called by the second
/// Tactical cell sweep (006D7031..006D71CD). Read raw live overlay/state/level;
/// structural deck admission and body frame jitter do not apply to shadows.
/// Native execution: tools/spatial_oracle/bridge_shadow_render.py.
pub(crate) fn build_bridge_shadow_instances_inner(
    terrain: &ResolvedTerrainGrid,
    atlas: &dyn BridgeAtlasLookup,
    overlay_names: &BTreeMap<u8, String>,
    overlay_entries: &[crate::map::overlay::OverlayEntry],
    live_grid: Option<&crate::sim::overlay_grid::OverlayGrid>,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    origin_y: f32,
    world_height: f32,
    cam_x: f32,
    cam_y: f32,
    sw: f32,
    sh: f32,
    out: &mut Vec<SpriteInstance>,
) {
    let mut cells: Vec<_> = terrain
        .iter()
        .filter_map(|cell| {
            let overlay = cell.bridge_facts.overlay_id?;
            let name = overlay_names.get(&overlay)?;
            is_high_bridge_body_identity(overlay, name).then_some((
                cell,
                name,
                cell.bridge_facts.state_byte,
            ))
        })
        .collect();
    // Wall damage owns its current identity/data in OverlayGrid. The published
    // index supplies iteration anchors, including paid construction and erased
    // tombstones; neither its cached frame nor Terrain bridge facts are live state.
    // Original 0047F510 has no Wall exclusion (concrete_wall_render.json).
    for entry in overlay_entries {
        let live_cell = live_grid.map(|grid| grid.cell(entry.rx, entry.ry));
        let Some((overlay, frame)) =
            super::overlays::overlay_render_identity(entry.overlay_id, entry.frame, live_cell)
        else {
            continue;
        };
        if !registry
            .and_then(|registry| registry.flags(overlay))
            .is_some_and(|flags| flags.wall)
        {
            continue;
        }
        let Some(name) = overlay_names.get(&overlay) else {
            continue;
        };
        // High identities are already emitted above with their retained bridge
        // geometry, even when layered rules give the same type a Wall flag.
        if is_high_bridge_body_identity(overlay, name) {
            continue;
        }
        let Some(cell) = terrain.cell(entry.rx, entry.ry) else {
            continue;
        };
        cells.push((cell, name, frame));
    }
    // The native shadow sweep descends diagonals, advancing X within each.
    // Neither elevation, atlas frame nor the compatibility depth scalar owns
    // the order in which an overlap reads the previous color and Z stores.
    cells.sort_unstable_by_key(|(cell, _, _)| {
        (
            std::cmp::Reverse(u32::from(cell.rx) + u32::from(cell.ry)),
            cell.rx,
        )
    });
    for (cell, name, frame) in cells {
        let (rx, ry) = (cell.rx, cell.ry);
        let Some(spr) = atlas.shadow_entry(name, frame) else {
            continue;
        };
        let (mut sx, mut sy) = terrain::iso_to_screen(rx, ry, cell.level);
        if cell.bridge_facts.raw_flags & crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF != 0 {
            sy += bridge_anchor_y_offset(frame);
            if (9..=17).contains(&frame) {
                sx += BRIDGE_SHADOW_EW_DX as f32;
                sy += BRIDGE_SHADOW_EW_DY as f32;
            }
        }
        let position = [
            sx + TILE_WIDTH / 2.0 + spr.offset_x,
            sy + TILE_HEIGHT / 2.0 + spr.offset_y,
        ];
        if !in_view(
            position[0],
            position[1],
            spr.pixel_size[0],
            spr.pixel_size[1],
            cam_x,
            cam_y,
            sw,
            sh,
            0.0,
        ) {
            continue;
        }
        let z_adjust = crate::render::native_z::ground_anchored_z_adjust(
            i32::from(cell.level as i8),
            crate::render::native_z::SHP_DRAW_Z_ADJUST_PX,
        );
        out.push(SpriteInstance {
            position,
            size: spr.pixel_size,
            uv_origin: spr.uv_origin,
            uv_size: spr.uv_size,
            depth: compute_sprite_depth_params(origin_y, world_height, position[1], cell.level),
            z_adjust: z_adjust as f32,
            z_gradient: crate::render::native_z::ZGradient::Flat as u32,
            ..Default::default()
        });
    }
}

pub(crate) fn build_bridge_shadow_instances(
    state: &AppState,
    sw: f32,
    sh: f32,
    out: &mut Vec<SpriteInstance>,
) {
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return;
    };
    let Some(terrain) = sim.resolved_terrain.as_ref() else {
        return;
    };
    let Some(atlas) = state.match_state.match_presentation.bridge_atlas.as_ref() else {
        return;
    };
    let (origin_y, world_height) = state
        .match_state
        .match_presentation
        .terrain_grid
        .as_ref()
        .map(|g| (g.origin_y, g.world_height))
        .unwrap_or((0.0, 1.0));
    let (cam_x, cam_y) = (
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
    );
    build_bridge_shadow_instances_inner(
        terrain,
        atlas,
        &state.match_state.match_presentation.overlay_names,
        state.match_state.match_presentation.overlays.as_slice(),
        sim.overlay_grid.as_ref(),
        state.overlay_registry(),
        origin_y,
        world_height,
        cam_x,
        cam_y,
        sw,
        sh,
        out,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_walls_and_high_bridges_share_native_shadow_sweep_order() {
        use crate::map::overlay::OverlayEntry;
        use crate::render::overlay_atlas::OverlaySpriteEntry;
        use crate::rules::{ini_parser::IniFile, overlay_types::OverlayTypeRegistry};
        struct Atlas(std::cell::RefCell<Vec<(String, u8)>>);
        impl BridgeAtlasLookup for Atlas {
            fn body_entry(&self, _: &str, _: u8) -> Option<&OverlaySpriteEntry> {
                None
            }
            fn shadow_entry(&self, name: &str, frame: u8) -> Option<&OverlaySpriteEntry> {
                self.0.borrow_mut().push((name.into(), frame));
                None
            }
        }
        let registry = OverlayTypeRegistry::from_ini(
            &IniFile::from_str("[OverlayTypes]\n0=WALL\n1=OTHER\n[WALL]\nWall=yes\n"),
            None,
        );
        let mut terrain = crate::map::resolved_terrain::test_grid(
            8,
            8,
            crate::map::resolved_terrain::test_flat_cell,
        );
        let high = terrain.cell_mut(5, 5).unwrap();
        high.bridge_facts.overlay_id = Some(0x18);
        high.bridge_facts.state_byte = 9;
        let mut grid = crate::sim::overlay_grid::OverlayGrid::new(8, 8);
        let mut entries = Vec::new();
        for (rx, ry, live_id, frame) in [
            (4, 4, Some(0), 36),
            (5, 6, Some(0), 20),
            (6, 6, None, 0),
            (7, 6, Some(1), 5),
        ] {
            entries.push(OverlayEntry {
                rx,
                ry,
                overlay_id: 0,
                frame: 5,
            });
            let cell = grid.cell_mut(rx, ry);
            cell.overlay_id = live_id;
            cell.overlay_data = frame;
        }
        let atlas = Atlas(Default::default());
        build_bridge_shadow_instances_inner(
            &terrain,
            &atlas,
            &BTreeMap::from([
                (0, "WALL".into()),
                (1, "OTHER".into()),
                (0x18, "BRIDGE1".into()),
            ]),
            &entries,
            Some(&grid),
            Some(&registry),
            0.0,
            5000.0,
            -1000.0,
            -1000.0,
            4000.0,
            4000.0,
            &mut Vec::new(),
        );
        // Original Tactical traversal descends diagonals. A Wall cannot be
        // appended after the high-bridge batch; live damage overrides cached5,
        // while erased/replaced slots must not query any stale Wall artwork.
        assert_eq!(
            *atlas.0.borrow(),
            vec![
                ("WALL".into(), 20),
                ("BRIDGE1".into(), 9),
                ("WALL".into(), 36),
            ]
        );
    }

    #[test]
    fn high_body_live_fields_match_original_caller_controls() {
        use crate::render::overlay_atlas::OverlaySpriteEntry;
        struct Atlas {
            entry: OverlaySpriteEntry,
            frames: std::cell::RefCell<Vec<u8>>,
        }
        impl BridgeAtlasLookup for Atlas {
            fn body_entry(&self, _: &str, frame: u8) -> Option<&OverlaySpriteEntry> {
                self.frames.borrow_mut().push(frame);
                Some(&self.entry)
            }
            fn shadow_entry(&self, _: &str, _: u8) -> Option<&OverlaySpriteEntry> {
                None
            }
        }
        let packet: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/bridge_body_render.json",
        ))
        .unwrap();
        for row in packet["controls"].as_array().unwrap() {
            let input = &row["input"];
            let flags = input["flags"].as_u64().unwrap() as u32;
            // The unraised ordinary-image arm belongs to the ordinary overlay owner.
            if flags & crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF == 0 {
                continue;
            }
            let rx = input["coords"][0].as_u64().unwrap() as u16;
            let ry = input["coords"][1].as_u64().unwrap() as u16;
            let mut terrain =
                test_bridge_grid(&[((rx, ry), 0x18, input["state"].as_u64().unwrap() as u8)]);
            let cell = terrain.cell_mut(rx, ry).unwrap();
            cell.level = input["level"].as_i64().unwrap() as u8;
            cell.bridge_facts.raw_flags = flags;
            // Deliberately stale derived deck height must never own body geometry.
            cell.bridge_deck_level = 99;
            let atlas = Atlas {
                entry: OverlaySpriteEntry {
                    uv_origin: [0.0; 2],
                    uv_size: [1.0; 2],
                    pixel_size: [60.0, 30.0],
                    offset_x: 0.0,
                    offset_y: 0.0,
                },
                frames: Default::default(),
            };
            let mut out = Vec::new();
            build_bridge_body_instances_inner(
                &terrain,
                &atlas,
                &BTreeMap::from([(0x18, "BRIDGE1".into())]),
                &CellLightGrid::new(),
                0.0,
                5000.0,
                -1000.0,
                -1000.0,
                4000.0,
                4000.0,
                &mut out,
            );
            let draw = &row["draws"][0];
            assert_eq!(
                *atlas.frames.borrow(),
                vec![draw["frame"].as_u64().unwrap() as u8],
                "{}",
                row["name"]
            );
            assert_eq!(out.len(), 1, "{}", row["name"]);
            assert_eq!(
                out[0].z_adjust,
                draw["z_adjust"].as_i64().unwrap() as f32,
                "{}",
                row["name"]
            );
            assert_eq!(out[0].z_gradient, draw["gradient"].as_u64().unwrap() as u32);
        }
    }

    fn test_bridge_grid(cells: &[((u16, u16), u8, u8)]) -> ResolvedTerrainGrid {
        let width = cells.iter().map(|c| c.0.0).max().unwrap() + 1;
        let height = cells.iter().map(|c| c.0.1).max().unwrap() + 1;
        let mut terrain = crate::map::resolved_terrain::test_grid(
            width,
            height,
            crate::map::resolved_terrain::test_flat_cell,
        );
        for &((x, y), overlay, state) in cells {
            let cell = terrain.cell_mut(x, y).unwrap();
            cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL
                | crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF;
            cell.bridge_facts.overlay_id = Some(overlay);
            cell.bridge_facts.state_byte = state;
        }
        terrain
    }

    #[test]
    fn startup_numeric_high_identity_emits_custom_named_body_and_shadow() {
        use crate::map::lighting::CellLightGrid;
        use crate::render::overlay_atlas::OverlaySpriteEntry;

        struct MockAtlas {
            entry: OverlaySpriteEntry,
            body_queries: std::cell::RefCell<Vec<(String, u8)>>,
            shadow_queries: std::cell::RefCell<Vec<(String, u8)>>,
        }
        impl BridgeAtlasLookup for MockAtlas {
            fn body_entry(&self, name: &str, frame: u8) -> Option<&OverlaySpriteEntry> {
                self.body_queries
                    .borrow_mut()
                    .push((name.to_string(), frame));
                (name == "HIGHANCHOR" && frame == 1).then_some(&self.entry)
            }

            fn shadow_entry(&self, name: &str, frame: u8) -> Option<&OverlaySpriteEntry> {
                self.shadow_queries
                    .borrow_mut()
                    .push((name.to_string(), frame));
                (name == "HIGHANCHOR" && frame == 1).then_some(&self.entry)
            }
        }

        // Healthy NS variant 1 is state byte 1.
        let terrain = test_bridge_grid(&[((5, 5), 0x18, 1)]);
        let atlas = MockAtlas {
            entry: OverlaySpriteEntry {
                uv_origin: [0.0, 0.0],
                uv_size: [1.0, 1.0],
                pixel_size: [60.0, 30.0],
                offset_x: 0.0,
                offset_y: 0.0,
            },
            body_queries: std::cell::RefCell::new(Vec::new()),
            shadow_queries: std::cell::RefCell::new(Vec::new()),
        };
        let overlay_names = BTreeMap::from([(0x18, "HIGHANCHOR".to_string())]);
        let lighting_grid = CellLightGrid::new();
        let (cell_x, cell_y) = terrain::iso_to_screen(5, 5, 4);
        let (cam_x, cam_y) = (cell_x - 400.0, cell_y - 300.0);
        let mut bodies = Vec::new();
        let mut shadows = Vec::new();

        build_bridge_body_instances_inner(
            &terrain,
            &atlas,
            &overlay_names,
            &lighting_grid,
            0.0,
            5000.0,
            cam_x,
            cam_y,
            800.0,
            600.0,
            &mut bodies,
        );
        build_bridge_shadow_instances_inner(
            &terrain,
            &atlas,
            &overlay_names,
            &[],
            None,
            None,
            0.0,
            5000.0,
            cam_x,
            cam_y,
            800.0,
            600.0,
            &mut shadows,
        );

        assert_eq!(bodies.len(), 1, "numeric high identity must emit one body");
        assert_eq!(
            shadows.len(),
            1,
            "numeric high identity must emit one shadow"
        );
        assert_eq!(
            atlas.body_queries.into_inner(),
            vec![("HIGHANCHOR".to_string(), 1)]
        );
        assert_eq!(
            atlas.shadow_queries.into_inner(),
            vec![("HIGHANCHOR".to_string(), 1)]
        );
    }
}
