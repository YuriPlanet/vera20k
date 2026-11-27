//! Native map-authored live Tag/Trigger ownership.
//!
//! Type registration follows Full_Init686B20's TeamType first references,
//! physical TriggerType reads and physical TagType reads. Runtime construction,
//! timer writes, Logic55AFB0 polling and deferred deletion all mutate this one
//! Simulation-owned, serialized authority. Reproducible bounded native histories:
//! tools/spatial_oracle/tag_lifecycle.{py,json,meta.json}.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

use crate::map::actions::ActionEntry;
use crate::map::cell_index::NativeCellIdentity;
use crate::map::events::EventCondition;
#[cfg(test)]
use crate::map::resolved_terrain::NativeCellQuery;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ini_value::crt_atoi;
#[cfg(test)]
use crate::sim::rng::SimRng;
use crate::sim::timer::CdTimer;
#[cfg(test)]
use crate::sim::world::FrameEffects;
#[cfg(test)]
use crate::sim::world::SimSoundEvent;
use crate::sim::world::Simulation;
use crate::util::state_hasher::hash_indices;

const ACTION_PLAY_SOUND: i32 = 19;
const ACTION_FORCE_TRIGGER: i32 = 22;
const ACTION_SET_GLOBAL: i32 = 28;
const ACTION_CLEAR_GLOBAL: i32 = 29;
const ACTION_CHANGE_VISIBLE_MAP_AREA: i32 = 40;
const ACTION_CENTER_CAMERA: i32 = 48;
const ACTION_ENABLE_TRIGGER: i32 = 53;
const ACTION_DISABLE_TRIGGER: i32 = 54;
const ACTION_SET_LOCAL: i32 = 56;
const ACTION_CLEAR_LOCAL: i32 = 57;
// VERA untrusted-map admission, not native Spring7265C0 semantics. Force22
// bypasses Tag's dispatch guard. Bound acyclic nesting and repeated branching
// too, so an authored graph cannot overflow the stack or expand exponentially.
const MAX_TRIGGER_SPRING_DEPTH: usize = 32;
const MAX_TRIGGER_SPRING_ACTIONS: usize = 4096;
const ACTION_DAMAGE_AT_WAYPOINT: i32 = 63;
const ACTION_ANNOUNCE_WIN: i32 = 67;
const ACTION_ANNOUNCE_LOSE: i32 = 68;
const ACTION_END_SCENARIO: i32 = 69;
const ACTION_PLAY_SOUND_AT_WAYPOINT: i32 = 99;
const ACTION_JUMP_CAMERA: i32 = 112;
const ACTION_SET_SUPER_CHARGE_PERCENTAGE: i32 = 129;
const ACTION_SET_ALTERNATE_BASE: i32 = 137;
const ACTION_CLEAR_ALTERNATE_BASE: i32 = 138;

const EVENT_ATTACKED: i32 = 6;
const EVENT_ANY: i32 = 8;
const EVENT_BRIDGE_COLLAPSED: i32 = 31;
const EVENT_GLOBAL_IS_SET: i32 = 27;
const EVENT_GLOBAL_IS_CLEAR: i32 = 28;
const EVENT_LOCAL_IS_SET: i32 = 36;
const EVENT_LOCAL_IS_CLEAR: i32 = 37;
const EVENT_ELAPSED_SCENARIO_TIME: i32 = 47;
const EVENT_TECHTYPE_EXISTS: i32 = 60;
const EVENT_TECHTYPE_DOES_NOT_EXIST: i32 = 61;
const LOGICAL_FRAMES_PER_SECOND: i32 = crate::util::fixed_math::RA2_LOGIC_FRAMES_PER_SECOND as i32;

/// Action20's music operand. Fresh reads retain the process catalog's index;
/// Name remains supported for old saved actions/effects and existing callers.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ThemeRequest {
    Name(String),
    Index(i32),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TriggerEffect {
    CameraAtCoord {
        coord: [i32; 3],
        velocity: Option<i32>,
    },
    RecentEventCell {
        cell: (i16, i16),
    },
    InputLockChanged {
        locked: bool,
        was_locked: bool,
    },
    /// TAction46's Display clears ahead of its lock: the current placement
    /// and its foundation, then the cursor modes. The other locks only lock.
    CancelPlacementModes,
    LocalizedMessage {
        csf_label: String,
    },
    /// A Wasm extension module stopped (`sim::ext_modules`); the players are
    /// told which and why (RFC 0001 section 3).
    ExtModuleStopped {
        reason: String,
    },
    /// Execute114 asks the retained client sidebar to select a nonempty strip.
    SelectSidebarTab {
        index: i32,
    },
    /// Execute115's named TechnoType cameo pulse, on the native frame clock.
    FlashSidebarCameo {
        category: crate::sim::production::ProductionCategory,
        type_index: i32,
        duration: i32,
        frame: i32,
    },
    /// A synchronous trigger receiver already published changed bridge state.
    /// The frame owner includes this receipt in its existing terrain refresh.
    BridgeStateChanged,
    /// Ordered native100/117 request. The app owns media, audio and clocks;
    /// 117 acquires a scenario hold only when its queued clip is drawn.
    RadarMovie {
        index: i32,
        pause_simulation: bool,
    },
    /// Execute10's5BF260 full-screen movie. The app's existing movie owner
    /// suspends its scenario visit and resumes that same session afterwards.
    FullscreenMovie {
        index: i32,
    },
    /// ChronoScreen53B460/53B560 edges; simulation owns its phase/cadence,
    /// while app resources own captured frames and physical audio.
    ChronoScreen {
        command: crate::sim::superweapon::chrono_screen::ChronoScreenCommand,
    },
    /// Execute110/6DF6FD forwards raw+90 to ScenarioPause684060.
    /// Its clock is the app system clock, not the deterministic world clock.
    ScenarioPause {
        duration: i32,
        outermost: bool,
    },
    ScenarioResume,
    /// Execute20 queues the song; the shared Theme owner controls admission,
    /// fade, subsequent playback and the process Main RNG.
    MusicTheme {
        request: ThemeRequest,
    },
    /// Home7315A0 also clears Follow, including when no coordinate is found.
    HomeCamera {
        coord: Option<[i32; 3]>,
    },
    /// 70F120 from trigger action 105 or team script action 52: the app owns
    /// the process wall-clock bubble. Reveal is synchronous in sim; this
    /// ordered cue never becomes gameplay state.
    TalkBubble {
        actor: Option<u64>,
        value: i32,
        duration: i32,
    },
    /// Ares trigger action 148 (TActionExt::Execute 100396C0) selects the
    /// announcer voice, VoxClass::EVAIndex: a process cache the client's
    /// announcer owns, which applies the voice-count bound.
    EvaVoice {
        index: i32,
    },
    /// Something about the scenario's Wasm extension modules the players
    /// should know at the start, such as a map's ignored `Wasm.Tags` (RFC
    /// 0001 section 2). Last, so saved effects keep their variant numbers.
    ExtModuleNotice {
        text: String,
    },
}

