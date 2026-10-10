//! One live object turn: AI, locomotor tails and synchronous cell/lifecycle effects.
//!
//! The master frame owns phase order. This owner completes one object's effects
//! before the live Logic cursor advances; no lifecycle work is deferred to a
//! batch tail. Native order and coordinate evidence remain beside each seam.

use std::collections::BTreeSet;

use super::{Simulation, techno_ai};
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::{self, teleport_movement};

/// Whether this Unit visit reaches FootClass's SHP body-counter cadence.
///
/// An entry-active TubeMovement owns the UnitClass AI call and returns before
/// FootClass AI. Tube state armed later during an ordinary Foot visit does not
/// retroactively suppress work already reached by that visit, so only the
/// entry snapshot belongs in this admission predicate.
pub(super) fn unit_body_counter_admitted(tube_active_at_entry: bool) -> bool {
    !tube_active_at_entry
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

#[cfg(test)]
#[path = "unit_fire_turn_tests.rs"]
mod unit_fire_turn_tests;

#[cfg(test)]
#[path = "per_cell_object_turn_tests.rs"]
mod per_cell_object_turn_tests;

#[derive(Default)]
pub(super) struct LiveObjectPassOutcome {
    pub movement: movement::MovementTickStats,
    pub destroyed_structure: bool,
    pub bridge_state_changed: bool,
    pub tube_turn_owned_ids: BTreeSet<u64>,
    ownership_changed: bool,
}

impl LiveObjectPassOutcome {
    pub(super) fn ownership_changed(&self) -> bool {
        self.ownership_changed
    }
}

#[derive(Default)]
pub(crate) struct GroundLocomotorOutcome {
    pub(super) movement: movement::MovementTickStats,
    pub(super) bridge_state_changed: bool,
    track_owned: bool,
    /// The tube-exit arrival ran `Per_Cell_Process(2)`.
    per_cell_ran: bool,
}

impl GroundLocomotorOutcome {
    /// Whether the Process changed bridge state.
    pub(crate) fn bridge_state_changed(&self) -> bool {
        self.bridge_state_changed
    }
}

#[derive(Default)]
pub(super) struct ObjectTurnOutcome {
    movement: movement::MovementTickStats,
    destroyed_structure: bool,
    bridge_state_changed: bool,
    tube_owned: bool,
    ownership_changed: bool,
}

/// What one object's locomotor Process did this turn.
#[derive(Default)]
struct LocomotorProcess {
    /// FootClass::AI admitted the Process (`0x004DA806..0x004DA877`).
    admitted: bool,
    movement: movement::MovementTickStats,
    bridge_state_changed: bool,
    track_owned: bool,
    /// The Process ran `Per_Cell_Process(2)`: a Hover, tube-exit, touchdown
    /// or warp arrival.
    per_cell_ran: bool,
    /// The Process UnInit its owner: an aircraft impact or a dead missile.
    ended: bool,
}

impl LocomotorProcess {
    fn from_ground(ground: GroundLocomotorOutcome) -> Self {
        Self {
            admitted: true,
            movement: ground.movement,
            bridge_state_changed: ground.bridge_state_changed,
            track_owned: ground.track_owned,
            per_cell_ran: ground.per_cell_ran,
            ended: false,
        }
    }

    fn admitted() -> Self {
        Self {
            admitted: true,
            ..Self::default()
        }
    }
}

impl Simulation {
    /// InfantryAI51BCB2..51BDCF, after the complete FootAI visit: an idle
    /// Infantry standing in an ordinary Building scatters through the one
    /// Infantry51D0D0 owner. This is also the first no-rally barracks exit.
    ///
    /// Original joined construction controls reach51BDC9 on frames269/486;
    /// Scatter's51D385 draw precedes its synchronous Walk Process51D478.
    /// Evidence: basic-factory-output-idle-prerequisite-research, original
    /// construction/no-rally and planning-header controls (gamemd1cdd1180).
    fn scatter_idle_infantry_from_building(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Result<bool, String> {
        use crate::map::cell_index::NativeCellIdentity;
        use crate::sim::mission::MissionType;
        use crate::sim::movement::{ScatterFlags, locomotor::MovementLayer};

        let Some(infantry) = self.substrate.entities.get(stable_id) else {
            return Ok(false);
        };
        if infantry.category != EntityCategory::Infantry
            || !infantry.lifecycle.object_alive
            || infantry.lifecycle.in_limbo
            || !matches!(
                infantry.mission.effective().known(),
                Some(MissionType::Guard | MissionType::AreaGuard)
            )
        {
            return Ok(false);
        }
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return Ok(false);
        };
        // The caller reads physical Object+9C, not virtual+4C navigation/bridge XY.
        let coord = movement::ground_pose::position_world_coord(&infantry.position);
        let cell = terrain.native_cell_identity(((coord.x / 256) as i16, (coord.y / 256) as i16));
        let NativeCellIdentity::Real(index) = cell else {
            return Ok(false);
        };
        let c = &terrain.cells()[index];
        let Some(building_id) =
            self.substrate
                .occupancy
                .first_building_on_layer(c.rx, c.ry, MovementLayer::Ground)
        else {
            return Ok(false);
        };
        let Some(building) = self.substrate.entities.get(building_id) else {
            return Err("InfantryAI ground Building has a retired identity".into());
        };
        let Some(object) = self.object_type(building.type_ref(), rules) else {
            return Err("InfantryAI ground Building has no type".into());
        };
        if object.invisible_in_game || (object.gate && building.is_open_gate()) {
            return Ok(false);
        }
        // RESIDUAL: specialized fence state. This prefix compares LaserFence+618
        // with8/12 and reads its owner's Firestorm+1FA. Their runtime
        // owners are absent. Such types retain the prior no-Scatter behavior
        // until those separate mechanisms establish the required state.
        // Stock GAPILE and all ordinary barracks have both type flags false.
        if object.laser_fence || object.firestorm_wall {
            return Ok(false);
        }
        let slaves = crate::sim::slave_deposit::SlaveDepositQuery {
            entities: &self.substrate.entities,
            occupancy: &self.substrate.occupancy,
            terrain,
            rules,
            interner: &self.interner,
        };
        // SlaveOwner+2DC, its manager+2D8 and this ground
        // Building must match before the shared6B0880 deposit predicate.
        if slaves.master(stable_id) == Some(building_id) {
            let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
            let queried = movement::ground_pose::query_object_cell(&cells, coord);
            if slaves.admits(stable_id, building_id, queried) {
                return Ok(false);
            }
        }
        self.scatter_null(stable_id, ScatterFlags::new(true, true), rules, registry)
    }

    /// FootClass::AI's one call of the active locomotor's Process
    /// (`0x004DA877`, `ILocomotion` vtable `+0x40`).
    ///
    /// - Admission (`0x004DA806..0x004DA86E`): a locomotor (Foot `+0x674`),
    ///   not sinking (`+0x3CD`), not falling (`+0x8D`) and not in limbo
    ///   (`+0x81`). The DirectRocker link test (`+0x2A8` against TechnoType
    ///   `+0x692`) is dormant: no retail warhead sets `DirectRocker=`, and
    ///   VERA keeps no link.
    /// - The active class picks the one Process: the ground corridor for
    ///   Drive, Ship and Walk, the air pass for Fly and Jumpjet, and Hover's,
    ///   Teleport's and Rocket's own.
    fn process_active_locomotor(
        &mut self,
        stable_id: u64,
        rules: Option<&RuleSet>,
        ctx: techno_ai::ObjectAiCtx<'_>,
    ) -> Result<LocomotorProcess, super::FrameAdvanceError> {
        let overlay_registry = ctx.overlay_registry;
        use crate::rules::locomotor_type::LocomotorKind;
        let (admitted, sinking) =
            self.substrate
                .entities
                .get(stable_id)
                .map_or((None, false), |entity| {
                    let sinking = entity.sinking.is_active();
                    let admitted = entity.locomotor.as_ref().and_then(|locomotor| {
                        (!sinking && !entity.is_falling_down() && !entity.lifecycle.in_limbo)
                            .then(|| locomotor.active_kind())
                    });
                    (admitted, sinking)
                });
        let ground = matches!(
            admitted,
            Some(LocomotorKind::Drive | LocomotorKind::Ship | LocomotorKind::Walk)
        );
        // The vehicle plane is a projection each object turn reconciles once
        // at Process entry: the ground corridor in `prepare_movement_visit`,
        // any other turn here. A sinking object keeps its projection.
        if !ground
            && !sinking
            && let Some(entity) = self.substrate.entities.get(stable_id)
        {
            self.substrate
                .cell_occupation
                .reconcile_entity(entity, &self.substrate.occupancy);
        }
        let Some(kind) = admitted else {
            return Ok(LocomotorProcess::default());
        };
        match kind {
            LocomotorKind::Drive | LocomotorKind::Ship | LocomotorKind::Walk => self
                .process_ground_locomotor_one(stable_id, rules, overlay_registry)
                .map(LocomotorProcess::from_ground),
            LocomotorKind::Hover => {
                self.complete_pending_order(stable_id, rules, overlay_registry);
                self.process_hover_locomotor(stable_id, rules, overlay_registry)
                    .map(|hover| LocomotorProcess {
                        admitted: true,
                        bridge_state_changed: hover.bridge_state_changed,
                        per_cell_ran: hover.per_cell_ran,
                        ..LocomotorProcess::default()
                    })
            }
            LocomotorKind::Fly | LocomotorKind::Jumpjet => {
                self.process_air_locomotor(stable_id, rules, overlay_registry)
            }
            LocomotorKind::Teleport => self.process_teleport_locomotor(stable_id, rules, ctx),
            LocomotorKind::Rocket => {
                Ok(self.process_rocket_locomotor(stable_id, rules, overlay_registry))
            }
        }
    }

    /// Fly and Jumpjet Process: the air pass inside the Fly cell-list
    /// transaction, then a touchdown's `Per_Cell_Process(2)` or an impact.
    fn process_air_locomotor(
        &mut self,
        stable_id: u64,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Result<LocomotorProcess, super::FrameAdvanceError> {
        let mut process = LocomotorProcess::admitted();
        self.complete_pending_order(stable_id, rules, overlay_registry);
        let jumpjet = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                entity.locomotor.as_ref().is_some_and(|locomotor| {
                    locomotor.active_kind() == crate::rules::locomotor_type::LocomotorKind::Jumpjet
                })
            });
        let air = self.tick_air_movement_with_cell_lists_one(stable_id, rules, overlay_registry);
        let jumpjet_unit_touchdown = jumpjet
            && self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| entity.category == EntityCategory::Unit);
        if air.touched_down && jumpjet_unit_touchdown {
            // Jumpjet54C8F0/54C8FF now run Unit PerCell2 and the class NULL
            // setter synchronously while the installed instance is phase4.
            process.per_cell_ran = true;
        } else if air.touched_down {
            process.bridge_state_changed |= self.per_cell_process(
                stable_id,
                movement::PerCellReason::Arrival,
                rules,
                overlay_registry,
            )?;
            process.per_cell_ran = true;
            if jumpjet {
                //54C8CB..54C8EA already cleared the locomotor destination and
                //moving byte;54C8FF calls the class NULL setter after PerCell.
                self.assign_null_destination(stable_id, rules, overlay_registry);
            }
        }
        if !air.impact {
            return Ok(process);
        }
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return Ok(process);
        };
        if entity.category == EntityCategory::Infantry {
            // The Infantry notice keeps the infantryman: its class AI runs on
            // to the sequencer and locomotion actions.
            if let Some(rules) = rules {
                self.aircraft_tracker_remove(stable_id);
                self.infantry_crash_impact(stable_id, rules, overlay_registry);
            }
            return Ok(process);
        }
        // The impact UnInits the object; `FootClass::AI` returns on the
        // cleared Object+90 (`0x004DA87E`) and the class AI after it
        // (`AircraftClass::AI 0x00414DAA`).
        match rules {
            Some(rules) if jumpjet => {
                process.bridge_state_changed |=
                    self.jumpjet_crash_impact(stable_id, rules, overlay_registry);
            }
            Some(rules) => {
                process.bridge_state_changed |=
                    self.fly_crash_impact(stable_id, rules, overlay_registry);
            }
            None => self.uninit(stable_id),
        }
        process.ended = true;
        Ok(process)
    }

    /// Teleport Process (`0x007192F0`): the warp and its arrival. A
    /// Chronosphere warp (the latch, or a state past 0) takes the whole
    /// Process (`0x00719351..0x00719361`, `movement::teleport_chrono`). Its
    /// arrival runs its own `Per_Cell_Process(2)` (state 5), so the cell
    /// change of its landing (state 4) takes no stand-in.
    fn process_teleport_locomotor(
        &mut self,
        stable_id: u64,
        rules: Option<&RuleSet>,
        ctx: techno_ai::ObjectAiCtx<'_>,
    ) -> Result<LocomotorProcess, super::FrameAdvanceError> {
        let overlay_registry = ctx.overlay_registry;
        if let Some(rules) = rules
            && self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| entity.active_chrono_warp().is_some())
        {
            let bridge_state_changed = self.process_chrono_warp(stable_id, rules, ctx)?;
            return Ok(LocomotorProcess {
                admitted: true,
                bridge_state_changed,
                per_cell_ran: true,
                ..LocomotorProcess::default()
            });
        }
        self.complete_pending_order(stable_id, rules, overlay_registry);
        let sim = self;
        let mut process = LocomotorProcess::admitted();
        let teleport_armed = sim
            .substrate
            .entities
            .get(stable_id)
            .and_then(|entity| entity.teleport_state())
            .is_some_and(|state| state.phase() == teleport_movement::TeleportPhase::Relocate);
        // Teleport Process 0x007197AF: already on the destination, no warp.
        let teleport_reached = sim.substrate.entities.get(stable_id).is_some_and(|entity| {
            teleport_movement::warp_destination_reached(entity, sim.resolved_terrain.as_ref())
        });
        let teleport_relocating = teleport_armed && !teleport_reached;
        if teleport_relocating {
            // 0x007193C7: the Techno detach sweep drops the locks aimed at
            // the owner; 0x00719442: the departure WarpOut at its Location.
            teleport_movement::release_incoming_target_locks(
                &mut sim.substrate.entities,
                stable_id,
            );
            if let Some(rules) = rules {
                sim.teleport_warp_out(stable_id, rules);
            }
            // 0x007195BF..0x007195CF: a parasite is ejected (ExitUnit, no
            // suppression).
            if let Some(rules) = rules
                && let Some(eater) = sim
                    .substrate
                    .entities
                    .get(stable_id)
                    .and_then(|entity| entity.parasite_eating_me)
            {
                sim.parasite_exit_unit(eater, rules);
            }
            // `0x007195DB` Mark(UP) and `0x007196BF` Mark(DOWN) bracket the
            // relocation: the cell list entry, and a Unit's raw 0x20 and
            // vehicle plane, leave the old cell and join the new one on its
            // OnBridge layer. An infantryman's sub-cell bit stays: his Mark
            // reaches no receiver, and the window calls none of its own.
            // ChronoOut plays at the old Location (`0x0071962C`).
            sim.foot_mark_remove(stable_id, rules, overlay_registry);
            if let Some(rules) = rules {
                sim.teleport_warp_sound(stable_id, teleport_movement::WarpSound::Out, rules);
            }
        }
        teleport_movement::process_teleport(
            &mut sim.substrate.entities,
            stable_id,
            sim.session.tick,
            sim.resolved_terrain.as_ref(),
        );
        if teleport_relocating {
            sim.foot_mark_put(stable_id, rules, overlay_registry);
        }
        // Teleport Process's warp step (`0x007192F0`), after the relocation.
        // Its `vt+0x480(NULL, 1)` is the owner's class setter
        // (`Simulation::assign_null_destination`), which reaches the
        // Teleport's own Stop_Moving through Foot's null arm.
        if teleport_reached {
            // 0x007197B4: vt+0x480(NULL, 1) before Stop_Moving.
            sim.assign_null_destination(stable_id, rules, overlay_registry);
        }
        if teleport_relocating {
            // 0x00719710: ChronoIn at the new Location.
            if let Some(rules) = rules {
                sim.teleport_warp_sound(stable_id, teleport_movement::WarpSound::In, rules);
            }
            // Relocation 0x0071971C calls vt+0x18C(2), including a same-cell
            // relocation; ordinary Fly motion has no such call.
            process.bridge_state_changed |= sim.per_cell_process(
                stable_id,
                movement::PerCellReason::Arrival,
                rules,
                overlay_registry,
            )?;
            process.per_cell_ran = true;
            // 0x00719725 Stop_Moving: the tick retired the request.
            // 0x0071972E CellClass::PickupCrate (`0x00481A00`) on the
            // destination cell for the linked Foot; its answer is not read.
            let cell = sim
                .substrate
                .entities
                .get(stable_id)
                .map(|entity| (entity.position.rx as i16, entity.position.ry as i16));
            if let Some(cell) = cell {
                let _ = sim.pickup_crate_at(stable_id, cell, rules, overlay_registry);
            }
            // 0x0071973C: vt+0x480(NULL, 1). A Teleporter still in radio
            // contact gets a Drive here, which the FootClass::AI tail ends
            // again.
            sim.assign_null_destination(stable_id, rules, overlay_registry);
            // 0x00719742..0x00719791: the arrival WarpOut at the Location.
            if let Some(rules) = rules {
                sim.teleport_warp_out(stable_id, rules);
            }
        }
        Ok(process)
    }

    /// Rocket Process (`0x006622C0`, [`Simulation::process_rocket`]). A
    /// detonation UnInits its owner, which ends `FootClass::AI` and
    /// `AircraftClass::AI` (`0x004DA87A`). RulesClass is always present
    /// natively; a rules-less fixture has no rocket block to fly by.
    fn process_rocket_locomotor(
        &mut self,
        stable_id: u64,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> LocomotorProcess {
        let mut process = LocomotorProcess::admitted();
        let Some(rules) = rules else {
            return process;
        };
        let outcome = self.process_rocket(stable_id, rules, overlay_registry);
        process.bridge_state_changed = outcome.bridge_state_changed;
        process.ended = !self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.is_object_alive());
        process
    }
}

