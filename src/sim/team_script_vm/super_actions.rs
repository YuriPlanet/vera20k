//! The computer's superweapon script actions (jump table `0x006E9F74`):
//! action 55 (`0x006EFC70`, case `0x006E9D95`) fires the house's Iron
//! Curtain at the team, action 57 (`0x006F0130`, case `0x006E9DC7`)
//! chronoshifts the team onto its leader's quarry. Retail AIMD runs 55 in
//! the "Soviet Iron Curtain" scripts after action 53 and 57 in "Allied
//! Chronosphere" after action 54.
//!
//! Evidence: `tools/superweapon_oracle.py` section `team_super_actions` runs
//! both handlers natively (the leader loop, the Supers search, the power
//! ratio, the RechargeTimer read, `GetRechargeTime` and `Quarry_To_Threat`;
//! the threat scan and Fire_SW are recorded stubs); `super_actions_tests.rs`
//! replays it.
//!
//! RESIDUALS:
//! - A team without a centre (`+0x34` NULL: no live joined member in the
//!   playfield) faults natively once its action fires, reading the centre's
//!   vtable (`0x006EFD6D`, `0x006F026F`). VERA fires nothing and finishes
//!   the action. Trigger: a charged Super with full power while the team's
//!   members have all left the playfield or not yet joined.
//! - Fire_SW takes the Super's `Type=` value as its Supers index (see
//!   [`Simulation::team_fire_super`]). A `[SuperWeaponTypes]` list shorter
//!   than that value makes Fire_SW read past the Supers vector natively;
//!   VERA fires nothing. Dormant: retail lists all twelve types in `Type=`
//!   order.
//! - The per-Super charge time override (`SuperClass+0x24`): the wait test
//!   divides by the type's `RechargeTime=` ([`super_nearly_ready`]'s
//!   residual).
//!
//! Ledger: neither handler draws a random number, writes a timer or detaches
//! anything itself. Greatest_Threat's empty-scan latch is the shared scan's
//! (`world/techno_ai/target_scan.rs`), Assign_Mission_Target's writes are
//! [`Simulation::team_assign_mission_target`]'s, and the recharge restarts
//! and launch effects are Fire_SW's (`superweapon/fire.rs`).

use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponKind;
use crate::sim::intern::InternedId;
use crate::sim::superweapon::{super_nearly_ready, super_types_with_type};
use crate::sim::world::Simulation;

use super::TeamTarget;

/// What an action does with the Super it checks this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SuperStep {
    /// Charged with full power: fire.
    Fire,
    /// Nearly charged: the team stays on the action.
    Wait,
    /// The action finishes.
    Finish,
}

