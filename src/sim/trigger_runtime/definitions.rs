//! Map-authored Trigger/Tag definition registries: Read6DD5B0's
//! TeamType, TriggerType and TagType reads into `RuntimeDefinitions`, action
//! reference resolution, and the Tag event/action classification tables.

use super::{
    DefinitionReference, RuntimeAction, RuntimeDefinitions, RuntimeEvent, TagType, TriggerType,
};
use crate::map::actions::{ActionDefinitionReference, ActionEntry, DefinitionSelector};
use crate::rules::ini_parser::is_native_none_type_name;
use crate::rules::ini_value::{crt_atoi, truncate_native_bytes};
use std::collections::BTreeSet;

impl RuntimeDefinitions {
    /// TeamTypeClass::FindByIDOrName @ 0x006F0FC0 (retail gamemd.exe).
    /// Scan the constructor vector, ID then Name for each entry; callers own
    /// token preprocessing. This never allocates, trims or handles sentinels.
    pub(crate) fn find_team_type(
        &self,
        requested: &str,
        teams: &crate::rules::team_ai_ini::TeamAiIniRegistry,
    ) -> Option<&str> {
        self.team_types
            .iter()
            .find(|id| {
                id.eq_ignore_ascii_case(requested)
                    || teams.team_type_name(id).eq_ignore_ascii_case(requested)
            })
            .map(String::as_str)
    }

    pub(crate) fn team_type_ids(&self) -> impl Iterator<Item = &str> {
        self.team_types.iter().map(String::as_str)
    }

