//! Live structural-body publication (576BA0/571490, setters 47E040/47E470). Authorities stay in Simulation
//! through synchronous fallout, including recursive DeathWeapon damage.
//!
//! Rims 576770/576200 and 571050/570AE0 run against live scalar cells and uses this same publisher.
//! Literal middle-tile replacement uses resident56EB80/47D2B0 inputs. Repair
//! constructors share this publisher; full engineer/zone/render delivery is
//! separately required before the bridge mechanism can close.

use super::*;
use crate::map::cell_index::NativeCellIdentity as Cell;
use crate::sim::bridge_state::Phase;
use crate::sim::bridge_state::publication::{self, BridgePublicationHost, CellCoord};
use crate::sim::bridge_state::ramp_repair::Family;
use crate::sim::world::FrameEffects;

#[path = "bridge_rim_publication.rs"]
mod rim_publication;

#[path = "bridge_tile_publication.rs"]
mod tile_publication;

#[path = "bridge_constructor_publication.rs"]
mod constructor_publication;

#[path = "bridge_pavement_publication.rs"]
mod pavement_publication;

#[path = "bridge_zone_publication.rs"]
mod zone_publication;

#[path = "bridge_occupants.rs"]
mod occupants;

#[path = "bridge_ordinary_publication.rs"]
mod ordinary_publication;

pub(crate) use ordinary_publication::{damage_ordinary, repair_from_engineer};

#[cfg(test)]
pub(in crate::sim::world) use ordinary_publication::tests::{
    ready_engineer, ready_repair_fixture, repair_frame,
};

#[cfg(test)]
#[path = "bridge_pavement_publication_tests.rs"]
mod pavement_tests;

#[cfg(test)]
#[path = "bridge_publication_tests.rs"]
mod tests;

pub(super) struct BodyResult {
    pub returned: bool,
    pub collapsed: bool,
}

impl BodyResult {
    fn no_change() -> Self {
        Self {
            returned: false,
            collapsed: false,
        }
    }
}

/// ProcessBridgeDamageStateMachine_High 0x00576BA0 or _Low 0x00571490. A
/// structural cell runs the anchor switch (0x005776D6, 0x00571FEB); any other
/// cell runs the middle-tile bridgehead branch. Both use the family's
/// instruction-identical helpers.
pub(super) fn run_state_machine(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    input: CellCoord,
    family: Family,
    frame_effects: FrameEffects<'_>,
) -> BodyResult {
    let terrain = sim
        .resolved_terrain
        .as_ref()
        .expect("bridge damage terrain");
    let selected = terrain.native_cell_identity(input);
    if terrain.native_cell_flags(selected) & BRIDGE_FLAG_STRUCTURAL == 0 {
        let mut host = LivePublication {
            sim,
            rules,
            registry,
            collapsed: false,
            frame_effects,
        };
        let returned = publication::advance_bridgehead(&mut host, input, family);
        return BodyResult {
            returned,
            collapsed: host.collapsed,
        };
    }
    let anchor = if terrain.native_cell_flags(selected) & BRIDGE_FLAG_ANCHOR_SELF != 0 {
        selected
    } else {
        match terrain.native_cell_anchor(selected) {
            Some(anchor) => anchor,
            None => return BodyResult::no_change(),
        }
    };
    let overlay = sim.cell_overlay(anchor).overlay_id;
    // Residual: ApplyDamageToCell 0x00587180 also admits a state machine by
    // a middle tile, and neither driver tests the anchor overlay before its
    // +11E switch. This gate keeps the pre-existing concrete behavior for
    // both families; its no-change result for a structural cell whose anchor
    // lacks these overlays is unproven against native.
    let anchor_overlays = match family {
        Family::High => [0x18, 0x19],
        Family::Low => [0xed, 0xee],
    };
    if !overlay.is_some_and(|overlay| anchor_overlays.contains(&overlay)) {
        return BodyResult::no_change();
    }
    let mut host = LivePublication {
        sim,
        rules,
        registry,
        collapsed: false,
        frame_effects,
    };
    let returned = publication::advance_body_at_anchor(&mut host, input, anchor, family);
    BodyResult {
        returned,
        collapsed: host.collapsed,
    }
}

/// UpdateAdjacentBridges outside a state machine: both CABHUT fallback twins
/// call the concrete 576770 (0x005745B4, 0x005751D0). Its clears reach
/// BlowUpBridge; returns whether one ran.
pub(super) fn update_adjacent_bridges(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    input: CellCoord,
    family: Family,
    frame_effects: FrameEffects<'_>,
) -> bool {
    let mut host = LivePublication {
        sim,
        rules,
        registry,
        collapsed: false,
        frame_effects,
    };
    rim_publication::update(&mut host, input, family);
    host.collapsed
}

struct LivePublication<'a> {
    sim: &'a mut Simulation,
    rules: &'a RuleSet,
    registry: Option<&'a crate::rules::overlay_types::OverlayTypeRegistry>,
    collapsed: bool,
    frame_effects: FrameEffects<'a>,
}

