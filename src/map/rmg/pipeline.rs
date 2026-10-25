//! The deterministic generation pipeline: every terrain phase in `STAGE_ORDER`,
//! wired together with the shared grid / scratch / rng / gauss and the per-phase
//! state threaded between them.
//!
//! Inputs are **pre-resolved** — `TileIds`, a `TileBlocks` provider, the cliff
//! ranges, the `Morphable=` predicate, the wheel-impassable table, the neutral
//! tech-building list, and the MapSeed-derived scalars. This layer never touches
//! `TheaterData` or the rules directly; the app layer builds the inputs and
//! hands them in, which keeps the pipeline unit-testable with the same synthetic
//! `TileBlocks` the individual phases use.
//!
//! The order is the native temperate driver's (see `STAGE_ORDER`): water →
//! finalize → regions → green spread → recalc → starts → tech → tiberium →
//! region reset → recalc → hills → patches → recalc → trees → rocks → recalc.
//! Emit is the caller's step. The three `RecalcAfter*`/`RecalcFinal` passes are
//! `lat_fixup` calls; they are RNG-free, so the one that matters observably is
//! the post-patch pass that produces the sand-LAT tiles the rock pass needs.

use super::grid::RmgGrid;
use super::phases::adjacency;
use super::phases::blob::BlobCtx;
use super::phases::carve::CarveCtx;
use super::phases::carve_driver::{self, ConnectorRegion};
use super::phases::hills::{HillsArgs, HillsCtx};
use super::phases::island_passes::{self, IslandCtx};
use super::phases::lat_patches::PatchCtx;
use super::phases::regions::{RegionCtx, Regions};
use super::phases::rocks::{RockArgs, RockCtx};
use super::phases::shore::TileBlocks;
use super::phases::starts::{StartsArgs, StartsCtx};
use super::phases::tech_buildings::{TechArgs, TechCtx, TechType};
use super::phases::tiberium::{TiberiumArgs, TiberiumCtx};
use super::phases::trees::{TreeArgs, TreeCtx};
use super::phases::water::{PlayableRect, WaterArgs};
use super::phases::zones::{self, ZoneKind, ZoneParams};
use super::phases::{
    cliff_materialization, cliff_variants, green_spread, hills, lat_fixup, lat_patches, regions,
    rocks, starts, tech_buildings, tiberium, trees, water, water_finalize,
};
use super::rng::RmgRng;
use super::scratch::RmgScratch;
use super::tiles::TileIds;
use super::x87::Gaussian;
use super::{MapGeometry, RmgConstructionTrace, Stage};
use crate::map::playfield::PlayfieldBounds;

/// The land-type count of the wheel-impassable table (`zones::LAND_TYPES`).
pub const LAND_TYPES: usize = super::phases::zones::LAND_TYPES;

/// The generator's default ground level (`MapSeed+0x30C` = 4).
const DEFAULT_LEVEL: u8 = 4;

/// Pre-resolved inputs for one generation run.
pub struct PipelineInputs<'a> {
    pub ids: &'a TileIds,
    pub blocks: &'a dyn TileBlocks,
    /// The `[TileSet] Morphable=` predicate for the corner engine.
    pub morphable: &'a dyn Fn(i32) -> bool,
    /// Per-land-type "wheel speed <= 1%" flags for the zone classifier.
    pub wheel_impassable: [bool; LAND_TYPES],
    /// The resolved `NeutralTechBuildings` list.
    pub tech_types: &'a [TechType],
    /// The generator's sine table, for the river's heading.
    pub trig: Option<&'a crate::util::retail_trig::TrigTable>,

    // Scalars from the `.SED` options / MapSeed / RMGMD settings.
    pub map_type: i32,
    /// How freely plateaus get linked by ramps. Zero does not disable the
    /// pass — it gives every adjacent pair exactly one ramp.
    pub accessibility: i32,
    pub theater: i32,
    pub num_players: i32,
    /// `WaterAmount` option.
    pub water_percent: i32,
    /// `Resources` option (gems appear only when 3).
    pub resources: i32,
    /// `Tiberium` percent option.
    pub tib_option: i32,
    /// `RMGMinimumTiberium` / `RMGMaximumTiberium`.
    pub min_tib: i32,
    pub max_tib: i32,
    /// `TiberiumLayout` option (start-selector slot target).
    pub tiberium_layout: i32,
    /// `Ruggedness` option (hill height-walk scale).
    pub ruggedness: i32,
    /// `Vegetation` option (patch probability + tree-count scale).
    pub vegetation: i32,
    /// The tree-count width term (`MapSeed+0x64`).
    pub width: i32,
    /// `MaxTrees` (`MapSeed+0x2FC`; 0 disables trees).
    pub max_trees: i32,
    /// Whether a river may attempt waterfall terrain, decided by the
    /// generation's very first draw. Only the river carver reads the value;
    /// the draw that produces it is stream-relevant to every map type.
    pub bridge_enabled: bool,
}

