//! The objects whose attack routine does not run this frame, collected before
//! the combat snapshot loop so the loop can skip them.
//!
//! Every refusal that is a GetFireError code (a teleport's warp, low power,
//! an empty garrison) is decided by `fire_error` and acted on by the firer's
//! class; what remains here is the attack routine not running at all.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/ movement state components.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use std::collections::BTreeSet;

use crate::sim::entity_store::EntityStore;

/// Collect stable IDs of entities whose attack routine does not run.
pub fn collect_fire_blocked_entities(entities: &EntityStore) -> BTreeSet<u64> {
    let mut blocked: BTreeSet<u64> = BTreeSet::new();

    for entity in entities.values() {
        // Rockets are projectiles, not weapon-bearing units — never fire.
        if entity.rocket_state.is_some() {
            blocked.insert(entity.stable_id());
            continue;
        }

        // `AircraftClass::Mission_Attack` state 4 (`0x004182A3`) moves to
        // state 10 before asking GetFireError when Ammo (`+0x2FC`) is exactly
        // zero; -1 is unlimited.
        if let Some(ref ammo) = entity.aircraft_ammo
            && ammo.current == 0
        {
            blocked.insert(entity.stable_id());
            continue;
        }

        // Attack dispatch is admitted by a call-local mission receipt in the
        // combat host. Docked aircraft cannot fire.
        if let Some(ref mission) = entity.aircraft_mission
            && mission.is_docked_idle()
        {
            blocked.insert(entity.stable_id());
            continue;
        }

        // A building still playing its build-up runs its Construction
        // mission, not Mission_Attack.
        if entity.building_up.is_some() {
            blocked.insert(entity.stable_id());
            continue;
        }
    }

    blocked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::sim::game_entity::GameEntity;

    fn make_entity(id: u64) -> GameEntity {
        GameEntity::test_default(id, "MTNK", "Americans", 5, 5)
    }

    #[test]
    fn test_entity_without_special_state_can_fire() {
        let mut store = EntityStore::new();
        store.insert(make_entity(1));
        let blocked = collect_fire_blocked_entities(&store);
        assert!(
            !blocked.contains(&1),
            "Normal entity should be able to fire"
        );
    }

    #[test]
    fn aircraft_signed_ammo_gate_matches_original_fire_error_prefix() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/aircraft_attack_release.json"
        ))
        .unwrap();
        let rows = corpus["fire_error_ammo"].as_array().unwrap();
        assert_eq!(rows.len(), 7);
        for row in rows {
            let mut store = EntityStore::new();
            let mut entity = make_entity(1);
            entity.category = EntityCategory::Aircraft;
            let mut ammo = crate::sim::docking::aircraft_dock::AircraftAmmo::new(-1);
            ammo.current = row["ammo"].as_i64().unwrap() as i32;
            entity.aircraft_ammo = Some(ammo);
            store.insert(entity);
            let blocked = collect_fire_blocked_entities(&store);
            assert_eq!(
                blocked.contains(&1),
                row["blocked"].as_bool().unwrap(),
                "{row}"
            );
        }
    }
}
