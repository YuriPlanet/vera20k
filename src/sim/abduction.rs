//! Ares `Abductor=` weapons (#680 Chrono Prison): the shot puts an eligible
//! target into its firer's passenger hold instead of hitting it.
//!
//! Native owner: `WeaponTypeExt::ExtData::conductAbduction` (Ares.dll
//! 10054A80), from two hooks:
//! - BulletClass_Detonate (100536E0, hook 0x46920B): a bullet snapped to its
//!   target (within 64 leptons) of a weapon without `Abductor.Temporal`
//!   abducts its Foot target; the bullet is then neutered and the other
//!   WarheadTypeExt effects are skipped (`sim::combat::world_receiver`).
//! - TemporalClass_Update_Abductor (100564F0, hook 0x71AAAC): the warp of an
//!   `Abductor.Temporal` weapon abducts the target it erases instead of
//!   destroying it (`sim::temporal`).
//!
//! Ledger: no RNG of its own; the anim, the owner change, Limbo and the hold
//! keep their owners' draws and detaches. The target's TemporalTargetingMe
//! chain is detached (ClearLinkedList), its captor frees it, its own captives
//! and spawns go, its slaves are freed to the abductor, and it leaves its
//! team.
//!
//! RESIDUALS (trigger: an abducted object in that state; frequency: mod
//! data only, no retail weapon is an `Abductor`):
//! - A spawned or enslaved abductee: Ares's DetachSpecificSpawnee and
//!   FreeSpecificSlave (10044380, 10044980) are not ported, so it stays its
//!   spawner's or master's; a slave master's manager stays detached after
//!   FreeSlaves (native zeroes it and gives it back to the abductee).
//! - The spawn manager's ResetTarget, CurrentTargets, Flashing, the
//!   locomotor's Force_Track and occupation re-mark, the piggyback end and
//!   the fresh locomotor of the type's kind, and the magnetron latches: a
//!   piggybacked or lifted abductee keeps that state in the hold.
//! - A Building abductor's abductee is not marked Absorbed.
//! - A non-Foot target, which native casts to FootClass regardless, is
//!   refused.

use crate::rules::ruleset::RuleSet;
use crate::rules::weapon_type::WeaponType;
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::{FrameEffects, Simulation, UninitContext};