/// Watches the grid between stages, for callers that draw the map as it builds.
///
/// Observation only: the grid arrives shared and the RNG, scratch and per-phase
/// state are not passed at all, so an observer cannot change what a run
/// generates. It fires at the boundary *named* by the stage whether or not that
/// stage did any work — a stage this configuration skips leaves the grid
/// untouched, so its boundary looks exactly like the one before it.
///
/// Waypoints are empty until Starts. Terrain is the same chronological
/// accumulator later moved into PipelineOutput: TIBTRE precedes ordinary TREE.
pub type StageObserver<'a> =
    &'a mut dyn FnMut(Stage, &RmgGrid, &[(u8, u16, u16)], &[(String, i16, i16)]);

/// What the pipeline collects for the emitter.
#[derive(Debug, Default)]
pub struct PipelineOutput {
    /// `(slot, x, y)` per generated start position.
    pub waypoints: Vec<(u8, u16, u16)>,
    /// Placed `(name, x, y)` terrain objects — trees and TIBTRE.
    pub terrain: Vec<(String, i16, i16)>,
    /// Placed `(name, x, y)` neutral tech buildings.
    pub structures: Vec<(String, i16, i16)>,
    /// Scenario-consuming generated Building constructors in exact order.
    pub(crate) construction_trace: RmgConstructionTrace,
    /// Start slots no region could fill. See `StartsOutcome`.
    pub unfilled_start_slots: usize,
}

