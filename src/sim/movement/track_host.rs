//! Drive4B0F20/Ship6A0980 world execution for every retained track. The retained cursor belongs
//! to the locomotor; TrackProcess owns just this call's paid budget and raw
//! descriptor. Every world receiver ends the entity borrow before continuing.
//!
//! Native boundaries and executable scalar witnesses are documented by
//! track_process.rs. PerCell currently admits MCV, ordered crush, sensor and
//! playfield receivers. Discovery/tag4, crates and the refinery radio branch
//! remain explicit receiver gaps; this host does not replay object AI for them.

use super::ground_pose::position_world_coord;
use super::locomotor::MovementLayer;
use super::track_process::{TrackFamily, TrackInvocation, TrackPayment, TrackProcess};
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{DriveCoord, TrackProgress};
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::{FrameEffects, Simulation};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TrackWorldEvent {
    MarkRemove,
    SetCoords,
    MarkPut,
    PerCell,
    Arrival,
}

/// One Process_Track call: its paid placements and the native AL. AL is
/// true only from the terminal tail (Drive 0x4B2283 / 0x4B22A1): after the
/// terminal PerCell a null, dead, limbo or falling owner (0x4B2218), a true
/// Enter_Idle_Mode (0x4B2273) or a true vt+504 (0x4B2291). The outer Process
/// then returns at once (0x4B057D / 0x69FC8D).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TrackPass {
    pub moved: u32,
    pub aborted: bool,
}

impl TrackPass {
    fn paid(moved: u32) -> Self {
        Self {
            moved,
            aborted: false,
        }
    }

    fn aborted(moved: u32) -> Self {
        Self {
            moved,
            aborted: true,
        }
    }
}

/// The Drive/Ship locomotor's track selector and cursor (`+0x58`/`+0x5C`).
fn progress(entity: &GameEntity, family: TrackFamily) -> Option<TrackProgress> {
    entity.locomotor.as_ref()?.track_progress(family)
}

/// The Drive/Ship locomotor's HeadToCoord (`+0x40`), null when absent.
fn head(entity: &GameEntity, family: TrackFamily) -> DriveCoord {
    entity
        .locomotor
        .as_ref()
        .and_then(|loco| loco.track_head(family))
        .unwrap_or(DriveCoord { x: 0, y: 0, z: 0 })
}

fn head_or_null(entity: Option<&GameEntity>, family: TrackFamily) -> DriveCoord {
    entity
        .map(|entity| head(entity, family))
        .unwrap_or(DriveCoord { x: 0, y: 0, z: 0 })
}

/// Stores the Drive/Ship locomotor's HeadToCoord (`+0x40`).
fn set_head(entity: &mut GameEntity, family: TrackFamily, value: Option<DriveCoord>) {
    if let Some(loco) = entity.locomotor.as_mut() {
        loco.store_track_head(family, value);
    }
}

/// Stores the Drive/Ship locomotor's `+0x63`, which Process_Drive_Track's
/// admission reads (Drive `0x004B0F2C`, Ship `0x006A05FC`).
fn set_track_valid(entity: &mut GameEntity, family: TrackFamily, value: bool) {
    if let Some(loco) = entity.locomotor.as_mut() {
        loco.store_track_valid(family, value);
    }
}

fn cell(coord: DriveCoord) -> (u16, u16) {
    ((coord.x / 256) as u16, (coord.y / 256) as u16)
}

impl Simulation {
    /// The active locomotor's Force_Track, on the ILoco interface (+4
    /// receiver): Drive `0x004B0C40` and its twin Ship `0x006A0310`, whose
    /// bodies differ only in their null coordinate and their occupation
    /// callee (`0x004B0AD0`, `0x006A01A0`). Every other class has the base's
    /// empty body (`0x0055AC10`). Selector/cursor publication precedes the
    /// null-coordinate return. Head, destination, residual and owner speed
    /// have independent lifetimes.
    pub(crate) fn force_track(
        &mut self,
        id: u64,
        selector: i32,
        supplied: DriveCoord,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        self.force_track_observed(id, selector, supplied, &mut |sim, id, coord| {
            sim.pickup_crate_at(
                id,
                super::foot_path::coord_cell(coord),
                rules,
                registry,
                effects,
            )
        })
    }

    /// `CellClass::PickupCrate @ 0x00481A00` for the Foot `id` committing to
    /// `cell`, as every movement host calls it; its AL. Without rules or the
    /// OverlayType table the cell answers as one without a crate, native's
    /// `0x00481A39` return: a Rust availability gate with no native
    /// counterpart. The frame's movement hosts bind both. RESIDUAL: two
    /// `Force_Track` callers hold no table and skip the pickup at `0x4B0D1B`:
    /// the parasite's grapple onto its victim's cell (`parasite_attach`) and
    /// the bunker's sell/destroy release onto the bunker's exit cell
    /// (`bunker_link::release_sell_destroy`). Trigger: a crate on exactly that
    /// cell. Effect: the crate waits for the next mover. Frequency: rare (a
    /// crate under a grappled victim or beside a dying bunker).
    pub(crate) fn pickup_crate_at(
        &mut self,
        id: u64,
        cell: (i16, i16),
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        match (rules, registry) {
            (Some(rules), Some(registry)) => {
                crate::sim::crates::pickup_crate(self, rules, registry, cell, id, effects)
            }
            _ => true,
        }
    }

