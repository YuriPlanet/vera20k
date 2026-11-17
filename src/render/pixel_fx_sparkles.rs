//! Per-frame water/ore sparkle render — observable parity with gamemd.exe's
//! DrawPixelFXSparkles. See ra2-rust-game-docs/PIXEL_FX_SPARKLES_GHIDRA_REPORT.md
//! for the full reverse-engineering and the design doc at
//! docs/plans/2026-05-18-pixel-fx-sparkle-design.md.
//!
//! Each visited water or ore cell keeps gamemd's `PixelFXClass` state
//! ([`PixelFxCells`]): a random wait, then one pulse from the dim base colour
//! to the bright peak and back, then a new wait at a new spot. A cell draws
//! nothing while it waits.
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

/// `TimerInitMask` (`+0x1C`) of both tables: the wait before a pulse is
/// `rand() & 0xFFF` milliseconds.
const TIMER_INIT_MASK: u32 = 0xFFF;

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
fn ping_pong_lerp(phase: u32, base: [i32; 3], peak: [i32; 3]) -> [i32; 3] {
    let mut lerp = (phase & 0xFFF) as i32;
    if (phase & 0x1000) != 0 {
        lerp = 0x1000 - lerp;
    }
    let inv = 0x1000 - lerp;
    // Signed `IMUL` and `SAR 0xC`, as `Update_Color` computes each channel.
    std::array::from_fn(|i| (base[i] * inv + peak[i] * lerp) >> 12)
}

/// One cell's `PixelFXClass` (0x3C bytes, held at `CellClass+0xFC`). The
/// field comments give each member's native offset.
#[derive(Debug, Clone, Copy, Default)]
struct PixelFx {
    /// `+0x04..+0x0C`, the colour the last `Update_Color` left.
    current: [i32; 3],
    /// `+0x10`, 0..=0x2000 over one pulse.
    phase: i32,
    /// `+0x14`, phase per millisecond.
    lerp_speed: i32,
    /// `+0x18..+0x20`.
    peak: [i32; 3],
    /// `+0x24..+0x2C`.
    base: [i32; 3],
    /// `+0x30`, `+0x34`: the pixel's offset from the cell's centre.
    offset: [i32; 2],
    /// `+0x38`, the wait in milliseconds before the pulse shows.
    timer: i32,
}

impl PixelFx {
    /// `PixelFXClass::Init @ 0x00631D40`, four `rand` draws (three for ore,
    /// which has no colour noise).
    fn init(&mut self, params: &SparkleParams, rand: &mut CrtRand) {
        let r = rand.next();
        self.offset = [(r & 0x3F) as i32 - 0x1F, ((r >> 5) & 0x1F) as i32 - 0xF];
        self.peak = params.peak_rgb.map(i32::from);
        if params.color_noise_bits != 0 {
            let shift = u32::from(params.color_noise_bits);
            let mask = (1u32 << shift) - 1;
            let r = rand.next();
            self.peak[0] -= (r & mask) as i32;
            self.peak[1] -= ((r >> shift) & mask) as i32;
            self.peak[2] -= ((r >> (2 * shift)) & mask) as i32;
        }
        self.base = params.base_rgb.map(i32::from);
        self.current = self.base;
        self.phase = 0;
        let span = params.lerp_speed_max - params.lerp_speed_min + 1;
        self.lerp_speed = (rand.next() % span + params.lerp_speed_min) as i32;
        self.timer = (rand.next() & TIMER_INIT_MASK) as i32;
    }

    /// `PixelFXClass::Tick_Timer @ 0x00631EE0`: true once the wait is over,
    /// and on every pass after it until the next `Init`.
    fn tick_timer(&mut self, delta_ms: u32) -> bool {
        self.timer = self.timer.wrapping_sub(delta_ms as i32);
        if self.timer < 1 {
            self.timer = 0;
            return true;
        }
        false
    }

