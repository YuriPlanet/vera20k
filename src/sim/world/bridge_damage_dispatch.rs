//! Live host of Apply_area_damage's bridge admission and callback order, and
//! of ApplyDamageToCell (`0x00587180`), which area damage and the CABHUT death
//! fallback (`0x00574483`, `0x0057459A`, `0x0057509F`, `0x005751B6`) share.

use super::*;
use crate::map::cell_index::NativeCellIdentity;
use crate::sim::bridge_state::damage_dispatch::{self, CellFields, CellReader, DamageHost};
use crate::sim::bridge_state::publication::CellCoord;
use crate::sim::bridge_state::ramp_repair::{Family, HutCells};
use crate::sim::world::FrameEffects;
use std::hash::{Hash, Hasher};

/// Map+D4's ordered packed-cell vector, independent of optional bridge geometry.
/// Native5650E3/5651EB initializes it empty; MouseSave5BE8E1..927 and
/// MouseLoad5BE598..61E retain the count and every coordinate, including duplicates.
/// Source: original587180..58740E and47DDD5..47DE2B (gamemd1cdd1180).
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub(in crate::sim::world) struct PendingBridgeCells {
    cells: Vec<(i16, i16)>,
}

impl PendingBridgeCells {
    pub(in crate::sim::world) fn append(&mut self, cell: (i16, i16)) {
        // Native skips this append on allocation failure, then continues debris.
        if self.cells.try_reserve(1).is_ok() {
            self.cells.push(cell);
        }
    }

    pub(in crate::sim::world) fn clear(&mut self) {
        //42F420 frees native capacity too. Retaining Rust capacity is not
        //observable gameplay state; allocation-failure equivalence is excluded.
        self.cells.clear();
    }

    fn at(&self, index: usize) -> Option<(i16, i16)> {
        self.cells.get(index).copied()
    }

    pub(in crate::sim::world) fn hash_state(&self, hasher: &mut impl Hasher) {
        if !self.cells.is_empty() {
            // Preserve the established empty-map hash representation.
            0x87f8bcu32.hash(hasher);
            self.cells.hash(hasher);
        }
    }
}

#[cfg(test)]
#[path = "bridge_moving_collapse_tests.rs"]
mod moving_collapse_tests;

/// The live cell reads behind a driver selection and the CABHUT fallback's
/// anchor search.
pub(super) struct LiveCells<'a> {
    sim: &'a Simulation,
}

impl<'a> LiveCells<'a> {
    pub(super) fn new(sim: &'a Simulation) -> Self {
        Self { sim }
    }

    fn terrain(&self) -> &'a ResolvedTerrainGrid {
        self.sim
            .resolved_terrain
            .as_ref()
            .expect("bridge damage terrain")
    }
}

impl HutCells for LiveCells<'_> {
    type Cell = NativeCellIdentity;
    /// A 0x100 cell without 0x80 or +0x2C, where native faults.
    type Error = ();

    fn lookup(&mut self, requested: CellCoord) -> Self::Cell {
        self.terrain().native_cell_identity(requested)
    }
    fn coord(&self, cell: Self::Cell) -> CellCoord {
        self.terrain().native_cell_coord(cell)
    }
    fn flags(&self, cell: Self::Cell) -> u32 {
        self.terrain().native_cell_flags(cell)
    }
    fn anchor(&self, cell: Self::Cell) -> Result<Self::Cell, ()> {
        self.terrain().native_cell_anchor(cell).ok_or(())
    }
}