    fn force_track_observed(
        &mut self,
        id: u64,
        selector: i32,
        supplied: DriveCoord,
        receive: &mut impl FnMut(&mut Simulation, u64, DriveCoord) -> bool,
    ) -> bool {
        use crate::rules::locomotor_type::LocomotorKind;
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        let Some(family) = entity
            .locomotor
            .as_ref()
            .and_then(|loco| match loco.active_kind() {
                LocomotorKind::Drive => Some(TrackFamily::Drive),
                LocomotorKind::Ship => Some(TrackFamily::Ship),
                _ => None,
            })
        else {
            return false;
        };
        let loco = entity.locomotor.as_mut().unwrap();
        loco.ensure_installed_track_state();
        let mut progress = loco.track_progress(family).unwrap();
        progress.select_forced(selector);
        loco.store_track_progress(family, progress);
        if supplied == (DriveCoord { x: 0, y: 0, z: 0 }) {
            return false;
        }
        loco.store_track_head(family, Some(supplied));
        loco.store_track_valid(family, true);
        //4B0D14/4B0D1B (Ship 6A03E4/6A03EB): address the supplied cell, then
        // the synchronous crate pickup; a false answer (4B0D22) drops the head
        // like a limbo owner. Witness tests supply the answer through the observer.
        let received = receive(self, id, supplied);
        let survives = self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| received && !entity.lifecycle.in_limbo());
        if !survives {
            if let Some(entity) = self.substrate.entities.get_mut(id)
                && entity.lifecycle.object_alive()
                && let Some(loco) = entity.locomotor.as_mut()
            {
                loco.store_track_head(family, None);
                loco.store_track_valid(family, false);
            }
            return false;
        }
        self.track_apply_occupation_at(id, family, supplied, true);
        let Some(loco) = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|entity| entity.locomotor.as_mut())
            .filter(|loco| loco.has_track_state(family))
        else {
            return false;
        };
        loco.store_track_destination(family, Some(supplied));
        loco.store_track_target_fraction(family, crate::util::fixed_math::SIM_ONE);
        true
    }

    pub(crate) fn run_track_process(
        &mut self,
        invocation: TrackInvocation,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<TrackPass, String> {
        let object = self
            .substrate
            .entities
            .get(invocation.entity_id)
            .and_then(|e| self.interner.object_type(e.type_ref(), rules?));
        let Some(entity) = self.substrate.entities.get_mut(invocation.entity_id) else {
            return Ok(TrackPass::default());
        };
        if !super::track_turn::admit_track_entry(entity, object.is_some_and(|o| o.has_turret)) {
            return Ok(TrackPass::default());
        }
        // One native entry owns admission, scalar prefix and paid loop, in that
        // order. No scalar speed update crosses the world receiver handoff.
        // The prefix also runs for `retry`, whose budget masks its speed.
        let current_grid = self.path_grid.as_deref();
        let fresh_budget = super::track_speed::advance(
            entity,
            object,
            rules,
            &self.houses,
            self.resolved_terrain.as_ref(),
            current_grid,
        );
        // Drive4B1297 / Ship6A095F: a tube word with no selected track enters
        // the tube instead of the paid loop.
        if self.track_tube_head(
            invocation.entity_id,
            invocation.family,
            rules,
            registry,
            effects,
        )? {
            return Ok(TrackPass::default());
        }
        self.try_run_track_points_observed(
            invocation,
            fresh_budget,
            rules,
            registry,
            &mut |_, _, _| {},
            effects,
        )
    }

    /// Geometry/receiver fixtures explicitly supply the paid budget; they do
    /// not certify the native entry or scalar prefix. Production enters above.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn run_track_points(
        &mut self,
        invocation: TrackInvocation,
        fresh_budget: i32,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
    ) -> u32 {
        self.run_track_points_observed(invocation, fresh_budget, rules, registry, &mut |_, _, _| {})
    }

    pub(super) fn track_survives(&self, id: u64) -> bool {
        self.substrate.entities.get(id).is_some_and(|entity| {
            entity.lifecycle.object_alive()
                && !entity.lifecycle.in_limbo()
                && !entity.is_falling_down()
        })
    }

    /// The track (`+0x58`/`+0x5C`), HeadToCoord (`+0x40`) and owner Location (`+0x9C`)
    /// that Process_Drive_Track (Drive `0x004B0F20`, Ship `0x006A05F0`) reads.
    fn track_state(
        &self,
        id: u64,
        family: TrackFamily,
    ) -> Option<(TrackProgress, DriveCoord, DriveCoord)> {
        let entity = self.substrate.entities.get(id)?;
        Some((
            progress(entity, family)?,
            head(entity, family),
            position_world_coord(&entity.position),
        ))
    }

    /// Shared paid-loop body. The observer tests synchronous receiver effects;
    /// the caller has already completed admission and the scalar prefix.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn run_track_points_observed(
        &mut self,
        invocation: TrackInvocation,
        fresh_budget: i32,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        observe: &mut impl FnMut(&mut Simulation, u64, TrackWorldEvent),
    ) -> u32 {
        self.try_run_track_points_observed(
            invocation,
            fresh_budget,
            rules,
            registry,
            observe,
            FrameEffects::default(),
        )
        .expect("track fixture must provide every coordinate receiver")
        .moved
    }

    fn try_run_track_points_observed(
        &mut self,
        invocation: TrackInvocation,
        fresh_budget: i32,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        observe: &mut impl FnMut(&mut Simulation, u64, TrackWorldEvent),
        effects: FrameEffects<'_>,
    ) -> Result<TrackPass, String> {
        let TrackInvocation {
            entity_id: id,
            family,
            retry,
            ..
        } = invocation;
        let Some((state, _, _)) = self.track_state(id, family) else {
            return Ok(TrackPass::default());
        };
        let mut call = TrackProcess::begin(family, &state, fresh_budget, retry);
        let mut moved = 0u32;
        let candidate_direction = self.substrate.entities.get(id).and_then(|entity| {
            entity
                .navigation
                .path_replay()
                .remaining_directions()
                .first()
                .copied()
        });
        let mut chain_allowed = candidate_direction
            .zip(call.chain_target_facing())
            .is_some_and(|(direction, facing)| {
                direction < 8 && crate::util::direction::direction_from_facing(facing) != direction
            });
        loop {
            let Some((state, stored_head, current)) = self.track_state(id, family) else {
                return Ok(TrackPass::paid(moved));
            };
            let Some(payment) = call.pay_current(&state) else {
                return Ok(TrackPass::paid(moved));
            };
            let TrackPayment::Sample(sample) = payment else {
                break;
            };
            if sample.terminal {
                call.adjust_terminal_budget(current, stored_head);
                // Drive4B1FEF/Ship6A1632 restore the Foot occupation enable
                // before terminal coordinates/Mark, then retire the selector.
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    entity.foot_occupation_enabled.resume();
                    entity.navigation.path_runtime_mut().clear_blockage();
                }
                self.track_place(
                    id,
                    stored_head,
                    cell(current),
                    true,
                    false,
                    rules,
                    registry,
                    observe,
                    effects,
                );
                moved = moved.saturating_add(1);
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    set_head(entity, family, None);
                    // Drive4B2104 / Ship6A1747 clear the active class's +63
                    // before selector retirement and the terminal PerCell.
                    set_track_valid(entity, family, false);
                    if let Some(mut state) = progress(entity, family) {
                        state.clear_selector();
                        entity
                            .locomotor
                            .as_mut()
                            .unwrap()
                            .store_track_progress(family, state);
                    }
                }
                // Native clears Head_To and selector before target+4C, then
                // queries the owner's physical cell and fresh +4C height.
                let reached = self.track_reached_destination(id, family, rules)?;
                if let Some(entity) = self.substrate.entities.get_mut(id)
                    && reached
                    && let Some(loco) = entity.locomotor.as_mut()
                {
                    loco.store_track_destination(family, None);
                }
                self.unit_per_cell_process(
                    id,
                    super::per_cell::PerCellReason::Arrival,
                    rules,
                    registry,
                    effects,
                );
                observe(self, id, TrackWorldEvent::PerCell);
                if !self.track_survives(id) {
                    return Ok(TrackPass::aborted(moved));
                }
                if reached {
                    let entity = self.substrate.entities.get_mut(id).unwrap();
                    entity.navigation.stop_moving();
                    entity.navigation.path_replay_mut().clear_live_head();
                    entity.navigation.settle_destination_setter();
                    if entity.mission.current().known() == Some(MissionType::Move) {
                        let returns = self.unit_enter_idle_mode(id, rules, false, effects);
                        observe(self, id, TrackWorldEvent::Arrival);
                        if returns {
                            return Ok(TrackPass::aborted(moved));
                        }
                    }
                }
                if self.track_navigation_gate(id, rules, effects) {
                    return Ok(TrackPass::aborted(moved));
                }
                if !self
                    .substrate
                    .entities
                    .get(id)
                    .is_some_and(|e| e.lifecycle.object_alive())
                {
                    return Ok(TrackPass::paid(moved));
                }
                // +504 false/alive admits the residual tail directly. It must
                // never restart the paid loop with a newly selected curve.
                break;
            }

            if self.track_occupation_enabled(id) {
                Self::track_raw_mark(
                    &mut self.substrate,
                    &self.path_grid,
                    &self.resolved_terrain,
                    id,
                    false,
                );
                // The first paid point drops the enable (Drive `0x004B161A`,
                // Ship `0x006A0CDA`).
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    entity.foot_occupation_enabled.suspend();
                    entity.navigation.path_runtime_mut().clear_blockage();
                }
            }
            let Some((live, live_head, actual)) = self.track_state(id, family) else {
                return Ok(TrackPass::paid(moved));
            };
            let previous = if live.cursor() == 0 {
                cell(actual)
            } else {
                call.previous_sample(live.cursor())
                    .and_then(|point| point.transform(family, &live, live_head))
                    .map(|(xy, _)| {
                        cell(DriveCoord {
                            x: xy[0],
                            y: xy[1],
                            z: actual.z,
                        })
                    })
                    .unwrap_or_else(|| cell(actual))
            };
            // The point XY was paid earlier; facing independently reloads the
            // live cursor BEFORE placement, then survives Mark callbacks.
            let Some((xy, _)) = sample.transform(family, &live, live_head) else {
                return Ok(TrackPass::paid(moved));
            };
            let paid_facing = call
                .live_facing_sample(live.cursor())
                .and_then(|point| point.transform(family, &live, live_head))
                .map(|(_, facing)| facing);
            self.track_place(
                id,
                DriveCoord {
                    x: xy[0],
                    y: xy[1],
                    z: if family == TrackFamily::Ship {
                        0
                    } else {
                        actual.z
                    },
                },
                previous,
                false,
                family == TrackFamily::Drive,
                rules,
                registry,
                observe,
                effects,
            );
            moved = moved.saturating_add(1);
            if !self
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.lifecycle.object_alive())
            {
                return Ok(TrackPass::paid(moved));
            }
            // A live object's SetHeight(0) runs with Marked cleared (Drive
            // `0x004B1A8B`/`0x004B1AA8`, Ship `0x006A10CD`/`0x006A10E7`).
            let marked = self.substrate.replace_foot_marked_byte(id, false);
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                let on_bridge = entity.on_bridge();
                entity.position.set_height(
                    on_bridge,
                    0,
                    self.resolved_terrain.as_ref(),
                    self.path_grid.as_deref(),
                );
            }
            self.substrate.replace_foot_marked_byte(id, marked);
            if let Some(facing) = paid_facing
                && let Some(entity) = self.substrate.entities.get_mut(id)
            {
                entity
                    .body_facing
                    .snap(u16::from(facing) << 8, self.session.binary_frame());
            }
            let Some((live, _, _)) = self.track_state(id, family) else {
                return Ok(TrackPass::paid(moved));
            };
            if call.is_at_occupation_handoff(&live) {
                Self::track_raw_mark(
                    &mut self.substrate,
                    &self.path_grid,
                    &self.resolved_terrain,
                    id,
                    false,
                );
            }
            let Some((live, _, _)) = self.track_state(id, family) else {
                return Ok(TrackPass::paid(moved));
            };
            if chain_allowed
                && call.is_at_chain_cursor(&live)
                && self.track_try_chain(
                    id,
                    family,
                    &mut call,
                    candidate_direction.unwrap(),
                    rules,
                    registry,
                    observe,
                    effects,
                )?
            {
                chain_allowed = false;
                if !self.track_survives(id) {
                    return Ok(TrackPass::paid(moved));
                }
            }
            let Some(entity) = self.substrate.entities.get_mut(id) else {
                return Ok(TrackPass::paid(moved));
            };
            let Some(mut state) = progress(entity, family) else {
                return Ok(TrackPass::paid(moved));
            };
            call.finish_surviving_point(&mut state);
            entity
                .locomotor
                .as_mut()
                .unwrap()
                .store_track_progress(family, state);
        }
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return Ok(TrackPass::paid(moved));
        };
        let Some(mut state) = progress(entity, family) else {
            return Ok(TrackPass::paid(moved));
        };
        call.store_residual(&mut state);
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .store_track_progress(family, state);
        let Some((live, stored_head, current)) = self.track_state(id, family) else {
            return Ok(TrackPass::paid(moved));
        };
        if let Some(step) = live.residual_step(family, current, stored_head) {
            let identity = |coord: DriveCoord| {
                self.resolved_terrain.as_ref().map(|terrain| {
                    terrain.native_cell_identity(((coord.x / 256) as i16, (coord.y / 256) as i16))
                })
            };
            let matches = |left, right| {
                if self.resolved_terrain.is_some() {
                    identity(left) == identity(right)
                } else {
                    cell(left) == cell(right)
                }
            };
            let chosen = step.choose(
                matches(step.interpolated, step.current),
                matches(step.interpolated, step.full),
                live.residual(),
            );
            self.track_place(
                id,
                chosen,
                cell(current),
                false,
                false,
                rules,
                registry,
                observe,
                effects,
            );
        }
        Ok(TrackPass::paid(moved))
    }

    fn track_occupation_enabled(&self, id: u64) -> bool {
        self.substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.foot_occupation_enabled())
    }

    /// Unit7441B0/744210 select the raw plane from exact live XYZ; REMOVE
    /// deliberately does not require a surviving structural bridge flag.
    pub(super) fn track_raw_mark(
        substrate: &mut ObjectSubstrate,
        path_grid: &Option<Arc<PathGrid>>,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        id: u64,
        put: bool,
    ) {
        let Some(entity) = substrate.entities.get(id) else {
            return;
        };
        let coord = position_world_coord(&entity.position);
        Self::track_raw_mark_at(substrate, path_grid, resolved_terrain, coord, put);
    }

    pub(super) fn track_raw_mark_at(
        substrate: &mut ObjectSubstrate,
        path_grid: &Option<Arc<PathGrid>>,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        coord: DriveCoord,
        put: bool,
    ) {
        use crate::sim::occupancy::VEHICLE_OCCUPATION_BIT;
        let (at, layer) = super::foot_mark::raw_occupation_plane(
            coord,
            put,
            resolved_terrain.as_ref(),
            path_grid.as_deref(),
        );
        let raw = &mut substrate.raw_cell_occupation;
        match (put, layer) {
            (true, MovementLayer::Bridge) => raw.mark_deck(at.0, at.1, VEHICLE_OCCUPATION_BIT),
            (true, _) => raw.mark_ground(at.0, at.1, VEHICLE_OCCUPATION_BIT),
            (false, MovementLayer::Bridge) => raw.clear_deck(at.0, at.1, VEHICLE_OCCUPATION_BIT),
            (false, _) => raw.clear_ground(at.0, at.1, VEHICLE_OCCUPATION_BIT),
        }
    }

    fn track_place(
        &mut self,
        id: u64,
        coord: DriveCoord,
        previous_track_cell: (u16, u16),
        terminal: bool,
        paid_drive_point: bool,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        observe: &mut impl FnMut(&mut Simulation, u64, TrackWorldEvent),
        effects: FrameEffects<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let old = (entity.position.rx(), entity.position.ry());
        let selected_cell = cell(coord);
        let crossing = old != selected_cell;
        if crossing {
            self.foot_mark_remove(id, rules, registry, effects);
            observe(self, id, TrackWorldEvent::MarkRemove);
        }
        // A same-cell SetLocation runs with Marked cleared: the paid point
        // (Drive `0x004B1A5D`/`0x004B1A74`, Ship `0x006A109F`/`0x006A10B6`),
        // the residual step (`0x004B25C9`/`0x004B25E3`, Ship
        // `0x006A1C1A`/`0x006A1C34`) and the terminal point with its
        // SetHeight(0) (`0x004B20A2`/`0x004B20CA`, Ship
        // `0x006A16E5`/`0x006A170D`).
        let saved_marked = (!crossing).then(|| self.substrate.replace_foot_marked_byte(id, false));
        super::ground_pose::foot_set_location(
            &mut self.substrate.entities,
            id,
            coord,
            rules,
            &self.interner,
        );
        observe(self, id, TrackWorldEvent::SetCoords);
        // Drive4B17DC SetLocation precedes the selected Cell lookup4B1800.
        // Keep that identity across Mark(DOWN), which may stamp the shared
        // Dummy again. Only a crossing paid Drive point reaches4B1999;
        // terminal/residual placements and same-cell points skip this arm.
        let pre_entry_cell = if crossing && paid_drive_point {
            self.resolved_terrain.as_ref().map(|terrain| {
                crate::map::resolved_terrain::NativeCellQuery::canonical(terrain)
                    .lookup((selected_cell.0 as i16, selected_cell.1 as i16))
            })
        } else {
            None
        };
        let grid = self.path_grid.as_deref();
        if crossing && let Some(entity) = self.substrate.entities.get_mut(id) {
            if !terminal {
                // Crossing uses actual current cell; this predicate instead
                // uses the previous transformed cached point (or current at
                // cursor0). The resolver also stores the entered cell's level.
                let on_bridge = entity.on_bridge();
                let update = super::movement_bridge::resolve_cell_transition_bridge_state(
                    &mut entity.position,
                    grid,
                    previous_track_cell,
                    selected_cell,
                    on_bridge,
                );
                super::movement_bridge::apply_bridge_layer_state(
                    &mut entity.locomotor,
                    &mut entity.on_bridge,
                    update,
                );
            } else if let Some(entered) =
                grid.and_then(|grid| grid.cell(selected_cell.0, selected_cell.1))
            {
                // The terminal point skips the bridge predicate but still
                // enters a new cell.
                entity
                    .position
                    .store_cell_level(entered.ground_level, entity.on_bridge());
            }
        }
        if terminal && let Some(entity) = self.substrate.entities.get_mut(id) {
            let on_bridge = entity.on_bridge();
            entity.position.set_height(
                on_bridge,
                0,
                self.resolved_terrain.as_ref(),
                self.path_grid.as_deref(),
            );
        }
        if let Some(marked) = saved_marked {
            self.substrate.replace_foot_marked_byte(id, marked);
        }
        if crossing {
            self.foot_mark_put_observed(
                id,
                rules,
                registry,
                &mut |sim, id| observe(sim, id, TrackWorldEvent::MarkPut),
                effects,
            );
        }
        if let Some(target) = pre_entry_cell {
            // Drive4B1999..4B1A58, after Mark(DOWN)4B1993 and before
            // SetHeight/track continuation. Type+D2B is TiltsWhenCrushes;
            // target+44/+2A8 is the live overlay/Wall, not a navigation head.
            let wall = self
                .cell_overlay(target)
                .overlay_id
                .and_then(|overlay| registry?.flags(overlay))
                .is_some_and(|flags| flags.wall);
            let Some(entity) = self.substrate.entities.get(id) else {
                return;
            };
            if entity
                .locomotor
                .as_ref()
                .and_then(|loco| loco.drive_straight())
                != Some(true)
            {
                return;
            }
            let Some(object) =
                rules.and_then(|rules| self.interner.object_type(entity.type_ref(), rules))
            else {
                return;
            };
            //4B1A0F GetUnit(false) selects Ground and does not exclude self
            //or filter health. Mark(DOWN) has already returned. This host
            //models active gameplay (native GameActive=true); the separate
            //loader/teardown query gate is not represented here.
            //Original histories: unit_per_cell_overlay.drive_crush.json.
            let slowdown = object.movement_zone
                == crate::rules::locomotor_type::MovementZone::CrusherAll
                && match target {
                    crate::map::cell_index::NativeCellIdentity::Real(index) => {
                        let cell = &self.resolved_terrain.as_ref().unwrap().cells()[index];
                        self.substrate
                            .occupancy
                            .first_category_on_layer(
                                cell.rx,
                                cell.ry,
                                MovementLayer::Ground,
                                EntityCategory::Unit,
                                &self.substrate.entities,
                            )
                            .is_some()
                    }
                    crate::map::cell_index::NativeCellIdentity::Dummy => false,
                };
            let tilt = object.tilts_when_crushes
                && (slowdown
                    || (wall
                        && (object.crusher
                            || crate::sim::combat::veterancy::has_weapon_ability(
                                entity.veterancy.rank_kind(),
                                object,
                                crate::rules::object_type::Ability::Crusher,
                            ))));
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                // A failed gate retains the flag; there is no per-frame clear.
                if slowdown {
                    entity.foot_speed.arm_crush_slowdown();
                }
                if tilt {
                    entity
                        .rocking
                        .get_or_insert_with(Default::default)
                        .set_vel_forwards_native(
                            crate::util::native_x87::NativeF32Bits::from_bits(0xbd4c_cccd),
                        );
                }
            }
        }
    }

    /// Apply_Track_Occupation_Mode(0/1), Drive4B0AD0 / Ship6A01A0:
    /// direct raw handoff then full supplied head, with no Foot+6B6 gate.
    pub(super) fn track_apply_occupation(&mut self, id: u64, family: TrackFamily, put: bool) {
        let supplied = head_or_null(self.substrate.entities.get(id), family);
        self.track_apply_occupation_at(id, family, supplied, put);
    }

    fn track_apply_occupation_at(
        &mut self,
        id: u64,
        family: TrackFamily,
        supplied: DriveCoord,
        put: bool,
    ) {
        let Some((state, retained_head, current)) = self.track_state(id, family) else {
            return;
        };
        if supplied == (DriveCoord { x: 0, y: 0, z: 0 }) {
            return;
        }
        let handoff = (!state.reversed() && state.turn_index() != -1)
            .then(|| {
                let index = usize::try_from(state.turn_index()).ok()?;
                if family == TrackFamily::Ship && index >= 64 {
                    return None;
                }
                let turn = super::drive_track::turn_track_at(index)?;
                if turn.normal_track == 0 {
                    return None;
                }
                let raw = super::drive_track::raw_track_meta(turn.normal_track)?;
                if raw.occupation_handoff_point_index < 0
                    || state.cursor() >= i32::from(raw.occupation_handoff_point_index)
                {
                    return None;
                }
                let point = super::drive_track::raw_track_points(turn.normal_track)
                    .get(raw.occupation_handoff_point_index as usize)?;
                let (x, y, _) = super::drive_track::transform_track_point(
                    point.x,
                    point.y,
                    point.facing,
                    turn.flags,
                );
                Some(DriveCoord {
                    // Transform4B47E2/E5 reloads class head after the crate
                    // receiver; Apply's final mark still uses supplied XYZ.
                    x: retained_head.x.wrapping_add(i32::from(x)),
                    y: retained_head.y.wrapping_add(i32::from(y)),
                    z: current.z,
                })
            })
            .flatten();
        if let Some(coord) = handoff {
            Self::track_raw_mark_at(
                &mut self.substrate,
                &self.path_grid,
                &self.resolved_terrain,
                coord,
                put,
            );
        }
        Self::track_raw_mark_at(
            &mut self.substrate,
            &self.path_grid,
            &self.resolved_terrain,
            supplied,
            put,
        );
    }

    fn track_try_chain(
        &mut self,
        id: u64,
        family: TrackFamily,
        call: &mut TrackProcess,
        direction: u8,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        observe: &mut impl FnMut(&mut Simulation, u64, TrackWorldEvent),
        effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        let Some(entity) = self.substrate.entities.get(id) else {
            return Ok(false);
        };
        let Some(from) = call.chain_target_facing() else {
            return Ok(false);
        };
        if direction >= 8 || crate::util::direction::direction_from_facing(from) == direction {
            return Ok(false);
        }
        let Some(selection) = super::drive_track::select_drive_track(from, direction * 32, false)
        else {
            return Ok(false);
        };
        if selection.entry_index == 0 {
            return Ok(false);
        }
        // A component fixture without rules or map cells cannot ask the
        // native predicate; production always has both. It takes no chain.
        let (Some(rules), Some(terrain)) = (rules, self.resolved_terrain.as_ref()) else {
            return Ok(false);
        };
        let candidate = super::track_head::offset_head(head(entity, family), direction);
        let saved_speed = entity.foot_speed.applied_fraction();
        //4B1BA1..4B1C3E: Unit+1AC(cell(head + delta), dir, Object 0x5F5F00,
        //0, 1), with no Mark bracket.
        let target = super::foot_path::coord_cell(candidate);
        let (height, native) = {
            let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
            let height = super::ground_pose::query_object_cell_height(
                &cells,
                position_world_coord(&entity.position),
                entity.on_bridge(),
            );
            (height, cells.lookup(target))
        };
        let code = self.foot_can_enter(
            id,
            native,
            super::infantry_entry::InfantryEntryArgs {
                direction: i32::from(direction),
                height,
                previous_cell: None,
            },
            rules,
            registry,
        )?;
        //4B1C44..4B1C4D: the jump table 0x4B2608 over codes 0..6.
        match code {
            0 | 2 => {}
            //4B1E52..4B1E68: ground-list+FC contact callbacks, no chain.
            1 => {
                self.uncloak_contacts_at_cell(target, rules)?;
                return Ok(false);
            }
            //4B1E6D..4B1EBB: the gate question, answer discarded.
            3 => {
                let _ = crate::sim::gate_runtime::request_gate_open_for_cell(
                    self,
                    (target.0 as u16, target.1 as u16),
                    id,
                    rules,
                );
                return Ok(false);
            }
            //4B1EC0..4B1F43: the forced deck-aware Scatter_Objects on the cell.
            6 => {
                self.scatter_blocked_track_cell(id, target, rules, registry, effects)?;
                return Ok(false);
            }
            _ => return Ok(false),
        }
        // Actual Unit+2C dispatch746E20 returns1; the next test reads
        // UnitType Passive+E0C. Stock absent Passive defaults false.
        if !self
            .substrate
            .entities
            .get(id)
            .and_then(|entity| self.interner.object_type(entity.type_ref(), rules))
            .is_some_and(|object| object.passive)
        {
            return Ok(false);
        }
        let entity = self.substrate.entities.get_mut(id).unwrap();
        let Some(mut state) = progress(entity, family) else {
            return Ok(false);
        };
        let accepted = call.accept_chain(&mut state, selection.turn_track_index as i32);
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .store_track_progress(family, state);
        if !accepted {
            return Ok(false);
        }
        set_head(entity, family, None);
        // Drive4B1CF5 / Ship6A1338 publish +63 for the PerCell receiver.
        set_track_valid(entity, family, true);
        self.unit_per_cell_process(
            id,
            super::per_cell::PerCellReason::Arrival,
            Some(rules),
            registry,
            effects,
        );
        observe(self, id, TrackWorldEvent::PerCell);
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            // Drive4B1D06 / Ship6A1349 clear it before the survival ladder.
            set_track_valid(entity, family, false);
        }
        if !self.track_survives(id) {
            return Ok(true);
        }
        let entity = self.substrate.entities.get_mut(id).unwrap();
        // Callback writes to the head are cleared before candidate install.
        set_head(entity, family, None);
        // Drive4B1DA5 / Ship6A13E8 restore +63 before candidate head stores,
        // after the transient PerCell corridor.
        set_track_valid(entity, family, true);
        set_head(entity, family, Some(candidate));
        // Drive4B1DBE / Ship6A1401: the crate question on the new head. A
        // true answer outside limbo (4B1DC3..4B1DD2) applies Apply1, the saved
        // owner fraction and the live queue shift; otherwise a live owner
        // drops the head to the dummy and clears +63 (4B1E1F..4B1E4D).
        let picked = self.pickup_crate_at(
            id,
            super::foot_path::coord_cell(candidate),
            Some(rules),
            registry,
            effects,
        );
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return Ok(true);
        };
        if !(picked && !entity.lifecycle.in_limbo()) {
            if entity.lifecycle.object_alive() {
                set_head(entity, family, None);
                set_track_valid(entity, family, false);
            }
            return Ok(true);
        }
        self.track_apply_occupation(id, family, true);
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.foot_speed.set_speed_fraction(saved_speed);
            entity.navigation.path_replay_mut().consume(1);
        }
        Ok(true)
    }

    fn track_reached_destination(
        &self,
        id: u64,
        family: TrackFamily,
        rules: Option<&RuleSet>,
    ) -> Result<bool, String> {
        let Some(entity) = self.substrate.entities.get(id) else {
            return Ok(false);
        };
        let Some(target) = entity.navigation.nav_com() else {
            return Ok(false);
        };
        let coord = super::navcom::nav_target_coordinate(
            target,
            Some(id),
            self,
            self.resolved_terrain.as_ref(),
            rules.map(|rules| (rules, &self.interner)),
        )?;
        let destination = entity
            .locomotor
            .as_ref()
            .and_then(|loco| loco.track_destination(family));
        let Some(destination) = destination else {
            return Ok(false);
        };
        // Drive4B2180..2194 / Ship6A17C3..17D7 query OWNER +4C for Z.
        // The cleared head makes ordinary +4C resolve to the placed owner;
        // NavCom contributes only the horizontal cell test.
        if cell(coord) != cell(position_world_coord(&entity.position)) {
            return Ok(false);
        }
        let owner = self.foot_navigation_coordinate(id)?;
        Ok(owner.z.wrapping_sub(destination.z).wrapping_abs()
            < 2 * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS)
    }

    /// Shared Foot EnterIdle4D82B0 base, reused by class receivers.
    /// Executed native latch/END/NavQueue controls: foot_enter_idle.json.
    /// Actual-GI Archive consumer: basic-factory-output-prerequisites packet.
    /// The class tail runs even when this base returns false for the latch.
    /// Scatter+687, Formation+520 and non-cell NavQueue remain named
    /// sibling residuals; fresh factory E1 has their constructor-empty state.
    pub(crate) fn foot_enter_idle_base(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        if entity.mission_leaf.foot_idle_entry_latch() != 0 {
            return false;
        }
        entity.mission_leaf.set_foot_idle_entry_latch(1);
        // Foot4D82D9 -> Techno709A54 lets a held Temporal target go first,
        // then asks RefreshMegaMission (vt+0x4D0, Techno709A5D) whether an
        // attack-move resumes. Its false answer asks the active PlanMgr
        // path (Techno+0x430=705D50 ->6385C0).
        self.temporal_release_if_warping(id);
        if !self.foot_refresh_mega_mission(id, rules, registry, effects) {
            self.planning_enter_idle(id);
        }
        // Foot4D831A..8376 asks the active IPiggyback, whatever its class,
        // and ends it before NavQueue when its Is_Ok_To_End allows.
        // Unit saves the base return but still executes its own receiver tail.
        let ended = self.try_end_piggyback(id, rules);
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        // Foot4D8382..83E2 takes NavQueue even with an existing NavCom.
        // Preserve its saved return independently from the resulting NavCom.
        // Non-Cell targets and other class END remain bounded base-receiver
        // gaps; no whole mission tick substitutes for those calls.
        let queued_cell = entity
            .navigation
            .nav_queue()
            .first()
            .copied()
            .and_then(|next| {
                if let crate::sim::components::NavTargetRef::Cell { rx, ry } = next {
                    Some((rx, ry))
                } else {
                    None
                }
            });
        // 4D838E..83CA: vt+0x480(queue[0], 0) through the one dispatcher,
        // which keeps its own NavCom fallback for a receiver no class setter
        // represents, then the queue shift.
        if let Some((rx, ry)) = queued_cell {
            let target = crate::sim::components::NavTargetRef::cell(rx, ry);
            if let Err(cause) =
                self.assign_destination_keeping_queue(id, Some(target), rules, registry, effects)
            {
                log::debug!("Foot {id} NavQueue destination: {cause:?}");
            }
            if let Some(entity) = self.substrate.entities.get_mut(id)
                && !entity.navigation.nav_queue().is_empty()
            {
                entity.navigation.pop_nav_queue_front();
            }
        }
        // A consumed NavQueue or ended piggyback returns true before
        // the Infantry Archive arm and final zero-speed setter.
        if ended || queued_cell.is_some() {
            return true;
        }
        // Foot4D8472..852A: Archive belongs to Techno, and +2DC is
        // SlaveOwner, not Team (+5D4). The same-cell comparison calls
        // virtual+48 on both owners and truncates signed leptons /256.
        let archived = self.substrate.entities.get(id).and_then(|entity| {
            if entity.category() != EntityCategory::Infantry || entity.slave.owner().is_some() {
                return None;
            }
            let target = entity.archive_target()?;
            let here =
                super::ground_pose::object_get_coords(entity, self.resolved_terrain.as_ref());
            let there = super::ground_pose::target_get_coords(
                target,
                self,
                self.resolved_terrain
                    .as_ref()
                    .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                    .as_ref(),
            )?;
            let cell = |coord: DriveCoord| ((coord.x / 256) as i16, (coord.y / 256) as i16);
            (cell(here) != cell(there)).then_some((
                target,
                entity.mission.current().known() == Some(MissionType::AreaGuard),
            ))
        });
        if let (Some((target, area_guard)), Some(rules)) = (archived, rules) {
            if !area_guard {
                let _ = self.mission_queue_exact(
                    id,
                    MissionId::from_known(MissionType::Move),
                    0,
                    self.session.binary_frame(),
                    &crate::sim::mission::authority::EntityReadyInputProvider,
                );
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    entity.set_archive_target(None);
                }
            }
            let destination = crate::sim::components::NavTargetRef::from(target);
            if let Err(cause) =
                self.assign_infantry_destination(id, destination, rules, registry, effects)
            {
                log::debug!("Infantry {id} idle Archive destination: {cause}");
            }
        }
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity
                .foot_speed
                .set_speed_fraction(crate::util::fixed_math::SIM_ZERO);
        }
        false
    }

    /// Unit EnterIdle738970(first_arg,1), shared by mission exits, initial
    /// placement, and locomotor/radio callers. The first argument skips the
    /// human harvester land check when nonzero. Native executable controls:
    /// tools/spatial_oracle/harvest_attack_return.json, track_destination.json
    /// and foot_enter_idle.json. The Foot Formation path remains a separate
    /// residual.
    pub(crate) fn unit_enter_idle_mode(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        skip_human_land_check: bool,
        effects: FrameEffects<'_>,
    ) -> bool {
        let saved_base_return = self.foot_enter_idle_base(id, rules, None, effects);
        let Some(entity) = self.substrate.entities.get(id) else {
            return saved_base_return;
        };
        //738990..7389AB: a resumed attack-move (HaveMegaMission, vt+0x4AC)
        // or Wait28 suppresses the Unit tail after Foot.
        if entity.mega_mission.is_attack_move()
            || entity.mission.effective().known() == Some(MissionType::Deliberate)
        {
            return saved_base_return;
        }
        // 0x007389B1..0x007389BF: a Magnetron lets go of what it holds.
        if entity.lift.target().is_some() {
            self.release_locomotor(id, true, rules, None, effects);
        }
        // 0x007389C4..0x00738A27: a Unit on Move (vt+0x184) with no NavCom,
        // standing exactly on its Cell's (vt+0x1B8) centre at height 0,
        // breaks its radio link (OVER_OUT to contact 0, vt+0x274(3)).
        if let Some(entity) = self.substrate.entities.get(id)
            && entity.mission.effective().known() == Some(MissionType::Move)
            && entity.navigation.nav_com().is_none()
        {
            let location = position_world_coord(&entity.position);
            let centre = |lepton: i32| i32::from((lepton / 256) as i16) * 256 + 128;
            if location.x == centre(location.x)
                && location.y == centre(location.y)
                && location.z == 0
            {
                crate::sim::radio::transmit_to_contact(
                    self,
                    id,
                    crate::sim::radio::RadioMessage::Break,
                    rules,
                    effects,
                );
            }
        }
        // `0x00738A2F`: the navigation list may send it on.
        self.handle_navigation_list(id, rules, None, effects);
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return saved_base_return;
        };
        let has_destination = entity.navigation.nav_com().is_some();
        let current = entity.mission.current().known();
        let effective = entity.mission.effective().known();
        let category = entity.category();
        let unit_type = rules.and_then(|rules| self.interner.object_type(entity.type_ref(), rules));
        let miner = unit_type.is_some_and(|object| object.harvester || object.weeder);
        // Unit738A6E calls the existing IsArmed701120 owner only after the
        // Harvester/Weeder gates. Armed idle keeps TarCom and burst; the
        // unarmed branch clears them before the final mission queue gate.
        // Original executable controls: harvest_attack_return.json plain_idle_*.
        let armed = !miner
            && unit_type
                .is_some_and(|object| crate::sim::combat::combat_weapon::is_armed(entity, object));
        let deploys_into = unit_type
            .and_then(|object| object.deploys_into.as_deref())
            .and_then(|name| rules.and_then(|rules| rules.object(name)))
            .is_some();
        // 0x00738A7C..0x00738AAF: an unarmed mission-only Unit (`+0x3D4`) of
        // a `Passengers=` type (`+0x5E0`) carrying a first passenger
        // (`+0x118`), outside a team (`+0x5D4`), selects Unload, skipping the
        // gates and setters below.
        let unloads = !has_destination
            && !miner
            && !armed
            && entity.is_mission_only()
            && unit_type.is_some_and(|object| object.passengers > 0)
            && entity
                .passenger_role
                .cargo()
                .is_some_and(|cargo| !cargo.is_empty())
            && self.team_script_vm.team_for_member(id).is_none();
        if !has_destination
            && !miner
            && !armed
            && !unloads
            && (entity.mcv_deploy_pending.is_pending()
                || current == Some(MissionType::AreaGuard)
                || (current == Some(MissionType::Unload) && deploys_into))
        {
            // Unarmed738AB4..738AE4 returns before either setter for pending
            // deployment, AreaGuard, or Unload with resolved DeploysInto.
            return saved_base_return;
        }
        let selection = if has_destination {
            Some(MissionType::Move)
        } else if unloads {
            Some(MissionType::Unload)
        } else if miner {
            rules.and_then(|rules| harvester_idle_selection(self, id, rules, skip_human_land_check))
        } else if armed {
            rules.map_or(Some(MissionType::Guard), |rules| {
                crate::sim::world::foot_enter_idle_mode_selection(
                    rules,
                    category,
                    current,
                    false,
                    false,
                    effective,
                    unit_idle_guard_pick(self, id, rules),
                )
                .queued_mission()
            })
        } else {
            // Unarmed738AEA..738B09 selects Guard and performs its setters
            // even on Patrol, Guard or a frozen mission. The armed early
            // selector gates do not apply to this branch.
            Some(MissionType::Guard)
        };
        if selection.is_some() && !has_destination && !unloads && (miner || !armed) {
            // Unit738AF5/738C75 calls Target(NULL) then Destination(NULL,1)
            // before its queue gate. Use the shared concrete setter owners,
            // including retained burst, movement, and Teleporter side effects.
            let _ = self.assign_target_represented(id, None, rules, None, effects);
            self.assign_null_destination(id, rules, None, effects);
        }
        // 0x00738C9D..0x00738CF8: a Unit on Enter (`+0xAC`) whose radio
        // contact 0 is a `Bunker=` building (`+0x16AB`) other than the first
        // ground Building of its own Cell (vt+0x1BC, `0x0047C520`), so one it
        // is still approaching, queues nothing.
        let approaching_bunker = self.substrate.entities.get(id).is_some_and(|actor| {
            let position = position_world_coord(&actor.position);
            let cell = (
                (position.x / 256) as i16 as u16,
                (position.y / 256) as i16 as u16,
            );
            actor.mission.current().raw() == 7
                && actor.radio_contacts.slot(0).is_some_and(|contact| {
                    self.substrate
                        .entities
                        .get(contact)
                        .is_some_and(|building| {
                            building.category() == EntityCategory::Structure
                                && rules
                                    .and_then(|rules| self.object_type(building.type_ref(), rules))
                                    .is_some_and(|object| object.bunker)
                        })
                        && self.substrate.occupancy.first_building_on_layer(
                            cell.0,
                            cell.1,
                            MovementLayer::Ground,
                        ) != Some(contact)
                })
        });
        // Unit738CFA..D12 suppresses assignment only after preceding writes.
        let selection = selection.filter(|_| {
            !approaching_bunker
                && !matches!(
                    self.substrate
                        .entities
                        .get(id)
                        .and_then(|actor| actor.mission.current().known()),
                    Some(
                        MissionType::Patrol
                            | MissionType::AreaGuard
                            | MissionType::Unload
                            | MissionType::Eaten
                    )
                )
        });
        if let Some(selection) = selection {
            let _ = self.mission_queue_exact(
                id,
                MissionId::from_known(selection),
                0,
                self.session.binary_frame(),
                &crate::sim::mission::authority::EntityReadyInputProvider,
            );
        }
        saved_base_return
    }

    fn track_navigation_gate(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        effects: FrameEffects<'_>,
    ) -> bool {
        // Foot vt+504 ->4DB9B0, called after terminal PerCell and
        // StopMoving4DF0D0. Stock Enter7 + Building contact + UnitRepair
        // calls Unit738970(0,0) at4DBA15; other admitted paths use(0,1).
        // The second argument only admits the retained tube-resume arm
        // 4D8403..8440. Ordinary ctor4D31F1 sets TubeIndex=-1, so both
        // arguments reach the same Unit tail here. Tube continuation remains
        // outside this receiver. Original arrival_terminal controls in
        // tools/spatial_oracle/building_repair.depot_service.json pin the
        // stock armed Enter -> queued Guard result and unchanged full RNG.
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if entity.navigation.nav_com().is_some() || entity.attack_target.target().is_some() {
            return false;
        }
        self.unit_enter_idle_mode(id, rules, false, effects)
    }
}

