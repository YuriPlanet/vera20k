//! Fire-gating — prevents units from firing during special locomotor states
//! or when a building is disabled by low power.
//!
//! RA2's `ILocomotion::Can_Fire()` prevents weapons from firing during certain
//! movement phases (teleport warp, rocket flight).
//! Additionally, `Powered=yes` defense buildings cannot fire when the owner
//! is in low-power state.
//!
//! This module provides a pre-scan that collects fire-blocked entities before
//! the combat snapshot loop, avoiding borrow conflicts with `query_mut`.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/ movement state components, power_system.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use std::collections::{BTreeMap, BTreeSet};

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::movement::teleport_movement::TeleportPhase;
use crate::sim::power_system::{self, PowerState};

/// Collect stable IDs of entities that are currently blocked from firing weapons.
///
/// Checks locomotor state (teleport/rocket) and power state
/// (Powered=yes buildings disabled during low power).
pub fn collect_fire_blocked_entities(
    entities: &EntityStore,
    power_states: &BTreeMap<InternedId, PowerState>,
    rules: Option<&RuleSet>,
    interner: &StringInterner,
) -> BTreeSet<u64> {
    let mut blocked: BTreeSet<u64> = BTreeSet::new();

    for entity in entities.values() {
        // Teleporting units cannot fire during the instant relocation tick.
        // ChronoDelay allows fire — the unit has arrived and is materializing.
        if let Some(ref state) = entity.teleport_state {
            match state.phase {
                TeleportPhase::Relocate => {
                    blocked.insert(entity.stable_id());
                    continue;
                }
                TeleportPhase::ChronoDelay => {}
            }
        }

        // Rockets are projectiles, not weapon-bearing units — never fire.
        if entity.rocket_state.is_some() {
            blocked.insert(entity.stable_id());
            continue;
        }

        // Aircraft with 0 ammo cannot fire — must reload at an airfield first.
        if let Some(ref ammo) = entity.aircraft_ammo {
            // Techno::GetFireError6FCA0D rejects exactly zero, not negative Ammo.
            if ammo.current == 0 {
                blocked.insert(entity.stable_id());
                continue;
            }
        }

        // Open migration: the aircraft mission currently issues only a legacy
        // request, not a FireAt call. Keep its exclusion explicit until the
        // admitted burst caller and pending-ammo writer are connected together;
        // removing this gate alone would run generic burst/ammo bookkeeping.
        // Docked-idle aircraft are parked on helipad — don't fire.
        if let Some(ref mission) = entity.aircraft_mission {
            if mission.is_attacking() || mission.is_docked_idle() {
                blocked.insert(entity.stable_id());
                continue;
            }
        }

        // Buildings still deploying cannot fire.
        if entity.building_up.is_some() {
            blocked.insert(entity.stable_id());
            continue;
        }

        // Powered=yes defense buildings cannot fire when the owner is in low power.
        if entity.category == EntityCategory::Structure {
            if let Some(rules) = rules {
                if !power_system::is_building_powered(power_states, rules, entity, interner) {
                    blocked.insert(entity.stable_id());
                    continue;
                }
            }
        }

        // Garrison fire gate: CanBeOccupied buildings with no occupants cannot fire.
        // gamemd FUN_007091D0: even if the building has its own weapons, an empty
        // garrisonable building is defenseless.
        if entity.category == EntityCategory::Structure {
            if let Some(rules) = rules {
                if let Some(obj) = rules.object(interner.resolve(entity.type_ref())) {
                    if obj.can_be_occupied
                        && entity.passenger_role.cargo().map_or(true, |c| c.is_empty())
                    {
                        blocked.insert(entity.stable_id());
                    }
                }
            }
        }
    }

    blocked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::StringInterner;
    use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};

    fn make_entity(id: u64) -> GameEntity {
        GameEntity::test_default(id, "MTNK", "Americans", 5, 5)
    }

    /// Empty power states and no rules — locomotor-only gating.
    fn no_power() -> (BTreeMap<InternedId, PowerState>, Option<&'static RuleSet>) {
        (BTreeMap::new(), None)
    }

    fn test_interner() -> StringInterner {
        let mut interner = StringInterner::new();
        interner.intern("MTNK");
        interner.intern("Americans");
        interner
    }

    #[test]
    fn test_teleport_relocate_blocks_fire() {
        let mut store = EntityStore::new();
        let mut e = make_entity(1);
        e.teleport_state = Some(TeleportState {
            phase: TeleportPhase::Relocate,
            target_rx: 20,
            target_ry: 20,
            being_warped_ticks: 16,
        });
        store.insert(e);
        let (ps, r) = no_power();
        let interner = test_interner();
        let blocked = collect_fire_blocked_entities(&store, &ps, r, &interner);
        assert!(blocked.contains(&1), "Relocate should block firing");
    }

    #[test]
    fn test_teleport_chrono_delay_allows_fire() {
        let mut store = EntityStore::new();
        let mut e = make_entity(1);
        e.teleport_state = Some(TeleportState {
            phase: TeleportPhase::ChronoDelay,
            target_rx: 20,
            target_ry: 20,
            being_warped_ticks: 10,
        });
        store.insert(e);
        let (ps, r) = no_power();
        let interner = test_interner();
        let blocked = collect_fire_blocked_entities(&store, &ps, r, &interner);
        assert!(!blocked.contains(&1), "ChronoDelay should allow firing");
    }

    #[test]
    fn test_entity_without_special_state_can_fire() {
        let mut store = EntityStore::new();
        store.insert(make_entity(1));
        let (ps, r) = no_power();
        let interner = test_interner();
        let blocked = collect_fire_blocked_entities(&store, &ps, r, &interner);
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
            let (power, rules) = no_power();
            let blocked = collect_fire_blocked_entities(&store, &power, rules, &test_interner());
            assert_eq!(
                blocked.contains(&1),
                row["blocked"].as_bool().unwrap(),
                "{row}"
            );
        }
    }
}
