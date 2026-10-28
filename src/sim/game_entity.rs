//! Unified entity struct replacing hecs ECS components.
//!
//! All 31 former ECS components are fields on `GameEntity`. Always-present
//! data is stored directly; optional/conditional components use `Option<T>`.
//! Zero-size markers (Selected, Repairing, VoxelModel/SpriteModel) become bools.
//!
//! ## Why plain structs?
//! - Deterministic iteration (sorted by stable_id) without per-query sorting
//! - Direct field access (`entity.position`) instead of `world.get::<&Position>(e)`
//! - No two-phase snapshot patterns needed for simple mutations
//! - Simpler borrow checker interactions than ECS archetype queries
//!
//! ## Dependency rules
//! - Part of sim/ — depends on map/ (EntityCategory), sim/components, sim/locomotor,
//!   sim/combat (TarCom), sim/animation, sim/miner, and special movement modules.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::map::entities::EntityCategory;
use crate::sim::ammo::TechnoAmmo;
use crate::sim::animation::Animation;
use crate::sim::cloak_disguise::{CloakRuntime, DisguiseRuntime, StealthAbility};
use crate::sim::combat::base_defense_response::BaseDefenseResponseState;
use crate::sim::combat::{Health, TarCom};
use crate::sim::components::{NavigationState, Position};
use crate::sim::intern::InternedId;
use crate::sim::miner::Miner;
use crate::sim::mission::{MissionCom, MissionLeafState, MissionType};
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::tube_movement::LowBridgeTubeMovementState;
use crate::sim::passenger::PassengerRole;
use crate::sim::radio::Contacts;
use crate::sim::superweapon::invulnerability::InvulnerabilityState;
use crate::sim::world::ObjectLifecycle;

#[cfg(feature = "ares")]
mod attach_effect;
mod distributed_fire;
mod emp;
mod foot_air;
mod pip_fill;
#[cfg(feature = "ares")]
mod type_conversion;
pub(crate) use distributed_fire::DistributedFireCandidate;
pub use pip_fill::rounded_pip_quantity;

use crate::sim::object_fall::ObjectFallState;

/// The barrel elevation's rate: the TechnoClass constructor passes 3
/// (`PUSH 3` at `0x006F2EDE`) to `FacingClass(int) @ 0x004C91E0`.
const BARREL_ELEVATION_ROT: i32 = 3;

/// A level barrel: the draw pitches by `d32 - 8`, which is zero at `0x4000`.
pub const BARREL_LEVEL: u16 = 0x4000;

/// The elevation Unlimbo turns a barrel toward: level minus the low byte of
/// `FireAngle=` shifted up a byte (`MOV CH, byte [Type+0x3D0]` at
/// `0x006F6DD9`, `SUB` at `0x006F6DDF`), kept to a word.
pub fn unlimbo_barrel_target(fire_angle: i32) -> u16 {
    BARREL_LEVEL.wrapping_sub(u16::from(fire_angle as u8) << 8)
}

pub(crate) use crate::sim::gate_runtime::BuildingGateRuntime;

/// Constructor state captured while a generated-map Techno still owns the
/// launch-time Scenario cursor. Projection validates all identity fields
/// before installing the already-consumed word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedTechnoInit {
    pub entity_index: usize,
    pub techno_type: String,
    pub cell: (u16, u16),
    pub techno_ctor_random_word: u16,
    pub native_unique_id: i32,
}

/// The two evidence-backed ways a live Techno obtains its persistent
/// constructor word. Only `FreshScenario` is allowed to advance Scenario RNG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TechnoConstructorInit {
    FreshScenario,
    PreconsumedGenerated(GeneratedTechnoInit),
}

