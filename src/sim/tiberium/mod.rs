//! Shared tiberium cell mutation logic.
//!
//! Owns the Rust equivalents of gamemd's cell-level tiberium view and mutation
//! boundaries. In a loaded YR map, `OverlayGrid` is the authority for both the
//! tiberium type and its raw 0..=11 density byte; there is no second store.

#[cfg(test)]
pub(crate) mod test_support;

use crate::map::bridge_facts::{BRIDGE_FLAG_DESTROYED_OR_RAMP, BRIDGE_FLAG_STRUCTURAL};
use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::rules::ruleset::RuleSet;
use crate::rules::tiberium_type::{TiberiumTypeId, TiberiumTypeRegistry};
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::StringInterner;
use crate::sim::miner::ResourceType;
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::ore_growth::OreGrowthState;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::pathfinding::PathGrid;
use crate::sim::rng::SimRng;

/// Mutable state needed to apply a shared tiberium reduction.
pub struct ReduceTiberiumContext<'a> {
    pub overlay_grid: Option<&'a mut OverlayGrid>,
    pub ore_growth_state: &'a mut OreGrowthState,
    pub overlay_registry: Option<&'a OverlayTypeRegistry>,
    pub tiberium_types: Option<&'a TiberiumTypeRegistry>,
    pub resolved_terrain: Option<&'a mut ResolvedTerrainGrid>,
    pub source_object_cells: Option<&'a std::collections::BTreeSet<(u16, u16)>>,
    /// Live ground object lists for the neighbour reseed's `CanSpreadTiberium`
    /// `FirstObject` gate.
    pub live_objects: Option<NativeCellObjectView<'a>>,
    pub rng: Option<&'a mut SimRng>,
    pub binary_frame: u32,
    pub spread_enabled: bool,
    pub radar_dirty_cells: Option<&'a mut Vec<(u16, u16)>>,
    pub radar_dirty_generation: Option<&'a mut u64>,
    pub tactical_dirty_cells: Option<&'a mut Vec<(u16, u16)>>,
}

/// Overlay-backed resource facts for one live map cell.
///
/// `overlay_data == 0` is still one present tiberium cell. The native search
/// score therefore uses `Value * (OverlayData + 1)` even though reduction's
/// full-removal return value is the raw pre-removal byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TiberiumCellView {
    pub overlay_id: u8,
    pub overlay_data: u8,
    pub tiberium_type: TiberiumTypeId,
    pub resource_type: ResourceType,
    pub nominal_value: i32,
}

/// Resolve one live resource cell exclusively from its overlay bytes and the
/// parsed overlay/tiberium registries.
pub fn tiberium_cell_view(
    overlay_grid: &OverlayGrid,
    overlay_registry: &OverlayTypeRegistry,
    tiberium_types: &TiberiumTypeRegistry,
    cell: (u16, u16),
) -> Option<TiberiumCellView> {
    let overlay = *overlay_grid.cell(cell.0, cell.1);
    let overlay_id = overlay.overlay_id?;
    let tiberium_type = overlay_registry.tiberium_type_for_overlay(tiberium_types, overlay_id)?;
    let ty = tiberium_types.get(tiberium_type)?;
    Some(TiberiumCellView {
        overlay_id,
        overlay_data: overlay.overlay_data,
        tiberium_type,
        resource_type: resource_type_for_tiberium_image(ty.image),
        nominal_value: ty.value.wrapping_mul(i32::from(overlay.overlay_data) + 1),
    })
}

pub fn resource_type_for_tiberium_image(image: u8) -> ResourceType {
    if image == 2 {
        ResourceType::Gem
    } else {
        ResourceType::Ore
    }
}

/// Existing live-object sources needed by the retail visible-building
/// exclusion in `CellClass::CanPlaceTiberium`.
#[derive(Clone, Copy)]
pub struct TiberiumPlacementObjectContext<'a> {
    entities: &'a EntityStore,
    occupancy: &'a OccupancyGrid,
    rules: &'a RuleSet,
    interner: &'a StringInterner,
    /// Live terrain-object cell index (`production.terrain_object_cells`):
    /// the terrain half of the native `FirstObject` list.
    terrain_object_cells: &'a std::collections::BTreeMap<(u16, u16), u64>,
}

impl<'a> TiberiumPlacementObjectContext<'a> {
    pub fn new(
        entities: &'a EntityStore,
        occupancy: &'a OccupancyGrid,
        rules: &'a RuleSet,
        interner: &'a StringInterner,
        terrain_object_cells: &'a std::collections::BTreeMap<(u16, u16), u64>,
    ) -> Self {
        Self {
            entities,
            occupancy,
            rules,
            interner,
            terrain_object_cells,
        }
    }

    /// The ground object-list view of this context.
    pub(crate) fn object_view(&self) -> NativeCellObjectView<'a> {
        NativeCellObjectView::new(self.occupancy, self.terrain_object_cells)
    }
}

/// Read-only view of the CellClass ground object lists for the
/// `CellClass+0xE4 FirstObject` gates.
///
/// gamemd-derived: the `AddContent` wrapper `0x005683C0` (sole caller of
/// `CellClass::AddContent @ 0x0047E8A0`) links every ground-layer object into
/// `FirstObject`: Technos through `TechnoClass::MarkCellLists @ 0x004D37DD`
/// and `BuildingClass @ 0x0043F691`, and every terrain object (spawning or
/// not) through `TerrainClass::Mark @ 0x0071BFF8`. Rust keeps Technos in the
/// occupancy grid and terrain objects in the production cell index, so the
/// view joins both.
#[derive(Clone, Copy)]
pub struct NativeCellObjectView<'a> {
    occupancy: &'a OccupancyGrid,
    terrain_object_cells: &'a std::collections::BTreeMap<(u16, u16), u64>,
}

impl<'a> NativeCellObjectView<'a> {
    pub fn new(
        occupancy: &'a OccupancyGrid,
        terrain_object_cells: &'a std::collections::BTreeMap<(u16, u16), u64>,
    ) -> Self {
        Self {
            occupancy,
            terrain_object_cells,
        }
    }

    /// `CellClass+0xE4 FirstObject != 0`: a terrain object or any member of
    /// the cell's ground object list (the bridge list is `AltObject`).
    pub(crate) fn ground_object_present(&self, cell: (u16, u16)) -> bool {
        self.terrain_object_cells.contains_key(&cell)
            || self.occupancy.count_on_layer(
                cell.0,
                cell.1,
                crate::sim::movement::locomotor::MovementLayer::Ground,
            ) > 0
    }

    /// Every cell whose ground object list is non-empty.
    pub(crate) fn occupied_ground_cells(&self) -> impl Iterator<Item = (u16, u16)> + 'a {
        self.terrain_object_cells.keys().copied().chain(
            self.occupancy
                .occupied_cells_on_layer(crate::sim::movement::locomotor::MovementLayer::Ground),
        )
    }
}

/// Proof that a caller selected an explicit new-cell admission policy.
///
/// Runtime placement can only construct this from both resolved map terrain
/// and the live CellClass-style object view. The crate-private compatibility
/// constructor keeps old non-native fixtures explicit without weakening the
/// production boundary.
#[derive(Clone, Copy)]
pub struct NewTiberiumAdmission<'a> {
    resolved_terrain: Option<&'a ResolvedTerrainGrid>,
    path_grid: Option<&'a PathGrid>,
    live_objects: Option<TiberiumPlacementObjectContext<'a>>,
}

