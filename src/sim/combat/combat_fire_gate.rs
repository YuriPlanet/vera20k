//! Shared admission for whether an object's attack routine runs this frame.
//! Unit/Infantry live slots query it directly; remaining class hosts collect a blocked
//! set before their combat snapshot loop.
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
        if fire_blocked(entity) {
            blocked.insert(entity.stable_id());
        }
    }

    blocked
}

/// The same owner predicate at an individual object's live fire slot.
pub(crate) fn fire_blocked(entity: &crate::sim::game_entity::GameEntity) -> bool {
    // Rockets are projectiles, not weapon-bearing units — never fire.
    if entity.locomotor.as_ref().is_some_and(|locomotor| {
        locomotor.active_kind() == crate::rules::locomotor_type::LocomotorKind::Rocket
    }) {
        return true;
    }

    // An empty aircraft (Ammo `+0x2FC` exactly zero; -1 is unlimited)
    // outside a Mission_Attack visit: GetFireError's T47 (`0x006FCA0D`)
    // refuses it anyway, and skipping the routine keeps the generic
    // retarget from handing it a new target. A visit its dispatch asked
    // for runs regardless; state 4's prefix (`0x004182A3`) is its own.
    if let Some(ref ammo) = entity.aircraft_ammo
        && ammo.current == 0
    {
        return true;
    }

    // Attack dispatch is admitted by a call-local mission receipt in the
    // combat host. Docked aircraft cannot fire.
    if let Some(ref mission) = entity.aircraft_mission
        && mission.is_docked_idle()
    {
        return true;
    }

    // A building still playing its build-up runs its Construction
    // mission, not Mission_Attack.
    if entity.building_up() {
        return true;
    }
    false
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
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/aircraft_attack_release.json",
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
