//! Game simulation - owns the EntityStore and deterministic tick stepping.
//!
//! The Simulation is the authoritative game state. It spawns entities from
//! map data, executes command envelopes on fixed ticks, advances gameplay
//! systems, and exposes deterministic state hashing for replay/desync checks.
//!
//! Responsibility boundaries:
//! - `command_schedule.rs`: queue admission, due batches, house order and group destinations
//! - `world_commands.rs`: individual payloads and selection/ownership helpers
//! - `world_hash.rs` — deterministic state-hash folds
//! - `world_spawn.rs` — entity spawning from map data and production
//! - `world_orders.rs` — click-order helpers, building entries and pending building detonation
//! - `lifecycle.rs` / `substrate.rs` — object transitions, stores and registration order
//! - `fork.rs` — every field shared, forked and restored (`util::forked`)
//! - `object_turn.rs` — complete live-object turn and pass outcomes
//! - `techno_ai.rs` — per-object AI dispatch within each turn
//! - `sim_events.rs` — sound, EVA and fire events a frame emits for the app
//! - `sim_combat_commit.rs` — fire visits, damage receivers and attack/loss events
//! - `sim_state_access.rs` — RNG views, accessors, installs and test mutators
//! - `sim_construction.rs` — constructors, rule handles, RNG streams, store access
//! - `sim_map_services.rs` — smudge/tiberium, lighting, ore growth, map area, triggers
//! - `sim_object_admission.rs` — projectile/wave/debris admission and membership checks
//! - `sim_house_ownership.rs` — house self-heal, derived house state and owner changes
//! - `sim_navigation_rebuild.rs` — post-load caches, path/zone grids, wall damage
//! - `sim_frame_upkeep.rs` — vision and fog, building-down visits, wakes, hash cadence

pub(crate) mod authored_load_host;
mod bounce_terrain;
pub mod bridge_orchestrator;
#[cfg(any(test, feature = "test-support"))]
pub mod bridge_test_evidence;
mod building_after_destruction;

#[cfg(all(test, tests_sim_world_a_b))]
mod building_after_destruction_tests;
pub(crate) mod building_anim;
mod building_scatter;
mod building_upgrade_art;
mod camera_home;
mod cell_content;
mod crash;
mod death_frames;
mod disk_laser;
pub use disk_laser::DiskLaserObservation;
#[cfg(all(test, tests_sim_world_c_o))]
mod disk_laser_tests;
#[cfg(test)] // shared by test groups
pub(crate) mod entry_test_fixture;
mod fork;
mod frame_channel;
mod frame_effects;
mod ice;
mod idle_sleep;
#[cfg(all(test, tests_sim_world_c_o))]
mod idle_sleep_tests;
mod network_commands;
mod object_entry;
mod queued_command;
mod quiet_units;
#[cfg(all(test, tests_sim_world_p_s))]
mod quiet_units_tests;
mod sinking;
mod steady_ai;
#[cfg(all(test, tests_sim_world_p_s))]
mod steady_ai_tests;
mod terrain_deformation;
mod time_warp;
mod trigger_chrono;
mod turn_streams;
#[cfg(all(test, tests_sim_world_t_z))]
mod turn_streams_tests;
mod type_tech_levels;
mod voxel_anim_effects;
pub(crate) use voxel_anim_effects::BounceContactDamage;
mod unit_repair_seek;
pub(crate) use frame_channel::FrameChannel;
pub use frame_effects::{
    FrameClient, FrameEffects, LocalPlanningBoundary, NetworkCommandBoundary, NetworkCommandError,
    OutcomeWaitBoundary, SoundBoundary, TacticalRadarBoundary,
};
pub use network_commands::{
    NetworkCommandQueues, NetworkInterruption, NetworkQueueContext, NetworkQueueError,
    NetworkQueuePass,
};
pub(crate) use type_tech_levels::TypeTechLevels;
#[cfg(all(test, tests_sim_world_t_z))]
mod unit_follows_tests;
#[cfg(all(test, tests_sim_world_t_z))]
mod unit_unlimbo_tests;
pub(crate) use sinking::{LivingSinkTrigger, SinkingState};
pub mod edge_cell;
mod gap_generator;
pub(crate) use gap_generator::GapGeneratorRuntime;
pub(crate) use gap_generator::LastOperational;
mod house_base;
#[cfg(all(test, tests_sim_world_c_o))]
mod house_cheer_tests;
mod house_defeat;
pub(crate) use house_defeat::{HouseBlowupSubset, PlayerOutcomeRequest};
mod alpha_shape;
mod ion_blast;
mod spotlight;
pub(crate) use house_base::HouseBaseState;
pub use ion_blast::RetainedDominatorRipples;
pub use spotlight::BuildingSpotlight;
mod infantry_death;
mod jumpjet_cruise;
pub use infantry_death::InfantryDeathPostlude;
pub use infantry_death::InfantryDeathSequence;
mod aircraft_attack;
mod aircraft_enter;
mod aircraft_fire_location;
mod aircraft_move;
#[cfg(test)]
pub(crate) use aircraft_move::tests::fixture as aircraft_search_fixture;
mod building_working_sound;
pub use building_working_sound::working_sound_building;
#[cfg(all(test, tests_sim_world_c_o))]
mod civilian_evacuation_tests;
pub(crate) mod damage_consequences;
pub mod display_layers;
mod display_registry;
mod expiry_listeners;
mod fly_landing;
mod fly_orders;
mod fly_process;
mod frame_error;
#[cfg(all(test, tests_sim_world_c_o))]
mod kratos_wreck_tests;
mod lifecycle;
mod master_frame;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use master_frame::FrameStep;
mod load_object_lifecycle;
mod logic_vector;
mod move_cell_input;
pub use move_cell_input::{CellActionQuery, MoveAction};
mod move_sound;
mod object_sound;
pub use object_sound::{ambient_sound_owner, object_sound_object};
mod turret_rotate_sound;
pub(crate) use move_sound::MoveSoundState;
pub(crate) use playfield_membership::PlayfieldMembership;
pub(crate) use selection_voice::DeployVoiceSuppression;
pub(crate) use turret_rotate_sound::TurretRotateLatch;
pub use turret_rotate_sound::turret_rotate_sound_techno;
#[cfg(all(test, tests_sim_world_c_o))]
mod main_sound_identity_tests;
#[cfg(all(test, tests_sim_world_c_o))]
mod move_sound_tests;
#[cfg(any(test, feature = "test-support"))]
mod native_cell_input_test_fixture;
mod navigation;
mod object_turn;
mod playfield_membership;
mod rally_cell_input;
mod rocket_flight;
pub use frame_error::FrameAdvanceError;
pub(crate) use world_orders::CapturedInfantryType;
pub use world_orders::EngineerBuildingAction;
pub use world_orders::HealArmKeys;
pub use world_orders::InfiltrateAction;
pub use world_orders::UnitRepairAction;
pub(crate) use world_orders::VehicleCaptureOutcome;
pub use world_orders::{ControlProjection, InfantryHealAction};
pub use world_orders::{EntryBaseAction, EntryClickAction};
mod fog_objects;
mod projectile_collision;
mod shroud_refresh;
mod substrate;
mod techno_ai;
mod track_cell_recalc;
mod trigger_camera;
#[cfg(all(test, tests_sim_world_t_z))]
mod trigger_camera_tests;
mod trigger_explosion;
mod trigger_strikes;
pub(crate) use shroud_refresh::{HumanPlayerClients, ParanoidMaps};
pub(crate) use techno_ai::AresPrismForwarding;
pub(crate) use techno_ai::ObjectAiCtx;
pub(crate) use techno_ai::PassiveScan;
pub(crate) use techno_ai::PatrolPost;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use techno_ai::PendingBuildingFire;
pub(crate) use techno_ai::PrismSupport;
pub(crate) use techno_ai::direct_greatest_threat;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use techno_ai::dispatch_foot_mission;
pub(crate) use techno_ai::enter_idle_mode;
pub(crate) use techno_ai::foot_enter_idle_mode_selection;
pub(crate) use techno_ai::passive_target_acquire;
pub(crate) use techno_ai::queue_and_commence;
pub(crate) use techno_ai::retaliate_and_scan;
pub(crate) use techno_ai::techno_unlimbo_idle_mode;
pub use techno_ai::{BuildingDelayedFire, DelayedFire};
pub(crate) use techno_ai::{can_fire_at, weapon_at_index};
mod command_schedule;
mod planning;
pub use planning::{
    PlanningAppendMessage, PlanningBadgeKind, PlanningDrawGraph, PlanningDrawNode,
    PlanningDrawSegment, PlanningPressRefusal, PlanningSelectionCheck,
};
mod selection_voice;
pub(crate) mod techno_ai_cloak;
pub(crate) mod unit_post;
#[cfg(all(test, tests_sim_world_t_z))]
mod wall_autofill_oracle_tests;
#[cfg(all(test, tests_sim_world_t_z))]
mod wall_restore_tests;
mod world_commands;
pub use world_commands::Selection;
mod world_hash;

#[cfg(all(test, tests_sim_world_c_o))]
mod custom_foundation_tests;
mod world_orders;
mod world_spawn;

