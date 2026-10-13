//! Process-persistent terrain-TMP variant selection.
//!
//! The cache belongs to the app process, while each map load supplies the raw
//! Main-stream draw callback. This keeps map asset resolution independent of
//! simulation RNG ownership and preserves the original one-time table lifetime.

use std::sync::{Arc, OnceLock};

const TABLE_SIDE: usize = 8;
const TABLE_LEN: usize = TABLE_SIDE * TABLE_SIDE;
const MAX_CANDIDATE_DRAWS: usize = 64;

const FIXED_TABLE_4: [u8; 16] = [
    0, 1, 2, 3, //
    3, 2, 1, 0, //
    2, 3, 0, 1, //
    1, 0, 3, 2,
];

const MOORE_NEIGHBORS: [(i32, i32); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// Process-lifetime owner of the lazily generated 8x8 selector table.
#[derive(Debug, Default)]
pub struct TileVariantSelectorCache {
    selector: TileVariantSelector,
    /// `g_ClearTile`/`g_WaterSet` survive scenario loads. Both executable BSS
    /// values start at zero; each completed theater registry load publishes
    /// values for a later map's pre-IsoMapPack Fill pass.
    cached_clear_tile_base: i32,
    cached_water_set_base: i32,
}

/// Shared identity of the native process table at89E620/initialized89E7C7.
/// Catalog clones retain this handle; the table is initialized only by the
/// existing4814F0 selector, using the Main callback of its current caller.
#[derive(Debug, Default, Clone)]
pub(crate) struct TileVariantSelector {
    table8: Arc<OnceLock<[u8; TABLE_LEN]>>,
}

impl TileVariantSelector {
    fn from_table(table: Option<[u8; TABLE_LEN]>) -> Self {
        let selector = Self::default();
        if let Some(table) = table {
            selector.table8.set(table).expect("new selector is empty");
        }
        selector
    }

    #[cfg(test)]
    pub(crate) fn from_initialized(table: [u8; TABLE_LEN]) -> Self {
        Self::from_table(Some(table))
    }

    pub(crate) fn initialized_table(&self) -> Option<[u8; TABLE_LEN]> {
        self.table8.get().copied()
    }

    pub(crate) fn select_variant(
        &self,
        cell: (i32, i32),
        sub_tile: u8,
        dimensions: (u32, u32),
        total_file_count: u8,
        raw_draw: &mut dyn FnMut() -> u32,
    ) -> u8 {
        let table = self.table8.get_or_init(|| generate_table(raw_draw));
        select_from_initialized_table(
            table,
            cell.0,
            cell.1,
            sub_tile,
            dimensions.0,
            dimensions.1,
            total_file_count,
        )
    }
}

impl TileVariantSelectorCache {
    /// Prepare a saved-load candidate without changing the live process
    /// table. Its matching Main cursor is committed with this candidate.
    pub(crate) fn detached(&self) -> Self {
        Self {
            selector: TileVariantSelector::from_table(self.selector.initialized_table()),
            cached_clear_tile_base: self.cached_clear_tile_base,
            cached_water_set_base: self.cached_water_set_base,
        }
    }

    /// Adopt a successful candidate into the retained process identity.
    /// Divergence means another load initialized the table after preparation;
    /// its Main continuation cannot be silently paired with this candidate.
    pub(crate) fn adopt_prepared(&mut self, prepared: Self) -> Result<(), &'static str> {
        match (
            self.selector.initialized_table(),
            prepared.selector.initialized_table(),
        ) {
            (Some(current), Some(candidate)) if current == candidate => {}
            (None, Some(candidate)) => self
                .selector
                .table8
                .set(candidate)
                .map_err(|_| "terrain selector changed during saved-load commit")?,
            (None, None) => {}
            _ => return Err("terrain selector diverged after saved-load preparation"),
        }
        self.cached_clear_tile_base = prepared.cached_clear_tile_base;
        self.cached_water_set_base = prepared.cached_water_set_base;
        Ok(())
    }

    pub(crate) fn selector(&self) -> TileVariantSelector {
        self.selector.clone()
    }

    #[cfg(test)]
    pub(crate) fn initialized_for_test(table: [u8; TABLE_LEN]) -> Self {
        Self {
            selector: TileVariantSelector::from_initialized(table),
            ..Self::default()
        }
    }

    /// Start one scenario load without initializing the table yet.
    pub fn begin_load<'cache, 'draw>(
        &'cache mut self,
        raw_draw: &'draw mut dyn FnMut() -> u32,
    ) -> TileVariantSelectionContext<'cache, 'draw> {
        let clear_fill_tile_base = self.cached_clear_tile_base;
        let water_fill_tile_base = self.cached_water_set_base;
        TileVariantSelectionContext {
            cache: self,
            raw_draw,
            clear_fill_tile_base,
            water_fill_tile_base,
            generated_table: false,
            map_fill_scenario_advance_count: 0,
            raw_draw_count: 0,
        }
    }

    /// Publish the current theater registry after this map's Fill pass. `None`
    /// maps to the native unresolved/reset sentinel `-1`.
    pub(crate) fn complete_theater_registry_load(
        &mut self,
        clear_tile: Option<u16>,
        water_set: Option<u16>,
    ) {
        self.cached_clear_tile_base = clear_tile.map_or(-1, i32::from);
        self.cached_water_set_base = water_set.map_or(-1, i32::from);
    }

    #[cfg(test)]
    pub(crate) fn cached_clear_tile_base(&self) -> i32 {
        self.cached_clear_tile_base
    }

    #[cfg(test)]
    pub(crate) fn cached_water_set_base(&self) -> i32 {
        self.cached_water_set_base
    }

    pub fn is_initialized(&self) -> bool {
        self.selector.table8.get().is_some()
    }
}