impl CellReader for LiveCells<'_> {
    type Cell = NativeCellIdentity;

    fn fields(&self, cell: Self::Cell) -> CellFields {
        let terrain = self.terrain();
        // Every bridge writer publishes CellClass+44 synchronously.
        let overlay = match cell {
            NativeCellIdentity::Real(index) => terrain.cells()[index].bridge_facts.overlay_id,
            NativeCellIdentity::Dummy => terrain.shared_cell_dummy().overlay_fields().0,
        };
        CellFields {
            flags: terrain.native_cell_flags(cell),
            tile: terrain.native_cell_tile_index(cell),
            overlay: overlay.filter(|value| *value != 0xff).map_or(-1, i32::from),
            level: terrain.native_cell_ground_fields(cell).0,
        }
    }

    fn resolve_anchor(&mut self, cell: Self::Cell) -> Option<Self::Cell> {
        let terrain = self.terrain();
        let retained = if terrain.native_cell_flags(cell) & BRIDGE_FLAG_ANCHOR_SELF != 0 {
            cell
        } else {
            terrain.native_cell_anchor(cell)?
        };
        Some(terrain.native_cell_identity(terrain.native_cell_coord(retained)))
    }

    fn tile_bases(&self) -> [i32; 2] {
        let terrain = self.terrain();
        [
            terrain
                .high_bridge_rim_tiles()
                .map_or_else(|| terrain.concrete_bridge_set_base(), |tiles| tiles.base),
            terrain.wood_bridge_set_base(),
        ]
    }

    fn middle_tiles(&self) -> Option<[i32; 2]> {
        self.terrain()
            .high_bridge_rim_tiles()
            .map(|tiles| tiles.middle)
    }
}

/// The bridge drivers that ApplyDamageToCell and the direct overlay blocks
/// call. Every driver publishes synchronously through the live host.
pub(crate) struct BridgeDamageDrivers<'a> {
    pub(super) sim: &'a mut Simulation,
    pub(super) rules: &'a RuleSet,
    registry: Option<&'a crate::rules::overlay_types::OverlayTypeRegistry>,
    /// A driver collapsed a span.
    pub(super) collapsed: bool,
    frame_effects: FrameEffects<'a>,
}

impl<'a> BridgeDamageDrivers<'a> {
    pub(crate) fn new(
        sim: &'a mut Simulation,
        rules: &'a RuleSet,
        registry: Option<&'a crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'a>,
    ) -> Self {
        Self {
            sim,
            rules,
            registry,
            collapsed: false,
            frame_effects,
        }
    }

    /// ApplyDamageToCell `0x00587180`: the driver is chosen again from live
    /// cell state on every call ([`damage_dispatch::select_driver`]); no
    /// match returns false. The resident Map+D4 vector is cleared on both
    /// boundaries, including no-driver calls. Recursive damage uses this same
    /// owner: never take/copy the vector into a private activation's drain.
    pub(crate) fn apply_damage_to_cell(&mut self, input: (i16, i16)) -> bool {
        self.sim.pending_bridge_collapses.clear();
        let cell = self
            .sim
            .resolved_terrain
            .as_ref()
            .expect("bridge damage terrain")
            .native_cell_identity(input);
        let returned = damage_dispatch::select_driver(&mut LiveCells::new(self.sim), cell)
            .is_some_and(|path| self.run(input, path));
        //587394 reloads the data and5873D4 reloads the live count after each
        //receiver: nested calls can clear this vector; callbacks can append.
        let mut index = 0;
        while let Some(point) = self.sim.pending_bridge_collapses.at(index) {
            moving_collapse::apply(
                self.sim,
                self.rules,
                self.registry,
                point,
                self.frame_effects,
            );
            index += 1;
        }
        self.sim.pending_bridge_collapses.clear();
        returned
    }

    /// Read-only receipt of the cascades already published by this owner.
    /// The bridge state and mutation services remain private to the host.
    pub(crate) fn collapsed(&self) -> bool {
        self.collapsed
    }

