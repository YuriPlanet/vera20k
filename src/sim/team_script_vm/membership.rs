//! Who joins a team and who leaves it: `TeamClass::Can_Add @ 0x006EA610`,
//! `Add_Member @ 0x006EA500`, `Remove_Member @ 0x006EA870` and `Recruit @
//! 0x006EAA90`, with the two callers that empty a team: its destructor
//! (`0x006E8DE0`) and the base-defense suspension (`0x006EC250`).
//!
//! The VM owns the member lists; these functions keep each member's own
//! state in step with them.
//!
//! Native evidence: the instruction-level specs in
//! `vera20k-dev/research/chain5/chain5_recruit_native.md`, and the
//! disassembly saved beside it (`asm/canadd.asm`, `addmember.asm`,
//! `removemember.asm`, `recruit.asm`, `team_dtor.asm`). Native execution:
//! `tools/team_recruit_oracle.py`'s `recruit` rows (the group filter and
//! penalty, the wrapping key, ties, the Unit search's own test, the entry
//! gate) replay in `recruit_oracle_tests`.
//!
//! RESIDUALS (none has a retail producer or reader that changes a skirmish):
//! - An object's team group (`+0x214`), which `Add_Member` writes
//!   (`0x006F1870`) and `Recruit` filters and penalises by, is not
//!   represented: every object reads -1, the constructor value, as every
//!   retail TeamType and TaskForce leaves `Group=` at -1.
//! - `Remove_Member` sets `+0x6B8` on every object it is called for; only a
//!   `Weeder=` unit's Mission_Guard reads it (`0x00740A60`), and VERA's
//!   Weeder takes its own guard arm.
//! - Of `Can_Add`'s building-entry flags (`0x004E0080`) only the Bio Reactor
//!   and Battle Bunker seeks' `+0x68F` and `+0x690` are written
//!   (`sim::building_entry`).
//! - `Recruit` adds a recruited unit's passengers (`0x006EADE8..0x006EAE17`);
//!   a passenger is in limbo, which `Can_Add` refuses while `ScenarioInit`
//!   is 0, so the loop adds nothing in play and is not ported.
//! - The team's Risk (`+0x4C`, the members' `ThreatPosed=` sum) is not kept:
//!   no ported reader.
//! - `Remove_Member`'s callers in unported owners do not remove: a
//!   building's occupant release with Hunt (`0x0045812B`), and script
//!   actions 60 and 64 and the team change and merge (`0x006E9E24`,
//!   `0x006E9F38`, `0x006E96FD`, `0x006ECFF8`).
//! - The removal loops of the five per-seek member loops from `0x006ECB50`
//!   (`0x006ECB7C..0x006ECCBC`) and of `0x0070F890` (`0x0070F8C7`) are not
//!   ported: none of these functions has a call or a pointer anywhere in the
//!   executable (`tools.native_inspect calls` and `find-bytes`).

use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::ground_pose::position_world_xy;
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;

use super::{TeamMember, TeamRules, member_type_identity, script_action_at};

/// `Recruit`'s penalty on a candidate outside the team's group, added to
/// the squared distance (`0x006EABFA`).
const OTHER_GROUP_PENALTY: i32 = 0x3200;

/// The group every object reads: `+0x214`'s constructor value (module
/// residual).
const OBJECT_GROUP: i32 = -1;

/// Whether a TaskForce entry of `amount` with `recruited` members counted
/// against it wants more (signed), as `TeamClass::AI`'s recruit loop
/// (`0x006E927A`) and `Recruit` (`0x006EAB43`) test it.
pub(super) const fn entry_short(amount: i32, recruited: i32) -> bool {
    amount > recruited
}

/// An object `Recruit` weighs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RecruitCandidate {
    pub(super) id: u64,
    /// Its location's XY (`vt+0x48`).
    pub(super) xy: [i32; 2],
    /// Its team group (`+0x214`).
    pub(super) group: i32,
    /// It passes the search's own tests: for a unit, the team's house's and
    /// of the entry's type.
    pub(super) matches: bool,
}

