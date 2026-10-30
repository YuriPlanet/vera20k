//! FootAI4DA692..4DA7AA: admitted high-flying sight refresh before Process.
//! Per-viewer clocks preserve the original local-player admission boundary;
//! House/view-cache reconciliation never advances these clocks or emits events.
use super::{FrameEffects, Simulation, UninitContext};
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::resolved_terrain::{AllocatedCellOrder, ROW_NEIGHBORS};
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::vision::FogState;
use crate::sim::{pathfinding::PathGrid, vision};

#[cfg(test)]
#[path = "shroud_refresh_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "reveal_to_all_tests.rs"]
mod reveal_to_all_tests;

#[cfg(test)]
#[path = "sight_cadence_tests.rs"]
mod sight_cadence_tests;

/// [`Simulation::sight_height_grid`]'s answer, a handle that borrows nothing
/// from the simulation, so it can cross the reveal's `&mut` calls.
pub(crate) struct SightHeights(SightLevels);

enum SightLevels {
    /// `RevealByHeight=no`, or no map to read.
    Flat,
    /// The published path grid's level plane ([`PathGrid::ground_height_grid`]).
    Published(std::sync::Arc<PathGrid>),
    /// The terrain's own cells, before the scenario publishes its navigation.
    Terrain,
}

impl SightHeights {
    /// Where a walk reads its cells' levels, `terrain` being the simulation's
    /// resolved terrain ([`vision::CellLevels`]); none without line of sight.
    pub(crate) fn cell_levels<'a>(
        &'a self,
        terrain: Option<&'a crate::map::resolved_terrain::ResolvedTerrainGrid>,
    ) -> Option<vision::CellLevels<'a>> {
        match &self.0 {
            SightLevels::Flat => None,
            SightLevels::Published(grid) => {
                Some(vision::CellLevels::Plane(grid.ground_height_grid()))
            }
            SightLevels::Terrain => terrain.map(vision::CellLevels::Terrain),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
struct RevealCalls {
    walks: Vec<Vec<(u16, u16)>>,
    border_scans: Vec<((u16, u16, i32), i32)>,
    /// Actual567DA0 receiver calls, in order.
    border_receivers: Vec<(u16, u16)>,
    border_ground_after: Vec<i8>,
    discovery_queries: std::collections::BTreeSet<(u16, u16)>,
    notifications: Vec<((u16, u16), bool)>,
    /// Map578100 receiver calls.
    edge_visits: usize,
    edge_notifications: Vec<(bool, (u16, u16), bool)>,
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static REVEAL_CALLS: std::cell::RefCell<Option<RevealCalls>> = const { std::cell::RefCell::new(None) };
}

#[cfg(any(test, feature = "test-support"))]
fn trace_reveal_calls<T>(run: impl FnOnce() -> T) -> (T, RevealCalls) {
    REVEAL_CALLS.with_borrow_mut(|trace| {
        assert!(trace.is_none(), "nested RevealArea traces");
        *trace = Some(RevealCalls::default());
    });
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            REVEAL_CALLS.with_borrow_mut(|trace| *trace = None);
        }
    }
    let reset = Reset;
    let result = run();
    let calls = REVEAL_CALLS.with_borrow_mut(|trace| trace.take().unwrap());
    drop(reset);
    (result, calls)
}

#[cfg(test)]
thread_local! {
    /// Matches the native oracle's explicitly supplied first5865F0 callback,
    /// not a claim that ordinary discovery naturally runs this mutation.
    static FIRST_CELL_RECEIVER: std::cell::RefCell<Option<Box<dyn FnOnce(&mut Simulation)>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn with_first_cell_receiver<T>(
    receiver: impl FnOnce(&mut Simulation) + 'static,
    run: impl FnOnce() -> T,
) -> T {
    FIRST_CELL_RECEIVER.with_borrow_mut(|hook| {
        assert!(hook.is_none());
        *hook = Some(Box::new(receiver));
    });
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            FIRST_CELL_RECEIVER.with_borrow_mut(|hook| *hook = None);
        }
    }
    let reset = Reset;
    let result = run();
    drop(reset);
    result
}

impl Simulation {
    /// Native PlayerPtr names one process's map. The network simulation keeps
    /// every human's map in immutable House order; local input identity never
    /// selects shared fog. Offline/campaign retains its one current viewpoint.
    pub(crate) fn represented_player_maps(&self) -> Vec<InternedId> {
        if self.session.game_mode().is_network() {
            self.houses
                .in_order()
                .filter(|house| house.is_human())
                .map(|house| house.name())
                .collect()
        } else {
            self.session.current_house().into_iter().collect()
        }
    }

    /// The movement step that drops another house's object from the player's
    /// selection over the player's shroud: a selected object (`+0x83`) whose
    /// owner fails the locomotor's owner test is deselected (vt+0x150) when
    /// its Location is shrouded on the player's map (`MapClass::IsShrouded @
    /// 0x00586360`, [`vision::point_is_shrouded`]). The fog arm's
    /// `MapClass::IsFogged @ 0x005865E0` is `xor al, al; ret 4`, so it never
    /// deselects. Without a player house or map nothing is asked.
    ///
    /// Three movement steps run it, each with its own owner test:
    /// - Foot's `PerCellProcess` (`0x004D8DA7..0x004D8DF5`), on every Unit
    ///   and Infantry cell arrival, and Fly's `Horizontal_Step`
    ///   (`0x004CF55D..0x004CF5EC`) ask `0x0050B6F0`
    ///   ([`Self::house_is_human_player`]);
    /// - Jumpjet's `Process` (`0x0054B07D..0x0054B10C`) compares the owner
    ///   with PlayerPtr (`0x00A83D4C`).
    ///
    /// In skirmish both tests answer alike.
    pub(crate) fn deselect_over_player_shroud(
        &mut self,
        id: u64,
        player_owned: impl FnOnce(&Self, InternedId) -> bool,
    ) {
        let (Some(player), Some(terrain)) =
            (self.session.current_house(), self.resolved_terrain.as_ref())
        else {
            return;
        };
        let Some(entity) = self
            .substrate
            .entities
            .get(id)
            .filter(|entity| entity.selection.is_selected())
        else {
            return;
        };
        if player_owned(self, entity.owner()) {
            return;
        }
        let location = crate::sim::movement::ground_pose::object_location(entity, Some(terrain));
        let cells = crate::map::resolved_terrain::NativeCellQuery::for_local_input(
            terrain,
            self.session.game_mode().is_network(),
        );
        if vision::point_is_shrouded(&self.fog, &cells, player, location)
            && let Some(entity) = self.substrate.entities.get_mut(id)
        {
            entity.selection.deselect();
        }
    }

    /// Original Execute1016DE8B7 ->6E1A70 ->567F70 ->578100. Action+90
    /// names the waypoint; current Player+240 gates both local conceal and
    /// the immediate whole-map update independently of the Trigger House.
    /// The signed half-level and bridge100/400 receiver match Action17,
    /// while conceal geometry and the complete two-pass sweep stay shared
    /// with their own owners. Steam gamemd, executed13 empty-resident
    /// controls in tools/spatial_oracle/trigger_conceal_area.json. No own
    /// RNG draw, timer or detach; resident callbacks use5865F0's owner.
    pub(crate) fn trigger_conceal_at_waypoint(
        &mut self,
        index: i32,
        context: UninitContext<'_>,
    ) -> bool {
        let viewers = self.represented_player_maps();
        if viewers.is_empty() {
            return false;
        }
        if viewers
            .iter()
            .all(|&viewer| self.fog.whole_map_revealed_for_owner(viewer))
        {
            return true;
        }
        let rules = context.rules();
        let cell = self.session.waypoint_cell(index).unwrap_or((0, 0));
        let coord = {
            let Some(terrain) = context.terrain().or(self.resolved_terrain.as_ref()) else {
                return false;
            };
            let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
            let bridge = cells.flags(cells.lookup(cell)) & 0x100 != 0
                || cells.flags(cells.lookup(cell)) & 0x400 != 0;
            let level = i32::from(cells.ground_fields(cells.lookup(cell)).0 as i8)
                + if bridge { 4 } else { 0 };
            let half = (level / 2) as i16;
            [
                i32::from(cell.0.wrapping_sub(half))
                    .wrapping_shl(8)
                    .wrapping_add(128),
                i32::from(cell.1.wrapping_sub(half))
                    .wrapping_shl(8)
                    .wrapping_add(128),
                level * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS,
            ]
        };
        let cells = vision::conceal_area_cells(coord, rules.reveal_trigger_radius());
        #[cfg(any(test, feature = "test-support"))]
        REVEAL_CALLS.with_borrow_mut(|trace| {
            if let Some(trace) = trace {
                trace.walks.push(cells.clone());
            }
        });
        for viewer in viewers {
            if self.fog.whole_map_revealed_for_owner(viewer) {
                continue;
            }
            for &cell in &cells {
                let native_cells = context
                    .terrain()
                    .or(self.resolved_terrain.as_ref())
                    .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
                self.fog
                    .conceal_map_cell(viewer, cell, native_cells.as_ref());
            }
            self.update_shroud_edges(viewer, context);
        }
        true
    }

    /// Logic55B29A/55B2AD's admission is outside the578100 owner: Action101
    /// invokes that same complete update immediately, at any frame. Each
    /// represented viewer has its own native map plane and notification
    /// history; another viewer's pending state never supplies this one.
    pub(super) fn tick_pending_shroud_conceal(&mut self, context: UninitContext<'_>) {
        if (self.session.binary_frame() as i32) % 120 != 0 {
            return;
        }
        // These scans can synchronously raise discovery Tags. HashMap order
        // must never choose their cross-House effects or RNG order. Match the
        // retained House registry; fixture-only planes have stable ID order.
        let mut viewers: Vec<_> = self
            .houses
            .order()
            .iter()
            .copied()
            .filter(|owner| self.fog.by_owner().contains_key(owner))
            .collect();
        let mut extra: Vec<_> = self
            .fog
            .by_owner()
            .keys()
            .copied()
            .filter(|owner| !self.houses.order().contains(owner))
            .collect();
        extra.sort_unstable();
        viewers.extend(extra);
        for viewer in viewers {
            self.update_shroud_edges(viewer, context);
        }
    }

    /// Map578100's two complete allocated passes. A synchronous discovery
    /// may modify shroud or reset the shared cursor; do not retain a cursor,
    /// Cell snapshot or visibility borrow across its callback.
    ///
    /// On a regular terrain ([`AllocatedCellOrder`]) each pass visits only the
    /// cells the viewer's map lists as changed ([`vision::ListedEdgeCells`]),
    /// in walk order: the walk's other cells neither change nor notify. It
    /// leaves the shared cursor and the canonical Dummy as the complete walk
    /// does, and falls back to that walk when a callback moves the cursor.
    /// Tests visiting every cell take the complete walk.
    fn update_shroud_edges(
        &mut self,
        viewer: crate::sim::intern::InternedId,
        context: UninitContext<'_>,
    ) {
        self.update_shroud_edges_notifying(viewer, context, &mut |sim, notify| {
            sim.notify_cell_change(notify.cell, viewer, notify.fresh, context);
        });
    }

    /// [`Self::update_shroud_edges`] with `notify` receiving each change
    /// synchronously, as CellChangeNotify does.
    fn update_shroud_edges_notifying(
        &mut self,
        viewer: crate::sim::intern::InternedId,
        context: UninitContext<'_>,
        notify: &mut dyn FnMut(&mut Self, vision::MapCellNotification),
    ) {
        let order = self
            .map_size_diamond()
            .filter(|_| !vision::visit_every_cell())
            .and_then(|(width, _)| {
                let terrain = context.terrain().or(self.resolved_terrain.as_ref())?;
                Some((
                    terrain.allocated_cell_order(width)?,
                    (terrain.width(), terrain.height()),
                ))
            });
        for refresh in [false, true] {
            match &order {
                Some((order, terrain_size)) => {
                    let listed = self.fog.take_edge_cells(viewer, refresh, *terrain_size);
                    let every = listed.is_none();
                    self.walk_edge_cells(viewer, refresh, order, listed, context, notify);
                    self.fog.finish_edge_walk(viewer);
                    if refresh && every {
                        self.fog.complete_edge_cells(viewer);
                    }
                }
                None => {
                    self.fog.untrack_edge_cells(viewer);
                    self.for_each_allocated_map_cell(context, |sim, cell| {
                        if let Some(change) = Self::visit_edge_cell(
                            &mut sim.fog,
                            &sim.resolved_terrain,
                            viewer,
                            cell,
                            refresh,
                            context,
                        ) {
                            notify(sim, change);
                        }
                    });
                }
            }
        }
        #[cfg(any(test, feature = "test-support"))]
        if let Some((order, _)) = &order
            && let Some(terrain) = context.terrain().or(self.resolved_terrain.as_ref())
        {
            let cells = crate::map::resolved_terrain::NativeCellQuery::isolated(terrain);
            let missing = order.missing_neighbors_through(u32::MAX);
            for rank in 0..order.len() {
                let beside_dummy = missing
                    .binary_search_by_key(&rank, |&(rank, _)| rank)
                    .is_ok();
                self.fog
                    .assert_edge_cell_tracked(viewer, order.cell(rank), beside_dummy, &cells);
            }
        }
    }

