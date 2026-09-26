//! Pure condition-driven script for the hidden tactical radar checkpoint.
//!
//! App/session code snapshots renderer-free production observations, translates
//! returned actions into ordinary simulation commands, and records the actual
//! scheduled tick. This module never mutates simulation state itself.

use crate::app::types::SIM_TICK_MS;

use super::placement::PlacementChoice;

const PRODUCTION_PROGRESS_INTERVALS: u64 = 53;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum StructureRole {
    Power,
    Refinery,
    Radar,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DeploymentContract {
    pub mcv_type_id: String,
    pub yard_type_id: String,
    pub deploy_facing: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ProductionTargetContract {
    pub role: StructureRole,
    pub type_id: String,
    pub expected_rate_frames: u16,
    pub expected_ready_tick: u64,
    pub expected_active_tick: u64,
    /// Ticks from the ready observation to construction complete: the place
    /// command's one-tick delay plus the type's build-up.
    pub construction_ticks: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct StageBudget {
    pub max_ticks: u64,
    pub max_wall_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct TacticalStageBudgets {
    pub deploy_yard: StageBudget,
    pub power_production: StageBudget,
    pub power_placement: StageBudget,
    pub refinery_production: StageBudget,
    pub refinery_placement: StageBudget,
    pub radar_production: StageBudget,
    pub radar_placement: StageBudget,
    pub radar_opening: StageBudget,
    pub stable_frames: StageBudget,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct TacticalExpectedLedger {
    pub yard_active_tick: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct TacticalScriptConfig {
    pub owner: String,
    pub deployment: DeploymentContract,
    pub power: ProductionTargetContract,
    pub refinery: ProductionTargetContract,
    pub radar: ProductionTargetContract,
    pub refinery_harvester_type_id: Option<String>,
    pub placement_radius: u16,
    pub warm_frames: u16,
    pub overall_tick_cap: u64,
    pub budgets: TacticalStageBudgets,
    pub expected: TacticalExpectedLedger,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum TacticalScriptConfigError {
    #[error("tactical script identity fields must be non-empty")]
    EmptyIdentity,
    #[error("production targets must be ordered power, refinery, radar")]
    WrongTargetRoles,
    #[error("power, refinery, and radar targets must be distinct")]
    DuplicateTargets,
    #[error("production target rates must be nonzero")]
    EmptyProductionRate,
    #[error("production target construction durations must be nonzero")]
    EmptyConstruction,
    #[error("all tactical stage budgets must be nonzero")]
    EmptyBudget,
    #[error("tactical expected-ledger arithmetic overflowed")]
    LedgerOverflow,
    #[error("{role:?} schedule-to-ready ledger does not match 2 + 53 * rate")]
    ProductionReadyLedger { role: StructureRole },
    #[error("{role:?} ready-to-active ledger differs from its construction duration")]
    ConstructionLedger { role: StructureRole },
    #[error("expected capture exceeds the overall tick cap")]
    OverallTickCap,
}

impl TacticalScriptConfig {
    fn validate(&self) -> Result<(), TacticalScriptConfigError> {
        if self.owner.trim().is_empty()
            || self.deployment.mcv_type_id.trim().is_empty()
            || self.deployment.yard_type_id.trim().is_empty()
            || self.power.type_id.trim().is_empty()
            || self.refinery.type_id.trim().is_empty()
            || self.radar.type_id.trim().is_empty()
            || self
                .refinery_harvester_type_id
                .as_ref()
                .is_some_and(|type_id| type_id.trim().is_empty())
        {
            return Err(TacticalScriptConfigError::EmptyIdentity);
        }
        if self.power.role != StructureRole::Power
            || self.refinery.role != StructureRole::Refinery
            || self.radar.role != StructureRole::Radar
        {
            return Err(TacticalScriptConfigError::WrongTargetRoles);
        }
        if self
            .power
            .type_id
            .eq_ignore_ascii_case(&self.refinery.type_id)
            || self.power.type_id.eq_ignore_ascii_case(&self.radar.type_id)
            || self
                .refinery
                .type_id
                .eq_ignore_ascii_case(&self.radar.type_id)
        {
            return Err(TacticalScriptConfigError::DuplicateTargets);
        }
        if [
            self.power.expected_rate_frames,
            self.refinery.expected_rate_frames,
            self.radar.expected_rate_frames,
        ]
        .contains(&0)
        {
            return Err(TacticalScriptConfigError::EmptyProductionRate);
        }
        if [
            self.power.construction_ticks,
            self.refinery.construction_ticks,
            self.radar.construction_ticks,
        ]
        .contains(&0)
        {
            return Err(TacticalScriptConfigError::EmptyConstruction);
        }
        let budgets = [
            self.budgets.deploy_yard,
            self.budgets.power_production,
            self.budgets.power_placement,
            self.budgets.refinery_production,
            self.budgets.refinery_placement,
            self.budgets.radar_production,
            self.budgets.radar_placement,
            self.budgets.radar_opening,
            self.budgets.stable_frames,
        ];
        if budgets
            .iter()
            .any(|budget| budget.max_ticks == 0 || budget.max_wall_ms == 0)
        {
            return Err(TacticalScriptConfigError::EmptyBudget);
        }

        for (target, start_tick) in [
            (&self.power, self.expected.yard_active_tick),
            (&self.refinery, self.power.expected_active_tick),
            (&self.radar, self.refinery.expected_active_tick),
        ] {
            let progress_ticks = PRODUCTION_PROGRESS_INTERVALS
                .checked_mul(u64::from(target.expected_rate_frames))
                .ok_or(TacticalScriptConfigError::LedgerOverflow)?;
            let expected_ready_tick = start_tick
                .checked_add(2)
                .and_then(|tick| tick.checked_add(progress_ticks))
                .ok_or(TacticalScriptConfigError::LedgerOverflow)?;
            if target.expected_ready_tick != expected_ready_tick {
                return Err(TacticalScriptConfigError::ProductionReadyLedger { role: target.role });
            }
            let expected_active_tick = target
                .expected_ready_tick
                .checked_add(target.construction_ticks)
                .ok_or(TacticalScriptConfigError::LedgerOverflow)?;
            if target.expected_active_tick != expected_active_tick {
                return Err(TacticalScriptConfigError::ConstructionLedger { role: target.role });
            }
        }
        // The opening timer belongs to actual render wall time, so its event
        // tick is observed. Bound the worst admitted opening + warm-frame work.
        let maximum_capture = self
            .radar
            .expected_active_tick
            .checked_add(self.budgets.radar_opening.max_ticks)
            .and_then(|tick| tick.checked_add(1 + u64::from(self.warm_frames)))
            .ok_or(TacticalScriptConfigError::LedgerOverflow)?;
        if maximum_capture > self.overall_tick_cap {
            return Err(TacticalScriptConfigError::OverallTickCap);
        }
        Ok(())
    }

    fn target(&self, role: StructureRole) -> &ProductionTargetContract {
        match role {
            StructureRole::Power => &self.power,
            StructureRole::Refinery => &self.refinery,
            StructureRole::Radar => &self.radar,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct TacticalEntityObservation {
    pub stable_id: u64,
    pub owner: String,
    pub type_id: String,
    pub cell: (u16, u16),
    pub facing: u8,
    pub active: bool,
    pub dying: bool,
    pub building_up: bool,
}

impl TacticalEntityObservation {
    fn live(&self) -> bool {
        self.active && !self.dying
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct BuildOptionObservation {
    pub type_id: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ProductionQueueObservation {
    pub type_id: String,
    /// Live active factory cadence. A queued tail that has not yet become the
    /// active factory object is represented by the rate-zero sentinel.
    pub resolved_rate_frames: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct TacticalObservation {
    pub tick: u64,
    pub total_sim_ms: u64,
    pub binary_frame: u32,
    pub wall_elapsed_ms: u64,
    pub accepted_rust_l0: bool,
    pub in_game: bool,
    pub local_owner: String,
    pub match_ended: bool,
    /// True only when the adapter built this list from the strict live view,
    /// without prototype-relaxed fallback.
    pub build_options_strict: bool,
    pub entities: Vec<TacticalEntityObservation>,
    pub build_options: Vec<BuildOptionObservation>,
    pub queued_production: Vec<ProductionQueueObservation>,
    pub ready_buildings: Vec<String>,
    pub placement_choice: Option<PlacementChoice>,
    pub power_authority_sufficient: bool,
    pub radar_authority_active: bool,
    pub radar_online: bool,
    pub readiness_complete: bool,
    pub capture_complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum DeployAttempt {
    First,
    Second,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum ScriptCommandPayload {
    DeployMcv {
        entity_id: u64,
        attempt: DeployAttempt,
    },
    QueueExactType {
        role: StructureRole,
        type_id: String,
    },
    PlaceExactType {
        role: StructureRole,
        choice: PlacementChoice,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum ExpectedCommandResult {
    McvTurnOrYard {
        mcv_id: u64,
        yard_type_id: String,
        deploy_facing: u8,
    },
    YardCreated {
        mcv_id: u64,
        yard_type_id: String,
    },
    QueueOrReady {
        type_id: String,
        expected_rate_frames: u16,
    },
    BuildingPlacedReadyConsumed {
        type_id: String,
        cell: (u16, u16),
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum ResolvedCommandResult {
    McvTurned {
        mcv_id: u64,
        facing: u8,
    },
    YardObserved {
        stable_id: u64,
        cell: (u16, u16),
    },
    QueueObserved {
        type_id: String,
        resolved_rate_frames: u16,
    },
    ReadyObserved {
        type_id: String,
    },
    BuildingObserved {
        stable_id: u64,
        type_id: String,
        cell: (u16, u16),
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct PendingCommand {
    pub action_id: u64,
    pub scheduled_tick: u64,
    pub execute_tick: u64,
    pub owner: String,
    pub payload: ScriptCommandPayload,
    pub expected_result: ExpectedCommandResult,
    pub resolved_result: Option<ResolvedCommandResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum TacticalAction {
    DeployMcv {
        action_id: u64,
        owner: String,
        entity_id: u64,
        attempt: DeployAttempt,
    },
    QueueExactType {
        action_id: u64,
        owner: String,
        role: StructureRole,
        type_id: String,
    },
    PlaceExactType {
        action_id: u64,
        owner: String,
        role: StructureRole,
        choice: PlacementChoice,
    },
    Capture,
    Complete,
    Fail {
        failure: TacticalFailure,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum TacticalScriptStage {
    AwaitRustL0,
    AwaitFirstDeployResult,
    NeedSecondDeploy,
    AwaitSecondDeployResult,
    AwaitYardConstruction,
    AwaitPowerQueueResult,
    AwaitPowerReady,
    AwaitPowerPlacementResult,
    AwaitPowerConstruction,
    AwaitRefineryQueueResult,
    AwaitRefineryReady,
    AwaitRefineryPlacementResult,
    AwaitRefineryConstruction,
    AwaitRadarQueueResult,
    AwaitRadarReady,
    AwaitRadarPlacementResult,
    AwaitRadarConstruction,
    AwaitRadarOnline,
    WarmStableFrames,
    CaptureRequested,
    Complete,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum BudgetStage {
    DeployYard,
    PowerProduction,
    PowerPlacement,
    RefineryProduction,
    RefineryPlacement,
    RadarProduction,
    RadarPlacement,
    RadarOpening,
    StableFrames,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum TacticalFailureCode {
    ClockDrift,
    BudgetExceeded,
    ObservationInvalid,
    CommandScheduleInvalid,
    CommandResultMissing,
    ExpectedLedgerDrift,
    PlacementUnavailable,
    MatchEnded,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct TacticalFailure {
    pub code: TacticalFailureCode,
    pub stage: TacticalScriptStage,
    pub tick: u64,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ObservedStructure {
    pub stable_id: u64,
    pub type_id: String,
    pub cell: (u16, u16),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct StructureBindings {
    pub yard: Option<ObservedStructure>,
    pub power: Option<ObservedStructure>,
    pub refinery: Option<ObservedStructure>,
    pub radar: Option<ObservedStructure>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct HarvesterObservation {
    pub stable_id: u64,
    pub type_id: String,
    pub cell: (u16, u16),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct TacticalObservedLedger {
    pub rust_l0_tick: Option<u64>,
    pub yard_active_tick: Option<u64>,
    pub power_ready_tick: Option<u64>,
    pub power_active_tick: Option<u64>,
    pub refinery_ready_tick: Option<u64>,
    pub refinery_active_tick: Option<u64>,
    pub radar_ready_tick: Option<u64>,
    pub radar_active_tick: Option<u64>,
    pub radar_online_tick: Option<u64>,
    pub second_readiness_tick: Option<u64>,
    pub capture_requested_tick: Option<u64>,
    pub capture_complete_tick: Option<u64>,
}

#[derive(Debug, Clone)]
struct IssuedCommand {
    action_id: u64,
    issued_tick: u64,
    owner: String,
    payload: ScriptCommandPayload,
    expected_result: ExpectedCommandResult,
}

#[derive(Debug, Clone)]
struct ScriptViolation {
    code: TacticalFailureCode,
    message: String,
}

type ScriptResult<T> = Result<T, ScriptViolation>;

#[derive(Debug)]
pub(crate) struct TacticalScript {
    config: TacticalScriptConfig,
    stage: TacticalScriptStage,
    budget_stage: Option<BudgetStage>,
    budget_start_tick: u64,
    budget_start_wall_ms: u64,
    last_tick: Option<u64>,
    last_wall_ms: Option<u64>,
    next_action_id: u64,
    issued: Option<IssuedCommand>,
    pending: Option<PendingCommand>,
    command_ledger: Vec<PendingCommand>,
    placements: Vec<PlacementChoice>,
    bindings: StructureBindings,
    mcv_id: Option<u64>,
    harvester: Option<HarvesterObservation>,
    observed: TacticalObservedLedger,
    failure: Option<TacticalFailure>,
    terminal_emitted: bool,
}

impl TacticalScript {
    pub(crate) fn new(config: TacticalScriptConfig) -> Result<Self, TacticalScriptConfigError> {
        config.validate()?;
        Ok(Self {
            config,
            stage: TacticalScriptStage::AwaitRustL0,
            budget_stage: None,
            budget_start_tick: 0,
            budget_start_wall_ms: 0,
            last_tick: None,
            last_wall_ms: None,
            next_action_id: 1,
            issued: None,
            pending: None,
            command_ledger: Vec::new(),
            placements: Vec::new(),
            bindings: StructureBindings::default(),
            mcv_id: None,
            harvester: None,
            observed: TacticalObservedLedger::default(),
            failure: None,
            terminal_emitted: false,
        })
    }

    pub(crate) fn stage(&self) -> TacticalScriptStage {
        self.stage
    }

    #[cfg(test)]
    pub(crate) fn pending_command(&self) -> Option<&PendingCommand> {
        self.pending.as_ref()
    }

    pub(crate) fn command_ledger(&self) -> &[PendingCommand] {
        &self.command_ledger
    }

    pub(crate) fn placement_ledger(&self) -> &[PlacementChoice] {
        &self.placements
    }

    pub(crate) fn structure_bindings(&self) -> &StructureBindings {
        &self.bindings
    }

    pub(crate) fn harvester_observation(&self) -> Option<&HarvesterObservation> {
        self.harvester.as_ref()
    }

    pub(crate) fn observed_ledger(&self) -> &TacticalObservedLedger {
        &self.observed
    }

    /// Consume one renderer-free snapshot and yield at most one action.
    ///
    /// A returned command action must immediately be passed through
    /// `commands::try_schedule_command`, followed by exactly one
    /// `record_scheduled` call with its returned execute tick. Until then (and
    /// while a scheduled command is pending) no second command is emitted.
    pub(crate) fn next_action(
        &mut self,
        observation: &TacticalObservation,
    ) -> Option<TacticalAction> {
        if self.stage == TacticalScriptStage::Failed {
            if self.terminal_emitted {
                return None;
            }
            self.terminal_emitted = true;
            return self
                .failure
                .clone()
                .map(|failure| TacticalAction::Fail { failure });
        }
        if self.stage == TacticalScriptStage::Complete {
            return None;
        }

        match self.advance(observation) {
            Ok(action) => action,
            Err(violation) => {
                let failure = self.set_failure(violation, observation.tick);
                self.terminal_emitted = true;
                Some(TacticalAction::Fail { failure })
            }
        }
    }

    /// Attach the live command queue's execute tick to the one issued action.
    pub(crate) fn record_scheduled(
        &mut self,
        action_id: u64,
        scheduled_tick: u64,
        execute_tick: u64,
    ) -> Result<(), TacticalFailure> {
        let Some(issued) = self.issued.take() else {
            return Err(self.set_failure(
                violation(
                    TacticalFailureCode::CommandScheduleInvalid,
                    "recorded a command schedule while no action awaited scheduling",
                ),
                scheduled_tick,
            ));
        };
        let expected_execute_tick = issued.issued_tick;
        if issued.action_id != action_id
            || issued.issued_tick != scheduled_tick
            || execute_tick != expected_execute_tick
        {
            return Err(self.set_failure(
                violation(
                    TacticalFailureCode::CommandScheduleInvalid,
                    format!(
                        "schedule mismatch: issued action {} at tick {}, recorded action {} at tick {} execute {}, expected {}",
                        issued.action_id,
                        issued.issued_tick,
                        action_id,
                        scheduled_tick,
                        execute_tick,
                        expected_execute_tick
                    ),
                ),
                scheduled_tick,
            ));
        }

        self.stage = match &issued.payload {
            ScriptCommandPayload::DeployMcv {
                attempt: DeployAttempt::First,
                ..
            } => TacticalScriptStage::AwaitFirstDeployResult,
            ScriptCommandPayload::DeployMcv {
                attempt: DeployAttempt::Second,
                ..
            } => TacticalScriptStage::AwaitSecondDeployResult,
            ScriptCommandPayload::QueueExactType { role, .. } => queue_result_stage(*role),
            ScriptCommandPayload::PlaceExactType { role, .. } => placement_result_stage(*role),
        };
        self.pending = Some(PendingCommand {
            action_id,
            scheduled_tick,
            execute_tick,
            owner: issued.owner,
            payload: issued.payload,
            expected_result: issued.expected_result,
            resolved_result: None,
        });
        Ok(())
    }

    /// Mark a failed `try_schedule_command` result. The script becomes terminal
    /// and the next observation emits its one `Fail` action.
    pub(crate) fn record_schedule_rejected(
        &mut self,
        action_id: u64,
        scheduled_tick: u64,
    ) -> TacticalFailure {
        let detail = self
            .issued
            .take()
            .map(|issued| {
                format!(
                    "live command scheduler rejected action {} (issued {} at tick {})",
                    action_id, issued.action_id, issued.issued_tick
                )
            })
            .unwrap_or_else(|| {
                format!(
                    "live command scheduler rejected action {} with no issued action",
                    action_id
                )
            });
        self.set_failure(
            violation(TacticalFailureCode::CommandScheduleInvalid, detail),
            scheduled_tick,
        )
    }

    fn advance(
        &mut self,
        observation: &TacticalObservation,
    ) -> ScriptResult<Option<TacticalAction>> {
        self.validate_observation(observation)?;
        if self.budget_stage.is_none() {
            self.enter_budget(BudgetStage::DeployYard, observation);
        }
        self.check_budget(observation)?;
        if observation.match_ended {
            return Err(violation(
                TacticalFailureCode::MatchEnded,
                "match ended before tactical capture completed",
            ));
        }
        self.validate_existing_bindings(observation)?;

        if self.issued.is_some() {
            return Ok(None);
        }
        self.resolve_pending(observation)?;
        if self.pending.is_some() {
            return Ok(None);
        }

        match self.stage {
            TacticalScriptStage::AwaitRustL0 => self.start_at_rust_l0(observation),
            TacticalScriptStage::NeedSecondDeploy => {
                let mcv_id = self.mcv_id.ok_or_else(|| {
                    violation(
                        TacticalFailureCode::ObservationInvalid,
                        "second deploy requested without a bound MCV",
                    )
                })?;
                Ok(Some(self.issue_deploy(
                    observation.tick,
                    mcv_id,
                    DeployAttempt::Second,
                )))
            }
            TacticalScriptStage::AwaitYardConstruction => self.await_yard_construction(observation),
            TacticalScriptStage::AwaitPowerReady => {
                self.await_ready(observation, StructureRole::Power)
            }
            TacticalScriptStage::AwaitPowerConstruction => {
                self.await_construction(observation, StructureRole::Power)
            }
            TacticalScriptStage::AwaitRefineryReady => {
                self.await_ready(observation, StructureRole::Refinery)
            }
            TacticalScriptStage::AwaitRefineryConstruction => {
                self.await_construction(observation, StructureRole::Refinery)
            }
            TacticalScriptStage::AwaitRadarReady => {
                self.await_ready(observation, StructureRole::Radar)
            }
            TacticalScriptStage::AwaitRadarConstruction => {
                self.await_construction(observation, StructureRole::Radar)
            }
            TacticalScriptStage::AwaitRadarOnline => self.await_radar_online(observation),
            TacticalScriptStage::WarmStableFrames => self.await_stable_frames(observation),
            TacticalScriptStage::CaptureRequested => {
                if Some(observation.tick) != self.observed.capture_requested_tick {
                    return Err(violation(
                        TacticalFailureCode::ExpectedLedgerDrift,
                        format!(
                            "capture completion was observed at tick {}, expected the simulation to remain stopped at {}",
                            observation.tick,
                            self.observed.capture_requested_tick.unwrap_or(0)
                        ),
                    ));
                }
                if observation.capture_complete {
                    self.observed.capture_complete_tick = Some(observation.tick);
                    self.stage = TacticalScriptStage::Complete;
                    Ok(Some(TacticalAction::Complete))
                } else {
                    Ok(None)
                }
            }
            TacticalScriptStage::AwaitFirstDeployResult
            | TacticalScriptStage::AwaitSecondDeployResult
            | TacticalScriptStage::AwaitPowerQueueResult
            | TacticalScriptStage::AwaitPowerPlacementResult
            | TacticalScriptStage::AwaitRefineryQueueResult
            | TacticalScriptStage::AwaitRefineryPlacementResult
            | TacticalScriptStage::AwaitRadarQueueResult
            | TacticalScriptStage::AwaitRadarPlacementResult => Err(violation(
                TacticalFailureCode::CommandResultMissing,
                "command-result stage has no pending command",
            )),
            TacticalScriptStage::Complete | TacticalScriptStage::Failed => Ok(None),
        }
    }

    fn validate_observation(&mut self, observation: &TacticalObservation) -> ScriptResult<()> {
        if !observation.accepted_rust_l0 || !observation.in_game {
            return Err(violation(
                TacticalFailureCode::ObservationInvalid,
                "tactical script requires accepted Rust-L0 and InGame",
            ));
        }
        if observation.local_owner != self.config.owner {
            return Err(violation(
                TacticalFailureCode::ObservationInvalid,
                format!(
                    "local owner drifted from '{}' to '{}'",
                    self.config.owner, observation.local_owner
                ),
            ));
        }
        if let Some(last_tick) = self.last_tick {
            if observation.tick < last_tick {
                return Err(violation(
                    TacticalFailureCode::ClockDrift,
                    format!(
                        "simulation tick moved backwards from {last_tick} to {}",
                        observation.tick
                    ),
                ));
            }
            if observation.tick > last_tick.saturating_add(1) {
                return Err(violation(
                    TacticalFailureCode::ClockDrift,
                    format!(
                        "script observation skipped from tick {last_tick} to {}",
                        observation.tick
                    ),
                ));
            }
        }
        if let Some(last_wall_ms) = self.last_wall_ms {
            if observation.wall_elapsed_ms < last_wall_ms {
                return Err(violation(
                    TacticalFailureCode::ClockDrift,
                    format!(
                        "wall elapsed time moved backwards from {last_wall_ms} to {}",
                        observation.wall_elapsed_ms
                    ),
                ));
            }
        }
        let expected_total_sim_ms = observation.tick.saturating_mul(u64::from(SIM_TICK_MS));
        if observation.total_sim_ms != expected_total_sim_ms {
            return Err(violation(
                TacticalFailureCode::ClockDrift,
                format!(
                    "tick {} carried total_sim_ms {}, expected {}",
                    observation.tick, observation.total_sim_ms, expected_total_sim_ms
                ),
            ));
        }
        let expected_binary_frame = observation.tick as u32;
        if observation.binary_frame != expected_binary_frame {
            return Err(violation(
                TacticalFailureCode::ClockDrift,
                format!(
                    "tick {} carried binary frame {}, expected {}",
                    observation.tick, observation.binary_frame, expected_binary_frame
                ),
            ));
        }
        if observation.tick > self.config.overall_tick_cap {
            return Err(violation(
                TacticalFailureCode::BudgetExceeded,
                format!(
                    "overall tick {} exceeded cap {}",
                    observation.tick, self.config.overall_tick_cap
                ),
            ));
        }
        self.last_tick = Some(observation.tick);
        self.last_wall_ms = Some(observation.wall_elapsed_ms);
        Ok(())
    }

    fn start_at_rust_l0(
        &mut self,
        observation: &TacticalObservation,
    ) -> ScriptResult<Option<TacticalAction>> {
        if observation.tick != 0 || observation.total_sim_ms != 0 || observation.binary_frame != 0 {
            return Err(violation(
                TacticalFailureCode::ExpectedLedgerDrift,
                "first tactical observation was not exact tick/time/frame zero",
            ));
        }
        let mcvs = self.live_entities(observation, &self.config.deployment.mcv_type_id, None);
        if mcvs.len() != 1 {
            return Err(violation(
                TacticalFailureCode::ObservationInvalid,
                format!("expected exactly one local MCV, observed {}", mcvs.len()),
            ));
        }
        let yards = self.live_entities(observation, &self.config.deployment.yard_type_id, None);
        if !yards.is_empty() {
            return Err(violation(
                TacticalFailureCode::ObservationInvalid,
                "construction yard existed before the first deploy command",
            ));
        }
        let mcv_id = mcvs[0].stable_id;
        self.mcv_id = Some(mcv_id);
        self.observed.rust_l0_tick = Some(observation.tick);
        Ok(Some(self.issue_deploy(
            observation.tick,
            mcv_id,
            DeployAttempt::First,
        )))
    }

    fn await_yard_construction(
        &mut self,
        observation: &TacticalObservation,
    ) -> ScriptResult<Option<TacticalAction>> {
        let yard = self.bound_structure(observation, StructureRoleOrYard::Yard)?;
        let complete = !yard.building_up;
        if !self.require_exact_event_tick(
            observation.tick,
            self.config.expected.yard_active_tick,
            complete,
            "construction yard completion",
        )? {
            return Ok(None);
        }
        self.observed.yard_active_tick = Some(observation.tick);
        self.require_enabled_build_option(observation, &self.config.power.type_id)?;
        self.require_local_production_idle(observation)?;
        self.enter_budget(BudgetStage::PowerProduction, observation);
        Ok(Some(
            self.issue_queue(observation.tick, StructureRole::Power),
        ))
    }

    fn await_ready(
        &mut self,
        observation: &TacticalObservation,
        role: StructureRole,
    ) -> ScriptResult<Option<TacticalAction>> {
        let target = self.config.target(role).clone();
        let matching_queue: Vec<&ProductionQueueObservation> = observation
            .queued_production
            .iter()
            .filter(|queued| queued.type_id.eq_ignore_ascii_case(&target.type_id))
            .collect();
        let ready_count = count_type(&observation.ready_buildings, &target.type_id);
        let unrelated_queue_count = observation
            .queued_production
            .len()
            .saturating_sub(matching_queue.len());
        let unrelated_ready_count = observation
            .ready_buildings
            .len()
            .saturating_sub(ready_count);
        if unrelated_queue_count != 0 || unrelated_ready_count != 0 {
            return Err(violation(
                TacticalFailureCode::ObservationInvalid,
                format!(
                    "{role:?} production was contaminated by {unrelated_queue_count} unrelated queue entries and {unrelated_ready_count} unrelated ready entries"
                ),
            ));
        }
        if ready_count > 1 {
            return Err(violation(
                TacticalFailureCode::ObservationInvalid,
                format!(
                    "ready set contains {} copies of exact target '{}'",
                    ready_count, target.type_id
                ),
            ));
        }
        if observation.tick < target.expected_ready_tick {
            if ready_count != 0 {
                return Err(violation(
                    TacticalFailureCode::ExpectedLedgerDrift,
                    format!(
                        "{role:?} target '{}' became ready early at tick {}, expected {}",
                        target.type_id, observation.tick, target.expected_ready_tick
                    ),
                ));
            }
            if matching_queue.len() != 1 {
                return Err(violation(
                    TacticalFailureCode::CommandResultMissing,
                    format!(
                        "{role:?} target '{}' did not remain the sole active production before ready tick {}",
                        target.type_id, target.expected_ready_tick
                    ),
                ));
            }
            // Enqueue runs after the factory sweep. Its receipt exposes the
            // rate-0 constructor; the next sweep must establish the real cadence.
            let just_queued = self.command_ledger.last().is_some_and(|entry| {
                observation.tick == entry.scheduled_tick.saturating_add(1)
                    && matches!(&entry.payload,
                        ScriptCommandPayload::QueueExactType { role: queued_role, .. }
                        if *queued_role == role)
            });
            let expected_rate = if just_queued {
                0
            } else {
                target.expected_rate_frames
            };
            if matching_queue[0].resolved_rate_frames != expected_rate {
                return Err(violation(
                    TacticalFailureCode::ExpectedLedgerDrift,
                    format!(
                        "{role:?} target '{}' cadence drifted from {} to {}",
                        target.type_id, expected_rate, matching_queue[0].resolved_rate_frames
                    ),
                ));
            }
        } else if !matching_queue.is_empty() {
            return Err(violation(
                TacticalFailureCode::CommandResultMissing,
                format!(
                    "{role:?} target '{}' remained queued when readiness was due",
                    target.type_id
                ),
            ));
        }
        if !self.require_exact_event_tick(
            observation.tick,
            target.expected_ready_tick,
            ready_count == 1,
            &format!("{role:?} ready"),
        )? {
            return Ok(None);
        }
        self.record_ready_tick(role, observation.tick);

        let choice = observation.placement_choice.clone().ok_or_else(|| {
            violation(
                TacticalFailureCode::PlacementUnavailable,
                format!(
                    "no real placement candidate for ready target '{}'",
                    target.type_id
                ),
            )
        })?;
        let yard = self.bindings.yard.as_ref().ok_or_else(|| {
            violation(
                TacticalFailureCode::ObservationInvalid,
                "placement requested without a bound construction yard",
            )
        })?;
        if !choice.type_id.eq_ignore_ascii_case(&target.type_id)
            || choice.anchor_yard_id != yard.stable_id
            || choice.anchor_cell != yard.cell
            || choice.radius > self.config.placement_radius
        {
            return Err(violation(
                TacticalFailureCode::PlacementUnavailable,
                format!(
                    "placement choice {:?} does not match target '{}' and bound yard {} at {:?} within radius {}",
                    choice, target.type_id, yard.stable_id, yard.cell, self.config.placement_radius
                ),
            ));
        }
        if role == StructureRole::Radar {
            self.record_required_harvester(observation)?;
        }
        self.enter_budget(placement_budget_stage(role), observation);
        Ok(Some(self.issue_place(observation.tick, role, choice)))
    }

    fn await_construction(
        &mut self,
        observation: &TacticalObservation,
        role: StructureRole,
    ) -> ScriptResult<Option<TacticalAction>> {
        let target = self.config.target(role).clone();
        let structure = self.bound_structure(observation, StructureRoleOrYard::Role(role))?;
        let complete = !structure.building_up;
        if !self.require_exact_event_tick(
            observation.tick,
            target.expected_active_tick,
            complete,
            &format!("{role:?} construction completion"),
        )? {
            return Ok(None);
        }
        self.record_active_tick(role, observation.tick);

        match role {
            StructureRole::Power => {
                if !observation.power_authority_sufficient {
                    return Err(violation(
                        TacticalFailureCode::ObservationInvalid,
                        "power plant completed without sufficient live power authority",
                    ));
                }
                self.require_enabled_build_option(observation, &self.config.refinery.type_id)?;
                self.require_local_production_idle(observation)?;
                self.enter_budget(BudgetStage::RefineryProduction, observation);
                Ok(Some(
                    self.issue_queue(observation.tick, StructureRole::Refinery),
                ))
            }
            StructureRole::Refinery => {
                self.require_enabled_build_option(observation, &self.config.radar.type_id)?;
                self.require_local_production_idle(observation)?;
                self.enter_budget(BudgetStage::RadarProduction, observation);
                Ok(Some(
                    self.issue_queue(observation.tick, StructureRole::Radar),
                ))
            }
            StructureRole::Radar => {
                if !observation.power_authority_sufficient || !observation.radar_authority_active {
                    return Err(violation(
                        TacticalFailureCode::ObservationInvalid,
                        "radar completed without live powered-radar authority",
                    ));
                }
                self.stage = TacticalScriptStage::AwaitRadarOnline;
                self.enter_budget(BudgetStage::RadarOpening, observation);
                Ok(None)
            }
        }
    }

    fn await_radar_online(
        &mut self,
        observation: &TacticalObservation,
    ) -> ScriptResult<Option<TacticalAction>> {
        self.bound_structure(observation, StructureRoleOrYard::Role(StructureRole::Radar))?;
        if !observation.power_authority_sufficient || !observation.radar_authority_active {
            return Err(violation(
                TacticalFailureCode::ObservationInvalid,
                "powered radar authority was lost while awaiting Online",
            ));
        }
        // RadarAnimation consumes real wall-clock buckets at draw. Construction
        // ticks stay exact, but readiness is condition-led within both budgets.
        if !observation.radar_online {
            return Ok(None);
        }
        if !observation.readiness_complete {
            return Err(violation(
                TacticalFailureCode::ObservationInvalid,
                "first complete post-render readiness tuple was absent at radar Online",
            ));
        }
        self.observed.radar_online_tick = Some(observation.tick);
        self.stage = TacticalScriptStage::WarmStableFrames;
        self.enter_budget(BudgetStage::StableFrames, observation);
        Ok(None)
    }

    fn await_stable_frames(
        &mut self,
        observation: &TacticalObservation,
    ) -> ScriptResult<Option<TacticalAction>> {
        if !observation.readiness_complete
            || !observation.radar_online
            || !observation.power_authority_sufficient
            || !observation.radar_authority_active
        {
            return Err(violation(
                TacticalFailureCode::ObservationInvalid,
                "complete readiness tuple did not remain true during stable frames",
            ));
        }
        let online_tick = self
            .observed
            .radar_online_tick
            .expect("Online entered this stage");
        let second_readiness_tick = online_tick + 1;
        let capture_tick = second_readiness_tick + u64::from(self.config.warm_frames);
        if observation.tick < second_readiness_tick {
            return Ok(None);
        }
        if observation.tick == second_readiness_tick {
            self.observed.second_readiness_tick = Some(observation.tick);
            return Ok(None);
        }
        if observation.tick < capture_tick {
            return Ok(None);
        }
        if observation.tick > capture_tick {
            return Err(violation(
                TacticalFailureCode::ExpectedLedgerDrift,
                format!(
                    "capture became actionable at tick {}, expected {}",
                    observation.tick, capture_tick
                ),
            ));
        }
        self.observed.capture_requested_tick = Some(observation.tick);
        self.stage = TacticalScriptStage::CaptureRequested;
        Ok(Some(TacticalAction::Capture))
    }

    fn resolve_pending(&mut self, observation: &TacticalObservation) -> ScriptResult<()> {
        let Some(mut pending) = self.pending.take() else {
            return Ok(());
        };
        // Offline scheduling returns the raw issue ordinal. Its command drains
        // at the next frame tail; that frame's committed result is observed N+1.
        let result_tick = pending.scheduled_tick.saturating_add(1);
        if observation.tick < result_tick {
            self.pending = Some(pending);
            return Ok(());
        }
        if observation.tick > result_tick {
            return Err(violation(
                TacticalFailureCode::CommandResultMissing,
                format!(
                    "action {} was first observed after result tick {} at tick {}",
                    pending.action_id, result_tick, observation.tick
                ),
            ));
        }

        let (resolved, next_stage) = match &pending.expected_result {
            ExpectedCommandResult::McvTurnOrYard {
                mcv_id,
                yard_type_id,
                deploy_facing,
            } => self.resolve_first_deploy(observation, *mcv_id, yard_type_id, *deploy_facing)?,
            ExpectedCommandResult::YardCreated {
                mcv_id,
                yard_type_id,
            } => self.resolve_second_deploy(observation, *mcv_id, yard_type_id)?,
            ExpectedCommandResult::QueueOrReady { type_id, .. } => {
                let role = command_role(&pending.payload).ok_or_else(|| {
                    violation(
                        TacticalFailureCode::ObservationInvalid,
                        "queue result carried a non-production payload",
                    )
                })?;
                (self.resolve_queue(observation, type_id)?, ready_stage(role))
            }
            ExpectedCommandResult::BuildingPlacedReadyConsumed { type_id, cell } => {
                let (role, choice) = match &pending.payload {
                    ScriptCommandPayload::PlaceExactType { role, choice } => {
                        (*role, choice.clone())
                    }
                    _ => {
                        return Err(violation(
                            TacticalFailureCode::ObservationInvalid,
                            "placement result carried a non-placement payload",
                        ));
                    }
                };
                (
                    self.resolve_placement(observation, role, type_id, *cell, choice)?,
                    construction_stage(role),
                )
            }
        };
        pending.resolved_result = Some(resolved);
        self.command_ledger.push(pending);
        self.stage = next_stage;
        Ok(())
    }

    fn require_local_production_idle(&self, observation: &TacticalObservation) -> ScriptResult<()> {
        if !observation.queued_production.is_empty() || !observation.ready_buildings.is_empty() {
            return Err(violation(
                TacticalFailureCode::ObservationInvalid,
                format!(
                    "next fixed target requires an idle local production ledger, observed {} queued and {} ready entries",
                    observation.queued_production.len(),
                    observation.ready_buildings.len()
                ),
            ));
        }
        Ok(())
    }

    fn resolve_first_deploy(
        &mut self,
        observation: &TacticalObservation,
        mcv_id: u64,
        yard_type_id: &str,
        deploy_facing: u8,
    ) -> ScriptResult<(ResolvedCommandResult, TacticalScriptStage)> {
        let yards = self.live_entities(observation, yard_type_id, None);
        if yards.len() > 1 {
            return Err(violation(
                TacticalFailureCode::ObservationInvalid,
                format!("first deploy produced {} matching yards", yards.len()),
            ));
        }
        let same_id_live = observation
            .entities
            .iter()
            .find(|entity| entity.stable_id == mcv_id && entity.live());
        let mcv = same_id_live.filter(|entity| {
            entity.owner == self.config.owner
                && entity
                    .type_id
                    .eq_ignore_ascii_case(&self.config.deployment.mcv_type_id)
        });
        if let Some(yard) = yards.first() {
            if same_id_live.is_some() {
                return Err(violation(
                    TacticalFailureCode::CommandResultMissing,
                    "first deploy left the original stable ID active alongside the construction yard",
                ));
            }
            let observed = ObservedStructure {
                stable_id: yard.stable_id,
                type_id: yard.type_id.clone(),
                cell: yard.cell,
            };
            self.bindings.yard = Some(observed);
            return Ok((
                ResolvedCommandResult::YardObserved {
                    stable_id: yard.stable_id,
                    cell: yard.cell,
                },
                TacticalScriptStage::AwaitYardConstruction,
            ));
        }

        let Some(mcv) = mcv else {
            return Err(violation(
                TacticalFailureCode::CommandResultMissing,
                "first deploy produced neither the expected yard nor the same live MCV",
            ));
        };
        if mcv.facing != deploy_facing {
            return Err(violation(
                TacticalFailureCode::CommandResultMissing,
                format!(
                    "first deploy left MCV {} at facing {}, expected {}",
                    mcv_id, mcv.facing, deploy_facing
                ),
            ));
        }
        Ok((
            ResolvedCommandResult::McvTurned {
                mcv_id,
                facing: mcv.facing,
            },
            TacticalScriptStage::NeedSecondDeploy,
        ))
    }

    fn resolve_second_deploy(
        &mut self,
        observation: &TacticalObservation,
        mcv_id: u64,
        yard_type_id: &str,
    ) -> ScriptResult<(ResolvedCommandResult, TacticalScriptStage)> {
        let mcv_live = observation
            .entities
            .iter()
            .any(|entity| entity.stable_id == mcv_id && entity.live());
        let yards = self.live_entities(observation, yard_type_id, None);
        if mcv_live || yards.len() != 1 {
            return Err(violation(
                TacticalFailureCode::CommandResultMissing,
                format!(
                    "second deploy requires MCV gone and one active yard; mcv_live={mcv_live} yards={}",
                    yards.len()
                ),
            ));
        }
        let yard = yards[0];
        self.bindings.yard = Some(ObservedStructure {
            stable_id: yard.stable_id,
            type_id: yard.type_id.clone(),
            cell: yard.cell,
        });
        Ok((
            ResolvedCommandResult::YardObserved {
                stable_id: yard.stable_id,
                cell: yard.cell,
            },
            TacticalScriptStage::AwaitYardConstruction,
        ))
    }

    fn resolve_queue(
        &self,
        observation: &TacticalObservation,
        type_id: &str,
    ) -> ScriptResult<ResolvedCommandResult> {
        let matching_queue: Vec<&ProductionQueueObservation> = observation
            .queued_production
            .iter()
            .filter(|queued| queued.type_id.eq_ignore_ascii_case(type_id))
            .collect();
        let ready_count = count_type(&observation.ready_buildings, type_id);
        if matching_queue.len() + ready_count != 1 {
            return Err(violation(
                TacticalFailureCode::CommandResultMissing,
                format!(
                    "queue command for '{}' observed {} live queue entries and {} ready entries",
                    type_id,
                    matching_queue.len(),
                    ready_count
                ),
            ));
        }
        if let Some(queued) = matching_queue.first() {
            if queued.resolved_rate_frames != 0 {
                return Err(violation(
                    TacticalFailureCode::ExpectedLedgerDrift,
                    format!(
                        "queue '{}' resolved rate {}, expected {}",
                        type_id, queued.resolved_rate_frames, 0
                    ),
                ));
            }
            Ok(ResolvedCommandResult::QueueObserved {
                type_id: type_id.to_string(),
                resolved_rate_frames: queued.resolved_rate_frames,
            })
        } else {
            Ok(ResolvedCommandResult::ReadyObserved {
                type_id: type_id.to_string(),
            })
        }
    }

    fn resolve_placement(
        &mut self,
        observation: &TacticalObservation,
        role: StructureRole,
        type_id: &str,
        cell: (u16, u16),
        choice: PlacementChoice,
    ) -> ScriptResult<ResolvedCommandResult> {
        let ready_count = count_type(&observation.ready_buildings, type_id);
        if ready_count != 0 {
            return Err(violation(
                TacticalFailureCode::CommandResultMissing,
                format!(
                    "placement command for '{}' left {} matching ready entries",
                    type_id, ready_count
                ),
            ));
        }
        let structures = self.live_entities(observation, type_id, Some(cell));
        if structures.len() != 1 {
            return Err(violation(
                TacticalFailureCode::CommandResultMissing,
                format!(
                    "placement command for '{}' at {:?} observed {} matching active structures",
                    type_id,
                    cell,
                    structures.len()
                ),
            ));
        }
        let structure = structures[0];
        let binding = ObservedStructure {
            stable_id: structure.stable_id,
            type_id: structure.type_id.clone(),
            cell: structure.cell,
        };
        match role {
            StructureRole::Power => self.bindings.power = Some(binding),
            StructureRole::Refinery => self.bindings.refinery = Some(binding),
            StructureRole::Radar => self.bindings.radar = Some(binding),
        }
        // The selected cell is already immutable in the pending payload. Copy
        // it into the placement ledger only after the downstream placement
        // condition succeeds.
        self.placements.push(choice);
        Ok(ResolvedCommandResult::BuildingObserved {
            stable_id: structure.stable_id,
            type_id: structure.type_id.clone(),
            cell: structure.cell,
        })
    }

    fn issue_deploy(
        &mut self,
        tick: u64,
        entity_id: u64,
        attempt: DeployAttempt,
    ) -> TacticalAction {
        let payload = ScriptCommandPayload::DeployMcv { entity_id, attempt };
        let expected_result = match attempt {
            DeployAttempt::First => ExpectedCommandResult::McvTurnOrYard {
                mcv_id: entity_id,
                yard_type_id: self.config.deployment.yard_type_id.clone(),
                deploy_facing: self.config.deployment.deploy_facing,
            },
            DeployAttempt::Second => ExpectedCommandResult::YardCreated {
                mcv_id: entity_id,
                yard_type_id: self.config.deployment.yard_type_id.clone(),
            },
        };
        let action_id = self.issue(tick, payload, expected_result);
        TacticalAction::DeployMcv {
            action_id,
            owner: self.config.owner.clone(),
            entity_id,
            attempt,
        }
    }

    fn issue_queue(&mut self, tick: u64, role: StructureRole) -> TacticalAction {
        let target = self.config.target(role).clone();
        let payload = ScriptCommandPayload::QueueExactType {
            role,
            type_id: target.type_id.clone(),
        };
        let expected_result = ExpectedCommandResult::QueueOrReady {
            type_id: target.type_id.clone(),
            expected_rate_frames: target.expected_rate_frames,
        };
        let action_id = self.issue(tick, payload, expected_result);
        TacticalAction::QueueExactType {
            action_id,
            owner: self.config.owner.clone(),
            role,
            type_id: target.type_id,
        }
    }

    fn issue_place(
        &mut self,
        tick: u64,
        role: StructureRole,
        choice: PlacementChoice,
    ) -> TacticalAction {
        let expected_result = ExpectedCommandResult::BuildingPlacedReadyConsumed {
            type_id: choice.type_id.clone(),
            cell: choice.cell,
        };
        let payload = ScriptCommandPayload::PlaceExactType {
            role,
            choice: choice.clone(),
        };
        let action_id = self.issue(tick, payload, expected_result);
        TacticalAction::PlaceExactType {
            action_id,
            owner: self.config.owner.clone(),
            role,
            choice,
        }
    }

    fn issue(
        &mut self,
        tick: u64,
        payload: ScriptCommandPayload,
        expected_result: ExpectedCommandResult,
    ) -> u64 {
        debug_assert!(self.issued.is_none());
        debug_assert!(self.pending.is_none());
        let action_id = self.next_action_id;
        self.next_action_id = self.next_action_id.saturating_add(1);
        self.issued = Some(IssuedCommand {
            action_id,
            issued_tick: tick,
            owner: self.config.owner.clone(),
            payload,
            expected_result,
        });
        action_id
    }

    fn live_entities<'a>(
        &self,
        observation: &'a TacticalObservation,
        type_id: &str,
        cell: Option<(u16, u16)>,
    ) -> Vec<&'a TacticalEntityObservation> {
        observation
            .entities
            .iter()
            .filter(|entity| {
                entity.live()
                    && entity.owner == self.config.owner
                    && entity.type_id.eq_ignore_ascii_case(type_id)
                    && cell.is_none_or(|expected| entity.cell == expected)
            })
            .collect()
    }

    fn bound_structure<'a>(
        &self,
        observation: &'a TacticalObservation,
        role: StructureRoleOrYard,
    ) -> ScriptResult<&'a TacticalEntityObservation> {
        let binding = match role {
            StructureRoleOrYard::Yard => self.bindings.yard.as_ref(),
            StructureRoleOrYard::Role(StructureRole::Power) => self.bindings.power.as_ref(),
            StructureRoleOrYard::Role(StructureRole::Refinery) => self.bindings.refinery.as_ref(),
            StructureRoleOrYard::Role(StructureRole::Radar) => self.bindings.radar.as_ref(),
        }
        .ok_or_else(|| {
            violation(
                TacticalFailureCode::ObservationInvalid,
                format!("missing bound {role:?} structure"),
            )
        })?;
        let matches: Vec<&TacticalEntityObservation> = observation
            .entities
            .iter()
            .filter(|entity| {
                entity.stable_id == binding.stable_id
                    && entity.owner == self.config.owner
                    && entity.type_id.eq_ignore_ascii_case(&binding.type_id)
                    && entity.cell == binding.cell
                    && entity.live()
            })
            .collect();
        if matches.len() != 1 {
            return Err(violation(
                TacticalFailureCode::ObservationInvalid,
                format!(
                    "bound {role:?} structure {} '{}' at {:?} was not uniquely active",
                    binding.stable_id, binding.type_id, binding.cell
                ),
            ));
        }
        Ok(matches[0])
    }

    fn validate_existing_bindings(&self, observation: &TacticalObservation) -> ScriptResult<()> {
        if self.bindings.yard.is_some() {
            self.bound_structure(observation, StructureRoleOrYard::Yard)?;
        }
        for (present, role) in [
            (self.bindings.power.is_some(), StructureRole::Power),
            (self.bindings.refinery.is_some(), StructureRole::Refinery),
            (self.bindings.radar.is_some(), StructureRole::Radar),
        ] {
            if present {
                self.bound_structure(observation, StructureRoleOrYard::Role(role))?;
            }
        }
        Ok(())
    }

    fn record_ready_tick(&mut self, role: StructureRole, tick: u64) {
        match role {
            StructureRole::Power => self.observed.power_ready_tick = Some(tick),
            StructureRole::Refinery => self.observed.refinery_ready_tick = Some(tick),
            StructureRole::Radar => self.observed.radar_ready_tick = Some(tick),
        }
    }

    fn record_active_tick(&mut self, role: StructureRole, tick: u64) {
        match role {
            StructureRole::Power => self.observed.power_active_tick = Some(tick),
            StructureRole::Refinery => self.observed.refinery_active_tick = Some(tick),
            StructureRole::Radar => self.observed.radar_active_tick = Some(tick),
        }
    }

    fn require_enabled_build_option(
        &self,
        observation: &TacticalObservation,
        type_id: &str,
    ) -> ScriptResult<()> {
        if !observation.build_options_strict {
            return Err(violation(
                TacticalFailureCode::ObservationInvalid,
                "build-option observation did not come from the strict live view",
            ));
        }
        let matching: Vec<&BuildOptionObservation> = observation
            .build_options
            .iter()
            .filter(|option| option.type_id.eq_ignore_ascii_case(type_id))
            .collect();
        if matching.len() != 1 || !matching[0].enabled {
            return Err(violation(
                TacticalFailureCode::ObservationInvalid,
                format!(
                    "exact next target '{}' was not uniquely present and enabled in the strict build-option view",
                    type_id
                ),
            ));
        }
        Ok(())
    }

    fn record_required_harvester(&mut self, observation: &TacticalObservation) -> ScriptResult<()> {
        let Some(type_id) = self.config.refinery_harvester_type_id.as_deref() else {
            return Ok(());
        };
        let harvesters = self.live_entities(observation, type_id, None);
        if harvesters.len() != 1 {
            return Err(violation(
                TacticalFailureCode::ObservationInvalid,
                format!(
                    "expected exactly one live refinery-spawned '{}' at radar placement, observed {}",
                    type_id,
                    harvesters.len()
                ),
            ));
        }
        let harvester = harvesters[0];
        self.harvester = Some(HarvesterObservation {
            stable_id: harvester.stable_id,
            type_id: harvester.type_id.clone(),
            cell: harvester.cell,
        });
        Ok(())
    }

    fn require_exact_event_tick(
        &self,
        observed_tick: u64,
        expected_tick: u64,
        event_present: bool,
        label: &str,
    ) -> ScriptResult<bool> {
        if observed_tick < expected_tick {
            if event_present {
                return Err(violation(
                    TacticalFailureCode::ExpectedLedgerDrift,
                    format!(
                        "{label} appeared early at tick {observed_tick}, expected {expected_tick}"
                    ),
                ));
            }
            return Ok(false);
        }
        if observed_tick == expected_tick {
            if event_present {
                return Ok(true);
            }
            return Err(violation(
                TacticalFailureCode::ExpectedLedgerDrift,
                format!("{label} absent at expected tick {expected_tick}"),
            ));
        }
        Err(violation(
            TacticalFailureCode::ExpectedLedgerDrift,
            format!(
                "{label} was first accepted late at tick {observed_tick}, expected {expected_tick}"
            ),
        ))
    }

    fn enter_budget(&mut self, stage: BudgetStage, observation: &TacticalObservation) {
        self.budget_stage = Some(stage);
        self.budget_start_tick = observation.tick;
        self.budget_start_wall_ms = observation.wall_elapsed_ms;
    }

    fn check_budget(&self, observation: &TacticalObservation) -> ScriptResult<()> {
        let Some(stage) = self.budget_stage else {
            return Ok(());
        };
        let budget = self.budget_for(stage);
        let elapsed_ticks = observation.tick.saturating_sub(self.budget_start_tick);
        let elapsed_wall_ms = observation
            .wall_elapsed_ms
            .saturating_sub(self.budget_start_wall_ms);
        if elapsed_ticks > budget.max_ticks || elapsed_wall_ms > budget.max_wall_ms {
            return Err(violation(
                TacticalFailureCode::BudgetExceeded,
                format!(
                    "{stage:?} used {elapsed_ticks} ticks / {elapsed_wall_ms} ms, caps are {} ticks / {} ms",
                    budget.max_ticks, budget.max_wall_ms
                ),
            ));
        }
        Ok(())
    }

    fn budget_for(&self, stage: BudgetStage) -> StageBudget {
        match stage {
            BudgetStage::DeployYard => self.config.budgets.deploy_yard,
            BudgetStage::PowerProduction => self.config.budgets.power_production,
            BudgetStage::PowerPlacement => self.config.budgets.power_placement,
            BudgetStage::RefineryProduction => self.config.budgets.refinery_production,
            BudgetStage::RefineryPlacement => self.config.budgets.refinery_placement,
            BudgetStage::RadarProduction => self.config.budgets.radar_production,
            BudgetStage::RadarPlacement => self.config.budgets.radar_placement,
            BudgetStage::RadarOpening => self.config.budgets.radar_opening,
            BudgetStage::StableFrames => self.config.budgets.stable_frames,
        }
    }

    fn set_failure(&mut self, violation: ScriptViolation, tick: u64) -> TacticalFailure {
        if let Some(failure) = &self.failure {
            return failure.clone();
        }
        let failure = TacticalFailure {
            code: violation.code,
            stage: self.stage,
            tick,
            message: violation.message,
        };
        self.stage = TacticalScriptStage::Failed;
        self.pending = None;
        self.issued = None;
        self.failure = Some(failure.clone());
        failure
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StructureRoleOrYard {
    Yard,
    Role(StructureRole),
}

fn violation(code: TacticalFailureCode, message: impl Into<String>) -> ScriptViolation {
    ScriptViolation {
        code,
        message: message.into(),
    }
}

fn count_type(types: &[String], expected: &str) -> usize {
    types
        .iter()
        .filter(|type_id| type_id.eq_ignore_ascii_case(expected))
        .count()
}

fn command_role(payload: &ScriptCommandPayload) -> Option<StructureRole> {
    match payload {
        ScriptCommandPayload::QueueExactType { role, .. }
        | ScriptCommandPayload::PlaceExactType { role, .. } => Some(*role),
        ScriptCommandPayload::DeployMcv { .. } => None,
    }
}

fn queue_result_stage(role: StructureRole) -> TacticalScriptStage {
    match role {
        StructureRole::Power => TacticalScriptStage::AwaitPowerQueueResult,
        StructureRole::Refinery => TacticalScriptStage::AwaitRefineryQueueResult,
        StructureRole::Radar => TacticalScriptStage::AwaitRadarQueueResult,
    }
}

fn placement_result_stage(role: StructureRole) -> TacticalScriptStage {
    match role {
        StructureRole::Power => TacticalScriptStage::AwaitPowerPlacementResult,
        StructureRole::Refinery => TacticalScriptStage::AwaitRefineryPlacementResult,
        StructureRole::Radar => TacticalScriptStage::AwaitRadarPlacementResult,
    }
}

fn ready_stage(role: StructureRole) -> TacticalScriptStage {
    match role {
        StructureRole::Power => TacticalScriptStage::AwaitPowerReady,
        StructureRole::Refinery => TacticalScriptStage::AwaitRefineryReady,
        StructureRole::Radar => TacticalScriptStage::AwaitRadarReady,
    }
}

fn construction_stage(role: StructureRole) -> TacticalScriptStage {
    match role {
        StructureRole::Power => TacticalScriptStage::AwaitPowerConstruction,
        StructureRole::Refinery => TacticalScriptStage::AwaitRefineryConstruction,
        StructureRole::Radar => TacticalScriptStage::AwaitRadarConstruction,
    }
}

fn placement_budget_stage(role: StructureRole) -> BudgetStage {
    match role {
        StructureRole::Power => BudgetStage::PowerPlacement,
        StructureRole::Refinery => BudgetStage::RefineryPlacement,
        StructureRole::Radar => BudgetStage::RadarPlacement,
    }
}

#[cfg(test)]
mod script_tests {
    use super::*;

    const OWNER: &str = "Russians";
    const YARD_ID: u64 = 10;
    const YARD_CELL: (u16, u16) = (20, 20);

    fn budget(max_ticks: u64, seconds: u64) -> StageBudget {
        StageBudget {
            max_ticks,
            max_wall_ms: seconds * 1000,
        }
    }

    fn stock_config() -> TacticalScriptConfig {
        TacticalScriptConfig {
            owner: OWNER.to_string(),
            deployment: DeploymentContract {
                mcv_type_id: "SMCV".to_string(),
                yard_type_id: "NACNST".to_string(),
                deploy_facing: 0x80,
            },
            power: ProductionTargetContract {
                role: StructureRole::Power,
                type_id: "NAPOWR".to_string(),
                expected_rate_frames: 11,
                expected_ready_tick: 617,
                expected_active_tick: 648,
                construction_ticks: 31,
            },
            refinery: ProductionTargetContract {
                role: StructureRole::Refinery,
                type_id: "NAREFN".to_string(),
                expected_rate_frames: 37,
                expected_ready_tick: 2611,
                expected_active_tick: 2642,
                construction_ticks: 31,
            },
            radar: ProductionTargetContract {
                role: StructureRole::Radar,
                type_id: "NARADR".to_string(),
                expected_rate_frames: 18,
                expected_ready_tick: 3598,
                expected_active_tick: 3629,
                construction_ticks: 31,
            },
            refinery_harvester_type_id: Some("HARV".to_string()),
            placement_radius: 16,
            warm_frames: 16,
            overall_tick_cap: 8192,
            budgets: TacticalStageBudgets {
                deploy_yard: budget(48, 15),
                power_production: budget(640, 90),
                power_placement: budget(48, 15),
                refinery_production: budget(2048, 270),
                refinery_placement: budget(48, 15),
                radar_production: budget(1024, 140),
                radar_placement: budget(48, 15),
                radar_opening: budget(4096, 20),
                stable_frames: budget(18, 10),
            },
            expected: TacticalExpectedLedger {
                yard_active_tick: 32,
            },
        }
    }

    fn entity(
        stable_id: u64,
        type_id: &str,
        cell: (u16, u16),
        facing: u8,
        building_up: bool,
    ) -> TacticalEntityObservation {
        TacticalEntityObservation {
            stable_id,
            owner: OWNER.to_string(),
            type_id: type_id.to_string(),
            cell,
            facing,
            active: true,
            dying: false,
            building_up,
        }
    }

    fn placement(type_id: &str, cell: (u16, u16), index: u32) -> PlacementChoice {
        PlacementChoice {
            type_id: type_id.to_string(),
            anchor_yard_id: YARD_ID,
            anchor_cell: YARD_CELL,
            cell,
            foundation: (2, 2),
            radius: 4,
            candidate_index: index,
        }
    }

    fn observation(tick: u64) -> TacticalObservation {
        let total_sim_ms = tick * u64::from(SIM_TICK_MS);
        let mut entities = Vec::new();
        if tick < 2 {
            entities.push(entity(
                1,
                "SMCV",
                YARD_CELL,
                if tick < 1 { 0x40 } else { 0x80 },
                false,
            ));
        }
        if tick >= 2 {
            entities.push(entity(YARD_ID, "NACNST", YARD_CELL, 0, tick < 32));
        }
        if tick >= 618 {
            entities.push(entity(20, "NAPOWR", (24, 20), 0, tick < 648));
        }
        if tick >= 2612 {
            entities.push(entity(30, "NAREFN", (24, 23), 0, tick < 2642));
            entities.push(entity(31, "HARV", (28, 23), 64, false));
        }
        if tick >= 3599 {
            entities.push(entity(40, "NARADR", (24, 26), 0, tick < 3629));
        }

        let mut build_options = Vec::new();
        if tick >= 32 {
            build_options.push(BuildOptionObservation {
                type_id: "NAPOWR".to_string(),
                enabled: true,
            });
        }
        if tick >= 648 {
            build_options.push(BuildOptionObservation {
                type_id: "NAREFN".to_string(),
                enabled: true,
            });
        }
        if tick >= 2642 {
            build_options.push(BuildOptionObservation {
                type_id: "NARADR".to_string(),
                enabled: true,
            });
        }

        let mut queued_production = Vec::new();
        if (33..617).contains(&tick) {
            queued_production.push(ProductionQueueObservation {
                type_id: "NAPOWR".to_string(),
                resolved_rate_frames: if tick == 33 { 0 } else { 11 },
            });
        }
        if (649..2611).contains(&tick) {
            queued_production.push(ProductionQueueObservation {
                type_id: "NAREFN".to_string(),
                resolved_rate_frames: if tick == 649 { 0 } else { 37 },
            });
        }
        if (2643..3598).contains(&tick) {
            queued_production.push(ProductionQueueObservation {
                type_id: "NARADR".to_string(),
                resolved_rate_frames: if tick == 2643 { 0 } else { 18 },
            });
        }

        let mut ready_buildings = Vec::new();
        let placement_choice = if (617..618).contains(&tick) {
            ready_buildings.push("NAPOWR".to_string());
            Some(placement("NAPOWR", (24, 20), 7))
        } else if (2611..2612).contains(&tick) {
            ready_buildings.push("NAREFN".to_string());
            Some(placement("NAREFN", (24, 23), 12))
        } else if (3598..3599).contains(&tick) {
            ready_buildings.push("NARADR".to_string());
            Some(placement("NARADR", (24, 26), 18))
        } else {
            None
        };

        TacticalObservation {
            tick,
            total_sim_ms,
            binary_frame: tick as u32,
            wall_elapsed_ms: tick,
            accepted_rust_l0: true,
            in_game: true,
            local_owner: OWNER.to_string(),
            match_ended: false,
            build_options_strict: true,
            entities,
            build_options,
            queued_production,
            ready_buildings,
            placement_choice,
            power_authority_sufficient: tick >= 648,
            radar_authority_active: tick >= 3629,
            radar_online: tick >= 3694,
            readiness_complete: tick >= 3694,
            capture_complete: false,
        }
    }

    fn record_if_command(script: &mut TacticalScript, tick: u64, action: &TacticalAction) {
        let action_id = match action {
            TacticalAction::DeployMcv { action_id, .. }
            | TacticalAction::QueueExactType { action_id, .. }
            | TacticalAction::PlaceExactType { action_id, .. } => *action_id,
            TacticalAction::Capture | TacticalAction::Complete | TacticalAction::Fail { .. } => {
                return;
            }
        };
        script
            .record_scheduled(action_id, tick, tick)
            .expect("record schedule");
    }

    fn drive_until(
        script: &mut TacticalScript,
        inclusive_tick: u64,
        mut alter: impl FnMut(u64, &mut TacticalObservation),
    ) -> Vec<(u64, TacticalAction)> {
        let mut actions = Vec::new();
        for tick in 0..=inclusive_tick {
            let mut obs = observation(tick);
            alter(tick, &mut obs);
            if let Some(action) = script.next_action(&obs) {
                record_if_command(script, tick, &action);
                actions.push((tick, action));
            }
        }
        actions
    }

    #[test]
    fn current_profile_ledger_drives_complete_loop_exactly() {
        let mut script = TacticalScript::new(stock_config()).expect("valid config");
        let actions = drive_until(&mut script, 3711, |_, _| {});

        let action_ticks: Vec<u64> = actions.iter().map(|(tick, _)| *tick).collect();
        assert_eq!(
            action_ticks,
            vec![0, 1, 32, 617, 648, 2611, 2642, 3598, 3711]
        );
        assert!(matches!(
            actions.last(),
            Some((3711, TacticalAction::Capture))
        ));
        assert_eq!(script.command_ledger().len(), 8);
        assert_eq!(script.placement_ledger().len(), 3);
        let placed_cells: Vec<(u16, u16)> = script
            .command_ledger()
            .iter()
            .filter_map(|entry| match (&entry.payload, &entry.resolved_result) {
                (
                    ScriptCommandPayload::PlaceExactType { choice, .. },
                    Some(ResolvedCommandResult::BuildingObserved { cell, .. }),
                ) => {
                    assert_eq!(choice.cell, *cell);
                    Some(*cell)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            placed_cells,
            script
                .placement_ledger()
                .iter()
                .map(|choice| choice.cell)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            script
                .harvester_observation()
                .map(|harvester| (harvester.stable_id, harvester.cell)),
            Some((31, (28, 23)))
        );
        assert_eq!(
            script.observed_ledger(),
            &TacticalObservedLedger {
                rust_l0_tick: Some(0),
                yard_active_tick: Some(32),
                power_ready_tick: Some(617),
                power_active_tick: Some(648),
                refinery_ready_tick: Some(2611),
                refinery_active_tick: Some(2642),
                radar_ready_tick: Some(3598),
                radar_active_tick: Some(3629),
                radar_online_tick: Some(3694),
                second_readiness_tick: Some(3695),
                capture_requested_tick: Some(3711),
                capture_complete_tick: None,
            }
        );

        let mut completed = observation(3711);
        completed.capture_complete = true;
        assert_eq!(
            script.next_action(&completed),
            Some(TacticalAction::Complete)
        );
        assert_eq!(script.stage(), TacticalScriptStage::Complete);
        assert_eq!(script.observed_ledger().capture_complete_tick, Some(3711));
    }

    #[test]
    fn wall_clock_online_observation_controls_warm_frames_without_fixed_sim_tick() {
        // Exercise the production script receiver with different render cadences:
        // its Online observation is independent of deterministic construction.
        for online in [3630, 3694, 3900, 7700] {
            let mut script = TacticalScript::new(stock_config()).unwrap();
            let capture = online + 17;
            let actions = drive_until(&mut script, capture, |tick, observation| {
                observation.radar_online = tick >= online;
                observation.readiness_complete = tick >= online;
            });
            assert!(
                matches!(actions.last(), Some((tick, TacticalAction::Capture)) if *tick == capture)
            );
            assert!(
                !actions
                    .iter()
                    .any(|(_, action)| matches!(action, TacticalAction::Fail { .. }))
            );
            assert_eq!(script.command_ledger().len(), 8);
            assert_eq!(script.observed_ledger().radar_online_tick, Some(online));
            assert_eq!(
                script.observed_ledger().second_readiness_tick,
                Some(online + 1)
            );
            assert_eq!(
                script.observed_ledger().capture_requested_tick,
                Some(capture)
            );
        }
    }

    #[test]
    fn radar_online_observation_is_bounded_by_tick_and_wall_budgets() {
        let mut script = TacticalScript::new(stock_config()).unwrap();
        let actions = drive_until(&mut script, 7726, |_, observation| {
            observation.radar_online = false;
            observation.readiness_complete = false;
        });
        assert!(actions.iter().any(|(_, action)| matches!(
            action,
            TacticalAction::Fail {
                failure: TacticalFailure {
                    code: TacticalFailureCode::BudgetExceeded,
                    ..
                }
            }
        )));
        let mut script = TacticalScript::new(stock_config()).unwrap();
        drive_until(&mut script, 3629, |_, _| {});
        let mut late = observation(3630);
        late.wall_elapsed_ms = 3629 + 20_001;
        assert!(matches!(
            script.next_action(&late),
            Some(TacticalAction::Fail {
                failure: TacticalFailure {
                    code: TacticalFailureCode::BudgetExceeded,
                    ..
                }
            })
        ));
    }

    #[test]
    fn raw_issue_stamp_precedes_the_observable_frame_result() {
        let mut script = TacticalScript::new(stock_config()).expect("valid config");
        let first = script.next_action(&observation(0)).expect("deploy action");
        record_if_command(&mut script, 0, &first);
        assert_eq!(script.pending_command().unwrap().execute_tick, 0);
        assert!(script.command_ledger().is_empty());
        assert!(matches!(
            script.next_action(&observation(1)),
            Some(TacticalAction::DeployMcv {
                attempt: DeployAttempt::Second,
                ..
            })
        ));
        assert_eq!(script.command_ledger().len(), 1);
    }

    #[test]
    fn first_deploy_may_create_the_exact_yard_directly() {
        let mut script = TacticalScript::new(stock_config()).expect("valid config");
        let first = script.next_action(&observation(0)).expect("deploy action");
        record_if_command(&mut script, 0, &first);

        let mut direct = observation(1);
        direct.entities.clear();
        direct
            .entities
            .push(entity(YARD_ID, "NACNST", YARD_CELL, 0, true));
        assert_eq!(script.next_action(&direct), None);
        assert_eq!(script.stage(), TacticalScriptStage::AwaitYardConstruction);
        assert_eq!(
            script
                .structure_bindings()
                .yard
                .as_ref()
                .map(|yard| yard.stable_id),
            Some(YARD_ID)
        );
    }

    #[test]
    fn configuration_rejects_rate_and_ledger_shortcuts() {
        let mut zero_rate = stock_config();
        zero_rate.power.expected_rate_frames = 0;
        assert_eq!(
            TacticalScript::new(zero_rate).err(),
            Some(TacticalScriptConfigError::EmptyProductionRate)
        );

        let mut drifted_ready = stock_config();
        drifted_ready.power.expected_ready_tick += 1;
        assert_eq!(
            TacticalScript::new(drifted_ready).err(),
            Some(TacticalScriptConfigError::ProductionReadyLedger {
                role: StructureRole::Power,
            })
        );

        let mut duplicate = stock_config();
        duplicate.refinery.type_id = duplicate.power.type_id.clone();
        assert_eq!(
            TacticalScript::new(duplicate).err(),
            Some(TacticalScriptConfigError::DuplicateTargets)
        );
    }

    #[test]
    fn first_deploy_rejects_wrong_identity_reusing_the_mcv_stable_id() {
        let mut script = TacticalScript::new(stock_config()).expect("valid config");
        let first = script.next_action(&observation(0)).expect("deploy action");
        record_if_command(&mut script, 0, &first);

        let mut wrong_identity = observation(1);
        wrong_identity.entities[0].type_id = "HTNK".to_string();
        assert!(matches!(
            script.next_action(&wrong_identity),
            Some(TacticalAction::Fail {
                failure: TacticalFailure {
                    code: TacticalFailureCode::CommandResultMissing,
                    ..
                }
            })
        ));
    }

    #[test]
    fn failed_second_deploy_is_terminal() {
        let mut script = TacticalScript::new(stock_config()).expect("valid config");
        let first = script.next_action(&observation(0)).expect("first deploy");
        record_if_command(&mut script, 0, &first);
        let second = script.next_action(&observation(1)).expect("second deploy");
        assert!(matches!(
            second,
            TacticalAction::DeployMcv {
                attempt: DeployAttempt::Second,
                ..
            }
        ));
        record_if_command(&mut script, 1, &second);

        let mut failed = observation(2);
        failed.entities.retain(|entity| entity.type_id != "NACNST");
        failed
            .entities
            .push(entity(1, "SMCV", YARD_CELL, 0x80, false));
        assert!(matches!(
            script.next_action(&failed),
            Some(TacticalAction::Fail {
                failure: TacticalFailure {
                    code: TacticalFailureCode::CommandResultMissing,
                    ..
                }
            })
        ));
    }

    #[test]
    fn next_queue_requires_exact_enabled_strict_option() {
        let mut script = TacticalScript::new(stock_config()).expect("valid config");
        let actions = drive_until(&mut script, 32, |tick, obs| {
            if tick == 32 {
                obs.build_options[0].enabled = false;
            }
        });
        assert!(matches!(
            actions.last(),
            Some((
                32,
                TacticalAction::Fail {
                    failure: TacticalFailure {
                        code: TacticalFailureCode::ObservationInvalid,
                        ..
                    }
                }
            ))
        ));
    }

    #[test]
    fn queue_result_requires_expected_live_rate() {
        let mut script = TacticalScript::new(stock_config()).expect("valid config");
        let actions = drive_until(&mut script, 33, |tick, obs| {
            if tick == 33 {
                obs.queued_production[0].resolved_rate_frames = 12;
            }
        });
        assert!(matches!(
            actions.last(),
            Some((
                33,
                TacticalAction::Fail {
                    failure: TacticalFailure {
                        code: TacticalFailureCode::ExpectedLedgerDrift,
                        ..
                    }
                }
            ))
        ));
    }

    #[test]
    fn first_factory_sweep_must_resolve_the_constructor_rate() {
        let mut script = TacticalScript::new(stock_config()).expect("valid config");
        let actions = drive_until(&mut script, 34, |tick, obs| {
            if tick == 34 {
                obs.queued_production[0].resolved_rate_frames = 0;
            }
        });
        assert!(matches!(
            actions.last(),
            Some((
                34,
                TacticalAction::Fail {
                    failure: TacticalFailure {
                        code: TacticalFailureCode::ExpectedLedgerDrift,
                        ..
                    }
                }
            ))
        ));
    }

    #[test]
    fn active_target_must_remain_the_only_local_production_until_ready() {
        let mut script = TacticalScript::new(stock_config()).expect("valid config");
        let actions = drive_until(&mut script, 100, |tick, obs| {
            if tick == 100 {
                obs.queued_production.clear();
            }
        });
        assert!(matches!(
            actions.last(),
            Some((
                100,
                TacticalAction::Fail {
                    failure: TacticalFailure {
                        code: TacticalFailureCode::CommandResultMissing,
                        ..
                    }
                }
            ))
        ));
    }

    #[test]
    fn placement_result_requires_ready_entry_consumed() {
        let mut script = TacticalScript::new(stock_config()).expect("valid config");
        let actions = drive_until(&mut script, 618, |tick, obs| {
            if tick == 618 {
                obs.ready_buildings.push("NAPOWR".to_string());
            }
        });
        assert!(matches!(
            actions.last(),
            Some((
                618,
                TacticalAction::Fail {
                    failure: TacticalFailure {
                        code: TacticalFailureCode::CommandResultMissing,
                        ..
                    }
                }
            ))
        ));
    }

    #[test]
    fn a_bound_structure_disappearing_is_terminal() {
        let mut script = TacticalScript::new(stock_config()).expect("valid config");
        let actions = drive_until(&mut script, 651, |tick, obs| {
            if tick == 651 {
                obs.entities.retain(|entity| entity.stable_id != 20);
                obs.power_authority_sufficient = false;
            }
        });
        assert!(matches!(
            actions.last(),
            Some((
                651,
                TacticalAction::Fail {
                    failure: TacticalFailure {
                        code: TacticalFailureCode::ObservationInvalid,
                        ..
                    }
                }
            ))
        ));
    }

    #[test]
    fn capture_completion_cannot_advance_the_simulation_past_the_ledger_tick() {
        let mut script = TacticalScript::new(stock_config()).expect("valid config");
        let actions = drive_until(&mut script, 3711, |_, _| {});
        assert!(matches!(
            actions.last(),
            Some((3711, TacticalAction::Capture))
        ));

        let mut late = observation(3712);
        late.capture_complete = true;
        assert!(matches!(
            script.next_action(&late),
            Some(TacticalAction::Fail {
                failure: TacticalFailure {
                    code: TacticalFailureCode::ExpectedLedgerDrift,
                    ..
                }
            })
        ));
    }

    #[test]
    fn stage_wall_budget_is_strict() {
        let mut script = TacticalScript::new(stock_config()).expect("valid config");
        let first = script.next_action(&observation(0)).expect("first deploy");
        record_if_command(&mut script, 0, &first);
        let mut late = observation(1);
        late.wall_elapsed_ms = 15_001;
        assert!(matches!(
            script.next_action(&late),
            Some(TacticalAction::Fail {
                failure: TacticalFailure {
                    code: TacticalFailureCode::BudgetExceeded,
                    ..
                }
            })
        ));
    }

    #[test]
    fn schedule_record_must_use_live_profile_delay() {
        let mut script = TacticalScript::new(stock_config()).expect("valid config");
        let first = script.next_action(&observation(0)).expect("first deploy");
        let action_id = match first {
            TacticalAction::DeployMcv { action_id, .. } => action_id,
            other => panic!("unexpected action: {other:?}"),
        };
        assert!(matches!(
            script.record_scheduled(action_id, 0, 3),
            Err(TacticalFailure {
                code: TacticalFailureCode::CommandScheduleInvalid,
                ..
            })
        ));
    }
}
