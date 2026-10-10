//! Assembles the pipeline's pre-resolved inputs from a real theater + rules and
//! runs the generator end-to-end into a populated `MapFile`.
//!
//! This is the layer between `run_pipeline` (which takes already-resolved
//! inputs) and the app: it resolves `TileIds`, the cliff ranges, the wheel-
//! impassable land table (from rules), and a per-tile `Morphable=` table, then
//! drives the pipeline and projects the grid through `emit::populate`. Theater
//! resolution is snapshotted into `ResolvedTheaterInputs` so the run itself
//! never borrows `TheaterData` — which keeps `generate_map` unit-testable with a
//! hand-built resolution and a synthetic `TileBlocks`.

use crate::map::theater::{TheaterCliffRanges, TheaterData};
use crate::rules::locomotor_type::SpeedType;
use crate::rules::terrain_rules::TerrainRules;

use super::phases::shore::TileBlocks;
use super::phases::tech_buildings::TechType;
use super::pipeline::{self, LAND_TYPES, PipelineInputs};
use super::rng::{RANGE_K_BITS, RmgRng};
use super::scratch::RmgScratch;
use super::tiles::TileIds;
use super::x87::{Gaussian, TruncF64};
use super::{
    GeneratedMap, MapGeometry, RmgOptions, RmgSettings, Stage, emit, executed_stages, grid,
};
use crate::map::retail_trig::TrigTable;

/// Land type → rules section, the RA2 `LandType` enum order (verified against
/// the zone classifier's `LAND_WATER == 2` / `LAND_BEACH == 6`).
const LAND_TYPE_SECTIONS: [Option<&str>; LAND_TYPES] = [
    Some("Clear"),    // 0
    Some("Road"),     // 1
    Some("Water"),    // 2
    Some("Rock"),     // 3
    Some("Wall"),     // 4
    Some("Tiberium"), // 5
    Some("Beach"),    // 6
    Some("Rough"),    // 7
    Some("Ice"),      // 8
    Some("Railroad"), // 9
    Some("Tunnel"),   // 10
    Some("Weeds"),    // 11
    None,             // 12
    None,             // 13
    None,             // 14
    None,             // 15
];

/// Wheel speed at or below this percent classifies the land as wheel-impassable.
const WHEEL_IMPASSABLE_MAX: u8 = 1;

/// Per-land-type "Wheel speed <= 1%" flags for the zone classifier, read from
/// the terrain rules. Stock rules make only Rock wheel-impassable.
pub fn wheel_impassable_from_rules(rules: &TerrainRules) -> [bool; LAND_TYPES] {
    std::array::from_fn(|land| {
        LAND_TYPE_SECTIONS[land]
            .and_then(|name| rules.semantics_by_name(name))
            .and_then(|sem| sem.cost_for_speed_type(SpeedType::Wheel))
            .is_some_and(|wheel| wheel <= WHEEL_IMPASSABLE_MAX)
    })
}

/// The facts the generator needs, snapshotted so the run does not borrow
/// `TheaterData`.
///
/// Mostly theater and rules, plus one thing that is neither: the sine table,
/// which comes out of the retail executable. It rides here because this is
/// already the "everything a run needs, resolved once" bundle, and threading a
/// second parameter through every entry point would say less.
#[derive(Debug, Clone)]
pub struct ResolvedTheaterInputs {
    pub ids: TileIds,
    pub cliff: TheaterCliffRanges,
    pub wheel_impassable: [bool; LAND_TYPES],
    /// Per-tile `Morphable=` flag, indexed by flat tile id.
    pub morphable: Vec<bool>,
    /// The generator's sine table, read from `gamemd.exe`. `None` when the
    /// install could not supply one — rivers are then skipped rather than
    /// carved from a table this build does not recognise.
    pub trig: Option<TrigTable>,
}

impl ResolvedTheaterInputs {
    /// Snapshot a theater + rules into the resolved input set.
    pub fn from_theater(
        theater: &TheaterData,
        rules: &TerrainRules,
        trig: Option<TrigTable>,
    ) -> Self {
        let morphable = (0..theater.lookup.len())
            .map(|tile| theater.lookup.is_morphable(tile as u16))
            .collect();
        Self {
            ids: TileIds::resolve(theater),
            cliff: theater.cliff_ranges,
            wheel_impassable: wheel_impassable_from_rules(rules),
            morphable,
            trig,
        }
    }

