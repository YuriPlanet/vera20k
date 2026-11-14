//! Per-frame water/ore sparkle render — observable parity with gamemd.exe's
//! DrawPixelFXSparkles. See ra2-rust-game-docs/PIXEL_FX_SPARKLES_GHIDRA_REPORT.md
//! for the full reverse-engineering and the design doc at
//! docs/plans/2026-05-18-pixel-fx-sparkle-design.md.
//!
//! Stateless / hash-derived: each visible water or ore cell, every frame,
//! hashes (cell_coord, cycle_index) to derive sub-pixel position, peak
//! colour noise, lerp speed, and timer-init for the current cycle, then
//! computes the sparkle's current RGB analytically. No per-cell persistent
//! state.
//!
//! ## Dependency rules
//! - Part of render/ — reads sim/ state through immutable references only.
//!   No writes to sim. No coupling to GPU types beyond SpriteInstance.

use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainGrid};
use crate::map::terrain::iso_to_screen;
use crate::render::batch::SpriteInstance;
use crate::sim::intern::InternedId;
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::vision::FogState;

/// Sparkle sprites draw through the passthrough sprite pipeline (depth-bypass +
/// no blend). The depth value is required by SpriteInstance but does not affect
/// visibility inside this pass; pass-ordering at the tactical tail, after
/// shroud and tactical overlays but before screen-fixed chrome, does the work.
/// Matches the constant-depth pattern used by smudge::build_visible_instances.
const SPARKLE_DEPTH: f32 = 0.5;

/// Tile dimensions used by iso_to_screen — derived from the map-layer
/// authority.
const TILE_WIDTH: f32 = crate::map::terrain::TILE_WIDTH;
const TILE_HEIGHT: f32 = crate::map::terrain::TILE_HEIGHT;

/// Per-species sparkle parameters mirroring gamemd's
/// g_PixelFXParams_Water (0x008367C8) and g_PixelFXParams_Ore (0x008367F0)
/// tables. Read directly from the binary; see report §5.2.
#[derive(Debug, Clone, Copy)]
struct SparkleParams {
    /// Dim endpoint of the lerp. Applied with weight (0x1000 - lerp).
    base_rgb: [u8; 3],
    /// Bright endpoint of the lerp. Applied with weight lerp; per cycle, each
    /// channel may be reduced by `0..(1 << color_noise_bits)`.
    peak_rgb: [u8; 3],
    /// Per-channel noise bit count subtracted from peak. 0 = no noise (ore).
    color_noise_bits: u8,
    /// Inclusive lower bound for the per-cell-per-cycle LerpSpeed (phase / ms).
    lerp_speed_min: u32,
    /// Inclusive upper bound for the per-cell-per-cycle LerpSpeed.
    lerp_speed_max: u32,
}

/// Water sparkle constants — verified by direct memory read at
/// gamemd.exe 0x008367C8. See report §5.2. (L1, L2, L3, L4)
const WATER: SparkleParams = SparkleParams {
    base_rgb: [40, 40, 80],
    peak_rgb: [158, 158, 224],
    color_noise_bits: 5,
    lerp_speed_min: 3,
    lerp_speed_max: 12,
};

/// Ore sparkle constants — verified by direct memory read at
/// gamemd.exe 0x008367F0. See report §5.2. (L6, L7, L8, L9)
const ORE: SparkleParams = SparkleParams {
    base_rgb: [176, 144, 0],
    peak_rgb: [255, 255, 240],
    color_noise_bits: 0,
    lerp_speed_min: 15,
    lerp_speed_max: 30,
};

/// Average cycle length for the stateless cycle-bucket approximation.
/// gamemd's per-cycle duration is (timer_init 0..4095 ms) + (active
/// 0x2000/lerp_speed ms). Avg ≈ 2048 + 430 = 2478 ms. Round to 2500 for
/// both species (coincidentally similar). See design doc §Cycle bucketing.
const WATER_CYCLE_BUCKET_MS: u64 = 2500;
const ORE_CYCLE_BUCKET_MS: u64 = 2500;