impl<'a> NewTiberiumAdmission<'a> {
    pub fn runtime(
        resolved_terrain: &'a ResolvedTerrainGrid,
        path_grid: Option<&'a PathGrid>,
        live_objects: TiberiumPlacementObjectContext<'a>,
    ) -> Self {
        Self {
            resolved_terrain: Some(resolved_terrain),
            path_grid,
            live_objects: Some(live_objects),
        }
    }

    #[cfg(test)]
    pub(crate) fn compatibility_without_native_context(
        resolved_terrain: Option<&'a ResolvedTerrainGrid>,
        path_grid: Option<&'a PathGrid>,
        live_objects: Option<TiberiumPlacementObjectContext<'a>>,
    ) -> Self {
        Self {
            resolved_terrain,
            path_grid,
            live_objects,
        }
    }

    /// The live CellClass-style object view this admission carries, if any.
    pub(crate) fn live_objects(&self) -> Option<TiberiumPlacementObjectContext<'a>> {
        self.live_objects
    }
}

pub(crate) fn resolved_cell_accepts_tiberium(cell: &ResolvedTerrainCell) -> bool {
    !cell.outside_playfield
        && cell.allows_tiberium
        && cell.slope_type == 0
        && !cell.base_build_blocked
        && cell.bridge_flags() & (BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_DESTROYED_OR_RAMP) == 0
}

pub(crate) fn live_cell_rejects_tiberium(
    cell: (u16, u16),
    context: TiberiumPlacementObjectContext<'_>,
) -> bool {
    let Some(occupancy) = context.occupancy.get(cell.0, cell.1) else {
        return false;
    };
    for occupant in &occupancy.occupants {
        let Some(entity) = context.entities.get(occupant.entity_id) else {
            continue;
        };
        if entity.category != EntityCategory::Structure || !entity.is_alive() {
            continue;
        }
        let type_name = context.interner.resolve(entity.type_ref());
        let invisible_exception = context
            .rules
            .object(type_name)
            .is_some_and(|object| object.invisible || object.invisible_in_game);
        if !invisible_exception {
            return true;
        }
    }
    false
}

/// Shared empty-cell admission for every production placement path.
pub(crate) fn can_place_new_tiberium(
    overlay_grid: &OverlayGrid,
    source_object_cells: &std::collections::BTreeSet<(u16, u16)>,
    admission: NewTiberiumAdmission<'_>,
    cell: (u16, u16),
) -> bool {
    if cell.0 >= overlay_grid.width()
        || cell.1 >= overlay_grid.height()
        || source_object_cells.contains(&cell)
        || overlay_grid.cell(cell.0, cell.1).overlay_id.is_some()
    {
        return false;
    }
    if let Some(terrain) = admission.resolved_terrain {
        let Some(terrain_cell) = terrain.cell(cell.0, cell.1) else {
            return false;
        };
        if !resolved_cell_accepts_tiberium(terrain_cell) {
            return false;
        }
    } else if admission
        .path_grid
        .is_some_and(|grid| grid.cell(cell.0, cell.1).is_none())
    {
        return false;
    }
    !admission
        .live_objects
        .is_some_and(|objects| live_cell_rejects_tiberium(cell, objects))
}

/// Mutable state for the native `CellClass::PlaceTiberium` boundary.
pub struct PlaceTiberiumContext<'a> {
    pub overlay_grid: &'a mut OverlayGrid,
    pub ore_growth_state: &'a mut OreGrowthState,
    pub overlay_registry: &'a OverlayTypeRegistry,
    pub tiberium_types: &'a TiberiumTypeRegistry,
    pub resolved_terrain: Option<&'a ResolvedTerrainGrid>,
    pub source_object_cells: &'a std::collections::BTreeSet<(u16, u16)>,
    pub new_cell_admission: Option<NewTiberiumAdmission<'a>>,
    /// Live ground object lists for the `CanSpreadTiberium` `FirstObject`
    /// gate of the existing-cell spread feed.
    pub live_objects: Option<NativeCellObjectView<'a>>,
    pub rng: &'a mut SimRng,
    pub binary_frame: u32,
    pub growth_enabled: bool,
    pub spread_enabled: bool,
    pub radar_dirty_cells: Option<&'a mut Vec<(u16, u16)>>,
    pub radar_dirty_generation: Option<&'a mut u64>,
    pub tactical_dirty_cells: Option<&'a mut Vec<(u16, u16)>>,
}

/// Place a new tiberium overlay or grow a matching existing overlay.
///
/// This keeps the retail write order: new placement stamps one of the twelve
/// flat image variants, invokes `AddToGrowthQueue` while data is still zero,
/// then writes the caller's exact data byte. Existing growth writes the
/// low-byte sum clamped to 11 and feeds the same type's spread queue.
pub fn place_tiberium(
    ctx: &mut PlaceTiberiumContext<'_>,
    cell: (u16, u16),
    type_id: TiberiumTypeId,
    amount: u8,
) -> bool {
    let Some(ty) = ctx.tiberium_types.get(type_id) else {
        return false;
    };
    if amount >= 12 || cell.0 >= ctx.overlay_grid.width() || cell.1 >= ctx.overlay_grid.height() {
        return false;
    }

    let flat = ctx.resolved_terrain.is_none_or(|terrain| {
        terrain
            .cell(cell.0, cell.1)
            .is_some_and(|terrain_cell| terrain_cell.slope_type == 0)
    });
    let current = *ctx.overlay_grid.cell(cell.0, cell.1);
    if current.overlay_id.is_none() {
        let Some(admission) = ctx.new_cell_admission else {
            return false;
        };
        if !can_place_new_tiberium(ctx.overlay_grid, ctx.source_object_cells, admission, cell) {
            return false;
        }
        let Some(variants) = ctx.overlay_registry.flat_tiberium_variant_ids(ty) else {
            return false;
        };
        let overlay_id = variants[ctx.rng.next_range_u32(12) as usize];
        ctx.overlay_grid
            .place_overlay(cell.0, cell.1, overlay_id, 0);
        ctx.ore_growth_state.add_native_growth_queue_cell(
            ctx.overlay_grid,
            ctx.overlay_registry,
            ctx.tiberium_types,
            cell.0,
            cell.1,
            ctx.binary_frame,
            ctx.rng,
        );
        // The overlay stamp already registered the cell mutation. Write the
        // exact raw byte without creating a second deferred overlay-dirty item.
        ctx.overlay_grid.cell_mut(cell.0, cell.1).overlay_data = amount;
        mark_place_tactical_dirty(ctx, cell);
        mark_place_radar_dirty(ctx, cell);
        return true;
    }

    let Some(view) = tiberium_cell_view(
        ctx.overlay_grid,
        ctx.overlay_registry,
        ctx.tiberium_types,
        cell,
    ) else {
        return false;
    };
    if !ctx.growth_enabled
        || !flat
        || view.tiberium_type != type_id
        || view.overlay_data >= 11
        || !crate::sim::ore_growth::native_percentage_admits(ty.growth_percentage_bits)
    {
        return false;
    }

    let new_data = view.overlay_data.wrapping_add(amount).min(11);
    ctx.overlay_grid.set_overlay_data(cell.0, cell.1, new_data);
    mark_place_tactical_dirty(ctx, cell);
    let source_has_object = crate::sim::ore_growth::cell_has_native_object(
        ctx.source_object_cells,
        ctx.live_objects,
        cell,
    );
    ctx.ore_growth_state.add_native_spread_queue_cell(
        ctx.overlay_grid,
        ctx.overlay_registry,
        ctx.tiberium_types,
        ctx.resolved_terrain,
        source_has_object,
        cell.0,
        cell.1,
        ctx.binary_frame,
        ctx.spread_enabled,
        ctx.rng,
    );
    true
}

