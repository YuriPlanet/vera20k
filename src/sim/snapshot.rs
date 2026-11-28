//! Simulation snapshot serialization for mid-match save/load.
//!
//! Serializes the full `Simulation` state into a compact binary blob via
//! `util::save_codec`, which writes bincode 1's layout. Caches and event
//! queues are `#[serde(skip)]`'d on `Simulation` and must be rebuilt through
//! the validated cache and map-authority restore sequence before the
//! simulation is exposed again.
//!
//! ## Dependency rules
//! - Part of sim/ — depends only on sim/world (Simulation).
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

pub mod migration;
mod object_references;
pub(super) mod retired_driver;
mod saved_ids;
#[path = "snapshot/saved_local_view.rs"]
pub mod saved_local_view;
#[path = "snapshot/scenario_resources.rs"]
pub mod scenario_resources;
mod simulation_restore;

use saved_local_view::SavedLocalView;
use scenario_resources::{SavedScenarioResources, SavedScenarioResourcesRef};

use crate::map::resolved_terrain::SavedSharedCellDummy;
use crate::rules::compatibility::RulesCompatibility;
use crate::sim::intern::InternedId;
use crate::sim::occupancy::RawCellOccupation;
use crate::sim::pathfinding::zone_hierarchy::{ZONE_PRECHECK_LEVELS, ZoneLevelGraph};
use crate::sim::world::Simulation;
use crate::util::save_codec;

/// The layout this build writes and accepts: a digest of the
/// [`save_codec::layout`] description of [`GameSnapshot`], traced once per
/// process. It changes whenever a saved type, field or enum variant does, so
/// nothing bumps it by hand and two branches cannot share one id for different
/// layouts. A change in what an unchanged field means renames that field. The
/// top bit is always set; the hand-kept counter used before stayed far below
/// it, so those saves read as another layout.
///
/// The browser build takes the same id from the build instead
/// (`VERA20K_SNAPSHOT_LAYOUT`, which tools/wasm/build.py sets from
/// `src/bin/snapshot-layout.rs`), so the tracing code stays out of the
/// package every player downloads.
fn snapshot_layout() -> u32 {
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    {
        env!("VERA20K_SNAPSHOT_LAYOUT")
            .parse()
            .expect("VERA20K_SNAPSHOT_LAYOUT is a snapshot layout id")
    }
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    {
        static LAYOUT: OnceLock<u32> = OnceLock::new();
        *LAYOUT.get_or_init(traced_on_a_game_stack)
    }
}

/// [`traced_snapshot_layout`] on a thread of its own with the 8 MiB stack of
/// the game's own threads. The trace runs under whichever caller first saves
/// or loads, and it decodes a made-up snapshot through every saved type:
/// unoptimized, its frames hold about 1.9 MiB of whole snapshots and worlds,
/// more than a 2 MiB thread has left. A caller whose thread did not start or
/// did not finish traces itself.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
fn traced_on_a_game_stack() -> u32 {
    std::thread::Builder::new()
        .name("snapshot layout".into())
        .stack_size(crate::util::GAME_STACK_BYTES)
        .spawn(traced_snapshot_layout)
        .ok()
        .and_then(|trace| trace.join().ok())
        .unwrap_or_else(traced_snapshot_layout)
}

/// [`snapshot_layout`], traced from the saved types.
pub fn traced_snapshot_layout() -> u32 {
    let description = save_codec::layout::describe::<GameSnapshot>()
        .unwrap_or_else(|error| panic!("cannot describe the snapshot layout: {error}"));
    let digest = crate::util::sha256::sha256_bytes(description.as_bytes());
    u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]]) | 0x8000_0000
}

const SNAPSHOT_PRODUCT_MAGIC: [u8; 8] = *b"VERA20K\0";
const SNAPSHOT_ENVELOPE_VERSION: u32 = 2;

/// Neutral type identity shared by the retained sidebar and its save envelope.
/// The kind matters: a SuperWeapon and an object never share a cameo entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SavedCameoId {
    Object(InternedId),
    SuperWeapon(InternedId),
}

/// Identity/order survive save/load; comparison keys are projections of
/// the restored simulation, canonical rules and current process string table.
/// Strip indices are building, defense, infantry and vehicle, respectively.
///
/// SidebarClass Save 6AC5E0 / Load 6AC5D0 only forward to RadarClass; Mouse
/// Save5BE6D0 writes the whole 0x556C object at5BE715..5BE71C, including
/// four strips at +1544, stride F94, and ActiveTab at +539C. Load5BDF70 reads
/// it at5BE089..5BE092; no-init5BE9B0->6A4F20 resets progress6AC7C0,
/// preserving count/type/order and the open tab.
/// Each strip's saved TopRow is absent: Load_Game_From_File calls TabClass Init
/// 6D03A0 at 67E68F, whose SidebarClass Init 6A5310 hides and shows the shown
/// sidebar, and ToggleSidebar 6A7D70 zeroes every strip's TopRow on the show.
/// Residual: each strip's PendingScroll, saved inside the same object, is not
/// carried, so a scroll still rolling at the save does not finish after the
/// load. Trigger: saving within the few frames after an arrow or wheel
/// scroll; a player cannot see it.
/// This neutral transport does not introduce a sim dependency on a UI owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedSidebarOrder {
    owner: InternedId,
    strips: [Vec<SavedCameoId>; 4],
    /// Each strip's record flash deadlines (+0x88): one per entry, then the
    /// empty records up to the last one a cameo flash lit (at most 75 records).
    flash_deadlines: [Vec<i32>; 4],
    redraw_until: i32,
    request_states: [Vec<u8>; 4],
    factory_links: [Vec<Option<crate::sim::production::ProductionFactoryVisit>>; 4],
    prediction_categories: Vec<crate::sim::production::ProductionCategory>,
    /// The open strip's index (Sidebar `ActiveTab`, `+0x539C`), part of the
    /// same 0x556C object Mouse Save/Load write and read.
    active_tab: u8,
    /// Phobos `ScenarioExt::SWSidebar_Enable`, false while the left sidebar is
    /// collapsed. Companion state to this local presentation envelope.
    #[cfg(feature = "phobos")]
    sw_sidebar_expanded: bool,
}

