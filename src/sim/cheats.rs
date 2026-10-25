//! Opt-in local developer trainer, adapted from @1372704087's PR #2083.
//!
//! This intentionally changes gameplay, is absent without `debug-tools`, and
//! is never saved as control state. The app must explicitly admit an offline
//! session; construction, load, replay, and network contexts deny all writes.
//! Effects use their existing owners. In particular invincibility never
//! rewrites Iron Curtain timers, and power keeps its real assessed totals.

use crate::sim::intern::InternedId;
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::Simulation;

pub const MAX_MONEY_TARGET: i32 = 1_000_000_000;
pub const MAX_DAMAGE_SCALE: u16 = 100;

/// Original contributor's wire switches, all inert by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CheatFlags {
    pub infinite_money: bool,
    pub money_target: i32,
    pub infinite_power: bool,
    pub instant_build: bool,
    pub invincible: bool,
    pub damage_scale: u16,
    pub reveal_map: bool,
    /// Skip technology/country/prerequisite gates, retaining build limits.
    pub tech_all: bool,
    /// Lift the normal technology-level cap, retaining other gates.
    pub tech_boost: bool,
    /// Skip ground/proximity admission, retaining occupancy and map bounds.
    pub build_anywhere: bool,
}

impl CheatFlags {
    fn normalized(mut self) -> Self {
        self.money_target = self.money_target.clamp(0, MAX_MONEY_TARGET);
        self.damage_scale = self.damage_scale.min(MAX_DAMAGE_SCALE);
        self
    }