/// Splitmix64 — Vigna's PRNG, used here as a one-shot 64→64 bit hash.
/// Three operations: add, xor-shift-multiply (×2). Well-distributed; avalanche
/// quality is more than enough for "looks random per pixel."
#[inline]
fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// Pack cell coordinates into a 64-bit key for hashing. Layout puts rx in
/// the high 16 bits of the upper 32, ry in the high 16 bits of the lower 32,
/// leaving the low 32 bits as a 0 sentinel that the caller can XOR with
/// cycle_index when mixing per-cycle entropy.
#[inline]
fn coord_key(rx: u16, ry: u16) -> u64 {
    ((rx as u64) << 32) | ((ry as u64) << 16)
}

/// Ping-pong lerp between base (dim) and peak (bright) colors.
///
/// Phase is the position within a cycle, domain [0, 0x2000). The cycle is
/// symmetric: phase [0, 0x1000) rises from base to peak; phase [0x1000,
/// 0x2000) falls from peak back to base. (L13, L14, L16)
///
/// Per-channel formula (L15):
///     current = (base * (0x1000 - lerp) + peak * lerp) >> 12
///
/// where `lerp = phase & 0xFFF`, optionally flipped if bit 0x1000 is set.
#[inline]
fn ping_pong_lerp(phase: u32, base: [u8; 3], peak: [u8; 3]) -> [u8; 3] {
    let mut lerp = phase & 0xFFF;
    if (phase & 0x1000) != 0 {
        lerp = 0x1000 - lerp;
    }
    let inv = 0x1000 - lerp;
    let blend = |b: u8, p: u8| -> u8 {
        // (base * inv + peak * lerp) >> 12. Use u32 to avoid overflow
        // (255 * 0x1000 = 1,044,480, fits in u32 easily).
        (((b as u32) * inv + (p as u32) * lerp) >> 12) as u8
    };
    [
        blend(base[0], peak[0]),
        blend(base[1], peak[1]),
        blend(base[2], peak[2]),
    ]
}

/// Extract a per-cycle sub-pixel offset (sub_x, sub_y) from the seed bits.
/// Ranges match gamemd's PixelFXClass::Init (report §6.1):
///   sub_x ∈ [-31, 32] (6 bits → bias by -0x1F)   (L10)
///   sub_y ∈ [-15, 16] (5 bits → bias by -0x0F)   (L11)
#[inline]
fn sub_pos_from_seed(s: u64) -> (i32, i32) {
    let sub_x = ((s & 0x3F) as i32) - 0x1F;
    let sub_y = (((s >> 6) & 0x1F) as i32) - 0x0F;
    (sub_x, sub_y)
}

/// Extract this cycle's LerpSpeed, biased into the species range.
/// gamemd uses `rand() % (max - min + 1) + min` — we mirror that. Uses
/// 4 bits of entropy starting at bit 23, leaving bits 11-22 for timer_init.
/// (L4 for water, L9 for ore)
#[inline]
fn lerp_speed_from_seed(s: u64, params: &SparkleParams) -> u32 {
    let range_span = params.lerp_speed_max - params.lerp_speed_min + 1;
    let raw = (s >> 23) & 0xF;
    params.lerp_speed_min + (raw as u32) % range_span
}

/// Extract this cycle's timer_init offset (0..4095 ms).
/// gamemd: `rand() & 0xFFF` — we do the same. Uses 12 bits starting at bit 11.
/// (L5/L26)
#[inline]
fn timer_init_from_seed(s: u64) -> u32 {
    ((s >> 11) & 0xFFF) as u32
}

/// Compute this cycle's peak RGB with per-channel noise subtract.
/// gamemd applies `mask = (1 << color_noise_bits) - 1`, then per channel:
///   peak[i] -= mask & rand_bits
///   rand_bits >>= color_noise_bits
/// We use 5 bits per channel from bit 27 onwards (15 bits total). For ore
/// (color_noise_bits = 0), no noise is applied. (L3, L8)
#[inline]
fn peak_with_noise(s: u64, params: &SparkleParams) -> [u8; 3] {
    if params.color_noise_bits == 0 {
        return params.peak_rgb;
    }
    let mask = (1u32 << params.color_noise_bits) - 1;
    let bits = s >> 27;
    let n0 = (bits as u32) & mask;
    let n1 = ((bits >> params.color_noise_bits) as u32) & mask;
    let n2 = ((bits >> (params.color_noise_bits * 2)) as u32) & mask;
    [
        params.peak_rgb[0].saturating_sub(n0 as u8),
        params.peak_rgb[1].saturating_sub(n1 as u8),
        params.peak_rgb[2].saturating_sub(n2 as u8),
    ]
}