/// `Recruit`'s search (`0x006EAB86..0x006EAE19`) over one class array in
/// order: a candidate in the team's `group` (any with -2), or any for a
/// `Recruiter=` TeamType, is keyed by its squared XY distance to `origin`
/// (`0x005F6560`, wrapping) plus `0x3200` outside the group (wrapping); it
/// wins when it passes its own tests, its key is below the best (signed) or
/// the best is still -1, and `can_add` admits it.
pub(super) fn recruit_pick(
    candidates: &[RecruitCandidate],
    origin: [i32; 2],
    group: i32,
    recruiter: bool,
    mut can_add: impl FnMut(u64) -> bool,
) -> Option<u64> {
    let mut best = -1i32;
    let mut winner = None;
    for candidate in candidates {
        if !(group == -2 || candidate.group == group || recruiter) {
            continue;
        }
        let mut key = super::team_ai::squared_xy_distance(candidate.xy, origin);
        if candidate.group != group {
            key = key.wrapping_add(OTHER_GROUP_PENALTY);
        }
        if candidate.matches && (key < best || best == -1) && can_add(candidate.id) {
            winner = Some(candidate.id);
            best = key;
        }
    }
    winner
}

impl Simulation {
    /// `TeamClass::Can_Add @ 0x006EA610`: whether `entity_id` may join team
    /// `team_id`, and the TaskForce entry it counts against, the first of
    /// its type. A `constrained` add (mind control's `0x006EA4F0`) skips the
    /// type and count tests; with no entry of its type the answer is the
    /// entry count.
    ///
    /// In order, it refuses: a member of this team; an object not active
    /// (`+0x90`), at zero Health or in limbo (`+0x81`; `ScenarioInit` is 0
    /// in play); another house's; one in radio contact, unless an
    /// `AirportBound=` aircraft (`0x0065AE30`); one of no entry's type; one
    /// on a mission MissionControl does not let recruit (`0x005B36E0`); one
    /// whose map recruit byte (`+0x421`) is clear, for a TeamType that is
    /// not `Autocreate=`; one whose team recruit byte (`+0x422`) is clear,
    /// for an `Autocreate=` TeamType, unless an aircraft or the team's
    /// current action is 8 (Unload); one seeking a building's entry
    /// (`0x004E0080`, [`crate::sim::building_entry`]); one draining
    /// (`+0x1CC`) or in a tank bunker (`+0x2E4`); a member of a team whose
    /// TeamType's `Priority=` is not lower (signed); an armed aircraft out
    /// of ammo (`+0x2FC`); one whose entry is full (signed); and one a
    /// Magnetron holds (`+0x2B0`, `0x006EA841`).
    pub(crate) fn team_can_add(
        &self,
        team_id: u64,
        entity_id: u64,
        constrained: bool,
        rules: &RuleSet,
    ) -> Option<usize> {
        let vm = &self.team_script_vm;
        let team = vm.teams.get(&team_id)?;
        let current_team = vm.member_team.get(&entity_id).copied();
        if current_team == Some(team_id) {
            return None;
        }
        let entity = self.substrate.entities.get(entity_id)?;
        if !entity.lifecycle.object_alive()
            || entity.health.current() == 0
            || (entity.lifecycle.in_limbo() && !self.object_placement_scope_active())
            || entity.owner() != team.owner
        {
            return None;
        }
        let aircraft = entity.category() == EntityCategory::Aircraft;
        let object = self.object_type(entity.type_ref(), rules)?;
        if !entity.radio_contacts.is_empty() && !(aircraft && object.airport_bound) {
            return None;
        }
        let entries = vm.task_force_entries(team);
        let member_type = member_type_identity(entity);
        let slot = entries
            .iter()
            .position(|entry| entry.member_type == member_type)
            .unwrap_or(entries.len());
        if slot == entries.len() && !constrained {
            return None;
        }
        if !crate::sim::mission::control::mission_recruitable(rules, entity.mission.effective()) {
            return None;
        }
        let metadata = team.team_type_id.and_then(|id| vm.team_type_ini.get(&id));
        let autocreate = metadata.is_some_and(|metadata| metadata.autocreate);
        if !entity.base_defense_response.recruitable_a() && !autocreate {
            return None;
        }
        let unloading = script_action_at(vm.scripts.get(&team.script_id), team.cursor)
            .is_some_and(|action| action.action_id == 8);
        if !entity.base_defense_response.recruitable_b() && autocreate && !(unloading || aircraft) {
            return None;
        }
        // `0x006EA79B..0x006EA7A4`: one seeking a building's entry.
        if entity.building_entry.seeking() {
            return None;
        }
        if entity.drain.target().is_some() || entity.bunker.link().installed_in().is_some() {
            return None;
        }
        if let Some(current) = current_team.and_then(|id| vm.teams.get(&id))
            && vm.priority(current) >= vm.priority(team)
        {
            return None;
        }
        if aircraft
            && crate::sim::combat::combat_weapon::weapon_for_index(
                object,
                entity.veterancy.rank(),
                0,
            )
            .is_some()
            && entity.ammo().is_some_and(|ammo| ammo.current() == 0)
        {
            return None;
        }
        if !constrained && team.slot_counts[slot] >= entries[slot].count {
            return None;
        }
        if entity.lift.source().is_some() {
            return None;
        }
        Some(slot)
    }

