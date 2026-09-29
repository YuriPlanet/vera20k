//! Computer teams (`TeamClass`) and their scripts (`ScriptClass`), with the
//! TeamType, TaskForce, ScriptType and AI trigger definitions they are made
//! from, resolved at the scenario boundary (`registry_install`).
//!
//! The VM owns every live team, from its construction (`TeamTypeClass::
//! Create_Team @ 0x006F09C0`, called by the computer's team creation in
//! `sim::ai_team_creation`) to its destruction (`TeamClass::~TeamClass @
//! 0x006E8DE0`), which feeds the team's outcome back into the weight of every
//! AI trigger whose first TeamType it is; it also owns those AI triggers'
//! running weights. It owns each team's member list (`+0x54`) and, derived
//! from those, each member's team (`FootClass+0x5D4`).
//!
//! - `membership`: `Can_Add`, `Add_Member`, `Remove_Member` and `Recruit`,
//!   with the destructor's member release and the base-defense suspension.
//! - `team_ai`: each team's update (`TeamClass::AI @ 0x006E9140`) up to its
//!   script step, with `Recalc`, `Calc_Center`, `Regroup`, the
//!   under-strength retreat and a member's damage report.
//! - `actions`: the script step and the ported script actions.
//! - `orders`: the member virtuals team code calls.
//!
//! Each submodule lists its residuals. The TeamType's Tag (`+0xD0`, created
//! by the constructor `0x006E4DE0`) is not ported; no retail TeamType names
//! one.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::rules::locomotor_type::MovementZone;
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::{CountryIdx, GeneralRules};
use crate::rules::team_ai_ini::TeamAiDefinitionSource;
use crate::sim::intern::InternedId;
use crate::sim::timer::CdTimer;
use crate::util::native_x87::{NativeF64Bits, X87Chop53, X87Ordering};

mod actions;
mod membership;
mod orders;
mod registry_install;
mod team_ai;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

/// The TeamType's other INI fields, beside the definition, and where it was
/// defined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamTypeIniMetadata {
    /// `Max=` (`+0xB8`, constructor -1): a house's team limit of this type;
    /// negative is none.
    pub max_teams: i32,
    /// `Autocreate=` (`+0xA9`). The computer's team creation also sets it on
    /// every TeamType it picks (`0x006F0E51`); only recruitment reads it.
    pub autocreate: bool,
    pub are_team_members_recruitable: bool,
    /// `Reinforce=` (`+0xAB`, ReadBool at `0x006F11FB`, constructor 0): a
    /// formed team of the type recruits while it is not full (`0x006E9235`),
    /// and the unit choosers count it as needing members until it fills
    /// (`0x004FEC26`).
    #[serde(default)]
    reinforce: bool,
    /// `Group=` (`+0x9C`, ReadInteger at `0x006F136C`, constructor -1): the
    /// team's group, else its TaskForce's (`TeamTypeClass::Get_Group @
    /// 0x006F1870`).
    #[serde(default = "minus_one")]
    group: i32,
    /// `Recruiter=` (`+0xA8`, ReadBool at `0x006F1249`): recruitment passes
    /// over the group filter.
    #[serde(default)]
    recruiter: bool,
    /// `Annoyance=` (`+0xA6`, ReadBool at `0x006F1193`): a formed team
    /// regroups when a member is hit (`0x006EB416`).
    #[serde(default)]
    annoyance: bool,
    /// `GuardSlower=` (`+0xA7`, ReadBool at `0x006F11AD`): the centre counts
    /// slow members twice (`0x006EB24F`).
    #[serde(default)]
    guard_slower: bool,
    /// `TransportsReturnOnUnload=` (`+0xF4`, ReadBool at `0x006F13EE`): a
    /// transport member keeps its ArchiveTarget as the script advances
    /// (`0x006E9393`).
    #[serde(default)]
    transports_return_on_unload: bool,
    /// `MindControlDecision=` (`+0xC0`, ReadInt at `0x006F1139`, constructor
    /// 0 at `0x006F0781`): when not 0, the fate a member's mind control
    /// gives its captives (`CaptureManagerClass::DecideUnitFate @
    /// 0x00472586`), in place of the roll.
    #[serde(default)]
    mind_control_decision: i32,
    /// `Droppod=` (`+0xB0`, ReadBool at `0x006F1215`, constructor 0 at
    /// `0x006F0753`): a member still in limbo keeps `Coordinate_Attack`
    /// busy (`0x006EB830..0x006EB847`).
    #[serde(default)]
    droppod: bool,
    /// `OnlyTargetHouseEnemy=` (`+0xF7`, ReadBool at `0x006F13D4`,
    /// constructor 0 at `0x006F07F7`): action 0's scan takes only the
    /// house's current enemy's objects.
    #[serde(default)]
    only_target_house_enemy: bool,
    pub source: TeamAiDefinitionSource,
}

const fn minus_one() -> i32 {
    -1
}

impl Default for TeamTypeIniMetadata {
    fn default() -> Self {
        Self {
            max_teams: -1,
            autocreate: false,
            are_team_members_recruitable: true,
            reinforce: false,
            group: -1,
            recruiter: false,
            annoyance: false,
            guard_slower: false,
            transports_return_on_unload: false,
            mind_control_decision: 0,
            droppod: false,
            only_target_house_enemy: false,
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

/// One resolved AITriggerType record: every field the raw reader at
/// `0x0041F580` stores, plus the lossless 18-token source. The computer's
/// team creation (`sim::ai_team_creation`) reads it; its running weight is an
/// [`AiTriggerTrackRecord`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamAiTriggerDefinition {
    pub id: InternedId,
    pub tokens: [String; 18],
    pub display_name: String,
    /// `+0xA4`.
    pub enabled: bool,
    /// `+0xDC`, the first TeamType.
    pub primary_team_type: Option<InternedId>,
    /// `+0xA0`/`+0xA8`; `None` is mode 0, which never qualifies.
    pub owner: Option<TeamAiTriggerOwner>,
    /// `+0xB0`, the house TechLevel it needs.
    pub threshold: i32,
    /// `+0x98`.
    pub condition: i32,
    /// `+0xD8`, the type the condition counts.
    pub object_type: Option<TeamMemberTypeIdentity>,
    /// `+0xE4..`: the condition's amount (`+0xE4`) and comparator (`+0xE8`)
    /// lead the 32 bytes.
    pub comparison_mask: [u8; 32],
    /// The initial weight (`+0xB8`), its minimum (`+0xC0`) and maximum
    /// (`+0xC8`).
    pub weights: [NativeF64Bits; 3],
    /// `+0xD0`.
    pub multiplayer: bool,
    /// `+0xAC`.
    pub side: i32,
    pub storage_flag_d1: bool,
    /// `+0xE0`, the second TeamType.
    pub secondary_team_type: Option<InternedId>,
    /// `+0xD2`, `+0xD3`, `+0xD4`.
    pub difficulty_enabled: [bool; 3],
    pub source: TeamAiDefinitionSource,
}

/// An AI trigger's running track record: its weight (`+0xB8`, which the
/// raw reader writes and team destruction adjusts), and how many of its teams
/// succeeded (`+0x104`) and ended (`+0x108`); both counts start at 0 in the
/// constructor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct AiTriggerTrackRecord {
    weight: NativeF64Bits,
    successes: i32,
    attempts: i32,
}

impl AiTriggerTrackRecord {
    const fn new(weight: NativeF64Bits) -> Self {
        Self {
            weight,
            successes: 0,
            attempts: 0,
        }
    }

