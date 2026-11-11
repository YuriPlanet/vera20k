//! Shared tiberium cell mutation logic.
//!
//! Owns the Rust equivalents of gamemd's cell-level tiberium view and mutation
//! boundaries. In a loaded YR map, `OverlayGrid` is the authority for both the
//! tiberium type and its raw 0..=11 density byte; there is no second store.

#[cfg(any(test, feature = "test-support"))]
pub(crate) mod test_support;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use test_support::TestPlacementContext as PlaceTiberiumContext;

use crate::map::bridge_facts::{BRIDGE_FLAG_DESTROYED_OR_RAMP, BRIDGE_FLAG_STRUCTURAL};
use crate::map::cell_index::NativeCellIdentity;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::rules::overlay_types::{OverlayTypeRegistry, TiberiumImageError};
use crate::rules::ruleset::RuleSet;
use crate::rules::tiberium_type::{TiberiumTypeId, TiberiumTypeRegistry};
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::StringInterner;
use crate::sim::miner::ResourceType;
use crate::sim::occupancy::{OccupancyGrid, RawCellOccupationGrid};
use crate::sim::ore_growth::OreGrowthState;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::overlay_mark_host::{OverlayRecalc, RuntimeOverlayError};
use crate::sim::rng::SimRng;
use crate::sim::world::{FrameEffects, Simulation};

/// 8 adjacent directions for spread: N, NE, E, SE, S, SW, W, NW.
pub(crate) const ADJACENT_OFFSETS: [(i32, i32); 8] = crate::util::direction::DIRECTION_DELTAS;

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
    pub dirty_cells: Option<&'a mut crate::sim::dirty_cells::DirtyCells>,
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
) -> Result<Option<TiberiumCellView>, TiberiumImageError> {
    let overlay = *overlay_grid.cell(cell.0, cell.1);
    let Some(overlay_id) = overlay.overlay_id else {
        return Ok(None);
    };
    let Some(tiberium_type) =
        overlay_registry.tiberium_type_for_overlay(tiberium_types, overlay_id)?
    else {
        return Ok(None);
    };
    tiberium_types
        .get(tiberium_type)
        .ok_or(TiberiumImageError::MissingType(tiberium_type))?;
    Ok(Some(TiberiumCellView {
        overlay_id,
        overlay_data: overlay.overlay_data,
        tiberium_type,
        resource_type: resource_type_for_tiberium_slot(tiberium_type),
        nominal_value: crate::map::tiberium_cell::tiberium_value(
            Some(overlay_id),
            overlay.overlay_data,
            overlay_registry,
            tiberium_types,
        )?,
    }))
}

/// DrawPips70A1FC..70A29C reads Storage slot1 as gems, independently of Image.
pub fn resource_type_for_tiberium_slot(slot: TiberiumTypeId) -> ResourceType {
    if slot.0 == 1 {
        ResourceType::Gem
    } else {
        ResourceType::Ore
    }
}

/// Existing live-object sources needed by the retail visible-building
/// exclusion in `CellClass::CanPlaceTiberium`, and the raw occupation planes
/// a new Overlay's Mark tests.
#[derive(Clone, Copy)]
pub struct TiberiumPlacementObjectContext<'a> {
    entities: &'a EntityStore,
    occupancy: &'a OccupancyGrid,
    /// Live `CellClass+0x124`/`+0x128` occupation planes, read by
    /// `CellClass::CheckCellPassability @ 0x004834A0` when
    /// `OverlayClass::Mark` admits the new Overlay (called at `0x005FD00C`;
    /// `0x005FD04F` for TIB01).
    raw_occupation: &'a RawCellOccupationGrid,
    rules: &'a RuleSet,
    interner: &'a StringInterner,
    /// Live terrain-object cell index (`TerrainObjectStore::cell_index`):
    /// the terrain half of the native `FirstObject` list.
    terrain_object_cells: &'a std::collections::BTreeMap<(u16, u16), u64>,
}

