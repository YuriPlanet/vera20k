//! The script step of `TeamClass::AI @ 0x006E9140` (`0x006E9364..
//! 0x006E9455`), the actions it dispatches through the jump table at
//! `0x006E9F74`, and the routines they share: `Coordinate_Move @
//! 0x006EBAD0`, `Coordinate_Attack @ 0x006EB490` and `Assign_Mission_Target
//! @ 0x006E9050`.
//!
//! Native evidence: the instruction-level spec
//! `vera20k-dev/research/chain5/chain5_actions_native.md` and the saved
//! disassembly (`asm/teamai.asm`, `a11.asm`, `a54.asm`, `a58.asm`,
//! `findown.asm`, `coordmove.asm`, `assignmt.asm`); for actions 0 and 53
//! and `Coordinate_Attack`, `vera20k-dev/research/chain6/
//! attack_actions_leads.md` with `coordinate_attack.asm`,
//! `gather_enemy_base.asm` and `asm/team_action_attack_quarry_6ED090.asm`.
//! Native execution: `tools/team_recruit_oracle.py`'s `guard`, `regroup`
//! (with its draw), `own_building` and `target_building` rows replay in
//! `recruit_oracle_tests`, and its `load`, `wait_loaded`, `unload` and
//! `deploy` rows (actions 14, 43, 8 and 9) in `transport_tests`.
//!
//! RESIDUALS:
//! - Actions outside [`action_is_ported`] are not ported. A team that reaches
//!   one stays on it and records the first as its refusal; the rest of its
//!   update runs. Trigger: an authored script reaches an unsupported action.
//!   Effect: the team idles on that action for good, keeping its
//!   members and its TeamType's `Max=` slot, and nothing else sends those
//!   units out.
//! - Script22's LAN/WOL path remains refused: Map580BC0 reads process-current
//!   shroud586360; native LAN guest setup5E0841..5E0931 installs its local
//!   participant at index0, then688109 makes that House PlayerPtr. Feeding
//!   this local plane into shared Scenario RNG would make network results
//!   client-dependent. A deterministic network visibility contract is still
//!   required; this is not an unreachable native branch or a whole22 port.
//!   Such teams retain their members and Max slot. Native would request one
//!   Scenario range for a nonempty pool, with no private timer or detach;
//!   the refused handler currently performs none of those effects.
//! - `Coordinate_Attack` orders an unloading unit to attack without asking
//!   Unit vt+0x4E4 (`0x00736D50`), which keeps it unloading when its type is
//!   `DeployToFire=`, or when a computer unit's `DeploysInto=` building is
//!   `TickTank=` (`+0x16C4`), its target is a unit and Cell `0x00487C10`
//!   answers true for its own cell. Infantry's and Aircraft's `0x0041C060`
//!   answer false. No retail type sets either key, so every retail member
//!   answers false. That includes the "Soviet SChop" teams' Siege Choppers,
//!   which action 9 sets unloading just before the team's attack actions:
//!   natively, as in VERA, those actions take them off Unload in the air.
//!   Trigger: a member of a modded type deploying when its team attacks.
//!   Effect: it stops deploying and attacks.

use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::object_type::ObjectCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::fire_error::FireError;
use crate::sim::combat::{ScanMission, quarry_mask};
use crate::sim::components::NavTargetRef;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::BlockingObject;
use crate::sim::movement::ground_pose::{object_get_coords, position_world_coord};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;

use super::super_actions::ChronoshiftTarget;

#[cfg(test)]
#[path = "idle_animation_tests.rs"]
mod idle_animation_tests;
use super::team_ai::member_is_live_joined;
use super::{TeamScriptAction, TeamScriptRefusal, TeamScriptState, TeamTarget, script_action_at};
use crate::sim::find_nearby_cell::{NearbyArgs, NearbyFootprint, PassabilityArgs};
use crate::sim::intern::StringInterner;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::team_script_vm::TeamScriptVm;
use crate::sim::world::ObjectSubstrate;

#[cfg(test)]
#[path = "scout_tests.rs"]
mod scout_tests;

#[cfg(test)]
#[path = "result_tests.rs"]
mod result_tests;

#[cfg(test)]
#[path = "spy_waypoint_tests.rs"]
mod spy_waypoint_tests;

impl TeamScriptState {
    /// TeamFlash6EF5F0 walks +54/+5D8 head first and writes signed Techno+F0.
    /// Script50 inlines the same loop at6E9CFF..6E9D18. Neither caller asks
    /// TechnoFlash6F9DD0 or filters dead, limbo, disguised or foreign members.
    /// Original Steam gamemd; trigger_flash_team's native
    /// dispatch/member controls and team_creation's signed countdown rows.
    fn flash_members(&self, entities: &mut EntityStore, frames: i32) {
        for member in self.members() {
            if let Some(entity) = entities.get_mut(member) {
                entity.flashing.start(frames);
            }
        }
    }
}

impl Simulation {
    /// TeamTypeFlash6F1F30, called by TAction104 at6DF574. Matching teams
    /// in live registration order flash all their linked members, including
    /// teams owned by another House. Member writes have no callback and do
    /// not change the registry, so the existing order can be read directly.
    pub(crate) fn flash_teams_of_type(
        substrate: &mut ObjectSubstrate,
        team_script_vm: &TeamScriptVm,
        team_type: InternedId,
        frames: i32,
    ) {
        for team in team_script_vm.teams_in_order() {
            if team.team_type_id() == Some(team_type) {
                team.flash_members(&mut substrate.entities, frames);
            }
        }
    }
}

/// Whether the dispatch has a port of `action_id`'s handler. Mode-specific
/// residuals can still refuse a path (Script22 currently admits single player).
pub(super) const fn action_is_ported(action_id: i32) -> bool {
    matches!(
        action_id,
        0 | 1
            | 2
            | 3
            | 4
            | 5
            | 6
            | 7
            | 8
            | 9
            | 10
            | 11
            | 12
            | 13
            | 14
            | 15
            | 16
            | 17
            | 19
            | 20
            | 21
            | 22
            | 23
            | 24
            | 25
            | 26
            | 27
            | 30
            | 34
            | 37
            | 38
            | 39
            | 40
            | 41
            | 42
            | 43
            | 46
            | 47
            | 49
            | 50
            | 53
            | 54
            | 55
            | 56
            | 57
            | 58
            | 59
            | 60
            | 61
            | 62
            | 63
            | 64
    )
}

/// A member's NavCom as a team target, for `Distance`.
fn nav_target(nav: NavTargetRef) -> TeamTarget {
    match nav {
        NavTargetRef::Cell { rx, ry } => TeamTarget::Cell {
            x: rx as i16,
            y: ry as i16,
        },
        NavTargetRef::Object { id } => TeamTarget::Object(id),
    }
}