impl LivePublication<'_> {
    fn terrain(&self) -> &ResolvedTerrainGrid {
        self.sim
            .resolved_terrain
            .as_ref()
            .expect("live bridge terrain")
    }

    fn real_coord(&self, cell: Cell) -> Option<(u16, u16)> {
        match cell {
            Cell::Real(index) => {
                let cell = &self.terrain().cells()[index];
                Some((cell.rx, cell.ry))
            }
            Cell::Dummy => None,
        }
    }

    /// Capture current values after this scalar store, never an outer-frame
    /// snapshot that could overwrite a later recursive receiver's writes.
    fn retain_real_write(&mut self, cell: Cell) {
        let Cell::Real(index) = cell else { return };
        let terrain = self.sim.resolved_terrain.as_ref().expect("live terrain");
        if !terrain.bridge_flag_authority_matches_shape(&self.sim.real_cell_bridge_flags_0x1180) {
            self.sim.real_cell_bridge_flags_0x1180 =
                terrain.capture_real_cell_bridge_flags_0x1180();
        }
        let resolved = &terrain.cells()[index];
        self.sim
            .real_cell_bridge_flags_0x1180
            .set_allocated_cell(index, resolved.bridge_facts.raw_flags);
        self.sim.dynamic_terrain_cells.record(resolved);
    }
}

impl BridgePublicationHost for LivePublication<'_> {
    type Cell = Cell;

    fn lookup(&mut self, coord: CellCoord) -> Cell {
        self.terrain().native_cell_identity(coord)
    }
    fn coord(&self, cell: Cell) -> CellCoord {
        self.terrain().native_cell_coord(cell)
    }
    fn flags(&self, cell: Cell) -> u32 {
        self.terrain().native_cell_flags(cell)
    }
    fn state(&self, cell: Cell) -> u8 {
        self.sim.cell_overlay(cell).overlay_data
    }
    fn write_flags(&mut self, cell: Cell, flags: u32) {
        self.sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .write_native_cell_flags(cell, flags);
        self.retain_real_write(cell);
    }
    fn write_state(&mut self, cell: Cell, state: u8) {
        match self.real_coord(cell) {
            Some((x, y)) => {
                if let Some(grid) = self.sim.overlay_grid.as_mut() {
                    grid.write_literal_bridge_state(x, y, state);
                }
            }
            None => self
                .terrain()
                .shared_cell_dummy()
                .write_overlay_state(state),
        }
        self.retain_real_write(cell);
    }
    fn write_anchor(&mut self, cell: Cell, anchor: Option<Cell>) {
        self.sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .write_native_cell_anchor(cell, anchor);
        self.retain_real_write(cell);
    }
    fn clear_overlay(&mut self, cell: Cell) {
        match self.real_coord(cell) {
            Some((x, y)) => {
                if let Some(grid) = self.sim.overlay_grid.as_mut() {
                    grid.clear_literal_bridge_identity(x, y);
                }
            }
            None => self
                .terrain()
                .shared_cell_dummy()
                .write_overlay_identity(-1),
        }
        self.retain_real_write(cell);
    }
    fn fallout(&mut self, cell: Cell) {
        self.collapsed = true;
        let (x, y) = self.coord(cell);
        blow_up_bridge_cell_fallout(
            self.sim,
            self.rules,
            x as u16,
            y as u16,
            self.registry,
            self.frame_effects,
        );
    }
    fn radar(&mut self, cell: Cell) {
        let (x, y) = self.coord(cell);
        self.sim.dirty_cells.mark_radar([(x as u16, y as u16)]);
    }
    fn perpendicular(
        &mut self,
        input: CellCoord,
        axis: Axis,
        phase: Phase,
        direction: u8,
        family: Family,
    ) {
        let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[usize::from(direction & 7)];
        // Native helpers retain this requested coordinate on their stack;
        // it is distinct from the retained allocation's current +24.
        let target_coord = (
            input.0.wrapping_add(dx as i16),
            input.1.wrapping_add(dy as i16),
        );
        let target = self.lookup(target_coord);
        if self.flags(target) & BRIDGE_FLAG_ANCHOR_SELF != 0
            && let Some(next) =
                crate::sim::bridge_specs::apply_ramp_transition(self.state(target), axis, phase)
        {
            if next == 0 {
                self.perpendicular(target_coord, axis, phase, direction, family);
                publication::set_bridge_direction(
                    self,
                    target,
                    if axis == Axis::NS { 0 } else { 6 },
                    false,
                );
            }
            self.write_state(target, next);
            if next == 0 {
                self.clear_overlay(target);
                self.radar(target);
            }
        }
        if let Err(error) =
            self.perpendicular_tile_tail(target_coord, target, axis, phase, direction, family)
        {
            // An unavailable/unadmitted input is not a successful native
            // sparse fallback. Preserve completed writes and report the gap.
            log::error!("bridge tile update at {target_coord:?} failed: {error}");
        }
    }
    fn rim(&mut self, coord: CellCoord, family: Family) {
        rim_publication::update(self, coord, family);
    }
    fn zones(&mut self, query: CellCoord) {
        let _ = invalidate_bridge_zones(self.sim, query);
        publish_bridge_navigation(self.sim, self.rules);
    }
    fn tile(&self, cell: Cell) -> i32 {
        LivePublication::tile(self, cell)
    }
    fn subtile(&self, cell: Cell) -> u8 {
        LivePublication::subtile(self, cell)
    }
    fn level(&self, cell: Cell) -> i8 {
        LivePublication::level(self, cell)
    }
    fn flood(&mut self, coord: CellCoord, tile: i32, level: i32) {
        if let Err(error) = self.replace_tiles(coord, tile, level) {
            log::error!("bridge flood at {coord:?} failed: {error}");
        }
    }
    fn recalc_zones(&mut self, cells: &[CellCoord]) {
        if let Err(error) = self.recalculate_bridge_zones(cells) {
            log::error!("bridge zone batch at {:?} failed: {error}", cells.first());
        }
    }
    fn middles(&self, family: Family) -> Option<(i32, [i32; 2])> {
        super::family_rim_tiles(self.terrain(), family).map(|keys| (keys.base, keys.middle))
    }
}
