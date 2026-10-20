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

use crate::map::resolved_terrain::ResolvedTerrainGrid;
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

/// Compute the cell bounds (inclusive) of the visible viewport.
///
/// Camera is in world pixels (top-left corner of viewport). vsw and vsh are
/// the effective viewport width and height in world pixels (already
/// zoom-corrected by the caller). Returns (rx_min, ry_min, rx_max, ry_max),
/// clamped to map bounds [0, map_w) and [0, map_h).
///
/// Iso cells are 60 wide × 30 tall (CELL_WIDTH × CELL_HEIGHT in render). We
/// add a 2-cell margin on every side so partially-visible cells at the edges
/// are included. (L27 — sparkles should appear at viewport edges.)
#[inline]
fn viewport_cell_bounds(
    camera_x: f32,
    camera_y: f32,
    vsw: f32,
    vsh: f32,
    map_w: u16,
    map_h: u16,
) -> (u16, u16, u16, u16) {
    const MARGIN_CELLS: i32 = 2;

    // World-pixel viewport rect → approximate cell range. Iso conversion is
    // approximate (we over-include because of the diamond shape) but cheap
    // and correct: the gate inside compute_sparkle_for_cell handles cells
    // outside the actual viewport via the screen position check.
    let rx_min =
        (((camera_x / TILE_WIDTH).floor() as i32) - MARGIN_CELLS).clamp(0, map_w as i32 - 1) as u16;
    let rx_max = ((((camera_x + vsw) / TILE_WIDTH).ceil() as i32) + MARGIN_CELLS)
        .clamp(0, map_w as i32 - 1) as u16;
    let ry_min = (((camera_y / TILE_HEIGHT).floor() as i32) - MARGIN_CELLS)
        .clamp(0, map_h as i32 - 1) as u16;
    let ry_max = ((((camera_y + vsh) / TILE_HEIGHT).ceil() as i32) + MARGIN_CELLS)
        .clamp(0, map_h as i32 - 1) as u16;
    (rx_min, ry_min, rx_max, ry_max)
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
    /// Local player's interned house ID for the sight check (L19). None when
    /// no map is loaded or no owner can be resolved — gate then fails.
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
    pub map_w: u16,
    pub map_h: u16,
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
fn gate_cell(rx: u16, ry: u16, input: &SparkleInput<'_>) -> Option<(bool, &'static SparkleParams)> {
    let cell = input.resolved_terrain.cell(rx, ry)?;
    let has_ore = ore_value_nonzero(rx, ry, input);
    // L17: `CMP dword ptr [ESI + 0xEC], 0x2` @ 0x006D7A6C, the cell's
    // LandType against Water.
    let water = cell.land_type() == crate::rules::terrain_rules::LandType::Water.as_index();
    if !water && !has_ore {
        return None;
    }

    if let Some(occ) = input.occupancy.get(rx, ry)
        && !occ.occupants.is_empty()
    {
        return None; // L18
    }

    if !input.sandbox_full_visibility {
        let owner_id = input.local_owner_id?; // L19 / L21
        if !input.fog.is_cell_visible(owner_id, rx, ry) {
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
) -> Option<SpriteInstance> {
    let (_is_ore, params) = gate_cell(rx, ry, input)?;
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
/// Cell iteration uses the module's own viewport-cell bounds computation; we
/// don't reuse the terrain pass's iteration to keep the sparkle module
/// self-contained (see design doc §Architectural Decisions).
pub fn build_sparkle_instances(input: &SparkleInput<'_>) -> Vec<SpriteInstance> {
    if !input.enable_extra_animations {
        return Vec::new();
    }
    let clock_ms = input.clock_ms;
    let (rx_min, ry_min, rx_max, ry_max) = viewport_cell_bounds(
        input.camera_x,
        input.camera_y,
        input.viewport_w,
        input.viewport_h,
        input.map_w,
        input.map_h,
    );
    let mut out: Vec<SpriteInstance> = Vec::with_capacity(256);
    for ry in ry_min..=ry_max {
        for rx in rx_min..=rx_max {
            if let Some(inst) = compute_sparkle_for_cell(rx, ry, clock_ms, input) {
                out.push(inst);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