    /// One Map578100 receiver: the sweep's, or with `refresh` the ground-frame
    /// refresh's.
    fn visit_edge_cell(
        fog: &mut FogState,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        viewer: crate::sim::intern::InternedId,
        cell: (u16, u16),
        refresh: bool,
        context: UninitContext<'_>,
    ) -> Option<vision::MapCellNotification> {
        #[cfg(any(test, feature = "test-support"))]
        REVEAL_CALLS.with_borrow_mut(|trace| {
            if let Some(trace) = trace {
                trace.edge_visits += 1;
            }
        });
        let cells = context
            .terrain()
            .or(resolved_terrain.as_ref())
            .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
        let change = if refresh {
            fog.refresh_map_cell_ground_frame(viewer, cell, cells.as_ref())
        } else {
            fog.sweep_pending_map_cell(viewer, cell, cells.as_ref())
        };
        #[cfg(any(test, feature = "test-support"))]
        if let Some(change) = &change {
            REVEAL_CALLS.with_borrow_mut(|trace| {
                if let Some(trace) = trace {
                    trace
                        .edge_notifications
                        .push((refresh, change.cell, change.fresh));
                }
            });
        }
        change
    }

    /// One Map578100 pass over `order`'s cells in walk order: the `listed`
    /// ones (for a refresh with their neighbours, whose frames read them), or
    /// every cell. Before each notification the shared cursor and the Dummy
    /// hold what the complete walk leaves there, and cells the callback lists
    /// later in the walk join this pass.
    fn walk_edge_cells(
        &mut self,
        viewer: crate::sim::intern::InternedId,
        refresh: bool,
        order: &AllocatedCellOrder,
        listed: Option<vision::ListedEdgeCells>,
        context: UninitContext<'_>,
        notify: &mut dyn FnMut(&mut Self, vision::MapCellNotification),
    ) {
        let mut ranks = RankSet::new(order.len());
        match listed {
            None => ranks.fill(order.len()),
            Some(listed) => ranks.add_listed(order, &listed, refresh, 0),
        }
        let mut notified = None;
        while let Some(rank) = ranks.pop() {
            let Some(change) = Self::visit_edge_cell(
                &mut self.fog,
                &self.resolved_terrain,
                viewer,
                order.cell(rank),
                refresh,
                context,
            ) else {
                continue;
            };
            if refresh {
                self.restamp_edge_dummy(viewer, order, notified, rank, context);
            }
            let cursor = order.cursor_after(rank);
            self.native_allocated_cell_cursor = Some(cursor);
            let mark = self.fog.edge_cells_listed(viewer, refresh);
            notify(self, change);
            notified = Some(rank);
            if self.native_allocated_cell_cursor != Some(cursor) {
                // A nested walk moved the shared cursor: the complete walk
                // goes on from there, and the cells it may skip wait for
                // the next update.
                let rest: Vec<_> = ranks.drain().map(|rank| order.cell(rank)).collect();
                self.fog.relist_edge_cells(viewer, refresh, rest);
                while let Some(cell) = self.next_allocated_map_cell(context) {
                    if let Some(change) = Self::visit_edge_cell(
                        &mut self.fog,
                        &self.resolved_terrain,
                        viewer,
                        cell,
                        refresh,
                        context,
                    ) {
                        notify(self, change);
                    }
                }
                return;
            }
            let since = self.fog.edge_cells_since(viewer, refresh, mark);
            ranks.add_listed(order, &since, refresh, rank + 1);
        }
        if refresh && let Some(last) = order.len().checked_sub(1) {
            self.restamp_edge_dummy(viewer, order, notified, last, context);
        }
        self.native_allocated_cell_cursor = Some(order.end());
    }

    /// Leave the canonical Dummy as the complete refresh walk's lookups leave
    /// it once it has visited `through`: stamped by the last unallocated
    /// neighbour of the last cell after `after` whose frame reads its
    /// neighbours. Without such a cell the walk stamped nothing since `after`.
    fn restamp_edge_dummy(
        &self,
        viewer: crate::sim::intern::InternedId,
        order: &AllocatedCellOrder,
        after: Option<u32>,
        through: u32,
        context: UninitContext<'_>,
    ) {
        let Some(terrain) = context.terrain().or(self.resolved_terrain.as_ref()) else {
            return;
        };
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        for &(rank, missing) in order.missing_neighbors_through(through).iter().rev() {
            if after.is_some_and(|after| rank <= after) {
                return;
            }
            let cell = order.cell(rank);
            if self
                .fog
                .ground_frame_reads_neighbors(viewer, cell, Some(&cells))
            {
                let (dx, dy) = ROW_NEIGHBORS[7 - missing.leading_zeros() as usize];
                cells.lookup((
                    (cell.0 as i16).wrapping_add(dx),
                    (cell.1 as i16).wrapping_add(dy),
                ));
                return;
            }
        }
    }

    /// Original `TActionClass::RevealAroundWaypoint 0x006E0FE0`, called by
    /// Execute17 at6DE34E. This uses TAction+90 and PlayerPtr, independently
    /// of the trigger's House and the trailing+44 camera waypoint. The
    /// signed level includes Map5657A0's short-circuit bridge100/400 gate;
    /// the truncating half-level subtraction wraps as a signed WORD before
    /// RevealArea5678E0 applies its own Z shift. Executed controls/readers:
    /// tools/spatial_oracle/trigger_action_values.{py,json,meta.json}.
    /// Ledger: no RNG draws, timer writes or detach calls. A negative radius
    /// natively indexes before the count table; VERA maps no cells (retail9).
    pub fn trigger_reveal_at_waypoint(&mut self, index: i32, context: UninitContext<'_>) -> bool {
        let rules = context.rules();
        let viewers = self.represented_player_maps();
        if viewers.is_empty() {
            return false;
        }
        let viewers: Vec<_> = viewers
            .into_iter()
            .filter(|&viewer| !self.fog.whole_map_revealed_for_owner(viewer))
            .collect();
        if viewers.is_empty() {
            return true;
        }
        let Some(cell) = self.session.waypoint_cell(index) else {
            return false;
        };
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        let bridge = cells.flags(cells.lookup(cell)) & 0x100 != 0
            || cells.flags(cells.lookup(cell)) & 0x400 != 0;
        // The third packed-cell query is separate even when a bridge flag
        // short-circuits the second one. Missing cells stamp the shared Dummy.
        let level =
            i32::from(cells.ground_fields(cells.lookup(cell)).0 as i8) + if bridge { 4 } else { 0 };
        let half_level = (level / 2) as i16;
        let center = (
            cell.0.wrapping_sub(half_level) as u16,
            cell.1.wrapping_sub(half_level) as u16,
        );
        let z = level * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS;
        let radius = rules.reveal_trigger_radius().clamp(0, i32::from(u16::MAX)) as u16;
        let config = self.sight_reveal_config(rules);
        let height_grid = self.sight_height_grid(&config);
        // Outline/unreveal/arg6 are zero and by-height is one. Native final0
        // followed by final1 leaves knowledge with no sustained sight source.
        for viewer in viewers {
            for final_pass in [false, true] {
                let _ = self.reveal_area(
                    &[viewer],
                    center,
                    z,
                    radius,
                    Some(&height_grid),
                    final_pass,
                    &config,
                    context,
                    crate::sim::vision::MapRevealProjection::Transient,
                    self.session.current_house(),
                );
            }
        }
        true
    }

    /// Original RevealZone6E11C0, Execute18 at6DE37B: +90 selects a Crusher
    /// raw zone, then local578850/578710 members reveal literal-radius2 pairs
    /// through the shared5678E0 knowledge/discovery owner. PlayerPtr is read
    /// at entry and before each pass, independently of the trigger House.
    /// Executed synthetic controls: tools/spatial_oracle/trigger_reveal_zone.
    /// No own RNG, timer or detach; occupied discovery/Tag effects retain
    /// their existing owners. Retail topology and aircraft drawing parity
    /// are not established by these empty-cell controls.
    pub(crate) fn trigger_reveal_zone_at_waypoint(
        &mut self,
        index: i32,
        context: UninitContext<'_>,
    ) -> bool {
        use crate::map::resolved_terrain::{NativeCellQuery, NativeLocalCellCursor};
        use crate::rules::locomotor_type::MovementZone;

        let viewers = self.represented_player_maps();
        if viewers.is_empty() {
            return false;
        }
        let network = self.session.game_mode().is_network();
        let viewers: Vec<_> = viewers
            .into_iter()
            .filter(|&viewer| !self.fog.whole_map_revealed_for_owner(viewer))
            .collect();
        if viewers.is_empty() {
            return true;
        }
        let rules = context.rules();
        let (Some(waypoint), Some(bounds)) =
            (self.session.waypoint_cell(index), self.playfield_bounds)
        else {
            return false;
        };
        let target_zone = {
            let (Some(terrain), Some(zones)) = (
                context.terrain().or(self.resolved_terrain.as_ref()),
                self.zone_grid.as_ref(),
            ) else {
                return false;
            };
            let query = NativeCellQuery::canonical(terrain);
            let Some(zone) = zones.get_zone_id_native_in_query(
                terrain,
                (waypoint.0 as u16, waypoint.1 as u16),
                MovementZone::Crusher,
                false,
                Some(&query),
            ) else {
                return false;
            };
            zone
        };
        self.native_local_cell_cursor = Some(NativeLocalCellCursor::reset(bounds));
        let mut height_source = self.path_grid.clone();
        let mut height_grid = self.sight_height_grid(&self.sight_reveal_config(rules));
        loop {
            // No borrow/cursor snapshot spans a reveal callback. A nested18
            // resets this same cursor; a nested40 changes the next bounds.
            let identity = {
                let (Some(terrain), Some(bounds), Some(cursor)) = (
                    context.terrain().or(self.resolved_terrain.as_ref()),
                    self.playfield_bounds,
                    self.native_local_cell_cursor.as_mut(),
                ) else {
                    return false;
                };
                cursor.next_in_query(bounds, &NativeCellQuery::canonical(terrain))
            };
            let Some(identity) = identity else {
                return true;
            };
            let half_level = {
                let (Some(terrain), Some(zones)) = (
                    context.terrain().or(self.resolved_terrain.as_ref()),
                    self.zone_grid.as_ref(),
                ) else {
                    return false;
                };
                let query = NativeCellQuery::canonical(terrain);
                let coord = query.coord(identity);
                let Some(zone) = zones.get_zone_id_native_in_query(
                    terrain,
                    (coord.0 as u16, coord.1 as u16),
                    MovementZone::Crusher,
                    false,
                    Some(&query),
                ) else {
                    return false;
                };
                if zone != target_zone {
                    continue;
                }
                i32::from(query.ground_fields(identity).0 as i8) / 2
            };
            let quarter_level = (half_level / 2) as i16;
            for final_pass in [false, true] {
                //6E12B1/6E12B4 reread retained Cell+24 after final0; an
                // intervening miss may have changed a retained Dummy's XY.
                // ESI/EDI retain half/quarter, even if its level changed.
                let center = {
                    let Some(terrain) = context.terrain().or(self.resolved_terrain.as_ref()) else {
                        return false;
                    };
                    let coord = NativeCellQuery::canonical(terrain).coord(identity);
                    (
                        coord.0.wrapping_sub(quarter_level) as u16,
                        coord.1.wrapping_sub(quarter_level) as u16,
                    )
                };
                let active_viewers = if network {
                    viewers.clone()
                } else {
                    let Some(viewer) = self.session.current_house() else {
                        return false;
                    };
                    vec![viewer]
                };
                let config = self.sight_reveal_config(rules);
                // Navigation publishes a new immutable Arc when heights
                // change. Reuse its snapshot across small radius2 passes;
                // a synchronous discovery callback can republish it, so
                // invalidate before the very next pass, not just next tick.
                let same_heights = match (&height_source, &self.path_grid) {
                    (Some(before), Some(current)) => std::sync::Arc::ptr_eq(before, current),
                    (None, None) => true,
                    _ => false,
                };
                if !same_heights {
                    height_grid = self.sight_height_grid(&config);
                    height_source = self.path_grid.clone();
                }
                for viewer in active_viewers {
                    let _ = self.reveal_area(
                        &[viewer],
                        center,
                        half_level * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS,
                        2,
                        Some(&height_grid),
                        final_pass,
                        &config,
                        context,
                        crate::sim::vision::MapRevealProjection::Transient,
                        self.session.current_house(),
                    );
                }
            }
        }
    }

    /// Talk70F120's Map5673A0(2,Player,0,0,0,1,1): uncached4A9CA0
    /// final0 plus its border. Shared geometry/cache/discovery owners; no RNG.
    pub(crate) fn reveal_talk_bubble_actor(&mut self, actor: u64, context: UninitContext<'_>) {
        let Some(entity) = self.substrate.entities.get(actor) else {
            return;
        };
        let coord = crate::sim::movement::ground_pose::object_get_coords(
            entity,
            context.terrain().or(self.resolved_terrain.as_ref()),
        );
        let config = self.sight_reveal_config(context.rules());
        //70F15B passesS28=true:56772E..5677AF also gates the existing
        //signed terrain-height query on General.RevealByHeight.
        let heights = self.sight_height_grid(&config);
        let viewers = self.represented_player_maps();
        let cell = ((coord.x / 256) as u16, (coord.y / 256) as u16);
        for viewer in viewers {
            if let Some(origin) = self.reveal_area(
                &[viewer],
                cell,
                coord.z,
                2,
                Some(&heights),
                false,
                &config,
                context,
                vision::MapRevealProjection::UncachedConditional,
                Some(viewer),
            ) {
                Self::refresh_sight_border(
                    &mut self.fog,
                    &self.resolved_terrain,
                    viewer,
                    origin,
                    5,
                    context,
                );
            }
        }
    }