    pub(crate) const fn weight(&self) -> NativeF64Bits {
        self.weight
    }

    /// `AITriggerTypeClass::RegisterSuccess @ 0x0041FD60` (`succeeded`) or
    /// `RegisterFailure @ 0x0041FE20`, both in PC53/chop:
    /// - success: `w' = (SuccessWeightDelta + w) + max(0, n * (s/n - 0.5))`,
    ///   then `s` and `n` grow by one;
    /// - failure: `w' = (FailureWeightDelta + w) + min(0, n * ((s/n - 0.5) *
    ///   TrackRecordCoefficient))`, then `n` grows by one;
    ///
    /// where `n` is the attempts and `s` the successes (the adjustment is 0
    /// while `n <= 0`), and `w'` is stored as a double and clamped: below
    /// `min` it becomes `min`, above `max` it becomes `max`
    /// (`TEST AH,0x41`, so an unordered compare keeps it).
    fn record(&mut self, succeeded: bool, bounds: [NativeF64Bits; 2], rules: &TeamRules) {
        type X = X87Chop53;
        let load = |bits: NativeF64Bits| {
            X::load_f64(bits).expect("AI trigger weights and Rules deltas are finite")
        };
        let zero = X::load_i32(0);
        let mut adjustment = zero;
        if self.attempts > 0 {
            let attempts = X::load_i32(self.attempts);
            let ratio =
                X::div(X::load_i32(self.successes), attempts).expect("attempts is positive");
            let mut record = X::sub(ratio, load(NativeF64Bits::HALF));
            if !succeeded {
                record = X::mul(record, load(rules.track_record_coefficient));
            }
            let product = X::mul(attempts, record);
            // `FCOM 0.0`: success keeps a product that is not below zero
            // (`TEST AH,0x1`), failure one that is below or equal
            // (`TEST AH,0x41`).
            let keep = match X::compare(product, zero) {
                X87Ordering::Less => !succeeded,
                X87Ordering::Equal => true,
                X87Ordering::Greater => succeeded,
            };
            if keep {
                adjustment = product;
            }
        }
        let delta = if succeeded {
            rules.success_weight_delta
        } else {
            rules.failure_weight_delta
        };
        let weight = X::add(X::add(load(delta), load(self.weight)), adjustment);
        self.weight = X::store_f64(weight).expect("an AI trigger weight stays finite");
        let [minimum, maximum] = bounds;
        if X::compare(load(self.weight), load(minimum)) == X87Ordering::Less {
            self.weight = minimum;
        }
        if X::compare(load(self.weight), load(maximum)) == X87Ordering::Greater {
            self.weight = maximum;
        }
        if succeeded {
            self.successes = self.successes.wrapping_add(1);
        }
        self.attempts = self.attempts.wrapping_add(1);
    }
}

/// What `TeamClass` reads beyond its own object: the game mode and the Rules
/// keys of the empty-team dissolve and the AI trigger feedback.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TeamRules {
    game_mode_nonzero: bool,
    /// `[General] DissolveUnfilledTeamDelay=` (`Rules+0x1190`).
    dissolve_unfilled_team_delay: i32,
    /// `[General] AITriggerSuccessWeightDelta=` (`Rules+0xC0`).
    success_weight_delta: NativeF64Bits,
    /// `[General] AITriggerFailureWeightDelta=` (`Rules+0xC8`).
    failure_weight_delta: NativeF64Bits,
    /// `[General] AITriggerTrackRecordCoefficient=` (`Rules+0xD0`).
    track_record_coefficient: NativeF64Bits,
}

impl TeamRules {
    pub(crate) fn new(general: &GeneralRules, game_mode_nonzero: bool) -> Self {
        Self {
            game_mode_nonzero,
            dissolve_unfilled_team_delay: general.dissolve_unfilled_team_delay,
            success_weight_delta: general.ai_trigger_success_weight_delta,
            failure_weight_delta: general.ai_trigger_failure_weight_delta,
            track_record_coefficient: general.ai_trigger_track_record_coefficient,
        }
    }
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

/// A candidate member passed to a test seam's TaskForce admission.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamScriptMember {
    pub entity_id: u64,
    pub member_type: TeamMemberTypeIdentity,
}

/// The first unported action a team's script reached, kept for diagnostics.
/// The team stays on it, running everything else `TeamClass::AI` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TeamScriptRefusal {
    UnsupportedAction { action_id: i32 },
}

/// One entry of a team's member list (`TeamClass+0x54`, linked through
/// `FootClass+0x5D8`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TeamMember {
    id: u64,
    /// `FootClass+0x689`: the member has joined up. `Add_Member` sets it only
    /// on a team's first member (`0x006EA554..0x006EA55C`); the forming step,
    /// `Regroup` and the move routines set it once the member is within
    /// `Stray=` of the team's centre. Only team code reads it, and the Foot
    /// CRC; the byte outlives a removal natively, which nothing reads.
    initiated: bool,
}

/// An `AbstractClass*` a team keeps: its centre (`+0x34`), mission target
/// (`+0x3C`) or move target (`+0x40`). A cell is the `CellClass` at those
/// coordinates; natively an out-of-map lookup gives the shared dummy cell,
/// whose coordinates a later miss restamps (not represented: no ported path
/// takes a team off the map).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TeamTarget {
    Cell { x: i16, y: i16 },
    Object(u64),
}