#[cfg(all(test, tests_sim_world_a_b))]
mod aegis_distributed_fire_tests;
#[cfg(all(test, tests_sim_world_a_b))]
mod aircraft_deployment_tests;
#[cfg(all(test, tests_sim_world_a_b))]
mod area_rocker_tests;
#[cfg(all(test, tests_sim_world_a_b))]
mod brute_smash_debris_tests;
#[cfg(all(test, tests_sim_world_a_b))]
mod building_discovery_services_tests;
#[cfg(all(test, tests_sim_world_a_b))]
mod building_fatal_lifecycle_tests;
#[cfg(all(test, tests_sim_world_a_b))]
mod building_power_roster_tests;
#[cfg(all(test, tests_sim_world_c_o))]
mod crash_tests;
#[cfg(all(test, tests_sim_world_c_o))]
mod damage_consequence_tests;
#[cfg(all(test, tests_sim_world_c_o))]
mod drone_rocking_tests;
#[cfg(all(test, tests_sim_world_c_o))]
mod engineer_building_repair_tests;
#[cfg(all(test, tests_sim_world_c_o))]
mod engineer_power_consumer_tests;
#[cfg(all(test, tests_sim_world_c_o))]
mod finite_ammo_tests;
#[cfg(all(test, tests_sim_world_c_o))]
mod fly_height_tests;
#[cfg(any(test, feature = "test-support"))]
pub mod gap_generator_tests;
#[cfg(all(test, tests_sim_world_c_o))]
mod gap_supercharge_tests;
#[cfg(all(test, tests_sim_world_c_o))]
mod gsi_04_18_tests;
#[cfg(all(test, tests_sim_world_c_o))]
mod house_ai_activation_tests;
#[cfg(all(test, tests_sim_world_c_o))]
mod jumpjet_infantry_tests;
#[cfg(any(test, feature = "test-support"))]
mod lifecycle_tests;
#[cfg(all(test, tests_sim_world_c_o))]
mod mastermind_overload_tests;
#[cfg(all(test, tests_sim_world_p_s))]
mod projectile_degenerates_tests;
#[cfg(all(test, tests_sim_world_p_s))]
mod projectile_trailer_tests;
#[cfg(all(test, tests_sim_world_p_s))]
mod squid_grapple_tests;
#[cfg(all(test, tests_sim_world_p_s))]
mod suicide_fire_tests;
#[cfg(any(test, feature = "test-support"))]
pub use lifecycle_tests::common_raw_terrain_cell as common_raw_test_terrain_cell;
#[cfg(all(test, tests_sim_world_c_o))]
mod overlay_explodes_tests;
#[cfg(all(test, tests_sim_world_t_z))]
mod team_script_vm_tests;
#[cfg(all(test, any(tests_sim_world_t_z, tests_sim_trigger)))]
pub(in crate::sim) mod voxel_meteor_tests;
#[cfg(all(test, tests_sim_world_t_z))]
mod voxel_water_expiry_tests;

pub(crate) use crash::CrashLatch;
pub(crate) use display_registry::entity_layer;
pub use lifecycle::LifecycleOutput;
pub(crate) use lifecycle::object_get_coords_cell;
pub use lifecycle::{
    BuildingLight, ConcealOutcome, ObjectLifecycle, PlacementEvidence, RemovalFacts, RevealOutcome,
    RevealPosition, RevealRequest, TechnoDiscoveryHistory, TerminalLifetime, UninitContext,
};
#[cfg(any(test, feature = "test-support"))]
pub use lifecycle::{LifecycleTestEvent, RevealFailure};
pub(crate) use load_object_lifecycle::LoadObjectLifecycle;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use load_object_lifecycle::LoadOverlayHandle;
pub use load_object_lifecycle::LoadOverlayLifecycleError;
pub(crate) use logic_vector::{LogicMembership, LogicVector};
pub(crate) use object_entry::{EntryFacts, FootEntryReceiver};
#[cfg(any(test, feature = "test-support"))]
pub use substrate::DeferredDeleteQueue;
pub(crate) use substrate::{DeferredDeleteTicket, ObjectKind, ObjectSubstrate};
pub use substrate::{EnterOrderCounter, StableIdCursor};
pub(in crate::sim) use world_spawn::TypeProjectionWrite;
pub(crate) use world_spawn::{CrushableFacts, MoverFacts, TypeFacts};
pub use world_spawn::{GeneratedTechnoInitError, GeneratedTechnoInitTable};

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::map::entities::EntityCategory;
#[cfg(any(test, feature = "test-support"))]
use crate::map::houses::HouseAllianceMap;
use crate::map::overlay::OverlayEntry;
use crate::map::playfield::PlayfieldBounds;
use crate::map::resolved_terrain::{
    RealCellBridgeFlags0x1180, ResolvedTerrainGrid, SharedCellDummy,
};
use crate::rules::locomotor_type::SpeedType;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::animation;
use crate::sim::bridge_state::BridgeRuntimeState;
use crate::sim::command::CommandEnvelope;
use crate::sim::entity_store::EntityStore;
use crate::sim::house_diplomacy::HouseAlliances;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::intern::StringInterner;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::overlay_grid::{
    OverlayGrid, WallDamageTransactionHost, WallDirtyStep, WallPointerTarget,
};
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::terrain_speed;
use crate::sim::pathfinding::zone_incremental::{
    PackedZoneCoord, ZoneRepairKind, repair_zone_cell,
};
use crate::sim::pathfinding::zone_map::ZoneGrid;
#[cfg(test)]
use crate::sim::power_system;
use crate::sim::production::{self, ProductionState};
use crate::sim::projectile::{
    Projectile, ProjectileBridgeCrossing, ProjectileCollisionResponse, ProjectileCoord,
    projectile_bridge_crossing,
};
use crate::sim::rng::{MainRng, SimRng, SimRngLogicalState, SimRngLogicalView};
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::team_script_vm::TeamScriptVm;
use crate::sim::trigger_runtime::{TriggerEffect, TriggerRuntime};
#[cfg(test)]
use crate::sim::vision;
use crate::sim::vision::FogState;
#[cfg(test)]
use crate::util::fixed_math::SimFixed;
use crate::util::forked::Forked;

/// Test fixtures' seed. Real launches negotiate a per-match seed through
/// `ScenarioDescriptor`, and only test builds can construct with this value.
#[cfg(any(test, feature = "test-support"))]
const DEFAULT_SIM_SEED: u64 = 0x5EED_CAFE_D15E_A5E5;

/// Result of one deterministic simulation tick.
#[derive(Debug, Clone, Copy)]
pub struct TickResult {
    pub tick: u64,
    /// Whether the late frame/tick commit and pending-delete drain ran.
    /// Victory, defeat, quit, and connection-loss exits leave this false.
    pub frame_committed: bool,
    pub executed_commands: usize,
    /// The state hash after this frame, on the frames
    /// [`Simulation::hash_state_every`] selects and on every frame that ends
    /// the match (`frame_committed` false); `None` on the others.
    pub state_hash: Option<u64>,
    /// This call latched the natural win/loss score snapshot before hashing.
    pub terminal_score_finalized: bool,
    pub spawned_entities: bool,
    /// A structure was destroyed (combat, sell, crush); the frame finalizer
    /// rebuilds navigation to unblock the footprint.
    pub destroyed_structure: bool,
    /// An entity's owner changed (garrison reconciliation, engineer capture).
    /// The atlases learn of it from [`Simulation::take_looks`].
    pub ownership_changed: bool,
    /// Bridge state changed this tick. Mutation owners publish navigation at
    /// their synchronous reader boundary; the frame finalizer also rebuilds
    /// the projection before committing the frame for subsequent ticks.
    pub bridge_state_changed: bool,
}

/// What [`Simulation::take_looks`] reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedLooks {
    /// Entities stored since, or given a new owner, type or disguise.
    pub entities: crate::sim::Touched,
    /// Anims stored since, or given a new colour remap or type.
    pub anims: crate::sim::Touched,
}

/// The master spine retains its admitted command batch for the live recorder.
/// Fixture callers can consume the tick without draining presentation queues.
#[derive(Debug)]
pub(crate) struct MasterFrameOutput {
    tick: TickResult,
    admitted_commands: Vec<CommandEnvelope>,
    network_failure: Option<crate::sim::command_queue::ChecksumMismatch>,
    network_waiting: bool,
    outcome_waiting: bool,
}

impl MasterFrameOutput {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn into_tick(self) -> TickResult {
        self.tick
    }
}

/// Saved continuation after the CRC modal. This is control state, not a
/// second command queue: DoList retains the original bytes and processed bits.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct NetworkFailureContinuation {
    failure: crate::sim::command_queue::ChecksumMismatch,
    tick_ms: u32,
    frame: FrameCompletion,
}

