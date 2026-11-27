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
//! - `+0x75` (NeedsRegrouping, written by Regroup and the damage report)
//!   and `+0x76` (written by Recalc under `GuardSlower=`) are not kept: no
//!   TeamClass code reads either. The private forced-active `+0x77` is
//!   written by shared reinforcement creation (`0x0065DD64`) and read by
//!   formation and recruitment in this owner.

use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::ScanMission;
use crate::sim::components::DriveCoord;
use crate::sim::game_entity::GameEntity;
use crate::sim::trigger_runtime::TeamTagOrder;

use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;

use super::{TeamScriptState, TeamTarget, script_action_at};
use crate::sim::intern::StringInterner;

/// Script actions 53 and 54 relax the stray distance (`Rules+0x1720`
/// instead of `+0x171C`) for Regroup and Coordinate_Move.
const RELAXED_STRAY_ACTIONS: [i32; 2] = [0x35, 0x36];

/// With batched target scans, the frames between a team's own turns
/// ([`Simulation::team_update_period`]).
const TEAM_UPDATE_PERIOD: u32 = 4;

#[cfg(all(test, tests_sim_t_z))]
#[path = "guard_slower_tests.rs"]
mod guard_slower_tests;

#[cfg(all(test, tests_sim_t_z))]
#[path = "retreat_tests.rs"]
mod retreat_tests;

#[cfg(all(test, tests_sim_t_z))]
#[path = "cadence_tests.rs"]
mod cadence_tests;

/// The team code's live-member test (`0x006EF9E0`, inlined throughout):
/// active (`+0x90`), with Health (`+0x6C`), out of limbo (`+0x81`;
/// the shared ScenarioInit/placement scope admits limbo members).
pub(super) fn member_is_live(entity: &GameEntity, placement_scope_active: bool) -> bool {
    entity.lifecycle.object_alive()
        && entity.health.current() != 0
        && (placement_scope_active || !entity.lifecycle.in_limbo())
}

/// A live member that has joined up (`+0x689`) or is an aircraft, which
/// team code treats as joined wherever it is.
pub(super) fn member_is_live_joined(
    entity: &GameEntity,
    initiated: bool,
    placement_scope_active: bool,
) -> bool {
    member_is_live(entity, placement_scope_active)
        && (initiated || entity.category() == EntityCategory::Aircraft)
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
    (member_is_live_joined(entity, initiated, placement_scope_active) && entity.in_playfield())
        .then(|| CenterSample {
            id: entity.stable_id(),
            xy: crate::sim::movement::ground_pose::position_world_xy(&entity.position),
            doubled,
            naval_transport,
        })
}

/// `Calc_Center`'s arithmetic (`0x006EB225..0x006EB31B`): the mean XY of
/// the samples (wrapping sums, signed truncating division), and the one
/// nearest `focus` by squared XY distance (`0x005F6500`,
/// `native_coord::squared_xy_distance_for_map`; 0 without a focus). A smaller
/// distance, or any distance after a kept 0, replaces the one kept, so without
/// a focus the last sample is closest. `None` without samples. One pass in
/// member order, so the caller hands its member walk over as it goes. A
/// VERA-only map compares the true 64-bit square instead of the wrapped
/// native one.
pub(super) fn center_of(
    samples: impl IntoIterator<Item = CenterSample>,
    focus: Option<[i32; 2]>,
    ares: bool,
    vera_only_map: bool,
) -> Option<([i32; 2], Option<u64>)> {
    let mut sum = [0i32; 2];
    let mut count = 0i32;
    let mut closest = None;
    let mut closest_distance = 0i64;
    for sample in samples {
        for _ in 0..if sample.doubled { 2 } else { 1 } {
            sum[0] = sum[0].wrapping_add(sample.xy[0]);
            sum[1] = sum[1].wrapping_add(sample.xy[1]);
            count += 1;
        }
        if sample.naval_transport {
            continue;
        }
        let distance = focus.map_or(0, |focus| {
            crate::util::native_coord::squared_xy_distance_for_map(
                sample.xy,
                focus,
                ares,
                vera_only_map,
            )
        });
        if closest_distance == 0 || distance < closest_distance {
            closest_distance = distance;
            closest = Some(sample.id);
        }
    }
    (count != 0).then(|| ([sum[0] / count, sum[1] / count], closest))
}

/// Whether `entity`'s `GetWeapon(0)` (vt+0x3F8) holds a WeaponType.
fn first_weapon_armed(interner: &StringInterner, entity: &GameEntity, rules: &RuleSet) -> bool {
    interner
        .object_type(entity.type_ref(), rules)
        .and_then(|object| {
            crate::sim::combat::combat_weapon::weapon_for_index(object, entity.veterancy.rank(), 0)
                .and_then(|record| record.weapon_id)
        })
        .is_some()
}