impl SavedSidebarOrder {
    pub fn new(owner: InternedId, strips: [Vec<SavedCameoId>; 4]) -> Self {
        let flash_deadlines = std::array::from_fn(|index| vec![0; strips[index].len()]);
        let request_states = std::array::from_fn(|index| vec![0; strips[index].len()]);
        let factory_links = std::array::from_fn(|index| vec![None; strips[index].len()]);
        Self {
            owner,
            strips,
            flash_deadlines,
            redraw_until: 0,
            request_states,
            factory_links,
            prediction_categories: Vec::new(),
            active_tab: 0,
            // `ScenarioExt::SWSidebar_Enable { true }` in Phobos's constructor.
            #[cfg(feature = "phobos")]
            sw_sidebar_expanded: true,
        }
    }

    /// Phobos stores `SWSidebar_Enable` in the scenario extension, outside the
    /// strips; this envelope carries it beside them for the same save.
    #[cfg(feature = "phobos")]
    pub fn sw_sidebar_expanded(&self) -> bool {
        self.sw_sidebar_expanded
    }

    #[cfg(feature = "phobos")]
    pub fn set_sw_sidebar_expanded(&mut self, expanded: bool) {
        self.sw_sidebar_expanded = expanded;
    }

    pub fn with_flash(mut self, deadlines: [Vec<i32>; 4], redraw_until: i32) -> Self {
        self.flash_deadlines = deadlines;
        self.redraw_until = redraw_until;
        self
    }

    pub fn with_requests(mut self, states: [Vec<u8>; 4]) -> Self {
        self.request_states = states;
        self
    }

    pub fn requests(&self) -> &[Vec<u8>; 4] {
        &self.request_states
    }

    /// Mouse Save5BE6D0 retains each entry's Factory pointer (+64), including
    /// associations to a Factory whose current product differs from the cameo.
    /// VERA saves that foreign identity; the app validates it against the loaded
    /// registry before installing the retained strips.
    pub fn with_factory_links(
        mut self,
        links: [Vec<Option<crate::sim::production::ProductionFactoryVisit>>; 4],
    ) -> Self {
        self.factory_links = links;
        self
    }

    pub fn factory_links(
        &self,
    ) -> &[Vec<Option<crate::sim::production::ProductionFactoryVisit>>; 4] {
        &self.factory_links
    }

    /// HouseSave504080/Load503040 retain the independent category gates;
    /// post-load NoInit4F5190 does not initialize them again. The client
    /// owns these local prediction decisions along with its cameo requests.
    pub fn with_prediction_categories(
        mut self,
        categories: Vec<crate::sim::production::ProductionCategory>,
    ) -> Self {
        self.prediction_categories = categories;
        self
    }

    pub fn prediction_categories(&self) -> &[crate::sim::production::ProductionCategory] {
        &self.prediction_categories
    }

    pub fn with_active_tab(mut self, index: u8) -> Self {
        self.active_tab = index;
        self
    }

    pub fn active_tab(&self) -> u8 {
        self.active_tab
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn owner(&self) -> InternedId {
        self.owner
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn strips(&self) -> &[Vec<SavedCameoId>; 4] {
        &self.strips
    }

    /// Hands the loader every part by value, so it validates and installs the
    /// strips without copying them.
    pub fn into_parts(self) -> SavedSidebarOrderParts {
        SavedSidebarOrderParts {
            owner: self.owner,
            strips: self.strips,
            flash_deadlines: self.flash_deadlines,
            redraw_until: self.redraw_until,
            request_states: self.request_states,
            factory_links: self.factory_links,
            prediction_categories: self.prediction_categories,
            active_tab: self.active_tab,
            #[cfg(feature = "phobos")]
            sw_sidebar_expanded: self.sw_sidebar_expanded,
        }
    }
}

/// [`SavedSidebarOrder`] taken apart by [`SavedSidebarOrder::into_parts`].
pub struct SavedSidebarOrderParts {
    pub owner: InternedId,
    pub strips: [Vec<SavedCameoId>; 4],
    pub flash_deadlines: [Vec<i32>; 4],
    pub redraw_until: i32,
    pub request_states: [Vec<u8>; 4],
    pub factory_links: [Vec<Option<crate::sim::production::ProductionFactoryVisit>>; 4],
    pub prediction_categories: Vec<crate::sim::production::ProductionCategory>,
    pub active_tab: u8,
    #[cfg(feature = "phobos")]
    pub sw_sidebar_expanded: bool,
}

/// TacticalSave6DBE00 writes the raw Tactical object; Load6DBD20 retains its
/// focus fields and LastAIFrame through NoInit6D1E30. Load re-applies the
/// clamped view and overwrites RequestedViewPosition with that view, so pending
/// scroll is deliberately absent from this neutral presentation transport.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedTacticalView {
    /// Committed tactical center in VERA world pixels, including its existing
    /// projection bias. UpdateViewport6D8B30 derives the viewport from it and
    /// the current viewport size at load, so the window size is not saved.
    pub view_center_world_pixels: [f32; 2],
    pub zoom: f32,
    pub focus_start: [i32; 2],
    pub focus_target: [i32; 2],
    pub focus_rate_bits: u32,
    pub focus_progress_bits: u32,
    pub last_ai_frame: u32,
}

/// Binary snapshot envelope — wraps the full `Simulation` state plus
/// compatibility hashes for the map and rules that were active at save time.
#[derive(Serialize, Deserialize)]
pub struct GameSnapshot {
    /// Stable VERA snapshot identity, independent of the file name.
    pub product_magic: [u8; 8],
    /// Public snapshot format version, checked before the layout or body.
    pub envelope_version: u32,
    /// [`snapshot_layout`] of the writing build; load rejects any other.
    pub layout: u32,
    /// Player-authored save description used by the load-game list.
    pub description: String,
    /// Hash of the map file — caller verifies on load to ensure same map.
    pub map_hash: u64,
    /// Hash of the merged rules — caller verifies on load to ensure same rules.
    pub rules_hash: u64,
    /// Simulation tick at save time — stored in header for quick preview.
    pub tick: u64,
    /// Unix timestamp (seconds) when the save was created.
    pub save_timestamp: u64,
    /// Map name at save time — stored in header for quick preview.
    pub map_name: String,
    /// Capability of the retained Rules owner, or this build for synthetic saves.
    compatibility: RulesCompatibility,
    /// Decoded world. Its skipped caches and live process state need restoration.
    #[cfg(not(any(test, feature = "test-support")))]
    #[serde(deserialize_with = "deserialize_simulation")]
    sim: Simulation,
    // Core fixtures inspect individual stages of this same wire field.
    #[cfg(any(test, feature = "test-support"))]
    #[serde(deserialize_with = "deserialize_simulation")]
    pub(crate) sim: Simulation,
    /// Presentation-owned Techno+3CA waterline shorts by stable entity ID.
    /// Original AbstractLoad410380 reads the raw object, then FootLoad4DB60D
    /// and no-init Foot4D3540 preserve this short (naval_lifetime_controls).
    /// Kept in the neutral envelope so sim never depends on a render owner.
    pub sinking_waterlines: Vec<(u64, i16)>,
    /// Presentation-owned Anim+FC copies. AnimSave4253B0/AbstractSave410320
    /// retain the signed dword independently of depth+100; no lockstep input.
    pub anim_intensities: Vec<(u64, i32)>,
    /// Retained local sidebar order is presentation state, outside the lockstep
    /// simulation hash. The app validates/hydrates it before committing a load.
    sidebar_order: Option<SavedSidebarOrder>,
    tactical_view: Option<SavedTacticalView>,
    local_view: Option<SavedLocalView>,
    scenario_resources: Option<SavedScenarioResources>,
    continuation: Option<SnapshotContinuation<[ZoneLevelGraph; ZONE_PRECHECK_LEVELS]>>,
}

/// Custom matches resume state outside the native save body. Stock player
/// loads still run LoadContent67E8CD's hierarchy rebuild and native resets.
#[derive(Serialize, Deserialize)]
struct SnapshotContinuation<Hierarchy> {
    zone_levels: Option<Hierarchy>,
    fallback_cell: SavedSharedCellDummy,
    raw_occupation: RawCellOccupation,
    reservation_mask: u128,
    kamikaze_timer: crate::sim::timer::CdTimer,
}

fn deserialize_simulation<'de, D>(deserializer: D) -> Result<Simulation, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let mut simulation = Simulation::deserialize(deserializer)?;
    if !simulation.retains_snapshot_continuation() {
        // gamemd reads the saved ScenarioClass Random at +0x218, then
        // immediately reseeds it. MapGen starts at Seed(0) until the live
        // process cursor, if present, is bound during player-load admission.
        *simulation.scenario_rng = crate::sim::rng::SimRng::new(0);
        *simulation.mapgen_rng = crate::sim::rng::SimRng::new(0);
    }
    Ok(simulation)
}