/// Per-load selector state. The supplied callback draws from the Main cursor
/// that owns this load (front-end Main for RMG preview, reseeded Main for an
/// ordinary match); Scenario randomness never enters here.
pub struct TileVariantSelectionContext<'cache, 'draw> {
    cache: &'cache mut TileVariantSelectorCache,
    raw_draw: &'draw mut dyn FnMut() -> u32,
    clear_fill_tile_base: i32,
    water_fill_tile_base: i32,
    generated_table: bool,
    map_fill_scenario_advance_count: usize,
    raw_draw_count: usize,
}

impl TileVariantSelectionContext<'_, '_> {
    /// Resolve one pre-IsoMapPack Fill cell. The callback owns Scenario RNG:
    /// Water calls `RandomRanged(0,3)` and advances once; every other value
    /// calls equal-bound `RandomRanged(0,0)` and leaves the cursor unchanged.
    pub(crate) fn draw_fill_tile_index(
        &mut self,
        is_water: bool,
        scenario_ranged_draw: &mut dyn FnMut(u32, u32) -> u32,
    ) -> i32 {
        let (base, high) = if is_water {
            self.map_fill_scenario_advance_count += 1;
            (self.water_fill_tile_base, 3)
        } else {
            (self.clear_fill_tile_base, 0)
        };
        base + scenario_ranged_draw(0, high) as i32
    }

    /// Select the final file index (`0` pristine, `1..` suffix siblings).
    ///
    /// Table generation happens before inspecting `total_file_count`, including
    /// the fixed-table path. The returned native table value is then wrapped by
    /// the immediate map-owner `% total_file_count` step.
    pub fn select_variant(
        &mut self,
        cell_x: i32,
        cell_y: i32,
        sub_tile: u8,
        template_width: u32,
        template_height: u32,
        total_file_count: u8,
    ) -> u8 {
        let was_initialized = self.cache.is_initialized();
        let selected = self.cache.selector.select_variant(
            (cell_x, cell_y),
            sub_tile,
            (template_width, template_height),
            total_file_count,
            &mut || {
                self.raw_draw_count += 1;
                (self.raw_draw)()
            },
        );
        self.generated_table |= !was_initialized;
        selected
    }

    /// Copy the existing process table without initializing it or drawing RNG.
    #[cfg(test)]
    pub(crate) fn initialized_table(&self) -> Option<[u8; TABLE_LEN]> {
        self.cache.selector.initialized_table()
    }

    pub(crate) fn selector(&self) -> TileVariantSelector {
        self.cache.selector()
    }

    /// True only for the load that performed the process-global table build.
    pub fn generated_table(&self) -> bool {
        self.generated_table
    }

    /// Scenario cursor advances caused by `[Map] Fill=Water` before variant
    /// selection. Equal-bound non-Water calls deliberately do not count.
    pub fn map_fill_scenario_advance_count(&self) -> usize {
        self.map_fill_scenario_advance_count
    }

    /// Number of raw Main draws consumed by this load's table construction.
    pub fn raw_draw_count(&self) -> usize {
        self.raw_draw_count
    }
}

/// Original48150F..481570: one process table, native Moore-neighbor order and
/// at most64 Main candidates per slot. Loading and runtime share this body.
fn generate_table(raw_draw: &mut dyn FnMut() -> u32) -> [u8; TABLE_LEN] {
    let mut table = [0u8; TABLE_LEN];
    let mut filled = [false; TABLE_LEN];
    for cell_index in 0..TABLE_LEN {
        for attempt in 0..MAX_CANDIDATE_DRAWS {
            let candidate = raw_draw() as u8 & 7;
            let collides = MOORE_NEIGHBORS.iter().any(|&(dx, dy)| {
                let mut neighbor = cell_index as i32 + dy * TABLE_SIDE as i32 + dx;
                if neighbor < 0 {
                    neighbor += TABLE_LEN as i32;
                } else if neighbor >= TABLE_LEN as i32 {
                    neighbor -= TABLE_LEN as i32;
                }
                let neighbor = neighbor as usize;
                filled[neighbor] && table[neighbor] == candidate
            });
            if !collides || attempt + 1 == MAX_CANDIDATE_DRAWS {
                table[cell_index] = candidate;
                filled[cell_index] = true;
                break;
            }
        }
    }
    table
}