/// Run the whole deterministic pipeline in place over `grid`/`scratch`, drawing
/// from `rng`/`gauss`. Returns the waypoints, terrain objects and structures the
/// emitter projects into the `MapFile`.
pub fn run_pipeline(
    grid: &mut RmgGrid,
    scratch: &mut RmgScratch,
    rng: &mut RmgRng,
    gauss: &mut Gaussian,
    geometry: &MapGeometry,
    inputs: &PipelineInputs<'_>,
    observe: StageObserver<'_>,
) -> PipelineOutput {
    let MapGeometry {
        gen_w,
        gen_h,
        map_w,
        map_h,
        stride,
        ..
    } = *geometry;
    let local_rect = [2, 5, gen_w, gen_h];
    // Emit-form start positions, filled in once `Starts` has produced them so
    // every later boundary can hand them to the observer.
    let mut waypoints: Vec<(u8, u16, u16)> = Vec::new();
    // Native RMG constructs TIBTRE at5A2C05 before ordinary TREE at5A48AA.
    // Borrow this one chronology for progress; final emission moves it intact.
    let mut terrain: Vec<(String, i16, i16)> = Vec::new();
    let mut structures: Vec<(String, i16, i16)> = Vec::new();
    let mut construction_trace = RmgConstructionTrace::default();

    // ---- Water + finalize (map types 0-2 shape the base terrain) ----------
    // Struct-literal fields move a `&mut`, so every context reborrows the shared
    // owners (`&mut *grid` …); each context is scoped so the reborrow releases
    // before the next stage.
    {
        let mut ctx = BlobCtx {
            grid: &mut *grid,
            scratch: &mut *scratch,
            ids: inputs.ids,
            blocks: inputs.blocks,
            rng: &mut *rng,
            gauss: &mut *gauss,
            trig: inputs.trig,
            map_w,
            map_h,
            rollback_level: DEFAULT_LEVEL,
        };
        let args = WaterArgs {
            map_type: inputs.map_type,
            water_percent: inputs.water_percent,
            num_players: inputs.num_players,
            bridge_enabled: inputs.bridge_enabled,
            playable: PlayableRect {
                x: local_rect[0],
                y: local_rect[1],
                w: gen_w,
                h: gen_h,
            },
        };
        water::run(&mut ctx, &args);
    }
    observe(Stage::Water, grid, &waypoints, &terrain);
    water_finalize::run(grid, inputs.ids, inputs.blocks, rng);
    observe(Stage::WaterFinalize, grid, &waypoints, &terrain);

    // ---- Regions -----------------------------------------------------------
    let mut regions: Regions = {
        let mut ctx = RegionCtx {
            grid: &mut *grid,
            scratch: &mut *scratch,
            ids: inputs.ids,
            rng: &mut *rng,
            map_type: inputs.map_type,
            default_level: DEFAULT_LEVEL,
        };
        regions::run(&mut ctx)
    };
    observe(Stage::Regions, grid, &waypoints, &terrain);

    // ---- Island passes (map types 3 and 4 only) ---------------------------
    // The cliff repair stamps region tags as a side effect, so the pass hands
    // back a rebuilt partition that replaces the one above wholesale. Every
    // later stage reads the rebuilt ids.
    if matches!(inputs.map_type, 3 | 4) {
        let mut ctx = IslandCtx {
            grid: &mut *grid,
            scratch: &mut *scratch,
            ids: inputs.ids,
            rng: &mut *rng,
            default_level: DEFAULT_LEVEL,
        };
        regions = island_passes::run(&mut ctx);

        // Then the ramps. THREE SEPARATE PASSES over every region, in this
        // order — adjacency for all of them, then carving for all of them,
        // then the release. Fusing the first two would let a region see a
        // neighbour's data before that neighbour had been measured, and the
        // maps would drift apart from the original.
        //
        // The first pass is implicit here: `adjacency::neighbour_ids` reads
        // the scratch grid directly rather than caching onto the region, and
        // the grid is already final by this point, so every region sees the
        // same finished partition the original's pre-pass would have built.
        // The third pass — freeing the vectors — has no analogue: the lists
        // are values and drop on their own.
        let playfield = PlayfieldBounds::from_normalized_local_size(
            map_w,
            local_rect[0],
            local_rect[1],
            gen_w,
            gen_h,
        );
        let connector_regions: Vec<ConnectorRegion> = regions
            .list
            .iter()
            .map(|r| ConnectorRegion {
                id: r.id,
                level: r.level,
                waterish: r.active,
                cell_count: adjacency::region_cell_count(scratch, r.id),
                neighbours: adjacency::neighbour_ids(scratch, r.id, regions.id_counter),
            })
            .collect();
        for region in &connector_regions {
            let mut ctx = CarveCtx {
                grid: &mut *grid,
                scratch: &mut *scratch,
                ids: inputs.ids,
                blocks: inputs.blocks,
                rng: &mut *rng,
                playfield: &playfield,
            };
            carve_driver::carve_connectors_for_region(
                &mut ctx,
                &connector_regions,
                region,
                inputs.accessibility,
                &mut structures,
                &mut construction_trace,
            );
        }
        // Original Generate598D6E..598D82: after connectors, jitter the
        // ridge edges, repair/materialize faces, then restamp registered
        // cliff variants. Generate ignores the materializer's refusal result
        // and still performs the variant pass; partial writes remain live.
        cliff_materialization::jitter(grid, scratch, inputs.ids, rng);
        let _ =
            cliff_materialization::materialize(grid, scratch, inputs.ids, inputs.blocks, rng, -1);
        cliff_variants::run(
            grid,
            scratch,
            inputs.blocks,
            rng,
            inputs.ids.cliffs.cliff_set.map_or(-1, i32::from),
        );
    }
    observe(Stage::IslandPasses, grid, &waypoints, &terrain);

    // ---- Green spread + first recalc --------------------------------------
    green_spread::run(grid, inputs.ids, rng);
    observe(Stage::GreenSpread, grid, &waypoints, &terrain);
    lat_fixup::run(grid, inputs.ids);
    observe(Stage::RecalcAfterTerrain, grid, &waypoints, &terrain);

    // ---- Starts (needs the zone field) ------------------------------------
    let outcome = {
        let zone_params = ZoneParams {
            map_w,
            local_rect,
            wheel_impassable: inputs.wheel_impassable,
        };
        let zones = zones::compute(
            grid,
            inputs.blocks,
            &zone_params,
            ZoneKind::for_map_type(inputs.map_type),
        );
        let args = StartsArgs {
            map_type: inputs.map_type,
            // SyncOptionsFromControls596C70 writes NumPlayers to global
            // MapSeed+50 (ABE028) at596DE9. Generate598960 uses that same
            // record (596644..59664C); CreateStartingPoints594B50 reads its
            // quota at594DF0, and SelectStartCandidates594F40 divides by it
            // at594F7D. Native's explicit4 stores (7A2494/596018) belong to
            // separate WOL territory setup, not ordinary dialog generation.
            start_quota: inputs.num_players,
            num_players: inputs.num_players,
            tiberium_layout: inputs.tiberium_layout,
            gen_w,
            gen_h,
            map_w,
        };
        let mut ctx = StartsCtx {
            grid: &mut *grid,
            scratch: &mut *scratch,
            ids: inputs.ids,
            rng: &mut *rng,
            regions: &mut regions,
        };
        starts::run(&mut ctx, &args, &zones)
    };
    // Captured before `outcome` moves into the tiberium phase, which is also
    // the phase that turns each unfilled slot into a field seeded at the map
    // origin.
    let unfilled_start_slots = outcome.unfilled_start_slots;
    waypoints = outcome
        .waypoints
        .iter()
        .enumerate()
        .filter_map(|(slot, spot)| spot.map(|(x, y)| (slot as u8, x as u16, y as u16)))
        .collect();
    observe(Stage::Starts, grid, &waypoints, &terrain);

    // ---- Tech buildings (map types 1-4; map type 0 places none) -----------
    let tech_placements = {
        let args = TechArgs {
            map_type: inputs.map_type,
            map_w,
            local_rect,
            stride: stride as i32,
        };
        let mut ctx = TechCtx {
            grid: &mut *grid,
            scratch: &mut *scratch,
            ids: inputs.ids,
            rng: &mut *rng,
            regions: &regions,
            types: inputs.tech_types,
        };
        tech_buildings::run_traced(&mut ctx, &args, &mut construction_trace, structures.len())
    };
    observe(Stage::TechBuildings, grid, &waypoints, &terrain);

    // ---- Tiberium ----------------------------------------------------------
    let tiberium = {
        let args = TiberiumArgs {
            map_type: inputs.map_type,
            resources: inputs.resources,
            tib_option: inputs.tib_option,
            min_tib: inputs.min_tib,
            max_tib: inputs.max_tib,
            map_w,
            local_rect,
        };
        let mut ctx = TiberiumCtx {
            grid: &mut *grid,
            scratch: &mut *scratch,
            ids: inputs.ids,
            rng: &mut *rng,
            gauss: &mut *gauss,
            regions: &regions,
            waypoints: &outcome.waypoints,
        };
        tiberium::run(&mut ctx, &args)
    };
    terrain.extend(tiberium.trees);
    observe(Stage::Tiberium, grid, &waypoints, &terrain);

    // ---- Region reset + second recalc -------------------------------------
    scratch.reset_region_ids();
    observe(Stage::RegionReset, grid, &waypoints, &terrain);
    lat_fixup::run(grid, inputs.ids);
    observe(Stage::RecalcAfterTiberium, grid, &waypoints, &terrain);

    // ---- Hills -------------------------------------------------------------
    {
        let args = HillsArgs {
            ruggedness: inputs.ruggedness,
            map_w,
            map_h,
        };
        let mut ctx = HillsCtx {
            grid: &mut *grid,
            scratch: &mut *scratch,
            ids: inputs.ids,
            gauss: &mut *gauss,
            rng: &mut *rng,
        };
        hills::run(&mut ctx, &args, inputs.morphable);
    }
    observe(Stage::Hills, grid, &waypoints, &terrain);

    // ---- LAT patches → recalc (creates sand-LAT) → trees → rocks ----------
    {
        let mut ctx = PatchCtx {
            grid: &mut *grid,
            scratch: &mut *scratch,
            ids: inputs.ids,
            rng: &mut *rng,
            gauss: &mut *gauss,
        };
        lat_patches::run(&mut ctx, inputs.theater, inputs.vegetation);
    }
    observe(Stage::LatPatches, grid, &waypoints, &terrain);
    lat_fixup::run(grid, inputs.ids);
    observe(Stage::RecalcAfterPatches, grid, &waypoints, &terrain);

    let trees = {
        let args = TreeArgs {
            width: inputs.width,
            vegetation: inputs.vegetation,
            max_trees: inputs.max_trees,
            stride: stride as i32,
        };
        let mut ctx = TreeCtx {
            grid: &mut *grid,
            scratch: &mut *scratch,
            ids: inputs.ids,
            rng: &mut *rng,
            gauss: &mut *gauss,
        };
        trees::run(&mut ctx, &args)
    };
    terrain.extend(trees);
    observe(Stage::Trees, grid, &waypoints, &terrain);

    // Rocks — temperate theater only.
    if inputs.theater == 0 {
        let args = RockArgs {
            map_w,
            map_h,
            stride: stride as i32,
        };
        let mut ctx = RockCtx {
            grid: &mut *grid,
            scratch: &mut *scratch,
            ids: inputs.ids,
            rng: &mut *rng,
        };
        rocks::run(&mut ctx, &args);
    }
    observe(Stage::Rocks, grid, &waypoints, &terrain);

    // ---- Final recalc ------------------------------------------------------
    lat_fixup::run(grid, inputs.ids);
    observe(Stage::RecalcFinal, grid, &waypoints, &terrain);

    // ---- Collect outputs for the emitter ----------------------------------
    structures.extend(
        tech_placements
            .into_iter()
            .map(|placement| (placement.name, placement.x, placement.y)),
    );

    PipelineOutput {
        waypoints,
        terrain,
        structures,
        construction_trace,
        unfilled_start_slots,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::rmg::options::RmgOptions;
    use crate::map::rmg::phases::shore::{SubTile, TileBlock};
    use crate::map::rmg::{RmgConstructionOutcome, RmgConstructionPhase};
    use crate::map::theater::TheaterCliffRanges;

    /// A `TileBlocks` provider that returns a 1x1 block for every tile — the
    /// same synthetic the water/shore phase tests use.
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
        // Distinct, resolvable bases per ground/LAT group; realistic spans.
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

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct ConstructionSnapshot {
        trace: RmgConstructionTrace,
        structures: Vec<(String, i16, i16)>,
        entities: Vec<(String, u16, u16)>,
        continuation: [u32; 4],
    }

    /// Run the actual active owners in their native phase order on a small,
    /// controlled map: the flood-region connector driver, neutral-tech driver,
    /// final structure append, and MapFile emitter.
    fn controlled_mixed_construction(seed: u16) -> ConstructionSnapshot {
        const STRIDE: usize = 77;
        const DIAMOND_MIN: i32 = 34;
        const DIAMOND_MAX: i32 = 118;

        let mut grid = RmgGrid::new(STRIDE, DIAMOND_MIN, DIAMOND_MAX);
        let mut scratch = RmgScratch::new(STRIDE, DIAMOND_MIN, DIAMOND_MAX);
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            let cell = grid.get_mut(x, y).expect("native construction cell");
            cell.tile = 0;
            cell.level = 4;
            cell.overlay = -1;
        }
        for cell in scratch.cells_mut() {
            cell.region = -1;
        }

        // A six-cell EW water span commits on the first accepted seed. Only
        // (50,50) belongs to the flood, so the real rejection picker and its
        // exact draw ownership remain in the path without a costly full-map
        // generation search.
        for y in 49..=51 {
            for x in 48..=52 {
                grid.get_mut(x, y).expect("water corridor").tile = 500;
            }
        }
        scratch.get_mut(50, 50).region = 0;
        scratch.get_mut(47, 49).region = 1;
        scratch.get_mut(53, 49).region = 2;

        let flood = ConnectorRegion {
            id: 0,
            level: 4,
            waterish: true,
            cell_count: 90,
            neighbours: vec![1, 2],
        };
        let connector_regions = vec![
            flood.clone(),
            ConnectorRegion {
                id: 1,
                level: 4,
                waterish: false,
                cell_count: 90,
                neighbours: vec![0, 9],
            },
            ConnectorRegion {
                id: 2,
                level: 4,
                waterish: false,
                cell_count: 90,
                neighbours: vec![0, 9],
            },
        ];

        let mut identity = ids();
        identity.pave = 600;
        identity.misc_pave = 620;
        identity.paved_roads = 640;
        identity.paved_road_ends = 660;
        let blocks = one_by_one();
        let playfield = PlayfieldBounds::from_normalized_local_size(74, 2, 5, 70, 70);
        let mut rng = RmgRng::new(seed);
        let mut structures = Vec::new();
        let mut trace = RmgConstructionTrace::default();
        {
            let mut ctx = CarveCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                blocks: &blocks,
                rng: &mut rng,
                playfield: &playfield,
            };
            assert!(carve_driver::carve_connectors_for_region(
                &mut ctx,
                &connector_regions,
                &flood,
                0,
                &mut structures,
                &mut trace,
            ));
        }

        let types = [
            TechType {
                name: "CAEMIT".to_string(),
                footprint: vec![(0, 0)],
            },
            TechType {
                name: "CADROP".to_string(),
                footprint: vec![(0, 0), (1000, 1000)],
            },
        ];
        let empty_regions = Regions::default();
        let args = TechArgs {
            map_type: 3,
            map_w: 74,
            local_rect: [2, 5, 70, 70],
            stride: STRIDE as i32,
        };
        let tech_placements = {
            let mut ctx = TechCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                rng: &mut rng,
                regions: &empty_regions,
                types: &types,
            };
            tech_buildings::run_traced(&mut ctx, &args, &mut trace, structures.len())
        };
        structures.extend(
            tech_placements
                .into_iter()
                .map(|placement| (placement.name, placement.x, placement.y)),
        );

        let options = RmgOptions {
            map_type: 3,
            seed: i32::from(seed),
            ..Default::default()
        };
        let mut map_file = super::super::emit::empty_map_file(&options, 70, 70);
        super::super::emit::populate(&mut map_file, &grid, &[], &structures, &[]);
        let entities = map_file
            .entities
            .into_iter()
            .map(|entity| (entity.type_id, entity.cell_x, entity.cell_y))
            .collect();
        let continuation = [
            rng.next_u32(),
            rng.next_u32(),
            rng.next_u32(),
            rng.next_u32(),
        ];

        ConstructionSnapshot {
            trace,
            structures,
            entities,
            continuation,
        }
    }

    #[test]
    fn production_trace_orders_huts_before_mixed_tech_and_binds_final_entities() {
        let expected = ConstructionSnapshot {
            trace: RmgConstructionTrace {
                lighting: None,
                events: vec![
                    crate::map::rmg::RmgConstructionEvent {
                        ordinal: 0,
                        phase: RmgConstructionPhase::BridgeRepairHut,
                        techno_type: "CABHUT".to_string(),
                        outcome: RmgConstructionOutcome::Emitted {
                            entity_index: 0,
                            cell: (47, 48),
                        },
                    },
                    crate::map::rmg::RmgConstructionEvent {
                        ordinal: 1,
                        phase: RmgConstructionPhase::BridgeRepairHut,
                        techno_type: "CABHUT".to_string(),
                        outcome: RmgConstructionOutcome::Emitted {
                            entity_index: 1,
                            cell: (52, 48),
                        },
                    },
                    crate::map::rmg::RmgConstructionEvent {
                        ordinal: 2,
                        phase: RmgConstructionPhase::NeutralTech,
                        techno_type: "CAEMIT".to_string(),
                        outcome: RmgConstructionOutcome::Emitted {
                            entity_index: 2,
                            cell: (57, 52),
                        },
                    },
                    crate::map::rmg::RmgConstructionEvent {
                        ordinal: 3,
                        phase: RmgConstructionPhase::NeutralTech,
                        techno_type: "CAEMIT".to_string(),
                        outcome: RmgConstructionOutcome::Emitted {
                            entity_index: 3,
                            cell: (57, 38),
                        },
                    },
                    crate::map::rmg::RmgConstructionEvent {
                        ordinal: 4,
                        phase: RmgConstructionPhase::NeutralTech,
                        techno_type: "CADROP".to_string(),
                        outcome: RmgConstructionOutcome::Discarded,
                    },
                ],
            },
            structures: vec![
                ("CABHUT".to_string(), 47, 48),
                ("CABHUT".to_string(), 52, 48),
                ("CAEMIT".to_string(), 57, 52),
                ("CAEMIT".to_string(), 57, 38),
            ],
            entities: vec![
                ("CABHUT".to_string(), 47, 48),
                ("CABHUT".to_string(), 52, 48),
                ("CAEMIT".to_string(), 57, 52),
                ("CAEMIT".to_string(), 57, 38),
            ],
            continuation: [1_708_357_426, 3_341_608_727, 3_980_574_297, 3_095_648_970],
        };

        let first = controlled_mixed_construction(4);
        let repeat = controlled_mixed_construction(4);
        assert_eq!(first, expected, "fixed native-order production oracle");
        assert_eq!(repeat, expected, "the full owner sequence is deterministic");

        let emitted_structures = first
            .structures
            .iter()
            .map(|(name, x, y)| (name.clone(), *x as u16, *y as u16))
            .collect::<Vec<_>>();
        assert_eq!(
            emitted_structures, first.entities,
            "emission preserves order"
        );
        for (ordinal, event) in first.trace.events.iter().enumerate() {
            assert_eq!(event.ordinal, ordinal);
            match &event.outcome {
                RmgConstructionOutcome::Emitted { entity_index, cell } => {
                    let entity = &first.entities[*entity_index];
                    assert_eq!(entity.0, event.techno_type);
                    assert_eq!((entity.1, entity.2), *cell);
                }
                RmgConstructionOutcome::Discarded => {
                    assert_eq!(event.techno_type, "CADROP");
                    assert!(
                        !first.entities.iter().any(|entity| entity.0 == "CADROP"),
                        "discarded construction has no fabricated MapEntity binding"
                    );
                }
            }
        }
        assert!(
            first.trace.events[..2]
                .iter()
                .all(|event| event.phase == RmgConstructionPhase::BridgeRepairHut)
        );
        assert!(
            first.trace.events[2..]
                .iter()
                .all(|event| event.phase == RmgConstructionPhase::NeutralTech)
        );
    }
}
