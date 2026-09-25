//! Deterministic TeamClass/ScriptClass execution seam for YR 1.001.
//!
//! The data here is resolved at the scenario boundary rather than parsed from
//! INI directly. `TeamClass::AI` is intentionally represented as raw action
//! records: only native action bodies with closed evidence execute.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::rules::locomotor_type::MovementZone;
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::CountryIdx;
use crate::rules::team_ai_ini::TeamAiDefinitionSource;
use crate::sim::command::CommandEnvelope;
use crate::sim::intern::InternedId;
use crate::util::native_x87::NativeF64Bits;

mod registry_install;

/// One resolved ScriptType action record.
///
/// This matches the `(action, argument)` pair read by `ScriptClass` instead of
/// promoting descriptive opcode names into a complete, guessed enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamScriptAction {
    pub action_id: i32,
    pub argument: i32,
}

/// One resolved ScriptType program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamScriptDefinition {
    pub id: InternedId,
    pub actions: Vec<TeamScriptAction>,
    pub source: TeamAiDefinitionSource,
}

/// A category-distinct TechnoType identity retained from native pointer
/// resolution. The ID alone is insufficient for custom rules that register
/// the same name in multiple native type families. TaskForce entries and
/// AITrigger token 6 both store this shape.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct TeamMemberTypeIdentity {
    pub category: ObjectCategory,
    pub id: InternedId,
}

/// One TaskForce requirement. `member_type` is the resolved native identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamTaskForceEntry {
    pub member_type: TeamMemberTypeIdentity,
    pub count: i32,
}

/// The TeamType-attached TaskForce definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamTaskForceDefinition {
    pub id: InternedId,
    /// Signed TaskForce `Group=` fallback consumed by Team recruitment.
    pub group: i32,
    pub entries: Vec<TeamTaskForceEntry>,
    pub source: TeamAiDefinitionSource,
}

/// The resolved ScriptType and TaskForce attachments carried by a TeamType.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamTypeDefinition {
    pub id: InternedId,
    pub script_id: InternedId,
    pub task_force_id: InternedId,
    /// Signed TeamType `Priority=` consumed by House base-defence suspension.
    pub priority: i32,
    /// TeamType `IsBaseDefense=` byte used by responder admission/assignment.
    pub is_base_defense: bool,
    /// TeamType `Suicide=` (`+0xAF`, read at `0x006F1271`, constructor 0):
    /// a member of such a team never retaliates (`ShouldRetaliate
    /// 0x00708A2C..0x00708A54`).
    #[serde(default)]
    pub suicide: bool,
    /// TeamType `Aggressive=` (`+0xAD`, read at `0x006F12A9`, constructor 0 at
    /// `0x006F0741`): a computer member on Move with no target passes the
    /// passive-acquire gate without CanAcquireTarget (`0x0070929A..0x007092EC`).
    #[serde(default)]
    pub aggressive: bool,
    /// `TeamTypeClass+0xEC`: post-load fold of resolved TaskForce movement rows.
    pub combined_movement_zone: MovementZone,
    /// `TeamTypeClass+0xF0`: whether AI eligibility compares House base zones.
    pub base_zone_relation_enforced: bool,
    /// `TeamTypeClass+0xF1`: require separation under the combined row and
    /// connectivity under the native Amphibious row.
    pub transport_crossing_required: bool,
}

/// Lossless load metadata beside the narrow TeamType fields already consumed
/// by live Team behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamTypeIniMetadata {
    pub max_teams: i32,
    pub autocreate: bool,
    pub are_team_members_recruitable: bool,
    pub raw_fields: Vec<(String, String)>,
    pub source: TeamAiDefinitionSource,
}

impl Default for TeamTypeIniMetadata {
    fn default() -> Self {
        Self {
            max_teams: -1,
            autocreate: false,
            are_team_members_recruitable: true,
            raw_fields: Vec::new(),
            source: TeamAiDefinitionSource::FixedAimd,
        }
    }
}

/// Native AITrigger owner mode retained by the Stage-A registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TeamAiTriggerOwner {
    All,
    Country(CountryIdx),
}

/// One resolved AITriggerType record. Selector semantics remain a later
/// evidence-gated stage; this retains every field whose storage is proven by
/// the active YR raw reader at `0x0041F580` plus the lossless 18-token source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamAiTriggerDefinition {
    pub id: InternedId,
    pub tokens: [String; 18],
    pub display_name: String,
    pub enabled: bool,
    pub primary_team_type: Option<InternedId>,
    pub owner: Option<TeamAiTriggerOwner>,
    pub threshold: i32,
    pub condition: i32,
    pub object_type: Option<TeamMemberTypeIdentity>,
    pub comparison_mask: [u8; 32],
    pub weights: [NativeF64Bits; 3],
    pub storage_flag_d0: bool,
    pub storage_i32_ac: i32,
    pub storage_flag_d1: bool,
    pub secondary_team_type: Option<InternedId>,
    pub difficulty_enabled: [bool; 3],
    pub source: TeamAiDefinitionSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamAiInstallDiagnostic {
    UnknownTaskForceMember {
        task_force_id: String,
        member_type: String,
        source: TeamAiDefinitionSource,
    },
    MissingTeamTypeScript {
        team_type_id: String,
        script_id: String,
        source: TeamAiDefinitionSource,
    },
    MissingTeamTypeTaskForce {
        team_type_id: String,
        task_force_id: String,
        source: TeamAiDefinitionSource,
    },
    MissingAiTriggerTeamType {
        trigger_id: String,
        team_type_id: String,
        source: TeamAiDefinitionSource,
    },
    UnknownAiTriggerOwner {
        trigger_id: String,
        owner: String,
        source: TeamAiDefinitionSource,
    },
    UnknownAiTriggerObject {
        trigger_id: String,
        object_type: String,
        source: TeamAiDefinitionSource,
    },
}

impl TeamAiInstallDiagnostic {
    pub(crate) fn source(&self) -> TeamAiDefinitionSource {
        match self {
            Self::UnknownTaskForceMember { source, .. }
            | Self::MissingTeamTypeScript { source, .. }
            | Self::MissingTeamTypeTaskForce { source, .. }
            | Self::MissingAiTriggerTeamType { source, .. }
            | Self::UnknownAiTriggerOwner { source, .. }
            | Self::UnknownAiTriggerObject { source, .. } => *source,
        }
    }

    pub(crate) fn is_fixed_source_refusal(&self) -> bool {
        self.source() == TeamAiDefinitionSource::FixedAimd
    }
}

/// A candidate member passed to TeamType/TaskForce admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamScriptMember {
    pub entity_id: u64,
    pub member_type: TeamMemberTypeIdentity,
}

/// Action-19 side effect emitted in TeamClass member-list order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamScriptEffect {
    PanicMember { entity_id: u64 },
}

/// Effects produced by one TeamClass update pass.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TeamScriptTick {
    pub effects: Vec<TeamScriptEffect>,
}

/// A serializable refusal instead of an inferred ScriptType transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TeamScriptRefusal {
    MissingScript { script_id: InternedId },
    MissingTeamType { team_type_id: InternedId },
    MissingTaskForce { task_force_id: InternedId },
    UnsupportedAction { action_id: i32 },
    OutOfRangeAction { action_id: i32 },
}

/// Persistent TeamClass execution state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamScriptState {
    id: u64,
    owner: InternedId,
    team_type_id: Option<InternedId>,
    task_force_id: Option<InternedId>,
    script_id: InternedId,
    cursor: i32,
    advance_pending: bool,
    delay_remaining_frames: u32,
    wait_condition_complete: bool,
    /// Three independent TeamClass bytes written by `FUN_006EC250`. Neutral
    /// displacement names avoid assigning broader semantics than the live
    /// suspension transaction proves.
    #[serde(default)]
    reached_required_strength_78: bool,
    response_latch_7d: bool,
    response_latch_7e: bool,
    response_latch_83: bool,
    response_suspend_start_frame: i32,
    response_suspend_duration_frames: i32,
    members: Vec<u64>,
    // Stable identity-aware admission summary. A vector keeps the ordered
    // category-distinct keys JSON-serializable as well as snapshot-safe.
    member_type_counts: Vec<(TeamMemberTypeIdentity, u32)>,
    target: Option<u64>,
    succeeded: bool,
    completed: bool,
    refusal: Option<TeamScriptRefusal>,
}

impl TeamScriptState {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn owner(&self) -> InternedId {
        self.owner
    }

    pub fn team_type_id(&self) -> Option<InternedId> {
        self.team_type_id
    }

    pub fn task_force_id(&self) -> Option<InternedId> {
        self.task_force_id
    }

    pub fn script_id(&self) -> InternedId {
        self.script_id
    }

    pub fn cursor(&self) -> i32 {
        self.cursor
    }

    pub fn advance_pending(&self) -> bool {
        self.advance_pending
    }

    #[cfg(test)]
    pub fn wait_frames_remaining(&self) -> u32 {
        self.delay_remaining_frames
    }

    pub fn members(&self) -> &[u64] {
        &self.members
    }

    #[cfg(test)]
    pub(crate) fn response_suspension_state(&self) -> (bool, bool, bool, i32, i32) {
        (
            self.response_latch_7d,
            self.response_latch_7e,
            self.response_latch_83,
            self.response_suspend_start_frame,
            self.response_suspend_duration_frames,
        )
    }

    pub fn member_type_counts(&self) -> &[(TeamMemberTypeIdentity, u32)] {
        &self.member_type_counts
    }

    pub fn target(&self) -> Option<u64> {
        self.target
    }

    pub fn succeeded(&self) -> bool {
        self.succeeded
    }

    pub fn completed(&self) -> bool {
        self.completed
    }

    pub fn refusal(&self) -> Option<TeamScriptRefusal> {
        self.refusal
    }
}

/// Owns resolved TeamType/TaskForce/ScriptType definitions and live TeamClass cursors.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TeamScriptVm {
    scripts: BTreeMap<InternedId, TeamScriptDefinition>,
    #[serde(default)]
    script_order: Vec<InternedId>,
    task_forces: BTreeMap<InternedId, TeamTaskForceDefinition>,
    #[serde(default)]
    task_force_order: Vec<InternedId>,
    team_types: BTreeMap<InternedId, TeamTypeDefinition>,
    #[serde(default)]
    team_type_order: Vec<InternedId>,
    #[serde(default)]
    team_type_ini: BTreeMap<InternedId, TeamTypeIniMetadata>,
    #[serde(default)]
    ai_triggers: BTreeMap<InternedId, TeamAiTriggerDefinition>,
    #[serde(default)]
    ai_trigger_order: Vec<InternedId>,
    teams: BTreeMap<u64, TeamScriptState>,
    next_team_id: u64,
}

