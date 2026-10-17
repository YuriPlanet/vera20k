//! Live host for the native bridge rim selector and restart loop.
//!
//! The original 575EE0 traversal runs even on a no-write refresh: its GetCell
//! calls can move the retained shared dummy, and tagged cells raise event31.

use super::*;
use crate::map::bridge_rim_tiles::HighBridgeRimTiles;
use crate::sim::bridge_state::rim::{self, HighBridgeRimHost, RimBounds, RimCell, RimCoord};

/// UpdateAdjacentBridges_High 576770 or, for the wooden family, 571050.
pub(super) fn update(publication: &mut LivePublication<'_>, input: RimCoord, family: Family) {
    let Some(tiles) = super::super::family_rim_tiles(publication.terrain(), family) else {
        // Synthetic grids without an active theater have no native tile keys.
        return;
    };
    let bounds = match family {
        Family::High => {
            let size = publication
                .sim
                .playfield_bounds
                .zip(publication.sim.playfield_size_height)
                .map(|(bounds, height)| (bounds.base, height))
                .or_else(|| {
                    publication
                        .sim
                        .bridge_state
                        .as_ref()?
                        .native_zone_source_size()
                });
            let Some((width, height)) = size else { return };
            RimBounds::Diamond { width, height }
        }
        Family::Low => RimBounds::Rect(NativeStartBounds::from_session(
            publication.sim,
            publication.terrain(),
        )),
    };
    rim::update_adjacent(
        &mut LiveRim {
            publication,
            tiles,
            bounds,
        },
        input,
    );
}

struct LiveRim<'a, 'world> {
    publication: &'a mut LivePublication<'world>,
    tiles: HighBridgeRimTiles,
    bounds: RimBounds,
}

impl HighBridgeRimHost for LiveRim<'_, '_> {
    type Cell = Cell;

    fn tiles(&self) -> HighBridgeRimTiles {
        self.tiles
    }
    fn bounds(&self) -> RimBounds {
        self.bounds
    }
    fn cell(&mut self, coord: RimCoord) -> Cell {
        self.publication.lookup(coord)
    }
    fn read(&self, cell: Cell) -> RimCell {
        let terrain = self.publication.terrain();
        RimCell {
            coord: self.publication.coord(cell),
            flags: self.publication.flags(cell),
            tile: terrain.native_cell_tile_index(cell),
            subtile: terrain.native_cell_sub_tile(cell),
            anchor: terrain
                .native_cell_anchor(cell)
                .map(|anchor| terrain.native_cell_coord(anchor)),
        }
    }
    fn allocated(&self, coord: RimCoord) -> bool {
        self.publication
            .terrain()
            .native_fixed_cell_index(coord.0, coord.1)
            .is_some()
    }
    fn clear_group(&mut self, cell: Cell, direction: u8) {
        publication::set_bridge_direction(self.publication, cell, direction, false);
    }
    fn clear_overlay_and_mark_radar(&mut self, cell: Cell) {
        self.publication.write_state(cell, 0);
        self.publication.clear_overlay(cell);
        self.publication.radar(cell);
    }
    fn notify_span(&mut self, first: RimCoord, end: RimCoord) {
        rim::notify_span_cells(self, first, end);
    }
    fn notify_cell(&mut self, cell: Cell) {
        let publication = &mut *self.publication;
        publication.sim.dispatch_bridge_cell_tag_event(
            cell,
            publication.rules,
            publication.registry,
            publication.frame_effects,
        );
    }
    // Native rectangles schedule partial tactical redraws. VERA clears color
    // and depth and rebuilds/uploads all three bridge batches every frame
    // (render/build_instances.rs, render/bridges.rs) from live CellClass fields.
    // This no-tile-write mechanism needs no extra retained redraw authority.
    // Literal56EB80 delivery must also migrate the immutable terrain-template
    // and presentation-grid consumers; that separate gap remains open.
    fn mark_span(&mut self, _start: RimCoord, _end: RimCoord) {}
    fn reset_span_marks(&mut self) {}
    fn mark_screen(&mut self) {}
}