/// Every game object (unit, infantry, building, aircraft) is one `GameEntity`.
/// Core fields are always present; optional subsystems use `Option<T>`. A
/// mechanism's own state is a record its module defines and alone writes
/// (for example `building_art`, `veterancy`); other code reads it through
/// getters and changes it through that module's functions.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GameEntity {
    /// Bounded optional observations; absent from saves and normal builds.
    #[cfg(feature = "debug-tools")]
    #[serde(skip)]
    pub(crate) debug_log: Option<crate::sim::debug_event_log::DebugEventLog>,
    // --- Always present (every entity has these) ---
    // The object's identity (stable id, native UniqueID, the constructor's
    // Scenario word, owner, type and class) is private to this module:
    // readers use the getters, construction and decoding establish it, and
    // only the owner-change path below moves the owner.
    /// Deterministic stable ID — primary key, used for cross-entity references,
    /// replay logs, state hashing, and networking. Never reused.
    stable_id: u64,
    /// AbstractClass+10, assigned by the concrete constructor after the base
    /// Techno Scenario word (Unit735454 / Building43BA15). May wrap or duplicate;
    /// stable handles remain the reference and storage authority.
    native_unique_id: i32,
    /// Native Techno+24C float bits, initialized to zero by6F2D62 and
    /// preserved by native persistence (70C32A). Only visual-character4 reads
    /// this displacement through70BE50; no simulation/RNG/timer consumer.
    /// Retain the raw datum, not a renderer-owned reconstructed animation.
    native_cloak_displacement: crate::util::native_x87::NativeF32Bits,
    /// Techno+3D0: TechnoClass::Constructor `0x006F2B40` clears it at
    /// `0x006F2F3D`; UnitClass::Mission_Harvest `0x0073E5E0` sets/clears it at
    /// `0x0073E8FA`/`0x0073E925`, and the repair depot reads it at
    /// `0x0044C4BC`. This belongs to common Techno state, not the optional
    /// `Miner`, because owner changes retain it and the depot consumes it.
    #[serde(default)]
    useless: bool,
    /// Techno+300: constructor6F2EA4 initializes zero. Factory StartProduction
    /// 4C9DED retains its owner's Cost_Of quote; Grand_Opening446AF2 consumes
    /// it independently of remaining debt and later House discounts (gamemd).
    production_start_price: i32,
    /// Techno+504 and Ares held sparkle. One private counter owner; stock
    /// constructor remains zero and only the feature-gated EMP producer writes it.
    #[serde(default)]
    emp: emp::EmpState,
    /// Ares builtin attachments: ordered instance vector, owned by attach_effect.
    #[cfg(feature = "ares")]
    #[serde(default)]
    attach_effects: attach_effect::AttachEffects,
    /// Phobos ShieldClass state: shield points and their recovery timers.
    #[cfg(feature = "phobos")]
    #[serde(default)]
    pub(super) shield: Option<crate::sim::shield::ShieldState>,
    /// Low word of the one raw Scenario RNG draw performed by the active-retail
    /// `TechnoClass` constructor (`0x006F3254`, stored at native `+0x3C8`).
    /// Later report-selection consumers read this persistent value; placement
    /// failure never refunds the draw.
    techno_ctor_random_word: u16,
    /// Building+6F4, constructor43B9B5 null; startup68C1B5 assigns a type
    /// pointer and Load453FB0 swizzles it. Capture never rerolls the reward.
    secret_lab_fallback: Option<(crate::rules::object_type::ObjectCategory, InternedId)>,
    /// gamemd.exe TechnoClass::Constructor @0x006F2B40 stores -1 at6F2D0F.
    /// One signed +214 owner for scenario recruitment and local control groups.
    /// Team removal, Limbo and ChangeOwner preserve it; raw Save70C250 and
    /// Load70BF50 retain it with the rest of the concrete object body.
    object_group: i32,
    /// One native Techno stage clock. Class receivers mutate through this
    /// owner; animation, miner and buildup components carry no competing copy.
    stage: crate::sim::stage::StageClass,
    /// The client-relative discovery bytes (`+0x41A..+0x41C`), a
    /// [`TechnoDiscoveryHistory`] that `world::lifecycle` owns.
    ///
    /// [`TechnoDiscoveryHistory`]: crate::sim::world::TechnoDiscoveryHistory
    pub discovery: crate::sim::world::TechnoDiscoveryHistory,
    /// Building-owned upgrade count and installed type identities. Original
    /// 4409E2 consumes the temporary Building and retains only its type here.
    #[serde(default)]
    building_upgrades: crate::sim::building_upgrades::BuildingUpgrades,
    /// World position in isometric cell coordinates + cached screen position.
    pub position: Position,
    /// The body heading, `TechnoClass+0x388` (PrimaryFacing), and its only
    /// copy: every class has one (the TechnoClass constructor builds it at
    /// `0x006F2EEB`), a building included — its `+0x388` is what its turret
    /// or its body aims. 16-bit DirStruct, 0 = north on screen, 0x4000 = east.
    ///
    /// Readers sample `current(frame)` (`FacingClass::Current @ 0x004C93D0`),
    /// writers turn it (`Set @ 0x004C9220`) or snap it (`Set_Current @
    /// 0x004C9300`). Its rate is written once, by the class constructor
    /// ([`Self::set_body_facing_rot`]). The image's only other `+0x39C` write
    /// is AircraftClass::AI's Carryall copy (`0x00415146..0x00415184`, the
    /// carrier's `+0x3A0` over its cargo's `+0x388` and `+0x3A0`), gated on
    /// AircraftType `+0xDFC` (`Carryall=`, read at `0x0041CCA2`). Retail sets
    /// that key only on `[HIND]`, a VehicleType, so the copy is dormant and not
    /// ported.
    pub body_facing: crate::sim::movement::FacingClass,
    /// The barrel elevation, `TechnoClass+0x370`, the first of the three
    /// FacingClasses (the body is `+0x388`, the turret `+0x3A0`,
    /// [`Self::barrel_facing`]). The TechnoClass constructor builds it at 0
    /// turning at rate 3 (`0x006F2EDE..0x006F2EE6`, `0x004C91E0`), and
    /// [`Self::unlimbo_barrel_elevation`] levels and aims it. `UnitClass::DrawVoxelBody`
    /// pitches the barrel voxel by it (`0x0073BB77..0x0073BBAC`); nothing in
    /// a unit's simulation reads it.
    ///
    /// The building writers (the constructor's `Set`s at `0x0043BA5E` and
    /// `0x0043BACB`, Sell's `Set_Current` at `0x0044A08C`, Mission_Missile's
    /// `Set`s at `0x0044CEE2` and `0x0044CF7A`, an MCV deploy's `Set_Current`
    /// at `0x00739827`) and their readers are not ported.
    barrel_elevation: crate::sim::movement::FacingClass,
    /// The body-animation counter (`FootClass+0x538`), a
    /// [`BodyFrameCounter`] that `sim::animation` owns.
    ///
    /// [`BodyFrameCounter`]: crate::sim::animation::BodyFrameCounter
    #[serde(default)]
    pub body_frame_counter: crate::sim::animation::BodyFrameCounter,
    /// UnitClass+6C0, owned by sim::animation. FireAt seeds it even when
    /// LimboLaunch has removed this Unit from Logic; Unlimbo preserves it.
    #[serde(default)]
    pub unit_firing_counter: crate::sim::animation::UnitFiringCounter,
    /// The owning house (`TechnoClass+0x21C`), interned.
    /// `TechnoClass::ChangeOwner @ 0x007014A0` moves it; here only the store's
    /// owner-change path does ([`Self::set_owner_from_store`]).
    owner: InternedId,
    /// The Techno constructor's House argument, retained at +0x14C
    /// (`0x006F322B..0x006F3241`). ChangeOwner never replaces it; Building's
    /// capture-sale tail compares it with the current owner (4492EF).
    /// Houses remain registered for the scenario's lifetime.
    initial_owner: InternedId,
    /// Ares3.0 TechnoExt+40, initialized by1004A776..1004A791 and
    /// transferred by10045B54. Capture preserves this Country identity.
    #[cfg(feature = "ares")]
    original_factory_country: Option<crate::rules::ruleset::CountryIdx>,
    /// The object's health (`ObjectClass+0x6C`), a [`Health`] that
    /// `combat::object_health` owns.
    pub health: Health,
    /// Object+70: signed damage reservations/recovery, not an actual-HP cache.
    /// Native constructors and map admission seed this from admitted health.
    pub estimated_health: crate::sim::estimated_health::EstimatedHealth,
    /// rules.ini section name (e.g., "HTNK", "E1", "GAPOWR") — interned for zero-cost clones.
    type_ref: InternedId,
    /// The concrete class (`AbstractClass::What_Am_I`): Unit, Infantry,
    /// Aircraft or Structure. Fixed at construction.
    category: EntityCategory,
    /// Facts the object's type fixes (its foundation and a building's base
    /// and cell-list inputs), a [`TypeFacts`] that construction stamps.
    ///
    /// [`TypeFacts`]: crate::sim::world::TypeFacts
    pub type_facts: crate::sim::world::TypeFacts,
    /// The object's veterancy: the running accumulator every rank is sampled
    /// from, and the rank last announced. `combat::veterancy` owns it.
    #[serde(default)]
    pub veterancy: crate::sim::combat::veterancy::Veterancy,
    /// The flash countdown (`TechnoClass+0xF0`) the elite promotion, vt+0x148
    /// Flash and team script action 50 seed. `sim::flashing` owns
    /// it.
    #[serde(default)]
    pub flashing: crate::sim::flashing::Flashing,
    /// The armor and firepower crate multipliers (`Techno+0x158`, `+0x160`),
    /// a [`CrateMultipliers`] that the crate arms own.
    ///
    /// [`CrateMultipliers`]: crate::sim::crates::CrateMultipliers
    #[serde(default)]
    pub(crate) crate_multipliers: crate::sim::crates::CrateMultipliers,
    /// The type facts the house counts and the score test (`DontScore=` and
    /// the rest), a [`TrackingFacts`](crate::sim::house_tracking::TrackingFacts)
    /// that construction stamps.
    #[serde(default)]
    pub(crate) tracking_facts: crate::sim::house_tracking::TrackingFacts,
    /// Native infantry total-count latches (`Techno+0x438/+0x439`), owned by
    /// `sim::house_tracking`. The constructor starts both clear; Add_Tracking
    /// sets the once-counted latch when it adds House+0x2F4.
    #[serde(default)]
    house_tracking_latches: crate::sim::house_tracking::InfantryTrackingLatches,
    /// Phobos placement history, written only by the house-tracking owner.
    #[cfg(feature = "phobos")]
    #[serde(default)]
    pub(crate) phobos_placement: crate::sim::house_tracking::PhobosPlacement,
    /// The raw signed type `Sight=`, a [`SightFacts`] that
    /// `sim::vision` owns.
    ///
    /// [`SightFacts`]: crate::sim::vision::SightFacts
    pub(crate) sight: crate::sim::vision::SightFacts,
    /// Foot65C/664 high-flying sight refresh timer, virtualized per viewer.
    /// Independent of the retained sight-admission latch and stored footprint.
    pub(crate) sight_refresh_timers: crate::sim::vision::SightRefreshTimers,
    /// Building43FB20 operational edge and Techno6FB170/6FB470 deposit latch.
    pub(crate) gap_generator: crate::sim::world::GapGeneratorRuntime,

    // --- Render model (mutually exclusive) ---
    /// True = VXL/HVA model, false = SHP sprite; effective art metadata is
    /// authoritative. Only the constructor writes it ([`Self::is_voxel`]).
    is_voxel: bool,

    // --- Bool markers (were zero-size ECS components) ---
    /// IsSelected (`ObjectClass+0x83`), a [`Selection`] that `sim::selection`
    /// owns.
    ///
    /// [`Selection`]: crate::sim::selection::Selection
    pub selection: crate::sim::selection::Selection,
    /// Offline for want of a powering building (`TechnoClass+0x1C8`), a
    /// [`Deactivation`] that `sim::powered_unit` owns.
    ///
    /// [`Deactivation`]: crate::sim::powered_unit::Deactivation
    #[serde(default)]
    pub deactivated: crate::sim::powered_unit::Deactivation,
    /// A building's repair byte (`BuildingClass+0x6E8`), a [`RepairByte`]
    /// that `production::production_repair` owns.
    ///
    /// [`RepairByte`]: crate::sim::production::RepairByte
    pub repairing: crate::sim::production::RepairByte,
    /// IsInLogic (`ObjectClass+0x98`), a [`LogicMembership`] that
    /// `world::logic_vector` owns. Not saved: a load rebuilds it from the saved
    /// vector order.
    ///
    /// [`LogicMembership`]: crate::sim::world::LogicMembership
    #[serde(skip)]
    pub in_logic_vector: crate::sim::world::LogicMembership,
    /// Independent, serialized ObjectClass lifecycle facts, an
    /// [`ObjectLifecycle`] that `world::lifecycle` owns.
    #[serde(default)]
    pub lifecycle: ObjectLifecycle,
    /// The playfield membership (`TechnoClass+0x3D5`), a
    /// [`PlayfieldMembership`] that `world::playfield_membership` owns.
    ///
    /// [`PlayfieldMembership`]: crate::sim::world::PlayfieldMembership
    #[serde(default)]
    pub(crate) in_playfield: crate::sim::world::PlayfieldMembership,
    /// Techno `+0x3D4`, a [`MissionOnly`] that `sim::aircraft` owns.
    ///
    /// [`MissionOnly`]: crate::sim::aircraft::MissionOnly
    #[serde(default)]
    pub mission_only: crate::sim::aircraft::MissionOnly,
    /// The type facts the removal paths read (Limbo's dirty-rect branch and
    /// `Occupier=`), a [`RemovalFacts`] that `world::lifecycle` owns.
    ///
    /// [`RemovalFacts`]: crate::sim::world::RemovalFacts
    #[serde(default)]
    pub(crate) removal_facts: crate::sim::world::RemovalFacts,
    /// The kill credit and the UnInit loss guard, a [`KillRecord`] that
    /// `combat::record_kill` owns.
    ///
    /// [`KillRecord`]: crate::sim::combat::KillRecord
    #[serde(default)]
    pub(crate) kill_record: crate::sim::combat::KillRecord,
    /// Foot's independent tracker Cell, slot-notification Cell and retained
    /// airborne-vector membership. Only the shared Foot air owner writes them.
    #[serde(default)]
    foot_air: foot_air::FootAirState,
    /// Techno's runtime accepted candidate/score pairs and fired history.
    /// The private owner writes them; normal native-like Load clears them.
    #[serde(default)]
    distributed_fire: distributed_fire::DistributedFireState,

    // --- Optional subsystem components ---
    /// Locomotor state — present on moving types, on every Infantry, and on
    /// zero-speed Drive/Ship Units and Aircraft whose native class-local
    /// payload still exists.
    pub locomotor: Option<LocomotorState>,
    /// FootClass-style owner navigation destination state.
    ///
    /// Native `NavCom` is distinct from the active execution path, so this can
    /// remain visible after a DriveTrack segment has cleared.
    #[serde(default)]
    pub navigation: NavigationState,
    /// Foot+5DC, retained across mission changes and swizzled by native Load.
    #[serde(default)]
    pub(crate) patrol_post: crate::sim::world::PatrolPost,
    /// Live Foot-owned applied speed; survives active locomotor replacement.
    #[serde(default)]
    pub foot_speed: crate::sim::components::FootSpeedState,
    /// Techno+2E8, ctor6F2E00 zero, saved70C354. Fly approach writes it;
    /// landing and Aircraft unload consume it. Independent of body rocking
    /// and of the active/suspended locomotor. Deterministic fixed radians.
    #[serde(default)]
    pub(crate) flight_attitude: crate::sim::movement::fly_height::FlightAttitude,
    /// Foot+6B4, ctor4D343E zero; IonBlast53CF7F writes a signed pixel
    /// lift. FootAI4DAECA clears it; Foot save4DBD7C retains it.
    #[serde(default)]
    ion_blast_lift: i8,
    /// The Foot occupation enable (`Foot+0x6B6`), an [`OccupationEnable`]
    /// that `movement::foot_mark` owns. Unit's constructor (`0x007353CE`)
    /// runs the Foot one that sets it.
    ///
    /// [`OccupationEnable`]: crate::sim::movement::foot_mark::OccupationEnable
    pub(crate) foot_occupation_enabled: crate::sim::movement::foot_mark::OccupationEnable,
    /// The Magnetron lift's latches and links (Foot `+0x6AD`/`+0x6AE`,
    /// Techno `+0x2AC`/`+0x2B0`), a [`LocomotorLift`] that `locomotor_lift`
    /// owns.
    ///
    /// [`LocomotorLift`]: crate::sim::locomotor_lift::LocomotorLift
    #[serde(default)]
    pub lift: crate::sim::locomotor_lift::LocomotorLift,
    /// The drop's and the Chrono Warp's kill credit (Techno `+0x427`,
    /// `+0x428`, `+0x42C`), a [`DropCredit`] that `locomotor_lift` owns.
    ///
    /// [`DropCredit`]: crate::sim::locomotor_lift::DropCredit
    #[serde(default)]
    pub(crate) drop_credit: crate::sim::locomotor_lift::DropCredit,
    /// Techno+0x1F8, a [`SetterForceReassign`] that `movement::track_path`
    /// owns.
    ///
    /// [`SetterForceReassign`]: crate::sim::movement::SetterForceReassign
    #[serde(default)]
    pub(crate) setter_force_reassign: crate::sim::movement::SetterForceReassign,
    /// Techno+0x284, a [`ChronoWarpDelay`] that `movement::teleport_movement`
    /// owns.
    ///
    /// [`ChronoWarpDelay`]: crate::sim::movement::teleport_movement::ChronoWarpDelay
    #[serde(default)]
    pub(crate) chrono_warp_delay: crate::sim::movement::teleport_movement::ChronoWarpDelay,
    /// The object's target (`TechnoClass+0x2B4`), a [`TarCom`] that the class
    /// target setter (`mission::concrete_effects`) writes.
    pub attack_target: TarCom,
    /// A building's delayed fire: a delayed shot, or a Prism support beam. A
    /// [`BuildingDelayedFire`] that `techno_ai::building_missions` owns.
    ///
    /// [`BuildingDelayedFire`]: crate::sim::world::BuildingDelayedFire
    #[serde(default)]
    pub pending_building_fire: crate::sim::world::BuildingDelayedFire,
    /// The Prism master's recruited supporters (`BuildingClass+0x664`), a
    /// [`PrismSupport`] that `techno_ai::building_missions` owns.
    ///
    /// [`PrismSupport`]: crate::sim::world::PrismSupport
    #[serde(default)]
    pub prism_support_count: crate::sim::world::PrismSupport,
    /// The current weapon and turret (`TechnoClass+0x138`, `+0x124`) and the
    /// charge turret's saved rearm (`+0x2F8`), a [`TurretSelection`] that
    /// `game_entity::gunner` owns.
    ///
    /// [`TurretSelection`]: gunner::TurretSelection
    turret_selection: gunner::TurretSelection,
    /// RadioClass-style live contacts for this entity, stored as stable IDs.
    /// Used by runtime building-entry/pathing exceptions such as contacted
    /// war factory exits and refinery dock entry. Kept per mover; a building
    /// being contacted does not globally relax passability for unrelated units.
    /// `sim::radio` owns the slots (`RadioClass+0xE4`/`+0xE8`).
    #[serde(default)]
    pub radio_contacts: Contacts,
    /// The tether latch (`TechnoClass+0x418`), a [`Tether`] that `sim::radio`
    /// owns.
    ///
    /// [`Tether`]: crate::sim::radio::Tether
    #[serde(default)]
    pub tether: crate::sim::radio::Tether,
    /// The computer's sale and repair inputs (`TechnoClass+0x3D1`,
    /// `BuildingClass+0x6DC`, `+0x6CB`), an [`AiSellRepair`] that
    /// `production::production_repair` owns.
    ///
    /// [`AiSellRepair`]: crate::sim::production::AiSellRepair
    #[serde(default)]
    pub(crate) ai_sell_repair: crate::sim::production::AiSellRepair,
    /// The turret heading, `TechnoClass+0x3A0` (SecondaryFacing), a
    /// [`FacingClass`] that moves only through its own methods. VERA keeps
    /// one for a `Turret=yes` unit and every aircraft: construction installs
    /// it (`world_spawn::construction`) and a Fly order installs it on an
    /// aircraft built without one ([`Self::ensure_fly_secondary_facing`]).
    /// A building aims its [`Self::body_facing`] instead.
    ///
    /// [`FacingClass`]: crate::sim::movement::FacingClass
    pub barrel_facing: Option<crate::sim::movement::FacingClass>,
    /// The turret rotation latch (`UnitClass+0x6AF`), a
    /// [`TurretRotationLatch`] that `world::unit_post` owns.
    ///
    /// [`TurretRotationLatch`]: crate::sim::world::unit_post::TurretRotationLatch
    #[serde(default)]
    pub(crate) turret_rotation_latch: crate::sim::world::unit_post::TurretRotationLatch,
    /// The frame of the object's own last shot (`TechnoClass+0x120`), a
    /// [`LastFireFrame`] that FireAt (`combat::world_fire`) writes.
    ///
    /// [`LastFireFrame`]: crate::sim::combat::world_fire::LastFireFrame
    #[serde(default)]
    pub last_fire_frame: crate::sim::combat::world_fire::LastFireFrame,
    /// The turret rotation-sound latch (`TechnoClass+0x49C`), a
    /// [`TurretRotateLatch`] that `world::turret_rotate_sound` owns.
    ///
    /// [`TurretRotateLatch`]: crate::sim::world::TurretRotateLatch
    #[serde(default)]
    pub(crate) turret_rotate_latch: crate::sim::world::TurretRotateLatch,
    /// Techno `+0x4F8`, a [`DeployVoiceSuppression`] that
    /// `world::selection_voice` owns.
    ///
    /// [`DeployVoiceSuppression`]: crate::sim::world::DeployVoiceSuppression
    #[serde(default)]
    pub suppress_deploy_voice: crate::sim::world::DeployVoiceSuppression,
    /// The rearm countdown (`TechnoClass+0x2EC`), a [`RearmTimer`] whose
    /// writers `combat::rof` documents.
    ///
    /// [`RearmTimer`]: crate::sim::combat::rof::RearmTimer
    #[serde(default)]
    pub rearm_timer: crate::sim::combat::rof::RearmTimer,
    /// The radar combat flash countdown (`TechnoClass+0x174`). The
    /// constructor starts it at the current frame with no duration
    /// (`0x006F2C1C..0x006F2C28`); ReceiveDamage restarts it with
    /// `[General] RadarCombatFlashTime=` (`0x00701FA6..0x00701FC8`). Only the
    /// radar reads it, so the world hash leaves it out.
    #[serde(default)]
    pub radar_combat_flash: crate::sim::timer::CdTimer,
    /// Gattling stage, value and report latch (`TechnoClass+0x140`,
    /// `+0x144`, `+0x4B8`), owned by `combat::gattling`.
    #[serde(default)]
    pub gattling: crate::sim::combat::gattling::GattlingState,
    /// The voxel turret's animation counter (`TechnoClass+0x148`), a
    /// [`TurretAnimCounter`] that `combat::gattling` owns.
    ///
    /// [`TurretAnimCounter`]: crate::sim::combat::gattling::TurretAnimCounter
    #[serde(default)]
    pub turret_anim_frame: crate::sim::combat::gattling::TurretAnimCounter,
    /// Optional presentation-only native recoil, owned by voxel_recoil.
    voxel_recoil: Option<Box<voxel_recoil::VoxelRecoil>>,
    /// Techno+3B8 burst and +43C radial launch indices, owned by
    /// `combat::burst`. Only the burst index resets on a null target.
    #[serde(default)]
    pub(crate) weapon_burst: crate::sim::combat::burst::WeaponBurst,
    /// Sole native Building+534/+538 body state; construction/sale use the
    /// existing private StageClass and MissionLeaf ready byte.
    building_body: Option<crate::sim::building_construction::BuildingBody>,
    /// Building `+0x544`, a [`PowerHealthSample`] that `sim::power_system`
    /// owns; a Building's alone is live.
    ///
    /// [`PowerHealthSample`]: crate::sim::power_system::PowerHealthSample
    building_power_health_sample: crate::sim::power_system::PowerHealthSample,
    /// Sale route metadata, never a second mission, ready byte or timer.
    building_sale: Option<crate::sim::building_construction::BuildingDown>,
    /// The wait before the building's computer factory tries its finished
    /// object again, an [`AiPlacementTimer`] that `production::factory_ai`
    /// owns.
    ///
    /// [`AiPlacementTimer`]: crate::sim::production::AiPlacementTimer
    #[serde(default)]
    pub(crate) ai_placement_timer: crate::sim::production::AiPlacementTimer,
    /// The Building's animation art: its AnimClass slots, their replay
    /// marks, the damaged-art flag, its StorageClass art, the Stuff flag and
    /// the Grand_Opening guard. `sim::building_art` owns it.
    #[serde(default)]
    pub building_art: crate::sim::building_art::BuildingArt,
    /// Independent Building+660, owned by sim::building_power.
    #[serde(default)]
    pub(crate) building_online: crate::sim::building_power::BuildingOnline,
    /// Per-instance Deliver.BaseNormal override, not a mutation of its type.
    #[serde(default)]
    unit_delivery_no_base: bool,
    /// Ares launch source; the entity owns its lifetime and save state.
    #[serde(default)]
    hunter_seeker_origin: Option<crate::sim::hunter_seeker::HunterSeekerOrigin>,
    /// Building+66C charger pointers and +661 weapon latch, owned by
    /// `sim::tesla_charging` independently of House power accounting.
    #[serde(default)]
    pub(crate) tesla_charge: crate::sim::tesla_charging::BuildingCharge,
    /// Foot+68E automatic charging-target intent, owned by `sim::tesla_charging`.
    #[serde(default)]
    pub(crate) tesla_charger: crate::sim::tesla_charging::TeslaCharger,
    /// The last operational sample (`BuildingClass+0x6C8`), a
    /// [`LastOperational`] that `world::gap_generator` owns.
    ///
    /// [`LastOperational`]: crate::sim::world::LastOperational
    #[serde(default)]
    pub building_last_operational: crate::sim::world::LastOperational,
    /// Building+53C, a signed index: constructor43B783 starts at zero,
    /// InitManagers442C5C installs the owner's Country index, and damage44298B
    /// records an attacker's House index. Sale44A1EF writes -1 before its
    /// live callbacks; RecordKill70304D/7033C3 then suppress BuildingsLost.
    building_loss_index: i32,
    /// A `PowersUnit=` building's share in its house's powered centers
    /// (`BuildingClass+0x662`), a [`PoweredCenter`] that `sim::powered_unit`
    /// owns.
    ///
    /// [`PoweredCenter`]: crate::sim::powered_unit::PoweredCenter
    #[serde(default)]
    pub(crate) building_powered_center: crate::sim::powered_unit::PoweredCenter,
    /// HasEngineer (`BuildingClass+0x6CC`), a [`HasEngineer`] that
    /// `sim::building_art` owns.
    ///
    /// [`HasEngineer`]: crate::sim::building_art::HasEngineer
    #[serde(default)]
    pub(crate) building_has_engineer: crate::sim::building_art::HasEngineer,
    /// The `HasSpotlight=` fact and the owned BuildingLight, a
    /// [`BuildingLight`] that `world::lifecycle` owns.
    ///
    /// [`BuildingLight`]: crate::sim::world::BuildingLight
    #[serde(default)]
    pub(crate) building_light: crate::sim::world::BuildingLight,
    /// The damage-fire transition cache and its eight AnimClass slots
    /// (`BuildingClass+0x5E8`, `+0x5C8..+0x5E4`), a [`DamageFires`] that
    /// `sim::anim_class` owns.
    ///
    /// [`DamageFires`]: crate::sim::anim_class::DamageFires
    #[serde(default)]
    pub(crate) damage_fires: crate::sim::anim_class::DamageFires,
    /// ObjectClass::OnBridge (`Object+0x8C`), an [`OnBridge`] that
    /// `movement::movement_bridge` owns.
    ///
    /// [`OnBridge`]: crate::sim::movement::movement_bridge::OnBridge
    #[serde(default)]
    pub(crate) on_bridge: crate::sim::movement::movement_bridge::OnBridge,
    /// Runtime-only Foot bridge mismatch latch (`FootClass+0x68b` in YR).
    #[serde(default)]
    pub(crate) runtime_bridge_transition:
        crate::sim::movement::movement_bridge::RuntimeBridgeTransitionState,
    /// Infantry sprite animation state (sequence + frame + timing).
    pub animation: Option<Animation>,
    /// The harvester's hold, unload and harvest latches, a [`Miner`] that
    /// `sim::miner` owns; construction installs it for a harvester type.
    pub miner: Option<Miner>,
    /// `SlaveManagerClass` of an `Enslaves=` master (`TechnoClass+0x2D8`).
    #[serde(default)]
    pub slave_manager: Option<crate::sim::slave_manager::SlaveManager>,
    /// The slave side of `SlaveManagerClass`: the master whose manager holds
    /// this slave (`TechnoClass+0x2DC`) and its Storage (`+0x33C`), written
    /// only by `sim::slave_manager`.
    #[serde(default)]
    pub slave: crate::sim::slave_manager::SlaveLink,
    /// The Foot's MegaMission order, the attack-move (`FootClass +0x5C4..+0x5D1`),
    /// owned by `sim::mission::mega_mission`.
    #[serde(default)]
    pub(crate) mega_mission: crate::sim::mission::mega_mission::MegaMission,
    /// The cloak state, progress and timers, a [`CloakRuntime`] that
    /// `sim::cloak_disguise` owns; the cloak AI installs it for a cloakable
    /// object.
    #[serde(default)]
    pub cloak: Option<CloakRuntime>,
    /// Instance stealth grant; sim::cloak_disguise owns its mutations.
    #[serde(default)]
    pub(crate) stealth_ability: StealthAbility,
    /// Last sensor coverage actually deposited into CellClass counters.
    /// Removal must use these cached owner/location/radius values after the
    /// entity's current facts have changed.
    /// `sensor_lifecycle` owns it.
    #[serde(default)]
    pub(crate) sensor_deposit: Option<crate::sim::sensor_lifecycle::SensorDeposit>,
    /// Core disguise identity, timestamp, and reveal tuple, owned by
    /// `cloak_disguise`.
    #[serde(default)]
    pub disguise: Option<DisguiseRuntime>,
    /// Active low-bridge TubeClass movement. Active YR behaviour — not to be
    /// confused with the subterranean tunnel locomotor, which is Tiberian Sun
    /// legacy and was removed as unreachable in stock YR.
    /// `movement::tube_movement` owns it.
    #[serde(default)]
    pub low_bridge_tube_state: Option<LowBridgeTubeMovementState>,
    /// Controller-owned reversible mind-control manager (`TechnoClass+0x2BC`),
    /// owned by `capture_manager`. Capacity and ordered victim links are
    /// authoritative runtime state; they cannot be reconstructed from the
    /// victims' back-links.
    #[serde(default)]
    pub capture_manager: Option<crate::sim::capture_manager::CaptureManagerState>,
    /// Spawn-manager pool carried by a `Spawns=` parent (V3 Launcher,
    /// Dreadnought, Boomer, Aircraft Carrier, Destroyer). Mirrors the native
    /// `TechnoClass+0x2D0` manager pointer: present iff `Spawns=` resolved.
    #[serde(default)]
    pub spawn_manager: Option<crate::sim::spawn_manager::SpawnManagerState>,
    /// A spawned child's SpawnOwner (`TechnoClass+0x2D4`), a [`SpawnOwner`]
    /// that `spawn_manager` owns.
    ///
    /// [`SpawnOwner`]: crate::sim::spawn_manager::SpawnOwner
    #[serde(default)]
    pub spawn_owner_id: crate::sim::spawn_manager::SpawnOwner,
    /// Object-level fall state, shared by Paradrop and DropAsBomb.
    #[serde(default)]
    fall: ObjectFallState,
    /// The IronCurtain or ForceShield state: the curtain's timer and its tint
    /// stage. `None` until the first curtain; `Some` after it, nullifying all
    /// damage (except healing) while the timer runs ([`is_invulnerable`]).
    /// Applied by superweapon launch handlers; `superweapon::invulnerability`
    /// owns it.
    ///
    /// [`is_invulnerable`]: crate::sim::superweapon::invulnerability::is_invulnerable
    #[serde(default)]
    pub invulnerability: Option<InvulnerabilityState>,
    /// Techno+294 owned manager or borrowed plane/building link; Airstrike
    /// owns both the relationship and its cancellation/removal lifecycle.
    #[serde(default)]
    pub airstrike: crate::sim::airstrike::AirstrikeState,
    /// Techno+1A8/+1B4/+1C0: outer timer and independent tint phase.
    #[serde(default)]
    pub airstrike_tint: crate::sim::superweapon::airstrike_tint::AirstrikeTintState,
    /// The victim side of mind control (`TechnoClass+0x2C0` MindControlledBy,
    /// `+0x2C8` the ring anim), written only by `capture_manager`.
    #[serde(default)]
    pub mind_control: crate::sim::capture_manager::MindControlLink,
    /// TemporalClass state (`TechnoClass+0x274` TemporalImUsing and `+0x278`
    /// TemporalTargetingMe), written only by `temporal`.
    #[serde(default)]
    pub temporal: crate::sim::temporal::TemporalState,
    /// The Crazy Ivan bomb it carries (`ObjectClass+0x38` and its
    /// `BombClass`), written only by `bomb`.
    #[serde(default)]
    pub bomb: Option<crate::sim::bomb::Bomb>,
    /// Psychedelic/chaos runtime, separate from reversible mind control: a
    /// [`BerserkState`] that `combat::receiver_health` owns.
    ///
    /// [`BerserkState`]: crate::sim::combat::BerserkState
    #[serde(default)]
    pub berserk: crate::sim::combat::BerserkState,
    /// The pending entry (`Unit+0x500`), a [`PendingEntry`] that
    /// `docking::building_dock` owns.
    ///
    /// [`PendingEntry`]: crate::sim::docking::building_dock::PendingEntry
    pub(crate) pending_entry: crate::sim::docking::building_dock::PendingEntry,
    /// Foot `+0x68F..+0x691`, cleared by the Foot constructor
    /// (`0x004D33D2..0x004D33DE`) and written only by `sim::building_entry`.
    #[serde(default)]
    pub(crate) building_entry: crate::sim::building_entry::BuildingEntryLatches,
    /// Shared signed Techno+0x2FC, with aircraft release/dock state where
    /// applicable. The `ammo` owner exposes the count's native mutations.
    ammo: Option<TechnoAmmo>,
    /// Techno+1FC/+204, constructed running at (binary frame,0) by
    /// TechnoClass6F2CE7..6F2CF3. Independent of the weapon rearm clock.
    ammo_reload_timer: crate::sim::timer::CdTimer,
    /// The spot an infantryman holds in its cell, an [`InfantrySpot`] cache
    /// that `movement::foot_mark` owns.
    ///
    /// [`InfantrySpot`]: crate::sim::movement::foot_mark::InfantrySpot
    pub(crate) sub_cell: crate::sim::movement::foot_mark::InfantrySpot,
    /// `Crushable=` and `DeployedCrushable=`, a [`CrushableFacts`] that
    /// construction stamps.
    ///
    /// [`CrushableFacts`]: crate::sim::world::CrushableFacts
    pub(crate) crushable_facts: crate::sim::world::CrushableFacts,
    /// Techno `+0x2A4`, a [`CrushImmunity`] that `sim::deploy` owns.
    ///
    /// [`CrushImmunity`]: crate::sim::deploy::CrushImmunity
    #[serde(default)]
    pub(crate) native_crush_immunity: crate::sim::deploy::CrushImmunity,
    /// `OmniCrusher=`, `Crusher=`, `Accelerates=`, `OmniCrushResistant=` and
    /// `ImmuneToRadiation=`, a [`MoverFacts`] that construction stamps.
    ///
    /// [`MoverFacts`]: crate::sim::world::MoverFacts
    pub(crate) mover_facts: crate::sim::world::MoverFacts,
    /// The dying mark, a [`TerminalLifetime`] that `world::lifecycle` owns.
    /// Products derived from the entities (the movement pass's blocker plane
    /// and block sets) follow it through the store's touch log, like any
    /// other field.
    ///
    /// [`TerminalLifetime`]: crate::sim::world::TerminalLifetime
    pub death: crate::sim::world::TerminalLifetime,
    /// Foot+53C/+540 latch and countdown. The shared Foot sound owner alone
    /// writes these; the process-local audio handle is not simulation state.
    #[serde(default)]
    pub move_sound: crate::sim::world::MoveSoundState,
    /// The crash latch (`FootClass+0x425`/`+0x426`), a [`CrashLatch`] that
    /// `world::crash` owns.
    #[serde(default)]
    pub crash: crate::sim::world::CrashLatch,
    /// Techno+3CD/+3CE retained sinking lifetime and prior Foot AI sound edge.
    /// `world::sinking` owns mutations; this is independent of crash +425/+426.
    #[serde(default)]
    pub sinking: crate::sim::world::SinkingState,

    // --- Passenger/transport system ---
    /// The passenger relation and cargo hold (`TechnoClass+0x114` CargoClass,
    /// `+0x11C` Transporter, `+0x82` InOpenToppedTransport), a
    /// [`PassengerRole`] that `sim::passenger` owns.
    pub passenger_role: PassengerRole,
    /// The image a harvester shows while it unloads, an [`UnloadImage`] that
    /// `miner::refinery_dock` owns.
    ///
    /// [`UnloadImage`]: crate::sim::miner::UnloadImage
    pub display_type_override: crate::sim::miner::UnloadImage,
    /// Techno `+0x338`, a [`CapturedInfantryType`] that `world::world_orders`
    /// owns.
    ///
    /// [`CapturedInfantryType`]: crate::sim::world::CapturedInfantryType
    pub(crate) capture_infantry_type_index: crate::sim::world::CapturedInfantryType,
    #[cfg(feature = "ares")]
    #[serde(default)]
    pub(crate) vehicle_driver: crate::sim::vehicle_theft::VehicleDriverState,
    /// Shared Building C4/PostMortem detonation latch: the marker `+0x6DF`
    /// with its C4Timer (`+0x528`) and C4AppliedBy (`+0x540`). The C4 plant
    /// (`sim::c4_plant`) owns one producer; qualifying delayed-death receiver
    /// results own the other. The Building LogicVector visit consumes expiry
    /// synchronously (`Simulation::tick_pending_building_detonation`) and
    /// keeps the marker, except a bridge repair hut's, which clears it.
    /// IronCurtain/ForceShield entry cancels it. A [`C4Detonation`] that
    /// `sim::c4_plant` owns.
    ///
    /// [`C4Detonation`]: crate::sim::c4_plant::C4Detonation
    #[serde(default)]
    pub(crate) pending_c4_detonation: crate::sim::c4_plant::C4Detonation,
    /// A building's HasBeenCaptured byte (`BuildingClass+0x6E3`), a
    /// [`HasBeenCaptured`] that `crew_survival` owns.
    ///
    /// [`HasBeenCaptured`]: crate::sim::crew_survival::HasBeenCaptured
    #[serde(default)]
    pub(crate) has_been_captured: crate::sim::crew_survival::HasBeenCaptured,
    /// A building's NoSurvivor byte (`BuildingClass+0x6E0`), a
    /// [`NoSurvivor`] that `crew_survival` owns.
    ///
    /// [`NoSurvivor`]: crate::sim::crew_survival::NoSurvivor
    #[serde(default)]
    pub(crate) no_survivor: crate::sim::crew_survival::NoSurvivor,
    /// Infantry+6D9, initialized clear at517A98. Building crew producers
    /// mark it before death Unlimbo443127 or after sale Unlimbo44A747;
    /// placement, ownership transfer and removal never clear it.
    #[serde(default)]
    nominal_survivor: bool,
    /// Both ends of the tank-bunker reciprocal link, a [`BunkerLinks`] that
    /// `docking::bunker_link` owns.
    ///
    /// [`BunkerLinks`]: crate::sim::docking::bunker_link::BunkerLinks
    #[serde(default)]
    pub bunker: crate::sim::docking::bunker_link::BunkerLinks,
    /// Runtime state for `Gate=yes` building passability.
    ///
    /// Native gate passability4525F0 accepts only mission `0x18` plus stable-open helper
    /// state. Opening and closing gates are still blockers for the same check.
    #[serde(default)]
    pub(crate) building_gate: Option<BuildingGateRuntime>,
    /// Shared Techno+350 door. Gate/factory/Unit consumers mutate it only
    /// through this entity's DoorClass operations.
    #[serde(default)]
    door: crate::sim::door::DoorClass,
    /// Tank-bunker install state machine. `Some` on `Bunker=yes` buildings from
    /// spawn (state `Idle` when empty); its presence marks the entity as a tank
    /// bunker. Drives entry admission → install.
    #[serde(default)]
    pub bunker_runtime: Option<crate::sim::docking::bunker_install::BunkerRuntime>,
    /// The deploy animation (`TechnoClass+0x130`) and Jumpjet landing request
    /// (`+0x134`), a [`DeployAttachment`] that `game_entity::simple_deploy`
    /// owns.
    ///
    /// [`DeployAttachment`]: simple_deploy::DeployAttachment
    deploy_attachment: simple_deploy::DeployAttachment,
    /// The Behind indicator Anim (`TechnoClass+0x12C`) that
    /// `game_entity::behind_indicator` owns.
    behind_anim: Option<crate::sim::anim_class::AnimId>,
    /// Unit+0x68C: runtime Deploy continuation, not the type's DeployToFire;
    /// a [`DeployContinuation`] that `mcv_deploy` owns.
    ///
    /// [`DeployContinuation`]: crate::sim::mcv_deploy::DeployContinuation
    #[serde(default)]
    pub(crate) mcv_deploy_pending: crate::sim::mcv_deploy::DeployContinuation,
    /// Infantry fear/prone runtime. `None` for non-infantry entities.
    #[serde(default)]
    pub(crate) infantry: Option<crate::sim::infantry::InfantryRuntime>,
    /// Optional body-rocking state only. Drive/Ship slope transitions belong
    /// to their typed locomotor payloads. This defaults to `None` and becomes
    /// present only when an explicit body-rocking producer activates it.
    #[serde(default)]
    pub rocking: Option<crate::sim::rocking::RockingState>,
    /// Exact native-width Mission state, owned by `sim::mission`: all writes
    /// pass through a named legacy compatibility adapter or the dormant
    /// exact-authority surface.
    pub mission: MissionCom,
    /// The passive target scan's timer, last frame and acquisition mark
    /// (`TechnoClass+0x180`/`+0x188`, `+0x4FC`, `+0x50C`), a [`PassiveScan`]
    /// that `techno_ai::target_scan` owns.
    ///
    /// [`PassiveScan`]: crate::sim::world::PassiveScan
    #[serde(default)]
    pub passive_scan: crate::sim::world::PassiveScan,
    /// Foot `+0x688`, a [`RetargetAfterStop`] that `combat::greatest_threat`
    /// owns.
    ///
    /// [`RetargetAfterStop`]: crate::sim::combat::greatest_threat::RetargetAfterStop
    #[serde(default)]
    pub(crate) foot_retarget_after_stop: crate::sim::combat::greatest_threat::RetargetAfterStop,
    /// Category-specific bytes read by Mission readiness and Aircraft policy.
    pub mission_leaf: MissionLeafState,
    /// SuspendedTarCom (`TechnoClass+0x2B8`), a [`SuspendedTarget`] that
    /// `mission::authority` owns.
    ///
    /// [`SuspendedTarget`]: crate::sim::mission::authority::SuspendedTarget
    pub suspended_attack_target: crate::sim::mission::authority::SuspendedTarget,
    /// Active House/base-defence recruitment, archive and attacker-cooldown
    /// bytes. Snapshot migration defaults reproduce Techno construction.
    #[serde(default)]
    pub(crate) base_defense_response: BaseDefenseResponseState,
    /// The actual Spark system link (`TechnoClass+0x308`), shared by
    /// FireAt and Cyborg AI; particles::spawn owns its mutations.
    #[serde(default)]
    pub(crate) spark_particle_system: crate::sim::particles::spawn::AttachedSpark,
    /// The damage-Smoke system handle (`TechnoClass+0x310`), a
    /// [`DamageSmoke`] that `particles::spawn` owns.
    ///
    /// [`DamageSmoke`]: crate::sim::particles::spawn::DamageSmoke
    #[serde(default)]
    pub damage_smoke_system_id: crate::sim::particles::spawn::DamageSmoke,
    /// The passengers a transport's Unload keeps aboard (`UnitClass+0x6E4`),
    /// an [`UnloadKeep`] that `transport_unload` owns.
    ///
    /// [`UnloadKeep`]: crate::sim::transport_unload::UnloadKeep
    #[serde(default)]
    pub(crate) transport_unload_keep_count: crate::sim::transport_unload::UnloadKeep,
    /// Oil derricks' ProduceCash timer ([`credit_income`](crate::sim::credit_income)).
    #[serde(default)]
    pub(crate) produce_cash: crate::sim::credit_income::ProduceCash,
    /// The Floating Disc drain link pair ([`credit_income`](crate::sim::credit_income)).
    #[serde(default)]
    pub(crate) drain: crate::sim::credit_income::Drain,
    /// The parasite fields: the ParasiteClass a dog or drone owns, the eater
    /// attached to a victim, paralysis ([`parasite`](crate::sim::combat::parasite)).
    #[serde(default)]
    pub parasite: crate::sim::combat::parasite::FootParasite,
    /// Techno `+0x508`, a [`SpatialThreat`] that `sim::house_threat` owns.
    ///
    /// [`SpatialThreat`]: crate::sim::house_threat::SpatialThreat
    #[serde(default)]
    pub(crate) cached_spatial_threat: crate::sim::house_threat::SpatialThreat,
}

