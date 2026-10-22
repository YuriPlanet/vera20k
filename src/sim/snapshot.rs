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

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

#[path = "snapshot/saved_local_view.rs"]
pub mod saved_local_view;
#[path = "snapshot/scenario_resources.rs"]
pub mod scenario_resources;

use saved_local_view::SavedLocalView;
use scenario_resources::{SavedScenarioResources, SavedScenarioResourcesRef};

use crate::rules::compatibility::RulesCompatibility;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;
use crate::util::save_codec;

/// The layout this build writes and accepts: a digest of the
/// [`save_codec::layout`] description of [`GameSnapshot`], traced once per
/// process. It changes whenever a saved type, field or enum variant does, so
/// nothing bumps it by hand and two branches cannot share one id for different
/// layouts. A change in what an unchanged field means renames that field. The
/// top bit is always set; the hand-kept counter used before stayed far below
/// it, so those saves read as another layout.
fn snapshot_layout() -> u32 {
    static LAYOUT: OnceLock<u32> = OnceLock::new();
    *LAYOUT.get_or_init(|| {
        let description = save_codec::layout::describe::<GameSnapshot>()
            .unwrap_or_else(|error| panic!("cannot describe the snapshot layout: {error}"));
        let digest = crate::util::sha256::sha256_bytes(description.as_bytes());
        u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]]) | 0x8000_0000
    })
}

const SNAPSHOT_PRODUCT_MAGIC: [u8; 8] = *b"VERA20K\0";
const SNAPSHOT_ENVELOPE_VERSION: u32 = 1;

/// Neutral type identity shared by the retained sidebar and its save envelope.
/// The kind matters: a SuperWeapon and an object never share a cameo entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SavedCameoId {
    Object(InternedId),
    SuperWeapon(InternedId),
}

/// Identity/order and per-strip scroll rows survive save/load; comparison keys are projections of
/// the restored simulation, canonical rules and current process string table.
/// Strip indices are building, defense, infantry and vehicle, respectively.
///
/// Mouse Save5BE6D0 writes the whole 0x556C object at5BE715..5BE71C, including
/// four strips at +1544, stride F94. Load5BDF70 reads it at5BE089..5BE092;
/// no-init5BE9B0->6A4F20 resets progress6AC7C0, preserving count/type/order.
/// This neutral transport does not introduce a sim dependency on a UI owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedSidebarOrder {
    owner: InternedId,
    strips: [Vec<SavedCameoId>; 4],
    scroll_rows: [usize; 4],
    flash_deadlines: [Vec<i32>; 4],
    redraw_until: i32,
    request_states: [Vec<u8>; 4],
    factory_links: [Vec<Option<crate::sim::production::ProductionFactoryVisit>>; 4],
    prediction_categories: Vec<crate::sim::production::ProductionCategory>,
}

impl SavedSidebarOrder {
    pub fn new(owner: InternedId, strips: [Vec<SavedCameoId>; 4], scroll_rows: [usize; 4]) -> Self {
        let flash_deadlines = std::array::from_fn(|index| vec![0; strips[index].len()]);
        let request_states = std::array::from_fn(|index| vec![0; strips[index].len()]);
        let factory_links = std::array::from_fn(|index| vec![None; strips[index].len()]);
        Self {
            owner,
            strips,
            scroll_rows,
            flash_deadlines,
            redraw_until: 0,
            request_states,
            factory_links,
            prediction_categories: Vec::new(),
        }
    }

    pub fn with_flash(mut self, deadlines: [Vec<i32>; 4], redraw_until: i32) -> Self {
        self.flash_deadlines = deadlines;
        self.redraw_until = redraw_until;
        self
    }

    pub fn flash(&self) -> (&[Vec<i32>; 4], i32) {
        (&self.flash_deadlines, self.redraw_until)
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

    #[cfg(any(test, feature = "test-support"))]
    pub fn owner(&self) -> InternedId {
        self.owner
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn strips(&self) -> &[Vec<SavedCameoId>; 4] {
        &self.strips
    }

    pub fn into_parts(self) -> (InternedId, [Vec<SavedCameoId>; 4], [usize; 4]) {
        (self.owner, self.strips, self.scroll_rows)
    }
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
    /// Public envelope contract. This changes only when the outer prefix does.
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
    /// The full authoritative simulation state (caches excluded via serde skip).
    pub sim: Simulation,
    /// Presentation-owned Techno+3CA waterline shorts by stable entity ID.
    /// Original AbstractLoad410380 reads the raw object, then FootLoad4DB60D
    /// and no-init Foot4D3540 preserve this short (naval_lifetime_controls).
    /// Kept in the neutral envelope so sim never depends on a render owner.
    pub sinking_waterlines: Vec<(u64, i16)>,
    /// Retained local sidebar order is presentation state, outside the lockstep
    /// simulation hash. The app validates/hydrates it before committing a load.
    sidebar_order: Option<SavedSidebarOrder>,
    tactical_view: Option<SavedTacticalView>,
    local_view: Option<SavedLocalView>,
    scenario_resources: Option<SavedScenarioResources>,
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
    #[error("snapshot product identity {found:?} is not a VERA20k save")]
    ProductMismatch { found: [u8; 8] },
    #[error("snapshot envelope version {found} does not match expected {expected}")]
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
    #[cfg(feature = "ares")]
    #[error("object {object_id} has invalid attached speed: {reason}")]
    InvalidAttachedSpeedState {
        object_id: u64,
        reason: &'static str,
    },
    #[error("invalid UnitDelivery snapshot state: {0}")]
    InvalidUnitDelivery(&'static str),
    #[error("invalid paid two-stage selection: {0}")]
    InvalidPaidSuperWeaponSelection(&'static str),
    #[cfg(feature = "ares")]
    #[error("invalid superweapon launch history: {0}")]
    InvalidSuperWeaponLaunchHistory(&'static str),
    #[error("invalid detached Particle {object_id}: {reason}")]
    InvalidDetachedParticleState {
        object_id: u64,
        reason: &'static str,
    },
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
    #[cfg(feature = "ares")]
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
/// The callback can advance Main without obtaining seed/replacement authority.
pub type SnapshotTerrainAssetPreparation<'a> = dyn FnMut(
        &mut crate::map::resolved_terrain::ResolvedTerrainGrid,
        crate::sim::rng::MainRngDraws,
        i32,
    ) -> Result<(), String>
    + 'a;

/// The app presentation saved beside the simulation, in `GameSnapshotRef` order.
type SavedPresentationRef<'a> = (
    &'a [(u64, i16)],
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
    sidebar_order: Option<&'a SavedSidebarOrder>,
    tactical_view: Option<&'a SavedTacticalView>,
    local_view: Option<&'a SavedLocalView>,
    scenario_resources: Option<&'a SavedScenarioResourcesRef<'a>>,
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
    ) -> Vec<u8> {
        // Retail provenance: Save_Game_To_File @ 0x0067CEF0 supplies a distinct
        // outer file identity; Write_Savegame_Metadata_To_Storage @ 0x006812E0
        // writes public Version=1, exact internal version, and Scenario
        // Description. The active list admits only an exact internal-version
        // match at 0x00559ED0..0x0055A04A. VERA keeps its Rust-native bincode
        // body while making the same load-bearing envelope identities explicit.
        let (sinking_waterlines, sidebar_order, tactical_view, local_view, scenario_resources) =
            presentation;
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
            sidebar_order,
            tactical_view,
            local_view,
            scenario_resources,
        };
        save_codec::serialize(&snapshot).expect("snapshot serialization should not fail")
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
            None,
            None,
            None,
            None,
        )
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
        sidebar_order: Option<&SavedSidebarOrder>,
        tactical_view: Option<&SavedTacticalView>,
        local_view: Option<&SavedLocalView>,
        scenario_resources: Option<&SavedScenarioResourcesRef<'_>>,
    ) -> Vec<u8> {
        Self::serialize(
            sim,
            map_hash,
            rules_hash,
            sim.session.map_name(),
            description,
            save_timestamp,
            (
                sinking_waterlines,
                sidebar_order,
                tactical_view,
                local_view,
                scenario_resources,
            ),
        )
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
            (&[], None, None, None, None),
        )
    }

    /// Deserialize a current-layout snapshot without content validation.
    ///
    /// This exists for internal tests and diagnostics. Production restoration
    /// uses [`Self::load_with_resources`] and validates the incoming resources.
    /// Like retail's post-read Scenario
    /// reinitializer, full deserialization resets the embedded Scenario RNG to
    /// `Random__Seed(0)` even though its saved bytes remain in the wire layout.
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
    /// The returned Scenario RNG is the canonical seed-zero state. Main/MapGen
    /// fields are deserialize placeholders; the app's in-scenario production
    /// load seam replaces them with the live process cursors and seed.
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
        let resources = snapshot
            .scenario_resources
            .as_ref()
            .ok_or(SnapshotError::MissingScenarioResources)?;
        if resources.map().map_hash() != snapshot.map_hash {
            return Err(SnapshotError::MapMismatch {
                expected: resources.map().map_hash(),
                found: snapshot.map_hash,
            });
        }
        if snapshot.local_view.is_none() {
            return Err(SnapshotError::MissingLocalView);
        }
        Ok(snapshot)
    }

    fn deserialize_admitted(bytes: &[u8]) -> Result<GameSnapshot, SnapshotError> {
        let snapshot: GameSnapshot = save_codec::deserialize(bytes)?;
        if snapshot.tick != snapshot.sim.session.tick() {
            return Err(SnapshotError::TickMetadataMismatch {
                header_tick: snapshot.tick,
                simulation_tick: snapshot.sim.session.tick(),
            });
        }
        if snapshot.map_name != snapshot.sim.session.map_name() {
            return Err(SnapshotError::MapNameMetadataMismatch {
                header_map_name: snapshot.map_name,
                simulation_map_name: snapshot.sim.session.map_name().to_owned(),
            });
        }
        Ok(snapshot)
    }

    /// Admit a current-layout header without deserializing Simulation. The
    /// fixed preamble rejects other layouts before any variable-length header
    /// fields; browser metadata has a separate compatibility policy.
    pub fn read_header(bytes: &[u8]) -> Result<GameSnapshotHeader, SnapshotError> {
        let preamble = Self::read_preamble(bytes)?;
        if preamble.layout != snapshot_layout() {
            return Err(SnapshotError::LayoutMismatch {
                expected: snapshot_layout(),
                found: preamble.layout,
            });
        }
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
        // Counter saves before 459 carry no capability marker. A layout id
        // always has its top bit set, so it is never below 459.
        if preamble.layout < 459 {
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

    /// Product and public-envelope admission shared by strict loading and the
    /// browser's readable-but-incompatible rows.
    fn read_preamble(bytes: &[u8]) -> Result<GameSnapshotPreamble, SnapshotError> {
        let preamble: GameSnapshotPreamble = save_codec::deserialize(bytes)?;
        if preamble.product_magic != SNAPSHOT_PRODUCT_MAGIC {
            return Err(SnapshotError::ProductMismatch {
                found: preamble.product_magic,
            });
        }
        if preamble.envelope_version != SNAPSHOT_ENVELOPE_VERSION {
            return Err(SnapshotError::EnvelopeVersionMismatch {
                expected: SNAPSHOT_ENVELOPE_VERSION,
                found: preamble.envelope_version,
            });
        }
        Ok(preamble)
    }
}

impl GameSnapshotHeader {
    pub fn compatibility(&self) -> RulesCompatibility {
        self.compatibility
    }

    pub fn is_compatible(&self) -> bool {
        self.layout == snapshot_layout() && self.compatibility.matches_build()
    }
}

#[derive(Debug, Clone, Copy)]
struct RestoredObjectIndex {
    highest_id: u64,
}

impl RestoredObjectIndex {
    fn build(
        sim: &Simulation,
    ) -> Result<(Self, BTreeMap<u64, &'static str>), SnapshotRestoreError> {
        use crate::sim::world::ObjectKind;
        let mut identities = BTreeMap::new();
        let registry = &mut identities;
        for kind in ObjectKind::ALL {
            match kind {
                ObjectKind::Entity => {
                    for (id, entity) in sim.substrate.entities.iter_sorted() {
                        Self::register(registry, "EntityStore", id, entity.stable_id())?;
                    }
                }
                ObjectKind::Anim => {
                    for (&id, anim) in sim.substrate.anims.iter() {
                        Self::register(registry, "AnimStore", id, anim.stable_id())?;
                    }
                }
                ObjectKind::VoxelAnim => {
                    for (&id, debris) in sim.substrate.voxel_anims.iter() {
                        Self::register(registry, "VoxelAnimStore", id, debris.stable_id())?;
                    }
                }
                ObjectKind::Spotlight => {
                    for (&id, light) in sim.substrate.spotlights.iter() {
                        Self::register(registry, "SpotlightStore", id, light.stable_id())?;
                    }
                }
                ObjectKind::ParticleSystem => {
                    for (&id, system) in sim.substrate.particle_systems.iter() {
                        Self::register(registry, "ParticleSystemStore", id, system.stable_id())?;
                    }
                }
                // Individual particles retain AIR Display membership.
                ObjectKind::Particle => {
                    for (_, system) in sim.substrate.particle_systems.iter() {
                        for particle in system.particles() {
                            let id = particle.stable_id();
                            Self::register(registry, "ParticleSystemStore.children", id, id)?;
                        }
                    }
                    for (id, particle, _, _) in sim.substrate.particle_systems.detached_particles()
                    {
                        Self::register(
                            registry,
                            "ParticleSystemStore.detached",
                            id,
                            particle.stable_id(),
                        )?;
                    }
                }
                ObjectKind::Terrain => {
                    for (&id, terrain) in sim.terrain_objects.objects() {
                        Self::register(registry, "TerrainObjectStore", id, terrain.stable_id())?;
                    }
                }
                ObjectKind::Projectile => {
                    for (&id, projectile) in sim.projectiles.iter() {
                        Self::register(registry, "ProjectileStore", id, projectile.id())?;
                    }
                }
                ObjectKind::Wave => {
                    for (&id, wave) in sim.waves.iter() {
                        Self::register(registry, "WaveStore", id, wave.id())?;
                    }
                }
                ObjectKind::Smudge => {
                    for (id, object) in sim.smudge_grid.iter().flat_map(|grid| grid.objects()) {
                        Self::register(registry, "SmudgeGrid", id, object.stable_id())?;
                    }
                }
            }
        }
        // The other registries numbered from the same allocator.
        for shot in sim.disk_laser_observations() {
            Self::register(registry, "DiskLaserStore", shot.id, shot.id)?;
        }
        for id in sim.load_objects.retained_overlay_ids() {
            Self::register(registry, "LoadObjectLifecycle", id, id)?;
        }

        let highest_id = identities.last_key_value().map_or(0, |(&id, _)| id);
        Ok((Self { highest_id }, identities))
    }

    fn register(
        identities: &mut BTreeMap<u64, &'static str>,
        registry: &'static str,
        registry_id: u64,
        object_id: u64,
    ) -> Result<(), SnapshotRestoreError> {
        if registry_id == 0 {
            return Err(SnapshotRestoreError::ReservedObjectId { registry });
        }
        if registry_id != object_id {
            return Err(SnapshotRestoreError::ObjectIdentityMismatch {
                registry,
                registry_id,
                object_id,
            });
        }
        if let Some(first_registry) = identities.insert(registry_id, registry) {
            return Err(SnapshotRestoreError::DuplicateObjectIdentity {
                object_id: registry_id,
                first_registry,
                second_registry: registry,
            });
        }
        Ok(())
    }
}

fn require_resolved_reference(
    resolves: bool,
    source_registry: &'static str,
    source_id: u64,
    field: &'static str,
    target_registry: &'static str,
    target_id: u64,
) -> Result<(), SnapshotRestoreError> {
    if resolves {
        Ok(())
    } else {
        Err(SnapshotRestoreError::UnresolvedObjectReference {
            source_registry,
            source_id,
            field,
            target_registry,
            target_id,
        })
    }
}

/// The one rule for the stores an Object pointer may name, whether TarCom,
/// NavCom or another target or owner slot: a Techno or a Terrain, the two
/// stores whose objects the coordinate owners (`target_get_coords`,
/// `nav_target_coordinate`) resolve.
fn require_object_reference(
    entity_ids: &BTreeSet<u64>,
    terrain_objects: &BTreeMap<u64, crate::sim::terrain_object::TerrainObjectState>,
    source_registry: &'static str,
    source_id: u64,
    field: &'static str,
    target_id: u64,
) -> Result<(), SnapshotRestoreError> {
    require_resolved_reference(
        entity_ids.contains(&target_id) || terrain_objects.contains_key(&target_id),
        source_registry,
        source_id,
        field,
        "EntityStore or TerrainObjectStore",
        target_id,
    )
}

fn validate_passenger_size_tables(sim: &Simulation) -> Result<(), SnapshotRestoreError> {
    for (carrier_id, entity) in sim.substrate.entities.iter_sorted() {
        let Some(cargo) = entity.passenger_role.cargo() else {
            continue;
        };
        if cargo.passengers().len() != cargo.passenger_sizes().len() {
            return Err(SnapshotRestoreError::PassengerSizeCountMismatch {
                carrier_id,
                passenger_count: cargo.passengers().len(),
                size_count: cargo.passenger_sizes().len(),
            });
        }
        let computed_total = cargo.recomputed_total_size();
        if computed_total != cargo.total_size() {
            return Err(SnapshotRestoreError::PassengerSizeTotalMismatch {
                carrier_id,
                saved_total: cargo.total_size(),
                computed_total,
            });
        }
    }
    Ok(())
}

fn restore_object_references(
    sim: &mut Simulation,
    identities: &BTreeMap<u64, &'static str>,
) -> Result<(), SnapshotRestoreError> {
    use crate::sim::combat::TargetKind;
    use crate::sim::docking::bunker_link::BunkerLink;
    use crate::sim::projectile::ProjectileTarget;

    let entity_ids: BTreeSet<u64> = sim.substrate.entities.keys_sorted().into_iter().collect();
    let anim_ids: BTreeSet<u64> = sim.substrate.anims.iter().map(|(&id, _)| id).collect();
    let particle_system_ids: BTreeSet<u64> = sim
        .substrate
        .particle_systems
        .iter()
        .map(|(&id, _)| id)
        .collect();

    if let Some(id) = sim.particle_systems().gas_cloud_system() {
        require_resolved_reference(
            particle_system_ids.contains(&id),
            "ParticleSystemStore",
            0,
            "gas_cloud_system",
            "ParticleSystemStore",
            id,
        )?;
    }

    // Building saved+55C slots swizzle to distinct owned Anim objects. Validate
    // before weak-pointer cleanup or reverse-index mutation. Anim+CC may remain
    // null, and a deferred Destroy is still a physically present valid target.
    let mut claimed_slots = BTreeMap::new();
    for (owner_id, entity) in sim.substrate.entities.iter_sorted() {
        // Diagnostic slots0..20 are Building+55C, slots21..28 are +5C8.
        for (slot, anim_id) in entity
            .building_art
            .anim_slots()
            .iter()
            .chain(entity.damage_fires.anim_ids().iter())
            .enumerate()
        {
            let Some(anim_id) = *anim_id else { continue };
            let slot = slot as u8;
            if entity.category() != crate::map::entities::EntityCategory::Structure {
                return Err(SnapshotRestoreError::InvalidBuildingAnimSlotOwner { owner_id, slot });
            }
            if !anim_ids.contains(&anim_id) {
                return Err(SnapshotRestoreError::MissingBuildingSlotAnim {
                    owner_id,
                    slot,
                    anim_id,
                });
            }
            if let Some((first_owner, first_slot)) = claimed_slots.insert(anim_id, (owner_id, slot))
            {
                return Err(SnapshotRestoreError::DuplicateBuildingSlotAnim {
                    anim_id,
                    first_owner,
                    first_slot,
                    second_owner: owner_id,
                    second_slot: slot,
                });
            }
        }
    }

    // Swizzle::Apply has no unmatched-reference recovery path. Validate the
    // complete modeled pointer graph before mutating even weak references or
    // derived caches, so a failed restore remains an atomic rejection.
    for (entity_id, entity) in sim.substrate.entities.iter_sorted() {
        if let Some(anim_id) = entity
            .parasite
            .class()
            .and_then(|class| class.grapple_anim())
        {
            require_resolved_reference(
                anim_ids.contains(&anim_id),
                "EntityStore",
                entity_id,
                "parasite.grapple.anim",
                "AnimStore",
                anim_id,
            )?;
        }
        if entity.tesla_charger.is_active()
            && !matches!(
                entity.category(),
                crate::map::entities::EntityCategory::Unit
                    | crate::map::entities::EntityCategory::Infantry
                    | crate::map::entities::EntityCategory::Aircraft
            )
        {
            return Err(SnapshotRestoreError::InvalidTeslaChargingState {
                object_id: entity_id,
                reason: "charging intent belongs to a Foot",
            });
        }
        if !entity.tesla_charge.is_default()
            && entity.category() != crate::map::entities::EntityCategory::Structure
        {
            return Err(SnapshotRestoreError::InvalidTeslaChargingState {
                object_id: entity_id,
                reason: "charger list and weapon latch belong to a Building",
            });
        }
        let mut seen_chargers = BTreeSet::new();
        for &charger in entity.tesla_charge.chargers() {
            require_resolved_reference(
                entity_ids.contains(&charger),
                "EntityStore",
                entity_id,
                "tesla_charge.chargers",
                "EntityStore",
                charger,
            )?;
            if !seen_chargers.insert(charger)
                || sim.substrate.entities.get(charger).unwrap().category()
                    != crate::map::entities::EntityCategory::Infantry
            {
                return Err(SnapshotRestoreError::InvalidTeslaChargingState {
                    object_id: entity_id,
                    reason: "chargers must be distinct Infantry identities",
                });
            }
        }
        // Do not require a current reciprocal TarCom or recompute +661:
        // native saves can lie between retarget/expiry and Building AI.
        for contact_id in entity.radio_contacts.iter_live() {
            require_resolved_reference(
                entity_ids.contains(&contact_id),
                "EntityStore",
                entity_id,
                "radio_contacts",
                "EntityStore",
                contact_id,
            )?;
        }

        if let Some(cargo) = entity.passenger_role.cargo() {
            for &passenger_id in cargo.passengers() {
                require_resolved_reference(
                    entity_ids.contains(&passenger_id),
                    "EntityStore",
                    entity_id,
                    "passenger_role.cargo.passengers",
                    "EntityStore",
                    passenger_id,
                )?;
            }
        }

        // SpawnManagerClass__Load @ 0x006B7F10 queues every non-null
        // SpawnControl child pointer plus its current and queued target slots
        // for the common post-load swizzle pass. Rust keeps stable IDs in those
        // fields, so admission requires the same references to resolve before
        // any cleanup mutates the restored graph.
        if let Some(owner) = entity.airstrike_link() {
            require_resolved_reference(
                sim.substrate
                    .entities
                    .get(owner)
                    .is_some_and(|owner| owner.airstrike.owns_manager()),
                "EntityStore",
                entity_id,
                "airstrike.link",
                "AirstrikeManager",
                owner,
            )?;
        }
        for referenced_id in entity.airstrike.referenced_ids() {
            require_resolved_reference(
                entity_ids.contains(&referenced_id),
                "EntityStore",
                entity_id,
                "airstrike.reference",
                "EntityStore",
                referenced_id,
            )?;
        }
        if let Some(manager) = entity.spawn_manager.as_ref() {
            for slot in manager.slots() {
                if let Some(spawn_id) = slot.spawn() {
                    require_resolved_reference(
                        entity_ids.contains(&spawn_id),
                        "EntityStore",
                        entity_id,
                        "spawn_manager.slots.spawn",
                        "EntityStore",
                        spawn_id,
                    )?;
                }
            }
            if let Some(TargetKind::Entity(target_id)) = manager.current_target() {
                require_object_reference(
                    &entity_ids,
                    sim.terrain_objects.objects(),
                    "EntityStore",
                    entity_id,
                    "spawn_manager.current_target",
                    target_id,
                )?;
            }
            if let Some(TargetKind::Entity(target_id)) = manager.queued_target() {
                require_object_reference(
                    &entity_ids,
                    sim.terrain_objects.objects(),
                    "EntityStore",
                    entity_id,
                    "spawn_manager.queued_target",
                    target_id,
                )?;
            }
        }

        // TechnoClass__Load @ 0x0070BF50 queues the spawned-child parent
        // pointer at Techno+0x2D4. It is an independently swizzled slot: native
        // load does not require a reciprocal SpawnControl entry.
        if let Some(parent_id) = entity.spawn_owner_id.owner() {
            require_resolved_reference(
                entity_ids.contains(&parent_id),
                "EntityStore",
                entity_id,
                "spawn_owner_id",
                "EntityStore",
                parent_id,
            )?;
        }

        // A bomb's planter (`BombClass+0x24`) is a pointer its expiry nulls
        // (`BombListClass::PointerGotInvalid @ 0x00439150`).
        if let Some(planter) = entity.bomb.and_then(|bomb| bomb.planter()) {
            require_resolved_reference(
                entity_ids.contains(&planter),
                "EntityStore",
                entity_id,
                "bomb.planter",
                "EntityStore",
                planter,
            )?;
        }

        if let Some(TargetKind::Entity(target_id)) = entity.attack_target.target() {
            require_object_reference(
                &entity_ids,
                sim.terrain_objects.objects(),
                "EntityStore",
                entity_id,
                "attack_target",
                target_id,
            )?;
        }
        if let Some(TargetKind::Entity(target_id)) = entity.suspended_attack_target.target() {
            require_object_reference(
                &entity_ids,
                sim.terrain_objects.objects(),
                "EntityStore",
                entity_id,
                "suspended_attack_target",
                target_id,
            )?;
        }

        let nav = &entity.navigation;
        for (field, targets) in [
            (
                "navigation.suspended_nav_com",
                nav.suspended_nav_com().as_slice(),
            ),
            ("navigation.nav_com_aux", nav.nav_com_aux().as_slice()),
            ("navigation.nav_com", nav.nav_com().as_slice()),
            ("navigation.nav_queue", nav.nav_queue()),
            ("navigation.nav_list", nav.nav_list()),
        ] {
            for target_id in targets.iter().filter_map(|target| target.object_id()) {
                require_object_reference(
                    &entity_ids,
                    sim.terrain_objects.objects(),
                    "EntityStore",
                    entity_id,
                    field,
                    target_id,
                )?;
            }
        }

        if let Some(target_id) = entity.tether.partner() {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                "tether",
                "EntityStore",
                target_id,
            )?;
        }

        if let Some(manager) = entity.capture_manager.as_ref() {
            for target_id in manager.victims() {
                require_resolved_reference(
                    entity_ids.contains(&target_id),
                    "EntityStore",
                    entity_id,
                    "capture_manager.nodes",
                    "EntityStore",
                    target_id,
                )?;
            }
        }
        if let Some(controller_id) = entity.mind_control.controller() {
            require_resolved_reference(
                entity_ids.contains(&controller_id),
                "EntityStore",
                entity_id,
                "mind_control.controller",
                "EntityStore",
                controller_id,
            )?;
        }
        for target_id in entity.temporal.references() {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                "temporal",
                "EntityStore",
                target_id,
            )?;
        }
        if let Some(pending) = entity.pending_entry() {
            require_resolved_reference(
                entity_ids.contains(&pending),
                "EntityStore",
                entity_id,
                "pending_entry",
                "EntityStore",
                pending,
            )?;
        }
        if let Some(ammo) = entity.ammo()
            && let Some(target_id) = ammo.dock()
        {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                "ammo.dock",
                "EntityStore",
                target_id,
            )?;
        }
        if let Some(master_id) = entity.slave.owner() {
            require_resolved_reference(
                entity_ids.contains(&master_id),
                "EntityStore",
                entity_id,
                "slave.owner",
                "EntityStore",
                master_id,
            )?;
        }
        if let Some(manager) = entity.slave_manager.as_ref() {
            for slave_id in manager.slaves() {
                require_resolved_reference(
                    entity_ids.contains(&slave_id),
                    "EntityStore",
                    entity_id,
                    "slave_manager.nodes.slave",
                    "EntityStore",
                    slave_id,
                )?;
            }
        }

        if let Some(target_id) = entity.passenger_role.inside_transport_id() {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                "passenger_role.inside",
                "EntityStore",
                target_id,
            )?;
        }

        for (field, system_id) in [
            (
                "spark_particle_system",
                entity.spark_particle_system.system(),
            ),
            (
                "damage_smoke_system_id",
                entity.damage_smoke_system_id.system(),
            ),
        ]
        .into_iter()
        .filter_map(|(field, system)| system.map(|id| (field, id)))
        {
            require_resolved_reference(
                particle_system_ids.contains(&system_id),
                "EntityStore",
                entity_id,
                field,
                "ParticleSystemStore",
                system_id,
            )?;
        }
        if let Some(target_id) = entity.bunker.occupant() {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                "bunker_occupant",
                "EntityStore",
                target_id,
            )?;
        }
        if let BunkerLink::Installed(target_id) = entity.bunker.link() {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                "bunker_link",
                "EntityStore",
                target_id,
            )?;
        }
    }

    for (&anim_id, anim) in sim.substrate.anims.iter() {
        if let Some(owner_id) = anim.owner_entity() {
            require_resolved_reference(
                entity_ids.contains(&owner_id),
                "AnimStore",
                anim_id,
                "owner_entity",
                "EntityStore",
                owner_id,
            )?;
        }
    }

    for (&system_id, system) in sim.substrate.particle_systems.iter() {
        if let Some(owner_id) = system.owner_entity() {
            require_object_reference(
                &entity_ids,
                sim.terrain_objects.objects(),
                "ParticleSystemStore",
                system_id,
                "owner_entity",
                owner_id,
            )?;
        }
        if let Some(crate::sim::combat::TargetKind::Entity(attached_id)) = system.target_ref() {
            require_object_reference(
                &entity_ids,
                sim.terrain_objects.objects(),
                "ParticleSystemStore",
                system_id,
                "target_ref",
                attached_id,
            )?;
        }
    }

    // BulletClass__Load @ 0x0046AE70 queues the non-null source/firer pointer
    // at +0xB0, target at +0x10C and awaited Anim at +0x154 for global
    // swizzling (46AF8B), independently of the wait flag. VERA's zero
    // source sentinel and Cell/null targets are the corresponding
    // null/non-pointer representations and therefore need no object lookup.
    for (&projectile_id, projectile) in sim.projectiles.iter() {
        if let Some(anim_id) = projectile.awaited_anim() {
            require_resolved_reference(
                anim_ids.contains(&anim_id),
                "ProjectileStore",
                projectile_id,
                "awaited_anim",
                "AnimStore",
                anim_id,
            )?;
        }
        if projectile.source_id() != crate::sim::combat::RAD_NO_ATTACKER {
            require_resolved_reference(
                entity_ids.contains(&projectile.source_id()),
                "ProjectileStore",
                projectile_id,
                "source_id",
                "EntityStore",
                projectile.source_id(),
            )?;
        }
        if let Some((target_id, target_registry, present)) = match projectile.target() {
            ProjectileTarget::Entity(id) => Some((id, "EntityStore", entity_ids.contains(&id))),
            ProjectileTarget::Terrain(id) => Some((
                id,
                "TerrainObjectStore",
                sim.terrain_objects.objects().contains_key(&id),
            )),
            ProjectileTarget::Cell { .. }
            | ProjectileTarget::DummyCell
            | ProjectileTarget::None => None,
        } {
            require_resolved_reference(
                present,
                "ProjectileStore",
                projectile_id,
                "target",
                target_registry,
                target_id,
            )?;
        }
    }

    let wave_ids = sim.waves.iter().map(|(&id, _)| id).collect::<BTreeSet<_>>();
    for (&wave_id, wave) in sim.waves.iter() {
        if let Some(owner_id) = wave.owner_id() {
            require_resolved_reference(
                entity_ids.contains(&owner_id),
                "WaveStore",
                wave_id,
                "owner_id",
                "EntityStore",
                owner_id,
            )?;
        }
        if let Some(TargetKind::Entity(target_id)) = wave.target_ref() {
            require_object_reference(
                &entity_ids,
                sim.terrain_objects.objects(),
                "WaveStore",
                wave_id,
                "target_ref",
                target_id,
            )?;
        }
    }
    for (owner_id, wave_id) in sim.active_wave_links() {
        require_resolved_reference(
            entity_ids.contains(&owner_id),
            "ActiveWaveLinks",
            owner_id,
            "owner",
            "EntityStore",
            owner_id,
        )?;
        require_resolved_reference(
            wave_ids.contains(&wave_id)
                && sim.waves.get(wave_id).and_then(|wave| wave.owner_id()) == Some(owner_id),
            "ActiveWaveLinks",
            owner_id,
            "wave",
            "WaveStore",
            wave_id,
        )?;
    }

    for object_id in sim.custom_sound_objects() {
        require_object_reference(
            &entity_ids,
            sim.terrain_objects.objects(),
            "ObjectCustomSounds",
            object_id,
            "object",
            object_id,
        )?;
    }

    for object_id in sim.substrate.pending_delete.object_tickets() {
        require_resolved_reference(
            identities.contains_key(&object_id),
            "PendingDeleteList",
            object_id,
            "entry",
            "object namespace",
            object_id,
        )?;
    }

    for factory in sim.production.factories().iter_insertion_ordered() {
        if let Some(object_id) = factory.object().and_then(|object| object.entity_id) {
            require_resolved_reference(
                entity_ids.contains(&object_id),
                "FactoryRegistry",
                u64::from(factory.owner().index()),
                "object.entity_id",
                "EntityStore",
                object_id,
            )?;
        }
    }

    // A deliberate weak identity: C4 kill credit may outlive its attacker.
    for entity in sim.substrate.entities.values_mut() {
        if entity
            .pending_c4_detonation
            .pending()
            .and_then(|pending| pending.source_entity_id())
            .is_some_and(|id| !entity_ids.contains(&id))
        {
            entity.pending_c4_detonation.forget_source();
        }
    }

    // The primary-factory links (the native primary byte, written by
    // Building448070/448160) name objects by id. Restore retains limbo
    // flags and prunes ids the validated object graph no longer holds.
    sim.production.retain_primary_factory_links(&entity_ids);

    Ok(())
}

