//! The generated launch's final `MapClass::InitCellAttributes(1)` ore-density rewrite.
//!
//! Depends on `map::authored_overlay` (native cell-iterator shape),
//! `map::cell_index`, `map::resolved_terrain`, `rules` (including `rules::overlay_types`),
//! `map::tiberium_cell`, and `sim::overlay_grid`; never on
//! render/, ui/, app/, audio/, or net/.

use crate::map::authored_overlay::NativeOverlayMapShape;
use crate::map::cell_index::canonical_cell_coord;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::tiberium_cell::{GerminatedCell, spread_cell_germinate_without_randomization};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::tiberium_type::TiberiumTypeRegistry;
use crate::sim::overlay_grid::OverlayGrid;

/// Logging/test receipt of one generated final `InitCellAttributes(1)` pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GeneratedCellAttributesReceipt {
    /// Real cells the native `CellIterator` visited.
    pub real_cells: u32,
    /// Iterator coordinates with no allocated cell (never expected on a
    /// generated map; logged so a shape mismatch cannot pass silently).
    pub unallocated_cells: u32,
    /// Real cells whose overlay resolved to a TiberiumClass and whose density
    /// was rewritten.
    pub germinated_cells: u32,
    /// The native caller-local wrapping sum of every `SpreadCellGerminate`
    /// return; `RandomMapGenerator::Generate` discards it.
    pub tiberium_value_total: i32,
}

/// Resolve one native fixed-stride lookup to an allocated real cell of the
/// generated grid; `None` is a `Get_CellClass` miss (the shared dummy).
fn resolve_real_cell(terrain: &ResolvedTerrainGrid, x: i16, y: i16) -> Option<(u16, u16)> {
    terrain
        .native_fixed_cell_index(x, y)
        .and_then(|_| canonical_cell_coord(i32::from(x), i32::from(y)))
}

