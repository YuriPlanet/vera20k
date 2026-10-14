//! FootAI4DA692..4DA7AA: admitted high-flying sight refresh before Process.
//! Per-viewer clocks preserve the original local-player admission boundary;
//! House/view-cache reconciliation never advances these clocks or emits events.
use super::{Simulation, UninitContext};
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::{pathfinding::PathGrid, vision};

#[cfg(test)]
#[path = "shroud_refresh_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "reveal_to_all_tests.rs"]
mod reveal_to_all_tests;

#[cfg(test)]
#[derive(Default)]
struct RevealCalls {
    walks: Vec<Vec<(u16, u16)>>,
    discovery_queries: std::collections::BTreeSet<(u16, u16)>,
}

#[cfg(test)]
thread_local! {
    static REVEAL_CALLS: std::cell::RefCell<Option<RevealCalls>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
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

impl Simulation {
    /// Original `TActionClass::RevealAroundWaypoint 0x006E0FE0`, called by
    /// Execute17 at6DE34E. This uses TAction+90 and PlayerPtr, independently
    /// of the trigger's House and the trailing+44 camera waypoint. The
    /// signed level includes Map5657A0's short-circuit bridge100/400 gate;
    /// the truncating half-level subtraction wraps as a signed WORD before
    /// RevealArea5678E0 applies its own Z shift. Executed controls/readers:
    /// tools/spatial_oracle/trigger_action_values.{py,json,meta.json}.
    /// Ledger: no RNG draws, timer writes or detach calls. A negative radius
    /// natively indexes before the count table; VERA maps no cells (retail9).
    pub(crate) fn trigger_reveal_at_waypoint(
        &mut self,
        index: i32,
        context: UninitContext<'_>,
    ) -> bool {
        let Some(rules) = context.rules() else {
            return false;
        };
        let Some(viewer) = self.session.current_house else {
            return false;
        };
        if self.fog.whole_map_revealed_for_owner(viewer) {
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
        let config = self.sight_reveal_config(Some(rules));
        let height_grid = self.sight_height_grid(&config);
        // Outline/unreveal/arg6 are zero and by-height is one. Native final0
        // followed by final1 leaves knowledge with no sustained sight source.
        for final_pass in [false, true] {
            self.reveal_area(
                &[viewer],
                center,
                z,
                radius,
                height_grid.as_deref(),
                final_pass,
                &config,
                context,
            );
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

        let Some(viewer) = self.session.current_house else {
            return false;
        };
        if self.fog.whole_map_revealed_for_owner(viewer) {
            return true;
        }
        let (Some(rules), Some(waypoint), Some(bounds)) = (
            context.rules(),
            self.session.waypoint_cell(index),
            self.playfield_bounds,
        ) else {
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
        let mut height_grid = self.sight_height_grid(&self.sight_reveal_config(Some(rules)));
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
                let Some(viewer) = self.session.current_house else {
                    return false;
                };
                let config = self.sight_reveal_config(Some(rules));
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
                self.reveal_area(
                    &[viewer],
                    center,
                    half_level * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS,
                    2,
                    height_grid.as_deref(),
                    final_pass,
                    &config,
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
        line_of_sight: Option<&[u8]>,
        final_pass: bool,
        config: &vision::VisionConfig,
        context: UninitContext<'_>,
    ) {
        let cells = config.collect_area_cells(
            cell,
            radius,
            z,
            line_of_sight,
            (self.fog.width, self.fog.height),
        );
        #[cfg(test)]
        REVEAL_CALLS.with_borrow_mut(|trace| {
            if let Some(trace) = trace {
                trace.walks.push(cells.clone());
            }
        });
        for &viewer in viewers {
            for &cell in &cells {
                let notify = {
                    let native_cells = context
                        .terrain()
                        .or(self.resolved_terrain.as_ref())
                        .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
                    self.fog
                        .reveal_map_cell(viewer, cell, final_pass, native_cells.as_ref())
                };
                if let Some(notify) = notify {
                    self.notify_cell_change(notify.cell, viewer, notify.fresh, context);
                }
            }
        }
    }

    /// Original CellChangeNotify5865F0..586709: seven height-admitted
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
                self.mark_radar_terrain_dirty_cells([candidate]);
            }
            #[cfg(test)]
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
    pub(crate) fn reveal_entire_map(
        &mut self,
        house: Option<crate::sim::intern::InternedId>,
        context: UninitContext<'_>,
    ) {
        if let Some(state) = house.and_then(|house| self.houses.get_mut(&house)) {
            state.map_is_clear = true;
        }
        let Some(player) = self.session.current_house else {
            return;
        };
        if house.is_some_and(|house| house != player) || !self.fog.admit_whole_map_reveal(player) {
            return;
        }
        self.reveal_allocated_map_cells(false, context);
    }

    /// Execute16 at6DE3A8: local House240 gate/set,578350/578290, then
    ///653810->Display4A9890 per allocated cell and final redraw. House241,
    /// sight/gap counters and sustained sources are untouched. Current
    /// PlayerPtr is reread for every receiver after synchronous callbacks.
    pub(crate) fn trigger_reveal_entire_map(&mut self, context: UninitContext<'_>) -> bool {
        let Some(player) = self.session.current_house else {
            return false;
        };
        if self.fog.whole_map_revealed_for_owner(player) {
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
        self.fog.admit_whole_map_reveal(player);
        self.reveal_allocated_map_cells(true, context);
        true
    }

    /// The simulation's one578350/578290 traversal host.16 and577F30 use
    ///different cell receivers; neither precollects cells before callbacks.
    ///Nested simulation reveals reset this same cursor, not an outer copy.
    fn reveal_allocated_map_cells(&mut self, forced: bool, context: UninitContext<'_>) {
        if self.reset_allocated_map_cell_cursor() {
            while let Some(cell) = self.next_allocated_map_cell(context) {
                self.reveal_allocated_map_cell(cell, forced, context);
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
            self.reveal_allocated_map_cell(cell, forced, context);
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
        context: UninitContext<'_>,
    ) {
        let Some(player) = self.session.current_house else {
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
    /// them as577AB0 does. Steam3e81a617..08600,577C90..577D89.
    pub(crate) fn trigger_reshroud_map(
        &mut self,
        incoming: Option<crate::sim::intern::InternedId>,
        rules: &RuleSet,
    ) {
        if let Some(owner) = incoming.and_then(|owner| self.houses.get_mut(&owner)) {
            owner.map_is_clear = false;
        }
        if incoming.is_some() && incoming != self.session.current_house {
            return;
        }
        if let Some(player) = self.session.current_house {
            self.transition_house_map(player, vision::MapTransition::Reshroud, rules);
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
            house.map_is_clear = !reset;
        }
        if self.fog.width != 0
            && self.fog.height != 0
            && (reset || !self.fog.whole_map_revealed_for_owner(owner))
        {
            //577AB0/577D90/577C90 initialize the same allocated cursor as16
            //and577F30.51's scalar Cell487690 leave receiver cannot dispatch
            //discovery; the other two only store cell fields. Their cell list
            //may be passed to the existing Fog release/store/put owner.
            //That owner's represented release/put callbacks do not
            //dispatch simulation Tags; full4ADEE0/4ADCD0 discovery remains
            //its existing residual, not a claim of reentrant equivalence.
            let cells = if self.reset_allocated_map_cell_cursor() {
                let mut cells = Vec::new();
                while let Some(cell) =
                    self.next_allocated_map_cell(UninitContext::with_rules(rules))
                {
                    cells.push(cell);
                }
                cells
            } else {
                self.resolved_terrain.as_ref().map_or_else(
                    || self.fog.rectangular_cells(),
                    |terrain| terrain.iter().map(|cell| (cell.rx, cell.ry)).collect(),
                )
            };
            //4ADEE0/4ADCD0 do not release allied mobile sight. The represented
            //multiplayer known-own branch and stock AllyReveal=yes directional
            //allied Building arm select callbacks; untouched admissions survive
            //the bulk counter overwrite, and the RevealToAll branch reveals the
            //rest's alternate sight. Independent41B discovery and campaign
            //Human remain caller residuals.
            let sources: std::collections::BTreeSet<u64> = self
                .substrate
                .entities
                .iter_sorted()
                .filter_map(|(id, entity)| {
                    if !entity.lifecycle.object_alive() || entity.lifecycle.in_limbo() {
                        return None;
                    }
                    let own = entity.owner() == owner;
                    let allied_building = entity.category() == EntityCategory::Structure
                        && crate::map::houses::is_allied_with(
                            &self.house_alliances,
                            self.interner.resolve(entity.owner()),
                            self.interner.resolve(owner),
                        );
                    (own || allied_building).then_some(id)
                })
                .collect();
            let old_spy_sat_active = self
                .houses
                .get(&owner)
                .is_some_and(|house| house.spy_sat_active);
            let (gaps, admitted_gap) = self.prepare_spy_sat_gap_reentry(owner, rules);
            // RESIDUAL: the other callers of `0x004ADCD0` are not ported.
            // MakeAlly (`0x004F9F4F`) and MakeEnemy (`0x004FA1E3`) bracket it
            // with `0x004ADEE0`, which has no `RevealToAll=` branch; VERA's
            // (`sim::house_diplomacy`) run without the bracket, a residual
            // recorded there. The first House update (`0x004F935A`) runs on the
            // first frame; stock skirmish maps place no such building, which
            // bounds those samples rather than proving campaign absence.
            // Fill_In_Data's call (`0x00684F89`) skips buildings. ALL01's live
            // SpySat_Boom061FF03C uses action51/577C90, so its represented
            // release/store/reentry path runs here; occupied discovery and
            // nested Tags in the original bracket remain the stated residual.
            let mut alternates = self.reveal_to_all_footprints(owner, rules);
            alternates.retain(|id, _| !sources.contains(id));
            self.fog.transition_whole_map_with_gap_reentry(
                owner,
                cells,
                transition,
                old_spy_sat_active,
                &sources,
                Some(&gaps),
                &alternates,
            );
            //577D90 sets240 before callbacks; every successful6FB170
            //re-admission clears it again, including a friendly generator.
            if admitted_gap {
                self.fog.invalidate_whole_map_reveal_for_owner(owner);
            }
        }
    }

    /// The settings every reveal reads (`TechnoClass::UpdateReveal @
    /// 0x0070AF50` and its callers, the Psychic Reveal): with a live map the
    /// stored playfield byte gates a Techno's, the map's `Size=` diamond
    /// bounds them, and the rules' sight keys and `AllyReveal=` shape them.
    pub(crate) fn sight_reveal_config(&self, rules: Option<&RuleSet>) -> vision::VisionConfig {
        vision::VisionConfig::new(
            rules,
            self.map_size_diamond(),
            self.playfield_bounds.is_some(),
        )
    }

    /// The ground levels a reveal under `config` tests its cells' line of
    /// sight against (`0x00567CB5`): none unless `RevealByHeight=`, or
    /// without a path grid.
    pub(crate) fn sight_height_grid(&self, config: &vision::VisionConfig) -> Option<Vec<u8>> {
        config
            .reveal_by_height()
            .then(|| self.path_grid().map(PathGrid::ground_height_grid))
            .flatten()
    }

    /// The gates `TechnoClass::UpdateReveal @ 0x0070AF50` and `ReReveal @
    /// 0x0070B1D0` share for an alternate house: the stored playfield byte
    /// `+0x3D5`, and an owner that is not `MultiplayPassive=`.
    fn passes_alternate_reveal_gates(&self, entity: &crate::sim::game_entity::GameEntity) -> bool {
        entity.in_playfield
            && !self
                .houses
                .get(&entity.owner())
                .is_some_and(|house| house.multiplay_passive)
    }

    /// The footprint `UpdateReveal(0, 0, 1, PlayerPtr, 0)` reveals for
    /// `current` past those gates: the source's sight as it stands, none for
    /// a zero radius ([`vision::entity_sight_footprint`]).
    fn current_house_sight_footprint(
        &self,
        entity: &crate::sim::game_entity::GameEntity,
        current: crate::sim::intern::InternedId,
        config: &vision::VisionConfig,
        height_grid: Option<&[u8]>,
        rules: &RuleSet,
    ) -> Option<vision::SightAdmission> {
        if !self.passes_alternate_reveal_gates(entity) {
            return None;
        }
        let ability = vision::entity_has_sight_ability(entity, &self.interner, Some(rules));
        vision::entity_sight_footprint(
            &self.fog,
            entity,
            config,
            height_grid,
            self.resolved_terrain.as_ref(),
            ability,
            current,
        )
    }

    /// `TechnoClass::UpdateReveal(0, 0, 1, PlayerPtr, 0)` (`0x0070AF50`) for
    /// a source that already holds its own sight, as a computer house's
    /// `RevealToAll=` building opens (`0x00446816..0x00446829`): past its
    /// gates, the footprint goes to `MapClass::RevealArea @ 0x005678E0` for
    /// the current house rather than the owner (`0x0070B17E..0x0070B1C2`). It
    /// writes the maps a reveal by the current house writes, counted as its
    /// own sight ([`vision::FogState::admit_alternate_sight`]). Each call
    /// counts again; [`Self::rereveal_sight_to_current_house`] takes it back.
    ///
    /// RESIDUALS:
    /// - In a network game each client passes its own `PlayerPtr`, so every
    ///   human player but the owner gets the sight; VERA gives it to the
    ///   session's current house, as `house_is_human_player` reads it.
    ///   Skirmish has one human.
    /// - A source without its own admission yet (`+0x250` clear) would
    ///   natively store this footprint as its own and leave its owner
    ///   without sight (`0x0070B0BF..0x0070B131`); VERA's owner sight comes
    ///   from the per-frame reconciliation regardless. No caller reaches it:
    ///   the opening follows the source's Unlimbo reveal (`0x006F6D27`,
    ///   ahead of the opening at `0x00440E76`), and the re-admission
    ///   callback's branch needs a placed building in the playfield.
    pub(crate) fn reveal_sight_to_current_house(&mut self, stable_id: u64, rules: &RuleSet) {
        let Some(current) = self.session.current_house else {
            return;
        };
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        let config = self.sight_reveal_config(Some(rules));
        let height_grid = self.sight_height_grid(&config);
        let Some(footprint) = self.current_house_sight_footprint(
            entity,
            current,
            &config,
            height_grid.as_deref(),
            rules,
        ) else {
            return;
        };
        for viewer in vision::direct_reveal_viewers(&self.fog, current, &config, &self.interner) {
            self.fog
                .admit_alternate_sight(stable_id, viewer, footprint.clone());
        }
    }

    /// `TechnoClass::ReReveal(0, 0, 1, PlayerPtr)` (`0x0070B1D0`), as
    /// DestructionEffects (`0x0044172F`) and Limbo's `RevealToAll=` branch
    /// (`0x006F6B06`) call it. Past its gates (the stored playfield byte
    /// `+0x3D5`, a `MultiplayPassive=` owner) it clears the source's latch
    /// (`0x0070B202`), so its ordinary sight stays counted
    /// ([`vision::FogState::clear_sight_latch`]), and releases the stored
    /// footprint for the current house whether the latch was set
    /// (`0x0070B20D..0x0070B21B`) or not (`0x0070B234..0x0070B24C`), both at
    /// `0x0070B24E..0x0070B26C` ([`vision::FogState::withdraw_alternate_sight`]).
    ///
    /// So a computer house's building that dies leaves the current house's
    /// map twice, death and Limbo, and the sight an `AllyReveal=` ally's sold
    /// building shared stays counted there. With the latch already clear the
    /// footprint is the source's sight as it stands: native keeps the stored
    /// coordinate and radius (`+0x254`, `+0x260`), and a building has not
    /// moved.
    pub(crate) fn rereveal_sight_to_current_house(&mut self, stable_id: u64, rules: &RuleSet) {
        let Some(current) = self.session.current_house else {
            return;
        };
        if !self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| self.passes_alternate_reveal_gates(entity))
        {
            return;
        }
        let config = self.sight_reveal_config(Some(rules));
        let footprint = match self.fog.clear_sight_latch(stable_id) {
            Some(footprint) => Some(footprint),
            None => {
                let height_grid = self.sight_height_grid(&config);
                let entity = self.substrate.entities.get(stable_id).unwrap();
                self.current_house_sight_footprint(
                    entity,
                    current,
                    &config,
                    height_grid.as_deref(),
                    rules,
                )
            }
        };
        let Some(footprint) = footprint.filter(|footprint| footprint.radius != 0) else {
            return;
        };
        let viewers = vision::direct_reveal_viewers(&self.fog, current, &config, &self.interner);
        self.fog
            .withdraw_alternate_sight(stable_id, &footprint, &viewers);
    }

    /// The sources `0x004ADCD0`'s `RevealToAll=` branch (`0x004ADE26..
    /// 0x004ADE68`) reveals again on `owner`'s map, keyed by registration
    /// order: each live building of such a type whose owner no human
    /// controls, which the allied-building branch (`0x004ADDB2..0x004ADDE4`:
    /// `AllyReveal=` and allied with the current house) does not take, built
    /// or still building up. Each calls `UpdateReveal(0, 0, 1, PlayerPtr, 0)`
    /// (`0x0070B17E..0x0070B1C2`, past its gates); only the maps a reveal by
    /// the current house writes receive it, as at the opening
    /// ([`Self::reveal_sight_to_current_house`]).
    ///
    /// RESIDUALS, each dormant in retail:
    /// - VERA's allied-building branch (the transition's `sources`) ignores
    ///   `AllyReveal=`, so under a map's `AllyReveal=no` an ally's building
    ///   is re-admitted as an allied source and skipped here; native skips
    ///   that branch (Rules `+0x17E7`, `0x004ADDB2`) and reveals it here.
    ///   Retail sets `AllyReveal=yes`.
    /// - Native's branch jumps past the gap re-admission (`0x004ADEA5`) that
    ///   `0x004ADEE0` undid (`0x004ADFC1..0x004ADFD9`); VERA's transition
    ///   re-admits the gaps of every generator. No retail type is both a gap
    ///   generator and `RevealToAll=`.
    /// - A retained alternate admission keeps its source count through a
    ///   transition that re-admits the building through the own or allied
    ///   branch instead; native's counters keep only that re-admission. If
    ///   the building's ordinary sight then left the map while it stood,
    ///   VERA would keep the cells in sight. It needs an `AllyReveal=`
    ///   alliance between a computer house and the current house that ends
    ///   mid-game.
    fn reveal_to_all_footprints(
        &mut self,
        owner: crate::sim::intern::InternedId,
        rules: &RuleSet,
    ) -> std::collections::BTreeMap<u64, vision::SightAdmission> {
        let mut footprints = std::collections::BTreeMap::new();
        let Some(current) = self.session.current_house else {
            return footprints;
        };
        self.fog.alliances = self.house_alliances.clone();
        let config = self.sight_reveal_config(Some(rules));
        if !vision::direct_reveal_viewers(&self.fog, current, &config, &self.interner)
            .contains(&owner)
        {
            return footprints;
        }
        let height_grid = self.sight_height_grid(&config);
        for (id, entity) in self.substrate.entities.iter_sorted() {
            if entity.category() != EntityCategory::Structure
                || !entity.lifecycle.object_alive()
                || entity.lifecycle.in_limbo()
                || self.house_is_human_player(entity.owner())
                || !self
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| object.reveal_to_all)
                || vision::direct_reveal_viewers(&self.fog, entity.owner(), &config, &self.interner)
                    .contains(&current)
            {
                continue;
            }
            if let Some(footprint) = self.current_house_sight_footprint(
                entity,
                current,
                &config,
                height_grid.as_deref(),
                rules,
            ) {
                footprints.insert(id, footprint);
            }
        }
        footprints
    }

    pub(super) fn refresh_high_flying_sight_before_process(
        &mut self,
        stable_id: u64,
        rules: Option<&RuleSet>,
    ) {
        let frame = self.session.binary_frame;
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
                rules.map(|rules| {
                    crate::sim::movement::SpeedRules::new(
                        rules,
                        &self.interner,
                        &self.type_handles,
                        &self.houses,
                    )
                }),
                frame,
            )
        {
            return;
        }
        let high = crate::sim::movement::air_movement::is_high_flying(
            entity,
            self.resolved_terrain.as_ref(),
            rules.map(|rules| (rules, &self.interner)),
        );
        if !high {
            return;
        }
        self.fog.alliances = self.house_alliances.clone();
        //4DA6B4 loads SOURCE House before4F9A50; reverse-only alliance
        //does not admit this event or mutate this viewer's countdown.
        let viewers = vision::allied_viewers(&self.fog, entity.owner(), &self.interner);
        let due: Vec<_> = viewers
            .into_iter()
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
        let height_grid = self.sight_height_grid(&config);
        let ability = vision::entity_has_sight_ability(entity, &self.interner, rules);
        for &viewer in &due {
            // The two reveal leaves may reject membership/zero-radius. The
            // due FootAI branch nevertheless reloads15 after its calls.
            if entity.in_playfield && !entity.passenger_role.is_inside_transport() {
                vision::refresh_entity_vision_for_viewer(
                    &mut self.fog,
                    entity,
                    &config,
                    height_grid.as_deref(),
                    self.resolved_terrain.as_ref(),
                    ability,
                    &self.interner,
                    viewer,
                );
            }
        }
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            for viewer in due {
                entity.sight_refresh_timers.reload(viewer, frame);
            }
        }
    }
}