impl<'a> TiberiumPlacementObjectContext<'a> {
    pub(crate) fn new(
        entities: &'a EntityStore,
        occupancy: &'a OccupancyGrid,
        raw_occupation: &'a RawCellOccupationGrid,
        rules: &'a RuleSet,
        interner: &'a StringInterner,
        terrain_object_cells: &'a std::collections::BTreeMap<(u16, u16), u64>,
    ) -> Self {
        Self {
            entities,
            occupancy,
            raw_occupation,
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

/// What a new tiberium cell is admitted against: the resolved map terrain and
/// the live CellClass-style object view. There is no admission without both.
#[derive(Clone, Copy)]
pub struct NewTiberiumAdmission<'a> {
    resolved_terrain: &'a ResolvedTerrainGrid,
    live_objects: TiberiumPlacementObjectContext<'a>,
}

impl<'a> NewTiberiumAdmission<'a> {
    pub fn runtime(
        resolved_terrain: &'a ResolvedTerrainGrid,
        live_objects: TiberiumPlacementObjectContext<'a>,
    ) -> Self {
        Self {
            resolved_terrain,
            live_objects,
        }
    }

    /// The live CellClass-style object view this admission carries.
    pub(crate) fn live_objects(&self) -> TiberiumPlacementObjectContext<'a> {
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

/// The building half of `CellClass::CanPlaceTiberium` (`0x004838E0`,
/// `0x00483918..0x00483960`): a live Building (`+0x6C` > 0) whose type has
/// neither `Invisible=` (`+0xC9A`) nor `InvisibleInGame=` (`+0x1701`) refuses
/// ore.
///
/// Residual: native tests only the first Building in the cell's object list,
/// and VERA tests every Structure in the cell. Trigger: a cell whose first
/// Building is dead or invisible also holds a live, visible one. Effect:
/// native admits ore there and VERA refuses it. Frequency: none in ordinary
/// play, where building foundations never share a cell. Downstream: none.
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
        if entity.category() != EntityCategory::Structure || !entity.is_alive() {
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

/// Shared lookup and empty-cell admission for every production placement path.
///
/// Adjacent_Cell481810 ->GetCell5657A0 precedes CanPlace4838E0, even if
/// the source later refuses spreading. Resolve through the map owner first:
/// misses stamp the live dummy; fixed-grid aliases use the returned real
/// Cell's coordinate. A valid loaded map allocates every admitted diamond
/// cell, so a dummy cannot become a new resource target. Original edge
/// refusal controls: tools/spatial_oracle/ore_queue.md.
pub(crate) fn admit_new_tiberium_target(
    overlay_grid: &OverlayGrid,
    source_object_cells: &std::collections::BTreeSet<(u16, u16)>,
    admission: NewTiberiumAdmission<'_>,
    cell: (u16, u16),
) -> Option<(u16, u16)> {
    let identity = admission
        .resolved_terrain
        .native_cell_identity((cell.0 as i16, cell.1 as i16));
    if identity == NativeCellIdentity::Dummy {
        return None;
    }
    let coord = admission.resolved_terrain.native_cell_coord(identity);
    let cell = (coord.0 as u16, coord.1 as u16);
    if cell.0 >= overlay_grid.width()
        || cell.1 >= overlay_grid.height()
        || source_object_cells.contains(&cell)
        || overlay_grid.cell(cell.0, cell.1).overlay_id.is_some()
    {
        return None;
    }
    let terrain_cell = admission.resolved_terrain.cell(cell.0, cell.1)?;
    (resolved_cell_accepts_tiberium(terrain_cell)
        && !live_cell_rejects_tiberium(cell, admission.live_objects))
    .then_some(cell)
}

/// CellClass::CanSpreadTiberium483690 source admission, shared by queues
/// and the non-forced483780 caller. FirstObject includes non-Techno terrain.
pub(crate) fn can_spread_tiberium(
    overlay_grid: &OverlayGrid,
    overlay_registry: &OverlayTypeRegistry,
    tiberium_types: &TiberiumTypeRegistry,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    source_has_object: bool,
    rx: u16,
    ry: u16,
    spread_enabled: bool,
) -> Result<Option<TiberiumTypeId>, TiberiumImageError> {
    if !spread_enabled || source_has_object {
        return Ok(None);
    }
    let Some(view) = tiberium_cell_view(overlay_grid, overlay_registry, tiberium_types, (rx, ry))?
    else {
        return Ok(None);
    };
    let own_type = view.tiberium_type;
    if view.overlay_data <= own_type.0 / 2 {
        return Ok(None);
    }
    if resolved_terrain
        .and_then(|terrain| terrain.cell(rx, ry))
        .is_some_and(|cell| cell.slope_type != 0)
    {
        return Ok(None);
    }
    let ty = tiberium_types
        .get(own_type)
        .ok_or(TiberiumImageError::MissingType(own_type))?;
    Ok(
        crate::sim::ore_growth::native_percentage_admits(ty.spread_percentage_bits)
            .then_some(own_type),
    )
}

/// A completed PlaceTiberium call. The Overlay constructor and Mark have
/// already completed synchronously in the world's existing owners.
#[derive(Debug)]
pub(crate) struct TiberiumPlacement {
    cell: (u16, u16),
}
impl TiberiumPlacement {
    pub(crate) fn cell(&self) -> (u16, u16) {
        self.cell
    }
}

fn placement_admission<'a>(
    sim: &'a Simulation,
    rules: &'a RuleSet,
) -> Option<NewTiberiumAdmission<'a>> {
    Some(NewTiberiumAdmission::runtime(
        sim.resolved_terrain.as_ref()?,
        TiberiumPlacementObjectContext::new(
            &sim.substrate.entities,
            &sim.substrate.occupancy,
            &sim.substrate.raw_cell_occupation,
            rules,
            &sim.interner,
            sim.terrain_objects.cell_index(),
        ),
    ))
}

/// CellClass::PlaceTiberium487190. A full-world borrow permits its synchronous
/// Overlay5FC380 -> Mark5FC570 -> CellAnim/Start callbacks before queue/data stores.
/// The grid, queues, RNG and object lifecycle remain in their existing owners.
pub(crate) fn place_tiberium(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    cell: (u16, u16),
    type_id: TiberiumTypeId,
    amount: u8,
    effects: FrameEffects<'_>,
) -> Result<Option<TiberiumPlacement>, RuntimeOverlayError> {
    // An absent index or an entry without a usable Image= places nothing.
    let Some(ty) = rules.tiberium_types.live(type_id) else {
        return Ok(None);
    };
    //4871B6 tests the retained E4 before any image dereference or placement draw.
    if i32::from(amount) >= ty.image().density_levels() {
        return Ok(None);
    }
    let Some(grid) = sim.overlay_grid.as_ref() else {
        return Ok(None);
    };
    if cell.0 >= grid.width() || cell.1 >= grid.height() {
        return Ok(None);
    }
    if grid.cell(cell.0, cell.1).overlay_id.is_none() {
        return place_new_tiberium(sim, rules, registry, cell, type_id, amount, effects);
    }
    //48738A precedes the first OverlayToTiberiumIndex call at487390.
    if !sim.ore_growth_config.grows() {
        return Ok(None);
    }
    let flat = sim.resolved_terrain.as_ref().is_none_or(|terrain| {
        terrain
            .cell(cell.0, cell.1)
            .is_some_and(|cell| cell.slope_type == 0)
    });
    let Some(view) = tiberium_cell_view(grid, registry, &rules.tiberium_types, cell)? else {
        return Ok(None);
    };
    let limit = ty.image().density_levels().wrapping_sub(1);
    if !flat
        || view.tiberium_type != type_id
        || i32::from(view.overlay_data) >= limit
        || !crate::sim::ore_growth::native_percentage_admits(ty.growth_percentage_bits)
    {
        return Ok(None);
    }
    let data = i32::from(view.overlay_data.wrapping_add(amount)).min(limit) as u8;
    let grid = sim.overlay_grid.as_mut().expect("read live overlay grid");
    grid.set_overlay_data(cell.0, cell.1, data);
    sim.dirty_cells.mark_tactical_once(cell);
    let objects =
        NativeCellObjectView::new(&sim.substrate.occupancy, sim.terrain_objects.cell_index());
    sim.ore_growth_state.add_native_spread_queue_cell(
        type_id,
        grid,
        registry,
        &rules.tiberium_types,
        sim.resolved_terrain.as_ref(),
        sim.terrain_objects.spawning_cells(),
        Some(objects),
        cell.0,
        cell.1,
        sim.session.binary_frame(),
        sim.ore_growth_config.spreads(),
        &mut sim.scenario_rng,
    )?;
    Ok(Some(TiberiumPlacement { cell }))
}

/// The new-cell arm of PlaceTiberium487190, also inlined by VoxelAnim74A385/74A5F0.
/// CanPlaceTiberium4839B6 rejects slopes before this arm; keep that admission.
pub(crate) fn place_new_tiberium(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    requested: (u16, u16),
    type_id: TiberiumTypeId,
    amount: u8,
    effects: FrameEffects<'_>,
) -> Result<Option<TiberiumPlacement>, RuntimeOverlayError> {
    let Some(ty) = rules.tiberium_types.live(type_id) else {
        return Ok(None);
    };
    let admitted = placement_admission(sim, rules).and_then(|admission| {
        admit_new_tiberium_target(
            sim.overlay_grid.as_ref()?,
            sim.terrain_objects.spawning_cells(),
            admission,
            requested,
        )
    });
    let Some(cell) = admitted else {
        return Ok(None);
    };
    //487252..487286: one literal flat draw before the selected pointer access.
    let variant = sim.scenario_rng.next_range_u32(12);
    // An `[OverlayTypes]` list shorter than the image run leaves the drawn
    // slot absent (gamemd reads past its array at 487286): keep the draw,
    // place nothing.
    let Ok(overlay_id) = registry.tiberium_placement_overlay_id(ty, variant) else {
        static REPORTED: std::sync::Once = std::sync::Once::new();
        REPORTED.call_once(|| {
            log::warn!(
                "Tiberium {} image slot {variant} is past the end of [OverlayTypes]; \
                 nothing placed",
                ty.section
            );
        });
        return Ok(None);
    };
    // Reuse the sole runtime Overlay5FC380 owner, including the constructor ID
    // BEFORE a nested CellAnim's ID, all Mark arms, refusal and deferred cleanup.
    crate::sim::crates::construct_runtime_overlay(
        sim,
        &rules.crate_rules,
        rules,
        registry,
        cell,
        overlay_id,
        OverlayRecalc::DeferOrdinary,
        effects,
    )?;
    let grid = sim
        .overlay_grid
        .as_mut()
        .expect("admitted live overlay grid");
    sim.ore_growth_state.add_native_growth_queue_cell(
        grid,
        registry,
        &rules.tiberium_types,
        sim.resolved_terrain.as_ref(),
        sim.ore_growth_config.grows(),
        type_id,
        cell.0,
        cell.1,
        sim.session.binary_frame(),
        &mut sim.scenario_rng,
    )?;
    //4872A0 writes this byte even when the Overlay constructor refused Unlimbo.
    grid.write_cell_data(cell.0, cell.1, amount);
    sim.dirty_cells.mark_tactical_once(cell);
    sim.dirty_cells.mark_radar([cell]);
    Ok(Some(TiberiumPlacement { cell }))
}

/// One port of CellClass::SpreadTiberium483780 for terrain's force1 call
/// and the growth scheduler's force0 call. Forced calls bypass source admission;
/// both resolve the source's live resource type (fallback0), draw one starting
/// direction and scan the native direction order for an empty admitted cell.
/// Original execution: tools/spatial_oracle/tibtre.json and growth/spread corpus.
///
/// `spawner` carries Phobos' `TerrainTypeTemp::pCurrentExt` context: only the
/// ore spawner's own forced calls set it, and only then do the two hooks below
/// fire. Every other caller passes `None` and keeps the stock scan.
pub(crate) fn spread_tiberium(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    source: (u16, u16),
    forced: bool,
    spawner: Option<SpawnerSpread>,
    effects: FrameEffects<'_>,
) -> Result<Option<TiberiumPlacement>, RuntimeOverlayError> {
    let Some(grid) = sim.overlay_grid.as_ref() else {
        return Ok(None);
    };
    let source_type = match (spawner, forced) {
        (Some(spawner), true) => {
            let resolved = tiberium_cell_view(grid, registry, &rules.tiberium_types, source)?;
            let source_type = match resolved {
                Some(view) => view.tiberium_type,
                None => match u8::try_from(spawner.tiberium_type) {
                    Ok(id) => TiberiumTypeId(id),
                    Err(_) => return Ok(None),
                },
            };
            return spawner_cell_spread(
                sim,
                rules,
                registry,
                source,
                source_type,
                spawner,
                effects,
            );
        }
        (None, true) => tiberium_cell_view(grid, registry, &rules.tiberium_types, source)?
            .map_or(TiberiumTypeId(0), |view| view.tiberium_type),
        (None, false) => {
            let Some(id) = can_spread_tiberium(
                grid,
                registry,
                &rules.tiberium_types,
                sim.resolved_terrain.as_ref(),
                crate::sim::ore_growth::cell_has_native_object(
                    sim.terrain_objects.spawning_cells(),
                    Some(NativeCellObjectView::new(
                        &sim.substrate.occupancy,
                        sim.terrain_objects.cell_index(),
                    )),
                    source,
                ),
                source.0,
                source.1,
                sim.ore_growth_config.spreads(),
            )?
            else {
                return Ok(None);
            };
            id
        }
        (Some(_), false) => return Ok(None),
    };
    let start = sim.scenario_rng.next_range_u32(8) as usize;
    for step in 0..8 {
        let (dx, dy) = ADJACENT_OFFSETS[(start + step) % 8];
        let requested = (
            (source.0 as i16).wrapping_add(dx as i16) as u16,
            (source.1 as i16).wrapping_add(dy as i16) as u16,
        );
        let target = placement_admission(sim, rules).and_then(|admission| {
            admit_new_tiberium_target(
                sim.overlay_grid.as_ref()?,
                sim.terrain_objects.spawning_cells(),
                admission,
                requested,
            )
        });
        let Some(target) = target else {
            continue;
        };
        //48385C returns the first admitted placement; no retry on refusal/error.
        return place_tiberium(sim, rules, registry, target, source_type, 3, effects);
    }
    Ok(None)
}

/// Phobos `TerrainTypeExt`'s live `SpawnsTiberium.*` context for one forced
/// spread: the fields `CellClass_SpreadTiberium_CellSpread @ 0x48381D` reads.
/// Only a Phobos build ever builds one; the stock scan keeps `None`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SpawnerSpread {
    /// `SpawnsTiberium.Type=`: the `[Tiberiums]` index used as the fallback.
    pub tiberium_type: i32,
    /// `SpawnsTiberium.Range=`: the CellSpread radius the scan searches.
    pub range: i32,
    /// `SpawnsTiberium.GrowthStage=`: the density range, drawn per admitted
    /// cell and clamped to the resource's own frame count.
    pub growth_stage: crate::rules::ini_value::PartialIntPair,
}

/// `CellClass_SpreadTiberium_CellSpread @ 0x48381D`: Phobos replaces the six
/// bytes that load `TiberiumClass::Array`, the `Random__RandomRanged
/// @ 0x0065C7E0(0,7)` at `0x00483839` and the whole eight-direction loop, then
/// returns the function's own epilogue at `0x004838CA` or `0x004838B0`. The
/// replacement draws one start index over the CellSpread ring list, takes the
/// first cell `CanTiberiumGerminate @ 0x004838E0` admits, and calls
/// `IncreaseTiberium @ 0x00487190` with the drawn growth stage as its amount.
/// Like the stock scan it returns that first placement result and never retries
/// another neighbour.
fn spawner_cell_spread(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    source: (u16, u16),
    type_id: TiberiumTypeId,
    spawner: SpawnerSpread,
    effects: FrameEffects<'_>,
) -> Result<Option<TiberiumPlacement>, RuntimeOverlayError> {
    // `SpawnsTiberium.Type=` naming an absent or image-less entry: no spread.
    let Some(ty) = rules.tiberium_types.live(type_id) else {
        return Ok(None);
    };
    let density = ty.image().density_levels();
    if density <= 0 {
        return Err(TiberiumImageError::InvalidDivisor {
            tiberium: type_id,
            field: "NumFrames",
            value: density,
        }
        .into());
    }
    let max_stage = density - 1;
    let offsets = adjacent_cells_in_range(spawner.range);
    let size = offsets.len();
    let start = sim
        .scenario_rng
        .next_range_i32_inclusive(0, size as i32 - 1) as usize;
    for step in 0..size {
        let (dx, dy) = offsets[(step + start) % size];
        let requested = (
            (source.0 as i16).wrapping_add(dx) as u16,
            (source.1 as i16).wrapping_add(dy) as u16,
        );
        let target = placement_admission(sim, rules).and_then(|admission| {
            admit_new_tiberium_target(
                sim.overlay_grid.as_ref()?,
                sim.terrain_objects.spawning_cells(),
                admission,
                requested,
            )
        });
        let Some(target) = target else {
            continue;
        };
        let stage = if spawner.growth_stage.is_single() {
            spawner.growth_stage.low
        } else {
            sim.scenario_rng
                .next_range_i32_inclusive(spawner.growth_stage.low, spawner.growth_stage.high)
        }
        .clamp(0, max_stage);
        let placed = place_tiberium(sim, rules, registry, target, type_id, stage as u8, effects)?;
        if placed.is_some() && stage >= max_stage {
            let objects = NativeCellObjectView::new(
                &sim.substrate.occupancy,
                sim.terrain_objects.cell_index(),
            );
            sim.ore_growth_state.add_native_spread_queue_cell(
                type_id,
                sim.overlay_grid.as_ref().expect("placed overlay grid"),
                registry,
                &rules.tiberium_types,
                sim.resolved_terrain.as_ref(),
                sim.terrain_objects.spawning_cells(),
                Some(objects),
                target.0,
                target.1,
                sim.session.binary_frame(),
                sim.ore_growth_config.spreads(),
                &mut sim.scenario_rng,
            )?;
        }
        return Ok(placed);
    }
    Ok(None)
}

fn adjacent_cells_in_range(range: i32) -> Vec<(i16, i16)> {
    crate::sim::cell_rect::cell_spread_offsets(range as u32 as usize)
}

/// Result of one `Reduce_Tiberium` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReduceTiberiumOutcome {
    pub tiberium_type: Option<TiberiumTypeId>,
    pub removed_amount: u16,
    pub resource_type: Option<ResourceType>,
    pub fully_removed: bool,
}

