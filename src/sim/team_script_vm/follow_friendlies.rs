//! Script10 / Team6EDB50 and the action-10 arm of Calc_Center6EAEE0.
//!
//! Adapted from the author's earlier ra2-mono follow-friendlies work; supplied
//! under this repository's GPLv3. See docs/script-action-10.md for the source
//! mapping and deliberate differences from the C# implementation.

use super::TeamTarget;
use super::team_ai::{member_is_live, member_is_live_joined};
use crate::map::entities::EntityCategory;
use crate::map::houses::is_allied_with;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::combat_weapon::is_armed;
use crate::sim::components::NavTargetRef;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::motion_query::is_moving;
use crate::sim::world::{FrameEffects, Simulation};
use crate::util::native_x87::distance_3d_leptons;

impl Simulation {
    /// Calc_Center6EAF1C..6EB1B2: nearest live allied Unit, then Infantry,
    /// outside this team, measured from the list head's full GetCoords.
    /// Strictly smaller whole-lepton distances replace the winner. VERA's
    /// stable IDs stand in for order within each native class array.
    pub(super) fn team_follow_friendly_center(
        &self,
        team_id: u64,
    ) -> (Option<TeamTarget>, Option<u64>) {
        let Some(team) = self.team_script_vm.teams.get(&team_id) else {
            return (None, None);
        };
        let Some(head) = team.members().next() else {
            return (None, None);
        };
        let Some(from) = self.team_target_coord(TeamTarget::Object(head)) else {
            return (None, None);
        };
        let owner = self.interner.resolve(team.owner);
        let mut best = None;
        let mut best_distance = -1;
        for category in [EntityCategory::Unit, EntityCategory::Infantry] {
            for candidate in self.substrate.entities.values_of_category(category) {
                if !member_is_live(candidate, self.object_placement_scope_active())
                    || self.team_script_vm.member_team.get(&candidate.stable_id()) == Some(&team_id)
                    // 6EAF7E/6EB079: the CANDIDATE asks whether the team
                    // owner is allied. Do not turn this into a mutual test.
                    || !is_allied_with(
                        self.house_alliances.as_map(),
                        self.interner.resolve(candidate.owner()),
                        owner,
                    )
                {
                    continue;
                }
                let Some(to) = self.team_target_coord(TeamTarget::Object(candidate.stable_id()))
                else {
                    continue;
                };
                // Coord magnitude41C380 and its inline Infantry equivalent
                // use the shared Sqrt_Approx/ftol length, not exact DistSq.
                let distance = distance_3d_leptons([from.x, from.y, from.z], [to.x, to.y, to.z]);
                if best_distance == -1 || distance < best_distance {
                    best = Some(candidate);
                    best_distance = distance;
                }
            }
        }
        let Some(friendly) = best else {
            return (None, None);
        };
        let zone = friendly
            .low_bridge_tube_state
            .as_ref()
            .and_then(|state| self.resolved_terrain.as_ref()?.tube(state.tube_id()))
            .map_or(TeamTarget::Object(friendly.stable_id()), |tube| {
                // CalcCenter's Script10 arm resolves the exit at6EB17E.
                self.canonical_team_cell((tube.exit.0 as i16, tube.exit.1 as i16))
            });
        (Some(zone), Some(head))
    }

    /// Team6EDB50..6EDC6B (dispatch6E9832): refresh the centre, store it in
    /// focus directly, Coordinate_Move, then mirror the followed Techno's
    /// target for live joined, stationary, armed Guard members. A tube exit
    /// is a Cell, so it cannot supply a target. The argument is unused.
    /// Evidence: the author's saved instruction-level action-10 dump from
    /// gamemd.exe; addresses and evidence boundary in docs/script-action-10.md.
    pub(super) fn team_action_follow_friendlies(
        &mut self,
        team_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        self.team_calc_center(team_id, rules, registry);
        let Some(team) = self.team_script_vm.teams.get_mut(&team_id) else {
            return;
        };
        // Not Assign_Mission_Target: preserve mission_target and do not
        // broadcast that setter's member-order cleanup.
        team.focus = team.zone;
        self.team_coordinate_move(team_id, rules, registry, frame_effects);
        let Some(TeamTarget::Object(friendly)) = self
            .team_script_vm
            .teams
            .get(&team_id)
            .and_then(|team| team.zone)
        else {
            return;
        };
        let mut member = self
            .team_script_vm
            .teams
            .get(&team_id)
            .and_then(|team| team.members().next());
        while let Some(id) = member {
            let Some(team) = self.team_script_vm.teams.get(&team_id) else {
                return;
            };
            member = team.next_member_after(id);
            let Some(entity) = self.substrate.entities.get(id) else {
                continue;
            };
            if !member_is_live_joined(
                entity,
                self.team_member_initiated(team_id, id),
                self.object_placement_scope_active(),
            ) {
                continue;
            }
            let Some(followed) = self.substrate.entities.get(friendly) else {
                return;
            };
            let target = followed.attack_target.target();
            // Script10/6EDBDD compares TarCom allocation identity.
            if let Some(held) = entity.attack_target.target()
                && !target.is_some_and(|target| {
                    NavTargetRef::from(held)
                        .same_pointer(target.into(), self.resolved_terrain.as_ref())
                })
            {
                self.team_member_clear_target(id, rules);
            }
            let Some(target) = self
                .substrate
                .entities
                .get(friendly)
                .and_then(|e| e.attack_target.target())
            else {
                continue;
            };
            let Some(entity) = self.substrate.entities.get(id) else {
                continue;
            };
            if is_moving(entity) == Some(false)
                && self
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| is_armed(entity, object))
                && entity.mission.effective() == MissionId::from_known(MissionType::Guard)
            {
                // Queue only, not Commence: the ordinary mission owner
                // decides when this queued Attack becomes current.
                self.team_member_queue_mission(id, MissionType::Attack, rules);
                self.team_member_assign_target(id, target.into(), rules, registry, frame_effects);
            }
        }
        // Coordinate_Move6EBF25..6EBF45 may already have marked +80 on
        // arrival. Unlike the C# progress callback, do not force Waiting.
    }
}
