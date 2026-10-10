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
//!   sim/combat (AttackTarget), sim/animation, sim/miner, and special movement modules.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::map::entities::EntityCategory;
use crate::sim::animation::Animation;
use crate::sim::cloak_disguise::{CloakRuntime, DisguiseRuntime};
use crate::sim::combat::combat_weapon::WeaponSlot;
use crate::sim::combat::{AttackTarget, TargetKind};
use crate::sim::components::{
    C4PlantState, HarvestOverlay, Health, MovementTarget, NavigationState, OrderIntent,
    PendingC4Detonation, Position, RockingState, VoxelAnimation,
};
use crate::sim::debug_event_log::{DebugEventKind, DebugEventLog};
use crate::sim::docking::aircraft_dock::AircraftAmmo;
use crate::sim::intern::InternedId;
use crate::sim::miner::Miner;
use crate::sim::mission::{MissionCom, MissionLeafState, MissionTimer, MissionType};
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::tube_movement::LowBridgeTubeMovementState;
use crate::sim::passenger::PassengerRole;
use crate::sim::radio::Contacts;
use crate::sim::superweapon::invulnerability::InvulnerabilityState;
use crate::sim::timer::CdTimer;
use crate::util::native_x87::NativeF64Bits;

#[path = "movement/foot_air.rs"]
mod foot_air;

/// Frames the passive target-scan timer is armed for at object construction.
/// The original's Techno constructor anchors the timer at the current frame and
/// writes 45 as its duration, so a freshly built object waits that long before
/// its first passive scan; the scanner then re-arms it from the `[General]`
/// targeting delays.
pub const PASSIVE_SCAN_CONSTRUCTION_DELAY_FRAMES: u32 = 45;

/// Infantry-only runtime fear/prone state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InfantryRuntime {
    pub fear_level: u16,
    pub is_prone: bool,
    /// Countdown to this man's next idle fidget.
    ///
    /// Re-armed to a fresh random wait every time the idle action fires, and
    /// only then — the same one-shot timer gamemd keeps on the infantry object.
    /// Its default is the unarmed sentinel, which reads as already due, so a
    /// freshly built infantryman is eligible on his first idle turn.
    #[serde(default)]
    pub idle_action_timer: MissionTimer,
    /// Infantry+6DC. `InfantryClass` failed-path receiver `0x0051DAF0`
    /// (Infantry vtable +0x500, called from `FootClass::Find_Path` failure
    /// `0x004D4044` and `Do_Action` `0x0051D6F0` at zero health) writes the
    /// current-cell `Can_Enter_Cell` answer here: 1 when the answer is nonzero
    /// (`0x51DBBE`), otherwise the zero answer byte (`0x51DBAC`). Readers:
    /// Infantry Mission_Move restart `0x00520FF1` and save/load `0x00521D0F`.
    #[serde(default)]
    pub cell_entry_blocked: bool,
}

impl InfantryRuntime {
    pub fn new() -> Self {
        Self {
            fear_level: 0,
            is_prone: false,
            idle_action_timer: MissionTimer::default(),
            cell_entry_blocked: false,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_foundation() -> String {
    "1x1".to_string()
}

/// `TechnoClass::Constructor @ 0x006F2B9C` seeds `+0x120` with `-100`, so a
/// freshly built object is already past the idle-turret dwell on frame 0.
pub(crate) const NATIVE_LAST_FIRE_FRAME_INIT: i64 = -100;

/// The barrel elevation's rate: the TechnoClass constructor passes 3
/// (`PUSH 3` at `0x006F2EDE`) to `FacingClass(int) @ 0x004C91E0`.
const BARREL_ELEVATION_ROT: i32 = 3;

/// A level barrel: the draw pitches by `d32 - 8`, which is zero at `0x4000`.
pub(crate) const BARREL_LEVEL: u16 = 0x4000;

/// The elevation Unlimbo turns a barrel toward: level minus the low byte of
/// `FireAngle=` shifted up a byte (`MOV CH, byte [Type+0x3D0]` at
/// `0x006F6DD9`, `SUB` at `0x006F6DDF`), kept to a word.
pub(crate) fn unlimbo_barrel_target(fire_angle: i32) -> u16 {
    BARREL_LEVEL.wrapping_sub(u16::from(fire_angle as u8) << 8)
}

fn default_last_fire_frame() -> i64 {
    NATIVE_LAST_FIRE_FRAME_INIT
}

fn default_base_plan_type_index() -> i32 {
    -1
}

/// Persistent TechnoClass state used by the active House base-defence
/// responder. The two admission bytes are constructor-true; cooldown writes
/// occur only after a responder assignment or strict budget overshoot. The
/// ArchiveTarget (`+0x218`) stored here is the general Techno field, which
/// responders, miners, the slave manager and the rally click all write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct BaseDefenseResponseState {
    pub(crate) recruitable_a: bool,
    pub(crate) recruitable_b: bool,
    archive_target: Option<TargetKind>,
    /// `TechnoClass+0x650`/`+0x658`: the attacker's call-for-help cooldown
    /// (`TechnoClass::RespondToBaseAttack @ 0x00708171..0x00708192`, written
    /// at `0x0070879E..0x007087A9`); the constructor leaves it at `-1`, 0.
    pub(crate) cooldown: CdTimer,
}

impl Default for BaseDefenseResponseState {
    fn default() -> Self {
        Self {
            recruitable_a: true,
            recruitable_b: true,
            archive_target: None,
            cooldown: CdTimer::from_raw(-1, 0),
        }
    }
}

fn default_techno_multiplier() -> NativeF64Bits {
    NativeF64Bits::ONE
}

/// The unit side of the tank-bunker reciprocal link (the pre-install approach
/// state plus the installed link, folded into one hashed field). Distinct from
/// `PassengerRole` cargo: a bunker is a single reciprocal link, never cargo.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum BunkerLink {
    /// Not heading to or inside any bunker.
    #[default]
    None,
    /// Ordered into bunker `id`, still approaching (pre-install). Cleared on any
    /// retask, which lets the building install machine reset. (The explicit
    /// unit-side marker is the abort signal the install machine reads.)
    Approaching(u64),
    /// Installed inside bunker `id` (reciprocal of `building.bunker_occupant`).
    Installed(u64),
}

impl BunkerLink {
    /// The bunker this unit is installed in, if any.
    pub fn installed_in(self) -> Option<u64> {
        match self {
            BunkerLink::Installed(id) => Some(id),
            _ => None,
        }
    }
    /// The bunker this unit is approaching, if any.
    pub fn approaching(self) -> Option<u64> {
        match self {
            BunkerLink::Approaching(id) => Some(id),
            _ => None,
        }
    }
}

pub(crate) use crate::sim::gate_runtime::BuildingGateRuntime;

/// Parent-owned `BuildingLightClass` runtime. It exists only for a successfully
/// placed `HasSpotlight=yes` building and is removed with that parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BuildingLightRuntime {
    pub behavior: u8,
    pub target_id: Option<u64>,
}

/// Independent ObjectClass lifecycle facts.
///
/// These bytes deliberately do not derive from health, store presence, cell
/// occupancy, LogicVector membership, or the Rust death-sequence state. Active
/// gamemd keeps those concerns independent, including alive objects that are in
/// limbo, active objects that are temporarily off-cell, and dead-limbo objects
/// that remain resolvable until the pending-delete drain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ObjectLifecycle {
    /// ObjectClass native-alive state (`ObjectClass+0x90` analogue).
    pub object_alive: bool,
    /// ObjectClass InLimbo state. This is independent of LogicVector membership.
    pub in_limbo: bool,
    /// Whether Mark/cell-list insertion currently owns cell membership.
    pub cell_marked: bool,
}

impl Default for ObjectLifecycle {
    fn default() -> Self {
        Self {
            object_alive: true,
            in_limbo: true,
            cell_marked: false,
        }
    }
}

/// Accepted Psychedelic-warhead state owned by TechnoClass.
///
/// `active` mirrors the byte at native `Techno+0x298`; `timer` mirrors the
/// signed dword at `+0x29C`. The latter is the exact distance-zero damage
/// kernel result, so it deliberately remains signed rather than becoming a
/// Rust duration type.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct BerserkState {
    pub active: bool,
    pub timer: i32,
}

/// A building's delayed fire (`BuildingClass+0x704` mode, `+0x714`
/// countdown, `+0x708..+0x710` payload), armed by Mission_Attack and served by
/// ProcessDelayedFire (`0x004503F0`) when the countdown ends.
///
/// A shot captures no target: expiry reads the building's live
/// `attack_target`, with the weapon Mission_Attack saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PendingBuildingFire {
    /// Signed native timer value, clamped to zero by ProcessDelayedFire after
    /// its pre-decrement.
    pub remaining_ticks: i32,
    pub fire: DelayedFire,
}

/// What a delayed fire does when its countdown ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DelayedFire {
    /// Mode 1: FireAt the live target with the saved weapon (`+0x708`).
    Weapon(WeaponSlot),
    /// Mode 2: a Prism support beam (`0x0044ABD0`) to the master's weapon-0
    /// FLH as it stood at recruitment (`+0x708..+0x710`, world leptons).
    SupportBeam {
        to: crate::sim::projectile::ProjectileCoord,
    },
}

impl std::hash::Hash for PendingBuildingFire {
    /// A weapon shot folds as the pre-support latch did (countdown, then the
    /// slot); a support beam folds a third tag and its point.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.remaining_ticks.hash(state);
        match self.fire {
            DelayedFire::Weapon(slot) => slot.hash(state),
            DelayedFire::SupportBeam { to } => {
                2isize.hash(state);
                to.hash(state);
            }
        }
    }
}

/// Unified entity struct — replaces all hecs ECS components.
///
/// Every game object (unit, infantry, building, aircraft) is one `GameEntity`.
/// `TechnoClass`'s rank cache starts at `-1` so an object's first sample after
/// spawn caches its rank without announcing a promotion.
fn veterancy_rank_cache_default() -> i8 {
    -1
}

/// A rookie accumulator, for snapshots written before the field existed.
fn veterancy_raw_default() -> crate::util::native_x87::NativeF32Bits {
    crate::util::native_x87::NativeF32Bits::POSITIVE_ZERO
}

/// Constructor state captured while a generated-map Techno still owns the
/// launch-time Scenario cursor. Projection validates all identity fields
/// before installing the already-consumed word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GeneratedTechnoInit {
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

/// Authored Building upgrades construct as distinct Technos, then Unlimbo at
/// their host location. The host/slot association is persistent identity; the
/// upgrade does not own a competing building footprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct StructureUpgradeLink {
    pub parent_stable_id: u64,
    pub slot: u8,
}

/// Client-relative Techno discovery history, native +41A/+41B/+41C.
/// Constructor6F2FB5..6F2FC1 clears all three; InitManagers6F3F40 and
/// ChangeOwner70173B classify only the current owner. Discovery6F4960 and
/// Conceal6F4A40 own the two retained observations. Infantry raw Save/Load
/// preserves the bytes: never reconstruct them from the restored owner.
/// Native peer CRC64DAB0 omits them; this is also omitted from the shared
/// Rust peer hash. The separate diagnostic CRC70C270 is not that checksum.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TechnoDiscoveryHistory {
    pub owned_by_current_house: bool,
    pub discovered_by_current_house: bool,
    /// A single historical byte for all other receiver houses, not a house set.
    pub discovered_by_other_house: bool,
}

