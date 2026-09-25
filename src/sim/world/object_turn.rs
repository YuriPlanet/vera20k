//! One live object turn: AI, locomotor tails and synchronous cell/lifecycle effects.
//!
//! The master frame owns phase order. This owner completes one object's effects
//! before the live Logic cursor advances; no lifecycle work is deferred to a
//! batch tail. Native order and coordinate evidence remain beside each seam.

use std::collections::BTreeSet;

use super::{Simulation, techno_ai};
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::lifecycle_request::LifecycleRequest;
use crate::sim::movement::{
    self, homing_movement, parachute_descent, rocket_movement, teleport_movement,
};
use crate::sim::pathfinding::PathGrid;

/// Whether this Unit visit reaches FootClass's SHP body-counter cadence.
///
/// An entry-active TubeMovement owns the UnitClass AI call and returns before
/// FootClass AI. Tube state armed later during an ordinary Foot visit does not
/// retroactively suppress work already reached by that visit, so only the
/// entry snapshot belongs in this admission predicate.
pub(super) fn shp_vehicle_counter_admitted(tube_active_at_entry: bool) -> bool {
    !tube_active_at_entry
}

/// The warp's two VocClass::PlayAt calls (`0x0071962C` at the old location,
/// `0x00719710` at the new): the type's ChronoOutSound/ChronoInSound
/// (TechnoType+0x578/+0x574), else `[AudioVisual]` (Rules+0x21C/+0x218), else
/// silence.
fn teleport_warp_sounds(
    sim: &mut Simulation,
    stable_id: u64,
    departure: Option<(u16, u16)>,
    rules: &RuleSet,
) {
    let Some(entity) = sim.substrate.entities.get(stable_id) else {
        return;
    };
    let arrival = (entity.position.rx, entity.position.ry);
    let object = sim.object_type(entity.type_ref(), rules);
    let chrono_out = object
        .and_then(|object| object.chrono_out_sound.clone())
        .or_else(|| rules.general.chrono_out_sound.clone());
    let chrono_in = object
        .and_then(|object| object.chrono_in_sound.clone())
        .or_else(|| rules.general.chrono_in_sound.clone());
    for (name, (rx, ry)) in [
        (chrono_out, departure.unwrap_or(arrival)),
        (chrono_in, arrival),
    ] {
        if let Some(name) = name {
            let sound_id = sim.interner.intern(&name);
            sim.sound_events
                .push(super::SimSoundEvent::ChronoTeleport { sound_id, rx, ry });
        }
    }
}

#[cfg(test)]
#[path = "track_object_turn_tests.rs"]
mod track_object_turn_tests;

#[cfg(test)]
#[path = "forced_track_object_turn_tests.rs"]
mod forced_track_object_turn_tests;

#[cfg(test)]
#[path = "teleport_anim_object_turn_tests.rs"]
mod teleport_anim_object_turn_tests;

#[derive(Default)]
pub(super) struct LiveObjectPassOutcome {
    pub movement: movement::MovementTickStats,
    pub destroyed_structure: bool,
    pub bridge_state_changed: bool,
    pub tube_turn_owned_ids: BTreeSet<u64>,
}

#[derive(Default)]
pub(super) struct GroundLocomotorOutcome {
    pub(super) movement: movement::MovementTickStats,
    pub(super) bridge_state_changed: bool,
    track_owned: bool,
}