// Explicit variants keep the continuation describable by the binary snapshot
// schema. Its layout fingerprint rejects snapshots with the earlier shape.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
enum FrameContinuation {
    Checksum(NetworkFailureContinuation),
    Interrupted(NetworkInterruptionContinuation),
    Waiting(NetworkWaitContinuation),
    Outcome(HouseOutcomeContinuation),
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct HouseOutcomeContinuation {
    cursor: house_defeat::HouseOutcomeCursor,
    tick_ms: u32,
    frame: FrameCompletion,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct NetworkInterruptionContinuation {
    interruption: NetworkInterruption,
    tick_ms: u32,
    frame: FrameCompletion,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct NetworkWaitContinuation {
    context: NetworkQueueContext,
    tick_ms: u32,
    frame: FrameCompletion,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct FrameCompletion {
    commands: Vec<CommandEnvelope>,
    execute_tick: u64,
    executed_commands: usize,
    spawned_entities: bool,
    destroyed_structure: bool,
    ownership_changed: bool,
    bridge_state_changed: bool,
}

enum FrameTailResult {
    Complete(bool),
    Checksum(crate::sim::command_queue::ChecksumMismatch),
    Interrupted(NetworkInterruption),
    Waiting(NetworkQueueContext),
    OutcomeWait(house_defeat::HouseOutcomeCursor),
}

/// One authoritative frame plus the transient facts emitted while producing it.
///
/// Channel order is preserved within each vector. The app owns cross-channel
/// presentation order; collecting this value only transfers ownership and does
/// not invent a single mixed event timeline.
#[derive(Debug)]
pub struct SimFrameOutput {
    /// Logic finished but Queue_AI is still waiting for transport/peer credit.
    pub network_waiting: bool,
    /// House4F860F/4F8736 is waiting for the process-owned Vox drain.
    /// No frame commit, later House visit or presentation frame ran yet.
    pub outcome_waiting: bool,
    /// Post-Logic suspension at Execute_DoList64CC61. Earlier effects stand;
    /// acknowledge the client modal, then resume the remaining frame tail once.
    pub network_failure: Option<crate::sim::command_queue::ChecksumMismatch>,
    admitted_commands: Vec<CommandEnvelope>,
    pub tick: TickResult,
    pub trigger_effects: Vec<TriggerEffect>,
    pub lifecycle_outputs: Vec<LifecycleOutput>,
    pub overlay_updates: Vec<OverlayEntry>,
    /// Coordinates whose overlay identity was erased this frame. Presentation
    /// drops their render entry; `overlay_updates` only ever upserts occupied
    /// cells and can never express a removal.
    pub overlay_removals: Vec<(u16, u16)>,
    pub sound_events: Vec<SimSoundEvent>,
    /// Prefix synchronously delivered at reached native handle boundaries.
    /// Other consumers still inspect the complete facts; audio takes the suffix.
    pub sound_events_delivered: usize,
    pub fire_events: Vec<SimFireEvent>,
    pub combat_lights: Vec<crate::sim::combat::CombatLightRequest>,
    /// Ordered axis writes from Bullet DetonateAtCoord; presentation owns the
    /// retained screen offset and applies these after Logic.
    pub screen_shake_updates: Vec<crate::sim::combat::ScreenShakeUpdate>,
    pub lighting_events: Vec<crate::sim::light_sources::LightingEvent>,
    /// The sync checksum this frame's multiplayer `Queue_AI` computed for
    /// the native history ring (retail fold plus VERA's money, health and
    /// trigger terms); `None` on single-player routes and on
    /// frames that terminated before the tail.
    pub multiplayer_checksum: Option<FrameSyncChecksum>,
}

/// One frame's multiplayer sync checksum, stamped with the pre-commit
/// native frame it describes (`Frame` when `0x00647684` runs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameSyncChecksum {
    pub frame: u32,
    pub checksum: Result<
        crate::sim::multiplayer_checksum::MultiplayerChecksumFrame,
        crate::sim::multiplayer_checksum::MultiplayerChecksumError,
    >,
}

/// Which native `Queue_AI @ 0x00647260` body the frame tail runs. gamemd
/// selects it from GameMode through the jump table at `0x006475D8`: modes 0
/// (campaign) and 5 (skirmish) take the single-player body at `0x006474DC`;
/// modes 1..=4 call `Queue_AI_Multiplayer @ 0x006475F0`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CommandQueueRoute {
    #[default]
    SinglePlayer,
    Multiplayer,
}

impl SimFrameOutput {
    pub fn take_admitted_commands(&mut self) -> Vec<CommandEnvelope> {
        std::mem::take(&mut self.admitted_commands)
    }
}

/// Front-end admission lane for one Main_Tick call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickLane {
    /// Normal gameplay: local selection/session ingress before live Logic,
    /// then EventClass gameplay dispatch in the Main_Tick tail.
    Ordinary,
    /// LAN/WOL modal pump: service PerTickUpdate and the late tail only.
    NetworkModal,
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HouseAiActivationOrderTestEvent {
    ProductionCompleted,
    HouseAngerDecay(InternedId),
    HouseActivation(InternedId),
    DefeatProcessed,
}

mod sim_combat_commit;
mod sim_construction;
mod sim_events;
mod sim_frame_upkeep;
mod sim_house_ownership;
mod sim_map_services;
mod sim_navigation_rebuild;
mod sim_object_admission;
mod sim_state_access;

pub use sim_events::AllianceEva;
pub use sim_events::AresTechCaptureLines;
pub use sim_events::AresUnitLostLine;
#[cfg(any(test, feature = "test-support"))]
pub use sim_events::FireOriginSnapshot;
pub use sim_events::SimFireEvent;
pub use sim_events::SimSoundEvent;
pub use sim_events::TriggerSoundTarget;

/// Named observations of the three native RNG authorities. Main is an owned
/// coherent boundary because retained playback may draw on another thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimulationRngViews<'a> {
    pub scenario: SimRngLogicalView<'a>,
    pub main: SimRngLogicalState,
    pub mapgen: SimRngLogicalView<'a>,
}

/// Owned logical RNG evidence used by the Rust pre-first-tick receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimulationRngState {
    pub scenario: SimRngLogicalState,
    pub main: SimRngLogicalState,
    pub mapgen: SimRngLogicalState,
}

fn deserialized_main_rng_placeholder() -> Forked<SimRng> {
    Forked::new(SimRng::new(0))
}

fn every_frame() -> Forked<Option<std::num::NonZeroU64>> {
    Forked::new(Some(std::num::NonZeroU64::MIN))
}