    /// `TeamClass::Add_Member @ 0x006EA500`: after [`Self::team_can_add`],
    /// the object leaves its old team, counts against its entry (not when
    /// `constrained`), joins the head of the list, initiated only as the
    /// first member (`+0x689`), gives the team a centre if it has none
    /// (`Calc_Center`), marks the team altered (`+0x7D`/`+0x7E`) and takes
    /// the TeamType's `AreTeamMembersRecruitable=` as its team recruit byte
    /// (`+0x422`, which leaving does not restore).
    pub(crate) fn team_add_member(
        &mut self,
        team_id: u64,
        entity_id: u64,
        constrained: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let Some(slot) = self.team_can_add(team_id, entity_id, constrained, rules) else {
            return false;
        };
        if let Some(old_team) = self.team_script_vm.member_team.get(&entity_id).copied() {
            self.team_remove_member(old_team, entity_id, false, Some(rules), frame_effects);
        }
        let vm = &mut self.team_script_vm;
        let (recruitable, on_trans_only) = vm
            .teams
            .get(&team_id)
            .and_then(|team| team.team_type_id)
            .and_then(|id| vm.team_type_ini.get(&id))
            .map_or((true, false), |metadata| {
                (
                    metadata.are_team_members_recruitable,
                    metadata.on_trans_only,
                )
            });
        let Some(team) = vm.teams.get_mut(&team_id) else {
            return false;
        };
        if !constrained {
            team.slot_counts[slot] = team.slot_counts[slot].wrapping_add(1);
        }
        let initiated = team.members.is_empty();
        let centreless = team.zone.is_none();
        vm.link_member(
            team_id,
            TeamMember {
                id: entity_id,
                initiated,
            },
        );
        //6EA58C..6EA5B0: B3=0 inherits a nonNULL Team Tag on every member;
        //B3=1 requires signed Type+5E0 Passengers>0. The Tag owner preserves
        //the Object attachment when Team+70 is NULL (6EA582..6EA587).
        //Executed eight-way controls: tools/spatial_oracle/team_creation.
        if !on_trans_only
            || self
                .substrate
                .entities
                .get(entity_id)
                .and_then(|entity| self.object_type(entity.type_ref(), rules))
                .is_some_and(|object| object.passengers > 0)
        {
            self.attach_team_member_trigger_tag(team_id, entity_id);
        }
        if centreless {
            self.team_calc_center(team_id, rules, registry);
        }
        if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
            team.altered = true;
            team.just_altered = true;
        }
        if let Some(entity) = self.substrate.entities.get_mut(entity_id) {
            // `+0x422` (`0x006EA5F5`).
            entity
                .base_defense_response
                .set_team_recruitable(recruitable);
        }
        true
    }

    /// `TeamClass::Remove_Member @ 0x006EA870` of `entity_id` from team
    /// `team_id` (a no-op for an object not in it): it stops counting
    /// against the first entry of its type, leaves the list, drops its
    /// suspended mission, destination and target (`+0xB0`, `+0x5A8`,
    /// `+0x2B8`) and, active, not falling (`+0x425`), out of limbo and
    /// unless `no_idle`, enters its idle mode (`vt+0x484(0, 1)`). If no
    /// member left is initiated, the head is, and the team loses its centre.
    /// The team is marked altered. Without rules (lifecycle tests) the idle
    /// order is not given.
    pub(crate) fn team_remove_member(
        &mut self,
        team_id: u64,
        entity_id: u64,
        no_idle: bool,
        rules: Option<&RuleSet>,
        frame_effects: FrameEffects<'_>,
    ) {
        if self.team_script_vm.member_team.get(&entity_id) != Some(&team_id) {
            return;
        }
        //6EA892..6EA8B5 clears only this Team's Tag and only when the
        //member has a House that IsControlledByHuman50B730 refuses.
        if self
            .substrate
            .entities
            .get(entity_id)
            .and_then(|entity| self.houses.get(&entity.owner()))
            .is_some_and(|house| !house.is_controlled_by_human(self.session.game_mode_nonzero()))
        {
            self.clear_team_member_trigger_tag(team_id, entity_id);
        }
        let member_type = self
            .substrate
            .entities
            .get(entity_id)
            .map(member_type_identity);
        let vm = &mut self.team_script_vm;
        let Some(team) = vm.teams.get(&team_id) else {
            return;
        };
        let slot = vm
            .task_force_entries(team)
            .iter()
            .position(|entry| Some(entry.member_type) == member_type);
        let other_initiated = team
            .members
            .iter()
            .any(|member| member.id != entity_id && member.initiated);
        if let Some(team) = vm.teams.get_mut(&team_id)
            && let Some(slot) = slot
        {
            team.slot_counts[slot] = team.slot_counts[slot].wrapping_sub(1);
        }
        vm.unlink_member(team_id, entity_id);
        let idles = self
            .substrate
            .entities
            .get_mut(entity_id)
            .is_some_and(|entity| {
                entity.mission.clear_suspended();
                entity.navigation.clear_suspended_nav_com();
                entity.suspended_attack_target.clear();
                entity.lifecycle.object_alive()
                    && !entity.crash.is_crashing()
                    && !entity.lifecycle.in_limbo()
            });
        if idles
            && !no_idle
            && let Some(rules) = rules
        {
            crate::sim::world::enter_idle_mode(self, entity_id, rules, None, frame_effects);
        }
        if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
            if !other_initiated && let Some(head) = team.members.first_mut() {
                head.initiated = true;
                team.zone = None;
            }
            team.just_altered = true;
            team.altered = true;
        }
    }

    /// `TeamClass::Has_Entered_Map @ 0x006EC370`: no member of team `team_id`
    /// is outside the playfield (`+0x3D5`); an empty team answers true.
    pub(crate) fn team_has_entered_map(&self, team_id: u64) -> bool {
        self.team_script_vm.team(team_id).is_none_or(|team| {
            team.members().all(|member| {
                self.substrate
                    .entities
                    .get(member)
                    .is_some_and(|entity| entity.in_playfield())
            })
        })
    }

    /// Remove `entity_id` from whatever team it is in; the object side's
    /// `if (Team) Team->Remove_Member(this, -1, no_idle)`.
    pub(crate) fn leave_team(
        &mut self,
        entity_id: u64,
        no_idle: bool,
        rules: Option<&RuleSet>,
        frame_effects: FrameEffects<'_>,
    ) {
        if let Some(team_id) = self.team_script_vm.member_team.get(&entity_id).copied() {
            self.team_remove_member(team_id, entity_id, no_idle, rules, frame_effects);
        }
    }

    /// Remove every member of team `team_id`, head first, as the destructor
    /// and the base-defense suspension do (`while (Member)
    /// Remove_Member(Member, -1, 0)`).
    fn team_remove_all_members(
        &mut self,
        team_id: u64,
        rules: &RuleSet,
        frame_effects: FrameEffects<'_>,
    ) {
        while let Some(head) = self
            .team_script_vm
            .teams
            .get(&team_id)
            .and_then(|team| team.members.first())
            .map(|member| member.id)
        {
            self.team_remove_member(team_id, head, false, Some(rules), frame_effects);
            // The member index and the lists change together, so the head
            // left; a stale head would otherwise stop this loop.
            let vm = &mut self.team_script_vm;
            if vm
                .teams
                .get(&team_id)
                .and_then(|team| team.members.first())
                .is_some_and(|member| member.id == head)
            {
                debug_assert!(false, "team {team_id} kept member {head}");
                vm.unlink_member(team_id, head);
            }
        }
    }

    /// `TeamClass::Recruit @ 0x006EAA90` for TaskForce entry `slot`: while
    /// the entry is short (signed), the class array of the entry's type is
    /// searched in construction order for the object nearest the team's
    /// centre (`+0x34`, or the origin without one) that `Can_Add` admits,
    /// by the wrapping i32 squared XY distance (`0x005F6560`), plus
    /// `0x3200` outside the team's group; a candidate replaces the best only
    /// when strictly nearer, or the best is still -1. A unit must also be
    /// the team's house's and of the entry's own type; an infantryman or an
    /// aircraft may be of any entry's type. The winner loses its target
    /// (`vt+0x3C8`) and joins ([`Self::team_add_member`]).
    pub(crate) fn team_recruit(
        &mut self,
        team_id: u64,
        slot: usize,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let vm = &self.team_script_vm;
        let Some(team) = vm.teams.get(&team_id) else {
            return false;
        };
        let waypoint = team
            .team_type_id
            .and_then(|id| vm.team_type_ini.get(&id))
            .map_or(-1, |metadata| metadata.waypoint);
        let origin = if waypoint >= 0 {
            self.session
                .waypoint_cell(waypoint)
                .map_or([0, 0], |(x, y)| {
                    self.team_target_xy(super::TeamTarget::Cell { x, y })
                })
        } else {
            team.zone.map_or([0, 0], |zone| self.team_target_xy(zone))
        };
        let entries = vm.task_force_entries(team);
        let Some(&entry) = entries.get(slot) else {
            return false;
        };
        if !entry_short(entry.count, team.slot_counts[slot]) {
            return false;
        }
        let group = vm.team_group(team);
        let recruiter = team
            .team_type_id
            .and_then(|id| vm.team_type_ini.get(&id))
            .is_some_and(|metadata| metadata.recruiter);
        let class = EntityCategory::from(entry.member_type.category);
        if class == EntityCategory::Structure {
            return false;
        }
        let owner = team.owner;
        let candidates: Vec<RecruitCandidate> = self
            .substrate
            .entities
            .iter_sorted()
            .filter(|(_, entity)| entity.category() == class)
            .map(|(id, entity)| RecruitCandidate {
                id,
                xy: position_world_xy(&entity.position),
                group: OBJECT_GROUP,
                // The unit search's own house and type test.
                matches: class != EntityCategory::Unit
                    || (entity.owner() == owner
                        && member_type_identity(entity) == entry.member_type),
            })
            .collect();
        let winner = recruit_pick(&candidates, origin, group, recruiter, |id| {
            self.team_can_add(team_id, id, false, rules).is_some()
        });
        let Some(winner) = winner else {
            return false;
        };
        self.team_member_clear_target(winner, rules);
        self.team_add_member(team_id, winner, false, rules, registry, frame_effects);
        true
    }

    /// `TeamClass::~TeamClass @ 0x006E8DE0`, reached through the scalar
    /// deleting destructor (`vt+0x20`): the team's AI triggers record its
    /// outcome, then every member leaves, head first, and the team is gone.
    /// The house's and TeamType's live-team counts (`+0x566C`, `+0xDC`) are
    /// counted from the team list.
    pub(crate) fn destroy_team(
        &mut self,
        team_id: u64,
        rules: &RuleSet,
        frame_effects: FrameEffects<'_>,
    ) {
        let team_rules = TeamRules::new(&rules.general, self.session.game_mode_nonzero());
        self.team_script_vm
            .record_trigger_outcome(team_id, &team_rules);
        self.team_remove_all_members(team_id, rules, frame_effects);
        self.clear_team_trigger_tag(team_id);
        self.team_script_vm.teams.remove(&team_id);
        self.team_script_vm.forget_team_to_rejoin(team_id);
    }

    /// TeamTypeClass::DestroyAll6F0A70, called by TAction5 at6DEB9A.
    /// The original walks the live array forward and revisits the index
    /// after deletion. Member release and AI feedback stay with destroy_team.
    /// This caller adds no RNG draws, timer writes or object-detach calls.
    pub(crate) fn destroy_teams_of_type(
        &mut self,
        team_type: crate::sim::intern::InternedId,
        rules: &RuleSet,
        frame_effects: FrameEffects<'_>,
    ) {
        let mut index = 0;
        loop {
            let Some((id, type_id)) = self
                .team_script_vm
                .teams_in_order()
                .nth(index)
                .map(|team| (team.id, team.team_type_id()))
            else {
                break;
            };
            if type_id == Some(team_type) {
                self.destroy_team(id, rules, frame_effects);
            } else {
                index += 1;
            }
        }
    }

    /// `0x006EC250`, called by `TechnoClass::RespondToBaseAttack @
    /// 0x007081A9`: every team of `owner`, in team order, whose TeamType's
    /// `Priority=` is below `priority` (signed) loses all its members, is
    /// marked altered and is suspended (`+0x83`) for `duration` frames from
    /// now (`+0x64`/`+0x6C`).
    pub(crate) fn suspend_teams_for_base_defense(
        &mut self,
        owner: crate::sim::intern::InternedId,
        priority: i32,
        duration: i32,
        rules: &RuleSet,
        frame_effects: FrameEffects<'_>,
    ) {
        let current_frame = self.session.binary_frame() as i32;
        let vm = &self.team_script_vm;
        let suspended: Vec<u64> = vm
            .teams
            .values()
            .filter(|team| {
                team.owner == owner
                    && team
                        .team_type_id
                        .and_then(|id| vm.team_types.get(&id))
                        .is_some_and(|definition| definition.priority < priority)
            })
            .map(|team| team.id)
            .collect();
        for team_id in suspended {
            self.team_remove_all_members(team_id, rules, frame_effects);
            if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                team.just_altered = true;
                team.altered = true;
                team.suspend_timer = crate::sim::timer::CdTimer::started(current_frame, duration);
                team.suspended = true;
            }
        }
    }
}
