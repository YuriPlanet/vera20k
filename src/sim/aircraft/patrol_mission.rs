//! AircraftClass::Mission_Patrol417300 (Mission25, vt+25C), original retail
//! instructions417300..4176D9. Map Aircraft records and team script action11
//! can assign Patrol. State0 picks a landing cell and updates the live team's
//! mission target; state1 starts Move_To; state2 may suspend Patrol for Enter
//! or Attack before checking arrival/landing; state4 only checks movement and
//! landing. State3 enters idle unconditionally. Only state0/default draws the
//! current mission's Rate epilogue (one Scenario RandomRanged0..2).
//! Aircraft's literal2 scan retains zero class flags6F8F29..6F8F76 (also
//! when Foot688 coerces it to1), so ordinary Techno targets fail6F821A.
//! Preserve the conditional Attack override without adding weapon class bits.
//!
//! The legacy House+210 waypoint graph is separate from active YR PlanMgr.
//! Its only waypoint insertion caller502160 has no decoded call/jump/pointer
//! reference; active YR commands use6379C0/637A10 and refuse Aircraft639DA0.
//! The +520 waypoint arm and +430 latch (whose only reader is4DC8C9 in that
//! arm) are therefore omitted for normal YR startup/orders. This does not
//! establish their absence from externally fabricated saves or memory.

use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{ScanMission, TargetKind};
use crate::sim::components::NavTargetRef;
use crate::sim::mission::authority::OverridePacket;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::{motion_query, nav_target_coordinate};
use crate::sim::world::{ObjectAiCtx, Simulation};

#[cfg(test)]
#[path = "patrol_mission_tests.rs"]
mod tests;

/// Live reads/virtual calls of417300, at their original sequence points.
trait PatrolHost {
    type Object: Copy;
    fn nav_com(&self) -> bool;
    fn enter_idle(&mut self);
    fn assign_attack_cell(&mut self);
    fn assign_team_target(&mut self);
    fn move_to_nav_com(&mut self);
    fn ammo(&self) -> i32;
    fn find_dock(&mut self) -> Option<Self::Object>;
    fn greatest_threat(&mut self) -> Option<Self::Object>;
    fn override_mission(&mut self, mission: MissionType, object: Self::Object);
    fn is_moving(&mut self) -> bool;
    fn nav_cell_free(&mut self) -> bool;
    fn epilogue(&mut self) -> i32;
}

/// An explicit state store, if any, and the returned delay. None preserves
/// state writes made by idle/callbacks; native does not store on those arms.
fn patrol_visit<H: PatrolHost>(state: u32, host: &mut H) -> (Option<u32>, i32) {
    match state {
        0 => {
            let next = if host.nav_com() {
                host.assign_attack_cell();
                host.assign_team_target();
                Some(1)
            } else {
                host.enter_idle();
                None
            };
            (next, host.epilogue())
        }
        1 => {
            if host.nav_com() {
                host.move_to_nav_com();
                (Some(2), 1)
            } else {
                host.enter_idle();
                (None, 1)
            }
        }
        2 => {
            if host.ammo() == 0
                && let Some(dock) = host.find_dock()
            {
                host.override_mission(MissionType::Enter, dock);
                return (Some(0), 1);
            }
            // FindDock's radio callbacks can change Ammo. Read it again.
            if host.ammo() != 0
                && let Some(target) = host.greatest_threat()
            {
                host.override_mission(MissionType::Attack, target);
                return (Some(0), 1);
            }
            if !host.is_moving() {
                (Some(3), 1)
            } else if host.nav_com() && !host.nav_cell_free() {
                (Some(0), 1)
            } else {
                (Some(4), 1)
            }
        }
        3 => {
            host.enter_idle();
            (None, 1)
        }
        4 => {
            let next = if !host.is_moving() {
                Some(3)
            } else if host.nav_com() && !host.nav_cell_free() {
                Some(0)
            } else {
                None
            };
            (next, 1)
        }
        _ => (None, host.epilogue()),
    }
}