/// Lightweight header extracted from a save file without deserializing the
/// full `Simulation`. Fields are laid out in the same order as `GameSnapshot`
/// so the save codec can decode them as a prefix.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GameSnapshotHeader {
    pub product_magic: [u8; 8],
    pub envelope_version: u32,
    pub layout: u32,
    pub description: String,
    pub map_hash: u64,
    pub rules_hash: u64,
    pub tick: u64,
    pub save_timestamp: u64,
    pub map_name: String,
    compatibility: RulesCompatibility,
}

/// The metadata prefix used before the capability marker. An incompatible
/// schema remains listable; its absent mode is never used to admit a load.
#[derive(Deserialize)]
struct LegacyGameSnapshotHeader {
    product_magic: [u8; 8],
    envelope_version: u32,
    version: u32,
    description: String,
    map_hash: u64,
    rules_hash: u64,
    tick: u64,
    save_timestamp: u64,
    map_name: String,
}

#[derive(Serialize, Deserialize)]
struct GameSnapshotPreamble {
    product_magic: [u8; 8],
    envelope_version: u32,
    layout: u32,
}

/// Errors that can occur during snapshot deserialization.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("save has no bound scenario resource inputs")]
    MissingScenarioResources,
    #[error("save has no retained local view")]
    MissingLocalView,
    #[error("invalid saved module projectile path")]
    InvalidModuleProjectilePath,
    #[error("snapshot contains an invalid simulation random number cursor")]
    InvalidRandomCursor,
    #[error("snapshot map continuation does not match the saved scenario rules and authority")]
    InvalidContinuation,
    #[error("snapshot product identity {found:?} is not a VERA20k save")]
    ProductMismatch { found: [u8; 8] },
    #[error("snapshot format version {found} is incompatible with this build's version {expected}")]
    EnvelopeVersionMismatch { expected: u32, found: u32 },
    #[error("save layout {found:#010x} does not match this build's {expected:#010x}")]
    LayoutMismatch { expected: u32, found: u32 },
    #[error("snapshot capability {found:?} does not match this build's {expected:?}")]
    CapabilityMismatch {
        expected: RulesCompatibility,
        found: RulesCompatibility,
    },
    #[error("map hash {found:#018x} does not match active map {expected:#018x}")]
    MapMismatch { expected: u64, found: u64 },
    #[error("rules hash {found:#018x} does not match active rules {expected:#018x}")]
    RulesMismatch { expected: u64, found: u64 },
    #[cfg(any(test, feature = "test-support"))]
    #[error("snapshot map name {found:?} does not match active map {expected:?}")]
    MapNameMismatch { expected: String, found: String },
    #[error(
        "snapshot header tick {header_tick} does not match serialized simulation tick {simulation_tick}"
    )]
    TickMetadataMismatch {
        header_tick: u64,
        simulation_tick: u64,
    },
    #[error(
        "snapshot header map name {header_map_name:?} does not match serialized session map name {simulation_map_name:?}"
    )]
    MapNameMetadataMismatch {
        header_map_name: String,
        simulation_map_name: String,
    },
    #[error("deserialization failed: {0}")]
    DeserializeFailed(#[from] save_codec::Error),
}