/// The game simulation - owns all authoritative game state.
/// Every field is a [`Forked`] value, and so is every field of `substrate`:
/// a world forks field by field, and a fork tells which fields it wrote
/// (`fork.rs`).
/// `Clone` serves exactly one copy, through [`Simulation::presentation_copy`]:
/// the world without its objects that drawing reads beside the presentation
/// frame's records while a tick runs on the simulation thread
/// (docs/sim-thread.md). No authority is copied: gameplay owns the one live
/// world.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Simulation {
    /// String interner for owner/type_ref — zero-cost ID clones instead of heap Strings.
    pub(crate) interner: Forked<crate::sim::intern::StringInterner>,
    /// Runtime TechnoType+634 writes; RuleSet remains the frozen read projection.
    #[serde(default)]
    type_tech_levels: Forked<TypeTechLevels>,
    /// Runtime BuildingType normal PowerUp animation names (44099B..4409C4).
    /// Shared by all instances of the type; the upgrade/art owner mutates it.
    #[serde(default)]
    building_upgrade_art_names: Forked<crate::sim::building_upgrades::BuildingUpgradeArtNames>,
    /// VoxelAnimAI74A8F8 supplies the live Type Damage field to receivers.
    #[serde(default)]
    voxel_type_damage: Forked<crate::sim::voxel_anim::VoxelTypeDamage>,
    /// Pre-resolved `[CombatDamage]` warhead handles (F04). Built by
    /// `resolve_type_handles` beside the type table; rebuilt from rules on
    /// load. NOT serialized, NOT hashed. `None` until resolved.
    #[serde(skip)]
    rule_handles: Forked<Option<crate::sim::type_handle_table::ResolvedRuleHandles>>,
    /// Factories, the enqueue stamp and the primary-factory flags.
    pub(crate) production: Forked<ProductionState>,
    /// Live terrain objects and their cell, animation and spawner indexes.
    pub(crate) terrain_objects: Forked<crate::sim::terrain_object::TerrainObjectStore>,
    /// Ore growth/spread configuration resolved from merged INI sources.
    pub(crate) ore_growth_config: Forked<crate::sim::ore_growth::OreGrowthConfig>,
    /// Per-TiberiumClass growth and spread queues, bitmaps and timers.
    pub(crate) ore_growth_state: Forked<crate::sim::ore_growth::OreGrowthState>,
    /// Session aggregate — scenario identity, seed, authoritative map
    /// bounds, MP start table, per-match options, and the frame clocks
    /// (`tick` plus the wrapping `binary_frame`, committed late at the end of
    /// `advance_tick`). `total_sim_ms` is diagnostic only. Constructed once
    /// from the app-layer descriptor; serialized + hashed except for that
    /// diagnostic accumulator. See `sim::scenario_session`.
    pub(crate) session: Forked<ScenarioSession>,
    /// Opt-in trainer state. No save, checksum, or normal-build representation.
    #[cfg(feature = "debug-tools")]
    #[serde(skip)]
    pub(crate) debug_tools: Forked<crate::sim::cheats::DebugToolsState>,
    /// Opt-in wall-clock observations. Never part of saves, hashes, or gameplay.
    #[serde(skip)]
    frame_profiling: Forked<bool>,
    #[serde(skip)]
    frame_profile: Forked<crate::sim::frame_profile::FrameProfile>,
    /// Native A8E7AC is a synchronous, wrapping placement/destruction scope,
    /// independent of GameMode. Balanced at frame boundaries; not saved/hash
    /// state. Only lifecycle's nested scope API mutates it.
    #[serde(skip, default)]
    object_placement_scope_depth: Forked<u32>,
    /// VERA's analog of native `0xA8ED6B`, which sends an initial building
    /// Unlimbo to Enter_Idle_Mode's Idle arm. Raised only by lifecycle's
    /// bypass API; balanced at frame boundaries; not saved/hash state.
    #[serde(skip, default)]
    buildup_bypass: Forked<bool>,
    /// Phobos `WarheadTypeExt::Reflected`: the warhead whose own damage is
    /// being reflected right now, so a reflected hit never reflects again.
    /// Synchronous, never saved or hashed.
    #[cfg(feature = "phobos")]
    #[serde(skip, default)]
    pub(crate) reflected_warhead: Forked<Option<crate::sim::intern::InternedId>>,
    /// Ares CanMakeStuffUp's RulesClass writes (100677B0, hook 0x687C16),
    /// drawn once at scenario start; saved and hashed. See
    /// `sim::ares_make_stuff_up`.
    #[serde(default)]
    pub(crate) ares_made_up_rules: Forked<Option<crate::sim::ares_make_stuff_up::AresMadeUpRules>>,
    /// Scenario RNG — gamemd `Scenario->Random` (Scen+0x218). Drives in-object-tick
    /// sim draws: scatter, sub-cell placement, smudge/destruction, particles,
    /// wall/overlay damage, bridge collapse/destruction presentation, ore growth/spread, TIBTRE,
    /// anim scorch/50-50, miner-dock jitter, and the terminal score projection.
    /// The snapshot load policy decides whether to resume this cursor or apply
    /// the native player-load reset. It is always hashed while live. With
    /// turn streams on, a Logic slot's turn draws from its own generator,
    /// held in this value while the turn runs (`turn_streams`).
    pub(crate) scenario_rng: Forked<SimRng>,
    /// Main/global RNG — gamemd `g_MainRng` (0x00886B88). This is a
    /// process-global cursor: it is neither part of ScenarioClass saves nor
    /// multiplayer checksums, and the live cursor continues across a load.
    /// The simulation owns this copy alone and draws it through `&mut`; the
    /// app keeps the process cell ([`MainRng`], `install_fresh_process_main`).
    #[serde(skip, default = "deserialized_main_rng_placeholder")]
    pub(crate) main_rng: Forked<SimRng>,
    /// Map-generator RNG — gamemd `g_MapGenRng` (0x00ABE890). VERA's fresh
    /// fixed-map construction uses `Random__Seed(0)`, matching the verified
    /// native fresh-process state; launch `.SED` generation installs its exact
    /// post-RMG continuation. Bridge repair consumes this stream; destruction
    /// remains Scenario-owned. Stock player loads retain the process cursor
    /// across in-scenario restore and fresh Scenario handoff.
    /// Network starts seed it from the agreed match seed and hash it, since
    /// bridge repair writes its chosen variant into authoritative overlays.
    /// Original Init_Random_Number_System52FC20 leaves this entire object
    /// unchanged; only random-map generation reseeds it at598996.
    pub(crate) mapgen_rng: Forked<SimRng>,
    /// Independent wrapping `AbstractClass+0x10` identity cursor, Scenario+214.
    /// Native numeric IDs may duplicate and are neither stable handles nor RNG.
    /// Original689310/689470 preserve the cursor across save/load, including
    ///683560's post-read Scenario reinitialization. See native_id_snapshot.
    /// Runtime constructors consume this continuation before class admission.
    pub(crate) native_unique_ids: Forked<Option<crate::sim::native_identity::NativeUniqueIdCursor>>,
    /// `MapClass+0x134` (`0x0087F91C`) analogue: the wrapping signed total that
    /// authored `ScenarioClass::Full_Init @ 0x00686B20` stores from
    /// `InitCellAttributes(0)`'s value-only `Get_Tiberium_Value` pass. No active
    /// reader is proved; cell-array teardown (a new Simulation) resets it and
    /// generated loads leave it `None`.
    #[serde(skip, default)]
    authored_tiberium_value_total: Forked<Option<i32>>,
    #[serde(skip, default)]
    pub(crate) post_load_particle_system_constructed:
        Forked<crate::sim::ore_twinkle::PostLoadGasCloud>,
    /// Successful raw `[Tubes]` constructor bindings from this fresh map read.
    /// Kept separate from resolved topology so its later owning transaction
    /// can consume the already-assigned IDs without recounting filtered facts.
    #[serde(skip, default)]
    pub(crate) native_map_tubes: Forked<crate::map::tubes::NativeMapTubes>,
    /// Shared authored/runtime OverlayClass registry/deferred-delete owner. These
    /// records survive saves so global deferred tickets keep their registry
    /// identities. Their queue order is hashed by the shared deletion owner.
    pub(crate) load_objects: Forked<LoadObjectLifecycle>,
    /// Deterministic fog/shroud visibility state.
    pub(crate) fog: Forked<FogState>,
    /// Each house's ally mask (`HouseClass+0x5788`) as a directed graph,
    /// written by `sim::house_diplomacy`: the scenario's install, then
    /// MakeAlly and MakeEnemy. A write during the object or fog pass must
    /// also forget [`Self::reveal_viewers`], as those writers do.
    pub(crate) house_alliances: Forked<crate::sim::house_diplomacy::HouseAlliances>,
    /// Object substrate — the active-object order plus the monotonic id and
    /// enter-order counters. The single owner the lifecycle contract
    /// (reveal/conceal/unlimbo/uninit) mutates; entity storage and the
    /// occupancy grid migrate here in later stages.
    pub(in crate::sim) substrate: ObjectSubstrate,
    /// Ordered release-visible handoffs produced by lifecycle transactions.
    /// The app drains these without feeding them back into simulation.
    #[serde(skip)]
    pub(crate) lifecycle_outputs: Forked<FrameChannel<LifecycleOutput>>,
    /// Derived presentation interest for the next admitted frame. The app
    /// copies only pending voice owners; this is never a second voice latch.
    /// A deploy conversion renames its old object's entry to the new one
    /// (`move_unit_voice`), following the audio owner's own move.
    /// `advance_app_frame` clears it on success and error, and restore starts
    /// empty. Neither snapshots nor world hashes include this transient set.
    #[serde(skip)]
    unit_voice_visit_interest: Forked<std::collections::BTreeSet<u64>>,
    /// A mission or production step placed objects this frame; the frame tail
    /// folds it into the tick's `spawned_entities`. Derived frame output, not
    /// gameplay state; saves occur at frame boundaries.
    #[serde(skip)]
    mission_spawned_entities: Forked<bool>,
    /// An ice run's Recalc_Ice changed land this frame; the frame tail
    /// rebuilds the zones once (`ice`, docs/ts-ice.md). Derived frame
    /// output, like `mission_spawned_entities`.
    #[serde(skip)]
    ice_zones_stale: Forked<bool>,
    /// The shared ownership chokepoint's report, drained into the tick's
    /// `ownership_changed`, including changes outside the live object pass.
    /// Not saved or hashed.
    #[serde(skip)]
    ownership_changed_since_output: Forked<bool>,
    /// Ordered occupied-cell identities finalized at the authoritative frame
    /// boundary. The app drains these into its render-only overlay list.
    #[serde(skip)]
    frame_overlay_updates: Forked<Vec<OverlayEntry>>,
    /// Ordered coordinates whose overlay identity was erased at the
    /// authoritative frame boundary. Drained into `SimFrameOutput` beside the
    /// occupied upserts.
    #[serde(skip)]
    frame_overlay_removals: Forked<Vec<(u16, u16)>>,
    /// One-shot raw post-match score result.
    pub(super) terminal_score_snapshot: Forked<crate::sim::score::TerminalScore>,
    /// Internal order proof; release builds carry no ledger or recording branch.
    #[cfg(any(test, feature = "test-support"))]
    #[serde(skip)]
    lifecycle_test_events: Forked<Vec<LifecycleTestEvent>>,
    #[cfg(test)]
    #[serde(skip)]
    pub(crate) receiver_fixture:
        Forked<Option<crate::sim::combat::receiver_fixture::FixturePolicy>>,
    /// App-visible outcomes produced by the authoritative trigger rung.
    /// Trigger actions mutate `trigger_runtime` during the frame; only their
    /// presentation outcomes are moved into `SimFrameOutput` after the tick.
    #[serde(skip)]
    trigger_effects: Forked<Vec<TriggerEffect>>,
    /// [`Self::revision`]'s count. Never hashed or saved: a loaded world
    /// starts again from zero under its own rules serial.
    #[serde(skip)]
    revision: Forked<u64>,
    #[cfg(any(test, feature = "test-support"))]
    #[serde(skip)]
    frame_step_trace: Forked<Vec<FrameStep>>,
    /// Focused ordering observer for the bounded House-update activation seam.
    #[cfg(any(test, feature = "test-support"))]
    #[serde(skip)]
    house_ai_activation_order_test_trace: Forked<Vec<HouseAiActivationOrderTestEvent>>,
    /// Sound events produced during the current tick and moved into the owned
    /// app-frame output batch.
    #[serde(skip)]
    pub(crate) sound_events: Forked<FrameChannel<SimSoundEvent>>,
    #[serde(skip)]
    pub(crate) lighting_sources: Forked<crate::sim::light_sources::LightingSources>,
    /// Fire events produced during combat and moved into the app-frame output
    /// for muzzle flash rendering and future projectile origin computation.
    #[serde(skip)]
    pub(crate) fire_events: Forked<FrameChannel<SimFireEvent>>,
    /// Native IC/ForceShield impact combat-light requests emitted in receiver
    /// order during the current master frame. The native object has no owner
    /// and is distinct from AnimClass/ParticleSystemClass, so this remains a
    /// dedicated presentation handoff rather than fabricated world state.
    #[serde(skip)]
    pub(crate) combat_light_requests: Forked<FrameChannel<crate::sim::combat::CombatLightRequest>>,
    /// Ordered Bullet DetonateAtCoord screen-shake writes for this frame.
    #[serde(skip)]
    pub(crate) screen_shake_updates: Forked<FrameChannel<crate::sim::combat::ScreenShakeUpdate>>,
    /// Persistent ordinary shots. This is authoritative save/hash state, not
    /// the render-side fire-event approximation.
    #[serde(default)]
    pub(crate) projectiles: Forked<crate::sim::projectile::ProjectileStore>,
    /// Phobos per-warhead-type animation creation counters.
    #[cfg(feature = "phobos")]
    #[serde(default)]
    pub(crate) warhead_anim_runtime: Forked<crate::sim::warhead_anim::WarheadAnimRuntime>,
    /// Persistent WaveClass registrations. They have their own logic lifetime;
    /// this is not a one-frame weapon-fire presentation list.
    pub(crate) waves: Forked<crate::sim::wave::WaveStore>,
    /// Stable form of TechnoClass +0x324: owner stable id -> exact Wave stable
    /// id. The native link is installed even for a constructor-short Wave and
    /// is cleared only by exact pointer-expiry cleanup.
    #[serde(default)]
    active_wave_links: Forked<BTreeMap<u64, u64>>,
    /// `ObjectClass+0x64`: the custom sound trigger action 99 gave an object
    /// (a Building or Terrain), keyed by stable id. Absent is the
    /// constructor's -1. Only `object_sound` writes it; the object's
    /// destruction removes it. A network trigger retains the authored
    /// name/slot so local playback availability cannot change idle admission.
    #[serde(default)]
    object_custom_sounds: Forked<BTreeMap<u64, crate::rules::sound_ini::SoundReference>>,
    /// Test observation of each refinery unload gate. The dock commits the
    /// gate's smoke and SpecialAnim itself; the frame tail drops these.
    #[cfg(any(test, feature = "test-support"))]
    #[serde(skip)]
    pub(crate) bale_events: Forked<Vec<crate::sim::components::BaleDepositEvent>>,
    /// Test observation of each tank-bunker wall rise or fall. The bunker link
    /// sets the wall anim slots itself; the frame tail drops these.
    #[cfg(any(test, feature = "test-support"))]
    #[serde(skip)]
    pub(crate) bunker_wall_events: Forked<Vec<crate::sim::components::BunkerWallAnimEvent>>,
    /// Resolved TeamClass/ScriptType runtime; scenario INI parsing remains a
    /// separate refused boundary until its record grammar is evidenced.
    pub(crate) team_script_vm: Forked<TeamScriptVm>,
    /// The HouseClass array: each House's state and registration order.
    pub(crate) houses: Forked<crate::sim::houses::Houses>,
    /// Derived movement inputs reused across object turns (blocker plane).
    /// Rebuilt from state on demand; never serialized or hashed.
    #[serde(skip)]
    pub(crate) movement_pass_cache: Forked<crate::sim::movement::movement_tick::MovementPassCache>,
    /// Each owner's reveal viewers, remembered through the object and fog
    /// passes (`vision::RevealViewers`); never serialized or hashed.
    #[serde(skip)]
    pub(crate) reveal_viewers: Forked<crate::sim::vision::RevealViewers>,
    /// The Civilian-side House, remembered through the object pass
    /// (`house_defeat::CivilianSideHouse`); never serialized or hashed.
    #[serde(skip)]
    civilian_house_memo: Forked<house_defeat::CivilianSideHouse>,
    /// Which listeners each pointer expiry reaches (`expiry_listeners`).
    /// Derived from the listener stores; never serialized or hashed.
    #[serde(skip)]
    expiry_listeners: Forked<expiry_listeners::ExpiryListeners>,
    /// Batched path searches (`movement::path_batch`); `None`
    /// searches in native order. Its registrations wait across a save.
    pub(crate) path_batch: Forked<Option<crate::sim::movement::path_batch::PathBatch>>,
    /// Idle objects sleep (`idle_sleep`). A runtime setting: never saved or
    /// hashed, and no result depends on it.
    #[serde(skip)]
    idle_sleep: Forked<idle_sleep::IdleSleep>,
    /// Batched target scans (`techno_ai::scan_batch`); `None` scans in
    /// native order. A save carries only the setting.
    pub(crate) scan_batch: Forked<Option<techno_ai::ScanBatch>>,
    /// Whether an effect anim that changes nothing but its picture runs as a
    /// presentation anim (`anim_class::EffectAnims`); a new world makes every
    /// anim in native order. A save carries the setting.
    pub(crate) effect_anims: Forked<crate::sim::anim_class::EffectAnims>,
    /// Whether each Logic slot's turn draws from its own stream
    /// (`turn_streams`); a new world draws every turn from the Scenario
    /// stream. A runtime setting: never saved, and a load turns it on again.
    #[serde(skip)]
    turn_streams: Forked<bool>,
    /// Retained native navigation base plane/raw movement rows plus the live
    /// derived hierarchy. ZoneGrid saves only its own retained base state;
    /// LoadContent67E8CD rebuilds hierarchy after bound Cell/object restoration.
    pub(crate) zone_grid: Forked<Option<ZoneGrid>>,
    /// Canonical dynamic navigation projection. Every reader reads it at its
    /// point of use; a republish replaces the `Arc` without mutating readers'
    /// shared copies.
    #[serde(skip)]
    pub(crate) path_grid: Forked<Option<Arc<PathGrid>>>,
    /// Derived cache: the marked-structure movement cells last published into
    /// `path_grid`. Source of truth is the marked structures themselves
    /// (`navigation::structure_movement_cells`). A full navigation rebuild
    /// resets it; synchronous Mark/Recalc publication updates affected cells.
    /// Frame-end structure publication reconciles the union of this set and
    /// the current one, skipping cells already current.
    #[serde(skip)]
    structure_navigation_cells: Forked<BTreeSet<(u16, u16)>>,
    #[serde(skip)]
    pub(crate) resolved_terrain: Forked<Option<ResolvedTerrainGrid>>,
    /// Process-global MapClass fallback CellClass identity. Native owns this at
    /// `0x00ABDC50`, outside Scenario serialization; live in-scenario loads
    /// retain the current handle and rebuilt terrain is rebound to it.
    #[serde(skip, default)]
    pub(crate) shared_cell_dummy: Forked<SharedCellDummy>,
    /// Shared Map+11C/+120 traversal for synchronous RevealZone578850/578710.
    /// Nested discovery actions overwrite this same cursor; never restore an
    /// outer cursor. Every exposed traversal resets before consuming it, and
    /// no action stack survives a save, so this private scratch is not saved
    /// or hashed. Terrain/query/input clones do not share its mutable state.
    #[serde(skip)]
    native_local_cell_cursor: Forked<Option<crate::map::resolved_terrain::NativeLocalCellCursor>>,
    #[serde(skip)]
    native_allocated_cell_cursor:
        Forked<Option<crate::map::resolved_terrain::NativeAllocatedCellCursor>>,
    /// Serialized value authority for allocated real CellClass bridge bits.
    /// The derived terrain grid is skipped; load installs its pristine map
    /// template and then writes these saved values directly back onto real
    /// cells. This is value state, never a setter/replay log.
    #[serde(default)]
    pub(crate) real_cell_bridge_flags_0x1180: Forked<RealCellBridgeFlags0x1180>,
    /// Exact value projection of runtime isometric-tile replacement. The
    /// parsed terrain grid is derived/skipped and receives these values before
    /// overlay/bridge/navigation reconstruction on restore.
    #[serde(default)]
    pub(crate) dynamic_terrain_cells: Forked<crate::map::resolved_terrain::DynamicTerrainCells>,
    /// Firestorm 2.0 MapClass+0x14C, the cracked ice waiting to freeze over
    /// (`ice`, docs/ts-ice.md). Empty unless the map runs ice.
    #[serde(default)]
    ice_cracks: Forked<Vec<ice::CrackedIce>>,
    /// The second step of the last terrain dent, Firestorm's one pending
    /// slot (`terrain_deformation`). Only terrain deformation fills it.
    terrain_dent: Forked<Option<terrain_deformation::PendingTerrainDent>>,
    pub(crate) bridge_state: Forked<Option<BridgeRuntimeState>>,
    /// Resident Map+D4 ordered collapse coordinates. Independent of optional
    /// geometry so even standalone fallout/save retains its native publication.
    /// The bridge damage controller owns its mutations and synchronous drain.
    pending_bridge_collapses: Forked<bridge_orchestrator::PendingBridgeCells>,
    /// Per-cell mutable overlay state (ore density, wall damage, bridge frames).
    /// Seeded from map [OverlayPack] at init, mutated during gameplay.
    pub(crate) overlay_grid: Forked<Option<crate::sim::overlay_grid::OverlayGrid>>,
    /// Persistent MapClass scenario-crate slots, including accepted ghosts and
    /// their native timer words. This is authoritative save/hash state.
    crate_authority: Forked<crate::sim::crates::CrateAuthority>,
    /// Per-cell smudge state (craters, scorches). Seeded from map [Smudge]
    /// entries at init, mutated by combat death-handling at runtime.
    pub(crate) smudge_grid: Forked<Option<crate::sim::smudge_grid::SmudgeGrid>>,
    /// Per-cell radiation field + site registry. Detonations of RadLevel>0
    /// weapons feed it as their damage commits; sites decay before the object
    /// pass (`RadiationState::tick_decay`); foot units take periodic damage
    /// from their cell.
    #[serde(default)]
    pub(crate) radiation: Forked<crate::sim::radiation::RadiationState>,
    /// `BombListClass`: the carriers of live Crazy Ivan bombs (each record
    /// sits on its carrier) and the BombVisible countdown, owned by `bomb`.
    /// Not saved: a load clears it and rebuilds the carriers.
    #[serde(skip)]
    pub(crate) bombs: Forked<crate::sim::bomb::BombList>,
    /// The kamikaze tracker (`0x00ABC5F8`): missiles out of their launcher's
    /// control and the cells they fly at, owned by `kamikaze`. Its timer is
    /// not saved; a load restarts it.
    pub(crate) kamikaze: Forked<crate::sim::kamikaze::KamikazeTracker>,
    /// DiskLaser Save4A7C10 retains delayed shots; emitted LaserDraws remain
    /// copied presentation facts. This owner has no Logic/Display membership.
    disk_lasers: Forked<disk_laser::DiskLasers>,
    /// The map's isometric playfield diamond ([Map] Size width + the raw
    /// LocalSize rect), set at map init. Threaded into the cell-rect occupancy
    /// validator's final playfield-corner test (the engine diamond, not a
    /// rectangle). `None` only in headless tests with no map loaded.
    #[serde(default)]
    playfield_bounds: Forked<Option<crate::sim::cell_rect::PlayfieldBounds>>,
    /// Immutable signed `[Map] Size=` height retained for later LocalSize
    /// normalization. The live predicate needs only Size width, but action 40
    /// can rewrite the four LocalSize dwords repeatedly during a scenario.
    #[serde(default)]
    playfield_size_height: Forked<Option<i32>>,
    /// Monotonic visible-area writer generation. Every successful trigger
    /// action 0x28 advances it, even when two writers normalize to the same
    /// bounds. Presentation consumes this as the global radar/scroll rebuild
    /// edge; cell-local bridge dirtiness is deliberately a separate channel.
    #[serde(default)]
    playfield_revision: Forked<u64>,
    /// Interned projection of RuleSet's ordered BridgeExplosions references.
    /// Rebuilt at map binding and snapshot restoration; not serialized authority.
    #[serde(skip)]
    bridge_explosions: Forked<Vec<InternedId>>,
    /// Interned projection of RuleSet's ordered MetallicDebris references,
    /// including unread types without an SHP. Rebuilt at map binding and
    /// snapshot restoration so per-cell selection needs no string interning.
    #[serde(skip)]
    metallic_debris: Forked<Vec<InternedId>>,
    /// Radar and tactical redraw marks for runtime cell changes.
    #[serde(skip)]
    pub(crate) dirty_cells: Forked<crate::sim::dirty_cells::DirtyCells>,
    /// Per-player power state (output, drain, low-power flag, spy blackout timer),
    /// assessed by each House's own update.
    pub(crate) power_states: Forked<crate::sim::power_system::PowerStates>,
    /// Per-owner superweapon instances. Outer key = owner, inner key = SW type ID.
    /// Deterministic iteration via nested BTreeMap.
    pub(crate) super_weapons: Forked<crate::sim::superweapon::SuperWeapons>,
    /// The Lightning Storm's globals (one at a time).
    pub(crate) lightning_storm: Forked<crate::sim::superweapon::lightning_storm::LightningStorm>,
    /// The Psychic Dominator's globals (one at a time).
    pub(crate) psychic_dominator:
        Forked<crate::sim::superweapon::psychic_dominator::PsychicDominatorState>,
    /// Pending UnitDelivery effects; simulation-owned, saved and hashed.
    #[serde(default)]
    pub(crate) unit_deliveries: Forked<crate::sim::superweapon::unit_delivery::UnitDeliveryState>,
    /// Ares ChronoWarpStateMachines moving chronoshifted buildings; owned by
    /// `superweapon::chronosphere`, saved and hashed.
    #[serde(default)]
    pub(crate) ares_chrono_warps: Forked<crate::sim::superweapon::AresChronoWarps>,
    /// The Houses whose Ares firestorm is up (HouseClass+0x1FA); owned by
    /// `sim::ares_firestorm`, saved and hashed.
    #[serde(default)]
    pub(crate) ares_firestorm: Forked<crate::sim::ares_firestorm::FirestormHouses>,
    /// Every House's Ares tunnel networks (HouseExt+0x64); owned by
    /// `sim::ares_tunnel`, saved and hashed.
    #[serde(default)]
    pub(crate) ares_tunnels: Forked<crate::sim::ares_tunnel::AresTunnels>,
    /// IonBlastClass's vector (`0x00AA0118`), owned by `ion_blast`. Native
    /// saves no blast; Rust retains pending damaging effects for match restore.
    #[serde(
        default,
        serialize_with = "ion_blast::save",
        deserialize_with = "ion_blast::load"
    )]
    ion_blasts: Forked<Vec<ion_blast::IonBlast>>,
    /// AlphaShapeClass's vector (`0x0088A0F0`), owned by `alpha_shape`.
    /// Saved, never hashed.
    #[serde(default)]
    alpha_shapes: Forked<alpha_shape::AlphaShapes>,
    /// Per-cell terrain speed modifier config (slope climb/descend).
    /// Built from [General] rules at map load.
    #[serde(skip)]
    terrain_speed_config: Forked<terrain_speed::TerrainSpeedConfig>,
    /// Negotiated lockstep ahead window. Offline producers stamp the current
    /// raw issue ordinal; a network transfer owner overwrites that stamp with
    /// `send_current + MaxAhead` before synchronized dispatch.
    input_delay_ticks: Forked<u64>,
    /// Native Main_Tick termination gates outside House state. These are
    /// front-end connection/session facts, so they are transient and un-hashed.
    #[serde(skip)]
    quit_requested: Forked<bool>,
    /// One-shot owner-tagged edge raised only when a due native EXIT command
    /// executes at the EventClass tail. App teardown consumes and clears it.
    #[serde(skip)]
    executed_exit_owner: Forked<Option<InternedId>>,
    /// Remote senders of executed EXIT events this frame, in execution
    /// order: the app runs DestroyConnection(house, 0) for each
    /// (`0x0064C9B1`) and drains them. A sender is the record's header byte.
    #[serde(skip)]
    departed_senders: Forked<Vec<i8>>,
    #[serde(skip)]
    connection_lost: Forked<bool>,
    /// Checksum computed by this frame's multiplayer tail, handed to the
    /// frame output. Transient per frame.
    #[serde(skip)]
    frame_sync_checksum: Forked<Option<FrameSyncChecksum>>,
    /// VERA-only: which frames hash their state for the TickResult, by tick
    /// number; `None` for none. A launch or a loaded save hashes every frame
    /// unless the caller asks for fewer; normal play's frame loop asks for
    /// every [`crate::sim::replay::PLAY_HASH_INTERVAL`]th. Test and dev worlds
    /// ([`Simulation::with_seed`]) hash only when asked. A frame that ends the
    /// match hashes regardless. Never saved or hashed.
    #[serde(skip, default = "every_frame")]
    state_hash_interval: Forked<Option<std::num::NonZeroU64>>,
    /// One admitted command FIFO. Native frame/flags remain in the record;
    /// remaining semantic commands retain their monotonic scheduler.
    pending_commands: Forked<
        crate::sim::command_queue::DoList<
            queued_command::QueuedCommand,
            { crate::sim::command_queue::NETWORK_COMMAND_CAPACITY },
        >,
    >,
    /// Locally issued events retain their shared OutList occupancy until
    /// the post-Logic transfer. Already-admitted/deferred work stays above.
    local_commands: Forked<
        crate::sim::command_queue::LocalCommandStage<
            queued_command::QueuedCommand,
            { crate::sim::command_queue::NETWORK_SOURCE_COMMAND_CAPACITY },
        >,
    >,
    /// While the dispatcher temporarily owns the FIFO, this derived count
    /// reserves its occupied native slots for callbacks that admit new work.
    /// Balanced at the dispatch boundary, never saved or hashed.
    #[serde(skip)]
    dispatch_reserved_slots: Forked<crate::sim::command_queue::NativeCommandOccupancy>,
    /// Non-authoritative copies for this frame's replay output. Admissions
    /// append once; collection drains them. Saved queue entries remain the
    /// only gameplay authority and a restored snapshot starts a new receipt.
    #[serde(skip)]
    frame_command_admissions: Forked<Vec<CommandEnvelope>>,
    /// The one suspended frame cursor: a House Vox wait or post-Logic
    /// network service. Restore and resume its suffix without replaying work.
    #[serde(default)]
    frame_continuation: Forked<Option<FrameContinuation>>,
    /// PlanMgr is the sole owner of shared nodes and per-actor plan tokens.
    planning: Forked<planning::PlanningState>,
    /// Diagnostic playback supplies a batch already containing generated
    /// events. Transient admission context, not gameplay or saved state.
    #[serde(skip)]
    replaying_recorded_commands: Forked<bool>,
    /// Serialized map trigger state, initialized and mutated by trigger_runtime.
    /// Remains installed while actions call other Simulation mechanisms.
    pub(crate) trigger_runtime: Forked<TriggerRuntime>,
    /// Loaded Wasm extension modules (`sim::ext_modules`), saved with their
    /// memory and globals and instantiated again by `restore_ext_modules`.
    /// Not a [`Forked`] field: a live module instance cannot be copied, so a
    /// turn run on a fork of the world must not call a module.
    pub(crate) ext_modules: crate::sim::ext_modules::ExtModules,
}

