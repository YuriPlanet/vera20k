//! The master frame: the one `Main_Tick`-shaped transaction that every app,
//! headless, replay and fixture frame runs.
//!
//! [`LOGIC_STEPS`] is the frame's order up to the House tail: command ingress
//! and admission, then `LogicClass::PerTickUpdate @ 0x0055AFB0` up to the
//! Factory sweep. Each [`FrameStep`] names one owner call, and
//! [`Simulation::advance_master_frame`] visits them in that order. The tail
//! follows in [`Simulation::run_late_region`]: the House update, `Queue_AI`'s
//! command exchange and dispatch, the late frame commit and the single
//! pending-delete drain. The tail can suspend the frame (House outcome wait,
//! network wait, checksum modal); a resume re-enters the tail without
//! repeating any earlier step.

use super::*;
use crate::rules::overlay_types::OverlayTypeRegistry;

/// One named step of the master frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameStep {
    /// Frame-ingress commands due this frame (ordinary lane only).
    SessionCommands,
    /// Command admission (ordinary lane only), the display-ground sort
    /// (`MainTick55DBC8`), the pre-Logic save hook (`MainTick55DBCD`) and the
    /// Logic visit output.
    AdmitCommands,
    /// The trigger poll. YR `LogicClass::Update` establishes trigger state
    /// before visiting the live LogicVector, so object work in this frame
    /// observes its actions.
    Triggers,
    /// Shroud growth, the pending conceal at the signed pre-increment frame
    /// boundary (`Logic55B29A/55B2AD`) and fog regrowth.
    Shroud,
    /// The scenario lighting transition.
    Lighting,
    /// Ore growth and spread.
    OreGrowth,
    /// `BombListClass::UpdateAll` (`0x0055B4E1`).
    BombList,
    /// `Kamikaze__Update` (`0x0055B4F0`).
    Kamikaze,
    /// The copied TeamClass AI pass (`0x0055B502..0x0055B59F`, TeamClass
    /// `+0x5C`). It precedes the Disk walk, `LightningStorm::Process`, the
    /// RadSite walk and the LogicClass object vector, so those steps see this
    /// frame's Team actions, and an object's `ReceiveDamage` that arms a
    /// Team's base-defense suspension reaches only the next Team visit.
    TeamScript,
    /// The reverse Disk walk (`0x0055B5A1..0x0055B5BD`), then
    /// `SpotLightClass::UpdateAll @ 0x005FF390` (`0x0055B5BE`, presentation
    /// combat-light flashes) and `LaserDrawClass::UpdateAllAI @ 0x00550150`
    /// (`0x0055B5C3`, the presentation's LaserUpdate).
    DiskLasers,
    /// `LightningStorm::Process @ 0x0053A6C0` (`0x0055B5C8`): the nuke flash,
    /// the Psychic Dominator, the Chronosphere screen effect and the storm.
    LightningStorm,
    /// The reverse RadSite walk (`0x0055B5CD..0x0055B5E7`), then
    /// `UpdateLightConverts` (`0x0055B5EA`) and `EMPulseClass::UpdateAll`
    /// (`0x0055B5F6`), neither of which has a simulation counterpart.
    Radiation,
    /// The live LogicClass object vector (`0x0055B5FB..0x0055B619`).
    LogicVector,
    /// Wake anims under moving units on water.
    Wakes,
    /// The frame's sight projection and Gap Generator cover.
    Vision,
    /// Publish the terrain navigation and smudge changes the object pass left
    /// pending.
    TerrainNavigation,
    /// `IonBlastClass::UpdateAll @ 0x0053D310` (`0x0055B64B`).
    IonBlasts,
    /// `MapClass__UpdateCrateRegenTimers @ 0x0056BBE0` (`0x0055B65A`).
    CrateRegen,
    /// The FactoryClass sweep (`0x0055B66A..0x0055B68B`).
    Factories,
    /// The tail's House update ([`Simulation::run_late_region`]).
    Houses,
    /// The late native frame commit (`MainTick55DE7E`).
    FrameCommit,
    /// The single end-of-tick `ProcessPendingDelete` drain.
    PendingDelete,
}

/// The master frame's steps before its tail, in the order they run.
pub(crate) const LOGIC_STEPS: [FrameStep; 19] = [
    FrameStep::SessionCommands,
    FrameStep::AdmitCommands,
    FrameStep::Triggers,
    FrameStep::Shroud,
    FrameStep::Lighting,
    FrameStep::OreGrowth,
    FrameStep::BombList,
    FrameStep::Kamikaze,
    FrameStep::TeamScript,
    FrameStep::DiskLasers,
    FrameStep::LightningStorm,
    FrameStep::Radiation,
    FrameStep::LogicVector,
    FrameStep::Wakes,
    FrameStep::Vision,
    FrameStep::TerrainNavigation,
    FrameStep::IonBlasts,
    FrameStep::CrateRegen,
    FrameStep::Factories,
];

impl FrameStep {
    /// The profile ledger slot this step's time folds into; a step without
    /// one folds into the next step that has one.
    #[cfg(feature = "telemetry")]
    fn profile_slot(self) -> Option<usize> {
        use crate::sim::frame_profile::phase;
        Some(match self {
            Self::SessionCommands => phase::COMMANDS_INGRESS,
            Self::AdmitCommands => phase::COMMANDS_ADMIT,
            Self::Triggers => phase::TRIGGERS,
            Self::Shroud => phase::SHROUD,
            Self::Lighting => phase::LIGHTING,
            Self::OreGrowth => phase::ORE_GROWTH,
            Self::BombList => return None,
            Self::Kamikaze | Self::DiskLasers => phase::BOMBS,
            Self::TeamScript => phase::TEAM_AI,
            Self::LightningStorm => phase::SUPERWEAPONS,
            Self::Radiation => phase::RADIATION,
            Self::LogicVector => phase::OBJECTS,
            Self::Wakes => phase::WAKES,
            Self::Vision => phase::VISION,
            Self::TerrainNavigation => phase::COMBAT,
            Self::IonBlasts => phase::ION_BLASTS,
            Self::CrateRegen => phase::CRATES,
            Self::Factories => phase::PRODUCTION,
            Self::Houses | Self::FrameCommit | Self::PendingDelete => return None,
        })
    }
}

