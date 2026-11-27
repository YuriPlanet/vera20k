//! Trigger runtime lifecycle: restore validation, definition installation,
//! live Tag/Trigger construction and timer resets, map classification,
//! object/team/cell Tag attachment and deferred deletion.

#[cfg(any(test, feature = "dev-overlays"))]
use super::TriggerTraceEvent;
use super::{
    DefinitionReference, LOGICAL_FRAMES_PER_SECOND, LiveTag, LiveTrigger, RetainedLocalVariables,
    RuntimeDefinitions, TagAttachment, TriggerRuntime, resolve_trigger_house,
    validate_live_registry,
};
use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ini_value::crt_atoi;
use crate::sim::rng::SimRng;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::timer::CdTimer;
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::Simulation;
use std::collections::BTreeSet;

impl Simulation {
    /// Validate this owner's retained identities before the restore coordinator
    /// publishes a candidate. No map re-read or live constructor runs on load.
    /// Destruction may leave orphan Triggers until their shared725C70 tickets
    /// are consumed, so that native intermediate state remains admissible.
    /// `ares` is the loading game's `RuleSet::ares_form`, which decides the
    /// Tag flags a registration is checked against.
    pub(crate) fn validate_restored_trigger_runtime(&self, ares: bool) -> Result<(), String> {
        let runtime = &self.trigger_runtime;
        let definitions = &runtime.definitions;
        runtime.module.validate()?;
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
                if !seen.insert(index)
                    || definitions.classification(tag.type_index, ares) & flag == 0
                {
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
                if !seen.insert(index) || definitions.classification(tag.type_index, ares) & 8 == 0
                {
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

    /// Install fixed AIMD plus scenario AI definitions after RuleSet identities
    /// are interned and before the first gameplay tick. This is data ingress
    /// only: the installed VM contains no live TeamClass instances.
    ///
    /// A definition that names something the RuleSet lacks still installs, from
    /// fixed AIMD as from a scenario, and comes back as a logged diagnostic.
    /// gamemd refuses none of them: TaskForceClass::ReadINI (`0x006E8420`)
    /// keeps a member only when its type resolved; TeamTypeClass::ReadINI
    /// (`0x006F1090`) finds or allocates its Script and TaskForce;
    /// AITriggerTypeClass::ReadINI (`0x0041F580`) keeps a trigger whose owner
    /// (`nOwnerMode` stays 0) or condition object (null) did not resolve.
    pub fn install_team_ai_registry(
        &mut self,
        registry: &crate::rules::team_ai_ini::TeamAiIniRegistry,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Vec<crate::sim::team_script_vm::TeamAiInstallDiagnostic> {
        // Cold callers stage the same physical Team prefix. Ordinary loaders
        // already prepared the map and its action-created identities; never
        // replay those reads or replace that tail during AI installation.
        let staged = (!self.trigger_runtime.definitions_prepared)
            .then(|| RuntimeDefinitions::read_teams(registry));
        let definitions = staged.as_ref().unwrap_or(&self.trigger_runtime.definitions);
        let (vm, diagnostics) =
            crate::sim::team_script_vm::TeamScriptVm::from_prepared_ini_registry(
                registry,
                definitions,
                &mut self.interner,
                rules,
            );
        if let Some(definitions) = staged {
            self.trigger_runtime.definitions = definitions;
        }
        *self.team_script_vm = vm;
        for diagnostic in &diagnostics {
            log::warn!("Team AI install diagnostic: {diagnostic:?}");
        }
        diagnostics
    }

    pub(crate) fn trigger_definitions_prepared(&self) -> bool {
        self.trigger_runtime.definitions_prepared
    }

    /// Snapshot the values that SetDefaults683610 and ClearScene6851F0 leave
    /// intact. Names come from the incoming map; no live Tag state transfers.
    pub fn retain_local_variables_for_fresh_load(&self) -> RetainedLocalVariables {
        self.trigger_runtime.check_attached();
        RetainedLocalVariables {
            set: self.trigger_runtime.locals_set.clone(),
        }
    }

    /// Install the outgoing values before ReadLocal689B20 and before any new
    /// Tag exists. This is a fresh-load handoff, never a saved-world overlay.
    pub fn inherit_local_variables_for_fresh_load(&mut self, prior: RetainedLocalVariables) {
        assert!(!self.trigger_runtime.definitions_prepared);
        self.trigger_runtime.locals_set = prior.set;
    }
    /// Full_Init687984..6879CB registry ingress. No live instance is created
    /// until a Cell/Object attachment or Fill classification requires it.
    pub fn initialize_trigger_definitions(
        &mut self,
        map: &crate::map::map_file::MapFile,
        teams: &crate::rules::team_ai_ini::TeamAiIniRegistry,
        eva: Option<&crate::rules::sound_ini::EvaRegistry>,
        sounds: Option<&crate::rules::sound_ini::SoundRegistry>,
        themes: Option<&crate::rules::theme_ini::ThemeCatalog>,
    ) {
        // ReadLocal689B2E clears names only. Missing second tokens and slots
        // absent from the map keep their current value;689C24 writes explicit
        // assignments directly without setter notifications or RNG draws.
        let mut locals_set = std::mem::take(&mut self.trigger_runtime.locals_set);
        for variable in map.local_variables.values() {
            match variable.initially_set {
                Some(true) => {
                    locals_set.insert(variable.index);
                }
                Some(false) => {
                    locals_set.remove(&variable.index);
                }
                None => {}
            }
        }
        let mut definitions = RuntimeDefinitions::read_map(map, teams, eva, sounds, themes);
        if self.session.execution_topology().is_lockstep() {
            for trigger in &mut definitions.trigger_types {
                for action in &mut trigger.actions {
                    action.retain_network_playback();
                }
            }
        }
        *self.trigger_runtime = TriggerRuntime {
            definitions,
            definitions_prepared: true,
            locals_set,
            ..TriggerRuntime::default()
        };
    }

    pub(crate) fn trigger_difficulty(&self) -> i32 {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(difficulty) = self.trigger_runtime.fixture_difficulty {
            return difficulty;
        }
        self.session
            .campaign_house_difficulties()
            .map_or(1, |(player, _)| player.table_index() as i32)
    }

    /// Original Tag6E52A0 includes pending-but-not-yet-destroyed instances.
    pub(super) fn shared_trigger_tag(&mut self, type_index: usize) -> usize {
        if let Some(index) = self.trigger_runtime.tag_order.iter().copied().find(|&i| {
            self.trigger_runtime
                .tag(i)
                .is_some_and(|tag| tag.type_index == type_index)
        }) {
            return index;
        }
        self.construct_trigger_tag(type_index)
    }

    pub(super) fn construct_trigger_tag(&mut self, type_index: usize) -> usize {
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
                timer: CdTimer::started(self.session.binary_frame() as i32, 0),
                completed: 0,
                retained_house: None,
                enabled: true,
                pending: false,
            }));
            self.trigger_runtime.trigger_order.push(index);
            // Reset726400 runs BEFORE enabled/difficulty admission.
            Self::reset_trigger_timers(
                &self.session,
                &mut self.scenario_rng,
                &mut self.trigger_runtime,
                index,
            );
            let typ = &self.trigger_runtime.definitions.trigger_types[type_index];
            let enabled = typ.enabled && typ.admits_difficulty(difficulty);
            next = typ.linked;
            self.trigger_runtime.triggers[index]
                .as_mut()
                .expect("the Trigger was pushed just above")
                .enabled = enabled;
            self.trigger_runtime.tags[tag_index]
                .as_mut()
                .expect("the Tag was pushed at the top of this build")
                .triggers
                .insert(0, index);
        }
        #[cfg(any(test, feature = "dev-overlays"))]
        if self.trigger_runtime.trace.is_some() {
            let tag = self.trigger_runtime.trace_tag(tag_index);
            Self::trace_event(
                &mut self.trigger_runtime,
                self.session.binary_frame(),
                |seq, binary_frame, parent| TriggerTraceEvent::TagConstructed {
                    seq,
                    binary_frame,
                    parent,
                    tag,
                },
            );
        }
        tag_index
    }