#[cfg(any(test, feature = "test-support"))]
impl Default for Simulation {
    fn default() -> Self {
        Self::new()
    }
}

fn dispatch_tiberium_reduction_inline(
    request: &crate::sim::combat::TiberiumReductionRequest,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    scenario_rng: &mut SimRng,
    overlay_grid: Option<&mut crate::sim::overlay_grid::OverlayGrid>,
    terrain: Option<&mut ResolvedTerrainGrid>,
    ore_growth_state: &mut crate::sim::ore_growth::OreGrowthState,
    source_object_cells: &BTreeSet<(u16, u16)>,
    live_objects: Option<crate::sim::tiberium::NativeCellObjectView<'_>>,
    binary_frame: u32,
    spread_enabled: bool,
    dirty_cells: &mut crate::sim::dirty_cells::DirtyCells,
) {
    let mut context = crate::sim::tiberium::ReduceTiberiumContext {
        overlay_grid,
        ore_growth_state,
        overlay_registry,
        tiberium_types: Some(&rules.tiberium_types),
        resolved_terrain: terrain,
        source_object_cells: Some(source_object_cells),
        live_objects,
        rng: Some(scenario_rng),
        binary_frame,
        spread_enabled,
        dirty_cells: Some(dirty_cells),
    };
    let _ = crate::sim::tiberium::reduce_tiberium(
        &mut context,
        (request.rx, request.ry),
        request.amount,
    );
}