impl TeamTarget {
    /// The target as a member's TarCom (`+0x2B4`) holds it.
    pub(crate) const fn target_kind(self) -> crate::sim::combat::TargetKind {
        match self {
            Self::Cell { x, y } => crate::sim::combat::TargetKind::Cell(x as u16, y as u16),
            Self::Object(id) => crate::sim::combat::TargetKind::Entity(id),
        }
    }
}

/// Persistent `TeamClass` state (constructor `0x006E8A90`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamScriptState {
    id: u64,
    owner: InternedId,
    team_type_id: Option<InternedId>,
    task_force_id: Option<InternedId>,
    script_id: InternedId,
    /// `ScriptClass+0x2C`, the current action; -1 before the first.
    cursor: i32,
    /// `+0x80`, StepCompleted: the next update moves to the next action and
    /// runs it (`0x006E9364..0x006E9440`).
    advance_pending: bool,
    /// `+0x78`, IsHasBeen: the team has been at full strength.
    has_been_full: bool,
    /// `+0x7D`, IsAltered: its members changed, so the next update runs
    /// `Recalc` (`0x006E917B`). Constructor set.
    altered: bool,
    /// `+0x7E`, set and cleared with `+0x7D`.
    just_altered: bool,
    /// `+0x83`, suspended by the base-defense response (`0x006EC250`) until
    /// its timer (`+0x64`/`+0x6C`) runs out.
    suspended: bool,
    suspend_timer: CdTimer,
    /// `+0x54`, head first: `Add_Member` prepends.
    members: Vec<TeamMember>,
    /// `+0x88`: the members counted against each TaskForce entry, which
    /// `Add_Member` raises for the first entry of the member's type.
    slot_counts: [i32; 6],
    /// `+0x84`: the team succeeded (script action 49). Its destruction then
    /// counts as its AI triggers' success.
    succeeded: bool,
    refusal: Option<TeamScriptRefusal>,
    /// `+0x50`, the construction frame: the unfilled-team dissolve clock and
    /// the unit choosers' earliest team.
    created_frame: i32,
    /// `+0x7F`, IsMoving: the team has formed and runs its script.
    formed: bool,
    /// `+0x79`, IsFullStrength: it has as many members as its TaskForce.
    full_strength: bool,
    /// `+0x7A`, IsUnderStrength. Constructor set.
    under_strength: bool,
    /// `+0x7B`, IsReforming: it regroups before its script goes on.
    reforming: bool,
    /// `+0x82`, IsLeavingMap: its mission target is a cell off the playfield.
    leaving_map: bool,
    /// `+0x34`, Zone: the team's centre.
    zone: Option<TeamTarget>,
    /// `+0x38`, ClosestMember.
    closest_member: Option<u64>,
    /// `+0x3C`, the mission target; only `Assign_Mission_Target` writes it.
    mission_target: Option<TeamTarget>,
    /// `+0x40`, the target: `Coordinate_Move` moves the members to it,
    /// `Coordinate_Attack` has them attack it.
    focus: Option<TeamTarget>,
    /// `+0x58`, script action 5's guard timer.
    guard_timer: CdTimer,
    /// `+0x81`: `Coordinate_Attack` found the leader unable to fire at the
    /// target (`0x006EB5D2`); the next step runs the action afresh without
    /// one (`0x006E940F..0x006E942F`).
    #[serde(default)]
    retarget: bool,
}

impl TeamScriptState {
    pub fn id(&self) -> u64 {
        self.id
    }

    /// `TeamClass::AI`'s empty-team test after the recruit loop
    /// (`0x006E929B..0x006E92D8`): with no members (`+0x54`), a team that has
    /// been full (`+0x78`) or, in a multiplayer game, one whose age
    /// (`Frame - +0x50`, wrapping) is above `DissolveUnfilledTeamDelay=`
    /// (signed) is destroyed.
    fn dissolves(&self, rules: &TeamRules, current_frame: i32) -> bool {
        self.members.is_empty()
            && (self.has_been_full
                || (rules.game_mode_nonzero
                    && current_frame.wrapping_sub(self.created_frame)
                        > rules.dissolve_unfilled_team_delay))
    }

    pub fn owner(&self) -> InternedId {
        self.owner
    }

    pub fn team_type_id(&self) -> Option<InternedId> {
        self.team_type_id
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

    /// The member ids in list order, head (the newest) first.
    pub fn members(&self) -> impl Iterator<Item = u64> + '_ {
        self.members.iter().map(|member| member.id)
    }

    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    #[cfg(test)]
    pub(crate) fn response_suspension_state(&self) -> (bool, bool, bool, i32, i32) {
        (
            self.altered,
            self.just_altered,
            self.suspended,
            self.suspend_timer.start_frame(),
            self.suspend_timer.duration(),
        )
    }

    pub fn succeeded(&self) -> bool {
        self.succeeded
    }

    pub fn refusal(&self) -> Option<TeamScriptRefusal> {
        self.refusal
    }

    pub fn formed(&self) -> bool {
        self.formed
    }

    pub(crate) const fn created_frame(&self) -> i32 {
        self.created_frame
    }

    /// The unit choosers' filter (`0x004FEC20..0x004FEC47`): a team still
    /// wants members while its TeamType is `Reinforce=` and it is not full
    /// (`+0x79`), or while it is neither forced (`+0x77`, which only
    /// reinforcements set: none is ported) nor has been full (`+0x78`).
    pub(crate) const fn wants_members(&self, reinforce: bool) -> bool {
        (reinforce && !self.full_strength) || !self.has_been_full
    }

    /// The selector's cancel test (`0x006F0E05..0x006F0E11`): the team is
    /// still forming (`!+0x7F`) or regrouping (`+0x7B`).
    pub(crate) const fn is_forming(&self) -> bool {
        !self.formed || self.reforming
    }
}