    /// Original MapClass::RevealArea5678E0, ordinary no-outline/no-unreveal
    /// route. Geometry stays with the shared native spiral/height owner;
    /// MapCell4A9CA0 publishes each write and its synchronous discovery
    /// before the next spiral cell. A final0/final1 pair retains knowledge
    /// without a sustained sight source. Executed controls live in
    /// tools/spatial_oracle/trigger_action_values and reveal_area.
    pub(crate) fn reveal_area(
        &mut self,
        viewers: &[crate::sim::intern::InternedId],
        cell: (u16, u16),
        z: i32,
        radius: u16,
        line_of_sight: Option<&SightHeights>,
        final_pass: bool,
        config: &vision::VisionConfig,
        context: UninitContext<'_>,
        projection: vision::MapRevealProjection,
        notification_viewer: Option<crate::sim::intern::InternedId>,
    ) -> Option<(u16, u16, i32)> {
        // Original5678E0 resolves/substitutes its House once at567AB4..567B12,
        // before the loop (backedge567D82→567B2A). Map5673A0 shares this gate
        // at567570..5675D0. Its admitted House is passed through4A9D88/8D
        // and586687/690 to virtual198; callbacks do not requery PlayerPtr.
        // Keep every logical plane's projection, but only this event-entry
        // native player admission may deliver gameplay notifications. A
        // projected AI/other plane is not another native client callback.
        let notification_viewer = notification_viewer.filter(|viewer| viewers.contains(viewer));
        let network = self.session.game_mode().is_network();
        let (cells, local_origin) = config.collect_area_cells(
            cell,
            radius,
            z,
            line_of_sight.and_then(|heights| heights.cell_levels(self.resolved_terrain.as_ref())),
            (self.fog.width(), self.fog.height()),
        );
        #[cfg(any(test, feature = "test-support"))]
        REVEAL_CALLS.with_borrow_mut(|trace| {
            if let Some(trace) = trace {
                trace.walks.push(cells.clone());
            }
        });
        for &viewer in viewers {
            let notify_view = if network {
                self.houses
                    .get(&viewer)
                    .is_some_and(|house| house.is_human())
            } else {
                notification_viewer == Some(viewer)
            };
            if projection == vision::MapRevealProjection::Source
                && !self.session.game_options().fog_of_war
                && !notify_view
            {
                // No receiver hears this plane: walk it in one go.
                let native_cells = context
                    .terrain()
                    .or(self.resolved_terrain.as_ref())
                    .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
                self.fog
                    .reveal_source_map_cells(viewer, &cells, final_pass, native_cells.as_ref());
                continue;
            }
            for &cell in &cells {
                {
                    let native_cells = context
                        .terrain()
                        .or(self.resolved_terrain.as_ref())
                        .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
                    self.fog
                        .clear_pending_fog(viewer, cell, native_cells.as_ref());
                }
                if projection == vision::MapRevealProjection::FogProtect {
                    let admitted = {
                        let native_cells = context
                            .terrain()
                            .or(self.resolved_terrain.as_ref())
                            .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
                        self.fog
                            .admits_fog_reveal(viewer, cell, native_cells.as_ref())
                    };
                    if admitted {
                        self.reveal_fog_map_cell(viewer, cell, notify_view, context);
                    }
                    continue;
                }
                let notify = {
                    let native_cells = context
                        .terrain()
                        .or(self.resolved_terrain.as_ref())
                        .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
                    if matches!(
                        projection,
                        vision::MapRevealProjection::UncachedConditional
                            | vision::MapRevealProjection::Fire
                    ) && self
                        .fog
                        .map_cell_fully_known(viewer, cell, native_cells.as_ref())
                    {
                        continue;
                    }
                    self.fog.reveal_map_cell(
                        viewer,
                        cell,
                        final_pass,
                        native_cells.as_ref(),
                        projection,
                    )
                };
                if let Some(notify) = notify {
                    if notify_view {
                        self.notify_cell_change(notify.cell, viewer, notify.fresh, context);
                    }
                    if notify.clean_fog {
                        self.clean_fog(viewer, notify.cell, context);
                    }
                }
            }
        }
        //5673A0's native House rejection567570..5D0 skips its border too.
        //Keep the admitted local XYZ captured before any synchronous callback;
        //a logical other-viewer projection is not another admitted native call.
        local_origin.filter(|_| (network && !viewers.is_empty()) || notification_viewer.is_some())
    }

    /// Original CellChangeNotify5865F0..5866BA: seven height-admitted
    /// diagonal cells, one nearest ground Techno per cell in native linked
    /// order, then its virtual+198. It does not scan every resident.
    /// Fresh knowledge additionally marks radar terrain dirty (6565A0);
    /// rendering owns the pixel/surface clipping. No RNG, timer or detach.
    pub(crate) fn notify_cell_change(
        &mut self,
        cell: (u16, u16),
        viewer: crate::sim::intern::InternedId,
        fresh: bool,
        context: UninitContext<'_>,
    ) {
        #[cfg(test)]
        {
            let receiver = FIRST_CELL_RECEIVER.with_borrow_mut(Option::take);
            if let Some(receiver) = receiver {
                receiver(self);
            }
        }
        #[cfg(any(test, feature = "test-support"))]
        REVEAL_CALLS.with_borrow_mut(|trace| {
            if let Some(trace) = trace {
                trace.notifications.push((cell, fresh));
            }
        });
        for rung in 0..7i16 {
            let diagonal = (
                (cell.0 as i16).wrapping_add(rung),
                (cell.1 as i16).wrapping_add(rung),
            );
            let candidate = {
                let Some(terrain) = context.terrain().or(self.resolved_terrain.as_ref()) else {
                    continue;
                };
                let query = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
                let identity = query.lookup(diagonal);
                let level = i16::from(query.ground_fields(identity).0 as i8);
                if level < 2 * rung - 1 || level > 2 * rung + 1 {
                    continue;
                }
                let coord = query.coord(identity);
                (coord.0 as u16, coord.1 as u16)
            };
            if fresh {
                self.dirty_cells.mark_radar([candidate]);
            }
            #[cfg(any(test, feature = "test-support"))]
            REVEAL_CALLS.with_borrow_mut(|trace| {
                if let Some(trace) = trace {
                    trace.discovery_queries.insert(candidate);
                }
            });
            if let Some(id) = self.nearest_cell_object(
                candidate,
                crate::sim::movement::locomotor::MovementLayer::Ground,
                None,
            ) {
                self.discover_techno(id, viewer, context);
            }
        }
    }

    /// `MapClass::RevealEntireMap @ 0x00577F30` for `house`. `None` is the
    /// native NULL that `Read_Scenario` passes for `Shroud=no`
    /// (`0x006849E7`); `MPlayer_Defeated` passes the defeated local house
    /// (`0x004FC20F`).
    /// - A named house's map-clear byte (`+0x241`) is set (`0x00577F48`).
    /// - Only the current house (`PlayerPtr`) or NULL goes on
    ///   (`0x00577F4F..0x00577F63`). Once per House+240
    ///   ([`vision::FogState::admit_whole_map_reveal`]), every allocated cell
    ///   in `CellIterator_Init @ 0x00578350` / `Next @ 0x00578290` order is
    ///   revealed to the current house ([`vision::FogState::reveal_cell`]),
    ///   each change notifying its cell ([`Self::notify_cell_change`]) before
    ///   the next cell.
    /// - GameMode 3 and 4 skip three cells (`0x00577F72..0x00578041`) when a
    ///   session object answers false; VERA has no session object (as in
    ///   [`vision::collect_reveal_cells`]).
    /// - The full radar refresh (`0x00657CE0`) and the redraw request
    ///   (`0x004F42F0`) are presentation; the app re-reads the fog each frame.
    ///
    /// Unlike [`Self::reveal_map_for_owner`] it writes no shroud counter and
    /// releases no sight or gap. No RNG draw, timer write or detach.
    pub fn reveal_entire_map(
        &mut self,
        house: Option<crate::sim::intern::InternedId>,
        context: UninitContext<'_>,
    ) {
        if let Some(state) = house.and_then(|house| self.houses.get_mut(&house)) {
            state.mark_map_clear();
        }
        if self.session.game_mode().is_network() {
            // Map577F4F admits PlayerPtr or NULL. VERA retains each human's
            // map, so a named human updates that same map on every peer;
            // NULL (Shroud=no) updates every represented human in House order.
            // Source: original gamemd.exe577F30..57807A.
            let viewers: Vec<_> = self
                .houses
                .order()
                .iter()
                .copied()
                .filter(|viewer| {
                    house.is_none_or(|house| house == *viewer)
                        && self
                            .houses
                            .get(viewer)
                            .is_some_and(|state| state.is_human())
                })
                .collect();
            for viewer in viewers {
                if self.fog.admit_whole_map_reveal(viewer) {
                    self.reveal_allocated_map_cells(false, Some(viewer), context);
                }
            }
        } else {
            let Some(player) = self.session.current_house() else {
                return;
            };
            if house.is_some_and(|house| house != player)
                || !self.fog.admit_whole_map_reveal(player)
            {
                return;
            }
            self.reveal_allocated_map_cells(false, None, context);
        }
    }

    /// Execute16 at6DE3A8: local House240 gate/set,578350/578290, then
    ///653810->Display4A9890 per allocated cell and final redraw. House241,
    /// sight/gap counters and sustained sources are untouched. Current
    /// PlayerPtr is reread for every receiver after synchronous callbacks.
    pub(crate) fn trigger_reveal_entire_map(&mut self, context: UninitContext<'_>) -> bool {
        let network = self.session.game_mode().is_network();
        let viewers = self.represented_player_maps();
        if viewers.is_empty()
            || viewers
                .iter()
                .all(|&viewer| self.fog.whole_map_revealed_for_owner(viewer))
        {
            return false;
        }
        if context
            .terrain()
            .or(self.resolved_terrain.as_ref())
            .is_none()
            || self.map_size_diamond().is_none()
        {
            return false;
        }
        for viewer in viewers {
            if self.fog.admit_whole_map_reveal(viewer) {
                self.reveal_allocated_map_cells(true, network.then_some(viewer), context);
            }
        }
        true
    }

    /// The simulation's one578350/578290 traversal host.16 and577F30 use
    ///different cell receivers; neither precollects cells before callbacks.
    ///Nested simulation reveals reset this same cursor, not an outer copy.
    fn reveal_allocated_map_cells(
        &mut self,
        forced: bool,
        represented_viewer: Option<InternedId>,
        context: UninitContext<'_>,
    ) {
        self.for_each_allocated_map_cell(context, |sim, cell| {
            sim.reveal_allocated_map_cell(cell, forced, represented_viewer, context);
        });
    }

    fn for_each_allocated_map_cell(
        &mut self,
        context: UninitContext<'_>,
        mut receive: impl FnMut(&mut Self, (u16, u16)),
    ) {
        if self.reset_allocated_map_cell_cursor() {
            while let Some(cell) = self.next_allocated_map_cell(context) {
                receive(self, cell);
            }
            return;
        }
        // Preserve the explicit synthetic-fixture fallback; no native map
        //or shared578350 cursor is claimed without Size fields.
        let cells: Vec<(u16, u16)> = match context.terrain().or(self.resolved_terrain.as_ref()) {
            Some(terrain) => terrain.iter().map(|cell| (cell.rx, cell.ry)).collect(),
            None => self.fog.rectangular_cells(),
        };
        for cell in cells {
            receive(self, cell);
        }
    }

    pub(super) fn reset_allocated_map_cell_cursor(&mut self) -> bool {
        let Some((width, _)) = self.map_size_diamond() else {
            return false;
        };
        self.native_allocated_cell_cursor =
            Some(crate::map::resolved_terrain::NativeAllocatedCellCursor::reset(width));
        true
    }

    /// No simulation borrow spans a cell receiver. Native next578290 reads
    /// the shared cursor after callbacks, including an inner completed scan.
    pub(super) fn next_allocated_map_cell(
        &mut self,
        context: UninitContext<'_>,
    ) -> Option<(u16, u16)> {
        let (width, _) = self.map_size_diamond()?;
        let terrain = context.terrain().or(self.resolved_terrain.as_ref())?;
        let identity = self
            .native_allocated_cell_cursor
            .as_mut()?
            .next_in_terrain(terrain, width)?;
        let coord = terrain.native_cell_coord(identity);
        Some((coord.0 as u16, coord.1 as u16))
    }