/// World-owned half of Apply_area_damage's per-cell transaction for non-combat
/// producers. Overlay/terrain/RNG are lent by AoELayerContext at each spread
/// entry; this value owns the disjoint Simulation fields needed by the optional
/// tiberium prelude and synchronous wall-navigation callbacks. It holds the
/// dirty cells and the navigation caches as the world's fields
/// (`util::forked`), so only a reduction or a wall step takes them for writing.
pub(crate) struct SimulationAreaDamageCellPrelude<'a> {
    rules: &'a RuleSet,
    tiberium_amount: Option<i32>,
    ore_growth_state: &'a mut crate::sim::ore_growth::OreGrowthState,
    source_object_cells: &'a BTreeSet<(u16, u16)>,
    /// Live terrain-object cell index: the terrain half of the native
    /// `FirstObject` list for the reduction reseed.
    terrain_object_cells: &'a BTreeMap<(u16, u16), u64>,
    binary_frame: u32,
    spread_enabled: bool,
    dirty_cells: &'a mut Forked<crate::sim::dirty_cells::DirtyCells>,
    zone_grid: &'a mut Forked<Option<ZoneGrid>>,
    path_grid: &'a mut Forked<Option<Arc<PathGrid>>>,
    bridge_state: Option<&'a BridgeRuntimeState>,
    playfield_bounds: Option<PlayfieldBounds>,
}

impl crate::sim::combat::combat_aoe::AoECellPrelude for SimulationAreaDamageCellPrelude<'_> {
    fn before_cell(
        &mut self,
        rx: u16,
        ry: u16,
        overlay_grid: Option<&mut crate::sim::overlay_grid::OverlayGrid>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        terrain: Option<&mut ResolvedTerrainGrid>,
        scenario_rng: Option<&mut SimRng>,
        occupancy: Option<&OccupancyGrid>,
    ) {
        let Some(amount) = self.tiberium_amount else {
            return;
        };
        if !crate::sim::combat::combat_aoe::tiberium_reduction_cell_admitted(
            overlay_grid.as_deref(),
            overlay_registry,
            rx,
            ry,
        ) {
            return;
        }
        let scenario_rng = scenario_rng
            .expect("production Apply_area_damage tiberium prelude requires scenario RNG");
        dispatch_tiberium_reduction_inline(
            &crate::sim::combat::TiberiumReductionRequest { rx, ry, amount },
            self.rules,
            overlay_registry,
            scenario_rng,
            overlay_grid,
            terrain,
            self.ore_growth_state,
            self.source_object_cells,
            occupancy.map(|occupancy| {
                crate::sim::tiberium::NativeCellObjectView::new(
                    occupancy,
                    self.terrain_object_cells,
                )
            }),
            self.binary_frame,
            self.spread_enabled,
            self.dirty_cells,
        );
    }

    fn wall_dirty_step(&mut self, step: WallDirtyStep, packed_coord: (u16, u16)) {
        match step {
            WallDirtyStep::Tactical => self.dirty_cells.mark_tactical(packed_coord),
            WallDirtyStep::Radar => self.dirty_cells.mark_radar([packed_coord]),
        }
    }

    fn wall_navigation_step(
        &mut self,
        terrain: &ResolvedTerrainGrid,
        overlays: &OverlayGrid,
        cell: (u16, u16),
        navigation_changed: bool,
        repair: ZoneRepairKind,
    ) {
        repair_wall_damage_navigation_authorities(
            self.zone_grid,
            self.path_grid,
            terrain,
            overlays,
            self.bridge_state,
            self.playfield_bounds,
            cell,
            navigation_changed,
            repair,
        );
    }
}