    fn active(self) -> bool {
        self.infinite_money
            || self.infinite_power
            || self.instant_build
            || self.invincible
            || self.damage_scale > 1
            || self.reveal_map
            || self.tech_all
            || self.tech_boost
            || self.build_anywhere
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainerAction {
    HealAll,
    VeteranAll,
    SuperweaponsReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DebugSessionContext {
    #[default]
    Disabled,
    Offline,
    Network,
    Replay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DebugToolsError {
    #[error("trainer is disabled outside an explicitly admitted offline match")]
    Unavailable,
    #[error("trainer requires the current living local human house")]
    NoLocalHouse,
}

#[derive(Debug, Default)]
pub(crate) struct DebugToolsState {
    context: DebugSessionContext,
    flags: CheatFlags,
    owner: Option<InternedId>,
    pending: u8,
    event_logging: bool,
    modified: bool,
    replay_locked: bool,
    logged_entities: Vec<u64>,
}

impl Simulation {
    /// Trusted runtime admission. Changing context drops all transient controls
    /// and hooks; repeating it does no work. It does not make a modified match
    /// suitable for deterministic recording again.
    pub fn set_debug_session_context(&mut self, context: DebugSessionContext) {
        if self.debug_tools.context == context
            || (self.debug_tools.replay_locked && context != DebugSessionContext::Replay)
        {
            return;
        }
        self.clear_debug_tools();
        self.debug_tools.context = context;
        self.debug_tools.replay_locked |= context == DebugSessionContext::Replay;
    }

    pub fn clear_debug_tools(&mut self) {
        let old_tech_owner = self
            .debug_tools
            .owner
            .filter(|_| self.debug_tools.flags.tech_all || self.debug_tools.flags.tech_boost);
        let modified = self.debug_tools.modified;
        let replay_locked = self.debug_tools.replay_locked;
        let logged = std::mem::take(&mut self.debug_tools.logged_entities);
        self.debug_tools = DebugToolsState {
            modified,
            replay_locked,
            context: if replay_locked {
                DebugSessionContext::Replay
            } else {
                DebugSessionContext::Disabled
            },
            ..Default::default()
        };
        for (_, power) in self.power_states.iter_mut() {
            power.set_debug_full_power(false);
        }
        if let Some(owner) = old_tech_owner {
            self.houses.request_house_tech_tree_recheck(owner);
        }
        for id in logged {
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                entity.set_debug_logging(false);
            }
        }
    }

    pub fn trainer_modified(&self) -> bool {
        self.debug_tools.modified
    }

    fn debug_local_owner(&self) -> Option<InternedId> {
        self.session.current_house().filter(|owner| {
            self.houses.get(owner).is_some_and(|house| {
                !house.is_defeated()
                    && house.is_controlled_by_human(self.session.game_mode_nonzero())
            })
        })
    }

    fn trainer_write_owner(&self) -> Result<InternedId, DebugToolsError> {
        if self.debug_tools.context != DebugSessionContext::Offline {
            return Err(DebugToolsError::Unavailable);
        }
        self.debug_local_owner()
            .ok_or(DebugToolsError::NoLocalHouse)
    }

    pub fn trainer_available(&self) -> bool {
        self.trainer_write_owner().is_ok()
    }

    pub fn trainer_unavailable_reason(&self) -> Option<&'static str> {
        match self.debug_tools.context {
            DebugSessionContext::Disabled => Some("No offline debug session is active"),
            DebugSessionContext::Network => Some("Trainer is disabled in network sessions"),
            DebugSessionContext::Replay => Some("Trainer is disabled during replay"),
            DebugSessionContext::Offline if self.debug_local_owner().is_none() => {
                Some("No living local human house")
            }
            DebugSessionContext::Offline => None,
        }
    }

    pub fn cheat_flags(&self) -> CheatFlags {
        self.debug_tools.flags
    }

    pub fn trainer_owner(&self) -> Option<InternedId> {
        self.debug_tools.owner
    }

    pub fn trainer_power_forced(&self) -> bool {
        self.debug_tools.flags.infinite_power
            && self
                .debug_tools
                .owner
                .and_then(|owner| self.power_states.get(&owner))
                .is_some_and(|power| power.is_low_power())
    }

    pub fn debug_reveal_map(&self) -> bool {
        self.trainer_available() && self.debug_tools.flags.reveal_map
    }

    pub fn set_cheat_flags(&mut self, flags: CheatFlags) -> Result<(), DebugToolsError> {
        let owner = self.trainer_write_owner()?;
        let flags = flags.normalized();
        self.debug_tools.modified |= flags.active();
        let old = self.debug_tools.flags;
        self.debug_tools.flags = flags;
        self.debug_tools.owner = Some(owner);
        for (&id, power) in self.power_states.iter_mut() {
            power.set_debug_full_power(id == owner && flags.infinite_power);
        }
        if old.tech_all != flags.tech_all || old.tech_boost != flags.tech_boost {
            self.houses.request_house_tech_tree_recheck(owner);
        }
        Ok(())
    }

    pub fn request_trainer_action(&mut self, action: TrainerAction) -> Result<(), DebugToolsError> {
        self.debug_tools.owner = Some(self.trainer_write_owner()?);
        self.debug_tools.pending |= match action {
            TrainerAction::HealAll => 1,
            TrainerAction::VeteranAll => 2,
            TrainerAction::SuperweaponsReady => 4,
        };
        self.debug_tools.modified = true;
        Ok(())
    }

    /// At the existing master-frame boundary; queued actions are consumed once.
    pub fn apply_debug_tools(&mut self, rules: &crate::rules::ruleset::RuleSet) {
        Self::refresh_debug_event_selection(&mut self.debug_tools, &mut self.substrate);
        let Ok(owner) = self.trainer_write_owner() else {
            if self.debug_tools.owner.is_some() || self.debug_tools.pending != 0 {
                self.clear_debug_tools();
            }
            return;
        };
        if self.debug_tools.owner.is_some_and(|old| old != owner) {
            self.clear_debug_tools();
            return;
        }
        let flags = self.debug_tools.flags;
        let pending = std::mem::take(&mut self.debug_tools.pending);
        if flags.infinite_money
            && let Some(house) = self.houses.get_mut(&owner)
            && house.economy().credits() < flags.money_target
        {
            // Add_Credits is the existing balance owner. Modulo subtraction
            // reaches the bounded target even from a native negative balance.
            let shortfall = flags.money_target.wrapping_sub(house.economy().credits());
            house.economy_mut().add_credits(shortfall);
        }
        if pending & 1 != 0 {
            let mut healed_structure = false;
            let (entities, interner) = self.entities_mut_and_interner();
            for entity in entities.values_mut() {
                if entity.owner() != owner
                    || !entity.lifecycle.object_alive()
                    || entity.death.is_dying()
                    || entity.health.current() <= 0
                {
                    continue;
                }
                if let Some(object) = interner.object_type(entity.type_ref(), rules)
                    && object.strength > entity.health.current()
                {
                    entity.health.set(object.strength);
                    healed_structure |=
                        entity.category() == crate::map::entities::EntityCategory::Structure;
                }
            }
            // The paused action bypasses Building AI's ordinary health sample.
            // Ask the existing House owner to refresh retained generation even
            // if the building's next live AI visit is gated.
            if healed_structure {
                Self::invalidate_house_power(&self.houses, &mut self.power_states, owner, true);
            }
        }
        if pending & 2 != 0 {
            for entity in self.substrate.entities.values_mut() {
                if entity.owner() == owner
                    && entity.lifecycle.object_alive()
                    && !entity.death.is_dying()
                    && entity.category() != crate::map::entities::EntityCategory::Structure
                {
                    entity.veterancy.set_elite();
                }
            }
        }
        if pending & 4 != 0 {
            let frame = self.session.binary_frame();
            for (type_id, instance) in self.super_weapons.house_instances_mut(&owner) {
                if let Some(sw) = rules.super_weapon(self.interner.resolve(*type_id)) {
                    instance.debug_charge_full(sw, frame);
                }
            }
        }
    }

    pub(crate) fn debug_flags_for(&self, owner: InternedId) -> CheatFlags {
        if self.trainer_available() && self.debug_tools.owner == Some(owner) {
            self.debug_tools.flags
        } else {
            CheatFlags::default()
        }
    }

    /// Filter at the shared entity receiver boundary. Bypass applies only to
    /// positive damage; heals remain intact. Real invulnerability is untouched.
    pub(crate) fn debug_damage_event(
        &self,
        mut event: crate::sim::combat::EntityDamageEvent,
    ) -> Option<crate::sim::combat::EntityDamageEvent> {
        if event.damage <= 0 {
            return Some(event);
        }
        if self
            .entities()
            .get(event.target_id)
            .is_some_and(|target| self.debug_flags_for(target.owner()).invincible)
        {
            return None;
        }
        let source = event
            .source_house
            .or_else(|| self.entities().get(event.attacker_id).map(|e| e.owner()));
        if let Some(source) = source {
            event.damage = event
                .damage
                .saturating_mul(i32::from(self.debug_flags_for(source).damage_scale.max(1)));
        }
        Some(event)
    }

    pub fn debug_event_logging(&self) -> bool {
        self.debug_tools.event_logging
    }

    pub fn set_debug_event_logging(&mut self, enabled: bool) {
        self.debug_tools.event_logging = enabled && self.trainer_available();
        Self::refresh_debug_event_selection(&mut self.debug_tools, &mut self.substrate);
    }

    fn refresh_debug_event_selection(
        debug_tools: &mut DebugToolsState,
        substrate: &mut ObjectSubstrate,
    ) {
        if !debug_tools.event_logging && debug_tools.logged_entities.is_empty() {
            return;
        }
        let selected: Vec<_> = if debug_tools.event_logging {
            substrate
                .entities
                .values()
                .filter(|e| e.selection.is_selected())
                .take(crate::sim::debug_event_log::DEBUG_EVENT_ENTITY_LIMIT)
                .map(|e| e.stable_id())
                .collect()
        } else {
            Vec::new()
        };
        if selected == debug_tools.logged_entities {
            return;
        }
        let old = std::mem::replace(&mut debug_tools.logged_entities, selected.clone());
        for id in old {
            if !selected.contains(&id)
                && let Some(entity) = substrate.entities.get_mut(id)
            {
                entity.set_debug_logging(false);
            }
        }
        for id in selected {
            if let Some(entity) = substrate.entities.get_mut(id) {
                entity.set_debug_logging(true);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::house_state::HouseState;

    fn simulation() -> (Simulation, InternedId) {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Allies");
        sim.houses
            .register(HouseState::new(owner, 0, None, true, 500, 10));
        sim.session.set_current_house_for_test(Some(owner));
        (sim, owner)
    }

    fn offline() -> (Simulation, InternedId) {
        let (mut sim, owner) = simulation();
        sim.set_debug_session_context(DebugSessionContext::Offline);
        (sim, owner)
    }

    #[test]
    fn trainer_requires_explicit_offline_admission_and_local_house() {
        let (mut sim, _) = simulation();
        let flags = CheatFlags {
            infinite_money: true,
            money_target: 5000,
            ..Default::default()
        };
        for context in [
            DebugSessionContext::Disabled,
            DebugSessionContext::Network,
            DebugSessionContext::Replay,
        ] {
            sim.set_debug_session_context(context);
            assert_eq!(
                sim.set_cheat_flags(flags),
                Err(DebugToolsError::Unavailable)
            );
            assert_eq!(
                sim.request_trainer_action(TrainerAction::HealAll),
                Err(DebugToolsError::Unavailable)
            );
        }
        let (mut sim, _) = offline();
        sim.session.set_current_house_for_test(None);
        assert_eq!(
            sim.set_cheat_flags(flags),
            Err(DebugToolsError::NoLocalHouse)
        );
        assert!(!sim.trainer_modified());
    }

    #[test]
    fn money_floor_is_bounded_and_never_removes_income() {
        let (mut sim, owner) = offline();
        sim.set_cheat_flags(CheatFlags {
            infinite_money: true,
            money_target: i32::MAX,
            damage_scale: u16::MAX,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(sim.cheat_flags().money_target, MAX_MONEY_TARGET);
        assert_eq!(sim.cheat_flags().damage_scale, MAX_DAMAGE_SCALE);
        sim.apply_debug_tools(crate::rules::ruleset::RuleSet::empty_for_test());
        assert_eq!(sim.houses[&owner].economy().credits(), MAX_MONEY_TARGET);
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .economy_mut()
            .add_credits(100);
        sim.apply_debug_tools(crate::rules::ruleset::RuleSet::empty_for_test());
        assert_eq!(
            sim.houses[&owner].economy().credits(),
            MAX_MONEY_TARGET + 100
        );
        sim.set_cheat_flags(CheatFlags::default()).unwrap();
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .economy_mut()
            .spend_money(MAX_MONEY_TARGET);
        sim.apply_debug_tools(crate::rules::ruleset::RuleSet::empty_for_test());
        assert_eq!(sim.houses[&owner].economy().credits(), 100);
        assert!(
            sim.trainer_modified(),
            "turning switches off cannot resume deterministic recording"
        );
    }

    #[test]
    fn context_change_drops_pending_actions_and_power_without_corrupting_totals() {
        let (mut sim, owner) = offline();
        let power = sim
            .power_states
            .map_mut_for_test()
            .entry(owner)
            .or_default();
        power.set_total_output_for_test(100);
        power.set_total_drain_for_test(200);
        power.set_is_low_power_for_test(true);
        sim.set_cheat_flags(CheatFlags {
            infinite_power: true,
            infinite_money: true,
            money_target: 5000,
            ..Default::default()
        })
        .unwrap();
        sim.request_trainer_action(TrainerAction::HealAll).unwrap();
        assert_eq!(sim.power_states[&owner].effective_totals(), (200, 200));
        assert!(!sim.power_states[&owner].low_power());
        assert_eq!(
            (
                sim.power_states[&owner].total_output(),
                sim.power_states[&owner].total_drain()
            ),
            (100, 200)
        );
        sim.set_debug_session_context(DebugSessionContext::Network);
        assert_eq!(sim.cheat_flags(), CheatFlags::default());
        assert_eq!(sim.debug_tools.pending, 0);
        assert!(sim.power_states[&owner].low_power());
        assert_eq!(sim.power_states[&owner].effective_totals(), (100, 200));
        sim.apply_debug_tools(crate::rules::ruleset::RuleSet::empty_for_test());
        assert_eq!(sim.houses[&owner].economy().credits(), 500);
    }

    #[test]
    fn disabled_controls_and_inspection_leave_saved_state_and_hash_unchanged() {
        let (mut sim, owner) = offline();
        let ty = sim.interner.intern("E1");
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            1,
            1,
            1,
            0,
            0,
            owner,
            crate::sim::combat::Health::new(100),
            ty,
            crate::map::entities::EntityCategory::Infantry,
            0,
            1,
            false,
        );
        entity.selection.set_selected_for_test(true);
        sim.substrate.entities.insert(entity);
        let hash = sim.state_hash();
        let before = crate::util::save_codec::serialize(&sim).unwrap();
        sim.set_cheat_flags(CheatFlags::default()).unwrap();
        sim.set_debug_event_logging(true);
        sim.apply_debug_tools(crate::rules::ruleset::RuleSet::empty_for_test());
        assert!(sim.entities().get(1).unwrap().debug_log().is_some());
        assert_eq!(sim.state_hash(), hash);
        assert_eq!(crate::util::save_codec::serialize(&sim).unwrap(), before);
        let restored: Simulation = crate::util::save_codec::deserialize(&before).unwrap();
        assert!(!restored.trainer_available());
        assert!(!restored.debug_event_logging());
        assert!(!restored.trainer_modified());
        assert!(restored.entities().get(1).unwrap().debug_log().is_none());
    }

    #[test]
    fn inspector_enrollment_is_bounded_and_follows_selection() {
        let (mut sim, owner) = offline();
        let ty = sim.interner.intern("E1");
        for id in 1..=40 {
            let mut entity = GameEntity::new_at_frame_zero_for_test(
                id,
                1,
                1,
                0,
                0,
                owner,
                crate::sim::combat::Health::new(100),
                ty,
                crate::map::entities::EntityCategory::Infantry,
                0,
                1,
                false,
            );
            entity.selection.set_selected_for_test(true);
            sim.substrate.entities.insert(entity);
        }
        sim.set_debug_event_logging(true);
        assert_eq!(
            sim.entities()
                .values()
                .filter(|e| e.debug_log().is_some())
                .count(),
            32
        );
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .selection
            .set_selected_for_test(false);
        sim.set_debug_event_logging(true);
        assert!(sim.entities().get(1).unwrap().debug_log().is_none());
        assert!(sim.entities().get(33).unwrap().debug_log().is_some());
        sim.clear_debug_tools();
        assert!(sim.entities().values().all(|e| e.debug_log().is_none()));
    }

    #[test]
    fn replay_batch_revokes_trainer_before_any_queued_work() {
        let (mut sim, owner) = offline();
        sim.set_cheat_flags(CheatFlags {
            infinite_money: true,
            money_target: 5000,
            ..Default::default()
        })
        .unwrap();
        let _ = sim.take_due_replay_commands([]);
        assert!(!sim.trainer_available());
        sim.apply_debug_tools(crate::rules::ruleset::RuleSet::empty_for_test());
        assert_eq!(sim.houses[&owner].economy().credits(), 500);
    }

    #[test]
    fn invincibility_and_damage_scale_apply_at_receiver_without_timer_writes() {
        use crate::sim::combat::{EntityDamageEvent, Health, ReceiverCallFlags};
        use crate::sim::superweapon::invulnerability::{InvulnKind, apply_invulnerability};
        let rules=crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str("[InfantryTypes]\n0=E1\n[E1]\nStrength=100\nArmor=none\n[Warheads]\n0=SA\n[SA]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n")).unwrap();
        let (mut sim, owner) = offline();
        let other = sim.interner.intern("Enemy");
        sim.houses
            .register(HouseState::new(other, 1, None, false, 0, 10));
        let ty = sim.interner.intern("E1");
        let warhead = sim.interner.intern("SA");
        for (id, house) in [(1, owner), (2, other)] {
            sim.substrate
                .entities
                .insert(GameEntity::new_at_frame_zero_for_test(
                    id,
                    1,
                    1,
                    0,
                    0,
                    house,
                    Health::new(100),
                    ty,
                    crate::map::entities::EntityCategory::Infantry,
                    0,
                    1,
                    false,
                ));
        }
        let hit = |target, attacker, source, damage| {
            EntityDamageEvent::direct_receiver(
                target,
                damage,
                0,
                attacker,
                Some(source),
                warhead,
                ReceiverCallFlags {
                    ignore_defenses: false,
                    arg6: false,
                },
            )
        };
        sim.set_cheat_flags(CheatFlags {
            invincible: true,
            damage_scale: 3,
            ..Default::default()
        })
        .unwrap();
        sim.commit_direct_damage_receiver(
            &rules,
            None,
            hit(1, 2, other, 10),
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(sim.entities().get(1).unwrap().health.current(), 100);
        assert!(sim.entities().get(1).unwrap().invulnerability.is_none());
        sim.commit_direct_damage_receiver(
            &rules,
            None,
            hit(2, 1, owner, 10),
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(sim.entities().get(2).unwrap().health.current(), 70);
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        apply_invulnerability(entity, 0, 300, InvulnKind::IronCurtain);
        let original = crate::util::save_codec::serialize(&entity.invulnerability).unwrap();
        sim.apply_debug_tools(&rules);
        sim.set_cheat_flags(CheatFlags::default()).unwrap();
        assert_eq!(
            crate::util::save_codec::serialize(&sim.entities().get(1).unwrap().invulnerability)
                .unwrap(),
            original
        );
    }
    #[test]
    fn replay_context_cannot_be_reopened_by_offline_refresh() {
        let (mut sim, _) = offline();
        sim.set_debug_session_context(DebugSessionContext::Replay);
        sim.clear_debug_tools();
        sim.set_debug_session_context(DebugSessionContext::Offline);
        sim.set_debug_event_logging(true);
        assert!(!sim.trainer_available());
        assert!(!sim.debug_event_logging());
        assert_eq!(
            sim.request_trainer_action(TrainerAction::HealAll),
            Err(DebugToolsError::Unavailable)
        );
    }

    #[test]
    fn heal_and_veterancy_actions_are_owned_and_consumed_once() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\n",
            ))
            .unwrap();
        let (mut sim, owner) = offline();
        let enemy = sim.interner.intern("Enemy");
        let ty = sim.interner.intern("E1");
        for (id, house) in [(1, owner), (2, enemy)] {
            sim.substrate
                .entities
                .insert(GameEntity::new_at_frame_zero_for_test(
                    id,
                    1,
                    1,
                    0,
                    0,
                    house,
                    crate::sim::combat::Health::new(10),
                    ty,
                    crate::map::entities::EntityCategory::Infantry,
                    0,
                    1,
                    false,
                ));
        }
        sim.request_trainer_action(TrainerAction::HealAll).unwrap();
        sim.request_trainer_action(TrainerAction::VeteranAll)
            .unwrap();
        sim.apply_debug_tools(&rules);
        assert_eq!(sim.entities().get(1).unwrap().health.current(), 100);
        assert_eq!(sim.entities().get(1).unwrap().veterancy.rank(), 200);
        assert_eq!(sim.entities().get(2).unwrap().health.current(), 10);
        assert_eq!(sim.entities().get(2).unwrap().veterancy.rank(), 0);
        sim.substrate.entities.get_mut(1).unwrap().health.set(50);
        sim.apply_debug_tools(&rules);
        assert_eq!(
            sim.entities().get(1).unwrap().health.current(),
            50,
            "action was consumed once"
        );
    }

    #[test]
    fn superweapon_action_charges_only_granted_unheld_local_instances() {
        let rules=crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str("[SuperWeaponTypes]\n0=TestSpecial\n[TestSpecial]\nType=ChronoSphere\nRechargeTime=5\n")).unwrap();
        let (mut sim, owner) = offline();
        let ty = sim.interner.intern("TestSpecial");
        let mut instance = crate::sim::superweapon::SuperWeaponInstance::new(ty, owner, 0);
        instance.set_is_active_for_test(true);
        sim.super_weapons
            .map_mut_for_test()
            .entry(owner)
            .or_default()
            .insert(ty, instance);
        sim.request_trainer_action(TrainerAction::SuperweaponsReady)
            .unwrap();
        sim.apply_debug_tools(&rules);
        assert!(sim.super_weapons[&owner][&ty].is_ready());
        let instance = sim
            .super_weapons
            .map_mut_for_test()
            .get_mut(&owner)
            .unwrap()
            .get_mut(&ty)
            .unwrap();
        instance.set_is_ready_for_test(false);
        instance.set_is_suspended_for_test(true);
        sim.request_trainer_action(TrainerAction::SuperweaponsReady)
            .unwrap();
        sim.apply_debug_tools(&rules);
        assert!(!sim.super_weapons[&owner][&ty].is_ready());
    }
    #[test]
    fn healing_a_structure_requests_power_and_radar_refresh_once() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[BuildingTypes]\n0=GAPOWR\n[GAPOWR]\nStrength=100\nPower=200\n",
            ))
            .unwrap();
        let (mut sim, owner) = offline();
        let ty = sim.interner.intern("GAPOWR");
        let entity = GameEntity::new_at_frame_zero_for_test(
            1,
            1,
            1,
            0,
            0,
            owner,
            crate::sim::combat::Health::new(50),
            ty,
            crate::map::entities::EntityCategory::Structure,
            0,
            1,
            false,
        );
        sim.substrate.entities.insert(entity);
        sim.power_states.map_mut_for_test().insert(
            owner,
            crate::sim::power_system::PowerState::settled_for_test(),
        );
        sim.request_trainer_action(TrainerAction::HealAll).unwrap();
        sim.apply_debug_tools(&rules);
        assert_eq!(sim.entities().get(1).unwrap().health.current(), 100);
        assert!(sim.power_states[&owner].power_recheck_for_test());
        assert!(sim.power_states[&owner].radar_recheck_for_test());
        sim.power_states.map_mut_for_test().insert(
            owner,
            crate::sim::power_system::PowerState::settled_for_test(),
        );
        sim.apply_debug_tools(&rules);
        assert!(!sim.power_states[&owner].power_recheck_for_test());
    }

    #[test]
    fn clearing_tech_flags_requests_existing_house_strip_recheck_once() {
        for flags in [
            CheatFlags {
                tech_all: true,
                ..Default::default()
            },
            CheatFlags {
                tech_boost: true,
                ..Default::default()
            },
        ] {
            let (mut sim, owner) = offline();
            sim.set_cheat_flags(flags).unwrap();
            sim.houses.get_mut(&owner).unwrap().take_tech_recheck();
            assert!(!sim.houses[&owner].tech_recheck_pending());
            sim.set_debug_session_context(DebugSessionContext::Network);
            assert!(sim.houses.get_mut(&owner).unwrap().take_tech_recheck());
            sim.clear_debug_tools();
            assert!(!sim.houses[&owner].tech_recheck_pending());
        }
    }
}