    fn is_morphable(&self, tile: i32) -> bool {
        tile >= 0 && (tile as usize) < self.morphable.len() && self.morphable[tile as usize]
    }
}

/// Where a generation observer is being called from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationPoint {
    /// Before any terrain work. The original draws the preview here too, so the
    /// box clears the moment Generate is pressed rather than showing the
    /// previous map until the new one is finished.
    Initial,
    /// The boundary after `Stage` in `STAGE_ORDER`.
    After(Stage),
}

/// The map as it stands at a generation boundary, in the shape the finished map
/// takes so the preview path that colours the final map colours these too.
#[derive(Debug)]
pub struct GenerationSnapshot {
    pub map_file: crate::map::map_file::MapFile,
    pub start_waypoints: Vec<(u8, u16, u16)>,
}

/// A boundary an observer was called at.
///
/// Projecting the grid walks every cell, so it happens only when
/// [`Self::snapshot`] is called — a boundary the observer ignores costs it
/// nothing.
pub struct GenerationPointView<'a> {
    point: GenerationPoint,
    options: &'a RmgOptions,
    geometry: MapGeometry,
    grid: &'a grid::RmgGrid,
    waypoints: &'a [(u8, u16, u16)],
}

impl GenerationPointView<'_> {
    pub fn point(&self) -> GenerationPoint {
        self.point
    }

    /// Project the grid as it stands into a `MapFile`.
    ///
    /// Trees and tech buildings are left out: the pipeline collects them as it
    /// goes and hands them over only at the end, and the preview draws neither
    /// — it reads cells and overlays.
    pub fn snapshot(&self) -> GenerationSnapshot {
        let mut map_file = emit::empty_map_file(
            self.options,
            self.geometry.gen_w as u32,
            self.geometry.gen_h as u32,
        );
        emit::populate(&mut map_file, self.grid, &[], &[], self.waypoints);
        GenerationSnapshot {
            map_file,
            start_waypoints: self.waypoints.to_vec(),
        }
    }
}

/// Run the full generator, returning a populated `MapFile` plus the generated
/// start waypoints and the executed stage list.
///
/// `resolved` is a theater/rules snapshot; `blocks` provides tile-block layouts
/// (from `theater_blocks::TheaterTileBlocks`); `tech_types` is the resolved
/// neutral tech-building list (empty places no tech buildings).
pub fn generate_map(
    options: &RmgOptions,
    settings: &RmgSettings,
    resolved: &ResolvedTheaterInputs,
    blocks: &dyn TileBlocks,
    tech_types: &[TechType],
) -> GeneratedMap {
    generate_map_observed(options, settings, resolved, blocks, tech_types, &mut |_| {})
}

/// Chance that a map is allowed to attempt a river waterfall crossing.
const BRIDGE_ENABLE_CHANCE: f32 = 0.25;

/// The generation's first random draw, taken during map bring-up.
///
/// It decides one thing — whether a river may attempt waterfall terrain — and
/// only the river reads the answer. The **draw itself** is what matters to every
/// other map type: it happens before any terrain work, unconditionally, whatever
/// the map type or the water amount, so a generator that skips it starts every
/// map one value out of step and nothing downstream can ever line up again.
fn draw_bridge_enable(rng: &mut RmgRng) -> bool {
    let scale = TruncF64::from_f64(f64::from_bits(RANGE_K_BITS));
    let draw = TruncF64::from_f64(f64::from(rng.next_u32()));
    draw.mul(scale)
        .lt(TruncF64::from_f64(f64::from(BRIDGE_ENABLE_CHANCE)))
}

