//! Game simulation - owns the EntityStore and deterministic tick stepping.
//!
//! The Simulation is the authoritative game state. It spawns entities from
//! map data, executes command envelopes on fixed ticks, advances gameplay
//! systems, and exposes deterministic state hashing for replay/desync checks.
//!
//! Responsibility boundaries:
//! - `command_schedule.rs`: queue admission, due batches, house order and group destinations
//! - `world_commands.rs`: individual payloads and selection/ownership helpers
//! - `world_hash.rs` / `hash_schema.rs` — deterministic folds and historical projections
//! - `world_spawn.rs` — entity spawning from map data and production
//! - `world_orders.rs` — order-intent tick systems (attack-move, guard, area-guard)
//! - `lifecycle.rs` / `substrate.rs` — object transitions, stores and registration order
//! - `object_turn.rs` — complete live-object turn and pass outcomes
//! - `techno_ai.rs` — per-object AI dispatch within each turn

pub(crate) mod authored_load_host;
mod bridge_hut_scatter;
pub(crate) mod bridge_orchestrator;
pub(crate) mod building_anim;
pub mod edge_cell;
mod gap_generator;
mod hash_schema;
mod house_base;
mod house_defeat;
pub(crate) use house_base::HouseBaseState;
mod infantry_terminal;
mod jumpjet_cruise;
#[cfg(test)]
pub(crate) use infantry_terminal::InfantryDeathSequence;
pub(crate) use infantry_terminal::{InfantryDeathPostlude, InfantryTerminal};
mod aircraft_attack;
mod aircraft_fire_location;
pub(crate) mod damage_consequences;
pub(crate) mod display_layers;
mod display_registry;
mod fly_landing;
mod fly_orders;
mod frame_error;
mod lifecycle;
mod load_object_lifecycle;
mod logic_vector;
mod move_cell_input;
mod navigation;
mod object_turn;
pub use frame_error::FrameAdvanceError;
mod shroud_refresh;
mod track_cell_recalc;
#[cfg(test)]
use object_turn::shp_vehicle_counter_admitted;
mod projectile_collision;
mod substrate;
mod techno_ai;
#[cfg(test)]
pub(crate) use techno_ai::ObjectAiCtx;
pub(crate) use techno_ai::harvester_enter_idle_mode_selector;
pub(crate) use techno_ai::queue_foot_enter_idle_mode;
mod command_schedule;
pub(crate) mod techno_ai_cloak;
pub(crate) mod unit_post;
mod world_commands;
mod world_hash;
mod world_orders;
mod world_spawn;

#[cfg(test)]
mod aircraft_deployment_tests;
#[cfg(test)]
mod damage_consequence_tests;
#[cfg(test)]
mod eva_dispatch_tests;
#[cfg(test)]
mod fly_height_tests;
#[cfg(test)]
pub(crate) mod gap_generator_tests;
#[cfg(test)]
mod gsi_04_18_tests;
#[cfg(test)]
mod house_ai_activation_tests;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
pub(crate) use lifecycle_tests::common_raw_terrain_cell as common_raw_test_terrain_cell;
#[cfg(test)]
mod team_script_vm_tests;

pub(crate) use lifecycle::{
    ConcealOutcome, LifecycleOutput, NULL_TARGET_CELL_SENTINEL, PlacementEvidence, RevealOutcome,
    RevealPosition, RevealRequest, UninitContext,
};
#[cfg(test)]
pub(crate) use lifecycle::{LifecycleTestEvent, RevealFailure};
pub(crate) use load_object_lifecycle::LoadObjectLifecycle;
pub(crate) use logic_vector::LogicVector;
pub use substrate::EnterOrderCounter;
pub(crate) use substrate::ObjectSubstrate;
pub(crate) use world_spawn::{GeneratedTechnoInitError, GeneratedTechnoInitTable};

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::map::actions::ActionMap;
use crate::map::bridge_facts::{BRIDGE_FLAG_DESTROYED_OR_RAMP, BRIDGE_FLAG_STRUCTURAL};
use crate::map::entities::EntityCategory;
use crate::map::events::EventMap;
use crate::map::houses::HouseAllianceMap;
use crate::map::overlay::OverlayEntry;
use crate::map::playfield::PlayfieldBounds;
use crate::map::resolved_terrain::{
    RealCellBridgeFlags0x1180, ResolvedTerrainGrid, SharedCellDummy,
};
use crate::map::trigger_graph::TriggerGraph;
use crate::map::triggers::TriggerMap;
use crate::rules::locomotor_type::SpeedType;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::ai::{self, AiPlayerState};
use crate::sim::animation;
use crate::sim::bridge_state::{BridgeRuntimeState, DamageState};
use crate::sim::combat;
use crate::sim::combat::combat_weapon::WeaponSlot;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::{AnimClassSpawnDescriptor, Position};
use crate::sim::docking::aircraft_dock;
use crate::sim::docking::building_dock;
use crate::sim::entity_store::EntityStore;
use crate::sim::house_state::HouseState;
use crate::sim::house_strategy;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::lifecycle_request::LifecycleRequest;
use crate::sim::movement;
use crate::sim::movement::drop_pod_movement;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::movement::rocket_movement;
use crate::sim::movement::teleport_movement;
use crate::sim::movement::tunnel_movement::{self, TunnelProcessContext};
use crate::sim::movement::turret;
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::overlay_grid::{
    WallDamageEvent, WallDamageTransactionHost, WallDirtyStep, WallPointerTarget,
    WallZoneRepairKind, damage_wall_overlay_with_runtime_host, recalc_overlay_passability,
};
use crate::sim::passenger;
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::terrain_cost::{TerrainCostGrid, build_canonical_terrain_cost_grids};
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
use crate::sim::rng::{SimRng, SimRngLogicalState, SimRngLogicalView};
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::team_script_vm::{TeamScriptEffect, TeamScriptVm};
use crate::sim::tiberium::TiberiumPlacementObjectContext;
use crate::sim::trigger_runtime::{TriggerEffect, TriggerRuntime};
use crate::sim::vision::{self, FogState};
use crate::util::fixed_math::SimFixed;

/// Dev/test fallback seed. Real launches negotiate a per-match seed through
/// `ScenarioDescriptor`; nothing on the launch path may rely on this value.
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
    pub state_hash: u64,
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

/// One authoritative frame plus the transient facts emitted while producing it.
///
/// Channel order is preserved within each vector. The app owns cross-channel
/// presentation order; collecting this value only transfers ownership and does
/// not invent a single mixed event timeline.
#[derive(Debug)]
pub(crate) struct SimFrameOutput {
    pub tick: TickResult,
    pub trigger_effects: Vec<TriggerEffect>,
    pub lifecycle_outputs: Vec<LifecycleOutput>,
    pub overlay_updates: Vec<OverlayEntry>,
    /// Coordinates whose overlay identity was erased this frame. Presentation
    /// drops their render entry; `overlay_updates` only ever upserts occupied
    /// cells and can never express a removal.
    pub overlay_removals: Vec<(u16, u16)>,
    pub sound_events: Vec<SimSoundEvent>,
    pub fire_events: Vec<SimFireEvent>,
    pub invulnerability_impacts: Vec<crate::sim::combat::InvulnerabilityImpactEffect>,
    pub(crate) lighting_events: Vec<crate::sim::light_sources::LightingEvent>,
}

/// Front-end admission lane for one Main_Tick call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TickLane {
    /// Normal gameplay: commands/input dispatch in the Main_Tick tail after
    /// the live object/global update walk.
    Ordinary,
    /// LAN/WOL modal pump: service PerTickUpdate and the late tail only.
    NetworkModal,
}

/// Static map trigger definitions borrowed for one authoritative frame.
///
/// The definitions remain map data; `Simulation` owns the mutable runtime
/// state and evaluates it in the master-frame spine.
#[derive(Clone, Copy)]
pub(crate) struct TriggerInputs<'a> {
    pub graph: &'a TriggerGraph,
    pub triggers: &'a TriggerMap,
    pub events: &'a EventMap,
    pub actions: &'a ActionMap,
    pub waypoints: &'a std::collections::HashMap<u32, crate::map::waypoints::Waypoint>,
    /// Bound match rules used by action callbacks that share ordinary Techno
    /// runtime calculations (not reparsed or substituted by trigger data).
    pub rules: Option<&'a RuleSet>,
}