/// What one frame visit reads, fixed for the whole visit and for a resume of
/// its tail.
#[derive(Clone, Copy)]
pub(super) struct FrameInputs<'a> {
    rules: &'a RuleSet,
    overlay_registry: Option<&'a OverlayTypeRegistry>,
    tick_ms: u32,
    effects: FrameEffects<'a>,
}

impl<'a> FrameInputs<'a> {
    pub(super) fn new(
        rules: &'a RuleSet,
        overlay_registry: Option<&'a OverlayTypeRegistry>,
        tick_ms: u32,
        effects: FrameEffects<'a>,
    ) -> Self {
        Self {
            rules,
            overlay_registry,
            tick_ms,
            effects: effects.with_overlay_registry(overlay_registry),
        }
    }
}

/// The caller's request for the steps before the tail.
#[derive(Clone, Copy)]
struct LogicRequest<'a> {
    ingress: &'a [CommandEnvelope],
    lane: TickLane,
    poll_triggers: bool,
}

impl Simulation {
    /// Mark the step the frame enters. Only tests observe it.
    #[inline]
    fn enter_frame_step(&mut self, step: FrameStep) {
        #[cfg(any(test, feature = "test-support"))]
        self.frame_step_trace.push(step);
        #[cfg(not(any(test, feature = "test-support")))]
        let _ = step;
    }