impl Simulation {
    /// Component-based movement fixtures enter the same Process corridor as
    /// live object turns. This exposes no alternate physics or callback loop.
    #[cfg(test)]
    pub(crate) fn process_ground_locomotor_for_test(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        grid: Option<&crate::sim::pathfinding::PathGrid>,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Result<movement::MovementTickStats, super::FrameAdvanceError> {
        self.install_fixture_path_grid(grid);
        self.process_ground_locomotor_one(id, rules, registry)
            .map(|outcome| outcome.movement)
    }

    /// The ordinary ground locomotor Process corridor, without Object/Techno AI.
    /// Infantry Scatter51D478 calls the active locomotor synchronously; its
    /// PerCell and boundary receivers must finish before Scatter returns.
    #[cfg(test)]
    pub(crate) fn process_ground_locomotor_stats_for_test(
        &mut self,
        stable_id: u64,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Result<movement::MovementTickStats, super::FrameAdvanceError> {
        self.process_ground_locomotor_one(stable_id, rules, overlay_registry)
            .map(|outcome| outcome.movement)
    }

    pub(crate) fn process_ground_locomotor_one(
        &mut self,
        stable_id: u64,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Result<GroundLocomotorOutcome, super::FrameAdvanceError> {
        let sim = self;
        let mut outcome = GroundLocomotorOutcome::default();
        sim.complete_pending_order(stable_id, rules, overlay_registry);
        let movement_before = sim
            .substrate
            .entities
            .get(stable_id)
            .map(|entity| entity.low_bridge_tube_state.is_some());
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
        if !sim.process_track_turn(stable_id, rules, overlay_registry) {
            return Ok(outcome);
        }
        let mut pending_movement = {
            let current_grid = sim.path_grid_snapshot();
            movement::movement_tick::begin_movement(
                &mut sim.substrate.entities,
                stable_id,
                current_grid.as_deref(),
                &sim.house_alliances,
                &mut sim.substrate.occupancy,
                &mut sim.substrate.cell_occupation,
                &mut sim.substrate.raw_cell_occupation,
                &mut sim.scenario_rng,
                sim.session.binary_frame,
                sim.resolved_terrain.as_ref(),
                sim.playfield_bounds,
                &mut sim.interner,
                rules,
                Some(&sim.type_handles),
                &sim.houses,
                &mut sim.movement_pass_cache,
            )
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
            // The Scatter calls a step queued while the pass held the world
            // (the tube exit), in call order.
            outcome.bridge_state_changed |= sim
                .run_scatter_requests(
                    pending_movement.take_scatter_requests(),
                    rules,
                    overlay_registry,
                )
                .map_err(|cause| frame_error(sim, cause))?;
            if let Some(request) = pending_movement.take_foot_path_request() {
                let held = Some(pending_movement.held_block_sets());
                let outcome = sim
                    .run_foot_path_request(&request, held, rules, overlay_registry)
                    .map_err(|cause| frame_error(sim, cause))?;
                if outcome == movement::FootPathOutcome::Resume {
                    pending_movement.reenter_mover(
                        sim,
                        movement::movement_tick::MoverReentry::FootPath(Box::new(request)),
                        rules,
                    );
                }
            }
            if let Some(request) = pending_movement.take_walk_admission_request() {
                let retry = sim
                    .run_walk_admission_request(
                        request,
                        Some(pending_movement.held_block_sets()),
                        rules,
                        overlay_registry,
                    )
                    .map_err(|cause| frame_error(sim, cause))?;
                if let Some(request) = retry {
                    //75B716/75BABB/75BC04: one recursive Process(0),
                    //with the already prepared mover visit retained.
                    pending_movement.request_foot_path(request);
                    continue;
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
                        Some(pending_movement.held_block_sets()),
                        rules,
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
                .run_track_process(invocation, rules, overlay_registry)
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
            pending_movement.reenter_mover(
                sim,
                movement::movement_tick::MoverReentry::AfterTrackEnd(invocation.entity_id),
                rules,
            );
            retry = true;
        }
        outcome.bridge_state_changed |= sim
            .run_scatter_requests(
                pending_movement.take_scatter_requests(),
                rules,
                overlay_registry,
            )
            .map_err(|cause| frame_error(sim, cause))?;
        if let Some((id, head)) = pending_movement.take_walk_per_cell() {
            outcome.bridge_state_changed |=
                sim.run_completed_walk_step(id, head, rules, overlay_registry)?;
            pending_movement.retain_walk_completion(id, &sim.substrate.entities);
        }
        if let Some((id, coord)) = pending_movement.take_walk_boundary() {
            sim.run_walk_boundary(id, coord, rules, overlay_registry);
            pending_movement.record_track_movement(1);
        }

        outcome
            .movement
            .merge(movement::movement_tick::finish_movement_pass(
                pending_movement,
                &mut sim.substrate.entities,
                &mut sim.movement_pass_cache,
            ));
        if let Some(tube_active) = movement_before {
            // Tube exits Unit73603F / Infantry51BA9B call vt+0x18C(2).
            let per_cell = sim
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| tube_active && entity.low_bridge_tube_state.is_none());
            if per_cell {
                outcome.bridge_state_changed |= sim.per_cell_process(
                    stable_id,
                    movement::PerCellReason::Arrival,
                    rules,
                    overlay_registry,
                )?;
                outcome.per_cell_ran = true;
            }
        }
        Ok(outcome)
    }
    pub(super) fn advance_live_object_pass(
        &mut self,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Result<LiveObjectPassOutcome, super::FrameAdvanceError> {
        let miner_config = rules.map(crate::sim::miner::MinerConfig::from_rules);
        let terrain_spawner_cells = self
            .production
            .terrain_animations
            .keys()
            .copied()
            .collect::<BTreeSet<_>>();
        let object_ctx = techno_ai::ObjectAiCtx {
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
            outcome.ownership_changed |= turn.ownership_changed;
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
        let ai = sim.object_ai_visit_one_with_effects(stable_id, rules, object_ctx);
        outcome.bridge_state_changed |= ai.bridge_state_changed;
        outcome.ownership_changed |= ai.ownership_changed();
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

        // Foot4DA54E follows the post-TechnoAI Object+90 gate above.
        // An entry-active Tube bypasses FootAI; it must retain the byte.
        if !tube_active_at_entry
            && let Some(entity) = sim.substrate.entities.get_mut(stable_id)
            && matches!(
                entity.category,
                EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
            )
        {
            entity.mission_leaf.set_foot_idle_entry_latch(0);
        }

        // ObjectClass::AI's fall step (`0x005F3F11..0x005F3FA4`), which the
        // Foot's TechnoClass::AI call (`0x004DA539`) reaches before Process.
        //
        // RESIDUAL: VERA runs it after the whole Techno visit, not at
        // ObjectClass::AI's start. Trigger: a paradropped object landing.
        // Effect: that frame's mission dispatch still sees it falling.
        // Frequency: once per paradrop landing. Risk: the first mission
        // Commence after landing waits one frame.
        let mut per_cell_ran = false;
        if let Some(rules) = rules
            && sim.advance_fall(
                stable_id,
                rules.general.parachute_max_fall_rate,
                Some(rules),
                overlay_registry,
            )
        {
            // Object AI5F3F8D: grounded fall completion calls vt+0x18C(2)
            // before the parachute animation's wind-down.
            outcome.bridge_state_changed |= sim.per_cell_process(
                stable_id,
                movement::PerCellReason::Arrival,
                Some(rules),
                overlay_registry,
            )?;
            per_cell_ran = true;
            sim.wind_down_parachute_anim(rules, stable_id);
        }

        if !tube_active_at_entry {
            sim.refresh_high_flying_sight_before_process(stable_id, rules);
        }

        // Foot4DA80C captures +538 before locomotor Process. The sound tail
        // compares this same counter after its canonical cadence owner runs.
        let Some(body_frame_before_process) = sim
            .substrate
            .entities
            .get(stable_id)
            .map(|entity| entity.body_frame_counter)
        else {
            return Ok(outcome);
        };
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
        // An entry-active TubeMovement leaf runs through the ground corridor
        // in place of the Foot AI and its Process.
        let process = if tube_active_at_entry {
            let ground = sim.process_ground_locomotor_one(stable_id, rules, overlay_registry)?;
            LocomotorProcess {
                admitted: false,
                ..LocomotorProcess::from_ground(ground)
            }
        } else {
            sim.process_active_locomotor(stable_id, rules, object_ctx)?
        };
        let track_owned = process.track_owned;
        per_cell_ran |= process.per_cell_ran;
        outcome.movement.merge(process.movement);
        outcome.bridge_state_changed |= process.bridge_state_changed;
        // `0x004DA87A`: the Process may have converted or removed the owner.
        if process.ended
            || sim
                .substrate
                .entities
                .get(stable_id)
                .is_none_or(|e| e.dying || !e.lifecycle.object_alive)
        {
            return Ok(outcome);
        }

        // FootClass advances the Unit body counter immediately after
        // this object's locomotor Process, against the still-current
        // absolute binary frame, inside the Process admission
        // (`0x004DA81A` bypasses both). The global frame commits only after
        // the complete live-object pass.
        if process.admitted && unit_body_counter_admitted(tube_active_at_entry) {
            let unit_body_cadence = sim.substrate.entities.get(stable_id).and_then(|entity| {
                if entity.category != EntityCategory::Unit {
                    return None;
                }
                let object = rules?.object(sim.interner.resolve(entity.type_ref()))?;
                Some((
                    crate::sim::animation::ShpVehicleCadence {
                        walk_rate: object.walk_rate,
                        idle_rate: object.idle_rate,
                    },
                    object.hover_attack,
                    object.deploy_to_land,
                ))
            });
            if let (Some((cadence, hover_attack, deploy_to_land)), Some(entity)) =
                (unit_body_cadence, sim.substrate.entities.get_mut(stable_id))
            {
                crate::sim::animation::tick_unit_body_frame_counter(
                    entity,
                    rules.map(|rules| {
                        crate::sim::movement::SpeedRules::new(
                            rules,
                            &sim.interner,
                            &sim.type_handles,
                            &sim.houses,
                        )
                    }),
                    cadence,
                    hover_attack,
                    deploy_to_land,
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

        movement::tick_locomotor_piggyback_restore_one(&mut sim.substrate.entities, stable_id);
        // FootClass::AI 0x004DAED0..0x004DAEDC: with vt+0x1D8 (the warp-in)
        // clear, the pending entry (`Unit+0x500`) asks for its slot.
        if let Some(rules) = rules
            && sim
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| !entity.is_warping_in() && entity.pending_entry().is_some())
        {
            crate::sim::docking::building_dock::try_pending_entry(sim, rules, stable_id);
        }
        // FootClass::AI tail 0x004DAEE1..0x004DAEF3, after the piggyback swap:
        // an infected Foot runs its eater's ParasiteClass AI in its own turn.
        if let Some(rules) = rules {
            sim.parasite_ai_for_victim(stable_id, rules, overlay_registry);
        }

        // Unit73649C follows the complete Foot tail. Its disguise pick must
        // follow Foot's own Scenario draws and precede Unit's firing suffix.
        if let Some(rules) = rules {
            sim.update_unit_disguise(stable_id, rules)
                .map_err(|cause| super::FrameAdvanceError {
                    tick: sim.session.tick,
                    binary_frame: sim.session.binary_frame,
                    entity_id: stable_id,
                    cause,
                })?;
        }

        let cell_after_movement = sim
            .substrate
            .entities
            .get(stable_id)
            .map(|entity| (entity.position.rx, entity.position.ry));
        // A mover whose Process has no ported `Per_Cell_Process` call (Fly,
        // a cruising Jumpjet, Rocket) takes
        // VERA's cell-change stand-in for the Foot body's sensor, uncloak,
        // Temporal and promote steps (`movement/per_cell.rs`).
        //
        // RESIDUAL: native Fly and Jumpjet cruise reach none of these from a
        // cell change. Trigger: such a mover changing cell. Effect: its
        // sensor deposit, cloak scan, Temporal release and (but for a Fly,
        // whose Process latches `+0x3D5` itself, `0x004CD510`) playfield
        // promote run there. Risk: an aircraft with `Sensors=` or a held
        // Temporal target; the promote is what admits a Jumpjet arriving
        // from off the map.
        if !track_owned
            && !walk_process_owned
            && !per_cell_ran
            && cell_before_movement != cell_after_movement
        {
            if let Some(rules) = rules {
                sim.refresh_unit_sensor_at_per_cell(stable_id, rules);
                crate::sim::world::techno_ai_cloak::uncloak_on_sensor_neighbour_after_cell_entry(
                    sim, stable_id, rules,
                );
            }
            sim.temporal_release_if_warping(stable_id);
            let fly = sim.substrate.entities.get(stable_id).is_some_and(|entity| {
                entity
                    .locomotor
                    .as_ref()
                    .is_some_and(|locomotor| locomotor.fly_runtime().is_some())
            });
            if !fly {
                sim.promote_entity_playfield_membership_after_move(stable_id);
            }
        }

        let mut lifecycle_requests = std::mem::take(&mut sim.pending_lifecycle_requests);
        for request in lifecycle_requests.drain(..) {
            if let Some(rules) = rules {
                sim.apply_lifecycle_request_with_rules(request, rules, overlay_registry);
            } else {
                sim.apply_lifecycle_request(request);
            }
        }
        debug_assert!(lifecycle_requests.is_empty());
        sim.pending_lifecycle_requests = lifecycle_requests;

        sim.tick_move_sound_after_process(stable_id, body_frame_before_process, rules);
        if let Some(rules) = rules {
            sim.sinking_edge_sounds(stable_id, rules);
            sim.crash_edge_sounds(stable_id, rules);
            if sim.tick_ship_sinking(stable_id, rules, overlay_registry) {
                return Ok(outcome);
            }
        }
        // Infantry51BDE7..51BF80 follows the Foot tail: rate-zero fire
        // cancellation, its second Ready/Commence, Fear, firing51BF59,
        // sequencer51BF6A and locomotion actions51BF7B. Inline FireAt
        // consequences join the dynamic Logic suffix before its next slot.
        let infantry = sim
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category == EntityCategory::Infantry);
        if let (Some(rules), true) = (rules, infantry) {
            //51BCA4..51BDCF follows the entire FootAI, including Process and
            // its tails. A successful Scatter invokes Process a second time
            // synchronously, before the Infantry Ready/Commence/fire suffix.
            outcome.bridge_state_changed |= sim
                .scatter_idle_infantry_from_building(stable_id, rules, overlay_registry)
                .map_err(|cause| super::FrameAdvanceError {
                    tick: sim.session.tick,
                    binary_frame: sim.session.binary_frame,
                    entity_id: stable_id,
                    cause,
                })?;
            outcome.bridge_state_changed |= sim
                .infantry_action_turn(stable_id, rules, overlay_registry)
                .map_err(|cause| super::FrameAdvanceError {
                    tick: sim.session.tick,
                    binary_frame: sim.session.binary_frame,
                    entity_id: stable_id,
                    cause,
                })?;
            if sim
                .substrate
                .entities
                .get(stable_id)
                .is_none_or(|actor| !actor.is_ai_alive())
            {
                return Ok(outcome);
            }
        }
        // AircraftClass::AI after FootClass::AI: past the map's edge the
        // aircraft may be removed (`0x00414F47..0x00414FDE`), ending its AI.
        if let Some(rules) = rules
            && sim.remove_aircraft_off_map(stable_id, rules, overlay_registry)
        {
            return Ok(outcome);
        }
        // UnitClass::AI after FootClass::AI, before its second Ready/Commence.
        crate::sim::miner::miner_system::unit_ai_clear_harvesting(sim, stable_id);
        // Unit7365E1 Fire_At_Target then7365E8 Facing_Update precede
        // its second Ready/Commence. FireAt appends into the live Logic walk.
        if !infantry {
            if let Some(rules) = rules {
                let receipt = sim.commit_fire_visit(
                    crate::sim::combat::world_receiver::FireVisit::UnitTarget(stable_id),
                    rules,
                    overlay_registry,
                );
                outcome.bridge_state_changed |= receipt.bridge_state_changed;
                outcome.destroyed_structure |= receipt.structure_destroyed;
            }
            sim.object_ai_post_movement_promote_one(stable_id, rules);
        }
        if let Some(rules) = rules {
            sim.aircraft_crash_smoke(stable_id, rules);
        }
        Ok(outcome)
    }
}