pub(crate) fn simulation_area_damage_cell_prelude<'a>(
    rules: &'a RuleSet,
    warhead: &crate::rules::warhead_type::WarheadType,
    base_damage: i32,
    affect_resource: bool,
    scenario_no_damage: bool,
    ore_growth_state: &'a mut crate::sim::ore_growth::OreGrowthState,
    source_object_cells: &'a BTreeSet<(u16, u16)>,
    terrain_object_cells: &'a BTreeMap<(u16, u16), u64>,
    binary_frame: u32,
    spread_enabled: bool,
    dirty_cells: &'a mut Forked<crate::sim::dirty_cells::DirtyCells>,
    zone_grid: &'a mut Forked<Option<ZoneGrid>>,
    path_grid: &'a mut Forked<Option<Arc<PathGrid>>>,
    bridge_state: Option<&'a BridgeRuntimeState>,
    playfield_bounds: Option<PlayfieldBounds>,
) -> SimulationAreaDamageCellPrelude<'a> {
    let amount = base_damage / 10;
    let tiberium_amount =
        (!scenario_no_damage && affect_resource && warhead.tiberium && amount > 0)
            .then_some(amount);
    SimulationAreaDamageCellPrelude {
        rules,
        tiberium_amount,
        ore_growth_state,
        source_object_cells,
        terrain_object_cells,
        binary_frame,
        spread_enabled,
        dirty_cells,
        zone_grid,
        path_grid,
        bridge_state,
        playfield_bounds,
    }
}

pub(crate) fn repair_wall_damage_navigation_authorities(
    zone_grid: &mut Option<ZoneGrid>,
    path_grid: &mut Option<Arc<PathGrid>>,
    terrain: &ResolvedTerrainGrid,
    overlays: &OverlayGrid,
    bridge_state: Option<&BridgeRuntimeState>,
    playfield_bounds: Option<PlayfieldBounds>,
    cell: (u16, u16),
    navigation_changed: bool,
    repair: ZoneRepairKind,
) {
    // Only the wall cell changed: publish it into the installed views. A
    // missing or mis-sized view (loading) is built whole from terrain.
    let installed = path_grid
        .as_mut()
        .filter(|grid| grid.width() == terrain.width() && grid.height() == terrain.height());
    match installed {
        Some(grid) if navigation_changed => {
            let refreshed = terrain.cell(cell.0, cell.1).is_some_and(|resolved| {
                Arc::make_mut(grid).refresh_resolved_cell(resolved, Some(overlays), false)
            });
            debug_assert!(refreshed, "wall Recalc cell must be inside the map");
        }
        Some(_) => {}
        None => {
            *path_grid = Some(Arc::new(PathGrid::from_resolved_terrain_with_bridges(
                terrain,
                Some(overlays),
            )));
        }
    }
    let tail_path_grid = path_grid
        .as_deref()
        .expect("wall repair installed a path grid");
    repair_zone_after_recalc(
        zone_grid,
        tail_path_grid,
        terrain,
        bridge_state,
        playfield_bounds,
        cell,
        repair,
    );
}

/// One native cell zone helper after the cell's Recalc was published:
/// AssignOrphanedCellZone (`0x0056D460`) or MergeAdjacentCellZone
/// (`0x0056D5A0`), then IncrementalRebuildZoneGraphAroundCell (`0x00584550`).
/// A missing zone grid, or one whose bridge inputs changed, is built whole.
pub(crate) fn repair_zone_after_recalc(
    zone_grid: &mut Option<ZoneGrid>,
    path_grid: &PathGrid,
    terrain: &ResolvedTerrainGrid,
    bridge_state: Option<&BridgeRuntimeState>,
    playfield_bounds: Option<PlayfieldBounds>,
    cell: (u16, u16),
    repair: ZoneRepairKind,
) {
    let bridge_records = bridge_state
        .map(BridgeRuntimeState::endpoint_records)
        .unwrap_or(&[]);
    let bridge_geometry = bridge_state.and_then(BridgeRuntimeState::native_zone_source_size);
    if zone_grid
        .as_ref()
        .is_some_and(|zones| !zones.bridge_inputs_match(bridge_records, bridge_geometry))
    {
        *zone_grid = None;
    }
    if let Some(zone_grid) = zone_grid.as_mut() {
        let _ = zone_grid.refresh_base_cell_attributes_at(terrain, cell.0, cell.1);
        let _ = repair_zone_cell(
            zone_grid,
            PackedZoneCoord::new(cell.0 as i16, cell.1 as i16),
            repair,
            path_grid,
            playfield_bounds,
            terrain,
            bridge_records,
        );
    } else {
        *zone_grid = Some(
            ZoneGrid::build_with_native_map_context(
                path_grid,
                terrain,
                bridge_records,
                bridge_geometry,
                playfield_bounds,
            )
            .into(),
        );
    }
}

/// Wall map callbacks over the installed presentation and navigation owners.
/// Native Cell expiry7258D0 has no Techno listener broadcast.
pub(crate) struct SimulationWallRuntimeHost<'a> {
    pub(crate) dirty_cells: &'a mut crate::sim::dirty_cells::DirtyCells,
    pub(crate) zone_grid: &'a mut Option<ZoneGrid>,
    pub(crate) path_grid: &'a mut Option<Arc<PathGrid>>,
    pub(crate) bridge_state: Option<&'a BridgeRuntimeState>,
    pub(crate) playfield_bounds: Option<PlayfieldBounds>,
}

impl WallDamageTransactionHost for SimulationWallRuntimeHost<'_> {
    fn dirty_step(&mut self, step: WallDirtyStep, packed_coord: (u16, u16)) {
        match step {
            WallDirtyStep::Tactical => self.dirty_cells.mark_tactical(packed_coord),
            WallDirtyStep::Radar => self.dirty_cells.mark_radar([packed_coord]),
        }
    }

    fn navigation_step(
        &mut self,
        terrain: &ResolvedTerrainGrid,
        overlays: &OverlayGrid,
        cell: (u16, u16),
        navigation_changed: bool,
        repair: ZoneRepairKind,
    ) {
        repair_wall_damage_navigation_authorities(
            self.zone_grid,
            self.path_grid,
            terrain,
            overlays,
            self.bridge_state,
            self.playfield_bounds,
            cell,
            navigation_changed,
            repair,
        );
    }

    // Ordinary Cell7258D0 has no Techno broadcast; real70D4A0 callers
    // (AreaDamage, wall sale, cliff collapse) own their explicit outer sweep.
    fn pointer_expired(&mut self, _target: WallPointerTarget) {}
}

fn dispatch_smudge_inline(
    request: &crate::sim::combat::SmudgeSpawnRequest,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    occupancy: &OccupancyGrid,
    raw_occupation: &crate::sim::occupancy::RawCellOccupationGrid,
    scenario_rng: &mut SimRng,
    overlay_grid: Option<&mut crate::sim::overlay_grid::OverlayGrid>,
    terrain: Option<&mut ResolvedTerrainGrid>,
    smudge_grid: Option<&mut crate::sim::smudge_grid::SmudgeGrid>,
    ore_growth_state: &mut crate::sim::ore_growth::OreGrowthState,
    source_object_cells: &BTreeSet<(u16, u16)>,
    terrain_object_cells: &BTreeMap<(u16, u16), u64>,
    binary_frame: u32,
    spread_enabled: bool,
    dirty_cells: &mut crate::sim::dirty_cells::DirtyCells,
    construction: &mut crate::sim::smudge_grid::SmudgeConstruction<'_>,
) -> Result<(), crate::rules::overlay_types::TiberiumImageError> {
    let (Some(smudge_grid), Some(overlay_grid), Some(terrain)) =
        (smudge_grid, overlay_grid, terrain)
    else {
        return Ok(());
    };
    let mut tiberium = crate::sim::combat::smudge_dispatch::SmudgeTiberiumContext {
        overlay_grid,
        ore_growth_state,
        overlay_registry,
        tiberium_types: Some(&rules.tiberium_types),
        source_object_cells: Some(source_object_cells),
        live_objects: Some(crate::sim::tiberium::NativeCellObjectView::new(
            occupancy,
            terrain_object_cells,
        )),
        binary_frame,
        spread_enabled,
        dirty_cells,
    };
    crate::sim::combat::smudge_dispatch::dispatch_smudge_request(
        request,
        rules.art(),
        &rules.smudge_types,
        smudge_grid,
        occupancy,
        terrain,
        raw_occupation,
        &mut tiberium,
        scenario_rng,
        Some(construction),
    )
}

/// Existing process RNG owner lent to a detached saved-world candidate.
pub enum SnapshotProcessState<'a> {
    Live(&'a Simulation),
    Cold(&'a MainRng),
}

impl ObjectSubstrate {
    /// Legacy non-lifecycle contact scrub retained only for separately classified
    /// failure paths. Reveal/Conceal/UnInit must use synchronous radio authority.
    pub(crate) fn clear_radio_contacts_for(&mut self, stable_id: u64) {
        self.entities.clear_radio_contacts_for(stable_id);
    }
}

#[cfg(all(test, any(tests_sim_world_t_z, tests_sim_world_p_s, tests_sim_t_z)))]
#[path = "world_tests.rs"]
pub(crate) mod tests;

#[cfg(all(test, tests_sim_world_p_s))]
#[path = "smudge_integration_tests.rs"]
mod smudge_integration_tests;

#[cfg(all(test, tests_sim_world_c_o))]
#[path = "harvest_attack_return_oracle_tests.rs"]
mod harvest_attack_return_oracle_tests;
#[cfg(all(
    test,
    any(
        tests_sim_world_c_o,
        tests_sim_world_a_b,
        tests_sim_world_p_s,
        tests_sim_world_t_z
    )
))]
#[path = "harvest_field_oracle_tests.rs"]
mod harvest_field_oracle_tests;
#[cfg(all(test, tests_sim_world_p_s))]
#[path = "refinery_dock_continuation_tests.rs"]
mod refinery_dock_continuation_tests;
#[cfg(all(
    test,
    any(
        tests_sim_world_p_s,
        tests_sim_world_a_b,
        tests_sim_world_c_o,
        tests_sim_world_t_z
    )
))]
#[path = "refinery_dock_oracle_tests.rs"]
mod refinery_dock_oracle_tests;
#[cfg(all(test, tests_sim_world_p_s))]
#[path = "refinery_dock_retail_tests.rs"]
mod refinery_dock_retail_tests;

#[cfg(all(test, tests_sim_world_c_o))]
#[path = "harvest_field_cycle_tests.rs"]
mod harvest_field_cycle_tests;