/// Core fields are always present; optional subsystems use `Option<T>`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GameEntity {
    // --- Always present (every entity has these) ---
    // Indexed identity is private in production. Synthetic fixtures retain raw
    // setup access; conditional declarations preserve the exact serde field order.
    /// Deterministic stable ID — primary key, used for cross-entity references,
    /// replay logs, state hashing, and networking. Never reused.
    #[cfg(test)]
    pub(crate) stable_id: u64,
    #[cfg(not(test))]
    stable_id: u64,
    /// AbstractClass+10, assigned by the concrete constructor after the base
    /// Techno Scenario word (Unit735454 / Building43BA15). May wrap or duplicate;
    /// stable handles remain the reference and storage authority.
    pub(crate) native_unique_id: i32,
    /// Native Techno+24C float bits, initialized to zero by6F2D62 and
    /// preserved by native persistence (70C32A). Only visual-character4 reads
    /// this displacement through70BE50; no simulation/RNG/timer consumer.
    /// Retain the raw datum, not a renderer-owned reconstructed animation.
    native_cloak_displacement: crate::util::native_x87::NativeF32Bits,
    /// Low word of the one raw Scenario RNG draw performed by the active-retail
    /// `TechnoClass` constructor (`0x006F3254`, stored at native `+0x3C8`).
    /// Later report-selection consumers read this persistent value; placement
    /// failure never refunds the draw.
    pub techno_ctor_random_word: u16,
    /// One native Techno stage clock. Class receivers mutate through this
    /// owner; animation, miner and buildup components carry no competing copy.
    stage: crate::sim::stage::StageClass,
    pub discovery: TechnoDiscoveryHistory,
    /// Authored structure-upgrade identity. `None` for ordinary Technos.
    pub structure_upgrade_link: Option<StructureUpgradeLink>,
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
    /// Persistent FootClass body-animation counter (`FootClass+0x538`).
    /// Unit SHP and voxel drawing reduce this against the selected model's
    /// frame count. It advances on absolute binary-frame cadence and never
    /// resets on a visual transition or deployment.
    #[serde(default)]
    pub body_frame_counter: u32,
    /// Owning player/faction name (e.g., "Americans", "Soviet") — interned for zero-cost clones.
    #[cfg(test)]
    pub(crate) owner: InternedId,
    #[cfg(not(test))]
    owner: InternedId,
    /// Current and maximum hit points.
    pub health: Health,
    /// Object+70: signed damage reservations/recovery, not an actual-HP cache.
    /// Native constructors and map admission seed this from admitted health.
    pub(crate) estimated_health: crate::sim::estimated_health::EstimatedHealth,
    /// rules.ini section name (e.g., "HTNK", "E1", "GAPOWR") — interned for zero-cost clones.
    #[cfg(test)]
    pub(crate) type_ref: InternedId,
    #[cfg(not(test))]
    type_ref: InternedId,
    /// Entity category: Unit, Infantry, Aircraft, or Structure.
    pub category: EntityCategory,
    /// Rules foundation string for structure footprint occupancy.
    ///
    /// Native CellClass list membership is removed from every foundation cell
    /// during ExitCell/Unlimbo-style lifecycle paths. Storing the parsed source
    /// string here lets `Simulation::uninit` perform that cleanup without a
    /// RuleSet borrow.
    #[serde(default = "default_foundation")]
    pub foundation: String,
    /// Immutable type inputs needed to reverse this building's hidden-counter
    /// contribution during cell-list exit without a RuleSet borrow.
    #[serde(default)]
    pub building_hidden_occupancy:
        Option<crate::rules::object_type::BuildingHiddenOccupancyProfile>,
    /// Immutable writer-profile snapshot for CellClass house base reservations.
    /// `None` is the exact native BuildingType eligibility gate.
    #[serde(default)]
    pub base_reservation_spacing: Option<i32>,
    /// Immutable BuildingType profile for the House edge refresh callback.
    /// True only when the parsed type has `Factory=BuildingType`.
    #[serde(default)]
    pub determines_waypoint_edge: bool,
    /// Immutable resolved membership in `[AI] BuildConst=`
    /// (`RulesClass__ReadAI @ 0x00672AE0`, binding
    /// `0x00672B14..0x00672C01`). Lifecycle authority copies this from
    /// BuildingType so rule-less Limbo and owner transfer paths can maintain
    /// native acquisition order exactly.
    #[serde(default)]
    pub build_const_eligible: bool,
    /// Immutable native BuildingType registry index for BasePlan lifecycle writers.
    #[serde(default = "default_base_plan_type_index")]
    pub base_plan_type_index: i32,
    /// Immutable BuildingType `IsBaseDefense=` fact.
    #[serde(default)]
    pub base_plan_is_defense: bool,
    /// Immutable non-null `UndeploysInto` fact used by successful Unlimbo fallback.
    #[serde(default)]
    pub base_plan_has_undeploy_target: bool,
    /// The running accumulator every rank is sampled from
    /// ([`Self::veterancy`]).
    ///
    /// gamemd-derived: the `VeterancyClass` float on `TechnoClass`, fed by
    /// `Record_The_Kill @ 0x00702D40` through `VeterancyClass::Add @
    /// 0x0074FF50`. Carried as its `f32` bit pattern so sim state holds no
    /// float and hashing, snapshots and replay stay integer-exact.
    #[serde(default = "veterancy_raw_default")]
    pub veterancy_raw: crate::util::native_x87::NativeF32Bits,
    /// Last rank announced, for crossing detection.
    ///
    /// gamemd-derived: `TechnoClass+0x13C`, initialised to `-1` by the
    /// constructor so the first sample after spawn caches without announcing.
    /// Holds the native `GetVeterancyLevel @ 0x00750030` code: `-1`
    /// uninitialised, `0` ELITE, `1` veteran, `2` rookie.
    #[serde(default = "veterancy_rank_cache_default")]
    pub veterancy_rank_cache: i8,
    /// Frames left on the newly-elite flash — `TechnoClass+0xF0`, seeded with
    /// `[AudioVisual] EliteFlashTimer=` at `0x006FA0DC` on the elite crossing.
    /// Presentation-only state; nothing in `sim/` reads it back.
    /// RESIDUAL (GSI-08.12): no renderer reads it either, so a unit that just
    /// turned elite does not flash.
    #[serde(default)]
    pub elite_flash_frames: u16,
    /// Mutable Techno instance armor multiplier (`Techno+0x158`). Native
    /// construction seeds this double to 1.0; the Armor crate
    /// (`crates::effects`) is its only writer, exactly once per object.
    #[serde(default = "default_techno_multiplier")]
    pub armor_multiplier: NativeF64Bits,
    /// `Techno+0x160`, the firepower multiplier `FireAt`'s damage build folds
    /// in (`0x006FE33D..0x006FE34D`). Seeded to 1.0; the Firepower crate is
    /// its only writer, exactly once per object.
    #[serde(default = "default_techno_multiplier")]
    pub firepower_multiplier: NativeF64Bits,
    /// House credited with destroying this object, captured at the instant its
    /// health reached zero. gamemd's kill-record step receives the actual killer
    /// at the moment of destruction; infantry linger in the logic vector through
    /// a death animation, so this observation retains the actual callback's
    /// attribution. It does not defer or suppress the native score callback.
    #[serde(skip)]
    pub killed_by: Option<InternedId>,
    /// Type's `DontScore=`, copied in at spawn so the score bookkeeping and the
    /// house counts (`house_tracking`) can honor it without a `RuleSet`
    /// borrow — the same reason `foundation` is copied. Persisted: a count
    /// removed after a reload must skip it as its addition did.
    #[serde(default)]
    pub dont_score: bool,
    /// The other type facts the house counts test (`house_tracking`), copied
    /// in at construction.
    #[serde(default)]
    pub tracking_facts: crate::sim::house_tracking::TrackingFacts,
    /// Fog-of-war sight range in cells.
    pub vision_range: u16,
    /// Exact immutable Type+5E8 == 0 predicate used by InfantryUnlimbo51E0EF.
    /// The fog range above clamps/truncates the raw Sight integer and cannot
    /// answer this query. Construction resolves it once; snapshots preserve
    /// it for rules-less re-entry, independently of client discovery history.
    pub(crate) sight_is_zero: bool,
    /// Foot65C/664 high-flying sight refresh timer, virtualized per viewer.
    /// Independent of the retained sight-admission latch and stored footprint.
    pub(crate) sight_refresh_timers: crate::sim::vision::SightRefreshTimers,
    /// Building43FB20 operational edge and Techno6FB170/6FB470 deposit latch.
    pub(crate) gap_generator: crate::sim::vision::GapGeneratorRuntime,

    // --- Render model (mutually exclusive) ---
    /// True = VXL/HVA model, false = SHP sprite; effective art metadata is authoritative.
    pub is_voxel: bool,

    // --- Bool markers (were zero-size ECS components) ---
    /// Whether this entity is currently selected by the local player.
    /// App-layer state — NOT part of authoritative simulation. Never read by sim logic.
    /// Mutations: `Command::Select` → `apply_selection_snapshot()` in world_commands.rs;
    /// combat.rs sets `selected = false` on death/transport entry.
    pub selected: bool,
    /// A building's repair byte (`BuildingClass+0x6E8`), which only
    /// `production::toggle_repair` sets; the repair step pays for each
    /// `RepairStep=` of health while it is on.
    pub repairing: bool,
    /// LogicClass active-vector membership — mirrors gamemd ObjectClass+0x98.
    /// True iff this entity is currently in `Simulation::logic`. Not serialized:
    /// Rust snapshots rebuild it from the serialized LogicVector order. Exact
    /// native save/load reconstruction remains unverified.
    #[serde(skip)]
    pub in_logic_vector: bool,
    /// Independent, serialized ObjectClass lifecycle facts.
    #[serde(default)]
    pub lifecycle: ObjectLifecycle,
    /// Canonical TechnoClass playfield-membership byte (`TechnoClass+0x3D5`).
    ///
    /// gamemd-derived: the constructor clears it at `0x006F2F5B`, Unlimbo
    /// establishes an exact mode-one result at `0x006F6CFE`, ordinary cell
    /// movement promotes false to true without normally demoting it at
    /// `0x006F511A..0x006F5139` (the Teleport warp's PerCell(2) included), the
    /// Chronosphere warp's arrival state clears it outside the playfield at
    /// `0x00719A99` (`movement::teleport_chrono`), and
    /// `MapClass::Set_Clipped_LocalSize @ 0x00567230` recomputes every
    /// Techno exactly after a LocalSize writer. Consumers must read this stored
    /// fact; a fresh bounds query would erase the native movement hysteresis.
    #[serde(default)]
    pub in_playfield: bool,
    /// Retained Techno+3D4 (VERA name; historical native name unconfirmed).
    /// Constructor6F2F55 clears it; special Aircraft Unlimbo4143EB and
    /// reinforcement65E6BE promote it. Never infer it from current type,
    /// mission, cargo or +3D5. Ordinary click action692766 reads this history;
    /// forced ObjectSelect5F4578 has a separate virtual+A0 prerequisite.
    #[serde(default)]
    mission_only: bool,
    /// Explicit represented type fact for the native type `+0xAC` tactical-dirty
    /// branch. False unless a caller has positive evidence; never inferred from
    /// category or render representation.
    #[serde(default)]
    pub dirty_rect_eligible: bool,
    /// Parsed InfantryType occupation capability used by capture-target expiry.
    #[serde(default)]
    pub occupier: bool,
    /// Rust guard suppressing extra UnInit accounting after an actual shared
    /// RecordTheKill callback. Native callbacks themselves may record again.
    /// This does not stand in for native-alive or `dying`.
    #[serde(default)]
    pub destruction_recorded: bool,
    /// Foot's independent tracker Cell, slot-notification Cell and retained
    /// airborne-vector membership. Only the shared Foot air owner writes them.
    #[serde(default)]
    foot_air: foot_air::FootAirState,

    // --- Optional subsystem components ---
    /// Locomotor state — present on moving types and on zero-speed Foot
    /// Drive/Ship types whose native class-local payload still exists.
    pub locomotor: Option<LocomotorState>,
    /// Active movement path — present when unit is moving along an A* path.
    pub movement_target: Option<MovementTarget>,
    /// FootClass-style owner navigation destination state.
    ///
    /// Native `NavCom` is distinct from the active execution path, so this can
    /// remain visible after a `MovementTarget` or DriveTrack segment has cleared.
    #[serde(default)]
    pub navigation: NavigationState,
    /// Live Foot-owned applied speed; survives active locomotor replacement.
    #[serde(default)]
    pub foot_speed: crate::sim::components::FootSpeedState,
    /// Techno+2E8, ctor6F2E00 zero, saved70C354. Fly approach writes it;
    /// landing and Aircraft unload consume it. Independent of body rocking
    /// and of the active/suspended locomotor. Deterministic fixed radians.
    #[serde(default)]
    pub(crate) flight_attitude: crate::sim::movement::fly_height::FlightAttitude,
    /// Foot+6B6 raw occupation enable; shared by Drive/Ship and world Mark.
    /// Foot ctor4D344A initializes1; Unit7353CE invokes that base ctor.
    pub(crate) foot_occupation_enabled: bool,
    /// Foot+6AD, constructor4D3414=0. PerformDeploy710352 sets it after a
    /// forced locomotor swap; that producer is still an unimplemented receiver.
    /// This is independent from MCV Unit+68C and infantry deploy animation.
    #[serde(default)]
    pub(crate) foot_locomotor_swap_active: bool,
    /// Techno+0x1F8, constructor zero (`0x006F2CE1`). The Unit setter's
    /// Teleporter arm raises it when a Drive cannot end yet (`0x007425C6`), so
    /// the next setter call runs for an unchanged NavCom (`0x00741A88`); every
    /// setter call that passes that check clears it (`0x00741A9C`). Unlimbo's
    /// own raise and clear (`0x006F6E1B`/`0x006F6E34`) bracket one call and
    /// leave nothing behind.
    #[serde(default)]
    pub(crate) setter_force_reassign: bool,
    /// Techno+0x284, constructor zero (`0x006F2DDE`): the warp-in the
    /// Chronosphere's Teleport states time (`0x00719B23`). A blocked landing
    /// writes it (`[General] ChronoDelay=`, `0x00719983`) and it stays for
    /// every later warp of the object; its other writer, the chrono
    /// reinforcement (`0x0065F212`, `ChronoReinfDelay=`), is not ported.
    #[serde(default)]
    chrono_warp_delay: i32,
    /// Active attack target — present when entity is firing at something.
    pub attack_target: Option<AttackTarget>,
    /// A building's delayed fire: a delayed shot, or a Prism support beam.
    #[serde(default)]
    pub pending_building_fire: Option<PendingBuildingFire>,
    /// `BuildingClass+0x664`, the supporters a Prism master has recruited for
    /// its next shot (constructor 0, `0x0043B895`). Mission_Attack counts
    /// them (`0x0044B4D7`); the shot's bonus (`0x004504CD`), a support beam
    /// (`0x0044ACCA`) and Mission_Attack's null-target and drop tails
    /// (`0x0044AF9F`, `0x0044B0ED`) reset it.
    #[serde(default)]
    pub prism_support_count: i32,
    /// Techno+138, constructor6F2B99..6F2BCA=0; SetGunnerWeapon70DC70 owns writes.
    /// Combat selection and turret facing read the same signed weapon number.
    current_weapon_number: i32,
    /// Techno+124, constructor6F2B99..6F2BCA=-1. SetGunnerWeapon70DC70 and
    /// charge AI6FA4FB own writes; indexed voxel drawing is its consumer.
    current_turret_index: i32,
    /// Techno+2F8, constructor6F2E98=0. FireAt saves the final rearm duration
    /// independently of later timer writers; charge AI reads it for drawing.
    charge_turret_delay: i32,
    /// RadioClass-style live contacts for this entity, stored as stable IDs.
    /// Used by runtime building-entry/pathing exceptions such as contacted
    /// war factory exits and refinery dock entry. Kept per mover; a building
    /// being contacted does not globally relax passability for unrelated units.
    #[serde(default)]
    pub radio_contacts: Contacts,
    /// Models the TechnoClass dock-entered flag (ENTER_DOCK(0x18) sets it,
    /// LEAVE_DOCK(0x19)/BREAK clears it). `Some(other_sid)` while this entity is
    /// linked-and-entered at that dock partner; `None` otherwise. The radio
    /// receivers set and clear it; teardown paths that reset an entity also
    /// clear it.
    #[serde(default)]
    pub dock_entered_with: Option<u64>,
    /// Persistent TechnoClass hostile-hit latch (`WasAttackedByEnemy`, native
    /// byte +0x3D1). It is independent of retaliation's transient attacker
    /// pointer and is consumed by the building AI low-credit sell decision.
    #[serde(default)]
    pub was_attacked_by_enemy: bool,
    /// A building's AI sale byte (`BuildingClass+0x6DC`), which the
    /// computer's low-credit sale requires in a campaign (`0x00450781`). The
    /// constructor sets it (`0x0043B93D`) and Init_Managers clears it for a
    /// type without a Buildup (`0x00442CBC`); the map's `[Structures]` reader
    /// then writes its AI Sellable field (`0x0044FB5B`).
    #[serde(default)]
    pub ai_sellable: bool,
    /// A building's AI repair byte (`BuildingClass+0x6CB`), which lets the
    /// computer's auto-repair start repair it (`0x004506DE`). The constructor
    /// clears it (`0x0043B91F`); the map's `[Structures]` reader writes its AI
    /// Repairable field (`0x0044FB70`), then the building's Unlimbo sets it
    /// outside a campaign for a house no human controls (`0x00440B7A`), and
    /// so does an MCV deploy for a computer house (`0x007397F4`).
    #[serde(default)]
    pub ai_repairable: bool,
    /// The turret heading, `TechnoClass+0x3A0` (SecondaryFacing): a
    /// `Turret=yes` unit's and every aircraft's. A building aims its
    /// [`Self::body_facing`] instead.
    pub barrel_facing: Option<crate::sim::movement::FacingClass>,
    /// Turret rotation latch — `UnitClass+0x6AF`, written only by
    /// `UnitClass::Facing_Update @ 0x00736990` (cleared at `0x00736AD5`, re-set
    /// from `FacingClass::Is_Rotating` at `0x00736B16`) and read only by
    /// `UnitClass::GetFireError @ 0x00741233`. While set, the aim block is
    /// skipped so the turret finishes the arc it started instead of
    /// re-snapshotting every frame, and — unless the projectile homes — the
    /// fire gate answers FIRE_ROTATING(4) at `0x00741250`, refusing the shot
    /// until the arc completes.
    ///
    /// Not folded into `world_hash`: it is last tick's `barrel_facing`
    /// `Is_Rotating`, which the hash already folds, so a divergence surfaces
    /// one tick later through the interpolator itself.
    #[serde(default)]
    pub turret_rotation_latch: bool,
    /// Frame of this object's own last shot — `TechnoClass+0x120`. The
    /// constructor seeds `-100` (`0x006F2B9C`) and the single other writer in
    /// the image is `TechnoClass::Fire_At @ 0x006FF743`, which stores
    /// `g_CurrentFrameCounter`. `UnitClass::Facing_Update` gates the idle turret
    /// return on `frame - this >= GuardAreaTargetingDelay + 5`, so the dwell is
    /// measured from the unit's own last shot, NOT from target loss.
    /// BuildingClass::Update starts a Gattling building's idle decay once
    /// `frame - this > GuardAreaTargetingDelay + 5` (`0x0043FEE9`).
    ///
    /// Not folded into `world_hash`: its two consumers' results are, so a
    /// divergence surfaces one tick later through `barrel_facing` or
    /// `gattling`.
    #[serde(default = "default_last_fire_frame")]
    pub last_fire_frame: i64,
    /// The rearm countdown (`TechnoClass+0x2EC`, a CDTimerClass whose
    /// duration is `+0x2F4`). It belongs to the object, not to its target:
    /// `Assign_Target @ 0x006FCDB0` never touches it, so a new target does not
    /// reload the weapon.
    ///
    /// Writers, from a scan of every `+0x2EC..+0x2F4` operand in the image:
    /// - The constructor starts it at the current frame with no duration
    ///   (`0x006F2E81`). Ported.
    /// - `TechnoClass::FireAt`, every launched shot, with GetROF's value (the
    ///   drawn gap mid-burst, the full reload after the burst's last shot),
    ///   halved for a berserk firer (`0x006FF274..0x006FF2CB`). Its DiskLaser
    ///   path stores the value unhalved, fires the disk laser and returns
    ///   (`0x006FE4A4..0x006FE4EF`). Ported (`world_receiver::emit_admitted_fire`);
    ///   VERA fires a DiskLaser weapon through the ordinary path, keeping only
    ///   the unhalved rearm.
    /// - `AircraftClass::Drop_Payload` restarts it with no duration
    ///   (`0x00415E88`). Ported (the paradrop success arm).
    /// - A Prism support beam starts the supporter's downtime,
    ///   {Frame, `PrismSupportDelay=`} (`0x0044ACD0..0x0044ACDC`). Ported
    ///   (`building_missions::process_delayed_fire`).
    /// - RESIDUAL, with their mechanisms:
    ///   - the C4 plant arms the planter with `GetROF(1)`
    ///     (`InfantryClass::PerCellProcess 0x0051A564`/`0x0051A624`; see
    ///     `tick_c4_plants`);
    ///   - `UnitClass::ReceiveGunner`/`RemoveGunner` read a running rearm
    ///     (`0x0074643A`, `0x00746502`) and hand it over (`0x0074646E`,
    ///     `0x0074655C`; see `temporal.rs`);
    ///   - `UnitClass::PerCellProcess` arms a unit that stops with no NavCom
    ///     and no path with `GetROF(1) / 4` when its type has
    ///     `MobileFire=no` (`0x0073ADCA..0x0073AE18`). Dormant: no retail
    ///     type sets it (the constructor's default is yes, `0x00711150`).
    ///
    /// Readers: GetFireError answers Rearm while it runs (`0x006FC94F`);
    /// `CanAutoCloak @ 0x006FBDC0` waits for it; `FootClass::Mission_Guard`
    /// returns its remaining frames as the next delay and draws nothing
    /// (`0x004D52A9`); the Prism walk skips a tower while it runs
    /// (`0x0044B3AE`); `TechnoClass::Compute_CRC` folds it (`0x0070C362`).
    /// RESIDUAL:
    /// - `AircraftClass::Mission_Guard` falls into the same Foot body
    ///   (`0x0041A92B`), but VERA's aircraft Guard is not a port of it.
    /// - `CanDeploySlashUnload @ 0x00700D50` reads it only for a type with
    ///   `UndeployDelay > -1` (`0x00700DF9..0x00700E23`); that type refuses
    ///   deployment while it runs. Stock GI/GGI keep the reader default-1.
    ///
    /// The charge-turret model index reads this with the saved +2F8 duration
    /// (6FA540); `game_entity::gunner` owns that presentation state.
    #[serde(default)]
    pub rearm_timer: crate::sim::timer::CdTimer,
    /// Gattling stage, value and report latch (`TechnoClass+0x140`,
    /// `+0x144`, `+0x4B8`), owned by `combat::gattling`.
    #[serde(default)]
    pub gattling: crate::sim::combat::gattling::GattlingState,
    /// `TechnoClass+0x148`, the voxel turret's animation counter. The
    /// constructor zeroes it (`0x006F2BE2`); the unit firing update
    /// (`0x007370D5`, `0x007370F2`, `0x0073713A`) and the building's attack
    /// (`0x0044B23C`, `0x0044B713`) and Gattling block (`0x0043FE88`,
    /// `0x0043FF8B`) advance it. Only the draws read it
    /// (`UnitClass::DrawVoxelBody @ 0x0073B500`: while the body's HVA frame is
    /// 0 the turret's frame is this modulo its frame count; the building's
    /// voxel draw `0x0043DA80`, emitted by `emit_building_turret_vxl`).
    ///
    /// Not folded into `world_hash`: its one reader is presentation.
    #[serde(default)]
    pub turret_anim_frame: i32,
    /// Optional presentation-only native recoil, owned by voxel_recoil.
    voxel_recoil: Option<Box<voxel_recoil::VoxelRecoil>>,
    /// Techno+3B8 survives target replacement and mission changes.
    #[serde(default)]
    pub weapon_burst: crate::sim::combat::burst::WeaponBurst,
    /// Sole native Building+534/+538 body state; construction/sale use the
    /// existing private StageClass and MissionLeaf ready byte.
    building_body: Option<crate::sim::building_construction::BuildingBody>,
    /// Building+544: actual health sampled by440042..440074. Constructor
    /// 43B78F and successful ordinary Unlimbo440D2D reset it to zero.
    /// A repair changes actual health without invalidating its House. The
    /// next sample does invalidate it; any other native dirty writer can
    /// expose that live health through an earlier House assessment.
    building_power_health_sample: i32,
    /// Sale route metadata, never a second mission, ready byte or timer.
    building_sale: Option<crate::sim::building_construction::BuildingDown>,
    /// `BuildingClass+0x550..+0x558`, the wait before the building's
    /// computer factory tries its finished object again. The constructor
    /// starts it with no duration (`0x0043B7A7`, `0x0043B7AD`); a placement
    /// that must wait restarts it for `[General] PlacementDelay=`
    /// (`0x004501CB..0x004501F7`); only `production::factory_ai` reads it.
    #[serde(default)]
    pub(crate) ai_placement_timer: crate::sim::timer::CdTimer,
    /// Retained Building+6E6 animation-transition flag (saved454469).
    /// Selfheal and arbitrary health writes do not refresh this value.
    #[serde(default)]
    pub building_damage_state_active: bool,
    /// Building+55C's 21 retained AnimClass references. AnimStore owns the
    /// animation objects and their scheduler/timers; these are slot identities.
    #[serde(default)]
    pub building_anim_slots: [Option<u64>; 21],
    /// Building+5B0: effect slots marked for replay by4547C0, cleared by4545D0.
    #[serde(default)]
    pub building_anim_effect_replay: [bool; 21],
    /// Retained raw StorageClass slots and last refinery animation tier.
    #[serde(default)]
    pub(crate) building_storage: crate::sim::building_art::BuildingStorage,
    /// Building+6C8: last4555D0 result, sampled for every Building by43FB20.
    #[serde(default)]
    pub building_last_operational: bool,
    /// Building+6EA, constructor43B996 true; distinct from operational6C8.
    #[serde(default = "default_true")]
    pub building_stuff_enabled: bool,
    /// Building HasEngineer: starts false, changed-owner transfer sets true.
    /// Authored NeedsEngineer initialization consumes it; never infer from owner.
    #[serde(default)]
    pub building_has_engineer: bool,
    /// Building+6E4: OnConstructionComplete's one-time allocation guard.
    #[serde(default)]
    pub building_actually_placed: bool,
    /// Persisted type fact needed to recreate the owned light on later Unlimbo.
    #[serde(default)]
    pub spotlight_capable: bool,
    #[serde(default)]
    pub building_light: Option<BuildingLightRuntime>,
    /// Native BuildingClass damage-fire transition cache. Distinct from the
    /// generic damaged-art state above.
    #[serde(default)]
    pub damage_fire_state_active: bool,
    /// Eight fixed AnimClass ownership slots at Building+0x5C8..+0x5E4.
    #[serde(default)]
    pub damage_fire_anim_ids: [Option<crate::sim::anim_class::AnimId>; 8],
    /// Persistent bridge layer flag — authoritative source for "is this entity on a bridge?"
    /// Mirrors original engine's FootClass+0x8C. Survives repath operations that reset
    /// locomotor.layer. Set during spawn, updated at cell-crossing bridge transitions.
    #[serde(default)]
    pub on_bridge: bool,
    /// Runtime-only Foot bridge mismatch latch (`FootClass+0x68b` in YR).
    #[serde(default)]
    pub(crate) runtime_bridge_transition:
        crate::sim::movement::movement_bridge::RuntimeBridgeTransitionState,
    /// Infantry sprite animation state (sequence + frame + timing).
    pub animation: Option<Animation>,
    /// Voxel HVA animation state (frame cycling for multi-frame models).
    pub voxel_animation: Option<VoxelAnimation>,
    /// Harvest overlay animation (oregath.shp ore-gathering visual).
    pub harvest_overlay: Option<HarvestOverlay>,
    /// Harvester state machine (ore collection, refinery docking, cargo).
    pub miner: Option<Miner>,
    /// `SlaveManagerClass` of an `Enslaves=` master (`TechnoClass+0x2D8`).
    #[serde(default)]
    pub(crate) slave_manager: Option<crate::sim::slave_manager::SlaveManager>,
    /// The slave side of `SlaveManagerClass`: the master whose manager holds
    /// this slave (`TechnoClass+0x2DC`) and its Storage (`+0x33C`), written
    /// only by `sim::slave_manager`.
    #[serde(default)]
    pub(crate) slave: crate::sim::slave_manager::SlaveLink,
    /// Persistent high-level order (AttackMove, Guard) that survives transient state changes.
    pub order_intent: Option<OrderIntent>,
    /// Evidence-bounded native cloak transition state and visual producer values.
    #[serde(default)]
    pub cloak: Option<CloakRuntime>,
    /// Last sensor coverage actually deposited into CellClass counters.
    /// Removal must use these cached owner/location/radius values after the
    /// entity's current facts have changed.
    #[serde(default)]
    pub sensor_deposit: Option<crate::sim::sensor_lifecycle::SensorDeposit>,
    /// Core disguise identity, timestamp, and reveal tuple.
    #[serde(default)]
    pub disguise: Option<DisguiseRuntime>,
    /// Active low-bridge TubeClass movement. Active YR behaviour — not to be
    /// confused with the subterranean tunnel locomotor, which is Tiberian Sun
    /// legacy and was removed as unreachable in stock YR.
    #[serde(default)]
    pub low_bridge_tube_state: Option<LowBridgeTubeMovementState>,
    /// Controller-owned reversible mind-control manager (`TechnoClass+0x2BC`).
    /// Capacity and ordered victim links are authoritative runtime state; they
    /// cannot be reconstructed from the victims' back-links.
    #[serde(default)]
    pub capture_manager: Option<crate::sim::capture_manager::CaptureManagerState>,
    /// Spawn-manager pool carried by a `Spawns=` parent (V3 Launcher,
    /// Dreadnought, Boomer, Aircraft Carrier, Destroyer). Mirrors the native
    /// `TechnoClass+0x2D0` manager pointer: present iff `Spawns=` resolved.
    #[serde(default)]
    pub spawn_manager: Option<crate::sim::spawn_manager::SpawnManagerState>,
    /// Back-pointer from a spawned child to the parent that owns its pool
    /// (native child `+0x2D4`). Kill credit and the "do not self-RTB" gate
    /// read it; cleared when the parent releases the child.
    #[serde(default)]
    pub spawn_owner_id: Option<u64>,
    /// Parachute descent state. `Some` while a paradropped unit is descending
    /// under a parachute, `None` otherwise. Set by
    /// `parachute_descent::begin_parachute_descent`, cleared on landing. It
    /// is also the object's IsFallingDown ([`Self::is_falling_down`]).
    #[serde(default)]
    pub parachute_state: Option<crate::sim::movement::parachute_descent::ParachuteDescentState>,
    /// The IronCurtain or ForceShield state: the curtain's timer and its tint
    /// stage. `None` until the first curtain; `Some` after it, nullifying all
    /// damage (except healing) while the timer runs ([`is_invulnerable`]).
    /// Applied by superweapon launch handlers.
    ///
    /// [`is_invulnerable`]: crate::sim::superweapon::invulnerability::is_invulnerable
    #[serde(default)]
    pub invulnerability: Option<InvulnerabilityState>,
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
    /// Psychedelic/chaos runtime, separate from reversible mind control.
    #[serde(default)]
    pub berserk: BerserkState,
    /// Native Foot/Unit+500 entry target, independent of Radio contacts.
    /// Unit741D9F installs it; Foot70D84F/70D889 and expiry clear it.
    pending_entry: Option<u64>,
    /// Aircraft ammo tracking and airfield docking state.
    /// Present on all Aircraft, including signed negative native Ammo counts.
    /// Non-aircraft entities have no aircraft ammo owner.
    pub aircraft_ammo: Option<AircraftAmmo>,
    /// Infantry sub-cell position (0–4). Only meaningful for infantry.
    pub sub_cell: Option<u8>,
    /// Whether this entity can be crushed by vehicles (Crushable= in rules.ini).
    /// Default false — only specific infantry and some walls are crushable.
    pub crushable: bool,
    /// Whether deployed infantry remains crushable by regular crushers.
    /// Defaults true; `DeployedCrushable=no` low-silhouette infantry blocks regular crush.
    #[serde(default = "default_true")]
    pub deployed_crushable: bool,
    /// Techno+2A4, cleared by construction and retained independently of
    /// Doing. Infantry520B4E/520BAD update it after their Do_Action request
    /// even when the requested Deployed/Ready sequence refuses.
    #[serde(default)]
    native_crush_immunity: u8,
    /// Whether this entity can crush non-Crushable targets (OmniCrusher= in rules.ini).
    /// Only Battle Fortress has this in YR.
    pub omni_crusher: bool,
    /// Whether this entity has normal TechnoType `Crusher=yes` capability.
    /// Kept separate from MovementZone and OmniCrusher; activation waits for the
    /// Drive PerCellProcess path so legacy cell-based crush does not drift.
    #[serde(default)]
    pub regular_crusher: bool,
    /// Whether Drive/Ship locomotion should ramp toward its target speed fraction.
    /// Parsed from `Accelerates=` and kept separate from raw `Speed=`.
    #[serde(default = "default_true")]
    pub drive_accelerates: bool,
    /// Whether this entity is immune to ALL crush types (OmniCrushResistant= in rules.ini).
    pub omni_crush_resistant: bool,
    /// Whether this entity ignores per-cell radiation damage (ImmuneToRadiation= in rules.ini).
    #[serde(default)]
    pub immune_to_radiation: bool,
    /// Whether this entity is playing its death animation (health=0, not yet despawned).
    /// Dying entities are excluded from combat targeting, pathfinding, and selection.
    /// A dying object stays cell-marked until its terminal UnInit. Products
    /// derived from the entities (the movement pass's blocker plane and block
    /// sets) follow the flag through the store's touch log, like any other field.
    pub dying: bool,
    /// Retained Infantry lifetime policy; sprite animation stores progress only.
    pub(crate) infantry_terminal: Option<crate::sim::world::InfantryTerminal>,
    /// Foot+53C/+540 latch and countdown. The shared Foot sound owner alone
    /// writes these; the process-local audio handle is not simulation state.
    #[serde(default)]
    pub(crate) move_sound: crate::sim::world::MoveSoundState,
    /// `FootClass+0x425`, the crash latch: set by `FootClass::Crash @
    /// 0x004DEBB0` (`0x004DEC7F`) and by a Magnetron dropping an airborne
    /// object (`0x0070FF25`); cleared by the constructor (`0x006F2FF9`) and a
    /// Jumpjet Descend touchdown (`0x0054CA12`). A crashing object is alive
    /// (Object+90) with Health 0 and falls under its locomotor until the
    /// impact UnInits it. VERA ports no Magnetron, so its one writer is
    /// `Simulation::foot_crash` (`sim::world::crash`).
    #[serde(default)]
    pub crashing: bool,
    /// `FootClass+0x426`, the latch as the previous `FootClass::AI` saw it: the
    /// rising edge plays the crash voice and sound (`0x004DACDD..0x004DADC2`).
    #[serde(default)]
    pub crashing_seen: bool,
    /// Techno+3CD/+3CE retained sinking lifetime and prior Foot AI sound edge.
    /// `world::sinking` owns mutations; this is independent of crash +425/+426.
    #[serde(default)]
    pub(crate) sinking: crate::sim::world::SinkingState,

    // --- Passenger/transport system ---
    /// Combined passenger/transport role — replaces separate passenger_cargo,
    /// transport_id, and boarding_state fields. See `PassengerRole` variants.
    pub passenger_role: PassengerRole,
    /// Temporary VXL model override for visual-only state changes.
    /// When Some, the renderer should use this type's VXL model instead of `type_ref`.
    /// Set during refinery unloading (UnloadingClass= from rules.ini).
    pub display_type_override: Option<InternedId>,
    /// Techno+338, constructor6F2EC8=-1. Infantry capture519F94 stores its
    /// native InfantryType array index. Building consumers are diagnostics;
    /// the Unit hijacker lifecycle is a separate mechanism.
    capture_infantry_type_index: i32,
    /// Active C4 plant intent on this attacker. Set by `Command::PlantC4`,
    /// cleared on arrival (after the building's pending detonation is set),
    /// when the player retasks the unit, or when the target is lost.
    /// `None` for non-C4 attackers or attackers not currently planting.
    #[serde(default)]
    pub c4_plant: Option<C4PlantState>,
    /// Shared Building C4/PostMortem detonation latch. Infantry planting owns
    /// one producer; qualifying delayed-death receiver results own the other.
    /// The Building LogicVector visit consumes expiry synchronously.
    /// Never cleared in the C4 path — matches gamemd marker semantics.
    /// IronCurtain/ForceShield entry cancels it; `None` means no shared latch.
    #[serde(default)]
    pub pending_c4_detonation: Option<PendingC4Detonation>,
    /// Building `+0x6E3` HasBeenCaptured: cleared by the BuildingClass
    /// constructor (`0x0043B96C`), set by every `BuildingClass::ChangeOwner`
    /// (`0x00448723`). Read by the survivor count, the survivor roll and the
    /// building crew pick (`crew_survival`). Hashed and persisted (v194).
    #[serde(default)]
    pub has_been_captured: bool,
    /// Stable ID of the unit installed in a `Bunker=yes` building.
    ///
    /// Mirrors the live `BuildingClass+0x2E4` role for tank bunkers: an empty
    /// bunker can be skipped by the NumberImpassableRows helper, while an
    /// occupied bunker remains a normal building blocker.
    #[serde(default)]
    pub bunker_occupant: Option<u64>,
    /// Unit side of the bunker reciprocal link (approach + installed states).
    #[serde(default)]
    pub bunker_link: BunkerLink,
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
    /// Techno+130/+134, both cleared by the Techno constructor. Animation ownership
    /// and the Jumpjet landing request survive independently of Unit6E0..6E2.
    deploy_anim: Option<crate::sim::anim_class::AnimId>,
    landing_for_deploy: bool,
    /// Unit+0x68C: runtime Deploy continuation, not the type's DeployToFire.
    /// Writers/readers are owned by sim::mcv_deploy (gamemd 0x007393C0).
    #[serde(default)]
    pub(crate) mcv_deploy_pending: bool,
    /// Infantry fear/prone runtime. `None` for non-infantry entities.
    #[serde(default)]
    pub infantry: Option<InfantryRuntime>,
    /// Optional body-rocking state only. Drive/Ship slope transitions belong
    /// to their typed locomotor payloads. This defaults to `None` and becomes
    /// present only when an explicit body-rocking producer activates it.
    #[serde(default)]
    pub rocking: Option<RockingState>,
    /// Exact native-width Mission state. All writes pass through a named legacy
    /// compatibility adapter or the dormant exact-authority surface.
    pub mission: MissionCom,
    /// Techno passive-acquisition cadence timer. Expiry opens the passive
    /// target-scan gate; the scanner re-arms it to the `[General]` targeting
    /// delay plus a 0..=2 jitter. Also re-armed short when a live target's
    /// pointer expires. Armed at construction for
    /// [`PASSIVE_SCAN_CONSTRUCTION_DELAY_FRAMES`].
    #[serde(default)]
    pub passive_scan_timer: MissionTimer,
    /// Frame of this object's last passive target scan. Stamped on entry to the
    /// scanner. No consumer yet — it is the scanner's own bookkeeping write,
    /// modelled so the object's hashed state matches what the scan performed.
    #[serde(default)]
    pub last_target_scan_frame: u32,
    /// `TechnoClass+0x50C`: the passive block's scan changed the target
    /// (`0x006FA6EE`; the teleport clone `0x007094C8` is unported). Every
    /// Assign_Target clears it first (`0x006FCDC4`). Read by the scan's drop
    /// step (`0x007098C3`), the off-mission clear (`0x006FA5F0`) and the Unit
    /// approach (`0x0074162D`, VERA's pursuit skip).
    #[serde(default)]
    pub passively_acquired_target: bool,
    /// Foot+688, initialized false at4D33A8. A stopped object unable to fire
    /// narrows its next scans through Foot::Greatest_Threat4D9920. This is
    /// independent of Techno's passive-target byte+50C.
    #[serde(default)]
    foot_retarget_after_stop: bool,
    /// Category-specific bytes read by Mission readiness and Aircraft policy.
    pub(crate) mission_leaf: MissionLeafState,
    /// Target identity archived by the Techno Override wrapper.
    pub(crate) suspended_attack_target: Option<TargetKind>,
    /// Active House/base-defence recruitment, archive and attacker-cooldown
    /// bytes. Snapshot migration defaults reproduce Techno construction.
    #[serde(default)]
    pub(crate) base_defense_response: BaseDefenseResponseState,
    /// Sim-side model of gamemd's TechnoClass `+0x308` (`DamageSparkSystem`): the
    /// `session.tick` at which the live AI_Update damage-Spark particle system
    /// expires and the object may roll again. `0` = no live system (may roll;
    /// matches `+0x308 == NULL`); `u64::MAX` = a system whose `Lifetime <= 0`
    /// holds indefinitely; otherwise the system is live while `session.tick <
    /// live_until`. Set on a successful spark roll to `spawn_tick + sparkType.Lifetime`
    /// (matching the spawned `ParticleSystemClass` decrementing once per tick), so
    /// the per-object draw cadence stays bit-aligned with gamemd. Hashed (it gates
    /// future `scenario_rng` draws). Dormant in stock YR — see
    /// [`crate::rules::object_type::ObjectType::emits_damage_spark`].
    #[serde(default)]
    pub damage_particle_live_until: u64,
    /// Active TechnoClass damage-Smoke `ParticleSystemClass` identity
    /// (`TechnoClass +0x310`). The system is created synchronously by the
    /// surviving ReceiveDamage postlude and explicitly UnInit when health
    /// recovers above `ConditionYellow`.
    #[serde(default)]
    pub damage_smoke_system_id: Option<u64>,
    /// `UnitClass+0x6E4`: the number of passengers a transport's Unload keeps
    /// aboard. Written by `UnitClass::Mission_Unload @ 0x0073D630` state 0
    /// (`0x0073D830`/`0x0073D83C`: a `TurretCount > 0` transport keeps one
    /// unless it carries exactly one) and read by state 3's
    /// `cargo > keep` gate (`0x0073D8C3`). Zero for every other object.
    /// Hashed only when non-zero (see `world_hash`).
    #[serde(default)]
    pub transport_unload_keep_count: u32,
    /// `BuildingClass+0x6D0`/`+0x6D8` ProduceCash timer (oil derricks). Seeded
    /// to the constructor's dead state (`start = construction frame`,
    /// `duration = 0`, `BuildingClass::Constructor @ 0x0043B92B..0x0043B937`);
    /// only a capture from a `MultiplayPassive` house (`BuildingClass::
    /// ChangeOwner @ 0x004482DB..0x004482F9`) and the re-arm in
    /// `BuildingClass::Update @ 0x0043FD5B..0x0043FD86` give it a duration.
    /// `+0x6D4`, the middle dword, is scratch the fire test never reads.
    /// Zero-duration on every non-derrick object. Hashed (v135) and persisted.
    #[serde(default)]
    pub produce_cash_timer: crate::sim::timer::CdTimer,
    /// `TechnoClass+0x1CC DrainTarget`: the building this object is draining
    /// (Floating Disc). Set by `Fire_At`'s `DrainWeapon` arm, cleared by
    /// `UnitClass::AI`'s cell recheck, the ally check in `AI_Update`, and
    /// pointer expiry. Hashed (v135) and persisted.
    #[serde(default)]
    pub drain_target: Option<u64>,
    /// `TechnoClass+0x1D0 DrainingMe`: the object draining this one — the
    /// reciprocal of `drain_target`. The victim's `AI_Update` reads it for the
    /// money transfer. Hashed (v135) and persisted.
    #[serde(default)]
    pub draining_me: Option<u64>,
    /// Foot+69C `ParasiteImUsing`: this owner's ParasiteClass, allocated by
    /// `TechnoClass::Init_Managers @ 0x006F4145` for a weapon-0 Parasite
    /// warhead (attack dog, Terror Drone). Hashed and persisted (v193).
    #[serde(default)]
    pub parasite: Option<Box<crate::sim::combat::parasite::ParasiteState>>,
    /// Foot+694 `ParasiteEatingMe`: the owner whose parasite is attached to
    /// this victim (AttachTo `0x0062AB2B`). Hashed and persisted (v193).
    #[serde(default)]
    pub parasite_eating_me: Option<u64>,
    /// Foot+698: frame before which parasite shots at this Foot are refused
    /// (`TechnoClass::Fire @ 0x006FF81F`, GetFireError `0x006FCAE1`).
    #[serde(default)]
    pub parasite_launch_lock: u32,
    /// Foot+6A0 ParalysisTimer, read through [`Self::is_paralyzed`]. FootClass
    /// ctor `0x004D33FC` starts it at the construction frame with zero
    /// duration; ParasiteClass arms it (release, bites) and clears it.
    #[serde(default)]
    pub paralysis_timer: crate::sim::timer::CdTimer,
    /// Techno+432 ReselectIfLimboed memo (`TechnoClass::Fire @ 0x006FF79C`),
    /// consumed by a successful parasite release. Process-local like
    /// `selected`: written only for the local player's selection, so it is
    /// saved but never folded into the peer hash.
    #[serde(default)]
    pub limbo_reselect: bool,
    /// Debug event log — records movement/state transitions for the inspector panel.
    /// Only allocated when debug inspector is active (X hotkey). Not included in state hashing.
    #[serde(skip)]
    pub debug_log: Option<DebugEventLog>,
    /// Techno+508, the contribution last published to the House spatial maps.
    /// Constructor6F310C..6F311E leaves this dword unwritten; None preserves
    /// that domain until admitted alive Unlimbo6F6ED2 or Add70F689 writes it.
    /// It is independent of current type/garrison threat and survives saves.
    #[serde(default)]
    cached_spatial_threat: Option<i32>,
}

