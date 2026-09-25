//! Shared ore fixtures for tests.
//!
//! Tiberium has one authority: the `OverlayGrid` cell, read through the
//! overlay and tiberium type registries. Tests seed ore the way a map does, by
//! placing a tiberium overlay with a density byte, never through a side store.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::StringInterner;
use crate::sim::miner::ResourceType;
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::tiberium::TiberiumPlacementObjectContext;
use crate::sim::world::Simulation;

/// Side length of the overlay grid [`place_tiberium`] creates on demand.
pub(crate) const TEST_GRID_SIZE: u16 = 64;

/// A flat clear map whose every cell accepts tiberium: the terrain half of a
/// `NewTiberiumAdmission`.
pub(crate) fn flat_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
    let mut cells = Vec::with_capacity(usize::from(width) * usize::from(height));
    for ry in 0..height {
        for rx in 0..width {
            cells.push(crate::sim::deploy_tests::clear_terrain_cell(rx, ry));
        }
    }
    ResolvedTerrainGrid::from_cells(width, height, cells)
}

/// A world with no objects in it: the live-object half of a
/// `NewTiberiumAdmission` for tests that are not about the object gate.
pub(crate) struct NoLiveObjects {
    entities: EntityStore,
    occupancy: OccupancyGrid,
    rules: RuleSet,
    interner: StringInterner,
    terrain_object_cells: BTreeMap<(u16, u16), u64>,
}

impl NoLiveObjects {
    pub(crate) fn new() -> Self {
        Self {
            entities: EntityStore::new(),
            occupancy: OccupancyGrid::new(),
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
        text.push_str(&format!("[{name}]\nTiberium=yes\n"));
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
        grid.recalculate_runtime_cell(
            terrain,
            overlay_registry_with_land(),
            cell,
            crate::sim::overlay_grid::NavigationPublication::FrameBoundary,
        );
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

/// Place ore or gems from an amount in the retired per-cell stock units
/// (120 per ore bale, 180 per gem bale) that older fixtures are written in.
pub(crate) fn place_stock_amount(
    sim: &mut Simulation,
    cell: (u16, u16),
    resource: ResourceType,
    amount: u16,
) {
    let per_bale = match resource {
        ResourceType::Ore => 120,
        ResourceType::Gem => 180,
    };
    let bales = amount.div_ceil(per_bale).clamp(1, 11) as u8;
    place_tiberium(sim, cell.0, cell.1, resource, bales);
}

/// Whether a cell holds a tiberium overlay, at any density.
pub(crate) fn has_tiberium(sim: &Simulation, cell: (u16, u16)) -> bool {
    sim.overlay_grid
        .as_ref()
        .and_then(|grid| grid.cell(cell.0, cell.1).overlay_id)
        .is_some_and(is_tiberium)
}

/// Remove whatever overlay the cell holds; on a fixture with map cells the
/// cell is recalculated as RecalcAttributes does (LandType back to the
/// ground's).
pub(crate) fn clear_tiberium(sim: &mut Simulation, cell: (u16, u16)) {
    if let Some(grid) = sim.overlay_grid.as_mut() {
        grid.clear_overlay(cell.0, cell.1);
        if let Some(terrain) = sim.resolved_terrain.as_mut() {
            grid.recalculate_runtime_cell(
                terrain,
                overlay_registry_with_land(),
                cell,
                crate::sim::overlay_grid::NavigationPublication::FrameBoundary,
            );
        }
    }
}

/// A cell's tiberium in the retired stock units (`bales * 120`, the ore rate),
/// so density assertions written against those units keep their numbers.
pub(crate) fn stock_amount_at(sim: &Simulation, cell: (u16, u16)) -> u16 {
    u16::from(bales_at(sim, cell.0, cell.1)) * 120
}