#[cfg(test)]
#[path = "track_host_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "track_force_tests.rs"]
mod force_tests;

/// `UnitClass::Enter_Idle_Mode`'s armed guard pick (`0x00738B67..
/// 0x00738C98`), reached with no NavCom and an effective mission that is
/// neither Guard nor Area Guard nor frozen: Area Guard when the owner's IQ
/// (`+0x24C`) reaches `[IQ] GuardArea=` (`Rules+0x1440`), the Unit has the
/// veteran GUARD_AREA ability (`0x0070D0D0`, 0x10) or its type is
/// `DefaultToGuardArea=` (`+0xD39`), unless it is in a team (`+0x5D4`), a
/// slave (`+0x2DC`), a slave master (`+0x2D8`) or in a tank bunker
/// (`+0x2E4`); otherwise Guard. Unlike Infantry's pick, a human's House
/// takes the IQ test too.
fn unit_idle_guard_pick(sim: &Simulation, id: u64, rules: &RuleSet) -> MissionType {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return MissionType::Guard;
    };
    let wide = sim
        .houses
        .get(&entity.owner())
        .is_some_and(|house| house.current_iq() >= rules.general.iq_guard_area)
        || sim
            .object_type(entity.type_ref(), rules)
            .is_some_and(|object| {
                crate::sim::combat::veterancy::has_weapon_ability(
                    entity.veterancy.rank_kind(),
                    object,
                    crate::rules::object_type::Ability::GuardArea,
                ) || object.default_to_guard_area
            });
    if wide
        && sim.team_script_vm.team_for_member(id).is_none()
        && entity.slave.owner().is_none()
        && entity.slave_manager.is_none()
        && entity.bunker.link().installed_in().is_none()
    {
        MissionType::AreaGuard
    } else {
        MissionType::Guard
    }
}

