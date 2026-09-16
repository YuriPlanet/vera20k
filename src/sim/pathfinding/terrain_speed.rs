//! Runtime per-cell speed modifiers applied during movement execution.
//!
//! The original engine applies terrain speed as a runtime modifier (not an
//! A* cost weight). The stages are combined each tick in this exact order —
//! the order is load-bearing, because stage 3 tests for *exact* zero and stages
//! 2 and 4 can both move a value off zero:
//!
//! 1. **Terrain type** — from rules.ini land-type sections ([Clear] Foot=100%,
//!    etc.), read for the *destination* cell. The only clamp here is an upper
//!    one: a value strictly above 1.0 is replaced by 1.0. There is **no lower
//!    clamp on the raw land-row value** — a 0% row stays 0.0 through stage 2.
//! 2. **Slope** — vehicles moving up/down a grade, chosen by SpeedType (Track vs
//!    other) and travel direction. Vanilla: uphill ×1.0 (no change), downhill
//!    ×1.2 (faster).
//! 3. **Zero substitution** — if the terrain × slope product is *exactly* 0.0 it
//!    is replaced by 0.5. This is a substitution on the combined value, not a
//!    floor: it is the reason a 0%-land-row mover moves at half speed instead of
//!    freezing, and it must run after the slope multiply, or a 0% row going
//!    downhill would yield 0.6 instead of 0.5.
//! 4. **Damaged mover** — ×0.75 when the owner's health ratio is at or below
//!    `[AudioVisual] ConditionYellow` (vanilla 50%). The test is `<=`.
//!
//! The original engine applies **no clamp to the combined value** at any point
//! in this chain.
//!
//! **Only the Drive and Ship locomotors run this chain.** In the original engine
//! the land-type × SpeedType table has exactly two speed consumers, both inside
//! `Process_Movement`; the slope coefficients appear at exactly the same two
//! sites; and the damaged-mover factor is read from the same two. Walk, Hover,
//! Mech, Jumpjet, Teleport, Tunnel, Fly and Rocket movement never touch any of
//! them — the hover locomotor in particular runs a pure accel/brake throttle
//! that never looks at land type, so a hover unit is NOT halved on land.
//!
//! The original engine has no crowd/density speed term: congestion is resolved by
//! blocking and re-pathing, never by scaling a mover's speed. A former synthetic
//! crowd-jam factor (radius-2 occupancy scan → 0.7×) was removed — it had no
//! source in the original engine and no INI key driving it (invented behavior).
//!
//! Depends on: `ResolvedTerrainGrid` (cell height + land type),
//! `SpeedCostProfile` (INI-parsed terrain percentages).

use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::{LocomotorKind, SpeedType};
use crate::util::fixed_math::{SIM_HALF, SIM_ONE, SimFixed};

// --- Constants from the original engine ---

/// Original engine clamps terrain speed multipliers above 1.0 to exactly 1.0.
/// This is the *only* clamp the land-row read performs — the compare is `<= 1.0
/// keep, else 1.0`, so a 0% row is passed through untouched.
const TERRAIN_SPEED_MAX: SimFixed = SIM_ONE;

/// Substituted for the terrain × slope product when that product is *exactly*
/// zero. The original engine tests the combined value against 0.0 and, on
/// equality, overwrites it with this constant — it is a substitution, not a
/// floor, and it runs after the slope multiply and before the damaged-mover
/// factor.
///
/// Original: `DriveLocomotionClass::Process_Movement`. The zero compare reloads
/// the value the slope multiply just stored, and all three height branches
/// (uphill / downhill / level) join at that reload, so the test never sees the
/// pre-slope row. The 0.5 is an immediate in the substitution branch, not a
/// table constant. Verified against the live binary 2026-08-04.
const ZERO_COMBINED_SUBSTITUTE: SimFixed = SIM_HALF;

/// Speed penalty applied to a mover whose health ratio is at or below
/// `[AudioVisual] ConditionYellow` (vanilla `50%`).
///
/// The original engine multiplies the already-computed per-cell speed fraction
/// by this constant, after the terrain × slope product, in Drive and Ship
/// `Process_Movement` and nowhere else.
const DAMAGED_MOVER_SPEED_FACTOR: SimFixed = SimFixed::lit("0.75");