mod behind_indicator;
mod construction_stage;
mod gunner;
mod simple_deploy;
mod voxel_recoil;

#[cfg(all(test, not(feature = "ares")))]
#[test]
fn default_build_rejects_saved_delivery_base_override_and_keeps_base_space() {
    let mut entity = GameEntity::test_default_of_category(
        1,
        "YARD",
        "Americans",
        20,
        20,
        EntityCategory::Structure,
    );
    assert!(entity.validate_ares_build_compatibility().is_ok());
    entity.unit_delivery_no_base = true;
    assert_eq!(
        entity.validate_ares_build_compatibility(),
        Err("delivery base overrides require a build with the ares feature")
    );
    assert!(entity.unit_delivery_provides_base_space());
}

impl GameEntity {
    /// Read-only Phobos shield state for presentation. Construction and the
    /// shield owner retain simulation-side mutation authority.
    #[cfg(feature = "phobos")]
    pub fn shield(&self) -> Option<&crate::sim::shield::ShieldState> {
        self.shield.as_ref()
    }

    /// Infantry+6D9: building survivor identity, not Type.Nominal.
    /// Added_To_Game502C3C skips its active counter; UI-name51F2C0 uses
    /// TXT_TECHNICIAN. The flag persists across placement and capture.
    pub const fn is_nominal_survivor(&self) -> bool {
        self.nominal_survivor
    }

