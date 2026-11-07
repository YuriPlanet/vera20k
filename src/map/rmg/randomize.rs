//! The dialog-time randomizer: the `Surprise Me` option draws plus the
//! per-map-type derived-field ranges.
//!
//! Separate from the generator's seeded RNG — this runs while the player is
//! still editing options and only decides which configuration appears, never
//! the terrain itself (that follows deterministically from the chosen seed).

use super::options::RmgOptions;
use super::settings::RmgSettings;

/// Inclusive uniform draw, matching the original's range helper on both ends.
pub trait RandomRanged {
    /// Returns a value in `[min, max]` inclusive.
    fn ranged(&mut self, min: i32, max: i32) -> i32;
}

// The process Main binding lives with the ui adapter (random_map_setup, F05): map
// stays sim-independent and callers supply any RandomRanged source.
/// Map-type buckets: archipelago, continent, team continent, inland, mountainous.
const MAP_TYPES: usize = 5;

// Per-map-type derived ranges. Vegetation is absent from this list: its bounds
// are INI-driven and come from `RMGMD.INI` via `RmgSettings`. The urban and
// accessibility minimums are zero-initialised storage that nothing ever writes,
// so they are constant zero rather than a table.
const WATER_MIN: [i32; MAP_TYPES] = [75, 0, 50, 0, 0];
const WATER_MAX: [i32; MAP_TYPES] = [100, 25, 100, 100, 100];
const RUGGEDNESS_MIN: [i32; MAP_TYPES] = [20, 20, 20, 20, 20];
const RUGGEDNESS_MAX: [i32; MAP_TYPES] = [100, 100, 100, 100, 100];
const URBAN_MIN: [i32; MAP_TYPES] = [0, 0, 0, 0, 0];
const URBAN_MAX: [i32; MAP_TYPES] = [50, 100, 100, 100, 0];
const ACCESSIBILITY_MIN: [i32; MAP_TYPES] = [0, 0, 0, 0, 0];
const ACCESSIBILITY_MAX: [i32; MAP_TYPES] = [100, 100, 100, 100, 20];
const REGION_SIZE_MIN: [i32; MAP_TYPES] = [50, 0, 35, 0, 0];
const REGION_SIZE_MAX: [i32; MAP_TYPES] = [100, 100, 100, 100, 50];

/// The resource option is scaled by this to produce the tiberium amount.
const TIBERIUM_PER_RESOURCE_STEP: i32 = 0x14;

/// Fill the derived fields from the map type, exactly in the original's order
/// (`MapSeedClass::RandomizeDerivedFields @ 0x00597260`).
///
/// Consumes eight RNG draws — water, ruggedness, urban, accessibility, region
/// size, tiberium layout, vegetation, seed — and the order is load-bearing:
/// reordering desynchronizes every later value in the stream. Tiberium is
/// computed from the resource option and consumes no draw.
pub fn derive_from_map_type(
    options: &mut RmgOptions,
    settings: &RmgSettings,
    rng: &mut impl RandomRanged,
) {
    let bucket = options.map_type.clamp(0, MAP_TYPES as i32 - 1) as usize;

    options.water_amount = rng.ranged(WATER_MIN[bucket], WATER_MAX[bucket]);
    options.ruggedness = rng.ranged(RUGGEDNESS_MIN[bucket], RUGGEDNESS_MAX[bucket]);
    options.urban_presence = rng.ranged(URBAN_MIN[bucket], URBAN_MAX[bucket]);
    options.accessibility = rng.ranged(ACCESSIBILITY_MIN[bucket], ACCESSIBILITY_MAX[bucket]);
    options.region_size = rng.ranged(REGION_SIZE_MIN[bucket], REGION_SIZE_MAX[bucket]);

    options.tiberium = options.resources * TIBERIUM_PER_RESOURCE_STEP;
    options.tiberium_layout = rng.ranged(0, 100);

    // Vegetation bounds are individually clamped, and an inverted pair collapses
    // the minimum onto the maximum rather than erroring.
    let mut min = settings.vegetation_min[bucket].clamp(0, 100);
    let max = settings.vegetation_max[bucket].clamp(0, 100);
    if max < min {
        min = max;
    }
    options.vegetation = rng.ranged(min, max);

    options.seed = rng.ranged(0, 0xFFFF);
}

/// Theater draw threshold: values above this select the second theater, so the
/// randomizer only ever produces the first two theaters.
const THEATER_THRESHOLD: i32 = 0x31;

/// Randomize the option set the way the dialog's Surprise Me button does: the
/// draws inline in `RandomMapSetupDialog::Proc` (0x00596786..0x00596841), the
/// same as the unreferenced `MapSeedClass::RandomizeAllOptions @ 0x00597380`.
///
/// Draw order: theater, map type, time, resources, size, then the derived
/// fields, then a second seed draw that supersedes the one the derived pass
/// wrote. Both seed draws consume the stream.
pub fn randomize(
    options: &mut RmgOptions,
    settings: &RmgSettings,
    rng: &mut impl RandomRanged,
    description: &str,
) {
    options.theater = i32::from(rng.ranged(0, 100) > THEATER_THRESHOLD);
    options.map_type = rng.ranged(1, 4);
    options.time = rng.ranged(0, 3);
    options.resources = rng.ranged(0, 3);
    // One draw drives both size axes.
    let size = rng.ranged(0, 3);
    options.width = size;
    options.height = size;

    derive_from_map_type(options, settings, rng);

    options.description = description.into();
    options.seed = rng.ranged(0, 0xFFFF);

    options.normalize();
}
