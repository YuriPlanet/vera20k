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
//! (with its draw) and `own_building` rows replay in `recruit_oracle_tests`.
//!
//! RESIDUALS:
//! - Actions other than 0, 2, 5, 6, 11, 19, 24, 49, 53, 54, 55, 57 and 58
//!   are not ported. A team that reaches one stays on it, as a native
//!   handler that has not finished does, and records the first as its
//!   refusal; the rest of its update runs. Trigger: 74 of the 163 retail
//!   AIMD TeamTypes reach one; the first they reach is 63 (17 TeamTypes), 14
//!   (16), 46 (15), 47 (12), 61 (9), 9 (3), 21 or 62 (1 each). Effect: the
//!   team idles on that action for good, keeping its members and its
//!   TeamType's `Max=` slot, and nothing else sends those units out. Action
//!   56 (`0x006EFE60`), the Chronosphere's other script action, takes
//!   `(mode << 16) | BuildingType index` and picks its target with
//!   `Find_Best_Target_Building @ 0x006EEBD0`, which actions 46 and 47 need
//!   too; no AIMD script names it, but four scripts of the retail campaign
//!   map SOV02SMD.MAP do, whose teams idle on it.
//! - `Coordinate_Attack` keeps a cell target that only a terrain object
//!   blocks: a team target holds no terrain object. Trigger: a cell target,
//!   which no ported caller gives (`Greatest_Threat`'s wall fallback is not
//!   ported). Effect: the members fire at the cell instead of the tree.
//! - `Coordinate_Attack` orders an unloading unit to attack without asking
//!   Unit vt+0x4E4 (`0x00736D50`), which keeps it unloading when its type is
//!   `DeployToFire=` (no retail type) or a computer unit's `DeploysInto=`
//!   building sets `+0x16C4` (identity unchecked; Infantry's `0x0041C060`
//!   answers false). Trigger: a member deploying when its team attacks.
//!   Effect: it stops deploying and attacks.
//! - A non-aircraft member's ammunition (`+0x2FC`) is not tracked and reads
//!   as its `Ammo=`: action 0 never finds such a team spent. Trigger: a
//!   team of units with `Ammo=` above 0. Effect: the team keeps attacking
//!   where native would move on once all are empty.

//! - `Coordinate_Move`'s far branch reads Foot `+0x68E` (a target the
//!   tank-bunker scans acquired), which no ported code writes: it reads
//!   clear, so an `Aggressive=` team's fighting member keeps fighting.

use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::ScanMission;
use crate::sim::combat::fire_error::FireError;
use crate::sim::components::NavTargetRef;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::BlockingObject;
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::world::Simulation;

use super::team_ai::member_is_live_joined;
use super::{TeamScriptAction, TeamScriptRefusal, TeamTarget, script_action_at};

/// Whether the dispatch ports `action_id`'s handler.
pub(super) const fn action_is_ported(action_id: i32) -> bool {
    matches!(
        action_id,
        0 | 2 | 5 | 6 | 11 | 19 | 24 | 49 | 53 | 54 | 55 | 57 | 58
    )
}

/// A member's NavCom (`+0x5A4`) names `target`.
fn nav_names(nav: NavTargetRef, target: TeamTarget) -> bool {
    match (nav, target) {
        (NavTargetRef::Cell { rx, ry }, TeamTarget::Cell { x, y }) => {
            rx == x as u16 && ry == y as u16
        }
        (
            NavTargetRef::Entity { id }
            | NavTargetRef::Object { id }
            | NavTargetRef::Building { id },
            TeamTarget::Object(object),
        ) => id == object,
        _ => false,
    }
}

/// A member's NavCom as a team target, for `Distance`.
fn nav_target(nav: NavTargetRef) -> TeamTarget {
    match nav {
        NavTargetRef::Cell { rx, ry } => TeamTarget::Cell {
            x: rx as i16,
            y: ry as i16,
        },
        NavTargetRef::Entity { id }
        | NavTargetRef::Object { id }
        | NavTargetRef::Building { id } => TeamTarget::Object(id),
    }
}

/// A member's TarCom (`+0x2B4`) names `target`.
fn tarcom_names(entity: &GameEntity, target: TeamTarget) -> bool {
    entity
        .attack_target
        .as_ref()
        .is_some_and(|attack| attack.target == target.target_kind())
}

