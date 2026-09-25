//! Drive4B0F20/Ship6A0980 world execution for every retained track. The retained cursor belongs
//! to the locomotor; TrackProcess owns just this call's paid budget and raw
//! descriptor. Every world receiver ends the entity borrow before continuing.
//!
//! Native boundaries and executable scalar witnesses are documented by
//! track_process.rs. PerCell currently admits MCV, ordered crush, sensor and
//! playfield receivers. Discovery/tag4, crates and the refinery radio branch
//! remain explicit receiver gaps; this host does not replay object AI for them.

use super::ground_pose::{commit_ground_height, position_world_coord};
use super::locomotor::MovementLayer;
use super::track_process::{TrackFamily, TrackInvocation, TrackPayment, TrackProcess};
#[cfg(test)]
use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{DriveCoord, DriveOccupationFootprint, TrackProgress};
use crate::sim::game_entity::GameEntity;
use crate::sim::lifecycle_request::{LifecycleRequest, UninitReason};
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;

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

fn progress(entity: &GameEntity, family: TrackFamily) -> Option<&TrackProgress> {
    match family {
        TrackFamily::Drive => entity.drive_locomotion.as_ref().map(|state| &state.track),
        TrackFamily::Ship => entity.ship_locomotion.as_ref().map(|state| &state.track),
    }
}

fn progress_mut(entity: &mut GameEntity, family: TrackFamily) -> Option<&mut TrackProgress> {
    match family {
        TrackFamily::Drive => entity
            .drive_locomotion
            .as_mut()
            .map(|state| &mut state.track),
        TrackFamily::Ship => entity
            .ship_locomotion
            .as_mut()
            .map(|state| &mut state.track),
    }
}

fn head(entity: &GameEntity, family: TrackFamily) -> DriveCoord {
    match family {
        TrackFamily::Drive => entity
            .drive_locomotion
            .as_ref()
            .and_then(|state| state.head_to),
        TrackFamily::Ship => entity
            .ship_locomotion
            .as_ref()
            .and_then(|state| state.head_to),
    }
    .unwrap_or(DriveCoord { x: 0, y: 0, z: 0 })
}

fn head_or_null(entity: Option<&GameEntity>, family: TrackFamily) -> DriveCoord {
    entity
        .map(|entity| head(entity, family))
        .unwrap_or(DriveCoord { x: 0, y: 0, z: 0 })
}

fn set_head(entity: &mut GameEntity, family: TrackFamily, value: Option<DriveCoord>) {
    match family {
        TrackFamily::Drive => {
            if let Some(state) = entity.drive_locomotion.as_mut() {
                state.head_to = value;
            }
        }
        TrackFamily::Ship => {
            if let Some(state) = entity.ship_locomotion.as_mut() {
                state.head_to = value;
            }
        }
    }
}

fn set_track_valid(entity: &mut GameEntity, family: TrackFamily, value: bool) {
    match family {
        TrackFamily::Drive => {
            if let Some(state) = entity.drive_locomotion.as_mut() {
                state.track_valid = value;
            }
        }
        TrackFamily::Ship => {
            if let Some(state) = entity.ship_locomotion.as_mut() {
                state.track_valid = value;
            }
        }
    }
}

fn cell(coord: DriveCoord) -> (u16, u16) {
    ((coord.x / 256) as u16, (coord.y / 256) as u16)
}

fn put_coords(entity: &mut GameEntity, coord: DriveCoord) {
    let (rx, ry) = cell(coord);
    entity.position.rx = rx;
    entity.position.ry = ry;
    entity.position.sub_x = SimFixed::from_num(coord.x.wrapping_sub(i32::from(rx as i16) * 256));
    entity.position.sub_y = SimFixed::from_num(coord.y.wrapping_sub(i32::from(ry as i16) * 256));
    entity.position.exact_z_leptons = Some(coord.z);
}

impl Simulation {
    /// Drive Force_Track4B0C40, on the ILoco interface (+4 receiver).
    /// Selector/cursor publication precedes the null-coordinate return. Head,
    /// destination, residual and owner speed have independent lifetimes.
    pub(crate) fn force_drive_track(
        &mut self,
        id: u64,
        selector: i32,
        supplied: DriveCoord,
    ) -> bool {
        self.force_drive_track_observed(id, selector, supplied, &mut |_, _, _| true)
    }