#[cfg(all(test, any(tests_sim_world_p_s, tests_sim_world_a_b)))]
#[path = "slave_manager_oracle_tests.rs"]
mod slave_manager_oracle_tests;

#[cfg(all(test, tests_sim_world_p_s))]
#[path = "slave_manager_cycle_tests.rs"]
mod slave_manager_cycle_tests;

#[cfg(all(test, tests_sim_world_p_s))]
mod slave_manager_reentrancy_tests;

#[cfg(all(test, tests_sim_world_a_b))]
#[path = "building_sale_oracle_tests.rs"]
mod building_sale_oracle_tests;

#[cfg(all(test, tests_sim_world_a_b))]
mod ai_captured_building_sale_tests;

#[cfg(all(test, tests_sim_world_a_b))]
#[path = "building_repair_oracle_tests.rs"]
mod building_repair_oracle_tests;

#[cfg(all(test, tests_sim_world_p_s))]
#[path = "refinery_dock_cycle_tests.rs"]
mod refinery_dock_cycle_tests;

#[cfg(all(
    test,
    any(
        tests_sim_world_c_o,
        tests_sim_world_a_b,
        tests_sim_world_p_s,
        tests_sim_world_t_z
    )
))]
#[path = "cmin_dock_oracle_tests.rs"]
mod cmin_dock_oracle_tests;

#[cfg(all(test, tests_sim_world_c_o))]
#[path = "cmin_dock_cycle_tests.rs"]
mod cmin_dock_cycle_tests;

#[cfg(all(test, tests_sim_world_p_s))]
#[path = "passenger_escape_oracle_tests.rs"]
mod passenger_escape_oracle_tests;

#[cfg(all(test, tests_sim_world_t_z))]
#[path = "vehicle_passenger_escape_oracle_tests.rs"]
mod vehicle_passenger_escape_oracle_tests;

#[cfg(all(test, tests_sim_world_a_b))]
mod bunker_entry_oracle_tests;

#[cfg(all(test, tests_sim_world_t_z))]
#[path = "world_orders_bridge_repair_tests.rs"]
mod world_orders_bridge_repair_tests;

#[cfg(all(test, tests_sim_world_p_s))]
#[path = "rng_routing_tests.rs"]
mod rng_routing_tests;

#[cfg(all(test, tests_sim_world_p_s))]
mod raw_record_convergence_tests;

#[cfg(all(test, tests_sim_world_p_s))]
#[path = "slice6_retask_tests.rs"]
mod slice6_retask_tests;

#[cfg(all(
    test,
    any(tests_sim_world_c_o, tests_sim_world_a_b, tests_sim_world_p_s)
))]
#[path = "global_parity_harness_tests.rs"]
mod global_parity_harness_tests;

#[cfg(all(test, tests_sim_world_p_s))]
#[path = "production_shadow_tests.rs"]
mod production_shadow_tests;

#[cfg(any(test, feature = "test-support"))]
pub mod factory_infantry_output_tests;
#[cfg(all(test, tests_sim_world_c_o))]
mod factory_jumpjet_output_tests;

#[cfg(all(test, tests_sim_world_p_s))]
#[path = "radar_dirty_ack_tests.rs"]
mod radar_dirty_ack_tests;

#[cfg(all(test, tests_sim_world_a_b))]
#[path = "bridge_parity_harness_tests.rs"]
mod bridge_parity_harness_tests;

/// Draw flags the drive locomotor passes to the wake `AnimClass` constructor
/// (0x004B0823 region: `PUSH 0x600`).
const WAKE_DRAW_FLAGS: u32 = 0x600;

/// The wake gate for one unit this frame. Only three locomotors read
/// `Rules->Wake` while moving, each in its own process: Drive (`0x004B078F`)
/// and Hover (`0x00514A24`) every 10th frame, Ship (`0x0069FE3C`) every 8th
/// frame and not for an `Underwater=` type (`underwater`, TechnoType+0xD69 at
/// `0x0069FE6E`). Each asks its own `Is_Moving_Now` first, then wants the
/// owner off a bridge (`+0x8C`) on a cell whose `CellClass+0xEC` mirror is
/// Water. The wake is anchored at the unit's Location, a Hover's altitude
/// included. A Jumpjet or Fly flier over water leaves none.
pub(crate) fn wake_anchor_for(
    entity: &crate::sim::game_entity::GameEntity,
    rules: crate::sim::movement::SpeedRules<'_>,
    underwater: bool,
    terrain: Option<&ResolvedTerrainGrid>,
    binary_frame: u32,
) -> Option<crate::sim::anim_class::AnimWorldCoord> {
    // Foot4DA81A bypasses the locomotor (and its movement-wake producer)
    // while sinking. Unit AI owns that hull's separate jittered wake.
    if entity.sinking.is_active() {
        return None;
    }
    let on_cadence = match entity.locomotor.as_ref().map(|loco| loco.active_kind()) {
        Some(crate::rules::locomotor_type::LocomotorKind::Drive) => binary_frame.is_multiple_of(10),
        // Hover Process 514705..514717 exits before its wake on entering
        // a tube. The Drive/Ship tails have no such check.
        Some(crate::rules::locomotor_type::LocomotorKind::Hover) => {
            binary_frame.is_multiple_of(10) && entity.active_tube().is_none()
        }
        Some(crate::rules::locomotor_type::LocomotorKind::Ship) => {
            binary_frame.is_multiple_of(8) && !underwater
        }
        _ => false,
    };
    if !on_cadence
        || !crate::sim::movement::motion_query::is_moving_now(entity, rules, binary_frame)
    {
        return None;
    }
    if entity.on_bridge() {
        return None;
    }
    let cell = terrain?.cell(entity.position.rx(), entity.position.ry())?;
    if cell.land_type != crate::rules::terrain_rules::LandType::Water.as_index() {
        return None;
    }
    Some(crate::sim::movement::ground_pose::object_location(entity, terrain).into())
}

#[cfg(all(test, tests_sim_world_t_z))]
#[path = "tibtre_oracle_tests.rs"]
mod tibtre_oracle_tests;

#[cfg(all(test, tests_sim_world_c_o))]
#[path = "ore_spread_oracle_tests.rs"]
mod ore_spread_oracle_tests;

#[cfg(all(test, tests_sim_world_c_o))]
mod drop_as_bomb_integration_tests;

mod airstrike_aircraft;

#[cfg(all(test, tests_sim_world_p_s))]
mod radiation_overlap_tests;

#[cfg(all(test, tests_sim_world_a_b))]
mod building_occupant_storage_tests;

/// The parts of the world its presentation copy takes only where they changed
/// ([`Simulation::presentation_copy`]).
#[derive(Default)]
struct SyncedWorld {
    terrain: Option<ResolvedTerrainGrid>,
    overlays: Option<crate::sim::overlay_grid::OverlayGrid>,
    smudges: Option<crate::sim::smudge_grid::SmudgeGrid>,
    terrain_objects: crate::sim::terrain_object::TerrainObjectStore,
    interner: crate::sim::intern::StringInterner,
    occupancy: OccupancyGrid,
    raw_cell_occupation: crate::sim::occupancy::RawCellOccupationGrid,
    hidden_occupation: crate::sim::occupancy::HiddenOccupationGrid,
    air_slots: crate::sim::occupancy::AirSlotGrid,
}

impl SyncedWorld {
    fn take(sim: &mut Simulation) -> Self {
        Self {
            terrain: sim.resolved_terrain.take(),
            overlays: sim.overlay_grid.take(),
            smudges: sim.smudge_grid.take(),
            terrain_objects: std::mem::take(&mut *sim.terrain_objects),
            interner: std::mem::take(&mut *sim.interner),
            occupancy: std::mem::take(&mut *sim.substrate.occupancy),
            raw_cell_occupation: std::mem::take(&mut *sim.substrate.raw_cell_occupation),
            hidden_occupation: std::mem::take(&mut *sim.substrate.hidden_occupation),
            air_slots: std::mem::take(&mut *sim.substrate.air_slots),
        }
    }

    fn put(self, sim: &mut Simulation) {
        *sim.resolved_terrain = self.terrain;
        *sim.overlay_grid = self.overlays;
        *sim.smudge_grid = self.smudges;
        *sim.terrain_objects = self.terrain_objects;
        *sim.interner = self.interner;
        *sim.substrate.occupancy = self.occupancy;
        *sim.substrate.raw_cell_occupation = self.raw_cell_occupation;
        *sim.substrate.hidden_occupation = self.hidden_occupation;
        *sim.substrate.air_slots = self.air_slots;
    }

    /// Bring `mirror` up to these parts; an absent part leaves none.
    fn sync_into(&mut self, mirror: &mut Self) {
        fn sync_option<T: Clone>(
            own: &mut Option<T>,
            copy: &mut Option<T>,
            sync: impl FnOnce(&mut T, &mut T),
        ) {
            match own {
                // A first copy starts whole; the sync then marks it as the
                // one later syncs bring up to date.
                Some(own) => sync(own, copy.get_or_insert_with(|| own.clone())),
                None => *copy = None,
            }
        }
        // The parts share nothing, so they sync side by side.
        let Self {
            terrain,
            overlays,
            smudges,
            terrain_objects,
            interner,
            occupancy,
            raw_cell_occupation,
            hidden_occupation,
            air_slots,
        } = self;
        use crate::sim::parallel::alongside;
        alongside(
            || sync_option(terrain, &mut mirror.terrain, ResolvedTerrainGrid::sync_into),
            || {
                alongside(
                    || occupancy.sync_into(&mut mirror.occupancy),
                    || {
                        alongside(
                            || {
                                raw_cell_occupation.sync_into(&mut mirror.raw_cell_occupation);
                                hidden_occupation.sync_into(&mut mirror.hidden_occupation);
                                air_slots.sync_into(&mut mirror.air_slots);
                            },
                            || {
                                sync_option(overlays, &mut mirror.overlays, |own, copy| {
                                    own.sync_into(copy)
                                });
                                sync_option(smudges, &mut mirror.smudges, |own, copy| {
                                    own.sync_into(copy)
                                });
                                terrain_objects.sync_into(&mut mirror.terrain_objects);
                                interner.sync_into(&mut mirror.interner);
                            },
                        )
                    },
                )
            },
        );
    }
}