/// The cell holding a member's Location (`vt+0x1BC`), as a team target.
fn member_cell(entity: &GameEntity) -> TeamTarget {
    TeamTarget::Cell {
        x: entity.position.rx as i16,
        y: entity.position.ry as i16,
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
                    self.team_member_clear_archive_target(member);
                }
            }
            if (cursor as u32) >= count {
                self.destroy_team(team_id, rules);
                return;
            }
            self.team_assign_mission_target(team_id, None, rules, registry);
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
            self.team_assign_mission_target(team_id, None, rules, registry);
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
        self.team_run_action(team_id, action, first, rules, registry);
    }

    /// The jump table's cases.
    fn team_run_action(
        &mut self,
        team_id: u64,
        action: TeamScriptAction,
        first: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let current_frame = self.session.binary_frame as i32;
        match action.action_id {
            0 => self.team_action_attack_quarry(team_id, action.argument, rules, registry),
            // `0x006E95AB`: returns; the team stays on the action.
            2 => {}
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
                self.team_regroup(team_id, rules, registry);
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
            11 => self.team_action_do_mission(team_id, action.argument, rules, registry),
            // `0x006E984B`: each member panics (`vt+0x518`, in list order).
            19 => {
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
                        crate::sim::infantry::apply_panic_force(object, entity);
                    }
                }
                if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                    team.advance_pending = true;
                }
            }
            // `0x006E94BA`, and `0x006E9CE1` which also records success
            // (`+0x84`), counted when the team is destroyed.
            24 | 49 => {
                if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
                    team.advance_pending = true;
                    team.succeeded |= action.action_id == 49;
                }
            }
            53 => self.team_action_gather_at_enemy_base(team_id, first, rules, registry),
            54 => self.team_action_regroup_at_base(team_id, first, rules, registry),
            55 => self.team_action_iron_curtain(team_id, rules, registry),
            57 => self.team_action_chronoshift(team_id, action.argument, rules, registry),
            58 => self.team_action_move_to_own_building(
                team_id,
                action.argument,
                first,
                rules,
                registry,
            ),
            action_id => {
                debug_assert!(!action_is_ported(action_id));
                if let Some(team) = self.team_script_vm.teams.get_mut(&team_id)
                    && team.refusal.is_none()
                {
                    team.refusal = Some(TeamScriptRefusal::UnsupportedAction { action_id });
                    log::warn!(
                        "TeamClass::AI: team {team_id} stays on unported script action {action_id}"
                    );
                }
            }
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
            if !entity.lifecycle.object_alive {
                continue;
            }
            self.team_member_join_up(team_id, member, rules, registry);
            let Some(entity) = self.substrate.entities.get(member) else {
                continue;
            };
            if !member_is_live_joined(entity, self.team_member_initiated(team_id, member))
                || entity.attack_target.is_some()
            {
                continue;
            }
            if entity.navigation.nav_com.is_none() {
                let stray = self.team_stray(team_id, rules);
                if self.team_member_distance(entity, zone) > stray.wrapping_mul(2)
                    && mission != MissionType::AreaGuard as i32
                {
                    self.team_member_queue_mission(member, MissionType::Move, rules);
                    self.team_member_set_destination(member, zone, rules, registry);
                    self.team_member_queue_mission(member, MissionType::Move, rules);
                    let cell = zone.and_then(|zone| self.team_target_cell_of_coord(zone));
                    self.team_member_set_destination(member, cell, rules, registry);
                    continue;
                }
            }
            let Some(entity) = self.substrate.entities.get(member) else {
                continue;
            };
            if entity.attack_target.is_some() || entity.navigation.nav_com.is_some() {
                continue;
            }
            let current = entity.mission.effective().raw();
            if current == mission
                || (mission == MissionType::Guard as i32 && current == MissionType::Unload as i32)
            {
                continue;
            }
            self.team_member_clear_archive_target(member);
            self.team_member_queue_mission_id(member, MissionId::from_raw(mission), rules);
            self.team_member_clear_target(member, rules);
            self.team_member_set_destination(member, None, rules, registry);
        }
    }

    /// The member with the highest `LeadershipRating=` (signed, the first in
    /// list order on a tie) among the live joined members, else the head
    /// (`0x006EE621..0x006EE683`, `0x006EFA2B..0x006EFA81`).
    pub(super) fn team_leader(&self, team_id: u64, rules: &RuleSet) -> Option<u64> {
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
            if member_is_live_joined(entity, member.initiated) && rating > best {
                leader = member.id;
                best = rating;
            }
        }
        Some(leader)
    }

    /// `HouseClass::Base_Center @ 0x0050DF30`: the centre of the house's base
    /// cell (`HouseState::base_origin`), or `(0, 0)` without one.
    fn house_base_center_xy(&self, owner: InternedId) -> [i32; 2] {
        let origin = crate::sim::house_state::house_state_for_owner_id(&self.houses, owner)
            .map_or((0, 0), |house| house.base_origin());
        if origin == (0, 0) {
            [0, 0]
        } else {
            [
                i32::from(origin.0) * 256 + 128,
                i32::from(origin.1) * 256 + 128,
            ]
        }
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
                .and_then(|house| house.enemy_house);
            let own = self.house_base_center_xy(owner);
            let enemy = enemy.map(|enemy| self.house_base_center_xy(enemy));
            let seed = regroup_seed_cell(own, enemy, rules.general.ai_safe_distance, || {
                self.scenario_rng.next_range_i32_inclusive(0, 255)
            });
            self.team_assign_gather_cell(team_id, seed, speed_type, rules, registry);
        }
        self.team_coordinate_move(team_id, rules, registry);
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
    ) {
        if !first {
            self.team_coordinate_move(team_id, rules, registry);
            return;
        }
        let gather = self.team_leader(team_id, rules).and_then(|leader| {
            let entity = self.substrate.entities.get(leader)?;
            let owner = entity.owner();
            let enemy = crate::sim::house_state::house_state_for_owner_id(&self.houses, owner)?
                .enemy_house?;
            let seed = gather_seed_cell(
                self.house_base_center_xy(owner),
                self.house_base_center_xy(enemy),
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
        self.team_assign_gather_cell(team_id, seed, speed_type, rules, registry);
        self.team_coordinate_move(team_id, rules, registry);
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
    ) {
        let cell = speed_type
            .and_then(|speed_type| {
                self.find_plain_passable_cell(
                    seed,
                    speed_type,
                    None,
                    crate::rules::locomotor_type::MovementZone::Normal,
                    (3, 3),
                )
            })
            .unwrap_or((0, 0));
        let target = TeamTarget::Cell {
            x: cell.0 as i16,
            y: cell.1 as i16,
        };
        self.team_assign_mission_target(team_id, Some(target), rules, registry);
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
    ) {
        if first
            && let Some(leader) = self.team_leader(team_id, rules)
            && let Some(building) = self.team_find_own_building(leader, argument)
            && let Some(cell) = self.team_building_approach_cell(leader, building, rules)
        {
            let target = TeamTarget::Cell {
                x: cell.0 as i16,
                y: cell.1 as i16,
            };
            self.team_assign_mission_target(team_id, Some(target), rules, registry);
        }
        let Some(team) = self.team_script_vm.teams.get_mut(&team_id) else {
            return;
        };
        if team.mission_target.is_some() {
            self.team_coordinate_move(team_id, rules, registry);
        } else {
            team.advance_pending = true;
        }
    }

    /// `FindOwnBuilding @ 0x006EEEA0` over the leader's house's buildings
    /// (House `+0x68`, in order, with no liveness filter): of BuildingType
    /// index `argument & 0xFFFF`, the highest score (signed, the first on a
    /// tie; none at or below -1): mode 0 `0x7FFFFFFF - threat`, 1 `threat`
    /// from the retained House map, 2 `0x7FFFFFFF - distance`, 3
    /// `distance`, the distance between the building's and the leader's
    /// Locations (`0x0041C380`); any other mode picks none.
    fn team_find_own_building(&self, leader: u64, argument: i32) -> Option<u64> {
        let index = argument & 0xFFFF;
        let mode = (argument as u32) >> 16;
        let leader = self.substrate.entities.get(leader)?;
        let house =
            crate::sim::house_state::house_state_for_owner_id(&self.houses, leader.owner())?;
        let from = position_world_coord(&leader.position);
        let buildings: Vec<(u64, [i32; 3])> = house
            .base_projection
            .buildings()
            .iter()
            .filter_map(|&id| {
                let building = self.substrate.entities.get(id)?;
                (building.base_plan_type_index == index).then(|| {
                    let location = position_world_coord(&building.position);
                    (id, [location.x, location.y, location.z])
                })
            })
            .collect();
        own_building_pick(&buildings, [from.x, from.y, from.z], mode, |location| {
            self.house_threat_at_cell(
                leader.owner(),
                ((location[0] / 256) as i16, (location[1] / 256) as i16),
            )
            .expect("FindOwnBuilding requires its native House threat source")
        })
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
            leader.on_bridge,
        )?;
        self.find_plain_passable_cell(
            seed,
            object.speed_type,
            Some(zone),
            object.movement_zone,
            (1, 1),
        )
        .filter(|&cell| cell != (0, 0))
    }

    /// `MapClass::Find_Nearby_Passable_Cell @ 0x0056DC20` as both team
    /// actions and `HouseClass::AI_GroundRallyPoint` (`0x00509D61..
    /// 0x00509D9A`) call it: no bridge-aware zone, overlay, height, obstacle
    /// or occupancy test, bridges allowed, no target cell (the frame-counter
    /// pick), no quadrant skip.
    pub(crate) fn find_plain_passable_cell(
        &self,
        seed: (i32, i32),
        speed_type: crate::rules::locomotor_type::SpeedType,
        zone: Option<u32>,
        movement_zone: crate::rules::locomotor_type::MovementZone,
        footprint: (i32, i32),
    ) -> Option<(u16, u16)> {
        use crate::sim::find_nearby_cell::{
            NearbyAnchorGate, NearbyFootprint, NearbyQuery, NearbySearchOptions, PassabilityArgs,
            find_nearby_passable_cell_with_options, map_owned_radius_cap,
        };
        let terrain = self.resolved_terrain.as_ref()?;
        let size = self
            .playfield_bounds
            .zip(self.playfield_size_height)
            .map(|(bounds, height)| (bounds.base, height))
            .or_else(|| self.bridge_state.as_ref()?.native_zone_source_size())?;
        let grid = self.path_grid_snapshot();
        find_nearby_passable_cell_with_options(
            seed,
            &NearbyQuery {
                native_cells: None,
                raw_occupation: Some(&self.substrate.raw_cell_occupation),
                passability: PassabilityArgs {
                    speed_type,
                    required_zone_id: zone,
                    movement_zone,
                    bridge_aware_zone: false,
                },
                footprint: NearbyFootprint::new(footprint.0, footprint.1),
                anchor_gate: NearbyAnchorGate::NativeHeightAware,
                allow_bridge_cells: true,
                check_height: false,
                check_occupancy: false,
                radius_cap: map_owned_radius_cap(size.0, size.1),
                target_cell: None,
                path_grid: grid.as_deref(),
                resolved_terrain: Some(terrain),
                overlay_grid: self.overlay_grid.as_ref(),
                occupancy: Some(&self.substrate.occupancy),
                entities: Some(&self.substrate.entities),
                zone_grid: self.zone_grid.as_ref(),
                playfield_bounds: self.playfield_bounds,
            },
            NearbySearchOptions {
                reject_any_overlay: false,
            },
            self.session.binary_frame,
        )
    }

    /// `Assign_Mission_Target @ 0x006E9050`: a changed mission target
    /// releases the members that aimed or moved at the old one (Guard, then
    /// their destination and target cleared); the move target follows the
    /// mission target unless it had moved off it; a cell target off the
    /// playfield (`0x00578540`) sets leaving-map and clears every member's
    /// destination, one on it clears leaving-map.
    pub(super) fn team_assign_mission_target(
        &mut self,
        team_id: u64,
        target: Option<TeamTarget>,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let Some(team) = self.team_script_vm.teams.get(&team_id) else {
            return;
        };
        let old = team.mission_target;
        let members: Vec<u64> = team.members().collect();
        if let Some(old) = old
            && target != Some(old)
        {
            for &member in &members {
                let Some(entity) = self.substrate.entities.get(member) else {
                    continue;
                };
                let aimed = tarcom_names(entity, old);
                let moving = entity
                    .navigation
                    .nav_com
                    .is_some_and(|nav| nav_names(nav, old));
                if aimed || moving {
                    self.team_member_queue_mission(member, MissionType::Guard, rules);
                    if moving {
                        self.team_member_set_destination(member, None, rules, registry);
                    }
                    if aimed {
                        self.team_member_clear_target(member, rules);
                    }
                }
            }
        }
        let Some(team) = self.team_script_vm.teams.get_mut(&team_id) else {
            return;
        };
        if team.focus == old || team.focus.is_none() {
            team.focus = target;
        }
        team.mission_target = target;
        if let Some(TeamTarget::Cell { x, y }) = target {
            let in_playfield = crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                (i32::from(x), i32::from(y)),
                self.playfield_bounds,
                self.resolved_terrain.as_ref(),
            );
            team.leaving_map = !in_playfield;
            if !in_playfield {
                for member in members {
                    self.team_member_set_destination(member, None, rules, registry);
                }
            }
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
    ) {
        self.team_assign_mission_target(team_id, None, rules, registry);
        let members: Vec<u64> = self
            .team_script_vm
            .teams
            .get(&team_id)
            .map(|team| team.members().collect())
            .unwrap_or_default();
        for member in members {
            self.team_member_clear_target(member, rules);
            if let Some(entity) = self.substrate.entities.get_mut(member) {
                entity.mark_stopped_cannot_fire();
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
    /// `Lagging_Units @ 0x006EBF50` is not consulted: it answers false
    /// unless `+0x7C` is set, which only `0x006EC119` would do, from no
    /// reachable path.
    pub(super) fn team_coordinate_move(
        &mut self,
        team_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
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
            if self.team_member_join_up(team_id, member, rules, registry) {
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
                || !member_is_live_joined(entity, self.team_member_initiated(team_id, member))
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
            let aircraft = entity.category == EntityCategory::Aircraft;
            let distance = self.team_member_distance(entity, Some(focus));
            let mut near = distance <= stray;
            if near
                && crate::sim::movement::air_movement::current_fly_height(entity, terrain) < 0
                && next_action != 3
            {
                near = false;
            }
            if near
                && aircraft
                && position_world_coord(&entity.position).z > 0
                && member_cell(entity) != focus
                && next_action != 3
            {
                near = false;
            }
            if near {
                if entity.mission.effective() == MissionId::from_known(MissionType::Move) {
                    let still_moving = entity.navigation.nav_com.is_some_and(|nav| {
                        self.team_member_distance(entity, Some(nav_target(nav)))
                            > rules.general.close_enough
                            || crate::sim::movement::motion_query::is_moving(entity) == Some(true)
                    });
                    if !still_moving && entity.attack_target.is_none() {
                        self.team_member_set_destination(member, None, rules, registry);
                        self.team_member_enter_idle_mode(member, rules);
                    }
                }
            } else {
                if aggressive && entity.attack_target.is_some() {
                    continue;
                }
                if entity.mission.effective() != MissionId::from_known(MissionType::Move) {
                    self.team_member_queue_mission(member, MissionType::Move, rules);
                    self.team_member_commence(member, rules);
                }
                if self
                    .substrate
                    .entities
                    .get(member)
                    .is_some_and(|entity| entity.navigation.nav_com.is_none())
                {
                    self.team_member_set_destination(member, Some(focus), rules, registry);
                }
                let Some(entity) = self.substrate.entities.get(member) else {
                    continue;
                };
                let balloon_hover = self
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| object.balloon_hover);
                let resend = entity.navigation.nav_com.is_some_and(|nav| {
                    !nav_names(nav, focus)
                        && (balloon_hover || (aircraft && nav_names(nav, member_cell(entity))))
                }) || (entity.navigation.nav_com.is_none() && balloon_hover);
                if resend {
                    self.team_member_set_destination(member, Some(focus), rules, registry);
                }
                finished = false;
            }
            let Some(entity) = self.substrate.entities.get(member) else {
                continue;
            };
            if let Some(nav) = entity.navigation.nav_com {
                let balloon_hover = self
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| object.balloon_hover);
                let aircraft_on_it = aircraft && nav_names(nav, member_cell(entity));
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
        crate::sim::world::team_leader_greatest_threat(self, rules, registry, leader, mission)
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
        ScanMission::TeamQuarry {
            mask: quarry_mask(quarry),
            only_target_house_enemy,
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
            );
        }
        let Some(team) = self.team_script_vm.teams.get(&team_id) else {
            return;
        };
        let finished = team.mission_target.is_none()
            || team.members().all(|member| {
                self.substrate.entities.get(member).is_some_and(|entity| {
                    self.object_type(entity.type_ref(), rules)
                        .is_some_and(|object| object.ammo > 0 && member_ammo(entity, object) <= 0)
                })
            });
        if finished && let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
            team.advance_pending = true;
        }
        self.team_coordinate_attack(team_id, rules, registry);
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
        // `0x006EB51A..0x006EB593`; a terrain object is not a team target
        // (module residual).
        if let Some(TeamTarget::Cell { x, y }) = focus
            && has_members
            && leader
                .and_then(|leader| self.substrate.entities.get(leader))
                .is_some_and(|entity| entity.category != EntityCategory::Aircraft)
            && let Some(BlockingObject::Entity(object)) =
                self.find_blocking_object((x as u16, y as u16))
            && let Some(team) = self.team_script_vm.teams.get_mut(&team_id)
        {
            focus = Some(TeamTarget::Object(object));
            team.focus = focus;
        }
        // `0x006EB59A..0x006EB5D2`. Without a target the action finishes
        // below, and a restart after the step's advance changes nothing.
        if attack_check_frame(self.session.binary_frame as i32)
            && let (Some(leader), Some(target)) = (leader, focus)
            && self.selected_weapon_fire_error(rules, leader, target.target_kind(), registry)
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
            self.team_member_join_up(team_id, member, rules, registry);
            let Some(entity) = self.substrate.entities.get(member) else {
                continue;
            };
            if !member_is_live_joined(entity, self.team_member_initiated(team_id, member)) {
                busy |= droppod && entity.lifecycle.in_limbo;
                continue;
            }
            let infiltrates = || {
                self.object_type(entity.type_ref(), rules)
                    .is_some_and(|object| object.infiltrate)
            };
            let mission = entity.mission.effective().raw();
            if action == 15 && entity.category == EntityCategory::Infantry && infiltrates() {
                self.team_member_queue_mission(member, MissionType::Capture, rules);
                self.team_member_assign_target(member, target, rules);
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
                    Some(rules),
                );
                self.team_member_queue_mission(member, MissionType::Attack, rules);
                self.team_member_clear_target(member, rules);
                self.team_member_set_destination(member, None, rules, registry);
            }
            let Some(entity) = self.substrate.entities.get(member) else {
                continue;
            };
            if entity.attack_target.is_none() {
                self.team_member_assign_target(member, target, rules);
            }
            let Some(entity) = self.substrate.entities.get(member) else {
                continue;
            };
            let spent_aircraft = entity.category == EntityCategory::Aircraft
                && self
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| {
                        crate::sim::combat::combat_weapon::primary_for_tier(
                            object,
                            entity.veterancy(),
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

/// `Quarry_To_Threat @ 0x00645BB0` (table `0x00645BF8`): script action 0's
/// quarry as a `Greatest_Threat` mask; 0 outside `2..=11`.
pub(super) const fn quarry_mask(quarry: i32) -> u32 {
    match quarry {
        2 => 0x20,
        3 => 0x40,
        4 => 0x8,
        5 => 0x10,
        6 => 0x1000,
        7 => 0x2000,
        9 => 0x800,
        10 => 0x8000,
        11 => 0x10000,
        _ => 0,
    }
}

/// A member's ammunition (`+0x2FC`): an aircraft's count; other classes'
/// is not tracked and reads as full (`Ammo=`, module residual).
fn member_ammo(entity: &GameEntity, object: &crate::rules::object_type::ObjectType) -> i32 {
    entity
        .aircraft_ammo
        .as_ref()
        .map_or(object.ammo, |ammo| ammo.current)
}

/// `FindOwnBuilding @ 0x006EEEA0`'s pick among `buildings` (id, location)
/// by `mode`: 0 lowest threat, 1 highest, 2 nearest `from` (the leader's
/// location), 3 farthest, by the approximated 3D distance; the first of
/// equal scores, none for another mode. Modes0/1 read the shared retained
/// House grid through native56BCD0; distance-only modes never query threat.
pub(super) fn own_building_pick(
    buildings: &[(u64, [i32; 3])],
    from: [i32; 3],
    mode: u32,
    mut threat_at: impl FnMut([i32; 3]) -> i32,
) -> Option<u64> {
    let mut best = None;
    let mut best_score = -1i32;
    for &(id, location) in buildings {
        let distance = || crate::util::native_x87::distance_3d_leptons(location, from);
        let score = match mode {
            0 => i32::MAX.wrapping_sub(threat_at(location)),
            1 => threat_at(location),
            2 => i32::MAX.wrapping_sub(distance()),
            3 => distance(),
            _ => -1,
        };
        if score > best_score {
            best_score = score;
            best = Some(id);
        }
    }
    best
}
