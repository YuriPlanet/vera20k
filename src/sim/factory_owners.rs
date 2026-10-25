//! Ares3.0 factory-source plans: the retained Country of a Techno and the
//! House's permanent plans remain with their existing state owners. This
//! module joins their lifecycle producers and the shared production decision.
//!
//! Native: Techno.Init1004A776, conversion10045B54, grant100449D0 and
//! qualification100221A0..10022325. Source and pinned binary identities:
//! `tools/rules_oracle/ares_factory_owners.md`. These branches draw no RNG or write timers;
//! their enclosing constructor/opening/conversion owners retain those effects.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::{FactoryType, ObjectType};
use crate::rules::ruleset::{CountryIdx, RuleSet};
use crate::sim::house_state::HouseState;
use crate::sim::world::Simulation;

///1004A776 and100244C9 use a single direct ParentCountry Find, not recursive
///inheritance, aliases, or the current owner's bit mask. Unknown parent uses self.
pub(crate) fn parent_or_self_country(rules: &RuleSet, country: &str) -> Option<CountryIdx> {
    rules
        .country_parent_name(country)
        .and_then(|parent| rules.ares_find_country(parent))
        .or_else(|| rules.country_index(country))
}

impl Simulation {
    /// Country indexes are stable within the restored Rules registry. Resolve
    /// every saved reference before gameplay resumes, as native swizzling does.
    pub(crate) fn validate_factory_country_references(
        &self,
        rules: &RuleSet,
    ) -> Result<(), &'static str> {
        for entity in self.substrate.entities.values() {
            if entity
                .original_factory_country()
                .is_some_and(|country| rules.country_name(country).is_none())
            {
                return Err("Techno original Country is absent from Rules");
            }
        }
        for house in self.houses.values() {
            if house
                .permanent_factory_plans()
                .iter()
                .any(|&country| rules.country_name(country).is_none())
            {
                return Err("House permanent plan Country is absent from Rules");
            }
        }
        Ok(())
    }

    ///10048E90 ->10045A80 copies TechnoExt+40, including null. Called at the
    ///existing successful MCV/slave-manager transfer point, never on capture.
    pub(crate) fn transfer_factory_country(&mut self, source: u64, target: u64) {
        let Some(country) = self
            .substrate
            .entities
            .get(source)
            .map(|entity| entity.original_factory_country())
        else {
            return;
        };
        if let Some(target) = self.substrate.entities.get_mut(target) {
            target.initialize_factory_country(country);
        }
    }

    ///100449D0 scans the base Type and all three installed upgrade pointers;
    ///one Permanent flag grants the host's preserved Country to its current House.
    pub(crate) fn grant_factory_plans(&mut self, id: u64, rules: &RuleSet) {
        let Some(building) = self.substrate.entities.get(id) else {
            return;
        };
        if building.category() != EntityCategory::Structure {
            return;
        }
        let Some(country) = building.original_factory_country() else {
            return;
        };
        let grants = std::iter::once(building.type_ref())
            .chain(
                building
                    .building_upgrades()
                    .types()
                    .iter()
                    .flatten()
                    .copied(),
            )
            .any(|type_id| {
                self.object_type(type_id, rules)
                    .is_some_and(ObjectType::factory_owners_permanent)
            });
        if grants && let Some(house) = self.houses.get_mut(&building.owner()) {
            house.acquire_factory_plan(country);
        }
    }
}

#[cfg(test)]
#[path = "factory_owners_tests.rs"]
mod tests;

/// The qualification suffix only. Success here does not imply that FindFactory,
///prerequisites or the build limit permit production. All consumers call CanBuild.
pub(crate) fn qualifies(
    sim: &Simulation,
    rules: &RuleSet,
    house: &HouseState,
    target: &ObjectType,
) -> bool {
    let allowed = target.factory_owners();
    let forbidden = target.forbidden_factory_owners();
    if allowed.is_empty() && forbidden.is_empty() {
        return true;
    }
    // Native compares pointers: a null original Country can pass a forbidden-
    //only list, but can never match an explicit allowed Country.
    let accepts = |country: Option<CountryIdx>| {
        (allowed.is_empty() || country.is_some_and(|country| allowed.contains(&country)))
            && !country.is_some_and(|country| forbidden.contains(&country))
    };
    if house
        .permanent_factory_plans()
        .iter()
        .copied()
        .map(Some)
        .any(accepts)
    {
        return true;
    }
    //10022258 walks House.Buildings, without the later factory availability
    //gates (power, limbo, selling, Naval). HasAllPlans belongs to the base Type.
    house.base_projection().buildings().iter().any(|&id| {
        sim.substrate.entities.get(id).is_some_and(|building| {
            accepts(building.original_factory_country())
                && sim
                    .object_type(building.type_ref(), rules)
                    .is_some_and(|ty| {
                        ty.factory_has_all_plans()
                            || ty.factory_type() == Some(FactoryType::for_category(target.category))
                    })
        })
    })
}