/// Owns resolved TeamType/TaskForce/ScriptType definitions and live TeamClass
/// state.
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
    /// Live teams by id, which increases with construction, so key order is
    /// TeamClass::Array order.
    teams: BTreeMap<u64, TeamScriptState>,
    next_team_id: u64,
    /// Each AI trigger's running weight and track record, set at its
    /// registration.
    #[serde(default)]
    ai_trigger_records: BTreeMap<InternedId, AiTriggerTrackRecord>,
    /// Each member's team (`FootClass+0x5D4`), derived from the teams' member
    /// lists: only [`Self::link_member`] and [`Self::unlink_member`] (and the
    /// head drop in [`Self::pointer_expired`]) change either, and it is saved
    /// with them.
    #[serde(default)]
    member_team: BTreeMap<u64, u64>,
    /// Each object's team to rejoin (`TechnoClass+0x434`): a
    /// `RejoinTeamIfLimboed=` Foot's team as it LimboLaunches at an Infantry
    /// target (`TechnoClass::Fire @ 0x006FF7A3..0x006FF7E9`), which its
    /// parasite release re-adds it to (`0x0062A3AD..0x0062A3BC`,
    /// `0x0062A74A..0x0062A759`). Like the field it outlives the rejoin, and a
    /// jump outside a team keeps it; it goes with the team
    /// (`TechnoClass::PointerExpired @ 0x00707BB2`) or the object's
    /// destructor, not its Limbo or UnInit.
    #[serde(default)]
    rejoin_team: BTreeMap<u64, u64>,
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
        // A re-read writes the weight (`0x0041F8AC`) and keeps the counts.
        self.ai_trigger_records
            .entry(definition.id)
            .and_modify(|record| record.weight = definition.weights[0])
            .or_insert_with(|| AiTriggerTrackRecord::new(definition.weights[0]));
        self.ai_triggers.insert(definition.id, definition);
    }

    /// `TeamTypeClass::Create_Team @ 0x006F09C0` with a house, as the
    /// computer's team creation calls it (`0x004F8AAD`): refused while the
    /// TeamType's `Max=` is not negative and already reached, in a
    /// multiplayer game by `owner`'s teams of the type (`0x005095D0`), in a
    /// campaign by every house's (`+0xDC`, signed). Otherwise a new team
    /// (`TeamClass::TeamClass @ 0x006E8A90`) joins the end of the team list:
    /// empty and forming (`+0x7F` clear; `+0x7A`, `+0x7D` and `+0x80` set,
    /// the other bytes `+0x74..+0x84` clear), created at the current frame
    /// (`+0x50`, and the timers `+0x58`/`+0x64` start there with no time),
    /// its script (`0x006913C0`) before its first action.
    ///
    /// RESIDUAL: the constructor's centre is the TeamType's `Waypoint=` cell
    /// (`0x006E8D1A..0x006E8D58`); `Waypoint=` is not read, and no retail
    /// AIMD TeamType sets one, so every team starts without a centre. The
    /// TeamType's Tag (`+0xD0`, created by `0x006E4DE0`) is not ported either;
    /// no retail TeamType names one.
    pub(crate) fn construct_team(
        &mut self,
        team_type_id: InternedId,
        owner: InternedId,
        game_mode_nonzero: bool,
        current_frame: i32,
    ) -> Option<u64> {
        let team_type = *self.team_types.get(&team_type_id)?;
        if self.at_max_teams(team_type_id, owner, !game_mode_nonzero) {
            return None;
        }
        let id = self.next_team_id;
        self.next_team_id = self.next_team_id.wrapping_add(1);
        self.teams.insert(
            id,
            TeamScriptState::new(
                id,
                owner,
                Some(team_type_id),
                Some(team_type.task_force_id),
                team_type.script_id,
                current_frame,
            ),
        );
        Some(id)
    }

    /// The first half of `TeamClass::~TeamClass @ 0x006E8DE0`: every AI
    /// trigger, in registry order, whose first TeamType is the team's records
    /// the team's outcome (`+0x84`: success, else failure; see
    /// [`AiTriggerTrackRecord::record`]). The destructor then removes the
    /// members and the team (`membership::destroy_team`).
    fn record_trigger_outcome(&mut self, team_id: u64, rules: &TeamRules) {
        let Some(team) = self.teams.get(&team_id) else {
            return;
        };
        let (Some(team_type_id), succeeded) = (team.team_type_id, team.succeeded) else {
            return;
        };
        for trigger_id in &self.ai_trigger_order {
            let Some(trigger) = self.ai_triggers.get(trigger_id) else {
                continue;
            };
            if trigger.primary_team_type != Some(team_type_id) {
                continue;
            }
            let bounds = [trigger.weights[1], trigger.weights[2]];
            if let Some(record) = self.ai_trigger_records.get_mut(trigger_id) {
                record.record(succeeded, bounds, rules);
            }
        }
    }

    /// Live teams in TeamClass::Array order (their construction order).
    pub(crate) fn teams_in_order(&self) -> impl Iterator<Item = &TeamScriptState> {
        self.teams.values()
    }

    pub(crate) fn team_type_definition(&self, id: InternedId) -> Option<&TeamTypeDefinition> {
        self.team_types.get(&id)
    }

    /// Whether the TeamType's `Max=` (`+0xB8`; -1, none, for a TeamType
    /// without metadata), when not negative, is reached by `owner`'s teams of
    /// the type (`0x005095D0`), or with `every_house` by every house's (the
    /// campaign's `+0xDC`).
    pub(crate) fn at_max_teams(
        &self,
        team_type_id: InternedId,
        owner: InternedId,
        every_house: bool,
    ) -> bool {
        let max_teams = self
            .team_type_ini
            .get(&team_type_id)
            .map_or(-1, |metadata| metadata.max_teams);
        max_teams >= 0
            && self
                .teams
                .values()
                .filter(|team| {
                    team.team_type_id == Some(team_type_id) && (every_house || team.owner == owner)
                })
                .count() as i32
                >= max_teams
    }

    /// The `MindControlDecision=` of team `team_id`'s TeamType.
    pub(crate) fn mind_control_decision(&self, team_id: u64) -> Option<i32> {
        let team_type_id = self.teams.get(&team_id)?.team_type_id?;
        Some(self.team_type_ini.get(&team_type_id)?.mind_control_decision)
    }

    /// The TeamType's `Reinforce=` (`+0xAB`).
    pub(crate) fn reinforce(&self, team_type_id: InternedId) -> bool {
        self.team_type_ini
            .get(&team_type_id)
            .is_some_and(|metadata| metadata.reinforce)
    }

    /// The team creation's write of `Autocreate=` (`+0xA9`, `0x006F0E51`) on
    /// a TeamType it picked.
    pub(crate) fn mark_autocreate(&mut self, team_type_id: InternedId) {
        if let Some(metadata) = self.team_type_ini.get_mut(&team_type_id) {
            metadata.autocreate = true;
        }
    }

    /// `TeamClass::Get_Needed_Types @ 0x006EF4D0`: the team's TaskForce
    /// entries in order, each type `count` times, less the first remaining
    /// one of each member's type (`vt+0x84`, read live by `member_type`), in
    /// member-list order.
    pub(crate) fn needed_types(
        &self,
        team: &TeamScriptState,
        mut member_type: impl FnMut(u64) -> Option<TeamMemberTypeIdentity>,
    ) -> Vec<TeamMemberTypeIdentity> {
        let Some(task_force) = team
            .task_force_id
            .and_then(|task_force_id| self.task_forces.get(&task_force_id))
        else {
            return Vec::new();
        };
        let mut needed: Vec<TeamMemberTypeIdentity> = task_force
            .entries
            .iter()
            .flat_map(|entry| {
                std::iter::repeat_n(entry.member_type, usize::try_from(entry.count).unwrap_or(0))
            })
            .collect();
        for member in &team.members {
            let Some(member_type) = member_type(member.id) else {
                continue;
            };
            if let Some(position) = needed.iter().position(|&needed| needed == member_type) {
                needed.remove(position);
            }
        }
        needed
    }

    /// The TaskForce of TeamType `team_type_id`, if it resolved.
    pub(crate) fn task_force_of(
        &self,
        team_type_id: InternedId,
    ) -> Option<&TeamTaskForceDefinition> {
        let team_type = self.team_types.get(&team_type_id)?;
        self.task_forces.get(&team_type.task_force_id)
    }

    /// The AI triggers in AITriggerTypeClass::Array order, with their running
    /// weights.
    pub(crate) fn ai_triggers_in_order(
        &self,
    ) -> impl Iterator<Item = (&TeamAiTriggerDefinition, AiTriggerTrackRecord)> {
        self.ai_trigger_order.iter().filter_map(|id| {
            let definition = self.ai_triggers.get(id)?;
            let record = self
                .ai_trigger_records
                .get(id)
                .copied()
                .unwrap_or_else(|| AiTriggerTrackRecord::new(definition.weights[0]));
            Some((definition, record))
        })
    }

    pub fn team(&self, id: u64) -> Option<&TeamScriptState> {
        self.teams.get(&id)
    }

    /// `entity_id`'s team (`FootClass+0x5D4`) and whether its TeamType is
    /// `IsBaseDefense=` (`+0xF6`).
    pub(crate) fn team_for_member(&self, entity_id: u64) -> Option<(u64, bool)> {
        let team_id = *self.member_team.get(&entity_id)?;
        let is_base_defense = self
            .teams
            .get(&team_id)
            .and_then(|team| team.team_type_id)
            .and_then(|id| self.team_types.get(&id))
            .is_some_and(|definition| definition.is_base_defense);
        Some((team_id, is_base_defense))
    }

    /// `TechnoClass::Fire @ 0x006FF7A3..0x006FF7E9`: `entity_id` remembers
    /// its team to rejoin (`+0x434`); outside a team it keeps the one it has.
    pub(crate) fn remember_team_to_rejoin(&mut self, entity_id: u64) {
        if let Some(&team_id) = self.member_team.get(&entity_id) {
            self.rejoin_team.insert(entity_id, team_id);
        }
    }

    /// `entity_id`'s team to rejoin (`TechnoClass+0x434`), if any.
    pub(crate) fn team_to_rejoin(&self, entity_id: u64) -> Option<u64> {
        self.rejoin_team.get(&entity_id).copied()
    }

    /// `TechnoClass::PointerExpired @ 0x00707BB2..0x00707BBA` for every
    /// object as team `team_id` goes: none keeps it to rejoin.
    fn forget_team_to_rejoin(&mut self, team_id: u64) {
        self.rejoin_team.retain(|_, team| *team != team_id);
    }

    /// The object's destructor: its team to rejoin goes with it.
    pub(crate) fn object_deleted(&mut self, entity_id: u64) {
        self.rejoin_team.remove(&entity_id);
    }

    /// The TeamType of the team `entity_id` belongs to (Foot `+0x5D4` then
    /// TeamClass `+0x24`), if any.
    pub(crate) fn member_team_type(&self, entity_id: u64) -> Option<&TeamTypeDefinition> {
        let team_id = *self.member_team.get(&entity_id)?;
        self.team_types
            .get(&self.teams.get(&team_id)?.team_type_id?)
    }

    /// The team's TaskForce entries, none for a team without a TeamType.
    fn task_force_entries(&self, team: &TeamScriptState) -> &[TeamTaskForceEntry] {
        team.task_force_id
            .and_then(|id| self.task_forces.get(&id))
            .map_or(&[], |task_force| task_force.entries.as_slice())
    }

    /// The TeamType's `Priority=`, 0 for a team without one.
    fn priority(&self, team: &TeamScriptState) -> i32 {
        team.team_type_id
            .and_then(|id| self.team_types.get(&id))
            .map_or(0, |definition| definition.priority)
    }

    /// `TeamTypeClass::Get_Group @ 0x006F1870`: the TeamType's `Group=`
    /// unless -1, else its TaskForce's; -1 without either.
    fn team_group(&self, team: &TeamScriptState) -> i32 {
        let group = team
            .team_type_id
            .and_then(|id| self.team_type_ini.get(&id))
            .map_or(-1, |metadata| metadata.group);
        if group != -1 {
            return group;
        }
        team.task_force_id
            .and_then(|id| self.task_forces.get(&id))
            .map_or(-1, |task_force| task_force.group)
    }

    /// Prepend `member` to `team_id`'s list (`0x006EA562..0x006EA56E`).
    fn link_member(&mut self, team_id: u64, member: TeamMember) {
        let Some(team) = self.teams.get_mut(&team_id) else {
            return;
        };
        team.members.insert(0, member);
        self.member_team.insert(member.id, team_id);
    }

    /// Unlink `entity_id` from `team_id`'s list, returning its record.
    fn unlink_member(&mut self, team_id: u64, entity_id: u64) -> Option<TeamMember> {
        let team = self.teams.get_mut(&team_id)?;
        let position = team
            .members
            .iter()
            .position(|member| member.id == entity_id)?;
        let member = team.members.remove(position);
        if self.member_team.get(&entity_id) == Some(&team_id) {
            self.member_team.remove(&entity_id);
        }
        Some(member)
    }

    /// `TeamClass::PointerExpired @ 0x006EAE60` for every team, as the
    /// pointer-expired broadcast (`0x007258D0`) reaches each through its
    /// listener vector: a team forgets `expired` as its centre, closest
    /// member, mission target and move target, and with `all` drops it from
    /// the head of its list (a member leaves through `Remove_Member` first,
    /// so only a list the removal missed has it there). The team's own
    /// update is the only reader of what it clears, so the teams' place in
    /// the listener order does not matter.
    pub(crate) fn pointer_expired(&mut self, expired: u64, all: bool) {
        let object = Some(TeamTarget::Object(expired));
        for team in self.teams.values_mut() {
            if all
                && team
                    .members
                    .first()
                    .is_some_and(|member| member.id == expired)
            {
                team.members.remove(0);
                if self.member_team.get(&expired) == Some(&team.id) {
                    self.member_team.remove(&expired);
                }
            }
            if team.focus == object {
                team.focus = None;
            }
            if team.mission_target == object {
                team.mission_target = None;
            }
            if team.zone == object {
                team.zone = None;
            }
            if team.closest_member == Some(expired) {
                team.closest_member = None;
            }
        }
    }
}