impl ReduceTiberiumOutcome {
    fn none() -> Self {
        Self {
            removed_amount: 0,
            tiberium_type: None,
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
) -> Result<ReduceTiberiumOutcome, TiberiumImageError> {
    if amount <= 0 {
        return Ok(ReduceTiberiumOutcome::none());
    }

    let (Some(grid), Some(registry), Some(types)) = (
        ctx.overlay_grid.as_deref(),
        ctx.overlay_registry,
        ctx.tiberium_types,
    ) else {
        // Cell overlay identity plus raw OverlayData is the only authority; an
        // incompletely initialized caller reduces nothing.
        return Ok(ReduceTiberiumOutcome::none());
    };

    // The caller's native Map lookup selects a CellClass, including packed
    // coordinate aliases. Resolve that identity before reading OverlayGrid;
    // an unallocated rectangular record is not the shared fallback object's
    // overlay. Legacy fixtures without terrain retain their grid-only input.
    let cell = if let Some(terrain) = ctx.resolved_terrain.as_deref() {
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        let target = cells.lookup((cell.0 as i16, cell.1 as i16));
        if target == NativeCellIdentity::Dummy {
            // RESIDUAL: a mineral-bearing shared Dummy needs Cell480A80's
            // separate overlay/recalc/live-coordinate response. This owner is
            // grid-backed; keep that Dummy resource unchanged rather than
            // reducing an unrelated rectangular overlay record. Trigger:
            // reduction of an unallocated coordinate while Dummy holds ore;
            // effect/risk: that ore and its downstream queue effects remain.
            return Ok(ReduceTiberiumOutcome::none());
        }
        let actual = cells.coord(target);
        (actual.0 as u16, actual.1 as u16)
    } else {
        cell
    };

    let Some(view) = tiberium_cell_view(grid, registry, types, cell)? else {
        // A non-tiberium overlay is an invalid reduction target.
        return Ok(ReduceTiberiumOutcome::none());
    };
    let current = view.overlay_data;

    // `CellClass::ReduceTiberium` @ 0x00480A80 calls
    // `TiberiumClass::RegisterForGrowth` @ 0x007235A0 at density 11 first.
    // Its `< 11` admission makes this live call a deliberate no-op: no queue
    // entry and no Scenario RNG draw are produced before the reduction.
    // The rejected call also precedes the enqueue rebuild check, so its
    // omission has no queue, timer or RNG effect (ore_queue.json reduction
    // density-11 controls). No growth context is needed by reduction.

    // Native partial predicate is signed `amount < current + 1`. This makes a
    // density-11 request of 11 leave the overlay present at density zero, while
    // 12 clears it. A positive request against data zero takes the full path.
    if amount < i32::from(current) + 1 {
        let remaining = current.wrapping_sub(amount as u8);
        if let Some(grid) = ctx.overlay_grid.as_deref_mut() {
            grid.set_overlay_data(cell.0, cell.1, remaining);
        }
        if let Some(dirty) = ctx.dirty_cells.as_deref_mut() {
            dirty.mark_tactical_once(cell);
        }
        return Ok(ReduceTiberiumOutcome {
            removed_amount: amount as u16,
            tiberium_type: Some(view.tiberium_type),
            resource_type: Some(view.resource_type),
            fully_removed: false,
        });
    }

    if let Some(grid) = ctx.overlay_grid.as_deref_mut() {
        grid.clear_overlay(cell.0, cell.1);
        if let (Some(terrain), Some(registry)) =
            (ctx.resolved_terrain.as_deref_mut(), ctx.overlay_registry)
        {
            // Retail recalculates cell attributes synchronously inside the full
            // removal boundary, before any later sim system can observe it.
            grid.recalculate_runtime_cell(terrain, registry, cell);
        }
    }

    // Retail dirties radar immediately after clear/recalc, before touching any
    // spread bitmap or queue state.
    if let Some(dirty) = ctx.dirty_cells.as_deref_mut() {
        dirty.mark_radar([cell]);
    }
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
            )?;
    }
    if let Some(dirty) = ctx.dirty_cells.as_deref_mut() {
        dirty.mark_tactical_once(cell);
    }