/// The cells `DrawPixelFXSparkles @ 0x006D7840` visits, in its order, for a
/// view whose top-left world pixel is `camera` and whose size is `view`
/// world pixels, on a map of `Size=` width and height `map_size`.
///
/// gamemd is handed the tactical rectangle (`0x006D491F..0x006D4924`), so it
/// starts from the view origin's cell on the Z=0 plane (the pixel-to-coordinate
/// transform `0x006D78CC..0x006D7962`,
/// [`crate::map::terrain::screen_to_plane_leptons`]),
/// two cells back along X. It walks `height / 15 + 17` rows of
/// `width / 60 + 4` cells (`0x006D7983..0x006D79C5`): row `r` starts `r / 2`
/// cells along X and `(r + 1) / 2` along Y from there (`0x006D79CF..
/// 0x006D7A05`), and each step to the right is one cell along X and one back
/// along Y (`0x006D7BB8..0x006D7BC1`). The extra rows reach raised cells
/// drawn up into the view. Cells off the map's `Size=` diamond are passed by
/// (`MapClass::In_Bounds @ 0x00568300`, `0x006D7A28`). The cell words wrap as
/// gamemd's 16-bit ones do.
fn sparkle_cells(
    camera: [f32; 2],
    view: [f32; 2],
    map_size: (i32, i32),
) -> impl Iterator<Item = (u16, u16)> {
    let origin = crate::map::terrain::screen_to_plane_leptons(camera[0], camera[1]);
    let start = ((origin.x / 256) as i16).wrapping_sub(2);
    let start_y = (origin.y / 256) as i16;
    let rows = view[1] as i32 / 15 + 17;
    let columns = view[0] as i32 / 60 + 4;
    (0..rows.max(0)).flat_map(move |row| {
        let x = start.wrapping_add((row / 2) as i16);
        let y = start_y.wrapping_add(((row + 1) / 2) as i16);
        (0..columns.max(0)).filter_map(move |column| {
            let cell = (x.wrapping_add(column as i16), y.wrapping_sub(column as i16));
            if !crate::map::playfield::size_diamond_contains(map_size.0, map_size.1, cell) {
                return None;
            }
            Some((u16::try_from(cell.0).ok()?, u16::try_from(cell.1).ok()?))
        })
    })
}

/// All read-only state the sparkle pass needs. Borrowed for the duration of
/// the build call; nothing escapes the function.
pub struct SparkleInput<'a> {
    /// Authoritative sim-time clock in milliseconds. Caller passes
    /// `Simulation.session.total_sim_ms` (pre-computed each tick by the sim).
    /// Deterministic across clients on the same tick — replays look identical.
    pub clock_ms: u64,
    /// From the live `[Options] DetailLevel != 0` projection. If false, build
    /// returns an empty Vec. (L22)
    pub enable_extra_animations: bool,
    /// Local player's interned house ID, whose map the gate reads (L19). None
    /// when no map is loaded or no owner can be resolved — gate then fails.
    pub local_owner_id: Option<InternedId>,
    /// True when sandbox-mode visibility bypass is active; sight gate (L19/L21)
    /// becomes a no-op when set. Mirrors the existing terrain-pass pattern.
    pub sandbox_full_visibility: bool,
    pub resolved_terrain: &'a ResolvedTerrainGrid,
    pub overlays: &'a OverlayGrid,
    pub overlay_registry: &'a crate::rules::overlay_types::OverlayTypeRegistry,
    pub occupancy: &'a OccupancyGrid,
    pub fog: &'a FogState,
    pub camera_x: f32,
    pub camera_y: f32,
    pub viewport_w: f32,
    pub viewport_h: f32,
    /// The map's `Size=` width and height (`MapClass+0xF4`, `+0xF8`).
    pub map_size: (i32, i32),
    /// White-texel UV coords; for `SelectionOverlay::white_texture()` which
    /// is a 1×1 texture, this is the full (0,0) → (1,1) rect.
    pub white_uv_origin: [f32; 2],
    pub white_uv_size: [f32; 2],
}