impl Simulation {
    /// `0x006E9364..0x006E9455`: a finished action (`+0x80`) steps the script
    /// and runs the next one at once with `first` set: each member drops its
    /// archive target, unless the TeamType is `TransportsReturnOnUnload=`
    /// and the member carries passengers; a script run past its end
    /// destroys the team; the mission target and move target are cleared.
    /// Otherwise the move target falls back to the mission target. Actions
    /// outside `0..=0x40` (and the -1 before the first) do nothing.
    pub(super) fn team_step_script(
        &mut self,
        team_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let vm = &mut self.team_script_vm;
        let Some(team) = vm.teams.get_mut(&team_id) else {
            return;
        };
        let mut first = false;
        if team.advance_pending {
            team.advance_pending = false;
            first = true;
            team.cursor = team.cursor.wrapping_add(1);
            let cursor = team.cursor;
            let members: Vec<u64> = team.members().collect();
            let keep_archive = team
                .team_type_id
                .and_then(|id| vm.team_type_ini.get(&id))
                .is_some_and(|metadata| metadata.transports_return_on_unload);
            let count = vm
                .scripts
                .get(&team.script_id)
                .map_or(0, |script| script.actions.len() as u32);
            for member in members {
                let carries = keep_archive
                    && self
                        .substrate
                        .entities
                        .get(member)
                        .and_then(|entity| self.object_type(entity.type_ref(), rules))
                        .is_some_and(|object| object.passengers > 0);
                if !carries {
                    self.substrate.team_member_clear_archive_target(member);
                }
            }
            if (cursor as u32) >= count {
                self.destroy_team(team_id, rules, frame_effects);
                return;
            }
            self.team_assign_mission_target(team_id, None, rules, registry, frame_effects);
            if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                team.focus = None;
            }
        } else if team.focus.is_none() {
            team.focus = team.mission_target;
        }
        // `0x006E940F..0x006E942F`: `Coordinate_Attack`'s restart.
        if self
            .team_script_vm
            .teams
            .get_mut(&team_id)
            .is_some_and(|team| std::mem::take(&mut team.retarget))
        {
            first = true;
            self.team_assign_mission_target(team_id, None, rules, registry, frame_effects);
            if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                team.focus = None;
            }
        }
        let vm = &self.team_script_vm;
        let Some(team) = vm.teams.get(&team_id) else {
            return;
        };
        let action = script_action_at(vm.scripts.get(&team.script_id), team.cursor).unwrap_or(
            TeamScriptAction {
                action_id: -1,
                argument: 0,
            },
        );
        if action.action_id as u32 > 0x40 {
            return;
        }
        self.team_run_action(team_id, action, first, rules, registry, frame_effects);
    }

    /// The jump table's cases.
    fn team_run_action(
        &mut self,
        team_id: u64,
        action: TeamScriptAction,
        first: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let current_frame = self.session.binary_frame() as i32;
        match action.action_id {
            0 => self.team_action_attack_quarry(
                team_id,
                action.argument,
                rules,
                registry,
                frame_effects,
            ),
            1 => self.team_action_attack_waypoint(
                team_id,
                action.argument,
                first,
                rules,
                registry,
                frame_effects,
            ),
            15 => self.team_action_spy_waypoint(
                team_id,
                action.argument,
                first,
                rules,
                registry,
                frame_effects,
            ),
            59 => self.team_action_attack_waypoint_building(
                team_id,
                action.argument,
                first,
                rules,
                registry,
                frame_effects,
            ),
            // `0x006E95AB`: returns; the team stays on the action.
            2 => {}
            3 => self.team_action_move_to_waypoint(
                team_id,
                action.argument,
                first,
                rules,
                registry,
                frame_effects,
            ),
            // TeamClass::AI Script4, 6E95B3..6E95EA: signed packed128
            // coordinates, Map5657A0, AssignMissionTarget, CoordinateMove.
            // The shared owners retain member orders, timers and cleanup;
            // this handler has no private RNG draw, timer or detach call.
            4 => {
                if first {
                    let target = self.canonical_team_cell((
                        (action.argument % 128) as i16,
                        (action.argument / 128) as i16,
                    ));
                    self.team_assign_mission_target(
                        team_id,
                        Some(target),
                        rules,
                        registry,
                        frame_effects,
                    );
                }
                self.team_coordinate_move(team_id, rules, registry, frame_effects);
            }
            // TeamClass::AI Script22 at6E9583: first visit uses centre or
            // head, Map580BC0 and AssignMissionTarget; every visit moves.
            // Its script argument is unused. Shared movement owns timers
            // and detach/EOF cleanup; only the search can request RNG.
            22 => {
                if !self.session.game_mode().is_single_player() {
                    self.refuse_team_script_action(team_id, action.action_id);
                    return;
                }
                if first {
                    let origin = self.team_script_vm.teams.get(&team_id).and_then(|team| {
                        team.zone
                            .or_else(|| team.members().next().map(TeamTarget::Object))
                    });
                    let Some(viewer) = self.session.current_house() else {
                        return;
                    };
                    let target = origin
                        .and_then(|origin| self.nearby_shrouded_cell(origin.target_kind(), viewer))
                        .map(|target| {
                            TeamTarget::from(crate::sim::combat::TargetKind::from(target))
                        });
                    self.team_assign_mission_target(
                        team_id,
                        target,
                        rules,
                        registry,
                        frame_effects,
                    );
                }
                self.team_coordinate_move(team_id, rules, registry, frame_effects);
            }
            // TeamClass::AI Script7/23, table6E9F90/6E9FD0 ->6E98C9/6E98E5:
            // FlagWin4FC9E0/FlagLose4FCBD0(PlayerPtr, false), then Team+80=1.
            // Original gamemd instructions: no first-visit/argument gate,
            // RNG draw, private timer or detach; House owns SavourDelay+298.
            7 | 23 => {
                self.request_player_outcome(
                    crate::sim::world::PlayerOutcomeRequest::Flag {
                        kind: if action.action_id == 7 {
                            crate::sim::house_state::HouseOutcomeKind::Victory
                        } else {
                            crate::sim::house_state::HouseOutcomeKind::Defeat
                        },
                        skip_timer: false,
                    },
                    rules,
                    frame_effects,
                );
                if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                    team.advance_pending = true;
                }
            }
            // `0x006E97CE`: the guard timer starts on the first frame at
            // `argument * 15` frames; each frame the team regroups (the result
            // is ignored) and moves on once the timer has run out.
            5 => {
                if first && let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                    team.guard_timer = crate::sim::timer::CdTimer::started(
                        current_frame,
                        guard_frames(action.argument),
                    );
                }
                self.team_regroup(team_id, rules, registry, frame_effects);
                if let Some(team) = self.team_script_vm.teams.get_mut(&team_id)
                    && team.guard_timer.remaining(current_frame) == 0
                {
                    team.advance_pending = true;
                }
            }
            // `0x006E98AE`: jump to line `argument` (1-based) through the
            // step, which adds one (`ScriptClass::Seek @ 0x006915A0`).
            6 => {
                if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                    team.cursor = action.argument.wrapping_sub(2);
                    team.advance_pending = true;
                }
            }
            8 => self.team_action_unload(team_id, action.argument, rules, registry, frame_effects),
            9 => self.team_action_deploy(team_id, rules, registry, frame_effects),
            10 => self.team_action_follow_friendlies(team_id, rules, registry, frame_effects),
            11 => self.team_action_do_mission(
                team_id,
                action.argument,
                rules,
                registry,
                frame_effects,
            ),
            // gamemd.exe TeamClass::AI Script13,6E9FA8 ->6E9879..98AD:
            // each member's class virtual520(argument), next link read AFTER
            // the callback, then +80=1. Refused animations still advance.
            13 => {
                let mut member = self
                    .team_script_vm
                    .teams
                    .get(&team_id)
                    .and_then(|team| team.members().next());
                while let Some(current) = member {
                    self.foot_play_idle_animation(current, action.argument, rules, frame_effects);
                    member = self
                        .team_script_vm
                        .teams
                        .get(&team_id)
                        .and_then(|team| team.next_member_after(current));
                }
                if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                    team.advance_pending = true;
                }
            }
            14 => self.team_action_load_onto_transport(team_id, rules, registry, frame_effects),
            16 => self.team_action_patrol_waypoint(
                team_id,
                action.argument,
                first,
                rules,
                registry,
                frame_effects,
            ),
            17 => {
                // Steam gamemd:6E972C..6E977F destroys the current Script,
                // constructs one from ScriptType::Array8B41CC[argument],
                // then Reset691590 sets its cursor to-1. It does NOT set
                // Team+80. Native Current691500 consequently returns -1 on
                // subsequent visits until another owner advances the Team.
                // Preserve mission/focus, members, guard timer and RNG. The
                // original constructor/destructor's instance registry has
                // no independent Rust identity: this VM owns the attachment.
                // tools/team_recruit_oracle.py --change-script executes the
                // caller, constructor/reset and the following script visit.
                let replacement = usize::try_from(action.argument)
                    .ok()
                    .and_then(|index| self.team_script_vm.script_order.get(index))
                    .copied();
                if let Some(replacement) = replacement {
                    if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                        team.script_id = replacement;
                        team.cursor = -1;
                    }
                } else {
                    // Native directly indexes the array and can fault. Keep
                    // the attachment on malformed authored input instead.
                    log::warn!(
                        "Team {team_id} Script17 names invalid ScriptType ordinal {}",
                        action.argument
                    );
                }
            }
            // Original gamemd.exe TeamClass::AI Script34, table6E9FFC ->
            //6E9ACF..6E9B6A: Zone+34 GetCoords, Ground578080, then Map565730
            // bridge flags100/400. Keep subcell XY even for an object Zone.
            // FocusOn6D2420 copies the coordinate; Team+80 completes now,
            // independently of the camera's progress. No House/first gate.
            34 => {
                let coord = self
                    .team_script_vm
                    .teams
                    .get(&team_id)
                    .and_then(|team| team.zone)
                    .and_then(|zone| self.team_target_coord(zone));
                if let (Some(coord), Some(terrain)) = (coord, self.resolved_terrain.as_ref()) {
                    let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
                    let ground =
                        crate::sim::movement::ground_pose::query_ground_height(&cells, coord)
                            .expect("authored camera ground query requires a valid native slope");
                    let cell = cells.lookup_world(coord.x, coord.y);
                    let on_bridge = cells.flags(cell) & 0x100 != 0
                        || cells.flags(cells.lookup_world(coord.x, coord.y)) & 0x400 != 0;
                    let z = crate::sim::movement::ground_pose::z_at_height(ground, 0, on_bridge);
                    self.publish_trigger_effects(vec![
                        crate::sim::trigger_runtime::TriggerEffect::CameraAtCoord {
                            coord: [coord.x, coord.y, z],
                            velocity: Some(action.argument),
                        },
                    ]);
                }
                if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                    team.advance_pending = true;
                }
            }
            37 => {
                //Steam gamemd:6E954D..6E957A caches Foot+5D8 before
                //vtD4 Limbo and vtF8 UnInit, then marks+80 to advance.
                //Neither direct deletion nor a damage/RecordKill path is
                //this action's call. Reuse the complete class lifecycle.
                let context = crate::sim::world::UninitContext::new(rules, registry)
                    .with_effects(frame_effects);
                let mut member = self
                    .team_script_vm
                    .teams
                    .get(&team_id)
                    .and_then(|team| team.members().next());
                while let Some(current) = member {
                    let next = self
                        .team_script_vm
                        .teams
                        .get(&team_id)
                        .and_then(|team| team.next_member_after(current));
                    let _ = self.techno_limbo_with_context(current, context);
                    self.uninit_with_context(current, context);
                    member = next;
                }
                if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                    team.advance_pending = true;
                }
            }
            64 => {
                //6E9F0D -> each member's vt34C/Foot4DFE00. Cache the next
                //member before its seek callback; successful entrants leave
                //the Team via the shared6EA870 owner with no_idle=true.
                let mut member = self
                    .team_script_vm
                    .teams
                    .get(&team_id)
                    .and_then(|team| team.members().next());
                while let Some(current) = member {
                    let next = self
                        .team_script_vm
                        .teams
                        .get(&team_id)
                        .and_then(|team| team.next_member_after(current));
                    if self.foot_enter_nearest_occupiable(current, rules, registry, frame_effects) {
                        self.team_remove_member(team_id, current, true, rules, frame_effects);
                    }
                    member = next;
                }
                if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                    team.advance_pending = true;
                }
            }
            // Retail TeamClass::AI6E9140's active table6E9F74 sends
            // Script12/38 to6E982B/6E9530 -> SetGlobal689670;39/40 at
            //6E9BA1/6E9BBE call SetLocal689910. All complete Team+80
            // even for a rejected or unchanged value. The shared trigger
            // owner retains dirty admission, linked timers and RNG order.
            12 | 38 | 39 | 40 => {
                Self::set_trigger_variable(
                    &self.session,
                    &mut self.scenario_rng,
                    &mut self.trigger_runtime,
                    matches!(action.action_id, 12 | 38),
                    action.argument,
                    matches!(action.action_id, 12 | 39),
                );
                if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                    team.advance_pending = true;
                }
            }
            42 => Self::team_action_force_facing(
                &self.session,
                &mut self.substrate,
                &mut self.team_script_vm,
                team_id,
                action.argument,
            ),
            // gamemd.exe Team19@6E984B/41@6E9BDB: each member's
            // Panic+518/UnPanic+51C in list order, then advance+80=1.
            // The callbacks do not unlink; the next action waits for the
            // next Team visit. Fear/stance remain with the Infantry owner.
            19 | 41 => {
                let members: Vec<u64> = self
                    .team_script_vm
                    .teams
                    .get(&team_id)
                    .map(|team| team.members().collect())
                    .unwrap_or_default();
                for member in members {
                    let Some(object) = self
                        .substrate
                        .entities
                        .get(member)
                        .and_then(|entity| self.object_type(entity.type_ref(), rules))
                    else {
                        continue;
                    };
                    if let Some(entity) = self.substrate.entities.get_mut(member) {
                        if action.action_id == 19 {
                            crate::sim::infantry::apply_panic_force(object, entity);
                        } else {
                            crate::sim::infantry::apply_unpanic_force(object, entity);
                        }
                    }
                }
                if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                    team.advance_pending = true;
                }
            }
            // Steam gamemd: Script20 at6E9683..6E96BC caches
            // Foot+5D8 before each ChangeOwner(vt+3D4), resolving the
            // Country through502D30 for every member, then sets Team+80.
            // The class owner can unlink a transferred member; neither
            // Team.House nor a Player@ start-slot selector is changed here.
            20 => {
                let mut member = self
                    .team_script_vm
                    .teams
                    .get(&team_id)
                    .and_then(|team| team.members().next());
                while let Some(current) = member {
                    let next = self
                        .team_script_vm
                        .teams
                        .get(&team_id)
                        .and_then(|team| team.next_member_after(current));
                    let owner = u16::try_from(action.argument).ok().and_then(|country| {
                        self.house_by_country_index(
                            crate::rules::ruleset::CountryIdx(country),
                            rules,
                        )
                    });
                    // An unresolved Country is ignored safely. Native null-
                    // House handling is outside the admitted SOV01 route.
                    if let Some(owner) = owner {
                        self.change_owner_with_rules(
                            current,
                            owner,
                            rules,
                            registry,
                            frame_effects,
                        );
                    }
                    member = next;
                }
                if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                    team.advance_pending = true;
                }
            }
            // `0x006E9650`: each member, in list order, scatters from the
            // team code's zero coordinate (`0x00B0E968`, written only by its
            // static initializer `0x006E8A30`), forced and not no-kidding
            // (`vt+0x174(&zero, 1, 0)`); the action is finished at once.
            21 => {
                let members: Vec<u64> = self
                    .team_script_vm
                    .teams
                    .get(&team_id)
                    .map(|team| team.members().collect())
                    .unwrap_or_default();
                for member in members {
                    if let Err(cause) = self.scatter(
                        member,
                        None,
                        crate::sim::movement::ScatterFlags::new(true, false),
                        rules,
                        registry,
                        frame_effects,
                    ) {
                        log::debug!("team member {member} did not scatter: {cause}");
                    }
                }
                if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                    team.advance_pending = true;
                }
            }
            // `TeamClass::AI` case 25 (`0x006E9901..0x006E9912`): PlayAtPos
            // `0x00750920` of the raw argument, centred (`0x2000`), volume
            // 1.0, no handle, then Team+80=1. The argument is a SOUNDMD
            // ordinal the process catalog resolves.
            25 => {
                self.sound_events
                    .emit(crate::sim::world::SimSoundEvent::VocCenteredIndex {
                        index: action.argument,
                    });
                frame_effects.flush_sound(self, rules);
                if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                    team.advance_pending = true;
                }
            }
            // Case 26 (`0x006E9921..0x006E992F`): the full-screen movie
            // `0x005BF260(argument, -1, 1, 1, 1)`, then Team+80=1. Trigger
            // action 10 makes the same call inside a bracket this case skips:
            // mouse hide/show, `0x006842F0` display mode and the `0x0072DDB0`
            // shell surfaces. VERA's one movie owner presents both alike
            // (presentation only).
            26 => {
                self.publish_trigger_effects(vec![
                    crate::sim::trigger_runtime::TriggerEffect::FullscreenMovie {
                        index: action.argument,
                    },
                ]);
                if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                    team.advance_pending = true;
                }
            }
            // Case 27 (`0x006E993E..0x006E9949`): Theme Queue_Song
            // `0x00720B20` of the argument, then Team+80=1. Trigger action
            // 20's ordinal form takes the same Theme request.
            27 => {
                self.publish_trigger_effects(vec![
                    crate::sim::trigger_runtime::TriggerEffect::MusicTheme {
                        request: crate::sim::trigger_runtime::ThemeRequest::Index(action.argument),
                    },
                ]);
                if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                    team.advance_pending = true;
                }
            }
            // Original gamemd,6E9FEC ->6E99E1..6E99FD
            // (tools/native_inspect): House+250=4, then Team+80=1.
            // Argument/first are unused; the existing House Strategy timer
            // decides when FireSale/Hunt runs. No direct RNG/timer/detach.
            30 => {
                let owner = self
                    .team_script_vm
                    .teams
                    .get(&team_id)
                    .map(TeamScriptState::owner);
                if let Some(house) = owner.and_then(|owner| self.houses.get_mut(&owner)) {
                    house.strategy_emergency_mut().set_state_four();
                }
                if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                    team.advance_pending = true;
                }
            }
            43 => Self::team_action_wait_until_loaded(
                &self.interner,
                &self.substrate,
                &mut self.team_script_vm,
                team_id,
                rules,
            ),
            46 => self.team_action_attack_enemy_building(
                team_id,
                action.argument,
                first,
                rules,
                registry,
                frame_effects,
            ),
            47 => self.team_action_move_to_enemy_building(
                team_id,
                action.argument,
                rules,
                registry,
                frame_effects,
            ),
            // `0x006E94BA`, and `0x006E9CE1` which also records success
            // (`+0x84`), counted when the team is destroyed.
            24 | 49 => {
                if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                    team.advance_pending = true;
                    team.succeeded |= action.action_id == 49;
                }
            }
            // Original6E9CF7..6E9D12: signed operand goes straight to each
            // member's existing Techno+F0, head first, without an alive gate.
            50 => {
                if first {
                    if let Some(team) = self.team_script_vm.teams.get(&team_id) {
                        team.flash_members(&mut self.substrate.entities, action.argument);
                    }
                    if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                        team.advance_pending = true;
                    }
                }
            }
            53 => self.team_action_gather_at_enemy_base(
                team_id,
                first,
                rules,
                registry,
                frame_effects,
            ),
            54 => self.team_action_regroup_at_base(team_id, first, rules, registry, frame_effects),
            55 => self.team_action_iron_curtain(team_id, rules, registry, frame_effects),
            56 => self.team_action_chronoshift(
                team_id,
                ChronoshiftTarget::Building(action.argument),
                rules,
                registry,
                frame_effects,
            ),
            57 => self.team_action_chronoshift(
                team_id,
                ChronoshiftTarget::Quarry(action.argument),
                rules,
                registry,
                frame_effects,
            ),
            58 => self.team_action_move_to_own_building(
                team_id,
                action.argument,
                first,
                rules,
                registry,
                frame_effects,
            ),
            // `0x006E9DF9..0x006E9E3D` (60),
            // `0x006E9E3E..0x006E9E82` (61), `0x006E9E83..0x006E9EC7` (62)
            // and `0x006E9EC8..` (63): each member, in list order, seeks a
            // grinder (vt+0x33C), tank bunker (Foot vt+0x344),
            // a Bio Reactor (vt+0x340, [`Self::foot_enter_bio_reactor`]) or
            // an occupiable building (vt+0x348,
            // [`Self::foot_enter_battle_bunker`]); one that finds one leaves
            // the team (`Remove_Member(member, -1, 1)`, `0x006E9E69`,
            // `0x006E9EAE`, `0x006E9EF3`). The action is finished at once,
            // also with no member (`0x006E953E`).
            60..=63 => {
                let members: Vec<u64> = self
                    .team_script_vm
                    .teams
                    .get(&team_id)
                    .map(|team| team.members().collect())
                    .unwrap_or_default();
                for member in members {
                    let found = match action.action_id {
                        60 => self.foot_enter_grinder(member, rules, registry, frame_effects),
                        61 => self.foot_enter_tank_bunker(member, rules, registry, frame_effects),
                        62 => self.foot_enter_bio_reactor(member, rules, registry, frame_effects),
                        _ => self.foot_enter_battle_bunker(member, rules, registry, frame_effects),
                    };
                    if found {
                        self.team_remove_member(team_id, member, true, rules, frame_effects);
                    }
                }
                if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                    team.advance_pending = true;
                }
            }
            action_id => {
                debug_assert!(!action_is_ported(action_id));
                self.refuse_team_script_action(team_id, action_id);
            }
        }
    }

    /// Keep the existing first-refusal diagnostic for unported action paths.
    fn refuse_team_script_action(&mut self, team_id: u64, action_id: i32) {
        if let Some(team) = self.team_script_vm.teams.get_mut(&team_id)
            && team.refusal.is_none()
        {
            team.refusal = Some(TeamScriptRefusal::UnsupportedAction { action_id });
            log::warn!("TeamClass::AI: team {team_id} stays on unported script action {action_id}");
        }
    }

    /// Sole IsMovingToWaypointOutsidePlayfield6EC300 port. All Foot and
    /// Aircraft consumers use the same Team cursor, Scenario waypoint owner
    /// and height-aware Map578460(mode1) lookup, including shared Dummy writes.
    fn team_is_moving_outside_playfield(&self, team_id: u64) -> bool {
        self.team_script_vm
            .team_step_waypoint(team_id)
            .and_then(|index| self.session.waypoint_cell(index))
            .is_some_and(|(x, y)| {
                !crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                    (i32::from(x), i32::from(y)),
                    self.playfield_bounds(),
                    self.resolved_terrain.as_ref(),
                )
            })
    }

    pub(crate) fn member_team_is_moving_outside_playfield(&self, member: u64) -> bool {
        self.team_script_vm
            .member_team
            .get(&member)
            .is_some_and(|&team| self.team_is_moving_outside_playfield(team))
    }

    /// Script3 / original6EC7D0. The established leader and CoordinateMove
    /// owners are shared with the other move actions. FNPC uses native MZ0
    /// and no zone constraint, a1x1 footprint and the frame-counter pick.
    fn team_action_move_to_waypoint(
        &mut self,
        team_id: u64,
        waypoint: i32,
        first: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        if first {
            let Some(leader) = self.team_leader(team_id, rules) else {
                return;
            };
            let Some(mut cell) = self.session.waypoint_cell(waypoint) else {
                log::warn!("Team {team_id} script3 names invalid waypoint {waypoint}");
                return;
            };
            let target = self.canonical_team_cell(cell);
            self.team_assign_mission_target(team_id, Some(target), rules, registry, effects);
            if !self.team_is_moving_outside_playfield(team_id) {
                use crate::sim::movement::infantry_entry::{EntryQueryMode, InfantryEntryArgs};
                let entry = self
                    .mover_can_enter(
                        leader,
                        cell,
                        InfantryEntryArgs::REPAIR,
                        EntryQueryMode::SkipLocomotor,
                        rules,
                        registry,
                    )
                    .expect("Team script3 uses its member's shared class entry receiver");
                if entry != 0 {
                    let speed = self
                        .substrate
                        .entities
                        .get(leader)
                        .and_then(|entity| self.object_type(entity.type_ref(), rules))
                        .expect("Team leader has its resolved TechnoType")
                        .speed_type;
                    cell = self
                        .nearby_passable_cell(
                            (i32::from(cell.0), i32::from(cell.1)),
                            NearbyArgs::plain(
                                PassabilityArgs {
                                    speed_type: speed,
                                    required_zone_id: None,
                                    movement_zone:
                                        crate::rules::locomotor_type::MovementZone::Normal,
                                    bridge_aware_zone: false,
                                },
                                NearbyFootprint::SINGLE,
                            ),
                        )
                        .map_or((0, 0), |(x, y)| (x as i16, y as i16));
                }
            }
            let target = (cell != (0, 0)).then(|| self.canonical_team_cell(cell));
            self.team_assign_mission_target(team_id, target, rules, registry, effects);
        }
        self.team_coordinate_move(team_id, rules, registry, effects);
    }

    /// Map5657A0 before a Team retains its Cell pointer. Real cells use the
    /// selected allocation's coordinates; a miss retains its invalid request
    /// as the Dummy token. Later virtual queries read that allocation's live
    /// state through the shared target-coordinate owner.
    pub(super) fn canonical_team_cell(&self, (x, y): (i16, i16)) -> TeamTarget {
        let (x, y) = self.resolved_terrain.as_ref().map_or((x, y), |terrain| {
            terrain.native_cell_coord(terrain.native_cell_identity((x, y)))
        });
        TeamTarget::Cell { x, y }
    }

    /// Script42 / Team6EDCA0, dispatched at6E9C15. Every member is read
    /// head first without alive/limbo/initiated filtering. A moving locomotor
    /// keeps the action pending; otherwise low16(argument<<13) is compared
    /// with PrimaryFacing::Current and a differing direction goes through
    /// ILocomotion+4C. Completion waits for moving, not the facing timer.
    /// Native Steam gamemd; tools/team_recruit_oracle.py
    /// --force-facing. Missing locomotors fault natively; Rust retains the
    /// pending action and reports the invalid state instead.
    fn team_action_force_facing(
        session: &ScenarioSession,
        substrate: &mut ObjectSubstrate,
        team_script_vm: &mut TeamScriptVm,
        team_id: u64,
        argument: i32,
    ) {
        let Some(team) = team_script_vm.teams.get(&team_id) else {
            return;
        };
        let members: Vec<_> = team.members().collect();
        let desired = argument.wrapping_shl(13) as u16;
        let frame = session.binary_frame();
        let mut finished = true;
        for member in members {
            let Some(entity) = substrate.entities.get_mut(member) else {
                finished = false;
                continue;
            };
            match crate::sim::movement::motion_query::is_moving(entity) {
                Some(true) => finished = false,
                Some(false) => {
                    if entity.body_facing_current(frame) != desired {
                        crate::sim::movement::locomotor_do_turn(entity, desired, frame);
                    }
                }
                None => {
                    log::error!("Team {team_id} Script42 member {member} has no active locomotor");
                    finished = false;
                }
            }
        }
        if finished && let Some(team) = team_script_vm.teams.get_mut(&team_id) {
            team.advance_pending = true;
        }
    }

    /// Script16 / Team6ECCE0..6ECE51, dispatched at6E962A. First takes
    /// the waypoint's Cell; an empty focus does so again only when the
    /// current script argument is unsigned-below702. On the global
    /// frame modulo ftol([AI]PatrolScan*900), the shared LeadershipRating
    /// leader asks GreatestThreat(1, its Location, false). A found object
    /// replaces the mission target. An empty scan clears a non-waypoint
    /// target; it does not immediately return to the waypoint in that visit.
    /// Object focus uses CoordinateAttack; Cell/NULL uses CoordinateMove.
    /// No RNG draw, timer write or detach in the handler; its shared setters
    /// retain member/radio/target cleanup. Steam gamemd;
    /// original instructions and reader, tools/team_recruit_oracle.py --patrol.
    /// Invalid modded periods whose IDIV faults natively skip scanning here
    /// with an error, retaining normal movement. This safety policy does not
    /// affect retail periods and is not claimed as native parity.
    fn team_action_patrol_waypoint(
        &mut self,
        team_id: u64,
        waypoint: i32,
        first: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        // Scenario68BCE0 resolves even a missing/out-of-range waypoint to
        // CellStruct::Empty before asking the existing canonical Map owner.
        let waypoint_target =
            self.canonical_team_cell(self.session.waypoint_cell(waypoint).unwrap_or((0, 0)));
        if first {
            self.team_assign_mission_target(
                team_id,
                Some(waypoint_target),
                rules,
                registry,
                effects,
            );
        }
        if self.team_script_vm.teams.get(&team_id).is_some_and(|team| {
            team.focus.is_none()
                && script_action_at(
                    self.team_script_vm.scripts.get(&team.script_id),
                    team.cursor,
                )
                .is_some_and(|action| (action.argument as u32) < 702)
        }) {
            self.team_assign_mission_target(
                team_id,
                Some(waypoint_target),
                rules,
                registry,
                effects,
            );
        }
        let frame = self.session.binary_frame() as i32;
        match frame.checked_rem(rules.general.patrol_scan_frames()) {
            Some(0) => {
                if let Some(leader) = self.team_leader(team_id, rules) {
                    let threat = crate::sim::world::direct_greatest_threat(
                        self,
                        rules,
                        registry,
                        leader,
                        ScanMission::Guard,
                    );
                    let focus = self
                        .team_script_vm
                        .teams
                        .get(&team_id)
                        .and_then(|team| team.focus);
                    // Team6ECE1A compares Abstract pointers, including Dummy.
                    if threat.is_some()
                        || focus.is_none_or(|focus| {
                            !NavTargetRef::from(focus.target_kind()).same_pointer(
                                waypoint_target.target_kind().into(),
                                self.resolved_terrain.as_ref(),
                            )
                        })
                    {
                        self.team_assign_mission_target(
                            team_id,
                            threat.map(TeamTarget::Object),
                            rules,
                            registry,
                            effects,
                        );
                    }
                }
            }
            Some(_) => {}
            None => log::error!("Team {team_id} Script16 has an invalid PatrolScan divisor"),
        }
        if self
            .team_script_vm
            .teams
            .get(&team_id)
            .is_some_and(|team| matches!(team.focus, Some(TeamTarget::Object(_))))
        {
            self.team_coordinate_attack(team_id, rules, registry, effects);
        } else {
            self.team_coordinate_move(team_id, rules, registry, effects);
        }
    }

    /// Script action 11 (`0x006ED7E0`, `first` unread): each live member
    /// joins up as in `Regroup`; each live joined member not fighting takes
    /// mission `mission` (clearing its archive target, target and
    /// destination) once idle and not already on it (a Guard order spares an
    /// unloading member); one without a destination more than twice stray
    /// from the centre is first pulled back to the centre's cell, unless the
    /// mission is Area Guard. The action never finishes.
    fn team_action_do_mission(
        &mut self,
        team_id: u64,
        mission: i32,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let Some(team) = self.team_script_vm.teams.get(&team_id) else {
            return;
        };
        let members: Vec<u64> = team.members().collect();
        for member in members {
            let Some(zone) = self
                .team_script_vm
                .teams
                .get(&team_id)
                .map(|team| team.zone)
            else {
                return;
            };
            let Some(entity) = self.substrate.entities.get(member) else {
                continue;
            };
            if !entity.lifecycle.object_alive() {
                continue;
            }
            self.team_member_join_up(team_id, member, rules, registry, frame_effects);
            let Some(entity) = self.substrate.entities.get(member) else {
                continue;
            };
            if !member_is_live_joined(
                entity,
                self.team_member_initiated(team_id, member),
                self.object_placement_scope_active(),
            ) || entity.attack_target.target().is_some()
            {
                continue;
            }
            if entity.navigation.nav_com().is_none() {
                let stray = self.team_stray(team_id, rules);
                if self.team_member_distance(entity, zone) > stray.wrapping_mul(2)
                    && mission != MissionType::AreaGuard as i32
                {
                    self.team_member_queue_mission(member, MissionType::Move, rules);
                    self.team_member_set_destination(member, zone, rules, registry, frame_effects);
                    self.team_member_queue_mission(member, MissionType::Move, rules);
                    let cell = zone.and_then(|zone| self.team_target_cell_of_coord(zone));
                    self.team_member_set_destination(member, cell, rules, registry, frame_effects);
                    continue;
                }
            }
            let Some(entity) = self.substrate.entities.get(member) else {
                continue;
            };
            if entity.attack_target.target().is_some() || entity.navigation.nav_com().is_some() {
                continue;
            }
            let current = entity.mission.effective().raw();
            if current == mission
                || (mission == MissionType::Guard as i32 && current == MissionType::Unload as i32)
            {
                continue;
            }
            self.substrate.team_member_clear_archive_target(member);
            self.team_member_queue_mission_id(member, MissionId::from_raw(mission), rules);
            self.team_member_clear_target(member, rules);
            self.team_member_set_destination(member, None, rules, registry, frame_effects);
        }
    }

    /// Whether team `team_id`'s TaskForce has an entry of an aircraft type
    /// with `Passengers=` above 0 (6EF470), which makes actions 14 and 8 take only
    /// aircraft as transports (`0x006ED203..0x006ED258`,
    /// `0x006EF2BD..0x006EF323`); otherwise only vehicles. No retail
    /// AircraftType has `Passengers=`. Reinforcement65DDAB uses this same
    /// decision before separating its Full transport/member chains.
    pub(super) fn team_air_transports(&self, team_id: u64, rules: &RuleSet) -> bool {
        let vm = &self.team_script_vm;
        vm.teams.get(&team_id).is_some_and(|team| {
            vm.task_force_entries(team).iter().any(|entry| {
                entry.member_type.category == ObjectCategory::Aircraft
                    && rules
                        .object_in_category(
                            entry.member_type.category,
                            self.interner.resolve(entry.member_type.id),
                        )
                        .is_some_and(|object| object.passengers > 0)
            })
        })
    }

    /// Script action 14 (`0x006ED200`, `first` unread), load onto transport.
    /// The transport is the first member, in list order, whose type's
    /// `Passengers=` is above its passenger count (`+0x114`) and that is a
    /// vehicle, or an aircraft with [`Self::team_air_transports`]; with none
    /// the action is finished (`0x006ED25A..0x006ED2CA`). While the transport
    /// is in radio contact (`0x0065AE30`) the action waits. Otherwise, while
    /// the team has more than one member (`+0x48`, read before each), each
    /// member joins up as in `Regroup`, and the first live joined one other
    /// than the transport whose mission (`vt+0x184`) is not Enter takes Enter
    /// with its target cleared and the transport as its destination; the
    /// action then returns, so one member is ordered per update
    /// (`0x006ED492..0x006ED4BD`). While any live joined member but the
    /// transport is left, the action waits. Once none is (a passenger leaves
    /// the team as it boards, by its Conceal's Foot `Detach_All`,
    /// `0x004D9744`), a `TransportsReturnOnUnload=` TeamType's transport
    /// archives the cell holding its Location (`0x006ED43F..0x006ED47C`), and
    /// the action is finished. An infantryman's destination setter makes the
    /// transport his radio contact (`0x0051AC25..0x0051AD17`), so the next
    /// member is ordered once he has boarded and his Limbo broke it.
    fn team_action_load_onto_transport(
        &mut self,
        team_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let transport_class = if self.team_air_transports(team_id, rules) {
            EntityCategory::Aircraft
        } else {
            EntityCategory::Unit
        };
        let Some(team) = self.team_script_vm.teams.get(&team_id) else {
            return;
        };
        let members: Vec<u64> = team.members().collect();
        let transport = members.iter().copied().find(|&member| {
            self.substrate.entities.get(member).is_some_and(|entity| {
                entity.category() == transport_class
                    && self
                        .object_type(entity.type_ref(), rules)
                        .is_some_and(|object| {
                            i64::from(entity.passenger_role.passenger_count())
                                < i64::from(object.passengers)
                        })
            })
        });
        let Some(transport) = transport else {
            if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                team.advance_pending = true;
            }
            return;
        };
        if self
            .substrate
            .entities
            .get(transport)
            .is_some_and(|entity| !entity.radio_contacts.is_empty())
        {
            return;
        }
        let mut waiting = false;
        for member in members {
            if self
                .team_script_vm
                .teams
                .get(&team_id)
                .is_none_or(|team| team.members.len() <= 1)
            {
                break;
            }
            self.team_member_join_up(team_id, member, rules, registry, frame_effects);
            let Some(entity) = self.substrate.entities.get(member) else {
                continue;
            };
            if member == transport
                || !member_is_live_joined(
                    entity,
                    self.team_member_initiated(team_id, member),
                    self.object_placement_scope_active(),
                )
            {
                continue;
            }
            waiting = true;
            if entity.mission.effective() != MissionId::from_known(MissionType::Enter) {
                self.team_member_queue_mission(member, MissionType::Enter, rules);
                self.team_member_clear_target(member, rules);
                self.team_member_set_destination(
                    member,
                    Some(TeamTarget::Object(transport)),
                    rules,
                    registry,
                    frame_effects,
                );
                return;
            }
        }
        if waiting {
            return;
        }
        let vm = &self.team_script_vm;
        if vm
            .teams
            .get(&team_id)
            .is_some_and(|team| vm.transports_return_on_unload(team))
            && let Some(entity) = self.substrate.entities.get_mut(transport)
        {
            // Team6ED44C..6ED47C: raw Location -> ONE Map565730 lookup,
            // then SetArchiveTarget. Object+1BC would query twice.
            let physical = position_world_coord(&entity.position);
            let (x, y) = self.resolved_terrain.as_ref().map_or(
                ((physical.x / 256) as i16, (physical.y / 256) as i16),
                |terrain| {
                    let cells = NativeCellQuery::canonical(terrain);
                    cells.coord(cells.lookup_world(physical.x, physical.y))
                },
            );
            entity.set_archive_target(Some(crate::sim::combat::TargetKind::Cell(
                x as u16, y as u16,
            )));
        }
        if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
            team.advance_pending = true;
        }
    }

    /// Script action 43 (`0x006E9C22..0x006E9C5C`, inline), wait until
    /// fully loaded: the action is finished once no member's type has
    /// `Passengers=` above its passenger count (`+0x114`), also with no
    /// member (`0x006E953E`); it reads no other member state. The two retail
    /// Ivan teams (an HTK and three Crazy Ivans) never fill its five seats,
    /// so they wait here for good.
    fn team_action_wait_until_loaded(
        interner: &StringInterner,
        substrate: &ObjectSubstrate,
        team_script_vm: &mut TeamScriptVm,
        team_id: u64,
        rules: &RuleSet,
    ) {
        let Some(team) = team_script_vm.teams.get(&team_id) else {
            return;
        };
        let loading = team.members().any(|member| {
            substrate.entities.get(member).is_some_and(|entity| {
                interner
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| {
                        i64::from(object.passengers)
                            > i64::from(entity.passenger_role.passenger_count())
                    })
            })
        });
        if !loading && let Some(team) = team_script_vm.teams.get_mut(&team_id) {
            team.advance_pending = true;
        }
    }

    /// Script action 8 (`0x006EF110`, `first` unread), unload. Each member
    /// joins up as in `Regroup` (`0x006EF13B..0x006EF1ED`); each live joined
    /// one carrying a passenger (`+0x118`) keeps the action waiting, and
    /// unless a building stands in the cell holding its GetCoords
    /// (`Look_up_building_in_cell @ 0x0047C520`) or its mission (`vt+0x184`)
    /// is Unload, drops its destination and target and queues Unload
    /// (`0x006EF23A..0x006EF2A2`); each passenger it lets out rejoins the
    /// team (`0x0073DC19`, [`crate::sim::transport_unload`]). Once none
    /// carries one, each member, in list order (`0x006EF2BD..0x006EF43F`), is
    /// a transport when its type's `Passengers=` is above 0 (and, with
    /// [`Self::team_air_transports`], it is an aircraft), another member when
    /// that is 0 (or it is not an aircraft there), else neither:
    /// - a transport of a `TransportsReturnOnUnload=` TeamType leaves the
    ///   team (`Remove_Member(member, -1, 0)`), drops its target, heads for
    ///   its ArchiveTarget on Move (commencing at once when ready) and drops
    ///   the archive;
    /// - otherwise `argument` decides who leaves the team, dropping target
    ///   and destination: 1 the other members, 2 the transports, 3 both, any
    ///   other value none.
    ///
    /// The action is then finished.
    fn team_action_unload(
        &mut self,
        team_id: u64,
        argument: i32,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let Some(team) = self.team_script_vm.teams.get(&team_id) else {
            return;
        };
        let members: Vec<u64> = team.members().collect();
        let mut unloaded = true;
        for &member in &members {
            self.team_member_join_up(team_id, member, rules, registry, frame_effects);
            let Some(entity) = self.substrate.entities.get(member) else {
                continue;
            };
            if !member_is_live_joined(
                entity,
                self.team_member_initiated(team_id, member),
                self.object_placement_scope_active(),
            ) || entity.passenger_role.passenger_count() == 0
            {
                continue;
            }
            unloaded = false;
            let coords = object_get_coords(entity, self.resolved_terrain.as_ref());
            let on_building = self
                .substrate
                .occupancy
                .first_building_on_layer(
                    (coords.x / 256) as u16,
                    (coords.y / 256) as u16,
                    MovementLayer::Ground,
                )
                .is_some();
            if on_building
                || entity.mission.effective() == MissionId::from_known(MissionType::Unload)
            {
                continue;
            }
            self.team_member_set_destination(member, None, rules, registry, frame_effects);
            self.team_member_clear_target(member, rules);
            self.team_member_queue_mission(member, MissionType::Unload, rules);
        }
        if !unloaded {
            return;
        }
        let transport_class = self
            .team_air_transports(team_id, rules)
            .then_some(EntityCategory::Aircraft);
        let vm = &self.team_script_vm;
        let returns = vm
            .teams
            .get(&team_id)
            .is_some_and(|team| vm.transports_return_on_unload(team));
        for member in members {
            let Some(entity) = self.substrate.entities.get(member) else {
                continue;
            };
            let passengers = self
                .object_type(entity.type_ref(), rules)
                .map_or(0, |object| object.passengers);
            let transport =
                passengers != 0 && transport_class.is_none_or(|class| entity.category() == class);
            if transport && passengers < 0 {
                continue;
            }
            if transport && returns {
                self.team_remove_member(team_id, member, false, rules, frame_effects);
                self.team_member_clear_target(member, rules);
                let archive = self
                    .substrate
                    .entities
                    .get(member)
                    .and_then(GameEntity::archive_target)
                    .map(TeamTarget::from);
                self.team_member_set_destination(member, archive, rules, registry, frame_effects);
                self.team_member_queue_mission(member, MissionType::Move, rules);
                self.team_member_commence(member, rules);
                self.substrate.team_member_clear_archive_target(member);
            } else if (transport && matches!(argument, 2 | 3))
                || (!transport && matches!(argument, 1 | 3))
            {
                self.team_remove_member(team_id, member, false, rules, frame_effects);
                self.team_member_clear_target(member, rules);
                self.team_member_set_destination(member, None, rules, registry, frame_effects);
            }
        }
        if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
            team.advance_pending = true;
        }
    }

    /// Script action 9 (`TeamClass::Script_Deploy @ 0x006ED4D0`, `first`
    /// unread), deploy. Each member joins up as in `Regroup`
    /// (`0x006ED4EF..0x006ED5A1`); then each live joined one (or aircraft):
    /// - a unit whose type `DeploysInto=` a building (`+0x404`) keeps the
    ///   action waiting (`0x006ED667`). Unless its mission (`vt+0x184`) is
    ///   Unload, it is lifted out of its cell (Mark(UP)) and the building is
    ///   asked whether it may stand with its origin at the unit's own cell
    ///   (`vt+0x1B8`, no (-1, -1) as Deploy and TryToDeploy take), for no
    ///   house (`BuildingTypeClass::CanPlaceAt`, `vt+0xA8`, `0x006ED6B3`).
    ///   If not, the site is cleared for the unit's house
    ///   (`Flush_For_Placement @ 0x0045EE70`, `0x006ED6E0`) and the unit is
    ///   put back; if so, it is put back, drops its destination and target
    ///   and queues Unload (`0x006ED6F3..0x006ED73A`), whose Mission_Unload
    ///   deploys it ([`crate::sim::mcv_deploy`]).
    /// - otherwise a unit of an `IsSimpleDeployer=` type (`+0xE13`) or an
    ///   infantryman of a `Deployer=` type (`0x005224E0`, `+0xEC8`) queues
    ///   Unload unless that is its mission (`0x006ED71B..0x006ED73A`).
    ///
    /// With no unit left waiting, the action is finished (`0x006ED74E`); a
    /// team with no members finishes it at once (`0x006ED4E1`). A deployed
    /// unit leaves the team as it becomes the building.
    fn team_action_deploy(
        &mut self,
        team_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let Some(team) = self.team_script_vm.teams.get(&team_id) else {
            return;
        };
        let members: Vec<u64> = team.members().collect();
        let mut deployed = true;
        for member in members {
            self.team_member_join_up(team_id, member, rules, registry, frame_effects);
            let Some(entity) = self.substrate.entities.get(member) else {
                continue;
            };
            if !member_is_live_joined(
                entity,
                self.team_member_initiated(team_id, member),
                self.object_placement_scope_active(),
            ) {
                continue;
            }
            let Some(object) = self.object_type(entity.type_ref(), rules) else {
                continue;
            };
            let unit = entity.category() == EntityCategory::Unit;
            let building = unit
                .then(|| crate::sim::mcv_deploy::deploys_into(self, entity, rules))
                .flatten();
            let deployer = (unit && object.is_simple_deployer)
                || (entity.category() == EntityCategory::Infantry && object.deployer);
            let unloading =
                entity.mission.effective() == MissionId::from_known(MissionType::Unload);
            let cell = (entity.position.rx() as i16, entity.position.ry() as i16);
            let owner = entity.owner();
            if let Some(building) = building {
                deployed = false;
                if unloading {
                    continue;
                }
                self.foot_mark_remove(member, rules, registry, frame_effects);
                let placeable = crate::sim::build_site::can_place_building_at(
                    self, rules, registry, building, cell, None, None, None,
                );
                if !placeable {
                    let _ = crate::sim::build_site::flush_for_placement(
                        self,
                        rules,
                        registry,
                        building,
                        cell,
                        owner,
                        frame_effects,
                    );
                }
                self.foot_mark_put(member, rules, registry, frame_effects);
                if placeable {
                    self.team_member_set_destination(member, None, rules, registry, frame_effects);
                    self.team_member_clear_target(member, rules);
                    self.team_member_queue_mission(member, MissionType::Unload, rules);
                }
            } else if deployer && !unloading {
                self.team_member_queue_mission(member, MissionType::Unload, rules);
            }
        }
        if deployed && let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
            team.advance_pending = true;
        }
    }

    /// The member with the highest `LeadershipRating=` (signed, the first in
    /// list order on a tie) among the live joined members, else the head
    /// (`0x006EE621..0x006EE683`, `0x006EFA2B..0x006EFA81`), also the
    /// shared GetLeader6EC3D0 called by House50D170.
    pub(crate) fn team_leader(&self, team_id: u64, rules: &RuleSet) -> Option<u64> {
        let team = self.team_script_vm.teams.get(&team_id)?;
        let mut leader = team.members.first()?.id;
        let mut best = -1i32;
        for member in &team.members {
            let Some(entity) = self.substrate.entities.get(member.id) else {
                continue;
            };
            let rating = self
                .object_type(entity.type_ref(), rules)
                .map_or(0, |object| object.leadership_rating);
            if member_is_live_joined(
                entity,
                member.initiated,
                self.object_placement_scope_active(),
            ) && rating > best
            {
                leader = member.id;
                best = rating;
            }
        }
        Some(leader)
    }

    /// Script action 54 (`0x006EFA10`), regroup at base: on its first frame
    /// the team's mission target becomes the cell `AISafeDistance=` cells
    /// from the leader's base centre towards its enemy's (a facing drawn
    /// with `RandomRanged(0, 255)` on the Scenario stream without an enemy):
    /// the passable 3x3 area nearest that cell (FNPC), or the off-map cell
    /// `(0, 0)` without one, which the action does not check. Each frame the
    /// team moves there.
    fn team_action_regroup_at_base(
        &mut self,
        team_id: u64,
        first: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        if first && let Some(leader) = self.team_leader(team_id, rules) {
            let Some(entity) = self.substrate.entities.get(leader) else {
                return;
            };
            let owner = entity.owner();
            let speed_type = self
                .object_type(entity.type_ref(), rules)
                .map(|object| object.speed_type);
            let enemy = crate::sim::house_state::house_state_for_owner_id(&self.houses, owner)
                .and_then(|house| house.enemy_house());
            let cells = self
                .resolved_terrain
                .as_ref()
                .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
            // 6EFAAE/6EFAF7: preserve the shared Map/Dummy query order.
            let [own_x, own_y, _] = self.house_base_coordinate(owner, cells.as_ref());
            let own = [own_x, own_y];
            let enemy = enemy.map(|enemy| {
                let [x, y, _] = self.house_base_coordinate(enemy, cells.as_ref());
                [x, y]
            });
            let seed = regroup_seed_cell(own, enemy, rules.general.ai_safe_distance, || {
                self.scenario_rng.next_range_i32_inclusive(0, 255)
            });
            self.team_assign_gather_cell(team_id, seed, speed_type, rules, registry, frame_effects);
        }
        self.team_coordinate_move(team_id, rules, registry, frame_effects);
    }

    /// Script action 53 (`0x006EF700`), gather at the enemy base: on its
    /// first frame the team's mission target becomes the passable 3x3 area
    /// (FNPC) nearest the cell `AISafeDistance=` cells from the enemy's base
    /// centre towards the team's own (the leader's location without a
    /// base), or the off-map cell `(0, 0)` without one, which the action
    /// does not check. Without members, or when the leader's house has no
    /// enemy with a base, the action finishes at once instead. Each later
    /// frame the team moves there.
    fn team_action_gather_at_enemy_base(
        &mut self,
        team_id: u64,
        first: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        if !first {
            self.team_coordinate_move(team_id, rules, registry, frame_effects);
            return;
        }
        let gather = self.team_leader(team_id, rules).and_then(|leader| {
            let entity = self.substrate.entities.get(leader)?;
            let owner = entity.owner();
            let enemy = crate::sim::house_state::house_state_for_owner_id(&self.houses, owner)?
                .enemy_house()?;
            let cells = self
                .resolved_terrain
                .as_ref()
                .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
            // 6EF7C0 precedes 6EF812; an empty enemy base returns before
            // querying the own base, including its Map/Dummy side effects.
            let enemy = self.house_base_coordinate(enemy, cells.as_ref());
            if enemy == [0, 0, 0] {
                return None;
            }
            let own = self.house_base_coordinate(owner, cells.as_ref());
            let seed = gather_seed_cell(
                [own[0], own[1]],
                [enemy[0], enemy[1]],
                crate::sim::movement::ground_pose::position_world_xy(&entity.position),
                rules.general.ai_safe_distance,
            )?;
            let speed_type = self
                .object_type(entity.type_ref(), rules)
                .map(|object| object.speed_type);
            Some((seed, speed_type))
        });
        let Some((seed, speed_type)) = gather else {
            if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                team.advance_pending = true;
            }
            return;
        };
        self.team_assign_gather_cell(team_id, seed, speed_type, rules, registry, frame_effects);
        self.team_coordinate_move(team_id, rules, registry, frame_effects);
    }

    /// Actions 53 and 54's mission target (`0x006EF947..0x006EF9AB`,
    /// `0x006EFBEE..0x006EFC59`, the same FNPC arguments): the passable 3x3
    /// area nearest `seed` for the leader's `speed_type`, or the cell
    /// `(0, 0)` without one.
    fn team_assign_gather_cell(
        &mut self,
        team_id: u64,
        seed: (i32, i32),
        speed_type: Option<crate::rules::locomotor_type::SpeedType>,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let cell = speed_type
            .and_then(|speed_type| {
                self.nearby_passable_cell(
                    seed,
                    NearbyArgs::plain(
                        PassabilityArgs {
                            speed_type,
                            required_zone_id: None,
                            movement_zone: crate::rules::locomotor_type::MovementZone::Normal,
                            bridge_aware_zone: false,
                        },
                        NearbyFootprint::new(3, 3),
                    ),
                )
            })
            .unwrap_or((0, 0));
        // Gather53/54: Map5657A0 at6EF99F/6EFC4F precedes the setter.
        let target = self.canonical_team_cell((cell.0 as i16, cell.1 as i16));
        self.team_assign_mission_target(team_id, Some(target), rules, registry, frame_effects);
    }

    /// Script action 58 (`0x006EE5C0`), move to own building: on its first
    /// frame the team's mission target becomes a passable cell (FNPC, in the
    /// leader's zone) nearest the top-left cell of the leader's house's
    /// building of BuildingType `argument & 0xFFFF` that
    /// `FindOwnBuilding @ 0x006EEEA0` picks by mode `argument >> 16`
    /// (unsigned); with none, the action finishes at once. While the team
    /// has a mission target it moves there (`Coordinate_Move`), finishing
    /// when it has arrived.
    ///
    /// Not ported: the first frame's move for a team already holding a
    /// mission target (`0x006EE5D9`), unreachable because the step clears it
    /// just before.
    fn team_action_move_to_own_building(
        &mut self,
        team_id: u64,
        argument: i32,
        first: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        if first
            && let Some(leader) = self.team_leader(team_id, rules)
            && let Some(building) = self.team_find_own_building(leader, argument)
            && let Some(cell) = self.team_building_approach_cell(leader, building, rules)
        {
            // TeamMoveToOwnBuilding6EE7B9 resolves the returned CellStruct.
            let target = self.canonical_team_cell((cell.0 as i16, cell.1 as i16));
            self.team_assign_mission_target(team_id, Some(target), rules, registry, frame_effects);
        }
        let Some(team) = self.team_script_vm.teams.get_mut(&team_id) else {
            return;
        };
        if team.mission_target.is_some() {
            self.team_coordinate_move(team_id, rules, registry, frame_effects);
        } else {
            team.advance_pending = true;
        }
    }

    /// `FindOwnBuilding @ 0x006EEEA0` over the leader's house's buildings
    /// (House `+0x68`, in order, with no liveness filter) of BuildingType
    /// index `argument & 0xFFFF`, by mode `argument >> 16`
    /// ([`own_building_pick`]).
    fn team_find_own_building(&self, leader: u64, argument: i32) -> Option<u64> {
        let index = argument & 0xFFFF;
        let mode = (argument as u32) >> 16;
        let leader = self.substrate.entities.get(leader)?;
        let house =
            crate::sim::house_state::house_state_for_owner_id(&self.houses, leader.owner())?;
        let terrain = self.resolved_terrain.as_ref();
        let buildings: Vec<(u64, [i32; 3])> = house
            .base_projection()
            .buildings()
            .iter()
            .filter_map(|&id| {
                let building = self.substrate.entities.get(id)?;
                (building.type_facts.base_plan_type_index() == index)
                    .then(|| (id, location_xyz(building, terrain)))
            })
            .collect();
        own_building_pick(
            &buildings,
            location_xyz(leader, terrain),
            mode,
            |building| self.building_threat(leader.owner(), building),
        )
    }

    /// Action 58's destination (`0x006EE6A5..0x006EE7C8`): FNPC from the
    /// building's top-left cell (Location `/256`, truncating) for the
    /// leader's `SpeedType=` and `MovementZone=`, restricted to the leader's
    /// zone (`GetZoneID @ 0x0056D230` at its cell and bridge), one cell wide.
    fn team_building_approach_cell(
        &self,
        leader: u64,
        building: u64,
        rules: &RuleSet,
    ) -> Option<(u16, u16)> {
        let leader = self.substrate.entities.get(leader)?;
        let object = self.object_type(leader.type_ref(), rules)?;
        let building = self.substrate.entities.get(building)?;
        let location = position_world_coord(&building.position);
        let seed = (
            i32::from((location.x / 256) as i16),
            i32::from((location.y / 256) as i16),
        );
        let leader_location = position_world_coord(&leader.position);
        let leader_cell = (
            (leader_location.x / 256) as u16,
            (leader_location.y / 256) as u16,
        );
        let terrain = self.resolved_terrain.as_ref()?;
        let zone = self.zone_grid.as_ref()?.get_zone_id_native(
            terrain,
            leader_cell,
            object.movement_zone,
            leader.on_bridge(),
        )?;
        self.nearby_passable_cell(
            seed,
            NearbyArgs::plain(
                PassabilityArgs {
                    speed_type: object.speed_type,
                    required_zone_id: Some(zone),
                    movement_zone: object.movement_zone,
                    bridge_aware_zone: false,
                },
                NearbyFootprint::SINGLE,
            ),
        )
        .filter(|&cell| cell != (0, 0))
    }

    /// `Assign_Mission_Target @ 0x006E9050`: a changed mission target
    /// releases the members that aimed or moved at the old one (Guard, then
    /// their destination and target cleared); the move target follows the
    /// mission target unless it had moved off it; a cell target off the
    /// playfield (`0x00578540`) sets leaving-map and clears every member's
    /// destination, one on it clears leaving-map.
    pub(crate) fn team_assign_mission_target(
        &mut self,
        team_id: u64,
        target: Option<TeamTarget>,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let Some(team) = self.team_script_vm.teams.get(&team_id) else {
            return;
        };
        let old = team.mission_target;
        let members: Vec<u64> = team.members().collect();
        if let Some(old) = old
            && !target.is_some_and(|new| {
                NavTargetRef::from(old.target_kind())
                    .same_pointer(new.target_kind().into(), self.resolved_terrain.as_ref())
            })
        {
            for &member in &members {
                let Some(entity) = self.substrate.entities.get(member) else {
                    continue;
                };
                //6E907B/6E9080 compare the retained allocation, not XYZ.
                let aimed = entity.attack_target.target().is_some_and(|attack| {
                    NavTargetRef::from(attack)
                        .same_pointer(old.target_kind().into(), self.resolved_terrain.as_ref())
                });
                let moving = entity.navigation.nav_com().is_some_and(|nav| {
                    nav.same_pointer(old.target_kind().into(), self.resolved_terrain.as_ref())
                });
                if aimed || moving {
                    self.team_member_queue_mission(member, MissionType::Guard, rules);
                    if moving {
                        self.team_member_set_destination(
                            member,
                            None,
                            rules,
                            registry,
                            frame_effects,
                        );
                    }
                    if aimed {
                        self.team_member_clear_target(member, rules);
                    }
                }
            }
        }
        let playfield_bounds = self.playfield_bounds();
        let Some(team) = self.team_script_vm.teams.get_mut(&team_id) else {
            return;
        };
        //6E90D3 reloads the old pointer after the member setters return.
        if team.focus.is_none()
            || team
                .focus
                .zip(team.mission_target)
                .is_some_and(|(focus, old)| {
                    NavTargetRef::from(focus.target_kind())
                        .same_pointer(old.target_kind().into(), self.resolved_terrain.as_ref())
                })
        {
            team.focus = target;
        }
        team.mission_target = target;
        if let Some(cell @ TeamTarget::Cell { x, y }) = target {
            //6E90FD -> Map578540 reads the retained Cell, even when the
            //new/old pointers matched. It does not restamp Map's Dummy.
            let in_playfield = self.resolved_terrain.as_ref().map_or_else(
                || {
                    crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                        (i32::from(x), i32::from(y)),
                        playfield_bounds,
                        None,
                    )
                },
                |terrain| {
                    let cells = NativeCellQuery::canonical(terrain);
                    crate::sim::cell_rect::retained_cell_is_in_playfield(
                        cell.target_kind().cell_identity(terrain).unwrap(),
                        playfield_bounds,
                        &cells,
                    )
                },
            );
            team.leaving_map = !in_playfield;
            if !in_playfield {
                for member in members {
                    self.team_member_set_destination(member, None, rules, registry, frame_effects);
                }
            }
        }
    }

    /// FireAt's FireOnce team branch (`0x006FF905..0x006FF91C`): release
    /// the team's mission target through `Assign_Mission_Target @ 0x006E9050`,
    /// then mark its script action complete (`+0x80`). The cursor advances
    /// at the next ordinary Team AI visit, not during the shot. Membership,
    /// focus, member orders and the pending flag retain their existing owners.
    /// Native execution: `tools/projectile_oracle/fire_once.py`.
    pub(crate) fn team_fire_once_complete(
        &mut self,
        firer: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let Some(team_id) = self.team_script_vm.member_team.get(&firer).copied() else {
            return;
        };
        self.team_assign_mission_target(team_id, None, rules, registry, frame_effects);
        // FireAt rereads Foot+0x5D4 after the member setters return.
        if let Some(team_id) = self.team_script_vm.member_team.get(&firer)
            && let Some(team) = self.team_script_vm.teams.get_mut(team_id)
        {
            team.advance_pending = true;
        }
    }

    /// `TeamClass::Scan_Limit @ 0x006EC3A0`, from a Drive or Ship member's
    /// path tail (`0x004B2EB6`) when it stopped holding a target it cannot
    /// fire at: the team drops its mission target, and each member, in list
    /// order, its target (`vt+0x3C8`), then sets that member's Foot scan-limit
    /// byte (`+0x688`, `0x006EC3BD`). A refused class target assignment does
    /// not suppress the following latch write.
    pub(crate) fn team_scan_limit(
        &mut self,
        team_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        self.team_assign_mission_target(team_id, None, rules, registry, frame_effects);
        let members: Vec<u64> = self
            .team_script_vm
            .teams
            .get(&team_id)
            .map(|team| team.members().collect())
            .unwrap_or_default();
        for member in members {
            self.team_member_clear_target(member, rules);
            if let Some(entity) = self.substrate.entities.get_mut(member) {
                entity.foot_retarget_after_stop.mark_stopped_cannot_fire();
            }
        }
    }

    /// `Coordinate_Move @ 0x006EBAD0`: moves the team to its move target
    /// (falling back to its mission target; nothing without either). Each
    /// live member not yet joined joins or heads for the centre as in
    /// `Regroup`; each live joined member not unloading is near the target
    /// within stray (doubled when flying high), unless it is below ground or
    /// an aircraft aloft off the target's cell while the next action is not
    /// 3: a near member ends its finished Move and idles, a far one is sent
    /// (an `Aggressive=` team's fighting member keeps fighting). The action
    /// finishes when some such member exists and all have arrived with no
    /// destination left, none is unloading, and the team is formed.
    ///
    /// A member whose locomotor still reports moving at its destination
    /// keeps that destination and holds the action. A hovering
    /// `DeployToLand=` Siege Chopper does so natively too: State 2 holds it
    /// with the moving byte set (`0x0054BE09..0x0054BE2F`), and only a
    /// `BalloonHover=` member is let off. Ares bug 895814 reports the stall
    /// in the game.
    ///
    /// `Lagging_Units @ 0x006EBF50` is not consulted: it answers false
    /// unless `+0x7C` is set, which only `0x006EC119` would do, from no
    /// reachable path.
    pub(super) fn team_coordinate_move(
        &mut self,
        team_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let vm = &mut self.team_script_vm;
        let Some(team) = vm.teams.get_mut(&team_id) else {
            return;
        };
        if team.members.is_empty() {
            return;
        }
        if team.focus.is_none() {
            team.focus = team.mission_target;
        }
        let Some(focus) = team.focus else {
            return;
        };
        let members: Vec<u64> = team.members().collect();
        let aggressive = team
            .team_type_id
            .and_then(|id| vm.team_types.get(&id))
            .is_some_and(|definition| definition.aggressive);
        let next_action =
            script_action_at(vm.scripts.get(&team.script_id), team.cursor.wrapping_add(1))
                .map_or(-1, |action| action.action_id);
        let mut finished = true;
        let mut found = false;
        for member in members {
            if !self.team_script_vm.teams.contains_key(&team_id) {
                return;
            }
            if self.team_member_join_up(team_id, member, rules, registry, frame_effects) {
                finished = false;
            }
            let Some(entity) = self.substrate.entities.get(member) else {
                continue;
            };
            let unload = MissionType::Unload as i32;
            let unloading = entity.mission.effective().raw() == unload
                || entity.mission.queued().raw() == unload;
            if unloading {
                finished = false;
            }
            if unloading
                || !member_is_live_joined(
                    entity,
                    self.team_member_initiated(team_id, member),
                    self.object_placement_scope_active(),
                )
            {
                continue;
            }
            let mut stray = self.team_stray(team_id, rules);
            let terrain = self.resolved_terrain.as_ref();
            if crate::sim::movement::air_movement::is_high_flying(
                entity,
                terrain,
                Some((rules, &self.interner)),
            ) {
                stray = stray.wrapping_mul(2);
            }
            found = true;
            let aircraft = entity.category() == EntityCategory::Aircraft;
            let distance = self.team_member_distance(entity, Some(focus));
            let mut near = distance <= stray;
            if near
                && crate::sim::movement::air_movement::current_fly_height(entity, terrain) < 0
                && next_action != 3
            {
                near = false;
            }
            if near && aircraft && position_world_coord(&entity.position).z > 0 {
                //6EBD25..6EBD33: GetCoords+48 -> one Map565730 lookup.
                let same_cell = self.resolved_terrain.as_ref().map_or_else(
                    || {
                        NavTargetRef::cell(entity.position.rx(), entity.position.ry())
                            .same_pointer(focus.target_kind().into(), None)
                    },
                    |terrain| {
                        let cells = NativeCellQuery::canonical(terrain);
                        let coord = object_get_coords(entity, Some(terrain));
                        let current = cells.lookup_world(coord.x, coord.y);
                        focus.target_kind().cell_identity(terrain) == Some(current)
                    },
                );
                if !same_cell && next_action != 3 {
                    near = false;
                }
            }
            if near {
                if entity.mission.effective() == MissionId::from_known(MissionType::Move) {
                    let still_moving = entity.navigation.nav_com().is_some_and(|nav| {
                        self.team_member_distance(entity, Some(nav_target(nav)))
                            > rules.general.close_enough
                            || crate::sim::movement::motion_query::is_moving(entity) == Some(true)
                    });
                    if !still_moving && entity.attack_target.target().is_none() {
                        self.team_member_set_destination(
                            member,
                            None,
                            rules,
                            registry,
                            frame_effects,
                        );
                        crate::sim::world::enter_idle_mode(
                            self,
                            member,
                            rules,
                            None,
                            frame_effects,
                        );
                    }
                }
            } else {
                if aggressive && entity.attack_target.target().is_some() {
                    if !entity.tesla_charger.is_active() {
                        continue;
                    }
                    //6EBDF3..6EBE0D recalls an automatic coil charger: the
                    //class setter clears its target before Move/Commence.
                    self.team_member_clear_target(member, rules);
                }
                if self.substrate.entities.get(member).is_some_and(|entity| {
                    entity.mission.effective() != MissionId::from_known(MissionType::Move)
                }) {
                    self.team_member_queue_mission(member, MissionType::Move, rules);
                    self.team_member_commence(member, rules);
                }
                if self
                    .substrate
                    .entities
                    .get(member)
                    .is_some_and(|entity| entity.navigation.nav_com().is_none())
                {
                    self.team_member_set_destination(
                        member,
                        Some(focus),
                        rules,
                        registry,
                        frame_effects,
                    );
                }
                let Some(entity) = self.substrate.entities.get(member) else {
                    continue;
                };
                let balloon_hover = self
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| object.balloon_hover);
                let resend = entity.navigation.nav_com().is_some_and(|nav| {
                    !nav.same_pointer(focus.target_kind().into(), self.resolved_terrain.as_ref())
                        && (balloon_hover
                            || (aircraft
                                && self.resolved_terrain.as_ref().map_or_else(
                                    || {
                                        nav.same_pointer(
                                            NavTargetRef::cell(
                                                entity.position.rx(),
                                                entity.position.ry(),
                                            ),
                                            None,
                                        )
                                    },
                                    |terrain| {
                                        //6EBE92: Object+1BC performs two lookups.
                                        let cells = NativeCellQuery::canonical(terrain);
                                        let current =
                                            crate::sim::movement::ground_pose::query_object_cell(
                                                &cells,
                                                position_world_coord(&entity.position),
                                            );
                                        crate::sim::combat::TargetKind::from(nav)
                                            .cell_identity(terrain)
                                            == Some(current)
                                    },
                                )))
                }) || (entity.navigation.nav_com().is_none() && balloon_hover);
                if resend {
                    self.team_member_set_destination(
                        member,
                        Some(focus),
                        rules,
                        registry,
                        frame_effects,
                    );
                }
                finished = false;
            }
            let Some(entity) = self.substrate.entities.get(member) else {
                continue;
            };
            let nav = entity.navigation.nav_com();
            //6EBEC1 repeats Object+1BC after the destination setter, even
            //with a NULL NavCom; its ordered Dummy lookups still take effect.
            let aircraft_on_it = aircraft
                && self.resolved_terrain.as_ref().map_or_else(
                    || {
                        nav.is_some_and(|nav| {
                            nav.same_pointer(
                                NavTargetRef::cell(entity.position.rx(), entity.position.ry()),
                                None,
                            )
                        })
                    },
                    |terrain| {
                        let cells = NativeCellQuery::canonical(terrain);
                        let current = crate::sim::movement::ground_pose::query_object_cell(
                            &cells,
                            position_world_coord(&entity.position),
                        );
                        nav.is_some_and(|nav| {
                            crate::sim::combat::TargetKind::from(nav).cell_identity(terrain)
                                == Some(current)
                        })
                    },
                );
            if nav.is_some() {
                let balloon_hover = self
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| object.balloon_hover);
                if !aircraft_on_it && !(balloon_hover && distance < stray) {
                    finished = false;
                }
            }
        }
        if let Some(team) = self.team_script_vm.teams.get_mut(&team_id)
            && found
            && finished
            && team.formed
        {
            team.advance_pending = true;
        }
    }

    /// The leader's `Greatest_Threat` (vt+0x3C4) for a script argument's
    /// quarry, as actions 0 and 57 call it (`0x006ED120..0x006ED15E`,
    /// `0x006F0210..0x006F0253`), around a copy of the leader's Location
    /// with [`Self::team_quarry_mission`].
    pub(super) fn team_quarry_threat(
        &mut self,
        team_id: u64,
        leader: u64,
        quarry: i32,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Option<u64> {
        let mission = self.team_quarry_mission(team_id, quarry);
        crate::sim::world::direct_greatest_threat(self, rules, registry, leader, mission)
    }

    /// What [`Self::team_quarry_threat`] asks for: `Quarry_To_Threat`'s mask
    /// of `quarry` and the team's TeamType's `OnlyTargetHouseEnemy=`
    /// (`+0xF7`).
    pub(super) fn team_quarry_mission(&self, team_id: u64, quarry: i32) -> ScanMission {
        let vm = &self.team_script_vm;
        let only_target_house_enemy = vm
            .teams
            .get(&team_id)
            .and_then(|team| team.team_type_id)
            .and_then(|id| vm.team_type_ini.get(&id))
            .is_some_and(|metadata| metadata.only_target_house_enemy);
        ScanMission::Quarry {
            mask: quarry_mask(quarry),
            only_target_house_enemy,
        }
    }

    /// Script1 / Team6EC9A0, dispatched at6E9605 in the supported retail
    /// gamemd. On the first visit, Scenario68BCE0 resolves the waypoint to
    /// its canonical Cell; Cell47C5A0 may replace it with a blocking object.
    /// Its nearest-object query uses the structural-bridge list (Cell140
    /// bit100), otherwise ground; the existing Aircraft/terrain fast paths
    /// remain ground-list queries.
    /// Later visits retain that target. An absent target or wholly spent
    /// team clears it and finishes; otherwise the existing CoordinateAttack
    /// owner orders the members. No RNG or timer write in this handler.
    /// Source: original6EC9A0..6ECA64 and6E9605..6E961D in Steam gamemd.exe.
    /// Cell47C5A0's Terrain result retains the same Object identity as a
    /// Techno, so PointerExpired6EAE97..6EAEA4 clears it after destruction.
    /// Retained Dummy Cell identity remains a separate TeamTarget residual.
    fn team_action_attack_waypoint(
        &mut self,
        team_id: u64,
        waypoint: i32,
        first: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        if first {
            let target =
                self.team_waypoint_blocker(waypoint)
                    .map(|(target, blocker)| match blocker {
                        Some(BlockingObject::Entity(object) | BlockingObject::Terrain(object)) => {
                            TeamTarget::Object(object)
                        }
                        None => target,
                    });
            self.team_assign_mission_target(team_id, target, rules, registry, frame_effects);
        }
        let Some(team) = self.team_script_vm.teams.get(&team_id) else {
            return;
        };
        if team.mission_target.is_none() || self.team_out_of_ammo(team_id, rules) {
            self.team_assign_mission_target(team_id, None, rules, registry, frame_effects);
            if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                team.advance_pending = true;
            }
            return;
        }
        self.team_coordinate_attack(team_id, rules, registry, frame_effects);
    }

    /// Script15 / Team6ECE60, dispatched at6E9637 (table6E9FB0).
    /// Original gamemd6ECE60..6ECF0C: first assign the waypoint Cell, then
    /// retry its ground-only Cell47C5A0 until an Object appears. Later visits
    /// retain that Object; PointerExpired leaves NULL, which advances without
    /// another waypoint query. No outer ammo gate, RNG, timer or detach here.
    /// The shared CoordinateAttack owns Infiltrate infantry's Capture order.
    fn team_action_spy_waypoint(
        &mut self,
        team_id: u64,
        waypoint: i32,
        first: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        if first {
            let target = self
                .session
                .waypoint_cell(waypoint)
                .map(|cell| self.canonical_team_cell(cell));
            self.team_assign_mission_target(team_id, target, rules, registry, effects);
        }
        let Some(team) = self.team_script_vm.teams.get(&team_id) else {
            return;
        };
        match team.mission_target {
            Some(target @ TeamTarget::Cell { .. }) => {
                let cell = target
                    .target_kind()
                    .tarcom_cell(self.resolved_terrain.as_ref())
                    .expect("a retained Cell has a coordinate");
                let Some(BlockingObject::Entity(object) | BlockingObject::Terrain(object)) =
                    self.find_blocking_object(cell, MovementLayer::Ground)
                else {
                    return;
                };
                self.team_assign_mission_target(
                    team_id,
                    Some(TeamTarget::Object(object)),
                    rules,
                    registry,
                    effects,
                );
            }
            Some(TeamTarget::Object(_)) => {}
            None => {
                self.team_assign_mission_target(team_id, None, rules, registry, effects);
                if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                    team.advance_pending = true;
                }
                return;
            }
        }
        self.team_coordinate_attack(team_id, rules, registry, effects);
    }

    /// Shared Scenario68BCE0 -> Cell47C5A0 selection for Script1 and59.
    /// Structural bridge bit100 selects the native bridge blocker list;
    /// each action owns its distinct admission of the returned object.
    fn team_waypoint_blocker(&self, waypoint: i32) -> Option<(TeamTarget, Option<BlockingObject>)> {
        let cell = self.session.waypoint_cell(waypoint)?;
        let target = self.canonical_team_cell(cell);
        let TeamTarget::Cell { x, y } = target else {
            unreachable!("canonical_team_cell returns a Cell target")
        };
        let layer = self
            .resolved_terrain
            .as_ref()
            .map_or(MovementLayer::Ground, |terrain| {
                let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
                if cells.flags(cells.lookup(cell)) & 0x100 != 0 {
                    MovementLayer::Bridge
                } else {
                    MovementLayer::Ground
                }
            });
        Some((
            target,
            self.find_blocking_object((x as u16, y as u16), layer),
        ))
    }

    /// Script59 / Team6ECA70, distinct from Script1: the first waypoint
    /// query admits only a BuildingRTTI6 returned by shared Cell47C5A0.
    /// Empty ground, terrain and another Foot finish the action. Subsequent
    /// visits retain the target without another waypoint query. Target plus
    /// any unlimited/positive-ammo member invokes the sole CoordinateAttack;
    /// otherwise AssignMissionTarget(NULL) and +80 advance, in that order.
    /// Steam gamemd, original6ECA70..6ECB45 instructions and
    /// team_attack_waypoint_building's bounded control corpus.
    fn team_action_attack_waypoint_building(
        &mut self,
        team_id: u64,
        waypoint: i32,
        first: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        if first {
            let target = self
                .team_waypoint_blocker(waypoint)
                .and_then(|(_, blocker)| {
                    let Some(BlockingObject::Entity(id)) = blocker else {
                        return None;
                    };
                    self.substrate
                        .entities
                        .get(id)
                        .filter(|entity| entity.category() == EntityCategory::Structure)
                        .map(|_| TeamTarget::Object(id))
                });
            self.team_assign_mission_target(team_id, target, rules, registry, effects);
        }
        let Some(team) = self.team_script_vm.teams.get(&team_id) else {
            return;
        };
        if team.mission_target.is_some() && !self.team_out_of_ammo(team_id, rules) {
            self.team_coordinate_attack(team_id, rules, registry, effects);
            return;
        }
        self.team_assign_mission_target(team_id, None, rules, registry, effects);
        if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
            team.advance_pending = true;
        }
    }

    /// Script action 0 (`0x006ED090`, `first` unread), attack quarry: a team
    /// with members and no mission target takes as one its leader's
    /// `Greatest_Threat` (`vt+0x3C4`) for the quarry ([`quarry_mask`]) around
    /// the leader's location, limited to its house's enemy when the TeamType
    /// sets `OnlyTargetHouseEnemy=`. The action finishes when the team still
    /// has no mission target, or when every member carries ammunition
    /// (`Ammo=` above 0) and has none left; either way the team then attacks
    /// ([`Self::team_coordinate_attack`]).
    fn team_action_attack_quarry(
        &mut self,
        team_id: u64,
        quarry: i32,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let Some(team) = self.team_script_vm.teams.get(&team_id) else {
            return;
        };
        if team.mission_target.is_none()
            && let Some(leader) = self.team_leader(team_id, rules)
        {
            let target = self.team_quarry_threat(team_id, leader, quarry, rules, registry);
            self.team_assign_mission_target(
                team_id,
                target.map(TeamTarget::Object),
                rules,
                registry,
                frame_effects,
            );
        }
        let Some(team) = self.team_script_vm.teams.get(&team_id) else {
            return;
        };
        let finished = team.mission_target.is_none() || self.team_out_of_ammo(team_id, rules);
        if finished && let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
            team.advance_pending = true;
        }
        self.team_coordinate_attack(team_id, rules, registry, frame_effects);
    }

    /// Every member carries ammunition (`Ammo=` above 0, type `+0x684`) and
    /// has none left (`+0x2FC`, [`member_ammo`]), as a team without members
    /// does. Actions 0 (`0x006ED17C..0x006ED19A`), 1
    /// (`0x006ECA11..0x006ECA3E`),59 (`0x006ECAF6..0x006ECB1A`) and46
    /// (`0x006EE396..0x006EE3BC`) walk the members from the first and stop
    /// attacking only then.
    fn team_out_of_ammo(&self, team_id: u64, rules: &RuleSet) -> bool {
        self.team_script_vm.teams.get(&team_id).is_none_or(|team| {
            team.members().all(|member| {
                self.substrate.entities.get(member).is_some_and(|entity| {
                    self.object_type(entity.type_ref(), rules)
                        .is_some_and(|object| object.ammo > 0 && member_ammo(entity, object) <= 0)
                })
            })
        })
    }

    /// Script action 46 (`0x006EE310`), attack enemy building: on its first
    /// frame a team with no mission target and a member takes as one the
    /// building [`Self::team_find_target_building`] picks for its first
    /// member (`+0x54`). With a mission target the team attacks it
    /// ([`Self::team_coordinate_attack`]) until it is out of ammunition
    /// ([`Self::team_out_of_ammo`]); then, or without a mission target, it
    /// drops the mission target and the action finishes.
    ///
    /// `Coordinate_Attack` gives an `Engineer=` member Attack. No arm on that
    /// path turns it into Capture: Infantry Mission_Attack (`0x0051F3E0`)
    /// converts only a `C4=` man (to Sabotage, `sim::c4_plant`) and a
    /// computer's `Infiltrate=`, `Occupier=` or `Assaulter=` man (to
    /// Capture), Foot Mission_Attack (`0x004D4DC0`) and its Approach
    /// (`0x004D5690`) have no engineer arm, and an infantryman may enter the
    /// building's cell, as its NavCom or Target, only on Enter, Capture, Eaten or
    /// Sabotage, or an engineer on Guard, AreaGuard or Patrol, never on
    /// Attack (`0x0051C2E1..0x0051C41C`). A computer's engineer captures
    /// only once hit on Guard, as it idles after an unload and after each
    /// retarget (`0x006E9050`): the hit queues Hunt (`0x00518BF2`),
    /// Mission_Hunt's engineer arm sends it in on Capture, and
    /// `Coordinate_Attack` leaves a member on Capture.
    fn team_action_attack_enemy_building(
        &mut self,
        team_id: u64,
        argument: i32,
        first: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let Some(team) = self.team_script_vm.teams.get(&team_id) else {
            return;
        };
        // `0x006EE317..0x006EE383`.
        let unit = team
            .members()
            .next()
            .filter(|_| first && team.mission_target.is_none());
        if let Some(unit) = unit
            && let Some(building) = self.team_find_target_building(team_id, unit, argument)
        {
            self.team_assign_mission_target(
                team_id,
                Some(TeamTarget::Object(building)),
                rules,
                registry,
                frame_effects,
            );
        }
        let Some(team) = self.team_script_vm.teams.get(&team_id) else {
            return;
        };
        if team.mission_target.is_some() && !self.team_out_of_ammo(team_id, rules) {
            self.team_coordinate_attack(team_id, rules, registry, frame_effects);
            return;
        }
        // `0x006EE3BE..0x006EE3C8`.
        self.team_assign_mission_target(team_id, None, rules, registry, frame_effects);
        if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
            team.advance_pending = true;
        }
    }

    /// Script action 47 (`0x006EE3F0`, `first` unread), move to enemy
    /// building: each frame a team with no mission target and a member looks
    /// for the building [`Self::team_find_target_building`] picks for its
    /// first member (`+0x54`); with one, the mission target becomes the
    /// passable cell nearest it in that member's zone, as action 58 finds it
    /// ([`Self::team_building_approach_cell`]), or none. A team with a member
    /// that is still without one finishes the action. Either way the team
    /// moves ([`Self::team_coordinate_move`]).
    fn team_action_move_to_enemy_building(
        &mut self,
        team_id: u64,
        argument: i32,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let Some(team) = self.team_script_vm.teams.get(&team_id) else {
            return;
        };
        let unit = team
            .members()
            .next()
            .filter(|_| team.mission_target.is_none());
        if let Some(unit) = unit {
            // `0x006EE411..0x006EE591`.
            if let Some(building) = self.team_find_target_building(team_id, unit, argument) {
                let target = self
                    .team_building_approach_cell(unit, building, rules)
                    //TeamMoveToBuilding6EE583 selects the Cell pointer.
                    .map(|(x, y)| self.canonical_team_cell((x as i16, y as i16)));
                self.team_assign_mission_target(team_id, target, rules, registry, frame_effects);
            }
            // `0x006EE59A..0x006EE5A3`.
            if let Some(team) = self.team_script_vm.teams.get_mut(&team_id)
                && team.mission_target.is_none()
            {
                team.advance_pending = true;
            }
        }
        self.team_coordinate_move(team_id, rules, registry, frame_effects);
    }

    /// `Find_Best_Target_Building @ 0x006EEBD0` as actions 46, 47 and 56 call it
    /// (`0x006EE32B..0x006EE376`, `0x006EE411..0x006EE45D`, `0x006EFF5D..0x006EFFAF`):
    /// `unit` is the first member for 46/47, the selected leader for 56.
    /// BuildingType index `argument & 0xFFFF`, mode
    /// `(argument >> 16) & 0xFFFF`, the unit's house's enemy (`+0x5600`) and
    /// the TeamType's `OnlyTargetHouseEnemy=` (`+0xF7`), over every building
    /// in construction order (BuildingClass::Array, [`target_building_pick`]).
    pub(super) fn team_find_target_building(
        &self,
        team_id: u64,
        unit: u64,
        argument: i32,
    ) -> Option<u64> {
        let index = argument & 0xFFFF;
        let mode = (argument as u32) >> 16;
        let unit_entity = self.substrate.entities.get(unit)?;
        let owner = unit_entity.owner();
        let enemy = self
            .houses
            .get(&owner)
            .and_then(|house| house.enemy_house());
        let asker = self.interner.resolve(owner);
        let terrain = self.resolved_terrain.as_ref();
        let buildings: Vec<TargetBuilding> = self
            .substrate
            .entities
            .iter_sorted()
            .filter(|(_, building)| {
                building.category() == EntityCategory::Structure
                    && building.type_facts.base_plan_type_index() == index
            })
            .map(|(id, building)| TargetBuilding {
                id,
                location: location_xyz(building, terrain),
                enemy_owned: Some(building.owner()) == enemy,
                allied: crate::map::houses::is_allied_with(
                    self.house_alliances.as_map(),
                    asker,
                    self.interner.resolve(building.owner()),
                ),
                passive: self
                    .houses
                    .get(&building.owner())
                    .is_some_and(|house| house.multiplay_passive()),
            })
            .collect();
        let vm = &self.team_script_vm;
        let only_enemy = vm
            .teams
            .get(&team_id)
            .and_then(|team| team.team_type_id)
            .and_then(|id| vm.team_type_ini.get(&id))
            .is_some_and(|metadata| metadata.only_target_house_enemy);
        target_building_pick(
            &buildings,
            location_xyz(unit_entity, terrain),
            mode,
            only_enemy,
            |building| self.building_threat(owner, building),
        )
    }

    /// The threat `owner`'s house map holds at the cell of `building`'s
    /// GetCoords (vt+0x48, its foundation centre; `0x0056BCD0`), as
    /// FindOwnBuilding and Find_Best_Target_Building score it.
    fn building_threat(&self, owner: InternedId, building: u64) -> i32 {
        let coords = self
            .substrate
            .entities
            .get(building)
            .map(|building| {
                crate::sim::movement::ground_pose::object_get_coords(
                    building,
                    self.resolved_terrain.as_ref(),
                )
            })
            .expect("a scored building is live");
        let cell = crate::util::lepton::lepton_to_cell_packed;
        self.house_threat_at_cell(owner, (cell(coords.x), cell(coords.y)))
            .expect("building scoring requires its native House threat source")
    }

    /// `Coordinate_Attack @ 0x006EB490`: the team attacks its move target,
    /// which falls back to its mission target. A cell target, for a team
    /// whose leader is not an aircraft, gives way to the object blocking the
    /// cell (`Find_Blocking_Object`). On frames where `Frame % 8 == 4` the
    /// leader asks GetFireError of the target with the weapon it would
    /// select; ILLEGAL makes the step restart the action (`+0x81`). Without
    /// a target or members the action finishes.
    ///
    /// Each live member not yet joined joins up as in `Regroup`. Each live
    /// joined member (or aircraft) that is not attacking, entering,
    /// capturing or sabotaging breaks radio contact and takes Attack with
    /// no target and no destination; on action 15 an `Infiltrate=`
    /// infantryman takes Capture at the target instead. A member left
    /// without a target then takes the team's. The action finishes once no
    /// member is still at it: a live joined member other than an aircraft
    /// with a primary weapon and no ammunition, or, for a `Droppod=` team, a
    /// member in limbo.
    fn team_coordinate_attack(
        &mut self,
        team_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let Some(team) = self.team_script_vm.teams.get_mut(&team_id) else {
            return;
        };
        if team.focus.is_none() {
            team.focus = team.mission_target;
        }
        let mut focus = team.focus;
        let has_members = !team.members.is_empty();
        let leader = self.team_leader(team_id, rules);
        // CoordinateAttack6EB51A..6EB593 retains any Cell47C5A0 Object,
        // including Terrain. Shared Object identity lets the existing
        // PointerExpired6EAE97..6EAEA4 clear it. Source: gamemd instructions.
        if let Some(TeamTarget::Cell { x, y }) = focus
            && has_members
            && leader
                .and_then(|leader| self.substrate.entities.get(leader))
                .is_some_and(|entity| entity.category() != EntityCategory::Aircraft)
            && let Some(BlockingObject::Entity(object) | BlockingObject::Terrain(object)) =
                self.find_blocking_object((x as u16, y as u16), MovementLayer::Ground)
            && let Some(team) = self.team_script_vm.teams.get_mut(&team_id)
        {
            focus = Some(TeamTarget::Object(object));
            team.focus = focus;
        }
        // `0x006EB59A..0x006EB5D2`. Without a target the action finishes
        // below, and a restart after the step's advance changes nothing.
        if attack_check_frame(self.session.binary_frame() as i32)
            && let (Some(leader), Some(target)) = (leader, focus)
            && self.selected_weapon_fire_error(rules, leader, target.target_kind(), true, registry)
                == FireError::Illegal
            && let Some(team) = self.team_script_vm.teams.get_mut(&team_id)
        {
            team.retarget = true;
        }
        let vm = &self.team_script_vm;
        let Some(team) = vm.teams.get(&team_id) else {
            return;
        };
        let Some(target) = focus.filter(|_| has_members) else {
            if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                team.advance_pending = true;
            }
            return;
        };
        let action = team.current_action(vm);
        let droppod = team
            .team_type_id
            .and_then(|id| vm.team_type_ini.get(&id))
            .is_some_and(|metadata| metadata.droppod);
        let members: Vec<u64> = team.members().collect();
        let mut busy = false;
        for member in members {
            if !self.team_script_vm.teams.contains_key(&team_id) {
                return;
            }
            self.team_member_join_up(team_id, member, rules, registry, frame_effects);
            let Some(entity) = self.substrate.entities.get(member) else {
                continue;
            };
            if !member_is_live_joined(
                entity,
                self.team_member_initiated(team_id, member),
                self.object_placement_scope_active(),
            ) {
                busy |= droppod && entity.lifecycle.in_limbo();
                continue;
            }
            let infiltrates = || {
                self.object_type(entity.type_ref(), rules)
                    .is_some_and(|object| object.infiltrate)
            };
            let mission = entity.mission.effective().raw();
            if action == 15 && entity.category() == EntityCategory::Infantry && infiltrates() {
                self.team_member_queue_mission(member, MissionType::Capture, rules);
                self.team_member_assign_target(member, target, rules, registry, frame_effects);
            } else if ![
                MissionType::Attack,
                MissionType::Enter,
                MissionType::Capture,
                MissionType::Sabotage,
            ]
            .iter()
            .any(|&kept| mission == kept as i32)
            {
                crate::sim::radio::transmit_to_contact(
                    self,
                    member,
                    crate::sim::radio::RadioMessage::Break,
                    rules,
                    frame_effects,
                );
                self.team_member_queue_mission(member, MissionType::Attack, rules);
                self.team_member_clear_target(member, rules);
                self.team_member_set_destination(member, None, rules, registry, frame_effects);
            }
            let Some(entity) = self.substrate.entities.get(member) else {
                continue;
            };
            if entity.attack_target.target().is_none() {
                self.team_member_assign_target(member, target, rules, registry, frame_effects);
            }
            let Some(entity) = self.substrate.entities.get(member) else {
                continue;
            };
            let spent_aircraft = entity.category() == EntityCategory::Aircraft
                && self
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| {
                        crate::sim::combat::combat_weapon::primary_for_tier(
                            object,
                            entity.veterancy.rank(),
                        )
                        .is_some()
                            && member_ammo(entity, object) <= 0
                    });
            busy |= !spent_aircraft;
        }
        if !busy && let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
            team.advance_pending = true;
        }
    }
}