    /// The two building crew producers' stores443127 and44A747.
    pub(crate) fn mark_nominal_survivor(&mut self) {
        debug_assert_eq!(self.category, EntityCategory::Infantry);
        self.nominal_survivor = true;
    }

    #[cfg(feature = "ares")]
    pub(crate) fn set_unit_delivery_base_normal(&mut self, base_normal: bool) {
        self.unit_delivery_no_base = self.category() == EntityCategory::Structure && !base_normal;
    }

    pub(crate) fn hunter_seeker_origin(
        &self,
    ) -> Option<crate::sim::hunter_seeker::HunterSeekerOrigin> {
        self.hunter_seeker_origin
    }

    #[cfg(feature = "ares")]
    pub(crate) fn set_hunter_seeker_origin(
        &mut self,
        origin: crate::sim::hunter_seeker::HunterSeekerOrigin,
    ) {
        self.hunter_seeker_origin = Some(origin);
    }

    /// Saved extension effects must be admitted by the build that resumes them.
    /// Keep the raw delivery override private to its entity owner.
    pub(crate) fn validate_ares_build_compatibility(&self) -> Result<(), &'static str> {
        #[cfg(not(feature = "ares"))]
        if self.unit_delivery_no_base {
            return Err("delivery base overrides require a build with the ares feature");
        }
        #[cfg(not(feature = "ares"))]
        if self.hunter_seeker_origin.is_some() {
            return Err("hunter seeker launch sources require a build with the ares feature");
        }
        Ok(())
    }

    pub(crate) fn unit_delivery_provides_base_space(&self) -> bool {
        #[cfg(feature = "ares")]
        {
            !self.unit_delivery_no_base
        }
        #[cfg(not(feature = "ares"))]
        {
            true
        }
    }

    /// Infantry519524/51958A and Unit73CEF3 subtract Foot+6B4 in DrawIt.
    pub fn ion_blast_lift_pixels(&self) -> i8 {
        self.ion_blast_lift
    }

    /// IonBlast (`0x0053CF7F`) stores the ripple's signed pixel lift.
    pub(crate) fn lift_by_ion_blast(&mut self, pixels: i8) {
        self.ion_blast_lift = pixels;
    }

    /// FootAI (`0x004DAECA`) clears the transient draw lift.
    pub(crate) fn clear_ion_blast_lift(&mut self) {
        self.ion_blast_lift = 0;
    }

    /// The infantry house-total latches (`Techno+0x438/+0x439`). Only
    /// `sim::house_tracking` can change them; the mutable borrow exists so
    /// that owner can reach the latches stored on the object.
    pub(crate) fn house_tracking_latches(
        &self,
    ) -> crate::sim::house_tracking::InfantryTrackingLatches {
        self.house_tracking_latches
    }

    pub(crate) fn house_tracking_latches_mut(
        &mut self,
    ) -> &mut crate::sim::house_tracking::InfantryTrackingLatches {
        &mut self.house_tracking_latches
    }

    /// The shared signed Techno ammo, including negative/unlimited counts.
    pub fn ammo(&self) -> Option<&TechnoAmmo> {
        self.ammo.as_ref()
    }

    /// Mutations remain on the shared ammo owner, rather than a caller's copy.
    pub(crate) fn ammo_mut(&mut self) -> Option<&mut TechnoAmmo> {
        self.ammo.as_mut()
    }

    /// Building442C81, Unit735592, Infantry517D69 and Aircraft414033 writes.
    pub(crate) fn initialize_ammo_from_type(
        &mut self,
        object: &crate::rules::object_type::ObjectType,
    ) {
        self.ammo = Some(TechnoAmmo::from_type(object));
    }

    /// The authoritative Techno reload clock, preserved verbatim in saves.
    pub const fn ammo_reload_timer(&self) -> crate::sim::timer::CdTimer {
        self.ammo_reload_timer
    }

    /// Techno ReceiveDamage701ADB..701B62, after the early immunity gates
    /// and before the bunker/warhead gates. StartReloading is unconditional
    /// after the count write; its own full-count check decides the timer.
    pub(crate) fn reduce_ammo_for_damage(
        &mut self,
        object: &crate::rules::object_type::ObjectType,
        damage: i32,
        now: i32,
    ) {
        if !object.damage_reduces_readiness {
            return;
        }
        if let Some(ammo) = self.ammo_mut() {
            ammo.reduce_readiness(
                damage,
                object.strength,
                object.ammo,
                object.readiness_reduction_multiplier,
            );
            self.start_reloading_ammo(object, now);
        }
    }

    /// TechnoClass::StartReloading6FB080: current type/count, signed raw
    /// frames and wrapping integer products; a full count leaves the clock.
    pub(crate) fn start_reloading_ammo(
        &mut self,
        object: &crate::rules::object_type::ObjectType,
        now: i32,
    ) {
        let Some(ammo) = self.ammo() else {
            return;
        };
        let current = ammo.current();
        if current >= object.ammo {
            return;
        }
        let duration = if current == 0 && object.empty_reload != -1 {
            object.empty_reload
        } else {
            let quotient = if object.pip_wrap() == 0 {
                1
            } else {
                // Native IDIV likewise faults for MIN/-1; the ordinary
                // signed quotient truncates toward zero.
                current / object.pip_wrap()
            };
            object.reload.wrapping_add(
                object
                    .reload_increment
                    .wrapping_mul(quotient)
                    .wrapping_mul(quotient),
            )
        };
        self.ammo_reload_timer.start(now, duration);
    }

    /// UnitClass::Reload736CA0, reached at UnitAI736689 only. MobileFire=no
    /// uses ILocomotion+10; a moving positive clock is re-anchored, not paused.
    /// The inherited Techno Reload6FB010 then adds at most one round.
    pub(crate) fn reload_unit_ammo(
        &mut self,
        object: &crate::rules::object_type::ObjectType,
        now: i32,
    ) -> Result<(), String> {
        if !object.mobile_fire {
            let moving = crate::sim::movement::motion_query::is_moving(self).ok_or_else(|| {
                "Unit Reload736CA0 requires ILocomotion+10 (E_POINTER)".to_owned()
            })?;
            let remaining = self.ammo_reload_timer.remaining(now);
            if moving && remaining > 0 {
                self.ammo_reload_timer.start(now, remaining.wrapping_add(1));
            }
        }
        self.reload_techno_ammo(object, now);
        Ok(())
    }

    /// gamemd.exe TechnoClass::Reload0x006FB010, shared by Unit's override
    /// and Ares InfantryClass_Update_Reload10047980 (gamemd51BCB2).
    pub(crate) fn reload_techno_ammo(
        &mut self,
        object: &crate::rules::object_type::ObjectType,
        now: i32,
    ) {
        if object.ammo != -1
            && self.ammo().is_some_and(|ammo| ammo.current() < object.ammo)
            && self.ammo_reload_timer.remaining(now) == 0
        {
            self.ammo_mut().unwrap().reload_round();
            // Foot/Unit Mark(2)4D3780 returns immediately, before any
            // callback or spatial write. No redraw event is synthesized.
            self.start_reloading_ammo(object, now);
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn install_ammo_for_test(&mut self, ammo: Option<TechnoAmmo>) {
        self.ammo = ammo;
    }

    /// Techno+0x284, the Chronosphere warp-in's frames.
    pub(crate) fn chrono_warp_delay(&self) -> i32 {
        self.chrono_warp_delay.get()
    }

    /// A fixture's delay.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_chrono_warp_delay(&mut self, frames: i32) {
        self.chrono_warp_delay.store(frames);
    }
}

