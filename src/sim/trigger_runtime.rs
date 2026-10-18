//! Native map-authored live Tag/Trigger ownership.
//!
//! Type registration follows Full_Init686B20's TeamType first references,
//! physical TriggerType reads and physical TagType reads. Runtime construction,
//! timer writes, Logic55AFB0 polling and deferred deletion all mutate this one
//! Simulation-owned, serialized authority. Reproducible bounded native histories:
//! tools/spatial_oracle/tag_lifecycle.{py,json,meta.json}.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

use crate::map::actions::{ActionDefinitionReference, ActionEntry, DefinitionSelector};
use crate::map::cell_index::NativeCellIdentity;
use crate::map::events::EventCondition;
#[cfg(any(test, feature = "test-support"))]
use crate::map::events::EventMap;
use crate::map::resolved_terrain::NativeCellQuery;
#[cfg(any(test, feature = "test-support"))]
use crate::map::triggers::TriggerMap;
#[cfg(any(test, feature = "test-support"))]
use crate::map::variable_names::LocalVariableMap;
use crate::rules::ini_parser::is_native_none_type_name;
use crate::rules::ini_value::{crt_atoi, truncate_native_bytes};
use crate::sim::timer::CdTimer;
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation, TriggerInputs};

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
const ACTION_DAMAGE_AT_WAYPOINT: i32 = 63;
const ACTION_ANNOUNCE_WIN: i32 = 67;
const ACTION_ANNOUNCE_LOSE: i32 = 68;
const ACTION_END_SCENARIO: i32 = 69;
const ACTION_PLAY_SOUND_AT_WAYPOINT: i32 = 99;
const ACTION_JUMP_CAMERA: i32 = 112;
const ACTION_SET_SUPER_CHARGE_PERCENTAGE: i32 = 129;
const ACTION_SET_ALTERNATE_BASE: i32 = 137;
const ACTION_CLEAR_ALTERNATE_BASE: i32 = 138;

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