/// Action 5's guard time (`0x006E97DA..0x006E97E9`): `argument * 15`
/// frames, wrapping.
pub(super) const fn guard_frames(argument: i32) -> i32 {
    argument.wrapping_mul(15)
}

/// Action 54's seed cell (`0x006EFAA3..0x006EFBEF`): the cell
/// `safe_distance` cells from the house's base centre `own` towards the
/// enemy's base centre, or along a facing of `draw() << 8` (the low byte of
/// `RandomRanged(0, 255)`) without an enemy.
pub(super) fn regroup_seed_cell(
    own: [i32; 2],
    enemy: Option<[i32; 2]>,
    safe_distance: i32,
    draw: impl FnOnce() -> i32,
) -> (i32, i32) {
    let facing = match enemy {
        None => (draw() as u16 & 0xFF) << 8,
        Some(enemy) => facing_towards(own, enemy),
    };
    safe_distance_cell(own, facing, safe_distance)
}

/// Action 53's seed cell (`0x006EF7B9..0x006EF93E`): `safe_distance` cells
/// from the enemy's base centre `enemy` towards the house's own `own`, or
/// towards the leader's location `leader` when the house has no base; none
/// when the enemy has no base.
pub(super) fn gather_seed_cell(
    own: [i32; 2],
    enemy: [i32; 2],
    leader: [i32; 2],
    safe_distance: i32,
) -> Option<(i32, i32)> {
    if enemy == [0, 0] {
        return None;
    }
    let own = if own == [0, 0] { leader } else { own };
    Some(safe_distance_cell(
        enemy,
        facing_towards(enemy, own),
        safe_distance,
    ))
}