/// Whether the join-up step ([`Simulation::team_member_join_up`]) has
/// anything to do for a member: it is live and has not joined up. A walk
/// asks first, and keeps what it has read of the member and the team when
/// the step would change nothing.
pub(super) fn member_awaits_join_up(
    entity: &GameEntity,
    initiated: bool,
    placement_scope_active: bool,
) -> bool {
    member_is_live(entity, placement_scope_active) && !initiated
}

impl TeamScriptState {
    /// The current action (`ScriptClass::Current @ 0x00691500`): -1 before
    /// the first.
    pub(crate) fn current_action(&self, vm: &super::TeamScriptVm) -> i32 {
        script_action_at(vm.scripts.get(&self.script_id), self.cursor)
            .map_or(-1, |action| action.action_id)
    }

    /// `TeamClass::Get_Stray @ 0x006F03B0` (which `Regroup`, `Coordinate_Move`
    /// and action 11 inline): `RelaxedStray=` while the current action is 53
    /// or 54, else `Stray=`.
    pub(super) fn stray(&self, vm: &super::TeamScriptVm, rules: &RuleSet) -> i32 {
        if RELAXED_STRAY_ACTIONS.contains(&self.current_action(vm)) {
            rules.general.relaxed_stray
        } else {
            rules.general.stray
        }
    }

    /// `0x006E9149..0x006E9174`: the team is suspended (`+0x83`) and its
    /// timer (`+0x64`) has time left at `frame`, so its update returns at
    /// once.
    pub(super) fn waits_out_suspension(&self, frame: i32) -> bool {
        self.suspended && self.suspend_timer.remaining(frame) != 0
    }

    /// Script action 5's end test (`0x006E97CE`): its guard timer (`+0x58`)
    /// has run out at `frame`.
    pub(super) fn guard_over(&self, frame: i32) -> bool {
        self.guard_timer.remaining(frame) == 0
    }
}

impl Simulation {
    /// Every team's update, in team order (`LogicClass::AI @ 0x0055B502..
    /// 0x0055B59F`, over a copy of the team list: a team the house AI adds
    /// later in the frame waits for the next one). With batched target scans
    /// the scans of the leaders whose patrol scan is due are answered side by
    /// side first (`techno_ai::scan_batch`).
    ///
    /// Sim-scale deviation from native order (update order and same-frame
    /// effects), on only with batched target scans, pending side-by-side
    /// play: a team updates on its own turn, one frame in
    /// [`Self::team_update_period`] ([`Self::team_own_turn`]), and on any
    /// other frame only when its update cannot wait
    /// ([`Self::team_update_can_wait`]). Natively every team updates every
    /// frame. A skipped update would have changed nothing that cannot wait:
    /// what it would have done, the team's next update does, up to three
    /// frames later. That update gives the members their next orders, sees
    /// them arrive, run out of ammunition or warp in, turns them on the
    /// attacker Ares' `TeamRetaliate=` made the team's target, draws what its
    /// action draws (action 48's house and building picks) and finds the
    /// team's centre (`Calc_Center`). The centre is read by the team's own
    /// update, which finds it first, by `Add_Member` only to find one when
    /// there is none, and by Ares' `TeamRetaliate=` range check. Its
    /// frame-gated checks, the patrol scan ([`Self::patrol_scan_due`]) and
    /// `Coordinate_Attack`'s leader check ([`Self::team_attack_check_due`]),
    /// run on its own turn after their frame, so each still runs once per its
    /// period, but for a patrol scan whose `PatrolScan=` period is shorter
    /// than the turns, which runs on every own turn.
    ///
    /// Residual: two member missions set the team's mission target outside
    /// its update, a `Carryall=` aircraft's Move (`0x00416EE4`) and an
    /// aircraft's Patrol (`0x0041738D`), which a team gives its members only
    /// through action 11. `Assign_Mission_Target` (`0x006E9050`) then sends
    /// the members bound for the old target to Guard, and the team's next
    /// update sends them on, here up to three frames later instead of on the
    /// next frame, so they can act on Guard in between. No aircraft type the
    /// retail rules list is `Carryall=` and no retail AI script gives Patrol;
    /// an ordinary aircraft's Move (`0x004166E0`) leaves its team alone.
    pub(crate) fn run_team_ai_pass(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) {
        let unfilled_team_timeout = self
            .session
            .rule_policy()
            .unfilled_team_timeout(rules.general.dissolve_unfilled_team_delay);
        let teams: Vec<u64> = self.team_script_vm.teams.keys().copied().collect();
        if self.target_scans_batched() {
            let leaders = self.due_patrol_leaders(rules);
            self.answer_batched_direct_scans(&leaders, ScanMission::Guard, rules, registry);
        }
        for team_id in teams {
            let Some(team) = self.team_script_vm.teams.get(&team_id) else {
                continue;
            };
            if self.team_own_turn(team_id) || !self.team_update_can_wait(team) {
                self.team_ai(
                    team_id,
                    rules,
                    registry,
                    frame_effects,
                    unfilled_team_timeout,
                );
            }
        }
        self.finish_batched_scans();
    }