fn mark_place_radar_dirty(ctx: &mut PlaceTiberiumContext<'_>, cell: (u16, u16)) {
    if let Some(cells) = ctx.radar_dirty_cells.as_deref_mut()
        && !cells.contains(&cell)
    {
        cells.push(cell);
        if let Some(generation) = ctx.radar_dirty_generation.as_deref_mut() {
            *generation = (*generation).wrapping_add(1);
        }
    }
}

fn mark_place_tactical_dirty(ctx: &mut PlaceTiberiumContext<'_>, cell: (u16, u16)) {
    if let Some(cells) = ctx.tactical_dirty_cells.as_deref_mut()
        && !cells.contains(&cell)
    {
        cells.push(cell);
    }
}

/// Result of one `Reduce_Tiberium` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReduceTiberiumOutcome {
    pub removed_amount: u16,
    pub resource_type: Option<ResourceType>,
    pub fully_removed: bool,
}

impl ReduceTiberiumOutcome {
    fn none() -> Self {
        Self {
            removed_amount: 0,
            resource_type: None,
            fully_removed: false,
        }
    }
}

/// Apply gamemd-shaped tiberium reduction to one cell.
pub fn reduce_tiberium(
    ctx: &mut ReduceTiberiumContext<'_>,
    cell: (u16, u16),
    amount: i32,
) -> ReduceTiberiumOutcome {
    if amount <= 0 {
        return ReduceTiberiumOutcome::none();
    }

    let Some(view) = (match (
        ctx.overlay_grid.as_deref(),
        ctx.overlay_registry,
        ctx.tiberium_types,
    ) {
        (Some(grid), Some(registry), Some(types)) => {
            // A non-tiberium overlay is an invalid reduction target.
            let Some(view) = tiberium_cell_view(grid, registry, types, cell) else {
                return ReduceTiberiumOutcome::none();
            };
            Some(view)
        }
        _ => None,
    }) else {
        // Cell overlay identity plus raw OverlayData is the only production
        // authority. An incompletely initialized caller cannot substitute the
        // serialized compatibility node map.
        return ReduceTiberiumOutcome::none();
    };
    let current = view.overlay_data;

    // `CellClass::ReduceTiberium` @ 0x00480A80 calls
    // `TiberiumClass::RegisterForGrowth` @ 0x007235A0 at density 11 first.
    // Its `< 11` admission makes this live call a deliberate no-op: no queue
    // entry and no Scenario RNG draw are produced before the reduction.
    if current == 11
        && let (Some(grid), Some(registry), Some(types), Some(rng)) = (
            ctx.overlay_grid.as_deref(),
            ctx.overlay_registry,
            ctx.tiberium_types,
            ctx.rng.as_deref_mut(),
        )
    {
        let _ = ctx.ore_growth_state.add_native_growth_queue_cell(
            grid,
            registry,
            types,
            cell.0,
            cell.1,
            ctx.binary_frame,
            rng,
        );
    }

    // Native partial predicate is signed `amount < current + 1`. This makes a
    // density-11 request of 11 leave the overlay present at density zero, while
    // 12 clears it. A positive request against data zero takes the full path.
    if amount < i32::from(current) + 1 {
        let remaining = current.wrapping_sub(amount as u8);
        if let Some(grid) = ctx.overlay_grid.as_deref_mut() {
            grid.set_overlay_data(cell.0, cell.1, remaining);
        }
        mark_tactical_dirty(ctx, cell);
        return ReduceTiberiumOutcome {
            removed_amount: amount as u16,
            resource_type: Some(view.resource_type),
            fully_removed: false,
        };
    }

    if let Some(grid) = ctx.overlay_grid.as_deref_mut() {
        grid.clear_overlay(cell.0, cell.1);
        if let (Some(terrain), Some(registry)) =
            (ctx.resolved_terrain.as_deref_mut(), ctx.overlay_registry)
        {
            // Retail recalculates cell attributes synchronously inside the full
            // removal boundary, before any later sim system can observe it.
            grid.recalculate_runtime_cell(
                terrain,
                registry,
                cell,
                crate::sim::overlay_grid::NavigationPublication::FrameBoundary,
            );
        }
    }

    // Retail dirties radar immediately after clear/recalc, before touching any
    // spread bitmap or queue state.
    mark_radar_dirty(ctx, cell);
    ctx.ore_growth_state
        .clear_native_spread_bitmap_cell(cell.0, cell.1);
    if let (Some(grid), Some(registry), Some(types), Some(source_object_cells), Some(rng)) = (
        ctx.overlay_grid.as_deref(),
        ctx.overlay_registry,
        ctx.tiberium_types,
        ctx.source_object_cells,
        ctx.rng.as_deref_mut(),
    ) {
        ctx.ore_growth_state
            .reseed_native_spread_neighbors_after_reduction(
                view.tiberium_type,
                grid,
                registry,
                types,
                ctx.resolved_terrain.as_deref(),
                source_object_cells,
                ctx.live_objects,
                cell,
                ctx.binary_frame,
                ctx.spread_enabled,
                rng,
            );
    }
    mark_tactical_dirty(ctx, cell);

    ReduceTiberiumOutcome {
        removed_amount: u16::from(current),
        resource_type: Some(view.resource_type),
        fully_removed: true,
    }
}

fn mark_radar_dirty(ctx: &mut ReduceTiberiumContext<'_>, cell: (u16, u16)) {
    if let Some(cells) = ctx.radar_dirty_cells.as_deref_mut()
        && !cells.contains(&cell)
    {
        cells.push(cell);
        if let Some(generation) = ctx.radar_dirty_generation.as_deref_mut() {
            *generation = (*generation).wrapping_add(1);
        }
    }
}