impl Simulation {
    /// `conductAbduction` (Ares.dll 10054A80): `owner` takes `target` into its
    /// hold with `weapon`'s `Abductor.*` settings. Refused unless the weapon
    /// is an `Abductor`, the target's type is not `ImmuneToAbduction`, the
    /// weapon's warhead CanAffectHouse it for the owner's house, it is not
    /// Iron Curtained, its health ratio is at most `AbductBelowPercent`, its
    /// health at most a positive `MaxHealth`, and the owner can hold it
    /// (CanHoldPassenger 100448E0). Answers whether it was taken.
    pub(crate) fn ares_conduct_abduction(
        &mut self,
        owner: u64,
        target: u64,
        weapon: &WeaponType,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let settings = &weapon.ares_abductor;
        if !settings.abductor || !self.ares_abduction_admits(owner, target, weapon, rules) {
            return false;
        }
        let context = UninitContext::new(rules, registry).with_effects(effects);
        let now = self.session.binary_frame();
        // "The target forgets what it was doing": Stop_Moving (vt+0x500,
        // 0x004DF0D0), the null destination (vt+0x480) and target (vt+0x3C8),
        // SetFocus(NULL) (0x0070C610), QueueMission(Sleep, 1), the mission
        // tick and the gattling stage.
        if let Some(entity) = self.substrate.entities.get_mut(target) {
            entity.navigation.stop_moving();
        }
        self.assign_null_destination(target, rules, registry, effects);
        let _ = self.assign_target_represented(target, None, rules, registry, effects);
        if let Some(entity) = self.substrate.entities.get_mut(target) {
            entity.set_archive_target(None);
        }
        let _ = self.mission_queue_exact(
            target,
            MissionId::from_known(MissionType::Sleep),
            1,
            now,
            &LiveReadyInputProvider { rules },
        );
        if let Some(entity) = self.substrate.entities.get_mut(target) {
            entity.mission.clear_ai_counter();
            entity.gattling.set_stage(0);
        }
        // TeamClass::LiberateMember (0x006EA870).
        if let Some((team, _)) = self.team_script_vm.team_for_member(target) {
            self.team_remove_member(team, target, false, rules, effects);
        }
        if let Some(controller) = self
            .substrate
            .entities
            .get(target)
            .and_then(|entity| entity.mind_control.controller())
        {
            self.free_unit(controller, target, rules, registry, effects);
        }
        self.free_all_captures(target, rules, registry, effects);
        // TemporalClass::Detach (ClearLinkedList 0x0071ADE0) of the chain
        // warping it.
        if let Some(head) = self
            .substrate
            .entities
            .get(target)
            .and_then(|entity| entity.temporal.chain_head())
        {
            self.temporal_clear_linked_list(head, rules, effects);
        }
        // SpawnManagerClass::KillNodes, then SlaveManagerClass::Killed with
        // the abductor as the killer.
        crate::sim::spawn_manager::kill_all_spawns_with_context(self, target, context);
        self.free_slaves(target, Some(owner), None, rules, registry, effects);
        if let Some(anim) = settings.anim.as_deref()
            && let Some(entity) = self.substrate.entities.get(target)
        {
            // GameCreate<AnimClass>(type, Location, 0, 1, 0x600, 0, false).
            let coord = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
            let world = crate::sim::anim_class::AnimWorldCoord {
                x: coord.x,
                y: coord.y,
                z: coord.z,
            };
            let (rx, ry, sub_x, sub_y, z) = world.to_cell_sub_z();
            let descriptor = crate::sim::components::AnimClassSpawnDescriptor {
                delay: 0,
                loop_count: 1,
                draw_flags: 0x600,
                z_adjust: 0,
                reverse: false,
                ..crate::sim::components::AnimClassSpawnDescriptor::new(
                    self.interner.intern(anim),
                    rx,
                    ry,
                    sub_x,
                    sub_y,
                    z,
                )
            };
            if let Err(error) = self.spawn_anim_at_world(rules, descriptor, world) {
                log::debug!("Abductor.Anim [{anim}] did not construct: {error}");
            }
        }
        // ClearPlanningTokens(NULL) (0x006386E0).
        self.planning_detach_actor(target);
        let owner_house = self
            .substrate
            .entities
            .get(owner)
            .map(|entity| entity.owner());
        let previous_owner = self
            .substrate
            .entities
            .get(target)
            .map(|entity| entity.owner());
        if settings.change_owner
            && let Some(owner_house) = owner_house
            && !self.ares_psionics_immune(target, rules)
        {
            self.change_owner_with_rules(target, owner_house, rules, registry, effects);
        }
        if self.techno_limbo_with_context(target, context)
            != crate::sim::world::ConcealOutcome::Concealed
        {
            log::warn!("Abduction: Target unit {target} could not be removed.");
        }
        if let Some(entity) = self.substrate.entities.get_mut(target) {
            entity.on_bridge.store(false);
        }
        let rider = self.substrate.entities.get(owner).is_some_and(|carrier| {
            self.interner
                .object_type(carrier.type_ref(), rules)
                .is_some_and(|object| object.open_topped)
        }) && self.substrate.entities.get(target).is_some_and(|abductee| {
            owner_house.is_some_and(|house| {
                crate::sim::combat::combat_weapon::is_ally_by_object(
                    Some(&self.house_alliances),
                    &self.interner,
                    abductee.owner(),
                    house,
                )
            })
        });
        let size = self
            .substrate
            .entities
            .get(target)
            .and_then(|entity| self.interner.object_type(entity.type_ref(), rules))
            .map_or(1.0, |object| object.size);
        let boarded = self.ares_abduct_into_hold(target, owner, size, rider, context);
        log::info!("ABDUCT {target} into {owner}: {boarded}");
        // The trigger events after AddPassenger and Undiscover: 72 (abducted
        // by House, source the abductor) and 71 on the abductee, then 74
        // (abducts something of House, source its previous owner House) and
        // 73 on the abductor; 71 and 73 only while their object is alive.
        self.dispatch_object_tag_event(target, 72, Some(target), Some(owner), context);
        let alive = |sim: &Self, id| {
            sim.substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.lifecycle.object_alive())
        };
        if alive(self, target) {
            self.dispatch_object_tag_event(target, 71, Some(target), None, context);
        }
        self.dispatch_object_ares_event(
            owner,
            74,
            previous_owner.map(crate::sim::trigger_runtime::AresEventSource::House),
            context,
        );
        if alive(self, owner) {
            self.dispatch_object_tag_event(owner, 73, Some(owner), None, context);
        }
        true
    }

    /// conductAbduction's refusals, in its order.
    fn ares_abduction_admits(
        &self,
        owner: u64,
        target: u64,
        weapon: &WeaponType,
        rules: &RuleSet,
    ) -> bool {
        let (Some(carrier), Some(victim)) = (
            self.substrate.entities.get(owner),
            self.substrate.entities.get(target),
        ) else {
            return false;
        };
        if !victim.category().is_foot() {
            return false;
        }
        let (Some(carrier_type), Some(victim_type)) = (
            self.interner.object_type(carrier.type_ref(), rules),
            self.interner.object_type(victim.type_ref(), rules),
        ) else {
            return false;
        };
        if victim_type.ares_immune_to_abduction {
            return false;
        }
        // WarheadTypeExt::CanAffectHouse (Ares.dll 10051B70) of the weapon's
        // warhead for the owner's house.
        if let Some(warhead) = weapon.warhead.as_deref().and_then(|id| rules.warhead(id)) {
            let source = carrier.owner();
            let allied = crate::sim::combat::combat_weapon::is_ally_by_object(
                Some(&self.house_alliances),
                &self.interner,
                source,
                victim.owner(),
            );
            let defaults = crate::rules::ares_warhead::AresWarheadRules::default();
            if !warhead.ares_rules().unwrap_or(&defaults).can_affect_house(
                true,
                source == victim.owner(),
                allied,
                warhead.affects_allies,
            ) {
                return false;
            }
        }
        // IsIronCurtained (vt+0x160).
        if crate::sim::superweapon::invulnerability::is_invulnerable(
            victim.invulnerability.as_ref(),
            self.session.binary_frame(),
        ) {
            return false;
        }
        // GetHealthPercentage: Health / Strength in doubles.
        let ratio = f64::from(victim.health.current()) / f64::from(victim_type.strength);
        if f64::from_bits(weapon.ares_abductor.abduct_below_percent.bits()) < ratio {
            return false;
        }
        let max_health = weapon.ares_abductor.max_health;
        if max_health > 0 && max_health < victim.health.current() {
            return false;
        }
        // CanHoldPassenger (100448E0).
        if victim_type.size > carrier_type.size_limit {
            return false;
        }
        let (count, total) = carrier
            .passenger_role
            .cargo()
            .map_or((0, 0), |cargo| (cargo.count(), cargo.total_size()));
        if carrier_type.ares_passengers_by_size {
            victim_type.size <= f64::from(carrier_type.passengers - total)
        } else {
            (count as i32) < carrier_type.passengers
        }
    }

    /// HasAresAbility's `PSIONICSIMMUNE` or the type's `ImmuneToPsionics=`,
    /// which keep an `Abductor.ChangeOwner` abductee's house.
    fn ares_psionics_immune(&self, id: u64, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return true;
        };
        self.interner
            .object_type(entity.type_ref(), rules)
            .is_none_or(|object| {
                object.immune_to_psionics
                    || object
                        .promotion_abilities()
                        .is_some_and(|abilities| abilities.psionics_immune(entity.veterancy.raw()))
            })
    }
}

#[cfg(test)]
#[path = "abduction_tests.rs"]
mod tests;