impl TeamScriptVm {
    pub fn register_script(&mut self, definition: TeamScriptDefinition) {
        if !self.scripts.contains_key(&definition.id) {
            self.script_order.push(definition.id);
        }
        self.scripts.insert(definition.id, definition);
    }

    pub fn register_task_force(&mut self, definition: TeamTaskForceDefinition) {
        if !self.task_forces.contains_key(&definition.id) {
            self.task_force_order.push(definition.id);
        }
        self.task_forces.insert(definition.id, definition);
    }

    pub fn register_team_type(&mut self, definition: TeamTypeDefinition) {
        if !self.team_types.contains_key(&definition.id) {
            self.team_type_order.push(definition.id);
        }
        self.team_type_ini.entry(definition.id).or_default();
        self.team_types.insert(definition.id, definition);
    }

    pub fn register_ai_trigger(&mut self, definition: TeamAiTriggerDefinition) {
        if !self.ai_triggers.contains_key(&definition.id) {
            self.ai_trigger_order.push(definition.id);
        }
        self.ai_triggers.insert(definition.id, definition);
    }

    /// Install an already-admitted TeamClass member list.
    ///
    /// This remains useful for scenario seams that do not yet instantiate
    /// TaskForce-backed members. New callers should use `create_team_from_type`.
    #[cfg(test)]
    pub fn create_team(
        &mut self,
        owner: InternedId,
        script_id: InternedId,
        members: Vec<u64>,
        target: Option<u64>,
        current_frame: i32,
    ) -> u64 {
        self.insert_team(
            owner,
            None,
            None,
            script_id,
            members,
            BTreeMap::new(),
            target,
            current_frame,
        )
    }

    /// Resolve the TeamType attachments and admit matching candidates in
    /// TaskForce-entry order, preserving input order within each type.
    #[cfg(test)]
    pub fn create_team_from_type(
        &mut self,
        owner: InternedId,
        team_type_id: InternedId,
        candidates: &[TeamScriptMember],
        target: Option<u64>,
        current_frame: i32,
    ) -> u64 {
        let Some(team_type) = self.team_types.get(&team_type_id).copied() else {
            return self.insert_refused_team(
                owner,
                team_type_id,
                TeamScriptRefusal::MissingTeamType { team_type_id },
                target,
                current_frame,
            );
        };
        let Some(task_force) = self.task_forces.get(&team_type.task_force_id) else {
            return self.insert_refused_team(
                owner,
                team_type_id,
                TeamScriptRefusal::MissingTaskForce {
                    task_force_id: team_type.task_force_id,
                },
                target,
                current_frame,
            );
        };
        if !self.scripts.contains_key(&team_type.script_id) {
            return self.insert_refused_team(
                owner,
                team_type_id,
                TeamScriptRefusal::MissingScript {
                    script_id: team_type.script_id,
                },
                target,
                current_frame,
            );
        }

        let mut used = vec![false; candidates.len()];
        let mut members = Vec::new();
        let mut counts = BTreeMap::new();
        for entry in &task_force.entries {
            for _ in 0..entry.count.max(0) {
                let Some((index, candidate)) =
                    candidates.iter().enumerate().find(|(index, candidate)| {
                        !used[*index] && candidate.member_type == entry.member_type
                    })
                else {
                    break;
                };
                used[index] = true;
                members.push(candidate.entity_id);
                *counts.entry(entry.member_type).or_insert(0) += 1;
            }
        }

        self.insert_team(
            owner,
            Some(team_type_id),
            Some(team_type.task_force_id),
            team_type.script_id,
            members,
            counts,
            target,
            current_frame,
        )
    }

    pub fn team(&self, id: u64) -> Option<&TeamScriptState> {
        self.teams.get(&id)
    }

    /// Resolve one entity's TeamClass pointer analogue in stable Team creation
    /// order. A member can belong to at most one live TeamClass in native.
    pub(crate) fn team_for_member(&self, entity_id: u64) -> Option<(u64, bool)> {
        self.teams.values().find_map(|team| {
            team.members.contains(&entity_id).then(|| {
                let is_base_defense = team
                    .team_type_id
                    .and_then(|id| self.team_types.get(&id))
                    .is_some_and(|definition| definition.is_base_defense);
                (team.id, is_base_defense)
            })
        })
    }

    /// The TeamType of the team `entity_id` belongs to (Foot `+0x5D4` then
    /// TeamClass `+0x24`), if any.
    pub(crate) fn member_team_type(&self, entity_id: u64) -> Option<&TeamTypeDefinition> {
        let (team_id, _) = self.team_for_member(entity_id)?;
        self.team_types
            .get(&self.teams.get(&team_id)?.team_type_id?)
    }

    /// Native `FUN_006EC250`: visit TeamClass instances in creation order,
    /// suspend those owned by `owner` whose signed TeamType priority is below
    /// `suspend_priority`, remove every member in member-list order, set the
    /// three response bytes, and arm the signed start/duration timer.
    pub(crate) fn suspend_teams_for_base_defense(
        &mut self,
        owner: InternedId,
        suspend_priority: i32,
        current_frame: i32,
        duration_frames: i32,
    ) -> Vec<u64> {
        let team_types = &self.team_types;
        let mut removed = Vec::new();
        for team in self.teams.values_mut() {
            let Some(definition) = team
                .team_type_id
                .and_then(|team_type_id| team_types.get(&team_type_id))
            else {
                continue;
            };
            if team.owner != owner || definition.priority >= suspend_priority {
                continue;
            }

            removed.extend(team.members.drain(..));
            team.member_type_counts.clear();
            team.response_latch_7d = true;
            team.response_latch_7e = true;
            team.response_latch_83 = true;
            team.response_suspend_start_frame = current_frame;
            team.response_suspend_duration_frames = duration_frames;
        }
        removed
    }

    #[cfg(test)]
    pub fn set_delay(&mut self, id: u64, remaining_frames: u32) -> bool {
        let Some(team) = self.teams.get_mut(&id) else {
            return false;
        };
        team.delay_remaining_frames = remaining_frames;
        true
    }

    #[cfg(test)]
    pub fn set_wait_condition_complete(&mut self, id: u64, complete: bool) -> bool {
        let Some(team) = self.teams.get_mut(&id) else {
            return false;
        };
        team.wait_condition_complete = complete;
        true
    }

    /// Execute one `TeamClass::AI` pass for every live team.
    ///
    /// `TeamClass::AI` stores completion in `+0x80`; the next update performs
    /// the shared `ScriptClass::HasNextMission` cursor advance.
    pub fn tick<F>(&mut self, current_frame: i32, owner_is_active: F) -> Vec<CommandEnvelope>
    where
        F: FnMut(InternedId) -> bool,
    {
        self.tick_effects(current_frame, owner_is_active);
        Vec::new()
    }

    /// Execute one pass and return native-backed non-command effects.
    ///
    /// The current command rung has no panic command; callers that own member
    /// mission application can consume these ordered effects directly.
    pub fn tick_effects<F>(&mut self, current_frame: i32, mut owner_is_active: F) -> TeamScriptTick
    where
        F: FnMut(InternedId) -> bool,
    {
        let mut result = TeamScriptTick::default();
        let mut deleted_teams = Vec::new();
        let team_types = &self.team_types;
        let task_forces = &self.task_forces;

        for team in self.teams.values_mut() {
            // gamemd-derived: TeamClass::AI @ 0x006E9140 checks the
            // FUN_006EC250 base-defense timer before every other Team gate.
            // Equality expires in this pass; only +0x83 clears here, so the
            // represented script body may resume without a one-frame gap.
            if team.response_latch_83 {
                if response_suspend_remaining(
                    team.response_suspend_start_frame,
                    team.response_suspend_duration_frames,
                    current_frame,
                ) != 0
                {
                    continue;
                }
                team.response_latch_83 = false;
            }

            // TeamClass::AI @ 0x006E917B calls FUN_006EA3E0 whenever +0x7D
            // remains set. A positive member count clears +0x7D/+0x7E and
            // records +0x78 once the TaskForce's signed total is reached. An
            // empty Team that has reached that state is deleted and returns 0;
            // an empty +0x78==0 Team retains both latches and returns 1.
            if team.response_latch_7d {
                if team.members.is_empty() {
                    if team.reached_required_strength_78 {
                        deleted_teams.push(team.id);
                        continue;
                    }
                } else {
                    let required_count =
                        team.team_type_id
                            .and_then(|team_type_id| team_types.get(&team_type_id))
                            .and_then(|team_type| task_forces.get(&team_type.task_force_id))
                            .map(|task_force| {
                                task_force.entries.iter().fold(0i32, |total, entry| {
                                    total.wrapping_add(entry.count)
                                })
                            })
                            .unwrap_or(team.members.len() as i32);
                    if team.members.len() as i32 == required_count {
                        team.reached_required_strength_78 = true;
                    }
                    team.response_latch_7e = false;
                    team.response_latch_7d = false;
                }
            }

            if team.completed || team.refusal.is_some() || !owner_is_active(team.owner) {
                continue;
            }

            if team.advance_pending {
                team.advance_pending = false;
                team.cursor = team.cursor.wrapping_add(1);
                if !script_action_at(self.scripts.get(&team.script_id), team.cursor).is_some() {
                    team.completed = true;
                }
                continue;
            }

            if team.delay_remaining_frames > 0 {
                team.delay_remaining_frames -= 1;
                if team.delay_remaining_frames > 0 {
                    continue;
                }
            }

            let Some(script) = self.scripts.get(&team.script_id) else {
                team.refusal = Some(TeamScriptRefusal::MissingScript {
                    script_id: team.script_id,
                });
                log::warn!(
                    "TeamClass::AI stopped team {}: missing ScriptType {}",
                    team.id,
                    team.script_id
                );
                continue;
            };
            let Some(action) = script_action_at(Some(script), team.cursor) else {
                team.completed = true;
                continue;
            };

            match action.action_id {
                // TeamClass::AI default path for case 2: neither complete nor advance.
                2 => {}
                // TeamClass::AI cases 5 and 43 wait until their gate completes.
                5 | 43 => {
                    if team.wait_condition_complete {
                        team.wait_condition_complete = false;
                        team.advance_pending = true;
                    }
                }
                // TeamClass::AI case 6 calls SetMission(argument - 2), then advances next update.
                6 => {
                    team.cursor = action.argument.wrapping_sub(2);
                    team.advance_pending = true;
                }
                // TeamClass::AI case 19 invokes Foot's panic-family virtual in member-list order.
                19 => {
                    result.effects.extend(
                        team.members
                            .iter()
                            .copied()
                            .map(|entity_id| TeamScriptEffect::PanicMember { entity_id }),
                    );
                    team.advance_pending = true;
                }
                // TeamClass::AI case 24 takes the shared advance path.
                24 => team.advance_pending = true,
                // TeamClass::AI case 49 records success but its +0x84 byte is not CRC-covered.
                49 => {
                    team.succeeded = true;
                    team.advance_pending = true;
                }
                0..=64 => {
                    team.refusal = Some(TeamScriptRefusal::UnsupportedAction {
                        action_id: action.action_id,
                    });
                    log::warn!(
                        "TeamClass::AI stopped team {}: ScriptType action {} is not implemented",
                        team.id,
                        action.action_id
                    );
                }
                action_id => {
                    team.refusal = Some(TeamScriptRefusal::OutOfRangeAction { action_id });
                    log::warn!(
                        "TeamClass::AI stopped team {}: ScriptType action {} is outside 0..=64",
                        team.id,
                        action_id
                    );
                }
            }
        }

        for team_id in deleted_teams {
            self.teams.remove(&team_id);
        }

        result
    }

