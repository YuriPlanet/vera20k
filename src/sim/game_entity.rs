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
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::map::entities::EntityCategory;
use crate::sim::aircraft::AircraftMission;
use crate::sim::animation::Animation;
use crate::sim::cloak_disguise::{CloakRuntime, DisguiseRuntime};
use crate::sim::combat::combat_weapon::WeaponSlot;
use crate::sim::combat::{AttackTarget, TargetKind};
use crate::sim::components::{
    BridgeOccupancy, BuildingDown, BuildingUp, C4PlantState, DriveLocomotionRuntime,
    HarvestOverlay, Health, MovementTarget, NavigationState, OrderIntent, PendingC4Detonation,
    Position, RockingState, ShipLocomotionRuntime, VoxelAnimation,
};
use crate::sim::debug_event_log::{DebugEventKind, DebugEventLog};
use crate::sim::deploy::DeployPhase;
use crate::sim::docking::aircraft_dock::AircraftAmmo;
use crate::sim::docking::building_dock::DockState;
use crate::sim::intern::InternedId;
use crate::sim::miner::Miner;
use crate::sim::mission::{MissionCom, MissionLeafState, MissionTimer, MissionType};
use crate::sim::movement::drop_pod_movement::DropPodState;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::rocket_movement::RocketState;
use crate::sim::movement::teleport_movement::TeleportState;
use crate::sim::movement::tube_movement::LowBridgeTubeMovementState;
use crate::sim::movement::tunnel_movement::TunnelState;
use crate::sim::passenger::PassengerRole;
use crate::sim::radio::Contacts;
use crate::sim::superweapon::invulnerability::InvulnerabilityState;
use crate::util::native_x87::NativeF64Bits;

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

fn default_last_fire_frame() -> i64 {
    NATIVE_LAST_FIRE_FRAME_INIT
}

fn default_base_plan_type_index() -> i32 {
    -1
}

/// Persistent TechnoClass state used by the active House base-defence
/// responder. The two admission bytes are constructor-true; archive/cooldown
/// writes occur only after a responder assignment or strict budget overshoot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct BaseDefenseResponseState {
    pub(crate) recruitable_a: bool,
    pub(crate) recruitable_b: bool,
    archive_target: Option<TargetKind>,
    pub(crate) cooldown_start_frame: i32,
    pub(crate) cooldown_duration_frames: i32,
}

impl Default for BaseDefenseResponseState {
    fn default() -> Self {
        Self {
            recruitable_a: true,
            recruitable_b: true,
            archive_target: None,
            cooldown_start_frame: -1,
            cooldown_duration_frames: 0,
        }
    }
}