/// `Coordinate_Attack`'s leader check runs when the signed `Frame % 8` is 4
/// (`0x006EB59A..0x006EB5AE`).
pub(super) const fn attack_check_frame(frame: i32) -> bool {
    frame % 8 == 4
}

/// The facing from `from` to `to` (`0x004CAE30` on the XY delta).
fn facing_towards(from: [i32; 2], to: [i32; 2]) -> u16 {
    crate::util::direction_tables::facing16_from_delta(
        to[0].wrapping_sub(from[0]),
        to[1].wrapping_sub(from[1]),
    )
}

/// Actions 53 and 54's seed (`0x006EF853..0x006EF93E`,
/// `0x006EFAA3..0x006EFBEF`): the point `safe_distance` cells (`<< 8`
/// leptons) from `from` along `facing`, as a cell of its truncated
/// quotients.
pub(super) fn safe_distance_cell(from: [i32; 2], facing: u16, safe_distance: i32) -> (i32, i32) {
    let point =
        crate::util::native_trig::facing_step_world_xy(from, facing, safe_distance.wrapping_shl(8));
    (
        i32::from((point[0] / 256) as i16),
        i32::from((point[1] / 256) as i16),
    )
}

/// A member's shared live ammunition (`+0x2FC`). A component-only fixture
/// without a concrete constructor retains its explicitly declared type prior.
fn member_ammo(entity: &GameEntity, object: &crate::rules::object_type::ObjectType) -> i32 {
    entity.ammo().map_or(object.ammo, |ammo| ammo.current())
}

