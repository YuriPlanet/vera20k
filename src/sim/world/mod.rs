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
//! - `object_turn.rs` — complete live-object turn and pass outcomes
//! - `techno_ai.rs` — per-object AI dispatch within each turn

pub(crate) mod authored_load_host;
pub mod bridge_orchestrator;
#[cfg(any(test, feature = "test-support"))]
pub mod bridge_test_evidence;
mod building_after_destruction;
#[cfg(test)]
mod building_after_destruction_tests;
pub(crate) mod building_anim;
mod building_scatter;
mod cell_content;
mod crash;
mod disk_laser;
pub use disk_laser::DiskLaserObservation;
#[cfg(test)]
mod disk_laser_tests;
#[cfg(test)]
pub(crate) mod entry_test_fixture;
mod frame_effects;
mod object_entry;
mod sinking;
mod time_warp;
mod trigger_chrono;
mod type_tech_levels;
pub use frame_effects::{FrameEffects, LocalDefeatBoundary, SoundBoundary};
pub(crate) use type_tech_levels::TypeTechLevels;
#[cfg(test)]
mod unit_unlimbo_tests;
pub(crate) use sinking::{LivingSinkTrigger, SinkingState};
pub mod edge_cell;
mod gap_generator;
pub(crate) use gap_generator::GapGeneratorRuntime;
pub(crate) use gap_generator::LastOperational;
mod house_base;
#[cfg(test)]
mod house_cheer_tests;
mod house_defeat;
pub(crate) use house_defeat::HouseBlowupSubset;
mod ion_blast;
pub(crate) use house_base::HouseBaseState;
mod infantry_terminal;
mod jumpjet_cruise;
#[cfg(any(test, feature = "test-support"))]
pub use infantry_terminal::InfantryDeathSequence;
pub use infantry_terminal::{InfantryDeathPostlude, InfantryTerminal, TerminalLifetime};
mod aircraft_attack;
mod aircraft_enter;
mod aircraft_fire_location;
mod aircraft_move;
mod building_working_sound;
pub(crate) mod damage_consequences;
pub mod display_layers;
mod display_registry;
mod expiry_listeners;
mod fly_landing;
mod fly_orders;
mod fly_process;
mod frame_error;
mod ground_keys;
mod lifecycle;
mod load_object_lifecycle;
mod logic_vector;
mod move_cell_input;
pub use move_cell_input::{CellActionQuery, MoveAction};
mod move_sound;
mod object_sound;
mod turret_rotate_sound;
pub(crate) use move_sound::MoveSoundState;
pub(crate) use playfield_membership::PlayfieldMembership;
pub(crate) use selection_voice::DeployVoiceSuppression;
pub(crate) use turret_rotate_sound::TurretRotateLatch;
#[cfg(test)]
mod main_sound_identity_tests;
#[cfg(test)]
mod move_sound_tests;
#[cfg(any(test, feature = "test-support"))]
mod native_cell_input_test_fixture;
mod navigation;
mod object_turn;
mod playfield_membership;
mod rally_cell_input;
mod rocket_flight;
pub use frame_error::FrameAdvanceError;
pub use world_orders::BuildingClickAction;
pub(crate) use world_orders::CapturedInfantryType;
pub use world_orders::EngineerBuildingAction;
pub use world_orders::UnitRepairAction;
mod shroud_refresh;
mod track_cell_recalc;
#[cfg(any(test, feature = "test-support"))]
use object_turn::foot_body_counter_admitted;
mod projectile_collision;
mod substrate;
mod techno_ai;
mod trigger_camera;
#[cfg(test)]
mod trigger_camera_tests;
mod trigger_explosion;
mod trigger_nuke;
pub(crate) use techno_ai::ObjectAiCtx;
pub(crate) use techno_ai::PassiveScan;
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
pub use planning::{PlanningBadgeKind, PlanningDrawSegment};
mod selection_voice;
pub(crate) mod techno_ai_cloak;
pub(crate) mod unit_post;
#[cfg(test)]
mod wall_autofill_oracle_tests;
mod world_commands;
mod world_hash;
mod world_orders;
mod world_spawn;

#[cfg(test)]
mod aegis_distributed_fire_tests;
#[cfg(test)]
mod aircraft_deployment_tests;
#[cfg(test)]
mod area_rocker_tests;
#[cfg(test)]
mod brute_smash_debris_tests;
#[cfg(test)]
mod building_fatal_lifecycle_tests;
#[cfg(test)]
mod crash_tests;
#[cfg(test)]
mod damage_consequence_tests;
#[cfg(test)]
mod drone_rocking_tests;
#[cfg(test)]
mod engineer_building_repair_tests;
#[cfg(test)]
mod engineer_power_consumer_tests;
#[cfg(test)]
mod fly_height_tests;
#[cfg(any(test, feature = "test-support"))]
pub mod gap_generator_tests;
#[cfg(test)]
mod gsi_04_18_tests;
#[cfg(test)]
mod house_ai_activation_tests;
#[cfg(test)]
mod jumpjet_infantry_tests;
#[cfg(any(test, feature = "test-support"))]
mod lifecycle_tests;
#[cfg(test)]
mod mastermind_overload_tests;
#[cfg(test)]
mod projectile_trailer_tests;
#[cfg(test)]
mod squid_grapple_tests;
#[cfg(test)]
mod suicide_fire_tests;
#[cfg(any(test, feature = "test-support"))]
pub use lifecycle_tests::common_raw_terrain_cell as common_raw_test_terrain_cell;
#[cfg(test)]
mod team_script_vm_tests;

pub(crate) use crash::CrashLatch;
pub(crate) use display_registry::entity_layer;
pub use lifecycle::LifecycleOutput;
pub use lifecycle::{
    BuildingLight, ConcealOutcome, ObjectLifecycle, PlacementEvidence, RemovalFacts, RevealOutcome,
    RevealPosition, RevealRequest, TechnoDiscoveryHistory, UninitContext,
};
#[cfg(any(test, feature = "test-support"))]
pub use lifecycle::{LifecycleTestEvent, RevealFailure};
pub(crate) use load_object_lifecycle::{LoadObjectLifecycle, OverlayConstructionReceipt};
pub(crate) use logic_vector::{LogicMembership, LogicVector};
pub(crate) use object_entry::{EntryFacts, FootEntryReceiver};
#[cfg(any(test, feature = "test-support"))]
pub(crate) use substrate::DeferredDeleteQueue;
pub use substrate::EnterOrderCounter;
pub(crate) use substrate::{DeferredDeleteTicket, ObjectKind, ObjectSubstrate};
pub(crate) use world_spawn::{CrushableFacts, MoverFacts, TypeFacts};
pub use world_spawn::{GeneratedTechnoInitError, GeneratedTechnoInitTable};

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[cfg(any(test, feature = "test-support"))]
use crate::map::actions::ActionMap;
use crate::map::entities::EntityCategory;
#[cfg(any(test, feature = "test-support"))]
use crate::map::events::EventMap;
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
use crate::sim::combat::combat_weapon::WeaponSlot;
use crate::sim::command::CommandEnvelope;
use crate::sim::components::{AnimClassSpawnDescriptor, DriveCoord, Position};
use crate::sim::entity_store::EntityStore;
use crate::sim::house_state::HouseState;
use crate::sim::house_strategy;
use crate::sim::intern::InternedId;
use crate::sim::lifecycle_request::LifecycleRequest;
use crate::sim::movement;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::overlay_grid::{
    WallDamageTransactionHost, WallDirtyStep, WallPointerTarget,
    damage_wall_overlay_with_runtime_host, recalc_overlay_passability,
};
use crate::sim::passenger;
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::terrain_speed;
use crate::sim::pathfinding::zone_incremental::{
    PackedZoneCoord, ZoneRepairKind, repair_zone_cell,
};
use crate::sim::pathfinding::zone_map::ZoneGrid;
use crate::sim::power_system::{self, PowerState};
use crate::sim::production::{self, ProductionState};
use crate::sim::projectile::{
    Projectile, ProjectileBridgeCrossing, ProjectileCollisionResponse, ProjectileCoord,
    projectile_bridge_crossing,
};
use crate::sim::radar::{RadarEventRequest, RadarEventType};
use crate::sim::rng::{MainRng, SimRng, SimRngLogicalState, SimRngLogicalView};
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::team_script_vm::TeamScriptVm;
use crate::sim::tiberium::TiberiumPlacementObjectContext;
use crate::sim::trigger_runtime::{TriggerEffect, TriggerRuntime};
use crate::sim::vision::{self, FogState};
use crate::util::fixed_math::SimFixed;

/// Test fixtures' seed. Real launches negotiate a per-match seed through
/// `ScenarioDescriptor`, and only test builds can construct with this value.
#[cfg(any(test, feature = "test-support"))]
const DEFAULT_SIM_SEED: u64 = 0x5EED_CAFE_D15E_A5E5;

#[derive(Default)]
struct ActiveVisionStructures {
    gap_generators: BTreeMap<InternedId, Vec<vision::GapGeneratorSource>>,
}

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
    /// An entity's owner changed (garrison reconciliation, engineer capture) — sprite
    /// atlas needs rebuild for the new house color.
    pub ownership_changed: bool,
    /// Bridge state changed this tick. Mutation owners publish navigation at
    /// their synchronous reader boundary; the frame finalizer also rebuilds
    /// the projection before committing the frame for subsequent ticks.
    pub bridge_state_changed: bool,
    pub movement: movement::MovementTickStats,
}

/// The master spine retains its admitted command batch for the live recorder.
/// Fixture callers can consume the tick without draining presentation queues.
#[derive(Debug)]
pub(crate) struct MasterFrameOutput {
    tick: TickResult,
    admitted_commands: Vec<CommandEnvelope>,
}

impl MasterFrameOutput {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn into_tick(self) -> TickResult {
        self.tick
    }
}

/// One authoritative frame plus the transient facts emitted while producing it.
///
/// Channel order is preserved within each vector. The app owns cross-channel
/// presentation order; collecting this value only transfers ownership and does
/// not invent a single mixed event timeline.
#[derive(Debug)]
pub struct SimFrameOutput {
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
    pub lighting_events: Vec<crate::sim::light_sources::LightingEvent>,
    /// The retail sync checksum this frame's multiplayer `Queue_AI` computed
    /// for the native history ring; `None` on single-player routes and on
    /// frames that terminated before the tail.
    pub multiplayer_checksum: Option<FrameSyncChecksum>,
}

/// One frame's retail multiplayer checksum, stamped with the pre-commit
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