/// Run the 6-gate check (subset of gamemd's 9-condition gate per report §4).
/// Skipped conditions (vs gamemd):
///   - "16-bit RGB565 surface mode" — not applicable to wgpu RGBA8 path.
///   - "surface lock succeeds" — wgpu doesn't lock surfaces; always succeeds.
///   - "viewport clip" — handled by depth/scissor, not a per-cell check.
///
/// Returns (is_ore, params) when the cell qualifies; None otherwise.
fn gate_cell(
    rx: u16,
    ry: u16,
    input: &SparkleInput<'_>,
    cells: &NativeCellQuery<'_>,
) -> Option<(bool, &'static SparkleParams)> {
    let cell = input.resolved_terrain.cell(rx, ry)?;
    let has_ore = ore_value_nonzero(rx, ry, input);
    // L17: `CMP dword ptr [ESI + 0xEC], 0x2` @ 0x006D7A6C, the cell's
    // LandType against Water.
    let water = cell.land_type == crate::rules::terrain_rules::LandType::Water.as_index();
    if !water && !has_ore {
        return None;
    }

    if let Some(occ) = input.occupancy.get(rx, ry)
        && !occ.occupants.is_empty()
    {
        return None; // L18
    }

    // L19 / L21: the local player's map, not its sight. gamemd asks for the
    // ground-open bit (`Cell+0x12C & 0x10`, `0x006D7A4F`) and that the cell is
    // not shrouded (`CellClass::IsShrouded @ 0x00487950`, `0x006D7A5F`), so
    // open water out of every unit's sight still sparkles.
    if !input.sandbox_full_visibility {
        let owner_id = input.local_owner_id?;
        if !input.fog.is_ground_open(owner_id, rx, ry)
            || crate::sim::vision::cell_is_shrouded(input.fog, cells, owner_id, (rx, ry))
        {
            return None;
        }
    }

    if cell.bridge_walkable {
        return None; // L20: bridge-deck cells skipped
    }

    let params: &'static SparkleParams = if has_ore { &ORE } else { &WATER };
    Some((has_ore, params))
}

/// Lookup the cell's tiberium-ore flag via the overlay grid + registry.
/// Returns true iff the cell has a non-empty tiberium overlay.
/// `OverlayCell.overlay_id` is `Option<u8>` (None = no overlay), so we
/// unwrap-or-skip via `.and_then`.
#[inline]
fn ore_value_nonzero(rx: u16, ry: u16, input: &SparkleInput<'_>) -> bool {
    input
        .overlays
        .cell(rx, ry)
        .overlay_id
        .and_then(|id| input.overlay_registry.flags(id))
        .is_some_and(|f| f.tiberium)
}