#[cfg(test)]
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

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HouseAiActivationOrderTestEvent {
    ProductionCompleted,
    HouseAngerDecay(InternedId),
    HouseActivation(InternedId),
    DefeatProcessed,
    AiGenerated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MovementSoundProbe {
    rx: u16,
    ry: u16,
    z: u8,
    sub_x_bits: i32,
    sub_y_bits: i32,
    facing: u8,
    path_index: Option<usize>,
    track_point: Option<u16>,
}

/// A sound event produced during simulation (combat, death, production).
/// Pure data — no audio library dependency. Drained by the app layer each frame.
#[derive(Debug, Clone)]
pub enum SimSoundEvent {
    /// Constructor-time animation start/report sound, keyed to object identity.
    AnimationStarted {
        anim_id: crate::sim::anim_class::AnimId,
        sound_id: InternedId,
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// Animation destruction releases its current handle before optional StopSound.
    AnimationStopped {
        anim_id: crate::sim::anim_class::AnimId,
        stop_sound_id: Option<InternedId>,
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// Native Fly AuxSound1/AuxSound2 at the phase callback world coordinate.
    AircraftPhase {
        sound_id: InternedId,
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// A weapon fired — play its Report= sound.
    WeaponFired {
        report_sound_id: InternedId,
        rx: u16,
        ry: u16,
    },
    /// An entity was destroyed — play its DieSound=.
    EntityDied {
        die_sound_id: InternedId,
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
    /// DeploySound on infantry stance entry or successful unit-to-building conversion.
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
    /// A miner docked at a refinery — play the building's deploy sound.
    /// The app layer should select the healthy or damaged sound variant
    /// based on the refinery's health ratio vs ConditionYellow.
    DockDeploy { building_id: u64 },
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
    /// A base structure / harvester took enemy damage. For the local owner
    /// the app admits `radar` (type 3 base / type 4 miner) on its client
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
    /// (a human house) as its ally took sourced damage. Once the listener's
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
    /// One of the `HouseClass::Update` advice lines
    /// (`EVA_InsufficientFunds` `0x004F8BA0`, `EVA_LowPower` `0x004F8D14`)
    /// for a human house; `event` is the `evamd.ini` section name.
    HouseEva {
        owner: InternedId,
        event: &'static str,
    },
    /// `BuildingClass::Sell @ 0x00449B70`, sell state 2 (`0x00449C99..
    /// 0x00449CE5`, the building is gone): `+0x6DD` (the build-animation-
    /// complete flag: set at `0x004467C9`, cleared in sell states 0 and 1,
    /// so state 2 waits for the sell-down animation to finish),
    /// `TechnoClass+0x41A` (owner is the local player) and `UndeploysInto=`
    /// (`Type+0x408`) null — a Construction Yard undeploys instead and stays
    /// silent. The upgrade-sell path (`0x0044AB22..0x0044AB36`) speaks on
    /// `+0x41A` alone; VERA sells no upgrades. App plays `EVA_StructureSold`.
    /// Timing DRIFT, recorded: native speaks after the sell-down animation
    /// (state 1); VERA's sale is synchronous, so the line comes at the click.
    StructureSold { owner: InternedId },
    /// `BuildingClass::ToggleRepair @ 0x00446FF0` (`0x004470B7`): repair
    /// switched on while `Health != Type.Strength` (`0x00447059`) and the
    /// owner is the local player (`0x004470A4 CALL 0x0050B6F0`). App plays
    /// `EVA_Repairing` for the local owner.
    Repairing { owner: InternedId },
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
    /// The lightning storm actually began — the moment the sky flips to Ion,
    /// which on retail data is ~250 frames *after* the Weather Controller
    /// fired. This is where `StormSound` belongs, not on the launch.
    ///
    /// `SuperClass::Launch @ 0x006CC390` case 2 calls `LightningStorm::Start
    /// @ 0x00539EB0` with `param_2 = [Rules+0x1794]` — `[General]
    /// LightningDeferment`, whose key string `"LightningDeferment"` sits at
    /// `0x0083BD18` and is pushed at `0x00670F82` in `RulesClass::ReadGeneral`
    /// (read `0x00670F75`, stored `0x00670F8F`). `Start`'s body opens with
    /// `if (param_2 != 0) { arm the countdown; store the duration; return; }`,
    /// and that early return is *before* the cue at `0x0053A044`
    /// (`VocClass::PlayAtPos @ 0x00750920`). `LightningStorm::Process @
    /// 0x0053A6C0` decrements the countdown and, at zero, re-enters `Start`
    /// with `param_2` zeroed (`0x0053AAC8 XOR EDX,EDX ; 0x0053AACA CALL
    /// 0x00539EB0`); that second entry is the one that plays it.
    ///
    /// Stock `rulesmd.ini:130` is `LightningDeferment=250`, so the launch call
    /// *always* takes the early return and the cue is always deferred.
    LightningStormBegan,
    /// A lightning bolt struck — play thunder sound.
    SuperWeaponStrike { rx: u16, ry: u16 },
    /// First occupant entered a CanBeOccupied building (cargo 0→1).
    /// Owner is the building owner at AddGarrisonOccupant time; civilian
    /// ownership transfer is reported separately from building reconciliation.
    /// App layer plays EVA_StructureGarrisoned if owner is local human.
    StructureGarrisoned { owner: InternedId },
    /// Last occupant left a garrisoned building (cargo 1→0).
    /// Owner is the **pre-revert** owner — the player whose garrison
    /// just emptied. Matches gamemd's CheckAutoSellOrCivilian which
    /// fires EVA before ChangeOwner. App layer plays EVA_StructureAbandoned
    /// if owner is local human.
    StructureAbandoned { owner: InternedId },
    /// First-occupant SFX from rulesmd [AudioVisual] BuildingGarrisonedSound.
    /// Positional cue gated on owner == local human.
    BuildingGarrisonedSfx { owner: InternedId, rx: u16, ry: u16 },
    /// SFX for conditional reciprocal-link harvester release. Resolved at
    /// the app layer to [AudioVisual] BunkerWallsDownSound (retail value
    /// "TankBunkerDown"). Stock zero-link refinery unload completion does
    /// not emit this event.
    RefineryExitSfx { rx: u16, ry: u16 },
    /// A struck building crossed a damage-state threshold and its type carries
    /// no `DamageSound=` of its own — the global `[AudioVisual]
    /// BuildingDamageSound=` cue, played at the building's own coordinate.
    ///
    /// gamemd: `BuildingClass::ReceiveDamage @ 0x00442230`. After the shared
    /// Techno receiver returns at `0x00442425`, `0x0044242C MOV AL,[ESI+0x90]`
    /// (`ObjectClass::IsAlive`) skips the whole dispatch for a dead building;
    /// otherwise `0x00442476 JMP [EAX*4 + 0x00442C18]` with `EAX = result - 2`
    /// enters `{0x004426AC, 0x004426C8, 0x004424A2, 0x0044247D}`. Entry 0
    /// (result 2) multiplies the `float` at `+0xE8` of the object pointed to
    /// by `BuildingClass+0x30C` by `1.5f` (`0x007E4460`) when that pointer is
    /// non-null, then falls through into entry 1 (result 3). The identity of
    /// that object is UNCHECKED: it is written once by
    /// `BuildingClass::Unlimbo @ 0x00440F5B` from `CALL 0x0062DC50` on
    /// `[BuildingTypeClass+0x764]` and is read and rewritten by
    /// `BuildingClass::UpdateGapGenerator_Tick` (`0x00454E7F`, `0x0045500C`).
    /// Both entries reach `0x004426D2 CMP [type+0x538],-1`, and only an absent
    /// per-type `DamageSound=` continues to
    /// `0x00442700 MOV ECX,[Rules+0x714]` / `0x00442706 CALL
    /// VocClass::PlayAtCoord @ 0x00750E20` at `[ESI+0x9C]`.
    ///
    /// Results 2 and 3 are the threshold crossings computed by
    /// `ObjectClass::ReceiveDamage @ 0x005F5390`: 2 when the hit took HP from
    /// `>= Strength >> 1` to below it, 3 when it took HP from above
    /// `Strength * Rules+0x1708` (ConditionRed) to below it. Ordinary hits
    /// that cross nothing return 1 and are silent.
    BuildingDamagedSfx { rx: u16, ry: u16 },
    /// A techno of any category crossed the half-strength threshold and its
    /// type authors a non-empty `VoiceFeedback=` — the damage voice line.
    ///
    /// gamemd: `TechnoClass::ReceiveDamage @ 0x00701900`. The damage result
    /// selects an arm through `0x00702049 JMP [EDI*4 + 0x00702D24]`
    /// (`{0x007027F7, 0x00702713, 0x00702695, 0x007027F7, 0x00702050}`, with
    /// `EDI` forced to 4 when `[ESI+0x6C]` Health is zero at `0x00702035`).
    /// Index 2 — result 2, the `Strength >> 1` crossing — is `0x00702695`:
    ///
    /// - `0x007026A1 MOV EAX,[EDI+0x4E8]` / `0x007026A9 JLE` — an empty
    ///   `VoiceFeedback=` list returns without drawing anything.
    /// - `0x007026B3 MOV ECX,0x886B88` / `CALL 0x0065C7E0` with `(0, 0x63)` —
    ///   `RandomRanged(0, 99)`; `0x007026BD CMP EAX,0x1E ; JGE` drops the cue,
    ///   so it speaks 30 times in 100. **The draw is spent before the owner
    ///   gate**, so this event is emitted for every qualifying crossing on any
    ///   house and the roll happens app-side.
    /// - `0x007026C6 MOV ECX,[ESI+0x21C]` / `CALL HouseClass::IsHumanPlayer @
    ///   0x0050B6F0` — in a skirmish or multiplayer game (`g_GameMode != 0`)
    ///   that is `house == g_PlayerPtr`, i.e. the local player only.
    /// - `0x007026DE CALL 0x0065C780` then `0x007026E7 DIV [EDI+0x4E8]` picks
    ///   `items[rand % count]` from `[EDI+0x4DC]`, and `0x00702709 CALL
    ///   VocClass::PlayAt @ 0x007509E0` plays it at the object's own coords
    ///   (`0x00702702 CALL [EDX+0x48]`).
    ///
    /// `TechnoTypeClass+0x4D8` is the `VoiceFeedback=` vector (items `+0x4DC`,
    /// count `+0x4E8`): `0x00712D9C LEA EDI,[EBP+0x4D8]` in
    /// `TechnoTypeClass::ReadINI` pushes the key string at `0x0084424C`
    /// (`"VoiceFeedback"`) into `CCINIClass::ReadSoundList @ 0x00525430` at
    /// `0x00712DCB`.
    ///
    /// Both draws are on `g_MainRng @ 0x00886B88`, which
    /// `Init_Random_Number_System @ 0x0052FC20` seeds from `g_RngSeed`
    /// alongside the scenario stream but which per-frame draw paths
    /// (`EBolt::DrawRecursiveBolt`, `LaserDrawClass::Draw`,
    /// `RadBeam::DrawAndTickAll`) also consume, so it is not lockstep state.
    /// They therefore belong to the presentation RNG here, not to `sim/`.
    VoiceFeedback {
        /// Owning house — `[ESI+0x21C]`, resolved against the local player.
        owner: InternedId,
        /// The techno's type, for the `VoiceFeedback=` list lookup.
        type_ref: InternedId,
        rx: u16,
        ry: u16,
    },
    /// Tank-bunker walls-up cue — emitted on install. App resolves to
    /// [AudioVisual] BunkerWallsUpSound (retail "TankBunkerUp").
    BunkerWallsUp { rx: u16, ry: u16 },
    /// Tank-bunker walls-down cue — emitted on normal exit / clear teardown.
    /// App resolves to [AudioVisual] BunkerWallsDownSound (retail "TankBunkerDown").
    BunkerWallsDown { rx: u16, ry: u16 },
    /// A paratrooper was dropped from a carrier aircraft.
    /// Played at the drop position; app layer resolves to [AudioVisual] ChuteSound.
    ChuteSound { rx: u16, ry: u16 },
    /// A C4-capable infantry claimed a plant on a CanC4 building.
    /// Played at the attacker's position. App resolves to
    /// `[SealPlaceBomb]` in soundmd.ini.
    C4Planted { rx: u16, ry: u16 },
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
            rx: position.rx,
            ry: position.ry,
            sub_x: position.sub_x,
            sub_y: position.sub_y,
            world_z_leptons: Self::world_z_leptons(position),
        }
    }

    pub(crate) fn wall_crushed(sound_id: String, position: &Position) -> Self {
        Self::WallCrushed {
            sound_id,
            rx: position.rx,
            ry: position.ry,
            sub_x: position.sub_x,
            sub_y: position.sub_y,
            world_z_leptons: Self::world_z_leptons(position),
        }
    }

    pub(crate) fn voc_at(sound_id: String, position: &Position) -> Self {
        Self::voc_at_for(sound_id, None, position)
    }

    pub(crate) fn voc_at_for(
        sound_id: String,
        audible_to: Option<[InternedId; 2]>,
        position: &Position,
    ) -> Self {
        Self::VocAt {
            sound_id,
            audible_to,
            rx: position.rx,
            ry: position.ry,
            sub_x: position.sub_x,
            sub_y: position.sub_y,
            world_z_leptons: Self::world_z_leptons(position),
        }
    }

    fn world_z_leptons(position: &Position) -> i32 {
        position.exact_z_leptons.unwrap_or_else(|| {
            i32::from(position.z).wrapping_mul(crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS)
        })
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

#[cfg(test)]
impl SimFireEvent {
    /// A primary-weapon shot by unit `attacker_id` from cell (0, 0), with no
    /// report, no muzzle animation and a zero fire coordinate.
    pub(crate) fn for_test(attacker_id: u64) -> Self {
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

/// Borrowed names for the three native RNG authorities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimulationRngViews<'a> {
    pub scenario: SimRngLogicalView<'a>,
    pub main: SimRngLogicalView<'a>,
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

/// Constructor-time lighting for a Simulation that has not reached Post_Map_Init
/// yet, and the value a deserialized snapshot starts from before the load path
/// re-supplies the live scenario profile.
fn default_scenario_normal_lighting() -> crate::map::lighting::LightingProfileUnits {
    crate::map::lighting::ParsedLightingProfiles::default().normal
}

/// The game simulation - owns all authoritative game state.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Simulation {
    /// String interner for owner/type_ref — zero-cost ID clones instead of heap Strings.
    pub interner: crate::sim::intern::StringInterner,
    /// Derived cache: interned-id -> object handle for one-hop type resolution.
    /// Built at init by `resolve_type_handles`; empty after deserialize (then
    /// `object_type` uses the name-path fallback). NOT serialized, NOT hashed.
    #[serde(skip)]
    pub(crate) type_handles: crate::sim::type_handle_table::TypeHandleTable,
    /// Pre-resolved `[CombatDamage]` warhead handles (F04). Built by
    /// `resolve_type_handles` beside the type table; rebuilt from rules on
    /// load. NOT serialized, NOT hashed. `None` until resolved.
    #[serde(skip)]
    pub(crate) rule_handles: Option<crate::sim::type_handle_table::ResolvedRuleHandles>,
    /// Credits, build queue state, and rally points.
    pub production: ProductionState,
    /// Session aggregate — scenario identity, seed, authoritative map
    /// bounds, MP start table, per-match options, and the frame clocks
    /// (`tick` plus the wrapping `binary_frame`, committed late at the end of
    /// `advance_tick`). `total_sim_ms` is diagnostic only. Constructed once
    /// from the app-layer descriptor; serialized + hashed except for that
    /// diagnostic accumulator. See `sim::scenario_session`.
    pub session: ScenarioSession,
    /// Scenario RNG — gamemd `Scenario->Random` (Scen+0x218). Drives in-object-tick
    /// sim draws: scatter, sub-cell placement, smudge/destruction, particles,
    /// wall/overlay damage, bridge collapse/destruction presentation, ore growth/spread, TIBTRE,
    /// anim scorch/50-50, miner-dock jitter, and the terminal score projection.
    /// Saved in the Scenario-shaped snapshot and hashed while live. Native load
    /// reads those bytes but immediately calls `Random__Seed(0)`, so deserialization
    /// deliberately discards the saved cursor.
    #[serde(deserialize_with = "deserialize_scenario_rng_reset")]
    pub(crate) scenario_rng: SimRng,
    /// Main/global RNG — gamemd `g_MainRng` (0x00886B88). This is a
    /// process-global cursor: it is neither part of ScenarioClass saves nor
    /// multiplayer checksums, and the live cursor continues across a load.
    #[serde(skip, default = "deserialized_process_rng_placeholder")]
    pub(crate) main_rng: SimRng,
    /// Map-generator RNG — gamemd `g_MapGenRng` (0x00ABE890). VERA's fresh
    /// fixed-map construction uses `Random__Seed(0)`, matching the verified
    /// native fresh-process state; launch `.SED` generation installs its exact
    /// post-RMG continuation. Bridge repair consumes this stream; destruction
    /// remains Scenario-owned. This cursor is not saved or checksummed; Rust
    /// retains it across in-scenario restore, while native cross-match process
    /// retention remains UNCHECKED.
    #[serde(skip, default = "deserialized_process_rng_placeholder")]
    pub(crate) mapgen_rng: SimRng,
    /// Independent wrapping `AbstractClass+0x10` identity cursor, Scenario+214.
    /// Native numeric IDs may duplicate and are neither stable handles nor RNG.
    /// Original689310/689470 preserve the cursor across save/load, including
    ///683560's post-read Scenario reinitialization. See native_id_snapshot.
    /// Runtime constructors still need to consume this shared continuation.
    pub(crate) native_unique_ids: Option<crate::sim::native_identity::NativeUniqueIdCursor>,
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
    /// ephemeral objects are neither gameplay objects nor snapshot/hash state.
    #[serde(skip, default)]
    pub(crate) load_objects: LoadObjectLifecycle,
    /// Deterministic fog/shroud visibility state.
    pub fog: FogState,
    /// Static alliance graph derived from map house data.
    pub house_alliances: HouseAllianceMap,
    /// Object substrate — the active-object order plus the monotonic id and
    /// enter-order counters. The single owner the lifecycle contract
    /// (reveal/conceal/unlimbo/uninit) mutates; entity storage and the
    /// occupancy grid migrate here in later stages.
    pub(crate) substrate: ObjectSubstrate,
    /// Ordered release-visible handoffs produced by lifecycle transactions.
    /// The app drains these without feeding them back into simulation.
    #[serde(skip)]
    pub(crate) lifecycle_outputs: Vec<LifecycleOutput>,
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
    /// Missiles whose rocket flight reached its target during this tick's
    /// movement pass. Drained at the end of that pass, in live-object order.
    #[serde(skip)]
    pub(crate) pending_rocket_detonations: Vec<u64>,
    /// Missile impacts awaiting the combat phase, which expands each into
    /// ordinary damage events so the shared damage → death → despawn pipeline
    /// resolves them. Filled during the movement pass, drained after combat in
    /// the same tick.
    #[serde(skip)]
    pub(crate) pending_missile_detonations: Vec<crate::sim::spawn_manager::MissileDetonation>,
    /// Aircraft whose Mission_Attack state4 visit, dispatched in their own
    /// LogicVector slot, requested the combat receiver's release this frame.
    /// Filled by the live pass, drained by combat in the same frame.
    #[serde(skip)]
    pub(crate) aircraft_fire_requests: std::collections::BTreeSet<u64>,
    /// BulletClass AI results produced in mixed Logic order and consumed at
    /// the existing combat receiver seam later in this master frame.
    #[serde(skip)]
    pub(crate) pending_projectile_detonations: Vec<crate::sim::projectile::ProjectileDetonation>,
    /// WaveClass AI damage requests produced in mixed Logic order and consumed
    /// at the established wave-damage receiver seam later in this frame.
    #[serde(skip)]
    pub(crate) pending_wave_damage_requests: Vec<crate::sim::wave::WaveDamageRequest>,
    /// Internal order proof; release builds carry no ledger or recording branch.
    #[cfg(test)]
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
    #[cfg(test)]
    #[serde(skip)]
    master_frame_test_trace: Vec<MasterFrameTestRung>,
    /// Focused ordering observer for the bounded House-update activation seam.
    #[cfg(test)]
    #[serde(skip)]
    house_ai_activation_order_test_trace: Vec<HouseAiActivationOrderTestEvent>,
    /// One-shot fixture hook proving that the House loop reloads live length.
    #[cfg(test)]
    #[serde(skip)]
    house_update_append_after_test: Option<(InternedId, InternedId)>,
    /// Sound events produced during the current tick and moved into the owned
    /// app-frame output batch.
    #[serde(skip)]
    pub sound_events: Vec<SimSoundEvent>,
    #[serde(skip)]
    pub(crate) lighting_sources: crate::sim::light_sources::LightingSources,
    /// Fire events produced during combat and moved into the app-frame output
    /// for muzzle flash rendering and future projectile origin computation.
    #[serde(skip)]
    pub(crate) fire_events: Vec<SimFireEvent>,
    /// Native IC/ForceShield impact combat-light requests emitted in receiver
    /// order during the current master frame. The native object has no owner
    /// and is distinct from AnimClass/ParticleSystemClass, so this remains a
    /// dedicated presentation handoff rather than fabricated world state.
    #[serde(skip)]
    pub(crate) invulnerability_impact_effects: Vec<crate::sim::combat::InvulnerabilityImpactEffect>,
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
    /// Per-AI-owner state for computer-controlled players.
    pub ai_players: Vec<AiPlayerState>,
    /// Resolved TeamClass/ScriptType runtime; scenario INI parsing remains a
    /// separate refused boundary until its record grammar is evidenced.
    pub(crate) team_script_vm: TeamScriptVm,
    /// Per-player state keyed by uppercase owner name. Deterministic iteration
    /// via BTreeMap. Equivalent to the original engine's HouseClass array.
    pub houses: BTreeMap<InternedId, HouseState>,
    /// Per-SpeedType terrain cost grids for cost-aware A* pathfinding.
    /// Built once at map load — units look up their SpeedType to pick the right grid.
    #[serde(skip)]
    pub terrain_costs: BTreeMap<SpeedType, TerrainCostGrid>,
    /// Derived movement inputs reused across object turns (blocker plane).
    /// Rebuilt from state on demand; never serialized or hashed.
    #[serde(skip)]
    pub(crate) movement_pass_cache: crate::sim::movement::movement_tick::MovementPassCache,
    /// Zone-based connectivity map for instant unreachability detection.
    /// Built from terrain data; rebuilt when buildings or bridges change.
    #[serde(skip)]
    pub(crate) zone_grid: Option<ZoneGrid>,
    /// Canonical dynamic navigation projection. Arc snapshots let one master
    /// frame pin its entry view while the sim publishes the next projection.
    #[serde(skip)]
    pub(crate) path_grid: Option<Arc<PathGrid>>,
    #[serde(skip)]
    pub resolved_terrain: Option<ResolvedTerrainGrid>,
    /// Process-global MapClass fallback CellClass identity. Native owns this at
    /// `0x00ABDC50`, outside Scenario serialization; live in-scenario loads
    /// retain the current handle and rebuilt terrain is rebound to it.
    #[serde(skip, default)]
    pub(crate) shared_cell_dummy: SharedCellDummy,
    /// Serialized value authority for allocated real CellClass bridge bits.
    /// The derived terrain grid is skipped; load installs its pristine map
    /// template and then writes these saved values directly back onto real
    /// cells. This is value state, never a setter/replay log.
    #[serde(default)]
    pub(crate) real_cell_bridge_flags_0x1180: RealCellBridgeFlags0x1180,
    /// Exact value projection of runtime isometric-tile replacement. The
    /// parsed terrain grid is derived/skipped and receives these values before
    /// overlay/bridge/navigation reconstruction on restore.
    #[serde(default)]
    pub(crate) dynamic_terrain_cells:
        BTreeMap<(u16, u16), crate::map::resolved_terrain::DynamicTerrainCellState>,
    pub bridge_state: Option<BridgeRuntimeState>,
    /// Per-cell mutable overlay state (ore density, wall damage, bridge frames).
    /// Seeded from map [OverlayPack] at init, mutated during gameplay.
    pub overlay_grid: Option<crate::sim::overlay_grid::OverlayGrid>,
    /// Persistent MapClass scenario-crate slots, including accepted ghosts and
    /// their native timer words. This is authoritative save/hash state.
    pub(crate) crate_authority: crate::sim::crates::CrateAuthority,
    /// The scenario's ordinary `[Lighting]` profile, retained so the runtime
    /// crate-regeneration rung reaches `OverlayClass::Mark` with the same
    /// CellClass `+0x10A` source scenario start used. Static map data, not
    /// simulation state: NOT serialized, NOT hashed, and carried across an
    /// in-scenario load beside the other process-scoped values.
    #[serde(skip, default = "default_scenario_normal_lighting")]
    pub(crate) scenario_normal_lighting: crate::map::lighting::LightingProfileUnits,
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
    pub(crate) playfield_size_height: Option<i32>,
    /// Monotonic visible-area writer generation. Every successful trigger
    /// action 0x28 advances it, even when two writers normalize to the same
    /// bounds. Presentation consumes this as the global radar/scroll rebuild
    /// edge; cell-local bridge dirtiness is deliberately a separate channel.
    #[serde(default)]
    pub(crate) playfield_revision: u64,
    /// SHP interned IDs for bridge destruction explosions (from rules.ini BridgeExplosions=).
    #[serde(skip)]
    pub bridge_explosions: Vec<InternedId>,
    /// SHP interned IDs for bridge metallic-debris animations
    /// (from `[General] MetallicDebris=`). Pre-interned at sim init so the
    /// per-cell debris cascade in `bridge_orchestrator::spawn_bridge_debris`
    /// runs allocation-free.
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
    /// Runtime cell rects dirtied by tiberium mutation side effects.
    #[serde(skip)]
    pub(crate) tactical_dirty_cells: Vec<(u16, u16)>,
    /// Per-player power state (output, drain, low-power flag, spy blackout timer).
    /// Updated each tick by `power_system::tick_power_states()`.
    pub power_states: BTreeMap<InternedId, PowerState>,
    /// Per-owner superweapon instances. Outer key = owner, inner key = SW type ID.
    /// Deterministic iteration via nested BTreeMap.
    pub(crate) super_weapons:
        BTreeMap<InternedId, BTreeMap<InternedId, crate::sim::superweapon::SuperWeaponInstance>>,
    /// Active lightning storm state (global — only one at a time).
    pub(crate) lightning_storm:
        Option<crate::sim::superweapon::lightning_storm::LightningStormState>,
    /// Whether superweapon grants have been initialized from map-placed buildings.
    pub(crate) super_weapons_initialized: bool,
    /// Per-cell terrain speed modifier config (slope climb/descend).
    /// Built from [General] rules at map load.
    #[serde(skip)]
    pub terrain_speed_config: terrain_speed::TerrainSpeedConfig,
    /// Distance in leptons below which a blocked unit stops instead of repathing.
    /// From CloseEnough= in [General]. Default 576 (~2.25 cells).
    pub close_enough: SimFixed,
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
    /// Pending gameplay commands waiting for their scheduled execution tick.
    /// Admitted through `queue_command(s)` and drained each tick when
    /// `cmd.execute_tick <= current_tick + 1`.
    pending_commands: Vec<CommandEnvelope>,
    /// Serialized map trigger state, initialized and mutated by trigger_runtime.
    /// Remains installed while actions call other Simulation mechanisms.
    pub(crate) trigger_runtime: TriggerRuntime,
}

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

#[allow(clippy::too_many_arguments)]
fn dispatch_tiberium_reduction_inline(
    request: &crate::sim::combat::TiberiumReductionRequest,
    rules: &RuleSet,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
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
    terrain_costs: &'a mut BTreeMap<SpeedType, TerrainCostGrid>,
    zone_grid: &'a mut Option<ZoneGrid>,
    path_grid: &'a mut Option<Arc<PathGrid>>,
    bridge_state: Option<&'a BridgeRuntimeState>,
    playfield_bounds: Option<PlayfieldBounds>,
}

impl crate::sim::combat::combat_aoe::AoECellPrelude for SimulationAreaDamageCellPrelude<'_> {
    #[allow(clippy::too_many_arguments)]
    fn before_cell(
        &mut self,
        rx: u16,
        ry: u16,
        overlay_grid: Option<&mut crate::sim::overlay_grid::OverlayGrid>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
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
        repair: WallZoneRepairKind,
    ) {
        repair_wall_damage_navigation_authorities(
            self.terrain_costs,
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

#[allow(clippy::too_many_arguments)]
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
    terrain_costs: &'a mut BTreeMap<SpeedType, TerrainCostGrid>,
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
        terrain_costs,
        zone_grid,
        path_grid,
        bridge_state,
        playfield_bounds,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn repair_wall_damage_navigation_authorities(
    terrain_costs: &mut BTreeMap<SpeedType, TerrainCostGrid>,
    zone_grid: &mut Option<ZoneGrid>,
    path_grid: &mut Option<Arc<PathGrid>>,
    terrain: &ResolvedTerrainGrid,
    bridge_state: Option<&BridgeRuntimeState>,
    playfield_bounds: Option<PlayfieldBounds>,
    cell: (u16, u16),
    navigation_changed: bool,
    repair: WallZoneRepairKind,
) {
    let resolved_path_grid = PathGrid::from_resolved_terrain_with_bridges(terrain, bridge_state);
    let mut tail_path_grid = path_grid
        .as_deref()
        .filter(|grid| {
            grid.width() == resolved_path_grid.width()
                && grid.height() == resolved_path_grid.height()
        })
        .cloned()
        .unwrap_or_else(|| resolved_path_grid.clone());
    if navigation_changed {
        let replaced = tail_path_grid.replace_cell_from(&resolved_path_grid, cell.0, cell.1);
        debug_assert!(replaced, "wall Recalc cell must be inside the map");
    }

    *terrain_costs = build_canonical_terrain_cost_grids(terrain);
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
        let bridge_records = bridge_state
            .map(BridgeRuntimeState::endpoint_records)
            .unwrap_or(&[]);
        let repair = match repair {
            WallZoneRepairKind::AssignOrphaned => ZoneRepairKind::AssignOrphaned,
            WallZoneRepairKind::MergeAdjacent => ZoneRepairKind::MergeAdjacent,
        };
        let _ = repair_zone_cell(
            zone_grid,
            PackedZoneCoord::new(cell.0 as i16, cell.1 as i16),
            repair,
            &tail_path_grid,
            playfield_bounds,
            terrain,
            bridge_records,
        );
    } else {
        *zone_grid = Some(ZoneGrid::build_with_native_map_context(
            &tail_path_grid,
            terrain_costs,
            terrain,
            bridge_state
                .map(BridgeRuntimeState::endpoint_records)
                .unwrap_or(&[]),
            bridge_geometry,
            playfield_bounds,
        ));
    }
    *path_grid = Some(Arc::new(tail_path_grid));
}

/// Borrow-split wall observer used by runtime placement and sale, where the
/// overlay/terrain transaction remains in its owning module but the live
/// presentation, navigation, and represented Cell-target authorities are
/// already available as disjoint Simulation fields.
pub(crate) struct SimulationWallRuntimeHost<'a> {
    pub(crate) entities: &'a mut EntityStore,
    #[cfg(test)]
    pub(crate) detach_trace: &'a mut Vec<crate::sim::combat::combat_aoe::CellTargetDetach>,
    pub(crate) radar_dirty_cells: &'a mut Vec<(u16, u16)>,
    pub(crate) radar_dirty_generation: &'a mut u64,
    pub(crate) tactical_dirty_cells: &'a mut Vec<(u16, u16)>,
    pub(crate) terrain_costs: &'a mut BTreeMap<SpeedType, TerrainCostGrid>,
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
        repair: WallZoneRepairKind,
    ) {
        repair_wall_damage_navigation_authorities(
            self.terrain_costs,
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
                rx,
                ry,
                #[cfg(test)]
                self.detach_trace,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn dispatch_smudge_inline(
    request: &crate::sim::combat::SmudgeSpawnRequest,
    rules: &RuleSet,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    interner: &StringInterner,
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
    crate::sim::combat::smudge_dispatch::drain_smudge_spawn_requests(
        std::slice::from_ref(request),
        &rules.art_registry,
        &rules.smudge_types,
        interner,
        smudge_grid,
        occupancy,
        terrain,
        raw_occupation,
        &mut tiberium,
        scenario_rng,
    );
}

impl Simulation {
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

    pub(crate) fn apply_fatal_lifecycle_stage(
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
            crate::sim::combat::FatalLifecycleStage::PostMortemExactZero { killer_owner } => {
                self.postmortem_exact_zero_callbacks(
                    stable_id,
                    killer_owner,
                    rules,
                    uninit_context,
                );
            }
            crate::sim::combat::FatalLifecycleStage::BeforeDeathEffects => {
                // RESIDUAL: this runs before the Techno death arm (sounds,
                // debris, death weapon). Natively that arm returns first, and
                // the Building NowDead block then ejects the occupants
                // (`0x00442625`) and turns the light off (`0x0044264C`) just
                // before DestructionEffects (`0x00442665`), so an ejected
                // garrison's Scatter draws follow the debris and death-weapon
                // draws there and precede them here.
                if !matches!(category, EntityCategory::Unit | EntityCategory::Structure) {
                    return;
                }
                let garrison = self.substrate.entities.get(stable_id).and_then(|entity| {
                    if category != EntityCategory::Structure {
                        return None;
                    }
                    let object = rules.object(self.interner.resolve(entity.type_ref()))?;
                    let passenger_ids = entity
                        .passenger_role
                        .cargo()
                        .map(|cargo| cargo.passengers.clone())
                        .unwrap_or_default();
                    if !object.can_be_occupied || passenger_ids.is_empty() {
                        return None;
                    }
                    let (foundation_w, foundation_h) =
                        crate::sim::production::foundation_dimensions(&object.foundation);
                    Some(crate::sim::combat::DestroyedGarrisonBuilding {
                        building_id: stable_id,
                        type_id: entity.type_ref(),
                        owner: entity.owner(),
                        rx: entity.position.rx,
                        ry: entity.position.ry,
                        z: entity.position.z,
                        foundation_w,
                        foundation_h,
                        passenger_ids,
                    })
                });
                // An absorbing building's passengers leave through
                // SpawnSurvivors' Phase A; its KillPassengers (`0x00441F27`)
                // runs after that and finds the list empty.
                let absorbs = category == EntityCategory::Structure
                    && self
                        .substrate
                        .entities
                        .get(stable_id)
                        .is_some_and(|entity| {
                            rules
                                .object(self.interner.resolve(entity.type_ref()))
                                .is_some_and(|object| object.infantry_absorb || object.unit_absorb)
                        });
                if let Some(event) = garrison {
                    production::eject_destruction_garrison_with_context(
                        self,
                        rules,
                        &event,
                        uninit_context,
                    );
                } else if !absorbs {
                    self.purge_carried_passengers_for_fatal(stable_id, uninit_context);
                }
                if category == EntityCategory::Structure {
                    //44264C precedes the recursive destruction effects.
                    self.set_building_light_active(stable_id, false);
                }
            }
            crate::sim::combat::FatalLifecycleStage::AfterDeathEffects => {
                if !matches!(category, EntityCategory::Unit | EntityCategory::Structure) {
                    return;
                }
                if category == EntityCategory::Structure
                    && self
                        .substrate
                        .entities
                        .get(stable_id)
                        .and_then(|building| building.bunker_occupant)
                        .is_some()
                {
                    crate::sim::docking::bunker_link::release_sell_destroy(self, stable_id);
                }
                self.release_move_sound(stable_id);
                self.uninit_with_context(stable_id, uninit_context);
            }
        }
    }

    fn tick_combat_with_fatal_lifecycle(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        tick_ms: u32,
        logic_order: &[u64],
        fire_suppressed: &BTreeSet<u64>,
        aircraft_fire_requests: &BTreeSet<u64>,
        projectile_detonations: &[crate::sim::projectile::ProjectileDetonation],
        wave_damage_events: &[crate::sim::wave::WaveDamageEvent],
    ) -> crate::sim::combat::CombatTickResult {
        let mut run = crate::sim::combat::world_receiver::ReceiverRun::default();
        let mut result = crate::sim::combat::world_receiver::tick_combat(
            self,
            &mut run,
            rules,
            overlay_registry,
            tick_ms,
            logic_order,
            fire_suppressed,
            aircraft_fire_requests,
            projectile_detonations,
            wave_damage_events,
        );
        result.consequences.finish_navigation(run.finish(self));
        result
    }

    /// Commit a completed Bullet's detonation while its current Logic slot and
    /// physical object are still present. The caller retires the Bullet only
    /// after this transaction returns.
    fn commit_logic_projectile_detonations(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        detonations: &[crate::sim::projectile::ProjectileDetonation],
    ) {
        if detonations.is_empty() {
            return;
        }
        let mut run = crate::sim::combat::world_receiver::ReceiverRun::default();
        let commit = crate::sim::combat::world_receiver::commit_projectiles(
            self,
            &mut run,
            detonations,
            rules,
            overlay_registry,
        );
        let terrain_navigation_changed_cells = run.finish(self);

        for projectile in commit.projectile_spawns {
            let stable_id = self.allocate_stable_id();
            self.admit_projectile(stable_id, projectile);
        }
        self.absorb_noncombat_damage_effects(
            rules,
            overlay_registry,
            commit.effects,
            commit.under_attack_events,
            terrain_navigation_changed_cells,
        );
    }

    pub(crate) fn commit_fired_wave(&mut self, rules: &RuleSet, event: &SimFireEvent) {
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::CombatFireEffectsCommitted {
            attacker_id: event.attacker_id,
            scenario_rng_state: self.scenario_rng.state(),
        });
        if let Some(wave) = self.prepare_fired_wave(
            rules,
            event,
            &self.substrate.entities,
            &self.interner,
            self.resolved_terrain.as_ref(),
        ) {
            self.admit_fired_wave(event.attacker_id, wave, None);
        }
    }

    /// Read the firing transaction directly; construction never installs a second
    /// entity, map, house or RNG authority into the world.
    fn prepare_fired_wave(
        &self,
        rules: &RuleSet,
        event: &SimFireEvent,
        entities: &EntityStore,
        interner: &StringInterner,
        terrain: Option<&ResolvedTerrainGrid>,
    ) -> Option<crate::sim::wave::Wave> {
        let Some(weapon) = rules.weapon(interner.resolve(event.weapon_id)) else {
            return None;
        };
        let wave_type = if weapon.is_sonic {
            0
        } else if weapon.is_mag_beam {
            3
        } else {
            return None;
        };
        let target = match event.target {
            crate::sim::combat::TargetKind::Entity(id) => {
                let Some(entity) = entities.get(id) else {
                    return None;
                };
                ProjectileCoord::new(
                    i32::from(entity.position.rx) * 256 + entity.position.sub_x.to_num::<i32>(),
                    i32::from(entity.position.ry) * 256 + entity.position.sub_y.to_num::<i32>(),
                    crate::sim::combat::object_world_z_leptons(entity, terrain),
                )
            }
            crate::sim::combat::TargetKind::Cell(rx, ry) => {
                self.wave_cell_target_position_in(terrain, rx, ry)
            }
        };
        let source = entities
            .get(event.attacker_id)
            .map(|entity| {
                ProjectileCoord::new(
                    i32::from(entity.position.rx) * 256 + entity.position.sub_x.to_num::<i32>(),
                    i32::from(entity.position.ry) * 256 + entity.position.sub_y.to_num::<i32>(),
                    crate::sim::combat::object_world_z_leptons(entity, terrain),
                )
            })
            .unwrap_or_else(|| {
                ProjectileCoord::new(
                    i32::from(event.origin_snapshot.rx) * 256
                        + event.origin_snapshot.sub_x.to_num::<i32>(),
                    i32::from(event.origin_snapshot.ry) * 256
                        + event.origin_snapshot.sub_y.to_num::<i32>(),
                    i32::from(event.origin_snapshot.z)
                        * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS,
                )
            });
        let wave = crate::sim::wave::Wave::new_owned(
            wave_type,
            event.attacker_id,
            event.target,
            source,
            target,
        );
        Some(wave)
    }

    /// Commit identity, constructor cleanup and immediate Logic admission in their
    /// original order. The receiver keeps ownership of all combat/map state.
    fn admit_fired_wave(
        &mut self,
        attacker_id: u64,
        mut wave: crate::sim::wave::Wave,
        terrain: Option<&ResolvedTerrainGrid>,
    ) {
        let stable_id = self.allocate_stable_id();
        if !wave.constructor_distance_is_live() {
            // Constructor UnInit precedes registration and unique identity,
            // but FireAt stores the returned dead pointer. The stable id only
            // represents that same-frame deferred cleanup window.
            self.waves.spawn(stable_id, wave);
            self.active_wave_links.insert(attacker_id, stable_id);
            let retired = self.retire_non_entity_object(stable_id);
            debug_assert!(retired);
            return;
        }
        let context = crate::sim::wave::WaveUpdateContext {
            owner_position: Some(wave.source),
            owner_current_target: wave.target_ref,
            target_position: Some(wave.target),
        };
        let _terminal = wave.initialize(context, terrain.or(self.resolved_terrain.as_ref()));
        self.admit_wave(stable_id, wave);
        self.active_wave_links.insert(attacker_id, stable_id);
    }

    /// Finish the live Logic pass after combat has modeled the pre-existing
    /// Techno callbacks. FireAt registers each Wave immediately, preserving
    /// its actual tail position; the live-length walk reaches those new slots
    /// only after every object that was already ahead of them has fired.
    fn visit_combat_appended_wave_tail(
        &mut self,
        preexisting_wave_ids: &BTreeSet<u64>,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) {
        let mut index = 0;
        while index < self.substrate.logic.len() {
            let stable_id = self.substrate.logic.as_slice()[index];
            if !preexisting_wave_ids.contains(&stable_id) && self.waves.get(stable_id).is_some() {
                let _ = self.object_ai_visit_one(
                    stable_id,
                    Some(rules),
                    techno_ai::ObjectAiCtx {
                        overlay_registry,
                        ..techno_ai::ObjectAiCtx::default()
                    },
                );
            }
            index += 1;
        }
    }

    /// Walk one Wave damage request in recorded-cell and current Cell-list
    /// order. Each receiver commits before the next occupant is selected, so
    /// fatal UnInit and nested effects are visible to the remaining walk.
    fn commit_logic_wave_damage_request(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        request: &crate::sim::wave::WaveDamageRequest,
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

        let mut run = crate::sim::combat::world_receiver::ReceiverRun::default();
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
                let Some(object_type) = rules.object(self.interner.resolve(firer.type_ref()))
                else {
                    break;
                };
                let Some(weapon_name) = crate::sim::combat::combat_weapon::primary_for_tier(
                    object_type,
                    firer.veterancy,
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
                                                && !entity.dying
                                                && !entity.lifecycle.in_limbo
                                                && entity.lifecycle.object_alive
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
                            let post_object_damage = crate::sim::combat::wave_post_object_damage(
                                &event,
                                &self.substrate.entities,
                                rules,
                                &self.interner,
                                &self.houses,
                                &house_alliances,
                                scenario_no_damage,
                                current_tick,
                                self.resolved_terrain.as_ref(),
                            );
                            if let Some(value) = post_object_damage {
                                shared_damage = value;
                            }
                            #[cfg(test)]
                            self.trace_lifecycle_for_test(
                                LifecycleTestEvent::WaveDamageReceiverSelected {
                                    wave_id: request.wave_id,
                                    target_id: target_id,
                                    scenario_rng_state: self.scenario_rng.state(),
                                },
                            );
                            crate::sim::combat::combat_aoe::AreaDamageReceiver::Entity(event)
                        }
                        CellObject::Terrain(stable_id) => {
                            #[cfg(test)]
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
                                #[cfg(test)]
                                detach_trace: &mut effects.cell_target_detaches,
                                radar_dirty_cells: &mut self.radar_terrain_dirty_cells,
                                radar_dirty_generation: &mut self.radar_terrain_dirty_generation,
                                tactical_dirty_cells: &mut self.tactical_dirty_cells,
                                terrain_costs: &mut self.terrain_costs,
                                zone_grid: &mut self.zone_grid,
                                path_grid: &mut self.path_grid,
                                bridge_state: self.bridge_state.as_ref(),
                                playfield_bounds: self.playfield_bounds,
                            };
                            crate::sim::overlay_grid::damage_wall_overlay_with_runtime_host(
                                grid,
                                registry,
                                self.resolved_terrain.as_mut(),
                                rx,
                                ry,
                                raw_ambient_damage,
                                &mut self.scenario_rng,
                                Some(&mut host),
                            )
                        };
                        #[cfg(test)]
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
                                coord.0,
                                coord.1,
                                #[cfg(test)]
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

        let terrain_navigation_changed_cells = run.finish(self);
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
        );
    }

    /// Complete one direct object ReceiveDamage call before its caller resumes.
    /// Unlike Apply_area_damage, a direct call has no collected area or area-wide
    /// Iron Curtain isolation decision. Nested deaths, world mutations and their
    /// publication finish before a live cell-list caller reads its successor.
    pub(crate) fn commit_direct_damage_receiver(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        event: crate::sim::combat::EntityDamageEvent,
    ) {
        let mut run = crate::sim::combat::world_receiver::ReceiverRun::default();
        let (effects, under_attack_events) = crate::sim::combat::world_receiver::commit_entities(
            self,
            &mut run,
            std::slice::from_ref(&event),
            Some(false),
            rules,
            overlay_registry,
        );
        let terrain_navigation_changed_cells = run.finish(self);
        self.absorb_noncombat_damage_effects(
            rules,
            overlay_registry,
            effects,
            under_attack_events,
            terrain_navigation_changed_cells,
        );
    }

    /// Terrain counterpart of a direct object receiver. Native bridge fallout
    /// forces Object damage, while Terrain's Wood/Immune gate still applies.
    pub(crate) fn commit_direct_terrain_damage_receiver(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        event: crate::sim::combat::TerrainDamageEvent,
    ) {
        let mut run = crate::sim::combat::world_receiver::ReceiverRun::default();
        let (effects, under_attack_events) = crate::sim::combat::world_receiver::commit_terrain(
            self,
            &mut run,
            event,
            true,
            rules,
            overlay_registry,
        );
        let terrain_navigation_changed_cells = run.finish(self);
        self.absorb_noncombat_damage_effects(
            rules,
            overlay_registry,
            effects,
            under_attack_events,
            terrain_navigation_changed_cells,
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
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        hits: &[crate::sim::combat::EntityDamageEvent],
    ) {
        let receivers = hits
            .iter()
            .copied()
            .map(crate::sim::combat::combat_aoe::AreaDamageReceiver::Entity)
            .collect::<Vec<_>>();
        self.commit_noncombat_aoe_receivers(rules, overlay_registry, &receivers);
    }

    /// World-owned entry for a complete non-combat Apply_area_damage receiver
    /// vector, including TerrainClass records in captured object-list order.
    pub(crate) fn commit_noncombat_aoe_receivers(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        receivers: &[crate::sim::combat::combat_aoe::AreaDamageReceiver],
    ) -> Vec<u64> {
        let mut run = crate::sim::combat::world_receiver::ReceiverRun::default();
        let (effects, under_attack_events) = crate::sim::combat::world_receiver::commit_area(
            self,
            &mut run,
            receivers,
            rules,
            overlay_registry,
        );
        let terrain_navigation_changed_cells = run.finish(self);
        // Fatal transitions are facts of this receiver transaction. Retain
        // them before consequence delivery can retire their objects.
        let fatal_ids = effects.despawned_ids.clone();

        self.absorb_noncombat_damage_effects(
            rules,
            overlay_registry,
            effects,
            under_attack_events,
            terrain_navigation_changed_cells,
        );
        fatal_ids
    }

    /// World-owned half of a non-combat damage transaction. Physical death
    /// consequences already happened recursively in combat; this consumes the
    /// same lifecycle/presentation/terrain outputs without leaving an alternate
    /// zero-HP object registered for the next LogicClass visit.
    fn absorb_noncombat_damage_effects(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        effects: crate::sim::combat::DeathEffects,
        under_attack_events: Vec<crate::sim::combat::UnderAttackEvent>,
        terrain_navigation_changed_cells: Vec<(u16, u16)>,
    ) {
        let _ = damage_consequences::DamageConsequences::immediate(
            effects,
            under_attack_events,
            terrain_navigation_changed_cells,
        )
        .commit(self, rules, overlay_registry, None);
    }

    /// `HouseClass::NotifyUnderAttack @ 0x004F93E0` for one damaged asset,
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
    ) {
        let game_mode_nonzero = self.session.game_mode_nonzero;
        for event in events {
            let event_type = if event.miner {
                RadarEventType::HarvesterUnderAttack
            } else {
                RadarEventType::BaseUnderAttack
            };
            self.sound_events.push(SimSoundEvent::UnderAttack {
                rx: event.rx,
                ry: event.ry,
                owner: event.owner,
                miner: event.miner,
                radar: RadarEventRequest::new(event_type, event.rx, event.ry),
            });
            if !event.structure {
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

    /// `TechnoClass::Death_Announcement @ 0x004D98C0` for this tick's damage
    /// kills: owner passes `HouseClass::IsHumanPlayer @ 0x0050B6F0`
    /// (`0x004D98CA`; the local player natively, every human house here with
    /// the app filtering), then `CreateRadarEvent(7, cell)` (`0x004D98FE`,
    /// 8-cell dedupe, admitted client-side) gates `EVA_UnitLost`
    /// (`0x004D9911`).
    pub(crate) fn dispatch_unit_lost_events(
        &mut self,
        events: &[crate::sim::combat::UnitLostEvent],
    ) {
        let game_mode_nonzero = self.session.game_mode_nonzero;
        for event in events {
            let human = self
                .houses
                .get(&event.owner)
                .is_some_and(|house| house.is_controlled_by_human(game_mode_nonzero));
            if !human {
                continue;
            }
            self.sound_events.push(SimSoundEvent::UnitLost {
                owner: event.owner,
                radar: RadarEventRequest::new(RadarEventType::UnitLost, event.rx, event.ry),
            });
        }
    }

    /// Borrow all three logical RNG objects without exposing mutation.
    pub fn rng_views(&self) -> SimulationRngViews<'_> {
        SimulationRngViews {
            scenario: self.scenario_rng.logical_view(),
            main: self.main_rng.logical_view(),
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

    /// Complete the in-scenario Load Game handoff with process-global state
    /// that is live at load time. Scenario RNG has already been reset by
    /// deserialization and is deliberately untouched here. The cold front-end
    /// load route has a distinct native handoff and does not use this seam.
    pub(crate) fn retain_in_scenario_process_state_from(&mut self, live: &Self) {
        self.session.seed = live.session.seed;
        self.main_rng = live.main_rng.clone();
        self.mapgen_rng = live.mapgen_rng.clone();
        self.bind_shared_cell_dummy(live.effective_shared_cell_dummy());
        self.substrate
            .raw_cell_occupation
            .retain_process_dummy_from(&live.substrate.raw_cell_occupation);
        // Static map data, not saved state: the live scenario is still the one
        // being reloaded, so its parsed lighting profile carries over.
        self.scenario_normal_lighting = live.scenario_normal_lighting;
    }

    /// Apply the successful load's native MapClass Resize reconstruction to
    /// every modeled field of the fixed fallback CellClass.
    ///
    /// `MouseClass::Load` dispatch at `0x005BE150` routes restored dimensions through
    /// `MapClass::Resize @ 0x00565C10`, whose unconditional call to
    /// `CellClass::Constructor @ 0x0047BBF0` reconstructs the fixed dummy in
    /// place. The app resets a detached candidate before fallible restoration,
    /// so a rejected transactional load cannot mutate the running world.
    pub(crate) fn reconstruct_cellclass_dummy_for_map_resize(&mut self) {
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
    pub(crate) fn bind_shared_cell_dummy(&mut self, shared_cell_dummy: SharedCellDummy) {
        self.shared_cell_dummy = shared_cell_dummy.clone();
        if let Some(terrain) = self.resolved_terrain.as_mut() {
            terrain.bind_shared_cell_dummy(shared_cell_dummy);
        }
    }

    /// Install a newly constructed map grid and capture the real CellClass
    /// `0x1180` value authority after row-major OverlayPack marking.
    pub(crate) fn install_resolved_terrain_for_new_map(
        &mut self,
        resolved_terrain: ResolvedTerrainGrid,
    ) {
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
    #[cfg(test)]
    pub(crate) fn apply_runtime_bridge_flag_stamp(
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
    pub(crate) fn apply_runtime_bridge_mark_stamp(
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

    /// Commit the allocated real-cell half of a runtime setter that already
    /// executed synchronously through `CellClassBridgeFlagState`. Dummy
    /// coordinate/flag effects are live at the native call point and must not
    /// be replayed here.
    pub(crate) fn apply_planned_bridge_flag_stamp_to_real_cells(
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
            .expect("terrain presence checked before planned bridge setter projection")
            .apply_planned_bridge_flag_stamp_to_real_cells(stamp);
        for (index, flags) in updates {
            self.real_cell_bridge_flags_0x1180
                .set_allocated_cell(index, flags);
        }
    }

    /// Synthetic fixtures may assign a detached grid directly. Production
    /// construction binds both owners, but gameplay must still read the live
    /// handle attached to the actual CellClass table it queried.
    pub(crate) fn effective_shared_cell_dummy(&self) -> SharedCellDummy {
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
        self.main_rng = main_rng;
    }

    /// Install the exact cursor left by the accepted random-map generation.
    /// VERA fixed-map construction currently keeps `Random__Seed(0)`; native
    /// fresh-process state is verified, while cross-match retention is UNCHECKED.
    pub(super) fn install_generated_mapgen_rng(&mut self, mapgen_rng: SimRng) {
        self.mapgen_rng = mapgen_rng;
    }

    /// Create a new empty simulation with the default deterministic seed.
    pub fn new() -> Self {
        Self::with_seed(DEFAULT_SIM_SEED)
    }

    /// Build the interned-id -> type-handle table and the pre-resolved rule
    /// handles beside it. Call once at sim init AFTER `intern_rule_type_ids`,
    /// and again after load once the RuleSet is available. Idempotent: the
    /// warhead names are already interned on every re-resolution path, so
    /// serialized interner state cannot grow or reorder.
    pub fn resolve_type_handles(&mut self, rules: &RuleSet) {
        self.type_handles =
            crate::sim::type_handle_table::TypeHandleTable::build(rules, &self.interner);
        self.rule_handles = Some(crate::sim::type_handle_table::ResolvedRuleHandles::resolve(
            rules,
            &mut self.interner,
        ));
    }

    /// Intern every rules type id (infantry, vehicle, aircraft, building) so
    /// `interner.get(type_id)` succeeds for any type this ruleset references.
    /// Moved from `RuleSet::intern_all_ids` (F04): interning is sim-side work
    /// over rules-owned canonical names.
    pub fn intern_rule_type_ids(&mut self, rules: &RuleSet) {
        for id in rules
            .infantry_ids
            .iter()
            .chain(&rules.vehicle_ids)
            .chain(&rules.aircraft_ids)
            .chain(&rules.building_ids)
        {
            self.interner.intern(id);
        }
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

    /// Resolve an entity's type to its `ObjectType` in one precomputed hop
    /// (two array indexes, no string allocation). Falls back to the name path
    /// when the table is unbuilt (test setups that skip `resolve_type_handles`),
    /// so no caller observes a stale empty table.
    #[inline]
    pub fn object_type<'r>(
        &self,
        type_ref: InternedId,
        rules: &'r RuleSet,
    ) -> Option<&'r ObjectType> {
        self.type_handles.object(&self.interner, type_ref, rules)
    }

    /// Create a new empty simulation with an explicit deterministic seed.
    /// Test/dev entry — u64 seeds wider than 32 bits keep their full value in
    /// `session.seed` (pinned harness baselines depend on it) even though the
    /// stream seeder consumes only 32 bits.
    pub fn with_seed(seed: u64) -> Self {
        let mut session = ScenarioSession::from_descriptor(
            &crate::sim::scenario_session::ScenarioDescriptor::default(),
        );
        session.seed = seed;
        Self::construct(session)
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
            interner: crate::sim::intern::StringInterner::new(),
            type_handles: crate::sim::type_handle_table::TypeHandleTable::default(),
            rule_handles: None,
            production: ProductionState::default(),
            session,
            scenario_rng: SimRng::new(seed),
            main_rng: SimRng::new(seed),
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
            mission_spawned_entities: false,
            frame_overlay_updates: Vec::new(),
            frame_overlay_removals: Vec::new(),
            terminal_score_snapshot: None,
            pending_lifecycle_requests: Vec::new(),
            pending_rocket_detonations: Vec::new(),
            pending_missile_detonations: Vec::new(),
            aircraft_fire_requests: Default::default(),
            pending_projectile_detonations: Vec::new(),
            pending_wave_damage_requests: Vec::new(),
            #[cfg(test)]
            lifecycle_test_events: Vec::new(),
            #[cfg(test)]
            receiver_fixture: None,
            trigger_effects: Vec::new(),
            #[cfg(test)]
            master_frame_test_trace: Vec::new(),
            #[cfg(test)]
            house_ai_activation_order_test_trace: Vec::new(),
            #[cfg(test)]
            house_update_append_after_test: None,
            sound_events: Vec::new(),
            lighting_sources: crate::sim::light_sources::LightingSources::default(),
            fire_events: Vec::new(),
            invulnerability_impact_effects: Vec::new(),
            projectiles: crate::sim::projectile::ProjectileStore::new(),
            waves: crate::sim::wave::WaveStore::new(),
            active_wave_links: BTreeMap::new(),
            pending_smudge_requests: Vec::new(),
            bale_events: Vec::new(),
            bunker_wall_events: Vec::new(),
            ai_players: Vec::new(),
            team_script_vm: TeamScriptVm::default(),
            houses: BTreeMap::new(),
            terrain_costs: BTreeMap::new(),
            zone_grid: None,
            path_grid: None,
            resolved_terrain: None,
            shared_cell_dummy: SharedCellDummy::fresh(),
            real_cell_bridge_flags_0x1180: RealCellBridgeFlags0x1180::default(),
            dynamic_terrain_cells: BTreeMap::new(),
            bridge_state: None,
            overlay_grid: None,
            crate_authority: crate::sim::crates::CrateAuthority::default(),
            scenario_normal_lighting: default_scenario_normal_lighting(),
            smudge_grid: None,
            radiation: crate::sim::radiation::RadiationState::default(),
            bombs: crate::sim::bomb::BombList::default(),
            playfield_bounds: None,
            playfield_size_height: None,
            playfield_revision: 0,
            bridge_explosions: Vec::new(),
            metallic_debris: Vec::new(),
            radar_terrain_dirty_cells: Vec::new(),
            radar_terrain_dirty_generation: 0,
            tactical_dirty_cells: Vec::new(),
            power_states: BTreeMap::new(),
            super_weapons: BTreeMap::new(),
            lightning_storm: None,
            super_weapons_initialized: false,
            terrain_speed_config: terrain_speed::TerrainSpeedConfig::default(),
            close_enough: SimFixed::from_num(576), // 2.25 cells × 256 lep/cell
            debug_event_logging: false,
            input_delay_ticks: 2,
            quit_requested: false,
            executed_exit_owner: None,
            connection_lost: false,
            pending_commands: Vec::new(),
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
    pub(crate) fn scatter_rng(&mut self) -> &mut SimRng {
        &mut self.scenario_rng
    } // bump displacement, idle/forced scatter, passenger unload exit, sell-eject
    #[allow(dead_code)] // Stream-routing audit anchor; callers currently co-borrow the field.
    pub(crate) fn subcell_rng(&mut self) -> &mut SimRng {
        &mut self.scenario_rng
    } // infantry sub-cell rotation, paradrop sub-cell
    #[allow(dead_code)] // Stream-routing audit anchor; callers currently co-borrow the field.
    pub(crate) fn smudge_rng(&mut self) -> &mut SimRng {
        &mut self.scenario_rng
    } // destruction smudge/survivor/debris, smudge type pick
    #[allow(dead_code)] // Stream-routing audit anchor; callers currently co-borrow the field.
    pub(crate) fn wall_damage_rng(&mut self) -> &mut SimRng {
        &mut self.scenario_rng
    } // overlay/wall damage roll
    pub(crate) fn bridge_rng(&mut self) -> &mut SimRng {
        &mut self.scenario_rng
    } // bridge collapse/debris/explosion
    #[allow(dead_code)] // Stream-routing audit anchor; callers currently co-borrow the field.
    pub(crate) fn ore_rng(&mut self) -> &mut SimRng {
        &mut self.scenario_rng
    } // ore growth/spread queue + direction + variant, TIBTRE
    #[allow(dead_code)] // Stream-routing audit anchor; callers currently co-borrow the field.
    pub(crate) fn anim_rng(&mut self) -> &mut SimRng {
        &mut self.scenario_rng
    } // building damage-fire type/start-frame
    pub(crate) fn particle_rng(&mut self) -> &mut SimRng {
        &mut self.scenario_rng
    } // particle/smoke/gas/fire lifetime/offset/dir/insert
    pub(crate) fn superweapon_rng(&mut self) -> &mut SimRng {
        &mut self.scenario_rng
    } // lightning-storm scatter/bolt
    pub(crate) fn miner_jitter_rng(&mut self) -> &mut SimRng {
        &mut self.scenario_rng
    } // dock-entry retry + unload-deploy frame jitter
    /// Capture the process-continuity Scenario cursor when gameplay returns to
    /// the frontend. The app stores this clone until the next successful Start
    /// reseeds the gameplay pair.
    pub(crate) fn clone_scenario_rng(&self) -> SimRng {
        self.scenario_rng.clone()
    }

    // --- Main/global gameplay stream ---
    #[allow(dead_code)] // Named Main-stream audit anchor retained beside direct borrows.
    pub(crate) fn weapon_spread_rng(&mut self) -> &mut SimRng {
        &mut self.main_rng
    } // verified main-only weapon/warhead property rolls; not detonation scatter
    #[allow(dead_code)] // Named Main-stream audit anchor for the staged House AI consumer.
    pub(crate) fn house_ai_rng(&mut self) -> &mut SimRng {
        &mut self.main_rng
    } // HouseClass superpower/AI gate roll

    /// Test/replay helper for the per-game Scenario/Main pair only.
    ///
    /// Same-process MapGen reset/retention is unverified, so reseeding the
    /// per-game pair must preserve the current MapGen object.
    #[cfg(test)]
    pub(crate) fn reseed_scenario_and_main(&mut self, seed: u64) {
        self.scenario_rng = SimRng::new(seed);
        self.main_rng = SimRng::new(seed);
        self.session.seed = seed;
    }

    /// The occupancy grid (per-cell object lists). Read access for systems above sim/.
    pub fn occupancy(&self) -> &OccupancyGrid {
        &self.substrate.occupancy
    }

    /// Mutable occupancy access for the few above-sim callers that unmark cells.
    #[cfg(test)]
    pub fn occupancy_mut(&mut self) -> &mut OccupancyGrid {
        &mut self.substrate.occupancy
    }

    /// The entity store. Read access for systems above sim/.
    pub fn entities(&self) -> &EntityStore {
        &self.substrate.entities
    }

    /// Logic scheduling order, also consumed by the current radar pipeline.
    pub(crate) fn logic_order(&self) -> &[u64] {
        self.substrate.logic.as_slice()
    }

    /// Retained Display membership and order. Tactical6D8F19..6D95A9 walks
    /// these five vectors directly; it neither queries GetLayer nor re-sorts.
    pub(crate) fn display_layers(&self) -> &display_layers::DisplayLayers {
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

    pub(crate) fn mark_radar_terrain_dirty_cells<I>(&mut self, cells: I)
    where
        I: IntoIterator<Item = (u16, u16)>,
    {
        let mut changed = false;
        for cell in cells {
            if !self.radar_terrain_dirty_cells.contains(&cell) {
                self.radar_terrain_dirty_cells.push(cell);
                changed = true;
            }
        }
        if changed {
            self.radar_terrain_dirty_generation =
                self.radar_terrain_dirty_generation.wrapping_add(1);
        }
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
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        request: crate::sim::combat::SmudgeSpawnRequest,
    ) {
        let binary_frame = self.session.binary_frame;
        let spread_enabled = self.production.ore_growth_config.spreads;
        dispatch_smudge_inline(
            &request,
            rules,
            overlay_registry,
            &self.interner,
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
        );
        self.flush_smudge_dirty();
    }

    pub(crate) fn reduce_tiberium_at_with_native_context<A: Into<i32>>(
        &mut self,
        cell: (u16, u16),
        amount: A,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
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
            // ore growth/spread — scenario stream. Direct field (not ore_rng()): this
            // literal co-borrows other &mut self fields, so the all-self accessor conflicts.
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

    fn movement_sound_probe(&self, stable_id: u64) -> Option<MovementSoundProbe> {
        let entity = self.substrate.entities.get(stable_id)?;
        Some(MovementSoundProbe {
            rx: entity.position.rx,
            ry: entity.position.ry,
            z: entity.position.z,
            sub_x_bits: entity.position.sub_x.to_bits(),
            sub_y_bits: entity.position.sub_y.to_bits(),
            facing: entity.facing,
            path_index: entity
                .movement_target
                .as_ref()
                .map(|target| target.next_index),
            track_point: entity
                .drive_locomotion
                .as_ref()
                .map(|state| &state.track)
                .or_else(|| entity.ship_locomotion.as_ref().map(|state| &state.track))
                .filter(|track| track.turn_index >= 0)
                .and_then(|track| u16::try_from(track.cursor).ok()),
        })
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
    pub(crate) fn looping_sound_owner_coord(
        &self,
        id: crate::sim::anim_class::AnimId,
    ) -> Option<crate::sim::anim_class::AnimWorldCoord> {
        if let Some(carrier) = crate::sim::bomb::ticking_sound_carrier(id) {
            return self.bomb_ticking_coord(carrier);
        }
        if let Some(coord) = self.anim_absolute_coord(id) {
            return Some(coord);
        }
        self.substrate
            .entities
            .get(id)
            .map(Self::movement_sound_world)
    }

    pub(crate) fn movement_sound_world(
        entity: &crate::sim::game_entity::GameEntity,
    ) -> crate::sim::anim_class::AnimWorldCoord {
        let locomotor_z = entity
            .locomotor
            .as_ref()
            .map_or(0, |locomotor| locomotor.altitude.to_num::<i32>());
        crate::sim::anim_class::AnimWorldCoord {
            x: i32::from(entity.position.rx)
                .wrapping_mul(256)
                .wrapping_add(entity.position.sub_x.to_num::<i32>()),
            y: i32::from(entity.position.ry)
                .wrapping_mul(256)
                .wrapping_add(entity.position.sub_y.to_num::<i32>()),
            z: i32::from(entity.position.z)
                .wrapping_mul(crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS)
                .wrapping_add(locomotor_z),
        }
    }

    /// Release an active FootClass MoveSound while the object is still
    /// represented, preserving the native stop-before-UnInit ordering.
    pub(crate) fn release_move_sound(&mut self, stable_id: u64) {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        if !entity.move_sound_active {
            return;
        }
        let world = Self::movement_sound_world(entity);
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.move_sound_active = false;
            entity.move_sound_countdown = 0;
        }
        self.sound_events.push(SimSoundEvent::AnimationStopped {
            anim_id: stable_id,
            stop_sound_id: None,
            world,
        });
    }

    /// FootClass's post-locomotor MoveSound tail. A fresh moving-now virtual
    /// answer or locomotor state change keeps the handle alive and reloads the
    /// three-visit grace counter. The process-local audio owner selects the
    /// sample only after its device and spatial-acceptance gates.
    fn tick_move_sound_after_process(
        &mut self,
        stable_id: u64,
        before: Option<MovementSoundProbe>,
        rules: Option<&RuleSet>,
    ) {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        if entity.category == EntityCategory::Structure || entity.locomotor.is_none() {
            return;
        }
        let after = self.movement_sound_probe(stable_id);
        let movement_changed = before.is_some() && before != after;
        let moving_now = crate::sim::movement::ready_producer::is_moving_now_for(
            entity,
            self.session.binary_frame,
        );
        let falling_or_crashing = entity.object_is_falling_down != 0
            || entity.parachute_state.is_some()
            || entity.locomotor.as_ref().is_some_and(|locomotor| {
                // A Jumpjet in its descent state (the only locomotor with a
                // crash speed). Read from the locomotor's own state field.
                locomotor.jumpjet_crash_speed > crate::util::fixed_math::SIM_ZERO
                    && locomotor.jumpjet_runtime().is_some_and(|runtime| {
                        runtime.phase == crate::sim::movement::jumpjet_flight::STATE_DESCEND
                    })
            });
        let active = entity.move_sound_active;
        let countdown = entity.move_sound_countdown;
        let type_ref = entity.type_ref();
        let world = Self::movement_sound_world(entity);
        let qualifies = (movement_changed || moving_now) && !falling_or_crashing;

        if qualifies {
            let mut started = false;
            if !active {
                let configured = rules
                    .and_then(|rules| self.object_type(type_ref, rules))
                    .and_then(|object| object.move_sound.as_deref())
                    .map(str::trim)
                    .filter(|sound| !sound.is_empty() && !sound.eq_ignore_ascii_case("none"))
                    .map(str::to_owned);
                if let Some(configured) = configured {
                    // gamemd `FootClass__AI @ 0x004DA530`: the active MoveSound
                    // tail loads `g_MainRng` at 0x004DAAC0, calls `Random__Next`
                    // at 0x004DAACB, then indexes the vector at 0x004DAAD3.
                    let _sound_index_draw = self.main_rng.next_u32();
                    let sound_id = self.interner.intern(&configured);
                    self.sound_events.push(SimSoundEvent::AnimationStarted {
                        anim_id: stable_id,
                        sound_id,
                        world,
                    });
                    started = true;
                }
            }
            if active || started {
                if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                    entity.move_sound_active = true;
                    entity.move_sound_countdown = 3;
                }
            }
        } else if active {
            if countdown == 0 || falling_or_crashing {
                self.release_move_sound(stable_id);
            } else if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                entity.move_sound_countdown = countdown - 1;
            }
        }
    }

    /// Accepted House result after its serialized SavourDelay has expired.
    /// Simulation termination and app outcome presentation share this query.
    /// Explicit solo wins/losses remain valid; the developer solo exception
    /// belongs only to automatic victory creation in `check_defeat`.
    pub(crate) fn ready_outcome_for_owner(
        &self,
        owner: InternedId,
    ) -> Option<crate::sim::house_state::HouseOutcomeState> {
        self.houses
            .get(&owner)?
            .outcome_state
            .filter(|outcome| outcome.exit_ready)
    }

    fn natural_outcome_exit_ready(&self) -> bool {
        self.houses
            .iter()
            .any(|(&owner, house)| house.is_human && self.ready_outcome_for_owner(owner).is_some())
    }

    fn termination_frame_requested(&self) -> bool {
        self.quit_requested || self.connection_lost || self.natural_outcome_exit_ready()
    }

    /// Advance the global ambient scalar before ore and active superweapons.
    /// A storm selecting Ion later in this frame can first move it next frame.
    fn tick_scenario_lighting_transition(&mut self, rules: &RuleSet) {
        if self.session.lighting.advance_transition_if_due(
            self.session.binary_frame,
            rules.general.ambient_change_rate_nonzero,
            rules.general.ambient_change_interval_frames,
            rules.general.ambient_change_step,
        ) {
            self.publish_global_lighting();
        }
    }

    fn tick_ore_growth_rungs(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
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
                self.production.ore_growth_state.tick_native_spread_driver(
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
            }
        }
    }

    /// Install the initial normalized MapClass playfield authority.
    ///
    /// `MapClass::Set_Clipped_LocalSize @ 0x00567230` establishes the five
    /// predicate fields. Size height is retained separately because later
    /// action-40 writers normalize another raw LocalSize against the same Size.
    pub(crate) fn install_playfield_from_map_header(
        &mut self,
        header: &crate::map::map_file::MapHeader,
    ) {
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
    pub(crate) fn change_visible_map_area(
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
                    (i32::from(entity.position.rx), i32::from(entity.position.ry)),
                    Some(bounds),
                    self.resolved_terrain.as_ref(),
                );
                let reveal = !entity.in_playfield
                    && member
                    && entity.lifecycle.object_alive
                    && !entity.lifecycle.in_limbo
                    && !entity.dying
                    && entity.health.current > 0
                    && entity.category != EntityCategory::Structure
                    && self
                        .houses
                        .get(&entity.owner())
                        .is_some_and(|house| house.is_human);
                Some((stable_id, member, reveal))
            })
            .collect::<Vec<_>>();
        for &(stable_id, member, _) in &membership_updates {
            if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                entity.in_playfield = member;
            }
        }
        // FUN_006E21E0's false->true callback is the Techno reveal/update
        // virtual (`0x0070ADC0`), only for human-owned live nonlimbo mobiles;
        // Buildings are explicitly excluded. Rust's owned equivalent commits
        // the sight reveal immediately, before this action returns.
        let reveal_config = crate::sim::vision::VisionConfig {
            require_playfield_membership: true,
            veteran_sight: rules.map_or(0.0, |rules| rules.general.veteran_sight),
            leptons_per_sight_increase: rules
                .map_or(0, |rules| rules.general.leptons_per_sight_increase),
            reveal_by_height: rules.is_none_or(|rules| rules.general.reveal_by_height),
            fog_of_war: self.session.game_options.fog_of_war,
        };
        let height_grid = reveal_config
            .reveal_by_height
            .then(|| {
                self.path_grid
                    .as_ref()
                    .map(|grid| grid.ground_height_grid())
            })
            .flatten();
        for (stable_id, _, reveal) in membership_updates {
            if reveal && let Some(entity) = self.substrate.entities.get(stable_id) {
                let sight_ability =
                    crate::sim::vision::entity_has_sight_ability(entity, &self.interner, rules);
                crate::sim::vision::reveal_entity_vision(
                    &mut self.fog,
                    entity,
                    &reveal_config,
                    height_grid.as_deref(),
                    sight_ability,
                    &self.interner,
                );
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

    /// Exact mode-one query for the stored TechnoClass+0x3D5 writer family.
    /// Absence means there is no live MapClass authority, not rectangular or
    /// permissive fallback authority.
    fn entity_playfield_membership_mode_one(
        &self,
        stable_id: u64,
        terrain: Option<&ResolvedTerrainGrid>,
    ) -> Option<bool> {
        let bounds = self.playfield_bounds?;
        let entity = self.substrate.entities.get(stable_id)?;
        Some(crate::sim::cell_rect::cell_is_in_playfield_height_aware(
            (i32::from(entity.position.rx), i32::from(entity.position.ry)),
            Some(bounds),
            terrain,
        ))
    }

    /// Unlimbo's exact establishment writer (`TechnoClass::Unlimbo @
    /// 0x006F6CFE`).
    fn establish_entity_playfield_membership_on_unlimbo(
        &mut self,
        stable_id: u64,
        context: UninitContext<'_>,
    ) {
        let Some(member) = self.entity_playfield_membership_mode_one(
            stable_id,
            context.terrain().or(self.resolved_terrain.as_ref()),
        ) else {
            return;
        };
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.in_playfield = member;
        }
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

    /// Ordinary per-cell movement writer (`0x006F511A..0x006F5139`): only
    /// promote 0 -> 1. A unit that walks back outside retains membership until
    /// an exact writer (teleport or Set_Clipped_LocalSize) clears it.
    pub(crate) fn promote_entity_playfield_membership_after_move(&mut self, stable_id: u64) {
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_none_or(|entity| entity.in_playfield)
        {
            return;
        }
        if self.entity_playfield_membership_mode_one(stable_id, self.resolved_terrain.as_ref())
            == Some(true)
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
        {
            entity.in_playfield = true;
        }
    }

    /// Teleport arrival's exceptional exact outside clear (`0x00719A99`). An
    /// inside arrival does not promote a previously-false byte.
    fn clear_entity_playfield_membership_after_teleport(&mut self, stable_id: u64) {
        if self.entity_playfield_membership_mode_one(stable_id, self.resolved_terrain.as_ref())
            == Some(false)
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
        {
            entity.in_playfield = false;
        }
    }

    fn poll_triggers_for_master_frame(&mut self, inputs: TriggerInputs<'_>) {
        // YR LogicClass::Update polls scenario triggers before the live-object walk.
        let effects = self.advance_triggers(inputs);
        self.trigger_effects.extend(effects);
    }

    /// Drain app-owned outcomes after their authoritative trigger actions ran.
    /// Production consumes trigger effects through `SimFrameOutput`; only
    /// fixture replay and trigger tests drain them directly.
    #[cfg(test)]
    pub(crate) fn drain_trigger_effects(&mut self) -> Vec<TriggerEffect> {
        std::mem::take(&mut self.trigger_effects)
    }

    /// Enable or disable per-entity debug event logging (F10 boundary method:
    /// the app toggles, sim owns the write). Flips the spawn-time flag and
    /// reconciles every existing entity, so existing and future entities
    /// always agree with the toggle. Enabling preserves logs an entity
    /// already accumulated; disabling drops them all.
    pub(crate) fn set_debug_event_logging(&mut self, enabled: bool) {
        self.debug_event_logging = enabled;
        for entity in self.substrate.entities.values_mut() {
            if enabled {
                if entity.debug_log.is_none() {
                    entity.debug_log = Some(crate::sim::debug_event_log::DebugEventLog::new());
                }
            } else {
                entity.debug_log = None;
            }
        }
    }

    /// Pre-merge the named owner's fog visibility so render-side queries hit
    /// the O(1) merged cache (F10 boundary method: the app requests, sim owns
    /// the write). Returns false when the owner name is not interned yet —
    /// no view is built and the per-query slow path stays in effect.
    pub(crate) fn prepare_fog_view_for(&mut self, owner: &str) -> bool {
        let Some(owner_id) = self.interner.get(owner) else {
            return false;
        };
        self.fog.build_merged_for(owner_id, &self.interner);
        true
    }

    /// Configure the command input delay (F10 boundary method; the app pushes
    /// its configured value once at match install).
    pub(crate) fn set_input_delay_ticks(&mut self, ticks: u64) {
        self.input_delay_ticks = ticks;
    }

    #[cfg(test)]
    pub(crate) fn take_master_frame_test_trace(&mut self) -> Vec<MasterFrameTestRung> {
        std::mem::take(&mut self.master_frame_test_trace)
    }

    #[cfg(test)]
    fn trace_master_frame_rung(&mut self, rung: MasterFrameTestRung) {
        self.master_frame_test_trace.push(rung);
    }

    #[cfg(test)]
    pub(crate) fn take_house_ai_activation_order_test_trace(
        &mut self,
    ) -> Vec<HouseAiActivationOrderTestEvent> {
        std::mem::take(&mut self.house_ai_activation_order_test_trace)
    }

    #[cfg(test)]
    fn trace_house_ai_activation_order(&mut self, event: HouseAiActivationOrderTestEvent) {
        self.house_ai_activation_order_test_trace.push(event);
    }

    #[cfg(test)]
    pub(crate) fn append_house_order_after_for_test(
        &mut self,
        after_owner: InternedId,
        appended_owner: InternedId,
    ) {
        debug_assert!(self.houses.contains_key(&appended_owner));
        debug_assert!(!self.session.house_order.contains(&appended_owner));
        self.house_update_append_after_test = Some((after_owner, appended_owner));
    }

    /// Shared identity source for every modeled runtime `AbstractClass` analogue.
    ///
    /// `AbstractClass::AssignUniqueID @ 0x00410230` delegates to
    /// `ScenarioClass::NextUniqueID @ 0x0068BCB0`; individual stores therefore
    /// must not own independent counters.
    pub(crate) fn allocate_stable_id(&mut self) -> u64 {
        let id = self.substrate.next_stable_object_id;
        self.substrate.next_stable_object_id =
            self.substrate.next_stable_object_id.saturating_add(1);
        id
    }

    pub(crate) fn admit_projectile(
        &mut self,
        stable_id: u64,
        spawn: crate::sim::projectile::ProjectileSpawn,
    ) -> u64 {
        self.projectiles
            .spawn_at(stable_id, self.session.binary_frame, spawn);
        let registered = self.register_projectile(stable_id, spawn.flat);
        debug_assert!(registered);
        stable_id
    }

    pub(crate) fn admit_wave(&mut self, stable_id: u64, wave: crate::sim::wave::Wave) -> u64 {
        self.waves.spawn(stable_id, wave);
        let registered = self.register_wave(stable_id);
        debug_assert!(registered);
        stable_id
    }

    /// Native Reveal's append: +0x98 guard → tail-append → set flag. Idempotent.
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
                    .is_some_and(|entity| entity.in_logic_vector)
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
            .filter(|e| e.in_logic_vector)
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

    /// Debug-only checks for relationships that remain true while the native
    /// lifecycle axes themselves are deliberately independent.
    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_lifecycle_consistent(&self) {
        for entity in self.substrate.entities.values() {
            if !entity.lifecycle.object_alive {
                debug_assert!(
                    entity.lifecycle.in_limbo,
                    "dead entity {} must have completed Conceal before alive clear",
                    entity.stable_id()
                );
                debug_assert!(
                    !entity.lifecycle.cell_marked,
                    "dead entity {} must be unmarked before alive clear",
                    entity.stable_id()
                );
                debug_assert!(
                    !entity.in_logic_vector,
                    "dead entity {} must leave LogicVector before alive clear",
                    entity.stable_id()
                );
                debug_assert!(
                    entity.destruction_recorded,
                    "dead entity {} must record its destruction exactly once before alive clear",
                    entity.stable_id()
                );
                debug_assert!(
                    self.substrate.pending_delete.contains(&entity.stable_id()),
                    "dead entity {} must remain pending until finalization",
                    entity.stable_id()
                );
            }
            if entity.lifecycle.cell_marked
                && !entity.passenger_role.is_inside_transport()
                && entity.occupancy_list_layer().is_some()
            {
                for (rx, ry) in crate::sim::occupancy::entity_occupancy_cells(entity) {
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
    }

    /// The active order, verbatim. No sorted-ID fallback (was DRIFT).
    ///
    /// This is a point-in-time copy: a consumer iterating it CANNOT observe an
    /// object registered or unregistered during its own pass. For native
    /// same-pass membership semantics use [`Self::for_each_live_object`].
    pub(crate) fn live_object_order_snapshot(&self) -> Vec<u64> {
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
    #[cfg(test)]
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
            body(self, id)?;
            i += 1;
        }
        Ok(())
    }

    /// Recompute the serialized/hash-covered OrePurifier count without changing
    /// cash or accumulated spending/harvesting statistics. Iterate only existing
    /// houses; never insert a missing house. A single pass over the entity store
    /// accumulates purifier counts per owner, so the cost is O(entities), not
    /// O(houses x entities). `rules` is the advance_tick tail's `Option`; with
    /// `None` the purifier count is 0 (no type data to classify structures by).
    pub(crate) fn refresh_economy_shadow(&mut self, rules: Option<&RuleSet>) {
        // One pass: accumulate OrePurifier building count per owner through
        // the same `House+0x538C` predicate as `count_purifiers_for_owner`
        // (`counts_as_purifier`: completed, alive, on-map purifier — a
        // `building_up` one is still before `OnConstructionComplete`
        // 0x0044637C and does not count), in a single sweep keyed by owner id.
        let mut purifiers: std::collections::BTreeMap<crate::sim::intern::InternedId, i32> =
            std::collections::BTreeMap::new();
        if let Some(rules) = rules {
            for e in self.substrate.entities.values() {
                if crate::sim::miner::miner_system::counts_as_purifier(self, rules, e) {
                    *purifiers.entry(e.owner()).or_insert(0) += 1;
                }
            }
        }
        for (id, house) in self.houses.iter_mut() {
            // Purifier-bonus base = real OrePurifier building COUNT (NOT silo
            // storage capacity, NOT the AI-virtual-inclusive effective count). Hashed.
            house.economy.purifier_count = purifiers.get(id).copied().unwrap_or(0);
            // spent_credits / harvested_credits accumulate via step_all / deposits;
            // intentionally untouched here.
        }
    }

    /// Per-tick production tail: refresh the per-house economy shadow (purifier count).
    /// Runs at the advance_tick tail, AFTER all authoritative systems.
    ///
    /// P5d: the factory registry is the authoritative queue-of-record and is mutated
    /// DIRECTLY by enqueue/cancel/delivery — there is no longer a `reconcile_from_queues`
    /// pass (the `queues_by_owner` mirror is retired), so its progress simply persists
    /// across ticks with no end-of-tick rebuild. `rules` is the tail's `Option`, threaded
    /// to the economy refresh.
    pub(crate) fn refresh_production_shadow(&mut self, rules: Option<&RuleSet>) {
        self.refresh_economy_shadow(rules);
    }

    /// Debug-only production asserts: the factory shell
    /// trace is well-formed (live Structures, strictly-increasing visit order).
    /// Divergence is surfaced, never equalized.
    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_production_shadow(&self) {
        self.debug_assert_factory_shell_trace();
        self.debug_assert_factory_conservation(); // P3
        self.debug_assert_factory_invariants(); // P5b (repurposed from the P5a inversion assert)
    }

    /// Debug-only P3 assert: each live shadow factory's `advance_one_step` conserves
    /// exact cost (C15) and settles correctly (C2/C12). Steps a CLONE against a CLONE
    /// economy seeded with exactly `original_balance`; SURFACES divergence with
    /// tick + owner + category, NEVER writes back to the shadow or the wallet.
    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_factory_conservation(&self) {
        use crate::sim::economy::Economy;
        use crate::sim::production::{PRODUCTION_STEPS, StepOutcome};
        for factory in self.production.factory_shadow.iter_insertion_ordered() {
            if factory.object.is_none() {
                continue; // queue-only / no active object: nothing to conserve
            }
            let cost = factory.original_balance;
            // A fresh, armed clone driven from progress 0 with exact funds.
            let mut f = factory.clone();
            f.progress = 0;
            f.balance = cost;
            f.on_hold = false;
            f.suspended = false;
            f.manual = false;
            let mut econ = Economy {
                credits: cost,
                ..Economy::default()
            };
            let mut steps = 0i32;
            loop {
                match f.advance_one_step(&mut econ) {
                    StepOutcome::Stepped => steps += 1,
                    StepOutcome::Completed => {
                        steps += 1;
                        break;
                    }
                    // Stalled/Idle cannot happen with exact funds + a fresh arm; the
                    // asserts below fire (steps != 54) and surface the divergence.
                    _ => break,
                }
            }
            debug_assert_eq!(
                steps, PRODUCTION_STEPS as i32,
                "C2: tick {} {:?}/{:?}: a full build must take 54 steps (got {})",
                self.session.tick, factory.owner, factory.category, steps,
            );
            debug_assert_eq!(
                econ.spent_credits, cost,
                "C15: tick {} {:?}/{:?}: total spent {} must equal full cost {}",
                self.session.tick, factory.owner, factory.category, econ.spent_credits, cost,
            );
            debug_assert_eq!(
                f.balance, 0,
                "C12: tick {} {:?}/{:?}: completion must zero the balance",
                self.session.tick, factory.owner, factory.category,
            );
            debug_assert!(
                f.suspended && f.object.is_some(),
                "C12: tick {} {:?}/{:?}: completion must suspend with the object attached",
                self.session.tick,
                factory.owner,
                factory.category,
            );
        }
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
        // the tail stamps strictly increase AND exceed the active build's `insertion_seq`
        // (FIFO `push_back` of a monotonic mint: the active build is the oldest, the tail
        // newer) — this is the D1 invariant expressed as a real self-check.
        let ordered = self.production.factory_shadow.iter_insertion_ordered();
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

        // (B) STATE: progress in 0..=54; 0 <= balance <= original_balance (the per-step
        // ladder only decrements balance, and cancel resets both to 0).
        for f in self.production.factory_shadow.iter_insertion_ordered() {
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
                f.balance >= 0 && f.balance <= f.original_balance,
                "P5b (B): tick {} {:?}/{:?}: balance {} out of [0, original {}]",
                self.session.tick,
                f.owner,
                f.category,
                f.balance,
                f.original_balance,
            );
        }
    }

    /// Test-only: force the active order and sync membership flags to it.
    #[cfg(test)]
    pub(crate) fn set_logic_order_for_test(&mut self, order: Vec<u64>) {
        for entity in self.substrate.entities.values_mut() {
            entity.in_logic_vector = false;
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
                e.in_logic_vector = true;
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

    /// Admit the `VoxelAnimClass` debris a death threw.
    ///
    /// gamemd-derived: `VoxelAnimClass::Constructor @ 0x007493B0` assigns the
    /// shared unique id (`AbstractClass::AssignUniqueID`), appends to the
    /// VoxelAnim registry, then `ObjectClass::Unlimbo` reveals the piece into
    /// the LogicClass vector. The launch velocity and the physics body were
    /// already built inside the combat transaction, which consumed the draws in
    /// native order; only the identity and the registration happen here.
    pub(crate) fn admit_death_debris(
        &mut self,
        spawns: Vec<crate::sim::voxel_anim::VoxelDebrisSpawn>,
    ) {
        for spawn in spawns {
            let stable_id = self.allocate_stable_id();
            let mut object = spawn.object;
            object.stable_id = stable_id;
            self.substrate.voxel_anims.insert(object);
            self.reveal_voxel_anim(stable_id);
        }
    }

    /// Apply one of `house_tracking`'s writers (Add/Remove_Tracking,
    /// Added_To_Game/Removed_From_Game) for an object on its owner's house.
    pub(crate) fn update_house_tracking(
        &mut self,
        stable_id: u64,
        update: fn(
            &mut crate::sim::house_tracking::HouseTracking,
            &crate::sim::game_entity::GameEntity,
        ),
    ) {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        if let Some(house) = self.houses.get_mut(&entity.owner()) {
            update(&mut house.tracking, entity);
        }
    }

    /// Append one successfully committed BuildConst Building to its owning
    /// House's native acquisition-ordered vector.
    ///
    /// gamemd-derived: `BuildingClass::Unlimbo @ 0x004411B6..0x00441223`.
    pub(crate) fn append_live_build_const(&mut self, stable_id: u64) {
        let Some(owner) = self.substrate.entities.get(stable_id).and_then(|entity| {
            (entity.category == EntityCategory::Structure
                && entity.build_const_eligible
                && entity.lifecycle.object_alive
                && !entity.lifecycle.in_limbo
                && entity.lifecycle.cell_marked)
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
        #[cfg(test)]
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
            .map(|entity| (entity.owner(), entity.build_const_eligible))
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
    #[cfg(test)]
    pub(crate) fn change_owner(&mut self, stable_id: u64, new_owner: InternedId) {
        self.change_owner_impl(stable_id, new_owner, None);
    }

    pub(crate) fn change_owner_with_rules(
        &mut self,
        stable_id: u64,
        new_owner: InternedId,
        rules: &RuleSet,
    ) {
        self.change_owner_impl(stable_id, new_owner, Some(rules));
    }

    fn change_owner_impl(
        &mut self,
        stable_id: u64,
        new_owner: InternedId,
        rules: Option<&RuleSet>,
    ) {
        let Some((old_owner, category, has_spawn_manager, build_const_eligible)) =
            self.substrate.entities.get(stable_id).map(|entity| {
                (
                    entity.owner(),
                    entity.category,
                    entity.spawn_manager.is_some(),
                    entity.build_const_eligible,
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
            && !rules
                .object(self.interner.resolve(entity.type_ref()))
                .is_some_and(|object| object.can_be_occupied)
        {
            self.bomb_defuse(stable_id);
        }
        if category == EntityCategory::Structure {
            //448260: gap removal precedes ordinary sight release and owner
            //swap. This order differs from Techno Limbo's sight-then-gap.
            self.remove_building_gap_before_limbo(stable_id);
            self.fog.release_entity_sight(stable_id);
        }
        // FootClass::ChangeOwner @ 0x004DBED0 removes from the deposited old
        // owner and adds to the new owner before later readers observe it.
        if let Some(rules) = rules {
            self.transfer_sensor_before_owner_change_with_rules(stable_id, new_owner, rules);
        } else {
            self.transfer_sensor_before_owner_change(stable_id, new_owner);
        }

        // Active YR chain: BuildingClass::ChangeOwner (0x00448260) delegates
        // to TechnoClass::ChangeOwner (0x007014A0), which moves the house
        // counts (`house_tracking`) below.

        // `TechnoClass::ChangeOwner` calls `SpawnManagerClass::Kill_All_Spawns`
        // before the house swap: a mind-controlled V3/Dreadnought/Boomer loses
        // the pool it built for its old owner. Run first so the children are
        // destroyed while still attributed to the previous house. The owner is
        // still alive here, so the slots re-arm with a zero regen wait and the
        // new owner's pool is rebuilt on the next manager pass.
        if has_spawn_manager {
            crate::sim::spawn_manager::kill_all_spawns(self, stable_id);
        }
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
        // gamemd-derived: `BuildingClass::ChangeOwner @ 0x00448260` removes
        // this pointer from the old House BuildConst vector before delegating
        // the Techno owner swap, then appends it to the new House tail.
        if build_const_eligible {
            self.remove_build_const_from_owner(stable_id);
        }
        // Techno70158A..7015E6: Removed_From_Game on the old house (not in
        // limbo), then Remove_Tracking from it and Add_Tracking to the new.
        let on_map = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| !entity.lifecycle.in_limbo);
        if on_map {
            self.update_house_tracking(
                stable_id,
                crate::sim::house_tracking::HouseTracking::removed_from_game,
            );
        }
        self.update_house_tracking(
            stable_id,
            crate::sim::house_tracking::HouseTracking::remove_tracking,
        );
        // `TechnoClass::ChangeOwner` runs the live-detach targeting sweep next,
        // before the house swap: everything shooting at this object is released
        // while the object still belongs to its old house. Engineer capture and
        // garrison transfer both come through here, so a squad that was firing
        // at a building stops the instant the building changes hands instead of
        // shooting at what is now its own structure.
        self.stop_all_targeting_on_detach(stable_id);
        self.substrate.entities.change_owner(stable_id, new_owner);
        self.update_house_tracking(
            stable_id,
            crate::sim::house_tracking::HouseTracking::add_tracking,
        );
        // `BuildingClass::ChangeOwner @ 0x00448723` marks every transferred
        // building HasBeenCaptured (+0x6E3); survivors read it at death.
        if category == EntityCategory::Structure
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
        {
            entity.has_been_captured = true;
        }
        // Techno701735..701751 writes the owner then recomputes only +41A.
        // A former current-house object's +41B history survives the transfer.
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.discovery.owned_by_current_house = self.session.current_house == Some(new_owner);
        }
        // Techno701757..70178E: Added_To_Game on the new house (not in limbo).
        if on_map {
            self.update_house_tracking(
                stable_id,
                crate::sim::house_tracking::HouseTracking::added_to_game,
            );
        }
        if build_const_eligible
            && let Some(house) = self.houses.get_mut(&new_owner)
            && !house.build_const_order.contains(&stable_id)
        {
            house.build_const_order.push(stable_id);
        }
        // `TechnoClass::ChangeOwner` closes with the mission half (the
        // `+0x484` call at 0x00701849 reads the NEW owner's
        // IsControlledByHuman). A rules-less transfer (tests only) cannot
        // read the type and skips it.
        if let Some(rules) = rules {
            self.change_owner_mission_half(stable_id, rules);
        }
        self.foot_neighbors_after_owner_change(stable_id, rules);
        self.refresh_waypoint_edge_from_committed_structure(stable_id);
        self.reveal_building_sight_after_owner_change(stable_id, rules);
        if let Some(rules) = rules {
            self.reapply_building_gap_after_owner_change(stable_id, rules);
        }
    }

    /// The mission half of `TechnoClass::ChangeOwner @ 0x007014A0`, every
    /// class (read 2026-09-23):
    /// - `0x007014D5..0x0070151A`: `Assign_Target(0)` (`+0x3C8`),
    ///   `Assign_Destination(0, 1)` (`+0x480`), and the ArchiveTarget
    ///   (`+0x218`) cleared unless a Unit is deploying (`0x00746DB0`, the
    ///   `+0x6E1`/`+0x6E2` latches);
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
    ///   exit link): `Assign_Destination(0, 1)`, `Assign_Target(0)` and
    ///   `Enter_Idle_Mode(0, 1)` (`+0x484`), which reads the NEW owner:
    ///   - a war or chrono miner takes the Unit leaf's harvester arm
    ///     ([`harvester_enter_idle_mode_selector`]): Harvest for the new owner,
    ///     Guard when that owner is human and the miner stands off ore, and
    ///     nothing while in radio contact (a miner docked at its refinery) or
    ///     while the Guard above is still only queued (a miner caught
    ///     mid-track);
    ///   - any other Unit or Infantry takes VERA's Foot selector
    ///     (`queue_foot_enter_idle_mode`) in place of the Unit `0x00738970`
    ///     and Infantry `0x0051CBA0` leaves (residual below);
    ///   - a Building (`0x0044D6A0` with `initial = 0`) calls `0x00447780(1)`
    ///     and `Queue_Mission(Guard, 0)`.
    ///
    /// VERA's legacy `order_intent` (an AttackMove goal or Guard anchor) is
    /// the duplicate of the TarCom/NavCom orders this drops, so it goes too:
    /// no order survives an owner change.
    ///
    /// Callers: engineer capture and garrison transfer, and mind control's
    /// CaptureUnit and FreeUnit. No stock miner is ever mind-controlled: HARV,
    /// CMIN and SMIN are `ImmuneToPsionics=yes`.
    ///
    /// RESIDUALS:
    /// - The Foot leaves also pick AreaGuard (a human house with the
    ///   GUARD_AREA option, or `DefaultToGuardArea=`; the AI IQ, Team and
    ///   SlaveOwner arms), Harvest or Unload, and read TarCom first; VERA's
    ///   selector returns Guard, Move or nothing. Trigger: a captured or
    ///   released Foot of such a type. Effect: Guard instead of AreaGuard.
    ///   Frequency: nil in stock (every `DefaultToGuardArea=` type is
    ///   psionic-immune and no stock house sets GUARD_AREA).
    /// - The Building leaf's `0x00447780(1)` re-selects the building's idle
    ///   animation state (BState `+0x534`/`+0x538`, the anim timer
    ///   `+0xF8..+0x10C`); VERA keeps the current one. Trigger: engineer
    ///   capture, Yuri Prime capture or garrison transfer. Effect: the
    ///   captured building's animation state (presentation). Frequency: per
    ///   building capture.
    /// - Aircraft Enter_Idle_Mode (`0x004176F0`) is not ported (no stock
    ///   aircraft can be captured); the trailing `+0x423`-gated
    ///   `vt+0x498`/`vt+0x494` and `vt+0x488(0, 0, 0, 0, 0)` calls are
    ///   unidentified.
    ///
    /// `+0x484` census (`search_instructions CALL [+0x484]`), the sites a
    /// miner can reach besides Move arrival, this owner change and the depot
    /// exit, with their gates; none is run by VERA today:
    /// - `DriveLocomotionClass::Stop_And_Scatter @ 0x004B48BE` → `+0x484(0,1)`
    ///   after `Stop_Moving` when the NavQueue is non-empty. VERA scatter is
    ///   UNCHECKED; natively a human-parked Guard miner scattered on ore
    ///   would re-queue Harvest through this call.
    /// - `FootClass::Enter_Destination @ 0x004DA1A6` (call at 0x004DA1C0;
    ///   callers `EventClass::Execute` 0x004C7453/0x004C759C and
    ///   `UnitClass::Receive_Radio` 0x00737546) → `+0x484(0,1)` when
    ///   NavCom == 0, current == Guard and no WeaponsFactory contact.
    /// - `FootClass::Check_Destination_Is_UnitRepair_Dock @ 0x004DBA15` →
    ///   `+0x484(0, !depot-contact)` when NavCom == 0 && TarCom == 0
    ///   (virtual slot; miner reachability unestablished).
    /// - `FootClass::ReceiveDamage @ 0x004D74C7`, gated on
    ///   `MissionControl[current].NoThreat && !Zombie`; no retail mission
    ///   sets either, so the call is unreachable on stock data.
    fn change_owner_mission_half(&mut self, stable_id: u64, rules: &RuleSet) {
        use crate::sim::mission::{MissionId, MissionType};
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        let category = entity.category;
        let current = entity.mission.current().known();
        let object = self.object_type(entity.type_ref(), rules);
        let unit_deploying = category == EntityCategory::Unit
            && entity.mission_leaf.as_unit().is_some_and(|leaf| {
                leaf.deploy_begin_active() != 0 || leaf.deploy_reverse_active() != 0
            });
        let simple_deployer_unloading = category == EntityCategory::Unit
            && current == Some(MissionType::Unload)
            && object.is_some_and(|object| object.is_simple_deployer);
        let factory_unloading = category == EntityCategory::Structure
            && current == Some(MissionType::Unload)
            && object.is_some_and(|object| object.weapons_factory);
        let in_limbo = entity.lifecycle.in_limbo;
        let is_dispatchable_miner = category == EntityCategory::Unit
            && entity
                .miner
                .as_ref()
                .is_some_and(|m| m.kind != crate::sim::miner::MinerKind::Slave);
        let now = self.session.binary_frame;
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            crate::sim::mission::concrete_effects::represented_assign_target(entity, None);
            crate::sim::mission::concrete_effects::represented_assign_destination_mode_one(
                entity, None,
            );
            entity.movement_target = None;
            entity.order_intent = None;
            if !unit_deploying {
                entity.base_defense_response.set_archive_target(None);
            }
        }
        let readiness = crate::sim::mission::authority::LiveReadyInputProvider { rules };
        if current != Some(MissionType::Selling) && !simple_deployer_unloading {
            // `Queue_Mission(Guard, 1)`: the queue write, then Ready_To_Commence
            // (`+0x200`) and Commence (`+0x1EC`). The immediate promotion runs
            // through the host's promotion step so the recorded readiness
            // degradation (absent locomotor producers read as "not moving")
            // applies here exactly as it does at the per-tick AI position.
            let _ = self.mission_queue_exact(
                stable_id,
                MissionId::from_known(MissionType::Guard),
                0,
                now,
                &readiness,
            );
            self.mission_host_promote(stable_id, now, rules);
        }
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
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            crate::sim::mission::concrete_effects::represented_assign_destination_mode_one(
                entity, None,
            );
            entity.movement_target = None;
            crate::sim::mission::concrete_effects::represented_assign_target(entity, None);
        }
        match category {
            EntityCategory::Unit if is_dispatchable_miner => {
                if let Some(selector) =
                    harvester_enter_idle_mode_selector(self, stable_id, rules, false)
                {
                    let _ = self.mission_queue_exact(
                        stable_id,
                        MissionId::from_known(selector),
                        0,
                        now,
                        &readiness,
                    );
                }
            }
            EntityCategory::Unit | EntityCategory::Infantry => {
                queue_foot_enter_idle_mode(self, stable_id, rules);
            }
            EntityCategory::Structure => {
                let _ = self.mission_queue_exact(
                    stable_id,
                    MissionId::from_known(MissionType::Guard),
                    0,
                    now,
                    &readiness,
                );
            }
            EntityCategory::Aircraft => {}
        }
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
    pub(crate) fn contending_house_count(&self) -> usize {
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
    /// registry are bound.
    pub fn rebuild_caches_after_load(
        &mut self,
        mut resolved_terrain: ResolvedTerrainGrid,
        terrain_speed_config: terrain_speed::TerrainSpeedConfig,
        bridge_explosions: Vec<InternedId>,
        metallic_debris: Vec<InternedId>,
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
                &mut self.production,
                terrain,
                Some(&mut resolved_terrain),
            );
        }
        for terrain in &terrain_objects {
            if terrain.is_live() {
                crate::sim::terrain_object::mark_terrain_occupation(
                    &mut self.production,
                    terrain,
                    Some(&mut resolved_terrain),
                );
            }
        }
        let terrain_costs = build_canonical_terrain_cost_grids(&resolved_terrain);

        self.resolved_terrain = Some(resolved_terrain);
        self.terrain_speed_config = terrain_speed_config;
        self.bridge_explosions = bridge_explosions;
        self.metallic_debris = metallic_debris;
        self.terrain_costs = terrain_costs;
    }

    /// Rebuild LogicClass membership flags from the restored active order.
    ///
    /// `+0x98` is not serialized (native does not round-trip it); vector presence
    /// is authoritative. Idempotent — safe to call after any load. Standalone (no
    /// heavy load-arg dependency) so save/load membership is unit-testable.
    pub(crate) fn rebuild_logic_membership(&mut self) {
        for entity in self.substrate.entities.values_mut() {
            entity.in_logic_vector = false;
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
                entity.in_logic_vector = true;
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

    /// Pin the current navigation projection across a mutable simulation frame.
    pub fn path_grid_snapshot(&self) -> Option<Arc<PathGrid>> {
        self.path_grid.clone()
    }

    /// Rebuild terrain costs, dynamic structure blockers, zones, and the
    /// canonical PathGrid as one simulation-owned projection.
    pub fn rebuild_dynamic_navigation(&mut self, rules: &RuleSet) -> bool {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        navigation::NavigationCaches {
            terrain_costs: &mut self.terrain_costs,
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
        true
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
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        mut navigation_rebuild_requested: bool,
    ) -> Vec<OverlayEntry> {
        let mut overlay_updates = Vec::new();
        let overlay_ready =
            rules.is_some() && self.resolved_terrain.is_some() && overlay_registry.is_some();
        if overlay_ready && let Some(grid) = self.overlay_grid.as_mut() {
            self.frame_overlay_removals = grid.take_removed_render_cells();
            let (dirty_cells, synchronous_passability_changed) =
                grid.take_dirty_cells_with_passability_signal();
            let synchronous_navigation_cells = grid.take_synchronous_navigation_cells();
            navigation_rebuild_requested |=
                synchronous_passability_changed || !synchronous_navigation_cells.is_empty();

            let terrain = self
                .resolved_terrain
                .as_mut()
                .expect("overlay-ready terrain");
            let registry = overlay_registry.expect("overlay-ready registry");
            for &(rx, ry) in &dirty_cells {
                navigation_rebuild_requested |=
                    recalc_overlay_passability(grid, terrain, registry, rx, ry);
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

        if navigation_rebuild_requested && let Some(rules) = rules {
            let _ = self.rebuild_dynamic_navigation(rules);
        }
        overlay_updates
    }

    /// Rebuild the zone connectivity map from the current PathGrid and terrain costs.
    /// Call after the PathGrid has been rebuilt so that zones reflect the latest
    /// walkability state.
    ///
    /// Tries an incremental update first (diffing against the previous PathGrid).
    /// Falls back to full rebuild if too many cells changed or no previous state.
    #[cfg(test)]
    pub fn rebuild_zone_grid(&mut self, path_grid: &PathGrid) {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        navigation::NavigationCaches {
            terrain_costs: &mut self.terrain_costs,
            zones: &mut self.zone_grid,
            path: &mut self.path_grid,
            playfield_bounds: self.playfield_bounds,
        }
        .rebuild_zones(path_grid, terrain, self.bridge_state.as_ref());
    }

    /// Rebuild without the PathGrid-only incremental shortcut. Reduced zone
    /// type can change while boolean walkability stays identical (notably a
    /// live OccupationBits=0 terrain object changing Building to Ground).
    fn rebuild_zone_grid_full(&mut self, path_grid: &PathGrid) {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        navigation::NavigationCaches {
            terrain_costs: &mut self.terrain_costs,
            zones: &mut self.zone_grid,
            path: &mut self.path_grid,
            playfield_bounds: self.playfield_bounds,
        }
        .rebuild_zones_full(path_grid, terrain, self.bridge_state.as_ref());
    }

    /// Consume terrain/overlay receipts at their existing world-reader boundary.
    /// VERA-internal projection protocol, gamemd equivalent UNCHECKED. A pinned
    /// pre-callback grid is only a fallback: bridge and wall callbacks may have
    /// already published newer canonical navigation. Never rebuild over it from
    /// a stale reader snapshot. The returned projection serves subsequent phases.
    fn finish_terrain_navigation_changes(
        &mut self,
        fallback_path_grid: Option<&PathGrid>,
        terrain_changed_cells: &[(u16, u16)],
    ) -> Option<Arc<PathGrid>> {
        let mut changed_cells = self
            .overlay_grid
            .as_mut()
            .map(|grid| grid.take_synchronous_navigation_cells())
            .unwrap_or_default();
        for &cell in terrain_changed_cells {
            if !changed_cells.contains(&cell) {
                changed_cells.push(cell);
            }
        }
        let canonical = self.path_grid_snapshot();
        if changed_cells.is_empty() {
            return canonical.or_else(|| fallback_path_grid.cloned().map(Arc::new));
        }
        let current = canonical.as_deref().or(fallback_path_grid);
        self.refresh_navigation_after_terrain_changes(current, &changed_cells)?;
        self.path_grid_snapshot()
    }

    /// Refresh navigation authority after inline overlay mutation or terrain
    /// object destruction. The incoming grid carries dynamic structure and
    /// wall blockers, so only synchronously changed cells are replaced from the
    /// current resolved-terrain/bridge projection.
    fn refresh_navigation_after_terrain_changes(
        &mut self,
        input_path_grid: Option<&PathGrid>,
        changed_cells: &[(u16, u16)],
    ) -> Option<PathGrid> {
        let (resolved_path_grid, terrain_costs) = {
            let terrain = self.resolved_terrain.as_ref()?;
            (
                PathGrid::from_resolved_terrain_with_bridges(terrain, self.bridge_state.as_ref()),
                build_canonical_terrain_cost_grids(terrain),
            )
        };

        self.terrain_costs = terrain_costs;
        let mut tail_path_grid = input_path_grid
            .filter(|grid| {
                grid.width() == resolved_path_grid.width()
                    && grid.height() == resolved_path_grid.height()
            })
            .cloned()?;
        for &(rx, ry) in changed_cells {
            let replaced = tail_path_grid.replace_cell_from(&resolved_path_grid, rx, ry);
            debug_assert!(replaced, "changed terrain cell must be inside the map");
        }
        self.rebuild_zone_grid_full(&tail_path_grid);
        Some(tail_path_grid)
    }

    pub(crate) fn effective_build_blocked(&self, rx: u16, ry: u16) -> Option<bool> {
        let terrain = self.resolved_terrain.as_ref()?;
        let cell = terrain.cell(rx, ry)?;
        if cell.bridge_facts.has_flag(BRIDGE_FLAG_STRUCTURAL)
            || cell.bridge_facts.has_flag(BRIDGE_FLAG_DESTROYED_OR_RAMP)
            || cell.overlay_blocks
            || cell.terrain_object_blocks
            || cell.slope_type != 0
        {
            return Some(true);
        }
        if let Some(bridge) = self
            .bridge_state
            .as_ref()
            .and_then(|state| state.cell(rx, ry))
        {
            return Some(if matches!(bridge.damage_state, DamageState::Destroyed) {
                cell.base_build_blocked
            } else {
                true
            });
        }
        Some(cell.build_blocked)
    }

    /// Apply combat-emitted wall damage events: drives the per-cell damage
    /// progression in `damage_wall_overlay`, runs the cardinal-neighbor cleanup
    /// for any cells the damage destroys.
    ///
    /// `overlay_registry` supplies per-overlay-type Strength/DamageLevels.
    pub(crate) fn apply_wall_damage_events(
        &mut self,
        events: &[WallDamageEvent],
        overlay_registry: &crate::map::overlay_types::OverlayTypeRegistry,
    ) {
        if events.is_empty() {
            return;
        }
        let Some(grid) = self.overlay_grid.as_mut() else {
            return;
        };
        #[cfg(test)]
        let mut cell_target_detaches = Vec::new();
        let mut host = SimulationWallRuntimeHost {
            entities: &mut self.substrate.entities,
            #[cfg(test)]
            detach_trace: &mut cell_target_detaches,
            radar_dirty_cells: &mut self.radar_terrain_dirty_cells,
            radar_dirty_generation: &mut self.radar_terrain_dirty_generation,
            tactical_dirty_cells: &mut self.tactical_dirty_cells,
            terrain_costs: &mut self.terrain_costs,
            zone_grid: &mut self.zone_grid,
            path_grid: &mut self.path_grid,
            bridge_state: self.bridge_state.as_ref(),
            playfield_bounds: self.playfield_bounds,
        };
        for event in events {
            let _ = damage_wall_overlay_with_runtime_host(
                grid,
                overlay_registry,
                self.resolved_terrain.as_mut(),
                event.rx,
                event.ry,
                event.damage,
                &mut self.scenario_rng,
                Some(&mut host),
            );
        }
    }

    /// Movement-side wall crush: a `Crusher=yes` drive vehicle that finishes the
    /// ground-movement stage standing on a `Wall=yes` overlay cell flattens that
    /// wall outright. This mirrors gamemd's per-cell-process wall crush — a
    /// forced, instant overlay removal — which is a SEPARATE path from the
    /// probabilistic weapon/warhead wall damage (the crush deals no unit damage
    /// and skips the Strength dice roll).
    ///
    /// The gate is `UnitClass::PerCellProcess @ 0x0073AFD4..B074`: the
    /// `Crusher=` flag (not `OmniCrusher=`, which governs unit-vs-unit crushing)
    /// over a `Crushable=` overlay, or over a `Wall=` overlay with the Drive
    /// locomotor. Fences and sandbags (`Crushable=yes`) fall to any crusher that
    /// reaches them; concrete walls only to a Drive crusher, and in stock YR only
    /// the Battle Fortress routes *through* those (`MovementZone=CrusherAll`).
    /// A crusher can only occupy an intact wall cell on the tick it enters, and
    /// the wall is removed that same tick, so this self-limits to one
    /// destruction per wall with no per-tick re-fire.
    ///
    /// Runs immediately after Phase-1 ground movement, before vision, rather
    /// than inside the per-cell entry callback (recorded ordering difference).
    /// Reuses the shared `apply_wall_damage_events` path so overlay clear,
    /// cardinal neighbor connectivity cleanup, and chain reaction match weapon
    /// damage, and queues the overlay type's `CrushSound=` at the crusher.
    ///
    /// Residuals: the `RockingForwardsPerFrame += 0.02` tilt (`0x0073B05B`) has
    /// no rendering consumer in VERA and is not written; the crush-clamp byte
    /// `Foot+0x6B5` that `0x0073B067` clears is not modelled at all; the CRUSHER
    /// weapon ability arm is not modelled (no stock type grants it).
    pub(crate) fn apply_wall_crush_on_driveover(
        &mut self,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) {
        let (Some(_rules), Some(registry)) = (rules, overlay_registry) else {
            return;
        };
        let Some(grid) = self.overlay_grid.as_ref() else {
            return;
        };

        // Phase 1 (read-only): collect every distinct wall cell currently
        // occupied by an active Crusher drive vehicle. Sorted entity iteration
        // keeps this deterministic; the per-cell dedup means two crushers on one
        // cell emit a single forced-destruction event.
        let mut events: Vec<WallDamageEvent> = Vec::new();
        let mut sounds: Vec<SimSoundEvent> = Vec::new();
        let mut seen: BTreeSet<(u16, u16)> = BTreeSet::new();
        for (_id, e) in self.substrate.entities.iter_sorted() {
            // `0x0073AFEB..B000`: `Crusher=` (`TechnoType+0xD28`) or the CRUSHER
            // weapon ability (`HasWeaponAbility(0x11)`). No stock type grants
            // the ability through `VeteranAbilities=`/`EliteAbilities=`, so only
            // the flag is modelled here.
            if !e.regular_crusher || !e.is_active() {
                continue;
            }
            let (rx, ry) = (e.position.rx, e.position.ry);
            let Some(flags) = grid
                .cell(rx, ry)
                .overlay_id
                .and_then(|oid| registry.flags(oid))
            else {
                continue;
            };
            // `0x0073B013..B034`: a `Crushable=` overlay is crushed by any
            // crusher; a `Wall=` overlay additionally needs
            // `MovementZone=CrusherAll`.
            //
            // `0x0073B027` loads the type pointer from `+0x6C4` and `0x0073B02D`
            // compares `TechnoTypeClass+0x5B4` against `0xC`, `JNZ` out. That
            // field is **MovementZone**, not the locomotor: `ReadINI 0x0071605E`
            // reads the key "MovementZone" (`0x008431C8`) through
            // `CCINIClass::ReadMovementZone 0x00474E40` against the name table at
            // `0x0081BA88`, where index 12 is `CrusherAll`. This gate used to
            // test `LocomotorKind::Drive` while its own comment named `+0x5B4`.
            //
            // Read from the entity's locomotor where native reads the type.
            // Equivalent because `piggyback` captures and restores
            // `movement_zone` across a temporary locomotor swap - which is
            // what the comment this replaced meant by "the primary kind".
            //
            // The difference is not academic: exactly **one** stock vehicle
            // carries `MovementZone=CrusherAll` - `BFRT`, the Battle Fortress -
            // where `Crusher=yes` covers 29. So every crusher tank was flattening
            // walls by driving over them, and retail lets only the Battle
            // Fortress do it. `Crushable=` overlays (sandbags, fences) are
            // unaffected: they fall to any crusher through the first clause.
            let crusher_all = e.locomotor.as_ref().is_some_and(|l| {
                l.movement_zone == crate::rules::locomotor_type::MovementZone::CrusherAll
            });
            if !(flags.crushable || (flags.wall && crusher_all)) {
                continue;
            }
            // `CellClass::DestroyOverlay @ 0x00480CB0` acts only on a `Wall=`
            // overlay; the sound cue precedes it and does not depend on it.
            // This sweep runs every frame rather than once per cell entry, so a
            // non-wall crushable overlay would re-fire its cue each frame; no
            // stock overlay carries `Crushable=yes` without `Wall=yes` and a
            // sound, so the cue is gated on the wall removal here (recorded).
            if flags.wall && seen.insert((rx, ry)) {
                if let Some(sound_id) = flags.crush_sound.as_ref() {
                    // `0x0073B045..B04D`: the overlay type's `CrushSound=` at
                    // the crusher's own coordinates.
                    sounds.push(SimSoundEvent::wall_crushed(sound_id.clone(), &e.position));
                }
                // damage == -1 = forced instant removal, bypassing the
                // probabilistic Strength gate the weapon path uses.
                events.push(WallDamageEvent { rx, ry, damage: -1 });
            }
        }
        self.sound_events.extend(sounds);

        if events.is_empty() {
            return;
        }

        // Phase 2 (mutating): shared teardown, identical to the combat wall path.
        self.apply_wall_damage_events(&events, registry);
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
    fn collect_spy_sat_candidate_owners(&self, rules: &RuleSet) -> BTreeSet<InternedId> {
        let selling =
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Selling);
        let mut active = BTreeSet::new();
        let mut scanned_houses = BTreeSet::new();
        for &owner in self.session.house_order.iter().chain(self.houses.keys()) {
            if !self.houses.contains_key(&owner) || !scanned_houses.insert(owner) {
                continue;
            }
            for &stable_id in self.substrate.entities.ids_for_owner(owner) {
                let Some(entity) = self.substrate.entities.get(stable_id) else {
                    continue;
                };
                let coarse_candidate = entity.category == EntityCategory::Structure
                    && !entity.lifecycle.in_limbo
                    && entity.lifecycle.cell_marked
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
            for (&viewer, deposit) in &entity.gap_generator.viewers {
                if !deposit.active {
                    continue;
                }
                effects.gap_generators.entry(viewer).or_default().push(
                    vision::GapGeneratorSource {
                        stable_id: entity.stable_id(),
                        owner: entity.owner(),
                        rx: entity.position.rx,
                        ry: entity.position.ry,
                        radius: deposit.radius,
                    },
                );
            }
        }
        effects
    }

    fn apply_active_vision_structures(&mut self, effects: &ActiveVisionStructures) {
        vision::materialize_gap_generator_sources(
            &mut self.fog,
            &effects.gap_generators,
            &self.interner,
        );
    }

    /// Commit aggregate SpySat transitions at the House update rung, then
    /// materialize the House-rung SpySat -> Gap result. EventClass commands
    /// execute later and therefore become visible to this scan next frame.
    fn reconcile_active_vision_structures(&mut self, rules: &RuleSet) {
        let active_owners = self.collect_spy_sat_candidate_owners(rules);
        let transitions: Vec<_> = self
            .houses
            .iter()
            .filter_map(|(&owner, house)| {
                let active = active_owners.contains(&owner);
                (house.spy_sat_active != active).then_some((owner, house.spy_sat_active, active))
            })
            .collect();
        for (owner, old_active, active) in transitions {
            let cells = self.resolved_terrain.as_ref().map_or_else(
                || self.fog.rectangular_cells(),
                |terrain| terrain.iter().map(|cell| (cell.rx, cell.ry)).collect(),
            );
            //509064/509012 call Map while577A still has its OLD value; callback
            //gap removal reads577A, not the separate MapIsClear byte241.
            //4ADEE0/4ADCD0 do not release allied mobile sight. The represented
            //multiplayer known-own branch and stock AllyReveal=yes directional
            //allied Building arm
            //select callbacks; untouched admissions survive the bulk counter
            //overwrite. Independent41B discovery, campaign Human and the
            //RevealToAll alternate-recipient branch remain caller residuals.
            let sources = self
                .substrate
                .entities
                .iter_sorted()
                .filter_map(|(id, entity)| {
                    if !entity.lifecycle.object_alive || entity.lifecycle.in_limbo {
                        return None;
                    }
                    let own = entity.owner() == owner;
                    let allied_building = entity.category == EntityCategory::Structure
                        && crate::map::houses::is_allied_with(
                            &self.house_alliances,
                            self.interner.resolve(entity.owner()),
                            self.interner.resolve(owner),
                        );
                    (own || allied_building).then_some(id)
                })
                .collect();
            //Map240's activation guard precedes all callbacks; explicit reset
            //has no idempotence guard. Re-admission scans live candidates and
            //rechecks current operational state independently for this viewer.
            if self.fog.width != 0
                && self.fog.height != 0
                && (!active || !self.fog.whole_map_revealed_owners.contains(&owner))
            {
                let (gaps, admitted_gap) = self.prepare_spy_sat_gap_reentry(owner, rules);
                self.fog.transition_whole_map_with_gap_reentry(
                    owner,
                    cells,
                    !active,
                    old_active,
                    &sources,
                    Some(&gaps),
                );
                //577D90 sets240 before callbacks; every successful6FB170
                //re-admission clears it again, including a friendly generator.
                if admitted_gap {
                    self.fog.whole_map_revealed_owners.remove(&owner);
                }
            }
            if let Some(house) = self.houses.get_mut(&owner) {
                house.map_is_clear = active;
                house.spy_sat_active = active;
            }
        }
        let effects = self.collect_active_vision_structures(rules);
        self.apply_active_vision_structures(&effects);
    }

    /// Visit the live House registry for the early House-update mechanisms.
    ///
    /// gamemd-derived: `LogicClass__PerTickUpdate @ 0x0055AFB0`, House loop
    /// `0x0055B68D..0x0055B6B1`, walks the House array forward, skips nulls,
    /// and reloads the live count after every `HouseClass__Update @ 0x004F8440`
    /// call. The bounded Rust mechanisms are anger decay before the verified
    /// AI-activation block `0x004F8564..0x004F85B7`.
    fn update_houses_anger_and_activation(&mut self, rules: Option<&RuleSet>) {
        let current_frame = self.session.binary_frame as i32;
        let game_mode_nonzero = self.session.game_mode_nonzero;
        let iq_production = rules.map(|rules| rules.general.iq_production);
        let mut index = 0;
        while index < self.session.house_order.len() {
            let owner = self.session.house_order[index];
            let represented = self.houses.contains_key(&owner);
            if let Some(house) = self.houses.get_mut(&owner) {
                house_strategy::decay_anger_scores(house, &self.session.house_order, current_frame);
            }
            if represented {
                #[cfg(test)]
                self.trace_house_ai_activation_order(
                    HouseAiActivationOrderTestEvent::HouseAngerDecay(owner),
                );
                if let Some(iq_production) = iq_production {
                    self.houses
                        .get_mut(&owner)
                        .expect("represented House remains registered during its update")
                        .update_ai_activation(game_mode_nonzero, iq_production);
                    #[cfg(test)]
                    self.trace_house_ai_activation_order(
                        HouseAiActivationOrderTestEvent::HouseActivation(owner),
                    );
                }

                #[cfg(test)]
                if self
                    .house_update_append_after_test
                    .is_some_and(|(after_owner, _)| after_owner == owner)
                {
                    let (_, appended_owner) = self
                        .house_update_append_after_test
                        .take()
                        .expect("append injection was just matched");
                    self.session.house_order.push(appended_owner);
                }
            }
            index += 1;
        }
    }

    fn refresh_fog(
        &mut self,
        path_grid: Option<&PathGrid>,
        config: &vision::VisionConfig,
        rules: Option<&RuleSet>,
    ) {
        // Recompute visibility in-place: clears FLAG_VISIBLE on existing grids
        // (preserving FLAG_REVEALED) then re-reveals from entity positions.
        // No allocation or merge_revealed_from pass needed.
        let height_grid = if config.reveal_by_height {
            path_grid.map(PathGrid::ground_height_grid)
        } else {
            None
        };

        vision::recompute_owner_visibility_in_place(
            &mut self.fog,
            &self.substrate.entities,
            path_grid,
            &self.house_alliances,
            config,
            height_grid.as_deref(),
            &self.interner,
            rules,
        );

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
                entry.1 = entry.1.min(entity.position.rx);
                entry.2 = entry.2.min(entity.position.ry);
                entry.3 = entry.3.max(entity.position.rx);
                entry.4 = entry.4.max(entity.position.ry);
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
    fn announce_super_weapon_building_complete(&mut self, stable_id: u64, rules: &RuleSet) {
        let Some((owner, type_ref)) = self
            .substrate
            .entities
            .get(stable_id)
            .filter(|e| e.category == EntityCategory::Structure && !e.dying)
            .map(|e| (e.owner(), e.type_ref()))
        else {
            return;
        };
        let Some(section) = rules
            .object(self.interner.resolve(type_ref))
            .and_then(|obj| obj.super_weapon.clone())
        else {
            return;
        };
        let sw_type = self.interner.intern(&section);
        self.sound_events
            .push(SimSoundEvent::SuperWeaponDetected { owner, sw_type });
    }

    /// Advance build-up animations and return completed building stable IDs.
    fn tick_building_up(&mut self) -> Vec<u64> {
        // Collect keys first to allow &mut iteration via get_mut().
        let keys = self.substrate.entities.keys_sorted();
        let mut finished: Vec<u64> = Vec::new();
        for &sid in &keys {
            if let Some(entity) = self.substrate.entities.get_mut(sid) {
                // Construction and deconstruction are the building's missions,
                // which hold while it is warped (`GameEntity::ai_frozen`).
                if entity.ai_frozen() {
                    continue;
                }
                if let Some(ref mut bu) = entity.building_up {
                    bu.elapsed_ticks = bu.elapsed_ticks.saturating_add(1);
                    if bu.elapsed_ticks >= bu.total_ticks {
                        finished.push(sid);
                    }
                }
            }
        }
        for &sid in &finished {
            if let Some(entity) = self.substrate.entities.get_mut(sid) {
                entity.building_up = None;
            }
        }
        finished
    }

    /// Advance building-down (undeploy) animations. When done, despawn the
    /// building and spawn the mobile unit (e.g., ConYard → MCV).
    /// Returns true if any entities were spawned (triggers atlas refresh).
    fn tick_building_down(
        &mut self,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        let keys = self.substrate.entities.keys_sorted();
        let mut finished: Vec<u64> = Vec::new();
        for &sid in &keys {
            if let Some(entity) = self.substrate.entities.get_mut(sid) {
                if entity.ai_frozen() {
                    continue;
                }
                if let Some(ref mut bd) = entity.building_down {
                    bd.elapsed_ticks = bd.elapsed_ticks.saturating_add(1);
                    if bd.elapsed_ticks >= bd.total_ticks {
                        finished.push(sid);
                    }
                }
            }
        }
        let any_finished = !finished.is_empty();
        for sid in finished {
            // Extract spawn data before despawning.
            let spawn_data = self.substrate.entities.get(sid).and_then(|e| {
                e.building_down.as_ref().map(|bd| {
                    (
                        bd.spawn_type,
                        bd.spawn_owner,
                        bd.spawn_rx,
                        bd.spawn_ry,
                        bd.spawn_z,
                        bd.was_selected,
                    )
                })
            });
            let Some((unit_type_id, owner_id, rx, ry, z, was_selected)) = spawn_data else {
                continue;
            };
            // Building449E66/70 captures current health/type ratio at actual
            // conversion, not when the reverse animation was requested.
            let converted_health = if let Some(rules) = rules {
                let Some(health) = self.substrate.entities.get(sid).and_then(|source| {
                    Some(crate::sim::conversion_health::ConversionHealth::capture(
                        source,
                        self.object_type(source.type_ref(), rules)?,
                        rules.object(self.interner.resolve(unit_type_id))?,
                        crate::sim::conversion_health::ConversionKind::Building,
                    ))
                }) else {
                    // A missing live type cannot supply a conversion ratio.
                    continue;
                };
                Some(health)
            } else {
                None
            };
            let rules = match rules {
                Some(rules) => {
                    self.uninit_with_rules(sid, rules);
                    rules
                }
                None => {
                    self.uninit(sid);
                    continue;
                }
            };
            let unit_type_str = self.interner.resolve(unit_type_id).to_string();
            let owner_str = self.interner.resolve(owner_id).to_string();
            if let Some(new_sid) = self.spawn_object_at_height_with_overlay_context(
                &unit_type_str,
                &owner_str,
                rx,
                ry,
                0,
                z,
                rules,
                overlay_registry,
            ) {
                if let Some(ge) = self.substrate.entities.get_mut(new_sid) {
                    converted_health
                        .expect("conversion with rules captured health")
                        .apply(ge);
                    ge.selected = was_selected;
                }
            }
        }
        any_finished
    }

    /// Spine region (LATE): AI commands, defeat detection, building animations,
    /// radar aging, and the late frame/tick commit. Accumulates
    /// `spawned_entities` (AI placements + undeploy spawns). Returns false when
    /// the terminating call skips frame commit and pending-delete processing.
    fn run_late_region(
        &mut self,
        commands: &[CommandEnvelope],
        rules: Option<&RuleSet>,
        path_grid: Option<&PathGrid>,
        height_map: &BTreeMap<(u16, u16), u8>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        tick_ms: u32,
        execute_tick: u64,
        executed_commands: &mut usize,
        spawned_entities: &mut bool,
        destroyed_structure: &mut bool,
        placed_building_owners: &mut Vec<InternedId>,
    ) -> bool {
        #[cfg(test)]
        self.trace_master_frame_rung(MasterFrameTestRung::Houses);
        if let Some(rules) = rules {
            self.reconcile_active_vision_structures(rules);
        }
        // Factory/production work has already completed in Phase 7. This early
        // House update follows optional vision reconciliation and precedes both
        // defeat processing and strategic AI command generation. Native anger
        // decay is unconditional; only the activation substep needs RuleSet.
        self.update_houses_anger_and_activation(rules);
        // --- Phase 8: Defeat detection (runs BEFORE AI) ---
        // gamemd evaluates each house's defeat before its AI manage/produce step,
        // so a defeated house issues no AI command this tick; tick_ai skips any
        // house flagged defeated via its is_defeated gate. The gate reads the
        // house's tracking counts (`house_defeat.rs`), which construction and
        // the frame-end pending-delete drain move: a death reaches the gate on
        // the next frame.
        if self.session.tick > 0 {
            self.check_defeat(rules, overlay_registry);
            #[cfg(test)]
            self.trace_house_ai_activation_order(HouseAiActivationOrderTestEvent::DefeatProcessed);
        }

        // --- Phase 8 (cont.): AI ---
        // DEPENDS ON: all prior phases + the defeat status set just above (defeated
        // houses are gated out inside tick_ai).
        // PRODUCES: commands applied immediately in the same tick.
        // Temporarily take ai_players out to avoid borrow conflict with &self.
        if rules.is_some() && !self.ai_players.is_empty() {
            let mut ai_state = std::mem::take(&mut self.ai_players);
            let ai_commands = ai::tick_ai(
                self,
                &mut ai_state,
                rules.expect("rules checked above"),
                path_grid,
                height_map,
                overlay_registry,
            );
            #[cfg(test)]
            self.trace_house_ai_activation_order(HouseAiActivationOrderTestEvent::AiGenerated);
            self.ai_players = ai_state;
            let mut ai_tail_path_grid = path_grid
                .cloned()
                .or_else(|| self.path_grid.as_deref().cloned());
            for cmd in &ai_commands {
                let cmd_owner_str = self.interner.resolve(cmd.owner).to_string();
                let applied = self.apply_command_with_overlays(
                    &cmd_owner_str,
                    &cmd.payload,
                    rules,
                    ai_tail_path_grid.as_ref(),
                    height_map,
                    overlay_registry,
                );
                if applied && self.is_wall_placement_command(&cmd.payload, rules) {
                    ai_tail_path_grid = self.path_grid.as_deref().cloned().or(ai_tail_path_grid);
                }
                let placed_owner = self.successful_non_wall_placement_owner(cmd, applied, rules);
                placed_building_owners.extend(placed_owner);
                if (applied
                    && matches!(cmd.payload, Command::DeployMcv { entity_id }
                    if self.substrate.entities.get(entity_id).is_none_or(|e| e.dying)))
                    || placed_owner.is_some()
                    || applied
                        && matches!(
                            cmd.payload,
                            Command::UndeployBuilding { .. } | Command::LaunchSuperWeapon { .. }
                        )
                {
                    *spawned_entities = true;
                }
            }
        }

        // --- Phase 9: Building animations + cleanup ---
        // DEPENDS ON: production (newly placed buildings start build-up).
        let completed_buildings = self.tick_building_up();
        if let Some(rules) = rules {
            for &stable_id in &completed_buildings {
                self.initialize_completed_building_anims(stable_id, rules);
                self.allocate_building_light(stable_id, rules);
                self.add_building_sensor_array_if_powered(stable_id, rules);
                self.announce_super_weapon_building_complete(stable_id, rules);
            }
            *spawned_entities |= production::spawn_completed_refinery_free_units(
                self,
                &completed_buildings,
                rules,
                path_grid,
                height_map,
                overlay_registry,
            );
        }
        // Advance building-down (undeploy) animations; spawn units when done.
        *spawned_entities |= self.tick_building_down(rules, overlay_registry);

        // EventClass dispatch is a Main_Tick tail rung: the complete live
        // Logic walk observes frame N's pre-command state, so an accepted
        // command first changes that object's AI behavior on frame N+1.
        let (executed, spawned, destroyed, placed_owners) = self.apply_due_commands(
            commands,
            rules,
            path_grid,
            height_map,
            execute_tick,
            overlay_registry,
        );
        *executed_commands += executed;
        *spawned_entities |= spawned;
        *destroyed_structure |= destroyed;
        placed_building_owners.extend(placed_owners);

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
        #[cfg(test)]
        self.trace_master_frame_rung(MasterFrameTestRung::FrameCommit);
        #[cfg(test)]
        self.trace_lifecycle_for_test(LifecycleTestEvent::BinaryFrameCommitted);

        // The one ordinary ProcessPendingDelete drain follows the native frame
        // commit. Alive queue entries keep their position; ready duplicate IDs
        // collapse and physically finalize exactly once.
        #[cfg(test)]
        self.trace_master_frame_rung(MasterFrameTestRung::PendingDelete);
        self.process_pending_delete();

        // Original55DE9F calls725C70 at this admitted late-frame boundary.
        // Stock bridge Overlay objects publish only Cell state; their isolated
        // destructor has no gameplay-object callback effects and cannot allocate
        // IDs. Drain the shared authored/runtime owner after gameplay objects.
        self.load_objects
            .drain_deferred()
            .expect("live Overlay deferred queue must contain its owned objects");

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
    fn run_team_script_pass(&mut self, rules: Option<&RuleSet>) {
        #[cfg(test)]
        self.trace_master_frame_rung(MasterFrameTestRung::TeamScript);
        let mut team_script_vm = std::mem::take(&mut self.team_script_vm);
        let team_tick = team_script_vm.tick_effects(self.session.binary_frame as i32, |owner| {
            !crate::sim::house_state::house_state_for_owner_id(&self.houses, owner)
                .is_some_and(|house| house.is_defeated)
        });
        self.team_script_vm = team_script_vm;
        for effect in team_tick.effects {
            match effect {
                // Original: TeamClass::AI action 19 walks TeamClass+0x54 and
                // invokes FootClass's panic-family virtual in member order.
                TeamScriptEffect::PanicMember { entity_id } => {
                    let Some(rules) = rules else { continue };
                    let Some(type_ref) = self
                        .substrate
                        .entities
                        .get(entity_id)
                        .map(|entity| entity.type_ref())
                    else {
                        continue;
                    };
                    let Some(object_type) = rules.object(self.interner.resolve(type_ref)) else {
                        continue;
                    };
                    if let Some(entity) = self.substrate.entities.get_mut(entity_id) {
                        crate::sim::infantry::apply_panic_force(object_type, entity);
                    }
                }
            }
        }
    }

    /// Install fixed AIMD plus scenario AI definitions after RuleSet identities
    /// are interned and before the first gameplay tick. This is data ingress
    /// only: the installed VM contains no live TeamClass instances.
    pub(crate) fn install_team_ai_registry(
        &mut self,
        registry: &crate::rules::team_ai_ini::TeamAiIniRegistry,
        rules: &RuleSet,
    ) -> Vec<crate::sim::team_script_vm::TeamAiInstallDiagnostic> {
        let (vm, diagnostics) =
            TeamScriptVm::from_ini_registry(registry, &mut self.interner, rules);
        if !diagnostics
            .iter()
            .any(crate::sim::team_script_vm::TeamAiInstallDiagnostic::is_fixed_source_refusal)
        {
            self.team_script_vm = vm;
        }
        diagnostics
    }

    /// `DriveLocomotionClass::Process` (0x004B0823 region; ships share the
    /// drive locomotor's process, hover runs the same test in its `Move` at
    /// 0x00514AC3) spawns `Rules->Wake` (Rules+0x94) when `Is_Moving_Now`
    /// (vtable +0x80) holds, `g_CurrentFrameCounter % 10 == 0`, the unit is
    /// not on a bridge (`+0x8C`), and its cell's `CellClass+0xEC` land type is
    /// 2 (Water). The anim goes at the unit's exact `PositionCoord`
    /// (+0xA0/+0xA4), not the cell centre, so each wake stays where the hull
    /// was and the trail forms behind it as the unit advances. The earlier
    /// form here spawned every 8 frames at the cell centre for any unit with
    /// a movement target, which put the foam mid-hull and under stationary
    /// ships.
    pub(crate) fn spawn_wakes_for_frame(&mut self, rules: &RuleSet) {
        if self.session.binary_frame % 10 != 0 {
            return;
        }
        let binary_frame = self.session.binary_frame;
        let terrain = self.resolved_terrain.as_ref();
        let wake_positions: Vec<(u16, u16, SimFixed, SimFixed, u8)> = self
            .substrate
            .entities
            .keys_sorted()
            .iter()
            .filter_map(|id| {
                wake_anchor_for(self.substrate.entities.get(*id)?, terrain, binary_frame)
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

    /// Fixture-only frame adapter (F09): unit tests drive one Main_Tick-shaped
    /// frame with explicitly supplied rules/heights/navigation. Production and
    /// tooling advance exclusively through `SimRuntime::advance_frame`, whose
    /// resources are bound at construction and cannot be substituted per call.
    #[cfg(test)]
    pub(crate) fn advance_tick(
        &mut self,
        commands: &[CommandEnvelope],
        rules: Option<&RuleSet>,
        height_map: &BTreeMap<(u16, u16), u8>,
        path_grid: Option<&PathGrid>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        tick_ms: u32,
    ) -> TickResult {
        self.advance_master_frame(
            commands,
            rules,
            height_map,
            path_grid,
            overlay_registry,
            tick_ms,
            TickLane::Ordinary,
            None,
        )
        .expect("fixture frame must complete")
    }

    /// App-facing authoritative frame transaction.
    ///
    /// The app submits commands and immutable map/rules inputs, then consumes
    /// the returned facts instead of reaching back into Simulation-owned
    /// transient queues. `SimRuntime::advance_frame` is the sole production
    /// caller (F09); headless and replay execution go through the runtime.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn advance_app_frame(
        &mut self,
        commands: &[CommandEnvelope],
        rules: Option<&RuleSet>,
        height_map: &BTreeMap<(u16, u16), u8>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        tick_ms: u32,
        lane: TickLane,
        trigger_inputs: Option<TriggerInputs<'_>>,
    ) -> Result<SimFrameOutput, FrameAdvanceError> {
        let path_grid = self.path_grid_snapshot();
        let tick = self.advance_master_frame(
            commands,
            rules,
            height_map,
            path_grid.as_deref(),
            overlay_registry,
            tick_ms,
            lane,
            trigger_inputs,
        )?;
        Ok(self.collect_frame_output(tick))
    }

    fn collect_frame_output(&mut self, tick: TickResult) -> SimFrameOutput {
        self.flush_radiation_lighting();
        let lighting_events = std::mem::take(&mut self.lighting_sources.pending);
        let trigger_effects = std::mem::take(&mut self.trigger_effects);
        // Preserve the established terminal-frame gate: these are committed
        // light-vector facts and the next admitted frame clears the producer
        // buffer before combat runs.
        let invulnerability_impacts = if tick.frame_committed {
            std::mem::take(&mut self.invulnerability_impact_effects)
        } else {
            Vec::new()
        };
        let lifecycle_outputs = std::mem::take(&mut self.lifecycle_outputs);
        let overlay_updates = std::mem::take(&mut self.frame_overlay_updates);
        let overlay_removals = std::mem::take(&mut self.frame_overlay_removals);
        let fire_events = std::mem::take(&mut self.fire_events);
        let sound_events = std::mem::take(&mut self.sound_events);
        SimFrameOutput {
            tick,
            trigger_effects,
            lifecycle_outputs,
            overlay_updates,
            overlay_removals,
            sound_events,
            fire_events,
            invulnerability_impacts,
            lighting_events,
        }
    }

    /// Advance exactly one authoritative simulation frame.
    ///
    /// This is the sole Main_Tick-shaped entry point, reached in production
    /// only through `advance_app_frame` (F09). The fixture-only `advance_tick`
    /// adapter and the replay fixture runners are `#[cfg(test)]`; gameplay
    /// supplies its map definitions through `trigger_inputs`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn advance_master_frame(
        &mut self,
        commands: &[CommandEnvelope],
        rules: Option<&RuleSet>,
        height_map: &BTreeMap<(u16, u16), u8>,
        path_grid: Option<&PathGrid>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        tick_ms: u32,
        lane: TickLane,
        trigger_inputs: Option<TriggerInputs<'_>>,
    ) -> Result<TickResult, FrameAdvanceError> {
        self.invulnerability_impact_effects.clear();
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
        let mut placed_building_owners = Vec::new();
        let mut tail_path_grid: Option<Arc<PathGrid>> = None;
        // No command-boundary drain: command-applied deaths (sell, MCV/slave
        // deploy-undeploy, engineer capture) now stay in the Dying window like
        // combat deaths, freed only by the single end-of-tick drain — matching
        // gamemd's one ProcessPendingDelete at the tail of Main_Tick. The mid-
        // tick raw-store consumers (vision, power, production, movement, miner,
        // aircraft, …) are dying-gated, so a corpse is excluded until that drain.
        let mut bridge_state_changed = false;
        let mut passenger_ownership_changed = false;

        if lane == TickLane::Ordinary {
            executed_commands += self.apply_due_frame_ingress_commands(commands, execute_tick);
        }
        #[cfg(test)]
        self.trace_master_frame_rung(MasterFrameTestRung::SessionCommands);

        // MainTick55DBC8 precedes Logic55DC9E (including trigger polling).
        self.sort_display_ground(rules);

        // YR LogicClass::Update establishes trigger state before visiting the
        // live LogicVector, so object work in this frame observes its actions.
        #[cfg(test)]
        self.trace_master_frame_rung(MasterFrameTestRung::Triggers);
        if let Some(inputs) = trigger_inputs {
            self.poll_triggers_for_master_frame(inputs);
        }

        // Logic55B29A/55B2AD: consume prior pending conceal at the signed
        // pre-increment native frame boundary, before ore, Teams and live objects.
        self.fog
            .flush_pending_gap_conceal(self.session.binary_frame as i32);

        // Object-AI stage: the authoritative per-object Mission host, run
        // immediately BEFORE Phase-1 ground movement — gamemd decides each
        // object's mission, then moves it, within one pass. Each live object
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
            if self.session.game_options.super_weapons {
                crate::sim::superweapon::tick_active_superweapon_effects(
                    self,
                    rules,
                    overlay_registry,
                );
            }
            self.radiation.tick_decay(
                self.session.binary_frame,
                &rules.radiation,
                self.resolved_terrain.as_ref(),
            );
        }
        // Native TeamClass AI precedes the main LogicClass object vector. In
        // particular, ordinary object ReceiveDamage paths that arm a Team's
        // base-defense suspension occur only after this frame's Team visit.
        self.run_team_script_pass(rules);
        // The live pass commits each object's AI, movement and lifecycle effects
        // before advancing its cursor; later phases need only these outcomes.
        #[cfg(test)]
        self.trace_master_frame_rung(MasterFrameTestRung::LogicVector);
        // Receipts are frame-local: an aborted earlier frame must not leak one.
        self.aircraft_fire_requests.clear();
        let object_pass = self.advance_live_object_pass(rules, path_grid, overlay_registry)?;
        spawned_entities |= std::mem::take(&mut self.mission_spawned_entities);
        let movement_stats = object_pass.movement;
        destroyed_structure |= object_pass.destroyed_structure;
        bridge_state_changed |= object_pass.bridge_state_changed;
        let tube_turn_owned_ids = object_pass.tube_turn_owned_ids;
        if let Some(rules) = rules {
            self.for_each_multiplayer_feedback_anim(|sim, id| sim.visit_anim(id, rules, None));
        }
        // Spawn-manager missiles that reached their target during the movement
        // pass are consumed here — the missile leaves the world at the moment
        // `RocketLocomotion::Process` would have called Detonate. The impact
        // itself is queued for the combat phase below, which runs it through
        // the same damage → death → despawn pipeline as any other detonation.
        if rules.is_some() {
            if !self.pending_rocket_detonations.is_empty() {
                let detonated = std::mem::take(&mut self.pending_rocket_detonations);
                crate::sim::spawn_manager::detonate_missiles(self, &detonated);
            }
        } else {
            // No RuleSet means no spawner could have launched anything; drop
            // both queues rather than letting them accumulate across ticks that
            // never reach the combat phase.
            self.pending_rocket_detonations.clear();
            self.pending_missile_detonations.clear();
        }
        movement::sync_formation_speeds_after_live_pass(&mut self.substrate.entities);
        if let Some(rules) = rules {
            crate::sim::gate_runtime::tick_gate_runtimes(
                &mut self.substrate.entities,
                &self.substrate.occupancy,
                rules,
                &self.interner,
                self.session.binary_frame,
            );
            // Slice 7d: break each war-factory exit contact whose vehicle has cleared
            // the factory footprint this tick (gamemd's per-cell-process break).
            crate::sim::production::tick_war_factory_exit_contacts(
                &mut self.substrate.entities,
                &self.substrate.occupancy,
                rules,
                &self.interner,
            );
        }
        // Movement-side wall crush (part of the ground-movement stage): a Crusher
        // drive vehicle that ended Phase-1 on a wall cell flattens the wall,
        // separate from the weapon-damage wall path. No-op when no crusher sits
        // on a wall, so it is hash-neutral for every non-crush scenario.
        self.apply_wall_crush_on_driveover(rules, overlay_registry);
        let post_crush_path_grid = self.path_grid_snapshot();
        let active_path_grid = post_crush_path_grid.as_deref().or(path_grid);
        // --- Phase 2.5: body rocking ---
        // Drive/Ship slope sampling now belongs to locomotor Process entry,
        // before movement. Body rocking keeps its established post-movement
        // order, rules/terrain gate, and fallback-coefficient input
        // independently.
        if let (Some(rules), Some(_terrain)) = (rules, self.resolved_terrain.as_ref()) {
            let mut hook = crate::sim::rocking::self_destruct::NoopSelfDestruct;
            crate::sim::rocking::tick(&mut self.substrate.entities, rules, &mut hook);
        }

        // Aircraft missions ran in their own LogicVector slots during the live
        // pass; combat admits the state4 releases they requested.
        let aircraft_fire_requests = std::mem::take(&mut self.aircraft_fire_requests);

        // Wake anims under moving units on water (native gate and cadence in
        // `spawn_wakes_for_frame`).
        if let Some(rules) = rules {
            self.spawn_wakes_for_frame(rules);
        }

        // --- Phase 3: Vision refresh ---
        // DEPENDS ON: movement (positions updated), spawn (new entities need LOS).
        // PRODUCES: fog state used by combat targeting (phase 5).
        let vision_config = vision::VisionConfig {
            require_playfield_membership: self.playfield_bounds.is_some(),
            veteran_sight: rules.map_or(0.0, |r| r.general.veteran_sight),
            leptons_per_sight_increase: rules.map_or(0, |r| r.general.leptons_per_sight_increase),
            // Height-based LOS: terrain 4+ levels above the viewer at the
            // obstruction cell blocks sight (a unit at a cliff base can't see over
            // the cliff). Parity review verified the obstruction sampling against
            // the original (mirror table + the +2 offset); default on, as in YR.
            reveal_by_height: rules.map_or(true, |r| r.general.reveal_by_height),
            fog_of_war: self.session.game_options.fog_of_war,
        };
        self.refresh_fog(path_grid, &vision_config, rules);

        if let Some(rules) = rules {
            // --- Phase 4: Power ---
            // DEPENDS ON: entity health (damaged buildings produce less power).
            // PRODUCES: power_states used by combat (cloaking) and production (build speed).
            let _power_events = power_system::tick_power_states(
                &mut self.power_states,
                &mut self.substrate.entities,
                rules,
                &self.interner,
            );
            // --- Phase 4.1: HouseClass::Update EVA advice ---
            // DEPENDS ON: this tick's power totals and the house wallets.
            // PRODUCES: `SimSoundEvent::HouseEva` (funds nag / low power) and
            //   the per-house timer/guard state folded by the hash.
            crate::sim::house_eva::tick_house_eva(self, rules);
            // --- Phase 4.5: Superweapons ---
            // DEPENDS ON: power state (suspend/resume gating).
            // PRODUCES: AnimClass bolts and explosions, damage to entities, sound_events.
            if self.session.game_options.super_weapons {
                crate::sim::superweapon::tick_superweapon_instances(self, rules);
            }

            // --- Phase 4.6: Deploy/Undeploy state machine ---
            // DEPENDS ON: the prior frame's command tail
            //   (ToggleInfantryDeploy may have set Deploying/Undeploying).
            // PRODUCES: phase advances (Deploying→Deployed, Undeploying→None)
            //   that combat (Phase 5) and animation (post-tick) read this tick.
            crate::sim::deploy::tick_deploy_state(&mut self.substrate.entities);

            // Infantry fear decay and runtime prone transitions happen after
            // deploy state and before combat consumes the prone bit.
            crate::sim::infantry::tick_fear_for_entities(
                &mut self.substrate.entities,
                &self.houses,
                rules,
                &self.interner,
            );

            // Idle fidgets, immediately after the stance pass so a man who just
            // stood back up is not eligible on the same tick he was prone.
            // Driven from the logic vector, not the entity store: limboed
            // objects never reach this in the original.
            // DEPENDS ON: prone bit, deploy phase, attack target, mission.
            // PRODUCES: Idle1/Idle2 sequence switches, idle facing changes, and
            //   scenario-RNG draws — the one idle path that moves the cursor.
            crate::sim::infantry::tick_idle_actions(
                &mut self.substrate.entities,
                self.substrate.logic.as_slice(),
                &self.houses,
                rules,
                &self.interner,
                &mut self.scenario_rng,
                self.session.binary_frame,
            );

            // --- Phase 5: Combat + Turret rotation ---
            // DEPENDS ON: vision/fog (targeting uses fog state), power (cloaking).
            // Combat reads barrel.current(binary_frame) at the START of the tick
            // (matching gamemd's Fire_At_Target which uses last-frame facing).
            // tick_turret_rotation runs AFTER combat to drive rotation toward the
            // target for the NEXT frame's fire decision (matches Facing_Update order).
            // tick_c4_plants runs alongside tick_capture_orders — both convert
            // walk-up intent into a state change on arrival. Detonation damage
            // is applied here so combat-pre conditions (invulnerability, dying)
            // are honored before tick_combat runs.
            // PRODUCES: damage, deaths, bridge damage, fire events. Ordered
            // ReceiveDamage retaliation is committed inline; only legacy
            // precomputed damage producers can still write last_attacker_id.
            // Adjacent idle engineers receive an enter-cell order here.
            // Repair and consumption occur synchronously at Walk's completed
            // step in the object pass. The capture system excludes repair huts.
            let bridge_repaired = self.tick_bridge_repair_orders_with_overlay_registry(
                rules,
                overlay_registry,
                &tube_turn_owned_ids,
            );
            spawned_entities |= self.tick_capture_orders(rules, &tube_turn_owned_ids);
            let c4_outcome = self.tick_c4_plants_with_overlay_registry(
                rules,
                overlay_registry,
                &tube_turn_owned_ids,
            );
            destroyed_structure |= c4_outcome.destroyed_structure;
            bridge_state_changed |= bridge_repaired | c4_outcome.bridge_state_changed;
            self.tick_order_intents_pre_combat(rules, overlay_registry, &tube_turn_owned_ids);
            // Pursuit: walk units with out-of-range attack_target into range,
            // halt movement on range entry. Must run before combat so combat
            // sees the up-to-date movement_target this tick.
            self.tick_attack_pursuit_with_overlay_registry(
                rules,
                active_path_grid,
                overlay_registry,
                &tube_turn_owned_ids,
            );
            // LogicClass live-object order drives the firing/damage/kill-credit
            // resolution sequence. Snapshot is owned, so it does not conflict
            // with the &mut self.entities borrow below.
            let logic_order = self.live_object_order_snapshot();
            // BulletClass/WaveClass AI already ran at each object's mixed
            // LogicClass slot. Keep their established receiver boundary here.
            let projectile_detonations = std::mem::take(&mut self.pending_projectile_detonations);
            let sonic_damage_requests = std::mem::take(&mut self.pending_wave_damage_requests);
            // Rules-less fixture dispatch is the only producer of this
            // compatibility buffer. If a caller supplies Rules later in the
            // same frame, retain the live one-receiver-at-a-time contract.
            for request in sonic_damage_requests {
                self.commit_logic_wave_damage_request(rules, overlay_registry, &request);
            }
            let preexisting_wave_ids = self
                .waves
                .iter()
                .map(|(&stable_id, _)| stable_id)
                .collect::<BTreeSet<_>>();
            let fire_suppressed = tube_turn_owned_ids.clone();
            let combat_result = self.tick_combat_with_fatal_lifecycle(
                rules,
                overlay_registry,
                tick_ms,
                &logic_order,
                &fire_suppressed,
                &aircraft_fire_requests,
                &projectile_detonations,
                &[],
            );
            for projectile in combat_result.projectile_spawns.iter().copied() {
                let stable_id = self.allocate_stable_id();
                self.admit_projectile(stable_id, projectile);
            }
            self.visit_combat_appended_wave_tail(&preexisting_wave_ids, rules, overlay_registry);
            let post_combat_path_grid = self.path_grid_snapshot();
            let active_post_combat_path_grid =
                post_combat_path_grid.as_deref().or(active_path_grid);
            turret::tick_turret_rotation(
                &mut self.substrate.entities,
                rules,
                self.session.binary_frame,
                &self.interner,
            );
            // S3: Unit barrel destinations were computed per-object in the
            // combat Phase-2 window (pre-death state — a unit whose target died
            // this tick keeps aiming at it this tick; idle-return starts next
            // tick). This is the unchanged write point; tick_turret_rotation
            // above still skips Units (it owns Aircraft/Building barrels until
            // their slices land).
            crate::sim::world::unit_post::apply_unit_facing(
                &mut self.substrate.entities,
                &combat_result.unit_facing,
                rules,
                &self.interner,
                self.session.binary_frame,
            );
            // SpawnManager pass. Native dispatches it per object from
            // `TechnoClass::AI_Update` (+0x2D0 → vtable+0x5C), after that
            // object's Mission_Dispatch → Fire_At → SetTarget. Running it
            // immediately after the combat phase preserves that
            // "target set, then manager reads it" ordering within the tick;
            // the manager self-gates to every 10 frames regardless.
            let ordinary_logic_order = logic_order
                .iter()
                .copied()
                .filter(|id| !tube_turn_owned_ids.contains(id))
                .collect::<Vec<_>>();
            crate::sim::spawn_manager::tick_spawn_managers(
                self,
                rules,
                &ordinary_logic_order,
                overlay_registry,
            );
            let receipt = combat_result.consequences.commit(
                self,
                rules,
                overlay_registry,
                active_post_combat_path_grid,
            );
            destroyed_structure |= receipt.structure_destroyed;
            bridge_state_changed |= receipt.bridge_state_changed;
            tail_path_grid = receipt.path_grid;
            let post_terrain_path_grid = tail_path_grid.as_deref().or(active_post_combat_path_grid);

            // No end-of-Phase-5 drain: combat-killed structures/voxels stay in
            // the Dying window through the Phase 5.5-8.5 consumers and are freed
            // only by the single end-of-tick drain (gamemd's one ProcessPending-
            // Delete). Those consumers (production speed/factory-spawn scans,
            // repairs, retaliation, miner, aircraft) are dying-gated. Combat
            // post-processing above still reads the dead ids while resolvable
            // (count decrement, owner snapshot) — that runs before this point.
            // --- Phase 6: Legacy retaliation + Passengers ---
            // DEPENDS ON: non-receiver damage producers that still use the
            // transitional last_attacker_id handoff. Ordered area/direct
            // receiver hits already completed Mission Override inline.
            let phase_six_path_grid = post_terrain_path_grid;
            let logic_order = self.live_object_order_snapshot();
            combat::tick_retaliation(
                &mut self.substrate.entities,
                rules,
                &self.interner,
                &logic_order,
                self.resolved_terrain.as_ref(),
                Some(&self.house_alliances),
            );
            passenger_ownership_changed = passenger::tick_passenger_system(self, rules);
            self.tick_order_intents_post_combat_with_overlay_registry(
                phase_six_path_grid,
                Some(rules),
                overlay_registry,
                &tube_turn_owned_ids,
            );
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
                #[cfg(test)]
                self.trace_master_frame_rung(MasterFrameTestRung::CrateRegen);
                let regen = crate::sim::crates::tick_crate_regeneration(
                    self,
                    rules,
                    overlay_registry,
                    phase_six_path_grid,
                    self.scenario_normal_lighting,
                );
                if regen.visible != 0 {
                    // Native Mark mutates live CellClass land/zone/bridge state
                    // synchronously. Rust's BridgeRuntimeState is a derived
                    // cache, so refresh it once per pass that installed an
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

            // --- Phase 7: Production + Repairs + Docks + Ore ---
            // DEPENDS ON: combat (dead entities removed), movement (positions stable).
            // PRODUCES: new entities (spawned units), credit changes, ore growth.
            // Phase 7, FIRST production step — the authoritative factory sweep (C1:
            // factories step BEFORE the house tail `run_late_region`). The previous
            // tick's tail reconcile prepared the registry; `step_all` charges each armed
            // factory's per-step cost against the REAL wallet (house.economy.credits) in
            // insertion_seq (temporal) order; the spawn/placement pass below then
            // delivers completed builds and advances the queue-of-record.
            //
            // DRIFT (same-tick transaction ordering; repair lane's to fix):
            // `LogicClass::PerTickUpdate @ 0x0055AFB0` runs the object loop
            // first — every depot repair debit
            // (`BuildingClass::MissionRepairAndProduce @ 0x0044B780`) and every
            // building's own repair debit are spent inside that object's `AI`
            // visit — then Tactical, then a SEPARATE pass over
            // `g_FactoryClass_Array` at 0x0055B66A where each factory's
            // per-step charge (`FactoryClass::AI`) sees the wallet, then the
            // houses (see
            // docs/research/ADVANCE_TICK_PHASE_PARTITION_NATIVE_SPINE_GHIDRA_REPORT.md).
            // So within one frame EVERY repair/depot debit precedes EVERY
            // factory step natively; VERA charges every factory here first,
            // then `tick_repairs` and `tick_building_docks` below. Trigger: a house whose credits fall
            // below one factory step plus one repair step in the same frame.
            // Player effect: which of the two stalls for that frame differs.
            // Frequency: only while a player is nearly broke with both a
            // factory and a repair running. Downstream risk: credit trajectory
            // and stall cadence, no lifecycle or RNG effect.
            production::revalidate_and_step_factories(self, rules);
            spawned_entities |= production::tick_production_with_overlay_registry(
                self,
                rules,
                height_map,
                phase_six_path_grid,
                overlay_registry,
            );
            #[cfg(test)]
            self.trace_house_ai_activation_order(
                HouseAiActivationOrderTestEvent::ProductionCompleted,
            );
            production::tick_repairs(self, rules);
            building_dock::tick_building_docks(self, rules, phase_six_path_grid);
            crate::sim::docking::bunker_install::tick_bunker_install(
                self,
                rules,
                phase_six_path_grid,
            );
            aircraft_dock::tick_aircraft_docks(self, rules);
            if spawned_entities {
                self.refresh_fog(phase_six_path_grid, &vision_config, Some(rules));
            }
        }

        // ===== SPINE REGION: LATE — AI, defeat, anims, frame commit =====
        // (Step 3a skeleton: extracted to a region method; call order unchanged —
        // behavior-preserving.) Native-spine note: gamemd runs HouseClass updates
        // (incl. defeat) in the tail and commits the frame counter late; AI
        // placement is project-deferred and kept in its current slot.
        let late_path_grid = tail_path_grid.as_deref().or(path_grid);
        let frame_committed = self.run_late_region(
            if lane == TickLane::Ordinary {
                commands
            } else {
                &[]
            },
            rules,
            late_path_grid,
            height_map,
            overlay_registry,
            tick_ms,
            execute_tick,
            &mut executed_commands,
            &mut spawned_entities,
            &mut destroyed_structure,
            &mut placed_building_owners,
        );
        spawned_entities |= std::mem::take(&mut self.mission_spawned_entities);
        self.frame_overlay_updates = self.finalize_frame_overlays_and_navigation(
            rules,
            overlay_registry,
            destroyed_structure || bridge_state_changed || spawned_entities,
        );
        #[cfg(debug_assertions)]
        self.debug_assert_logic_membership_consistent();
        #[cfg(debug_assertions)]
        self.debug_assert_lifecycle_consistent();
        // Refresh the retained purifier-count projection before hashing.
        // Cash and factory state remain owned by their direct mutation paths.
        self.refresh_production_shadow(rules);
        #[cfg(debug_assertions)]
        self.debug_assert_production_shadow();

        // Living sprite/voxel/harvest animation state belongs to the committed
        // simulation frame. Keep it inside the authoritative frame transaction
        // so hashed side effects and every snapshot observe the same state.
        // Dying animation advancement remains in the live-object scheduler,
        // which owns the single pending-delete drain above.
        if frame_committed && let Some(animation_sequences) = animation_sequences {
            let game_options = self.session.game_options.clone();
            let binary_frame = self.session.binary_frame;
            {
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
            animation::tick_voxel_animations(self.entities_mut());
            animation::tick_harvest_overlays(self.entities_mut());
        }
        building_anim::finalize(self, &placed_building_owners, frame_committed, rules);
        #[cfg(debug_assertions)]
        self.debug_assert_logic_membership_consistent();
        #[cfg(debug_assertions)]
        self.debug_assert_lifecycle_consistent();
        let terminal_score_finalized =
            self.natural_outcome_exit_ready() && self.finalize_terminal_score_snapshot();
        let state_hash = self.state_hash();
        Ok(TickResult {
            tick: self.session.tick,
            frame_committed,
            executed_commands,
            state_hash,
            terminal_score_finalized,
            spawned_entities,
            destroyed_structure,
            ownership_changed: passenger_ownership_changed,
            bridge_state_changed,
            movement: movement_stats,
        })
    }

    /// World owner for the dormant `TunnelLocomotionClass::Process` path.
    ///
    /// `TunnelLocomotionClass::Process @ 0x00728e30` removes the surface
    /// object before it enters state 3, then restores that same object-list
    /// membership before Foot's state-7 abort-motion cleanup.
    fn tick_tunnel_locomotor_one(&mut self, stable_id: u64, path_grid: Option<&PathGrid>) {
        let Some((mut state, position, movement_target, cell_marked, layer)) =
            self.substrate.entities.get(stable_id).and_then(|entity| {
                entity.tunnel_state.map(|state| {
                    (
                        state,
                        (entity.position.rx, entity.position.ry),
                        entity
                            .movement_target
                            .as_ref()
                            .map(|target| (target.next_index, target.path.len())),
                        entity.lifecycle.cell_marked,
                        entity.locomotor.as_ref().map(|locomotor| locomotor.layer),
                    )
                })
            })
        else {
            return;
        };

        // The Burrow transition owns the remove-before-underground ordering.
        if state.phase == tunnel_movement::TunnelPhase::Burrow && cell_marked {
            self.remove_entity_occupancy(stable_id);
        }

        let destination_reached = movement_target
            .map(|(next_index, path_len)| next_index.saturating_add(1) >= path_len)
            .unwrap_or(true);
        let surface_cell_available = path_grid.map_or(true, |grid| {
            grid.is_walkable_on_layer(position.0, position.1, MovementLayer::Ground)
        }) && self.substrate.occupancy.is_empty_on_layer(
            position.0,
            position.1,
            MovementLayer::Ground,
        );
        let mut context = TunnelProcessContext {
            destination_reached,
            surface_cell_available,
            layer: layer.unwrap_or(MovementLayer::Ground),
            z: 0,
            surface_occupied: cell_marked,
            underground_occupied: false,
            abort_motion_called: false,
        };
        let outcome = tunnel_movement::process_tunnel(&mut state, &mut context);

        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.tunnel_state = Some(state);
            if let Some(locomotor) = entity.locomotor.as_mut() {
                locomotor.layer = context.layer;
                locomotor.runtime_payload =
                    crate::sim::movement::locomotion::piggyback::LocomotorRuntimePayload::Tunnel(
                        Some(state),
                    );
            }
            if context.abort_motion_called {
                entity.movement_target = None;
            }
        }

        // State 6's surface mark happens before state 7 clears Foot motion.
        if context.surface_occupied
            && self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| !entity.lifecycle.cell_marked)
        {
            self.add_entity_occupancy(stable_id);
        }
        if outcome == teleport_movement::SpecialMovementOutcome::Complete
            && self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| entity.tunnel_state.is_some())
        {
            if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                entity.tunnel_state = None;
                if let Some(locomotor) = entity.locomotor.as_mut() {
                    locomotor.runtime_payload = crate::sim::movement::locomotion::piggyback::LocomotorRuntimePayload::Tunnel(None);
                }
            }
        }
    }

    /// World owner for `DropPodLocomotionClass::Process` placement.
    ///
    /// Drop pods retain no cell-list membership while descending. On the
    /// terminal frame this performs one atomic choice: unlimbo and mark the
    /// target, or zero health and enqueue the common crush teardown.
    fn drop_pod_virtual_unlimbo_admitted(
        &self,
        stable_id: u64,
        target: (u16, u16),
        path_grid: Option<&PathGrid>,
    ) -> bool {
        use crate::sim::cell_rect::{
            IsClearToMoveResult, LiveCellPassabilityQuery, evaluate_live_cell_passability,
        };
        use crate::sim::pathfinding::cell_entry::{
            CanEnterCellContext, CanEnterLayerContext, CellEntryResult, TerrainCheckResult,
            TerrainEntryMode, check_terrain_with_layers,
            classify_occupied_cell_with_layers_and_ignored_and_occupation, evaluate_can_enter_cell,
        };

        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return false;
        };
        let category = entity.category;
        let owner = entity.owner();
        let regular_crusher = entity.regular_crusher;
        let omni_crusher = entity.omni_crusher;
        let locomotor = entity.locomotor.as_ref();
        let movement_zone = locomotor.map_or(Default::default(), |state| state.movement_zone);
        let speed_type = locomotor.map_or(Default::default(), |state| state.speed_type);
        let locomotor_kind = locomotor.map_or(
            crate::rules::locomotor_type::LocomotorKind::Drive,
            |state| state.effective_kind(),
        );
        let cost_grid = self.terrain_costs.get(&speed_type);

        // Named location: ObjectClass::Unlimbo's virtual Foot +0x1AC gate.
        // DropPod itself never substitutes a direct list-emptiness predicate.
        let land_passable = evaluate_can_enter_cell(CanEnterCellContext {
            wall: None,
            target,
            terrain_layer: MovementLayer::Ground,
            movement_zone: Some(movement_zone),
            speed_type: Some(speed_type),
            path_grid,
            resolved_terrain: self.resolved_terrain.as_ref(),
            terrain_costs: cost_grid,
            bypass_grid: false,
            mode: TerrainEntryMode::SpawnLike,
            is_infantry: category == EntityCategory::Infantry,
            mover_is_crusher: crate::sim::movement::bump_crush::CrushCapability::new(
                regular_crusher,
                omni_crusher,
            )
            .wall_arm_crusher(),
        })
        .is_clear();
        let cell_clear = evaluate_live_cell_passability(LiveCellPassabilityQuery {
            target,
            speed_type,
            movement_zone,
            requested_zone: None,
            actual_zone: 0,
            requested_layer: Some(MovementLayer::Ground),
            ignore_infantry: false,
            ignore_vehicles: false,
            land_passable,
            path_grid,
            resolved_terrain: self.resolved_terrain.as_ref(),
            raw_occupation: Some(&self.substrate.raw_cell_occupation),
        });
        if !matches!(
            cell_clear,
            IsClearToMoveResult::Clear { .. } | IsClearToMoveResult::ClearWinged
        ) {
            return false;
        }

        let layers = CanEnterLayerContext::single(MovementLayer::Ground);
        match check_terrain_with_layers(
            target,
            layers,
            category,
            path_grid,
            cost_grid,
            &self.substrate.occupancy,
        ) {
            TerrainCheckResult::Clear => true,
            TerrainCheckResult::Impassable => false,
            TerrainCheckResult::NeedsBlockerCheck => matches!(
                classify_occupied_cell_with_layers_and_ignored_and_occupation(
                    target,
                    layers,
                    stable_id,
                    movement::bump_crush::CrushCapability::new(regular_crusher, omni_crusher,),
                    self.interner.resolve(owner),
                    locomotor_kind,
                    false,
                    None,
                    &self.substrate.occupancy,
                    &self.substrate.cell_occupation,
                    &self.substrate.raw_cell_occupation,
                    self.session.binary_frame,
                    &self.substrate.entities,
                    &self.house_alliances,
                    &self.interner,
                ),
                CellEntryResult::Clear
            ),
        }
    }

    fn tick_drop_pod_locomotor_one(&mut self, stable_id: u64, path_grid: Option<&PathGrid>) {
        let Some(state) = self
            .substrate
            .entities
            .get(stable_id)
            .and_then(|entity| entity.drop_pod_state.clone())
        else {
            return;
        };

        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.lifecycle.cell_marked)
        {
            self.remove_entity_occupancy(stable_id);
        }

        let landing = drop_pod_movement::landing_from_virtual_unlimbo(&state, |target, _facing| {
            self.drop_pod_virtual_unlimbo_admitted(stable_id, target, path_grid)
        });
        let result = {
            let entity = self
                .substrate
                .entities
                .get_mut(stable_id)
                .expect("drop pod owner remained live during its Process visit");
            let state = entity
                .drop_pod_state
                .as_mut()
                .expect("drop pod state remained attached during its Process visit");
            drop_pod_movement::process_drop_pod_state(state, landing)
        };
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            if let (Some(state), Some(locomotor)) =
                (entity.drop_pod_state.as_ref(), entity.locomotor.as_mut())
            {
                locomotor.runtime_payload =
                    crate::sim::movement::locomotion::piggyback::LocomotorRuntimePayload::DropPod(
                        Some(state.clone()),
                    );
            }
        }

        match result.outcome {
            rocket_movement::SpecialMovementOutcome::Continue => {}
            rocket_movement::SpecialMovementOutcome::Complete => {
                let target = self
                    .substrate
                    .entities
                    .get(stable_id)
                    .and_then(|entity| entity.drop_pod_state.as_ref())
                    .map(|state| (state.target_rx, state.target_ry));
                if let (Some((rx, ry)), Some(entity)) =
                    (target, self.substrate.entities.get_mut(stable_id))
                {
                    entity.position.rx = rx;
                    entity.position.ry = ry;
                    entity.position.z = 0;
                    entity.position.exact_z_leptons = None;
                    if let Some(locomotor) = entity.locomotor.as_mut() {
                        locomotor.layer = MovementLayer::Ground;
                        locomotor.runtime_payload = crate::sim::movement::locomotion::piggyback::LocomotorRuntimePayload::DropPod(None);
                    }
                    entity.drop_pod_state = None;
                }
                self.add_entity_occupancy(stable_id);
            }
            rocket_movement::SpecialMovementOutcome::Abort => {
                if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                    entity.health.current = 0;
                    if let Some(locomotor) = entity.locomotor.as_mut() {
                        locomotor.runtime_payload = crate::sim::movement::locomotion::piggyback::LocomotorRuntimePayload::DropPod(None);
                    }
                }
                self.pending_lifecycle_requests
                    .push(LifecycleRequest::Uninit {
                        stable_id,
                        reason: crate::sim::lifecycle_request::UninitReason::Crush,
                    });
            }
        }
    }
}

#[cfg(test)]
#[path = "world_tests.rs"]
pub(crate) mod tests;

#[cfg(test)]
#[path = "smudge_integration_tests.rs"]
mod smudge_integration_tests;

#[cfg(test)]
#[path = "world_orders_c4_tests.rs"]
mod world_orders_c4_tests;

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

#[cfg(test)]
#[path = "radar_dirty_ack_tests.rs"]
mod radar_dirty_ack_tests;

#[cfg(test)]
#[path = "bridge_parity_harness_tests.rs"]
mod bridge_parity_harness_tests;

/// Draw flags the drive locomotor passes to the wake `AnimClass` constructor
/// (0x004B0823 region: `PUSH 0x600`).
const WAKE_DRAW_FLAGS: u32 = 0x600;

/// The wake gate for one unit this frame: moving now, not on a bridge, on a
/// cell whose `CellClass+0xEC` mirror is Water, anchored at its exact leptons.
pub(crate) fn wake_anchor_for(
    entity: &crate::sim::game_entity::GameEntity,
    terrain: Option<&ResolvedTerrainGrid>,
    binary_frame: u32,
) -> Option<(u16, u16, SimFixed, SimFixed, u8)> {
    if !crate::sim::movement::ready_producer::is_moving_now_for(entity, binary_frame) {
        return None;
    }
    if entity.is_on_bridge_layer() {
        return None;
    }
    let cell = terrain?.cell(entity.position.rx, entity.position.ry)?;
    if cell.yr_cell_land_type != crate::rules::terrain_rules::LandType::Water.as_index() {
        return None;
    }
    Some((
        entity.position.rx,
        entity.position.ry,
        entity.position.sub_x,
        entity.position.sub_y,
        entity.position.z,
    ))
}