impl TeamScriptState {
    /// `TeamClass::TeamClass @ 0x006E8A90`'s state (see
    /// [`TeamScriptVm::construct_team`]).
    fn new(
        id: u64,
        owner: InternedId,
        team_type_id: Option<InternedId>,
        task_force_id: Option<InternedId>,
        script_id: InternedId,
        current_frame: i32,
    ) -> Self {
        Self {
            id,
            owner,
            team_type_id,
            task_force_id,
            script_id,
            cursor: -1,
            advance_pending: true,
            has_been_full: false,
            altered: true,
            just_altered: false,
            suspended: false,
            suspend_timer: CdTimer::started(current_frame, 0),
            members: Vec::new(),
            slot_counts: [0; 6],
            succeeded: false,
            refusal: None,
            created_frame: current_frame,
            formed: false,
            full_strength: false,
            under_strength: true,
            reforming: false,
            leaving_map: false,
            zone: None,
            closest_member: None,
            mission_target: None,
            focus: None,
            guard_timer: CdTimer::started(current_frame, 0),
            retarget: false,
        }
    }

    /// Whether any state the recruitment chain added is off its constructor
    /// value (see [`TeamScriptVm::hash_state`]).
    fn recruitment_state_moved(&self, current_frame: i32) -> bool {
        !self.members.is_empty()
            || self.slot_counts != [0; 6]
            || self.full_strength
            || !self.under_strength
            || self.reforming
            || self.leaving_map
            || self.zone.is_some()
            || self.closest_member.is_some()
            || self.mission_target.is_some()
            || self.focus.is_some()
            || self.guard_timer.remaining(current_frame) != 0
    }
}