/// Structural failures found before a deserialized simulation is admitted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SnapshotRestoreError {
    #[error("a Wasm extension module in the save cannot run: {0}")]
    ExtModule(String),
    #[error(transparent)]
    Factory(#[from] crate::sim::production::FactoryRestoreError),
    #[error(transparent)]
    Tiberium(#[from] crate::rules::overlay_types::TiberiumImageError),
    #[error("object {object_id} has invalid attached stats: {reason}")]
    InvalidAttachedStatsState {
        object_id: u64,
        reason: &'static str,
    },
    #[error("invalid retained foundation for object {object_id}")]
    InvalidFoundation { object_id: u64 },
    #[error("invalid saved live Cell Land: {0}")]
    InvalidSavedCellLand(String),
    #[error("invalid UnitDelivery snapshot state: {0}")]
    InvalidUnitDelivery(&'static str),
    #[error("invalid paid two-stage selection: {0}")]
    InvalidPaidSuperWeaponSelection(&'static str),
    #[error("invalid superweapon launch history: {0}")]
    InvalidSuperWeaponLaunchHistory(&'static str),
    #[error("invalid detached Particle {object_id}: {reason}")]
    InvalidDetachedParticleState {
        object_id: u64,
        reason: &'static str,
    },
    #[error("invalid saved VoxelAnimType damage identity")]
    InvalidVoxelTypeDamage,
    #[error("invalid saved VoxelAnim {object_id} type identity")]
    InvalidVoxelAnimType { object_id: u64 },
    #[error("invalid saved TechnoType TechLevel state: {reason}")]
    InvalidTypeTechLevels { reason: String },
    #[error("invalid scalable Bullet runtime: {0}")]
    InvalidScalableProjectileRuntime(&'static str),
    #[error("invalid radiation site order: {0}")]
    InvalidRadiationSiteOrder(&'static str),
    #[error("invalid saved Spotlight {object_id}: {reason}")]
    InvalidSpotlightState {
        object_id: u64,
        reason: &'static str,
    },
    #[error("invalid saved DiskLaser {object_id}: {reason}")]
    InvalidDiskLaserState {
        object_id: u64,
        reason: &'static str,
    },
    #[error("entity {object_id} has invalid Tesla charging state: {reason}")]
    InvalidTeslaChargingState {
        object_id: u64,
        reason: &'static str,
    },
    #[error("entity {object_id} has invalid Unit follower state: {reason}")]
    InvalidUnitFollowerState {
        object_id: u64,
        reason: &'static str,
    },
    #[error("invalid saved planning graph: {reason}")]
    InvalidPlanningReferences { reason: &'static str },
    #[error("non-Building entity {owner_id} retains animation slot {slot}")]
    InvalidBuildingAnimSlotOwner { owner_id: u64, slot: u8 },
    #[error("Building {owner_id} slot {slot} references missing AnimStore object {anim_id}")]
    MissingBuildingSlotAnim {
        owner_id: u64,
        slot: u8,
        anim_id: u64,
    },
    #[error(
        "animation {anim_id} belongs to both Building {first_owner} slot {first_slot} and Building {second_owner} slot {second_slot}"
    )]
    DuplicateBuildingSlotAnim {
        anim_id: u64,
        first_owner: u64,
        first_slot: u8,
        second_owner: u64,
        second_slot: u8,
    },
    #[error("invalid saved Cell list: {reason}")]
    InvalidCellMembership { reason: String },
    #[error("saved current house {owner} is absent from the HouseClass registry")]
    InvalidCurrentHouse {
        owner: crate::sim::intern::InternedId,
    },
    #[error("invalid saved frame continuation: {reason}")]
    InvalidFrameContinuation { reason: &'static str },
    #[error("invalid saved multiplayer session: {reason}")]
    InvalidMultiplayerSession { reason: &'static str },
    #[error("invalid saved control authority: {0}")]
    InvalidControlAuthority(#[from] crate::sim::control_authority::ControlGrantError),
    #[error("invalid saved factory-source Country reference: {reason}")]
    InvalidFactoryCountry { reason: &'static str },
    #[error("entity {object_id} is a live infantryman with the dying mark")]
    InvalidInfantryDyingMark { object_id: u64 },
    #[error("house {owner} has invalid serialized outcome state: {reason}")]
    InvalidHouseOutcomeState {
        owner: crate::sim::intern::InternedId,
        reason: &'static str,
    },
    #[error("house {owner} has invalid produced history: {reason}")]
    InvalidHouseProductionHistory {
        owner: crate::sim::intern::InternedId,
        reason: &'static str,
    },
    #[error("house {owner} has invalid native scenario name state: {reason}")]
    InvalidHouseScenarioName {
        owner: crate::sim::intern::InternedId,
        reason: &'static str,
    },
    #[error("{registry} contains reserved object id 0")]
    ReservedObjectId { registry: &'static str },
    #[error(
        "{registry} registry key {registry_id} does not match the object's serialized id {object_id}"
    )]
    ObjectIdentityMismatch {
        registry: &'static str,
        registry_id: u64,
        object_id: u64,
    },
    #[error("object id {object_id} is registered by both {first_registry} and {second_registry}")]
    DuplicateObjectIdentity {
        object_id: u64,
        first_registry: &'static str,
        second_registry: &'static str,
    },
    #[error("next object id {next_id} is not after the highest restored object id {highest_id}")]
    ObjectIdCounterBehind { next_id: u64, highest_id: u64 },
    #[error("LogicVector contains duplicate object id {object_id}")]
    DuplicateLogicIdentity { object_id: u64 },
    #[error("LogicVector object id {object_id} has no restored registry identity")]
    MissingLogicIdentity { object_id: u64 },
    #[error("DisplayClass object id {object_id} has no restored registry identity")]
    MissingDisplayIdentity { object_id: u64 },
    #[error("saved Terrain {object_id} references unresolved type {type_name}")]
    MissingTerrainType { object_id: u64, type_name: String },
    #[error(
        "this save is damaged and cannot be loaded: its saved {registry} {object_id} field {field} \
         names interned id {id}, which the save's own name table does not hold"
    )]
    UnknownInternedId {
        registry: &'static str,
        object_id: u64,
        field: &'static str,
        id: u32,
    },
    #[error(transparent)]
    UnknownSavedId(#[from] saved_ids::SavedIdError),
    #[error("live {registry} object id {object_id} is absent from LogicVector")]
    MissingRequiredLogicIdentity {
        registry: &'static str,
        object_id: u64,
    },
    #[error("inactive {registry} object id {object_id} remains in LogicVector")]
    InactiveLogicIdentity {
        registry: &'static str,
        object_id: u64,
    },
    #[error("runtime Smudge {object_id} has not completed constructor UnInit")]
    InvalidSmudgeLifetime { object_id: u64 },
    #[error("terminal {registry} object id {object_id} is absent from PendingDeleteList")]
    MissingDeferredDeleteIdentity {
        registry: &'static str,
        object_id: u64,
    },
    #[error("PendingDeleteList {registry} object id {object_id} remains in LogicVector")]
    DeferredDeleteLogicIdentity {
        registry: &'static str,
        object_id: u64,
    },
    #[error("PendingDeleteList {registry} identity {identity} has no restored owner")]
    UnresolvedDeferredDeleteTicket {
        registry: &'static str,
        identity: u64,
    },
    #[error("this save is damaged and cannot be loaded: {reason}")]
    InvalidTeamState { reason: &'static str },
    #[error("restored trigger runtime is inconsistent: {reason}")]
    InvalidTriggerRuntime { reason: String },
    #[error(
        "{source_registry} object {source_id} field {field} references missing {target_registry} object {target_id}"
    )]
    UnresolvedObjectReference {
        source_registry: &'static str,
        source_id: u64,
        field: &'static str,
        target_registry: &'static str,
        target_id: u64,
    },
    #[error(
        "carrier {carrier_id} has {passenger_count} passengers but {size_count} saved Size entries"
    )]
    PassengerSizeCountMismatch {
        carrier_id: u64,
        passenger_count: usize,
        size_count: usize,
    },
    #[error(
        "carrier {carrier_id} saved CargoTotalSize {saved_total}, but its ordered passenger Sizes produce {computed_total}"
    )]
    PassengerSizeTotalMismatch {
        carrier_id: u64,
        saved_total: i32,
        computed_total: i32,
    },
    #[error("snapshot restore requires the {component}")]
    MissingMapAuthorityComponent { component: &'static str },
    #[error(
        "snapshot overlay grid is {overlay_width}x{overlay_height}, but restored terrain is {terrain_width}x{terrain_height}"
    )]
    MapAuthorityDimensionMismatch {
        overlay_width: u16,
        overlay_height: u16,
        terrain_width: u16,
        terrain_height: u16,
    },
    #[error(
        "snapshot overlay grid storage has {found} cells, but its dimensions require {expected}"
    )]
    MapAuthorityCellStorageMismatch { expected: usize, found: usize },
    #[error(
        "snapshot retained wall-neighbor storage has {found} cells, but its dimensions require {expected}"
    )]
    RetainedWallNeighborStorageMismatch { expected: usize, found: usize },
    #[error("snapshot navigation could not be published from retained map authority")]
    NavigationAuthorityPublicationFailed,
    #[error("snapshot custom map continuation is invalid: {0}")]
    InvalidCustomMapContinuation(&'static str),
    #[error("snapshot terrain assets could not be prepared: {reason}")]
    TerrainAssetPreparationFailed { reason: String },
    #[error("snapshot real-cell bridge flags do not match restored CellClass allocation")]
    RealCellBridgeFlagAuthorityMismatch,
    #[error(
        "snapshot dynamic terrain cell ({rx},{ry}) is absent from restored CellClass allocation"
    )]
    DynamicTerrainCellMissing { rx: u16, ry: u16 },
}

