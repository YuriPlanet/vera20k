//! Cell overlay shadow sweep and high-bridge body instances.
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
use crate::render::tactical_draw_plan::RenderZPolicy;

use super::helpers::{compute_sprite_depth_params, in_view};
use super::{CellBody, CellBodyAtlas, CellBodyDraw, cell_sweep_key};

/// `0x0047F7DA..0x0047F80B`: a high-bridge body is a `0x4E00` SHP draw that
/// reads and writes Z, as an ordinary body is.
pub(crate) const HIGH_BRIDGE_BODY_DRAW: CellBodyDraw = CellBodyDraw {
    atlas: CellBodyAtlas::Bridge,
    render_z: RenderZPolicy::ReadWrite,
};

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

/// State-dependent Y term in CellClass::Get_Draw_Offset480110.
/// Projection owns signed level/camera; the atlas owns OverlayType Y bias.
/// The unraised EF identity has its own -15 term, independently of IsRubble.
pub(super) fn cell_overlay_y_offset(overlay: u8, flags: u32, state: u8) -> f32 {
    if flags & crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF != 0 {
        match state {
            9..=17 => BRIDGE_BODY_Y_OFFSET_STATE_9_TO_17,
            _ => BRIDGE_BODY_Y_OFFSET_STATE_0_TO_8,
        }
    } else if overlay == crate::sim::overlay_grid::RUBBLE_OVERLAY_INDEX {
        -15.0 // original480144..48014D
    } else {
        0.0
    }
}