/// An object's Location (`+0x9C`) as the building finders read it.
fn location_xyz(
    entity: &GameEntity,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> [i32; 3] {
    let location = crate::sim::movement::ground_pose::object_location(entity, terrain);
    [location.x, location.y, location.z]
}

/// The score `FindOwnBuilding` (`0x006EEF02..0x006EF0C1`) and
/// `Find_Best_Target_Building` (`0x006EEC6D..0x006EEE1F`) give a building by
/// `mode`: 0 `0x7FFFFFFF - threat`, 1 `threat`, the threat the scorer's
/// house holds at the building's GetCoords cell (each axis rounded toward
/// zero, `0x006EEC91..0x006EECB9`, `0x0056BCD0`); 2 `0x7FFFFFFF - distance`,
/// 3 `distance`, between the two
/// Locations (`Sqrt_Approx`, ftol); any other mode -1, which no building
/// beats. Distance modes never read the threat.
fn building_pick_score(
    mode: u32,
    threat: impl FnOnce() -> i32,
    location: [i32; 3],
    from: [i32; 3],
) -> i32 {
    let distance = || crate::util::native_x87::distance_3d_leptons(location, from);
    match mode {
        0 => i32::MAX.wrapping_sub(threat()),
        1 => threat(),
        2 => i32::MAX.wrapping_sub(distance()),
        3 => distance(),
        _ => -1,
    }
}

/// `FindOwnBuilding @ 0x006EEEA0`'s pick among `buildings` (id, Location)
/// from `from` (the leader's Location): the highest
/// [`building_pick_score`] (signed, the first on a tie, none at or below
/// -1). `threat_at` answers a building's threat.
pub(super) fn own_building_pick(
    buildings: &[(u64, [i32; 3])],
    from: [i32; 3],
    mode: u32,
    mut threat_at: impl FnMut(u64) -> i32,
) -> Option<u64> {
    let mut best = None;
    let mut best_score = -1i32;
    for &(id, location) in buildings {
        let score = building_pick_score(mode, || threat_at(id), location, from);
        if score > best_score {
            best_score = score;
            best = Some(id);
        }
    }
    best
}

/// A building of the searched type on `Find_Best_Target_Building`'s walk.
pub(super) struct TargetBuilding {
    pub(super) id: u64,
    /// Location (`+0x9C`).
    pub(super) location: [i32; 3],
    /// Its owner is the enemy house the caller passes.
    pub(super) enemy_owned: bool,
    /// The unit's house is allied with its owner (`IsAlliedWith @
    /// 0x004F9A50`, true for the house itself).
    pub(super) allied: bool,
    /// Its owner's type is `MultiplayPassive=` (HouseType `+0x1A6`).
    pub(super) passive: bool,
}

/// `Find_Best_Target_Building @ 0x006EEBD0`'s walk (`0x006EEC0D..0x006EEE55`)
/// over the buildings of the searched type in BuildingClass::Array order:
/// one of the enemy, of a house the unit's is not allied with, or of an
/// allied passive house is scored ([`building_pick_score`] from `from`, the
/// unit's Location). The highest of the enemy's buildings and of all
/// (signed, the first on a tie, none at or below -1) are kept, and
/// `only_enemy` (`OnlyTargetHouseEnemy=`) picks the enemy's
/// (`0x006EEE5B..0x006EEE7A`). The original also returns the enemy's when it
/// beats the overall best (`0x006EEE6D`), which it never can.
pub(super) fn target_building_pick(
    buildings: &[TargetBuilding],
    from: [i32; 3],
    mode: u32,
    only_enemy: bool,
    mut threat_at: impl FnMut(u64) -> i32,
) -> Option<u64> {
    let (mut enemy_best, mut enemy_score) = (None, -1i32);
    let (mut any_best, mut any_score) = (None, -1i32);
    for building in buildings {
        if !building.enemy_owned && building.allied && !building.passive {
            continue;
        }
        let score = building_pick_score(mode, || threat_at(building.id), building.location, from);
        if score > enemy_score && building.enemy_owned {
            enemy_score = score;
            enemy_best = Some(building.id);
        }
        if score > any_score {
            any_score = score;
            any_best = Some(building.id);
        }
    }
    if only_enemy { enemy_best } else { any_best }
}

#[cfg(test)]
#[path = "force_facing_tests.rs"]
mod force_facing_tests;
#[cfg(test)]
#[path = "removal_actions_tests.rs"]
mod removal_tests;
#[cfg(test)]
#[path = "actions_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "transport_actions_tests.rs"]
mod transport_tests;

#[cfg(test)]
#[path = "unpanic_tests.rs"]
mod unpanic_tests;