    fn reveal_allocated_map_cell(
        &mut self,
        cell: (u16, u16),
        forced: bool,
        represented_viewer: Option<InternedId>,
        context: UninitContext<'_>,
    ) {
        // Campaign/offline preserves the native per-cell PlayerPtr reread.
        // A network projection keeps the explicitly admitted House viewpoint;
        // another client's input identity cannot select this plane.
        let Some(player) = represented_viewer.or_else(|| self.session.current_house()) else {
            return;
        };
        let notify = {
            let cells = context
                .terrain()
                .or(self.resolved_terrain.as_ref())
                .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
            if forced {
                self.fog
                    .reveal_forced_map_cell(player, cell, cells.as_ref())
            } else {
                self.fog.reveal_cell(player, cell, cells.as_ref())
            }
        };
        if let Some(notify) = notify {
            self.notify_cell_change(notify.cell, player, notify.fresh, context);
            if notify.clean_fog {
                self.clean_fog(player, notify.cell, context);
            }
        }
    }

    /// `MapClass::Reveal @ 0x00577D90` for `owner`: it sets the house's
    /// map-clear byte (`+0x241`, `0x00577DB0`), then explores every allocated
    /// cell ([`Self::transition_house_map`]). A Spy Satellite coming online
    /// and the Reveal crate call it.
    pub(crate) fn reveal_map_for_owner(
        &mut self,
        owner: crate::sim::intern::InternedId,
        rules: &RuleSet,
    ) {
        self.transition_house_map(owner, vision::MapTransition::Reveal, rules);
    }

    /// `MapClass::Reset_Shroud @ 0x00577AB0` for `owner`: it clears the
    /// house's map-clear byte (`+0x241`, `0x00577ACA`), then shrouds every
    /// allocated cell again ([`Self::transition_house_map`]). The last Spy
    /// Satellite going offline, the Darkness crate and a spy in a radar
    /// building call it.
    pub(crate) fn reset_shroud_for_owner(
        &mut self,
        owner: crate::sim::intern::InternedId,
        rules: &RuleSet,
    ) {
        self.transition_house_map(owner, vision::MapTransition::ResetShroud, rules);
    }

    /// Execute51 -> Map577C90. Its nonnull House loses241 even when it is
    /// not PlayerPtr; only null/current Player changes the local map. The
    /// allocated-cell receiver leaves487690 counters rather than overwriting
    /// them as577AB0 does. Steam gamemd,577C90..577D89.
    pub(crate) fn trigger_reshroud_map(
        &mut self,
        incoming: Option<crate::sim::intern::InternedId>,
        rules: &RuleSet,
    ) {
        if let Some(owner) = incoming.and_then(|owner| self.houses.get_mut(&owner)) {
            owner.mark_map_shrouded();
        }
        for player in self.represented_player_maps() {
            if incoming.is_none_or(|owner| owner == player) {
                self.transition_house_map(player, vision::MapTransition::Reshroud, rules);
            }
        }
    }

    /// LogicClass::AI55B20B..55B27B, before the pending-shroud/fog rungs.
    /// gamemd.exe tests ShroudGrow and ordered nonzero ShroudRate, then the
    /// Scenario timer; neither FogOfWar nor a mode/editor flag gates it.
    pub(super) fn tick_shroud_growth(&mut self, context: UninitContext<'_>) {
        let rules = &context.rules().general;
        let rate = f64::from_bits(rules.shroud_rate.bits());
        if !rules.shroud_grow
            || rate == 0.0
            || rate.is_nan()
            || !self
                .session
                .poll_shroud_growth(rules.shroud_growth_interval_frames())
        {
            return;
        }
        self.grow_shroud(context);
    }

    /// DisplayClass::GrowShroud4ACAC0, shared by Logic55B27B and
    /// Execute31 at6DE2B5, then4ADFF0(0,0). The cell passes
    /// leave sight admissions/House240/241 alone. The Ground display vector
    /// 8A0394 is re-read after each See callback, whose discovery can raise
    /// Tags. AbstractFlags14/1 admits every GameEntity, with no Alive90 gate.
    pub(crate) fn grow_shroud(&mut self, context: UninitContext<'_>) {
        let viewers = self.represented_player_maps();
        if let Some(size) = self.map_size_diamond() {
            let cells = context
                .terrain()
                .or(self.resolved_terrain.as_ref())
                .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
            for &viewer in &viewers {
                self.fog.grow_shroud(viewer, size, cells.as_ref());
            }
        }
        if self.session.game_mode().is_network() {
            for viewer in viewers {
                self.refresh_techno_shroud(false, false, Some(viewer), context);
            }
        } else {
            self.refresh_techno_shroud(false, false, None, context);
        }
    }

    /// Shared DisplayClass::RefreshTechnoShroud4ADFF0 Ground-vector visitor.
    fn refresh_techno_shroud(
        &mut self,
        skip_buildings: bool,
        reveal_fog: bool,
        only_viewer: Option<crate::sim::intern::InternedId>,
        context: UninitContext<'_>,
    ) {
        let mut ordinal = 0;
        while let Some(id) = self
            .substrate
            .display
            .members(super::display_layers::DisplayLayer::GROUND)
            .get(ordinal)
            .copied()
        {
            let arguments = self.substrate.entities.get(id).and_then(|entity| {
                let building = entity.category() == EntityCategory::Structure;
                if skip_buildings && building {
                    return None;
                }
                // House50B6F0 compares against PlayerPtr in a nonzero
                // mode. Project that same test onto the explicit viewer;
                // the process-local receiver belongs only to ordinary calls.
                let human = only_viewer
                    .filter(|_| self.session.game_mode_nonzero())
                    .map_or_else(
                        || self.house_is_human_player(entity.owner()),
                        |viewer| entity.owner() == viewer,
                    );
                if human {
                    let discovered = only_viewer.map_or_else(
                        || entity.discovery.discovered_by_current_house(),
                        |viewer| {
                            if self.session.game_mode().is_network() {
                                self.techno_discovered_by_viewer(id, viewer)
                            } else {
                                self.fog.sight_is_latched(id, viewer)
                            }
                        },
                    );
                    discovered.then_some((false, reveal_fog))
                } else if building
                    && context.rules().general.ally_reveal
                    && only_viewer
                        .or(self.session.current_house())
                        .is_some_and(|viewer| {
                            crate::map::houses::is_allied_with(
                                self.house_alliances.as_map(),
                                self.interner.resolve(entity.owner()),
                                self.interner.resolve(viewer),
                            )
                        })
                {
                    Some((reveal_fog, false))
                } else {
                    None
                }
            });
            if let Some((incremental, skip_reveal)) = arguments {
                self.reveal_uncached_object_sight(
                    id,
                    incremental,
                    skip_reveal,
                    only_viewer,
                    context,
                );
            }
            ordinal += 1;
        }
        //6DA7D0/virtual38(1) mark presentation dirty. Fog's existing
        //ground-change tracking and view publication consume those writes.
    }

    /// Logic55B2B2..55B326: after pending shroud, before lighting/ore/AI.
    /// The timer reload precedes callbacks; no RNG draw or object detach.
    pub(super) fn tick_fog_regrowth(&mut self, context: UninitContext<'_>) {
        let rate = f64::from_bits(context.rules().general.fog_rate.bits());
        if !self.session.game_options().fog_of_war
            || rate == 0.0
            || rate.is_nan()
            || !self
                .session
                .poll_fog_regrowth(context.rules().general.fog_regrowth_interval_frames())
        {
            return;
        }
        // Native holds one local viewer. VERA retains each human's plane
        // in stable House order, so selecting a different local player cannot
        // alter shared fog hashes, snapshot IDs or the timer's effects.
        for viewer in self.fog_viewers() {
            self.for_each_allocated_map_cell(context, |sim, cell| {
                let cells = context
                    .terrain()
                    .or(sim.resolved_terrain.as_ref())
                    .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
                sim.fog.mark_pending_fog(viewer, cell, cells.as_ref());
            });
            self.refresh_techno_shroud(false, true, Some(viewer), context);
            self.for_each_allocated_map_cell(context, |sim, cell| {
                let pending = {
                    let cells = context
                        .terrain()
                        .or(sim.resolved_terrain.as_ref())
                        .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
                    sim.fog.take_pending_fog(viewer, cell, cells.as_ref())
                };
                if pending {
                    sim.regrow_fog_at_cell(viewer, cell, context);
                }
            });
        }
    }

    pub(super) fn fog_viewers(&self) -> Vec<crate::sim::intern::InternedId> {
        if self.session.game_mode().is_network() {
            return self.represented_player_maps();
        }
        let current = self.session.current_house();
        let mut viewers: Vec<_> = self
            .houses
            .order()
            .iter()
            .copied()
            .filter(|house| Some(*house) == current || self.owner_is_human(*house))
            .collect();
        if let Some(current) = current
            && !viewers.contains(&current)
        {
            viewers.push(current);
        }
        viewers
    }

    fn fog_clients_current_first(&self) -> Vec<crate::sim::intern::InternedId> {
        if self.session.game_mode().is_network() {
            return self.represented_player_maps();
        }
        let mut clients = self.fog_viewers();
        if let Some(current) = self.session.current_house() {
            clients.retain(|&viewer| viewer != current);
            clients.insert(0, current);
        }
        clients
    }

    /// RegrowFogAtCell4ACC50, with an explicit DFS stack preserving its
    /// neighbor and postorder FogCell calls without depending on host stack size.
    /// Residual: native480243 rendering also stores Cell121. After a
    /// Reshroud577D30 clear, a viewed cell can therefore suppress stale-edge
    /// recursion which an offscreen cell retains. VERA keeps this cache in
    /// simulation so camera/render frequency cannot alter deterministic state;
    /// a stock campaign-visible consequence has not been established.
    fn regrow_fog_at_cell(
        &mut self,
        viewer: crate::sim::intern::InternedId,
        cell: (u16, u16),
        context: UninitContext<'_>,
    ) {
        let in_bounds = |cell: (u16, u16)| {
            self.map_size_diamond().map_or(
                cell.0 < self.fog.width() && cell.1 < self.fog.height(),
                |(w, h)| {
                    crate::map::playfield::size_diamond_contains(
                        w,
                        h,
                        (cell.0 as i16, cell.1 as i16),
                    )
                },
            )
        };
        if !in_bounds(cell) {
            return;
        }
        let begin = |sim: &mut Self, cell| {
            let cells = context
                .terrain()
                .or(sim.resolved_terrain.as_ref())
                .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
            sim.fog
                .begin_fog_regrowth_cell(viewer, cell, cells.as_ref())
        };
        let mut stack = vec![(cell, 0usize, begin(self, cell))];
        while let Some((current, ordinal, _)) = stack.last_mut() {
            if *ordinal == 8 {
                let (current, _, was_known) = stack.pop().unwrap();
                if was_known {
                    self.fog_cell(viewer, current, context);
                }
                continue;
            }
            let (dx, dy) = crate::map::resolved_terrain::ROW_NEIGHBORS[*ordinal];
            *ordinal += 1;
            let next = (
                (current.0 as i16).wrapping_add(dx) as u16,
                (current.1 as i16).wrapping_add(dy) as u16,
            );
            let recurse = {
                let cells = context
                    .terrain()
                    .or(self.resolved_terrain.as_ref())
                    .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
                self.fog
                    .refresh_regrown_fog_neighbor(viewer, next, cells.as_ref())
            };
            let in_bounds = self.map_size_diamond().map_or(
                next.0 < self.fog.width() && next.1 < self.fog.height(),
                |(w, h)| {
                    crate::map::playfield::size_diamond_contains(
                        w,
                        h,
                        (next.0 as i16, next.1 as i16),
                    )
                },
            );
            if recurse && in_bounds {
                stack.push((next, 0, begin(self, next)));
            }
        }
    }

    /// Full_Init687C63 ->5866C0 freezes never-mapped cells after map setup.
    pub(crate) fn initialize_fog_of_war(&mut self, context: UninitContext<'_>) {
        if !self.session.game_options().fog_of_war {
            return;
        }
        for viewer in self.fog_viewers() {
            self.for_each_allocated_map_cell(context, |sim, cell| {
                let initialized = {
                    let cells = context
                        .terrain()
                        .or(sim.resolved_terrain.as_ref())
                        .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
                    sim.fog.fog_cell_initialized(viewer, cell, cells.as_ref())
                };
                if !initialized {
                    sim.fog_cell(viewer, cell, context);
                }
            });
        }
    }

    /// RevealFogCell4A9DD0 updates only the fog channel, then notifies and
    /// cleans remembered objects. Ground knowledge and counters are retained.
    fn reveal_fog_map_cell(
        &mut self,
        viewer: crate::sim::intern::InternedId,
        cell: (u16, u16),
        notify_viewer: bool,
        context: UninitContext<'_>,
    ) {
        let notify = {
            let cells = context
                .terrain()
                .or(self.resolved_terrain.as_ref())
                .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
            self.fog.begin_fog_reveal(viewer, cell, cells.as_ref())
        };
        for (dx, dy) in crate::map::resolved_terrain::ROW_NEIGHBORS {
            let next = (
                (cell.0 as i16).wrapping_add(dx) as u16,
                (cell.1 as i16).wrapping_add(dy) as u16,
            );
            let recurse = {
                let cells = context
                    .terrain()
                    .or(self.resolved_terrain.as_ref())
                    .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
                self.fog
                    .propagate_map_cell_edge(viewer, next, true, cells.as_ref())
            };
            if recurse {
                self.reveal_fog_map_cell(viewer, next, notify_viewer, context);
            }
        }
        if let Some(notify) = notify {
            if notify_viewer {
                self.notify_cell_change(notify.cell, viewer, notify.fresh, context);
            }
            if notify.clean_fog {
                self.clean_fog(viewer, notify.cell, context);
            }
        }
    }