    /// `PixelFXClass::Update_Color @ 0x00631E50`.
    fn update_color(&mut self, delta_ms: u32) {
        self.phase = self
            .phase
            .wrapping_add(self.lerp_speed.wrapping_mul(delta_ms as i32))
            .min(0x2000);
        let phase = self.phase as u32;
        self.current = ping_pong_lerp(phase, self.base, self.peak);
    }
}

/// The MSVC CRT `rand` (`_rand`) PixelFX draws from. Presentation only: it is
/// never the simulation's random stream.
#[derive(Debug, Clone, Copy)]
struct CrtRand(u32);

impl Default for CrtRand {
    /// The CRT's seed before any `srand`.
    fn default() -> Self {
        Self(1)
    }
}

impl CrtRand {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(214_013).wrapping_add(2_531_011);
        (self.0 >> 16) & 0x7FFF
    }
}

/// The sparkle pass's retained state: each visited cell's PixelFX (gamemd
/// keeps it at `CellClass+0xFC` until the map is freed), the `rand` stream,
/// and the last pass's clock (`DAT_00B0E640`).
#[derive(Debug, Default)]
pub struct PixelFxCells {
    cells: std::collections::HashMap<(u16, u16), PixelFx>,
    rand: CrtRand,
    last_ms: u32,
}