    pub(crate) fn hash_state(&self, current_frame: i32, hasher: &mut impl Hasher) {
        // TeamClass::ComputeCRC observes live Team/Script state, not the VM's
        // source registry or allocator. Action-49's +0x84 success flag is not
        // included by the captured YR 1.001 CRC sequence.
        self.teams.len().hash(hasher);
        for (id, team) in &self.teams {
            id.hash(hasher);
            team.team_type_id.hash(hasher);
            team.task_force_id.hash(hasher);
            team.script_id.hash(hasher);
            team.owner.hash(hasher);
            team.cursor.hash(hasher);
            team.advance_pending.hash(hasher);
            team.delay_remaining_frames.hash(hasher);
            // gamemd-derived: TeamClass CRC callback at vtable +0x34,
            // raw body 0x006EC5A0..0x006EC720, feeds one normalized remaining
            // time for +0x64/+0x6C before the +0x78/+0x7D/+0x7E/+0x83
            // bytes. It skips +0x68 and never feeds raw start.
            response_suspend_remaining(
                team.response_suspend_start_frame,
                team.response_suspend_duration_frames,
                current_frame,
            )
            .hash(hasher);
            team.reached_required_strength_78.hash(hasher);
            team.response_latch_7d.hash(hasher);
            team.response_latch_7e.hash(hasher);
            team.response_latch_83.hash(hasher);
            team.members.hash(hasher);
            team.target.hash(hasher);
            team.member_type_counts.hash(hasher);
        }
    }

    #[cfg(test)]
    fn insert_refused_team(
        &mut self,
        owner: InternedId,
        team_type_id: InternedId,
        refusal: TeamScriptRefusal,
        target: Option<u64>,
        current_frame: i32,
    ) -> u64 {
        let script_id = match refusal {
            TeamScriptRefusal::MissingScript { script_id } => script_id,
            _ => InternedId::from_index(0),
        };
        let id = self.insert_team(
            owner,
            Some(team_type_id),
            None,
            script_id,
            Vec::new(),
            BTreeMap::new(),
            target,
            current_frame,
        );
        self.teams.get_mut(&id).expect("inserted team").refusal = Some(refusal);
        id
    }

    #[cfg(test)]
    fn insert_team(
        &mut self,
        owner: InternedId,
        team_type_id: Option<InternedId>,
        task_force_id: Option<InternedId>,
        script_id: InternedId,
        members: Vec<u64>,
        member_type_counts: BTreeMap<TeamMemberTypeIdentity, u32>,
        target: Option<u64>,
        current_frame: i32,
    ) -> u64 {
        let id = self.next_team_id;
        self.next_team_id = self.next_team_id.wrapping_add(1);
        self.teams.insert(
            id,
            TeamScriptState {
                id,
                owner,
                team_type_id,
                task_force_id,
                script_id,
                cursor: 0,
                advance_pending: false,
                delay_remaining_frames: 0,
                wait_condition_complete: false,
                reached_required_strength_78: false,
                response_latch_7d: true,
                response_latch_7e: false,
                response_latch_83: false,
                response_suspend_start_frame: current_frame,
                response_suspend_duration_frames: 0,
                members,
                member_type_counts: member_type_counts.into_iter().collect(),
                target,
                succeeded: false,
                completed: false,
                refusal: None,
            },
        );
        id
    }
}

/// Native signed/wrapping remaining time for the Team base-defense suspension.
///
/// gamemd-derived: `TeamClass::AI @ 0x006E9140` and the Team CRC callback at
/// vtable `+0x34` (`0x006EC5A0`) share this exact `+0x64/+0x6C` calculation.
fn response_suspend_remaining(start_frame: i32, duration_frames: i32, now: i32) -> i32 {
    if start_frame == -1 {
        return duration_frames;
    }
    let elapsed = now.wrapping_sub(start_frame);
    if elapsed < duration_frames {
        duration_frames.wrapping_sub(elapsed)
    } else {
        0
    }
}

fn script_action_at(
    script: Option<&TeamScriptDefinition>,
    cursor: i32,
) -> Option<TeamScriptAction> {
    let index = usize::try_from(cursor).ok()?;
    script?.actions.get(index).copied()
}