/// Generated launch tail of `RandomMapGenerator::Generate @ 0x00598960`.
///
/// gamemd-derived: after the generator constructors, its final whole-map
/// `RecalcAttributes(-1)` loop (`0x0059937D`), and the growth-then-spread
/// queue initialization (`TiberiumClass::InitGrowthQueues_All @ 0x00722D00`,
/// `InitSpreadQueues_All @ 0x00722240`), `Generate` calls
/// `MapClass::InitCellAttributes(1) @ 0x00568BB0` (`push 1` at `0x0059943F`,
/// call at `0x0059944C`). For every real cell in `CellIterator` order that
/// pass calls `SpreadCellGerminate(0)` before the cell's own
/// `RecalcAttributes(-1)` and adds the return to a caller-local wrapping
/// total; the return is not stored (the `MapClass+0x134` store belongs to
/// `Full_Init`'s argument-0 call only) and the already initialized queues are
/// not rebuilt.
///
/// The per-cell Recalc after the rewrite is not repeated here:
/// `CellClass::RecalcAttributes @ 0x0047D2B0` never reads `+0x11E`, and the
/// identities the germination reads are already final after the generator's
/// whole-map Recalc, so no attribute changes. The pass's terrain-Anim
/// scalar-delete/recreate is the eager tile-anim set (native ID chronology
/// remains the G10 phase-journal residual). Germination reads only overlay
/// identity and writes only the receiver's density, so its result does not
/// depend on the visiting order; the order still fixes which missing
/// neighbour the shared dummy's coordinate retains last. The density lives in
/// the overlay grid alone, as every runtime density write does.
pub fn run_generated_final_cell_attributes(
    terrain: &ResolvedTerrainGrid,
    overlay_grid: &mut OverlayGrid,
    tiberium_types: &TiberiumTypeRegistry,
    overlay_registry: &OverlayTypeRegistry,
    map_width: u16,
    map_height: u16,
) -> GeneratedCellAttributesReceipt {
    let dummy = terrain.shared_cell_dummy();
    let mut receipt = GeneratedCellAttributesReceipt::default();
    let shape = NativeOverlayMapShape::new(i32::from(map_width), i32::from(map_height));
    for (x, y) in shape.recalc_cells() {
        let Some((rx, ry)) = resolve_real_cell(terrain, x, y) else {
            receipt.unallocated_cells += 1;
            continue;
        };
        receipt.real_cells += 1;
        let receiver_overlay_id = overlay_grid.cell(rx, ry).overlay_id;
        let germinated = {
            let overlay_grid: &OverlayGrid = overlay_grid;
            spread_cell_germinate_without_randomization(
                tiberium_types,
                overlay_registry,
                receiver_overlay_id,
                (x, y),
                |(nx, ny)| match resolve_real_cell(terrain, nx, ny) {
                    Some((nrx, nry)) => {
                        let neighbor = overlay_grid.cell(nrx, nry);
                        (neighbor.overlay_id, neighbor.overlay_data)
                    }
                    None => {
                        dummy.stamp_coord(i32::from(nx), i32::from(ny));
                        dummy.overlay_fields()
                    }
                },
            )
        };
        let Some(GerminatedCell { density, value }) = germinated else {
            continue;
        };
        // Direct `CellClass+0x11E` write: no runtime dirtiness on a fresh load.
        overlay_grid.cell_mut(rx, ry).overlay_data = density;
        receipt.germinated_cells += 1;
        receipt.tiberium_value_total = receipt.tiberium_value_total.wrapping_add(value);
    }
    receipt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::tiberium_cell::ORE_DENSITY_BY_NEIGHBOR_COUNT;

    use crate::map::basic::{BasicSection, SpecialFlagsSection};
    use crate::map::resolved_terrain::ResolvedTerrainCell;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::world::Simulation;

    /// Storage covering the whole `(8, 8)` native diamond (`x`, `y` in
    /// `1..=15`).
    const STORAGE: u16 = 16;
    const MAP_WIDTH: u16 = 8;
    const MAP_HEIGHT: u16 = 8;

    fn flat_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
        ResolvedTerrainCell {
            speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
            base_speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
            ..crate::map::resolved_terrain::test_tiberium_cell(rx, ry)
        }
    }

    fn flat_terrain() -> ResolvedTerrainGrid {
        crate::map::resolved_terrain::test_grid(STORAGE, STORAGE, flat_cell)
    }

    /// Overlay ids: `ORE` = 0 (outside every native image range, so it falls
    /// back to the first TiberiumClass), `GEM` = 27 (the native Cruentus
    /// `Image=2` range `27..=38`), `ROCK` = 28.
    const ORE: u8 = 0;
    const GEM: u8 = 27;
    const ROCK: u8 = 28;

    fn two_class_rules() -> (RuleSet, OverlayTypeRegistry) {
        let mut ini = String::from(
            "[General]\nTiberiumGrows=yes\nTiberiumSpreads=yes\n\
             [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [Tiberiums]\n0=Riparius\n1=Cruentus\n\
             [Riparius]\nImage=1\nValue=25\nGrowth=2200\nGrowthPercentage=.06\n\
             Spread=2200\nSpreadPercentage=.06\n\
             [Cruentus]\nImage=2\nValue=50\nGrowth=2200\nGrowthPercentage=.06\n\
             Spread=2200\nSpreadPercentage=.06\n\
             [OverlayTypes]\n0=ORE\n",
        );
        for index in 1..GEM {
            ini.push_str(&format!("{index}=FILL{index}\n"));
        }
        ini.push_str(&format!(
            "{GEM}=GEM\n{ROCK}=ROCK\n[ORE]\nTiberium=yes\n[GEM]\nTiberium=yes\n[ROCK]\nIsARock=yes\n"
        ));
        let ini = IniFile::from_str(&ini);
        let rules = RuleSet::from_ini(&ini).expect("germination rules");
        let registry = OverlayTypeRegistry::from_ini(&ini, None);
        assert_eq!(
            registry
                .tiberium_type_for_overlay(&rules.tiberium_types, GEM)
                .map(|id| id.0),
            Some(1),
            "the gem overlay must resolve to the second TiberiumClass"
        );
        (rules, registry)
    }

    /// One 4x4 ore field inside the diamond, a second-class gem beside it, a
    /// rock beside that, and an isolated ore cell.
    fn painted_grid() -> OverlayGrid {
        let mut grid = OverlayGrid::new(STORAGE, STORAGE);
        for ry in 6..=9 {
            for rx in 6..=9 {
                grid.place_overlay(rx, ry, ORE, 5);
            }
        }
        grid.place_overlay(10, 7, GEM, 5);
        grid.place_overlay(10, 8, ROCK, 0);
        grid.place_overlay(12, 12, ORE, 5);
        // The fixture placements are not the pass's runtime dirtiness.
        grid.take_dirty_cells();
        grid
    }

    /// `SpreadCellGerminate(0)`: density from the same-class neighbour count
    /// through `g_OreDensityByNeighborCount`; a different TiberiumClass or a
    /// non-resource overlay never counts; the return sums `(state + 1) * Value`.
    #[test]
    fn generated_pass_rewrites_every_resource_density_from_same_class_neighbours() {
        let (rules, registry) = two_class_rules();
        let terrain = flat_terrain();
        let mut grid = painted_grid();

        let receipt = run_generated_final_cell_attributes(
            &terrain,
            &mut grid,
            &rules.tiberium_types,
            &registry,
            MAP_WIDTH,
            MAP_HEIGHT,
        );

        let mut expected_total = 0i32;
        for ry in 6..=9u16 {
            for rx in 6..=9u16 {
                let on_x_edge = rx == 6 || rx == 9;
                let on_y_edge = ry == 6 || ry == 9;
                let expected = match (on_x_edge, on_y_edge) {
                    (true, true) => 4,
                    (true, false) | (false, true) => 7,
                    (false, false) => 11,
                };
                assert_eq!(
                    grid.cell(rx, ry).overlay_data,
                    expected,
                    "field cell ({rx},{ry})"
                );
                expected_total += (i32::from(expected) + 1) * 25;
            }
        }
        assert_eq!(
            grid.cell(10, 7).overlay_data,
            0,
            "the gem counts no ore neighbours"
        );
        expected_total += 50;
        assert_eq!(grid.cell(10, 8).overlay_data, 0, "the rock is untouched");
        assert_eq!(grid.cell(12, 12).overlay_data, 0, "isolated ore");
        expected_total += 25;
        assert_eq!(
            receipt,
            GeneratedCellAttributesReceipt {
                real_cells: u32::from(MAP_HEIGHT) * (2 * u32::from(MAP_WIDTH) - 1),
                unallocated_cells: 0,
                germinated_cells: 18,
                tiberium_value_total: expected_total,
            }
        );
        assert!(
            grid.take_dirty_cells().is_empty(),
            "a fresh-load density rewrite emits no runtime dirtiness"
        );
    }

    /// A `Get_CellClass` miss stamps the shared dummy and reads its retained
    /// overlay identity, so one same-class dummy identity counts once per
    /// missing direction; the last miss of the last visited receiver is the
    /// coordinate the dummy keeps.
    #[test]
    fn generated_pass_counts_the_shared_dummy_for_missing_neighbours_in_native_order() {
        let (rules, registry) = two_class_rules();
        let terrain = flat_terrain();
        let mut grid = OverlayGrid::new(STORAGE, STORAGE);
        // Last cell of the (8, 8) `CellIterator`: sum 24, x 9..=15 -> (15, 9).
        grid.place_overlay(15, 9, ORE, 5);
        let dummy = terrain.shared_cell_dummy();
        dummy.set_overlay_fields(Some(ORE), 3);

        let receipt = run_generated_final_cell_attributes(
            &terrain,
            &mut grid,
            &rules.tiberium_types,
            &registry,
            MAP_WIDTH,
            MAP_HEIGHT,
        );

        // NE (16,8), E (16,9), SE (16,10) miss the 16-wide storage; the five
        // other neighbours are real, empty cells.
        assert_eq!(
            grid.cell(15, 9).overlay_data,
            ORE_DENSITY_BY_NEIGHBOR_COUNT[3]
        );
        assert_eq!(receipt.germinated_cells, 1);
        assert_eq!(
            receipt.tiberium_value_total,
            (i32::from(ORE_DENSITY_BY_NEIGHBOR_COUNT[3]) + 1) * 25
        );
        assert_eq!(
            terrain.dummy_cell_requested_coord(),
            (16, 10),
            "the SE miss is the last stamp in N, NE, E, SE, S, SW, W, NW order"
        );
        assert_eq!(
            dummy.overlay_fields(),
            (Some(0), 3),
            "germination never writes the dummy's identity or state"
        );
    }

    /// Generator tail order: the growth-then-spread queues are seeded from the
    /// painted densities before `InitCellAttributes(1)` rewrites them, and
    /// nothing rebuilds them afterwards.
    #[test]
    fn generated_queues_are_seeded_before_germination_and_left_alone() {
        let (rules, registry) = two_class_rules();
        let mut sim = Simulation::with_seed(0x0C_0001);
        sim.install_resolved_terrain_for_new_map(flat_terrain());
        let terrain = flat_terrain();
        let mut grid = painted_grid();
        // Painted 0 qualifies for growth (below MaxDensity - 1) and not for
        // spread; the interior germinates to 11, which would invert both.
        grid.cell_mut(7, 7).overlay_data = 0;
        let scenario_before = sim.scenario_rng.state();
        let main_before = sim.main_rng.state();
        let basic = BasicSection::default();
        let special_flags = SpecialFlagsSection::default();

        let stats = crate::sim::runtime::initialize_native_tiberium_queues(
            &mut sim,
            &basic,
            &special_flags,
            &rules,
            &registry,
            Some(&grid),
            (MAP_WIDTH, MAP_HEIGHT),
        )
        .expect("a grid seeds the native queues");
        let queue_bitmaps = |sim: &Simulation| {
            sim.production
                .ore_growth_state
                .native_tiberium_state()
                .classes
                .iter()
                .map(|class| (class.growth_bitmap.clone(), class.spread_bitmap.clone()))
                .collect::<Vec<_>>()
        };
        let bitmaps_before = queue_bitmaps(&sim);
        let receipt = run_generated_final_cell_attributes(
            &terrain,
            &mut grid,
            &rules.tiberium_types,
            &registry,
            MAP_WIDTH,
            MAP_HEIGHT,
        );

        assert_eq!(grid.cell(7, 7).overlay_data, 11);
        assert_eq!(receipt.germinated_cells, 18);
        let (ore_growth, ore_spread) = &bitmaps_before[0];
        assert!(
            ore_growth.contains(&(7, 7)),
            "seeded from the painted density: {stats:?}"
        );
        assert!(!ore_spread.contains(&(7, 7)));
        assert_eq!(
            queue_bitmaps(&sim),
            bitmaps_before,
            "germination rebuilds no queue"
        );
        assert_eq!(sim.scenario_rng.state(), scenario_before);
        assert_eq!(sim.main_rng.state(), main_before);
    }
}