impl TeamScriptVm {
    /// Folds each team's creation frame and forming byte, the AI triggers
    /// whose track record left its registered state (tagged), and each team's
    /// recruitment state once it leaves its constructor values plus the
    /// objects' teams to rejoin (tagged).
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
            // The retired test-only wait counter, always 0.
            0u32.hash(hasher);
            // gamemd-derived: TeamClass CRC callback at vtable +0x34,
            // raw body 0x006EC5A0..0x006EC720, feeds one normalized remaining
            // time for +0x64/+0x6C before the +0x78/+0x7D/+0x7E/+0x83
            // bytes. It skips +0x68 and never feeds raw start.
            team.suspend_timer.remaining(current_frame).hash(hasher);
            team.has_been_full.hash(hasher);
            team.altered.hash(hasher);
            team.just_altered.hash(hasher);
            team.suspended.hash(hasher);
            team.members().collect::<Vec<u64>>().hash(hasher);
            // The retired target and per-type counts, always empty.
            None::<u64>.hash(hasher);
            Vec::<(TeamMemberTypeIdentity, u32)>::new().hash(hasher);
            team.created_frame.hash(hasher);
            team.formed.hash(hasher);
            if team.recruitment_state_moved(current_frame) {
                b"team-recruitment-v1".hash(hasher);
                team.members.hash(hasher);
                team.slot_counts.hash(hasher);
                team.full_strength.hash(hasher);
                team.under_strength.hash(hasher);
                team.reforming.hash(hasher);
                team.leaving_map.hash(hasher);
                team.zone.hash(hasher);
                team.closest_member.hash(hasher);
                team.mission_target.hash(hasher);
                team.focus.hash(hasher);
                team.guard_timer.remaining(current_frame).hash(hasher);
            }
        }
        if self.teams.values().any(|team| team.retarget) {
            b"team-retarget-v1".hash(hasher);
            for (id, team) in &self.teams {
                if team.retarget {
                    id.hash(hasher);
                }
            }
        }
        if !self.rejoin_team.is_empty() {
            b"team-rejoin-v1".hash(hasher);
            self.rejoin_team.hash(hasher);
        }
        let changed: Vec<_> = self
            .ai_trigger_order
            .iter()
            .filter_map(|id| {
                let record = self.ai_trigger_records.get(id)?;
                let initial = self.ai_triggers.get(id)?.weights[0];
                (*record != AiTriggerTrackRecord::new(initial)).then_some((id, record))
            })
            .collect();
        if !changed.is_empty() {
            b"ai-trigger-records-v1".hash(hasher);
            changed.hash(hasher);
        }
    }

    /// A formed team without a TeamType, running `script_id` from its first
    /// action (cursor 0, already stepped), whose members are `members` in
    /// list order, all joined up.
    #[cfg(test)]
    pub fn create_team(
        &mut self,
        owner: InternedId,
        script_id: InternedId,
        members: Vec<u64>,
        current_frame: i32,
    ) -> u64 {
        let id = self.next_team_id;
        self.next_team_id = self.next_team_id.wrapping_add(1);
        let mut team = TeamScriptState::new(id, owner, None, None, script_id, current_frame);
        team.cursor = 0;
        team.advance_pending = false;
        team.altered = false;
        team.formed = true;
        team.has_been_full = true;
        team.full_strength = true;
        team.under_strength = false;
        for &member in &members {
            self.member_team.insert(member, id);
        }
        team.members = members
            .into_iter()
            .map(|id| TeamMember {
                id,
                initiated: true,
            })
            .collect();
        self.teams.insert(id, team);
        id
    }

    /// A formed team of TeamType `team_type_id` whose members are
    /// `candidates` admitted in TaskForce-entry order, preserving input order
    /// within each type, listed in admission order and all initiated: the
    /// state after it formed and ran into its first action (cursor 0).
    #[cfg(test)]
    pub fn create_team_from_type(
        &mut self,
        owner: InternedId,
        team_type_id: InternedId,
        candidates: &[TeamScriptMember],
        current_frame: i32,
    ) -> u64 {
        let team_type = *self
            .team_types
            .get(&team_type_id)
            .expect("the seam's TeamType is registered");
        let task_force = self
            .task_forces
            .get(&team_type.task_force_id)
            .expect("the seam's TaskForce is registered");
        let mut used = vec![false; candidates.len()];
        let mut members = Vec::new();
        let mut slot_counts = [0; 6];
        for (slot, entry) in task_force.entries.iter().enumerate() {
            for _ in 0..entry.count.max(0) {
                let Some((index, candidate)) =
                    candidates.iter().enumerate().find(|(index, candidate)| {
                        !used[*index] && candidate.member_type == entry.member_type
                    })
                else {
                    break;
                };
                used[index] = true;
                members.push(TeamMember {
                    id: candidate.entity_id,
                    initiated: true,
                });
                slot_counts[slot] += 1;
            }
        }
        let full = members.len() as i32
            == task_force
                .entries
                .iter()
                .fold(0i32, |total, entry| total.wrapping_add(entry.count));
        let id = self.next_team_id;
        self.next_team_id = self.next_team_id.wrapping_add(1);
        let mut team = TeamScriptState::new(
            id,
            owner,
            Some(team_type_id),
            Some(team_type.task_force_id),
            team_type.script_id,
            current_frame,
        );
        team.cursor = 0;
        team.advance_pending = false;
        team.altered = false;
        team.formed = true;
        team.full_strength = full;
        team.has_been_full = full;
        team.under_strength = false;
        team.slot_counts = slot_counts;
        for member in &members {
            self.member_team.insert(member.id, id);
        }
        team.members = members;
        self.teams.insert(id, team);
        id
    }
}

