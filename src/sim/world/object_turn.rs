//! One live object turn: AI, locomotor tails and synchronous cell/lifecycle effects.
//!
//! The master frame owns phase order. This owner completes one object's effects
//! before the live Logic cursor advances; no lifecycle work is deferred to a
//! batch tail. Native order and coordinate evidence remain beside each seam.

use super::{Simulation, techno_ai};
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::{self, teleport_movement};

/// Whether this Foot visit reaches the shared body-counter cadence.
///
/// An entry-active TubeMovement owns the UnitClass AI call and returns before
/// FootClass AI. Tube state armed later during an ordinary Foot visit does not
/// retroactively suppress work already reached by that visit, so only the
/// entry snapshot belongs in this admission predicate.
pub(super) fn foot_body_counter_admitted(tube_active_at_entry: bool) -> bool {
    !tube_active_at_entry
}

/// `UnitClass::AI` (`0x007360C0`) and `InfantryClass::AI` (`0x0051BAB0`)
/// give an object holding a tube (`Foot+0x684`) the whole turn
/// ([`Simulation::tube_object_turn`]).
fn tube_active(entity: &crate::sim::game_entity::GameEntity) -> bool {
    !entity.death.is_dying()
        && matches!(
            entity.category(),
            EntityCategory::Unit | EntityCategory::Infantry
        )
        && entity.low_bridge_tube_state.is_some()
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
#[path = "per_cell_object_turn_tests.rs"]
mod per_cell_object_turn_tests;

#[cfg(test)]
#[path = "airborne_cell_tag_tests.rs"]
mod airborne_cell_tag_tests;

#[cfg(test)]
#[path = "periodic_cell_object_turn_tests.rs"]
mod periodic_cell_object_turn_tests;

#[cfg(test)]
#[path = "object_turn_lookup_tests.rs"]
mod object_turn_lookup_tests;

#[derive(Default)]
pub(super) struct LiveObjectPassOutcome {
    pub destroyed_structure: bool,
    pub bridge_state_changed: bool,
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
}

#[derive(Default)]
pub(super) struct ObjectTurnOutcome {
    movement: movement::MovementTickStats,
    destroyed_structure: bool,
    bridge_state_changed: bool,
    ownership_changed: bool,
}

/// What one object's locomotor Process did this turn.
#[derive(Default)]
pub(crate) struct LocomotorProcess {
    /// FootClass::AI admitted the Process (`0x004DA806..0x004DA877`).
    admitted: bool,
    movement: movement::MovementTickStats,
    bridge_state_changed: bool,
    /// The Process ran `Per_Cell_Process(2)`: a Hover, tube-exit, touchdown
    /// or warp arrival.
    per_cell_ran: bool,
    /// The Process UnInit its owner: an aircraft impact or a dead missile.
    ended: bool,
}

impl LocomotorProcess {
    pub(crate) fn bridge_state_changed(&self) -> bool {
        self.bridge_state_changed
    }
    fn from_ground(ground: GroundLocomotorOutcome) -> Self {
        Self {
            admitted: true,
            movement: ground.movement,
            bridge_state_changed: ground.bridge_state_changed,
            per_cell_ran: false,
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
    /// construction/no-rally and planning-header controls (gamemd).
    fn scatter_idle_infantry_from_building(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: super::FrameEffects<'_>,
    ) -> Result<bool, String> {
        use crate::map::cell_index::NativeCellIdentity;
        use crate::sim::mission::MissionType;
        use crate::sim::movement::{ScatterFlags, locomotor::MovementLayer};

        let Some(infantry) = self.substrate.entities.get(stable_id) else {
            return Ok(false);
        };
        if infantry.category() != EntityCategory::Infantry
            || !infantry.lifecycle.object_alive()
            || infantry.lifecycle.in_limbo()
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
        // Ares 3.0 InfantryClass_Update 10014DB0 (hook 0x51BD4C): an `IsPassable`
        // building is ignored (0x51BD7D); a `Firestorm.Wall` one takes the
        // stock FirestormWall branch (0x51BD56) in place of `FirestormWall=`:
        // only while its owner's firestorm is up does the man scatter.
        #[cfg(feature = "ares")]
        if object.building_type_ext().is_passable {
            return Ok(false);
        }
        #[cfg(feature = "ares")]
        if object.building_type_ext().firestorm_wall {
            if !self.ares_firestorm.is_active(building.owner()) {
                return Ok(false);
            }
        } else if object.laser_fence {
            return Ok(false);
        }
        // RESIDUAL: specialized fence state. This prefix compares LaserFence+618
        // with8/12 and reads its owner's Firestorm+1FA. Their runtime
        // owners are absent. Such types retain the prior no-Scatter behavior
        // until those separate mechanisms establish the required state.
        // Stock GAPILE and all ordinary barracks have both type flags false.
        // (Ares builds own the firestorm arm above; the hook leaves the stock
        // `FirestormWall=` flag unread.)
        #[cfg(not(feature = "ares"))]
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
        self.scatter(
            stable_id,
            None,
            ScatterFlags::new(true, true),
            rules,
            registry,
            effects,
        )
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
    ///   Teleport's, Rocket's and Tunnel's own.
    pub(crate) fn process_active_locomotor(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        ctx: techno_ai::ObjectAiCtx<'_>,
    ) -> Result<LocomotorProcess, super::FrameAdvanceError> {
        let _area = crate::sim::frame_profile::area(crate::sim::frame_profile::phase::MOVEMENT);
        let overlay_registry = ctx.overlay_registry;
        use crate::rules::locomotor_type::LocomotorKind;
        // A quiet unit is not falling (`world::quiet_units`).
        let quiet = self.quiet(stable_id);
        let admitted = self.substrate.entities.get(stable_id).and_then(|entity| {
            let sinking = entity.sinking.is_active();
            entity.locomotor.as_ref().and_then(|locomotor| {
                (!sinking && (quiet || !entity.is_falling_down()) && !entity.lifecycle.in_limbo())
                    .then(|| locomotor.active_kind())
            })
        });
        let Some(kind) = admitted else {
            return Ok(LocomotorProcess::default());
        };
        // Ares TechnoExt::CreateWithDroppod (10044030) installs the DropPod
        // locomotor over the Foot's own: its Process (`0x004B5B70`) is the
        // one this call makes until the pod lands.
        #[cfg(feature = "ares")]
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.ares_drop_pod().is_some())
        {
            self.process_drop_pod_locomotor(stable_id, rules, overlay_registry, ctx.effects);
            return Ok(LocomotorProcess::admitted());
        }
        match kind {
            LocomotorKind::Drive | LocomotorKind::Ship | LocomotorKind::Walk => self
                .process_ground_locomotor_one(stable_id, rules, overlay_registry, ctx.effects)
                .map(LocomotorProcess::from_ground),
            LocomotorKind::Hover => {
                self.complete_pending_order(stable_id, rules, overlay_registry, ctx.effects);
                self.process_hover_locomotor(stable_id, rules, overlay_registry, ctx.effects)
                    .map(|hover| LocomotorProcess {
                        admitted: true,
                        bridge_state_changed: hover.bridge_state_changed,
                        per_cell_ran: hover.per_cell_ran,
                        ..LocomotorProcess::default()
                    })
            }
            LocomotorKind::Fly | LocomotorKind::Jumpjet => {
                self.process_air_locomotor(stable_id, rules, overlay_registry, ctx.effects)
            }
            LocomotorKind::Teleport => self.process_teleport_locomotor(stable_id, rules, ctx),
            LocomotorKind::Rocket => {
                Ok(self.process_rocket_locomotor(stable_id, rules, overlay_registry, ctx.effects))
            }
            LocomotorKind::Tunnel => {
                self.complete_pending_order(stable_id, rules, overlay_registry, ctx.effects);
                let tunnel =
                    self.process_tunnel_locomotor(stable_id, rules, overlay_registry, ctx.effects);
                Ok(LocomotorProcess {
                    admitted: true,
                    bridge_state_changed: tunnel.bridge_state_changed,
                    per_cell_ran: tunnel.per_cell_ran,
                    ..LocomotorProcess::default()
                })
            }
        }
    }

    /// Fly and Jumpjet Process: the air pass inside the Fly cell-list
    /// transaction, then a touchdown's `Per_Cell_Process(2)` or an impact.
    fn process_air_locomotor(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: super::FrameEffects<'_>,
    ) -> Result<LocomotorProcess, super::FrameAdvanceError> {
        let mut process = LocomotorProcess::admitted();
        self.complete_pending_order(stable_id, rules, overlay_registry, effects);
        let jumpjet = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                entity.locomotor.as_ref().is_some_and(|locomotor| {
                    locomotor.active_kind() == crate::rules::locomotor_type::LocomotorKind::Jumpjet
                })
            });
        let air = self.tick_air_movement_with_cell_lists_one(
            stable_id,
            rules,
            overlay_registry,
            effects,
        )?;
        process.bridge_state_changed |= air.bridge_state_changed;
        if air.touched_down && jumpjet {
            // Jumpjet54C8F0/54C8FF run the class PerCell2 and NULL
            // setter synchronously while the installed instance is phase4.
            process.per_cell_ran = true;
        } else if air.touched_down {
            process.bridge_state_changed |= self.per_cell_process(
                stable_id,
                movement::PerCellReason::Arrival,
                rules,
                overlay_registry,
                effects,
            )?;
            process.per_cell_ran = true;
        }
        if !air.impact {
            return Ok(process);
        }
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return Ok(process);
        };
        if entity.category() == EntityCategory::Infantry {
            // The Infantry notice keeps the infantryman: its class AI runs on
            // to the sequencer and locomotion actions.
            self.substrate.aircraft_tracker_remove(stable_id);
            self.infantry_crash_impact(stable_id, rules, overlay_registry, effects);
            return Ok(process);
        }
        // The impact UnInits the object; `FootClass::AI` returns on the
        // cleared Object+90 (`0x004DA87E`) and the class AI after it
        // (`AircraftClass::AI 0x00414DAA`).
        process.bridge_state_changed |= if jumpjet {
            self.jumpjet_crash_impact(stable_id, rules, overlay_registry, effects)
        } else {
            self.fly_crash_impact(stable_id, rules, overlay_registry, effects)
        };
        process.ended = true;
        Ok(process)
    }

    /// Teleport Process (`0x007192F0`): the warp and its arrival. A
    /// Chronosphere warp (the latch, or a state past 0) takes the whole
    /// Process (`0x00719351..0x00719361`, `movement::teleport_chrono`). Its
    /// arrival runs its own `Per_Cell_Process(2)` in state 5; the coordinate
    /// change in state 4 supplies no additional callback.
    fn process_teleport_locomotor(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        ctx: techno_ai::ObjectAiCtx<'_>,
    ) -> Result<LocomotorProcess, super::FrameAdvanceError> {
        let overlay_registry = ctx.overlay_registry;
        if self
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
        self.complete_pending_order(stable_id, rules, overlay_registry, ctx.effects);
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
            // `0x007193C7`: the detach sweep (`0x0070D4A0`) with the owner,
            // so every Techno aiming at it restores its mission and takes the
            // class NULL target setter.
            sim.stop_all_targeting_on_detach(stable_id, rules, overlay_registry, ctx.effects);
            // 7193CC..7193F4: walk the Bullet array backwards before WarpOut,
            // Mark(UP), or relocation changes the target's original pose.
            sim.lose_projectile_targets(stable_id, rules);
            // `0x00719442`: the departure WarpOut at its Location.
            sim.teleport_warp_out(stable_id, rules);
            // 0x007195BF..0x007195CF: a parasite is ejected (ExitUnit, no
            // suppression).
            if let Some(eater) = sim
                .substrate
                .entities
                .get(stable_id)
                .and_then(|entity| entity.parasite.eater())
            {
                sim.parasite_exit_unit(eater, rules, ctx.effects);
            }
            // `0x007195DB` Mark(UP) and `0x007196BF` Mark(DOWN) bracket the
            // relocation: the cell list entry, and a Unit's raw 0x20, leave
            // the old cell and join the new one on its OnBridge layer. An
            // infantryman's sub-cell bit stays: his Mark reaches no receiver,
            // and the window calls none of its own.
            // ChronoOut plays at the old Location (`0x0071962C`).
            sim.foot_mark_remove(stable_id, rules, overlay_registry, ctx.effects);
            Self::teleport_warp_sound(
                &mut sim.interner,
                &sim.substrate,
                sim.resolved_terrain.as_ref(),
                &mut sim.sound_events,
                stable_id,
                teleport_movement::WarpSound::Out,
                rules,
            );
        }
        sim.process_teleport(stable_id, rules, ctx);
        if teleport_relocating {
            sim.foot_mark_put(stable_id, rules, overlay_registry, ctx.effects);
        }
        // Teleport Process's warp step (`0x007192F0`), after the relocation.
        // Its `vt+0x480(NULL, 1)` is the owner's class setter
        // (`Simulation::assign_null_destination`), which reaches the
        // Teleport's own Stop_Moving through Foot's null arm.
        if teleport_reached {
            // 0x007197B4: vt+0x480(NULL, 1) before Stop_Moving.
            sim.assign_null_destination(stable_id, rules, overlay_registry, ctx.effects);
        }
        if teleport_relocating {
            // 0x00719710: ChronoIn at the new Location.
            Self::teleport_warp_sound(
                &mut sim.interner,
                &sim.substrate,
                sim.resolved_terrain.as_ref(),
                &mut sim.sound_events,
                stable_id,
                teleport_movement::WarpSound::In,
                rules,
            );
            // Relocation 0x0071971C calls vt+0x18C(2), including a same-cell
            // relocation; ordinary Fly motion has no such call.
            process.bridge_state_changed |= sim.per_cell_process(
                stable_id,
                movement::PerCellReason::Arrival,
                rules,
                overlay_registry,
                ctx.effects,
            )?;
            process.per_cell_ran = true;
            // 0x00719725: Process's own Stop_Moving, after the arrival
            // Per_Cell_Process, which still read Is_Moving set.
            if let Some(locomotor) = sim
                .substrate
                .entities
                .get_mut(stable_id)
                .and_then(|entity| entity.locomotor.as_mut())
            {
                locomotor.teleport_process_stop();
            }
            // 0x0071972E CellClass::PickupCrate (`0x00481A00`) on the
            // destination cell for the linked Foot; its answer is not read.
            let cell = sim
                .substrate
                .entities
                .get(stable_id)
                .map(|entity| (entity.position.rx() as i16, entity.position.ry() as i16));
            if let Some(cell) = cell {
                let _ = sim.pickup_crate_at(stable_id, cell, rules, overlay_registry, ctx.effects);
            }
            // 0x0071973C: vt+0x480(NULL, 1). A Teleporter still in radio
            // contact gets a Drive here, which the FootClass::AI tail ends
            // again.
            sim.assign_null_destination(stable_id, rules, overlay_registry, ctx.effects);
            // 0x00719742..0x00719791: the arrival WarpOut at the Location.
            sim.teleport_warp_out(stable_id, rules);
        }
        Ok(process)
    }

    /// Rocket Process (`0x006622C0`, [`Simulation::process_rocket`]). A
    /// detonation UnInits its owner, which ends `FootClass::AI` and
    /// `AircraftClass::AI` (`0x004DA87A`).
    fn process_rocket_locomotor(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: super::FrameEffects<'_>,
    ) -> LocomotorProcess {
        let mut process = LocomotorProcess::admitted();
        let outcome = self.process_rocket(stable_id, rules, overlay_registry, effects);
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
    /// live object turns, or the tube turn that replaces it. This exposes no
    /// alternate physics or callback loop.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn process_ground_locomotor_for_test(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        grid: Option<&crate::sim::pathfinding::PathGrid>,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Result<movement::MovementTickStats, super::FrameAdvanceError> {
        self.install_fixture_path_grid(grid);
        self.process_ground_locomotor_stats_for_test(id, rules, registry)
    }

    /// The ordinary ground locomotor Process corridor, without Object/Techno AI,
    /// or the tube turn of an object holding a tube.
    /// Infantry Scatter51D478 calls the active locomotor synchronously; its
    /// PerCell and boundary receivers must finish before Scatter returns.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn process_ground_locomotor_stats_for_test(
        &mut self,
        stable_id: u64,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Result<movement::MovementTickStats, super::FrameAdvanceError> {
        let effects = super::FrameEffects::default();
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(tube_active)
        {
            return self
                .tube_object_turn(
                    stable_id,
                    rules.unwrap_or(crate::rules::ruleset::RuleSet::empty_for_test()),
                    overlay_registry,
                    effects,
                )
                .map(|turn| turn.movement);
        }
        self.process_ground_locomotor_one(
            stable_id,
            rules.unwrap_or(crate::rules::ruleset::RuleSet::empty_for_test()),
            overlay_registry,
            effects,
        )
        .map(|outcome| outcome.movement)
    }

    pub(crate) fn process_ground_locomotor_one(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: super::FrameEffects<'_>,
    ) -> Result<GroundLocomotorOutcome, super::FrameAdvanceError> {
        let sim = self;
        let mut outcome = GroundLocomotorOutcome::default();
        sim.complete_pending_order(stable_id, rules, overlay_registry, effects);
        // Drive4B050B..0557 / Ship69FC1B..FC67 samples the containing
        // cell slope before any active-track, destination or turn return.
        let slope = sim.substrate.entities.get(stable_id).and_then(|entity| {
            sim.resolved_terrain
                .as_ref()?
                .cell(entity.position.rx(), entity.position.ry())
                .map(|cell| cell.slope_type)
        });
        if let Some(slope) = slope
            && let Some(entity) = sim.substrate.entities.get_mut_if(stable_id, |entity| {
                movement::slope_transition::process_entry_changes(entity, slope)
            })
        {
            movement::slope_transition::sample_process_entry(
                entity,
                slope,
                sim.session.binary_frame(),
            );
        }
        match sim.process_track_turn(stable_id, rules, overlay_registry, effects) {
            movement::track_turn::TrackProcessEntry::Movement => {}
            movement::track_turn::TrackProcessEntry::Tail => {
                sim.track_process_rest_tail(stable_id, rules, overlay_registry, effects);
                return Ok(outcome);
            }
            movement::track_turn::TrackProcessEntry::Return => return Ok(outcome),
        }
        // The grid is borrowed in place, beside the other fields: a snapshot
        // would count a shared handle up and down for every mover's visit.
        let mut pending_movement = movement::movement_tick::begin_movement(
            &mut sim.substrate.entities,
            stable_id,
            sim.path_grid.as_deref(),
            sim.session.binary_frame(),
            sim.resolved_terrain.as_ref(),
            sim.playfield_bounds,
            &mut sim.interner,
            rules,
            &sim.houses,
        );
        // Process_Movement's no-queue request and the Process_Track after it.
        // When the active-track Process_Track(0) ends its track, the same
        // Process continues into Process_Movement and Process_Track(1)
        // (Drive 0x4B0583..0x4B0667 / Ship 0x69FC93..0x69FD0E;
        // `movement::track_continuation`).
        let frame_error = |sim: &Simulation, cause| super::FrameAdvanceError {
            tick: sim.session.tick(),
            binary_frame: sim.session.binary_frame(),
            entity_id: stable_id,
            cause,
        };
        let mut retry = false;
        let mut process_tail = true;
        loop {
            if let Some(mut request) = pending_movement.take_foot_path_request() {
                let outcome = sim
                    .run_foot_path_request(&mut request, rules, overlay_registry, effects)
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
                    .run_walk_admission_request(request, rules, overlay_registry, effects)
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
                let out = sim
                    .run_track_process_movement(
                        id,
                        family,
                        movement::ProcessMovementArgs::OUTER,
                        rules,
                        overlay_registry,
                        effects,
                    )
                    .map_err(|cause| frame_error(sim, cause))?;
                // Drive4B0A7E/4B064C: the out byte returns the Process
                // immediately; it must not run the common rest-speed tail.
                if out {
                    process_tail = false;
                }
                if !out
                    && sim
                        .substrate
                        .entities
                        .get(id)
                        .is_some_and(|entity| entity.lifecycle.object_alive())
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
                        .is_some_and(|entity| entity.lifecycle.object_alive())
                })
            else {
                break;
            };
            let invocation = movement::track_process::TrackInvocation {
                retry,
                ..invocation
            };
            let pass = sim
                .run_track_process(invocation, rules, overlay_registry, effects)
                .map_err(|cause| frame_error(sim, cause))?;
            pending_movement.record_track_movement(pass.moved);
            // Only the FIRST, active-track call tests Process_Track's AL
            // (4B057B). The post-fresh call ignores AL and goes through the
            // live-owner check to the common tail (4B0AAF..4B0ABC).
            if invocation.active_gate && pass.aborted {
                process_tail = false;
                break;
            }
            if retry || !invocation.active_gate {
                break;
            }
            if !sim
                .begin_track_end_continuation(
                    invocation.entity_id,
                    invocation.family,
                    rules,
                    effects,
                )
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
        if let Some((id, head)) = pending_movement.take_walk_per_cell() {
            outcome.bridge_state_changed |=
                sim.run_completed_walk_step(id, head, rules, overlay_registry, effects)?;
        }
        if let Some((id, coord)) = pending_movement.take_walk_boundary() {
            sim.run_walk_boundary(id, coord, rules, overlay_registry, effects);
            pending_movement.record_track_movement(1);
        }

        outcome
            .movement
            .merge(movement::movement_tick::finish_movement_pass(
                pending_movement,
            ));
        if process_tail {
            sim.track_process_rest_tail(stable_id, rules, overlay_registry, effects);
        }
        Ok(outcome)
    }

    /// FootClass::AI's sound steps after the move sound, in native order.
    /// `quiet`: the object is quiet now (`world::quiet_units`).
    fn foot_edge_sounds(
        &mut self,
        stable_id: u64,
        quiet: bool,
        rules: &RuleSet,
        effects: super::FrameEffects<'_>,
    ) {
        let sim = self;
        // Foot AI 4DAB3C..4DABB1: a DropAsBomb object starting to fall plays
        // its type's VoiceFalling at its Location, before the sinking/crash
        // edges and looping-sound position update (PlayAt7509E0, ungated).
        // A quiet unit has no fall edge.
        if !quiet
            && let Some(entity) = sim.substrate.entities.get_mut_if(
                stable_id,
                crate::sim::game_entity::GameEntity::falling_edge_pending,
            )
            && entity.observe_falling_edge()
            && let Some(voice) = sim
                .interner
                .object_type(entity.type_ref(), rules)
                .and_then(|object| object.voice_falling.clone())
        {
            let event =
                super::SimSoundEvent::voc_at_location(voice, entity, sim.resolved_terrain.as_ref());
            sim.sound_events.emit(event);
        }
        Self::sinking_edge_sounds(
            &sim.interner,
            &mut sim.substrate,
            &mut sim.sound_events,
            stable_id,
            rules,
        );
        Self::crash_edge_sounds(
            &sim.interner,
            &mut sim.substrate,
            &mut sim.sound_events,
            &sim.session,
            &sim.houses,
            stable_id,
            rules,
        );
        // Foot4DADF0 ->750D40 uses the coordinate BEFORE those callbacks
        // and Unit's subsequent hull lowering. Finish PlayAt/Release first.
        if let Some(entity) = sim.substrate.entities.get(stable_id) {
            effects.update_looping_sound(sim, rules, stable_id, Self::movement_sound_world(entity));
        }
    }

    /// FootClass::AI's pending entry, then the eater's ParasiteClass AI.
    /// `quiet`: the object is quiet now (`world::quiet_units`), and has
    /// neither.
    fn foot_pending_entry_and_parasite(
        &mut self,
        stable_id: u64,
        quiet: bool,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: super::FrameEffects<'_>,
    ) {
        let sim = self;
        if quiet {
            return;
        }
        // FootClass::AI 0x004DAED0..0x004DAEDC: with vt+0x1D8 (the warp-in)
        // clear, the pending entry (`Unit+0x500`) asks for its slot.
        if sim
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| !entity.is_warping_in() && entity.pending_entry().is_some())
        {
            crate::sim::docking::building_dock::try_pending_entry(sim, rules, stable_id, effects);
        }
        // FootClass::AI tail 0x004DAEE1..0x004DAEF3, after the piggyback swap:
        // an infected Foot runs its eater's ParasiteClass AI in its own turn.
        sim.parasite_ai_for_victim(stable_id, rules, overlay_registry, effects);
    }

    pub(super) fn advance_live_object_pass(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: super::FrameEffects<'_>,
    ) -> Result<LiveObjectPassOutcome, super::FrameAdvanceError> {
        self.answer_batched_path_searches(rules, overlay_registry);
        self.answer_batched_scans(rules, overlay_registry);
        let object_ctx = techno_ai::ObjectAiCtx {
            effects,
            overlay_registry,
        };

        let mut outcome = LiveObjectPassOutcome::default();
        // Sight writes to the maps the pass never reads wait for its end,
        // and apply map by map in parallel (`vision::Planes`). Without a
        // current house the app's sound boundary reads the map of whichever
        // house it falls back to, so none waits.
        if let Some(current) = self.session.current_house() {
            let kept = self
                .houses
                .keys()
                .copied()
                .filter(|&house| self.owner_is_human(house))
                .chain([current])
                .collect();
            self.fog.defer_plane_writes(kept);
        }
        // The pass's sight walks ask the client maps once, and each owner's
        // viewers once until the ally graph changes (`vision::RevealViewers`).
        let clients = self.fog_viewers();
        self.reveal_viewers.begin(clients);
        // Its garrisonable civilian buildings walk the Houses for the
        // Civilian-side one once between them (`CivilianSideHouse`).
        self.civilian_house_memo.begin();
        let walked = self.try_for_each_live_object::<super::FrameAdvanceError>(|sim, stable_id| {
            #[cfg(feature = "debug-tools")]
            let debug_before = sim
                .substrate
                .entities
                .get(stable_id)
                .and_then(crate::sim::debug_event_log::DebugObservation::capture);
            // A sleeping object's visit is counted, not run (`idle_sleep`).
            let check = match sim.visit_sleeper(stable_id, rules, overlay_registry, effects) {
                super::idle_sleep::SleepVisit::Counted => return Ok(()),
                super::idle_sleep::SleepVisit::Awake => None,
                super::idle_sleep::SleepVisit::Check(check) => Some(check),
            };
            let turn = sim.advance_live_object_turn(stable_id, rules, object_ctx)?;
            if let Some(check) = check {
                sim.finish_sleep_check(stable_id, *check);
            }
            sim.sleep_after_turn(stable_id, rules);
            sim.quiet_after_turn(stable_id, rules);
            #[cfg(feature = "debug-tools")]
            if let Some(before) = debug_before
                && let Some(entity) = sim.substrate.entities.get_mut(stable_id)
            {
                before.record(entity, sim.session.binary_frame());
            }
            outcome.destroyed_structure |= turn.destroyed_structure;
            outcome.bridge_state_changed |= turn.bridge_state_changed;
            outcome.ownership_changed |= turn.ownership_changed;
            Ok(())
        });
        self.fog.settle_plane_writes();
        self.reveal_viewers.end();
        self.civilian_house_memo.end();
        self.finish_batched_path_searches();
        self.finish_batched_scans();
        walked?;
        Ok(outcome)
    }

    pub(super) fn advance_live_object_turn(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        object_ctx: techno_ai::ObjectAiCtx<'_>,
    ) -> Result<ObjectTurnOutcome, super::FrameAdvanceError> {
        let sim = self;
        let overlay_registry = object_ctx.overlay_registry;
        let mut outcome = ObjectTurnOutcome::default();
        // The turn looks this object up by id about a hundred times; from
        // here the store answers for it without its id index (VERA-only,
        // every answer unchanged).
        sim.substrate.entities.focus(stable_id);
        // UnitClass::AI / InfantryClass::AI give an active TubeMovement
        // object the whole live-object turn.  Capture before the leaf:
        // successful finalization clears the payload but must still skip
        // every ordinary locomotor tail and the second mission checkpoint.
        // A quiet unit holds no tube (`world::quiet_units`). The mark only
        // clears during a turn, so a turn that starts loud stays loud and
        // asks no more; one that starts quiet asks the live mark.
        let (tube_active_at_entry, was_structure, entered_quiet) = sim
            .substrate
            .entities
            .get_and_quiet(stable_id)
            .map_or((false, false, false), |(entity, quiet)| {
                (
                    !quiet && tube_active(entity),
                    entity.category() == EntityCategory::Structure,
                    quiet,
                )
            });
        let quiet = |sim: &Simulation| entered_quiet && sim.quiet(stable_id);
        #[cfg(debug_assertions)]
        if entered_quiet {
            sim.check_quiet(stable_id, rules);
        }
        let _building_area = was_structure
            .then(|| crate::sim::frame_profile::area(crate::sim::frame_profile::phase::BUILDINGS));
        let mut ai = sim.object_ai_visit_one_with_effects(stable_id, rules, object_ctx);
        if let Some(error) = ai.frame_error.take() {
            return Err(error);
        }
        outcome.bridge_state_changed |= ai.bridge_state_changed;
        outcome.destroyed_structure |= ai.destroyed_structure;
        outcome.ownership_changed |= ai.ownership_changed();
        // One read serves the three gates after the AI visit.
        let Some(entity) = sim.substrate.entities.get(stable_id) else {
            outcome.destroyed_structure |= was_structure;
            return Ok(outcome);
        };
        let dying = entity.death.is_dying();
        let foot = matches!(
            entity.category(),
            EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
        );
        if was_structure && dying {
            outcome.destroyed_structure = true;
        }
        if dying || (!tube_active_at_entry && foot && !entity.lifecycle.object_alive()) {
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
        // `Simulation::temporal_ai_prologue`). A quiet unit is not warped.
        if !quiet(sim) && entity.ai_frozen() {
            return Ok(outcome);
        }
        // BuildingClass::AI calls no FootClass::AI. Every step below returns
        // on a building's class, or on its missing locomotor, except three:
        // the sound steps (which also deliver the frame's sounds so far) and
        // the pending entry and parasite, which read state VERA keeps on
        // every object, and the fire visit, which commits even when it fires
        // nothing. A building without a locomotor takes those three, in
        // their order, and skips the rest (VERA-only, results unchanged).
        // One that is not alive took none: the tail returned on Object+90
        // after the Process, before the first of them.
        if !foot && entity.locomotor.is_none() {
            if entity.lifecycle.object_alive() {
                // Only Units go quiet.
                sim.foot_edge_sounds(stable_id, false, rules, object_ctx.effects);
                sim.foot_pending_entry_and_parasite(
                    stable_id,
                    false,
                    rules,
                    overlay_registry,
                    object_ctx.effects,
                );
                // A visit that fires nothing commits no consequences.
                let receipt = sim.commit_no_fire(rules, overlay_registry, object_ctx.effects);
                outcome.bridge_state_changed |= receipt.bridge_state_changed;
                outcome.destroyed_structure |= receipt.structure_destroyed;
            }
            return Ok(outcome);
        }

        // Ares 3.0 10014BA0 (hook 0x4DA53E) burns a live Foot standing in
        // an active firestorm wall, then heals one in tiberium, right after
        // FootAI's TechnoAI call; the hook then replays 4DA53E..548's
        // Object+90 reload, so a burnt Foot returns here.
        #[cfg(feature = "ares")]
        if !tube_active_at_entry
            && sim.substrate.entities.get(stable_id).is_some_and(|entity| {
                matches!(
                    entity.category(),
                    EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
                )
            })
        {
            outcome.bridge_state_changed |=
                sim.ares_foot_firestorm(stable_id, rules, overlay_registry, object_ctx.effects);
            sim.ares_foot_tiberium_heal(stable_id, rules);
            if sim
                .substrate
                .entities
                .get(stable_id)
                .is_none_or(|entity| entity.death.is_dying() || !entity.lifecycle.object_alive())
            {
                return Ok(outcome);
            }
        }
        // Foot4DA54E follows the post-TechnoAI Object+90 gate above.
        // An entry-active Tube bypasses FootAI; it must retain the byte.
        // A quiet unit's latch is clear (`world::quiet_units`).
        if !tube_active_at_entry
            && !quiet(sim)
            && let Some(entity) = sim.substrate.entities.get_mut_if(stable_id, |entity| {
                matches!(
                    entity.category(),
                    EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
                ) && entity.mission_leaf.foot_idle_entry_latch() != 0
            })
        {
            entity.mission_leaf.set_foot_idle_entry_latch(0);
        }

        // Foot4DA554..629 runs before movement and before the class's fire
        // suffix. A field created by a later Logic slot cannot hit this Foot
        // retroactively. Complete its receiver/lifecycle callbacks now.
        if !tube_active_at_entry
            && let Some(receipt) = crate::sim::combat::world_receiver::apply_foot_radiation(
                sim,
                rules,
                stable_id,
                overlay_registry,
                object_ctx.effects,
            )
        {
            outcome.destroyed_structure |= receipt.structure_destroyed;
            outcome.bridge_state_changed |= receipt.bridge_state_changed;
        }
        // Foot4DA62F reloads Object+90 after the synchronous receiver.
        if !tube_active_at_entry
            && sim.substrate.entities.get(stable_id).is_none_or(|entity| {
                entity.death.is_dying()
                    || (matches!(
                        entity.category(),
                        EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
                    ) && !entity.lifecycle.object_alive())
            })
        {
            return Ok(outcome);
        }

        if !tube_active_at_entry {
            // Foot4DA63B..670 precedes the high-flight sight block and
            // Process. Cruise crossing itself has no PerCell promotion.
            sim.promote_balloon_playfield_membership_before_process(stable_id, rules);
            sim.refresh_high_flying_sight_before_process(stable_id, rules, object_ctx.effects);
            sim.dispatch_airborne_cell_tag_event(
                stable_id,
                super::UninitContext::new(rules, overlay_registry).with_effects(object_ctx.effects),
            );
        }

        // Foot4DA80C captures +538 before locomotor Process. The sound tail
        // compares this same counter after its canonical cadence owner runs.
        // Only that owner below writes it (`animation::tick_foot_body_frame_counter`),
        // so a quiet unit, which takes neither while it stays quiet
        // (`world::quiet_units`), reads it only once something writes it.
        let mut body_frame_before_process = if quiet(sim) {
            None
        } else {
            let Some(counter) = sim
                .substrate
                .entities
                .get(stable_id)
                .map(|entity| entity.body_frame_counter.get())
            else {
                return Ok(outcome);
            };
            Some(counter)
        };
        // An entry-active TubeMovement leaf runs in place of the Foot AI and
        // its Process.
        let process = if tube_active_at_entry {
            let tube =
                sim.tube_object_turn(stable_id, rules, overlay_registry, object_ctx.effects)?;
            LocomotorProcess {
                movement: tube.movement,
                bridge_state_changed: tube.bridge_state_changed,
                per_cell_ran: tube.per_cell_ran,
                ..LocomotorProcess::default()
            }
        } else {
            sim.process_active_locomotor(stable_id, rules, object_ctx)?
        };
        outcome.movement.merge(process.movement);
        outcome.bridge_state_changed |= process.bridge_state_changed;
        // `0x004DA87A`: the Process may have converted or removed the owner.
        if process.ended
            || sim
                .substrate
                .entities
                .get(stable_id)
                .is_none_or(|e| e.death.is_dying() || !e.lifecycle.object_alive())
        {
            return Ok(outcome);
        }

        // FootClass advances its shared body counter immediately after
        // this object's locomotor Process, against the still-current
        // absolute binary frame, inside the Process admission
        // (`0x004DA81A` bypasses both). The global frame commits only after
        // the complete live-object pass.
        // Read now what the Process left unwritten if it made the unit loud.
        if body_frame_before_process.is_none() && !quiet(sim) {
            body_frame_before_process = sim
                .substrate
                .entities
                .get(stable_id)
                .map(|entity| entity.body_frame_counter.get());
        }
        if process.admitted
            && foot_body_counter_admitted(tube_active_at_entry)
            && body_frame_before_process.is_some()
        {
            let foot_body_cadence = sim.substrate.entities.get(stable_id).and_then(|entity| {
                if !entity.category().is_foot() {
                    return None;
                }
                let object = sim.interner.object_type(entity.type_ref(), rules)?;
                #[cfg(feature = "ares")]
                let air_rate = object.techno_combat().movement().air_rate();
                #[cfg(not(feature = "ares"))]
                let air_rate = 0;
                Some((
                    crate::sim::animation::FootBodyCadence {
                        walk_rate: object.walk_rate,
                        idle_rate: object.idle_rate,
                    },
                    object.hover_attack,
                    object.deploy_to_land,
                    air_rate,
                ))
            });
            let frame = sim.session.binary_frame();
            if let Some((cadence, hover_attack, deploy_to_land, air_rate)) = foot_body_cadence
                && let Some(entity) = sim.substrate.entities.get_mut_if(stable_id, |entity| {
                    crate::sim::animation::foot_body_frame_due(
                        entity,
                        crate::sim::movement::SpeedRules::new(rules, &sim.interner, &sim.houses),
                        cadence,
                        hover_attack,
                        deploy_to_land,
                        air_rate,
                        sim.resolved_terrain.as_ref(),
                        frame,
                    )
                })
            {
                crate::sim::animation::tick_foot_body_frame_counter(
                    entity,
                    crate::sim::movement::SpeedRules::new(rules, &sim.interner, &sim.houses),
                    cadence,
                    hover_attack,
                    deploy_to_land,
                    air_rate,
                    sim.resolved_terrain.as_ref(),
                    sim.session.binary_frame(),
                );
            }
        }

        // UnitClass::AI (`0x007363A4..0x007363C8`) and InfantryClass::AI
        // (`0x0051BAB8..0x0051BAD9`) return after an entry-active
        // TubeMovement. A tube the Process armed this turn ends nothing:
        // neither AI reads `+0x684` after its FootClass::AI call, so the
        // Foot and class tails run.
        if tube_active_at_entry {
            return Ok(outcome);
        }

        // Foot4DAA01 MoveSound,4DABC7 sinking and4DACDD crash edges run
        // before Scatter/piggyback/pending-entry/Parasite callbacks. In
        // particular, a later Squid cull cannot run this visit's sound edge.
        if let Some(body_frame_before_process) = body_frame_before_process {
            Self::tick_move_sound_after_process(
                &sim.interner,
                &sim.session,
                &sim.main_rng,
                &mut sim.substrate,
                &mut sim.sound_events,
                &sim.houses,
                stable_id,
                body_frame_before_process,
                rules,
            );
        }
        sim.foot_edge_sounds(stable_id, quiet(sim), rules, object_ctx.effects);

        // FootClass::AI 0x004DADF5..0x004DAE59: the64-frame idle Scatter.
        match sim.foot_idle_scatter(stable_id, rules, overlay_registry, object_ctx.effects) {
            Ok(changed) => outcome.bridge_state_changed |= changed,
            Err(cause) => log::debug!("Foot {stable_id} did not idle-scatter: {cause}"),
        }
        // FootClass::AI 0x004DAE5F..0x004DAEC3: the active IPiggyback ends
        // when its Is_Ok_To_End allows.
        sim.try_end_piggyback(stable_id, rules);
        // Foot4DAECA clears the transient draw lift after piggyback callbacks,
        // before the warp-in/pending-entry predicate and ParasiteAI.
        if let Some(entity) = sim
            .substrate
            .entities
            .get_mut_if(stable_id, |entity| entity.ion_blast_lift_pixels() != 0)
        {
            entity.clear_ion_blast_lift();
        }
        sim.foot_pending_entry_and_parasite(
            stable_id,
            quiet(sim),
            rules,
            overlay_registry,
            object_ctx.effects,
        );

        // Unit73649C follows the complete Foot tail. Its disguise pick must
        // follow Foot's own Scenario draws and precede Unit's firing suffix.
        sim.update_unit_disguise(stable_id, rules)
            .map_err(|cause| super::FrameAdvanceError {
                tick: sim.session.tick(),
                binary_frame: sim.session.binary_frame(),
                entity_id: stable_id,
                cause,
            })?;

        // FootClass::AI4DA806..4DAEF3 adds no PerCell for a bare coordinate
        // change. Keep sensors/uncloak/Temporal under actual class arrivals;
        // airborne Jumpjet54AEC0 can retain a sensor circle at its old cell.

        // Aircraft414DA8..414DB0 rereads Object+90 after the complete Foot
        // tail. A dead aircraft returns before descent, wake draws and the
        // remaining class suffix, regardless of which callback cleared Alive.
        if sim.substrate.entities.get(stable_id).is_some_and(|entity| {
            entity.category() == EntityCategory::Aircraft && !entity.lifecycle.object_alive()
        }) {
            return Ok(outcome);
        }
        if sim.tick_ship_sinking(stable_id, rules, overlay_registry, object_ctx.effects) {
            return Ok(outcome);
        }
        // Aircraft414ED0..414F42 emits after sinking, before the off-map
        // removal below. Its independent child survives that removal.
        sim.emit_aircraft_trailer(stable_id, rules, object_ctx.effects)
            .map_err(|cause| super::FrameAdvanceError {
                tick: sim.session.tick(),
                binary_frame: sim.session.binary_frame(),
                entity_id: stable_id,
                cause,
            })?;
        // Infantry51BDE7..51BF80 follows the Foot tail: rate-zero fire
        // cancellation, its second Ready/Commence, Fear, firing51BF59,
        // sequencer51BF6A and locomotion actions51BF7B. Inline FireAt
        // consequences join the dynamic Logic suffix before its next slot.
        let infantry = sim
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category() == EntityCategory::Infantry);
        if infantry {
            // Installed Ares InfantryClass_Update_Reload10047980 replaces
            // gamemd51BCB2: out of limbo, Reload(+424) before idle Scatter.
            // The shared Techno owner supplies count/clock semantics; the
            // Unit-only MobileFire pause is deliberately not used here.
            #[cfg(feature = "ares")]
            if let Some(entity) = sim.substrate.entities.get_mut(stable_id)
                && entity.is_object_alive()
                && !entity.lifecycle.in_limbo()
                && let Some(obj) = sim.interner.object_type(entity.type_ref(), rules)
            {
                entity.reload_techno_ammo(obj, sim.session.binary_frame() as i32);
            }
            //51BCA4..51BDCF follows the entire FootAI, including Process and
            // its tails. A successful Scatter invokes Process a second time
            // synchronously, before the Infantry Ready/Commence/fire suffix.
            outcome.bridge_state_changed |= sim
                .scatter_idle_infantry_from_building(
                    stable_id,
                    rules,
                    overlay_registry,
                    object_ctx.effects,
                )
                .map_err(|cause| super::FrameAdvanceError {
                    tick: sim.session.tick(),
                    binary_frame: sim.session.binary_frame(),
                    entity_id: stable_id,
                    cause,
                })?;
            let receipt = sim
                .infantry_action_turn(stable_id, rules, overlay_registry, object_ctx.effects)
                .map_err(|cause| super::FrameAdvanceError {
                    tick: sim.session.tick(),
                    binary_frame: sim.session.binary_frame(),
                    entity_id: stable_id,
                    cause,
                })?;
            outcome.bridge_state_changed |= receipt.bridge_state_changed;
            outcome.destroyed_structure |= receipt.structure_destroyed;
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
        if sim.remove_aircraft_off_map(stable_id, rules, overlay_registry, object_ctx.effects) {
            return Ok(outcome);
        }
        // Aircraft414FDF..415046 follows removal, before its Ready/Commence.
        sim.cleanup_aircraft_dummy_cells(stable_id, rules, overlay_registry, object_ctx.effects);
        // UnitClass::AI after FootClass::AI, before its second Ready/Commence.
        crate::sim::miner::miner_system::unit_ai_clear_harvesting(&mut sim.substrate, stable_id);
        // Unit7365E1 Fire_At_Target then7365E8 Facing_Update precede
        // its second Ready/Commence. FireAt appends into the live Logic walk.
        if !infantry {
            // A quiet unit's fire visit fires nothing and changes nothing
            // (`world::quiet_units`); its commit remains.
            let receipt = if quiet(sim) {
                sim.commit_no_fire(rules, overlay_registry, object_ctx.effects)
            } else {
                sim.commit_fire_visit(
                    crate::sim::combat::world_fire::FireVisit::UnitTarget(stable_id),
                    rules,
                    overlay_registry,
                    object_ctx.effects,
                )
            };
            outcome.bridge_state_changed |= receipt.bridge_state_changed;
            outcome.destroyed_structure |= receipt.structure_destroyed;
            // Unit73666B..736685: a visceroid's UpdateVisceroid737180.
            sim.unit_update_visceroid(stable_id, rules, overlay_registry, object_ctx.effects);
            // UnitAI736689 Reload+424 follows FireAtTarget/Facing and
            // precedes the second Ready/Commence. Aircraft/Infantry have
            // no corresponding ordinary Reload dispatch here. The native
            // post-Foot alive gate precedes firing; no second gate is added.
            let now = sim.session.binary_frame() as i32;
            if let Some(entity) = sim.substrate.entities.get_mut_if(stable_id, |entity| {
                entity.category() == EntityCategory::Unit
                    && sim
                        .interner
                        .object_type(entity.type_ref(), rules)
                        .is_some_and(|obj| entity.reload_unit_ammo_writes(obj, now))
            }) && let Some(obj) = sim.interner.object_type(entity.type_ref(), rules)
            {
                entity
                    .reload_unit_ammo(obj, sim.session.binary_frame() as i32)
                    .map_err(|cause| super::FrameAdvanceError {
                        tick: sim.session.tick(),
                        binary_frame: sim.session.binary_frame(),
                        entity_id: stable_id,
                        cause,
                    })?;
            }
            sim.object_ai_post_movement_promote_one(stable_id, rules);
            // Unit73671F..736817 follows that Commence (the CTF flag block
            // between them is unreachable).
            sim.unit_ai_seek_repair(stable_id, rules, overlay_registry, object_ctx.effects);
            // Unit73681D..73697B, the last block of UnitClass::AI.
            crate::sim::miner::miner_system::unit_ai_nudge_off_refinery_pad(
                sim,
                stable_id,
                rules,
                overlay_registry,
                object_ctx.effects,
            );
        }
        sim.aircraft_crash_smoke(stable_id, rules);
        // Aircraft415136..415184, the last block of AircraftClass::AI.
        sim.carryall_carry_cargo(stable_id, rules, overlay_registry, object_ctx.effects);
        Ok(outcome)
    }
}
