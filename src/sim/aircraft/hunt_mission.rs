//! `AircraftClass::Mission_Hunt @ 0x00414A80` (vt+0x228), an aircraft's Hunt.
//!
//! - Without Ammo (`+0x2FC` 0) the aircraft leaves its team
//!   (`TeamClass::Remove_Member(this, -1, 0)`, `0x006EA870`), enters idle
//!   mode (vt+0x484 `(0, 1)`) and returns the Rate epilogue,
//!   `ftol(Rate * 900) + RandomRanged(0, 2)` (`0x00414AB2..0x00414ADE`), of
//!   the mission current after that call: the idle mode may have commenced
//!   another (`0x005B3A00` reads `+0xAC`).
//! - With a Target (`+0x2B4`) it queues Attack (vt+0x1E8 `(Attack, 0)`) and
//!   returns 1.
//! - Without one it assigns (vt+0x3C8) what Greatest_Threat (vt+0x3C4,
//!   Foot `0x004D9920`) finds around its Location, NULL included: in a
//!   nonzero game mode (`0x00A8B238`) first with the harvester mask `0x40`,
//!   then, still without a Target, with mask 0. A Target found queues Attack;
//!   none sends it into idle mode. Either way it returns 1.
//!
//! Mask 0 finds nothing for an aircraft: it has no `+0x3C4` override of its
//! own, so its flags word is 0 (`0x006F8F29..0x006F8F72`) and the class gate
//! (`0x006F821A`) refuses every candidate. Mask `0x40`'s flags word admits
//! units and buildings, and its quarry gate (`0x006F866D`) only types with
//! `Storage=`. A hunting aircraft therefore takes only harvesters and
//! refineries, and only in multiplayer; a human-owned one outside a team
//! passes over a refinery, as over any unarmed building
//! (`0x006F85AB..0x006F8601`).
//!
//! Callers: the house's All_To_Hunt (`0x00501400`, which Strategy runs when
//! a computer house loses its last factory), the idle mode's pick for a
//! `+0x3D4` aircraft with Ammo outside a team, and Hunt orders from triggers.
//!
//! RNG: the Rate epilogue's one Scenario `RandomRanged(0, 2)`; the scans
//! draw nothing. Timer writes: the mission timer epilogue
//! ([`super::dispatch_mission`]).
//!
//! Evidence: tools/spatial_oracle/aircraft_hunt.py runs the original
//! 0x00414A80 over Ammo (0, 1, 3, unlimited -1), team, Target, game mode,
//! every scan answer and, without Ammo, each mission the idle mode may
//! commence, with the original MissionControl lookup; its tests replay every
//! row through [`hunt_visit`].

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{ScanMission, TargetKind};
use crate::sim::mission::MissionType;
use crate::sim::world::{ObjectAiCtx, Simulation};

#[cfg(test)]
#[path = "hunt_mission_tests.rs"]
mod tests;

/// The harvester mask the multiplayer pass pushes (`0x00414B19`).
const HARVESTER_MASK: u32 = 0x40;

/// What a visit reads once, before it calls anything.
struct HuntFacts {
    /// Ammo `+0x2FC`.
    ammo: i32,
    /// The team `+0x5D4` is set.
    team: bool,
    /// The session's game mode (`0x00A8B238`) is nonzero.
    game_mode: bool,
}

/// What a Mission_Hunt visit does and reads live, each where the original
/// does it.
trait HuntHost {
    /// What Greatest_Threat answers.
    type Threat;
    /// The Target `+0x2B4` is set.
    fn target(&mut self) -> bool;
    /// vt+0x3C4 `Greatest_Threat(mask, &Location, 0)`.
    fn greatest_threat(&mut self, mask: u32) -> Option<Self::Threat>;
    /// vt+0x3C8 `Assign_Target(threat)`.
    fn assign_target(&mut self, threat: Option<Self::Threat>);
    /// vt+0x1E8 `Queue_Mission(mission, 0)` (`0x0041BA90`).
    fn queue(&mut self, mission: MissionType);
    /// vt+0x484 `Enter_Idle_Mode(0, 1)`.
    fn enter_idle_mode(&mut self);
    /// `TeamClass::Remove_Member(this, -1, 0)` (`0x006EA870`).
    fn leave_team(&mut self);
    /// The Rate epilogue of the current mission.
    fn epilogue(&mut self) -> i32;
}

