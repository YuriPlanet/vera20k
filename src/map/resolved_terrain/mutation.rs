//! Mutation authority for runtime terrain projections.
//! Grid storage and its derived land/blocking/zone metadata never escape mutably
//! in production.

use super::{ResolvedTerrainGrid, overlay_reduced_zone_type, recalc_zone_type, zone_class};
use crate::rules::overlay_types::{OverlayTypeFlags, retained_overlay_land};
use crate::rules::terrain_rules::{LandType, SpeedCostProfile};

impl ResolvedTerrainGrid {
    /// Commit the overlay-derived Land, blocking and Zone projection together.
    /// Literal overlay storage/removal belongs to OverlayGrid. `land_flags` may
    /// retain the removed Tiberium's Land branch on a slope, as in RecalcAttributes
    /// (`0x0047D2B0`); `flags` always describes the surviving overlay identity.
    pub(crate) fn apply_overlay_attributes(
        &mut self,
        rx: u16,
        ry: u16,
        flags: Option<&OverlayTypeFlags>,
        land_flags: Option<&OverlayTypeFlags>,
    ) -> bool {
        let overlay_zone = overlay_reduced_zone_type(flags);
        let new_blocks = matches!(
            overlay_zone,
            Some(zone_class::WALL) | Some(zone_class::IMPASSABLE)
        );

        let Some(terrain_cell) = self.cell_mut(rx, ry) else {
            return false;
        };

        let old_overlay_zone_type = terrain_cell.overlay_zone_type;
        let old = (
            terrain_cell.overlay_blocks,
            terrain_cell.zone_type,
            terrain_cell.land_type,
            terrain_cell.speed_costs,
            terrain_cell.ground_walk_blocked,
        );

        restore_pristine_land(terrain_cell);
        // Ground blocking follows whichever land the cell ends up with. With no
        // overlay land that is the pristine terrain value already cached in
        // `base_ground_walk_blocked`; with one it is the overlay's `Land=` row.
        // `CellClass__RecalcAttributes` @ `0x0047D2B0` recomputes LandType from
        // scratch on every overlay change and keeps no separate blocked bit, so
        // neither value may outlive the land that produced it.
        let mut land_ground_blocked = terrain_cell.base_ground_walk_blocked;
        if let Some(flags) = land_flags
            && let Some(land) = retained_overlay_land(flags, terrain_cell.slope_type)
        {
            apply_overlay_land(terrain_cell, land, flags.land_speed_costs);
            // The ramp exemption is a property of the tile, not the land row,
            // so it survives the override exactly as it does in the pristine
            // value baked into `base_ground_walk_blocked`.
            land_ground_blocked =
                terrain_cell.canonical_ramp.is_none() && flags.land_ground_blocked;
        }

        terrain_cell.overlay_blocks = new_blocks;
        terrain_cell.overlay_zone_type = overlay_zone;
        terrain_cell.ground_walk_blocked =
            land_ground_blocked || terrain_cell.terrain_object_blocks || new_blocks;
        terrain_cell.zone_type = recalc_zone_type(
            terrain_cell.outside_playfield,
            terrain_cell.overlay_zone_type,
            terrain_cell.land_type,
            terrain_cell.speed_costs.wheel,
            terrain_cell.terrain_object_occupation,
        );

        let new = (
            terrain_cell.overlay_blocks,
            terrain_cell.zone_type,
            terrain_cell.land_type,
            terrain_cell.speed_costs,
            terrain_cell.ground_walk_blocked,
        );
        old_overlay_zone_type != terrain_cell.overlay_zone_type || old != new
    }

    /// Commit occupation and its derived blocking/Zone fields at the same seam.
    /// Preserve the occupation writer's pristine-ground predicate: it differs
    /// from the overlay writer's effective-Land predicate.
    pub(crate) fn set_terrain_object_occupation(
        &mut self,
        cell: (u16, u16),
        occupation: Option<i32>,
    ) {
        let Some(terrain_cell) = self.cell_mut(cell.0, cell.1) else {
            return;
        };
        let blocked = occupation.is_some_and(|occupation| occupation & 0x07 != 0);
        terrain_cell.terrain_object_occupation = occupation;
        terrain_cell.terrain_object_blocks = blocked;
        terrain_cell.ground_walk_blocked =
            terrain_cell.base_ground_walk_blocked || terrain_cell.overlay_blocks || blocked;
        terrain_cell.zone_type = recalc_zone_type(
            terrain_cell.outside_playfield,
            terrain_cell.overlay_zone_type,
            terrain_cell.land_type,
            terrain_cell.speed_costs.wheel,
            terrain_cell.terrain_object_occupation,
        );
    }
}

fn restore_pristine_land(cell: &mut crate::map::resolved_terrain::ResolvedTerrainCell) {
    cell.land_type = cell.base_land_type;
    cell.speed_costs = cell.base_speed_costs;
}

fn apply_overlay_land(
    cell: &mut crate::map::resolved_terrain::ResolvedTerrainCell,
    land: LandType,
    speed_costs: Option<SpeedCostProfile>,
) {
    cell.land_type = land.as_index();
    cell.speed_costs = speed_costs.unwrap_or_default();
}