mod construction_stage;
mod gunner;
mod simple_deploy;
mod voxel_recoil;

impl GameEntity {
    /// Techno+0x284, the Chronosphere warp-in's frames.
    pub(crate) fn chrono_warp_delay(&self) -> i32 {
        self.chrono_warp_delay
    }

    pub(crate) fn set_chrono_warp_delay(&mut self, frames: i32) {
        self.chrono_warp_delay = frames;
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
            .map_or((0, 0), |c| (c.state, c.depth as i32));
        crate::sim::cloak_disguise::visual_character(
            state,
            progress,
            cloaking_stages,
            invisible,
            self.category == EntityCategory::Structure,
            self.discovery.owned_by_current_house,
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

    #[cfg(test)]
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

    pub(crate) const fn pending_entry(&self) -> Option<u64> {
        self.pending_entry
    }

    /// Native Unit741D9F / Foot70D84F / Techno707AE7 entry-pointer writer.
    pub(crate) fn set_pending_entry(&mut self, target: Option<u64>) {
        self.pending_entry = target;
    }

    pub(crate) const fn cached_spatial_threat(&self) -> Option<i32> {
        self.cached_spatial_threat
    }

    pub(crate) fn retain_spatial_threat(&mut self, value: i32) {
        self.cached_spatial_threat = Some(value);
    }
    pub(crate) fn native_stage(&self) -> &crate::sim::stage::StageClass {
        &self.stage
    }

    /// Read native Doing and shared +F8 for SHP drawing, including actions
    /// absent from the generic sequence vocabulary. Presentation never advances
    /// this clock or owns a writable copy of its progress. Doing-1 is retained;
    /// the frame-selector caller owns its native default-action selection.
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

    pub(crate) fn set_native_stage_value(&mut self, value: i32) {
        self.stage.set_value(value);
    }

    #[cfg(test)]
    pub(crate) fn install_native_stage_fixture(&mut self, stage: crate::sim::stage::StageClass) {
        self.stage = stage;
    }

    /// Invalidate the one live Foot path head. Native callers write
    /// Foot+5E0=-1; retained suffix/reference words and the destination
    /// remain intact.
    pub(crate) fn clear_live_path_head(&mut self) {
        self.navigation.path_replay.clear_live_head();
    }

    /// `TechnoClass::ArchiveTarget` (`Techno+0x218`): the base-defence
    /// responder's post, a harvester's archived ore cell and a factory's
    /// rally point share this one field, stored in
    /// [`BaseDefenseResponseState`].
    pub(crate) fn archive_target(&self) -> Option<crate::sim::combat::TargetKind> {
        self.base_defense_response.archive_target
    }

    /// `TechnoClass::Set_ArchiveTarget @ 0x0070C610`, a plain store.
    pub(crate) fn set_archive_target(&mut self, target: Option<crate::sim::combat::TargetKind>) {
        self.base_defense_response.archive_target = target;
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

    pub(crate) const fn is_mission_only(&self) -> bool {
        self.mission_only
    }

    /// Native SET writers are monotonic for this object's lifetime.
    pub(crate) fn mark_mission_only(&mut self) {
        self.mission_only = true;
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
        let Some(object) = rules.object(type_id) else {
            return;
        };
        let camera = crate::sim::combat::combat_weapon::primary_for_tier(object, self.veterancy())
            .and_then(|id| rules.weapon(id))
            .is_some_and(|weapon| weapon.camera);
        if !object.selectable || !object.landable || camera {
            self.mark_mission_only();
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

    /// Immutable indexed type identity, established by construction/decoding.
    pub fn type_ref(&self) -> InternedId {
        self.type_ref
    }

    pub(crate) const fn foot_retarget_after_stop(&self) -> bool {
        self.foot_retarget_after_stop
    }

    /// Drive4B2E9F / Ship6A24F2 / Hover51684D: the held target cannot
    /// be fired at after stopping. Set before the team Scan_Limit/target clear;
    /// Team6EC3BD also sets it after each member's class target clear.
    pub(crate) fn mark_stopped_cannot_fire(&mut self) {
        self.foot_retarget_after_stop = true;
    }

    /// FootGreatestThreat4D9951..55 clears only an empty completed base scan.
    /// Class overrides which return before Foot leave the byte untouched.
    pub(crate) fn finish_foot_threat_scan(&mut self, found: bool) {
        if !found {
            self.foot_retarget_after_stop = false;
        }
    }

    /// Mission_Rescue4DE03E resets the latch before its direct mask0 scan.
    pub(crate) fn clear_rescue_retarget_latch(&mut self) {
        self.foot_retarget_after_stop = false;
    }

    /// Foot4D9931..33 changes AL only; all upper mask bits survive.
    pub(crate) const fn coerce_foot_threat_mask(&self, mask: u32) -> u32 {
        if self.foot_retarget_after_stop {
            (mask & !2) | 1
        } else {
            mask
        }
    }

    /// The body heading at `frame`: `FacingClass::Current @ 0x004C93D0` on
    /// `+0x388`.
    pub(crate) fn body_facing_current(&self, frame: u32) -> u16 {
        self.body_facing.current(frame)
    }

    /// The body heading's high byte at `frame`, the 8-bit direction a reader
    /// that drops the DirStruct's low byte sees.
    pub(crate) fn body_facing_byte(&self, frame: u32) -> u8 {
        (self.body_facing.current(frame) >> 8) as u8
    }

    /// The body heading at `frame` as a rounded DirType byte,
    /// `((raw >> 7) + 1) >> 1`: the conversion gamemd applies when it passes
    /// `Current()` on as a direction argument (an Unlimbo's, a Do_Turn
    /// comparison's), wrapping 0xFF80.. to 0.
    pub(crate) fn body_facing_dir(&self, frame: u32) -> u8 {
        (((u32::from(self.body_facing.current(frame)) >> 7) + 1) >> 1) as u8
    }

    /// The barrel elevation (`+0x370`); its readers sample `current(frame)`
    /// (`FacingClass::Current @ 0x004C93D0`).
    pub(crate) fn barrel_elevation(&self) -> &crate::sim::movement::FacingClass {
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

    /// The class constructor's one rate write on `+0x388` (`Set_ROT @
    /// 0x004C9680`): the constant 127 for infantry (`0x00517BC5`), `ROT=`
    /// (`Type+0x71C`) for a unit (`0x00735579`), an aircraft (`0x00413FE7`)
    /// and a building (`BuildingClass::Init` at `0x00442CA5`).
    /// Veterancy level sampled from [`Self::veterancy_raw`]: 0 = rookie,
    /// 100 = veteran, 200 = elite. The damage multiplier, the armour divisor,
    /// elite weapon selection and the chevron read it.
    pub fn veterancy(&self) -> u16 {
        crate::sim::combat::veterancy::rank_u16(self.veterancy_raw)
    }

    /// Seed the accumulator at a rank's threshold (`SetVeteran @ 0x00750090`
    /// writes 1.0f, `SetElite @ 0x007500B0` 2.0f; a rookie is 0.0f), as a
    /// scenario-authored rank does.
    pub fn set_veterancy_rank(&mut self, rank_u16: u16) {
        self.veterancy_raw = crate::sim::combat::veterancy::raw_for_rank(rank_u16);
    }

    pub(crate) fn set_body_facing_rot(&mut self, type_rot: i32) {
        self.body_facing
            .set_rot(if self.category == EntityCategory::Infantry {
                127
            } else {
                type_rot
            });
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
        self.miner
            .as_ref()
            .is_some_and(|miner| miner.kind != crate::sim::miner::MinerKind::Slave)
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
            killed_by: None,
            cached_spatial_threat: None,
            dont_score: false,
            tracking_facts: Default::default(),
            stable_id,
            native_unique_id,
            native_cloak_displacement: crate::util::native_x87::NativeF32Bits::POSITIVE_ZERO,
            techno_ctor_random_word,
            stage: crate::sim::stage::StageClass::constructed(construction_frame as i32),
            discovery: TechnoDiscoveryHistory::default(),
            structure_upgrade_link: None,
            position: Position {
                rx,
                ry,
                z,
                exact_z_leptons: None,
                sub_x: init_sub_x,
                sub_y: init_sub_y,
            },
            // The Unlimbo direction; the class constructor supplies the rate.
            body_facing: crate::sim::movement::FacingClass::new(u16::from(facing) << 8, 0),
            barrel_elevation: crate::sim::movement::FacingClass::new(0, BARREL_ELEVATION_ROT),
            body_frame_counter: 0,
            owner,
            health,
            estimated_health: crate::sim::estimated_health::EstimatedHealth::from_raw(
                health.current,
            ),
            type_ref,
            category,
            foundation: default_foundation(),
            building_hidden_occupancy: (category == EntityCategory::Structure)
                .then(crate::rules::object_type::BuildingHiddenOccupancyProfile::default),
            base_reservation_spacing: None,
            determines_waypoint_edge: false,
            build_const_eligible: false,
            base_plan_type_index: -1,
            base_plan_is_defense: false,
            base_plan_has_undeploy_target: false,
            veterancy_raw: crate::sim::combat::veterancy::raw_for_rank(veterancy),
            veterancy_rank_cache: veterancy_rank_cache_default(),
            elite_flash_frames: 0,
            armor_multiplier: NativeF64Bits::ONE,
            firepower_multiplier: NativeF64Bits::ONE,
            vision_range,
            sight_is_zero: false,
            sight_refresh_timers: crate::sim::vision::SightRefreshTimers::at_construction(
                construction_frame,
            ),
            gap_generator: crate::sim::vision::GapGeneratorRuntime::default(),
            is_voxel,
            selected: false,
            repairing: false,
            in_logic_vector: false,
            lifecycle: ObjectLifecycle::default(),
            in_playfield: false,
            mission_only: false,
            dirty_rect_eligible: false,
            occupier: false,
            destruction_recorded: false,
            foot_air: foot_air::FootAirState::new(stable_id),
            locomotor: None,
            movement_target: None,
            navigation: NavigationState::at_frame(construction_frame),
            foot_speed: crate::sim::components::FootSpeedState::default(),
            flight_attitude: Default::default(),
            foot_occupation_enabled: true,
            foot_locomotor_swap_active: false,
            setter_force_reassign: false,
            chrono_warp_delay: 0,
            attack_target: None,
            pending_building_fire: None,
            prism_support_count: 0,
            current_weapon_number: 0,
            current_turret_index: -1,
            charge_turret_delay: 0,
            radio_contacts: Contacts::default(),
            dock_entered_with: None,
            was_attacked_by_enemy: false,
            ai_sellable: false,
            ai_repairable: false,
            barrel_facing: None,
            turret_rotation_latch: false,
            last_fire_frame: NATIVE_LAST_FIRE_FRAME_INIT,
            // `TechnoClass` constructor `0x006F2E81..0x006F2E8C`: started
            // at the construction frame with no duration.
            rearm_timer: crate::sim::timer::CdTimer::started(construction_frame as i32, 0),
            gattling: Default::default(),
            turret_anim_frame: 0,
            voxel_recoil: None,
            weapon_burst: Default::default(),
            building_body: (category == EntityCategory::Structure).then(Default::default),
            building_power_health_sample: 0,
            building_sale: None,
            ai_placement_timer: crate::sim::timer::CdTimer::default(),
            building_damage_state_active: false,
            building_anim_slots: [None; 21],
            building_anim_effect_replay: [false; 21],
            building_storage: Default::default(),
            building_last_operational: false,
            building_stuff_enabled: true,
            building_has_engineer: false,
            building_actually_placed: false,
            spotlight_capable: false,
            building_light: None,
            damage_fire_state_active: false,
            damage_fire_anim_ids: [None; 8],
            on_bridge: false,
            runtime_bridge_transition: Default::default(),
            animation: None,
            voxel_animation: None,
            harvest_overlay: None,
            miner: None,
            slave_manager: None,
            slave: Default::default(),
            order_intent: None,
            cloak: None,
            sensor_deposit: None,
            disguise: Some(DisguiseRuntime::new(construction_frame)),
            low_bridge_tube_state: None,
            capture_manager: None,
            spawn_manager: None,
            spawn_owner_id: None,
            parachute_state: None,
            invulnerability: None,
            mind_control: Default::default(),
            temporal: Default::default(),
            bomb: None,
            berserk: BerserkState::default(),
            pending_entry: None,
            aircraft_ammo: None,
            // Infantry get sub-cell 2 (first distinct position) at spawn so
            // they don't all pile up at cell center when multiple are created.
            sub_cell: if category == EntityCategory::Infantry {
                Some(2)
            } else {
                None
            },
            crushable: false,
            deployed_crushable: true,
            native_crush_immunity: 0,
            omni_crusher: false,
            regular_crusher: false,
            drive_accelerates: true,
            omni_crush_resistant: false,
            immune_to_radiation: false,
            dying: false,
            infantry_terminal: None,
            move_sound: crate::sim::world::MoveSoundState::default(),
            crashing: false,
            crashing_seen: false,
            sinking: crate::sim::world::SinkingState::default(),
            passenger_role: PassengerRole::None,
            display_type_override: None,
            capture_infantry_type_index: -1,
            c4_plant: None,
            pending_c4_detonation: None,
            has_been_captured: false,
            bunker_occupant: None,
            bunker_link: BunkerLink::None,
            building_gate: None,
            door: crate::sim::door::DoorClass::at_frame(construction_frame),
            bunker_runtime: None,
            deploy_anim: None,
            landing_for_deploy: false,
            mcv_deploy_pending: false,
            infantry: if category == EntityCategory::Infantry {
                Some(InfantryRuntime::new())
            } else {
                None
            },
            rocking: None,
            mission: MissionCom::at_frame(construction_frame),
            passive_scan_timer: MissionTimer::armed(
                construction_frame,
                PASSIVE_SCAN_CONSTRUCTION_DELAY_FRAMES,
            ),
            // `TechnoClass::Constructor 0x006F3106`: `+0x4FC = Frame`.
            last_target_scan_frame: construction_frame,
            passively_acquired_target: false,
            foot_retarget_after_stop: false,
            mission_leaf: MissionLeafState::constructed(category, construction_frame as i32),
            suspended_attack_target: None,
            base_defense_response: BaseDefenseResponseState::default(),
            damage_particle_live_until: 0,
            damage_smoke_system_id: None,
            transport_unload_keep_count: 0,
            produce_cash_timer: crate::sim::timer::CdTimer::started(construction_frame as i32, 0),
            drain_target: None,
            draining_me: None,
            parasite: None,
            parasite_eating_me: None,
            parasite_launch_lock: 0,
            paralysis_timer: crate::sim::timer::CdTimer::started(construction_frame as i32, 0),
            limbo_reselect: false,
            debug_log: None,
        }
    }

    /// Explicit zero-word constructor for tests that exercise construction
    /// frame anchoring without participating in gameplay RNG ownership.
    #[cfg(test)]
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
    #[cfg(test)]
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
    /// clears it when the fall grounds (`0x005F3F86`). The paradrop descent is
    /// VERA's only fall, so its state is the one owner.
    pub(crate) fn is_falling_down(&self) -> bool {
        self.parachute_state.is_some()
    }

    /// Put a fixture into a fall where it is, with a FallRate of 0.
    #[cfg(test)]
    pub(crate) fn set_falling_down_for_test(&mut self, falling: bool) {
        self.parachute_state = falling
            .then_some(crate::sim::movement::parachute_descent::ParachuteDescentState { rate: 0 });
    }

    /// Record a debug event if the event log is active. No-op when `debug_log` is `None`.
    pub fn push_debug_event(&mut self, tick: u32, kind: DebugEventKind) {
        if let Some(log) = &mut self.debug_log {
            log.push(tick, kind);
        }
    }

    /// Mark a live RadioClass-style contact with another entity.
    ///
    /// First-null slot insert: idempotent, and full slots deny without evicting
    /// (the receiver dock idiom). Slot order is hash-relevant and deterministic.
    pub fn mark_live_contact_with(&mut self, other_stable_id: u64) {
        self.radio_contacts.insert(other_stable_id);
    }

    /// Whether this entity has a live RadioClass-style contact with another entity.
    pub fn has_live_contact_with(&self, other_stable_id: u64) -> bool {
        self.radio_contacts.contains(other_stable_id)
    }

    /// Clear a live RadioClass-style contact with another entity (BREAK: nulls
    /// the slot in place, no compaction).
    pub fn clear_live_contact_with(&mut self, other_stable_id: u64) {
        self.radio_contacts.remove(other_stable_id);
    }

    /// Runtime movement/path layer with Ground as the fallback.
    ///
    /// This is not the object-list selector. Use `occupancy_list_layer` when
    /// selecting gamemd `FirstObject` versus `AltObject` style occupancy.
    pub fn movement_layer_or_ground(&self) -> crate::sim::movement::locomotor::MovementLayer {
        self.locomotor.as_ref().map_or(
            crate::sim::movement::locomotor::MovementLayer::Ground,
            |l| l.layer,
        )
    }

    /// Object-list layer for occupancy/cache membership.
    ///
    /// This mirrors gamemd's `ObjectClass+0x8C` / `OnBridge` selector for
    /// `CellClass::FirstObject` versus `AltObject`. It is intentionally not the
    /// same as locomotor/path layer; ramps can have `loco.layer` and `on_bridge`
    /// disagree for a tick.
    pub fn occupancy_list_layer(&self) -> Option<crate::sim::movement::locomotor::MovementLayer> {
        use crate::sim::movement::locomotor::MovementLayer;

        let motion_layer = self
            .locomotor
            .as_ref()
            .map_or(MovementLayer::Ground, |l| l.layer);
        if matches!(
            motion_layer,
            MovementLayer::Air | MovementLayer::Underground
        ) {
            return None;
        }

        Some(if self.on_bridge {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        })
    }

    /// Whether this entity is currently on a bridge deck.
    pub fn is_on_bridge_layer(&self) -> bool {
        self.on_bridge
    }

    /// Create a minimal entity for testing. Fills sensible defaults for most fields.
    #[cfg(test)]
    /// Create a minimal test entity with the given owner and type_ref strings.
    /// Uses a shared test interner via `test_intern()` for consistent IDs.
    pub fn test_default(stable_id: u64, type_ref: &str, owner: &str, rx: u16, ry: u16) -> Self {
        Self::test_default_of_category(stable_id, type_ref, owner, rx, ry, EntityCategory::Unit)
    }

    /// Construct the requested class before its private class state is installed.
    /// Tests must not turn a Unit into a Building by changing only `category`.
    #[cfg(test)]
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
            Health { current: 100 },
            crate::sim::intern::test_intern(type_ref),
            category,
            0, // veterancy = rookie
            5, // vision_range = 5 cells
            true,
        )
    }

    /// FootClass::IsParalyzed `0x004DE770` (vtable +0x380): the Foot+6A0
    /// timer has time left. Readers: GetFireError `0x006FC623`/`0x006FCCD5`,
    /// CloakingTick `0x006FB775`, ShouldUncloak `0x006FBCC0`, and the
    /// locomotor movement setters.
    pub fn is_paralyzed(&self, frame: u32) -> bool {
        self.paralysis_timer.remaining(frame as i32) != 0
    }

    /// Whether this entity is alive (health > 0).
    pub fn is_alive(&self) -> bool {
        self.health.current > 0
    }

    /// Whether ObjectClass native-alive state is set. This is intentionally
    /// independent from health and the Rust death-sequence state.
    pub fn is_object_alive(&self) -> bool {
        self.lifecycle.object_alive
    }

    /// Transitional Rust system gate until ordinary Infantry lifecycle authority
    /// migrates. Distinct from both health-based `is_alive()` and native-alive
    /// `is_object_alive()`; death-sequence state still suppresses current raw-store
    /// consumers even while ObjectClass native-alive remains set.
    pub fn is_active(&self) -> bool {
        self.lifecycle.object_alive && !self.dying
    }

    /// ObjectClass IsAlive (`+0x90`) as the per-object AI and a Unit's fire
    /// update read it. Native clears it when an ordinary death UnInits the
    /// object inside ReceiveDamage; VERA defers that UnInit, so Health 0 counts
    /// as dead here unless the object is crashing, and a wreck keeps IsAlive
    /// until its impact. A death sequence (`dying`) runs through its own path.
    pub fn is_ai_alive(&self) -> bool {
        self.is_active() && (self.health.current > 0 || self.crashing)
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
    pub(crate) fn infantry_deploy_doing(&self) -> bool {
        self.category == EntityCategory::Infantry
            && self
                .mission_leaf
                .as_infantry()
                .is_some_and(|leaf| (27..=30).contains(&leaf.doing()))
    }

    pub(crate) const fn native_crush_immunity(&self) -> u8 {
        self.native_crush_immunity
    }

    pub(crate) fn set_infantry_deploy_crush_immunity(&mut self, raw: u8) {
        assert_eq!(self.category, EntityCategory::Infantry);
        self.native_crush_immunity = raw;
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
    pub(crate) fn in_construction_bstate(&self) -> bool {
        self.building_body
            .as_ref()
            .is_some_and(|body| body.state() == 0)
    }

    /// Building440042..440074: compare actual health, publish the new sample
    /// and tell the host to invalidate the owning House's derived state.
    pub(crate) fn sample_building_health_for_power(&mut self) -> bool {
        if self.category != EntityCategory::Structure
            || self.building_power_health_sample == self.health.current
        {
            return false;
        }
        self.building_power_health_sample = self.health.current;
        true
    }

    pub(crate) fn reset_building_health_sample_at_unlimbo(&mut self) {
        if self.category == EntityCategory::Structure {
            self.building_power_health_sample = 0;
        }
    }

    pub(crate) fn building_power_health_sample(&self) -> Option<i32> {
        (self.category == EntityCategory::Structure).then_some(self.building_power_health_sample)
    }

    pub(crate) fn hash_building_health_sample(&self, hasher: &mut impl std::hash::Hasher) {
        if let Some(sample) = self.building_power_health_sample() {
            use std::hash::Hash;
            sample.hash(hasher);
        }
    }

    pub(crate) fn record_infantry_capture_type(&mut self, native_index: i32) {
        self.capture_infantry_type_index = native_index;
    }

    pub(crate) fn hash_capture_infantry_type(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        self.capture_infantry_type_index.hash(hasher);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native_cloak_fixture() -> serde_json::Value {
        serde_json::from_str(crate::test_fixture::text(
            "tools/procedural_drawing_oracle/translucent_blitter_a.json",
        ))
        .unwrap()
    }

    #[test]
    fn visual_character_matches_original_executed_transition_controls() {
        let native = native_cloak_fixture();
        let rows = native["cloak_transition"].as_array().unwrap();
        assert_eq!(rows.len(), 256);
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 10, 20);
        for row in rows {
            let mut cloak = CloakRuntime::new(0);
            cloak.state = row["state"].as_i64().unwrap() as i32;
            cloak.depth = row["depth"].as_i64().unwrap() as u32;
            entity.cloak = Some(cloak);
            entity.discovery.owned_by_current_house = row["owned"].as_bool().unwrap();
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
            let saved = bincode::serialize(&entity).unwrap();
            let restored: GameEntity = bincode::deserialize(&saved).unwrap();
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
    fn test_new_entity_defaults() {
        let e = GameEntity::test_default(1, "HTNK", "Americans", 30, 40);
        assert_eq!(e.stable_id, 1);
        assert_eq!(e.type_ref, crate::sim::intern::test_intern("HTNK"));
        assert_eq!(e.owner, crate::sim::intern::test_intern("Americans"));
        assert_eq!(e.position.rx, 30);
        assert_eq!(e.position.ry, 40);
        assert_eq!(e.position.z, 0);
        assert_eq!(e.body_facing_current(0), 0);
        assert_eq!(e.health.current, 100);
        assert_eq!(e.category, EntityCategory::Unit);
        assert_eq!(e.veterancy(), 0);
        assert_eq!(e.vision_range, 5);
        assert!(e.is_voxel);
        assert!(!e.selected);
        assert!(!e.repairing);
        assert!(e.locomotor.is_none());
        assert!(e.movement_target.is_none());
        assert!(e.attack_target.is_none());
        assert!(e.radio_contacts.is_empty());
        assert_eq!(e.rally_cell(), None);
        assert!(!e.was_attacked_by_enemy);
        assert!(e.barrel_facing.is_none());
        assert!(e.miner.is_none());
        assert!(e.order_intent.is_none());
        assert!(!e.on_bridge);
        assert!(
            !e.in_playfield,
            "TechnoClass ctor @ 0x006F2F5B initializes +0x3D5 false"
        );
    }

    #[test]
    fn occupancy_list_layer_from_on_bridge_not_loco_layer() {
        // GATE A2 / P5 (L13): the object-list layer is selected by the occupant's
        // OnBridge byte, NOT the locomotor/path layer. Pin a mismatch — loco.layer
        // = Ground while on_bridge = true — and assert the list layer follows
        // on_bridge (Bridge), the gamemd `Object+0x8C` selector.
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};

        let mut entity = GameEntity::test_default(1, "HTNK", "Americans", 5, 5);
        let mut loco = LocomotorState::for_test_kind(LocomotorKind::Drive);
        loco.layer = MovementLayer::Ground;
        entity.locomotor = Some(loco);

        entity.on_bridge = true;
        assert_eq!(
            entity.occupancy_list_layer(),
            Some(MovementLayer::Bridge),
            "list layer must follow on_bridge, not loco.layer"
        );

        entity.on_bridge = false;
        assert_eq!(
            entity.occupancy_list_layer(),
            Some(MovementLayer::Ground),
            "list layer must follow on_bridge when off the deck"
        );
    }

    #[test]
    fn gsi_05_10_pending_building_fire_serde_default_is_none() {
        let entity = GameEntity::test_default(1, "NATSLA", "Soviet", 30, 40);
        let mut value = serde_json::to_value(entity).expect("serialize entity");
        value
            .as_object_mut()
            .expect("entity object")
            .remove("pending_building_fire");

        let restored: GameEntity = serde_json::from_value(value).expect("deserialize entity");
        assert!(restored.pending_building_fire.is_none());
    }

    #[test]
    fn gsi_13_06_body_frame_counter_serde_default_is_zero() {
        let entity = GameEntity::test_default(1, "DRON", "Soviet", 30, 40);
        let mut value = serde_json::to_value(entity).expect("serialize entity");
        value
            .as_object_mut()
            .expect("entity object")
            .remove("body_frame_counter");

        let restored: GameEntity = serde_json::from_value(value).expect("deserialize entity");
        assert_eq!(restored.body_frame_counter, 0);
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

    #[test]
    fn test_is_alive() {
        let mut e = GameEntity::test_default(1, "E1", "Soviet", 10, 10);
        assert!(e.is_alive());
        e.health.current = 0;
        assert!(!e.is_alive());
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    #[test]
    fn lifecycle_authority_state_axes_are_independent() {
        let mut e = GameEntity::test_default(1, "E1", "Americans", 3, 3);
        assert_eq!(e.lifecycle, ObjectLifecycle::default());
        assert!(!e.in_logic_vector);
        assert!(!e.dirty_rect_eligible);
        assert!(!e.destruction_recorded);

        // Exercise every combination without deriving one fact from another.
        for bits in 0u8..64 {
            e.lifecycle.object_alive = bits & 0b00_0001 != 0;
            e.lifecycle.in_limbo = bits & 0b00_0010 != 0;
            e.lifecycle.cell_marked = bits & 0b00_0100 != 0;
            e.in_logic_vector = bits & 0b00_1000 != 0;
            e.dying = bits & 0b01_0000 != 0;
            e.health.current = if bits & 0b10_0000 != 0 { 1 } else { 0 };

            assert_eq!(e.is_object_alive(), bits & 0b00_0001 != 0);
            assert_eq!(e.lifecycle.in_limbo, bits & 0b00_0010 != 0);
            assert_eq!(e.lifecycle.cell_marked, bits & 0b00_0100 != 0);
            assert_eq!(e.in_logic_vector, bits & 0b00_1000 != 0);
            assert_eq!(e.dying, bits & 0b01_0000 != 0);
            assert_eq!(e.is_alive(), bits & 0b10_0000 != 0);
            assert_eq!(e.is_active(), e.lifecycle.object_alive && !e.dying,);
        }
    }
}

#[cfg(test)]
mod mission_shadow_tests {
    use super::*;
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};

    #[test]
    fn mission_defaults_to_idle_none() {
        let e = GameEntity::test_default(1, "E1", "Americans", 3, 3);
        assert_eq!(e.mission.current(), MissionId::NONE);
        assert_eq!(e.mission.handler_state(), 0);
        assert_eq!(e.mission.ai_counter(), 0);
    }

    /// A map placement authored as Sleep, Sticky or Harmless means "stand still
    /// and do nothing". Those missions have no completion transition, so the
    /// derived idle-Unit Guard reading must never take one back — otherwise
    /// every neutral civilian on a stock skirmish map scans and opens fire.
    #[test]
    fn stand_still_missions_beat_the_derived_guard_reading() {
        for mission in [
            MissionType::Sleep,
            MissionType::Sticky,
            MissionType::Harmless,
        ] {
            for category in [
                crate::map::entities::EntityCategory::Unit,
                crate::map::entities::EntityCategory::Infantry,
                crate::map::entities::EntityCategory::Structure,
            ] {
                let mut e = GameEntity::test_default(1, "CIVBTM", "Neutral", 3, 3);
                e.category = category;
                // No destination, order or target may override this mission.
                e.mission.apply_test_fixture(MissionTestFixture {
                    current: MissionId::from_known(mission),
                    suspended: MissionId::NONE,
                    queued: MissionId::NONE,
                    movement_bypass_latch: 0,
                    handler_state: 0,
                    mission_start_frame: 0,
                    ai_counter: 0,
                    dispatch_timer: MissionDispatchTimer::at_frame(0),
                });
                assert_eq!(
                    e.passive_acquire_mission(),
                    mission,
                    "{category:?} committed to {mission:?}"
                );
            }
        }
    }

    /// Arrival belongs to the Move handler, not the passive consumer. Until
    /// that owner commences Guard, an empty NavCom still reads Move.
    #[test]
    fn an_empty_navigation_goal_does_not_commence_guard() {
        let mut e = GameEntity::test_default(1, "MTNK", "Americans", 3, 3); // Unit
        e.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Move),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        assert_eq!(e.passive_acquire_mission(), MissionType::Move);
    }

    /// Area Guard is a job that never finishes AND has its own handler, which
    /// owns its acquisition. The finished-job bridge must not read it as Guard,
    /// or the object would be admitted to the passive-acquire block as well and
    /// scan twice per cadence. This is the state AI-slot starting units spawn
    /// in, so it is every non-human unit in every skirmish.
    #[test]
    fn committed_area_guard_is_never_bridged_to_guard() {
        for category in [
            crate::map::entities::EntityCategory::Unit,
            crate::map::entities::EntityCategory::Infantry,
        ] {
            let mut e = GameEntity::test_default(1, "MTNK", "Americans", 3, 3);
            e.category = category;
            e.mission.apply_test_fixture(MissionTestFixture {
                current: MissionId::from_known(MissionType::AreaGuard),
                suspended: MissionId::NONE,
                queued: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: MissionDispatchTimer::at_frame(0),
            });
            assert_eq!(
                e.passive_acquire_mission(),
                MissionType::AreaGuard,
                "{category:?} committed to Area Guard"
            );
        }
    }

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
