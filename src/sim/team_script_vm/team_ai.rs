//! `TeamClass::AI @ 0x006E9140`: each team's update, and the state it keeps
//! about its members: `Recalc @ 0x006EA3E0`, `Calc_Center @ 0x006EAEE0`,
//! `Regroup @ 0x006EB870`, the under-strength retreat (`0x006EA0D0`) and a
//! member's damage report (`0x006EB380`).
//!
//! Native evidence: the instruction-level spec
//! `vera20k-dev/research/chain5/chain5_team_ai_native.md` and the saved
//! disassembly (`asm/teamai.asm`, `recalc.asm`, `calccenter.asm`,
//! `regroup.asm`, `retreat.asm`, `tookdmg.asm`, `footdmg.asm`). Native
//! execution: `tools/team_recruit_oracle.py`'s `recalc`, `center` and
//! `distance` rows replay in `recruit_oracle_tests`.
//!
//! RESIDUALS:
//! - `Calc_Center`'s branch for a team whose current action is 10 (follow a
//!   friendly unit, `0x006EAF1C..0x006EB1B0`) is not ported; action 10 is
//!   not, so such a team is stopped on it, and its centre is the mean as for
//!   any other action.
//! - `Recalc` destroying an empty team that was leaving the map (`+0x82`)
//!   first springs trigger event 0x17 ("team leaves map") on every tag
//!   (`0x006EA49F..0x006EA4D2`, `0x006E53A0`), whose evaluation draws on the
//!   Scenario RNG for a trigger with event 51 (`0x007264C0` ->
//!   `0x00726400`). That producer is not connected to the shared live Tag
//!   owner. Trigger: a team sent to an off-map cell (action 54
//!   without a passable cell) loses its last member. Effect: map triggers on
//!   event 0x17 never fire from it.
//! - `+0x75` (NeedsRegrouping, written by Regroup and the damage report)
//!   and `+0x76` (written by Recalc under `GuardSlower=`) are not kept: no
//!   TeamClass code reads either. The private forced-active `+0x77` is
//!   written by shared reinforcement creation (`0x0065DD64`) and read by
//!   formation and recruitment in this owner.
//! - The under-strength retreat moves the team to its centre: the search
//!   for the house's nearest safe unarmed building (`0x006EA173..
//!   0x006EA39B`), which weights distance by the House threat map
//!   (`0x0056BCD0`, whose producers `AI_BuildThreatMap @ 0x0050940A` and
//!   `Adjust_Threat @ 0x004FA31B` are not ported) and asks `0x004CBC40` for a
//!   cell beside it, is not ported. Natively that search starts from the
//!   same centre cell and keeps it when no building yields a cell. Trigger: a
//!   formed team falls to a third of its TaskForce (`Reinforce=yes`, more
//!   than two) or loses its first member. Effect: the team regroups where it
//!   stands instead of falling back to a base building.

use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::DriveCoord;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::ground_pose::object_get_coords;
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;

use super::{TeamRules, TeamScriptState, TeamTarget, script_action_at};

/// Script actions 53 and 54 relax the stray distance (`Rules+0x1720`
/// instead of `+0x171C`) for Regroup and Coordinate_Move.
const RELAXED_STRAY_ACTIONS: [i32; 2] = [0x35, 0x36];

/// The team code's live-member test (`0x006EF9E0`, inlined throughout):
/// active (`+0x90`), with Health (`+0x6C`), out of limbo (`+0x81`;
/// the shared ScenarioInit/placement scope admits limbo members).
pub(super) fn member_is_live(entity: &GameEntity, placement_scope_active: bool) -> bool {
    entity.lifecycle.object_alive
        && entity.health.current != 0
        && (placement_scope_active || !entity.lifecycle.in_limbo)
}

/// A live member that has joined up (`+0x689`) or is an aircraft, which
/// team code treats as joined wherever it is.
pub(super) fn member_is_live_joined(
    entity: &GameEntity,
    initiated: bool,
    placement_scope_active: bool,
) -> bool {
    member_is_live(entity, placement_scope_active)
        && (initiated || entity.category == EntityCategory::Aircraft)
}