/// Re-enter the pending movement pass for the same mover of this Process (see
/// `MoverReentry`), with the Simulation's current grids and state.
fn reenter_pending_pass(
    sim: &mut Simulation,
    pending: &mut movement::movement_tick::PendingMovementPass,
    reentry: movement::movement_tick::MoverReentry,
    rules: Option<&RuleSet>,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    timing: movement::MovementConfig,
) {
    let current_grid = sim.path_grid_snapshot();
    pending.reenter_mover(
        reentry,
        &mut sim.substrate.entities,
        current_grid.as_deref().or(path_grid),
        sim.zone_grid.as_ref(),
        sim.resolved_terrain.as_ref(),
        &sim.terrain_costs,
        &sim.house_alliances,
        &mut sim.substrate.occupancy,
        &mut sim.substrate.cell_occupation,
        &mut sim.substrate.raw_cell_occupation,
        &mut sim.substrate.next_occupancy_enter_order,
        &mut sim.scenario_rng,
        sim.session.tick,
        sim.session.binary_frame,
        sim.overlay_grid.as_ref(),
        overlay_registry,
        sim.playfield_bounds,
        &sim.terrain_speed_config,
        timing.close_enough,
        timing.path_delay_ticks,
        timing.blockage_path_delay_ticks,
        &mut sim.interner,
        rules,
        Some(&sim.type_handles),
        Some(&sim.production.slave_bindings),
        &mut sim.movement_pass_cache,
        &sim.houses,
    );
}

#[derive(Default)]
pub(super) struct ObjectTurnOutcome {
    movement: movement::MovementTickStats,
    destroyed_structure: bool,
    bridge_state_changed: bool,
    tube_owned: bool,
}

