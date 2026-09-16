//! Ordinary Drive4B0F20/Ship6A0980 world execution. The retained cursor belongs
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
use crate::sim::occupancy::CellListInsertion;
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::cell_entry::CellEntryResult;
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

fn set_head(entity: &mut GameEntity, family: TrackFamily, value: Option<DriveCoord>) {
    match family {
        TrackFamily::Drive => {
            if let Some(state) = entity.drive_locomotion.as_mut() {
                state.head_to = value;
                if value.is_none() {
                    state.pending_track_occupation = false;
                }
            }
        }
        TrackFamily::Ship => {
            if let Some(state) = entity.ship_locomotion.as_mut() {
                state.head_to = value;
                if value.is_none() {
                    state.pending_track_occupation = false;
                }
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
    pub(crate) fn run_ordinary_track_process(
        &mut self,
        invocation: TrackInvocation,
        rules: Option<&RuleSet>,
        fallback_grid: Option<&PathGrid>,
        registry: Option<&OverlayTypeRegistry>,
    ) -> u32 {
        self.run_ordinary_track_process_observed(
            invocation,
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

    /// The observer is used by integration fixtures to exercise mutations at
    /// the actual production boundary; it is a no-op in normal execution.
    pub(super) fn run_ordinary_track_process_observed(
        &mut self,
        invocation: TrackInvocation,
        rules: Option<&RuleSet>,
        fallback_grid: Option<&PathGrid>,
        registry: Option<&OverlayTypeRegistry>,
        observe: &mut impl FnMut(&mut Simulation, u64, TrackWorldEvent),
    ) -> u32 {
        let TrackInvocation {
            entity_id: id,
            family,
            fresh_budget,
        } = invocation;
        let pending = self.substrate.entities.get_mut(id).is_some_and(|entity| {
            let pending = match family {
                TrackFamily::Drive => entity
                    .drive_locomotion
                    .as_mut()
                    .map(|state| &mut state.pending_track_occupation),
                TrackFamily::Ship => entity
                    .ship_locomotion
                    .as_mut()
                    .map(|state| &mut state.pending_track_occupation),
            };
            pending.is_some_and(std::mem::take)
        });
        if pending {
            self.track_apply_occupation(id, family, true, fallback_grid);
        }
        let Some((state, _, _)) = self.track_state(id, family) else {
            return 0;
        };
        let mut call = TrackProcess::begin(family, &state, fresh_budget);
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
                return moved;
            };
            let Some(payment) = call.pay_current(&state) else {
                return moved;
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
                let reached = self.track_reached_destination(id, family);
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    set_head(entity, family, None);
                    if let Some(state) = progress_mut(entity, family) {
                        state.clear_selector();
                    }
                    entity.drive_track = None;
                    if let Some(drive) = entity.drive_locomotion.as_mut() {
                        drive.track_valid = false;
                        drive.occupation_head_to = None;
                        drive.occupation_handoff = None;
                    }
                    if let Some(ship) = entity.ship_locomotion.as_mut() {
                        ship.occupation_head_to = None;
                        ship.occupation_handoff = None;
                    }
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
                    if reached
                        || entity
                            .movement_target
                            .as_ref()
                            .is_some_and(|target| target.next_index >= target.path.len())
                    {
                        entity.movement_target = None;
                        entity.navigation.pending_arrival_clear =
                            !reached && entity.navigation.nav_com.is_some();
                    }
                }
                self.track_per_cell(id, rules, fallback_grid);
                observe(self, id, TrackWorldEvent::PerCell);
                if !self.track_survives(id) {
                    return moved;
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
                            return moved;
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
                    return moved;
                }
                if !self
                    .substrate
                    .entities
                    .get(id)
                    .is_some_and(|e| e.lifecycle.object_alive)
                {
                    return moved;
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
                return moved;
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
                return moved;
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
                return moved;
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
                return moved;
            };
            if call.is_at_occupation_handoff(&live) {
                self.track_raw_mark(id, false, fallback_grid);
            }
            let Some((live, _, _)) = self.track_state(id, family) else {
                return moved;
            };
            if chain_allowed && call.is_at_chain_cursor(&live) {
                if self.track_try_chain(
                    id,
                    family,
                    &mut call,
                    candidate_direction.unwrap(),
                    rules,
                    fallback_grid,
                    observe,
                ) {
                    chain_allowed = false;
                    if !self.track_survives(id) {
                        return moved;
                    }
                }
            }
            let Some(entity) = self.substrate.entities.get_mut(id) else {
                return moved;
            };
            let Some(state) = progress_mut(entity, family) else {
                return moved;
            };
            call.finish_surviving_point(state);
            self.track_consume_reached_node(id);
        }
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return moved;
        };
        let Some(state) = progress_mut(entity, family) else {
            return moved;
        };
        call.store_residual(state);
        let Some((live, stored_head, current)) = self.track_state(id, family) else {
            return moved;
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
        moved
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
    fn track_raw_mark(&mut self, id: u64, put: bool, fallback_grid: Option<&PathGrid>) {
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
            let removed_layer = self.substrate.entities.get_mut(id).and_then(|entity| {
                if entity.lifecycle.in_limbo || !entity.lifecycle.cell_marked {
                    return None;
                }
                entity.lifecycle.cell_marked = false;
                Some(if entity.on_bridge {
                    MovementLayer::Bridge
                } else {
                    MovementLayer::Ground
                })
            });
            if let Some(layer) = removed_layer {
                self.substrate
                    .occupancy
                    .remove_on_layer(old.0, old.1, id, layer);
                // RemoveContent's raw clear/Recalc still run if list search
                // found no link. The Foot enable is re-read after unlink.
                if self.track_occupation_enabled(id) {
                    self.track_raw_mark(id, false, fallback_grid);
                }
                self.recalculate_track_cell(old, rules, registry);
            }
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
            let entered_cell = self.substrate.entities.get_mut(id).and_then(|entity| {
                if entity.lifecycle.in_limbo || entity.lifecycle.cell_marked {
                    return None;
                }
                // Object5F58F7 publishes marked=true BEFORE Foot Enter.
                entity.lifecycle.cell_marked = true;
                let at = (entity.position.rx, entity.position.ry);
                let layer = if entity.on_bridge {
                    MovementLayer::Bridge
                } else {
                    MovementLayer::Ground
                };
                entity.occupancy_enter_order = self.substrate.next_occupancy_enter_order.next();
                self.substrate.occupancy.add(
                    at.0,
                    at.1,
                    id,
                    layer,
                    entity.sub_cell,
                    CellListInsertion::from_category(entity.category),
                );
                Some(at)
            });
            if let Some(entered_cell) = entered_cell {
                // AddContent47E8A0 discovery/tag4 belongs here. Map object
                // tags are not represented; no periodic trigger substitute.
                observe(self, id, TrackWorldEvent::MarkPut);
                if self.track_occupation_enabled(id) {
                    self.track_raw_mark(id, true, fallback_grid);
                }
                // Enter retains the addressed cell for its post-discovery
                // slot relookup; a callback's new owner XYZ is only raw-mark's
                // input, not the Recalc receiver coordinate.
                self.recalculate_track_cell(entered_cell, rules, registry);
            }
        }
    }

    /// Apply_Track_Occupation_Mode(0/1), Drive4B0AD0 / Ship6A01A0:
    /// direct raw handoff then full supplied head, with no Foot+6B6 gate.
    fn track_apply_occupation(
        &mut self,
        id: u64,
        family: TrackFamily,
        put: bool,
        fallback_grid: Option<&PathGrid>,
    ) {
        let Some((state, supplied, current)) = self.track_state(id, family) else {
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
                    x: supplied.x.wrapping_add(i32::from(x)),
                    y: supplied.y.wrapping_add(i32::from(y)),
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
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                match family {
                    TrackFamily::Drive => {
                        if let Some(state) = entity.drive_locomotion.as_mut() {
                            state.pending_track_occupation = false;
                        }
                    }
                    TrackFamily::Ship => {
                        if let Some(state) = entity.ship_locomotion.as_mut() {
                            state.pending_track_occupation = false;
                        }
                    }
                }
            }
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
        observe: &mut impl FnMut(&mut Simulation, u64, TrackWorldEvent),
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let Some(from) = call.chain_target_facing() else {
            return false;
        };
        if direction >= 8 || crate::util::direction::direction_from_facing(from) == direction {
            return false;
        }
        let Some(selection) = super::drive_track::select_drive_track(from, direction * 32, false)
        else {
            return false;
        };
        if selection.entry_index == 0 {
            return false;
        }
        let candidate = super::track_head::offset_head(head(entity, family), direction);
        let saved_speed = entity.foot_speed.applied_fraction;
        let grid = self.path_grid.as_deref().or(fallback_grid);
        let Some(snapshot) = super::movement_tick::snapshot_mover(
            &self.substrate.entities,
            id,
            self.playfield_bounds,
            rules,
            &self.interner,
        ) else {
            return false;
        };
        let entity = self.substrate.entities.get_mut(id).unwrap();
        let layer = if entity.on_bridge {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        let entry = super::movement_occupancy::evaluate_runtime_can_enter_cell_with_transition(
            grid,
            layer,
            &mut entity.runtime_bridge_transition,
            entity.on_bridge,
            super::movement_occupancy::RuntimeCanEnterCellArgs::runtime(
                cell(candidate),
                direction as i8,
                super::movement_occupancy::runtime_current_effective_height(
                    grid,
                    (entity.position.rx, entity.position.ry),
                    entity.on_bridge,
                    entity.position.z,
                ),
            ),
        );
        let chain = super::movement_tick::DeferredDriveTrackChain {
            target_cell: cell(candidate),
            head: candidate,
            layers: entry.layers,
            bridge_traversal_allowed: entry.bridge_traversal_allowed,
            cur_face: from,
            next_face: direction * 32,
        };
        let skips = super::movement_occupancy::build_live_building_entry_skip_map(
            &self.substrate.entities,
            id,
            &self.interner,
            rules,
        );
        let result = super::movement_tick::classify_drive_track_chain_entry(
            chain,
            id,
            &snapshot,
            grid,
            self.resolved_terrain.as_ref(),
            snapshot
                .speed_type
                .and_then(|speed| self.terrain_costs.get(&speed)),
            &self.substrate.occupancy,
            &self.substrate.cell_occupation,
            &skips,
            &self.substrate.entities,
            &self.house_alliances,
            &self.interner,
        );
        match result {
            // Original jump table4B2608 admits codes0 and2 here. Code1
            // goes to redraw4B1E52 and common advancement, without a chain.
            CellEntryResult::Clear
            | CellEntryResult::TemporaryBlock { .. }
            | CellEntryResult::TemporaryOccupation => {}
            CellEntryResult::ScatterRequired { .. } => {
                super::movement_tick::drive_track_chain_check_crushable_obstacle(
                    &mut self.substrate.entities,
                    &self.substrate.occupancy,
                    chain,
                    id,
                    &snapshot,
                    rules,
                    &self.house_alliances,
                    &self.interner,
                );
                return false;
            }
            CellEntryResult::FriendlyStationary { blocker_id } => {
                super::bump_crush::scatter_blocker(
                    &mut self.substrate.entities,
                    blocker_id,
                    grid,
                    self.resolved_terrain.as_ref(),
                    &self.substrate.occupancy,
                    chain.layers.object_list_layer,
                    &mut self.scenario_rng,
                    rules,
                    &self.interner,
                    crate::sim::movement::DestinationTiming::new(
                        self.session.binary_frame,
                        rules.map_or(self.blockage_path_delay_ticks, |r| {
                            r.general.blockage_path_delay_ticks
                        }),
                    ),
                );
                return false;
            }
            _ => return false,
        }
        // Actual Unit+2C dispatch746E20 returns1; the next test reads
        // UnitType Passive+E0C. Stock absent Passive defaults false.
        if !self
            .substrate
            .entities
            .get(id)
            .and_then(|entity| rules?.object(self.interner.resolve(entity.type_ref())))
            .is_some_and(|object| object.passive)
        {
            return false;
        }
        let entity = self.substrate.entities.get_mut(id).unwrap();
        if !call.accept_chain(
            progress_mut(entity, family).unwrap(),
            selection.turn_track_index as i32,
        ) {
            return false;
        }
        set_head(entity, family, None);
        if let Some(drive) = entity.drive_locomotion.as_mut() {
            drive.track_valid = true;
        }
        self.track_per_cell(id, rules, fallback_grid);
        observe(self, id, TrackWorldEvent::PerCell);
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            if let Some(drive) = entity.drive_locomotion.as_mut() {
                drive.track_valid = false;
            }
        }
        if !self.track_survives(id) {
            return true;
        }
        let entity = self.substrate.entities.get_mut(id).unwrap();
        // Callback writes to the head are cleared before candidate install.
        set_head(entity, family, None);
        // Accepted Drive chain4B1DA5 restores +63 before candidate head
        // stores4B1DA9..4B1DB4, after the transient PerCell corridor.
        if family == TrackFamily::Drive
            && let Some(drive) = entity.drive_locomotion.as_mut()
        {
            drive.track_valid = true;
        }
        set_head(entity, family, Some(candidate));
        // Crate pickup is an explicit receiver gap. A surviving pickup
        // precedes Apply1, then the saved owner fraction and live queue shift.
        self.track_apply_occupation(id, family, true, fallback_grid);
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.foot_speed.applied_fraction = saved_speed;
            super::path_markers::consume_path_replay(&mut entity.navigation.path_replay, 1);
        }
        true
    }

    fn track_reached_destination(&self, id: u64, family: TrackFamily) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let Some(target) = entity.navigation.nav_com else {
            return false;
        };
        let coord = match target {
            crate::sim::components::NavTargetRef::Cell { rx, ry } => {
                super::navcom::target_cell_coord(rx, ry, self.resolved_terrain.as_ref())
            }
            _ => {
                let Some(coord) = super::navcom::resolve_entity_nav_target_drive_coord(
                    target,
                    &self.substrate.entities,
                ) else {
                    return false;
                };
                coord
            }
        };
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
            return false;
        };
        // Drive4B2180..2194 / Ship6A17C3..17D7 query OWNER +4C for Z.
        // The cleared head makes ordinary +4C resolve to the placed owner;
        // NavCom contributes only the horizontal cell test.
        let owner = position_world_coord(&entity.position);
        cell(coord) == cell(owner)
            && owner.z.wrapping_sub(destination.z).wrapping_abs()
                < 2 * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS
    }

    /// Bounded Unit738970 receiver. Existing idle selectors cover ordinary
    /// human vehicles/miners; deploy/radio/AI arms remain receiver residuals.
    fn track_enter_idle_mode(&mut self, id: u64, rules: Option<&RuleSet>) -> bool {
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
                entity.attack_target = None;
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

    fn track_per_cell(
        &mut self,
        id: u64,
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
        // Unit PerCell2 739EC0: after MCV retry, +6D1==0 admits
        // Ready(+200)73ACC2 -> Commence(+1EC)73ACD1, BEFORE full-cell
        // crush73B089 and Foot sensor/playfield tail73B0A0. The existing
        // miner.unload_active owns +6D1 (ctor7353FE, unload73DFDA).
        // This promotes only: it does not dispatch a mission handler or
        // repeat the object AI prefix. mission_host_promote retains its
        // documented unavailable locomotor/height fallback for other inputs.
        let promote = self.substrate.entities.get(id).is_some_and(|entity| {
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
                self.session.tick as u32,
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
        if let Some(rules) = rules {
            self.refresh_unit_sensor_at_per_cell(id, rules);
            crate::sim::world::techno_ai_cloak::uncloak_on_sensor_neighbour_after_cell_entry(
                self, id, rules,
            );
        }
        self.promote_entity_playfield_membership_after_move(id);
    }
}

#[cfg(test)]
#[path = "track_host_tests.rs"]
mod tests;