    pub(super) fn reset_trigger_timers(
        session: &ScenarioSession,
        scenario_rng: &mut SimRng,
        trigger_runtime: &mut TriggerRuntime,
        index: usize,
    ) {
        let Some(trigger) = trigger_runtime.trigger(index) else {
            return;
        };
        let type_index = trigger.type_index;
        let event_count = trigger_runtime.definitions.trigger_types[type_index]
            .events
            .len();
        for ordinal in 0..event_count {
            let event =
                &trigger_runtime.definitions.trigger_types[type_index].events[ordinal].condition;
            let duration = match event.kind {
                13 => event.value.wrapping_mul(LOGICAL_FRAMES_PER_SECOND),
                51 => {
                    let value = event.value;
                    let drawn = scenario_rng.next_range_i32_inclusive(0, value);
                    (value / 2)
                        .wrapping_add(drawn)
                        .wrapping_mul(LOGICAL_FRAMES_PER_SECOND)
                }
                _ => continue,
            };
            let trigger = trigger_runtime.triggers[index]
                .as_mut()
                .expect("the Trigger was read live at the top of this reset");
            trigger.timer.start(session.binary_frame() as i32, duration);
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
            let identity = Self::trigger_cell_identity(&self.resolved_terrain, cell);
            if self.trigger_runtime.cell_tags.contains_key(&identity) {
                continue;
            }
            let tag = self.shared_trigger_tag(type_index);
            self.trigger_runtime.tags[tag]
                .as_mut()
                .expect("`shared_trigger_tag` answers a live Tag")
                .attached_cell = cell;
            //4AD2C9 repeats5657A0 after live construction; a miss stamps the
            // same retained Dummy a second time before CellReplace485250.
            let identity = Self::trigger_cell_identity(&self.resolved_terrain, cell);
            self.trigger_runtime
                .replace_cell_trigger_tag(identity, Some(tag));
        }
    }