    /// The bulk store that `Reveal` and `Reset_Shroud` share. It is bracketed
    /// by `0x004ADEE0` (each callback object releases its sight and gap on
    /// `owner`'s map) and `0x004ADCD0` (each re-admits them, in registration
    /// order, its gap as it is now). Gap removal reads the house's Spy
    /// Satellite byte (`+0x577A`), which its caller changes only afterwards.
    /// The map-clear byte (`+0x241`) is written whatever the map does. A
    /// reveal is skipped once the map is revealed (`+0x240`); a reset never
    /// is. Natively only `PlayerPtr`'s shroud changes; VERA keeps one plane
    /// per house.
    fn transition_house_map(
        &mut self,
        owner: crate::sim::intern::InternedId,
        transition: vision::MapTransition,
        rules: &RuleSet,
    ) {
        let reset = transition != vision::MapTransition::Reveal;
        //577AB0/577D90 store241 before both bracket callbacks. Reshroud
        //577C90 does so only for its nonnull incoming House, already handled
        //by trigger_reshroud_map; the null path must retain Player241.
        if transition != vision::MapTransition::Reshroud
            && let Some(house) = self.houses.get_mut(&owner)
        {
            if reset {
                house.mark_map_shrouded();
            } else {
                house.mark_map_clear();
            }
        }
        if !self.fog.begin_map_transition(owner, transition) {
            return;
        }
        let context = UninitContext::with_rules(rules);
        // MapClass::Reveal577D90, Reset_Shroud577AB0 and Reshroud577C90
        // call the SAME live4ADEE0/4ADCD0 visitors as diplomacy. Source:
        // original gamemd instructions. Each visitor snapshots its count,
        // then reloads the next Techno slot after synchronous callbacks.
        self.walk_map_sight(context, true, Some(owner), false);
        // Native initializes578350's allocated cursor AFTER pre-release.
        // These scalar cell stores dispatch no occupied-object callbacks.
        let cells = if self.reset_allocated_map_cell_cursor() {
            let mut cells = Vec::new();
            while let Some(cell) = self.next_allocated_map_cell(context) {
                cells.push(cell);
            }
            cells
        } else {
            self.resolved_terrain.as_ref().map_or_else(
                || self.fog.rectangular_cells(),
                |terrain| terrain.iter().map(|cell| (cell.rx, cell.ry)).collect(),
            )
        };
        self.fog.store_map_transition(owner, cells, transition);
        self.walk_map_sight(context, false, Some(owner), false);
        if reset {
            //577B8B/577D72 clear240 AFTER post-admission and nested Tags.
            self.fog.invalidate_whole_map_reveal_for_owner(owner);
        }
    }