/// Native4814F0 coordinate arithmetic after one-time process initialization.
/// Loading and runtime terrain presentation share this operation and table.
pub(crate) fn select_from_initialized_table(
    table8: &[u8; TABLE_LEN],
    cell_x: i32,
    cell_y: i32,
    sub_tile: u8,
    template_width: u32,
    template_height: u32,
    total_file_count: u8,
) -> u8 {
    let Ok(template_width) = i32::try_from(template_width) else {
        return 0;
    };
    let Ok(template_height) = i32::try_from(template_height) else {
        return 0;
    };
    if total_file_count == 0 || template_width == 0 || template_height == 0 {
        // Retail inputs never reach this malformed boundary. Safe Rust keeps
        // the pristine image instead of reproducing native divide-by-zero.
        return 0;
    }

    let (template_x, template_y) = if sub_tile == 0 {
        (i32::from(cell_x as i16), i32::from(cell_y as i16))
    } else {
        let sub_tile = i32::from(sub_tile);
        (
            i32::from(cell_x.wrapping_sub(sub_tile % template_width) as i16) / template_width,
            i32::from(cell_y.wrapping_sub(sub_tile / template_width) as i16) / template_height,
        )
    };
    let raw = if total_file_count <= 4 {
        let x = (template_x & 3) as usize;
        let y = (template_y & 3) as usize;
        FIXED_TABLE_4[y * 4 + x]
    } else {
        let x = (template_x & 7) as usize;
        let y = (template_y & 7) as usize;
        table8[y * TABLE_SIDE + x]
    };
    raw % total_file_count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::rng::SimRng;

    fn patterned_cache() -> TileVariantSelectorCache {
        let mut table = [0u8; TABLE_LEN];
        for (index, value) in table.iter_mut().enumerate() {
            *value = (index & 7) as u8;
        }
        TileVariantSelectorCache {
            selector: TileVariantSelector::from_initialized(table),
            ..TileVariantSelectorCache::default()
        }
    }

    #[test]
    fn rejected_saved_candidate_leaves_retained_selector_cold() {
        let live = TileVariantSelectorCache::default();
        let retained = live.selector();
        let mut candidate = live.detached();
        let mut main = SimRng::new(0);
        {
            let mut draw = || main.next_u32();
            candidate
                .begin_load(&mut draw)
                .select_variant(1, 2, 0, 1, 1, 8);
        }
        assert!(candidate.is_initialized());
        assert!(!live.is_initialized());
        assert!(retained.initialized_table().is_none());
        drop(candidate);
        assert!(!live.is_initialized());
    }

    #[test]
    fn successful_saved_candidate_adopts_into_existing_selector_identity() {
        let mut live = TileVariantSelectorCache::default();
        let retained = live.selector();
        let mut candidate = live.detached();
        candidate.complete_theater_registry_load(Some(23), Some(41));
        let mut main = SimRng::new(0);
        {
            let mut draw = || main.next_u32();
            candidate
                .begin_load(&mut draw)
                .select_variant(1, 2, 0, 1, 1, 8);
        }
        let expected = candidate.selector.initialized_table();
        live.adopt_prepared(candidate).unwrap();
        assert_eq!(retained.initialized_table(), expected);
        assert_eq!(live.cached_clear_tile_base(), 23);
        assert_eq!(live.cached_water_set_base(), 41);
        let mut no_draw = || panic!("adopted process selector is warm");
        let mut next_load = live.begin_load(&mut no_draw);
        assert_eq!(next_load.select_variant(1, 2, 0, 1, 1, 8), 4);
        assert_eq!(next_load.raw_draw_count(), 0);
    }

    #[test]
    fn saved_candidate_cannot_overwrite_a_diverged_process_selector() {
        let mut live = TileVariantSelectorCache::default();
        let candidate = live.detached();
        let mut main = SimRng::new(7);
        {
            let mut draw = || main.next_u32();
            live.begin_load(&mut draw).select_variant(1, 2, 0, 1, 1, 8);
        }
        let before = live.selector.initialized_table();
        assert!(live.adopt_prepared(candidate).is_err());
        assert_eq!(live.selector.initialized_table(), before);
    }

    #[test]
    fn gsi_02_11_counts_and_downstream_modulo_match_native_tables() {
        let mut cache = patterned_cache();
        let mut forbidden_draw = || panic!("initialized cache must not draw");
        let mut selector = cache.begin_load(&mut forbidden_draw);

        assert_eq!(selector.select_variant(3, 0, 0, 1, 1, 2), 1);
        assert_eq!(selector.select_variant(3, 0, 0, 1, 1, 4), 3);
        assert_eq!(selector.select_variant(7, 0, 0, 1, 1, 5), 2);
        assert_eq!(selector.select_variant(7, 0, 0, 1, 1, 8), 7);
        assert_eq!(selector.raw_draw_count(), 0);
    }

    #[test]
    fn gsi_02_11_multisubtile_cells_share_the_template_anchor() {
        let mut cache = patterned_cache();
        let mut forbidden_draw = || panic!("initialized cache must not draw");
        let mut selector = cache.begin_load(&mut forbidden_draw);

        let anchor = selector.select_variant(3, 6, 0, 3, 2, 4);
        let fifth_subtile = selector.select_variant(11, 13, 5, 3, 2, 4);
        assert_eq!(fifth_subtile, anchor);
        assert_eq!(anchor, 1);

        // Subtile zero bypasses normalization entirely: dividing (3,2) by
        // (3,2) would incorrectly select fixed-table value 2 instead of 1.
        assert_eq!(selector.select_variant(3, 2, 0, 3, 2, 4), 1);
    }

    #[test]
    fn gsi_04_02_preview_warms_cache_and_fresh_match_reuses_it_without_draws() {
        let mut cache = TileVariantSelectorCache::default();
        let mut first_rng = SimRng::new(0);
        let mut first_draw = || first_rng.next_u32();
        {
            let mut selector = cache.begin_load(&mut first_draw);
            assert_eq!(selector.select_variant(0, 0, 0, 1, 1, 2), 0);
            assert!(selector.generated_table());
            assert_eq!(selector.raw_draw_count(), 128);
        }
        let first_cursor = first_rng.logical_state();
        assert_eq!((first_cursor.index_a, first_cursor.index_b), (128, 231));

        let expected = [
            3, 6, 5, 3, 7, 5, 6, 4, //
            1, 0, 2, 0, 4, 3, 7, 2, //
            7, 4, 6, 3, 5, 2, 5, 4, //
            5, 3, 1, 2, 7, 3, 1, 6, //
            7, 2, 4, 6, 1, 2, 4, 2, //
            4, 1, 7, 5, 7, 6, 3, 6, //
            3, 5, 0, 1, 4, 0, 1, 2, //
            4, 7, 2, 6, 2, 3, 7, 5,
        ];
        assert_eq!(cache.selector.initialized_table(), Some(expected));
        assert!(
            cache.is_initialized(),
            "closing or discarding the preview owner must not clear it"
        );

        // A later match creates a fresh Main cursor, but the process-owned
        // selector cache survives the preview/dialog lifetime.
        let mut second_rng = SimRng::new(0xA55A_1234);
        let untouched = second_rng.state();
        let mut second_draw = || second_rng.next_u32();
        {
            let mut selector = cache.begin_load(&mut second_draw);
            assert_eq!(selector.select_variant(0, 0, 0, 1, 1, 8), 3);
            assert!(!selector.generated_table());
            assert_eq!(selector.raw_draw_count(), 0);
        }
        assert_eq!(second_rng.state(), untouched);

        // A later Water fill advances Scenario once while reuse of the warm
        // selector table leaves the separately seeded Main cursor untouched.
        let mut water_scenario_rng = SimRng::new(0xCAFE_BABE);
        let mut expected_scenario_rng = water_scenario_rng.clone();
        let expected_tile = expected_scenario_rng.next_range_u32_inclusive(0, 3) as i32;
        let mut scenario_ranged =
            |low, high| water_scenario_rng.next_range_u32_inclusive(low, high);
        let mut water_main_rng = SimRng::new(0xCAFE_BABE);
        let main_before = water_main_rng.logical_state();
        let mut main_draw = || water_main_rng.next_u32();
        {
            let mut selector = cache.begin_load(&mut main_draw);
            assert_eq!(
                selector.draw_fill_tile_index(true, &mut scenario_ranged),
                expected_tile
            );
            let _ = selector.select_variant(0, 0, 0, 1, 1, 8);
            assert_eq!(selector.map_fill_scenario_advance_count(), 1);
            assert_eq!(selector.raw_draw_count(), 0);
        }
        assert_eq!(
            water_scenario_rng.logical_state(),
            expected_scenario_rng.logical_state()
        );
        assert_eq!(water_main_rng.logical_state(), main_before);
    }
}