/// A member as `Calc_Center` weighs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CenterSample {
    pub(super) id: u64,
    /// Its location's XY (`+0x9C`, `+0xA0`).
    pub(super) xy: [i32; 2],
    /// Counted twice: a `GuardSlower=` team's member its weapon test admits.
    pub(super) doubled: bool,
    /// A naval transport, which is never the closest member.
    pub(super) naval_transport: bool,
}

/// `Calc_Center`'s member test (`0x006EB1CD..0x006EB21F`): a live joined
/// member (or aircraft) in the playfield (`+0x3D5`) counts.
pub(super) fn center_sample(
    entity: &GameEntity,
    initiated: bool,
    doubled: bool,
    naval_transport: bool,
    placement_scope_active: bool,
) -> Option<CenterSample> {
    (member_is_live_joined(entity, initiated, placement_scope_active) && entity.in_playfield).then(
        || CenterSample {
            id: entity.stable_id(),
            xy: crate::sim::movement::ground_pose::position_world_xy(&entity.position),
            doubled,
            naval_transport,
        },
    )
}

/// `Calc_Center`'s arithmetic (`0x006EB225..0x006EB31B`): the mean XY of
/// the samples (wrapping sums, signed truncating division), and the one
/// nearest `focus` by squared XY distance (`0x005F6500`, wrapping; 0
/// without a focus). A smaller distance, or any distance after a kept 0,
/// replaces the one kept, so without a focus the last sample is closest.
/// `None` without samples.
pub(super) fn center_of(
    samples: &[CenterSample],
    focus: Option<[i32; 2]>,
) -> Option<([i32; 2], Option<u64>)> {
    let mut sum = [0i32; 2];
    let mut count = 0i32;
    let mut closest = None;
    let mut closest_distance = 0i32;
    for sample in samples {
        for _ in 0..if sample.doubled { 2 } else { 1 } {
            sum[0] = sum[0].wrapping_add(sample.xy[0]);
            sum[1] = sum[1].wrapping_add(sample.xy[1]);
            count += 1;
        }
        if sample.naval_transport {
            continue;
        }
        let distance = focus.map_or(0, |focus| squared_xy_distance(sample.xy, focus));
        if closest_distance == 0 || distance < closest_distance {
            closest_distance = distance;
            closest = Some(sample.id);
        }
    }
    (count != 0).then(|| ([sum[0] / count, sum[1] / count], closest))
}

/// `0x005F6500`/`0x005F6560`: the squared XY distance, wrapping.
pub(super) fn squared_xy_distance(from: [i32; 2], to: [i32; 2]) -> i32 {
    let dx = from[0].wrapping_sub(to[0]);
    let dy = from[1].wrapping_sub(to[1]);
    dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy))
}

impl TeamScriptState {
    /// The current action (`ScriptClass::Current @ 0x00691500`): -1 before
    /// the first.
    pub(crate) fn current_action(&self, vm: &super::TeamScriptVm) -> i32 {
        script_action_at(vm.scripts.get(&self.script_id), self.cursor)
            .map_or(-1, |action| action.action_id)
    }
}

impl Simulation {
    /// Every team's update, in team order (`LogicClass::AI @ 0x0055B502..
    /// 0x0055B59F`, over a copy of the team list: a team the house AI adds
    /// later in the frame waits for the next one).
    pub(crate) fn run_team_ai_pass(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let teams: Vec<u64> = self.team_script_vm.teams.keys().copied().collect();
        for team_id in teams {
            if self.team_script_vm.teams.contains_key(&team_id) {
                self.team_ai(team_id, rules, registry, frame_effects);
            }
        }
    }