    /// Explicit footprint/cell fixture adapter with no live Techno registry.
    /// Production map transitions use walk_map_sight's native receiver gates.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn transition_whole_map_with_gap_reentry(
        &mut self,
        owner: crate::sim::intern::InternedId,
        cells: Vec<(u16, u16)>,
        transition: vision::MapTransition,
        _old_spy_sat_active: bool,
        sources: &std::collections::BTreeMap<u64, Option<vision::SightAdmission>>,
        gap_reentry: Option<&[vision::GapGeneratorSource]>,
        alternates: &std::collections::BTreeMap<u64, vision::SightAdmission>,
        config: &vision::VisionConfig,
        context: UninitContext<'_>,
    ) {
        assert!(
            self.substrate.entities.is_empty(),
            "footprint-only map fixture"
        );
        let Some(plan) =
            self.fog
                .prepare_map_transition(owner, transition, sources, gap_reentry, alternates)
        else {
            return;
        };
        for &id in &plan.ids {
            if plan.held.contains(&id) {
                self.release_object_sight_event(
                    id,
                    &[owner],
                    config,
                    context,
                    self.session.current_house(),
                );
            }
            for &gap in plan.gaps.get(&id).into_iter().flatten() {
                self.fog
                    .remove_map_transition_gap(owner, gap, _old_spy_sat_active);
            }
        }
        self.fog.store_map_transition(owner, cells, transition);
        for id in plan.ids {
            if plan.held.contains(&id)
                && let Some(Some(source)) = sources.get(&id)
            {
                self.fog
                    .retain_sight_admission(id, owner, source.clone(), false);
                let heights = self.sight_height_grid(config);
                self.walk_source_sight(
                    owner,
                    source,
                    false,
                    config,
                    Some(&heights),
                    context,
                    self.session.current_house(),
                );
            }
            if let Some(source) = alternates.get(&id) {
                if !self.fog.holds_alternate_sight(id, owner) {
                    self.fog
                        .retain_sight_admission(id, owner, source.clone(), true);
                }
                let heights = self.sight_height_grid(config);
                self.walk_source_sight(
                    owner,
                    source,
                    false,
                    config,
                    Some(&heights),
                    context,
                    self.session.current_house(),
                );
            }
            for &gap in plan.reentry.get(&id).into_iter().flatten() {
                self.fog.reenter_map_transition_gap(owner, gap);
            }
        }
        if transition != vision::MapTransition::Reveal {
            self.fog.invalidate_whole_map_reveal_for_owner(owner);
        }
    }

    /// Techno70AF50 ordinary source admission. Metadata250/254/260 and
    /// every derived viewer index exist before the first Map5678E0 receiver;
    /// Cell4A9CA0 is the only counter/cache writer. There is no post-callback
    /// reinsertion, so a nested Limbo cannot resurrect the outer latch.
    pub(crate) fn update_object_sight_with_context(
        &mut self,
        id: u64,
        radius: i32,
        only_viewer: Option<crate::sim::intern::InternedId>,
        context: UninitContext<'_>,
    ) {
        let config = self.sight_reveal_config(context.rules());
        let heights = self.sight_height_grid(&config);
        self.update_object_sight_in_view(id, radius, only_viewer, &config, Some(&heights), context);
    }

    pub(crate) fn update_object_sight_in_view(
        &mut self,
        id: u64,
        radius: i32,
        only_viewer: Option<crate::sim::intern::InternedId>,
        config: &vision::VisionConfig,
        heights: Option<&SightHeights>,
        context: UninitContext<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let map_viewers = match only_viewer {
            Some(viewer) => vec![viewer],
            None => self
                .reveal_viewers
                .direct(
                    &self.fog,
                    self.house_alliances.as_map(),
                    entity.owner(),
                    config,
                    &self.interner,
                )
                .to_vec(),
        };
        let requested = match only_viewer {
            Some(viewer) => vec![viewer],
            None => self.sight_event_viewers(id, entity.owner(), &map_viewers, true, false),
        };
        self.update_object_sight_for_viewers(id, radius, &requested, config, heights, context);
    }

    /// The logical clients that execute this source event: the active human
    /// and current-house planes, its owner plane, any direct map projections,
    /// and (for a release) clients that previously retained this source.
    /// Registration order is shared with the House owner, not reconstructed
    /// from interned-name or contribution-map ordering.
    fn sight_event_viewers(
        &self,
        id: u64,
        owner: crate::sim::intern::InternedId,
        map_viewers: &[crate::sim::intern::InternedId],
        include_active: bool,
        include_cached: bool,
    ) -> Vec<crate::sim::intern::InternedId> {
        let mut selected = std::collections::BTreeSet::new();
        if include_active {
            selected.extend(self.fog_viewers());
        }
        selected.insert(owner);
        selected.extend(map_viewers.iter().copied());
        if include_cached {
            selected.extend(self.fog.source_sight_viewers(id));
        }
        let mut ordered = Vec::with_capacity(selected.len());
        for &house in self.houses.order() {
            if selected.remove(&house) {
                ordered.push(house);
            }
        }
        ordered.extend(selected);
        ordered
    }

    /// Each active logical client executes its native UpdateReveal state
    /// before the separate RevealArea house gate decides whether that client's
    /// map receives a cell contribution.
    fn update_object_sight_for_viewers(
        &mut self,
        id: u64,
        radius: i32,
        requested_viewers: &[InternedId],
        config: &vision::VisionConfig,
        heights: Option<&SightHeights>,
        context: UninitContext<'_>,
    ) {
        let notification_viewer = self.session.current_house();
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let owner = entity.owner();
        if (config.require_playfield_membership() && !entity.in_playfield())
            || config.passive_owner(owner)
        {
            return;
        }
        //70AF87 tests exact raw Type+5E8 zero before the latch. Effective
        //radius zero from a nonzero raw Sight is admitted and retains250.
        if entity.sight.is_zero() && radius == 0 {
            return;
        }
        let override_radius = (radius != 0).then_some(radius);
        let Some(admission) = vision::entity_sight_admission(
            &self.fog,
            entity,
            owner,
            config,
            heights.and_then(|heights| heights.cell_levels(self.resolved_terrain.as_ref())),
            context.terrain().or(self.resolved_terrain.as_ref()),
            config.sight_ability(entity, &self.interner, context.rules()),
            override_radius,
        ) else {
            return;
        };
        let fresh: Vec<_> = requested_viewers
            .iter()
            .copied()
            .filter(|&viewer| !self.fog.source_sight_is_latched(id, viewer))
            .collect();
        // Each logical PlayerPtr has its own native Techno +250/+254/+260.
        // Publish all source state before any RevealArea receiver can reenter.
        for &viewer in &fresh {
            self.fog.retain_source_sight(id, viewer, owner, &admission);
        }
        let admitted: Vec<_> = fresh
            .iter()
            .copied()
            .filter(|&viewer| {
                vision::reveal_admits(
                    self.house_alliances.as_map(),
                    owner,
                    viewer,
                    config,
                    &self.interner,
                )
            })
            .collect();
        // Register each accepted map contribution before walking any cells.
        // A newly latched source may follow a rejected release while its old
        // knowledge counts remain; retire that stale index without changing
        // the remembered map counts.
        for &viewer in &admitted {
            if self.fog.sight_is_latched(id, viewer) {
                self.fog.abandon_sight_admission(id, viewer);
            }
            self.fog
                .retain_sight_admission(id, viewer, admission.clone(), false);
        }
        for viewer in admitted {
            self.walk_source_sight(
                viewer,
                &admission,
                false,
                config,
                heights,
                context,
                notification_viewer,
            );
        }
    }

    /// ReReveal70B1D0 clears its retained latch before its final1 walk. The
    /// cached XYZ/radius are transported; LOS and map geometry are re-read
    /// now, rather than replaying the derived admission's old cell vector.
    pub(crate) fn release_object_sight_with_context(
        &mut self,
        id: u64,
        context: UninitContext<'_>,
    ) {
        let config = self.sight_reveal_config(context.rules());
        let viewers = if let Some(entity) = self.substrate.entities.get(id) {
            let projected = self.reveal_viewers.direct(
                &self.fog,
                self.house_alliances.as_map(),
                entity.owner(),
                &config,
                &self.interner,
            );
            self.sight_event_viewers(id, entity.owner(), &projected, true, true)
        } else {
            // Some existing release callers drop their entity-index entry
            // before clearing retained sight. In that case only visit the
            // clients already cached for this source; each release reads its
            // own canonical owner and geometry below.
            self.fog.source_sight_viewers(id)
        };
        if viewers.is_empty() {
            return;
        }
        self.release_object_sight_event(
            id,
            &viewers,
            &config,
            context,
            self.session.current_house(),
        );
    }

    /// ReReveal each logical client source before the House gate. A refused
    /// release retires that viewer's contribution index but leaves its map
    /// knowledge counts untouched.
    fn release_object_sight_event(
        &mut self,
        id: u64,
        viewers: &[crate::sim::intern::InternedId],
        config: &vision::VisionConfig,
        context: UninitContext<'_>,
        notification_viewer: Option<crate::sim::intern::InternedId>,
    ) {
        if self.substrate.entities.get(id).is_some_and(|entity| {
            (config.require_playfield_membership() && !entity.in_playfield())
                || config.passive_owner(entity.owner())
        }) {
            return;
        }
        let heights = self.sight_height_grid(config);
        for &viewer in viewers {
            // ReReveal's already-clear client arm must not decrement another
            // source's map counts; only a set native latch owns this release.
            let Some(source) = self.fog.clear_source_sight_latch(id, viewer) else {
                continue;
            };
            if !vision::reveal_admits(
                self.house_alliances.as_map(),
                source.owner,
                viewer,
                config,
                &self.interner,
            ) {
                // Native RevealArea's House gate refuses this release. Retire
                // only the derived index; the old map counts stay remembered.
                self.fog.abandon_sight_admission(id, viewer);
                continue;
            }
            let _ = self.fog.take_sight_admission(id, viewer, false);
            self.walk_source_sight_at(
                viewer,
                source.origin,
                source.radius,
                true,
                config,
                Some(&heights),
                context,
                notification_viewer,
            );
        }
    }

    /// One viewer's `MapClass::RevealArea @ 0x005678E0` walk of `source`
    /// under `heights` ([`Self::sight_height_grid`]).
    fn walk_source_sight(
        &mut self,
        viewer: crate::sim::intern::InternedId,
        source: &vision::SightAdmission,
        final_pass: bool,
        config: &vision::VisionConfig,
        heights: Option<&SightHeights>,
        context: UninitContext<'_>,
        notification_viewer: Option<crate::sim::intern::InternedId>,
    ) {
        self.walk_source_sight_at(
            viewer,
            source.origin,
            source.radius,
            final_pass,
            config,
            heights,
            context,
            notification_viewer,
        );
    }

    /// One `MapClass::RevealArea @ 0x005678E0` walk using the calling logical
    /// client's native cached geometry, independent of its derived contribution.
    fn walk_source_sight_at(
        &mut self,
        viewer: crate::sim::intern::InternedId,
        origin: (u16, u16, i32),
        radius: i32,
        final_pass: bool,
        config: &vision::VisionConfig,
        heights: Option<&SightHeights>,
        context: UninitContext<'_>,
        notification_viewer: Option<crate::sim::intern::InternedId>,
    ) {
        // Non-retail negative radii retain their raw cache but walk no cells.
        // Native567A63 instead indexes before its count table; that out-of-table
        // traversal is not reproduced. Ordinary positive radii clamp at10.
        let _ = self.reveal_area(
            &[viewer],
            (origin.0, origin.1),
            origin.2,
            radius.clamp(0, i32::from(vision::MAX_SIGHT_RANGE)) as u16,
            heights,
            final_pass,
            config,
            context,
            crate::sim::vision::MapRevealProjection::Source,
            notification_viewer,
        );
    }

    /// Ordinary Unlimbo6F6D27 sight precedes border6F6D61 and HouseAdded.
    /// The parent rereads current XYZ and cached260 after synchronous
    /// callbacks, even if they removed the source's retained admission.
    pub(crate) fn reveal_object_sight_on_unlimbo(&mut self, id: u64, context: UninitContext<'_>) {
        self.update_object_sight_with_context(id, 0, None, context);
        self.refresh_object_sight_border(id, context);
    }

    /// Map567DA0's current-source border after Unlimbo6F6D61, Unit
    /// PerCell2 73AC7A..73ACAE or Spy camera4156D1/415854. These parents
    /// reread XYZ and cached260 after the sight callbacks. The shared
    /// release/update pair does not imply this suffix for its other callers.
    pub(crate) fn refresh_object_sight_border(&mut self, id: u64, context: UninitContext<'_>) {
        self.refresh_object_sight_border_on(id, None, context);
    }

    /// [`Self::refresh_object_sight_border`] on `only_viewer`'s map, or else
    /// on each executing logical client's map: around its current XYZ, out
    /// to that map's cached radius (`+0x260`) + 3.
    fn refresh_object_sight_border_on(
        &mut self,
        id: u64,
        only_viewer: Option<crate::sim::intern::InternedId>,
        context: UninitContext<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let config = self.sight_reveal_config(context.rules());
        let origin = (
            entity.position.rx(),
            entity.position.ry(),
            crate::sim::movement::ground_pose::object_world_z_leptons(
                entity,
                context.terrain().or(self.resolved_terrain.as_ref()),
            ),
        );
        let viewers = match only_viewer {
            Some(viewer) => vec![viewer],
            None => {
                let projected = self.reveal_viewers.direct(
                    &self.fog,
                    self.house_alliances.as_map(),
                    entity.owner(),
                    &config,
                    &self.interner,
                );
                //567DA0 has no RevealArea house gate: an enemy's source
                //still refreshes the active client's existing ground edges.
                self.sight_event_viewers(id, entity.owner(), &projected, true, false)
            }
        };
        for &viewer in viewers.iter() {
            //The border reads this client's native +0x260, not a
            //viewer-local contribution. An alternate RevealToAll walk leaves
            //that ordinary cache intact; constructor `0x006F2D96` starts it
            //at0 and ReReveal `0x0070B202` retains it on release.
            let radius = self
                .fog
                .source_sight(id, viewer)
                .map(|source| source.radius)
                .unwrap_or(0);
            Self::refresh_sight_border(
                &mut self.fog,
                &self.resolved_terrain,
                viewer,
                origin,
                radius.wrapping_add(3),
                context,
            );
        }
    }

    /// Map567DA0: ground-frame storage only, with no discovery callback.
    /// Shared by arrival, uncached reveal and both FireAt routes. Renderer
    /// view generations replace6DA7D0's dirty queue, not retained Cell120.
    pub(crate) fn refresh_sight_border(
        fog: &mut FogState,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        viewer: crate::sim::intern::InternedId,
        origin: (u16, u16, i32),
        radius: i32,
        context: UninitContext<'_>,
    ) {
        Self::refresh_sight_border_range(fog, resolved_terrain, viewer, origin, radius, 0, context);
    }

    /// Map567DA0's independent inner ring; Foot4DA77B uses this after its
    /// periodic sight pair, while the other callers use the full disc above.
    fn refresh_sight_border_range(
        fog: &mut FogState,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        viewer: InternedId,
        origin: (u16, u16, i32),
        radius: i32,
        inner: i32,
        context: UninitContext<'_>,
    ) {
        #[cfg(any(test, feature = "test-support"))]
        REVEAL_CALLS.with_borrow_mut(|trace| {
            if let Some(trace) = trace {
                trace.border_scans.push((origin, radius));
            }
        });
        let cells = context
            .terrain()
            .or(resolved_terrain.as_ref())
            .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
        fog.refresh_sight_border(viewer, origin, radius, inner, cells.as_ref());
        // Each refresh writes only its own cell's frame, so every frame
        // read after the whole border is the one its refresh stored.
        #[cfg(any(test, feature = "test-support"))]
        REVEAL_CALLS.with_borrow_mut(|trace| {
            if let Some(trace) = trace {
                let border = vision::sight_border_cells(origin, radius, inner);
                for &cell in &border {
                    trace.border_receivers.push(cell);
                    if cell.0 < fog.width() && cell.1 < fog.height() {
                        trace.border_ground_after.push(
                            fog.by_owner_mut_for_test()
                                .get_mut(&viewer)
                                .expect("the refresh made a plane")
                                .map_cell_edge_cache_for_test(cell)
                                .0[0],
                        );
                    }
                }
            }
        });
    }

    /// Object See vslot120, called by Map567230 and Display4ADFF0.
    /// Aircraft41ADF0 uses a Map5678E0 pair; Techno70ADC0 uses conditional
    /// Map5673A0 plus its border. Neither replaces250/254/260 source state.
    /// Active slots: Aircraft7E23C4=41ADF0, Unit7F5D90=70ADC0.
    pub(crate) fn reveal_uncached_object_sight(
        &mut self,
        id: u64,
        _incremental: bool,
        skip_reveal: bool,
        only_viewer: Option<crate::sim::intern::InternedId>,
        context: UninitContext<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.category() == EntityCategory::Aircraft {
            //41AE00 reads raw Type Sight before GetHeight;41AE10 forces1
            //on the ground. Aircraft See has no Techno playfield/passive,
            //elevation or veterancy gate, and no567DA0 border.
            let mut radius = entity.sight.range();
            if crate::sim::movement::air_movement::current_fly_height(
                entity,
                context.terrain().or(self.resolved_terrain.as_ref()),
            ) == 0
            {
                radius = 1;
            }
            if radius == 0 {
                //41AEA4..41AEAF returns with stock skirmish FogOfWar clear:
                //6AD88F forces g_FOW=0,686BDE/687C16 installs it. Optional
                //enabled-fog unreveal/fogged-object handling is not ported.
                return;
            }
            for final_pass in [false, true] {
                //41AE63..41AE95 recopies live Location/Owner after final0;
                //a synchronous Cell notification may have changed either.
                //The radius stays in EBX across both calls.
                let Some(entity) = self.substrate.entities.get(id) else {
                    return;
                };
                let owner = entity.owner();
                let location = crate::sim::movement::ground_pose::object_location(
                    entity,
                    context.terrain().or(self.resolved_terrain.as_ref()),
                );
                let cell = ((location.x / 256) as u16, (location.y / 256) as u16);
                let config = self.sight_reveal_config(context.rules());
                let viewers = self.reveal_viewers.direct(
                    &self.fog,
                    self.house_alliances.as_map(),
                    owner,
                    &config,
                    &self.interner,
                );
                let viewers: Vec<_> = viewers
                    .iter()
                    .copied()
                    .filter(|viewer| only_viewer.is_none_or(|only| *viewer == only))
                    .collect();
                let heights = self.sight_height_grid(&config);
                let _ = self.reveal_area(
                    &viewers,
                    cell,
                    location.z,
                    radius.clamp(0, i32::from(vision::MAX_SIGHT_RANGE)) as u16,
                    Some(&heights),
                    final_pass,
                    &config,
                    context,
                    if skip_reveal {
                        vision::MapRevealProjection::FogProtect
                    } else {
                        vision::MapRevealProjection::Transient
                    },
                    self.session.current_house(),
                );
            }
            return;
        }
        let notification_viewer = self.session.current_house();
        let config = self.sight_reveal_config(context.rules());
        if (config.require_playfield_membership() && !entity.in_playfield())
            || (self.session.game_mode_nonzero() && config.passive_owner(entity.owner()))
        {
            return;
        }
        let geometry = vision::entity_sight_geometry(
            entity,
            entity.owner(),
            &config,
            context.terrain().or(self.resolved_terrain.as_ref()),
            config.sight_ability(entity, &self.interner, context.rules()),
            None,
        );
        //70AEFD exits on zero effective radius before calling5673A0.
        if geometry.radius == 0 {
            return;
        }
        let viewers = self.reveal_viewers.direct(
            &self.fog,
            self.house_alliances.as_map(),
            entity.owner(),
            &config,
            &self.interner,
        );
        let viewers: Vec<_> = viewers
            .iter()
            .copied()
            .filter(|viewer| only_viewer.is_none_or(|only| *viewer == only))
            .collect();
        let heights = self.sight_height_grid(&config);
        for &viewer in viewers.iter() {
            let border_origin = self.reveal_area(
                &[viewer],
                (geometry.origin.0, geometry.origin.1),
                geometry.origin.2,
                geometry.radius.clamp(0, i32::from(vision::MAX_SIGHT_RANGE)) as u16,
                Some(&heights),
                false,
                &config,
                context,
                if skip_reveal {
                    vision::MapRevealProjection::FogProtect
                } else {
                    vision::MapRevealProjection::UncachedConditional
                },
                notification_viewer,
            );
            let border_origin = border_origin.or_else(|| {
                only_viewer.and_then(|_| {
                    config
                        .collect_area_cells(
                            (geometry.origin.0, geometry.origin.1),
                            geometry.radius.clamp(0, i32::from(vision::MAX_SIGHT_RANGE)) as u16,
                            geometry.origin.2,
                            heights.cell_levels(self.resolved_terrain.as_ref()),
                            (self.fog.width(), self.fog.height()),
                        )
                        .1
                })
            });
            let Some(border_origin) = border_origin else {
                continue;
            };
            //5673F8/56742A store translated XY in the Map local XYZ;
            //5678C0/C7 passes that local to567DA0, which applies its own
            //height translation. Z is unchanged, not doubled.
            Self::refresh_sight_border(
                &mut self.fog,
                &self.resolved_terrain,
                viewer,
                border_origin,
                geometry.radius.wrapping_add(3),
                context,
            );
        }
    }

    /// The frame's sight projection pass (`Simulation::refresh_fog`): it
    /// ends last frame's derived sight and marks the retained footprints
    /// again ([`vision::FogState::mark_untracked_sight`],
    /// [`vision::FogState::mark_cleared_cells`]). It reveals and releases
    /// nothing: a source's footprint opens and closes only at its native
    /// events, each through this owner (listed in [`crate::sim::vision`]).
    ///
    /// The one exception is VERA-only and fixture-only: a simulation built
    /// without a map descriptor sizes its fog on its first frame
    /// ([`vision::FogState::begin_sight_frame`]), after its objects' Unlimbo
    /// found no map to reveal on, and each placed source that holds sight
    /// then takes the admission its Unlimbo (`0x006F6D27`) would have made
    /// (`Self::admit_sight_on_sized_fog`). A launched scenario sizes its fog
    /// at construction and never gets there, so only test builds keep it.
    pub(crate) fn refresh_sight_sources(&mut self, config: &vision::VisionConfig, rules: &RuleSet) {
        let frame = self
            .fog
            .begin_sight_frame(&self.substrate.entities, self.path_grid.as_deref());
        #[cfg(any(test, feature = "test-support"))]
        if frame.sized {
            self.admit_sight_on_sized_fog(config, UninitContext::new(rules, None));
        }
        // Only that fixture admission reads the reveal settings here.
        #[cfg(not(any(test, feature = "test-support")))]
        let _ = (config, rules);
        self.fog.mark_untracked_sight(&frame.untracked);
        self.fog.mark_cleared_cells();
        #[cfg(any(test, feature = "test-support"))]
        {
            let cells = self
                .resolved_terrain
                .as_ref()
                .map(crate::map::resolved_terrain::NativeCellQuery::isolated);
            self.fog.assert_sight_projection_consistent(cells.as_ref());
        }
    }

    /// [`Self::refresh_sight_sources`]'s stand-in for the Unlimbo admissions a
    /// fog sized on the first frame missed. Fixtures alone build a simulation
    /// without a map descriptor (the parity-digest tool's synthetic run too,
    /// whose passive houses hold no sight); sizing their fog when they are
    /// built would need the bounds their objects give it.
    #[cfg(any(test, feature = "test-support"))]
    fn admit_sight_on_sized_fog(
        &mut self,
        config: &vision::VisionConfig,
        context: UninitContext<'_>,
    ) {
        // Each owner's viewers are asked once (`vision::RevealViewers`).
        self.reveal_viewers.begin();
        //IDs alone cross callbacks. Requery the live object at every turn.
        for id in self.substrate.entities.keys_sorted() {
            if self
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| vision::holds_sight(entity, config))
            {
                let heights = self.sight_height_grid(config);
                self.update_object_sight_in_view(id, 0, None, config, Some(&heights), context);
            }
        }
        self.reveal_viewers.end();
    }

    /// The settings every reveal reads (`TechnoClass::UpdateReveal @
    /// 0x0070AF50` and its callers, the Psychic Reveal): with a live map the
    /// stored playfield byte gates a Techno's, as a `MultiplayPassive=`
    /// house does, the map's `Size=` diamond bounds them, and the rules'
    /// sight keys and `AllyReveal=` shape them.
    pub(crate) fn sight_reveal_config(&self, rules: &RuleSet) -> vision::VisionConfig {
        let passive_owners = self
            .houses
            .iter()
            .filter(|(_, house)| house.multiplay_passive())
            .map(|(&owner, _)| owner)
            .collect();
        vision::VisionConfig::new(
            rules,
            self.map_size_diamond(),
            self.playfield_bounds.is_some(),
            passive_owners,
        )
    }

    /// `TechnoClass::ReReveal(0, 0, 0, 0)` (`0x0070B1D0`) then
    /// `UpdateReveal(0, 0, 0, 0, radius)` (`0x0070AF50`): the object's sight
    /// leaves the footprint its last reveal stored (`+0x254`, `+0x260`) and
    /// opens where it stands (a
    /// nonzero `radius` replaces its sight), for an object that holds sight
    /// ([`vision::holds_sight`]). Callers: the Unit and Infantry cell
    /// arrivals (`0x0073AC5F`/`0x0073AC74`, `0x0051A8D7`/`0x0051A8EB`), the
    /// Jumpjet descent (`0x0054C93C`/`0x0054C95B`), Fly's landing transition
    /// (`0x004CD43F`/`0x004CD454`) and the Spy Plane's snapshots
    /// (`0x00415672`/`0x00415697`, `0x004157F7`/`0x0041581C`).
    ///
    /// Carryall_Pickup's pair (`0x00416C6D`/`0x00416C81`) is dormant, as the
    /// pickup is: it needs an AircraftType with `Carryall=`, which no retail
    /// type sets (`retail_aircraft_types_are_not_carryalls`).
    pub(crate) fn refresh_object_sight(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        radius: i32,
        effects: FrameEffects<'_>,
    ) {
        let config = self.sight_reveal_config(rules);
        if !self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| vision::holds_sight(entity, &config))
        {
            return;
        }
        let context = UninitContext::new(rules, None).with_effects(effects);
        //The two functions each re-read their receiver after callbacks.
        self.release_object_sight_with_context(stable_id, context);
        self.update_object_sight_with_context(stable_id, radius, None, context);
    }

    /// Whether the ally brackets visit `entity` at all
    /// (`MapClass::ParanoidRevealAll @ 0x004ADEE0`,
    /// `MapClass::ParanoidUnrevealAll @ 0x004ADCD0`): both walk the Techno
    /// array and skip only a null slot and an object in limbo (`+0x81`,
    /// `0x004ADF08`, `0x004ADCFB`), with no IsAlive test. VERA's dying mark
    /// sits only on objects whose UnInit native has already run (its Limbo,
    /// or the SHP death sequence that stands for it, [`vision::holds_sight`]),
    /// so they are skipped too.
    fn in_ally_bracket_walk(entity: &crate::sim::game_entity::GameEntity) -> bool {
        !entity.lifecycle.in_limbo() && !entity.death.is_dying()
    }

    /// Whether the ally brackets' allied-Building arm takes `entity` on
    /// `viewer`'s map (`0x004ADEE0` at `0x004ADF7D..0x004ADFB9`, `0x004ADCD0`
    /// at `0x004ADDA6..0x004ADE1C`). The Human arm comes first and takes
    /// every object of a House `HouseClass::IsHumanPlayer @ 0x0050B6F0`
    /// admits (`0x004ADF34`, `0x004ADD27`; in a nonzero game mode, the map's
    /// player alone). The live walk uses that current-House predicate and41B;
    /// The
    /// allied-Building arm then takes a Building of
    /// another House when `AllyReveal=` is set (`Rules+0x17E7`) and that House
    /// counts the map's player an ally (`HouseClass::IsAlliedWith @
    /// 0x004F9A50`, its own mask): the house gate of `MapClass::RevealArea`
    /// ([`vision::reveal_admits`]).
    fn takes_allied_building_arm(
        &self,
        entity: &crate::sim::game_entity::GameEntity,
        viewer: crate::sim::intern::InternedId,
        config: &vision::VisionConfig,
    ) -> bool {
        Self::in_ally_bracket_walk(entity)
            && entity.category() == EntityCategory::Structure
            && entity.owner() != viewer
            && vision::reveal_admits(
                self.house_alliances.as_map(),
                entity.owner(),
                viewer,
                config,
                &self.interner,
            )
    }

    /// Map4ADEE0 before a live diplomacy write. The matching4ADCD0 walk
    /// must run even if MakeEnemy finds no ally bit to clear.
    pub(crate) fn release_alliance_sight(&mut self, context: UninitContext<'_>) {
        self.walk_map_sight(context, true, None, false);
    }

    /// Map4ADCD0 after diplomacy, including the Human41B arm's ordinary
    /// Sight admission and cached-radius border. No source state lives here:
    /// Techno sight and gap event owners retain their own caches/latches.
    pub(crate) fn admit_alliance_sight(&mut self, context: UninitContext<'_>) {
        self.walk_map_sight(context, false, None, false);
    }

    /// Fill_In_Data684F89 -> Map4ADCD0(1,0), before the first Logic frame.
    /// The native process stores its current player's map. VERA stores every
    /// House's map, so LAN/WOL runs this same owner for each human viewpoint
    /// in House order; the actual client's presentation identity stays local.
    /// Buildings are skipped at this caller. Source: original gamemd.exe.
    pub(crate) fn initialize_scenario_sight(&mut self, context: UninitContext<'_>) {
        let viewers = self.represented_player_maps();
        for viewer in viewers {
            self.walk_map_sight(context, false, Some(viewer), true);
        }
    }

    /// House4F9338..4F935F: only PlayerPtr consumes +24B, before entering
    /// Map4ADCD0(0,0). Keep the clear synchronous with the shared callbacks.
    /// Native constructor/caller controls: tools/spatial_oracle/house_initial_sight.
    pub(super) fn update_initial_house_sight(
        &mut self,
        owner: InternedId,
        context: UninitContext<'_>,
    ) {
        let represented_view = if self.session.game_mode().is_network() {
            self.houses
                .get(&owner)
                .is_some_and(|house| house.is_human())
        } else {
            self.session.current_house() == Some(owner)
        };
        if represented_view
            && self
                .houses
                .get_mut(&owner)
                .is_some_and(|house| house.take_initial_sight_refresh())
        {
            self.walk_map_sight(context, false, Some(owner), false);
        }
    }

    /// Original4ADEE0..4ADFEC /4ADCD0..4ADED5, arguments(0,0) from
    /// MakeAlly4F9B9A/4F9F4F and MakeEnemy4F9FD8/4FA1E3. Both visit in
    /// Techno registration order; each source is re-read after callbacks.
    /// House50B6F0 and41B select the actual current player's Human arm,
    /// including campaign controlled Houses. In LAN/WOL each represented
    /// human's own map takes the Human arm, independently of which client is
    /// local. An AI's own mobile never gains this human-only admission.
    ///
    /// Each logical client's native UpdateReveal/ReReveal state is tracked at
    /// `0x0070AF50`/`0x0070B1D0`; the House gate at `0x00567AB0..0x00567B12`
    /// independently decides whether that client's map receives the walk.
    /// Whole-map reveal/reshroud and live diplomacy share this visitor.
    fn walk_map_sight(
        &mut self,
        context: UninitContext<'_>,
        release: bool,
        only_viewer: Option<InternedId>,
        skip_buildings: bool,
    ) {
        let count = self.substrate.entities.techno_registry_len();
        let viewers =
            only_viewer.map_or_else(|| self.houses.order().to_vec(), |viewer| vec![viewer]);
        let clients = self.fog_clients_current_first();
        let config = self.sight_reveal_config(context.rules());
        // Map4ADEE0/4ADCD0 retain registration and viewer order. Scratch is
        // local to this invocation: callbacks may reenter the same walk.
        let mut callback_viewers = Vec::with_capacity(viewers.len());
        let mut followups = Vec::with_capacity(viewers.len());
        for ordinal in 0..count {
            let Some(id) = self.substrate.entities.techno_registry_at(ordinal) else {
                break;
            };
            let current = self.session.current_house();
            // Keep the branch chosen for THIS receiver across its callbacks
            // (4ADD4C..4ADDA1,4ADDE4..4ADE21). Reload the next receiver at
            // the next ordinal, not this object's eligibility mid-branch.
            enum Followup {
                SightAndGap,
                AlliedSight,
                RevealToAll,
                Gap,
            }
            callback_viewers.clear();
            followups.clear();
            for &viewer in &viewers {
                let Some(entity) = self.substrate.entities.get(id) else {
                    break;
                };
                if !Self::in_ally_bracket_walk(entity)
                    || (skip_buildings && entity.category() == EntityCategory::Structure)
                {
                    break;
                }
                let human = if self.session.game_mode().is_network() {
                    entity.owner() == viewer
                        && self
                            .houses
                            .get(&viewer)
                            .is_some_and(|house| house.is_human())
                } else {
                    Some(viewer) == current && self.house_is_human_player(entity.owner())
                };
                if human && !self.techno_discovered_by_viewer(id, viewer) {
                    continue;
                }
                let allied = !human && self.takes_allied_building_arm(entity, viewer, &config);
                let followup = if human || allied {
                    callback_viewers.push(viewer);
                    if allied {
                        Followup::AlliedSight
                    } else {
                        Followup::SightAndGap
                    }
                } else if !release
                    && clients.contains(&viewer)
                    && entity.category() == EntityCategory::Structure
                    && self
                        .object_type(entity.type_ref(), context.rules())
                        .is_some_and(|object| object.reveal_to_all)
                {
                    Followup::RevealToAll
                } else {
                    Followup::Gap
                };
                followups.push((viewer, followup));
            }
            // One UpdateReveal per native receiver, with each logical client's
            // source state installed before any RevealArea can reenter.
            if !callback_viewers.is_empty() {
                if release {
                    self.release_object_sight_event(
                        id,
                        &callback_viewers,
                        &config,
                        context,
                        current,
                    );
                } else {
                    let heights = self.sight_height_grid(&config);
                    self.update_object_sight_for_viewers(
                        id,
                        0,
                        &callback_viewers,
                        &config,
                        Some(&heights),
                        context,
                    );
                }
            }
            for (viewer, followup) in followups.drain(..) {
                match followup {
                    Followup::SightAndGap | Followup::AlliedSight => {
                        if !release {
                            self.refresh_object_sight_border_on(id, Some(viewer), context);
                        }
                        if matches!(followup, Followup::AlliedSight) {
                            //4ADFBF/4ADE21 skip the gap callbacks.
                            continue;
                        }
                    }
                    Followup::RevealToAll => {
                        //4ADE68 has no matching pre-release; each call counts
                        //again on this logical client's PlayerPtr map.
                        self.reveal_sight_for_house(id, viewer, context);
                        self.refresh_object_sight_border_on(id, Some(viewer), context);
                        continue;
                    }
                    Followup::Gap => {}
                }
                // Native rereads the TYPE after sight/border callbacks before
                // virtual418/414; shared gap ownership retains that decision.
                self.refresh_map_callback_gap(id, viewer, !release, context.rules());
            }
        }
    }

    /// The ground levels a reveal under `config` tests its cells' line of
    /// sight against (`Cell+0x11B`, read at `0x00567CB5`): none unless
    /// `RevealByHeight=`. A handle on the current path grid, which keeps the
    /// levels; taking it copies no cells.
    ///
    /// The map read places its objects, and their Unlimbo reveals
    /// (`0x006F6D27`), before `ScenarioClass::Post_Map_Init` publishes the
    /// path grid (`Simulation::finalize_scenario_post_map`). Native cells
    /// carry their levels from the map read on, so until then the walk reads
    /// each cell's level off the terrain, the levels the grid will publish
    /// ([`vision::CellLevels::Terrain`]). Reading none there (#1886) stored
    /// footprints that ignored the heights, which the first release after
    /// publication then walked with them, leaving the cells only the reveal
    /// reached counted and published in sight.
    pub(crate) fn sight_height_grid(&self, config: &vision::VisionConfig) -> SightHeights {
        if !config.reveal_by_height() {
            return SightHeights(SightLevels::Flat);
        }
        if let Some(grid) = self.path_grid_snapshot() {
            return SightHeights(SightLevels::Published(grid));
        }
        // The terrain of the fog's own map; a fixture's may differ.
        match self.resolved_terrain.as_ref() {
            Some(terrain)
                if (terrain.width(), terrain.height()) == (self.fog.width(), self.fog.height()) =>
            {
                SightHeights(SightLevels::Terrain)
            }
            _ => SightHeights(SightLevels::Flat),
        }
    }

    /// The gates `TechnoClass::UpdateReveal @ 0x0070AF50` and `ReReveal @
    /// 0x0070B1D0` share for an alternate house: the stored playfield byte
    /// `+0x3D5`, and an owner that is not `MultiplayPassive=`.
    fn passes_alternate_reveal_gates(
        entity: &crate::sim::game_entity::GameEntity,
        config: &vision::VisionConfig,
    ) -> bool {
        entity.in_playfield() && !config.passive_owner(entity.owner())
    }

    /// The footprint `UpdateReveal(0, 0, 1, PlayerPtr, 0)` reveals for
    /// `current` past those gates: the source's sight as it stands. A fresh
    /// zero effective radius still writes250/254/260, then walks no cells.
    fn current_house_sight_footprint(
        &self,
        entity: &crate::sim::game_entity::GameEntity,
        current: crate::sim::intern::InternedId,
        config: &vision::VisionConfig,
        height_grid: Option<&SightHeights>,
        rules: &RuleSet,
    ) -> Option<vision::SightAdmission> {
        if !Self::passes_alternate_reveal_gates(entity, config) {
            return None;
        }
        let ability = config.sight_ability(entity, &self.interner, rules);
        vision::entity_sight_admission(
            &self.fog,
            entity,
            current,
            config,
            height_grid.and_then(|heights| heights.cell_levels(self.resolved_terrain.as_ref())),
            self.resolved_terrain.as_ref(),
            ability,
            None,
        )
    }

    /// `TechnoClass::UpdateReveal(0, 0, 1, PlayerPtr, 0)` (`0x0070AF50`) for
    /// a source, as a computer house's
    /// `RevealToAll=` building opens (`0x00446816..0x00446829`). One logical
    /// client callback writes only that `PlayerPtr` map through
    /// `MapClass::RevealArea @ 0x005678E0` (`0x0070B17E..0x0070B1C2`), counted
    /// as that client's alternate source. Each call counts again;
    /// [`Self::rereveal_sight_to_current_house`] takes it back.
    ///
    /// RESIDUALS:
    /// - VERA represents the active human/current logical clients and their
    ///   own map planes; other client lifecycle is outside that active set.
    pub(crate) fn reveal_sight_to_current_house(
        &mut self,
        stable_id: u64,
        context: UninitContext<'_>,
        only_viewer: Option<InternedId>,
    ) {
        let viewers =
            only_viewer.map_or_else(|| self.fog_clients_current_first(), |viewer| vec![viewer]);
        for viewer in viewers {
            if self.session.game_mode().is_network()
                && self
                    .substrate
                    .entities
                    .get(stable_id)
                    .is_some_and(|entity| entity.owner() == viewer)
            {
                continue;
            }
            self.reveal_sight_for_house(stable_id, viewer, context);
        }
    }

    fn reveal_sight_for_house(
        &mut self,
        stable_id: u64,
        current: crate::sim::intern::InternedId,
        context: UninitContext<'_>,
    ) {
        let rules = context.rules();
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        let source_owner = entity.owner();
        let config = self.sight_reveal_config(rules);
        let height_grid = self.sight_height_grid(&config);
        let Some(footprint) =
            self.current_house_sight_footprint(entity, current, &config, Some(&height_grid), rules)
        else {
            return;
        };
        // UpdateReveal70B0B1..70B0E3: a clear250 installs the ordinary
        // cache BEFORE choosing explicit alternate House at70B0EC..70B12C.
        // MakeEnemy's allied pre-release can produce this state for a live
        // RevealToAll building. Metadata does not reveal the owner's map.
        if !self.fog.source_sight_is_latched(stable_id, current) {
            self.fog
                .retain_source_sight(stable_id, current, source_owner, &footprint);
        }
        if footprint.radius == 0 {
            return;
        }
        if !self.fog.holds_alternate_sight(stable_id, current) {
            self.fog
                .retain_sight_admission(stable_id, current, footprint.clone(), true);
        }
        self.walk_source_sight(
            current,
            &footprint,
            false,
            &config,
            Some(&height_grid),
            context,
            Some(current),
        );
    }

    /// `TechnoClass::ReReveal(0, 0, 1, PlayerPtr)` (`0x0070B1D0`), as
    /// DestructionEffects (`0x0044172F`) and Limbo's `RevealToAll=` branch
    /// (`0x006F6B06`) call it. Past its gates (the stored playfield byte
    /// `+0x3D5`, a `MultiplayPassive=` owner) it clears the source's latch
    /// (`0x0070B202`), so its ordinary sight stays counted
    /// ([`vision::FogState::clear_sight_latch`]), and releases the stored
    /// footprint for the current house whether the latch was set
    /// (`0x0070B20D..0x0070B21B`) or not (`0x0070B234..0x0070B24C`), both at
    /// `0x0070B24E..0x0070B26C` (the shared source event owner).
    ///
    /// So a computer house's building that dies leaves the current house's
    /// map twice, death and Limbo, and the sight an `AllyReveal=` ally's sold
    /// building shared stays counted there. With the latch already clear the
    /// footprint is the source's sight as it stands: native keeps the stored
    /// coordinate and radius (`+0x254`, `+0x260`), and a building has not
    /// moved.
    pub(crate) fn rereveal_sight_to_current_house(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        skip_owner: bool,
    ) {
        // Each active logical client has its own native PlayerPtr and
        // Techno +0x250/+0x254/+0x260 cache. Network clients retain stable
        // House order; offline preserves the session client first. Run the
        // same RevealToAll body for every maintained logical plane.
        let mut clients = self.fog_clients_current_first();
        for viewer in self.fog.alternate_sight_viewers(stable_id) {
            if !clients.contains(&viewer) {
                clients.push(viewer);
            }
        }
        for &client in &clients {
            if self.session.game_mode().is_network()
                && skip_owner
                && self
                    .substrate
                    .entities
                    .get(stable_id)
                    .is_some_and(|entity| entity.owner() == client)
            {
                continue;
            }
            self.rereveal_sight_for_house(stable_id, client, rules);
        }

        // Non-client source planes (notably a computer owner's own map) do
        // not acquire PlayerPtr's RevealToAll arm. Retire any such remaining
        // native source cache through the ordinary ReReveal owner and its
        // existing per-viewer House gate.
        let remaining: Vec<_> = self
            .fog
            .source_sight_viewers(stable_id)
            .into_iter()
            .filter(|viewer| !clients.contains(viewer))
            .collect();
        if !remaining.is_empty() {
            let config = self.sight_reveal_config(rules);
            self.release_object_sight_event(
                stable_id,
                &remaining,
                &config,
                UninitContext::with_rules(rules),
                self.session.current_house(),
            );
        }
    }

    fn rereveal_sight_for_house(
        &mut self,
        stable_id: u64,
        current: crate::sim::intern::InternedId,
        rules: &RuleSet,
    ) {
        let config = self.sight_reveal_config(rules);
        if !self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| Self::passes_alternate_reveal_gates(entity, &config))
        {
            return;
        }
        let footprint = match self.fog.clear_sight_latch(stable_id, current) {
            Some(footprint) => Some(footprint),
            None => {
                let height_grid = self.sight_height_grid(&config);
                let entity = self.substrate.entities.get(stable_id).unwrap();
                self.current_house_sight_footprint(
                    entity,
                    current,
                    &config,
                    Some(&height_grid),
                    rules,
                )
            }
        };
        let Some(footprint) = footprint.filter(|footprint| footprint.radius != 0) else {
            return;
        };
        self.fog.take_sight_admission(stable_id, current, true);
        let heights = self.sight_height_grid(&config);
        self.walk_source_sight(
            current,
            &footprint,
            true,
            &config,
            Some(&heights),
            UninitContext::with_rules(rules),
            Some(current),
        );
    }

    pub(super) fn refresh_high_flying_sight_before_process(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let frame = self.session.binary_frame();
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        if entity.death.is_dying()
            || entity.lifecycle.in_limbo()
            || !matches!(
                entity.category(),
                EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
            )
            || !crate::sim::movement::motion_query::is_moving_now(
                entity,
                crate::sim::movement::SpeedRules::new(rules, &self.interner, &self.houses),
                frame,
            )
        {
            return;
        }
        let high = crate::sim::movement::air_movement::is_high_flying(
            entity,
            self.resolved_terrain.as_ref(),
            Some((rules, &self.interner)),
        );
        if !high {
            return;
        }
        //4DA6B4 loads SOURCE House before4F9A50; reverse-only alliance
        //does not admit this event or mutate this viewer's countdown.
        let viewers = self.reveal_viewers.allied(
            &self.fog,
            self.house_alliances.as_map(),
            entity.owner(),
            &self.interner,
        );
        let due: Vec<_> = viewers
            .iter()
            .copied()
            .filter(|&viewer| {
                entity
                    .sight_refresh_timers
                    .timer(viewer)
                    .expired(frame as i32)
            })
            .collect();
        if due.is_empty() {
            return;
        }
        let config = self.sight_reveal_config(rules);
        //The two leaves reload their own receiver after synchronous discovery.
        let context = UninitContext::new(rules, None).with_effects(effects);
        self.release_object_sight_event(
            stable_id,
            &due,
            &config,
            context,
            self.session.current_house(),
        );
        let heights = self.sight_height_grid(&config);
        self.update_object_sight_for_viewers(stable_id, 0, &due, &config, Some(&heights), context);
        for viewer in due {
            let Some(entity) = self.substrate.entities.get(stable_id) else {
                break;
            };
            let terrain = context.terrain().or(self.resolved_terrain.as_ref());
            // Foot4DA710..4DA77B rereads height, current XYZ and cached260
            // after both sight callbacks, even if UpdateReveal refused sight.
            let height = crate::sim::movement::air_movement::current_fly_height(entity, terrain);
            let radius = self
                .fog
                .source_sight(stable_id, viewer)
                .map_or(0, |source| source.radius);
            let inner = if height == entity.sight_refresh_timers.last_height(viewer) {
                radius.wrapping_sub(3)
            } else {
                0
            };
            let origin = (
                entity.position.rx(),
                entity.position.ry(),
                crate::sim::movement::ground_pose::object_world_z_leptons(entity, terrain),
            );
            Self::refresh_sight_border_range(
                &mut self.fog,
                &self.resolved_terrain,
                viewer,
                origin,
                radius.wrapping_add(3),
                inner,
                context,
            );
            let Some(entity) = self.substrate.entities.get(stable_id) else {
                break;
            };
            //4DA784..4DA7AA samples relative height again after the border's
            //canonical cell queries, stores264, then reloads the countdown.
            let height = crate::sim::movement::air_movement::current_fly_height(
                entity,
                context.terrain().or(self.resolved_terrain.as_ref()),
            );
            let now = self.session.binary_frame();
            let clocks = &mut self
                .substrate
                .entities
                .get_mut(stable_id)
                .unwrap()
                .sight_refresh_timers;
            clocks.set_last_height(viewer, height);
            clocks.reload(viewer, now);
        }
    }
}