/// `ConditionYellow` is stored as a fraction; the sim compares it as an integer
/// scaled by this factor so no float enters the health test.
const CONDITION_YELLOW_SCALE: i64 = 1000;

/// Whether the mover's health ratio is at or below `[AudioVisual] ConditionYellow`.
///
/// The original engine's test is `ratio <= ConditionYellow` — a unit sitting
/// exactly on the threshold IS penalised. `condition_yellow_x1000` is the
/// pre-scaled integer form parsed from `[AudioVisual]`; the cross-multiply keeps
/// the comparison in integers, matching the building damage-state gate.
pub fn is_at_or_below_condition_yellow(
    current_hp: i64,
    max_hp: i64,
    condition_yellow_x1000: i64,
) -> bool {
    max_hp > 0 && current_hp * CONDITION_YELLOW_SCALE <= max_hp * condition_yellow_x1000
}

/// Whether this locomotor is one of the two that route through the land-type ×
/// SpeedType speed table, the slope coefficients and the damaged-mover factor.
///
/// Drive and Ship `Process_Movement` are the complete set in the original
/// engine. The slope term there carries an extra "is this a vehicle" gate, which
/// is redundant in stock YR because only vehicles and ships install these two
/// locomotors.
fn uses_land_type_speed_chain(kind: LocomotorKind) -> bool {
    matches!(kind, LocomotorKind::Drive | LocomotorKind::Ship)
}

/// Cliff/slope speed coefficients from `[General]` in rules.ini.
///
/// The original engine keeps four: tracked vs wheeled vehicles, each with an
/// uphill and a downhill coefficient, selected by the mover's SpeedType (Track
/// uses the tracked pair, every other SpeedType uses the wheeled pair) and by
/// travel direction. Vanilla values are 1.0 uphill / 1.2 downhill for both.
#[derive(Debug, Clone)]
pub struct TerrainSpeedConfig {
    /// Tracked vehicle moving uphill (`TrackedUphill=`; vanilla 1.0).
    pub tracked_uphill: SimFixed,
    /// Tracked vehicle moving downhill (`TrackedDownhill=`; vanilla 1.2).
    pub tracked_downhill: SimFixed,
    /// Non-tracked (wheeled and other) vehicle moving uphill (`WheeledUphill=`; vanilla 1.0).
    pub wheeled_uphill: SimFixed,
    /// Non-tracked vehicle moving downhill (`WheeledDownhill=`; vanilla 1.2).
    pub wheeled_downhill: SimFixed,
}

impl Default for TerrainSpeedConfig {
    fn default() -> Self {
        // Vanilla rulesmd.ini [General]: 1.0 uphill / 1.2 downhill for both pairs.
        Self {
            tracked_uphill: SIM_ONE,
            tracked_downhill: SimFixed::lit("1.2"),
            wheeled_uphill: SIM_ONE,
            wheeled_downhill: SimFixed::lit("1.2"),
        }
    }
}

impl TerrainSpeedConfig {
    /// Build config from the four parsed `[General]` slope coefficients.
    pub fn from_general(
        tracked_uphill: SimFixed,
        tracked_downhill: SimFixed,
        wheeled_uphill: SimFixed,
        wheeled_downhill: SimFixed,
    ) -> Self {
        Self {
            tracked_uphill,
            tracked_downhill,
            wheeled_uphill,
            wheeled_downhill,
        }
    }
}

