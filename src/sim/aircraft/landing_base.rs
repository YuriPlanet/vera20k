//! Aircraft auxiliary41B6A0: the live landing base in leptons.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::StringInterner;
use crate::sim::mission::{MissionId, MissionType};

/// Fly callers obtain this through Aircraft's auxiliary interface. Other Foot
/// categories fail that query and use zero, even with a Carryall rules field.
/// Cargo, sparse contacts and effective Mission remain their existing owners;
/// this value is recomputed, never stored as an independent altitude target.
/// Native comparisons: tools/spatial_oracle/fly_landing_base.{py,json}.
pub(crate) fn landing_base(
    entity: &GameEntity,
    entities: &EntityStore,
    rules_context: Option<(&RuleSet, &StringInterner)>,
) -> i32 {
    if entity.category != EntityCategory::Aircraft {
        return 0;
    }
    let Some((rules, interner)) = rules_context else {
        return 0;
    };
    if !rules
        .object(interner.resolve(entity.type_ref()))
        .is_some_and(|object| object.carryall)
    {
        return 0;
    }
    let loaded = entity
        .passenger_role
        .cargo()
        .is_some_and(|cargo| cargo.count() != 0);
    if !loaded && entity.mission.effective() == MissionId::from_known(MissionType::Enter) {
        //65AD40 reads raw slot0, not the first live contact. A hole must not
        // promote another dock to this special ground-level landing case.
        let docking_at_ground = entity
            .radio_contacts
            .slot(0)
            .and_then(|id| entities.get(id))
            .filter(|contact| contact.category == EntityCategory::Structure)
            .and_then(|contact| rules.object(interner.resolve(contact.type_ref())))
            .is_some_and(|object| object.unit_repair || object.helipad);
        if docking_at_ground {
            return 0;
        }
    }
    //65AE30 scans every slot. It does not require the contact to be a building.
    if loaded || !entity.radio_contacts.is_empty() {
        100
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn carryall_reader_matches_original() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/fly_landing_base.json"
        ))
        .unwrap();
        let rows = vectors["rules"].as_array().unwrap();
        assert_eq!(rows.len(), 9);
        for row in rows {
            let field = row["raw"]
                .as_str()
                .map_or(String::new(), |raw| format!("Carryall={raw}\n"));
            let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
                "[AircraftTypes]\n0=TEST\n[TEST]\nStrength=100\n{field}"
            )))
            .unwrap();
            assert_eq!(
                rules.object("TEST").unwrap().carryall,
                row["carryall"].as_bool().unwrap(),
                "{row}"
            );
        }
    }
}
