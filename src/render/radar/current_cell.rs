//! Current CellClass facts consumed by retained radar terrain reconstruction.
//!
//! This adapter keeps full surface rebuilds and incremental dirty-cell updates
//! on one presentation-owned read contract. It never owns or serializes map
//! state; successful save load supplies the restored simulation components.

use std::collections::HashMap;

use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::runtime::SimRuntime;

use super::helpers::{
    OverlayClassification, current_cell_radar_source, minimap_overlay_datum,
    radar_colors_for_tmp_metadata,
};

/// Read-only live CellClass authority for one retained-radar reconstruction.
///
/// An outer `Option` at the projection call distinguishes the explicit
/// no-runtime map/presentation fallback from an installed simulation authority. Missing
/// components inside an installed authority fail absent instead of reviving
/// stale presentation entries.
#[derive(Clone, Copy)]
pub(crate) struct CurrentRadarCellAuthority<'a> {
    resolved_terrain: Option<&'a ResolvedTerrainGrid>,
    overlay_grid: Option<&'a OverlayGrid>,
    overlay_registry: Option<&'a OverlayTypeRegistry>,
    rules: Option<&'a RuleSet>,
}

impl<'a> CurrentRadarCellAuthority<'a> {
    pub(crate) fn new(
        resolved_terrain: Option<&'a ResolvedTerrainGrid>,
        overlay_grid: Option<&'a OverlayGrid>,
        overlay_registry: Option<&'a OverlayTypeRegistry>,
        rules: Option<&'a RuleSet>,
    ) -> Self {
        Self {
            resolved_terrain,
            overlay_grid,
            overlay_registry,
            rules,
        }
    }

    /// Production presentation wiring after the restored simulation commits.
    pub(crate) fn from_runtime(runtime: &'a SimRuntime) -> Self {
        Self::new(
            runtime.simulation.resolved_terrain(),
            runtime.simulation.overlay_grid(),
            Some(&runtime.resources.overlay_registry),
            Some(&runtime.resources.rules),
        )
    }

    /// Current tile branch of `CellClass::GetRadarColor @ 0x0047C060`.
    /// Bit 0x2000 selects the first sibling TMP only when the pristine
    /// subimage advertised damaged data and the sibling entered the native
    /// variant chain. Both metadata triples remain retained even though this
    /// active function reads RadarLeft and duplicates it into the raw pair.
    pub(crate) fn tile_radar_colors(
        self,
        rx: u16,
        ry: u16,
        terrain_brightness: f32,
    ) -> Option<([u8; 3], [u8; 3])> {
        let metadata = self.resolved_terrain?.current_tile_radar_metadata(rx, ry)?;
        Some(radar_colors_for_tmp_metadata(
            metadata.left,
            metadata.right,
            metadata.valid,
            terrain_brightness,
        ))
    }

    /// Resolve the current source branch for `CellClass::GetRadarColor`.
    ///
    /// Verified active YR sources: `CellClass::GetRadarColor @ 0x0047C060`
    /// reads current terrain occupation, structural bridge state, and current
    /// overlay identity/data. Successful load rebuilds the whole authority via
    /// `FUN_00685120 -> RadarClass::Init @ 0x00655B20` after swizzling current
    /// CellClass state; no saved radar pixels or dirty queue participate.
    pub(super) fn source(
        self,
        rx: u16,
        ry: u16,
        structural_bridge_color: [u8; 3],
        overlay_radar_colors: &HashMap<(u8, u8), [u8; 3]>,
    ) -> Option<([u8; 3], OverlayClassification)> {
        let terrain_object_present = self
            .resolved_terrain
            .and_then(|terrain| terrain.cell(rx, ry))
            .is_some_and(|cell| cell.terrain_object_occupation.is_some());
        let resolved_cell = self
            .resolved_terrain
            .and_then(|terrain| terrain.cell(rx, ry));
        // `CellClass+0x140 & 0x100` is the live structural-color branch in
        // `CellClass::GetRadarColor @ 0x0047C060`. Keep it independent from
        // the immutable high-family routing fact: a saved collapse clears the
        // structural bit, but its current Cell+0x44 byte is still the runtime
        // high-bridge overlay authority.
        let structural_bridge_present =
            resolved_cell.is_some_and(|cell| cell.bridge_facts.has_structural_bridge());
        // The high-family slots 47E040 stamps structural, or marks destroyed
        // (0x400) when it clears the deck, read their Cell+0x44 identity with
        // data 0; a family-only auxiliary stamp such as ExtraDir6 keeps
        // neither bit and reads the identity with its data.
        let high_walker = resolved_cell.is_some_and(|cell| {
            cell.bridge_facts.family != crate::map::bridge_facts::BridgeStampFamily::None
                && cell.bridge_facts.raw_flags
                    & (crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL
                        | crate::map::bridge_facts::BRIDGE_FLAG_DESTROYED_OR_RAMP)
                    != 0
        });
        // Constructor5FC380 stamps structural side cells with Cell+44=-1
        // (native -1 is Rust None); bridge_constructor.json cases8..11 retain
        // these original outputs. GetRadarColor47C060's bit100 branch precedes
        // its overlay branch and cannot be vetoed by absent sprite data or
        // runtime damage state.
        let overlay = self.overlay_grid.and_then(|grid| {
            let cell = grid.cell(rx, ry);
            let data = if high_walker { 0 } else { cell.overlay_data };
            cell.overlay_id.map(|overlay_id| {
                minimap_overlay_datum(rx, ry, overlay_id, data, self.overlay_registry, self.rules)
            })
        });
        current_cell_radar_source(
            terrain_object_present,
            structural_bridge_present,
            overlay,
            structural_bridge_color,
            overlay_radar_colors,
        )
    }
}

#[cfg(test)]
#[path = "current_cell_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "current_cell_damage_pipeline_tests.rs"]
mod damage_pipeline_tests;