impl Simulation {
    /// Component-based movement fixtures enter the same Process corridor as
    /// live object turns. This exposes no alternate physics or callback loop.
    #[cfg(test)]
    pub(crate) fn process_ground_locomotor_for_test(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        grid: Option<&PathGrid>,
        registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> Result<movement::MovementTickStats, super::FrameAdvanceError> {
        self.process_ground_locomotor_one(id, rules, grid, registry)
            .map(|outcome| outcome.movement)
    }

    /// The ordinary ground locomotor Process corridor, without Object/Techno AI.
    /// Infantry Scatter51D478 calls the active locomotor synchronously; its
    /// PerCell and boundary receivers must finish before Scatter returns.
    pub(super) fn process_ground_locomotor_one(
        &mut self,
        stable_id: u64,
        rules: Option<&RuleSet>,
        path_grid: Option<&PathGrid>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> Result<GroundLocomotorOutcome, super::FrameAdvanceError> {
        let timing = movement::MovementConfig::from_rules(self.session.binary_frame, rules);
        self.process_ground_locomotor_with_config(
            stable_id,
            rules,
            path_grid,
            overlay_registry,
            timing,
        )
    }

    #[cfg(test)]
    pub(crate) fn process_ground_locomotor_with_config_for_test(
        &mut self,
        stable_id: u64,
        rules: Option<&RuleSet>,
        path_grid: Option<&PathGrid>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        timing: movement::MovementConfig,
    ) -> Result<movement::MovementTickStats, super::FrameAdvanceError> {
        self.process_ground_locomotor_with_config(
            stable_id,
            rules,
            path_grid,
            overlay_registry,
            timing,
        )
        .map(|outcome| outcome.movement)
    }

    fn process_ground_locomotor_with_config(
        &mut self,
        stable_id: u64,
        rules: Option<&RuleSet>,
        path_grid: Option<&PathGrid>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        timing: movement::MovementConfig,
    ) -> Result<GroundLocomotorOutcome, super::FrameAdvanceError> {
        let sim = self;
        let one = [stable_id];
        let mut outcome = GroundLocomotorOutcome::default();
        // Grid-less component fixtures keep the pass's empty-arrival cleanup;
        // the pass searches with the Simulation grid or the caller's.
        if path_grid.is_some() || sim.path_grid.is_some() {
            sim.complete_pending_track_order(stable_id, rules);
        }
        let movement_before = sim.substrate.entities.get(stable_id).map(|entity| {
            (
                (entity.position.rx, entity.position.ry),
                entity.movement_target.is_some(),
                entity.low_bridge_tube_state.is_some(),
                entity.locomotor.as_ref().map(|loco| loco.active_kind()),
            )
        });
        // Drive4B050B..0557 / Ship69FC1B..FC67 samples the containing
        // cell slope before any active-track, destination or turn return.
        // Entry-active Tube owns its whole visit and does not call Process.
        if let Some(entity) = sim.substrate.entities.get_mut(stable_id)
            && entity.low_bridge_tube_state.is_none()
            && let Some(slope) = sim.resolved_terrain.as_ref().and_then(|terrain| {
                terrain
                    .cell(entity.position.rx, entity.position.ry)
                    .map(|cell| cell.slope_type)
            })
        {
            movement::slope_transition::sample_process_entry(
                entity,
                slope,
                sim.session.binary_frame,
            );
        }
        if !sim.process_track_turn(stable_id, rules, path_grid) {
            return Ok(outcome);
        }
        let mut pending_movement = {
            let current_grid = sim.path_grid_snapshot();
            movement::movement_tick::begin_movement_with_grids_scoped(
                &mut sim.substrate.entities,
                Some(&one),
                current_grid.as_deref().or(path_grid),
                &sim.terrain_costs,
                &sim.house_alliances,
                &mut sim.substrate.occupancy,
                &mut sim.substrate.cell_occupation,
                &mut sim.substrate.raw_cell_occupation,
                &mut sim.substrate.next_occupancy_enter_order,
                &mut sim.scenario_rng,
                sim.session.tick,
                sim.session.binary_frame,
                sim.zone_grid.as_ref(),
                sim.resolved_terrain.as_ref(),
                sim.overlay_grid.as_ref(),
                overlay_registry,
                sim.playfield_bounds,
                &sim.terrain_speed_config,
                timing.close_enough,
                timing.path_delay_ticks,
                timing.blockage_path_delay_ticks,
                &mut sim.interner,
                rules,
                Some(&sim.type_handles),
                Some(&sim.production.slave_bindings),
                &mut sim.movement_pass_cache,
                &sim.houses,
            )
            .map_err(|cause| super::FrameAdvanceError {
                tick: sim.session.tick,
                binary_frame: sim.session.binary_frame,
                entity_id: stable_id,
                cause,
            })?
        };
        // Process_Movement's no-queue request and the Process_Track after it.
        // When the active-track Process_Track(0) ends its track, the same
        // Process continues into Process_Movement and Process_Track(1)
        // (Drive 0x4B0583..0x4B0667 / Ship 0x69FC93..0x69FD0E;
        // `movement::track_continuation`).
        let frame_error = |sim: &Simulation, cause| super::FrameAdvanceError {
            tick: sim.session.tick,
            binary_frame: sim.session.binary_frame,
            entity_id: stable_id,
            cause,
        };
        let mut retry = false;
        loop {
            if let Some(request) = pending_movement.take_foot_path_request() {
                let lent = pending_movement.lent_block_set(request.owner());
                let outcome = sim
                    .run_foot_path_request(&request, lent, rules, path_grid, overlay_registry)
                    .map_err(|cause| frame_error(sim, cause))?;
                if outcome == movement::FootPathOutcome::Resume {
                    reenter_pending_pass(
                        sim,
                        &mut pending_movement,
                        movement::movement_tick::MoverReentry::FootPath(Box::new(request)),
                        rules,
                        path_grid,
                        overlay_registry,
                        timing,
                    );
                }
            }
            // Drive4B0A79 / Ship6A0142 and the track-end continuation
            // 4B0647 / 69FCEE: Process_Movement(&out, 1, 0); Process_Track
            // follows unless the out byte is set or the Foot died
            // (4B0A7E..4B0AAA, 4B064C..4B0667).
            if let Some((id, family)) = pending_movement.take_track_movement() {
                let rules = rules.ok_or_else(|| {
                    frame_error(sim, "Drive/Ship Process_Movement requires rules".into())
                })?;
                let out = sim
                    .run_track_process_movement(
                        id,
                        family,
                        movement::ProcessMovementArgs::OUTER,
                        rules,
                        path_grid,
                        overlay_registry,
                    )
                    .map_err(|cause| frame_error(sim, cause))?;
                if !out
                    && sim
                        .substrate
                        .entities
                        .get(id)
                        .is_some_and(|entity| entity.lifecycle.object_alive)
                {
                    pending_movement.record_native_track(
                        movement::track_process::TrackInvocation::after_process_movement(
                            id, family,
                        ),
                    );
                }
            }
            let Some(invocation) = pending_movement
                .take_native_track()
                // Ordinary Process reloads Object+90 after the fresh receiver.
                .filter(|invocation| {
                    sim.substrate
                        .entities
                        .get(invocation.entity_id)
                        .is_some_and(|entity| entity.lifecycle.object_alive)
                })
            else {
                break;
            };
            let invocation = movement::track_process::TrackInvocation {
                retry,
                ..invocation
            };
            let pass = sim
                .run_track_process(invocation, rules, path_grid, overlay_registry)
                .map_err(|cause| frame_error(sim, cause))?;
            pending_movement.record_track_movement(pass.moved);
            outcome.track_owned = true;
            if retry || !invocation.active_gate || pass.aborted {
                break;
            }
            if !sim
                .begin_track_end_continuation(invocation.entity_id, invocation.family, rules)
                .map_err(|cause| frame_error(sim, cause))?
            {
                break;
            }
            reenter_pending_pass(
                sim,
                &mut pending_movement,
                movement::movement_tick::MoverReentry::AfterTrackEnd(invocation.entity_id),
                rules,
                path_grid,
                overlay_registry,
                timing,
            );
            retry = true;
        }
        if let Some((id, head)) = pending_movement.take_walk_per_cell() {
            outcome.bridge_state_changed |=
                sim.run_completed_walk_step(id, head, rules, path_grid, overlay_registry)?;
            pending_movement.retain_walk_completion(id, &sim.substrate.entities);
        }
        if let Some((id, coord)) = pending_movement.take_walk_boundary() {
            sim.run_walk_boundary(id, coord, rules, path_grid, overlay_registry);
            pending_movement.record_track_movement(1);
        }

        outcome
            .movement
            .merge(movement::movement_tick::finish_movement_pass(
                pending_movement,
                &mut sim.substrate.entities,
                &sim.house_alliances,
                &mut sim.substrate.cell_occupation,
                sim.session.tick,
                sim.session.binary_frame,
                sim.resolved_terrain.as_ref(),
                sim.path_grid.as_deref().or(path_grid),
                &mut sim.interner,
                rules,
                &mut sim.sound_events,
                &mut sim.pending_lifecycle_requests,
                &mut sim.movement_pass_cache,
            ));
        if let Some((old_cell, had_target, tube_active, kind)) = movement_before {
            let per_cell = sim.substrate.entities.get(stable_id).is_some_and(|entity| {
                // Tube exits Unit73603F / Infantry51BA9B and Hover arrival5146CA / cell-entry
                // 515A1C. The existing Hover integrator still approximates
                // native crossing timing; its accepted entries use this owner.
                (tube_active && entity.low_bridge_tube_state.is_none())
                    || (kind == Some(crate::rules::locomotor_type::LocomotorKind::Hover)
                        && (old_cell != (entity.position.rx, entity.position.ry)
                            || (had_target && entity.movement_target.is_none())))
            });
            if per_cell {
                sim.foot_neighbors_at_per_cell(stable_id);
            }
        }
        Ok(outcome)
    }
    pub(super) fn advance_live_object_pass(
        &mut self,
        rules: Option<&RuleSet>,
        path_grid: Option<&PathGrid>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> Result<LiveObjectPassOutcome, super::FrameAdvanceError> {
        let miner_config = rules.map(crate::sim::miner::MinerConfig::from_rules);
        let terrain_spawner_cells = self
            .production
            .terrain_spawners
            .keys()
            .copied()
            .collect::<BTreeSet<_>>();
        let object_ctx = techno_ai::ObjectAiCtx {
            path_grid,
            overlay_registry,
            terrain_spawner_cells: Some(&terrain_spawner_cells),
            miner_config: miner_config.as_ref(),
        };

        let mut outcome = LiveObjectPassOutcome::default();
        self.try_for_each_live_object::<super::FrameAdvanceError>(|sim, stable_id| {
            let turn = sim.advance_live_object_turn(stable_id, rules, object_ctx)?;
            outcome.movement.merge(turn.movement);
            outcome.destroyed_structure |= turn.destroyed_structure;
            outcome.bridge_state_changed |= turn.bridge_state_changed;
            if turn.tube_owned {
                outcome.tube_turn_owned_ids.insert(stable_id);
            }
            Ok(())
        })?;
        Ok(outcome)
    }

    pub(super) fn advance_live_object_turn(
        &mut self,
        stable_id: u64,
        rules: Option<&RuleSet>,
        object_ctx: techno_ai::ObjectAiCtx<'_>,
    ) -> Result<ObjectTurnOutcome, super::FrameAdvanceError> {
        let sim = self;
        let path_grid = object_ctx.path_grid;
        let overlay_registry = object_ctx.overlay_registry;
        let mut outcome = ObjectTurnOutcome::default();
        // UnitClass::AI / InfantryClass::AI give an active TubeMovement
        // object the whole live-object turn.  Capture before the leaf:
        // successful finalization clears the payload but must still skip
        // every ordinary locomotor tail and the second mission checkpoint.
        let tube_active_at_entry = sim.substrate.entities.get(stable_id).is_some_and(|entity| {
            !entity.dying
                && matches!(
                    entity.category,
                    EntityCategory::Unit | EntityCategory::Infantry
                )
                && entity.low_bridge_tube_state.is_some()
        });
        let was_structure = sim
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category == EntityCategory::Structure);
        sim.object_ai_visit_one(stable_id, rules, object_ctx);
        if was_structure
            && sim
                .substrate
                .entities
                .get(stable_id)
                .is_none_or(|entity| entity.dying)
        {
            outcome.destroyed_structure = true;
        }
        if sim.substrate.entities.get(stable_id).is_none_or(|entity| {
            entity.dying
                || (!tube_active_at_entry
                    && matches!(
                        entity.category,
                        EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
                    )
                    && !entity.lifecycle.object_alive)
        }) {
            // Foot4DA53E..548 reloads Object+90 after TechnoAI and returns
            // before locomotor Process when false. Health and the Rust dying
            // state are independent: even Process's slope prelude must wait
            // until this live owner admission succeeds. Entry-active Tube AI
            // bypasses Foot AI and therefore does not reach this predicate.
            // Evidence: tools/spatial_oracle/foot_enter_idle, foot_ai_reset rows.
            return Ok(outcome);
        }
        // A temporal-warped object's leaf AI returned before its locomotor
        // Process (Unit 0x007362B5..0x007362CB, Infantry and Aircraft alike;
        // `Simulation::temporal_ai_prologue`).
        if sim
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(crate::sim::game_entity::GameEntity::ai_frozen)
        {
            return Ok(outcome);
        }

        if !tube_active_at_entry {
            sim.refresh_high_flying_sight_before_process(stable_id, rules, path_grid);
        }

        let before_movement = sim.movement_sound_probe(stable_id);
        let cell_before_movement = sim
            .substrate
            .entities
            .get(stable_id)
            .map(|entity| (entity.position.rx, entity.position.ry));
        let walk_process_owned = sim
            .substrate
            .entities
            .get(stable_id)
            .and_then(|e| e.locomotor.as_ref())
            .is_some_and(|l| l.kind == crate::rules::locomotor_type::LocomotorKind::Walk);
        let one = [stable_id];
        let ground =
            sim.process_ground_locomotor_one(stable_id, rules, path_grid, overlay_registry)?;
        let track_owned = ground.track_owned;
        outcome.movement.merge(ground.movement);
        outcome.bridge_state_changed |= ground.bridge_state_changed;
        // Synchronous turn/arrival callbacks may convert or remove the owner.
        if sim
            .substrate
            .entities
            .get(stable_id)
            .is_none_or(|e| e.dying || !e.lifecycle.object_alive)
        {
            return Ok(outcome);
        }

        // FootClass advances the SHP Unit body counter immediately after
        // this object's locomotor Process, against the still-current
        // absolute binary frame. The global frame commits only after the
        // complete live-object pass.
        if shp_vehicle_counter_admitted(tube_active_at_entry) {
            let shp_vehicle_cadence = sim.substrate.entities.get(stable_id).and_then(|entity| {
                if entity.category != EntityCategory::Unit || entity.is_voxel {
                    return None;
                }
                let object = rules?.object(sim.interner.resolve(entity.type_ref()))?;
                Some(crate::sim::animation::ShpVehicleCadence {
                    walk_rate: object.walk_rate,
                    idle_rate: object.idle_rate,
                })
            });
            if let (Some(cadence), Some(entity)) = (
                shp_vehicle_cadence,
                sim.substrate.entities.get_mut(stable_id),
            ) {
                crate::sim::animation::tick_shp_vehicle_body_frame_counter(
                    entity,
                    cadence,
                    sim.session.binary_frame,
                );
            }
        }

        // A direction-8 producer also ends this object's ordinary turn as
        // soon as it arms TubeMovement.  The leaf itself starts on the
        // object's next visit; an entry-active leaf may have cleared the
        // payload above, hence the captured half of this predicate.
        let tube_owns_whole_turn = tube_active_at_entry
            || sim.substrate.entities.get(stable_id).is_some_and(|entity| {
                matches!(
                    entity.category,
                    EntityCategory::Unit | EntityCategory::Infantry
                ) && entity.low_bridge_tube_state.is_some()
            });
        if tube_owns_whole_turn {
            outcome.tube_owned = true;
            return Ok(outcome);
        }

        let air = sim.tick_air_movement_with_cell_lists_one(stable_id, rules);
        if air.impact {
            // The impact UnInits the object; `FootClass::AI` returns on the
            // cleared Object+90 (`0x004DA87E`) and the class AI after it
            // (`AircraftClass::AI 0x00414DAA`).
            let jumpjet = sim.substrate.entities.get(stable_id).is_some_and(|entity| {
                entity.locomotor.as_ref().is_some_and(|locomotor| {
                    locomotor.active_kind() == crate::rules::locomotor_type::LocomotorKind::Jumpjet
                })
            });
            match rules {
                Some(rules) if jumpjet => {
                    sim.jumpjet_crash_impact(stable_id, rules, overlay_registry);
                }
                Some(rules) => sim.fly_crash_impact(stable_id, rules, overlay_registry),
                None => sim.uninit(stable_id),
            }
            return Ok(outcome);
        }
        let teleport_armed = sim
            .substrate
            .entities
            .get(stable_id)
            .and_then(|entity| entity.teleport_state.as_ref())
            .is_some_and(|state| {
                state.phase == crate::sim::movement::teleport_movement::TeleportPhase::Relocate
            });
        // Teleport Process 0x007197AF: already on the destination, no warp.
        let teleport_reached = sim.substrate.entities.get(stable_id).is_some_and(|entity| {
            teleport_movement::warp_destination_reached(entity, sim.resolved_terrain.as_ref())
        });
        let teleport_relocating = teleport_armed && !teleport_reached;
        // Teleport Process 0x007195BF..0x007195CF: the warp step ejects a
        // parasite (ExitUnit, no suppression) before the relocation.
        if teleport_relocating
            && let Some(rules) = rules
            && let Some(eater) = sim
                .substrate
                .entities
                .get(stable_id)
                .and_then(|entity| entity.parasite_eating_me)
        {
            sim.parasite_exit_unit(eater, rules);
        }
        if let Some(rules) = rules {
            let warp_out_type = sim.interner.intern(&rules.general.warp_out.name);
            let mut warp_spawns = Vec::new();
            let mut teleport_visuals = teleport_movement::TeleportVisuals {
                anim_spawns: &mut warp_spawns,
                warp_out_type,
            };
            teleport_movement::tick_teleport_movement(
                &mut sim.substrate.entities,
                &mut sim.substrate.occupancy,
                &one,
                sim.session.tick,
                sim.resolved_terrain.as_ref(),
                Some(&mut teleport_visuals),
            );
            // RESIDUAL: the destination WarpOut is built here with the source
            // one; native builds it after the arrival's Per_Cell_Process(2),
            // Stop_Moving, crate pickup and NULL assign (`0x00719742`). Only
            // an arrival step that draws RNG (a crush's death animation)
            // could see the order.
            for descriptor in warp_spawns {
                let type_name = descriptor.type_name;
                if let Err(error) = sim.spawn_anim_object(rules, descriptor) {
                    // An art type that never bound draws nothing natively
                    // either; see `spawn_combat_explosion_anim`.
                    log::debug!(
                        "teleport warp [{}] did not construct: {error}",
                        sim.interner.resolve(type_name)
                    );
                }
            }
        } else {
            teleport_movement::tick_teleport_movement(
                &mut sim.substrate.entities,
                &mut sim.substrate.occupancy,
                &one,
                sim.session.tick,
                sim.resolved_terrain.as_ref(),
                None,
            );
        }
        // Teleport Process's warp step (`0x007192F0`), after the relocation.
        let unit_teleport = sim
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category == EntityCategory::Unit);
        let unit_warp_arrival = teleport_relocating && unit_teleport;
        if teleport_reached && unit_teleport {
            // 0x007197B4: vt+0x480(NULL, 1) before Stop_Moving.
            sim.set_unit_null_destination(stable_id, rules);
        }
        if teleport_relocating {
            if let Some(rules) = rules {
                teleport_warp_sounds(sim, stable_id, cell_before_movement, rules);
            }
            if unit_warp_arrival {
                // 0x0071971C: vt+0x18C(2) is UnitClass::Per_Cell_Process
                // (0x00739EC0), with the Foot body and its playfield tail.
                sim.unit_per_cell_process_arrival(stable_id, rules);
                // 0x00719725 Stop_Moving: the tick retired the request.
                // 0x0071972E CellClass::PickupCrate (`0x00481A00`): the crate
                // receiver every mover still lacks (`movement::track_fresh`
                // residuals).
                // 0x0071973C: vt+0x480(NULL, 1). A Teleporter still in radio
                // contact gets a Drive here, which the FootClass::AI tail below
                // ends again.
                sim.set_unit_null_destination(stable_id, rules);
            } else {
                // Relocation71971C calls PerCell(2), including a same-cell
                // relocation; ordinary Fly motion has no such call.
                sim.foot_neighbors_at_per_cell(stable_id);
                // PerCell(2)'s tail `0x006F5090` lets a held Temporal target go
                // (a Chrono Legionnaire teleporting away from its victim).
                sim.temporal_release_if_warping(stable_id);
            }
        }
        let rocket_arrivals = rocket_movement::tick_rocket_movement(
            &mut sim.substrate.entities,
            &one,
            sim.session.tick,
        );
        let rocket_arrived = !rocket_arrivals.is_empty();
        sim.pending_rocket_detonations.extend(rocket_arrivals);
        // `ILoco::Process 0x00662FD5..0x00662FE1`: after its flight step a
        // missile left with no Health explodes where it is and is UnInit, so
        // `FootClass::AI` and `AircraftClass::AI` stop here.
        let rocket_died = !rocket_arrived
            && sim.substrate.entities.get(stable_id).is_some_and(|entity| {
                entity.health.current <= 0
                    && entity.rocket_state.is_some()
                    && entity.locomotor.as_ref().is_some_and(|locomotor| {
                        locomotor.active_kind()
                            == crate::rules::locomotor_type::LocomotorKind::Rocket
                    })
            });
        if rocket_died {
            crate::sim::spawn_manager::detonate_dead_missile(sim, stable_id);
            return Ok(outcome);
        }
        sim.tick_tunnel_locomotor_one(stable_id, path_grid);
        sim.tick_drop_pod_locomotor_one(stable_id, path_grid);
        let _ = homing_movement::tick_homing_movement(
            &mut sim.substrate.entities,
            &one,
            sim.session.tick,
        );
        if let Some(rules) = rules {
            let falling = |sim: &Simulation| {
                sim.substrate
                    .entities
                    .get(stable_id)
                    .is_some_and(|entity| entity.parachute_state.is_some())
            };
            let was_falling = falling(sim);
            parachute_descent::tick_parachute_descent_in_order(
                &mut sim.substrate.entities,
                &one,
                rules.general.parachute_max_fall_rate,
                sim.session.tick,
            );
            if was_falling && !falling(sim) {
                // Object AI5F3F8D: grounded fall completion precedes the
                // parachute animation's wind-down.
                sim.foot_neighbors_at_per_cell(stable_id);
                sim.wind_down_parachute_anim(rules, stable_id);
            }
        }
        movement::tick_locomotor_piggyback_restore_one(&mut sim.substrate.entities, stable_id);
        // FootClass::AI tail 0x004DAEE1..0x004DAEF3, after the piggyback swap:
        // an infected Foot runs its eater's ParasiteClass AI in its own turn.
        if let Some(rules) = rules {
            sim.parasite_ai_for_victim(stable_id, rules, overlay_registry);
        }

        let cell_after_movement = sim
            .substrate
            .entities
            .get(stable_id)
            .map(|entity| (entity.position.rx, entity.position.ry));
        if !track_owned
            && !walk_process_owned
            && !unit_warp_arrival
            && let Some(rules) = rules
        {
            sim.move_unit_sensor_after_cell_change(
                stable_id,
                cell_before_movement,
                cell_after_movement,
                rules,
            );
        }
        if unit_warp_arrival {
            // The Unit Per_Cell_Process above ran the Foot body.
        } else if teleport_relocating {
            // The warp's PerCell(2) ends in the `0x006F5090` playfield tail,
            // which only promotes. (The exact outside clear at `0x00719A99`
            // belongs to the Chronosphere warp states, not this step.)
            sim.promote_entity_playfield_membership_after_move(stable_id);
        } else if !track_owned && !walk_process_owned && cell_before_movement != cell_after_movement
        {
            // `FootClass::PerCellProcess @ 0x004D85D0` runs the `Sensors=`
            // neighbour scan on its cell-enter arm, after the sensor
            // deposit has moved (`0x004D8611`/`0x004D8621`, issued just
            // above) and before its `FUN_006F5090` playfield-membership
            // tail — which is the promote below.
            if let Some(rules) = rules {
                crate::sim::world::techno_ai_cloak::uncloak_on_sensor_neighbour_after_cell_entry(
                    sim, stable_id, rules,
                );
            }
            // `0x006F5090`'s head (`0x006F50A3..0x006F50B4`): entering a cell
            // lets a held Temporal target go.
            sim.temporal_release_if_warping(stable_id);
            sim.promote_entity_playfield_membership_after_move(stable_id);
        }

        let mut lifecycle_requests = std::mem::take(&mut sim.pending_lifecycle_requests);
        for request in lifecycle_requests.drain(..) {
            let LifecycleRequest::Uninit { stable_id, .. } = request;
            sim.release_move_sound(stable_id);
            if let Some(rules) = rules {
                sim.apply_lifecycle_request_with_rules(request, rules);
            } else {
                sim.apply_lifecycle_request(request);
            }
        }
        debug_assert!(lifecycle_requests.is_empty());
        sim.pending_lifecycle_requests = lifecycle_requests;

        sim.tick_move_sound_after_process(stable_id, before_movement, rules);
        if let Some(rules) = rules {
            sim.crash_edge_sounds(stable_id, rules);
        }
        // UnitClass::AI after FootClass::AI, before its second Ready/Commence.
        crate::sim::miner::miner_system::unit_ai_clear_harvesting(sim, stable_id);
        sim.object_ai_post_movement_promote_one(stable_id, rules);
        if let Some(rules) = rules {
            sim.aircraft_crash_smoke(stable_id, rules);
        }
        Ok(outcome)
    }
}