/// An object's TechnoType identity (`vt+0x88`), as TaskForce entries name
/// types.
pub(crate) fn member_type_identity(
    entity: &crate::sim::game_entity::GameEntity,
) -> TeamMemberTypeIdentity {
    use crate::map::entities::EntityCategory;
    let category = match entity.category {
        EntityCategory::Infantry => ObjectCategory::Infantry,
        EntityCategory::Unit => ObjectCategory::Vehicle,
        EntityCategory::Aircraft => ObjectCategory::Aircraft,
        EntityCategory::Structure => ObjectCategory::Building,
    };
    TeamMemberTypeIdentity {
        category,
        id: entity.type_ref(),
    }
}

fn script_action_at(
    script: Option<&TeamScriptDefinition>,
    cursor: i32,
) -> Option<TeamScriptAction> {
    let index = usize::try_from(cursor).ok()?;
    script?.actions.get(index).copied()
}

/// Puts `members`, all of the first one's type, in a team of that many whose
/// TeamType has `Suicide=suicide` and `Aggressive=aggressive`, and returns it.
#[cfg(test)]
pub(crate) fn join_team_for_test(
    sim: &mut crate::sim::world::Simulation,
    members: &[u64],
    suicide: bool,
    aggressive: bool,
) -> u64 {
    let first = sim.substrate.entities.get(members[0]).unwrap();
    let owner = first.owner();
    let member_type = member_type_identity(first);
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
            count: i32::try_from(members.len()).unwrap(),
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
        combined_movement_zone: MovementZone::Normal,
        base_zone_relation_enforced: true,
        transport_crossing_required: false,
    });
    let candidates: Vec<TeamScriptMember> = members
        .iter()
        .map(|&entity_id| TeamScriptMember {
            entity_id,
            member_type,
        })
        .collect();
    teams.create_team_from_type(owner, team_type_id, &candidates, 0)
}