impl GameEntity {
    /// Native703860 base decision through the shared cloak owner. Accepted
    /// Technos have one real owner; retained +41A belongs to discovery, not cloak.
    /// Building4544A0 delegates here only with its +6ED override stage zero.
    /// The nonzero building override has no Rust producer and remains separate.
    pub fn visual_character(
        &self,
        cloaking_stages: i32,
        invisible: bool,
        query: crate::sim::cloak_disguise::VisualCharacterQuery,
    ) -> u8 {
        let (state, progress) = self
            .cloak
            .as_ref()
            .map_or((0, 0), |c| (c.state(), c.depth() as i32));
        crate::sim::cloak_disguise::visual_character(
            state,
            progress,
            cloaking_stages,
            invisible,
            self.category == EntityCategory::Structure,
            self.discovery.owned_by_current_house(),
            true,
            query,
        )
    }

    /// `Techno70BE50`: actual COM410220 reads Abstract+10, then native7C5F00
    /// truncates retained+24C. Wrapping signed addition precedes IDIV400.
    /// Offset is RGB565 words, not world coordinates or a per-frame RNG draw.
    /// Original executed controls: translucent_blitter_a.json/ cloak_offsets.
    pub fn native_cloak_offset_words(&self) -> i32 {
        use crate::util::native_x87::{NativeF32Bits, X87Chop53};
        let displacement = if self.native_cloak_displacement == NativeF32Bits::POSITIVE_ZERO {
            0
        } else {
            X87Chop53::load_f32(self.native_cloak_displacement)
                .map(X87Chop53::ftol_i32_low_masked)
                .unwrap_or(0)
        };
        self.native_unique_id.wrapping_add(displacement) % 400
    }

    pub(crate) fn door_phase(&self) -> crate::sim::door::DoorPhase {
        self.door.phase()
    }

    pub(crate) fn door_draw_progress(&self, frame: u32) -> f64 {
        self.door.draw_progress(frame)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn door_timer_fields(&self) -> (i32, i32, i32) {
        self.door.timer_fields()
    }

    pub(crate) fn open_door(&mut self, ticks: u32, frame: u32) {
        self.door.open(ticks, frame);
    }

    pub(crate) fn close_door(&mut self, ticks: u32, frame: u32) {
        self.door.close(ticks, frame);
    }

    pub(crate) fn reverse_door(&mut self, frame: u32) {
        self.door.reverse(frame);
    }

    pub(crate) fn advance_door(&mut self, frame: u32) {
        self.door.advance(frame);
    }

    pub(crate) fn hash_door_state(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        self.door.hash(hasher);
    }

    ///Building4525F0: MissionClass owns Open24, DoorClass owns stable-open.
    pub(crate) fn is_open_gate(&self) -> bool {
        self.mission.effective().known() == Some(crate::sim::mission::MissionType::Open)
            && self.door_phase() == crate::sim::door::DoorPhase::OpenStable
    }

    /// The pending entry (`Unit+0x500`); see [`PendingEntry`].
    ///
    /// [`PendingEntry`]: crate::sim::docking::building_dock::PendingEntry
    pub const fn pending_entry(&self) -> Option<u64> {
        self.pending_entry.get()
    }

    /// A fixture's pending entry.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_pending_entry(&mut self, target: Option<u64>) {
        match target {
            Some(building) => self.pending_entry.install(building),
            None => self.pending_entry.clear(),
        }
    }

    /// Techno `+0x508`; see [`SpatialThreat`].
    ///
    /// [`SpatialThreat`]: crate::sim::house_threat::SpatialThreat
    pub(crate) const fn cached_spatial_threat(&self) -> Option<i32> {
        self.cached_spatial_threat.get()
    }
    pub fn native_stage(&self) -> &crate::sim::stage::StageClass {
        &self.stage
    }

    /// Read native Doing and shared +F8 for SHP drawing, including actions
    /// absent from the generic sequence vocabulary. Presentation never advances
    /// this clock or owns a writable copy of its progress. Doing-1 is retained;
    /// the frame-selector caller owns its native default-action selection.
    /// Serves the Infantry draw-frame selector `0x00518D80`: Doing `+0x6C4`, and
    /// the Stage value `+0xF8` it takes modulo the count (`0x00518E08..0x00518E21`).
    pub fn infantry_sprite_pose(&self) -> Option<(i32, i32)> {
        if !crate::sim::movement::infantry_action::doing_owns_sequence(self) {
            return None;
        }
        Some((self.mission_leaf.as_infantry()?.doing(), self.stage.value()))
    }

    /// Advance the common clock at this class's native scheduling point.
    pub(crate) fn tick_native_stage(&mut self, now: i32) -> bool {
        self.stage.advance(now)
    }

    /// Start a class action/control without altering independent FC/110 state.
    pub(crate) fn restart_native_stage(&mut self, value: i32, now: i32, rate: i32) {
        self.stage.restart(value, now, rate);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_native_stage_value(&mut self, value: i32) {
        self.stage.set_value(value);
    }

    #[cfg(not(any(test, feature = "test-support")))]
    pub(crate) fn set_native_stage_value(&mut self, value: i32) {
        self.stage.set_value(value);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn install_native_stage_fixture(&mut self, stage: crate::sim::stage::StageClass) {
        self.stage = stage;
    }

    pub fn building_upgrades(&self) -> &crate::sim::building_upgrades::BuildingUpgrades {
        &self.building_upgrades
    }

    pub(crate) fn advance_building_upgrade_level(&mut self, capacity: i32) -> bool {
        self.building_upgrades.power_up(capacity)
    }

    pub(crate) fn commit_building_upgrade_type(&mut self, type_id: InternedId) {
        self.building_upgrades.commit_type(type_id);
    }

    pub(crate) fn remove_building_upgrade_level(&mut self) -> Option<usize> {
        self.building_upgrades.power_down()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_building_upgrades_for_test(
        &mut self,
        count: i8,
        types: [Option<InternedId>; 3],
    ) {
        self.building_upgrades =
            crate::sim::building_upgrades::BuildingUpgrades::fixture(count, types);
    }

    /// Invalidate the one live Foot path head. Native callers write
    /// Foot+5E0=-1; retained suffix/reference words and the destination
    /// remain intact.
    pub(crate) fn clear_live_path_head(&mut self) {
        self.navigation.path_replay_mut().clear_live_head();
    }

    /// The stored playfield membership (`TechnoClass+0x3D5`).
    pub fn in_playfield(&self) -> bool {
        self.in_playfield.get()
    }

    /// A fixture's playfield membership.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_in_playfield(&mut self, in_playfield: bool) {
        self.in_playfield.store(in_playfield);
    }

    /// `Object+0x8C`, OnBridge; see the field.
    pub fn on_bridge(&self) -> bool {
        self.on_bridge.get()
    }

    /// A fixture's OnBridge.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_on_bridge(&mut self, on_bridge: bool) {
        self.on_bridge.store(on_bridge);
    }

    /// The spot an infantryman holds in its cell; see the field.
    /// VERA-only: the `InfantrySpot` placement cache; natively the coordinate's
    /// in-cell offset carries the spot (`0x00481180`).
    pub(crate) fn sub_cell(&self) -> Option<u8> {
        self.sub_cell.get()
    }

    /// A fixture's spot: held, or released for `None`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_sub_cell(&mut self, sub_cell: Option<u8>) {
        match sub_cell {
            Some(spot) => self.sub_cell.hold(spot),
            None => self.sub_cell.release(),
        }
    }

    /// `Foot+0x6B6`, the occupation enable; see the field.
    pub(crate) fn foot_occupation_enabled(&self) -> bool {
        self.foot_occupation_enabled.get()
    }

