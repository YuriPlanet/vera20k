//! The block `BuildingClass::ReceiveDamage` (`0x00442230`) runs at
//! `0x00442942..0x00442A90`, after the TechnoClass receiver (`0x00442425`,
//! whose ShouldRetaliate has already had its say), the result switch and the
//! IsAlive re-test (`0x00442905`), for a building a hit with a source left
//! alive with a non-zero result:
//!
//! - unless the type is `Insignificant=` (`+0x232`) or 1x1 with
//!   `UndeploysInto=` (vt+0x80, `0x00465D40`), the owner's
//!   `HouseClass::NotifyUnderAttack` (`0x004F93E0`);
//! - `+0x53C` takes the source house's index (`0x00442980`). A CRC fold and a
//!   `!= -1` test (`0x0070304D`, `0x007033C3`) are its only readers, and a
//!   house index never meets that test, so VERA keeps no copy;
//! - nothing more when the raw mission (`+0xAC`) is Selling, the owner is
//!   allied with the source (`0x004F9A90`), weapon 0 cannot aim
//!   ([`building_weapon0_aims`]), or TarCom is in range of the weapon
//!   SelectWeapon picks for it (vt+0x3AC, `0x006F7780`);
//! - an Aircraft source, or any source while the owner is human and
//!   `[CombatDamage] PlayerReturnFire=` (`Rules+0x17EC`) is clear, turns
//!   `+0x388` to a random direction. That happens only if `+0x388` is not
//!   still rotating (`0x004C9480`) and the building is operational (vt+0x350).
//!   One Scenario `Random::Next` (`0x00442A73`) draws it; its low byte is the
//!   direction's high byte (`Set_Desired` `0x004C9220`, at the type's
//!   `ROT=`). For a `Turret=yes` type `+0x388` is the turret; for any other
//!   it is the body;
//! - any other source is offered to BuildingClass::SetTarget (vt+0x3C8,
//!   `0x00443B90`), which takes it only in range.
//!
//! Evidence: `tools/spatial_oracle/building_retaliation.json` runs the block
//! natively, with Set_Desired, Is_Rotating, SetTarget, GetWeapon and the RNG;
//! the tests replay its rows through [`Simulation::building_hit_response`]
//! and one sourced hit through the production receiver.
//!
//! RESIDUALS:
//! - A V3, Dreadnought or Boomer missile's hit names its launcher as the
//!   source; native names the rocket, an AircraftClass
//!   (`RocketLocomotionClass` `0x006632B8..0x006632C7`). Trigger: such a
//!   missile hits an armed AI building. Effect: native turns `+0x388` and
//!   draws once; VERA offers the launcher to SetTarget and draws nothing.
//!   Frequency: common in AI games. Later owner: the missile-source chain,
//!   which also owns kill credit, ShouldRetaliate and experience.
//! - `NotifyUnderAttack` springs TEvent 6 ("attacked by any house") on the
//!   house's tags (`0x004F95D4..0x004F95FD`, `0x006E53A0`). VERA evaluates
//!   event6 but lacks this House-tag ingress. Trigger: a house-attached tag
//!   with an admitted attacked event. Effect: this producer does not spring
//!   it. Active retail skirmish reachability is unproved; a plain event6 is
//!   object-classified, so House attachment needs a compound/linked event.
//!   Later owner: the House-tag trigger chain.
//! - The killing blow of a Selling or `Explodes=` building, which keeps
//!   IsAlive and runs the block natively: the residual at the call in
//!   `combat::world_receiver::commit_entities`.
//! - A heal with a source and a non-zero result runs the block natively; the
//!   receiver's healing arm returns before it. Trigger, reachability not
//!   traced: a repair IFV's RepairBullet (`[Mechanical]`, `Damage=-50`, 100%
//!   against `medium`) healing a deployed Slave Miner (YAREFN,
//!   `Armor=medium`), if its targeting admits the building. Effect: native
//!   pings the ore-miner line and stops at the ally test; VERA pings nothing.
//! - Is_Operational has no EMP state; no retail EMP source was traced.
//!   Its Tesla charger count comes from `sim::tesla_charging`.
//!
//! No RNG beyond the one Scenario draw per turn; `+0x388`'s timer is the only
//! timer written; nothing is detached.

use super::building_missions::building_weapon0_aims;
use super::target_scan::{can_fire_at, select_weapon};
use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::combat_weapon::is_ally_by_object;
use crate::sim::combat::damage::DamageState;
use crate::sim::combat::{TargetKind, UnderAttackEvent, resolve_target_coords};
use crate::sim::mission::MissionType;
use crate::sim::world::Simulation;

impl Simulation {
    /// The block for building `id`, alive after a hit whose ReceiveDamage
    /// `result` is not PostMortem. Returns the NotifyUnderAttack ping.
    ///
    /// `source` is the source object; one no longer in the store is NULL, as
    /// native detaches a destroyed object (a bullet's source is cleared when
    /// its firer expires).
    pub(crate) fn building_hit_response(
        &mut self,
        id: u64,
        source: Option<u64>,
        result: DamageState,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
    ) -> Option<UnderAttackEvent> {
        // No source (`0x00442942`) or a result of 0 (`0x0044294A`).
        let source = source.filter(|&source| self.substrate.entities.contains(source))?;
        if result == DamageState::Unaffected {
            return None;
        }
        let building = self.substrate.entities.get(id)?;
        let source_entity = self.substrate.entities.get(source)?;
        let obj = self.object_type(building.type_ref(), rules)?;
        // The radar cell is the building's GetCoords (vt+0x48, `0x00447AC0`,
        // the foundation centre; `0x004F94AE`, `0x004F950E`, `0x004F956A`).
        let (rx, ry, _, _) = resolve_target_coords(&TargetKind::Entity(id), self)?;
        let ping = (!obj.insignificant && !obj.is_1x1_with_undeploy()).then(|| UnderAttackEvent {
            rx,
            ry,
            owner: building.owner(),
            victim_type: building.type_ref(),
        });
        if building.mission.current().known() == Some(MissionType::Selling)
            || is_ally_by_object(
                Some(&self.house_alliances),
                &self.interner,
                building.owner(),
                source_entity.owner(),
            )
            || !building_weapon0_aims(self, rules, id)
            || building.attack_target.target().is_some_and(|attack| {
                let weapon = select_weapon(self, rules, id, Some(attack));
                can_fire_at(self, rules, id, attack, weapon, overlay_registry)
            })
        {
            return ping;
        }
        let human = self.owner_is_human(building.owner());
        if source_entity.category() == EntityCategory::Aircraft
            || (human && !rules.general.player_return_fire)
        {
            let frame = self.session.binary_frame;
            let rotating = building.body_facing.is_rotating(frame);
            if !rotating && self.building_operational_state(id, rules) == Some(true) {
                let direction = ((self.scenario_rng.next_u32() & 0xFF) as u16) << 8;
                if let Some(building) = self.substrate.entities.get_mut(id) {
                    building.body_facing.set(direction, frame);
                }
            }
        } else {
            let _ = self.assign_target_represented(
                id,
                Some(TargetKind::Entity(source)),
                Some(rules),
                overlay_registry,
                crate::sim::world::FrameEffects::default(),
            );
        }
        ping
    }
}

#[cfg(test)]
#[path = "building_retaliation_tests.rs"]
mod tests;