    /// One driver at `input`: DamageOrdinaryWoodBridge (`0x0057BAA0`) or
    /// DestroyBridge_High (`0x0057CCF0`) for a direct path, the family's
    /// state machine (`0x00576BA0`, `0x00571490`) otherwise. Each publishes
    /// synchronously, so the next call reads the flags it set.
    pub(super) fn run(&mut self, input: (i16, i16), path: DispatchPath) -> bool {
        let family = match path {
            DispatchPath::LowDirect | DispatchPath::LowStateMachine => Family::Low,
            DispatchPath::HighDirect | DispatchPath::HighStateMachine => Family::High,
        };
        let result = if path.is_state_machine() {
            live_publication::run_state_machine(
                self.sim,
                self.rules,
                self.registry,
                input,
                family,
                self.frame_effects,
            )
        } else {
            live_publication::damage_ordinary(
                self.sim,
                self.rules,
                self.registry,
                input,
                family,
                self.frame_effects,
            )
            .unwrap_or_else(|error| {
                panic!("ordinary {family:?} bridge publication at {input:?}: {error}")
            })
        };
        self.collapsed |= result.collapsed;
        result.returned
    }
}

/// Apply_area_damage's four admission blocks around one event's cell.
struct LiveDamage<'a> {
    drivers: BridgeDamageDrivers<'a>,
    event: &'a BridgeDamageEvent,
    strength: i32,
}

impl LiveDamage<'_> {
    fn cells(&self) -> LiveCells<'_> {
        LiveCells::new(self.drivers.sim)
    }
}

impl CellReader for LiveDamage<'_> {
    type Cell = NativeCellIdentity;

    fn fields(&self, cell: Self::Cell) -> CellFields {
        self.cells().fields(cell)
    }

    fn resolve_anchor(&mut self, cell: Self::Cell) -> Option<Self::Cell> {
        self.cells().resolve_anchor(cell)
    }

    fn tile_bases(&self) -> [i32; 2] {
        self.cells().tile_bases()
    }

    fn middle_tiles(&self) -> Option<[i32; 2]> {
        self.cells().middle_tiles()
    }
}

impl DamageHost for LiveDamage<'_> {
    fn roll_strength(&mut self) -> i32 {
        self.drivers
            .sim
            .scenario_rng
            .next_range_i32_inclusive(1, self.strength)
    }

    fn apply(&mut self, path: DispatchPath) -> bool {
        let input = (self.event.rx as i16, self.event.ry as i16);
        if path.is_state_machine() {
            self.drivers.apply_damage_to_cell(input)
        } else {
            self.drivers.run(input, path)
        }
    }

    fn detach(&mut self, cell: Self::Cell) {
        let NativeCellIdentity::Real(_) = cell else {
            return;
        };
        let (rx, ry) = self.cells().terrain().native_cell_coord(cell);
        let rules = self.drivers.rules;
        self.drivers.sim.stop_all_targeting_cell(
            rx as u16,
            ry as u16,
            Some(rules),
            self.drivers.registry,
            self.drivers.frame_effects,
        );
    }

    fn dirty(&mut self, _path: DispatchPath) {
        // Tactical presentation rebuilds a complete frame; this cell marks the
        // same native damage attempt even when its driver returns false.
        self.drivers
            .sim
            .tactical_dirty_cells
            .push((self.event.rx, self.event.ry));
    }
}

/// Returns whether a driver collapsed a span.
pub(super) fn run(
    sim: &mut Simulation,
    events: &[BridgeDamageEvent],
    strength: i32,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) -> bool {
    if sim.resolved_terrain.is_none() || sim.bridge_state.is_none() {
        return false;
    }
    let mut collapsed = false;
    for event in events {
        let cell = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_identity((event.rx as i16, event.ry as i16));
        let mut host = LiveDamage {
            drivers: BridgeDamageDrivers::new(sim, rules, registry, frame_effects),
            event,
            strength,
        };
        damage_dispatch::dispatch(
            &mut host,
            cell,
            event.damage,
            event.impact_z_leptons,
            event.is_ion_cannon,
        );
        collapsed |= host.drivers.collapsed;
    }
    collapsed
}