/// Existing rule owner borrowed for one authoritative trigger frame.
/// Live definitions and mutable Trigger/Tag state belong to `Simulation`.
#[derive(Clone, Copy)]
pub struct TriggerInputs<'a> {
    // Legacy focused fixtures finish their test-only definition installation
    // on first dispatch. Production uses the installed live registry only.
    #[cfg(any(test, feature = "test-support"))]
    pub events: &'a EventMap,
    #[cfg(any(test, feature = "test-support"))]
    pub actions: &'a ActionMap,
    /// Bound match rules used by action callbacks that share ordinary Techno
    /// runtime calculations (not reparsed or substituted by trigger data).
    pub rules: Option<&'a RuleSet>,
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MasterFrameTestRung {
    SessionCommands,
    Triggers,
    LogicVector,
    CrateRegen,
    Houses,
    TeamScript,
    FrameCommit,
    PendingDelete,
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HouseAiActivationOrderTestEvent {
    ProductionCompleted,
    HouseAngerDecay(InternedId),
    HouseActivation(InternedId),
    DefeatProcessed,
}

/// A sound event produced during simulation (combat, death, production).
/// Pure data — no audio library dependency. Drained by the app layer each frame.
#[derive(Debug, Clone)]
pub enum SimSoundEvent {
    /// PlanMgr638593 accepted an editing order; audio gates the local issuer.
    PlanningNodeAdded { owner: InternedId },
    /// TAction21 Execute6DD8B0 calls VoxQueueVoice752480 with its literal
    /// signed DialogList index, type2 and priority-1. The process audio
    /// catalog owns ordinal resolution; no cue-name guess enters simulation.
    TriggerEva { index: i32 },
    /// A pending acknowledgement owner's live AI reached Techno6F9EBB.
    /// The app owns the latch/handle; this fact preserves the actual visit.
    UnitVoiceVisit { owner: u64 },
    /// An admitted QueueVoice708D90 request, before any later positional
    /// cue. The sole audio owner latches it; only a reached Techno head drains.
    /// Keep the canonical name outside the gameplay identity interner.
    UnitVoiceRequested { owner: u64, sound_id: String },
    /// Techno destructor6F4607 destroys its voice handle before its owned
    /// Anims. The app discards both pending and playing voice identities.
    UnitVoiceDestroyed { owner: u64 },
    /// Voc7509E0 on the object's handle (Building+6A0 for Construction).
    /// The existing app/audio handle owner performs looping updates and release.
    ObjectSoundStarted {
        owner: u64,
        sound_id: InternedId,
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// Start/report sound on an object or animation handle. Keep the canonical
    /// sound name outside the gameplay interner: a Main-selected cue must not
    /// allocate an ID that changes later gameplay hashes or saved identities.
    AnimationStarted {
        anim_id: crate::sim::anim_class::AnimId,
        sound_id: String,
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// Hard-stop the owner's handle, then optionally play StopSound. Anim
    /// destruction emits ObjectSoundReleased first so its Report plays out.
    AnimationStopped {
        anim_id: crate::sim::anim_class::AnimId,
        stop_sound_id: Option<InternedId>,
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// `IncreaseGattlingStage`'s report (`VocClass::PlayAt @ 0x0070DFDD` on
    /// the techno's `+0x4A4` handle): start the stage weapon's loop, stopping
    /// any loop the handle held. `owner` is `combat::gattling::
    /// gattling_sound_owner(techno)`.
    GattlingLoop {
        owner: u64,
        sound_id: String,
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// A stage-up's hard stop of that loop (`VocHandle::StopAndClear @
    /// 0x00405D40`).
    GattlingLoopStop { owner: u64 },
    /// A decay's or Limbo's release of that loop (`SoundEvent::Release @
    /// 0x00406060`): it stops repeating and plays out.
    GattlingLoopRelease { owner: u64 },
    /// `SoundEvent::Release @ 0x00406060` on an object's own sound handle
    /// (`FootClass+0x544` or `AnimClass+0x1A0`, keyed by object id). A one-shot
    /// plays out; an uncounted loop stops repeating. Anim UnInit4255D5 and
    /// scalar destructor4228E0 share this operation with Foot4D3677.
    ObjectSoundReleased { owner: u64 },
    /// FootLimbo4DB353 ->405FD0: stop repetition for any valid event, even
    /// a counted loop, and detach the handle. The current audio plays out.
    ObjectSoundDetached { owner: u64 },
    /// Native Fly AuxSound1/AuxSound2 at the phase callback world coordinate.
    AircraftPhase {
        sound_id: InternedId,
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// An entity was destroyed — play its DieSound=.
    EntityDied {
        die_sound_id: String,
        rx: u16,
        ry: u16,
    },
    /// An entity was crushed by a vehicle — play its CrushSound= (the squish).
    /// Normal crush teardown does not also enter the ordinary DieSound path.
    EntityCrushed {
        crush_sound_id: InternedId,
        rx: u16,
        ry: u16,
    },
    /// DeploySound on infantry stance entry, successful unit-to-building
    /// conversion, or a building's undeploy start (Selling's UndeploysInto
    /// arm plays the building type's own `DeploySound=`).
    EntityDeployed {
        deploy_sound_id: InternedId,
        rx: u16,
        ry: u16,
    },
    /// An infantry entity entered the Undeploying phase — play its UndeploySound=.
    EntityUndeployed {
        undeploy_sound_id: InternedId,
        rx: u16,
        ry: u16,
    },
    /// A transport placed an ejected passenger — `UnitClass::Mission_Unload @
    /// 0x0073D630` plays the transport type's `LeaveTransportSound=`
    /// (`+0x568`) through `VocClass::PlayAtCoord` at the TRANSPORT's own
    /// coordinate (`0x0073DC28`..`0x0073DC67`), once per placed passenger.
    LeaveTransport {
        sound_id: InternedId,
        rx: u16,
        ry: u16,
    },
    /// A building finished construction — play EVA "Construction complete".
    BuildingComplete { owner: InternedId },
    /// `HouseClass::Place_Production 0x004FB5C6..0x004FB644`: a unit left the
    /// factory of a human-controlled house. `EVA_UnitReady` plays for the
    /// local owner once its client admits `radar` (type 6).
    UnitComplete {
        owner: InternedId,
        radar: RadarEventRequest,
    },
    /// One accepted HouseClass win/loss transition. The app resolves the
    /// local owner's faction-specific STANDARD EVA; this edge is transient so
    /// loading a mid-Savour snapshot cannot replay the announcement.
    MatchOutcome {
        owner: InternedId,
        kind: crate::sim::house_state::HouseOutcomeKind,
    },
    /// Global `[AudioVisual] SellSound=` for a successful wall-sale event.
    /// The EventClass receiver can differ from the wall owner.
    WallSold { receiver: InternedId },
    /// A deploy command failed target placement validation.
    /// App layer gates this to the local human player and plays
    /// `EVA_CannotDeployHere`.
    CannotDeployHere { owner: InternedId },
    /// `FactoryClass::StartProduction` refused to queue a build
    /// (`0x004C9D3B..0x004C9D5F`): `[AudioVisual] ScoldSound=` when the house
    /// is the local player's.
    ProductionRefused { owner: InternedId },
    /// The PLACE event's Unlimbo succeeded (`HouseClass @ 0x004FB0E0`,
    /// `0x004FB236`; a wall's Unlimbo also returns 1 at `0x00440865`):
    /// `[AudioVisual] BuildingSlam=` centred at full volume when the house is
    /// the local player's (`0x004FB2CC..0x004FB314`).
    BuildingPlaced { owner: InternedId },
    /// A chrono teleport happened — play the resolved warp sound at this position.
    /// Sim emits two of these per warp: one at the source cell with the unit's
    /// `ChronoOutSound=`, one at the destination cell with the unit's
    /// `ChronoInSound=`.
    ChronoTeleport {
        sound_id: InternedId,
        rx: u16,
        ry: u16,
    },
    /// `TechnoClass::StartUncloaking @ 0x007036C0` accepted an arg-zero
    /// state-1/2 transition and called positional `VocClass::PlayAt @
    /// 0x007509E0` with `[AudioVisual] CloakSound` at the Techno world coord.
    CloakSound {
        sound_id: String,
        rx: u16,
        ry: u16,
        sub_x: SimFixed,
        sub_y: SimFixed,
        world_z_leptons: i32,
    },
    /// `UnitClass::PerCellProcess @ 0x0073B036..B04D`: a crusher flattened the
    /// overlay under it and played the overlay type's `CrushSound=`
    /// (`OverlayTypeClass+0x1F0`) positionally at its own coordinates.
    WallCrushed {
        sound_id: String,
        rx: u16,
        ry: u16,
        sub_x: SimFixed,
        sub_y: SimFixed,
        world_z_leptons: i32,
    },
    /// `VocClass::PlayAtPos @ 0x00750920` of a rules-named sound, centred
    /// (`0x2000`), full volume (`1.0f`), without an owner handle. Walk's
    /// exhausted-retry branch 0x0075B085..0x0075B0B0 requests ScoldSound this
    /// way. Evidence: `tools/spatial_oracle/foot_scold_latch.{py,json,md}`.
    VocCentered { sound_id: String },
    /// `VocClass::PlayAt @ 0x007509E0` of a rules-named sound at an object's
    /// Location: the mind-control capture, release and overload sounds.
    /// `audible_to` names the houses a `HouseClass::IsHumanPlayer
    /// @ 0x0050B6F0` gate admits (the local player must own one of them; the
    /// app resolves it); `None` plays for everyone.
    VocAt {
        sound_id: String,
        audible_to: Option<[InternedId; 2]>,
        rx: u16,
        ry: u16,
        sub_x: SimFixed,
        sub_y: SimFixed,
        world_z_leptons: i32,
    },
    /// VocClass::PlayAtCoord750E20 retains the full signed native coordinate
    /// and owns its playback handle independently of the source object.
    VocAtCoord {
        sound_id: String,
        coordinate: DriveCoord,
        flags: u32,
    },
    /// VocClass750F40 detaches independent coordinate requests whose flags
    /// overlap and whose full signed XYZ equals the supplied coordinate.
    VocStopAtCoord { coordinate: DriveCoord, flags: u32 },
    /// `TechnoClass::AI_Update @ 0x006FA054..0x006FA145` crossed a rank.
    ///
    /// Native plays `[AudioVisual] UpgradeVeteranSound=`/`UpgradeEliteSound=`
    /// positionally at the object (`VocClass::PlayAt @ 0x007509E0`) and the
    /// `EVA_UnitPromoted` voice (`0x00752700`), both only when
    /// `HouseClass::IsHumanPlayer @ 0x0050B6F0` holds for the owner — in a
    /// skirmish that is `owner == the local player`, which the app resolves.
    /// `sound_id` is `None` when the rules key is absent or empty (native's
    /// invalid Voc index is silence).
    UnitPromoted {
        owner: InternedId,
        sound_id: Option<InternedId>,
        elite: bool,
        rx: u16,
        ry: u16,
    },
    /// A building / harvester admitted a damage or first Temporal-head warning.
    /// For the local owner the app admits `radar` (type 3 base / type 4 miner) on its client
    /// event array; that result is native's only rate limit on the EVA voice.
    UnderAttack {
        rx: u16,
        ry: u16,
        owner: InternedId,
        miner: bool,
        radar: RadarEventRequest,
    },
    /// `HouseClass::NotifyUnderAttack @ 0x004F93E0`, non-local branch
    /// (`0x004F955D..0x004F95B3`): a building of a house that lists `owner`
    /// (a human house) as its ally admitted a warning. Once the listener's
    /// client admits `radar` (`CreateRadarEvent(0x10, cell)`):
    /// `EVA_OurAllyIsUnderAttack` plus the `BaseUnderAttackSound` siren, both
    /// for `owner`.
    AllyUnderAttack {
        owner: InternedId,
        radar: RadarEventRequest,
    },
    /// `TechnoClass::Death_Announcement @ 0x004D98C0` fired for a unit of
    /// `owner` (a human house) that is not `Spawned=`. The app plays
    /// `EVA_UnitLost` for the local owner once its client admits `radar`
    /// (`CreateRadarEvent(7, cell)`, 8-cell dedupe).
    UnitLost {
        owner: InternedId,
        radar: RadarEventRequest,
    },
    /// A `PlayEVA` line native speaks only for the player's house: the
    /// `HouseClass::Update` advice (`EVA_InsufficientFunds` `0x004F8BA0`,
    /// `EVA_LowPower` `0x004F8D14`), the depot's empty wallet, the crate
    /// upgrades and the Robot Tank lines (`sim::powered_unit`). `event` is
    /// the `evamd.ini` section name; the app applies the local-owner test.
    HouseEva {
        owner: InternedId,
        event: &'static str,
    },
    /// `BuildingClass::Mission_Selling @ 0x00449C30`'s completing stage-2 visit
    /// (`0x00449CC1..0x00449CE5`, once the pack-up animation has set
    /// `+0x6DD`): `TechnoClass+0x41A` (owner is the local player) and no
    /// `UndeploysInto=` (`Type+0x408`) — an undeploying building stays
    /// silent. The upgrade-sale arm (`0x0044AB22..0x0044AB36`) is dormant in
    /// retail data. App plays `EVA_StructureSold`.
    StructureSold { owner: InternedId },
    /// A sale order's click (`BuildingClass::Sell_Back @ 0x00447110`:
    /// `VocClass::PlayAtPos` of `[AudioVisual] GenericClick=`) for the
    /// owner's player (`HouseClass::IsHumanPlayer @ 0x0050B6F0`; the app
    /// resolves it).
    SellClick { owner: InternedId },
    /// `BuildingClass::ToggleRepair @ 0x00446FF0` (`0x004470B7`): repair
    /// switched on while `Health != Type.Strength` (`0x00447059`) and the
    /// owner is the local player (`0x004470A4 CALL 0x0050B6F0`). App plays
    /// `EVA_Repairing` for the local owner. Depot MissionRepair44C507 emits
    /// the same request when its first paid service step starts.
    Repairing { owner: InternedId },
    /// Depot MissionRepair44BDB2 reaches CreateRadarEvent8 after a service
    /// request completes. The app resolves the local-owner+41A gate and
    /// existing client radar admission before playing EVA_UnitRepaired.
    UnitRepaired {
        owner: InternedId,
        radar: RadarEventRequest,
    },
    /// `BuildingClass::ChangeOwner @ 0x00448260` announce block
    /// (`0x004483C0..0x0044848F`) for an engineer capture
    /// (`InfantryClass::PerCellProcess 0x00519A27 PUSH 1` = announce). Native
    /// gate: the OLD or the NEW owner is the local player (`0x004483C6` /
    /// `0x004483D1 CALL 0x0050B6F0`), the new owner's type not
    /// `MultiplayPassive` (`0x004483E1`). `tech_building` is `NeedsEngineer=`
    /// (`Type+0x1552`, `0x00448401`): clear → `CreateRadarEvent(10, cell)`
    /// (`0x00448472 MOV ECX,0xA`) and, when accepted, `EVA_BuildingCaptured`
    /// (`0x0044848A`); set → `EVA_TechBuildingLost` for a local OLD owner
    /// (`0x00448415`, ECX = `this->Owner`) and the type's `CaptureEvaEvent=`
    /// (`Type+0x1554`, `0x00448459 QueueVoice`) for a local NEW owner. The
    /// sim cannot see the local player: it emits for any human-controlled
    /// side and the app applies the local test. `radar` is the type-10
    /// request of an ordinary building; a tech building reaches none.
    BuildingCaptured {
        old_owner: InternedId,
        new_owner: InternedId,
        tech_building: bool,
        radar: Option<RadarEventRequest>,
        capture_eva_event: Option<InternedId>,
    },
    /// `BuildingClass::Infiltrate @ 0x004571E0`'s announcements
    /// (`sim::spy_infiltration`). When the building's owner or the spy's
    /// house is the local player (`0x004571FE`, `0x0045720E`, both
    /// `0x0050B6F0`), `CreateRadarEvent(9, cell)` runs (`0x0045722F`), and
    /// its answer gates most of the lines `effect` picks
    /// (`eva_producers::infiltration_eva_events`). The sim cannot see the
    /// local player: it emits when either side is human-controlled and the
    /// app applies the local test.
    BuildingInfiltrated {
        owner: InternedId,
        infiltrator: InternedId,
        effect: crate::sim::spy_infiltration::InfiltrationEffect,
        radar: RadarEventRequest,
    },
    /// `SuperClass::AI_Ready @ 0x006CBCA0` (`0x006CBE63`): the charge
    /// expired and `+0x6F` (IsReady) was set (`0x006CBDCC`); the announce
    /// argument is `house == PlayerPtr` (`0x004F8E42..0x004F8E47` in
    /// `HouseClass::Update`). `sw_type` is the `[SuperWeaponTypes]` section;
    /// the app maps its `Type=` onto the `*Ready` line through the
    /// `0x006CBDE6` jump table (`0x006CBEA8`).
    SuperWeaponReady {
        owner: InternedId,
        sw_type: InternedId,
    },
    /// `HouseClass @ 0x0050AF10` (label `HouseClass__Update_Owned_Supers`):
    /// a granted Super's hold changed (`SuperClass::Suspend @ 0x006CB4D0`
    /// returned true) or it was lost (`SuperClass @ 0x006CB7B0` returned
    /// true). For the player's house native then clears the selected Super
    /// (`0x008809A0`) when it is this one (`0x0050B181..0x0050B190`); the app
    /// applies that local-owner half.
    SuperWeaponStatusChanged {
        owner: InternedId,
        sw_type: InternedId,
    },
    /// `BuildingClass::OnConstructionComplete @ 0x00445F80`
    /// (`0x004468AD..0x00446995`): a building whose type carries
    /// `SuperWeapon=` finished building up. Native speaks only when the owner
    /// is not the local player (`0x004468B3`), not allied with it
    /// (`0x004468CD IsAlliedWith(PlayerPtr)`), `[0xA8B538] == 0` (local
    /// player not defeated, `0x004468DA`), `GameMode != 0` (`0x004468E7`),
    /// and the type's `AuxBuilding=` is absent or owned by the building's
    /// owner (`0x0044692E CountOwnedInstances`); the line comes from the
    /// `[SuperWeaponTypes]` list index (`0x00446948` table at `0x00446FC0`).
    /// The app applies the listener gates and the table.
    SuperWeaponDetected {
        owner: InternedId,
        sw_type: InternedId,
    },
    /// `HouseClass::MPlayer_Defeated @ 0x004FC0B0`, non-local branch
    /// (`0x004FC30C..0x004FC3BC`): a house whose type is not
    /// `MultiplayPassive` (`0x004FC30F`) was defeated. Native also skips the
    /// line for the observer house (`0x004FC343 CMP ESI,[0xAC1198]`), which
    /// VERA does not model. App plays `EVA_PlayerDefeated` when `house` is
    /// not the local player (the local defeat is `EVA_YouHaveLost` via
    /// [`SimSoundEvent::MatchOutcome`]).
    PlayerDefeated { house: InternedId },
    /// A superweapon fired. `sw_type` is the interned `[SuperWeaponTypes]`
    /// section name of the launched weapon — the discriminator the app layer
    /// needs to pick the cue, and the same object gamemd switches on.
    ///
    /// `SuperClass::Launch @ 0x006CC390` switches on `*(param_1[10] + 0xB4)`,
    /// the launched `SuperWeaponTypeClass`'s `Type=` index, and each case
    /// plays its own cue: nothing at all for `ChronoSphere`, `ParaDrop`,
    /// `AmerParaDrop` and `SpyPlane`; an EVA line only for `IronCurtain`
    /// (`0x006CCF21`), `LightningStorm` (`0x006CCD81`) and `ChronoWarp`
    /// (`0x006CCD03`); a positional `[AudioVisual]` cue only for `ForceShield`
    /// (`0x006CD176`, from the *type's* own `StartSound=`) and `PsychicReveal`
    /// (`0x006CD7BF`); and both for `MultiMissile`, `PsychicDominator` and
    /// `GeneticConverter`. Carrying the type rather than a per-case flag is
    /// what lets the app read both the case index and `StartSound=` off one
    /// object, as `param_1[10]` does.
    SuperWeaponLaunched {
        owner: InternedId,
        sw_type: InternedId,
        rx: u16,
        ry: u16,
    },
    /// A `CreateRadarEvent` with no local-player test and no sound, on every
    /// client: each admits it on its own radar. Type 13 from
    /// `SuperClass::Launch` case 1 (`0x006CCF2F`), case 4 (`0x006CC4BE` at
    /// the Chronosphere's source, `0x006CC4D2` at its target), case 7
    /// (`0x006CCDD7`), a storm's start (`0x00539F89`) or a `NUKE` warhead's
    /// impact (`BulletClass::AI`, `0x00467EA7`); type 12 from a computer
    /// house's `RevealToAll=` building opening (`0x004468A8`).
    RadarEvent { radar: RadarEventRequest },
    /// A Lightning Storm started (`LightningStorm::Start @ 0x00539EB0`'s
    /// undeferred half) under `[General] LightningPrintText=` (`0x0053A014`;
    /// without it nothing is pushed): every client plays `StormSound` centred
    /// (`0x0053A044`) and posts `TXT_LIGHTNING_STORM` in the player's colours
    /// (`0x0053A076`). On retail data (`LightningDeferment=250`) this trails
    /// the launch's EVA line by the whole countdown.
    LightningStormBegan,
    /// A deferred Lightning Storm's countdown reached a multiple of 225
    /// (`LightningStorm::Process @ 0x0053AAD7`) under `LightningPrintText=`
    /// (`0x0053AAE9`; without it nothing is pushed): every client speaks
    /// `EVA_LightningStormCreated` (`0x0053AB11`) and posts
    /// `TXT_LIGHTNING_STORM_APPROACHING` (`0x0053AB40`).
    LightningStormApproaching,
    /// `owner`'s Lightning Storm was refused because a storm rages or counts
    /// down (`SuperClass::ClickFire @ 0x006CBAA7..0x006CBAC7`). The client
    /// whose player owns it (Fire_SW passes `this == PlayerPtr`,
    /// `0x004FAE8E`) posts `Msg:LightningStormActive`
    /// (`LightningStorm::PrintMessage @ 0x0053AE00`).
    LightningStormRefused { owner: InternedId },
    /// `owner`'s Psychic Dominator was refused because one is active
    /// (`0x006CBAD6..0x006CBAF6`). The client whose player owns it posts
    /// `Msg:DominatorActive` (`PsyDom::PrintMessage @ 0x0053B410`).
    PsychicDominatorRefused { owner: InternedId },
    /// First occupant entered a CanBeOccupied building (cargo 0→1).
    /// Owner is the building owner at AddGarrisonOccupant time; civilian
    /// ownership transfer is reported separately from building reconciliation.
    /// App layer plays EVA_StructureGarrisoned if owner is local human.
    StructureGarrisoned { owner: InternedId },
    /// CheckAutoSellOrCivilian458200 abandoned an empty map garrison.
    /// House50B6F0 already admitted the old owner. The sound precedes radar
    /// admission; EVA plays only when CreateRadarEvent accepts the request.
    StructureAbandoned {
        owner: InternedId,
        radar: crate::sim::radar::RadarEventRequest,
    },
    /// First-occupant SFX from rulesmd [AudioVisual] BuildingGarrisonedSound.
    /// Positional cue gated on owner == local human.
    BuildingGarrisonedSfx { owner: InternedId, rx: u16, ry: u16 },
    /// Tank-bunker walls-up cue — emitted on install. App resolves to
    /// [AudioVisual] BunkerWallsUpSound (retail "TankBunkerUp").
    BunkerWallsUp { rx: u16, ry: u16 },
    /// Tank-bunker walls-down cue — emitted on normal exit / clear teardown.
    /// App resolves to [AudioVisual] BunkerWallsDownSound (retail "TankBunkerDown").
    BunkerWallsDown { rx: u16, ry: u16 },
    /// A paratrooper left the paradrop plane (`AircraftClass::Drop_Payload`
    /// plays `[AudioVisual] ChuteSound=` at the plane's Location,
    /// `0x00415E21`): the plane's cell.
    ChuteSound { rx: u16, ry: u16 },
    /// An engineer entered a `BridgeRepairHut` and triggered bridge repair.
    /// Played at the BUILDING's cell, NOT the engineer's. `owner` is the
    /// engineer's house. App layer plays the spatial `[BridgeRepaired]` sound
    /// for everyone in range, gated on
    /// `rules.bridge_rules.repair_sound.is_some()`. `radar` is present when
    /// native House50B6F0 passed; the EVA line then waits on the client's
    /// non-drawing type-14 radar event creation/dedup gate.
    BridgeRepaired {
        rx: u16,
        ry: u16,
        owner: InternedId,
        radar: Option<RadarEventRequest>,
    },
}

impl SimSoundEvent {
    pub(crate) fn cloak_sound(sound_id: String, position: &Position) -> Self {
        Self::CloakSound {
            sound_id,
            rx: position.rx(),
            ry: position.ry(),
            sub_x: position.sub_x(),
            sub_y: position.sub_y(),
            world_z_leptons: Self::world_z_leptons(position),
        }
    }

    pub(crate) fn wall_crushed(sound_id: String, position: &Position) -> Self {
        Self::WallCrushed {
            sound_id,
            rx: position.rx(),
            ry: position.ry(),
            sub_x: position.sub_x(),
            sub_y: position.sub_y(),
            world_z_leptons: Self::world_z_leptons(position),
        }
    }

    pub(crate) fn voc_at(sound_id: String, position: &Position) -> Self {
        Self::voc_at_for(sound_id, None, position)
    }

    /// `VocClass::PlayAt @ 0x007509E0` for a source whose authoritative
    /// location comes from `FootClass::GetCoords` rather than raw Position.
    pub(crate) fn voc_at_world_coord(
        sound_id: String,
        coordinate: crate::sim::anim_class::AnimWorldCoord,
    ) -> Self {
        let (rx, ry, sub_x, sub_y, _) = coordinate.to_cell_sub_z();
        Self::VocAt {
            sound_id,
            audible_to: None,
            rx,
            ry,
            sub_x,
            sub_y,
            world_z_leptons: coordinate.z,
        }
    }

    pub(crate) fn voc_at_for(
        sound_id: String,
        audible_to: Option<[InternedId; 2]>,
        position: &Position,
    ) -> Self {
        Self::VocAt {
            sound_id,
            audible_to,
            rx: position.rx(),
            ry: position.ry(),
            sub_x: position.sub_x(),
            sub_y: position.sub_y(),
            world_z_leptons: Self::world_z_leptons(position),
        }
    }

    fn world_z_leptons(position: &Position) -> i32 {
        crate::sim::movement::ground_pose::position_world_coord(position).z
    }
}

/// The firer's own position at the authoritative fire tick (the electric
/// spark admission reads it; the muzzle is `SimFireEvent::fire_coord`).
#[derive(Debug, Clone)]
pub struct FireOriginSnapshot {
    pub rx: u16,
    pub ry: u16,
    pub sub_x: SimFixed,
    pub sub_y: SimFixed,
    pub z: u8,
    pub facing: u8,
}

/// One shot, as combat resolved it at the authoritative fire tick. The world
/// constructs the shot's muzzle animation from it and the app positions the
/// weapon report sound at `fire_coord`.
#[derive(Debug, Clone)]
pub struct SimFireEvent {
    /// Stable ID of the entity that fired.
    pub attacker_id: u64,
    /// Type id of the firing object at the fire tick.
    pub attacker_type_ref: InternedId,
    /// Which weapon slot was used (Primary or Secondary).
    pub weapon_slot: WeaponSlot,
    /// Selected weapon section id.
    pub weapon_id: InternedId,
    /// Firing object's facing at the fire tick.
    pub facing: u8,
    /// Firing object's veterancy at the fire tick.
    pub veterancy: u16,
    /// Source facts from the fire tick, before burst/cooldown updates.
    pub origin_snapshot: FireOriginSnapshot,
    /// What was fired at — entity stable ID or ground cell coord.
    /// For projectile trajectory: Entity → look up entity position; Cell →
    /// use cell center as the destination.
    pub target: crate::sim::combat::TargetKind,
    /// Weapon report sound id. The app layer positions this at the resolved
    /// fire origin for both normal and garrison fire.
    pub report_sound_id: Option<InternedId>,
    /// The shot's fire coordinate in world leptons (`combat::fire_coord`): the
    /// bullet origin, the muzzle animation and the report sound share it.
    pub fire_coord: crate::sim::projectile::ProjectileCoord,
    /// `fire_coord.y` minus the firer coordinate's Y; a building's muzzle
    /// animation derives its `ZAdjust` from it.
    pub fire_offset_y: i32,
    /// The muzzle `AnimClass` type this shot constructs, if any: the weapon's
    /// `Anim=` by aim facing, or `OccupantAnim=` for an occupied building.
    pub muzzle_anim: Option<InternedId>,
    /// The firer is a building with occupants (native's `+0x408` count above
    /// zero), whichever weapon fired.
    pub occupied_building: bool,
    /// The firer's class: a building's flash is not attached to it.
    pub firer_category: EntityCategory,
}

#[cfg(any(test, feature = "test-support"))]
impl SimFireEvent {
    /// A primary-weapon shot by unit `attacker_id` from cell (0, 0), with no
    /// report, no muzzle animation and a zero fire coordinate.
    pub fn for_test(attacker_id: u64) -> Self {
        Self {
            attacker_id,
            attacker_type_ref: crate::sim::intern::test_intern("TESTFIRER"),
            weapon_slot: crate::sim::combat::combat_weapon::WeaponSlot::Primary,
            weapon_id: crate::sim::intern::test_intern("TESTWEAPON"),
            facing: 0,
            veterancy: 0,
            origin_snapshot: FireOriginSnapshot {
                rx: 0,
                ry: 0,
                sub_x: crate::util::fixed_math::SimFixed::ZERO,
                sub_y: crate::util::fixed_math::SimFixed::ZERO,
                z: 0,
                facing: 0,
            },
            target: crate::sim::combat::TargetKind::Cell(0, 0),
            report_sound_id: None,
            fire_coord: crate::sim::projectile::ProjectileCoord::new(0, 0, 0),
            fire_offset_y: 0,
            muzzle_anim: None,
            occupied_building: false,
            firer_category: EntityCategory::Unit,
        }
    }
}

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

fn deserialized_process_rng_placeholder() -> SimRng {
    SimRng::new(0)
}

fn deserialized_main_rng_placeholder() -> MainRng {
    MainRng::new(0)
}

fn every_frame() -> Option<std::num::NonZeroU64> {
    Some(std::num::NonZeroU64::MIN)
}

fn deserialize_scenario_rng_reset<'de, D>(deserializer: D) -> Result<SimRng, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // gamemd saves ScenarioClass as a raw block that includes Random at +0x218,
    // then immediately reseeds that embedded object with zero after reading it.
    // Consume the saved bytes to preserve our snapshot layout, but reproduce the
    // active load result instead of restoring the saved cursor.
    let _saved = <SimRng as serde::Deserialize>::deserialize(deserializer)?;
    Ok(SimRng::new(0))
}

/// The game simulation - owns all authoritative game state.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Simulation {
    /// String interner for owner/type_ref — zero-cost ID clones instead of heap Strings.
    pub interner: crate::sim::intern::StringInterner,
    /// Runtime TechnoType+634 writes; RuleSet remains the frozen read projection.
    #[serde(default)]
    type_tech_levels: TypeTechLevels,
    /// Pre-resolved `[CombatDamage]` warhead handles (F04). Built by
    /// `resolve_type_handles` beside the type table; rebuilt from rules on
    /// load. NOT serialized, NOT hashed. `None` until resolved.
    #[serde(skip)]
    pub(crate) rule_handles: Option<crate::sim::type_handle_table::ResolvedRuleHandles>,
    /// Factories, ready buildings and producer focus; also ore growth,
    /// terrain objects and airfield docks.
    pub production: ProductionState,
    /// Session aggregate — scenario identity, seed, authoritative map
    /// bounds, MP start table, per-match options, and the frame clocks
    /// (`tick` plus the wrapping `binary_frame`, committed late at the end of
    /// `advance_tick`). `total_sim_ms` is diagnostic only. Constructed once
    /// from the app-layer descriptor; serialized + hashed except for that
    /// diagnostic accumulator. See `sim::scenario_session`.
    pub session: ScenarioSession,
    /// Native A8E7AC is a synchronous, wrapping placement/destruction scope,
    /// independent of GameMode. Balanced at frame boundaries; not saved/hash
    /// state. Only lifecycle's nested scope API mutates it.
    #[serde(skip, default)]
    object_placement_scope_depth: u32,
    /// VERA's analog of native `0xA8ED6B`, which sends an initial building
    /// Unlimbo to Enter_Idle_Mode's Idle arm. Raised only by lifecycle's
    /// bypass API; balanced at frame boundaries; not saved/hash state.
    #[serde(skip, default)]
    buildup_bypass: bool,
    /// Scenario RNG — gamemd `Scenario->Random` (Scen+0x218). Drives in-object-tick
    /// sim draws: scatter, sub-cell placement, smudge/destruction, particles,
    /// wall/overlay damage, bridge collapse/destruction presentation, ore growth/spread, TIBTRE,
    /// anim scorch/50-50, miner-dock jitter, and the terminal score projection.
    /// Saved in the Scenario-shaped snapshot and hashed while live. Native load
    /// reads those bytes but immediately calls `Random__Seed(0)`, so deserialization
    /// deliberately discards the saved cursor.
    #[serde(deserialize_with = "deserialize_scenario_rng_reset")]
    pub scenario_rng: SimRng,
    /// Main/global RNG — gamemd `g_MainRng` (0x00886B88). This is a
    /// process-global cursor: it is neither part of ScenarioClass saves nor
    /// multiplayer checksums, and the live cursor continues across a load.
    #[serde(skip, default = "deserialized_main_rng_placeholder")]
    pub main_rng: MainRng,
    /// Map-generator RNG — gamemd `g_MapGenRng` (0x00ABE890). VERA's fresh
    /// fixed-map construction uses `Random__Seed(0)`, matching the verified
    /// native fresh-process state; launch `.SED` generation installs its exact
    /// post-RMG continuation. Bridge repair consumes this stream; destruction
    /// remains Scenario-owned. This cursor is not saved or checksummed; Rust
    /// retains it across in-scenario restore and fresh Scenario handoff.
    /// Original Init_Random_Number_System52FC20 leaves this entire object
    /// unchanged; only random-map generation reseeds it at598996.
    #[serde(skip, default = "deserialized_process_rng_placeholder")]
    pub mapgen_rng: SimRng,
    /// Independent wrapping `AbstractClass+0x10` identity cursor, Scenario+214.
    /// Native numeric IDs may duplicate and are neither stable handles nor RNG.
    /// Original689310/689470 preserve the cursor across save/load, including
    ///683560's post-read Scenario reinitialization. See native_id_snapshot.
    /// Runtime constructors consume this continuation before class admission.
    pub native_unique_ids: Option<crate::sim::native_identity::NativeUniqueIdCursor>,
    /// `MapClass+0x134` (`0x0087F91C`) analogue: the wrapping signed total that
    /// authored `ScenarioClass::Full_Init @ 0x00686B20` stores from
    /// `InitCellAttributes(0)`'s value-only `Get_Tiberium_Value` pass. No active
    /// reader is proved; cell-array teardown (a new Simulation) resets it and
    /// generated loads leave it `None`.
    #[serde(skip, default)]
    pub(crate) authored_tiberium_value_total: Option<i32>,
    /// `DAT_00A8ED78` analogue for this fresh load: the GasCloudSys
    /// `ParticleSystemClass` has been constructed by a post-load setup
    /// (`FUN_00684C30`) since `Clear_Scene` nulled it. A generated launch
    /// constructs it inside the synthetic `Full_Init`'s setup before any
    /// generator constructor; the later post-`Post_Map_Init` setup then skips.
    #[serde(skip, default)]
    pub(crate) post_load_particle_system_constructed: bool,
    /// Successful raw `[Tubes]` constructor bindings from this fresh map read.
    /// Kept separate from resolved topology so its later owning transaction
    /// can consume the already-assigned IDs without recounting filtered facts.
    #[serde(skip, default)]
    pub(crate) native_map_tubes: crate::map::tubes::NativeMapTubesState,
    /// Shared authored/runtime OverlayClass registry/deferred-delete owner. These
    /// records survive saves so global deferred tickets keep their registry
    /// identities. Their queue order is hashed by the shared deletion owner.
    pub(crate) load_objects: LoadObjectLifecycle,
    /// Deterministic fog/shroud visibility state.
    pub fog: FogState,
    /// Each house's ally mask (`HouseClass+0x5788`) as a directed graph,
    /// written by `sim::house_diplomacy`: the scenario's install, then
    /// MakeAlly and MakeEnemy.
    pub house_alliances: HouseAllianceMap,
    /// Object substrate — the active-object order plus the monotonic id and
    /// enter-order counters. The single owner the lifecycle contract
    /// (reveal/conceal/unlimbo/uninit) mutates; entity storage and the
    /// occupancy grid migrate here in later stages.
    pub substrate: ObjectSubstrate,
    /// Ordered release-visible handoffs produced by lifecycle transactions.
    /// The app drains these without feeding them back into simulation.
    #[serde(skip)]
    pub(crate) lifecycle_outputs: Vec<LifecycleOutput>,
    /// Derived presentation interest for the next admitted frame. The app
    /// copies only pending voice owners; this is never a second voice latch.
    /// `advance_app_frame` clears it on success and error, and restore starts
    /// empty. Neither snapshots nor world hashes include this transient set.
    #[serde(skip)]
    unit_voice_visit_interest: std::collections::BTreeSet<u64>,
    /// Conversion receipts drained into the same tick's navigation/app outputs.
    /// Derived frame output, not gameplay state; saves occur at frame boundaries.
    #[serde(skip)]
    pub(crate) mission_spawned_entities: bool,
    /// Ordered occupied-cell identities finalized at the authoritative frame
    /// boundary. The app drains these into its render-only overlay list.
    #[serde(skip)]
    frame_overlay_updates: Vec<OverlayEntry>,
    /// Ordered coordinates whose overlay identity was erased at the
    /// authoritative frame boundary. Drained into `SimFrameOutput` beside the
    /// occupied upserts.
    #[serde(skip)]
    frame_overlay_removals: Vec<(u16, u16)>,
    /// One-shot raw post-match score result. Serialized and hashed so a save at
    /// the outcome wait cannot repeat its Scenario RNG draws after load.
    pub(super) terminal_score_snapshot: Option<crate::sim::score::TerminalScoreSnapshot>,
    /// Reusable movement-to-world lifecycle request buffer. Requests are applied
    /// immediately after the movement call returns.
    #[serde(skip)]
    pub(crate) pending_lifecycle_requests: Vec<LifecycleRequest>,
    /// Aircraft strike requests retained by the existing aircraft receiver.
    /// Building, Unit and Infantry FireAt commit within their live slots.
    #[serde(skip)]
    pub(crate) fire_requests: crate::sim::combat::FireRequests,
    /// BulletClass AI results produced in mixed Logic order and consumed at
    /// the existing combat receiver seam later in this master frame.
    #[serde(skip)]
    pub(crate) pending_projectile_detonations: Vec<crate::sim::projectile::ProjectileDetonation>,
    /// WaveClass AI damage requests produced in mixed Logic order and consumed
    /// at the established wave-damage receiver seam later in this frame.
    #[serde(skip)]
    pub(crate) pending_wave_damage_requests: Vec<crate::sim::wave::WaveDamageRequest>,
    /// Internal order proof; release builds carry no ledger or recording branch.
    #[cfg(any(test, feature = "test-support"))]
    #[serde(skip)]
    lifecycle_test_events: Vec<LifecycleTestEvent>,
    #[cfg(test)]
    #[serde(skip)]
    pub(crate) receiver_fixture: Option<crate::sim::combat::receiver_fixture::FixturePolicy>,
    /// App-visible outcomes produced by the authoritative trigger rung.
    /// Trigger actions mutate `trigger_runtime` during the frame; only their
    /// presentation outcomes are moved into `SimFrameOutput` after the tick.
    #[serde(skip)]
    trigger_effects: Vec<TriggerEffect>,
    #[cfg(any(test, feature = "test-support"))]
    #[serde(skip)]
    master_frame_test_trace: Vec<MasterFrameTestRung>,
    /// Focused ordering observer for the bounded House-update activation seam.
    #[cfg(any(test, feature = "test-support"))]
    #[serde(skip)]
    house_ai_activation_order_test_trace: Vec<HouseAiActivationOrderTestEvent>,
    /// One-shot fixture hook proving that the House loop reloads live length.
    #[cfg(any(test, feature = "test-support"))]
    #[serde(skip)]
    house_update_append_after_test: Option<(InternedId, InternedId)>,
    /// Sound events produced during the current tick and moved into the owned
    /// app-frame output batch.
    #[serde(skip)]
    pub sound_events: Vec<SimSoundEvent>,
    #[serde(skip)]
    pub lighting_sources: crate::sim::light_sources::LightingSources,
    /// Fire events produced during combat and moved into the app-frame output
    /// for muzzle flash rendering and future projectile origin computation.
    #[serde(skip)]
    pub fire_events: Vec<SimFireEvent>,
    /// Native IC/ForceShield impact combat-light requests emitted in receiver
    /// order during the current master frame. The native object has no owner
    /// and is distinct from AnimClass/ParticleSystemClass, so this remains a
    /// dedicated presentation handoff rather than fabricated world state.
    #[serde(skip)]
    pub(crate) combat_light_requests: Vec<crate::sim::combat::CombatLightRequest>,
    /// Persistent ordinary shots. This is authoritative save/hash state, not
    /// the render-side fire-event approximation.
    #[serde(default)]
    pub projectiles: crate::sim::projectile::ProjectileStore,
    /// Persistent WaveClass registrations. They have their own logic lifetime;
    /// this is not a one-frame weapon-fire presentation list.
    pub waves: crate::sim::wave::WaveStore,
    /// Stable form of TechnoClass +0x324: owner stable id -> exact Wave stable
    /// id. The native link is installed even for a constructor-short Wave and
    /// is cleared only by exact pointer-expiry cleanup.
    #[serde(default)]
    pub(crate) active_wave_links: BTreeMap<u64, u64>,
    /// `ObjectClass+0x64`: the custom sound trigger action 99 gave an object
    /// (a Building or Terrain), keyed by stable id. Absent is the
    /// constructor's -1. Only `object_sound` writes it; the object's
    /// destruction removes it.
    #[serde(default)]
    pub(crate) object_custom_sounds: BTreeMap<u64, String>,
    /// Hookless-test adapter retained for old fixture callsites. Production
    /// combat and superweapons commit smudges inline, so this remains empty and
    /// never persists across ticks.
    #[serde(skip)]
    pub(crate) pending_smudge_requests: Vec<crate::sim::combat::SmudgeSpawnRequest>,
    /// Bale deposit events emitted during refinery dock unloading and consumed
    /// by the authoritative frame tail for SpecialAnim and particle creation.
    #[serde(skip)]
    pub(crate) bale_events: Vec<crate::sim::components::BaleDepositEvent>,
    /// Tank-bunker wall-anim events — walls rising on install / falling on
    /// teardown. Consumed by the authoritative frame tail before hashing.
    #[serde(skip)]
    pub(crate) bunker_wall_events: Vec<crate::sim::components::BunkerWallAnimEvent>,
    /// Resolved TeamClass/ScriptType runtime; scenario INI parsing remains a
    /// separate refused boundary until its record grammar is evidenced.
    pub team_script_vm: TeamScriptVm,
    /// Per-player state keyed by uppercase owner name. Deterministic iteration
    /// via BTreeMap. Equivalent to the original engine's HouseClass array.
    pub houses: BTreeMap<InternedId, HouseState>,
    /// Derived movement inputs reused across object turns (blocker plane).
    /// Rebuilt from state on demand; never serialized or hashed.
    #[serde(skip)]
    pub(crate) movement_pass_cache: crate::sim::movement::movement_tick::MovementPassCache,
    /// Which listeners each pointer expiry reaches (`expiry_listeners`).
    /// Derived from the listener stores; never serialized or hashed.
    #[serde(skip)]
    expiry_listeners: expiry_listeners::ExpiryListeners,
    /// Batched path searches (`movement::path_batch`); `None`
    /// searches in native order. Its registrations wait across a save.
    pub(crate) path_batch: Option<crate::sim::movement::path_batch::PathBatch>,
    /// Batched target scans (`techno_ai::scan_batch`); `None` scans in
    /// native order. A save carries only the setting.
    pub(crate) scan_batch: Option<techno_ai::ScanBatch>,
    /// Retained native navigation base plane/raw movement rows plus the live
    /// derived hierarchy. ZoneGrid saves only its own retained base state;
    /// LoadContent67E8CD rebuilds hierarchy after bound Cell/object restoration.
    pub zone_grid: Option<ZoneGrid>,
    /// Canonical dynamic navigation projection. Every reader reads it at its
    /// point of use; a republish replaces the `Arc` without mutating readers'
    /// shared copies.
    #[serde(skip)]
    pub(crate) path_grid: Option<Arc<PathGrid>>,
    /// Derived cache: the marked-structure movement cells last published into
    /// `path_grid`. Source of truth is the marked structures themselves
    /// (`navigation::structure_movement_cells`). A full navigation rebuild
    /// resets it; synchronous Mark/Recalc publication updates affected cells.
    /// Frame-end structure publication reconciles the union of this set and
    /// the current one, skipping cells already current.
    #[serde(skip)]
    structure_navigation_cells: BTreeSet<(u16, u16)>,
    #[serde(skip)]
    pub resolved_terrain: Option<ResolvedTerrainGrid>,
    /// Process-global MapClass fallback CellClass identity. Native owns this at
    /// `0x00ABDC50`, outside Scenario serialization; live in-scenario loads
    /// retain the current handle and rebuilt terrain is rebound to it.
    #[serde(skip, default)]
    pub(crate) shared_cell_dummy: SharedCellDummy,
    /// Shared Map+11C/+120 traversal for synchronous RevealZone578850/578710.
    /// Nested discovery actions overwrite this same cursor; never restore an
    /// outer cursor. Every exposed traversal resets before consuming it, and
    /// no action stack survives a save, so this private scratch is not saved
    /// or hashed. Terrain/query/input clones do not share its mutable state.
    #[serde(skip)]
    native_local_cell_cursor: Option<crate::map::resolved_terrain::NativeLocalCellCursor>,
    #[serde(skip)]
    native_allocated_cell_cursor: Option<crate::map::resolved_terrain::NativeAllocatedCellCursor>,
    /// Serialized value authority for allocated real CellClass bridge bits.
    /// The derived terrain grid is skipped; load installs its pristine map
    /// template and then writes these saved values directly back onto real
    /// cells. This is value state, never a setter/replay log.
    #[serde(default)]
    pub real_cell_bridge_flags_0x1180: RealCellBridgeFlags0x1180,
    /// Exact value projection of runtime isometric-tile replacement. The
    /// parsed terrain grid is derived/skipped and receives these values before
    /// overlay/bridge/navigation reconstruction on restore.
    #[serde(default)]
    pub dynamic_terrain_cells:
        BTreeMap<(u16, u16), crate::map::resolved_terrain::DynamicTerrainCellState>,
    pub bridge_state: Option<BridgeRuntimeState>,
    /// Resident Map+D4 ordered collapse coordinates. Independent of optional
    /// geometry so even standalone fallout/save retains its native publication.
    /// The bridge damage controller owns its mutations and synchronous drain.
    pending_bridge_collapses: bridge_orchestrator::PendingBridgeCells,
    /// Per-cell mutable overlay state (ore density, wall damage, bridge frames).
    /// Seeded from map [OverlayPack] at init, mutated during gameplay.
    pub overlay_grid: Option<crate::sim::overlay_grid::OverlayGrid>,
    /// Persistent MapClass scenario-crate slots, including accepted ghosts and
    /// their native timer words. This is authoritative save/hash state.
    pub crate_authority: crate::sim::crates::CrateAuthority,
    /// Per-cell smudge state (craters, scorches). Seeded from map [Smudge]
    /// entries at init, mutated by combat death-handling at runtime.
    pub smudge_grid: Option<crate::sim::smudge_grid::SmudgeGrid>,
    /// Per-cell radiation field + site registry. Detonations of RadLevel>0
    /// weapons feed it during the combat phase; sites decay in their own
    /// post-combat step; foot units take periodic damage from their cell.
    #[serde(default)]
    pub radiation: crate::sim::radiation::RadiationState,
    /// `BombListClass`: the carriers of live Crazy Ivan bombs (each record
    /// sits on its carrier) and the BombVisible countdown, owned by `bomb`.
    /// Not saved: a load clears it and rebuilds the carriers.
    #[serde(skip)]
    pub(crate) bombs: crate::sim::bomb::BombList,
    /// The kamikaze tracker (`0x00ABC5F8`): missiles out of their launcher's
    /// control and the cells they fly at, owned by `kamikaze`. Its timer is
    /// not saved; a load restarts it.
    pub(crate) kamikaze: crate::sim::kamikaze::KamikazeTracker,
    /// DiskLaser Save4A7C10 retains delayed shots; emitted LaserDraws remain
    /// copied presentation facts. This owner has no Logic/Display membership.
    disk_lasers: disk_laser::DiskLasers,
    /// The map's isometric playfield diamond ([Map] Size width + the raw
    /// LocalSize rect), set at map init. Threaded into the cell-rect occupancy
    /// validator's final playfield-corner test (the engine diamond, not a
    /// rectangle). `None` only in headless tests with no map loaded.
    #[serde(default)]
    pub playfield_bounds: Option<crate::sim::cell_rect::PlayfieldBounds>,
    /// Immutable signed `[Map] Size=` height retained for later LocalSize
    /// normalization. The live predicate needs only Size width, but action 40
    /// can rewrite the four LocalSize dwords repeatedly during a scenario.
    #[serde(default)]
    pub playfield_size_height: Option<i32>,
    /// Monotonic visible-area writer generation. Every successful trigger
    /// action 0x28 advances it, even when two writers normalize to the same
    /// bounds. Presentation consumes this as the global radar/scroll rebuild
    /// edge; cell-local bridge dirtiness is deliberately a separate channel.
    #[serde(default)]
    pub playfield_revision: u64,
    /// Interned projection of RuleSet's ordered BridgeExplosions references.
    /// Rebuilt at map binding and snapshot restoration; not serialized authority.
    #[serde(skip)]
    pub bridge_explosions: Vec<InternedId>,
    /// Interned projection of RuleSet's ordered MetallicDebris references,
    /// including unread types without an SHP. Rebuilt at map binding and
    /// snapshot restoration so per-cell selection needs no string interning.
    #[serde(skip)]
    pub metallic_debris: Vec<InternedId>,
    /// Runtime terrain cells whose radar/minimap terrain pixel needs refresh.
    /// Presentation reads this generation and acknowledges the exact batch
    /// only after its radar update completes. The list is de-duplicated within
    /// that pending update window, then cleared so the same cell can re-arm.
    #[serde(skip)]
    pub radar_terrain_dirty_cells: Vec<(u16, u16)>,
    #[serde(skip)]
    pub radar_terrain_dirty_generation: u64,
    /// Derived index of `radar_terrain_dirty_cells` for
    /// [`Self::mark_radar_terrain_dirty_cells`], with the list length and
    /// generation it was built for. Every other writer pushes (changing the
    /// length) and bumps the generation it is given
    /// ([`mark_wall_radar_dirty_cell`]); the only clear empties the list. A
    /// mismatch of either rebuilds the index from the list.
    #[serde(skip)]
    radar_terrain_dirty_index: (BTreeSet<(u16, u16)>, usize, u64),
    /// Runtime cell rects dirtied by tiberium mutation side effects.
    #[serde(skip)]
    pub(crate) tactical_dirty_cells: Vec<(u16, u16)>,
    /// Per-player power state (output, drain, low-power flag, spy blackout timer).
    /// Updated each tick by `power_system::tick_power_states()`.
    pub power_states: BTreeMap<InternedId, PowerState>,
    /// Per-owner superweapon instances. Outer key = owner, inner key = SW type ID.
    /// Deterministic iteration via nested BTreeMap.
    pub super_weapons:
        BTreeMap<InternedId, BTreeMap<InternedId, crate::sim::superweapon::SuperWeaponInstance>>,
    /// The Lightning Storm's globals (one at a time).
    pub lightning_storm: crate::sim::superweapon::lightning_storm::LightningStorm,
    /// The Psychic Dominator's globals (one at a time).
    pub psychic_dominator: crate::sim::superweapon::psychic_dominator::PsychicDominatorState,
    /// IonBlastClass's vector (`0x00AA0118`), owned by `ion_blast`. Native
    /// saves no blast.
    #[serde(skip)]
    ion_blasts: Vec<ion_blast::IonBlast>,
    /// Per-cell terrain speed modifier config (slope climb/descend).
    /// Built from [General] rules at map load.
    #[serde(skip)]
    pub terrain_speed_config: terrain_speed::TerrainSpeedConfig,
    /// When true, newly spawned entities get a `DebugEventLog` allocated.
    /// Toggled by the debug inspector hotkey (X). Debug-only — not included in state hashing.
    #[serde(skip)]
    pub debug_event_logging: bool,
    /// Negotiated lockstep ahead window. Offline producers stamp the current
    /// raw issue ordinal; a network transfer owner overwrites that stamp with
    /// `send_current + MaxAhead` before synchronized dispatch.
    pub input_delay_ticks: u64,
    /// Native Main_Tick termination gates outside House state. These are
    /// front-end connection/session facts, so they are transient and un-hashed.
    #[serde(skip)]
    pub quit_requested: bool,
    /// One-shot owner-tagged edge raised only when a due native EXIT command
    /// executes at the EventClass tail. App teardown consumes and clears it.
    #[serde(skip)]
    executed_exit_owner: Option<InternedId>,
    #[serde(skip)]
    pub(crate) connection_lost: bool,
    /// GameMode's `Queue_AI` route. The app's session owner is the authority
    /// and writes it before every frame it admits; never saved or hashed.
    #[serde(skip)]
    command_queue_route: CommandQueueRoute,
    /// Checksum computed by this frame's multiplayer tail, handed to the
    /// frame output. Transient per frame.
    #[serde(skip)]
    frame_sync_checksum: Option<FrameSyncChecksum>,
    /// VERA-only: which frames hash their state for the TickResult, by tick
    /// number; `None` for none. A launch or a loaded save hashes every frame
    /// unless the caller asks for fewer; normal play's frame loop asks for
    /// every [`crate::sim::replay::PLAY_HASH_INTERVAL`]th. Test and dev worlds
    /// ([`Simulation::with_seed`]) hash only when asked. A frame that ends the
    /// match hashes regardless. Never saved or hashed.
    #[serde(skip, default = "every_frame")]
    state_hash_interval: Option<std::num::NonZeroU64>,
    /// Pending gameplay commands waiting for their scheduled execution tick.
    /// Admitted through `queue_command(s)` and drained each tick when
    /// `cmd.execute_tick <= current_tick + 1`.
    pending_commands: Vec<CommandEnvelope>,
    /// PlanMgr is the sole owner of shared nodes and per-actor plan tokens.
    planning: planning::PlanningState,
    /// Diagnostic playback supplies a batch already containing generated
    /// events. Transient admission context, not gameplay or saved state.
    #[cfg(any(test, feature = "test-support"))]
    #[serde(skip)]
    replaying_recorded_commands: bool,
    /// Serialized map trigger state, initialized and mutated by trigger_runtime.
    /// Remains installed while actions call other Simulation mechanisms.
    pub(crate) trigger_runtime: TriggerRuntime,
}

#[cfg(any(test, feature = "test-support"))]
impl Default for Simulation {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn mark_wall_radar_dirty_cell(
    cells: &mut Vec<(u16, u16)>,
    generation: &mut u64,
    cell: (u16, u16),
) {
    if !cells.contains(&cell) {
        cells.push(cell);
        *generation = (*generation).wrapping_add(1);
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
    radar_dirty_cells: &mut Vec<(u16, u16)>,
    radar_dirty_generation: &mut u64,
    tactical_dirty_cells: &mut Vec<(u16, u16)>,
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
        radar_dirty_cells: Some(radar_dirty_cells),
        radar_dirty_generation: Some(radar_dirty_generation),
        tactical_dirty_cells: Some(tactical_dirty_cells),
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
/// tiberium prelude and synchronous wall-navigation callbacks.
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
    radar_dirty_cells: &'a mut Vec<(u16, u16)>,
    radar_dirty_generation: &'a mut u64,
    tactical_dirty_cells: &'a mut Vec<(u16, u16)>,
    zone_grid: &'a mut Option<ZoneGrid>,
    path_grid: &'a mut Option<Arc<PathGrid>>,
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
            self.radar_dirty_cells,
            self.radar_dirty_generation,
            self.tactical_dirty_cells,
        );
    }

    fn wall_dirty_step(&mut self, step: WallDirtyStep, packed_coord: (u16, u16)) {
        match step {
            WallDirtyStep::Tactical => self.tactical_dirty_cells.push(packed_coord),
            WallDirtyStep::Radar => mark_wall_radar_dirty_cell(
                self.radar_dirty_cells,
                self.radar_dirty_generation,
                packed_coord,
            ),
        }
    }

    fn wall_navigation_step(
        &mut self,
        terrain: &ResolvedTerrainGrid,
        cell: (u16, u16),
        navigation_changed: bool,
        repair: ZoneRepairKind,
    ) {
        repair_wall_damage_navigation_authorities(
            self.zone_grid,
            self.path_grid,
            terrain,
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
    radar_dirty_cells: &'a mut Vec<(u16, u16)>,
    radar_dirty_generation: &'a mut u64,
    tactical_dirty_cells: &'a mut Vec<(u16, u16)>,
    zone_grid: &'a mut Option<ZoneGrid>,
    path_grid: &'a mut Option<Arc<PathGrid>>,
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
        radar_dirty_cells,
        radar_dirty_generation,
        tactical_dirty_cells,
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
            let refreshed = terrain
                .cell(cell.0, cell.1)
                .is_some_and(|resolved| Arc::make_mut(grid).refresh_resolved_cell(resolved, false));
            debug_assert!(refreshed, "wall Recalc cell must be inside the map");
        }
        Some(_) => {}
        None => {
            *path_grid = Some(Arc::new(PathGrid::from_resolved_terrain_with_bridges(
                terrain,
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
        *zone_grid = Some(ZoneGrid::build_with_native_map_context(
            path_grid,
            terrain,
            bridge_records,
            bridge_geometry,
            playfield_bounds,
        ));
    }
}

/// Borrow-split wall observer used by runtime placement and sale, where the
/// overlay/terrain transaction remains in its owning module but the live
/// presentation, navigation, and represented Cell-target authorities are
/// already available as disjoint Simulation fields.
pub(crate) struct SimulationWallRuntimeHost<'a> {
    pub(crate) entities: &'a mut EntityStore,
    pub(crate) target_terrain_objects:
        &'a BTreeMap<u64, crate::sim::terrain_object::TerrainObjectState>,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) detach_trace: &'a mut Vec<crate::sim::combat::combat_aoe::CellTargetDetach>,
    pub(crate) radar_dirty_cells: &'a mut Vec<(u16, u16)>,
    pub(crate) radar_dirty_generation: &'a mut u64,
    pub(crate) tactical_dirty_cells: &'a mut Vec<(u16, u16)>,
    pub(crate) zone_grid: &'a mut Option<ZoneGrid>,
    pub(crate) path_grid: &'a mut Option<Arc<PathGrid>>,
    pub(crate) bridge_state: Option<&'a BridgeRuntimeState>,
    pub(crate) playfield_bounds: Option<PlayfieldBounds>,
}

impl WallDamageTransactionHost for SimulationWallRuntimeHost<'_> {
    fn dirty_step(&mut self, step: WallDirtyStep, packed_coord: (u16, u16)) {
        match step {
            WallDirtyStep::Tactical => self.tactical_dirty_cells.push(packed_coord),
            WallDirtyStep::Radar => mark_wall_radar_dirty_cell(
                self.radar_dirty_cells,
                self.radar_dirty_generation,
                packed_coord,
            ),
        }
    }

    fn navigation_step(
        &mut self,
        terrain: &ResolvedTerrainGrid,
        cell: (u16, u16),
        navigation_changed: bool,
        repair: ZoneRepairKind,
    ) {
        repair_wall_damage_navigation_authorities(
            self.zone_grid,
            self.path_grid,
            terrain,
            self.bridge_state,
            self.playfield_bounds,
            cell,
            navigation_changed,
            repair,
        );
    }

    fn pointer_expired(&mut self, target: WallPointerTarget) {
        if let WallPointerTarget::Real(rx, ry) = target {
            crate::sim::combat::combat_aoe::expire_cell_target_references(
                self.entities,
                self.target_terrain_objects,
                rx,
                ry,
                #[cfg(any(test, feature = "test-support"))]
                self.detach_trace,
            );
        }
    }
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
    radar_dirty_cells: &mut Vec<(u16, u16)>,
    radar_dirty_generation: &mut u64,
    tactical_dirty_cells: &mut Vec<(u16, u16)>,
    construction: &mut crate::sim::smudge_grid::SmudgeConstruction<'_>,
) {
    let (Some(smudge_grid), Some(overlay_grid), Some(terrain)) =
        (smudge_grid, overlay_grid, terrain)
    else {
        return;
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
        radar_dirty_cells,
        radar_dirty_generation,
        tactical_dirty_cells,
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
    );
}

/// Existing process RNG owner lent to a detached saved-world candidate.
pub enum SnapshotProcessState<'a> {
    Live(&'a Simulation),
    Cold(&'a MainRng),
}

impl Simulation {
    /// The live CellClass level of a cell in the terrain the simulation owns,
    /// or `None` without terrain or off the map. Bridge-body and cliff writes
    /// update it, so readers never see a load-time copy.
    pub(crate) fn terrain_cell_level(&self, rx: u16, ry: u16) -> Option<u8> {
        Some(self.resolved_terrain.as_ref()?.cell(rx, ry)?.level)
    }

    /// Resolve the CellClass identity returned by MapClass::Get_CellClass and
    /// then dispatch its live GetTargetCoords virtual. Fixed-stride aliases
    /// therefore use the returned real CellClass's canonical coordinate, while
    /// misses retain and restamp the one process-global dummy identity.
    fn wave_cell_target_position(&self, rx: u16, ry: u16) -> ProjectileCoord {
        self.wave_cell_target_position_in(self.resolved_terrain.as_ref(), rx, ry)
    }

    fn wave_cell_target_position_in(
        &self,
        terrain: Option<&ResolvedTerrainGrid>,
        rx: u16,
        ry: u16,
    ) -> ProjectileCoord {
        use crate::sim::cell_rect::{CellRef, get_cellclass_fallback};

        match get_cellclass_fallback(terrain, i32::from(rx), i32::from(ry)) {
            CellRef::Real(cell) => {
                crate::sim::projectile::cell_target_coord(terrain, cell.rx, cell.ry)
            }
            CellRef::Dummy { cell } => {
                let live_dummy = if terrain.is_some() {
                    cell
                } else {
                    // A mapless lookup still addresses Simulation's retained
                    // process dummy. The fallback supplies native i16 packing;
                    // restamp only its coordinate so live level/slope/bridge
                    // bytes remain authoritative.
                    let coord = cell.snapshot().coord;
                    let live_dummy = self.effective_shared_cell_dummy();
                    live_dummy.stamp_coord(coord.0, coord.1);
                    live_dummy
                };
                crate::sim::projectile::dummy_cell_target_coord(&live_dummy)
            }
        }
    }

    pub fn apply_fatal_lifecycle_stage(
        &mut self,
        rules: &RuleSet,
        stage: crate::sim::combat::FatalLifecycleStage,
        stable_id: u64,
        category: EntityCategory,
        uninit_context: UninitContext<'_>,
    ) {
        match stage {
            crate::sim::combat::FatalLifecycleStage::MaintainDamageSmoke { state } => {
                self.maintain_damage_smoke_after_receive(stable_id, state, rules);
            }
            crate::sim::combat::FatalLifecycleStage::BeforeDeathEffects { attacker } => {
                // A building's occupants. A unit's passengers stay for its own
                // arms: an exploding unit kills them before its death weapon
                // (`world_receiver::handle_death`), and the rest leave or die
                // in its UnitClass arm
                // (`crew_survival::release_dying_unit_passengers`).
                //
                // RESIDUAL: this runs before the Techno death arm (sounds,
                // debris, death weapon). Natively that arm returns first, and
                // the Building NowDead block then ejects the occupants
                // (`0x00442625`) and turns the light off (`0x0044264C`) just
                // before DestructionEffects (`0x00442665`), so an ejected
                // garrison's Scatter draws follow the debris and death-weapon
                // draws there and precede them here.
                if category != EntityCategory::Structure {
                    return;
                }
                let garrison = self.substrate.entities.get(stable_id).and_then(|entity| {
                    let object = self.interner.object_type(entity.type_ref(), rules)?;
                    let passenger_ids = entity
                        .passenger_role
                        .cargo()
                        .map(|cargo| cargo.passengers().to_vec())
                        .unwrap_or_default();
                    if !object.can_be_occupied || passenger_ids.is_empty() {
                        return None;
                    }
                    let (foundation_w, foundation_h) =
                        crate::rules::foundation::foundation_dimensions(&object.foundation);
                    Some(
                        crate::sim::passenger::garrison_eject::DestroyedGarrisonBuilding {
                            building_id: stable_id,
                            rx: entity.position.rx(),
                            ry: entity.position.ry(),
                            z: entity.position.z(),
                            foundation_w,
                            foundation_h,
                            passenger_ids,
                        },
                    )
                });
                // An absorbing building's passengers leave through
                // SpawnSurvivors' Phase A; its KillPassengers (`0x00441F27`,
                // crediting the attacker) runs after that and finds the list
                // empty.
                let absorbs = self
                    .substrate
                    .entities
                    .get(stable_id)
                    .is_some_and(|entity| {
                        self.interner
                            .object_type(entity.type_ref(), rules)
                            .is_some_and(|object| object.absorbs_passengers())
                    });
                if let Some(event) = garrison {
                    passenger::garrison_eject::eject_destruction_garrison_with_context(
                        self,
                        rules,
                        &event,
                        uninit_context,
                    );
                } else if !absorbs {
                    self.kill_passengers(stable_id, attacker, uninit_context);
                }
                //44264C precedes the recursive destruction effects.
                self.set_building_light_active(stable_id, false);
            }
            crate::sim::combat::FatalLifecycleStage::AfterDeathEffects => {
                if !matches!(category, EntityCategory::Unit | EntityCategory::Structure) {
                    return;
                }
                // Unit's receiver retains a `Crashable=` type its Crash(0)
                // latched (`0x00738475`) and any other type's sinking hull
                // (`0x00738493`); their own AI terminal calls UnInit. A
                // Magnetron's drop raises the crash latch too, and keeps nothing.
                if self
                    .substrate
                    .entities
                    .get(stable_id)
                    .is_some_and(|entity| {
                        if self
                            .object_type(entity.type_ref(), rules)
                            .is_some_and(|object| object.crashable)
                        {
                            entity.crash.is_crashing()
                        } else {
                            entity.sinking.is_active()
                        }
                    })
                {
                    return;
                }
                if category == EntityCategory::Structure
                    && self
                        .substrate
                        .entities
                        .get(stable_id)
                        .and_then(|building| building.bunker.occupant())
                        .is_some()
                {
                    crate::sim::docking::bunker_link::release_sell_destroy(
                        self,
                        stable_id,
                        uninit_context.effects(),
                    );
                }
                // `BuildingClass::AfterDestruction @ 0x00441F60` follows the
                // dying building's UnInit and reads its type and Location.
                let destroyed_building = (category == EntityCategory::Structure)
                    .then(|| self.substrate.entities.get(stable_id))
                    .flatten()
                    .map(|building| {
                        (
                            building.type_ref(),
                            (building.position.rx(), building.position.ry()),
                        )
                    });
                self.uninit_with_context(stable_id, uninit_context);
                if let Some((type_id, origin)) = destroyed_building {
                    self.building_after_destruction(
                        rules,
                        uninit_context.registry(),
                        type_id,
                        origin,
                        uninit_context.effects(),
                    );
                }
            }
        }
    }

    fn tick_combat_with_fatal_lifecycle(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        tick_ms: u32,
        logic_order: &[u64],
        fire_suppressed: &BTreeSet<u64>,
        fire_requests: &crate::sim::combat::FireRequests,
        projectile_detonations: &[crate::sim::projectile::ProjectileDetonation],
        frame_effects: FrameEffects<'_>,
    ) -> crate::sim::combat::CombatTickResult {
        let mut run =
            crate::sim::combat::world_receiver::ReceiverRun::default().with_effects(frame_effects);
        let mut result = crate::sim::combat::world_fire::tick_combat(
            self,
            &mut run,
            rules,
            overlay_registry,
            tick_ms,
            logic_order,
            fire_suppressed,
            fire_requests,
            projectile_detonations,
        );
        result.consequences.finish_navigation(run.finish());
        result
    }

    /// Live class and mission FireAt deliveries finish before the next
    /// dynamic Logic object slot. The caller retains frame invalidations.
    pub fn commit_fire_visit(
        &mut self,
        visit: crate::sim::combat::world_fire::FireVisit,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> damage_consequences::DamageCommitReceipt {
        #[cfg(test)]
        if let crate::sim::combat::world_fire::FireVisit::Building { id, shot } = &visit
            && crate::sim::combat::receiver_fixture::substitute_building_fire(
                self, *id, *shot, rules,
            )
        {
            return damage_consequences::DamageCommitReceipt {
                area_result: None,
                structure_destroyed: false,
                bridge_state_changed: false,
            };
        }
        #[cfg(test)]
        let observed_direct = match &visit {
            crate::sim::combat::world_fire::FireVisit::Direct { id, .. } => Some(*id),
            crate::sim::combat::world_fire::FireVisit::Building { id, .. } => Some(*id),
            _ => None,
        };
        #[cfg(test)]
        if let Some(id) = observed_direct {
            crate::sim::combat::receiver_fixture::observe_fire_visit(self, id, "entry");
        }
        let mut run =
            crate::sim::combat::world_receiver::ReceiverRun::default().with_effects(frame_effects);
        let mut emit = crate::sim::combat::CombatEmit::default();
        let mut pings = Vec::new();
        crate::sim::combat::world_fire::visit_fire(
            self,
            &mut run,
            visit,
            rules,
            overlay_registry,
            &mut emit,
            &mut pings,
        );
        let crate::sim::combat::CombatEmit {
            effects,
            fire_events,
            // The receiver already committed these inline before returning.
            damage_events: _,
            remove_attack: _,
            unit_facing: _,
            spawn_target_updates: _,
            drain_links: _,
        } = emit;
        let receipt = damage_consequences::DamageConsequences::live_fire(
            effects,
            pings,
            run.finish(),
            fire_events,
        )
        .commit(self, rules, overlay_registry, frame_effects);
        #[cfg(test)]
        if let Some(id) = observed_direct {
            crate::sim::combat::receiver_fixture::observe_fire_visit(self, id, "return");
        }
        receipt
    }

    /// Commit a completed Bullet's detonation while its current Logic slot and
    /// physical object are still present. The caller retires the Bullet only
    /// after this transaction returns.
    fn commit_logic_projectile_detonations(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        detonations: &[crate::sim::projectile::ProjectileDetonation],
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        if detonations.is_empty() {
            return false;
        }
        let mut run =
            crate::sim::combat::world_receiver::ReceiverRun::default().with_effects(frame_effects);
        let commit = crate::sim::combat::world_receiver::commit_projectiles(
            self,
            &mut run,
            detonations,
            rules,
            overlay_registry,
        );
        let terrain_navigation_changed_cells = run.finish();

        #[cfg(test)]
        if let Some(fixture) = self.receiver_fixture.as_mut() {
            let changed = commit.effects.bridge_state_changed;
            fixture
                .tail_effects
                .push((commit.effects, commit.under_attack_events));
            return changed;
        }
        self.absorb_noncombat_damage_effects(
            rules,
            overlay_registry,
            commit.effects,
            commit.under_attack_events,
            terrain_navigation_changed_cells,
            frame_effects,
        )
        .bridge_state_changed
    }

    pub(crate) fn commit_fired_wave(
        &mut self,
        rules: &RuleSet,
        event: &SimFireEvent,
        wave_type: u8,
    ) {
        if let Some(wave) = self.prepare_fired_wave(rules, event, wave_type) {
            self.admit_fired_wave(event.attacker_id, wave, rules);
        }
    }

    /// Read the firing transaction directly; construction never installs a second
    /// entity, map, house or RNG authority into the world.
    fn prepare_fired_wave(
        &self,
        rules: &RuleSet,
        event: &SimFireEvent,
        wave_type: u8,
    ) -> Option<crate::sim::wave::Wave> {
        debug_assert!(matches!(wave_type, 0 | 3));
        let target = match event.target {
            // FireAt aims at the target's vt+0xA4 (`0x006FE1EE`), including
            // a Building's TargetCoordOffset. Wave AI later reads +58 instead.
            crate::sim::combat::TargetKind::Entity(_) => {
                crate::sim::combat::fire_coord::effect_target_coordinate(
                    self,
                    rules,
                    event.target.projectile_target(self),
                )?
            }
            crate::sim::combat::TargetKind::Cell(rx, ry) => self.wave_cell_target_position(rx, ry),
        };
        // Sonic 0x006FF465 and Magnetic 0x006FF63C pass the constructor
        // &[ESP+0x88], this target coordinate (0x006FE204), first and
        // &[ESP+0x44], the launch source (`event.fire_coord`: GetFLH or a
        // Dropping projectile's GetCoords), second.
        let wave = crate::sim::wave::Wave::new_owned(
            wave_type,
            event.attacker_id,
            event.target,
            target,
            event.fire_coord,
        );
        Some(wave)
    }

    /// Commit identity, constructor cleanup and immediate Logic admission in their
    /// original order. The receiver keeps ownership of all combat/map state.
    fn admit_fired_wave(
        &mut self,
        attacker_id: u64,
        mut wave: crate::sim::wave::Wave,
        rules: &RuleSet,
    ) {
        let stable_id = self.allocate_stable_id();
        if !wave.constructor_distance_is_live() {
            // Constructor UnInit precedes registration and unique identity,
            // but FireAt stores the returned dead pointer. The stable id only
            // represents that same-frame deferred cleanup window.
            self.waves.spawn(stable_id, wave);
            self.active_wave_links.insert(attacker_id, stable_id);
            let retired = self.retire_non_entity_object(stable_id, UninitContext::default());
            debug_assert!(retired);
            return;
        }
        // 75EAA4 calls Create_ID410230 only after the short-distance abort.
        // The Scenario-owned cursor is separate from Rust's stable handle;
        // this chain has no consumer of the Wave's numeric ID itself.
        self.next_native_runtime_id();
        // Constructor75E950 runs an immediate Update_Wave762AF0, including
        // current target+58 and GetFLH(0), before the first Logic AI visit.
        let context = self.wave_update_context(&wave, Some(rules));
        let terminal = wave.initialize(context, self.resolved_terrain.as_ref());
        self.admit_wave(stable_id, wave);
        self.active_wave_links.insert(attacker_id, stable_id);
        if terminal {
            let retired = self.retire_non_entity_object(stable_id, UninitContext::default());
            debug_assert!(retired);
        }
    }

    /// Compatibility continuation for emissions from the remaining global
    /// combat hosts. Live Unit/Infantry firing uses the main object pass.
    /// FireAt appends each bullet, muzzle anim and wave to
    /// the Logic vector as it fires, and `LogicClass::PerTickUpdate` re-reads
    /// the vector's length after every AI call (`0x0055B613`), so those objects
    /// take their first AI later in the same pass, in append order; an Inviso
    /// bullet detonates on that visit. Anything they append in turn (explosion
    /// anims) follows. A detonated bullet leaves the vector
    /// (`LogicClass::Remove @ 0x0055BAE0`), which shifts the next entry into
    /// its slot, and the cursor moves past it: that entry waits a frame.
    ///
    /// RESIDUAL: a Techno created in this global-tail window (a crew survivor
    /// or spawned aircraft) still takes its first AI next frame. This tail
    /// visits only non-Techno objects; the remaining global hosts must migrate
    /// to their original live slots to close that same-frame lifecycle.
    pub fn visit_combat_tail(
        &mut self,
        first_tail_id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let mut bridge_state_changed = false;
        let mut index = 0;
        while index < self.substrate.logic.len() {
            let stable_id = self.substrate.logic.as_slice()[index];
            if stable_id >= first_tail_id && !self.substrate.entities.contains(stable_id) {
                bridge_state_changed |= self
                    .object_ai_visit_one_with_effects(
                        stable_id,
                        Some(rules),
                        techno_ai::ObjectAiCtx {
                            effects: frame_effects,
                            overlay_registry,
                            ..techno_ai::ObjectAiCtx::default()
                        },
                    )
                    .bridge_state_changed;
            }
            index += 1;
        }
        bridge_state_changed
    }

    /// Walk one Wave damage request in recorded-cell and current Cell-list
    /// order. Each receiver commits before the next occupant is selected, so
    /// fatal UnInit and nested effects are visible to the remaining walk.
    /// This is the receiver walk of `WaveClass::DamageArea` (`0x0075F330`).
    fn commit_logic_wave_damage_request(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        request: &crate::sim::wave::WaveDamageRequest,
        frame_effects: FrameEffects<'_>,
    ) {
        // DamageArea reads WaveClass+0x1D4 at the call boundary. Health and
        // Rust death-animation state do not null that pointer; only the exact
        // PointerExpired callback does. Revalidate the buffered fixture path
        // against the live Wave so an intervening owner UnInit still aborts.
        if self
            .waves
            .get(request.wave_id)
            .and_then(|wave| wave.owner_id)
            != Some(request.firer_id)
        {
            return;
        }

        use crate::sim::occupancy::CellObjectMember as CellObject;

        /// The cell's object list `WaveClass::DamageArea @ 0x0075F330` walks (the
        /// bridge layer's when its height test picks it), in its current order.
        fn current_order(
            occupancy: &crate::sim::occupancy::OccupancyGrid,
            terrain_cells: &BTreeMap<(u16, u16), u64>,
            rx: u16,
            ry: u16,
            layer: MovementLayer,
        ) -> Vec<CellObject> {
            occupancy
                .cell_objects(rx, ry, layer, terrain_cells.get(&(rx, ry)).copied())
                .collect()
        }

        let mut run =
            crate::sim::combat::world_receiver::ReceiverRun::default().with_effects(frame_effects);
        // Wave's synchronous receiver (0x0075F42C) can detonate a DeathWeapon
        // at 0x0070D782. Zero-delay/default-Start impact Anim construction
        // (0x00469C93 -> 0x00422702/0x00424D5A) accesses the live smudge map
        // before returning. Keep that authority resident through recursion.
        // Evidence: active gamemd.exe bodies and caller instructions.
        let house_alliances = self.house_alliances.clone();
        let current_tick = u64::from(self.session.binary_frame);
        let scenario_no_damage = self.session.no_damage;
        let mut effects = crate::sim::combat::DeathEffects::default();
        let mut under_attack_events = Vec::new();
        let mut collapsed_terrain_cells = BTreeMap::new();

        {
            for recorded in &request.recorded_cells {
                let cell = recorded.current_cell(self.resolved_terrain.as_ref());
                // GetWeapon(0) is re-entered exactly once per cell, with the
                // owner's current rank. A null owner aborts DamageArea before
                // any receiver, wall, cliff, or RNG work.
                let Some(firer) = self.substrate.entities.get(request.firer_id) else {
                    break;
                };
                let Some(object_type) = self.interner.object_type(firer.type_ref(), rules) else {
                    break;
                };
                let Some(weapon_name) = crate::sim::combat::combat_weapon::primary_for_tier(
                    object_type,
                    firer.veterancy.rank(),
                ) else {
                    break;
                };
                let Some(weapon) = rules.weapon(weapon_name) else {
                    break;
                };
                let raw_ambient_damage = weapon.ambient_damage;
                let Some(warhead_name) = weapon.warhead.as_deref() else {
                    break;
                };
                let warhead_ref = self.interner.intern(warhead_name);
                let source_house = Some(firer.owner());
                let mut shared_damage = raw_ambient_damage;

                // The shared dummy is still a native CellClass entry, so the
                // GetWeapon(0) call above belongs to it too. Its final stamped
                // coordinate cannot own a real cell list, wall, or cliff.
                if !cell.real {
                    continue;
                }
                let (Ok(rx), Ok(ry)) = (u16::try_from(cell.rx), u16::try_from(cell.ry)) else {
                    continue;
                };
                let layer = if crate::sim::cell_kernel::selects_infantry_bridge_layer(
                    cell.structural_bridge,
                    cell.level as u8,
                    request.wave_z,
                ) {
                    MovementLayer::Bridge
                } else {
                    MovementLayer::Ground
                };

                let mut current = current_order(
                    &self.substrate.occupancy,
                    &self.production.terrain_object_cells,
                    rx,
                    ry,
                    layer,
                )
                .first()
                .copied();
                while let Some(receiver_object) = current {
                    let receiver = match receiver_object {
                        CellObject::Entity(target_id) => {
                            let eligible =
                                target_id != request.firer_id
                                    && self.substrate.entities.get(target_id).is_some_and(
                                        |entity| {
                                            entity.is_alive()
                                                && !entity.death.is_dying()
                                                && !entity.lifecycle.in_limbo()
                                                && entity.lifecycle.object_alive()
                                        },
                                    );
                            if !eligible {
                                let order = current_order(
                                    &self.substrate.occupancy,
                                    &self.production.terrain_object_cells,
                                    rx,
                                    ry,
                                    layer,
                                );
                                current = order
                                    .iter()
                                    .position(|&item| item == receiver_object)
                                    .and_then(|index| order.get(index + 1).copied());
                                continue;
                            }
                            let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
                                target_id,
                                shared_damage,
                                0,
                                request.firer_id,
                                source_house,
                                warhead_ref,
                                crate::sim::combat::ReceiverCallFlags {
                                    ignore_defenses: false,
                                    arg6: false,
                                },
                            );
                            // A unit inside its war factory returns at the
                            // head of `UnitClass::ReceiveDamage`
                            // (`0x00737CB6..0x00737D2E`) before it touches
                            // the shared damage.
                            let post_object_damage =
                                if self.unit_inside_war_factory(target_id, rules) {
                                    None
                                } else {
                                    crate::sim::combat::wave_post_object_damage(
                                        &event,
                                        &self.substrate.entities,
                                        rules,
                                        &self.interner,
                                        &self.houses,
                                        &house_alliances,
                                        scenario_no_damage,
                                        current_tick,
                                        self.resolved_terrain.as_ref(),
                                    )
                                };
                            if let Some(value) = post_object_damage {
                                shared_damage = value;
                            }
                            #[cfg(any(test, feature = "test-support"))]
                            self.trace_lifecycle_for_test(
                                LifecycleTestEvent::WaveDamageReceiverSelected {
                                    wave_id: request.wave_id,
                                    target_id,
                                    scenario_rng_state: self.scenario_rng.state(),
                                },
                            );
                            crate::sim::combat::combat_aoe::AreaDamageReceiver::Entity(event)
                        }
                        CellObject::Terrain(stable_id) => {
                            #[cfg(any(test, feature = "test-support"))]
                            self.trace_lifecycle_for_test(
                                LifecycleTestEvent::WaveDamageReceiverSelected {
                                    wave_id: request.wave_id,
                                    target_id: stable_id,
                                    scenario_rng_state: self.scenario_rng.state(),
                                },
                            );
                            crate::sim::combat::combat_aoe::AreaDamageReceiver::Terrain(
                                crate::sim::combat::TerrainDamageEvent {
                                    stable_id,
                                    rx,
                                    ry,
                                    damage: shared_damage,
                                    distance_leptons: 0,
                                    warhead_ref,
                                    near_center_ic_isolation_eligible: false,
                                },
                            )
                        }
                    };
                    let (nested, mut pings) = crate::sim::combat::world_receiver::commit_area(
                        self,
                        &mut run,
                        std::slice::from_ref(&receiver),
                        rules,
                        overlay_registry,
                    );
                    effects.append(nested);
                    under_attack_events.append(&mut pings);

                    // Native loads current->NextObject only after its callback.
                    // If UnInit removed the current object, its represented
                    // list link is gone and this walk terminates.
                    let order = current_order(
                        &self.substrate.occupancy,
                        &self.production.terrain_object_cells,
                        rx,
                        ry,
                        layer,
                    );
                    current = order
                        .iter()
                        .position(|&item| item == receiver_object)
                        .and_then(|index| order.get(index + 1).copied());
                }

                // ChainReaction's Wave-tail callee is a bare RET. Wall damage
                // follows receivers and reloads raw AmbientDamage, ignoring
                // the shared mutable occupant value.
                if let (Some(grid), Some(registry)) = (self.overlay_grid.as_mut(), overlay_registry)
                    && let Some(overlay_id) = grid.cell(rx, ry).overlay_id
                    && let Some(flags) = registry.flags(overlay_id)
                {
                    let _chain_reaction_no_op = flags.chain_reaction;
                    if flags.wall {
                        let _wall = {
                            let mut host = SimulationWallRuntimeHost {
                                entities: &mut self.substrate.entities,
                                target_terrain_objects: &self.production.terrain_objects,
                                #[cfg(any(test, feature = "test-support"))]
                                detach_trace: &mut effects.cell_target_detaches,
                                radar_dirty_cells: &mut self.radar_terrain_dirty_cells,
                                radar_dirty_generation: &mut self.radar_terrain_dirty_generation,
                                tactical_dirty_cells: &mut self.tactical_dirty_cells,
                                zone_grid: &mut self.zone_grid,
                                path_grid: &mut self.path_grid,
                                bridge_state: self.bridge_state.as_ref(),
                                playfield_bounds: self.playfield_bounds,
                            };
                            crate::sim::overlay_grid::damage_wall_overlay_with_runtime_host(
                                grid,
                                registry,
                                self.resolved_terrain.as_mut(),
                                crate::sim::overlay_grid::WallPointerTarget::Real(rx, ry),
                                raw_ambient_damage,
                                &mut self.scenario_rng,
                                Some(&mut host),
                            )
                        };
                        #[cfg(any(test, feature = "test-support"))]
                        effects.wall_mutations.extend(_wall.mutations);
                    }
                }

                // The cliff tail is independent of damage magnitude and
                // Warhead. Eligibility alone consumes one Scenario draw,
                // including signed chances outside 0..=100.
                let destroyable_cliff = self
                    .resolved_terrain
                    .as_ref()
                    .is_some_and(|terrain| terrain.is_destroyable_cliff(rx, ry));
                if destroyable_cliff {
                    let chance_draw = self.scenario_rng.next_range_u32_inclusive(0, 99) as i32;
                    if chance_draw < rules.combat_damage.collapse_chance
                        && let Some(mutation) = self.resolved_terrain.as_mut().and_then(|terrain| {
                            terrain.collapse_destroyable_cliff_terrain(rx, ry, |cell_x, cell_y| {
                                if let Some(grid) = self.overlay_grid.as_mut() {
                                    let _ = grid.clear_overlay(cell_x, cell_y);
                                }
                                if let Some(grid) = self.smudge_grid.as_mut() {
                                    let _ = grid.clear_cell_slot(cell_x, cell_y);
                                }
                            })
                        })
                    {
                        // Native performs one complete zone rebuild after the
                        // two sparse stamps and before its three footprint
                        // passes. The Rust navigation owner rebuilds all
                        // movement zones as the corresponding single unit.
                        self.rebuild_dynamic_navigation(rules);

                        // Pass three's externally represented effects remain
                        // strictly interleaved: detach this CellClass target,
                        // then dirty this radar cell, in old-TMP row order.
                        for &coord in &mutation.original_footprint {
                            crate::sim::combat::combat_aoe::expire_cell_target_references(
                                &mut self.substrate.entities,
                                &self.production.terrain_objects,
                                coord.0,
                                coord.1,
                                #[cfg(any(test, feature = "test-support"))]
                                &mut effects.cell_target_detaches,
                            );
                            self.mark_radar_terrain_dirty_cells([coord]);
                        }
                        {
                            for &coord in &mutation.original_footprint {
                                if !self.tactical_dirty_cells.contains(&coord) {
                                    self.tactical_dirty_cells.push(coord);
                                }
                            }
                        };

                        // Resolve the three types in native order, then make
                        // two row-major attempts at each of the 15 grid cells.
                        // Every representable allocation is successful, so
                        // each attempt consumes type, X, Y, and delay draws.
                        let anim_types = crate::rules::effect_asset_catalog::CLIFF_COLLAPSE_ANIMS
                            .map(|name| self.interner.intern(name));
                        for &(cell_x, cell_y) in &mutation.animation_cells {
                            for _ in 0..2 {
                                let type_index =
                                    self.scenario_rng.next_range_u32_inclusive(0, 2) as usize;
                                let jitter_x =
                                    self.scenario_rng.next_range_u32_inclusive(0, 16) as i32 - 8;
                                let jitter_y =
                                    self.scenario_rng.next_range_u32_inclusive(0, 24) as i32 - 12;
                                let level = self
                                    .resolved_terrain
                                    .as_ref()
                                    .expect("collapse terrain retained")
                                    .collapse_animation_level(cell_x, cell_y);
                                let delay = self.scenario_rng.next_range_u32_inclusive(0, 2) as u16;
                                let world_coord = crate::sim::anim_class::AnimWorldCoord {
                                    x: i32::from(cell_x)
                                        .wrapping_mul(256)
                                        .wrapping_add(128)
                                        .wrapping_add(jitter_x),
                                    y: i32::from(cell_y)
                                        .wrapping_mul(256)
                                        .wrapping_add(128)
                                        .wrapping_add(jitter_y),
                                    z: i32::from(level).wrapping_mul(104),
                                };
                                let mut descriptor =
                                    crate::sim::components::AnimClassSpawnDescriptor::new(
                                        anim_types[type_index],
                                        cell_x as u16,
                                        cell_y as u16,
                                        SimFixed::from_num(128 + jitter_x),
                                        SimFixed::from_num(128 + jitter_y),
                                        level as u8,
                                    );
                                descriptor.delay = delay;
                                descriptor.loop_count = 1;
                                descriptor.draw_flags = 0x600;
                                descriptor.z_adjust = 0;
                                descriptor.reverse = false;
                                // AnimClass construction completes before the
                                // next attempt selects its type. This matters
                                // when the selected AnimType consumes Scenario
                                // RNG for RandomRate during construction.
                                let _ = self.spawn_anim_at_world(rules, descriptor, world_coord);
                            }
                        }

                        let terrain = self
                            .resolved_terrain
                            .as_ref()
                            .expect("collapse terrain retained");
                        for &(cell_x, cell_y) in &mutation.changed_cells {
                            if let Some(cell) = terrain.cell(cell_x, cell_y) {
                                collapsed_terrain_cells.insert(
                                    (cell_x, cell_y),
                                    crate::map::resolved_terrain::DynamicTerrainCellState::capture(
                                        cell,
                                    ),
                                );
                            }
                        }
                    }
                }
            }
        }

        let terrain_navigation_changed_cells = run.finish();
        self.dynamic_terrain_cells.extend(collapsed_terrain_cells);
        if let Some(terrain) = self.resolved_terrain.as_ref() {
            self.real_cell_bridge_flags_0x1180 = terrain.capture_real_cell_bridge_flags_0x1180();
        }
        self.absorb_noncombat_damage_effects(
            rules,
            overlay_registry,
            effects,
            under_attack_events,
            terrain_navigation_changed_cells,
            frame_effects,
        );
    }

    /// Complete one direct object ReceiveDamage call before its caller resumes.
    /// Unlike Apply_area_damage, a direct call has no collected area or area-wide
    /// Iron Curtain isolation decision. Nested deaths, world mutations and their
    /// publication finish before a live cell-list caller reads its successor.
    pub(crate) fn commit_direct_damage_receiver(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        event: crate::sim::combat::EntityDamageEvent,
        frame_effects: FrameEffects<'_>,
    ) -> damage_consequences::DamageCommitReceipt {
        let mut run =
            crate::sim::combat::world_receiver::ReceiverRun::default().with_effects(frame_effects);
        let (effects, under_attack_events) = crate::sim::combat::world_receiver::commit_entities(
            self,
            &mut run,
            std::slice::from_ref(&event),
            Some(false),
            rules,
            overlay_registry,
        );
        let terrain_navigation_changed_cells = run.finish();
        self.absorb_noncombat_damage_effects(
            rules,
            overlay_registry,
            effects,
            under_attack_events,
            terrain_navigation_changed_cells,
            frame_effects,
        )
    }

    /// `ReceiveDamage(&Health, 0, C4Warhead=, NULL, 1, 1, NULL)` (vt+0x16C):
    /// the self-destruct that ends a flying locomotor's owner when it has
    /// nowhere to go, in Fly's Stop_Moving (`0x004CD0CD`) and landing retry
    /// (`0x004CEBC7`) and Jumpjet's Stop_Moving (`0x0054B6B6`). Native passes
    /// the Health field itself; this uses the shared bridge_ground stock
    /// C4Warhead=Super fatal-path quotient, not general aliased packet
    /// support: override-only early damage-pointer writes remain outside it.
    pub fn receive_own_health_c4(
        &mut self,
        id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let Some(health) = self.substrate.entities.get(id).map(|e| e.health.current()) else {
            return;
        };
        let warhead = self.interner.intern(&rules.bridge_warheads.c4_name);
        self.commit_direct_damage_receiver(
            rules,
            overlay_registry,
            crate::sim::combat::EntityDamageEvent::direct_receiver(
                id,
                health,
                0,
                crate::sim::combat::RAD_NO_ATTACKER,
                None,
                warhead,
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses: true,
                    arg6: true,
                },
            ),
            frame_effects,
        );
    }

    /// Terrain counterpart of a direct object receiver. Native bridge fallout
    /// forces Object damage, while Terrain's Wood/Immune gate still applies.
    pub(crate) fn commit_direct_terrain_damage_receiver(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        event: crate::sim::combat::TerrainDamageEvent,
        frame_effects: FrameEffects<'_>,
    ) {
        let mut run =
            crate::sim::combat::world_receiver::ReceiverRun::default().with_effects(frame_effects);
        let (effects, under_attack_events) = crate::sim::combat::world_receiver::commit_terrain(
            self,
            &mut run,
            event,
            true,
            rules,
            overlay_registry,
        );
        let terrain_navigation_changed_cells = run.finish();
        self.absorb_noncombat_damage_effects(
            rules,
            overlay_registry,
            effects,
            under_attack_events,
            terrain_navigation_changed_cells,
            frame_effects,
        );
    }

    /// Commit one non-combat Apply_area_damage hit list through the ordinary
    /// ReceiveDamage -> death transaction, then hand its deferred outputs back
    /// to the world owner. `hits` remains in CellClass/object-list order; the
    /// combat helper enters a nested DeathWeapon before advancing to the next
    /// hit.
    pub(crate) fn commit_noncombat_aoe_hits(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        hits: &[crate::sim::combat::EntityDamageEvent],
        frame_effects: FrameEffects<'_>,
    ) {
        let receivers = hits
            .iter()
            .copied()
            .map(crate::sim::combat::combat_aoe::AreaDamageReceiver::Entity)
            .collect::<Vec<_>>();
        self.commit_noncombat_aoe_receivers(rules, overlay_registry, &receivers, frame_effects);
    }

    /// World-owned entry for a complete non-combat Apply_area_damage receiver
    /// vector, including TerrainClass records in captured object-list order.
    pub(crate) fn commit_noncombat_aoe_receivers(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        receivers: &[crate::sim::combat::combat_aoe::AreaDamageReceiver],
        frame_effects: FrameEffects<'_>,
    ) -> damage_consequences::DamageCommitReceipt {
        let mut run =
            crate::sim::combat::world_receiver::ReceiverRun::default().with_effects(frame_effects);
        let (effects, under_attack_events, area_result) =
            crate::sim::combat::world_receiver::commit_area_with_dispatch(
                self,
                &mut run,
                receivers,
                rules,
                overlay_registry,
            );
        let terrain_navigation_changed_cells = run.finish();
        let mut receipt = self.absorb_noncombat_damage_effects(
            rules,
            overlay_registry,
            effects,
            under_attack_events,
            terrain_navigation_changed_cells,
            frame_effects,
        );
        receipt.area_result = Some(area_result);
        receipt
    }

    /// World-owned half of a non-combat damage transaction. Physical death
    /// consequences already happened recursively in combat; this consumes the
    /// same lifecycle/presentation/terrain outputs without leaving an alternate
    /// zero-HP object registered for the next LogicClass visit.
    fn absorb_noncombat_damage_effects(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: crate::sim::combat::DeathEffects,
        under_attack_events: Vec<crate::sim::combat::UnderAttackEvent>,
        terrain_navigation_changed_cells: Vec<(u16, u16)>,
        frame_effects: FrameEffects<'_>,
    ) -> damage_consequences::DamageCommitReceipt {
        damage_consequences::DamageConsequences::immediate(
            effects,
            under_attack_events,
            terrain_navigation_changed_cells,
        )
        .commit(self, rules, overlay_registry, frame_effects)
    }

    /// `HouseClass::NotifyUnderAttack @ 0x004F93E0` for a building warning,
    /// plus the `UnitClass::ReceiveDamage 0x00738530` harvester ping.
    ///
    /// The victim's own line: the radar diamond (type 4 miner / type 3 base,
    /// `0x004F94E4`/`0x004F9544`) is requested for the victim house; the app
    /// keeps the local-player filter and admits it there. The ally line (`0x004F955D..0x004F95B3`,
    /// building path only): every *other* human house that the victim house
    /// lists in its own ally bitfield (`House+0x5788`, read at `0x004F9450`
    /// through `IsAlliedWith`'s one-way rule) hears
    /// `EVA_OurAllyIsUnderAttack` behind `CreateRadarEvent(0x10, cell)`,
    /// unless the victim's country is `MultiplayPassive=` in a non-campaign
    /// mode (`0x004F945D..0x004F9472`). Native has no other rate limit.
    pub(crate) fn dispatch_under_attack_events(
        &mut self,
        events: &[crate::sim::combat::UnderAttackEvent],
        rules: &RuleSet,
    ) {
        let game_mode_nonzero = self.session.game_mode_nonzero;
        for event in events {
            let Some(object) = self.object_type(event.victim_type, rules) else {
                continue;
            };
            let structure = object.category == crate::rules::object_type::ObjectCategory::Building;
            // House4F9491..4F94A3: a deployed miner needs both predicates.
            // Non-building producers have already admitted Harvester= units.
            // Native corpus: tools/spatial_oracle/temporal_house_notice.json.
            let miner = !structure || (object.undeploys_into.is_some() && object.resource_gatherer);
            let event_type = if miner {
                RadarEventType::HarvesterUnderAttack
            } else {
                RadarEventType::BaseUnderAttack
            };
            self.sound_events.push(SimSoundEvent::UnderAttack {
                rx: event.rx,
                ry: event.ry,
                owner: event.owner,
                miner,
                radar: RadarEventRequest::new(event_type, event.rx, event.ry),
            });
            if !structure {
                continue;
            }
            let victim_passive = self
                .houses
                .get(&event.owner)
                .is_some_and(|house| house.multiplay_passive);
            if game_mode_nonzero && victim_passive {
                continue;
            }
            let victim_name = self.interner.resolve(event.owner).to_string();
            let listeners: Vec<InternedId> = self
                .session
                .house_order
                .iter()
                .copied()
                .filter(|&listener| {
                    listener != event.owner
                        && self
                            .houses
                            .get(&listener)
                            .is_some_and(|house| house.is_human)
                        && crate::map::houses::is_allied_with(
                            &self.house_alliances,
                            &victim_name,
                            self.interner.resolve(listener),
                        )
                })
                .collect();
            for listener in listeners {
                self.sound_events.push(SimSoundEvent::AllyUnderAttack {
                    owner: listener,
                    radar: RadarEventRequest::new(
                        RadarEventType::AllyUnderAttack,
                        event.rx,
                        event.ry,
                    ),
                });
            }
        }
    }

    /// `TechnoClass::Death_Announcement @ 0x004D98C0` for damage kills
    /// and the living-hull Squid cull: owner passes `HouseClass::IsHumanPlayer @ 0x0050B6F0`
    /// (`0x004D98CA`; the local player natively, every human house here with
    /// the app filtering), then `CreateRadarEvent(7, cell)` (`0x004D98FE`,
    /// 8-cell dedupe, admitted client-side) gates `EVA_UnitLost`
    /// (`0x004D9911`).
    pub(crate) fn dispatch_unit_lost_events(
        &mut self,
        events: &[crate::sim::combat::UnitLostEvent],
    ) {
        for event in events {
            if !self.owner_is_human(event.owner) {
                continue;
            }
            self.sound_events.push(SimSoundEvent::UnitLost {
                owner: event.owner,
                radar: RadarEventRequest::new(RadarEventType::UnitLost, event.rx, event.ry),
            });
        }
    }

    /// Observe all three logical RNG objects without exposing mutation.
    pub fn rng_views(&self) -> SimulationRngViews<'_> {
        SimulationRngViews {
            scenario: self.scenario_rng.logical_view(),
            main: self.main_rng.logical_state(),
            mapgen: self.mapgen_rng.logical_view(),
        }
    }

    /// Summarise this tick for cross-engine parity comparison.
    ///
    /// Read-only and side-effect free: the caller decides whether to record it, so
    /// enabling capture cannot perturb the run being measured. The RNG cursor comes from
    /// the main stream, which is the one the original engine exposes globally.
    pub fn parity_digest(&self) -> crate::sim::parity_digest::ParityDigest {
        let views = self.rng_views();
        crate::sim::parity_digest::ParityDigest::capture(
            self.session.tick,
            self.entities(),
            &self.houses,
            views.main.index_a,
            views.main.index_b,
            views.scenario.index_a,
            views.scenario.index_b,
        )
    }

    /// Capture all three complete logical RNG objects as immutable evidence.
    pub fn rng_state(&self) -> SimulationRngState {
        SimulationRngState {
            scenario: self.scenario_rng.logical_state(),
            main: self.main_rng.logical_state(),
            mapgen: self.mapgen_rng.logical_state(),
        }
    }

    /// LoadContent67E730 does not restore process Main/MapGen. ScenarioLoad
    /// 689470 -> post-read683560 has already reset Scenario to Seed(0).
    /// The candidate stages the one process Main cursor: draws made while the
    /// load is prepared advance it, and the commit moves it into that cell
    /// (`install_fresh_process_main`). In-scenario loading keeps the live
    /// seed and MapGen; the cold shell starts MapGen from the CRT Seed(0)
    /// object (native fixture in world/rng_routing_tests.rs).
    pub fn retain_process_state_for_load(&mut self, process: SnapshotProcessState<'_>) {
        match process {
            SnapshotProcessState::Live(live) => {
                self.session.seed = live.session.seed;
                self.main_rng = live.main_rng.staged_copy();
                self.mapgen_rng = live.mapgen_rng.clone();
                self.bind_shared_cell_dummy(live.effective_shared_cell_dummy());
                self.substrate
                    .raw_cell_occupation
                    .retain_process_dummy_from(&live.substrate.raw_cell_occupation);
            }
            SnapshotProcessState::Cold(main) => {
                self.session.seed = 0;
                self.main_rng = main.staged_copy();
                self.mapgen_rng = SimRng::new(0);
            }
        }
    }

    /// Init_Random_Number_System52FC20, ordinary offline menu-load tail
    /// 52E619. The caller supplies the resolved clock/override seed. Both
    /// Scenario and process Main are reseeded, Main in its shared process
    /// cell; MapGen is untouched. In-game options resume does not reach this
    /// caller. Executed full-state controls:
    /// tools/rmg_oracle/vectors/rng.json::menu_rng_cases.
    pub fn initialize_random_number_system(&mut self, seed: u32) {
        self.session.seed = u64::from(seed);
        self.scenario_rng = SimRng::new(u64::from(seed));
        self.main_rng.install_from(MainRng::new(u64::from(seed)));
    }

    /// Apply the successful load's native MapClass Resize reconstruction to
    /// every modeled field of the fixed fallback CellClass.
    ///
    /// `MouseClass::Load` dispatch at `0x005BE150` routes restored dimensions through
    /// `MapClass::Resize @ 0x00565C10`, whose unconditional call to
    /// `CellClass::Constructor @ 0x0047BBF0` reconstructs the fixed dummy in
    /// place. The app resets a detached candidate before fallible restoration,
    /// so a rejected transactional load cannot mutate the running world.
    pub fn reconstruct_cellclass_dummy_for_map_resize(&mut self) {
        self.effective_shared_cell_dummy()
            .reconstruct_for_map_resize();
        self.substrate
            .base_reservations
            .reconstruct_dummy_for_map_resize();
        self.substrate
            .raw_cell_occupation
            .reconstruct_dummy_for_map_resize();
    }

    /// Adopt the process-global CellClass identity already bound to a load's
    /// resolved grid, before that grid is cloned into Simulation.
    pub fn bind_shared_cell_dummy(&mut self, shared_cell_dummy: SharedCellDummy) {
        self.shared_cell_dummy = shared_cell_dummy.clone();
        if let Some(terrain) = self.resolved_terrain.as_mut() {
            terrain.bind_shared_cell_dummy(shared_cell_dummy);
        }
    }

    /// Rebind a successfully prepared map to the retained process selector.
    /// The map owner keeps its catalogs on the same lazy table identity.
    pub fn bind_tile_variant_selector(
        &mut self,
        selector: crate::map::tile_variant_selector::TileVariantSelector,
    ) {
        if let Some(terrain) = self.resolved_terrain.as_mut() {
            terrain.bind_tile_variant_selector(selector);
        }
    }

    /// Install a newly constructed map grid and capture the real CellClass
    /// `0x1180` value authority after row-major OverlayPack marking.
    pub fn install_resolved_terrain_for_new_map(&mut self, resolved_terrain: ResolvedTerrainGrid) {
        self.bind_shared_cell_dummy(resolved_terrain.shared_cell_dummy());
        self.real_cell_bridge_flags_0x1180 =
            resolved_terrain.capture_real_cell_bridge_flags_0x1180();
        self.dynamic_terrain_cells.clear();
        self.resolved_terrain = Some(resolved_terrain);
        // A fresh grid restarts its mutation epoch; drop any plane keyed on the old one.
        self.movement_pass_cache = Default::default();
    }

    /// Apply one live runtime setter and update only the allocated real-cell
    /// serialized values it changed. Missing slots remain owned solely by the
    /// process dummy and are intentionally absent from Scenario payload.
    #[cfg(any(test, feature = "test-support"))]
    pub fn apply_runtime_bridge_flag_stamp(
        &mut self,
        stamp: crate::map::bridge_facts::BridgeFlagStamp,
    ) {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        if !terrain.bridge_flag_authority_matches_shape(&self.real_cell_bridge_flags_0x1180) {
            self.real_cell_bridge_flags_0x1180 = terrain.capture_real_cell_bridge_flags_0x1180();
        }
        let updates = self
            .resolved_terrain
            .as_mut()
            .expect("terrain presence checked before runtime bridge setter")
            .apply_runtime_bridge_flag_stamp(stamp);
        for (index, flags) in updates {
            self.real_cell_bridge_flags_0x1180
                .set_allocated_cell(index, flags);
        }
    }

    /// High-bridge anchor construction uses the same literal setter as bridge
    /// damage/repair, but it additionally creates the family/anchor relations
    /// from which Rust derives live bridge topology.
    pub fn apply_runtime_bridge_mark_stamp(
        &mut self,
        stamp: crate::map::bridge_facts::BridgeFlagStamp,
        family: crate::map::bridge_facts::BridgeStampFamily,
    ) {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        if !terrain.bridge_flag_authority_matches_shape(&self.real_cell_bridge_flags_0x1180) {
            self.real_cell_bridge_flags_0x1180 = terrain.capture_real_cell_bridge_flags_0x1180();
        }
        let updates = self
            .resolved_terrain
            .as_mut()
            .expect("terrain presence checked before runtime bridge Mark setter")
            .apply_runtime_bridge_mark_stamp(stamp, family);
        for (index, flags) in updates {
            self.real_cell_bridge_flags_0x1180
                .set_allocated_cell(index, flags);
        }
    }

    /// Synthetic fixtures may assign a detached grid directly. Production
    /// construction binds both owners, but gameplay must still read the live
    /// handle attached to the actual CellClass table it queried.
    pub fn effective_shared_cell_dummy(&self) -> SharedCellDummy {
        self.resolved_terrain
            .as_ref()
            .map(ResolvedTerrainGrid::shared_cell_dummy)
            .unwrap_or_else(|| self.shared_cell_dummy.clone())
    }

    /// Install the Scenario cursor advanced by the pre-IsoMapPack Fill pass.
    /// Called only by sim's opaque bootstrap owner; Main remains independently
    /// owned by the terrain variant selector.
    pub(super) fn install_terrain_load_advanced_scenario_rng(&mut self, scenario_rng: SimRng) {
        self.scenario_rng = scenario_rng;
    }

    /// Install the Main cursor advanced by one-time terrain variant-table
    /// generation. Called only by sim's opaque bootstrap owner; Scenario
    /// remains independently owned by the Fill pass.
    pub(super) fn install_variant_advanced_main_rng(&mut self, main_rng: SimRng) {
        self.main_rng = main_rng.into();
    }

    /// Bind a successful fresh or saved scenario to the retained process Main
    /// cell, moving its staged cursor into that same identity.
    /// App orchestration must first cancel the outgoing playback callbacks.
    pub fn install_fresh_process_main(&mut self, process: &MainRng) {
        let staged = std::mem::replace(&mut self.main_rng, process.shared_owner());
        process.install_from(staged);
    }

    /// Install the process MapGen cursor prepared by the bootstrap owner.
    pub(super) fn install_generated_mapgen_rng(&mut self, mapgen_rng: SimRng) {
        self.mapgen_rng = mapgen_rng;
    }

    /// Publish a completed RMG run into the resident process cursor. Original
    /// Generate598960 advances MapGen in place and returns without restoring
    /// it; preview OK/Cancel leave it unchanged (59664C,59951F..599527).
    pub fn install_mapgen_continuation(
        &mut self,
        continuation: crate::rng_continuation::MapGenRngContinuation,
    ) {
        self.install_generated_mapgen_rng(SimRng::from_mapgen_continuation(continuation));
    }

    /// Carry the resident process cursor through a fresh Scenario replacement.
    /// Original52FC20 reseeds Scenario/Main only. The outgoing world remains
    /// inactive throughout loading, so this opaque snapshot has no competing
    /// draw authority and the fresh bootstrap consumes it exactly once.
    pub fn mapgen_continuation_for_fresh_load(
        &self,
    ) -> crate::rng_continuation::MapGenRngContinuation {
        self.mapgen_rng.mapgen_continuation()
    }

    /// Campaign ClearScene/SetDefaults retain these Scenario flag bits. The
    /// inactive loader can carry their existing owners' draw-free projection
    /// through map reads without creating a second mutable flags word.
    /// Original683610/686B20; cold Scenario ctor6833F3 word0x8088.
    pub fn retained_campaign_special_flags(&self) -> crate::map::basic::SpecialFlagsSection {
        crate::map::basic::SpecialFlagsSection {
            inert: Some(self.session.no_damage),
            tiberium_grows: Some(self.session.tiberium_grows_flag),
            tiberium_spreads: Some(self.session.tiberium_spreads_flag),
            destroyable_bridges: Some(
                self.bridge_state
                    .as_ref()
                    .is_none_or(|state| state.is_destroyable()),
            ),
            ..Default::default()
        }
    }

    /// Create a new empty simulation with the test fixtures' seed. Test builds
    /// only: a launch constructs through `ScenarioDescriptor`
    /// (`Simulation::from_descriptor`, `ScenarioBootstrapRng::into_simulation`),
    /// so no match can start on the default seed (AT-3).
    #[cfg(any(test, feature = "test-support"))]
    pub fn new() -> Self {
        Self::with_seed(DEFAULT_SIM_SEED)
    }

    /// Bind the interner's type handles ([`StringInterner::bind_types`]) and
    /// build the pre-resolved rule handles beside them. Call once at sim init
    /// AFTER `intern_rule_type_ids`, and again after load once the RuleSet is
    /// available. Idempotent: the warhead names are already interned on every
    /// re-resolution path, so serialized interner state cannot grow or reorder.
    ///
    /// [`StringInterner::bind_types`]: crate::sim::intern::StringInterner::bind_types
    pub fn resolve_type_handles(&mut self, rules: &RuleSet) {
        self.interner.bind_types(rules);
        self.rule_handles = Some(crate::sim::type_handle_table::ResolvedRuleHandles::resolve(
            rules,
            &mut self.interner,
        ));
    }

    /// Intern every rules type id (infantry, vehicle, aircraft, building,
    /// `[SuperWeaponTypes]`) so `interner.get(type_id)` succeeds for any type
    /// this ruleset references, and a command naming one carries the same id
    /// in every world built from these rules: the Chrono Warp is clicked
    /// without ever being granted. Moved from `RuleSet::intern_all_ids` (F04):
    /// interning is sim-side work over rules-owned canonical names.
    pub fn intern_rule_type_ids(&mut self, rules: &RuleSet) {
        for id in rules
            .infantry_ids
            .iter()
            .chain(&rules.vehicle_ids)
            .chain(&rules.aircraft_ids)
            .chain(&rules.building_ids)
            .chain(&rules.super_weapon_order)
        {
            self.interner.intern(id);
        }
    }

    /// Resolve the rules-owned ordered AnimType lists in this world's interner.
    /// These IDs are derived, never portable between independently constructed
    /// worlds. Native ReadGeneral owns the list names/order; both map binding
    /// and snapshot restoration project those names through this one writer.
    pub fn resolve_rule_animation_lists(&mut self, rules: &RuleSet) {
        self.bridge_explosions = rules
            .bridge_rules
            .explosions
            .iter()
            .map(|name| self.interner.intern(name))
            .collect();
        self.metallic_debris = rules
            .general
            .metallic_debris
            .iter()
            .map(|name| self.interner.intern(name))
            .collect();
    }

    /// Pre-resolved rule handles for combat comparisons.
    ///
    /// # Panics
    /// Panics if `resolve_type_handles` has not run (init and load both call it
    /// before any combat tick).
    pub fn rule_handles(&self) -> crate::sim::type_handle_table::ResolvedRuleHandles {
        self.rule_handles.expect(
            "Simulation::resolve_type_handles must run at sim init \
             before combat reads warhead handles",
        )
    }

    /// Resolve an entity's type to its `ObjectType` through the interner's
    /// bound handles (`StringInterner::object_type`): two array indexes, no
    /// hashing, and the name path wherever the handles do not apply.
    #[inline]
    pub fn object_type<'r>(
        &self,
        type_ref: InternedId,
        rules: &'r RuleSet,
    ) -> Option<&'r ObjectType> {
        self.interner.object_type(type_ref, rules)
    }

    /// Create a new empty simulation with an explicit deterministic seed.
    /// Test/dev entry — u64 seeds wider than 32 bits keep their full value in
    /// `session.seed` (pinned harness baselines depend on it) even though the
    /// stream seeder consumes only 32 bits. Its frames hash their state for
    /// the TickResult only once [`Self::hash_state_every`] asks, or when they
    /// end the match: hashing the whole world after every frame is most of an
    /// unoptimized frame's time.
    pub fn with_seed(seed: u64) -> Self {
        let mut session = ScenarioSession::from_descriptor(
            &crate::sim::scenario_session::ScenarioDescriptor::default(),
        );
        session.seed = seed;
        let mut simulation = Self::construct(session);
        simulation.native_unique_ids =
            Some(crate::sim::native_identity::NativeUniqueIdCursor::for_synthetic_simulation());
        simulation.state_hash_interval = None;
        simulation
    }

    /// Construct a session simulation from an app-layer launch descriptor.
    /// The only entry real launches use; `new()`/`with_seed()` remain for
    /// tests and dev tooling.
    pub fn from_descriptor(desc: &crate::sim::scenario_session::ScenarioDescriptor) -> Self {
        Self::construct(ScenarioSession::from_descriptor(desc))
    }

    /// Shared constructor body: seed both gameplay streams identically from
    /// the session seed and construct fresh MapGen with native Seed(0) state.
    fn construct(session: ScenarioSession) -> Self {
        let seed = session.seed;
        let mut out = Self {
            movement_pass_cache: Default::default(),
            expiry_listeners: Default::default(),
            path_batch: None,
            scan_batch: None,
            interner: crate::sim::intern::StringInterner::new(),
            type_tech_levels: TypeTechLevels::default(),
            rule_handles: None,
            production: ProductionState::default(),
            session,
            object_placement_scope_depth: 0,
            buildup_bypass: false,
            scenario_rng: SimRng::new(seed),
            main_rng: MainRng::new(seed),
            mapgen_rng: SimRng::new(0),
            native_unique_ids: None,
            authored_tiberium_value_total: None,
            post_load_particle_system_constructed: false,
            native_map_tubes: crate::map::tubes::NativeMapTubesState::default(),
            load_objects: LoadObjectLifecycle::default(),
            fog: FogState::default(),
            house_alliances: HouseAllianceMap::default(),
            substrate: ObjectSubstrate::new(),
            lifecycle_outputs: Vec::new(),
            unit_voice_visit_interest: Default::default(),
            mission_spawned_entities: false,
            frame_overlay_updates: Vec::new(),
            frame_overlay_removals: Vec::new(),
            terminal_score_snapshot: None,
            pending_lifecycle_requests: Vec::new(),
            fire_requests: Default::default(),
            pending_projectile_detonations: Vec::new(),
            pending_wave_damage_requests: Vec::new(),
            #[cfg(any(test, feature = "test-support"))]
            lifecycle_test_events: Vec::new(),
            #[cfg(test)]
            receiver_fixture: None,
            trigger_effects: Vec::new(),
            #[cfg(any(test, feature = "test-support"))]
            master_frame_test_trace: Vec::new(),
            #[cfg(any(test, feature = "test-support"))]
            house_ai_activation_order_test_trace: Vec::new(),
            #[cfg(any(test, feature = "test-support"))]
            house_update_append_after_test: None,
            sound_events: Vec::new(),
            lighting_sources: crate::sim::light_sources::LightingSources::default(),
            fire_events: Vec::new(),
            combat_light_requests: Vec::new(),
            projectiles: crate::sim::projectile::ProjectileStore::new(),
            waves: crate::sim::wave::WaveStore::new(),
            active_wave_links: BTreeMap::new(),
            object_custom_sounds: BTreeMap::new(),
            pending_smudge_requests: Vec::new(),
            bale_events: Vec::new(),
            bunker_wall_events: Vec::new(),
            team_script_vm: TeamScriptVm::default(),
            houses: BTreeMap::new(),
            zone_grid: None,
            path_grid: None,
            structure_navigation_cells: BTreeSet::new(),
            resolved_terrain: None,
            shared_cell_dummy: SharedCellDummy::fresh(),
            native_local_cell_cursor: None,
            native_allocated_cell_cursor: None,
            real_cell_bridge_flags_0x1180: RealCellBridgeFlags0x1180::default(),
            dynamic_terrain_cells: BTreeMap::new(),
            bridge_state: None,
            pending_bridge_collapses: Default::default(),
            overlay_grid: None,
            crate_authority: crate::sim::crates::CrateAuthority::default(),
            smudge_grid: None,
            radiation: crate::sim::radiation::RadiationState::default(),
            bombs: crate::sim::bomb::BombList::default(),
            kamikaze: crate::sim::kamikaze::KamikazeTracker::default(),
            disk_lasers: disk_laser::DiskLasers::default(),
            playfield_bounds: None,
            playfield_size_height: None,
            playfield_revision: 0,
            bridge_explosions: Vec::new(),
            metallic_debris: Vec::new(),
            radar_terrain_dirty_cells: Vec::new(),
            radar_terrain_dirty_generation: 0,
            radar_terrain_dirty_index: Default::default(),
            tactical_dirty_cells: Vec::new(),
            power_states: BTreeMap::new(),
            super_weapons: BTreeMap::new(),
            lightning_storm: Default::default(),
            psychic_dominator: Default::default(),
            ion_blasts: Vec::new(),
            terrain_speed_config: terrain_speed::TerrainSpeedConfig::default(),
            debug_event_logging: false,
            input_delay_ticks: u64::from(crate::util::config::default_input_delay_ticks()),
            quit_requested: false,
            executed_exit_owner: None,
            connection_lost: false,
            command_queue_route: CommandQueueRoute::SinglePlayer,
            frame_sync_checksum: None,
            state_hash_interval: every_frame(),
            pending_commands: Vec::new(),
            planning: planning::PlanningState::default(),
            #[cfg(any(test, feature = "test-support"))]
            replaying_recorded_commands: false,
            trigger_runtime: TriggerRuntime::default(),
        };
        debug_assert_eq!(out.scenario_rng.state(), out.main_rng.state());
        // Authoritative map bounds are session state, known at construction —
        // vision must never run against a zero-dim first-tick window. Fixture
        // sims built without a descriptor (zero bounds) keep the lazy
        // derivation as a fallback inside the vision recompute.
        out.fog.width = out.session.map_width;
        out.fog.height = out.session.map_height;
        out
    }

    // --- Scenario stream (gamemd Scenario->Random @ Scen+0x218) ---
    // Keep accessors distinct even though several return the same stream today:
    // the intent name is the per-consumer routing record and the grep/audit anchor.
    // Consumers that borrow `scenario_rng` directly beside other fields: infantry
    // sub-cell rotation and paradrop sub-cell; destruction smudge, survivor and
    // debris and the smudge type pick; the overlay/wall damage roll; ore growth
    // and spread (queue, direction, variant, TIBTRE); the building damage-fire
    // type and start frame.
    pub(crate) fn bridge_rng(&mut self) -> &mut SimRng {
        &mut self.scenario_rng
    } // bridge collapse/debris/explosion
    pub(crate) fn particle_rng(&mut self) -> &mut SimRng {
        &mut self.scenario_rng
    } // particle/smoke/gas/fire lifetime/offset/dir/insert
    pub(crate) fn superweapon_rng(&mut self) -> &mut SimRng {
        &mut self.scenario_rng
    } // lightning-storm scatter/bolt
    /// Capture the process-continuity Scenario cursor when gameplay returns to
    /// the frontend. The app stores this clone until the next successful Start
    /// reseeds the gameplay pair.
    pub fn clone_scenario_rng(&self) -> SimRng {
        self.scenario_rng.clone()
    }

    // --- Main/global process stream (`main_rng`) ---
    // Its owning modules consume the same cursor: terrain-load variants
    // (`terrain_load_draws`), selection_voice, Gattling Report, death sounds
    // and MoveSound. Local selection calls the domain operation; it must not
    // create a seeded copy that loses loading or live-consumer continuation.

    /// Process Main886B88, shared by native command voices, Voc and Theme.
    /// Draws remain outside Scenario/hash/save state; no copied audio cursor.
    pub fn presentation_main_draws(&self) -> crate::sim::rng::MainRngDraws {
        self.main_rng.draws()
    }

    /// The occupancy grid (per-cell object lists). Read access for systems above sim/.
    pub fn occupancy(&self) -> &OccupancyGrid {
        &self.substrate.occupancy
    }

    /// Mutable occupancy access for the few above-sim callers that unmark cells.
    #[cfg(any(test, feature = "test-support"))]
    pub fn occupancy_mut(&mut self) -> &mut OccupancyGrid {
        &mut self.substrate.occupancy
    }

    /// The entity store. Read access for systems above sim/.
    pub fn entities(&self) -> &EntityStore {
        &self.substrate.entities
    }

    /// Logic scheduling order, also consumed by the current radar pipeline.
    pub fn logic_order(&self) -> &[u64] {
        self.substrate.logic.as_slice()
    }

    /// Retained Display membership and order. Tactical6D8F19..6D95A9 walks
    /// these five vectors directly; it neither queries GetLayer nor re-sorts.
    pub fn display_layers(&self) -> &display_layers::DisplayLayers {
        &self.substrate.display
    }

    /// Mutable entity-store access for above-sim callers.
    pub fn entities_mut(&mut self) -> &mut EntityStore {
        &mut self.substrate.entities
    }

    /// Disjoint access to the entity store (mutable) and the interner (shared)
    /// for the few above-sim callers that need both at once. The field-level
    /// disjoint borrow that made this trivial when `entities` was a sibling
    /// field of `interner` is no longer reachable from outside sim/.
    pub fn entities_mut_and_interner(
        &mut self,
    ) -> (&mut EntityStore, &crate::sim::intern::StringInterner) {
        (&mut self.substrate.entities, &self.interner)
    }

    /// Resolve an InternedId back to its display string.
    #[inline]
    pub fn resolve(&self, id: crate::sim::intern::InternedId) -> &str {
        self.interner.resolve(id)
    }

    /// Append each cell not yet in the pending radar-terrain batch. A
    /// whole-map reveal marks every cell, so membership goes through the
    /// derived index rather than a scan of the list.
    pub fn mark_radar_terrain_dirty_cells<I>(&mut self, cells: I)
    where
        I: IntoIterator<Item = (u16, u16)>,
    {
        let built_for = (
            self.radar_terrain_dirty_cells.len(),
            self.radar_terrain_dirty_generation,
        );
        let index = &mut self.radar_terrain_dirty_index;
        if (index.1, index.2) != built_for {
            index.0 = self.radar_terrain_dirty_cells.iter().copied().collect();
        }
        let mut changed = false;
        for cell in cells {
            if index.0.insert(cell) {
                self.radar_terrain_dirty_cells.push(cell);
                changed = true;
            }
        }
        if changed {
            self.radar_terrain_dirty_generation =
                self.radar_terrain_dirty_generation.wrapping_add(1);
        }
        index.1 = self.radar_terrain_dirty_cells.len();
        index.2 = self.radar_terrain_dirty_generation;
    }

    pub(crate) fn flush_smudge_dirty(&mut self) {
        let dirty = self
            .smudge_grid
            .as_mut()
            .map(crate::sim::smudge_grid::SmudgeGrid::drain_dirty)
            .unwrap_or_default();
        for cell in dirty {
            self.tactical_dirty_cells.push(cell);
            self.mark_radar_terrain_dirty_cells([cell]);
        }
    }

    /// Commit an AnimClass smudge at its producer boundary, using the live
    /// world map, resource, occupation and dirty-state authorities.
    pub(crate) fn commit_smudge_request_inline(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        request: crate::sim::combat::SmudgeSpawnRequest,
    ) {
        let binary_frame = self.session.binary_frame;
        let spread_enabled = self.production.ore_growth_config.spreads;
        let priority = self.object_placement_scope_active();
        let mut construction = crate::sim::smudge_grid::SmudgeConstruction::new(
            &mut self.substrate.next_stable_object_id,
            &self.substrate.entities,
            &mut self.native_unique_ids,
            priority,
        );
        dispatch_smudge_inline(
            &request,
            rules,
            overlay_registry,
            &self.substrate.occupancy,
            &self.substrate.raw_cell_occupation,
            &mut self.scenario_rng,
            self.overlay_grid.as_mut(),
            self.resolved_terrain.as_mut(),
            self.smudge_grid.as_mut(),
            &mut self.production.ore_growth_state,
            &self.production.tiberium_spawning_terrain_cells,
            &self.production.terrain_object_cells,
            binary_frame,
            spread_enabled,
            &mut self.radar_terrain_dirty_cells,
            &mut self.radar_terrain_dirty_generation,
            &mut self.tactical_dirty_cells,
            &mut construction,
        );
        if let Some(id) = construction.constructed_id() {
            // Full SmudgeCtor6B4A50 retires after its Mark/Place, before the
            // producer can make another RNG draw or construct the next object.
            self.uninit_with_context(
                id,
                lifecycle::UninitContext::new(Some(rules), overlay_registry),
            );
        }
        self.flush_smudge_dirty();
    }

    /// Execute59 at6DE313 uses +90 as a waypoint, not the +44 tail or an
    /// amount. Map57B790..57B860 tests121 offsets with sqrtApprox/ftol and
    /// always reduces the unchanged centre pointer, never the candidate.
    /// Seven original executions (Steam3e81a617..08600, real and Dummy,
    /// signed extremes) each observed109 ordered calls480A80(receiver,1).
    /// Packed subtraction leaves offsets independent of the centre, so reuse
    /// that constant count without adding another floating distance kernel.
    pub(crate) fn trigger_reduce_tiberium_at_waypoint(
        &mut self,
        waypoint: i32,
        rules: &RuleSet,
        overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    ) -> bool {
        let Some(cell) = self.session.waypoint_cell(waypoint) else {
            return false;
        };
        if self.resolved_terrain.is_none() || self.overlay_grid.is_none() {
            return false;
        }
        // Keep every call, including after depletion. The shared owner
        // performs each native lookup and owns RNG/queues/radar/cleanup.
        for _ in 0..109 {
            self.reduce_tiberium_at_with_native_context(
                (cell.0 as u16, cell.1 as u16),
                1,
                Some(rules),
                Some(overlay_registry),
            );
        }
        true
    }

    pub(crate) fn reduce_tiberium_at_with_native_context<A: Into<i32>>(
        &mut self,
        cell: (u16, u16),
        amount: A,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> crate::sim::tiberium::ReduceTiberiumOutcome {
        let mut ctx = crate::sim::tiberium::ReduceTiberiumContext {
            overlay_grid: self.overlay_grid.as_mut(),
            ore_growth_state: &mut self.production.ore_growth_state,
            overlay_registry,
            tiberium_types: rules.map(|rules| &rules.tiberium_types),
            resolved_terrain: self.resolved_terrain.as_mut(),
            source_object_cells: Some(&self.production.tiberium_spawning_terrain_cells),
            live_objects: Some(crate::sim::tiberium::NativeCellObjectView::new(
                &self.substrate.occupancy,
                &self.production.terrain_object_cells,
            )),
            // ore growth/spread — scenario stream. Direct field: this literal
            // co-borrows other &mut self fields, so an all-self accessor conflicts.
            rng: Some(&mut self.scenario_rng),
            binary_frame: self.session.binary_frame,
            spread_enabled: self.production.ore_growth_config.spreads,
            radar_dirty_cells: Some(&mut self.radar_terrain_dirty_cells),
            radar_dirty_generation: Some(&mut self.radar_terrain_dirty_generation),
            tactical_dirty_cells: Some(&mut self.tactical_dirty_cells),
        };
        crate::sim::tiberium::reduce_tiberium(&mut ctx, cell, amount.into())
    }

    /// Intern a string, returning its InternedId.
    #[inline]
    pub fn intern(&mut self, s: &str) -> crate::sim::intern::InternedId {
        self.interner.intern(s)
    }

    /// Current world coordinate of whatever object holds one app-side loop
    /// handle, whether that is an anim or a `MoveSound`-carrying entity.
    ///
    /// Read-only, and audio-agnostic: `sim/` never learns that a sound exists.
    /// It exists because gamemd's owner re-drives its handle every update with
    /// its own coordinate (`AnimClass::UpdateLoopingSound @ 0x00750D40`, whose
    /// first argument is the owner's coords), and the app-side owner needs the
    /// same value. Anim ids and entity stable ids come from one
    /// `allocate_stable_id` counter — `spawn_anim_at_world` rejects a
    /// collision against both maps — so one id names at most one object.
    pub fn looping_sound_owner_coord(
        &self,
        id: crate::sim::anim_class::AnimId,
    ) -> Option<crate::sim::anim_class::AnimWorldCoord> {
        if let Some(carrier) = crate::sim::bomb::ticking_sound_carrier(id) {
            return self.bomb_ticking_coord(carrier);
        }
        // `TechnoClass::AI_Update` re-drives a gattling type's loop at its
        // Location every frame (`0x006F9E76..0x006F9EA8`).
        if let Some(techno) = crate::sim::combat::gattling::gattling_sound_techno(id) {
            return self
                .substrate
                .entities
                .get(techno)
                .map(Self::movement_sound_world);
        }
        // The same per-visit re-drive for a building's turret rotation loop on
        // its `+0x488` handle (`0x006FA04F`).
        if let Some(techno) = turret_rotate_sound::turret_rotate_sound_techno(id) {
            return self
                .substrate
                .entities
                .get(techno)
                .map(Self::movement_sound_world);
        }
        // `BuildingClass::Update` re-drives the `+0x6B4` working sound on an
        // unchanged visit (`building_working_sound`).
        if building_working_sound::working_sound_building(id).is_some() {
            return self.working_sound_owner_coord(id);
        }
        if let Some(coord) = self.anim_absolute_coord(id) {
            return Some(coord);
        }
        self.substrate
            .entities
            .get(id)
            .map(Self::movement_sound_world)
    }

    /// The object's coordinate for its sounds. Sound owners hold no terrain,
    /// so an object without an exact coordinate uses its stored level.
    /// The object's Location (`Object+0x9C`), where its sounds play.
    pub(crate) fn movement_sound_world(
        entity: &crate::sim::game_entity::GameEntity,
    ) -> crate::sim::anim_class::AnimWorldCoord {
        let [x, y] = crate::sim::movement::ground_pose::position_world_xy(&entity.position);
        crate::sim::anim_class::AnimWorldCoord {
            x,
            y,
            z: crate::sim::movement::ground_pose::object_world_z_leptons(entity, None),
        }
    }

    /// Accepted House result after its serialized SavourDelay has expired.
    /// Simulation termination and app outcome presentation share this query.
    /// Explicit solo wins/losses remain valid; the developer solo exception
    /// belongs only to automatic victory creation in the house rung.
    pub fn ready_outcome_for_owner(
        &self,
        owner: InternedId,
    ) -> Option<crate::sim::house_state::HouseOutcomeState> {
        self.houses
            .get(&owner)?
            .outcome()
            .filter(|outcome| outcome.exit_ready)
    }

    fn natural_outcome_exit_ready(&self) -> bool {
        self.session
            .current_house
            .is_some_and(|owner| self.ready_outcome_for_owner(owner).is_some())
    }

    fn termination_frame_requested(&self) -> bool {
        self.quit_requested || self.connection_lost || self.natural_outcome_exit_ready()
    }

    /// Advance the global ambient scalar before ore and active superweapons.
    /// A storm selecting Ion later in this frame can first move it next frame.
    /// `LogicClass::PerTickUpdate` (`0x0055B33D..0x0055B4D7`) restarts the
    /// fade timer with `NukeAmbientChangeRate=` while the nuke flash fades in
    /// or out (`0x0055B389`, `0x0055B392`), else with
    /// `DominatorAmbientChangeRate=` while the Psychic Dominator is active
    /// (`PsyDom::Active`, `0x0055B3A4`), else with `AmbientChangeRate=`. The
    /// chrono screen's test (`0x0055B39B`) shares the nuke arm. Original
    /// controls: superweapon_oracle.json/ambient_step, chrono=1.
    pub(crate) fn tick_scenario_lighting_transition(&mut self, rules: &RuleSet) {
        let (rate_nonzero, normal_interval, ambient_step) =
            self.session.lighting.ambient_change_terms(rules);
        let interval_frames = if self.session.lighting.nuke_flash_running()
            || self.session.chrono_screen().active()
        {
            self.session.lighting.nuke_change_rate()
        } else if crate::sim::superweapon::psychic_dominator::active(self) {
            self.session.lighting.dominator_change_rate
        } else {
            normal_interval
        };
        if self.session.lighting.advance_transition_if_due(
            self.session.binary_frame,
            rate_nonzero,
            interval_frames,
            ambient_step,
        ) {
            //55B4C6 refreshes Cell scalars without calling palette53AD00.
            self.publish_global_lighting(crate::sim::light_sources::PaletteUpdate::Preserve);
        }
    }

    fn tick_ore_growth_rungs(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        // Native TiberiumClass drivers run before the main live-object vector,
        // growth first and spread second.
        let native_growth_ready = !rules.tiberium_types.is_empty()
            && self
                .production
                .ore_growth_state
                .native_tiberium_state()
                .classes
                .len()
                == rules.tiberium_types.len()
            && self.overlay_grid.is_some()
            && overlay_registry.is_some();
        if native_growth_ready {
            let live_objects = TiberiumPlacementObjectContext::new(
                &self.substrate.entities,
                &self.substrate.occupancy,
                rules,
                &self.interner,
                &self.production.terrain_object_cells,
            );
            if let (Some(grid), Some(registry)) = (self.overlay_grid.as_mut(), overlay_registry) {
                let dirty_start = grid.pending_dirty_cells().len();
                self.production.ore_growth_state.tick_native_growth_driver(
                    grid,
                    registry,
                    &rules.tiberium_types,
                    self.resolved_terrain.as_ref(),
                    &self.production.tiberium_spawning_terrain_cells,
                    Some(live_objects),
                    &mut self.scenario_rng,
                    self.session.binary_frame,
                    self.production.ore_growth_config.grows,
                    self.production.ore_growth_config.spreads,
                    self.production.ore_growth_config.tiberium_grows_flag,
                    Some(&mut self.radar_terrain_dirty_cells),
                    Some(&mut self.radar_terrain_dirty_generation),
                    Some(&mut self.tactical_dirty_cells),
                );
                let spread = self.production.ore_growth_state.tick_native_spread_driver(
                    grid,
                    registry,
                    &rules.tiberium_types,
                    self.resolved_terrain.as_ref(),
                    &self.production.tiberium_spawning_terrain_cells,
                    Some(live_objects),
                    &mut self.scenario_rng,
                    self.session.binary_frame,
                    self.production.ore_growth_config.grows,
                    self.production.ore_growth_config.spreads,
                    Some(&mut self.radar_terrain_dirty_cells),
                    Some(&mut self.radar_terrain_dirty_generation),
                    Some(&mut self.tactical_dirty_cells),
                );
                // Original Place487190 -> Overlay Mark5FC570 -> Recalc47D2B0
                // publishes LandType before Logic55B4DC returns to live-object
                // AI. Within the ore loops, a stamped cell is excluded by its
                // overlay identity before admission reads its land; growth and
                // source gates use density/slope. Their Recalcs can therefore
                // publish as this batch without changing admission or RNG order.
                // Native cell results: tools/spatial_oracle/ore_queue.json.
                let marked = grid.pending_dirty_cells()[dirty_start..].to_vec();
                self.publish_tiberium_cells(rules, registry, &marked);
                // No identity allocator runs between these ordinary ore
                // constructors. Their stock CellAnim branch is absent. Apply
                // their ordered registry/ID effects before live-object AI;
                // Scenario IDs and retirement order are preserved independently
                // of the intervening Scenario RNG and queue mutations.
                self.publish_overlay_constructions(spread.into_overlay_constructions());
            }
        }
    }

    /// Overlay Mark5FC570 ->Recalc47D2B0 completes before its Logic caller
    /// returns. Both the global ore rung and a Terrain AI slot must publish
    /// the path view before the next object; retain render dirty receipts.
    pub(crate) fn publish_tiberium_cells(
        &mut self,
        rules: &RuleSet,
        registry: &crate::rules::overlay_types::OverlayTypeRegistry,
        cells: &[(u16, u16)],
    ) {
        if let (Some(grid), Some(terrain)) =
            (self.overlay_grid.as_mut(), self.resolved_terrain.as_mut())
        {
            for &cell in cells {
                if grid.cell(cell.0, cell.1).overlay_id.is_some() {
                    grid.recalculate_runtime_cell(terrain, registry, cell);
                }
            }
        }
        self.finish_terrain_navigation_changes(rules, &[]);
    }

    pub(crate) fn publish_overlay_constructions(
        &mut self,
        receipts: impl IntoIterator<Item = OverlayConstructionReceipt>,
    ) {
        for receipt in receipts {
            let stable_id = self.allocate_stable_id();
            let handle = self
                .load_objects
                .construct_overlay(stable_id, receipt.cell(), || {
                    crate::sim::native_identity::NativeUniqueIdCursor::assign_runtime(
                        &mut self.native_unique_ids,
                    )
                })
                .expect("runtime Overlay constructor must join its owned registries");
            self.load_objects
                .finish_cell_construction(handle, receipt, &mut self.substrate.pending_delete)
                .expect("runtime Overlay constructor must finish its owned lifecycle");
        }
    }

    /// `MapClass::In_Bounds @ 0x00568300` — the active diamond test against
    /// `MapClass+0xF4` (size width) and `MapClass+0xF8` (size height). No map
    /// size (a headless fixture) admits nothing.
    pub fn map_cell_in_bounds(&self, cell: (i16, i16)) -> bool {
        self.map_size_diamond().is_some_and(|(width, height)| {
            crate::map::playfield::size_diamond_contains(width, height, cell)
        })
    }

    /// `MapClass+0xF4/+0xF8`, the `In_Bounds` diamond's width and height.
    pub fn map_size_diamond(&self) -> Option<(i32, i32)> {
        self.playfield_bounds
            .zip(self.playfield_size_height)
            .map(|(bounds, height)| (bounds.base, height))
    }

    /// MapRect, `MapClass+0x124..+0x130` as `[left, top, width, height]`:
    /// `MapClass::Resize @ 0x00565C10` writes `(1, 1, SizeW + SizeH - 1,
    /// SizeW + SizeH - 1)`. Random cell draws read it
    /// (`MapClass::PlaceCrateAtRandomCell @ 0x0056BD8B`,
    /// `LightningStorm::Start @ 0x00539EE6`).
    pub(crate) fn map_rect(&self) -> Option<[i32; 4]> {
        let (width, height) = self.map_size_diamond()?;
        let extent = width.wrapping_add(height).wrapping_sub(1);
        Some([1, 1, extent, extent])
    }

    /// Install the initial normalized MapClass playfield authority.
    ///
    /// `MapClass::Set_Clipped_LocalSize @ 0x00567230` establishes the five
    /// predicate fields. Size height is retained separately because later
    /// action-40 writers normalize another raw LocalSize against the same Size.
    pub fn install_playfield_from_map_header(&mut self, header: &crate::map::map_file::MapHeader) {
        self.playfield_size_height = Some(header.height as i32);
        self.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds::from_map_header(
            header,
        ));
        self.playfield_revision = 0;
    }

    /// Apply YR trigger action 0x28's mutable visible-map-area authority.
    ///
    /// `FUN_006E21E0` writes LocalSize, normalizes it, recalculates all cells,
    /// rebuilds zone connectivity/all levels, and refreshes radar before it
    /// returns to `TriggerAction__Execute @ 0x006DD8B0`. Rust performs every
    /// corresponding owned update synchronously here, so later actions and
    /// the same master frame observe one coherent authority.
    pub fn change_visible_map_area(
        &mut self,
        raw_local_size: [i32; 4],
        rules: Option<&RuleSet>,
    ) -> bool {
        let (Some(current), Some(size_height)) =
            (self.playfield_bounds, self.playfield_size_height)
        else {
            // A live scenario always has MapClass Size authority. Headless
            // fixtures without a map must not invent a rectangular fallback.
            return false;
        };
        let bounds = crate::sim::cell_rect::PlayfieldBounds::from_raw_local_size(
            current.base,
            size_height,
            raw_local_size,
        );
        self.playfield_bounds = Some(bounds);
        // FUN_006E21E0 runs the complete radar-surface rebuild on every
        // execution. Do not deduplicate equal normalized writers.
        self.playfield_revision = self.playfield_revision.wrapping_add(1);

        // `MapClass::Set_Clipped_LocalSize @ 0x00567230` exact-recomputes the
        // canonical TechnoClass+0x3D5 byte for every represented Techno. This
        // must precede later action consumers and may both demote and promote;
        // ordinary per-cell movement's writer is deliberately only 0 -> 1.
        let membership_updates = self
            .substrate
            .entities
            .keys_sorted()
            .into_iter()
            .filter_map(|stable_id| {
                let entity = self.substrate.entities.get(stable_id)?;
                let member = crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                    (
                        i32::from(entity.position.rx()),
                        i32::from(entity.position.ry()),
                    ),
                    Some(bounds),
                    self.resolved_terrain.as_ref(),
                );
                let reveal = !entity.in_playfield()
                    && member
                    && entity.lifecycle.object_alive()
                    && !entity.lifecycle.in_limbo()
                    && !entity.death.is_dying()
                    && entity.health.current() > 0
                    && entity.category() != EntityCategory::Structure
                    && self
                        .houses
                        .get(&entity.owner())
                        .is_some_and(|house| house.is_human);
                Some((stable_id, member, reveal))
            })
            .collect::<Vec<_>>();
        // Set_Clipped_LocalSize's exact store (`0x0056730E`).
        for &(stable_id, member, _) in &membership_updates {
            if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                entity.in_playfield.store(member);
            }
        }
        // FUN_006E21E0's false->true callback is the Techno reveal/update
        // virtual (`0x0070ADC0`), only for human-owned live nonlimbo mobiles;
        // Buildings are explicitly excluded. Rust's owned equivalent commits
        // the sight reveal immediately, before this action returns.
        for (stable_id, _, reveal) in membership_updates {
            if reveal {
                self.reveal_uncached_object_sight(stable_id, UninitContext::new(rules, None));
            }
        }

        if let Some(terrain) = self.resolved_terrain.as_mut() {
            terrain.recalc_playfield_attributes(bounds);
        }

        // Rebuild from the retained structure-blocked PathGrid. PathGrid does
        // not cache the outside flag; ZoneGrid does, so a full rebuild covers
        // native connectivity plus every retained hierarchy level without
        // discarding dynamic building blockers.
        if let Some(path_grid) = self.path_grid.clone() {
            self.rebuild_zone_grid_full(path_grid.as_ref());
        }

        // Native calls RefreshRadar after the cell/zone rebuild. The distinct
        // playfield revision is the global geometry invalidation seam; the
        // presentation-acknowledged dirty-cell batch remains cell-local.
        true
    }

    /// Acknowledge one completed radar-terrain presentation update.
    ///
    /// Active `RadarClass` dirty processing at `0x00655250` drains and clears
    /// its retained cell list after the update. The generation guard prevents
    /// a stale client acknowledgement from clearing a newer producer batch.
    /// Both fields are presentation handoff state (`serde(skip)` and omitted
    /// from `state_hash`), so this consumption cannot mutate lockstep state.
    pub(crate) fn acknowledge_radar_terrain_dirty(&mut self, generation: u64) -> bool {
        if generation != self.radar_terrain_dirty_generation
            || self.radar_terrain_dirty_cells.is_empty()
        {
            return false;
        }
        self.radar_terrain_dirty_cells.clear();
        true
    }

    fn poll_triggers_for_master_frame(
        &mut self,
        inputs: TriggerInputs<'_>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        // YR LogicClass::Update polls scenario triggers before the live-object walk.
        let effects = self.advance_triggers(inputs, overlay_registry, frame_effects);
        self.publish_trigger_effects(effects);
    }

    /// Commit synchronous Tag outcomes to the existing frame-output boundary.
    /// Object callbacks and the master-frame poll share this single queue.
    pub(crate) fn publish_trigger_effects(&mut self, effects: Vec<TriggerEffect>) {
        self.trigger_effects.extend(effects);
    }

    /// Drain app-owned outcomes after their authoritative trigger actions ran.
    /// Production consumes trigger effects through `SimFrameOutput`; only
    /// fixture replay and trigger tests drain them directly.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn drain_trigger_effects(&mut self) -> Vec<TriggerEffect> {
        std::mem::take(&mut self.trigger_effects)
    }

    /// Enable or disable per-entity debug event logging (F10 boundary method:
    /// the app toggles, sim owns the write). Flips the spawn-time flag and
    /// reconciles every existing entity, so existing and future entities
    /// always agree with the toggle. Enabling preserves logs an entity
    /// already accumulated; disabling drops them all.
    #[cfg(any(feature = "dev-overlays", test))]
    pub fn set_debug_event_logging(&mut self, enabled: bool) {
        self.debug_event_logging = enabled;
        for entity in self.substrate.entities.values_mut() {
            entity.set_debug_logging(enabled);
        }
    }

    /// Tell the fog that presentation prepared a view for the named owner
    /// (F10 boundary method: the app requests, sim owns the write). Bumps the
    /// view generation render dirty-gates on. Returns false, and bumps
    /// nothing, when the owner name is not interned yet.
    pub fn prepare_fog_view_for(&mut self, owner: &str) -> bool {
        if self.interner.get(owner).is_none() {
            return false;
        }
        self.fog.bump_view_generation();
        true
    }

    /// Configure the command input delay (F10 boundary method; the app pushes
    /// its configured value once at match install).
    pub fn set_input_delay_ticks(&mut self, ticks: u64) {
        self.input_delay_ticks = ticks;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn take_master_frame_test_trace(&mut self) -> Vec<MasterFrameTestRung> {
        std::mem::take(&mut self.master_frame_test_trace)
    }

    #[cfg(any(test, feature = "test-support"))]
    fn trace_master_frame_rung(&mut self, rung: MasterFrameTestRung) {
        self.master_frame_test_trace.push(rung);
    }

    #[cfg(any(test, feature = "test-support"))]
    fn trace_house_ai_activation_order(&mut self, event: HouseAiActivationOrderTestEvent) {
        self.house_ai_activation_order_test_trace.push(event);
    }

    /// Shared identity source for every modeled runtime `AbstractClass` analogue.
    ///
    /// `AbstractClass::Create_ID @ 0x00410230` delegates to
    /// `ScenarioClass::NextUniqueID @ 0x0068BCB0`; individual stores therefore
    /// must not own independent counters.
    pub fn allocate_stable_id(&mut self) -> u64 {
        ObjectSubstrate::allocate_stable_id(
            &mut self.substrate.next_stable_object_id,
            &self.substrate.entities,
        )
    }

    /// CreateBullet46B050 without Fire: consumes one native constructor ID,
    /// retains the independent construction fields and joins Object expiry
    /// listeners through the existing store, but not Logic or Display.
    pub(crate) fn construct_projectile(
        &mut self,
        source: u64,
        target: crate::sim::projectile::ProjectileTarget,
        payload: crate::sim::projectile::ProjectilePayload,
        rules: &RuleSet,
    ) -> u64 {
        let native_id = crate::sim::native_identity::NativeUniqueIdCursor::assign_runtime(
            &mut self.native_unique_ids,
        );
        let id = self.allocate_stable_id();
        self.projectiles.construct(
            id,
            native_id,
            self.session.binary_frame,
            source,
            target,
            payload,
        );
        self.construct_bullet_scheme(id, rules);
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::UnfiredProjectileConstructed {
            stable_id: id,
            native_id,
            source_id: source,
            target,
            payload,
            position: self
                .projectiles
                .get(id)
                .expect("constructed Bullet")
                .position,
            in_logic: self
                .projectiles
                .get(id)
                .expect("constructed Bullet")
                .in_logic_vector,
            in_display: self.substrate.display.layer_of(id).is_some(),
        });
        id
    }

    pub fn admit_projectile(
        &mut self,
        stable_id: u64,
        spawn: crate::sim::projectile::ProjectileSpawn,
    ) -> u64 {
        let location = crate::sim::projectile::bullet_unlimbo_coord(
            self.resolved_terrain.as_ref(),
            &self.effective_shared_cell_dummy(),
            spawn.origin,
        );
        self.projectiles
            .spawn_at(stable_id, self.session.binary_frame, spawn, location);
        let registered = self.register_projectile(stable_id, spawn.flat);
        debug_assert!(registered);
        if let Some(style) = spawn.line_trail {
            self.lifecycle_outputs
                .push(LifecycleOutput::LineTrailConstructed { stable_id, style });
        }
        stable_id
    }

    /// `BulletClass::Construct @ 0x004664C0`, `0x00466519..0x0046653B`: a
    /// `FirersPalette=` BulletType (`+0x2A9`) whose Owner (`+0xB0`) exists
    /// keeps the Owner's House colour scheme at `+0x114`; every other bullet
    /// keeps -1. Runs after each admission and re-Construct.
    pub(crate) fn construct_bullet_scheme(&mut self, bullet: u64, rules: &RuleSet) {
        let Some(projectile) = self.projectiles.get(bullet) else {
            return;
        };
        let firers_palette = projectile
            .payload
            .projectile_type()
            .and_then(|id| rules.projectile(self.interner.resolve(id)))
            .is_some_and(|kind| kind.firers_palette);
        let house = firers_palette
            .then(|| self.substrate.entities.get(projectile.source_id))
            .flatten()
            .map(|owner| owner.owner());
        self.projectiles.set_firer_house(bullet, house);
    }

    /// Admit the bullets a detonation spawned (`BulletClass::SpawnShrapnel
    /// @ 0x0046A310`), each [`Self::construct_bullet_scheme`]d as every
    /// constructed bullet is. SpawnShrapnel then Fires each one (vt+0x1F0,
    /// `0x0046A875`, `0x0046AD29`), whose Inviso arm (`0x004688B7..0x00468986`)
    /// stands an `Inviso=` child on its target's vt+0x58, the coordinate the
    /// spawn captured ([`ProjectileStore::fire_inviso`]); its OnBridge stays
    /// clear, as only FireAt copies one.
    ///
    /// RESIDUAL: a child has no WeaponType. The constructor zeroes `+0x130`
    /// (`0x00466401`) and only SetWeaponType (`0x0046B260`, called from
    /// `0x0044CAFF`, `0x006CDB7E`, `0x006E34E6`, `0x006FE573` and
    /// `0x0070D764`) writes it, so a `FlakScatter=` child would fault reading
    /// its `Range=` in Fire's flak placement (`0x004687E5`). VERA stands it on
    /// its target. Trigger: a shrapnel weapon whose projectile is `Inviso=`
    /// and `FlakScatter=`; the five retail shrapnel weapons' projectiles are
    /// `Inviso=` without it. Effect: native crashes.
    ///
    /// [`ProjectileStore::fire_inviso`]: crate::sim::projectile::ProjectileStore::fire_inviso
    pub(crate) fn admit_projectile_spawns(
        &mut self,
        spawns: impl IntoIterator<Item = crate::sim::projectile::ProjectileSpawn>,
        rules: &RuleSet,
    ) {
        for projectile in spawns {
            let stable_id = self.allocate_stable_id();
            let inviso = projectile
                .payload
                .projectile_type()
                .and_then(|id| rules.projectile(self.interner.resolve(id)))
                .is_some_and(|kind| kind.inviso);
            let placement = projectile.initial_target_position;
            self.admit_projectile(stable_id, projectile);
            self.construct_bullet_scheme(stable_id, rules);
            #[cfg(test)]
            if let Some(fixture) = self.receiver_fixture.as_mut() {
                fixture
                    .admitted_spawns
                    .push((stable_id, inviso, projectile));
            }
            if inviso {
                self.projectiles.fire_inviso(stable_id, placement, false);
            }
        }
    }

    pub fn admit_wave(&mut self, stable_id: u64, wave: crate::sim::wave::Wave) -> u64 {
        self.waves.spawn(stable_id, wave);
        let registered = self.register_wave(stable_id);
        debug_assert!(registered);
        stable_id
    }

    // Native Reveal's append: +0x98 guard → tail-append → set flag. Idempotent.
    // Reveal/Conceal and raw LogicVector transactions live in lifecycle.rs.

    /// Debug-only invariant: the active order and each stored object's
    /// membership flag are two views of one set and must never disagree. The
    /// order must be duplicate-free, and its length must equal the number of
    /// objects whose `in_logic_vector` is set. O(n); compiled out of release.
    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_logic_membership_consistent(&self) {
        let order = self.substrate.logic.as_slice();
        let mut seen = std::collections::BTreeSet::new();
        for &id in order {
            let first_occurrence = seen.insert(id);
            debug_assert!(first_occurrence, "logic order has duplicate id {id}");
            debug_assert!(
                self.substrate
                    .entities
                    .get(id)
                    .is_some_and(|entity| entity.in_logic_vector.is_member())
                    || self
                        .substrate
                        .anims
                        .get(id)
                        .is_some_and(|anim| anim.in_logic_vector)
                    || self
                        .substrate
                        .voxel_anims
                        .get(id)
                        .is_some_and(|debris| debris.in_logic_vector)
                    || self
                        .substrate
                        .particle_systems
                        .get(id)
                        .is_some_and(|system| system.in_logic_vector)
                    || self
                        .production
                        .terrain_objects
                        .get(&id)
                        .is_some_and(|terrain| terrain.in_logic_vector)
                    || self
                        .projectiles
                        .get(id)
                        .is_some_and(|projectile| projectile.in_logic_vector)
                    || self.waves.get(id).is_some_and(|wave| wave.in_logic_vector),
                "logic order id {id} is missing or not membership-flagged",
            );
        }
        let flagged_entities = self
            .substrate
            .entities
            .values()
            .filter(|e| e.in_logic_vector.is_member())
            .count();
        let flagged_anims = self
            .substrate
            .anims
            .iter()
            .filter(|(_, anim)| anim.in_logic_vector)
            .count();
        let flagged_voxel_anims = self
            .substrate
            .voxel_anims
            .iter()
            .filter(|(_, debris)| debris.in_logic_vector)
            .count();
        let flagged_particle_systems = self
            .substrate
            .particle_systems
            .iter()
            .filter(|(_, system)| system.in_logic_vector)
            .count();
        let flagged_terrain = self
            .production
            .terrain_objects
            .values()
            .filter(|terrain| terrain.in_logic_vector)
            .count();
        let flagged_projectiles = self
            .projectiles
            .iter()
            .filter(|(_, projectile)| projectile.in_logic_vector)
            .count();
        let flagged_waves = self
            .waves
            .iter()
            .filter(|(_, wave)| wave.in_logic_vector)
            .count();
        let flagged = flagged_entities
            + flagged_anims
            + flagged_voxel_anims
            + flagged_particle_systems
            + flagged_terrain
            + flagged_projectiles
            + flagged_waves;
        debug_assert_eq!(
            order.len(),
            flagged,
            "logic order length ({}) != objects flagged in_logic_vector ({})",
            order.len(),
            flagged
        );
    }

    /// Release test builds have no debug assertions, so the check is a no-op
    /// there, like the `debug_assert!`s it holds. Tests call it without a cfg.
    #[cfg(all(test, not(debug_assertions)))]
    pub(crate) fn debug_assert_logic_membership_consistent(&self) {}

    /// Debug-only checks for relationships that remain true while the native
    /// lifecycle axes themselves are deliberately independent.
    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_lifecycle_consistent(&self) {
        for entity in self.substrate.entities.values() {
            if !entity.lifecycle.object_alive() {
                debug_assert!(
                    entity.lifecycle.in_limbo(),
                    "dead entity {} must have completed Conceal before alive clear",
                    entity.stable_id()
                );
                debug_assert!(
                    !entity.lifecycle.cell_marked(),
                    "dead entity {} must be unmarked before alive clear",
                    entity.stable_id()
                );
                debug_assert!(
                    !entity.in_logic_vector.is_member(),
                    "dead entity {} must leave LogicVector before alive clear",
                    entity.stable_id()
                );
                debug_assert!(
                    entity.kill_record.is_recorded(),
                    "dead entity {} must record its destruction exactly once before alive clear",
                    entity.stable_id()
                );
                debug_assert!(
                    self.substrate
                        .pending_delete
                        .contains_object(&entity.stable_id()),
                    "dead entity {} must remain pending until finalization",
                    entity.stable_id()
                );
            }
            if !entity.lifecycle.cell_marked() || entity.passenger_role.is_inside_transport() {
                continue;
            }
            // A marked Foot sits where its Mark put it: the Mark owner's own
            // list decision, which the Ground answer makes rules-independent.
            let listed = if entity.category() == EntityCategory::Structure {
                if crate::sim::occupancy::cell_list_layer_for_entity(
                    entity,
                    self.resolved_terrain.as_ref(),
                )
                .is_some()
                {
                    crate::sim::occupancy::entity_occupancy_cells(entity)
                } else {
                    Vec::new()
                }
            } else {
                self.foot_mark_cell(entity.stable_id())
                    .map(|(cell, _)| vec![cell])
                    .unwrap_or_default()
            };
            for (rx, ry) in listed {
                debug_assert!(
                    self.substrate
                        .occupancy
                        .contains_entity(rx, ry, entity.stable_id()),
                    "cell-marked entity {} missing occupancy at ({rx}, {ry})",
                    entity.stable_id()
                );
            }
        }
    }

    /// The active order, verbatim. No sorted-ID fallback (was DRIFT).
    ///
    /// This is a point-in-time copy: a consumer iterating it CANNOT observe an
    /// object registered or unregistered during its own pass. For native
    /// same-pass membership semantics use [`Self::for_each_live_object`].
    pub fn live_object_order_snapshot(&self) -> Vec<u64> {
        self.substrate.logic.snapshot()
    }

    /// Forward pass over the active-object order that RE-READS the live length
    /// after every body call.
    ///
    /// Consequences (the native scheduler contract):
    /// - An object the body tail-appends via `register_live_object` runs later
    ///   in the SAME pass (the length grows before the cursor reaches it).
    /// - A compacting `unregister_live_object` shifts successors left while the
    ///   cursor still advances by one, so the object pulled into the just-
    ///   processed slot is skipped this pass. There is no index repair.
    /// - Each member is visited at most once per pass; the index only advances.
    ///
    /// The body must tolerate an id whose entity is absent — there is no item
    /// guard here. `uninit` always conceals before freeing the store
    /// slot, so the order never references a removed entity in practice.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn for_each_live_object<F: FnMut(&mut Simulation, u64)>(&mut self, mut body: F) {
        let result: Result<(), std::convert::Infallible> =
            self.try_for_each_live_object(|sim, id| {
                body(sim, id);
                Ok(())
            });
        match result {
            Ok(()) => {}
            Err(never) => match never {},
        }
    }

    /// The same live cursor, with failure stopping before the next object.
    /// Prior callback writes remain in the world; this is not a rollback.
    pub(crate) fn try_for_each_live_object<E>(
        &mut self,
        mut body: impl FnMut(&mut Simulation, u64) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut i = 0;
        while i < self.substrate.logic.len() {
            let id = self.substrate.logic.as_slice()[i];
            #[cfg(any(test, feature = "test-support"))]
            let _draw_context = crate::sim::rng::observe_logic_object(id);
            body(self, id)?;
            i += 1;
        }
        Ok(())
    }

    /// Debug-only P5b invariants on the now-authoritative registry (repurposed from the
    /// P5a inversion-readiness assert — the legacy upfront charge it compared against is
    /// retired, so the comparison is gone). Read-only; SURFACES divergence with
    /// tick+owner+category; NEVER writes back.
    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_factory_invariants(&self) {
        use crate::sim::production::PRODUCTION_STEPS;

        // (A) ORDER: the registry sweep order must be a TOTAL order — `iter_insertion_ordered`
        // yields strictly-increasing `insertion_seq` with NO ties (the strict-monotonic
        // `enqueue_order` property the hash fold + `step_all` charge order both depend on; a
        // tie would make the sweep order ambiguous and desync lockstep). Within each factory
        // the tail stamps strictly increase AND exceed the factory's `insertion_seq`, its
        // construction stamp (FIFO `push_back` of a monotonic mint: every queued build was
        // stamped after the factory was made, and a promotion keeps the factory's stamp).
        let ordered = self.production.factories.iter_insertion_ordered();
        let mut prev_seq: Option<u64> = None;
        for f in &ordered {
            if let Some(p) = prev_seq {
                debug_assert!(
                    f.insertion_seq > p,
                    "P5d (A): tick {}: insertion_seq must be strictly increasing across the sweep ({} after {})",
                    self.session.tick,
                    f.insertion_seq,
                    p,
                );
            }
            prev_seq = Some(f.insertion_seq);
            let mut tail_prev = f.insertion_seq;
            for e in &f.queue {
                debug_assert!(
                    e.enqueue_order > tail_prev,
                    "P5d (A): tick {} {:?}/{:?}: tail enqueue_order must strictly exceed the active build + prior tail ({} after {})",
                    self.session.tick,
                    f.owner,
                    f.category,
                    e.enqueue_order,
                    tail_prev,
                );
                tail_prev = e.enqueue_order;
            }
        }

        // (B) STATE: progress in 0..=54; balance >= 0 (the seed is non-negative, the
        // per-step ladder only decrements it, and cancel resets it to 0).
        for f in self.production.factories.iter_insertion_ordered() {
            debug_assert!(
                f.progress <= PRODUCTION_STEPS,
                "P5b (B): tick {} {:?}/{:?}: progress {} exceeds {}",
                self.session.tick,
                f.owner,
                f.category,
                f.progress,
                PRODUCTION_STEPS,
            );
            debug_assert!(
                f.balance >= 0,
                "P5b (B): tick {} {:?}/{:?}: balance {} is negative",
                self.session.tick,
                f.owner,
                f.category,
                f.balance,
            );
        }
    }

    /// Test-only: force the active order and sync membership flags to it.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_logic_order_for_test(&mut self, order: Vec<u64>) {
        for entity in self.substrate.entities.values_mut() {
            entity.in_logic_vector.mark(false);
        }
        for anim in self.substrate.anims.values_mut() {
            anim.in_logic_vector = false;
        }
        for debris in self.substrate.voxel_anims.values_mut() {
            debris.in_logic_vector = false;
        }
        for (_, system) in self.substrate.particle_systems.iter_mut() {
            system.in_logic_vector = false;
        }
        for terrain in self.production.terrain_objects.values_mut() {
            terrain.in_logic_vector = false;
        }
        for (_, projectile) in self.projectiles.iter_mut() {
            projectile.in_logic_vector = false;
        }
        for (_, wave) in self.waves.iter_mut() {
            wave.in_logic_vector = false;
        }
        for &id in &order {
            if let Some(e) = self.substrate.entities.get_mut(id) {
                e.in_logic_vector.mark(true);
            } else if let Some(anim) = self.substrate.anims.get_mut(id) {
                anim.in_logic_vector = true;
            } else if let Some(debris) = self.substrate.voxel_anims.get_mut(id) {
                debris.in_logic_vector = true;
            } else if let Some(system) = self.substrate.particle_systems.get_mut(id) {
                system.in_logic_vector = true;
            } else if let Some(terrain) = self.production.terrain_objects.get_mut(&id) {
                terrain.in_logic_vector = true;
            } else if let Some(projectile) = self.projectiles.get_mut(id) {
                projectile.in_logic_vector = true;
            } else if let Some(wave) = self.waves.get_mut(id) {
                wave.in_logic_vector = true;
            }
        }
        self.substrate.logic.set_order_for_test(order);
    }

    /// Common VoxelAnim7493B0 constructor/admission for typed Techno and
    /// Bullet debris. Create_ID74944E precedes launch74951D..749559 and
    /// Bounce4397E0 RNG; admission finishes before the next typed selector.
    /// `admit_inline=false` is the caller's existing no-callback numeric-fixture
    /// boundary: its packet retains both identities for later registration.
    pub fn emit_debris_voxel(
        &mut self,
        type_id: crate::rules::voxel_anim_type::VoxelAnimTypeId,
        voxel_type: &crate::rules::voxel_anim_type::VoxelAnimType,
        owner_house: Option<InternedId>,
        origin: glam::IVec3,
        admit_inline: bool,
        packets: &mut Vec<crate::sim::voxel_anim::VoxelDebrisSpawn>,
    ) -> Result<(), crate::util::native_x87::NativeX87Error> {
        let stable_id = self.allocate_stable_id();
        let native_unique_id = self.next_native_runtime_id();
        let object = crate::sim::voxel_anim::spawn_debris_piece(
            stable_id,
            native_unique_id,
            type_id,
            voxel_type,
            owner_house,
            origin,
            &mut self.scenario_rng,
        )?;
        let spawn = crate::sim::voxel_anim::VoxelDebrisSpawn { object };
        if admit_inline {
            self.admit_death_debris(std::iter::once(spawn));
        } else {
            packets.push(spawn);
        }
        Ok(())
    }

    /// Register already-constructed debris. This also delivers numeric-fixture
    /// packets; neither stable nor native identity is allocated a second time.
    pub(crate) fn admit_death_debris(
        &mut self,
        spawns: impl IntoIterator<Item = crate::sim::voxel_anim::VoxelDebrisSpawn>,
    ) {
        for spawn in spawns {
            let stable_id = spawn.object.stable_id;
            self.substrate.voxel_anims.insert(spawn.object);
            self.reveal_voxel_anim(stable_id);
        }
    }

    /// `HouseClass::IsControlledByHuman @ 0x0050B730` for `owner`'s house;
    /// false for an owner without one.
    pub fn owner_is_human(&self, owner: InternedId) -> bool {
        self.houses
            .get(&owner)
            .is_some_and(|house| house.is_controlled_by_human(self.session.game_mode_nonzero))
    }

    /// Apply `house_tracking`'s Add_Tracking or Remove_Tracking for an object
    /// on its owner's house.
    pub(crate) fn update_house_tracking(
        &mut self,
        stable_id: u64,
        update: fn(
            &mut crate::sim::house_tracking::HouseTracking,
            &mut crate::sim::game_entity::GameEntity,
        ),
    ) {
        let Some(owner) = self
            .substrate
            .entities
            .get(stable_id)
            .map(crate::sim::game_entity::GameEntity::owner)
        else {
            return;
        };
        if let (Some(entity), Some(house)) = (
            self.substrate.entities.get_mut(stable_id),
            self.houses.get_mut(&owner),
        ) {
            update(&mut house.tracking, entity);
        }
    }

    /// `BuildingClass::OnConstructionComplete`'s self-heal grant
    /// (`0x00446382..0x004463B4`): a structure joins its owner's house with the
    /// type's `InfantryGainSelfHeal` (`+0x1564` → house `+0x164`) and
    /// `UnitsGainSelfHeal` (`+0x1568` → house `+0x168`). Each arm skips its add
    /// when the type's count is zero; both use a plain wrapping `add`.
    pub(crate) fn grant_house_self_heal(&mut self, stable_id: u64, rules: &RuleSet) {
        let Some((owner, infantry, units)) = self.structure_self_heal_gain(stable_id, rules) else {
            return;
        };
        if infantry == 0 && units == 0 {
            return;
        }
        if let Some(house) = self.houses.get_mut(&owner) {
            house.grant_self_heal(infantry, units);
        }
    }

    /// `BuildingClass::Limbo`'s self-heal share (`0x004459AE..0x004459CA`, the
    /// unit arm at `0x004459E0`): subtract the type's counts from the owner's
    /// house, then raise a negative count to zero.
    pub(crate) fn remove_house_self_heal(&mut self, stable_id: u64, rules: &RuleSet) {
        let Some((owner, infantry, units)) = self.structure_self_heal_gain(stable_id, rules) else {
            return;
        };
        if let Some(house) = self.houses.get_mut(&owner) {
            house.revoke_self_heal(infantry, units);
        }
    }

    /// `BuildingClass::ChangeOwner`'s self-heal share
    /// (`0x00448AC8..0x00448B04` for infantry, `0x00448B0A..0x00448B46` for
    /// units): **the old owner loses the type's counts**, and a negative result
    /// is raised to zero. Both arms first test the type's count and the
    /// building's own `+0x6E4` "actually placed on the map" byte.
    ///
    /// There is no new-owner add in that block. `ChangeOwner` gives the counts to
    /// the new house through the virtual `Grand_Opening(1)` it calls at
    /// `0x00448CEF`, whose entry (`0x00445F80`, `mov al,[ebp+0x6E4]`) reaches the
    /// add at `0x00446382..0x004463B4` for the current owner — so this function
    /// must not credit anyone, and its caller must run it with the *passed* old
    /// owner: `TechnoClass::ChangeOwner` has already written the new owner by
    /// then, which is why an earlier version that read the entity's current
    /// owner back never took the counters off the house that lost the building.
    pub(crate) fn remove_old_owner_self_heal(
        &mut self,
        stable_id: u64,
        old_owner: InternedId,
        rules: &RuleSet,
    ) {
        let Some((_, infantry, units)) = self.structure_self_heal_gain(stable_id, rules) else {
            return;
        };
        let placed = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.building_art.actually_placed());
        if !placed || (infantry == 0 && units == 0) {
            return;
        }
        if let Some(house) = self.houses.get_mut(&old_owner) {
            house.revoke_self_heal(infantry, units);
        }
    }

    /// Owner, `InfantryGainSelfHeal` and `UnitsGainSelfHeal` of a structure,
    /// or `None` for a missing entity or a non-structure.
    fn structure_self_heal_gain(
        &self,
        stable_id: u64,
        rules: &RuleSet,
    ) -> Option<(InternedId, i32, i32)> {
        let entity = self.substrate.entities.get(stable_id)?;
        if entity.category() != EntityCategory::Structure {
            return None;
        }
        let object = self.object_type(entity.type_ref(), rules)?;
        Some((
            entity.owner(),
            object.infantry_gain_self_heal,
            object.units_gain_self_heal,
        ))
    }

    /// `HouseClass::Added_To_Game @ 0x00502A80` for an object on its owner's
    /// house, priced with that house's current `Cost_Of` factors. It counts
    /// no powered center and invalidates no power; the first GrandOpening
    /// owns that invalidation.
    pub(crate) fn house_added_to_game(&mut self, stable_id: u64) {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        if let Some(house) = self.houses.get_mut(&entity.owner()) {
            let factors = house.cost_factors();
            house.tracking.added_to_game(entity, &factors);
        }
    }

    /// `HouseClass::Removed_From_Game @ 0x005025F0` for an object on its
    /// owner's house, priced like [`Self::house_added_to_game`]. Between its
    /// sidebar update and its class switch, a counted `PowersUnit=` building
    /// stops counting (`0x0050263B..0x00502776`,
    /// [`Self::powered_center_removed`]). The prefix counters precede that
    /// callback; class/type/capacity reads follow it on the same receiving House.
    /// A Building invalidates its house's power regardless of
    /// DontScore (`0x005027FA`).
    pub(crate) fn house_removed_from_game(
        &mut self,
        stable_id: u64,
        rules: Option<&RuleSet>,
        effects: FrameEffects<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        let owner = entity.owner();
        let structure = entity.category() == EntityCategory::Structure;
        if let Some(house) = self.houses.get_mut(&owner) {
            house.tracking.removed_from_game_prefix(entity);
        }
        self.powered_center_removed(stable_id, rules, effects);
        //50277B resumes after the callback prelude. Pin the original House
        // receiver, but reborrow the live object/type at this actual phase.
        if let Some(entity) = self.substrate.entities.get(stable_id)
            && let Some(house) = self.houses.get_mut(&owner)
        {
            let factors = house.cost_factors();
            house.tracking.removed_from_game(entity, &factors);
        }
        if structure {
            self.invalidate_house_power(owner, false);
        }
    }

    /// Private retained House dirty bytes, shared by their represented native
    /// writers. An absent House cannot acquire derived state from a callback.
    pub(crate) fn invalidate_house_power(&mut self, owner: InternedId, radar: bool) {
        if self.houses.contains_key(&owner) {
            self.power_states
                .entry(owner)
                .or_default()
                .invalidate(radar);
        }
    }

    /// Inline native House+1FC writers request the existing House tail;
    /// power/radar dirty bytes remain with their distinct PowerState owner.
    pub(crate) fn request_house_tech_tree_recheck(&mut self, owner: InternedId) {
        if let Some(house) = self.houses.get_mut(&owner) {
            house.request_tech_recheck();
        }
    }

    /// BuildingAI440042..44006C samples live health before the common Techno
    /// tail. Repair701410 itself leaves both House dirty bytes untouched.
    pub(super) fn sample_building_health_for_house_update(&mut self, id: u64) {
        let owner = self.substrate.entities.get_mut(id).and_then(|entity| {
            entity
                .sample_building_health_for_power()
                .then_some(entity.owner())
        });
        if let Some(owner) = owner {
            self.invalidate_house_power(owner, true);
        }
    }

    /// House4F844B..4F8506: assess dirty power, rewrite all owned Factory
    /// rates without touching timers, then refresh the radar projection.
    /// Numeric/order evidence: the engineer-repair native consumer packet.
    fn assess_house_derived_state(&mut self, owner: InternedId, rules: &RuleSet) {
        if !self.houses.contains_key(&owner) {
            return;
        }
        let state = self.power_states.entry(owner).or_default();
        let was_low_power = state.is_low_power;
        let assessed = power_system::assess_house_power(
            state,
            &self.substrate.entities,
            rules,
            owner,
            &self.interner,
            self.session.binary_frame,
        );
        if assessed {
            production::refresh_factory_rates_for_house(self, rules, owner);
            // Power508DD5 compares the old/current ratio;508DDB invokes the
            // shared hold/revoke before the radar tail, only on that edge.
            if self.power_states[&owner].is_low_power != was_low_power {
                crate::sim::superweapon::update_owned_supers(self, rules, owner);
            }
        }
        // 0x004F84FB: once the current house is defeated no House rechecks
        // radar, so the radar its defeat switched on stays on.
        if self.local_player_defeated() {
            self.power_states
                .get_mut(&owner)
                .expect("represented House power state")
                .skip_radar_recheck();
            return;
        }
        let buildings = self.houses[&owner].base_projection.buildings();
        power_system::assess_house_radar_projection(
            self.power_states
                .get_mut(&owner)
                .expect("represented House power state"),
            &self.substrate.entities,
            buildings,
            rules,
            owner,
            &self.interner,
            self.session.free_radar,
            self.session.binary_frame,
        );
    }

    /// Append one successfully committed BuildConst Building to its owning
    /// House's native acquisition-ordered vector.
    ///
    /// gamemd-derived: `BuildingClass::Unlimbo @ 0x004411B6..0x00441223`.
    pub(crate) fn append_live_build_const(&mut self, stable_id: u64) {
        let Some(owner) = self.substrate.entities.get(stable_id).and_then(|entity| {
            (entity.category() == EntityCategory::Structure
                && entity.type_facts.build_const_eligible()
                && entity.lifecycle.object_alive()
                && !entity.lifecycle.in_limbo()
                && entity.lifecycle.cell_marked())
            .then_some(entity.owner())
        }) else {
            return;
        };
        let _appended = if let Some(house) = self.houses.get_mut(&owner)
            && !house.build_const_order.contains(&stable_id)
        {
            house.build_const_order.push(stable_id);
            true
        } else {
            false
        };
        #[cfg(any(test, feature = "test-support"))]
        if _appended {
            self.trace_lifecycle_for_test(LifecycleTestEvent::BuildConstAppended { stable_id });
        }
    }

    /// Stable-remove a Building pointer from the owning House's BuildConst
    /// vector before Limbo or owner transfer makes it unavailable.
    ///
    /// gamemd-derived: House pointer expiry around `0x004FBA1D` and
    /// `BuildingClass::ChangeOwner @ 0x00448260`.
    pub(crate) fn remove_build_const_from_owner(&mut self, stable_id: u64) {
        let Some((owner, eligible)) = self
            .substrate
            .entities
            .get(stable_id)
            .map(|entity| (entity.owner(), entity.type_facts.build_const_eligible()))
        else {
            return;
        };
        if eligible
            && let Some(house) = self.houses.get_mut(&owner)
            && let Some(index) = house
                .build_const_order
                .iter()
                .position(|&id| id == stable_id)
        {
            house.build_const_order.remove(index);
        }
    }

    /// Change an entity's owner through the authoritative ownership chokepoint.
    /// House counts, the `by_owner` index, and the entity owner move exactly once
    /// for every live transfer, regardless of whether capture or garrison code
    /// requested it.
    #[cfg(any(test, feature = "test-support"))]
    pub fn change_owner(&mut self, stable_id: u64, new_owner: InternedId) {
        self.change_owner_impl(stable_id, new_owner, None, None, FrameEffects::default());
    }

    /// Original House502D30: first matching registration, not owner-name order.
    /// Country identity remains RuleSet-owned; duplicate-Country Houses are valid.
    pub(crate) fn house_by_country_index(
        &self,
        country: crate::rules::ruleset::CountryIdx,
        rules: &RuleSet,
    ) -> Option<InternedId> {
        self.session.house_order.iter().copied().find(|owner| {
            self.houses
                .get(owner)
                .and_then(|house| house.country)
                .and_then(|id| rules.country_index(self.interner.resolve(id)))
                == Some(country)
        })
    }

    /// Original House510F60 -> Scenario510ED0, otherwise502D30. Only the
    /// literal Player@A..H selectors0x117B..0x1182 use the start-slot mapping.
    /// Executed Steam3e81a617..08600 controls: trigger_all_change_house.json.
    pub(crate) fn house_by_trigger_selector(
        &self,
        selector: i32,
        rules: &RuleSet,
    ) -> Option<InternedId> {
        if (0x117B..=0x1182).contains(&selector) {
            self.session
                .start_slot_houses
                .get(&((selector - 0x117B) as u32))
                .copied()
        } else {
            let country = u16::try_from(selector).ok()?;
            self.house_by_country_index(crate::rules::ruleset::CountryIdx(country), rules)
        }
    }

    pub(crate) fn change_owner_with_rules(
        &mut self,
        stable_id: u64,
        new_owner: InternedId,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        self.change_owner_impl(stable_id, new_owner, Some(rules), registry, effects);
    }

    fn change_owner_impl(
        &mut self,
        stable_id: u64,
        new_owner: InternedId,
        rules: Option<&RuleSet>,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some((old_owner, category, has_spawn_manager, build_const_eligible)) =
            self.substrate.entities.get(stable_id).map(|entity| {
                (
                    entity.owner(),
                    entity.category(),
                    entity.spawn_manager.is_some(),
                    entity.type_facts.build_const_eligible(),
                )
            })
        else {
            return;
        };
        if old_owner == new_owner {
            return;
        }
        // `BuildingClass::ChangeOwner` first (`0x00448277..0x0044828E`): a
        // building changing hands loses its bomb unless it is
        // `CanBeOccupied=`. A rules-less transfer (tests only) cannot read
        // the type and keeps it.
        if category == EntityCategory::Structure
            && let Some(rules) = rules
            && let Some(entity) = self.substrate.entities.get(stable_id)
            && !self
                .interner
                .object_type(entity.type_ref(), rules)
                .is_some_and(|object| object.can_be_occupied)
        {
            self.bomb_defuse(stable_id);
        }
        if category == EntityCategory::Structure {
            //448260: gap removal precedes ordinary sight release and owner
            //swap. This order differs from Techno Limbo's sight-then-gap.
            // `ReReveal(0, 0, 0, 0)` (`0x00448BD9`) releases the old owner's
            // sight only: what a `RevealToAll=` opening showed the current
            // house stays.
            self.remove_building_gap_before_limbo(stable_id);
            self.release_object_sight_with_context(stable_id, UninitContext::new(rules, None));
            // Original4483A6 clears PrimaryFactory before the old-house
            //teardown.448C69 initializes it after the delegated owner swap.
            self.production.clear_primary_factory(stable_id);
        }
        // FootClass::ChangeOwner @ 0x004DBED0 removes from the deposited old
        // owner and adds to the new owner before later readers observe it.
        if let Some(rules) = rules {
            self.transfer_sensor_before_owner_change_with_rules(
                stable_id, new_owner, rules, registry,
            );
        } else {
            self.transfer_sensor_before_owner_change(stable_id, new_owner);
        }

        // Active YR chain: BuildingClass::ChangeOwner (0x00448260) delegates
        // to TechnoClass::ChangeOwner (0x007014A0), which moves the house
        // counts (`house_tracking`) below.

        // `BuildingClass::ChangeOwner @ 0x004482AA..0x004482F9`, still on the
        // OLD owner: a `MultiplayPassive` old owner and a non-zero
        // `ProduceCashStartup` credit the NEW owner and arm the ProduceCash
        // timer (oil derricks). Runs ahead of the Techno owner swap like the
        // native, before the BuildConst/count moves below. A rules-less
        // transfer (tests only) cannot read the type and grants nothing.
        if let Some(rules) = rules {
            crate::sim::credit_income::produce_cash_on_owner_change(
                self, stable_id, old_owner, new_owner, rules,
            );
        }
        // Building4484AF precedes the delegated Techno owner transfer.
        self.enable_building_after_owner_change(stable_id, rules);
        // `0x00448559..0x004486C8`: the objects docked at a building go along
        // with it or are sent off. A rules-less transfer (tests only) cannot
        // read the type and keeps every link.
        let taken_along = match rules {
            Some(rules) if category == EntityCategory::Structure => self
                .take_docked_along_on_owner_change(stable_id, new_owner, rules, registry, effects),
            _ => Vec::new(),
        };
        // `0x004486DF..0x00448701`: the building's own factory is abandoned
        // for its old owner.
        if category == EntityCategory::Structure {
            crate::sim::production::detach_building_factory(self, rules, stable_id, effects);
        }
        // gamemd-derived: `BuildingClass::ChangeOwner @ 0x00448260` removes
        // this pointer from the old House BuildConst vector before delegating
        // the Techno owner swap, then appends it to the new House tail.
        if build_const_eligible {
            self.remove_build_const_from_owner(stable_id);
        }
        if category == EntityCategory::Structure {
            self.leave_house_base_lists(stable_id, old_owner);
        }
        // Original7014DB/7014E9 dispatch the class target and destination
        // setters, then70151A clears Archive and70156E queues Guard. All run
        // on the OLD house, before SpawnManager and the701735 owner swap.
        // The late70182F/70183B setters remain below on the new house.
        if let Some(rules) = rules {
            use crate::sim::mission::{MissionId, MissionType};
            let entity = self.substrate.entities.get(stable_id).unwrap();
            let current = entity.mission.current().known();
            let unit_deploying = category == EntityCategory::Unit
                && entity.mission_leaf.as_unit().is_some_and(|leaf| {
                    leaf.deploy_begin_active() != 0 || leaf.deploy_reverse_active() != 0
                });
            let simple_deployer_unloading = category == EntityCategory::Unit
                && current == Some(MissionType::Unload)
                && self
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| object.is_simple_deployer);
            let _ = self.assign_target_represented(stable_id, None, Some(rules), registry, effects);
            self.assign_null_destination(stable_id, Some(rules), None, effects);
            if !unit_deploying && let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                entity.set_archive_target(None);
            }
            if current != Some(MissionType::Selling) && !simple_deployer_unloading {
                // `Queue_Mission(Guard, 1)`: the queue write, then Ready_To_Commence
                // (`+0x200`) and Commence (`+0x1EC`).
                let _ = self.mission_queue_exact(
                    stable_id,
                    MissionId::from_known(MissionType::Guard),
                    1,
                    self.session.binary_frame,
                    &crate::sim::mission::authority::LiveReadyInputProvider { rules },
                );
            }
        }

        // `TechnoClass::ChangeOwner` calls `SpawnManagerClass::Kill_All_Spawns`
        // before the house swap: a mind-controlled V3/Dreadnought/Boomer loses
        // the pool it built for its old owner, and a Carrier's airborne
        // Hornets crash. The children are destroyed while still
        // attributed to the previous house. The owner is still alive here, so
        // the slots re-arm with a zero regen wait and the new owner's pool is
        // rebuilt on the next manager pass.
        if has_spawn_manager {
            crate::sim::spawn_manager::kill_all_spawns_with_context(
                self,
                stable_id,
                UninitContext::new(rules, registry).with_effects(effects),
            );
        }
        // Techno701585 -> Planning638840 removes this actor's path before
        // the house swap. Already generated command events remain queued.
        self.planning_detach_actor(stable_id);
        // Techno70158A..7015E6: Removed_From_Game on the old house (not in
        // limbo), then Remove_Tracking from it and Add_Tracking to the new.
        let on_map = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| !entity.lifecycle.in_limbo());
        if on_map {
            self.house_removed_from_game(stable_id, rules, effects);
        }
        //7015A8 calls RecordTheKill(NULL) while the old owner is still
        //installed, then7015D2 adds CostOf(oldHouse) to the new owner's
        //score even when DontScore suppressed RecordTheKill itself.
        if let Some(rules) = rules {
            self.record_the_kill(
                stable_id,
                crate::sim::combat::KillSource::Techno(None),
                crate::sim::combat::KillCallback::OwnerChange,
                UninitContext::new(Some(rules), registry).with_effects(effects),
            );
            let cost = self
                .substrate
                .entities
                .get(stable_id)
                .and_then(|entity| self.object_type(entity.type_ref(), rules))
                .map_or(0, |object| self.cost_of(old_owner, object, rules));
            if let Some(house) = self.houses.get_mut(&new_owner) {
                house.economy.add_score(cost);
            }
        }
        self.update_house_tracking(
            stable_id,
            crate::sim::house_tracking::HouseTracking::remove_tracking,
        );
        //7015E6 receives the NEW House before the object owner changes.
        //Call the one native Add_Tracking owner with that explicit receiver.
        if let Some(entity) = self.substrate.entities.get_mut(stable_id)
            && let Some(house) = self.houses.get_mut(&new_owner)
        {
            house.tracking.add_tracking(entity);
            //701607..70164D: DontScore suppresses this captured-object
            //count, not the preceding old-House price addition.
            if !entity.tracking_facts.dont_score() {
                house.stats.record_kill(category);
            }
        }
        //701674 Mark(UP),70D4A0 detach and701691 Mark(DOWN) all precede
        //the owner store. PlaceDown's Cell discovery therefore observes the
        //old House and can dirty its radar as well as its power.
        let mark_context = UninitContext::new(rules, registry).with_effects(effects);
        if on_map {
            self.unmark_entity_remove(stable_id, mark_context);
        }
        // `TechnoClass::ChangeOwner` runs the live-detach targeting sweep next,
        // before the house swap: everything shooting at this object is released
        // while the object still belongs to its old house. Engineer capture and
        // garrison transfer both come through here, so a squad that was firing
        // at a building stops the instant the building changes hands instead of
        // shooting at what is now its own structure.
        self.stop_all_targeting_on_detach(stable_id, rules, registry, effects);
        if on_map {
            self.mark_entity_put(stable_id, mark_context);
        }
        //Techno701701..719 removes from the old owner using the retained
        //Cell. The same Cell survives the owner write for701774..780.
        let spatial_threat_cell =
            rules.and_then(|_| self.spatial_threat_before_owner_change(stable_id));
        self.substrate.entities.change_owner(stable_id, new_owner);
        if let Some(rules) = rules {
            self.spatial_threat_after_owner_change(stable_id, spatial_threat_cell, rules);
        }
        if category == EntityCategory::Structure {
            self.move_house_base_tracking(stable_id, old_owner, new_owner);
        }
        // `BuildingClass::ChangeOwner @ 0x00448723` marks every transferred
        // building HasBeenCaptured (+0x6E3); survivors read it at death. Its
        // repair stops without a sound (`+0x6E8 = 0`, `0x00448CE8`).
        if category == EntityCategory::Structure
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
        {
            entity.has_been_captured.mark();
            entity.repairing.stop_on_capture();
        }
        // `0x00448C6E..0x00448C94`, after the owner swap: the old house
        // records the building it lost (House+54E0), and the new house
        // forgets it if it had lost it before.
        if category == EntityCategory::Structure {
            if let Some(house) = self.houses.get_mut(&old_owner) {
                house.record_lost_building(stable_id);
            }
            if let Some(house) = self.houses.get_mut(&new_owner) {
                house.forget_lost_building(stable_id);
            }
        }
        // A factory leaves the old house's counter (Recount, `0x0044870E`)
        // and joins the new one's (IncrementFactoryCount, `0x00448CDD`),
        // whether it is in limbo or not.
        if let Some(entity) = self.substrate.entities.get(stable_id) {
            if let Some(house) = self.houses.get_mut(&old_owner) {
                house.tracking.recount(entity);
            }
            if let Some(house) = self.houses.get_mut(&new_owner) {
                house.tracking.increment_factory_count(entity);
            }
        }
        // A Helipad's docks leave the old house's AirportDocks
        // (`0x00448B4C..0x00448B6A`) and join the new one's
        // (`0x00449229..0x00449245`), whether it has opened or not.
        if category == EntityCategory::Structure
            && let Some(rules) = rules
            && let Some(docks) = self
                .substrate
                .entities
                .get(stable_id)
                .and_then(|entity| self.object_type(entity.type_ref(), rules))
                .filter(|object| object.helipad)
                .map(|object| object.number_of_docks)
        {
            for (owner, delta) in [(old_owner, docks.wrapping_neg()), (new_owner, docks)] {
                if let Some(house) = self.houses.get_mut(&owner) {
                    house.tracking.add_airport_docks(delta);
                }
            }
        }
        // A hospital's or machine shop's self-heal counts leave the old house
        // here (`0x00448AC8..0x00448B04`, `0x00448B0A..0x00448B46`, each arm
        // gated on the type's count and the building's own `+0x6E4` byte). The
        // new house gains them from the `Grand_Opening(1)` below — the one
        // `ChangeOwner` calls at `0x00448CEF` — not from this block, so the
        // counters move exactly once.
        if category == EntityCategory::Structure
            && let Some(rules) = rules
        {
            self.remove_old_owner_self_heal(stable_id, old_owner, rules);
        }
        // Techno701735..701751 writes the owner then recomputes only +41A.
        // A former current-house object's +41B history survives the transfer.
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity
                .discovery
                .classify_owner(self.session.current_house == Some(new_owner));
        }
        // Techno701757..70178E: Added_To_Game on the new house (not in limbo).
        if on_map {
            self.house_added_to_game(stable_id);
        }
        if build_const_eligible
            && let Some(house) = self.houses.get_mut(&new_owner)
            && !house.build_const_order.contains(&stable_id)
        {
            house.build_const_order.push(stable_id);
        }
        if category == EntityCategory::Structure {
            self.join_house_base_lists(stable_id, old_owner, new_owner, rules);
        }
        // `TechnoClass::ChangeOwner` closes with the mission half (the
        // `+0x484` call at 0x00701849 reads the NEW owner's
        // IsControlledByHuman). A rules-less transfer (tests only) cannot
        // read the type and skips it.
        if let Some(rules) = rules {
            self.change_owner_mission_half(stable_id, rules, effects);
            if category == EntityCategory::Structure {
                crate::sim::production::initialize_factory_primary(self, stable_id, rules);
                //448CEF: Grand_Opening(1) also runs on an already placed
                //building; its6E4 gate prevents repeating first-opening slots.
                self.grand_opening(stable_id, true, false, rules, None, effects);
            }
        }
        // `FootClass::ChangeOwner @ 0x004DBF13..0x004DBF32`: a Foot given to
        // a human house leaves its team.
        if category != EntityCategory::Structure
            && self
                .houses
                .get(&new_owner)
                .is_some_and(|house| house.is_human)
        {
            self.leave_team(stable_id, false, rules, effects);
        }
        self.foot_neighbors_after_owner_change(stable_id, rules);
        self.refresh_waypoint_edge_from_committed_structure(stable_id);
        self.reveal_building_sight_after_owner_change(stable_id, rules);
        if let Some(rules) = rules {
            self.reapply_building_gap_after_owner_change(stable_id, rules);
        }
        if let Some(rules) = rules {
            self.relink_docked_after_owner_change(stable_id, &taken_along, rules, effects);
        }
        // Building449267/449284: old House then new House, after lists,
        // primary and opening moved. Literal BuildCat0 selects the yard's
        // Building slot; Defense has no extra capture callback.
        if category == EntityCategory::Structure
            && let Some(rules) = rules
            && let Some(factory_category) = self
                .substrate
                .entities
                .get(stable_id)
                .and_then(|entity| self.object_type(entity.type_ref(), rules))
                .and_then(crate::sim::production::factory_category_for_building)
        {
            production::update_factory_queue(self, rules, old_owner, factory_category, effects);
            production::update_factory_queue(self, rules, new_owner, factory_category, effects);
        }
        // RESIDUAL: the computer's sale of a captured building
        // (`0x004492D7..0x0044936E`) is unported, since VERA keeps no
        // InitialOwner. Outside campaign (GameMode != 0), a new owner no human
        // controls sells a building whose InitialOwner is another house that
        // is not `MultiplayPassive`: each upgrade is refunded in full to it
        // (`GetRefund`, type vt+0xB8) and removed (`RemoveLastUpgrade`), then
        // `Sell_Back(1)` (vt+0x1A0). Trigger: an AI engineer captures another
        // side's building in skirmish. Effect: the AI keeps the building.
        // Frequency: each such capture. Risk: the AI gains its factory, power
        // or prerequisites.
        //44936E/449379: both houses' retained1FC asks their next House tail.
        if category == EntityCategory::Structure {
            self.request_house_tech_tree_recheck(old_owner);
            self.request_house_tech_tree_recheck(new_owner);
        }
    }

    /// `BuildingClass::ChangeOwner @ 0x00448260`'s docked objects
    /// (`0x00448559..0x004486C8`), before the building's own owner swap. Each
    /// radio slot is read afresh, since the transmits on the way change them.
    ///
    /// A Foot (`AbstractFlags & 4`, `0x004485A2`) that answers NEED_TO_MOVE
    /// (`0x13`) with ROGER changes owner first (vt+0x3D4, `0x00448663`) when
    /// the building is a `WeaponsFactory=` or the Foot is at its dock
    /// ([`Self::docked_at_dock_coord`]). Its tether (`+0x418`) is kept for
    /// [`Self::relink_docked_after_owner_change`].
    ///
    /// Any other contact gets RUN_AWAY (`0x17`), then OVER_OUT (`3`). A
    /// contact that is not a Foot is sent both as a null target
    /// (`0x004485A8`), which the transmit resolves to the building's first
    /// contact on each send.
    ///
    /// A refinery's dock coordinate is the cell beside its pad
    /// ([`crate::sim::movement::navcom::building_dock_cell`]), so a miner
    /// unloading on the pad is sent off.
    fn take_docked_along_on_owner_change(
        &mut self,
        building: u64,
        new_owner: InternedId,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Vec<(u64, bool)> {
        use crate::sim::radio::{RadioMessage, RadioPayload, RadioResponse, transmit};
        let mut taken = Vec::new();
        let Some((capacity, weapons_factory)) =
            self.substrate.entities.get(building).map(|entity| {
                (
                    entity.radio_contacts.capacity(),
                    self.object_type(entity.type_ref(), rules)
                        .is_some_and(|object| object.weapons_factory),
                )
            })
        else {
            return taken;
        };
        for slot in 0..capacity {
            let Some(contact) = self
                .substrate
                .entities
                .get(building)
                .and_then(|entity| entity.radio_contacts.slot(slot))
            else {
                continue;
            };
            let Some(foot) = self
                .substrate
                .entities
                .get(contact)
                .map(|entity| entity.category() != EntityCategory::Structure)
            else {
                continue;
            };
            if !foot {
                for message in [RadioMessage::RunAway, RadioMessage::Break] {
                    crate::sim::radio::transmit_to_contact(
                        self,
                        building,
                        message,
                        Some(rules),
                        effects,
                    );
                }
                continue;
            }
            let roger = transmit(
                self,
                building,
                contact,
                RadioMessage::NeedToMove,
                RadioPayload::default(),
                Some(rules),
                effects,
            ) == RadioResponse::Roger;
            if roger && (weapons_factory || self.docked_at_dock_coord(building, contact, rules)) {
                let tethered = self
                    .substrate
                    .entities
                    .get(contact)
                    .is_some_and(|entity| entity.tether.partner().is_some());
                self.change_owner_impl(contact, new_owner, Some(rules), registry, effects);
                taken.push((contact, tethered));
            } else {
                for message in [RadioMessage::RunAway, RadioMessage::Break] {
                    transmit(
                        self,
                        building,
                        contact,
                        message,
                        RadioPayload::default(),
                        Some(rules),
                        effects,
                    );
                }
            }
        }
        taken
    }

    /// Whether `contact`'s coordinate (vt+0x48, `0x00448606`) lies within
    /// `0x3F` leptons (Distance3D, `0x00448644`) of `building`'s dock
    /// coordinate for it (GetDockCoord vt+0xA8, `0x004485F5`).
    fn docked_at_dock_coord(&self, building: u64, contact: u64, rules: &RuleSet) -> bool {
        let terrain = self.resolved_terrain.as_ref();
        let Some(dock) = crate::sim::movement::navcom::building_dock_coordinate(
            &self.substrate.entities,
            building,
            Some(contact),
            terrain,
            rules,
            &self.interner,
        ) else {
            return false;
        };
        self.substrate.entities.get(contact).is_some_and(|entity| {
            let at = crate::sim::movement::ground_pose::object_get_coords(entity, terrain);
            crate::util::native_x87::distance_3d_leptons(
                [at.x, at.y, at.z],
                [dock.x, dock.y, dock.z],
            ) <= 0x3F
        })
    }

    /// `BuildingClass::ChangeOwner`'s re-link (`0x00449289..0x004492D7`),
    /// after the owner swap: HELLO (`2`) to each object taken along, then,
    /// when that object was tethered, the tether set on the building
    /// (`0x004492B7`) and on the object (`0x004492C3`).
    fn relink_docked_after_owner_change(
        &mut self,
        building: u64,
        taken: &[(u64, bool)],
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        for &(contact, tethered) in taken {
            crate::sim::radio::transmit(
                self,
                building,
                contact,
                crate::sim::radio::RadioMessage::Hello,
                crate::sim::radio::RadioPayload::default(),
                Some(rules),
                effects,
            );
            if tethered {
                if let Some(entity) = self.substrate.entities.get_mut(building) {
                    entity.tether.attach(contact);
                }
                if let Some(entity) = self.substrate.entities.get_mut(contact) {
                    entity.tether.attach(building);
                }
            }
        }
    }

    /// The mission half of `TechnoClass::ChangeOwner @ 0x007014A0`, every
    /// class. The early setters/Guard queue are inline before the house swap
    /// in `change_owner_impl`; this consumer handles only the new-house tail:
    /// - `0x007014D5..0x0070151A`: `Assign_Target(0)` (`+0x3C8`),
    ///   `Assign_Destination(0, 1)` (`+0x480` at `0x007014E9`; a Drive/Ship
    ///   Unit's is the Unit setter, [`Self::assign_null_destination`]), and
    ///   the ArchiveTarget (`+0x218`) cleared unless a Unit is deploying
    ///   (`0x00746DB0`, the `+0x6E1`/`+0x6E2` latches);
    /// - `0x00701524..0x0070156E`: `Queue_Mission(Guard, commence_now = 1)`
    ///   (`+0x1E8` = `MissionClass::Queue_Mission @ 0x005B35E0`) unless current
    ///   is Selling (0x13), or a Unit whose type is `IsSimpleDeployer`
    ///   (`UnitType+0xE13`) is in Unload; a ready object commits Guard on the
    ///   spot;
    /// - `0x00701793..0x007017BA`, after the `+0x21C` house swap: a Rescue
    ///   (0x15) current or queued mission becomes `Assign_Mission(Guard)`;
    /// - `0x007017C0..0x00701849`, unless in limbo (`+0x81`), a
    ///   `WeaponsFactory=` building unloading, or in radio contact with a
    ///   `WeaponsFactory=` building (`BuildingType+0x16BD`, the war-factory
    ///   exit link): `Assign_Destination(0, 1)` (`0x0070182F`),
    ///   `Assign_Target(0)` and `Enter_Idle_Mode(0, 1)` (`+0x484`), which
    ///   reads the NEW owner ([`enter_idle_mode`]):
    ///   - a war or chrono miner takes the Unit leaf's harvester arm
    ///     (`Simulation::unit_enter_idle_mode`): Harvest for the new owner,
    ///     Guard when that owner is human and the miner stands off ore, and
    ///     nothing while in radio contact (a miner docked at its refinery) or
    ///     while the Guard above is still only queued (a miner caught
    ///     mid-track);
    ///   - any other Unit or Infantry takes its class leaf, Unit
    ///     `0x00738970` or Infantry `0x0051CBA0`;
    ///   - a Building (`0x0044D6A0` with `initial = 0`) calls `0x00447780(1)`
    ///     and `Queue_Mission(Guard, 0)`.
    ///
    /// Nothing here clears an attack-move's MegaMission (`Foot+0x5C4`). The
    /// Enter_Idle_Mode above asks RefreshMegaMission
    /// ([`Simulation::foot_refresh_mega_mission`]), which resumes the order
    /// when the object had found a target on the way or Guard was committed
    /// on the spot; otherwise the order ends there. A released object goes
    /// on to its MegaDestination for its old owner; CaptureUnit resets a
    /// captive's orders to Guard afterwards (`0x00471EA5`), which drops the
    /// resumed NavCom but keeps the MegaMission.
    ///
    /// Callers: engineer capture and garrison transfer, and mind control's
    /// CaptureUnit and FreeUnit. No stock miner is ever mind-controlled: HARV,
    /// CMIN and SMIN are `ImmuneToPsionics=yes`.
    ///
    /// RESIDUAL: no stock aircraft can be captured, so the Aircraft
    /// Enter_Idle_Mode (`0x004176F0`) here is dormant; the trailing
    /// `+0x423`-gated `vt+0x498`/`vt+0x494` and `vt+0x488(0, 0, 0, 0, 0)`
    /// calls are unidentified.
    ///
    /// `+0x484` census (`search_instructions CALL [+0x484]`), the sites a
    /// miner can reach besides Move arrival, this owner change and the depot
    /// exit, with their gates:
    /// - `DriveLocomotionClass::Stop_And_Scatter @ 0x004B48BE` → `+0x484(0,1)`
    ///   after `Stop_Moving` when the NavQueue is non-empty. Not run by VERA:
    ///   VERA scatter is UNCHECKED; natively a human-parked Guard miner
    ///   scattered on ore would re-queue Harvest through this call.
    /// - `FootClass::Queue_Navigation_List @ 0x004DA0E0` → `+0x484(0,1)` at
    ///   `0x004DA1A6` when NavCom == 0, Get_Mission == Guard and contact 0 is
    ///   not a WeaponsFactory building ([`Simulation::queue_navigation_list`]).
    ///   Its callers are ExitObject's Infantry post (`0x00444DAC`) and the
    ///   Unit setter's WeaponsFactory arm (`0x00742E28`,
    ///   `Simulation::unit_factory_exit_arm`).
    /// - `FootClass::Check_Destination_Is_UnitRepair_Dock @ 0x004DBA15` →
    ///   `+0x484(0, !depot-contact)` when NavCom == 0 && TarCom == 0
    ///   (virtual slot; miner reachability unestablished; VERA's
    ///   `track_navigation_gate`).
    /// - `FootClass::ReceiveDamage @ 0x004D74C7`, gated on
    ///   `MissionControl[current].NoThreat && !Zombie` (retail `[Harmless]`)
    ///   for a teamless Foot hit by a source; the damage receiver
    ///   (`combat::world_receiver`) calls it.
    fn change_owner_mission_half(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        use crate::sim::mission::{MissionId, MissionType};
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        let category = entity.category();
        let current = entity.mission.current().known();
        let object = self.object_type(entity.type_ref(), rules);
        let factory_unloading = category == EntityCategory::Structure
            && current == Some(MissionType::Unload)
            && object.is_some_and(|object| object.weapons_factory);
        let in_limbo = entity.lifecycle.in_limbo();
        let now = self.session.binary_frame;
        let rescue = self.substrate.entities.get(stable_id).is_some_and(|e| {
            e.mission.current().known() == Some(MissionType::Rescue)
                || e.mission.queued().known() == Some(MissionType::Rescue)
        });
        if rescue {
            let _ = self.mission_assign_exact(
                stable_id,
                MissionId::from_known(MissionType::Guard),
                now,
            );
        }
        if in_limbo || factory_unloading {
            return;
        }
        let in_factory_contact = self.substrate.entities.get(stable_id).is_some_and(|e| {
            e.radio_contacts.iter_live().any(|other| {
                self.substrate
                    .entities
                    .get(other)
                    .and_then(|b| self.object_type(b.type_ref(), rules))
                    .is_some_and(|obj| obj.weapons_factory)
            })
        });
        if in_factory_contact {
            return;
        }
        self.assign_null_destination(stable_id, Some(rules), None, effects);
        let _ = self.assign_target_represented(stable_id, None, Some(rules), None, effects);
        enter_idle_mode(self, stable_id, rules, None, effects);
    }

    /// Legacy non-lifecycle contact scrub retained only for separately classified
    /// failure paths. Reveal/Conceal/UnInit must use synchronous radio authority.
    pub(crate) fn clear_radio_contacts_for(&mut self, stable_id: u64) {
        self.substrate.entities.clear_radio_contacts_for(stable_id);
    }

    /// Number of houses that can actually contend for the match outcome.
    ///
    /// MultiplayPassive houses (stock Civilian/JP) are roster filler: they are
    /// never defeated and never counted alive, so they must not make a
    /// solo developer board look contested. Automatic victory creation uses
    /// `> 1`; accepted explicit outcomes never depend on this count.
    ///
    /// VERA-internal: the gamemd equivalent is UNCHECKED. Neither the native
    /// defeat block nor its all-allied scan has an "is there a real opponent"
    /// precondition. Native campaign instead skips automatic empty-house defeat
    /// at 0x004F8E86..0x004F8F87 when SessionType is zero. This developer Battle
    /// sandbox policy is not that campaign lifecycle. The passive filter is
    /// gamemd-derived; the count includes defeated opponents so an ordinary
    /// last-player victory remains valid.
    pub fn contending_house_count(&self) -> usize {
        self.houses
            .values()
            .filter(|house| !house.multiplay_passive)
            .count()
    }

    /// Restore externally-derived cache fields after validated snapshot
    /// identity fixup and substrate re-registration.
    ///
    /// The caller must provide the same map/rules data that was used to initialize
    /// the original simulation. Cache fields were `#[serde(skip)]`'d and are at
    /// their Default values after deserialization. The caller must first run
    /// `restore_after_snapshot_load`, which resolves stable-ID references and
    /// rebuilds registry, LogicVector-membership, and CellClass-list caches.
    ///
    /// Overlay, bridge, and navigation authority are restored separately by
    /// `restore_map_authority_after_snapshot_load` once rules and the overlay
    /// registry are bound. Rule animation lists are re-interned from names:
    /// an outgoing simulation's numeric IDs do not belong to this saved world.
    pub fn rebuild_caches_after_load(
        &mut self,
        mut resolved_terrain: ResolvedTerrainGrid,
        terrain_speed_config: terrain_speed::TerrainSpeedConfig,
        rules: &RuleSet,
    ) {
        resolved_terrain.bind_shared_cell_dummy(self.shared_cell_dummy.clone());
        // Restore externally-derived data only. Substrate caches are rebuilt
        // transactionally by `restore_after_snapshot_load` before this call.
        // The supplied map grid predates runtime Terrain lifecycle changes, so
        // serialized Terrain objects must overwrite that derived projection.
        // Clear every known source first so Limbo/Destroyed objects stay absent,
        // then replay only live objects through the shared occupation writer.
        let terrain_objects: Vec<_> = self.production.terrain_objects.values().cloned().collect();
        for terrain in &terrain_objects {
            crate::sim::terrain_object::unmark_terrain_occupation(
                terrain,
                Some(&mut resolved_terrain),
            );
        }
        for terrain in &terrain_objects {
            if terrain.is_live() {
                crate::sim::terrain_object::mark_terrain_occupation(
                    terrain,
                    Some(&mut resolved_terrain),
                );
            }
        }
        self.resolved_terrain = Some(resolved_terrain);
        self.terrain_speed_config = terrain_speed_config;
        self.resolve_rule_animation_lists(rules);
    }

    /// Rebuild LogicClass membership flags from the restored active order.
    ///
    /// `+0x98` is not serialized (native does not round-trip it); vector presence
    /// is authoritative: RegisterObject sets it after its insert (`0x0055BAC6`)
    /// and UnregisterObject clears it after its removal (`0x0055BB27`), so the
    /// saved order determines it. Idempotent — safe to call after any load.
    /// Standalone (no heavy load-arg dependency) so save/load membership is
    /// unit-testable.
    pub fn rebuild_logic_membership(&mut self) {
        for entity in self.substrate.entities.values_mut() {
            entity.in_logic_vector.mark(false);
        }
        for anim in self.substrate.anims.values_mut() {
            anim.in_logic_vector = false;
        }
        for debris in self.substrate.voxel_anims.values_mut() {
            debris.in_logic_vector = false;
        }
        for (_, system) in self.substrate.particle_systems.iter_mut() {
            system.in_logic_vector = false;
        }
        for terrain in self.production.terrain_objects.values_mut() {
            terrain.in_logic_vector = false;
        }
        for (_, projectile) in self.projectiles.iter_mut() {
            projectile.in_logic_vector = false;
        }
        for (_, wave) in self.waves.iter_mut() {
            wave.in_logic_vector = false;
        }
        for &id in &self.substrate.logic.snapshot() {
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                entity.in_logic_vector.mark(true);
            } else if let Some(anim) = self.substrate.anims.get_mut(id) {
                anim.in_logic_vector = true;
            } else if let Some(debris) = self.substrate.voxel_anims.get_mut(id) {
                debris.in_logic_vector = true;
            } else if let Some(system) = self.substrate.particle_systems.get_mut(id) {
                system.in_logic_vector = true;
            } else if let Some(terrain) = self.production.terrain_objects.get_mut(&id) {
                terrain.in_logic_vector = true;
            } else if let Some(projectile) = self.projectiles.get_mut(id) {
                projectile.in_logic_vector = true;
            } else if let Some(wave) = self.waves.get_mut(id) {
                wave.in_logic_vector = true;
            }
        }
        // Alive, limbo, cell Mark, and death-sequence state are independent
        // serialized facts. Load repair must never derive them from this vector.
    }

    /// Borrow the current canonical dynamic navigation projection.
    pub fn path_grid(&self) -> Option<&PathGrid> {
        self.path_grid.as_deref()
    }

    /// Clone the current projection's `Arc` only to split borrows at a point
    /// of use (the grid read alongside a `&mut self` call). Take it where the
    /// grid is read, never to carry a view across a republish.
    pub fn path_grid_snapshot(&self) -> Option<Arc<PathGrid>> {
        self.path_grid.clone()
    }

    /// Rebuild dynamic structure blockers, zones, and the canonical PathGrid
    /// as one simulation-owned projection.
    pub fn rebuild_dynamic_navigation(&mut self, rules: &RuleSet) -> bool {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let result = navigation::NavigationCaches {
            zones: &mut self.zone_grid,
            path: &mut self.path_grid,
            playfield_bounds: self.playfield_bounds,
        }
        .rebuild_dynamic(
            terrain,
            self.bridge_state.as_ref(),
            &self.substrate.entities,
            &self.interner,
            rules,
        );
        match result {
            Ok(cells) => self.structure_navigation_cells = cells,
            Err(error) => {
                log::error!("Navigation restoration refused: {error}");
                return false;
            }
        }
        true
    }

    /// Publish marked-structure footprint changes into the path view only.
    /// Native cell occupation marks are read live by the pathfinder, and the
    /// zone map is built from terrain movement classes, which structures do
    /// not change; zone IDs therefore stay as they are.
    fn publish_structure_navigation(&mut self, rules: &RuleSet) {
        let current =
            navigation::structure_movement_cells(&self.substrate.entities, &self.interner, rules);
        let candidates: Vec<_> = current
            .union(&self.structure_navigation_cells)
            .copied()
            .collect();
        self.publish_recalculated_cells_with_presence(rules, &candidates, &current);
    }

    /// Finalize mutable overlay identity, passability, and canonical navigation
    /// before the frame hash is latched.
    ///
    /// `dirty_cells` carries mutations that also re-derive CellClass attributes,
    /// mirroring `OverlayClass::Mark`'s `RecalcAttributes` tail; only its
    /// occupied cells produce an upsert. Identity erasures that native performs
    /// *without* a recalc arrive separately through `take_removed_render_cells`
    /// and cross the boundary as removals, since an upsert list cannot express
    /// one.
    fn finalize_frame_overlays_and_navigation(
        &mut self,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        navigation_rebuild_requested: bool,
        structures_changed: bool,
    ) -> Vec<OverlayEntry> {
        let mut overlay_updates = Vec::new();
        let mut navigation_cells = Vec::new();
        let overlay_ready =
            rules.is_some() && self.resolved_terrain.is_some() && overlay_registry.is_some();
        if overlay_ready && let Some(grid) = self.overlay_grid.as_mut() {
            self.frame_overlay_removals = grid.take_removed_render_cells();
            let dirty_cells = grid.take_dirty_cells();
            navigation_cells = grid.take_synchronous_navigation_cells();

            let terrain = self
                .resolved_terrain
                .as_mut()
                .expect("overlay-ready terrain");
            let registry = overlay_registry.expect("overlay-ready registry");
            for &(rx, ry) in &dirty_cells {
                if recalc_overlay_passability(grid, terrain, registry, rx, ry)
                    && !navigation_cells.contains(&(rx, ry))
                {
                    navigation_cells.push((rx, ry));
                }
            }
            for (rx, ry) in dirty_cells {
                let cell = grid.cell(rx, ry);
                if let Some(overlay_id) = cell.overlay_id {
                    overlay_updates.push(OverlayEntry {
                        rx,
                        ry,
                        overlay_id,
                        frame: cell.overlay_data,
                    });
                }
            }
        }

        if let Some(rules) = rules {
            if navigation_rebuild_requested {
                let _ = self.rebuild_dynamic_navigation(rules);
            } else {
                if structures_changed {
                    self.publish_structure_navigation(rules);
                }
                self.publish_recalculated_cells(rules, &navigation_cells);
            }
        }
        overlay_updates
    }

    /// Publish completed structure/overlay/terrain Recalcs (`CellClass::RecalcAttributes`
    /// @ `0x0047D2B0`) through the one-cell navigation owner. Zone IDs are not
    /// touched: gamemd's non-wall Mark (`0x005FC570`) and ReduceTiberium
    /// (`0x00480A80`) run no zone helper, and the wall, sale and terrain owners
    /// run theirs themselves.
    fn publish_recalculated_cells(&mut self, rules: &RuleSet, cells: &[(u16, u16)]) {
        // During initial map Reveal no navigation views are installed yet;
        // their first full rebuild reads every admitted Mark. Avoid scanning
        // all structures once per placement before that rebuild.
        if cells.is_empty() || (self.path_grid.is_none() && self.zone_grid.is_none()) {
            return;
        }
        let blocked = navigation::structure_blocked_among(
            &self.substrate.entities,
            &self.interner,
            rules,
            cells,
        );
        self.publish_recalculated_cells_with_presence(rules, cells, &blocked);
    }

    /// [`Self::publish_recalculated_cells`] with structure presence already
    /// scanned. A cache that cannot take one cell (mis-sized during a load
    /// transition) is rebuilt whole rather than left stale.
    fn publish_recalculated_cells_with_presence(
        &mut self,
        rules: &RuleSet,
        cells: &[(u16, u16)],
        blocked: &BTreeSet<(u16, u16)>,
    ) {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        let mut caches = navigation::NavigationCaches {
            zones: &mut self.zone_grid,
            path: &mut self.path_grid,
            playfield_bounds: self.playfield_bounds,
        };
        let published = cells.iter().try_for_each(|&coord| {
            caches.publish_recalculated_cell_with_presence(terrain, coord, blocked.contains(&coord))
        });
        if let Err(error) = published {
            log::warn!("Recalc navigation publication fell back to a rebuild: {error}");
            let _ = self.rebuild_dynamic_navigation(rules);
            return;
        }
        for &coord in cells {
            if blocked.contains(&coord) {
                self.structure_navigation_cells.insert(coord);
            } else {
                self.structure_navigation_cells.remove(&coord);
            }
        }
    }

    /// Rebuild the zone connectivity map from the current PathGrid. Call after
    /// the PathGrid has been rebuilt so that zones reflect the latest
    /// walkability state. Unchanged cells and classes keep the current zones.
    #[cfg(any(test, feature = "test-support"))]
    pub fn rebuild_zone_grid(&mut self, path_grid: &PathGrid) {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        navigation::NavigationCaches {
            zones: &mut self.zone_grid,
            path: &mut self.path_grid,
            playfield_bounds: self.playfield_bounds,
        }
        .rebuild_zones(path_grid, terrain, self.bridge_state.as_ref());
    }

    /// Rebuild without `rebuild_zones`' reuse of unchanged inputs. Reduced zone
    /// type can change while boolean walkability stays identical (notably a
    /// live OccupationBits=0 terrain object changing Building to Ground).
    fn rebuild_zone_grid_full(&mut self, path_grid: &PathGrid) {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        navigation::NavigationCaches {
            zones: &mut self.zone_grid,
            path: &mut self.path_grid,
            playfield_bounds: self.playfield_bounds,
        }
        .rebuild_zones_full(path_grid, terrain, self.bridge_state.as_ref());
    }

    /// Consume terrain/overlay receipts before the next path reader. Overlay
    /// receipts publish their Recalc only. A destroyed terrain object follows
    /// `TerrainClass::Limbo` @ `0x0071C930`: Recalc (`0x0071CA2A`), then
    /// AssignOrphanedCellZone (`0x0071CA42`) and the local graph update
    /// (`0x0071CA51`) while `g_MapEditorMode` is zero. Deaths are published
    /// after the receiver batch, not inside each Limbo.
    fn finish_terrain_navigation_changes(
        &mut self,
        rules: &RuleSet,
        terrain_changed_cells: &[(u16, u16)],
    ) {
        let overlay_cells = self
            .overlay_grid
            .as_mut()
            .map(|grid| grid.take_synchronous_navigation_cells())
            .unwrap_or_default();
        let all_cells: Vec<_> = overlay_cells
            .iter()
            .chain(terrain_changed_cells)
            .copied()
            .collect();
        let blocked = navigation::structure_blocked_among(
            &self.substrate.entities,
            &self.interner,
            rules,
            &all_cells,
        );
        self.publish_recalculated_cells_with_presence(rules, &overlay_cells, &blocked);
        // Each Limbo's AssignOrphaned sees later deaths' cells unpublished.
        for &cell in terrain_changed_cells {
            self.publish_cell_and_assign_orphaned_zone(rules, cell, &blocked);
        }
    }

    /// One Recalc'd cell's navigation publication, then
    /// AssignOrphanedCellZone (`0x0056D460`) and the local graph update
    /// (`0x00584550`): the per-cell tail `TerrainClass::Limbo` and
    /// `BuildingClass::AfterDestruction` share. `blocked` is the structure
    /// presence already scanned for the caller's cells.
    fn publish_cell_and_assign_orphaned_zone(
        &mut self,
        rules: &RuleSet,
        cell: (u16, u16),
        blocked: &BTreeSet<(u16, u16)>,
    ) {
        self.publish_recalculated_cells_with_presence(rules, &[cell], blocked);
        if let (Some(terrain), Some(path)) =
            (self.resolved_terrain.as_ref(), self.path_grid.as_deref())
        {
            repair_zone_after_recalc(
                &mut self.zone_grid,
                path,
                terrain,
                self.bridge_state.as_ref(),
                self.playfield_bounds,
                cell,
                ZoneRepairKind::AssignOrphaned,
            );
        }
    }

    /// Direct Cell480CB0 caller, retaining its receiver across synchronous
    /// callbacks. Unit73B056 uses the same transaction host as AreaDamage;
    /// the latter's additional outer detach belongs to its own caller.
    pub(crate) fn apply_wall_damage(
        &mut self,
        target: crate::sim::overlay_grid::WallPointerTarget,
        damage: i32,
        overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    ) {
        let Some(grid) = self.overlay_grid.as_mut() else {
            return;
        };
        #[cfg(any(test, feature = "test-support"))]
        let mut cell_target_detaches = Vec::new();
        let mut host = SimulationWallRuntimeHost {
            entities: &mut self.substrate.entities,
            target_terrain_objects: &self.production.terrain_objects,
            #[cfg(any(test, feature = "test-support"))]
            detach_trace: &mut cell_target_detaches,
            radar_dirty_cells: &mut self.radar_terrain_dirty_cells,
            radar_dirty_generation: &mut self.radar_terrain_dirty_generation,
            tactical_dirty_cells: &mut self.tactical_dirty_cells,
            zone_grid: &mut self.zone_grid,
            path_grid: &mut self.path_grid,
            bridge_state: self.bridge_state.as_ref(),
            playfield_bounds: self.playfield_bounds,
        };
        let _ = damage_wall_overlay_with_runtime_host(
            grid,
            overlay_registry,
            self.resolved_terrain.as_mut(),
            target,
            damage,
            &mut self.scenario_rng,
            Some(&mut host),
        );
    }

    pub(crate) fn default_vision_range_for_category(category: EntityCategory) -> u16 {
        match category {
            EntityCategory::Infantry => 5,
            EntityCategory::Unit => 6,
            EntityCategory::Aircraft => 8,
            EntityCategory::Structure => 7,
        }
    }

    /// Find houses with at least one native-eligible SpySat provider. This is
    /// the House-rung edge detector; consumers read the persisted house latch.
    /// `HouseClass::Update_SpySat @ 0x00508F60` walks the House's building
    /// list (House+0x68, items `+0x6C`, count `+0x78`) in its order.
    fn collect_spy_sat_candidate_owners(&self, rules: &RuleSet) -> BTreeSet<InternedId> {
        let selling =
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Selling);
        let mut active = BTreeSet::new();
        let mut scanned_houses = BTreeSet::new();
        for &owner in self.session.house_order.iter().chain(self.houses.keys()) {
            let Some(house) = self.houses.get(&owner) else {
                continue;
            };
            if !scanned_houses.insert(owner) {
                continue;
            }
            for &stable_id in house.base_projection.buildings() {
                let Some(entity) = self.substrate.entities.get(stable_id) else {
                    continue;
                };
                let coarse_candidate = !entity.lifecycle.in_limbo()
                    && entity.lifecycle.cell_marked()
                    && entity.mission.current() != selling
                    && entity.mission.queued() != selling
                    && self
                        .object_type(entity.type_ref(), rules)
                        .is_some_and(|object| object.spy_sat);
                if !coarse_candidate {
                    continue;
                }
                // The first coarse candidate decides the house. Warp-out is a
                // blocking result, not a reason to continue to a later uplink.
                if !entity.is_warped_out() {
                    active.insert(owner);
                }
                break;
            }
        }
        active
    }

    /// Read retained Building deposits; power classification belongs only to
    /// the Building43FB20 operational edge, never a House/view refresh.
    fn collect_active_vision_structures(&self, _rules: &RuleSet) -> ActiveVisionStructures {
        let mut effects = ActiveVisionStructures::default();
        for entity in self.substrate.entities.values() {
            for (&viewer, deposit) in entity.gap_generator.viewers() {
                if !deposit.active() {
                    continue;
                }
                effects.gap_generators.entry(viewer).or_default().push(
                    vision::GapGeneratorSource {
                        stable_id: entity.stable_id(),
                        owner: entity.owner(),
                        rx: entity.position.rx(),
                        ry: entity.position.ry(),
                        radius: deposit.radius(),
                    },
                );
            }
        }
        effects
    }

    fn apply_active_vision_structures(&mut self, effects: &ActiveVisionStructures) {
        vision::materialize_gap_generator_sources(
            &mut self.fog,
            &self.house_alliances,
            &effects.gap_generators,
            &self.interner,
        );
    }

    /// Commit aggregate SpySat transitions in the existing pre-House batch,
    /// then materialize SpySat -> Gap. EventClass commands execute later and
    /// become visible next frame. Native4F84F1 gates508F60 on the dirty suffix;
    /// ordinary Unlimbo discovery6F49DE, GrandOpening446A84 and Selling44AB1B
    /// arm it. Wider dirty/cross-House callback cadence remains a residual;
    /// this cue chain preserves the existing map/latch scheduling.
    fn reconcile_active_vision_structures(&mut self, rules: &RuleSet) {
        let active_owners = self.collect_spy_sat_candidate_owners(rules);
        let transitions: Vec<_> = self
            .houses
            .iter()
            .filter_map(|(&owner, house)| {
                let active = active_owners.contains(&owner);
                (house.spy_sat_active != active).then_some((owner, active))
            })
            .collect();
        for (owner, active) in transitions {
            //509064/509012 call Map while577A still has its OLD value.
            if active {
                self.reveal_map_for_owner(owner, rules);
            } else {
                self.reset_shroud_for_owner(owner, rules);
            }
            if let Some(house) = self.houses.get_mut(&owner) {
                house.spy_sat_active = active;
                // House509069/509017 stores577A after the Map callback;
                //509090/50903F then requests the current PlayerPtr's cue.
                // Native750920 receives center8192, volume1f and NULL handle.
                // Other map callers (crates/infiltration) do not reach this
                // aggregate edge. Source: tools/input_oracle/spy_sat_audio.md.
                if self.session.current_house == Some(owner) {
                    let sound = if active {
                        &rules.general.spy_sat_activation_sound
                    } else {
                        &rules.general.spy_sat_deactivation_sound
                    };
                    if let Some(sound_id) = sound {
                        self.sound_events.push(SimSoundEvent::VocCentered {
                            sound_id: sound_id.clone(),
                        });
                    }
                }
            }
        }
        let effects = self.collect_active_vision_structures(rules);
        self.apply_active_vision_structures(&effects);
    }

    /// Early mechanisms inside one reached House update.
    ///
    /// gamemd-derived: `LogicClass__PerTickUpdate @ 0x0055AFB0`, House loop
    /// `0x0055B68D..0x0055B6B1`, walks the House array forward, skips nulls,
    /// and reloads the live count after every `HouseClass__Update @ 0x004F8440`
    /// call. The bounded Rust mechanisms are anger decay before the verified
    /// AI-activation block `0x004F8564..0x004F85B7`.
    fn update_house_anger_and_activation(&mut self, owner: InternedId, rules: Option<&RuleSet>) {
        let current_frame = self.session.binary_frame as i32;
        let game_mode_nonzero = self.session.game_mode_nonzero;
        let iq_production = rules.map(|rules| rules.general.iq_production);
        let represented = self.houses.contains_key(&owner);
        if let Some(house) = self.houses.get_mut(&owner) {
            house_strategy::decay_anger_scores(house, &self.session.house_order, current_frame);
        }
        if represented {
            #[cfg(any(test, feature = "test-support"))]
            self.trace_house_ai_activation_order(HouseAiActivationOrderTestEvent::HouseAngerDecay(
                owner,
            ));
            if let Some(iq_production) = iq_production {
                self.houses
                    .get_mut(&owner)
                    .expect("represented House remains registered during its update")
                    .update_ai_activation(game_mode_nonzero, iq_production);
                #[cfg(any(test, feature = "test-support"))]
                self.trace_house_ai_activation_order(
                    HouseAiActivationOrderTestEvent::HouseActivation(owner),
                );
            }
        }
    }

    /// The per-frame fog pass: ends last frame's sight, releases the sight of
    /// objects that no longer hold it and admits the sight of objects without
    /// one (the remaining House admission reconciliation), then applies SpySat
    /// and Gap Generator effects. Footprints move only at reveal events
    /// ([`Self::refresh_object_sight`]).
    fn refresh_fog(&mut self, config: &vision::VisionConfig, rules: Option<&RuleSet>) {
        self.refresh_sight_sources(config, rules);

        // Apply SpySat and Gap Generator effects if rules are available.
        if let Some(rules) = rules {
            let effects = self.collect_active_vision_structures(rules);
            self.apply_active_vision_structures(&effects);
        }

        // Diagnostic: log fog grid stats on first tick to debug coverage issues.
        if self.session.tick == 1 {
            log::info!(
                "Fog grid: {}x{}, {} owners",
                self.fog.width,
                self.fog.height,
                self.fog.by_owner.len()
            );
            for (owner, vis) in &self.fog.by_owner {
                let total = vis.width() as u32 * vis.height() as u32;
                let visible_count = vis.cells_raw().iter().filter(|c| **c & 0x02 != 0).count();
                let revealed_count = vis.cells_raw().iter().filter(|c| **c & 0x01 != 0).count();
                log::info!(
                    "  Owner '{}': {}/{} visible, {}/{} revealed",
                    owner,
                    visible_count,
                    total,
                    revealed_count,
                    total
                );
            }
            use std::collections::BTreeMap as DiagMap;
            let mut entity_stats: DiagMap<String, (u32, u16, u16, u16, u16)> = DiagMap::new();
            for entity in self.substrate.entities.values() {
                let entry = entity_stats
                    .entry(self.interner.resolve(entity.owner()).to_string())
                    .or_insert((0, u16::MAX, u16::MAX, 0, 0));
                entry.0 += 1;
                entry.1 = entry.1.min(entity.position.rx());
                entry.2 = entry.2.min(entity.position.ry());
                entry.3 = entry.3.max(entity.position.rx());
                entry.4 = entry.4.max(entity.position.ry());
            }
            for (owner, (count, min_rx, min_ry, max_rx, max_ry)) in &entity_stats {
                log::info!(
                    "  Entities '{}': {} units, rx={}..{}, ry={}..{}",
                    owner,
                    count,
                    min_rx,
                    max_rx,
                    min_ry,
                    max_ry
                );
            }
        }
    }

    /// `BuildingClass::OnConstructionComplete @ 0x00445F80`, the
    /// `SuperWeapon=` announce block (`0x004468AD..0x00446995`): a completed
    /// building whose type names a `[SuperWeaponTypes]` section
    /// (`Type+0x16F0 != -1`) reports itself. Only the owner-independent half
    /// lives here; the listener gates (local player, `IsAlliedWith(PlayerPtr)`,
    /// `[0xA8B538]`, `GameMode`, `AuxBuilding` ownership) are the app's.
    pub(crate) fn announce_super_weapon_building_complete(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
    ) {
        let Some((owner, type_ref)) = self
            .substrate
            .entities
            .get(stable_id)
            .filter(|e| e.category() == EntityCategory::Structure && !e.death.is_dying())
            .map(|e| (e.owner(), e.type_ref()))
        else {
            return;
        };
        let Some(section) = self
            .interner
            .object_type(type_ref, rules)
            .and_then(|obj| obj.super_weapon.clone())
        else {
            return;
        };
        let sw_type = self.interner.intern(&section);
        self.sound_events
            .push(SimSoundEvent::SuperWeaponDetected { owner, sw_type });
    }

    /// The Selling mission's visit in the building's own LogicVector slot
    /// (`BuildingClass::Update`: UpdateAnimation `0x0043FE22`, then Sell
    /// through the mission dispatch in `TechnoClass::Update` `0x0043FE56`), so
    /// stage 1's Scenario draws and the completion keep native order among the
    /// frame's objects. `sim::building_construction` steps the pack-up and
    /// `production::production_sell` runs Sell's stages. Every visit stops the
    /// repair (`ToggleRepair(0)`, `0x00449C41`) and re-arms the mission for
    /// the next frame (Sell returns 1). A building at Health 0 runs no mission
    /// handler (`MissionClass::AI 0x005B30A7`), a warped one no AI
    /// (`GameEntity::ai_frozen`). An object that enters the map (the crew, an
    /// undeploy's unit) marks the frame's spawn refresh; the object turn
    /// reports a building that left it.
    pub(crate) fn visit_building_down(
        &mut self,
        sid: u64,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        use crate::sim::building_construction::PackUpFrame;
        let now = self.session.binary_frame as i32;
        let visit = {
            let Some(entity) = self.substrate.entities.get(sid) else {
                return;
            };
            // The visit is the Selling mission's handler; Assign_Mission
            // (which Selling does not refuse) can end it early.
            if !entity.building_down()
                || entity.mission.current().known()
                    != Some(crate::sim::mission::MissionType::Selling)
                || entity.ai_frozen()
                || !entity.is_ai_alive()
                || entity.lifecycle.in_limbo()
            {
                return;
            }
            let Some(entity) = self.substrate.entities.get_mut(sid) else {
                return;
            };
            let mut status = entity.mission.handler_state();
            let visit = entity.advance_building_down(&mut status);
            entity.mission.set_handler_state(status);
            entity.mission.write_dispatch_epilogue(now, 1);
            visit
        };
        // Every visit stops a repair first (`ToggleRepair(0)`, `0x00449C41`).
        if let Some(rules) = rules {
            production::toggle_repair(self, rules, sid, production::RepairControl::Stop);
        }
        let spawned = match visit {
            PackUpFrame::StageZero => {
                production::sell_stage_zero(self, rules, sid, effects);
                false
            }
            PackUpFrame::StageOne => {
                production::sell_stage_one(self, rules, overlay_registry, sid, effects)
            }
            PackUpFrame::Complete => {
                production::sell_complete(self, rules, overlay_registry, sid, effects)
            }
            PackUpFrame::Waiting => false,
        };
        if spawned {
            self.mission_spawned_entities = true;
        }
    }

    /// Spine region (LATE): AI commands, defeat detection, building animations,
    /// radar aging, and the late frame/tick commit. Accumulates
    /// `spawned_entities` (command placements). Returns false when
    /// the terminating call skips frame commit and pending-delete processing.
    fn run_late_region(
        &mut self,
        commands: &mut Vec<CommandEnvelope>,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        tick_ms: u32,
        execute_tick: u64,
        executed_commands: &mut usize,
        spawned_entities: &mut bool,
        effects: FrameEffects<'_>,
    ) -> bool {
        #[cfg(any(test, feature = "test-support"))]
        self.trace_master_frame_rung(MasterFrameTestRung::Houses);
        if let Some(rules) = rules {
            self.reconcile_active_vision_structures(rules);
        }
        // Factory/production work has already completed in Phase 7. This early
        // House update follows optional vision reconciliation and precedes both
        // defeat processing and strategic AI command generation. Native anger
        // decay is unconditional; only the activation substep needs RuleSet.
        // --- Phase 8: Defeat detection, strategy and building choice ---
        // gamemd evaluates each house's defeat before its AI manage/produce
        // step. The gate reads the house's tracking counts (`house_defeat.rs`),
        // which construction and the frame-end pending-delete drain move: a
        // death reaches the gate on the next frame. Each house's strategy tick
        // (`house_strategy.rs`) and building choice follow its own gate.
        let generated = self.house_rung(rules, overlay_registry, self.session.tick > 0, effects);
        #[cfg(any(test, feature = "test-support"))]
        let commands_include_generated = self.replaying_recorded_commands;
        #[cfg(not(any(test, feature = "test-support")))]
        let commands_include_generated = false;
        self.admit_generated_frame_commands(commands, generated, commands_include_generated);
        #[cfg(any(test, feature = "test-support"))]
        {
            self.replaying_recorded_commands = false;
        }
        #[cfg(any(test, feature = "test-support"))]
        if self.session.tick > 0 {
            self.trace_house_ai_activation_order(HouseAiActivationOrderTestEvent::DefeatProcessed);
        }

        // `Queue_AI_Multiplayer` computes and stores the sync checksum
        // (`0x00647684`, history `0x00B04474[Frame & 0xFF]`) before any
        // command transfer or dispatch; the single-player body never does.
        if self.command_queue_route == CommandQueueRoute::Multiplayer {
            self.frame_sync_checksum = Some(FrameSyncChecksum {
                frame: self.session.binary_frame,
                checksum: self.compute_retail_multiplayer_checksum(),
            });
        }

        // EventClass dispatch is a Main_Tick tail rung: the complete live
        // Logic walk observes frame N's pre-command state, so an accepted
        // command first changes that object's AI behavior on frame N+1.
        let (executed, spawned) =
            self.apply_due_commands(commands, rules, execute_tick, overlay_registry, effects);
        *executed_commands += executed;
        *spawned_entities |= spawned;

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
        self.session.total_sim_ms = self.session.total_sim_ms.saturating_add(tick_ms as u64);
        self.session.binary_frame = self.session.binary_frame.wrapping_add(1);
        #[cfg(any(test, feature = "test-support"))]
        self.trace_master_frame_rung(MasterFrameTestRung::FrameCommit);
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::BinaryFrameCommitted);

        // The one ordinary ProcessPendingDelete drain follows the native frame
        // commit. Alive queue entries keep their position; ready duplicate IDs
        // collapse and physically finalize exactly once.
        #[cfg(any(test, feature = "test-support"))]
        self.trace_master_frame_rung(MasterFrameTestRung::PendingDelete);
        self.process_pending_delete_with(
            rules,
            overlay_registry,
            effects,
            1,
            self.session.started(),
        );

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
        self.session.tick = execute_tick;
        true
    }

    /// Run the copied TeamClass AI pass before the live LogicClass object walk.
    ///
    /// Native `LogicClass::PerTickUpdate @ 0x0055AFB0` snapshots the Team array
    /// and calls TeamClass's `+0x5C` slot at `0x0055B502..0x0055B59F`; only
    /// afterwards can Building/Foot `ReceiveDamage` reach the base-defense
    /// suspension writer. A frame-N hit therefore arms suspension for the next
    /// Team visit, not the Team visit already completed on frame N.
    fn run_team_script_pass(
        &mut self,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        #[cfg(any(test, feature = "test-support"))]
        self.trace_master_frame_rung(MasterFrameTestRung::TeamScript);
        if let Some(rules) = rules {
            self.run_team_ai_pass(rules, overlay_registry, effects);
        }
    }

    /// Install fixed AIMD plus scenario AI definitions after RuleSet identities
    /// are interned and before the first gameplay tick. This is data ingress
    /// only: the installed VM contains no live TeamClass instances.
    ///
    /// A fixed-AIMD definition that fails RuleSet resolution refuses the whole
    /// install (`Err`, nothing installed); scenario-origin omissions install
    /// and come back as logged, nonfatal diagnostics.
    pub fn install_team_ai_registry(
        &mut self,
        registry: &crate::rules::team_ai_ini::TeamAiIniRegistry,
        rules: &RuleSet,
    ) -> Result<
        Vec<crate::sim::team_script_vm::TeamAiInstallDiagnostic>,
        Vec<crate::sim::team_script_vm::TeamAiInstallDiagnostic>,
    > {
        let (vm, diagnostics) =
            TeamScriptVm::from_ini_registry(registry, &mut self.interner, rules);
        if diagnostics
            .iter()
            .any(crate::sim::team_script_vm::TeamAiInstallDiagnostic::is_fixed_source_refusal)
        {
            return Err(diagnostics);
        }
        self.team_script_vm = vm;
        for diagnostic in &diagnostics {
            log::warn!("Team AI install diagnostic: {diagnostic:?}");
        }
        Ok(diagnostics)
    }

    /// The Drive, Hover and Ship locomotors each spawn `Rules->Wake`
    /// (Rules+0x94) from their own process; [`wake_anchor_for`] holds the
    /// three gates. All three build `AnimClass(Wake, Location, 0, 1, 0x600,
    /// 0, 0)` at the unit's exact `Location` (`+0x9C`), not the cell centre,
    /// so each wake stays where the hull was and the trail forms behind it as
    /// the unit advances.
    pub(crate) fn spawn_wakes_for_frame(&mut self, rules: &RuleSet) {
        if !self.session.binary_frame.is_multiple_of(10)
            && !self.session.binary_frame.is_multiple_of(8)
        {
            return;
        }
        let binary_frame = self.session.binary_frame;
        let terrain = self.resolved_terrain.as_ref();
        let rules_context = Some(crate::sim::movement::SpeedRules::new(
            rules,
            &self.interner,
            &self.houses,
        ));
        let wake_positions: Vec<(u16, u16, SimFixed, SimFixed, u8)> = self
            .substrate
            .entities
            .keys_sorted()
            .iter()
            .filter_map(|id| {
                let entity = self.substrate.entities.get(*id)?;
                let underwater = self
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| object.underwater);
                wake_anchor_for(entity, rules_context, underwater, terrain, binary_frame)
            })
            .collect();
        if wake_positions.is_empty() {
            return;
        }
        // `new AnimClass(Wake, &coord, 0, 1, 0x600, 0, 0)`: delay 0, one
        // loop, draw flags 0x600, no ZAdjust, not reversed. Going through the
        // AnimClass constructor puts the wake on the sorted ground layer with
        // its art `YSortAdjust=-288`, so it sorts under the hull instead of
        // over it.
        let wake_name = self.interner.intern(&rules.general.wake.name);
        for (rx, ry, sub_x, sub_y, z) in wake_positions {
            let descriptor = AnimClassSpawnDescriptor {
                loop_count: 1,
                draw_flags: WAKE_DRAW_FLAGS,
                ..AnimClassSpawnDescriptor::new(wake_name, rx, ry, sub_x, sub_y, z)
            };
            let world =
                crate::sim::anim_class::AnimWorldCoord::from_cell_sub_z(rx, ry, sub_x, sub_y, z);
            if let Err(error) = self.spawn_anim_at_world(rules, descriptor, world) {
                log::debug!(
                    "wake [{}] did not construct: {error}",
                    rules.general.wake.name
                );
            }
        }
    }

    /// A frame reads only the canonical `path_grid`. A fixture that builds its
    /// grid beside the simulation seeds it here once; after that the installed
    /// grid (and any republish of it) wins over the fixture's copy.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn install_fixture_path_grid(&mut self, path_grid: Option<&PathGrid>) {
        if self.path_grid.is_none()
            && let Some(grid) = path_grid
        {
            self.path_grid = Some(Arc::new(grid.clone()));
        }
    }

    /// Fixture-only frame adapter (F09): unit tests drive one Main_Tick-shaped
    /// frame with explicitly supplied rules/heights/navigation. Production and
    /// tooling advance exclusively through `SimRuntime::advance_frame`, whose
    /// resources are bound at construction and cannot be substituted per call.
    #[cfg(any(test, feature = "test-support"))]
    pub fn advance_tick(
        &mut self,
        commands: &[CommandEnvelope],
        rules: Option<&RuleSet>,
        path_grid: Option<&PathGrid>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        tick_ms: u32,
    ) -> TickResult {
        self.install_fixture_path_grid(path_grid);
        self.advance_master_frame(
            commands,
            rules,
            overlay_registry,
            tick_ms,
            TickLane::Ordinary,
            None,
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
    pub fn advance_app_frame(
        &mut self,
        commands: &[CommandEnvelope],
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        tick_ms: u32,
        lane: TickLane,
        trigger_inputs: Option<TriggerInputs<'_>>,
        effects: FrameEffects<'_>,
    ) -> Result<SimFrameOutput, FrameAdvanceError> {
        let frame = self.advance_master_frame(
            commands,
            rules,
            overlay_registry,
            tick_ms,
            lane,
            trigger_inputs,
            effects,
        );
        // A failed frame must not carry a previous caller's derived interest
        // into a later headless, replay or ordinary presentation frame.
        self.unit_voice_visit_interest.clear();
        Ok(self.collect_frame_output(frame?, effects))
    }

    /// The session owner's GameMode route for the next admitted frames.
    pub fn set_command_queue_route(&mut self, route: CommandQueueRoute) {
        self.command_queue_route = route;
    }

    /// From the next frame on, hash the state for the TickResult only after
    /// frames whose tick is a multiple of `frames` (1: every frame), and after
    /// every frame that ends the match (an EXIT, a quit, the current house's
    /// win or loss, a lost connection), whose `frame_committed` is false. A
    /// fixed frame count, so the same commands always hash the same frames.
    pub fn hash_state_every(&mut self, frames: std::num::NonZeroU64) {
        self.state_hash_interval = Some(frames);
    }

    /// Install the sole audio owner's read-only pending-interest projection
    /// immediately before an admitted frame. No voice names/state live here.
    pub fn prepare_unit_voice_visits(&mut self, owners: std::collections::BTreeSet<u64>) {
        self.unit_voice_visit_interest = owners;
    }

    /// Emit only interested reached heads, in the existing ordered channel.
    fn record_unit_voice_visit(&mut self, owner: u64) {
        if self.unit_voice_visit_interest.contains(&owner) {
            self.sound_events
                .push(SimSoundEvent::UnitVoiceVisit { owner });
        }
    }

    fn collect_frame_output(
        &mut self,
        frame: MasterFrameOutput,
        effects: FrameEffects<'_>,
    ) -> SimFrameOutput {
        let MasterFrameOutput {
            tick,
            admitted_commands,
        } = frame;
        let lighting_events = std::mem::take(&mut self.lighting_sources.pending);
        let trigger_effects = std::mem::take(&mut self.trigger_effects);
        // Preserve the established terminal-frame gate: these are committed
        // light-vector facts and the next admitted frame clears the producer
        // buffer before combat runs.
        let combat_lights = if tick.frame_committed {
            std::mem::take(&mut self.combat_light_requests)
        } else {
            Vec::new()
        };
        let lifecycle_outputs = std::mem::take(&mut self.lifecycle_outputs);
        let multiplayer_checksum = self.frame_sync_checksum.take();
        let overlay_updates = std::mem::take(&mut self.frame_overlay_updates);
        let overlay_removals = std::mem::take(&mut self.frame_overlay_removals);
        let fire_events = std::mem::take(&mut self.fire_events);
        let sound_events = std::mem::take(&mut self.sound_events);
        let sound_events_delivered = effects.delivered_sound_events();
        assert!(sound_events_delivered <= sound_events.len());
        SimFrameOutput {
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
            lighting_events,
            multiplayer_checksum,
        }
    }

    /// Advance exactly one authoritative simulation frame.
    ///
    /// This is the sole Main_Tick-shaped entry point, reached in production
    /// only through `advance_app_frame` (F09). The fixture-only `advance_tick`
    /// adapter and the replay fixture runners are `#[cfg(test)]`; gameplay
    /// supplies its map definitions through `trigger_inputs`.
    pub(crate) fn advance_master_frame(
        &mut self,
        commands: &[CommandEnvelope],
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        tick_ms: u32,
        lane: TickLane,
        trigger_inputs: Option<TriggerInputs<'_>>,
        effects: FrameEffects<'_>,
    ) -> Result<MasterFrameOutput, FrameAdvanceError> {
        let effects = effects
            .with_rules(rules)
            .with_overlay_registry(overlay_registry);
        // A failed earlier frame never reached collection; drop its value.
        self.frame_sync_checksum = None;
        self.combat_light_requests.clear();
        self.pending_projectile_detonations.clear();
        self.pending_wave_damage_requests.clear();
        let animation_sequences = rules.map(RuleSet::animation_sequences);
        // The wrapping native frame counter is committed LATE (end of this fn,
        // beside self.session.tick) so every phase sees the same pre-increment
        // frame N. execute_tick stays here: command scheduling below filters on
        // the separate monotonic ordinal.
        let execute_tick = self.session.tick.saturating_add(1);
        let mut executed_commands = 0usize;
        let mut spawned_entities = false;
        let mut destroyed_structure = false;
        // Every phase below reads the canonical `path_grid` at its point of
        // use, so a mid-frame republish is visible to the next reader.
        // No command-boundary drain: command-applied deaths (sell, MCV/slave
        // deploy-undeploy, engineer capture) now stay in the Dying window like
        // combat deaths, freed only by the single end-of-tick drain — matching
        // gamemd's one ProcessPendingDelete at the tail of Main_Tick. The mid-
        // tick raw-store consumers (vision, power, production, movement, miner,
        // aircraft, …) are dying-gated, so a corpse is excluded until that drain.
        let mut bridge_state_changed = false;

        if lane == TickLane::Ordinary {
            executed_commands +=
                self.apply_due_frame_ingress_commands(commands, rules, execute_tick, effects);
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trace_master_frame_rung(MasterFrameTestRung::SessionCommands);

        let mut commands = if lane == TickLane::Ordinary {
            self.admit_frame_commands(commands, rules)
        } else {
            Vec::new()
        };

        // MainTick55DBC8 precedes Logic55DC9E (including trigger polling).
        self.sort_display_ground(rules);

        self.lifecycle_outputs.push(LifecycleOutput::LogicVisit);

        // YR LogicClass::Update establishes trigger state before visiting the
        // live LogicVector, so object work in this frame observes its actions.
        #[cfg(any(test, feature = "test-support"))]
        self.trace_master_frame_rung(MasterFrameTestRung::Triggers);
        if let Some(inputs) = trigger_inputs {
            self.poll_triggers_for_master_frame(inputs, overlay_registry, effects);
        }

        // Logic55B29A/55B2AD: consume prior pending conceal at the signed
        // pre-increment native frame boundary, before ore, Teams and live objects.
        self.tick_pending_shroud_conceal(
            UninitContext::new(rules, overlay_registry).with_effects(effects),
        );

        // Interleaved object-AI pass: each object's mission precedes its own
        // locomotion. Unit/Infantry firing and their class-specific tails run
        // in that same slot; newly appended Bullets/Anims join the live walk.
        // Each live object
        // gets its `+0xC4` AI-counter tick, its owner-local queued-mission
        // promotion (Ready→Commence), and its absorbed mission-handler
        // dispatch (Harvest: the miner FSM, timer-gated with the post-handler
        // epilogue write) here. Player commands from frame N-1 are already
        // represented; frame N's EventClass tail is dispatched below.
        // Movement stays before the Phase-3 vision recompute.
        //
        // Dock-reservation corpse sweep first (was the global miner tick's
        // pre-pass): reservations held by/on dying objects release before the
        // Harvest dispatches run.
        if let Some(rules) = rules {
            self.tick_scenario_lighting_transition(rules);
            self.tick_ore_growth_rungs(rules, overlay_registry);
            // `BombListClass::UpdateAll` follows growth and spread (0x0055B4E1).
            self.bomb_list_update(rules);
            // `Kamikaze__Update` follows it (0x0055B4F0). `LightningStorm::
            // Process` runs whatever the superweapons option (0x0055B5C8): a
            // map's NUKE weapon starts the nuke flash without a Super.
            self.kamikaze_update(rules, effects);
            // Logic55B5A1..55B5BD: reverse Disk AI before LaserUpdate and
            // ordinary object visits. FireAt births below wait until next tick.
            bridge_state_changed |= self.tick_disk_lasers(rules, overlay_registry, effects);
            self.lifecycle_outputs.push(LifecycleOutput::LaserUpdate {
                frame: self.session.binary_frame as i32,
            });
            bridge_state_changed |= crate::sim::superweapon::tick_active_superweapon_effects(
                self,
                rules,
                overlay_registry,
                effects,
            );
            self.radiation.tick_decay(
                self.session.binary_frame,
                &rules.radiation,
                self.resolved_terrain.as_ref(),
                &mut self.lighting_sources.pending,
            );
        }
        // Native TeamClass AI precedes the main LogicClass object vector. In
        // particular, ordinary object ReceiveDamage paths that arm a Team's
        // base-defense suspension occur only after this frame's Team visit.
        self.run_team_script_pass(rules, overlay_registry, effects);
        // The live pass commits each object's AI, movement and lifecycle effects
        // before advancing its cursor; later phases need only these outcomes.
        #[cfg(any(test, feature = "test-support"))]
        self.trace_master_frame_rung(MasterFrameTestRung::LogicVector);
        // Receipts are frame-local: an aborted earlier frame must not leak one.
        self.fire_requests = Default::default();
        let object_pass = self.advance_live_object_pass(rules, overlay_registry, effects)?;
        let ownership_changed = object_pass.ownership_changed();
        spawned_entities |= std::mem::take(&mut self.mission_spawned_entities);
        let movement_stats = object_pass.movement;
        destroyed_structure |= object_pass.destroyed_structure;
        bridge_state_changed |= object_pass.bridge_state_changed;
        let tube_turn_owned_ids = object_pass.tube_turn_owned_ids;
        //Gate Open runs in Building Mission AI; factory clearance runs in
        //the arriving Unit's Per_Cell_Process before Ready/Commence.
        // Aircraft missions retain their existing deferred receiver. Building
        // shots already finished in their own dynamic Logic slots.
        let fire_requests = std::mem::take(&mut self.fire_requests);

        // Wake anims under moving units on water (native gate and cadence in
        // `spawn_wakes_for_frame`).
        if let Some(rules) = rules {
            self.spawn_wakes_for_frame(rules);
        }

        // --- Phase 3: Vision refresh ---
        // DEPENDS ON: movement (positions updated), spawn (new entities need LOS).
        // PRODUCES: fog state used by combat targeting (phase 5).
        let vision_config = self.sight_reveal_config(rules);
        self.refresh_fog(&vision_config, rules);

        if let Some(rules) = rules {
            // --- Phase 5: Combat + Turret rotation ---
            // DEPENDS ON: vision/fog (targeting uses fog state), power (cloaking).
            // Buildings, Units and Infantry fired in their own live slots.
            // Units also committed Facing_Update after paid movement. This
            // tail hosts the remaining classes;
            // PRODUCES: damage, deaths, bridge damage, fire events. Ordered
            // ReceiveDamage retaliation is committed inline.
            // Engineer repair, capture and consumption, and a C4 plant, occur
            // synchronously at Walk's completed step in the object pass; a
            // charge goes off in its building's own Update visit.
            // Preserve live-object order among the remaining class requests.
            // Foot fire and its same-pass Bullet visits have already completed.
            let logic_order = self.live_object_order_snapshot();
            // BulletClass/WaveClass AI already ran at each object's mixed
            // LogicClass slot. Keep their established receiver boundary here.
            let projectile_detonations = std::mem::take(&mut self.pending_projectile_detonations);
            let sonic_damage_requests = std::mem::take(&mut self.pending_wave_damage_requests);
            // Rules-less fixture dispatch is the only producer of this
            // compatibility buffer. If a caller supplies Rules later in the
            // same frame, retain the live one-receiver-at-a-time contract.
            for request in sonic_damage_requests {
                self.commit_logic_wave_damage_request(rules, overlay_registry, &request, effects);
            }
            let first_tail_id = self.substrate.next_stable_object_id;
            let combat_result = self.tick_combat_with_fatal_lifecycle(
                rules,
                overlay_registry,
                tick_ms,
                &logic_order,
                &tube_turn_owned_ids,
                &fire_requests,
                &projectile_detonations,
                effects,
            );
            bridge_state_changed |=
                self.visit_combat_tail(first_tail_id, rules, overlay_registry, effects);
            // Unit facing already committed immediately after that Unit's
            // FireAt, before this frame's next live Logic object. Each
            // SpawnManager ran in its owner's TechnoClass::AI slot.
            let receipt = combat_result
                .consequences
                .commit(self, rules, overlay_registry, effects);
            destroyed_structure |= receipt.structure_destroyed;
            bridge_state_changed |= receipt.bridge_state_changed;

            // No end-of-Phase-5 drain: combat-killed structures/voxels stay in
            // the Dying window through the Phase 5.5-8.5 consumers and are freed
            // only by the single end-of-tick drain (gamemd's one ProcessPending-
            // Delete). Those consumers (production speed/factory-spawn scans,
            // repairs, retaliation, miner, aircraft) are dying-gated. Combat
            // post-processing above still reads the dead ids while resolvable
            // (count decrement, owner snapshot) — that runs before this point.
            // --- Phase 6: Passengers ---
            // Retaliation is not a phase: every receiver issues its Mission
            // Override inline (`TechnoClass::ReceiveDamage 0x00702A43`).
            passenger::tick_passenger_system(self, rules, overlay_registry, effects);
            // `IonBlastClass::UpdateAll @ 0x0053D310` follows the object loop
            // (`0x0055B64B`).
            self.update_ion_blasts();
            // `LogicClass__PerTickUpdate @ 0x0055AFB0` calls
            // `MapClass__UpdateCrateRegenTimers @ 0x0056BBE0` at `0x0055B65A`,
            // between `AlphaShapeClass::PurgeDisabled` and the Tactical,
            // Factory and House callbacks, on the pre-increment frame counter
            // this tick's phases have all observed.
            // The `Option` is a Rust availability gate with no native
            // counterpart: `MapClass__UpdateCrateRegenTimers` owns its own two
            // gates and nothing else. The sole production caller
            // (`SimRuntime::advance_frame`) always binds the registry, and
            // `crate_regen_rung_runs_in_every_production_frame` pins that.
            if let Some(overlay_registry) = overlay_registry {
                #[cfg(any(test, feature = "test-support"))]
                self.trace_master_frame_rung(MasterFrameTestRung::CrateRegen);
                let normal_lighting = self
                    .session
                    .lighting
                    .profile(crate::sim::scenario_session::ScenarioLightingProfile::Normal)
                    .into();
                let regen = crate::sim::crates::tick_crate_regeneration(
                    self,
                    rules,
                    overlay_registry,
                    normal_lighting,
                );
                if regen.visible != 0 {
                    // Native Mark mutates live CellClass land/zone/bridge state
                    // synchronously. Rust's BridgeRuntimeState records are a
                    // derived cache, so refresh them once per pass that installed an
                    // overlay and let the existing frame-boundary navigation
                    // seam republish; this adds no RNG or ordering boundary.
                    //
                    // Deliberately not refreshed on a removal-only pass: only a
                    // Mark can stamp bridge topology, and the random placer
                    // refuses any cell that already carries an overlay, so a
                    // crate can never sit on — or un-stamp — a bridge piece
                    // under stock rules.
                    bridge_state_changed |= self.refresh_bridge_runtime_after_crate_mark();
                }
            }

            // --- Phase 7: Production + Docks + Ore ---
            // DEPENDS ON: completed live-object visits, including depot spending.
            // PRODUCES: factory charges/change flags, dock/ore updates.
            // Phase 7, FIRST production step — the authoritative factory sweep (C1:
            // factories step BEFORE the house tail `run_late_region`). `step_all`
            // charges each armed factory's per-step cost against the REAL wallet
            // (house.economy.credits) in insertion_seq (temporal) order. Completed heads retain their
            // change flag until the next Strip prefix; its PLACE event tail
            // releases the object and advances the queue-of-record there.
            //
            // Native55AFB0 visits all objects before55B66A's Factory sweep.
            // Depot service now spends inside its Building mission visit, so
            // every factory observes the wallet after that object's repair.
            production::step_factories(self);
            #[cfg(any(test, feature = "test-support"))]
            self.trace_house_ai_activation_order(
                HouseAiActivationOrderTestEvent::ProductionCompleted,
            );
            crate::sim::docking::bunker_install::tick_bunker_install(
                self,
                rules,
                overlay_registry,
                effects,
            );
            if spawned_entities {
                self.refresh_fog(&vision_config, Some(rules));
            }
        }

        // ===== SPINE REGION: LATE — AI, defeat, anims, frame commit =====
        // (Step 3a skeleton: extracted to a region method; call order unchanged —
        // behavior-preserving.) Native-spine note: gamemd runs HouseClass updates
        // (incl. defeat) in the tail and commits the frame counter late; AI
        // placement is project-deferred and kept in its current slot.
        let frame_committed = self.run_late_region(
            &mut commands,
            rules,
            overlay_registry,
            tick_ms,
            execute_tick,
            &mut executed_commands,
            &mut spawned_entities,
            effects,
        );
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
        #[cfg(debug_assertions)]
        self.debug_assert_logic_membership_consistent();
        #[cfg(debug_assertions)]
        self.debug_assert_lifecycle_consistent();
        #[cfg(debug_assertions)]
        self.debug_assert_factory_invariants();

        // Living sprite animation state belongs to the committed simulation
        // frame. Keep it inside the authoritative frame transaction so hashed
        // side effects and every snapshot observe the same state. Dying
        // animation advancement remains in the live-object scheduler, which
        // owns the single pending-delete drain above.
        if frame_committed && let Some(animation_sequences) = animation_sequences {
            let game_options = self.session.game_options.clone();
            let binary_frame = self.session.binary_frame;
            let (entities, interner) = self.entities_mut_and_interner();
            animation::tick_non_dying_animations(
                entities,
                animation_sequences,
                rules,
                &game_options,
                interner,
                binary_frame,
            );
        }
        building_anim::finalize(self);
        #[cfg(debug_assertions)]
        self.debug_assert_logic_membership_consistent();
        #[cfg(debug_assertions)]
        self.debug_assert_lifecycle_consistent();
        let terminal_score_finalized =
            self.natural_outcome_exit_ready() && self.finalize_terminal_score_snapshot();
        // A frame that ends the match hashes whatever its tick: the replay
        // log records it as its last frame.
        let state_hash = (!frame_committed
            || self
                .state_hash_interval
                .is_some_and(|frames| self.session.tick.is_multiple_of(frames.get())))
        .then(|| self.state_hash());
        Ok(MasterFrameOutput {
            admitted_commands: commands,
            tick: TickResult {
                tick: self.session.tick,
                frame_committed,
                executed_commands,
                state_hash,
                terminal_score_finalized,
                spawned_entities,
                destroyed_structure,
                ownership_changed,
                bridge_state_changed,
                movement: movement_stats,
            },
        })
    }
}

#[cfg(test)]
#[path = "world_tests.rs"]
pub(crate) mod tests;

#[cfg(test)]
#[path = "smudge_integration_tests.rs"]
mod smudge_integration_tests;

#[cfg(test)]
#[path = "harvest_attack_return_oracle_tests.rs"]
mod harvest_attack_return_oracle_tests;
#[cfg(test)]
#[path = "harvest_field_oracle_tests.rs"]
mod harvest_field_oracle_tests;
#[cfg(test)]
#[path = "refinery_dock_continuation_tests.rs"]
mod refinery_dock_continuation_tests;
#[cfg(test)]
#[path = "refinery_dock_oracle_tests.rs"]
mod refinery_dock_oracle_tests;
#[cfg(test)]
#[path = "refinery_dock_retail_tests.rs"]
mod refinery_dock_retail_tests;

#[cfg(test)]
#[path = "harvest_field_cycle_tests.rs"]
mod harvest_field_cycle_tests;

#[cfg(test)]
#[path = "slave_manager_oracle_tests.rs"]
mod slave_manager_oracle_tests;

#[cfg(test)]
#[path = "slave_manager_cycle_tests.rs"]
mod slave_manager_cycle_tests;

#[cfg(test)]
#[path = "building_sale_oracle_tests.rs"]
mod building_sale_oracle_tests;

#[cfg(test)]
#[path = "building_repair_oracle_tests.rs"]
mod building_repair_oracle_tests;

#[cfg(test)]
#[path = "refinery_dock_cycle_tests.rs"]
mod refinery_dock_cycle_tests;

#[cfg(test)]
#[path = "cmin_dock_oracle_tests.rs"]
mod cmin_dock_oracle_tests;

#[cfg(test)]
#[path = "cmin_dock_cycle_tests.rs"]
mod cmin_dock_cycle_tests;

#[cfg(test)]
#[path = "passenger_escape_oracle_tests.rs"]
mod passenger_escape_oracle_tests;

#[cfg(test)]
#[path = "world_orders_bridge_repair_tests.rs"]
mod world_orders_bridge_repair_tests;

#[cfg(test)]
#[path = "rng_routing_tests.rs"]
mod rng_routing_tests;

#[cfg(test)]
#[path = "slice6_retask_tests.rs"]
mod slice6_retask_tests;

#[cfg(test)]
#[path = "mission_authoritative_tests.rs"]
mod mission_authoritative_tests;

#[cfg(test)]
#[path = "global_parity_harness_tests.rs"]
mod global_parity_harness_tests;

#[cfg(test)]
#[path = "production_shadow_tests.rs"]
mod production_shadow_tests;

#[cfg(any(test, feature = "test-support"))]
pub mod factory_infantry_output_tests;
#[cfg(test)]
mod factory_jumpjet_output_tests;

#[cfg(test)]
#[path = "radar_dirty_ack_tests.rs"]
mod radar_dirty_ack_tests;

#[cfg(test)]
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
/// Water. The wake is anchored at the exact leptons. A Jumpjet or Fly flier
/// over water leaves none.
pub(crate) fn wake_anchor_for(
    entity: &crate::sim::game_entity::GameEntity,
    rules: Option<crate::sim::movement::SpeedRules<'_>>,
    underwater: bool,
    terrain: Option<&ResolvedTerrainGrid>,
    binary_frame: u32,
) -> Option<(u16, u16, SimFixed, SimFixed, u8)> {
    // Foot4DA81A bypasses the locomotor (and its movement-wake producer)
    // while sinking. Unit AI owns that hull's separate jittered wake.
    if entity.sinking.is_active() {
        return None;
    }
    let on_cadence = match entity.locomotor.as_ref().map(|loco| loco.active_kind()) {
        Some(
            crate::rules::locomotor_type::LocomotorKind::Drive
            | crate::rules::locomotor_type::LocomotorKind::Hover,
        ) => binary_frame.is_multiple_of(10),
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
    if cell.yr_cell_land_type != crate::rules::terrain_rules::LandType::Water.as_index() {
        return None;
    }
    Some((
        entity.position.rx(),
        entity.position.ry(),
        entity.position.sub_x(),
        entity.position.sub_y(),
        entity.position.z(),
    ))
}

#[cfg(test)]
#[path = "tibtre_oracle_tests.rs"]
mod tibtre_oracle_tests;

#[cfg(test)]
#[path = "ore_spread_oracle_tests.rs"]
mod ore_spread_oracle_tests;

#[cfg(test)]
mod drop_as_bomb_integration_tests;

mod airstrike_aircraft;

#[cfg(test)]
mod radiation_overlap_tests;