/// Compute one cell's sparkle for the given clock time. Returns None when
/// the cell doesn't qualify (gate fails). Caller pushes the returned
/// SpriteInstance into a Vec.
fn compute_sparkle_for_cell(
    rx: u16,
    ry: u16,
    clock_ms: u64,
    input: &SparkleInput<'_>,
    cells: &NativeCellQuery<'_>,
) -> Option<SpriteInstance> {
    let (_is_ore, params) = gate_cell(rx, ry, input, cells)?;
    let bucket_ms = if _is_ore {
        ORE_CYCLE_BUCKET_MS
    } else {
        WATER_CYCLE_BUCKET_MS
    };

    // L26: per-cell offset hashed from coord-only key, breaks global beat sync.
    let cell_offset_ms = splitmix64(coord_key(rx, ry)) % bucket_ms;
    let shifted_t = clock_ms + cell_offset_ms;
    let cycle_index = shifted_t / bucket_ms;
    let cycle_pos_ms = shifted_t % bucket_ms;

    // L24: re-randomize sub-pos, color noise, lerp speed, timer_init each cycle
    // by mixing cycle_index into the seed.
    let s = splitmix64(coord_key(rx, ry) ^ cycle_index);

    let (sub_x, sub_y) = sub_pos_from_seed(s);
    let timer_init_ms = timer_init_from_seed(s);
    let lerp_speed = lerp_speed_from_seed(s, params);
    let peak = peak_with_noise(s, params);

    let active_duration_ms = 0x2000u32 / lerp_speed;

    // L23: cells START dim each cycle (during the timer_init wait, draw base).
    // L25: most of cycle is dim — peak is brief mid-active.
    // After active phase ends, sit at base until bucket boundary.
    let current_rgb = if (cycle_pos_ms as u32) < timer_init_ms {
        params.base_rgb
    } else if (cycle_pos_ms as u32) < timer_init_ms + active_duration_ms {
        let active_progress = cycle_pos_ms as u32 - timer_init_ms;
        let phase = (active_progress * lerp_speed) & 0x1FFF;
        ping_pong_lerp(phase, params.base_rgb, peak)
    } else {
        params.base_rgb
    };

    // Cell center in screen pixels. iso_to_screen returns the NW corner of
    // the cell's bounding diamond; shift by half a tile to land on the centre.
    // Elevation is taken from the resolved terrain cell.
    let cell = input.resolved_terrain.cell(rx, ry)?;
    let (sx_nw, sy_nw) = iso_to_screen(rx, ry, cell.level);
    let screen_x = sx_nw + TILE_WIDTH / 2.0;
    let screen_y = sy_nw + TILE_HEIGHT / 2.0;

    // Emit (L12: 1×1 size; L28: alpha=1.0 opaque).
    Some(SpriteInstance {
        position: [screen_x + sub_x as f32, screen_y + sub_y as f32],
        size: [1.0, 1.0],
        uv_origin: input.white_uv_origin,
        uv_size: input.white_uv_size,
        depth: SPARKLE_DEPTH,
        tint: [
            current_rgb[0] as f32 / 255.0,
            current_rgb[1] as f32 / 255.0,
            current_rgb[2] as f32 / 255.0,
        ],
        alpha: 1.0,
        ..Default::default()
    })
}

