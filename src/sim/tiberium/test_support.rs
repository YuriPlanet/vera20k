//! Shared ore fixtures for tests.
//!
//! Tiberium has one authority: the `OverlayGrid` cell, read through the
//! overlay and tiberium type registries. Tests seed ore the way a map does, by
//! placing a tiberium overlay with a density byte, never through a side store.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::map::resolved_terrain::{
    ResolvedTerrainCell, ResolvedTerrainGrid, test_retail_clear_speed_costs,
};
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::StringInterner;
use crate::sim::miner::ResourceType;
use crate::sim::occupancy::{OccupancyGrid, RawCellOccupationGrid};
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::tiberium::TiberiumPlacementObjectContext;
use crate::sim::world::Simulation;

/// Side length of the overlay grid [`place_tiberium`] creates on demand.
pub(crate) const TEST_GRID_SIZE: u16 = 64;

/// Flat Clear cells that allow tiberium and carry the retail `[Clear]` row,
/// whose Track entry a new ore Overlay's Mark admission reads
/// (`CellClass::CheckCellPassability @ 0x004834A0`, called at `0x005FD00C`;
/// `0x005FD04F` for TIB01).
pub(crate) fn flat_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
    crate::map::resolved_terrain::test_grid(width, height, |rx, ry| ResolvedTerrainCell {
        speed_costs: test_retail_clear_speed_costs(),
        base_speed_costs: test_retail_clear_speed_costs(),
        ..crate::map::resolved_terrain::test_tiberium_cell(rx, ry)
    })
}

/// A world with no objects in it: the live-object half of a
/// `NewTiberiumAdmission` for tests that are not about the object gate.
pub(crate) struct NoLiveObjects {
    entities: EntityStore,
    occupancy: OccupancyGrid,
    raw_occupation: RawCellOccupationGrid,
    rules: RuleSet,
    interner: StringInterner,
    terrain_object_cells: BTreeMap<(u16, u16), u64>,
}

impl NoLiveObjects {
    pub(crate) fn new() -> Self {
        Self {
            entities: EntityStore::new(),
            occupancy: OccupancyGrid::new(),
            raw_occupation: RawCellOccupationGrid::new(),
            rules: RuleSet::from_ini(&IniFile::from_str(
                "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n",
            ))
            .expect("empty rules"),
            interner: StringInterner::default(),
            terrain_object_cells: BTreeMap::new(),
        }
    }

    pub(crate) fn context(&self) -> TiberiumPlacementObjectContext<'_> {
        TiberiumPlacementObjectContext::new(
            &self.entities,
            &self.occupancy,
            &self.raw_occupation,
            &self.rules,
            &self.interner,
            &self.terrain_object_cells,
        )
    }
}

/// Stock-shaped `[Tiberiums]` and `[OverlayTypes]` sections: Riparius (ore,
/// `Value=25`) on `TIB01..TIB20` at the native overlay indices 102..=121, and
/// Cruentus (gems, `Value=50`) on `GEM01..GEM12` at 27..=38. Append it to a
/// test's rules text so `RuleSet::tiberium_types` is populated.
pub(crate) fn tiberium_rules_text() -> String {
    let mut text = String::from(
        "\n[Tiberiums]\n0=Riparius\n1=Cruentus\n\
         [Riparius]\nImage=1\nValue=25\nGrowth=2200\nGrowthPercentage=0\n\
         Spread=2200\nSpreadPercentage=0\n\
         [Cruentus]\nImage=2\nValue=50\nGrowth=10000\nGrowthPercentage=0\n\
         Spread=10000\nSpreadPercentage=0\n[OverlayTypes]\n",
    );
    let mut tiberium_names = Vec::new();
    for raw_key in (1..=124).filter(|key| *key != 40 && *key != 41) {
        let name = match raw_key {
            28..=39 => format!("GEM{:02}", raw_key - 27),
            105..=124 => format!("TIB{:02}", raw_key - 104),
            _ => format!("FILL{raw_key:03}"),
        };
        text.push_str(&format!("{raw_key}={name}\n"));
        if name.starts_with("TIB") || name.starts_with("GEM") {
            tiberium_names.push(name);
        }
    }
    for name in tiberium_names {
        text.push_str(&format!("[{name}]\nTiberium=yes\nLand=Tiberium\n"));
    }
    text
}

/// The overlay registry matching [`tiberium_rules_text`].
pub(crate) fn overlay_registry() -> &'static OverlayTypeRegistry {
    static REGISTRY: OnceLock<OverlayTypeRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        OverlayTypeRegistry::from_ini(&IniFile::from_str(&tiberium_rules_text()), None)
    })
}

/// Place ore or gems that yield `bales` bales (1..=11), creating the overlay
/// grid if the fixture has none. `bales` is the raw density byte:
/// `CellClass::ReduceTiberium @ 0x00480A80` hands out one bale per density
/// level and nothing for the bite that clears a density-0 cell.
pub(crate) fn place_tiberium(
    sim: &mut Simulation,
    rx: u16,
    ry: u16,
    resource: ResourceType,
    bales: u8,
) {
    let name = match resource {
        ResourceType::Ore => "TIB01",
        ResourceType::Gem => "GEM01",
    };
    let overlay_id = overlay_registry()
        .id_for_name(name)
        .expect("fixture overlay is registered");
    let grid = sim
        .overlay_grid
        .get_or_insert_with(|| OverlayGrid::new(TEST_GRID_SIZE, TEST_GRID_SIZE));
    grid.place_overlay(rx, ry, overlay_id, bales.clamp(1, 11));
}