impl Simulation {
    /// Validate and re-register a deserialized simulation before it can resume.
    ///
    /// Native load registers old identities and pointer slots, resolves them,
    /// then restores global active/cell membership. Rust stores stable IDs
    /// directly, so the equivalent transaction builds one global namespace,
    /// rejects any unmatched modeled native pointer, cleans only deliberate
    /// weak/derived identities, and reconstructs skipped indexes in dependency
    /// order.
    pub fn restore_after_snapshot_load(&mut self) -> Result<(), SnapshotRestoreError> {
        for (_, entity) in self.substrate.entities.iter_sorted() {
            entity
                .validate_ares_build_compatibility()
                .map_err(SnapshotRestoreError::InvalidUnitDelivery)?;
        }
        self.validate_paid_super_weapon_selections()
            .map_err(SnapshotRestoreError::InvalidPaidSuperWeaponSelection)?;
        #[cfg(feature = "ares")]
        self.super_weapons
            .validate_ares_launch_history()
            .map_err(SnapshotRestoreError::InvalidSuperWeaponLaunchHistory)?;
        self.unit_deliveries
            .validate(self)
            .map_err(SnapshotRestoreError::InvalidUnitDelivery)?;
        self.validate_restored_spotlights()
            .map_err(
                |(object_id, reason)| SnapshotRestoreError::InvalidSpotlightState {
                    object_id,
                    reason,
                },
            )?;
        self.projectiles
            .validate_scalable_runtime(&self.interner)
            .map_err(SnapshotRestoreError::InvalidScalableProjectileRuntime)?;
        self.radiation
            .validate_site_order()
            .map_err(SnapshotRestoreError::InvalidRadiationSiteOrder)?;
        self.validate_planning_references()
            .map_err(|reason| SnapshotRestoreError::InvalidPlanningReferences { reason })?;
        if let Some(owner) = self.session.current_house()
            && !self.houses.contains_key(&owner)
        {
            return Err(SnapshotRestoreError::InvalidCurrentHouse { owner });
        }
        if self.session.outcome_request().is_some() && self.session.current_house().is_none() {
            return Err(SnapshotRestoreError::InvalidFrameContinuation {
                reason: "scenario result request has no current House",
            });
        }
        self.validate_frame_continuation_for_restore()
            .map_err(|reason| SnapshotRestoreError::InvalidFrameContinuation { reason })?;
        if let Some(observer) = self.session.observer_house()
            && !self.houses.contains_key(&observer)
        {
            return Err(SnapshotRestoreError::InvalidMultiplayerSession {
                reason: "observer House is absent",
            });
        }
        if self.session.game_mode().is_network()
            && self.session.network_max_ahead_frames().is_none()
        {
            return Err(SnapshotRestoreError::InvalidMultiplayerSession {
                reason: "network timing is absent",
            });
        }
        let scenario_names_admitted = self
            .houses
            .values()
            .any(|house| house.native_scenario_name().is_some());
        // VERA-internal: the dying mark is the SHP vehicle/aircraft death and
        // UnInit's. A live infantryman with it would take the vehicle death
        // path; his corpse is an ordinary IsAlive object at Health 0.
        for (object_id, entity) in self.substrate.entities.iter_sorted() {
            if entity.category() == crate::map::entities::EntityCategory::Infantry
                && entity.death.is_dying()
                && entity.lifecycle.object_alive()
            {
                return Err(SnapshotRestoreError::InvalidInfantryDyingMark { object_id });
            }
        }
        for (&owner, house) in &self.houses {
            house.validate_scenario_name().map_err(|reason| {
                SnapshotRestoreError::InvalidHouseScenarioName { owner, reason }
            })?;
            if scenario_names_admitted && house.native_scenario_name().is_none() {
                return Err(SnapshotRestoreError::InvalidHouseScenarioName {
                    owner,
                    reason: "admitted scenario requires a name for every registered House",
                });
            }
            house.validate_production_history().map_err(|reason| {
                SnapshotRestoreError::InvalidHouseProductionHistory { owner, reason }
            })?;
            if house.is_to_die() && (house.has_won() || house.has_lost()) {
                return Err(SnapshotRestoreError::InvalidHouseOutcomeState {
                    owner,
                    reason: "Flag_To_Die conflicts with a terminal result",
                });
            }
            // FlagWin4FC9EE refuses Lost, and FlagLose4FCBDE clears Won.
            // House4F8657 also clears Won after its wait, independently of
            // the sticky process result requests. A later Action2 may change
            // the House flags and restart +298 after victory was admitted.
            if house.has_won() && house.has_lost() {
                return Err(SnapshotRestoreError::InvalidHouseOutcomeState {
                    owner,
                    reason: "Won and Lost flags conflict",
                });
            }
        }

        let (index, identities) = RestoredObjectIndex::build(self)?;
        let seen_logic = self
            .substrate
            .validate_restored_counters_and_logic(index.highest_id, &identities)?;
        let pending_delete_ids = self
            .substrate
            .pending_delete
            .object_tickets()
            .collect::<BTreeSet<_>>();
        for (object_id, _, alive, in_limbo) in self.substrate.particle_systems.detached_particles()
        {
            if seen_logic.contains(&object_id)
                || (in_limbo && self.display_layers().layer_of(object_id).is_some())
                || (!alive && !in_limbo)
            {
                return Err(SnapshotRestoreError::InvalidDetachedParticleState {
                    object_id,
                    reason: "detached body remains in Logic, limbo body remains in Display, or dead body is not in limbo",
                });
            }
            if !alive && !pending_delete_ids.contains(&object_id) {
                return Err(SnapshotRestoreError::MissingDeferredDeleteIdentity {
                    registry: "ParticleSystemStore.detached",
                    object_id,
                });
            }
        }
        for ticket in self.substrate.pending_delete.retained_tickets() {
            use crate::sim::world::DeferredDeleteTicket;
            let (registry, identity, resolves) = match ticket {
                DeferredDeleteTicket::Object(id) => ("Object", id, identities.contains_key(&id)),
                DeferredDeleteTicket::Overlay(id) => {
                    ("Overlay", id, self.load_objects.has_overlay(id))
                }
                DeferredDeleteTicket::DiskLaser(id) => ("DiskLaser", id, self.has_disk_laser(id)),
                DeferredDeleteTicket::Tag(id) => {
                    ("Tag", id as u64, self.trigger_runtime.has_live_tag(id))
                }
                DeferredDeleteTicket::Trigger(id) => (
                    "Trigger",
                    id as u64,
                    self.trigger_runtime.has_live_trigger(id),
                ),
            };
            if !resolves {
                return Err(SnapshotRestoreError::UnresolvedDeferredDeleteTicket {
                    registry,
                    identity,
                });
            }
        }
        if let Some(grid) = &self.smudge_grid {
            for (object_id, object) in grid.objects() {
                if object.object_alive() || !object.in_limbo() {
                    return Err(SnapshotRestoreError::InvalidSmudgeLifetime { object_id });
                }
                if seen_logic.contains(&object_id) {
                    return Err(SnapshotRestoreError::InactiveLogicIdentity {
                        registry: "SmudgeGrid",
                        object_id,
                    });
                }
                if !pending_delete_ids.contains(&object_id) {
                    return Err(SnapshotRestoreError::MissingDeferredDeleteIdentity {
                        registry: "SmudgeGrid",
                        object_id,
                    });
                }
            }
        }
        for terrain in self.terrain_objects.objects().values() {
            let in_logic = seen_logic.contains(&terrain.stable_id());
            let pending_delete = pending_delete_ids.contains(&terrain.stable_id());
            if !terrain.is_live() && in_logic {
                return Err(SnapshotRestoreError::InactiveLogicIdentity {
                    registry: "TerrainObjectStore",
                    object_id: terrain.stable_id(),
                });
            }
            if terrain.is_live() && pending_delete {
                return Err(SnapshotRestoreError::DeferredDeleteLogicIdentity {
                    registry: "TerrainObjectStore",
                    object_id: terrain.stable_id(),
                });
            }
            if !terrain.is_live() && !pending_delete {
                return Err(SnapshotRestoreError::MissingDeferredDeleteIdentity {
                    registry: "TerrainObjectStore",
                    object_id: terrain.stable_id(),
                });
            }
        }
        for (&object_id, _) in self.projectiles.iter() {
            let in_logic = seen_logic.contains(&object_id);
            let pending_delete = pending_delete_ids.contains(&object_id);
            if !in_logic && !pending_delete {
                return Err(SnapshotRestoreError::MissingRequiredLogicIdentity {
                    registry: "ProjectileStore",
                    object_id,
                });
            }
            if in_logic && pending_delete {
                return Err(SnapshotRestoreError::DeferredDeleteLogicIdentity {
                    registry: "ProjectileStore",
                    object_id,
                });
            }
        }
        for (&object_id, _) in self.waves.iter() {
            let in_logic = seen_logic.contains(&object_id);
            let pending_delete = pending_delete_ids.contains(&object_id);
            if !in_logic && !pending_delete {
                return Err(SnapshotRestoreError::MissingRequiredLogicIdentity {
                    registry: "WaveStore",
                    object_id,
                });
            }
            if in_logic && pending_delete {
                return Err(SnapshotRestoreError::DeferredDeleteLogicIdentity {
                    registry: "WaveStore",
                    object_id,
                });
            }
        }

        validate_passenger_size_tables(self)?;
        // Concrete Unit74451C/Aircraft41B4DC/Infantry521A0C/Building453ECF
        // Load reconstructs Techno's three empty vectors through NoInit
        //6F43AB..6F4410. Their raw headers have no element swizzles; discard
        // them before validating strong references, retaining real TarCom.
        for id in self.substrate.entities.keys_sorted() {
            self.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .clear_distributed_fire_after_load();
        }
        restore_object_references(self, &identities)?;

        // EntityStore already restored its indexes during Deserialize (Clone
        // retains them too). Reference restoration above changes no indexed identity.
        // Continue with Logic slots (including ParticleSystem). Actual CellClass
        // list heads/order were serialized, like483C10/4839F0: validate references
        // without requerying geometry before map authority is reattached.
        self.projectiles
            .restart_arm_timers_after_load(self.session.binary_frame() as i32);
        self.projectiles.restore_scalable_after_load();
        // ParasiteClass Load 6295DB..6295F3 does the same for its suppression
        // and bite timers: a bite is due at once and suppression is dropped.
        let frame = self.session.binary_frame();
        // RadSiteClass::Load65B3D0 also restarts its level/light clocks;
        // the saved field and source stay intact until ordinary RadSite AI.
        self.radiation.restart_timers_after_load(frame);
        for id in self.substrate.entities.keys_sorted() {
            self.substrate.restore_move_sound_state_after_load(id);
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                entity.parasite.restart_timers_after_load(frame);
            }
        }
        self.rebuild_logic_membership();
        self.substrate.rebuild_building_anim_slot_indices();
        Self::rebuild_bomb_carriers(&self.substrate, &mut self.bombs);
        // The kamikaze tracker's Load (`0x0054E7B0`) follows its Clear.
        self.kamikaze
            .restart_after_load(self.session.binary_frame());
        self.substrate
            .occupancy
            .restore_memberships(&self.substrate.entities)
            .map_err(|reason| SnapshotRestoreError::InvalidCellMembership { reason })?;
        Ok(())
    }

    /// Reproject serialized overlay and bridge authority onto the fresh
    /// map-derived terrain cache, then publish canonical navigation before the
    /// restored simulation can run another frame.
    ///
    /// The full row-major sweep is required because OverlayGrid's dirty queues
    /// are transient: a saved cell may have been cleared since map load, so an
    /// occupied-only replay would leave the original map overlay's passability
    /// behind. Bridge cell state needs no separate pass: its flags return
    /// through the dynamic terrain cells above, and its overlay identity and
    /// state byte through OverlayGrid.
    pub fn restore_map_authority_after_snapshot_load(
        &mut self,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
        prepare_terrain_assets: Option<&mut SnapshotTerrainAssetPreparation<'_>>,
    ) -> Result<SnapshotMapRestoreOutput, SnapshotRestoreError> {
        self.validate_type_tech_levels(rules)?;
        // TerrainLoad71CDA0 preserves the separately saved Display/Logic
        // vectors. Only ObjectUnlimbo5F4FEF..5F5040's eligible spawners need
        // Logic; validate with restored type state without rebuilding order.
        for terrain in self.terrain_objects.objects().values() {
            if !terrain.is_live() {
                continue;
            }
            let type_name = self.interner.resolve(terrain.type_ref());
            let terrain_type = rules
                .terrain_object_type_case_insensitive(type_name)
                .ok_or_else(|| SnapshotRestoreError::MissingTerrainType {
                    object_id: terrain.stable_id(),
                    type_name: type_name.to_owned(),
                })?;
            if !terrain.in_logic_vector()
                && terrain.joins_logic_on_unlimbo(terrain_type, self.session.game_mode())
            {
                return Err(SnapshotRestoreError::MissingRequiredLogicIdentity {
                    registry: "TerrainObjectStore",
                    object_id: terrain.stable_id(),
                });
            }
        }
        #[cfg(feature = "ares")]
        self.validate_attached_speed_after_restore(rules)
            .map_err(
                |(object_id, reason)| SnapshotRestoreError::InvalidAttachedSpeedState {
                    object_id,
                    reason,
                },
            )?;
        #[cfg(feature = "ares")]
        self.validate_factory_country_references(rules)
            .map_err(|reason| SnapshotRestoreError::InvalidFactoryCountry { reason })?;
        self.validate_disk_lasers_after_restore(rules)
            .map_err(
                |(object_id, reason)| SnapshotRestoreError::InvalidDiskLaserState {
                    object_id,
                    reason,
                },
            )?;
        let (overlay_width, overlay_height, overlay_cell_count, retained_wall_count) = self
            .overlay_grid
            .as_ref()
            .map(|grid| {
                (
                    grid.width(),
                    grid.height(),
                    grid.cell_storage_len(),
                    grid.retained_neighbor_count_storage_len(),
                )
            })
            .ok_or(SnapshotRestoreError::MissingMapAuthorityComponent {
                component: "OverlayGrid",
            })?;
        let expected_overlay_cell_count = overlay_width as usize * overlay_height as usize;
        if overlay_cell_count != expected_overlay_cell_count {
            return Err(SnapshotRestoreError::MapAuthorityCellStorageMismatch {
                expected: expected_overlay_cell_count,
                found: overlay_cell_count,
            });
        }
        // `CellClass+0x122`'s wall contribution is written only by
        // `OverlayClass::Mark` and decremented only by an explicit removal;
        // nothing in gamemd rebuilds it from final wall identities, so the
        // saved plane must cover every cell.
        if retained_wall_count != expected_overlay_cell_count {
            return Err(SnapshotRestoreError::RetainedWallNeighborStorageMismatch {
                expected: expected_overlay_cell_count,
                found: retained_wall_count,
            });
        }
        let (terrain_width, terrain_height) = self
            .resolved_terrain
            .as_ref()
            .map(|terrain| (terrain.width(), terrain.height()))
            .ok_or(SnapshotRestoreError::MissingMapAuthorityComponent {
                component: "ResolvedTerrainGrid",
            })?;
        if (overlay_width, overlay_height) != (terrain_width, terrain_height) {
            return Err(SnapshotRestoreError::MapAuthorityDimensionMismatch {
                overlay_width,
                overlay_height,
                terrain_width,
                terrain_height,
            });
        }

        // Reproject runtime isometric replacements onto the pristine
        // app-supplied map before overlay, bridge, and zone reconstruction.
        self.dynamic_terrain_cells
            .apply_to(
                self.resolved_terrain
                    .as_mut()
                    .expect("validated terrain cache"),
            )
            .map_err(|(rx, ry)| SnapshotRestoreError::DynamicTerrainCellMissing { rx, ry })?;

        // MouseClass::Load @ 0x005BE657 calls the restored-cell TMP admission
        // pass @ 0x00546DA0 before publishing the loaded map. Admission must
        // see saved live replacements, not the baseline terrain. The optional
        // app callback binds retail assets and can consume only this candidate's
        // Main draws; legacy/headless callers already carry their catalogs.
        if let Some(prepare_terrain_assets) = prepare_terrain_assets {
            let (size_width, _) = self.map_size_diamond().ok_or(
                SnapshotRestoreError::MissingMapAuthorityComponent {
                    component: "Map Size",
                },
            )?;
            prepare_terrain_assets(
                self.resolved_terrain
                    .as_mut()
                    .expect("validated terrain cache"),
                self.main_rng.draws(),
                size_width,
            )
            .map_err(|reason| SnapshotRestoreError::TerrainAssetPreparationFailed { reason })?;
        }

        // Native CellClass::Load restores allocated real-cell flag words as
        // values. Do this before derived overlay/bridge reconstruction and do
        // not route through GetCell or SetBridgeDirection: those would stamp
        // the process-global dummy that Resize reconstructs at commit.
        if !self
            .resolved_terrain
            .as_mut()
            .expect("validated terrain cache")
            .restore_real_cell_bridge_flags_0x1180(&self.real_cell_bridge_flags_0x1180)
        {
            return Err(SnapshotRestoreError::RealCellBridgeFlagAuthorityMismatch);
        }

        // The app-supplied terrain template is the immutable load-time map.
        // Action 40's mutable LocalSize is serialized on Simulation, so replay
        // its CellClass-derived cache before overlay and zone reconstruction.
        if let Some(bounds) = self.playfield_bounds() {
            let _ = self
                .resolved_terrain
                .as_mut()
                .expect("validated terrain cache")
                .recalc_playfield_attributes(bounds);
        }

        {
            let overlay_grid = self.overlay_grid.as_mut().expect("validated overlay cache");
            let resolved_terrain = self
                .resolved_terrain
                .as_mut()
                .expect("validated terrain cache");
            for ry in 0..overlay_height {
                for rx in 0..overlay_width {
                    let _ = crate::sim::overlay_grid::recalc_overlay_passability(
                        overlay_grid,
                        resolved_terrain,
                        overlay_registry,
                        rx,
                        ry,
                    );
                }
            }
        }

        if !self.rebuild_dynamic_navigation(rules) {
            return Err(SnapshotRestoreError::NavigationAuthorityPublicationFailed);
        }

        let occupied_overlays = self
            .overlay_grid
            .as_ref()
            .expect("validated overlay cache")
            .iter_occupied()
            .filter_map(|(rx, ry, cell)| {
                cell.overlay_id
                    .map(|overlay_id| crate::map::overlay::OverlayEntry {
                        rx,
                        ry,
                        overlay_id,
                        frame: cell.overlay_data,
                    })
            })
            .collect();
        let native_tiberium_stats =
            self.rebuild_native_tiberium_queues_after_snapshot_load(rules, overlay_registry)?;
        // CellTag identities are meaningful only after the canonical Cell
        // allocation is restored. Keep their final owner validation in this
        // shared admission seam, before an app can publish the candidate.
        self.validate_restored_trigger_runtime()
            .map_err(|reason| SnapshotRestoreError::InvalidTriggerRuntime { reason })?;
        Ok(SnapshotMapRestoreOutput {
            occupied_overlays,
            native_tiberium_stats,
        })
    }

    /// Replace serialized Tiberium queue stores with their native post-load
    /// cell-derived result. This consumes no RNG and leaves both class timers
    /// due at the restored binary frame.
    ///
    /// gamemd-derived: `TiberiumClass::Load @ 0x00721E80` discards both
    /// dynamic queue stores and calls `InitGrowthQueues_All @ 0x00722D00` then
    /// `InitSpreadQueues_All @ 0x00722240`. Spread admission reads the ground
    /// CellClass object list, never the bridge/AltObject list.
    fn rebuild_native_tiberium_queues_after_snapshot_load(
        &mut self,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    ) -> Result<crate::sim::ore_growth::NativeTiberiumRebuildStats, SnapshotRestoreError> {
        let overlay_grid = self.overlay_grid.as_ref().ok_or(
            SnapshotRestoreError::MissingMapAuthorityComponent {
                component: "OverlayGrid",
            },
        )?;
        let resolved_terrain = self.resolved_terrain.as_ref().ok_or(
            SnapshotRestoreError::MissingMapAuthorityComponent {
                component: "ResolvedTerrainGrid",
            },
        )?;
        let mut source_object_cells: BTreeSet<(u16, u16)> = self
            .terrain_objects
            .objects()
            .values()
            .filter(|terrain| terrain.is_live())
            .map(crate::sim::terrain_object::TerrainObjectState::cell)
            .collect();
        source_object_cells.extend(
            self.substrate
                .occupancy
                .occupied_cells_on_layer(crate::sim::movement::locomotor::MovementLayer::Ground),
        );

        let grows = self.ore_growth_config.grows();
        let spreads = self.ore_growth_config.spreads();
        let current_frame = self.session.binary_frame();
        // The stores are sized and walked from the native `MapRect` (`[Map]
        // Size`), never the cell-array extent in `session.map_*`. The rect the
        // fresh load rebuilt with is serialized; a zero rect (no rebuild ever
        // ran) falls back to the retained MapClass Size dimensions.
        let mut native_rect = self.ore_growth_state.native_rect();
        if native_rect == (0, 0)
            && let (Some(bounds), Some(height)) =
                (self.playfield_bounds(), self.playfield_size_height())
        {
            native_rect = (
                u16::try_from(bounds.base).unwrap_or(0),
                u16::try_from(height).unwrap_or(0),
            );
        }
        Ok(self
            .ore_growth_state
            .rebuild_native_tiberium_queues_from_overlays(
                overlay_grid,
                overlay_registry,
                &rules.tiberium_types,
                Some(resolved_terrain),
                &source_object_cells,
                grows,
                spreads,
                current_frame,
                native_rect,
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::ResolvedTerrainGrid;
    use crate::rules::locomotor_type::MovementZone;
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::pathfinding::PathGrid;
    use crate::sim::pathfinding::zone_map::ZoneGrid;
    use crate::sim::world::{RevealOutcome, Simulation};

    /// Helper: advance a sim by one tick with empty inputs.
    fn tick(sim: &mut Simulation) {
        sim.advance_tick(&[], None, None, None, 67);
    }

    fn flat_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
        crate::map::resolved_terrain::test_grid(
            width,
            height,
            crate::map::resolved_terrain::test_clear_cell,
        )
    }

    #[test]
    fn retail_static_tree_display_survives_load_without_logic_membership() {
        use crate::sim::world::display_layers::DisplayLayer;

        let Some(battle) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("Dustbowl.mmx")
        else {
            return;
        };
        let rules = &battle.rules;
        let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
            &battle.processed_rules,
            Some(&battle.fixed_art),
        );
        let terrain = flat_terrain(8, 8);
        let mut sim = Simulation::new();
        sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(8, 8));
        sim.install_resolved_terrain_for_new_map(terrain.clone());
        crate::sim::terrain_spawn::construct_terrain_objects(
            &mut sim,
            &[
                crate::map::overlay::TerrainObject {
                    rx: 3,
                    ry: 4,
                    name: "TREE01".into(),
                },
                crate::map::overlay::TerrainObject {
                    rx: 5,
                    ry: 4,
                    name: "TIBTRE01".into(),
                },
            ],
            rules,
            false,
        );
        let tree = sim.terrain_objects.id_at((3, 4)).unwrap();
        let spawner = sim.terrain_objects.id_at((5, 4)).unwrap();
        assert_eq!(sim.live_object_order_snapshot(), [spawner]);
        assert_eq!(
            sim.substrate.display.members(DisplayLayer::GROUND),
            [tree, spawner]
        );
        let bytes = GameSnapshot::save(&sim, 0, 0, "terrain-membership", 0);
        let mut loaded = GameSnapshot::load(&bytes).unwrap().sim;
        loaded
            .restore_after_snapshot_load()
            .expect("a live static tree needs no Logic slot");
        loaded.rebuild_caches_after_load(
            terrain,
            crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            rules,
        );
        loaded
            .restore_map_authority_after_snapshot_load(rules, &registry, None)
            .unwrap();
        assert_eq!(loaded.live_object_order_snapshot(), [spawner]);
        assert_eq!(
            loaded.substrate.display.members(DisplayLayer::GROUND),
            [tree, spawner]
        );
        assert!(!loaded.terrain_objects.get(tree).unwrap().in_logic_vector());
        let warhead_ref = loaded.interner.intern("HE");
        loaded.commit_direct_terrain_damage_receiver(
            rules,
            Some(&registry),
            crate::sim::combat::TerrainDamageEvent {
                stable_id: tree,
                rx: 3,
                ry: 4,
                damage: loaded.terrain_objects.get(tree).unwrap().health(),
                distance_leptons: 0,
                warhead_ref,
                near_center_ic_isolation_eligible: false,
            },
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(
            loaded.substrate.display.members(DisplayLayer::GROUND),
            [spawner]
        );
        assert_eq!(loaded.live_object_order_snapshot(), [spawner]);
        assert!(loaded.terrain_objects.id_at((3, 4)).is_none());
        assert_eq!(loaded.substrate.pending_delete.object_ids(), [tree]);

        loaded.set_logic_order_for_test(Vec::new());
        loaded.rebuild_logic_membership();
        assert_eq!(
            loaded
                .restore_map_authority_after_snapshot_load(rules, &registry, None)
                .unwrap_err(),
            SnapshotRestoreError::MissingRequiredLogicIdentity {
                registry: "TerrainObjectStore",
                object_id: spawner,
            },
        );
        assert_eq!(
            loaded
                .restore_map_authority_after_snapshot_load(
                    crate::rules::ruleset::RuleSet::empty_for_test(),
                    &registry,
                    None,
                )
                .unwrap_err(),
            SnapshotRestoreError::MissingTerrainType {
                object_id: spawner,
                type_name: "TIBTRE01".into(),
            },
        );
    }

    mod gsi_17_04_tests {
        use super::flat_terrain;
        use crate::map::basic::{BasicSection, SpecialFlagsSection};
        use crate::map::entities::EntityCategory;
        use crate::map::map_file::MapHeader;
        use crate::rules::ini_parser::IniFile;
        use crate::rules::overlay_types::OverlayTypeRegistry;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::combat::Health;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::movement::locomotor::MovementLayer;
        use crate::sim::ore_growth::{OreGrowthConfig, OreGrowthState};
        use crate::sim::overlay_grid::OverlayGrid;
        use crate::sim::rng::SimRng;
        use crate::sim::snapshot::{GameSnapshot, SnapshotRestoreError};
        use crate::sim::terrain_object::TerrainObjectState;
        use crate::sim::world::Simulation;
        use std::collections::BTreeSet;

        fn tiberium_fixture() -> (RuleSet, OverlayTypeRegistry, u8) {
            let ini = IniFile::from_str(
                "[General]\n\
                 TiberiumGrows=yes\n\
                 TiberiumSpreads=yes\n\
                 [InfantryTypes]\n\
                 [VehicleTypes]\n\
                 [AircraftTypes]\n\
                 [BuildingTypes]\n\
                 [Warheads]\n\
                 [TerrainTypes]\n0=TREE01\n\
                 [TREE01]\nFixtureOnly=1\n\
                 [OverlayTypes]\n\
                 0=ORE\n\
                 [ORE]\n\
                 Tiberium=yes\n\
                 [Tiberiums]\n\
                 0=Riparius\n\
                 [Riparius]\n\
                 Image=1\n\
                 Growth=17\n\
                 GrowthPercentage=1\n\
                 Spread=23\n\
                 SpreadPercentage=1\n",
            );
            let rules = RuleSet::from_ini(&ini).expect("post-load tiberium fixture rules");
            let registry = OverlayTypeRegistry::from_ini(&ini, None);
            let ore_id = registry.id_for_name("ORE").expect("fixture ORE id");
            (rules, registry, ore_id)
        }

        fn base_sim(ore_id: u8, cells: &[(u16, u16)]) -> Simulation {
            base_sim_sized(ore_id, 8, cells)
        }

        fn base_sim_sized(ore_id: u8, size: u16, cells: &[(u16, u16)]) -> Simulation {
            let mut sim = Simulation::with_seed(0x17_04);
            sim.session.set_binary_frame_for_test(91);
            sim.session.set_map_width_for_test(size);
            sim.session.set_map_height_for_test(size);
            sim.ore_growth_config = OreGrowthConfig::for_test(true, true, false);
            sim.ore_growth_state = OreGrowthState::new(size, size);
            let mut overlays = OverlayGrid::new(size, size);
            for &(rx, ry) in cells {
                overlays.place_overlay(rx, ry, ore_id, 5);
            }
            sim.overlay_grid = Some(overlays);
            sim.install_resolved_terrain_for_new_map(flat_terrain(size, size));
            sim
        }

        fn map_header(width: u32, height: u32) -> MapHeader {
            MapHeader {
                theater: "TEMPERATE".to_string(),
                fill: "Clear".to_string(),
                level: 0,
                width,
                height,
                local_left: 0,
                local_top: 0,
                local_width: width,
                local_height: height,
            }
        }

        fn add_marked_unit(sim: &mut Simulation, cell: (u16, u16), on_bridge: bool) {
            let stable_id = sim.substrate.allocate_stable_id();
            let owner = sim.interner.intern("Americans");
            let type_ref = sim.interner.intern("TESTUNIT");
            let mut entity = GameEntity::new_at_frame_zero_for_test(
                stable_id,
                cell.0,
                cell.1,
                0,
                0,
                owner,
                Health::new(100),
                type_ref,
                EntityCategory::Unit,
                0,
                5,
                true,
            );
            entity.lifecycle.set_in_limbo_for_test(false);
            entity.in_logic_vector.set_for_test(true);
            entity.set_on_bridge(on_bridge);
            sim.substrate.entities.insert(entity);
            sim.add_entity_occupancy(stable_id);
            sim.substrate
                .logic
                .try_push(stable_id)
                .expect("fixture LogicClass registration");
        }

        fn add_live_terrain_object(sim: &mut Simulation, cell: (u16, u16)) {
            let stable_id = sim.substrate.allocate_stable_id();
            let type_ref = sim.interner.intern("TREE01");
            sim.terrain_objects
                .objects_mut_for_test()
                .insert(stable_id, {
                    let mut terrain =
                        TerrainObjectState::for_test(stable_id, type_ref, cell.0, cell.1);
                    terrain.set_occupation_bits_for_test(7);
                    terrain
                });
            sim.terrain_objects
                .cell_index_mut_for_test()
                .insert(cell, stable_id);
        }

        fn seed_serialized_queue_state(
            sim: &mut Simulation,
            rules: &RuleSet,
            registry: &OverlayTypeRegistry,
            ore_cell: (u16, u16),
        ) {
            sim.ore_growth_state
                .reset_native_tiberium_classes(rules.tiberium_types.len(), 7);
            let overlay = sim.overlay_grid.as_ref().expect("fixture overlay grid");
            let resolved = sim.resolved_terrain.as_ref();
            let mut throwaway_rng = SimRng::new(13);
            assert!(
                sim.ore_growth_state
                    .add_native_growth_queue_cell(
                        overlay,
                        registry,
                        &rules.tiberium_types,
                        resolved,
                        true,
                        registry
                            .tiberium_type_for_overlay(
                                &rules.tiberium_types,
                                overlay.cell(ore_cell.0, ore_cell.1).overlay_id.unwrap()
                            )
                            .unwrap(),
                        ore_cell.0,
                        ore_cell.1,
                        41,
                        &mut throwaway_rng,
                    )
                    .is_some()
            );
            assert!(
                sim.ore_growth_state
                    .add_native_spread_queue_cell(
                        registry
                            .tiberium_type_for_overlay(
                                &rules.tiberium_types,
                                overlay.cell(ore_cell.0, ore_cell.1).overlay_id.unwrap()
                            )
                            .unwrap(),
                        overlay,
                        registry,
                        &rules.tiberium_types,
                        resolved,
                        &std::collections::BTreeSet::new(),
                        None,
                        ore_cell.0,
                        ore_cell.1,
                        41,
                        true,
                        &mut throwaway_rng,
                    )
                    .is_some()
            );
        }

        fn restore_before_map_authority(sim: &Simulation) -> Simulation {
            let resolved = sim
                .resolved_terrain
                .clone()
                .expect("fixture resolved terrain cache");
            let bytes = GameSnapshot::save(sim, 0, 0, "gsi_17_04", 0);
            let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
            restored
                .restore_after_snapshot_load()
                .expect("stable references and substrate re-register");
            restored.rebuild_caches_after_load(
                resolved,
                Default::default(),
                &crate::sim::runtime::SimResources::empty().rules,
            );
            restored
        }

        #[test]
        fn gsi_17_04_postload_rebuild_discards_saved_queues_and_uses_ground_object_list() {
            let (rules, registry, ore_id) = tiberium_fixture();
            let mut sim = base_sim(ore_id, &[(4, 5), (5, 5), (6, 5), (7, 5)]);
            add_marked_unit(&mut sim, (5, 5), false);
            add_marked_unit(&mut sim, (6, 5), true);
            add_live_terrain_object(&mut sim, (7, 5));
            seed_serialized_queue_state(&mut sim, &rules, &registry, (4, 5));

            let mut restored = restore_before_map_authority(&sim);
            let saved_class = &restored.ore_growth_state.native_tiberium_state().classes[0];
            assert_eq!(saved_class.growth_timer.start_frame(), 7);
            assert_ne!(
                saved_class.growth.heap_entry(0).unwrap().priority_bits,
                0.0f32.to_bits()
            );
            assert_eq!(
                restored
                    .substrate
                    .occupancy
                    .count_on_layer(5, 5, MovementLayer::Ground),
                1
            );
            assert_eq!(
                restored
                    .substrate
                    .occupancy
                    .count_on_layer(6, 5, MovementLayer::Bridge),
                1
            );

            let rng_before = restored.rng_state();
            let restore_output = restored
                .restore_map_authority_after_snapshot_load(&rules, &registry, None)
                .expect("all map-authority rebuild dependencies");
            assert_eq!(
                restored.rng_state(),
                rng_before,
                "load rebuild draws no RNG"
            );
            assert_eq!(restore_output.native_tiberium_stats.growth_entries, 4);
            assert_eq!(restore_output.native_tiberium_stats.spread_entries, 2);

            let class = &restored.ore_growth_state.native_tiberium_state().classes[0];
            assert_eq!(
                (
                    class.growth_timer.start_frame(),
                    class.growth_timer.duration()
                ),
                (91, 0)
            );
            assert_eq!(
                (
                    class.spread_timer.start_frame(),
                    class.spread_timer.duration()
                ),
                (91, 0)
            );
            assert!(
                class
                    .growth
                    .iter_heap()
                    .all(|entry| entry.priority_bits == 0.0f32.to_bits())
            );
            assert!(
                class
                    .spread
                    .iter_heap()
                    .all(|entry| entry.priority_bits == 0.0f32.to_bits())
            );
            let growth_cells: BTreeSet<_> = class
                .growth
                .iter_heap()
                .map(|entry| (entry.rx, entry.ry))
                .collect();
            let spread_cells: BTreeSet<_> = class
                .spread
                .iter_heap()
                .map(|entry| (entry.rx, entry.ry))
                .collect();
            assert_eq!(
                growth_cells,
                BTreeSet::from([(4, 5), (5, 5), (6, 5), (7, 5)])
            );
            assert_eq!(spread_cells, BTreeSet::from([(4, 5), (6, 5)]));
            assert_eq!(class.growth_bitmap, growth_cells);
            assert_eq!(class.spread_bitmap, spread_cells);
        }

        /// The post-load rebuild walks the serialized native `MapRect`
        /// (`0x0087F8DC` / `0x0087F8E0`), never the cell-array extent in
        /// `session.map_*`. With 12x12 storage over an 8x8 `[Map] Size`,
        /// (5, 5) lies on anti-diagonal 10: inside the (8, 8) walk (first
        /// diagonal 9), outside a (12, 12) walk (first diagonal 13).
        #[test]
        fn gsi_17_04_postload_rebuild_walks_the_serialized_native_rect() {
            let (rules, registry, ore_id) = tiberium_fixture();
            let mut sim = base_sim_sized(ore_id, 12, &[(5, 5), (7, 7)]);
            sim.install_playfield_from_map_header(&map_header(8, 8));
            {
                let overlay = sim.overlay_grid.as_ref().expect("fixture overlay grid");
                let fresh = sim
                    .ore_growth_state
                    .rebuild_native_tiberium_queues_from_overlays(
                        overlay,
                        &registry,
                        &rules.tiberium_types,
                        sim.resolved_terrain.as_ref(),
                        &BTreeSet::new(),
                        true,
                        true,
                        7,
                        (8, 8),
                    );
                assert_eq!(fresh.growth_entries, 2);
            }
            assert_eq!(sim.ore_growth_state.native_rect(), (8, 8));

            let mut restored = restore_before_map_authority(&sim);
            assert_eq!(
                restored.ore_growth_state.native_rect(),
                (8, 8),
                "the rect survives the snapshot"
            );
            assert_eq!(restored.session.map_width(), 12);
            let output = restored
                .restore_map_authority_after_snapshot_load(&rules, &registry, None)
                .expect("all map-authority rebuild dependencies");
            assert_eq!(output.native_tiberium_stats.growth_entries, 2);
            assert_eq!(output.native_tiberium_stats.spread_entries, 2);
            let class = &restored.ore_growth_state.native_tiberium_state().classes[0];
            assert_eq!(class.growth_bitmap, BTreeSet::from([(5, 5), (7, 7)]));
            assert_eq!(
                class.growth.capacity(),
                crate::sim::ore_growth::native_tiberium_queue_capacity((8, 8))
            );
        }

        /// A snapshot written before the rect was serialized deserializes it
        /// as `(0, 0)`; the rebuild then falls back to the retained MapClass
        /// `Size` (`+0xF4` width plus the separately retained height), never
        /// the cell-array extent.
        #[test]
        fn gsi_17_04_postload_rebuild_zero_rect_falls_back_to_map_size() {
            let (rules, registry, ore_id) = tiberium_fixture();
            let mut sim = base_sim_sized(ore_id, 12, &[(5, 5), (7, 7)]);
            sim.install_playfield_from_map_header(&map_header(8, 8));
            sim.ore_growth_state.set_native_rect_for_tests((0, 0));

            let mut restored = restore_before_map_authority(&sim);
            assert_eq!(restored.ore_growth_state.native_rect(), (0, 0));
            let output = restored
                .restore_map_authority_after_snapshot_load(&rules, &registry, None)
                .expect("all map-authority rebuild dependencies");
            assert_eq!(output.native_tiberium_stats.growth_entries, 2);
            assert_eq!(restored.ore_growth_state.native_rect(), (8, 8));
        }

        /// Fresh-load `InitGrowthQueues_All @ 0x00722D00` /
        /// `InitSpreadQueues_All @ 0x00722240` read `Cell+0xE4 FirstObject`,
        /// which `TerrainClass::Mark @ 0x0071BFF8` links for every terrain
        /// object, spawning or not. A plain tree over ore keeps the cell out
        /// of the spread store while `CanGrowTiberium @ 0x00483620` still
        /// admits it to growth.
        #[test]
        fn oq_38_fresh_load_queue_init_counts_plain_terrain_objects_as_first_object() {
            let (rules, registry, ore_id) = tiberium_fixture();
            let mut sim = base_sim(ore_id, &[(4, 5), (5, 5)]);
            add_live_terrain_object(&mut sim, (5, 5));
            assert!(
                !sim.terrain_objects.spawning_cells().contains(&(5, 5)),
                "the tree is not an ore spawner"
            );

            let live_grid = sim.overlay_grid.take();
            let stats = crate::sim::runtime::initialize_native_tiberium_queues(
                &mut sim,
                &BasicSection::default(),
                &SpecialFlagsSection::default(),
                &rules,
                &registry,
                live_grid.as_ref(),
                (8, 8),
            )
            .expect("overlay grid present");
            sim.overlay_grid = live_grid;

            assert_eq!(stats.growth_entries, 2);
            assert_eq!(stats.spread_entries, 1);
            let class = &sim.ore_growth_state.native_tiberium_state().classes[0];
            assert_eq!(class.growth_bitmap, BTreeSet::from([(4, 5), (5, 5)]));
            assert_eq!(class.spread_bitmap, BTreeSet::from([(4, 5)]));
        }

        #[test]
        fn gsi_17_04_first_resumed_pass_reloads_type_growth_and_spread_intervals() {
            let (rules, registry, ore_id) = tiberium_fixture();
            let mut sim = base_sim(ore_id, &[(4, 5)]);
            seed_serialized_queue_state(&mut sim, &rules, &registry, (4, 5));
            let mut restored = restore_before_map_authority(&sim);
            restored
                .restore_map_authority_after_snapshot_load(&rules, &registry, None)
                .expect("all map-authority rebuild dependencies");
            restored.resolve_type_handles(&rules);

            let timer_before =
                restored.ore_growth_state.native_tiberium_state().classes[0].growth_timer;
            assert_eq!(
                (timer_before.start_frame(), timer_before.duration()),
                (91, 0)
            );
            restored.advance_tick(&[], Some(&rules), None, Some(&registry), 67);

            let class = &restored.ore_growth_state.native_tiberium_state().classes[0];
            assert_eq!(
                (
                    class.growth_timer.start_frame(),
                    class.growth_timer.duration()
                ),
                (91, 17)
            );
            assert_eq!(
                (
                    class.spread_timer.start_frame(),
                    class.spread_timer.duration()
                ),
                (91, 23)
            );
            assert_eq!(restored.session.binary_frame(), 92);
        }

        #[test]
        fn gsi_17_04_missing_dependency_rejects_postload_queue_admission() {
            let (rules, registry, ore_id) = tiberium_fixture();
            let mut sim = base_sim(ore_id, &[(4, 5)]);
            sim.ore_growth_state
                .reset_native_tiberium_classes(rules.tiberium_types.len(), 7);

            let overlays = sim.overlay_grid.take();
            assert!(matches!(
                sim.restore_map_authority_after_snapshot_load(&rules, &registry, None),
                Err(SnapshotRestoreError::MissingMapAuthorityComponent {
                    component: "OverlayGrid",
                })
            ));
            sim.overlay_grid = overlays;
            sim.resolved_terrain = None;
            assert!(matches!(
                sim.restore_map_authority_after_snapshot_load(&rules, &registry, None),
                Err(SnapshotRestoreError::MissingMapAuthorityComponent {
                    component: "ResolvedTerrainGrid",
                })
            ));
            let class = &sim.ore_growth_state.native_tiberium_state().classes[0];
            assert_eq!(
                (
                    class.growth_timer.start_frame(),
                    class.growth_timer.duration()
                ),
                (7, 0)
            );
        }
    }

    fn rebuild_load_caches(sim: &mut Simulation, terrain: ResolvedTerrainGrid) {
        // Synthetic fixtures use unchecked snapshots and often bypass the
        // monotonic allocators. Rebuild their substrate caches directly; the
        // production path uses `restore_after_snapshot_load`.
        sim.substrate.entities.rebuild_class_registries();
        sim.rebuild_logic_membership();
        sim.substrate
            .occupancy
            .restore_memberships(&sim.substrate.entities)
            .expect("fixture has valid saved Cell references");
        sim.rebuild_caches_after_load(
            terrain,
            crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            &crate::sim::runtime::SimResources::empty().rules,
        );
    }

    #[test]
    fn snapshot_restore_replays_overlay_passability_and_publishes_canonical_navigation() {
        use crate::map::overlay::OverlayEntry;
        use crate::rules::ini_parser::IniFile;
        use crate::rules::overlay_types::OverlayTypeRegistry;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::overlay_grid::{OverlayGrid, recalc_overlay_passability};
        use crate::sim::pathfinding::zone_map::ZONE_INVALID;

        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [OverlayTypes]\n0=WALL\n\
             [WALL]\nWall=yes\nStrength=100\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("snapshot overlay rules");
        let registry = OverlayTypeRegistry::from_ini(&ini, None);

        let cleared_since_map_load = (0, 0);
        let runtime_wall = (2, 0);
        let mut map_terrain = flat_terrain(3, 1);
        let mut map_overlays = OverlayGrid::from_overlay_entries(
            &[OverlayEntry {
                rx: cleared_since_map_load.0,
                ry: cleared_since_map_load.1,
                overlay_id: 0,
                frame: 0,
            }],
            3,
            1,
        );
        assert!(recalc_overlay_passability(
            &mut map_overlays,
            &mut map_terrain,
            &registry,
            cleared_since_map_load.0,
            cleared_since_map_load.1,
        ));
        assert!(
            map_terrain
                .cell(cleared_since_map_load.0, cleared_since_map_load.1)
                .expect("map wall terrain")
                .overlay_blocks
        );

        let mut sim = Simulation::new();
        let runtime_overlays = OverlayGrid::from_overlay_entries(
            &[OverlayEntry {
                rx: runtime_wall.0,
                ry: runtime_wall.1,
                overlay_id: 0,
                frame: 0x21,
            }],
            3,
            1,
        );
        // This fixture places a live wall but is not a blocker-count subject:
        // its plane stays zero. A real wall always carries its
        // `OverlayClass::Mark` increments.
        sim.overlay_grid = Some(runtime_overlays);
        sim.install_resolved_terrain_for_new_map(map_terrain.clone());
        sim.restore_map_authority_after_snapshot_load(&rules, &registry, None)
            .expect("publish the live runtime overlay navigation before saving");
        // Original Scenario full-load resets this stream to Seed(0). Match
        // that retained load policy when comparing the published live fixture.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let authoritative_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "overlay_restore.map", 0);
        let mut restored = GameSnapshot::load(&bytes)
            .expect("snapshot with runtime overlay authority")
            .sim;
        restored
            .restore_after_snapshot_load()
            .expect("stable snapshot identity");
        assert!(
            restored
                .zone_grid
                .as_ref()
                .unwrap()
                .is_native_load_pending()
        );
        restored.rebuild_caches_after_load(
            map_terrain,
            crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            &rules,
        );

        let restore_output = restored
            .restore_map_authority_after_snapshot_load(&rules, &registry, None)
            .expect("restored overlay and navigation authority");
        let terrain = restored
            .resolved_terrain
            .as_ref()
            .expect("restored terrain");
        assert!(!terrain.cell(0, 0).expect("cleared cell").overlay_blocks);
        assert!(
            terrain
                .cell(2, 0)
                .expect("runtime wall cell")
                .overlay_blocks
        );
        let path = restored.path_grid().expect("canonical restored path grid");
        assert!(path.is_walkable(0, 0));
        assert!(!path.is_walkable(2, 0));
        let normal_zones = restored
            .zone_grid
            .as_ref()
            .and_then(|zones| zones.map_for(MovementZone::Normal))
            .expect("canonical restored ground zones");
        assert_ne!(
            normal_zones.zone_at(0, 0, MovementLayer::Ground),
            ZONE_INVALID
        );
        assert_eq!(
            normal_zones.zone_at(2, 0, MovementLayer::Ground),
            ZONE_INVALID
        );
        assert_eq!(restore_output.occupied_overlays.len(), 1);
        assert_eq!(
            (
                restore_output.occupied_overlays[0].rx,
                restore_output.occupied_overlays[0].ry,
                restore_output.occupied_overlays[0].overlay_id,
                restore_output.occupied_overlays[0].frame,
            ),
            (2, 0, 0, 0x21)
        );
        assert_eq!(restore_output.native_tiberium_stats, Default::default());
        let saved_authority = save_codec::serialize(&sim).unwrap();
        let loaded_authority = save_codec::serialize(&restored).unwrap();
        assert!(
            saved_authority == loaded_authority,
            "overlay authority differs at byte {:?}; lengths {} vs {}",
            saved_authority
                .iter()
                .zip(&loaded_authority)
                .position(|(a, b)| a != b),
            saved_authority.len(),
            loaded_authority.len()
        );
        assert_zone_grids_equivalent(
            sim.zone_grid.as_ref().unwrap(),
            restored.zone_grid.as_ref().unwrap(),
        );
        assert_eq!(restored.state_hash(), authoritative_hash);
    }

    fn terrain_asset_restore_fixture() -> (
        Simulation,
        crate::rules::ruleset::RuleSet,
        crate::rules::overlay_types::OverlayTypeRegistry,
    ) {
        let ini = crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [OverlayTypes]\n0=WALL\n[WALL]\nWall=yes\nStrength=100\n",
        );
        let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).unwrap();
        let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
        let mut sim = Simulation::new();
        sim.set_playfield_bounds_for_test(Some(crate::sim::cell_rect::PlayfieldBounds {
            base: 2,
            off_fc: -8,
            off_100: -8,
            off_104: 16,
            off_108: 16,
        }));
        sim.set_playfield_size_height_for_test(Some(2));
        sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(4, 4));
        sim.overlay_grid.as_mut().unwrap().place_overlay(2, 2, 0, 0);
        sim.install_resolved_terrain_for_new_map(flat_terrain(4, 4));
        let mut replaced = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(1, 2)
            .unwrap()
            .clone();
        replaced.final_tile_index = 71;
        sim.dynamic_terrain_cells.record(&replaced);
        sim.real_cell_bridge_flags_0x1180.set_allocated_cell(
            2 * 4 + 1,
            crate::map::bridge_facts::BRIDGE_FLAG_DIRECTION_ZERO,
        );
        (sim, rules, registry)
    }

    #[test]
    fn snapshot_restore_prepares_live_terrain_assets_before_map_publication() {
        let (mut sim, rules, registry) = terrain_asset_restore_fixture();
        let rng_before = sim.rng_state();
        let mut expected_main = sim.main_rng.snapshot_for_test();
        let expected_word = expected_main.next_u32();
        let mut calls = 0;
        let mut prepare = |terrain: &mut ResolvedTerrainGrid,
                           main: crate::sim::rng::MainRngDraws,
                           size_width: i32| {
            calls += 1;
            assert_eq!(
                size_width, 2,
                "the native iterator uses Size, not grid width"
            );
            assert_eq!(terrain.cell(1, 2).unwrap().final_tile_index, 71);
            assert_eq!(terrain.cell(1, 2).unwrap().bridge_facts.raw_flags, 0);
            assert!(!terrain.cell(2, 2).unwrap().overlay_blocks);
            assert_eq!(main.next_u32(), expected_word);
            Ok(())
        };

        sim.restore_map_authority_after_snapshot_load(&rules, &registry, Some(&mut prepare))
            .expect("prepare assets from dynamic cells before final map publication");

        assert_eq!(calls, 1);
        assert_eq!(sim.main_rng.logical_state(), expected_main.logical_state());
        assert_eq!(sim.rng_state().scenario, rng_before.scenario);
        assert_eq!(sim.rng_state().mapgen, rng_before.mapgen);
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        assert_eq!(
            terrain.cell(1, 2).unwrap().bridge_facts.raw_flags,
            crate::map::bridge_facts::BRIDGE_FLAG_DIRECTION_ZERO,
        );
        assert!(terrain.cell(2, 2).unwrap().overlay_blocks);
        assert!(!sim.path_grid().unwrap().is_walkable(2, 2));
    }

    #[test]
    fn snapshot_restore_terrain_asset_error_stops_map_publication() {
        let (mut sim, rules, registry) = terrain_asset_restore_fixture();
        let mut expected_main = sim.main_rng.snapshot_for_test();
        let expected_word = expected_main.next_u32();
        let mut prepare =
            |terrain: &mut ResolvedTerrainGrid, main: crate::sim::rng::MainRngDraws, _: i32| {
                assert_eq!(terrain.cell(1, 2).unwrap().final_tile_index, 71);
                assert_eq!(main.next_u32(), expected_word);
                Err("missing selected TMP".to_string())
            };

        let error = sim
            .restore_map_authority_after_snapshot_load(&rules, &registry, Some(&mut prepare))
            .expect_err("a rejected candidate must not publish map authority");

        assert!(matches!(
            error,
            SnapshotRestoreError::TerrainAssetPreparationFailed { reason }
                if reason == "missing selected TMP"
        ));
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        assert_eq!(terrain.cell(1, 2).unwrap().bridge_facts.raw_flags, 0);
        assert!(!terrain.cell(2, 2).unwrap().overlay_blocks);
        assert!(sim.path_grid().is_none());
        assert_eq!(sim.main_rng.logical_state(), expected_main.logical_state());
    }

    #[test]
    fn snapshot_restore_rejects_truncated_overlay_cell_storage() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::overlay_grid::{OverlayCell, OverlayGrid};

        #[derive(serde::Serialize)]
        struct OverlayGridWire {
            width: u16,
            height: u16,
            cells: Vec<OverlayCell>,
            retained_neighbor_counts: Vec<u8>,
        }

        let malformed_bytes = save_codec::serialize(&OverlayGridWire {
            width: 2,
            height: 1,
            cells: vec![OverlayCell::default()],
            retained_neighbor_counts: vec![0; 2],
        })
        .expect("malformed overlay wire fixture");
        let malformed: OverlayGrid =
            save_codec::deserialize(&malformed_bytes).expect("wire-compatible OverlayGrid");
        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [OverlayTypes]\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("truncated-grid rules");
        let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
        let mut sim = Simulation::new();
        sim.overlay_grid = Some(malformed);
        sim.resolved_terrain = Some(flat_terrain(2, 1));

        assert!(matches!(
            sim.restore_map_authority_after_snapshot_load(&rules, &registry, None),
            Err(SnapshotRestoreError::MapAuthorityCellStorageMismatch {
                expected: 2,
                found: 1,
            })
        ));
    }

    #[test]
    fn snapshot_restore_rejects_truncated_retained_wall_neighbor_storage() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::overlay_grid::{OverlayCell, OverlayGrid};

        #[derive(serde::Serialize)]
        struct OverlayGridWire {
            width: u16,
            height: u16,
            cells: Vec<OverlayCell>,
            retained_neighbor_counts: Vec<u8>,
        }

        let malformed_bytes = save_codec::serialize(&OverlayGridWire {
            width: 2,
            height: 1,
            cells: vec![OverlayCell::default(); 2],
            retained_neighbor_counts: vec![7],
        })
        .expect("malformed retained wall-neighbor wire fixture");
        let malformed: OverlayGrid =
            save_codec::deserialize(&malformed_bytes).expect("wire-compatible OverlayGrid");
        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [OverlayTypes]\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("truncated-retained-grid rules");
        let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
        let mut sim = Simulation::new();
        sim.overlay_grid = Some(malformed);
        sim.resolved_terrain = Some(flat_terrain(2, 1));

        assert!(matches!(
            sim.restore_map_authority_after_snapshot_load(&rules, &registry, None),
            Err(SnapshotRestoreError::RetainedWallNeighborStorageMismatch {
                expected: 2,
                found: 1,
            })
        ));
    }

    fn cell_order(sim: &Simulation, rx: u16, ry: u16, layer: MovementLayer) -> Vec<u64> {
        sim.substrate
            .occupancy
            .get(rx, ry)
            .map(|occ| occ.iter_layer(layer).map(|o| o.entity_id).collect())
            .unwrap_or_default()
    }

    fn assert_zone_grids_equivalent(a: &ZoneGrid, b: &ZoneGrid) {
        assert_eq!(a.width(), b.width());
        assert_eq!(a.height(), b.height());
        for &mz in MovementZone::all_ground() {
            let map_a = a.map_for(mz).expect("zone map exists for movement zone");
            let map_b = b.map_for(mz).expect("zone map exists for movement zone");
            let hierarchy_a = a.hierarchy_for(mz).expect("native hierarchy");
            let hierarchy_b = b.hierarchy_for(mz).expect("restored native hierarchy");
            for level in 0..3 {
                let graph_a = hierarchy_a.level(level).unwrap();
                let graph_b = hierarchy_b.level(level).unwrap();
                assert_eq!(graph_a.record_slot_count(), graph_b.record_slot_count());
                for slot in 0..graph_a.record_slot_count() {
                    let zone = u16::try_from(slot).unwrap();
                    // Record equality includes the retained native+20 threat
                    // seed; equal connectivity alone does not prove its input.
                    assert_eq!(graph_a.record(zone), graph_b.record(zone));
                    assert_eq!(graph_a.edges(zone), graph_b.edges(zone));
                }
            }
            assert_eq!(map_a.zone_count(), map_b.zone_count());
            for y in 0..a.height() {
                for x in 0..a.width() {
                    assert_eq!(
                        map_a.zone_at(x, y, MovementLayer::Ground),
                        map_b.zone_at(x, y, MovementLayer::Ground),
                        "ground zone mismatch for {mz:?} at ({x},{y})"
                    );
                    assert_eq!(
                        map_a.zone_at(x, y, MovementLayer::Bridge),
                        map_b.zone_at(x, y, MovementLayer::Bridge),
                        "bridge zone mismatch for {mz:?} at ({x},{y})"
                    );
                }
            }
        }
    }

    /// Prove snapshot round-trip preserves all authoritative state.
    ///
    /// 1. Create a Simulation, advance N ticks
    /// 2. Save snapshot -> bytes -> load snapshot
    /// 3. Advance both the loaded sim and a reference sim for M more ticks
    /// 4. Assert both reach the same state hash
    #[test]
    fn round_trip_preserves_state_hash() {
        // Create two identical simulations from the same seed.
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();

        // Advance both for 50 ticks to build up some state.
        for _ in 0..50 {
            tick(&mut sim_a);
            tick(&mut sim_b);
        }

        // Native in-scenario load restarts Scenario RNG from Seed0. Put both
        // source/reference branches on that cursor before testing unrelated
        // snapshot persistence and continued deterministic execution.
        sim_a.scenario_rng = crate::sim::rng::SimRng::new(0);
        sim_b.scenario_rng = crate::sim::rng::SimRng::new(0);

        // Snapshot sim_a at tick 50.
        let hash_at_50 = sim_a.state_hash();
        let bytes = GameSnapshot::save(&sim_a, 0, 0, "test_map", 0);

        // Load the snapshot.
        let snapshot = GameSnapshot::load(&bytes).expect("load should succeed");
        let mut sim_loaded = snapshot.sim;

        // Verify the loaded sim has the same state hash as the original at tick 50.
        assert_eq!(
            sim_loaded.state_hash(),
            hash_at_50,
            "loaded snapshot must match original state hash at save point"
        );

        // Advance both the original and loaded sims for 50 more ticks.
        for _ in 0..50 {
            tick(&mut sim_a);
            tick(&mut sim_loaded);
        }

        // Both must reach the same state hash at tick 100.
        assert_eq!(
            sim_a.state_hash(),
            sim_loaded.state_hash(),
            "original and loaded sim must reach identical state after continued ticking"
        );

        // The reference sim (never serialized) must also match.
        for _ in 0..50 {
            tick(&mut sim_b);
        }
        assert_eq!(
            sim_a.state_hash(),
            sim_b.state_hash(),
            "reference sim (never serialized) must match serialized sim"
        );
    }

    #[test]
    fn current_header_with_missing_body_reports_deserialization_failure() {
        let bytes = save_codec::serialize(&GameSnapshotHeader {
            product_magic: SNAPSHOT_PRODUCT_MAGIC,
            envelope_version: SNAPSHOT_ENVELOPE_VERSION,
            layout: snapshot_layout(),
            description: "current fixture".to_string(),
            map_hash: 1,
            rules_hash: 2,
            tick: 3,
            save_timestamp: 4,
            map_name: "current-layout".to_string(),
            compatibility: RulesCompatibility::default(),
        })
        .expect("serialize current header only");

        assert!(matches!(
            GameSnapshot::load(&bytes),
            Err(SnapshotError::DeserializeFailed(_))
        ));
    }

    #[test]
    fn gsi_17_02_header_roundtrip_carries_identity_versions_and_description() {
        let mut sim = Simulation::new();
        sim.session
            .set_map_name_for_test("OFFICIAL.MAP".to_string());
        let bytes =
            GameSnapshot::save_validated(&sim, 0x1234, 0x5678, "Hold the northern ridge", 0x9abc);

        let header = GameSnapshot::read_header(&bytes).expect("current VERA header");
        assert_eq!(header.product_magic, SNAPSHOT_PRODUCT_MAGIC);
        assert_eq!(header.envelope_version, SNAPSHOT_ENVELOPE_VERSION);
        assert_eq!(header.layout, snapshot_layout());
        assert_eq!(header.compatibility, RulesCompatibility::default());
        assert_eq!(header.description, "Hold the northern ridge");
        assert_eq!(header.map_name, "OFFICIAL.MAP");
    }

    #[test]
    fn snapshot_capability_rejects_foreign_mode_before_body_decode() {
        let bytes = GameSnapshot::save_validated(&Simulation::new(), 0, 0, "synthetic", 0);
        let mut header = GameSnapshot::read_header(&bytes).unwrap();
        // Flip every capability bit: the result is a foreign mode for any build.
        header.compatibility = RulesCompatibility::from_capabilities(
            !crate::rules::ARES_ENABLED,
            !crate::rules::PHOBOS_ENABLED,
            !crate::rules::NPATCH_ENABLED,
        );
        // A complete header with no body proves capability admission runs before
        // Simulation deserialization, even without bound scenario resources.
        let bytes = save_codec::serialize(&header).unwrap();
        for result in [
            GameSnapshot::load(&bytes),
            GameSnapshot::load_with_resources(&bytes),
        ] {
            assert!(matches!(
                result,
                Err(SnapshotError::CapabilityMismatch { expected, found })
                    if expected == RulesCompatibility::default() && found == header.compatibility
            ));
        }
        let metadata = GameSnapshot::read_metadata(&bytes).unwrap();
        assert_eq!(metadata.compatibility, header.compatibility);
        assert!(!metadata.is_compatible());
    }

    #[test]
    fn snapshot_capability_keeps_legacy_metadata_readable_without_admitting_it() {
        let bytes = save_codec::serialize(&(
            SNAPSHOT_PRODUCT_MAGIC,
            SNAPSHOT_ENVELOPE_VERSION,
            457_u32,
            "older layout",
            1_u64,
            2_u64,
            3_u64,
            4_u64,
            "OLD.MAP",
        ))
        .unwrap();
        let header = GameSnapshot::read_metadata(&bytes).unwrap();
        assert_eq!(header.layout, 457);
        assert_eq!(header.description, "older layout");
        assert_eq!(header.map_name, "OLD.MAP");
        assert!(!header.is_compatible());
        assert!(matches!(
            GameSnapshot::read_header(&bytes),
            Err(SnapshotError::LayoutMismatch { found: 457, .. })
        ));
    }

    #[test]
    fn gsi_17_02_wrong_product_is_rejected_before_missing_body_decode() {
        let foreign_preamble = GameSnapshotPreamble {
            product_magic: *b"NOTVERA\0",
            envelope_version: SNAPSHOT_ENVELOPE_VERSION,
            layout: snapshot_layout(),
        };
        let preamble_only = save_codec::serialize(&foreign_preamble).expect("foreign preamble");

        assert!(matches!(
            GameSnapshot::load(&preamble_only),
            Err(SnapshotError::ProductMismatch { found }) if found == *b"NOTVERA\0"
        ));
    }

    #[test]
    fn gsi_17_02_public_and_internal_versions_gate_before_missing_body_decode() {
        let wrong_public = GameSnapshotPreamble {
            product_magic: SNAPSHOT_PRODUCT_MAGIC,
            envelope_version: SNAPSHOT_ENVELOPE_VERSION + 1,
            layout: snapshot_layout(),
        };
        let preamble_only = save_codec::serialize(&wrong_public).expect("public-version preamble");
        assert!(matches!(
            GameSnapshot::load(&preamble_only),
            Err(SnapshotError::EnvelopeVersionMismatch {
                expected: SNAPSHOT_ENVELOPE_VERSION,
                found: 2,
            })
        ));

        let wrong_internal = GameSnapshotPreamble {
            product_magic: SNAPSHOT_PRODUCT_MAGIC,
            envelope_version: SNAPSHOT_ENVELOPE_VERSION,
            layout: snapshot_layout() ^ 1,
        };
        let preamble_only =
            save_codec::serialize(&wrong_internal).expect("schema-version preamble");
        assert!(matches!(
            GameSnapshot::load(&preamble_only),
            Err(SnapshotError::LayoutMismatch {
                expected,
                found,
            }) if expected == snapshot_layout() && found == snapshot_layout() ^ 1
        ));
    }

    #[test]
    fn signed_actual_and_estimated_health_survive_save_restore_independently() {
        for (actual, estimated) in [(-7, 100_000), (100_000, -19), (i32::MIN, i32::MAX)] {
            let mut sim = Simulation::new();
            assert_eq!(sim.substrate.allocate_stable_id(), 1);
            let mut entity =
                crate::sim::game_entity::GameEntity::test_default(1, "SIGNED", "Americans", 0, 0);
            entity.set_type_ref_for_test(sim.intern("SIGNED"));
            entity.set_owner_for_test(sim.intern("Americans"));
            entity.health.set_for_test(actual);
            entity.estimated_health =
                crate::sim::estimated_health::EstimatedHealth::from_raw(estimated);
            sim.substrate.entities.insert(entity);
            // Native load resets Scenario RNG to seed zero; isolate the health
            // roundtrip from that intentional load transition.
            sim.scenario_rng = crate::sim::rng::SimRng::new(0);
            let before = sim.state_hash();
            let bytes = GameSnapshot::save(&sim, 0, 0, "signed-health", 0);
            let mut restored = GameSnapshot::load(&bytes)
                .expect("signed health schema")
                .sim;
            restored
                .restore_after_snapshot_load()
                .expect("restore signed health");
            let entity = restored.substrate.entities.get(1).unwrap();
            assert_eq!(
                (entity.health.current(), entity.estimated_health.get()),
                (actual, estimated)
            );
            assert_eq!(restored.state_hash(), before);
            restored
                .substrate
                .entities
                .get_mut(1)
                .unwrap()
                .health
                .set_for_test(actual.wrapping_add(1));
            assert_ne!(restored.state_hash(), before);
            restored
                .substrate
                .entities
                .get_mut(1)
                .unwrap()
                .health
                .set_for_test(actual);
            restored
                .substrate
                .entities
                .get_mut(1)
                .unwrap()
                .estimated_health =
                crate::sim::estimated_health::EstimatedHealth::from_raw(estimated.wrapping_add(1));
            assert_ne!(restored.state_hash(), before);
        }
    }

    #[test]
    fn fractional_passenger_sizes_roundtrip_and_recompute_after_remove() {
        use crate::sim::game_entity::GameEntity;
        use crate::sim::passenger::{PassengerCargo, PassengerRole};

        let oracle: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/cargo_total_size.json",
        ))
        .unwrap();
        let row = oracle["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["name"] == "positive_then_negative")
            .unwrap();
        let size_bits: Vec<u64> = row["input"]["size_f64_bits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|bits| u64::from_str_radix(bits.as_str().unwrap(), 16).unwrap())
            .collect();
        let sizes: Vec<f64> = size_bits.iter().copied().map(f64::from_bits).collect();
        let native_total = row["total_size_i32"].as_i64().unwrap() as i32;

        let mut sim = Simulation::new();
        let carrier_id = sim.substrate.allocate_stable_id();
        let passenger_ids = [
            sim.substrate.allocate_stable_id(),
            sim.substrate.allocate_stable_id(),
        ];
        let mut cargo = PassengerCargo::new(2, 1.25);
        cargo.board_forced_chain(&passenger_ids.into_iter().zip(sizes).collect::<Vec<_>>());
        assert_eq!(cargo.total_size(), native_total);

        let owner = sim.intern("Americans");
        let mut carrier = GameEntity::test_default(carrier_id, "TRAN", "Americans", 0, 0);
        carrier.set_owner_for_test(owner);
        carrier.set_type_ref_for_test(sim.intern("TRAN"));
        carrier.passenger_role = PassengerRole::transport(cargo);
        sim.substrate.entities.insert(carrier);
        for id in passenger_ids {
            let mut passenger = GameEntity::test_default(id, "E1", "Americans", 0, 0);
            passenger.set_owner_for_test(owner);
            passenger.set_type_ref_for_test(sim.intern("E1"));
            passenger
                .passenger_role
                .set_inside_transport(carrier_id, false);
            sim.substrate.entities.insert(passenger);
        }

        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let before = sim.state_hash();
        let bytes = GameSnapshot::save(&sim, 0, 0, "fractional-cargo", 0);
        let mut restored = GameSnapshot::load(&bytes)
            .expect("fractional cargo schema")
            .sim;
        restored
            .restore_after_snapshot_load()
            .expect("fractional cargo references");
        let cargo = restored
            .substrate
            .entities
            .get(carrier_id)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap();
        assert_eq!(cargo.passengers(), passenger_ids.as_slice());
        assert_eq!(
            cargo
                .passenger_sizes()
                .iter()
                .map(|size| size.to_bits())
                .collect::<Vec<_>>(),
            size_bits
        );
        assert_eq!(cargo.size_limit().to_bits(), 1.25_f64.to_bits());
        assert_eq!(cargo.total_size(), native_total);
        assert_eq!(cargo.total_size(), cargo.recomputed_total_size());
        assert_eq!(restored.state_hash(), before);

        let cargo = restored
            .substrate
            .entities
            .get_mut(carrier_id)
            .unwrap()
            .passenger_role
            .cargo_mut()
            .unwrap();
        assert!(cargo.disembark(passenger_ids[1]));
        assert_eq!(cargo.passengers(), &passenger_ids[..1]);
        assert_eq!(cargo.passenger_sizes()[0].to_bits(), size_bits[0]);
        assert_eq!(cargo.total_size(), cargo.recomputed_total_size());
        assert_eq!(cargo.total_size(), 1);
        restored
            .substrate
            .entities
            .get_mut(passenger_ids[1])
            .unwrap()
            .passenger_role
            .clear_carrier_relation();
        assert_ne!(restored.state_hash(), before);
    }

    #[test]
    fn exact_network_mode_timing_and_observer_identity_survive_snapshot() {
        use crate::sim::scenario_session::ScenarioGameMode;
        for mode in [ScenarioGameMode::Lan, ScenarioGameMode::Wol] {
            // Native Scenario load reseeds the RNG to zero. Isolate the
            // persisted session fields from that intentional load-time change.
            let mut sim = Simulation::with_seed(0);
            let owner = sim.intern("Observer");
            sim.houses
                .register(crate::sim::house_state::HouseState::new(
                    owner, 0, None, true, 0, 1,
                ));
            sim.session.set_current_house_for_test(Some(owner));
            sim.session.set_game_mode(mode);
            sim.session.install_network_timing(73);
            sim.session.install_multiplayer_house_identity(
                crate::skirmish_modes::MpModeClass::Battle,
                Some(owner),
            );
            let mut header = crate::map::preview::ScenarioStartHeader::default();
            header.read_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[Header]\nNumCoopHumanStartSpots=2\n",
            ));
            sim.session.install_start_header(header);
            let bytes = GameSnapshot::save(&sim, 0, 0, "network-mode", 0);
            let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
            restored.restore_after_snapshot_load().unwrap();
            assert_eq!(restored.session.game_mode(), mode);
            assert_eq!(restored.session.start_header().coop_human_start_spots(), 2);
            assert_eq!(restored.session.network_max_ahead_frames(), Some(73));
            assert_eq!(restored.session.observer_house(), Some(owner));
            assert_eq!(
                restored.session.multiplayer_mode_class(),
                Some(crate::skirmish_modes::MpModeClass::Battle)
            );
            assert_eq!(restored.state_hash(), sim.state_hash());
        }
    }

    #[test]
    fn current_house_is_saved_client_input_with_validated_reference() {
        let mut sim = Simulation::with_seed(0);
        let mut owners = Vec::new();
        for name in ["First", "Second"] {
            let owner = sim.intern(name);
            sim.houses
                .register(crate::sim::house_state::HouseState::new(
                    owner, 0, None, true, 0, 1,
                ));
            owners.push(owner);
        }
        let shared_hash = sim.state_hash();
        let rng = sim.rng_state();
        let mut saved_identities = Vec::new();
        for owner in owners.iter().copied() {
            sim.session.set_current_house_for_test(Some(owner));
            assert_eq!(sim.state_hash(), shared_hash);
            assert_eq!(sim.rng_state(), rng);
            let bytes = GameSnapshot::save(&sim, 0, 0, "current-house", 0);
            let mut restored = GameSnapshot::load(&bytes).expect("current schema").sim;
            restored
                .restore_after_snapshot_load()
                .expect("valid House input");
            assert_eq!(restored.session.current_house(), Some(owner));
            assert_eq!(restored.state_hash(), shared_hash);
            saved_identities.push(restored.session.current_house());
            assert_eq!(restored.session.current_house(), Some(owner));
            assert_eq!(restored.state_hash(), shared_hash);
            assert_eq!(restored.rng_state(), rng);
        }
        assert_ne!(saved_identities[0], saved_identities[1]);

        let missing = sim.intern("Missing");
        sim.session.set_current_house_for_test(Some(missing));
        assert_eq!(
            sim.restore_after_snapshot_load(),
            Err(SnapshotRestoreError::InvalidCurrentHouse { owner: missing })
        );
    }

    #[test]
    fn scenario_side_snapshot_matches_original_signed_retention() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/eva_selection.json",
        ))
        .unwrap();
        // Native Scenario Load reseeds its saved RNG to0. Isolate Side
        // persistence from that independently established load behavior.
        let mut sim = Simulation::with_seed(0);
        let owner = sim.intern("DifferentHouseSide");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner, 2, None, true, 0, 1,
            ));
        sim.session.install_current_house(Some(owner));
        let hash = sim.state_hash();
        let rng = sim.rng_state();
        for row in corpus["scenario_round_trips"].as_array().unwrap() {
            sim.session
                .install_scenario_side(row["input_side"].as_i64().unwrap() as i32);
            assert_eq!(sim.state_hash(), hash);
            assert_eq!(sim.rng_state(), rng);
            let bytes = GameSnapshot::save(&sim, 0, 0, "scenario-side", 0);
            sim.session
                .install_scenario_side(row["overwritten_before_load"].as_i64().unwrap() as i32);
            let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
            restored.restore_after_snapshot_load().unwrap();
            assert_eq!(
                restored.session.scenario_side(),
                row["restored_side"].as_i64().unwrap() as i32
            );
            assert_eq!(restored.session.current_house(), Some(owner));
            assert_eq!(restored.houses[&owner].side_index(), 2);
            assert_eq!(
                restored.state_hash(),
                hash,
                "client presentation input is not shared CRC state"
            );
            assert_eq!(restored.rng_state(), rng);
        }
    }

    #[test]
    fn restore_rejects_a_live_infantryman_with_the_dying_mark() {
        use crate::map::entities::EntityCategory;
        use crate::sim::game_entity::GameEntity;
        for (category, object_alive, rejected) in [
            (EntityCategory::Infantry, true, true),
            (EntityCategory::Infantry, false, false),
            (EntityCategory::Unit, true, false),
        ] {
            let mut sim = Simulation::new();
            let mut entity = GameEntity::test_default(1, "E1", "Americans", 5, 5);
            entity.set_category_for_test(category);
            entity.death.set_dying_for_test(true);
            entity.lifecycle.set_object_alive_for_test(object_alive);
            sim.substrate.entities.insert(entity);
            let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
            let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
            assert_eq!(
                restored.restore_after_snapshot_load()
                    == Err(SnapshotRestoreError::InvalidInfantryDyingMark { object_id: 1 }),
                rejected,
                "{category:?} alive={object_alive}"
            );
        }
    }

    #[test]
    fn authored_wall_neighbor_authority_roundtrips_v115() {
        let mut sim = Simulation::new();
        sim.overlay_grid = Some(
            crate::sim::overlay_grid::OverlayGrid::from_finalized_map_payload(
                crate::map::authored_overlay::FinalizedOverlayPayload::from_cells_for_test(
                    2,
                    2,
                    vec![(-1, 0), (2, 3), (-1, 0), (-1, 0)],
                    vec![0, 7, 255, 4],
                ),
            ),
        );

        let bytes = GameSnapshot::save(&sim, 0, 0, "authored-wall-counts", 0);
        let restored = GameSnapshot::load(&bytes)
            .expect("current v115 snapshot")
            .sim;
        assert_eq!(
            restored
                .overlay_grid
                .as_ref()
                .expect("overlay authority")
                .retained_neighbor_counts(),
            &[0, 7, 255, 4][..]
        );
    }

    #[test]
    fn overlay_rubble_building_type_roundtrips() {
        let mut sim = Simulation::new();
        let ruin = sim.interner.intern("CAOUTP");
        let mut grid = crate::sim::overlay_grid::OverlayGrid::new(3, 3);
        grid.place_overlay(1, 2, 0xEF, 4);
        grid.cell_mut_for_test(1, 2).rubble_building_type = Some(ruin);
        grid.place_overlay(2, 2, 0xEF, 0);
        sim.overlay_grid = Some(grid);

        let bytes = GameSnapshot::save(&sim, 0, 0, "rubble", 0);
        let restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        let grid = restored.overlay_grid.as_ref().expect("overlay authority");
        assert_eq!(grid.cell(1, 2).rubble_building_type, Some(ruin));
        assert_eq!(grid.cell(1, 2).overlay_data, 4);
        assert_eq!(grid.cell(2, 2).rubble_building_type, None);
    }

    #[test]
    fn phase14_crate_authority_roundtrips_visible_and_ghost_slots() {
        let mut sim = Simulation::with_seed(0);
        *sim.crate_authority.slot_mut_for_test(0) = crate::sim::crates::CrateSlot {
            start_frame: 17,
            aux: 0x40b5_1800,
            duration: 3375,
            cell_x: 6,
            cell_y: 9,
        };
        *sim.crate_authority.slot_mut_for_test(1) = crate::sim::crates::CrateSlot {
            start_frame: -2,
            aux: u32::MAX,
            duration: i32::MIN,
            cell_x: -4,
            cell_y: 12,
        };
        let expected = sim.crate_authority.clone();

        let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
        let loaded = GameSnapshot::load(&bytes).expect("v114 crate authority snapshot");
        assert_eq!(loaded.sim.crate_authority, expected);
    }

    #[test]
    fn drive_ship_active_and_stashed_slope_phase_roundtrip_without_resample_or_rng() {
        use crate::map::entities::EntityCategory;
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::movement::locomotion::LocomotorRuntimePayload;
        use crate::sim::movement::locomotor::LocomotorState;

        // Full snapshot load restarts Scenario RNG from Seed0. Start on that
        // canonical cursor so the slope-only round trip can prove no draw.
        let mut sim = Simulation::with_seed(0);
        sim.session.set_binary_frame_for_test(51);
        let mut entity = GameEntity::test_default(1, "SLOPE", "Americans", 0, 0);
        entity.set_owner_for_test(sim.intern("Americans"));
        entity.set_type_ref_for_test(sim.intern("SLOPE"));
        entity.set_category_for_test(EntityCategory::Unit);
        let mut locomotor = LocomotorState::for_test_kind_at_frame(LocomotorKind::Drive, 40);
        let drive = locomotor.active_slope_transition_mut().unwrap();
        drive.snap(2, 40);
        drive.sample_process_entry(7, 49);
        assert!(locomotor.begin_piggyback(LocomotorKind::Ship, 50));
        let ship = locomotor.active_slope_transition_mut().unwrap();
        ship.snap(4, 40);
        ship.sample_process_entry(9, 49);
        entity.locomotor = Some(locomotor);
        sim.substrate.entities.insert(entity);

        let before_hash = sim.state_hash();
        let before_rng = sim.scenario_rng.logical_state();
        let before = sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .locomotor
            .as_ref()
            .unwrap();
        let active_before = before.runtime_payload().clone();
        let stashed_before = before.piggyback().clone();

        let bytes = GameSnapshot::save(&sim, 1, 2, "Drive Ship slope", 3);
        let mut restored = GameSnapshot::load(&bytes)
            .expect("current v107 slope snapshot")
            .sim;
        let loaded = restored
            .substrate
            .entities
            .get(1)
            .unwrap()
            .locomotor
            .as_ref()
            .unwrap();
        assert_eq!(*loaded.runtime_payload(), active_before);
        assert_eq!(*loaded.piggyback(), stashed_before);
        assert_eq!(restored.session.binary_frame(), 51);
        assert_eq!(restored.state_hash(), before_hash);
        assert_eq!(restored.scenario_rng.logical_state(), before_rng);
        assert_eq!(
            loaded.active_slope_transition().unwrap().hash_fields(),
            (4, 9, 49, 3),
            "the saved active timer starts two committed frames before session frame 51"
        );
        assert!(matches!(
            loaded.piggyback().as_deref().map(|stashed| stashed.runtime_payload()),
            Some(LocomotorRuntimePayload::Drive(state))
                if state.slope().hash_fields() == (2, 7, 49, 3)
        ));

        let mut live_cell = crate::map::resolved_terrain::test_clear_cell(0, 0);
        live_cell.slope_type = 12;
        let live_terrain = ResolvedTerrainGrid::from_cells(1, 1, vec![live_cell]);
        restored.resolved_terrain = Some(live_terrain.clone());
        assert_eq!(
            restored
                .substrate
                .entities
                .get(1)
                .unwrap()
                .locomotor
                .as_ref()
                .unwrap()
                .active_slope_transition()
                .unwrap()
                .hash_fields(),
            (4, 9, 49, 3),
            "load/rebuild trusts the saved cache instead of resampling live terrain"
        );

        let mut sound_events = Vec::new();
        crate::sim::movement::tick_movement_with_grids(
            &mut restored.substrate.entities,
            Some(&[1]),
            None,
            &Default::default(),
            &mut restored.substrate.occupancy,
            &mut restored.substrate.raw_cell_occupation,
            &mut restored.scenario_rng,
            52,
            52,
            None,
            Some(&live_terrain),
            None,
            &crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            &mut restored.interner,
            None,
            &mut sound_events,
        );
        assert_eq!(
            restored
                .substrate
                .entities
                .get(1)
                .unwrap()
                .locomotor
                .as_ref()
                .unwrap()
                .active_slope_transition()
                .unwrap()
                .hash_fields(),
            (9, 12, 52, 3),
            "only the next eligible Process restarts against live terrain"
        );
        assert_eq!(restored.scenario_rng.logical_state(), before_rng);
    }

    #[test]
    fn building_loss_index_survives_transfer_and_snapshot_with_its_full_hash_value() {
        use crate::map::entities::EntityCategory;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::intern::test_intern;

        let mut hashes = std::collections::BTreeSet::new();
        for retained in [0, 3, 11, -1] {
            let mut sim = Simulation::new();
            sim.scenario_rng = crate::sim::rng::SimRng::new(0);
            let mut building = GameEntity::test_default_of_category(
                1,
                "GAPOWR",
                "Americans",
                4,
                5,
                EntityCategory::Structure,
            );
            match retained {
                0 => {}
                3 => building.initialize_building_loss_index(3),
                11 => building.record_building_damage_house_index(11),
                -1 => building.suppress_building_sale_loss(),
                _ => unreachable!(),
            }
            sim.substrate.entities.insert(building);
            sim.substrate
                .entities
                .change_owner(1, test_intern("Russians"));
            assert_eq!(
                sim.substrate.entities.get(1).unwrap().building_loss_index(),
                retained,
                "owner transfer must not recompute the retained index"
            );
            let expected_hash = sim.state_hash();
            assert!(
                hashes.insert(expected_hash),
                "hash the full index, not only whether it suppresses loss"
            );
            let rng = sim.scenario_rng.logical_state();
            let bytes = GameSnapshot::save(&sim, 1, 2, "building_loss_index", 0);
            let restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
            assert_eq!(
                restored
                    .substrate
                    .entities
                    .get(1)
                    .unwrap()
                    .building_loss_index(),
                retained
            );
            assert_eq!(restored.state_hash(), expected_hash);
            assert_eq!(restored.scenario_rng.logical_state(), rng);
        }
    }

    #[test]
    fn techno_constructor_word_and_installed_upgrades_roundtrip_without_a_draw_and_hash() {
        let mut sim = Simulation::new();
        let mut entity =
            crate::sim::game_entity::GameEntity::test_default(1, "UP1", "Americans", 4, 5);
        entity.set_techno_ctor_random_word_for_test(0xA55A);
        let upgrade_type = sim.interner.intern("UP1");
        entity.set_building_upgrades_for_test(3, [None, None, Some(upgrade_type)]);
        sim.substrate.entities.insert(entity);
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[BuildingTypes]\n0=UP1\n[UP1]\nUpgrades=3\n",
            ))
            .unwrap();
        sim.set_building_upgrade_art_name(upgrade_type, 0, "ART_UP", &rules);
        // Native load resets Scenario RNG to Seed(0); align the source before
        // comparing persistence/hash state unrelated to that separate rule.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let source_rng = sim.scenario_rng.logical_state();
        let source_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "techno-constructor", 0);
        let restored = GameSnapshot::load(&bytes).unwrap().sim;
        let restored_entity = restored.substrate.entities.get(1).unwrap();
        assert_eq!(restored.scenario_rng.logical_state(), source_rng);
        assert_eq!(restored_entity.techno_ctor_random_word(), 0xA55A);
        assert_eq!(restored_entity.building_upgrades().count(), 3);
        assert_eq!(
            restored_entity.building_upgrades().types(),
            &[None, None, Some(upgrade_type)]
        );
        assert_eq!(restored.state_hash(), source_hash);
        assert_eq!(
            restored.building_upgrade_art_names().name(upgrade_type, 0),
            Some("ART_UP")
        );

        let mut changed_word = GameSnapshot::load(&bytes).unwrap().sim;
        let entity = changed_word.substrate.entities.get_mut(1).unwrap();
        entity.set_techno_ctor_random_word_for_test(entity.techno_ctor_random_word() ^ 1);
        assert_ne!(changed_word.state_hash(), source_hash);

        let mut changed_upgrade = restored;
        changed_upgrade
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .set_building_upgrades_for_test(2, [None, Some(upgrade_type), None]);
        assert_ne!(changed_upgrade.state_hash(), source_hash);
    }

    #[test]
    fn native_tiberium_cargo_identity_roundtrips_and_changes_miner_and_slave_hashes() {
        use crate::rules::tiberium_type::TiberiumTypeId;
        use crate::sim::miner::{CargoBale, Miner, MinerConfig, MinerKind, ResourceType};
        for slave in [false, true] {
            let mut sim = Simulation::new();
            let mut entity =
                crate::sim::game_entity::GameEntity::test_default(1, "HARV", "Americans", 4, 5);
            let cargo = vec![CargoBale {
                tiberium_type: TiberiumTypeId(2),
                resource_type: ResourceType::Ore,
                value: 25,
            }];
            if slave {
                entity.slave = crate::sim::slave_manager::SlaveLink::for_test(None, cargo);
            } else {
                let mut miner = Miner::new(MinerKind::War, &MinerConfig::default(), 40);
                miner.set_cargo_for_test(cargo);
                entity.miner = Some(miner);
            }
            sim.substrate.entities.insert(entity);
            // Native loading resets Scenario RNG to Seed(0). Keep that
            // independent rule out of this cargo persistence/hash comparison.
            sim.scenario_rng = crate::sim::rng::SimRng::new(0);
            let hash = sim.state_hash();
            let bytes = GameSnapshot::save(&sim, 0, 0, "native cargo identity", 0);
            let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
            assert_eq!(restored.state_hash(), hash);
            let entity = restored.substrate.entities.get_mut(1).unwrap();
            let mut cargo = if slave {
                entity.slave.cargo().to_vec()
            } else {
                entity.miner.as_ref().unwrap().cargo().to_vec()
            };
            assert_eq!(cargo[0].tiberium_type, TiberiumTypeId(2));
            // Same presentation/value/amount; only the native slot differs.
            cargo[0].tiberium_type = TiberiumTypeId(0);
            if slave {
                entity.slave = crate::sim::slave_manager::SlaveLink::for_test(None, cargo);
            } else {
                entity.miner.as_mut().unwrap().set_cargo_for_test(cargo);
            }
            assert_ne!(restored.state_hash(), hash, "slave={slave}");
        }
    }

    #[test]
    fn slave_manager_roundtrips_and_hashes_its_nodes_and_links() {
        use crate::sim::slave_manager::{ManagerState, SlaveManager, SlaveNode, SlaveState};
        let mut sim = Simulation::new();
        let mut parent =
            crate::sim::game_entity::GameEntity::test_default(1, "SMIN", "Americans", 4, 5);
        parent.set_techno_ctor_random_word_for_test(0x1111);
        let slav = sim.intern("SLAV");
        parent.slave_manager = Some(SlaveManager::new(slav, [Some(2), Some(3)], 500, 25, 7));
        sim.substrate.entities.insert(parent);
        for (stable_id, word) in [(2, 0x2222), (3, 0x3333)] {
            let mut slave = crate::sim::game_entity::GameEntity::test_default(
                stable_id,
                "SLAV",
                "Americans",
                4,
                5,
            );
            slave.set_techno_ctor_random_word_for_test(word);
            // Slave 3 carries one level of gems.
            let cargo = (stable_id == 3)
                .then_some(crate::sim::miner::CargoBale {
                    tiberium_type: crate::rules::tiberium_type::TiberiumTypeId(1),
                    resource_type: crate::sim::miner::ResourceType::Gem,
                    value: 50,
                })
                .into_iter()
                .collect();
            slave.slave = crate::sim::slave_manager::SlaveLink::for_test(Some(1), cargo);
            sim.substrate.entities.insert(slave);
        }
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let source_rng = sim.scenario_rng.logical_state();
        let source_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "slave-manager", 0);
        let restored = GameSnapshot::load(&bytes).unwrap().sim;
        assert_eq!(restored.scenario_rng.logical_state(), source_rng);
        assert_eq!(
            restored.substrate.entities.get(1).unwrap().slave_manager,
            sim.substrate.entities.get(1).unwrap().slave_manager
        );
        for (stable_id, word) in [(2, 0x2222), (3, 0x3333)] {
            let slave = restored.substrate.entities.get(stable_id).unwrap();
            assert_eq!(slave.techno_ctor_random_word(), word);
            assert_eq!(slave.slave.owner(), Some(1));
        }
        assert_eq!(
            restored
                .substrate
                .entities
                .get(3)
                .unwrap()
                .slave
                .cargo()
                .len(),
            1
        );
        assert_eq!(restored.state_hash(), source_hash);

        let mut changed_node = GameSnapshot::load(&bytes).unwrap().sim;
        let manager = changed_node
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .slave_manager
            .as_mut()
            .unwrap();
        let mut nodes = manager.nodes().to_vec();
        nodes[0].set_state_for_test(SlaveState::Scanning);
        let timer = manager.ai_timer();
        manager.set_for_test(ManagerState::Ready, 0, nodes, timer);
        assert_ne!(changed_node.state_hash(), source_hash);

        let mut changed_order = GameSnapshot::load(&bytes).unwrap().sim;
        let manager = changed_order
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .slave_manager
            .as_mut()
            .unwrap();
        let mut nodes: Vec<SlaveNode> = manager.nodes().to_vec();
        nodes.reverse();
        let timer = manager.ai_timer();
        manager.set_for_test(ManagerState::Ready, 0, nodes, timer);
        assert_ne!(changed_order.state_hash(), source_hash);

        let mut changed_owner = restored;
        changed_owner.substrate.entities.get_mut(2).unwrap().slave =
            crate::sim::slave_manager::SlaveLink::for_test(Some(3), Vec::new());
        assert_ne!(changed_owner.state_hash(), source_hash);
    }

    #[test]
    fn house_ai_activation_all_combinations_roundtrip_v115() {
        use crate::sim::house_state::{HouseAiActivationLatches, HouseState};

        for bits in 0u8..16 {
            let mut sim = Simulation::new();
            let owner = sim.interner.intern("Computer1");
            let latches = HouseAiActivationLatches {
                production: bits & 1 != 0,
                autocreate_allowed: bits & 2 != 0,
                ai_triggers_active: bits & 4 != 0,
                auto_base_building: bits & 8 != 0,
            };
            let mut house = HouseState::new(owner, 0, None, false, 0, 10);
            house.set_ai_activation_for_test(latches);
            sim.houses.register(house);

            let description = format!("house-ai-activation-{bits}");
            let bytes = GameSnapshot::save(&sim, 0, 0, &description, 0);
            assert_eq!(
                GameSnapshot::read_header(&bytes).unwrap().layout,
                super::snapshot_layout()
            );
            let restored = GameSnapshot::load(&bytes)
                .expect("current v115 snapshot")
                .sim;
            assert_eq!(restored.houses[&owner].ai_activation(), latches);
        }
    }

    #[test]
    fn alternate_base_center_roundtrips_with_primary_center_and_hash() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("AMERICANS");
        let country = sim.interner.intern("Americans");
        let mut house =
            crate::sim::house_state::HouseState::new(owner, 0, Some(country), false, 0, 10);
        house.set_base_center_for_test(Some((40, 50)));
        house.set_alternate_base_center_for_test((93, 106));
        house.set_base_plan_center_for_test((19, 21));
        sim.houses.register(house);
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "alternate-base-center", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().layout,
            super::snapshot_layout()
        );
        let restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;

        assert_eq!(restored.houses[&owner].base_center(), Some((40, 50)));
        assert_eq!(restored.houses[&owner].alternate_base_center(), (93, 106));
        assert_eq!(restored.houses[&owner].base_plan_center(), (19, 21));
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn naval_build_const_order_and_membership_roundtrip_with_hash() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("AMERICANS");
        let country = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("GACNST");
        let house = crate::sim::house_state::HouseState::new(owner, 0, Some(country), false, 0, 10);
        sim.houses.register(house);
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let entity = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
            9,
            4,
            5,
            0,
            0,
            owner,
            crate::sim::combat::Health::new(1000),
            type_ref,
            crate::map::entities::EntityCategory::Structure,
            0,
            5,
            false,
        );
        sim.substrate.entities.insert(entity);

        let default_hash = sim.state_hash();

        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_build_const_order_for_test(vec![9, 3]);
        sim.substrate
            .entities
            .get_mut(9)
            .unwrap()
            .type_facts
            .set_build_const_eligible_for_test(true);

        let ordered_hash = sim.state_hash();
        assert_ne!(ordered_hash, default_hash);
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .build_const_order_mut_for_test()
            .swap(0, 1);
        assert_ne!(
            sim.state_hash(),
            ordered_hash,
            "stored vector order is hashed"
        );
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .build_const_order_mut_for_test()
            .swap(0, 1);

        sim.substrate
            .entities
            .get_mut(9)
            .unwrap()
            .type_facts
            .set_build_const_eligible_for_test(false);
        assert_ne!(
            sim.state_hash(),
            ordered_hash,
            "entity membership is hashed"
        );
        sim.substrate
            .entities
            .get_mut(9)
            .unwrap()
            .type_facts
            .set_build_const_eligible_for_test(true);
        assert_eq!(sim.state_hash(), ordered_hash);

        let bytes = GameSnapshot::save(&sim, 0, 0, "naval-build-const", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().layout,
            super::snapshot_layout()
        );
        let restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;

        assert_eq!(restored.houses[&owner].build_const_order(), vec![9, 3]);
        assert!(
            restored
                .substrate
                .entities
                .get(9)
                .unwrap()
                .type_facts
                .build_const_eligible()
        );
        assert_eq!(restored.state_hash(), ordered_hash);
    }

    #[test]
    fn gsi_04_05_base_plan_and_building_facts_roundtrip_current_snapshot() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Computer1");
        let type_ref = sim.interner.intern("GAPOWR");
        let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, false, 0, 10);
        house.base_plan_mut().set_percent_built_for_test(-17);
        house.base_plan_mut().set_nodes_for_test(vec![
            crate::sim::base_plan::BasePlanNode {
                type_or_control: 2,
                packed_cell: crate::sim::base_plan::pack_base_plan_cell(-4, 9),
                filled: true,
                retry_count: -8,
            },
            crate::sim::base_plan::BasePlanNode {
                type_or_control: -3,
                packed_cell: 0,
                filled: false,
                retry_count: 6,
            },
        ]);
        sim.houses.register(house);
        let mut entity = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
            1,
            12,
            14,
            0,
            0,
            owner,
            crate::sim::combat::Health::new(750),
            type_ref,
            crate::map::entities::EntityCategory::Structure,
            0,
            5,
            false,
        );
        entity.type_facts.set_base_plan_type_index_for_test(2);
        entity.type_facts.set_base_plan_is_defense_for_test(true);
        entity
            .type_facts
            .set_base_plan_has_undeploy_target_for_test(true);
        sim.substrate.entities.insert(entity);
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "base-plan", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().layout,
            super::snapshot_layout()
        );
        let restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        assert_eq!(restored.houses[&owner].base_plan().percent_built(), -17);
        assert_eq!(restored.houses[&owner].base_plan().nodes().len(), 2);
        assert_eq!(
            restored.houses[&owner].base_plan().nodes()[0].retry_count,
            -8
        );
        let restored_entity = restored.substrate.entities.get(1).unwrap();
        assert_eq!(restored_entity.type_facts.base_plan_type_index(), 2);
        assert!(restored_entity.type_facts.base_plan_is_defense());
        assert!(restored_entity.type_facts.base_plan_has_undeploy_target());
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn gsi_04_05_house_and_techno_base_defense_state_roundtrip_with_hash() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Computer1");
        let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, false, 10_000, 10);
        house.strategy_emergency_mut().set_state_four();
        house.strategy_emergency_mut().set_all_to_hunt_bias();
        house.strategy_emergency_mut().note_building_attack(-17);
        house.strategy_emergency_mut().note_building_attacker(3);
        house.restart_strategy_timer(12, 106);
        sim.houses.register(house);
        let mut responder =
            crate::sim::game_entity::GameEntity::test_default(1, "E1", "Computer1", 4, 5);
        responder
            .base_defense_response
            .set_recruitable_a_for_test(false);
        responder
            .base_defense_response
            .set_recruitable_b_for_test(true);
        responder.set_archive_target(Some(crate::sim::combat::TargetKind::Entity(9)));
        responder
            .base_defense_response
            .set_cooldown_for_test(crate::sim::timer::CdTimer::started(-11, 225));
        sim.substrate.entities.insert(responder);
        let script_id = sim.interner.intern("BaseDefenseScript");
        let task_force_id = sim.interner.intern("BaseDefenseTaskForce");
        let team_type_id = sim.interner.intern("BaseDefenseTeamType");
        let ai_trigger_id = sim.interner.intern("BaseDefenseAITrigger");
        let member_type = sim.interner.intern("E1");
        let member_identity = crate::sim::team_script_vm::TeamMemberTypeIdentity {
            category: crate::rules::object_type::ObjectCategory::Infantry,
            id: member_type,
        };
        sim.team_script_vm
            .register_script(crate::sim::team_script_vm::TeamScriptDefinition {
                id: script_id,
                source: crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd,
                actions: vec![crate::sim::team_script_vm::TeamScriptAction {
                    action_id: 2,
                    argument: 0,
                }],
            });
        sim.team_script_vm.register_task_force(
            crate::sim::team_script_vm::TeamTaskForceDefinition {
                id: task_force_id,
                source: crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd,
                group: 7,
                entries: vec![crate::sim::team_script_vm::TeamTaskForceEntry {
                    member_type: member_identity,
                    count: 1,
                }],
            },
        );
        sim.team_script_vm
            .register_team_type(crate::sim::team_script_vm::TeamTypeDefinition {
                id: team_type_id,
                script_id,
                task_force_id,
                priority: 0,
                is_base_defense: true,
                suicide: false,
                aggressive: false,
                combined_movement_zone: crate::rules::locomotor_type::MovementZone::Amphibious,
                base_zone_relation_enforced: false,
                transport_crossing_required: true,
            });
        sim.team_script_vm.register_ai_trigger(
            crate::sim::team_script_vm::TeamAiTriggerDefinition {
                id: ai_trigger_id,
                tokens: std::array::from_fn(|index| format!("raw-token-{index}")),
                display_name: "Base defense trigger".to_string(),
                enabled: true,
                primary_team_type: Some(team_type_id),
                owner: Some(crate::sim::team_script_vm::TeamAiTriggerOwner::Country(
                    crate::rules::ruleset::CountryIdx(4),
                )),
                threshold: 7,
                condition: 6,
                object_type: Some(member_identity),
                comparison_mask: std::array::from_fn(|index| index as u8),
                weights: [
                    crate::util::native_x87::NativeF64Bits::from_bits(1.5_f64.to_bits()),
                    crate::util::native_x87::NativeF64Bits::from_bits(2.5_f64.to_bits()),
                    crate::util::native_x87::NativeF64Bits::from_bits(3.5_f64.to_bits()),
                ],
                multiplayer: true,
                side: -9,
                storage_flag_d1: false,
                secondary_team_type: None,
                difficulty_enabled: [true, false, true],
                source: crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd,
            },
        );
        let team_id = sim.team_script_vm.create_team_from_type(
            owner,
            team_type_id,
            &[crate::sim::team_script_vm::TeamScriptMember {
                entity_id: 1,
                member_type: member_identity,
            }],
            sim.session.binary_frame() as i32,
        );
        let resources = crate::sim::runtime::SimResources::empty();
        sim.suspend_teams_for_base_defense(
            owner,
            1,
            1800,
            &resources.rules,
            crate::sim::world::FrameEffects::default(),
        );
        assert!(sim.team_script_vm.team_for_member(1).is_none());
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_05_strategy", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().layout,
            super::snapshot_layout()
        );
        let restored = GameSnapshot::load(&bytes).expect("v102 snapshot").sim;
        let emergency = restored.houses[&owner].strategy_emergency();
        assert_eq!(emergency.mode(), 4);
        assert!(emergency.all_to_hunt_bias());
        assert_eq!(emergency.last_building_attack_frame(), -17);
        assert_eq!(emergency.last_attacker_house_index(), 3);
        assert_eq!(
            *restored.houses[&owner].strategy_timer(),
            crate::sim::timer::CdTimer::started(12, 106)
        );
        let restored_responder = restored.substrate.entities.get(1).unwrap();
        let response = restored_responder.base_defense_response;
        assert!(!response.recruitable_a());
        assert!(response.recruitable_b());
        assert_eq!(
            restored_responder.archive_target(),
            Some(crate::sim::combat::TargetKind::Entity(9))
        );
        assert_eq!(
            response.cooldown(),
            crate::sim::timer::CdTimer::started(-11, 225)
        );
        let team = restored.team_script_vm.team(team_id).unwrap();
        assert_eq!(team.member_count(), 0);
        assert_eq!(restored.team_script_vm.registry_counts(), (1, 1, 1, 1));
        assert_eq!(restored.team_script_vm.team_type_order(), &[team_type_id]);
        let restored_team_type = restored
            .team_script_vm
            .team_type(team_type_id)
            .expect("persisted TeamType");
        assert_eq!(
            restored_team_type.combined_movement_zone,
            crate::rules::locomotor_type::MovementZone::Amphibious
        );
        assert!(!restored_team_type.base_zone_relation_enforced);
        assert!(restored_team_type.transport_crossing_required);
        assert_eq!(restored.team_script_vm.script_order(), &[script_id]);
        assert_eq!(restored.team_script_vm.task_force_order(), &[task_force_id]);
        assert_eq!(restored.team_script_vm.ai_trigger_order(), &[ai_trigger_id]);
        let restored_script = restored
            .team_script_vm
            .script(script_id)
            .expect("persisted ScriptType");
        assert_eq!(
            restored_script.source,
            crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd
        );
        assert_eq!(restored_script.actions.len(), 1);
        assert_eq!(restored_script.actions[0].action_id, 2);
        assert_eq!(restored_script.actions[0].argument, 0);
        let restored_task_force = restored
            .team_script_vm
            .task_force(task_force_id)
            .expect("persisted TaskForce");
        assert_eq!(
            restored_task_force.source,
            crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd
        );
        assert_eq!(restored_task_force.entries.len(), 1);
        assert_eq!(restored_task_force.entries[0].member_type, member_identity);
        assert_eq!(restored_task_force.entries[0].count, 1);
        assert_eq!(restored_task_force.group, 7);
        let restored_trigger = restored
            .team_script_vm
            .ai_trigger(ai_trigger_id)
            .expect("persisted typed AITriggerType");
        assert_eq!(restored_trigger.tokens[11], "raw-token-11");
        assert_eq!(restored_trigger.display_name, "Base defense trigger");
        assert!(restored_trigger.enabled);
        assert_eq!(restored_trigger.primary_team_type, Some(team_type_id));
        assert_eq!(
            restored_trigger.owner,
            Some(crate::sim::team_script_vm::TeamAiTriggerOwner::Country(
                crate::rules::ruleset::CountryIdx(4)
            ))
        );
        assert_eq!(restored_trigger.threshold, 7);
        assert_eq!(restored_trigger.condition, 6);
        assert_eq!(restored_trigger.object_type, Some(member_identity));
        assert_eq!(restored_trigger.comparison_mask[31], 31);
        assert_eq!(
            restored_trigger.weights,
            [
                crate::util::native_x87::NativeF64Bits::from_bits(1.5_f64.to_bits()),
                crate::util::native_x87::NativeF64Bits::from_bits(2.5_f64.to_bits()),
                crate::util::native_x87::NativeF64Bits::from_bits(3.5_f64.to_bits()),
            ]
        );
        assert!(restored_trigger.multiplayer);
        assert_eq!(restored_trigger.side, -9);
        assert!(!restored_trigger.storage_flag_d1);
        assert_eq!(restored_trigger.secondary_team_type, None);
        assert_eq!(restored_trigger.difficulty_enabled, [true, false, true]);
        assert_eq!(
            restored_trigger.source,
            crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd
        );
        assert_eq!(
            team.response_suspension_state(),
            (true, true, true, 0, 1800)
        );
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn wave_active_inactive_dummy_links_roundtrip_with_identical_hash() {
        use crate::map::entities::EntityCategory;
        use crate::sim::combat::Health;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::projectile::ProjectileCoord;
        use crate::sim::wave::{Wave, WaveRecordedCell, WaveUpdateContext};
        use crate::util::native_x87::NativeF64Bits;

        let mut sim = Simulation::new();
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("DLPH");
        let mut owner_ids = Vec::new();
        for ordinal in 1..=3 {
            let stable_id = sim.substrate.allocate_stable_id();
            owner_ids.push(stable_id);
            let mut entity = GameEntity::new_at_frame_zero_for_test(
                stable_id,
                ordinal,
                2,
                0,
                0,
                owner,
                Health::new(200),
                type_ref,
                EntityCategory::Unit,
                0,
                5,
                false,
            );
            entity.in_logic_vector.set_for_test(true);
            sim.substrate.entities.insert(entity);
            sim.substrate
                .logic
                .try_push(stable_id)
                .expect("owner Logic registration");
        }

        let active_id = sim.substrate.allocate_stable_id();
        let active_target = crate::sim::combat::TargetKind::Cell(8, 2);
        let active_source = ProjectileCoord::new(256, 512, 0);
        let active_endpoint = ProjectileCoord::new(2048, 512, 0);
        let mut active = Wave::new_owned(
            0,
            owner_ids[0],
            active_target,
            active_source,
            active_endpoint,
        );
        assert!(!active.initialize(
            WaveUpdateContext {
                owner_position: Some(active_source),
                owner_fire_coordinate: Some(active_source),
                owner_current_target: Some(active_target),
                target_position: Some(active_endpoint),
                target_is_unit: false,
            },
            None,
        ));
        let constructor_direction = active.direction_octant();
        let moved_endpoint = ProjectileCoord::new(0, 512, 0);
        let _ = active.advance(
            WaveUpdateContext {
                owner_position: Some(active_source),
                owner_fire_coordinate: Some(active_source),
                owner_current_target: Some(active_target),
                target_position: Some(moved_endpoint),
                target_is_unit: false,
            },
            None,
        );
        assert_eq!(
            active.direction_octant(),
            constructor_direction,
            "live geometry refresh retains constructor-only +0x1CC"
        );
        let moved_edges = active.edge_geometry();
        sim.admit_wave(active_id, active);
        sim.link_active_wave_for_test(owner_ids[0], active_id);

        let inactive_id = sim.substrate.allocate_stable_id();
        let mut inactive = Wave::new_owned(
            0,
            owner_ids[1],
            crate::sim::combat::TargetKind::Cell(9, 2),
            ProjectileCoord::new(512, 512, 0),
            ProjectileCoord::new(2304, 512, 50),
        );
        inactive.set_active_geometry_for_test(false);
        inactive.set_decaying_for_test(true);
        inactive.set_fade_in_for_test(NativeF64Bits::from_bits(0x3fe0_0000_0000_0000));
        inactive.set_fade_out_for_test(NativeF64Bits::from_bits(0x3fc9_9999_a000_0000));
        inactive.set_direction_octant_for_test(6);
        inactive.replace_recorded_cells(vec![WaveRecordedCell::real(7, 2)]);
        sim.admit_wave(inactive_id, inactive);
        sim.link_active_wave_for_test(owner_ids[1], inactive_id);

        let dummy_id = sim.substrate.allocate_stable_id();
        let mut dummy = Wave::new_owned(
            0,
            owner_ids[2],
            crate::sim::combat::TargetKind::Cell(10, 2),
            ProjectileCoord::new(768, 512, 0),
            ProjectileCoord::new(2560, 512, 50),
        );
        dummy.set_active_geometry_for_test(false);
        dummy.set_decaying_for_test(true);
        dummy.set_direction_octant_for_test(2);
        dummy.replace_recorded_cells(vec![WaveRecordedCell::shared_dummy()]);
        sim.admit_wave(dummy_id, dummy);
        sim.link_active_wave_for_test(owner_ids[2], dummy_id);

        let expected_hash = sim.state_hash();
        let bytes = GameSnapshot::save(&sim, 0, 0, "wave-state.map", 0);
        let mut restored = GameSnapshot::load(&bytes).expect("Wave snapshot").sim;
        restored
            .restore_after_snapshot_load()
            .expect("Wave identities restore structurally");

        assert_eq!(restored.state_hash(), expected_hash);
        assert_eq!(
            restored.active_wave_links().collect::<Vec<_>>(),
            sim.active_wave_links().collect::<Vec<_>>()
        );
        assert_eq!(restored.waves.get(active_id), sim.waves.get(active_id));
        assert_eq!(
            restored.waves.get(active_id).unwrap().direction_octant(),
            constructor_direction
        );
        assert_eq!(
            restored.waves.get(active_id).unwrap().edge_geometry(),
            moved_edges
        );
        assert_eq!(restored.waves.get(inactive_id), sim.waves.get(inactive_id));
        assert_eq!(restored.waves.get(dummy_id), sim.waves.get(dummy_id));
        assert_eq!(
            restored.waves.get(dummy_id).unwrap().recorded_cells(),
            vec![WaveRecordedCell::shared_dummy()],
        );
    }

    #[test]
    fn building_anim_slot_restore_rejects_invalid_graph_before_mutation() {
        for case in 0..8 {
            let (mut sim, rules, owner) = crate::sim::building_art::slot_test_fixture();
            let anim = sim
                .set_building_anim_slot(owner, 3, false, false, 0, &rules)
                .unwrap();
            let expected = match case {
                0 | 1 => {
                    let target = if case == 0 { 999 } else { owner };
                    sim.substrate
                        .entities
                        .get_mut(owner)
                        .unwrap()
                        .building_art
                        .anim_slots_mut_for_test()[3] = Some(target);
                    SnapshotRestoreError::MissingBuildingSlotAnim {
                        owner_id: owner,
                        slot: 3,
                        anim_id: target,
                    }
                }
                2 => {
                    sim.substrate
                        .entities
                        .get_mut(owner)
                        .unwrap()
                        .set_category_for_test(crate::map::entities::EntityCategory::Unit);
                    SnapshotRestoreError::InvalidBuildingAnimSlotOwner {
                        owner_id: owner,
                        slot: 3,
                    }
                }
                3 => {
                    sim.substrate
                        .entities
                        .get_mut(owner)
                        .unwrap()
                        .building_art
                        .anim_slots_mut_for_test()[4] = Some(anim);
                    SnapshotRestoreError::DuplicateBuildingSlotAnim {
                        anim_id: anim,
                        first_owner: owner,
                        first_slot: 3,
                        second_owner: owner,
                        second_slot: 4,
                    }
                }
                4 => {
                    let second = sim.substrate.allocate_stable_id();
                    let mut entity = sim.substrate.entities.get(owner).unwrap().clone();
                    entity.set_stable_id_for_test(second);
                    sim.substrate.entities.insert(entity);
                    SnapshotRestoreError::DuplicateBuildingSlotAnim {
                        anim_id: anim,
                        first_owner: owner,
                        first_slot: 3,
                        second_owner: second,
                        second_slot: 3,
                    }
                }
                5 => {
                    sim.substrate
                        .entities
                        .get_mut(owner)
                        .unwrap()
                        .damage_fires
                        .set_slot_for_test(0, Some(999));
                    SnapshotRestoreError::MissingBuildingSlotAnim {
                        owner_id: owner,
                        slot: 21,
                        anim_id: 999,
                    }
                }
                6 => {
                    sim.substrate
                        .entities
                        .get_mut(owner)
                        .unwrap()
                        .damage_fires
                        .set_slot_for_test(0, Some(anim));
                    SnapshotRestoreError::DuplicateBuildingSlotAnim {
                        anim_id: anim,
                        first_owner: owner,
                        first_slot: 3,
                        second_owner: owner,
                        second_slot: 21,
                    }
                }
                _ => {
                    let entity = sim.substrate.entities.get_mut(owner).unwrap();
                    entity.building_art.anim_slots_mut_for_test()[3] = None;
                    entity.damage_fires.set_slot_for_test(0, Some(anim));
                    entity.set_category_for_test(crate::map::entities::EntityCategory::Unit);
                    SnapshotRestoreError::InvalidBuildingAnimSlotOwner {
                        owner_id: owner,
                        slot: 21,
                    }
                }
            };
            let before = GameSnapshot::save(&sim, 0, 0, "slot-rejection.map", 0);
            assert_eq!(
                sim.restore_after_snapshot_load(),
                Err(expected),
                "case {case}"
            );
            assert_eq!(
                GameSnapshot::save(&sim, 0, 0, "slot-rejection.map", 0),
                before,
                "restore must reject atomically: {case}"
            );
            assert_eq!(sim.anim(anim).unwrap().building_slot(), Some((owner, 3)));
        }
    }

    #[test]
    fn building_anim_slot_restore_keeps_expired_slots_clear_during_deferred_destroy() {
        let (mut sim, rules, owner) = crate::sim::building_art::slot_test_fixture();
        let anim = sim
            .set_building_anim_slot(owner, 3, false, false, 0, &rules)
            .unwrap();
        assert!(sim.anim(anim).unwrap().owner_entity().is_none());
        sim.destroy_anim(anim, &rules);
        assert!(sim.substrate.pending_delete.contains_object(&anim));
        // Anim UnInit4255B0 broadcasts before deferred physical deletion;
        // Building451B40 clears slot3 immediately but retains Anim+118.
        assert_eq!(
            sim.entities().get(owner).unwrap().building_art.anim_slots()[3],
            None
        );
        assert!(sim.anim(anim).unwrap().building_slot().is_none());
        assert!(sim.anim(anim).unwrap().is_building_anim());
        let bytes = GameSnapshot::save(&sim, 0, 0, "expired-slot.map", 0);
        // Scenario load consumes the saved RNG state and reseeds zero; keep
        // the animation-constructor draw in the saved input, as above.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let hash = sim.state_hash();
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(restored.anim(anim).unwrap().building_slot(), None);
        assert!(restored.anim(anim).unwrap().is_building_anim());
        assert!(restored.substrate.pending_delete.contains_object(&anim));
        assert_eq!(restored.state_hash(), hash);
    }

    #[test]
    fn building_anim_slots_roundtrip_with_runtime_and_reverse_index() {
        let (mut sim, rules, id) = crate::sim::building_art::slot_test_fixture();
        let load_rng = crate::sim::rng::SimRng::new(0).logical_state();
        let anim = sim
            .set_building_anim_slot(id, 3, true, false, 0, &rules)
            .unwrap();
        sim.substrate
            .anims
            .get_mut(anim)
            .unwrap()
            .runtime_mut_for_test()
            .current_frame = 17;
        sim.substrate
            .anims
            .get_mut(anim)
            .unwrap()
            .runtime_mut_for_test()
            .paused = true;
        let building = sim.substrate.entities.get_mut(id).unwrap();
        building.building_art.anim_effect_replay_mut_for_test()[16] = true;
        building.building_last_operational.set_for_test(true);
        building.building_art.set_stuff_enabled_for_test(false);
        building
            .building_art
            .storage_mut_for_test()
            .set_amounts_for_test([
                crate::util::native_x87::NativeF32Bits::from_bits(0x7fc01234),
                crate::util::native_x87::NativeF32Bits::NEGATIVE_ZERO,
                crate::util::native_x87::NativeF32Bits::ONE,
                crate::util::native_x87::NativeF32Bits::from_bits(0xbf800000),
            ]);
        building
            .building_art
            .storage_mut_for_test()
            .set_refinery_tier_for_test(-17);
        assert_ne!(sim.scenario_rng.logical_state(), load_rng);
        let bytes = GameSnapshot::save(&sim, 1, 2, "building-anim.map", 0);
        // Native Scenario load consumes the saved RNG bytes, then reseeds zero.
        // The RandomRate constructor draw remains observable before saving;
        // compare all saved slot/runtime state against the native load state.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected = sim.state_hash();
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().layout,
            snapshot_layout()
        );
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(restored.scenario_rng.logical_state(), load_rng);
        assert_eq!(
            restored
                .entities()
                .get(id)
                .unwrap()
                .building_art
                .anim_slots()[3],
            Some(anim)
        );
        assert!(
            restored
                .entities()
                .get(id)
                .unwrap()
                .building_art
                .damage_state_active()
        );
        assert_eq!(restored.anim(anim).unwrap().runtime().current_frame, 17);
        assert!(restored.anim(anim).unwrap().runtime().paused);
        assert!(
            restored
                .entities()
                .get(id)
                .unwrap()
                .building_art
                .anim_effect_replay()[16]
        );
        assert!(
            restored
                .entities()
                .get(id)
                .unwrap()
                .building_last_operational
                .is_operational()
        );
        assert!(
            !restored
                .entities()
                .get(id)
                .unwrap()
                .building_art
                .stuff_enabled()
        );
        assert_eq!(
            restored.entities().get(id).unwrap().building_art.storage(),
            sim.entities().get(id).unwrap().building_art.storage()
        );
        assert_eq!(restored.anim(anim).unwrap().building_slot(), Some((id, 3)));
        assert_eq!(restored.state_hash(), expected);
    }

    #[test]
    fn gsi_13_06_body_frame_counter_roundtrips_and_changes_hash() {
        use crate::map::entities::EntityCategory;
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::sim::combat::Health;
        use crate::sim::components::DriveCoord;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::movement::ShipLocomotionRuntime;
        use crate::sim::movement::locomotor::LocomotorState;
        use crate::util::fixed_math::{SIM_HALF, SIM_ONE};

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Soviet");
        let type_ref = sim.interner.intern("DRON");
        sim.substrate
            .entities
            .insert(GameEntity::new_at_frame_zero_for_test(
                1,
                5,
                5,
                0,
                0,
                owner,
                Health::new(100),
                type_ref,
                EntityCategory::Unit,
                0,
                5,
                false,
            ));
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let zero_counter_hash = sim.state_hash();
        sim.substrate
            .entities
            .get_mut(1)
            .expect("Terror Drone")
            .body_frame_counter
            .set_for_test(u32::MAX);
        let populated_counter_hash = sim.state_hash();
        assert_ne!(populated_counter_hash, zero_counter_hash);
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .unit_firing_counter
            .set_for_test(7);
        assert_ne!(sim.state_hash(), populated_counter_hash);

        let ship_head = DriveCoord::cell(6, 5, 0);
        let entity = sim.substrate.entities.get_mut(1).expect("SHP unit");
        entity.foot_speed.set_speed_fraction(SIM_HALF);
        // This serialization fixture supplies a Ship instance explicitly.
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Ship));
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .install_ship_state_for_test(Some(
                ShipLocomotionRuntime::default()
                    .with_destination_for_test(Some(ship_head))
                    .with_head_to_for_test(Some(ship_head))
                    .with_target_speed_fraction_for_test(SIM_ONE),
            ));
        let populated_shp_state_hash = sim.state_hash();
        assert_ne!(populated_shp_state_hash, populated_counter_hash);

        let bytes = GameSnapshot::save(&sim, 1, 2, "counter.map", 0);
        let restored = GameSnapshot::load(&bytes)
            .expect("v82 body-counter snapshot")
            .sim;
        assert_eq!(
            restored
                .substrate
                .entities
                .get(1)
                .expect("restored Terror Drone")
                .body_frame_counter
                .get(),
            u32::MAX
        );
        assert_eq!(
            restored
                .entities()
                .get(1)
                .unwrap()
                .unit_firing_counter
                .get(),
            7
        );
        let restored_ship = restored
            .substrate
            .entities
            .get(1)
            .expect("restored SHP unit")
            .locomotor
            .as_ref()
            .and_then(|loco| loco.selected_ship_runtime())
            .and_then(|runtime| runtime.retained())
            .expect("restored Ship runtime");
        assert_eq!(restored_ship.destination(), Some(ship_head));
        assert_eq!(restored_ship.head_to(), Some(ship_head));
        assert_eq!(restored_ship.target_speed_fraction(), SIM_ONE);
        let restored_owner_speed = &restored.substrate.entities.get(1).unwrap().foot_speed;
        assert_eq!(restored_owner_speed.applied_fraction(), SIM_HALF);
        assert_eq!(restored.state_hash(), populated_shp_state_hash);
    }

    #[test]
    fn gsi_05_10_pending_building_fire_roundtrips_and_changes_hash() {
        use crate::map::entities::EntityCategory;
        use crate::sim::combat::Health;
        use crate::sim::combat::combat_weapon::WeaponSlot;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::projectile::ProjectileCoord;
        use crate::sim::world::{DelayedFire, PendingBuildingFire};

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Soviet");
        let type_ref = sim.interner.intern("NATSLA");
        for id in [1, 2] {
            let entity = GameEntity::new_at_frame_zero_for_test(
                id,
                5,
                5,
                0,
                0,
                owner,
                Health::new(600),
                type_ref,
                EntityCategory::Structure,
                0,
                8,
                false,
            );
            sim.substrate.entities.insert(entity);
        }
        // Full snapshot load resets Scenario RNG to Seed0. Compare the
        // authoritative delayed-fire state on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let without_latch = sim.state_hash();
        let shot = PendingBuildingFire::new(17, DelayedFire::Weapon(WeaponSlot::Secondary));
        sim.substrate
            .entities
            .get_mut(1)
            .expect("Tesla Coil")
            .pending_building_fire
            .set_for_test(Some(shot));
        let with_latch = sim.state_hash();
        assert_ne!(with_latch, without_latch);
        let beam = PendingBuildingFire::new(
            17,
            DelayedFire::SupportBeam {
                to: ProjectileCoord::new(1280, 1408, 378),
            },
        );
        let supporter = sim.substrate.entities.get_mut(2).expect("second tower");
        supporter.pending_building_fire.set_for_test(Some(beam));
        supporter.prism_support_count.set_for_test(3);
        let with_beam = sim.state_hash();
        assert_ne!(with_beam, with_latch);

        let bytes = GameSnapshot::save(&sim, 1, 2, "delay.map", 0);
        let restored = GameSnapshot::load(&bytes)
            .expect("delayed-fire snapshot")
            .sim;
        let entities = &restored.substrate.entities;
        assert_eq!(
            entities.get(1).unwrap().pending_building_fire.pending(),
            Some(shot)
        );
        assert_eq!(
            entities.get(2).unwrap().pending_building_fire.pending(),
            Some(beam)
        );
        assert_eq!(entities.get(2).unwrap().prism_support_count.count(), 3);
        assert_eq!(restored.state_hash(), with_beam);
    }

    #[test]
    fn gsi_01_04_pending_exit_command_roundtrips_without_terminal_edge() {
        use crate::sim::command::{Command, CommandEnvelope};
        use crate::sim::house_state::HouseState;

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        sim.houses
            .register(HouseState::new(owner, 0, None, true, 0, 10));
        let hash_without_pending_input = sim.state_hash();
        sim.queue_command(CommandEnvelope::new(owner, 17, Command::ExitMatch));
        assert_eq!(
            sim.state_hash(),
            hash_without_pending_input,
            "pending EXIT follows the existing external-command hash convention"
        );
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 1, 2, "abort.map", 0);
        let mut restored = GameSnapshot::load(&bytes).expect("v75 EXIT snapshot").sim;

        assert_eq!(
            restored.pending_command_snapshot(),
            sim.pending_command_snapshot()
        );
        assert!(!restored.quit_requested());
        assert_eq!(restored.take_executed_exit_owner(), None);
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn pending_game_speed_transition_roundtrips_in_current_version_and_executes_once() {
        use crate::sim::command::{Command, CommandEnvelope};
        use crate::sim::house_state::HouseState;

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        sim.houses
            .register(HouseState::new(owner, 0, None, true, 0, 10));
        let hash_without_pending_input = sim.state_hash();
        sim.queue_command(CommandEnvelope::new(
            owner,
            1,
            Command::SetGameSpeed { speed: 4 },
        ));
        assert_eq!(sim.state_hash(), hash_without_pending_input);

        let bytes = GameSnapshot::save(&sim, 1, 2, "speed.map", 0);
        let header = GameSnapshot::read_header(&bytes).expect("current GameSpeed header");
        assert_eq!(header.layout, snapshot_layout());
        let mut restored = GameSnapshot::load(&bytes)
            .expect("current GameSpeed snapshot")
            .sim;
        assert_eq!(
            restored.pending_command_snapshot(),
            sim.pending_command_snapshot()
        );
        assert_eq!(restored.session.game_options().game_speed, 1);
        assert_eq!(restored.projected_in_game_options_speed(), Some(4));

        let due = restored.take_due_commands();
        let result = restored.advance_tick(&due, None, None, None, 67);
        assert_eq!(result.executed_commands, 1);
        assert_eq!(restored.session.game_options().game_speed, 4);
        assert!(restored.pending_command_snapshot().is_empty());
        assert_eq!(result.state_hash, Some(restored.state_hash()));

        let second = restored.advance_tick(&[], None, None, None, 67);
        assert_eq!(second.executed_commands, 0);
        assert_eq!(restored.session.game_options().game_speed, 4);
    }

    #[test]
    fn pending_flag_to_die_snapshot_preserves_latch_shared_timer_and_hash() {
        use crate::sim::house_state::HouseState;
        let mut original = Simulation::with_seed(0);
        let owner = original.interner.intern("Americans");
        original
            .houses
            .register(HouseState::new(owner, 0, None, true, 0, 10));
        let before = original.state_hash();
        assert!(original.flag_house_to_die(owner));
        assert_ne!(original.state_hash(), before);
        let saved_hash = original.state_hash();
        let bytes = GameSnapshot::save(&original, 11, 22, "", 33);
        let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        restored
            .restore_after_snapshot_load()
            .expect("pending destruction is valid");
        assert_eq!(restored.state_hash(), saved_hash);
        let house = restored.houses.get_mut(&owner).unwrap();
        assert!(house.is_to_die());
        assert_eq!(
            house.outcome_timer(),
            crate::sim::timer::CdTimer::started(0, 0)
        );
        assert!(house.outcome().is_none());
        assert!(house.take_due_to_die(0));
        assert!(!house.take_due_to_die(0));
        assert_ne!(
            restored.state_hash(),
            saved_hash,
            "latch itself is hash-relevant"
        );
    }

    #[test]
    fn gsi_01_04_mid_savour_snapshot_preserves_remaining_frames_without_replaying_eva() {
        use crate::sim::house_state::{HouseOutcomeKind, HouseState};
        use crate::sim::world::SimSoundEvent;

        let mut original = Simulation::with_seed(0x104);
        original.session.set_tick_for_test(145);
        original.session.set_binary_frame_for_test(145);
        let owner = original.interner.intern("Americans");
        original
            .houses
            .register(HouseState::new(owner, 0, None, true, 10_000, 10));
        let before_outcome = original.state_hash();
        assert!(
            original
                .houses
                .get_mut(&owner)
                .expect("house")
                .accept_win_result(100, 90, false)
        );
        assert_ne!(
            original.state_hash(),
            before_outcome,
            "accepted outcome and Savour target are hash-relevant"
        );
        let accepted_hash = original.state_hash();
        original
            .houses
            .get_mut(&owner)
            .expect("house")
            .supply_allow_win_tag_state(0, crate::sim::timer::CdTimer::started(100, 91));
        assert_ne!(
            original.state_hash(),
            accepted_hash,
            "the shared result timer itself is hash-relevant"
        );
        original
            .houses
            .get_mut(&owner)
            .expect("house")
            .supply_allow_win_tag_state(0, crate::sim::timer::CdTimer::started(100, 90));
        original.sound_events.emit(SimSoundEvent::MatchOutcome {
            owner,
            kind: HouseOutcomeKind::Victory,
        });
        // Full snapshot load resets Scenario RNG to Seed0; compare hashes on
        // that canonical post-load cursor.
        original.scenario_rng = crate::sim::rng::SimRng::new(0);
        let saved_hash = original.state_hash();
        let bytes = GameSnapshot::save(&original, 11, 22, "", 33);
        let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;

        assert!(
            restored.sound_events.is_empty(),
            "the already-issued outcome EVA edge must not survive save/load"
        );
        restored
            .restore_after_snapshot_load()
            .expect("mid-Savour outcome state is structurally valid");
        assert_eq!(restored.state_hash(), saved_hash);

        let restored_house = restored.houses.get(&owner).expect("restored house");
        let outcome = restored_house.outcome().expect("accepted outcome");
        assert_eq!(outcome.kind, HouseOutcomeKind::Victory);
        assert!(restored.session().outcome_request().is_none());
        assert_eq!(
            restored_house
                .outcome_timer()
                .remaining(restored.session.binary_frame() as i32),
            45
        );

        assert_eq!(restored_house.pending_outcome(189, true), None);
        assert_eq!(
            restored_house.pending_outcome(190, true),
            Some(HouseOutcomeKind::Victory)
        );
    }

    #[test]
    fn gsi_01_04_conflicting_native_house_result_flags_are_rejected() {
        use crate::sim::house_state::HouseState;

        for (won, lost, to_die) in [
            (true, true, false),
            (true, false, true),
            (false, true, true),
        ] {
            let mut malformed = Simulation::new();
            let owner = malformed.interner.intern("Americans");
            let mut house = HouseState::new(owner, 0, None, true, 10_000, 10);
            if to_die {
                assert!(house.flag_to_die(0));
            }
            house.supply_result_flags_for_test(won, lost, crate::sim::timer::CdTimer::default());
            malformed.houses.register(house);

            let bytes = GameSnapshot::save(&malformed, 11, 22, "", 33);
            let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
            assert_eq!(
                restored.restore_after_snapshot_load(),
                Err(SnapshotRestoreError::InvalidHouseOutcomeState {
                    owner,
                    reason: if to_die {
                        "Flag_To_Die conflicts with a terminal result"
                    } else {
                        "Won and Lost flags conflict"
                    },
                }),
                "FlagWin/FlagLose cannot produce conflicting House flags"
            );
        }
    }

    #[test]
    fn defeated_house_without_a_result_restores() {
        use crate::sim::house_state::HouseState;

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let mut house = HouseState::new(owner, 0, None, false, 10_000, 10);
        house.set_is_defeated_for_test(true);
        sim.houses.register(house);
        let bytes = GameSnapshot::save(&sim, 11, 22, "", 33);
        let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        assert_eq!(restored.restore_after_snapshot_load(), Ok(()));
        assert!(restored.houses[&owner].is_defeated());
        assert!(restored.houses[&owner].outcome().is_none());
    }

    #[test]
    fn gsi_04_20_mid_fade_lighting_roundtrip_preserves_hash_and_next_rung() {
        let mut original = Simulation::with_seed(0x420);
        original.session.set_binary_frame_for_test(20);
        original
            .session
            .lighting_mut()
            .normal_mut_for_test()
            .red_percent = 93;
        original
            .session
            .lighting_mut()
            .ion_mut_for_test()
            .blue_percent = 71;
        original
            .session
            .lighting_mut()
            .set_current_ambient_for_test(95);
        original
            .session
            .lighting_mut()
            .set_target_ambient_for_test(87);
        original
            .session
            .lighting_mut()
            .set_selected_profile_for_test(
                crate::sim::scenario_session::ScenarioLightingProfile::Ion,
            );
        original
            .session
            .lighting_mut()
            .set_transition_timer_for_test(crate::sim::timer::CdTimer::from_raw(17, 3));

        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // lighting persistence on that same post-load cursor.
        original.scenario_rng = crate::sim::rng::SimRng::new(0);
        let saved_hash = original.state_hash();
        let bytes = GameSnapshot::save(&original, 11, 22, "", 33);
        let mut restored = GameSnapshot::load(&bytes)
            .expect("v65 lighting snapshot")
            .sim;

        assert_eq!(restored.session.lighting(), original.session.lighting());
        assert_eq!(restored.state_hash(), saved_hash);

        let original_due = original
            .session
            .lighting_mut()
            .advance_transition_if_due(20, true, 180, 20);
        let restored_due = restored
            .session
            .lighting_mut()
            .advance_transition_if_due(20, true, 180, 20);
        assert_eq!(restored_due, original_due);
        assert_eq!(restored.session.lighting(), original.session.lighting());
        assert_eq!(restored.state_hash(), original.state_hash());
    }

    /// `HouseClass+0x242` rides the raw House block in the native save; a
    /// v132 snapshot carries it and the restored hash still matches.
    #[test]
    fn house_harvester_no_ore_latch_roundtrips_at_v132() {
        let mut sim = Simulation::with_seed(0x242);
        let owner = sim.interner.intern("Americans");
        let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, true, 5_000, 10);
        house.set_harvester_no_ore_for_test(true);
        sim.houses.register(house);
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "house_no_ore_latch", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().layout,
            super::snapshot_layout()
        );
        let restored = GameSnapshot::load(&bytes)
            .expect("v132 house latch snapshot")
            .sim;
        assert!(restored.houses[&owner].harvester_no_ore());
        assert_eq!(restored.state_hash(), expected_hash);
    }

    /// `HouseClass+0x57D4` and the low-power guard ride the House block in
    /// the native save; a v133 snapshot carries them and the restored hash
    /// still matches.
    #[test]
    fn house_eva_advice_state_roundtrips_at_v133() {
        let mut sim = Simulation::with_seed(0x57D4);
        let owner = sim.interner.intern("Americans");
        let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, true, 5_000, 10);
        house.set_eva_funds_timer_for_test(crate::sim::timer::CdTimer::started(1_234, 2_880));
        house.set_eva_low_power_guard_for_test(true);
        sim.houses.register(house);
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "house_eva_advice", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().layout,
            super::snapshot_layout()
        );
        let restored = GameSnapshot::load(&bytes)
            .expect("v133 house EVA snapshot")
            .sim;
        assert_eq!(
            *restored.houses[&owner].eva_funds_timer(),
            crate::sim::timer::CdTimer::started(1_234, 2_880)
        );
        assert!(restored.houses[&owner].eva_low_power_guard());
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn gsi_04_18_spy_sat_latch_and_erased_map_knowledge_roundtrip_at_v65() {
        let mut sim = Simulation::with_seed(0x418);
        sim.fog.set_width_for_test(12);
        sim.fog.set_height_for_test(12);
        let owner = sim.interner.intern("Soviet");
        let gapper = sim.interner.intern("Americans");
        let mut house = crate::sim::house_state::HouseState::new(owner, 1, None, true, 10_000, 10);
        house.set_spy_sat_active_for_test(true);
        house.set_map_is_clear_for_test(true);
        sim.houses.register(house);
        sim.fog.reveal_all_for_owner(owner);
        crate::sim::vision::apply_gap_generators(
            &mut sim.fog,
            sim.house_alliances.as_map(),
            &[(gapper, 6, 6, 2)],
            &sim.interner,
        );
        assert!(sim.fog.is_cell_revealed(owner, 0, 0));
        assert!(!sim.fog.is_cell_revealed(owner, 6, 6));
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // SpySat/map-knowledge persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_18_shroud", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().layout,
            super::snapshot_layout()
        );
        let restored = GameSnapshot::load(&bytes).expect("v65 SpySat snapshot").sim;

        assert!(restored.houses[&owner].spy_sat_active());
        assert!(restored.houses[&owner].map_is_clear());
        assert!(restored.fog.is_cell_revealed(owner, 0, 0));
        assert!(!restored.fog.is_cell_revealed(owner, 6, 6));
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn gsi_04_15_active_tube_detachment_and_exact_z_roundtrip_hash() {
        use crate::map::tube_facts::TubeId;
        use crate::sim::components::DriveCoord;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::movement::tube_movement::LowBridgeTubeMovementState;

        let mut sim = Simulation::new();
        let entity_id = sim.substrate.allocate_stable_id();
        let mut entity = GameEntity::test_default(entity_id, "MTNK", "Americans", 5, 7);
        entity.lifecycle.set_in_limbo_for_test(false);
        entity.lifecycle.set_cell_marked_for_test(false);
        entity.in_logic_vector.set_for_test(true);
        entity.position.set_z_for_test(3);
        entity.position.set_exact_z_leptons_for_test(Some(-37));
        entity.low_bridge_tube_state = Some(LowBridgeTubeMovementState::for_test(
            TubeId(9),
            2,
            DriveCoord {
                x: 1_536,
                y: 2_048,
                z: -19,
            },
        ));
        sim.substrate.entities.insert(entity);
        sim.substrate
            .logic
            .try_push(entity_id)
            .expect("active TubeMovement fixture enters LogicClass order");
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // TubeMovement persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_15_tube", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().layout,
            super::snapshot_layout()
        );
        let mut restored = GameSnapshot::load(&bytes)
            .expect("current tube snapshot")
            .sim;
        restored
            .restore_after_snapshot_load()
            .expect("active detached tube object restores");

        let restored_entity = restored
            .substrate
            .entities
            .get(entity_id)
            .expect("tube mover restored");
        assert_eq!(restored_entity.position.z(), 3);
        assert_eq!(restored_entity.position.exact_z_leptons(), Some(-37));
        assert_eq!(
            restored_entity.low_bridge_tube_state,
            Some(LowBridgeTubeMovementState::for_test(
                TubeId(9),
                2,
                DriveCoord {
                    x: 1_536,
                    y: 2_048,
                    z: -19,
                }
            ))
        );
        assert!(!restored_entity.lifecycle.cell_marked());
        assert!(
            !restored
                .substrate
                .occupancy
                .contains_entity(5, 7, entity_id)
        );
        assert_eq!(restored.substrate.raw_cell_occupation.ground_bits(5, 7), 0);
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn gsi_04_07_wall_sell_pending_command_house_mode_roundtrip_and_hash() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Receiver");
        let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, false, 0, 10);
        house.set_player_control_for_test(true);
        sim.houses.register(house);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.queue_command(crate::sim::command::CommandEnvelope::new(
            owner,
            17,
            crate::sim::command::Command::SellWallAtCell { x: -3, y: 9 },
        ));
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // pending-command/house-mode persistence on that post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_player_control_for_test(false);
        assert_ne!(sim.state_hash(), expected_hash);
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_player_control_for_test(true);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false),
        );
        assert_ne!(sim.state_hash(), expected_hash);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );

        let bytes = GameSnapshot::save(&sim, 1, 2, "wall.map", 0);
        let restored = GameSnapshot::load(&bytes)
            .expect("v60 wall-sale snapshot")
            .sim;
        assert_eq!(
            restored.pending_command_snapshot(),
            sim.pending_command_snapshot()
        );
        assert!(restored.houses.get(&owner).unwrap().player_control());
        assert!(restored.session.game_mode_nonzero());
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn gsi_04_07_damage_v60_air_spatial_armor_berserk_hostile_hit_iq_smoke_capture_anger_and_delay_roundtrip_hash()
     {
        let mut sim = Simulation::new();
        let entity_id = sim.substrate.allocate_stable_id();
        let mut aircraft = crate::sim::game_entity::GameEntity::test_default(
            entity_id,
            "ORCA",
            "AMERICANS",
            11,
            7,
        );
        aircraft = aircraft.with_air_spatial_membership_for_test(Some(143), 91);
        aircraft.crate_multipliers.set_armor_multiplier_for_test(
            crate::util::native_x87::NativeF64Bits::from_bits(1.5_f64.to_bits()),
        );
        aircraft.berserk.set_active_for_test(true);
        aircraft.berserk.set_timer_for_test(-17);
        aircraft
            .ai_sell_repair
            .set_was_attacked_by_enemy_for_test(true);
        aircraft.damage_smoke_system_id.set_for_test(Some(2));
        aircraft.capture_manager = Some(
            crate::sim::capture_manager::CaptureManagerState::with_victims_for_test(
                3,
                false,
                &[3, 4],
            ),
        );
        aircraft.pending_c4_detonation.set_for_test(Some(
            crate::sim::c4_plant::PendingC4Detonation::new(
                crate::sim::timer::CdTimer::started(11, 35),
                Some(3),
            ),
        ));
        sim.substrate.entities.insert(aircraft);
        sim.substrate
            .entities
            .insert(crate::sim::game_entity::GameEntity::test_default(
                3,
                "E1",
                "AMERICANS",
                12,
                7,
            ));
        sim.substrate
            .entities
            .insert(crate::sim::game_entity::GameEntity::test_default(
                4,
                "E1",
                "AMERICANS",
                13,
                7,
            ));
        sim.substrate.particle_systems.insert({
            let mut system = crate::sim::particles::ParticleSystem::test_fixture(
                2,
                crate::rules::particle_system_type::ParticleSystemTypeId(0),
                glam::IVec3::ZERO,
            );
            system.set_offset_for_test(glam::IVec3::ZERO);
            system.set_spawn_timer_for_test(crate::util::native_x87::NativeF32Bits::ONE);
            system.set_owner_entity_for_test(Some(entity_id));
            system.set_target_coords_for_test(glam::IVec3::ZERO);
            system
        });
        sim.set_next_stable_object_id_for_test(5);
        let owner = sim.interner.intern("ComputerIQ");
        let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, false, 0, 51);
        house.set_current_iq_for_test(2);
        let threat_peer = sim.interner.intern("ThreatPeer");
        house.grudge_scores_mut_for_test().insert(threat_peer, 350);
        house.set_enemy_house_for_test(Some(threat_peer));
        sim.houses.register(house);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                threat_peer,
                1,
                None,
                false,
                0,
                51,
            ));
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // damage-authority persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        {
            let entity = sim.substrate.entities.get_mut(entity_id).unwrap();
            *entity = entity
                .clone()
                .with_air_spatial_membership_for_test(Some(143), 92);
        }
        assert_ne!(sim.state_hash(), expected_hash, "vector order is hashed");
        {
            let entity = sim.substrate.entities.get_mut(entity_id).unwrap();
            *entity = entity
                .clone()
                .with_air_spatial_membership_for_test(Some(144), 91);
        }
        assert_ne!(sim.state_hash(), expected_hash, "bucket identity is hashed");
        {
            let entity = sim.substrate.entities.get_mut(entity_id).unwrap();
            *entity = entity
                .clone()
                .with_air_spatial_membership_for_test(Some(143), 91);
        }
        assert_eq!(sim.state_hash(), expected_hash);
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .crate_multipliers
            .set_armor_multiplier_for_test(crate::util::native_x87::NativeF64Bits::ONE);
        assert_ne!(sim.state_hash(), expected_hash, "instance armor is hashed");
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .crate_multipliers
            .set_armor_multiplier_for_test(crate::util::native_x87::NativeF64Bits::from_bits(
                1.5_f64.to_bits(),
            ));
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .berserk
            .set_timer_for_test(-18);
        assert_ne!(sim.state_hash(), expected_hash, "berserk timer is hashed");
        {
            let entity = sim.substrate.entities.get_mut(entity_id).unwrap();
            entity.berserk.set_timer_for_test(-17);
            entity.berserk.set_active_for_test(false);
        }
        assert_ne!(
            sim.state_hash(),
            expected_hash,
            "berserk active byte is hashed"
        );
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .berserk
            .set_active_for_test(true);
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .ai_sell_repair
            .set_was_attacked_by_enemy_for_test(false);
        assert_ne!(
            sim.state_hash(),
            expected_hash,
            "hostile-hit latch is hashed"
        );
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .ai_sell_repair
            .set_was_attacked_by_enemy_for_test(true);
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_current_iq_for_test(1);
        assert_ne!(sim.state_hash(), expected_hash, "CurrentIQ is hashed");
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_current_iq_for_test(2);
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .grudge_scores_mut_for_test()
            .insert(threat_peer, 351);
        assert_ne!(sim.state_hash(), expected_hash, "anger score is hashed");
        {
            let house = sim.houses.get_mut(&owner).unwrap();
            house.grudge_scores_mut_for_test().insert(threat_peer, 350);
            house.set_enemy_house_for_test(None);
        }
        assert_ne!(
            sim.state_hash(),
            expected_hash,
            "selected enemy identity is hashed"
        );
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_enemy_house_for_test(Some(threat_peer));
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .damage_smoke_system_id
            .set_for_test(None);
        assert_ne!(
            sim.state_hash(),
            expected_hash,
            "damage-Smoke identity is hashed"
        );
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .damage_smoke_system_id
            .set_for_test(Some(2));
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .capture_manager
            .as_mut()
            .unwrap()
            .reverse_nodes_for_test();
        assert_ne!(sim.state_hash(), expected_hash, "MCNode order is hashed");
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .capture_manager
            .as_mut()
            .unwrap()
            .reverse_nodes_for_test();

        let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_07_air_spatial", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().layout,
            super::snapshot_layout()
        );
        let mut restored = GameSnapshot::load(&bytes).expect("v60 snapshot").sim;
        restored
            .restore_after_snapshot_load()
            .expect("damage-Smoke pointer resolves through ParticleSystemStore");
        let entity = restored.substrate.entities.get(entity_id).unwrap();
        assert_eq!(entity.air_spatial_bucket(), Some(143));
        assert_eq!(entity.air_spatial_enter_order(), 91);
        assert_eq!(entity.crate_multipliers.armor().bits(), 1.5_f64.to_bits());
        assert!(entity.berserk.is_active());
        assert_eq!(entity.berserk.timer(), -17);
        assert!(entity.ai_sell_repair.was_attacked_by_enemy());
        assert_eq!(entity.damage_smoke_system_id.system(), Some(2));
        assert_eq!(
            entity.pending_c4_detonation.pending(),
            Some(crate::sim::c4_plant::PendingC4Detonation::new(
                crate::sim::timer::CdTimer::started(11, 35),
                Some(3)
            ))
        );
        assert_eq!(
            entity
                .capture_manager
                .as_ref()
                .map(|manager| manager.victims().collect::<Vec<_>>()),
            Some(vec![3, 4])
        );
        assert_eq!(restored.houses.get(&owner).unwrap().current_iq(), 2);
        assert_eq!(
            restored.houses[&owner].grudge_scores().get(&threat_peer),
            Some(&350)
        );
        assert_eq!(restored.houses[&owner].enemy_house(), Some(threat_peer));
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn gsi_04_12_raw_occupation_snapshot_roundtrip_preserves_both_planes() {
        let mut sim = Simulation::new();
        let ground_owner = sim.interner.intern("Americans");
        let deck_owner = sim.interner.intern("Russians");
        sim.substrate.raw_cell_occupation.mark_ground(17, 23, 0x23);
        sim.substrate.raw_cell_occupation.mark_deck(17, 23, 0xC4);
        sim.substrate
            .raw_cell_occupation
            .mark_ground_infantry(17, 23, 0x04, ground_owner);
        sim.substrate
            .raw_cell_occupation
            .mark_deck_infantry(17, 23, 0x08, deck_owner);
        sim.substrate.raw_cell_occupation.mark_ground(2, 31, 0x02);
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // occupation-plane persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_12_raw_occupation", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes)
                .expect("current snapshot header")
                .layout,
            super::snapshot_layout()
        );

        let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        restored
            .restore_after_snapshot_load()
            .expect("restore transient caches without replacing raw bytes");

        assert_eq!(
            restored.substrate.raw_cell_occupation.ground_bits(17, 23),
            0x27
        );
        assert_eq!(
            restored.substrate.raw_cell_occupation.deck_bits(17, 23),
            0xCC
        );
        assert_eq!(
            restored
                .substrate
                .raw_cell_occupation
                .ground_infantry_owner(17, 23),
            Some(ground_owner)
        );
        assert_eq!(
            restored
                .substrate
                .raw_cell_occupation
                .deck_infantry_owner(17, 23),
            Some(deck_owner)
        );
        assert_eq!(
            restored.substrate.raw_cell_occupation.ground_bits(2, 31),
            0x02
        );
        assert_eq!(restored.state_hash(), expected_hash);
    }

    /// The cell `AltObject` slots (`CellClass+0xE0`) are a serialized authority:
    /// which owner holds a cell, and when it was released, cannot be rebuilt
    /// from entity positions or locomotor phase, so they must survive a save
    /// verbatim and reach the state hash.
    #[test]
    fn air_slot_snapshot_roundtrip_preserves_the_cell_holders() {
        let empty_hash = Simulation::new().state_hash();
        let mut sim = Simulation::new();
        assert!(sim.substrate.set_cell_air_slot((17, 23), Some(41)));
        assert!(sim.substrate.set_cell_air_slot((2, 31), Some(99)));
        // `0x00487D70` refuses a claim while another object holds the slot,
        // which is what keeps one hovering Jumpjet per cell.
        assert!(!sim.substrate.set_cell_air_slot((17, 23), Some(42)));
        assert_eq!(sim.substrate.air_slots.holder(17, 23), Some(41));
        let expected_hash = sim.state_hash();
        assert_ne!(
            expected_hash, empty_hash,
            "held air slots must reach the state hash"
        );

        let bytes = GameSnapshot::save(&sim, 0, 0, "air_slot_roundtrip", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes)
                .expect("current snapshot header")
                .layout,
            super::snapshot_layout()
        );

        let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        restored
            .restore_after_snapshot_load()
            .expect("restore transient caches without replacing the slots");

        assert_eq!(restored.substrate.air_slots.holder(17, 23), Some(41));
        assert_eq!(restored.substrate.air_slots.holder(2, 31), Some(99));
        assert_eq!(restored.substrate.air_slots.holder(5, 5), None);

        // The post-restore hash check is differential on purpose: a bare
        // `Simulation::new()` does not round-trip its own state hash, which a
        // no-slot control sim shows just as well, so comparing `restored`
        // against `expected_hash` would be measuring that instead. What matters
        // here is that the restored slots are still hash-visible.
        let control = GameSnapshot::save(&Simulation::new(), 0, 0, "air_slot_control", 0);
        let mut control = GameSnapshot::load(&control).expect("control snapshot").sim;
        control
            .restore_after_snapshot_load()
            .expect("restore the control");
        assert_ne!(
            restored.state_hash(),
            control.state_hash(),
            "restored air slots must still reach the state hash"
        );
    }

    /// Native r8 slot_claim_replacement_release retains+560=(10,10) while
    /// replacement_claim stores+564=(11,10). A save must retain both Cells and
    /// continue the executed same-owner/empty-release behavior from that point.
    #[test]
    fn foot_air_distinct_native_caches_roundtrip_and_continue_notifications() {
        use crate::map::entities::EntityCategory;
        use crate::sim::combat::Health;
        use crate::sim::game_entity::GameEntity;

        let mut sim = Simulation::with_seed(0);
        sim.session.set_map_width_for_test(40);
        sim.session.set_map_height_for_test(40);
        let id = sim.substrate.allocate_stable_id();
        let owner = sim.intern("Americans");
        let type_ref = sim.intern("SHAD");
        let entity = GameEntity::new_at_frame_zero_for_test(
            id,
            10,
            10,
            0,
            0,
            owner,
            Health::new(100),
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        sim.substrate.entities.insert(entity);
        sim.add_entity_occupancy(id);
        Simulation::aircraft_tracker_add(&sim.session, &mut sim.substrate, id);
        assert!(sim.substrate.set_cell_air_slot((10, 10), Some(id)));
        assert!(sim.substrate.set_cell_air_slot((11, 10), Some(id)));
        let retained = sim.substrate.entities.get(id).unwrap().clone();
        let expected_hash = sim.state_hash();

        // Independent values affect lockstep even when the raw grid, vector
        // membership, position and the other cache are unchanged.
        *sim.substrate.entities.get_mut(id).unwrap() =
            retained.clone().with_air_tracker_cell_for_test((11, 10));
        assert_ne!(
            sim.state_hash(),
            expected_hash,
            "Foot+560 is independently hashed"
        );
        *sim.substrate.entities.get_mut(id).unwrap() =
            retained.clone().with_air_slot_cell_for_test((10, 10));
        assert_ne!(
            sim.state_hash(),
            expected_hash,
            "Foot+564 is independently hashed"
        );
        *sim.substrate.entities.get_mut(id).unwrap() = retained;

        let bytes = GameSnapshot::save(&sim, 0, 0, "foot_air_caches", 0);
        let mut restored = GameSnapshot::load(&bytes)
            .expect("v290 Foot air snapshot")
            .sim;
        restored
            .restore_after_snapshot_load()
            .expect("restore Foot air continuation");
        let actor = restored.substrate.entities.get(id).unwrap();
        assert_eq!(actor.air_tracker_cell(), (10, 10));
        assert_eq!(actor.air_slot_cell(), (11, 10));
        assert_eq!(restored.substrate.air_slots.holder(10, 10), None);
        assert_eq!(restored.substrate.air_slots.holder(11, 10), Some(id));
        assert_eq!(restored.state_hash(), expected_hash);

        for owner in [Some(id), None] {
            assert!(sim.substrate.set_cell_air_slot((11, 10), owner));
            assert!(restored.substrate.set_cell_air_slot((11, 10), owner));
            assert_eq!(restored.substrate.air_slots.holder(11, 10), None);
            let actor = restored.substrate.entities.get(id).unwrap();
            assert_eq!(actor.air_tracker_cell(), (10, 10));
            assert_eq!(actor.air_slot_cell(), (11, 10));
            assert_eq!(restored.state_hash(), sim.state_hash());
        }

        // A non-null slot Cell can coexist with an empty raw grid. Save that
        // executed stale-cache state too; load must not infer+564 from Cell+E0.
        let stale = GameSnapshot::save(&restored, 0, 0, "foot_air_stale_slot", 0);
        let mut stale = GameSnapshot::load(&stale)
            .expect("stale Foot slot snapshot")
            .sim;
        stale
            .restore_after_snapshot_load()
            .expect("restore stale slot cache");
        assert_eq!(
            stale.substrate.entities.get(id).unwrap().air_slot_cell(),
            (11, 10)
        );
        assert_eq!(
            stale.substrate.entities.get(id).unwrap().air_tracker_cell(),
            (10, 10)
        );
        assert_eq!(stale.substrate.air_slots.holder(11, 10), None);
        assert_eq!(stale.state_hash(), restored.state_hash());
        assert!(stale.substrate.set_cell_air_slot((12, 10), Some(id)));
        assert!(stale.substrate.set_cell_air_slot((12, 10), None));
        assert_eq!(
            stale.substrate.entities.get(id).unwrap().air_slot_cell(),
            (0, 0)
        );
        assert_eq!(
            stale.substrate.entities.get(id).unwrap().air_tracker_cell(),
            (10, 10)
        );
    }

    /// The executed unit_uninit_live_slot control retires the live Foot while
    /// keeping+564 and its occupied Cell+E0. Only the later selected destructor
    /// block clears that matching slot; a save between these phases is valid.
    #[test]
    fn foot_air_retired_cached_slot_survives_restore_until_physical_destruction() {
        use crate::map::entities::EntityCategory;
        use crate::sim::combat::Health;
        use crate::sim::game_entity::GameEntity;

        let mut sim = Simulation::with_seed(0);
        sim.session.set_map_width_for_test(40);
        sim.session.set_map_height_for_test(40);
        let owner = sim.intern("Americans");
        let type_ref = sim.intern("SHAD");
        for rx in [10, 12] {
            let id = sim.substrate.allocate_stable_id();
            let entity = GameEntity::new_at_frame_zero_for_test(
                id,
                rx,
                10,
                0,
                0,
                owner,
                Health::new(100),
                type_ref,
                EntityCategory::Unit,
                0,
                5,
                true,
            );
            sim.substrate.entities.insert(entity);
            sim.add_entity_occupancy(id);
        }
        Simulation::aircraft_tracker_add(&sim.session, &mut sim.substrate, 1);
        assert!(sim.substrate.set_cell_air_slot((10, 10), Some(1)));
        assert!(sim.substrate.set_cell_air_slot((12, 10), Some(2)));
        let foreign_cache = sim.substrate.entities.get(2).unwrap().air_slot_cell();
        sim.uninit(1);
        let retired_hash = sim.state_hash();
        assert_eq!(sim.substrate.air_slots.holder(10, 10), Some(1));

        let bytes = GameSnapshot::save(&sim, 0, 0, "retired_foot_air_slot", 0);
        let mut restored = GameSnapshot::load(&bytes)
            .expect("retired Foot snapshot")
            .sim;
        restored
            .restore_after_snapshot_load()
            .expect("restore retired Foot before drain");
        let actor = restored.substrate.entities.get(1).unwrap();
        assert!(!actor.lifecycle.object_alive());
        assert!(actor.lifecycle.in_limbo());
        assert_eq!(actor.air_tracker_cell(), (0, 0));
        assert_eq!(actor.air_slot_cell(), (10, 10));
        assert_eq!(restored.substrate.air_slots.holder(10, 10), Some(1));
        assert_eq!(restored.substrate.pending_delete.object_ids(), vec![1]);
        assert_eq!(restored.state_hash(), retired_hash);

        // This exercises the existing Rust drain and shared finalization
        // integration; native proof here is bounded to the slot-cache block.
        restored.process_pending_delete();
        sim.process_pending_delete();
        assert!(restored.substrate.entities.get(1).is_none());
        assert_eq!(restored.substrate.air_slots.holder(10, 10), None);
        assert_eq!(restored.substrate.air_slots.holder(12, 10), Some(2));
        assert_eq!(
            restored.substrate.entities.get(2).unwrap().air_slot_cell(),
            foreign_cache
        );
        assert!(restored.substrate.pending_delete.is_empty());
        assert_eq!(restored.state_hash(), sim.state_hash());
    }

    #[test]
    fn cell_fog_sensor_and_cloak_state_roundtrips_with_hash() {
        use crate::map::entities::EntityCategory;
        use crate::sim::cloak_disguise::CloakRuntime;
        use crate::sim::combat::Health;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::sensor_lifecycle::SensorDeposit;

        let mut sim = Simulation::new();
        let viewer = sim.interner.intern("AMERICANS");
        let source_owner = sim.interner.intern("RUSSIANS");
        let type_ref = sim.interner.intern("SUB");
        sim.fog.set_width_for_test(8);
        sim.fog.set_height_for_test(8);
        sim.fog
            .insert_fogged_object_footprint(viewer, 91, vec![(3, 3), (4, 3)]);
        sim.fog.sensors_add_at(viewer, (3, 3), 2);
        assert!(sim.fog.set_cloaked_by_house(7, 3, 3));
        assert!(
            sim.fog
                .draw_objects_cloaked(Some(source_owner), source_owner, 7, 3, 3)
        );
        let entity_id = sim.substrate.allocate_stable_id();
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            entity_id,
            3,
            3,
            0,
            0,
            source_owner,
            Health::new(600),
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            false,
        );
        let mut cloak = CloakRuntime::new(0);
        cloak.establish_unlimbo_fully_cloaked();
        entity.cloak = Some(cloak);
        entity.sensor_deposit = Some(SensorDeposit::unit(viewer, (3, 3), 2));
        sim.substrate.entities.insert(entity);
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // fog/sensor/cloak persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "cell_fog_sensor_cloak", 0);
        let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;

        assert_eq!(restored.state_hash(), expected_hash);
        assert_eq!(restored.fog.fogged_objects().len(), 1);
        assert!(restored.fog.has_sensor_for_house(viewer, 3, 3));
        assert!(restored.fog.is_cloaked_by_house(7, 3, 3));
        let restored_entity = restored.substrate.entities.get(entity_id).unwrap();
        assert_eq!(
            restored_entity.cloak,
            sim.substrate.entities.get(entity_id).unwrap().cloak
        );
        assert_eq!(
            restored_entity.sensor_deposit,
            sim.substrate
                .entities
                .get(entity_id)
                .unwrap()
                .sensor_deposit
        );
        let same_hash = restored.state_hash();
        restored
            .substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .sensor_deposit
            .as_mut()
            .unwrap()
            .set_center_for_test((4, 3));
        assert_ne!(restored.state_hash(), same_hash);
    }

    #[test]
    fn gsi_04_05_hidden_snapshot_and_hash_preserve_counter_and_exit_profile() {
        use crate::map::entities::EntityCategory;
        use crate::sim::game_entity::GameEntity;

        let mut sim = Simulation::new();
        sim.session.set_map_width_for_test(32);
        sim.session.set_map_height_for_test(32);
        let entity_id = sim.substrate.allocate_stable_id();
        let mut building = GameEntity::test_default_of_category(
            entity_id,
            "GAREFN",
            "AMERICANS",
            10,
            10,
            EntityCategory::Structure,
        );
        building
            .type_facts
            .set_foundation_for_test("4x3".to_string());
        let mut profile = crate::rules::object_type::BuildingHiddenOccupancyProfile::default();
        profile.add_occupy[0] = Some((-1, 0));
        profile.add_occupy[1] = Some((-1, -1));
        profile.remove_occupy[0] = Some((3, 1));
        building
            .type_facts
            .set_building_hidden_occupancy_for_test(Some(profile));
        sim.substrate.entities.insert(building);
        sim.add_entity_occupancy(entity_id);

        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // hidden-occupation persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();
        sim.substrate
            .hidden_occupation
            .exit_building((10, 10), "4x3", profile, Some((32, 32)));
        assert_ne!(sim.state_hash(), expected_hash);
        sim.substrate
            .hidden_occupation
            .enter_building((10, 10), "4x3", profile, Some((32, 32)));
        let mut stored = profile;
        stored.occupy_height = 4;
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .type_facts
            .set_building_hidden_occupancy_for_test(Some(stored));
        assert_ne!(sim.state_hash(), expected_hash);
        stored.occupy_height = 2;
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .type_facts
            .set_building_hidden_occupancy_for_test(Some(stored));
        assert_eq!(sim.state_hash(), expected_hash);

        let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_05_hidden", 0);
        let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        restored
            .restore_after_snapshot_load()
            .expect("restore skipped cell-list caches");

        assert_eq!(restored.substrate.hidden_occupation.count(9, 9), 1);
        assert_eq!(restored.substrate.hidden_occupation.count(13, 11), 0);
        assert_eq!(
            restored
                .substrate
                .entities
                .get(entity_id)
                .unwrap()
                .type_facts
                .building_hidden_occupancy(),
            Some(profile)
        );
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn gsi_04_05_reservation_roundtrip_preserves_real_and_house_state_but_clears_dummy() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("AMERICANS");
        let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10);
        house.base_reservation_mut().update_bounds(3, 4, 5, 6);
        house
            .base_reservation_mut()
            .append_perimeter_cell_if_absent(u32::from(3u16) | (u32::from(4u16) << 16));
        sim.houses.register(house);
        let entity_id = sim.substrate.allocate_stable_id();
        let mut building = crate::sim::game_entity::GameEntity::test_default_of_category(
            entity_id,
            "GAPOWR",
            "AMERICANS",
            3,
            4,
            crate::map::entities::EntityCategory::Structure,
        );
        building.set_owner_for_test(owner);
        building
            .type_facts
            .set_base_reservation_spacing_for_test(Some(-3));
        sim.substrate.entities.insert(building);
        let mut reservation_terrain = flat_terrain(4, 5);
        reservation_terrain.test_set_native_allocated_cells(&[(3, 4)]);
        sim.resolved_terrain = Some(reservation_terrain);
        sim.substrate
            .base_reservations
            .reserve(sim.resolved_terrain.as_ref(), 3, 4, 0);
        sim.substrate
            .base_reservations
            .reserve(sim.resolved_terrain.as_ref(), 1, 0, 1);
        assert_eq!(
            sim.substrate
                .base_reservations
                .raw_mask(sim.resolved_terrain.as_ref(), 2, 0),
            1 << 1,
            "distinct valid-linear null slots share the dummy before save"
        );
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // base-reservation persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        sim.substrate
            .base_reservations
            .clear(sim.resolved_terrain.as_ref(), 3, 4, 0);
        assert_ne!(sim.state_hash(), expected_hash, "real mask is hashed");
        sim.substrate
            .base_reservations
            .reserve(sim.resolved_terrain.as_ref(), 3, 4, 0);
        sim.substrate
            .base_reservations
            .clear(sim.resolved_terrain.as_ref(), 2, 0, 1);
        assert_ne!(
            sim.state_hash(),
            expected_hash,
            "valid-linear native-unallocated shared dummy mask is hashed"
        );
        sim.substrate
            .base_reservations
            .reserve(sim.resolved_terrain.as_ref(), 1, 0, 1);
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .type_facts
            .set_base_reservation_spacing_for_test(Some(9));
        assert_ne!(
            sim.state_hash(),
            expected_hash,
            "entity writer profile is hashed"
        );
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .type_facts
            .set_base_reservation_spacing_for_test(Some(-3));
        assert_eq!(sim.state_hash(), expected_hash);

        let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_05_reservation", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().layout,
            super::snapshot_layout()
        );
        sim.substrate
            .base_reservations
            .clear(sim.resolved_terrain.as_ref(), 2, 0, 1);
        let expected_loaded_hash = sim.state_hash();

        let mut restored = GameSnapshot::load(&bytes)
            .expect("reservation snapshot")
            .sim;
        restored
            .restore_after_snapshot_load()
            .expect("restore transient caches without rebuilding reservations");
        assert_eq!(restored.substrate.base_reservations.raw_mask(None, 3, 4), 1);
        assert_eq!(restored.substrate.base_reservations.dummy_mask(), 0);
        let restored_house = restored.houses.get(&owner).expect("restored owner");
        assert_eq!(restored_house.base_reservation().bounds(), (3, 4, 5, 6));
        assert_eq!(
            restored_house.base_reservation().perimeter_cells(),
            &[u32::from(3u16) | (u32::from(4u16) << 16)]
        );
        assert_eq!(
            restored
                .substrate
                .entities
                .get(entity_id)
                .unwrap()
                .type_facts
                .base_reservation_spacing(),
            Some(-3)
        );
        assert_eq!(restored.state_hash(), expected_loaded_hash);
    }

    #[test]
    fn gsi_04_05_v40_roundtrip_restores_drive_runtime_and_cleared_foot_occupation() {
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::movement::DriveLocomotionRuntime;
        use crate::sim::movement::locomotor::LocomotorState;

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("AMERICANS");
        let type_ref = sim.interner.intern("MTNK");
        let entity_id = sim.substrate.allocate_stable_id();
        let mut entity = GameEntity::test_default(entity_id, "MTNK", "AMERICANS", 2, 2);
        entity.set_owner_for_test(owner);
        entity.set_type_ref_for_test(type_ref);
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        sim.substrate.entities.insert(entity);
        sim.add_entity_occupancy(entity_id);

        sim.substrate
            .entities
            .get_mut(entity_id)
            .expect("Drive unit")
            .locomotor
            .as_mut()
            .unwrap()
            .install_drive_state_for_test(Some(DriveLocomotionRuntime::default()));
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .set_foot_occupation_enabled(false);
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // Drive and Foot enable persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_05", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes)
                .expect("current snapshot header")
                .layout,
            super::snapshot_layout()
        );

        let mut restored_a = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        let mut restored_b = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        restored_a
            .restore_after_snapshot_load()
            .expect("first deterministic rebuild");
        restored_b
            .restore_after_snapshot_load()
            .expect("second deterministic rebuild");

        for restored in [&restored_a, &restored_b] {
            assert!(
                restored
                    .substrate
                    .entities
                    .get(entity_id)
                    .expect("restored Drive unit")
                    .locomotor
                    .as_ref()
                    .and_then(|loco| loco.selected_drive_runtime())
                    .and_then(|runtime| runtime.retained())
                    .is_some(),
                "restored Drive runtime"
            );
            assert!(
                !restored
                    .substrate
                    .entities
                    .get(entity_id)
                    .unwrap()
                    .foot_occupation_enabled()
            );
            assert_eq!(restored.state_hash(), expected_hash);
        }
    }

    #[test]
    fn gsi_04_07_v40_roundtrip_restores_wall_owner() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("AMERICANS");
        let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(8, 8);
        overlays.place_owned_wall(3, 4, 2, 0x1A, owner);
        sim.overlay_grid = Some(overlays);
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // wall-owner persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();
        let mut unowned = Simulation::new();
        unowned.scenario_rng = crate::sim::rng::SimRng::new(0);
        let _ = unowned.interner.intern("AMERICANS");
        let mut unowned_overlays = crate::sim::overlay_grid::OverlayGrid::new(8, 8);
        unowned_overlays.place_overlay(3, 4, 2, 0x1A);
        unowned.overlay_grid = Some(unowned_overlays);
        assert_ne!(
            unowned.state_hash(),
            expected_hash,
            "wall ownership participates in deterministic state"
        );

        let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_07", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes)
                .expect("current snapshot header")
                .layout,
            super::snapshot_layout()
        );
        let restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        let cell = restored
            .overlay_grid
            .as_ref()
            .expect("overlay grid")
            .cell(3, 4);
        assert_eq!(cell.overlay_id, Some(2));
        assert_eq!(cell.overlay_data, 0x1A);
        assert_eq!(cell.wall_owner, Some(owner));
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn validated_load_requires_exact_content_and_session_metadata() {
        use crate::sim::command::{Command, CommandEnvelope};

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("AMERICANS");
        sim.session.set_map_name_for_test("MAP01.MAP".to_string());
        sim.session.set_tick_for_test(0x1_0000_0007);
        sim.session.set_binary_frame_for_test(u32::MAX - 2);
        sim.session.set_total_sim_ms_for_test(12_345);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, false, 0, 10,
            ));
        sim.queue_command(CommandEnvelope::new(
            owner,
            sim.session.tick() + 3,
            Command::Stop { entity_id: 71 },
        ));
        sim.scenario_rng.next_u32();
        sim.main_rng.next_u32();
        sim.mapgen_rng.next_u32();
        let process_default = crate::sim::rng::SimRng::new(0).logical_state();
        assert_ne!(sim.rng_state().scenario, process_default);

        let bytes = GameSnapshot::save_validated(&sim, 0x11, 0x22, "Campaign foothold", 0x33);
        let restored =
            GameSnapshot::load_validated(&bytes, 0x11, 0x22, "MAP01.MAP").expect("exact metadata");
        assert_eq!(restored.tick, sim.session.tick());
        assert_eq!(restored.map_name, sim.session.map_name());
        assert_eq!(restored.sim.session.binary_frame(), u32::MAX - 2);
        assert_eq!(restored.sim.session.total_sim_ms(), 12_345);
        assert_eq!(restored.sim.houses.order(), vec![owner]);
        assert_eq!(
            restored.sim.pending_command_snapshot(),
            sim.pending_command_snapshot()
        );
        assert_eq!(
            restored.sim.scenario_rng.logical_state(),
            process_default,
            "native post-read Scenario reinitialization must reset the saved cursor"
        );
        assert_eq!(restored.sim.main_rng.logical_state(), process_default);
        assert_eq!(restored.sim.mapgen_rng.logical_state(), process_default);

        assert!(matches!(
            GameSnapshot::load_validated(&bytes, 0x12, 0x22, "MAP01.MAP"),
            Err(SnapshotError::MapMismatch { .. })
        ));
        assert!(matches!(
            GameSnapshot::load_validated(&bytes, 0x11, 0x23, "MAP01.MAP"),
            Err(SnapshotError::RulesMismatch { .. })
        ));
        assert!(matches!(
            GameSnapshot::load_validated(&bytes, 0x11, 0x22, "MAP02.MAP"),
            Err(SnapshotError::MapNameMismatch { .. })
        ));

        let mut inconsistent = GameSnapshot::load_unchecked(&bytes).expect("current snapshot");
        inconsistent.tick = inconsistent.tick.wrapping_add(1);
        let inconsistent_bytes =
            save_codec::serialize(&inconsistent).expect("corrupt tick metadata");
        assert!(matches!(
            GameSnapshot::load_validated(&inconsistent_bytes, 0x11, 0x22, "MAP01.MAP"),
            Err(SnapshotError::TickMetadataMismatch { .. })
        ));

        inconsistent.tick = inconsistent.sim.session.tick();
        inconsistent
            .sim
            .session
            .set_map_name_for_test("OTHER.MAP".to_string());
        let inconsistent_bytes =
            save_codec::serialize(&inconsistent).expect("corrupt map metadata");
        assert!(matches!(
            GameSnapshot::load_validated(&inconsistent_bytes, 0x11, 0x22, "MAP01.MAP"),
            Err(SnapshotError::MapNameMetadataMismatch { .. })
        ));
    }

    #[test]
    fn restore_validates_references_then_rebuilds_logic_particles_and_cells() {
        use crate::rules::particle_system_type::ParticleSystemTypeId;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::particles::ParticleSystem;
        use glam::IVec3;

        let mut sim = Simulation::new();
        sim.session.set_map_name_for_test("RESTORE.MAP".to_string());
        let owner = sim.interner.intern("AMERICANS");
        let type_ref = sim.interner.intern("MTNK");

        let entity_id = sim.substrate.allocate_stable_id();
        let mut entity = GameEntity::test_default(entity_id, "MTNK", "AMERICANS", 5, 6);
        entity.set_owner_for_test(owner);
        entity.set_type_ref_for_test(type_ref);
        entity.pending_c4_detonation.set_for_test(Some(
            crate::sim::c4_plant::PendingC4Detonation::new(
                crate::sim::timer::CdTimer::from_raw(-1, 0),
                Some(999),
            ),
        ));
        sim.substrate.entities.insert(entity);
        sim.add_entity_occupancy(entity_id);

        let particle_id = sim.substrate.allocate_stable_id();
        sim.substrate.particle_systems.insert({
            let mut system =
                ParticleSystem::test_fixture(particle_id, ParticleSystemTypeId(0), IVec3::ZERO);
            system.set_facing_for_test(0x1d);
            system.set_target_ref_for_test(Some(crate::sim::combat::TargetKind::Entity(entity_id)));
            system.set_owner_entity_for_test(Some(entity_id));
            system
        });
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .spark_particle_system
            .bind(Some(particle_id));
        sim.set_logic_order_for_test(vec![particle_id, entity_id]);

        let bytes = GameSnapshot::save_validated(&sim, 7, 8, "Restore fixture", 9);
        let mut restored = GameSnapshot::load_validated(&bytes, 7, 8, "RESTORE.MAP")
            .expect("strict snapshot")
            .sim;
        restored
            .restore_after_snapshot_load()
            .expect("valid restored object graph");

        assert_eq!(
            restored.live_object_order_snapshot(),
            vec![particle_id, entity_id]
        );
        assert!(
            restored
                .substrate
                .particle_systems
                .get(particle_id)
                .expect("particle system")
                .in_logic_vector()
        );
        assert!(
            restored
                .substrate
                .entities
                .get(entity_id)
                .expect("entity")
                .in_logic_vector
                .is_member()
        );
        assert!(
            restored
                .substrate
                .occupancy
                .contains_entity(5, 6, entity_id)
        );
        assert_eq!(
            restored
                .substrate
                .entities
                .get(entity_id)
                .expect("entity")
                .pending_c4_detonation
                .pending()
                .and_then(|pending| pending.source_entity_id()),
            None,
            "a successful restore cleans a dangling weak reference"
        );
        let system = restored
            .substrate
            .particle_systems
            .get(particle_id)
            .expect("particle system");
        assert_eq!(system.owner_entity(), Some(entity_id));
        assert_eq!(
            system.target_ref(),
            Some(crate::sim::combat::TargetKind::Entity(entity_id))
        );
        assert_eq!(
            restored
                .substrate
                .entities
                .get(entity_id)
                .unwrap()
                .spark_particle_system
                .system(),
            Some(particle_id)
        );
    }

    #[test]
    fn restore_rejects_unresolved_native_pointer_before_weak_cleanup() {
        use crate::rules::particle_system_type::ParticleSystemTypeId;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::particles::ParticleSystem;
        use glam::IVec3;

        let mut sim = Simulation::new();
        let entity_id = sim.substrate.allocate_stable_id();
        let mut entity = GameEntity::test_default(entity_id, "MTNK", "AMERICANS", 5, 6);
        entity.pending_c4_detonation.set_for_test(Some(
            crate::sim::c4_plant::PendingC4Detonation::new(
                crate::sim::timer::CdTimer::from_raw(-1, 0),
                Some(999),
            ),
        ));
        sim.substrate.entities.insert(entity);

        let particle_id = sim.substrate.allocate_stable_id();
        sim.substrate.particle_systems.insert({
            let mut system =
                ParticleSystem::test_fixture(particle_id, ParticleSystemTypeId(0), IVec3::ZERO);
            system.set_facing_for_test(0x1d);
            system.set_target_ref_for_test(Some(crate::sim::combat::TargetKind::Entity(999)));
            system.set_owner_entity_for_test(Some(entity_id));
            system
        });

        assert_eq!(
            sim.restore_after_snapshot_load(),
            Err(SnapshotRestoreError::UnresolvedObjectReference {
                source_registry: "ParticleSystemStore",
                source_id: particle_id,
                field: "target_ref",
                target_registry: "EntityStore or TerrainObjectStore",
                target_id: 999,
            })
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(entity_id)
                .expect("entity")
                .pending_c4_detonation
                .pending()
                .and_then(|pending| pending.source_entity_id()),
            Some(999),
            "failed restoration must not clean a later weak reference"
        );
        assert_eq!(
            sim.substrate
                .particle_systems
                .get(particle_id)
                .expect("particle system")
                .target_ref(),
            Some(crate::sim::combat::TargetKind::Entity(999)),
            "failed restoration must not sanitize the unresolved strong reference"
        );
    }

    #[derive(Debug, Clone, Copy)]
    struct Gsi1703Ids {
        parent: u64,
        child: u64,
        target: u64,
        projectile: u64,
    }

    fn gsi_17_03_projectile(
        source_id: u64,
        target: crate::sim::projectile::ProjectileTarget,
    ) -> crate::sim::projectile::ProjectileSpawn {
        use crate::sim::intern::InternedId;
        use crate::sim::projectile::{
            ProjectileCollisionPolicy, ProjectileCoord, ProjectilePayload, ProjectileSpawn,
            ProjectileTrajectory, ProjectileVelocity, ProjectileVisualState, TargetExpiryPolicy,
        };

        ProjectileSpawn {
            native_unique_id: 0,
            line_trail: None,
            flat: false,
            source_id,
            origin: ProjectileCoord::new(0, 0, 0),
            target,
            initial_target_position: ProjectileCoord::new(256, 256, 0),
            payload: ProjectilePayload::new(1, InternedId::from_index(0), None, false),
            speed_leptons_per_frame: 64,
            velocity: ProjectileVelocity::new(64, 0, 0),
            trajectory: ProjectileTrajectory::Straight,
            guidance: None,
            visual: ProjectileVisualState::new(0, 0, 0),
            arm_frames: 0,
            fuse_frames: None,
            ranged_fuse: false,
            tracks_target: false,
            target_expiry: TargetExpiryPolicy::Expire,
            collision: ProjectileCollisionPolicy::NONE,
        }
    }

    fn terrain_target_snapshot_fixture() -> (Simulation, crate::rules::ruleset::RuleSet, u64, u64) {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::combat::RAD_NO_ATTACKER;
        use crate::sim::projectile::ProjectileTarget;
        use crate::sim::terrain_object::{
            TerrainObjectState, mark_terrain_occupation, mark_terrain_raw_occupation,
        };

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=DUMMY\n[DUMMY]\nPrimary=GUN\n[GUN]\nProjectile=SHOT\nWarhead=WH\n[SHOT]\nInviso=yes\n[WH]\nWood=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n[TerrainTypes]\n0=TREE01\n[TREE01]\nStrength=10\nArmor=wood\nTemperateOccupationBits=7\n",
        ))
        .unwrap();
        let mut sim = Simulation::new();
        sim.session.set_map_width_for_test(4);
        sim.session.set_map_height_for_test(4);
        let mut terrain = flat_terrain(4, 4);
        terrain.cell_mut_for_test(1, 1).unwrap().level = 2;
        let tree_id = sim.substrate.allocate_stable_id();
        let tree = TerrainObjectState::new(
            tree_id,
            sim.interner.intern("TREE01"),
            (1, 1),
            rules
                .terrain_object_type_case_insensitive("TREE01")
                .unwrap(),
            false,
            Some(&terrain),
        );
        let coordinate = tree.world_coord();
        mark_terrain_occupation(&tree, Some(&mut terrain));
        mark_terrain_raw_occupation(
            &mut sim.substrate.raw_cell_occupation,
            tree.cell(),
            tree.occupation_bits(),
        );
        sim.terrain_objects
            .cell_index_mut_for_test()
            .insert(tree.cell(), tree_id);
        sim.terrain_objects
            .objects_mut_for_test()
            .insert(tree_id, tree);
        sim.resolved_terrain = Some(terrain);
        assert!(sim.register_terrain_object(tree_id, &rules));

        let projectile_id = sim.substrate.allocate_stable_id();
        let mut spawn = gsi_17_03_projectile(RAD_NO_ATTACKER, ProjectileTarget::Terrain(tree_id));
        spawn.initial_target_position = coordinate;
        spawn.payload = crate::sim::projectile::ProjectilePayload::new(
            1,
            sim.interner.intern("WH"),
            rules
                .weapon("GUN")
                .unwrap()
                .projectile
                .as_deref()
                .map(|name| sim.interner.intern(name)),
            rules.weapon("GUN").unwrap().bright,
        )
        .with_installed_weapon(Some(sim.interner.intern("GUN")));
        sim.admit_projectile(projectile_id, spawn);
        (sim, rules, tree_id, projectile_id)
    }

    #[test]
    fn bullet_constructor_fields_roundtrip_and_hash_independently() {
        use crate::sim::projectile::ProjectilePayload;
        let (mut sim, _, _, projectile_id) = terrain_target_snapshot_fixture();
        let base = sim.projectiles.get(projectile_id).unwrap().payload();
        let baseline_hash = sim.state_hash();
        let baseline_bytes = GameSnapshot::save(&sim, 0, 0, "bullet-constructor-state", 0);
        let mut restored_baseline = GameSnapshot::load(&baseline_bytes).unwrap().sim;
        restored_baseline.restore_after_snapshot_load().unwrap();
        let restored_baseline_hash = restored_baseline.state_hash();
        let mut hashes = BTreeSet::new();
        let mut restored_hashes = BTreeSet::new();
        // Native Construct carries type+AC and Bright+E0 independently of the
        // optional WeaponType+130. In particular a null weapon must not erase
        // the type/Bright supplied to a shrapnel or transient drop Bullet.
        for kind in [None, base.projectile_type()] {
            for installed in [None, base.installed_weapon()] {
                for bright in [false, true] {
                    let payload =
                        ProjectilePayload::new(base.base_damage, base.warhead, kind, bright)
                            .with_installed_weapon(installed);
                    sim.projectiles
                        .get_mut(projectile_id)
                        .unwrap()
                        .set_payload_for_test(payload);
                    assert!(hashes.insert(sim.state_hash()));
                    let bytes = GameSnapshot::save(&sim, 0, 0, "bullet-constructor-state", 0);
                    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
                    restored.restore_after_snapshot_load().unwrap();
                    assert_eq!(
                        restored.projectiles.get(projectile_id).unwrap().payload(),
                        payload
                    );
                    let restored_hash = restored.state_hash();
                    assert!(restored_hashes.insert(restored_hash));
                    assert_eq!(restored_hash == restored_baseline_hash, payload == base);
                    sim.projectiles
                        .get_mut(projectile_id)
                        .unwrap()
                        .set_payload_for_test(base);
                    assert_eq!(sim.state_hash(), baseline_hash);
                }
            }
        }
        assert_eq!(hashes.len(), 8);
        assert_eq!(restored_hashes.len(), 8);
    }

    #[test]
    fn terrain_projectile_target_roundtrip_preserves_identity_coordinate_and_hash_kind() {
        use crate::sim::projectile::ProjectileTarget;
        let (mut sim, _, tree_id, projectile_id) = terrain_target_snapshot_fixture();
        let coordinate = sim.terrain_objects.objects()[&tree_id].world_coord();
        let terrain_hash = sim.state_hash();
        sim.projectiles
            .get_mut(projectile_id)
            .unwrap()
            .set_target_for_test(ProjectileTarget::Entity(tree_id));
        assert_ne!(
            terrain_hash,
            sim.state_hash(),
            "pointer kinds cannot hash alike"
        );
        sim.projectiles
            .get_mut(projectile_id)
            .unwrap()
            .set_target_for_test(ProjectileTarget::Terrain(tree_id));

        let bytes = GameSnapshot::save(&sim, 0, 0, "terrain-target", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(
            restored.projectiles.get(projectile_id).unwrap().target(),
            ProjectileTarget::Terrain(tree_id)
        );
        assert_eq!(
            restored.terrain_objects.objects()[&tree_id].world_coord(),
            coordinate
        );
        assert_eq!(
            restored
                .projectiles
                .get(projectile_id)
                .unwrap()
                .last_target_position(),
            coordinate
        );
        assert_eq!(
            restored.terrain_objects.cell_index().get(&(1, 1)),
            Some(&tree_id)
        );
    }

    #[test]
    fn terrain_projectile_target_restore_rejects_missing_and_wrong_kind_references() {
        use crate::sim::game_entity::GameEntity;
        use crate::sim::projectile::ProjectileTarget;
        for wrong_kind in [false, true] {
            let (mut sim, _, _, projectile_id) = terrain_target_snapshot_fixture();
            let target_id = if wrong_kind {
                let id = sim.substrate.allocate_stable_id();
                sim.substrate.entities.insert(GameEntity::test_default(
                    id,
                    "DUMMY",
                    "Americans",
                    2,
                    2,
                ));
                sim.register_live_object(id);
                id
            } else {
                9_999
            };
            sim.projectiles
                .get_mut(projectile_id)
                .unwrap()
                .set_target_for_test(ProjectileTarget::Terrain(target_id));
            let bytes = GameSnapshot::save(&sim, 0, 0, "bad-terrain-target", 0);
            let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
            assert_eq!(
                restored.restore_after_snapshot_load(),
                Err(SnapshotRestoreError::UnresolvedObjectReference {
                    source_registry: "ProjectileStore",
                    source_id: projectile_id,
                    field: "target",
                    target_registry: "TerrainObjectStore",
                    target_id,
                }),
                "wrong kind={wrong_kind}"
            );
            assert_eq!(
                restored.projectiles.get(projectile_id).unwrap().target(),
                ProjectileTarget::Terrain(target_id)
            );
        }
    }

    #[test]
    fn navcom_restore_admits_only_techno_and_terrain_objects() {
        use crate::sim::components::NavTargetRef;
        use crate::sim::game_entity::GameEntity;
        // `nav_target_coordinate` resolves a NavCom Object in the entity or
        // Terrain store. Another registered identity (a projectile) or a
        // missing one names no NavCom target.
        for case in ["techno", "terrain", "projectile", "missing"] {
            let (mut sim, _, tree_id, projectile_id) = terrain_target_snapshot_fixture();
            let spawn = |sim: &mut Simulation| {
                let id = sim.substrate.allocate_stable_id();
                sim.substrate.entities.insert(GameEntity::test_default(
                    id,
                    "DUMMY",
                    "Americans",
                    2,
                    2,
                ));
                sim.register_live_object(id);
                id
            };
            let foot = spawn(&mut sim);
            let target = match case {
                "techno" => spawn(&mut sim),
                "terrain" => tree_id,
                "projectile" => projectile_id,
                _ => 9_999,
            };
            sim.substrate
                .entities
                .get_mut(foot)
                .unwrap()
                .navigation
                .set_nav_com_for_test(Some(NavTargetRef::object(target)));
            let bytes = GameSnapshot::save(&sim, 0, 0, "navcom-object", 0);
            let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
            let expected = if matches!(case, "techno" | "terrain") {
                Ok(())
            } else {
                Err(SnapshotRestoreError::UnresolvedObjectReference {
                    source_registry: "EntityStore",
                    source_id: foot,
                    field: "navigation.nav_com",
                    target_registry: "EntityStore or TerrainObjectStore",
                    target_id: target,
                })
            };
            assert_eq!(restored.restore_after_snapshot_load(), expected, "{case}");
        }
    }

    #[test]
    fn terrain_projectile_target_cell_repair_survives_pending_delete_roundtrip() {
        use crate::sim::combat::RAD_NO_ATTACKER;
        use crate::sim::projectile::ProjectileTarget;
        let (mut sim, rules, tree_id, projectile_id) = terrain_target_snapshot_fixture();
        let coordinate = sim.terrain_objects.objects()[&tree_id].world_coord();
        let warhead_id = sim.interner.intern("WH");
        crate::sim::combat::world_receiver::apply_area_damage(
            &mut sim,
            &rules,
            None,
            coordinate,
            10,
            rules.warhead("WH").unwrap(),
            (RAD_NO_ATTACKER, None, warhead_id),
            crate::sim::world::FrameEffects::default(),
        );
        let cell_target = ProjectileTarget::Cell { rx: 1, ry: 1 };
        assert_eq!(
            sim.projectiles.get(projectile_id).unwrap().target(),
            cell_target
        );
        assert!(sim.substrate.pending_delete.contains_object(&tree_id));
        assert!(!sim.terrain_objects.cell_index().contains_key(&(1, 1)));
        let bytes = GameSnapshot::save(&sim, 0, 0, "retired-terrain-target", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(
            restored.projectiles.get(projectile_id).unwrap().target(),
            cell_target
        );
        assert!(restored.substrate.pending_delete.contains_object(&tree_id));
        assert!(!restored.terrain_objects.cell_index().contains_key(&(1, 1)));
        restored.flush_pending_delete();
        assert!(!restored.terrain_objects.objects().contains_key(&tree_id));
        assert_eq!(
            restored.projectiles.get(projectile_id).unwrap().target(),
            cell_target
        );
        let bytes = GameSnapshot::save(&restored, 0, 0, "deleted-terrain-target", 0);
        GameSnapshot::load(&bytes)
            .unwrap()
            .sim
            .restore_after_snapshot_load()
            .unwrap();
    }

    fn gsi_17_03_reference_fixture() -> (Simulation, Gsi1703Ids) {
        use crate::sim::combat::TargetKind;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::projectile::ProjectileTarget;
        use crate::sim::spawn_manager::{
            SpawnManagerMode, SpawnManagerState, SpawnSlot, SpawnSlotState,
        };
        use crate::sim::timer::CdTimer;

        let mut sim = Simulation::new();
        let parent = sim.substrate.allocate_stable_id();
        let child = sim.substrate.allocate_stable_id();
        let target = sim.substrate.allocate_stable_id();

        let mut parent_entity = GameEntity::test_default(parent, "CARRIER", "AMERICANS", 1, 1);
        parent_entity.pending_c4_detonation.set_for_test(Some(
            crate::sim::c4_plant::PendingC4Detonation::new(
                crate::sim::timer::CdTimer::from_raw(-1, 0),
                Some(9_999),
            ),
        ));
        let mut manager = SpawnManagerState::for_test(
            sim.interner.intern("HORNET"),
            vec![
                SpawnSlot::for_test(
                    Some(child),
                    SpawnSlotState::ReadyDocked,
                    CdTimer::default(),
                    false,
                ),
                // A second saved pointer to the same child deliberately proves
                // that swizzle aliases are valid, not duplicate ownership.
                SpawnSlot::for_test(
                    Some(child),
                    SpawnSlotState::ReadyDocked,
                    CdTimer::default(),
                    false,
                ),
            ],
        );
        manager.set_rates_for_test(45, 20);
        manager.set_current_target_for_test(Some(TargetKind::Entity(target)));
        manager.set_queued_target_for_test(Some(TargetKind::Entity(target)));
        manager.set_mode_for_test(SpawnManagerMode::Launching);
        parent_entity.spawn_manager = Some(manager);
        sim.substrate.entities.insert(parent_entity);

        let mut child_entity = GameEntity::test_default(child, "HORNET", "AMERICANS", 2, 1);
        child_entity.spawn_owner_id.set_for_test(Some(parent));
        sim.substrate.entities.insert(child_entity);
        sim.substrate
            .entities
            .insert(GameEntity::test_default(target, "E1", "SOVIET", 3, 1));
        sim.register_live_object(parent);
        sim.register_live_object(child);
        sim.register_live_object(target);

        let projectile = sim.substrate.allocate_stable_id();
        sim.admit_projectile(
            projectile,
            gsi_17_03_projectile(target, ProjectileTarget::Entity(target)),
        );

        (
            sim,
            Gsi1703Ids {
                parent,
                child,
                target,
                projectile,
            },
        )
    }

    #[test]
    fn gsi_17_03_each_spawn_and_projectile_pointer_role_must_resolve_atomically() {
        #[derive(Debug, Clone, Copy)]
        enum MissingRole {
            SpawnSlot,
            CurrentTarget,
            QueuedTarget,
            SpawnOwner,
            ProjectileSource,
            ProjectileTarget,
        }

        let cases = [
            MissingRole::SpawnSlot,
            MissingRole::CurrentTarget,
            MissingRole::QueuedTarget,
            MissingRole::SpawnOwner,
            MissingRole::ProjectileSource,
            MissingRole::ProjectileTarget,
        ];
        for role in cases {
            let (mut sim, ids) = gsi_17_03_reference_fixture();
            let (source_registry, source_id, field) = match role {
                MissingRole::SpawnSlot => {
                    sim.substrate
                        .entities
                        .get_mut(ids.parent)
                        .unwrap()
                        .spawn_manager
                        .as_mut()
                        .unwrap()
                        .slots_mut_for_test()[0]
                        .set_spawn_for_test(Some(9_999));
                    ("EntityStore", ids.parent, "spawn_manager.slots.spawn")
                }
                MissingRole::CurrentTarget => {
                    sim.substrate
                        .entities
                        .get_mut(ids.parent)
                        .unwrap()
                        .spawn_manager
                        .as_mut()
                        .unwrap()
                        .set_current_target_for_test(Some(crate::sim::combat::TargetKind::Entity(
                            9_999,
                        )));
                    ("EntityStore", ids.parent, "spawn_manager.current_target")
                }
                MissingRole::QueuedTarget => {
                    sim.substrate
                        .entities
                        .get_mut(ids.parent)
                        .unwrap()
                        .spawn_manager
                        .as_mut()
                        .unwrap()
                        .set_queued_target_for_test(Some(crate::sim::combat::TargetKind::Entity(
                            9_999,
                        )));
                    ("EntityStore", ids.parent, "spawn_manager.queued_target")
                }
                MissingRole::SpawnOwner => {
                    sim.substrate
                        .entities
                        .get_mut(ids.child)
                        .unwrap()
                        .spawn_owner_id
                        .set_for_test(Some(9_999));
                    ("EntityStore", ids.child, "spawn_owner_id")
                }
                MissingRole::ProjectileSource => {
                    sim.projectiles
                        .get_mut(ids.projectile)
                        .unwrap()
                        .set_source_id_for_test(9_999);
                    ("ProjectileStore", ids.projectile, "source_id")
                }
                MissingRole::ProjectileTarget => {
                    sim.projectiles
                        .get_mut(ids.projectile)
                        .unwrap()
                        .set_target_for_test(crate::sim::projectile::ProjectileTarget::Entity(
                            9_999,
                        ));
                    ("ProjectileStore", ids.projectile, "target")
                }
            };

            assert_eq!(
                sim.restore_after_snapshot_load(),
                Err(SnapshotRestoreError::UnresolvedObjectReference {
                    source_registry,
                    source_id,
                    field,
                    target_registry: if matches!(
                        role,
                        MissingRole::CurrentTarget | MissingRole::QueuedTarget
                    ) {
                        "EntityStore or TerrainObjectStore"
                    } else {
                        "EntityStore"
                    },
                    target_id: 9_999,
                }),
                "missing {role:?} must reject the snapshot"
            );
            assert_eq!(
                sim.substrate
                    .entities
                    .get(ids.parent)
                    .unwrap()
                    .pending_c4_detonation
                    .pending()
                    .and_then(|pending| pending.source_entity_id()),
                Some(9_999),
                "{role:?} rejection must precede later weak-reference cleanup"
            );
        }
    }

    #[test]
    fn gsi_17_03_null_and_cell_reference_forms_need_no_object_identity() {
        use crate::sim::combat::{RAD_NO_ATTACKER, TargetKind};
        use crate::sim::projectile::ProjectileTarget;

        let (mut sim, ids) = gsi_17_03_reference_fixture();
        let manager = sim
            .substrate
            .entities
            .get_mut(ids.parent)
            .unwrap()
            .spawn_manager
            .as_mut()
            .unwrap();
        for slot in manager.slots_mut_for_test() {
            slot.set_spawn_for_test(None);
        }
        manager.set_current_target_for_test(Some(TargetKind::Cell(7, 8)));
        manager.set_queued_target_for_test(Some(TargetKind::Cell(9, 10)));
        sim.substrate
            .entities
            .get_mut(ids.child)
            .unwrap()
            .spawn_owner_id
            .set_for_test(None);
        let projectile = sim.projectiles.get_mut(ids.projectile).unwrap();
        projectile.set_source_id_for_test(RAD_NO_ATTACKER);
        projectile.set_target_for_test(ProjectileTarget::Cell { rx: 11, ry: 12 });

        let null_target_projectile = sim.substrate.allocate_stable_id();
        sim.admit_projectile(
            null_target_projectile,
            gsi_17_03_projectile(RAD_NO_ATTACKER, ProjectileTarget::None),
        );

        assert_eq!(sim.restore_after_snapshot_load(), Ok(()));
    }

    #[test]
    fn gsi_04_01_snapshot_handoff_retains_live_process_dummy_without_serializing_it() {
        use crate::sim::combat::RAD_NO_ATTACKER;
        use crate::sim::projectile::ProjectileTarget;
        use crate::sim::{movement::locomotor::MovementLayer, occupancy::RawCellKey};

        let mut live = Simulation::new();
        let process_dummy = live.shared_cell_dummy.clone();
        process_dummy.set_level_slope(-7, 11);
        process_dummy.stamp_coord(7, 9);

        let before_neighbor_count = live.state_hash();
        process_dummy.adjust_neighbor_count(true);
        assert_ne!(before_neighbor_count, live.state_hash());

        let owner = live.intern("DummyOccupationOwner");
        let hash_before_raw = live.state_hash();
        live.substrate.raw_cell_occupation.write_occupant(
            RawCellKey::Dummy,
            MovementLayer::Ground,
            4,
            Some(owner),
            true,
        );
        let ground_hash = live.state_hash();
        assert_ne!(hash_before_raw, ground_hash);
        live.substrate.raw_cell_occupation.write_occupant(
            RawCellKey::Dummy,
            MovementLayer::Bridge,
            8,
            Some(owner),
            true,
        );
        assert_ne!(
            ground_hash,
            live.state_hash(),
            "the fallback cell's two raw planes are future-affecting"
        );
        let projectile_id = live.substrate.allocate_stable_id();
        live.admit_projectile(
            projectile_id,
            gsi_17_03_projectile(RAD_NO_ATTACKER, ProjectileTarget::DummyCell),
        );

        let hash_at_a = live.state_hash();
        process_dummy.stamp_coord(8, 10);
        assert_ne!(
            hash_at_a,
            live.state_hash(),
            "a retained Bullet pointer makes the live dummy snapshot future behavior"
        );
        process_dummy.stamp_coord(7, 9);

        let bytes = GameSnapshot::save(&live, 0, 0, "shared-dummy.map", 0);
        let cold = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        assert!(
            cold.substrate
                .raw_cell_occupation
                .dummy_for_hash()
                .is_none(),
            "process-global occupation is not saved Scenario payload"
        );
        assert_eq!(
            cold.projectiles.get(projectile_id).unwrap().target(),
            ProjectileTarget::DummyCell
        );
        assert_eq!(
            cold.shared_cell_dummy.snapshot().coord,
            (0, 0),
            "the process-global CellClass bytes are not Scenario payload"
        );
        assert!(!cold.shared_cell_dummy.same_identity(&process_dummy));
        assert_eq!(cold.shared_cell_dummy.neighbor_count(), 0);

        let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        restored
            .retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(&live));
        assert_eq!(
            restored.substrate.raw_cell_occupation.dummy_for_hash(),
            live.substrate.raw_cell_occupation.dummy_for_hash()
        );
        assert!(restored.shared_cell_dummy.same_identity(&process_dummy));
        assert_eq!(restored.shared_cell_dummy.neighbor_count(), 1);
        assert_eq!(
            restored.shared_cell_dummy.snapshot(),
            crate::map::resolved_terrain::SharedCellDummySnapshot {
                coord: (7, 9),
                level: -7,
                slope_type: 11,
                bridge_flags_0x1180: 0,
            },
            "fallible candidate preparation retains identity without mutating the live process"
        );
        restored
            .restore_after_snapshot_load()
            .expect("dummy CellClass target needs no Object identity fixup");

        let terrain_template = flat_terrain(2, 2);
        restored.rebuild_caches_after_load(
            terrain_template,
            crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            &crate::sim::runtime::SimResources::empty().rules,
        );
        let rebuilt_dummy = restored
            .resolved_terrain
            .as_ref()
            .expect("rebuilt terrain")
            .shared_cell_dummy();
        assert!(rebuilt_dummy.same_identity(&process_dummy));
        assert_eq!(rebuilt_dummy.snapshot().coord, (7, 9));

        restored.reconstruct_cellclass_dummy_for_map_resize();
        assert!(
            restored
                .substrate
                .raw_cell_occupation
                .dummy_for_hash()
                .is_none(),
            "Cell47BBF0 reconstructs both raw bytes and owners"
        );
        assert_eq!(
            rebuilt_dummy.snapshot(),
            crate::map::resolved_terrain::SharedCellDummySnapshot {
                coord: (0, 0),
                level: 0,
                slope_type: 0,
                bridge_flags_0x1180: 0,
            }
        );

        process_dummy.stamp_coord(-2, 7);
        assert_eq!(
            rebuilt_dummy.snapshot().coord,
            (-2, 7),
            "the restored DummyCell target retains the same live identity after reconstruction"
        );
    }

    #[test]
    fn gsi_04_01_successful_load_restores_real_flags_and_zeroes_only_dummy() {
        use crate::map::bridge_facts::{
            BRIDGE_FLAG_STRUCTURAL, BridgeFlagStamp, BridgeStampSlot,
            MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
        };
        use crate::rules::ini_parser::IniFile;
        use crate::rules::overlay_types::OverlayTypeRegistry;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::overlay_grid::OverlayGrid;

        let mut live = Simulation::new();
        let mut map_load_terrain = flat_terrain(4, 3);
        map_load_terrain.test_set_native_allocated_cells(&[(0, 0), (1, 1)]);
        {
            let pristine_bridge_cell = map_load_terrain.cell_mut_for_test(1, 1).unwrap();
            pristine_bridge_cell.level = 2;
            pristine_bridge_cell.slope_type = 1;
            pristine_bridge_cell.bridge_facts.raw_flags = MODELED_CELLCLASS_BRIDGE_FLAG_MASK;
        }
        let pristine_load_template = map_load_terrain.clone();
        let expected_ground_z = {
            let target =
                crate::sim::projectile::cell_target_coord(Some(&pristine_load_template), 1, 1);
            let cell = pristine_load_template.cell(1, 1).unwrap();
            crate::util::lepton::ground_height_leptons(
                cell.level,
                cell.slope_type,
                target.x,
                target.y,
            )
            .expect("fixture uses a native-supported CellClass slope")
        };
        live.install_resolved_terrain_for_new_map(map_load_terrain);
        live.overlay_grid = Some(OverlayGrid::new(4, 3));
        assert_ne!(
            live.resolved_terrain
                .as_ref()
                .unwrap()
                .cell(1, 1)
                .unwrap()
                .bridge_facts
                .raw_flags
                & BRIDGE_FLAG_STRUCTURAL,
            0,
            "the pristine allocated CellClass starts on the high bridge target surface"
        );
        let hash_with_pristine_bridge = live.state_hash();

        // The real anchor is allocated. Every neighbor is unallocated or
        // missing, so this live clear updates both serialized real authority
        // and the process dummy through their distinct native targets before
        // the snapshot is written.
        live.apply_runtime_bridge_flag_stamp(BridgeFlagStamp::new((1, 1), 0, false));
        let process_dummy = live.effective_shared_cell_dummy();
        assert_eq!(
            live.resolved_terrain
                .as_ref()
                .unwrap()
                .cell(1, 1)
                .unwrap()
                .bridge_facts
                .raw_flags
                & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            0,
            "the live runtime setter clears the allocated CellClass before save"
        );
        assert_eq!(
            live.real_cell_bridge_flags_0x1180,
            live.resolved_terrain
                .as_ref()
                .unwrap()
                .capture_real_cell_bridge_flags_0x1180(),
            "serialized real-cell authority records the runtime-cleared value"
        );
        process_dummy.reconstruct_for_map_resize();
        let hash_after_runtime_clear = live.state_hash();
        assert_ne!(
            hash_after_runtime_clear, hash_with_pristine_bridge,
            "the future-affecting real-cell value authority is hashed even with a clear dummy"
        );
        assert!(live.rebuild_dynamic_navigation(&crate::sim::runtime::SimResources::empty().rules));
        process_dummy.stamp_coord(-23, 17);
        process_dummy.set_level_slope(-7, 11);
        process_dummy.apply_bridge_flag_slot(BridgeStampSlot::Anchor, true);
        let dirty_dummy_before_load = process_dummy.snapshot();
        assert_ne!(dirty_dummy_before_load.bridge_flags_0x1180, 0);

        // In-scenario process state retains the shared Dummy, while native
        // Scenario deserialization independently resets its RNG to Seed(0).
        live.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = GameSnapshot::save(&live, 0, 0, "bridge-dummy.map", 0);
        let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        restored
            .retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(&live));
        restored
            .restore_after_snapshot_load()
            .expect("snapshot references restore");
        restored.rebuild_caches_after_load(
            pristine_load_template,
            crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            &crate::sim::runtime::SimResources::empty().rules,
        );
        let rebuilt_terrain = restored.resolved_terrain.as_ref().unwrap();
        assert_eq!(
            rebuilt_terrain.cell(1, 1).unwrap().bridge_facts.raw_flags
                & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            "candidate cache rebuild starts from the nonzero pristine map template"
        );
        assert_eq!(
            crate::sim::projectile::cell_target_coord(Some(rebuilt_terrain), 1, 1).z,
            expected_ground_z + crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS,
            "before direct value restore the pristine raw 0x100 selects the +416 target surface"
        );
        let pristine_candidate_authority = rebuilt_terrain.capture_real_cell_bridge_flags_0x1180();
        let serialized_cleared_authority = restored.real_cell_bridge_flags_0x1180.clone();
        assert_ne!(serialized_cleared_authority, pristine_candidate_authority);
        let hash_with_serialized_clear = restored.state_hash();
        restored.real_cell_bridge_flags_0x1180 = pristine_candidate_authority.clone();
        let hash_if_pristine_template_were_authority = restored.state_hash();
        assert_ne!(
            hash_with_serialized_clear, hash_if_pristine_template_were_authority,
            "the canonical hash distinguishes the saved clear from the pristine nonzero template"
        );
        restored.real_cell_bridge_flags_0x1180 = serialized_cleared_authority.clone();
        assert_eq!(restored.state_hash(), hash_with_serialized_clear);
        let candidate_dummy_before_authority_restore =
            restored.effective_shared_cell_dummy().snapshot();
        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n[OverlayTypes]\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("empty map-authority rules");
        restored
            .restore_map_authority_after_snapshot_load(&rules, &OverlayTypeRegistry::empty(), None)
            .expect("serialized real CellClass values restore directly");

        let candidate_dummy = restored.effective_shared_cell_dummy();
        assert!(candidate_dummy.same_identity(&process_dummy));
        assert_eq!(
            candidate_dummy.snapshot(),
            candidate_dummy_before_authority_restore,
            "direct real-cell restoration must not lookup, stamp, or mutate the dummy"
        );
        assert_eq!(candidate_dummy.snapshot(), dirty_dummy_before_load);
        let restored_terrain = restored.resolved_terrain.as_ref().unwrap();
        assert_eq!(
            restored_terrain.cell(1, 1).unwrap().bridge_facts.raw_flags & BRIDGE_FLAG_STRUCTURAL,
            0,
            "the saved clear removes the pristine real CellClass structural bit"
        );
        assert_eq!(
            restored_terrain.cell(1, 1).unwrap().bridge_facts.raw_flags
                & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            0,
            "direct CellClass value restore replaces the complete pristine modeled mask"
        );
        assert_eq!(
            crate::sim::projectile::cell_target_coord(Some(restored_terrain), 1, 1).z,
            expected_ground_z,
            "restored raw 0x100 clear removes +416 while retaining the 104-lepton ground kernel"
        );
        let saved_authority = save_codec::serialize(&live).unwrap();
        let loaded_authority = save_codec::serialize(&restored).unwrap();
        assert!(
            saved_authority == loaded_authority,
            "real Cell authority differs at byte {:?}; lengths {} vs {}",
            saved_authority
                .iter()
                .zip(&loaded_authority)
                .position(|(a, b)| a != b),
            saved_authority.len(),
            loaded_authority.len()
        );
        assert_zone_grids_equivalent(
            live.zone_grid.as_ref().unwrap(),
            restored.zone_grid.as_ref().unwrap(),
        );
        assert_eq!(
            restored.state_hash(),
            live.state_hash(),
            "completed native hierarchy rebuild matches the published saved-clear world"
        );
        assert_eq!(
            restored.real_cell_bridge_flags_0x1180,
            serialized_cleared_authority
        );
        assert_eq!(
            restored_terrain.cell(0, 0).unwrap().bridge_facts.raw_flags
                & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            0,
            "untouched allocated cells retain their exact saved zero"
        );
        assert!(restored_terrain.cell(1, 0).is_none());
        assert_eq!(
            restored_terrain.cells()[1].bridge_facts.raw_flags & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            0,
            "unallocated storage is not promoted into real-cell authority"
        );

        // MouseClass::Load reaches Resize only at accepted commit. That ctor
        // zeros the same dummy identity and does not rewrite loaded real cells.
        restored.reconstruct_cellclass_dummy_for_map_resize();

        let reconstructed = restored.effective_shared_cell_dummy();
        assert!(reconstructed.same_identity(&process_dummy));
        assert_eq!(
            reconstructed.snapshot(),
            crate::map::resolved_terrain::SharedCellDummySnapshot {
                coord: (0, 0),
                level: 0,
                slope_type: 0,
                bridge_flags_0x1180: 0,
            }
        );
        assert_eq!(
            restored
                .resolved_terrain
                .as_ref()
                .unwrap()
                .cell(1, 1)
                .unwrap()
                .bridge_facts
                .raw_flags
                & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            0,
            "dummy reconstruction must not resurrect pristine real CellClass flags"
        );
        assert_eq!(
            crate::sim::projectile::cell_target_coord(restored.resolved_terrain.as_ref(), 1, 1,).z,
            expected_ground_z,
            "accepted Resize keeps the restored real CellClass on the 104-lepton ground surface"
        );
        assert_eq!(
            restored.real_cell_bridge_flags_0x1180, serialized_cleared_authority,
            "accepted Resize clears only the dummy and retains saved real-cell hash authority"
        );
    }

    #[test]
    fn gsi_17_03_repeated_aliases_do_not_imply_reciprocal_spawn_ownership() {
        let (mut sim, ids) = gsi_17_03_reference_fixture();
        sim.substrate
            .entities
            .get_mut(ids.child)
            .unwrap()
            .spawn_owner_id
            .set_for_test(Some(ids.target));

        assert_eq!(sim.restore_after_snapshot_load(), Ok(()));
        assert_eq!(
            sim.substrate
                .entities
                .get(ids.parent)
                .unwrap()
                .spawn_manager
                .as_ref()
                .unwrap()
                .slots()
                .iter()
                .map(|slot| slot.spawn())
                .collect::<Vec<_>>(),
            vec![Some(ids.child), Some(ids.child)]
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(ids.child)
                .unwrap()
                .spawn_owner_id
                .owner(),
            Some(ids.target)
        );
    }

    #[test]
    fn restore_clears_move_sound_without_playback_or_rng_draw() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::game_entity::GameEntity;

        let rules = RuleSet::from_ini_with_sounds_for_test(
            &IniFile::from_str(
                "[InfantryTypes]\n\
                 [VehicleTypes]\n0=TEST\n\
                 [AircraftTypes]\n\
                 [BuildingTypes]\n\
                 [TEST]\nMoveSound=VMove\n",
            ),
            &["VMove"],
        )
        .expect("move-sound rules");
        let mut sim = Simulation::new();
        sim.session
            .set_map_name_for_test("MOVESOUND.MAP".to_string());
        let owner = sim.interner.intern("AMERICANS");
        let type_ref = sim.interner.intern("TEST");
        sim.interner.intern("VMove");
        let entity_id = sim.substrate.allocate_stable_id();
        let mut entity = GameEntity::test_default(entity_id, "TEST", "AMERICANS", 7, 9);
        entity.set_owner_for_test(owner);
        entity.set_type_ref_for_test(type_ref);
        entity.move_sound = crate::sim::world::MoveSoundState::from_raw_for_test(true, 2);
        sim.substrate.entities.insert(entity);

        let bytes = GameSnapshot::save_validated(&sim, 17, 18, "Move sound fixture", 19);
        let mut restored = GameSnapshot::load_validated(&bytes, 17, 18, "MOVESOUND.MAP")
            .expect("strict snapshot")
            .sim;
        let rng_before = restored.rng_state();
        restored
            .restore_after_snapshot_load()
            .expect("valid restored object graph");
        restored.resolve_type_handles(&rules);
        assert_eq!(restored.rng_state(), rng_before);
        let entity = restored.substrate.entities.get(entity_id).expect("entity");
        assert!(!entity.move_sound.is_active());
        assert_eq!(entity.move_sound.countdown(), 0);
        assert!(restored.sound_events.is_empty());
    }

    #[test]
    fn exact_mission_schema_round_trips_raw_ids_leaves_archives_and_locomotors() {
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::sim::combat::TargetKind;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::mission::state::MissionTestFixture;
        use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionLeafState};
        use crate::sim::movement::locomotor::LocomotorState;

        let mut leaves = [
            MissionLeafState::unit_raw_for_test(1, 2, 3, 4),
            MissionLeafState::infantry_raw_for_test(5, 41),
            MissionLeafState::aircraft_raw_for_test(6, 7),
            MissionLeafState::building_raw_for_test(8),
            MissionLeafState::unit_raw_for_test(9, 10, 11, 12),
            MissionLeafState::infantry_raw_for_test(13, -1),
        ];
        leaves[1].set_infantry_water_state(false);
        leaves[5].set_infantry_water_state(true);

        let mut sim = Simulation::new();
        for (index, &leaf) in leaves.iter().enumerate() {
            let id = index as u64 + 1;
            let mut entity = GameEntity::test_default(id, "MTNK", "Americans", 5, 5);
            entity.mission_leaf = leaf;
            entity
                .suspended_attack_target
                .set_for_test(Some(if index & 1 == 0 {
                    TargetKind::Entity(100 + id)
                } else {
                    TargetKind::Cell(index as u16, (index + 1) as u16)
                }));
            entity.set_falling_down_for_test(index & 1 == 0);
            entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
            if index == 0 {
                entity.mission.apply_test_fixture(MissionTestFixture {
                    current: MissionId::from_raw(i32::MIN),
                    suspended: MissionId::from_raw(0x1234_5678),
                    queued: MissionId::from_raw(i32::MAX),
                    movement_bypass_latch: 0xa5,
                    handler_state: 0x1122_3344,
                    mission_start_frame: 0x5566_7788,
                    ai_counter: 0x99aa_bbcc,
                    dispatch_timer: MissionDispatchTimer::from_raw(-17, -29),
                });
            }
            sim.substrate.entities.insert(entity);
        }

        let bytes = GameSnapshot::save(&sim, 1, 2, "mission-schema", 3);
        let loaded = GameSnapshot::load(&bytes).expect("load exact Mission schema");

        for (index, &leaf) in leaves.iter().enumerate() {
            let entity = loaded
                .sim
                .substrate
                .entities
                .get(index as u64 + 1)
                .expect("restored Mission fixture");
            let id = index as u64 + 1;
            let expected_suspended_target = Some(if index & 1 == 0 {
                TargetKind::Entity(100 + id)
            } else {
                TargetKind::Cell(index as u16, (index + 1) as u16)
            });
            assert_eq!(entity.mission_leaf, leaf);
            assert_eq!(
                entity.suspended_attack_target.target(),
                expected_suspended_target,
                "suspended TargetKind variant and payload must round-trip"
            );
            assert_eq!(entity.is_falling_down(), index & 1 == 0);
        }

        let first = loaded.sim.substrate.entities.get(1).unwrap();
        assert_eq!(first.mission.current(), MissionId::from_raw(i32::MIN));
        assert_eq!(first.mission.suspended(), MissionId::from_raw(0x1234_5678));
        assert_eq!(first.mission.queued(), MissionId::from_raw(i32::MAX));
        assert_eq!(
            first.mission.dispatch_timer(),
            MissionDispatchTimer::from_raw(-17, -29)
        );
    }

    #[test]
    fn constructor_owner_survives_transfers_and_snapshot_restore() {
        use crate::map::entities::EntityCategory;
        use crate::sim::game_entity::GameEntity;

        let mut sim = Simulation::new();
        let initial = sim.interner.intern("Americans");
        let current = sim.interner.intern("Russians");
        let type_ref = sim.interner.intern("GAPOWR");
        let mut entity = GameEntity::test_default_of_category(
            1,
            "GAPOWR",
            "Americans",
            5,
            5,
            EntityCategory::Structure,
        );
        entity.set_owner_for_test(initial);
        entity.set_type_ref_for_test(type_ref);
        sim.substrate.entities.insert(entity);
        sim.substrate.entities.change_owner(1, current);
        // Match the scenario RNG's post-load seed reset for this state-only
        // snapshot test. Physical resumed capture is tested separately.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();
        let bytes = GameSnapshot::save(&sim, 0, 0, "captured-owner", 0);
        let mut loaded = GameSnapshot::load(&bytes).expect("load constructor owner");
        let restored = loaded.sim.substrate.entities.get(1).unwrap();
        assert_eq!(restored.owner(), current);
        assert_eq!(restored.initial_owner(), initial);
        assert_eq!(loaded.sim.state_hash(), expected_hash);
        loaded.sim.substrate.entities.change_owner(1, initial);
        let recaptured = loaded.sim.substrate.entities.get(1).unwrap();
        assert_eq!(recaptured.owner(), initial);
        assert_eq!(recaptured.initial_owner(), initial);
    }

    #[test]
    fn entity_construction_frame_round_trips_as_dispatch_start() {
        use crate::map::entities::EntityCategory;
        use crate::sim::combat::Health;
        use crate::sim::game_entity::GameEntity;

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("MTNK");
        let entity = GameEntity::new_at_frame_for_test(
            1,
            5,
            5,
            0,
            0,
            owner,
            Health::new(100),
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            true,
            37,
        );
        sim.substrate.entities.insert(entity);

        let bytes = GameSnapshot::save(&sim, 0, 0, "frame-37", 0);
        let loaded = GameSnapshot::load(&bytes).expect("load frame-37 entity");
        assert_eq!(
            loaded
                .sim
                .substrate
                .entities
                .get(1)
                .unwrap()
                .mission
                .dispatch_timer()
                .start_frame(),
            37
        );
    }

    #[test]
    fn house_difficulty_round_trips_through_snapshot() {
        use crate::sim::house_state::{HouseDifficulty, HouseState};

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Computer1");
        let mut house = HouseState::new(owner, 0, None, false, 0, 10);
        house.set_difficulty_for_test(HouseDifficulty::Easy);
        sim.houses.register(house);
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // house-difficulty persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
        let loaded = GameSnapshot::load(&bytes).expect("load should succeed");

        assert_eq!(
            loaded
                .sim
                .houses
                .get(&owner)
                .map(|house| house.difficulty()),
            Some(HouseDifficulty::Easy),
        );
        assert_eq!(loaded.sim.state_hash(), expected_hash);
    }

    /// Issue #1949. Retail witnesses register an extra hostile House beside
    /// the admitted scenario Houses before saving (Dustbowl defences, the
    /// Hills bridge layers). The native House array holds every entry with
    /// the bounded name `FromName50C170` reads, so such a registration must
    /// install that name and its array membership, and snapshot401 must
    /// restore the mixed scenario. A registration that skips the name stays
    /// rejectable: the admitted-scenario array invariant is native, not a
    /// test convenience.
    #[test]
    fn additionally_registered_scenario_house_round_trips_through_snapshot() {
        use crate::sim::house_state::HouseState;

        let mut sim = Simulation::with_seed(0x1949);
        let launch = sim.interner.intern("<human player>");
        let mut launch_house = HouseState::new(launch, 0, None, true, 10_000, 10);
        launch_house.initialize_scenario_name("<human player>");
        sim.houses.register(launch_house);
        sim.session.set_current_house_for_test(Some(launch));
        let hostile = sim.interner.intern("Russians");
        let mut hostile_house = HouseState::new(hostile, 1, None, true, 0, 10);
        hostile_house.initialize_scenario_name("Russians");
        sim.houses.register(hostile_house);

        let bytes = GameSnapshot::save(&sim, 0, 0, "mixed-registered-houses", 0);
        let mut restored = GameSnapshot::load(&bytes).expect("load should succeed").sim;
        restored
            .restore_after_snapshot_load()
            .expect("mixed admitted and additionally registered Houses restore");
        assert_eq!(
            Simulation::scenario_house_named(&restored.houses, "Russians"),
            Some(hostile)
        );
        assert_eq!(
            restored.houses[&hostile].native_scenario_name(),
            Some("Russians")
        );

        // The pre-fix witness shape — the same extra House without its
        // native name — is the state the validation must keep rejecting.
        let mut nameless = Simulation::with_seed(0x1949);
        let launch = nameless.interner.intern("<human player>");
        let mut launch_house = HouseState::new(launch, 0, None, true, 10_000, 10);
        launch_house.initialize_scenario_name("<human player>");
        nameless.houses.register(launch_house);
        let hostile = nameless.interner.intern("Russians");
        nameless
            .houses
            .register(HouseState::new(hostile, 1, None, true, 0, 10));
        let bytes = GameSnapshot::save(&nameless, 0, 0, "nameless-registered-house", 0);
        let mut rejected = GameSnapshot::load(&bytes).expect("load should succeed").sim;
        assert!(matches!(
            rejected.restore_after_snapshot_load(),
            Err(SnapshotRestoreError::InvalidHouseScenarioName { owner, .. }) if owner == hostile
        ));
    }

    /// A cell TarCom survives serialize → deserialize as the same
    /// `TargetKind::Cell` variant.
    #[test]
    fn cell_attack_target_round_trips_through_snapshot() {
        use crate::sim::combat::TargetKind;
        use crate::sim::game_entity::GameEntity;

        let mut sim = Simulation::new();
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        entity
            .attack_target
            .set_for_test(Some(TargetKind::Cell(50, 50)));
        sim.substrate.entities.insert(entity);

        let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
        let loaded = GameSnapshot::load(&bytes).expect("load should succeed");
        let restored = loaded
            .sim
            .substrate
            .entities
            .get(1)
            .expect("entity should be restored")
            .attack_target
            .target()
            .expect("attack_target should be restored");
        assert!(matches!(restored, TargetKind::Cell(50, 50)));
    }

    /// The active order is serialized directly and restored verbatim — not
    /// re-derived, not sorted. (ledger 13)
    #[test]
    fn saveload_restores_live_object_order_verbatim() {
        use crate::sim::game_entity::GameEntity;
        let mut sim = Simulation::new();
        for id in [10u64, 20, 30] {
            sim.substrate
                .entities
                .insert(GameEntity::test_default(id, "MTNK", "Americans", 5, 5));
            sim.register_live_object(id);
        }
        // Force an order whose sequence differs from stable-id order.
        sim.set_logic_order_for_test(vec![20, 10, 30]);

        let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
        let restored = GameSnapshot::load(&bytes).expect("load should succeed").sim;
        assert_eq!(restored.live_object_order_snapshot(), vec![20, 10, 30]);
    }

    /// Rust snapshots preserve a dead-limbo object's independent state and the
    /// ordered pending-delete boundary instead of reconstructing either from
    /// global storage or LogicVector membership.
    #[test]
    fn lifecycle_authority_pending_boundary_roundtrips_queue_and_state() {
        use crate::sim::game_entity::GameEntity;

        let mut sim = Simulation::new();
        let mut entity = GameEntity::test_default(7, "MTNK", "Americans", 5, 5);
        entity.lifecycle.set_object_alive_for_test(false);
        entity.lifecycle.set_in_limbo_for_test(true);
        entity.lifecycle.set_cell_marked_for_test(false);
        entity.in_logic_vector.set_for_test(false);
        entity.death.set_dying_for_test(true);
        entity.removal_facts.set_dirty_rect_eligible_for_test(true);
        entity.kill_record.set_destruction_recorded_for_test(true);
        sim.substrate.entities.insert(entity);
        for id in [7, 3, 7] {
            sim.substrate.pending_delete.queue_object(id);
        }
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // lifecycle-boundary persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let hash_before = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
        let restored = GameSnapshot::load(&bytes).expect("load should succeed").sim;
        let restored_entity = restored.substrate.entities.get(7).expect("entity restored");

        assert!(!restored_entity.lifecycle.object_alive());
        assert!(restored_entity.lifecycle.in_limbo());
        assert!(!restored_entity.lifecycle.cell_marked());
        assert!(!restored_entity.in_logic_vector.is_member());
        assert!(restored_entity.death.is_dying());
        assert!(restored_entity.removal_facts.dirty_rect_eligible());
        assert!(restored_entity.kill_record.is_recorded());
        assert_eq!(
            restored.substrate.pending_delete.object_ids(),
            vec![7, 3, 7]
        );
        assert_eq!(restored.state_hash(), hash_before);
    }

    #[test]
    fn lifecycle_authority_logic_rebuild_does_not_rederive_limbo_or_mark() {
        use crate::sim::game_entity::GameEntity;

        let mut sim = Simulation::new();
        let mut off_cell_member = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        off_cell_member.lifecycle.set_in_limbo_for_test(false);
        off_cell_member.lifecycle.set_cell_marked_for_test(false);
        sim.substrate.entities.insert(off_cell_member);
        sim.substrate
            .logic
            .try_push(1)
            .expect("logic fixture append");

        let mut marked_non_member = GameEntity::test_default(2, "MTNK", "Americans", 6, 6);
        marked_non_member.lifecycle.set_in_limbo_for_test(false);
        marked_non_member.lifecycle.set_cell_marked_for_test(true);
        sim.substrate.entities.insert(marked_non_member);

        let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
        let mut restored = GameSnapshot::load(&bytes).expect("load should succeed").sim;
        restored.rebuild_logic_membership();

        let member = restored.substrate.entities.get(1).expect("member restored");
        assert!(member.in_logic_vector.is_member());
        assert!(!member.lifecycle.in_limbo());
        assert!(!member.lifecycle.cell_marked());

        let non_member = restored
            .substrate
            .entities
            .get(2)
            .expect("non-member restored");
        assert!(!non_member.in_logic_vector.is_member());
        assert!(!non_member.lifecycle.in_limbo());
        assert!(non_member.lifecycle.cell_marked());
    }

    #[test]
    fn lifecycle_authority_bookkeeping_facts_roundtrip_and_change_state_hash() {
        use crate::sim::game_entity::GameEntity;

        let mut sim = Simulation::new();
        sim.substrate
            .entities
            .insert(GameEntity::test_default(1, "MTNK", "Americans", 5, 5));
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // lifecycle bookkeeping persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let default_hash = sim.state_hash();

        let entity = sim.substrate.entities.get_mut(1).expect("fixture entity");
        entity.removal_facts.set_dirty_rect_eligible_for_test(true);
        entity.kill_record.set_destruction_recorded_for_test(true);
        let changed_hash = sim.state_hash();
        assert_ne!(default_hash, changed_hash);

        let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
        let restored = GameSnapshot::load(&bytes).expect("load should succeed").sim;
        let restored_entity = restored.substrate.entities.get(1).expect("entity restored");
        assert!(restored_entity.removal_facts.dirty_rect_eligible());
        assert!(restored_entity.kill_record.is_recorded());
        assert_eq!(restored.state_hash(), changed_hash);
    }

    #[test]
    fn techno_playfield_membership_roundtrips_and_changes_state_hash_v87() {
        use crate::sim::game_entity::GameEntity;

        let mut sim = Simulation::new();
        sim.substrate
            .entities
            .insert(GameEntity::test_default(1, "MTNK", "Americans", 5, 5));
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let default_hash = sim.state_hash();
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .set_in_playfield(true);
        let member_hash = sim.state_hash();
        assert_ne!(default_hash, member_hash);

        let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
        let restored = GameSnapshot::load(&bytes)
            .expect("v87 membership loads")
            .sim;
        assert!(restored.substrate.entities.get(1).unwrap().in_playfield());
        assert_eq!(restored.state_hash(), member_hash);
    }

    #[test]
    fn gsi_04_10_destroyed_terrain_snapshot_rebuild_does_not_resurrect_cache() {
        use crate::map::resolved_terrain::zone_class;
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::pathfinding::zone_map::ZONE_INVALID;
        use crate::sim::terrain_object::{
            TerrainDamageResult, TerrainObjectLifecycle, TerrainObjectState,
            damage_terrain_object_at_cell, mark_terrain_occupation, mark_terrain_raw_occupation,
        };
        use crate::sim::terrain_spawn::TerrainAnimationState;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nTreeStrength=10\n\
             [InfantryTypes]\n\
             [VehicleTypes]\n0=DUMMY\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [TerrainTypes]\n0=TREE01\n1=TIBTRE01\n\
             [DUMMY]\nPrimary=Gun\nStrength=100\nArmor=heavy\n\
             [Gun]\nDamage=10\nWarhead=WH\n\
             [WH]\nWood=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n\
             [TREE01]\nStrength=10\nArmor=wood\nImmune=no\nTemperateOccupationBits=7\n\
             [TIBTRE01]\nStrength=10\nArmor=wood\nImmune=yes\nSpawnsTiberium=yes\nTemperateOccupationBits=7\n",
        ))
        .expect("terrain damage rules");
        let mut sim = Simulation::new();
        let damaged_cell = (0, 0);
        let destroyed_cell = (1, 1);
        let spawner_cell = (2, 2);
        let damaged_id = 1;
        let destroyed_id = 2;
        let spawner_id = 3;
        let tree_type = sim.interner.intern("TREE01");
        let spawner_type = sim.interner.intern("TIBTRE01");
        let damaged = {
            let mut terrain =
                TerrainObjectState::for_test(damaged_id, tree_type, damaged_cell.0, damaged_cell.1);
            terrain.set_health_for_test(6);
            terrain.set_max_health_for_test(10);
            terrain.set_occupation_bits_for_test(7);
            terrain
        };
        let destroyed = {
            let mut terrain = TerrainObjectState::for_test(
                destroyed_id,
                sim.interner.intern("TREE01"),
                destroyed_cell.0,
                destroyed_cell.1,
            );
            terrain.set_health_for_test(10);
            terrain.set_max_health_for_test(10);
            terrain.set_occupation_bits_for_test(7);
            terrain
        };
        let spawner = {
            let mut terrain = TerrainObjectState::for_test(
                spawner_id,
                spawner_type,
                spawner_cell.0,
                spawner_cell.1,
            );
            terrain.set_health_for_test(10);
            terrain.set_max_health_for_test(10);
            terrain.set_occupation_bits_for_test(7);
            terrain
        };
        for terrain in [&damaged, &destroyed, &spawner] {
            sim.terrain_objects
                .objects_mut_for_test()
                .insert(terrain.stable_id(), terrain.clone());
            assert!(sim.register_terrain_object(terrain.stable_id(), &rules));
            sim.terrain_objects
                .cell_index_mut_for_test()
                .insert(terrain.cell(), terrain.stable_id());
            mark_terrain_raw_occupation(
                &mut sim.substrate.raw_cell_occupation,
                terrain.cell(),
                terrain.occupation_bits(),
            );
        }
        sim.set_next_stable_object_id_for_test(spawner_id + 1);
        sim.terrain_objects.animations_mut_for_test().insert(
            spawner_cell,
            TerrainAnimationState::new(
                spawner_type,
                crate::util::native_x87::NativeF32Bits::from_bits(0x3b44_9ba6),
                3,
                22,
                sim.session.binary_frame(),
            ),
        );
        sim.terrain_objects
            .spawning_cells_mut_for_test()
            .insert(spawner_cell);

        let mut original_occupied_grid = flat_terrain(3, 3);
        for terrain in [&damaged, &destroyed, &spawner] {
            mark_terrain_occupation(terrain, Some(&mut original_occupied_grid));
        }
        let stale_original_grid = original_occupied_grid.clone();
        assert!(
            !PathGrid::from_resolved_terrain(&stale_original_grid)
                .is_walkable(destroyed_cell.0, destroyed_cell.1)
        );
        sim.resolved_terrain = Some(original_occupied_grid);

        let result = damage_terrain_object_at_cell(
            &mut sim.terrain_objects,
            &mut sim.substrate.raw_cell_occupation,
            &rules,
            &sim.interner,
            destroyed_cell,
            10,
            rules.warhead("WH").expect("wood warhead"),
            sim.resolved_terrain.as_mut(),
        );
        assert_eq!(result, TerrainDamageResult::Destroyed);
        assert!(!sim.unregister_non_entity_object(destroyed_id));
        assert_eq!(sim.substrate.display.layer_of(destroyed_id), None);
        sim.substrate.pending_delete.queue_object(destroyed_id);
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // destroyed-terrain persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        assert!(sim.rebuild_dynamic_navigation(&rules));
        let authoritative_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_10_destroyed_terrain", 0);
        let mut restored = GameSnapshot::load(&bytes)
            .expect("destroyed Terrain snapshot")
            .sim;
        restored
            .restore_after_snapshot_load()
            .expect("restore serialized authority");
        restored.rebuild_caches_after_load(
            stale_original_grid,
            crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            &rules,
        );

        assert_eq!(
            restored.terrain_objects.objects()[&destroyed_id].lifecycle(),
            TerrainObjectLifecycle::Destroyed
        );
        assert!(
            !restored
                .terrain_objects
                .cell_index()
                .contains_key(&destroyed_cell)
        );
        let restored_damaged = &restored.terrain_objects.objects()[&damaged_id];
        assert_eq!(restored_damaged.health(), 6);
        assert_eq!(restored_damaged.lifecycle(), TerrainObjectLifecycle::Live);
        assert_eq!(restored_damaged.cell(), damaged_cell);
        assert_eq!(
            restored.terrain_objects.cell_index()[&damaged_cell],
            damaged_id
        );
        let restored_spawner = &restored.terrain_objects.objects()[&spawner_id];
        assert_eq!(restored_spawner.health(), 10);
        assert_eq!(restored_spawner.lifecycle(), TerrainObjectLifecycle::Live);
        assert_eq!(restored_spawner.cell(), spawner_cell);
        assert_eq!(
            restored.terrain_objects.cell_index()[&spawner_cell],
            spawner_id
        );
        assert_eq!(
            restored.terrain_objects.animations()[&spawner_cell].type_ref(),
            spawner_type
        );
        assert!(
            restored
                .terrain_objects
                .spawning_cells()
                .contains(&spawner_cell)
        );
        assert_eq!(restored.next_stable_object_id(), spawner_id + 1);
        let reconciled = restored
            .resolved_terrain
            .as_ref()
            .expect("reconciled terrain");
        let source = reconciled
            .cell(destroyed_cell.0, destroyed_cell.1)
            .expect("source cell");
        assert_eq!(source.terrain_object_occupation, None);
        assert!(!source.terrain_object_blocks);
        assert!(!source.ground_walk_blocked);
        assert_eq!(source.zone_type, zone_class::GROUND);
        for (cell, bits) in [(damaged_cell, 7), (spawner_cell, 7)] {
            let live = reconciled.cell(cell.0, cell.1).expect("live Terrain cell");
            assert_eq!(live.terrain_object_occupation, Some(bits));
            assert!(live.terrain_object_blocks);
            assert!(live.ground_walk_blocked);
            let raw_mask = crate::sim::terrain_object::terrain_raw_occupation_mask(bits);
            assert_eq!(
                restored
                    .substrate
                    .raw_cell_occupation
                    .ground_bits(cell.0, cell.1)
                    & raw_mask,
                raw_mask
            );
        }
        assert_eq!(
            restored
                .substrate
                .raw_cell_occupation
                .ground_bits(destroyed_cell.0, destroyed_cell.1)
                & crate::sim::terrain_object::terrain_raw_occupation_mask(
                    destroyed.occupation_bits(),
                ),
            0
        );

        let path = PathGrid::from_resolved_terrain_with_bridges(
            reconciled,
            restored.overlay_grid.as_ref(),
        );
        assert!(path.is_walkable(destroyed_cell.0, destroyed_cell.1));
        assert!(!path.is_walkable(damaged_cell.0, damaged_cell.1));
        assert!(!path.is_walkable(spawner_cell.0, spawner_cell.1));
        assert!(restored.rebuild_dynamic_navigation(&rules));
        assert_ne!(
            restored
                .zone_grid
                .as_ref()
                .and_then(|zones| zones.map_for(MovementZone::Normal))
                .expect("normal zone map")
                .zone_at(destroyed_cell.0, destroyed_cell.1, MovementLayer::Ground),
            ZONE_INVALID
        );
        assert_eq!(restored.state_hash(), authoritative_hash);

        let mut changed_health = GameSnapshot::load(&bytes)
            .expect("health-mutation control snapshot")
            .sim;
        changed_health
            .terrain_objects
            .objects_mut_for_test()
            .get_mut(&damaged_id)
            .expect("damaged Terrain authority")
            .set_health_for_test(5);
        assert_ne!(changed_health.state_hash(), authoritative_hash);

        let mut changed_lifecycle = GameSnapshot::load(&bytes)
            .expect("lifecycle-mutation control snapshot")
            .sim;
        changed_lifecycle
            .terrain_objects
            .objects_mut_for_test()
            .get_mut(&spawner_id)
            .expect("spawning Terrain authority")
            .set_lifecycle_for_test(TerrainObjectLifecycle::Limbo);
        assert_ne!(changed_lifecycle.state_hash(), authoritative_hash);
    }

    #[test]
    fn saveload_rebuild_is_deterministic() {
        use crate::map::entities::EntityCategory;
        use crate::sim::game_entity::GameEntity;

        let terrain = flat_terrain(8, 8);
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        for (stable_id, type_id, category, rx, ry) in [
            (3, "GAPOWR", EntityCategory::Structure, 2, 2),
            (1, "MTNK", EntityCategory::Unit, 3, 2),
            (2, "E1", EntityCategory::Infantry, 3, 2),
        ] {
            let mut entity = GameEntity::test_default(stable_id, type_id, "Americans", rx, ry);
            entity.set_owner_for_test(owner);
            entity.set_category_for_test(category);
            if category == EntityCategory::Infantry {
                entity.set_sub_cell(Some(2));
            }
            sim.substrate.entities.insert(entity);
            sim.add_entity_occupancy(stable_id);
        }
        let bytes = GameSnapshot::save(&sim, 0, 0, "deterministic_rebuild", 0);

        let mut a = GameSnapshot::load(&bytes)
            .expect("first load should succeed")
            .sim;
        let mut b = GameSnapshot::load(&bytes)
            .expect("second load should succeed")
            .sim;
        rebuild_load_caches(&mut a, terrain.clone());
        rebuild_load_caches(&mut b, terrain);

        assert_eq!(cell_order(&a, 3, 2, MovementLayer::Ground), vec![2, 1]);
        assert_eq!(
            cell_order(&a, 3, 2, MovementLayer::Ground),
            cell_order(&b, 3, 2, MovementLayer::Ground)
        );

        let path_a = PathGrid::from_resolved_terrain_with_bridges(
            a.resolved_terrain.as_ref().expect("terrain restored"),
            a.overlay_grid.as_ref(),
        );
        let path_b = PathGrid::from_resolved_terrain_with_bridges(
            b.resolved_terrain.as_ref().expect("terrain restored"),
            b.overlay_grid.as_ref(),
        );
        assert_eq!(path_a, path_b);

        a.rebuild_zone_grid(&path_a);
        b.rebuild_zone_grid(&path_b);
        assert_zone_grids_equivalent(
            a.zone_grid.as_ref().expect("zone grid rebuilt"),
            b.zone_grid.as_ref().expect("zone grid rebuilt"),
        );
        assert_eq!(a.state_hash(), b.state_hash());
    }

    // --- Slice 1: reveal/conceal/unlimbo/uninit lifecycle chokepoint ---

    /// Slice 3: `unlimbo(ge)` places the entity into BOTH the active order and
    /// occupancy in one atomic call — a caller can never observe it in `logic`
    /// without occupancy, because the method returns only after both. The
    /// house tracks it (Add_Tracking, Added_To_Game). (No-op collapse: same
    /// end state as the old 4-step.)
    #[test]
    fn unlimbo_ge_places_into_logic_and_occupancy_atomically() {
        use crate::sim::game_entity::GameEntity;
        let mut sim = Simulation::new();
        let ge = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        // `place_spawned` resolves the owner and Unlimbo the type against
        // `sim.interner`; test_default interns both in the test interner.
        sim.interner = crate::sim::intern::test_interner();
        let (id, outcome) = sim.unlimbo(ge);
        assert!(matches!(outcome, RevealOutcome::Revealed { .. }));

        let e = sim.substrate.entities.get(id).expect("entity in store");
        assert!(e.in_logic_vector.is_member(), "must be in the active order");
        assert!(!e.lifecycle.in_limbo());
        assert!(e.lifecycle.cell_marked());
        assert_eq!(sim.live_object_order_snapshot(), vec![id]);
        assert!(
            sim.substrate.occupancy.contains_entity(5, 5, id),
            "must be registered in its foundation cell",
        );
        #[cfg(debug_assertions)]
        sim.debug_assert_lifecycle_consistent();
    }

    /// Slice 3: `create_limbo(ge)` stores the entity and its house tracks it
    /// (Add_Tracking) but leaves it OUT of the active order and OUT of occupancy (born InLimbo).
    #[test]
    fn create_limbo_leaves_entity_out_of_logic_and_occupancy() {
        use crate::sim::game_entity::GameEntity;
        let mut sim = Simulation::new();
        let mut ge = GameEntity::test_default(2, "E1", "Americans", 6, 6);
        // `place_spawned` resolves the owner against `sim.interner`; re-intern so
        // the id is valid there (test_default uses the thread-local test interner).
        ge.set_owner_for_test(sim.interner.intern("Americans"));
        let id = Simulation::create_limbo(&mut sim.substrate, &mut sim.houses, ge);

        let e = sim.substrate.entities.get(id).expect("entity in store");
        assert!(
            !e.in_logic_vector.is_member(),
            "limbo object is not an active member"
        );
        assert!(e.lifecycle.in_limbo());
        assert!(!e.lifecycle.cell_marked());
        assert!(sim.live_object_order_snapshot().is_empty());
        assert!(
            !sim.substrate.occupancy.contains_entity(6, 6, id),
            "limbo object must not occupy a cell",
        );
    }

    // --- LogicClass live count-reload pass (scheduler contract) ---

    /// Insert an entity into the store and append it to the active order.
    fn spawn_and_register(sim: &mut Simulation, id: u64) {
        use crate::sim::game_entity::GameEntity;
        sim.substrate
            .entities
            .insert(GameEntity::test_default(id, "MTNK", "Americans", 5, 5));
        sim.register_live_object(id);
    }

    /// Registering the same object twice is a no-op: the order keeps one entry
    /// and the body runs for it exactly once.
    #[test]
    fn logic_scheduler_duplicate_registration_is_idempotent() {
        let mut sim = Simulation::new();
        spawn_and_register(&mut sim, 1);
        sim.register_live_object(1); // duplicate
        assert_eq!(sim.live_object_order_snapshot(), vec![1]);

        let mut visits = 0;
        sim.for_each_live_object(|_, id| {
            if id == 1 {
                visits += 1;
            }
        });
        assert_eq!(visits, 1);
    }

    /// Literal saved Cell lists survive re-entry order and actual restore.
    /// Restore hydrates metadata; it does not replay the historical enter stamps.
    #[test]
    fn saveload_occupancy_list_order_survives_reentry() {
        use crate::map::entities::EntityCategory;
        use crate::sim::game_entity::GameEntity;
        let mut sim = Simulation::new();
        for _ in 0..3 {
            let id = sim.substrate.allocate_stable_id();
            let mut entity = GameEntity::test_default(id, "E1", "Americans", 5, 5);
            entity.set_category_for_test(EntityCategory::Infantry);
            entity.lifecycle.set_in_limbo_for_test(false);
            sim.substrate.entities.insert(entity);
            sim.add_entity_occupancy(id);
        }
        sim.interner = crate::sim::intern::test_interner();
        sim.remove_entity_occupancy(1);
        sim.add_entity_occupancy(1);
        let list = |sim: &Simulation| -> Vec<u64> {
            sim.substrate
                .occupancy
                .get(5, 5)
                .unwrap()
                .iter_layer(MovementLayer::Ground)
                .map(|o| o.entity_id)
                .collect()
        };
        assert_eq!(list(&sim), vec![1, 3, 2]);
        // Snapshot deserialization intentionally executes native Scenario Seed0.
        // Compare this state-only roundtrip on that same admitted RNG cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let hash = sim.state_hash();
        let saved = GameSnapshot::save(&sim, 0, 0, "re-entry list", 0);
        let mut restored = GameSnapshot::load(&saved).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(list(&restored), vec![1, 3, 2]);
        assert_eq!(restored.state_hash(), hash);
        restored.remove_entity_occupancy(3);
        assert_eq!(list(&restored), vec![1, 2]);
        restored.add_entity_occupancy(3);
        assert_eq!(list(&restored), vec![3, 1, 2]);
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(list(&restored), vec![3, 1, 2], "fixup is idempotent");
    }

    /// Every saved type traces to one description, and its id can never equal
    /// a counter from the hand-kept scheme.
    #[test]
    fn snapshot_layout_describes_every_saved_type() {
        let description = save_codec::layout::describe::<GameSnapshot>()
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(description.starts_with("GameSnapshot\n"));
        assert!(description.contains("\nSimulation struct{"));
        assert_ne!(super::snapshot_layout() & 0x8000_0000, 0);
        assert_eq!(super::snapshot_layout(), super::snapshot_layout());
    }
}
