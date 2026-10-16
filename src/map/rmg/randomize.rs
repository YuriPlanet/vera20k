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

/// Fill the derived fields from the map type, exactly in the original's order.
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

/// Randomize the option set the way the dialog's Surprise Me button does.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Records every draw and replays scripted values in order.
    struct ScriptedRng {
        values: Vec<i32>,
        calls: Vec<(i32, i32)>,
    }

    impl ScriptedRng {
        fn new(values: Vec<i32>) -> Self {
            Self {
                values,
                calls: Vec::new(),
            }
        }
    }

    impl RandomRanged for ScriptedRng {
        fn ranged(&mut self, min: i32, max: i32) -> i32 {
            self.calls.push((min, max));
            let index = self.calls.len() - 1;
            *self.values.get(index).unwrap_or(&min)
        }
    }

    #[test]
    fn derive_draws_in_the_original_order_with_the_right_ranges() {
        let mut options = RmgOptions {
            map_type: 4,
            resources: 3,
            ..Default::default()
        };
        let mut rng = ScriptedRng::new(vec![0; 8]);
        derive_from_map_type(&mut options, &RmgSettings::default(), &mut rng);

        // Mountainous (bucket 4) ranges, in draw order.
        assert_eq!(
            rng.calls,
            vec![
                (0, 100),    // water
                (20, 100),   // ruggedness
                (0, 0),      // urban
                (0, 20),     // accessibility
                (0, 50),     // region size
                (0, 100),    // tiberium layout
                (0, 0),      // vegetation (default settings are 0/0)
                (0, 0xFFFF), // seed
            ]
        );
    }

    #[test]
    fn tiberium_is_resources_times_twenty_and_is_not_drawn() {
        let mut options = RmgOptions {
            map_type: 1,
            resources: 3,
            ..Default::default()
        };
        let mut rng = ScriptedRng::new(vec![0; 8]);
        derive_from_map_type(&mut options, &RmgSettings::default(), &mut rng);
        assert_eq!(options.tiberium, 60);
        assert_eq!(rng.calls.len(), 8, "tiberium must not consume a draw");
    }

    #[test]
    fn archipelago_is_water_heavy_and_continent_is_not() {
        assert_eq!((WATER_MIN[0], WATER_MAX[0]), (75, 100));
        assert_eq!((WATER_MIN[1], WATER_MAX[1]), (0, 25));
    }

    #[test]
    fn mountainous_is_impassable_and_never_urban() {
        // These two corroborate the map-type ordering taken from RMGMD.INI.
        assert_eq!((ACCESSIBILITY_MIN[4], ACCESSIBILITY_MAX[4]), (0, 20));
        assert_eq!((URBAN_MIN[4], URBAN_MAX[4]), (0, 0));
    }

    #[test]
    fn inverted_vegetation_bounds_collapse_onto_the_maximum() {
        let settings = RmgSettings {
            vegetation_min: [80; 5],
            vegetation_max: [30; 5],
            ..Default::default()
        };
        let mut options = RmgOptions {
            map_type: 0,
            ..Default::default()
        };
        let mut rng = ScriptedRng::new(vec![0; 8]);
        derive_from_map_type(&mut options, &settings, &mut rng);
        // Vegetation is draw #7 (index 6): min collapsed from 80 down to 30.
        assert_eq!(rng.calls[6], (30, 30));
    }

    #[test]
    fn out_of_range_map_type_clamps_into_the_table() {
        let mut options = RmgOptions {
            map_type: 99,
            ..Default::default()
        };
        let mut rng = ScriptedRng::new(vec![0; 8]);
        derive_from_map_type(&mut options, &RmgSettings::default(), &mut rng);
        assert_eq!(rng.calls[0], (WATER_MIN[4], WATER_MAX[4]));
    }

    #[test]
    fn randomize_only_ever_picks_the_first_two_theaters() {
        for value in 0..=100 {
            let mut options = RmgOptions::default();
            let mut rng = ScriptedRng::new(vec![value, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            randomize(&mut options, &RmgSettings::default(), &mut rng, "");
            assert!(
                options.theater == 0 || options.theater == 1,
                "theater {} out of range for draw {value}",
                options.theater
            );
        }
    }

    #[test]
    fn randomize_theater_flips_above_the_threshold() {
        let theater_for = |first: i32| {
            let mut options = RmgOptions::default();
            let mut rng = ScriptedRng::new(vec![first, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            randomize(&mut options, &RmgSettings::default(), &mut rng, "");
            options.theater
        };
        assert_eq!(theater_for(0x31), 0, "at the threshold stays on theater 0");
        assert_eq!(theater_for(0x32), 1, "just above the threshold flips");
    }

    #[test]
    fn randomize_writes_both_size_axes_from_one_draw() {
        let mut options = RmgOptions::default();
        let mut rng = ScriptedRng::new(vec![0, 1, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        randomize(&mut options, &RmgSettings::default(), &mut rng, "");
        assert_eq!((options.width, options.height), (2, 2));
        assert_eq!(rng.calls[4], (0, 3), "size is a single 0..3 draw");
    }

    #[test]
    fn randomize_draws_the_seed_twice() {
        let mut options = RmgOptions::default();
        let mut rng = ScriptedRng::new(vec![0; 16]);
        randomize(&mut options, &RmgSettings::default(), &mut rng, "");
        let seed_draws = rng
            .calls
            .iter()
            .filter(|call| **call == (0, 0xFFFF))
            .count();
        assert_eq!(
            seed_draws, 2,
            "derived pass and the button each draw a seed"
        );
        // 5 button draws + 8 derived draws + 1 final seed draw.
        assert_eq!(rng.calls.len(), 14);
    }

    #[test]
    fn randomize_draws_time_before_resources() {
        let mut options = RmgOptions::default();
        // theater, map type, time=2, resources=1, size
        let mut rng = ScriptedRng::new(vec![0, 1, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        randomize(&mut options, &RmgSettings::default(), &mut rng, "");
        assert_eq!(options.time, 2);
        assert_eq!(options.resources, 1);
    }

    #[test]
    fn randomize_sets_the_description_and_normalizes() {
        let mut options = RmgOptions::default();
        let mut rng = ScriptedRng::new(vec![0; 16]);
        randomize(
            &mut options,
            &RmgSettings::default(),
            &mut rng,
            "Random Map",
        );
        assert_eq!(options.description, "Random Map");
        assert!(options.tiberium >= 1, "normalize floors tiberium at 1");
    }
}