/// One visit; returns its delay.
fn hunt_visit<H: HuntHost>(facts: &HuntFacts, host: &mut H) -> i32 {
    if facts.ammo == 0 {
        if facts.team {
            host.leave_team();
        }
        host.enter_idle_mode();
        return host.epilogue();
    }
    if !host.target() {
        if facts.game_mode {
            let threat = host.greatest_threat(HARVESTER_MASK);
            host.assign_target(threat);
        }
        if !host.target() {
            let threat = host.greatest_threat(0);
            host.assign_target(threat);
        }
        if !host.target() {
            host.enter_idle_mode();
            return 1;
        }
    }
    host.queue(MissionType::Attack);
    1
}

impl Simulation {
    /// [`hunt_visit`] on aircraft `id`.
    pub(crate) fn aircraft_mission_hunt(
        &mut self,
        id: u64,
        rules: &RuleSet,
        ctx: ObjectAiCtx<'_>,
    ) -> i32 {
        let entity = self.substrate.entities.get(id).expect("aircraft dispatch");
        debug_assert_eq!(entity.category(), EntityCategory::Aircraft);
        let facts = HuntFacts {
            ammo: entity
                .aircraft_ammo
                .as_ref()
                .map_or(-1, |ammo| ammo.current),
            team: self.team_script_vm.team_for_member(id).is_some(),
            game_mode: self.session.game_mode_nonzero,
        };
        hunt_visit(
            &facts,
            &mut WorldHunt {
                sim: self,
                id,
                rules,
                ctx,
            },
        )
    }
}

struct WorldHunt<'a> {
    sim: &'a mut Simulation,
    id: u64,
    rules: &'a RuleSet,
    ctx: ObjectAiCtx<'a>,
}

impl HuntHost for WorldHunt<'_> {
    type Threat = u64;

    fn target(&mut self) -> bool {
        let entity = self.sim.substrate.entities.get(self.id).unwrap();
        crate::sim::combat::target_present(
            entity.attack_target.as_ref(),
            &self.sim.substrate.entities,
        )
    }

    fn greatest_threat(&mut self, mask: u32) -> Option<u64> {
        let mission = match mask {
            0 => ScanMission::Hunt,
            mask => ScanMission::Quarry {
                mask,
                only_target_house_enemy: false,
            },
        };
        crate::sim::world::direct_greatest_threat(
            self.sim,
            self.rules,
            self.ctx.overlay_registry,
            self.id,
            mission,
        )
    }

    fn assign_target(&mut self, threat: Option<u64>) {
        self.sim
            .assign_target_represented(
                self.id,
                threat.map(TargetKind::Entity),
                Some(self.rules),
                None,
                crate::sim::world::FrameEffects::default(),
            )
            .expect("hunting aircraft");
    }

    fn queue(&mut self, mission: MissionType) {
        super::queue_mission(self.sim, self.id, mission);
    }

    fn enter_idle_mode(&mut self) {
        super::enter_idle_mode_for(
            self.sim,
            self.id,
            self.rules,
            self.ctx.overlay_registry,
            self.ctx.effects,
        );
    }

    fn leave_team(&mut self) {
        self.sim
            .leave_team(self.id, false, Some(self.rules), self.ctx.effects);
    }

    fn epilogue(&mut self) -> i32 {
        self.sim
            .mission_rate_epilogue_for(self.rules, self.id, MissionType::Hunt)
    }
}