/// Action20's music operand. Theme owns the immutable process catalog, so a
/// type8 name is kept verbatim until that owner resolves it. This preserves
/// Read6DD7E0's From_Name721210 result without a second music registry in sim.
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
    LocalizedMessage {
        csf_label: String,
    },
    /// Execute114 asks the retained client sidebar to select a nonempty strip.
    SelectSidebarTab {
        index: i32,
    },
    /// Execute115's named TechnoType cameo pulse, on the native frame clock.
    FlashSidebarCameo {
        type_id: crate::sim::intern::InternedId,
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
    /// Execute20 queues the song; the shared Theme owner controls admission,
    /// fade, subsequent playback and the process Main RNG.
    MusicTheme {
        request: ThemeRequest,
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct RuntimeAction {
    entry: ActionEntry,
    reference: Option<DefinitionReference>,
    /// Original Read6DD5B0 materializes +90 once, including Vox753250.
    value: Option<i32>,
    /// Parameter type 7's +90: `VocClass::FindIndex @ 0x007514D0` of the raw
    /// token (`0x006DD7CE..0x006DD7D5`). The canonical SOUNDMD id stands for
    /// the resolved index so no Voc ordinal enters simulation; `None` is the
    /// -1 a missing or unknown name materializes.
    sound: Option<String>,
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
impl std::ops::Deref for RuntimeEvent {
    type Target = EventCondition;
    fn deref(&self) -> &Self::Target {
        &self.condition
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
struct RuntimeDefinitions {
    team_types: Vec<String>,
    /// TeamType+D0 references resolved during physical Read6F1341. This is
    /// the sole owner; Team constructors consume the retained ordinal and
    /// never re-run a factory or infer the pointer from a truncated Type ID.
    team_tags: Vec<Option<usize>>,
    trigger_types: Vec<TriggerType>,
    tag_types: Vec<TagType>,
}

impl RuntimeDefinitions {
    fn team_type(&mut self, name: &str) -> Option<usize> {
        if is_native_none_type_name(name) {
            return None;
        }
        if let Some(i) = self
            .team_types
            .iter()
            .position(|id| id.eq_ignore_ascii_case(name))
        {
            return Some(i);
        }
        let index = self.team_types.len();
        self.team_types
            .push(truncate_native_bytes(name, 24).to_string());
        self.team_tags.push(None);
        Some(index)
    }
    fn trigger_type(&mut self, name: &str) -> Option<usize> {
        if is_native_none_type_name(name) {
            return None;
        }
        if let Some(i) = self
            .trigger_types
            .iter()
            .position(|typ| typ.id.eq_ignore_ascii_case(name))
        {
            return Some(i);
        }
        let index = self.trigger_types.len();
        self.trigger_types.push(TriggerType::new(
            truncate_native_bytes(name, 24).to_string(),
        ));
        Some(index)
    }
    fn tag_type(&mut self, name: &str) -> Option<usize> {
        if is_native_none_type_name(name) {
            return None;
        }
        if let Some(i) = self
            .tag_types
            .iter()
            .position(|typ| typ.id.eq_ignore_ascii_case(name))
        {
            return Some(i);
        }
        let index = self.tag_types.len();
        let id = truncate_native_bytes(name, 24).to_string();
        self.tag_types.push(TagType {
            name: id.clone(),
            id,
            repeat: 0,
            trigger: None,
        });
        Some(index)
    }
    fn resolve_action(
        &mut self,
        entry: ActionEntry,
        eva: Option<&crate::rules::sound_ini::EvaRegistry>,
        sounds: Option<&crate::rules::sound_ini::SoundRegistry>,
    ) -> RuntimeAction {
        let reference = match entry.definition_reference() {
            Some(ActionDefinitionReference::Team(selector)) => match selector {
                DefinitionSelector::Index(i) => usize::try_from(i)
                    .ok()
                    .filter(|i| *i < self.team_types.len()),
                DefinitionSelector::Name(name) => self.team_type(name),
            }
            .map(DefinitionReference::Team),
            Some(ActionDefinitionReference::Trigger(selector)) => match selector {
                DefinitionSelector::Index(i) => usize::try_from(i)
                    .ok()
                    .filter(|i| *i < self.trigger_types.len()),
                DefinitionSelector::Name(name) => self.trigger_type(name),
            }
            .map(DefinitionReference::Trigger),
            Some(ActionDefinitionReference::Tag(selector)) => match selector {
                DefinitionSelector::Index(i) => usize::try_from(i)
                    .ok()
                    .filter(|i| *i < self.tag_types.len()),
                DefinitionSelector::Name(name) => self.tag_type(name),
            }
            .map(DefinitionReference::Tag),
            None => None,
        };
        let parameter_type = entry.params.first().map_or(0, |v| crt_atoi(v));
        let value = if parameter_type == 6 {
            // Original753250 scans the raw, untrimmed name. Missing names
            // materialize -1; no cue-name resolution runs during Execute.
            Some(
                entry
                    .params
                    .get(1)
                    .and_then(|name| eva.and_then(|catalog| catalog.index_of(name)))
                    .unwrap_or(-1),
            )
        } else {
            entry.literal_value()
        };
        // FindIndex7514D0 compares the untrimmed token case-insensitively.
        let sound = if parameter_type == 7 {
            entry.params.get(1).and_then(|name| sounds?.get(name))
        } else if matches!(entry.kind, 19 | 98 | 99) {
            //Execute's Voc call consumes +90 regardless of the authored
            //parameter type. Resolve a literal ordinal once in this reader,
            //through the same fixed catalog as a type7 name.
            value.and_then(|index| sounds?.entry_by_index(index))
        } else {
            None
        }
        .map(|sound| sound.id.clone());
        RuntimeAction {
            entry,
            reference,
            value,
            sound,
        }
    }
    fn read_map(
        map: &crate::map::map_file::MapFile,
        teams: &crate::rules::team_ai_ini::TeamAiIniRegistry,
        eva: Option<&crate::rules::sound_ini::EvaRegistry>,
        sounds: Option<&crate::rules::sound_ini::SoundRegistry>,
    ) -> Self {
        let mut definitions = Self::default();
        // Full_Init687984/68798D: reads are transactions, not the final merged
        // TeamType vector. Team.Tag can first-construct TagTypes here.
        for team in teams.team_type_read_sequence() {
            let Some(index) = definitions.team_type(&team.id) else {
                continue;
            };
            // Read6F131E retains +D0 on an absent/empty ReadString; a
            // nonempty value calls6E6310, including either null literal.
            // Only this physical pass can construct a new TagType identity.
            let tag = team.physical_read_section().read_string("Tag", "", 128);
            if !tag.is_empty() {
                let resolved = definitions.tag_type(&tag);
                definitions.team_tags[index] = resolved;
            }
        }
        if let Some(section) = map.ini.section("Triggers") {
            for key in section.keys() {
                if section.read_name(key, 24).is_none() {
                    continue;
                }
                let Some(index) = definitions.trigger_type(key) else {
                    continue;
                };
                let id = definitions.trigger_types[index].id.to_ascii_uppercase();
                let Some(record) = map.triggers.get(&id) else {
                    continue;
                };
                //7272D6 resolves the linked Type before events and actions.
                let linked = record
                    .linked_trigger_id
                    .as_deref()
                    .and_then(|name| definitions.trigger_type(name));
                let mut events = map
                    .events
                    .get(&id)
                    .map_or_else(Vec::new, |record| record.conditions.clone());
                // Events are prepended, but their reference factories ran in
                // source order before the retained reverse traversal.
                for event in events.iter_mut().rev() {
                    if let Some(team) = event.team_name.as_deref() {
                        event.team_name = definitions
                            .team_type(team)
                            .map(|index| definitions.team_types[index].clone());
                    }
                }
                let actions = map
                    .actions
                    .get(&id)
                    .map_or_else(Vec::new, |record| record.entries.clone());
                let actions = actions
                    .into_iter()
                    .map(|entry| definitions.resolve_action(entry, eva, sounds))
                    .collect();
                definitions.trigger_types[index] = TriggerType {
                    id: definitions.trigger_types[index].id.clone(),
                    owner: record.owner.clone(),
                    linked,
                    enabled: record.enabled,
                    difficulty: [
                        record.difficulty.easy,
                        record.difficulty.medium,
                        record.difficulty.hard,
                    ],
                    events: events.into_iter().map(RuntimeEvent::from).collect(),
                    transfer_on_capture: definitions.trigger_types[index].transfer_on_capture
                        || record
                            .fields
                            .get(7)
                            .is_some_and(|value| crt_atoi(value) != 0),
                    actions,
                };
            }
        }
        if let Some(section) = map.ini.section("Tags") {
            for key in section.keys() {
                if section.read_name(key, 24).is_none() {
                    continue;
                }
                let Some(index) = definitions.tag_type(key) else {
                    continue;
                };
                let id = definitions.tag_types[index].id.to_ascii_uppercase();
                let Some(record) = map.tags.get(&id) else {
                    continue;
                };
                let repeat = record.fields.first().map_or(0, |value| crt_atoi(value));
                let trigger = record
                    .fields
                    .get(2)
                    .and_then(|name| definitions.trigger_type(name));
                definitions.tag_types[index].repeat = repeat;
                definitions.tag_types[index].name =
                    record.fields.get(1).map_or_else(String::new, |name| {
                        truncate_native_bytes(name, 48).to_string()
                    });
                definitions.tag_types[index].trigger = trigger;
            }
        }
        definitions
    }
    fn classification(&self, tag_type: usize) -> u8 {
        // TagType6E61F0 -> TriggerType7271E0; enabled and difficulty bytes do
        // not gate registration. Invalid cyclic definitions are rejected by
        // the bounded Rust traversal rather than reproducing native hangs.
        let mut next = self.tag_types[tag_type].trigger;
        let mut visited = BTreeSet::new();
        let mut flags = 0;
        while let Some(index) = next {
            if !visited.insert(index) {
                break;
            }
            let typ = &self.trigger_types[index];
            for event in &typ.events {
                flags |= event_classification(event.kind);
            }
            for action in &typ.actions {
                flags |= action_classification(action.entry.kind);
            }
            next = typ.linked;
        }
        flags
    }

    fn has_action(&self, tag_type: usize, kind: i32) -> bool {
        // AllowWin TagType6E6220 -> TriggerType726FE0 follows definition links.
        let mut next = self.tag_types[tag_type].trigger;
        let mut visited = BTreeSet::new();
        while let Some(index) = next {
            if !visited.insert(index) {
                break;
            }
            let typ = &self.trigger_types[index];
            if typ.actions.iter().any(|a| a.entry.kind == kind) {
                return true;
            }
            next = typ.linked;
        }
        false
    }

    fn has_event(&self, tag_type: usize, kind: i32) -> bool {
        // Line/zone getters6E6250/6280/62B0 walk linked TriggerTypes and
        // scan their Event nodes at726F80/6F50/6FB0. Enabled/difficulty and
        // live-instance state do not gate this definition query.
        let mut next = self.tag_types[tag_type].trigger;
        let mut visited = BTreeSet::new();
        while let Some(index) = next {
            if !visited.insert(index) {
                break;
            }
            let typ = &self.trigger_types[index];
            if typ.events.iter().any(|event| event.kind == kind) {
                return true;
            }
            next = typ.linked;
        }
        false
    }
}

// Original jump-table bytes71F700/71F744/71F780/71F7C0 and6E3F08.
// These are classification only, never substitutes for event predicates.
fn event_classification(kind: i32) -> u8 {
    let mut flags = 0;
    if matches!(kind, 0 | 1 | 4 | 8 | 24 | 25 | 26 | 31 | 53 | 54 | 59) {
        flags |= 1;
    }
    if matches!(
        kind,
        0 | 1 | 2 | 4 | 6 | 7 | 8 | 29 | 33 | 34 | 35 | 38 | 39 | 40 | 41 | 42 | 43 | 44 | 48 | 49
    ) {
        flags |= 2;
    }
    if matches!(kind, 8 | 24) {
        flags |= 4;
    }
    if matches!(
        kind,
        3 | 5
            | 8
            | 9
            | 10
            | 11
            | 12
            | 15
            | 16
            | 17
            | 18
            | 19
            | 20
            | 21
            | 22
            | 30
            | 32
            | 52
            | 55
            | 56
            | 57
            | 58
    ) {
        flags |= 8;
    }
    if matches!(
        kind,
        8 | 13 | 14 | 23 | 27 | 28 | 36 | 37 | 45 | 46 | 47 | 50 | 51 | 60 | 61
    ) {
        flags |= 16;
    }
    flags
}
fn action_classification(kind: i32) -> u8 {
    if matches!(kind, 14 | 32 | 60 | 61 | 62 | 91 | 111) {
        2
    } else {
        0
    }
}

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
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
    #[cfg(any(test, feature = "test-support"))]
    fixture_awaiting_records: bool,
    #[cfg(any(test, feature = "test-support"))]
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
    }
}
impl Eq for TriggerRuntime {}

impl TriggerRuntime {
    pub(crate) fn has_live_tag(&self, id: usize) -> bool {
        self.tag(id).is_some()
    }
    pub(crate) fn has_live_trigger(&self, id: usize) -> bool {
        self.trigger(id).is_some()
    }
    pub(crate) fn hash_state(&self, hasher: &mut impl Hasher) {
        // Definition ordinals, timers, active registration order, attachments
        // and the deferred queue affect later actions and are all retained.
        // Observers never participate in this fold.
        self.globals_set.hash(hasher);
        self.locals_set.hash(hasher);
        self.definitions.hash(hasher);
        self.tags.hash(hasher);
        self.triggers.hash(hasher);
        self.tag_order.hash(hasher);
        self.trigger_order.hash(hasher);
        self.logic_tags.hash(hasher);
        self.map_tags.hash(hasher);
        self.house_tags.hash(hasher);
        self.attachments.hash(hasher);
        self.cell_tags.hash(hasher);
        self.dirty.hash(hasher);
        self.logic_cursor.hash(hasher);
        self.map_cursor.hash(hasher);
        self.definitions_prepared.hash(hasher);
        self.classified.hash(hasher);
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
            EVENT_TECHTYPE_EXISTS => {
                let sim = simulation;
                let min_count = condition.value;
                let Some(type_id) = condition.type_name.as_deref() else {
                    return false;
                };
                if type_id.is_empty() {
                    return false;
                }
                // Native71EA03 tests the signed threshold inside its scan:
                // even a nonpositive threshold requires at least one Techno.
                !sim.entities().is_empty()
                    && count_techtype(sim, type_id) as i64 >= i64::from(min_count)
            }
            EVENT_TECHTYPE_DOES_NOT_EXIST => {
                let sim = simulation;
                let Some(type_id) = condition.type_name.as_deref() else {
                    return false;
                };
                if type_id.is_empty() {
                    return false;
                }
                count_techtype(sim, type_id) == 0
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

impl Simulation {
    /// Validate this owner's retained identities before the restore coordinator
    /// publishes a candidate. No map re-read or live constructor runs on load.
    /// Destruction may leave orphan Triggers until their shared725C70 tickets
    /// are consumed, so that native intermediate state remains admissible.
    pub(crate) fn validate_restored_trigger_runtime(&self) -> Result<(), String> {
        let runtime = &self.trigger_runtime;
        let definitions = &runtime.definitions;
        if definitions.team_tags.len() != definitions.team_types.len()
            || definitions
                .team_tags
                .iter()
                .flatten()
                .any(|&tag| tag >= definitions.tag_types.len())
        {
            return Err("TeamType has an unresolved retained TagType reference".to_string());
        }
        if runtime.globals_set.iter().any(|&index| index >= 50)
            || runtime.locals_set.iter().any(|&index| index >= 100)
        {
            return Err("Scenario variable index is outside its native owner".to_string());
        }
        for (index, typ) in definitions.trigger_types.iter().enumerate() {
            if typ
                .linked
                .is_some_and(|linked| linked >= definitions.trigger_types.len())
            {
                return Err(format!("TriggerType {index} has an unresolved linked type"));
            }
            if typ.events.iter().any(|event| {
                event
                    .retained_house
                    .is_some_and(|house| !self.houses.contains_key(&house))
            }) {
                return Err(format!(
                    "TriggerType {index} has an unresolved retained event House"
                ));
            }
            for action in &typ.actions {
                let resolves = match action.reference {
                    Some(DefinitionReference::Team(index)) => index < definitions.team_types.len(),
                    Some(DefinitionReference::Trigger(index)) => {
                        index < definitions.trigger_types.len()
                    }
                    Some(DefinitionReference::Tag(index)) => index < definitions.tag_types.len(),
                    None => true,
                };
                if !resolves {
                    return Err(format!(
                        "TriggerType {index} has an unresolved action reference"
                    ));
                }
            }
        }
        for (index, typ) in definitions.tag_types.iter().enumerate() {
            if typ
                .trigger
                .is_some_and(|trigger| trigger >= definitions.trigger_types.len())
            {
                return Err(format!("TagType {index} has an unresolved TriggerType"));
            }
        }
        validate_live_registry("Tag", &runtime.tags, &runtime.tag_order)?;
        validate_live_registry("Trigger", &runtime.triggers, &runtime.trigger_order)?;
        let mut trigger_parents = BTreeSet::new();
        for (index, tag) in runtime
            .tags
            .iter()
            .enumerate()
            .filter_map(|(index, tag)| tag.as_ref().map(|tag| (index, tag)))
        {
            let Some(typ) = definitions.tag_types.get(tag.type_index) else {
                return Err(format!("Tag {index} has an unresolved TagType"));
            };
            let mut chain = Vec::new();
            let mut visited = BTreeSet::new();
            let mut next = typ.trigger;
            while let Some(type_index) = next {
                if !visited.insert(type_index) {
                    break;
                }
                chain.push(type_index);
                next = definitions.trigger_types[type_index].linked;
            }
            chain.reverse();
            let mut remaining = chain.as_slice();
            for &trigger in &tag.triggers {
                let Some(live) = runtime.trigger(trigger) else {
                    return Err(format!(
                        "Tag {index} has an unresolved live Trigger {trigger}"
                    ));
                };
                if !trigger_parents.insert(trigger) {
                    return Err(format!("Trigger {trigger} is linked by multiple live Tags"));
                }
                let Some(position) = remaining.iter().position(|&typ| typ == live.type_index)
                else {
                    return Err(format!(
                        "Tag {index} has a Trigger outside its retained linked-type order"
                    ));
                };
                remaining = &remaining[position + 1..];
            }
        }
        let queued_triggers: BTreeSet<_> = self
            .substrate
            .pending_delete
            .retained_tickets()
            .filter_map(|ticket| match ticket {
                crate::sim::world::DeferredDeleteTicket::Trigger(index) => Some(index),
                _ => None,
            })
            .collect();
        for (index, trigger) in runtime
            .triggers
            .iter()
            .enumerate()
            .filter_map(|(index, trigger)| trigger.as_ref().map(|trigger| (index, trigger)))
        {
            if trigger
                .retained_house
                .is_some_and(|house| !self.houses.contains_key(&house))
            {
                return Err(format!("Trigger {index} has an unresolved retained House"));
            }
            if trigger.type_index >= definitions.trigger_types.len() {
                return Err(format!("Trigger {index} has an unresolved TriggerType"));
            }
            if !trigger_parents.contains(&index) && !queued_triggers.contains(&index) {
                return Err(format!(
                    "Trigger {index} has neither a live Tag nor a deferred destructor"
                ));
            }
        }
        for (name, tags, flag) in [
            ("Logic", &runtime.logic_tags, 16),
            ("Map", &runtime.map_tags, 4),
        ] {
            let mut seen = BTreeSet::new();
            for &index in tags {
                let Some(tag) = runtime.tag(index) else {
                    return Err(format!("{name} references unresolved Tag {index}"));
                };
                if !seen.insert(index) || definitions.classification(tag.type_index) & flag == 0 {
                    return Err(format!(
                        "{name} has an invalid registration for Tag {index}"
                    ));
                }
            }
        }
        for (&house, tags) in &runtime.house_tags {
            if !self.houses.contains_key(&house) {
                return Err(format!(
                    "House Tag list references unresolved House {}",
                    house.index()
                ));
            }
            let mut seen = BTreeSet::new();
            for &index in tags {
                let Some(tag) = runtime.tag(index) else {
                    return Err(format!("House references unresolved Tag {index}"));
                };
                if !seen.insert(index) || definitions.classification(tag.type_index) & 8 == 0 {
                    return Err(format!("House has an invalid registration for Tag {index}"));
                }
            }
        }
        for (&attachment, &index) in &runtime.attachments {
            if runtime.tag(index).is_none() {
                return Err(format!("{attachment:?} references unresolved Tag {index}"));
            }
            let resolves = match attachment {
                TagAttachment::Object(id) => self.classify_object(id).is_some(),
                TagAttachment::Team(id) => self.team_script_vm.team(id).is_some(),
            };
            if !resolves {
                return Err(format!(
                    "Tag {index} has an unresolved {attachment:?} attachment"
                ));
            }
        }
        for (&cell, &index) in &runtime.cell_tags {
            if runtime.tag(index).is_none() {
                return Err(format!("Cell {cell:?} references unresolved Tag {index}"));
            }
            let resolves = self
                .resolved_terrain
                .as_ref()
                .is_some_and(|terrain| match cell {
                    NativeCellIdentity::Real(index) => {
                        terrain.cells().get(index).is_some_and(|cell| {
                            terrain.native_cell_identity((cell.rx as i16, cell.ry as i16))
                                == NativeCellIdentity::Real(index)
                        })
                    }
                    NativeCellIdentity::Dummy => true,
                });
            if !resolves {
                return Err(format!("Tag {index} has an unresolved {cell:?} attachment"));
            }
        }
        // Original Object5F3B80/Techno6F4500 destructors do not call
        // SetTag5F5B50(nullptr). A native Tag can therefore retain a counter
        // after its last attached object is gone. Keep the saved authority;
        // requiring equality with live indexes rejects valid death snapshots.
        // Executed: tag_lifecycle --damage-events object_destruction_controls.
        Ok(())
    }

    pub(crate) fn trigger_definitions_prepared(&self) -> bool {
        self.trigger_runtime.definitions_prepared
    }
    /// Full_Init687984..6879CB registry ingress. No live instance is created
    /// until a Cell/Object attachment or Fill classification requires it.
    pub fn initialize_trigger_definitions(
        &mut self,
        map: &crate::map::map_file::MapFile,
        teams: &crate::rules::team_ai_ini::TeamAiIniRegistry,
        eva: Option<&crate::rules::sound_ini::EvaRegistry>,
        sounds: Option<&crate::rules::sound_ini::SoundRegistry>,
    ) {
        self.trigger_runtime = TriggerRuntime {
            definitions: RuntimeDefinitions::read_map(map, teams, eva, sounds),
            definitions_prepared: true,
            locals_set: map
                .local_variables
                .values()
                .filter(|v| v.initially_set)
                .map(|v| v.index)
                .collect(),
            ..TriggerRuntime::default()
        };
    }

    fn trigger_difficulty(&self) -> i32 {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(difficulty) = self.trigger_runtime.fixture_difficulty {
            return difficulty;
        }
        self.session
            .campaign_house_difficulties()
            .map_or(1, |(player, _)| player.table_index() as i32)
    }

    /// Legacy unit fixtures declare one synthetic Tag per Trigger. This seam
    /// supplies fixture definitions only; construction, polling and actions use
    /// the same production owner. Map loading always uses the physical reader.
    #[cfg(any(test, feature = "test-support"))]
    fn initialize_map_triggers(&mut self, triggers: &TriggerMap, locals: &LocalVariableMap) {
        let mut ids: Vec<_> = triggers.keys().collect();
        ids.sort();
        let mut definitions = RuntimeDefinitions::default();
        for id in &ids {
            definitions.trigger_type(id);
        }
        for id in ids {
            let index = definitions.trigger_type(id).unwrap();
            let record = &triggers[id];
            let linked = record
                .linked_trigger_id
                .as_deref()
                .and_then(|id| definitions.trigger_type(id));
            let typ = &mut definitions.trigger_types[index];
            typ.owner = record.owner.clone();
            typ.linked = linked;
            typ.enabled = record.enabled;
            typ.transfer_on_capture |= record
                .fields
                .get(7)
                .is_some_and(|value| crt_atoi(value) != 0);
            typ.difficulty = [
                record.difficulty.easy,
                record.difficulty.medium,
                record.difficulty.hard,
            ];
            definitions.tag_types.push(TagType {
                id: id.clone(),
                name: id.clone(),
                repeat: 0,
                trigger: Some(index),
            });
        }
        self.trigger_runtime = TriggerRuntime {
            definitions,
            definitions_prepared: true,
            locals_set: locals
                .values()
                .filter(|v| v.initially_set)
                .map(|v| v.index)
                .collect(),
            fixture_awaiting_records: true,
            ..TriggerRuntime::default()
        };
    }

    #[cfg(any(test, feature = "test-support"))]
    fn install_pending_trigger_fixture(&mut self, inputs: TriggerInputs<'_>) {
        if !self.trigger_runtime.fixture_awaiting_records {
            return;
        }
        let count = self.trigger_runtime.definitions.trigger_types.len();
        for index in 0..count {
            let id = self.trigger_runtime.definitions.trigger_types[index]
                .id
                .to_ascii_uppercase();
            let events = inputs
                .events
                .get(&id)
                .map_or_else(Vec::new, |r| r.conditions.clone());
            let entries = inputs
                .actions
                .get(&id)
                .map_or_else(Vec::new, |r| r.entries.clone());
            let actions = entries
                .into_iter()
                .map(|entry| {
                    self.trigger_runtime
                        .definitions
                        .resolve_action(entry, None, None)
                })
                .collect();
            let typ = &mut self.trigger_runtime.definitions.trigger_types[index];
            typ.events = events.into_iter().map(RuntimeEvent::from).collect();
            typ.actions = actions;
        }
        self.trigger_runtime.fixture_awaiting_records = false;
        self.classify_map_triggers(inputs.rules);
    }

    /// Original Tag6E52A0 includes pending-but-not-yet-destroyed instances.
    fn shared_trigger_tag(&mut self, type_index: usize) -> usize {
        if let Some(index) = self.trigger_runtime.tag_order.iter().copied().find(|&i| {
            self.trigger_runtime
                .tag(i)
                .is_some_and(|tag| tag.type_index == type_index)
        }) {
            return index;
        }
        self.construct_trigger_tag(type_index)
    }

    fn construct_trigger_tag(&mut self, type_index: usize) -> usize {
        //6E4DE0/725FA0 call Abstract410170, whose ID remains-1: no
        // Create_ID410230 call and no Scenario numeric-ID draw occurs here.
        let tag_index = self.trigger_runtime.tags.len();
        self.trigger_runtime.tags.push(Some(LiveTag {
            type_index,
            triggers: Vec::new(),
            refs: 0,
            attached_cell: (0, 0),
            pending: false,
            dispatch_guard: false,
        }));
        self.trigger_runtime.tag_order.push(tag_index);
        let mut next = self.trigger_runtime.definitions.tag_types[type_index].trigger;
        let mut visited = BTreeSet::new();
        let difficulty = self.trigger_difficulty();
        while let Some(type_index) = next {
            if !visited.insert(type_index) {
                break;
            }
            let index = self.trigger_runtime.triggers.len();
            self.trigger_runtime.triggers.push(Some(LiveTrigger {
                type_index,
                timer: CdTimer::started(self.session.binary_frame as i32, 0),
                completed: 0,
                retained_house: None,
                enabled: true,
                pending: false,
            }));
            self.trigger_runtime.trigger_order.push(index);
            // Reset726400 runs BEFORE enabled/difficulty admission.
            self.reset_trigger_timers(index);
            let typ = &self.trigger_runtime.definitions.trigger_types[type_index];
            let enabled = typ.enabled && typ.admits_difficulty(difficulty);
            next = typ.linked;
            self.trigger_runtime.triggers[index]
                .as_mut()
                .unwrap()
                .enabled = enabled;
            self.trigger_runtime.tags[tag_index]
                .as_mut()
                .unwrap()
                .triggers
                .insert(0, index);
        }
        tag_index
    }

    fn reset_trigger_timers(&mut self, index: usize) {
        let Some(trigger) = self.trigger_runtime.trigger(index) else {
            return;
        };
        let type_index = trigger.type_index;
        let event_count = self.trigger_runtime.definitions.trigger_types[type_index]
            .events
            .len();
        for ordinal in 0..event_count {
            let event = &self.trigger_runtime.definitions.trigger_types[type_index].events[ordinal];
            let duration = match event.kind {
                13 => event.value.wrapping_mul(LOGICAL_FRAMES_PER_SECOND),
                51 => {
                    let value = event.value;
                    let drawn = self.scenario_rng.next_range_i32_inclusive(0, value);
                    (value / 2)
                        .wrapping_add(drawn)
                        .wrapping_mul(LOGICAL_FRAMES_PER_SECOND)
                }
                _ => continue,
            };
            let trigger = self.trigger_runtime.triggers[index].as_mut().unwrap();
            trigger
                .timer
                .start(self.session.binary_frame as i32, duration);
            trigger.completed &= !(1u32.wrapping_shl(ordinal as u32));
        }
    }

    /// Native4AD1B3 ordered CellTags reader. Run after Cell storage is installed
    /// and before authored Techno attachments/Fill. Reads through shared INI
    /// readers; registry/instance order is never taken from a HashMap scan.
    pub(crate) fn install_authored_cell_trigger_tags(
        &mut self,
        map: &crate::map::map_file::MapFile,
    ) {
        let Some(section) = map.ini.section("CellTags") else {
            return;
        };
        // Basic68A13D pushes default0; use the existing production reader.
        let format = map.basic.new_ini_format.unwrap_or(0);
        for key in section.keys() {
            let Some(name) = section.read_name(key, 128) else {
                continue;
            };
            //4AD20C resolves the allocating TagType factory before admission.
            let Some(type_index) = self.trigger_runtime.definitions.tag_type(name) else {
                continue;
            };
            let packed = crt_atoi(key);
            let stride = if format < 4 { 128 } else { 1000 };
            let cell = ((packed % stride) as i16, (packed / stride) as i16);
            let identity = self.trigger_cell_identity(cell);
            if self.trigger_runtime.cell_tags.contains_key(&identity) {
                continue;
            }
            let tag = self.shared_trigger_tag(type_index);
            self.trigger_runtime.tags[tag]
                .as_mut()
                .unwrap()
                .attached_cell = cell;
            //4AD2C9 repeats5657A0 after live construction; a miss stamps the
            // same retained Dummy a second time before CellReplace485250.
            let identity = self.trigger_cell_identity(cell);
            self.replace_cell_trigger_tag(identity, Some(tag));
        }
    }

    /// Fill684D47..684EC1 registers in retained TagType order: map, Logic,
    /// then House. Registration does not increase the attachment refcount.
    pub fn classify_map_triggers(&mut self, rules: Option<&crate::rules::ruleset::RuleSet>) {
        if self.trigger_runtime.classified {
            return;
        }
        for type_index in 0..self.trigger_runtime.definitions.tag_types.len() {
            let flags = self.trigger_runtime.definitions.classification(type_index);
            if flags & 4 != 0 {
                let tag = self.shared_trigger_tag(type_index);
                self.trigger_runtime.map_tags.push(tag);
            }
            if flags & 16 != 0 {
                let tag = self.shared_trigger_tag(type_index);
                self.trigger_runtime.logic_tags.push(tag);
            }
            if flags & 8 != 0 {
                let tag = self.shared_trigger_tag(type_index);
                let owner = self.trigger_runtime.definitions.tag_types[type_index]
                    .trigger
                    .and_then(|t| {
                        self.trigger_runtime.definitions.trigger_types[t]
                            .owner
                            .as_deref()
                    });
                if let Some(house) = resolve_trigger_house(self, owner, rules) {
                    self.trigger_runtime
                        .house_tags
                        .entry(house)
                        .or_default()
                        .push(tag);
                }
            }
        }
        // Fill684EC3 counts each AllowWin TagType after registration, even
        // disabled ones. It counts definition families, not attached refs.
        for type_index in 0..self.trigger_runtime.definitions.tag_types.len() {
            if !self.trigger_runtime.definitions.has_action(type_index, 15) {
                continue;
            }
            let owner = self.trigger_runtime.definitions.tag_types[type_index]
                .trigger
                .and_then(|t| {
                    self.trigger_runtime.definitions.trigger_types[t]
                        .owner
                        .as_deref()
                });
            if let Some(house) = resolve_trigger_house(self, owner, rules)
                && let Some(house) = self.houses.get_mut(&house)
            {
                house.register_allow_win_tag_type();
            }
        }
        self.trigger_runtime.classified = true;
    }

    fn trigger_cell_identity(&self, cell: (i16, i16)) -> NativeCellIdentity {
        let terrain = self
            .resolved_terrain
            .as_ref()
            .expect("live CellTags require installed native map cell storage");
        NativeCellQuery::canonical(terrain).lookup(cell)
    }

    fn replace_cell_trigger_tag(&mut self, cell: NativeCellIdentity, tag: Option<usize>) {
        if let Some(previous) = self.trigger_runtime.cell_tags.remove(&cell)
            && let Some(old) = self.trigger_runtime.tags[previous].as_mut()
        {
            old.refs = old.refs.wrapping_sub(1);
        }
        if let Some(tag) = tag {
            self.trigger_runtime.cell_tags.insert(cell, tag);
            let live = self.trigger_runtime.tags[tag].as_mut().unwrap();
            live.refs = live.refs.wrapping_add(1);
        }
    }

    /// Map568CDB..568DCA's fresh InitCellAttributes line-marker subset.
    /// The caller supplies its existing anti-diagonal final cell iterator.
    /// Horizontal25 wins over vertical26 for a Tag carrying both. Flags
    /// remain on cells after a later Tag detach, as the native writer does.
    /// Steam3e81a617..08600; tools/spatial_oracle/tag_lifecycle --line-events.
    pub(crate) fn initialize_cell_trigger_lines(
        &self,
        terrain: &mut crate::map::resolved_terrain::ResolvedTerrainGrid,
        cells: impl Iterator<Item = (i32, i32)>,
    ) {
        let bounds = crate::sim::scenario_bootstrap::NativeStartBounds::from_session(self, terrain);
        for (x, y) in cells {
            let source = terrain.native_cell_identity((x as i16, y as i16));
            let Some(tag) = self.trigger_runtime.cell_tags.get(&source).copied() else {
                continue;
            };
            let Some(tag) = self.trigger_runtime.tag(tag) else {
                continue;
            };
            let coord = terrain.native_cell_coord(source);
            let (kind, count, flag) = if self
                .trigger_runtime
                .definitions
                .has_event(tag.type_index, 25)
            {
                (25, bounds.width, 0x100000)
            } else if self
                .trigger_runtime
                .definitions
                .has_event(tag.type_index, 26)
            {
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

    fn replace_trigger_tag_attachment(
        &mut self,
        attachment: TagAttachment,
        tag: Option<usize>,
    ) -> bool {
        if let Some(previous) = self.trigger_runtime.attachments.remove(&attachment)
            && let Some(old) = self.trigger_runtime.tags[previous].as_mut()
            && matches!(attachment, TagAttachment::Object(_))
        {
            old.refs = old.refs.wrapping_sub(1);
        }
        if let Some(tag) = tag {
            self.trigger_runtime.attachments.insert(attachment, tag);
            let live = self.trigger_runtime.tags[tag].as_mut().unwrap();
            if matches!(attachment, TagAttachment::Object(_)) {
                live.refs = live.refs.wrapping_add(1);
            }
        }
        tag.is_some()
    }

    /// Physical Object storage disappears without a SetTag5F5B50 call.
    /// Steam3e81a617 Object5F3B80, including actual scalar/drain controls in
    /// tag_lifecycle --damage-events, leaves Tag+2C and the Tag itself intact.
    /// Remove only the Rust index whose source object no longer exists; no
    /// counter write, pending ticket, RNG draw or timer change belongs here.
    pub(crate) fn finalize_object_trigger_tag_attachment(&mut self, object: u64) {
        self.trigger_runtime
            .attachments
            .remove(&TagAttachment::Object(object));
    }

    /// Object5F5B50 attachment funnel. An absent/null authored reference clears
    /// the current attachment without destroying a zero-reference Tag.
    pub(crate) fn attach_object_trigger_tag(&mut self, object: u64, name: Option<&str>) {
        let type_index = name.and_then(|name| {
            self.trigger_runtime
                .definitions
                .tag_types
                .iter()
                .position(|typ| {
                    typ.id.eq_ignore_ascii_case(name) || typ.name.eq_ignore_ascii_case(name)
                })
        });
        let tag = type_index.map(|typ| self.shared_trigger_tag(typ));
        self.replace_trigger_tag_attachment(TagAttachment::Object(object), tag);
    }
    /// Team constructor6E8D76..6E8D99 allocates an independent Tag rather
    /// than sharing the map TagType instance. Team+70 itself is uncounted;
    /// members enter the ordinary Object5F5B50 reference-count funnel.
    pub(crate) fn attach_team_trigger_tag(
        &mut self,
        team: u64,
        team_type: crate::sim::intern::InternedId,
    ) {
        let definitions = &self.trigger_runtime.definitions;
        let name = self.interner.resolve(team_type);
        let type_index = definitions
            .team_types
            .iter()
            .position(|id| id.eq_ignore_ascii_case(name))
            .and_then(|index| definitions.team_tags[index]);
        let tag = type_index.map(|typ| self.construct_trigger_tag(typ));
        self.replace_trigger_tag_attachment(TagAttachment::Team(team), tag);
    }

    pub(crate) fn attach_team_member_trigger_tag(&mut self, team: u64, object: u64) {
        // AddMember6EA582..6EA587 skips attachment when Team+70 is NULL,
        // retaining an authored Object Tag. The B3/Passengers admission gate
        // belongs to the shared Team caller; Object5F5B50 owns references.
        // Executed controls: tools/spatial_oracle/team_creation.member_tag.
        let Some(tag) = self
            .trigger_runtime
            .attachments
            .get(&TagAttachment::Team(team))
            .copied()
        else {
            return;
        };
        self.replace_trigger_tag_attachment(TagAttachment::Object(object), Some(tag));
    }

    /// Team RemoveMember6EA892..6EA8B5 may clear only the Team's own Tag.
    /// Its caller retains the native House admission gate; authored object
    /// attachments belonging to another Tag remain intact.
    pub(crate) fn clear_team_member_trigger_tag(&mut self, team: u64, object: u64) {
        if let Some(&tag) = self
            .trigger_runtime
            .attachments
            .get(&TagAttachment::Team(team))
            && self
                .trigger_runtime
                .attachments
                .get(&TagAttachment::Object(object))
                == Some(&tag)
        {
            self.replace_trigger_tag_attachment(TagAttachment::Object(object), None);
        }
    }

    /// Destructor6E8E75..6E8EAB follows member removal. Only a zero-reference
    /// Tag is queued, then the direct Team pointer is cleared without a draw.
    pub(crate) fn clear_team_trigger_tag(&mut self, team: u64) {
        if let Some(&tag) = self
            .trigger_runtime
            .attachments
            .get(&TagAttachment::Team(team))
            && self
                .trigger_runtime
                .tag(tag)
                .is_some_and(|tag| tag.refs == 0)
        {
            self.mark_live_tag_for_delete(tag);
        }
        self.replace_trigger_tag_attachment(TagAttachment::Team(team), None);
    }

    fn mark_live_trigger_for_delete(&mut self, index: usize) {
        if let Some(trigger) = self
            .trigger_runtime
            .triggers
            .get_mut(index)
            .and_then(Option::as_mut)
        {
            trigger.pending = true;
            self.substrate.pending_delete.queue_trigger(index);
        }
    }
    fn mark_live_tag_for_delete(&mut self, index: usize) {
        if let Some(tag) = self
            .trigger_runtime
            .tags
            .get_mut(index)
            .and_then(Option::as_mut)
        {
            tag.pending = true;
            self.substrate.pending_delete.queue_tag(index);
        }
    }
    fn detach_live_tag(&mut self, index: usize) {
        //7258D0 -> every Object/Cell, Map577920 and Logic55B880. This does
        // not set Tag.pending; ProcessTriggerEvent queues its removal separately.
        self.trigger_runtime.logic_tags.retain(|&tag| tag != index);
        self.trigger_runtime.map_tags.retain(|&tag| tag != index);
        for tags in self.trigger_runtime.house_tags.values_mut() {
            tags.retain(|&tag| tag != index);
        }
        let attachments: Vec<_> = self
            .trigger_runtime
            .attachments
            .iter()
            .filter_map(|(&id, &tag)| (tag == index).then_some(id))
            .collect();
        for attachment in attachments {
            self.replace_trigger_tag_attachment(attachment, None);
        }
        let cells: Vec<_> = self
            .trigger_runtime
            .cell_tags
            .iter()
            .filter_map(|(&cell, &tag)| (tag == index).then_some(cell))
            .collect();
        for cell in cells {
            self.replace_cell_trigger_tag(cell, None);
        }
    }

    /// Trigger deleting destructor726950 selected by the common725C70 owner.
    /// Pointer expiry splices this identity out of every live Tag chain.
    pub(crate) fn delete_live_trigger(&mut self, index: usize) {
        if self.trigger_runtime.trigger(index).is_none() {
            return;
        }
        for tag in self.trigger_runtime.tags.iter_mut().flatten() {
            tag.triggers.retain(|&t| t != index);
        }
        self.trigger_runtime.trigger_order.retain(|&t| t != index);
        self.trigger_runtime.triggers[index] = None;
        #[cfg(any(test, feature = "test-support"))]
        self.trigger_runtime.freed_trace.push("trigger");
    }

    /// Tag destructor6E4F60 selected by the common725C70 owner. It can
    /// append Trigger tickets; the same global live scan consumes them later.
    pub(crate) fn delete_live_tag(
        &mut self,
        index: usize,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        game_state: i32,
        started: bool,
    ) {
        let Some(tag) = self.trigger_runtime.tag(index) else {
            return;
        };
        let typ = tag.type_index;
        let triggers = tag.triggers.clone();
        self.detach_live_tag(index);
        // Active-start destructor6E4F60 searches AFTER detach:
        // Find=-1 therefore still decrements the signed cursor.
        let flags = self.trigger_runtime.definitions.classification(typ);
        self.adjust_tag_destructor_cursors(flags, game_state, started);
        if (game_state as u8) != 0
            && started
            && self.trigger_runtime.definitions.has_action(typ, 15)
        {
            // Destructor uses the live reversed-chain head's owner.
            let owner = triggers
                .first()
                .and_then(|&t| self.trigger_runtime.trigger(t))
                .and_then(|t| {
                    self.trigger_runtime.definitions.trigger_types[t.type_index]
                        .owner
                        .as_deref()
                });
            if let Some(house) = resolve_trigger_house(self, owner, rules)
                && let Some(house) = self.houses.get_mut(&house)
            {
                house.removed_allow_win_tag(self.session.binary_frame as i32);
            }
        }
        for trigger in triggers {
            self.substrate.pending_delete.queue_trigger(trigger);
        }
        self.trigger_runtime.tag_order.retain(|&t| t != index);
        self.trigger_runtime.tags[index] = None;
        #[cfg(any(test, feature = "test-support"))]
        self.trigger_runtime.freed_trace.push("tag");
    }
    fn adjust_tag_destructor_cursors(&mut self, flags: u8, game_state: i32, started: bool) {
        if (game_state as u8) == 0 || !started {
            return;
        }
        for (bit, cursor, count) in [
            (
                16,
                &mut self.trigger_runtime.logic_cursor,
                self.trigger_runtime.logic_tags.len(),
            ),
            (
                4,
                &mut self.trigger_runtime.map_cursor,
                self.trigger_runtime.map_tags.len(),
            ),
        ] {
            if flags & bit != 0 && *cursor >= -1 {
                *cursor = cursor.wrapping_sub(1);
                if *cursor < 0 && count == 0 {
                    *cursor = 0;
                }
            }
        }
    }

    /// Selected HasOccurred71E940 predicates. Numeric/timer controls execute
    /// the retail body; shared +54 and mutable persistence belong to this owner.
    fn has_occurred(
        &mut self,
        ordinal: usize,
        input: TagEventInput,
        incoming_house: Option<crate::sim::intern::InternedId>,
        persistence: &mut bool,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        live_index: usize,
    ) -> bool {
        let type_index = self.trigger_runtime.trigger(live_index).unwrap().type_index;
        let condition = self.trigger_runtime.definitions.trigger_types[type_index].events[ordinal]
            .condition
            .clone();
        match condition.kind {
            23 => {
                //71EC9A gate, then71EEA9 scans all Teams for the retained
                //Type pointer, null head and nonzero leaving byte. A match
                //sets persistence before the generic true tail; a null
                //incoming House skips that scan without latching.
                if input.kind != 23 || self.buildup_bypass_active() {
                    return false;
                }
                if incoming_house.is_none() {
                    return true;
                }
                let team_type = match condition.team_name.as_deref() {
                    Some(name) => {
                        let Some(id) = self.interner.get(name) else {
                            return false;
                        };
                        Some(id)
                    }
                    None => None,
                };
                if self.team_script_vm.has_empty_leaving_team(team_type) {
                    *persistence = true;
                    true
                } else {
                    false
                }
            }
            34 => {
                //71EDBB..71EE54: 68BD00 waypoint Location versus actor
                //GetCoords, signed wrapping deltas, sqrtApprox/ftol <=1280.
                //No persistence, Event+54, timer, RNG or detach write.
                if input.kind != 34 || self.buildup_bypass_active() {
                    return false;
                }
                let Some(waypoint) = self.trigger_waypoint_location(condition.value) else {
                    return false;
                };
                let Some(actor) = input.actor.and_then(|id| self.substrate.entities.get(id)) else {
                    return false;
                };
                let point = crate::sim::movement::ground_pose::object_get_coords(
                    actor,
                    self.resolved_terrain.as_ref(),
                );
                crate::util::native_x87::distance_3d_leptons([point.x, point.y, point.z], waypoint)
                    <= 0x500
            }
            14 => self.session.scenario_timer_elapsed(),
            // HasOccurred45@71EAE7 /46@71EB06 read Scenario+352C,
            // not normal/target ambient: signed inclusive comparisons with
            // Event+34. Both return directly, without House/raised-kind or
            // A8ED6B admission, persistence, timer, RNG or detach writes.
            // Original Steam gamemd3e81a617..08600, instruction reading.
            45 => self.session.lighting.current_ambient <= condition.value,
            46 => self.session.lighting.current_ambient >= condition.value,
            17 => {
                // Steam3e81a617..08600:71EE82 takes the incoming House;
                // 71EF3F..71EF89 reads tracked+2F0, then its ordered+68 list.
                // Only null/limbo entries are skipped: neither Health nor
                // Alive gates this leaf. Factory is the raw nonzero RTTI,
                // independent of Event.value, actor, raised kind and init.
                // No persistence, Event+54, timer, RNG or detach write.
                // Executed controls: tag_lifecycle --no-factories-events.
                let Some(owner) = incoming_house else {
                    return true;
                };
                let Some(house) = self.houses.get(&owner) else {
                    return false;
                };
                if house.tracking.buildings() <= 0 {
                    return true;
                }
                let Some(rules) = rules else {
                    return false;
                };
                !house.base_projection.buildings().iter().any(|&id| {
                    self.substrate.entities.get(id).is_some_and(|building| {
                        !building.lifecycle.in_limbo()
                            && self
                                .object_type(building.type_ref(), rules)
                                .is_some_and(|ty| ty.has_factory())
                    })
                })
            }
            19..=22 => {
                // Steam3e81a617: HasOccurred71EFEB..71F03D reads incoming
                // House+26C/+274/+270/+278, not a Country or built count.
                // Match sets persistence; mismatch leaves it alone. The null
                // House takes the generic true tail without that write.
                // Original execution: tag_lifecycle --build-type-events.
                let Some(owner) = incoming_house else {
                    return true;
                };
                use crate::rules::object_type::ObjectCategory;
                let category = match condition.kind {
                    19 => ObjectCategory::Building,
                    20 => ObjectCategory::Vehicle,
                    21 => ObjectCategory::Infantry,
                    22 => ObjectCategory::Aircraft,
                    _ => unreachable!(),
                };
                let matched = self
                    .houses
                    .get(&owner)
                    .is_some_and(|house| house.last_built_type_index(category) == condition.value);
                if matched {
                    *persistence = true;
                }
                matched
            }
            12 | 52 => {
                // Steam3e81a617..08600: HasOccurred71EF01/71EF20 calls
                // IHouse::AvailableMoney4F6990 on Register7264C0's incoming
                // House, then compares the literal signed threshold inclusively.
                // No raised-kind gate, persistence, retained-House or timer write.
                // Executed: tag_lifecycle --funds-events. The shared Economy
                // getter retains its documented empty-silo coverage boundary.
                let Some(owner) = incoming_house else {
                    return true;
                };
                let funds = crate::sim::credit_income::available_money(self, owner);
                if condition.kind == 12 {
                    funds >= condition.value
                } else {
                    funds <= condition.value
                }
            }
            1 | 24 | 25 | 26 | 59 => {
                //71ECAF ->71F1BD compares actor6F9DB0's House ArrayIndex
                //with Country502D30's first matching House+30, then retains
                //the actor's House via6F9DC0. Rust House identities represent
                //unique registered slots. Wildcard-1 skips the comparison.
                //The duplicate-Country distinction is executed by the
                //--entered-cell-events controls, not a Country-value match.
                if input.kind != condition.kind || self.buildup_bypass_active() {
                    return false;
                }
                let Some(owner) = input
                    .actor
                    .and_then(|actor| self.substrate.entities.get(actor))
                    .map(|actor| actor.owner())
                else {
                    return false;
                };
                if !self.houses.contains_key(&owner) {
                    return false;
                }
                let selected_house = rules.and_then(|rules| {
                    u16::try_from(condition.value).ok().and_then(|country| {
                        self.house_by_country_index(
                            crate::rules::ruleset::CountryIdx(country),
                            rules,
                        )
                    })
                });
                if condition.value != -1 && selected_house != Some(owner) {
                    return false;
                }
                self.trigger_runtime.definitions.trigger_types[type_index].events[ordinal]
                    .retained_house = Some(owner);
                *persistence = true;
                true
            }
            32 | 57 => {
                //71EEFB/71EF1F ->49FAE0 with House+5550 (active, not owned).
                //The null incoming House reaches the original generic true
                //tail without writing persistence.
                let Some(owner) = incoming_house else {
                    return true;
                };
                let Some(rules) = rules else {
                    return false;
                };
                let Some(type_name) = usize::try_from(condition.value).ok().and_then(|index| {
                    rules
                        .type_array_ids(crate::rules::object_type::ObjectCategory::Building)
                        .get(index)
                }) else {
                    return false;
                };
                let count = self.houses.get(&owner).map_or(0, |house| {
                    self.interner.get(type_name).map_or(0, |id| {
                        house
                            .tracking
                            .active_count(crate::map::entities::EntityCategory::Structure, id)
                    })
                });
                let result = (count != 0) == (condition.kind == 32);
                if result {
                    *persistence = true;
                }
                result
            }
            15 | 16 => {
                // HasOccurred71EE82 takes the Trigger's incoming House,
                // not Event.value's Country. 71F03F/71F058 compare signed
                // House+5488/+5434 loss counters with Event+34 using >=.
                // The null-House tail is true; neither arm latches, writes
                // the timer/retained House, draws RNG or detaches anything.
                // Steam3e81a617..08600; tag_lifecycle --loss-threshold-events.
                let Some(owner) = incoming_house else {
                    return true;
                };
                let Some(house) = self.houses.get(&owner) else {
                    return false;
                };
                let losses = if condition.kind == 15 {
                    house.stats.buildings_lost()
                } else {
                    house.stats.units_lost()
                };
                (losses as i32) >= condition.value
            }
            9 | 10 | 11 | 55 | 56 => {
                //71F069 ->502D30: Event.value selects a Country's first
                //registered House, independently of the incoming House or
                //raised event. No matching House reaches the AL1 tail; this
                //does not write persistence or the Type-owned Event+54.
                //The private HouseTracking owner preserves separate signed
                //Unit/Infantry totals and the tracked Building scalar.
                //10's71F115..71F12E tests tracked buildings <=0, including
                //constructor-complete Limbo until actual destruction. Steam
                //3e81a617..08600; tag_lifecycle --building-loss-events.
                //55/56: Steam3e81a617..08600, 71F159 /71F16F..71F187;
                //tools/spatial_oracle/tag_lifecycle.py --naval-land-events.
                //56's TEST after SUB observes wrapping subtraction, not a
                //signed comparison of the two source counts. These events
                //read tracked scalars, unlike9's on-map CounterClass totals.
                let owner = rules.and_then(|rules| {
                    u16::try_from(condition.value).ok().and_then(|country| {
                        self.house_by_country_index(
                            crate::rules::ruleset::CountryIdx(country),
                            rules,
                        )
                    })
                });
                owner.is_none_or(|owner| {
                    self.houses
                        .get(&owner)
                        .is_none_or(|house| match condition.kind {
                            9 => house.tracking.units_destroyed(),
                            10 => house.tracking.buildings() <= 0,
                            11 => house.tracking.all_destroyed(),
                            55 => house.tracking.navy() <= 0,
                            56 => {
                                house
                                    .tracking
                                    .vehicles()
                                    .wrapping_sub(house.tracking.navy())
                                    <= 0
                                    && house.tracking.infantry() <= 0
                            }
                            _ => unreachable!(),
                        })
                })
            }
            30 | 58 => {
                //71EFA2/71EFC2 selects Event.value's Country, then the retained
                //House power ratio. Neither branch writes persistence.
                let owner = rules.and_then(|rules| {
                    u16::try_from(condition.value).ok().and_then(|country| {
                        self.house_by_country_index(
                            crate::rules::ruleset::CountryIdx(country),
                            rules,
                        )
                    })
                });
                owner.is_none_or(|owner| {
                    self.power_states
                        .get(&owner)
                        .is_none_or(crate::sim::power_system::PowerState::has_full_power)
                        == (condition.kind == 58)
                })
            }
            44 => {
                //Object5F57FA supplies source Techno;71EE57/71EE70 reads
                //that live object's +21C -> House+30 at predicate time.
                //The --damage-source-events full-body controls establish the
                //shared A8ED6B gate71EC9A..71ECA9 before this source arm;
                //neither timer nor Event+54 is written.
                if input.kind != 44 || self.buildup_bypass_active() {
                    return false;
                }
                input
                    .source
                    .and_then(|source| self.substrate.entities.get(source))
                    .and_then(|source| self.base_reservation_house_index(source.owner()))
                    .is_some_and(|index| index == condition.value)
            }
            13 | 51 => self
                .trigger_runtime
                .trigger(live_index)
                .unwrap()
                .timer
                .expired(self.session.binary_frame as i32),
            //71EC83/71F284 routes these discrete events through71EC9A:
            //matching raised kind and clear A8ED6B, then71F1B1 AL1.
            //All secondary switch entries are the generic true tail; no
            //House-retention or persistence write. Original Steam identity:
            //3e81a617..08600; RecordKill702D40 publishes4/6/7/48/29 here.
            //Object5F55EA..5F573F raises38..43 through this same gate. The
            //original bodies ignore value/incoming House/actor/source here;
            //tag_lifecycle.py --damage-events / trigger_damage_events.json
            //execute30 controls, including prior persistence and NULL House.
            //49/50 use the same gate. Steam3e81a617,71EC9A/71F1B1;
            //trigger_action_values --crate records their original predicates.
            //31: bridge575EE0 raises it on each tagged span cell. After this
            //gate,71EE91 routes it to71F069, whose Country502D30 lookup
            //selects only the71F1B1 true tail either way.
            2 | 4 | 6 | 7 | 29 | 31 | 33 | 38 | 39 | 40 | 41 | 42 | 43 | 48 | 49 | 50 => {
                input.kind == condition.kind && !self.buildup_bypass_active()
            }
            //71F28B selects the generic true tail71F1B4, no persistence write.
            EVENT_ANY => true,
            _ => self
                .trigger_runtime
                .evaluate_event(&condition, self.session.binary_frame, self),
        }
    }

    fn register_live_trigger_event(
        &mut self,
        index: usize,
        input: TagEventInput,
        mut persistence: bool,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) -> bool {
        //7264C0 admits enabled/nonpending instances. Force bypasses predicate
        //and House forwarding; its caller-seeded persistence still resets.
        let Some(trigger) = self.trigger_runtime.trigger(index) else {
            return false;
        };
        if !trigger.enabled || trigger.pending {
            return false;
        }
        let type_index = trigger.type_index;
        let incoming_house = resolve_trigger_house(
            self,
            self.trigger_runtime.definitions.trigger_types[type_index]
                .owner
                .as_deref(),
            rules,
        );
        let mut result = true;
        if !input.force {
            let count = self.trigger_runtime.definitions.trigger_types[type_index]
                .events
                .len();
            for ordinal in 0..count {
                let bit = 1u32.wrapping_shl(ordinal as u32);
                let completed = self.trigger_runtime.trigger(index).unwrap().completed & bit != 0;
                if !completed {
                    if !self.has_occurred(
                        ordinal,
                        input,
                        incoming_house,
                        &mut persistence,
                        rules,
                        index,
                    ) {
                        result = false;
                        continue;
                    }
                    let kind = self.trigger_runtime.definitions.trigger_types[type_index].events
                        [ordinal]
                        .kind;
                    //726564..726582 uses the current mutable flag, then BOTH
                    //native71F950/71F9C0 classifications. Entered/count/power
                    //do not latch despite writes to persistence.
                    if persistence
                        && matches!(
                            kind,
                            2 | 3
                                | 4
                                | 7
                                | 18
                                | 19
                                | 20
                                | 21
                                | 22
                                | 23
                                | 24
                                | 25
                                | 26
                                | 29
                                | 33
                                | 34
                                | 35
                                | 38
                                | 39
                                | 40
                                | 41
                                | 42
                                | 43
                                | 48
                                | 49
                                | 50
                                | 53
                                | 54
                        )
                    {
                        self.trigger_runtime.triggers[index]
                            .as_mut()
                            .unwrap()
                            .completed |= bit;
                    }
                }
                //726584 forwards shared TEvent+54 even for an already latched
                //node. Null never clears the Live+2C retained House.
                if let Some(house) = self.trigger_runtime.definitions.trigger_types[type_index]
                    .events[ordinal]
                    .retained_house
                {
                    self.trigger_runtime.triggers[index]
                        .as_mut()
                        .unwrap()
                        .retained_house = Some(house);
                }
            }
        }
        if result && persistence {
            self.reset_trigger_timers(index);
        }
        result
    }

    /// HouseClass::Update's Tag raise (`0x004F8F87..0x004F8FBC`): every eighth
    /// frame (signed `Frame % 8`), Tag6E53A0 raises event 8 with no object and
    /// the zero Cell on each of the House's Tags (+3C), last to first. The
    /// count (+48) is read once and the list re-read for every slot.
    /// RESIDUAL: a raise that shrinks the list leaves slots native still reads
    /// (the stale pointer past the count); VERA skips them. No retail
    /// skirmish trigger reaches it: their event-8 Tags fire once.
    pub(crate) fn raise_house_tags(
        &mut self,
        house: crate::sim::intern::InternedId,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        if (self.session.binary_frame as i32) % 8 != 0 {
            return;
        }
        let count = self
            .trigger_runtime
            .house_tags
            .get(&house)
            .map_or(0, Vec::len);
        let mut effects = Vec::new();
        for slot in (0..count).rev() {
            let Some(&tag) = self
                .trigger_runtime
                .house_tags
                .get(&house)
                .and_then(|tags| tags.get(slot))
            else {
                continue;
            };
            self.dispatch_live_tag_event(
                tag,
                TagEventInput {
                    kind: EVENT_ANY,
                    actor: None,
                    source: None,
                    cell: (0, 0),
                    force: false,
                },
                &mut effects,
                rules,
                overlay_registry,
                frame_effects,
            );
        }
        self.publish_trigger_effects(effects);
    }

    /// Recalc6EA49F..6EA4D2 walks the current Tag array backwards before
    /// destroying an empty, previously-full leaving Team. No own RNG draws,
    /// timers or detach: ordinary Register/Reset/actions own those effects.
    pub(crate) fn raise_team_left_map_tags(
        &mut self,
        rules: &crate::rules::ruleset::RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let count = self.trigger_runtime.tag_order.len();
        let mut effects = Vec::new();
        for ordinal in (0..count).rev() {
            // As with House tag traversal, shrinking callbacks can leave a
            // stale native slot beyond the new count; Rust skips that slot.
            let Some(&tag) = self.trigger_runtime.tag_order.get(ordinal) else {
                continue;
            };
            if self.dispatch_live_tag_event(
                tag,
                TagEventInput {
                    kind: 23,
                    ..Default::default()
                },
                &mut effects,
                Some(rules),
                registry,
                frame_effects,
            ) && self.trigger_runtime.tag_order.is_empty()
            {
                break;
            }
        }
        self.publish_trigger_effects(effects);
    }

    /// Original Tag6E53A0 attached-object ingress. `subject` chooses the
    /// attachment; `actor` and `source` keep the caller's distinct objects.
    /// Discovery6F4A18, Infantry519B53/519F44/51A025 use this same funnel.
    /// Returns whether the caller had a nonnull Tag, independently of whether
    /// an action fired. Pickup481A7F skips its Alive reread without a Tag.
    pub(crate) fn dispatch_object_tag_event(
        &mut self,
        subject: u64,
        kind: i32,
        actor: Option<u64>,
        source: Option<u64>,
        context: crate::sim::world::UninitContext<'_>,
    ) -> bool {
        let Some(&tag) = self
            .trigger_runtime
            .attachments
            .get(&TagAttachment::Object(subject))
        else {
            return false;
        };
        let mut effects = Vec::new();
        self.dispatch_live_tag_event(
            tag,
            TagEventInput {
                kind,
                actor,
                source,
                cell: (0, 0),
                force: false,
            },
            &mut effects,
            context.rules(),
            context.registry(),
            context.effects(),
        );
        self.publish_trigger_effects(effects);
        true
    }

    /// Bridge575EE0's per-cell ingress: each visited CellClass whose +3C Tag
    /// is nonnull calls Tag6E53A0(31, null actor, Cell ABD480, unforced,
    /// null source). ABD480 is the zero Cell (startup5618B0). The raise is
    /// synchronous inside the collapse traversal; HasOccurred71E940 admits
    /// it through71EC9A/71F069 (raised31, clear A8ED6B) without latching.
    pub(crate) fn dispatch_bridge_cell_tag_event(
        &mut self,
        cell: crate::map::cell_index::NativeCellIdentity,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let Some(&tag) = self.trigger_runtime.cell_tags.get(&cell) else {
            return;
        };
        let mut effects = Vec::new();
        self.dispatch_live_tag_event(
            tag,
            TagEventInput {
                kind: EVENT_BRIDGE_COLLAPSED,
                actor: None,
                source: None,
                cell: (0, 0),
                force: false,
            },
            &mut effects,
            Some(rules),
            overlay_registry,
            frame_effects,
        );
        self.publish_trigger_effects(effects);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn tag_dispatch_trace_for_test(&self) -> &[(usize, i32)] {
        &self.trigger_runtime.dispatch_trace
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn clear_tag_dispatch_trace_for_test(&mut self) {
        self.trigger_runtime.dispatch_trace.clear();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn trigger_local_is_set_for_test(&self, index: u32) -> bool {
        self.trigger_runtime.locals_set.contains(&index)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn cell_trigger_tag_for_test(
        &self,
        cell: crate::map::cell_index::NativeCellIdentity,
    ) -> Option<usize> {
        self.trigger_runtime.cell_tags.get(&cell).copied()
    }

    /// Pickup481AC1 writes Scenario+34BE only after its optional Tag callback
    /// survives. Logic55AFB0 consumes and clears this existing serialized flag.
    /// Steam3e81a617; tools/spatial_oracle/trigger_crate.json.
    pub(crate) fn mark_crate_pickup_event(&mut self) {
        self.trigger_runtime.dirty[3] = true;
    }

    /// Foot4D8978..4D8A14, reached from the unique Arrival per-cell owner.
    /// Cell5657A0 and its Tag pointer are retained before the first callback;
    /// each raise re-reads the actor's packed Location cell. Bridge100/400
    /// requires OnBridge8C. This does not invent a Fly per-cell producer.
    /// Source: tools/spatial_oracle/tag_lifecycle.py --entered-cell-events;
    /// original Steam3e81a617..08600. No own RNG/timer/detach.
    pub(crate) fn dispatch_foot_cell_tag_events(
        &mut self,
        actor: u64,
        context: crate::sim::world::UninitContext<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(actor) else {
            return;
        };
        let cell = crate::sim::movement::coord_cell(
            crate::sim::movement::ground_pose::position_world_coord(&entity.position),
        );
        let Some(terrain) = context.terrain().or(self.resolved_terrain.as_ref()) else {
            return;
        };
        let mut identity = NativeCellQuery::canonical(terrain).lookup(cell);
        let direct_tag = (terrain.native_cell_flags(identity) & 0x500 == 0 || entity.on_bridge())
            .then(|| self.trigger_runtime.cell_tags.get(&identity).copied())
            .flatten();
        let mut effects = Vec::new();
        if let Some(tag) = direct_tag {
            for kind in [1, 59] {
                let Some(entity) = self.substrate.entities.get(actor) else {
                    break;
                };
                let cell = crate::sim::movement::coord_cell(
                    crate::sim::movement::ground_pose::position_world_coord(&entity.position),
                );
                self.dispatch_live_tag_event(
                    tag,
                    TagEventInput {
                        kind,
                        actor: Some(actor),
                        source: None,
                        cell,
                        force: false,
                    },
                    &mut effects,
                    context.rules(),
                    context.registry(),
                    context.effects(),
                );
            }
        }
        //4D8A70..4D8B66: a missing/inadmissible direct CellTag does not
        //suppress line checks. Scan each complete native cell-array row/column,
        //looking up the live attachment at every slot. Input Location is
        //re-read for every callback while the traversal uses the initial XY.
        for kind in [25, 26] {
            let Some(terrain) = context.terrain().or(self.resolved_terrain.as_ref()) else {
                break;
            };
            let flag = if kind == 25 { 0x100000 } else { 0x200000 };
            if terrain.native_cell_flags(identity) & flag == 0 {
                continue;
            }
            let bounds =
                crate::sim::scenario_bootstrap::NativeStartBounds::from_session(self, terrain);
            let count = if kind == 25 {
                bounds.width
            } else {
                bounds.height
            };
            for offset in 0..count {
                let Some(terrain) = context.terrain().or(self.resolved_terrain.as_ref()) else {
                    break;
                };
                let queried = if kind == 25 {
                    (bounds.min_rx.wrapping_add(offset) as i16, cell.1)
                } else {
                    (cell.0, bounds.min_ry.wrapping_add(offset) as i16)
                };
                let selected = NativeCellQuery::canonical(terrain).lookup(queried);
                //The horizontal loop stores every last lookup into the retained
                //Cell slot4D8AAC; vertical admission then reads that same slot.
                if kind == 25 {
                    identity = selected;
                }
                let Some(tag) = self.trigger_runtime.cell_tags.get(&selected).copied() else {
                    continue;
                };
                let Some(live) = self.trigger_runtime.tag(tag) else {
                    continue;
                };
                if !self
                    .trigger_runtime
                    .definitions
                    .has_event(live.type_index, kind)
                {
                    continue;
                }
                let Some(entity) = self.substrate.entities.get(actor) else {
                    break;
                };
                let cell = crate::sim::movement::coord_cell(
                    crate::sim::movement::ground_pose::position_world_coord(&entity.position),
                );
                self.dispatch_live_tag_event(
                    tag,
                    TagEventInput {
                        kind,
                        actor: Some(actor),
                        source: None,
                        cell,
                        force: false,
                    },
                    &mut effects,
                    context.rules(),
                    context.registry(),
                    context.effects(),
                );
            }
        }
        self.publish_trigger_effects(effects);
        //The Unit MCV53/54 interlude and later Map24 zone scan are outside
        //these selected ALL01 predicates. Ordinary Fly ingress is unproven.
    }

    ///6E57C0 reads each linked LIVE Trigger's Type+A0, without enabled,
    /// pending or difficulty tests.5F5B50 replaces refs, never moves the
    /// Engineer's attachment and never deletes the old zero-ref Tag here.
    pub(crate) fn transfer_engineer_tag_to_building(&mut self, engineer: u64, building: u64) {
        let attachment = TagAttachment::Object(engineer);
        let Some(&tag) = self.trigger_runtime.attachments.get(&attachment) else {
            return;
        };
        let transfer = self.trigger_runtime.tag(tag).is_some_and(|tag| {
            tag.triggers.iter().any(|&live| {
                self.trigger_runtime.trigger(live).is_some_and(|trigger| {
                    self.trigger_runtime.definitions.trigger_types[trigger.type_index]
                        .transfer_on_capture
                })
            })
        });
        if transfer {
            //519F66 re-reads after the gate; keep the native attachment owner.
            let tag = self.trigger_runtime.attachments.get(&attachment).copied();
            self.replace_trigger_tag_attachment(TagAttachment::Object(building), tag);
        }
    }

    fn trigger_action_context(
        &self,
        type_index: usize,
        live_index: Option<usize>,
        input: TagEventInput,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) -> TriggerActionContext {
        TriggerActionContext {
            live_index,
            input,
            incoming_house: resolve_trigger_house(
                self,
                self.trigger_runtime.definitions.trigger_types[type_index]
                    .owner
                    .as_deref(),
                rules,
            ),
        }
    }

    fn spring_live_trigger(
        &mut self,
        index: usize,
        input: TagEventInput,
        effects: &mut Vec<TriggerEffect>,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        //7265C0 deliberately does not call RegisterEvent. Force22 springs
        // directly, but disabled/pending instances still reject it.
        let Some(trigger) = self.trigger_runtime.trigger(index) else {
            return false;
        };
        if !trigger.enabled || trigger.pending {
            return false;
        }
        let type_index = trigger.type_index;
        let count = self.trigger_runtime.definitions.trigger_types[type_index]
            .actions
            .len();
        let mut result = false;
        for ordinal in 0..count {
            //7265E7..726602 repeats the authored Country lookup for every
            //action and preserves Spring's incoming actor and packed Cell.
            let context = self.trigger_action_context(type_index, Some(index), input, rules);
            let action =
                self.trigger_runtime.definitions.trigger_types[type_index].actions[ordinal].clone();
            result |= self.apply_live_trigger_action(
                &action,
                context,
                effects,
                rules,
                overlay_registry,
                frame_effects,
            );
        }
        result
    }

    fn dispatch_live_tag_event(
        &mut self,
        index: usize,
        input: TagEventInput,
        effects: &mut Vec<TriggerEffect>,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        #[cfg(any(test, feature = "test-support"))]
        self.trigger_runtime
            .dispatch_trace
            .push((index, input.kind));
        let Some(tag) = self.trigger_runtime.tag(index) else {
            return false;
        };
        if self.buildup_bypass_active() || tag.pending || tag.dispatch_guard {
            return false;
        }
        let repeat = self.trigger_runtime.definitions.tag_types[tag.type_index].repeat;
        let chain = tag.triggers.clone();
        self.trigger_runtime.tags[index]
            .as_mut()
            .unwrap()
            .dispatch_guard = true;
        let mut result = false;
        let mut detach_attachment = false;
        let mut detach_all = false;
        for trigger in chain {
            if !self.register_live_trigger_event(trigger, input, repeat == 2, rules) {
                continue;
            }
            match repeat {
                0 => {
                    self.spring_live_trigger(
                        trigger,
                        input,
                        effects,
                        rules,
                        overlay_registry,
                        frame_effects,
                    );
                    self.mark_live_trigger_for_delete(trigger);
                    result = true;
                    detach_attachment = true;
                    detach_all = true;
                }
                1 => {
                    if self.trigger_runtime.tag(index).unwrap().refs == 1 {
                        self.spring_live_trigger(
                            trigger,
                            input,
                            effects,
                            rules,
                            overlay_registry,
                            frame_effects,
                        );
                        self.mark_live_trigger_for_delete(trigger);
                        result = true;
                        detach_all = true;
                    }
                    detach_attachment = true;
                }
                2 => {
                    self.spring_live_trigger(
                        trigger,
                        input,
                        effects,
                        rules,
                        overlay_registry,
                        frame_effects,
                    );
                    result = true;
                }
                _ => {}
            }
            // Action70 (6DF094 ->6E58B0) can immediately destroy this Tag,
            // including through a nested Force22. Native retains a stale
            // pointer here; safe storage must stop this dispatch rather than
            // reading the freed Tag or executing another linked Trigger.
            if self.trigger_runtime.tag(index).is_none() {
                return result;
            }
        }
        self.trigger_runtime.tags[index]
            .as_mut()
            .unwrap()
            .dispatch_guard = false;
        let object = input.actor;
        let cell = input.cell;
        if detach_attachment {
            if let Some(object) = object
                && self
                    .trigger_runtime
                    .attachments
                    .get(&TagAttachment::Object(object))
                    == Some(&index)
            {
                self.replace_trigger_tag_attachment(TagAttachment::Object(object), None);
            }
            if cell != (0, 0) {
                let identity = self.trigger_cell_identity(cell);
                self.replace_cell_trigger_tag(identity, None);
            }
        }
        if detach_all {
            self.detach_live_tag(index);
            self.substrate.pending_delete.queue_tag(index);
        }
        result
    }

    /// The 50 Scenario global values, read as the campaign carry-over record
    /// copies their bytes (`0x004C6187..0x004C61A1`).
    pub fn global_variables(&self) -> [bool; SCENARIO_GLOBAL_COUNT] {
        std::array::from_fn(|index| self.trigger_runtime.globals_set.contains(&(index as u32)))
    }

    /// Shared Scenario SetGlobal689670/SetLocal689910 mutation, including
    /// Tag timer resets. Trigger and Team scripts use this same owner.
    pub fn set_trigger_variable(&mut self, global: bool, index: i32, set: bool) -> bool {
        let count = if global { SCENARIO_GLOBAL_COUNT } else { 100 };
        if !(0..count as i32).contains(&index) {
            return false;
        }
        let values = if global {
            &mut self.trigger_runtime.globals_set
        } else {
            &mut self.trigger_runtime.locals_set
        };
        let previous = values.contains(&(index as u32));
        if set {
            values.insert(index as u32);
        } else {
            values.remove(&(index as u32));
        }
        if previous != set {
            self.trigger_runtime.dirty[0] = true;
            let events = if global { [27, 28] } else { [36, 37] };
            //689670/689910 -> Tag6E57F0/6E5820 ->7263A0/7263D0:
            // Tag registry order, then each reversed live chain, including
            // disabled instances. This differs from Enable53's global array.
            let mut tag_ordinal = 0;
            while tag_ordinal < self.trigger_runtime.tag_order.len() {
                let tag = self.trigger_runtime.tag_order[tag_ordinal];
                let chain = self.trigger_runtime.tag(tag).unwrap().triggers.clone();
                for live in chain {
                    let typ = self.trigger_runtime.trigger(live).unwrap().type_index;
                    if self.trigger_runtime.definitions.trigger_types[typ]
                        .events
                        .iter()
                        .any(|e| events.contains(&e.kind) && e.value == index)
                    {
                        self.reset_trigger_timers(live);
                    }
                }
                tag_ordinal += 1;
            }
        }
        previous
    }

    /// Logic55AFB0 reads the mutable list length and dirty bytes afresh for
    /// every Tag. A removed current Tag shifts the next entry, which is skipped
    /// until the next visit; an enabled later Tag can fire during this pass.
    pub fn advance_triggers(
        &mut self,
        inputs: TriggerInputs<'_>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Vec<TriggerEffect> {
        #[cfg(any(test, feature = "test-support"))]
        self.install_pending_trigger_fixture(inputs);
        let mut effects = Vec::new();
        self.trigger_runtime.logic_cursor = 0;
        while let Some(&tag) = self
            .trigger_runtime
            .logic_tags
            .get(self.trigger_runtime.logic_cursor as usize)
        {
            let mut fired = false;
            if self.trigger_runtime.dirty[3] {
                fired = self.dispatch_live_tag_event(
                    tag,
                    TagEventInput {
                        kind: 50,
                        actor: None,
                        source: None,
                        cell: (0, 0),
                        force: false,
                    },
                    &mut effects,
                    inputs.rules,
                    overlay_registry,
                    frame_effects,
                );
            }
            if !fired && self.trigger_runtime.dirty[0] {
                for event in [27, 28, 36, 37] {
                    if self.dispatch_live_tag_event(
                        tag,
                        TagEventInput {
                            kind: event,
                            actor: None,
                            source: None,
                            cell: (0, 0),
                            force: false,
                        },
                        &mut effects,
                        inputs.rules,
                        overlay_registry,
                        frame_effects,
                    ) {
                        fired = true;
                        break;
                    }
                }
            }
            if !fired && self.trigger_runtime.dirty[2] {
                for event in [45, 46] {
                    if self.dispatch_live_tag_event(
                        tag,
                        TagEventInput {
                            kind: event,
                            actor: None,
                            source: None,
                            cell: (0, 0),
                            force: false,
                        },
                        &mut effects,
                        inputs.rules,
                        overlay_registry,
                        frame_effects,
                    ) {
                        fired = true;
                        break;
                    }
                }
            }
            if !fired {
                fired = self.dispatch_live_tag_event(
                    tag,
                    TagEventInput {
                        kind: 13,
                        actor: None,
                        source: None,
                        cell: (0, 0),
                        force: false,
                    },
                    &mut effects,
                    inputs.rules,
                    overlay_registry,
                    frame_effects,
                );
            }
            if !fired {
                self.dispatch_live_tag_event(
                    tag,
                    TagEventInput {
                        kind: 51,
                        actor: None,
                        source: None,
                        cell: (0, 0),
                        force: false,
                    },
                    &mut effects,
                    inputs.rules,
                    overlay_registry,
                    frame_effects,
                );
            }
            // Scenario countdown14 expiry still belongs to its timer owner;
            // the campaign opening has no started countdown.
            self.trigger_runtime.logic_cursor = self.trigger_runtime.logic_cursor.wrapping_add(1);
        }
        self.trigger_runtime.dirty = [false; 4];
        effects
    }

    /// Shared country selection in6E0AA0/6E0B60/6E3120/6E3180..6E32A0.
    ///0x2325 (8997) invokes original726910's retained live+2C getter, not Spring's
    ///incoming authored House. The special/Country lookup has one world owner.
    /// Executed controls: trigger_all_change_house.json (Steam3e81a617..08600).
    fn live_action_house(
        &self,
        live: usize,
        value: i32,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Option<crate::sim::intern::InternedId> {
        if value == 0x2325 {
            self.trigger_runtime.trigger(live)?.retained_house
        } else if value == -1 {
            None
        } else {
            self.house_by_trigger_selector(value, rules)
        }
    }

    /// Action36/6E0B60..6E0C91, Steam3e81a617..08600. Two forward mutable
    /// Techno-array passes put Powered/PoweredSpecial Buildings last, then
    /// run their original4549B0 dispatcher even for a same-owner callback.
    /// No own RNG, timer or detach; ChangeOwner and operational consumers
    /// retain their lifecycle/side effects. No extra Alive/limbo filter here.
    fn trigger_change_all_house_objects(
        &mut self,
        owner: crate::sim::intern::InternedId,
        target: crate::sim::intern::InternedId,
        rules: &crate::rules::ruleset::RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let mut found = false;
        for powered_pass in [false, true] {
            let mut ordinal = 0;
            while ordinal < self.substrate.entities.techno_registry_len() {
                let id = self.substrate.entities.techno_registry_at(ordinal).unwrap();
                let eligible = self.substrate.entities.get(id).is_some_and(|entity| {
                    let powered = entity.category()
                        == crate::map::entities::EntityCategory::Structure
                        && self
                            .interner
                            .object_type(entity.type_ref(), rules)
                            .is_some_and(|object| object.powered || object.powered_special);
                    entity.owner() == owner && powered == powered_pass
                });
                if eligible {
                    self.change_owner_with_rules(id, target, rules, registry, effects);
                    if powered_pass {
                        self.dispatch_building_operational(id, rules, registry, effects);
                    }
                    found = true;
                }
                ordinal += 1;
            }
        }
        found
    }

    ///Tag6E5380: identity membership of a live Trigger in the Tag's chain.
    fn tag_contains_live_trigger(&self, attachment: TagAttachment, live: usize) -> bool {
        self.trigger_runtime
            .attachments
            .get(&attachment)
            .and_then(|&tag| self.trigger_runtime.tag(tag))
            .is_some_and(|tag| tag.triggers.contains(&live))
    }

    ///TActionCreateBuilding6E4200. Cell486840 runs BEFORE Type+8C45E880;
    ///130's corresponding lookup is after construction. Both callers use
    ///the unique458A80 foundation-clearing placement and ordinary lifecycle.
    fn trigger_create_building_at_waypoint(
        &mut self,
        owner: crate::sim::intern::InternedId,
        waypoint: i32,
        type_name: &str,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let Some(at) = self.session.waypoint_cell(waypoint) else {
            return false;
        };
        let coord = self.resolved_terrain.as_ref().and_then(|terrain| {
            let cells = NativeCellQuery::canonical(terrain);
            crate::sim::cell_kernel::native_cell_own_coords(cells.lookup(at), &cells).map(
                |(x, y, z)| crate::sim::components::DriveCoord {
                    x: x as i32,
                    y: y as i32,
                    z: z as i32,
                },
            )
        });
        let Some(coord) = coord else {
            return false;
        };
        let Some(object) = rules.object(type_name).filter(|object| {
            object.category == crate::rules::object_type::ObjectCategory::Building
        }) else {
            return false;
        };
        let type_id = object.id.clone();
        let owner_name = self.interner.resolve(owner).to_owned();
        let Some(created) = self.construct_object_limbo_at_height(
            &type_id,
            &owner_name,
            0,
            0,
            0,
            0,
            rules,
            frame_effects,
        ) else {
            return false;
        };
        if self
            .place_building_with_cleared_foundation(
                created,
                coord,
                0,
                rules,
                overlay_registry,
                frame_effects,
            )
            .is_none()
        {
            //6E42A5 repeats F8 even when458A80 has already rejected/UnInited.
            self.uninit_with_context(
                created,
                crate::sim::world::UninitContext::new(Some(rules), overlay_registry)
                    .with_effects(frame_effects),
            );
            return false;
        }
        self.grand_opening(
            created,
            false,
            false,
            rules,
            overlay_registry,
            frame_effects,
        );
        true
    }

    fn apply_live_trigger_action(
        &mut self,
        action: &RuntimeAction,
        context: TriggerActionContext,
        effects: &mut Vec<TriggerEffect>,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        #[cfg(any(test, feature = "test-support"))]
        if self.trigger_runtime.stop_before_action == Some(action.entry.kind) {
            std::panic::panic_any(action.entry.kind);
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trigger_runtime
            .action_trace
            .push((action.entry.kind, action.value.unwrap_or(0)));
        let reference = action.reference;
        let value = action.value;
        let sound = action.sound.as_deref();
        let action = &action.entry;
        match action.kind {
            6 => {
                // Steam3e81a617..08600, Execute6DEBF9..6DEC3B:
                // shared6E45E0 selects the House, then IHouse vt+38 is
                // AllToHunt501400. A NULL Trigger/House is rejected.
                // Native evidence: trigger_action_values --all-to-hunt.
                let (Some(live), Some(value), Some(rules)) = (context.live_index, value, rules)
                else {
                    return false;
                };
                let Some(owner) = self.live_action_house(live, value, rules) else {
                    return false;
                };
                crate::sim::house_strategy::all_to_hunt(
                    self,
                    rules,
                    owner,
                    overlay_registry,
                    frame_effects,
                );
            }
            74 | 75 => {
                // Steam3e81a617:6DF2D1/6DF310 -> existing6E45E0 selector,
                // then only House+1F2=1/0 at6DF2FA/6DF339. No immediate
                // timer, Team, sibling-latch or RNG change belongs here.
                // Native actions/consumer streams: --ai-activation corpus.
                let (Some(live), Some(value), Some(rules)) = (context.live_index, value, rules)
                else {
                    return false;
                };
                let Some(house) = self
                    .live_action_house(live, value, rules)
                    .and_then(|owner| self.houses.get_mut(&owner))
                else {
                    return false;
                };
                house.set_ai_triggers_active(action.kind == 74);
            }
            9 => {
                // Execute6DEAD6 -> shared House selector6E45E0;6DEAFF
                // writes only House+250=4. No immediate FireSale/Hunt and
                // no strategy timer reset. Those remain with house_strategy.
                // The shared6E45E0 selector rejects a null Trigger.
                // Steam3e81a617..08600, --no-factories-events Action9 controls.
                let (Some(live), Some(value), Some(rules)) = (context.live_index, value, rules)
                else {
                    return false;
                };
                let Some(house) = self
                    .live_action_house(live, value, rules)
                    .and_then(|owner| self.houses.get_mut(&owner))
                else {
                    return false;
                };
                house.strategy_emergency.set_state_four();
            }
            12 => {
                // Steam3e81a617..08600, Execute6DF051..6DF08F: reverse
                // live Trigger array, then726720's deferred marker. No
                // immediate detach, RNG or timer write belongs to this action.
                if let Some(DefinitionReference::Trigger(target)) = reference {
                    for ordinal in (0..self.trigger_runtime.trigger_order.len()).rev() {
                        let live = self.trigger_runtime.trigger_order[ordinal];
                        if self
                            .trigger_runtime
                            .trigger(live)
                            .is_some_and(|trigger| trigger.type_index == target)
                        {
                            self.mark_live_trigger_for_delete(live);
                        }
                    }
                }
            }
            70 => {
                // Execute6DF094 -> Tag vt20=6E58B0 ->6E4F60 destroys each
                // matching Tag immediately; linked Triggers remain deferred.
                // Revisit the shifted entry after every destructor. Native
                // captures the old count and can read stale slots past its
                // contracted array; safe storage stops at the live end.
                if let Some(DefinitionReference::Tag(target)) = reference {
                    let mut ordinal = 0;
                    while ordinal < self.trigger_runtime.tag_order.len() {
                        let live = self.trigger_runtime.tag_order[ordinal];
                        if self
                            .trigger_runtime
                            .tag(live)
                            .is_some_and(|tag| tag.type_index == target)
                        {
                            self.delete_live_tag(live, rules, 1, self.session.started());
                        } else {
                            ordinal += 1;
                        }
                    }
                }
            }
            38 => {
                // Execute6DE189..6DE1C8 resolves6E45E0 once, then calls
                // House4F9F90(source,target,0), followed by the reverse call.
                // The diplomacy owner retains anger, fog and threat updates.
                let (Some(live), Some(source), Some(value), Some(rules)) =
                    (context.live_index, context.incoming_house, value, rules)
                else {
                    return false;
                };
                let Some(target) = self.live_action_house(live, value, rules) else {
                    return false;
                };
                self.make_enemy(source, target, rules);
                self.make_enemy(target, source, rules);
            }
            23..=27 => {
                use crate::sim::scenario_session::ScenarioTimerAction;
                let seconds = value.unwrap_or(0);
                self.session.change_scenario_timer(match action.kind {
                    23 => ScenarioTimerAction::Start,
                    24 => ScenarioTimerAction::Stop,
                    25 => ScenarioTimerAction::Extend(seconds),
                    26 => ScenarioTimerAction::Shorten(seconds),
                    27 => ScenarioTimerAction::Set(seconds),
                    _ => unreachable!(),
                });
            }
            103 => self.session.set_scenario_timer_caption(action.csf_label()),
            20 => {
                // Steam3e81a617..08600: Execute6DE8E4..6DE903 passes +90
                // to Theme720B20 and returns AL1, even when audio is gated.
                // Type8 is an untrimmed section key, not Sound= or an ordinal.
                // The process catalog is immutable, so its existing owner can
                // resolve the retained name at delivery; numeric sentinels
                // pass through unchanged. No scenario RNG/timer/detach here.
                let request = if action.params.first().map_or(0, |v| crt_atoi(v)) == 8 {
                    ThemeRequest::Name(action.params.get(1).cloned().unwrap_or_default())
                } else {
                    ThemeRequest::Index(value.unwrap_or(-1))
                };
                effects.push(TriggerEffect::MusicTheme { request });
            }
            0 => {
                //Unsigned kind-1 falls through6DFDDD with the Execute
                //entry's AL1. ALL01 has nine authored no-op tail records.
            }
            3 => {
                //6DEA97 ->6E45E0 writes only selected House+1EE. The
                //retained live selector owns8997 and Country/Player lookup.
                let (Some(live), Some(rules), Some(value)) = (context.live_index, rules, value)
                else {
                    return false;
                };
                let Some(owner) = self.live_action_house(live, value, rules) else {
                    return false;
                };
                if let Some(house) = self.houses.get_mut(&owner) {
                    house.enable_scripted_production();
                }
            }
            10 => {
                //6DE72F..6DE76C brackets Movie5BF260(index,-1,1,1,1)
                //with the existing input/display/audio service boundaries.
                //Movie admission belongs to that shared app owner; Execute
                //still returns AL1 even for an absent movie name.
                effects.push(TriggerEffect::FullscreenMovie {
                    index: value.unwrap_or(0),
                });
            }
            51 => {
                //6DED66 calls the incoming House's577C90 map transition,
                //then returns AL1 independently of the void service result.
                let Some(rules) = rules else { return false };
                self.trigger_reshroud_map(context.incoming_house, rules);
            }
            113 => {
                //6DF848 selects +90 through the retained live House selector,
                //then House50C8C0 captures the initial Techno count and visits
                //the live array forward. No alive/limbo/health gate precedes
                //virtual388(0); the Infantry owner decides CanCheer.
                let (Some(live), Some(rules), Some(value)) = (context.live_index, rules, value)
                else {
                    return false;
                };
                let Some(owner) = self.live_action_house(live, value, rules) else {
                    return false;
                };
                self.house_cheer(owner, rules, frame_effects);
            }
            60 => {
                //6DDEF5 scans the live Building registry forward and calls
                //its unique447110 SellBack(1) owner for tagged candidates.
                //A matched candidate makes AL1 even when SellBack refuses.
                let (Some(live), Some(rules)) = (context.live_index, rules) else {
                    return false;
                };
                let mut matched = false;
                let mut ordinal = 0;
                while ordinal < self.substrate.entities.techno_registry_len() {
                    let id = self.substrate.entities.techno_registry_at(ordinal).unwrap();
                    let eligible = self.substrate.entities.get(id).is_some_and(|entity| {
                        entity.category() == crate::map::entities::EntityCategory::Structure
                            && entity.lifecycle.object_alive()
                            && entity.lifecycle.cell_marked()
                            && !entity.lifecycle.in_limbo()
                    });
                    if eligible && self.tag_contains_live_trigger(TagAttachment::Object(id), live) {
                        crate::sim::production::sell_back(
                            self,
                            rules,
                            id,
                            crate::sim::production::SellOrder::Computer,
                            overlay_registry,
                            frame_effects,
                        );
                        matched = true;
                    }
                    ordinal += 1;
                }
                return matched;
            }
            81..=83 => {
                use crate::map::entities::EntityCategory;
                use crate::sim::mission::{MissionId, MissionType};

                // Steam3e81a617..08600: Execute6DDB67 ->6E01C0 (81),
                //6DDBF6 (82),6DDC71 (83). Original execution, including
                //Tag6E5380/GetMission5B3040/Queue5B35E0/41BA90:
                //tools/spatial_oracle/trigger_action_values.py --wakeup.
                //81 walks Techno by live Trigger identity and deliberately
                //does NOT test Health or Limbo. 82/83 walk Foot and compare
                //the incoming House, not the scalar or retained House.
                let mut ordinal = 0;
                while let Some(id) = if action.kind == 81 {
                    self.substrate.entities.techno_registry_at(ordinal)
                } else {
                    self.substrate.entities.foot_registry_at(ordinal)
                } {
                    let eligible = self.substrate.entities.get(id).is_some_and(|entity| {
                        let mission = entity.mission.effective().known();
                        entity.lifecycle.object_alive()
                            && entity.lifecycle.cell_marked()
                            && if action.kind == 81 {
                                entity.category() != EntityCategory::Structure
                                    && matches!(
                                        mission,
                                        Some(MissionType::Sleep | MissionType::Harmless)
                                    )
                                    && context.live_index.is_some_and(|live| {
                                        self.tag_contains_live_trigger(
                                            TagAttachment::Object(id),
                                            live,
                                        )
                                    })
                            } else {
                                entity.health.current() > 0
                                    && !entity.lifecycle.in_limbo()
                                    && if action.kind == 82 {
                                        mission == Some(MissionType::Sleep)
                                            && Some(entity.owner()) != context.incoming_house
                                    } else {
                                        mission == Some(MissionType::Harmless)
                                            && Some(entity.owner()) == context.incoming_house
                                    }
                            }
                    });
                    if eligible && let Some(entity) = self.substrate.entities.get_mut(id) {
                        //Every admitted call is Queue(Guard,0): no readiness,
                        //Commence, timer write, RNG draw or detach. The normal
                        //AI promotion remains with Mission's existing owner.
                        crate::sim::mission::authority::queue_entity_mission_deferred(
                            entity,
                            MissionId::from_known(MissionType::Guard),
                        );
                    }
                    ordinal += 1;
                }
                //Original AL=1 even for an empty registry or no matches.
            }
            32 => {
                //6DEC3B ->6E2050 uses live Tag membership, not owner/name.
                //Repeated forward scans re-read the dynamic registry after
                //every direct C4 receiver, including nested trigger effects.
                let (Some(live), Some(rules)) = (context.live_index, rules) else {
                    return false;
                };
                let Some(c4_name) = rules
                    .warhead(&rules.bridge_warheads.c4_name)
                    .map(|warhead| warhead.id.as_str())
                else {
                    return false;
                };
                let c4 = self.interner.intern(c4_name);
                let mut matched = false;
                loop {
                    let mut visited_match = false;
                    let mut ordinal = 0;
                    while ordinal < self.substrate.entities.techno_registry_len() {
                        let id = self.substrate.entities.techno_registry_at(ordinal).unwrap();
                        let health = self.substrate.entities.get(id).and_then(|entity| {
                            (entity.health.current() > 0
                                && entity.lifecycle.object_alive()
                                && entity.lifecycle.cell_marked()
                                && !entity.lifecycle.in_limbo())
                            .then_some(entity.health.current())
                        });
                        let Some(health) = health.filter(|_| {
                            self.tag_contains_live_trigger(TagAttachment::Object(id), live)
                        }) else {
                            ordinal += 1;
                            continue;
                        };
                        let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
                            id,
                            health,
                            0,
                            crate::sim::combat::RAD_NO_ATTACKER,
                            None,
                            c4,
                            crate::sim::combat::ReceiverCallFlags {
                                ignore_defenses: true,
                                arg6: false,
                            },
                        );
                        self.commit_direct_damage_receiver(
                            rules,
                            overlay_registry,
                            event,
                            frame_effects,
                        );
                        matched = true;
                        visited_match = true;
                        //6E20DF decrements the slot before the loop increment:
                        //a removed object exposes its successor at this slot.
                    }
                    if !visited_match {
                        break;
                    }
                }
                //Static initializer6DCF10 makes B0E680==(0,0). For a real
                //incoming Cell, the same587180 owner as Action63 retries at
                //most four times. No authored waypoint substitutes for Cell.
                if context.input.cell != (0, 0) && self.resolved_terrain.is_some() {
                    let mut driver =
                        crate::sim::world::bridge_orchestrator::BridgeDamageDrivers::new(
                            self,
                            rules,
                            overlay_registry,
                            frame_effects,
                        );
                    for _ in 0..4 {
                        if driver.apply_damage_to_cell(context.input.cell) {
                            break;
                        }
                    }
                    if driver.collapsed() {
                        effects.push(TriggerEffect::BridgeStateChanged);
                    }
                    //6E213D..6E21AF invalidates a256px rectangle; the app's
                    //normal draw rebuilds the whole tactical scene each frame.
                }
                return matched;
            }
            116 => {
                //6E1980 clears the first Building/Terrain's custom handle;
                //otherwise750F40 detaches matching coordinate requests,flags1.
                return self.trigger_stop_sound_at_waypoint(
                    action.waypoint_index,
                    rules,
                    frame_effects,
                );
            }
            118 => {
                //6E2C20 ->Map588AC0 clears smudge identities, preserving
                //their data bytes;4F42F0(1) then invalidates tactical drawing.
                let Some(rules) = rules else { return false };
                //Native6E2C20 returns AL1 after its void map/draw calls.
                //The owner's safe unavailable-map/type result is a Rust
                //boundary, not a reason to retry an admitted action.
                self.trigger_clear_all_smudges(rules);
            }
            125 => {
                let (Some(owner), Some(rules), Some(waypoint)) =
                    (context.incoming_house, rules, action.waypoint_index)
                else {
                    return false;
                };
                return self.trigger_create_building_at_waypoint(
                    owner,
                    waypoint as i32,
                    action.inline_type_name(),
                    rules,
                    overlay_registry,
                    frame_effects,
                );
            }
            111 => {
                //Execute6DF717 scans Building constructors in forward order;
                //first live+marked+nonlimbo match gets SellBuilding457DE0(0,0).
                let (Some(live), Some(rules)) = (context.live_index, rules) else {
                    return false;
                };
                let mut ordinal = 0;
                while ordinal < self.substrate.entities.techno_registry_len() {
                    let id = self.substrate.entities.techno_registry_at(ordinal).unwrap();
                    let eligible = self.substrate.entities.get(id).is_some_and(|e| {
                        e.category() == crate::map::entities::EntityCategory::Structure
                            && e.lifecycle.object_alive()
                            && e.lifecycle.cell_marked()
                            && !e.lifecycle.in_limbo()
                    });
                    if eligible && self.tag_contains_live_trigger(TagAttachment::Object(id), live) {
                        crate::sim::passenger::garrison_eject::sell_building_occupants(
                            self,
                            rules,
                            overlay_registry,
                            id,
                            crate::sim::passenger::garrison_eject::GarrisonEjectMode::Release,
                            frame_effects,
                        );
                        return true;
                    }
                    ordinal += 1;
                }
                return false;
            }
            130 => {
                //6E3120 uses the same live-trigger country selector as
                //119..122;50D320 consumes the House's retained history.
                let (Some(live), Some(rules), Some(value)) = (context.live_index, rules, value)
                else {
                    return false;
                };
                let owner = self.live_action_house(live, value, rules);
                let Some(owner) = owner else {
                    return false;
                };
                self.restore_initial_house_buildings(owner, rules, overlay_registry, frame_effects);
            }
            119..=122 => {
                //6E3180/31E0/3240/32A0 require a live Trigger context.
                //8997 resolves its retained House via726910; other values
                //use the existing510F60/510ED0/502D30 selector owner.
                let (Some(live), Some(rules), Some(value)) = (context.live_index, rules, value)
                else {
                    return false;
                };
                let owner = self.live_action_house(live, value, rules);
                let Some(owner) = owner else {
                    return false;
                };
                use crate::sim::world::HouseBlowupSubset;
                let subset = match action.kind {
                    119 => HouseBlowupSubset::All,
                    120 => HouseBlowupSubset::Buildings,
                    121 => HouseBlowupSubset::Land,
                    122 => HouseBlowupSubset::Naval,
                    _ => unreachable!(),
                };
                self.house_blowup(owner, subset, rules, overlay_registry, frame_effects);
            }
            36 => {
                let (Some(live), Some(owner), Some(value), Some(rules)) =
                    (context.live_index, context.incoming_house, value, rules)
                else {
                    return false;
                };
                let Some(target) = self.live_action_house(live, value, rules) else {
                    return false;
                };
                return self.trigger_change_all_house_objects(
                    owner,
                    target,
                    rules,
                    overlay_registry,
                    frame_effects,
                );
            }
            14 => {
                //6E0AA0 ->6E0B40 dynamic Techno array;8997 is only726910's
                //live+2C. Never substitute the authored incoming House.
                let (Some(live_index), Some(rules), Some(value)) =
                    (context.live_index, rules, value)
                else {
                    return false;
                };
                let target = self.live_action_house(live_index, value, rules);
                let Some(target) = target else {
                    return false;
                };
                let mut found = false;
                let mut ordinal = 0;
                while ordinal < self.substrate.entities.techno_registry_len() {
                    let id = self.substrate.entities.techno_registry_at(ordinal).unwrap();
                    let eligible = self.substrate.entities.get(id).is_some_and(|entity| {
                        entity.lifecycle.object_alive()
                            && entity.lifecycle.cell_marked()
                            && !entity.lifecycle.in_limbo()
                    });
                    let attached =
                        self.tag_contains_live_trigger(TagAttachment::Object(id), live_index);
                    if eligible && attached {
                        //Native announce=false; same-owner early return still
                        //counts as a matched candidate. Shared owner updates
                        //House counts, power, lists, discovery and cleanup.
                        self.change_owner_with_rules(
                            id,
                            target,
                            rules,
                            overlay_registry,
                            frame_effects,
                        );
                        found = true;
                    }
                    ordinal += 1;
                }
                return found;
            }
            16 => {
                return self.trigger_reveal_entire_map(
                    crate::sim::world::UninitContext::new(rules, overlay_registry)
                        .with_effects(frame_effects),
                );
            }
            5 => {
                let (Some(DefinitionReference::Team(team)), Some(rules)) = (reference, rules)
                else {
                    return false;
                };
                // Execute6DEB8D..6DEBAB, TeamType6F0A70. The original
                // removes matching teams, releasing rather than killing members.
                let type_id = self
                    .interner
                    .intern(&self.trigger_runtime.definitions.team_types[team]);
                self.destroy_teams_of_type(type_id, rules, frame_effects);
            }
            104 => {
                //6DF562 ->6F1F30 ->6EF5F0: signed+90 writes each member's
                //flash counter, filtering only TeamType identity. A null
                //Team reference still returns AL1 in the original.
                if let Some(DefinitionReference::Team(team)) = reference {
                    let team_id = self
                        .interner
                        .intern(&self.trigger_runtime.definitions.team_types[team]);
                    self.flash_teams_of_type(team_id, value.unwrap_or(0));
                }
            }
            4 | 7 | 80 | 107 => {
                let Some(DefinitionReference::Team(team)) = reference else {
                    return false;
                };
                let Some(rules) = rules else {
                    return false;
                };
                let team_id = self
                    .interner
                    .intern(&self.trigger_runtime.definitions.team_types[team]);
                if action.kind == 4 {
                    return self
                        .trigger_create_team(team_id, rules, overlay_registry, frame_effects)
                        .is_some();
                }
                // Reinforcement6E2DC0 uses the TeamType waypoint. Action80
                //6DEBD0 passes its+44 override to the same65D8E0 owner;
                //null Team or waypoint-1 is rejected before that call.
                let waypoint = if matches!(action.kind, 80 | 107) {
                    let Some(waypoint) = action.waypoint_index else {
                        return false;
                    };
                    waypoint as i32
                } else {
                    -1
                };
                if matches!(action.kind, 80 | 107) && waypoint == -1 {
                    return false;
                }
                //1076DF672..6DF698 shares the same resolved Team/+44 gate,
                //then calls chrono65EC30 instead of ordinary65D8E0. Only -1
                //is rejected, not every signed negative (original execution).
                let (result, recent_event) = if action.kind == 107 {
                    self.spawn_chrono_team_reinforcements(
                        team_id,
                        waypoint,
                        rules,
                        overlay_registry,
                        frame_effects,
                    )
                } else {
                    self.spawn_team_reinforcements(
                        team_id,
                        waypoint,
                        rules,
                        overlay_registry,
                        frame_effects,
                    )
                };
                if let Some(cell) = recent_event {
                    effects.push(TriggerEffect::RecentEventCell { cell });
                }
                return result;
            }
            11 => {
                // Execute6DE0A8 skips an empty inline label. CSF resolution
                // and MessageList5D3BA0 remain ordered presentation consumers.
                let label = action.csf_label();
                if !label.is_empty() {
                    effects.push(TriggerEffect::LocalizedMessage {
                        csf_label: label.to_string(),
                    });
                }
            }
            17 => {
                let (Some(rules), Some(waypoint)) = (rules, value) else {
                    return false;
                };
                return self.trigger_reveal_at_waypoint(
                    waypoint,
                    crate::sim::world::UninitContext::new(Some(rules), overlay_registry)
                        .with_effects(frame_effects),
                );
            }
            108 => {
                //Steam3e81a617: active Execute6DF69B..6DF6CD ->68BCC0
                //->56BEC0. Unset valid waypoints supply(0,0) to FNPC; the
                //Place AL is returned. Compare the full scalar with20 before
                //the fixed choice's low-byte store, including276 -> byte20.
                //Original execution: trigger_action_values.py --crate.
                let (Some(rules), Some(registry), Some(value), Some(waypoint)) =
                    (rules, overlay_registry, value, action.waypoint_index)
                else {
                    return false;
                };
                let Some(cell) = self.session.waypoint_cell(waypoint as i32) else {
                    return false;
                };
                return crate::sim::crates::place_crate(
                    self,
                    rules,
                    registry,
                    cell,
                    (value != 20).then_some(value as u8),
                );
            }
            101 => {
                let (Some(rules), Some(waypoint)) = (rules, value) else {
                    return false;
                };
                return self.trigger_conceal_at_waypoint(
                    waypoint,
                    crate::sim::world::UninitContext::new(Some(rules), overlay_registry)
                        .with_effects(frame_effects),
                );
            }
            18 => {
                let (Some(rules), Some(waypoint)) = (rules, value) else {
                    return false;
                };
                return self.trigger_reveal_zone_at_waypoint(
                    waypoint,
                    crate::sim::world::UninitContext::new(Some(rules), overlay_registry)
                        .with_effects(frame_effects),
                );
            }
            ACTION_PLAY_SOUND => {
                // Execute6DD8B0 case19 (6DE780..6DE791): Voc PlayAtPos750920
                // of +90, centre pan2000, volume1.0, no handle. -1 returns at
                //75093B.
                if let Some(sound_id) = sound {
                    self.sound_events.push(SimSoundEvent::VocCentered {
                        sound_id: sound_id.to_owned(),
                    });
                    frame_effects.flush_sound(self, rules);
                }
            }
            55 => {
                // Execute6DF1AD: +44 waypoint, +90 native radar-event type.
                // Client65FA70 owns admission/dedup; no House filter here.
                let Some(cell) = action
                    .waypoint_index
                    .and_then(|waypoint| self.session.waypoint_cell(waypoint as i32))
                else {
                    return false;
                };
                if let Ok(kind) = crate::sim::radar::RadarEventType::try_from(value.unwrap_or(0)) {
                    self.sound_events.push(SimSoundEvent::RadarEvent {
                        radar: crate::sim::radar::RadarEventRequest::new(
                            kind,
                            cell.0 as u16,
                            cell.1 as u16,
                        ),
                    });
                    frame_effects.flush_sound(self, rules);
                }
            }
            100 | 117 => {
                // Execute6DF546/6DF90E -> radar movie5BFF50. Preparation,
                // the game-mode gate and missing-media handling are app-owned;
                // the action returns success independently of queue admission.
                effects.push(TriggerEffect::RadarMovie {
                    index: value.unwrap_or(0),
                    pause_simulation: action.kind == 117,
                });
            }
            114 => {
                let index = value.unwrap_or(0);
                if !(0..4).contains(&index) {
                    return false;
                }
                effects.push(TriggerEffect::SelectSidebarTab { index });
                //6E4100 ->6ABAC0/6A7590: presentation checks retained strip
                //membership. Native's empty-strip AL0 is only a client
                //receipt: all three Tag callers6E5439/6E5450/6E546D ignore
                //Spring's AL, as does ForceTrigger. Keep that client result
                //out of deterministic simulation; the UI owns actual admission.
            }
            115 => {
                let Some(rules) = rules else { return false };
                //6E4150 scans the common TechnoType array A8EB04 in reverse,
                //with byte-exact names, then6AC5F0 dispatches its own class.
                //Retail SOV01 uses the unique SAPC identity; duplicate names
                //across type classes retain the rules owner's existing order.
                let Some(object) = rules
                    .all_objects()
                    .filter(|object| object.id == action.inline_type_name())
                    .last()
                else {
                    return false;
                };
                let type_id = self.interner.intern(&object.id);
                effects.push(TriggerEffect::FlashSidebarCameo {
                    type_id,
                    duration: value.unwrap_or(0),
                    frame: self.session.binary_frame as i32,
                });
            }
            139 => {
                // Execute6DFCE3 -> TAction6E3B40 uses the incoming House,
                // not Action.value as a selector. House50BCD0 owns the raw
                // frame-duration timer and5779 dirty flag; reuse that port.
                // Native Steam3e81a617..08600; timer/availability goldens:
                // tools/superweapon_oracle.json::radar_outage.
                let Some(owner) = context.incoming_house else {
                    return false;
                };
                self.power_states
                    .entry(owner)
                    .or_default()
                    .start_radar_outage(self.session.binary_frame, value.unwrap_or(0));
            }
            128 => {
                //Execute6DFAD9 ->6E1A40 ->House50D6D0. Only +44 waypoint
                //and incoming House are read: the numeric +90 is ignored.
                let (Some(owner), Some(waypoint), Some(rules)) =
                    (context.incoming_house, action.waypoint_index, rules)
                else {
                    return false;
                };
                let Some(cell) = self.session.waypoint_cell(waypoint as i32) else {
                    return false;
                };
                return self
                    .chronoshift_house_to(owner, cell, rules, overlay_registry, frame_effects)
                    .map_or_else(
                        |error| {
                            log::debug!("Action128: {error}");
                            false
                        },
                        |_| true,
                    );
            }
            127 => {
                // Execute6DFABF -> ChronoScreen53B460. A nonpositive duration
                // is a no-op there; the action still returns success.
                self.start_chrono_screen_effect(value.unwrap_or(0), effects);
            }
            131 => {
                // FlashBuildings6E4560..6E45DE: incoming House's live
                // Building list, reverse order, exact inline type ID; raw
                // Techno+F0 write, without Building::Flash's disguise gate.
                let Some(owner) = context.incoming_house else {
                    return false;
                };
                let ids: Vec<_> = self
                    .houses
                    .get(&owner)
                    .map(|house| {
                        house
                            .base_projection
                            .buildings()
                            .iter()
                            .rev()
                            .copied()
                            .collect()
                    })
                    .unwrap_or_default();
                for id in ids {
                    let matches = self.substrate.entities.get(id).is_some_and(|entity| {
                        self.resolve(entity.type_ref()) == action.inline_type_name()
                    });
                    if matches && let Some(entity) = self.substrate.entities.get_mut(id) {
                        entity.flashing.start(value.unwrap_or(0));
                    }
                }
            }
            21 => {
                let Some(index) = value else {
                    return false;
                };
                self.sound_events.push(SimSoundEvent::TriggerEva { index });
                frame_effects.flush_sound(self, rules);
            }
            59 => {
                let (Some(rules), Some(registry), Some(waypoint)) =
                    (rules, overlay_registry, value)
                else {
                    return false;
                };
                return self.trigger_reduce_tiberium_at_waypoint(waypoint, rules, registry);
            }
            86 => {
                let Some(value) = value else {
                    return false;
                };
                self.production
                    .ore_growth_config
                    .set_enabled_from_trigger(value);
            }
            88 => {
                let (Some(rules), Some(index), Some(waypoint)) =
                    (rules, value, action.waypoint_index)
                else {
                    return false;
                };
                return self.trigger_create_particle_system_at_waypoint(
                    index,
                    waypoint as i32,
                    rules,
                );
            }
            89 => {
                let Some(waypoint) = action.waypoint_index else {
                    return false;
                };
                return self.trigger_mark_particle_systems_done_at_waypoint(waypoint as i32);
            }
            30 => {
                let (Some(owner), Some(value), Some(rules)) =
                    (context.incoming_house, value, rules)
                else {
                    return false;
                };
                return self.trigger_auto_base_building(
                    owner,
                    value,
                    rules,
                    overlay_registry,
                    frame_effects,
                );
            }
            76 => {
                // 0x006DF34F: a null incoming House returns success without even
                // reading +0x90. Otherwise only the existing +0x565C owner changes.
                let Some(owner) = context.incoming_house else {
                    return true;
                };
                let Some(value) = value else { return false };
                let Some(house) = self.houses.get_mut(&owner) else {
                    return false;
                };
                house.team_creation.set_trigger_ratio(value);
            }
            8 => {
                let (Some(rules), Some(waypoint)) = (rules, action.waypoint_index) else {
                    return false;
                };
                return self.trigger_drop_zone_flare(rules, waypoint as i32);
            }
            98 => {
                return self.trigger_play_sound_at_random_waypoint(sound, rules, frame_effects);
            }
            41 => {
                let (Some(rules), Some(index), Some(waypoint)) =
                    (rules, value, action.waypoint_index)
                else {
                    return false;
                };
                return self.trigger_play_anim_at_waypoint(rules, index, waypoint as i32);
            }
            42 => {
                let (Some(rules), Some(index), Some(waypoint)) =
                    (rules, value, action.waypoint_index)
                else {
                    return false;
                };
                let Some(changed) = self.trigger_explosion_at_waypoint(
                    rules,
                    index,
                    waypoint as i32,
                    overlay_registry,
                    frame_effects,
                ) else {
                    return false;
                };
                if changed {
                    effects.push(TriggerEffect::BridgeStateChanged);
                }
            }
            95 => {
                let (Some(rules), Some(waypoint)) = (rules, action.waypoint_index) else {
                    return false;
                };
                return self.trigger_nuke_strike(rules, waypoint as i32);
            }
            46 | 47 => {
                let locked = action.kind == 46;
                let was_locked = self.session.input_locked();
                self.session.set_input_locked(locked);
                effects.push(TriggerEffect::InputLockChanged { locked, was_locked });
            }
            ACTION_FORCE_TRIGGER | ACTION_ENABLE_TRIGGER | ACTION_DISABLE_TRIGGER => {
                if let Some(DefinitionReference::Trigger(target)) = reference {
                    let difficulty = self.trigger_difficulty();
                    let mut ordinal = 0;
                    while ordinal < self.trigger_runtime.trigger_order.len() {
                        let live = self.trigger_runtime.trigger_order[ordinal];
                        if self
                            .trigger_runtime
                            .trigger(live)
                            .is_some_and(|t| t.type_index == target)
                        {
                            match action.kind {
                                ACTION_FORCE_TRIGGER => {
                                    self.spring_live_trigger(
                                        live,
                                        context.input,
                                        effects,
                                        rules,
                                        overlay_registry,
                                        frame_effects,
                                    );
                                }
                                ACTION_ENABLE_TRIGGER => {
                                    if self.trigger_runtime.definitions.trigger_types[target]
                                        .admits_difficulty(difficulty)
                                    {
                                        self.trigger_runtime.triggers[live]
                                            .as_mut()
                                            .unwrap()
                                            .enabled = true;
                                        self.reset_trigger_timers(live);
                                    }
                                }
                                ACTION_DISABLE_TRIGGER => {
                                    self.trigger_runtime.triggers[live]
                                        .as_mut()
                                        .unwrap()
                                        .enabled = false;
                                }
                                _ => unreachable!(),
                            }
                        }
                        ordinal += 1;
                    }
                }
            }
            ACTION_SET_GLOBAL | ACTION_CLEAR_GLOBAL | ACTION_SET_LOCAL | ACTION_CLEAR_LOCAL => {
                let global = matches!(action.kind, ACTION_SET_GLOBAL | ACTION_CLEAR_GLOBAL);
                if let Some(index) = value {
                    self.set_trigger_variable(
                        global,
                        index,
                        matches!(action.kind, ACTION_SET_GLOBAL | ACTION_SET_LOCAL),
                    );
                }
            }
            142..=144 => {
                let Some(value) = value else {
                    return false;
                };
                // Original Execute6DFD4F/7C/A9 ->6E2E60/6E2EB0/6E2F00:
                // no House/actor gate;53AD00 uses normal-cache flag0, then
                // the raw normal component is stored. The published snapshot
                // includes that store: Cell refresh reads ambient/Ground/Level,
                // not RGB, and the explicit palette operation preserves order.
                let rgb = self
                    .session
                    .lighting
                    .retint_normal_component((action.kind - 142) as usize, value);
                self.publish_global_lighting(crate::sim::light_sources::PaletteUpdate::Normal(rgb));
            }
            73 => {
                let Some(value) = value else {
                    return false;
                };
                let storm = crate::sim::superweapon::lightning_storm::raging(self);
                self.session.lighting.set_ambient_light(value, storm);
            }
            64..=66 => {
                let (Some(rules), Some(waypoint)) = (rules, value) else {
                    return false;
                };
                // Execute64/65/66 at6DDDF6/6DDE4B/6DDEA0 uses +90,
                // not the +44 tail waypoint, with damage50/100/300.
                // The same C4 reference/coordinate/Flash owners serve ordinary
                // impacts; stock C4=Super is not Bright, so materializes no
                // light. Do not force a screen/nuke flash from the action name.
                // Native: trigger_map_lighting.json flash_controls (+meta).
                let Some([x, y, z]) = self.trigger_waypoint_location(waypoint) else {
                    return false;
                };
                let warhead_ref = self.interner.intern(&rules.bridge_warheads.c4_name);
                self.combat_light_requests
                    .push(crate::sim::combat::CombatLightRequest::Impact {
                        target_id: None,
                        damage: match action.kind {
                            64 => 50,
                            65 => 100,
                            _ => 300,
                        },
                        warhead_ref,
                        coord: crate::sim::projectile::ProjectileCoord::new(x, y, z),
                        force_create: false,
                        flags: 0,
                    });
            }
            71 | 72 => {
                let Some(bits) = value else {
                    return false;
                };
                self.session
                    .lighting
                    .set_ambient_change_input(action.kind == 72, bits);
            }
            ACTION_DAMAGE_AT_WAYPOINT => {
                let (Some(rules), Some(waypoint)) = (rules, value) else {
                    return false;
                };
                let Some(changed) = crate::sim::world::bridge_orchestrator::damage_at_waypoint(
                    self,
                    rules,
                    waypoint,
                    overlay_registry,
                    frame_effects,
                ) else {
                    return false;
                };
                if changed {
                    effects.push(TriggerEffect::BridgeStateChanged);
                }
            }
            ACTION_CHANGE_VISIBLE_MAP_AREA => {
                // `TActionClass::Read @ 0x006DD5B0` parses ActionID,
                // ParamType, Param3, then stores the four writer dwords at
                // +0x34..+0x40. ActionEntry.params is chunk[1..], so these are
                // exactly indices 2..5; params[0]/[1] must never leak in.
                if let Some(raw_local_size) = parse_visible_map_area(&action.params) {
                    let _ = self.change_visible_map_area(raw_local_size, rules);
                }
            }
            ACTION_CENTER_CAMERA | ACTION_JUMP_CAMERA => {
                // Execute6DD8B0 resolves GetWaypoint68BCC0 and native Cell
                // ground/bridge state before FocusOn/CenterMap. Presentation
                // receives the immutable coordinate; the shared simulation
                // query also retains any process CellClass Dummy writes.
                let Some(coord) = action
                    .waypoint_index
                    .and_then(|index| self.trigger_waypoint_coord(index as i32))
                else {
                    return false;
                };
                let velocity = if action.kind == ACTION_CENTER_CAMERA {
                    let Some(velocity) = value else { return false };
                    Some(velocity)
                } else {
                    None
                };
                effects.push(TriggerEffect::CameraAtCoord { coord, velocity });
            }
            ACTION_PLAY_SOUND_AT_WAYPOINT => {
                return self.trigger_play_sound_at_waypoint(
                    sound,
                    action.waypoint_index,
                    rules,
                    frame_effects,
                );
            }
            106 => {
                // Active inline Execute6DF5D1, not the similarly annotated
                // separate helper6E37F0. Type9's string is+54; raw value is+90.
                let (Some(rules), Some(value)) = (rules, value) else {
                    return false;
                };
                self.set_object_type_tech_level(action.inline_type_name(), value, rules);
            }
            ACTION_SET_SUPER_CHARGE_PERCENTAGE | 132..=134 => {
                // Execute1296E42D0,1326E4320,1336E4360 and1346E43A0 select
                // the incoming House's Super-array ordinal (+90). Only129
                // has grant/percentage gates.132 writes +48 as the override;
                //133 clears it without restarting the current countdown;
                //134 calls the same Reset owner as spy infiltration.
                // Original Steam identity and executed admissions/numerics:
                // tools/spatial_oracle/trigger_super_charge_percent.meta.json
                // trigger_superweapon_state.meta.json and
                // trigger_superweapon_reset.meta.json.
                let Some(rules) = rules else {
                    return false;
                };
                let Some(owner) = context.incoming_house else {
                    return false;
                };
                let Some(index) = value.and_then(|value| usize::try_from(value).ok()) else {
                    return false;
                };
                // Native's index==count out-of-array access is excluded; the
                // retained Rust registry admits only actual stock entries.
                let Some(name) = rules.super_weapon_order.get(index) else {
                    return false;
                };
                let Some(type_id) = self.interner.get(name) else {
                    return false;
                };
                let Some(instance) = self
                    .super_weapons
                    .get_mut(&owner)
                    .and_then(|weapons| weapons.get_mut(&type_id))
                else {
                    return false;
                };
                match action.kind {
                    132 => instance.set_recharge_time(action.auxiliary_value()),
                    133 => instance.reset_recharge_time(),
                    134 => crate::sim::superweapon::reset_super(self, rules, owner, name),
                    _ => {
                        let Some(sw) = rules.super_weapon(name) else {
                            return false;
                        };
                        return instance.set_charge_percentage(
                            action.auxiliary_value(),
                            sw,
                            self.session.binary_frame,
                        );
                    }
                }
            }
            135 | 136 | 140 | 141 => {
                // Execute1356E43E0/1366E4440 and1406E4460/1416E44C0 use
                // Spring's incoming House, ignoring scalar+90. Setters135/140
                // read waypoint+44 and reject the empty cell; clears ignore it.
                // 140 calls House50DA20: cell+54F4 and current frame+54FC.
                // 141 calls50DA50: clear cell and restore frame-100.
                // 135/136 controls: trigger_superweapon_state.meta.json.
                // 140/141: original Steam gamemd3e81a617..08600 instructions.
                let Some(owner) = context.incoming_house else {
                    return false;
                };
                let target = if matches!(action.kind, 135 | 140) {
                    let Some(cell) = action
                        .waypoint_index
                        .and_then(|index| self.session.waypoint_cell(index as i32))
                        .filter(|&cell| cell != (0, 0))
                    else {
                        return false;
                    };
                    Some((cell.0 as u16, cell.1 as u16))
                } else {
                    None
                };
                let Some(house) = self.houses.get_mut(&owner) else {
                    return false;
                };
                if let Some(cell) = target {
                    if action.kind == 135 {
                        house.set_preferred_super_weapon_target(cell);
                    } else {
                        house.alert_super_weapon_defense(cell, self.session.binary_frame as i32);
                    }
                } else if action.kind == 136 {
                    house.clear_preferred_super_weapon_target();
                } else {
                    house.clear_super_weapon_defense();
                }
            }
            90 => {
                // Steam3e81a617, Execute6DF51B..6DF537: GetWaypoint68BCC0's
                // packed cell (an unset slot is (0,0)) goes straight to
                // LightningStorm::CreateCloudBolt53A140; the action succeeds.
                let (Some(rules), Some(waypoint)) = (rules, action.waypoint_index) else {
                    return false;
                };
                let Some(cell) = self.session.waypoint_cell(waypoint as i32) else {
                    return false;
                };
                crate::sim::superweapon::lightning_storm::create_cloud_bolt(self, rules, cell);
            }
            102 => {
                // 6DD9EC..6DDAB8: the waypoint's signed centre/Ground/bridge
                // coordinate is truncated back to a cell (413A30). A raging
                // storm (IsActive53A100) fails at 6DDA98 without restarting;
                // otherwise Start539EB0 takes the same LightningStormDuration
                // (+179C) / LightningDeferment (+1794) as the Super launch,
                // the incoming House, and no charge gate.
                let (Some(rules), Some(waypoint)) = (rules, action.waypoint_index) else {
                    return false;
                };
                if crate::sim::superweapon::lightning_storm::raging(self) {
                    return false;
                }
                let Some([x, y, _]) = self.trigger_waypoint_coord(waypoint as i32) else {
                    return false;
                };
                crate::sim::superweapon::lightning_storm::start(
                    self,
                    rules,
                    rules.general.lightning_storm_duration,
                    rules.general.lightning_deferment,
                    ((x / 256) as i16, (y / 256) as i16),
                    context.incoming_house,
                );
            }
            ACTION_SET_ALTERNATE_BASE => {
                let Some(house_id) = context.incoming_house else {
                    return false;
                };
                let Some(waypoint_index) = action.waypoint_index else {
                    return false;
                };
                let Some(cell) = self.session.waypoint_cell(waypoint_index as i32) else {
                    return false;
                };
                if cell == (0, 0) {
                    return false;
                }
                // gamemd-derived: `TriggerAction__Execute @ 0x006DD8B0`
                // case 137 calls `FUN_006E44E0`, which rejects the packed-zero
                // waypoint and writes only `HouseClass+0x5494` through
                // `FUN_0050DFE0`.
                if let Some(house) = self.houses.get_mut(&house_id) {
                    house.alternate_base_center = (cell.0 as u16, cell.1 as u16);
                }
            }
            ACTION_CLEAR_ALTERNATE_BASE => {
                let Some(house_id) = context.incoming_house else {
                    return false;
                };
                // gamemd-derived: `TriggerAction__Execute @ 0x006DD8B0`
                // case 138 calls `FUN_006E4540`, which writes packed zero only
                // to `HouseClass+0x5494` through `FUN_0050DFF0`.
                if let Some(house) = self.houses.get_mut(&house_id) {
                    house.alternate_base_center = (0, 0);
                }
            }
            1 | 2 | ACTION_ANNOUNCE_WIN | ACTION_ANNOUNCE_LOSE | ACTION_END_SCENARIO => {
                //6DDCB1/6DDD23 and6DF399..6DF3CE use PlayerPtr, not the
                //incoming Trigger House. The shared House result/timer
                //owner drives normal frame termination and ScenarioExit.
                return self.trigger_house_outcome(
                    action.kind,
                    value.unwrap_or(0),
                    rules,
                    frame_effects,
                );
            }
            _ => return false,
        }
        true
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
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> Option<crate::sim::intern::InternedId> {
    let rules = rules?;
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

fn count_techtype(sim: &Simulation, type_id: &str) -> usize {
    sim.substrate
        .entities
        .values()
        .filter(|e| {
            sim.interner
                .resolve(e.type_ref())
                .eq_ignore_ascii_case(type_id)
        })
        .count()
}

#[cfg(test)]
#[path = "trigger_runtime_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "trigger_runtime_native_tests.rs"]
mod native_tests;

#[cfg(test)]
#[path = "trigger_runtime_superweapon_state_tests.rs"]
mod superweapon_state_tests;

#[cfg(test)]
#[path = "trigger_runtime_defense_target_tests.rs"]
mod defense_target_tests;

#[cfg(test)]
#[path = "trigger_runtime_type_tech_level_tests.rs"]
mod type_tech_level_tests;

#[cfg(test)]
#[path = "trigger_runtime_all_to_hunt_tests.rs"]
mod all_to_hunt_tests;

#[cfg(test)]
#[path = "trigger_runtime_no_factories_tests.rs"]
mod no_factories_tests;

#[cfg(test)]
#[path = "trigger_runtime_damage_event_tests.rs"]
mod damage_event_tests;

#[cfg(test)]
#[path = "trigger_runtime_reveal_zone_tests.rs"]
mod reveal_zone_tests;

#[cfg(test)]
#[path = "trigger_runtime_ai_activation_tests.rs"]
mod ai_activation_tests;

#[cfg(test)]
#[path = "trigger_runtime_crate_tests.rs"]
mod crate_tests;

#[cfg(test)]
#[path = "trigger_runtime_build_type_tests.rs"]
mod build_type_tests;

#[cfg(test)]
#[path = "trigger_runtime_wakeup_tests.rs"]
mod wakeup_tests;

#[cfg(test)]
#[path = "trigger_runtime_chrono_reinforcement_tests.rs"]
mod chrono_reinforcement_tests;

#[cfg(test)]
#[path = "trigger_runtime_lightning_tests.rs"]
mod lightning_tests;

#[cfg(test)]
#[path = "trigger_runtime_explosion_tests.rs"]
mod explosion_tests;

#[cfg(test)]
#[path = "trigger_runtime_movement_event_tests.rs"]
mod movement_event_tests;

#[cfg(test)]
#[path = "trigger_runtime_ore_tests.rs"]
mod ore_tests;

#[cfg(test)]
#[path = "trigger_runtime_particle_tests.rs"]
mod particle_tests;

#[cfg(test)]
#[path = "trigger_runtime_map_lighting_tests.rs"]
mod map_lighting_tests;

#[cfg(test)]
#[path = "trigger_runtime_tint_tests.rs"]
mod tint_tests;

#[cfg(test)]
#[path = "trigger_runtime_ambient_event_tests.rs"]
mod ambient_event_tests;

#[cfg(test)]
#[path = "trigger_runtime_nuke_tests.rs"]
mod nuke_tests;