/// Original definition pointers translated to retained registry ordinals.
/// Resolve once while Read6DD5B0 executes; later constructors cannot change the
/// meaning of a short index or make a missing reference spring a new instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
enum DefinitionReference {
    Team(usize),
    Trigger(usize),
    Tag(usize),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct RuntimeAction {
    entry: ActionEntry,
    reference: Option<DefinitionReference>,
    /// Original Read6DD5B0 materializes +90 once, including Vox753250.
    value: Option<i32>,
    /// Offline: the SOUNDMD name resolved by Read6DD7CE/FindIndex7514D0.
    /// Network playback: the authored name or nonnegative literal slot,
    /// resolved only by the client's audio owner. Also carries action21's
    /// network EVA reference; gameplay uses the unchanged numeric `value`.
    sound: Option<crate::rules::sound_ini::SoundReference>,
}

impl RuntimeAction {
    fn playback_operand(&self) -> bool {
        let kind = self.entry.kind;
        match self.entry.params.first().map_or(0, |value| crt_atoi(value)) {
            6 => kind == 21,
            7 => matches!(kind, 19 | 98 | 99),
            8 => kind == 20,
            _ => matches!(kind, 19 | 20 | 21 | 98 | 99),
        }
    }

    // VERA network adaptation: ObjectClass::Set_Custom_Sound5F6CB0 and
    // AI5F3E70 keep the same requested identity and wake/sleep admission on
    // every peer even when one client cannot play it. A negative literal
    // still means no sound. No RNG, timer or detach is added by this read.
    //
    // RESIDUAL: an action-99 name stays a name here, so where this player's
    // catalog lacks it the object keeps a custom sound (and stays awake for
    // it) whose every PlayAt7509E0 visit finds no cue and stops the object's
    // live handle: its previous loop is cut. Native resolves the name in each
    // player's own catalog to -1, and Set_Custom_Sound5F6CB0(-1) detaches the
    // handle through 405FD0, so the loop plays out. Network games only; a
    // misspelt, mod-only or locally missing name on a building or tree that
    // already plays a custom sound.
    fn retain_network_playback(&mut self) {
        if !self.playback_operand() || self.entry.kind == 20 {
            return;
        }
        use crate::rules::sound_ini::SoundReference;
        self.sound = match self.entry.params.first().map_or(0, |value| crt_atoi(value)) {
            6 | 7 => self
                .entry
                .params
                .get(1)
                .filter(|name| !name.is_empty())
                .map(|name| SoundReference::Name(name.to_ascii_uppercase())),
            _ => self
                .value
                .filter(|index| *index >= 0)
                .map(SoundReference::Index),
        };
    }
}

impl Hash for RuntimeAction {
    fn hash<H: Hasher>(&self, hasher: &mut H) {
        self.entry.hash(hasher);
        self.reference.hash(hasher);
        // VERA network adaptation: Read6DD7BC/6DD7CE/6DD7E0 resolves a
        // playback operand against a process catalog. Its raw name/index is
        // already in entry; local resolution is not shared simulation state.
        // An unusual action such as SetTimer27 still consumes +90 as gameplay
        // and must keep its actual resolved value in the hash.
        if !self.playback_operand() {
            self.value.hash(hasher);
            self.sound.hash(hasher);
        }
    }
}

/// Original shared TEvent+54, constructed null. Map parsing owns only the
/// condition; this Type-owned node retains the actual entering House across
/// live instances, Reset726400 and Enable. EventRegister7264C0 forwards it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct RuntimeEvent {
    condition: EventCondition,
    retained_house: Option<crate::sim::intern::InternedId>,
}
impl From<EventCondition> for RuntimeEvent {
    fn from(condition: EventCondition) -> Self {
        Self {
            condition,
            retained_house: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct TriggerType {
    id: String,
    owner: Option<String>,
    linked: Option<usize>,
    enabled: bool,
    /// Literal Type+9C/9D/9E order, indexed by Scenario difficulty0/1/2.
    difficulty: [bool; 3],
    events: Vec<RuntimeEvent>,
    /// Header token8, sticky Type+A0 Read727475..72749B; Engineer Tag transfer.
    transfer_on_capture: bool,
    actions: Vec<RuntimeAction>,
}

impl TriggerType {
    // Constructor726C80, not a header manufactured from retail defaults.
    fn new(id: String) -> Self {
        Self {
            id,
            owner: None,
            linked: None,
            enabled: true,
            difficulty: [true; 3],
            events: Vec::new(),
            transfer_on_capture: false,
            actions: Vec::new(),
        }
    }
    fn admits_difficulty(&self, difficulty: i32) -> bool {
        usize::try_from(difficulty)
            .ok()
            .and_then(|i| self.difficulty.get(i))
            .copied()
            .unwrap_or(true)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct TagType {
    id: String,
    /// TagType6E6080 stores a separate48-byte display name used by6E5E70.
    name: String,
    repeat: i32,
    trigger: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct RuntimeDefinitions {
    team_types: Vec<String>,
    /// TeamType+D0 references resolved during physical Read6F1341. This is
    /// the sole owner; Team constructors consume the retained ordinal and
    /// never re-run a factory or infer the pointer from a truncated Type ID.
    team_tags: Vec<Option<usize>>,
    trigger_types: Vec<TriggerType>,
    tag_types: Vec<TagType>,
}

mod actions;
mod definitions;
mod dispatch;
mod events;
mod lifecycle;
mod module_events;
mod module_triggers;
#[cfg(any(test, feature = "dev-overlays"))]
mod trace;
mod untriggered;

pub(crate) use untriggered::{StockActionCall, StockReference, StockReferenceKind};

pub(crate) use module_events::{
    StockClass, StockInput, StockReading, stock_class, stock_poll_due, value_filters_actor_house,
    value_names_house,
};
pub(crate) use module_triggers::{
    DeferredFiring, MAX_BACKLOG_RECORDS, MAX_QUEUED_RECORDS, MAX_RECORDS_PER_FRAME, ModuleTrigger,
    ModuleTriggers, TriggerScope,
};

#[cfg(test)]
use definitions::event_classification;
#[cfg(any(test, feature = "dev-overlays"))]
use trace::TriggerTrace;
#[cfg(any(test, feature = "dev-overlays"))]
pub use trace::{TraceTag, TriggerSpringRecord, TriggerTraceEvent};

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct LiveTrigger {
    type_index: usize,
    timer: CdTimer,
    completed: u32,
    /// Live+2C, ctor725FB5 null; only nonnull shared Event+54 forwards here.
    retained_house: Option<crate::sim::intern::InternedId>,
    enabled: bool,
    pending: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct LiveTag {
    type_index: usize,
    ///6E4F44 prepends instances, reversing definition-construction order.
    triggers: Vec<usize>,
    /// Native+2C is written by attachment operations, not reconstructed from
    /// live Object/Cell indexes. Object5F3B80 frees an attached Object without
    /// decrementing this counter; damage-events' destruction controls pin it.
    refs: i32,
    attached_cell: (i16, i16),
    pending: bool,
    dispatch_guard: bool,
}
// The derived fold, but with the trigger list as 64-bit words on every
// target ([`hash_indices`]).
impl Hash for LiveTag {
    fn hash<H: Hasher>(&self, hasher: &mut H) {
        self.type_index.hash(hasher);
        hash_indices(&self.triggers, hasher);
        self.refs.hash(hasher);
        self.attached_cell.hash(hasher);
        self.pending.hash(hasher);
        self.dispatch_guard.hash(hasher);
    }
}
/// Team IDs and Techno stable IDs belong to separate identity domains.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
enum TagAttachment {
    Object(u64),
    Team(u64),
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TriggerRuntime {
    globals_set: BTreeSet<u32>,
    locals_set: BTreeSet<u32>,
    definitions: RuntimeDefinitions,
    tags: Vec<Option<LiveTag>>,
    triggers: Vec<Option<LiveTrigger>>,
    /// Native registry order survives storage holes and stable-removal.
    tag_order: Vec<usize>,
    trigger_order: Vec<usize>,
    logic_tags: Vec<usize>,
    map_tags: Vec<usize>,
    house_tags: BTreeMap<crate::sim::intern::InternedId, Vec<usize>>,
    attachments: BTreeMap<TagAttachment, usize>,
    /// CellClass allocation identity, including the process-wide Dummy. Two
    /// authored coordinates can resolve to one retained cell attachment.
    cell_tags: BTreeMap<NativeCellIdentity, usize>,
    /// Scenario+34AA/34A9/34AB/34BE, consumed in Logic55AFB0 order.
    dirty: [bool; 4],
    logic_cursor: i32,
    map_cursor: i32,
    definitions_prepared: bool,
    classified: bool,
    /// Triggers Wasm extension modules made, with their bindings and queued
    /// records (RFC 0001 section 7). Empty without modules.
    module: ModuleTriggers,
    /// The empty stand-in of a presentation copy of the world
    /// ([`Self::detached`]). Debug builds refuse reads of it.
    #[serde(skip)]
    detached: bool,
    /// Whether event sites also build payloads for client modules' forwarded
    /// `SimEvent` kinds (`sim::ext_modules::client_outbox`). Presentation
    /// only: set when modules are installed or restored, never saved.
    #[serde(skip)]
    client_forward: bool,
    /// Scratch for one synchronous Spring call tree. Empty between dispatches;
    /// save/restore and checksums keep only the authoritative trigger state.
    #[serde(skip)]
    spring_depth: usize,
    #[serde(skip)]
    spring_actions_left: usize,
    #[cfg(any(test, feature = "test-support"))]
    #[serde(skip)]
    fixture_difficulty: Option<i32>,
    #[cfg(any(test, feature = "test-support"))]
    #[serde(skip)]
    action_trace: Vec<(i32, i32)>,
    #[cfg(any(test, feature = "test-support"))]
    #[serde(skip)]
    dispatch_trace: Vec<(usize, i32)>,
    #[cfg(any(test, feature = "test-support"))]
    #[serde(skip)]
    freed_trace: Vec<&'static str>,
    /// Native stock comparison abandons its real first-poll stack at the same
    /// first gameplay action boundary; no action receives a success substitute.
    #[cfg(any(test, feature = "test-support"))]
    #[serde(skip)]
    stop_before_action: Option<i32>,
    /// Dev campaign trigger-trace capture: `Some` while recording.
    #[cfg(any(test, feature = "dev-overlays"))]
    #[serde(skip)]
    trace: Option<TriggerTrace>,
}

/// Draw-free transfer of the outgoing Scenario's local values into a fresh
/// Scenario. TriggerRuntime remains authoritative; saved restoration does not
/// use this transfer. ClearScene6851F0 clears globals, but retains these slots.
#[derive(Debug)]
pub struct RetainedLocalVariables {
    set: BTreeSet<u32>,
}

impl PartialEq for TriggerRuntime {
    fn eq(&self, other: &Self) -> bool {
        self.globals_set == other.globals_set
            && self.locals_set == other.locals_set
            && self.definitions == other.definitions
            && self.tags == other.tags
            && self.triggers == other.triggers
            && self.tag_order == other.tag_order
            && self.trigger_order == other.trigger_order
            && self.logic_tags == other.logic_tags
            && self.map_tags == other.map_tags
            && self.house_tags == other.house_tags
            && self.attachments == other.attachments
            && self.cell_tags == other.cell_tags
            && self.dirty == other.dirty
            && self.logic_cursor == other.logic_cursor
            && self.map_cursor == other.map_cursor
            && self.definitions_prepared == other.definitions_prepared
            && self.classified == other.classified
            && self.module == other.module
    }
}
impl Eq for TriggerRuntime {}

impl TriggerRuntime {
    /// The empty runtime a presentation copy of the world holds in place of
    /// the triggers (`Simulation::presentation_copy`): only gameplay reads
    /// them. Debug builds panic on a read of it, so a reader of the copy
    /// shows up instead of finding no trigger.
    pub(crate) fn detached() -> Self {
        Self {
            detached: true,
            ..Self::default()
        }
    }

    /// Debug builds: refuse a read of a [`Self::detached`] runtime.
    #[inline]
    pub(crate) fn check_attached(&self) {
        debug_assert!(
            !self.detached,
            "read of the triggers of a presentation copy of the world: only the live world holds them"
        );
    }

    /// `TeamTypeClass::Array[index]` (`0x00A8ECA4`) by ID, in constructor
    /// order. None past its end, where native reads out of bounds.
    pub(crate) fn team_type_at(&self, index: usize) -> Option<&str> {
        self.definitions.team_types.get(index).map(String::as_str)
    }

    /// Trigger deleting destructor726950 selected by the common725C70 owner.
    /// Pointer expiry splices this identity out of every live Tag chain.
    pub(crate) fn delete_live_trigger(&mut self, index: usize) {
        if self.trigger(index).is_none() {
            return;
        }
        for tag in self.tags.iter_mut().flatten() {
            tag.triggers.retain(|&t| t != index);
        }
        self.trigger_order.retain(|&t| t != index);
        self.triggers[index] = None;
        #[cfg(any(test, feature = "test-support"))]
        self.freed_trace.push("trigger");
    }

    pub(crate) fn has_live_tag(&self, id: usize) -> bool {
        self.tag(id).is_some()
    }
    pub(crate) fn has_live_trigger(&self, id: usize) -> bool {
        self.trigger(id).is_some()
    }

    /// Map568CDB..568DCA's fresh InitCellAttributes line-marker subset.
    /// The caller supplies its existing anti-diagonal final cell iterator and
    /// the MapClass cell-array rect (`+0x124..+0x130`) the lines span.
    /// Horizontal25 wins over vertical26 for a Tag carrying both. Flags
    /// remain on cells after a later Tag detach, as the native writer does.
    /// Steam gamemd; tools/spatial_oracle/tag_lifecycle --line-events.
    pub(crate) fn initialize_cell_trigger_lines(
        &self,
        bounds: crate::sim::scenario_bootstrap::NativeStartBounds,
        terrain: &mut ResolvedTerrainGrid,
        cells: impl Iterator<Item = (i32, i32)>,
    ) {
        for (x, y) in cells {
            let source = terrain.native_cell_identity((x as i16, y as i16));
            let Some(tag) = self.cell_tags.get(&source).copied() else {
                continue;
            };
            let Some(tag) = self.tag(tag) else {
                continue;
            };
            let coord = terrain.native_cell_coord(source);
            let (kind, count, flag) = if self.definitions.has_event(tag.type_index, 25) {
                (25, bounds.width, 0x100000)
            } else if self.definitions.has_event(tag.type_index, 26) {
                (26, bounds.height, 0x200000)
            } else {
                continue;
            };
            for offset in 0..count {
                let cell = if kind == 25 {
                    (bounds.min_rx.wrapping_add(offset) as i16, coord.1)
                } else {
                    (coord.0, bounds.min_ry.wrapping_add(offset) as i16)
                };
                let identity = terrain.native_cell_identity(cell);
                let flags = terrain.native_cell_flags(identity) | flag;
                terrain.write_native_cell_flags(identity, flags);
            }
        }
    }
    pub(crate) fn hash_state(&self, hasher: &mut impl Hasher) {
        // Definition ordinals, timers, active registration order, attachments
        // and the deferred queue affect later actions and are all retained.
        // Observers never participate in this fold. Index lists fold as
        // 64-bit words on every target ([`hash_indices`]).
        self.hash_globals(hasher);
        self.hash_locals(hasher);
        self.hash_definitions(hasher);
        self.hash_live(hasher);
    }

    // A start report lists the four parts of `hash_state` apart
    // (`Simulation::state_hash_owners`); together they fold the same bytes.

    /// The set Scenario globals.
    pub(crate) fn hash_globals(&self, hasher: &mut impl Hasher) {
        self.globals_set.hash(hasher);
    }

    /// The set locals.
    pub(crate) fn hash_locals(&self, hasher: &mut impl Hasher) {
        self.locals_set.hash(hasher);
    }

    /// The map's TeamType names, and its trigger and Tag types with their
    /// events and actions.
    pub(crate) fn hash_definitions(&self, hasher: &mut impl Hasher) {
        self.definitions.hash(hasher);
    }

    /// The live Tags and triggers, their orders, attachments and cursors.
    pub(crate) fn hash_live(&self, hasher: &mut impl Hasher) {
        self.tags.hash(hasher);
        self.triggers.hash(hasher);
        hash_indices(&self.tag_order, hasher);
        hash_indices(&self.trigger_order, hasher);
        hash_indices(&self.logic_tags, hasher);
        hash_indices(&self.map_tags, hasher);
        self.house_tags.len().hash(hasher);
        for (house, tags) in &self.house_tags {
            house.hash(hasher);
            hash_indices(tags, hasher);
        }
        self.attachments.hash(hasher);
        self.cell_tags.hash(hasher);
        self.dirty.hash(hasher);
        self.logic_cursor.hash(hasher);
        self.map_cursor.hash(hasher);
        self.definitions_prepared.hash(hasher);
        self.classified.hash(hasher);
        // Stock play has no module triggers and folds nothing new.
        if !self.module.is_empty() {
            b"module-triggers-v1".hash(hasher);
            self.module.hash(hasher);
        }
    }

    /// The module trigger registry (`module_triggers`).
    pub(crate) fn module_triggers(&self) -> &ModuleTriggers {
        &self.module
    }

    pub(crate) fn module_triggers_mut(&mut self) -> &mut ModuleTriggers {
        &mut self.module
    }

    /// Whether a module trigger listens for event `kind`: the one check an
    /// event site makes before it builds a payload (RFC 0001 section 15).
    /// With client forwarding on it is also true for the kinds client
    /// modules receive as `SimEvent`.
    #[inline]
    pub(crate) fn module_listens(&self, kind: i32) -> bool {
        self.module.listens(kind) || self.forwards_to_clients(kind)
    }

    /// Whether an event of `kind` goes to the client outbox.
    #[inline]
    pub(crate) fn forwards_to_clients(&self, kind: i32) -> bool {
        self.client_forward && crate::sim::ext_modules::CLIENT_FORWARDED_EVENTS.contains(&kind)
    }

    pub(crate) fn set_client_forwarding(&mut self, on: bool) {
        self.client_forward = on;
    }

    /// Whether a module trigger decides event `kind`: the one check a
    /// decision point makes before it builds a request (RFC 0001 15.1).
    #[inline]
    pub(crate) fn module_decides(&self, kind: i32) -> bool {
        self.module.decides(kind)
    }

    /// VERA-only match-checksum terms
    /// ([`Simulation::compute_multiplayer_frame_checksum`]): each set
    /// Scenario global, then each set local (high bit set), then each live
    /// trigger in registration order with what `TriggerClass::ComputeCRC @
    /// 0x00726790` folds (type, Destroyed `+0x30`, Enabled, the timer's
    /// remaining time, the occurred-event bits), then each live Tag with
    /// what `TagClass::ComputeCRC @ 0x006E56E0` folds (type, Destroyed).
    /// `ScenarioClass::ComputeCRC @ 0x0068B940` folds no variable; VERA adds
    /// them because actions write them and events poll them.
    pub(crate) fn fold_match_checksum(
        &self,
        frame: i32,
        checksum: &mut crate::sim::multiplayer_checksum::RetailChecksumAccumulator,
    ) {
        for &index in &self.globals_set {
            checksum.fold_value(index);
        }
        for &index in &self.locals_set {
            checksum.fold_value(index | 0x8000_0000);
        }
        for trigger in self.trigger_order.iter().filter_map(|&i| self.trigger(i)) {
            checksum.fold_value(trigger.type_index as u32);
            checksum.fold_value(u32::from(trigger.pending) | u32::from(trigger.enabled) << 1);
            checksum.fold_value(trigger.timer.remaining(frame) as u32);
            checksum.fold_value(trigger.completed);
        }
        for tag in self.tag_order.iter().filter_map(|&i| self.tag(i)) {
            checksum.fold_value(tag.type_index as u32);
            checksum.fold_value(u32::from(tag.pending));
        }
        self.module.fold_match_checksum(checksum);
    }
    fn tag(&self, index: usize) -> Option<&LiveTag> {
        self.tags.get(index)?.as_ref()
    }
    fn trigger(&self, index: usize) -> Option<&LiveTrigger> {
        self.triggers.get(index)?.as_ref()
    }
    fn evaluate_event(
        &self,
        condition: &EventCondition,
        current_frame: u32,
        simulation: &Simulation,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> bool {
        match condition.kind {
            EVENT_ELAPSED_SCENARIO_TIME => {
                condition.value <= (current_frame as i32) / LOGICAL_FRAMES_PER_SECOND
            }
            EVENT_GLOBAL_IS_SET
            | EVENT_GLOBAL_IS_CLEAR
            | EVENT_LOCAL_IS_SET
            | EVENT_LOCAL_IS_CLEAR => {
                let global = matches!(condition.kind, EVENT_GLOBAL_IS_SET | EVENT_GLOBAL_IS_CLEAR);
                let limit = if global { 50 } else { 100 };
                if !(0..limit).contains(&condition.value) {
                    // Native Get689760/689A00 leaves an uninitialized output
                    // byte on invalid indices. Reject malformed conditions;
                    // stack residue is not a deterministic gameplay contract.
                    return false;
                }
                let values = if global {
                    &self.globals_set
                } else {
                    &self.locals_set
                };
                values.contains(&(condition.value as u32))
                    == matches!(condition.kind, EVENT_GLOBAL_IS_SET | EVENT_LOCAL_IS_SET)
            }
            EVENT_TECHTYPE_EXISTS | EVENT_TECHTYPE_DOES_NOT_EXIST => {
                // TEventClass::HasOccurred, original gamemd.exe:
                // 71E96B/71EA30 select the last global TechnoType with an
                // exact stored ID before comparing concrete type pointers.
                // Reuse Action106's registry owner, including its explicit
                // unrepresentable truncated-identity boundary.
                let Some(object) = condition
                    .type_name
                    .as_deref()
                    .and_then(|name| rules.native_techno_type_exact(name))
                else {
                    return false;
                };
                let count = count_techtype(simulation, object);
                if condition.kind == EVENT_TECHTYPE_EXISTS {
                    // 71EA19 checks the signed threshold inside the scan:
                    // zero/negative still requires a nonempty Techno vector.
                    !simulation.entities().is_empty() && count as i64 >= i64::from(condition.value)
                } else {
                    count == 0
                }
            }
            _ => false,
        }
    }
}

/// Caller-owned parameters to native Tag6E53A0 / Register7264C0. `actor`
/// supplies Entered's actual House; `source` is the live source Techno passed
/// by Object5F5390, not a copied damage-source House.
#[derive(Clone, Copy, Default)]
struct TagEventInput {
    kind: i32,
    actor: Option<u64>,
    source: Option<u64>,
    cell: (i16, i16),
    force: bool,
    /// A non-Techno object Ares passes in the Techno source slot.
    ares_source: Option<AresEventSource>,
}

/// What Ares 3.0 passes as RaiseEvent's `TechnoClass*` source when it is not
/// a Techno. TEventExt::HasOccured (1004F7A0) reads it only for the kind it
/// was raised with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum AresEventSource {
    /// A HouseClass: an abductee's previous owner (74) or a destroyer's
    /// House (85).
    House(crate::sim::intern::InternedId),
    /// The fired or deactivated SuperClass, by its type (75, 76).
    Super(crate::sim::intern::InternedId),
    /// SWTypeExt::Activate's SWLaunchEventData: the Super's type and its
    /// target Cell (77).
    Launch {
        sw_type: crate::sim::intern::InternedId,
        cell: (i16, i16),
    },
}

/// The two native TeamClass event23 broadcasts use opposite array walks.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TeamTagOrder {
    Forward,
    Reverse,
}

/// Spring7265C0 resolves its authored House per action, preserving the incoming
/// actor and packed Cell alongside the live Trigger. Direct TAction controls
/// may deliberately supply no live instance.
#[derive(Clone, Copy)]
struct TriggerActionContext {
    live_index: Option<usize>,
    incoming_house: Option<crate::sim::intern::InternedId>,
    input: TagEventInput,
}

/// Scenario global variables: SetGlobal689670 rejects indices from 0x32.
pub const SCENARIO_GLOBAL_COUNT: usize = 50;

impl TriggerRuntime {
    /// Retail CellClass::Constructor @ 0x0047BBF0, reached by
    /// MapClass::Resize @ 0x00565C10, directly clears Dummy+3C at 0x0047BC18.
    /// This is not ReplaceTag485250: retain Tag refs, timers and live state.
    /// Source: original gamemd.exe instructions.
    pub(super) fn reconstruct_dummy_for_map_resize(&mut self) {
        self.cell_tags.remove(&NativeCellIdentity::Dummy);
    }

    fn replace_cell_trigger_tag(&mut self, cell: NativeCellIdentity, tag: Option<usize>) {
        if let Some(previous) = self.cell_tags.remove(&cell)
            && let Some(old) = self.tags[previous].as_mut()
        {
            old.refs = old.refs.wrapping_sub(1);
            #[cfg(any(test, feature = "dev-overlays"))]
            self.trace_refs(previous);
        }
        if let Some(tag) = tag {
            self.cell_tags.insert(cell, tag);
            let live = self.tags[tag]
                .as_mut()
                .expect("a Tag handed to a cell is a live Tag");
            live.refs = live.refs.wrapping_add(1);
            #[cfg(any(test, feature = "dev-overlays"))]
            self.trace_refs(tag);
        }
    }
    fn replace_trigger_tag_attachment(
        &mut self,
        attachment: TagAttachment,
        tag: Option<usize>,
    ) -> bool {
        if let Some(previous) = self.attachments.remove(&attachment)
            && let Some(old) = self.tags[previous].as_mut()
            && matches!(attachment, TagAttachment::Object(_))
        {
            old.refs = old.refs.wrapping_sub(1);
            #[cfg(any(test, feature = "dev-overlays"))]
            self.trace_refs(previous);
        }
        if let Some(tag) = tag {
            self.attachments.insert(attachment, tag);
            let live = self.tags[tag]
                .as_mut()
                .expect("a Tag handed to an attachment is a live Tag");
            if matches!(attachment, TagAttachment::Object(_)) {
                live.refs = live.refs.wrapping_add(1);
                #[cfg(any(test, feature = "dev-overlays"))]
                self.trace_refs(tag);
            }
        }
        tag.is_some()
    }
    /// Physical Object storage disappears without a SetTag5F5B50 call.
    /// Steam gamemd Object5F3B80, including actual scalar/drain controls in
    /// tag_lifecycle --damage-events, leaves Tag+2C and the Tag itself intact.
    /// Remove only the Rust index whose source object no longer exists; no
    /// counter write, pending ticket, RNG draw or timer change belongs here.
    pub(crate) fn finalize_object_trigger_tag_attachment(&mut self, object: u64) {
        self.attachments.remove(&TagAttachment::Object(object));
        // A module trigger binding goes with its object (RFC 0001 section 7).
        self.module.forget_object(object);
    }
    pub(crate) fn attach_team_member_trigger_tag(&mut self, team: u64, object: u64) {
        // AddMember6EA582..6EA587 skips attachment when Team+70 is NULL,
        // retaining an authored Object Tag. The B3/Passengers admission gate
        // belongs to the shared Team caller; Object5F5B50 owns references.
        // Executed controls: tools/spatial_oracle/team_creation.member_tag.
        let Some(tag) = self.attachments.get(&TagAttachment::Team(team)).copied() else {
            return;
        };
        self.replace_trigger_tag_attachment(TagAttachment::Object(object), Some(tag));
    }
    /// Team RemoveMember6EA892..6EA8B5 may clear only the Team's own Tag.
    /// Its caller retains the native House admission gate; authored object
    /// attachments belonging to another Tag remain intact.
    pub(crate) fn clear_team_member_trigger_tag(&mut self, team: u64, object: u64) {
        if let Some(&tag) = self.attachments.get(&TagAttachment::Team(team))
            && self.attachments.get(&TagAttachment::Object(object)) == Some(&tag)
        {
            self.replace_trigger_tag_attachment(TagAttachment::Object(object), None);
        }
    }
    fn detach_live_tag(&mut self, index: usize) {
        //7258D0 -> every Object/Cell, Map577920 and Logic55B880. This does
        // not set Tag.pending; ProcessTriggerEvent queues its removal separately.
        self.logic_tags.retain(|&tag| tag != index);
        self.map_tags.retain(|&tag| tag != index);
        for tags in self.house_tags.values_mut() {
            tags.retain(|&tag| tag != index);
        }
        let attachments: Vec<_> = self
            .attachments
            .iter()
            .filter_map(|(&id, &tag)| (tag == index).then_some(id))
            .collect();
        for attachment in attachments {
            self.replace_trigger_tag_attachment(attachment, None);
        }
        let cells: Vec<_> = self
            .cell_tags
            .iter()
            .filter_map(|(&cell, &tag)| (tag == index).then_some(cell))
            .collect();
        for cell in cells {
            self.replace_cell_trigger_tag(cell, None);
        }
    }
    fn adjust_tag_destructor_cursors(&mut self, flags: u8, game_state: i32, started: bool) {
        if (game_state as u8) == 0 || !started {
            return;
        }
        for (bit, cursor, count) in [
            (16, &mut self.logic_cursor, self.logic_tags.len()),
            (4, &mut self.map_cursor, self.map_tags.len()),
        ] {
            if flags & bit != 0 && *cursor >= -1 {
                *cursor = cursor.wrapping_sub(1);
                if *cursor < 0 && count == 0 {
                    *cursor = 0;
                }
            }
        }
    }
    /// Pickup481AC1 writes Scenario+34BE only after its optional Tag callback
    /// survives. Logic55AFB0 consumes and clears this existing serialized flag.
    /// Steam gamemd; tools/spatial_oracle/trigger_crate.json.
    pub(crate) fn mark_crate_pickup_event(&mut self) {
        self.dirty[3] = true;
    }
    /// LogicClass::AI's ambient fade writes Scenario+34AB after each step
    /// (`0x0055B4BA`), so the next Logic55AFB0 poll tries events 45/46
    /// before event 13.
    pub(crate) fn mark_ambient_light_changed(&mut self) {
        self.dirty[2] = true;
    }
    /// TagClass::IsTransferable6E57C0 reads each linked LIVE Trigger's
    /// Type+A0, without enabled, pending or difficulty tests (retail gamemd).
    pub(crate) fn object_tag_is_transferable(&self, object: u64) -> bool {
        self.attachments
            .get(&TagAttachment::Object(object))
            .and_then(|&tag| self.tag(tag))
            .is_some_and(|tag| {
                tag.triggers.iter().any(|&live| {
                    self.trigger(live).is_some_and(|trigger| {
                        self.definitions.trigger_types[trigger.type_index].transfer_on_capture
                    })
                })
            })
    }

    /// ObjectClass::SetTag5F5B50 after IsTransferable6E57C0: engineer
    /// capture519F66, thief capture520439 and vehicle crew738374 all copy
    /// the attachment. The source keeps its reference; the old zero-ref Tag
    /// is not deleted here. Source: retail gamemd.exe instructions.
    pub(crate) fn transfer_object_tag(&mut self, source: u64, target: u64) {
        if self.object_tag_is_transferable(source) {
            let attachment = TagAttachment::Object(source);
            let tag = self.attachments.get(&attachment).copied();
            self.replace_trigger_tag_attachment(TagAttachment::Object(target), tag);
        }
    }
}

fn validate_live_registry<T>(
    family: &str,
    entries: &[Option<T>],
    order: &[usize],
) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for &index in order {
        if !seen.insert(index) || entries.get(index).is_none_or(Option::is_none) {
            return Err(format!("{family} registry has an invalid identity {index}"));
        }
    }
    if seen.len() != entries.iter().flatten().count() {
        return Err(format!("{family} storage and active registry disagree"));
    }
    Ok(())
}

/// Resolve a trigger's canonical HouseType owner to the first registered House.
///
/// gamemd-derived: `TriggerTypeClass::Read` canonicalizes the owner through
/// `HouseTypeClass__FindIndexOfName @ 0x005117D0` (alias before ID in source
/// order; `<random>` returns -2). Before that lookup, ReadINI727292..7272AA
/// handles its literal `<none>` by selecting Country zero. Scenario475540
/// does not have this TriggerType caller rule.
/// Only registered Country results reach this owner. `TriggerClass::Spring @
/// 0x007265C0` passes that type index to `HouseClass__Find_By_Country_Index @
/// 0x00502D30`, whose global House-array scan returns the first matching
/// registration.
fn resolve_trigger_house(
    sim: &Simulation,
    trigger_owner: Option<&str>,
    rules: &crate::rules::ruleset::RuleSet,
) -> Option<crate::sim::intern::InternedId> {
    let trigger_house_type = rules.trigger_house_type_index(trigger_owner?)?;
    sim.house_by_country_index(trigger_house_type, rules)
}

fn parse_visible_map_area(fields: &[String]) -> Option<[i32; 4]> {
    let atoi = |index: usize| {
        fields
            .get(index)
            .map(|field| crate::rules::ini_value::crt_atoi(field))
    };
    Some([atoi(2)?, atoi(3)?, atoi(4)?, atoi(5)?])
}

fn count_techtype(sim: &Simulation, object: &crate::rules::object_type::ObjectType) -> usize {
    // HasOccurred71EA03/71EAC0 walks all stored Technos, including Limbo
    // and Health0, comparing virtual84's concrete type pointer. Category
    // plus interned ID preserves that identity for representable types.
    let Some(type_id) = sim.interner.get(&object.id) else {
        return 0;
    };
    let category = crate::map::entities::EntityCategory::from(object.category);
    sim.substrate
        .entities
        .values()
        .filter(|e| e.category() == category && e.type_ref() == type_id)
        .count()
}

// Observer read surface over the definitions and live instances above. The
// views are read-only: every mutation keeps its existing owner, and no view
// reconstructs a reference from a raw authored token — resolved identities
// come straight from this runtime's own definition registries.

/// One trigger event's observer view: the condition kind and its
/// materialized value. The predicate's typed name/team fields stay with the
/// evaluation owner; a wave-timing observer needs kind and value only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerEventView {
    pub kind: i32,
    pub value: i32,
}

/// One trigger action's observer view. `team_type` is the TeamType id the
/// action's definition reference resolved to (the reinforcement and
/// create-team actions 4/7/80/107 carry one); `waypoint` is the action's
/// materialized `TAction+44` destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerActionView {
    pub kind: i32,
    pub team_type: Option<String>,
    pub value: Option<i32>,
    pub waypoint: Option<u32>,
    /// The trigger an action 53 (enable trigger) names, when the loaded
    /// entry resolved one — the indirect outcome hop a defense persona's
    /// protected-target walk follows (a solo defense map's statue carries
    /// destroyed -> enable the trigger whose action announces the loss).
    pub enable_trigger: Option<String>,
}

/// One trigger's definition chain: the head trigger's events and actions,
/// followed by every linked trigger's, walked exactly like the definition
/// getters (`TagType6E61F0` -> `TriggerType7271E0`, the `has_event`/
/// `has_action` link traversal) with a cycle guard.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TriggerDefinitionView {
    pub events: Vec<TriggerEventView>,
    pub actions: Vec<TriggerActionView>,
}

/// One armed (registered, live) trigger instance's observer view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmedTriggerView {
    /// The chain head's TriggerType id (exact case).
    pub trigger_id: String,
    pub enabled: bool,
    /// The per-event completion bits the poll consumes.
    pub completed_event_bits: u32,
    /// The live timer's remaining frames when the chain head's own events
    /// carry an elapsed-time event (13/51): the timer owner starts it at the
    /// instance's construction (event 13: `value * 15` logic frames; 51:
    /// `(value / 2 + RandomRanged(0, value)) * 15`) and re-arms it on every
    /// repeat fire. `None` when the head carries none — the read
    /// never invents a deadline. Event 47 (elapsed scenario time) has no
    /// timer; an observer computes its deadline from the event view's value.
    pub timer_remaining_frames: Option<i32>,
    /// The head chain's events and actions.
    pub definition: TriggerDefinitionView,
}

impl TriggerRuntime {
    /// Walk `head`'s definition chain (linked triggers included) into an
    /// observer view.
    fn definition_view(&self, head: Option<usize>) -> TriggerDefinitionView {
        let mut view = TriggerDefinitionView::default();
        let mut next = head;
        let mut visited = BTreeSet::new();
        while let Some(index) = next {
            if !visited.insert(index) {
                break;
            }
            let typ = &self.definitions.trigger_types[index];
            view.events
                .extend(typ.events.iter().map(|event| TriggerEventView {
                    kind: event.condition.kind,
                    value: event.condition.value,
                }));
            view.actions
                .extend(typ.actions.iter().map(|action| TriggerActionView {
                    kind: action.entry.kind,
                    team_type: match action.reference {
                        Some(DefinitionReference::Team(team)) => {
                            Some(self.definitions.team_types[team].clone())
                        }
                        _ => None,
                    },
                    value: action.value,
                    waypoint: action.entry.waypoint_index,
                    enable_trigger: match action.reference {
                        Some(DefinitionReference::Trigger(trigger)) => {
                            Some(self.definitions.trigger_types[trigger].id.clone())
                        }
                        _ => None,
                    },
                }));
            next = typ.linked;
        }
        view
    }

    /// Every registered live trigger instance, in registry order, read at
    /// `current_frame`.
    pub fn armed_trigger_views(&self, current_frame: i32) -> Vec<ArmedTriggerView> {
        self.check_attached();
        self.trigger_order
            .iter()
            .filter_map(|&index| {
                let live = self.trigger(index)?;
                let typ = &self.definitions.trigger_types[live.type_index];
                let has_timer_event = typ
                    .events
                    .iter()
                    .any(|event| matches!(event.condition.kind, 13 | 51));
                Some(ArmedTriggerView {
                    trigger_id: typ.id.clone(),
                    enabled: live.enabled,
                    completed_event_bits: live.completed,
                    timer_remaining_frames: has_timer_event
                        .then(|| live.timer.remaining(current_frame)),
                    definition: self.definition_view(Some(live.type_index)),
                })
            })
            .collect()
    }

    /// One trigger definition's observer view, found by its authored id —
    /// registered or not: a lose trigger may carry no tag anywhere, so no
    /// live instance ever exists for it, while an enable-trigger hop (the
    /// defense persona's protected-target walk) still reads its outcome.
    pub fn trigger_definition_view(&self, trigger_id: &str) -> Option<TriggerDefinitionView> {
        self.check_attached();
        let index = self
            .definitions
            .trigger_types
            .iter()
            .position(|typ| typ.id == trigger_id)?;
        Some(self.definition_view(Some(index)))
    }

    /// The definition chains attached to `object` (the retained Object
    /// attachment funnel's registrations), each walked through its linked
    /// triggers. An object with no attached Tag yields an empty vec.
    pub fn object_tag_definition_views(&self, object: u64) -> Vec<TriggerDefinitionView> {
        self.check_attached();
        match self.attachments.get(&TagAttachment::Object(object)) {
            Some(&tag) => self
                .tag(tag)
                .map(|live| {
                    vec![self.definition_view(self.definitions.tag_types[live.type_index].trigger)]
                })
                .unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// The tag attached to a cell, as its tag-storage index.
    pub(crate) fn cell_tag(&self, cell: NativeCellIdentity) -> Option<usize> {
        self.cell_tags.get(&cell).copied()
    }

    /// The tag attached to an object, as its tag-storage index.
    pub(crate) fn object_tag(&self, object: u64) -> Option<usize> {
        self.attachments
            .get(&TagAttachment::Object(object))
            .copied()
    }

    /// A global trigger variable's current value. Out-of-range indices read
    /// clear, never panic.
    pub fn global_variable_is_set(&self, index: u32) -> bool {
        self.check_attached();
        self.globals_set.contains(&index)
    }

    /// A local trigger variable's current value. Out-of-range indices read
    /// clear, never panic.
    pub fn local_variable_is_set(&self, index: u32) -> bool {
        self.check_attached();
        self.locals_set.contains(&index)
    }
}

#[path = "trigger_runtime_ares.rs"]
mod ares;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_ares_tests.rs"]
mod ares_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_tests.rs"]
mod tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_factory_owner_tests.rs"]
mod factory_owner_tests;

// Vanilla duplicate keys: Ares's `INIClass_Parse_Override` (`0x5260D9`) keeps
// only the later one (rules::ini_parser::ares_syntax).
#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_duplicate_ini_tests.rs"]
mod duplicate_ini_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_zone_entry_tests.rs"]
mod zone_entry_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_native_tests.rs"]
mod native_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_superweapon_state_tests.rs"]
mod superweapon_state_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_superweapon_grant_tests.rs"]
mod superweapon_grant_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_defense_target_tests.rs"]
mod defense_target_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_type_tech_level_tests.rs"]
mod type_tech_level_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_all_to_hunt_tests.rs"]
mod all_to_hunt_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_no_factories_tests.rs"]
mod no_factories_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_damage_event_tests.rs"]
mod damage_event_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_reveal_zone_tests.rs"]
mod reveal_zone_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_ai_activation_tests.rs"]
mod ai_activation_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_crate_tests.rs"]
mod crate_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_build_type_tests.rs"]
mod build_type_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_house_notification_tests.rs"]
mod house_notification_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_house_attacked_tests.rs"]
mod house_attacked_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_group_wakeup_tests.rs"]
mod group_wakeup_tests;
#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_wakeup_tests.rs"]
mod wakeup_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_chrono_reinforcement_tests.rs"]
mod chrono_reinforcement_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_lightning_tests.rs"]
mod lightning_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_building_power_tests.rs"]
mod building_power_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_house_directive_tests.rs"]
mod house_directive_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_home_camera_tests.rs"]
mod home_camera_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_explosion_tests.rs"]
mod explosion_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_movement_event_tests.rs"]
mod movement_event_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_shroud_spy_tests.rs"]
mod shroud_spy_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_periodic_cell_tests.rs"]
mod periodic_cell_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_infantry_psycho_tests.rs"]
mod infantry_psycho_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_ore_tests.rs"]
mod ore_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_particle_tests.rs"]
mod particle_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_map_lighting_tests.rs"]
mod map_lighting_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_tint_tests.rs"]
mod tint_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_ambient_event_tests.rs"]
mod ambient_event_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_nuke_tests.rs"]
mod nuke_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_direct_strike_tests.rs"]
mod direct_strike_tests;

#[cfg(all(test, tests_sim_trigger))]
mod meteor_action_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_talk_tests.rs"]
mod talk_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_cargo_tests.rs"]
mod cargo_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_restore_tests.rs"]
mod restore_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_scenario_pause_tests.rs"]
mod scenario_pause_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_outcome_tests.rs"]
mod outcome_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_digest_ini_tests.rs"]
mod digest_ini_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "world/building_sale_trigger_tests.rs"]
mod building_sale_trigger_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_building_sale_tests.rs"]
mod building_sale_native_tests;

#[cfg(all(test, tests_sim_trigger))]
#[path = "trigger_runtime_skirmish_map_tests.rs"]
mod skirmish_map_tests;