/// Derived facts produced by the transactional map-authority restore seam.
#[derive(Debug)]
pub struct SnapshotMapRestoreOutput {
    pub occupied_overlays: Vec<crate::map::overlay::OverlayEntry>,
    pub native_tiberium_stats: crate::sim::ore_growth::NativeTiberiumRebuildStats,
}

/// Retail asset admission on restored live cells, before derived map publication.
/// The callback draws the candidate's Main and nothing else of it.
pub type SnapshotTerrainAssetPreparation<'a> = dyn FnMut(
        &mut crate::map::resolved_terrain::ResolvedTerrainGrid,
        &mut crate::sim::rng::SimRng,
        i32,
    ) -> Result<(), String>
    + 'a;

/// Incoming Cell values and their theater assets at the player-file boundary.
pub enum PlayerSaveCells<'borrow, 'assets> {
    Saved {
        land: crate::map::resolved_terrain::SavedCellLandValues,
        prepare_assets: &'borrow mut SnapshotTerrainAssetPreparation<'assets>,
    },
    /// A synthetic fixture with no saved resource record or live Cell values.
    #[cfg(any(test, feature = "test-support"))]
    FixtureBaseline,
}

/// The app presentation saved beside the simulation, in `GameSnapshotRef` order.
type SavedPresentationRef<'a> = (
    &'a [(u64, i16)],
    &'a [(u64, i32)],
    Option<&'a SavedSidebarOrder>,
    Option<&'a SavedTacticalView>,
    Option<&'a SavedLocalView>,
    Option<&'a SavedScenarioResourcesRef<'a>>,
);

/// Internal borrow-based envelope for serialization (avoids cloning Simulation).
#[derive(Serialize)]
struct GameSnapshotRef<'a> {
    product_magic: [u8; 8],
    envelope_version: u32,
    layout: u32,
    description: String,
    map_hash: u64,
    rules_hash: u64,
    tick: u64,
    save_timestamp: u64,
    map_name: String,
    compatibility: RulesCompatibility,
    sim: &'a Simulation,
    sinking_waterlines: &'a [(u64, i16)],
    anim_intensities: &'a [(u64, i32)],
    sidebar_order: Option<&'a SavedSidebarOrder>,
    tactical_view: Option<&'a SavedTacticalView>,
    local_view: Option<&'a SavedLocalView>,
    scenario_resources: Option<&'a SavedScenarioResourcesRef<'a>>,
    continuation: Option<SnapshotContinuation<&'a [ZoneLevelGraph; ZONE_PRECHECK_LEVELS]>>,
}

impl GameSnapshot {
    fn serialize(
        sim: &Simulation,
        map_hash: u64,
        rules_hash: u64,
        map_name: &str,
        description: &str,
        save_timestamp: u64,
        presentation: SavedPresentationRef<'_>,
    ) -> Result<Vec<u8>, save_codec::Error> {
        // Retail provenance: Save_Game_To_File @ 0x0067CEF0 supplies a distinct
        // outer file identity; Write_Savegame_Metadata_To_Storage @ 0x006812E0
        // writes public Version=1, exact internal version, and Scenario
        // Description. The active list admits only an exact internal-version
        // match at 0x00559ED0..0x0055A04A. VERA keeps its Rust-native bincode
        // body while making the same load-bearing envelope identities explicit.
        let (
            sinking_waterlines,
            anim_intensities,
            sidebar_order,
            tactical_view,
            local_view,
            scenario_resources,
        ) = presentation;
        let snapshot = GameSnapshotRef {
            product_magic: SNAPSHOT_PRODUCT_MAGIC,
            envelope_version: SNAPSHOT_ENVELOPE_VERSION,
            layout: snapshot_layout(),
            description: description.to_string(),
            map_hash,
            rules_hash,
            tick: sim.session.tick(),
            save_timestamp,
            map_name: map_name.to_string(),
            compatibility: scenario_resources
                .map_or_else(RulesCompatibility::default, |resources| {
                    resources.compatibility()
                }),
            sim,
            sinking_waterlines,
            anim_intensities,
            sidebar_order,
            tactical_view,
            local_view,
            scenario_resources,
            continuation: sim
                .retains_snapshot_continuation()
                .then(|| SnapshotContinuation {
                    zone_levels: sim.zone_grid.as_ref().map(|zones| zones.saved_hierarchy()),
                    fallback_cell: sim.effective_shared_cell_dummy().saved_continuation(),
                    raw_occupation: sim.substrate.raw_cell_occupation.saved_fallback_cell(),
                    reservation_mask: sim.substrate.base_reservations.dummy_mask(),
                    kamikaze_timer: sim.kamikaze.saved_continuation_timer(),
                }),
        };
        save_codec::serialize(&snapshot)
    }