/// The no-destination harvester selector of `UnitClass::Enter_Idle_Mode @
/// 0x00738970`, for a unit whose `Harvester=`/`Weeder=` flag
/// (`UnitType+0xE0E`/`+0xE0F`) is set. Returns the mission the body commits
/// through `Queue_Mission(selector, 0)` (`+0x1E8` = 0x005B35E0), or `None`
/// on one of the harvester arm's early returns (nothing assigned).
///
/// Body, decompiled 2026-09-06:
/// - `RadioClass::In_Radio_Contact` ⇒ return (nothing assigned);
/// - current (`+0xAC`) or queued (`+0xB4`) == Harvest(10) ⇒ return;
/// - selector = Harvest; when the FIRST explicit argument is 0 AND the OWNER passes
///   `HouseClass::IsControlledByHuman @ 0x0050B730`: the cell under the unit
///   (`MapClass::Get_CellClass_At_Coord`) has `LandType` (`CellClass+0xEC`)
///   ≠ 5 (Tiberium; 0xB Weeds for a Weeder) ⇒ selector = Guard(5). An AI
///   house always takes Harvest; so does every caller passing first explicit argument 1
///   (`TechnoClass::Unlimbo @ 0x006F6E2A` calls `+0x484(1, 1)`, which is why
///   a freshly built miner always leaves the factory on Harvest);
/// - `Assign_Target(0)` (`+0x3C8`), `Assign_Destination(0, 1)` (`+0x480`) —
///   the callers own those writes;
/// - the concrete Unit receiver clears target and destination before its tail
///   gate on current {Patrol25, AreaGuard11, Unload16, Eaten9}.
///
/// The concrete receiver is `Simulation::unit_enter_idle_mode`; callers use
/// it for the Foot base, installed destination, setter and queue effects. This
/// pure selector stays local to that receiver.
///
/// `skip_human_land_check` is the FIRST explicit argument != 0.
/// Original738970 loads caller arg1 into BL; miner gate738C0A tests BL.
/// Decompiler parameter numbering included implicit this. Terminal +484(0,1)
/// therefore retains the human land check.
fn harvester_idle_selection(
    sim: &Simulation,
    id: u64,
    rules: &RuleSet,
    skip_human_land_check: bool,
) -> Option<MissionType> {
    let entity = sim.substrate.entities.get(id)?;
    if !entity.radio_contacts.is_empty() {
        return None;
    }
    let current = entity.mission.current().known();
    if current == Some(MissionType::Harvest)
        || entity.mission.queued() == MissionId::from_known(MissionType::Harvest)
    {
        return None;
    }
    let human = !skip_human_land_check
        && sim
            .houses
            .get(&entity.owner())
            .is_none_or(|house| house.is_controlled_by_human(sim.session.game_mode_nonzero()));
    let weeder = sim
        .object_type(entity.type_ref(), rules)
        .is_some_and(|obj| !obj.harvester && obj.weeder);
    let wanted_land = if weeder {
        crate::rules::terrain_rules::LandType::Weeds
    } else {
        crate::rules::terrain_rules::LandType::Tiberium
    };
    let land_matches =
        cell_land_type_is(sim, entity.position.rx(), entity.position.ry(), wanted_land);
    Some(if human && !land_matches {
        MissionType::Guard
    } else {
        MissionType::Harvest
    })
}

/// `CellClass+0xEC` (`LandType`) of one cell: the resolved terrain's land
/// type, which the overlay recompute keeps current when ore is placed or
/// removed. Without resolved terrain no cell has a land type.
fn cell_land_type_is(
    sim: &Simulation,
    rx: u16,
    ry: u16,
    wanted: crate::rules::terrain_rules::LandType,
) -> bool {
    sim.resolved_terrain.as_ref().is_some_and(|terrain| {
        terrain
            .cell(rx, ry)
            .is_some_and(|cell| cell.land_type == wanted.as_index())
    })
}