impl PixelFxCells {
    /// A new map frees its cells and their PixelFX; the clock and the `rand`
    /// stream are process globals and stay.
    pub fn clear_on_load(&mut self) {
        self.cells.clear();
    }
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
    /// The wall clock in milliseconds (`timeGetTime`, `0x006D7896`).
    pub wall_ms: u32,
    /// The pass's two opening gates: the published frame rate is not below
    /// `Game__GetMinFrameRate @ 0x0055AF60`, and `[Options] DetailLevel` is
    /// not 0. If false, build returns an empty Vec and leaves the clock.
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

/// One cell's turn in the pass (`0x006D7A9A..0x006D7BB4`): create its
/// PixelFX, or re-arm one whose pulse reached 0x2000 (`0x006D7AD7`), run its
/// wait down (`0x006D7AF7`), and once the wait is over advance the pulse and
/// draw its pixel (`0x006D7B65..0x006D7BB4`) if that pixel lies in the
/// tactical rectangle (`0x006D7B41..0x006D7B5C`). Returns None
/// when the cell doesn't qualify or shows nothing this pass.
fn compute_sparkle_for_cell(
    rx: u16,
    ry: u16,
    delta_ms: u32,
    input: &SparkleInput<'_>,
    cells: &NativeCellQuery<'_>,
    fx: &mut PixelFxCells,
) -> Option<SpriteInstance> {
    let (_is_ore, params) = gate_cell(rx, ry, input, cells)?;
    let PixelFxCells {
        cells: fx_cells,
        rand,
        ..
    } = fx;
    let sparkle = match fx_cells.entry((rx, ry)) {
        std::collections::hash_map::Entry::Vacant(entry) => {
            let mut sparkle = PixelFx::default();
            sparkle.init(params, rand);
            entry.insert(sparkle)
        }
        std::collections::hash_map::Entry::Occupied(entry) => {
            let sparkle = entry.into_mut();
            if sparkle.phase >= 0x2000 {
                sparkle.init(params, rand);
            }
            sparkle
        }
    };
    if !sparkle.tick_timer(delta_ms) {
        return None;
    }

    // The cell's tile rectangle centre plus the sparkle's offset. iso_to_screen
    // returns the NW corner of the cell's bounding diamond; shift by half a
    // tile to land on the centre. Elevation is taken from the resolved terrain
    // cell.
    let cell = input.resolved_terrain.cell(rx, ry)?;
    let (sx_nw, sy_nw) = iso_to_screen(rx, ry, cell.level);
    let x = sx_nw + TILE_WIDTH / 2.0 + sparkle.offset[0] as f32;
    let y = sy_nw + TILE_HEIGHT / 2.0 + sparkle.offset[1] as f32;
    // Only a pixel inside the tactical rectangle advances and draws.
    if !(input.camera_x..input.camera_x + input.viewport_w).contains(&x)
        || !(input.camera_y..input.camera_y + input.viewport_h).contains(&y)
    {
        return None;
    }
    sparkle.update_color(delta_ms);
    let channel = |value: i32| value.clamp(0, 255) as f32 / 255.0;

    // Emit (L12: 1×1 size; L28: alpha=1.0 opaque).
    Some(SpriteInstance {
        position: [x, y],
        size: [1.0, 1.0],
        uv_origin: input.white_uv_origin,
        uv_size: input.white_uv_size,
        depth: SPARKLE_DEPTH,
        tint: sparkle.current.map(channel),
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
pub fn build_sparkle_instances(
    input: &SparkleInput<'_>,
    fx: &mut PixelFxCells,
) -> Vec<SpriteInstance> {
    if !input.enable_extra_animations {
        return Vec::new();
    }
    // `0x006D7896..0x006D78B7`: the time since the last pass, at most 1 s.
    let delta_ms = input.wall_ms.wrapping_sub(fx.last_ms).min(1000);
    fx.last_ms = input.wall_ms;
    // The draw's own query: a miss stamps its dummy, not the simulation's.
    let cells = NativeCellQuery::isolated(input.resolved_terrain);
    sparkle_cells(
        [input.camera_x, input.camera_y],
        [input.viewport_w, input.viewport_h],
        input.map_size,
    )
    .filter_map(|(rx, ry)| compute_sparkle_for_cell(rx, ry, delta_ms, input, &cells, fx))
    .collect()
}

#[cfg(all(test, tests_render_a_r))]
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
        let mut input = SparkleInput {
            wall_ms: 0,
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
        // Passes 16 ms apart: the cell waits up to 4095 ms, drawing nothing,
        // then pulses.
        let mut fx = PixelFxCells::default();
        let passes: Vec<_> = (1..=400)
            .map(|pass| {
                input.wall_ms = pass * 16;
                build_sparkle_instances(&input, &mut fx)
            })
            .collect();
        assert!(passes[0].is_empty(), "the first pass falls in the wait");
        let shown: Vec<_> = passes.into_iter().flatten().collect();
        assert!(!shown.is_empty());
        for sparkle in shown {
            let [x, y] = sparkle.position;
            assert!(
                (x - 240.0).abs() <= 32.0 && (y - 510.0).abs() <= 16.0,
                "({x}, {y})"
            );
        }
    }

    /// `PixelFXClass::Tick_Timer @ 0x00631EE0` gates the pixel write: calm
    /// water shows nothing while its sparkle waits, then one pulse from the
    /// dim base to the bright peak and back, then nothing again.
    #[test]
    fn a_water_sparkle_shows_only_while_it_pulses() {
        let mut fx = PixelFxCells::default();
        let mut sparkle = PixelFx::default();
        sparkle.init(&WATER, &mut fx.rand);
        let wait = sparkle.timer;
        let pulse_ms = (0x2000 + sparkle.lerp_speed - 1) / sparkle.lerp_speed;
        let mut colours = Vec::new();
        let mut shown_at = Vec::new();
        for ms in 1..=(wait + pulse_ms + 50) {
            if sparkle.phase >= 0x2000 {
                break;
            }
            if sparkle.tick_timer(1) {
                sparkle.update_color(1);
                colours.push(sparkle.current);
                shown_at.push(ms);
            }
        }
        assert_eq!(
            shown_at.first(),
            Some(&wait.max(1)),
            "nothing shows during the wait"
        );
        assert_eq!(colours.last(), Some(&[40, 40, 80]), "the pulse ends dim");
        let brightest = colours.iter().map(|rgb| rgb[2]).max().unwrap();
        assert!(brightest > 190, "{brightest}");
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
}
