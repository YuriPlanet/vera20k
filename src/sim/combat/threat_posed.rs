//! The shared live threat getter used by acquisition and base response.
//!
//! Native identity: TechnoClass::Get_ThreatPosed708B40 (vt+2C0).
//! Executable comparisons: tools/spatial_oracle/base_defense_response.{json,md}.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::StringInterner;

/// Read the existing type, cargo and reciprocal bunker owners. Native checks
/// the receiver's type first, then Building occupant count, then Building+2E4
/// (installed tank), and finally the receiver type's ThreatPosed+670.
pub(crate) fn live_threat_posed(
    candidate: &GameEntity,
    candidate_type: Option<&ObjectType>,
    entities: &EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
) -> i32 {
    let Some(candidate_type) = candidate_type else {
        return 0;
    };
    if candidate.category() == EntityCategory::Structure {
        let occupants = candidate
            .passenger_role
            .cargo()
            .map_or(0, |cargo| cargo.count() as i32);
        if occupants > 0 {
            return occupants.wrapping_mul(rules.general.threat_per_occupant);
        }
        if let Some(tank_id) = candidate.bunker_occupant {
            // Native reads the linked tank's TYPE field directly; do not
            // recurse through this getter or add another reciprocal-link gate.
            return entities
                .get(tank_id)
                .and_then(|tank| rules.object(interner.resolve(tank.type_ref())))
                .map_or(0, |tank_type| tank_type.threat_posed);
        }
    }
    candidate_type.threat_posed
}