impl Simulation {
    /// Script action 55 (`0x006EFC70`): the leader's house fires its first
    /// Iron Curtain (`Type=` 1) at the cell of the team's centre once
    /// [`Self::team_super_step`] allows, then the action finishes. It also
    /// finishes without members or such a Super, and waits while the Super
    /// nearly has its charge.
    pub(super) fn team_action_iron_curtain(
        &mut self,
        team_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let checked = self.team_leader(team_id, rules).and_then(|leader| {
            // The leader's house (`+0x21C`).
            let owner = self.substrate.entities.get(leader)?.owner();
            let curtain =
                super_types_with_type(rules, SuperWeaponKind::IronCurtain.native_index()).next()?;
            Some((owner, self.team_super_step(owner, curtain, rules)))
        });
        match checked {
            Some((_, SuperStep::Wait)) => return,
            Some((owner, SuperStep::Fire)) => {
                if let Some(centre) = self.team_centre_cell(team_id) {
                    self.team_fire_super(
                        owner,
                        SuperWeaponKind::IronCurtain,
                        centre,
                        rules,
                        registry,
                    );
                }
            }
            Some((_, SuperStep::Finish)) | None => {}
        }
        if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
            team.advance_pending = true;
        }
    }

    /// Script action 57 (`0x006F0130`), argument the quarry: with the
    /// leader's house's last Chronosphere (`Type=` 3) passing
    /// [`Self::team_super_step`], the leader looks for the quarry as action 0
    /// does ([`Self::team_quarry_threat`]). Finding one, the house fires the
    /// Chronosphere at the cell of the team's centre and its last Chrono Warp
    /// (`Type=` 4, whose charge is not read) at the target's cell, and the
    /// target becomes the team's mission target (`Assign_Mission_Target @
    /// 0x006E9050`). The action then finishes, target or not. It also
    /// finishes without members, a Chronosphere or a Chrono Warp, and waits
    /// while the Chronosphere nearly has its charge.
    pub(super) fn team_action_chronoshift(
        &mut self,
        team_id: u64,
        quarry: i32,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let checked = self.team_leader(team_id, rules).and_then(|leader| {
            let owner = self.substrate.entities.get(leader)?.owner();
            let sphere = super_types_with_type(rules, SuperWeaponKind::ChronoSphere.native_index())
                .last()?;
            super_types_with_type(rules, SuperWeaponKind::ChronoWarp.native_index()).last()?;
            Some((leader, owner, self.team_super_step(owner, sphere, rules)))
        });
        match checked {
            Some((_, _, SuperStep::Wait)) => return,
            Some((leader, owner, SuperStep::Fire)) => {
                if let Some(target) =
                    self.team_quarry_threat(team_id, leader, quarry, rules, registry)
                    && let Some(centre) = self.team_centre_cell(team_id)
                    && let Some(TeamTarget::Cell { x, y }) =
                        self.team_target_cell_of_coord(TeamTarget::Object(target))
                {
                    self.team_fire_super(
                        owner,
                        SuperWeaponKind::ChronoSphere,
                        centre,
                        rules,
                        registry,
                    );
                    self.team_fire_super(
                        owner,
                        SuperWeaponKind::ChronoWarp,
                        (x, y),
                        rules,
                        registry,
                    );
                    self.team_assign_mission_target(
                        team_id,
                        Some(TeamTarget::Object(target)),
                        rules,
                        registry,
                    );
                }
            }
            Some((_, _, SuperStep::Finish)) | None => {}
        }
        if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
            team.advance_pending = true;
        }
    }

    /// The test both actions put to the Super they check
    /// (`0x006EFD46..0x006EFE4F`, `0x006F01ED..0x006F020A` and
    /// `0x006F032B..0x006F039C`): charged (`+0x6F`) with the house's power
    /// ratio (`GetPowerRatio @ 0x004FCE30`, `FCOMP` with 1.0) not below one
    /// fires; otherwise a Super that is [`super_nearly_ready`] waits.
    fn team_super_step(&self, owner: InternedId, type_name: &str, rules: &RuleSet) -> SuperStep {
        let charged = self
            .interner
            .get(type_name)
            .and_then(|type_id| self.super_weapons.get(&owner)?.get(&type_id))
            .is_some_and(|instance| instance.is_ready);
        // A house without a power state has produced 0 of 0 drained: ratio 1.
        let full_power = self
            .power_states
            .get(&owner)
            .is_none_or(|power| power.has_full_power());
        if charged && full_power {
            SuperStep::Fire
        } else if super_nearly_ready(self, rules, owner, type_name) {
            SuperStep::Wait
        } else {
            SuperStep::Finish
        }
    }

    /// The cell of the team's centre (`+0x34`, its GetCoords over 256 toward
    /// zero), `None` without one (module residual).
    fn team_centre_cell(&self, team_id: u64) -> Option<(i16, i16)> {
        let zone = self.team_script_vm.teams.get(&team_id)?.zone?;
        match self.team_target_cell_of_coord(zone)? {
            TeamTarget::Cell { x, y } => Some((x, y)),
            TeamTarget::Object(_) => None,
        }
    }

    /// `Fire_SW(Type=, &cell)` (`0x006EFDB4`, `0x006F02B6`, `0x006F030D`):
    /// the actions pass the Super's `Type=` value where Fire_SW takes an
    /// index into the house's Supers, the `[SuperWeaponTypes]` entry at that
    /// position. That is the Super itself when the list puts each type at
    /// the index of its `Type=`, as retail does.
    fn team_fire_super(
        &mut self,
        owner: InternedId,
        kind: SuperWeaponKind,
        (x, y): (i16, i16),
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let Some(name) = usize::try_from(kind.native_index())
            .ok()
            .and_then(|index| rules.super_weapon_order.get(index))
        else {
            return;
        };
        let sw_type_id = self.interner.intern(name);
        self.fire_super_weapon(rules, owner, sw_type_id, (x as u16, y as u16), registry);
    }
}

#[cfg(test)]
#[path = "super_actions_tests.rs"]
mod tests;