fn default_armor_multiplier() -> NativeF64Bits {
    NativeF64Bits::ONE
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum BuildingGatePhase {
    #[default]
    ClosedStable,
    Opening,
    OpenStable,
    Closing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum BuildingGateMissionState {
    #[default]
    Setup,
    OpeningWait,
    OpenHold,
    BeginClose,
    ClosingWait,
    PostClose,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BuildingGateRuntime {
    pub mission_18_active: bool,
    pub phase: BuildingGatePhase,
    #[serde(default)]
    pub mission_state: BuildingGateMissionState,
    /// Open/close transition deferral. `duration` is the active remaining ticks;
    /// `start_frame` is the native helper baseline that direction reversal
    /// preserves while it rewrites the duration.
    #[serde(default)]
    pub transition_timer: MissionTimer,
    /// Nominal transition length (not a live timer) — direction reversal reads it
    /// to recompute the reversed remaining.
    #[serde(default)]
    pub transition_total_ticks: u32,
    /// Stable-open hold countdown (reseeds while occupants remain in the footprint).
    #[serde(default)]
    pub hold_timer: MissionTimer,
}

/// Parent-owned `BuildingLightClass` runtime. It exists only for a successfully
/// placed `HasSpotlight=yes` building and is removed with that parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BuildingLightRuntime {
    pub behavior: u8,
    pub target_id: Option<u64>,
}

impl Default for BuildingGateRuntime {
    fn default() -> Self {
        Self {
            mission_18_active: false,
            phase: BuildingGatePhase::ClosedStable,
            mission_state: BuildingGateMissionState::Setup,
            // armed(0, 0) — NOT the sentinel — preserves the exact numeric values
            // the old (last_frame, ticks_remaining) u32 pairs held at default,
            // keeping the gate's wrapping_sub arithmetic and the state hash identical.
            transition_timer: MissionTimer::armed(0, 0),
            transition_total_ticks: 0,
            hold_timer: MissionTimer::armed(0, 0),
        }
    }
}

impl BuildingGateRuntime {
    pub fn can_garrison_passable(self) -> bool {
        self.mission_18_active && self.phase == BuildingGatePhase::OpenStable
    }
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

/// Building shot held behind its art-authored firing animation delay.
///
/// The target is deliberately not captured: expiry reads the building's live
/// `attack_target`, while the selected weapon slot remains the one saved when
/// `BuildingClass::Mission_Attack` armed the shot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PendingBuildingFire {
    /// Signed native timer value, clamped to zero by ProcessDelayedFire after
    /// its pre-decrement.
    pub remaining_ticks: i32,
    pub weapon_slot: WeaponSlot,
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
}

/// The three evidence-backed ways a live Techno obtains its persistent
/// constructor word. Only `FreshScenario` is allowed to advance Scenario RNG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TechnoConstructorInit {
    FreshScenario,
    PreconsumedGenerated(GeneratedTechnoInit),
    Restored(u16),
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
    /// Low word of the one raw Scenario RNG draw performed by the active-retail
    /// `TechnoClass` constructor (`0x006F3254`, stored at native `+0x3C8`).
    /// Later report-selection consumers read this persistent value; placement
    /// failure never refunds the draw.
    pub techno_ctor_random_word: u16,
    pub discovery: TechnoDiscoveryHistory,
    /// Authored structure-upgrade identity. `None` for ordinary Technos.
    pub structure_upgrade_link: Option<StructureUpgradeLink>,
    /// World position in isometric cell coordinates + cached screen position.
    pub position: Position,
    /// Body facing direction (0–255, RA2 convention: 0=N, 64=E, 128=S, 192=W).
    pub facing: u8,
    /// Target body facing for gradual rotation (vehicles only).
    /// When `Some`, the entity is rotating in place and should not advance position.
    /// Infantry always turn instantly (RA2 behavior), so this stays `None` for them.
    pub facing_target: Option<u8>,
    /// Binary-frame hull FacingClass shared by movement and combat. Combat can
    /// retain an arbitrary16-bit heading without a movement `facing_target`;
    /// fresh Drive/Ship admission must use that full sample, not the `facing`
    /// byte used for display. Movement's completed byte-target adapter still
    /// retires this state; migrating all lifecycle writers to persistent native
    /// Facing ownership remains required.
    #[serde(default)]
    pub body_facing: Option<crate::sim::movement::FacingClass>,
    /// Persistent FootClass body-animation counter (`FootClass+0x538`).
    /// Unit SHP drawing takes the walk-frame remainder from this counter; it
    /// advances on absolute binary-frame cadence and never resets on a visual
    /// Stand/Walk transition.
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
    /// Veterancy level: 0 = rookie, 100 = veteran, 200 = elite.
    ///
    /// A projection of [`Self::veterancy_raw`], refreshed wherever the raw
    /// accumulator is written. Every existing reader — the damage multiplier,
    /// the armour divisor, elite weapon selection, the chevron — consumes this.
    pub veterancy: u16,
    /// The running accumulator every rank is sampled from.
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
    #[serde(default)]
    pub elite_flash_frames: u16,
    /// Mutable Techno instance armor multiplier. Native construction seeds
    /// this double to 1.0; armor powerups are its active non-neutral writer.
    #[serde(default = "default_armor_multiplier")]
    pub armor_multiplier: NativeF64Bits,
    /// House credited with destroying this object, captured at the instant its
    /// health reached zero. gamemd's kill-record step receives the actual killer
    /// at the moment of destruction; infantry linger in the logic vector through
    /// a death animation, so this field is that moment, recorded once.
    #[serde(skip)]
    pub killed_by: Option<InternedId>,
    /// Score value this object's destruction is worth to `killed_by`, resolved at
    /// the same instant from the type's `Cost=` and this object's veterancy.
    /// Resolved at capture time because the rules are in hand there and the
    /// veterancy is still the value it died at.
    #[serde(skip)]
    pub kill_award_points: i32,
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
    /// Building is being repaired (spending credits to heal).
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
    /// Chronosphere warp's arrival state can clear it at `0x00719A99` (not
    /// represented), and `MapClass::Set_Clipped_LocalSize @ 0x00567230` recomputes every
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
    /// Rust bookkeeping that makes the UnInit-time score record
    /// (`Simulation::record_destruction_once`) exactly-once. This does not
    /// stand in for native-alive or `dying`.
    #[serde(default)]
    pub destruction_recorded: bool,
    /// Monotonic order of the last successful insertion into a CellClass-style
    /// object list. Serialized because `OccupancyGrid` is a rebuilt cache; this
    /// is the authoritative fact needed to reconstruct its linked-list order.
    #[serde(default)]
    pub occupancy_enter_order: u64,
    /// Bucket membership in gamemd's independent 20 x 20 airborne-object
    /// spatial grid. Air movement updates this only on entry, exit, or a real
    /// bucket crossing; the ordinary cell-list insertion order is separate.
    #[serde(default)]
    pub air_spatial_bucket: Option<u16>,
    /// Append order inside `air_spatial_bucket`. The native bucket is a vector,
    /// so crossing into a bucket moves the object to that vector's tail.
    #[serde(default)]
    pub air_spatial_enter_order: u64,

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
    /// Active attack target — present when entity is firing at something.
    pub attack_target: Option<AttackTarget>,
    /// Generic non-Prism Building delayed-fire latch.
    #[serde(default)]
    pub pending_building_fire: Option<PendingBuildingFire>,
    /// TechnoClass `CurrentWeaponNumber`: the last live weapon slot selected
    /// for this object. Slot zero is the constructor state. Fatal receiver
    /// logic reuses this exact slot for the Suicide gate and death fallback.
    #[serde(default)]
    pub current_weapon_index: u8,
    /// Actual weapon identity returned by the most recent live selection.
    /// Class overrides can make this differ from the type's static slot.
    ///
    /// No simulation reader remains: the death weapon reads
    /// `GetCurrentWeapon` (`combat_weapon::current_weapon`) like native. It is
    /// kept only because every historical hash projection folds its per-fire
    /// value, which no earlier schema can reconstruct; retiring it re-pins
    /// those ratchet probes and is left to a dedicated change.
    #[serde(default)]
    pub current_weapon_ref: Option<InternedId>,
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
    /// Per-producer rally target cell for selected factory rally visuals.
    /// Owner-level `HouseState.rally_point` remains the production fallback.
    #[serde(default)]
    pub rally_target: Option<(u16, u16)>,
    /// Persistent TechnoClass hostile-hit latch (`WasAttackedByEnemy`, native
    /// byte +0x3D1). It is independent of retaliation's transient attacker
    /// pointer and is consumed by the building AI low-credit sell decision.
    #[serde(default)]
    pub was_attacked_by_enemy: bool,
    /// Independent turret/barrel facing — only on entities with Turret=yes in rules.ini.
    /// Timer-based 16-bit interpolator mirroring gamemd's BarrelFacing primitive.
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
    ///
    /// Not folded into `world_hash`: its only consumer is that idle return, so
    /// a divergence surfaces one tick later through `barrel_facing`, which the
    /// hash does fold.
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
    /// - RESIDUAL, with their mechanisms:
    ///   - the C4 plant arms the planter with `GetROF(1)`
    ///     (`InfantryClass::PerCellProcess 0x0051A564`/`0x0051A624`; see
    ///     `tick_c4_plants`);
    ///   - `UnitClass::ReceiveGunner`/`RemoveGunner` read a running rearm
    ///     (`0x0074643A`, `0x00746502`) and hand it over (`0x0074646E`,
    ///     `0x0074655C`; see `temporal.rs`);
    ///   - the Prism support beam arms a support tower with
    ///     `PrismSupportDelay=` (`0x0044ACB2`; unported Prism);
    ///   - `UnitClass::PerCellProcess` arms a unit that stops with no NavCom
    ///     and no path with `GetROF(1) / 4` when its type has
    ///     `MobileFire=no` (`0x0073ADCA..0x0073AE18`). Dormant: no retail
    ///     type sets it (the constructor's default is yes, `0x00711150`).
    ///
    /// Readers: GetFireError answers Rearm while it runs (`0x006FC94F`);
    /// `CanAutoCloak @ 0x006FBDC0` waits for it; `FootClass::Mission_Guard`
    /// returns its remaining frames as the next delay and draws nothing
    /// (`0x004D52A9`); `TechnoClass::Compute_CRC` folds it (`0x0070C362`).
    /// RESIDUAL:
    /// - `AircraftClass::Mission_Guard` falls into the same Foot body
    ///   (`0x0041A92B`), but VERA's aircraft Guard is not a port of it.
    /// - `CanDeploySlashUnload @ 0x00700D50` refuses a deployed infantryman's
    ///   undeploy while it runs (`0x00700E02`; see
    ///   `Command::ToggleInfantryDeploy`).
    /// - The Prism support-candidate loop skips a tower while it runs
    ///   (`0x0044B3AE`).
    /// - The charge-turret frame (`IsChargeTurret=`, the Prism Tank) reads it
    ///   with the `+0x2F8` ROF copy (`0x006FA540`), which VERA does not keep
    ///   or draw.
    #[serde(default)]
    pub rearm_timer: crate::sim::timer::CdTimer,
    /// Gattling stage, value and report latch (`TechnoClass+0x140`,
    /// `+0x144`, `+0x4B8`), owned by `combat::gattling`.
    #[serde(default)]
    pub gattling: crate::sim::combat::gattling::GattlingState,
    /// `TechnoClass+0x148`, the voxel turret's animation counter. The
    /// constructor zeroes it (`0x006F2BE2`); the unit firing update
    /// (`0x007370D5`, `0x007370F2`, `0x0073713A`) and the building's attack
    /// and update advance it. Only the draw reads it
    /// (`UnitClass::DrawVoxelBody @ 0x0073B500`: while the body's HVA frame is
    /// 0 the turret's frame is this modulo its frame count).
    ///
    /// Not folded into `world_hash`: its one reader is presentation.
    #[serde(default)]
    pub turret_anim_frame: i32,
    /// Techno+3B8 survives target replacement and mission changes.
    #[serde(default)]
    pub weapon_burst: crate::sim::combat::burst::WeaponBurst,
    /// Building construction animation progress.
    pub building_up: Option<BuildingUp>,
    /// Reverse build-up animation — building is undeploying into a mobile unit.
    pub building_down: Option<BuildingDown>,
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
    /// Bridge deck occupancy marker.
    pub bridge_occupancy: Option<BridgeOccupancy>,
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
    /// Teleport movement state machine (warp out/in phases).
    pub teleport_state: Option<TeleportState>,
    /// Dormant YR TunnelLocomotionClass process state. Its underground depth
    /// lives in the typed runtime, because `Position::z` cannot represent -256.
    #[serde(default)]
    pub tunnel_state: Option<TunnelState>,
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
    /// Rocket/missile flight state machine (launch/ascend/terminal/detonate).
    pub rocket_state: Option<RocketState>,
    /// Distinct DropPodLocomotionClass descent state; never shares parachute
    /// state or surface occupation while airborne.
    #[serde(default)]
    pub drop_pod_state: Option<DropPodState>,
    /// Homing missile flight state. `Some` while this entity is an in-flight
    /// homing projectile; `None` otherwise. Distinct from `rocket_state` —
    /// ballistic-arc rockets keep using `rocket_state`; only `Ranged=yes`
    /// projectiles attach a `HomingState`.
    #[serde(default)]
    pub homing_state: Option<crate::sim::movement::homing_movement::HomingState>,
    /// Parachute descent state. `Some` while a paradropped unit is descending
    /// under a parachute, `None` otherwise. Set by
    /// `parachute_descent::begin_parachute_descent`, cleared on landing.
    #[serde(default)]
    pub parachute_state: Option<crate::sim::movement::parachute_descent::ParachuteDescentState>,
    /// Active IronCurtain or ForceShield invulnerability timer.
    /// `None` = entity is vulnerable to damage. `Some` = all damage is nullified
    /// (except healing) until the timer expires. Applied by superweapon launch handlers.
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
    /// DriveLocomotion destination/head-to state separate from curve stepping.
    #[serde(default)]
    pub drive_locomotion: Option<DriveLocomotionRuntime>,
    /// ShipLocomotion destination/head-to and target speed state.
    #[serde(default)]
    pub ship_locomotion: Option<ShipLocomotionRuntime>,
    /// Docking state machine — present when unit is approaching, waiting,
    /// or servicing at a repair depot.
    pub dock_state: Option<DockState>,
    /// Aircraft ammo tracking and airfield docking state.
    /// Present on all Aircraft, including signed negative native Ammo counts.
    /// Non-aircraft entities have no aircraft ammo owner.
    pub aircraft_ammo: Option<AircraftAmmo>,
    /// Aircraft mission state machine — controls attack runs, guard, RTB, idle.
    /// Present on aircraft with Fly locomotor. None for non-aircraft and jumpjets.
    pub aircraft_mission: Option<AircraftMission>,
    /// Infantry sub-cell position (0–4). Only meaningful for infantry.
    pub sub_cell: Option<u8>,
    /// Whether this entity can be crushed by vehicles (Crushable= in rules.ini).
    /// Default false — only specific infantry and some walls are crushable.
    pub crushable: bool,
    /// Whether deployed infantry remains crushable by regular crushers.
    /// Defaults true; `DeployedCrushable=no` low-silhouette infantry blocks regular crush.
    #[serde(default = "default_true")]
    pub deployed_crushable: bool,
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
    /// Render-only depth bias used when this entity is under or near a bridge.
    pub zfudge_bridge: i32,
    /// Prevents the unit from taking under-bridge water routes.
    pub too_big_to_fit_under_bridge: bool,
    /// Whether this entity is playing its death animation (health=0, not yet despawned).
    /// Dying entities are excluded from combat targeting, pathfinding, and selection.
    /// A dying object stays cell-marked until its terminal UnInit. Products
    /// derived from the entities (the movement pass's blocker plane and block
    /// sets) follow the flag through the store's touch log, like any other field.
    pub dying: bool,
    /// Retained Infantry lifetime policy; sprite animation stores progress only.
    pub(crate) infantry_terminal: Option<crate::sim::world::InfantryTerminal>,
    /// Ticks remaining before a permanently blocked infantry scatters sideways.
    /// Set when movement is stuck on a non-temporary obstacle; counts down each tick.
    /// When it reaches 0, the unit scatters to a random adjacent cell instead of
    /// endlessly repathing to the same blocked destination.
    /// Original engine: 30-frame scatter queue interval.
    pub blocked_scatter_timer: u8,
    /// FootClass movement-sound handle state. Native starts the configured
    /// MoveSound on the object's own post-locomotor AI tail and keeps it alive
    /// through brief moving-now dropouts with a three-visit grace countdown.
    #[serde(default)]
    pub move_sound_active: bool,
    /// Remaining stopped AI visits before an active MoveSound is released.
    #[serde(default)]
    pub move_sound_countdown: u8,
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

    // --- Passenger/transport system ---
    /// Original owner of a CanBeOccupied building, saved when the first garrison
    /// occupant enters. Used to revert ownership when the last occupant exits.
    /// Matches original engine's `CheckAutoSellOrCivilian` which transfers back
    /// to the Civilian house — we store the actual pre-garrison owner instead of
    /// hardcoding "Neutral".
    pub garrison_original_owner: Option<InternedId>,
    /// Combined passenger/transport role — replaces separate passenger_cargo,
    /// transport_id, and boarding_state fields. See `PassengerRole` variants.
    pub passenger_role: PassengerRole,
    /// Weapon-selection override applied when this entity is acting as a
    /// transport firing a passenger's weapon. See `WeaponOverride` for the
    /// semantics of each variant — `IfvSlot` for Gunner=yes transports,
    /// `OpenTransport` for open-topped non-Gunner transports.
    ///
    /// Set by `passenger.rs` when a passenger boards; cleared when the
    /// transport is empty.
    pub weapon_override: Option<crate::sim::combat::combat_weapon::WeaponOverride>,
    /// Temporary VXL model override for visual-only state changes.
    /// When Some, the renderer should use this type's VXL model instead of `type_ref`.
    /// Set during refinery unloading (UnloadingClass= from rules.ini).
    pub display_type_override: Option<InternedId>,
    /// Target building for an engineer-arrival intent. Set by
    /// `CaptureBuilding`, cleared on arrival or if the target is lost.
    /// Overloaded: when the target's type has `BridgeRepairHut=yes`,
    /// `tick_bridge_repair_orders` consumes the engineer for bridge repair
    /// instead of capture (the original game never captures CABHUTs).
    pub capture_target: Option<u64>,
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
    pub building_gate: Option<BuildingGateRuntime>,
    /// Tank-bunker install state machine. `Some` on `Bunker=yes` buildings from
    /// spawn (state `Idle` when empty); its presence marks the entity as a tank
    /// bunker. Drives entry admission → install.
    #[serde(default)]
    pub bunker_runtime: Option<crate::sim::docking::bunker_install::BunkerRuntime>,
    /// Active deploy-fire phase. `None` = upright (default). `Some(Deploying)` /
    /// `Some(Deployed)` / `Some(Undeploying)` for the three machine states.
    /// Hashed for lockstep determinism. Set by `Command::ToggleInfantryDeploy`,
    /// advanced by `tick_deploy_state`. Animation reflects this; combat does not
    /// read it (weapon pick is target-driven).
    #[serde(default)]
    pub deploy_state: Option<DeployPhase>,
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
    /// Category-specific bytes read by Mission readiness and Aircraft policy.
    pub(crate) mission_leaf: MissionLeafState,
    /// Target identity archived by the Techno Override wrapper.
    pub(crate) suspended_attack_target: Option<TargetKind>,
    /// Active House/base-defence recruitment, archive and attacker-cooldown
    /// bytes. Snapshot migration defaults reproduce Techno construction.
    #[serde(default)]
    pub(crate) base_defense_response: BaseDefenseResponseState,
    /// ObjectClass falling-down byte read by Infantry readiness.
    pub(crate) object_is_falling_down: u8,
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
    /// `duration = 0`, `BuildingClass::Constructor @ 0x0043B92B`); armed only by
    /// a capture from a `MultiplayPassive` house. Zero-duration on every
    /// non-derrick object. Hashed (v135) and persisted.
    #[serde(default)]
    pub produce_cash_timer: crate::sim::credit_income::ProduceCashTimer,
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
}

impl GameEntity {
    /// `TechnoClass::ArchiveTarget` (`Techno+0x218`): the base-defence
    /// responder's post and a harvester's archived ore cell share this one
    /// field, stored in [`BaseDefenseResponseState`].
    pub(crate) fn archive_target(&self) -> Option<crate::sim::combat::TargetKind> {
        self.base_defense_response.archive_target
    }

    /// `TechnoClass::Set_ArchiveTarget @ 0x0070C610`, a plain store.
    pub(crate) fn set_archive_target(&mut self, target: Option<crate::sim::combat::TargetKind>) {
        self.base_defense_response.archive_target = target;
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
        let camera = crate::sim::combat::combat_weapon::primary_for_tier(object, self.veterancy)
            .and_then(|id| rules.weapon(id))
            .is_some_and(|weapon| weapon.camera);
        if !object.selectable || !object.landable || camera {
            self.mark_mission_only();
        }
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

    /// The mission the passive target-acquisition gate reads.
    ///
    /// The original always holds a real mission selector, so the gate can read
    /// it directly. VERA's mission substrate is mid-migration: an object that
    /// was never explicitly ordered still holds the `NONE` sentinel, so the
    /// authoritative selector wins when it names a known mission and the live
    /// machines fill in otherwise. A machine-less Structure reads as Guard —
    /// the original has no idle mission, and Guard is the arm that makes a base
    /// defence engage.
    ///
    /// A target the object's own scanner installed does NOT change its mission:
    /// the passive commit writes the target pointer and nothing else, so the
    /// object stays on Guard (or Move, or Harvest) and keeps rescanning on
    /// cadence. The legacy derivation reads any `attack_target` as Attack, which
    /// would latch the object out of the gate after a single scan — it would
    /// acquire once per target and then go dormant.
    ///
    /// VERA-INTERNAL bridge: the mission a freshly unlimboed object holds in
    /// the original is UNCHECKED, so this is a representation choice, not a
    /// verified value.
    ///
    /// The bridge also covers a committed mission whose work is over. VERA's
    /// mission substrate is written on the way IN — a Move, Stop, AttackMove,
    /// Enter or Capture order commits its selector — but almost nothing writes
    /// one back when the job finishes, so a unit that was ordered once holds
    /// that selector for the rest of the match. Read literally, a single move
    /// order would deafen a unit permanently: the gate admits Move only for the
    /// handful of stock types with `OpportunityFire`, and Stop is one of the
    /// missions that strips a scanner target outright. The original has no idle
    /// mission and its Move and Stop handlers return the object to Guard when
    /// they complete — the same shape as the building Attack handler's
    /// null-target arm. VERA has no equivalent handler, so when the committed
    /// mission's live machinery has gone quiet the derived reading wins instead.
    /// The gamemd handler equivalent is UNCHECKED.
    ///
    /// The bridge explicitly does NOT cover the missions that mean "stand still
    /// and do nothing" ([`MissionType::holds_until_retasked`]). Those never
    /// finish, so letting the derived Guard reading win there would make every
    /// map-authored Sleep/Sticky/Harmless placement scan and shoot.
    pub fn passive_acquire_mission(&self) -> MissionType {
        // A Health-0 wreck runs no mission handler (`MissionClass::AI @
        // 0x005B30A7`), so no job of its can finish and hand it back to Guard:
        // the passive block reads its committed mission as it stands
        // (`0x006FA697`).
        if self.health.current <= 0 {
            return self.mission.current().known().unwrap_or(MissionType::None);
        }
        let (derived, _) = self.derived_mission_with(!self.passively_acquired_target);
        let derived = if derived == MissionType::None
            && matches!(
                self.category,
                EntityCategory::Structure | EntityCategory::Infantry
            )
            && !self.passenger_role.is_inside_transport()
        {
            MissionType::Guard
        } else {
            derived
        };
        match self.mission.current().known() {
            // "Stand still and do nothing" is not a job that finishes — those
            // missions have no completion transition in the original either, so
            // the derived reading must never take one back to Guard. This is
            // what a map placement authored as Sleep, Sticky or Harmless means,
            // and without it every such neutral object scans and opens fire.
            Some(known) if known.holds_until_retasked() => known,
            // Area Guard is a job that never finishes: "hold this spot and
            // cover it" is the standing state, not a leftover selector. It also
            // has its own handler, which owns its target acquisition, and the
            // passive-acquire block admits {Move, Harvest, Guard} only. Letting
            // the finished-job bridge read it as Guard would put the object
            // through BOTH scanners on the same cadence.
            Some(MissionType::AreaGuard) => MissionType::AreaGuard,
            // A committed mission still doing something wins; one whose work is
            // finished defers to what the object is actually doing (nothing).
            Some(known) if !self.committed_mission_is_finished() => known,
            _ => derived,
        }
    }

    /// Whether the committed mission selector has outlived the work it named:
    /// nowhere to drive, no navigation goal, no standing player order, and no
    /// target beyond one the passive scanner installed. See
    /// [`GameEntity::passive_acquire_mission`] for why this exists.
    fn committed_mission_is_finished(&self) -> bool {
        self.movement_target.is_none()
            && self.navigation.nav_com.is_none()
            && self.order_intent.is_none()
            && (self.attack_target.is_none() || self.passively_acquired_target)
    }

    /// Classifier: the mission + sub-phase the legacy `Option<T>` machines
    /// imply. Since the authority flip, `mission` advances only through the
    /// exact verbs — this derivation is no longer projected into it and
    /// survives as the cross-check the harvest seam asserts against and as the
    /// fallback [`GameEntity::passive_acquire_mission`] uses for an object that
    /// still holds the `NONE` sentinel.
    #[cfg(test)]
    pub fn derived_mission(&self) -> (MissionType, u8) {
        self.derived_mission_with(true)
    }

    /// [`GameEntity::derived_mission`], with control over whether an installed
    /// `attack_target` implies mission Attack. Only the passive-acquire path
    /// passes `false`, and only for a target its own scanner installed.
    fn derived_mission_with(&self, attack_target_implies_attack: bool) -> (MissionType, u8) {
        if self.miner.is_some() {
            // The whole harvest loop is one mission; the FSM cursor of record
            // (MissionCom.handler_state) is its sub-phase.
            return (MissionType::Harvest, self.mission.handler_state() as u8);
        }
        if let Some(aircraft) = &self.aircraft_mission {
            return match aircraft {
                AircraftMission::Idle => (MissionType::Guard, 0),
                AircraftMission::Move { sub_state } => (MissionType::Move, *sub_state),
                AircraftMission::Attack { sub_state, .. } => (MissionType::Attack, *sub_state),
                AircraftMission::Guard => (MissionType::Guard, 0),
                AircraftMission::ReturnToBase { .. } => (MissionType::Enter, 0),
                AircraftMission::Docking { sub_state, .. } => (MissionType::Enter, *sub_state),
                AircraftMission::DockedIdle { .. } => (MissionType::Guard, 0),
                AircraftMission::ParaDropApproach { .. } => (MissionType::ParadropApproach, 0),
                AircraftMission::ParaDropOverfly { .. } => (MissionType::ParadropOverfly, 0),
            };
        }
        if self.dock_state.is_some() {
            return (MissionType::Enter, 0);
        }
        if attack_target_implies_attack && self.attack_target.is_some() {
            return (MissionType::Attack, 0);
        }
        if self.movement_target.is_some() {
            return (MissionType::Move, 0);
        }
        // S3: gamemd has no "None" mission — an idle ground vehicle sits in
        // Guard(5), dispatched at [Guard] Rate (and the passive-acquire gate
        // covers missions {Move, Harvest, Guard} only, so idle Units must be
        // Guard for the later acquisition slice to ever fire). Units only this
        // slice: infantry is S6, aircraft already maps idle via
        // aircraft_mission, buildings are S8. In-transport passengers keep the
        // legacy None placeholder until the enter-transport mission commit is
        // traced.
        if self.category == EntityCategory::Unit && !self.passenger_role.is_inside_transport() {
            return (MissionType::Guard, 0);
        }
        (MissionType::None, 0)
    }

    /// Construct after the owning world funnel has resolved the explicit
    /// `TechnoConstructorInit` capability. Kept inside `sim` so ordinary app,
    /// render, and diagnostic code cannot silently invent the native word.
    pub(in crate::sim) fn new_at_frame_from_constructor_word(
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
            kill_award_points: 0,
            dont_score: false,
            tracking_facts: Default::default(),
            stable_id,
            techno_ctor_random_word,
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
            facing,
            facing_target: None,
            body_facing: None,
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
            veterancy,
            veterancy_raw: crate::sim::combat::veterancy::raw_for_rank(veterancy),
            veterancy_rank_cache: veterancy_rank_cache_default(),
            elite_flash_frames: 0,
            armor_multiplier: NativeF64Bits::ONE,
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
            occupancy_enter_order: stable_id,
            air_spatial_bucket: None,
            air_spatial_enter_order: stable_id,
            locomotor: None,
            movement_target: None,
            navigation: NavigationState {
                path_runtime: crate::sim::components::FootPathRuntime::at_frame(construction_frame),
                ..NavigationState::default()
            },
            foot_speed: crate::sim::components::FootSpeedState::default(),
            flight_attitude: Default::default(),
            foot_occupation_enabled: true,
            foot_locomotor_swap_active: false,
            setter_force_reassign: false,
            attack_target: None,
            pending_building_fire: None,
            current_weapon_index: 0,
            current_weapon_ref: None,
            radio_contacts: Contacts::default(),
            dock_entered_with: None,
            rally_target: None,
            was_attacked_by_enemy: false,
            barrel_facing: None,
            turret_rotation_latch: false,
            last_fire_frame: NATIVE_LAST_FIRE_FRAME_INIT,
            // `TechnoClass` constructor `0x006F2E81..0x006F2E8C`: started
            // at the construction frame with no duration.
            rearm_timer: crate::sim::timer::CdTimer::started(construction_frame as i32, 0),
            gattling: Default::default(),
            turret_anim_frame: 0,
            weapon_burst: Default::default(),
            building_up: None,
            building_down: None,
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
            bridge_occupancy: None,
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
            disguise: None,
            teleport_state: None,
            tunnel_state: None,
            low_bridge_tube_state: None,
            capture_manager: None,
            spawn_manager: None,
            spawn_owner_id: None,
            rocket_state: None,
            drop_pod_state: None,
            homing_state: None,
            parachute_state: None,
            invulnerability: None,
            mind_control: Default::default(),
            temporal: Default::default(),
            bomb: None,
            berserk: BerserkState::default(),
            drive_locomotion: None,
            ship_locomotion: None,
            dock_state: None,
            aircraft_ammo: None,
            aircraft_mission: None,
            // Infantry get sub-cell 2 (first distinct position) at spawn so
            // they don't all pile up at cell center when multiple are created.
            sub_cell: if category == EntityCategory::Infantry {
                Some(2)
            } else {
                None
            },
            crushable: false,
            deployed_crushable: true,
            omni_crusher: false,
            regular_crusher: false,
            drive_accelerates: true,
            omni_crush_resistant: false,
            immune_to_radiation: false,
            zfudge_bridge: 0,
            too_big_to_fit_under_bridge: false,
            dying: false,
            infantry_terminal: None,
            blocked_scatter_timer: 0,
            move_sound_active: false,
            move_sound_countdown: 0,
            crashing: false,
            crashing_seen: false,
            garrison_original_owner: None,
            passenger_role: PassengerRole::None,
            weapon_override: None,
            display_type_override: None,
            capture_target: None,
            c4_plant: None,
            pending_c4_detonation: None,
            has_been_captured: false,
            bunker_occupant: None,
            bunker_link: BunkerLink::None,
            building_gate: None,
            bunker_runtime: None,
            deploy_state: None,
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
            mission_leaf: MissionLeafState::for_entity_category(category),
            suspended_attack_target: None,
            base_defense_response: BaseDefenseResponseState::default(),
            object_is_falling_down: 0,
            damage_particle_live_until: 0,
            damage_smoke_system_id: None,
            transport_unload_keep_count: 0,
            produce_cash_timer: crate::sim::credit_income::ProduceCashTimer::constructed(
                construction_frame,
            ),
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

    #[cfg(test)]
    pub(crate) fn set_object_is_falling_down_for_test(&mut self, raw: u8) {
        self.object_is_falling_down = raw;
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
        Self::new_at_frame_zero_for_test(
            stable_id,
            rx,
            ry,
            0, // z = ground level
            0, // facing = north
            crate::sim::intern::test_intern(owner),
            Health { current: 100 },
            crate::sim::intern::test_intern(type_ref),
            EntityCategory::Unit,
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

    /// Whether this entity is in any deploy phase (Deploying, Deployed, or Undeploying).
    /// Used by the 7 movement-command handlers to silently ignore movement orders.
    pub fn is_deployed(&self) -> bool {
        self.deploy_state.is_some()
    }

    /// Whether this entity has finished deploying and is in the stationary
    /// Deployed phase (not transitioning).
    pub fn is_fully_deployed(&self) -> bool {
        matches!(self.deploy_state, Some(DeployPhase::Deployed))
    }

    /// A building's current mission is Construction (0x12) or Selling
    /// (0x13). VERA keeps a building's build-up and its build-down (the
    /// Construction Yard repack) in `building_up`/`building_down` without
    /// publishing those missions, so either counts.
    pub(crate) fn constructing_or_selling(&self) -> bool {
        self.building_up.is_some()
            || self.building_down.is_some()
            || matches!(
                self.mission.current().known(),
                Some(MissionType::Selling | MissionType::Construction)
            )
    }

    /// A building's BState (`+0x534`) is 0, BSTATE_CONSTRUCTION: through its
    /// build-up (Unlimbo's `Begin_Mode(0)` until the Construction mission
    /// completes; Grand_Opening's queued `Begin_Mode(1)` applies at the end
    /// of that Update, `0x0043FFB4`), and through a pack-up from Selling's
    /// stage-1 `Begin_Mode(0)` to the conversion. A pack-up's stage 0 and 1
    /// visits run in the idle BState 1.
    pub(crate) fn in_construction_bstate(&self) -> bool {
        self.building_up.is_some()
            || self
                .building_down
                .as_ref()
                .is_some_and(|down| down.sell_stage >= 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_entity_defaults() {
        let e = GameEntity::test_default(1, "HTNK", "Americans", 30, 40);
        assert_eq!(e.stable_id, 1);
        assert_eq!(e.type_ref, crate::sim::intern::test_intern("HTNK"));
        assert_eq!(e.owner, crate::sim::intern::test_intern("Americans"));
        assert_eq!(e.position.rx, 30);
        assert_eq!(e.position.ry, 40);
        assert_eq!(e.position.z, 0);
        assert_eq!(e.facing, 0);
        assert_eq!(e.health.current, 100);
        assert_eq!(e.category, EntityCategory::Unit);
        assert_eq!(e.veterancy, 0);
        assert_eq!(e.vision_range, 5);
        assert!(e.is_voxel);
        assert!(!e.selected);
        assert!(!e.repairing);
        assert!(e.locomotor.is_none());
        assert!(e.movement_target.is_none());
        assert!(e.attack_target.is_none());
        assert!(e.radio_contacts.is_empty());
        assert_eq!(e.rally_target, None);
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
    fn new_entity_has_no_rally_target() {
        let e = GameEntity::test_default(1, "GAWEAP", "Americans", 30, 40);
        assert_eq!(e.rally_target, None);
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
    use crate::sim::combat::{AttackTarget, TargetKind};
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};

    #[test]
    fn mission_defaults_to_idle_none() {
        let e = GameEntity::test_default(1, "E1", "Americans", 3, 3);
        assert_eq!(e.mission.current(), MissionId::NONE);
        assert_eq!(e.mission.handler_state(), 0);
        assert_eq!(e.mission.ai_counter(), 0);
    }

    #[test]
    fn derived_mission_idle_when_no_machine_active() {
        // S3: an idle machine-less Unit sits in Guard (the gamemd idle mission
        // for ground vehicles); other categories keep the legacy None
        // placeholder until their slices land (infantry S6, buildings S8).
        let e = GameEntity::test_default(1, "E1", "Americans", 3, 3); // Unit
        assert_eq!(e.derived_mission(), (MissionType::Guard, 0));

        let mut s = GameEntity::test_default(2, "GAPILE", "Americans", 3, 3);
        s.category = crate::map::entities::EntityCategory::Structure;
        assert_eq!(s.derived_mission(), (MissionType::None, 0));

        let mut i = GameEntity::test_default(3, "E1", "Americans", 3, 3);
        i.category = crate::map::entities::EntityCategory::Infantry;
        assert_eq!(i.derived_mission(), (MissionType::None, 0));
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
                // No destination, no order, no target: the committed mission
                // reads as "finished", which is what used to hand it to Guard.
                assert!(e.committed_mission_is_finished());
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

    /// The bridge still applies to missions whose work really does end: an
    /// idle Unit that was ordered to Move once must not stay deaf forever.
    #[test]
    fn a_finished_ordinary_mission_still_defers_to_the_derived_reading() {
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
        assert!(e.committed_mission_is_finished());
        assert_eq!(e.passive_acquire_mission(), MissionType::Guard);
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
            assert!(e.committed_mission_is_finished());
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
    fn passenger_derive_unchanged_placeholder() {
        // In-transport passengers keep the legacy None placeholder: the
        // mission a passenger holds inside a transport is untraced (do NOT
        // guess Sleep/Guard); this pin flips with the traced value later.
        let mut e = GameEntity::test_default(1, "E1", "Americans", 3, 3);
        e.passenger_role = crate::sim::passenger::PassengerRole::Inside { transport_id: 99 };
        assert_eq!(e.derived_mission(), (MissionType::None, 0));
    }

    #[test]
    fn derived_mission_tracks_attack_target() {
        let mut e = GameEntity::test_default(1, "E1", "Americans", 3, 3);
        e.attack_target = Some(AttackTarget {
            target: TargetKind::Entity(2),
            pending_infantry_fire: None,
        });
        assert_eq!(e.derived_mission().0, MissionType::Attack);
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