    /// Serialize a headless save with metadata owned by the active session.
    ///
    /// Map/rules digests come from the app's loaded content owners. Tick and map
    /// name are copied from `ScenarioSession`, so the envelope cannot disagree
    /// with the authoritative body. The app supplies the wall-clock timestamp;
    /// sim code remains clock-independent.
    #[cfg(any(test, feature = "test-support"))]
    pub fn save_validated(
        sim: &Simulation,
        map_hash: u64,
        rules_hash: u64,
        description: &str,
        save_timestamp: u64,
    ) -> Vec<u8> {
        Self::save_validated_with_presentation(
            sim,
            map_hash,
            rules_hash,
            description,
            save_timestamp,
            &[],
            &[],
            None,
            None,
            None,
            None,
        )
        .expect("serialize snapshot fixture")
    }

    /// Save the app's retained presentation alongside the simulation. A headless
    /// caller uses `save_validated`, whose presentation supplement is empty.
    pub fn save_validated_with_presentation(
        sim: &Simulation,
        map_hash: u64,
        rules_hash: u64,
        description: &str,
        save_timestamp: u64,
        sinking_waterlines: &[(u64, i16)],
        anim_intensities: &[(u64, i32)],
        sidebar_order: Option<&SavedSidebarOrder>,
        tactical_view: Option<&SavedTacticalView>,
        local_view: Option<&SavedLocalView>,
        scenario_resources: Option<&SavedScenarioResourcesRef<'_>>,
    ) -> Result<Vec<u8>, save_codec::Error> {
        Self::serialize(
            sim,
            map_hash,
            rules_hash,
            sim.session.map_name(),
            description,
            save_timestamp,
            (
                sinking_waterlines,
                anim_intensities,
                sidebar_order,
                tactical_view,
                local_view,
                scenario_resources,
            ),
        )
    }

    /// Scenario metadata needed to bind the incoming theater and terrain.
    pub fn session(&self) -> &crate::sim::scenario_session::ScenarioSession {
        self.sim.session()
    }

    /// Whether an Options speed input was still queued when the player file was written.
    pub fn has_pending_game_speed_input(&self) -> bool {
        self.sim.pending_command_snapshot().iter().any(|command| {
            matches!(
                command.payload,
                crate::sim::command::Command::SetGameSpeed { .. }
            )
        })
    }

    /// Inspect an internal checkpoint or an intermediate player-load stage in a fixture.
    #[cfg(any(test, feature = "test-support"))]
    pub fn into_simulation_for_test(self) -> Simulation {
        self.sim
    }

    /// Restore the reference graph of a fixture without applying the player-file resets.
    #[cfg(any(test, feature = "test-support"))]
    pub fn restore_references_for_test(self) -> Result<Simulation, SnapshotRestoreError> {
        let mut simulation = self.sim;
        simulation.restore_after_snapshot_load()?;
        Ok(simulation)
    }

    /// Restore an internal checkpoint whose external terrain is supplied by a client fixture.
    #[cfg(any(test, feature = "test-support"))]
    pub fn restore_checkpoint_map_for_test(
        self,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
        terrain: crate::map::resolved_terrain::ResolvedTerrainGrid,
        speed: crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig,
    ) -> Result<Simulation, SnapshotRestoreError> {
        let mut simulation = self.restore_references_for_test()?;
        simulation.rebuild_caches_after_load(terrain, speed, rules);
        simulation.restore_map_authority_after_snapshot_load(rules, overlay_registry, None)?;
        Ok(simulation)
    }

    /// Prepare a detached player world for the caller's transaction. All fallible
    /// world restoration finishes before the candidate can be published. An
    /// internal checkpoint or replay keeps its own simulation continuation.
    pub fn restore_player_world(
        self,
        process: crate::sim::world::SnapshotProcessState<'_>,
        rules: &crate::rules::ruleset::RuleSet,
        terrain_template: crate::map::resolved_terrain::ResolvedTerrainGrid,
        terrain_speed_config: crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig,
        cells: PlayerSaveCells<'_, '_>,
    ) -> Result<(Simulation, SnapshotMapRestoreOutput), SnapshotRestoreError> {
        let overlay_registry = rules.overlay_registry();
        let mut simulation = self.sim;
        simulation.validate_saved_interned_ids()?;
        // The save's other parts name ids of the same interner.
        let interned = simulation.interner.len();
        saved_ids::check_saved_interned_ids(&self.sidebar_order, interned)?;
        saved_ids::check_saved_interned_ids(&self.continuation, interned)?;
        simulation.retain_process_state_for_load(process);
        let retain_continuation = simulation.retains_snapshot_continuation();
        if retain_continuation != self.continuation.is_some() {
            return Err(SnapshotRestoreError::InvalidCustomMapContinuation(
                "presence differs from the saved scenario rules and authority",
            ));
        }
        // Native MouseLoad->Resize reconstructs the dummy before subsequent
        // LoadContent67E8CD hierarchy reads. The fallible candidate must see
        // that state without touching the running match's retained identity.
        simulation.bind_shared_cell_dummy(crate::map::resolved_terrain::SharedCellDummy::fresh());
        if !retain_continuation {
            simulation.reconstruct_cellclass_dummy_for_map_resize();
            // InfantryLoad521A0C -> TechnoNoInit6F4335 resets the transition clock.
            simulation.restart_cloak_progress_after_player_load();
            simulation.restore_object_timers_after_player_load();
        }
        simulation.validate_ares_state_for_load(rules.ares())?;
        simulation.restore_after_snapshot_load()?;
        crate::sim::production::validate_restored_factory_state(&simulation, rules)?;
        simulation.rebuild_caches_after_load(terrain_template, terrain_speed_config, rules);

        let map_restore = match cells {
            PlayerSaveCells::Saved {
                land,
                prepare_assets,
            } => simulation.restore_map_authority_after_user_load(
                rules,
                overlay_registry,
                Some(prepare_assets),
                land,
            )?,
            #[cfg(any(test, feature = "test-support"))]
            PlayerSaveCells::FixtureBaseline => simulation
                .restore_map_authority_after_snapshot_load(rules, overlay_registry, None)?,
        };
        // Older headless saves may omit types that had never spawned. Add
        // those canonical names without changing any saved ID, then build
        // the handle table; a missing name must never become sidebar ID zero.
        simulation.intern_rule_type_ids(rules);
        simulation.resolve_type_handles(rules);
        simulation.rebuild_lighting_sources_after_load(rules);
        // Post_Load_Game6851AC/6851B1 reselects lighting, then refreshes the
        // flagged Anim conversions from the incoming stored Locations.
        if !retain_continuation {
            simulation.reinitialize_lighting_after_user_load();
            simulation.refresh_animated_tiberium_after_load(rules, overlay_registry)?;
        }
        if let Some(continuation) = self.continuation {
            continuation.restore(&mut simulation)?;
        }

        Ok((simulation, map_restore))
    }