    /// The frames between a team's own turns ([`Self::run_team_ai_pass`]):
    /// `None`, every frame as natively, unless target scans are batched.
    pub(super) fn team_update_period(&self) -> Option<u32> {
        self.target_scans_batched().then_some(TEAM_UPDATE_PERIOD)
    }

    /// Whether this frame is team `team_id`'s own turn: the frame plus its
    /// phase, its id modulo the period, is a multiple of the period; every
    /// frame is without one. Team ids rise with construction and are saved,
    /// so the phase keeps no state of its own.
    pub(super) fn team_own_turn(&self, team_id: u64) -> bool {
        self.team_update_period().is_none_or(|period| {
            (u64::from(self.session.binary_frame()) + team_id % u64::from(period))
                % u64::from(period)
                == 0
        })
    }

    /// Whether `team`'s update may wait for its own turn this frame
    /// ([`Self::run_team_ai_pass`]); false runs it now, as natively. What
    /// changes the team outside its update or at its end, and what a timer
    /// ends, runs on the next frame; an action that counts its work in
    /// updates, or that is not listed here, runs every frame. Natively
    /// (`TeamClass::AI @ 0x006E9140`):
    /// - a suspended team's update returns at once until its timer
    ///   (`+0x64`) runs out ([`TeamScriptState::waits_out_suspension`]): it
    ///   waits while time is left and runs on the frame it runs out;
    /// - a team that is forming (`+0x7F` clear) gathers its members
    ///   (`Coordinate_Move`) every update, and one that recruits
    ///   ([`Self::team_recruits`]), forming or a formed `Reinforce=` team
    ///   below full strength, recruits a member per short TaskForce entry
    ///   per update: both run every frame;
    /// - membership changes (`+0x7D`, `Recalc`, and the destruction or the
    ///   under-strength retreat it leads to; an empty team that has never been
    ///   full keeps `+0x7D`, so in a multiplayer game it dissolves on the
    ///   frame its age passes `DissolveUnfilledTeamDelay=`,
    ///   `0x006E929B..0x006E92D8`), regrouping (`+0x7B`,
    ///   `0x006E9339..0x006E935B`), a finished action (`+0x80`: the next one
    ///   starts on the next update) and `Coordinate_Attack`'s restart
    ///   (`+0x81`) run at once;
    /// - action 5 runs on the frame its guard timer (`+0x58`) runs out
    ///   ([`TeamScriptState::guard_over`]);
    /// - the actions that move or attack (0, 1, 3, 4, 15, 16, 22, 46, 47, 48,
    ///   53, 54, 58, 59), whose updates order the members by their state,
    ///   wait while the team has a target (`+0x40`); without one the update
    ///   takes up the mission target (`+0x3C`, `0x006E9402..0x006E940C`),
    ///   finds a target or finishes, at once. So when a target goes
    ///   (`PointerExpired`) while the mission target stays, as the object
    ///   `Coordinate_Attack` aims at on its cell or an Ares `TeamRetaliate=`
    ///   attacker can, the mission target is taken up again on the next
    ///   frame. Action 11 orders its members by their state and never
    ///   finishes, and action 2 does nothing; both wait.
    fn team_update_can_wait(&self, team: &TeamScriptState) -> bool {
        let frame = self.session.binary_frame() as i32;
        if team.suspended {
            return team.waits_out_suspension(frame);
        }
        if !team.formed
            || self.team_recruits(team)
            || team.altered
            || team.reforming
            || team.advance_pending
            || team.retarget
        {
            return false;
        }
        match team.current_action(&self.team_script_vm) {
            5 => !team.guard_over(frame),
            0 | 1 | 3 | 4 | 15 | 16 | 22 | 46 | 47 | 48 | 53 | 54 | 58 | 59 => team.focus.is_some(),
            2 | 11 => true,
            _ => false,
        }
    }