    pub(super) fn team_type(&mut self, name: &str) -> Option<usize> {
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
    pub(super) fn trigger_type(&mut self, name: &str) -> Option<usize> {
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
    pub(super) fn tag_type(&mut self, name: &str) -> Option<usize> {
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
    pub(super) fn resolve_action(
        &mut self,
        entry: ActionEntry,
        eva: Option<&crate::rules::sound_ini::EvaRegistry>,
        sounds: Option<&crate::rules::sound_ini::SoundRegistry>,
        themes: Option<&crate::rules::theme_ini::ThemeCatalog>,
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
        let value = match parameter_type {
            // TAction::Read6DD7BC/6DD7CE stores the raw name's EVA753250
            // or Voc7514D0 ordinal in +90 for every action, including timer
            // Execute27 at6DE5C9. Unknown names materialize -1, not zero.
            6 | 7 => Some(
                entry
                    .params
                    .get(1)
                    .and_then(|name| {
                        if parameter_type == 6 {
                            eva?.index_of(name)
                        } else {
                            sounds?.index_of(name)
                        }
                    })
                    .unwrap_or(-1),
            ),
            // TActionClass::Read6DD7E0..6DD7EB, original instruction listing:
            // From_Name721210 stores its signed result at +90 for every
            // ActionID, independently of playback admission. Reuse the same
            // process-selected catalog as Theme; raw missing names become -1.
            8 => Some(entry.params.get(1).map_or(-1, |name| {
                themes.map_or(-1, |catalog| catalog.from_name(name))
            })),
            _ => entry.literal_value(),
        };
        // FindIndex7514D0 compares the untrimmed token case-insensitively.
        let sound = if parameter_type == 7 || matches!(entry.kind, 19 | 98 | 99) {
            //Execute's Voc call consumes +90 regardless of the authored
            //parameter type. Resolve a literal ordinal once in this reader,
            //through the same fixed catalog as a type7 name.
            value.and_then(|index| sounds?.entry_by_index(index))
        } else {
            None
        }
        .map(|sound| crate::rules::sound_ini::SoundReference::Name(sound.id.clone()));
        RuntimeAction {
            entry,
            reference,
            value,
            sound,
        }
    }
    pub(crate) fn read_teams(teams: &crate::rules::team_ai_ini::TeamAiIniRegistry) -> Self {
        let mut definitions = Self::default();
        // Full_Init687984/68798D: reads are transactions, not the final merged
        // TeamType vector. Team.Tag can first-construct TagTypes here.
        for (id, read) in teams.team_type_registration_sequence() {
            let Some(index) = definitions.team_type(id) else {
                continue;
            };
            let Some(team) = read else { continue };
            // Read6F131E retains +D0 on an absent/empty ReadString; a
            // nonempty value calls6E6310, including either null literal.
            // Only this physical pass can construct a new TagType identity.
            let tag = team.physical_read_section().read_string("Tag", "", 128);
            if !tag.is_empty() {
                let resolved = definitions.tag_type(&tag);
                definitions.team_tags[index] = resolved;
            }
        }
        definitions
    }

    pub(super) fn read_map(
        map: &crate::map::map_file::MapFile,
        teams: &crate::rules::team_ai_ini::TeamAiIniRegistry,
        eva: Option<&crate::rules::sound_ini::EvaRegistry>,
        sounds: Option<&crate::rules::sound_ini::SoundRegistry>,
        themes: Option<&crate::rules::theme_ini::ThemeCatalog>,
    ) -> Self {
        let mut definitions = Self::read_teams(teams);
        // LoadFromINIList7275D0/6E5ED0 find or allocate each key's Type,
        // ignoring case, and ReadINI727240/6E6080 read every record by that
        // Type's own ID, exact case. A key differing from an earlier one only
        // in case revisits that Type and reads the earlier key's records.
        // LoadFromStraw525A60/526177 retains every physical entry. The
        // 7275D0/6E5ED0 loops revisit a Type for each copy, reading its header
        // through the shared CRC lookup526B10: events prepend again while
        // actions replace. No new RNG/timer/detach owner belongs to the read;
        // the existing Fill/LiveTrigger constructor consumes those events.
        if let Some(section) = map.ini.section("Triggers") {
            for key in section.physical_keys() {
                if section.read_name(key, 24).is_none() {
                    continue;
                }
                let Some(index) = definitions.trigger_type(key) else {
                    continue;
                };
                let id = definitions.trigger_types[index].id.clone();
                let Some(record) = map.triggers.get(&id) else {
                    continue;
                };
                //7272D6 resolves the linked Type before events and actions.
                let linked = record
                    .linked_trigger_id()
                    .and_then(|name| definitions.trigger_type(name));
                let mut events = map
                    .events
                    .get(&id)
                    .map_or_else(Vec::new, |record| record.conditions().to_vec());
                // TEvent::Read71F595 is find-only and runs before this
                // Trigger's action factories. Unknown references stay null.
                for event in events.iter_mut().rev() {
                    if let Some(team) = event.team_name.as_deref() {
                        event.team_name =
                            definitions.find_team_type(team, teams).map(str::to_owned);
                    }
                }
                //727530..7275A3: a present entry rebuilds the action list.
                let actions = map.actions.get(&id).map(|record| {
                    record
                        .entries()
                        .iter()
                        .cloned()
                        .map(|entry| definitions.resolve_action(entry, eva, sounds, themes))
                        .collect::<Vec<_>>()
                });
                let typ = &mut definitions.trigger_types[index];
                typ.owner = record.owner().map(str::to_string);
                typ.linked = linked;
                typ.enabled = record.enabled();
                typ.difficulty = record.difficulty();
                typ.transfer_on_capture |= record.transfer_on_capture();
                //7274E2..727514 prepends to the list a revisit retains.
                let retained = std::mem::take(&mut typ.events);
                typ.events = events
                    .into_iter()
                    .map(RuntimeEvent::from)
                    .chain(retained)
                    .collect();
                if let Some(actions) = actions {
                    typ.actions = actions;
                }
            }
        }
        if let Some(section) = map.ini.section("Tags") {
            for key in section.physical_keys() {
                if section.read_name(key, 24).is_none() {
                    continue;
                }
                let Some(index) = definitions.tag_type(key) else {
                    continue;
                };
                let Some(record) = map.tags.get(&definitions.tag_types[index].id) else {
                    continue;
                };
                let trigger = record
                    .trigger()
                    .and_then(|name| definitions.trigger_type(name));
                let typ = &mut definitions.tag_types[index];
                typ.repeat = record.repeat();
                typ.name = record.name().to_string();
                typ.trigger = trigger;
            }
        }
        definitions
    }
    /// `ares` is the game's `RuleSet::ares_form`.
    pub(super) fn classification(&self, tag_type: usize, ares: bool) -> u8 {
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
                flags |= event_flags(event.condition.kind, ares);
            }
            for action in &typ.actions {
                flags |= action_flags(action.entry.kind, ares);
            }
            next = typ.linked;
        }
        flags
    }

    pub(super) fn has_action(&self, tag_type: usize, kind: i32) -> bool {
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

    pub(super) fn has_event(&self, tag_type: usize, kind: i32) -> bool {
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
            if typ.events.iter().any(|event| event.condition.kind == kind) {
                return true;
            }
            next = typ.linked;
        }
        false
    }
}

/// An event's Tag flags in this game: the stock table
/// ([`event_classification`]), behind Ares' own answer when Ares plays
/// (`RuleSet::ares_form`).
fn event_flags(kind: i32, ares: bool) -> u8 {
    // Ares 3.0 TEventExt::GetFlags 1004F6A0 (hook 0x71F683) answers its own
    // events before the stock table: the object events 62..74, 79, 80, 83
    // and 84 are attached-object (2); the superweapon, reverse-engineered
    // type, owned type, destroyed-by-House and KeepAlive events are House
    // Tags (8); 86 is a Logic Tag (16). Other kinds keep the stock flags.
    if ares {
        match kind {
            62..=74 | 79 | 80 | 83 | 84 => return 2,
            75..=78 | 81 | 82 | 85 | 87 | 88 => return 8,
            86 => return 16,
            _ => {}
        }
    }
    event_classification(kind)
}

/// An action's Tag flags in this game: the stock table
/// ([`action_classification`]), behind Ares' own answer when Ares plays
/// (`RuleSet::ares_form`).
fn action_flags(kind: i32, ares: bool) -> u8 {
    // Ares 3.0 TActionExt::GetFlags 10039930 (hook 0x6E3EE0): kill drivers
    // (147) and set group (149) act on attached objects; AuxPower (146) and
    // the EVA voice (148) answer 0, as the stock table does above 111.
    if ares && matches!(kind, 147 | 149) {
        return 2;
    }
    action_classification(kind)
}

// Original jump-table bytes71F700/71F744/71F780/71F7C0 and6E3F08.
// These are classification only, never substitutes for event predicates.
pub(super) fn event_classification(kind: i32) -> u8 {
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