/// Build one SpriteInstance per qualifying water/ore cell in the viewport.
///
/// Returns an empty Vec if `enable_extra_animations` is off — checked up-front
/// so the viewport iteration is skipped entirely (zero work). (L22)
///
/// The cells come from gamemd's own walk, [`sparkle_cells`].
pub fn build_sparkle_instances(input: &SparkleInput<'_>) -> Vec<SpriteInstance> {
    if !input.enable_extra_animations {
        return Vec::new();
    }
    let clock_ms = input.clock_ms;
    // The draw's own query: a miss stamps its dummy, not the simulation's.
    let cells = NativeCellQuery::isolated(input.resolved_terrain);
    sparkle_cells(
        [input.camera_x, input.camera_y],
        [input.viewport_w, input.viewport_h],
        input.map_size,
    )
    .filter_map(|(rx, ry)| compute_sparkle_for_cell(rx, ry, clock_ms, input, &cells))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_walk_reaches_every_cell_centred_in_a_view_away_from_the_map_corner() {
        // A 200 x 200 map seen from world pixel (3000, 3300), 800 x 600 pixels:
        // the view's cells, all on the map, lie around rx 160..190, ry 45..80.
        let (camera, view, size) = ([3000.0, 3300.0], [800.0, 600.0], (200, 200));
        let walked: std::collections::BTreeSet<_> = sparkle_cells(camera, view, size).collect();
        let mut in_view = 0;
        for rx in 0..=400_u16 {
            for ry in 0..=400_u16 {
                let (x, y) = iso_to_screen(rx, ry, 0);
                let (x, y) = (x + TILE_WIDTH / 2.0, y + TILE_HEIGHT / 2.0);
                if (camera[0]..camera[0] + view[0]).contains(&x)
                    && (camera[1]..camera[1] + view[1]).contains(&y)
                {
                    in_view += 1;
                    assert!(walked.contains(&(rx, ry)), "({rx}, {ry}) is in view");
                }
            }
        }
        assert!(in_view > 200, "{in_view} cells in view");
    }

    #[test]
    fn open_water_out_of_every_units_sight_still_sparkles() {
        // One water cell, (20, 12), drawn centred on world pixel (240, 510), on
        // a map its viewer has explored but sees none of.
        const SIDE: u16 = 32;
        let cells = (0..SIDE)
            .flat_map(|ry| {
                (0..SIDE).map(move |rx| {
                    let mut cell = crate::map::resolved_terrain::test_smudge_cell(rx, ry);
                    if (rx, ry) == (20, 12) {
                        cell.land_type = crate::rules::terrain_rules::LandType::Water.as_index();
                    }
                    cell
                })
            })
            .collect();
        let terrain = ResolvedTerrainGrid::from_cells(SIDE, SIDE, cells);
        let owner = InternedId::from_index(1);
        let mut fog = FogState::new(SIDE, SIDE);
        fog.reveal_all_for_owner(owner);
        assert!(!fog.is_cell_visible(owner, 20, 12));
        let input = SparkleInput {
            clock_ms: 0,
            enable_extra_animations: true,
            local_owner_id: Some(owner),
            sandbox_full_visibility: false,
            resolved_terrain: &terrain,
            overlays: &OverlayGrid::new(SIDE, SIDE),
            overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry::empty(),
            occupancy: &OccupancyGrid::default(),
            fog: &fog,
            camera_x: 0.0,
            camera_y: 300.0,
            viewport_w: 800.0,
            viewport_h: 600.0,
            map_size: (16, 16),
            white_uv_origin: [0.0; 2],
            white_uv_size: [1.0; 2],
        };
        let sparkles = build_sparkle_instances(&input);
        assert_eq!(sparkles.len(), 1);
        let [x, y] = sparkles[0].position;
        assert!(
            (x - 240.0).abs() <= 32.0 && (y - 510.0).abs() <= 16.0,
            "({x}, {y})"
        );
    }

    #[test]
    fn lerp_at_phase_0_is_base() {
        // L16: phase 0 → base color (sparkle just spawned, dim).
        // L23: cells START dim each cycle.
        let result = ping_pong_lerp(0, [40, 40, 80], [158, 158, 224]);
        assert_eq!(result, [40, 40, 80]);
    }

    #[test]
    fn lerp_at_phase_0x1000_is_peak() {
        // L16: phase 0x1000 → peak color (sparkle at brightest).
        let result = ping_pong_lerp(0x1000, [40, 40, 80], [158, 158, 224]);
        assert_eq!(result, [158, 158, 224]);
    }

    #[test]
    fn lerp_ping_pong_symmetry() {
        // L14: phase (0x1000 - x) and (0x1000 + x) must yield same color
        // for any x in 1..0x1000. This is the ping-pong invariant.
        let base = [40, 40, 80];
        let peak = [158, 158, 224];
        for x in [1u32, 100, 0x400, 0x800, 0xFFF] {
            let rising = ping_pong_lerp(0x1000 - x, base, peak);
            let falling = ping_pong_lerp(0x1000 + x, base, peak);
            assert_eq!(rising, falling, "asymmetry at x={:#x}", x);
        }
    }

    #[test]
    fn cell_offset_breaks_sync_for_neighbours() {
        // L26: adjacent cells must produce different cell_offset_ms at the
        // same clock_ms (else they'd peak together → visible map-wide pulse).
        let bucket = WATER_CYCLE_BUCKET_MS;
        let off_a = splitmix64(coord_key(50, 50)) % bucket;
        let off_b = splitmix64(coord_key(51, 50)) % bucket;
        let off_c = splitmix64(coord_key(50, 51)) % bucket;
        assert_ne!(off_a, off_b);
        assert_ne!(off_a, off_c);
        // Spread check: over many neighbour pairs, average offset diff
        // should be > bucket/8 (catches degenerate hashes).
        let mut diff_sum: u64 = 0;
        let mut count = 0u64;
        for x in 0u16..20 {
            for y in 0u16..20 {
                let a = splitmix64(coord_key(x, y)) % bucket;
                let b = splitmix64(coord_key(x + 1, y)) % bucket;
                diff_sum += a.abs_diff(b);
                count += 1;
            }
        }
        let avg = diff_sum / count;
        assert!(
            avg > bucket / 8,
            "avg neighbour offset diff too small: {} < {}",
            avg,
            bucket / 8
        );
    }
}