    /// `TeamClass::AI @ 0x006E9140` up to its action dispatch.
    fn team_ai(
        &mut self,
        team_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let current_frame = self.session.binary_frame as i32;
        let Some(team) = self.team_script_vm.teams.get_mut(&team_id) else {
            return;
        };
        // `0x006E9149..0x006E9174`: a suspended team waits out its timer.
        if team.suspended {
            if team.suspend_timer.remaining(current_frame) != 0 {
                return;
            }
            team.suspended = false;
        }
        // `0x006E917B`: a changed membership is recounted; an emptied team
        // that has been full is destroyed there.
        if team.altered && !self.team_recalc(team_id, rules, frame_effects) {
            return;
        }
        let Some(team) = self.team_script_vm.teams.get(&team_id) else {
            return;
        };
        if team.formed && team.under_strength {
            self.team_under_strength_retreat(team_id, rules, registry, frame_effects);
        }
        let vm = &mut self.team_script_vm;
        let Some(team) = vm.teams.get_mut(&team_id) else {
            return;
        };
        // `0x006E91AD..0x006E91F5`: a full team forms: its members are
        // initiated when it was reforming, and its script starts over.
        if !team.formed && (team.full_strength || team.forced_active) {
            team.formed = true;
            team.has_been_full = true;
            team.under_strength = false;
            if team.reforming || team.forced_active {
                for member in &mut team.members {
                    member.initiated = true;
                }
            }
            team.cursor = -1;
            team.advance_pending = true;
        }
        if team.reforming || team.formed || team.zone.is_none() || team.closest_member.is_none() {
            self.team_calc_center(team_id, rules, registry);
        }
        self.team_recruit_short_entries(team_id, rules, registry, frame_effects);

        let Some(team) = self.team_script_vm.teams.get(&team_id) else {
            return;
        };
        // `0x006E929B..0x006E9339`: an empty team that has been full, or
        // that could not fill before `DissolveUnfilledTeamDelay=` in a
        // multiplayer game, is destroyed.
        let team_rules = TeamRules::new(&rules.general, self.session.game_mode_nonzero);
        if team.dissolves(&team_rules, current_frame) {
            self.destroy_team(team_id, rules, frame_effects);
            return;
        }
        // The head member's warp-in (`vt+0x1D8`, Techno `+0x271`).
        let warping = team
            .members
            .first()
            .and_then(|head| self.substrate.entities.get(head.id))
            .is_some_and(GameEntity::is_warping_in);
        if !team.formed {
            self.team_coordinate_move(team_id, rules, registry, frame_effects);
            return;
        }
        if team.reforming || team.under_strength || warping {
            let regrouped = self.team_regroup(team_id, rules, registry, frame_effects);
            if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                team.reforming = !regrouped;
            }
            return;
        }
        self.team_step_script(team_id, rules, registry, frame_effects);
    }

    /// `0x006E9227..0x006E9299`: a team still forming, or a `Reinforce=`
    /// team below full strength, recruits for each short TaskForce entry,
    /// unless its house is human and it has been full.
    fn team_recruit_short_entries(
        &mut self,
        team_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let vm = &self.team_script_vm;
        let Some(team) = vm.teams.get(&team_id) else {
            return;
        };
        let reinforce = team
            .team_type_id
            .is_some_and(|team_type| vm.reinforce(team_type));
        if !(!team.formed || (!team.full_strength && reinforce))
            || (self.owner_is_human(team.owner) && team.has_been_full)
        {
            return;
        }
        let mut slot = 0;
        loop {
            let vm = &self.team_script_vm;
            let Some(team) = vm.teams.get(&team_id) else {
                return;
            };
            let entries = vm.task_force_entries(team);
            let Some(entry) = entries.get(slot) else {
                return;
            };
            if super::membership::entry_short(entry.count, team.slot_counts[slot]) {
                self.team_recruit(team_id, slot, rules, registry, frame_effects);
            }
            slot += 1;
        }
    }

    /// `TeamClass::Recalc @ 0x006EA3E0`: the team is full when it has
    /// exactly its TaskForce's total (`0x006E8160`, a wrapping i32 sum), and
    /// then has been full; it is under strength, for a `Reinforce=` TeamType,
    /// at a third of that total (signed truncation) or below when the total
    /// is above 2 and below the total otherwise, else until it has been
    /// full. A change of that clears nothing but sets reforming. With no
    /// members the team loses its centre and is under strength; one that has
    /// been full is destroyed (false), one that has not keeps its altered
    /// bytes, so the recount repeats every update.
    pub(super) fn team_recalc(
        &mut self,
        team_id: u64,
        rules: &RuleSet,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let vm = &mut self.team_script_vm;
        let Some(team) = vm.teams.get(&team_id) else {
            return false;
        };
        let desired = vm
            .task_force_entries(team)
            .iter()
            .fold(0i32, |total, entry| total.wrapping_add(entry.count));
        let reinforce = team
            .team_type_id
            .is_some_and(|team_type| vm.reinforce(team_type));
        let team = vm.teams.get_mut(&team_id).expect("resolved above");
        let was_under_strength = team.under_strength;
        let total = team.members.len() as i32;
        if total <= 0 {
            team.under_strength = true;
            team.full_strength = false;
            team.zone = None;
            if team.has_been_full {
                self.destroy_team(team_id, rules, frame_effects);
                return false;
            }
        } else {
            team.full_strength = total == desired;
            if team.full_strength {
                team.has_been_full = true;
            }
            team.under_strength = if reinforce {
                if desired > 2 {
                    total <= desired / 3
                } else {
                    total < desired
                }
            } else {
                !team.has_been_full
            };
            team.just_altered = false;
            team.altered = false;
        }
        if was_under_strength != team.under_strength {
            team.reforming = true;
        }
        true
    }

    /// `Calc_Center @ 0x006EAEE0` into the team's centre (`+0x34`) and
    /// closest member (`+0x38`): the mean XY (signed truncating division) of
    /// its live joined members in the playfield (`+0x3D5`), a
    /// `GuardSlower=` team counting twice each member its weapon test
    /// admits (`vt+0x4E0`); the cell there, unless the closest member
    /// (nearest the team's move target by squared XY distance, the last in
    /// list order while there is none; a naval transport never counts; the
    /// head when none does) cannot enter it (`vt+0x1AC`), when the centre is
    /// that member. Without such members both are cleared.
    pub(crate) fn team_calc_center(
        &mut self,
        team_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let vm = &self.team_script_vm;
        let Some(team) = vm.teams.get(&team_id) else {
            return;
        };
        let guard_slower = team
            .team_type_id
            .and_then(|id| vm.team_type_ini.get(&id))
            .is_some_and(|metadata| metadata.guard_slower);
        let focus = team.focus.map(|focus| self.team_target_xy(focus));
        let samples: Vec<CenterSample> = team
            .members
            .iter()
            .filter_map(|member| {
                let entity = self.substrate.entities.get(member.id)?;
                let naval_transport = self
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| object.passengers > 0 && object.naval);
                center_sample(
                    entity,
                    member.initiated,
                    guard_slower && self.guard_slower_weight(entity, rules),
                    naval_transport,
                    self.object_placement_scope_active(),
                )
            })
            .collect();
        let head = team.members.first().map(|member| member.id);
        let (zone, closest) = match center_of(&samples, focus) {
            None => (None, None),
            Some((mean, closest)) => {
                let cell = TeamTarget::Cell {
                    x: (mean[0] / 256) as i16,
                    y: (mean[1] / 256) as i16,
                };
                let closest = closest.or(head);
                let zone = match closest {
                    Some(member)
                        if self.team_member_refuses_cell(member, cell, rules, registry) =>
                    {
                        TeamTarget::Object(member)
                    }
                    _ => cell,
                };
                (Some(zone), closest)
            }
        };
        if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
            team.zone = zone;
            team.closest_member = closest;
        }
    }

    /// `FootClass vt+0x4E0` (`0x004DBFD0`), which a `GuardSlower=` team's
    /// centre weights twice: unarmed (`GetWeapon(0)`), or of `TechLevel=-1`,
    /// or `DeploysInto=` a building, or of `Speed=` 13 or less.
    fn guard_slower_weight(&self, entity: &GameEntity, rules: &RuleSet) -> bool {
        self.object_type(entity.type_ref(), rules)
            .is_some_and(|object| {
                crate::sim::combat::combat_weapon::weapon_for_index(
                    object,
                    entity.veterancy.rank(),
                    0,
                )
                .is_none()
                    || object.tech_level == -1
                    || object.deploys_into.is_some()
                    || object.speed <= 13
            })
    }

    /// `vt+0x1AC Can_Enter_Cell(cell, -1, -1, NULL, 1)` is not "move ok"
    /// (non-zero) for `member`, by its class's own answer
    /// ([`Simulation::mover_can_enter`]). An answer VERA cannot read admits.
    fn team_member_refuses_cell(
        &self,
        member: u64,
        cell: TeamTarget,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> bool {
        let TeamTarget::Cell { x, y } = cell else {
            return false;
        };
        self.mover_can_enter(
            member,
            (x, y),
            crate::sim::movement::infantry_entry::InfantryEntryArgs::REPAIR,
            crate::sim::movement::infantry_entry::EntryQueryMode::CheckLocomotor,
            rules,
            registry,
        )
        .is_ok_and(|code| code != 0)
    }

    /// `Regroup @ 0x006EB870`: each live member not yet joined joins when
    /// within stray of the centre (`Distance @ 0x005F6360`), or, without a
    /// destination, is sent to the centre; each live joined member (or
    /// aircraft) outside stray, and not area-guarding a target, without a
    /// destination is sent to the centre's cell and the team is not
    /// regrouped; one within stray (or area-guarding a target) guards in
    /// place unless on Area Guard. True when no joined member was sent.
    pub(super) fn team_regroup(
        &mut self,
        team_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        use crate::sim::mission::MissionType;
        let Some(team) = self.team_script_vm.teams.get(&team_id) else {
            return true;
        };
        let members = team.members.clone();
        let mut regrouped = true;
        for member in members {
            let Some(zone) = self
                .team_script_vm
                .teams
                .get(&team_id)
                .map(|team| team.zone)
            else {
                break;
            };
            let Some(entity) = self.substrate.entities.get(member.id) else {
                continue;
            };
            if !entity.lifecycle.object_alive {
                continue;
            }
            self.team_member_join_up(team_id, member.id, rules, registry, frame_effects);
            let Some(entity) = self.substrate.entities.get(member.id) else {
                continue;
            };
            let initiated = self.team_member_initiated(team_id, member.id);
            if !member_is_live_joined(entity, initiated, self.object_placement_scope_active()) {
                continue;
            }
            let stray = self.team_stray(team_id, rules);
            let mission = entity.mission.effective().known();
            let area_guarding_target =
                mission == Some(MissionType::AreaGuard) && entity.attack_target.is_some();
            if self.team_member_distance(entity, zone) > stray && !area_guarding_target {
                if entity.navigation.nav_com.is_some() {
                    continue;
                }
                self.team_member_queue_mission(member.id, MissionType::Move, rules);
                regrouped = false;
                let cell = zone.and_then(|zone| self.team_target_cell_of_coord(zone));
                self.team_member_set_destination(member.id, cell, rules, registry, frame_effects);
            } else {
                if mission == Some(MissionType::AreaGuard) {
                    continue;
                }
                self.team_member_queue_mission(member.id, MissionType::Guard, rules);
                self.team_member_set_destination(member.id, None, rules, registry, frame_effects);
            }
        }
        regrouped
    }

    /// `0x006EA0D0`, when a formed team is under strength: it stops (not
    /// formed, before its first action); an empty one loses its centre;
    /// otherwise, with a centre (or its closest member as the centre), it
    /// moves to the centre's cell (module residual).
    fn team_under_strength_retreat(
        &mut self,
        team_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let Some(team) = self.team_script_vm.teams.get_mut(&team_id) else {
            return;
        };
        team.formed = false;
        team.cursor = -1;
        if team.members.is_empty() {
            team.zone = None;
            return;
        }
        self.team_calc_center(team_id, rules, registry);
        let Some(team) = self.team_script_vm.teams.get_mut(&team_id) else {
            return;
        };
        if team.zone.is_none() {
            let Some(closest) = team.closest_member else {
                return;
            };
            team.zone = Some(TeamTarget::Object(closest));
        }
        let zone = team.zone.expect("set above");
        let focus = self.team_target_cell_of_coord(zone);
        if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
            team.focus = focus;
        }
        self.team_coordinate_move(team_id, rules, registry, frame_effects);
    }

    /// `TeamClass::Took_Damage @ 0x006EB380`, from `FootClass::ReceiveDamage
    /// @ 0x004D7453` when member `member` took damage from `source` with a
    /// result other than none (0) and gone (5): unless the TeamType is
    /// `Suicide=`, a forming team loses its centre and reforms; a formed one
    /// does too for an `Annoyance=` TeamType, unless `source` is a member,
    /// the head is an aircraft or unarmed (`GetWeapon(0)`), or `source` is
    /// the team's move target. Nothing after those writes has an effect
    /// (`0x006EB432..0x006EB477` ends in a discarded `WhatAmI`).
    pub(crate) fn team_took_damage(&mut self, member: u64, source: Option<u64>, rules: &RuleSet) {
        let Some(source) = source else {
            return;
        };
        let Some(team_id) = self.team_script_vm.member_team.get(&member).copied() else {
            return;
        };
        let vm = &self.team_script_vm;
        let Some(team) = vm.teams.get(&team_id) else {
            return;
        };
        let definition = team.team_type_id.and_then(|id| vm.team_types.get(&id));
        if definition.is_some_and(|definition| definition.suicide) {
            return;
        }
        if team.formed {
            if team.members.iter().any(|member| member.id == source) {
                return;
            }
            let Some(head) = team
                .members
                .first()
                .and_then(|head| self.substrate.entities.get(head.id))
            else {
                return;
            };
            let armed = self
                .object_type(head.type_ref(), rules)
                .and_then(|object| {
                    crate::sim::combat::combat_weapon::weapon_for_index(
                        object,
                        head.veterancy.rank(),
                        0,
                    )
                })
                .is_some();
            if head.category == EntityCategory::Aircraft
                || !armed
                || team.focus == Some(TeamTarget::Object(source))
            {
                return;
            }
            let annoyance = team
                .team_type_id
                .and_then(|id| vm.team_type_ini.get(&id))
                .is_some_and(|metadata| metadata.annoyance);
            if !annoyance {
                return;
            }
        }
        if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
            team.zone = None;
            team.reforming = true;
        }
    }

    /// The join-up step `Regroup`, `Coordinate_Move`, `Coordinate_Attack`
    /// and action 11 inline for each member: a live member not yet joined
    /// (`+0x689`) joins when within stray of the centre (`Distance @
    /// 0x005F6360`); outside it, without a destination, it is sent to the
    /// centre (Move, target cleared). True when it stayed outside stray.
    pub(super) fn team_member_join_up(
        &mut self,
        team_id: u64,
        member: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        use crate::sim::mission::MissionType;
        let Some(entity) = self.substrate.entities.get(member) else {
            return false;
        };
        if !member_is_live(entity, self.object_placement_scope_active())
            || self.team_member_initiated(team_id, member)
        {
            return false;
        }
        let zone = self
            .team_script_vm
            .teams
            .get(&team_id)
            .and_then(|team| team.zone);
        if self.team_member_distance(entity, zone) <= self.team_stray(team_id, rules) {
            self.team_set_member_initiated(team_id, member);
            return false;
        }
        if entity.navigation.nav_com.is_none() {
            self.team_member_queue_mission(member, MissionType::Move, rules);
            self.team_member_clear_target(member, rules);
            self.team_member_set_destination(member, zone, rules, registry, frame_effects);
        }
        true
    }

    /// Whether `member` of team `team_id` has joined up (`+0x689`).
    pub(super) fn team_member_initiated(&self, team_id: u64, member: u64) -> bool {
        self.team_script_vm
            .teams
            .get(&team_id)
            .and_then(|team| team.members.iter().find(|record| record.id == member))
            .is_some_and(|record| record.initiated)
    }

    pub(super) fn team_set_member_initiated(&mut self, team_id: u64, member: u64) {
        if let Some(record) = self
            .team_script_vm
            .teams
            .get_mut(&team_id)
            .and_then(|team| team.members.iter_mut().find(|record| record.id == member))
        {
            record.initiated = true;
        }
    }

    /// `TeamClass::Get_Stray @ 0x006F03B0` (which `Regroup`, `Coordinate_Move`
    /// and action 11 inline): `RelaxedStray=` while the current action is 53
    /// or 54, else `Stray=`.
    pub(crate) fn team_stray(&self, team_id: u64, rules: &RuleSet) -> i32 {
        let vm = &self.team_script_vm;
        let action = vm
            .teams
            .get(&team_id)
            .map_or(-1, |team| team.current_action(vm));
        if RELAXED_STRAY_ACTIONS.contains(&action) {
            rules.general.relaxed_stray
        } else {
            rules.general.stray
        }
    }

    /// A team target's coordinate (`vt+0x48`): a cell's centre at its floor
    /// height (`0x00486840`), else the object's GetCoords
    /// ([`object_get_coords`]): a building's foundation centre
    /// (`0x00447AC0`), any other object's Location.
    pub(super) fn team_target_coord(&self, target: TeamTarget) -> Option<DriveCoord> {
        match target {
            TeamTarget::Cell { x, y } => Some(crate::sim::movement::target_cell_coord(
                x as u16,
                y as u16,
                self.resolved_terrain
                    .as_ref()
                    .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                    .as_ref(),
            )),
            TeamTarget::Object(id) => {
                let entity = self.substrate.entities.get(id)?;
                Some(object_get_coords(entity, self.resolved_terrain.as_ref()))
            }
        }
    }

    /// A team target's XY (0, 0 for a missing object).
    pub(super) fn team_target_xy(&self, target: TeamTarget) -> [i32; 2] {
        self.team_target_coord(target)
            .map_or([0, 0], |coord| [coord.x, coord.y])
    }

    /// The cell holding a team target's coordinate, as Regroup and the
    /// retreat form it (`/256` truncating toward zero).
    pub(super) fn team_target_cell_of_coord(&self, target: TeamTarget) -> Option<TeamTarget> {
        let coord = self.team_target_coord(target)?;
        Some(TeamTarget::Cell {
            x: (coord.x / 256) as i16,
            y: (coord.y / 256) as i16,
        })
    }

    /// `ObjectClass::Distance @ 0x005F6360` from `member` to `target`
    /// ([`Simulation::object_distance_to`]), 0 without one.
    pub(super) fn team_member_distance(
        &self,
        member: &GameEntity,
        target: Option<TeamTarget>,
    ) -> i32 {
        target
            .and_then(|target| self.object_distance_to(member.stable_id(), target.target_kind()))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::TeamTarget;

    /// The centre's closest member refuses it by its own `Can_Enter_Cell`
    /// (`vt+0x1AC`), an Aircraft's `0x004196B0` included: in game mode 0 the
    /// current house's aircraft refuses a cell whose ground its house has not
    /// mapped.
    #[test]
    fn an_aircraft_member_refuses_a_shrouded_centre() {
        let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture();
        let hornet = sim
            .spawn_object("HORNET", "Americans", 17, 15, 0, &rules)
            .expect("aircraft");
        sim.substrate
            .entities
            .get_mut(hornet)
            .expect("aircraft")
            .discovery
            .owned_by_current_house = true;
        sim.fog = crate::sim::vision::FogState::new(33, 33);
        let centre = TeamTarget::Cell { x: 12, y: 12 };
        assert!(sim.team_member_refuses_cell(hornet, centre, &rules, Some(&registry)));

        let owner = sim.interner.intern("Americans");
        crate::sim::vision::reveal_radius(&mut sim.fog, owner, 12, 12, 2);
        assert!(!sim.team_member_refuses_cell(hornet, centre, &rules, Some(&registry)));
    }

    /// `ObjectClass::Distance @ 0x005F6360` measures from the member's
    /// GetCoords (`vt+0x48` at `0x005F6391`), whose Z is the object's world
    /// Z: a hovering member's height counts.
    #[test]
    fn a_hovering_members_distance_counts_its_height() {
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::sim::movement::locomotor::LocomotorState;
        let mut sim = crate::sim::world::Simulation::new();
        let mut member =
            crate::sim::game_entity::GameEntity::test_default(1, "HOVER", "Americans", 4, 4);
        member.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Hover));
        member.position.exact_z_leptons = Some(120);
        sim.substrate.entities.insert(member);
        let member = sim.substrate.entities.get(1).expect("member");
        // The cell target is (4, 4)'s centre at its floor, straight below, so
        // the whole distance is the height.
        let distance = sim.team_member_distance(member, Some(TeamTarget::Cell { x: 4, y: 4 }));
        assert_eq!(distance, 120);
    }
}
