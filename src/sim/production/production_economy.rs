//! Resource economy: the harvester type query and legacy ore-node test
//! utilities. Harvesting itself runs from the per-object AI (the Harvest
//! mission handlers and the slave manager).

use crate::rules::ruleset::RuleSet;

pub fn is_harvester_type(rules: &RuleSet, type_id: &str) -> bool {
    rules
        .object_case_insensitive(type_id)
        .is_some_and(|obj| obj.harvester)
}