impl Simulation {
    pub(crate) fn aircraft_mission_patrol(
        &mut self,
        id: u64,
        rules: &RuleSet,
        ctx: ObjectAiCtx<'_>,
    ) -> i32 {
        let state = self
            .substrate
            .entities
            .get(id)
            .unwrap()
            .mission
            .handler_state();
        let (next, delay) = patrol_visit(
            state,
            &mut WorldPatrol {
                sim: self,
                id,
                rules,
                ctx,
            },
        );
        if let Some(next) = next
            && let Some(entity) = self.substrate.entities.get_mut(id)
        {
            entity.mission.set_handler_state(next);
        }
        delay
    }
}

struct WorldPatrol<'a> {
    sim: &'a mut Simulation,
    id: u64,
    rules: &'a RuleSet,
    ctx: ObjectAiCtx<'a>,
}

impl WorldPatrol<'_> {
    fn nav(&self) -> Option<NavTargetRef> {
        self.sim
            .substrate
            .entities
            .get(self.id)
            .unwrap()
            .navigation
            .nav_com()
    }
}

impl PatrolHost for WorldPatrol<'_> {
    type Object = u64;

    fn nav_com(&self) -> bool {
        self.nav().is_some()
    }

    fn enter_idle(&mut self) {
        super::enter_idle_mode_for(
            self.sim,
            self.id,
            self.rules,
            self.ctx.overlay_registry,
            self.ctx.effects,
        );
    }

    fn assign_attack_cell(&mut self) {
        let destination = self
            .sim
            .aircraft_find_attack_cell(self.id, self.nav(), self.rules);
        self.sim
            .assign_aircraft_destination(self.id, destination, self.rules, self.ctx.effects);
    }

    fn assign_team_target(&mut self) {
        //41738D rereads Team and NavCom after Assign_Destination41AA80.
        if let Some((team, _)) = self.sim.team_script_vm.team_for_member(self.id) {
            self.sim.team_assign_mission_target(
                team,
                self.nav().map(|nav| TargetKind::from(nav).into()),
                self.rules,
                self.ctx.overlay_registry,
                self.ctx.effects,
            );
        }
    }

    fn move_to_nav_com(&mut self) {
        let coord = nav_target_coordinate(
            self.nav().expect("Patrol state1 NavCom"),
            Some(self.id),
            self.sim,
            self.sim.resolved_terrain.as_ref(),
            Some((self.rules, &self.sim.interner)),
        )
        .expect("live Patrol NavCom coordinate");
        self.sim
            .aircraft_locomotor_move_to(self.id, coord, self.rules);
    }

    fn ammo(&self) -> i32 {
        self.sim
            .substrate
            .entities
            .get(self.id)
            .unwrap()
            .ammo()
            .map_or(-1, |ammo| ammo.current())
    }

    fn find_dock(&mut self) -> Option<u64> {
        self.sim
            .aircraft_find_docking_bay(self.id, self.rules, self.ctx.effects)
    }

    fn greatest_threat(&mut self) -> Option<u64> {
        //417461..417481 pushes mask2, a copy of Location, and false.
        // The represented scanner returns only Techno entities (native bit2).
        crate::sim::world::direct_greatest_threat(
            self.sim,
            self.rules,
            self.ctx.overlay_registry,
            self.id,
            ScanMission::AreaGuard,
        )
    }

    fn override_mission(&mut self, mission: MissionType, object: u64) {
        let packet = OverridePacket {
            mission: MissionId::from_known(mission),
            combat_target: (mission == MissionType::Attack).then_some(TargetKind::Entity(object)),
            destination: (mission == MissionType::Enter).then_some(NavTargetRef::object(object)),
        };
        self.sim
            .mission_override_represented(
                self.id,
                packet,
                self.rules,
                self.ctx.overlay_registry,
                self.ctx.effects,
            )
            .expect("represented Patrol override setters");
    }

    fn is_moving(&mut self) -> bool {
        motion_query::is_moving(self.sim.substrate.entities.get(self.id).unwrap()) == Some(true)
    }

    fn nav_cell_free(&mut self) -> bool {
        self.sim.aircraft_nav_cell_free(self.id, self.rules)
    }

    fn epilogue(&mut self) -> i32 {
        Simulation::mission_rate_epilogue_for(
            &mut self.sim.scenario_rng,
            &self.sim.substrate,
            self.rules,
            self.id,
            MissionType::Patrol,
        )
    }
}