    /// The leaders whose patrol scan (Script16's, mask 1) this step reaches
    /// as the teams stand before its first update: of a formed team neither
    /// reforming nor under strength nor waiting out a suspension, which
    /// [`Self::team_ai`] lets reach its script, on action 16 with no step
    /// pending, whose scan is due ([`Self::patrol_scan_due`]): only on the
    /// team's own turn, which always runs. An earlier update can still
    /// change that; an answer no scan takes is dropped.
    pub(super) fn due_patrol_leaders(&self, rules: &RuleSet) -> Vec<u64> {
        let frame = self.session.binary_frame() as i32;
        let vm = &self.team_script_vm;
        vm.teams
            .iter()
            .filter(|&(&id, team)| {
                team.formed
                    && !team.reforming
                    && !team.under_strength
                    && !team.waits_out_suspension(frame)
                    && !team.advance_pending
                    && team.current_action(vm) == 16
                    && self.patrol_scan_due(id, rules) == Some(true)
            })
            .filter_map(|(&id, _)| self.team_leader(id, rules))
            .collect()
    }

    /// `TeamClass::AI @ 0x006E9140` up to its action dispatch.
    fn team_ai(
        &mut self,
        team_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
        unfilled_team_timeout: Option<i32>,
    ) {
        let current_frame = self.session.binary_frame() as i32;
        // VERA: the teams of a house whose `Forces` a Wasm module claimed
        // stand as they are until the claim goes (RFC 0001 6i,
        // `sim::ext_modules::claims`).
        if let Some(team) = self.team_script_vm.teams.get(&team_id)
            && self.module_claims(team.owner, vera20k_wasm_ext::DOMAIN_FORCES)
        {
            return;
        }
        let Some(team) = self.team_script_vm.teams.get_mut(&team_id) else {
            return;
        };
        // `0x006E9149..0x006E9174`: a suspended team waits out its timer.
        if team.waits_out_suspension(current_frame) {
            return;
        }
        team.suspended = false;
        // `0x006E917B`: a changed membership is recounted; an emptied team
        // that has been full is destroyed there.
        if team.altered && !self.team_recalc(team_id, rules, registry, frame_effects) {
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
            self.team_calc_center(team_id, rules);
        }
        self.team_recruit_short_entries(team_id, rules, frame_effects);

        let Some(team) = self.team_script_vm.teams.get(&team_id) else {
            return;
        };
        // `0x006E929B..0x006E9339`: an empty team that has been full, or
        // that could not fill before `DissolveUnfilledTeamDelay=` in a
        // multiplayer game, is destroyed.
        if team.dissolves(current_frame, unfilled_team_timeout) {
            // TeamClass::AI @0x006E9140, retail gamemd.exe:6E92D0..6E9320
            // raises23 while the empty Team still exists for HasOccurred23.
            if team.leaving_map {
                self.raise_team_left_map_tags(
                    TeamTagOrder::Forward,
                    rules,
                    registry,
                    frame_effects,
                );
            }
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
            let regrouped = self.team_regroup(team_id, rules, frame_effects);
            if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                team.reforming = !regrouped;
            }
            return;
        }
        self.team_step_script(team_id, rules, registry, frame_effects);
    }

    /// `0x006E9227..0x006E9299`: whether `team`'s update recruits
    /// ([`Self::team_recruit_short_entries`]): it is still forming, or a
    /// `Reinforce=` team below full strength, and its house is not human with
    /// the team having been full.
    pub(super) fn team_recruits(&self, team: &TeamScriptState) -> bool {
        let reinforce = team
            .team_type_id
            .is_some_and(|team_type| self.team_script_vm.reinforce(team_type));
        (!team.formed || (!team.full_strength && reinforce))
            && !(self.owner_is_human(team.owner) && team.has_been_full)
    }

    /// `0x006E9227..0x006E9299`: a team that recruits
    /// ([`Self::team_recruits`]) does so for each short TaskForce entry.
    pub(super) fn team_recruit_short_entries(
        &mut self,
        team_id: u64,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
    ) {
        if !self
            .team_script_vm
            .teams
            .get(&team_id)
            .is_some_and(|team| self.team_recruits(team))
        {
            return;
        }
        let mut walks = super::membership::RecruitWalks::default();
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
                self.team_recruit(team_id, slot, &mut walks, rules, frame_effects);
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
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
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
                if team.leaving_map {
                    //6EA49F..6EA4D2 raises23 while this empty Team still
                    //exists. Callbacks can destroy Teams or create new Tags.
                    self.raise_team_left_map_tags(
                        TeamTagOrder::Reverse,
                        rules,
                        registry,
                        frame_effects,
                    );
                }
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
    /// `GuardSlower=` team counting twice each member of a slow kind
    /// (`vt+0x4E0`, `guard_slower_weight`); the cell there, unless the
    /// closest member (nearest the team's move target by squared XY distance, the last in
    /// list order while there is none; a naval transport never counts; the
    /// head when none does) cannot enter it (`vt+0x1AC`), when the centre is
    /// that member. Without such members both are cleared.
    pub(crate) fn team_calc_center(&mut self, team_id: u64, rules: &RuleSet) {
        let vm = &self.team_script_vm;
        let Some(team) = vm.teams.get(&team_id) else {
            return;
        };
        // Calc_Center6EAF13 branches before the ordinary member mean. This
        // also applies to callers outside the action handler (formation,
        // regroup and the team's ordinary AI pass).
        if team.current_action(vm) == 10 {
            let (zone, closest) = self.team_follow_friendly_center(team_id);
            if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                team.zone = zone;
                team.closest_member = closest;
            }
            return;
        }
        let guard_slower = team
            .team_type_id
            .and_then(|id| vm.team_type_ini.get(&id))
            .is_some_and(|metadata| metadata.guard_slower);
        let focus = team.focus.map(|focus| self.team_target_xy(focus));
        let samples = team.members.iter().filter_map(|member| {
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
        });
        let head = team.members.first().map(|member| member.id);
        let (zone, closest) = match center_of(
            samples,
            focus,
            rules.ares(),
            self.session.map_exceeds_original_size(),
        ) {
            None => (None, None),
            Some((mean, closest)) => {
                // CalcCenter6EB331 selects Map565730's Cell before CanEnter.
                let (x, y) = self.resolved_terrain.as_ref().map_or(
                    ((mean[0] / 256) as i16, (mean[1] / 256) as i16),
                    |terrain| {
                        let cells =
                            crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
                        cells.coord(cells.lookup_world(mean[0], mean[1]))
                    },
                );
                let cell = TeamTarget::Cell { x, y };
                //6EB33C publishes the pointer before CanEnter's map queries.
                if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                    team.zone = Some(cell);
                }
                let closest = closest.or(head);
                let zone = match closest {
                    Some(member) if self.team_member_refuses_cell(member, cell, rules) => {
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
    /// centre weights twice: a type of `TechLevel=-1`, or `DeploysInto=` a
    /// building, or of converted speed 13 leptons/frame or less
    /// (`TechnoTypeClass::Read_INI @ 0x0071465F`). Its first test, a NULL
    /// `GetWeapon(0)` (`0x004DBFD7..0x004DBFE1`), never passes: GetWeapon
    /// `0x0070E140` returns NULL only for index -1 and otherwise the address
    /// of a weapon slot (`0x007177C0`/`0x007177E0`), so an unarmed member is
    /// weighted by those three tests alone.
    fn guard_slower_weight(&self, entity: &GameEntity, rules: &RuleSet) -> bool {
        self.object_type(entity.type_ref(), rules)
            .is_some_and(|object| {
                self.object_tech_level(object) == -1
                    || object.deploys_into.is_some()
                    || crate::util::fixed_math::ra2_speed_to_leptons_per_frame(object.speed) <= 13
            })
    }

    /// `vt+0x1AC Can_Enter_Cell(cell, -1, -1, NULL, 1)` is not "move ok"
    /// (non-zero) for `member`, by its class's own answer
    /// ([`Simulation::mover_can_enter`]). An answer VERA cannot read admits.
    fn team_member_refuses_cell(&self, member: u64, cell: TeamTarget, rules: &RuleSet) -> bool {
        let TeamTarget::Cell { x, y } = cell else {
            return false;
        };
        self.mover_can_enter(
            member,
            (x, y),
            crate::sim::movement::infantry_entry::InfantryEntryArgs::REPAIR,
            crate::sim::movement::infantry_entry::EntryQueryMode::CheckLocomotor,
            rules,
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
        frame_effects: &mut FrameEffects<'_>,
    ) -> bool {
        use crate::sim::mission::MissionType;
        if !self.team_script_vm.teams.contains_key(&team_id) {
            return true;
        }
        // The walk reads a copy: a member's join or move can change the list.
        let members = self.team_script_vm.copy_members(team_id);
        let mut regrouped = true;
        for (position, &member) in members.iter().enumerate() {
            let Some(zone) = self
                .team_script_vm
                .teams
                .get(&team_id)
                .map(|team| team.zone)
            else {
                break;
            };
            let Some(entity) = self.substrate.entities.get(member) else {
                continue;
            };
            if !entity.lifecycle.object_alive() {
                continue;
            }
            self.team_member_join_up(team_id, position, member, rules, frame_effects);
            let Some(entity) = self.substrate.entities.get(member) else {
                continue;
            };
            let initiated = self.team_member_initiated(team_id, position, member);
            if !member_is_live_joined(entity, initiated, self.object_placement_scope_active()) {
                continue;
            }
            let stray = self.team_stray(team_id, rules);
            let mission = entity.mission.effective().known();
            let area_guarding_target =
                mission == Some(MissionType::AreaGuard) && entity.attack_target.target().is_some();
            if self.team_member_distance(entity, zone) > stray && !area_guarding_target {
                if entity.navigation.nav_com().is_some() {
                    continue;
                }
                self.team_member_queue_mission(member, MissionType::Move, rules);
                regrouped = false;
                let cell = zone.and_then(|zone| self.team_target_cell_of_coord(zone));
                self.team_member_set_destination(member, cell, rules, frame_effects);
            } else {
                if mission == Some(MissionType::AreaGuard) {
                    continue;
                }
                self.team_member_queue_mission(member, MissionType::Guard, rules);
                self.team_member_set_destination(member, None, rules, frame_effects);
            }
        }
        self.team_script_vm.restore_member_ids(members);
        regrouped
    }

    /// `0x006EA0D0`, when a formed team is under strength: it stops (not
    /// formed, before its first action); an empty one loses its centre;
    /// otherwise it searches the House's unarmed buildings and falls back to
    /// its centre when no candidate yields a cell. Original controls:
    /// `tools.team_recruit_oracle --retreat`.
    fn team_under_strength_retreat(
        &mut self,
        team_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
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
        self.team_calc_center(team_id, rules);
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
        let owner = team.owner;
        let mut focus = self.team_target_cell_of_coord(zone);
        let mut best_score = i32::MAX;
        if let Some(house) = self.houses.get(&owner) {
            // 6EA173 iterates the retained House Building array, in order.
            for &building_id in house.base_projection().buildings() {
                let Some(building) = self.substrate.entities.get(building_id) else {
                    continue;
                };
                if building.lifecycle.in_limbo() {
                    continue;
                }
                let Some(object) = self.object_type(building.type_ref(), rules) else {
                    continue;
                };
                if crate::sim::combat::combat_weapon::get_weapon(
                    building,
                    object,
                    0,
                    &self.substrate.entities,
                    rules,
                    &self.interner,
                )
                .is_some()
                {
                    continue;
                }
                let Some(destination) = self.team_target_coord(TeamTarget::Object(building_id))
                else {
                    continue;
                };
                let Some(source) = self.team_target_coord(zone) else {
                    continue;
                };
                // 6EA1FF..6EA256 squares dx,dz,dy through the shared original
                // Sqrt_Approx/ftol kernel; this is centre distance, no foundation subtraction.
                let distance = crate::util::native_x87::sqrt_approx_length([
                    destination.x.wrapping_sub(source.x),
                    destination.z.wrapping_sub(source.z),
                    destination.y.wrapping_sub(source.y),
                ]);
                let target = ((destination.x / 256) as i16, (destination.y / 256) as i16);
                let Ok(threat) = self.house_threat_at_cell(owner, target) else {
                    continue;
                };
                let mut score = distance.wrapping_mul(threat.wrapping_add(1));
                // UnitRepair+16A9 gets signed division by two, 6EA274..288.
                if object.unit_repair {
                    score /= 2;
                }
                if score >= best_score {
                    continue;
                }
                // Inline GetLeader6EA2E5..341 has the same owner as Script16,
                // Script54 and House50D170: highest live joined LeadershipRating.
                let Some(leader) = self.team_leader(team_id, rules) else {
                    continue;
                };
                let from = ((source.x / 256) as i16, (source.y / 256) as i16);
                let Some((x, y)) = self.foot_nearby_location(leader, from, target, 2..4, rules)
                else {
                    continue;
                };
                best_score = score;
                focus = Some(TeamTarget::Cell { x, y });
            }
        }
        // 6EA39B resolves the selected packed cell through Map5657A0 before
        // retaining it. This also preserves fixed-stride aliases/Dummy stamps.
        let focus = match (focus, self.resolved_terrain.as_ref()) {
            (Some(TeamTarget::Cell { x, y }), Some(terrain)) => {
                let (x, y) = terrain.native_cell_coord(terrain.native_cell_identity((x, y)));
                Some(TeamTarget::Cell { x, y })
            }
            _ => focus,
        };
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
    /// the team's move target. Nothing after those writes has an effect in
    /// stock gamemd (`0x006EB432..0x006EB477` ends in a discarded `WhatAmI`);
    /// Ares replaces that tail ([`Self::ares_team_retaliate`]).
    pub(crate) fn team_took_damage(
        &mut self,
        member: u64,
        source: Option<u64>,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let ares = rules.ares_form();
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
        let formed = team.formed;
        if formed {
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
            if head.category() == EntityCategory::Aircraft
                || !first_weapon_armed(&self.interner, head, rules)
                || team.focus == Some(TeamTarget::Object(source))
            {
                return;
            }
            let annoyance = team
                .team_type_id
                .and_then(|id| vm.team_type_ini.get(&id))
                .is_some_and(|metadata| metadata.annoyance);
            if !annoyance {
                if ares {
                    self.ares_team_retaliate(team_id, source, rules, registry);
                }
                return;
            }
        }
        if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
            team.zone = None;
            team.reforming = true;
        }
        if ares && formed {
            self.ares_team_retaliate(team_id, source, rules, registry);
        }
    }

    /// Ares TeamClass_AttackedBy_Retaliate (1003A180, hook 0x6EB432), the
    /// formed team's tail under `[General] TeamRetaliate=yes`. An armed
    /// Techno focus (`+0x40`, `GetWeapon(0)`) is kept unless the team has a
    /// centre (`+0x34`) that the focus is close enough to attack:
    /// IsCloseEnoughToAttackCoords (vt+0x3B0, `0x006F7930`) is CanFireAt of
    /// the cell at the centre's GetCoords with the weapon SelectWeapon picks
    /// against that cell. Otherwise an attacker that is no Aircraft, of no
    /// House the team's owner is allied with (IsAlliedWith, inlined) and, for
    /// a Foot, neither in limbo nor `ConsideredAircraft=` becomes the focus.
    fn ares_team_retaliate(
        &mut self,
        team_id: u64,
        attacker: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        if !rules.ares_general().team_retaliate() {
            return;
        }
        let Some(team) = self.team_script_vm.teams.get(&team_id) else {
            return;
        };
        let (focus, zone, owner) = (team.focus, team.zone, team.owner);
        if let Some(TeamTarget::Object(focus)) = focus
            && self
                .substrate
                .entities
                .get(focus)
                .is_some_and(|entity| first_weapon_armed(&self.interner, entity, rules))
        {
            let Some(zone) = zone else {
                return;
            };
            let cells = self
                .resolved_terrain
                .as_ref()
                .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
            let Some(centre) = crate::sim::movement::ground_pose::target_get_coords(
                zone.target_kind(),
                self,
                cells.as_ref(),
            ) else {
                return;
            };
            let (x, y) = crate::sim::movement::coord_cell(centre);
            let cell = crate::sim::combat::TargetKind::Cell(x as u16, y as u16);
            if !self.techno_can_fire_at_target(focus, cell, rules, registry) {
                return;
            }
        }
        let Some(attacker_entity) = self.substrate.entities.get(attacker) else {
            return;
        };
        if attacker_entity.category() == EntityCategory::Aircraft
            || crate::sim::combat::combat_weapon::is_ally_by_object(
                Some(&self.house_alliances),
                &self.interner,
                owner,
                attacker_entity.owner(),
            )
            || (attacker_entity.category().is_foot()
                && (attacker_entity.lifecycle.in_limbo()
                    || self
                        .interner
                        .object_type(attacker_entity.type_ref(), rules)
                        .is_some_and(|object| object.considered_aircraft)))
        {
            return;
        }
        if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
            team.focus = Some(TeamTarget::Object(attacker));
        }
    }

    /// The join-up step `Regroup`, `Coordinate_Move`, `Coordinate_Attack`
    /// and action 11 inline for each member: a live member not yet joined
    /// (`+0x689`) joins when within stray of the centre (`Distance @
    /// 0x005F6360`); outside it, without a destination, it is sent to the
    /// centre (Move, target cleared). True when it stayed outside stray.
    /// `position` is where the caller's walk met `member`
    /// ([`Self::team_member_initiated`]).
    pub(super) fn team_member_join_up(
        &mut self,
        team_id: u64,
        position: usize,
        member: u64,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
    ) -> bool {
        use crate::sim::mission::MissionType;
        let Some(entity) = self.substrate.entities.get(member) else {
            return false;
        };
        let vm = &self.team_script_vm;
        let team = vm.teams.get(&team_id);
        let initiated = team.is_some_and(|team| team.member_initiated(position, member));
        if !member_awaits_join_up(entity, initiated, self.object_placement_scope_active()) {
            return false;
        }
        let zone = team.and_then(|team| team.zone);
        let stray = team.map_or(rules.general.stray, |team| team.stray(vm, rules));
        if self.team_member_distance(entity, zone) <= stray {
            self.team_script_vm
                .team_set_member_initiated(team_id, member);
            return false;
        }
        if entity.navigation.nav_com().is_none() {
            self.team_member_queue_mission(member, MissionType::Move, rules);
            self.team_member_clear_target(member, rules);
            self.team_member_set_destination(member, zone, rules, frame_effects);
        }
        true
    }

    /// Whether `member` of team `team_id` has joined up (`+0x689`).
    /// `position` is where the caller's walk met `member` in its copy of the
    /// list ([`TeamScriptState::member_initiated`]).
    pub(super) fn team_member_initiated(&self, team_id: u64, position: usize, member: u64) -> bool {
        self.team_script_vm
            .teams
            .get(&team_id)
            .is_some_and(|team| team.member_initiated(position, member))
    }

    /// Team `team_id`'s stray distance ([`TeamScriptState::stray`]); `Stray=`
    /// for a missing team.
    pub(crate) fn team_stray(&self, team_id: u64, rules: &RuleSet) -> i32 {
        let vm = &self.team_script_vm;
        vm.teams
            .get(&team_id)
            .map_or(rules.general.stray, |team| team.stray(vm, rules))
    }

    /// A team target's coordinate (`vt+0x48`): a cell's centre at its floor
    /// height (`0x00486840`), else the object's GetCoords
    /// ([`object_get_coords`]): a building's foundation centre
    /// (`0x00447AC0`), any other object's Location.
    pub(super) fn team_target_coord(&self, target: TeamTarget) -> Option<DriveCoord> {
        // Abstract+48 reads the retained pointer. The coordinate owner also
        // keeps a Dummy's live coordinate instead of performing a new lookup.
        crate::sim::movement::ground_pose::target_get_coords(
            target.target_kind(),
            self,
            self.resolved_terrain
                .as_ref()
                .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                .as_ref(),
        )
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

impl crate::sim::team_script_vm::TeamScriptVm {
    pub(super) fn team_set_member_initiated(&mut self, team_id: u64, member: u64) {
        if let Some(record) = self
            .teams
            .get_mut(&team_id)
            .and_then(|team| team.members.iter_mut().find(|record| record.id == member))
        {
            record.initiated = true;
        }
    }
}

#[cfg(all(test, tests_sim_t_z))]
mod tests {
    use super::TeamTarget;

    /// The centre's closest member refuses it by its own `Can_Enter_Cell`
    /// (`vt+0x1AC`), an Aircraft's `0x004196B0` included: in game mode 0 the
    /// current house's aircraft refuses a cell whose ground its house has not
    /// mapped.
    #[test]
    fn an_aircraft_member_refuses_a_shrouded_centre() {
        let (mut sim, rules, _registry) = crate::sim::world::entry_test_fixture::fixture();
        let hornet = sim
            .spawn_object("HORNET", "Americans", 17, 15, 0, &rules)
            .expect("aircraft");
        sim.substrate
            .entities
            .get_mut(hornet)
            .expect("aircraft")
            .seldom
            .discovery
            .set_owned_by_current_house_for_test(true);
        *sim.fog = crate::sim::vision::FogState::new(33, 33);
        let centre = TeamTarget::Cell { x: 12, y: 12 };
        assert!(sim.team_member_refuses_cell(hornet, centre, &rules));

        let owner = sim.interner.intern("Americans");
        crate::sim::vision::reveal_radius(&mut sim.fog, owner, 12, 12, 2);
        assert!(!sim.team_member_refuses_cell(hornet, centre, &rules));
    }

    /// Ares TeamClass_AttackedBy_Retaliate (1003A180, hook 0x6EB432): with
    /// `[General] TeamRetaliate=yes` a formed team without a focus takes an
    /// enemy attacker as its focus; with the stock tail it keeps none.
    #[test]
    fn ares_team_retaliate_focuses_an_enemy_attacker() {
        let _ares = crate::rules::ares_switch::read_ares_way(true);
        for retaliate in [false, true] {
            let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
                "[General]\nTeamRetaliate={}\n\
                 [Countries]\n0=Americans\n1=Russians\n\
                 [VehicleTypes]\n0=TANK\n[WeaponTypes]\n0=GUN\n[Warheads]\n0=WH\n\
                 [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
                 [GUN]\nDamage=10\nRange=5\nROF=20\nWarhead=WH\n\
                 [TANK]\nStrength=300\nArmor=none\nSpeed=6\nPrimary=GUN\n",
                if retaliate { "yes" } else { "no" }
            ));
            let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).expect("rules");
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 7);
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            let member = sim
                .spawn_object("TANK", "Russians", 10, 10, 0, &rules)
                .expect("member");
            let attacker = sim
                .spawn_object("TANK", "Americans", 14, 10, 0, &rules)
                .expect("attacker");
            let team =
                crate::sim::team_script_vm::join_team_for_test(&mut sim, &[member], false, false);
            let state = sim.team_script_vm.teams.get_mut(&team).expect("team");
            state.formed = true;
            state.focus = None;
            sim.team_took_damage(member, Some(attacker), &rules, None);
            let focus = sim.team_script_vm.teams.get(&team).expect("team").focus;
            assert_eq!(
                focus == Some(TeamTarget::Object(attacker)),
                retaliate,
                "{focus:?}"
            );
        }
    }
}