    Ok(ReduceTiberiumOutcome {
        removed_amount: u16::from(current),
        tiberium_type: Some(view.tiberium_type),
        resource_type: Some(view.resource_type),
        fully_removed: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    use crate::map::overlay::{OverlayDataPack, OverlayEntry};
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::overlay_types::OverlayTypeRegistry;
    use crate::rules::ruleset::RuleSet;
    use crate::rules::terrain_rules::LandType;
    use crate::rules::tiberium_type::TiberiumTypeRegistry;
    use crate::sim::entity_store::EntityStore;
    use crate::sim::intern::StringInterner;
    use crate::sim::occupancy::{OccupancyGrid, RawCellOccupationGrid};
    use crate::sim::rng::SimRng;
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::world::Simulation;

    // Legacy split-field fixtures call the sole production Simulation path.
    fn place_tiberium(
        ctx: &mut PlaceTiberiumContext<'_>,
        cell: (u16, u16),
        id: TiberiumTypeId,
        amount: u8,
    ) -> Option<TiberiumPlacement> {
        test_support::with_placement_fixture(ctx, |sim, rules, registry| {
            super::place_tiberium(
                sim,
                rules,
                registry,
                cell,
                id,
                amount,
                FrameEffects::default(),
            )
        })
        .expect("valid placement fixture")
    }
    fn spread_tiberium(
        ctx: &mut PlaceTiberiumContext<'_>,
        cell: (u16, u16),
        forced: bool,
        spawner: Option<SpawnerSpread>,
    ) -> Option<TiberiumPlacement> {
        test_support::with_placement_fixture(ctx, |sim, rules, registry| {
            super::spread_tiberium(
                sim,
                rules,
                registry,
                cell,
                forced,
                spawner,
                FrameEffects::default(),
            )
        })
        .expect("valid spread fixture")
    }
    fn tiberium_cell_view(
        grid: &OverlayGrid,
        registry: &OverlayTypeRegistry,
        types: &TiberiumTypeRegistry,
        cell: (u16, u16),
    ) -> Option<TiberiumCellView> {
        super::tiberium_cell_view(grid, registry, types, cell).expect("valid resource fixture")
    }
    fn reduce_tiberium(
        ctx: &mut ReduceTiberiumContext<'_>,
        cell: (u16, u16),
        amount: i32,
    ) -> ReduceTiberiumOutcome {
        super::reduce_tiberium(ctx, cell, amount).expect("valid reduction fixture")
    }

    /// Native Image721C5C, classification5FDD20 and Place487190/Overlay5FC380:
    /// Image selects numeric objects; cargo kind stays with the Tiberium slot.
    #[test]
    fn retained_image_uses_slots_and_runs_foreign_overlay_mark_synchronously() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::retail_ini_fixture::{
            retail_assets, retail_battle_rules_for_map_with_patch, retail_ini,
        };
        use crate::sim::native_identity::NativeUniqueIdCursor;
        use crate::sim::world::LoadOverlayHandle;

        let patch = IniFile::from_str(
            "[Tiberiums]\n0=Riparius\n1=Cruentus\n[Riparius]\nImage=2\n[Cruentus]\nImage=3\n",
        );
        let Some(retail) = retail_battle_rules_for_map_with_patch("Hills.mmx", &patch) else {
            return;
        };
        let registry = retail.rules.overlay_registry();
        let mut cells = OverlayGrid::new(32, 32);
        cells.place_overlay(10, 10, registry.id_for_name("GEM01").unwrap(), 3);
        cells.place_overlay(12, 10, registry.id_for_name("TIB2_01").unwrap(), 3);
        for (cell, slot, kind) in [
            ((10, 10), 0, ResourceType::Ore),
            ((12, 10), 1, ResourceType::Gem),
        ] {
            let view =
                super::tiberium_cell_view(&cells, registry, &retail.rules.tiberium_types, cell)
                    .unwrap()
                    .unwrap();
            assert_eq!(
                (view.tiberium_type, view.resource_type),
                (TiberiumTypeId(slot), kind)
            );
        }

        // Change only names/bodies at the twelve existing selected slots before
        // the production reader allocates them. No synthetic image fallback.
        let root = retail_ini("rulesmd.ini").unwrap();
        let art = retail_ini("artmd.ini").unwrap();
        let base = retail
            .rules
            .tiberium_types
            .get(TiberiumTypeId(0))
            .unwrap()
            .image()
            .base_overlay()
            .unwrap() as usize;
        let original_names: Vec<_> = root
            .section("OverlayTypes")
            .unwrap()
            .registry_ids()
            .into_iter()
            .skip(base)
            .take(12)
            .map(str::to_owned)
            .collect();
        assert_eq!(original_names.len(), 12);
        let mut definitions = String::new();
        let sections = root
            .section_names()
            .into_iter()
            .map(|name| {
                let mut section = root.section(name).unwrap().clone();
                if name == "OverlayTypes" {
                    let entries: Vec<_> = section
                        .raw_entries()
                        .map(|(k, v)| (k.to_owned(), v.to_owned()))
                        .collect();
                    for (key, value) in entries {
                        if let Some(index) = original_names.iter().position(|name| name == &value) {
                            let foreign = format!("FOREIGN{index:02}");
                            section.set(&key, &foreign);
                            definitions.push_str(&format!(
                                "[{foreign}]\nTiberium=no\nLand=Clear\nCellAnim=FIRE01\n"
                            ));
                        }
                    }
                }
                section
            })
            .collect::<Vec<_>>();
        let mut root = IniFile::from_sections(sections);
        root.merge(&IniFile::from_str(&definitions));
        let mut layers = RulesLayerStack::new(root);
        layers.push(RulesLayerKind::Scenario, patch);
        let processed = layers.process_with_fixed_art(&art).unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        rules.install_art_data(crate::rules::retail_ini_fixture::art_registry(&art));
        let (_, mut assets) = retail_assets().unwrap();
        crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
        assert_eq!(
            rules.bind_anim_class_assets(&["FIRE01".into()], &assets, "TEM", "TEMPERATE"),
            0,
            "the real CellAnim must bind without skipped types"
        );
        let registry = rules.overlay_registry().clone();
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 31);
        sim.install_resolved_terrain_for_new_map(test_support::flat_terrain(32, 32));
        sim.overlay_grid = Some(OverlayGrid::new(32, 32));
        sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(4000));
        sim.ore_growth_config =
            crate::sim::ore_growth::OreGrowthConfig::for_test(true, true, false);
        sim.ore_growth_state = crate::sim::ore_growth::OreGrowthState::new(32, 32);
        sim.ore_growth_state
            .reset_native_tiberium_classes(rules.tiberium_types.len(), 0);
        super::place_tiberium(
            &mut sim,
            &rules,
            &registry,
            (10, 10),
            TiberiumTypeId(0),
            3,
            FrameEffects::default(),
        )
        .unwrap()
        .expect("admitted numeric overlay placement");
        let stored = sim.overlay_grid.as_ref().unwrap().cell(10, 10);
        let selected = stored.overlay_id.unwrap();
        assert!(registry.name(selected).unwrap().starts_with("FOREIGN"));
        assert!(!registry.flags(selected).unwrap().tiberium);
        assert_eq!(stored.overlay_data, 3);
        let (&anim_id, anim) = sim
            .anims()
            .next()
            .expect("shared Mark creates the CellAnim");
        assert_eq!(sim.interner.resolve(anim.type_id()), "FIRE01");
        assert_eq!(anim.draw_flags(), 0x600);
        assert!(anim.in_logic_vector());
        let ids: Vec<_> = sim.load_objects.retained_overlay_ids().collect();
        assert_eq!(ids.len(), 1);
        let overlay = sim
            .load_objects
            .snapshot(
                LoadOverlayHandle::from_stable_id(ids[0]),
                &sim.substrate.pending_delete,
            )
            .unwrap();
        assert_eq!(
            overlay.native_id.map(|id| id.wrapping_add(1)),
            Some(anim.native_unique_id())
        );
        assert!(ids[0] < anim_id, "Overlay allocates before its CellAnim");
        assert!(overlay.queued && !overlay.alive);
        assert_eq!(
            sim.ore_growth_state.native_tiberium_state().classes[0]
                .growth
                .len(),
            1
        );
        let cold_only =
            RuleSet::from_ini(&IniFile::from_str("[Tiberiums]\n0=Cold\n[Cold]\nValue=1\n"))
                .unwrap();
        let mut empty = Simulation::new();
        let before = empty.scenario_rng.native_state_hex();
        assert!(
            super::place_tiberium(
                &mut empty,
                &cold_only,
                cold_only.overlay_registry(),
                (10, 10),
                TiberiumTypeId(0),
                0,
                FrameEffects::default()
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(
            empty.scenario_rng.native_state_hex(),
            before,
            "cold E4=0 rejects amount0 before any RNG"
        );

        //48738A's growth flag precedes classification. The earlier entry has
        //no Image=, so it is inert and the lookup finds no Tiberium.
        let cold = crate::rules::tiberium_type::with_tiberium_image_slots(&IniFile::from_str(
            "[Tiberiums]\n0=Cold\n1=Bound\n[Cold]\nValue=1\n[Bound]\nImage=1\n\
             [Animations]\n0=FIRE01\n[OverlayTypes]\n0=ORE\n\
             [ORE]\nTiberium=yes\nLand=Road\nCellAnim=FIRE01\n",
        ));
        let mut cold = RuleSet::from_ini_with_fixed_art_for_test(&cold, &art).unwrap();
        cold.install_art_data(crate::rules::retail_ini_fixture::art_registry(&art));
        assert_eq!(
            cold.bind_anim_class_assets(&["FIRE01".into()], &assets, "TEM", "TEMPERATE"),
            0
        );
        let mut blocked = Simulation::new();
        blocked.overlay_grid = Some(OverlayGrid::new(32, 32));
        blocked
            .overlay_grid
            .as_mut()
            .unwrap()
            .place_overlay(10, 10, 0, 0);
        blocked.resolved_terrain = Some(test_support::flat_terrain(32, 32));
        blocked.ore_growth_config =
            crate::sim::ore_growth::OreGrowthConfig::for_test(false, true, false);
        let rng = blocked.scenario_rng.clone();
        assert!(
            super::place_tiberium(
                &mut blocked,
                &cold,
                cold.overlay_registry(),
                (10, 10),
                TiberiumTypeId(1),
                1,
                FrameEffects::default()
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(
            blocked.scenario_rng.native_state_hex(),
            rng.native_state_hex()
        );
        blocked.ore_growth_config =
            crate::sim::ore_growth::OreGrowthConfig::for_test(true, true, false);
        assert!(
            super::place_tiberium(
                &mut blocked,
                &cold,
                cold.overlay_registry(),
                (10, 10),
                TiberiumTypeId(1),
                1,
                FrameEffects::default()
            )
            .unwrap()
            .is_none(),
            "slot0 is inert, so the stored overlay is no Tiberium"
        );

        // Mark5FD1AE constructs the CellAnim (delay0 -> Start424CE0)
        // BEFORE GetTiberiumType5FD1BB; the inert entry does not stop Mark.
        // ReadINI5FE8C9..5FE8E7 coerces Tiberium + Land=Clear to Tiberium
        // land. Road avoids that independent, earlier germination lookup.
        blocked.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(5000));
        assert!(
            crate::sim::crates::construct_runtime_overlay(
                &mut blocked,
                &cold.crate_rules,
                &cold,
                cold.overlay_registry(),
                (12, 12),
                0,
                OverlayRecalc::Immediate,
                FrameEffects::default(),
            )
            .is_ok()
        );
        let (&child_id, child) = blocked
            .anims()
            .next()
            .expect("CellAnim allocation must precede the Tiberium lookup");
        let overlays: Vec<_> = blocked.load_objects.retained_overlay_ids().collect();
        assert_eq!(overlays.len(), 1);
        let overlay = blocked
            .load_objects
            .snapshot(
                LoadOverlayHandle::from_stable_id(overlays[0]),
                &blocked.substrate.pending_delete,
            )
            .unwrap();
        assert_eq!(
            overlay.native_id.map(|id| id.wrapping_add(1)),
            Some(child.native_unique_id())
        );
        assert_eq!(child.runtime().delay_remaining, 0);
        assert!(child.start_sound_active());
        assert!(
            blocked.sound_events.iter().any(|event| matches!(
                event,
                crate::sim::world::SimSoundEvent::AnimationStarted { anim_id, sound_id, world }
                    if *anim_id == child_id
                        && sound_id == "BUILDINGFIREBIG"
                        && *world == child.world_coord()
            )),
            "retail FIRE01 Start must emit its sound before classification"
        );
    }

    /// `SpawnsTiberium.Type=` may name a `[Tiberiums]` entry without `Image=`
    /// (`TiberiumClass::ReadINI @ 0x00721C3F`, default -1): nothing is placed.
    #[test]
    fn spawner_spread_from_an_entry_without_image_places_nothing() {
        let ini = crate::rules::tiberium_type::with_tiberium_image_slots(&IniFile::from_str(
            "[Tiberiums]\n0=Bound\n1=NoImage\n[Bound]\nImage=1\n[NoImage]\nValue=5\n",
        ));
        let rules = RuleSet::from_ini(&ini).unwrap();
        let mut sim = Simulation::new();
        sim.overlay_grid = Some(OverlayGrid::new(8, 8));
        sim.resolved_terrain = Some(test_support::flat_terrain(8, 8));
        let spawner = SpawnerSpread {
            tiberium_type: 1,
            range: 1,
            growth_stage: crate::rules::ini_value::PartialIntPair::single(3),
        };
        let placed = super::spread_tiberium(
            &mut sim,
            &rules,
            rules.overlay_registry(),
            (4, 4),
            true,
            Some(spawner),
            FrameEffects::default(),
        )
        .expect("an entry without Image= must not stop the frame");
        assert!(placed.is_none());
    }

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
        // Retail's [Tiberium] row, the Land every ore overlay here copies. An
        // unauthored one is the zero row, whose Wheel 0 makes ore impassable.
        let mut ini_text = format!(
            "\
[Tiberium]
Foot=90%
Track=70%
Wheel=50%
Float=0%
FloatBeach=0%
Hover=50%
Amphibious=50%
Buildable=no

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
        flat_clear_terrain_grid(1, 1)
    }

    fn flat_clear_terrain_grid(width: u16, height: u16) -> ResolvedTerrainGrid {
        crate::map::resolved_terrain::test_grid(width, height, |rx, ry| ResolvedTerrainCell {
            allows_tiberium: true,
            ..crate::map::resolved_terrain::test_loader_clear_cell(rx, ry)
        })
    }

    /// Phobos' two `CellClass::SpreadTiberium @ 0x00483780` hooks on one forced
    /// spawner call: `CellClass_SpreadTiberium_TiberiumType @ 0x00483811`
    /// replaces the stock Riparius fallback with `SpawnsTiberium.Type=`, and
    /// `CellClass_SpreadTiberium_CellSpread @ 0x0048381D` searches the
    /// CellSpread disk for `SpawnsTiberium.Range=` and germinates the drawn
    /// `SpawnsTiberium.GrowthStage=` as the placement amount.
    #[test]
    fn spawner_hooks_replace_the_fallback_type_scan_and_stage() {
        let (overlay_registry, tiberium_types) = native_tiberium_fixture();
        let mut overlay = OverlayGrid::new(16, 16);
        // Every stock neighbour already holds ore, so the eight-direction scan
        // can only refuse: a placement proves the wider CellSpread search.
        for (dx, dy) in ADJACENT_OFFSETS {
            let cell = overlay.cell_mut_for_test((8 + dx as i32) as u16, (8 + dy as i32) as u16);
            cell.overlay_id = Some(105);
            cell.overlay_data = 3;
        }
        let mut growth = OreGrowthState::new(16, 16);
        growth.reset_native_tiberium_classes(tiberium_types.len(), 40);
        let mut rng = SimRng::new(0x51);
        let source_cells = BTreeSet::from([(8, 8)]);
        let terrain = test_support::flat_terrain(16, 16);
        let no_objects = test_support::NoLiveObjects::new();
        let admission = NewTiberiumAdmission::runtime(&terrain, no_objects.context());
        let mut ctx = PlaceTiberiumContext {
            overlay_grid: &mut overlay,
            ore_growth_state: &mut growth,
            overlay_registry: &overlay_registry,
            tiberium_types: &tiberium_types,
            resolved_terrain: Some(&terrain),
            source_object_cells: &source_cells,
            new_cell_admission: Some(admission),
            live_objects: Some(admission.live_objects().object_view()),
            rng: &mut rng,
            binary_frame: 40,
            growth_enabled: true,
            spread_enabled: true,
            dirty_cells: None,
        };
        let placed = spread_tiberium(
            &mut ctx,
            (8, 8),
            true,
            Some(SpawnerSpread {
                tiberium_type: 1,
                range: 2,
                growth_stage: crate::rules::ini_value::PartialIntPair::single(7),
            }),
        )
        .expect("a range-2 spawner reaches the ring its stock scan never walks");
        let cell = placed.cell();
        let (dx, dy) = ((cell.0 as i32 - 8).abs(), (cell.1 as i32 - 8).abs());
        assert_eq!(
            dx.max(dy) + dx.min(dy) / 2,
            2,
            "the admitted cell sits in the second CellSpread ring: {cell:?}"
        );
        let view = tiberium_cell_view(ctx.overlay_grid, &overlay_registry, &tiberium_types, cell)
            .expect("the germinated cell resolves to a resource");
        assert_eq!(view.tiberium_type, TiberiumTypeId(1), "SpawnsTiberium.Type");
        assert_eq!(view.overlay_data, 7, "SpawnsTiberium.GrowthStage");
        // Native48380C/48380F bypass the fallback when the source has ore.
        // An invalid, unused SpawnsTiberium.Type must not stop that spread.
        let source_overlay = overlay_registry
            .flat_tiberium_variant_ids(tiberium_types.get(TiberiumTypeId(1)).unwrap())
            .unwrap()[0];
        ctx.overlay_grid.place_overlay(8, 8, source_overlay, 11);
        assert!(
            spread_tiberium(
                &mut ctx,
                (8, 8),
                true,
                Some(SpawnerSpread {
                    tiberium_type: -1,
                    range: 2,
                    growth_stage: crate::rules::ini_value::PartialIntPair::single(7),
                }),
            )
            .is_some()
        );
    }

    #[test]
    fn negative_spawner_range_uses_the_unsigned_enumerator_limit() {
        assert_eq!(adjacent_cells_in_range(-1), adjacent_cells_in_range(0x100));
    }

    /// Phobos `GeneralUtils::AdjacentCellsInRange` covers gamemd's own
    /// CellSpread disk, but in `CellSpreadEnumerator`'s order: the center
    /// first, then ring 1 with the startup table's offsets 2 and 4 swapped —
    /// the difference that enumerator's own header states.
    #[test]
    fn adjacent_cells_in_range_keeps_the_enumerators_ring_one_order() {
        assert_eq!(adjacent_cells_in_range(0), vec![(0, 0)]);
        assert_eq!(
            adjacent_cells_in_range(1),
            vec![
                (0, 0),
                (-1, -1),
                (0, -1),
                (1, -1),
                (-1, 0),
                (1, 0),
                (-1, 1),
                (0, 1),
                (1, 1),
            ]
        );

        use std::collections::BTreeSet;
        let cells = adjacent_cells_in_range(2);
        let splash = crate::sim::combat::cell_spread::splash_cells(2.0);
        assert_eq!(cells.len(), splash.len(), "the second ring holds 12 cells");
        assert_eq!(
            cells.iter().copied().collect::<BTreeSet<_>>(),
            splash.iter().copied().collect::<BTreeSet<_>>(),
            "a range-2 spawner covers the same disk as a CellSpread=2 warhead"
        );
    }

    #[test]
    fn gsi_04_09_new_placement_requires_proof_and_rejects_outside_playfield() {
        let (overlay_registry, tiberium_types) = native_tiberium_fixture();
        let mut terrain = flat_clear_terrain();
        terrain.cell_mut_for_test(0, 0).unwrap().outside_playfield = true;
        let rules_ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n",
        );
        let rules = RuleSet::from_ini(&rules_ini).expect("rules");
        let entities = EntityStore::new();
        let occupancy = OccupancyGrid::new();
        let interner = StringInterner::default();
        let terrain_object_cells = BTreeMap::new();
        let raw_occupation = RawCellOccupationGrid::new();
        let live_objects = TiberiumPlacementObjectContext::new(
            &entities,
            &occupancy,
            &raw_occupation,
            &rules,
            &interner,
            &terrain_object_cells,
        );
        let runtime_admission = NewTiberiumAdmission::runtime(&terrain, live_objects);

        let mut overlay = OverlayGrid::new(1, 1);
        let mut growth = OreGrowthState::new(1, 1);
        growth.reset_native_tiberium_classes(tiberium_types.len(), 40);
        let source_cells = BTreeSet::new();
        let mut rng = SimRng::new(0x409);
        let before_rng = rng.logical_state();
        let mut dirty = crate::sim::dirty_cells::DirtyCells::default();

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
                dirty_cells: Some(&mut dirty),
            };
            assert!(place_tiberium(&mut ctx, (0, 0), TiberiumTypeId(0), 3).is_none());
        }