/// Walk ranks still to visit, taken in ascending order.
struct RankSet {
    words: Vec<u64>,
    /// No word before this one holds a rank.
    first: usize,
}

impl RankSet {
    fn new(len: u32) -> Self {
        Self {
            words: vec![0; (len as usize).div_ceil(64)],
            first: 0,
        }
    }

    fn insert(&mut self, rank: u32) {
        let word = rank as usize / 64;
        self.words[word] |= 1 << (rank % 64);
        self.first = self.first.min(word);
    }

    /// Every rank below `len`.
    fn fill(&mut self, len: u32) {
        self.words.fill(u64::MAX);
        if let Some(last) = self.words.last_mut()
            && len % 64 != 0
        {
            *last = (1 << (len % 64)) - 1;
        }
        self.first = 0;
    }

    /// The ranks a pass visits for `listed`, from `from` on: each listed cell
    /// and, for a refresh, the cells whose frames read it, and every cell
    /// beside the Dummy when its ground changed.
    fn add_listed(
        &mut self,
        order: &AllocatedCellOrder,
        listed: &vision::ListedEdgeCells,
        refresh: bool,
        from: u32,
    ) {
        let mut add = |cell: (i32, i32)| {
            if let Some(rank) = order.rank(cell).filter(|&rank| rank >= from) {
                self.insert(rank);
            }
        };
        for &(x, y) in &listed.cells {
            let (x, y) = (i32::from(x), i32::from(y));
            add((x, y));
            if refresh {
                for (dx, dy) in ROW_NEIGHBORS {
                    add((x + i32::from(dx), y + i32::from(dy)));
                }
            }
        }
        if refresh && listed.beside_dummy {
            for &(rank, _) in order.missing_neighbors_through(u32::MAX) {
                if rank >= from {
                    self.insert(rank);
                }
            }
        }
    }

    fn pop(&mut self) -> Option<u32> {
        while let Some(word) = self.words.get_mut(self.first) {
            if *word == 0 {
                self.first += 1;
                continue;
            }
            let bit = word.trailing_zeros();
            *word &= *word - 1;
            return Some(self.first as u32 * 64 + bit);
        }
        None
    }

    fn drain(&mut self) -> impl Iterator<Item = u32> + '_ {
        std::iter::from_fn(|| self.pop())
    }
}