    fn force_drive_track_observed(
        &mut self,
        id: u64,
        selector: i32,
        supplied: DriveCoord,
        receive: &mut impl FnMut(&mut Simulation, u64, DriveCoord) -> bool,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        if !entity
            .locomotor
            .as_ref()
            .is_some_and(|loco| loco.kind == crate::rules::locomotor_type::LocomotorKind::Drive)
        {
            return false;
        }
        let drive = entity.drive_locomotion.get_or_insert_with(Default::default);
        drive.track.select_forced(selector);
        if supplied == (DriveCoord { x: 0, y: 0, z: 0 }) {
            return false;
        }
        drive.head_to = Some(supplied);
        drive.track_valid = true;
        //4B0D14/4B0D1B: address the supplied cell, then synchronous crate
        // pickup. The shared track host's crate receiver is still incomplete;
        // the observer preserves its callback/reload boundary for witnesses.
        let received = receive(self, id, supplied);
        let survives = self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| received && !entity.lifecycle.in_limbo);
        if !survives {
            if let Some(entity) = self.substrate.entities.get_mut(id)
                && entity.lifecycle.object_alive
                && let Some(drive) = entity.drive_locomotion.as_mut()
            {
                drive.head_to = None;
                drive.track_valid = false;
            }
            return false;
        }
        self.track_apply_occupation_at(id, TrackFamily::Drive, supplied, true, None);
        let Some(drive) = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|entity| entity.drive_locomotion.as_mut())
        else {
            return false;
        };
        drive.destination = Some(supplied);
        drive.target_speed_fraction = crate::util::fixed_math::SIM_ONE;
        true
    }

    /// Stop a caller-owned track before retiring its descriptor/head. A mere
    /// marker clear used to leave the retained cursor and raw claims alive.
    pub(crate) fn cancel_drive_track(&mut self, id: u64) {
        self.track_apply_occupation(id, TrackFamily::Drive, false, None);
        if let Some(entity) = self.substrate.entities.get_mut(id)
            && let Some(drive) = entity.drive_locomotion.as_mut()
        {
            drive.head_to = None;
            drive.destination = None;
            drive.track.clear_selector();
            drive.track_valid = false;
            drive.occupation_head_to = None;
            drive.occupation_handoff = None;
        }
    }

    pub(crate) fn run_track_process(
        &mut self,
        invocation: TrackInvocation,
        rules: Option<&RuleSet>,
        fallback_grid: Option<&PathGrid>,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<TrackPass, String> {
        if invocation.apply_fresh_occupation {
            self.track_apply_occupation(
                invocation.entity_id,
                invocation.family,
                true,
                fallback_grid,
            );
        }
        let object = self
            .substrate
            .entities
            .get(invocation.entity_id)
            .and_then(|e| rules?.object(self.interner.resolve(e.type_ref())));
        let Some(entity) = self.substrate.entities.get_mut(invocation.entity_id) else {
            return Ok(TrackPass::default());
        };
        if !super::track_turn::admit_track_entry(entity, object.is_some_and(|o| o.has_turret)) {
            return Ok(TrackPass::default());
        }
        // One native entry owns admission, scalar prefix and paid loop, in that
        // order. No scalar speed update crosses the world receiver handoff.
        // The prefix also runs for `retry`, whose budget masks its speed.
        let current_grid = self.path_grid.as_deref().or(fallback_grid);
        let fresh_budget = super::track_speed::advance(entity, object, rules, current_grid);
        self.try_run_track_points_observed(
            invocation,
            fresh_budget,
            rules,
            fallback_grid,
            registry,
            &mut |_, _, _| {},
        )
    }

    /// Geometry/receiver fixtures explicitly supply the paid budget; they do
    /// not certify the native entry or scalar prefix. Production enters above.
    #[cfg(test)]
    pub(super) fn run_track_points(
        &mut self,
        invocation: TrackInvocation,
        fresh_budget: i32,
        rules: Option<&RuleSet>,
        fallback_grid: Option<&PathGrid>,
        registry: Option<&OverlayTypeRegistry>,
    ) -> u32 {
        if invocation.apply_fresh_occupation {
            self.track_apply_occupation(
                invocation.entity_id,
                invocation.family,
                true,
                fallback_grid,
            );
        }
        self.run_track_points_observed(
            invocation,
            fresh_budget,
            rules,
            fallback_grid,
            registry,
            &mut |_, _, _| {},
        )
    }

    fn track_survives(&self, id: u64) -> bool {
        self.substrate.entities.get(id).is_some_and(|entity| {
            entity.lifecycle.object_alive
                && !entity.lifecycle.in_limbo
                && entity.object_is_falling_down == 0
        })
    }

    fn track_state(
        &self,
        id: u64,
        family: TrackFamily,
    ) -> Option<(TrackProgress, DriveCoord, DriveCoord)> {
        let entity = self.substrate.entities.get(id)?;
        Some((
            *progress(entity, family)?,
            head(entity, family),
            position_world_coord(&entity.position),
        ))
    }

    /// Shared paid-loop body. The observer tests synchronous receiver effects;
    /// the caller has already completed admission and the scalar prefix.
    #[cfg(test)]
    pub(super) fn run_track_points_observed(
        &mut self,
        invocation: TrackInvocation,
        fresh_budget: i32,
        rules: Option<&RuleSet>,
        fallback_grid: Option<&PathGrid>,
        registry: Option<&OverlayTypeRegistry>,
        observe: &mut impl FnMut(&mut Simulation, u64, TrackWorldEvent),
    ) -> u32 {
        self.try_run_track_points_observed(
            invocation,
            fresh_budget,
            rules,
            fallback_grid,
            registry,
            observe,
        )
        .expect("track fixture must provide every coordinate receiver")
        .moved
    }

    fn try_run_track_points_observed(
        &mut self,
        invocation: TrackInvocation,
        fresh_budget: i32,
        rules: Option<&RuleSet>,
        fallback_grid: Option<&PathGrid>,
        registry: Option<&OverlayTypeRegistry>,
        observe: &mut impl FnMut(&mut Simulation, u64, TrackWorldEvent),
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
                .path_replay
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
                self.set_track_occupation_enabled(id, true);
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    entity.navigation.path_runtime.path_blocked = false;
                }
                self.track_place(
                    id,
                    stored_head,
                    cell(current),
                    true,
                    rules,
                    fallback_grid,
                    registry,
                    observe,
                );
                moved = moved.saturating_add(1);
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    set_head(entity, family, None);
                    // Drive4B2104 / Ship6A1747 clear the active class's +63
                    // before selector retirement and the terminal PerCell.
                    set_track_valid(entity, family, false);
                    if let Some(state) = progress_mut(entity, family) {
                        state.clear_selector();
                    }
                    if let Some(drive) = entity.drive_locomotion.as_mut() {
                        drive.occupation_head_to = None;
                        drive.occupation_handoff = None;
                    }
                    if let Some(ship) = entity.ship_locomotion.as_mut() {
                        ship.occupation_head_to = None;
                        ship.occupation_handoff = None;
                    }
                }
                // Native clears Head_To and selector before target+4C, then
                // queries the owner's physical cell and fresh +4C height.
                let reached = self.track_reached_destination(id, family, rules)?;
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    if reached {
                        match family {
                            TrackFamily::Drive => {
                                if let Some(state) = entity.drive_locomotion.as_mut() {
                                    state.destination = None;
                                }
                            }
                            TrackFamily::Ship => {
                                if let Some(state) = entity.ship_locomotion.as_mut() {
                                    state.destination = None;
                                }
                            }
                        }
                    }
                }
                // Retire only the completed path adapter before PerCell. The
                // native terminal tail has no legacy finalizer/body-turn reset;
                // a callback may install a new path that must survive this tail.
                self.track_consume_reached_node(id);
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    if reached {
                        entity.movement_target = None;
                    } else if entity
                        .movement_target
                        .as_ref()
                        .is_some_and(|target| target.next_index >= target.path.len())
                    {
                        super::movement_commands::spend_track_route(entity);
                    }
                }
                self.unit_track_per_cell(
                    id,
                    super::track_turn::PerCellReason::Arrival,
                    rules,
                    fallback_grid,
                );
                observe(self, id, TrackWorldEvent::PerCell);
                if !self.track_survives(id) {
                    return Ok(TrackPass::aborted(moved));
                }
                if reached {
                    let entity = self.substrate.entities.get_mut(id).unwrap();
                    super::navcom::foot_stop_moving(entity);
                    entity.navigation.path_replay.clear_live_head();
                    entity.navigation.pending_arrival_clear = false;
                    if entity.mission.current().known() == Some(MissionType::Move) {
                        let returns = self.track_enter_idle_mode(id, rules);
                        observe(self, id, TrackWorldEvent::Arrival);
                        if returns {
                            return Ok(TrackPass::aborted(moved));
                        }
                    }
                }
                if !reached {
                    if let Some(entity) = self.substrate.entities.get_mut(id) {
                        if entity.movement_target.is_none() {
                            entity.navigation.pending_arrival_clear =
                                entity.navigation.nav_com.is_some();
                        }
                    }
                }
                if self.track_navigation_gate(id, rules) {
                    return Ok(TrackPass::aborted(moved));
                }
                if !self
                    .substrate
                    .entities
                    .get(id)
                    .is_some_and(|e| e.lifecycle.object_alive)
                {
                    return Ok(TrackPass::paid(moved));
                }
                // +504 false/alive admits the residual tail directly. It must
                // never restart the paid loop with a newly selected curve.
                break;
            }

            if self.track_occupation_enabled(id) {
                self.track_raw_mark(id, false, fallback_grid);
                self.set_track_occupation_enabled(id, false);
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    entity.navigation.path_runtime.path_blocked = false;
                }
            }
            let Some((live, live_head, actual)) = self.track_state(id, family) else {
                return Ok(TrackPass::paid(moved));
            };
            let previous = if live.cursor == 0 {
                cell(actual)
            } else {
                call.previous_sample(live.cursor)
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
                .live_facing_sample(live.cursor)
                .and_then(|point| point.transform(family, &live, live_head))
                .map(|(_, facing)| facing);
            self.track_consume_reached_node(id);
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
                rules,
                fallback_grid,
                registry,
                observe,
            );
            moved = moved.saturating_add(1);
            if !self
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.lifecycle.object_alive)
            {
                return Ok(TrackPass::paid(moved));
            }
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                let marked = entity.lifecycle.cell_marked;
                entity.lifecycle.cell_marked = false;
                commit_ground_height(
                    &mut entity.position,
                    entity.on_bridge,
                    self.resolved_terrain.as_ref(),
                    self.path_grid.as_deref().or(fallback_grid),
                );
                entity.lifecycle.cell_marked = marked;
            }
            if let Some(facing) = paid_facing {
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    entity.facing = facing;
                    entity.facing_target = None;
                    if let Some(body) = entity.body_facing.as_mut() {
                        body.snap(u16::from(facing) << 8, self.session.binary_frame);
                    }
                }
            }
            let Some((live, _, _)) = self.track_state(id, family) else {
                return Ok(TrackPass::paid(moved));
            };
            if call.is_at_occupation_handoff(&live) {
                self.track_raw_mark(id, false, fallback_grid);
            }
            let Some((live, _, _)) = self.track_state(id, family) else {
                return Ok(TrackPass::paid(moved));
            };
            if chain_allowed && call.is_at_chain_cursor(&live) {
                if self.track_try_chain(
                    id,
                    family,
                    &mut call,
                    candidate_direction.unwrap(),
                    rules,
                    fallback_grid,
                    registry,
                    observe,
                )? {
                    chain_allowed = false;
                    if !self.track_survives(id) {
                        return Ok(TrackPass::paid(moved));
                    }
                }
            }
            let Some(entity) = self.substrate.entities.get_mut(id) else {
                return Ok(TrackPass::paid(moved));
            };
            let Some(state) = progress_mut(entity, family) else {
                return Ok(TrackPass::paid(moved));
            };
            call.finish_surviving_point(state);
            self.track_consume_reached_node(id);
        }
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return Ok(TrackPass::paid(moved));
        };
        let Some(state) = progress_mut(entity, family) else {
            return Ok(TrackPass::paid(moved));
        };
        call.store_residual(state);
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
                live.residual,
            );
            self.track_place(
                id,
                chosen,
                cell(current),
                false,
                rules,
                fallback_grid,
                registry,
                observe,
            );
        }
        Ok(TrackPass::paid(moved))
    }

    fn track_consume_reached_node(&mut self, id: u64) {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        let Some(target) = entity.movement_target.as_mut() else {
            return;
        };
        if target.path.get(target.next_index).copied()
            == Some((entity.position.rx, entity.position.ry))
        {
            // This compatibility cache follows the accepted path node being
            // consumed. The shared native track host owns XYZ/OnBridge and
            // list/raw occupation separately; ramps can legitimately disagree
            // with this path layer. Publish before the last adapter is retired,
            // because the next order uses this layer when no paid head remains.
            if let Some(locomotor) = entity.locomotor.as_mut() {
                locomotor.layer = target.layer_at(target.next_index);
            }
            target.next_index += 1;
            if let Some(&(x, y)) = target.path.get(target.next_index) {
                let (dx, dy, length) = crate::util::lepton::cell_delta_to_lepton_dir(
                    i32::from(x) - i32::from(entity.position.rx),
                    i32::from(y) - i32::from(entity.position.ry),
                );
                target.move_dir_x = dx;
                target.move_dir_y = dy;
                target.move_dir_len = length;
            }
        }
    }

    fn track_occupation_enabled(&self, id: u64) -> bool {
        self.substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.foot_occupation_enabled)
    }

    fn set_track_occupation_enabled(&mut self, id: u64, value: bool) {
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.foot_occupation_enabled = value;
        }
    }

    /// Unit7441B0/744210 select the raw plane from exact live XYZ; REMOVE
    /// deliberately does not require a surviving structural bridge flag.
    pub(super) fn track_raw_mark(&mut self, id: u64, put: bool, fallback_grid: Option<&PathGrid>) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let coord = position_world_coord(&entity.position);
        self.track_raw_mark_at(id, coord, put, fallback_grid);
    }

    fn track_raw_mark_at(
        &mut self,
        id: u64,
        coord: DriveCoord,
        put: bool,
        fallback_grid: Option<&PathGrid>,
    ) -> MovementLayer {
        let at = cell(coord);
        let terrain = self.resolved_terrain.as_ref();
        let ground = super::ground_pose::ground_surface_z_at(
            [coord.x, coord.y],
            false,
            terrain,
            self.path_grid.as_deref().or(fallback_grid),
        )
        .unwrap_or(coord.z);
        let structural = terrain
            .and_then(|terrain| terrain.cell(at.0, at.1))
            .is_some_and(|cell| cell.bridge_facts.has_structural_bridge());
        let deck = coord.z
            >= ground.wrapping_add(crate::util::lepton::BRIDGE_HEIGHT_DELTA_LEPTONS as i32)
            && (!put || structural);
        let layer = if deck {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        let raw = &mut self.substrate.raw_cell_occupation;
        match (put, deck) {
            (true, true) => raw.mark_deck(at.0, at.1, 0x20),
            (true, false) => raw.mark_ground(at.0, at.1, 0x20),
            (false, true) => raw.clear_deck(at.0, at.1, 0x20),
            (false, false) => raw.clear_ground(at.0, at.1, 0x20),
        }
        if put {
            self.substrate
                .cell_occupation
                .mark_vehicle_on_layer(at.0, at.1, id, layer);
        } else {
            self.substrate
                .cell_occupation
                .clear_vehicle_on_layer(at.0, at.1, id, layer);
        }
        if !put {
            let mark = DriveOccupationFootprint {
                rx: at.0,
                ry: at.1,
                layer,
            };
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                if let Some(state) = entity.drive_locomotion.as_mut() {
                    if state.occupation_handoff == Some(mark) {
                        state.occupation_handoff = None;
                    }
                    if state.occupation_head_to == Some(mark) {
                        state.occupation_head_to = None;
                    }
                }
                if let Some(state) = entity.ship_locomotion.as_mut() {
                    if state.occupation_handoff == Some(mark) {
                        state.occupation_handoff = None;
                    }
                    if state.occupation_head_to == Some(mark) {
                        state.occupation_head_to = None;
                    }
                }
            }
        }
        layer
    }

    fn track_set_coords(&mut self, id: u64, coord: DriveCoord, rules: Option<&RuleSet>) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if position_world_coord(&entity.position) == coord {
            return;
        }
        let cargo = rules
            .and_then(|rules| rules.object(self.interner.resolve(entity.type_ref())))
            .filter(|object| object.open_topped)
            .and_then(|_| entity.passenger_role.cargo())
            .map(|cargo| cargo.passengers.clone())
            .unwrap_or_default();
        put_coords(self.substrate.entities.get_mut(id).unwrap(), coord);
        // Foot4DB810 -> Techno7104F0 propagates changed XYZ to OpenTopped
        // cargo in cargo-list order, before the caller resumes Mark(PUT).
        for passenger in cargo {
            if let Some(entity) = self.substrate.entities.get_mut(passenger) {
                put_coords(entity, coord);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn track_place(
        &mut self,
        id: u64,
        coord: DriveCoord,
        previous_track_cell: (u16, u16),
        terminal: bool,
        rules: Option<&RuleSet>,
        fallback_grid: Option<&PathGrid>,
        registry: Option<&OverlayTypeRegistry>,
        observe: &mut impl FnMut(&mut Simulation, u64, TrackWorldEvent),
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let old = (entity.position.rx, entity.position.ry);
        let selected_cell = cell(coord);
        let crossing = old != selected_cell;
        if crossing {
            self.foot_mark_remove(id, rules, fallback_grid, registry);
            observe(self, id, TrackWorldEvent::MarkRemove);
        }
        let saved_marked = if !crossing {
            self.substrate.entities.get_mut(id).map(|entity| {
                let marked = entity.lifecycle.cell_marked;
                entity.lifecycle.cell_marked = false;
                marked
            })
        } else {
            None
        };
        self.track_set_coords(id, coord, rules);
        observe(self, id, TrackWorldEvent::SetCoords);
        if crossing && !terminal {
            // Crossing uses actual current cell; this predicate instead uses
            // the previous transformed cached point (or current at cursor0).
            let grid = self.path_grid.as_deref().or(fallback_grid);
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                if let Some((source, destination)) = grid.and_then(|grid| {
                    grid.cell(previous_track_cell.0, previous_track_cell.1)
                        .zip(grid.cell(selected_cell.0, selected_cell.1))
                }) {
                    match super::movement_bridge::compute_bridge_transition(source, destination) {
                        super::movement_bridge::BridgeTransition::Enter { deck_level } => {
                            entity.on_bridge = true;
                            entity.bridge_occupancy =
                                Some(crate::sim::components::BridgeOccupancy { deck_level });
                        }
                        super::movement_bridge::BridgeTransition::Exit => {
                            entity.on_bridge = false;
                            entity.bridge_occupancy = None;
                        }
                        super::movement_bridge::BridgeTransition::NoChange => {}
                    }
                }
            }
        }
        if terminal {
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                commit_ground_height(
                    &mut entity.position,
                    entity.on_bridge,
                    self.resolved_terrain.as_ref(),
                    self.path_grid.as_deref().or(fallback_grid),
                );
            }
        }
        if let Some(marked) = saved_marked {
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                entity.lifecycle.cell_marked = marked;
            }
        }
        if crossing {
            self.foot_mark_put_observed(id, rules, fallback_grid, registry, &mut |sim, id| {
                observe(sim, id, TrackWorldEvent::MarkPut)
            });
        }
    }

    /// Apply_Track_Occupation_Mode(0/1), Drive4B0AD0 / Ship6A01A0:
    /// direct raw handoff then full supplied head, with no Foot+6B6 gate.
    pub(super) fn track_apply_occupation(
        &mut self,
        id: u64,
        family: TrackFamily,
        put: bool,
        fallback_grid: Option<&PathGrid>,
    ) {
        let supplied = head_or_null(self.substrate.entities.get(id), family);
        self.track_apply_occupation_at(id, family, supplied, put, fallback_grid);
    }

    fn track_apply_occupation_at(
        &mut self,
        id: u64,
        family: TrackFamily,
        supplied: DriveCoord,
        put: bool,
        fallback_grid: Option<&PathGrid>,
    ) {
        let Some((state, retained_head, current)) = self.track_state(id, family) else {
            return;
        };
        if supplied == (DriveCoord { x: 0, y: 0, z: 0 }) {
            return;
        }
        let handoff = (!state.reversed && state.turn_index != -1)
            .then(|| {
                let index = usize::try_from(state.turn_index).ok()?;
                if family == TrackFamily::Ship && index >= 64 {
                    return None;
                }
                let turn = super::drive_track::turn_track_at(index)?;
                if turn.normal_track == 0 {
                    return None;
                }
                let raw = super::drive_track::raw_track_meta(turn.normal_track)?;
                if raw.occupation_handoff_point_index < 0
                    || state.cursor >= i32::from(raw.occupation_handoff_point_index)
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
        let handoff_mark = handoff.map(|coord| {
            let layer = self.track_raw_mark_at(id, coord, put, fallback_grid);
            DriveOccupationFootprint {
                rx: cell(coord).0,
                ry: cell(coord).1,
                layer,
            }
        });
        let layer = self.track_raw_mark_at(id, supplied, put, fallback_grid);
        if put {
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                let marks = match family {
                    TrackFamily::Drive => entity.drive_locomotion.as_mut().map(|state| {
                        (&mut state.occupation_handoff, &mut state.occupation_head_to)
                    }),
                    TrackFamily::Ship => entity.ship_locomotion.as_mut().map(|state| {
                        (&mut state.occupation_handoff, &mut state.occupation_head_to)
                    }),
                };
                if let Some((handoff, head)) = marks {
                    *handoff = handoff_mark;
                    *head = Some(DriveOccupationFootprint {
                        rx: cell(supplied).0,
                        ry: cell(supplied).1,
                        layer,
                    });
                }
            }
        }
    }

    /// FootLimbo4DB260 calls active ILocomotion+9C(mode0) before TechnoLimbo.
    /// Drive4B48D0 / Ship6A3F00 forward the live head to Apply0; instance
    /// retirement must not erase that descriptor/head before this receiver.
    pub(crate) fn release_track_occupation_before_foot_limbo(&mut self, id: u64) {
        let family = self.substrate.entities.get(id).and_then(|entity| {
            // Foot4DB266..26E skips Apply0 when already in limbo. A second
            // conceal must not destructively clear a newer owner's raw claim.
            if entity.lifecycle.in_limbo {
                return None;
            }
            match entity.locomotor.as_ref()?.kind {
                crate::rules::locomotor_type::LocomotorKind::Drive => Some(TrackFamily::Drive),
                crate::rules::locomotor_type::LocomotorKind::Ship => Some(TrackFamily::Ship),
                _ => None,
            }
        });
        if let Some(family) = family {
            self.track_apply_occupation(id, family, false, None);
        }
    }

    fn track_try_chain(
        &mut self,
        id: u64,
        family: TrackFamily,
        call: &mut TrackProcess,
        direction: u8,
        rules: Option<&RuleSet>,
        fallback_grid: Option<&PathGrid>,
        registry: Option<&OverlayTypeRegistry>,
        observe: &mut impl FnMut(&mut Simulation, u64, TrackWorldEvent),
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
        let saved_speed = entity.foot_speed.applied_fraction;
        //4B1BA1..4B1C3E: Unit+1AC(cell(head + delta), dir, Object 0x5F5F00,
        //0, 1), with no Mark bracket.
        let target = super::foot_path::coord_cell(candidate);
        let (height, native) = {
            let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
            let height = super::ground_pose::query_object_cell_height(
                &cells,
                position_world_coord(&entity.position),
                entity.on_bridge,
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
            //4B1E52..4B1E68: redraw (presentation), no chain.
            1 => return Ok(false),
            //4B1E6D..4B1EBB: the gate question, answer discarded.
            3 => {
                let owner = self
                    .substrate
                    .entities
                    .get(id)
                    .map(|actor| self.interner.resolve(actor.owner()).to_owned())
                    .unwrap_or_default();
                let _ = crate::sim::gate_runtime::request_gate_open_for_cell(
                    &mut self.substrate.entities,
                    &self.substrate.occupancy,
                    (target.0 as u16, target.1 as u16),
                    id,
                    &owner,
                    rules,
                    &self.house_alliances,
                    &self.interner,
                );
                return Ok(false);
            }
            //4B1EC0..4B1F43: the forced deck-aware Scatter_Objects on the cell.
            6 => {
                self.scatter_blocked_track_cell(id, target, rules, fallback_grid);
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
            .and_then(|entity| rules.object(self.interner.resolve(entity.type_ref())))
            .is_some_and(|object| object.passive)
        {
            return Ok(false);
        }
        let entity = self.substrate.entities.get_mut(id).unwrap();
        if !call.accept_chain(
            progress_mut(entity, family).unwrap(),
            selection.turn_track_index as i32,
        ) {
            return Ok(false);
        }
        set_head(entity, family, None);
        // Drive4B1CF5 / Ship6A1338 publish +63 for the PerCell receiver.
        set_track_valid(entity, family, true);
        self.unit_track_per_cell(
            id,
            super::track_turn::PerCellReason::Arrival,
            Some(rules),
            fallback_grid,
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
        // Crate pickup is an explicit receiver gap. A surviving pickup
        // precedes Apply1, then the saved owner fraction and live queue shift.
        self.track_apply_occupation(id, family, true, fallback_grid);
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.foot_speed.applied_fraction = saved_speed;
            super::path_markers::consume_path_replay(&mut entity.navigation.path_replay, 1);
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
        let Some(target) = entity.navigation.nav_com else {
            return Ok(false);
        };
        let coord = super::navcom::nav_target_coordinate(
            target,
            Some(id),
            &self.substrate.entities,
            self.resolved_terrain.as_ref(),
            rules.map(|rules| (rules, &self.interner)),
        )?;
        let destination = match family {
            TrackFamily::Drive => entity
                .drive_locomotion
                .as_ref()
                .and_then(|state| state.destination),
            TrackFamily::Ship => entity
                .ship_locomotion
                .as_ref()
                .and_then(|state| state.destination),
        };
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

    /// `UnitClass::Enter_Idle_Mode(0, 1)` (vt+0x484) from a class caller:
    /// the refinery dock's Mission_Enter refusal (`0x004D92E2`) and
    /// Mission_Unload's lost contact (`0x0073DEF2`).
    pub(crate) fn unit_enter_idle_mode(&mut self, id: u64, rules: Option<&RuleSet>) -> bool {
        self.track_enter_idle_mode(id, rules)
    }

    /// Bounded Unit738970 receiver. Existing idle selectors cover ordinary
    /// human vehicles/miners; deploy/radio/AI arms remain receiver residuals.
    pub(super) fn track_enter_idle_mode(&mut self, id: u64, rules: Option<&RuleSet>) -> bool {
        // Foot4D82D9 -> Techno709A54 lets a held Temporal target go first.
        self.temporal_release_if_warping(id);
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        // Foot4D833D..8376 queries/ends the active Drive before NavQueue.
        // Unit saves the base return but still executes its own receiver tail.
        let ended_drive = super::locomotor_owner::try_end_drive_at_foot_idle(entity);
        // Foot4D8382..83E2 takes NavQueue even with an existing NavCom.
        // Preserve its saved return independently from the resulting NavCom.
        // Non-Cell targets and other class END remain bounded base-receiver
        // gaps; no whole mission tick substitutes for those calls.
        let queued_cell = entity
            .navigation
            .nav_queue
            .first()
            .copied()
            .and_then(|next| {
                if let crate::sim::components::NavTargetRef::Cell { rx, ry } = next {
                    Some((rx, ry))
                } else {
                    None
                }
            });
        if let Some(next) = queued_cell {
            super::navcom::set_destination_internal_cell(
                entity,
                next,
                self.resolved_terrain.as_ref(),
            );
            entity.navigation.nav_queue.remove(0);
            entity.navigation.pending_arrival_clear = true;
        }
        // Normal Foot4D8538 invokes +544(0.0); Unit dispatch4D3710
        // writes Foot+578. The true-return NavQueue arm skips this setter.
        if !ended_drive && entity.navigation.nav_queue.is_empty() && queued_cell.is_none() {
            entity.foot_speed.applied_fraction = crate::util::fixed_math::SIM_ZERO;
            entity.foot_speed.cached_current_speed = 0;
            if let Some(target) = entity.movement_target.as_mut() {
                target.current_speed = crate::util::fixed_math::SIM_ZERO;
            }
        }
        let saved_base_return = ended_drive || queued_cell.is_some();
        let has_destination = entity.navigation.nav_com.is_some();
        let current = entity.mission.current().known();
        let miner = rules
            .and_then(|rules| rules.object(self.interner.resolve(entity.type_ref())))
            .is_some_and(|object| object.harvester || object.weeder);
        // Unit738AB4..ABC preserves pending Unit+68C and returns the saved
        // Foot result before Guard/target/destination/mission side effects.
        if !has_destination && !miner && entity.mcv_deploy_pending {
            return saved_base_return;
        }
        let selection = if has_destination {
            Some(MissionType::Move)
        } else if miner {
            rules.and_then(|rules| {
                crate::sim::world::harvester_enter_idle_mode_selector(self, id, rules, false)
            })
        } else {
            Some(MissionType::Guard)
        };
        if selection.is_some() && !has_destination {
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                // Unit738AF5/738C75 calls the virtual target setter before
                // the Guard/Harvest queue; clearing only Target loses its
                // retained burst reset (Techno6FCF5B).
                crate::sim::mission::concrete_effects::represented_assign_target(entity, None);
            }
        }
        // Unit738CFA..D12 suppresses assignment only after preceding writes.
        let selection = selection.filter(|_| {
            !matches!(
                current,
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
                self.session.binary_frame,
                &crate::sim::mission::authority::EntityReadyInputProvider,
            );
        }
        saved_base_return
    }

    fn track_navigation_gate(&mut self, id: u64, rules: Option<&RuleSet>) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if entity.navigation.nav_com.is_some() || entity.attack_target.is_some() {
            return false;
        }
        self.track_enter_idle_mode(id, rules)
    }

    /// Unit Per_Cell_Process(2) (`0x00739EC0`) outside a track: the Teleport
    /// warp's arrival call (`0x0071971C`).
    pub(crate) fn unit_per_cell_process_arrival(&mut self, id: u64, rules: Option<&RuleSet>) {
        self.unit_track_per_cell(id, super::track_turn::PerCellReason::Arrival, rules, None);
    }

    pub(super) fn unit_track_per_cell(
        &mut self,
        id: u64,
        reason: super::track_turn::PerCellReason,
        rules: Option<&RuleSet>,
        _fallback_grid: Option<&PathGrid>,
    ) {
        // Unit739EC0 invokes the MCV receiver before normal crush/Foot tail.
        if let Some(rules) = rules {
            crate::sim::mcv_deploy::per_cell_process(self, id, rules);
        }
        if !self.track_survives(id) {
            return;
        }
        // 0x0073A31F..0x0073A5EA, before the Ready/Commence below: a tethered
        // unit on Enter arriving north-adjacent to its dock sends DOCK_NOW.
        if let Some(rules) = rules
            && reason == super::track_turn::PerCellReason::Arrival
        {
            crate::sim::miner::per_cell_dock_now(self, rules, id);
        }
        // Unit PerCell2 739EC0: after MCV retry, +6D1==0 admits
        // Ready(+200)73ACC2 -> Commence(+1EC)73ACD1, BEFORE full-cell
        // crush73B089 and Foot sensor/playfield tail73B0A0. The existing
        // miner.unload_active owns +6D1 (ctor7353FE, unload73DFDA).
        // This promotes only: it does not dispatch a mission handler or
        // repeat the object AI prefix. mission_host_promote retains its
        // documented unavailable locomotor/height fallback for other inputs.
        let promote = reason == super::track_turn::PerCellReason::Arrival
            && self.substrate.entities.get(id).is_some_and(|entity| {
                !entity
                    .miner
                    .as_ref()
                    .is_some_and(|miner| miner.unload_active)
            });
        if let Some(rules) = rules.filter(|_| promote) {
            self.mission_host_promote(id, self.session.binary_frame, rules);
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let at = (entity.position.rx, entity.position.ry);
        let layer = if entity.on_bridge {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        let mut cursor = self
            .substrate
            .occupancy
            .get(at.0, at.1)
            .and_then(|list| list.first_on_layer(layer));
        while let Some(victim) = cursor {
            // Save successor BEFORE lifecycle can remove the current object.
            cursor = self
                .substrate
                .occupancy
                .get(at.0, at.1)
                .and_then(|list| list.next_on_layer(layer, victim));
            if victim == id {
                continue;
            }
            let Some(crusher) = self.substrate.entities.get(id) else {
                return;
            };
            let coord = position_world_coord(&crusher.position);
            let capability = super::bump_crush::CrushCapability::new(
                crusher.regular_crusher,
                crusher.omni_crusher,
            );
            let kills = super::bump_crush::classify_drive_crush_phase(
                super::bump_crush::DriveCrushPhase::FullyInCell,
                &[victim],
                &self.substrate.entities,
                id,
                &self.house_alliances,
                &self.interner,
                (coord.x, coord.y),
                capability,
                super::bump_crush::ScatterEligibility::from_rules(rules),
                self.session.binary_frame,
                rules,
                &self.houses,
            );
            if !matches!(kills, super::bump_crush::DriveCrushOutcome::Kill { ref victims } if victims.contains(&victim))
            {
                continue;
            }
            if let Some(rules) = rules {
                if let Some(entity) = self.substrate.entities.get(victim) {
                    super::bump_crush::emit_crush_kill_sounds_at(
                        entity,
                        (i32::from(at.0), i32::from(at.1)),
                        rules,
                        &mut self.interner,
                        &mut self.sound_events,
                    );
                }
                let owner = self.substrate.entities.get(id).map(|entity| entity.owner());
                if let Some(entity) = self.substrate.entities.get_mut(victim) {
                    entity.health.current = 0;
                    crate::sim::combat::capture_kill_credit(entity, owner, rules, &self.interner);
                }
                crate::sim::combat::award_kill_experience(
                    &mut self.substrate.entities,
                    rules,
                    &self.interner,
                    &self.house_alliances,
                    id,
                    victim,
                );
                self.apply_lifecycle_request_with_rules(
                    LifecycleRequest::Uninit {
                        stable_id: victim,
                        reason: UninitReason::Crush,
                    },
                    rules,
                );
            } else {
                self.apply_lifecycle_request(LifecycleRequest::Uninit {
                    stable_id: victim,
                    reason: UninitReason::Crush,
                });
            }
        }
        if !self.track_survives(id) {
            return;
        }
        // Foot4D85D7 skips the reason2 body for turn completion. Shared
        // planning-waypoint maintenance at4D8DFD remains a required receiver
        // gap; it must not be substituted with the ordinary path queue.
        if reason == super::track_turn::PerCellReason::TurnComplete {
            return;
        }
        if let Some(rules) = rules {
            self.refresh_unit_sensor_at_per_cell(id, rules);
        }
        self.foot_neighbors_at_per_cell(id);
        if let Some(rules) = rules {
            crate::sim::world::techno_ai_cloak::uncloak_on_sensor_neighbour_after_cell_entry(
                self, id, rules,
            );
        }
        // `0x006F5090`'s head lets a held Temporal target go.
        self.temporal_release_if_warping(id);
        self.promote_entity_playfield_membership_after_move(id);
    }
}

#[cfg(test)]
#[path = "track_host_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "track_force_tests.rs"]
mod force_tests;