        assert_eq!(overlay.cell(0, 0).overlay_id, None);
        assert!(growth.native_tiberium_state().classes.iter().all(|class| {
            class.growth.is_empty()
                && class.spread.is_empty()
                && class.growth_bitmap.is_empty()
                && class.spread_bitmap.is_empty()
        }));
        assert!(dirty.radar().is_empty());
        assert_eq!(dirty.radar_generation(), 0);
        assert!(dirty.tactical_for_test().is_empty());
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
        let terrain = test_support::flat_terrain(8, 8);
        let no_objects = test_support::NoLiveObjects::new();
        let admission = NewTiberiumAdmission::runtime(&terrain, no_objects.context());
        let mut dirty = crate::sim::dirty_cells::DirtyCells::default();
        {
            let mut ctx = PlaceTiberiumContext {
                overlay_grid: &mut overlay,
                ore_growth_state: &mut growth,
                overlay_registry: &overlay_registry,
                tiberium_types: &tiberium_types,
                resolved_terrain: Some(&terrain),
                source_object_cells: &source_cells,
                new_cell_admission: Some(admission),
                live_objects: Some(admission.live_objects().object_view()),
                rng: &mut rng,
                binary_frame: 40,
                growth_enabled: true,
                spread_enabled: true,
                dirty_cells: Some(&mut dirty),
            };
            assert!(place_tiberium(&mut ctx, (4, 4), TiberiumTypeId(0), 11).is_some());
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
        assert_eq!(dirty.radar(), vec![(4, 4)]);
        assert_eq!(dirty.radar_generation(), 1);
        assert_eq!(dirty.tactical_for_test(), vec![(4, 4)]);

        let state_before_reject = rng.logical_state();
        let mut reject_ctx = PlaceTiberiumContext {
            overlay_grid: &mut overlay,
            ore_growth_state: &mut growth,
            overlay_registry: &overlay_registry,
            tiberium_types: &tiberium_types,
            resolved_terrain: Some(&terrain),
            source_object_cells: &source_cells,
            new_cell_admission: Some(admission),
            live_objects: Some(admission.live_objects().object_view()),
            rng: &mut rng,
            binary_frame: 40,
            growth_enabled: true,
            spread_enabled: true,
            dirty_cells: Some(&mut dirty),
        };
        assert!(place_tiberium(&mut reject_ctx, (5, 5), TiberiumTypeId(0), 12,).is_none());
        assert_eq!(rng.logical_state(), state_before_reject);
        assert_eq!(overlay.cell(5, 5).overlay_id, None);
    }