    /// Fill684D47..684EC1 registers in retained TagType order: map, Logic,
    /// then House. Registration does not increase the attachment refcount.
    pub fn classify_map_triggers(&mut self, rules: &crate::rules::ruleset::RuleSet) {
        if self.trigger_runtime.classified {
            return;
        }
        let ares = rules.ares_form();
        for type_index in 0..self.trigger_runtime.definitions.tag_types.len() {
            let flags = self
                .trigger_runtime
                .definitions
                .classification(type_index, ares);
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

    pub(super) fn trigger_cell_identity(
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        cell: (i16, i16),
    ) -> NativeCellIdentity {
        let terrain = resolved_terrain
            .as_ref()
            .expect("live CellTags require installed native map cell storage");
        NativeCellQuery::canonical(terrain).lookup(cell)
    }

    /// Object+34 nonnull gate; dispatch itself owns Tag liveness.
    pub(crate) fn object_has_trigger_tag(&self, object: u64) -> bool {
        self.trigger_runtime
            .attachments
            .contains_key(&TagAttachment::Object(object))
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
        self.trigger_runtime
            .replace_trigger_tag_attachment(TagAttachment::Object(object), tag);
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
        self.trigger_runtime
            .replace_trigger_tag_attachment(TagAttachment::Team(team), tag);
    }

    /// Destructor6E8E75..6E8EAB follows member removal. Only a zero-reference
    /// Tag is queued, then the direct Team pointer is cleared without a draw.
    pub(crate) fn clear_team_trigger_tag(
        substrate: &mut ObjectSubstrate,
        trigger_runtime: &mut TriggerRuntime,
        team: u64,
    ) {
        if let Some(&tag) = trigger_runtime.attachments.get(&TagAttachment::Team(team))
            && trigger_runtime.tag(tag).is_some_and(|tag| tag.refs == 0)
        {
            Self::mark_live_tag_for_delete(substrate, trigger_runtime, tag);
        }
        trigger_runtime.replace_trigger_tag_attachment(TagAttachment::Team(team), None);
    }

    pub(super) fn mark_live_trigger_for_delete(
        substrate: &mut ObjectSubstrate,
        trigger_runtime: &mut TriggerRuntime,
        index: usize,
    ) {
        if let Some(trigger) = trigger_runtime
            .triggers
            .get_mut(index)
            .and_then(Option::as_mut)
        {
            trigger.pending = true;
            substrate.pending_delete.queue_trigger(index);
        }
    }
    pub(super) fn mark_live_tag_for_delete(
        substrate: &mut ObjectSubstrate,
        trigger_runtime: &mut TriggerRuntime,
        index: usize,
    ) {
        if let Some(tag) = trigger_runtime.tags.get_mut(index).and_then(Option::as_mut) {
            tag.pending = true;
            substrate.pending_delete.queue_tag(index);
        }
    }

    /// Tag destructor6E4F60 selected by the common725C70 owner. It can
    /// append Trigger tickets; the same global live scan consumes them later.
    pub(crate) fn delete_live_tag(
        &mut self,
        index: usize,
        rules: &crate::rules::ruleset::RuleSet,
        game_state: i32,
        started: bool,
    ) {
        let Some(tag) = self
            .trigger_runtime
            .tags
            .get_mut(index)
            .and_then(Option::as_mut)
        else {
            return;
        };
        let typ = tag.type_index;
        // The Tag is freed below; its chain moves out with nothing left to
        // read it, since detaching a Tag never reads its Triggers.
        let triggers = std::mem::take(&mut tag.triggers);
        self.trigger_runtime.detach_live_tag(index);
        // Active-start destructor6E4F60 searches AFTER detach:
        // Find=-1 therefore still decrements the signed cursor.
        let flags = self
            .trigger_runtime
            .definitions
            .classification(typ, rules.ares_form());
        self.trigger_runtime
            .adjust_tag_destructor_cursors(flags, game_state, started);
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
                house.removed_allow_win_tag(self.session.binary_frame() as i32);
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
}