/// Puts `member` in a one-member team whose TeamType has `Suicide=suicide`
/// and `Aggressive=aggressive`.
#[cfg(test)]
pub(crate) fn join_one_member_team_for_test(
    sim: &mut crate::sim::world::Simulation,
    member: u64,
    suicide: bool,
    aggressive: bool,
) {
    use crate::rules::object_type::ObjectCategory;
    use crate::rules::team_ai_ini::TeamAiDefinitionSource;
    use crate::sim::team_script_vm::{
        TeamMemberTypeIdentity, TeamScriptDefinition, TeamScriptMember, TeamTaskForceDefinition,
        TeamTaskForceEntry, TeamTypeDefinition,
    };
    let owner = sim.substrate.entities.get(member).unwrap().owner();
    let member_type = TeamMemberTypeIdentity {
        category: ObjectCategory::Vehicle,
        id: sim.substrate.entities.get(member).unwrap().type_ref(),
    };
    let script_id = sim.interner.intern("TEST_SCRIPT");
    let task_force_id = sim.interner.intern("TEST_TASK_FORCE");
    let team_type_id = sim.interner.intern("TEST_TEAM");
    let teams = &mut sim.team_script_vm;
    teams.register_script(TeamScriptDefinition {
        id: script_id,
        source: TeamAiDefinitionSource::FixedAimd,
        actions: Vec::new(),
    });
    teams.register_task_force(TeamTaskForceDefinition {
        id: task_force_id,
        source: TeamAiDefinitionSource::FixedAimd,
        group: -1,
        entries: vec![TeamTaskForceEntry {
            member_type,
            count: 1,
        }],
    });
    teams.register_team_type(TeamTypeDefinition {
        id: team_type_id,
        script_id,
        task_force_id,
        priority: 0,
        is_base_defense: false,
        suicide,
        aggressive,
        combined_movement_zone: crate::rules::locomotor_type::MovementZone::Normal,
        base_zone_relation_enforced: true,
        transport_crossing_required: false,
    });
    teams.create_team_from_type(
        owner,
        team_type_id,
        &[TeamScriptMember {
            entity_id: member,
            member_type,
        }],
        None,
        0,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;

    use crate::rules::ruleset::RuleSet;
    use crate::rules::team_ai_ini::TeamAiIniRegistry;
    use crate::sim::intern::StringInterner;

    fn action(action_id: i32, argument: i32) -> TeamScriptAction {
        TeamScriptAction {
            action_id,
            argument,
        }
    }

    fn state_hash_at(vm: &TeamScriptVm, current_frame: i32) -> u64 {
        let mut hasher = DefaultHasher::new();
        vm.hash_state(current_frame, &mut hasher);
        hasher.finish()
    }

    fn state_hash(vm: &TeamScriptVm) -> u64 {
        state_hash_at(vm, 0)
    }

    #[test]
    fn case_six_sets_argument_minus_two_then_advances_next_update() {
        let owner = InternedId::from_index(1);
        let script = InternedId::from_index(2);
        let mut vm = TeamScriptVm::default();
        vm.register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::FixedAimd,
            actions: vec![action(6, 2), action(2, 0)],
        });
        let team = vm.create_team(owner, script, vec![], None, 0);

        vm.tick_effects(1, |_| true);
        assert_eq!(vm.team(team).expect("team").cursor(), 0);
        assert!(vm.team(team).expect("team").advance_pending());

        vm.tick_effects(2, |_| true);
        assert_eq!(vm.team(team).expect("team").cursor(), 1);
        assert!(!vm.team(team).expect("team").advance_pending());
    }

    #[test]
    fn wait_actions_gate_then_use_the_common_advance() {
        let owner = InternedId::from_index(1);
        let script = InternedId::from_index(2);
        let mut vm = TeamScriptVm::default();
        vm.register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::FixedAimd,
            actions: vec![action(5, 2), action(43, 0)],
        });
        let team = vm.create_team(owner, script, vec![], None, 0);

        vm.tick_effects(10, |_| true);
        assert_eq!(vm.team(team).expect("team").cursor(), 0);
        assert!(!vm.team(team).expect("team").advance_pending());
        assert!(vm.set_wait_condition_complete(team, true));
        vm.tick_effects(11, |_| true);
        assert!(vm.team(team).expect("team").advance_pending());
        vm.tick_effects(12, |_| true);
        assert_eq!(vm.team(team).expect("team").cursor(), 1);
    }

    #[test]
    fn action_nineteen_preserves_member_order_and_defers_advance() {
        let owner = InternedId::from_index(1);
        let script = InternedId::from_index(2);
        let mut vm = TeamScriptVm::default();
        vm.register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::FixedAimd,
            actions: vec![action(19, 0), action(2, 0)],
        });
        let team = vm.create_team(owner, script, vec![9, 3, 7], None, 0);

        let tick = vm.tick_effects(1, |_| true);
        assert_eq!(
            tick.effects,
            vec![
                TeamScriptEffect::PanicMember { entity_id: 9 },
                TeamScriptEffect::PanicMember { entity_id: 3 },
                TeamScriptEffect::PanicMember { entity_id: 7 },
            ]
        );
        assert!(vm.team(team).expect("team").advance_pending());
        vm.tick_effects(2, |_| true);
        assert_eq!(vm.team(team).expect("team").cursor(), 1);
    }

    #[test]
    fn success_flag_is_persisted_but_not_crc_projected() {
        let owner = InternedId::from_index(1);
        let script = InternedId::from_index(2);
        let mut vm = TeamScriptVm::default();
        vm.register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::FixedAimd,
            actions: vec![action(49, 0)],
        });
        let team = vm.create_team(owner, script, vec![], None, 0);
        vm.tick_effects(1, |_| true);

        let mut without_success = vm.clone();
        without_success
            .teams
            .get_mut(&team)
            .expect("team")
            .succeeded = false;
        assert_eq!(state_hash(&vm), state_hash(&without_success));
        assert!(vm.team(team).expect("team").succeeded());
    }

    #[test]
    fn unsupported_and_out_of_range_actions_never_advance() {
        let owner = InternedId::from_index(1);
        let supported = InternedId::from_index(2);
        let out_of_range = InternedId::from_index(3);
        let mut vm = TeamScriptVm::default();
        vm.register_script(TeamScriptDefinition {
            id: supported,
            source: TeamAiDefinitionSource::FixedAimd,
            actions: vec![action(0, 0)],
        });
        vm.register_script(TeamScriptDefinition {
            id: out_of_range,
            source: TeamAiDefinitionSource::FixedAimd,
            actions: vec![action(65, 0)],
        });
        let first = vm.create_team(owner, supported, vec![], None, 0);
        let second = vm.create_team(owner, out_of_range, vec![], None, 0);

        vm.tick_effects(1, |_| true);
        assert_eq!(
            vm.team(first).expect("team").refusal(),
            Some(TeamScriptRefusal::UnsupportedAction { action_id: 0 })
        );
        assert_eq!(
            vm.team(second).expect("team").refusal(),
            Some(TeamScriptRefusal::OutOfRangeAction { action_id: 65 })
        );
    }

    #[test]
    fn task_force_admission_uses_entry_then_candidate_order() {
        let owner = InternedId::from_index(1);
        let script = InternedId::from_index(2);
        let task_force = InternedId::from_index(3);
        let team_type = InternedId::from_index(4);
        let tank = InternedId::from_index(5);
        let infantry = InternedId::from_index(6);
        let tank_identity = TeamMemberTypeIdentity {
            category: ObjectCategory::Vehicle,
            id: tank,
        };
        let infantry_identity = TeamMemberTypeIdentity {
            category: ObjectCategory::Infantry,
            id: infantry,
        };
        let mut vm = TeamScriptVm::default();
        vm.register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::FixedAimd,
            actions: vec![action(2, 0)],
        });
        vm.register_task_force(TeamTaskForceDefinition {
            id: task_force,
            source: TeamAiDefinitionSource::FixedAimd,
            group: -1,
            entries: vec![
                TeamTaskForceEntry {
                    member_type: infantry_identity,
                    count: 2,
                },
                TeamTaskForceEntry {
                    member_type: tank_identity,
                    count: 1,
                },
            ],
        });
        vm.register_team_type(TeamTypeDefinition {
            id: team_type,
            script_id: script,
            task_force_id: task_force,
            priority: 0,
            is_base_defense: false,
            suicide: false,
            aggressive: false,
            combined_movement_zone: MovementZone::Fly,
            base_zone_relation_enforced: true,
            transport_crossing_required: false,
        });
        let team = vm.create_team_from_type(
            owner,
            team_type,
            &[
                TeamScriptMember {
                    entity_id: 10,
                    member_type: tank_identity,
                },
                TeamScriptMember {
                    entity_id: 20,
                    member_type: infantry_identity,
                },
                TeamScriptMember {
                    entity_id: 30,
                    member_type: infantry_identity,
                },
            ],
            None,
            0,
        );

        let state = vm.team(team).expect("team");
        assert_eq!(state.members(), &[20, 30, 10]);
        assert!(state.member_type_counts().contains(&(infantry_identity, 2)));
        assert!(state.member_type_counts().contains(&(tank_identity, 1)));
    }

    #[test]
    fn gsi_04_05_task_force_admission_distinguishes_duplicate_native_families() {
        let owner = InternedId::from_index(1);
        let script = InternedId::from_index(2);
        let task_force = InternedId::from_index(3);
        let team_type = InternedId::from_index(4);
        let duplicate_id = InternedId::from_index(5);
        let infantry_identity = TeamMemberTypeIdentity {
            category: ObjectCategory::Infantry,
            id: duplicate_id,
        };
        let vehicle_identity = TeamMemberTypeIdentity {
            category: ObjectCategory::Vehicle,
            id: duplicate_id,
        };
        let mut vm = TeamScriptVm::default();
        vm.register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::FixedAimd,
            actions: vec![action(2, 0)],
        });
        vm.register_task_force(TeamTaskForceDefinition {
            id: task_force,
            source: TeamAiDefinitionSource::FixedAimd,
            group: -1,
            entries: vec![TeamTaskForceEntry {
                member_type: infantry_identity,
                count: 1,
            }],
        });
        vm.register_team_type(TeamTypeDefinition {
            id: team_type,
            script_id: script,
            task_force_id: task_force,
            priority: 0,
            is_base_defense: false,
            suicide: false,
            aggressive: false,
            combined_movement_zone: MovementZone::Fly,
            base_zone_relation_enforced: true,
            transport_crossing_required: false,
        });

        let team = vm.create_team_from_type(
            owner,
            team_type,
            &[
                TeamScriptMember {
                    entity_id: 10,
                    member_type: vehicle_identity,
                },
                TeamScriptMember {
                    entity_id: 20,
                    member_type: infantry_identity,
                },
            ],
            None,
            0,
        );

        assert_eq!(
            vm.team(team).unwrap().members(),
            &[20],
            "same-name Unit candidate must not satisfy an Infantry pointer requirement"
        );
    }

    #[test]
    fn gsi_04_05_base_defense_suspension_uses_signed_priority_and_ordered_removal() {
        fn install(
            vm: &mut TeamScriptVm,
            owner: InternedId,
            ordinal: u32,
            priority: i32,
            is_base_defense: bool,
            members: &[u64],
        ) -> u64 {
            let script = InternedId::from_index(100);
            let member_type = InternedId::from_index(101);
            let member_identity = TeamMemberTypeIdentity {
                category: ObjectCategory::Infantry,
                id: member_type,
            };
            let task_force = InternedId::from_index(110 + ordinal);
            let team_type = InternedId::from_index(120 + ordinal);
            if !vm.scripts.contains_key(&script) {
                vm.register_script(TeamScriptDefinition {
                    id: script,
                    source: TeamAiDefinitionSource::FixedAimd,
                    actions: vec![action(2, 0)],
                });
            }
            vm.register_task_force(TeamTaskForceDefinition {
                id: task_force,
                source: TeamAiDefinitionSource::FixedAimd,
                group: -1,
                entries: vec![TeamTaskForceEntry {
                    member_type: member_identity,
                    count: members.len() as i32,
                }],
            });
            vm.register_team_type(TeamTypeDefinition {
                id: team_type,
                script_id: script,
                task_force_id: task_force,
                priority,
                is_base_defense,
                suicide: false,
                aggressive: false,
                combined_movement_zone: MovementZone::Fly,
                base_zone_relation_enforced: !is_base_defense,
                transport_crossing_required: false,
            });
            let candidates = members
                .iter()
                .copied()
                .map(|entity_id| TeamScriptMember {
                    entity_id,
                    member_type: member_identity,
                })
                .collect::<Vec<_>>();
            vm.create_team_from_type(owner, team_type, &candidates, None, 0)
        }

        let owner = InternedId::from_index(1);
        let other_owner = InternedId::from_index(2);
        let mut vm = TeamScriptVm::default();
        let low = install(&mut vm, owner, 0, 0, false, &[10, 20]);
        let high_base_defense = install(&mut vm, owner, 1, 1, true, &[30]);
        let other = install(&mut vm, other_owner, 2, -7, false, &[40]);
        assert_eq!(vm.team_for_member(30), Some((high_base_defense, true)));

        let before = state_hash(&vm);
        let removed = vm.suspend_teams_for_base_defense(owner, 1, -9, 1800);
        assert_eq!(removed, vec![10, 20]);
        assert!(vm.team(low).unwrap().members().is_empty());
        assert!(vm.team(low).unwrap().member_type_counts().is_empty());
        assert_eq!(
            vm.team(low).unwrap().response_suspension_state(),
            (true, true, true, -9, 1800)
        );
        assert_eq!(vm.team(high_base_defense).unwrap().members(), &[30]);
        assert_eq!(vm.team(other).unwrap().members(), &[40]);
        assert_eq!(vm.team_for_member(10), None);
        assert_ne!(before, state_hash(&vm));

        let restored: TeamScriptVm =
            serde_json::from_str(&serde_json::to_string(&vm).unwrap()).unwrap();
        assert_eq!(
            restored.team(low).unwrap().response_suspension_state(),
            (true, true, true, -9, 1800)
        );
        assert_eq!(state_hash(&restored), state_hash(&vm));
    }

    #[test]
    fn gsi_04_05_response_suspend_remaining_matches_native_signed_boundaries() {
        assert_eq!(response_suspend_remaining(100, 3, 100), 3);
        assert_eq!(response_suspend_remaining(100, 3, 102), 1);
        assert_eq!(response_suspend_remaining(100, 3, 103), 0);
        assert_eq!(response_suspend_remaining(100, 0, 100), 0);
        assert_eq!(response_suspend_remaining(100, -7, 100), 0);
        assert_eq!(response_suspend_remaining(-1, 9, i32::MAX), 9);
        assert_eq!(response_suspend_remaining(-1, -7, i32::MIN), -7);
        assert_eq!(response_suspend_remaining(i32::MAX - 1, 5, i32::MIN), 3);
    }

    #[test]
    fn gsi_04_05_team_constructor_uses_native_response_latch_defaults() {
        let owner = InternedId::from_index(1);
        let script = InternedId::from_index(2);
        let mut vm = TeamScriptVm::default();
        vm.register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::FixedAimd,
            actions: vec![action(2, 0)],
        });

        let team = vm.create_team(owner, script, vec![], None, -19);
        let state = vm.team(team).unwrap();
        assert!(!state.reached_required_strength_78);
        assert_eq!(
            state.response_suspension_state(),
            (true, false, false, -19, 0)
        );
    }

    #[test]
    fn gsi_04_05_suspension_blocks_before_other_gates_and_resumes_on_equality() {
        let owner = InternedId::from_index(1);
        let script = InternedId::from_index(2);
        let task_force = InternedId::from_index(3);
        let team_type = InternedId::from_index(4);
        let mut vm = TeamScriptVm::default();
        vm.register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::FixedAimd,
            actions: vec![action(24, 0), action(2, 0)],
        });
        vm.register_task_force(TeamTaskForceDefinition {
            id: task_force,
            source: TeamAiDefinitionSource::FixedAimd,
            group: -1,
            entries: vec![],
        });
        vm.register_team_type(TeamTypeDefinition {
            id: team_type,
            script_id: script,
            task_force_id: task_force,
            priority: 0,
            is_base_defense: false,
            suicide: false,
            aggressive: false,
            combined_movement_zone: MovementZone::Fly,
            base_zone_relation_enforced: true,
            transport_crossing_required: false,
        });
        let team = vm.create_team_from_type(owner, team_type, &[], None, 0);
        vm.suspend_teams_for_base_defense(owner, 1, 100, 3);

        for frame in [100, 101, 102] {
            vm.tick_effects(frame, |_| {
                panic!("native timer must precede later Team gates")
            });
            let state = vm.team(team).unwrap();
            assert!(!state.advance_pending());
            assert_eq!(
                state.response_suspension_state(),
                (true, true, true, 100, 3)
            );
        }

        let encoded = serde_json::to_string(&vm).unwrap();
        let mut restored: TeamScriptVm = serde_json::from_str(&encoded).unwrap();
        restored.tick_effects(103, |_| true);
        let state = restored.team(team).unwrap();
        assert!(
            state.advance_pending(),
            "expiry resumes in the equality pass"
        );
        assert_eq!(
            state.response_suspension_state(),
            (true, true, false, 100, 3),
            "timer clears only +0x83"
        );
    }

    #[test]
    fn gsi_04_05_rearming_restarts_the_full_response_delay() {
        let owner = InternedId::from_index(1);
        let script = InternedId::from_index(2);
        let task_force = InternedId::from_index(3);
        let team_type = InternedId::from_index(4);
        let member_type = InternedId::from_index(5);
        let member_identity = TeamMemberTypeIdentity {
            category: ObjectCategory::Infantry,
            id: member_type,
        };
        let mut vm = TeamScriptVm::default();
        vm.register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::FixedAimd,
            actions: vec![action(24, 0)],
        });
        vm.register_task_force(TeamTaskForceDefinition {
            id: task_force,
            source: TeamAiDefinitionSource::FixedAimd,
            group: -1,
            entries: vec![TeamTaskForceEntry {
                member_type: member_identity,
                count: 1,
            }],
        });
        vm.register_team_type(TeamTypeDefinition {
            id: team_type,
            script_id: script,
            task_force_id: task_force,
            priority: 0,
            is_base_defense: false,
            suicide: false,
            aggressive: false,
            combined_movement_zone: MovementZone::Fly,
            base_zone_relation_enforced: true,
            transport_crossing_required: false,
        });
        let team = vm.create_team_from_type(
            owner,
            team_type,
            &[TeamScriptMember {
                entity_id: 10,
                member_type: member_identity,
            }],
            None,
            99,
        );

        vm.tick_effects(99, |_| true);
        let admitted = vm.team(team).unwrap();
        assert!(admitted.reached_required_strength_78);
        assert_eq!(
            admitted.response_suspension_state(),
            (false, false, false, 99, 0)
        );

        assert_eq!(
            vm.suspend_teams_for_base_defense(owner, 1, 100, 5),
            vec![10]
        );
        vm.tick_effects(103, |_| panic!("first delay remains active"));
        assert!(
            vm.suspend_teams_for_base_defense(owner, 1, 103, 5)
                .is_empty()
        );
        let mut without_reached = vm.clone();
        without_reached
            .teams
            .get_mut(&team)
            .unwrap()
            .reached_required_strength_78 = false;
        assert_ne!(
            state_hash_at(&vm, 103),
            state_hash_at(&without_reached, 103),
            "native CRC feeds +0x78"
        );

        let encoded = serde_json::to_string(&vm).unwrap();
        let mut restored: TeamScriptVm = serde_json::from_str(&encoded).unwrap();
        assert_eq!(state_hash_at(&vm, 103), state_hash_at(&restored, 103));
        restored.tick_effects(107, |_| panic!("rearmed delay remains active"));
        restored.tick_effects(108, |_| true);

        assert!(
            restored.team(team).is_none(),
            "empty +0x78 Team is deleted by the mandatory +0x7D helper at expiry"
        );
    }

    #[test]
    fn gsi_04_05_team_hash_normalizes_response_timer_to_remaining_frames() {
        let owner = InternedId::from_index(1);
        let script = InternedId::from_index(2);
        let mut first = TeamScriptVm::default();
        first.register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::FixedAimd,
            actions: vec![action(2, 0)],
        });
        let team = first.create_team(owner, script, vec![], None, 0);
        let mut second = first.clone();

        {
            let state = first.teams.get_mut(&team).unwrap();
            state.response_latch_7d = true;
            state.response_latch_7e = true;
            state.response_latch_83 = true;
            state.response_suspend_start_frame = 90;
            state.response_suspend_duration_frames = 20;
        }
        {
            let state = second.teams.get_mut(&team).unwrap();
            state.response_latch_7d = true;
            state.response_latch_7e = true;
            state.response_latch_83 = true;
            state.response_suspend_start_frame = 95;
            state.response_suspend_duration_frames = 15;
        }

        assert_eq!(state_hash_at(&first, 100), state_hash_at(&second, 100));
        second
            .teams
            .get_mut(&team)
            .unwrap()
            .response_suspend_duration_frames = 16;
        assert_ne!(state_hash_at(&first, 100), state_hash_at(&second, 100));
    }

    #[test]
    fn gsi_04_05_aimd_install_preserves_registry_order_and_creates_no_teams() {
        use crate::rules::ini_parser::IniFile;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[Countries]\n0=British\n[InfantryTypes]\n0=E1\n[E1]\nStrength=100\nTechLevel=1\n",
        ))
        .expect("minimal rules");
        let comparison = "0100000003000000000000000000000000000000000000000000000000000000";
        let fixed = IniFile::from_str(&format!(
            "[TeamTypes]\n0=TT1\n1=TT2\n\
             [TT1]\nScript=S1\nTaskForce=TF1\nPriority=5\nAutocreate=yes\nIsBaseDefense=yes\n\
             [TT2]\nScript=S1\nTaskForce=TF1\nPriority=7\n\
             [ScriptTypes]\n0=S1\n[S1]\n0=-1,9\n\
             [TaskForces]\n0=TF1\n[TF1]\n0=-2,E1\nGroup=3\n\
             [AITriggerTypes]\nAT=Trigger,TT1,British,2,4,E1,{comparison},40,10,40,1,0,1,0,TT2,1,0,1\n"
        ));
        let map = IniFile::from_str(
            "[TaskForces]\n0=TF1\n[TF1]\n0=-2,E1\nGroup=9\n",
        );
        let registry = TeamAiIniRegistry::from_sources(&fixed, &map, true);
        let mut interner = StringInterner::new();
        let (vm, diagnostics) =
            TeamScriptVm::from_ini_registry(&registry, &mut interner, &rules);

        assert!(diagnostics.is_empty());
        assert_eq!(vm.registry_counts(), (1, 1, 2, 1));
        assert_eq!(
            vm.team_type_order()
                .iter()
                .map(|id| interner.resolve(*id))
                .collect::<Vec<_>>(),
            ["TT1", "TT2"]
        );
        assert_eq!(
            vm.script_order()
                .iter()
                .map(|id| interner.resolve(*id))
                .collect::<Vec<_>>(),
            ["S1"]
        );
        assert_eq!(
            vm.task_force_order()
                .iter()
                .map(|id| interner.resolve(*id))
                .collect::<Vec<_>>(),
            ["TF1"]
        );
        assert_eq!(
            vm.ai_trigger_order()
                .iter()
                .map(|id| interner.resolve(*id))
                .collect::<Vec<_>>(),
            ["AT"]
        );
        assert!(vm.teams.is_empty(), "definition ingress must not create Teams");

        let tt1 = interner.get("TT1").unwrap();
        let metadata = vm.team_type_ini(tt1).expect("TeamType metadata");
        assert_eq!(metadata.max_teams, -1);
        assert!(metadata.autocreate);
        assert!(metadata.are_team_members_recruitable);
        assert_eq!(vm.task_forces[&interner.get("TF1").unwrap()].entries[0].count, -2);
        assert_eq!(
            vm.task_forces[&interner.get("TF1").unwrap()].group,
            9,
            "map TaskForce re-read must preserve its signed Group in the resolved registry"
        );
        assert_eq!(vm.scripts[&interner.get("S1").unwrap()].actions[0].action_id, -1);
        assert_eq!(
            vm.scripts[&interner.get("S1").unwrap()].source,
            TeamAiDefinitionSource::FixedAimd
        );
        assert_eq!(
            vm.task_forces[&interner.get("TF1").unwrap()].source,
            TeamAiDefinitionSource::Scenario
        );
        let trigger = vm
            .ai_trigger(interner.get("AT").unwrap())
            .expect("typed AITrigger");
        assert_eq!(trigger.display_name, "Trigger");
        assert_eq!(trigger.primary_team_type, Some(tt1));
        assert_eq!(
            trigger.owner,
            Some(TeamAiTriggerOwner::Country(CountryIdx(0)))
        );
        assert_eq!(trigger.tokens[3], "2");
        assert_eq!(trigger.threshold, 1);
        assert_eq!(trigger.condition, 4);
        assert_eq!(
            trigger.object_type,
            Some(TeamMemberTypeIdentity {
                category: ObjectCategory::Infantry,
                id: interner.get("E1").unwrap(),
            })
        );
        assert_eq!(&trigger.comparison_mask[..5], &[1, 0, 0, 0, 3]);
        assert_eq!(
            trigger.weights,
            [
                NativeF64Bits::from_bits(40.0_f64.to_bits()),
                NativeF64Bits::from_bits(10.0_f64.to_bits()),
                NativeF64Bits::from_bits(40.0_f64.to_bits()),
            ]
        );
        assert!(trigger.storage_flag_d0);
        assert_eq!(trigger.storage_i32_ac, 1);
        assert!(!trigger.storage_flag_d1);
        assert_eq!(trigger.secondary_team_type, interner.get("TT2"));
        assert_eq!(trigger.difficulty_enabled, [true, false, true]);

        let encoded = serde_json::to_string(&vm).unwrap();
        let restored: TeamScriptVm = serde_json::from_str(&encoded).unwrap();
        assert_eq!(restored.registry_counts(), vm.registry_counts());
        assert_eq!(restored.team_type_order(), vm.team_type_order());
        assert_eq!(restored.ai_trigger_order(), vm.ai_trigger_order());
        assert_eq!(
            restored.ai_trigger(interner.get("AT").unwrap()),
            vm.ai_trigger(interner.get("AT").unwrap())
        );
        assert_eq!(restored.task_force(interner.get("TF1").unwrap()).unwrap().group, 9);
        assert_eq!(
            restored.scripts[&interner.get("S1").unwrap()].source,
            TeamAiDefinitionSource::FixedAimd
        );
        assert_eq!(
            restored
                .task_force(interner.get("TF1").unwrap())
                .unwrap()
                .source,
            TeamAiDefinitionSource::Scenario
        );
    }

    #[test]
    fn gsi_04_05_ai_trigger_threshold_folds_member_tech_levels_in_slot_order() {
        use crate::rules::ini_parser::IniFile;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n1=GGI\n2=HIGH\n3=HIDDEN\n4=MISSING\n\
             [E1]\nStrength=100\nTechLevel=1\n\
             [GGI]\nStrength=100\nTechLevel=5\n\
             [HIGH]\nStrength=100\nTechLevel=12\n\
             [HIDDEN]\nStrength=100\nTechLevel=-1\n\
             [MISSING]\nStrength=100\n",
        ))
        .expect("minimal rules");
        let comparison = "00".repeat(32);
        let fixed = IniFile::from_str(&format!(
            "[TeamTypes]\n0=PRIMARY\n1=DOWN\n2=UP\n3=DEFAULTED\n\
             [PRIMARY]\nScript=S\nTaskForce=PRIMARY_TF\n\
             [DOWN]\nScript=S\nTaskForce=DOWN_TF\n\
             [UP]\nScript=S\nTaskForce=UP_TF\n\
             [DEFAULTED]\nScript=S\nTaskForce=DEFAULTED_TF\n\
             [ScriptTypes]\n0=S\n[S]\n0=2,0\n\
             [TaskForces]\n0=PRIMARY_TF\n1=DOWN_TF\n2=UP_TF\n3=DEFAULTED_TF\n\
             [PRIMARY_TF]\n0=99,E1\n1=77,GGI\n\
             [DOWN_TF]\n0=1,HIGH\n1=1,HIDDEN\n\
             [UP_TF]\n0=1,HIDDEN\n1=1,HIGH\n\
             [DEFAULTED_TF]\n0=1,MISSING\n\
             [AITriggerTypes]\n\
             AT=Threshold down,PRIMARY,<all>,99,0,<none>,{comparison},1,1,1,1,0,1,0,DOWN,1,1,1\n\
             AT2=Threshold up,UP,<all>,99,0,<none>,{comparison},1,1,1,1,0,1,0,<none>,1,1,1\n\
             AT3=Missing default,DEFAULTED,<all>,99,0,<none>,{comparison},1,1,1,1,0,1,0,<none>,1,1,1\n"
        ));
        let registry = TeamAiIniRegistry::from_sources(&fixed, &IniFile::from_str(""), true);
        let mut interner = StringInterner::new();
        let (vm, diagnostics) =
            TeamScriptVm::from_ini_registry(&registry, &mut interner, &rules);

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let trigger = vm.ai_trigger(interner.get("AT").unwrap()).unwrap();
        assert_eq!(trigger.tokens[3], "99");
        assert_eq!(
            trigger.threshold, 11,
            "a later -1 TechLevel replaces even a prior 12 with 11 in nonzero game modes"
        );
        assert_eq!(
            vm.ai_trigger(interner.get("AT2").unwrap())
                .unwrap()
                .threshold,
            12,
            "slot order remains load-bearing because a later 12 raises the -1 sentinel's 11"
        );
        assert_eq!(
            vm.ai_trigger(interner.get("AT3").unwrap())
                .unwrap()
                .threshold,
            255,
            "an omitted TechLevel keeps the native constructor value in nonzero modes"
        );

        let registry = TeamAiIniRegistry::from_sources(&fixed, &IniFile::from_str(""), false);
        let mut interner = StringInterner::new();
        let (vm, diagnostics) =
            TeamScriptVm::from_ini_registry(&registry, &mut interner, &rules);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            vm.ai_trigger(interner.get("AT").unwrap())
                .unwrap()
                .threshold,
            12,
            "TechLevel=-1 is ignored when g_GameMode is zero"
        );
        assert_eq!(
            vm.ai_trigger(interner.get("AT3").unwrap())
                .unwrap()
                .threshold,
            255,
            "the missing-key constructor value is mode-independent"
        );
    }

    #[test]
    fn gsi_04_05_team_type_zone_fields_derive_from_final_resolved_task_force() {
        use crate::rules::ini_parser::IniFile;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n1=DUP\n\
             [VehicleTypes]\n0=TANK\n1=BOAT\n2=ODDTRANSPORT\n3=DUP\n\
             [BuildingTypes]\n0=BLD\n1=DUP\n\
             [E1]\nStrength=100\nMovementZone=Infantry\n\
             [DUP]\nStrength=100\nTechLevel=6\nMovementZone=Infantry\n\
             [TANK]\nStrength=100\nMovementZone=Normal\n\
             [BOAT]\nStrength=100\nMovementZone=Water\nNaval=yes\nPassengers=0\n\
             [ODDTRANSPORT]\nStrength=100\nMovementZone=Amphibious\nNaval=yes\nPassengers=-3\n\
             [BLD]\nStrength=100\nMovementZone=Normal\n",
        ))
        .expect("zone derivation rules");
        let fixed = IniFile::from_str(
            "[TeamTypes]\n0=EMPTY\n1=COUNTS\n2=PURE_NAVAL\n3=TRANSPORT\n4=BASE\n5=INVALID\n6=BUILDING\n7=DUPLICATE\n\
             [EMPTY]\nScript=S\nTaskForce=EMPTY_TF\n\
             [COUNTS]\nScript=S\nTaskForce=COUNTS_TF\n\
             [PURE_NAVAL]\nScript=S\nTaskForce=NAV_TF\n\
             [TRANSPORT]\nScript=S\nTaskForce=TRANSPORT_TF\n\
             [BASE]\nScript=S\nTaskForce=TRANSPORT_TF\nIsBaseDefense=yes\n\
             [INVALID]\nScript=S\nTaskForce=INVALID_TF\n\
             [BUILDING]\nScript=S\nTaskForce=BUILDING_TF\n\
             [DUPLICATE]\nScript=S\nTaskForce=DUP_TF\n\
             [ScriptTypes]\n0=S\n[S]\n0=2,0\n\
             [TaskForces]\n0=EMPTY_TF\n1=COUNTS_TF\n2=NAV_TF\n3=TRANSPORT_TF\n4=INVALID_TF\n5=BUILDING_TF\n6=DUP_TF\n\
             [EMPTY_TF]\nGroup=-1\n\
             [COUNTS_TF]\n0=0,E1\n1=-7,TANK\n\
             [NAV_TF]\n0=1,BOAT\n\
             [TRANSPORT_TF]\n0=1,ODDTRANSPORT\n\
             [INVALID_TF]\n0=1,TANK\n1=1,BOAT\n2=1,E1\n\
             [BUILDING_TF]\n0=1,BLD\n\
             [DUP_TF]\n0=1,DUP\n",
        );
        let registry = TeamAiIniRegistry::from_sources(&fixed, &IniFile::from_str(""), true);
        let mut interner = StringInterner::new();
        let (vm, diagnostics) =
            TeamScriptVm::from_ini_registry(&registry, &mut interner, &rules);

        assert_eq!(
            diagnostics,
            vec![TeamAiInstallDiagnostic::UnknownTaskForceMember {
                task_force_id: "BUILDING_TF".to_string(),
                member_type: "BLD".to_string(),
                source: TeamAiDefinitionSource::FixedAimd,
            }],
            "native TaskForce lookup never searches BuildingTypeClass"
        );
        let definition = |name: &str| vm.team_type(interner.get(name).unwrap()).copied().unwrap();
        let empty = definition("EMPTY");
        assert_eq!(empty.combined_movement_zone, MovementZone::Fly);
        assert!(empty.base_zone_relation_enforced);
        assert!(!empty.transport_crossing_required);

        let counts = definition("COUNTS");
        assert_eq!(counts.combined_movement_zone, MovementZone::Normal);
        assert!(counts.base_zone_relation_enforced);
        assert_eq!(
            vm.task_force(interner.get("COUNTS_TF").unwrap())
                .unwrap()
                .entries
                .iter()
                .map(|entry| entry.count)
                .collect::<Vec<_>>(),
            [0, -7],
            "signed authored counts are retained but do not gate the native fold"
        );

        let pure_naval = definition("PURE_NAVAL");
        assert_eq!(pure_naval.combined_movement_zone, MovementZone::Water);
        assert!(!pure_naval.base_zone_relation_enforced);
        assert!(!pure_naval.transport_crossing_required);

        let transport = definition("TRANSPORT");
        assert_eq!(transport.combined_movement_zone, MovementZone::Amphibious);
        assert!(transport.base_zone_relation_enforced);
        assert!(transport.transport_crossing_required);

        let base = definition("BASE");
        assert_eq!(base.combined_movement_zone, MovementZone::Amphibious);
        assert!(!base.base_zone_relation_enforced);
        assert!(
            base.transport_crossing_required,
            "IsBaseDefense overrides only +0xF0 after the member fold"
        );

        let invalid = definition("INVALID");
        assert_eq!(invalid.combined_movement_zone, MovementZone::Invalid);
        assert!(
            !invalid.base_zone_relation_enforced,
            "the pure-naval member still disables enforcement before the fold becomes invalid"
        );
        assert!(!invalid.transport_crossing_required);

        let building = definition("BUILDING");
        assert_eq!(building.combined_movement_zone, MovementZone::Fly);
        assert!(
            vm.task_force(interner.get("BUILDING_TF").unwrap())
                .unwrap()
                .entries
                .is_empty()
        );

        assert_eq!(
            rules.object("DUP").unwrap().category,
            crate::rules::object_type::ObjectCategory::Building,
            "the broad lookup preserves its later-registry winner"
        );
        assert_eq!(
            rules.task_force_member_object("DUP").unwrap().category,
            crate::rules::object_type::ObjectCategory::Infantry,
            "TaskForce resolution stops on the first native family"
        );
        let duplicate = definition("DUPLICATE");
        assert_eq!(duplicate.combined_movement_zone, MovementZone::Infantry);
        let duplicate_task_force = vm
            .task_force(interner.get("DUP_TF").unwrap())
            .unwrap();
        assert_eq!(duplicate_task_force.entries.len(), 1);
        let duplicate_identity = TeamMemberTypeIdentity {
            category: ObjectCategory::Infantry,
            id: interner.get("DUP").unwrap(),
        };
        assert_eq!(
            duplicate_task_force.entries[0].member_type,
            duplicate_identity,
            "the first searched native family is retained instead of an ambiguous name"
        );

        let encoded = serde_json::to_string(&vm).unwrap();
        let restored: TeamScriptVm = serde_json::from_str(&encoded).unwrap();
        assert_eq!(
            restored
                .task_force(interner.get("DUP_TF").unwrap())
                .unwrap()
                .entries[0]
                .member_type,
            duplicate_identity,
            "category-distinct TaskForce identity survives registry serialization"
        );
    }

    #[test]
    fn gsi_04_05_ai_trigger_object_retains_first_native_family_across_roundtrip() {
        use crate::rules::ini_parser::IniFile;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=DUP\n\
             [VehicleTypes]\n0=DUP\n\
             [AircraftTypes]\n0=DUP\n\
             [BuildingTypes]\n0=DUP\n\
             [DUP]\nStrength=100\nTechLevel=1\n",
        ))
        .expect("duplicate-family rules");
        let comparison = "00".repeat(32);
        let fixed = IniFile::from_str(&format!(
            "[AITriggerTypes]\n\
             AT=Duplicate,<none>,<all>,0,0,DUP,{comparison},1,1,1,1,0,1,0,<none>,1,1,1\n"
        ));
        let registry = TeamAiIniRegistry::from_sources(&fixed, &IniFile::from_str(""), true);
        let mut interner = StringInterner::new();
        let (vm, diagnostics) = TeamScriptVm::from_ini_registry(&registry, &mut interner, &rules);

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            rules.object("DUP").unwrap().category,
            ObjectCategory::Building,
            "the broad RuleSet lookup keeps its later-registry overlay winner"
        );
        assert_eq!(
            rules.ai_trigger_object("DUP").unwrap().category,
            ObjectCategory::Infantry,
            "native AITrigger lookup stops on the first matching family"
        );
        let expected = TeamMemberTypeIdentity {
            category: ObjectCategory::Infantry,
            id: interner.get("DUP").unwrap(),
        };
        let trigger_id = interner.get("AT").unwrap();
        assert_eq!(
            vm.ai_trigger(trigger_id).unwrap().object_type,
            Some(expected)
        );

        let restored: TeamScriptVm =
            serde_json::from_str(&serde_json::to_string(&vm).unwrap()).unwrap();
        assert_eq!(
            restored.ai_trigger(trigger_id).unwrap().object_type,
            Some(expected),
            "category-distinct AITrigger identity survives registry serialization"
        );
    }

    #[test]
    fn gsi_04_05_aimd_install_retains_unfilled_reference_placeholders_without_diagnostics() {
        use crate::rules::ini_parser::IniFile;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\n",
        ))
        .expect("minimal rules");
        let fixed = IniFile::from_str(
            "[TeamTypes]\n0=TT\n[TT]\nScript=MISSING_SCRIPT\nTaskForce=MISSING_TF\n\
             [ScriptTypes]\n0=FIRST_SCRIPT\n[FIRST_SCRIPT]\n0=2,0\n\
             [TaskForces]\n0=FIRST_TF\n[FIRST_TF]\n0=1,E1\n",
        );
        let registry = TeamAiIniRegistry::from_sources(&fixed, &IniFile::from_str(""), true);
        let mut interner = StringInterner::new();
        let (vm, diagnostics) =
            TeamScriptVm::from_ini_registry(&registry, &mut interner, &rules);

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let team_type = &vm.team_types[&interner.get("TT").unwrap()];
        assert_eq!(team_type.script_id, interner.get("MISSING_SCRIPT").unwrap());
        assert_eq!(team_type.task_force_id, interner.get("MISSING_TF").unwrap());
        assert!(
            vm.scripts[&team_type.script_id].actions.is_empty(),
            "a valid unlisted Script remains the native empty placeholder"
        );
        assert!(
            vm.task_forces[&team_type.task_force_id].entries.is_empty(),
            "a valid unlisted TaskForce remains the native empty placeholder"
        );
        assert_eq!(
            vm.script_order()
                .iter()
                .map(|id| interner.resolve(*id))
                .collect::<Vec<_>>(),
            ["MISSING_SCRIPT", "FIRST_SCRIPT"]
        );
        assert_eq!(
            vm.task_force_order()
                .iter()
                .map(|id| interner.resolve(*id))
                .collect::<Vec<_>>(),
            ["MISSING_TF", "FIRST_TF"]
        );
        assert!(vm.teams.is_empty());
    }

    #[test]
    fn gsi_04_05_task_force_admission_omits_unknown_members() {
        use crate::rules::ini_parser::IniFile;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\n",
        ))
        .expect("minimal rules");
        let fixed = IniFile::from_str(
            "[TaskForces]\n0=PARTIAL_TF\n[PARTIAL_TF]\n0=1,E1\n1=2,GHOST\n",
        );
        let registry = TeamAiIniRegistry::from_sources(&fixed, &IniFile::from_str(""), true);
        let mut interner = StringInterner::new();
        let (vm, diagnostics) = TeamScriptVm::from_ini_registry(&registry, &mut interner, &rules);

        assert_eq!(
            diagnostics,
            [TeamAiInstallDiagnostic::UnknownTaskForceMember {
                task_force_id: "PARTIAL_TF".to_string(),
                member_type: "GHOST".to_string(),
                source: TeamAiDefinitionSource::FixedAimd,
            }]
        );
        assert_eq!(
            vm.task_forces[&interner.get("PARTIAL_TF").unwrap()]
                .entries
                .len(),
            1,
            "unresolved TechnoTypes do not increment the native TaskForce entry count"
        );
    }

    #[test]
    fn gsi_04_05_aimd_install_fills_placeholders_without_reordering_first_references() {
        use crate::rules::ini_parser::IniFile;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\n",
        ))
        .expect("minimal rules");
        let fixed = IniFile::from_str(
            "[TeamTypes]\n0=BASE\n[BASE]\nScript=BASE_SCRIPT\nTaskForce=BASE_TF\n\
             [ScriptTypes]\n0=BASE_SCRIPT\n[BASE_SCRIPT]\n0=2,0\n\
             [TaskForces]\n0=BASE_TF\n[BASE_TF]\n0=1,E1\n",
        );
        let map = IniFile::from_str(
            "[TeamTypes]\n0=MAP_EARLIER\n1=BASE\n\
             [MAP_EARLIER]\nScript=EARLIER_SCRIPT\nTaskForce=EARLIER_TF\n\
             [BASE]\nScript=LATER_SCRIPT\nTaskForce=LATER_TF\n\
             [ScriptTypes]\n0=LATER_SCRIPT\n1=EARLIER_SCRIPT\n\
             [EARLIER_SCRIPT]\n0=11,1\n[LATER_SCRIPT]\n0=22,2\n\
             [TaskForces]\n0=LATER_TF\n1=EARLIER_TF\n\
             [EARLIER_TF]\n0=1,E1\n[LATER_TF]\n0=2,E1\n",
        );
        let registry = TeamAiIniRegistry::from_sources(&fixed, &map, true);
        let mut interner = StringInterner::new();
        let (vm, diagnostics) = TeamScriptVm::from_ini_registry(&registry, &mut interner, &rules);

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            vm.script_order()
                .iter()
                .map(|id| interner.resolve(*id))
                .collect::<Vec<_>>(),
            ["BASE_SCRIPT", "EARLIER_SCRIPT", "LATER_SCRIPT"],
            "fixed then map TeamType source order owns placeholders even when final identity and ScriptTypes orders differ"
        );
        assert_eq!(
            vm.task_force_order()
                .iter()
                .map(|id| interner.resolve(*id))
                .collect::<Vec<_>>(),
            ["BASE_TF", "EARLIER_TF", "LATER_TF"],
            "fixed then map TeamType source order owns placeholders even when final identity and TaskForces orders differ"
        );

        let later = vm.team_types[&interner.get("BASE").unwrap()];
        assert_eq!(later.script_id, interner.get("LATER_SCRIPT").unwrap());
        assert_eq!(later.task_force_id, interner.get("LATER_TF").unwrap());
        assert_eq!(vm.scripts[&later.script_id].actions[0].action_id, 22);
        assert_eq!(vm.task_forces[&later.task_force_id].entries[0].count, 2);
        assert_eq!(vm.registry_counts(), (3, 3, 2, 0));
    }

    #[test]
    fn gsi_04_05_team_type_none_attachments_use_case_insensitive_first_fallback() {
        use crate::rules::ini_parser::IniFile;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\n",
        ))
        .expect("minimal rules");
        let fixed = IniFile::from_str(
            "[TeamTypes]\n0=FIRST\n1=FALLBACK\n\
             [FIRST]\nScript=FIRST_SCRIPT\nTaskForce=FIRST_TF\n\
             [FALLBACK]\nScript=<NONE>\nTaskForce=NONE\n\
             [ScriptTypes]\n0=FIRST_SCRIPT\n[FIRST_SCRIPT]\n0=2,0\n\
             [TaskForces]\n0=FIRST_TF\n[FIRST_TF]\n0=1,E1\n",
        );
        let registry = TeamAiIniRegistry::from_sources(&fixed, &IniFile::from_str(""), true);
        let mut interner = StringInterner::new();
        let (vm, diagnostics) = TeamScriptVm::from_ini_registry(&registry, &mut interner, &rules);

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let first = vm.team_types[&interner.get("FIRST").unwrap()];
        let fallback = vm.team_types[&interner.get("FALLBACK").unwrap()];
        assert_eq!(fallback.script_id, first.script_id);
        assert_eq!(fallback.task_force_id, first.task_force_id);
        assert_eq!(
            vm.script_order()
                .iter()
                .map(|id| interner.resolve(*id))
                .collect::<Vec<_>>(),
            ["FIRST_SCRIPT"]
        );
        assert_eq!(
            vm.task_force_order()
                .iter()
                .map(|id| interner.resolve(*id))
                .collect::<Vec<_>>(),
            ["FIRST_TF"]
        );
        assert!(interner.get("<NONE>").is_none());
        assert!(interner.get("NONE").is_none());
    }

    #[test]
    fn gsi_04_05_team_type_none_attachments_refuse_only_when_registry_is_empty() {
        use crate::rules::ini_parser::IniFile;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\n",
        ))
        .expect("minimal rules");
        let fixed = IniFile::from_str(
            "[TeamTypes]\n0=ONLY\n[ONLY]\nScript=NONE\nTaskForce=<NONE>\n",
        );
        let registry = TeamAiIniRegistry::from_sources(&fixed, &IniFile::from_str(""), true);
        let mut interner = StringInterner::new();
        let (vm, diagnostics) = TeamScriptVm::from_ini_registry(&registry, &mut interner, &rules);

        assert_eq!(
            diagnostics,
            [
                TeamAiInstallDiagnostic::MissingTeamTypeScript {
                    team_type_id: "ONLY".to_string(),
                    script_id: "NONE".to_string(),
                    source: TeamAiDefinitionSource::FixedAimd,
                },
                TeamAiInstallDiagnostic::MissingTeamTypeTaskForce {
                    team_type_id: "ONLY".to_string(),
                    task_force_id: "<NONE>".to_string(),
                    source: TeamAiDefinitionSource::FixedAimd,
                },
            ]
        );
        assert!(vm.script_order().is_empty());
        assert!(vm.task_force_order().is_empty());
    }

    #[test]
    #[ignore = "requires extracted retail rulesmd.ini and aimd.ini"]
    fn gsi_04_05_retail_aimd_installs_all_resolved_definitions_without_teams() {
        use crate::rules::ini_parser::IniFile;

        let aimd_path = std::env::var_os("VERA20K_RETAIL_AIMD")
            .expect("set VERA20K_RETAIL_AIMD to extracted retail aimd.ini");
        let rules_path = std::env::var_os("VERA20K_RETAIL_RULESMD")
            .expect("set VERA20K_RETAIL_RULESMD to extracted retail rulesmd.ini");
        let aimd_bytes = std::fs::read(aimd_path).expect("read aimd.ini");
        let rules_bytes = std::fs::read(rules_path).expect("read rulesmd.ini");
        assert_eq!(
            crate::util::sha256::sha256_hex(&aimd_bytes),
            "5df41eaec00a78d0760ef5eecdf27d65ae1cd537309c7eac973318266986f89d"
        );
        assert_eq!(
            crate::util::sha256::sha256_hex(&rules_bytes),
            "3d341ef8a13a4b5ab24af2eef48ac94931ac2bb87d950fe3330a07e2d25672ef"
        );
        let aimd = IniFile::from_bytes(&aimd_bytes).expect("parse aimd.ini");
        let rules_ini = IniFile::from_bytes(&rules_bytes).expect("parse rulesmd.ini");
        let rules = RuleSet::from_ini(&rules_ini).expect("load retail rules");
        let registry = TeamAiIniRegistry::from_sources(&aimd, &IniFile::from_str(""), true);
        let mut interner = StringInterner::new();
        let (vm, diagnostics) =
            TeamScriptVm::from_ini_registry(&registry, &mut interner, &rules);

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(vm.registry_counts(), (132, 88, 163, 165));
        assert_eq!(
            vm.team_type_ini
                .values()
                .filter(|metadata| metadata.autocreate)
                .count(),
            163
        );
        assert_eq!(
            vm.team_types
                .values()
                .filter(|team_type| team_type.is_base_defense)
                .count(),
            12
        );
        let zone_rows = vm
            .team_type_order()
            .iter()
            .map(|id| {
                let team_type = vm.team_type(*id).expect("ordered TeamType");
                format!(
                    "{}\t{}\t{}\t{}\n",
                    interner.resolve(*id),
                    team_type.combined_movement_zone as i8,
                    u8::from(team_type.base_zone_relation_enforced),
                    u8::from(team_type.transport_crossing_required),
                )
            })
            .collect::<String>();
        assert_eq!(zone_rows.lines().count(), 163);
        assert_eq!(
            vm.team_types
                .values()
                .filter(|team_type| !team_type.base_zone_relation_enforced)
                .count(),
            19
        );
        assert_eq!(
            vm.team_types
                .values()
                .filter(|team_type| team_type.transport_crossing_required)
                .count(),
            0
        );
        assert_eq!(
            crate::util::sha256::sha256_hex(zone_rows.as_bytes()),
            "1274426ac3e9ce7adc7d4babab8bd9a04b61519cca02a720b0d0a38cd22da2e1",
            "all stock TeamType post-load zone fields must match the native oracle"
        );
        assert_eq!(
            vm.ai_triggers
                .values()
                .filter(|trigger| { matches!(trigger.owner, Some(TeamAiTriggerOwner::Country(_))) })
                .count(),
            10
        );
        assert_eq!(
            vm.ai_triggers
                .values()
                .filter(|trigger| trigger.object_type.is_some())
                .count(),
            109
        );
        assert_eq!(
            vm.ai_triggers
                .values()
                .filter(|trigger| trigger.secondary_team_type.is_some())
                .count(),
            49
        );
        let anti_nuke = vm
            .ai_trigger(interner.get("0CAD0DCC-G").expect("stock trigger identity"))
            .expect("resolved stock Allied Anti-Nuke trigger");
        assert_eq!(anti_nuke.display_name, "Allied Anti-Nuke 1");
        assert_eq!(anti_nuke.owner, Some(TeamAiTriggerOwner::All));
        assert_eq!(anti_nuke.tokens[3], "9");
        assert_eq!(anti_nuke.threshold, 9);
        assert_eq!(anti_nuke.condition, 0);
        assert_eq!(
            anti_nuke.object_type,
            Some(TeamMemberTypeIdentity {
                category: ObjectCategory::Building,
                id: interner.get("NAMISL").unwrap(),
            })
        );
        assert_eq!(&anti_nuke.comparison_mask[..5], &[1, 0, 0, 0, 3]);
        assert_eq!(
            anti_nuke.weights,
            [
                NativeF64Bits::from_bits(70.0_f64.to_bits()),
                NativeF64Bits::from_bits(10.0_f64.to_bits()),
                NativeF64Bits::from_bits(70.0_f64.to_bits()),
            ]
        );
        assert!(anti_nuke.storage_flag_d0);
        assert_eq!(anti_nuke.storage_i32_ac, 1);
        assert!(!anti_nuke.storage_flag_d1);
        assert_eq!(anti_nuke.difficulty_enabled, [false, true, true]);
        assert!(vm.teams.is_empty());

        let threshold_rows = vm
            .ai_trigger_order()
            .iter()
            .map(|id| {
                let trigger = vm.ai_trigger(*id).expect("ordered AITrigger");
                format!("{}\t{}\n", interner.resolve(*id), trigger.threshold)
            })
            .collect::<String>();
        assert_eq!(threshold_rows.lines().count(), 165);
        assert_eq!(
            crate::util::sha256::sha256_hex(
                threshold_rows.as_bytes()
            ),
            "76096bc2d9592ff4c1054c23a38660e74c3860afbc2882db5c6dcc2074da8aad",
            "every game-mode-nonzero retail AITrigger threshold must match the native oracle"
        );

        let zero_mode_registry =
            TeamAiIniRegistry::from_sources(&aimd, &IniFile::from_str(""), false);
        let mut zero_mode_interner = StringInterner::new();
        let (zero_mode_vm, zero_mode_diagnostics) = TeamScriptVm::from_ini_registry(
            &zero_mode_registry,
            &mut zero_mode_interner,
            &rules,
        );
        assert!(zero_mode_diagnostics.is_empty(), "{zero_mode_diagnostics:?}");
        let zero_mode_rows = zero_mode_vm
            .ai_trigger_order()
            .iter()
            .map(|id| {
                let trigger = zero_mode_vm.ai_trigger(*id).expect("ordered AITrigger");
                format!(
                    "{}\t{}\n",
                    zero_mode_interner.resolve(*id),
                    trigger.threshold
                )
            })
            .collect::<String>();
        assert_eq!(zero_mode_rows.lines().count(), 165);
        assert_eq!(
            crate::util::sha256::sha256_hex(
                zero_mode_rows.as_bytes()
            ),
            "3253b17c65d2006bf542c38a811ec68ef2847e588dc1f21165e7070af5d5e1f7",
            "every game-mode-zero retail AITrigger threshold must match the native oracle"
        );
        let mode_sensitive = "0C8C51BC-G";
        assert_eq!(
            zero_mode_vm
                .ai_trigger(zero_mode_interner.get(mode_sensitive).unwrap())
                .unwrap()
                .threshold,
            5
        );
        assert_eq!(
            vm.ai_trigger(interner.get(mode_sensitive).unwrap())
                .unwrap()
                .threshold,
            11
        );
    }
}
