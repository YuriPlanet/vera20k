//! Assembles the pipeline's pre-resolved inputs from a real theater + rules and
//! runs the generator end-to-end into a populated `MapFile`.
//!
//! This is the layer between `run_pipeline` (which takes already-resolved
//! inputs) and the app: it resolves `TileIds` (with the theater's cliff
//! ranges), the wheel-impassable land table (from rules), and a per-tile
//! `Morphable=` table, then drives the pipeline and projects the grid through
//! `emit::populate`. Theater resolution is snapshotted into
//! `ResolvedTheaterInputs` so the run itself never borrows `TheaterData` — which
//! keeps `generate_map` unit-testable with a hand-built resolution and a
//! synthetic `TileBlocks`.

use crate::map::theater::TheaterData;
use crate::rules::locomotor_type::SpeedType;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::terrain_rules::TerrainRules;

#[cfg(any(test, feature = "test-support"))]
use super::executed_stages;
use super::phases::shore::TileBlocks;
use super::phases::tech_buildings::TechType;
use super::phases::zones::OverlayZoneFacts;
use super::pipeline::{self, LAND_TYPES, PipelineInputs};
use super::rng::{RANGE_K_BITS, RmgRng};
use super::scratch::RmgScratch;
use super::tiles::TileIds;
use super::x87::{Gaussian, TruncF64};
use super::{GeneratedMap, MapGeometry, RmgOptions, RmgSettings, Stage, emit, grid};
use crate::util::retail_trig::TrigTable;

/// Per-land-type "Wheel speed <= 1%" flags for the zone classifier, read from
/// the terrain rules' rows in `LandType` order (the classifier's
/// `LAND_WATER == 2` / `LAND_BEACH == 6`); indices past the twelve land types
/// stay false.
pub fn wheel_impassable_from_rules(rules: &TerrainRules) -> [bool; LAND_TYPES] {
    std::array::from_fn(|land| {
        u8::try_from(land)
            .ok()
            .and_then(|land| rules.semantics_for_land_type(land))
            .and_then(|row| row.cost_for_speed_type(SpeedType::Wheel))
            .is_some_and(|wheel| {
                crate::rules::terrain_rules::compare_speed_to_one_percent(wheel)
                    != crate::util::native_x87::MaskedX87Ordering::Greater
            })
    })
}

/// The facts the generator needs, snapshotted so the run does not borrow
/// `TheaterData`: theater and rules, plus the river's sine table.
#[derive(Debug, Clone)]
pub struct ResolvedTheaterInputs {
    /// Resolved tile ids, including the theater's cliff/waterfall ranges.
    pub ids: TileIds,
    pub wheel_impassable: [bool; LAND_TYPES],
    /// Low byte of `[General] CliffBackImpassability=`.
    pub cliff_back_impassability: u8,
    /// What the start-zone classifier reads from each overlay type, by id.
    pub overlay_zones: Vec<OverlayZoneFacts>,
    /// Per-tile `Morphable=` flag, indexed by flat tile id.
    pub morphable: Vec<bool>,
    /// The generator's sine table: always the retail table in production.
    /// Unit tests of other phases pass `None`, which skips river carving.
    pub trig: Option<&'static TrigTable>,
}

impl ResolvedTheaterInputs {
    /// Snapshot a theater + rules into the resolved input set.
    pub fn from_theater(
        theater: &TheaterData,
        rules: &TerrainRules,
        overlays: &OverlayTypeRegistry,
        cliff_back_impassability: u8,
    ) -> Self {
        let morphable = (0..theater.lookup.len())
            .map(|tile| theater.lookup.is_morphable(tile as u16))
            .collect();
        Self {
            ids: TileIds::resolve(theater),
            wheel_impassable: wheel_impassable_from_rules(rules),
            cliff_back_impassability,
            overlay_zones: OverlayZoneFacts::from_registry(overlays),
            morphable,
            trig: Some(TrigTable::embedded()),
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
    terrain: &'a [(String, i16, i16)],
}

impl GenerationPointView<'_> {
    pub fn point(&self) -> GenerationPoint {
        self.point
    }

    /// Project the grid as it stands into a `MapFile`.
    ///
    /// Terrain placements borrow the pipeline's chronological accumulator,
    /// so an early snapshot contains only constructors already reached.
    /// Projection uses the same emitter as the final generated map.
    pub fn snapshot(&self) -> GenerationSnapshot {
        let mut map_file = emit::empty_map_file(
            self.options,
            self.geometry.gen_w as u32,
            self.geometry.gen_h as u32,
        );
        emit::populate(&mut map_file, self.grid, self.terrain, &[], self.waypoints);
        GenerationSnapshot {
            map_file,
            start_waypoints: self.waypoints.to_vec(),
        }
    }
}

/// What the generator may construct.
pub struct GenerationObjects<'a> {
    /// The resolved neutral tech-building list (empty places no tech
    /// buildings).
    pub tech_types: &'a [TechType],
    /// The Ares urban catalog and Main draw, used when the options' urban
    /// switch is set.
    pub urban_areas: Option<super::phases::urban_areas::UrbanInputs<'a>>,
}

impl<'a> GenerationObjects<'a> {
    /// Tech buildings only.
    pub fn tech(tech_types: &'a [TechType]) -> Self {
        Self {
            tech_types,
            urban_areas: None,
        }
    }
}