/// [`overlay_registry`] with every stock land row admitting Foot, Track and
/// Wheel at 100%, so a recalculated ore cell carries a passable `[Tiberium]`
/// speed row.
pub(crate) fn overlay_registry_with_land() -> &'static OverlayTypeRegistry {
    static REGISTRY: OnceLock<OverlayTypeRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut text = tiberium_rules_text();
        for land in crate::rules::terrain_rules::LandType::ALL {
            text.push_str(&format!(
                "[{}]\nFoot=100%\nTrack=100%\nWheel=100%\n",
                land.section_name()
            ));
        }
        OverlayTypeRegistry::from_ini(&IniFile::from_str(&text), None)
    })
}

/// [`place_tiberium`] on a fixture with map cells: the overlay, then the
/// cell's recalculation through the production path (`CellClass::
/// RecalcAttributes @ 0x0047D2B0` writes LandType 5 and its speed row), as
/// an ore cell reads to the harvest scan's Is_Cell_Harvestable.
pub(crate) fn place_tiberium_on_map(
    sim: &mut Simulation,
    cell: (u16, u16),
    resource: ResourceType,
    bales: u8,
) {
    place_tiberium(sim, cell.0, cell.1, resource, bales);
    if let (Some(grid), Some(terrain)) = (sim.overlay_grid.as_mut(), sim.resolved_terrain.as_mut())
    {
        grid.recalculate_runtime_cell(terrain, overlay_registry_with_land(), cell);
    }
}

/// Bales left on a cell (its density byte), or 0 when it holds no tiberium.
pub(crate) fn bales_at(sim: &Simulation, rx: u16, ry: u16) -> u8 {
    let Some(grid) = sim.overlay_grid.as_ref() else {
        return 0;
    };
    let cell = grid.cell(rx, ry);
    match cell.overlay_id {
        Some(id) if is_tiberium(id) => cell.overlay_data,
        _ => 0,
    }
}

fn is_tiberium(overlay_id: u8) -> bool {
    overlay_registry()
        .flags(overlay_id)
        .is_some_and(|flags| flags.tiberium)
}

/// Compatibility input for old split-field fixtures only. Production placement
/// always receives its live Simulation; no gameplay owner uses this container.
pub(crate) struct TestPlacementContext<'a> {
    pub overlay_grid: &'a mut OverlayGrid,
    pub ore_growth_state: &'a mut crate::sim::ore_growth::OreGrowthState,
    pub overlay_registry: &'a OverlayTypeRegistry,
    pub tiberium_types: &'a crate::rules::tiberium_type::TiberiumTypeRegistry,
    pub resolved_terrain: Option<&'a ResolvedTerrainGrid>,
    pub source_object_cells: &'a std::collections::BTreeSet<(u16, u16)>,
    pub new_cell_admission: Option<super::NewTiberiumAdmission<'a>>,
    pub live_objects: Option<super::NativeCellObjectView<'a>>,
    pub rng: &'a mut crate::sim::rng::SimRng,
    pub binary_frame: u32,
    pub growth_enabled: bool,
    pub spread_enabled: bool,
    pub dirty_cells: Option<&'a mut crate::sim::dirty_cells::DirtyCells>,
}

/// Run the real world-owned operation on a legacy queue/grid fixture. Copies
/// of immutable scene priors exist only here; resulting cell/queue/RNG writes
/// return to the fixture even when the production operation returns an error.
/// Tests of callback mutations, identities or lifecycle must use a full world.
pub(crate) fn with_placement_fixture<R>(
    ctx: &mut TestPlacementContext<'_>,
    run: impl FnOnce(&mut Simulation, &RuleSet, &OverlayTypeRegistry) -> R,
) -> R {
    let objects = ctx
        .new_cell_admission
        .map(|admission| admission.live_objects());
    let mut rules = objects
        .map(|objects| objects.rules.clone())
        .unwrap_or_else(|| RuleSet::empty_for_test().clone());
    rules.tiberium_types = ctx.tiberium_types.clone();
    let mut sim = Simulation::new();
    sim.native_unique_ids =
        Some(crate::sim::native_identity::NativeUniqueIdCursor::test_at_current_value(0));
    sim.overlay_grid = Some(ctx.overlay_grid.clone());
    sim.resolved_terrain = ctx.resolved_terrain.cloned();
    sim.ore_growth_state = ctx.ore_growth_state.clone();
    sim.scenario_rng = ctx.rng.clone();
    sim.session.set_binary_frame_for_test(ctx.binary_frame);
    sim.ore_growth_config = crate::sim::ore_growth::OreGrowthConfig::for_test(
        ctx.growth_enabled,
        ctx.spread_enabled,
        false,
    );
    *sim.terrain_objects.spawning_cells_mut_for_test() = ctx.source_object_cells.clone();
    if let Some(objects) = objects {
        sim.substrate.entities = objects.entities.clone();
        sim.substrate.occupancy = objects.occupancy.clone();
        sim.substrate.raw_cell_occupation = objects.raw_occupation.clone();
        sim.interner = objects.interner.clone();
        *sim.terrain_objects.cell_index_mut_for_test() = objects.terrain_object_cells.clone();
    } else if let Some(objects) = ctx.live_objects {
        sim.substrate.occupancy = objects.occupancy.clone();
        *sim.terrain_objects.cell_index_mut_for_test() = objects.terrain_object_cells.clone();
    }
    if let Some(dirty) = ctx.dirty_cells.as_deref() {
        sim.dirty_cells = dirty.clone();
    }
    let result = run(&mut sim, &rules, ctx.overlay_registry);
    *ctx.overlay_grid = sim
        .overlay_grid
        .take()
        .expect("fixture operation preserves the grid");
    *ctx.ore_growth_state = sim.ore_growth_state;
    *ctx.rng = sim.scenario_rng;
    if let Some(dirty) = ctx.dirty_cells.as_deref_mut() {
        *dirty = sim.dirty_cells;
    }
    result
}
