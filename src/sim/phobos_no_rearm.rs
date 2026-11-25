//! Phobos `TechnoExt::UpdateRearmInEMPState`
//! (`Ext/Techno/Body.Update.cpp:866-880`) and `TechnoExt::UpdateRearmInTemporal`
//! (`:882-895`): the `NoRearm.*` / `NoReload.*` timer holds.
//!
//! Each visit bumps a running timer's start frame once (`StartTime++`) while
//! its key holds, so a continuous condition keeps the remaining time still.
//! The timers themselves stay ordinary countdowns: `rearm_timer` belongs to
//! `combat::rof` and the Techno `ReloadTimer` to `game_entity`'s ammo owner.
//!
//! Never depends on render/ui/audio/net (sim invariant #1).

use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

impl Simulation {
    /// `TechnoExt::UpdateRearmInEMPState`, called per techno visit from
    /// `TechnoExt::OnEarlyUpdate` (`:23`) unless the visit is Temporal-frozen
    /// (native's frozen leaves skip `TechnoClass_AI` entirely, which is why
    /// the Temporal arm exists). Native reads `IsUnderEMP()` and
    /// `Deactivated` — the [`GameEntity::emp_remaining`] and
    /// [`GameEntity::deactivated`] owners — and returns when neither holds.
    pub(crate) fn update_rearm_in_emp_state(&mut self, id: u64, rules: &RuleSet) {
        let under_emp_or_deactivated =
            self.substrate.entities.get(id).is_some_and(|entity| {
                entity.emp_remaining() > 0 || entity.deactivated.is_offline()
            });
        if !under_emp_or_deactivated {
            return;
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let keys = object.phobos_no_rearm;
        let globals = &rules.phobos_globals;
        let entity = self.substrate.entities.get_mut(id).unwrap();
        if entity.seldom.rearm_timer.timer().in_progress()
            && keys.rearm_under_emp(globals.no_rearm_under_emp)
        {
            entity.seldom.rearm_timer.delay_one_frame();
        }
        if entity.ammo_reload_timer().in_progress()
            && keys.reload_under_emp(globals.no_reload_under_emp)
        {
            entity.delay_ammo_reload_one_frame();
        }
    }

    /// `TechnoExt::UpdateRearmInTemporal` on a Temporal weapon's held target,
    /// reached from `TechnoExt::UpdateTemporal` (`:853-863`) inside the
    /// `TemporalClass_AI @ 0x71A88D` hook while the warp runs. No predicate
    /// beyond the keys: the target's type decides.
    pub(crate) fn update_rearm_in_temporal(&mut self, target: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(target) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let keys = object.phobos_no_rearm;
        let globals = &rules.phobos_globals;
        let entity = self.substrate.entities.get_mut(target).unwrap();
        if entity.seldom.rearm_timer.timer().in_progress()
            && keys.rearm_temporal(globals.no_rearm_temporal)
        {
            entity.seldom.rearm_timer.delay_one_frame();
        }
        if entity.ammo_reload_timer().in_progress()
            && keys.reload_temporal(globals.no_reload_temporal)
        {
            entity.delay_ammo_reload_one_frame();
        }
    }
}