#[cfg(test)]
mod oracle_tests;
#[cfg(test)]
mod recruit_oracle_tests;

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

    /// A TeamType `team_type` of `entries` whose script is `[2,0]`.
    fn register_team_type(
        vm: &mut TeamScriptVm,
        team_type: InternedId,
        entries: Vec<TeamTaskForceEntry>,
    ) {
        let script = InternedId::from_index(2);
        let task_force = InternedId::from_index(3);
        vm.register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::FixedAimd,
            actions: vec![action(2, 0)],
        });
        vm.register_task_force(TeamTaskForceDefinition {
            id: task_force,
            source: TeamAiDefinitionSource::FixedAimd,
            group: -1,
            entries,
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
    }

    #[test]
    fn task_force_admission_uses_entry_then_candidate_order() {
        let owner = InternedId::from_index(1);
        let team_type = InternedId::from_index(4);
        let tank_identity = TeamMemberTypeIdentity {
            category: ObjectCategory::Vehicle,
            id: InternedId::from_index(5),
        };
        let infantry_identity = TeamMemberTypeIdentity {
            category: ObjectCategory::Infantry,
            id: InternedId::from_index(6),
        };
        let mut vm = TeamScriptVm::default();
        register_team_type(
            &mut vm,
            team_type,
            vec![
                TeamTaskForceEntry {
                    member_type: infantry_identity,
                    count: 2,
                },
                TeamTaskForceEntry {
                    member_type: tank_identity,
                    count: 1,
                },
            ],
        );
        let candidates = [
            (10, tank_identity),
            (20, infantry_identity),
            (30, infantry_identity),
        ]
        .map(|(entity_id, member_type)| TeamScriptMember {
            entity_id,
            member_type,
        });
        let team = vm.create_team_from_type(owner, team_type, &candidates, 0);

        let state = vm.team(team).expect("team");
        assert_eq!(state.members().collect::<Vec<_>>(), [20, 30, 10]);
        assert_eq!(state.slot_counts[..2], [2, 1]);
        assert!(state.full_strength);
    }

    #[test]
    fn gsi_04_05_task_force_admission_distinguishes_duplicate_native_families() {
        let owner = InternedId::from_index(1);
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
        register_team_type(
            &mut vm,
            team_type,
            vec![TeamTaskForceEntry {
                member_type: infantry_identity,
                count: 1,
            }],
        );
        let candidates =
            [(10, vehicle_identity), (20, infantry_identity)].map(|(entity_id, member_type)| {
                TeamScriptMember {
                    entity_id,
                    member_type,
                }
            });
        let team = vm.create_team_from_type(owner, team_type, &candidates, 0);

        assert_eq!(
            vm.team(team).unwrap().members().collect::<Vec<_>>(),
            [20],
            "same-name Unit candidate must not satisfy an Infantry pointer requirement"
        );
    }

    #[test]
    fn gsi_04_05_team_constructor_uses_native_response_latch_defaults() {
        let owner = InternedId::from_index(1);
        let team_type = InternedId::from_index(4);
        let mut vm = TeamScriptVm::default();
        register_team_type(&mut vm, team_type, Vec::new());

        let team = vm.construct_team(team_type, owner, true, -19).unwrap();
        let state = vm.team(team).unwrap();
        assert!(!state.has_been_full);
        assert!(state.under_strength && state.advance_pending && !state.formed);
        assert_eq!(state.cursor, -1);
        assert_eq!(
            state.response_suspension_state(),
            (true, false, false, -19, 0)
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
        let team = first.create_team(owner, script, vec![], 0);
        let mut second = first.clone();
        for (vm, timer) in [
            (&mut first, CdTimer::started(90, 20)),
            (&mut second, CdTimer::started(95, 15)),
        ] {
            let state = vm.teams.get_mut(&team).unwrap();
            state.altered = true;
            state.just_altered = true;
            state.suspended = true;
            state.suspend_timer = timer;
        }

        assert_eq!(state_hash_at(&first, 100), state_hash_at(&second, 100));
        second.teams.get_mut(&team).unwrap().suspend_timer = CdTimer::started(95, 16);
        assert_ne!(state_hash_at(&first, 100), state_hash_at(&second, 100));
    }

    #[test]
    fn recruitment_state_folds_once_it_leaves_the_constructor_values() {
        let owner = InternedId::from_index(1);
        let team_type = InternedId::from_index(4);
        let mut vm = TeamScriptVm::default();
        register_team_type(&mut vm, team_type, Vec::new());
        let team = vm.construct_team(team_type, owner, true, 0).unwrap();
        let constructed = state_hash_at(&vm, 0);

        vm.teams.get_mut(&team).unwrap().zone = Some(TeamTarget::Cell { x: 3, y: 4 });
        assert_ne!(state_hash_at(&vm, 0), constructed);
    }

    #[test]
    fn member_index_follows_the_member_lists_through_a_save() {
        let owner = InternedId::from_index(1);
        let script = InternedId::from_index(2);
        let mut vm = TeamScriptVm::default();
        let first = vm.create_team(owner, script, vec![7, 9], 0);
        let second = vm.create_team(owner, script, vec![11], 0);
        assert_eq!(vm.team_for_member(9), Some((first, false)));

        vm.pointer_expired(7, true);
        assert_eq!(vm.team_for_member(7), None);
        assert_eq!(vm.team(first).unwrap().members().collect::<Vec<_>>(), [9]);
        vm.pointer_expired(9, false);
        assert_eq!(vm.team_for_member(9), Some((first, false)));

        let restored: TeamScriptVm =
            serde_json::from_str(&serde_json::to_string(&vm).unwrap()).unwrap();
        assert_eq!(restored.team_for_member(9), Some((first, false)));
        assert_eq!(restored.team_for_member(11), Some((second, false)));
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
        let map = IniFile::from_str("[TaskForces]\n0=TF1\n[TF1]\n0=-2,E1\nGroup=9\n");
        let registry = TeamAiIniRegistry::from_sources(&fixed, &map, true);
        let mut interner = StringInterner::new();
        let (vm, diagnostics) = TeamScriptVm::from_ini_registry(&registry, &mut interner, &rules);

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
        assert!(
            vm.teams.is_empty(),
            "definition ingress must not create Teams"
        );

        let tt1 = interner.get("TT1").unwrap();
        let metadata = vm.team_type_ini(tt1).expect("TeamType metadata");
        assert_eq!(metadata.max_teams, -1);
        assert!(metadata.autocreate);
        assert!(metadata.are_team_members_recruitable);
        assert_eq!(
            vm.task_forces[&interner.get("TF1").unwrap()].entries[0].count,
            -2
        );
        assert_eq!(
            vm.task_forces[&interner.get("TF1").unwrap()].group,
            9,
            "map TaskForce re-read must preserve its signed Group in the resolved registry"
        );
        assert_eq!(
            vm.scripts[&interner.get("S1").unwrap()].actions[0].action_id,
            -1
        );
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
        assert!(trigger.multiplayer);
        assert_eq!(trigger.side, 1);
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
        assert_eq!(
            restored
                .task_force(interner.get("TF1").unwrap())
                .unwrap()
                .group,
            9
        );
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
        let (vm, diagnostics) = TeamScriptVm::from_ini_registry(&registry, &mut interner, &rules);

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
        let (vm, diagnostics) = TeamScriptVm::from_ini_registry(&registry, &mut interner, &rules);
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
        let (vm, diagnostics) = TeamScriptVm::from_ini_registry(&registry, &mut interner, &rules);

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
        let duplicate_task_force = vm.task_force(interner.get("DUP_TF").unwrap()).unwrap();
        assert_eq!(duplicate_task_force.entries.len(), 1);
        let duplicate_identity = TeamMemberTypeIdentity {
            category: ObjectCategory::Infantry,
            id: interner.get("DUP").unwrap(),
        };
        assert_eq!(
            duplicate_task_force.entries[0].member_type, duplicate_identity,
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
        let (vm, diagnostics) = TeamScriptVm::from_ini_registry(&registry, &mut interner, &rules);

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
        let fixed =
            IniFile::from_str("[TaskForces]\n0=PARTIAL_TF\n[PARTIAL_TF]\n0=1,E1\n1=2,GHOST\n");
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
        let fixed =
            IniFile::from_str("[TeamTypes]\n0=ONLY\n[ONLY]\nScript=NONE\nTaskForce=<NONE>\n");
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
        let (vm, diagnostics) = TeamScriptVm::from_ini_registry(&registry, &mut interner, &rules);

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
        assert!(anti_nuke.multiplayer);
        assert_eq!(anti_nuke.side, 1);
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
            crate::util::sha256::sha256_hex(threshold_rows.as_bytes()),
            "76096bc2d9592ff4c1054c23a38660e74c3860afbc2882db5c6dcc2074da8aad",
            "every game-mode-nonzero retail AITrigger threshold must match the native oracle"
        );

        let zero_mode_registry =
            TeamAiIniRegistry::from_sources(&aimd, &IniFile::from_str(""), false);
        let mut zero_mode_interner = StringInterner::new();
        let (zero_mode_vm, zero_mode_diagnostics) =
            TeamScriptVm::from_ini_registry(&zero_mode_registry, &mut zero_mode_interner, &rules);
        assert!(
            zero_mode_diagnostics.is_empty(),
            "{zero_mode_diagnostics:?}"
        );
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
            crate::util::sha256::sha256_hex(zero_mode_rows.as_bytes()),
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