    pub fn sidebar_order(&self) -> Option<&SavedSidebarOrder> {
        self.sidebar_order.as_ref()
    }

    pub fn tactical_view(&self) -> Option<&SavedTacticalView> {
        self.tactical_view.as_ref()
    }

    pub fn take_local_view(&mut self) -> Option<SavedLocalView> {
        self.local_view.take()
    }

    pub fn take_scenario_resources(&mut self) -> Option<SavedScenarioResources> {
        self.scenario_resources.take()
    }

    /// Test-only constructor for deliberately synthetic envelope metadata.
    #[cfg(any(test, feature = "test-support"))]
    pub fn save(
        sim: &Simulation,
        map_hash: u64,
        rules_hash: u64,
        map_name: &str,
        save_timestamp: u64,
    ) -> Vec<u8> {
        Self::serialize(
            sim,
            map_hash,
            rules_hash,
            map_name,
            map_name,
            save_timestamp,
            (&[], &[], None, None, None, None),
        )
        .expect("serialize snapshot fixture")
    }

    /// Deserialize an internal current-layout checkpoint without content validation.
    ///
    /// This exists for internal tests and diagnostics. Production restoration
    /// uses [`Self::load_with_resources`] and validates the incoming resources.
    /// Keep planning state here. Only admitted stock player loads clear it.
    /// The stock Scenario RNG follows retail's post-read `Random__Seed(0)`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn load_unchecked(bytes: &[u8]) -> Result<GameSnapshot, SnapshotError> {
        let _ = Self::read_header(bytes)?;
        Ok(save_codec::deserialize(bytes)?)
    }

    /// Compatibility alias for existing unit fixtures that intentionally use
    /// synthetic zero hashes and header-only map names.
    #[cfg(any(test, feature = "test-support"))]
    pub fn load(bytes: &[u8]) -> Result<GameSnapshot, SnapshotError> {
        Self::load_unchecked(bytes)
    }

    /// Deserialize only when the save belongs to the exact active content.
    ///
    /// Layout and compatibility metadata are rejected before the simulation
    /// body is admitted. The duplicated preview metadata must also agree with
    /// `ScenarioSession`; no warning/continue or zero-hash sentinel exists.
    /// A stock snapshot returns seed-zero RNG placeholders and
    /// [`Self::restore_player_world`] supplies its process cursors. A custom
    /// session retains its Scenario and MapGen continuations.
    #[cfg(any(test, feature = "test-support"))]
    pub fn load_validated(
        bytes: &[u8],
        expected_map_hash: u64,
        expected_rules_hash: u64,
        expected_map_name: &str,
    ) -> Result<GameSnapshot, SnapshotError> {
        let header = Self::read_header(bytes)?;
        if header.map_hash != expected_map_hash {
            return Err(SnapshotError::MapMismatch {
                expected: expected_map_hash,
                found: header.map_hash,
            });
        }
        if header.rules_hash != expected_rules_hash {
            return Err(SnapshotError::RulesMismatch {
                expected: expected_rules_hash,
                found: header.rules_hash,
            });
        }
        if header.map_name != expected_map_name {
            return Err(SnapshotError::MapNameMismatch {
                expected: expected_map_name.to_string(),
                found: header.map_name,
            });
        }

        Self::deserialize_admitted(bytes)
    }

    /// Production cold/cross-map admission. Compatibility is checked against
    /// the saved resource owner and its rebuilt RuleSet, never the outgoing
    /// match or header values passed back as their own expected content.
    pub fn load_with_resources(bytes: &[u8]) -> Result<GameSnapshot, SnapshotError> {
        Self::read_header(bytes)?;
        let snapshot = Self::deserialize_admitted(bytes)?;
        snapshot.require_player_resources()?;
        Ok(snapshot)
    }

    fn require_player_resources(&self) -> Result<&SavedScenarioResources, SnapshotError> {
        let resources = self
            .scenario_resources
            .as_ref()
            .ok_or(SnapshotError::MissingScenarioResources)?;
        if resources.map().map_hash() != self.map_hash {
            return Err(SnapshotError::MapMismatch {
                expected: resources.map().map_hash(),
                found: self.map_hash,
            });
        }
        if self.local_view.is_none() {
            return Err(SnapshotError::MissingLocalView);
        }
        Ok(resources)
    }

    fn deserialize_admitted(bytes: &[u8]) -> Result<GameSnapshot, SnapshotError> {
        let mut snapshot: GameSnapshot = save_codec::deserialize(bytes)?;
        Self::validate_admitted(&mut snapshot)?;
        Ok(snapshot)
    }

    /// The checks a decoded snapshot passes before anything reads it, in
    /// place: a snapshot is large, and a load passes it on by value once.
    fn validate_admitted(snapshot: &mut GameSnapshot) -> Result<(), SnapshotError> {
        // Layout tracing deserializes synthetic fields. Validate the actual path
        // only when admitting a current or converted player save.
        if !snapshot.sim.ext_modules.valid_saved_projectile_paths() {
            return Err(SnapshotError::InvalidModuleProjectilePath);
        }
        if snapshot.tick != snapshot.sim.session.tick() {
            return Err(SnapshotError::TickMetadataMismatch {
                header_tick: snapshot.tick,
                simulation_tick: snapshot.sim.session.tick(),
            });
        }
        if snapshot.map_name != snapshot.sim.session.map_name() {
            return Err(SnapshotError::MapNameMetadataMismatch {
                header_map_name: snapshot.map_name.clone(),
                simulation_map_name: snapshot.sim.session.map_name().to_owned(),
            });
        }
        // Native content Load clears PlanMgr, then TechnoLoad zeros +514.
        // Stock player loads clear future paths. Internal checkpoints and
        // custom-authority player snapshots retain them.
        let retain_continuation = snapshot.sim.retains_snapshot_continuation();
        if retain_continuation != snapshot.continuation.is_some() {
            return Err(SnapshotError::InvalidContinuation);
        }
        if retain_continuation {
            if !snapshot.sim.scenario_rng.valid_checkpoint()
                || !snapshot.sim.mapgen_rng.valid_checkpoint()
            {
                return Err(SnapshotError::InvalidRandomCursor);
            }
        } else {
            snapshot.sim.reset_planning_after_player_load();
        }
        Ok(())
    }

    /// Admit a current-layout header without deserializing Simulation. The
    /// fixed preamble rejects other layouts before any variable-length header
    /// fields; browser metadata has a separate compatibility policy.
    pub fn read_header(bytes: &[u8]) -> Result<GameSnapshotHeader, SnapshotError> {
        let preamble = Self::read_preamble(bytes)?;
        Self::validate_version(preamble.envelope_version)?;
        if preamble.layout != snapshot_layout() {
            return Err(SnapshotError::LayoutMismatch {
                expected: snapshot_layout(),
                found: preamble.layout,
            });
        }
        Self::read_compatible_header(bytes)
    }

    fn read_compatible_header(bytes: &[u8]) -> Result<GameSnapshotHeader, SnapshotError> {
        let header: GameSnapshotHeader = save_codec::deserialize(bytes)?;
        if !header.compatibility.matches_build() {
            return Err(SnapshotError::CapabilityMismatch {
                expected: RulesCompatibility::default(),
                found: header.compatibility,
            });
        }
        Ok(header)
    }

    /// SaveInfo681840/559ED0 reads metadata even when its internal version is
    /// incompatible. Browser5596A0 sorts those rows before5599DF hides them;
    /// full load still requires the strict current-layout header above.
    pub fn read_metadata(bytes: &[u8]) -> Result<GameSnapshotHeader, SnapshotError> {
        let preamble = Self::read_preamble(bytes)?;
        // Version 1's metadata prefix stays readable for listing and deleting
        // incompatible saves. Its world body is never admitted.
        if preamble.envelope_version != 1 {
            Self::validate_version(preamble.envelope_version)?;
        }
        // Counter saves before 459 carry no capability marker. A layout id
        // always has its top bit set, so it is never below 459.
        if preamble.envelope_version == 1 && preamble.layout < 459 {
            let header: LegacyGameSnapshotHeader = save_codec::deserialize(bytes)?;
            return Ok(GameSnapshotHeader {
                product_magic: header.product_magic,
                envelope_version: header.envelope_version,
                layout: header.version,
                description: header.description,
                map_hash: header.map_hash,
                rules_hash: header.rules_hash,
                tick: header.tick,
                save_timestamp: header.save_timestamp,
                map_name: header.map_name,
                // The schema mismatch always rejects this row. The old format
                // did not record a capability and cannot certify either mode.
                compatibility: RulesCompatibility::Retail,
            });
        }
        Ok(save_codec::deserialize(bytes)?)
    }

    /// Read the product and fixed version/layout words before variable data.
    fn read_preamble(bytes: &[u8]) -> Result<GameSnapshotPreamble, SnapshotError> {
        let preamble: GameSnapshotPreamble = save_codec::deserialize(bytes)?;
        if preamble.product_magic != SNAPSHOT_PRODUCT_MAGIC {
            return Err(SnapshotError::ProductMismatch {
                found: preamble.product_magic,
            });
        }
        Ok(preamble)
    }

    fn validate_version(found: u32) -> Result<(), SnapshotError> {
        if found != SNAPSHOT_ENVELOPE_VERSION {
            return Err(SnapshotError::EnvelopeVersionMismatch {
                expected: SNAPSHOT_ENVELOPE_VERSION,
                found,
            });
        }
        Ok(())
    }
}