    /// A fixture's occupation enable.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_foot_occupation_enabled(&mut self, enabled: bool) {
        if enabled {
            self.foot_occupation_enabled.resume();
        } else {
            self.foot_occupation_enabled.suspend();
        }
    }

    /// `TechnoClass::ArchiveTarget` (`Techno+0x218`): the base-defence
    /// responder's post, a harvester's archived ore cell and a factory's
    /// rally point share this one field, stored in
    /// [`BaseDefenseResponseState`].
    pub fn archive_target(&self) -> Option<crate::sim::combat::TargetKind> {
        self.base_defense_response.archive_target()
    }

    /// `TechnoClass::Set_ArchiveTarget @ 0x0070C610`, a plain store.
    pub fn set_archive_target(&mut self, target: Option<crate::sim::combat::TargetKind>) {
        self.base_defense_response.set_archive_target(target);
    }

    /// Techno `+0x4F8`; see [`DeployVoiceSuppression`].
    ///
    /// [`DeployVoiceSuppression`]: crate::sim::world::DeployVoiceSuppression
    pub fn deploy_voice_suppressed(&self) -> bool {
        self.suppress_deploy_voice.get()
    }

    /// A factory's rally point is its ArchiveTarget: `BuildingClass::
    /// SetRallyPoint @ 0x00443860` archives the clicked cell through event
    /// 0x1E (`Set_ArchiveTarget`), and `BuildingClass::ExitObject_Main @
    /// 0x00443C60` reads `+0x218` for the object leaving it. The shared
    /// Building destination setter (`0x00455D50`) can also replace this
    /// archive; Stop reaches its null arm and removes the rally.
    pub(crate) fn rally_cell(&self) -> Option<(u16, u16)> {
        match self.archive_target() {
            Some(crate::sim::combat::TargetKind::Cell(rx, ry)) => Some((rx, ry)),
            _ => None,
        }
    }

    /// Techno `+0x3D4`; see [`MissionOnly`].
    ///
    /// [`MissionOnly`]: crate::sim::aircraft::MissionOnly
    pub const fn is_mission_only(&self) -> bool {
        self.mission_only.get()
    }

    /// AircraftUnlimbo4143A8..4143F2, only after Foot placement succeeds.
    /// GetWeapon(0) uses the same tier/slot authority as production combat.
    pub(crate) fn retain_aircraft_unlimbo_control(
        &mut self,
        rules: &crate::rules::ruleset::RuleSet,
        type_id: &str,
    ) {
        if self.category != EntityCategory::Aircraft {
            return;
        }
        let Some(object) =
            rules.object_in_category(crate::rules::object_type::ObjectCategory::Aircraft, type_id)
        else {
            return;
        };
        let camera =
            crate::sim::combat::combat_weapon::primary_for_tier(object, self.veterancy.rank())
                .and_then(|id| rules.weapon(id))
                .is_some_and(|weapon| weapon.camera);
        if !object.selectable || !object.landable || camera {
            self.mission_only.mark();
        }
    }

    /// The rest of AircraftClass::Unlimbo's tail, after the retained +3D4
    /// ([`Self::retain_aircraft_unlimbo_control`]): the Stage restarts at 0
    /// with rate 1 (`0x0041441C..0x00414444`), then SetSpeedFraction
    /// (vt+0x544, Foot `0x004D3710`) takes 1.0 when GetHeight (vt+0x1C8) is
    /// the type's FlightLevel (vt+0xBC), else 0 (`0x0041444E..0x0041448A`).
    /// Native comparison: tools/spatial_oracle/aircraft_unlimbo_height.json.
    pub(crate) fn finish_aircraft_unlimbo(&mut self, height: i32, flight_level: i32, now: i32) {
        self.stage.restart(0, now, 1);
        let fraction = if height == flight_level {
            crate::util::fixed_math::SIM_ONE
        } else {
            crate::util::fixed_math::SIM_ZERO
        };
        self.foot_speed.set_speed_fraction(fraction);
    }

    /// Immutable storage key. Construction and snapshot decoding establish it.
    pub fn stable_id(&self) -> u64 {
        self.stable_id
    }

    /// Indexed ownership. Live transfers go through Simulation's owner lifecycle
    /// and EntityStore's index update; ordinary payload mutation cannot assign it.
    pub fn owner(&self) -> InternedId {
        self.owner
    }

    /// Constructor House identity, preserved across ownership transfers and load.
    pub(crate) fn initial_owner(&self) -> InternedId {
        self.initial_owner
    }

    #[cfg(feature = "ares")]
    pub(crate) fn original_factory_country(&self) -> Option<crate::rules::ruleset::CountryIdx> {
        self.original_factory_country
    }

    /// InitManagers1004A776 and the conversion10045B54 use the same retained
    /// identity; ordinary ownership changes never call this writer.
    #[cfg(feature = "ares")]
    pub(crate) fn initialize_factory_country(
        &mut self,
        country: Option<crate::rules::ruleset::CountryIdx>,
    ) {
        self.original_factory_country = country;
    }

    pub(crate) const fn building_loss_index(&self) -> i32 {
        self.building_loss_index
    }

    /// Building InitManagers' retained Country index (442C5C).
    pub(crate) fn initialize_building_loss_index(&mut self, country_index: i32) {
        self.building_loss_index = country_index;
    }

    /// Building ReceiveDamage's nonnull source House index (44298B).
    pub(crate) fn record_building_damage_house_index(&mut self, house_index: i32) {
        self.building_loss_index = house_index;
    }

    /// The sale's accounting gate precedes its live destruction callbacks.
    pub(crate) fn suppress_building_sale_loss(&mut self) {
        self.building_loss_index = -1;
    }

    /// Indexed type identity, established by construction/decoding and changed
    /// only by the same-object type-conversion owner.
    pub fn type_ref(&self) -> InternedId {
        self.type_ref
    }

    pub(crate) fn secret_lab_fallback(
        &self,
    ) -> Option<(crate::rules::object_type::ObjectCategory, InternedId)> {
        self.secret_lab_fallback
    }

    pub(in crate::sim) fn assign_secret_lab_fallback(
        &mut self,
        production: (crate::rules::object_type::ObjectCategory, InternedId),
    ) {
        self.secret_lab_fallback = Some(production);
    }

    /// One rules-to-model binding for construction and live type changes.
    pub(crate) fn object_uses_voxel(
        type_id: &str,
        object: &crate::rules::object_type::ObjectType,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> bool {
        rules
            .art()
            .resolve_metadata_entry(type_id, &object.image)
            .map(|entry| entry.voxel)
            .unwrap_or(matches!(
                object.category,
                crate::rules::object_type::ObjectCategory::Vehicle
                    | crate::rules::object_type::ObjectCategory::Aircraft
            ))
    }

    /// A VXL/HVA model (true) or an SHP sprite.
    pub const fn is_voxel(&self) -> bool {
        self.is_voxel
    }

    /// Install a fixture's model kind.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_is_voxel_for_test(&mut self, is_voxel: bool) {
        self.is_voxel = is_voxel;
    }

    /// The concrete class, fixed at construction.
    pub const fn category(&self) -> EntityCategory {
        self.category
    }

    /// AbstractClass+10, the native UniqueID.
    pub(crate) const fn native_unique_id(&self) -> i32 {
        self.native_unique_id
    }

    /// The TechnoClass constructor's retained Scenario word (`+0x3C8`).
    pub const fn techno_ctor_random_word(&self) -> u16 {
        self.techno_ctor_random_word
    }

    /// The native signed Techno+214 value, shared by Recruit6EAA90 and
    /// TeamAssign731060. It is independent of live Team membership.
    pub const fn object_group(&self) -> i32 {
        self.object_group
    }

    /// Callers own admission/order; scenario readers and
    /// TeamType::Get_Group6F1870 supply raw signed values.
    pub(crate) fn assign_object_group(&mut self, group: i32) {
        self.object_group = group;
    }

    pub(crate) const fn production_start_price(&self) -> i32 {
        self.production_start_price
    }

    /// Factory StartProduction4C9DED writes this once on a newly created
    /// product. Hold/resume, placement and owner changes retain it (gamemd).
    pub(crate) fn record_production_start_price(&mut self, price: i32) {
        self.production_start_price = price;
    }
}

/// Fixture setup for the identity a constructor or decoder establishes.
#[cfg(any(test, feature = "test-support"))]
impl GameEntity {
    pub fn set_object_group_for_test(&mut self, group: i32) {
        self.assign_object_group(group);
    }

    pub fn set_stable_id_for_test(&mut self, stable_id: u64) {
        self.stable_id = stable_id;
    }

    /// Supply the building crew producer's retained marker in a UI fixture.
    pub fn mark_nominal_survivor_for_test(&mut self) {
        self.mark_nominal_survivor();
    }

    pub(crate) fn set_native_unique_id_for_test(&mut self, native_unique_id: i32) {
        self.native_unique_id = native_unique_id;
    }

    pub(crate) fn set_techno_ctor_random_word_for_test(&mut self, word: u16) {
        self.techno_ctor_random_word = word;
    }

    /// Before insertion only: supply the constructor and current House
    /// together. A stored entity changes only its current owner through
    /// [`EntityStore::change_owner`](crate::sim::entity_store::EntityStore::change_owner),
    /// which keeps the store's owner column.
    pub fn set_owner_for_test(&mut self, owner: InternedId) {
        self.owner = owner;
        self.initial_owner = owner;
    }

    pub fn set_type_ref_for_test(&mut self, type_ref: InternedId) {
        self.type_ref = type_ref;
    }

    /// Changes only the class tag. A fixture that needs another class's
    /// private state (a Building's body, an Infantry's runtime) constructs it
    /// with [`Self::test_default_of_category`] instead.
    pub fn set_category_for_test(&mut self, category: EntityCategory) {
        self.category = category;
    }
}

impl GameEntity {
    /// Foot `+0x688`; see [`RetargetAfterStop`].
    ///
    /// [`RetargetAfterStop`]: crate::sim::combat::greatest_threat::RetargetAfterStop
    pub const fn foot_retarget_after_stop(&self) -> bool {
        self.foot_retarget_after_stop.get()
    }

    /// The body heading at `frame`: `FacingClass::Current @ 0x004C93D0` on
    /// `+0x388`.
    pub fn body_facing_current(&self, frame: u32) -> u16 {
        self.body_facing.current(frame)
    }

    /// The body heading's high byte at `frame`, the 8-bit direction a reader
    /// that drops the DirStruct's low byte sees.
    pub fn body_facing_byte(&self, frame: u32) -> u8 {
        (self.body_facing.current(frame) >> 8) as u8
    }

    /// The body heading at `frame` as a rounded DirType byte,
    /// `((raw >> 7) + 1) >> 1`: the conversion gamemd applies when it passes
    /// `Current()` on as a direction argument (an Unlimbo's, a Do_Turn
    /// comparison's), wrapping 0xFF80.. to 0.
    /// `+0x388` (PrimaryFacing) through `FacingClass::Current @ 0x004C93D0`.
    pub(crate) fn body_facing_dir(&self, frame: u32) -> u8 {
        crate::util::direction_tables::round_facing16_to_8(self.body_facing.current(frame))
    }

    /// The barrel elevation (`+0x370`); its readers sample `current(frame)`
    /// (`FacingClass::Current @ 0x004C93D0`).
    pub fn barrel_elevation(&self) -> &crate::sim::movement::FacingClass {
        &self.barrel_elevation
    }

    /// Whether the barrel elevation still holds its constructor value, which
    /// an object that never unlimboed keeps.
    pub(crate) fn barrel_elevation_is_constructed(&self) -> bool {
        self.barrel_elevation == crate::sim::movement::FacingClass::new(0, BARREL_ELEVATION_ROT)
    }

    /// `TechnoClass::Unlimbo`'s barrel writes, right after its body snap
    /// (`0x006F6DAA`): the elevation snaps level (`Set_Current(0x4000)` at
    /// `0x006F6DC3`), then turns toward the type's `FireAngle=` (`Set` at
    /// `0x006F6DF5`, [`unlimbo_barrel_target`]).
    pub(crate) fn unlimbo_barrel_elevation(&mut self, fire_angle: i32, frame: u32) {
        self.barrel_elevation.snap(BARREL_LEVEL, frame);
        self.barrel_elevation
            .set(unlimbo_barrel_target(fire_angle), frame);
    }

    /// vt+0x148 `Flash(frames)` for a building or aircraft, whose
    /// IsDisguisedTo (vt+0xC8, `0x0041C020`) answers the object's own
    /// disguise flag (`+0x1D8`) and GetDisguiseHouse (vt+0xD0, `0x0041BE70`)
    /// none, whoever asks. `TechnoClass::Flash @ 0x006F9DD0` then starts the
    /// flash countdown (`+0xF0`) unless that flag is up. A building's
    /// `0x00456E00` adds the redraw flag (`+0x80`) and relights its slot
    /// anims ([`crate::sim::world::ObjectSubstrate::flash_building`]). Foot
    /// objects go through [`crate::sim::world::Simulation::flash_object`].
    pub(crate) fn flash_base(&mut self, frames: i32) {
        if !self
            .disguise
            .as_ref()
            .is_some_and(|disguise| disguise.is_disguised())
        {
            self.flashing.start(frames);
        }
    }

    /// The class constructor's one rate write on `+0x388` (`Set_ROT @
    /// 0x004C9680`): the constant 127 for infantry (`0x00517BC5`), `ROT=`
    /// (`Type+0x71C`) for a unit (`0x00735579`), an aircraft (`0x00413FE7`)
    /// and a building (`BuildingClass::Init` at `0x00442CA5`).
    pub(crate) fn set_body_facing_rot(&mut self, type_rot: i32) {
        self.body_facing
            .set_rot(if self.category == EntityCategory::Infantry {
                127
            } else {
                type_rot
            });
    }

    /// Spawn owns Aircraft initialization. Legacy/headless movers may lack the
    /// Secondary controller; initialize it once from the body's heading without
    /// replacing an existing live turn. The body's rate stands in for `ROT=`:
    /// the Aircraft constructor (`0x00413FD2..0x00414015`) gives both
    /// controllers that one value.
    pub fn ensure_fly_secondary_facing(&mut self) {
        let initial = self.body_facing.destination();
        let body = &self.body_facing;
        self.barrel_facing
            .get_or_insert_with(|| crate::sim::movement::FacingClass::with_rate_of(initial, body));
    }

    pub(crate) fn set_owner_from_store(
        &mut self,
        owner: InternedId,
        _: crate::sim::entity_store::OwnerChangeAuthority,
    ) {
        self.owner = owner;
    }

    /// The Harvest FSM cursor of record, decoded from
    /// `MissionCom::handler_state`. `None` when the entity has no Miner
    /// component. An out-of-vocabulary cursor decodes as `SearchOre` (the
    /// zeroed handler state every mission transition writes) — the only
    /// writers are the FSM commit and the zeroing transitions, so anything
    /// else is a logic error surfaced by the dispatch-time debug assert.
    pub fn miner_state(&self) -> Option<crate::sim::miner::MinerState> {
        self.miner.as_ref().map(|_| {
            crate::sim::miner::MinerState::from_cursor(self.mission.handler_state())
                .unwrap_or(crate::sim::miner::MinerState::SearchOre)
        })
    }

    /// The type's `Harvester=` (`+0xE0E`): a War or Chrono Miner, which runs
    /// the harvest and refinery dock missions. The Slave Miner carries a Miner
    /// component only as an order marker (`sim::slave_manager`), so it is not
    /// one.
    pub fn is_harvester(&self) -> bool {
        self.miner.as_ref().is_some_and(|miner| {
            matches!(
                miner.kind(),
                crate::sim::miner::MinerKind::War | crate::sim::miner::MinerKind::Chrono
            )
        })
    }

    /// The native Techno+3D0 useless latch read by Repair depots.
    pub(crate) const fn is_useless(&self) -> bool {
        self.useless
    }

    /// Ares TechnoExt+9C. Gameplay and drawing read the same driver state.
    #[cfg(feature = "ares")]
    pub fn driver_killed(&self) -> bool {
        self.vehicle_driver.killed()
    }

    /// `Mission_Harvest`'s two writes to Techno+3D0 (`0x0073E8FA`,
    /// `0x0073E925`). Other systems only read this shared Techno state.
    pub(crate) fn set_useless(&mut self, useless: bool) {
        self.useless = useless;
    }

    /// The committed mission read at TechnoAI6FA697, before its class's
    /// subsequent Ready/Commence checkpoint. Clearing a target or destination
    /// does not change this selector; only the mission owner can commence a
    /// queued Guard. Native FV bridge-collapse execution at Logic45 rejects
    /// Attack here and consumes no scan draw (`fv_cell_attack` continuation).
    pub fn passive_acquire_mission(&self) -> MissionType {
        self.mission.current().known().unwrap_or(MissionType::None)
    }