/// Compute the combined per-cell speed multiplier for a unit moving between cells.
///
/// Returns `1.0` unchanged for every locomotor the original engine does not route
/// through the land-type table (see [`uses_land_type_speed_chain`]); otherwise the
/// terrain × slope product, then the damaged-mover factor when
/// `below_condition_yellow` is set.
pub fn compute_cell_speed_modifier(
    speed_type: SpeedType,
    locomotor_kind: LocomotorKind,
    mover_world: (i32, i32),
    next_cell: (u16, u16),
    terrain: &ResolvedTerrainGrid,
    config: &TerrainSpeedConfig,
    below_condition_yellow: bool,
) -> SimFixed {
    if !uses_land_type_speed_chain(locomotor_kind) {
        return SIM_ONE;
    }
    let terrain_factor = terrain_speed_factor(speed_type, next_cell, terrain);
    // Two sampled ground heights, as native does: the mover's exact position
    // against the destination cell's own coordinate.
    let slope_factor = slope_factor_for(
        speed_type,
        ground_height_at_world(mover_world, terrain),
        ground_height_at_world(cell_centre_world(next_cell), terrain),
        config,
    );

    combine_speed_stages(terrain_factor, slope_factor, below_condition_yellow)
}

/// Stages 2–4 of the per-cell multiplier chain, isolated so the ordering can be
/// pinned without building a terrain grid.
///
/// `terrain_factor` has already had its `> 1.0 → 1.0` cap applied and may be
/// exactly zero.
fn combine_speed_stages(
    terrain_factor: SimFixed,
    slope_factor: SimFixed,
    below_condition_yellow: bool,
) -> SimFixed {
    // Stage 3: exact-zero substitution on the *combined* value. Ordering
    // matters — a 0% land row multiplied by the 1.2 downhill coefficient is
    // still exactly 0, so the substitution yields 0.5 here where a pre-slope
    // floor would have yielded 0.6.
    let mut combined = terrain_factor * slope_factor;
    if combined == SimFixed::from_num(0) {
        combined = ZERO_COMBINED_SUBSTITUTE;
    }
    // Stage 4: damaged mover. No clamp follows it in the original engine.
    if below_condition_yellow {
        combined * DAMAGED_MOVER_SPEED_FACTOR
    } else {
        combined
    }
}

/// Terrain height of a cell, defaulting to 0 outside the grid.
fn cell_level(cell: (u16, u16), terrain: &ResolvedTerrainGrid) -> u8 {
    terrain.cell(cell.0, cell.1).map(|c| c.level).unwrap_or(0)
}

/// Ground height in leptons under an exact world position.
///
/// `CellClass::ComputeGroundHeightAtCoord 0x0047B3A0`, through the evaluator
/// that `tools/ramp_height_vectors.json` already pins against 158 native
/// fixtures. The low bytes of the world coordinates carry the sub-cell offset,
/// which is the whole point: on a ramp, two positions in one cell have
/// different ground heights.
///
/// Falls back to the cell's flat base when the position is off-grid or the
/// slope is one the evaluator does not model, which is the behaviour this
/// function replaced (a pure level-byte reading) and so cannot regress.
fn ground_height_at_world(world: (i32, i32), terrain: &ResolvedTerrainGrid) -> i32 {
    let cell = (
        crate::sim::cell_kernel::world_to_cell_trunc(world.0),
        crate::sim::cell_kernel::world_to_cell_trunc(world.1),
    );
    let (level, slope) = match (u16::try_from(cell.0), u16::try_from(cell.1)) {
        (Ok(x), Ok(y)) => terrain
            .cell(x, y)
            .map_or((0, 0), |c| (c.level, c.slope_type)),
        _ => (0, 0),
    };
    crate::sim::cell_kernel::cell_floor_height(level, slope, world.0, world.1).unwrap_or_else(
        |_| i32::from(level as i8) * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS,
    )
}

/// The centre of a cell, in world leptons - what native samples for the
/// destination (`0x004B3CEC` passes the destination coordinate, not the
/// mover's).
fn cell_centre_world(cell: (u16, u16)) -> (i32, i32) {
    (i32::from(cell.0) * 256 + 128, i32::from(cell.1) * 256 + 128)
}