    #[test]
    fn gsi_04_09_existing_growth_honors_threshold_clamp_and_tactical_only_dirty() {
        // ReadDouble scans a float: `.00001` widens to 9.99999974737875e-06,
        // just under the 1e-05 gate; `.000011` clears it.
        for (growth_percentage, succeeds) in
            [(".000009", false), (".00001", false), (".000011", true)]
        {
            let (overlay_registry, tiberium_types) =
                native_tiberium_fixture_with_riparius_growth(growth_percentage);
            let tib01 = overlay_registry.id_for_name("TIB01").expect("TIB01");
            let mut overlay = OverlayGrid::new(8, 8);
            overlay.place_overlay(4, 4, tib01, 10);
            let mut growth = OreGrowthState::new(8, 8);
            growth.reset_native_tiberium_classes(tiberium_types.len(), 0);
            let mut rng = SimRng::new(12);
            let source_cells = BTreeSet::new();
            let mut dirty = crate::sim::dirty_cells::DirtyCells::default();
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
                dirty_cells: Some(&mut dirty),
            };

            assert_eq!(
                place_tiberium(&mut ctx, (4, 4), TiberiumTypeId(0), 7).is_some(),
                succeeds,
                "GrowthPercentage={growth_percentage}"
            );
            assert_eq!(
                overlay.cell(4, 4).overlay_data,
                if succeeds { 11 } else { 10 }
            );
            assert!(dirty.radar().is_empty());
            assert_eq!(dirty.radar_generation(), 0);
            assert_eq!(
                dirty.tactical_for_test(),
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
                (5, 0, 0xEE),
            ]);
            OverlayGrid::from_generated_overlays(&entries, &data, &mut terrain, &overlay_registry)
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
            (grid.cell(5, 0).overlay_id, grid.cell(5, 0).overlay_data),
            (None, 0),
            "unallocated native slot receives neither pack"
        );
        assert_eq!(
            grid.take_dirty_cells(),
            Vec::new(),
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
            let mut overlay = OverlayGrid::new(8, 8);
            overlay.place_overlay(4, 4, tib01, data);
            let mut growth = OreGrowthState::new(8, 8);
            growth.reset_native_tiberium_classes(tiberium_types.len(), 0);
            let mut dirty = crate::sim::dirty_cells::DirtyCells::default();
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
                dirty_cells: Some(&mut dirty),
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
                    assert!(
                        dirty.radar().is_empty(),
                        "partial mutation is tactical-only"
                    );
                    assert_eq!(dirty.radar_generation(), 0);
                }
                None => {
                    assert_eq!(overlay.cell(4, 4).overlay_id, None);
                    assert_eq!(dirty.radar(), vec![(4, 4)]);
                    assert_eq!(dirty.radar_generation(), 1);
                }
            }
            assert_eq!(dirty.tactical_for_test(), vec![(4, 4)]);
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
            dirty_cells: None,
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
            dirty_cells: None,
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
        use crate::sim::world::TickLane;
        use std::sync::Arc;

        let (overlay_registry, tiberium_types) = native_tiberium_fixture();
        let mut rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n",
        ))
        .expect("empty roster rules");
        rules.tiberium_types = tiberium_types;
        let mut sim = Simulation::new();
        sim.hash_state_every(std::num::NonZeroU64::MIN);
        sim.ore_growth_config.set_grows_for_test(false);
        sim.ore_growth_config.set_spreads_for_test(false);
        sim.ore_growth_state = OreGrowthState::new(1, 1);
        sim.ore_growth_state
            .reset_native_tiberium_classes(rules.tiberium_types.len(), 0);
        let mut terrain = flat_clear_terrain();
        let cell = terrain.cell_mut_for_test(0, 0).unwrap();
        cell.speed_costs.foot = Some(crate::rules::terrain_rules::test_speed_percent(37));
        cell.base_speed_costs.foot = Some(crate::rules::terrain_rules::test_speed_percent(37));
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
        let before = sim.path_grid_snapshot().unwrap();

        let outcome = crate::sim::world::Simulation::reduce_tiberium_at_with_native_context(
            &sim.terrain_objects,
            &sim.ore_growth_config,
            &mut sim.ore_growth_state,
            &sim.session,
            &mut sim.scenario_rng,
            &sim.substrate,
            &mut sim.resolved_terrain,
            &mut sim.overlay_grid,
            &mut sim.dirty_cells,
            (0, 0),
            4,
            &rules,
            Some(&overlay_registry),
        )
        .unwrap();
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
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .cell(0, 0)
                .unwrap()
                .speed_costs
                .foot,
            Some(crate::rules::terrain_rules::test_speed_percent(37)),
            "the restored land row is the cell's own, read without publication"
        );
        let repeated = sim.overlay_grid.as_mut().unwrap().recalculate_runtime_cell(
            sim.resolved_terrain.as_mut().unwrap(),
            &overlay_registry,
            (0, 0),
        );
        assert!(!repeated.navigation_changed);

        let deferred = sim
            .advance_app_frame(
                &[],
                &rules,
                None,
                67,
                TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
            .expect("fixture frame must complete");
        assert!(deferred.overlay_updates.is_empty());
        let published = sim.path_grid_snapshot().unwrap();
        assert!(
            Arc::ptr_eq(&before, &published),
            "the walkable path cell is current; only its land row changes"
        );
        let mut pending = sim.overlay_grid.as_ref().unwrap().clone();
        assert_eq!(
            pending.take_dirty_cells(),
            vec![(0, 0)],
            "presentation dirtiness waits for the registry"
        );
        assert!(pending.take_synchronous_navigation_cells().is_empty());

        let first = sim
            .advance_app_frame(
                &[],
                &rules,
                Some(&overlay_registry),
                67,
                TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
            .expect("fixture frame must complete");
        assert!(sim.zone_grid.is_some());
        assert!(
            first.overlay_updates.is_empty(),
            "erased tiberium has no overlay upsert"
        );
        assert_eq!(first.tick.state_hash, Some(sim.state_hash()));
        assert!(Arc::ptr_eq(&published, &sim.path_grid_snapshot().unwrap()));
        let mut pending = sim.overlay_grid.as_ref().unwrap().clone();
        assert!(pending.take_dirty_cells().is_empty());
        assert!(pending.take_synchronous_navigation_cells().is_empty());
        let second = sim
            .advance_app_frame(
                &[],
                &rules,
                Some(&overlay_registry),
                67,
                TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
            .expect("fixture frame must complete");
        assert!(second.overlay_updates.is_empty());
        assert!(
            Arc::ptr_eq(&published, &sim.path_grid_snapshot().unwrap()),
            "no duplicate navigation publication"
        );
        assert_eq!(second.tick.state_hash, Some(sim.state_hash()));
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
            dirty_cells: None,
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
                    overlay_registry
                        .tiberium_type_for_overlay(&tiberium_types, overlay_id)
                        .unwrap()
                        .unwrap(),
                    &overlay,
                    &overlay_registry,
                    &tiberium_types,
                    None,
                    &source_object_cells,
                    None,
                    5,
                    5,
                    100,
                    true,
                    &mut rng,
                )
                .unwrap()
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
            dirty_cells: None,
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