    /// The steps the frames since the last call entered, in order.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn take_frame_step_trace(&mut self) -> Vec<FrameStep> {
        std::mem::take(&mut self.frame_step_trace)
    }

    /// Advance exactly one authoritative simulation frame.
    ///
    /// This is the sole Main_Tick-shaped entry point, reached in production
    /// only through `advance_app_frame` (F09), which always polls the trigger
    /// definitions installed at scenario init, with the frame's `rules`. The
    /// fixture-only `advance_tick` adapter and the replay fixture runners are
    /// `#[cfg(test)]`; they and focused frame tests may pass `poll_triggers`
    /// false to leave the poll out, also with map triggers installed, as when
    /// a test isolates the House rung's Tag raise.
    pub(crate) fn advance_master_frame(
        &mut self,
        commands: &[CommandEnvelope],
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        tick_ms: u32,
        lane: TickLane,
        poll_triggers: bool,
        effects: FrameEffects<'_>,
    ) -> Result<MasterFrameOutput, FrameAdvanceError> {
        if self.frame_continuation.is_some() {
            return Err(FrameAdvanceError::new(
                self.session.tick(),
                self.session.binary_frame(),
                0,
                "pending frame must be resumed before another Logic visit".to_owned(),
            ));
        }
        // The disabled path reads no clock and does not construct a ledger.
        #[cfg(feature = "telemetry")]
        let mut frame_clock = self
            .frame_profiling
            .then(crate::sim::frame_profile::FrameClock::start);
        #[cfg(feature = "debug-tools")]
        self.apply_debug_tools(rules);
        let inputs = FrameInputs::new(rules, overlay_registry, tick_ms, effects);
        // A failed earlier frame never reached collection; drop its value.
        self.frame_sync_checksum = None;
        self.combat_light_requests.discard();
        self.screen_shake_updates.discard();
        // The wrapping native frame counter is committed LATE (the tail's
        // FrameCommit, beside self.session.tick) so every step sees the same
        // pre-increment frame N. execute_tick is fixed here: command
        // scheduling filters on the separate monotonic ordinal.
        //
        // Every step reads the canonical `path_grid` at its point of use, so
        // a mid-frame republish is visible to the next reader. No
        // command-boundary drain: command-applied deaths (sell, MCV/slave
        // deploy-undeploy, engineer capture) stay in the Dying window like
        // combat deaths, freed only by the single end-of-tick drain, matching
        // gamemd's one ProcessPendingDelete at the tail of Main_Tick. The
        // mid-tick raw-store consumers (vision, power, production, movement,
        // miner, aircraft, …) are dying-gated, so a corpse is excluded until
        // that drain.
        let mut frame = FrameCompletion {
            commands: Vec::new(),
            execute_tick: self.session.tick().saturating_add(1),
            executed_commands: 0,
            spawned_entities: false,
            destroyed_structure: false,
            ownership_changed: false,
            bridge_state_changed: false,
        };
        let request = LogicRequest {
            ingress: commands,
            lane,
            poll_triggers,
        };
        for step in LOGIC_STEPS {
            self.enter_frame_step(step);
            self.run_logic_step(step, inputs, request, &mut frame)?;
            #[cfg(feature = "telemetry")]
            if let (Some(clock), Some(slot)) = (frame_clock.as_mut(), step.profile_slot()) {
                clock.stamp(slot);
            }
        }
        let tail_result = self.run_late_region(inputs, &mut frame, None)?;
        #[cfg(feature = "telemetry")]
        if let Some(clock) = frame_clock.as_mut() {
            clock.stamp(crate::sim::frame_profile::phase::LATE_AI);
        }
        frame.bridge_state_changed |= self
            .trigger_effects
            .iter()
            .any(|effect| matches!(effect, TriggerEffect::BridgeStateChanged));
        Ok(self.complete_or_suspend_frame(
            frame,
            tail_result,
            inputs,
            #[cfg(feature = "telemetry")]
            frame_clock,
        ))
    }

    /// Run one step before the tail. Each arm calls the step's owner.
    fn run_logic_step(
        &mut self,
        step: FrameStep,
        inputs: FrameInputs<'_>,
        request: LogicRequest<'_>,
        frame: &mut FrameCompletion,
    ) -> Result<(), FrameAdvanceError> {
        let FrameInputs {
            rules,
            overlay_registry,
            effects,
            ..
        } = inputs;
        match step {
            FrameStep::SessionCommands => {
                if request.lane == TickLane::Ordinary {
                    frame.executed_commands += self.apply_due_frame_ingress_commands(
                        request.ingress,
                        rules,
                        frame.execute_tick,
                        effects,
                    );
                }
            }
            FrameStep::AdmitCommands => {
                if request.lane == TickLane::Ordinary {
                    frame.commands = self.admit_frame_commands(request.ingress, rules, effects);
                }
                // MainTick55DBC8 precedes Logic55DC9E (including trigger polling).
                Self::sort_display_ground(
                    &self.interner,
                    &self.terrain_objects,
                    &mut self.substrate,
                    rules,
                );
                // MainTick55DBCD saves the post-input, post-sort world before Logic.
                effects.before_logic(self);
                self.lifecycle_outputs.emit(LifecycleOutput::LogicVisit);
            }
            FrameStep::Triggers => {
                if request.poll_triggers {
                    self.poll_triggers_for_master_frame(rules, overlay_registry, effects);
                }
            }
            FrameStep::Shroud => {
                self.tick_shroud_growth(
                    UninitContext::new(rules, overlay_registry).with_effects(effects),
                );
                // Logic55B29A/55B2AD: consume prior pending conceal at the signed
                // pre-increment native frame boundary, before ore, Teams and live objects.
                self.tick_pending_shroud_conceal(
                    UninitContext::new(rules, overlay_registry).with_effects(effects),
                );
                self.tick_fog_regrowth(
                    UninitContext::new(rules, overlay_registry).with_effects(effects),
                );
            }
            FrameStep::Lighting => self.tick_scenario_lighting_transition(rules),
            FrameStep::OreGrowth => self.tick_ore_growth_rungs(rules, overlay_registry),
            // `BombListClass::UpdateAll` follows growth and spread (0x0055B4E1).
            FrameStep::BombList => Self::bomb_list_update(
                &self.interner,
                &mut self.substrate,
                &self.houses,
                &self.resolved_terrain,
                &mut self.bombs,
                rules,
            ),
            FrameStep::Kamikaze => self.kamikaze_update(rules, effects),
            FrameStep::TeamScript => self.run_team_ai_pass(rules, overlay_registry, effects),
            FrameStep::DiskLasers => {
                // FireAt births below wait until next tick.
                frame.bridge_state_changed |=
                    self.tick_disk_lasers(rules, overlay_registry, effects);
                self.lifecycle_outputs.emit(LifecycleOutput::LaserUpdate {
                    frame: self.session.binary_frame() as i32,
                });
            }
            // Runs whatever the superweapons option: a map's NUKE weapon
            // starts the nuke flash without a Super.
            FrameStep::LightningStorm => {
                frame.bridge_state_changed |=
                    crate::sim::superweapon::tick_active_superweapon_effects(
                        self,
                        rules,
                        overlay_registry,
                        effects,
                    );
            }
            FrameStep::Radiation => {
                self.radiation.tick_decay(
                    self.session.binary_frame(),
                    rules,
                    self.resolved_terrain.as_ref(),
                    &mut self.lighting_sources,
                );
                // `LightSourceClass::UpdateLightConverts @ 0x00554D50`
                // (0x0055B5EA..0x0055B5F1, a 6 ms budget) applies queued cell
                // light converts on a wall-clock budget; the renderer's
                // `DeferredCellLightRefresh` owns that. `EMPulseClass::UpdateAll
                // @ 0x004C54A0` (0x0055B5F6) expires EMPulses. RESIDUAL:
                // EMPulseClass is not modelled. Its constructor (0x004C52B0)
                // has no caller, so the walk always finds an empty vector: no
                // RNG, timer or detach work is skipped.
            }
            FrameStep::LogicVector => {
                // Interleaved object-AI pass: each object's mission precedes
                // its own locomotion. Unit/Infantry firing and their
                // class-specific tails run in that same slot; newly appended
                // Bullets/Anims join the live walk. Each live object gets its
                // `+0xC4` AI-counter tick, its owner-local queued-mission
                // promotion (Ready→Commence), and its absorbed mission-handler
                // dispatch (Harvest: the miner FSM, timer-gated with the
                // post-handler epilogue write) here. Player commands from
                // frame N-1 are already represented; frame N's EventClass tail
                // is dispatched in the tail. Movement stays before the vision
                // refresh. The live pass commits each object's AI, movement
                // and lifecycle effects before advancing its cursor; later
                // steps need only these outcomes.
                let object_pass =
                    self.advance_live_object_pass(rules, overlay_registry, effects)?;
                frame.ownership_changed |= object_pass.ownership_changed();
                frame.spawned_entities |= std::mem::take(&mut self.mission_spawned_entities);
                frame.destroyed_structure |= object_pass.destroyed_structure;
                frame.bridge_state_changed |= object_pass.bridge_state_changed;
                // Gate Open runs in Building Mission AI; factory clearance
                // runs in the arriving Unit's Per_Cell_Process before
                // Ready/Commence.
            }
            // Native gate and cadence in `spawn_wakes_for_frame`.
            FrameStep::Wakes => self.spawn_wakes_for_frame(rules),
            FrameStep::Vision => {
                // Sources open, move and close their sight at their own events
                // (Unlimbo, arrival, Limbo), so an object spawned after this
                // pass holds its sight already.
                let vision_config = self.sight_reveal_config(rules);
                self.refresh_fog(&vision_config, rules);
            }
            FrameStep::TerrainNavigation => {
                // Every class fired in its own live Logic slot (an aircraft
                // in its Mission_Attack dispatch), and each Bullet and Wave
                // committed its damage in its own slot: no combat step
                // follows the object pass. Killed structures/voxels stay in
                // the Dying window through the steps below and are freed only
                // by the single end-of-tick drain (gamemd's one
                // ProcessPendingDelete). Those consumers (production
                // speed/factory-spawn scans, repairs, retaliation, miner,
                // aircraft) are dying-gated.
                //
                // Publish what the pass left pending. Every Foot turn ends
                // with a fire commit, and every damage commit publishes the
                // overlay Recalcs recorded so far
                // (`finish_terrain_navigation_changes`), so this catches only
                // those recorded after the pass's last commit. Live-fire
                // commits leave smudge dirt (a cliff collapse's cleared slots)
                // to this flush.
                // RESIDUAL: native completes each Recalc inside its producer's
                // Logic slot (`publish_tiberium_cells`). Trigger: an overlay
                // Recalc whose producer does not publish it, made in a
                // Building or non-Techno turn. Effect: until the next commit
                // (or this call), path searches read the old cell. Frequency:
                // rare; a harvest, a crate pickup and a destroyed wall happen
                // in a Foot turn or a damage commit.
                self.finish_terrain_navigation_changes(rules, &[]);
                Self::flush_smudge_dirty(&mut self.smudge_grid, &mut self.dirty_cells);
                // Retaliation is not a step: every receiver issues its Mission
                // Override inline (`TechnoClass::ReceiveDamage 0x00702A43`),
                // and passengers board in their class PerCellProcess.
            }
            // Follows the object loop.
            FrameStep::IonBlasts => {
                frame.bridge_state_changed |=
                    self.update_ion_blasts(rules, overlay_registry, effects);
            }
            FrameStep::CrateRegen => {
                // Called between `AlphaShapeClass::PurgeDisabled` and the
                // Tactical, Factory and House callbacks, on the
                // pre-increment frame counter this tick's steps have all
                // observed. The `Option` is a Rust availability gate with no
                // native counterpart: `MapClass__UpdateCrateRegenTimers` owns
                // its own two gates and nothing else. The sole production
                // caller (`SimRuntime::advance_frame`) always binds the
                // registry, and `crate_regen_rung_runs_in_every_production_frame`
                // pins that.
                if let Some(overlay_registry) = overlay_registry {
                    let regen =
                        crate::sim::crates::tick_crate_regeneration(self, rules, overlay_registry);
                    if regen.visible != 0 {
                        // Native Mark mutates live CellClass land/zone/bridge
                        // state synchronously. Rust's BridgeRuntimeState
                        // records are a derived cache, so refresh them once
                        // per pass that installed an overlay and let the
                        // existing frame-boundary navigation seam republish;
                        // this adds no RNG or ordering boundary.
                        //
                        // Deliberately not refreshed on a removal-only pass:
                        // only a Mark can stamp bridge topology, and the
                        // random placer refuses any cell that already carries
                        // an overlay, so a crate can never sit on — or
                        // un-stamp — a bridge piece under stock rules.
                        frame.bridge_state_changed |=
                            self.refresh_bridge_runtime_after_crate_mark();
                    }
                }
            }
            FrameStep::Factories => {
                // The authoritative factory sweep, before the House tail.
                // `step_all` charges each armed factory's per-step cost
                // against the REAL wallet (house.economy.credits) in
                // insertion_seq (temporal) order. Completed heads retain their
                // change flag until the next Strip prefix; its PLACE event
                // tail releases the object and advances the queue-of-record
                // there.
                //
                // Native55AFB0 visits all objects before55B66A's Factory
                // sweep. Depot service spends inside its Building mission
                // visit, so every factory observes the wallet after that
                // object's repair. The tank-bunker install runs in the
                // bunker's Repair mission.
                production::step_factories(self);
                #[cfg(any(test, feature = "test-support"))]
                self.trace_house_ai_activation_order(
                    HouseAiActivationOrderTestEvent::ProductionCompleted,
                );
            }
            FrameStep::Houses | FrameStep::FrameCommit | FrameStep::PendingDelete => {
                unreachable!("{step:?} runs in the frame tail")
            }
        }
        Ok(())
    }

    /// The frame tail: the House update (incl. defeat), `Queue_AI`'s command
    /// exchange and dispatch, then the late frame commit and pending-delete
    /// drain. Accumulates command placements into `frame`. A House outcome
    /// wait resumes here with its saved `cursor`.
    fn run_late_region(
        &mut self,
        inputs: FrameInputs<'_>,
        frame: &mut FrameCompletion,
        cursor: Option<house_defeat::HouseOutcomeCursor>,
    ) -> Result<FrameTailResult, FrameAdvanceError> {
        let FrameInputs {
            rules,
            overlay_registry,
            effects,
            ..
        } = inputs;
        if cursor.is_none() {
            self.enter_frame_step(FrameStep::Houses);
        }
        // The factory sweep has already run. This early House update assesses
        // its services before both defeat processing and strategic AI command
        // generation. Native anger decay is unconditional; only the activation
        // substep needs RuleSet.
        // Defeat detection, strategy and building choice: gamemd evaluates
        // each house's defeat before its AI manage/produce step. The gate
        // reads the house's tracking counts (`house_defeat.rs`), which
        // construction and the frame-end pending-delete drain move: a
        // death reaches the gate on the next frame. Each house's strategy tick
        // (`house_strategy.rs`) and building choice follow its own gate.
        let generated = match self.house_rung(
            rules,
            overlay_registry,
            self.session.tick() > 0,
            effects,
            cursor,
        ) {
            Ok(generated) => generated,
            Err(cursor) => return Ok(FrameTailResult::OutcomeWait(cursor)),
        };
        self.issue_generated_frame_commands(generated);
        // MainTick55DD9B -> PlanMgr637550 publishes Logic-generated plan
        // events before Queue_AI's CRC and network exchange, even if it waits.
        self.publish_planning_commands();
        #[cfg(any(test, feature = "test-support"))]
        if self.session.tick() > 0 {
            self.trace_house_ai_activation_order(HouseAiActivationOrderTestEvent::DefeatProcessed);
        }

        // `Queue_AI_Multiplayer` computes and stores the sync checksum
        // (`0x00647684`, history `0x00B04474[Frame & 0xFF]`) before any
        // command transfer or dispatch; the single-player body never does.
        // VERA appends money, health and trigger terms to the native fold.
        if self.session.game_mode().command_queue_route() == CommandQueueRoute::Multiplayer {
            self.frame_sync_checksum = Some(FrameSyncChecksum {
                frame: self.session.binary_frame(),
                checksum: self.compute_multiplayer_frame_checksum(),
            });
        }

        let context = if let Some(sync) = &self.frame_sync_checksum {
            let checksum = sync.checksum.as_ref().map_err(|error| {
                FrameAdvanceError::new(
                    self.session.tick(),
                    self.session.binary_frame(),
                    0,
                    format!("multiplayer checksum: {error}"),
                )
            })?;
            effects.record_frame_checksum(sync.frame, checksum.value);
            Some(NetworkQueueContext {
                frame: sync.frame,
                checksum: checksum.value,
            })
        } else {
            None
        };

        self.run_network_command_tail(inputs, frame, context)
    }

    /// Resume point inside Queue_AI after Logic and its CRC publication. Both
    /// a first visit and a network wait re-enter this one command/tail owner.
    fn run_network_command_tail(
        &mut self,
        inputs: FrameInputs<'_>,
        frame: &mut FrameCompletion,
        context: Option<NetworkQueueContext>,
    ) -> Result<FrameTailResult, FrameAdvanceError> {
        let FrameInputs {
            rules,
            overlay_registry,
            effects,
            ..
        } = inputs;
        let pass = if let Some(context) = context {
            match effects.exchange_commands(context, &mut self.network_command_queues()) {
                Ok(pass) => pass,
                Err(NetworkCommandError::Interrupted(reason)) => {
                    frame
                        .commands
                        .extend(std::mem::take(&mut self.frame_command_admissions));
                    return Ok(FrameTailResult::Interrupted(reason));
                }
                Err(error) => {
                    return Err(FrameAdvanceError::new(
                        self.session.tick(),
                        self.session.binary_frame(),
                        0,
                        format!("network exchange: {error}"),
                    ));
                }
            }
        } else {
            None
        };
        if pass == Some(NetworkQueuePass::Waiting) {
            frame
                .commands
                .extend(std::mem::take(&mut self.frame_command_admissions));
            return Ok(FrameTailResult::Waiting(context.expect("network context")));
        }

        // Queue_AI follows Logic, including Strip and House-generated events.
        // Keep local occupancy until this transfer; the earlier app drain is
        // only for already-admitted commands and local frame ingress.
        if pass.is_none() {
            self.transfer_local_commands();
        }

        // EventClass dispatch is a Main_Tick tail rung: the complete live
        // Logic walk observes frame N's pre-command state, so an accepted
        // command first changes that object's AI behavior on frame N+1.
        let result = if pass == Some(NetworkQueuePass::SkipDispatch) {
            Ok((0, false))
        } else {
            // All input is already admitted before Logic or by the network
            // queue view. `frame.commands` now carries only the replay journal,
            // including earlier wait polls; never admit that journal twice.
            self.dispatch_owned_commands(&[], rules, frame.execute_tick, overlay_registry, effects)
        };
        let failure = match result {
            Ok((executed, spawned)) => {
                frame.executed_commands += executed;
                frame.spawned_entities |= spawned;
                None
            }
            Err((NetworkCommandError::Mismatch(failure), executed, spawned)) => {
                frame.executed_commands += executed;
                frame.spawned_entities |= spawned;
                Some(failure)
            }
            Err((NetworkCommandError::Unavailable(cause), _, _)) => {
                return Err(FrameAdvanceError::new(
                    self.session.tick(),
                    self.session.binary_frame(),
                    0,
                    cause,
                ));
            }
            Err((NetworkCommandError::Interrupted(reason), _, _)) => {
                return Ok(FrameTailResult::Interrupted(reason));
            }
        };
        frame
            .commands
            .extend(std::mem::take(&mut self.frame_command_admissions));
        self.replaying_recorded_commands = false;
        if failure.is_some() {
            //64CCEB's modal precedes Queue_AI clearing GameActive648266.
            //No retirement, staged MegaMission execution or frame commit yet.
            return Ok(FrameTailResult::Checksum(
                failure.expect("checksum failure"),
            ));
        }
        Ok(FrameTailResult::Complete(
            self.commit_frame_tail(inputs, frame.execute_tick),
        ))
    }

    /// MainTick55DE7E..55DEA4 is also reached after a CRC modal returns.
    /// Independent victory/defeat/restart/EXIT flags still skip this tail.
    pub(super) fn commit_frame_tail(&mut self, inputs: FrameInputs<'_>, execute_tick: u64) -> bool {
        let FrameInputs {
            rules,
            overlay_registry,
            tick_ms,
            effects,
        } = inputs;
        // MainTick55DE45 -> PlanMgr637550 publishes command-generated plans
        // after Queue_AI, using any TIMING change it just applied. Native
        // publishes before checking the terminal flags55DE4F..55DE71 too.
        self.publish_planning_commands();
        // Main_Tick returns immediately on a terminal result. The wrapping
        // frame commit, pacing tail, and pending-delete drain are all skipped
        // on that same terminating call.
        if self.termination_frame_requested() {
            return false;
        }

        // Commit the wrapping native frame LATE, after every phase has observed
        // frame N. Stored-start timer consumers therefore capture N, and the
        // next admitted advance begins on N+1. Host-provided milliseconds are
        // retained for diagnostics only and never determine the frame.
        self.session.commit_native_frame(tick_ms);
        self.enter_frame_step(FrameStep::FrameCommit);
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::BinaryFrameCommitted);

        // The one ordinary ProcessPendingDelete drain follows the native frame
        // commit. Alive queue entries keep their position; ready duplicate IDs
        // collapse and physically finalize exactly once.
        self.enter_frame_step(FrameStep::PendingDelete);
        self.process_pending_delete_with(
            rules,
            overlay_registry,
            effects,
            1,
            self.session.started(),
        );

        // MainTick55DEA4 follows Frame++ and ProcessPendingDelete725C70.
        // Network waits and terminal transitions never enter this tail.
        self.prune_invalid_planning_nodes();

        // Validate actual membership after the drain. Rebuilding from current
        // phase/terrain would erase legitimate retained Cell list history.
        #[cfg(debug_assertions)]
        if std::env::var("OCCUPANCY_DEBUG").is_ok() {
            debug_assert!(
                self.substrate
                    .occupancy
                    .validate_memberships(&self.substrate.entities)
                    .is_ok()
            );
        }

        // The separate Rust session tick has no direct native field; preserve
        // its existing post-debug-validation relation.
        self.session.complete_tick(execute_tick);
        true
    }

    /// Fixture-only frame adapter (F09): unit tests drive one Main_Tick-shaped
    /// frame with explicitly supplied rules/heights/navigation, without the
    /// trigger poll. Production and tooling advance exclusively through
    /// `SimRuntime::advance_frame`, whose resources are bound at construction
    /// and cannot be substituted per call. A fixture that passes no rules runs
    /// the production frame on the rules an empty INI reads to
    /// ([`RuleSet::empty_for_test`]).
    #[cfg(any(test, feature = "test-support"))]
    pub fn advance_tick(
        &mut self,
        commands: &[CommandEnvelope],
        rules: Option<&RuleSet>,
        path_grid: Option<&PathGrid>,
        overlay_registry: Option<&OverlayTypeRegistry>,
        tick_ms: u32,
    ) -> TickResult {
        self.install_fixture_path_grid(path_grid);
        self.advance_master_frame(
            commands,
            rules.unwrap_or(RuleSet::empty_for_test()),
            overlay_registry,
            tick_ms,
            TickLane::Ordinary,
            false,
            FrameEffects::default(),
        )
        .expect("fixture frame must complete")
        .into_tick()
    }

    /// App-facing authoritative frame transaction.
    ///
    /// The app submits commands and immutable map/rules inputs, then consumes
    /// the returned facts instead of reaching back into Simulation-owned
    /// transient queues. `SimRuntime::advance_frame` is the sole production
    /// caller (F09); headless and replay execution go through the runtime.
    /// Every frame polls the installed scenario triggers.
    pub fn advance_app_frame(
        &mut self,
        commands: &[CommandEnvelope],
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        tick_ms: u32,
        lane: TickLane,
        effects: FrameEffects<'_>,
    ) -> Result<SimFrameOutput, FrameAdvanceError> {
        let frame = self.advance_master_frame(
            commands,
            rules,
            overlay_registry,
            tick_ms,
            lane,
            true,
            effects,
        );
        // A failed frame must not carry a previous caller's derived interest
        // into a later headless, replay or ordinary presentation frame.
        self.unit_voice_visit_interest.clear();
        Ok(self.collect_frame_output(frame?, effects))
    }

    /// HouseClass::Update4F860F/4F8736 has not yet published its request.
    pub fn pending_outcome_wait(&self) -> Option<crate::sim::house_state::HouseOutcomeKind> {
        match &self.frame_continuation {
            Some(FrameContinuation::Outcome(pending)) => Some(pending.cursor.kind),
            _ => None,
        }
    }

    pub(crate) fn validate_frame_continuation_for_restore(&self) -> Result<(), &'static str> {
        if let Some(FrameContinuation::Outcome(pending)) = &self.frame_continuation {
            let owner = self
                .houses
                .order()
                .get(pending.cursor.index)
                .ok_or("outcome cursor has no House")?;
            let kind = self.houses.get(owner).and_then(|house| {
                house.pending_outcome(
                    self.session.binary_frame() as i32,
                    self.session.game_mode_nonzero(),
                )
            });
            if kind != Some(pending.cursor.kind) {
                return Err("outcome cursor disagrees with the waiting House result");
            }
            if pending.frame.execute_tick != self.session.tick().saturating_add(1) {
                return Err("outcome cursor disagrees with the suspended frame");
            }
        }
        Ok(())
    }

    /// Resume the saved House head, then its suffix and remaining frame.
    /// Earlier object/House visits and input admission are never repeated.
    pub fn resume_outcome_wait(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<SimFrameOutput, FrameAdvanceError> {
        if self.pending_outcome_wait().is_none() {
            return Err(FrameAdvanceError::new(
                self.session.tick(),
                self.session.binary_frame(),
                0,
                "no House outcome wait is pending".to_owned(),
            ));
        }
        let Some(FrameContinuation::Outcome(mut pending)) = self.frame_continuation.take() else {
            unreachable!("checked House outcome continuation")
        };
        let inputs = FrameInputs::new(rules, overlay_registry, pending.tick_ms, effects);
        let result = self.run_late_region(inputs, &mut pending.frame, Some(pending.cursor))?;
        let output = self.complete_or_suspend_frame(
            pending.frame,
            result,
            inputs,
            #[cfg(feature = "telemetry")]
            None,
        );
        self.unit_voice_visit_interest.clear();
        Ok(self.collect_frame_output(output, inputs.effects))
    }

    /// The modal is client-owned; this world-owned continuation identifies
    /// the suspended native frame without permitting another Logic pass.
    pub fn pending_network_failure(&self) -> Option<crate::sim::command_queue::ChecksumMismatch> {
        match self.frame_continuation.as_ref() {
            Some(FrameContinuation::Checksum(pending)) => Some(pending.failure),
            _ => None,
        }
    }

    pub fn pending_network_wait(&self) -> bool {
        matches!(self.frame_continuation, Some(FrameContinuation::Waiting(_)))
    }

    pub fn pending_network_interruption(&self) -> Option<NetworkInterruption> {
        match self.frame_continuation.as_ref() {
            Some(FrameContinuation::Interrupted(pending)) => Some(pending.interruption),
            _ => None,
        }
    }

    /// The app's native wait timeout supplies the failure result; no second
    /// Logic pass or attempted command decode runs before its error modal.
    pub fn interrupt_network_wait(&mut self, interruption: NetworkInterruption) -> bool {
        let Some(FrameContinuation::Waiting(_)) = self.frame_continuation.as_ref() else {
            return false;
        };
        let Some(FrameContinuation::Waiting(pending)) = self.frame_continuation.take() else {
            unreachable!("checked wait continuation")
        };
        self.frame_continuation = Some(FrameContinuation::Interrupted(
            NetworkInterruptionContinuation {
                interruption,
                tick_ms: pending.tick_ms,
                frame: pending.frame,
            },
        ));
        true
    }

    /// Resume only MainTick's tail after Stop acknowledges64CCEB's modal.
    /// Queue_AI's false result clears GameActive but does not set EXIT; the
    /// caller returns through normal network-shell cleanup after this output.
    pub fn resume_after_network_failure(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<SimFrameOutput, FrameAdvanceError> {
        if !matches!(
            self.frame_continuation,
            Some(FrameContinuation::Checksum(_) | FrameContinuation::Interrupted(_))
        ) {
            return Err(FrameAdvanceError::new(
                self.session.tick(),
                self.session.binary_frame(),
                0,
                "no checksum modal frame is pending".to_owned(),
            ));
        }
        let (tick_ms, frame) = match self.frame_continuation.take() {
            Some(FrameContinuation::Checksum(pending)) => (pending.tick_ms, pending.frame),
            Some(FrameContinuation::Interrupted(pending)) => (pending.tick_ms, pending.frame),
            _ => unreachable!("checked fatal network continuation"),
        };
        Ok(self.finish_aborted_network_frame(
            frame,
            FrameInputs::new(rules, overlay_registry, tick_ms, effects),
        ))
    }

    /// Wait result8 (Quit Game) has no additional error modal. Native still
    /// returns to MainTick's commit tail before the game exits to the shell.
    pub fn leave_network_wait(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<SimFrameOutput, FrameAdvanceError> {
        if !self.pending_network_wait() {
            return Err(FrameAdvanceError::new(
                self.session.tick(),
                self.session.binary_frame(),
                0,
                "no network wait frame is pending".to_owned(),
            ));
        }
        let Some(FrameContinuation::Waiting(pending)) = self.frame_continuation.take() else {
            unreachable!("checked wait continuation")
        };
        Ok(self.finish_aborted_network_frame(
            pending.frame,
            FrameInputs::new(rules, overlay_registry, pending.tick_ms, effects),
        ))
    }

    fn finish_aborted_network_frame(
        &mut self,
        frame: FrameCompletion,
        inputs: FrameInputs<'_>,
    ) -> SimFrameOutput {
        let committed = self.commit_frame_tail(inputs, frame.execute_tick);
        let mut frame = self.finish_master_frame(
            frame,
            committed,
            inputs,
            #[cfg(feature = "telemetry")]
            None,
        );
        // This is the terminal match frame even when its native commit ran.
        frame.tick.state_hash = Some(self.state_hash());
        self.unit_voice_visit_interest.clear();
        self.collect_frame_output(frame, inputs.effects)
    }

    /// Poll the saved post-Logic Queue_AI visit without re-running House,
    /// object updates, CRC publication or command producers. Each returned
    /// output contains only effects not already delivered by earlier polls.
    pub fn resume_network_wait(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<SimFrameOutput, FrameAdvanceError> {
        if !self.pending_network_wait() {
            return Err(FrameAdvanceError::new(
                self.session.tick(),
                self.session.binary_frame(),
                0,
                "no network wait frame is pending".to_owned(),
            ));
        }
        let Some(FrameContinuation::Waiting(mut pending)) = self.frame_continuation.take() else {
            unreachable!("checked wait continuation")
        };
        let inputs = FrameInputs::new(rules, overlay_registry, pending.tick_ms, effects);
        let result =
            self.run_network_command_tail(inputs, &mut pending.frame, Some(pending.context));
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                self.frame_continuation = Some(FrameContinuation::Waiting(pending));
                return Err(error);
            }
        };
        let output = self.complete_or_suspend_frame(
            pending.frame,
            result,
            inputs,
            #[cfg(feature = "telemetry")]
            None,
        );
        Ok(self.collect_frame_output(output, inputs.effects))
    }

    fn collect_frame_output(
        &mut self,
        frame: MasterFrameOutput,
        effects: FrameEffects<'_>,
    ) -> SimFrameOutput {
        let MasterFrameOutput {
            tick,
            admitted_commands,
            network_failure,
            network_waiting,
            outcome_waiting,
        } = frame;
        let lighting_events = if outcome_waiting {
            Vec::new()
        } else {
            self.take_lighting_events()
        };
        let trigger_effects = if outcome_waiting {
            Vec::new()
        } else {
            std::mem::take(&mut self.trigger_effects)
        };
        // Preserve the established terminal-frame gate: these are committed
        // light-vector facts and the next admitted frame clears the producer
        // buffer before combat runs.
        let combat_lights = if tick.frame_committed {
            self.combat_light_requests.take()
        } else {
            Vec::new()
        };
        // Shake writes happen synchronously at the detonation prologue, before
        // a possible later frame wait. They are already-observed process
        // presentation effects, so hand them off even when Logic suspends.
        let screen_shake_updates = self.screen_shake_updates.take();
        let lifecycle_outputs = if outcome_waiting {
            Vec::new()
        } else {
            self.lifecycle_outputs.take()
        };
        let multiplayer_checksum = if outcome_waiting {
            None
        } else {
            self.frame_sync_checksum.take()
        };
        let overlay_updates = if outcome_waiting {
            Vec::new()
        } else {
            std::mem::take(&mut self.frame_overlay_updates)
        };
        let overlay_removals = if outcome_waiting {
            Vec::new()
        } else {
            std::mem::take(&mut self.frame_overlay_removals)
        };
        let fire_events = if outcome_waiting {
            Vec::new()
        } else {
            self.fire_events.take()
        };
        let sound_events = self.sound_events.take();
        let sound_events_delivered = effects.delivered_sound_events();
        assert!(sound_events_delivered <= sound_events.len());
        SimFrameOutput {
            network_waiting,
            outcome_waiting,
            network_failure,
            admitted_commands,
            tick,
            trigger_effects,
            lifecycle_outputs,
            overlay_updates,
            overlay_removals,
            sound_events,
            sound_events_delivered,
            fire_events,
            combat_lights,
            screen_shake_updates,
            lighting_events,
            multiplayer_checksum,
        }
    }

    fn complete_or_suspend_frame(
        &mut self,
        mut frame: FrameCompletion,
        result: FrameTailResult,
        inputs: FrameInputs<'_>,
        #[cfg(feature = "telemetry")] frame_clock: Option<crate::sim::frame_profile::FrameClock>,
    ) -> MasterFrameOutput {
        let tick_ms = inputs.tick_ms;
        // A MindControl shot changes an existing SHP's house-colour key.
        // Publish the atlas invalidation before the next draw, even with no spawn.
        frame.ownership_changed |= std::mem::take(&mut self.ownership_changed_since_output);
        let frame_committed = if let FrameTailResult::Complete(committed) = result {
            committed
        } else {
            let tick = TickResult {
                tick: self.session.tick(),
                frame_committed: false,
                executed_commands: frame.executed_commands,
                state_hash: None,
                terminal_score_finalized: false,
                spawned_entities: frame.spawned_entities,
                destroyed_structure: frame.destroyed_structure,
                ownership_changed: frame.ownership_changed,
                bridge_state_changed: frame.bridge_state_changed,
            };
            let (network_failure, network_waiting, outcome_waiting) = match result {
                FrameTailResult::Checksum(failure) => {
                    self.frame_continuation =
                        Some(FrameContinuation::Checksum(NetworkFailureContinuation {
                            failure,
                            tick_ms,
                            frame,
                        }));
                    (Some(failure), false, false)
                }
                FrameTailResult::Waiting(context) => {
                    self.frame_continuation =
                        Some(FrameContinuation::Waiting(NetworkWaitContinuation {
                            context,
                            tick_ms,
                            frame,
                        }));
                    (None, true, false)
                }
                FrameTailResult::Interrupted(interruption) => {
                    self.frame_continuation = Some(FrameContinuation::Interrupted(
                        NetworkInterruptionContinuation {
                            interruption,
                            tick_ms,
                            frame,
                        },
                    ));
                    (None, false, false)
                }
                FrameTailResult::OutcomeWait(cursor) => {
                    self.frame_continuation =
                        Some(FrameContinuation::Outcome(HouseOutcomeContinuation {
                            cursor,
                            tick_ms,
                            frame,
                        }));
                    (None, false, true)
                }
                FrameTailResult::Complete(_) => unreachable!(),
            };
            return MasterFrameOutput {
                tick,
                admitted_commands: Vec::new(),
                network_failure,
                network_waiting,
                outcome_waiting,
            };
        };
        self.finish_master_frame(
            frame,
            frame_committed,
            inputs,
            #[cfg(feature = "telemetry")]
            frame_clock,
        )
    }

    /// Finish the same frame after the single commit/pending-delete tail.
    fn finish_master_frame(
        &mut self,
        frame: FrameCompletion,
        frame_committed: bool,
        inputs: FrameInputs<'_>,
        #[cfg(feature = "telemetry")] mut frame_clock: Option<
            crate::sim::frame_profile::FrameClock,
        >,
    ) -> MasterFrameOutput {
        let FrameInputs {
            rules,
            overlay_registry,
            ..
        } = inputs;
        let FrameCompletion {
            commands,
            execute_tick: _,
            executed_commands,
            mut spawned_entities,
            destroyed_structure,
            mut ownership_changed,
            mut bridge_state_changed,
        } = frame;
        macro_rules! profile_phase {
            ($phase:ident) => {
                #[cfg(feature = "telemetry")]
                if let Some(clock) = frame_clock.as_mut() {
                    clock.stamp(crate::sim::frame_profile::phase::$phase);
                }
            };
        }
        spawned_entities |= std::mem::take(&mut self.mission_spawned_entities);
        bridge_state_changed |= self
            .trigger_effects
            .iter()
            .any(|effect| matches!(effect, TriggerEffect::BridgeStateChanged));
        self.frame_overlay_updates = self.finalize_frame_overlays_and_navigation(
            rules,
            overlay_registry,
            bridge_state_changed,
            destroyed_structure || spawned_entities,
        );
        profile_phase!(OVERLAYS);
        #[cfg(debug_assertions)]
        self.debug_assert_logic_membership_consistent();
        #[cfg(debug_assertions)]
        self.debug_assert_lifecycle_consistent();
        #[cfg(debug_assertions)]
        self.debug_assert_factory_invariants();
        profile_phase!(DEBUG_ASSERTS);

        // Living sprite animation state belongs to the committed simulation
        // frame. Keep it inside the authoritative frame transaction so hashed
        // side effects and every snapshot observe the same state. Dying
        // animation advancement remains in the live-object scheduler, which
        // owns the single pending-delete drain above.
        if frame_committed {
            let game_options = self.session.game_options().clone();
            let binary_frame = self.session.binary_frame();
            let (entities, interner) = self.entities_mut_and_interner();
            animation::tick_non_dying_animations(
                entities,
                rules.animation_sequences(),
                rules,
                &game_options,
                interner,
                binary_frame,
            );
        }
        #[cfg(any(test, feature = "test-support"))]
        building_anim::finalize(self);
        profile_phase!(ANIMS);
        #[cfg(debug_assertions)]
        self.debug_assert_logic_membership_consistent();
        #[cfg(debug_assertions)]
        self.debug_assert_lifecycle_consistent();
        profile_phase!(DEBUG_ASSERTS);
        // Native GameExit mode-0 victory/defeat branches skip both
        // ScoreDialog__FillEntries `0x5C9700` and `ScoreDialog__Run `0x5C9720`.
        // Keep their survivor bonus and Scenario RNG draws out of campaigns;
        // Campaign's standalone result dialog reads its own House statistics.
        let terminal_score_finalized = self.session.game_mode_nonzero()
            && self.natural_outcome_exit_ready()
            && Self::finalize_terminal_score_snapshot(
                &self.session,
                &mut self.scenario_rng,
                &mut self.terminal_score_snapshot,
                &self.houses,
            );
        profile_phase!(SCORE);
        // A frame that ends the match hashes whatever its tick: the replay
        // log records it as its last frame.
        let state_hash = (!frame_committed
            || self
                .state_hash_interval
                .is_some_and(|frames| self.session.tick().is_multiple_of(frames.get())))
        .then(|| self.state_hash());
        profile_phase!(HASH);
        #[cfg(feature = "telemetry")]
        if let Some(clock) = frame_clock {
            let (phases, total) = clock.finish();
            self.frame_profile.record(phases, total);
        }
        // Resuming a suspended frame can transfer ownership in its remaining tail.
        ownership_changed |= std::mem::take(&mut self.ownership_changed_since_output);
        MasterFrameOutput {
            network_failure: None,
            network_waiting: false,
            outcome_waiting: false,
            admitted_commands: commands,
            tick: TickResult {
                tick: self.session.tick(),
                frame_committed,
                executed_commands,
                state_hash,
                terminal_score_finalized,
                spawned_entities,
                destroyed_structure,
                ownership_changed,
                bridge_state_changed,
            },
        }
    }
}