/// Run the full generator, returning a populated `MapFile` plus the generated
/// start waypoints and the executed stage list.
///
/// `resolved` is a theater/rules snapshot; `blocks` provides tile-block layouts
/// (from `theater_blocks::TheaterTileBlocks`); `objects` lists what may be
/// constructed.
pub fn generate_map(
    options: &RmgOptions,
    settings: &RmgSettings,
    resolved: &ResolvedTheaterInputs,
    blocks: &dyn TileBlocks,
    objects: &GenerationObjects<'_>,
) -> GeneratedMap {
    generate_map_observed(options, settings, resolved, blocks, objects, &mut |_| {})
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

/// Mixed into the map seed for VERA's urban foot stream.
const VERA_URBAN_SALT: u16 = 0xB71D;

/// The urban pass's inputs, the presence it reads and VERA's fill: Ares'
/// switch keeps the seed's own presence and the native district count;
/// VERA's opt-in passes presence 1 (`0x005A5020` never takes its early exit
/// at 1 or more) and the Buildings slider (1..=100) as the fill, from one
/// district up to most of the open land.
fn urban_inputs<'a>(
    options: &RmgOptions,
    objects: &'a GenerationObjects<'a>,
    vera: &'a super::phases::urban_areas::UrbanInputs<'a>,
) -> (
    Option<&'a super::phases::urban_areas::UrbanInputs<'a>>,
    i32,
    Option<i32>,
) {
    #[cfg(feature = "ares")]
    if options.urban_areas {
        if objects.urban_areas.is_none() {
            log::warn!("random map: urban areas requested without an urban catalog");
        }
        return (objects.urban_areas.as_ref(), options.urban_presence, None);
    }
    let _ = objects;
    if options.urban_enabled && options.urban_presence > 0 {
        return (Some(vera), 1, Some(options.urban_presence.min(100)));
    }
    (None, options.urban_presence, None)
}

/// Seeds Fair Starts tries, by generated interior area: fewer on the large
/// sizes, where one generation takes seconds.
fn fair_start_attempts(options: &RmgOptions) -> u32 {
    let geometry = MapGeometry::from_options(options);
    match geometry.gen_w * geometry.gen_h {
        ..=14_400 => 6,
        14_401..=62_500 => 4,
        _ => 2,
    }
}

/// Step between the seeds Fair Starts tries (a prime, so the sixteen-bit
/// seeds it visits never repeat within a search).
const FAIR_SEED_STEP: u32 = 7919;