    /// Construct after the owning world funnel has resolved the explicit
    /// `TechnoConstructorInit` capability. Kept inside `sim` so ordinary app,
    /// render, and diagnostic code cannot silently invent the native word.
    pub(in crate::sim) fn new_at_frame_from_constructor_word(
        stable_id: u64,
        native_unique_id: i32,
        rx: u16,
        ry: u16,
        z: u8,
        facing: u8,
        owner: InternedId,
        health: Health,
        type_ref: InternedId,
        category: EntityCategory,
        veterancy: u16,
        vision_range: u16,
        is_voxel: bool,
        construction_frame: u32,
        techno_ctor_random_word: u16,
    ) -> Self {
        // Infantry spawn at sub-cell 2 (top of diamond) instead of cell center
        // so they don't overlap with other units at the same position.
        let (init_sub_x, init_sub_y) = if category == EntityCategory::Infantry {
            crate::util::lepton::subcell_lepton_offset(Some(2))
        } else {
            (
                crate::util::lepton::CELL_CENTER_LEPTON,
                crate::util::lepton::CELL_CENTER_LEPTON,
            )
        };
        Self {
            cached_spatial_threat: Default::default(),
            tracking_facts: Default::default(),
            house_tracking_latches: Default::default(),
            #[cfg(feature = "phobos")]
            phobos_placement: Default::default(),
            #[cfg(feature = "debug-tools")]
            debug_log: None,
            stable_id,
            native_unique_id,
            native_cloak_displacement: crate::util::native_x87::NativeF32Bits::POSITIVE_ZERO,
            useless: false,
            production_start_price: 0,
            emp: Default::default(),
            #[cfg(feature = "ares")]
            attach_effects: Default::default(),
            techno_ctor_random_word,
            secret_lab_fallback: None,
            object_group: -1,
            stage: crate::sim::stage::StageClass::constructed(construction_frame as i32),
            discovery: Default::default(),
            building_upgrades: crate::sim::building_upgrades::BuildingUpgrades::default(),
            position: Position::new(rx, ry, z, None, init_sub_x, init_sub_y),
            // The Unlimbo direction; the class constructor supplies the rate.
            body_facing: crate::sim::movement::FacingClass::new(u16::from(facing) << 8, 0),
            // Unlimbo's type-derived initialization arms the turret loop latch.
            turret_rotate_latch: Default::default(),
            suppress_deploy_voice: Default::default(),
            barrel_elevation: crate::sim::movement::FacingClass::new(0, BARREL_ELEVATION_ROT),
            body_frame_counter: Default::default(),
            unit_firing_counter: Default::default(),
            owner,
            initial_owner: owner,
            #[cfg(feature = "ares")]
            original_factory_country: None,
            health,
            estimated_health: crate::sim::estimated_health::EstimatedHealth::from_raw(
                health.current(),
            ),
            type_ref,
            category,
            type_facts: crate::sim::world::TypeFacts::new(category),
            veterancy: crate::sim::combat::veterancy::Veterancy::new(veterancy),
            flashing: Default::default(),
            crate_multipliers: Default::default(),
            sight: crate::sim::vision::SightFacts::new(vision_range),
            sight_refresh_timers: crate::sim::vision::SightRefreshTimers::at_construction(
                construction_frame,
            ),
            gap_generator: crate::sim::world::GapGeneratorRuntime::default(),
            is_voxel,
            selection: Default::default(),
            deactivated: Default::default(),
            repairing: Default::default(),
            in_logic_vector: Default::default(),
            lifecycle: ObjectLifecycle::default(),
            in_playfield: Default::default(),
            mission_only: Default::default(),
            removal_facts: Default::default(),
            kill_record: Default::default(),
            foot_air: foot_air::FootAirState::new(stable_id),
            distributed_fire: Default::default(),
            locomotor: None,
            navigation: NavigationState::at_frame(construction_frame),
            patrol_post: Default::default(),
            foot_speed: crate::sim::components::FootSpeedState::default(),
            flight_attitude: Default::default(),
            ion_blast_lift: 0,
            foot_occupation_enabled: crate::sim::movement::foot_mark::OccupationEnable::constructed(
            ),
            lift: Default::default(),
            drop_credit: Default::default(),
            setter_force_reassign: Default::default(),
            chrono_warp_delay: Default::default(),
            attack_target: TarCom::default(),
            pending_building_fire: Default::default(),
            prism_support_count: Default::default(),
            turret_selection: gunner::TurretSelection::constructed(),
            radio_contacts: Contacts::default(),
            tether: crate::sim::radio::Tether::default(),
            ai_sell_repair: Default::default(),
            barrel_facing: None,
            turret_rotation_latch: Default::default(),
            last_fire_frame: Default::default(),
            // `TechnoClass` constructor `0x006F2E81..0x006F2E8C`: started
            // at the construction frame with no duration.
            rearm_timer: crate::sim::combat::rof::RearmTimer::new(construction_frame as i32),
            radar_combat_flash: crate::sim::timer::CdTimer::started(construction_frame as i32, 0),
            gattling: Default::default(),
            turret_anim_frame: Default::default(),
            voxel_recoil: None,
            weapon_burst: Default::default(),
            building_body: (category == EntityCategory::Structure).then(Default::default),
            building_power_health_sample: Default::default(),
            building_sale: None,
            ai_placement_timer: Default::default(),
            building_art: Default::default(),
            building_online: Default::default(),
            unit_delivery_no_base: false,
            hunter_seeker_origin: None,
            tesla_charge: Default::default(),
            tesla_charger: Default::default(),
            building_last_operational: Default::default(),
            building_loss_index: 0,
            building_powered_center: Default::default(),
            building_has_engineer: Default::default(),
            building_light: Default::default(),
            damage_fires: Default::default(),
            on_bridge: Default::default(),
            runtime_bridge_transition: Default::default(),
            animation: None,
            miner: None,
            slave_manager: None,
            slave: Default::default(),
            mega_mission: Default::default(),
            cloak: None,
            stealth_ability: StealthAbility::default(),
            sensor_deposit: None,
            disguise: Some(DisguiseRuntime::new(construction_frame)),
            low_bridge_tube_state: None,
            capture_manager: None,
            spawn_manager: None,
            spawn_owner_id: Default::default(),
            fall: ObjectFallState::default(),
            invulnerability: None,
            airstrike: Default::default(),
            airstrike_tint: Default::default(),
            mind_control: Default::default(),
            temporal: Default::default(),
            bomb: None,
            berserk: Default::default(),
            pending_entry: Default::default(),
            building_entry: Default::default(),
            ammo: None,
            ammo_reload_timer: crate::sim::timer::CdTimer::started(construction_frame as i32, 0),
            // Infantry get sub-cell 2 (first distinct position) at spawn so
            // they don't all pile up at cell center when multiple are created.
            sub_cell: crate::sim::movement::foot_mark::InfantrySpot::new(
                if category == EntityCategory::Infantry {
                    Some(2)
                } else {
                    None
                },
            ),
            crushable_facts: Default::default(),
            native_crush_immunity: Default::default(),
            mover_facts: Default::default(),
            death: crate::sim::world::TerminalLifetime::default(),
            move_sound: crate::sim::world::MoveSoundState::default(),
            crash: crate::sim::world::CrashLatch::default(),
            sinking: crate::sim::world::SinkingState::default(),
            passenger_role: PassengerRole::default(),
            display_type_override: Default::default(),
            capture_infantry_type_index: crate::sim::world::CapturedInfantryType::constructed(),
            #[cfg(feature = "ares")]
            vehicle_driver: Default::default(),
            pending_c4_detonation: Default::default(),
            has_been_captured: Default::default(),
            no_survivor: Default::default(),
            nominal_survivor: false,
            bunker: Default::default(),
            building_gate: None,
            door: crate::sim::door::DoorClass::at_frame(construction_frame),
            bunker_runtime: None,
            deploy_attachment: Default::default(),
            behind_anim: None,
            mcv_deploy_pending: Default::default(),
            infantry: if category == EntityCategory::Infantry {
                Some(crate::sim::infantry::InfantryRuntime::new())
            } else {
                None
            },
            rocking: None,
            mission: MissionCom::at_frame(construction_frame),
            passive_scan: crate::sim::world::PassiveScan::new(construction_frame),
            foot_retarget_after_stop: Default::default(),
            mission_leaf: MissionLeafState::constructed(category, construction_frame as i32),
            suspended_attack_target: Default::default(),
            base_defense_response: BaseDefenseResponseState::default(),
            spark_particle_system: Default::default(),
            damage_smoke_system_id: Default::default(),
            transport_unload_keep_count: Default::default(),
            produce_cash: crate::sim::credit_income::ProduceCash::new(construction_frame),
            drain: Default::default(),
            parasite: crate::sim::combat::parasite::FootParasite::new(construction_frame),
            #[cfg(feature = "phobos")]
            shield: None,
        }
    }

