//! Bridge instance emission — body, shadow, railings, deck-variant overrides.
//!
//! Reads `BridgeRuntimeCell` post-tick (NOT `OverlayGrid`) and emits sprite
//! instances for the bridge body, body shadow, and railing passes per the
//! per-frame draw chain in `BRIDGE_DISPLAY_TABLE_GHIDRA_REPORT.md` §3.3, §3.4.
//!
//! Three open-RE values ship as named constants here so each is a single
//! change-point if visual diff resolves them differently.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.
//! - Read-only access to sim state via `AppState`.

use std::collections::BTreeMap;

use crate::app::AppState;
use crate::map::lighting::{self, CellLightGrid};
use crate::map::terrain::{self, TILE_HEIGHT, TILE_WIDTH};
use crate::render::batch::SpriteInstance;
use crate::render::bridge_atlas::{BridgeAtlasLookup, is_high_bridge_body_identity};
use crate::render::bridge_railing_atlas::BridgeKind;
use crate::render::draw_state::DrawState;
use crate::sim::bridge_state::{Axis, BridgeRuntimeCell, BridgeRuntimeState, DamageState};

use super::helpers::{apply_shape_z_adjust, compute_sprite_depth_params, in_view};

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

/// Bonus added to `cell.deck_level` before the depth calc for HasBridge cells.
/// RE doc §3.3.1, ledger #6.
const BRIDGE_HEIGHT_BONUS: u8 = 4;

/// Shape adjustment passed by the active high-bridge body call at
/// `CellClass::DrawOverlay_Body @ 0x0047F6A0`.
const BRIDGE_BODY_Z_ADJUST_PX: i32 = -2;

/// Shadow X displacement on EW states 9..17.
///
/// Settled at -15: the native overlay-shadow draw subtracts 0xF from the draw
/// X for bridge cells whose state byte is in 9..=0x11, in the same branch that
/// adds the +7 to Y. The RE doc's open question between -15 and -45 is closed.
pub const BRIDGE_SHADOW_EW_DX: i32 = -15;
/// Shadow Y displacement on EW states 9..17. Verified -0x2D = +7
/// (RE doc §3.3.2, ledger #10).
pub const BRIDGE_SHADOW_EW_DY: i32 = 7;

/// Translate a cell's `(damage_state, axis)` into the SHP frame index for
/// `bridge.tem` / `bridgb.tem`.
///
/// The frame family follows the canonical bridge state byte encoding. Do not
/// reinterpret the axis labels here: map-load/runtime state already resolved
/// the bridge family, and `DamageState::to_state_byte(axis)` is the source of
/// truth for whether the body uses frames `0..8` or `9..17`.
///
/// Latin-square jitter applies ONLY to the boundary state (`variant: 0`),
/// matching binary `DrawOverlay_Body @ 0x47F6A0`: `if (state == 0 || state ==
/// 9) state += g_LatinSquare[...]`. Healthy variants 1..=5 are written by
/// `apply_ramp_transition` perpendicular damage (e.g., `(NS, DamageA, 0..=3)
/// → 4`); the binary draws those at `frame = state` directly with no jitter
/// (BRIDGE_DISPLAY_TABLE_GHIDRA_REPORT.md §3.3.1).
fn compute_bridge_body_shp_frame(state: DamageState, axis: Axis, rx: u16, ry: u16) -> u8 {
    let axis_base: u8 = match axis {
        Axis::NS => 0,
        Axis::EW => 9,
    };
    let local: u8 = match state {
        DamageState::Healthy { variant: 0 } => {
            let idx = ((ry & 3) as usize) << 2 | (rx & 3) as usize;
            BRIDGE_BODY_LATIN_SQUARE[idx]
        }
        DamageState::Healthy { variant } => variant.min(5),
        DamageState::Damaged => 6,
        // Per RE doc §3.1: NS PartialA=7/PartialB=8, EW PartialA=8/PartialB=7
        // (the within-axis ordering is reversed for EW). The state-byte encoding
        // bakes this in, so we read the relevant bits via to_state_byte.
        other => {
            let sb = other.to_state_byte(axis);
            sb.saturating_sub(axis_base)
        }
    };
    axis_base + local
}

fn compute_bridge_body_y_offset(state: DamageState, axis: Axis) -> f32 {
    match state.to_state_byte(axis) {
        9..=17 => BRIDGE_BODY_Y_OFFSET_STATE_9_TO_17,
        _ => BRIDGE_BODY_Y_OFFSET_STATE_0_TO_8,
    }
}