impl SnapshotContinuation<[ZoneLevelGraph; ZONE_PRECHECK_LEVELS]> {
    fn restore(self, sim: &mut Simulation) -> Result<(), SnapshotRestoreError> {
        use SnapshotRestoreError::InvalidCustomMapContinuation;

        match (sim.zone_grid.as_mut(), self.zone_levels) {
            (Some(zones), Some(levels)) => zones
                .restore_saved_hierarchy(levels)
                .map_err(InvalidCustomMapContinuation)?,
            (None, None) => {}
            _ => {
                return Err(InvalidCustomMapContinuation(
                    "zone history is missing or unexpected",
                ));
            }
        }
        let terrain = sim
            .resolved_terrain
            .as_ref()
            .ok_or(InvalidCustomMapContinuation("bound map terrain is missing"))?;
        // Map rebinding runs native lookups. Publish the saved fallback
        // values last so a read cannot overwrite its retained coordinate.
        sim.effective_shared_cell_dummy()
            .restore_saved_continuation(self.fallback_cell, terrain)
            .map_err(InvalidCustomMapContinuation)?;
        sim.substrate
            .raw_cell_occupation
            .restore_saved_fallback_cell(self.raw_occupation, &sim.houses)
            .map_err(InvalidCustomMapContinuation)?;
        sim.substrate
            .base_reservations
            .restore_saved_dummy_mask(self.reservation_mask);
        sim.kamikaze.restore_continuation_timer(self.kamikaze_timer);
        Ok(())
    }
}

impl GameSnapshotHeader {
    pub fn compatibility(&self) -> RulesCompatibility {
        self.compatibility
    }

    pub fn is_compatible(&self) -> bool {
        self.envelope_version == SNAPSHOT_ENVELOPE_VERSION
            && self.layout == snapshot_layout()
            && self.compatibility.matches_build()
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod tests;
