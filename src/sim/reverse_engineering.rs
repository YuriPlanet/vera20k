//! Ares 3.0 reverse engineering. A victim ground at a
//! `ReverseEngineersVictims=` grinder records its type, or its
//! `ReversedAs=` type, with the grinder owner's House (HouseExt `+0x34`,
//! [`crate::sim::house_state::HouseState::record_ares_reverse_engineered`]).
//! That House then builds the type as though its requirements were met:
//! RequirementsMet (`10021FF0`) answers Overridden for a listed type, ahead
//! of PrerequisiteOverride and the Secret Lab (`production::can_build`).
//! Trigger event 78 reads the list (`sim::trigger_runtime_ares`), and a spy's
//! `SpyEffect.UndoReverseEngineer` empties it (`sim::spy_infiltration`).
//!
//! The two hooks sit in the PerCellProcess grinder arms, after the Eaten
//! arrival's refunds and Tag event 48 and before `EnterGrinderSound`
//! ([`Simulation::foot_per_cell_eaten`]):
//! - InfantryClass_UpdatePosition_EnteredGrinder (Ares.dll 100498D0, hook
//!   0x5198AD) and UnitClass_UpdatePosition_EnteredGrinder (1004C460, hook
//!   0x73A1BC) reverse engineer; when that recorded something, the victim
//!   owner's player hears `EVA_ReverseEngineeredInfantry` (or `...Vehicle`)
//!   and `EVA_NewTechnologyAcquired`, then the grinder's Tag raises Ares
//!   event 80 (this type, the victim as source) and 79 (anything).
//! - The vehicle arm then credits the grinder's owner its hijacker's refund
//!   (#368: the captor InfantryType, Techno `+0x338`, refunded to the
//!   vehicle's owner, `GetRefund` type vt+0xB8).
//!
//! No arm draws random numbers, writes a timer or detaches anything; the
//! Tag events may run trigger actions.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::TechnoTypeReference;
use crate::sim::world::{SimSoundEvent, Simulation, UninitContext};

#[cfg(test)]
#[path = "reverse_engineering_tests.rs"]
mod tests;

impl Simulation {
    /// The grinder hooks' body (Ares.dll 100498D0, hook 0x5198AD; 1004C460,
    /// hook 0x73A1BC) for `victim` entering the `Grinding=` building
    /// `grinder`.
    pub(crate) fn ares_entered_grinder(
        &mut self,
        victim: u64,
        grinder: u64,
        rules: &RuleSet,
        context: UninitContext<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(victim) else {
            return;
        };
        let victim_owner = entity.owner();
        let vehicle = entity.category() == EntityCategory::Unit;
        let Some(victim_type) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        if self.ares_reverse_engineer(grinder, victim_type, rules) {
            // The victim's owner, ControlledByPlayer.
            if self.house_is_human_player(victim_owner) {
                for event in [
                    if vehicle {
                        "EVA_ReverseEngineeredVehicle"
                    } else {
                        "EVA_ReverseEngineeredInfantry"
                    },
                    "EVA_NewTechnologyAcquired",
                ] {
                    self.sound_events.emit(SimSoundEvent::HouseEva {
                        owner: victim_owner,
                        event,
                    });
                }
            }
            // RaiseEvent(0x50, grinder, -, false, victim), then 0x4F with
            // no source while the grinder still has a Tag.
            self.dispatch_object_tag_event(grinder, 80, Some(grinder), Some(victim), context);
            self.dispatch_object_tag_event(grinder, 79, Some(grinder), None, context);
        }
        if vehicle {
            self.refund_ground_hijacker(victim, grinder, rules);
        }
    }

    /// BuildingExt::ExtData::ReverseEngineer (Ares.dll 10012E00): only a
    /// `ReverseEngineersVictims=` building reverse engineers, for its owner,
    /// through HouseExt::ExtData::ReverseEngineer (10022360): a
    /// `CanBeReversed=` victim records `ReversedAs=` (the victim's own type
    /// when unset) unless the House lists it already. When the type was not
    /// buildable before (PrereqValidate 10021B00, `can_build` with requirements
    /// and queued counts) House+1FC is set when RequirementsMet (10021FF0),
    /// asked after the record, answers at least Incomplete: a theater outside
    /// `Prerequisite.RequiredTheaters` (Unbuildable) or an owned
    /// `Prerequisite.Negative` entry (Forbidden) answer lower. Answers whether
    /// the type was newly recorded.
    fn ares_reverse_engineer(
        &mut self,
        grinder: u64,
        victim_type: &ObjectType,
        rules: &RuleSet,
    ) -> bool {
        let Some(building) = self.substrate.entities.get(grinder) else {
            return false;
        };
        let owner = building.owner();
        if !self
            .object_type(building.type_ref(), rules)
            .is_some_and(|object| object.building_type_ext().reverse_engineers_victims())
        {
            return false;
        }
        let (can_be_reversed, reversed_as) = victim_type.techno_combat().reverse_engineering();
        if !can_be_reversed {
            return false;
        }
        let reversed = reversed_as.cloned().unwrap_or_else(|| TechnoTypeReference {
            category: victim_type.category,
            id: victim_type.id.clone(),
        });
        if self
            .houses
            .get(&owner)
            .is_none_or(|house| house.has_ares_reverse_engineered(reversed.category, &reversed.id))
        {
            return false;
        }
        let was_buildable = match rules.object_in_category(reversed.category, &reversed.id) {
            Some(object) => {
                crate::sim::production::can_build(self, rules, owner, object, false, true)
                    == crate::sim::production::CanBuild::Yes
            }
            None => false,
        };
        let Some(house) = self.houses.get_mut(&owner) else {
            return false;
        };
        house.record_ares_reverse_engineered(&reversed);
        // 10022360 tests `RequirementsMet(type) >= 2` before House+1FC.
        let recheck = !was_buildable
            && rules
                .object_in_category(reversed.category, &reversed.id)
                .is_none_or(|object| {
                    crate::sim::production::requirements_met(
                        self,
                        rules,
                        owner,
                        &self.houses[&owner],
                        object,
                    ) >= crate::sim::production::RequirementStatus::Incomplete
                });
        if recheck && let Some(house) = self.houses.get_mut(&owner) {
            house.request_tech_recheck();
        }
        true
    }

    /// UnitClass_UpdatePosition_EnteredGrinder's hijacker refund (#368): a
    /// vehicle whose captor InfantryType index (Techno `+0x338`) is not
    /// negative credits the grinder's owner that type's refund to the
    /// vehicle's owner (`GetRefund(owner, 0)`).
    fn refund_ground_hijacker(&mut self, vehicle: u64, grinder: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(vehicle) else {
            return;
        };
        let captor = entity.capture_infantry_type_index.get();
        let owner = entity.owner();
        if captor < 0 {
            return;
        }
        let Some(hijacker) = rules.type_array_at(ObjectCategory::Infantry, captor) else {
            return;
        };
        let (Some(house), Some(factors)) = (
            self.houses.get(&owner),
            Simulation::house_cost_factors(&self.houses, owner),
        ) else {
            return;
        };
        let refund = crate::sim::production::type_refund(
            rules,
            hijacker,
            house,
            &factors,
            self.session.game_mode_nonzero(),
            false,
        );
        if let Some(receiver) = self.substrate.entities.get(grinder).map(|b| b.owner()) {
            crate::sim::credit_income::add_credits(&mut self.houses, receiver, refund);
        }
    }
}