    /// Explicit zero-word constructor for tests that exercise construction
    /// frame anchoring without participating in gameplay RNG ownership.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new_at_frame_for_test(
        stable_id: u64,
        rx: u16,
        ry: u16,
        z: u8,
        facing: u8,
        owner: InternedId,
        health: Health,
        type_ref: InternedId,
        category: EntityCategory,
        veterancy: u16,
        vision_range: u16,
        is_voxel: bool,
        construction_frame: u32,
    ) -> Self {
        Self::new_at_frame_from_constructor_word(
            stable_id,
            0,
            rx,
            ry,
            z,
            facing,
            owner,
            health,
            type_ref,
            category,
            veterancy,
            vision_range,
            is_voxel,
            construction_frame,
            0,
        )
    }

    /// Explicit frame-zero constructor for tests that do not exercise
    /// construction-time Mission timer anchoring.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new_at_frame_zero_for_test(
        stable_id: u64,
        rx: u16,
        ry: u16,
        z: u8,
        facing: u8,
        owner: InternedId,
        health: Health,
        type_ref: InternedId,
        category: EntityCategory,
        veterancy: u16,
        vision_range: u16,
        is_voxel: bool,
    ) -> Self {
        Self::new_at_frame_for_test(
            stable_id,
            rx,
            ry,
            z,
            facing,
            owner,
            health,
            type_ref,
            category,
            veterancy,
            vision_range,
            is_voxel,
            0,
        )
    }

    /// ObjectClass `+0x8D` IsFallingDown. `ObjectClass::Paradrop`
    /// (`0x005F5940`) raises it at `0x005F5965`, and `ObjectClass::AI`
    /// clears it when the fall grounds (`0x005F3F86`).
    pub(crate) fn is_falling_down(&self) -> bool {
        self.fall.falling()
    }

    pub(crate) fn was_dropped_as_bomb(&self) -> bool {
        self.fall.dropped_as_bomb()
    }

    pub(crate) fn fall_rate(&self) -> i32 {
        self.fall.rate()
    }

    pub(crate) fn is_parachuted(&self) -> bool {
        self.fall.parachuted()
    }

    pub(crate) fn parachute_animation(&self) -> Option<u64> {
        self.fall.parachute_animation()
    }

    pub(crate) fn set_parachute_animation(&mut self, animation: Option<u64>) {
        self.fall.set_parachute_animation(animation);
    }

    pub(crate) fn parachute_animation_expired(&mut self, animation: u64) {
        self.fall.parachute_animation_expired(animation);
    }

    /// Paradrop5F5940 raises +84/+8D and resets +2C before placement.
    pub(crate) fn begin_parachute_fall(&mut self, z: i32) {
        self.position.put_z(z);
        self.fall.begin_parachute();
    }

    /// DropAsBomb5F416A/71 writes neither Location nor FallRate nor +84.
    pub(crate) fn begin_bomb_fall(&mut self) {
        self.fall.begin_bomb();
    }

    pub(crate) fn finish_fall(&mut self) {
        self.fall.finish();
    }

    /// FootClass::AI's +8D/+8E edge; true as a DropAsBomb fall starts.
    pub(crate) fn observe_falling_edge(&mut self) -> bool {
        self.fall.observe_falling_edge()
    }

    /// ObjectAI5F3FBC..3FFA updates even the grounding frame's rate.
    pub(crate) fn advance_fall_rate(&mut self, parachute_cap: i32, free_fall_cap: i32) {
        self.fall.advance_rate(parachute_cap, free_fall_cap);
    }

    /// Put a fixture into a fall where it is, with a FallRate of 0.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_falling_down_for_test(&mut self, falling: bool) {
        self.fall.set_falling_for_test(falling);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_fall_rate_for_test(&mut self, rate: i32) {
        self.fall.set_rate_for_test(rate);
    }

    /// Mark a live RadioClass-style contact with another entity.
    ///
    /// First-null slot insert: idempotent, and full slots deny without evicting
    /// (the receiver dock idiom). Slot order is hash-relevant and deterministic.
    /// `RadioClass+0xE4`/`+0xE8`, the contact slots; see the field.
    #[cfg(test)]
    pub fn mark_live_contact_with(&mut self, other_stable_id: u64) {
        self.radio_contacts.insert(other_stable_id);
    }

    /// Whether this entity has a live RadioClass-style contact with another entity.
    /// `RadioClass+0xE4`/`+0xE8`, the contact slots; see the field.
    pub fn has_live_contact_with(&self, other_stable_id: u64) -> bool {
        self.radio_contacts.contains(other_stable_id)
    }

    /// Clear a live RadioClass-style contact with another entity (BREAK: nulls
    /// the slot in place, no compaction).
    /// `RadioClass+0xE4`/`+0xE8`, the contact slots; see the field.
    pub fn clear_live_contact_with(&mut self, other_stable_id: u64) {
        self.radio_contacts.remove(other_stable_id);
    }

    /// Runtime movement/path layer with Ground as the fallback.
    ///
    /// This is not the object-list selector, which is
    /// `occupancy::cell_list_layer_for_entity`.
    /// VERA-only: the layer cached on the locomotor, where gamemd asks the
    /// locomotor's `In_Which_Layer`; placements store it with OnBridge.
    pub fn movement_layer_or_ground(&self) -> crate::sim::movement::locomotor::MovementLayer {
        self.locomotor.as_ref().map_or(
            crate::sim::movement::locomotor::MovementLayer::Ground,
            |l| l.layer(),
        )
    }

    /// Create a minimal entity for testing. Fills sensible defaults for most fields.
    #[cfg(any(test, feature = "test-support"))]
    /// Create a minimal test entity with the given owner and type_ref strings.
    /// Uses a shared test interner via `test_intern()` for consistent IDs.
    pub fn test_default(stable_id: u64, type_ref: &str, owner: &str, rx: u16, ry: u16) -> Self {
        Self::test_default_of_category(stable_id, type_ref, owner, rx, ry, EntityCategory::Unit)
    }

    /// Construct the requested class before its private class state is installed.
    /// Tests must not turn a Unit into a Building by changing only `category`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn test_default_of_category(
        stable_id: u64,
        type_ref: &str,
        owner: &str,
        rx: u16,
        ry: u16,
        category: EntityCategory,
    ) -> Self {
        Self::new_at_frame_zero_for_test(
            stable_id,
            rx,
            ry,
            0, // z = ground level
            0, // facing = north
            crate::sim::intern::test_intern(owner),
            Health::new(100),
            crate::sim::intern::test_intern(type_ref),
            category,
            0, // veterancy = rookie
            5, // vision_range = 5 cells
            true,
        )
    }

    /// Whether this entity is alive (health > 0).
    /// `Object+0x6C`, Health, above 0.
    pub fn is_alive(&self) -> bool {
        self.health.current() > 0
    }

    /// Whether ObjectClass native-alive state is set. This is intentionally
    /// independent from health and the Rust death-sequence state.
    /// `Object+0x90`, IsAlive; see the field.
    pub fn is_object_alive(&self) -> bool {
        self.lifecycle.object_alive()
    }

    /// `ObjectClass+0x81`, InLimbo: off the map, as an object still in its
    /// factory or a transport, or a V3 rocket in its launcher.
    pub fn in_limbo(&self) -> bool {
        self.lifecycle.in_limbo()
    }

    /// IsAlive (`Object+0x90`) without the `TerminalLifetime` dying mark.
    /// VERA-only: the mark, which native lacks, adds the SHP death sequence
    /// of a non-Infantry object; those records are its source of truth. An
    /// infantry corpse answers true: it keeps IsAlive at Health 0 until its
    /// death sequence UnInits it (`world::infantry_death`).
    pub fn is_active(&self) -> bool {
        self.lifecycle.object_alive() && !self.death.is_dying()
    }

    /// ObjectClass IsAlive (`+0x90`) as the per-object AI and a Unit's fire
    /// update read it. Native clears it when an ordinary death UnInits the
    /// object inside ReceiveDamage; VERA defers that UnInit, so Health 0 counts
    /// as dead here unless the object is crashing, and a wreck keeps IsAlive
    /// until its impact. An infantry corpse on its death sequence answers
    /// false here, though it keeps IsAlive and its visit.
    pub fn is_ai_alive(&self) -> bool {
        self.is_active() && (self.health.current() > 0 || self.crash.is_crashing())
    }

    /// `MissionClass::AI @ 0x005B3060`'s gate before the mission handler, the
    /// same for every class: IsAlive (`+0x90`, `0x005B306C`; while VERA defers
    /// UnInit, [`Self::is_active`]), the dispatch timer run out (`+0xC8`,
    /// `+0xD0`, `0x005B307A..0x005B30A1`) and Health above zero (`+0x6C`,
    /// signed, `0x005B30A7`). A crashing wreck is alive at Health 0: its Techno
    /// AI runs, but no handler does and its timer is not rewritten.
    pub(crate) fn mission_handler_due(&self, now: u32) -> bool {
        self.is_active() && self.mission.dispatch_timer().due(now) && self.health.current() > 0
    }

    /// Infantry522510's Doing27..30 deployment predicate. Undeploy31 already
    /// accepts destinations; Units retain their distinct deployment controller.
    pub fn is_deployed(&self) -> bool {
        if self.category == EntityCategory::Infantry {
            self.infantry_deploy_doing()
        } else {
            self.mission_leaf.as_unit().is_some_and(|leaf| {
                leaf.deployed() != 0
                    || leaf.deploy_begin_active() != 0
                    || leaf.deploy_reverse_active() != 0
            })
        }
    }

    /// Native deployed state. Unit6E0 remains set during reverse animation;
    /// consumers needing the transition exclusion also read unit_deploying().
    pub fn is_fully_deployed(&self) -> bool {
        if self.category == EntityCategory::Infantry {
            self.mission_leaf
                .as_infantry()
                .is_some_and(|leaf| (28..=30).contains(&leaf.doing()))
        } else {
            self.mission_leaf
                .as_unit()
                .is_some_and(|leaf| leaf.deployed() != 0)
        }
    }

    /// Infantry522510's deployment family, used by its weapon/mission and
    /// destination consumers. Undeploy31 is outside this native predicate.
    pub fn infantry_deploy_doing(&self) -> bool {
        self.category == EntityCategory::Infantry
            && self
                .mission_leaf
                .as_infantry()
                .is_some_and(|leaf| (27..=30).contains(&leaf.doing()))
    }

    /// Techno `+0x2A4`; see [`CrushImmunity`].
    ///
    /// [`CrushImmunity`]: crate::sim::deploy::CrushImmunity
    pub(crate) const fn native_crush_immunity(&self) -> u8 {
        self.native_crush_immunity.get()
    }

    /// A fixture infantryman's raw crush-immunity byte.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_infantry_deploy_crush_immunity(&mut self, raw: u8) {
        assert_eq!(self.category, EntityCategory::Infantry);
        self.native_crush_immunity = crate::sim::deploy::CrushImmunity::from_raw(raw);
    }

    /// Native Get_Mission(vt184) reads current, otherwise queued. The
    /// Construction and Selling producers publish that same authority.
    pub(crate) fn constructing_or_selling(&self) -> bool {
        matches!(
            self.mission.effective().known(),
            Some(MissionType::Construction | MissionType::Selling)
        )
    }

    /// Native Building+534 is0, independent of mission status and ready byte.
    pub fn in_construction_bstate(&self) -> bool {
        self.building_body
            .as_ref()
            .is_some_and(|body| body.state() == 0)
    }

    /// Building440042..440074: compare actual health, publish the new sample
    /// and tell the host to invalidate the owning House's derived state.
    pub fn sample_building_health_for_power(&mut self) -> bool {
        self.category == EntityCategory::Structure
            && self
                .building_power_health_sample
                .sample(self.health.current())
    }

    /// A Building's reset at a successful ordinary Unlimbo (`0x00440D2D`).
    pub(crate) fn reset_building_health_sample_at_unlimbo(&mut self) {
        if self.category == EntityCategory::Structure {
            self.building_power_health_sample.reset();
        }
    }

    pub fn building_power_health_sample(&self) -> Option<i32> {
        (self.category == EntityCategory::Structure)
            .then_some(self.building_power_health_sample.get())
    }

    pub(crate) fn hash_building_health_sample(&self, hasher: &mut impl std::hash::Hasher) {
        if let Some(sample) = self.building_power_health_sample() {
            use std::hash::Hash;
            sample.hash(hasher);
        }
    }

    /// VERA-only: folds Techno `+0x338`, the captor's InfantryType index, into
    /// the state hash.
    pub(crate) fn hash_capture_infantry_type(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        self.capture_infantry_type_index.get().hash(hasher);
        #[cfg(feature = "ares")]
        if self.vehicle_driver != Default::default() {
            b"ares-vehicle-driver-v1".hash(hasher);
            self.vehicle_driver.hash(hasher);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native_cloak_fixture() -> &'static serde_json::Value {
        crate::test_fixture::json("tools/procedural_drawing_oracle/translucent_blitter_a.json")
    }

    #[test]
    fn visual_character_matches_original_executed_transition_controls() {
        let native = native_cloak_fixture();
        let rows = native["cloak_transition"].as_array().unwrap();
        assert_eq!(rows.len(), 256);
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 10, 20);
        for row in rows {
            let mut cloak = CloakRuntime::new(0);
            cloak.set_state_for_test(row["state"].as_i64().unwrap() as i32);
            cloak.set_depth_for_test(row["depth"].as_i64().unwrap() as u32);
            entity.cloak = Some(cloak);
            entity
                .discovery
                .set_owned_by_current_house_for_test(row["owned"].as_bool().unwrap());
            let query = if row["force"].as_bool().unwrap() {
                crate::sim::cloak_disguise::VisualCharacterQuery::sensor(false, false, false)
            } else {
                crate::sim::cloak_disguise::VisualCharacterQuery::screen(
                    true, false, false, false, false, false,
                )
            };
            assert_eq!(
                entity.visual_character(row["stage"].as_i64().unwrap() as i32, false, query),
                row["character"].as_u64().unwrap() as u8,
                "native control {row}"
            );
        }
    }

    #[test]
    fn native_cloak_offset_matches_original_identity_float_and_wrap_controls() {
        let native = native_cloak_fixture();
        let rows = native["cloak_offsets"].as_array().unwrap();
        assert_eq!(rows.len(), 14);
        let mut entity = GameEntity::test_default(777, "MTNK", "Americans", 10, 20);
        for row in rows {
            entity.native_unique_id = row["native_unique_id"].as_i64().unwrap() as i32;
            let hex = row["raw_f32_hex"].as_str().unwrap();
            let bytes: [u8; 4] =
                std::array::from_fn(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap());
            entity.native_cloak_displacement =
                crate::util::native_x87::NativeF32Bits::from_bits(u32::from_le_bytes(bytes));
            assert_eq!(
                entity.native_cloak_offset_words(),
                row["offset_words"].as_i64().unwrap() as i32,
                "native control {row}"
            );
            let saved = crate::util::save_codec::serialize(&entity).unwrap();
            let restored: GameEntity = crate::util::save_codec::deserialize(&saved).unwrap();
            assert_eq!(
                restored.native_cloak_displacement,
                entity.native_cloak_displacement
            );
            assert_eq!(
                restored.native_cloak_offset_words(),
                entity.native_cloak_offset_words()
            );
        }
    }

    #[test]
    fn live_contacts_are_per_entity_and_idempotent() {
        let mut contacted = GameEntity::test_default(1, "MTNK", "Americans", 30, 40);
        let unrelated = GameEntity::test_default(2, "MTNK", "Americans", 31, 40);

        contacted.mark_live_contact_with(100);
        contacted.mark_live_contact_with(100);

        assert_eq!(contacted.radio_contacts.len(), 1); // idempotent — one slot used
        assert!(contacted.has_live_contact_with(100));
        assert!(!unrelated.has_live_contact_with(100));

        contacted.clear_live_contact_with(100);
        assert!(!contacted.has_live_contact_with(100));
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    #[test]
    fn lifecycle_authority_state_axes_are_independent() {
        let mut e = GameEntity::test_default(1, "E1", "Americans", 3, 3);
        assert_eq!(e.lifecycle, ObjectLifecycle::default());
        assert!(!e.in_logic_vector.is_member());
        assert!(!e.removal_facts.dirty_rect_eligible());
        assert!(!e.kill_record.is_recorded());

        // Exercise every combination without deriving one fact from another.
        for bits in 0u8..64 {
            e.lifecycle.set_object_alive_for_test(bits & 0b00_0001 != 0);
            e.lifecycle.set_in_limbo_for_test(bits & 0b00_0010 != 0);
            e.lifecycle.set_cell_marked_for_test(bits & 0b00_0100 != 0);
            e.in_logic_vector.set_for_test(bits & 0b00_1000 != 0);
            e.death.set_dying_for_test(bits & 0b01_0000 != 0);
            e.health
                .set_for_test(if bits & 0b10_0000 != 0 { 1 } else { 0 });

            assert_eq!(e.is_object_alive(), bits & 0b00_0001 != 0);
            assert_eq!(e.lifecycle.in_limbo(), bits & 0b00_0010 != 0);
            assert_eq!(e.lifecycle.cell_marked(), bits & 0b00_0100 != 0);
            assert_eq!(e.in_logic_vector.is_member(), bits & 0b00_1000 != 0);
            assert_eq!(e.death.is_dying(), bits & 0b01_0000 != 0);
            assert_eq!(e.is_alive(), bits & 0b10_0000 != 0);
            assert_eq!(
                e.is_active(),
                e.lifecycle.object_alive() && !e.death.is_dying(),
            );
        }
    }
}

#[cfg(test)]
mod mission_shadow_tests {
    use super::*;
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};

    #[test]
    fn mission_round_trips_through_serde() {
        // Slice 6 un-skips the field so the queued/suspended interrupt stack +
        // timer survive a save/load. (current/substate are also reconciled from
        // the legacy machines on load; the rest persists as serialized.)
        let mut e = GameEntity::test_default(1, "E1", "Americans", 3, 3);
        e.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Attack),
            suspended: MissionId::NONE,
            queued: MissionId::from_known(MissionType::Guard),
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 99,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        let json = serde_json::to_string(&e).expect("serialize entity");
        assert!(
            json.contains("mission"),
            "mission must round-trip — present in serialized form"
        );
        let restored: GameEntity = serde_json::from_str(&json).expect("deserialize entity");
        assert_eq!(
            restored.mission.current(),
            MissionId::from_known(MissionType::Attack)
        );
        assert_eq!(
            restored.mission.queued(),
            MissionId::from_known(MissionType::Guard)
        );
        assert_eq!(restored.mission.ai_counter(), 99);
    }
}
