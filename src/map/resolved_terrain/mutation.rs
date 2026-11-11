//! Mutation authority for runtime terrain projections.
//! Grid storage and its derived land/blocking/zone metadata never escape mutably
//! in production.

use super::{ResolvedTerrainGrid, overlay_reduced_zone_type, recalc_zone_type, zone_class};
use crate::rules::overlay_types::{
    OverlayTypeFlags, retained_overlay_land, uses_early_recalc_land_branch,
};
use crate::rules::terrain_rules::{LandType, LandTypeSemantics, SpeedCostProfile};

impl ResolvedTerrainGrid {
    /// Commit the overlay-derived Land, blocking and Zone projection together.
    /// Literal overlay storage/removal belongs to OverlayGrid. `land_flags` may
    /// retain the removed Tiberium's Land branch on a slope, as in RecalcAttributes
    /// (`0x0047D2B0`); `flags` always describes the surviving overlay identity.
    /// `cliff_back_rock` is the Rock row under `CliffBackImpassability=2`, which
    /// that function's early overlay branch writes unconditionally behind a
    /// cliff (`0x0047D31E..0x0047D53E`).
    ///
    /// This projects a Recalc's result and runs again as a replay (frame end,
    /// snapshot rebuild), so it reads no cell through the Map lookup: the
    /// Recalc's own neighbour reads are [`Self::read_cliff_back_neighbours`].
    /// A replay tests the neighbours' current levels, which differ from the
    /// Recalc's only if one changed since.
    pub(crate) fn apply_overlay_attributes(
        &mut self,
        rx: u16,
        ry: u16,
        flags: Option<&OverlayTypeFlags>,
        land_flags: Option<&OverlayTypeFlags>,
        cliff_back_rock: Option<&LandTypeSemantics>,
    ) -> bool {
        // Like the cell recalc, an unchanged projection leaves shared cell
        // storage shared (see `cow_cells`).
        let Some(index) = self.index(rx, ry) else {
            return false;
        };
        self.cells.begin_shadow(index);
        let changed =
            self.apply_overlay_attributes_shadowed(rx, ry, flags, land_flags, cliff_back_rock);
        self.cells.end_shadow();
        changed
    }

    fn apply_overlay_attributes_shadowed(
        &mut self,
        rx: u16,
        ry: u16,
        flags: Option<&OverlayTypeFlags>,
        land_flags: Option<&OverlayTypeFlags>,
        cliff_back_rock: Option<&LandTypeSemantics>,
    ) -> bool {
        let overlay_zone = overlay_reduced_zone_type(flags);
        let new_blocks = matches!(
            overlay_zone,
            Some(zone_class::WALL) | Some(zone_class::IMPASSABLE)
        );

        let overlay_cliff_rock = cliff_back_rock.filter(|_| {
            land_flags.is_some_and(uses_early_recalc_land_branch)
                && self
                    .index(rx, ry)
                    .is_some_and(|index| self.replayed_cell_is_behind_cliff(index))
        });
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
        if let Some(rock) = overlay_cliff_rock {
            terrain_cell.land_type = LandType::Rock.as_index();
            terrain_cell.speed_costs = rock.speed_costs;
            land_ground_blocked = rock.ground_blocked;
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

    /// RecalcZoneType (`0x00483C80`) of one cell whose terrain inputs are
    /// unchanged, with the object list's building arm answered by the caller
    /// ([`super::recalc_zone_type_with_buildings`]). Answers whether the
    /// class changed.
    #[cfg(feature = "ares")]
    pub(crate) fn recalc_zone_type_with_buildings_at(
        &mut self,
        (rx, ry): (u16, u16),
        building_impassable: bool,
    ) -> bool {
        let Some(cell) = self.cell_mut(rx, ry) else {
            return false;
        };
        let zone_type = super::recalc_zone_type_with_buildings(
            cell.outside_playfield,
            cell.overlay_zone_type,
            cell.land_type,
            cell.speed_costs.wheel,
            cell.terrain_object_occupation,
            building_impassable,
        );
        let changed = cell.zone_type != zone_type;
        cell.zone_type = zone_type;
        changed
    }

    /// Commit occupation and its derived blocking/Zone fields at the same seam.
    /// Preserve the occupation writer's pristine-ground predicate: it differs
    /// from the overlay writer's effective-Land predicate.
    pub(crate) fn set_terrain_object_occupation(
        &mut self,
        cell: (u16, u16),
        occupation: Option<i32>,
    ) {
        // Terrain's packed Location may name a fixed512 alias. PlaceDown/
        // PickUp568471/5688A1 admit only allocated CellClass identities.
        let Some(index) = self.native_fixed_cell_index(cell.0 as i16, cell.1 as i16) else {
            return;
        };
        let (x, y) =
            self.native_cell_coord(crate::map::cell_index::NativeCellIdentity::Real(index));
        let Some(terrain_cell) = self.cell_mut(x as u16, y as u16) else {
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
