//! The live `TechnoType+634` value, shared by every House and consumer.
//!
//! RuleSet keeps the completed reader projection. Changes made during a match
//! live here until scene retirement transfers them back to the Rules process
//! owner, before its optional prefix read and destructive Type reset.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::snapshot::SnapshotRestoreError;

use super::Simulation;

const CATEGORIES: [ObjectCategory; 4] = [
    ObjectCategory::Infantry,
    ObjectCategory::Vehicle,
    ObjectCategory::Aircraft,
    ObjectCategory::Building,
];

/// Sparse current values over the immutable, scenario-bound reader output.
/// Keys retain category and exact constructor spelling, rather than the broad
/// case-insensitive name lookup. Maps give canonical save/hash order and allow
/// allocation-free reads; only the trigger and retirement owners mutate them.
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct TypeTechLevels {
    changed: [BTreeMap<String, i32>; 4],
}

impl TypeTechLevels {
    fn category_index(category: ObjectCategory) -> usize {
        match category {
            ObjectCategory::Infantry => 0,
            ObjectCategory::Vehicle => 1,
            ObjectCategory::Aircraft => 2,
            ObjectCategory::Building => 3,
        }
    }

    pub(crate) fn get(&self, object: &ObjectType) -> i32 {
        self.changed[Self::category_index(object.category)]
            .get(object.id.as_str())
            .copied()
            .unwrap_or(object.tech_level)
    }

    fn set(&mut self, object: &ObjectType, value: i32) {
        let values = &mut self.changed[Self::category_index(object.category)];
        // TechLevel is a value, not Super's inherit sentinel: writing the
        // reader value leaves no distinguishable runtime state to retain.
        if value == object.tech_level {
            values.remove(object.id.as_str());
        } else {
            values.insert(object.id.clone(), value);
        }
    }
}

impl Simulation {
    /// Current shared `TechnoType+634`, including an authored Action106 write.
    /// Presentation and gameplay ask the same owner; neither mutates RuleSet.
    pub fn object_tech_level(&self, object: &ObjectType) -> i32 {
        self.type_tech_levels.get(object)
    }

    /// Declared receiver fields of the native dispatch fixture. This supplies
    /// category-distinct priors directly, including equal IDs in different
    /// arrays; it does not stand in for executing the action under test.
    #[cfg(test)]
    pub(crate) fn supply_object_tech_level_for_test(&mut self, object: &ObjectType, value: i32) {
        self.type_tech_levels.set(object, value);
    }

    /// Active Execute106 inline body6DF5D1: reverse global type order,
    /// exact stored ID, signed raw value, then reverse registered Houses dirty.
    /// No incoming House admission, RNG, timer or detach occurs in this leaf.
    /// Original Steam identity/execution: trigger_object_tech_level.meta.json.
    pub(crate) fn set_object_type_tech_level(&mut self, name: &str, value: i32, rules: &RuleSet) {
        if let Some(object) = rules.native_techno_type_exact(name) {
            self.type_tech_levels.set(object, value);
        }
        for house in self.houses.in_order_mut().rev() {
            house.request_tech_recheck();
        }
    }

    /// The outgoing scene has stopped. Move its changes to the process Rules
    /// owner before the pre-reset prefix read. Full_Init's existing destructive
    /// reset subsequently deletes these types; names are not transplanted into
    /// the new scene. An active scene must not call this retirement operation.
    pub fn retire_object_type_tech_levels(&mut self) -> Vec<(ObjectCategory, String, i32)> {
        std::mem::take(&mut self.type_tech_levels.changed)
            .into_iter()
            .zip(CATEGORIES)
            .flat_map(|(values, category)| {
                values
                    .into_iter()
                    .map(move |(id, value)| (category, id, value))
            })
            .collect()
    }

    pub(super) fn hash_type_tech_levels(&self, hasher: &mut impl Hasher) {
        if self
            .type_tech_levels
            .changed
            .iter()
            .any(|values| !values.is_empty())
        {
            b"type-tech-levels-v1".hash(hasher);
            self.type_tech_levels.changed.hash(hasher);
        }
    }

    pub(crate) fn validate_type_tech_levels(
        &self,
        rules: &RuleSet,
    ) -> Result<(), SnapshotRestoreError> {
        for (values, category) in self.type_tech_levels.changed.iter().zip(CATEGORIES) {
            for id in values.keys() {
                if !rules
                    .object_in_category(category, id)
                    .is_some_and(|object| object.id == *id)
                {
                    return Err(SnapshotRestoreError::InvalidTypeTechLevels {
                        reason: format!("unresolved {category:?} constructor identity {id:?}"),
                    });
                }
            }
        }
        Ok(())
    }
}