/// Build the high-bridge bodies for the cell body sweep (RE doc §3.3, Step 5
/// pass 1). Reads the live CellClass identity and state byte post-tick from
/// `live_grid`; without one no body draws.
///
/// Takes only the fields the body builder actually needs from `AppState`
/// so the function is exercisable in unit tests with a pure-data mock atlas
/// (`BridgeAtlasLookup` trait). The shadow builder uses the same lookup seam.
pub(crate) fn build_bridge_body_instances_inner(
    terrain: &ResolvedTerrainGrid,
    live_grid: Option<&crate::sim::overlay_grid::OverlayGrid>,
    atlas: &dyn BridgeAtlasLookup,
    overlay_names: &BTreeMap<u8, String>,
    lighting_grid: &CellLightGrid,
    origin_y: f32,
    world_height: f32,
    cam_x: f32,
    cam_y: f32,
    sw: f32,
    sh: f32,
    out: &mut Vec<CellBody>,
) {
    // 0047F6A0 tests live Cell.flags 0x80, not structural admission or a
    // decoded simulation damage state. Frozen overlays retain these live
    // fields. The sweep orders the bodies (bridge_body_render.json scenes[0]).
    let cells = terrain.iter().filter(|cell| {
        cell.bridge_facts.raw_flags & crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF != 0
    });
    for cell in cells {
        let (rx, ry) = (cell.rx, cell.ry);
        let Some(live) = live_grid.map(|grid| grid.cell(rx, ry)) else {
            continue;
        };
        let Some(overlay) = live.overlay_id else {
            continue;
        };
        let Some(name) = overlay_names.get(&overlay) else {
            continue;
        };
        if !is_high_bridge_body_identity(overlay, name) {
            continue;
        }

        let frame = compute_bridge_body_shp_frame(live.overlay_data, rx, ry);
        let y_offset =
            cell_overlay_y_offset(overlay, cell.bridge_facts.raw_flags, live.overlay_data);
        let z = cell.level;
        let (sx, sy) = terrain::iso_to_screen(rx, ry, z);
        let sy = sy + y_offset;
        if !in_view(sx, sy, 120.0, 120.0, cam_x, cam_y, sw, sh, 120.0) {
            continue;
        }

        let Some(spr) = atlas.body_entry(name, u16::from(frame)) else {
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
        out.push(CellBody {
            cell: (rx, ry),
            draw: HIGH_BRIDGE_BODY_DRAW,
            instance: SpriteInstance {
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
            },
        });
    }
}

/// Thin `AppState` wrapper around `build_bridge_body_instances_inner`. Pulls
/// the live fields the inner function needs out of `state` and forwards.
pub(crate) fn build_bridge_body_instances(
    state: &AppState,
    sw: f32,
    sh: f32,
    out: &mut Vec<CellBody>,
) {
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return;
    };
    let Some(terrain) = sim.resolved_terrain() else {
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
        sim.overlay_grid(),
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
    rubble_types: Option<(
        &crate::rules::ruleset::RuleSet,
        &crate::sim::intern::StringInterner,
    )>,
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
            let live = live_grid?.cell(cell.rx, cell.ry);
            let overlay = live.overlay_id?;
            let name = overlay_names.get(&overlay)?;
            if !is_high_bridge_body_identity(overlay, name) {
                return None;
            }
            let sprite = atlas.shadow_entry(name, u16::from(live.overlay_data))?;
            Some((cell, overlay, sprite, live.overlay_data, None))
        })
        .collect();
    // Wall damage owns its current identity/data in OverlayGrid. The published
    // index supplies iteration anchors, including paid construction and erased
    // tombstones; its cached frame is not live state.
    // Original 0047F510 has no Wall exclusion (concrete_wall_render.json).
    for entry in overlay_entries {
        let live_cell = live_grid.map(|grid| grid.cell(entry.rx, entry.ry));
        let Some((overlay, frame)) =
            super::overlays::overlay_render_identity(entry.overlay_id, entry.frame, live_cell)
        else {
            continue;
        };
        let Some(flags) = registry.and_then(|registry| registry.flags(overlay)) else {
            continue;
        };
        let Some(name) = overlay_names.get(&overlay) else {
            continue;
        };
        // High identities are already represented by their retained geometry.
        if is_high_bridge_body_identity(overlay, name) {
            continue;
        }
        let Some(cell) = terrain.cell(entry.rx, entry.ry) else {
            continue;
        };
        // Native47F5DA tests Crate before IsRubble. A crate uses its own
        // image, independently of any recorded BuildingType.
        let (sprite, proxy) = if flags.is_rubble && !flags.crate_type {
            let Some(type_id) = live_cell.and_then(|cell| cell.rubble_building_type) else {
                continue;
            };
            let Some((rules, interner)) = rubble_types else {
                continue;
            };
            let Some(art) = rules.building_rubble_art(interner.resolve(type_id)) else {
                continue;
            };
            // Tactical6D70CF -> Cell47FDE0 uses the overlay image's own
            // shadow rectangle BEFORE Cell47F65C selects building art.
            // Out-of-range retained data or a missing proxy must stay silent.
            let Some(proxy) = atlas.shadow_entry(name, u16::from(frame)) else {
                continue;
            };
            let Some(sprite) = atlas.shadow_entry(
                &crate::render::overlay_atlas::rubble_image_name(&art.shp),
                art.shadow_frame,
            ) else {
                continue;
            };
            (sprite, Some(proxy))
        } else {
            // Original Tactical6D70BC..6D719F and Cell47F641..47F649
            // admit every ordinary overlay by its actual shadow frame,
            // without a Wall/Crate/IsARock predicate.
            let Some(sprite) = atlas.shadow_entry(
                name,
                // BOTH drawing47F5E2 and outer bounds47FE3F force0 for
                // Crate, despite live Mark dataFF; this same selected entry
                // supplies the crop for viewport admission and drawing.
                u16::from(if flags.crate_type { 0 } else { frame }),
            ) else {
                continue;
            };
            (sprite, None)
        };
        // Empty resource shadow frames do not enter the diagonal sort. The
        // atlas owns this immutable stencil; retain its entry rather than a
        // second name/frame lookup or a per-frame image copy.
        cells.push((cell, overlay, sprite, frame, proxy));
    }
    // The native shadow sweep descends diagonals, advancing X within each.
    // Neither elevation, atlas frame nor the compatibility depth scalar owns
    // the order in which an overlap reads the previous color and Z stores.
    cells.sort_unstable_by_key(|(cell, _, _, _, _)| cell_sweep_key(cell.rx, cell.ry));
    for (cell, overlay, spr, raw_state, proxy) in cells {
        let (rx, ry) = (cell.rx, cell.ry);
        let (mut sx, mut sy) = terrain::iso_to_screen(rx, ry, cell.level);
        sy += cell_overlay_y_offset(overlay, cell.bridge_facts.raw_flags, raw_state);
        if cell.bridge_facts.raw_flags & crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF != 0 {
            if (9..=17).contains(&raw_state) {
                sx += BRIDGE_SHADOW_EW_DX as f32;
                sy += BRIDGE_SHADOW_EW_DY as f32;
            }
        }
        // Tactical6D70D0 -> 47FDE0 -> 421B60 requires positive intersection.
        // A crate uses the same forced-data0 crop for this gate and drawing;
        // rubble additionally has an independent overlay-image proxy.
        let intersects_view = |entry: &crate::render::overlay_atlas::OverlaySpriteEntry| {
            let x = sx + TILE_WIDTH / 2.0 + entry.offset_x;
            let y = sy + TILE_HEIGHT / 2.0 + entry.offset_y;
            x + entry.pixel_size[0] > cam_x
                && x < cam_x + sw
                && y + entry.pixel_size[1] > cam_y
                && y < cam_y + sh
        };
        if proxy.is_some_and(|proxy| !intersects_view(proxy)) || !intersects_view(spr) {
            continue;
        }
        let position = [
            sx + TILE_WIDTH / 2.0 + spr.offset_x,
            sy + TILE_HEIGHT / 2.0 + spr.offset_y,
        ];
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

pub(crate) fn build_bridge_shadow_instances(state: &AppState, out: &mut Vec<SpriteInstance>) {
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return;
    };
    let Some(terrain) = sim.resolved_terrain() else {
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
    let (width, height) = crate::app::input::camera::tactical_viewport_size(state);
    let zoom = state.match_state.input.zoom_level.max(f32::EPSILON);
    build_bridge_shadow_instances_inner(
        terrain,
        atlas,
        &state.match_state.match_presentation.overlay_names,
        state.match_state.match_presentation.overlays.as_slice(),
        sim.overlay_grid(),
        state.overlay_registry(),
        state.rules().map(|rules| (rules, sim.interner())),
        origin_y,
        world_height,
        cam_x,
        cam_y,
        width as f32 / zoom,
        height as f32 / zoom,
        out,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rubble_origin_emits_its_building_shadow_in_the_cell_sweep() {
        use crate::map::overlay::OverlayEntry;
        use crate::render::overlay_atlas::OverlaySpriteEntry;
        use crate::rules::{ini_parser::IniFile, overlay_types::OverlayTypeRegistry};

        struct Atlas {
            proxy: OverlaySpriteEntry,
            building: OverlaySpriteEntry,
        }
        impl BridgeAtlasLookup for Atlas {
            fn body_entry(&self, _: &str, _: u16) -> Option<&OverlaySpriteEntry> {
                None
            }
            fn shadow_entry(&self, name: &str, frame: u16) -> Option<&OverlaySpriteEntry> {
                match (name, frame) {
                    ("RUBBLE", 0) => Some(&self.proxy),
                    ("__rubble::CTHSE05.SHP", 7) => Some(&self.building),
                    _ => None,
                }
            }
        }
        let registry = OverlayTypeRegistry::from_ini(
            &IniFile::from_str("[OverlayTypes]\n0=RUBBLE\n[RUBBLE]\nIsRubble=yes\n"),
            None,
        );
        let terrain = crate::map::resolved_terrain::test_grid(
            8,
            8,
            crate::map::resolved_terrain::test_flat_cell,
        );
        let mut interner = crate::sim::intern::StringInterner::new();
        let building = interner.intern("CAHSE05");
        let mut rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n0=CAHSE05\n",
        ))
        .unwrap();
        rules.set_building_rubble_art_for_test(
            "CAHSE05",
            crate::rules::building_rubble::RubbleArt {
                shp: "CTHSE05.SHP".into(),
                body_frame: 3,
                shadow_frame: 7,
            },
        );
        let mut grid = crate::sim::overlay_grid::OverlayGrid::new(8, 8);
        let entries: Vec<_> = [(4, 4), (4, 5)]
            .into_iter()
            .map(|(rx, ry)| {
                grid.cell_mut_for_test(rx, ry).overlay_id = Some(0);
                OverlayEntry {
                    rx,
                    ry,
                    overlay_id: 0,
                    frame: 0,
                }
            })
            .collect();
        grid.cell_mut_for_test(4, 4).rubble_building_type = Some(building);
        let atlas = Atlas {
            proxy: OverlaySpriteEntry {
                uv_origin: [0.0; 2],
                uv_size: [1.0; 2],
                pixel_size: [29.0, 15.0],
                offset_x: -4.0,
                offset_y: -7.0,
                draw_point_offset: [0; 2],
                extended: false,
            },
            building: OverlaySpriteEntry {
                uv_origin: [0.0; 2],
                uv_size: [1.0; 2],
                pixel_size: [74.0, 37.0],
                offset_x: -50.0,
                offset_y: 1.0,
                draw_point_offset: [0; 2],
                extended: false,
            },
        };
        let emit = |grid: &crate::sim::overlay_grid::OverlayGrid, viewport: [f32; 4]| {
            let mut out = Vec::new();
            build_bridge_shadow_instances_inner(
                &terrain,
                &atlas,
                &BTreeMap::from([(0, "RUBBLE".into())]),
                &entries,
                Some(&grid),
                Some(&registry),
                Some((&rules, &interner)),
                0.0,
                5000.0,
                viewport[0],
                viewport[1],
                viewport[2],
                viewport[3],
                &mut out,
            );
            out
        };
        let viewport = [-1000.0, -1000.0, 4000.0, 4000.0];
        let out = emit(&grid, viewport);
        // Original 47F641..47F696 admits IsRubble only at Cell+40's
        // recorded building type; the other foundation cell has no draw.
        assert_eq!(
            out.len(),
            1,
            "the destroyed building's origin owns its shadow"
        );
        grid.cell_mut_for_test(4, 4).rubble_building_type = None;
        assert!(emit(&grid, viewport).is_empty(), "cleared origin type");
        grid.cell_mut_for_test(4, 4).rubble_building_type = Some(building);
        grid.cell_mut_for_test(4, 4).overlay_id = None;
        assert!(emit(&grid, viewport).is_empty(), "erased live overlay");
        grid.cell_mut_for_test(4, 4).overlay_id = Some(0);
        grid.cell_mut_for_test(4, 4).overlay_data = 255;
        assert!(
            emit(&grid, viewport).is_empty(),
            "retained data misses the proxy frame"
        );
        grid.cell_mut_for_test(4, 4).overlay_data = 0;
        let cell = terrain.cell(4, 4).unwrap();
        let point = crate::map::terrain::iso_to_screen(4, 4, cell.level);
        let outside_proxy = [point.0 + 30.0 - 45.0, point.1 + 15.0 - 10.0, 10.0, 100.0];
        assert!(
            emit(&grid, outside_proxy).is_empty(),
            "building stencil overlap cannot bypass native rock-proxy admission"
        );
    }

    #[test]
    fn live_overlays_and_high_bridges_share_native_shadow_sweep_order() {
        use crate::map::overlay::OverlayEntry;
        use crate::render::overlay_atlas::OverlaySpriteEntry;
        use crate::rules::{ini_parser::IniFile, overlay_types::OverlayTypeRegistry};
        struct Atlas(BTreeMap<u16, OverlaySpriteEntry>);
        impl BridgeAtlasLookup for Atlas {
            fn body_entry(&self, _: &str, _: u16) -> Option<&OverlaySpriteEntry> {
                None
            }
            fn shadow_entry(&self, _: &str, frame: u16) -> Option<&OverlaySpriteEntry> {
                self.0.get(&frame)
            }
        }
        let registry = OverlayTypeRegistry::from_ini(
            &IniFile::from_str(
                "[OverlayTypes]\n0=WALL\n1=OTHER\n[WALL]\nWall=yes\n[OTHER]\nWall=no\n",
            ),
            None,
        );
        let terrain = crate::map::resolved_terrain::test_grid(
            8,
            8,
            crate::map::resolved_terrain::test_flat_cell,
        );
        let mut grid = crate::sim::overlay_grid::OverlayGrid::new(8, 8);
        let high = grid.cell_mut_for_test(5, 5);
        high.overlay_id = Some(0x18);
        high.overlay_data = 9;
        let mut entries = Vec::new();
        for (rx, ry, published_id, live_id, frame) in [
            (4, 4, 0, Some(0), 36),
            (5, 6, 0, Some(0), 20),
            (6, 6, 0, None, 0),
            (7, 6, 0, Some(1), 5),
            (7, 7, 1, Some(1), 5),
        ] {
            entries.push(OverlayEntry {
                rx,
                ry,
                overlay_id: published_id,
                frame: 5,
            });
            let cell = grid.cell_mut_for_test(rx, ry);
            cell.overlay_id = live_id;
            cell.overlay_data = frame;
        }
        let atlas = Atlas(
            [5, 9, 20, 36]
                .into_iter()
                .map(|frame| {
                    (
                        frame,
                        OverlaySpriteEntry {
                            uv_origin: [f32::from(frame), 0.0],
                            uv_size: [1.0; 2],
                            pixel_size: [1.0; 2],
                            offset_x: 0.0,
                            offset_y: 0.0,
                            draw_point_offset: [0; 2],
                            extended: false,
                        },
                    )
                })
                .collect(),
        );
        let mut instances = Vec::new();
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
            None,
            0.0,
            5000.0,
            -1000.0,
            -1000.0,
            4000.0,
            4000.0,
            &mut instances,
        );
        // Original Tactical traversal descends diagonals across every type.
        // Tags observe actual submissions, not the cache lookup order. Live
        // damage overrides cached5; erased/replaced anchors emit no stale
        // WALL artwork and the published OTHER emits its own ordinary shadow.
        assert_eq!(
            instances
                .iter()
                .map(|sprite| sprite.uv_origin[0])
                .collect::<Vec<_>>(),
            [5.0, 20.0, 9.0, 36.0],
        );
    }

    #[test]
    fn high_body_live_fields_match_original_caller_controls() {
        use crate::render::overlay_atlas::OverlaySpriteEntry;
        struct Atlas {
            entry: OverlaySpriteEntry,
            frames: std::cell::RefCell<Vec<u16>>,
        }
        impl BridgeAtlasLookup for Atlas {
            fn body_entry(&self, _: &str, frame: u16) -> Option<&OverlaySpriteEntry> {
                self.frames.borrow_mut().push(frame);
                Some(&self.entry)
            }
            fn shadow_entry(&self, _: &str, _: u16) -> Option<&OverlaySpriteEntry> {
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
            let (mut terrain, overlays) =
                test_bridge_grid(&[((rx, ry), 0x18, input["state"].as_u64().unwrap() as u8)]);
            let cell = terrain.cell_mut_for_test(rx, ry).unwrap();
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
                    draw_point_offset: [0; 2],
                    extended: false,
                },
                frames: Default::default(),
            };
            let mut out = Vec::new();
            build_bridge_body_instances_inner(
                &terrain,
                Some(&overlays),
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
                vec![draw["frame"].as_u64().unwrap() as u16],
                "{}",
                row["name"]
            );
            assert_eq!(out.len(), 1, "{}", row["name"]);
            assert_eq!(
                out[0].instance.z_adjust,
                draw["z_adjust"].as_i64().unwrap() as f32,
                "{}",
                row["name"]
            );
            assert_eq!(
                out[0].instance.z_gradient,
                draw["gradient"].as_u64().unwrap() as u32
            );
        }
    }

    fn test_bridge_grid(
        cells: &[((u16, u16), u8, u8)],
    ) -> (ResolvedTerrainGrid, crate::sim::overlay_grid::OverlayGrid) {
        let width = cells.iter().map(|c| c.0.0).max().unwrap() + 1;
        let height = cells.iter().map(|c| c.0.1).max().unwrap() + 1;
        let mut terrain = crate::map::resolved_terrain::test_grid(
            width,
            height,
            crate::map::resolved_terrain::test_flat_cell,
        );
        let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(width, height);
        for &((x, y), overlay, state) in cells {
            let cell = terrain.cell_mut_for_test(x, y).unwrap();
            cell.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL
                | crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF;
            let live = overlays.cell_mut_for_test(x, y);
            live.overlay_id = Some(overlay);
            live.overlay_data = state;
        }
        (terrain, overlays)
    }

    #[test]
    fn startup_numeric_high_identity_emits_custom_named_body_and_shadow() {
        use crate::map::lighting::CellLightGrid;
        use crate::render::overlay_atlas::OverlaySpriteEntry;

        struct MockAtlas {
            entry: OverlaySpriteEntry,
            body_queries: std::cell::RefCell<Vec<(String, u16)>>,
            shadow_queries: std::cell::RefCell<Vec<(String, u16)>>,
        }
        impl BridgeAtlasLookup for MockAtlas {
            fn body_entry(&self, name: &str, frame: u16) -> Option<&OverlaySpriteEntry> {
                self.body_queries
                    .borrow_mut()
                    .push((name.to_string(), frame));
                (name == "HIGHANCHOR" && frame == 1).then_some(&self.entry)
            }

            fn shadow_entry(&self, name: &str, frame: u16) -> Option<&OverlaySpriteEntry> {
                self.shadow_queries
                    .borrow_mut()
                    .push((name.to_string(), frame));
                (name == "HIGHANCHOR" && frame == 1).then_some(&self.entry)
            }
        }

        // Healthy NS variant 1 is state byte 1.
        let (terrain, overlays) = test_bridge_grid(&[((5, 5), 0x18, 1)]);
        let atlas = MockAtlas {
            entry: OverlaySpriteEntry {
                uv_origin: [0.0, 0.0],
                uv_size: [1.0, 1.0],
                pixel_size: [60.0, 30.0],
                offset_x: 0.0,
                offset_y: 0.0,
                draw_point_offset: [0; 2],
                extended: false,
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
            Some(&overlays),
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
            Some(&overlays),
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