fn mark_tactical_dirty(ctx: &mut ReduceTiberiumContext<'_>, cell: (u16, u16)) {
    if let Some(cells) = ctx.tactical_dirty_cells.as_deref_mut()
        && !cells.contains(&cell)
    {
        cells.push(cell);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    use crate::map::overlay::{OverlayDataPack, OverlayEntry};
    use crate::map::overlay_types::OverlayTypeRegistry;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::rules::terrain_rules::{LandType, SpeedCostProfile, TerrainClass};
    use crate::rules::tiberium_type::TiberiumTypeRegistry;
    use crate::sim::entity_store::EntityStore;
    use crate::sim::intern::StringInterner;
    use crate::sim::occupancy::OccupancyGrid;
    use crate::sim::rng::SimRng;
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::world::Simulation;

    /// `NativeCellObjectView` is the Rust read of `Cell+0xE4 FirstObject != 0`:
    /// terrain objects live in the production cell index, Technos in the
    /// ground occupancy list, and either alone makes the cell occupied.
    #[test]
    fn native_cell_object_view_joins_terrain_objects_with_ground_occupancy() {
        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            6,
            5,
            1,
            crate::sim::movement::locomotor::MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::AppendBuilding,
        );
        let terrain_object_cells = std::collections::BTreeMap::from([((5u16, 5u16), 9u64)]);
        let view = NativeCellObjectView::new(&occupancy, &terrain_object_cells);

        assert!(view.ground_object_present((5, 5)), "plain terrain object");
        assert!(view.ground_object_present((6, 5)), "ground-list Techno");
        assert!(!view.ground_object_present((7, 5)));
        assert_eq!(
            view.occupied_ground_cells().collect::<BTreeSet<_>>(),
            BTreeSet::from([(5, 5), (6, 5)])
        );
        assert!(crate::sim::ore_growth::cell_has_native_object(
            &BTreeSet::new(),
            Some(view),
            (5, 5)
        ));
        assert!(!crate::sim::ore_growth::cell_has_native_object(
            &BTreeSet::new(),
            None,
            (5, 5)
        ));
    }

    fn native_tiberium_fixture() -> (OverlayTypeRegistry, TiberiumTypeRegistry) {
        native_tiberium_fixture_with_riparius_growth(".06")
    }

    fn native_tiberium_fixture_with_riparius_growth(
        riparius_growth: &str,
    ) -> (OverlayTypeRegistry, TiberiumTypeRegistry) {
        let mut ini_text = format!(
            "\
[Tiberiums]
0=Riparius
1=Cruentus
2=Vinifera

[Riparius]
Image=1
Value=25
Growth=2200
GrowthPercentage={riparius_growth}
Spread=2200
SpreadPercentage=.06

[Cruentus]
Image=2
Value=50
Growth=10000
GrowthPercentage=0
Spread=10000
SpreadPercentage=0

[Vinifera]
Image=3
Value=25
Growth=2200
GrowthPercentage=.06
Spread=2200
SpreadPercentage=.06

[OverlayTypes]
"
        );
        let mut tiberium_names = Vec::new();
        for raw_key in (1..=149).filter(|key| *key != 40 && *key != 41) {
            let name = match raw_key {
                28..=39 => format!("GEM{:02}", raw_key - 27),
                105..=124 => format!("TIB{:02}", raw_key - 104),
                130..=149 => format!("TIB2_{:02}", raw_key - 129),
                _ => format!("FILL{raw_key:03}"),
            };
            ini_text.push_str(&format!("{raw_key}={name}\n"));
            if name.starts_with("TIB") || name.starts_with("GEM") {
                tiberium_names.push(name);
            }
        }
        for name in tiberium_names {
            ini_text.push_str(&format!("[{name}]\nTiberium=yes\n"));
        }
        let ini = IniFile::from_str(&ini_text);
        (
            OverlayTypeRegistry::from_ini(&ini, None),
            TiberiumTypeRegistry::from_ini(&ini),
        )
    }

    fn flat_clear_terrain() -> ResolvedTerrainGrid {
        let land_type = LandType::Clear.as_index();
        let speed_costs = SpeedCostProfile::default();
        ResolvedTerrainGrid::from_cells(
            1,
            1,
            vec![ResolvedTerrainCell {
                rx: 0,
                ry: 0,
                source_tile_index: 0,
                source_sub_tile: 0,
                final_tile_index: 0,
                final_sub_tile: 0,
                is_wood_bridge_repair_tile: false,
                level: 0,
                filled_clear: true,
                tileset_index: None,
                land_type,
                yr_cell_land_type: land_type,
                slope_type: 0,
                template_height: 0,
                render_offset_x: 0,
                render_offset_y: 0,
                terrain_class: TerrainClass::Clear,
                speed_costs,
                is_water: false,
                is_cliff_like: false,
                is_rough: false,
                is_road: false,
                accepts_smudge: true,
                allows_tiberium: true,
                height_in_pixels: 0,
                variant: 0,
                has_ramp: false,
                canonical_ramp: None,
                ground_walk_blocked: false,
                terrain_object_blocks: false,
                terrain_object_occupation: None,
                overlay_blocks: false,
                overlay_zone_type: None,
                outside_playfield: false,
                zone_type: zone_class::GROUND,
                base_ground_walk_blocked: false,
                base_build_blocked: false,
                base_land_type: land_type,
                base_yr_cell_land_type: land_type,
                base_terrain_class: TerrainClass::Clear,
                base_speed_costs: speed_costs,
                build_blocked: false,
                has_bridge_deck: false,
                bridge_walkable: false,
                bridge_transition: false,
                bridge_deck_level: 0,
                bridge_layer: None,
                bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
                tube_index: None,
                radar_left: [0; 3],
                radar_right: [0; 3],
                has_damaged_data: false,
                bridgehead_anchor_class_at_load: None,
            }],
        )
    }

    fn flat_clear_terrain_grid(width: u16, height: u16) -> ResolvedTerrainGrid {
        let mut seed = flat_clear_terrain();
        let template = seed.cells.remove(0);
        let mut cells = Vec::with_capacity(usize::from(width) * usize::from(height));
        for ry in 0..height {
            for rx in 0..width {
                let mut cell = template.clone();
                cell.rx = rx;
                cell.ry = ry;
                cells.push(cell);
            }
        }
        ResolvedTerrainGrid::from_cells(width, height, cells)
    }

    #[test]
    fn gsi_04_09_new_placement_requires_proof_and_rejects_outside_playfield() {
        let (overlay_registry, tiberium_types) = native_tiberium_fixture();
        let mut terrain = flat_clear_terrain();
        terrain.cell_mut(0, 0).unwrap().outside_playfield = true;
        let rules_ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n",
        );
        let rules = RuleSet::from_ini(&rules_ini).expect("rules");
        let entities = EntityStore::new();
        let occupancy = OccupancyGrid::new();
        let interner = StringInterner::default();
        let terrain_object_cells = BTreeMap::new();
        let live_objects = TiberiumPlacementObjectContext::new(
            &entities,
            &occupancy,
            &rules,
            &interner,
            &terrain_object_cells,
        );
        let runtime_admission = NewTiberiumAdmission::runtime(&terrain, None, live_objects);

        let mut overlay = OverlayGrid::new(1, 1);
        let mut growth = OreGrowthState::new(1, 1);
        growth.reset_native_tiberium_classes(tiberium_types.len(), 40);
        let source_cells = BTreeSet::new();
        let mut rng = SimRng::new(0x409);
        let before_rng = rng.logical_state();
        let mut radar_dirty = Vec::new();
        let mut radar_generation = 0;
        let mut tactical_dirty = Vec::new();

        for admission in [None, Some(runtime_admission)] {
            let mut ctx = PlaceTiberiumContext {
                overlay_grid: &mut overlay,
                ore_growth_state: &mut growth,
                overlay_registry: &overlay_registry,
                tiberium_types: &tiberium_types,
                resolved_terrain: Some(&terrain),
                source_object_cells: &source_cells,
                new_cell_admission: admission,
                live_objects: None,
                rng: &mut rng,
                binary_frame: 40,
                growth_enabled: true,
                spread_enabled: true,
                radar_dirty_cells: Some(&mut radar_dirty),
                radar_dirty_generation: Some(&mut radar_generation),
                tactical_dirty_cells: Some(&mut tactical_dirty),
            };
            assert!(!place_tiberium(&mut ctx, (0, 0), TiberiumTypeId(0), 3));
        }

        assert_eq!(overlay.cell(0, 0).overlay_id, None);
        assert!(growth.native_tiberium_state().classes.iter().all(|class| {
            class.growth.is_empty()
                && class.spread.is_empty()
                && class.growth_bitmap.is_empty()
                && class.spread_bitmap.is_empty()
        }));
        assert!(radar_dirty.is_empty());
        assert_eq!(radar_generation, 0);
        assert!(tactical_dirty.is_empty());
        assert_eq!(rng.logical_state(), before_rng);
    }

    #[test]
    fn gsi_04_09_new_placement_uses_primary_variant_growth_hook_and_exact_data() {
        let (overlay_registry, tiberium_types) = native_tiberium_fixture();
        let variants = overlay_registry
            .flat_tiberium_variant_ids(tiberium_types.get(TiberiumTypeId(0)).unwrap())
            .expect("Riparius variants");
        let mut overlay = OverlayGrid::new(8, 8);
        let mut growth = OreGrowthState::new(8, 8);
        growth.reset_native_tiberium_classes(tiberium_types.len(), 40);
        let mut rng = SimRng::new(0x409);
        let mut expected_rng = rng.clone();
        let expected_overlay = variants[expected_rng.next_range_u32(12) as usize];
        expected_rng.next_u32(); // AddToGrowthQueue priority.
        let source_cells = BTreeSet::new();
        let compatibility_admission =
            NewTiberiumAdmission::compatibility_without_native_context(None, None, None);
        let mut radar_dirty = Vec::new();
        let mut radar_generation = 0;
        let mut tactical_dirty = Vec::new();
        {
            let mut ctx = PlaceTiberiumContext {
                overlay_grid: &mut overlay,
                ore_growth_state: &mut growth,
                overlay_registry: &overlay_registry,
                tiberium_types: &tiberium_types,
                resolved_terrain: None,
                source_object_cells: &source_cells,
                new_cell_admission: Some(compatibility_admission),
                live_objects: None,
                rng: &mut rng,
                binary_frame: 40,
                growth_enabled: true,
                spread_enabled: true,
                radar_dirty_cells: Some(&mut radar_dirty),
                radar_dirty_generation: Some(&mut radar_generation),
                tactical_dirty_cells: Some(&mut tactical_dirty),
            };
            assert!(place_tiberium(&mut ctx, (4, 4), TiberiumTypeId(0), 11));
        }
        assert_eq!(overlay.cell(4, 4).overlay_id, Some(expected_overlay));
        assert_eq!(overlay.cell(4, 4).overlay_data, 11);
        let class = &growth.native_tiberium_state().classes[0];
        assert_eq!(
            class.growth.len(),
            1,
            "AddToGrowthQueue must run while the newly stamped cell still has data zero"
        );
        assert!(class.growth_bitmap.contains(&(4, 4)));
        assert_eq!(rng.logical_state(), expected_rng.logical_state());
        assert_eq!(radar_dirty, vec![(4, 4)]);
        assert_eq!(radar_generation, 1);
        assert_eq!(tactical_dirty, vec![(4, 4)]);

        let state_before_reject = rng.logical_state();
        let mut reject_ctx = PlaceTiberiumContext {
            overlay_grid: &mut overlay,
            ore_growth_state: &mut growth,
            overlay_registry: &overlay_registry,
            tiberium_types: &tiberium_types,
            resolved_terrain: None,
            source_object_cells: &source_cells,
            new_cell_admission: Some(compatibility_admission),
            live_objects: None,
            rng: &mut rng,
            binary_frame: 40,
            growth_enabled: true,
            spread_enabled: true,
            radar_dirty_cells: Some(&mut radar_dirty),
            radar_dirty_generation: Some(&mut radar_generation),
            tactical_dirty_cells: Some(&mut tactical_dirty),
        };
        assert!(!place_tiberium(
            &mut reject_ctx,
            (5, 5),
            TiberiumTypeId(0),
            12,
        ));
        assert_eq!(rng.logical_state(), state_before_reject);
        assert_eq!(overlay.cell(5, 5).overlay_id, None);
    }

    #[test]
    fn gsi_04_09_existing_growth_honors_threshold_clamp_and_tactical_only_dirty() {
        for (growth_percentage, succeeds) in [(".000009", false), (".00001", true)] {
            let (overlay_registry, tiberium_types) =
                native_tiberium_fixture_with_riparius_growth(growth_percentage);
            let tib01 = overlay_registry.id_for_name("TIB01").expect("TIB01");
            let mut overlay = OverlayGrid::new(8, 8);
            overlay.place_overlay(4, 4, tib01, 10);
            let mut growth = OreGrowthState::new(8, 8);
            growth.reset_native_tiberium_classes(tiberium_types.len(), 0);
            let mut rng = SimRng::new(12);
            let source_cells = BTreeSet::new();
            let mut radar_dirty = Vec::new();
            let mut radar_generation = 0;
            let mut tactical_dirty = Vec::new();
            let mut ctx = PlaceTiberiumContext {
                overlay_grid: &mut overlay,
                ore_growth_state: &mut growth,
                overlay_registry: &overlay_registry,
                tiberium_types: &tiberium_types,
                resolved_terrain: None,
                source_object_cells: &source_cells,
                new_cell_admission: None,
                live_objects: None,
                rng: &mut rng,
                binary_frame: 0,
                growth_enabled: true,
                spread_enabled: true,
                radar_dirty_cells: Some(&mut radar_dirty),
                radar_dirty_generation: Some(&mut radar_generation),
                tactical_dirty_cells: Some(&mut tactical_dirty),
            };

            assert_eq!(
                place_tiberium(&mut ctx, (4, 4), TiberiumTypeId(0), 7),
                succeeds,
                "GrowthPercentage={growth_percentage}"
            );
            assert_eq!(
                overlay.cell(4, 4).overlay_data,
                if succeeds { 11 } else { 10 }
            );
            assert!(radar_dirty.is_empty());
            assert_eq!(radar_generation, 0);
            assert_eq!(
                tactical_dirty,
                if succeeds { vec![(4, 4)] } else { Vec::new() }
            );
            assert_eq!(
                growth.native_tiberium_state().classes[0]
                    .spread_bitmap
                    .contains(&(4, 4)),
                succeeds
            );
        }
    }

    #[test]
    fn gsi_04_09_overlay_view_preserves_raw_zero_max_and_extra_range_identity() {
        let (overlay_registry, tiberium_types) = native_tiberium_fixture();
        let tib13 = overlay_registry.id_for_name("TIB13").expect("TIB13");
        let gem12 = overlay_registry.id_for_name("GEM12").expect("GEM12");
        let tib2_20 = overlay_registry.id_for_name("TIB2_20").expect("TIB2_20");
        let mut overlay = OverlayGrid::new(8, 8);
        overlay.place_overlay(1, 1, tib13, 0);
        overlay.place_overlay(2, 2, gem12, 11);
        overlay.place_overlay(3, 3, tib2_20, 7);

        let zero = tiberium_cell_view(&overlay, &overlay_registry, &tiberium_types, (1, 1))
            .expect("raw data zero remains a present resource cell");
        assert_eq!(zero.tiberium_type, TiberiumTypeId(0));
        assert_eq!(zero.resource_type, ResourceType::Ore);
        assert_eq!(zero.overlay_data, 0);
        assert_eq!(zero.nominal_value, 25);

        let max =
            tiberium_cell_view(&overlay, &overlay_registry, &tiberium_types, (2, 2)).expect("gem");
        assert_eq!(max.tiberium_type, TiberiumTypeId(1));
        assert_eq!(max.resource_type, ResourceType::Gem);
        assert_eq!(max.overlay_data, 11);
        assert_eq!(max.nominal_value, 600);

        let extra = tiberium_cell_view(&overlay, &overlay_registry, &tiberium_types, (3, 3))
            .expect("TIB2 extra range");
        assert_eq!(extra.tiberium_type, TiberiumTypeId(2));
        assert_eq!(extra.resource_type, ResourceType::Ore);
        assert_eq!(extra.overlay_data, 7);
        assert_eq!(extra.nominal_value, 200);
    }

    #[test]
    fn gsi_04_09_native_packed_map_snapshot_preserves_one_cell_authority() {
        let (overlay_registry, tiberium_types) = native_tiberium_fixture();
        let tib01 = overlay_registry.id_for_name("TIB01").expect("TIB01");
        let gem12 = overlay_registry.id_for_name("GEM12").expect("GEM12");
        let tib13 = overlay_registry.id_for_name("TIB13").expect("TIB13");
        assert_eq!((tib01, gem12, tib13), (102, 38, 114));

        let entries = [
            OverlayEntry {
                rx: 0,
                ry: 0,
                overlay_id: tib01,
                frame: 99,
            },
            OverlayEntry {
                rx: 1,
                ry: 0,
                overlay_id: gem12,
                frame: 99,
            },
            OverlayEntry {
                rx: 2,
                ry: 0,
                overlay_id: tib13,
                frame: 99,
            },
            // Registered but unavailable art: pass one rejects identity while
            // pass two must retain the raw byte.
            OverlayEntry {
                rx: 4,
                ry: 0,
                overlay_id: 0,
                frame: 99,
            },
            // Bytes exist in both packs but the native cell slot is absent.
            OverlayEntry {
                rx: 5,
                ry: 0,
                overlay_id: tib01,
                frame: 99,
            },
        ];
        let build_grid = |empty_raw: u8| {
            let mut terrain = flat_clear_terrain_grid(6, 1);
            terrain.test_set_native_allocated_cells(&[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)]);
            let data = OverlayDataPack::from_cells([
                (0, 0, 0),
                (1, 0, 11),
                (2, 0, 7),
                (3, 0, empty_raw),
                (4, 0, 0x5A),
                (5, 0, 0xEE),
            ]);
            OverlayGrid::from_native_overlay_packs(
                &entries,
                &data,
                &mut terrain,
                &overlay_registry,
                &BTreeSet::from([tib01, gem12, tib13]),
                false,
            )
        };

        let mut grid = build_grid(0xA5);
        assert_eq!(
            tiberium_cell_view(&grid, &overlay_registry, &tiberium_types, (0, 0))
                .map(|view| (view.tiberium_type, view.overlay_data)),
            Some((TiberiumTypeId(0), 0)),
            "data zero remains a present ore cell"
        );
        assert_eq!(
            tiberium_cell_view(&grid, &overlay_registry, &tiberium_types, (1, 0))
                .map(|view| (view.tiberium_type, view.overlay_data)),
            Some((TiberiumTypeId(1), 11))
        );
        assert_eq!(
            tiberium_cell_view(&grid, &overlay_registry, &tiberium_types, (2, 0))
                .map(|view| (view.tiberium_type, view.overlay_data)),
            Some((TiberiumTypeId(0), 7)),
            "TIB13 resolves through Riparius' extra range"
        );
        assert_eq!(
            (grid.cell(3, 0).overlay_id, grid.cell(3, 0).overlay_data),
            (None, 0xA5)
        );
        assert_eq!(
            (grid.cell(4, 0).overlay_id, grid.cell(4, 0).overlay_data),
            (None, 0x5A),
            "rejected pass-one identity does not suppress pass-two data"
        );
        assert_eq!(
            (grid.cell(5, 0).overlay_id, grid.cell(5, 0).overlay_data),
            (None, 0),
            "unallocated native slot receives neither pack"
        );
        assert_eq!(
            grid.take_dirty_cells_with_passability_signal(),
            (Vec::new(), false),
            "map initialization creates no runtime mutation trace"
        );

        let mut sim = Simulation::new();
        sim.overlay_grid = Some(grid);
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // packed-overlay persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();
        let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_09_packed.map", 0);
        let restored = GameSnapshot::load(&bytes)
            .expect("current packed-map snapshot")
            .sim;
        assert_eq!(restored.state_hash(), expected_hash);
        let restored_grid = restored
            .overlay_grid
            .as_ref()
            .expect("restored overlay grid");
        for rx in 0..6 {
            assert_eq!(
                restored_grid.cell(rx, 0),
                sim.overlay_grid.as_ref().unwrap().cell(rx, 0)
            );
        }

        let mut raw_changed = restored;
        raw_changed.overlay_grid = Some(build_grid(0xA6));
        assert_ne!(
            raw_changed.state_hash(),
            expected_hash,
            "identity-empty OverlayData remains serialized and hash-authoritative"
        );
    }

    #[test]
    fn gsi_04_09_missing_reducer_context_fails_closed() {
        let mut growth = OreGrowthState::new(10, 10);
        let mut radar_dirty = Vec::new();
        let mut radar_generation = 0;
        let mut tactical_dirty = Vec::new();
        let mut ctx = ReduceTiberiumContext {
            overlay_grid: None,
            ore_growth_state: &mut growth,
            overlay_registry: None,
            tiberium_types: None,
            resolved_terrain: None,
            source_object_cells: None,
            live_objects: None,
            rng: None,
            binary_frame: 0,
            spread_enabled: false,
            radar_dirty_cells: Some(&mut radar_dirty),
            radar_dirty_generation: Some(&mut radar_generation),
            tactical_dirty_cells: Some(&mut tactical_dirty),
        };

        assert_eq!(
            reduce_tiberium(&mut ctx, (5, 5), 2),
            ReduceTiberiumOutcome::none()
        );
        assert!(radar_dirty.is_empty());
        assert_eq!(radar_generation, 0);
        assert!(tactical_dirty.is_empty());
    }

    #[test]
    fn gsi_04_09_reducer_boundary_table_uses_raw_overlay_data() {
        let (overlay_registry, tiberium_types) = native_tiberium_fixture();
        let tib01 = overlay_registry.id_for_name("TIB01").expect("TIB01");
        let cases = [
            // data, amount, removed, fully_removed, remaining_data
            (0u8, 1, 0u16, true, None),
            (11, 11, 11, false, Some(0)),
            (11, 12, 11, true, None),
        ];

        for (data, amount, removed, fully_removed, remaining_data) in cases {
            // Deliberately contradictory legacy state proves it is not read or
            // mirrored when the production overlay context is complete.
            let mut overlay = OverlayGrid::new(8, 8);
            overlay.place_overlay(4, 4, tib01, data);
            let mut growth = OreGrowthState::new(8, 8);
            growth.reset_native_tiberium_classes(tiberium_types.len(), 0);
            let mut radar_dirty = Vec::new();
            let mut radar_generation = 0;
            let mut tactical_dirty = Vec::new();
            let mut ctx = ReduceTiberiumContext {
                overlay_grid: Some(&mut overlay),
                ore_growth_state: &mut growth,
                overlay_registry: Some(&overlay_registry),
                tiberium_types: Some(&tiberium_types),
                resolved_terrain: None,
                source_object_cells: None,
                live_objects: None,
                rng: None,
                binary_frame: 0,
                spread_enabled: false,
                radar_dirty_cells: Some(&mut radar_dirty),
                radar_dirty_generation: Some(&mut radar_generation),
                tactical_dirty_cells: Some(&mut tactical_dirty),
            };

            let outcome = reduce_tiberium(&mut ctx, (4, 4), amount);
            assert_eq!(
                outcome.removed_amount, removed,
                "data={data} amount={amount}"
            );
            assert_eq!(
                outcome.resource_type,
                Some(ResourceType::Ore),
                "data={data} amount={amount}"
            );
            assert_eq!(
                outcome.fully_removed, fully_removed,
                "data={data} amount={amount}"
            );
            match remaining_data {
                Some(expected) => {
                    assert_eq!(overlay.cell(4, 4).overlay_id, Some(tib01));
                    assert_eq!(overlay.cell(4, 4).overlay_data, expected);
                    assert!(radar_dirty.is_empty(), "partial mutation is tactical-only");
                    assert_eq!(radar_generation, 0);
                }
                None => {
                    assert_eq!(overlay.cell(4, 4).overlay_id, None);
                    assert_eq!(radar_dirty, vec![(4, 4)]);
                    assert_eq!(radar_generation, 1);
                }
            }
            assert_eq!(tactical_dirty, vec![(4, 4)]);
        }

        let mut overlay = OverlayGrid::new(8, 8);
        overlay.place_overlay(4, 4, tib01, 6);
        let mut growth = OreGrowthState::new(8, 8);
        let mut ctx = ReduceTiberiumContext {
            overlay_grid: Some(&mut overlay),
            ore_growth_state: &mut growth,
            overlay_registry: Some(&overlay_registry),
            tiberium_types: Some(&tiberium_types),
            resolved_terrain: None,
            source_object_cells: None,
            live_objects: None,
            rng: None,
            binary_frame: 0,
            spread_enabled: false,
            radar_dirty_cells: None,
            radar_dirty_generation: None,
            tactical_dirty_cells: None,
        };
        assert_eq!(reduce_tiberium(&mut ctx, (4, 4), 0).removed_amount, 0);
        assert_eq!(reduce_tiberium(&mut ctx, (4, 4), -3).removed_amount, 0);
        assert_eq!(overlay.cell(4, 4).overlay_data, 6);
    }

    #[test]
    fn gsi_04_09_max_density_reduction_runs_the_growth_admission_without_rng() {
        let (overlay_registry, tiberium_types) = native_tiberium_fixture();
        let tib01 = overlay_registry.id_for_name("TIB01").expect("TIB01");
        let mut overlay = OverlayGrid::new(8, 8);
        overlay.place_overlay(4, 4, tib01, 11);
        let mut growth = OreGrowthState::new(8, 8);
        growth.reset_native_tiberium_classes(tiberium_types.len(), 0);
        let mut rng = SimRng::new(0x480a80);
        let expected_rng = rng.clone();
        let mut ctx = ReduceTiberiumContext {
            overlay_grid: Some(&mut overlay),
            ore_growth_state: &mut growth,
            overlay_registry: Some(&overlay_registry),
            tiberium_types: Some(&tiberium_types),
            resolved_terrain: None,
            source_object_cells: None,
            live_objects: None,
            rng: Some(&mut rng),
            binary_frame: 42,
            spread_enabled: false,
            radar_dirty_cells: None,
            radar_dirty_generation: None,
            tactical_dirty_cells: None,
        };

        let outcome = reduce_tiberium(&mut ctx, (4, 4), 11);

        assert_eq!(outcome.removed_amount, 11);
        assert!(!outcome.fully_removed);
        assert_eq!(overlay.cell(4, 4).overlay_data, 0);
        assert!(growth.native_tiberium_state().classes[0].growth.is_empty());
        assert!(
            growth.native_tiberium_state().classes[0]
                .growth_bitmap
                .is_empty()
        );
        assert_eq!(
            rng.logical_state(),
            expected_rng.logical_state(),
            "the density-11 RegisterForGrowth call rejects before its priority RNG draw"
        );
    }

    #[test]
    fn gsi_04_09_full_reduction_propagates_synchronous_path_refresh() {
        use crate::rules::locomotor_type::SpeedType;
        use crate::sim::overlay_grid::NavigationPublication;
        use crate::sim::world::TickLane;
        use std::sync::Arc;

        let (overlay_registry, tiberium_types) = native_tiberium_fixture();
        let mut rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n",
        ))
        .expect("empty roster rules");
        rules.tiberium_types = tiberium_types;
        let mut sim = Simulation::new();
        sim.production.ore_growth_config.grows = false;
        sim.production.ore_growth_config.spreads = false;
        sim.production.ore_growth_state = OreGrowthState::new(1, 1);
        sim.production
            .ore_growth_state
            .reset_native_tiberium_classes(rules.tiberium_types.len(), 0);
        let mut terrain = flat_clear_terrain();
        let cell = terrain.cell_mut(0, 0).unwrap();
        cell.speed_costs.foot = Some(37);
        cell.base_speed_costs.foot = Some(37);
        let mut overlay = OverlayGrid::new(1, 1);
        overlay.place_overlay(0, 0, overlay_registry.id_for_name("TIB01").unwrap(), 3);
        assert!(crate::sim::overlay_grid::recalc_overlay_passability(
            &mut overlay,
            &mut terrain,
            &overlay_registry,
            0,
            0,
        ));
        overlay.take_dirty_cells();
        sim.overlay_grid = Some(overlay);
        sim.resolved_terrain = Some(terrain);
        assert!(sim.rebuild_dynamic_navigation(&rules));
        let old_cost = sim.terrain_costs[&SpeedType::Foot].cost_at(0, 0);
        assert_ne!(old_cost, 37, "fixture must expose a canonical cost change");
        let before = sim.path_grid_snapshot().unwrap();

        let outcome = sim.reduce_tiberium_at_with_native_context(
            (0, 0),
            4,
            Some(&rules),
            Some(&overlay_registry),
        );
        assert!(outcome.fully_removed);
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .cell(0, 0)
                .unwrap()
                .land_type,
            LandType::Clear.as_index(),
            "full reduction projects terrain synchronously"
        );
        assert_eq!(sim.terrain_costs[&SpeedType::Foot].cost_at(0, 0), old_cost);
        let repeated = sim.overlay_grid.as_mut().unwrap().recalculate_runtime_cell(
            sim.resolved_terrain.as_mut().unwrap(),
            &overlay_registry,
            (0, 0),
            NavigationPublication::FrameBoundary,
        );
        assert!(!repeated.navigation_changed);

        let deferred = sim
            .advance_app_frame(
                &[],
                Some(&rules),
                &BTreeMap::new(),
                None,
                67,
                TickLane::Ordinary,
                None,
            )
            .expect("fixture frame must complete");
        assert!(deferred.overlay_updates.is_empty());
        assert!(Arc::ptr_eq(&before, &sim.path_grid_snapshot().unwrap()));
        assert_eq!(
            sim.overlay_grid
                .as_ref()
                .unwrap()
                .clone()
                .take_dirty_cells_with_passability_signal(),
            (vec![(0, 0)], true),
            "missing registry retains the first true result despite the false repeat"
        );

        let first = sim
            .advance_app_frame(
                &[],
                Some(&rules),
                &BTreeMap::new(),
                Some(&overlay_registry),
                67,
                TickLane::Ordinary,
                None,
            )
            .expect("fixture frame must complete");
        assert_eq!(sim.terrain_costs[&SpeedType::Foot].cost_at(0, 0), 37);
        assert!(sim.zone_grid.is_some());
        assert!(
            first.overlay_updates.is_empty(),
            "erased tiberium has no overlay upsert"
        );
        assert_eq!(first.tick.state_hash, sim.state_hash());
        let published = sim.path_grid_snapshot().unwrap();
        assert!(!Arc::ptr_eq(&before, &published));
        assert_eq!(
            sim.overlay_grid
                .as_ref()
                .unwrap()
                .clone()
                .take_dirty_cells_with_passability_signal(),
            (Vec::new(), false)
        );
        let second = sim
            .advance_app_frame(
                &[],
                Some(&rules),
                &BTreeMap::new(),
                Some(&overlay_registry),
                67,
                TickLane::Ordinary,
                None,
            )
            .expect("fixture frame must complete");
        assert!(second.overlay_updates.is_empty());
        assert!(
            Arc::ptr_eq(&published, &sim.path_grid_snapshot().unwrap()),
            "no duplicate navigation publication"
        );
        assert_eq!(second.tick.state_hash, sim.state_hash());
    }

    /// `Reduce_Tiberium @ 0x00480A80` full removal calls the REMOVED class's
    /// `AddToSpreadQueue @ 0x00722AF0` on every in-bounds neighbour whose
    /// removed-class flag byte is clear, and `CanSpreadTiberium @ 0x00483690`
    /// takes no class: it admits the neighbour on its OWN index,
    /// `data > index / 2`, slope, that class's `SpreadPercentage`, and
    /// `FirstObject`. A fully harvested gem (Cruentus, retail
    /// `SpreadPercentage=0`) next to ore therefore queues the ore cell into the
    /// gem store with one Scenario draw, although the gem processor itself
    /// never runs.
    #[test]
    fn gsi_04_09_full_gem_removal_queues_an_ore_neighbor_into_the_gem_store() {
        let (overlay_registry, tiberium_types) = native_tiberium_fixture();
        let tib01 = overlay_registry.id_for_name("TIB01").expect("TIB01");
        let gem01 = overlay_registry.id_for_name("GEM01").expect("GEM01");
        let mut overlay = OverlayGrid::new(10, 10);
        overlay.place_overlay(5, 5, gem01, 1);
        overlay.place_overlay(6, 5, tib01, 4);
        let mut growth = OreGrowthState::new(10, 10);
        growth.reset_native_tiberium_classes(tiberium_types.len(), 100);
        let mut rng = SimRng::new(5);
        let mut expected_rng = rng.clone();
        expected_rng.next_u32();
        let source_object_cells = BTreeSet::new();

        let mut ctx = ReduceTiberiumContext {
            overlay_grid: Some(&mut overlay),
            ore_growth_state: &mut growth,
            overlay_registry: Some(&overlay_registry),
            tiberium_types: Some(&tiberium_types),
            resolved_terrain: None,
            source_object_cells: Some(&source_object_cells),
            live_objects: None,
            rng: Some(&mut rng),
            binary_frame: 200,
            spread_enabled: true,
            radar_dirty_cells: None,
            radar_dirty_generation: None,
            tactical_dirty_cells: None,
        };

        let outcome = reduce_tiberium(&mut ctx, (5, 5), 2);

        assert!(outcome.fully_removed);
        let gem_class = &growth.native_tiberium_state().classes[1];
        assert_eq!(
            gem_class
                .spread
                .iter_heap()
                .map(|entry| (entry.rx, entry.ry))
                .collect::<Vec<_>>(),
            vec![(6, 5)],
            "the ore neighbour enters the removed gem class's store"
        );
        assert!(gem_class.spread_bitmap.contains(&(6, 5)));
        let ore_class = &growth.native_tiberium_state().classes[0];
        assert!(ore_class.spread.is_empty());
        assert!(ore_class.spread_bitmap.is_empty());
        assert_eq!(
            rng.logical_state(),
            expected_rng.logical_state(),
            "one draw for the one admitted neighbour"
        );
    }

    #[test]
    fn gsi_04_09_full_reduction_clears_all_bitmaps_and_reseeds_neighbors_into_the_removed_class() {
        let (overlay_registry, tiberium_types) = native_tiberium_fixture();
        let tib01 = overlay_registry.id_for_name("TIB01").expect("TIB01");
        let gem01 = overlay_registry.id_for_name("GEM01").expect("GEM01");
        let tib2_20 = overlay_registry.id_for_name("TIB2_20").expect("TIB2_20");
        assert_eq!(tib2_20, 146);
        let mut overlay = OverlayGrid::new(10, 10);
        overlay.place_overlay(6, 5, tib2_20, 4);
        overlay.place_overlay(5, 6, tib2_20, 4);
        let mut growth = OreGrowthState::new(10, 10);
        growth.reset_native_tiberium_classes(tiberium_types.len(), 100);
        let mut rng = SimRng::new(3);
        let source_object_cells = BTreeSet::new();
        // Seed the removed-cell membership into a wrong class as well as the
        // actual class. Switching the authoritative overlay between calls is
        // enough to leave the native per-class bitmap/heap state behind. The
        // fixture's Cruentus carries the retail `SpreadPercentage=0`, so
        // `CanSpreadTiberium @ 0x00483690` (`pct < 1e-05`) never admits it.
        for overlay_id in [tib01, gem01, tib2_20] {
            overlay.place_overlay(5, 5, overlay_id, 3);
            let inserted = growth
                .add_native_spread_queue_cell(
                    &overlay,
                    &overlay_registry,
                    &tiberium_types,
                    None,
                    false,
                    5,
                    5,
                    100,
                    true,
                    &mut rng,
                )
                .is_some();
            assert_eq!(inserted, overlay_id != gem01);
        }
        for (class_index, class) in growth.native_tiberium_state().classes.iter().enumerate() {
            assert_eq!(
                class.spread_bitmap.contains(&(5, 5)),
                class_index != 1,
                "precondition: class {class_index} removed-cell bit"
            );
        }
        let mut expected_rng = rng.clone();
        expected_rng.next_u32();
        expected_rng.next_u32();

        let mut ctx = ReduceTiberiumContext {
            overlay_grid: Some(&mut overlay),
            ore_growth_state: &mut growth,
            overlay_registry: Some(&overlay_registry),
            tiberium_types: Some(&tiberium_types),
            resolved_terrain: None,
            source_object_cells: Some(&source_object_cells),
            live_objects: None,
            rng: Some(&mut rng),
            binary_frame: 200,
            spread_enabled: true,
            radar_dirty_cells: None,
            radar_dirty_generation: None,
            tactical_dirty_cells: None,
        };

        let outcome = reduce_tiberium(&mut ctx, (5, 5), 4);

        assert!(outcome.fully_removed);
        let class = &growth.native_tiberium_state().classes[2];
        assert!(class.spread_bitmap.contains(&(6, 5)));
        assert!(class.spread_bitmap.contains(&(5, 6)));
        assert_eq!(
            class
                .spread
                .iter_heap()
                .filter(|entry| (entry.rx, entry.ry) == (6, 5) || (entry.rx, entry.ry) == (5, 6))
                .count(),
            2
        );
        let reseeded: Vec<_> = class
            .spread
            .iter_heap()
            .filter(|entry| (entry.rx, entry.ry) != (5, 5))
            .map(|entry| (entry.rx, entry.ry))
            .collect();
        assert_eq!(reseeded, vec![(6, 5), (5, 6)]);
        for (class_index, wrong_class) in growth.native_tiberium_state().classes.iter().enumerate()
        {
            assert!(
                !wrong_class.spread_bitmap.contains(&(5, 5)),
                "full removal clears class {class_index}'s removed-cell bit"
            );
            if class_index != 2 {
                // Class 1 (retail `SpreadPercentage=0`) was never admitted, so
                // it has no stale entry to retain.
                let expected_stale = if class_index == 1 {
                    Vec::new()
                } else {
                    vec![(5, 5)]
                };
                assert_eq!(
                    wrong_class
                        .spread
                        .iter_heap()
                        .map(|entry| (entry.rx, entry.ry))
                        .collect::<Vec<_>>(),
                    expected_stale,
                    "wrong class {class_index} retains only its stale removed-cell heap entry"
                );
                assert!(wrong_class.spread_bitmap.is_empty());
            }
        }
        assert_eq!(
            rng.logical_state(),
            expected_rng.logical_state(),
            "reseed consumes exactly one raw draw per accepted neighbor"
        );
    }

}