/// As [`generate_map`], calling `observe` at every generation boundary.
///
/// The observer sees the run; it cannot alter it. Everything that decides the
/// output — the RNG, the scratch, the grid — is either not handed over or handed
/// over shared, so an observed run generates exactly what an unobserved one does.
pub fn generate_map_observed(
    options: &RmgOptions,
    settings: &RmgSettings,
    resolved: &ResolvedTheaterInputs,
    blocks: &dyn TileBlocks,
    tech_types: &[TechType],
    observe: &mut dyn FnMut(GenerationPointView<'_>),
) -> GeneratedMap {
    let mut options = options.clone();
    options.normalize();

    let geometry = MapGeometry::from_options(&options);
    let mut rng = RmgRng::new(options.seed_u16());
    let bridge_enabled = draw_bridge_enable(&mut rng);
    let mut scratch = RmgScratch::new(geometry.stride, geometry.diamond_min, geometry.diamond_max);
    let mut grid = grid::RmgGrid::new(geometry.stride, geometry.diamond_min, geometry.diamond_max);
    let mut gauss = Gaussian::default();

    let morphable = |tile: i32| resolved.is_morphable(tile);
    let inputs = PipelineInputs {
        ids: &resolved.ids,
        blocks,
        cliff: &resolved.cliff,
        morphable: &morphable,
        wheel_impassable: resolved.wheel_impassable,
        tech_types,
        trig: resolved.trig.as_ref(),
        map_type: options.map_type,
        accessibility: options.accessibility,
        theater: options.theater,
        num_players: options.num_players,
        bridge_enabled,
        water_percent: options.water_amount,
        resources: options.resources,
        tib_option: options.tiberium,
        min_tib: settings.min_tiberium,
        max_tib: settings.max_tiberium,
        tiberium_layout: options.tiberium_layout,
        ruggedness: options.ruggedness,
        vegetation: options.vegetation,
        // MapSeed+0x64 tree-count width term — the dialog Width option
        // (0..3); a final binary confirmation is still open.
        width: options.width,
        max_trees: settings.max_trees,
    };

    observe(GenerationPointView {
        point: GenerationPoint::Initial,
        options: &options,
        geometry,
        grid: &grid,
        waypoints: &[],
    });

    let mut on_stage = |stage: Stage, grid: &grid::RmgGrid, waypoints: &[(u8, u16, u16)]| {
        observe(GenerationPointView {
            point: GenerationPoint::After(stage),
            options: &options,
            geometry,
            grid,
            waypoints,
        });
    };
    let output = pipeline::run_pipeline(
        &mut grid,
        &mut scratch,
        &mut rng,
        &mut gauss,
        &geometry,
        &inputs,
        &mut on_stage,
    );

    let mut map_file = emit::empty_map_file(&options, geometry.gen_w as u32, geometry.gen_h as u32);
    emit::populate(
        &mut map_file,
        &grid,
        &output.terrain,
        &output.structures,
        &output.waypoints,
    );

    GeneratedMap {
        map_file,
        mapgen_continuation: Some(rng.into_continuation()),
        construction_trace: output.construction_trace,
        start_waypoints: output.waypoints,
        stages_run: executed_stages(&options),
        unfilled_start_slots: output.unfilled_start_slots,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::rmg::RmgConstructionOutcome;
    use crate::map::rmg::STAGE_ORDER;
    use crate::map::rmg::phases::shore::{SubTile, TileBlock};
    use crate::map::rmg::tiles::SpecialTerrain;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::terrain_rules::LandType;

    #[test]
    fn gsi_04_04_rmg_land_sections_follow_canonical_order() {
        assert_eq!(
            LAND_TYPE_SECTIONS,
            [
                Some("Clear"),
                Some("Road"),
                Some("Water"),
                Some("Rock"),
                Some("Wall"),
                Some("Tiberium"),
                Some("Beach"),
                Some("Rough"),
                Some("Ice"),
                Some("Railroad"),
                Some("Tunnel"),
                Some("Weeds"),
                None,
                None,
                None,
                None,
            ],
        );
    }

    struct OneByOne(TileBlock);
    impl TileBlocks for OneByOne {
        fn block(&self, _tile: i32) -> Option<&TileBlock> {
            Some(&self.0)
        }
    }

    fn one_by_one() -> OneByOne {
        OneByOne(TileBlock {
            width: 1,
            height: 1,
            subtiles: vec![Some(SubTile::default())],
        })
    }

    fn ids() -> TileIds {
        TileIds {
            clear: 0,
            ramp_base: 600,
            ramp_smooth: 620,
            rough: 700,
            sand: 800,
            green: 100,
            rough_lat: 710,
            sand_lat: 810,
            green_lat: 110,
            pave_lat: -1,
            pave: -1,
            water_base: 500,
            shore: 400,
            water_bridge: -1,
            misc_pave: -1,
            paved_roads: -1,
            paved_road_ends: -1,
            medians: -1,
            special: SpecialTerrain::default(),
        }
    }

    fn resolved() -> ResolvedTheaterInputs {
        ResolvedTheaterInputs {
            trig: None,
            ids: ids(),
            cliff: TheaterCliffRanges::default(),
            wheel_impassable: {
                let mut w = [false; LAND_TYPES];
                w[3] = true; // Rock, as in stock rules
                w
            },
            morphable: Vec::new(),
        }
    }

    #[test]
    fn gsi_04_04_wheel_table_uses_canonical_land_indices() {
        let rules = TerrainRules::from_ini(&IniFile::from_str(
            "[Clear]\nWheel=100%\n\
             [Road]\nWheel=100%\n\
             [Rough]\nWheel=60%\n\
             [Rock]\nWheel=0%\n\
             [Water]\nWheel=0%\n\
             [Ice]\nWheel=100%\n",
        ));
        let wheel = wheel_impassable_from_rules(&rules);
        assert!(wheel[LandType::Water.as_index() as usize]);
        assert!(wheel[LandType::Rock.as_index() as usize]);
        assert!(!wheel[LandType::Clear.as_index() as usize]);
        assert!(!wheel[LandType::Road.as_index() as usize]);
        assert!(!wheel[LandType::Rough.as_index() as usize]);
        assert!(!wheel[LandType::Ice.as_index() as usize]);
    }

    #[test]
    fn missing_wheel_key_is_passable() {
        let rules = TerrainRules::from_ini(&IniFile::from_str(""));
        let wheel = wheel_impassable_from_rules(&rules);
        // No section defines Wheel → nothing is wheel-impassable.
        assert!(wheel.iter().all(|&blocked| !blocked));
    }

    fn options(map_type: i32, theater: i32) -> RmgOptions {
        RmgOptions {
            num_players: 4,
            map_type,
            theater,
            water_amount: 30,
            tiberium: 50,
            vegetation: 50,
            ruggedness: 20,
            ..Default::default()
        }
    }

    #[test]
    fn generate_map_produces_a_populated_map() {
        let resolved = resolved();
        let blocks = one_by_one();
        let settings = RmgSettings::default();
        let generated = generate_map(&options(1, 0), &settings, &resolved, &blocks, &[]);
        // Every in-band cell is projected, and the terrain is shaped (not all
        // no-tile). The header carries the generated dimensions.
        assert!(!generated.map_file.cells.is_empty(), "cells were emitted");
        let real = generated
            .map_file
            .cells
            .iter()
            .filter(|c| c.tile_index != -1)
            .count();
        assert!(real > 0, "some cells carry real tiles");
        assert_eq!(generated.map_file.header.theater, "TEMPERATE");
        // Rocks run on temperate.
        assert!(generated.stages_run.contains(&Stage::Rocks));
    }

    #[test]
    fn generate_map_is_deterministic() {
        let resolved = resolved();
        let blocks = one_by_one();
        let settings = RmgSettings::default();
        let options = options(1, 0);
        let snapshot = || {
            let mut g = generate_map(&options, &settings, &resolved, &blocks, &[]);
            let tiles: Vec<i32> = g.map_file.cells.iter().map(|c| c.tile_index).collect();
            let overlays: Vec<(u16, u16, u8)> = g
                .map_file
                .overlays
                .iter()
                .map(|o| (o.rx, o.ry, o.overlay_id))
                .collect();
            let mapgen = g.take_mapgen_continuation().into_native_parts();
            (tiles, overlays, g.start_waypoints, mapgen)
        };
        let first = snapshot();
        let second = snapshot();
        assert_eq!(first, second);
        assert_ne!(
            first.3,
            RmgRng::new(options.seed_u16())
                .into_continuation()
                .into_native_parts(),
            "generation must carry an advanced MapGen cursor into the map receipt"
        );
    }

    #[test]
    fn generated_construction_trace_binds_exact_final_entities() {
        let resolved = resolved();
        let blocks = one_by_one();
        let settings = RmgSettings::default();
        let types = [TechType {
            name: "CATECH".to_string(),
            footprint: vec![(0, 0)],
        }];
        let mut chosen = None;
        for seed in 1..=8 {
            let mut options = options(1, 0);
            options.seed = seed;
            let generated = generate_map(&options, &settings, &resolved, &blocks, &types);
            if !generated.construction_trace.events.is_empty() {
                chosen = Some(generated);
                break;
            }
        }
        let generated = chosen.expect("bounded fixture reaches a constructed neutral type");
        for (ordinal, event) in generated.construction_trace.events.iter().enumerate() {
            assert_eq!(event.ordinal, ordinal);
            match &event.outcome {
                RmgConstructionOutcome::Discarded => {}
                RmgConstructionOutcome::Emitted { entity_index, cell } => {
                    let entity = &generated.map_file.entities[*entity_index];
                    assert_eq!(entity.type_id, event.techno_type);
                    assert_eq!((entity.cell_x, entity.cell_y), *cell);
                }
            }
        }
    }

    /// The comparable content of a generated map: what the emitter projected
    /// plus the start positions.
    fn projection(
        generated: &GeneratedMap,
    ) -> (Vec<i32>, Vec<(u16, u16, u8, u8)>, Vec<(u8, u16, u16)>) {
        let tiles = generated
            .map_file
            .cells
            .iter()
            .map(|cell| cell.tile_index)
            .collect();
        let overlays = generated
            .map_file
            .overlays
            .iter()
            .map(|o| (o.rx, o.ry, o.overlay_id, o.frame))
            .collect();
        (tiles, overlays, generated.start_waypoints.clone())
    }

    fn observe_run(
        options: &RmgOptions,
    ) -> (GeneratedMap, Vec<GenerationPoint>, Vec<GenerationSnapshot>) {
        let resolved = resolved();
        let blocks = one_by_one();
        let settings = RmgSettings::default();
        let mut points = Vec::new();
        let mut snapshots = Vec::new();
        let generated =
            generate_map_observed(options, &settings, &resolved, &blocks, &[], &mut |view| {
                points.push(view.point());
                snapshots.push(view.snapshot());
            });
        (generated, points, snapshots)
    }

    #[test]
    fn observing_a_run_does_not_change_what_it_generates() {
        let resolved = resolved();
        let blocks = one_by_one();
        let settings = RmgSettings::default();
        let plain = generate_map(&options(1, 0), &settings, &resolved, &blocks, &[]);
        // The observer does real work at every boundary — it projects the whole
        // grid — so this covers the projection as well as the callback.
        let (observed, _, _) = observe_run(&options(1, 0));

        assert_eq!(plain.stages_run, observed.stages_run);
        assert_eq!(projection(&plain), projection(&observed));
    }

    #[test]
    fn every_boundary_is_reported_once_in_pipeline_order() {
        // Emit is the caller's step, so the pipeline never reaches a boundary
        // for it; everything before it reports, including the stages this
        // configuration skips.
        let expected: Vec<GenerationPoint> = std::iter::once(GenerationPoint::Initial)
            .chain(
                STAGE_ORDER
                    .iter()
                    .filter(|stage| **stage != Stage::Emit)
                    .map(|stage| GenerationPoint::After(*stage)),
            )
            .collect();
        let (_, points, _) = observe_run(&options(1, 0));
        assert_eq!(points, expected);
    }

    #[test]
    fn a_skipped_stage_still_reports_its_boundary() {
        // Snow runs no rock pass, but the boundary after it is what the
        // original's last in-progress preview sits at, so it must still fire.
        let (generated, points, _) = observe_run(&options(1, 1));
        assert!(!generated.stages_run.contains(&Stage::Rocks));
        assert!(points.contains(&GenerationPoint::After(Stage::Rocks)));
    }

    #[test]
    fn snapshots_share_the_final_maps_dimensions() {
        // The preview's cell-admission test reads the header, so a snapshot
        // whose header differed would rasterise to a different pixel size and
        // the preview box would jump about while generating.
        let (generated, _, snapshots) = observe_run(&options(1, 0));
        let final_header = &generated.map_file.header;
        for snapshot in &snapshots {
            let header = &snapshot.map_file.header;
            assert_eq!(
                (
                    header.width,
                    header.height,
                    header.local_left,
                    header.local_top,
                    header.local_width,
                    header.local_height
                ),
                (
                    final_header.width,
                    final_header.height,
                    final_header.local_left,
                    final_header.local_top,
                    final_header.local_width,
                    final_header.local_height
                ),
            );
            assert_eq!(
                snapshot.map_file.cells.len(),
                generated.map_file.cells.len()
            );
        }
    }

    #[test]
    fn start_positions_appear_from_the_starts_boundary_onwards() {
        let (generated, points, snapshots) = observe_run(&options(1, 0));
        assert!(
            !generated.start_waypoints.is_empty(),
            "the run placed starts"
        );
        let first_with_starts = points
            .iter()
            .zip(&snapshots)
            .position(|(_, snapshot)| !snapshot.start_waypoints.is_empty())
            .expect("some snapshot carries the starts");
        assert_eq!(
            points[first_with_starts],
            GenerationPoint::After(Stage::Starts)
        );
        // And once placed they stay, so the markers do not blink out again.
        assert!(
            snapshots[first_with_starts..]
                .iter()
                .all(|snapshot| snapshot.start_waypoints == generated.start_waypoints)
        );
    }

    #[test]
    fn non_temperate_theater_skips_rocks_stage() {
        let resolved = resolved();
        let blocks = one_by_one();
        let settings = RmgSettings::default();
        let generated = generate_map(&options(1, 1), &settings, &resolved, &blocks, &[]);
        assert_eq!(generated.map_file.header.theater, "SNOW");
        assert!(!generated.stages_run.contains(&Stage::Rocks));
        assert!(generated.stages_run.contains(&Stage::Trees));
    }

    // ---- The generation matrix -------------------------------------------
    //
    // Everything below is a REGRESSION RATCHET, never parity evidence: the
    // inputs are synthetic and both sides of every comparison come from this
    // build. It can expose a panic, a gate regression, or nondeterminism. It
    // cannot show the generator agrees with the original.
    //
    // The synthetic fixture leaves whole systems inert, so the matrix must not
    // be read as covering them: `morphable` is always false, so the hills
    // corner engine never morphs; `TheaterCliffRanges::default()` leaves every
    // range `None`, so cliff drops never fire; and six `TileIds` fields are
    // -1, so the pave and road rejections in the start gate never trigger.

    /// Seeds the matrix crosses. 0 is the normalizer's floor — an unset seed
    /// of -1 clamps to it — and 0xFFFF is the randomizer's ceiling; the rest
    /// are a fixed spread. A single seed would hide exactly what this matrix
    /// exists to find, because the start selector starves seed-dependently.
    const MATRIX_SEEDS: [i32; 5] = [0, 1, 4242, 30011, 0xFFFF];

    /// What the dialog itself can emit: it offers two theaters, and its
    /// map-type list starts at 1, so archipelago is not reachable from it.
    const DIALOG_MAP_TYPES: [i32; 4] = [1, 2, 3, 4];
    const DIALOG_THEATERS: [i32; 2] = [0, 1];
    /// Reachable only through a hand-authored `.SED`: archipelago, and the
    /// theaters the dialog never offers. Real input surfaces, but a failure
    /// here ranks below one the player can reach by clicking.
    const SED_ONLY_MAP_TYPE: i32 = 0;
    const SED_ONLY_THEATERS: [i32; 3] = [2, 3, 4];

    /// Rock overlay id range — the observable product of the temperate branch.
    const ROCK_OVERLAYS: std::ops::RangeInclusive<u8> = 168..=177;

    fn matrix_options(map_type: i32, theater: i32, seed: i32) -> RmgOptions {
        RmgOptions {
            seed,
            ..options(map_type, theater)
        }
    }

    /// The connector pass is live, and its randomness reaches the map.
    ///
    /// Accessibility only ever enters through the connector pass, and only on
    /// the island map types. So if raising it changes the generated map, the
    /// pass is running, its rolls are being taken, and the results reach the
    /// terrain — which no unit test of the pieces can show on its own.
    ///
    /// The ordinary map types must be untouched by it, since they never reach
    /// the pass at all. That half is what makes the first half meaningful:
    /// together they say the difference comes from the connector pass and not
    /// from accessibility leaking into something else.
    #[test]
    fn accessibility_moves_island_maps_and_leaves_the_others_alone() {
        let cells = |m: &GeneratedMap| {
            m.map_file
                .cells
                .iter()
                .map(|c| c.tile_index)
                .collect::<Vec<_>>()
        };
        let differs = |map_type, seed| {
            let mut low = matrix_options(map_type, 0, seed);
            low.accessibility = 0;
            let mut high = matrix_options(map_type, 0, seed);
            high.accessibility = 100;
            cells(&run_cell(&low)) != cells(&run_cell(&high))
        };

        for map_type in [3, 4] {
            assert!(
                MATRIX_SEEDS.into_iter().any(|seed| differs(map_type, seed)),
                "map type {map_type}: the bounded seed matrix must witness accessibility"
            );
        }
        for map_type in [0, 2] {
            for seed in MATRIX_SEEDS {
                assert!(
                    !differs(map_type, seed),
                    "map type {map_type} seed {seed}: accessibility must not leak outside island maps"
                );
            }
        }
    }

    fn run_cell(options: &RmgOptions) -> GeneratedMap {
        generate_map(
            options,
            &RmgSettings::default(),
            &resolved(),
            &one_by_one(),
            &[],
        )
    }

    fn rock_overlay_count(generated: &GeneratedMap) -> usize {
        generated
            .map_file
            .overlays
            .iter()
            .filter(|o| ROCK_OVERLAYS.contains(&o.overlay_id))
            .count()
    }

    /// Generate one cell, assert it produced a real map, and return how many
    /// rock overlays it painted.
    ///
    /// The rock gate is checked on its observable product, never on
    /// `stages_run`: that field is assigned straight from
    /// `executed_stages(&options)`, so comparing the two would compare one pure
    /// function against itself and could not fail.
    ///
    /// Only the OFF side is a per-cell assertion. How many rocks a temperate
    /// map paints is probabilistic, and zero is a legitimate outcome — the
    /// first run of this matrix found `map_type=1, theater=0, seed=0` painting
    /// none while `map_type=0` on the same theater and seed painted some. The
    /// on side is therefore asserted in aggregate by the callers: if the
    /// temperate branch were dead, no cell in the whole tier would paint any.
    #[must_use]
    fn assert_cell_generates(map_type: i32, theater: i32, seed: i32) -> usize {
        let generated = run_cell(&matrix_options(map_type, theater, seed));
        let cell = format!("map_type={map_type} theater={theater} seed={seed}");

        assert!(
            !generated.map_file.cells.is_empty(),
            "{cell}: emitted no cells"
        );
        assert!(
            generated.map_file.cells.iter().any(|c| c.tile_index != -1),
            "{cell}: every emitted cell is no-tile"
        );

        let rocks = rock_overlay_count(&generated);
        if theater != 0 {
            assert_eq!(rocks, 0, "{cell}: only the temperate branch paints rocks");
        }
        rocks
    }

    /// Every configuration the dialog can emit, across the seed spread.
    #[test]
    fn dialog_reachable_matrix_generates() {
        let mut temperate_rocks = 0usize;
        for map_type in DIALOG_MAP_TYPES {
            for theater in DIALOG_THEATERS {
                for seed in MATRIX_SEEDS {
                    temperate_rocks += assert_cell_generates(map_type, theater, seed);
                }
            }
        }
        // The on side of the rock gate, asserted over the tier rather than per
        // cell: a dead temperate branch would paint nothing anywhere.
        assert!(
            temperate_rocks > 0,
            "no dialog-reachable temperate configuration painted any rock"
        );
    }

    /// The `.SED`-only tier, kept separate so a failure here is not confused
    /// with one on a surface the player can reach from the dialog.
    #[test]
    fn sed_only_matrix_generates() {
        let mut temperate_rocks = 0usize;
        for theater in DIALOG_THEATERS {
            for seed in MATRIX_SEEDS {
                temperate_rocks += assert_cell_generates(SED_ONLY_MAP_TYPE, theater, seed);
            }
        }
        for map_type in DIALOG_MAP_TYPES.iter().copied().chain([SED_ONLY_MAP_TYPE]) {
            for theater in SED_ONLY_THEATERS {
                for seed in MATRIX_SEEDS {
                    let rocks = assert_cell_generates(map_type, theater, seed);
                    debug_assert_eq!(rocks, 0, "non-temperate tier paints no rocks");
                }
            }
        }
        assert!(
            temperate_rocks > 0,
            "archipelago painted no rock on any temperate seed"
        );
    }

    /// Determinism across the matrix. A ratchet: both sides are produced by
    /// this build, so it detects nondeterminism and never drift.
    #[test]
    fn matrix_generation_is_deterministic() {
        for map_type in 0..=4 {
            for theater in [0, 1] {
                for seed in [0, 4242, 0xFFFF] {
                    let options = matrix_options(map_type, theater, seed);
                    assert_eq!(
                        projection(&run_cell(&options)),
                        projection(&run_cell(&options)),
                        "map_type={map_type} theater={theater} seed={seed}"
                    );
                }
            }
        }
    }

    fn snapshot_projection(
        snapshot: &GenerationSnapshot,
    ) -> (Vec<i32>, Vec<(u16, u16, u8, u8)>, Vec<(u8, u16, u16)>) {
        let tiles = snapshot
            .map_file
            .cells
            .iter()
            .map(|cell| cell.tile_index)
            .collect();
        let overlays = snapshot
            .map_file
            .overlays
            .iter()
            .map(|o| (o.rx, o.ry, o.overlay_id, o.frame))
            .collect();
        (tiles, overlays, snapshot.start_waypoints.clone())
    }

    /// The inverse of the no-op guard this replaces: `IslandPasses` must now
    /// leave a mark on map types 3 and 4, and must still leave none on the
    /// ordinary types (which do not reach the stage at all).
    ///
    /// The connector/low-deck construction now runs after the rebuilt region
    /// list; this test isolates the earlier destroy-and-rebuild terrain effect.
    #[test]
    fn island_passes_reshape_only_the_island_map_types() {
        let changed = |map_type, seed| {
            let (_, points, snapshots) = observe_run(&matrix_options(map_type, 0, seed));
            let index_of = |point: GenerationPoint| {
                points
                    .iter()
                    .position(|candidate| *candidate == point)
                    .unwrap_or_else(|| panic!("{point:?} was never reported"))
            };
            let before =
                snapshot_projection(&snapshots[index_of(GenerationPoint::After(Stage::Regions))]);
            let after = snapshot_projection(
                &snapshots[index_of(GenerationPoint::After(Stage::IslandPasses))],
            );
            before != after
        };

        for map_type in [3, 4] {
            assert!(
                MATRIX_SEEDS.into_iter().any(|seed| changed(map_type, seed)),
                "map type {map_type}: the bounded seed matrix must witness IslandPasses"
            );
        }
        for map_type in [0, 2] {
            for seed in MATRIX_SEEDS {
                assert!(
                    !changed(map_type, seed),
                    "map type {map_type} seed {seed}: IslandPasses must remain island-only"
                );
            }
        }
    }
}

#[cfg(test)]
mod bridge_enable_coin_tests {
    use super::*;

    /// The coin is the generation's first draw, and it is taken before any
    /// terrain work regardless of map type or water amount.
    ///
    /// Nothing in the port reads the result yet — only the river will — so the
    /// value is not what this pins. What it pins is the **cursor**: the draw
    /// happens, exactly once, so every stage after it reads the stream from the
    /// position the original reads it from. A generator that skips it is one
    /// value out of step on every map it will ever make, and no amount of
    /// correct terrain logic downstream can recover that.
    #[test]
    fn the_coin_consumes_exactly_one_draw() {
        for seed in [0u16, 1, 4242, 30011, 0xFFFF] {
            let mut coined = RmgRng::new(seed);
            let _ = draw_bridge_enable(&mut coined);

            let mut manual = RmgRng::new(seed);
            manual.next_u32();

            for index in 0..8 {
                assert_eq!(
                    coined.next_u32(),
                    manual.next_u32(),
                    "seed {seed}: the coin did not leave the cursor exactly one \
                     draw in, diverging at {index}"
                );
            }
        }
    }

    /// Both outcomes must be reachable, or the comparison is against a constant
    /// and the threshold could be anything.
    #[test]
    fn the_coin_lands_both_ways_at_roughly_one_in_four() {
        let mut enabled = 0usize;
        const TRIALS: usize = 400;
        for seed in 0..TRIALS {
            let mut rng = RmgRng::new(seed as u16);
            if draw_bridge_enable(&mut rng) {
                enabled += 1;
            }
        }
        assert!(
            enabled > 0 && enabled < TRIALS,
            "the coin never varies: {enabled}/{TRIALS}"
        );
        // Generous band — this is a sanity check on the threshold, not a
        // distribution test on the generator.
        assert!(
            (TRIALS / 10..TRIALS / 2).contains(&enabled),
            "{enabled}/{TRIALS} enabled — nowhere near the one-in-four the \
             threshold should give"
        );
    }
}

#[cfg(test)]
mod send_check {
    /// The worker that generation is about to move onto needs to own its
    /// inputs, so every one of them has to cross a thread boundary.
    #[test]
    fn generator_inputs_and_output_are_send() {
        const fn assert_send<T: Send>() {}
        assert_send::<crate::map::rmg::RmgOptions>();
        assert_send::<crate::map::rmg::RmgSettings>();
        assert_send::<super::ResolvedTheaterInputs>();
        assert_send::<crate::map::rmg::theater_blocks::TheaterTileBlocks>();
        assert_send::<crate::map::rmg::GeneratedMap>();
        // The worker publishes these as it goes.
        assert_send::<super::GenerationSnapshot>();
        assert_send::<crate::map::rmg::preview::PreviewPalette>();
    }
}