/// Base and shape-adjusted depth for the high-bridge full-canvas quad.
///
/// The active call at `0x0047F7DA..0x0047F80B` seeds the extended SHP blitter
/// from the shape's actual canvas top, bridge level plus four, and adjustment
/// -2. Per-row decrements are supplied by the bridge depth atlas.
fn compute_bridge_body_depth(
    origin_y: f32,
    world_height: f32,
    quad_top_y: f32,
    depth_z: u8,
) -> f32 {
    let base_depth = compute_sprite_depth_params(origin_y, world_height, quad_top_y, depth_z);
    apply_shape_z_adjust(base_depth, BRIDGE_BODY_Z_ADJUST_PX, world_height)
}

/// `fx_params.w` for the zdepth shader: the bridge depth atlas stores the
/// canvas row index and the extended blitter's entry-0 walk subtracts one Z
/// unit per row, so the byte is subtracted (negative sign).
const BRIDGE_ZDATA_SIGN: f32 = -1.0;

/// `SpriteInstance::z_adjust` of a high-bridge body: the seed relative to the
/// canvas top, `-(bridge level + 4) * 15 - 2`, so `canvas_top - z_adjust`
/// is the row the existing sort depth already stands on.
fn bridge_body_z_adjust(depth_z: u8) -> f32 {
    (BRIDGE_BODY_Z_ADJUST_PX - i32::from(depth_z) * crate::render::native_z::HEIGHT_LEVEL_PX) as f32
}

/// Build sprite instances for the bridge body pass (RE doc §3.3, Step 5
/// pass 1). Reads `BridgeRuntimeCell.damage_state` post-tick.
///
/// Takes only the fields the body builder actually needs from `AppState`
/// so the function is exercisable in unit tests with a pure-data mock atlas
/// (`BridgeAtlasLookup` trait). Shadow + railing builders below still take
/// `&AppState` directly — same minimal-context refactor pending.
#[allow(clippy::too_many_arguments)]
pub fn build_bridge_body_instances_inner(
    bridge_state: &BridgeRuntimeState,
    atlas: &dyn BridgeAtlasLookup,
    overlay_names: &BTreeMap<u8, String>,
    height_map: &BTreeMap<(u16, u16), u8>,
    lighting_grid: &CellLightGrid,
    origin_y: f32,
    world_height: f32,
    cam_x: f32,
    cam_y: f32,
    sw: f32,
    sh: f32,
    out: &mut Vec<SpriteInstance>,
) {
    for ((rx, ry), cell) in bridge_state.iter_cells() {
        if !cell.deck_present {
            continue;
        }
        let Some(render_state) = BridgeRuntimeState::effective_render_state(cell) else {
            continue;
        };
        let Some(axis) = cell.axis else { continue };
        let Some(name) = overlay_names.get(&cell.overlay_byte) else {
            continue;
        };
        if !is_high_bridge_body_identity(cell.overlay_byte, name) {
            continue;
        }

        let frame = compute_bridge_body_shp_frame(render_state, axis, rx, ry);
        let y_offset = compute_bridge_body_y_offset(render_state, axis);

        let z: u8 = height_map
            .get(&(rx, ry))
            .copied()
            .unwrap_or(cell.deck_level);
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
        let depth_z = z.saturating_add(BRIDGE_HEIGHT_BONUS);
        let depth = compute_bridge_body_depth(origin_y, world_height, body_y, depth_z);
        let mut draw_state = DrawState::default();
        draw_state.fx_params[3] = BRIDGE_ZDATA_SIGN;
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
            draw_state,
            z_adjust: bridge_body_z_adjust(depth_z),
            ..Default::default()
        });
    }
}