/// Factor 1: terrain type speed from INI land-type percentages.
///
/// Looks up the *destination* cell's terrain speed for the unit's SpeedType.
/// Matches original engine: `> 100%` → 100%, missing → 100%, and **0% passes
/// through as 0.0** — the 50% rescue happens later, on the combined value, in
/// [`compute_cell_speed_modifier`].
fn terrain_speed_factor(
    speed_type: SpeedType,
    next_cell: (u16, u16),
    terrain: &ResolvedTerrainGrid,
) -> SimFixed {
    let Some(cell) = terrain.cell(next_cell.0, next_cell.1) else {
        return SIM_ONE;
    };
    let multiplier = cell.speed_costs.speed_multiplier_for(speed_type);
    if multiplier > TERRAIN_SPEED_MAX {
        TERRAIN_SPEED_MAX
    } else {
        multiplier
    }
}

/// Pick the slope coefficient from two sampled ground heights, in leptons.
///
/// Destination higher than the mover = uphill; lower = downhill; equal = no
/// change. Track SpeedType uses the tracked pair, every other SpeedType the
/// wheeled pair — matching the original engine's `SpeedType == Track` test
/// (infantry are handled by a separate precomputed-foot mechanism and don't
/// reach this vehicle path).
///
/// These are **heights**, not cell level bytes. Native compares two
/// `GetGroundHeight` results (`0x004B3CEC` destination, `0x004B3D1A` the mover's
/// own coordinate, compared at `0x004B3D21`), and the two differ exactly where a
/// mover is partway down a ramp whose cell level already equals the flat cell it
/// is leaving for — which is about half of all ramp-to-flat adjacencies on the
/// stock maps, so it is the ordinary case rather than a corner.
fn slope_factor_for(
    speed_type: SpeedType,
    cur_height: i32,
    next_height: i32,
    config: &TerrainSpeedConfig,
) -> SimFixed {
    let tracked = speed_type == SpeedType::Track;
    if next_height > cur_height {
        // Uphill.
        if tracked {
            config.tracked_uphill
        } else {
            config.wheeled_uphill
        }
    } else if next_height < cur_height {
        // Downhill.
        if tracked {
            config.tracked_downhill
        } else {
            config.wheeled_downhill
        }
    } else {
        SIM_ONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::terrain_rules::SpeedCostProfile;

    /// A8 D1: leaving a ramp onto a flat cell at the ramp's own level is
    /// downhill, and reading level bytes cannot see that.
    ///
    /// Native compares two `GetGroundHeight` samples - the destination's
    /// coordinate at `0x004B3CEC` and the mover's own at `0x004B3D1A`, compared
    /// at `0x004B3D21`. This port compared the two cells' integer `level` bytes,
    /// which are equal in exactly this shape, so it applied 1.0 where gamemd
    /// applies `Tracked/WheeledDownhill`.
    ///
    /// Stated without assuming which way a given ramp faces: somewhere inside a
    /// ramp cell there is a position whose ground height differs from the flat
    /// neighbour's, while the two cells' level bytes are identical. That is the
    /// whole defect, and the sampled comparison sees it.
    #[test]
    fn a_ramp_position_differs_in_height_from_a_flat_cell_at_the_same_level() {
        use crate::util::lepton::ground_height_leptons;

        const LEVEL: u8 = 3;
        let flat = ground_height_leptons(LEVEL, 0, 5 * 256 + 128, 5 * 256 + 128)
            .expect("a flat cell is always evaluable");

        // Sample across one ramp cell of each modelled slope.
        let mut found_any = false;
        for slope in 1..=20u8 {
            let Ok(corner) = ground_height_leptons(LEVEL, slope, 4 * 256 + 8, 4 * 256 + 8) else {
                continue;
            };
            let Ok(opposite) = ground_height_leptons(LEVEL, slope, 4 * 256 + 248, 4 * 256 + 248)
            else {
                continue;
            };
            if corner == flat && opposite == flat {
                continue;
            }
            found_any = true;

            // The level bytes are equal, so the old level-byte comparison called
            // every one of these flat.
            let config = TerrainSpeedConfig::default();
            let by_level = slope_factor_for(
                SpeedType::Track,
                i32::from(LEVEL),
                i32::from(LEVEL),
                &config,
            );
            assert_eq!(by_level, SIM_ONE, "equal levels always read as flat");

            let sampled = slope_factor_for(SpeedType::Track, corner, flat, &config);
            let sampled_other = slope_factor_for(SpeedType::Track, opposite, flat, &config);
            assert!(
                sampled != SIM_ONE || sampled_other != SIM_ONE,
                "slope {slope}: sampling must see a grade the level bytes hide"
            );
        }
        assert!(
            found_any,
            "no modelled slope varied the height inside a cell; the premise is wrong"
        );
    }

    #[test]
    fn speed_multiplier_for_normal_terrain() {
        let profile = SpeedCostProfile {
            foot: Some(100),
            track: Some(100),
            ..Default::default()
        };
        assert_eq!(profile.speed_multiplier_for(SpeedType::Foot), SIM_ONE);
    }

    #[test]
    fn speed_multiplier_for_rough_terrain() {
        let profile = SpeedCostProfile {
            track: Some(75),
            ..Default::default()
        };
        let mult = profile.speed_multiplier_for(SpeedType::Track);
        assert_eq!(mult, SimFixed::lit("0.75"));
    }

    #[test]
    fn gsi_04_04_speed_profile_preserves_zero_multiplier() {
        let profile = SpeedCostProfile {
            foot: Some(0),
            ..Default::default()
        };
        assert_eq!(
            profile.speed_multiplier_for(SpeedType::Foot),
            SimFixed::from_num(0),
        );
    }

    #[test]
    fn speed_multiplier_none_defaults_to_one() {
        let profile = SpeedCostProfile::default();
        assert_eq!(profile.speed_multiplier_for(SpeedType::Foot), SIM_ONE);
    }

    /// RC-7 / AT-13: percentages above 100 clamp to full speed (1.0); a sub-100
    /// percentage passes through as its fraction. The original never lets a
    /// terrain speed bonus push a unit faster than its base speed.
    #[test]
    fn speed_multiplier_clamps_at_one() {
        let fast = SpeedCostProfile {
            foot: Some(120),
            ..Default::default()
        };
        assert_eq!(fast.speed_multiplier_for(SpeedType::Foot), SIM_ONE);

        // 80% is below the cap, so it passes through as the fraction 80/100
        // (no clamp). Compare against the same fixed-point division the
        // implementation performs — a `lit("0.8")` literal differs by 1 ULP
        // because 0.8 is not exactly representable in binary fixed-point.
        let slow = SpeedCostProfile {
            foot: Some(80),
            ..Default::default()
        };
        let pass_through = SimFixed::from_num(80u8) / SimFixed::from_num(100u8);
        assert_eq!(slow.speed_multiplier_for(SpeedType::Foot), pass_through);
        assert!(pass_through < SIM_ONE, "80% must stay below the 1.0 cap");
    }

    /// GSI-06.04 G1: the retail health test is `ratio <= ConditionYellow`, so a
    /// mover sitting exactly on the threshold IS damaged. Stock
    /// `[AudioVisual] ConditionYellow=50%`.
    #[test]
    fn gsi_06_04_condition_yellow_test_is_inclusive() {
        const STOCK: i64 = 500; // 50% x1000
        // Exactly on the threshold — penalised.
        assert!(is_at_or_below_condition_yellow(50, 100, STOCK));
        // Below — penalised.
        assert!(is_at_or_below_condition_yellow(49, 100, STOCK));
        // Above — not penalised.
        assert!(!is_at_or_below_condition_yellow(51, 100, STOCK));
        assert!(!is_at_or_below_condition_yellow(100, 100, STOCK));
        // A zero-max entity has no ratio to compare.
        assert!(!is_at_or_below_condition_yellow(0, 0, STOCK));
    }

    /// GSI-06.13 gap 7 — the `0.0 → 0.5` rescue is a substitution on the
    /// *combined* value, applied after the slope multiply. A 0% land row going
    /// downhill must yield exactly 0.5, not `0.5 * 1.2 = 0.6`, which is what a
    /// pre-slope floor produced.
    #[test]
    fn gsi_06_13_zero_land_row_substitution_runs_after_slope() {
        let zero = SimFixed::from_num(0);
        let downhill = SimFixed::lit("1.2");
        assert_eq!(
            combine_speed_stages(zero, downhill, false),
            SimFixed::lit("0.5"),
        );
        assert_eq!(
            combine_speed_stages(zero, SIM_ONE, false),
            SimFixed::lit("0.5")
        );
        // A non-zero row is untouched by the substitution. Compare against the
        // same fixed-point product the implementation performs — `lit("0.6")`
        // differs by 1 ULP because 0.6 is not exactly representable.
        let half = SimFixed::lit("0.5");
        assert_eq!(combine_speed_stages(half, downhill, false), half * downhill);
        assert!(combine_speed_stages(half, downhill, false) > half);
    }

    /// GSI-06.13 gap 8 — the damaged-mover factor is the *last* operation and
    /// there is no combined clamp after it. `[Clear]` (1.0) downhill (1.2) at or
    /// below ConditionYellow is `1.2 * 0.75 = 0.9`; a `[0.3, 1.2]` clamp applied
    /// before the multiply would have produced the same 0.9 here but 0.225 for a
    /// 0.3-floored product, which the engine never yields.
    #[test]
    fn gsi_06_13_damaged_factor_is_last_and_uncapped() {
        assert_eq!(
            combine_speed_stages(SIM_ONE, SimFixed::lit("1.2"), true),
            SimFixed::lit("0.9"),
        );
        // The zero substitution feeds the damaged multiply: 0.5 * 0.75.
        assert_eq!(
            combine_speed_stages(SimFixed::from_num(0), SimFixed::lit("1.2"), true),
            SimFixed::lit("0.375"),
        );
        // Undamaged downhill on a full-speed row exceeds 1.0 — no upper clamp.
        assert!(combine_speed_stages(SIM_ONE, SimFixed::lit("1.2"), false) > SIM_ONE);
    }

    #[test]
    fn default_config_values() {
        let config = TerrainSpeedConfig::default();
        assert_eq!(config.tracked_uphill, SIM_ONE);
        assert_eq!(config.tracked_downhill, SimFixed::lit("1.2"));
        assert_eq!(config.wheeled_uphill, SIM_ONE);
        assert_eq!(config.wheeled_downhill, SimFixed::lit("1.2"));
    }

    #[test]
    fn slope_uphill_no_change_downhill_boost() {
        let config = TerrainSpeedConfig::default();
        // Uphill (next higher) → 1.0; downhill → 1.2; flat → 1.0. Track and Wheel
        // share vanilla values but exercise both selection arms.
        for st in [SpeedType::Track, SpeedType::Wheel] {
            assert_eq!(
                slope_factor_for(st, 0, 1, &config),
                SIM_ONE,
                "uphill {st:?}"
            );
            let down = slope_factor_for(st, 1, 0, &config);
            assert_eq!(down, SimFixed::lit("1.2"), "downhill {st:?}");
            assert_eq!(slope_factor_for(st, 2, 2, &config), SIM_ONE, "flat {st:?}");
        }
    }

    #[test]
    fn slope_selects_tracked_vs_wheeled_pair() {
        // Distinct values per pair to prove the SpeedType arm is honoured.
        let config = TerrainSpeedConfig {
            tracked_uphill: SimFixed::lit("0.5"),
            tracked_downhill: SimFixed::lit("1.5"),
            wheeled_uphill: SimFixed::lit("0.7"),
            wheeled_downhill: SimFixed::lit("1.1"),
        };
        assert_eq!(
            slope_factor_for(SpeedType::Track, 0, 1, &config),
            SimFixed::lit("0.5")
        );
        assert_eq!(
            slope_factor_for(SpeedType::Track, 1, 0, &config),
            SimFixed::lit("1.5")
        );
        // Foot and Wheel both take the wheeled (non-Track) pair.
        assert_eq!(
            slope_factor_for(SpeedType::Wheel, 0, 1, &config),
            SimFixed::lit("0.7")
        );
        assert_eq!(
            slope_factor_for(SpeedType::Foot, 1, 0, &config),
            SimFixed::lit("1.1")
        );
    }
}