/// As [`generate_map`], calling `observe` at every generation boundary.
///
/// The observer sees the run; it cannot alter it. Everything that decides the
/// output — the RNG, the scratch, the grid — is either not handed over or handed
/// over shared, so an observed run generates exactly what an unobserved one does.
///
/// VERA extension: with [`RmgOptions::fair_starts`] this generates from a few
/// seeds, the options' own first, and returns the first fair map or else the
/// fairest (`fairness::FairnessReport`). The search reads only the options,
/// so the setup preview and the match load find the same map; the observer
/// sees every attempt.
pub fn generate_map_observed(
    options: &RmgOptions,
    settings: &RmgSettings,
    resolved: &ResolvedTheaterInputs,
    blocks: &dyn TileBlocks,
    objects: &GenerationObjects<'_>,
    observe: &mut dyn FnMut(GenerationPointView<'_>),
) -> GeneratedMap {
    let mut options = options.clone();
    options.normalize();
    if !options.fair_starts {
        return generate_once(&options, settings, resolved, blocks, objects, observe).0;
    }
    let mut best: Option<(GeneratedMap, super::fairness::FairnessReport)> = None;
    for attempt in 0..fair_start_attempts(&options) {
        let seed = (u32::from(options.seed_u16()) + attempt * FAIR_SEED_STEP) & 0xFFFF;
        let candidate = RmgOptions {
            seed: seed as i32,
            ..options.clone()
        };
        let (map, report) = generate_once(&candidate, settings, resolved, blocks, objects, observe);
        let report = report.unwrap_or_default();
        let fair = report.is_fair();
        if best
            .as_ref()
            .is_none_or(|(_, kept)| report.score() > kept.score())
        {
            best = Some((map, report));
        }
        if fair {
            break;
        }
    }
    best.expect("Fair Starts makes at least one attempt").0
}

/// One generation from `options` as they stand (already normalized), and the
/// fairness report under Fair Starts.
fn generate_once(
    options: &RmgOptions,
    settings: &RmgSettings,
    resolved: &ResolvedTheaterInputs,
    blocks: &dyn TileBlocks,
    objects: &GenerationObjects<'_>,
    observe: &mut dyn FnMut(GenerationPointView<'_>),
) -> (GeneratedMap, Option<super::fairness::FairnessReport>) {
    let options = options.clone();
    // VERA extension: an opted-in Urban Presence runs the same urban pass with
    // VERA's catalogue, and its foot picks draw from a stream seeded by the
    // map seed, so the setup preview and the match load build the same map.
    let vera_draws = std::cell::RefCell::new(RmgRng::new(options.seed_u16() ^ VERA_URBAN_SALT));
    let vera_draw = || vera_draws.borrow_mut().next_u32();
    let vera_urban = super::phases::urban_areas::UrbanInputs {
        catalog: &settings.vera_urban,
        main_draw: &vera_draw,
    };
    let (urban, urban_presence, urban_fill) = urban_inputs(&options, objects, &vera_urban);

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
        morphable: &morphable,
        wheel_impassable: resolved.wheel_impassable,
        cliff_back_impassability: resolved.cliff_back_impassability,
        overlay_zones: &resolved.overlay_zones,
        tech_types: objects.tech_types,
        trig: resolved.trig,
        map_type: options.map_type,
        accessibility: options.accessibility,
        region_size: options.region_size,
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
        fair_starts: options.fair_starts,
        seed: options.seed_u16(),
        urban_presence,
        urban_fill,
        urban_areas: urban,
    };

    observe(GenerationPointView {
        point: GenerationPoint::Initial,
        options: &options,
        geometry,
        grid: &grid,
        waypoints: &[],
        terrain: &[],
    });

    let mut on_stage = |stage: Stage,
                        grid: &grid::RmgGrid,
                        waypoints: &[(u8, u16, u16)],
                        terrain: &[(String, i16, i16)]| {
        observe(GenerationPointView {
            point: GenerationPoint::After(stage),
            options: &options,
            geometry,
            grid,
            waypoints,
            terrain,
        });
    };
    let mut output = pipeline::run_pipeline(
        &mut grid,
        &mut scratch,
        &mut rng,
        &mut gauss,
        &geometry,
        &inputs,
        &mut on_stage,
    );

    output.construction_trace.lighting = settings.generated_lighting_override(&options);
    let mut map_file = emit::empty_map_file(&options, geometry.gen_w as u32, geometry.gen_h as u32);
    if let Some(lighting) = settings.generated_lighting_ini(&options) {
        map_file.ini = lighting;
    }
    emit::populate(
        &mut map_file,
        &grid,
        &output.terrain,
        &output.structures,
        &output.waypoints,
    );
    emit::populate_urban(&mut map_file, &output.urban_areas);

    let generated = GeneratedMap {
        map_file,
        mapgen_continuation: Some(rng.into_continuation()),
        construction_trace: output.construction_trace,
        start_waypoints: output.waypoints,
        #[cfg(any(test, feature = "test-support"))]
        stages_run: executed_stages(&options),
        unfilled_start_slots: output.unfilled_start_slots,
    };
    (generated, output.fairness)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::rmg::RmgConstructionOutcome;
    use crate::map::rmg::STAGE_ORDER;
    use crate::map::rmg::phases::shore::{SubTile, TileBlock};
    use crate::map::theater::TheaterCliffRanges;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::terrain_rules::LandType;

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
            slope_set_pieces: -1,
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
            cliffs: TheaterCliffRanges::default(),
        }
    }

    fn resolved() -> ResolvedTheaterInputs {
        ResolvedTheaterInputs {
            trig: None,
            ids: ids(),
            wheel_impassable: {
                let mut w = [false; LAND_TYPES];
                w[3] = true; // Rock, as in stock rules
                w
            },
            cliff_back_impassability: 2,
            overlay_zones: Vec::new(),
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
    fn missing_wheel_key_is_passable_but_missing_section_is_not() {
        // An authored section reads an absent Wheel as 1.0; an unauthored one
        // keeps the zero row (`0x0089EA40`), which is wheel-impassable.
        let rules = TerrainRules::from_ini(&IniFile::from_str("[Clear]\nBuildable=yes\n"));
        let wheel = wheel_impassable_from_rules(&rules);
        for land in LandType::ALL {
            assert_eq!(
                wheel[land.as_index() as usize],
                land != LandType::Clear,
                "{}",
                land.section_name()
            );
        }
        assert!(wheel[LandType::ALL.len()..].iter().all(|&blocked| !blocked));
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

    /// [`generate_map`] of `options(map_type, theater)`, run once per test
    /// process: several tests read the same map.
    fn shared_run(map_type: i32, theater: i32) -> std::sync::Arc<GeneratedMap> {
        crate::test_fixture::once_per_process(("generated map", map_type, theater), || {
            let options = options(map_type, theater);
            let generated = generate_map(
                &options,
                &RmgSettings::default(),
                &resolved(),
                &one_by_one(),
                &GenerationObjects::tech(&[]),
            );
            Some(std::sync::Arc::new(generated))
        })
        .expect("generated map")
    }

    /// Ares urban areas (hook 0x598FB8): the switch runs the stage, and each
    /// placed urban object follows the structures at the entity index its
    /// constructor row names.
    #[cfg(feature = "ares")]
    #[test]
    fn urban_objects_follow_the_structures_with_matching_trace_rows() {
        use crate::map::rmg::RmgConstructionPhase;
        use crate::map::rmg::phases::urban_areas::{UrbanCatalog, UrbanFoot, UrbanInputs};
        let foot = |name: &str| UrbanFoot {
            name: name.into(),
            known: true,
        };
        let catalog = UrbanCatalog {
            structures: Vec::new(),
            infantry: vec![foot("CIV1")],
            vehicles: vec![foot("TRUCKA")],
            ..Default::default()
        };
        let draws = std::cell::Cell::new(0u32);
        let main_draw = || {
            draws.set(draws.get() + 1);
            draws.get()
        };
        let objects = GenerationObjects {
            tech_types: &[],
            urban_areas: Some(UrbanInputs {
                catalog: &catalog,
                main_draw: &main_draw,
            }),
        };
        let mut placed = 0;
        for seed in 1..=8 {
            let options = RmgOptions {
                urban_areas: true,
                urban_presence: 60,
                num_players: 2,
                water_amount: 0,
                seed,
                ..options(1, 0)
            };
            let generated = generate_map(
                &options,
                &RmgSettings::default(),
                &resolved(),
                &one_by_one(),
                &objects,
            );
            assert!(generated.stages_run.contains(&Stage::UrbanAreas));
            for event in &generated.construction_trace.events {
                if event.phase != RmgConstructionPhase::UrbanAreas {
                    continue;
                }
                if let RmgConstructionOutcome::Emitted { entity_index, cell } = &event.outcome {
                    let entity = &generated.map_file.entities[*entity_index];
                    assert_eq!(entity.type_id, event.techno_type);
                    assert_eq!((entity.cell_x, entity.cell_y), *cell);
                    assert_eq!(entity.owner, "Neutral");
                    placed += 1;
                }
            }
        }
        assert!(placed > 0, "some seed's urban area placed foots");
        assert!(draws.get() > 0, "each foot pick draws Main");
    }

    /// VERA extension: a thirty-player seed gets a start for every player.
    #[test]
    fn a_thirty_player_map_places_every_start() {
        let options = RmgOptions {
            num_players: 30,
            ..options(1, 0)
        };
        let generated = generate_map(
            &options,
            &RmgSettings::default(),
            &resolved(),
            &one_by_one(),
            &GenerationObjects::tech(&[]),
        );
        assert_eq!(generated.unfilled_start_slots, 0);
        let mut slots: Vec<u8> = generated.start_waypoints.iter().map(|w| w.0).collect();
        slots.sort_unstable();
        assert_eq!(slots, (0..30).collect::<Vec<u8>>());
    }

    /// VERA extension: thirty players' islands survive the start threshold,
    /// so an archipelago seats every player at the default and largest size.
    #[test]
    #[ignore = "slow; run when changing what it covers"]
    fn thirty_player_archipelagos_place_every_start() {
        let Some((settings, resolved, blocks)) = retail_inputs() else {
            return;
        };
        for size in [0, crate::map::rmg::MAX_SIZE_OPTION] {
            let options = RmgOptions {
                num_players: 30,
                width: size,
                height: size,
                ..options(0, 0)
            };
            let generated = generate_map(
                &options,
                &settings,
                &resolved,
                &blocks,
                &GenerationObjects::tech(&[]),
            );
            assert_eq!(
                (
                    generated.unfilled_start_slots,
                    generated.start_waypoints.len()
                ),
                (0, 30),
                "size option {size}"
            );
        }
    }

    /// VERA extension: the largest size option generates a 500x500 interior.
    #[test]
    #[ignore = "slow; run when changing what it covers"]
    fn a_colossal_thirty_player_map_generates_at_500() {
        let options = RmgOptions {
            num_players: 30,
            width: crate::map::rmg::MAX_SIZE_OPTION,
            height: crate::map::rmg::MAX_SIZE_OPTION,
            ..options(1, 0)
        };
        let generated = generate_map(
            &options,
            &RmgSettings::default(),
            &resolved(),
            &one_by_one(),
            &GenerationObjects::tech(&[]),
        );
        assert_eq!(generated.unfilled_start_slots, 0);
        assert_eq!(generated.start_waypoints.len(), 30);
        assert_eq!(
            (
                generated.map_file.header.width,
                generated.map_file.header.height
            ),
            (504, 512)
        );
    }

    /// VERA extension: the dialog's Urban and Desert theaters and
    /// its Archipelago map type generate from retail data with every start
    /// placed, and DESERT's empty waterfall sets are absent to the generator.
    #[test]
    #[ignore = "slow; run when changing what it covers"]
    fn the_added_theaters_and_archipelago_generate_from_retail_data() {
        for (theater, name) in [(2, "URBAN"), (3, "DESERT")] {
            let Some((settings, resolved, blocks)) = retail_inputs_for(name) else {
                return;
            };
            if name == "DESERT" {
                assert_eq!(resolved.ids.cliffs.waterfall_bases(), [-1; 4]);
            } else {
                assert!(
                    resolved
                        .ids
                        .cliffs
                        .waterfall_bases()
                        .iter()
                        .all(|&b| b >= 0)
                );
            }
            for map_type in [0, 1, 4] {
                let options = RmgOptions {
                    num_players: 8,
                    ..options(map_type, theater)
                };
                let generated = generate_map(
                    &options,
                    &settings,
                    &resolved,
                    &blocks,
                    &GenerationObjects::tech(&[]),
                );
                assert_eq!(
                    (
                        generated.unfilled_start_slots,
                        generated.start_waypoints.len()
                    ),
                    (0, 8),
                    "{name} map type {map_type}"
                );
            }
        }
    }

    /// VERA extension: an opted-in Urban Presence runs the urban pass
    /// (`0x005A5020`) in any build with VERA's catalogue: paved roads,
    /// civilian buildings of the theater's style families and foots, every one
    /// traced so the load replays it. The same presence without the opt-in
    /// leaves the pass out.
    #[test]
    #[ignore = "slow; run when changing what it covers"]
    fn opted_in_urban_presence_builds_paved_districts() {
        let Some((mut settings, resolved, blocks)) = retail_inputs() else {
            return;
        };
        let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets().expect("retail");
        crate::map::theater::load_theater(&mut assets, "TEMPERATE").expect("TEMPERATE");
        let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()
            .expect("retail rules");
        let art = crate::rules::retail_ini_fixture::retail_ini("artmd.ini").expect("art");
        settings.vera_urban = crate::map::rmg::tech_catalog::resolve_vera_urban(
            &rules,
            &art,
            &[],
            "tem",
            "TEMPERATE",
            4242,
            |file| assets.contains(file),
        );
        assert!(!settings.vera_urban.structures.is_empty());
        assert!(!settings.vera_urban.infantry.is_empty());
        assert!(
            !settings
                .vera_urban
                .structures
                .iter()
                .any(|entry| entry.name == "CALOND06"),
            "Temperate draws CALOND06 as Westwood's placeholder cube"
        );

        let base = RmgOptions {
            num_players: 4,
            width: 2,
            height: 2,
            urban_presence: 80,
            water_amount: 15,
            seed: 4242,
            ..options(1, 0)
        };
        let objects = GenerationObjects::tech(&[]);
        let plain = generate_map(&base, &settings, &resolved, &blocks, &objects);
        assert!(!plain.stages_run.contains(&Stage::UrbanAreas));
        let urban = generate_map(
            &RmgOptions {
                urban_enabled: true,
                ..base.clone()
            },
            &settings,
            &resolved,
            &blocks,
            &objects,
        );
        assert!(urban.stages_run.contains(&Stage::UrbanAreas));
        let roads = urban
            .map_file
            .cells
            .iter()
            .filter(|cell| resolved.ids.is_paved_road(cell.tile_index))
            .count();
        assert!(roads > 0, "the district has paved roads");
        // Buildings 1 is one district; 100 paves most of the open land (this
        // small map's land is broken up, so it takes only a few more).
        let paved = |presence: i32| {
            let map = generate_map(
                &RmgOptions {
                    urban_enabled: true,
                    urban_presence: presence,
                    ..base.clone()
                },
                &settings,
                &resolved,
                &blocks,
                &objects,
            );
            map.map_file
                .cells
                .iter()
                .filter(|cell| cell.tile_index == resolved.ids.pave)
                .count()
        };
        let (least, most) = (paved(1), paved(100));
        assert!(
            most > least * 3 / 2,
            "Buildings 100 paves {most} cells, 1 paves {least}"
        );
        assert!(
            urban.map_file.entities.len() > plain.map_file.entities.len() + 4,
            "the district adds buildings: {} vs {}",
            urban.map_file.entities.len(),
            plain.map_file.entities.len()
        );
        for (index, entity) in urban.map_file.entities.iter().enumerate() {
            let traced = urban.construction_trace.events.iter().any(|event| {
                event.outcome
                    == crate::map::rmg::RmgConstructionOutcome::Emitted {
                        entity_index: index,
                        cell: (entity.cell_x, entity.cell_y),
                    }
            });
            assert!(
                traced,
                "entity {index} {} has no constructor",
                entity.type_id
            );
        }

        // Street furniture: props and trees on pavement that touches a road.
        let props = &settings.vera_urban.props;
        assert!(!props.is_empty() && !settings.vera_urban.trees.is_empty());
        let tile_at = |x: u16, y: u16| {
            urban
                .map_file
                .cells
                .iter()
                .find(|cell| (cell.rx, cell.ry) == (x, y))
                .map_or(-1, |cell| cell.tile_index)
        };
        let placed: Vec<_> = urban
            .map_file
            .entities
            .iter()
            .filter(|entity| props.iter().any(|prop| prop.name == entity.type_id))
            .collect();
        assert!(!placed.is_empty(), "the districts have street furniture");
        for prop in placed {
            let (x, y) = (prop.cell_x, prop.cell_y);
            let by_road = [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]
                .into_iter()
                .any(|(nx, ny)| resolved.ids.is_paved_road(tile_at(nx, ny)));
            assert!(by_road, "{} at {x},{y} stands off the road", prop.type_id);
        }
        let street_trees = urban
            .map_file
            .terrain_objects
            .iter()
            .filter(|tree| {
                let tile = tile_at(tree.rx, tree.ry);
                resolved.ids.is_pave(tile) || resolved.ids.is_misc_pave(tile)
            })
            .count();
        assert!(street_trees > 0, "trees stand on the pavement");

        // A city over the largest size keeps its buildings within budget.
        let largest = generate_map(
            &RmgOptions {
                urban_enabled: true,
                urban_presence: 100,
                width: 8,
                height: 8,
                ..base.clone()
            },
            &settings,
            &resolved,
            &blocks,
            &objects,
        );
        let buildings = largest
            .map_file
            .entities
            .iter()
            .filter(|entity| entity.category == crate::map::entities::EntityCategory::Structure)
            .count();
        assert!(
            (1500..3000).contains(&buildings),
            "{buildings} buildings on the largest map"
        );
    }

    /// VERA extension: the exported `.yrm` text of a generated map parses
    /// back to the same cells, overlays, objects and starts.
    #[test]
    fn exported_map_text_reads_back_the_generated_map() {
        let generated = generate_map(
            &options(1, 0),
            &RmgSettings::default(),
            &resolved(),
            &one_by_one(),
            &GenerationObjects::tech(&[]),
        );
        let map = &generated.map_file;
        let thumbnail: Vec<u8> = (0..6 * 4 * 4).map(|byte| byte as u8 | 3).collect();
        let starts = crate::map::preview::PreviewSourceBounds {
            origin_x: 3,
            origin_y: 5,
            width: 40,
            height: 30,
            start_points: vec![
                crate::map::preview::PreviewStartPoint { x: 10, y: 12 },
                crate::map::preview::PreviewStartPoint { x: 30, y: 25 },
            ],
        };
        let text = crate::map::map_export::multiplayer_map_text(
            map,
            "Exported",
            2,
            4,
            Some(crate::map::map_export::ExportPreview {
                width: 6,
                height: 4,
                rgba: &thumbnail,
            }),
            Some(&starts),
        );
        assert_eq!(
            crate::map::scenario_menu::preview_source_bounds_from_verified_source(
                &crate::rules::ini_parser::IniFile::from_str(&text)
            ),
            Some(starts.clone()),
            "the map list reads the start markers back"
        );
        let read = crate::map::map_file::MapFile::from_bytes(text.as_bytes()).expect("parses");
        let shown = crate::map::preview::decode_preview_image_from_ini(&read.ini)
            .expect("the thumbnail decodes")
            .expect("the map carries a thumbnail");
        assert_eq!((shown.width, shown.height), (6, 4));
        let rgb = |rgba: &[u8]| -> Vec<u8> {
            rgba.chunks_exact(4)
                .flat_map(|p| [p[0], p[1], p[2]])
                .collect()
        };
        assert_eq!(rgb(&shown.rgba), rgb(&thumbnail));

        assert_eq!(read.basic.name.as_deref(), Some("Exported"));
        assert_eq!(read.header.theater, map.header.theater);
        assert_eq!(
            (read.header.width, read.header.local_width),
            (map.header.width, map.header.local_width)
        );
        let cells = |m: &crate::map::map_file::MapFile| {
            let mut cells: Vec<_> = m
                .cells
                .iter()
                .map(|c| (c.rx, c.ry, c.tile_index, c.sub_tile, c.z))
                .collect();
            cells.sort_unstable();
            cells
        };
        assert!(!map.cells.is_empty());
        assert_eq!(cells(&read), cells(map));
        let overlays = |m: &crate::map::map_file::MapFile| {
            let mut overlays: Vec<_> = m
                .overlays
                .iter()
                .map(|o| (o.rx, o.ry, o.overlay_id, o.frame))
                .collect();
            overlays.sort_unstable();
            overlays
        };
        assert!(!map.overlays.is_empty());
        assert_eq!(overlays(&read), overlays(map));
        let objects = |m: &crate::map::map_file::MapFile| {
            let mut objects: Vec<_> = m
                .entities
                .iter()
                .map(|e| (e.type_id.clone(), e.cell_x, e.cell_y, e.facing))
                .chain(
                    m.terrain_objects
                        .iter()
                        .map(|t| (t.name.clone(), t.rx, t.ry, 0)),
                )
                .collect();
            objects.sort_unstable();
            objects
        };
        assert_eq!(objects(&read), objects(map));
        assert_eq!(read.waypoints.len(), generated.start_waypoints.len());
        for (slot, x, y) in &generated.start_waypoints {
            let waypoint = &read.waypoints[&u32::from(*slot)];
            assert_eq!((waypoint.rx, waypoint.ry), (*x, *y));
        }
    }

    /// VERA extension: Fair Starts leaves the terrain and the starts of a
    /// seed as they were, evens the ore near every start, and finds the same
    /// map on every run.
    #[test]
    fn fair_starts_even_the_ore_and_repeat() {
        let (settings, resolved, blocks) = (RmgSettings::default(), resolved(), one_by_one());
        let objects = GenerationObjects::tech(&[]);
        let fair = RmgOptions {
            fair_starts: true,
            ..options(1, 0)
        };
        let plain = generate_map(&options(1, 0), &settings, &resolved, &blocks, &objects);
        let (first, report) =
            generate_once(&fair, &settings, &resolved, &blocks, &objects, &mut |_| {});
        let report = report.expect("Fair Starts measures the map");
        let tiles = |map: &GeneratedMap| -> Vec<(i32, u8)> {
            map.map_file
                .cells
                .iter()
                .map(|cell| (cell.tile_index, cell.z))
                .collect()
        };
        assert_eq!(
            tiles(&first),
            tiles(&plain),
            "the pass runs after every stage"
        );
        assert_eq!(first.start_waypoints, plain.start_waypoints);
        assert!(report.ore_max > 0, "the starts have ore");
        assert!(
            report.ore_min * 100 >= report.ore_max * 95,
            "ore near the starts is even: {report:?}"
        );
        assert!(
            report.expansion_min > 0 && report.expansion_min * 100 >= report.expansion_max * 90,
            "every start has an expansion field of about the same ore: {report:?}"
        );

        let overlays = |map: &GeneratedMap| -> Vec<(u16, u16, u8, u8)> {
            map.map_file
                .overlays
                .iter()
                .map(|o| (o.rx, o.ry, o.overlay_id, o.frame))
                .collect()
        };
        let searched = generate_map(&fair, &settings, &resolved, &blocks, &objects);
        let again = generate_map(&fair, &settings, &resolved, &blocks, &objects);
        assert_eq!(overlays(&searched), overlays(&again));
        assert_eq!(searched.start_waypoints, again.start_waypoints);
    }

    #[test]
    fn generate_map_is_deterministic() {
        let resolved = resolved();
        let blocks = one_by_one();
        let settings = RmgSettings::default();
        let options = options(1, 0);
        let snapshot = || {
            let mut g = generate_map(
                &options,
                &settings,
                &resolved,
                &blocks,
                &GenerationObjects::tech(&[]),
            );
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
            let generated = generate_map(
                &options,
                &settings,
                &resolved,
                &blocks,
                &GenerationObjects::tech(&types),
            );
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

    /// Map type 0's start-phase flips (`CreateStartingPoints @ 0x00594B50`,
    /// 0x00594EA6..0x00594ED5) place a tech building on a start-free island,
    /// and the trace binds it to its entity. Retail tiles, so the sea
    /// separates islands into their own regions.
    #[test]
    fn map_type_zero_places_tech_at_start_flips() {
        let Some((settings, resolved, blocks)) = retail_inputs() else {
            return;
        };
        let types = [TechType {
            name: "CATECH".to_string(),
            footprint: vec![(0, 0)],
        }];
        let placed = (1..=8).find_map(|seed| {
            let mut options = options(0, 0);
            options.seed = seed;
            let generated = generate_map(
                &options,
                &settings,
                &resolved,
                &blocks,
                &GenerationObjects::tech(&types),
            );
            let emitted =
                generated
                    .construction_trace
                    .events
                    .iter()
                    .find_map(|event| match event.outcome {
                        RmgConstructionOutcome::Emitted { entity_index, cell } => {
                            Some((entity_index, cell))
                        }
                        RmgConstructionOutcome::Discarded => None,
                    })?;
            Some((generated, emitted))
        });
        let (generated, (entity_index, cell)) =
            placed.expect("a map-type-0 seed wins a flip and places");
        let entity = &generated.map_file.entities[entity_index];
        assert_eq!(entity.type_id, "CATECH");
        assert_eq!((entity.cell_x, entity.cell_y), cell);
    }

    type Projection = (Vec<i32>, Vec<(u16, u16, u8, u8)>, Vec<(u8, u16, u16)>);

    /// The comparable content of a generated map: what the emitter projected
    /// plus the start positions.
    fn projection(generated: &GeneratedMap) -> Projection {
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

    type ObservedRun = (GeneratedMap, Vec<GenerationPoint>, Vec<GenerationSnapshot>);

    fn observe_run(options: &RmgOptions) -> ObservedRun {
        observe_run_with(options, &RmgSettings::default(), &resolved(), &one_by_one())
    }

    fn observe_run_with(
        options: &RmgOptions,
        settings: &RmgSettings,
        resolved: &ResolvedTheaterInputs,
        blocks: &dyn TileBlocks,
    ) -> ObservedRun {
        let mut points = Vec::new();
        let mut snapshots = Vec::new();
        let generated = generate_map_observed(
            options,
            settings,
            resolved,
            blocks,
            &GenerationObjects::tech(&[]),
            &mut |view| {
                points.push(view.point());
                snapshots.push(view.snapshot());
            },
        );
        (generated, points, snapshots)
    }

    /// Retail TEMPERATE generator inputs, or `None` when the install skips.
    ///
    /// The synthetic fixture has no relief ahead of the island passes (its
    /// rivers are off) and admits no connector, so the witnesses of those two
    /// stages read retail data instead.
    fn retail_inputs() -> Option<(
        RmgSettings,
        ResolvedTheaterInputs,
        crate::map::rmg::theater_blocks::TheaterTileBlocks,
    )> {
        retail_inputs_for("TEMPERATE")
    }

    /// [`retail_inputs`] for the named retail theater.
    fn retail_inputs_for(
        theater_name: &str,
    ) -> Option<(
        RmgSettings,
        ResolvedTheaterInputs,
        crate::map::rmg::theater_blocks::TheaterTileBlocks,
    )> {
        let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets()?;
        let rules = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini")?;
        let theater = crate::map::theater::load_theater(&mut assets, theater_name)
            .unwrap_or_else(|| panic!("retail {theater_name} theater"));
        let ruleset = crate::rules::ruleset::RuleSet::from_ini(&rules).expect("retail rules");
        let resolved = ResolvedTheaterInputs::from_theater(
            &theater,
            &TerrainRules::from_ini(&rules),
            ruleset.overlay_registry(),
            ruleset.general.cliff_back_impassability,
        );
        let blocks =
            crate::map::rmg::theater_blocks::TheaterTileBlocks::build(&theater.lookup, |name| {
                assets.get(name)
            });
        Some((RmgSettings::load(&assets), resolved, blocks))
    }

    /// [`observe_run`] of `options(map_type, theater)`, run once per test
    /// process: several tests read the same observed run.
    fn shared_observed_run(map_type: i32, theater: i32) -> std::sync::Arc<ObservedRun> {
        crate::test_fixture::once_per_process(("observed run", map_type, theater), || {
            Some(std::sync::Arc::new(observe_run(&options(
                map_type, theater,
            ))))
        })
        .expect("observed run")
    }

    #[test]
    fn observing_a_run_does_not_change_what_it_generates() {
        let plain = shared_run(1, 0);
        // The observer does real work at every boundary — it projects the whole
        // grid — so this covers the projection as well as the callback.
        let observed = shared_observed_run(1, 0);
        let (observed, _, _) = &*observed;

        assert_eq!(plain.stages_run, observed.stages_run);
        assert_eq!(projection(&plain), projection(observed));
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
        let run = shared_observed_run(1, 0);
        let (_, points, _) = &*run;
        assert_eq!(*points, expected);
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
        let run = shared_observed_run(1, 0);
        let (generated, _, snapshots) = &*run;
        let final_header = &generated.map_file.header;
        for snapshot in snapshots {
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
        let run = shared_observed_run(1, 0);
        let (generated, points, snapshots) = &*run;
        assert!(
            !generated.start_waypoints.is_empty(),
            "the run placed starts"
        );
        let first_with_starts = points
            .iter()
            .zip(snapshots)
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

    // ---- The generation matrix -------------------------------------------
    //
    // Everything below is a REGRESSION RATCHET, never parity evidence: the
    // inputs are synthetic (retail for the island-pass and accessibility
    // witnesses) and both sides of every comparison come from this build. It can expose a panic, a gate regression, or nondeterminism. It
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
    #[ignore = "slow; run when changing what it covers"]
    fn accessibility_moves_island_maps_and_leaves_the_others_alone() {
        let Some((settings, resolved, blocks)) = retail_inputs() else {
            return;
        };
        let cells = |options: &RmgOptions| {
            generate_map(
                options,
                &settings,
                &resolved,
                &blocks,
                &GenerationObjects::tech(&[]),
            )
            .map_file
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
            cells(&low) != cells(&high)
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
            &GenerationObjects::tech(&[]),
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
    #[ignore = "slow; run when changing what it covers"]
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
    #[ignore = "slow; run when changing what it covers"]
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
    #[ignore = "slow; run when changing what it covers"]
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

    fn snapshot_projection(snapshot: &GenerationSnapshot) -> Projection {
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
    #[ignore = "slow; run when changing what it covers"]
    fn island_passes_reshape_only_the_island_map_types() {
        let Some((settings, resolved, blocks)) = retail_inputs() else {
            return;
        };
        let changed = |map_type, seed| {
            let (_, points, snapshots) = observe_run_with(
                &matrix_options(map_type, 0, seed),
                &settings,
                &resolved,
                &blocks,
            );
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

    #[test]
    fn accepted_generated_map_uses_retail_night_lighting() {
        use crate::map::lighting::parse_lighting_profiles;
        let mut settings = RmgSettings::default();
        // RMGMD.INI: retail night differs by theater; level 3 emits zero.
        settings.apply(&IniFile::from_str(
            "[General]\nTemperateAmbientLight=75,100,75,35\n\
             SnowAmbientLight=75,100,75,55\nRMGLevelLightSettings=3,3,3,3\n\
             TemperateAmbientRed=109,108,71,51\nTemperateAmbientGreen=80,94,81,46\n\
             TemperateAmbientBlue=49,68,119,141\nSnowAmbientRed=112,99,106,76\n\
             SnowAmbientGreen=80,104,107,74\nSnowAmbientBlue=133,107,154,118\n",
        ));
        for (theater, ambient) in [(0, 35), (1, 55)] {
            let options = RmgOptions {
                theater,
                time: 3,
                ..options(1, theater)
            };
            let mut preview_has_no_synthetic_lighting = false;
            let generated = generate_map_observed(
                &options,
                &settings,
                &resolved(),
                &one_by_one(),
                &GenerationObjects::tech(&[]),
                &mut |view| {
                    if view.point() == GenerationPoint::Initial {
                        preview_has_no_synthetic_lighting = view.snapshot().map_file.ini.is_empty();
                    }
                },
            );
            assert!(preview_has_no_synthetic_lighting);
            let section = generated.map_file.ini.section("Lighting").unwrap();
            assert_eq!(section.entry_count(), 12);
            assert_eq!(section.get_for_test("RedTint"), Some("1.000000"));
            let profiles = parse_lighting_profiles(&generated.map_file.ini);
            assert_eq!(profiles.normal.ambient_percent, ambient);
            assert_eq!(profiles.normal.ground_units, 0);
            assert_eq!(profiles.normal.level_units, 0);
            let descriptor = crate::sim::scenario_session::ScenarioDescriptor {
                lighting: crate::sim::scenario_session::ScenarioLightingState::from_map(&profiles),
                ..Default::default()
            };
            let mut simulation = crate::sim::world::Simulation::from_descriptor(&descriptor);
            // This no-tech/no-bridge fixture isolates the real initialization
            // replay from Building IDs/RNG; it still uses its production owner.
            assert!(generated.construction_trace.events.is_empty());
            let rng_before = simulation.rng_state();
            simulation
                .replay_staged_generated_construction_trace(&generated.construction_trace)
                .unwrap();
            assert_eq!(simulation.rng_state(), rng_before);
            let lighting = simulation.session().lighting();
            assert_eq!(
                (lighting.current_ambient(), lighting.target_ambient()),
                (ambient, ambient)
            );
            assert_eq!(lighting.normal().level_units, 3);
            let expected_rgb = if theater == 0 {
                [51, 46, 141]
            } else {
                [76, 74, 118]
            };
            let normal = lighting.normal();
            assert_eq!(
                [
                    normal.red_percent,
                    normal.green_percent,
                    normal.blue_percent
                ],
                expected_rgb
            );
            assert_eq!(normal.ground_units, 0);
            assert_eq!(lighting.ion().level_units, 0);
            assert_eq!(profiles.ion.ambient_percent, ambient);
            assert_eq!(
                [
                    profiles.ion.red_percent,
                    profiles.ion.green_percent,
                    profiles.ion.blue_percent
                ],
                [30, 40, 75]
            );
            assert_eq!(
                (profiles.ion.ground_units, profiles.ion.level_units),
                (0, 0)
            );
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
    }
}