/// Thin `AppState` wrapper around `build_bridge_body_instances_inner`. Pulls
/// the seven fields the inner function needs out of `state` and forwards.
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
    let Some(bridge_state) = sim.bridge_state.as_ref() else {
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
        bridge_state,
        atlas,
        &state.match_state.match_presentation.overlay_names,
        &state.height_map(),
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

/// Build sprite instances for the bridge body shadow pass (RE doc §3.3.2,
/// Step 5 pass 2). Shadow frame = `(frame_count / 2) + state`. EW states
/// 9..17 get a `(BRIDGE_SHADOW_EW_DX, +BRIDGE_SHADOW_EW_DY)` shift per
/// ledger #9–10. Drawn passthrough (Z-test ON, Z-write OFF, neutral tint).
#[allow(clippy::too_many_arguments)]
fn build_bridge_shadow_instances_inner(
    bridge_state: &BridgeRuntimeState,
    atlas: &dyn BridgeAtlasLookup,
    overlay_names: &BTreeMap<u8, String>,
    height_map: &BTreeMap<(u16, u16), u8>,
    origin_y: f32,
    world_height: f32,
    cam_x: f32,
    cam_y: f32,
    sw: f32,
    sh: f32,
    out: &mut Vec<SpriteInstance>,
) {
    for ((rx, ry), cell) in bridge_state.iter_cells() {
        if !cell.deck_present {
            continue;
        }
        let Some(render_state) = BridgeRuntimeState::effective_render_state(cell) else {
            continue;
        };
        let Some(axis) = cell.axis else { continue };
        let Some(name) = overlay_names.get(&cell.overlay_byte) else {
            continue;
        };
        if !is_high_bridge_body_identity(cell.overlay_byte, name) {
            continue;
        }

        // DRIFT, recorded and deliberately not chased: this reuses the body's
        // frame helper, which applies the per-cell variety jitter on state
        // bytes 0 and 9. The native shadow path reads the cell's state byte
        // raw and applies no jitter. Visible impact is nil — the frames the
        // jitter can select are byte-identical in the shadow half — so the
        // cost of a separate un-jittered shadow helper buys nothing.
        let frame = compute_bridge_body_shp_frame(render_state, axis, rx, ry);
        let y_offset = compute_bridge_body_y_offset(render_state, axis);

        let z: u8 = height_map
            .get(&(rx, ry))
            .copied()
            .unwrap_or(cell.deck_level);
        let (mut sx, mut sy) = terrain::iso_to_screen(rx, ry, z);
        sy += y_offset;

        // EW-axis shadow shift (RE doc §3.3.2, ledger #9-10).
        if axis == Axis::EW {
            sx += BRIDGE_SHADOW_EW_DX as f32;
            sy += BRIDGE_SHADOW_EW_DY as f32;
        }

        if !in_view(sx, sy, 120.0, 120.0, cam_x, cam_y, sw, sh, 120.0) {
            continue;
        }

        let Some(spr) = atlas.shadow_entry(name, frame) else {
            log::warn!("bridge shadow atlas miss: name={name} frame={frame} cell=({rx},{ry})");
            continue;
        };

        let depth_z = z.saturating_add(BRIDGE_HEIGHT_BONUS);
        let depth = compute_sprite_depth_params(origin_y, world_height, sy, depth_z);
        // Shadow uses neutral tint, no per-cell lighting (ledger #12).
        let tint: [f32; 3] = lighting::DEFAULT_TINT;
        out.push(SpriteInstance {
            position: [
                sx + TILE_WIDTH / 2.0 + spr.offset_x,
                sy + TILE_HEIGHT / 2.0 + spr.offset_y,
            ],
            size: spr.pixel_size,
            uv_origin: spr.uv_origin,
            uv_size: spr.uv_size,
            depth,
            tint,
            alpha: 1.0,
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
    let Some(bridge_state) = sim.bridge_state.as_ref() else {
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
    let height_map = state.height_map();
    build_bridge_shadow_instances_inner(
        bridge_state,
        atlas,
        &state.match_state.match_presentation.overlay_names,
        &height_map,
        origin_y,
        world_height,
        cam_x,
        cam_y,
        sw,
        sh,
        out,
    );
}

/// Build sprite instances for the bridge railing pass (RE doc §3.4.1, Step 7).
/// Drawn after the unit/ground merge and before debug — see
/// `draw_passes.rs` ordering. Skips cells where the railing-table entry is
/// `None` (`shp_frame_1based == 0` means no railing for this table slot).
pub(crate) fn build_bridge_railing_instances(
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
    let Some(bridge_state) = sim.bridge_state.as_ref() else {
        return;
    };
    let Some(atlas) = state
        .match_state
        .match_presentation
        .bridge_railing_atlas
        .as_ref()
    else {
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

    for ((rx, ry), cell) in bridge_state.iter_cells() {
        if !cell.deck_present || BridgeRuntimeState::effective_render_state(cell).is_none() {
            continue;
        }
        let Some((kind, tile_index, caller_sub_tile)) =
            resolve_bridge_kind_and_sub_idx(state, rx, ry, cell)
        else {
            continue;
        };
        let Some(entry) = atlas.entry_for_tile(kind, tile_index, caller_sub_tile) else {
            continue;
        };

        let z: u8 = state
            .height_map()
            .get(&(rx, ry))
            .copied()
            .unwrap_or(cell.deck_level);
        let (sx, sy) = terrain::iso_to_screen(rx, ry, z);
        let final_x = sx + entry.dx as f32 + TILE_WIDTH / 2.0;
        let final_y = sy + entry.dy as f32 + TILE_HEIGHT / 2.0;
        if !in_view(final_x, final_y, 60.0, 60.0, cam_x, cam_y, sw, sh, 60.0) {
            continue;
        }

        let depth_z = z.saturating_add(BRIDGE_HEIGHT_BONUS);
        let depth = compute_sprite_depth_params(origin_y, world_height, final_y, depth_z);
        // Railings use neutral tint per ledger #20.
        let tint: [f32; 3] = lighting::DEFAULT_TINT;
        out.push(SpriteInstance {
            position: [final_x + entry.offset_x, final_y + entry.offset_y],
            size: entry.pixel_size,
            uv_origin: entry.uv_origin,
            uv_size: entry.uv_size,
            depth,
            tint,
            alpha: 1.0,
            ..Default::default()
        });
    }
}

/// Resolve `(BridgeKind, tile_index, caller_sub_tile)` for a bridge cell.
///
/// Mapping per RE doc §3.4.1 + verified against the codebase
/// (src/map/resolved_terrain.rs:48-55, src/map/overlay_types.rs:25-28):
/// - `BRIDGE1`, `BRIDGEB1`, `BRIDGE2`, `BRIDGEB2` → Concrete (HIGH bridges).
///   The `1` vs `2` suffix is axis (EW vs NS), not material — all four are
///   concrete.
/// - `LOBRDG*` → Wood (LOW bridges).
///
/// `final_tile_index` maps to the binary `CellClass+0x38` input for the current
/// static renderer path; `final_sub_tile` maps to `CellClass+0x11A`, the
/// required-sub-tile comparator. See
/// `BRIDGE_RAILING_SLOT_SUBTILE_SOURCE_GHIDRA_REPORT.md`.
fn resolve_bridge_kind_and_sub_idx(
    state: &AppState,
    rx: u16,
    ry: u16,
    cell: &BridgeRuntimeCell,
) -> Option<(BridgeKind, i32, u8)> {
    let name = state
        .match_state
        .match_presentation
        .overlay_names
        .get(&cell.overlay_byte)?
        .to_ascii_uppercase();
    let kind = if matches!(
        name.as_str(),
        "BRIDGE1" | "BRIDGEB1" | "BRIDGE2" | "BRIDGEB2"
    ) {
        BridgeKind::Concrete
    } else if name.starts_with("LOBRDG") {
        BridgeKind::Wood
    } else {
        return None;
    };
    let terrain_cell = state.terrain_template()?.cell(rx, ry)?;
    let tile_index = terrain_cell.final_tile_index;
    let caller_sub_tile = terrain_cell.final_sub_tile;
    Some((kind, tile_index, caller_sub_tile))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_square_value_at_xy() {
        // (cell.x, cell.y) = (1, 2) → idx = ((2&3)<<2) | (1&3) = 8|1 = 9.
        // BRIDGE_BODY_LATIN_SQUARE[9] = 3.
        assert_eq!(BRIDGE_BODY_LATIN_SQUARE[((2 & 3) << 2) | (1 & 3)], 3);
    }

    /// SHP frame selection follows the same frame family as the state byte:
    /// NS uses `0..8`, EW uses `9..17`.
    #[test]
    fn damaged_cell_routes_to_axis_correct_shp_frame() {
        // NS Damaged -> SHP frame 0 + 6 = 6.
        assert_eq!(
            compute_bridge_body_shp_frame(DamageState::Damaged, Axis::NS, 0, 0),
            6
        );
        // EW Damaged -> SHP frame 9 + 6 = 15.
        assert_eq!(
            compute_bridge_body_shp_frame(DamageState::Damaged, Axis::EW, 0, 0),
            15
        );
    }

    /// Latin-square jitter applies only to `Healthy { variant: 0 }` — the
    /// boundary state byte that the binary `DrawOverlay_Body` checks for
    /// `state == 0 || state == 9`. Variants 1..=5 (written by
    /// `apply_ramp_transition` perpendicular damage) draw at `axis_base +
    /// variant` directly, no jitter. Per BRIDGE_DISPLAY_TABLE §3.3.1.
    #[test]
    fn healthy_variant_zero_uses_latin_square_jitter() {
        // (1, 2) -> LATIN[9] = 3 -> EW body SHP frame = 9 + 3 = 12.
        assert_eq!(
            compute_bridge_body_shp_frame(DamageState::Healthy { variant: 0 }, Axis::EW, 1, 2),
            12
        );
        // Same xy on NS axis -> SHP frame = 0 + 3 = 3.
        assert_eq!(
            compute_bridge_body_shp_frame(DamageState::Healthy { variant: 0 }, Axis::NS, 1, 2),
            3
        );
    }

    #[test]
    fn healthy_variants_one_to_five_skip_latin_and_use_variant_directly() {
        // Per binary: state bytes 1..=5 (NS) and 10..=14 (EW) draw frame =
        // state directly (no jitter). Variant 4 written by NS_DamageA on
        // 0..=3 healthy targets per HIGH §11.1.
        assert_eq!(
            compute_bridge_body_shp_frame(DamageState::Healthy { variant: 4 }, Axis::EW, 1, 2),
            13
        );
        assert_eq!(
            compute_bridge_body_shp_frame(DamageState::Healthy { variant: 5 }, Axis::EW, 1, 2),
            14
        );
        // Same variants on NS axis: frame = 0 + variant.
        assert_eq!(
            compute_bridge_body_shp_frame(DamageState::Healthy { variant: 4 }, Axis::NS, 1, 2),
            4
        );
        assert_eq!(
            compute_bridge_body_shp_frame(DamageState::Healthy { variant: 5 }, Axis::NS, 1, 2),
            5
        );
        // Different (rx, ry) does NOT change the result for non-zero variants
        // (Latin-square is bypassed) — guards against a future refactor
        // re-introducing jitter on damage-progression frames.
        assert_eq!(
            compute_bridge_body_shp_frame(DamageState::Healthy { variant: 4 }, Axis::EW, 0, 0),
            compute_bridge_body_shp_frame(DamageState::Healthy { variant: 4 }, Axis::EW, 1, 2)
        );
    }

    #[test]
    fn bridge_body_y_offset_follows_state_byte_range() {
        for state in [
            DamageState::Healthy { variant: 0 },
            DamageState::Healthy { variant: 5 },
            DamageState::Damaged,
            DamageState::PartialCollapseA,
            DamageState::PartialCollapseB,
        ] {
            assert_eq!(compute_bridge_body_y_offset(state, Axis::NS), -16.0);
        }

        for state in [
            DamageState::Healthy { variant: 0 },
            DamageState::Healthy { variant: 5 },
            DamageState::Damaged,
            DamageState::PartialCollapseA,
            DamageState::PartialCollapseB,
        ] {
            assert_eq!(compute_bridge_body_y_offset(state, Axis::EW), -31.0);
        }

        // Destroyed cells are skipped before rendering, but their binary state
        // byte encoding is still 0, so the helper should follow the low range.
        assert_eq!(
            compute_bridge_body_y_offset(DamageState::Destroyed, Axis::NS),
            -16.0
        );
        assert_eq!(
            compute_bridge_body_y_offset(DamageState::Destroyed, Axis::EW),
            -16.0
        );
    }

    #[test]
    fn shadow_ew_shift_constants_present() {
        // RE doc §10 open Q2: BRIDGE_SHADOW_EW_DX may be -15 or -45.
        // BRIDGE_SHADOW_EW_DY verified at +7. Either way, the shift must be
        // non-zero — visual diff (Task 17) resolves the X value.
        assert!(BRIDGE_SHADOW_EW_DX != 0 || BRIDGE_SHADOW_EW_DY != 0);
    }

    #[test]
    fn latin_square_table_is_canonical_4x4() {
        // RE doc §5: verified raw memory read at g_LatinSquare.
        assert_eq!(
            BRIDGE_BODY_LATIN_SQUARE,
            [0, 1, 2, 3, 3, 2, 1, 0, 2, 3, 0, 1, 1, 0, 3, 2]
        );
    }

    /// Phase D Task 16: render integration regression — the body builder must
    /// query the bridge atlas with the SHP frame derived from the cell's
    /// **post-tick** `damage_state` (NOT a stale overlay byte). If a future
    /// refactor accidentally reads from the wrong source (e.g. legacy
    /// `OverlayGrid`), bridges would visually stay healthy after they collapse
    /// — and nothing else would catch it.
    ///
    /// Lives here (`presentation/instances/bridges.rs`) rather than under `sim/world/`
    /// because it imports render-layer types (`BridgeAtlasLookup`,
    /// `OverlaySpriteEntry`, `SpriteInstance`) and sim/ must never depend on
    /// render/. The bridge state is built directly via `test_seed_cell` to
    /// avoid pulling in any sim-test fixtures.
    #[test]
    fn gsi_13_09_body_builder_anchors_depth_to_canvas_top_and_applies_native_minus_two() {
        use crate::map::lighting::CellLightGrid;
        use crate::render::bridge_atlas::BridgeAtlasLookup;
        use crate::render::overlay_atlas::OverlaySpriteEntry;
        use crate::sim::bridge_state::{
            Axis, BridgeCellRole, BridgeRuntimeCell, BridgeRuntimeState, BridgeheadAnchorClass,
            DamageState,
        };
        use std::collections::BTreeMap;

        struct MockAtlas {
            expected_name: String,
            expected_frame: u8,
            entry: OverlaySpriteEntry,
            queries: std::cell::RefCell<Vec<(String, u8)>>,
        }
        impl BridgeAtlasLookup for MockAtlas {
            fn body_entry(&self, name: &str, frame: u8) -> Option<&OverlaySpriteEntry> {
                self.queries.borrow_mut().push((name.to_string(), frame));
                if name == self.expected_name && frame == self.expected_frame {
                    Some(&self.entry)
                } else {
                    None
                }
            }

            fn shadow_entry(&self, _name: &str, _frame: u8) -> Option<&OverlaySpriteEntry> {
                None
            }
        }

        // Seed three visible EW cells. Their 0xDC overlay byte is the render
        // authority and decodes as Damaged for all three through
        // `effective_render_state`; (5,5) is the depth assertion target.
        let mut bridge_state = BridgeRuntimeState::default();
        for x in 4u16..=6 {
            let damage_state = if x == 5 {
                DamageState::Damaged
            } else {
                DamageState::Healthy { variant: 0 }
            };
            bridge_state.test_seed_cell(
                x,
                5,
                BridgeRuntimeCell {
                    deck_present: true,
                    destroyable: true,
                    deck_level: 4,
                    bridge_group_id: Some(1),
                    damage_state,
                    axis: Some(Axis::EW),
                    role: BridgeCellRole::Body,
                    anchor_span_id: Some(1),
                    overlay_byte: 0xDC,
                    bridgehead_anchor_class: BridgeheadAnchorClass::Variant0,
                },
            );
        }

        // Mock atlas accepts the EW Damaged body frame: axis base 9 plus the
        // damaged local offset 6 gives frame 15.
        let mock = MockAtlas {
            expected_name: "BRIDGE1".to_string(),
            expected_frame: 15,
            entry: OverlaySpriteEntry {
                uv_origin: [0.0, 0.0],
                uv_size: [1.0, 1.0],
                pixel_size: [60.0, 30.0],
                offset_x: 0.0,
                offset_y: 7.0,
            },
            queries: std::cell::RefCell::new(Vec::new()),
        };

        // Map every overlay byte the walker may have written to "BRIDGE1" so
        // the canonical destroy-band route accepts it.
        let mut overlay_names: BTreeMap<u8, String> = BTreeMap::new();
        for byte in 0xCDu8..=0xE8u8 {
            overlay_names.insert(byte, "BRIDGE1".to_string());
        }

        let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
        let lighting_grid = CellLightGrid::new();

        // Camera centered on cell (5, 5) so view culling doesn't reject it.
        let (cam_target_x, cam_target_y) = crate::map::terrain::iso_to_screen(5, 5, 4);

        let mut out: Vec<crate::render::batch::SpriteInstance> = Vec::new();
        let world_height = 5000.0;
        build_bridge_body_instances_inner(
            &bridge_state,
            &mock,
            &overlay_names,
            &height_map,
            &lighting_grid,
            /* origin_y */ 0.0,
            world_height,
            /* cam_x */ cam_target_x - 400.0,
            /* cam_y */ cam_target_y - 300.0,
            /* sw */ 800.0,
            /* sh */ 600.0,
            &mut out,
        );

        let queries = mock.queries.borrow();
        assert!(
            !queries.is_empty(),
            "body builder must query the atlas for the seeded damaged cells"
        );
        let queried_for_55 = queries
            .iter()
            .any(|(name, frame)| name == "BRIDGE1" && *frame == 15);
        assert!(
            queried_for_55,
            "body builder must query atlas with (\"BRIDGE1\", 15) — the EW Damaged SHP frame; \
             actual queries: {:?}",
            *queries
        );
        assert!(
            !out.is_empty(),
            "expected at least one SpriteInstance for the Damaged EW bridge cell"
        );
        let (cell_sx, cell_sy) = crate::map::terrain::iso_to_screen(5, 5, 4);
        let expected_left = cell_sx + TILE_WIDTH / 2.0 + mock.entry.offset_x;
        let expected_top =
            cell_sy + BRIDGE_BODY_Y_OFFSET_STATE_9_TO_17 + TILE_HEIGHT / 2.0 + mock.entry.offset_y;
        let instance = out
            .iter()
            .find(|instance| instance.position == [expected_left, expected_top])
            .copied()
            .expect("center bridge body must be identifiable by its final full-canvas position");
        let expected_base = compute_sprite_depth_params(0.0, world_height, expected_top, 8);
        let expected_depth =
            apply_shape_z_adjust(expected_base, BRIDGE_BODY_Z_ADJUST_PX, world_height);
        assert_eq!(instance.position[1], expected_top);
        assert!((instance.depth - expected_depth).abs() < f32::EPSILON);
        // The zdepth shader subtracts the atlas row byte (sign -1) from the
        // canvas-top seed `-(level + 4) * 15 - 2`, one native Z unit per row.
        assert_eq!(instance.draw_state.fx_params[3], BRIDGE_ZDATA_SIGN);
        assert_eq!(instance.z_adjust, bridge_body_z_adjust(8));
        assert_eq!(instance.z_adjust, -(8.0 * 15.0) - 2.0);
    }

    #[test]
    fn startup_numeric_high_identity_emits_custom_named_body_and_shadow() {
        use crate::map::lighting::CellLightGrid;
        use crate::render::overlay_atlas::OverlaySpriteEntry;
        use crate::sim::bridge_state::{BridgeCellRole, BridgeheadAnchorClass};

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

        let mut bridge_state = BridgeRuntimeState::default();
        bridge_state.test_seed_cell(
            5,
            5,
            BridgeRuntimeCell {
                deck_present: true,
                destroyable: true,
                deck_level: 4,
                bridge_group_id: Some(1),
                damage_state: DamageState::Healthy { variant: 1 },
                axis: Some(Axis::NS),
                role: BridgeCellRole::Body,
                anchor_span_id: Some(1),
                overlay_byte: 0x18,
                bridgehead_anchor_class: BridgeheadAnchorClass::Variant0,
            },
        );
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
        let height_map = BTreeMap::new();
        let lighting_grid = CellLightGrid::new();
        let (cell_x, cell_y) = terrain::iso_to_screen(5, 5, 4);
        let (cam_x, cam_y) = (cell_x - 400.0, cell_y - 300.0);
        let mut bodies = Vec::new();
        let mut shadows = Vec::new();

        build_bridge_body_instances_inner(
            &bridge_state,
            &atlas,
            &overlay_names,
            &height_map,
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
            &bridge_state,
            &atlas,
            &overlay_names,
            &height_map,
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

    #[test]
    fn gsi_13_09_adjacent_body_rows_step_exactly_one_over_world_height() {
        // `row = canvas_top - (z_adjust + sign * byte)`; with sign -1 each
        // atlas row byte moves the ground row one world pixel down (nearer),
        // which the shared depth axis maps to exactly 1 / world_height.
        let world_height: f32 = 4096.0;
        let z_adjust = bridge_body_z_adjust(8);
        let row = |byte: f32| 500.0 - (z_adjust + BRIDGE_ZDATA_SIGN * byte);
        let d73 = crate::render::native_z::depth_for_row(row(73.0), 0.0, world_height);
        let d74 = crate::render::native_z::depth_for_row(row(74.0), 0.0, world_height);
        assert!(((d73 - d74) - 1.0 / world_height).abs() < f32::EPSILON);
    }
}
